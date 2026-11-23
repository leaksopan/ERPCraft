//! Stockkeeping units (BC's Create Stockkeeping Unit report 5706 and the 5700 table triggers):
//! a copy of an item's costs and planning parameters per location (and variant), which the
//! requisition worksheet plans by.

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::inventory::LOCATION;
use crate::schema::sku::{FROM_ITEM, SKU};
use crate::schema::tables::{ITEM, ITEM_LEDGER, PURCH_LINE, SALES_LINE};

fn take_item(r: &mut Record, item: &Record) {
    for f in FROM_ITEM {
        r.set(f, item.get(f).clone());
    }
}

pub fn on_insert(ctx: &Ctx, t: &TableDef, r: &mut Record) -> Result<bool> {
    if t.name != SKU {
        return Ok(false);
    }
    let item = ctx.get_code(ITEM, r.str("Item No."))?;
    ctx.get_code(LOCATION, r.str("Location Code"))?;
    if r.str("Description").is_empty() {
        take_item(r, &item);
    }
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    Ok(true)
}

pub fn on_validate(ctx: &Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    if t.name != SKU {
        return Ok(());
    }
    if field == "Item No." && !r.str("Item No.").is_empty() {
        let item = ctx.get_code(ITEM, r.str("Item No."))?;
        take_item(r, &item);
    }
    if field == "Transfer-from Code" && !r.str("Transfer-from Code").is_empty() && r.str("Transfer-from Code") == r.str("Location Code") {
        refuse!("Transfer-from Code must not be the same as Location Code.");
    }
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    Ok(())
}

/// The calculated fields of a stockkeeping unit.
pub fn calc_fields(db: &crate::db::Database, r: &mut Record) {
    let (item, loc, variant) = (r.str("Item No.").to_string(), r.str("Location Code").to_string(), r.str("Variant Code").to_string());
    let same = |x: &Record, loc_field: &str, item_field: &str| x.str(item_field) == item && x.str(loc_field) == loc && x.str("Variant Code") == variant;
    let inventory: Decimal = db.rows(ITEM_LEDGER).filter(|e| same(e, "Location Code", "Item No.")).map(|e| e.get("Quantity").as_dec()).sum();
    let open = |table: &str| -> Decimal {
        db.rows(table)
            .filter(|l| l.str("Document Type") == "Order" && l.str("Type") == "Item" && same(l, "Location Code", "No."))
            .map(|l| {
                let per = match l.get("Qty. per Unit of Measure").as_dec() {
                    d if d.is_zero() => Decimal::ONE,
                    d => d,
                };
                l.get("Outstanding Quantity").as_dec() * per
            })
            .sum()
    };
    let in_transit: Decimal = db
        .rows(crate::schema::transfer::TRANSFER_LINE)
        .filter(|l| same(l, "Transfer-to Code", "Item No."))
        .map(|l| l.get("Quantity Shipped").as_dec() - l.get("Quantity Received").as_dec())
        .sum();
    r.set("Inventory", Value::Dec(inventory));
    r.set("Qty. on Purch. Order", Value::Dec(open(PURCH_LINE)));
    r.set("Qty. on Sales Order", Value::Dec(open(SALES_LINE)));
    r.set("Qty. in Transit", Value::Dec(in_transit));
}

/// The stockkeeping unit of an item at a location (blank variant), if there is one.
pub fn of(ctx: &Ctx, item: &str, location: &str) -> Option<Record> {
    ctx.db.get(SKU, &Key(vec![Value::text(location), Value::text(item), Value::text("")])).cloned()
}

/// BC's Create Stockkeeping Unit: one per location (not in transit) for each item given.
fn create(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let items: Vec<String> = match (opt_str(p, "table"), p.get("key"), opt_str(p, "no")) {
        (Some(ITEM), Some(k), _) => vec![k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)).unwrap_or_default()],
        (_, _, Some(no)) => vec![no.to_string()],
        _ => p.get("keys").and_then(Json::as_array).map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|x| x.first()).and_then(Json::as_str).map(str::to_string))).collect()).unwrap_or_default(),
    };
    if items.is_empty() {
        return Err(EngineError::invalid("missing parameter 'no'"));
    }
    let only = opt_str(p, "location_code").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let replace = p.get("replace_previous").and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))).unwrap_or(false);
    let locations: Vec<String> = ctx.db.rows(LOCATION).filter(|l| !l.get("Use As In-Transit").as_bool() && only.as_ref().is_none_or(|o| l.str("Code") == o)).map(|l| l.str("Code").to_string()).collect();
    if locations.is_empty() {
        refuse!("There are no locations to create stockkeeping units for.");
    }
    let t = ctx.table(SKU)?;
    let mut created = 0;
    for no in &items {
        let item = ctx.get_code(ITEM, no)?;
        if item.str("Type") != "Inventory" {
            refuse!("Type must be equal to 'Inventory' in Item: No.={no}. Current value is '{}'.", item.str("Type"));
        }
        for loc in &locations {
            let existing = of(ctx, no, loc);
            if existing.is_some() && !replace {
                continue;
            }
            let mut r = existing.clone().unwrap_or_else(|| t.init_record());
            r.set("Item No.", Value::text(no));
            r.set("Location Code", Value::text(loc));
            r.set("Variant Code", Value::text(""));
            take_item(&mut r, &item);
            r.set("Last Date Modified", Value::Date(ctx.work_date));
            if existing.is_some() {
                ctx.db.modify(t, r)?;
            } else {
                ctx.db.insert(t, r)?;
            }
            created += 1;
        }
    }
    Ok(json!({"created": created, "message": format!("{created} stockkeeping unit(s) were created.")}))
}

/// BC's Inventory - List (report 701) over stockkeeping units, and Inventory Availability
/// (report 705): per SKU its inventory and, for availability, its requirements and receipts.
fn inventory_report(ctx: &mut Ctx, p: &Json, availability: bool) -> Result<Json> {
    let only = opt_str(p, "item_no").map(str::to_string);
    let mut skus: Vec<Record> = ctx.db.rows(SKU).filter(|s| only.as_ref().is_none_or(|o| s.str("Item No.") == o)).cloned().collect();
    skus.sort_by(|a, b| (a.str("Item No."), a.str("Location Code")).cmp(&(b.str("Item No."), b.str("Location Code"))));
    let mut rows = Vec::new();
    for mut s in skus {
        calc_fields(ctx.db, &mut s);
        let n = |f: &str| s.get(f).as_dec().normalize().to_string();
        if availability {
            let projected = s.get("Inventory").as_dec() + s.get("Qty. on Purch. Order").as_dec() - s.get("Qty. on Sales Order").as_dec();
            let reorder = !s.str("Reordering Policy").is_empty() && projected <= s.get("Reorder Point").as_dec();
            rows.push(json!([s.str("Item No."), s.str("Location Code"), s.str("Variant Code"), s.str("Description"), n("Inventory"), n("Qty. on Sales Order"), n("Qty. on Purch. Order"), projected.normalize().to_string(), n("Reorder Point"), if reorder { "Yes" } else { "" }]));
        } else {
            rows.push(json!([s.str("Item No."), s.str("Location Code"), s.str("Variant Code"), s.str("Description"), s.str("Shelf No."), n("Inventory"), format!("{:.2}", s.get("Unit Cost").as_dec())]));
        }
    }
    Ok(if availability {
        json!({"report": {"title": "Inventory Availability", "columns": ["Item No.", "Location Code", "Variant Code", "Description", "Inventory", "Gross Requirement", "Scheduled Receipt", "Projected Available Balance", "Reorder Point", "Reorder"], "rows": rows}})
    } else {
        json!({"report": {"title": "Inventory - List", "columns": ["Item No.", "Location Code", "Variant Code", "Description", "Shelf No.", "Inventory", "Unit Cost"], "rows": rows}})
    })
}

fn inventory_list(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    inventory_report(ctx, p, false)
}

fn inventory_availability(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    inventory_report(ctx, p, true)
}

fn s_report() -> Json {
    json!({"type": "object", "properties": {"item_no": {"type": "string"}}})
}

fn s_create() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}, "keys": {"type": "array"},
        "location_code": {"type": "string", "description": "only this location (blank: every location)"},
        "replace_previous": {"type": "boolean", "description": "Replace Previous SKUs"}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "item.create_sku", label: "Create Stockkeeping Unit", group: "Inventory", summary: "Create a stockkeeping unit of the item for each location.", mutates: true, params: s_create, run: create },
    CommandDef { id: "sku.inventory_list", label: "Inventory - List", group: "Inventory", summary: "The stockkeeping units with their inventory and unit cost.", mutates: false, params: s_report, run: inventory_list },
    CommandDef { id: "sku.inventory_availability", label: "Inventory Availability", group: "Inventory", summary: "The stockkeeping units' inventory, requirements, receipts and projected balance.", mutates: false, params: s_report, run: inventory_availability },
];
