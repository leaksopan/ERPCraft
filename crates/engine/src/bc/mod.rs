//! Business Central API connector (API v2.0): reads and writes customers, vendors, items and G/L
//! accounts of a Dynamics 365 Business Central company over OAuth2 client credentials. See
//! docs/bc-connector.md.
//!
//! - "BC Connection Setup" (50100) holds tenant, environment, company and app id; the client
//!   secret is only ever read from the `ERPCRAFT_BC_CLIENT_SECRET` environment variable.
//! - "BC Sync Log" (50101) keeps one entry per import / export.
//! - Commands: `bc.setup`, `bc.test_connection`, `bc.import`, `bc.export`.

pub mod client;
pub mod map;

use std::collections::HashMap;

use erpcraft_meta::{
    ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Key, Metadata, NavGroup, NavLink as L, PageDef, PageKind as K,
    Record, TableDef, Value,
};
use serde_json::{Map, Value as Json, json};

use crate::commands::CommandDef;
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::records;
use client::{Client, SECRET_ENV, fetch_all, is_fatal};
use map::Entity;

pub const SETUP: &str = "BC Connection Setup";
pub const SYNC_LOG: &str = "BC Sync Log";

pub const DEFAULT_ENVIRONMENT: &str = "Production";
pub const DEFAULT_API_BASE: &str = "https://api.businesscentral.dynamics.com/v2.0";
pub const DEFAULT_TOKEN_URL: &str = "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token";

/// Most records one import or export reads from Business Central.
pub const MAX_RECORDS: usize = 50_000;
/// Error messages returned by one import / export.
const MAX_ERRORS: usize = 20;

// ---- metadata -------------------------------------------------------------------------------

/// Tables and pages of the connector.
pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(SETUP, 50100)
            .fields(vec![
                F::code("Primary Key", 10),
                F::text("Tenant ID", 100).tooltip("Microsoft Entra tenant id (GUID) or domain, e.g. contoso.onmicrosoft.com"),
                F::text("Environment", 50).tooltip("Business Central environment name; blank means Production"),
                F::text("Company ID", 50).tooltip("Company id (GUID); run Test Connection to list the companies"),
                F::text("Company Name", 100),
                F::text("Client ID", 50).tooltip("Application (client) id of the Microsoft Entra app. The secret is read from ERPCRAFT_BC_CLIENT_SECRET."),
                F::text("API Base URL", 250).tooltip("Blank means https://api.businesscentral.dynamics.com/v2.0"),
                F::text("Token URL", 250).tooltip("Blank means https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
                F::text("Last Import At", 30).read_only(),
                F::text("Last Export At", 30).read_only(),
            ])
            .key(&["Primary Key"])
            .pages("bc-connection-setup", Some("bc-connection-setup")),
    );
    m.add_table(
        TableDef::new(SYNC_LOG, 50101)
            .fields(vec![
                F::integer("Entry No."),
                F::text("Date/Time", 30),
                F::code("User ID", 50),
                F::option("Direction", &["Import", "Export"]),
                F::text("Entity", 30),
                F::integer("Created"),
                F::integer("Updated"),
                F::integer("Unchanged"),
                F::integer("Skipped"),
                F::text("Message", 250),
            ])
            .key(&["Entry No."])
            .describe_by("Message")
            .pages("bc-sync-log", None),
    );
    let entity_action = |caption: &str, command: &str, entity: &str| {
        Act::command(caption, A::Process, command).param("entity", entity).confirm(&format!("Do you want to run '{caption}'?"))
    };
    m.add_page(
        PageDef::new("bc-connection-setup", 50100, "BC Connection Setup", K::Card, Some(SETUP))
            .group(G::new("General", &["Tenant ID", "Environment", "Company ID", "Company Name", "Client ID"]))
            .group(G::new("Advanced", &["API Base URL", "Token URL"]).collapsed())
            .group(G::new("Status", &["Last Import At", "Last Export At"]))
            .action(Act::command("Test Connection", A::Process, "bc.test_connection").icon("stats"))
            .action(entity_action("Import Customers", "bc.import", "customers"))
            .action(entity_action("Import Vendors", "bc.import", "vendors"))
            .action(entity_action("Import Items", "bc.import", "items"))
            .action(entity_action("Import G/L Accounts", "bc.import", "accounts"))
            .action(entity_action("Export Customers", "bc.export", "customers"))
            .action(entity_action("Export Vendors", "bc.export", "vendors"))
            .action(entity_action("Export Items", "bc.export", "items"))
            .action(Act::page("Sync Log", A::Navigate, "bc-sync-log"))
            .keywords("business central dynamics 365 api connector import export sync setup"),
    );
    m.add_page(
        PageDef::new("bc-sync-log", 50101, "BC Sync Log", K::List, Some(SYNC_LOG))
            .read_only()
            .columns(&["Entry No.", "Date/Time", "User ID", "Direction", "Entity", "Created", "Updated", "Unchanged", "Skipped", "Message"])
            .keywords("business central sync log import export"),
    );
    let Some(rc) = m.role_centers.get_mut(crate::schema::role_center::BUSINESS_MANAGER) else { return };
    let links = [L::new("BC Connection Setup", "bc-connection-setup"), L::new("BC Sync Log", "bc-sync-log")];
    match rc.sections.iter_mut().find(|s| s.caption == "Setup") {
        Some(s) => s.links.extend(links),
        None => rc.sections.push(NavGroup::new("Setup", links.into())),
    }
}

// ---- settings -------------------------------------------------------------------------------

/// The connection setup with defaults applied.
pub struct Settings {
    pub tenant: String,
    pub environment: String,
    pub company_id: String,
    pub company_name: String,
    pub client_id: String,
    pub api_base: String,
    pub token_url: String,
}

impl Settings {
    pub fn read(ctx: &Ctx) -> Result<Settings> {
        let r = ctx.setup(SETUP)?;
        let get = |f: &str| r.str(f).trim().to_string();
        let tenant = get("Tenant ID");
        let client_id = get("Client ID");
        for (field, v) in [("Tenant ID", &tenant), ("Client ID", &client_id)] {
            if v.is_empty() {
                return Err(EngineError::refused(format!(
                    "{field} must have a value in {SETUP}: Primary Key=. It cannot be zero or empty."
                )));
            }
        }
        let or = |v: String, d: &str| if v.is_empty() { d.to_string() } else { v };
        let api_base = or(get("API Base URL"), DEFAULT_API_BASE).trim_end_matches('/').to_string();
        let token_url = or(get("Token URL"), DEFAULT_TOKEN_URL).replace("{tenant}", &url_segment(&tenant));
        Ok(Settings {
            environment: or(get("Environment"), DEFAULT_ENVIRONMENT),
            company_id: get("Company ID").trim_start_matches('{').trim_end_matches('}').to_string(),
            company_name: get("Company Name"),
            tenant,
            client_id,
            api_base,
            token_url,
        })
    }

    fn require_company(&self) -> Result<()> {
        if self.company_id.is_empty() {
            return Err(EngineError::refused(format!(
                "Company ID must have a value in {SETUP}: Primary Key=. It cannot be zero or empty. Run bc.test_connection to list the companies."
            )));
        }
        Ok(())
    }
}

/// Percent-encodes `s` for a URL path segment.
fn url_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn now_text() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

// ---- commands -------------------------------------------------------------------------------

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "bc.setup",
        label: "Business Central Connection Setup",
        group: "Business Central",
        summary: "Sets fields of the BC Connection Setup (tenant_id, environment, company_id, company_name, client_id, api_base_url, token_url). The client secret is never stored: set the ERPCRAFT_BC_CLIENT_SECRET environment variable.",
        mutates: true,
        params: setup_params,
        run: setup,
    },
    CommandDef {
        id: "bc.test_connection",
        label: "Test Business Central Connection",
        group: "Business Central",
        summary: "Signs in to Business Central with the connection setup and lists the companies (id, name, displayName) so you can pick the Company ID.",
        mutates: false,
        params: no_params,
        run: test_connection,
    },
    CommandDef {
        id: "bc.import",
        label: "Import from Business Central",
        group: "Business Central",
        summary: "Imports customers, vendors, items or G/L accounts from the Business Central company (API v2.0) and creates or updates them by No.",
        mutates: true,
        params: import_params,
        run: import,
    },
    CommandDef {
        id: "bc.export",
        label: "Export to Business Central",
        group: "Business Central",
        summary: "Exports customers, vendors or items to the Business Central company: creates the ones it doesn't have, updates changed ones (If-Match on the record's ETag).",
        mutates: true,
        params: export_params,
        run: export,
    },
];

fn no_params() -> Json {
    json!({"type": "object", "properties": {}})
}

fn setup_params() -> Json {
    json!({
        "type": "object",
        "properties": {
            "tenant_id": {"type": "string", "description": "Microsoft Entra tenant id or domain"},
            "environment": {"type": "string", "description": "BC environment name (default Production)"},
            "company_id": {"type": "string", "description": "BC company id (GUID) from bc.test_connection"},
            "company_name": {"type": "string"},
            "client_id": {"type": "string", "description": "Application (client) id of the Entra app"},
            "api_base_url": {"type": "string", "description": "Default https://api.businesscentral.dynamics.com/v2.0"},
            "token_url": {"type": "string", "description": "Default https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"}
        }
    })
}

fn import_params() -> Json {
    json!({
        "type": "object",
        "properties": {
            "entity": {"type": "string", "enum": ["customers", "vendors", "items", "accounts"]},
            "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RECORDS, "description": "Most records to read (default and cap 50000)"}
        },
        "required": ["entity"]
    })
}

fn export_params() -> Json {
    json!({
        "type": "object",
        "properties": {
            "entity": {"type": "string", "enum": ["customers", "vendors", "items"]},
            "no": {"type": "string", "description": "Export only this record"},
            "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RECORDS, "description": "Most records to export (default and cap 50000)"}
        },
        "required": ["entity"]
    })
}

fn entity_param(p: &Json) -> Result<Entity> {
    let s = p.get("entity").and_then(Json::as_str).unwrap_or("");
    Entity::parse(s).ok_or_else(|| {
        EngineError::invalid(format!("'{}' is not an entity the connector knows. Use customers, vendors, items or accounts.", map::truncate(s, 40)))
    })
}

fn limit_param(p: &Json) -> Result<usize> {
    match p.get("limit") {
        None | Some(Json::Null) => Ok(MAX_RECORDS),
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 => Ok(usize::try_from(n).unwrap_or(MAX_RECORDS).min(MAX_RECORDS)),
            _ => Err(EngineError::invalid("'limit' must be a whole number of at least 1.")),
        },
    }
}

fn setup(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    if p.get("client_secret").is_some() || p.get("Client Secret").is_some() {
        return Err(EngineError::invalid(format!(
            "The client secret is never stored in the company. Set the environment variable {SECRET_ENV} instead."
        )));
    }
    let t = ctx.table(SETUP)?;
    let mut r = ctx.setup(SETUP)?;
    let fields = [
        ("tenant_id", "Tenant ID"),
        ("environment", "Environment"),
        ("company_id", "Company ID"),
        ("company_name", "Company Name"),
        ("client_id", "Client ID"),
        ("api_base_url", "API Base URL"),
        ("token_url", "Token URL"),
    ];
    for (param, field) in fields {
        // Parameter name or the field name itself.
        if let Some(v) = p.get(param).or_else(|| p.get(field)) {
            let v = match v {
                Json::String(s) => Json::String(s.trim().to_string()),
                other => other.clone(),
            };
            records::validate(ctx, t, &mut r, field, &v, true)?;
        }
    }
    ctx.put(SETUP, r.clone())?;
    Ok(json!({"setup": r.to_json(t), "secret_set": client::client_secret().is_some()}))
}

fn test_connection(ctx: &mut Ctx, _: &Json) -> Result<Json> {
    let s = Settings::read(ctx)?;
    let c = Client::connect(&s)?;
    let (rows, truncated) = fetch_all(&c, &c.companies_url(), 1_000)?;
    let field = |r: &Json, k: &str| r.get(k).and_then(Json::as_str).map(|v| map::truncate(v, 250)).unwrap_or_default();
    let companies: Vec<Json> = rows
        .iter()
        .map(|r| json!({"id": field(r, "id"), "name": field(r, "name"), "displayName": field(r, "displayName")}))
        .collect();
    let selected = companies.iter().find(|c| c["id"].as_str().is_some_and(|id| !s.company_id.is_empty() && id.eq_ignore_ascii_case(&s.company_id)));
    let message = match (selected, s.company_id.is_empty()) {
        (Some(c), _) => format!("Connected. Company {} is selected.", c["name"].as_str().unwrap_or("")),
        (None, true) => format!("Connected. {} companies found; set Company ID to one of them.", companies.len()),
        (None, false) => "Connected, but the Company ID in the setup is not one of these companies.".to_string(),
    };
    Ok(json!({
        "ok": true,
        "message": message,
        "environment": s.environment,
        "companies": companies,
        "truncated": truncated,
        "selected": selected.cloned(),
    }))
}

#[derive(Default)]
struct Stats {
    created: i64,
    updated: i64,
    unchanged: i64,
    skipped: i64,
    errors: Vec<String>,
}

impl Stats {
    fn fail(&mut self, msg: String) {
        self.skipped += 1;
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(msg);
        }
    }

    fn to_json(&self, entity: Entity, read: usize, truncated: bool) -> Json {
        json!({
            "entity": entity.api_name(),
            "read": read,
            "created": self.created,
            "updated": self.updated,
            "unchanged": self.unchanged,
            "skipped": self.skipped,
            "errors": self.errors,
            "truncated": truncated,
        })
    }

    fn summary(&self) -> String {
        let mut s = format!("{} created, {} updated, {} unchanged, {} skipped", self.created, self.updated, self.unchanged, self.skipped);
        if let Some(e) = self.errors.first() {
            s.push_str(". First error: ");
            s.push_str(e);
        }
        map::truncate(&s, 250)
    }
}

enum Outcome {
    Created,
    Updated,
    Unchanged,
}

fn import(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let entity = entity_param(p)?;
    let limit = limit_param(p)?;
    let s = Settings::read(ctx)?;
    s.require_company()?;
    let c = Client::connect(&s)?;
    let url = c.entity_url(&s.company_id, entity.api_name())?;
    let (rows, truncated) = fetch_all(&c, &url, limit)?;
    let t = ctx.table(entity.table())?;
    let defaults = default_values(ctx, t, entity);
    let mut stats = Stats::default();
    for row in &rows {
        let no = row.get("number").and_then(Json::as_str).map(|n| n.trim().to_uppercase()).unwrap_or_default();
        if no.is_empty() {
            stats.fail(format!("A record without a number was skipped ({}).", map::truncate(row.get("id").and_then(Json::as_str).unwrap_or("no id"), 40)));
            continue;
        }
        if no.chars().count() > 20 {
            stats.fail(format!("{} {}: No. is longer than 20 characters; skipped.", t.caption, map::truncate(&no, 40)));
            continue;
        }
        match import_one(ctx, t, entity, &no, row, &defaults) {
            Ok(Outcome::Created) => stats.created += 1,
            Ok(Outcome::Updated) => stats.updated += 1,
            Ok(Outcome::Unchanged) => stats.unchanged += 1,
            Err(e) => stats.fail(format!("{} {no}: {e}", t.caption)),
        }
    }
    let mut setup = ctx.setup(SETUP)?;
    setup.set("Last Import At", Value::text(now_text()));
    ctx.put(SETUP, setup)?;
    log(ctx, "Import", entity, &stats)?;
    Ok(stats.to_json(entity, rows.len(), truncated))
}

/// Posting groups (and the like) from the table's first record, for new records.
fn default_values(ctx: &Ctx, t: &TableDef, entity: Entity) -> Map<String, Json> {
    let mut out = Map::new();
    if let Some(first) = ctx.db.rows(&t.name).next() {
        for f in entity.default_fields() {
            let v = first.get(f);
            if !v.is_blank() {
                out.insert((*f).to_string(), v.to_json());
            }
        }
    }
    out
}

fn import_one(ctx: &mut Ctx, t: &TableDef, entity: Entity, no: &str, row: &Json, defaults: &Map<String, Json>) -> Result<Outcome> {
    let values = map::from_api(entity, t, row);
    if entity == Entity::Items {
        ensure_unit_of_measure(ctx, &values)?;
    }
    let key = Key(vec![Value::text(no)]);
    if let Some(cur) = ctx.db.get(&t.name, &key) {
        if !map::changes(t, cur, &values) {
            return Ok(Outcome::Unchanged);
        }
        records::update(ctx, t, &key, &values)?;
        return Ok(Outcome::Updated);
    }
    // Validate everything on a new record first and insert it last, so a record that fails
    // leaves nothing behind.
    let mut merged = defaults.clone();
    merged.extend(values);
    let mut r: Record = t.init_record();
    r.set("No.", Value::text(no));
    for f in &t.fields {
        if f.name == "No." {
            continue;
        }
        if let Some(v) = merged.get(&f.name) {
            records::validate(ctx, t, &mut r, &f.name, v, true)?;
        }
    }
    records::insert(ctx, t, r)?;
    Ok(Outcome::Created)
}

/// Creates the item's base unit of measure when the company doesn't have it yet.
fn ensure_unit_of_measure(ctx: &mut Ctx, values: &Map<String, Json>) -> Result<()> {
    let Some(code) = values.get("Base Unit of Measure").and_then(Json::as_str).filter(|c| !c.is_empty()) else { return Ok(()) };
    let uom = ctx.table("Unit of Measure")?;
    if ctx.db.get(&uom.name, &Key(vec![Value::text(code)])).is_none() {
        let mut v = Map::new();
        v.insert("Code".into(), Json::String(code.to_string()));
        v.insert("Description".into(), Json::String(code.to_string()));
        records::create(ctx, uom, &v, None)?;
    }
    Ok(())
}

fn export(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let entity = entity_param(p)?;
    if !entity.exportable() {
        return Err(EngineError::invalid("bc.export supports customers, vendors and items."));
    }
    let limit = limit_param(p)?;
    let only = p.get("no").and_then(Json::as_str).map(|n| n.trim().to_uppercase()).filter(|n| !n.is_empty());
    let t = ctx.table(entity.table())?;
    let local: Vec<Record> = match &only {
        Some(no) => vec![ctx.get_code(&t.name, no)?],
        None => ctx.db.rows(&t.name).take(limit).cloned().collect(),
    };
    let s = Settings::read(ctx)?;
    s.require_company()?;
    let c = Client::connect(&s)?;
    let url = c.entity_url(&s.company_id, entity.api_name())?;
    let (remote_rows, _) = fetch_all(&c, &url, MAX_RECORDS)?;
    let remote: HashMap<String, &Json> = remote_rows
        .iter()
        .filter_map(|r| r.get("number").and_then(Json::as_str).map(|n| (n.trim().to_uppercase(), r)))
        .collect();
    let mut stats = Stats::default();
    for r in &local {
        let no = r.str("No.").to_string();
        let body = map::to_api(entity, t, r);
        let result = match remote.get(&no) {
            Some(rem) => {
                let diff: Map<String, Json> =
                    body.iter().filter(|(k, v)| k.as_str() != "number" && !map::same(v, rem.get(k.as_str()))).map(|(k, v)| (k.clone(), v.clone())).collect();
                if diff.is_empty() {
                    Ok(Outcome::Unchanged)
                } else {
                    let id = rem.get("id").and_then(Json::as_str).unwrap_or("");
                    let etag = rem.get("@odata.etag").and_then(Json::as_str).unwrap_or("");
                    c.record_url(&url, id).and_then(|u| c.patch(&u, etag, &diff)).map(|_| Outcome::Updated)
                }
            }
            None => c.post(&url, &body).map(|_| Outcome::Created),
        };
        match result {
            Ok(Outcome::Created) => stats.created += 1,
            Ok(Outcome::Updated) => stats.updated += 1,
            Ok(Outcome::Unchanged) => stats.unchanged += 1,
            Err(e) if is_fatal(&e) => return Err(e),
            Err(e) => stats.fail(format!("{} {no}: {e}", t.caption)),
        }
    }
    let mut setup = ctx.setup(SETUP)?;
    setup.set("Last Export At", Value::text(now_text()));
    ctx.put(SETUP, setup)?;
    log(ctx, "Export", entity, &stats)?;
    Ok(stats.to_json(entity, local.len(), false))
}

fn log(ctx: &mut Ctx, direction: &str, entity: Entity, stats: &Stats) -> Result<()> {
    let t = ctx.table(SYNC_LOG)?;
    let mut r = t.init_record();
    r.set("Entry No.", Value::Int(ctx.db.next_entry_no(SYNC_LOG)));
    r.set("Date/Time", Value::text(now_text()));
    r.set("User ID", Value::text(map::truncate(&ctx.user.to_uppercase(), 50)));
    r.set("Direction", Value::text(direction));
    r.set("Entity", Value::text(entity.api_name()));
    r.set("Created", Value::Int(stats.created));
    r.set("Updated", Value::Int(stats.updated));
    r.set("Unchanged", Value::Int(stats.unchanged));
    r.set("Skipped", Value::Int(stats.skipped));
    r.set("Message", Value::text(stats.summary()));
    ctx.insert(SYNC_LOG, r)?;
    Ok(())
}
