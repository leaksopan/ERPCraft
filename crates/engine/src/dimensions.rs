//! Dimension sets (BC's DimensionManagement): a combination of dimension values is stored once
//! in Dimension Set Entry under a Dimension Set ID; documents, journals and entries keep the ID,
//! and their Global / Shortcut Dimension 1 and 2 fields show its first two dimensions.

use std::collections::BTreeMap;

use erpcraft_meta::{Key, Record, TableDef, Value};
use serde_json::{Map, json};

use crate::ctx::Ctx;
use crate::error::Result;
use crate::refuse;
use crate::schema::dimensions::{DEFAULT_DIM, DIM_SET_ENTRY, DIM_VALUE, DIMENSION, MASTERS};
use crate::side::Side;

/// Dimension code → value code.
pub type Dims = BTreeMap<String, String>;

const GL_SETUP: &str = "General Ledger Setup";

/// The G/L Setup's dimension for shortcut slot `n` (1 and 2 are the global dimensions).
pub fn shortcut_dimension(ctx: &Ctx, n: usize) -> String {
    let Some(setup) = ctx.db.rows(GL_SETUP).next() else { return String::new() };
    let field = match n {
        1 => "Global Dimension 1 Code",
        2 => "Global Dimension 2 Code",
        _ => return setup.str(&format!("Shortcut Dimension {n} Code")).to_string(),
    };
    setup.str(field).to_string()
}

/// Which shortcut slot (1–8) shows `dimension`, or 0.
pub fn global_no(ctx: &Ctx, dimension: &str) -> i64 {
    (1..=8).find(|n| !dimension.is_empty() && shortcut_dimension(ctx, *n).eq_ignore_ascii_case(dimension)).map_or(0, |n| n as i64)
}

pub fn set_entries(ctx: &Ctx, id: i64) -> Dims {
    if id == 0 {
        return Dims::new();
    }
    ctx.db
        .rows(DIM_SET_ENTRY)
        .filter(|e| e.get("Dimension Set ID").as_int() == id)
        .map(|e| (e.str("Dimension Code").to_string(), e.str("Dimension Value Code").to_string()))
        .collect()
}

/// The ID of the set holding exactly `dims` (blank values left out), created when new; 0 for none.
pub fn set_id(ctx: &mut Ctx, dims: &Dims) -> Result<i64> {
    let dims: Dims = dims.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.to_uppercase(), v.to_uppercase())).collect();
    if dims.is_empty() {
        return Ok(0);
    }
    let mut sets: BTreeMap<i64, Dims> = BTreeMap::new();
    for e in ctx.db.rows(DIM_SET_ENTRY) {
        sets.entry(e.get("Dimension Set ID").as_int()).or_default().insert(e.str("Dimension Code").to_string(), e.str("Dimension Value Code").to_string());
    }
    if let Some((id, _)) = sets.iter().find(|(_, s)| **s == dims) {
        return Ok(*id);
    }
    let id = sets.keys().max().copied().unwrap_or(0).saturating_add(1);
    let t = ctx.table(DIM_SET_ENTRY)?;
    for (dim, value) in &dims {
        let d = ctx.get_code(DIMENSION, dim)?;
        let Some(v) = ctx.db.get(DIM_VALUE, &Key(vec![Value::text(dim), Value::text(value)])).cloned() else {
            refuse!("The dimension value {value} has not been set up for dimension {dim}.");
        };
        if v.get("Blocked").as_bool() {
            refuse!("Dimension Value {dim} - {value} is blocked.");
        }
        let mut r = t.init_record();
        r.set("Dimension Set ID", Value::Int(id));
        r.set("Dimension Code", Value::text(dim));
        r.set("Dimension Value Code", Value::text(value));
        r.set("Dimension Name", d.get("Name").clone());
        r.set("Dimension Value Name", v.get("Name").clone());
        r.set("Global Dimension No.", Value::Int(global_no(ctx, dim)));
        ctx.db.insert(t, r)?;
    }
    Ok(id)
}

/// The default dimensions of a master record (Default Dimension).
pub fn defaults(ctx: &Ctx, table_id: u32, no: &str) -> Dims {
    if no.is_empty() {
        return Dims::new();
    }
    ctx.db
        .rows(DEFAULT_DIM)
        .filter(|d| d.get("Table ID").as_int() == i64::from(table_id) && d.str("No.").eq_ignore_ascii_case(no))
        .filter(|d| !d.str("Dimension Value Code").is_empty())
        .map(|d| (d.str("Dimension Code").to_string(), d.str("Dimension Value Code").to_string()))
        .collect()
}

/// The values of shortcut slots 1 and 2 in a set (Global Dimension 1 / 2 Code on entries).
pub fn globals(ctx: &Ctx, id: i64) -> (String, String) {
    let set = set_entries(ctx, id);
    let value = |n| set.get(&shortcut_dimension(ctx, n).to_uppercase()).cloned().unwrap_or_default();
    (value(1), value(2))
}

/// Stores `dims` on `r`: its Dimension Set ID and the slot 1 / 2 fields the table has.
pub fn apply(ctx: &mut Ctx, t: &TableDef, r: &mut Record, dims: &Dims) -> Result<()> {
    if t.field("Dimension Set ID").is_none() {
        return Ok(());
    }
    let id = set_id(ctx, dims)?;
    r.set("Dimension Set ID", Value::Int(id));
    let (d1, d2) = globals(ctx, id);
    for (n, v) in [(1, d1), (2, d2)] {
        for f in [format!("Shortcut Dimension {n} Code"), format!("Global Dimension {n} Code")] {
            if t.field(&f).is_some() {
                r.set(&f, Value::text(&v));
            }
        }
    }
    Ok(())
}

/// `Shortcut Dimension 2 Code` → 2.
fn slot(field: &str) -> Option<usize> {
    let rest = field.strip_prefix("Shortcut Dimension ").or_else(|| field.strip_prefix("Global Dimension "))?;
    let n: usize = rest.strip_suffix(" Code")?.parse().ok()?;
    (1..=8).contains(&n).then_some(n)
}

fn table_id(t: &TableDef) -> Option<u32> {
    MASTERS.iter().any(|(_, id)| *id == t.object_id).then_some(t.object_id)
}

/// Validating a dimension field: on a document, journal or entry it changes the record's set; on
/// a master it becomes the record's default dimension (BC's ValidateShortcutDimCode).
pub fn on_validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    let Some(n) = slot(field) else { return Ok(()) };
    let dimension = shortcut_dimension(ctx, n);
    let value = r.str(field).to_uppercase();
    if dimension.is_empty() {
        if !value.is_empty() {
            refuse!("Shortcut Dimension {n} Code must have a value in General Ledger Setup.");
        }
        return Ok(());
    }
    if !value.is_empty() && ctx.db.get(DIM_VALUE, &Key(vec![Value::text(&dimension), Value::text(&value)])).is_none() {
        refuse!("The dimension value {value} has not been set up for dimension {dimension}.");
    }
    r.set(field, Value::text(&value));
    if t.field("Dimension Set ID").is_some() {
        let mut dims = set_entries(ctx, r.get("Dimension Set ID").as_int());
        if value.is_empty() {
            dims.remove(&dimension);
        } else {
            dims.insert(dimension, value);
        }
        return apply(ctx, t, r, &dims);
    }
    if let Some(id) = table_id(t) {
        let no = r.str("No.").to_string();
        let key = Key(vec![Value::Int(i64::from(id)), Value::text(&no), Value::text(&dimension)]);
        let dt = ctx.table(DEFAULT_DIM)?;
        match (ctx.db.get(DEFAULT_DIM, &key).cloned(), value.is_empty()) {
            (Some(_), true) => {
                ctx.db.remove(DEFAULT_DIM, &key);
            }
            (Some(mut d), false) => {
                d.set("Dimension Value Code", Value::text(&value));
                ctx.db.modify(dt, d)?;
            }
            (None, false) => {
                let mut v = Map::new();
                v.insert("Table ID".into(), json!(id));
                v.insert("No.".into(), json!(no));
                v.insert("Dimension Code".into(), json!(dimension));
                v.insert("Dimension Value Code".into(), json!(value));
                crate::records::create(ctx, dt, &v, None)?;
            }
            (None, true) => {}
        }
    }
    Ok(())
}

/// After a document header's customer or vendor changes: the party's default dimensions.
pub fn after_header_validate(ctx: &mut Ctx, side: &Side, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    if field != side.party_no {
        return Ok(());
    }
    let dims = defaults(ctx, if side.sales { 18 } else { 23 }, r.str(side.party_no));
    apply(ctx, t, r, &dims)
}

/// After a line's No. changes: the header's dimensions with the item's or account's defaults on
/// top (BC combines the sources, the line's own taking priority).
pub fn after_line_validate(ctx: &mut Ctx, side: &Side, r: &mut Record, field: &str) -> Result<()> {
    if field != "No." {
        return Ok(());
    }
    let lt = ctx.table(side.line)?;
    let header = ctx.db.get(side.header, &crate::documents::header_key(r.str("Document Type"), r.str("Document No."))).cloned();
    let mut dims = header.map(|h| set_entries(ctx, h.get("Dimension Set ID").as_int())).unwrap_or_default();
    let source = match r.str("Type") {
        "Item" => Some(27),
        "G/L Account" => Some(15),
        "Fixed Asset" => Some(5600),
        _ => None,
    };
    if let Some(id) = source {
        dims.extend(defaults(ctx, id, r.str("No.")));
    }
    apply(ctx, lt, r, &dims)
}

#[cfg(test)]
mod tests {
    #[test]
    fn slots() {
        assert_eq!(super::slot("Shortcut Dimension 2 Code"), Some(2));
        assert_eq!(super::slot("Global Dimension 1 Code"), Some(1));
        assert_eq!(super::slot("Shortcut Dimension 9 Code"), None);
        assert_eq!(super::slot("Name"), None);
    }
}
