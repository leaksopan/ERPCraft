//! Costing: Value Entries (BC 5802) behind every item ledger entry, Adjust Cost - Item Entries
//! (report 795) with a chronological moving average, and Post Inventory Cost to G/L (report
//! 1002) for adjustments not yet on the G/L.

use std::collections::{HashMap, VecDeque};

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::Result;
use crate::ledger::Txn;
use crate::posting::{GlDoc, GlLine, post_gl};
use crate::refuse;
use crate::schema::tables::{GEN_POSTING_SETUP, INVENTORY_SETUP, ITEM, ITEM_LEDGER};

pub const VALUE_ENTRY: &str = "Value Entry";

/// The value entry of a new item ledger entry (cost already on the G/L, as the posting routines
/// post it with the document).
pub fn on_item_entry(ctx: &mut Ctx, ile: &Record) -> Result<()> {
    let t = ctx.table(VALUE_ENTRY)?;
    let item = ctx.get_code(ITEM, ile.str("Item No."))?;
    let gen_bus = match ile.str("Source Type") {
        "Customer" => ctx.db.get_code(crate::schema::tables::CUSTOMER, ile.str("Source No.")).map(|c| c.str("Gen. Bus. Posting Group").to_string()).unwrap_or_default(),
        "Vendor" => ctx.db.get_code(crate::schema::tables::VENDOR, ile.str("Source No.")).map(|c| c.str("Gen. Bus. Posting Group").to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    let qty = ile.get("Quantity").as_dec();
    let cost = ile.get("Cost Amount (Actual)").as_dec();
    let mut v = t.init_record();
    v.set("Entry No.", Value::Int(ctx.db.next_entry_no(VALUE_ENTRY)));
    for f in ["Item No.", "Posting Date", "Document No.", "Document Type", "Description", "Location Code", "Source Type", "Source No."] {
        v.set(f, ile.get(f).clone());
    }
    v.set("Item Ledger Entry Type", ile.get("Entry Type").clone());
    v.set("Item Ledger Entry No.", ile.get("Entry No.").clone());
    v.set("Item Ledger Entry Quantity", dec(qty));
    v.set("Valued Quantity", dec(qty));
    v.set("Invoiced Quantity", dec(qty));
    v.set("Cost per Unit", dec(if qty.is_zero() { Decimal::ZERO } else { (cost / qty).round_dp(5) }));
    v.set("Cost Amount (Actual)", dec(cost));
    v.set("Cost Posted to G/L", dec(cost));
    v.set("Sales Amount (Actual)", ile.get("Sales Amount (Actual)").clone());
    v.set("Entry Type", Value::text("Direct Cost"));
    v.set("Gen. Bus. Posting Group", Value::text(gen_bus));
    v.set("Gen. Prod. Posting Group", item.get("Gen. Prod. Posting Group").clone());
    v.set("Inventory Posting Group", item.get("Inventory Posting Group").clone());
    v.set("Valuation Date", ile.get("Posting Date").clone());
    v.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, v)?;
    Ok(())
}

fn setup_account(ctx: &Ctx, bus: &str, prod: &str, field: &str) -> Result<String> {
    let s = ctx.get(GEN_POSTING_SETUP, &Key(vec![Value::text(bus), Value::text(prod)]))?;
    match s.str(field) {
        "" => refuse!("{field} must have a value in General Posting Setup: Gen. Bus. Posting Group='{bus}',Gen. Prod. Posting Group='{prod}'."),
        a => Ok(a.to_string()),
    }
}

/// The G/L lines of a value entry's cost not yet on the G/L: inventory against COGS (sales),
/// inventory adjustments (adjustments) or, for transfers, nothing (both sides are inventory).
fn gl_for(ctx: &Ctx, v: &Record, amount: Decimal) -> Result<Vec<GlLine>> {
    let item = ctx.get_code(ITEM, v.str("Item No."))?;
    let inventory = crate::posting::inventory_account(ctx, &item, v.str("Location Code"))?;
    let desc = format!("Cost adjustment {} {}", v.str("Item No."), v.str("Document No."));
    let mut lines = vec![GlLine { account: inventory, amount, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" }];
    let contra = match v.str("Item Ledger Entry Type") {
        "Sale" => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "COGS Account")?),
        "Positive Adjmt." | "Negative Adjmt." => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "Inventory Adjmt. Account")?),
        "Purchase" => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "Purch. Account")?),
        _ => None,
    };
    if let Some(c) = contra {
        lines.push(GlLine { account: c, amount: -amount, vat_amount: Decimal::ZERO, description: desc, gen_posting_type: "" });
    }
    Ok(lines)
}

fn entry_cost(ctx: &Ctx, ile_no: i64) -> Decimal {
    ctx.db.rows(VALUE_ENTRY).filter(|v| v.get("Item Ledger Entry No.").as_int() == ile_no).map(|v| v.get("Cost Amount (Actual)").as_dec()).sum()
}

/// BC's Adjust Cost - Item Entries for average costing: walking each item's entries by date, an
/// outbound entry is valued at the running average of what came in before it; a transfer's
/// receipt takes its shipment's cost. Differences become adjustment value entries.
fn adjust_cost(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let only = opt_str(p, "item_no").map(str::to_string);
    let automatic = ctx.setup(INVENTORY_SETUP)?.get("Automatic Cost Posting").as_bool();
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| only.as_ref().is_none_or(|o| i.str("No.") == o) && i.str("Type") != "Service").cloned().collect();
    let vt = ctx.table(VALUE_ENTRY)?;
    let it = ctx.table(ITEM_LEDGER)?;
    let itt = ctx.table(ITEM)?;
    let txn = Txn::new(ctx, "INVTADJMT", "", "");
    let mut gl: Vec<GlLine> = Vec::new();
    let mut adjusted = 0;
    let date = ctx.work_date;
    for item in items {
        let no = item.str("No.").to_string();
        let mut ledger: Vec<Record> = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no).cloned().collect();
        ledger.sort_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
        if ledger.is_empty() {
            continue;
        }
        let standard = item.str("Costing Method") == "Standard";
        let (mut qty, mut cost) = (Decimal::ZERO, Decimal::ZERO);
        let mut shipped: HashMap<String, VecDeque<Decimal>> = HashMap::new();
        for e in &ledger {
            let q = e.get("Quantity").as_dec();
            let current = entry_cost(ctx, e.get("Entry No.").as_int());
            let transfer = e.str("Entry Type") == "Transfer";
            let target = if q < Decimal::ZERO {
                // Outbound: the running average (standard cost for standard items).
                let unit = if standard { item.get("Standard Cost").as_dec() } else if qty > Decimal::ZERO { cost / qty } else { item.get("Unit Cost").as_dec() };
                let t = round_amount(q * unit);
                if transfer {
                    shipped.entry(e.str("Document No.").to_string()).or_default().push_back(-t / -q);
                }
                Some(t)
            } else if transfer {
                // A transfer's receipt: the cost its shipment left with.
                shipped.get_mut(e.str("Document No.")).and_then(VecDeque::pop_front).map(|unit| round_amount(q * unit))
            } else {
                None
            };
            let value = target.unwrap_or(current);
            if let Some(t) = target {
                let diff = t - current;
                if !diff.is_zero() {
                    let mut v = vt.init_record();
                    let src = ctx.db.rows(VALUE_ENTRY).find(|v| v.get("Item Ledger Entry No.") == e.get("Entry No.")).cloned();
                    v.set("Entry No.", Value::Int(ctx.db.next_entry_no(VALUE_ENTRY)));
                    if let Some(src) = &src {
                        for f in ["Item No.", "Document No.", "Document Type", "Description", "Location Code", "Source Type", "Source No.", "Item Ledger Entry Type", "Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "Inventory Posting Group"] {
                            v.set(f, src.get(f).clone());
                        }
                    }
                    v.set("Posting Date", Value::Date(date.max(e.get("Posting Date").as_date().unwrap_or(date))));
                    v.set("Valuation Date", e.get("Posting Date").clone());
                    v.set("Item Ledger Entry No.", e.get("Entry No.").clone());
                    v.set("Item Ledger Entry Quantity", dec(Decimal::ZERO));
                    v.set("Cost Amount (Actual)", dec(diff));
                    v.set("Entry Type", Value::text("Direct Cost"));
                    v.set("Adjustment", Value::Bool(true));
                    v.set("User ID", Value::text(ctx.user.to_uppercase()));
                    if automatic {
                        gl.extend(gl_for(ctx, &v, diff)?);
                        v.set("Cost Posted to G/L", dec(diff));
                    }
                    ctx.db.insert(vt, v)?;
                    let mut ile = e.clone();
                    ile.set("Cost Amount (Actual)", dec(t));
                    ctx.db.modify(it, ile)?;
                    adjusted += 1;
                }
            }
            qty += q;
            cost += value;
        }
        if !standard && qty > Decimal::ZERO {
            let mut i = ctx.get_code(ITEM, &no)?;
            i.set("Unit Cost", dec((cost / qty).round_dp(5)));
            ctx.db.modify(itt, i)?;
        }
    }
    if !gl.is_empty() {
        post_gl(ctx, &GlDoc { posting_date: date, doc_type: "", doc_no: "ADJCOST", ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    }
    Ok(json!({"adjusted": adjusted, "message": format!("{adjusted} item ledger entries were adjusted.")}))
}

/// BC's Post Inventory Cost to G/L: every value entry's cost not yet on the G/L.
fn post_inventory_cost(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let date = match opt_str(p, "posting_date").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| crate::error::EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?,
        None => ctx.work_date,
    };
    let pending: Vec<Record> = ctx.db.rows(VALUE_ENTRY).filter(|v| v.get("Cost Amount (Actual)").as_dec() != v.get("Cost Posted to G/L").as_dec()).cloned().collect();
    if pending.is_empty() {
        return Ok(json!({"entries": 0, "message": "There is nothing to post to the general ledger."}));
    }
    let vt = ctx.table(VALUE_ENTRY)?;
    let txn = Txn::new(ctx, "INVTPCOST", "", "");
    let mut gl = Vec::new();
    let mut rows = Vec::new();
    for mut v in pending.clone() {
        let amount = v.get("Cost Amount (Actual)").as_dec() - v.get("Cost Posted to G/L").as_dec();
        gl.extend(gl_for(ctx, &v, amount)?);
        rows.push(json!([v.get("Entry No.").as_int(), v.str("Item No."), v.str("Item Ledger Entry Type"), format!("{:.2}", amount)]));
        v.set("Cost Posted to G/L", v.get("Cost Amount (Actual)").clone());
        ctx.db.modify(vt, v)?;
    }
    post_gl(ctx, &GlDoc { posting_date: date, doc_type: "", doc_no: "INVTCOST", ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    Ok(json!({"entries": pending.len(), "report": {"title": "Post Inventory Cost to G/L", "columns": ["Value Entry No.", "Item No.", "Entry Type", "Cost"], "rows": rows},
              "message": format!("{} value entries were posted to the general ledger.", pending.len())}))
}

fn s_adjust() -> Json {
    json!({"type": "object", "properties": {"item_no": {"type": "string", "description": "one item; all when blank"}}})
}

fn s_post() -> Json {
    json!({"type": "object", "properties": {"posting_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "inventory.adjust_cost", label: "Adjust Cost - Item Entries", group: "Inventory", summary: "Value outbound item entries at the running average cost and post the differences.", mutates: true, params: s_adjust, run: adjust_cost },
    CommandDef { id: "inventory.post_cost_to_gl", label: "Post Inventory Cost to G/L", group: "Inventory", summary: "Post the inventory cost of value entries not yet on the general ledger.", mutates: true, params: s_post, run: post_inventory_cost },
];
