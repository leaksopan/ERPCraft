//! Ledger entries shared by every posting routine: transactions and G/L registers, customer and
//! vendor ledger entries with their detailed entries, applications (apply / unapply), and bank
//! account ledger entries. Mirrors BC's Gen. Jnl.-Post Line for these account types.

use chrono::NaiveDate;
use erpcraft_meta::{Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::finance::{DTLD_CUST_LEDGER, DTLD_VEND_LEDGER, GL_REGISTER};
use crate::schema::tables::{BANK_LEDGER, CUST_LEDGER, GL_ENTRY, VEND_LEDGER};
use crate::side::Side;

/// One transaction: every entry it writes shares its number and source code.
pub struct Txn {
    pub no: i64,
    pub source_code: String,
    pub template: String,
    pub batch: String,
    /// The document's or journal line's dimension set: its entries carry it.
    pub dim_set: i64,
}

impl Txn {
    /// The next transaction number (highest used by any ledger + 1).
    pub fn new(ctx: &Ctx, source_code: &str, template: &str, batch: &str) -> Self {
        let max = [GL_ENTRY, CUST_LEDGER, VEND_LEDGER, BANK_LEDGER, DTLD_CUST_LEDGER, DTLD_VEND_LEDGER]
            .iter()
            .flat_map(|t| ctx.db.rows(t).map(|r| r.get("Transaction No.").as_int()))
            .max()
            .unwrap_or(0);
        Self { no: max.saturating_add(1), source_code: source_code.to_string(), template: template.to_string(), batch: batch.to_string(), dim_set: 0 }
    }
}

/// A G/L Register for G/L entries `from..=to` of `txn`.
pub fn register(ctx: &mut Ctx, txn: &Txn, from: i64, to: i64) -> Result<()> {
    let t = ctx.table(GL_REGISTER)?;
    let no = ctx.db.rows(GL_REGISTER).map(|r| r.get("No.").as_int()).max().unwrap_or(0).saturating_add(1);
    let mut r = t.init_record();
    r.set("No.", Value::Int(no));
    r.set("From Entry No.", Value::Int(from));
    r.set("To Entry No.", Value::Int(to));
    r.set("Creation Date", Value::Date(ctx.work_date));
    r.set("Source Code", Value::text(&txn.source_code));
    r.set("User ID", Value::text(ctx.user.to_uppercase()));
    r.set("Journal Templ. Name", Value::text(&txn.template));
    r.set("Journal Batch Name", Value::text(&txn.batch));
    r.set("Transaction No.", Value::Int(txn.no));
    ctx.db.insert(t, r)?;
    Ok(())
}

fn dtld_table(side: &Side) -> &'static str {
    if side.sales { DTLD_CUST_LEDGER } else { DTLD_VEND_LEDGER }
}
fn entry_field(side: &Side) -> &'static str {
    if side.sales { "Cust. Ledger Entry No." } else { "Vendor Ledger Entry No." }
}
fn applied_field(side: &Side) -> &'static str {
    if side.sales { "Applied Cust. Ledger Entry No." } else { "Applied Vend. Ledger Entry No." }
}

/// A customer or vendor ledger entry to write.
pub struct PartyEntry<'a> {
    pub party_no: &'a str,
    pub posting_date: NaiveDate,
    /// The document's own date (BC's Document Date); the posting date when there is none.
    pub document_date: Option<NaiveDate>,
    pub doc_type: &'a str,
    pub doc_no: &'a str,
    pub ext_doc_no: &'a str,
    pub description: &'a str,
    pub currency: &'a str,
    /// Signed: positive = the party owes (customer invoice) / we owe less (vendor payment). In the
    /// entry's currency.
    pub amount: Decimal,
    /// `amount` in LCY (the same for an LCY entry).
    pub amount_lcy: Decimal,
    /// Sales (LCY) for customers, Purchase (LCY) for vendors.
    pub sales_purch: Decimal,
    pub due_date: Option<NaiveDate>,
    pub person: &'a str,
    pub posting_group: &'a str,
}

/// Writes a customer / vendor ledger entry and its "Initial Entry" detailed entry. Returns the
/// entry number.
pub fn post_party_entry(ctx: &mut Ctx, side: &Side, txn: &Txn, e: &PartyEntry) -> Result<i64> {
    let party = ctx.get_code(side.party_table, e.party_no)?;
    let lt = ctx.table(side.ledger)?;
    let entry_no = ctx.db.next_entry_no(side.ledger);
    let mut r = lt.init_record();
    r.set("Entry No.", Value::Int(entry_no));
    r.set(side.ledger_party, Value::text(e.party_no));
    r.set(if side.sales { "Customer Name" } else { "Vendor Name" }, party.get("Name").clone());
    r.set("Posting Date", Value::Date(e.posting_date));
    r.set("Document Date", Value::Date(e.document_date.unwrap_or(e.posting_date)));
    r.set("Document Type", Value::text(e.doc_type));
    r.set("Document No.", Value::text(e.doc_no));
    r.set("External Document No.", Value::text(e.ext_doc_no));
    r.set("Description", Value::text(e.description));
    r.set("Currency Code", Value::text(e.currency));
    r.set("Amount", dec(e.amount));
    r.set("Amount (LCY)", dec(e.amount_lcy));
    r.set("Remaining Amount", dec(e.amount));
    r.set(if side.sales { "Sales (LCY)" } else { "Purchase (LCY)" }, dec(e.sales_purch));
    r.set("Due Date", e.due_date.map_or(Value::Date(e.posting_date), Value::Date));
    r.set("Open", Value::Bool(!e.amount.is_zero()));
    r.set(side.person, Value::text(e.person));
    r.set(side.posting_group, Value::text(e.posting_group));
    r.set("Transaction No.", Value::Int(txn.no));
    r.set("Source Code", Value::text(&txn.source_code));
    r.set("Journal Batch Name", Value::text(&txn.batch));
    r.set("User ID", Value::text(ctx.user.to_uppercase()));
    stamp_dims(ctx, lt, &mut r, txn.dim_set);
    ctx.db.insert(lt, r)?;
    detailed(ctx, side, txn.no, entry_no, "Initial Entry", e.posting_date, e.doc_type, e.doc_no, e.party_no, (e.amount, e.amount_lcy), 0, 0)?;
    Ok(entry_no)
}

/// A detailed entry with its own LCY amount (gains and losses carry LCY only).
#[allow(clippy::too_many_arguments)]
pub fn detailed_lcy(
    ctx: &mut Ctx,
    side: &Side,
    txn: i64,
    ledger_entry: i64,
    entry_type: &str,
    posting_date: NaiveDate,
    doc_type: &str,
    doc_no: &str,
    party_no: &str,
    amount: Decimal,
    amount_lcy: Decimal,
) -> Result<i64> {
    detailed(ctx, side, txn, ledger_entry, entry_type, posting_date, doc_type, doc_no, party_no, (amount, amount_lcy), 0, 0)
}

#[allow(clippy::too_many_arguments)]
fn detailed(
    ctx: &mut Ctx,
    side: &Side,
    txn: i64,
    ledger_entry: i64,
    entry_type: &str,
    posting_date: NaiveDate,
    doc_type: &str,
    doc_no: &str,
    party_no: &str,
    (amount, amount_lcy): (Decimal, Decimal),
    applied_to: i64,
    application_no: i64,
) -> Result<i64> {
    let t = ctx.table(dtld_table(side))?;
    let no = ctx.db.next_entry_no(&t.name);
    let mut d = t.init_record();
    d.set("Entry No.", Value::Int(no));
    d.set(entry_field(side), Value::Int(ledger_entry));
    d.set("Entry Type", Value::text(entry_type));
    // SetLedgerEntryAmount (BC's OnInsert): applications don't count in the entry's amounts.
    d.set("Ledger Entry Amount", Value::Bool(entry_type == "Initial Entry"));
    d.set("Posting Date", Value::Date(posting_date));
    d.set("Document Type", Value::text(doc_type));
    d.set("Document No.", Value::text(doc_no));
    d.set("Amount", dec(amount));
    d.set("Amount (LCY)", dec(amount_lcy));
    d.set("Debit Amount", dec(amount.max(Decimal::ZERO)));
    d.set("Credit Amount", dec((-amount).max(Decimal::ZERO)));
    d.set("Debit Amount (LCY)", dec(amount_lcy.max(Decimal::ZERO)));
    d.set("Credit Amount (LCY)", dec((-amount_lcy).max(Decimal::ZERO)));
    d.set(side.ledger_party, Value::text(party_no));
    d.set("Transaction No.", Value::Int(txn));
    d.set("Application No.", Value::Int(application_no));
    d.set(applied_field(side), Value::Int(applied_to));
    d.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, d)?;
    Ok(no)
}

fn ledger_entry(ctx: &Ctx, side: &Side, entry_no: i64) -> Result<Record> {
    ctx.get(side.ledger, &erpcraft_meta::Key(vec![Value::Int(entry_no)]))
}

/// The detailed entry types one application writes (and an unapply reverses).
fn is_application_part(ty: &str) -> bool {
    matches!(ty, "Application" | "Realized Gain" | "Realized Loss" | "Unrealized Gain" | "Unrealized Loss" | "Payment Discount")
}

/// The posting group account a payment discount of `disc` (sign of the entry) goes to.
fn discount_account(ctx: &Ctx, side: &Side, entry: &Record, disc: Decimal) -> Result<String> {
    let (table, debit) = if side.sales { ("Customer Posting Group", disc > Decimal::ZERO) } else { ("Vendor Posting Group", disc > Decimal::ZERO) };
    let field = if debit { "Payment Disc. Debit Acc." } else { "Payment Disc. Credit Acc." };
    let group = entry.str(side.posting_group);
    let g = ctx.get(table, &erpcraft_meta::Key(vec![Value::text(group)]))?;
    match g.str(field) {
        "" => refuse!("{field} must have a value in {table}: Code={group}. It cannot be zero or empty."),
        a => Ok(a.to_string()),
    }
}

/// The entry's remaining LCY (sum of its detailed entries' Amount (LCY)).
fn remaining_lcy(ctx: &Ctx, side: &Side, entry_no: i64) -> Decimal {
    ctx.db.rows(dtld_table(side)).filter(|d| d.get(entry_field(side)).as_int() == entry_no).map(|d| d.get("Amount (LCY)").as_dec()).sum()
}

/// The entry's open unrealized gain / loss (Adjust Exchange Rates not yet reversed).
fn unrealized(ctx: &Ctx, side: &Side, entry_no: i64) -> Decimal {
    ctx.db
        .rows(dtld_table(side))
        .filter(|d| d.get(entry_field(side)).as_int() == entry_no && d.str("Entry Type").starts_with("Unrealized"))
        .map(|d| d.get("Amount (LCY)").as_dec())
        .sum()
}

/// `fcy` of `entry` at the entry's own (original) rate.
fn at_original_rate(entry: &Record, fcy: Decimal) -> Decimal {
    let (amount, lcy) = (entry.get("Amount").as_dec(), entry.get("Amount (LCY)").as_dec());
    if amount.is_zero() { fcy } else { crate::ctx::round_amount(fcy * lcy / amount) }
}

/// Applies the open entry `applying` to the open entries `targets` (same party, opposite sign),
/// oldest first as given, until `applying` is used up. Writes "Application" detailed entries and
/// updates remaining amounts. Returns (target document no., amount applied).
///
/// In a foreign currency (BC's CalcCurrencyRealizedGainLoss) both sides apply at the applying
/// entry's rate; the target's difference to its own rate is a realized gain or loss, and the
/// applied share of an earlier unrealized adjustment is reversed first. Both go to the G/L.
pub fn apply(ctx: &mut Ctx, side: &Side, applying: i64, targets: &[i64], posting_date: NaiveDate) -> Result<Vec<(String, Decimal)>> {
    let lt = ctx.table(side.ledger)?;
    let mut a = ledger_entry(ctx, side, applying)?;
    if !a.get("Open").as_bool() {
        refuse!("{} Ledger Entry {applying} is not open, so it cannot be applied.", side.party_caption);
    }
    let txn = Txn::new(ctx, "APPLICATION", "", "");
    let application_no = ctx.db.rows(dtld_table(side)).map(|d| d.get("Application No.").as_int()).max().unwrap_or(0).saturating_add(1);
    let party = a.str(side.ledger_party).to_string();
    let currency = a.str("Currency Code").to_string();
    let mut applied = Vec::new();
    for &tno in targets {
        let left = a.get("Remaining Amount").as_dec();
        if left.is_zero() {
            break;
        }
        let mut t = ledger_entry(ctx, side, tno)?;
        if t.str(side.ledger_party) != party {
            refuse!("{} Ledger Entry {tno} belongs to another {}.", side.party_caption, side.party_caption.to_lowercase());
        }
        if t.str("Currency Code") != currency {
            refuse!("All entries in one application must be in the same currency.");
        }
        let rem = t.get("Remaining Amount").as_dec();
        if !t.get("Open").as_bool() || rem.is_zero() || rem.is_sign_positive() == left.is_sign_positive() {
            refuse!("{} Ledger Entry {tno} cannot be applied: it must be open and have the opposite sign.", side.party_caption);
        }
        // BC's payment discount: paid by the discount date and covering the rest, the discount
        // closes the entry.
        let disc = t.get("Remaining Pmt. Disc. Possible").as_dec();
        let disc_ok = currency.is_empty()
            && !disc.is_zero()
            && t.get("Pmt. Discount Date").as_date().is_some_and(|d| posting_date <= d)
            && left.abs() >= (rem - disc).abs();
        let disc = if disc_ok { disc } else { Decimal::ZERO };
        let take = left.abs().min((rem - disc).abs());
        // Each entry moves toward zero by `take`.
        let on_target = if rem.is_sign_positive() { -take } else { take };
        let on_applying = -on_target;
        let (doc_type, doc_no) = (a.str("Document Type").to_string(), a.str("Document No.").to_string());
        if currency.is_empty() {
            detailed(ctx, side, txn.no, tno, "Application", posting_date, &doc_type, &doc_no, &party, (on_target, on_target), applying, application_no)?;
            detailed(ctx, side, txn.no, applying, "Application", posting_date, &doc_type, &doc_no, &party, (on_applying, on_applying), tno, application_no)?;
            if !disc.is_zero() {
                detailed(ctx, side, txn.no, tno, "Payment Discount", posting_date, &doc_type, &doc_no, &party, (-disc, -disc), applying, application_no)?;
                let party_acc = crate::currency::party_account(ctx, side, &t)?;
                let acc = discount_account(ctx, side, &t, disc)?;
                crate::currency::post_gain_loss(ctx, &crate::currency::GainLossEntry { txn: &txn, date: posting_date, doc_no: &doc_no, party_account: &party_acc, other: &acc, amount: -disc, description: "Payment Discount" })?;
            }
        } else {
            // Reverse the applied share of each side's unrealized adjustment.
            for (entry, on_x, before) in [(&t, on_target, rem), (&a, on_applying, left)] {
                let open = unrealized(ctx, side, entry.get("Entry No.").as_int());
                let share = if before.is_zero() { Decimal::ZERO } else { crate::ctx::round_amount(open * on_x.abs() / before.abs()) };
                if !share.is_zero() {
                    let gain = open.is_sign_positive();
                    let ty = if gain { "Unrealized Gain" } else { "Unrealized Loss" };
                    let no = entry.get("Entry No.").as_int();
                    detailed(ctx, side, txn.no, no, ty, posting_date, &doc_type, &doc_no, &party, (Decimal::ZERO, -share), 0, application_no)?;
                    let party_acc = crate::currency::party_account(ctx, side, entry)?;
                    let acc = crate::currency::gain_loss_account(ctx, &currency, false, gain)?;
                    let desc = format!("Reversal of {ty}");
                    crate::currency::post_gain_loss(ctx, &crate::currency::GainLossEntry { txn: &txn, date: posting_date, doc_no: &doc_no, party_account: &party_acc, other: &acc, amount: -share, description: &desc })?;
                }
            }
            // The applying side at its own rate (all that is left when it closes).
            let lcy_new = if (left + on_applying).is_zero() { -remaining_lcy(ctx, side, applying) } else { at_original_rate(&a, on_applying) };
            detailed(ctx, side, txn.no, applying, "Application", posting_date, &doc_type, &doc_no, &party, (on_applying, lcy_new), tno, application_no)?;
            detailed(ctx, side, txn.no, tno, "Application", posting_date, &doc_type, &doc_no, &party, (on_target, -lcy_new), applying, application_no)?;
            // What the target is worth at its own rate, against what it was applied at.
            // A target that closes keeps nothing in LCY; a part keeps its own rate.
            let realized = if (rem + on_target).is_zero() { -remaining_lcy(ctx, side, tno) } else { at_original_rate(&t, on_target) + lcy_new };
            if !realized.is_zero() {
                let gain = realized.is_sign_positive();
                let ty = if gain { "Realized Gain" } else { "Realized Loss" };
                detailed(ctx, side, txn.no, tno, ty, posting_date, &doc_type, &doc_no, &party, (Decimal::ZERO, realized), 0, application_no)?;
                let party_acc = crate::currency::party_account(ctx, side, &t)?;
                let acc = crate::currency::gain_loss_account(ctx, &currency, true, gain)?;
                crate::currency::post_gain_loss(ctx, &crate::currency::GainLossEntry { txn: &txn, date: posting_date, doc_no: &doc_no, party_account: &party_acc, other: &acc, amount: realized, description: ty })?;
            }
        }
        t.set("Remaining Amount", dec(rem + on_target - disc));
        if !disc.is_zero() {
            t.set("Remaining Pmt. Disc. Possible", dec(Decimal::ZERO));
        }
        if (rem + on_target - disc).is_zero() {
            t.set("Open", Value::Bool(false));
            t.set("Closed by Entry No.", Value::Int(applying));
        }
        ctx.db.modify(lt, t.clone())?;
        a.set("Remaining Amount", dec(left + on_applying));
        if (left + on_applying).is_zero() {
            a.set("Open", Value::Bool(false));
            a.set("Closed by Entry No.", Value::Int(tno));
        }
        applied.push((t.str("Document No.").to_string(), take));
    }
    ctx.db.modify(lt, a)?;
    Ok(applied)
}

/// Undoes the latest application of `entry_no`: reverses its "Application" detailed entries and
/// reopens every entry involved.
pub fn unapply(ctx: &mut Ctx, side: &Side, entry_no: i64) -> Result<Json> {
    let dt = ctx.table(dtld_table(side))?;
    let lt = ctx.table(side.ledger)?;
    let app_no = ctx
        .db
        .rows(&dt.name)
        .filter(|d| d.get(entry_field(side)).as_int() == entry_no && d.str("Entry Type") == "Application" && !d.get("Unapplied").as_bool())
        .map(|d| d.get("Application No.").as_int())
        .max()
        .ok_or_else(|| EngineError::refused(format!("{} Ledger Entry {entry_no} has no application to undo.", side.party_caption)))?;
    let originals: Vec<Record> = ctx
        .db
        .rows(&dt.name)
        .filter(|d| d.get("Application No.").as_int() == app_no && is_application_part(d.str("Entry Type")) && !d.get("Unapplied").as_bool())
        .cloned()
        .collect();
    let txn = Txn::new(ctx, "UNAPPLY", "", "");
    let mut reopened = Vec::new();
    for o in originals {
        let le = o.get(entry_field(side)).as_int();
        let amount = o.get("Amount").as_dec();
        let amount_lcy = o.get("Amount (LCY)").as_dec();
        let new_no = detailed(
            ctx,
            side,
            txn.no,
            le,
            o.str("Entry Type"),
            ctx.work_date,
            o.str("Document Type"),
            o.str("Document No."),
            o.str(side.ledger_party),
            (-amount, -amount_lcy),
            o.get(applied_field(side)).as_int(),
            app_no,
        )?;
        // A gain or loss of the application goes back off the G/L too.
        let ty = o.str("Entry Type");
        if ty != "Application" && !amount_lcy.is_zero() {
            let entry = ledger_entry(ctx, side, le)?;
            let party_acc = crate::currency::party_account(ctx, side, &entry)?;
            let acc = if ty == "Payment Discount" {
                discount_account(ctx, side, &entry, -amount)?
            } else {
                crate::currency::gain_loss_account(ctx, entry.str("Currency Code"), ty.starts_with("Realized"), ty.ends_with("Gain"))?
            };
            let desc = format!("Unapply {ty}");
            crate::currency::post_gain_loss(ctx, &crate::currency::GainLossEntry { txn: &txn, date: ctx.work_date, doc_no: o.str("Document No."), party_account: &party_acc, other: &acc, amount: -amount_lcy, description: &desc })?;
        }
        let mut undo = ctx.get(&dt.name, &erpcraft_meta::Key(vec![Value::Int(new_no)]))?;
        undo.set("Unapplied", Value::Bool(true));
        undo.set("Unapplied by Entry No.", Value::Int(o.get("Entry No.").as_int()));
        ctx.db.modify(dt, undo)?;
        let mut orig = o.clone();
        orig.set("Unapplied", Value::Bool(true));
        orig.set("Unapplied by Entry No.", Value::Int(new_no));
        ctx.db.modify(dt, orig)?;
        let mut e = ledger_entry(ctx, side, le)?;
        let rem = e.get("Remaining Amount").as_dec() - if matches!(ty, "Application" | "Payment Discount") { amount } else { Decimal::ZERO };
        if ty == "Payment Discount" {
            e.set("Remaining Pmt. Disc. Possible", dec(-amount));
        }
        e.set("Remaining Amount", dec(rem));
        e.set("Open", Value::Bool(!rem.is_zero()));
        e.set("Closed by Entry No.", Value::Int(0));
        ctx.db.modify(lt, e)?;
        if !reopened.contains(&le) {
            reopened.push(le);
        }
    }
    Ok(json!({"reopened_entries": reopened, "message": format!("The entries were successfully unapplied ({} entries reopened).", reopened.len())}))
}

/// Open entries of `party` with the opposite sign of `amount`, oldest due date first, optionally
/// only those of one document.
pub fn open_entries_against(ctx: &Ctx, side: &Side, party: &str, amount: Decimal, doc_no: Option<&str>) -> Vec<i64> {
    let mut v: Vec<&Record> = ctx
        .db
        .rows(side.ledger)
        .filter(|e| {
            e.str(side.ledger_party) == party
                && e.get("Open").as_bool()
                && !e.get("Remaining Amount").as_dec().is_zero()
                && e.get("Remaining Amount").as_dec().is_sign_positive() != amount.is_sign_positive()
                && doc_no.is_none_or(|d| e.str("Document No.") == d)
        })
        .collect();
    v.sort_by(|a, b| (a.get("Due Date"), a.get("Entry No.")).cmp(&(b.get("Due Date"), b.get("Entry No."))));
    v.iter().map(|e| e.get("Entry No.").as_int()).collect()
}

/// A bank account ledger entry.
#[allow(clippy::too_many_arguments)]
pub fn post_bank_entry(
    ctx: &mut Ctx,
    txn: &Txn,
    bank: &str,
    posting_date: NaiveDate,
    doc_type: &str,
    doc_no: &str,
    description: &str,
    amount: Decimal,
    bal_type: &str,
    bal_no: &str,
) -> Result<i64> {
    let t = ctx.table(BANK_LEDGER)?;
    let no = ctx.db.next_entry_no(BANK_LEDGER);
    let mut e = t.init_record();
    e.set("Entry No.", Value::Int(no));
    e.set("Bank Account No.", Value::text(bank));
    e.set("Posting Date", Value::Date(posting_date));
    e.set("Document Type", Value::text(doc_type));
    e.set("Document No.", Value::text(doc_no));
    e.set("Description", Value::text(description));
    e.set("Amount", dec(amount));
    e.set("Remaining Amount", dec(amount));
    e.set("Bal. Account Type", Value::text(bal_type));
    e.set("Bal. Account No.", Value::text(bal_no));
    e.set("Open", Value::Bool(true));
    e.set("Statement Status", Value::text("Open"));
    e.set("Transaction No.", Value::Int(txn.no));
    e.set("Source Code", Value::text(&txn.source_code));
    e.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, e)?;
    Ok(no)
}

/// The G/L account behind a bank account (its Bank Account Posting Group).
pub fn bank_gl_account(ctx: &Ctx, bank: &str) -> Result<String> {
    let b = ctx.get_code(crate::schema::tables::BANK_ACCOUNT, bank)?;
    if b.get("Blocked").as_bool() {
        refuse!("Blocked must be equal to 'No' in Bank Account: No.={bank}. Current value is 'Yes'.");
    }
    ctx.db
        .get_code(crate::schema::tables::BANK_POSTING_GROUP, b.str("Bank Acc. Posting Group"))
        .map(|r| r.str("G/L Account No.").to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::refused(format!("Bank Account {bank} has no Bank Acc. Posting Group with a G/L Account No.")))
}

/// The receivables / payables G/L account of a customer / vendor (its posting group).
pub fn party_gl_account(ctx: &Ctx, side: &Side, party_no: &str) -> Result<(String, String)> {
    let p = ctx.get_code(side.party_table, party_no)?;
    let group = p.str(side.posting_group).to_string();
    let (table, field) = if side.sales {
        (crate::schema::tables::CUST_POSTING_GROUP, "Receivables Account")
    } else {
        (crate::schema::tables::VEND_POSTING_GROUP, "Payables Account")
    };
    let acc = ctx
        .db
        .get_code(table, &group)
        .map(|r| r.str(field).to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::refused(format!("{} must have a value in {}: No.={party_no}.", side.posting_group, side.party_caption)))?;
    Ok((acc, group))
}

/// An entry's Dimension Set ID and Global Dimension 1 / 2 Code from `set`.
pub fn stamp_dims(ctx: &Ctx, t: &TableDef, r: &mut Record, set: i64) {
    if set == 0 || t.field("Dimension Set ID").is_none() {
        return;
    }
    r.set("Dimension Set ID", Value::Int(set));
    let (d1, d2) = crate::dimensions::globals(ctx, set);
    r.set("Global Dimension 1 Code", Value::text(d1));
    r.set("Global Dimension 2 Code", Value::text(d2));
}
