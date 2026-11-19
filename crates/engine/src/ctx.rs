//! The context every trigger, posting routine and command runs in.

use chrono::NaiveDate;
use erpcraft_meta::{Key, Metadata, Record, TableDef, Value};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::db::Database;
use crate::error::{EngineError, Result};

pub struct Ctx<'a> {
    pub meta: &'static Metadata,
    pub db: &'a mut Database,
    pub work_date: NaiveDate,
    pub user: &'a str,
}

impl Ctx<'_> {
    pub fn table(&self, name: &str) -> Result<&'static TableDef> {
        self.meta.table(name).ok_or_else(|| EngineError::not_found(format!("Table '{name}' does not exist.")))
    }

    /// A record by key, or BC's "does not exist" error.
    pub fn get(&self, table: &str, key: &Key) -> Result<Record> {
        let t = self.table(table)?;
        self.db.get(&t.name, key).cloned().ok_or_else(|| {
            EngineError::not_found(format!(
                "The {} does not exist. Identification fields and values: {}",
                t.caption,
                crate::db::key_text(t, key)
            ))
        })
    }

    /// A record of a one-Code-key table (Customer, Item, G/L Account, …).
    pub fn get_code(&self, table: &str, code: &str) -> Result<Record> {
        self.get(table, &Key(vec![Value::text(code)]))
    }

    /// The single record of a setup table (key `''`), or an initialised one when it doesn't exist.
    pub fn setup(&self, table: &str) -> Result<Record> {
        let t = self.table(table)?;
        Ok(self.db.get(&t.name, &Key(vec![Value::text("")])).cloned().unwrap_or_else(|| t.init_record()))
    }

    pub fn put(&mut self, table: &str, r: Record) -> Result<()> {
        let t = self.table(table)?;
        let key = r.key(t);
        if self.db.exists(&t.name, &key) { self.db.modify(t, r) } else { self.db.insert(t, r).map(|_| ()) }
    }

    pub fn insert(&mut self, table: &str, r: Record) -> Result<Key> {
        let t = self.table(table)?;
        self.db.insert(t, r)
    }
}

/// AL `TestField(Field)`: refuse when `field` of `r` is blank.
pub fn test_field(t: &TableDef, r: &Record, field: &str) -> Result<()> {
    if r.get(field).is_blank() {
        let key = crate::db::key_text(t, &r.key(t));
        return Err(EngineError::refused(format!("{field} must have a value in {}: {key}. It cannot be zero or empty.", t.caption)));
    }
    Ok(())
}

/// AL `TestField(Field, Value)`.
pub fn test_field_eq(t: &TableDef, r: &Record, field: &str, expected: &str) -> Result<()> {
    if r.str(field) != expected {
        let key = crate::db::key_text(t, &r.key(t));
        return Err(EngineError::refused(format!(
            "{field} must be equal to '{expected}' in {}: {key}. Current value is '{}'.",
            t.caption,
            r.str(field)
        )));
    }
    Ok(())
}

/// Rounds an amount to the currency precision (0.01).
pub fn round_amount(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

/// Rounds a unit amount (price / cost) to 5 decimals.
pub fn round_unit(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(5, RoundingStrategy::MidpointAwayFromZero)
}

pub fn dec(d: Decimal) -> Value {
    Value::Dec(d)
}
