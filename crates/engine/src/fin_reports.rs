//! BC's financial reports (AccSchedManagement): a row definition (Acc. Schedule Line) evaluated
//! against a column definition (Column Layout) for a date filter, from the G/L entries.
//!
//! Rows total posting accounts, total accounts or account categories, or are formulas over other
//! rows (`10+20`, `-R30`, `40..60`); columns take the amounts as Net Change, Balance at Date,
//! Beginning Balance, Year / Month to Date, Rest of / Entire Fiscal Year, optionally shifted by a
//! comparison date formula, or are formulas over other columns.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use erpcraft_meta::{FieldType, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::fin_reports::{ACC_SCHED_LINE, COLUMN_LAYOUT, FINANCIAL_REPORT};
use crate::schema::tables::{GL_ACCOUNT, GL_ENTRY};

/// The G/L entries of one account, as (posting date, amount).
type Entries = HashMap<String, Vec<(NaiveDate, Decimal)>>;

fn entries_by_account(ctx: &Ctx) -> Entries {
    let mut m: Entries = HashMap::new();
    for e in ctx.db.rows(GL_ENTRY) {
        if let Some(d) = e.get("Posting Date").as_date() {
            m.entry(e.str("G/L Account No.").to_string()).or_default().push((d, e.get("Amount").as_dec()));
        }
    }
    m
}

/// The amount of `accounts` between `from` and `to` (both inclusive, open ends allowed).
fn amount(entries: &Entries, accounts: &[String], from: Option<NaiveDate>, to: Option<NaiveDate>, amount_type: &str) -> Decimal {
    accounts
        .iter()
        .filter_map(|a| entries.get(a))
        .flatten()
        .filter(|(d, _)| from.is_none_or(|f| *d >= f) && to.is_none_or(|t| *d <= t))
        .map(|(_, a)| match amount_type {
            "Debit Amount" => (*a).max(Decimal::ZERO),
            "Credit Amount" => (-*a).max(Decimal::ZERO),
            _ => *a,
        })
        .sum()
}

fn code_matches(no: &str, filter: &str) -> bool {
    crate::filter::matches(&FieldType::Code(20), &Value::text(no), filter)
}

/// The posting accounts a row adds up.
fn row_accounts(ctx: &Ctx, line: &Record) -> Vec<String> {
    let filter = line.str("Totaling").trim().to_string();
    if filter.is_empty() {
        return Vec::new();
    }
    let accounts: Vec<&Record> = ctx.db.rows(GL_ACCOUNT).collect();
    let posting = |a: &&&Record| a.str("Account Type") == "Posting";
    match line.str("Totaling Type") {
        "Posting Accounts" => accounts.iter().filter(posting).filter(|a| code_matches(a.str("No."), &filter)).map(|a| a.str("No.").to_string()).collect(),
        "Total Accounts" => {
            // A total account adds up the posting accounts in its own Totaling.
            let mut out = Vec::new();
            for t in accounts.iter().filter(|a| matches!(a.str("Account Type"), "Total" | "End-Total") && code_matches(a.str("No."), &filter)) {
                let range = t.str("Totaling");
                for a in accounts.iter().filter(posting).filter(|a| !range.is_empty() && code_matches(a.str("No."), range)) {
                    if !out.iter().any(|x: &String| x == a.str("No.")) {
                        out.push(a.str("No.").to_string());
                    }
                }
            }
            out
        }
        "Account Category" => accounts
            .iter()
            .filter(posting)
            .filter(|a| crate::filter::matches(&FieldType::Text(80), &Value::text(a.str("Account Category")), &filter))
            .map(|a| a.str("No.").to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// January 1 – December 31 of `d`'s year (ERPCraft has no accounting periods yet).
fn fiscal_year(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    (NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap_or(d), NaiveDate::from_ymd_opt(d.year(), 12, 31).unwrap_or(d))
}

/// The dates a column type looks at, for the report's date filter `from..to`.
fn column_range(column_type: &str, row_type: &str, from: NaiveDate, to: NaiveDate) -> (Option<NaiveDate>, Option<NaiveDate>) {
    let (fy0, fy1) = fiscal_year(to);
    // A row's own type wins for amount columns (BC: Balance at Date / Beginning Balance rows).
    let ty = match (row_type, column_type) {
        ("Balance at Date", "Net Change") => "Balance at Date",
        ("Beginning Balance", "Net Change") => "Beginning Balance",
        _ => column_type,
    };
    match ty {
        "Balance at Date" => (None, Some(to)),
        "Beginning Balance" => (None, from.pred_opt()),
        "Year to Date" => (Some(fy0), Some(to)),
        "Rest of Fiscal Year" => (to.succ_opt(), Some(fy1)),
        "Entire Fiscal Year" => (Some(fy0), Some(fy1)),
        "Month to Date" => (NaiveDate::from_ymd_opt(to.year(), to.month(), 1), Some(to)),
        _ => (Some(from), Some(to)),
    }
}

// ---- formulas: `10+20`, `-(R10 - R20) * 2`, `40..60` (the sum of rows 40 to 60) ----

struct Formula<'a> {
    s: &'a [u8],
    i: usize,
}

impl Formula<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }

    fn token(&mut self) -> String {
        self.peek();
        let start = self.i;
        while let Some(c) = self.s.get(self.i).copied() {
            let range = c == b'.' && self.s.get(self.i + 1) == Some(&b'.');
            if range || !(c.is_ascii_alphanumeric() || c == b'_' || c == b'.') {
                break;
            }
            self.i += 1;
        }
        String::from_utf8_lossy(self.s.get(start..self.i).unwrap_or_default()).to_string()
    }

    fn expr(&mut self, value: &dyn Fn(&str, Option<&str>) -> Option<Decimal>) -> Result<Decimal> {
        let mut v = self.term(value)?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term(value)?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term(value)?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn term(&mut self, value: &dyn Fn(&str, Option<&str>) -> Option<Decimal>) -> Result<Decimal> {
        let mut v = self.factor(value)?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.factor(value)?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.factor(value)?;
                    v = if d.is_zero() { Decimal::ZERO } else { v / d };
                }
                Some(b'%') => {
                    self.i += 1;
                    let d = self.factor(value)?;
                    v = if d.is_zero() { Decimal::ZERO } else { v / d * Decimal::ONE_HUNDRED };
                }
                _ => return Ok(v),
            }
        }
    }

    fn factor(&mut self, value: &dyn Fn(&str, Option<&str>) -> Option<Decimal>) -> Result<Decimal> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.factor(value)?)
            }
            Some(b'+') => {
                self.i += 1;
                self.factor(value)
            }
            Some(b'(') => {
                self.i += 1;
                let v = self.expr(value)?;
                if self.peek() != Some(b')') {
                    refuse!("The parenthesis in the formula are not balanced.");
                }
                self.i += 1;
                Ok(v)
            }
            Some(_) => {
                let a = self.token();
                if a.is_empty() {
                    refuse!("The formula contains an invalid character.");
                }
                let b = if self.s.get(self.i..self.i + 2) == Some(b"..") {
                    self.i += 2;
                    Some(self.token())
                } else {
                    None
                };
                if let Some(v) = value(&a, b.as_deref()) {
                    return Ok(v);
                }
                if b.is_none()
                    && let Ok(n) = a.parse::<Decimal>()
                {
                    return Ok(n);
                }
                refuse!("{a} is not a row or column number, nor a number.");
            }
            None => refuse!("The formula ends too early."),
        }
    }
}

/// Evaluates `formula` with `value(code)` / `value(from, Some(to))` giving row or column amounts.
fn eval(formula: &str, value: &dyn Fn(&str, Option<&str>) -> Option<Decimal>) -> Result<Decimal> {
    let mut f = Formula { s: formula.as_bytes(), i: 0 };
    let v = f.expr(value)?;
    if f.peek().is_some() {
        refuse!("The formula {formula} is not valid.");
    }
    Ok(v)
}

fn round(d: Decimal, factor: &str) -> String {
    // No "-0.00": a value that rounds to zero is zero.
    let d = if d.abs() < Decimal::new(5, 3) { Decimal::ZERO } else { d };
    match factor {
        "1" => format!("{:.0}", d.round_dp(0)),
        "1000" => format!("{:.1}", (d / Decimal::from(1000)).round_dp(1)),
        "1000000" => format!("{:.1}", (d / Decimal::from(1_000_000)).round_dp(1)),
        _ => format!("{:.2}", d.round_dp(2)),
    }
}

fn sorted(ctx: &Ctx, table: &str, key: &str, name: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(table).filter(|r| r.str(key) == name).cloned().collect();
    v.sort_by_key(|r| r.get("Line No.").as_int());
    v
}

fn parse_date(p: &Json, k: &str) -> Result<Option<NaiveDate>> {
    match opt_str(p, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?)),
        None => Ok(None),
    }
}

/// BC's Acc. Schedule Overview: every shown row with its amount per shown column.
pub fn overview(ctx: &Ctx, rows_name: &str, columns_name: &str, from: NaiveDate, to: NaiveDate, title: &str) -> Result<Json> {
    let lines = sorted(ctx, ACC_SCHED_LINE, "Schedule Name", rows_name);
    if lines.is_empty() {
        refuse!("The row definition {rows_name} has no rows.");
    }
    let cols = sorted(ctx, COLUMN_LAYOUT, "Column Layout Name", columns_name);
    if cols.is_empty() {
        refuse!("The column definition {columns_name} has no columns.");
    }
    let entries = entries_by_account(ctx);
    // Amount columns first, per row; formula rows and columns after, in order.
    let mut values: Vec<Vec<Decimal>> = vec![vec![Decimal::ZERO; cols.len()]; lines.len()];
    let row_index = |code: &str| lines.iter().position(|l| !l.str("Row No.").is_empty() && l.str("Row No.").eq_ignore_ascii_case(code));
    let col_index = |code: &str| cols.iter().position(|c| !c.str("Column No.").is_empty() && c.str("Column No.").eq_ignore_ascii_case(code));
    for (ci, c) in cols.iter().enumerate() {
        if c.str("Column Type") == "Formula" {
            continue;
        }
        let (mut f, mut t) = (from, to);
        let cmp = c.str("Comparison Date Formula").trim();
        if !cmp.is_empty() {
            f = crate::dateformula::calc(cmp, from)?;
            t = crate::dateformula::calc(cmp, to)?;
        }
        for (ri, l) in lines.iter().enumerate() {
            if matches!(l.str("Totaling Type"), "Formula" | "Set Base For Percent") {
                continue;
            }
            let (a, b) = column_range(c.str("Column Type"), l.str("Row Type"), f, t);
            let amount_type = if c.str("Amount Type") != "Net Amount" { c.str("Amount Type") } else { l.str("Amount Type") };
            let v = if c.str("Ledger Entry Type") == "Budget Entries" { Decimal::ZERO } else { amount(&entries, &row_accounts(ctx, l), a, b, amount_type) };
            // BC's CalcCellValue: a row's opposite sign applies before formulas use the row.
            let v = if l.get("Show Opposite Sign").as_bool() { -v } else { v };
            if let Some(cell) = values.get_mut(ri).and_then(|r| r.get_mut(ci)) {
                *cell = v;
            }
        }
    }
    // Row formulas, top to bottom (a formula may use rows above or below that are amounts).
    for (ri, l) in lines.iter().enumerate() {
        if l.str("Totaling Type") != "Formula" {
            continue;
        }
        for (ci, c) in cols.iter().enumerate() {
            if c.str("Column Type") == "Formula" {
                continue;
            }
            let snapshot = values.clone();
            let lookup = |a: &str, b: Option<&str>| -> Option<Decimal> {
                let ia = row_index(a)?;
                let ib = match b {
                    Some(b) => row_index(b)?,
                    None => ia,
                };
                let (lo, hi) = if ia <= ib { (ia, ib) } else { (ib, ia) };
                Some((lo..=hi).filter(|i| lines.get(*i).is_some_and(|x| x.str("Totaling Type") != "Formula" || *i < ri)).filter_map(|i| snapshot.get(i).and_then(|r| r.get(ci))).sum())
            };
            let v = eval(l.str("Totaling"), &lookup).map_err(|e| EngineError::refused(format!("Row {}: {}", l.str("Row No."), e.message)))?;
            let v = if l.get("Show Opposite Sign").as_bool() { -v } else { v };
            if let Some(cell) = values.get_mut(ri).and_then(|r| r.get_mut(ci)) {
                *cell = v;
            }
        }
    }
    // Column formulas, then the columns' opposite sign.
    for (ci, c) in cols.iter().enumerate() {
        if c.str("Column Type") != "Formula" {
            continue;
        }
        for ri in 0..lines.len() {
            let row = values.get(ri).cloned().unwrap_or_default();
            let lookup = |a: &str, b: Option<&str>| -> Option<Decimal> {
                let ia = col_index(a)?;
                let ib = match b {
                    Some(b) => col_index(b)?,
                    None => ia,
                };
                let (lo, hi) = if ia <= ib { (ia, ib) } else { (ib, ia) };
                Some((lo..=hi).filter_map(|i| row.get(i)).sum())
            };
            let v = eval(c.str("Formula"), &lookup).map_err(|e| EngineError::refused(format!("Column {}: {}", c.str("Column No."), e.message)))?;
            if let Some(cell) = values.get_mut(ri).and_then(|r| r.get_mut(ci)) {
                *cell = v;
            }
        }
    }
    for (ci, c) in cols.iter().enumerate() {
        if c.get("Show Opposite Sign").as_bool() {
            for r in values.iter_mut() {
                if let Some(v) = r.get_mut(ci) {
                    *v = -*v;
                }
            }
        }
    }
    // What is shown.
    let shown_cols: Vec<usize> = (0..cols.len()).filter(|i| cols.get(*i).is_some_and(|c| c.str("Show") != "Never")).collect();
    let mut columns = vec![json!("Row No."), json!("Description")];
    columns.extend(shown_cols.iter().filter_map(|i| cols.get(*i)).map(|c| json!(c.str("Column Header"))));
    let mut out = Vec::new();
    for (ri, l) in lines.iter().enumerate() {
        let r = values.get(ri).cloned().unwrap_or_default();
        let nonzero = shown_cols.iter().any(|i| r.get(*i).is_some_and(|v| !v.is_zero()));
        let first = shown_cols.first().and_then(|i| r.get(*i)).copied().unwrap_or_default();
        let show = match l.str("Show") {
            "No" => false,
            "If Any Column Not Zero" => nonzero,
            "When Positive Balance" => first > Decimal::ZERO,
            "When Negative Balance" => first < Decimal::ZERO,
            _ => true,
        };
        if !show {
            continue;
        }
        let indent = "    ".repeat(usize::try_from(l.get("Indentation").as_int().clamp(0, 10)).unwrap_or(0));
        let heading = l.str("Totaling").trim().is_empty() && l.str("Totaling Type") != "Formula";
        let mut row = vec![json!(l.str("Row No.")), json!(format!("{indent}{}", l.str("Description")))];
        for i in &shown_cols {
            let c = cols.get(*i);
            let v = r.get(*i).copied().unwrap_or_default();
            let hide = heading
                || match c.map(|c| c.str("Show")) {
                    Some("When Positive") => v <= Decimal::ZERO,
                    Some("When Negative") => v >= Decimal::ZERO,
                    _ => false,
                };
            row.push(json!(if hide { String::new() } else { round(v, c.map(|c| c.str("Rounding Factor")).unwrap_or("None")) }));
        }
        out.push(Json::Array(row));
    }
    Ok(json!({"report": {"title": format!("{title} ({} .. {})", from, to), "columns": columns, "rows": out}}))
}

/// `financial_report.overview`: the named financial report (or `rows` + `columns`) for
/// `from`..`to` (default: the work date's year up to the work date).
fn run_overview(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let name = opt_str(p, "name").map(str::to_string).or_else(|| match (opt_str(p, "table"), p.get("key")) {
        (Some(FINANCIAL_REPORT), Some(k)) => k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)),
        _ => None,
    });
    let (rows, columns, title) = match name {
        Some(n) => {
            let r = ctx.get_code(FINANCIAL_REPORT, &n)?;
            let rows = r.str("Financial Report Row Group").to_string();
            let cols = r.str("Financial Report Column Group").to_string();
            if rows.is_empty() {
                refuse!("Row Definition must have a value in Financial Report: Name={n}. It cannot be zero or empty.");
            }
            if cols.is_empty() {
                refuse!("Column Definition must have a value in Financial Report: Name={n}. It cannot be zero or empty.");
            }
            let title = if r.str("Description").is_empty() { n.clone() } else { r.str("Description").to_string() };
            (rows, cols, title)
        }
        None => {
            let rows = opt_str(p, "rows").ok_or_else(|| EngineError::invalid("missing parameter 'name' (or 'rows' and 'columns')"))?.to_string();
            let cols = opt_str(p, "columns").ok_or_else(|| EngineError::invalid("missing parameter 'columns'"))?.to_string();
            (rows.clone(), cols, rows)
        }
    };
    let to = parse_date(p, "to")?.unwrap_or(ctx.work_date);
    let from = parse_date(p, "from")?.unwrap_or(fiscal_year(to).0);
    if from > to {
        refuse!("The starting date {from} is after the ending date {to}.");
    }
    overview(ctx, &rows, &columns, from, to, &title)
}

fn s_overview() -> Json {
    json!({"type": "object", "properties": {
        "name": {"type": "string", "description": "the financial report"},
        "rows": {"type": "string", "description": "a row definition (instead of name)"},
        "columns": {"type": "string", "description": "a column definition (instead of name)"},
        "from": {"type": "string", "description": "YYYY-MM-DD; default the start of the year"},
        "to": {"type": "string", "description": "YYYY-MM-DD; default the work date"},
        "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "financial_report.overview",
    label: "View Financial Report",
    group: "Finance",
    summary: "A financial report (row and column definitions) evaluated on the G/L entries for a period.",
    mutates: false,
    params: s_overview,
    run: run_overview,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_add_subtract_and_range() {
        let v = |a: &str, b: Option<&str>| -> Option<Decimal> {
            match (a, b) {
                ("10", None) => Some(Decimal::from(100)),
                ("20", None) => Some(Decimal::from(30)),
                ("10", Some("20")) => Some(Decimal::from(130)),
                _ => None,
            }
        };
        assert_eq!(eval("10-20", &v).ok(), Some(Decimal::from(70)));
        assert_eq!(eval("-(10+20)*2", &v).ok(), Some(Decimal::from(-260)));
        assert_eq!(eval("10..20", &v).ok(), Some(Decimal::from(130)));
        assert_eq!(eval("20%10", &v).ok(), Some(Decimal::from(30)));
        assert!(eval("10+", &v).is_err());
        assert!(eval("(10", &v).is_err());
    }
}
