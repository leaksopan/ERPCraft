//! PostgreSQL store.
//!
//! Layout, in one schema per company (default `erpcraft`, or `?schema=name` on the URL):
//! - one table per BC table, named and with columns named as in BC (`"Sales Header"."No."`),
//!   typed (`numeric`, `bigint`, `boolean`, `date`, `text`), primary key as in BC, plus
//!   `"$version" bigint` taken from the sequence `"$version_seq"` on every write;
//! - `"$deleted"`: deletions, so other sessions can drop rows they still hold.
//!
//! Saving a command's changes is one SQL transaction. Updates and deletes check the row version
//! the session read (optimistic concurrency): if another session wrote the row since, nothing is
//! saved and the user gets BC's "Another user has modified the record" error. Before each command
//! a session pulls rows with a version above the last one it saw.

use erpcraft_meta::{FieldType, Key, Metadata, Record, TableDef, Value};
use postgres::types::ToSql;
use postgres::{Client, NoTls};
use serde_json::Value as Json;

use super::Store;
use crate::db::{Changes, Database, VERSION_FIELD};
use crate::error::{EngineError, ErrorKind, Result};

/// Versions re-read on refresh below the last one seen: a transaction that took its sequence value
/// before another but committed after it would otherwise be missed.
const REFRESH_OVERLAP: i64 = 1000;

pub struct Postgres {
    client: Client,
    schema: String,
    describe: String,
    last_seen: i64,
}

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn db_err(e: postgres::Error) -> EngineError {
    match e.as_db_error() {
        Some(d) => EngineError::io(format!("PostgreSQL: {} ({})", d.message(), d.code().code())),
        None => EngineError::io(format!("PostgreSQL: {e}")),
    }
}

fn conflict(t: &TableDef) -> EngineError {
    EngineError {
        kind: ErrorKind::Conflict,
        message: format!(
            "Another user has modified the record for this {} after you retrieved it from the database. \
             Enter your changes again in the updated window, or start the interrupted activity again.",
            t.caption
        ),
    }
}

fn sql_type(ty: &FieldType) -> &'static str {
    match ty {
        FieldType::Code(_) | FieldType::Text(_) | FieldType::Option(_) | FieldType::Blob => "text NOT NULL DEFAULT ''",
        FieldType::Decimal => "numeric NOT NULL DEFAULT 0",
        FieldType::Integer => "bigint NOT NULL DEFAULT 0",
        FieldType::Boolean => "boolean NOT NULL DEFAULT false",
        FieldType::Date => "date",
    }
}

/// Parameters are bound as text and cast in SQL, so one code path serves every type.
fn cast(ty: &FieldType) -> &'static str {
    match ty {
        FieldType::Code(_) | FieldType::Text(_) | FieldType::Option(_) | FieldType::Blob => "::text",
        FieldType::Decimal => "::text::numeric",
        FieldType::Integer => "::text::bigint",
        FieldType::Boolean => "::text::boolean",
        FieldType::Date => "::text::date",
    }
}

fn to_param(ty: &FieldType, v: &Value) -> Option<String> {
    match (ty, v) {
        (FieldType::Date, Value::Date(d)) => Some(d.format("%Y-%m-%d").to_string()),
        (FieldType::Date, _) => None,
        (FieldType::Decimal, v) => Some(v.as_dec().to_string()),
        (FieldType::Integer, v) => Some(v.as_int().to_string()),
        (FieldType::Boolean, v) => Some(if v.as_bool() { "true" } else { "false" }.to_string()),
        (_, v) => Some(v.as_str().to_string()),
    }
}

fn stored_fields(t: &TableDef) -> impl Iterator<Item = &erpcraft_meta::FieldDef> {
    t.fields.iter().filter(|f| !f.is_flow())
}

/// `?schema=name` (stripped from the URL) or `ERPCRAFT_SCHEMA`, else `erpcraft`.
fn split_schema(url: &str) -> Result<(String, String)> {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b.to_string(), q.to_string()),
        None => (url.to_string(), String::new()),
    };
    let mut schema = std::env::var("ERPCRAFT_SCHEMA").unwrap_or_else(|_| "erpcraft".to_string());
    let mut rest = Vec::new();
    for part in query.split('&').filter(|p| !p.is_empty()) {
        match part.split_once('=') {
            Some(("schema", v)) => schema = v.to_string(),
            _ => rest.push(part.to_string()),
        }
    }
    if schema.is_empty() || schema.len() > 63 || !schema.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(EngineError::invalid(format!("invalid schema name '{schema}' (use lowercase letters, digits and _)")));
    }
    let url = if rest.is_empty() { base } else { format!("{base}?{}", rest.join("&")) };
    Ok((url, schema))
}

impl Postgres {
    /// Connects and creates or upgrades the schema for `meta`.
    pub fn connect(url: &str, meta: &Metadata) -> Result<Self> {
        let (url, schema) = split_schema(url)?;
        let config: postgres::Config = url.parse().map_err(|e| EngineError::invalid(format!("invalid PostgreSQL URL: {e}")))?;
        let host = config
            .get_hosts()
            .first()
            .map(|h| match h {
                postgres::config::Host::Tcp(s) => s.clone(),
                #[allow(unreachable_patterns)]
                _ => "socket".to_string(),
            })
            .unwrap_or_default();
        let port = config.get_ports().first().copied().unwrap_or(5432);
        let describe = format!("postgres {}@{host}:{port}/{} (schema {schema})", config.get_user().unwrap_or(""), config.get_dbname().unwrap_or(""));
        let client = match config.connect(NoTls) {
            Ok(c) => c,
            // 3D000 invalid_catalog_name: the database doesn't exist yet. Create it from the
            // maintenance database, as a first run on a fresh server should just work.
            Err(e) if e.as_db_error().is_some_and(|d| d.code().code() == "3D000") => {
                let dbname = config.get_dbname().unwrap_or_default().to_string();
                if dbname.is_empty() || !dbname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(db_err(e));
                }
                let mut admin = config.clone();
                admin.dbname("postgres");
                let mut c = admin.connect(NoTls).map_err(db_err)?;
                // Another process may create it at the same moment (42P04 / 23505): fine.
                if let Err(e) = c.batch_execute(&format!("CREATE DATABASE {}", q(&dbname)))
                    && !e.as_db_error().is_some_and(|d| matches!(d.code().code(), "42P04" | "23505"))
                {
                    return Err(db_err(e));
                }
                config.connect(NoTls).map_err(db_err)?
            }
            Err(e) => return Err(db_err(e)),
        };
        let mut pg = Self { client, schema, describe, last_seen: 0 };
        pg.ensure_schema(meta)?;
        Ok(pg)
    }

    fn table(&self, name: &str) -> String {
        format!("{}.{}", q(&self.schema), q(name))
    }

    fn seq(&self) -> String {
        format!("{}.{}", q(&self.schema), q("$version_seq"))
    }

    fn ensure_schema(&mut self, meta: &Metadata) -> Result<()> {
        let s = q(&self.schema);
        let mut ddl = format!(
            "CREATE SCHEMA IF NOT EXISTS {s};
             CREATE SEQUENCE IF NOT EXISTS {seq};
             CREATE TABLE IF NOT EXISTS {s}.\"$deleted\" (table_name text NOT NULL, key_json text NOT NULL, version bigint NOT NULL);
             CREATE INDEX IF NOT EXISTS \"$deleted_version\" ON {s}.\"$deleted\" (version);
             CREATE TABLE IF NOT EXISTS {s}.\"$meta\" (key text PRIMARY KEY, value text NOT NULL);
             INSERT INTO {s}.\"$meta\" (key, value) VALUES ('format', 'erpcraft-company-1') ON CONFLICT (key) DO NOTHING;\n",
            seq = self.seq()
        );
        for t in meta.tables.values() {
            let cols: Vec<String> = stored_fields(t).map(|f| format!("{} {}", q(&f.name), sql_type(&f.ty))).collect();
            let pk: Vec<String> = t.primary_key.iter().map(|k| q(k)).collect();
            let tbl = self.table(&t.name);
            ddl.push_str(&format!(
                "CREATE TABLE IF NOT EXISTS {tbl} ({}, \"$version\" bigint NOT NULL DEFAULT 0, PRIMARY KEY ({}));\n",
                cols.join(", "),
                pk.join(", ")
            ));
            // Fields added in a later version of the app.
            for f in stored_fields(t) {
                ddl.push_str(&format!("ALTER TABLE {tbl} ADD COLUMN IF NOT EXISTS {} {};\n", q(&f.name), sql_type(&f.ty)));
            }
            ddl.push_str(&format!("CREATE INDEX IF NOT EXISTS {} ON {tbl} (\"$version\");\n", q(&format!("ix_{}_version", t.object_id))));
        }
        self.client.batch_execute(&ddl).map_err(db_err)
    }

    fn current_version(&mut self) -> Result<i64> {
        let row = self.client.query_one(&format!("SELECT last_value, is_called FROM {}", self.seq()), &[]).map_err(db_err)?;
        let last: i64 = row.get(0);
        let called: bool = row.get(1);
        Ok(if called { last } else { 0 })
    }

    fn select_sql(&self, t: &TableDef, where_clause: &str) -> String {
        let cols: Vec<String> = stored_fields(t).map(|f| format!("{}::text", q(&f.name))).collect();
        format!("SELECT {}, \"$version\" FROM {} {where_clause}", cols.join(", "), self.table(&t.name))
    }

    fn read_rows(&mut self, t: &TableDef, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Vec<Record>> {
        let rows = self.client.query(sql, params).map_err(db_err)?;
        let fields: Vec<&erpcraft_meta::FieldDef> = stored_fields(t).collect();
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let mut r = t.init_record();
            for (i, f) in fields.iter().enumerate() {
                let text: Option<String> = row.try_get(i).map_err(db_err)?;
                let j = text.map_or(Json::Null, Json::String);
                let v = Value::from_json(&f.ty, &j).map_err(|e| EngineError::invalid(format!("{}.{}: {e}", t.name, f.name)))?;
                r.set(&f.name, v);
            }
            let version: i64 = row.try_get(fields.len()).map_err(db_err)?;
            r.set(VERSION_FIELD, Value::Int(version));
            out.push(r);
        }
        Ok(out)
    }

    /// `"A" = $n::text AND "B" = $m::text::bigint …` for a key, appending its parameters.
    fn key_clause(t: &TableDef, key: &Key, params: &mut Vec<Option<String>>) -> String {
        let mut parts = Vec::new();
        for (name, v) in t.primary_key.iter().zip(key.0.iter()) {
            let ty = t.field(name).map(|f| f.ty.clone()).unwrap_or(FieldType::Text(250));
            params.push(to_param(&ty, v));
            parts.push(format!("{} = ${}{}", q(name), params.len(), cast(&ty)));
        }
        parts.join(" AND ")
    }
}

impl Store for Postgres {
    fn load(&mut self, meta: &Metadata) -> Result<Database> {
        let mut db = Database::default();
        self.last_seen = self.current_version()?;
        for t in meta.tables.values() {
            let sql = self.select_sql(t, "");
            for r in self.read_rows(t, &sql, &[])? {
                db.load_row(t, r);
            }
        }
        Ok(db)
    }

    fn save(&mut self, meta: &Metadata, db: &mut Database, changes: &Changes) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let seq = self.seq();
        let schema = q(&self.schema);
        let mut versions: Vec<(String, Key, i64)> = Vec::new();
        let mut tx = self.client.transaction().map_err(db_err)?;
        for ((table, key), stored) in changes {
            let Some(t) = meta.table(table) else { continue };
            let tbl = format!("{schema}.{}", q(&t.name));
            match db.get(&t.name, key) {
                Some(row) if *stored == 0 => {
                    let mut params: Vec<Option<String>> = Vec::new();
                    let mut cols = Vec::new();
                    let mut vals = Vec::new();
                    for f in stored_fields(t) {
                        params.push(to_param(&f.ty, row.get(&f.name)));
                        cols.push(q(&f.name));
                        vals.push(format!("${}{}", params.len(), cast(&f.ty)));
                    }
                    let sql = format!(
                        "INSERT INTO {tbl} ({}, \"$version\") VALUES ({}, nextval('{seq}')) RETURNING \"$version\"",
                        cols.join(", "),
                        vals.join(", "),
                        seq = seq.replace('\'', "''")
                    );
                    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
                    let r = tx.query_one(&sql, &refs).map_err(|e| {
                        if e.as_db_error().is_some_and(|d| d.code().code() == "23505") {
                            EngineError::refused(format!(
                                "The record in table {} already exists. Identification fields and values: {}",
                                t.caption,
                                crate::db::key_text(t, key)
                            ))
                        } else {
                            db_err(e)
                        }
                    })?;
                    versions.push((t.name.clone(), key.clone(), r.get(0)));
                }
                Some(row) => {
                    let mut params: Vec<Option<String>> = Vec::new();
                    let mut sets = Vec::new();
                    for f in stored_fields(t).filter(|f| !t.primary_key.contains(&f.name)) {
                        params.push(to_param(&f.ty, row.get(&f.name)));
                        sets.push(format!("{} = ${}{}", q(&f.name), params.len(), cast(&f.ty)));
                    }
                    let mut where_clause = Self::key_clause(t, key, &mut params);
                    params.push(Some(stored.to_string()));
                    where_clause.push_str(&format!(" AND \"$version\" = ${}::text::bigint", params.len()));
                    sets.push(format!("\"$version\" = nextval('{}')", seq.replace('\'', "''")));
                    let sql = format!("UPDATE {tbl} SET {} WHERE {where_clause} RETURNING \"$version\"", sets.join(", "));
                    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
                    let rows = tx.query(&sql, &refs).map_err(db_err)?;
                    let Some(r) = rows.first() else { return Err(conflict(t)) };
                    versions.push((t.name.clone(), key.clone(), r.get(0)));
                }
                None if *stored == 0 => {}
                None => {
                    let mut params: Vec<Option<String>> = Vec::new();
                    let mut where_clause = Self::key_clause(t, key, &mut params);
                    params.push(Some(stored.to_string()));
                    where_clause.push_str(&format!(" AND \"$version\" = ${}::text::bigint", params.len()));
                    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
                    let n = tx.execute(&format!("DELETE FROM {tbl} WHERE {where_clause}"), &refs).map_err(db_err)?;
                    if n == 0 {
                        return Err(conflict(t));
                    }
                    let key_json = key.to_json().to_string();
                    tx.execute(
                        &format!("INSERT INTO {schema}.\"$deleted\" (table_name, key_json, version) VALUES ($1, $2, nextval('{}'))", seq.replace('\'', "''")),
                        &[&t.name, &key_json],
                    )
                    .map_err(db_err)?;
                }
            }
        }
        tx.commit().map_err(db_err)?;
        for (table, key, v) in versions {
            db.set_version(&table, &key, v);
        }
        Ok(())
    }

    fn refresh(&mut self, meta: &Metadata, db: &mut Database) -> Result<bool> {
        let current = self.current_version()?;
        if current <= self.last_seen {
            return Ok(false);
        }
        let since = (self.last_seen - REFRESH_OVERLAP).max(0);
        // Deletions first: a row deleted and inserted again comes back with the later version.
        let deleted = self
            .client
            .query(&format!("SELECT table_name, key_json FROM {}.\"$deleted\" WHERE version > $1", q(&self.schema)), &[&since])
            .map_err(db_err)?;
        for row in deleted {
            let table: String = row.get(0);
            let key_json: String = row.get(1);
            let (Some(t), Ok(j)) = (meta.table(&table), serde_json::from_str::<Json>(&key_json)) else { continue };
            if let Ok(key) = Key::from_json(t, &j) {
                // A row re-inserted later is read again below.
                db.unload_row(&t.name, &key);
            }
        }
        for t in meta.tables.values() {
            let sql = self.select_sql(t, "WHERE \"$version\" > $1");
            for r in self.read_rows(t, &sql, &[&since])? {
                db.load_row(t, r);
            }
        }
        self.last_seen = current;
        Ok(true)
    }

    fn describe(&self) -> String {
        self.describe.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::split_schema;

    #[test]
    fn schema_comes_from_the_url() {
        let (url, schema) = split_schema("postgres://u:p@localhost/erp?schema=acme&sslmode=disable").unwrap();
        assert_eq!(schema, "acme");
        assert_eq!(url, "postgres://u:p@localhost/erp?sslmode=disable");
        assert!(split_schema("postgres://localhost/erp?schema=Bad-Name").is_err());
    }
}
