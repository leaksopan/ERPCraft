//! Bank account reconciliation (BC's Bank Acc. Reconciliation, Suggest Bank Acc. Recon. Lines,
//! Match Bank Rec. Lines, Bank Acc. Reconciliation Post): statement lines are matched to open
//! bank account ledger entries; posting closes them and files the statement.

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::bank_rec::{BANK_REC, BANK_REC_LINE, BANK_STMT, BANK_STMT_LINE};
use crate::schema::tables::{BANK_ACCOUNT, BANK_LEDGER};

/// The reconciliation an action runs on: `key` [bank, statement] or `bank_account_no` +
/// `statement_no`.
fn rec_of(ctx: &Ctx, p: &Json) -> Result<Record> {
    let (bank, stmt) = match (opt_str(p, "table"), p.get("key").and_then(Json::as_array)) {
        (Some(BANK_REC), Some(k)) => (k.first().and_then(Json::as_str).unwrap_or("").to_string(), k.get(1).and_then(Json::as_str).unwrap_or("").to_string()),
        _ => (
            opt_str(p, "bank_account_no").ok_or_else(|| EngineError::invalid("missing parameter 'bank_account_no'"))?.to_string(),
            opt_str(p, "statement_no").ok_or_else(|| EngineError::invalid("missing parameter 'statement_no'"))?.to_string(),
        ),
    };
    ctx.get(BANK_REC, &Key(vec![Value::text(&bank), Value::text(&stmt)]))
}

fn lines_of(ctx: &Ctx, rec: &Record) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(BANK_REC_LINE)
        .filter(|l| l.str("Bank Account No.") == rec.str("Bank Account No.") && l.str("Statement No.") == rec.str("Statement No."))
        .cloned()
        .collect();
    v.sort_by_key(|l| l.get("Statement Line No.").as_int());
    v
}

/// Bank entries applied to a line.
fn applied_to(ctx: &Ctx, line: &Record) -> Vec<Record> {
    ctx.db
        .rows(BANK_LEDGER)
        .filter(|e| {
            e.str("Bank Account No.") == line.str("Bank Account No.")
                && e.str("Statement No.") == line.str("Statement No.")
                && e.get("Statement Line No.").as_int() == line.get("Statement Line No.").as_int()
                && e.str("Statement Status") == "Bank Acc. Entry Applied"
        })
        .cloned()
        .collect()
}

/// Recomputes a line's Applied Amount, Applied Entries and Difference.
fn refresh_line(ctx: &mut Ctx, mut line: Record) -> Result<()> {
    let entries = applied_to(ctx, &line);
    let applied: Decimal = entries.iter().map(|e| e.get("Remaining Amount").as_dec()).sum();
    line.set("Applied Amount", dec(applied));
    line.set("Applied Entries", Value::Int(entries.len() as i64));
    line.set("Difference", dec(line.get("Statement Amount").as_dec() - applied));
    let lt = ctx.table(BANK_REC_LINE)?;
    ctx.db.modify(lt, line)
}

fn set_applied(ctx: &mut Ctx, entry: &Record, line: Option<&Record>) -> Result<()> {
    let t = ctx.table(BANK_LEDGER)?;
    let mut e = entry.clone();
    match line {
        Some(l) => {
            e.set("Statement Status", Value::text("Bank Acc. Entry Applied"));
            e.set("Statement No.", l.get("Statement No.").clone());
            e.set("Statement Line No.", l.get("Statement Line No.").clone());
        }
        None => {
            e.set("Statement Status", Value::text("Open"));
            e.set("Statement No.", Value::text(""));
            e.set("Statement Line No.", Value::Int(0));
        }
    }
    ctx.db.modify(t, e)
}

fn open_unapplied(ctx: &Ctx, bank: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(BANK_LEDGER)
        .filter(|e| e.str("Bank Account No.") == bank && e.get("Open").as_bool() && matches!(e.str("Statement Status"), "" | "Open"))
        .cloned()
        .collect();
    v.sort_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
    v
}

/// BC's Suggest Lines: a statement line for every open, unapplied bank entry up to the statement
/// date, applied to it.
fn suggest_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let rec = rec_of(ctx, p)?;
    let bank = rec.str("Bank Account No.").to_string();
    let until = rec.get("Statement Date").as_date().unwrap_or(ctx.work_date);
    let lt = ctx.table(BANK_REC_LINE)?;
    let mut next = lines_of(ctx, &rec).iter().map(|l| l.get("Statement Line No.").as_int()).max().unwrap_or(0);
    let mut added = 0;
    for e in open_unapplied(ctx, &bank).into_iter().filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d <= until)) {
        next += 10000;
        let mut l = lt.init_record();
        l.set("Bank Account No.", Value::text(&bank));
        l.set("Statement No.", rec.get("Statement No.").clone());
        l.set("Statement Line No.", Value::Int(next));
        l.set("Document No.", e.get("Document No.").clone());
        l.set("Transaction Date", e.get("Posting Date").clone());
        l.set("Value Date", e.get("Posting Date").clone());
        l.set("Description", e.get("Description").clone());
        l.set("Statement Amount", e.get("Remaining Amount").clone());
        ctx.db.insert(lt, l.clone())?;
        set_applied(ctx, &e, Some(&l))?;
        refresh_line(ctx, l)?;
        added += 1;
    }
    Ok(json!({"lines": added, "message": format!("{added} line(s) were suggested.")}))
}

/// BC's Match Automatically: each unmatched line takes an open bank entry with the same amount,
/// the same document number first, then the closest date.
fn match_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let rec = rec_of(ctx, p)?;
    let bank = rec.str("Bank Account No.").to_string();
    let mut matched = 0;
    for l in lines_of(ctx, &rec).into_iter().filter(|l| l.get("Applied Entries").as_int() == 0) {
        let amount = l.get("Statement Amount").as_dec();
        let date = l.get("Transaction Date").as_date();
        let mut candidates: Vec<Record> = open_unapplied(ctx, &bank).into_iter().filter(|e| e.get("Remaining Amount").as_dec() == amount).collect();
        candidates.sort_by_key(|e| {
            let same_doc = !l.str("Document No.").is_empty() && e.str("Document No.") == l.str("Document No.");
            let days = match (date, e.get("Posting Date").as_date()) {
                (Some(a), Some(b)) => (a - b).num_days().abs(),
                _ => i64::MAX,
            };
            (!same_doc, days)
        });
        if let Some(e) = candidates.first() {
            set_applied(ctx, e, Some(&l))?;
            refresh_line(ctx, l)?;
            matched += 1;
        }
    }
    Ok(json!({"matched": matched, "message": format!("{matched} line(s) were matched.")}))
}

fn remove_match(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let rec = rec_of(ctx, p)?;
    let mut n = 0;
    for l in lines_of(ctx, &rec) {
        for e in applied_to(ctx, &l) {
            set_applied(ctx, &e, None)?;
            n += 1;
        }
        refresh_line(ctx, l)?;
    }
    Ok(json!({"message": format!("{n} match(es) were removed.")}))
}

/// BC's Bank Acc. Reconciliation Post: the statement must add up and every line be applied; the
/// applied entries close, the bank account remembers the statement, and it is filed.
fn post(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let rec = rec_of(ctx, p)?;
    let bank_no = rec.str("Bank Account No.").to_string();
    let stmt_no = rec.str("Statement No.").to_string();
    let lines = lines_of(ctx, &rec);
    if lines.is_empty() {
        refuse!("There is nothing to post.");
    }
    let total: Decimal = lines.iter().map(|l| l.get("Statement Amount").as_dec()).sum();
    let ending = rec.get("Statement Ending Balance").as_dec();
    let last = rec.get("Balance Last Statement").as_dec();
    if ending != last + total {
        refuse!("Statement Ending Balance must be equal to Total Balance. ({ending} should be {})", last + total);
    }
    for l in &lines {
        let applied: Decimal = applied_to(ctx, l).iter().map(|e| e.get("Remaining Amount").as_dec()).sum();
        let difference = l.get("Statement Amount").as_dec() - applied;
        if !difference.is_zero() {
            refuse!("The total difference is {difference}. It must be 0. (line {}, {})", l.get("Statement Line No.").as_int(), l.str("Description"));
        }
    }
    let bt = ctx.table(BANK_LEDGER)?;
    let lt = ctx.table(BANK_REC_LINE)?;
    let st = ctx.table(BANK_STMT)?;
    let slt = ctx.table(BANK_STMT_LINE)?;
    for l in &lines {
        for mut e in applied_to(ctx, l) {
            e.set("Statement Status", Value::text("Closed"));
            e.set("Open", Value::Bool(false));
            e.set("Remaining Amount", dec(Decimal::ZERO));
            ctx.db.modify(bt, e)?;
        }
        let mut sl = slt.init_record();
        for f in &slt.fields {
            sl.set(&f.name, l.get(&f.name).clone());
        }
        ctx.db.insert(slt, sl)?;
        ctx.db.remove(&lt.name, &l.key(lt));
    }
    let mut bank = ctx.get_code(BANK_ACCOUNT, &bank_no)?;
    let name = bank.str("Name").to_string();
    bank.set("Last Statement No.", Value::text(&stmt_no));
    bank.set("Balance Last Statement", dec(ending));
    let bat = ctx.table(BANK_ACCOUNT)?;
    ctx.db.modify(bat, bank)?;
    let mut s = st.init_record();
    for f in ["Bank Account No.", "Statement No.", "Statement Ending Balance", "Statement Date", "Balance Last Statement"] {
        s.set(f, rec.get(f).clone());
    }
    s.set("Bank Account Name", Value::text(name));
    s.set("G/L Balance at Posting Date", dec(ctx.db.rows(BANK_LEDGER).filter(|e| e.str("Bank Account No.") == bank_no).map(|e| e.get("Amount").as_dec()).sum()));
    ctx.db.insert(st, s)?;
    let rt = ctx.table(BANK_REC)?;
    ctx.db.remove(&rt.name, &rec.key(rt));
    Ok(json!({"open": {"page": "bank-account-statement", "key": [bank_no, stmt_no]}, "message": format!("Statement {stmt_no} of bank account {bank_no} was posted.")}))
}

/// A new reconciliation takes the bank's next statement number and its last balance.
pub fn on_insert(ctx: &Ctx, r: &mut Record) -> Result<()> {
    let bank_no = r.str("Bank Account No.").to_string();
    if bank_no.is_empty() {
        return Ok(());
    }
    let bank = ctx.get_code(BANK_ACCOUNT, &bank_no)?;
    if r.str("Statement No.").is_empty() {
        let last = bank.str("Last Statement No.");
        let next = if last.is_empty() { "1".to_string() } else { crate::noseries::increment(last, 1).unwrap_or_else(|| format!("{last}1")) };
        r.set("Statement No.", Value::text(next));
    }
    r.set("Balance Last Statement", bank.get("Balance Last Statement").clone());
    if r.get("Statement Date").as_date().is_none() {
        r.set("Statement Date", Value::Date(ctx.work_date));
    }
    Ok(())
}

/// A statement line's Difference follows its amount.
pub fn on_line_validate(ctx: &mut Ctx, r: &mut Record, field: &str) {
    if field == "Statement Amount" {
        let applied: Decimal = applied_to(ctx, r).iter().map(|e| e.get("Remaining Amount").as_dec()).sum();
        r.set("Applied Amount", dec(applied));
        r.set("Difference", dec(r.get("Statement Amount").as_dec() - applied));
    }
}

fn s_rec() -> Json {
    json!({"type": "object", "properties": {"bank_account_no": {"type": "string"}, "statement_no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "bank_rec.suggest_lines", label: "Suggest Lines", group: "Cash Management", summary: "Statement lines for the open bank entries up to the statement date, matched to them.", mutates: true, params: s_rec, run: suggest_lines },
    CommandDef { id: "bank_rec.match", label: "Match Automatically", group: "Cash Management", summary: "Match unmatched statement lines to open bank entries of the same amount.", mutates: true, params: s_rec, run: match_lines },
    CommandDef { id: "bank_rec.remove_match", label: "Remove Match", group: "Cash Management", summary: "Remove every match of a bank reconciliation.", mutates: true, params: s_rec, run: remove_match },
    CommandDef { id: "bank_rec.post", label: "Post", group: "Cash Management", summary: "Post a bank reconciliation: close the matched bank entries and file the statement.", mutates: true, params: s_rec, run: post },
];
