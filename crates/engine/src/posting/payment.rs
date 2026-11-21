//! Customer and vendor payments in one step (what a one-line Cash Receipt / Payment Journal
//! posts), applied to open invoices oldest due date first, or to one document.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use super::{GlDoc, GlLine, check_posting_date, post_gl};
use crate::ctx::Ctx;
use crate::error::Result;
use crate::ledger::{PartyEntry, Txn, apply, bank_gl_account, open_entries_against, party_gl_account, post_bank_entry, post_party_entry};
use crate::noseries;
use crate::refuse;
use crate::schema::tables::GL_SETUP;
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
    let bank_gl = bank_gl_account(ctx, p.bank_account)?;
    let (party_gl, group) = party_gl_account(ctx, side, p.party_no)?;
    let series = ctx.setup(GL_SETUP)?.str("Payment Nos.").to_string();
    if series.is_empty() {
        refuse!("You must specify a number series in the Payment Nos. field on the General Ledger Setup page.");
    }
    // Check the document to apply to before anything is written.
    let s = if side.sales { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    let pay_amount = -s * p.amount;
    let targets = open_entries_against(ctx, side, p.party_no, pay_amount, p.applies_to_doc_no);
    if let Some(d) = p.applies_to_doc_no
        && targets.is_empty()
    {
        refuse!("There is no open {} ledger entry for document {d} to apply the payment to.", side.party_caption.to_lowercase());
    }
    let doc_no = noseries::next(ctx, &series)?;
    let description = p.description.map_or_else(|| format!("Payment {}", party.str("Name")), str::to_string);
    let txn = Txn::new(ctx, "PAYMENT", "", "");

    // Sales: the customer is credited (-), the bank debited (+). Purchase: the reverse.
    post_gl(
        ctx,
        &GlDoc { posting_date: p.posting_date, doc_type: "Payment", doc_no: &doc_no, ext_doc_no: "", source_type: side.party_caption, source_no: p.party_no, txn: &txn },
        vec![
            GlLine { account: bank_gl, amount: s * p.amount, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" },
            GlLine { account: party_gl, amount: -s * p.amount, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" },
        ],
    )?;
    post_bank_entry(ctx, &txn, p.bank_account, p.posting_date, "Payment", &doc_no, &description, s * p.amount, side.party_caption, p.party_no)?;
    let entry = post_party_entry(
        ctx,
        side,
        &txn,
        &PartyEntry {
            party_no: p.party_no,
            posting_date: p.posting_date,
            document_date: None,
            doc_type: "Payment",
            doc_no: &doc_no,
            ext_doc_no: "",
            description: &description,
            currency: "",
            amount: pay_amount,
            amount_lcy: pay_amount,
            sales_purch: Decimal::ZERO,
            due_date: Some(p.posting_date),
            person: "",
            posting_group: &group,
        },
    )?;
    let applied = apply(ctx, side, entry, &targets, p.posting_date)?;
    let applied_total: Decimal = applied.iter().map(|(_, a)| *a).sum();
    Ok(json!({
        "document_no": doc_no,
        "entry_no": entry,
        "applied": applied.iter().map(|(d, a)| json!({"document_no": d, "amount": a.to_string()})).collect::<Vec<_>>(),
        "unapplied": (p.amount - applied_total).to_string(),
        "message": format!("Payment {doc_no} of {} posted for {} {}.", p.amount, side.party_caption, p.party_no),
    }))
}
