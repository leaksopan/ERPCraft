//! One row of a table, and its primary key.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::table::TableDef;
use crate::value::Value;

/// A row: field name → value. Fields the record doesn't hold read as [`Value::Null`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    values: BTreeMap<String, Value>,
}

static NULL: Value = Value::Null;

impl Record {
    pub fn get(&self, field: &str) -> &Value {
        self.values.get(field).unwrap_or(&NULL)
    }

    pub fn set(&mut self, field: &str, v: Value) {
        self.values.insert(field.to_string(), v);
    }

    pub fn str(&self, field: &str) -> &str {
        self.get(field).as_str()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    /// The record's primary key under `table`.
    pub fn key(&self, table: &TableDef) -> Key {
        Key(table.primary_key.iter().map(|k| self.get(k).clone()).collect())
    }

    /// JSON object in the table's field order (stored fields and calculated ones alike).
    pub fn to_json(&self, table: &TableDef) -> Json {
        let mut m = Map::new();
        for f in &table.fields {
            m.insert(f.name.clone(), self.get(&f.name).to_json());
        }
        Json::Object(m)
    }

    /// JSON object holding only `fields`.
    pub fn to_json_fields(&self, fields: &[String]) -> Json {
        let mut m = Map::new();
        for f in fields {
            m.insert(f.clone(), self.get(f).to_json());
        }
        Json::Object(m)
    }

    /// Reads a stored record, coercing each value to its field's type. Unknown fields are dropped;
    /// a value that doesn't fit its type is an error naming the field.
    pub fn from_json(table: &TableDef, j: &Json) -> Result<Record, String> {
        let Some(obj) = j.as_object() else {
            return Err(format!("a {} record must be a JSON object", table.name));
        };
        let mut r = table.init_record();
        for f in &table.fields {
            if f.is_flow() {
                continue;
            }
            if let Some(v) = obj.get(&f.name) {
                let v = Value::from_json(&f.ty, v).map_err(|e| format!("{}.{}: {e}", table.name, f.name))?;
                r.set(&f.name, v);
            }
        }
        Ok(r)
    }
}

/// A primary key value: one [`Value`] per key field, ordered the way BC sorts by primary key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(pub Vec<Value>);

impl Key {
    /// `Order|S-ORD1001` style text, used in messages and as the CLI's key syntax.
    pub fn display(&self) -> String {
        self.0.iter().map(ToString::to_string).collect::<Vec<_>>().join("|")
    }

    /// Reads a key for `table` from JSON: an array (one value per key field), an object keyed by
    /// field name, or, for single-field keys, a bare value. A string with `|` separators is also
    /// accepted for multi-field keys.
    pub fn from_json(table: &TableDef, j: &Json) -> Result<Key, String> {
        let parts: Vec<Json> = match j {
            Json::Array(a) => a.clone(),
            Json::Object(o) => table.primary_key.iter().map(|k| o.get(k).cloned().unwrap_or(Json::Null)).collect(),
            Json::String(s) if table.primary_key.len() > 1 => s.split('|').map(|p| Json::String(p.to_string())).collect(),
            other => vec![other.clone()],
        };
        if parts.len() != table.primary_key.len() {
            return Err(format!(
                "{} has a key of {} field(s) ({}), got {}",
                table.name,
                table.primary_key.len(),
                table.primary_key.join(", "),
                parts.len()
            ));
        }
        let mut out = Vec::with_capacity(parts.len());
        for (name, v) in table.primary_key.iter().zip(parts.iter()) {
            let Some(f) = table.field(name) else {
                return Err(format!("{}: key field {name} is not defined", table.name));
            };
            out.push(Value::from_json(&f.ty, v).map_err(|e| format!("{}.{name}: {e}", table.name))?);
        }
        Ok(Key(out))
    }

    pub fn to_json(&self) -> Json {
        Json::Array(self.0.iter().map(Value::to_json).collect())
    }
}
