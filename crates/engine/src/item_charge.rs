//! Item charges (BC's Item Charge, Item Charge Assignment (Purch), Suggest Item Charge Assignment,
//! ItemChargeAssgnt posting): a purchase charge line is shared over item lines of the document
//! or posted receipt lines; posting puts each share on the item's inventory and on the receiving
//! item ledger entry as a value entry (so Adjust Cost carries it to what was sold).

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::tables::{ITEM, ITEM_LEDGER, PURCH_LINE};

pub const ITEM_CHARGE: &str = "Item Charge";
pub const ASSIGNMENT: &str = "Item Charge Assignment (Purch)";
pub const APPLIES_TO_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", "Receipt", "Transfer Receipt", "Return Shipment", "Sales Shipment", "Return Receipt"];

pub fn assignments(ctx: &Ctx, doc_type: &str, doc_no: &str, line_no: i64) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(ASSIGNMENT)
        .filter(|a| a.str("Document Type") == doc_type && a.str("Document No.") == doc_no && a.get("Document Line No.").as_int() == line_no)
        .cloned()
        .collect();
    v.sort_by_key(|a| a.get("Line No.").as_int());
    v
}

/// The item ledger entry an assignment's cost lands on: the receipt line's, or (an item line of
/// the same document) the entry its posting made / its order's latest receipt made.
pub fn target_entry(ctx: &Ctx, a: &Record, posted_now: &[(i64, i64)]) -> Option<i64> {
    let line = a.get("Applies-to Doc. Line No.").as_int();
    match a.str("Applies-to Doc. Type") {
        "Receipt" => ctx
            .db
            .get("Purch. Rcpt. Line", &Key(vec![a.get("Applies-to Doc. No.").clone(), Value::Int(line)]))
            .map(|r| r.get("Item Ledger Entry No.").as_int())
            .filter(|n| *n != 0),
        _ => posted_now.iter().find(|(l, _)| *l == line).map(|(_, e)| *e).or_else(|| {
            ctx.db
                .rows("Purch. Rcpt. Line")
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
pub fn check_assigned(ctx: &Ctx, l: &Record, to_invoice: Decimal) -> Result<Vec<Record>> {
    let a = assignments(ctx, l.str("Document Type"), l.str("Document No."), l.get("Line No.").as_int());
    let assigned: Decimal = a.iter().map(|x| x.get("Qty. to Assign").as_dec()).sum();
    if assigned != to_invoice {
        refuse!("You must assign item charge {} if you want to invoice it.", l.str("No."));
    }
    Ok(a)
}

fn charge_line(ctx: &Ctx, p: &Json) -> Result<Record> {
    let t = ctx.table(PURCH_LINE)?;
    let key = if opt_str(p, "table") == Some(ASSIGNMENT) {
        let at = ctx.table(ASSIGNMENT)?;
        let a = ctx.get(ASSIGNMENT, &Key::from_json(at, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?)?;
        Key(vec![a.get("Document Type").clone(), a.get("Document No.").clone(), a.get("Document Line No.").clone()])
    } else {
        Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?
    };
    let l = ctx.get(PURCH_LINE, &key)?;
    if l.str("Type") != "Charge (Item)" {
        refuse!("Type must be equal to 'Charge (Item)' in Purchase Line: Document Type={}, Document No.={}, Line No.={}. Current value is '{}'.", l.str("Document Type"), l.str("Document No."), l.get("Line No.").as_int(), l.str("Type"));
    }
    Ok(l)
}

/// BC's Item Charge Assignment for the selected charge line (filled with the document's item
/// lines the first time).
fn open_assignment(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let l = charge_line(ctx, p)?;
    let (dt, dn, ln) = (l.str("Document Type").to_string(), l.str("Document No.").to_string(), l.get("Line No.").as_int());
    if assignments(ctx, &dt, &dn, ln).is_empty() {
        let at = ctx.table(ASSIGNMENT)?;
        let items: Vec<Record> = ctx.db.rows(PURCH_LINE).filter(|x| x.str("Document Type") == dt && x.str("Document No.") == dn && x.str("Type") == "Item").cloned().collect();
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
            a.set("Unit Cost", l.get("Direct Unit Cost").clone());
            ctx.db.insert(at, a)?;
        }
    }
    Ok(json!({"open": {"page": "item-charge-assignment-purch", "filters": [["Document Type", dt], ["Document No.", dn], ["Document Line No.", ln.to_string()]]}}))
}

/// BC's Suggest Item Charge Assignment: Equally, or By Amount of the lines it applies to.
fn suggest(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let l = charge_line(ctx, p)?;
    let (dt, dn, ln) = (l.str("Document Type").to_string(), l.str("Document No.").to_string(), l.get("Line No.").as_int());
    if assignments(ctx, &dt, &dn, ln).is_empty() {
        open_assignment(ctx, p)?;
    }
    let list = assignments(ctx, &dt, &dn, ln);
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
            match a.str("Applies-to Doc. Type") {
                "Receipt" => ctx.db.get("Purch. Rcpt. Line", &Key(vec![a.get("Applies-to Doc. No.").clone(), Value::Int(line)])).map(|r| r.get("Quantity").as_dec() * r.get("Direct Unit Cost").as_dec()).unwrap_or_default(),
                t => ctx.db.get(PURCH_LINE, &Key(vec![Value::text(t), a.get("Applies-to Doc. No.").clone(), Value::Int(line)])).map(|r| r.get("Line Amount").as_dec()).unwrap_or_default(),
            }
        })
        .collect();
    let total: Decimal = weights.iter().sum();
    if total.is_zero() {
        refuse!("The lines have no amount to assign by.");
    }
    let at = ctx.table(ASSIGNMENT)?;
    let mut left = qty;
    let unit = l.get("Direct Unit Cost").as_dec();
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
    if t.name == ASSIGNMENT && matches!(field, "Qty. to Assign" | "Unit Cost") {
        r.set("Amount to Assign", dec(round_amount(r.get("Qty. to Assign").as_dec() * r.get("Unit Cost").as_dec())));
    }
}

fn s_line() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "method": {"type": "string", "enum": ["Equally", "By Amount"], "default": "Equally"}}, "required": ["key"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "purchase.charge.assignment", label: "Item Charge Assignment", group: "Purchasing", summary: "The item lines a purchase charge line is shared over.", mutates: true, params: s_line, run: open_assignment },
    CommandDef { id: "purchase.charge.suggest", label: "Suggest Item Charge Assignment", group: "Purchasing", summary: "Share a purchase charge line over its item lines equally or by amount.", mutates: true, params: s_line, run: suggest },
];
