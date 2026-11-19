//! The in-memory company database a session works on.
//!
//! Rows by primary key, per table. Validation, triggers and posting live above it
//! ([`crate::records`], [`crate::posting`]); persistence lives below it ([`crate::store`]: a
//! PostgreSQL database or a JSON file). Every insert, modify and remove is recorded as a change,
//! so a store writes only the rows a command touched.

use std::collections::BTreeMap;
use std::path::Path;

use erpcraft_meta::{Key, Metadata, Record, TableDef, Value};
use serde_json::{Map, Value as Json, json};

use crate::error::{EngineError, Result};

pub const FORMAT: &str = "erpcraft-company";
pub const FORMAT_VERSION: i64 = 1;
/// Largest data file read (bytes). A company file is JSON text; anything bigger is refused rather
/// than read into memory.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Hidden field holding a row's version in the store (PostgreSQL `"$version"`): 0 for a row that
/// was never stored. Used for optimistic concurrency, like BC's SystemRowVersion.
pub const VERSION_FIELD: &str = "$version";

/// Rows changed since the last save: (table, key) → the row's stored version before the change
/// (0 for a row that is new).
pub type Changes = BTreeMap<(String, Key), i64>;

#[derive(Clone, Debug, Default)]
pub struct Database {
    tables: BTreeMap<String, BTreeMap<Key, Record>>,
    changes: Changes,
}

static EMPTY: BTreeMap<Key, Record> = BTreeMap::new();

impl Database {
    pub fn rows(&self, table: &str) -> impl DoubleEndedIterator<Item = &Record> {
        self.tables.get(table).unwrap_or(&EMPTY).values()
    }

    pub fn count(&self, table: &str) -> usize {
        self.tables.get(table).map_or(0, BTreeMap::len)
    }

    pub fn get(&self, table: &str, key: &Key) -> Option<&Record> {
        self.tables.get(table)?.get(key)
    }

    /// A record of a table with a one-field Code key.
    pub fn get_code(&self, table: &str, code: &str) -> Option<&Record> {
        self.get(table, &Key(vec![Value::text(code)]))
    }

    pub fn exists(&self, table: &str, key: &Key) -> bool {
        self.get(table, key).is_some()
    }

    /// Inserts a new row; refuses a duplicate key (AL `Insert(true)` error text).
    pub fn insert(&mut self, t: &TableDef, r: Record) -> Result<Key> {
        let key = r.key(t);
        let rows = self.tables.entry(t.name.clone()).or_default();
        if rows.contains_key(&key) {
            return Err(EngineError::refused(format!(
                "The record in table {} already exists. Identification fields and values: {}",
                t.caption,
                key_text(t, &key)
            )));
        }
        rows.insert(key.clone(), r);
        self.changes.entry((t.name.clone(), key.clone())).or_insert(0);
        Ok(key)
    }

    /// Replaces an existing row (the key must not change).
    pub fn modify(&mut self, t: &TableDef, r: Record) -> Result<()> {
        let key = r.key(t);
        let rows = self.tables.entry(t.name.clone()).or_default();
        match rows.get_mut(&key) {
            Some(slot) => {
                let version = slot.get(VERSION_FIELD).as_int();
                let mut r = r;
                r.set(VERSION_FIELD, Value::Int(version));
                *slot = r;
                self.changes.entry((t.name.clone(), key)).or_insert(version);
                Ok(())
            }
            None => Err(EngineError::not_found(format!(
                "The {} does not exist. Identification fields and values: {}",
                t.caption,
                key_text(t, &key)
            ))),
        }
    }

    pub fn remove(&mut self, table: &str, key: &Key) -> Option<Record> {
        let r = self.tables.get_mut(table)?.remove(key)?;
        self.changes.entry((table.to_string(), key.clone())).or_insert(r.get(VERSION_FIELD).as_int());
        Some(r)
    }

    /// Removes every row (recorded as deletions, so a store deletes them too).
    pub fn clear(&mut self) {
        let keys: Vec<(String, Key)> =
            self.tables.iter().flat_map(|(t, rows)| rows.keys().map(move |k| (t.clone(), k.clone()))).collect();
        for (t, k) in keys {
            self.remove(&t, &k);
        }
    }

    /// The changes since the last call, emptying the list.
    pub fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Puts a row read from the store (insert or replace) without recording a change.
    pub fn load_row(&mut self, t: &TableDef, r: Record) {
        let key = r.key(t);
        self.tables.entry(t.name.clone()).or_default().insert(key, r);
    }

    /// Drops a row another session deleted, without recording a change.
    pub fn unload_row(&mut self, table: &str, key: &Key) {
        if let Some(rows) = self.tables.get_mut(table) {
            rows.remove(key);
        }
    }

    /// Records the version the store gave a row after saving it.
    pub fn set_version(&mut self, table: &str, key: &Key, version: i64) {
        if let Some(r) = self.tables.get_mut(table).and_then(|rows| rows.get_mut(key)) {
            r.set(VERSION_FIELD, Value::Int(version));
        }
    }

    /// The next `Entry No.` of a ledger table (last + 1).
    pub fn next_entry_no(&self, table: &str) -> i64 {
        self.rows(table).next_back().map_or(0, |r| r.get("Entry No.").as_int()).saturating_add(1)
    }

    pub fn to_json(&self, meta: &Metadata) -> Json {
        let mut tables = Map::new();
        for (name, rows) in &self.tables {
            let Some(t) = meta.table(name) else { continue };
            if rows.is_empty() {
                continue;
            }
            let stored: Vec<Json> = rows
                .values()
                .map(|r| {
                    let mut m = Map::new();
                    for f in t.fields.iter().filter(|f| !f.is_flow()) {
                        let v = r.get(&f.name);
                        if !v.is_blank() || t.primary_key.contains(&f.name) {
                            m.insert(f.name.clone(), v.to_json());
                        }
                    }
                    Json::Object(m)
                })
                .collect();
            tables.insert(name.clone(), Json::Array(stored));
        }
        json!({"format": FORMAT, "version": FORMAT_VERSION, "tables": tables})
    }

    pub fn from_json(meta: &Metadata, j: &Json) -> Result<Database> {
        if j.get("format").and_then(Json::as_str) != Some(FORMAT) {
            return Err(EngineError::invalid("not an ERPCraft company file (missing \"format\")"));
        }
        let version = j.get("version").and_then(Json::as_i64).unwrap_or(0);
        if version > FORMAT_VERSION {
            return Err(EngineError::invalid(format!(
                "the company file is version {version}; this build reads up to {FORMAT_VERSION}"
            )));
        }
        let mut db = Database::default();
        let Some(tables) = j.get("tables").and_then(Json::as_object) else {
            return Ok(db);
        };
        for (name, rows) in tables {
            // Tables this build doesn't know are skipped, so a newer extension's data doesn't
            // stop the company from opening.
            let Some(t) = meta.table(name) else { continue };
            let Some(rows) = rows.as_array() else { continue };
            for row in rows {
                let r = Record::from_json(t, row).map_err(EngineError::invalid)?;
                db.insert(t, r)?;
            }
        }
        Ok(db)
    }

    pub fn load(meta: &Metadata, path: &Path) -> Result<Database> {
        let len = std::fs::metadata(path).map_err(|e| EngineError::io(format!("{}: {e}", path.display())))?.len();
        if len > MAX_FILE_BYTES {
            return Err(EngineError::io(format!("{}: file is larger than {MAX_FILE_BYTES} bytes", path.display())));
        }
        let text = std::fs::read_to_string(path).map_err(|e| EngineError::io(format!("{}: {e}", path.display())))?;
        let j: Json =
            serde_json::from_str(&text).map_err(|e| EngineError::invalid(format!("{}: {e}", path.display())))?;
        Database::from_json(meta, &j)
    }

    /// Writes the data file atomically (temp file + rename), so a crash mid-write never leaves a
    /// half-written company behind.
    pub fn save(&self, meta: &Metadata, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(&self.to_json(meta)).map_err(|e| EngineError::internal(e.to_string()))?;
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::io(format!("{}: {e}", dir.display())))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| EngineError::io(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path).map_err(|e| EngineError::io(format!("{}: {e}", path.display())))
    }
}

/// `Document Type='Order',No.='S-ORD1001'` (BC's identification text).
pub fn key_text(t: &TableDef, key: &Key) -> String {
    t.primary_key.iter().zip(key.0.iter()).map(|(f, v)| format!("{f}='{v}'")).collect::<Vec<_>>().join(",")
}
