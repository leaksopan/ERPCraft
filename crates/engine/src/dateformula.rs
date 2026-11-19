//! BC date formulas (`30D`, `2W`, `1M`, `1Y`, `CM`, `1M+CM`, `-1D`), as used by Payment Terms.

use chrono::{Datelike, Days, Months, NaiveDate};

use crate::error::{EngineError, Result};

/// Applies `formula` to `date`. A blank formula returns `date`.
pub fn calc(formula: &str, date: NaiveDate) -> Result<NaiveDate> {
    let f = formula.trim().to_uppercase().replace(' ', "");
    if f.is_empty() {
        return Ok(date);
    }
    let bad = || EngineError::invalid(format!("The date formula '{formula}' is not valid. Use e.g. 30D, 2W, 1M, CM or 1M+CM."));
    let mut d = date;
    // Split into signed terms: "1M+CM" → ["+1M", "+CM"], "-1D" → ["-1D"].
    let mut terms: Vec<(bool, String)> = Vec::new();
    let mut neg = false;
    let mut cur = String::new();
    for c in f.chars() {
        if c == '+' || c == '-' {
            if !cur.is_empty() {
                terms.push((neg, std::mem::take(&mut cur)));
            }
            neg = c == '-';
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        terms.push((neg, cur));
    }
    if terms.len() > 8 {
        return Err(bad());
    }
    for (neg, term) in terms {
        d = match term.as_str() {
            // Current month / week / year: the last day of it.
            "CM" => end_of_month(d).ok_or_else(bad)?,
            "CW" => {
                let left = 6 - u64::from(d.weekday().num_days_from_monday());
                d.checked_add_days(Days::new(left)).ok_or_else(bad)?
            }
            "CY" => NaiveDate::from_ymd_opt(d.year(), 12, 31).ok_or_else(bad)?,
            _ => {
                let unit = term.chars().last().ok_or_else(bad)?;
                let n: u32 = term.get(..term.len() - 1).ok_or_else(bad)?.parse().map_err(|_| bad())?;
                if n > 10_000 {
                    return Err(bad());
                }
                match (unit, neg) {
                    ('D', false) => d.checked_add_days(Days::new(n.into())),
                    ('D', true) => d.checked_sub_days(Days::new(n.into())),
                    ('W', false) => d.checked_add_days(Days::new(u64::from(n) * 7)),
                    ('W', true) => d.checked_sub_days(Days::new(u64::from(n) * 7)),
                    ('M', false) => d.checked_add_months(Months::new(n)),
                    ('M', true) => d.checked_sub_months(Months::new(n)),
                    ('Y', false) => d.checked_add_months(Months::new(n.saturating_mul(12))),
                    ('Y', true) => d.checked_sub_months(Months::new(n.saturating_mul(12))),
                    _ => None,
                }
                .ok_or_else(bad)?
            }
        };
    }
    Ok(d)
}

fn end_of_month(d: NaiveDate) -> Option<NaiveDate> {
    let first_next = if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1)?
    };
    first_next.pred_opt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn formulas() {
        assert_eq!(calc("30D", d(2026, 1, 15)).unwrap(), d(2026, 2, 14));
        assert_eq!(calc("1M", d(2026, 1, 31)).unwrap(), d(2026, 2, 28));
        assert_eq!(calc("CM", d(2026, 2, 3)).unwrap(), d(2026, 2, 28));
        assert_eq!(calc("1M+CM", d(2026, 1, 15)).unwrap(), d(2026, 2, 28));
        assert_eq!(calc("-1D", d(2026, 3, 1)).unwrap(), d(2026, 2, 28));
        assert_eq!(calc("", d(2026, 3, 1)).unwrap(), d(2026, 3, 1));
        assert!(calc("abc", d(2026, 3, 1)).is_err());
    }
}
