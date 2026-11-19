//! Fixed assets: card defaults, depreciation books, FA ledger entries, and the G/L effect of
//! acquisition, depreciation, write-down and disposal (BC FA Jnl.-Post Line with G/L
//! integration). Depreciation is straight-line over BC's 360-day year
//! (`DepreciationCalculation.DeprDays`).
//!
//! Acquisition comes from a purchase invoice line of type Fixed Asset, or from a FA G/L Journal
//! line whose balancing account says how the asset was obtained: a vendor or bank (purchase), a
//! grant account (hibah), or equity (opening balance).

use chrono::{Datelike, Days, Months, NaiveDate};
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::ledger::Txn;
use crate::posting::GlLine;
use crate::refuse;
use crate::schema::finance::GEN_JNL_LINE;
use crate::schema::fixed_assets::{DEPRECIATION_BOOK, FA_DEPR_BOOK, FA_LEDGER, FA_POSTING_GROUP, FA_SETUP, FA_SUBCLASS, FIXED_ASSET};
use crate::schema::tables::GL_ACCOUNT;

/// Book value types: these entries make up the asset's book value.
const BOOK_VALUE_TYPES: &[&str] = &["Acquisition Cost", "Depreciation", "Write-Down", "Appreciation", "Book Value on Disposal"];

pub fn default_book(ctx: &Ctx) -> Result<String> {
    let b = ctx.setup(FA_SETUP)?.str("Default Depr. Book").to_string();
    if b.is_empty() {
        refuse!("You must specify a Default Depr. Book on the Fixed Asset Setup page.");
    }
    Ok(b)
}

fn book_key(fa: &str, book: &str) -> Key {
    Key(vec![Value::text(fa), Value::text(book)])
}

/// `OnInsert` of a fixed asset: number from FA Setup › Fixed Asset Nos., and a depreciation book
/// row for the default book, as the FA card shows one.
pub fn on_insert(ctx: &mut Ctx, r: &mut Record) -> Result<()> {
    if r.str("No.").is_empty() {
        let series = ctx.setup(FA_SETUP)?.str("Fixed Asset Nos.").to_string();
        if series.is_empty() {
            refuse!("You must specify a number series in the Fixed Asset Nos. field on the Fixed Asset Setup page.");
        }
        let no = crate::noseries::next(ctx, &series)?;
        r.set("No.", Value::text(no));
    }
    r.set("Search Description", Value::text(r.str("Description").to_uppercase()));
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    let book = ctx.setup(FA_SETUP)?.str("Default Depr. Book").to_string();
    if !book.is_empty() && ctx.db.get(FA_DEPR_BOOK, &book_key(r.str("No."), &book)).is_none() {
        let t = ctx.table(FA_DEPR_BOOK)?;
        let mut b = t.init_record();
        b.set("FA No.", r.get("No.").clone());
        b.set("Depreciation Book Code", Value::text(&book));
        b.set("FA Posting Group", r.get("FA Posting Group").clone());
        ctx.db.insert(t, b)?;
    }
    Ok(())
}

pub fn on_validate_fa(ctx: &mut Ctx, r: &mut Record, field: &str) -> Result<()> {
    match field {
        "Description" => r.set("Search Description", Value::text(r.str("Description").to_uppercase())),
        "FA Subclass Code" => {
            let code = r.str("FA Subclass Code").to_string();
            if code.is_empty() {
                return Ok(());
            }
            let sub = ctx.get_code(FA_SUBCLASS, &code)?;
            if !sub.str("FA Class Code").is_empty() {
                r.set("FA Class Code", sub.get("FA Class Code").clone());
            }
            if !sub.str("Default FA Posting Group").is_empty() {
                r.set("FA Posting Group", sub.get("Default FA Posting Group").clone());
                set_book_posting_group(ctx, r.str("No."), sub.str("Default FA Posting Group"))?;
            }
        }
        "FA Posting Group" => set_book_posting_group(ctx, r.str("No."), &r.str("FA Posting Group").to_string())?,
        _ => {}
    }
    Ok(())
}

fn set_book_posting_group(ctx: &mut Ctx, fa: &str, group: &str) -> Result<()> {
    let t = ctx.table(FA_DEPR_BOOK)?;
    let rows: Vec<Record> = ctx.db.rows(FA_DEPR_BOOK).filter(|b| b.str("FA No.") == fa).cloned().collect();
    for mut b in rows {
        b.set("FA Posting Group", Value::text(group));
        ctx.db.modify(t, b)?;
    }
    Ok(())
}

/// Depreciation book: years ↔ ending date (BC: ending = starting + years − 1 day).
pub fn on_validate_book(_ctx: &mut Ctx, r: &mut Record, field: &str) -> Result<()> {
    if matches!(field, "No. of Depreciation Years" | "Depreciation Starting Date") {
        let years = r.get("No. of Depreciation Years").as_dec();
        if years < Decimal::ZERO {
            refuse!("No. of Depreciation Years must not be negative.");
        }
        if let Some(start) = r.get("Depreciation Starting Date").as_date()
            && !years.is_zero()
        {
            let months = (years * Decimal::from(12)).round().to_string().parse::<u32>().unwrap_or(0);
            if let Some(end) = start.checked_add_months(Months::new(months)).and_then(|d| d.pred_opt()) {
                r.set("Depreciation Ending Date", Value::Date(end));
            }
        }
    }
    Ok(())
}

pub fn on_delete_fa(ctx: &mut Ctx, r: &Record) -> Result<()> {
    let no = r.str("No.");
    if ctx.db.rows(FA_LEDGER).any(|e| e.str("FA No.") == no) {
        refuse!("You cannot delete Fixed Asset {no} because it has ledger entries.");
    }
    let t = ctx.table(FA_DEPR_BOOK)?;
    let keys: Vec<Key> = ctx.db.rows(FA_DEPR_BOOK).filter(|b| b.str("FA No.") == no).map(|b| b.key(t)).collect();
    for k in keys {
        ctx.db.remove(FA_DEPR_BOOK, &k);
    }
    Ok(())
}

/// FlowFields of Fixed Asset (default book) and FA Depreciation Book.
pub fn calc_fields(db: &crate::db::Database, table: &str, r: &mut Record) {
    let (fa, book) = if table == FIXED_ASSET {
        let book = db.get(FA_SETUP, &Key(vec![Value::text("")])).map(|s| s.str("Default Depr. Book").to_string()).unwrap_or_default();
        (r.str("No.").to_string(), book)
    } else {
        (r.str("FA No.").to_string(), r.str("Depreciation Book Code").to_string())
    };
    let entries: Vec<&Record> = db.rows(FA_LEDGER).filter(|e| e.str("FA No.") == fa && e.str("Depreciation Book Code") == book).collect();
    let sum = |types: &[&str]| -> Decimal { entries.iter().filter(|e| types.contains(&e.str("FA Posting Type"))).map(|e| e.get("Amount").as_dec()).sum() };
    r.set("Acquisition Cost", dec(sum(&["Acquisition Cost"])));
    r.set("Depreciation", dec(sum(&["Depreciation"])));
    r.set("Book Value", dec(sum(BOOK_VALUE_TYPES)));
    if table == FA_DEPR_BOOK {
        r.set("Proceeds on Disposal", dec(sum(&["Proceeds on Disposal"])));
        r.set("Gain/Loss", dec(sum(&["Gain/Loss"])));
        let last = entries.iter().filter(|e| e.str("FA Posting Type") == "Depreciation").filter_map(|e| e.get("FA Posting Date").as_date()).max();
        r.set("Last Depreciation Date", last.map_or(Value::Null, Value::Date));
    }
}

/// An FA ledger entry to write once the G/L entries of the transaction exist.
pub struct FaEntrySpec {
    pub fa_no: String,
    pub book: String,
    pub posting_type: &'static str,
    pub amount: Decimal,
    pub description: String,
    pub depreciation_days: i64,
    pub part_of_book_value: bool,
}

pub fn post_entry(ctx: &mut Ctx, txn: &Txn, s: &FaEntrySpec, date: NaiveDate, doc_type: &str, doc_no: &str, ext_doc_no: &str) -> Result<i64> {
    let t = ctx.table(FA_LEDGER)?;
    let fa = ctx.get_code(FIXED_ASSET, &s.fa_no)?;
    let book = ctx.get(FA_DEPR_BOOK, &book_key(&s.fa_no, &s.book))?;
    let no = ctx.db.next_entry_no(FA_LEDGER);
    let mut e = t.init_record();
    e.set("Entry No.", Value::Int(no));
    e.set("FA No.", Value::text(&s.fa_no));
    e.set("Depreciation Book Code", Value::text(&s.book));
    e.set("FA Posting Date", Value::Date(date));
    e.set("Posting Date", Value::Date(date));
    e.set("Document Type", Value::text(doc_type));
    e.set("Document No.", Value::text(doc_no));
    e.set("External Document No.", Value::text(ext_doc_no));
    e.set("Description", Value::text(&s.description));
    e.set("FA Posting Type", Value::text(s.posting_type));
    e.set("Amount", dec(s.amount));
    e.set("Debit Amount", dec(s.amount.max(Decimal::ZERO)));
    e.set("Credit Amount", dec((-s.amount).max(Decimal::ZERO)));
    e.set("No. of Depreciation Days", Value::Int(s.depreciation_days));
    e.set("FA Posting Group", book.get("FA Posting Group").clone());
    e.set("FA Location Code", fa.get("FA Location Code").clone());
    e.set("Part of Book Value", Value::Bool(s.part_of_book_value));
    e.set("Transaction No.", Value::Int(txn.no));
    e.set("Source Code", Value::text(&txn.source_code));
    e.set("Journal Batch Name", Value::text(&txn.batch));
    e.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, e)?;
    // The card and the book remember the acquisition / disposal.
    if s.posting_type == "Acquisition Cost" {
        let ft = ctx.table(FIXED_ASSET)?;
        let mut f = fa.clone();
        f.set("Acquired", Value::Bool(true));
        ctx.db.modify(ft, f)?;
        if book.get("Acquisition Date").as_date().is_none() {
            let bt = ctx.table(FA_DEPR_BOOK)?;
            let mut b = book.clone();
            b.set("Acquisition Date", Value::Date(date));
            if b.get("Depreciation Starting Date").as_date().is_none() {
                b.set("Depreciation Starting Date", Value::Date(date));
                on_validate_book(ctx, &mut b, "Depreciation Starting Date")?;
            }
            ctx.db.modify(bt, b)?;
        }
    }
    if s.posting_type == "Book Value on Disposal" {
        let bt = ctx.table(FA_DEPR_BOOK)?;
        let mut b = ctx.get(FA_DEPR_BOOK, &book_key(&s.fa_no, &s.book))?;
        b.set("Disposal Date", Value::Date(date));
        ctx.db.modify(bt, b)?;
    }
    Ok(no)
}

struct Posting {
    acquisition: String,
    accum_depr: String,
    write_down: String,
    gains: String,
    losses: String,
}

/// The posting group accounts and book of `fa_no`, refusing a blocked, inactive or disposed asset.
fn posting_of(ctx: &Ctx, fa_no: &str, book: &str) -> Result<(Record, Posting)> {
    let fa = ctx.get_code(FIXED_ASSET, fa_no)?;
    if fa.get("Blocked").as_bool() || fa.get("Inactive").as_bool() {
        refuse!("Fixed Asset {fa_no} is {}.", if fa.get("Blocked").as_bool() { "blocked" } else { "inactive" });
    }
    let b = ctx
        .db
        .get(FA_DEPR_BOOK, &book_key(fa_no, book))
        .cloned()
        .ok_or_else(|| EngineError::refused(format!("Fixed Asset {fa_no} has no depreciation book {book}.")))?;
    if b.get("Disposal Date").as_date().is_some() {
        refuse!("Fixed Asset {fa_no} has been disposed of; nothing more can be posted to it.");
    }
    let dbk = ctx.get_code(DEPRECIATION_BOOK, book)?;
    if !dbk.get("G/L Integration - Acq. Cost").as_bool() || !dbk.get("G/L Integration - Depreciation").as_bool() {
        refuse!("Depreciation book {book} is not integrated with the general ledger. ERPCraft posts fixed assets through the G/L only.");
    }
    let group = b.str("FA Posting Group").to_string();
    if group.is_empty() {
        refuse!("FA Posting Group must have a value in FA Depreciation Book: FA No.='{fa_no}',Depreciation Book Code='{book}'.");
    }
    let g = ctx.get_code(FA_POSTING_GROUP, &group)?;
    let acc = |f: &str| g.str(f).to_string();
    Ok((
        b,
        Posting {
            acquisition: acc("Acquisition Cost Account"),
            accum_depr: acc("Accum. Depreciation Account"),
            write_down: if acc("Write-Down Account").is_empty() { acc("Accum. Depreciation Account") } else { acc("Write-Down Account") },
            gains: acc("Gains Acc. on Disposal"),
            losses: acc("Losses Acc. on Disposal"),
        },
    ))
}

fn values(ctx: &Ctx, fa_no: &str, book: &str) -> (Decimal, Decimal, Decimal) {
    let mut r = Record::default();
    r.set("FA No.", Value::text(fa_no));
    r.set("Depreciation Book Code", Value::text(book));
    calc_fields(ctx.db, FA_DEPR_BOOK, &mut r);
    (r.get("Acquisition Cost").as_dec(), r.get("Depreciation").as_dec(), r.get("Book Value").as_dec())
}

/// Purchase invoice line of type Fixed Asset: the acquisition cost account and the FA entry.
pub fn acquisition_from_document(ctx: &Ctx, fa_no: &str, amount: Decimal, _date: NaiveDate, description: &str) -> Result<(String, FaEntrySpec)> {
    let book = default_book(ctx)?;
    let (_, p) = posting_of(ctx, fa_no, &book)?;
    Ok((
        p.acquisition,
        FaEntrySpec {
            fa_no: fa_no.to_string(),
            book,
            posting_type: "Acquisition Cost",
            amount,
            description: description.to_string(),
            depreciation_days: 0,
            part_of_book_value: true,
        },
    ))
}

/// The G/L lines and FA entries of a journal line's Fixed Asset side (`amount` signed as on the
/// line: acquisition positive, depreciation negative, disposal = −proceeds).
pub fn journal_effect(ctx: &Ctx, line: &Record, fa_no: &str, amount: Decimal) -> Result<(Vec<GlLine>, Vec<FaEntrySpec>)> {
    let book = match line.str("Depreciation Book Code") {
        "" => default_book(ctx)?,
        b => b.to_string(),
    };
    let (_, p) = posting_of(ctx, fa_no, &book)?;
    let desc = line.str("Description").to_string();
    let gl = |account: &str, amount: Decimal| GlLine { account: account.to_string(), amount, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" };
    let spec = |t: &'static str, amount: Decimal, bv: bool| FaEntrySpec {
        fa_no: fa_no.to_string(),
        book: book.clone(),
        posting_type: t,
        amount,
        description: desc.clone(),
        depreciation_days: line.get("No. of Depreciation Days").as_int(),
        part_of_book_value: bv,
    };
    let (acq, depr, bv) = values(ctx, fa_no, &book);
    match line.str("FA Posting Type") {
        "Acquisition Cost" => Ok((vec![gl(&p.acquisition, amount)], vec![spec("Acquisition Cost", amount, true)])),
        "Depreciation" => {
            if acq.is_zero() {
                refuse!("Fixed Asset {fa_no} has not been acquired, so it cannot be depreciated.");
            }
            if bv + amount < Decimal::ZERO {
                refuse!("The book value of Fixed Asset {fa_no} would become negative ({}).", bv + amount);
            }
            Ok((vec![gl(&p.accum_depr, amount)], vec![spec("Depreciation", amount, true)]))
        }
        "Write-Down" => Ok((vec![gl(&p.write_down, amount)], vec![spec("Write-Down", amount, true)])),
        "Disposal" => {
            if acq.is_zero() {
                refuse!("Fixed Asset {fa_no} has not been acquired, so it cannot be disposed of.");
            }
            let proceeds = -amount;
            let gain = proceeds - bv;
            let mut lines = vec![gl(&p.acquisition, -acq), gl(&p.accum_depr, -(bv - acq))];
            if gain > Decimal::ZERO {
                lines.push(gl(&p.gains, -gain));
            } else if gain < Decimal::ZERO {
                lines.push(gl(&p.losses, -gain));
            }
            let _ = depr;
            Ok((
                lines,
                vec![spec("Proceeds on Disposal", -proceeds, false), spec("Gain/Loss", -gain, false), spec("Book Value on Disposal", -bv, true)],
            ))
        }
        "" => refuse!("FA Posting Type must have a value on the journal line for Fixed Asset {fa_no}."),
        other => refuse!("FA Posting Type {other} is not supported yet."),
    }
}

/// BC `DeprDays` on a 360-day year, both dates inclusive.
pub fn depr_days(start: NaiveDate, end: NaiveDate) -> i64 {
    if end < start {
        return 0;
    }
    let sd = if start.day() == 31 { 30 } else { i64::from(start.day()) };
    let ed = if end.succ_opt().is_some_and(|n| n.day() == 1) { 30 } else { i64::from(end.day()) };
    1 + ed - sd + 30 * (i64::from(end.month()) - i64::from(start.month())) + 360 * (i64::from(end.year()) - i64::from(start.year()))
}

/// BC `ToMorrow` on a 360-day year: the next day, skipping the 31st.
fn tomorrow(d: NaiveDate) -> Option<NaiveDate> {
    let n = d.checked_add_days(Days::new(1))?;
    if n.day() == 31 { n.checked_add_days(Days::new(1)) } else { Some(n) }
}

/// Calculate Depreciation: straight-line depreciation of every acquired asset up to `date`, as
/// lines in the FA G/L Journal (template ASSETS), balanced against each posting group's
/// Depreciation Expense Acc.; posted at once when `post` is set.
pub fn calculate_depreciation(ctx: &mut Ctx, date: NaiveDate, post: bool, only: Option<&str>) -> Result<Json> {
    let book = default_book(ctx)?;
    let lt = ctx.table(GEN_JNL_LINE)?;
    let books: Vec<Record> = ctx
        .db
        .rows(FA_DEPR_BOOK)
        .filter(|b| b.str("Depreciation Book Code") == book && only.is_none_or(|f| b.str("FA No.") == f))
        .cloned()
        .collect();
    let mut created = 0;
    let mut total = Decimal::ZERO;
    for b in books {
        let mut b = b;
        calc_fields(ctx.db, FA_DEPR_BOOK, &mut b);
        let fa_no = b.str("FA No.").to_string();
        let fa = ctx.get_code(FIXED_ASSET, &fa_no)?;
        let acq = b.get("Acquisition Cost").as_dec();
        let years = b.get("No. of Depreciation Years").as_dec();
        if fa.get("Blocked").as_bool() || fa.get("Inactive").as_bool() || acq.is_zero() || years.is_zero() || b.get("Disposal Date").as_date().is_some() {
            continue;
        }
        if b.str("Depreciation Method") != "Straight-Line" {
            continue;
        }
        let Some(starting) = b.get("Depreciation Starting Date").as_date() else { continue };
        let start = match b.get("Last Depreciation Date").as_date() {
            Some(last) => match tomorrow(last) {
                Some(d) => d.max(starting),
                None => continue,
            },
            None => starting,
        };
        let end = b.get("Depreciation Ending Date").as_date().map_or(date, |e| e.min(date));
        let days = depr_days(start, end);
        if days <= 0 {
            continue;
        }
        let bv = b.get("Book Value").as_dec();
        let per_day = acq / (years * Decimal::from(360));
        let mut amount = round_amount(per_day * Decimal::from(days));
        // The last period takes what is left, so the asset ends at zero.
        if end == b.get("Depreciation Ending Date").as_date().unwrap_or(NaiveDate::MAX) || amount > bv {
            amount = bv;
        }
        if amount <= Decimal::ZERO {
            continue;
        }
        let group = ctx.get_code(FA_POSTING_GROUP, b.str("FA Posting Group"))?;
        let expense = group.str("Depreciation Expense Acc.").to_string();
        if expense.is_empty() || ctx.db.get_code(GL_ACCOUNT, &expense).is_none() {
            refuse!("Depreciation Expense Acc. must have a value in FA Posting Group: Code='{}'.", group.str("Code"));
        }
        let mut v = Map::new();
        v.insert("Journal Template Name".into(), json!("ASSETS"));
        v.insert("Journal Batch Name".into(), json!("DEFAULT"));
        let mut r = crate::records::create(ctx, lt, &v, None)?;
        r.set("Posting Date", Value::Date(end));
        r.set("Document Type", Value::text(""));
        r.set("Account Type", Value::text("Fixed Asset"));
        r.set("Account No.", Value::text(&fa_no));
        r.set("FA Posting Type", Value::text("Depreciation"));
        r.set("Depreciation Book Code", Value::text(&book));
        r.set("Description", Value::text(format!("Depreciation {} {}", fa.str("Description"), end.format("%b %Y"))));
        r.set("Amount", dec(-amount));
        r.set("Debit Amount", dec(Decimal::ZERO));
        r.set("Credit Amount", dec(amount));
        r.set("Bal. Account Type", Value::text("G/L Account"));
        r.set("Bal. Account No.", Value::text(&expense));
        r.set("No. of Depreciation Days", Value::Int(days));
        ctx.db.modify(lt, r)?;
        created += 1;
        total += amount;
    }
    let mut out = json!({"lines": created, "total": total.to_string(),
        "message": format!("{created} depreciation lines ({total}) were created in the Fixed Asset G/L Journal.")});
    if post && created > 0 {
        let r = crate::journal::post(ctx, "ASSETS", "DEFAULT")?;
        out["posted"] = r;
        out["message"] = json!(format!("Depreciation of {total} was posted for {created} fixed assets."));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn days_on_a_360_day_year() {
        assert_eq!(depr_days(d(2026, 1, 1), d(2026, 1, 31)), 30);
        assert_eq!(depr_days(d(2026, 1, 1), d(2026, 12, 31)), 360);
        assert_eq!(depr_days(d(2026, 2, 1), d(2026, 2, 28)), 30);
        assert_eq!(depr_days(d(2026, 1, 15), d(2026, 2, 14)), 30);
        assert_eq!(tomorrow(d(2026, 1, 30)), Some(d(2026, 2, 1)));
    }
}
