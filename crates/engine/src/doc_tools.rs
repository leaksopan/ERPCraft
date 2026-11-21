//! Document functions from BC's sales and purchase pages that build on posting and document
//! creation: Preview Posting, Test Report, Post and New and Copy Document.

use std::collections::BTreeSet;

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
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

/// Runs `post` on a copy of the company and returns the copy, so nothing is kept.
fn run_on_copy(ctx: &Ctx, post: impl FnOnce(&mut Ctx) -> Result<Json>) -> Result<(crate::db::Database, Json)> {
    let mut copy = ctx.db.clone();
    let mut scratch = Ctx { meta: ctx.meta, db: &mut copy, work_date: ctx.work_date, user: ctx.user };
    let posted = post(&mut scratch)?;
    Ok((copy, posted))
}

/// Posts the document on a copy of the company and returns the copy, so nothing is kept.
fn post_on_copy(ctx: &Ctx, side: &Side, p: &Json) -> Result<(crate::db::Database, Json)> {
    let (doc_type, no) = doc_ref(p, side)?;
    let mode = PostMode::parse(opt_str(p, "mode"))?;
    run_on_copy(ctx, |c| posting::post_document(c, side, &doc_type, &no, mode))
}

/// BC's Preview Posting: the entries posting would make, then nothing is written.
fn preview(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (copy, _) = post_on_copy(ctx, side, p)?;
    preview_report(ctx, &copy)
}

/// The Posting Preview of the entries `copy` has that the company doesn't.
fn preview_report(ctx: &Ctx, copy: &crate::db::Database) -> Result<Json> {
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

/// BC's Post and Print: posts, then prints the posted invoice or credit memo to `path`.
fn post_and_print(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let mode = PostMode::parse(opt_str(p, "mode"))?;
    let posted = posting::post_document(ctx, side, &doc_type, &no, mode)?;
    let (Some(table), Some(posted_no)) = (posted.get("posted_table").and_then(Json::as_str), posted.get("posted_no").and_then(Json::as_str)) else {
        // Shipped or received only: nothing invoiced to print.
        return Ok(posted);
    };
    let t = ctx.table(table)?;
    let key = Key(vec![Value::text(posted_no)]);
    let (bytes, title, pages) = crate::print::render(ctx, &t.name, &key, None)?;
    let mut out = posted.clone();
    out["title"] = json!(title);
    out["pages"] = json!(pages);
    match opt_str(p, "path").map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => {
            if !path.to_ascii_lowercase().ends_with(".pdf") {
                refuse!("The file name must end in .pdf.");
            }
            let path = std::path::Path::new(path);
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| EngineError::io(format!("{}: {e}", dir.display())))?;
            }
            std::fs::write(path, &bytes).map_err(|e| EngineError::io(format!("{}: {e}", path.display())))?;
            out["path"] = json!(path.display().to_string());
        }
        None => out["pdf_base64"] = json!(crate::print::base64(&bytes)),
    }
    Ok(out)
}

/// BC's posted invoice / credit memo Statistics: amounts, VAT, cost and profit of the posted lines.
fn posted_statistics(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let table = opt_str(p, "table").ok_or_else(|| EngineError::invalid("missing parameter 'table'"))?;
    let (line_table, sales) = match table {
        "Sales Invoice Header" => ("Sales Invoice Line", true),
        "Sales Cr.Memo Header" => ("Sales Cr.Memo Line", true),
        "Purch. Inv. Header" => ("Purch. Inv. Line", false),
        "Purch. Cr. Memo Hdr." => ("Purch. Cr. Memo Line", false),
        other => refuse!("Statistics are available for posted invoices and credit memos, not {other}."),
    };
    let t = ctx.table(table)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    let h = ctx.get(&t.name, &key)?;
    let no = h.str("No.").to_string();
    let lines: Vec<&Record> = ctx.db.rows(line_table).filter(|l| l.str("Document No.") == no).collect();
    let sum = |f: &str| -> Decimal { lines.iter().map(|l| l.get(f).as_dec()).sum() };
    let amount = sum("Amount");
    let incl = sum("Amount Including VAT");
    let mut rows = vec![
        json!(["Amount", amount.to_string()]),
        json!(["VAT Amount", (incl - amount).to_string()]),
        json!(["Amount Including VAT", incl.to_string()]),
        json!(["Quantity", sum("Quantity").to_string()]),
        json!(["No. of Lines", lines.len()]),
    ];
    if sales {
        let cost: Decimal = lines.iter().map(|l| l.get("Quantity").as_dec() * l.get("Unit Cost (LCY)").as_dec()).sum();
        let cost = crate::ctx::round_amount(cost);
        let profit = amount - cost;
        let pct = if amount.is_zero() { Decimal::ZERO } else { crate::ctx::round_amount(profit * Decimal::ONE_HUNDRED / amount) };
        rows.push(json!(["Cost (LCY)", cost.to_string()]));
        rows.push(json!(["Profit (LCY)", profit.to_string()]));
        rows.push(json!(["Profit %", pct.to_string()]));
    }
    Ok(json!({"report": {"title": format!("{} Statistics - {no}", t.caption), "columns": ["", "Value"], "rows": rows}}))
}

fn s_posted_stats() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}}, "required": ["table", "key"]})
}

fn s_post_print() -> Json {
    json!({"type": "object", "properties": {
        "document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"},
        "mode": {"type": "string", "description": "orders only: Ship / Receive, Invoice, or both (the default)"},
        "path": {"type": "string", "description": "write the PDF here (must end in .pdf); without it the PDF comes back as base64"}},
        "required": ["no"]})
}

fn journal_ref(p: &Json, default_template: &str) -> (String, String) {
    (
        opt_str(p, "template").unwrap_or(default_template).to_uppercase(),
        opt_str(p, "batch").unwrap_or("DEFAULT").to_uppercase(),
    )
}

/// Preview Posting on a general journal batch.
fn journal_preview(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, b) = journal_ref(p, "GENERAL");
    let (copy, _) = run_on_copy(ctx, |c| crate::journal::post(c, &t, &b))?;
    preview_report(ctx, &copy)
}

/// Preview Posting on an item journal batch.
fn item_journal_preview(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, b) = journal_ref(p, "ITEM");
    let (copy, _) = run_on_copy(ctx, |c| crate::item_journal::post(c, &t, &b))?;
    preview_report(ctx, &copy)
}

/// BC's journal Test Report: every line checked as posting would, without posting.
fn journal_test(ctx: &mut Ctx, p: &Json, item: bool) -> Result<Json> {
    let (t, b) = journal_ref(p, if item { "ITEM" } else { "GENERAL" });
    let mut problems = if item { Vec::new() } else { crate::journal::check(ctx, &t, &b)? };
    if problems.is_empty() {
        let posted = run_on_copy(ctx, |c| if item { crate::item_journal::post(c, &t, &b) } else { crate::journal::post(c, &t, &b) });
        if let Err(e) = posted {
            problems.push(e.message);
        }
    }
    let rows: Vec<Json> = if problems.is_empty() { vec![json!(["There are no errors. The journal can be posted."])] } else { problems.iter().map(|m| json!([m])).collect() };
    Ok(json!({"ok": problems.is_empty(), "report": {"title": format!("{t} / {b} - Test Report"), "columns": ["Result"], "rows": rows}}))
}

fn journal_test_report(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    journal_test(ctx, p, false)
}

fn item_journal_test_report(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    journal_test(ctx, p, true)
}

/// BC's Renumber Document Numbers: the batch's documents numbered again from its number series,
/// in line order, lines of one document staying together.
fn renumber(ctx: &mut Ctx, p: &Json, table: &str, batch_table: &str, default_template: &str) -> Result<Json> {
    let (t, b) = journal_ref(p, default_template);
    let batch = ctx.get(batch_table, &Key(vec![Value::text(&t), Value::text(&b)]))?;
    let series = batch.str("No. Series").to_string();
    if series.is_empty() {
        refuse!("No. Series must have a value in the journal batch {t} {b}.");
    }
    let mut next = crate::noseries::peek(ctx, &series)?;
    let lt = ctx.table(table)?;
    let mut lines: Vec<Record> =
        ctx.db.rows(table).filter(|l| l.str("Journal Template Name") == t && l.str("Journal Batch Name") == b).cloned().collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    let mut map: Vec<(String, String)> = Vec::new();
    let mut changed = 0;
    for mut l in lines {
        let old = l.str("Document No.").to_string();
        let new = match map.iter().find(|(o, _)| *o == old) {
            Some((_, n)) => n.clone(),
            None => {
                let n = next.clone();
                next = crate::noseries::increment(&next, 1).ok_or_else(|| EngineError::invalid(format!("The number {n} can't be increased.")))?;
                map.push((old.clone(), n.clone()));
                n
            }
        };
        if new != old {
            l.set("Document No.", Value::text(&new));
            ctx.db.modify(lt, l)?;
            changed += 1;
        }
    }
    Ok(json!({"lines": changed, "message": format!("{changed} line(s) were renumbered.")}))
}

fn journal_renumber(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    renumber(ctx, p, "Gen. Journal Line", "Gen. Journal Batch", "GENERAL")
}

fn item_journal_renumber(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    renumber(ctx, p, "Item Journal Line", "Item Journal Batch", "ITEM")
}

fn s_journal() -> Json {
    json!({"type": "object", "properties": {"template": {"type": "string"}, "batch": {"type": "string", "default": "DEFAULT"}}})
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
per_side!(sales_post_print, purch_post_print, post_and_print);

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
    cmd!("sales.document.post_and_print", "Post and Print", "Sales", true, s_post_print, sales_post_print, "Post a sales document and print the posted invoice or credit memo."),
    cmd!("purchase.document.post_and_print", "Post and Print", "Purchasing", true, s_post_print, purch_post_print, "Post a purchase document and print the posted invoice or credit memo."),
    cmd!("document.posted_statistics", "Statistics", "Documents", false, s_posted_stats, posted_statistics, "Amounts, VAT, cost and profit of a posted invoice or credit memo."),
    cmd!("journal.preview_post", "Preview Posting", "Finance", false, s_journal, journal_preview, "The entries posting a general journal batch would make, without posting it."),
    cmd!("journal.test_report", "Test Report", "Finance", false, s_journal, journal_test_report, "What would stop a general journal batch from posting."),
    cmd!("journal.renumber", "Renumber Document Numbers", "Finance", true, s_journal, journal_renumber, "Number a journal batch's documents again from its number series."),
    cmd!("item_journal.preview_post", "Preview Posting", "Inventory", false, s_journal, item_journal_preview, "The entries posting an item journal batch would make, without posting it."),
    cmd!("item_journal.test_report", "Test Report", "Inventory", false, s_journal, item_journal_test_report, "What would stop an item journal batch from posting."),
    cmd!("item_journal.renumber", "Renumber Document Numbers", "Inventory", true, s_journal, item_journal_renumber, "Number an item journal batch's documents again from its number series."),
    cmd!("purchase.document.preview_post", "Preview Posting", "Purchasing", false, s_doc_mode, purch_preview, "The entries posting a purchase document would make, without posting it."),
    cmd!("purchase.document.test_report", "Test Report", "Purchasing", false, s_doc_mode, purch_test, "What would stop a purchase document from posting."),
    cmd!("purchase.document.post_and_new", "Post and New", "Purchasing", true, s_doc_mode, purch_post_new, "Post a purchase document and open a new one of the same type."),
    cmd!("purchase.document.copy", "Copy Document", "Purchasing", true, s_copy, purch_copy, "Copy the lines (and vendor) of another purchase document or posted invoice / credit memo."),
];
