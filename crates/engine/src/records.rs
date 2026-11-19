//! Record operations with validation and triggers: what AL's `Validate`, `Insert(true)`,
//! `Modify(true)` and `Delete(true)` do.

use erpcraft_meta::{FieldType, Key, PageDef, Record, TableDef, Value};
use serde_json::{Map, Value as Json};

use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::triggers;

/// Sets `field` of `r` from JSON, checking type, editability and table relation, then runs the
/// field's `OnValidate` trigger. `user` = true for values coming from a user / agent (read-only
/// fields refused); false for code setting fields.
pub fn validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str, v: &Json, user: bool) -> Result<()> {
    let Some(f) = t.field(field) else {
        return Err(EngineError::not_found(format!("Table {} has no field '{field}'.", t.caption)));
    };
    if f.is_flow() {
        return Err(EngineError::refused(format!("{} is calculated and cannot be changed.", f.caption)));
    }
    if user && !f.editable {
        return Err(EngineError::refused(format!("{} is read-only.", f.caption)));
    }
    let value = Value::from_json(&f.ty, v).map_err(|e| EngineError::invalid(format!("{}: {e}", f.caption)))?;
    check_relation(ctx, t, &f.name, f.relation.as_deref(), &value)?;
    let old = r.get(&f.name).clone();
    r.set(&f.name, value);
    triggers::on_validate(ctx, t, r, &f.name, &old)
}

fn check_relation(ctx: &Ctx, t: &TableDef, field: &str, rel: Option<&str>, v: &Value) -> Result<()> {
    let Some(rel) = rel else { return Ok(()) };
    if v.is_blank() {
        return Ok(());
    }
    let rt = ctx.table(rel)?;
    if ctx.db.get(&rt.name, &Key(vec![v.clone()])).is_none() {
        return Err(EngineError::refused(format!(
            "The field {field} of table {} contains a value ({v}) that cannot be found in the related table ({}).",
            t.caption, rt.caption
        )));
    }
    Ok(())
}

pub fn insert(ctx: &mut Ctx, t: &TableDef, mut r: Record) -> Result<Record> {
    triggers::on_insert(ctx, t, &mut r)?;
    // A one-field Code key (No., Code) must have a value. Setup tables (key '') and multi-field
    // keys (posting setups, lines) may hold blanks, as in BC.
    if let [k] = t.primary_key.as_slice()
        && k != "Primary Key"
        && matches!(t.field(k).map(|f| &f.ty), Some(FieldType::Code(_)))
        && r.get(k).is_blank()
    {
        return Err(EngineError::refused(format!("{k} must have a value in {}. It cannot be zero or empty.", t.caption)));
    }
    ctx.db.insert(t, r.clone())?;
    Ok(r)
}

pub fn modify(ctx: &mut Ctx, t: &TableDef, mut r: Record) -> Result<Record> {
    triggers::on_modify(ctx, t, &mut r)?;
    ctx.db.modify(t, r.clone())?;
    Ok(r)
}

pub fn delete(ctx: &mut Ctx, t: &TableDef, key: &Key) -> Result<()> {
    let r = ctx.get(&t.name, key)?;
    triggers::on_delete(ctx, t, &r)?;
    ctx.db.remove(&t.name, key);
    Ok(())
}

/// Creates a record the way a card page does: initialise, apply the page's view filters and the
/// key fields, insert (number series, defaults), then validate the other given fields in table
/// field order and modify.
pub fn create(ctx: &mut Ctx, t: &TableDef, values: &Map<String, Json>, page: Option<&PageDef>) -> Result<Record> {
    let mut r = t.init_record();
    if let Some(p) = page {
        for vf in &p.view {
            if let Some(f) = t.field(&vf.field) {
                let v = Value::from_json(&f.ty, &Json::String(vf.value.clone())).map_err(EngineError::invalid)?;
                r.set(&f.name, v);
            }
        }
    }
    for k in &t.primary_key {
        if let Some(v) = lookup(values, k) {
            let Some(f) = t.field(k) else { continue };
            let v = Value::from_json(&f.ty, v).map_err(|e| EngineError::invalid(format!("{}: {e}", f.caption)))?;
            r.set(&f.name, v);
        }
    }
    let mut r = insert(ctx, t, r)?;
    let mut changed = false;
    for f in &t.fields {
        if t.primary_key.contains(&f.name) {
            continue;
        }
        if let Some(v) = lookup(values, &f.name) {
            validate(ctx, t, &mut r, &f.name, v, true)?;
            changed = true;
        }
    }
    unknown_fields(t, values)?;
    if changed {
        r = modify(ctx, t, r)?;
    }
    Ok(r)
}

/// Validates `values` into an existing record (in table field order) and modifies it.
pub fn update(ctx: &mut Ctx, t: &TableDef, key: &Key, values: &Map<String, Json>) -> Result<Record> {
    unknown_fields(t, values)?;
    let mut r = ctx.get(&t.name, key)?;
    for f in &t.fields {
        if let Some(v) = lookup(values, &f.name) {
            if t.primary_key.contains(&f.name) {
                let cur = r.get(&f.name).to_json();
                if &cur != v {
                    return Err(EngineError::refused(format!("{} is part of the primary key and cannot be changed here.", f.caption)));
                }
                continue;
            }
            validate(ctx, t, &mut r, &f.name, v, true)?;
        }
    }
    modify(ctx, t, r)
}

fn lookup<'a>(values: &'a Map<String, Json>, field: &str) -> Option<&'a Json> {
    values.get(field).or_else(|| values.iter().find(|(k, _)| k.eq_ignore_ascii_case(field)).map(|(_, v)| v))
}

fn unknown_fields(t: &TableDef, values: &Map<String, Json>) -> Result<()> {
    if let Some(k) = values.keys().find(|k| t.field(k).is_none()) {
        return Err(EngineError::not_found(format!("Table {} has no field '{k}'.", t.caption)));
    }
    Ok(())
}
