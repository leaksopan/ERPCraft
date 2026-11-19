//! One typed field value.

use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::{Value as Json, json};

use crate::table::FieldType;

/// A field value. Code, Text and Option fields all hold [`Value::Text`]; a blank date (AL `0D`)
/// is [`Value::Null`].
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Dec(Decimal),
    Date(NaiveDate),
    Text(String),
}

impl Value {
    pub fn text(s: impl Into<String>) -> Self {
        Value::Text(s.into())
    }

    /// The empty value of a field type (AL's `Init` values).
    pub fn default_for(ty: &FieldType) -> Self {
        match ty {
            FieldType::Code(_) | FieldType::Text(_) => Value::Text(String::new()),
            FieldType::Option(opts) => Value::Text(opts.first().cloned().unwrap_or_default()),
            FieldType::Decimal => Value::Dec(Decimal::ZERO),
            FieldType::Integer => Value::Int(0),
            FieldType::Boolean => Value::Bool(false),
            FieldType::Date => Value::Null,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Value::Text(s) => s,
            _ => "",
        }
    }

    pub fn as_dec(&self) -> Decimal {
        match self {
            Value::Dec(d) => *d,
            Value::Int(i) => Decimal::from(*i),
            _ => Decimal::ZERO,
        }
    }

    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn as_date(&self) -> Option<NaiveDate> {
        match self {
            Value::Date(d) => Some(*d),
            _ => None,
        }
    }

    /// AL's "has no value": empty text, zero, false, blank date.
    pub fn is_blank(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Bool(b) => !b,
            Value::Int(i) => *i == 0,
            Value::Dec(d) => d.is_zero(),
            Value::Date(_) => false,
            Value::Text(s) => s.is_empty(),
        }
    }

    /// The JSON form used by the CLI, MCP and the data file. Decimals are JSON numbers.
    pub fn to_json(&self) -> Json {
        match self {
            Value::Null => Json::Null,
            Value::Bool(b) => json!(b),
            Value::Int(i) => json!(i),
            Value::Dec(d) => {
                let s = d.normalize().to_string();
                serde_json::from_str::<Json>(&s).unwrap_or(Json::String(s))
            }
            Value::Date(d) => json!(d.format("%Y-%m-%d").to_string()),
            Value::Text(s) => json!(s),
        }
    }

    /// Reads a value of type `ty` from JSON, checking it the way AL assignment would: Code is
    /// upper-cased and trimmed, lengths are capped, options must be one of the list.
    pub fn from_json(ty: &FieldType, v: &Json) -> Result<Value, String> {
        match ty {
            FieldType::Code(len) => {
                let s = json_text(v)?.trim().to_uppercase();
                check_len(&s, *len)?;
                Ok(Value::Text(s))
            }
            FieldType::Text(len) => {
                let s = json_text(v)?;
                check_len(&s, *len)?;
                Ok(Value::Text(s))
            }
            FieldType::Decimal => match v {
                Json::Null => Ok(Value::Dec(Decimal::ZERO)),
                Json::Number(n) => {
                    Decimal::from_str(&n.to_string()).map(Value::Dec).map_err(|_| format!("{n} is not a valid decimal"))
                }
                Json::String(s) => parse_decimal(s).map(Value::Dec),
                other => Err(format!("{other} is not a valid decimal")),
            },
            FieldType::Integer => match v {
                Json::Null => Ok(Value::Int(0)),
                Json::Number(n) => n.as_i64().map(Value::Int).ok_or_else(|| format!("{n} is not a valid integer")),
                Json::String(s) if s.trim().is_empty() => Ok(Value::Int(0)),
                Json::String(s) => {
                    s.trim().replace(',', "").parse::<i64>().map(Value::Int).map_err(|_| format!("'{s}' is not a valid integer"))
                }
                other => Err(format!("{other} is not a valid integer")),
            },
            FieldType::Boolean => match v {
                Json::Null => Ok(Value::Bool(false)),
                Json::Bool(b) => Ok(Value::Bool(*b)),
                Json::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "yes" | "1" => Ok(Value::Bool(true)),
                    "false" | "no" | "0" | "" => Ok(Value::Bool(false)),
                    _ => Err(format!("'{s}' is not a valid boolean")),
                },
                other => Err(format!("{other} is not a valid boolean")),
            },
            FieldType::Date => match v {
                Json::Null => Ok(Value::Null),
                Json::String(s) if s.trim().is_empty() => Ok(Value::Null),
                Json::String(s) => parse_date(s).map(Value::Date),
                other => Err(format!("{other} is not a valid date (use YYYY-MM-DD)")),
            },
            FieldType::Option(opts) => {
                let s = match v {
                    Json::Null => String::new(),
                    Json::Number(n) => {
                        let i = n.as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(usize::MAX);
                        return opts.get(i).cloned().map(Value::Text).ok_or_else(|| format!("{n} is not a valid option"));
                    }
                    other => json_text(other)?,
                };
                opts.iter()
                    .find(|o| o.eq_ignore_ascii_case(s.trim()))
                    .cloned()
                    .map(Value::Text)
                    .ok_or_else(|| format!("'{s}' is not a valid option. Valid options are: {}", option_list(opts)))
            }
        }
    }

    /// The text a user sees for this value in a field of type `ty`.
    pub fn display(&self, ty: &FieldType) -> String {
        match (self, ty) {
            (Value::Dec(d), _) => format_decimal(*d, 2),
            (Value::Int(i), FieldType::Integer) => i.to_string(),
            (Value::Bool(b), _) => if *b { "Yes" } else { "No" }.to_string(),
            (Value::Date(d), _) => d.format("%d/%m/%Y").to_string(),
            (Value::Null, _) => String::new(),
            (v, _) => v.to_string(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => Ok(()),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Dec(d) => write!(f, "{}", d.normalize()),
            Value::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
            Value::Text(s) => f.write_str(s),
        }
    }
}

fn json_text(v: &Json) -> Result<String, String> {
    match v {
        Json::Null => Ok(String::new()),
        Json::String(s) => Ok(s.clone()),
        Json::Number(n) => Ok(n.to_string()),
        Json::Bool(b) => Ok(b.to_string()),
        other => Err(format!("{other} is not a valid text value")),
    }
}

fn check_len(s: &str, len: u16) -> Result<(), String> {
    let n = s.chars().count();
    if n > usize::from(len) {
        return Err(format!("The length of the string is {n}, but it must be less than or equal to {len} characters."));
    }
    Ok(())
}

fn option_list(opts: &[String]) -> String {
    opts.iter().map(|o| if o.is_empty() { "' '".to_string() } else { o.clone() }).collect::<Vec<_>>().join(", ")
}

fn parse_decimal(s: &str) -> Result<Decimal, String> {
    let t = s.trim().replace(',', "");
    if t.is_empty() {
        return Ok(Decimal::ZERO);
    }
    Decimal::from_str(&t).map_err(|_| format!("'{s}' is not a valid decimal"))
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    let t = s.trim();
    for fmt in ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y", "%d.%m.%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, fmt) {
            return Ok(d);
        }
    }
    Err(format!("'{s}' is not a valid date"))
}

/// `1234567.5` → `1,234,567.50`: grouped thousands, at least `min_scale` decimals.
pub fn format_decimal(d: Decimal, min_scale: u32) -> String {
    let d = d.round_dp_with_strategy(5, RoundingStrategy::MidpointAwayFromZero).normalize();
    let mut s = d.abs().to_string();
    let (int, frac) = match s.find('.') {
        Some(i) => {
            let frac = s.split_off(i + 1);
            s.pop();
            (s, frac)
        }
        None => (s, String::new()),
    };
    let mut frac = frac;
    while frac.len() < min_scale as usize {
        frac.push('0');
    }
    let mut grouped = String::new();
    let digits: Vec<char> = int.chars().collect();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    let sign = if d.is_sign_negative() && !d.is_zero() { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_formatting() {
        assert_eq!(format_decimal(Decimal::new(12345675, 1), 2), "1,234,567.50");
        assert_eq!(format_decimal(Decimal::new(-5, 0), 2), "-5.00");
        assert_eq!(format_decimal(Decimal::ZERO, 2), "0.00");
        assert_eq!(format_decimal(Decimal::new(123456, 5), 2), "1.23456");
    }

    #[test]
    fn code_is_upper_cased_and_capped() {
        assert_eq!(Value::from_json(&FieldType::Code(10), &json!(" c001 ")).unwrap(), Value::text("C001"));
        assert!(Value::from_json(&FieldType::Code(3), &json!("ABCD")).is_err());
    }

    #[test]
    fn option_matches_case_insensitively() {
        let ty = FieldType::Option(vec!["Open".into(), "Released".into()]);
        assert_eq!(Value::from_json(&ty, &json!("released")).unwrap(), Value::text("Released"));
        assert_eq!(Value::from_json(&ty, &json!(1)).unwrap(), Value::text("Released"));
        assert!(Value::from_json(&ty, &json!("Closed")).is_err());
    }

    #[test]
    fn decimal_json_roundtrip() {
        let v = Value::from_json(&FieldType::Decimal, &json!(1234.56)).unwrap();
        assert_eq!(v.to_json(), json!(1234.56));
        assert_eq!(Value::from_json(&FieldType::Decimal, &json!("1,000.5")).unwrap(), Value::Dec(Decimal::new(10005, 1)));
    }
}
