//! Item charges (BC's Item Charge, Item Charge Assignment (Purch) / (Sales), Suggest Item Charge
//! Assignment, Get Receipt Lines / Get Shipment Lines, ItemChargeAssgnt posting): a charge line is
//! shared over item lines of the document or of posted receipts / shipments. A purchase charge's
//! shares go onto the item's inventory and the receiving item ledger entry as cost (so Adjust
//! Cost carries them to what was sold); a sales charge's shares go onto the shipped entries as
//! sales amount.

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::finance::{PURCH_RCPT_LINE, SALES_SHIPMENT_LINE};
use crate::schema::tables::{ITEM, ITEM_LEDGER, PURCH_LINE, SALES_LINE};

pub const ITEM_CHARGE: &str = "Item Charge";
pub const ASSIGNMENT: &str = "Item Charge Assignment (Purch)";
pub const ASSIGNMENT_SALES: &str = "Item Charge Assignment (Sales)";

/// The tables of a side: (assignment, document line, posted line, its Applies-to Doc. Type).
fn tables(sales: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    if sales { (ASSIGNMENT_SALES, SALES_LINE, SALES_SHIPMENT_LINE, "Shipment") } else { (ASSIGNMENT, PURCH_LINE, PURCH_RCPT_LINE, "Receipt") }
}
pub const APPLIES_TO_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", "Receipt", "Transfer Receipt", "Return Shipment", "Sales Shipment", "Return Receipt"];

pub fn assignments(ctx: &Ctx, sales: bool, doc_type: &str, doc_no: &str, line_no: i64) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(tables(sales).0)
        .filter(|a| a.str("Document Type") == doc_type && a.str("Document No.") == doc_no && a.get("Document Line No.").as_int() == line_no)
        .cloned()
        .collect();
    v.sort_by_key(|a| a.get("Line No.").as_int());
    v
}

/// The item ledger entry an assignment's cost lands on: the receipt line's, or (an item line of
/// the same document) the entry its posting made / its order's latest receipt made.
pub fn target_entry(ctx: &Ctx, sales: bool, a: &Record, posted_now: &[(i64, i64)]) -> Option<i64> {
    let line = a.get("Applies-to Doc. Line No.").as_int();
    let (_, _, posted, posted_type) = tables(sales);
    match a.str("Applies-to Doc. Type") {
        t if t == posted_type => ctx
            .db
            .get(posted, &Key(vec![a.get("Applies-to Doc. No.").clone(), Value::Int(line)]))
            .map(|r| r.get("Item Ledger Entry No.").as_int())
            .filter(|n| *n != 0),
        _ => posted_now.iter().find(|(l, _)| *l == line).map(|(_, e)| *e).or_else(|| {
            ctx.db
                .rows(posted)
                .filter(|r| r.str("Order No.") == a.str("Applies-to Doc. No.") && r.get("Order Line No.").as_int() == line)
                .map(|r| r.get("Item Ledger Entry No.").as_int())
                .max()
        }),
    }
}

/// Puts `amount` (LCY) of `charge` on an item ledger entry: a value entry, the entry's cost and
/// the item's unit cost (BC's charge value entry; the receiving entry's quantity carries it).
pub fn add_cost(ctx: &mut Ctx, ile_no: i64, charge: &str, amount: Decimal, doc_no: &str, posting_date: chrono::NaiveDate) -> Result<()> {
    let it = ctx.table(ITEM_LEDGER)?;
    let mut ile = ctx.get(ITEM_LEDGER, &Key(vec![Value::Int(ile_no)]))?;
    ile.set("Cost Amount (Actual)", dec(ile.get("Cost Amount (Actual)").as_dec() + amount));
    ctx.db.modify(it, ile.clone())?;
    let vt = ctx.table(crate::costing::VALUE_ENTRY)?;
    let src = ctx.db.rows(crate::costing::VALUE_ENTRY).find(|v| v.get("Item Ledger Entry No.").as_int() == ile_no).cloned();
    let mut v = vt.init_record();
    v.set("Entry No.", Value::Int(ctx.db.next_entry_no(crate::costing::VALUE_ENTRY)));
    if let Some(src) = &src {
        for f in ["Item No.", "Location Code", "Source Type", "Source No.", "Item Ledger Entry Type", "Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "Inventory Posting Group"] {
            v.set(f, src.get(f).clone());
        }
    }
    v.set("Posting Date", Value::Date(posting_date));
    v.set("Valuation Date", ile.get("Posting Date").clone());
    v.set("Document No.", Value::text(doc_no));
    v.set("Document Type", Value::text("Purchase Invoice"));
    v.set("Item Ledger Entry No.", Value::Int(ile_no));
    v.set("Cost Amount (Actual)", dec(amount));
    v.set("Cost Posted to G/L", dec(amount));
    v.set("Entry Type", Value::text("Direct Cost"));
    v.set("Item Charge No.", Value::text(charge));
    v.set("Description", Value::text(format!("Item charge {charge}")));
    v.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(vt, v)?;
    // The item's average cost takes the extra cost over what is on hand.
    let item_no = ile.str("Item No.").to_string();
    let on_hand: Decimal = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item_no).map(|e| e.get("Quantity").as_dec()).sum();
    if on_hand > Decimal::ZERO {
        let itt = ctx.table(ITEM)?;
        let mut item = ctx.get_code(ITEM, &item_no)?;
        let unit = item.get("Unit Cost").as_dec();
        item.set("Unit Cost", dec(((unit * on_hand + amount) / on_hand).round_dp(5)));
        ctx.db.modify(itt, item)?;
    }
    Ok(())
}

/// Puts `amount` (LCY) of a sales `charge` on a shipped item ledger entry: a value entry with the
/// sales amount, and the entry's Sales Amount (Actual).
pub fn add_sales_amount(ctx: &mut Ctx, ile_no: i64, charge: &str, amount: Decimal, doc_no: &str, posting_date: chrono::NaiveDate) -> Result<()> {
    let it = ctx.table(ITEM_LEDGER)?;
    let mut ile = ctx.get(ITEM_LEDGER, &Key(vec![Value::Int(ile_no)]))?;
    ile.set("Sales Amount (Actual)", dec(ile.get("Sales Amount (Actual)").as_dec() + amount));
    ctx.db.modify(it, ile.clone())?;
    let vt = ctx.table(crate::costing::VALUE_ENTRY)?;
    let src = ctx.db.rows(crate::costing::VALUE_ENTRY).find(|v| v.get("Item Ledger Entry No.").as_int() == ile_no).cloned();
    let mut v = vt.init_record();
    v.set("Entry No.", Value::Int(ctx.db.next_entry_no(crate::costing::VALUE_ENTRY)));
    if let Some(src) = &src {
        for f in ["Item No.", "Location Code", "Source Type", "Source No.", "Item Ledger Entry Type", "Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "Inventory Posting Group"] {
            v.set(f, src.get(f).clone());
        }
    }
    v.set("Posting Date", Value::Date(posting_date));
    v.set("Valuation Date", ile.get("Posting Date").clone());
    v.set("Document No.", Value::text(doc_no));
    v.set("Document Type", Value::text("Sales Invoice"));
    v.set("Item Ledger Entry No.", Value::Int(ile_no));
    v.set("Sales Amount (Actual)", dec(amount));
    v.set("Entry Type", Value::text("Direct Cost"));
    v.set("Item Charge No.", Value::text(charge));
    v.set("Description", Value::text(format!("Item charge {charge}")));
    v.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(vt, v)?;
    Ok(())
}

/// The assignment shares of a charge line's `amount` (LCY), the last taking the rounding.
pub fn shares(assignments: &[Record], qty: Decimal, amount: Decimal) -> Vec<(Record, Decimal)> {
    let mut left = amount;
    let n = assignments.len();
    assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let s = if i + 1 == n || qty.is_zero() { left } else { round_amount(amount * a.get("Qty. to Assign").as_dec() / qty) };
            left -= s;
            (a.clone(), s)
        })
        .collect()
}

/// Checks a charge line can be invoiced: everything to invoice is assigned.
pub fn check_assigned(ctx: &Ctx, sales: bool, l: &Record, to_invoice: Decimal) -> Result<Vec<Record>> {
    let a = assignments(ctx, sales, l.str("Document Type"), l.str("Document No."), l.get("Line No.").as_int());
    let assigned: Decimal = a.iter().map(|x| x.get("Qty. to Assign").as_dec()).sum();
    if assigned != to_invoice {
        refuse!("You must assign item charge {} if you want to invoice it.", l.str("No."));
    }
    Ok(a)
}

/// The charge line a command is for (from the line or one of its assignment rows) and whether it
/// is a sales line.
fn charge_line(ctx: &Ctx, p: &Json, default_sales: bool) -> Result<(Record, bool)> {
    let table = opt_str(p, "table").unwrap_or(if default_sales { SALES_LINE } else { PURCH_LINE });
    let sales = matches!(table, ASSIGNMENT_SALES | SALES_LINE);
    let (assignment, line_table, _, _) = tables(sales);
    let key = if table == assignment {
        let at = ctx.table(assignment)?;
        let a = ctx.get(assignment, &Key::from_json(at, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?)?;
        Key(vec![a.get("Document Type").clone(), a.get("Document No.").clone(), a.get("Document Line No.").clone()])
    } else {
        let t = ctx.table(line_table)?;
        Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?
    };
    let l = ctx.get(line_table, &key)?;
    if l.str("Type") != "Charge (Item)" {
        refuse!("Type must be equal to 'Charge (Item)' in {line_table}: Document Type={}, Document No.={}, Line No.={}. Current value is '{}'.", l.str("Document Type"), l.str("Document No."), l.get("Line No.").as_int(), l.str("Type"));
    }
    Ok((l, sales))
}

fn unit_of(l: &Record, sales: bool) -> Value {
    l.get(if sales { "Unit Price" } else { "Direct Unit Cost" }).clone()
}

fn page_of(sales: bool) -> &'static str {
    if sales { "item-charge-assignment-sales" } else { "item-charge-assignment-purch" }
}

fn open_filters(dt: &str, dn: &str, ln: i64, sales: bool) -> Json {
    json!({"open": {"page": page_of(sales), "filters": [["Document Type", dt], ["Document No.", dn], ["Document Line No.", ln.to_string()]]}})
}

/// BC's Item Charge Assignment for the selected charge line (filled with the document's item
/// lines the first time).
fn open_assignment(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    open_assignment_on(ctx, p, false)
}

fn open_sales_assignment(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    open_assignment_on(ctx, p, true)
}

fn open_assignment_on(ctx: &mut Ctx, p: &Json, default_sales: bool) -> Result<Json> {
    let (l, sales) = charge_line(ctx, p, default_sales)?;
    let (assignment, line_table, _, _) = tables(sales);
    let (dt, dn, ln) = (l.str("Document Type").to_string(), l.str("Document No.").to_string(), l.get("Line No.").as_int());
    if assignments(ctx, sales, &dt, &dn, ln).is_empty() {
        let at = ctx.table(assignment)?;
        let items: Vec<Record> = ctx.db.rows(line_table).filter(|x| x.str("Document Type") == dt && x.str("Document No.") == dn && x.str("Type") == "Item").cloned().collect();
        for (i, x) in items.iter().enumerate() {
            let mut a = at.init_record();
            a.set("Document Type", Value::text(&dt));
            a.set("Document No.", Value::text(&dn));
            a.set("Document Line No.", Value::Int(ln));
            a.set("Line No.", Value::Int((i as i64 + 1) * 10000));
            a.set("Item Charge No.", l.get("No.").clone());
            a.set("Item No.", x.get("No.").clone());
            a.set("Description", x.get("Description").clone());
            a.set("Applies-to Doc. Type", Value::text(&dt));
            a.set("Applies-to Doc. No.", Value::text(&dn));
            a.set("Applies-to Doc. Line No.", x.get("Line No.").clone());
            a.set("Unit Cost", unit_of(&l, sales));
            ctx.db.insert(at, a)?;
        }
    }
    Ok(open_filters(&dt, &dn, ln, sales))
}

/// BC's Get Receipt Lines / Get Shipment Lines: assignment rows for the item lines of a posted
/// receipt (purchase) or shipment (sales).
fn get_posted_lines(ctx: &mut Ctx, p: &Json, default_sales: bool) -> Result<Json> {
    let (l, sales) = charge_line(ctx, p, default_sales)?;
    let (assignment, _, posted, posted_type) = tables(sales);
    let doc = opt_str(p, "document_no").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::invalid("missing parameter 'document_no'"))?.to_uppercase();
    let (dt, dn, ln) = (l.str("Document Type").to_string(), l.str("Document No.").to_string(), l.get("Line No.").as_int());
    let mut found: Vec<Record> = ctx.db.rows(posted).filter(|r| r.str("Document No.") == doc && r.str("Type") == "Item").cloned().collect();
    found.sort_by_key(|r| r.get("Line No.").as_int());
    if found.is_empty() {
        refuse!("There are no item lines in {posted_type} {doc}.");
    }
    let existing = assignments(ctx, sales, &dt, &dn, ln);
    let at = ctx.table(assignment)?;
    let mut next = existing.iter().map(|a| a.get("Line No.").as_int()).max().unwrap_or(0);
    let mut added = 0;
    for r in found {
        if existing.iter().any(|a| a.str("Applies-to Doc. Type") == posted_type && a.str("Applies-to Doc. No.") == doc && a.get("Applies-to Doc. Line No.") == r.get("Line No.")) {
            continue;
        }
        if r.get("Item Ledger Entry No.").as_int() == 0 {
            continue;
        }
        next += 10000;
        let mut a = at.init_record();
        a.set("Document Type", Value::text(&dt));
        a.set("Document No.", Value::text(&dn));
        a.set("Document Line No.", Value::Int(ln));
        a.set("Line No.", Value::Int(next));
        a.set("Item Charge No.", l.get("No.").clone());
        a.set("Item No.", r.get("No.").clone());
        a.set("Description", r.get("Description").clone());
        a.set("Applies-to Doc. Type", Value::text(posted_type));
        a.set("Applies-to Doc. No.", Value::text(&doc));
        a.set("Applies-to Doc. Line No.", r.get("Line No.").clone());
        a.set("Unit Cost", unit_of(&l, sales));
        ctx.db.insert(at, a)?;
        added += 1;
    }
    let mut out = open_filters(&dt, &dn, ln, sales);
    out["message"] = json!(format!("{added} line(s) of {posted_type} {doc} were added."));
    Ok(out)
}

fn get_receipt_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    get_posted_lines(ctx, p, false)
}

fn get_shipment_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    get_posted_lines(ctx, p, true)
}

/// BC's Suggest Item Charge Assignment: Equally, or By Amount of the lines it applies to.
fn suggest(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    suggest_on(ctx, p, false)
}

fn suggest_sales(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    suggest_on(ctx, p, true)
}

fn suggest_on(ctx: &mut Ctx, p: &Json, default_sales: bool) -> Result<Json> {
    let (l, sales) = charge_line(ctx, p, default_sales)?;
    let (assignment, line_table, posted, posted_type) = tables(sales);
    let (dt, dn, ln) = (l.str("Document Type").to_string(), l.str("Document No.").to_string(), l.get("Line No.").as_int());
    if assignments(ctx, sales, &dt, &dn, ln).is_empty() {
        open_assignment_on(ctx, p, sales)?;
    }
    let list = assignments(ctx, sales, &dt, &dn, ln);
    if list.is_empty() {
        refuse!("There are no item lines to assign item charge {} to.", l.str("No."));
    }
    let qty = l.get("Quantity").as_dec() - l.get("Quantity Invoiced").as_dec();
    let by_amount = opt_str(p, "method").is_some_and(|m| m.eq_ignore_ascii_case("By Amount"));
    let weights: Vec<Decimal> = list
        .iter()
        .map(|a| {
            if !by_amount {
                return Decimal::ONE;
            }
            let line = a.get("Applies-to Doc. Line No.").as_int();
            let price = if sales { "Unit Price" } else { "Direct Unit Cost" };
            match a.str("Applies-to Doc. Type") {
                t if t == posted_type => ctx.db.get(posted, &Key(vec![a.get("Applies-to Doc. No.").clone(), Value::Int(line)])).map(|r| r.get("Quantity").as_dec() * r.get(price).as_dec()).unwrap_or_default(),
                t => ctx.db.get(line_table, &Key(vec![Value::text(t), a.get("Applies-to Doc. No.").clone(), Value::Int(line)])).map(|r| r.get("Line Amount").as_dec()).unwrap_or_default(),
            }
        })
        .collect();
    let total: Decimal = weights.iter().sum();
    if total.is_zero() {
        refuse!("The lines have no amount to assign by.");
    }
    let at = ctx.table(assignment)?;
    let mut left = qty;
    let unit = unit_of(&l, sales).as_dec();
    let n = list.len();
    for (i, (mut a, w)) in list.into_iter().zip(weights).enumerate() {
        let q = if i + 1 == n { left } else { (qty * w / total).round_dp(5) };
        left -= q;
        a.set("Qty. to Assign", dec(q));
        a.set("Amount to Assign", dec(round_amount(q * unit)));
        ctx.db.modify(at, a)?;
    }
    Ok(json!({"message": format!("Item charge {} was assigned {}.", l.str("No."), if by_amount { "by amount" } else { "equally" })}))
}

/// Assignment rows keep Amount to Assign = Qty. to Assign × Unit Cost.
pub fn on_validate(_ctx: &Ctx, t: &TableDef, r: &mut Record, field: &str) {
    if (t.name == ASSIGNMENT || t.name == ASSIGNMENT_SALES) && matches!(field, "Qty. to Assign" | "Unit Cost") {
        r.set("Amount to Assign", dec(round_amount(r.get("Qty. to Assign").as_dec() * r.get("Unit Cost").as_dec())));
    }
}

fn s_line() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "method": {"type": "string", "enum": ["Equally", "By Amount"], "default": "Equally"}}, "required": ["key"]})
}

fn s_posted() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "document_no": {"type": "string", "description": "the posted receipt (purchase) or shipment (sales)"}}, "required": ["key", "document_no"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "purchase.charge.assignment", label: "Item Charge Assignment", group: "Purchasing", summary: "The item lines a purchase charge line is shared over.", mutates: true, params: s_line, run: open_assignment },
    CommandDef { id: "purchase.charge.suggest", label: "Suggest Item Charge Assignment", group: "Purchasing", summary: "Share a purchase charge line over its item lines equally or by amount.", mutates: true, params: s_line, run: suggest },
    CommandDef { id: "purchase.charge.get_receipt_lines", label: "Get Receipt Lines", group: "Purchasing", summary: "Assign a purchase charge to the item lines of a posted receipt.", mutates: true, params: s_posted, run: get_receipt_lines },
    CommandDef { id: "sales.charge.assignment", label: "Item Charge Assignment", group: "Sales", summary: "The item lines a sales charge line is shared over.", mutates: true, params: s_line, run: open_sales_assignment },
    CommandDef { id: "sales.charge.suggest", label: "Suggest Item Charge Assignment", group: "Sales", summary: "Share a sales charge line over its item lines equally or by amount.", mutates: true, params: s_line, run: suggest_sales },
    CommandDef { id: "sales.charge.get_shipment_lines", label: "Get Shipment Lines", group: "Sales", summary: "Assign a sales charge to the item lines of a posted shipment.", mutates: true, params: s_posted, run: get_shipment_lines },
];
