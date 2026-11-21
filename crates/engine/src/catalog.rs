//! Catalog items (BC's Catalog Item Management, codeunit 5703 NonstockItemMgt): items a vendor
//! offers that are not kept on stock until one is turned into a real item from its template.

use erpcraft_meta::{Record, TableDef, Value};
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::catalog::{ITEM_TEMPL, NONSTOCK_ITEM, NONSTOCK_SETUP, TEMPLATE_FIELDS};
use crate::schema::tables::{INVENTORY_SETUP, ITEM};

pub fn on_insert(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<bool> {
    if t.name != NONSTOCK_ITEM {
        return Ok(false);
    }
    if r.str("Entry No.").is_empty() {
        let series = crate::noseries::series_from_setup(ctx, INVENTORY_SETUP, "Nonstock Item Nos.")?;
        r.set("Entry No.", Value::text(crate::noseries::next(ctx, &series)?));
        r.set("No. Series", Value::text(&series));
    }
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    Ok(true)
}

pub fn on_validate(ctx: &Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    if t.name != NONSTOCK_ITEM {
        return Ok(());
    }
    if !r.str("Item No.").is_empty() && matches!(field, "Vendor No." | "Vendor Item No." | "Manufacturer Code" | "Unit of Measure" | "Item Templ. Code") {
        refuse!("You cannot change {field} because an item exists for this catalog item.");
    }
    if field == "Item Templ. Code"
        && let Some(tpl) = ctx.db.get_code(ITEM_TEMPL, r.str("Item Templ. Code"))
        && r.str("Item Category Code").is_empty()
    {
        r.set("Item Category Code", tpl.get("Item Category Code").clone());
    }
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    Ok(())
}

/// The new item's number by the setup's No. Format.
fn item_no(ctx: &mut Ctx, c: &Record) -> Result<String> {
    let setup = ctx.db.rows(NONSTOCK_SETUP).next().cloned();
    let format = setup.as_ref().map_or("Vendor Item No.", |s| s.str("No. Format")).to_string();
    let sep = setup.as_ref().map(|s| s.str("No. Format Separator").to_string()).unwrap_or_default();
    let (vendor_item, mfr) = (c.str("Vendor Item No."), c.str("Manufacturer Code"));
    let no = match format.as_str() {
        "Mfr. + Vendor Item No." => format!("{mfr}{sep}{vendor_item}"),
        "Vendor Item No. + Mfr." => format!("{vendor_item}{sep}{mfr}"),
        "Entry No." => c.str("Entry No.").to_string(),
        "Item No. Series" => {
            let series = crate::noseries::series_from_setup(ctx, INVENTORY_SETUP, "Item Nos.")?;
            crate::noseries::next(ctx, &series)?
        }
        _ => vendor_item.to_string(),
    };
    Ok(no.to_uppercase())
}

/// BC's Create Item (NonstockItemMgt.NonstockAutoItem): an item from the catalog item and its
/// template, linked back on the catalog item.
fn create_item(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let entry = match (opt_str(p, "table"), p.get("key")) {
        (Some(NONSTOCK_ITEM), Some(k)) => k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)),
        _ => opt_str(p, "entry_no").map(str::to_string),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'entry_no'"))?;
    let mut c = ctx.get_code(NONSTOCK_ITEM, &entry)?;
    if !c.str("Item No.").is_empty() {
        refuse!("Item {} already exists.", c.str("Item No."));
    }
    for f in ["Vendor Item No.", "Item Templ. Code", "Description", "Unit of Measure"] {
        if c.str(f).is_empty() {
            refuse!("{f} must have a value in Catalog Item: Entry No.={entry}. It cannot be zero or empty.");
        }
    }
    let tpl = ctx.get_code(ITEM_TEMPL, c.str("Item Templ. Code"))?;
    let no = item_no(ctx, &c)?;
    if ctx.db.get_code(ITEM, &no).is_some() {
        refuse!("Item {no} already exists.");
    }
    let mut values = Map::new();
    values.insert("No.".into(), json!(no));
    for f in TEMPLATE_FIELDS {
        let v = tpl.get(f);
        if !v.to_string().is_empty() {
            values.insert((*f).to_string(), v.to_json());
        }
    }
    values.insert("Description".into(), json!(c.str("Description")));
    values.insert("Base Unit of Measure".into(), json!(c.str("Unit of Measure")));
    for (from, to) in [("Vendor No.", "Vendor No."), ("Vendor Item No.", "Vendor Item No."), ("Item Category Code", "Item Category Code")] {
        if !c.str(from).is_empty() {
            values.insert(to.into(), json!(c.str(from)));
        }
    }
    let it = ctx.table(ITEM)?;
    let mut item = crate::records::create(ctx, it, &values, None)?;
    for (from, to) in [("Unit Price", "Unit Price"), ("Negotiated Cost", "Unit Cost"), ("Negotiated Cost", "Last Direct Cost"), ("Net Weight", "Net Weight"), ("Gross Weight", "Gross Weight")] {
        item.set(to, c.get(from).clone());
    }
    item.set("Created From Nonstock Item", Value::Bool(true));
    ctx.db.modify(it, item)?;
    let ct = ctx.table(NONSTOCK_ITEM)?;
    c.set("Item No.", Value::text(&no));
    ctx.db.modify(ct, c)?;
    Ok(json!({"item_no": no, "open": {"page": "item-card", "key": no}, "message": format!("Item {no} was created.")}))
}

fn s_create() -> Json {
    json!({"type": "object", "properties": {"entry_no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "catalog.create_item",
    label: "Create Item",
    group: "Inventory",
    summary: "Turn a catalog item into an item, from its item template.",
    mutates: true,
    params: s_create,
    run: create_item,
}];
