//! Recurring general journals (BC's Recurring General Journal, Gen. Jnl.-Post Batch for recurring
//! templates, Gen. Jnl. Allocation): each line posts with its recurring method, its allocations
//! balance it, and afterwards it moves on by its recurring frequency instead of being deleted.

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::ledger::Txn;
use crate::posting::{GlDoc, GlLine, post_gl};
use crate::refuse;
use crate::schema::finance::{GEN_JNL_LINE, GEN_JNL_TEMPLATE};
use crate::schema::tables::{GL_ACCOUNT, GL_ENTRY};

pub const ALLOCATION: &str = "Gen. Jnl. Allocation";

/// Whether `template` is a recurring journal template.
pub fn is_recurring(ctx: &Ctx, template: &str) -> bool {
    ctx.db.get_code(GEN_JNL_TEMPLATE, template).is_some_and(|t| t.get("Recurring").as_bool())
}

fn balance(ctx: &Ctx, account: &str, to: NaiveDate) -> Decimal {
    ctx.db
        .rows(GL_ENTRY)
        .filter(|e| e.str("G/L Account No.") == account && e.get("Posting Date").as_date().is_some_and(|d| d <= to))
        .map(|e| e.get("Amount").as_dec())
        .sum()
}

fn allocations(ctx: &Ctx, l: &Record) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(ALLOCATION)
        .filter(|a| {
            a.str("Journal Template Name") == l.str("Journal Template Name")
                && a.str("Journal Batch Name") == l.str("Journal Batch Name")
                && a.get("Journal Line No.").as_int() == l.get("Line No.").as_int()
        })
        .cloned()
        .collect();
    v.sort_by_key(|a| a.get("Line No.").as_int());
    v
}

/// The G/L lines one recurring line posts (the line's account, then its balancing account or
/// its allocations, the last allocation taking the rounding).
fn effects(ctx: &Ctx, l: &Record, amount: Decimal) -> Result<Vec<(String, Decimal)>> {
    let gl_only = |ty: &str, no: &str| -> Result<String> {
        if !matches!(ty, "" | "G/L Account") {
            refuse!("Recurring lines post to G/L accounts; {ty} {no} is not one.");
        }
        Ok(no.to_string())
    };
    let mut out = vec![(gl_only(l.str("Account Type"), l.str("Account No."))?, amount)];
    let allocs = allocations(ctx, l);
    if !allocs.is_empty() {
        let mut left = -amount;
        for (i, a) in allocs.iter().enumerate() {
            let share = if i + 1 == allocs.len() { left } else { round_amount(-amount * a.get("Allocation %").as_dec() / Decimal::ONE_HUNDRED) };
            left -= share;
            out.push((a.str("Account No.").to_string(), share));
        }
    } else if !l.str("Bal. Account No.").is_empty() {
        out.push((gl_only(l.str("Bal. Account Type"), l.str("Bal. Account No."))?, -amount));
    }
    let sum: Decimal = out.iter().map(|(_, a)| *a).sum();
    if !sum.is_zero() {
        refuse!(
            "Recurring line {} ({}) does not balance by {sum}: give it a balancing account or allocations adding up to 100%.",
            l.get("Line No.").as_int(),
            l.str("Description")
        );
    }
    Ok(out)
}

/// Posts a recurring batch: each line due by its posting date (and not expired) posts; reversing
/// methods are reversed the next day; then every posted line moves on by its frequency, its
/// document number counts up, and variable amounts are cleared.
pub fn post(ctx: &mut Ctx, template: &str, batch: &str) -> Result<Json> {
    let lt = ctx.table(GEN_JNL_LINE)?;
    let mut lines: Vec<Record> = ctx
        .db
        .rows(GEN_JNL_LINE)
        .filter(|l| l.str("Journal Template Name") == template && l.str("Journal Batch Name") == batch && !l.str("Account No.").is_empty())
        .cloned()
        .collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    if lines.is_empty() {
        refuse!("There is nothing to post.");
    }
    let source = ctx.get_code(GEN_JNL_TEMPLATE, template)?.str("Source Code").to_string();
    let mut posted = 0;
    for mut l in lines {
        let method = l.str("Recurring Method").to_string();
        if method.is_empty() {
            refuse!("Recurring Method must have a value in Gen. Journal Line: Journal Template Name={template}, Journal Batch Name={batch}, Line No.={}.", l.get("Line No.").as_int());
        }
        let frequency = l.str("Recurring Frequency").to_string();
        if frequency.is_empty() {
            refuse!("Recurring Frequency must have a value in Gen. Journal Line: Journal Template Name={template}, Journal Batch Name={batch}, Line No.={}.", l.get("Line No.").as_int());
        }
        let date = l.get("Posting Date").as_date().ok_or_else(|| EngineError::refused("Posting Date must have a value."))?;
        if l.get("Expiration Date").as_date().is_some_and(|x| x < date) {
            continue;
        }
        crate::posting::check_posting_date(ctx, date)?;
        let code = method.split_whitespace().next().unwrap_or("").to_string();
        let amount = match code.as_str() {
            // Balance: whatever the account holds is moved away.
            "B" | "RB" => -balance(ctx, l.str("Account No."), date),
            _ => l.get("Amount").as_dec(),
        };
        if !amount.is_zero() {
            ctx.get_code(GL_ACCOUNT, l.str("Account No."))?;
            let lines = effects(ctx, &l, amount)?;
            let doc_no = l.str("Document No.").to_string();
            let desc = l.str("Description").to_string();
            let mut txn = Txn::new(ctx, &source, template, batch);
            txn.dim_set = l.get("Dimension Set ID").as_int();
            let gl = |sign: Decimal| -> Vec<GlLine> {
                lines.iter().map(|(a, x)| GlLine { account: a.clone(), amount: *x * sign, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" }).collect()
            };
            post_gl(ctx, &GlDoc { posting_date: date, doc_type: l.str("Document Type"), doc_no: &doc_no, ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl(Decimal::ONE))?;
            if code.starts_with('R') {
                // Reversing: undone the day after.
                let next = date.succ_opt().unwrap_or(date);
                let rtxn = Txn::new(ctx, &source, template, batch);
                post_gl(ctx, &GlDoc { posting_date: next, doc_type: l.str("Document Type"), doc_no: &doc_no, ext_doc_no: "", source_type: "", source_no: "", txn: &rtxn }, gl(Decimal::NEGATIVE_ONE))?;
            }
            posted += 1;
        }
        // The line moves on.
        l.set("Posting Date", Value::Date(crate::dateformula::calc(&frequency, date)?));
        if let Some(next) = crate::noseries::increment(l.str("Document No."), 1) {
            l.set("Document No.", Value::text(next));
        }
        if matches!(code.as_str(), "V" | "RV" | "B" | "RB") {
            l.set("Amount", dec(Decimal::ZERO));
            l.set("Debit Amount", dec(Decimal::ZERO));
            l.set("Credit Amount", dec(Decimal::ZERO));
        }
        ctx.db.modify(lt, l)?;
    }
    Ok(json!({"lines": posted, "message": format!("{posted} recurring line(s) were posted.")}))
}

/// New allocation lines take the next line number.
pub fn on_allocation_insert(ctx: &Ctx, r: &mut Record) {
    if r.get("Line No.").as_int() == 0 {
        let last = ctx
            .db
            .rows(ALLOCATION)
            .filter(|a| a.str("Journal Template Name") == r.str("Journal Template Name") && a.str("Journal Batch Name") == r.str("Journal Batch Name") && a.get("Journal Line No.") == r.get("Journal Line No."))
            .map(|a| a.get("Line No.").as_int())
            .max()
            .unwrap_or(0);
        r.set("Line No.", Value::Int(last + 10000));
    }
    if let Some(a) = ctx.db.get(GL_ACCOUNT, &Key(vec![r.get("Account No.").clone()])) {
        r.set("Account Name", a.get("Name").clone());
    }
}
