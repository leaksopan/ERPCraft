//! VAT entries (written by document posting, one per posting group combination as BC's invoice
//! posting buffer does), the VAT statement (report 12 / page 474) and Calc. and Post VAT
//! Settlement (report 20).

use chrono::NaiveDate;
use erpcraft_meta::{FieldType, Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::ledger::Txn;
use crate::refuse;
use crate::schema::vat::{VAT_ENTRY, VAT_STMT_LINE};

/// One VAT entry to write.
pub struct VatSpec {
    pub gen_bus: String,
    pub gen_prod: String,
    pub vat_bus: String,
    pub vat_prod: String,
    /// "Sale" or "Purchase".
    pub vat_type: &'static str,
    pub base: Decimal,
    pub amount: Decimal,
}

/// Adds `base` / `amount` to the spec of the same posting groups (BC's posting buffer).
pub fn add(specs: &mut Vec<VatSpec>, s: VatSpec) {
    match specs.iter_mut().find(|x| x.gen_bus == s.gen_bus && x.gen_prod == s.gen_prod && x.vat_bus == s.vat_bus && x.vat_prod == s.vat_prod && x.vat_type == s.vat_type) {
        Some(x) => {
            x.base += s.base;
            x.amount += s.amount;
        }
        None => specs.push(s),
    }
}

/// The document a set of VAT entries belongs to.
pub struct VatDoc<'a> {
    pub posting_date: NaiveDate,
    pub document_date: Option<NaiveDate>,
    pub doc_type: &'a str,
    pub doc_no: &'a str,
    pub ext_doc_no: &'a str,
    pub bill_to: &'a str,
    pub vat_reg_no: &'a str,
    pub country: &'a str,
}

pub fn post(ctx: &mut Ctx, txn: &Txn, doc: &VatDoc, specs: &[VatSpec]) -> Result<()> {
    let t = ctx.table(VAT_ENTRY)?;
    for s in specs {
        let mut e = t.init_record();
        e.set("Entry No.", Value::Int(ctx.db.next_entry_no(VAT_ENTRY)));
        e.set("Gen. Bus. Posting Group", Value::text(&s.gen_bus));
        e.set("Gen. Prod. Posting Group", Value::text(&s.gen_prod));
        e.set("VAT Bus. Posting Group", Value::text(&s.vat_bus));
        e.set("VAT Prod. Posting Group", Value::text(&s.vat_prod));
        e.set("Posting Date", Value::Date(doc.posting_date));
        e.set("VAT Reporting Date", Value::Date(doc.posting_date));
        e.set("Document Date", Value::Date(doc.document_date.unwrap_or(doc.posting_date)));
        e.set("Document Type", Value::text(doc.doc_type));
        e.set("Document No.", Value::text(doc.doc_no));
        e.set("External Document No.", Value::text(doc.ext_doc_no));
        e.set("Type", Value::text(s.vat_type));
        e.set("Base", dec(s.base));
        e.set("Amount", dec(s.amount));
        e.set("VAT Calculation Type", Value::text("Normal VAT"));
        e.set("Bill-to/Pay-to No.", Value::text(doc.bill_to));
        e.set("VAT Registration No.", Value::text(doc.vat_reg_no));
        e.set("Country/Region Code", Value::text(doc.country));
        e.set("Transaction No.", Value::Int(txn.no));
        e.set("Source Code", Value::text(&txn.source_code));
        e.set("User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.insert(t, e)?;
    }
    Ok(())
}

fn date(p: &Json, k: &str) -> Result<Option<NaiveDate>> {
    match opt_str(p, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?)),
        None => Ok(None),
    }
}

fn in_range(d: Option<NaiveDate>, from: Option<NaiveDate>, to: Option<NaiveDate>) -> bool {
    d.is_some_and(|d| from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t))
}

fn code_matches(v: &str, filter: &str) -> bool {
    filter.trim().is_empty() || crate::filter::matches(&FieldType::Code(20), &Value::text(v), filter)
}

/// The template and name of a statement (from params or the selected statement line).
fn statement_ref(p: &Json) -> (String, String) {
    let from_key = match (opt_str(p, "table"), p.get("key").and_then(Json::as_array)) {
        (Some(VAT_STMT_LINE), Some(k)) => Some((
            k.first().and_then(Json::as_str).unwrap_or("VAT").to_string(),
            k.get(1).and_then(Json::as_str).unwrap_or("DEFAULT").to_string(),
        )),
        _ => None,
    };
    from_key.unwrap_or_else(|| (opt_str(p, "template").unwrap_or("VAT").to_uppercase(), opt_str(p, "name").unwrap_or("DEFAULT").to_uppercase()))
}

/// BC's VATStmtManagement.CalcLineTotal for one line.
fn line_total(ctx: &Ctx, lines: &[Record], l: &Record, from: Option<NaiveDate>, to: Option<NaiveDate>, selection: &str, depth: usize) -> Result<Decimal> {
    if depth > 20 {
        refuse!("Row {} refers to itself through Row Totaling.", l.str("Row No."));
    }
    let v = match l.str("Type") {
        "Account Totaling" => ctx
            .db
            .rows(crate::schema::tables::GL_ENTRY)
            .filter(|e| !l.str("Account Totaling").is_empty() && code_matches(e.str("G/L Account No."), l.str("Account Totaling")))
            .filter(|e| in_range(e.get("Posting Date").as_date(), from, to))
            .map(|e| e.get("Amount").as_dec())
            .sum(),
        "VAT Entry Totaling" => ctx
            .db
            .rows(VAT_ENTRY)
            .filter(|e| e.str("Type") == l.str("Gen. Posting Type"))
            .filter(|e| code_matches(e.str("VAT Bus. Posting Group"), l.str("VAT Bus. Posting Group")) && code_matches(e.str("VAT Prod. Posting Group"), l.str("VAT Prod. Posting Group")))
            .filter(|e| match selection {
                "Open" => !e.get("Closed").as_bool(),
                "Closed" => e.get("Closed").as_bool(),
                _ => true,
            })
            .filter(|e| in_range(e.get("VAT Reporting Date").as_date().or_else(|| e.get("Posting Date").as_date()), from, to))
            .map(|e| match l.str("Amount Type") {
                "Base" | "Full Base" => e.get("Base").as_dec(),
                "Amount" | "Full Amount" => e.get("Amount").as_dec(),
                _ => Decimal::ZERO,
            })
            .sum(),
        "Row Totaling" => {
            let mut sum = Decimal::ZERO;
            for r in lines.iter().filter(|r| !r.str("Row No.").is_empty() && code_matches(r.str("Row No."), l.str("Row Totaling"))) {
                if r.get("Line No.") == l.get("Line No.") {
                    continue;
                }
                sum += line_total(ctx, lines, r, from, to, selection, depth + 1)?;
            }
            sum
        }
        _ => Decimal::ZERO,
    };
    Ok(if l.str("Calculate with") == "Opposite Sign" { -v } else { v })
}

/// BC's VAT Statement Preview: every printed row with its amount for the period.
fn preview(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (template, name) = statement_ref(p);
    let mut lines: Vec<Record> =
        ctx.db.rows(VAT_STMT_LINE).filter(|l| l.str("Statement Template Name") == template && l.str("Statement Name") == name).cloned().collect();
    if lines.is_empty() {
        refuse!("The VAT statement {template} {name} has no lines.");
    }
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    let (from, to) = (date(p, "from")?, date(p, "to")?.or(Some(ctx.work_date)));
    let selection = opt_str(p, "selection").unwrap_or("Open and Closed").to_string();
    let mut rows = Vec::new();
    for l in lines.iter().filter(|l| l.get("Print").as_bool()) {
        let v = line_total(ctx, &lines, l, from, to, &selection, 0)?;
        let shown = if l.str("Print with") == "Opposite Sign" { -v } else { v };
        let amount = if l.str("Type") == "Description" { String::new() } else { format!("{:.2}", shown.round_dp(2)) };
        rows.push(json!([l.str("Row No."), l.str("Description"), l.str("Box No."), amount]));
    }
    let period = match from {
        Some(f) => format!("{f} .. {}", to.map(|t| t.to_string()).unwrap_or_default()),
        None => format!(".. {}", to.map(|t| t.to_string()).unwrap_or_default()),
    };
    Ok(json!({"report": {"title": format!("VAT Statement {name} ({period}, {selection})"), "columns": ["Row No.", "Description", "Box No.", "Amount"], "rows": rows}}))
}

/// BC's Calc. and Post VAT Settlement: per VAT posting setup, the open sales and purchase VAT of
/// the period is closed by a Settlement entry and moved to the settlement account.
fn settlement(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (from, to) = (date(p, "from")?, date(p, "to")?.or(Some(ctx.work_date)));
    let posting_date = date(p, "posting_date")?.or(to).ok_or_else(|| EngineError::refused("Enter the posting date."))?;
    let doc_no = opt_str(p, "document_no").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::refused("Enter the document no."))?.to_uppercase();
    let settlement_acc = opt_str(p, "settlement_account").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::refused("Enter the settlement account."))?.to_string();
    ctx.get_code(crate::schema::tables::GL_ACCOUNT, &settlement_acc)?;
    let post = p.get("post").and_then(Json::as_bool).unwrap_or(true);
    crate::posting::check_posting_date(ctx, posting_date)?;

    let open: Vec<Record> = ctx
        .db
        .rows(VAT_ENTRY)
        .filter(|e| !e.get("Closed").as_bool() && matches!(e.str("Type"), "Sale" | "Purchase"))
        .filter(|e| in_range(e.get("VAT Reporting Date").as_date().or_else(|| e.get("Posting Date").as_date()), from, to))
        .cloned()
        .collect();
    // (vat bus, vat prod, type) → (base, amount, entry nos.)
    let mut groups: Vec<((String, String, String), Decimal, Decimal, Vec<i64>)> = Vec::new();
    for e in &open {
        let k = (e.str("VAT Bus. Posting Group").to_string(), e.str("VAT Prod. Posting Group").to_string(), e.str("Type").to_string());
        let no = e.get("Entry No.").as_int();
        match groups.iter_mut().find(|g| g.0 == k) {
            Some(g) => {
                g.1 += e.get("Base").as_dec();
                g.2 += e.get("Amount").as_dec();
                g.3.push(no);
            }
            None => groups.push((k, e.get("Base").as_dec(), e.get("Amount").as_dec(), vec![no])),
        }
    }
    let rows: Vec<Json> = groups.iter().map(|((b, pr, ty), base, amount, _)| json!([b, pr, ty, format!("{:.2}", base), format!("{:.2}", amount)])).collect();
    let total: Decimal = groups.iter().map(|g| g.2).sum();
    let report = json!({"title": format!("Calc. and Post VAT Settlement{}", if post { "" } else { " - Test Report (Not Posted)" }),
        "columns": ["VAT Bus. Posting Group", "VAT Prod. Posting Group", "Type", "Base", "Amount"], "rows": rows});
    if !post || groups.is_empty() {
        return Ok(json!({"report": report, "total": total.to_string(), "message": if groups.is_empty() { "There is no open VAT to settle." } else { "" }}));
    }
    let txn = Txn::new(ctx, "VATSTMT", "", "");
    let mut gl = Vec::new();
    let vt = ctx.table(VAT_ENTRY)?;
    for ((bus, prod, ty), _, amount, nos) in &groups {
        let setup = ctx.get(crate::schema::tables::VAT_POSTING_SETUP, &Key(vec![Value::text(bus), Value::text(prod)]))?;
        let account = setup.str(if ty == "Sale" { "Sales VAT Account" } else { "Purchase VAT Account" }).to_string();
        // The VAT account is cleared; the settlement account takes the net.
        gl.push(crate::posting::GlLine { account, amount: -*amount, vat_amount: Decimal::ZERO, description: format!("VAT Settlement {bus} {prod}"), gen_posting_type: "" });
        let settle_no = ctx.db.next_entry_no(VAT_ENTRY);
        let mut s = vt.init_record();
        s.set("Entry No.", Value::Int(settle_no));
        s.set("VAT Bus. Posting Group", Value::text(bus));
        s.set("VAT Prod. Posting Group", Value::text(prod));
        s.set("Posting Date", Value::Date(posting_date));
        s.set("VAT Reporting Date", Value::Date(posting_date));
        s.set("Document No.", Value::text(&doc_no));
        s.set("Type", Value::text("Settlement"));
        s.set("Amount", dec(-*amount));
        s.set("Closed", Value::Bool(true));
        s.set("Transaction No.", Value::Int(txn.no));
        s.set("Source Code", Value::text(&txn.source_code));
        s.set("User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.insert(vt, s)?;
        for no in nos {
            let mut e = ctx.get(VAT_ENTRY, &Key(vec![Value::Int(*no)]))?;
            e.set("Closed", Value::Bool(true));
            e.set("Closed by Entry No.", Value::Int(settle_no));
            ctx.db.modify(vt, e)?;
        }
    }
    gl.push(crate::posting::GlLine { account: settlement_acc, amount: total, vat_amount: Decimal::ZERO, description: "VAT Settlement".to_string(), gen_posting_type: "" });
    crate::posting::post_gl(ctx, &crate::posting::GlDoc { posting_date, doc_type: "", doc_no: &doc_no, ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    Ok(json!({"report": report, "total": total.to_string(), "message": format!("The VAT settlement {doc_no} was posted: {} VAT entries closed.", open.len())}))
}

fn s_preview() -> Json {
    json!({"type": "object", "properties": {
        "template": {"type": "string", "default": "VAT"}, "name": {"type": "string", "default": "DEFAULT"},
        "from": {"type": "string", "description": "YYYY-MM-DD"}, "to": {"type": "string", "description": "YYYY-MM-DD; default the work date"},
        "selection": {"type": "string", "enum": ["Open", "Closed", "Open and Closed"], "default": "Open and Closed"},
        "table": {"type": "string"}, "key": {}}})
}

fn s_settlement() -> Json {
    json!({"type": "object", "properties": {
        "from": {"type": "string", "description": "YYYY-MM-DD"}, "to": {"type": "string", "description": "YYYY-MM-DD; default the work date"},
        "posting_date": {"type": "string", "description": "YYYY-MM-DD; default the ending date"},
        "document_no": {"type": "string"}, "settlement_account": {"type": "string"},
        "post": {"type": "boolean", "default": true}},
        "required": ["document_no", "settlement_account"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "vat_statement.preview", label: "Preview", group: "Finance", summary: "A VAT statement's rows and amounts for a period.", mutates: false, params: s_preview, run: preview },
    CommandDef {
        id: "vat.settlement",
        label: "Calc. and Post VAT Settlement",
        group: "Finance",
        summary: "Close the open sales and purchase VAT of a period and move it to the settlement account.",
        mutates: true,
        params: s_settlement,
        run: settlement,
    },
];
