//! Text output: aligned tables, "Field ..... Value" cards, number formatting.

use std::fmt::Write as _;

use erpcraft_engine::meta::{FieldType, TableDef};
use serde_json::Value;

/// Widest a table cell gets before it is cut.
const MAX_CELL: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// An aligned text table with a header row and a rule under it.
pub fn table(out: &mut String, headers: &[String], aligns: &[Align], rows: &[Vec<String>]) {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate().take(cols) {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(width(c).min(MAX_CELL));
            }
        }
    }
    let line = |out: &mut String, cells: &[String]| {
        let mut s = String::new();
        for (i, w) in widths.iter().enumerate() {
            let cell = cut(cells.get(i).map_or("", String::as_str));
            let pad = w.saturating_sub(width(&cell));
            if i > 0 {
                s.push_str("  ");
            }
            if aligns.get(i) == Some(&Align::Right) {
                s.push_str(&" ".repeat(pad));
                s.push_str(&cell);
            } else {
                s.push_str(&cell);
                s.push_str(&" ".repeat(pad));
            }
        }
        let _ = writeln!(out, "{}", s.trim_end());
    };
    line(out, headers);
    let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    line(out, &rule);
    for r in rows {
        line(out, r);
    }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

fn cut(s: &str) -> String {
    let s = s.replace(['\n', '\r', '\t'], " ");
    if width(&s) <= MAX_CELL {
        return s;
    }
    let mut t: String = s.chars().take(MAX_CELL.saturating_sub(3)).collect();
    t.push_str("...");
    t
}

/// `Field ........ Value` lines.
pub fn card(out: &mut String, pairs: &[(String, String)]) {
    let w = pairs.iter().map(|(k, _)| width(k)).max().unwrap_or(0) + 2;
    for (k, v) in pairs {
        let dots = ".".repeat(w.saturating_sub(width(k)).max(2));
        let _ = writeln!(out, "{k} {dots} {v}");
    }
}

/// Groups the integer part of a number in thousands; `min_scale` pads decimals (2 for amounts).
pub fn number(raw: &str, min_scale: usize) -> String {
    let raw = raw.trim();
    if raw.contains(['e', 'E']) {
        return raw.to_string();
    }
    let (sign, digits) = match raw.strip_prefix('-') {
        Some(d) => ("-", d),
        None => ("", raw),
    };
    let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    let mut grouped = String::new();
    let n = int.len();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let mut frac = frac.to_string();
    while frac.len() < min_scale {
        frac.push('0');
    }
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

/// Whether text is a plain number (`-12`, `1554000.5`).
pub fn is_number(s: &str) -> bool {
    let s = s.trim();
    let d = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = d.split_once('.').unwrap_or((d, ""));
    !int.is_empty() && int.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
}

/// A JSON value as text, numbers formatted for a field type when one is known.
pub fn value(v: &Value, ty: Option<&FieldType>) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Number(n) => {
            let scale = if matches!(ty, Some(FieldType::Decimal)) { 2 } else { 0 };
            number(&n.to_string(), scale)
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// AL "has no value": null, empty text, zero, false.
pub fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// Records as a table over `columns` of `t`.
pub fn records(out: &mut String, t: Option<&TableDef>, columns: &[String], records: &[Value]) {
    let ty = |c: &str| t.and_then(|t| t.field(c)).map(|f| &f.ty);
    let aligns: Vec<Align> = columns
        .iter()
        .map(|c| match ty(c) {
            Some(ft) if ft.is_numeric() => Align::Right,
            Some(_) => Align::Left,
            None => {
                if records.iter().any(|r| r.get(c).is_some_and(Value::is_number)) {
                    Align::Right
                } else {
                    Align::Left
                }
            }
        })
        .collect();
    let rows: Vec<Vec<String>> = records.iter().map(|r| columns.iter().map(|c| value(r.get(c).unwrap_or(&Value::Null), ty(c))).collect()).collect();
    table(out, columns, &aligns, &rows);
}

/// A command's `report` ({title, columns, rows}) as a table. Columns after the first whose cells are
/// all numbers are right-aligned with thousands separators.
pub fn report(out: &mut String, report: &Value) {
    if let Some(title) = report.get("title").and_then(Value::as_str) {
        let _ = writeln!(out, "{title}\n");
    }
    let headers: Vec<String> = report.get("columns").and_then(Value::as_array).map(|a| a.iter().map(|c| value(c, None)).collect()).unwrap_or_default();
    let raw_rows: Vec<Vec<String>> = report
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| match r {
                    Value::Array(cells) => cells.iter().map(|c| value(c, None)).collect(),
                    other => vec![value(other, None)],
                })
                .collect()
        })
        .unwrap_or_default();
    let cols = headers.len().max(raw_rows.iter().map(Vec::len).max().unwrap_or(0));
    let mut headers = headers;
    headers.resize(cols, String::new());
    let numeric: Vec<bool> = (0..cols)
        .map(|i| {
            let cells: Vec<&str> = raw_rows.iter().filter_map(|r| r.get(i)).map(String::as_str).filter(|c| !c.is_empty()).collect();
            i > 0 && !cells.is_empty() && cells.iter().all(|c| is_number(c) || c.contains(',') && is_number(&c.replace(',', "")))
        })
        .collect();
    let rows: Vec<Vec<String>> = raw_rows
        .into_iter()
        .map(|r| r.into_iter().enumerate().map(|(i, c)| if numeric.get(i) == Some(&true) && is_number(&c) { number(&c, 0) } else { c }).collect())
        .collect();
    let aligns: Vec<Align> = numeric.iter().map(|n| if *n { Align::Right } else { Align::Left }).collect();
    table(out, &headers, &aligns, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_group_thousands() {
        assert_eq!(number("1554000", 2), "1,554,000.00");
        assert_eq!(number("-1234.5", 2), "-1,234.50");
        assert_eq!(number("999", 0), "999");
        assert_eq!(number("abc", 2), "abc");
        assert_eq!(number("1e21", 2), "1e21");
    }

    #[test]
    fn tables_align() {
        let mut s = String::new();
        table(&mut s, &["No.".into(), "Amount".into()], &[Align::Left, Align::Right], &[vec!["A".into(), "1,000.00".into()], vec!["BB".into(), "5.00".into()]]);
        assert_eq!(s, "No.    Amount\n---  --------\nA    1,000.00\nBB       5.00\n");
    }
}
