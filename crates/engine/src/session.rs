//! A session on one company: the database, the work date, the user, and [`Session::run`], the
//! single entry point every client uses.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use erpcraft_meta::{Key, Metadata, PageDef, Record, TableDef};
use serde_json::{Value as Json, json};

use crate::commands::{self, CommandDef};
use crate::ctx::Ctx;
use crate::db::Database;
use crate::error::{EngineError, Result};
use crate::schema;

pub struct Session {
    db: Database,
    path: Option<PathBuf>,
    work_date: NaiveDate,
    user: String,
    /// Bumped after every successful mutating command, so clients can refresh cached views.
    revision: u64,
}

impl Session {
    /// An empty in-memory company (no data file).
    pub fn in_memory() -> Self {
        Self { db: Database::default(), path: None, work_date: today(), user: default_user(), revision: 0 }
    }

    /// Opens the company data file at `path`; a missing file starts an empty company that is saved
    /// there on the first change.
    pub fn open(path: &Path) -> Result<Self> {
        let db = if path.exists() { Database::load(schema::base_app(), path)? } else { Database::default() };
        Ok(Self { db, path: Some(path.to_path_buf()), work_date: today(), user: default_user(), revision: 0 })
    }

    pub fn meta(&self) -> &'static Metadata {
        schema::base_app()
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn work_date(&self) -> NaiveDate {
        self.work_date
    }

    pub fn set_work_date(&mut self, d: NaiveDate) {
        self.work_date = d;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn set_user(&mut self, u: &str) {
        self.user = u.to_string();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the company has any data yet (used to offer "Initialize company").
    pub fn is_empty(&self) -> bool {
        self.meta().tables.keys().all(|t| self.db.count(t) == 0)
    }

    pub fn commands(&self) -> &'static [CommandDef] {
        commands::all()
    }

    /// Runs a command. A failing (or panicking) command leaves the company exactly as it was:
    /// changes are made on a copy that replaces the data only on success, like a BC write
    /// transaction. Mutating commands save the data file afterwards.
    pub fn run(&mut self, id: &str, params: &Json) -> Result<Json> {
        let cmd = commands::find(id).ok_or_else(|| EngineError::not_found(format!("Unknown command '{id}'. Run command.list to see them.")))?;
        let params = if params.is_null() { json!({}) } else { params.clone() };
        if !params.is_object() {
            return Err(EngineError::invalid("command parameters must be a JSON object"));
        }
        let mut db = if cmd.mutates { self.db.clone() } else { Database::default() };
        let mut work_date = self.work_date;
        let user = self.user.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let target = if cmd.mutates { &mut db } else { &mut self.db };
            let mut ctx = Ctx { meta: schema::base_app(), db: target, work_date, user: &user };
            let r = (cmd.run)(&mut ctx, &params);
            work_date = ctx.work_date;
            r
        }));
        let result = match outcome {
            Ok(r) => r,
            Err(_) => Err(EngineError::internal(format!("internal error in command '{id}'; nothing was changed"))),
        }?;
        if cmd.mutates {
            self.db = db;
            self.revision = self.revision.wrapping_add(1);
            self.save()?;
        }
        if work_date != self.work_date {
            self.work_date = work_date;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(result)
    }

    /// Writes the data file (no-op in memory).
    pub fn save(&self) -> Result<()> {
        match &self.path {
            Some(p) => self.db.save(schema::base_app(), p),
            None => Ok(()),
        }
    }

    // ---- typed reads for clients that render (the egui UI) -----------------------------------

    pub fn table(&self, name: &str) -> Option<&'static TableDef> {
        self.meta().table(name)
    }

    pub fn page(&self, id: &str) -> Option<&'static PageDef> {
        self.meta().page(id)
    }

    /// A record with FlowFields calculated.
    pub fn record(&self, table: &str, key: &Key) -> Option<Record> {
        let t = self.table(table)?;
        let mut r = self.db.get(&t.name, key)?.clone();
        crate::calc::calc_fields(&self.db, self.work_date, &t.name, &mut r);
        Some(r)
    }

    /// The records a page shows (its view, plus filters and search).
    pub fn page_records(&self, page: &PageDef, filters: &[(String, String)], search: &str) -> Vec<Record> {
        let Some(t) = page.source_table.as_deref().and_then(|t| self.table(t)) else { return Vec::new() };
        commands::list_records(&self.db, self.work_date, t, Some(page), filters, search)
    }

    /// The page that opens a record of `table` (document page by type for sales / purchase).
    pub fn card_page_for(&self, table: &str, r: &Record) -> Option<String> {
        commands::card_page_for(self.table(table)?, r)
    }

    /// A cue's value on the role center.
    pub fn cue_value(&self, cue: &erpcraft_meta::CueDef) -> rust_decimal::Decimal {
        commands::cue_value(&self.db, self.meta(), self.work_date, cue)
    }

    /// Tell Me search over pages.
    pub fn search_pages(&self, query: &str) -> Vec<&'static PageDef> {
        commands::search_pages(self.meta(), query).into_iter().filter_map(|id| self.page(id)).collect()
    }
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn default_user() -> String {
    std::env::var("ERPCRAFT_USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "ADMIN".to_string())
}
