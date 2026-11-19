//! The command registry. Everything a user, the CLI or an agent can do is a command here; the
//! egui UI, `erpcraft-cli run` and the MCP server all call [`crate::Session::run`].
//!
//! Context convention: an action run from a page passes the current record as `table` + `key`
//! (and its own fixed params). Commands that act on a document or a customer accept that context
//! as well as explicit parameters.

use chrono::NaiveDate;
use erpcraft_meta::{FieldType, Key, PageDef, PageKind, Record, TableDef};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::calc::calc_fields;
use crate::ctx::Ctx;
use crate::db::Database;
use crate::documents;
use crate::error::{EngineError, Result};
use crate::filter::{record_matches, record_search};
use crate::posting::{self, payment};
use crate::records;
use crate::refuse;
use crate::schema::tables::{BANK_ACCOUNT, CUSTOMER, GL_ACCOUNT, ITEM, VENDOR};
use crate::side::{PURCHASE, SALES, Side};

pub type RunFn = fn(&mut Ctx, &Json) -> Result<Json>;

pub struct CommandDef {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub summary: &'static str,
    /// Changes data (the session saves after it; read-only commands never write the file).
    pub mutates: bool,
    /// JSON Schema of the parameters (MCP `inputSchema`, CLI help, UI parameter dialogs).
    pub params: fn() -> Json,
    pub run: RunFn,
}

impl CommandDef {
    pub fn to_json(&self) -> Json {
        json!({
            "id": self.id,
            "label": self.label,
            "group": self.group,
            "summary": self.summary,
            "mutates": self.mutates,
            "params": (self.params)(),
        })
    }
}

pub fn find(id: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.id == id)
}

pub fn all() -> &'static [CommandDef] {
    COMMANDS
}

// ---- parameter helpers ----------------------------------------------------------------------

fn obj(params: &Json) -> Map<String, Json> {
    params.as_object().cloned().unwrap_or_default()
}

fn opt_str<'a>(params: &'a Json, name: &str) -> Option<&'a str> {
    params.get(name).and_then(Json::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn req_str<'a>(params: &'a Json, name: &str) -> Result<&'a str> {
    opt_str(params, name).ok_or_else(|| EngineError::invalid(format!("missing parameter '{name}'")))
}

fn opt_dec(params: &Json, name: &str) -> Result<Option<Decimal>> {
    match params.get(name) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => match erpcraft_meta::Value::from_json(&FieldType::Decimal, v) {
            Ok(v) => Ok(Some(v.as_dec())),
            Err(e) => Err(EngineError::invalid(format!("{name}: {e}"))),
        },
    }
}

fn opt_date(params: &Json, name: &str) -> Result<Option<NaiveDate>> {
    match params.get(name) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => match erpcraft_meta::Value::from_json(&FieldType::Date, v) {
            Ok(v) => Ok(v.as_date()),
            Err(e) => Err(EngineError::invalid(format!("{name}: {e}"))),
        },
    }
}

fn table_param<'a>(ctx: &Ctx, params: &'a Json) -> Result<(&'static TableDef, Option<&'static PageDef>)> {
    if let Some(p) = opt_str(params, "page") {
        let page = ctx.meta.page(p).ok_or_else(|| EngineError::not_found(format!("Page '{p}' does not exist.")))?;
        let t = page.source_table.as_deref().ok_or_else(|| EngineError::invalid(format!("Page '{p}' has no source table.")))?;
        return Ok((ctx.table(t)?, Some(page)));
    }
    let t = req_str(params, "table")?;
    Ok((ctx.table(t)?, None))
}

fn key_param(t: &TableDef, params: &Json) -> Result<Key> {
    let k = params.get("key").ok_or_else(|| EngineError::invalid("missing parameter 'key'"))?;
    Key::from_json(t, k).map_err(EngineError::invalid)
}

/// The context record's key values when the action was run from `table`.
fn context_key<'a>(params: &'a Json, table: &str) -> Option<Vec<&'a Json>> {
    if opt_str(params, "table") != Some(table) {
        return None;
    }
    match params.get("key")? {
        Json::Array(a) => Some(a.iter().collect()),
        other => Some(vec![other]),
    }
}

fn context_code(params: &Json, table: &str) -> Option<String> {
    context_key(params, table)?.first().and_then(|v| v.as_str()).map(str::to_string)
}

/// (document type, no.) from `document_type` + `no`, or from a header context.
fn doc_ref(params: &Json, side: &Side) -> Result<(String, String)> {
    if let Some(k) = context_key(params, side.header)
        && let [t, n] = k.as_slice()
        && let (Some(t), Some(n)) = (t.as_str(), n.as_str())
    {
        return Ok((t.to_string(), n.to_string()));
    }
    let no = opt_str(params, "no").or_else(|| opt_str(params, "document_no")).ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    let t = opt_str(params, "document_type").unwrap_or("Order");
    Ok((t.to_string(), no.to_string()))
}

pub fn read(ctx: &Ctx, t: &TableDef, mut r: Record) -> Record {
    calc_fields(ctx.db, ctx.work_date, &t.name, &mut r);
    r
}

fn record_json(ctx: &Ctx, t: &TableDef, r: Record) -> Json {
    read(ctx, t, r).to_json(t)
}

/// Records of `t` matching a page's view, filters and a search text, flow fields calculated.
pub fn list_records(
    db: &Database,
    work_date: NaiveDate,
    t: &TableDef,
    page: Option<&PageDef>,
    filters: &[(String, String)],
    search: &str,
) -> Vec<Record> {
    let mut all_filters: Vec<(String, String)> = page.map(|p| p.view.iter().map(|v| (v.field.clone(), v.value.clone())).collect()).unwrap_or_default();
    all_filters.extend(filters.iter().cloned());
    let has_flow_filter = all_filters.iter().any(|(f, _)| t.field(f).is_some_and(|f| f.is_flow()));
    db.rows(&t.name)
        .filter_map(|r| {
            let mut r = r.clone();
            if has_flow_filter || !search.is_empty() || page.is_some() {
                calc_fields(db, work_date, &t.name, &mut r);
            }
            (record_matches(t, &r, &all_filters) && record_search(t, &r, search)).then_some(r)
        })
        .map(|mut r| {
            if !(has_flow_filter || !search.is_empty() || page.is_some()) {
                calc_fields(db, work_date, &t.name, &mut r);
            }
            r
        })
        .collect()
}

fn filters_param(params: &Json) -> Vec<(String, String)> {
    params
        .get("filters")
        .and_then(Json::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), match v {
                    Json::String(s) => s.clone(),
                    other => other.to_string(),
                }))
                .collect()
        })
        .unwrap_or_default()
}

// ---- schemas ---------------------------------------------------------------------------------

fn s_none() -> Json {
    json!({"type": "object", "properties": {}})
}
fn s_table() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string", "description": "Table name, e.g. Customer, Sales Header"}}, "required": ["table"]})
}
fn s_page() -> Json {
    json!({"type": "object", "properties": {"page": {"type": "string", "description": "Page id (customer-card) or BC object id (21)"}}, "required": ["page"]})
}
fn s_rolecenter() -> Json {
    json!({"type": "object", "properties": {"id": {"type": "string", "description": "Role center id (default business-manager)"}}})
}
fn s_search() -> Json {
    json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]})
}
fn s_list() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"}, "page": {"type": "string", "description": "list page id; applies its view (e.g. sales-order-list = Document Type Order)"},
        "filters": {"type": "object", "description": "field -> BC filter expression: 10000, <>Open, 10000|20000, >100, 2026-01-01..2026-01-31, *text*"},
        "search": {"type": "string"}, "fields": {"type": "array", "items": {"type": "string"}},
        "limit": {"type": "integer", "default": 100}, "offset": {"type": "integer", "default": 0}}})
}
fn s_get() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {"description": "primary key: array of values, or a single value for one-field keys"}}, "required": ["table", "key"]})
}
fn s_new() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "page": {"type": "string"}, "values": {"type": "object", "description": "field -> value; validated in table field order with triggers"}}})
}
fn s_validate() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "field": {"type": "string"}, "value": {}}, "required": ["table", "key", "field"]})
}
fn s_modify() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "values": {"type": "object"}}, "required": ["table", "key", "values"]})
}
fn s_lookup() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "field": {"type": "string"}, "search": {"type": "string"}, "limit": {"type": "integer"}}, "required": ["table", "field"]})
}
fn s_init() -> Json {
    json!({"type": "object", "properties": {"demo": {"type": "boolean", "default": true, "description": "load the Auto Craft demo company"}, "force": {"type": "boolean", "description": "replace existing data"}}})
}
fn s_workdate() -> Json {
    json!({"type": "object", "properties": {"date": {"type": "string", "description": "YYYY-MM-DD; omit to read"}}})
}
fn s_doc_new_sales() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "enum": ["Quote", "Order", "Invoice", "Credit Memo"], "default": "Order"},
        "customer_no": {"type": "string"},
        "values": {"type": "object", "description": "extra header fields"},
        "lines": {"type": "array", "items": {"type": "object", "description": "{type: Item|G/L Account, no, quantity, unit_price?, line_discount?}"}}}})
}
fn s_doc_new_purch() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "enum": ["Quote", "Order", "Invoice", "Credit Memo"], "default": "Order"},
        "vendor_no": {"type": "string"},
        "values": {"type": "object", "description": "extra header fields, e.g. Vendor Invoice No."},
        "lines": {"type": "array", "items": {"type": "object", "description": "{type: Item|G/L Account, no, quantity, direct_unit_cost?, line_discount?}"}}}})
}
fn s_doc() -> Json {
    json!({"type": "object", "properties": {"document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"}}, "required": ["no"]})
}
fn s_line() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "default": "Order"}, "no": {"type": "string", "description": "document no."},
        "type": {"type": "string", "default": "Item"}, "line_no": {"type": "string", "description": "item / account no."},
        "quantity": {"type": "number"}, "unit_price": {"type": "number"}, "line_discount": {"type": "number"}}, "required": ["no", "line_no", "quantity"]})
}
fn s_cust_payment() -> Json {
    json!({"type": "object", "properties": {
        "customer_no": {"type": "string"}, "amount": {"type": "number", "description": "amount received"},
        "bank_account_no": {"type": "string"}, "posting_date": {"type": "string"},
        "applies_to_doc_no": {"type": "string", "description": "posted invoice to apply to; omit to apply oldest first"}}, "required": ["amount"]})
}
fn s_vend_payment() -> Json {
    json!({"type": "object", "properties": {
        "vendor_no": {"type": "string"}, "amount": {"type": "number", "description": "amount paid"},
        "bank_account_no": {"type": "string"}, "posting_date": {"type": "string"},
        "applies_to_doc_no": {"type": "string"}}, "required": ["amount"]})
}
fn s_no() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}}, "required": ["no"]})
}
fn s_date() -> Json {
    json!({"type": "object", "properties": {"date": {"type": "string", "description": "as-of date (default: all entries)"}}})
}

// ---- metadata commands -----------------------------------------------------------------------

fn table_list(ctx: &mut Ctx, _: &Json) -> Result<Json> {
    let rows: Vec<Json> = ctx
        .meta
        .tables
        .values()
        .map(|t| json!({"name": t.name, "id": t.object_id, "caption": t.caption, "records": ctx.db.count(&t.name), "key": t.primary_key}))
        .collect();
    Ok(json!({"tables": rows}))
}

pub fn table_json(t: &TableDef) -> Json {
    let fields: Vec<Json> = t
        .fields
        .iter()
        .map(|f| {
            let mut j = json!({"name": f.name, "caption": f.caption, "type": f.ty.name(), "editable": f.editable, "flowfield": f.is_flow()});
            if let FieldType::Option(o) = &f.ty {
                j["options"] = json!(o);
            }
            if let Some(r) = &f.relation {
                j["relation"] = json!(r);
            }
            if !f.tooltip.is_empty() {
                j["tooltip"] = json!(f.tooltip);
            }
            j
        })
        .collect();
    json!({"name": t.name, "id": t.object_id, "caption": t.caption, "key": t.primary_key, "lookup_page": t.lookup_page, "card_page": t.card_page, "fields": fields})
}

fn table_describe(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    Ok(table_json(ctx.table(req_str(p, "table")?)?))
}

fn page_list(ctx: &mut Ctx, _: &Json) -> Result<Json> {
    let rows: Vec<Json> = ctx
        .meta
        .pages
        .values()
        .map(|p| json!({"id": p.id, "object_id": p.object_id, "caption": p.caption, "type": p.kind.name(), "table": p.source_table}))
        .collect();
    Ok(json!({"pages": rows}))
}

pub fn page_json(p: &PageDef) -> Json {
    let actions: Vec<Json> = p
        .actions
        .iter()
        .map(|a| {
            json!({"caption": a.caption.replace('&', ""), "area": a.area.caption(), "command": a.command, "open_page": a.open_page,
                   "link": a.link, "shortcut": a.shortcut, "params": a.params, "confirm": a.confirm})
        })
        .collect();
    json!({
        "id": p.id, "object_id": p.object_id, "caption": p.caption, "type": p.kind.name(), "table": p.source_table,
        "card_page": p.card_page, "editable": p.editable,
        "view": p.view.iter().map(|v| json!({"field": v.field, "value": v.value})).collect::<Vec<_>>(),
        "groups": p.groups.iter().map(|g| json!({"caption": g.caption, "fields": g.fields, "promoted": g.promoted})).collect::<Vec<_>>(),
        "columns": p.columns,
        "parts": p.parts.iter().map(|x| json!({"caption": x.caption, "page": x.page, "link": x.link})).collect::<Vec<_>>(),
        "factboxes": p.factboxes.iter().map(|f| json!({"caption": f.caption, "via": f.via, "fields": f.fields})).collect::<Vec<_>>(),
        "actions": actions,
    })
}

fn page_describe(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let id = req_str(p, "page")?;
    let page = ctx.meta.page(id).ok_or_else(|| EngineError::not_found(format!("Page '{id}' does not exist.")))?;
    Ok(page_json(page))
}

/// Value of one cue: `count`, `sum:<field>` or `overdue:<field>` over a page's records.
pub fn cue_value(db: &Database, meta: &erpcraft_meta::Metadata, work_date: NaiveDate, cue: &erpcraft_meta::CueDef) -> Decimal {
    let Some(page) = meta.page(&cue.page) else { return Decimal::ZERO };
    let Some(t) = page.source_table.as_deref().and_then(|t| meta.table(t)) else { return Decimal::ZERO };
    let rows = list_records(db, work_date, t, Some(page), &[], "");
    if let Some(f) = cue.measure.strip_prefix("sum:") {
        return rows.iter().map(|r| r.get(f).as_dec()).sum();
    }
    if let Some(f) = cue.measure.strip_prefix("overdue:") {
        return rows
            .iter()
            .filter(|r| r.get("Open").as_bool() && r.get("Due Date").as_date().is_some_and(|d| d < work_date))
            .map(|r| r.get(f).as_dec().abs())
            .sum();
    }
    Decimal::from(rows.len())
}

fn rolecenter_get(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let id = opt_str(p, "id").unwrap_or(crate::schema::role_center::BUSINESS_MANAGER);
    let rc = ctx.meta.role_center(id).ok_or_else(|| EngineError::not_found(format!("Role center '{id}' does not exist.")))?;
    let links = |v: &[erpcraft_meta::NavLink]| v.iter().map(|l| json!({"caption": l.caption, "page": l.page})).collect::<Vec<_>>();
    let groups = |v: &[erpcraft_meta::NavGroup]| v.iter().map(|g| json!({"caption": g.caption, "links": links(&g.links)})).collect::<Vec<_>>();
    let cues: Vec<Json> = rc
        .cue_groups
        .iter()
        .map(|g| {
            json!({"caption": g.caption, "cues": g.cues.iter().map(|c| json!({"caption": c.caption, "page": c.page,
                "value": cue_value(ctx.db, ctx.meta, ctx.work_date, c).to_string()})).collect::<Vec<_>>()})
        })
        .collect();
    Ok(json!({"id": rc.id, "caption": rc.caption, "embedding": links(&rc.embedding), "creation": links(&rc.creation),
              "action_groups": groups(&rc.action_groups), "sections": groups(&rc.sections), "cues": cues}))
}

fn command_list(_: &mut Ctx, _: &Json) -> Result<Json> {
    Ok(json!({"commands": all().iter().map(CommandDef::to_json).collect::<Vec<_>>()}))
}

/// Tell Me (Alt+Q): pages and commands whose caption or keywords contain every word of the query.
pub fn search_pages(meta: &erpcraft_meta::Metadata, query: &str) -> Vec<&'static str> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let _ = meta;
    let mut hits: Vec<(usize, &'static str)> = crate::schema::base_app()
        .pages
        .values()
        .filter(|p| p.kind != PageKind::ListPart)
        .filter_map(|p| {
            let hay = format!("{} {} {}", p.caption, p.keywords, p.id).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str())).then(|| {
                let rank = if p.caption.to_lowercase().starts_with(words.first().map_or("", String::as_str)) { 0 } else { 1 };
                (rank, p.id.as_str())
            })
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, id)| id).collect()
}

fn search(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let q = req_str(p, "query")?;
    let pages: Vec<Json> = search_pages(ctx.meta, q)
        .into_iter()
        .filter_map(|id| ctx.meta.page(id))
        .map(|pg| json!({"page": pg.id, "caption": pg.caption, "type": pg.kind.name()}))
        .collect();
    let ql = q.to_lowercase();
    let commands: Vec<Json> = all()
        .iter()
        .filter(|c| c.label.to_lowercase().contains(&ql) || c.id.contains(&ql))
        .map(|c| json!({"command": c.id, "label": c.label}))
        .collect();
    Ok(json!({"pages": pages, "commands": commands}))
}

// ---- record commands -------------------------------------------------------------------------

fn record_list(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, page) = table_param(ctx, p)?;
    let filters = filters_param(p);
    for (f, _) in &filters {
        if t.field(f).is_none() {
            refuse!("Table {} has no field '{f}'.", t.caption);
        }
    }
    let search = opt_str(p, "search").unwrap_or("");
    let rows = list_records(ctx.db, ctx.work_date, t, page, &filters, search);
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(100).min(10_000) as usize;
    let offset = p.get("offset").and_then(Json::as_u64).unwrap_or(0) as usize;
    let fields: Option<Vec<String>> =
        p.get("fields").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    let records: Vec<Json> = rows
        .iter()
        .skip(offset)
        .take(limit)
        .map(|r| match &fields {
            Some(f) => r.to_json_fields(f),
            None => r.to_json(t),
        })
        .collect();
    Ok(json!({"table": t.name, "key": t.primary_key, "total": rows.len(), "offset": offset, "records": records}))
}

fn record_get(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let key = key_param(t, p)?;
    let r = ctx.get(&t.name, &key)?;
    Ok(record_json(ctx, t, r))
}

/// The page that shows a record of `t` (a document page by type for sales / purchase headers).
pub fn card_page_for(t: &TableDef, r: &Record) -> Option<String> {
    if let Some(side) = Side::for_header(&t.name) {
        return Some(posting::document_page(side, r.str("Document Type")).to_string());
    }
    t.card_page.clone()
}

fn record_new(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, page) = table_param(ctx, p)?;
    let values = p.get("values").and_then(Json::as_object).cloned().unwrap_or_default();
    let r = records::create(ctx, t, &values, page)?;
    let key = r.key(t);
    let open = card_page_for(t, &r).map(|pg| json!({"page": pg, "key": key.to_json()}));
    Ok(json!({"key": key.to_json(), "record": record_json(ctx, t, r), "open": open}))
}

fn record_validate(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let key = key_param(t, p)?;
    let field = req_str(p, "field")?;
    let mut values = Map::new();
    values.insert(field.to_string(), p.get("value").cloned().unwrap_or(Json::Null));
    let r = records::update(ctx, t, &key, &values)?;
    Ok(record_json(ctx, t, r))
}

fn record_modify(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let key = key_param(t, p)?;
    let values = p.get("values").and_then(Json::as_object).cloned().ok_or_else(|| EngineError::invalid("missing parameter 'values'"))?;
    let r = records::update(ctx, t, &key, &values)?;
    Ok(record_json(ctx, t, r))
}

fn record_delete(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let key = key_param(t, p)?;
    records::delete(ctx, t, &key)?;
    Ok(json!({"deleted": key.to_json(), "message": format!("{} {} deleted.", t.caption, key.display())}))
}

fn record_lookup(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let field = req_str(p, "field")?;
    let f = t.field(field).ok_or_else(|| EngineError::not_found(format!("Table {} has no field '{field}'.", t.caption)))?;
    if let FieldType::Option(o) = &f.ty {
        return Ok(json!({"options": o}));
    }
    let Some(rel) = &f.relation else {
        return Ok(json!({"values": []}));
    };
    let rt = ctx.table(rel)?;
    let search = opt_str(p, "search").unwrap_or("");
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(200).min(5000) as usize;
    let desc = rt.description_field.clone().unwrap_or_default();
    let values: Vec<Json> = ctx
        .db
        .rows(&rt.name)
        .filter(|r| record_search(rt, r, search))
        .take(limit)
        .map(|r| json!({"value": r.key(rt).0.first().map(ToString::to_string).unwrap_or_default(), "description": r.str(&desc)}))
        .collect();
    Ok(json!({"table": rt.name, "values": values}))
}

// ---- company / session -----------------------------------------------------------------------

fn company_init(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let force = p.get("force").and_then(Json::as_bool).unwrap_or(false);
    let demo = p.get("demo").and_then(Json::as_bool).unwrap_or(true);
    let has_data = ctx.meta.tables.keys().any(|t| ctx.db.count(t) > 0);
    if has_data && !force {
        refuse!("The company already has data. Pass force=true to replace it.");
    }
    *ctx.db = Database::default();
    crate::demo::setup(ctx)?;
    if demo {
        crate::demo::master_data(ctx)?;
        crate::demo::transactions(ctx)?;
    }
    Ok(json!({"message": if demo { "Demo company Auto Craft created." } else { "Empty company with base setup created." }}))
}

fn company_info(ctx: &mut Ctx, _: &Json) -> Result<Json> {
    let info = ctx.setup(crate::schema::tables::COMPANY_INFO)?;
    let t = ctx.table(crate::schema::tables::COMPANY_INFO)?;
    Ok(json!({"company": info.to_json(t), "work_date": ctx.work_date.to_string(), "user": ctx.user}))
}

fn session_workdate(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    if let Some(d) = opt_date(p, "date")? {
        ctx.work_date = d;
    }
    Ok(json!({"work_date": ctx.work_date.to_string()}))
}

// ---- documents -------------------------------------------------------------------------------

fn doc_new(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let party_param = if side.sales { "customer_no" } else { "vendor_no" };
    let party = opt_str(p, party_param).map(str::to_string).or_else(|| context_code(p, side.party_table)).unwrap_or_default();
    let doc_type = opt_str(p, "document_type").unwrap_or("Order");
    let values = p.get("values").and_then(Json::as_object).cloned().unwrap_or_default();
    let lines = p.get("lines").and_then(Json::as_array).cloned().unwrap_or_default();
    documents::new_document(ctx, side, doc_type, &party, &values, &lines)
}

fn doc_add_line(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let mut spec = obj(p);
    for k in ["document_type", "no", "document_no", "table", "key"] {
        spec.remove(k);
    }
    if let Some(item) = spec.remove("line_no") {
        spec.insert("no".into(), item);
    }
    let r = documents::add_line(ctx, side, &doc_type, &no, &Json::Object(spec))?;
    let lt = ctx.table(side.line)?;
    Ok(json!({"line": r.to_json(lt), "message": format!("Line {} added to {doc_type} {no}.", r.get("Line No."))}))
}

fn sales_new(c: &mut Ctx, p: &Json) -> Result<Json> {
    doc_new(c, &SALES, p)
}
fn purch_new(c: &mut Ctx, p: &Json) -> Result<Json> {
    doc_new(c, &PURCHASE, p)
}
fn sales_line(c: &mut Ctx, p: &Json) -> Result<Json> {
    doc_add_line(c, &SALES, p)
}
fn purch_line(c: &mut Ctx, p: &Json) -> Result<Json> {
    doc_add_line(c, &PURCHASE, p)
}
fn sales_release(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &SALES)?;
    documents::release(c, &SALES, &t, &n)
}
fn purch_release(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &PURCHASE)?;
    documents::release(c, &PURCHASE, &t, &n)
}
fn sales_reopen(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &SALES)?;
    documents::reopen(c, &SALES, &t, &n)
}
fn purch_reopen(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &PURCHASE)?;
    documents::reopen(c, &PURCHASE, &t, &n)
}
fn sales_post(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &SALES)?;
    posting::post_document(c, &SALES, &t, &n)
}
fn purch_post(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &PURCHASE)?;
    posting::post_document(c, &PURCHASE, &t, &n)
}
fn sales_make_order(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (_, n) = doc_ref(p, &SALES)?;
    documents::make_order(c, &SALES, &n)
}
fn purch_make_order(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (_, n) = doc_ref(p, &PURCHASE)?;
    documents::make_order(c, &PURCHASE, &n)
}
fn sales_stats(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &SALES)?;
    documents::statistics(c, &SALES, &t, &n)
}
fn purch_stats(c: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, n) = doc_ref(p, &PURCHASE)?;
    documents::statistics(c, &PURCHASE, &t, &n)
}

fn register_payment(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let party_param = if side.sales { "customer_no" } else { "vendor_no" };
    let party = opt_str(p, party_param)
        .map(str::to_string)
        .or_else(|| context_code(p, side.party_table))
        .ok_or_else(|| EngineError::invalid(format!("missing parameter '{party_param}'")))?;
    let amount = opt_dec(p, "amount")?.ok_or_else(|| EngineError::invalid("missing parameter 'amount'"))?;
    let bank = match opt_str(p, "bank_account_no") {
        Some(b) => b.to_string(),
        None => ctx
            .db
            .rows(BANK_ACCOUNT)
            .find(|b| !b.get("Blocked").as_bool())
            .map(|b| b.str("No.").to_string())
            .ok_or_else(|| EngineError::refused("There is no bank account to post the payment to."))?,
    };
    let date = opt_date(p, "posting_date")?.unwrap_or(ctx.work_date);
    payment::register(
        ctx,
        side,
        &payment::Payment {
            party_no: &party,
            amount,
            bank_account: &bank,
            posting_date: date,
            applies_to_doc_no: opt_str(p, "applies_to_doc_no"),
            description: opt_str(p, "description"),
        },
    )
}

fn cust_payment(c: &mut Ctx, p: &Json) -> Result<Json> {
    register_payment(c, &SALES, p)
}
fn vend_payment(c: &mut Ctx, p: &Json) -> Result<Json> {
    register_payment(c, &PURCHASE, p)
}

// ---- reports ---------------------------------------------------------------------------------

fn party_statistics(ctx: &mut Ctx, p: &Json, table: &str) -> Result<Json> {
    let no = opt_str(p, "no").map(str::to_string).or_else(|| context_code(p, table)).ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    let t = ctx.table(table)?;
    let r = read(ctx, t, ctx.get_code(table, &no)?);
    let fields: &[&str] = if table == CUSTOMER {
        &["Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Sales (LCY)", "Payments (LCY)", "Credit Limit (LCY)"]
    } else {
        &["Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Purchases (LCY)", "Payments (LCY)"]
    };
    let rows: Vec<Json> = fields.iter().map(|f| json!([f, r.get(f).to_string()])).collect();
    Ok(json!({"report": {"title": format!("{} Statistics - {} {}", t.caption, no, r.str("Name")), "columns": ["", "Value"], "rows": rows}}))
}

fn cust_stats(c: &mut Ctx, p: &Json) -> Result<Json> {
    party_statistics(c, p, CUSTOMER)
}
fn vend_stats(c: &mut Ctx, p: &Json) -> Result<Json> {
    party_statistics(c, p, VENDOR)
}

fn item_availability(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = opt_str(p, "no").map(str::to_string).or_else(|| context_code(p, ITEM)).ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    let t = ctx.table(ITEM)?;
    let r = read(ctx, t, ctx.get_code(ITEM, &no)?);
    let inv = r.get("Inventory").as_dec();
    let on_so = r.get("Qty. on Sales Order").as_dec();
    let on_po = r.get("Qty. on Purch. Order").as_dec();
    let rows = vec![
        json!(["Inventory", inv.to_string()]),
        json!(["Qty. on Sales Order", on_so.to_string()]),
        json!(["Qty. on Purch. Order", on_po.to_string()]),
        json!(["Projected Available Balance", (inv - on_so + on_po).to_string()]),
    ];
    Ok(json!({"report": {"title": format!("Item Availability - {no} {}", r.str("Description")), "columns": ["", "Quantity"], "rows": rows}}))
}

fn trial_balance(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let as_of = opt_date(p, "date")?;
    let t = ctx.table(GL_ACCOUNT)?;
    let accounts: Vec<Record> = ctx.db.rows(GL_ACCOUNT).cloned().collect();
    let mut rows = Vec::new();
    let (mut debit, mut credit) = (Decimal::ZERO, Decimal::ZERO);
    for a in accounts {
        let no = a.str("No.").to_string();
        let bal: Decimal = if a.str("Account Type") == "Posting" {
            ctx.db
                .rows(crate::schema::tables::GL_ENTRY)
                .filter(|e| e.str("G/L Account No.") == no && as_of.is_none_or(|d| e.get("Posting Date").as_date().is_some_and(|pd| pd <= d)))
                .map(|e| e.get("Amount").as_dec())
                .sum()
        } else {
            read(ctx, t, a.clone()).get("Balance").as_dec()
        };
        if a.str("Account Type") == "Posting" {
            if bal > Decimal::ZERO {
                debit += bal;
            } else {
                credit -= bal;
            }
        }
        if bal.is_zero() && a.str("Account Type") == "Posting" {
            continue;
        }
        rows.push(json!([no, a.str("Name"), a.str("Account Type"), bal.to_string()]));
    }
    rows.push(json!(["", "Total Debit", "", debit.to_string()]));
    rows.push(json!(["", "Total Credit", "", credit.to_string()]));
    Ok(json!({"report": {"title": "Trial Balance", "columns": ["No.", "Name", "Account Type", "Balance"], "rows": rows},
              "balanced": debit == credit}))
}

// ---- the registry ----------------------------------------------------------------------------

macro_rules! cmd {
    ($id:literal, $label:literal, $group:literal, $mutates:expr, $params:expr, $run:expr, $summary:literal) => {
        CommandDef { id: $id, label: $label, group: $group, summary: $summary, mutates: $mutates, params: $params, run: $run }
    };
}

static COMMANDS: &[CommandDef] = &[
    cmd!("command.list", "Commands", "Metadata", false, s_none, command_list, "Every command with its parameter schema."),
    cmd!("table.list", "Tables", "Metadata", false, s_none, table_list, "Every table with its record count."),
    cmd!("table.describe", "Describe Table", "Metadata", false, s_table, table_describe, "Fields, types, options and relations of a table."),
    cmd!("page.list", "Pages", "Metadata", false, s_none, page_list, "Every page (List, Card, Document, ListPart)."),
    cmd!("page.describe", "Describe Page", "Metadata", false, s_page, page_describe, "Layout of a page: FastTabs, columns, parts, FactBoxes, actions."),
    cmd!("rolecenter.get", "Role Center", "Metadata", false, s_rolecenter, rolecenter_get, "Navigation, actions and cue values of a role center."),
    cmd!("search", "Tell Me", "Metadata", false, s_search, search, "Find pages and commands by name (Alt+Q)."),
    cmd!("record.list", "List Records", "Data", false, s_list, record_list, "Records of a table or list page, with BC filters and search."),
    cmd!("record.get", "Get Record", "Data", false, s_get, record_get, "One record by primary key, FlowFields calculated."),
    cmd!("record.new", "New Record", "Data", true, s_new, record_new, "Create a record (number series, defaults, field validation)."),
    cmd!("record.validate", "Set Field", "Data", true, s_validate, record_validate, "Set one field with its OnValidate trigger and save."),
    cmd!("record.modify", "Modify Record", "Data", true, s_modify, record_modify, "Set several fields (validated in table order) and save."),
    cmd!("record.delete", "Delete Record", "Data", true, s_get, record_delete, "Delete a record (OnDelete checks and cascades)."),
    cmd!("record.lookup", "Lookup Values", "Data", false, s_lookup, record_lookup, "Allowed values of a field: options or related records."),
    cmd!("company.init", "Initialize Company", "Company", true, s_init, company_init, "Create the base setup, optionally with the Auto Craft demo data."),
    cmd!("company.info", "Company Information", "Company", false, s_none, company_info, "Company information, work date and user."),
    cmd!("session.workdate", "Work Date", "Company", false, s_workdate, session_workdate, "Read or set the work date used as default posting date."),
    cmd!("sales.document.new", "New Sales Document", "Sales", true, s_doc_new_sales, sales_new, "Create a sales quote, order, invoice or credit memo, optionally with lines."),
    cmd!("sales.line.add", "Add Sales Line", "Sales", true, s_line, sales_line, "Add an item or G/L account line to a sales document."),
    cmd!("sales.document.release", "Release", "Sales", true, s_doc, sales_release, "Release a sales document (status Released)."),
    cmd!("sales.document.reopen", "Reopen", "Sales", true, s_doc, sales_reopen, "Reopen a released sales document."),
    cmd!("sales.document.post", "Post", "Sales", true, s_doc, sales_post, "Post a sales order (ship and invoice), invoice or credit memo."),
    cmd!("sales.quote.make_order", "Make Order", "Sales", true, s_doc, sales_make_order, "Convert a sales quote to an order."),
    cmd!("sales.document.statistics", "Statistics", "Sales", false, s_doc, sales_stats, "Amounts, VAT and profit of a sales document."),
    cmd!("purchase.document.new", "New Purchase Document", "Purchasing", true, s_doc_new_purch, purch_new, "Create a purchase quote, order, invoice or credit memo, optionally with lines."),
    cmd!("purchase.line.add", "Add Purchase Line", "Purchasing", true, s_line, purch_line, "Add an item or G/L account line to a purchase document."),
    cmd!("purchase.document.release", "Release", "Purchasing", true, s_doc, purch_release, "Release a purchase document."),
    cmd!("purchase.document.reopen", "Reopen", "Purchasing", true, s_doc, purch_reopen, "Reopen a released purchase document."),
    cmd!("purchase.document.post", "Post", "Purchasing", true, s_doc, purch_post, "Post a purchase order (receive and invoice), invoice or credit memo."),
    cmd!("purchase.quote.make_order", "Make Order", "Purchasing", true, s_doc, purch_make_order, "Convert a purchase quote to an order."),
    cmd!("purchase.document.statistics", "Statistics", "Purchasing", false, s_doc, purch_stats, "Amounts and VAT of a purchase document."),
    cmd!("payment.customer.register", "Register Customer Payment", "Cash Management", true, s_cust_payment, cust_payment, "Post a payment received and apply it to open invoices."),
    cmd!("payment.vendor.register", "Make Vendor Payment", "Cash Management", true, s_vend_payment, vend_payment, "Post a payment to a vendor and apply it to open invoices."),
    cmd!("customer.statistics", "Customer Statistics", "Sales", false, s_no, cust_stats, "Balance, sales and payments of a customer."),
    cmd!("vendor.statistics", "Vendor Statistics", "Purchasing", false, s_no, vend_stats, "Balance, purchases and payments of a vendor."),
    cmd!("item.availability", "Item Availability", "Inventory", false, s_no, item_availability, "Inventory, quantities on order and projected availability."),
    cmd!("report.trial_balance", "Trial Balance", "Finance", false, s_date, trial_balance, "G/L account balances with debit / credit totals."),
];

#[cfg(test)]
mod tests {
    #[test]
    fn command_ids_are_unique_and_schemas_are_objects() {
        let mut ids: Vec<&str> = super::all().iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
        for c in super::all() {
            assert_eq!((c.params)()["type"], "object", "{}", c.id);
        }
    }

}
