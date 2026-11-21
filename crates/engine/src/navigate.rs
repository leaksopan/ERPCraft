//! BC's Navigate (page 344, "Find entries..."): every posted document and entry with a document
//! number (and posting date), counted per table, each count opening those records.

use erpcraft_meta::Key;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};

/// Where Navigate looks, in BC's order: posted documents (by No.), then entries (by Document No.).
const SOURCES: &[(&str, &str)] = &[
    ("Sales Shipment Header", "No."),
    ("Sales Invoice Header", "No."),
    ("Sales Cr.Memo Header", "No."),
    ("Purch. Rcpt. Header", "No."),
    ("Purch. Inv. Header", "No."),
    ("Purch. Cr. Memo Hdr.", "No."),
    ("G/L Entry", "Document No."),
    ("VAT Entry", "Document No."),
    ("Cust. Ledger Entry", "Document No."),
    ("Detailed Cust. Ledg. Entry", "Document No."),
    ("Vendor Ledger Entry", "Document No."),
    ("Detailed Vendor Ledg. Entry", "Document No."),
    ("Item Ledger Entry", "Document No."),
    ("Value Entry", "Document No."),
    ("Bank Account Ledger Entry", "Document No."),
    ("FA Ledger Entry", "Document No."),
];

/// The document number and posting date to look for: the parameters, or the selected record's.
fn target(ctx: &Ctx, p: &Json) -> Result<(String, String)> {
    if let (Some(table), Some(key)) = (opt_str(p, "table"), p.get("key")) {
        let t = ctx.table(table)?;
        let key = Key::from_json(t, key).map_err(EngineError::invalid)?;
        let r = ctx.get(&t.name, &key)?;
        let no = if t.field("Document No.").is_some() { r.str("Document No.") } else { r.str("No.") };
        return Ok((no.to_string(), r.get("Posting Date").as_date().map(|d| d.to_string()).unwrap_or_default()));
    }
    let no = opt_str(p, "document_no").ok_or_else(|| EngineError::invalid("missing parameter 'document_no'"))?;
    Ok((no.to_string(), opt_str(p, "posting_date").unwrap_or("").to_string()))
}

/// Find entries from a lot / serial number information card: the entries with that number.
fn navigate_tracking(ctx: &Ctx, table: &str, p: &Json) -> Result<Json> {
    let t = ctx.table(table)?;
    let r = ctx.get(&t.name, &Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?)?;
    let field = if table == crate::schema::tracking::LOT_INFO { "Lot No." } else { "Serial No." };
    let (item, no) = (r.str("Item No.").to_string(), r.str(field).to_string());
    let mut rows = Vec::new();
    let mut links = Vec::new();
    for (source, page) in [("Item Ledger Entry", "item-ledger-entries"), ("Reservation Entry", "reservation-entries")] {
        let count = ctx.db.rows(source).filter(|e| e.str("Item No.") == item && e.str(field) == no).count();
        if count > 0 {
            rows.push(json!([ctx.table(source)?.caption, count]));
            links.push(json!({"page": page, "filters": [["Item No.", item], [field, no]]}));
        }
    }
    let message = rows.is_empty().then(|| format!("There are no entries with {field} {no}."));
    Ok(json!({"message": message, "report": {"title": format!("Navigate - {field} {no}"), "columns": ["Related Entries", "No. of Entries"], "rows": rows, "links": links}}))
}

fn navigate(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    if let Some(table @ (crate::schema::tracking::LOT_INFO | crate::schema::tracking::SERIAL_INFO)) = opt_str(p, "table") {
        return navigate_tracking(ctx, table, p);
    }
    let (no, date) = target(ctx, p)?;
    if no.is_empty() {
        return Err(EngineError::invalid("There is no document number to find entries for."));
    }
    let mut rows = Vec::new();
    let mut links = Vec::new();
    for (table, field) in SOURCES {
        let Some(t) = ctx.meta.table(table) else { continue };
        let count = ctx
            .db
            .rows(table)
            .filter(|r| r.str(field).eq_ignore_ascii_case(&no))
            .filter(|r| date.is_empty() || r.get("Posting Date").as_date().map(|d| d.to_string()).as_deref() == Some(date.as_str()))
            .count();
        if count == 0 {
            continue;
        }
        let page = ctx.meta.pages.values().find(|pg| pg.source_table.as_deref() == Some(t.name.as_str()) && pg.kind == erpcraft_meta::PageKind::List);
        rows.push(json!([t.caption, count]));
        let mut filters = vec![json!([field, no])];
        if !date.is_empty() && t.field("Posting Date").is_some() {
            filters.push(json!(["Posting Date", date]));
        }
        links.push(page.map_or(Json::Null, |pg| json!({"page": pg.id, "filters": filters})));
    }
    let title = if date.is_empty() { format!("Navigate - {no}") } else { format!("Navigate - {no} - {date}") };
    let message = if rows.is_empty() { Some(format!("There are no posted entries with Document No. {no}.")) } else { None };
    Ok(json!({"document_no": no, "posting_date": date, "message": message, "report": {"title": title, "columns": ["Related Entries", "No. of Entries"], "rows": rows, "links": links}}))
}

fn s_navigate() -> Json {
    json!({"type": "object", "properties": {
        "document_no": {"type": "string"},
        "posting_date": {"type": "string", "description": "YYYY-MM-DD; blank: any date"}},
        "required": ["document_no"]})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "navigate",
    label: "Find entries",
    group: "Navigate",
    summary: "Every posted document and entry with a document number (and posting date), per table.",
    mutates: false,
    params: s_navigate,
    run: navigate,
}];
