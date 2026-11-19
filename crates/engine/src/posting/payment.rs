//! Customer and vendor payments (what BC's Cash Receipt / Payment Journal posts), applied to open
//! invoices oldest due date first, or to one document.

use chrono::NaiveDate;
use erpcraft_meta::{Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use super::{GlDoc, GlLine, check_posting_date, post_gl};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::noseries;
use crate::refuse;
use crate::schema::tables::{BANK_ACCOUNT, BANK_LEDGER, BANK_POSTING_GROUP, CUST_POSTING_GROUP, GL_SETUP, VEND_POSTING_GROUP};
use crate::side::Side;

pub struct Payment<'a> {
    pub party_no: &'a str,
    pub amount: Decimal,
    pub bank_account: &'a str,
    pub posting_date: NaiveDate,
    pub applies_to_doc_no: Option<&'a str>,
    pub description: Option<&'a str>,
}

pub fn register(ctx: &mut Ctx, side: &Side, p: &Payment) -> Result<Json> {
    if p.amount <= Decimal::ZERO {
        refuse!("The payment amount must be positive.");
    }
    check_posting_date(ctx, p.posting_date)?;
    let party = ctx.get_code(side.party_table, p.party_no)?;
    let blocked = party.str("Blocked");
    if blocked == "All" || (!side.sales && blocked == "Payment") {
        refuse!("You cannot post a payment for {} {} because it is blocked with type {blocked}.", side.party_caption, p.party_no);
    }
    let bank = ctx.get_code(BANK_ACCOUNT, p.bank_account)?;
    if bank.get("Blocked").as_bool() {
        refuse!("Blocked must be equal to 'No' in Bank Account: No.={}. Current value is 'Yes'.", p.bank_account);
    }
    let bank_group = bank.str("Bank Acc. Posting Group").to_string();
    let bank_gl = ctx
        .db
        .get_code(BANK_POSTING_GROUP, &bank_group)
        .map(|r| r.str("G/L Account No.").to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::refused(format!("Bank Account {} has no Bank Acc. Posting Group with a G/L Account No.", p.bank_account)))?;
    let group = party.str(side.posting_group).to_string();
    let party_gl = if side.sales {
        ctx.db.get_code(CUST_POSTING_GROUP, &group).map(|r| r.str("Receivables Account").to_string())
    } else {
        ctx.db.get_code(VEND_POSTING_GROUP, &group).map(|r| r.str("Payables Account").to_string())
    }
    .filter(|s| !s.is_empty())
    .ok_or_else(|| EngineError::refused(format!("{} must have a value in {}: No.={}.", side.posting_group, side.party_caption, p.party_no)))?;

    let series = ctx.setup(GL_SETUP)?.str("Payment Nos.").to_string();
    if series.is_empty() {
        refuse!("You must specify a number series in the Payment Nos. field on the General Ledger Setup page.");
    }
    let doc_no = noseries::next(ctx, &series)?;
    let description = p.description.map_or_else(|| format!("Payment {}", party.str("Name")), str::to_string);

    // Sales: the customer is credited (-), the bank debited (+). Purchase: the reverse.
    let s = if side.sales { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    post_gl(
        ctx,
        &GlDoc {
            posting_date: p.posting_date,
            doc_type: "Payment",
            doc_no: &doc_no,
            ext_doc_no: "",
            source_type: side.party_caption,
            source_no: p.party_no,
        },
        vec![
            GlLine { account: bank_gl, amount: s * p.amount, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" },
            GlLine { account: party_gl, amount: -s * p.amount, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" },
        ],
    )?;

    let blt = ctx.table(BANK_LEDGER)?;
    let mut be = blt.init_record();
    be.set("Entry No.", Value::Int(ctx.db.next_entry_no(BANK_LEDGER)));
    be.set("Bank Account No.", Value::text(p.bank_account));
    be.set("Posting Date", Value::Date(p.posting_date));
    be.set("Document Type", Value::text("Payment"));
    be.set("Document No.", Value::text(&doc_no));
    be.set("Description", Value::text(&description));
    be.set("Amount", dec(s * p.amount));
    be.set("Remaining Amount", dec(s * p.amount));
    be.set("Bal. Account Type", Value::text(side.party_caption));
    be.set("Bal. Account No.", Value::text(p.party_no));
    be.set("Open", Value::Bool(true));
    be.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(blt, be)?;

    // Ledger entry for the payment, then apply it.
    let lt = ctx.table(side.ledger)?;
    let pay_entry_no = ctx.db.next_entry_no(side.ledger);
    let pay_amount = -s * p.amount;
    let mut open: Vec<Record> = ctx
        .db
        .rows(side.ledger)
        .filter(|e| {
            e.str(side.ledger_party) == p.party_no
                && e.get("Open").as_bool()
                && (e.get("Remaining Amount").as_dec() * s) > Decimal::ZERO
                && p.applies_to_doc_no.is_none_or(|d| e.str("Document No.") == d)
        })
        .cloned()
        .collect();
    if let Some(d) = p.applies_to_doc_no
        && open.is_empty()
    {
        refuse!("There is no open {} ledger entry for document {d} to apply the payment to.", side.party_caption.to_lowercase());
    }
    open.sort_by(|a, b| (a.get("Due Date"), a.get("Entry No.")).cmp(&(b.get("Due Date"), b.get("Entry No."))));
    let mut left = p.amount;
    let mut applied = Vec::new();
    for mut e in open {
        if left.is_zero() {
            break;
        }
        let rem = e.get("Remaining Amount").as_dec();
        let take = left.min(rem.abs());
        let new_rem = rem - s * take;
        e.set("Remaining Amount", dec(new_rem));
        if new_rem.is_zero() {
            e.set("Open", Value::Bool(false));
            e.set("Closed by Entry No.", Value::Int(pay_entry_no));
        }
        applied.push(json!({"document_no": e.str("Document No."), "amount": take.to_string()}));
        left -= take;
        ctx.db.modify(lt, e)?;
    }
    let mut e = lt.init_record();
    e.set("Entry No.", Value::Int(pay_entry_no));
    e.set(side.ledger_party, Value::text(p.party_no));
    e.set(if side.sales { "Customer Name" } else { "Vendor Name" }, party.get("Name").clone());
    e.set("Posting Date", Value::Date(p.posting_date));
    e.set("Document Type", Value::text("Payment"));
    e.set("Document No.", Value::text(&doc_no));
    e.set("Description", Value::text(&description));
    e.set("Amount", dec(pay_amount));
    e.set("Amount (LCY)", dec(pay_amount));
    e.set("Remaining Amount", dec(-s * left));
    e.set("Open", Value::Bool(!left.is_zero()));
    e.set("Due Date", Value::Date(p.posting_date));
    e.set(side.posting_group, Value::text(&group));
    e.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(lt, e)?;
    Ok(json!({
        "document_no": doc_no,
        "applied": applied,
        "unapplied": left.to_string(),
        "message": format!("Payment {doc_no} of {} posted for {} {}.", p.amount, side.party_caption, p.party_no),
    }))
}
