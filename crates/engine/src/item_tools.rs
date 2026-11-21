//! Functions on BC's Item Card: Adjust Inventory (page 1327) and Copy Item.

use erpcraft_meta::{Key, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::item_journal;
use crate::records;
use crate::refuse;
use crate::schema::inventory::{ITEM_JNL_BATCH, ITEM_JNL_LINE, ITEM_JNL_TEMPLATE};
use crate::schema::tables::ITEM;

fn item_no<'a>(p: &'a Json) -> Result<&'a str> {
    if opt_str(p, "table") == Some(ITEM)
        && let Some(k) = p.get("key")
    {
        return k.as_str().or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str)).ok_or_else(|| EngineError::invalid("bad item key"));
    }
    opt_str(p, "item_no").or_else(|| opt_str(p, "no")).ok_or_else(|| EngineError::invalid("missing parameter 'item_no'"))
}

/// BC's Adjust Inventory: the new quantity on hand at a location becomes a positive or negative
/// adjustment, posted at once through the user's own item journal batch (so the shared journals
/// keep their lines).
fn adjust_inventory(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let item = item_no(p)?.to_string();
    ctx.get_code(ITEM, &item)?;
    let location = opt_str(p, "location_code").unwrap_or("").to_string();
    let new_qty = match p.get("new_inventory") {
        Some(Json::Number(n)) => n.to_string(),
        Some(Json::String(s)) => s.trim().to_string(),
        _ => refuse!("Enter the new inventory."),
    };
    let new_qty: Decimal = new_qty.replace(',', "").parse().map_err(|_| EngineError::invalid(format!("'{new_qty}' is not a number.")))?;
    if new_qty.is_sign_negative() {
        refuse!("The new inventory cannot be negative.");
    }
    let current = item_journal::stock(ctx, &item, if location.is_empty() { None } else { Some(&location) });
    let delta = new_qty - current;
    if delta.is_zero() {
        return Ok(json!({"message": format!("Item {item} already has {new_qty} on hand.")}));
    }
    let template = ctx
        .db
        .rows(ITEM_JNL_TEMPLATE)
        .find(|t| t.str("Type") == "Item")
        .map(|t| t.str("Name").to_string())
        .ok_or_else(|| EngineError::not_found("There is no item journal template of type Item."))?;
    let batch: String = format!("ADJ-{}", ctx.user.to_uppercase()).chars().take(10).collect();
    let batch_key = Key(vec![Value::text(&template), Value::text(&batch)]);
    if ctx.db.get(ITEM_JNL_BATCH, &batch_key).is_none() {
        let bt = ctx.table(ITEM_JNL_BATCH)?;
        let mut v = Map::new();
        v.insert("Journal Template Name".into(), json!(template));
        v.insert("Name".into(), json!(batch));
        v.insert("Description".into(), json!("Adjust Inventory"));
        records::create(ctx, bt, &v, None)?;
    }
    let lt = ctx.table(ITEM_JNL_LINE)?;
    let mut v = Map::new();
    v.insert("Journal Template Name".into(), json!(template));
    v.insert("Journal Batch Name".into(), json!(batch));
    v.insert("Document No.".into(), json!(format!("ADJ-{item}")));
    v.insert("Entry Type".into(), json!(if delta.is_sign_positive() { "Positive Adjmt." } else { "Negative Adjmt." }));
    v.insert("Item No.".into(), json!(item));
    if !location.is_empty() {
        v.insert("Location Code".into(), json!(location));
    }
    v.insert("Quantity".into(), json!(delta.abs().to_string()));
    records::create(ctx, lt, &v, None)?;
    item_journal::post(ctx, &template, &batch)?;
    Ok(json!({"message": format!("The inventory of item {item} was adjusted to {new_qty}."), "adjusted_by": delta.to_string()}))
}

/// BC's Copy Item: a new item with this one's general information (BC also offers units of
/// measure, prices and more, which ERPCraft doesn't keep per item yet).
fn copy_item(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let source_no = item_no(p)?.to_string();
    let source = ctx.get_code(ITEM, &source_no)?;
    let t = ctx.table(ITEM)?;
    let target_no = opt_str(p, "target_no").unwrap_or("").trim().to_uppercase();
    if !target_no.is_empty() && ctx.db.get(ITEM, &Key(vec![Value::text(&target_no)])).is_some() {
        refuse!("The Item already exists. Identification fields and values: No.='{target_no}'");
    }
    let mut v = Map::new();
    if !target_no.is_empty() {
        v.insert("No.".into(), json!(target_no));
    }
    for f in t.fields.iter().filter(|f| !f.is_flow() && f.editable && !matches!(f.name.as_str(), "No." | "Last Date Modified" | "Last Time Modified")) {
        let value = source.get(&f.name);
        if !matches!(value, Value::Null) && !value.to_string().is_empty() {
            v.insert(f.name.clone(), value.to_json());
        }
    }
    let created = records::create(ctx, t, &v, None)?;
    let no = created.str("No.").to_string();
    Ok(json!({
        "no": no,
        "open": {"page": "item-card", "key": no},
        "message": format!("Item {no} was created as a copy of {source_no}."),
    }))
}

fn s_adjust() -> Json {
    json!({"type": "object", "properties": {
        "item_no": {"type": "string"},
        "location_code": {"type": "string", "description": "blank: no location"},
        "new_inventory": {"type": "number", "description": "the quantity on hand after the adjustment"}},
        "required": ["item_no", "new_inventory"]})
}

fn s_copy() -> Json {
    json!({"type": "object", "properties": {
        "item_no": {"type": "string"},
        "target_no": {"type": "string", "description": "blank: the next number from the item number series"}},
        "required": ["item_no"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "item.adjust_inventory",
        label: "Adjust Inventory",
        group: "Inventory",
        summary: "Set an item's quantity on hand at a location; the difference is posted as an adjustment.",
        mutates: true,
        params: s_adjust,
        run: adjust_inventory,
    },
    CommandDef { id: "item.copy", label: "Copy Item", group: "Inventory", summary: "Create a new item from an existing one.", mutates: true, params: s_copy, run: copy_item },
];
