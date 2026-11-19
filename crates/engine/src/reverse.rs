//! Reverse Transaction (BC's Reversal Entry): posts the opposite of every G/L, customer, vendor
//! and bank entry of a journal transaction and marks both sides reversed. Transactions posted from
//! sales / purchase documents are corrected with credit memos instead, as in BC.

use erpcraft_meta::{Key, Record, Value};
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::ledger::{PartyEntry, Txn, apply, post_bank_entry, post_party_entry, register};
use crate::refuse;
use crate::schema::finance::{DTLD_CUST_LEDGER, DTLD_VEND_LEDGER, GL_REGISTER};
use crate::schema::tables::{BANK_LEDGER, CUST_LEDGER, GL_ENTRY, VEND_LEDGER};
use crate::side::{PURCHASE, SALES};

const DOCUMENT_SOURCES: &[&str] = &["SALES", "PURCHASES"];

fn entries(ctx: &Ctx, table: &str, txn: i64) -> Vec<Record> {
    ctx.db.rows(table).filter(|r| r.get("Transaction No.").as_int() == txn).cloned().collect()
}

pub fn reverse(ctx: &mut Ctx, txn_no: i64) -> Result<Json> {
    let gl = entries(ctx, GL_ENTRY, txn_no);
    if gl.is_empty() {
        refuse!("There is no G/L transaction {txn_no} to reverse.");
    }
    if gl.iter().any(|e| e.get("Reversed").as_bool()) {
        refuse!("Transaction {txn_no} has already been reversed.");
    }
    let source = gl.first().map(|e| e.str("Source Code").to_string()).unwrap_or_default();
    if DOCUMENT_SOURCES.contains(&source.as_str()) || source == "REVERSAL" {
        refuse!(
            "You cannot reverse transaction {txn_no} because it was posted from a {}. Use a corrective credit memo instead.",
            if source == "REVERSAL" { "reversal" } else { "sales or purchase document" }
        );
    }
    if ctx.db.rows(crate::schema::fixed_assets::FA_LEDGER).any(|e| e.get("Transaction No.").as_int() == txn_no) {
        refuse!("Transaction {txn_no} includes fixed asset entries. Post a correcting FA journal line instead.");
    }
    let cust = entries(ctx, CUST_LEDGER, txn_no);
    let vend = entries(ctx, VEND_LEDGER, txn_no);
    for (side, list, dtld) in [(&SALES, &cust, DTLD_CUST_LEDGER), (&PURCHASE, &vend, DTLD_VEND_LEDGER)] {
        let field = if side.sales { "Cust. Ledger Entry No." } else { "Vendor Ledger Entry No." };
        for e in list.iter() {
            let no = e.get("Entry No.").as_int();
            let applied =
                ctx.db.rows(dtld).any(|d| d.get(field).as_int() == no && d.str("Entry Type") == "Application" && !d.get("Unapplied").as_bool());
            if applied {
                refuse!(
                    "You cannot reverse {} Ledger Entry {no} because the entry is applied to an entry. You must unapply all involved entries first.",
                    side.party_caption
                );
            }
        }
    }

    let txn = Txn::new(ctx, "REVERSAL", "", "");
    let date = gl.first().and_then(|e| e.get("Posting Date").as_date()).unwrap_or(ctx.work_date);
    let glt = ctx.table(GL_ENTRY)?;
    let first = ctx.db.next_entry_no(GL_ENTRY);
    let mut next = first;
    for e in &gl {
        let mut r = e.clone();
        let amount = e.get("Amount").as_dec();
        r.set("Entry No.", Value::Int(next));
        r.set("Amount", dec(-amount));
        r.set("Debit Amount", dec((-amount).max(rust_decimal::Decimal::ZERO)));
        r.set("Credit Amount", dec(amount.max(rust_decimal::Decimal::ZERO)));
        r.set("VAT Amount", dec(-e.get("VAT Amount").as_dec()));
        r.set("Description", Value::text(format!("Reversal of {}", e.str("Description"))));
        r.set("Transaction No.", Value::Int(txn.no));
        r.set("Source Code", Value::text("REVERSAL"));
        r.set("Reversed", Value::Bool(true));
        r.set("Reversed Entry No.", e.get("Entry No.").clone());
        r.set("Reversed by Entry No.", Value::Int(0));
        r.set("User ID", Value::text(ctx.user.to_uppercase()));
        r.set(crate::db::VERSION_FIELD, Value::Null);
        ctx.db.insert(glt, r)?;
        let mut o = e.clone();
        o.set("Reversed", Value::Bool(true));
        o.set("Reversed by Entry No.", Value::Int(next));
        ctx.db.modify(glt, o)?;
        next = next.saturating_add(1);
    }
    register(ctx, &txn, first, next - 1)?;
    let rt = ctx.table(GL_REGISTER)?;
    let regs: Vec<Record> = entries(ctx, GL_REGISTER, txn_no);
    for mut r in regs {
        r.set("Reversed", Value::Bool(true));
        ctx.db.modify(rt, r)?;
    }

    // Customer / vendor entries: an opposite entry, applied to the original so both close.
    for (side, list) in [(&SALES, cust), (&PURCHASE, vend)] {
        let lt = ctx.table(side.ledger)?;
        for e in list {
            let amount = e.get("Amount").as_dec();
            let new_no = post_party_entry(
                ctx,
                side,
                &txn,
                &PartyEntry {
                    party_no: e.str(side.ledger_party),
                    posting_date: date,
                    doc_type: e.str("Document Type"),
                    doc_no: e.str("Document No."),
                    ext_doc_no: e.str("External Document No."),
                    description: &format!("Reversal of {}", e.str("Description")),
                    currency: e.str("Currency Code"),
                    amount: -amount,
                    sales_purch: -e.get(if side.sales { "Sales (LCY)" } else { "Purchase (LCY)" }).as_dec(),
                    due_date: e.get("Due Date").as_date(),
                    person: e.str(side.person),
                    posting_group: e.str(side.posting_group),
                },
            )?;
            if !amount.is_zero() {
                apply(ctx, side, new_no, &[e.get("Entry No.").as_int()], date)?;
            }
            for (no, by) in [(e.get("Entry No.").as_int(), new_no), (new_no, e.get("Entry No.").as_int())] {
                let mut r = ctx.get(&lt.name, &Key(vec![Value::Int(no)]))?;
                r.set("Reversed", Value::Bool(true));
                if no == new_no {
                    r.set("Reversed Entry No.", Value::Int(by));
                } else {
                    r.set("Reversed by Entry No.", Value::Int(by));
                }
                ctx.db.modify(lt, r)?;
            }
        }
    }
    // Bank entries.
    let blt = ctx.table(BANK_LEDGER)?;
    for e in entries(ctx, BANK_LEDGER, txn_no) {
        let new_no = post_bank_entry(
            ctx,
            &txn,
            e.str("Bank Account No."),
            date,
            e.str("Document Type"),
            e.str("Document No."),
            &format!("Reversal of {}", e.str("Description")),
            -e.get("Amount").as_dec(),
            e.str("Bal. Account Type"),
            e.str("Bal. Account No."),
        )?;
        let mut o = e.clone();
        o.set("Reversed", Value::Bool(true));
        o.set("Reversed by Entry No.", Value::Int(new_no));
        o.set("Open", Value::Bool(false));
        o.set("Remaining Amount", dec(rust_decimal::Decimal::ZERO));
        ctx.db.modify(blt, o)?;
        let mut n = ctx.get(BANK_LEDGER, &Key(vec![Value::Int(new_no)]))?;
        n.set("Reversed", Value::Bool(true));
        n.set("Reversed Entry No.", e.get("Entry No.").clone());
        n.set("Open", Value::Bool(false));
        n.set("Remaining Amount", dec(rust_decimal::Decimal::ZERO));
        ctx.db.modify(blt, n)?;
    }
    Ok(json!({"reversed_transaction": txn_no, "reversal_transaction": txn.no, "message": format!("Transaction {txn_no} was successfully reversed.")}))
}

/// The transaction number from `transaction_no`, or from the ledger entry given as context.
pub fn transaction_of(ctx: &Ctx, params: &Json) -> Result<i64> {
    if let Some(n) = params.get("transaction_no").and_then(Json::as_i64) {
        return Ok(n);
    }
    let table = params.get("table").and_then(Json::as_str).unwrap_or_default();
    let key = params.get("key").ok_or_else(|| EngineError::invalid("missing parameter 'transaction_no'"))?;
    let t = ctx.table(table)?;
    let key = Key::from_json(t, key).map_err(EngineError::invalid)?;
    let r = ctx.get(&t.name, &key)?;
    let n = r.get("Transaction No.").as_int();
    if n == 0 {
        refuse!("The selected entry has no transaction number.");
    }
    Ok(n)
}
