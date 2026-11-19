//! Number series (AL codeunit "No. Series"): the next document / master-data number.

use erpcraft_meta::Value;

use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::schema::tables::NO_SERIES;

/// The series code a setup table names in `field` (e.g. Sales & Receivables Setup › Order Nos.).
pub fn series_from_setup(ctx: &Ctx, setup_table: &str, field: &str) -> Result<String> {
    let s = ctx.setup(setup_table)?;
    let code = s.str(field).to_string();
    if code.is_empty() {
        return Err(EngineError::refused(format!(
            "You must specify a number series in the {field} field on the {setup_table} page."
        )));
    }
    Ok(code)
}

/// Takes the next number of `series` and records it as used.
pub fn next(ctx: &mut Ctx, series: &str) -> Result<String> {
    let mut r = ctx
        .get_code(NO_SERIES, series)
        .map_err(|_| EngineError::refused(format!("The No. Series {series} does not exist.")))?;
    let last = r.str("Last No. Used").to_string();
    let by = u128::try_from(r.get("Increment-by No.").as_int().max(1)).unwrap_or(1);
    let next = if last.is_empty() {
        let start = r.str("Starting No.").to_string();
        if start.is_empty() {
            return Err(EngineError::refused(format!("The number series {series} has no Starting No.")));
        }
        start
    } else {
        increment(&last, by).ok_or_else(|| {
            EngineError::refused(format!("The number {last} in No. Series {series} cannot be extended to more than 20 characters."))
        })?
    };
    let ending = r.str("Ending No.");
    if !ending.is_empty() && next.len() == ending.len() && next.as_str() > ending {
        return Err(EngineError::refused(format!("You have used all the numbers in No. Series {series}.")));
    }
    r.set("Last No. Used", Value::text(next.clone()));
    ctx.put(NO_SERIES, r)?;
    Ok(next)
}

/// `S-ORD1009` + 1 → `S-ORD1010`; keeps the width of the number part (zero padded).
pub fn increment(s: &str, by: u128) -> Option<String> {
    let digits_start = s.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit()).last().map(|(i, _)| i)?;
    let (prefix, digits) = s.split_at(digits_start);
    let n: u128 = digits.parse().ok()?;
    let next = n.checked_add(by)?;
    let out = format!("{prefix}{next:0width$}", width = digits.len());
    (out.len() <= 20).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::increment;

    #[test]
    fn increments_keep_width() {
        assert_eq!(increment("S-ORD1009", 1).unwrap(), "S-ORD1010");
        assert_eq!(increment("C00099", 1).unwrap(), "C00100");
        assert_eq!(increment("A9", 1).unwrap(), "A10");
        assert_eq!(increment("10000", 10000).unwrap(), "20000");
        assert!(increment("ABC", 1).is_none());
    }
}
