//! Requisition worksheet (BC's Req. Worksheet, Calculate Plan - Req. Wksh., Carry Out Action
//! Msg. - Req.): each item's projected inventory (on hand + open purchase orders - open sales
//! orders) against its reordering policy proposes a purchase; carrying out makes purchase orders
//! per vendor.

use chrono::NaiveDate;
use erpcraft_meta::{Record, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::tables::{ITEM, ITEM_LEDGER, PURCH_LINE, SALES_LINE};
use crate::side::PURCHASE;

pub const REQ_TEMPLATE: &str = "Req. Wksh. Template";
pub const REQ_NAME: &str = "Requisition Wksh. Name";
pub const REQ_LINE: &str = "Requisition Line";

fn per(l: &Record) -> Decimal {
    match l.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    }
}

/// On hand + open purchase orders - open sales orders, in the base unit (at one location when
/// planning a stockkeeping unit).
pub fn projected(ctx: &Ctx, item: &str) -> Decimal {
    projected_at(ctx, item, None)
}

pub fn projected_at(ctx: &Ctx, item: &str, location: Option<&str>) -> Decimal {
    let at = |l: &Record| location.is_none_or(|x| l.str("Location Code") == x);
    let on_hand: Decimal = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && at(e)).map(|e| e.get("Quantity").as_dec()).sum();
    let open = |table: &str| -> Decimal {
        ctx.db
            .rows(table)
            .filter(|l| l.str("Document Type") == "Order" && l.str("Type") == "Item" && l.str("No.") == item && at(l))
            .map(|l| l.get("Outstanding Quantity").as_dec() * per(l))
            .sum()
    };
    on_hand + open(PURCH_LINE) - open(SALES_LINE)
}

/// BC's planning for one item: what to order (0 = nothing).
pub fn plan_quantity(item: &Record, projected: Decimal) -> Decimal {
    let reorder_point = item.get("Reorder Point").as_dec();
    let safety = item.get("Safety Stock Quantity").as_dec();
    let mut qty = match item.str("Reordering Policy") {
        "Fixed Reorder Qty." if projected <= reorder_point.max(safety) => {
            let step = item.get("Reorder Quantity").as_dec();
            if step <= Decimal::ZERO {
                reorder_point.max(safety) - projected + Decimal::ONE
            } else {
                let mut q = step;
                while projected + q <= reorder_point {
                    q += step;
                }
                q
            }
        }
        "Maximum Qty." if projected <= reorder_point.max(safety) => item.get("Maximum Inventory").as_dec() - projected,
        "Lot-for-Lot" | "Order" if projected < safety || projected < Decimal::ZERO => safety - projected,
        _ => Decimal::ZERO,
    };
    if qty <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    qty = qty.max(item.get("Minimum Order Quantity").as_dec());
    let multiple = item.get("Order Multiple").as_dec();
    if multiple > Decimal::ZERO {
        qty = (qty / multiple).ceil() * multiple;
    }
    qty
}

fn batch_of(p: &Json) -> (String, String) {
    (opt_str(p, "template").unwrap_or("REQ").to_uppercase(), opt_str(p, "batch").unwrap_or("DEFAULT").to_uppercase())
}

fn date_param(p: &Json, k: &str, d: NaiveDate) -> Result<NaiveDate> {
    match opt_str(p, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD)."))),
        None => Ok(d),
    }
}

fn lines_of(ctx: &Ctx, template: &str, batch: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(REQ_LINE).filter(|l| l.str("Worksheet Template Name") == template && l.str("Journal Batch Name") == batch).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

/// BC's Calculate Plan: a New line for every item below its reorder point (or safety stock).
fn calculate_plan(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (template, batch) = batch_of(p);
    let order_date = date_param(p, "order_date", ctx.work_date)?;
    let only = opt_str(p, "item_no").map(str::to_string);
    let lt = ctx.table(REQ_LINE)?;
    // Earlier suggestions for the same items go.
    for l in lines_of(ctx, &template, &batch) {
        if only.as_ref().is_none_or(|o| l.str("No.") == o) && l.str("Action Message") == "New" {
            ctx.db.remove(&lt.name, &l.key(lt));
        }
    }
    let mut next = lines_of(ctx, &template, &batch).iter().map(|l| l.get("Line No.").as_int()).max().unwrap_or(0);
    let skus: Vec<Record> = ctx.db.rows(crate::schema::sku::SKU).cloned().collect();
    let items: Vec<Record> = ctx
        .db
        .rows(ITEM)
        .filter(|i| only.as_ref().is_none_or(|o| i.str("No.") == o) && i.str("Type") != "Service" && !i.get("Blocked").as_bool())
        .cloned()
        .collect();
    // What to plan: each item's stockkeeping units when it has any, else the item itself.
    let mut plans: Vec<(Record, Record, Option<String>)> = Vec::new();
    for item in items {
        let own: Vec<&Record> = skus.iter().filter(|s| s.str("Item No.") == item.str("No.")).collect();
        if own.is_empty() {
            plans.push((item.clone(), item.clone(), None));
        } else {
            for s in own {
                plans.push((item.clone(), s.clone(), Some(s.str("Location Code").to_string())));
            }
        }
    }
    plans.retain(|(_, p, _)| !p.str("Reordering Policy").is_empty() && matches!(p.str("Replenishment System"), "" | "Purchase"));
    let mut created = 0;
    for (item, params, location) in plans {
        let no = item.str("No.").to_string();
        let qty = plan_quantity(&params, projected_at(ctx, &no, location.as_deref()));
        if qty.is_zero() {
            continue;
        }
        next += 10000;
        let due = crate::dateformula::calc(params.str("Lead Time Calculation"), order_date)?;
        let cost = match params.get("Last Direct Cost").as_dec() {
            d if d.is_zero() => params.get("Unit Cost").as_dec(),
            d => d,
        };
        let mut l = lt.init_record();
        l.set("Worksheet Template Name", Value::text(&template));
        l.set("Journal Batch Name", Value::text(&batch));
        l.set("Line No.", Value::Int(next));
        l.set("Type", Value::text("Item"));
        l.set("No.", Value::text(&no));
        l.set("Description", item.get("Description").clone());
        l.set("Quantity", dec(qty));
        l.set("Original Quantity", dec(qty));
        l.set("Unit of Measure Code", item.get("Base Unit of Measure").clone());
        l.set("Qty. per Unit of Measure", dec(Decimal::ONE));
        l.set("Quantity (Base)", dec(qty));
        l.set("Vendor No.", if params.str("Vendor No.").is_empty() { item.get("Vendor No.").clone() } else { params.get("Vendor No.").clone() });
        if let Some(loc) = &location {
            l.set("Location Code", Value::text(loc));
        }
        l.set("Direct Unit Cost", dec(cost));
        l.set("Order Date", Value::Date(order_date));
        l.set("Due Date", Value::Date(due));
        l.set("Action Message", Value::text("New"));
        l.set("Accept Action Message", Value::Bool(true));
        l.set("Replenishment System", Value::text("Purchase"));
        ctx.db.insert(lt, l)?;
        created += 1;
    }
    Ok(json!({"lines": created, "message": format!("{created} line(s) were suggested.")}))
}

/// BC's Carry Out Action Message: accepted New lines become purchase orders, one per vendor.
fn carry_out(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (template, batch) = batch_of(p);
    let lt = ctx.table(REQ_LINE)?;
    let lines: Vec<Record> = lines_of(ctx, &template, &batch).into_iter().filter(|l| l.get("Accept Action Message").as_bool() && l.get("Quantity").as_dec() > Decimal::ZERO).collect();
    if lines.is_empty() {
        refuse!("There is nothing to carry out.");
    }
    let mut vendors: Vec<String> = Vec::new();
    for l in &lines {
        if l.str("Vendor No.").is_empty() {
            refuse!("Vendor No. must have a value in Requisition Line: Worksheet Template Name={template}, Journal Batch Name={batch}, Line No.={}. It cannot be zero or empty.", l.get("Line No.").as_int());
        }
        if !vendors.contains(&l.str("Vendor No.").to_string()) {
            vendors.push(l.str("Vendor No.").to_string());
        }
    }
    let mut orders = Vec::new();
    for v in vendors {
        let doc_lines: Vec<Json> = lines
            .iter()
            .filter(|l| l.str("Vendor No.") == v)
            .map(|l| json!({"type": "Item", "no": l.str("No."), "quantity": l.get("Quantity").as_dec().to_string(), "direct_unit_cost": l.get("Direct Unit Cost").as_dec().to_string()}))
            .collect();
        let mut values = Map::new();
        if let Some(first) = lines.iter().find(|l| l.str("Vendor No.") == v) {
            if let Some(d) = first.get("Due Date").as_date() {
                values.insert("Expected Receipt Date".into(), json!(d.to_string()));
            }
            if !first.str("Location Code").is_empty() {
                values.insert("Location Code".into(), json!(first.str("Location Code")));
            }
        }
        let doc = crate::documents::new_document(ctx, &PURCHASE, "Order", &v, &values, &doc_lines)?;
        orders.push(doc.get("no").cloned().unwrap_or(Json::Null));
    }
    for l in &lines {
        ctx.db.remove(&lt.name, &l.key(lt));
    }
    Ok(json!({"orders": orders, "message": format!("{} purchase order(s) were created.", orders.len())}))
}

fn s_plan() -> Json {
    json!({"type": "object", "properties": {"template": {"type": "string", "default": "REQ"}, "batch": {"type": "string", "default": "DEFAULT"},
        "order_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"}, "item_no": {"type": "string"}}})
}

fn s_batch() -> Json {
    json!({"type": "object", "properties": {"template": {"type": "string", "default": "REQ"}, "batch": {"type": "string", "default": "DEFAULT"}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "reqwksh.calculate_plan", label: "Calculate Plan", group: "Purchasing", summary: "Suggest purchases for items below their reorder point, by their reordering policy.", mutates: true, params: s_plan, run: calculate_plan },
    CommandDef { id: "reqwksh.carry_out", label: "Carry Out Action Message", group: "Purchasing", summary: "Turn the accepted requisition lines into purchase orders, one per vendor.", mutates: true, params: s_batch, run: carry_out },
];
