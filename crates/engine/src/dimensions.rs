//! Dimension sets (BC's DimensionManagement): a combination of dimension values is stored once
//! in Dimension Set Entry under a Dimension Set ID; documents, journals and entries keep the ID,
//! and their Global / Shortcut Dimension 1 and 2 fields show its first two dimensions.

use std::collections::BTreeMap;

use erpcraft_meta::{Key, Record, TableDef, Value};
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};

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

/// Dimension Set IDs whose `dimension` value matches `value_filter` (BC's
/// DimensionManagement.GetDimSetIDsForFilter). A filter that takes blank also takes the sets
/// without the dimension, including set 0.
pub fn set_ids_for_filter(ctx: &Ctx, dimension: &str, value_filter: &str) -> std::collections::BTreeSet<i64> {
    let ty = erpcraft_meta::FieldType::Code(20);
    let all: std::collections::BTreeSet<i64> = std::iter::once(0).chain(ctx.db.rows(DIM_SET_ENTRY).map(|e| e.get("Dimension Set ID").as_int())).collect();
    let mut with_dim = std::collections::BTreeSet::new();
    let mut out = std::collections::BTreeSet::new();
    for e in ctx.db.rows(DIM_SET_ENTRY).filter(|e| e.str("Dimension Code").eq_ignore_ascii_case(dimension)) {
        let id = e.get("Dimension Set ID").as_int();
        with_dim.insert(id);
        if crate::filter::matches(&ty, e.get("Dimension Value Code"), value_filter) {
            out.insert(id);
        }
    }
    if crate::filter::matches(&ty, &Value::text(""), value_filter) {
        out.extend(all.difference(&with_dim).copied());
    }
    out
}

/// BC's Set Dimension Filter: the Dimension Set ID filter for entries whose dimensions match
/// every (dimension, value filter) given.
fn set_filter(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut lines: Vec<(String, String)> = p
        .get("lines")
        .and_then(Json::as_array)
        .map(|ls| ls.iter().filter_map(|l| Some((l.get("dimension_code")?.as_str()?.to_string(), l.get("value_filter").and_then(Json::as_str).unwrap_or("").to_string()))).collect())
        .unwrap_or_default();
    if let Some(code) = opt_str(p, "dimension_code").filter(|c| !c.trim().is_empty()) {
        lines.push((code.trim().to_string(), opt_str(p, "value_filter").unwrap_or("").to_string()));
    }
    if lines.is_empty() {
        return Ok(json!({"filter": {"field": "Dimension Set ID", "value": ""}}));
    }
    let mut ids: Option<std::collections::BTreeSet<i64>> = None;
    for (code, values) in &lines {
        let code = code.to_uppercase();
        if ctx.db.rows(DIMENSION).all(|d| d.str("Code") != code) {
            refuse!("The Dimension does not exist. Identification fields and values: Code='{code}'");
        }
        let found = set_ids_for_filter(ctx, &code, values);
        ids = Some(match ids {
            None => found,
            Some(prev) => prev.intersection(&found).copied().collect(),
        });
    }
    let ids = ids.unwrap_or_default();
    // BC: no matching set filters everything out.
    let value = if ids.is_empty() { "0&<>0".to_string() } else { ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|") };
    Ok(json!({"filter": {"field": "Dimension Set ID", "value": value}, "sets": ids.len()}))
}

fn s_set_filter() -> Json {
    json!({"type": "object", "properties": {
        "dimension_code": {"type": "string"},
        "value_filter": {"type": "string", "description": "BC filter on the dimension value, e.g. SALES|ADM or blank"},
        "lines": {"type": "array", "items": {"type": "object", "properties": {"dimension_code": {"type": "string"}, "value_filter": {"type": "string"}}}}}})
}

/// The selected records of a master table: `keys` (several) or `key` (one).
fn selected_records(ctx: &Ctx, t: &TableDef, p: &Json) -> Result<Vec<Record>> {
    let keys: Vec<Json> = match (p.get("keys").and_then(Json::as_array), p.get("key")) {
        (Some(ks), _) if !ks.is_empty() => ks.clone(),
        (_, Some(k)) => vec![k.clone()],
        _ => refuse!("Select the records to set dimensions for."),
    };
    keys.iter().map(|k| ctx.get(&t.name, &Key::from_json(t, k).map_err(crate::error::EngineError::invalid)?)).collect()
}

/// BC's Default Dimensions-Multiple: one dimension's default value set on (or, with a blank
/// value, removed from) every selected record. A global dimension goes through the record's
/// Global Dimension field, so card and default dimension stay in step.
fn dimensions_multiple(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let table = opt_str(p, "table").ok_or_else(|| crate::error::EngineError::invalid("missing parameter 'table'"))?;
    let t = ctx.table(table)?.clone();
    let Some(id) = table_id(&t) else {
        refuse!("{} has no default dimensions.", t.caption);
    };
    let dimension = opt_str(p, "dimension_code").map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());
    let records = selected_records(ctx, &t, p)?;
    if let Some(dimension) = dimension {
        if ctx.db.get_code(DIMENSION, &dimension).is_none() {
            refuse!("The Dimension does not exist. Identification fields and values: Code='{dimension}'");
        }
        let value = opt_str(p, "dimension_value_code").unwrap_or("").trim().to_uppercase();
        if !value.is_empty() && ctx.db.get(DIM_VALUE, &Key(vec![Value::text(&dimension), Value::text(&value)])).is_none() {
            refuse!("The dimension value {value} has not been set up for dimension {dimension}.");
        }
        let n = global_no(ctx, &dimension);
        for mut r in records.clone() {
            let field = format!("Global Dimension {n} Code");
            if (n == 1 || n == 2) && t.field(&field).is_some() {
                crate::records::validate(ctx, &t, &mut r, &field, &json!(value), true)?;
                ctx.db.modify(&t, r)?;
                continue;
            }
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
    }
    // The page BC shows: each dimension any of them has, with its value, or (Conflict).
    let nos: Vec<String> = records.iter().map(|r| r.str("No.").to_string()).collect();
    let all: Vec<Dims> = nos.iter().map(|no| defaults(ctx, id, no)).collect();
    let codes: std::collections::BTreeSet<String> = all.iter().flat_map(|d| d.keys().cloned()).collect();
    let rows: Vec<Json> = codes
        .iter()
        .map(|c| {
            let values: std::collections::BTreeSet<String> = all.iter().map(|d| d.get(c).cloned().unwrap_or_default()).collect();
            let shown = if values.len() == 1 { values.into_iter().next().unwrap_or_default() } else { "(Conflict)".to_string() };
            json!([c, shown])
        })
        .collect();
    Ok(json!({"records": nos.len(), "report": {"title": format!("Default Dimensions-Multiple ({} {})", nos.len(), t.caption), "columns": ["Dimension Code", "Dimension Value Code"], "rows": rows}}))
}

fn s_multiple() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"}, "keys": {"type": "array", "description": "the selected records' keys"}, "key": {},
        "dimension_code": {"type": "string"}, "dimension_value_code": {"type": "string", "description": "blank removes the dimension"}}})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "dimension.set_filter",
    label: "Set Dimension Filter",
    group: "Finance",
    summary: "The Dimension Set ID filter that limits entries to the given dimension values.",
    mutates: false,
    params: s_set_filter,
    run: set_filter,
}, CommandDef {
    id: "dimension.multiple",
    label: "Dimensions-Multiple",
    group: "Finance",
    summary: "Set or remove a default dimension on several customers, vendors, items, accounts or fixed assets at once.",
    mutates: true,
    params: s_multiple,
    run: dimensions_multiple,
}];
