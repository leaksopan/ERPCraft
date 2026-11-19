//! General journals (General, Cash Receipts, Payments): line defaults and validation, and posting
//! (BC's Gen. Jnl.-Post Batch). Every document (Document No. + Posting Date) must balance unless
//! its lines carry a balancing account.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::ledger::{PartyEntry, Txn, apply, bank_gl_account, open_entries_against, party_gl_account, post_bank_entry, post_party_entry};
use crate::noseries;
use crate::posting::{GlDoc, GlLine, check_posting_date, post_gl};
use crate::refuse;
use crate::schema::finance::{GEN_JNL_BATCH, GEN_JNL_LINE, GEN_JNL_TEMPLATE};
use crate::schema::tables::{BANK_ACCOUNT, CUSTOMER, GL_ACCOUNT, VENDOR};
use crate::side::{PURCHASE, SALES, Side};

fn batch_lines<'a>(ctx: &'a Ctx, template: &'a str, batch: &'a str) -> impl Iterator<Item = &'a Record> {
    ctx.db.rows(GEN_JNL_LINE).filter(move |l| l.str("Journal Template Name") == template && l.str("Journal Batch Name") == batch)
}

/// `OnInsert`: line number, posting date, document number and type, balancing account.
pub fn on_insert(ctx: &mut Ctx, r: &mut Record) -> Result<()> {
    let template = r.str("Journal Template Name").to_string();
    let batch = r.str("Journal Batch Name").to_string();
    let tmpl = ctx.get_code(GEN_JNL_TEMPLATE, &template)?;
    let b = ctx.get(GEN_JNL_BATCH, &Key(vec![Value::text(&template), Value::text(&batch)]))?;
    let last = batch_lines(ctx, &template, &batch).max_by_key(|l| l.get("Line No.").as_int()).cloned();
    if r.get("Line No.").as_int() == 0 {
        r.set("Line No.", Value::Int(last.as_ref().map_or(0, |l| l.get("Line No.").as_int()).saturating_add(10000)));
    }
    if r.get("Posting Date").as_date().is_none() {
        r.set("Posting Date", last.as_ref().map(|l| l.get("Posting Date").clone()).filter(|v| v.as_date().is_some()).unwrap_or(Value::Date(ctx.work_date)));
    }
    if r.str("Document No.").is_empty() {
        let next = match &last {
            Some(l) if !l.str("Document No.").is_empty() => {
                // A new document after a balanced one; otherwise continue the open one.
                let open: Decimal = batch_lines(ctx, &template, &batch)
                    .filter(|x| x.str("Document No.") == l.str("Document No.") && x.str("Bal. Account No.").is_empty())
                    .map(|x| x.get("Amount").as_dec())
                    .sum();
                if open.is_zero() && b.str("No. Series").is_empty() {
                    noseries::increment(l.str("Document No."), 1).unwrap_or_else(|| l.str("Document No.").to_string())
                } else if open.is_zero() {
                    peek_after(ctx, b.str("No. Series"), l.str("Document No."))
                } else {
                    l.str("Document No.").to_string()
                }
            }
            _ if !b.str("No. Series").is_empty() => noseries::peek(ctx, b.str("No. Series"))?,
            _ => String::new(),
        };
        r.set("Document No.", Value::text(next));
    }
    if r.str("Document Type").is_empty() {
        match tmpl.str("Type") {
            "Cash Receipts" | "Payments" => r.set("Document Type", Value::text("Payment")),
            _ => {}
        }
    }
    if r.str("Bal. Account No.").is_empty() && !b.str("Bal. Account No.").is_empty() {
        r.set("Bal. Account Type", b.get("Bal. Account Type").clone());
        r.set("Bal. Account No.", b.get("Bal. Account No.").clone());
    }
    r.set("Source Code", tmpl.get("Source Code").clone());
    Ok(())
}

/// The number after `current` in `series`, or the series' next number when that is later.
fn peek_after(ctx: &Ctx, series: &str, current: &str) -> String {
    let next = noseries::peek(ctx, series).unwrap_or_default();
    let after = noseries::increment(current, 1).unwrap_or_default();
    if after.len() == next.len() && after > next { after } else if next.is_empty() { after } else { next }
}

fn account_name(ctx: &Ctx, account_type: &str, no: &str) -> Result<String> {
    let (table, field) = match account_type {
        "Customer" => (CUSTOMER, "Name"),
        "Vendor" => (VENDOR, "Name"),
        "Bank Account" => (BANK_ACCOUNT, "Name"),
        _ => (GL_ACCOUNT, "Name"),
    };
    let r = ctx.get_code(table, no)?;
    if table == GL_ACCOUNT {
        if r.str("Account Type") != "Posting" {
            refuse!("Account Type must be equal to 'Posting' in G/L Account: No.={no}. Current value is '{}'.", r.str("Account Type"));
        }
        if r.get("Blocked").as_bool() {
            refuse!("Blocked must be equal to 'No' in G/L Account: No.={no}. Current value is 'Yes'.");
        }
    }
    if (table == CUSTOMER || table == VENDOR) && r.str("Blocked") == "All" {
        refuse!("{account_type} {no} is blocked with type All.");
    }
    Ok(r.str(field).to_string())
}

fn side_of(account_type: &str) -> Option<&'static Side> {
    match account_type {
        "Customer" => Some(&SALES),
        "Vendor" => Some(&PURCHASE),
        _ => None,
    }
}

fn set_amount(r: &mut Record, amount: Decimal) {
    r.set("Amount", dec(amount));
    r.set("Debit Amount", dec(amount.max(Decimal::ZERO)));
    r.set("Credit Amount", dec((-amount).max(Decimal::ZERO)));
}

/// Field `OnValidate` for journal lines.
pub fn on_validate(ctx: &mut Ctx, r: &mut Record, field: &str) -> Result<()> {
    match field {
        "Account Type" => {
            r.set("Account No.", Value::text(""));
            r.set("Description", Value::text(""));
        }
        "Bal. Account Type" => r.set("Bal. Account No.", Value::text("")),
        "Account No." => {
            let no = r.str("Account No.").to_string();
            if !no.is_empty() {
                let name = account_name(ctx, r.str("Account Type"), &no)?;
                if r.str("Description").is_empty() || field == "Account No." {
                    r.set("Description", Value::text(name));
                }
            }
        }
        "Bal. Account No." => {
            let no = r.str("Bal. Account No.").to_string();
            if !no.is_empty() {
                account_name(ctx, r.str("Bal. Account Type"), &no)?;
            }
        }
        "Amount" => {
            let a = r.get("Amount").as_dec();
            set_amount(r, a);
        }
        "Debit Amount" => {
            let a = r.get("Debit Amount").as_dec();
            set_amount(r, a);
        }
        "Credit Amount" => {
            let a = r.get("Credit Amount").as_dec();
            set_amount(r, -a);
        }
        "Applies-to Doc. No." => {
            let doc = r.str("Applies-to Doc. No.").to_string();
            if doc.is_empty() {
                return Ok(());
            }
            let Some(side) = side_of(r.str("Account Type")) else {
                refuse!("Applies-to Doc. No. can only be used on lines for a customer or a vendor.");
            };
            let party = r.str("Account No.").to_string();
            let entry = ctx
                .db
                .rows(side.ledger)
                .find(|e| e.str(side.ledger_party) == party && e.str("Document No.") == doc && e.get("Open").as_bool())
                .cloned()
                .ok_or_else(|| EngineError::refused(format!("There is no open {} ledger entry for document {doc}.", side.party_caption.to_lowercase())))?;
            r.set("Applies-to Doc. Type", entry.get("Document Type").clone());
            if r.get("Amount").as_dec().is_zero() {
                set_amount(r, -entry.get("Remaining Amount").as_dec());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Problems that would stop posting: documents out of balance, missing accounts.
pub fn check(ctx: &Ctx, template: &str, batch: &str) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut balance: BTreeMap<(String, Option<NaiveDate>), Decimal> = BTreeMap::new();
    for l in batch_lines(ctx, template, batch) {
        if l.str("Account No.").is_empty() && l.get("Amount").as_dec().is_zero() {
            continue;
        }
        let line = l.get("Line No.");
        if l.str("Account No.").is_empty() {
            problems.push(format!("Line {line}: Account No. must have a value."));
        }
        if l.str("Document No.").is_empty() {
            problems.push(format!("Line {line}: Document No. must have a value."));
        }
        if l.get("Posting Date").as_date().is_none() {
            problems.push(format!("Line {line}: Posting Date must have a value."));
        }
        if l.get("Amount").as_dec().is_zero() {
            problems.push(format!("Line {line}: Amount must have a value."));
        }
        if l.str("Bal. Account No.").is_empty() {
            *balance.entry((l.str("Document No.").to_string(), l.get("Posting Date").as_date())).or_default() += l.get("Amount").as_dec();
        }
    }
    for ((doc, date), diff) in balance {
        if !diff.is_zero() {
            problems.push(format!(
                "Document No. {doc} is out of balance by {diff} (posting date {}). Please check that Posting Date, Document Type, Document No. and Amount are correct for each line.",
                date.map(|d| d.to_string()).unwrap_or_default()
            ));
        }
    }
    Ok(problems)
}

/// The effect of one account side of a journal line.
struct Effect<'a> {
    account_type: &'a str,
    account_no: &'a str,
    amount: Decimal,
}

/// Posts every line of the batch; documents become transactions; posted lines are deleted.
pub fn post(ctx: &mut Ctx, template: &str, batch: &str) -> Result<Json> {
    let problems = check(ctx, template, batch)?;
    if let Some(p) = problems.first() {
        return Err(EngineError::refused(p.clone()));
    }
    let lt = ctx.table(GEN_JNL_LINE)?;
    let mut lines: Vec<Record> =
        batch_lines(ctx, template, batch).filter(|l| !(l.str("Account No.").is_empty() && l.get("Amount").as_dec().is_zero())).cloned().collect();
    if lines.is_empty() {
        refuse!("There is nothing to post.");
    }
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    let source_code = ctx.get_code(GEN_JNL_TEMPLATE, template)?.str("Source Code").to_string();
    let series = ctx.get(GEN_JNL_BATCH, &Key(vec![Value::text(template), Value::text(batch)]))?.str("No. Series").to_string();

    // Group lines into documents (Document No. + Posting Date), in line order.
    let mut docs: Vec<((String, NaiveDate), Vec<Record>)> = Vec::new();
    for l in lines.iter().cloned() {
        let date = l.get("Posting Date").as_date().ok_or_else(|| EngineError::refused("Posting Date must have a value."))?;
        let k = (l.str("Document No.").to_string(), date);
        match docs.iter_mut().find(|(dk, _)| *dk == k) {
            Some((_, v)) => v.push(l),
            None => docs.push((k, vec![l])),
        }
    }
    let mut transactions = 0;
    for ((doc_no, date), doc_lines) in &docs {
        check_posting_date(ctx, *date)?;
        if !series.is_empty() && noseries::peek(ctx, &series).ok().as_deref() == Some(doc_no.as_str()) {
            noseries::next(ctx, &series)?;
        }
        let txn = Txn::new(ctx, &source_code, template, batch);
        let mut gl: Vec<GlLine> = Vec::new();
        let mut party_effects: Vec<(&Record, Effect)> = Vec::new();
        for l in doc_lines {
            let amount = l.get("Amount").as_dec();
            let desc = l.str("Description").to_string();
            let mut sides = vec![Effect { account_type: l.str("Account Type"), account_no: l.str("Account No."), amount }];
            if !l.str("Bal. Account No.").is_empty() {
                sides.push(Effect { account_type: l.str("Bal. Account Type"), account_no: l.str("Bal. Account No."), amount: -amount });
            }
            for s in sides {
                let gl_account = match s.account_type {
                    "Customer" => party_gl_account(ctx, &SALES, s.account_no)?.0,
                    "Vendor" => party_gl_account(ctx, &PURCHASE, s.account_no)?.0,
                    "Bank Account" => bank_gl_account(ctx, s.account_no)?,
                    _ => s.account_no.to_string(),
                };
                gl.push(GlLine { account: gl_account, amount: s.amount, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                if s.account_type != "G/L Account" {
                    party_effects.push((l, s));
                }
            }
        }
        let first = doc_lines.first();
        let doc_type = first.map(|l| l.str("Document Type").to_string()).unwrap_or_default();
        let ext = first.map(|l| l.str("External Document No.").to_string()).unwrap_or_default();
        post_gl(ctx, &GlDoc { posting_date: *date, doc_type: &doc_type, doc_no, ext_doc_no: &ext, source_type: "", source_no: "", txn: &txn }, gl)?;
        for (l, s) in party_effects {
            let desc = l.str("Description");
            match s.account_type {
                "Bank Account" => {
                    let (bal_type, bal_no) = if l.str("Account No.") == s.account_no && l.str("Account Type") == "Bank Account" {
                        (l.str("Bal. Account Type"), l.str("Bal. Account No."))
                    } else {
                        (l.str("Account Type"), l.str("Account No."))
                    };
                    post_bank_entry(ctx, &txn, s.account_no, *date, l.str("Document Type"), doc_no, desc, s.amount, bal_type, bal_no)?;
                }
                t => {
                    let Some(side) = side_of(t) else { continue };
                    let (_, group) = party_gl_account(ctx, side, s.account_no)?;
                    let entry = post_party_entry(
                        ctx,
                        side,
                        &txn,
                        &PartyEntry {
                            party_no: s.account_no,
                            posting_date: *date,
                            doc_type: l.str("Document Type"),
                            doc_no,
                            ext_doc_no: l.str("External Document No."),
                            description: desc,
                            currency: "",
                            amount: s.amount,
                            sales_purch: Decimal::ZERO,
                            due_date: l.get("Due Date").as_date().or(Some(*date)),
                            person: "",
                            posting_group: &group,
                        },
                    )?;
                    // Applies-to belongs to the line's own account, not its balancing account.
                    let is_main = l.str("Account No.") == s.account_no && l.str("Account Type") == t;
                    let applies_to = l.str("Applies-to Doc. No.");
                    if is_main && !applies_to.is_empty() {
                        let targets = open_entries_against(ctx, side, s.account_no, s.amount, Some(applies_to));
                        if targets.is_empty() {
                            refuse!("There is no open {} ledger entry for document {applies_to}.", side.party_caption.to_lowercase());
                        }
                        apply(ctx, side, entry, &targets, *date)?;
                    }
                }
            }
        }
        transactions += 1;
    }
    for l in &lines {
        ctx.db.remove(&lt.name, &l.key(lt));
    }
    Ok(json!({
        "posted_lines": lines.len(),
        "transactions": transactions,
        "message": "The journal lines were successfully posted.",
    }))
}
