//! Where a company is kept. A session loads it once, runs commands on its in-memory copy, and asks
//! the store to write the rows each command changed.
//!
//! - [`postgres::Postgres`]: one PostgreSQL table per BC table, row versions for optimistic
//!   concurrency, and a change feed so sessions see each other's work. The production store.
//! - [`JsonFile`]: one JSON file, for demos, tests and offline use.
//! - [`Memory`]: nothing persisted.

#[cfg(feature = "postgres")]
pub mod postgres;

use std::path::{Path, PathBuf};

use erpcraft_meta::Metadata;

use crate::db::{Changes, Database};
use crate::error::Result;

pub trait Store: Send {
    /// Reads the whole company.
    fn load(&mut self, meta: &Metadata) -> Result<Database>;

    /// Writes `changes` (rows of `db`, or deletions) atomically. On success the stored row versions
    /// are recorded in `db`; on failure nothing is written.
    fn save(&mut self, meta: &Metadata, db: &mut Database, changes: &Changes) -> Result<()>;

    /// Applies what other sessions wrote since the last load / refresh. Returns whether anything
    /// changed.
    fn refresh(&mut self, _meta: &Metadata, _db: &mut Database) -> Result<bool> {
        Ok(false)
    }

    /// For logs and the UI: "postgres erpcraft@localhost:5432/erp", "file D:\x.json".
    fn describe(&self) -> String;
}

/// Keeps nothing.
pub struct Memory;

impl Store for Memory {
    fn load(&mut self, _meta: &Metadata) -> Result<Database> {
        Ok(Database::default())
    }
    fn save(&mut self, _meta: &Metadata, _db: &mut Database, _changes: &Changes) -> Result<()> {
        Ok(())
    }
    fn describe(&self) -> String {
        "memory".to_string()
    }
}

/// The whole company in one JSON file, rewritten atomically after each change.
pub struct JsonFile {
    path: PathBuf,
}

impl JsonFile {
    pub fn new(path: &Path) -> Self {
        Self { path: path.to_path_buf() }
    }
}

impl Store for JsonFile {
    fn load(&mut self, meta: &Metadata) -> Result<Database> {
        if self.path.exists() { Database::load(meta, &self.path) } else { Ok(Database::default()) }
    }
    fn save(&mut self, meta: &Metadata, db: &mut Database, changes: &Changes) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        db.save(meta, &self.path)
    }
    fn describe(&self) -> String {
        format!("file {}", self.path.display())
    }
}

/// Whether `source` names a PostgreSQL database rather than a file.
pub fn is_postgres_url(source: &str) -> bool {
    let s = source.trim_start();
    s.starts_with("postgres://") || s.starts_with("postgresql://")
}

/// Picks the company source the way every client does: `--db <url>`, else `ERPCRAFT_DATABASE_URL`,
/// else `--data <file>`, else `ERPCRAFT_DATA`, else `company.erpcraft.json` in the current
/// directory.
pub fn resolve_source(db_flag: Option<&str>, data_flag: Option<&str>) -> String {
    if let Some(db) = db_flag.filter(|s| !s.trim().is_empty()) {
        return db.to_string();
    }
    if let Ok(db) = std::env::var("ERPCRAFT_DATABASE_URL")
        && !db.trim().is_empty()
    {
        return db;
    }
    if let Some(f) = data_flag.filter(|s| !s.trim().is_empty()) {
        return f.to_string();
    }
    std::env::var("ERPCRAFT_DATA").unwrap_or_else(|_| "company.erpcraft.json".to_string())
}

/// A source safe to print: the password of a PostgreSQL URL replaced by `***`.
pub fn redact(source: &str) -> String {
    if !is_postgres_url(source) {
        return source.to_string();
    }
    match (source.find("://"), source.rfind('@')) {
        (Some(scheme), Some(at)) if at > scheme => {
            let creds = &source[scheme + 3..at];
            match creds.split_once(':') {
                Some((user, _)) => format!("{}{user}:***{}", &source[..scheme + 3], &source[at..]),
                None => source.to_string(),
            }
        }
        _ => source.to_string(),
    }
}
