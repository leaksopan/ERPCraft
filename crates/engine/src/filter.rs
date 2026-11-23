//! Field filter expressions on one field: `10000`, `<>Open`, `10000|20000`, `''`,
//! `*bike*`, `>100`, `<=2026-01-31`, `10000..20000`.

use erpcraft_meta::{FieldType, Record, TableDef, Value};
use serde_json::Value as Json;

/// Whether `v` (of type `ty`) matches the filter expression `expr`.
pub fn matches(ty: &FieldType, v: &Value, expr: &str) -> bool {
    expr.split('|').any(|alt| matches_one(ty, v, alt.trim()))
}

fn parse(ty: &FieldType, s: &str) -> Option<Value> {
    let s = s.trim().trim_matches('\'');
    match ty {
        // Compare text case-insensitively, as BC does for Code and (with @) Text.
        FieldType::Code(_) | FieldType::Text(_) | FieldType::Option(_) => Some(Value::text(s.to_uppercase())),
        _ => Value::from_json(ty, &Json::String(s.to_string())).ok(),
    }
}

fn norm(ty: &FieldType, v: &Value) -> Value {
    match ty {
        FieldType::Code(_) | FieldType::Text(_) | FieldType::Option(_) => Value::text(v.as_str().to_uppercase()),
        FieldType::Decimal => Value::Dec(v.as_dec()),
        _ => v.clone(),
    }
}

fn matches_one(ty: &FieldType, v: &Value, expr: &str) -> bool {
    if expr == "''" {
        return v.is_blank() || matches!(v, Value::Null);
    }
    let v = norm(ty, v);
    if let Some(rest) = expr.strip_prefix("<>") {
        if rest == "''" {
            return !v.is_blank();
        }
        return parse(ty, rest).is_none_or(|r| v != r);
    }
    for (op, f) in [
        (">=", (|o| o != std::cmp::Ordering::Less) as fn(std::cmp::Ordering) -> bool),
        ("<=", |o| o != std::cmp::Ordering::Greater),
        (">", |o| o == std::cmp::Ordering::Greater),
        ("<", |o| o == std::cmp::Ordering::Less),
    ] {
        if let Some(rest) = expr.strip_prefix(op) {
            return parse(ty, rest).is_some_and(|r| f(v.cmp(&r)));
        }
    }
    if let Some((lo, hi)) = expr.split_once("..") {
        let lo_ok = lo.trim().is_empty() || parse(ty, lo).is_some_and(|l| v >= l);
        let hi_ok = hi.trim().is_empty() || parse(ty, hi).is_some_and(|h| v <= h);
        return lo_ok && hi_ok;
    }
    if expr.contains('*') {
        return wildcard(&expr.to_uppercase(), &v.to_string().to_uppercase());
    }
    parse(ty, expr).is_some_and(|r| v == r)
}

/// `*` matches any run of characters.
fn wildcard(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(rest) = text.get(pos..) else { return false };
        match rest.find(part) {
            Some(found) => {
                if i == 0 && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    parts.last().is_none_or(|last| last.is_empty() || text.ends_with(last))
}

/// Whether `r` matches every (field, expression) filter.
pub fn record_matches(t: &TableDef, r: &Record, filters: &[(String, String)]) -> bool {
    filters.iter().all(|(field, expr)| match t.field(field) {
        Some(f) => matches(&f.ty, r.get(&f.name), expr),
        None => false,
    })
}

/// Whether any searchable field (Code / Text) of `r` contains `q` (the list page's search box).
pub fn record_search(t: &TableDef, r: &Record, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    t.fields.iter().any(|f| {
        matches!(f.ty, FieldType::Code(_) | FieldType::Text(_)) && r.get(&f.name).as_str().to_lowercase().contains(&q)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn filter_expressions() {
        let code = FieldType::Code(20);
        assert!(matches(&code, &Value::text("C10000"), "c10000"));
        assert!(matches(&code, &Value::text("C10000"), "C2|C10000"));
        assert!(matches(&code, &Value::text("C10000"), "<>C2"));
        assert!(matches(&code, &Value::text("C10000"), "C1*"));
        assert!(!matches(&code, &Value::text("C10000"), "*9*"));
        assert!(matches(&code, &Value::text(""), "''"));
        assert!(matches(&code, &Value::text("20000"), "10000..30000"));
        let dec = FieldType::Decimal;
        assert!(matches(&dec, &Value::Dec(Decimal::new(150, 0)), ">100"));
        assert!(!matches(&dec, &Value::Dec(Decimal::new(50, 0)), ">=100"));
    }
}
