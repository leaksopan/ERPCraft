//! Document functions from BC's sales and purchase pages that build on posting and document
//! creation: Preview Posting, Test Report, Post and New and Copy Document.

use std::collections::BTreeSet;

use erpcraft_meta::{Key, Record, Value};
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, doc_ref, opt_str};
use crate::ctx::Ctx;
use crate::documents;
use crate::error::{EngineError, Result};
use crate::posting::{self, PostMode};
use crate::refuse;
use crate::side::{PURCHASE, SALES, Side};

/// Ledger tables a posting writes to, in the order BC's Posting Preview lists them.
const ENTRY_TABLES: &[&str] = &[
    "G/L Entry",
    "VAT Entry",
    "Cust. Ledger Entry",
    "Detailed Cust. Ledg. Entry",
    "Vendor Ledger Entry",
    "Detailed Vendor Ledg. Entry",
    "Item Ledger Entry",
    "Value Entry",
    "Bank Account Ledger Entry",
    "FA Ledger Entry",
];

/// Posts the document on a copy of the company and returns the copy, so nothing is kept.
fn post_on_copy(ctx: &Ctx, side: &Side, p: &Json) -> Result<(crate::db::Database, Json)> {
    let (doc_type, no) = doc_ref(p, side)?;
    let mode = PostMode::parse(opt_str(p, "mode"))?;
    let mut copy = ctx.db.clone();
    let mut scratch = Ctx { meta: ctx.meta, db: &mut copy, work_date: ctx.work_date, user: ctx.user };
    let posted = posting::post_document(&mut scratch, side, &doc_type, &no, mode)?;
    Ok((copy, posted))
}

/// BC's Preview Posting: the entries posting would make, then nothing is written.
fn preview(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (copy, _) = post_on_copy(ctx, side, p)?;
    let mut rows = Vec::new();
    let mut entries = Map::new();
    for table in ENTRY_TABLES {
        let Some(t) = ctx.meta.table(table) else { continue };
        let before: BTreeSet<Key> = ctx.db.rows(table).map(|r| r.key(t)).collect();
        let new: Vec<&Record> = copy.rows(table).filter(|r| !before.contains(&r.key(t))).collect();
        if new.is_empty() {
            continue;
        }
        rows.push(json!([t.caption, new.len()]));
        entries.insert(t.name.clone(), Json::Array(new.iter().map(|r| r.to_json(t)).collect()));
    }
    let gl_rows: Vec<Json> = entries
        .get("G/L Entry")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .map(|e| {
                    let s = |f: &str| e.get(f).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).unwrap_or_default();
                    json!([s("G/L Account No."), s("Description"), s("Amount")])
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "message": "Posting Preview: these entries are not posted.",
        "entries": entries,
        "report": {
            "title": "Posting Preview",
            "columns": ["Related Entries", "No. of Entries"],
            "rows": rows,
            "details": {"title": "G/L Entries", "columns": ["G/L Account No.", "Description", "Amount"], "rows": gl_rows},
        },
    }))
}

/// BC's Test Report: what posting would refuse, without posting.
fn test_report(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let (ok, text) = match post_on_copy(ctx, side, p) {
        Ok(_) => (true, "There are no errors. The document can be posted.".to_string()),
        Err(e) => (false, e.message),
    };
    Ok(json!({
        "ok": ok,
        "report": {"title": format!("{} {doc_type} {no} - Test Report", if side.sales { "Sales" } else { "Purchase" }), "columns": ["Result"], "rows": [[text]]},
    }))
}

/// BC's Post and New: posts, then opens a new document of the same type.
fn post_and_new(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let mode = PostMode::parse(opt_str(p, "mode"))?;
    let posted = posting::post_document(ctx, side, &doc_type, &no, mode)?;
    let created = documents::new_document(ctx, side, &doc_type, "", &Map::new(), &[])?;
    let message = format!(
        "{} {}",
        posted.get("message").and_then(Json::as_str).unwrap_or("The document was posted."),
        created.get("message").and_then(Json::as_str).unwrap_or_default()
    );
    Ok(json!({"posted": posted, "open": created.get("open"), "no": created.get("no"), "message": message}))
}

/// Where Copy Document reads from: header table, line table, the header's key and the line link.
struct Source {
    header: &'static str,
    line: &'static str,
    key: Key,
    /// (line field, header value) pairs that select the lines.
    link: Vec<(&'static str, Value)>,
}

fn source(side: &Side, from_type: &str, from_no: &str) -> Result<Source> {
    let posted = |header: &'static str, line: &'static str| Source {
        header,
        line,
        key: Key(vec![Value::text(from_no)]),
        link: vec![("Document No.", Value::text(from_no))],
    };
    Ok(match from_type {
        "Quote" | "Order" | "Invoice" | "Credit Memo" => Source {
            header: side.header,
            line: side.line,
            key: documents::header_key(from_type, from_no),
            link: vec![("Document Type", Value::text(from_type)), ("Document No.", Value::text(from_no))],
        },
        "Posted Invoice" => posted(side.inv_header, side.inv_line),
        "Posted Credit Memo" => posted(side.cr_header, side.cr_line),
        other => refuse!(
            "Copy Document cannot copy from '{other}'. Choose Quote, Order, Invoice, Credit Memo, Posted Invoice or Posted Credit Memo."
        ),
    })
}

/// BC's Copy Document: the source's lines (and, with Include Header, its customer or vendor) onto
/// this document. Without Recalculate Lines, prices and discounts are copied as they were.
fn copy_document(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let from_type = opt_str(p, "from_document_type").ok_or_else(|| EngineError::invalid("missing parameter 'from_document_type'"))?;
    let from_no = opt_str(p, "from_no").ok_or_else(|| EngineError::invalid("missing parameter 'from_no'"))?;
    let include_header = flag(p, "include_header", true);
    let recalculate = flag(p, "recalculate_lines", false);
    let src = source(side, from_type, from_no)?;
    if src.header == side.header && from_type == doc_type && from_no == no {
        refuse!("A document cannot be copied onto itself.");
    }
    let src_header = ctx.get(src.header, &src.key)?;
    let target_key = documents::header_key(&doc_type, &no);
    let ht = ctx.table(side.header)?;
    let target = ctx.get(side.header, &target_key)?;
    if target.str("Status") != "Open" && !target.str("Status").is_empty() {
        refuse!("Status must be equal to 'Open' in {}: Document Type={doc_type}, No.={no}. Current value is '{}'.", ht.caption, target.str("Status"));
    }
    if include_header {
        let party = src_header.str(side.party_no).to_string();
        if !party.is_empty() && party != target.str(side.party_no) {
            let mut v = Map::new();
            v.insert(side.party_no.to_string(), json!(party));
            crate::records::update(ctx, ht, &target_key, &v)?;
        }
    }
    let mut lines: Vec<Record> = ctx.db.rows(src.line).filter(|l| src.link.iter().all(|(f, v)| l.get(f) == v)).cloned().collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    let price = side.price;
    let mut copied = 0usize;
    for l in &lines {
        let mut spec = Map::new();
        let ty = l.str("Type").to_string();
        spec.insert("Type".into(), json!(ty));
        if !l.str("No.").is_empty() {
            spec.insert("No.".into(), json!(l.str("No.")));
        }
        spec.insert("Description".into(), json!(l.str("Description")));
        if !l.get("Quantity").as_dec().is_zero() {
            spec.insert("Quantity".into(), l.get("Quantity").to_json());
            if !recalculate {
                spec.insert(price.into(), l.get(price).to_json());
                spec.insert("Line Discount %".into(), l.get("Line Discount %").to_json());
            }
        }
        documents::add_line(ctx, side, &doc_type, &no, &Json::Object(spec))?;
        copied += 1;
    }
    Ok(json!({
        "lines": copied,
        "message": format!("{copied} line(s) were copied from {from_type} {from_no}."),
    }))
}

/// A Boolean parameter, as JSON true/false or the "Yes" / "No" a form sends.
fn flag(p: &Json, name: &str, default: bool) -> bool {
    match p.get(name) {
        Some(Json::Bool(b)) => *b,
        Some(Json::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" => true,
            "no" | "false" | "0" => false,
            _ => default,
        },
        _ => default,
    }
}

fn s_doc_mode() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"},
        "mode": {"type": "string", "description": "orders only: Ship / Receive, Invoice, or both (the default)"}},
        "required": ["no"]})
}

fn s_copy() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "default": "Order"}, "no": {"type": "string", "description": "the document to copy onto"},
        "from_document_type": {"type": "string", "enum": ["Quote", "Order", "Invoice", "Credit Memo", "Posted Invoice", "Posted Credit Memo"]},
        "from_no": {"type": "string"},
        "include_header": {"type": "boolean", "default": true, "description": "take the customer or vendor too"},
        "recalculate_lines": {"type": "boolean", "default": false, "description": "price the lines again instead of copying prices"}},
        "required": ["no", "from_document_type", "from_no"]})
}

/// BC's Select items: a line for each chosen item (no quantity yet, as BC adds them).
fn select_items(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let items: Vec<String> = match p.get("items") {
        Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(Json::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        _ => Vec::new(),
    };
    if items.is_empty() {
        refuse!("Select at least one item.");
    }
    for item in &items {
        documents::add_line(ctx, side, &doc_type, &no, &json!({"Type": "Item", "No.": item}))?;
    }
    Ok(json!({"lines": items.len(), "message": format!("{} line(s) were added.", items.len())}))
}

fn s_items() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"},
        "items": {"type": "array", "items": {"type": "string"}, "description": "item numbers"}},
        "required": ["no", "items"]})
}

macro_rules! per_side {
    ($sales:ident, $purch:ident, $f:ident) => {
        fn $sales(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, &SALES, p)
        }
        fn $purch(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, &PURCHASE, p)
        }
    };
}
per_side!(sales_preview, purch_preview, preview);
per_side!(sales_test, purch_test, test_report);
per_side!(sales_post_new, purch_post_new, post_and_new);
per_side!(sales_copy, purch_copy, copy_document);
per_side!(sales_items, purch_items, select_items);

macro_rules! cmd {
    ($id:literal, $label:literal, $group:literal, $mutates:expr, $params:expr, $run:expr, $summary:literal) => {
        CommandDef { id: $id, label: $label, group: $group, summary: $summary, mutates: $mutates, params: $params, run: $run }
    };
}

pub static COMMANDS: &[CommandDef] = &[
    cmd!("sales.document.preview_post", "Preview Posting", "Sales", false, s_doc_mode, sales_preview, "The entries posting a sales document would make, without posting it."),
    cmd!("sales.document.test_report", "Test Report", "Sales", false, s_doc_mode, sales_test, "What would stop a sales document from posting."),
    cmd!("sales.document.post_and_new", "Post and New", "Sales", true, s_doc_mode, sales_post_new, "Post a sales document and open a new one of the same type."),
    cmd!("sales.document.copy", "Copy Document", "Sales", true, s_copy, sales_copy, "Copy the lines (and customer) of another sales document or posted invoice / credit memo."),
    cmd!("sales.document.select_items", "Select items", "Sales", true, s_items, sales_items, "Add a line for each of the chosen items."),
    cmd!("purchase.document.select_items", "Select items", "Purchasing", true, s_items, purch_items, "Add a line for each of the chosen items."),
    cmd!("purchase.document.preview_post", "Preview Posting", "Purchasing", false, s_doc_mode, purch_preview, "The entries posting a purchase document would make, without posting it."),
    cmd!("purchase.document.test_report", "Test Report", "Purchasing", false, s_doc_mode, purch_test, "What would stop a purchase document from posting."),
    cmd!("purchase.document.post_and_new", "Post and New", "Purchasing", true, s_doc_mode, purch_post_new, "Post a purchase document and open a new one of the same type."),
    cmd!("purchase.document.copy", "Copy Document", "Purchasing", true, s_copy, purch_copy, "Copy the lines (and vendor) of another purchase document or posted invoice / credit memo."),
];
