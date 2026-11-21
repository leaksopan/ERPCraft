//! Fiscal years (BC's Accounting Period, report 93 Create Fiscal Year, codeunit 6 Fiscal
//! Year-Close, report 94 Close Income Statement) and the fiscal year of a date.
//!
//! BC posts the closing entries on a closing date (C31-12-2025) that sorts after the last day of
//! the year. ERPCraft has no closing dates: the closing lines carry source code CLSINCOME and the
//! financial reports treat those entries as falling just after their posting date.

use chrono::{Datelike, NaiveDate};
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::fiscal::{ACCOUNTING_PERIOD, GL_BUDGET_ENTRY};
use crate::schema::tables::{GL_ACCOUNT, GL_ENTRY};

/// BC's Source Code Setup › Close Income Statement.
pub const CLOSE_INCOME_SOURCE: &str = "CLSINCOME";

fn periods(ctx: &Ctx) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(ACCOUNTING_PERIOD).cloned().collect();
    v.sort_by_key(|p| p.get("Starting Date").as_date());
    v
}

/// The accounting period holding `date`: (start, end); calendar month without periods.
pub fn period_of(ctx: &Ctx, date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let ps = periods(ctx);
    let i = ps.iter().rposition(|p| p.get("Starting Date").as_date().is_some_and(|s| s <= date));
    match i.and_then(|i| Some((ps.get(i)?.get("Starting Date").as_date()?, ps.get(i + 1).and_then(|n| n.get("Starting Date").as_date())))) {
        Some((s, Some(next))) => (s, next.pred_opt().unwrap_or(next)),
        Some((s, None)) => (s, s.checked_add_months(chrono::Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(s)),
        None => {
            let s = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date);
            (s, s.checked_add_months(chrono::Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(s))
        }
    }
}

/// The fiscal year holding `date`, from the accounting periods (BC's AccountingPeriodMgt);
/// January – December without them.
pub fn fiscal_year(ctx: &Ctx, date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let starts: Vec<NaiveDate> =
        periods(ctx).iter().filter(|p| p.get("New Fiscal Year").as_bool()).filter_map(|p| p.get("Starting Date").as_date()).collect();
    match starts.iter().rposition(|s| *s <= date) {
        Some(i) => {
            let start = starts.get(i).copied().unwrap_or(date);
            let end = match starts.get(i + 1) {
                Some(next) => next.pred_opt().unwrap_or(*next),
                // The last year ends with its last period.
                None => periods(ctx).last().and_then(|p| p.get("Starting Date").as_date()).map(|s| period_of(ctx, s).1).unwrap_or(date),
            };
            (start, end.max(start))
        }
        None => (NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap_or(date), NaiveDate::from_ymd_opt(date.year(), 12, 31).unwrap_or(date)),
    }
}

fn date_param(p: &Json, k: &str) -> Result<Option<NaiveDate>> {
    match opt_str(p, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?)),
        None => Ok(None),
    }
}

/// BC's Create Fiscal Year: `no_of_periods` periods of `period_length` from `starting_date`, the
/// first one opening the year.
fn create_year(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let start = match date_param(p, "starting_date")? {
        Some(d) => d,
        // After the last year, or January 1 of the work date's year.
        None => periods(ctx).last().and_then(|l| l.get("Starting Date").as_date()).map(|s| period_of(ctx, s).1.succ_opt().unwrap_or(s)).unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(ctx.work_date.year(), 1, 1).unwrap_or(ctx.work_date)
        }),
    };
    let n = p.get("no_of_periods").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(12);
    if !(1..=365).contains(&n) {
        refuse!("No. of Periods must be between 1 and 365.");
    }
    let length = opt_str(p, "period_length").map(str::trim).filter(|s| !s.is_empty()).unwrap_or("1M").to_string();
    let t = ctx.table(ACCOUNTING_PERIOD)?;
    let first_existing = periods(ctx).first().and_then(|f| f.get("Starting Date").as_date());
    let mut d = start;
    let mut created = Vec::new();
    for i in 0..n {
        if ctx.db.get(ACCOUNTING_PERIOD, &Key(vec![Value::Date(d)])).is_some() {
            refuse!("The Accounting Period already exists. Identification fields and values: Starting Date='{d}'");
        }
        let mut r = t.init_record();
        r.set("Starting Date", Value::Date(d));
        r.set("Name", Value::text(d.format("%B").to_string()));
        r.set("New Fiscal Year", Value::Bool(i == 0));
        // A year before an existing one is closed at once (BC asks first).
        let closed = first_existing.is_some_and(|f| d < f);
        r.set("Closed", Value::Bool(closed));
        r.set("Date Locked", Value::Bool(closed));
        ctx.db.insert(t, r)?;
        created.push(d.to_string());
        let next = crate::dateformula::calc(&length, d)?;
        if next <= d {
            refuse!("The period length {length} must move the date forward.");
        }
        d = next;
    }
    Ok(json!({"periods": created, "message": format!("The fiscal year {} .. {} was created with {n} periods.", start, d.pred_opt().unwrap_or(d))}))
}

/// BC's Fiscal Year-Close: the earliest open fiscal year becomes Closed and Date Locked; a later
/// year must exist.
fn close_year(ctx: &mut Ctx, _p: &Json) -> Result<Json> {
    let ps = periods(ctx);
    let Some(first_open) = ps.iter().find(|p| p.get("New Fiscal Year").as_bool() && !p.get("Closed").as_bool()) else {
        refuse!("There is no open fiscal year to close.");
    };
    let start = first_open.get("Starting Date").as_date().unwrap_or(ctx.work_date);
    let Some(next_start) = ps.iter().filter(|p| p.get("New Fiscal Year").as_bool()).filter_map(|p| p.get("Starting Date").as_date()).find(|d| *d > start) else {
        refuse!("You must create a new fiscal year before you can close the old year.");
    };
    let t = ctx.table(ACCOUNTING_PERIOD)?;
    for mut p in ps.into_iter().filter(|p| p.get("Starting Date").as_date().is_some_and(|d| d >= start && d < next_start)) {
        p.set("Closed", Value::Bool(true));
        p.set("Date Locked", Value::Bool(true));
        ctx.db.modify(t, p)?;
    }
    let end = next_start.pred_opt().unwrap_or(next_start);
    Ok(json!({"message": format!("The fiscal year from {start} to {end} is closed.")}))
}

/// BC's Close Income Statement: journal lines that bring every income statement account of the
/// closed fiscal year to zero against the retained earnings account, dated the year's last day.
fn close_income_statement(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let ending = match date_param(p, "fiscal_year_ending_date")? {
        Some(d) => d,
        None => {
            // The last closed fiscal year.
            let closed: Vec<NaiveDate> =
                periods(ctx).iter().filter(|x| x.get("New Fiscal Year").as_bool() && x.get("Closed").as_bool()).filter_map(|x| x.get("Starting Date").as_date()).collect();
            match closed.last() {
                Some(s) => fiscal_year(ctx, *s).1,
                None => refuse!("Enter the ending date for the fiscal year."),
            }
        }
    };
    let (start, end) = fiscal_year(ctx, ending);
    if end != ending || !periods(ctx).iter().any(|x| x.get("Starting Date").as_date() == Some(start) && x.get("New Fiscal Year").as_bool()) {
        refuse!("The fiscal year does not exist.");
    }
    if periods(ctx).iter().any(|x| x.get("Starting Date").as_date().is_some_and(|d| d >= start && d <= end) && !x.get("Closed").as_bool()) {
        refuse!("The fiscal year must be closed before the income statement can be closed.");
    }
    let doc_no = opt_str(p, "document_no").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::refused("Enter a Document No."))?.to_uppercase();
    let retained = opt_str(p, "retained_earnings_account").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::refused("Enter Retained Earnings Account No."))?.to_string();
    let ra = ctx.get_code(GL_ACCOUNT, &retained)?;
    if ra.str("Income/Balance") != "Balance Sheet" || ra.str("Account Type") != "Posting" {
        refuse!("Retained Earnings Acc. must be a balance sheet posting account.");
    }
    let details = opt_str(p, "post_to_retained_earnings").is_some_and(|s| s.eq_ignore_ascii_case("Details"));
    let template = opt_str(p, "template").unwrap_or("GENERAL").to_uppercase();
    let batch = opt_str(p, "batch").unwrap_or("DEFAULT").to_uppercase();
    let description = opt_str(p, "posting_description").map(str::to_string).unwrap_or_else(|| format!("Close Income Statement {}", end.year()));
    // Each income statement account's net change for the year (earlier closings excluded).
    let mut amounts: Vec<(String, Decimal)> = Vec::new();
    for a in ctx.db.rows(GL_ACCOUNT).filter(|a| a.str("Income/Balance") == "Income Statement" && a.str("Account Type") == "Posting") {
        let no = a.str("No.");
        let sum: Decimal = ctx
            .db
            .rows(GL_ENTRY)
            .filter(|e| e.str("G/L Account No.") == no && e.str("Source Code") != CLOSE_INCOME_SOURCE)
            .filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d >= start && d <= end))
            .map(|e| e.get("Amount").as_dec())
            .sum();
        if !sum.is_zero() {
            amounts.push((no.to_string(), sum));
        }
    }
    if amounts.is_empty() {
        refuse!("There is nothing to close in the income statement for {start} .. {end}.");
    }
    let lt = ctx.table("Gen. Journal Line")?;
    let add = |ctx: &mut Ctx, account: &str, amount: Decimal| -> Result<()> {
        let mut v = Map::new();
        v.insert("Journal Template Name".into(), json!(template));
        v.insert("Journal Batch Name".into(), json!(batch));
        v.insert("Posting Date".into(), json!(end.to_string()));
        v.insert("Document No.".into(), json!(doc_no));
        v.insert("Account Type".into(), json!("G/L Account"));
        v.insert("Account No.".into(), json!(account));
        v.insert("Description".into(), json!(description));
        v.insert("Amount".into(), json!(amount.to_string()));
        let mut r = crate::records::create(ctx, lt, &v, None)?;
        r.set("Source Code", Value::text(CLOSE_INCOME_SOURCE));
        ctx.db.modify(lt, r)
    };
    let mut total = Decimal::ZERO;
    for (account, sum) in &amounts {
        add(ctx, account, -*sum)?;
        if details {
            add(ctx, &retained, *sum)?;
        }
        total += *sum;
    }
    if !details {
        add(ctx, &retained, total)?;
    }
    Ok(json!({"lines": amounts.len(), "message": format!("The journal lines to close the income statement for {start} .. {end} were created in {template} {batch}. Post the journal to close it.")}))
}

/// New budget entries are numbered and stamped (BC's G/L Budget Entry OnInsert).
pub fn on_budget_insert(ctx: &Ctx, r: &mut Record) {
    if r.get("Entry No.").as_int() == 0 {
        r.set("Entry No.", Value::Int(ctx.db.next_entry_no(GL_BUDGET_ENTRY)));
    }
    r.set("User ID", Value::text(ctx.user.to_uppercase()));
    r.set("Last Date-Modified", Value::Date(ctx.work_date));
}

/// New accounting periods are named after their month.
pub fn on_period_insert(r: &mut Record) {
    if r.str("Name").is_empty()
        && let Some(d) = r.get("Starting Date").as_date()
    {
        r.set("Name", Value::text(d.format("%B").to_string()));
    }
}

fn s_create() -> Json {
    json!({"type": "object", "properties": {
        "starting_date": {"type": "string", "description": "YYYY-MM-DD; default after the last period"},
        "no_of_periods": {"type": "integer", "default": 12},
        "period_length": {"type": "string", "default": "1M", "description": "a date formula: 1M, 1Q, 4W"}}})
}

fn s_none() -> Json {
    json!({"type": "object", "properties": {}})
}

fn s_close_income() -> Json {
    json!({"type": "object", "properties": {
        "fiscal_year_ending_date": {"type": "string", "description": "YYYY-MM-DD; default the last closed year's end"},
        "template": {"type": "string", "default": "GENERAL"}, "batch": {"type": "string", "default": "DEFAULT"},
        "document_no": {"type": "string"},
        "retained_earnings_account": {"type": "string"},
        "post_to_retained_earnings": {"type": "string", "enum": ["Balance", "Details"], "default": "Balance"},
        "posting_description": {"type": "string"}},
        "required": ["document_no", "retained_earnings_account"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "accounting_period.create_year", label: "Create Year", group: "Finance", summary: "Create a fiscal year of accounting periods.", mutates: true, params: s_create, run: create_year },
    CommandDef { id: "accounting_period.close_year", label: "Close Year", group: "Finance", summary: "Close the earliest open fiscal year (a later one must exist).", mutates: true, params: s_none, run: close_year },
    CommandDef {
        id: "accounting_period.close_income_statement",
        label: "Close Income Statement",
        group: "Finance",
        summary: "Create the journal lines that close a closed fiscal year's income statement accounts to retained earnings.",
        mutates: true,
        params: s_close_income,
        run: close_income_statement,
    },
];
