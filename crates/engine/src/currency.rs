//! Currencies (BC's Currency Exchange Rate, CurrExchRate.ExchangeAmtFCYToLCY, report 595 Exch.
//! Rate Adjustment): the currency factor of a date, conversions, and the unrealized gains and
//! losses on open customer and vendor entries.
//!
//! A currency factor is FCY per LCY: Exchange Rate Amount / Relational Exch. Rate Amount
//! (100 EUR = 1,700,000 IDR → 100 / 1,700,000).

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, round_amount};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::side::{PURCHASE, SALES, Side};

pub const EXCH_RATE: &str = "Currency Exchange Rate";

/// The exchange rate in force on `date`: the latest one starting on or before it.
fn rate_on(ctx: &Ctx, code: &str, date: NaiveDate) -> Result<Record> {
    ctx.db
        .rows(EXCH_RATE)
        .filter(|r| r.str("Currency Code").eq_ignore_ascii_case(code) && r.get("Starting Date").as_date().is_some_and(|d| d <= date))
        .max_by_key(|r| r.get("Starting Date").as_date())
        .cloned()
        .ok_or_else(|| EngineError::refused(format!("The Currency Exchange Rate does not exist. Identification fields and values: Currency Code='{code}',Starting Date='{date}'")))
}

fn ratio(a: Decimal, b: Decimal) -> Result<Decimal> {
    if a.is_zero() || b.is_zero() {
        refuse!("Exchange Rate Amount and Relational Exch. Rate Amount must not be 0.");
    }
    Ok(a / b)
}

/// The currency factor of `code` on `date` (1 for LCY, a blank code).
pub fn factor(ctx: &Ctx, code: &str, date: NaiveDate) -> Result<Decimal> {
    if code.trim().is_empty() {
        return Ok(Decimal::ONE);
    }
    let r = rate_on(ctx, code, date)?;
    ratio(r.get("Exchange Rate Amount").as_dec(), r.get("Relational Exch. Rate Amount").as_dec())
}

/// The adjustment factor of `code` on `date` (Adjustment Exch. Rate Amount / Relational Adjmt
/// Exch Rate Amt), for Adjust Exchange Rates.
pub fn adjustment_factor(ctx: &Ctx, code: &str, date: NaiveDate) -> Result<Decimal> {
    let r = rate_on(ctx, code, date)?;
    let (a, b) = (r.get("Adjustment Exch. Rate Amount").as_dec(), r.get("Relational Adjmt Exch Rate Amt").as_dec());
    if a.is_zero() || b.is_zero() { ratio(r.get("Exchange Rate Amount").as_dec(), r.get("Relational Exch. Rate Amount").as_dec()) } else { ratio(a, b) }
}

/// An amount in the currency → LCY, rounded to the LCY's precision.
pub fn to_lcy(amount: Decimal, factor: Decimal) -> Decimal {
    if factor.is_zero() || factor == Decimal::ONE { amount } else { round_amount(amount / factor) }
}

/// An LCY amount → the currency (unrounded unit prices are rounded by the caller).
pub fn from_lcy(amount: Decimal, factor: Decimal) -> Decimal {
    if factor.is_zero() { amount } else { amount * factor }
}

/// A currency's G/L account for gains or losses.
pub fn gain_loss_account(ctx: &Ctx, code: &str, realized: bool, gain: bool) -> Result<String> {
    let c = ctx.get_code("Currency", code)?;
    let field = match (realized, gain) {
        (true, true) => "Realized Gains Acc.",
        (true, false) => "Realized Losses Acc.",
        (false, true) => "Unrealized Gains Acc.",
        (false, false) => "Unrealized Losses Acc.",
    };
    match c.str(field) {
        "" => refuse!("{field} must have a value in Currency: Code={code}. It cannot be zero or empty."),
        a => Ok(a.to_string()),
    }
}

/// The receivables / payables account of a ledger entry's posting group.
pub fn party_account(ctx: &Ctx, side: &Side, entry: &Record) -> Result<String> {
    let (table, field) = if side.sales { ("Customer Posting Group", "Receivables Account") } else { ("Vendor Posting Group", "Payables Account") };
    let group = entry.str(side.posting_group);
    let g = ctx.get(table, &Key(vec![Value::text(group)]))?;
    match g.str(field) {
        "" => refuse!("{field} must have a value in {table}: Code={group}. It cannot be zero or empty."),
        a => Ok(a.to_string()),
    }
}

pub struct GainLossEntry<'a> {
    pub txn: &'a crate::ledger::Txn,
    pub date: NaiveDate,
    pub doc_no: &'a str,
    pub party_account: &'a str,
    pub other: &'a str,
    pub amount: Decimal,
    pub description: &'a str,
}

/// Two G/L lines moving `amount` (LCY) onto the party account and off the gain / loss account.
pub fn post_gain_loss(ctx: &mut Ctx, entry: &GainLossEntry<'_>) -> Result<()> {
    if entry.amount.is_zero() {
        return Ok(());
    }
    let lines = vec![
        crate::posting::GlLine { account: entry.party_account.to_string(), amount: entry.amount, vat_amount: Decimal::ZERO, description: entry.description.to_string(), gen_posting_type: "" },
        crate::posting::GlLine { account: entry.other.to_string(), amount: -entry.amount, vat_amount: Decimal::ZERO, description: entry.description.to_string(), gen_posting_type: "" },
    ];
    crate::posting::post_gl(ctx, &crate::posting::GlDoc { posting_date: entry.date, doc_type: "", doc_no: entry.doc_no, ext_doc_no: "", source_type: "", source_no: "", txn: entry.txn }, lines)?;
    Ok(())
}

/// BC's Adjust Exchange Rates (report 595, customers and vendors): each open entry in a currency
/// revalued at the adjustment rate of `ending_date`; the difference to its remaining LCY is an
/// unrealized gain or loss, posted to the party account and the currency's unrealized account.
fn adjust(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let ending = match opt_str(p, "ending_date").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?,
        None => ctx.work_date,
    };
    let doc_no = opt_str(p, "document_no").map(str::trim).filter(|s| !s.is_empty()).unwrap_or("EXCHRATEADJ").to_uppercase();
    let only: Option<String> = opt_str(p, "currency_code").map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());
    let currencies: Vec<String> = ctx.db.rows("Currency").map(|c| c.str("Code").to_string()).filter(|c| only.as_ref().is_none_or(|o| o == c)).collect();
    let mut adjusted = 0;
    let mut total = Decimal::ZERO;
    let txn = crate::ledger::Txn::new(ctx, "EXCHRATADJ", "", "");
    for code in currencies {
        let Ok(adj) = adjustment_factor(ctx, &code, ending) else { continue };
        for side in [&SALES, &PURCHASE] {
            let lt = ctx.table(side.ledger)?;
            let open: Vec<Record> = ctx
                .db
                .rows(side.ledger)
                .filter(|e| e.str("Currency Code") == code && e.get("Open").as_bool() && e.get("Posting Date").as_date().is_some_and(|d| d <= ending))
                .cloned()
                .map(|r| crate::commands::read(ctx, lt, r))
                .collect();
            for e in open {
                let rem = e.get("Remaining Amount").as_dec();
                let rem_lcy = e.get("Remaining Amt. (LCY)").as_dec();
                let diff = to_lcy(rem, adj) - rem_lcy;
                if diff.is_zero() {
                    continue;
                }
                let gain = diff.is_sign_positive();
                let entry_type = if gain { "Unrealized Gain" } else { "Unrealized Loss" };
                let no = e.get("Entry No.").as_int();
                crate::ledger::detailed_lcy(ctx, side, txn.no, no, entry_type, ending, e.str("Document Type"), &doc_no, e.str(side.ledger_party), Decimal::ZERO, diff)?;
                let party_acc = party_account(ctx, side, &e)?;
                let other = gain_loss_account(ctx, &code, false, gain)?;
                let desc = format!("Adjmt. of {code} {} {}", e.str("Document Type"), e.str("Document No."));
                post_gain_loss(ctx, &GainLossEntry { txn: &txn, date: ending, doc_no: &doc_no, party_account: &party_acc, other: &other, amount: diff, description: &desc })?;
                adjusted += 1;
                total += diff;
            }
        }
        let ct = ctx.table("Currency")?;
        if let Some(mut c) = ctx.db.get_code("Currency", &code).cloned() {
            c.set("Last Date Adjusted", Value::Date(ending));
            ctx.db.modify(ct, c)?;
        }
    }
    Ok(json!({"entries": adjusted, "total": total.to_string(), "message": format!("{adjusted} entries were adjusted ({total} LCY).")}))
}

fn s_adjust() -> Json {
    json!({"type": "object", "properties": {
        "ending_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"},
        "document_no": {"type": "string", "default": "EXCHRATEADJ"},
        "currency_code": {"type": "string", "description": "one currency; all when blank"}}})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "currency.adjust_exchange_rates",
    label: "Adjust Exchange Rates",
    group: "Finance",
    summary: "Revalue open customer and vendor entries in foreign currency at the adjustment exchange rate, posting unrealized gains and losses.",
    mutates: true,
    params: s_adjust,
    run: adjust,
}];
