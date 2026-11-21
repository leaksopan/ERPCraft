//! Reminders (BC's Create Reminders / ReminderMake, Issue Reminders), finance charge memos
//! (Create Finance Charge Memos / FinChrgMemoMake, Issue) and the customer statement (report
//! 1316). Issuing posts the additional fee and interest to the customer and the G/L.

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::ledger::{PartyEntry, Txn, post_party_entry};
use crate::refuse;
use crate::schema::reminders::*;
use crate::schema::tables::{CUST_LEDGER, CUSTOMER};
use crate::side::SALES;

fn date_param(p: &Json, k: &str, default: NaiveDate) -> Result<NaiveDate> {
    match opt_str(p, k).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD)."))),
        None => Ok(default),
    }
}

fn formula(f: &str, d: NaiveDate) -> Result<NaiveDate> {
    if f.trim().is_empty() { Ok(d) } else { crate::dateformula::calc(f, d) }
}

/// The customer's open invoices and finance charge memos (in LCY), oldest due first.
fn open_debits(ctx: &Ctx, customer: &str) -> Result<Vec<Record>> {
    let t = ctx.table(CUST_LEDGER)?;
    let mut v: Vec<Record> = ctx
        .db
        .rows(CUST_LEDGER)
        .filter(|e| e.str("Customer No.") == customer && e.get("Open").as_bool() && matches!(e.str("Document Type"), "Invoice" | "Finance Charge Memo" | "Reminder"))
        .cloned()
        .map(|r| crate::commands::read(ctx, t, r))
        .filter(|e| e.get("Remaining Amount").as_dec() > Decimal::ZERO)
        .collect();
    v.sort_by_key(|e| (e.get("Due Date").as_date(), e.get("Entry No.").as_int()));
    Ok(v)
}

fn setup_series(ctx: &mut Ctx, field: &str) -> Result<String> {
    let series = crate::noseries::series_from_setup(ctx, crate::schema::tables::SALES_SETUP, field)?;
    crate::noseries::next(ctx, &series)
}

fn posting_group_account(ctx: &Ctx, group: &str, field: &str) -> Result<String> {
    let g = ctx.get("Customer Posting Group", &Key(vec![Value::text(group)]))?;
    match g.str(field) {
        "" => refuse!("{field} must have a value in Customer Posting Group: Code={group}. It cannot be zero or empty."),
        a => Ok(a.to_string()),
    }
}

/// A new reminder / finance charge memo header for `customer`.
fn new_header(ctx: &mut Ctx, table: &str, series_field: &str, customer: &Record, date: NaiveDate, extra: &[(&str, Value)]) -> Result<Record> {
    let t = ctx.table(table)?;
    let no = setup_series(ctx, series_field)?;
    let mut h = t.init_record();
    h.set("No.", Value::text(&no));
    h.set("Customer No.", customer.get("No.").clone());
    for f in ["Name", "Address", "City", "Post Code", "Country/Region Code", "Currency Code", "Customer Posting Group"] {
        h.set(f, customer.get(f).clone());
    }
    h.set("Posting Date", Value::Date(date));
    h.set("Document Date", Value::Date(date));
    h.set("User ID", Value::text(ctx.user.to_uppercase()));
    for (f, v) in extra {
        h.set(f, v.clone());
    }
    Ok(h)
}

fn add_line(ctx: &mut Ctx, table: &str, doc_field: &str, doc_no: &str, line_no: i64, values: &[(&str, Value)]) -> Result<()> {
    let t = ctx.table(table)?;
    let mut l = t.init_record();
    l.set(doc_field, Value::text(doc_no));
    l.set("Line No.", Value::Int(line_no));
    for (f, v) in values {
        l.set(f, v.clone());
    }
    ctx.db.insert(t, l)?;
    Ok(())
}

fn entry_line_values(e: &Record) -> Vec<(&'static str, Value)> {
    vec![
        ("Type", Value::text("Customer Ledger Entry")),
        ("Entry No.", e.get("Entry No.").clone()),
        ("Posting Date", e.get("Posting Date").clone()),
        ("Document Date", e.get("Document Date").clone()),
        ("Due Date", e.get("Due Date").clone()),
        ("Document Type", e.get("Document Type").clone()),
        ("Document No.", e.get("Document No.").clone()),
        ("Description", e.get("Description").clone()),
        ("Original Amount", e.get("Amount").clone()),
        ("Remaining Amount", e.get("Remaining Amount").clone()),
    ]
}

// ---- Reminders ------------------------------------------------------------------------------------

/// The overdue entries a reminder for `customer` on `date` lists, and its level (0: none).
fn reminder_lines(ctx: &Ctx, customer: &Record, date: NaiveDate) -> Result<(i64, Vec<(Record, i64)>)> {
    let terms_code = customer.str("Reminder Terms Code");
    if terms_code.is_empty() {
        return Ok((0, Vec::new()));
    }
    let terms = ctx.get_code(REMINDER_TERMS, terms_code)?;
    let mut levels: Vec<Record> = ctx.db.rows(REMINDER_LEVEL).filter(|l| l.str("Reminder Terms Code") == terms_code).cloned().collect();
    levels.sort_by_key(|l| l.get("No.").as_int());
    let Some(top) = levels.last().map(|l| l.get("No.").as_int()) else { return Ok((0, Vec::new())) };
    let max = terms.get("Max. No. of Reminders").as_int();
    let mut out = Vec::new();
    for e in open_debits(ctx, customer.str("No."))?.into_iter().filter(|e| e.str("Document Type") != "Reminder") {
        let last = e.get("Last Issued Reminder Level").as_int();
        if max > 0 && last >= max {
            continue;
        }
        let level_no = (last + 1).min(top);
        let Some(level) = levels.iter().find(|l| l.get("No.").as_int() == level_no) else { continue };
        let due = e.get("Due Date").as_date().unwrap_or(date);
        if formula(level.str("Grace Period"), due)? < date {
            out.push((e, level_no));
        }
    }
    let level = out.iter().map(|(_, l)| *l).max().unwrap_or(0);
    let total: Decimal = out.iter().map(|(e, _)| e.get("Remaining Amount").as_dec()).sum();
    if total < terms.get("Minimum Amount (LCY)").as_dec() {
        return Ok((0, Vec::new()));
    }
    Ok((level, out))
}

/// Fills a reminder's lines: one per overdue entry, then the level's additional fee.
fn fill_reminder(ctx: &mut Ctx, h: &mut Record, customer: &Record, date: NaiveDate) -> Result<bool> {
    let (level, lines) = reminder_lines(ctx, customer, date)?;
    if lines.is_empty() {
        return Ok(false);
    }
    let no = h.str("No.").to_string();
    let terms_code = customer.str("Reminder Terms Code").to_string();
    let lvl = ctx.get(REMINDER_LEVEL, &Key(vec![Value::text(&terms_code), Value::Int(level)]))?;
    h.set("Reminder Terms Code", Value::text(&terms_code));
    h.set("Reminder Level", Value::Int(level));
    h.set("Due Date", Value::Date(formula(lvl.str("Due Date Calculation"), date)?));
    h.set("Posting Description", Value::text(format!("Reminder {no}")));
    let mut line_no = 0;
    for (e, l) in &lines {
        line_no += 10000;
        let mut v = entry_line_values(e);
        v.push(("No. of Reminders", Value::Int(*l)));
        add_line(ctx, REMINDER_LINE, "Reminder No.", &no, line_no, &v)?;
    }
    let fee = lvl.get("Additional Fee (LCY)").as_dec();
    if !fee.is_zero() {
        let account = posting_group_account(ctx, customer.str("Customer Posting Group"), "Additional Fee Account")?;
        add_line(
            ctx,
            REMINDER_LINE,
            "Reminder No.",
            &no,
            line_no + 10000,
            &[("Type", Value::text("G/L Account")), ("No.", Value::text(account)), ("Description", Value::text("Additional Fee")), ("Amount", dec(fee))],
        )?;
    }
    Ok(true)
}

/// BC's Create Reminders: a reminder for every customer (or the one given) with overdue entries.
fn create_reminders(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let date = date_param(p, "reminder_date", ctx.work_date)?;
    let only = opt_str(p, "customer_no").map(str::to_string);
    let customers: Vec<Record> =
        ctx.db.rows(CUSTOMER).filter(|c| only.as_ref().is_none_or(|o| c.str("No.") == o) && c.str("Blocked") != "All").cloned().collect();
    let ht = ctx.table(REMINDER_HEADER)?;
    let mut created = Vec::new();
    for c in customers {
        // One open reminder per customer.
        if ctx.db.rows(REMINDER_HEADER).any(|h| h.str("Customer No.") == c.str("No.")) {
            continue;
        }
        if reminder_lines(ctx, &c, date)?.1.is_empty() {
            continue;
        }
        let mut h = new_header(ctx, REMINDER_HEADER, "Reminder Nos.", &c, date, &[])?;
        fill_reminder(ctx, &mut h, &c, date)?;
        created.push(h.str("No.").to_string());
        ctx.db.insert(ht, h)?;
    }
    Ok(json!({"reminders": created, "message": format!("{} reminder(s) were created.", created.len())}))
}

fn doc_of(ctx: &Ctx, p: &Json, table: &str) -> Result<Record> {
    let no = match (opt_str(p, "table"), p.get("key")) {
        (Some(t), Some(k)) if t == table => k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)),
        _ => opt_str(p, "no").map(str::to_string),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    ctx.get_code(table, &no)
}

fn lines_of(ctx: &Ctx, table: &str, doc_field: &str, no: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(table).filter(|l| l.str(doc_field) == no).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

/// BC's Suggest Reminder Lines: the lines again from the customer's entries now.
fn suggest_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut h = doc_of(ctx, p, REMINDER_HEADER)?;
    let no = h.str("No.").to_string();
    let lt = ctx.table(REMINDER_LINE)?;
    for l in lines_of(ctx, REMINDER_LINE, "Reminder No.", &no) {
        ctx.db.remove(&lt.name, &l.key(lt));
    }
    let customer = ctx.get_code(CUSTOMER, h.str("Customer No."))?;
    let date = h.get("Document Date").as_date().unwrap_or(ctx.work_date);
    let found = fill_reminder(ctx, &mut h, &customer, date)?;
    let ht = ctx.table(REMINDER_HEADER)?;
    ctx.db.modify(ht, h)?;
    Ok(json!({"message": if found { "The reminder lines were suggested." } else { "There is nothing overdue to remind of." }}))
}

/// Posts a customer entry of `amount` (fee and / or interest) against the given G/L amounts.
#[allow(clippy::too_many_arguments)]
fn post_charge(ctx: &mut Ctx, source: &str, customer: &str, group: &str, date: NaiveDate, doc_type: &str, doc_no: &str, description: &str, gl: &[(String, Decimal)]) -> Result<()> {
    let total: Decimal = gl.iter().map(|(_, a)| *a).sum();
    if total.is_zero() {
        return Ok(());
    }
    let receivables = posting_group_account(ctx, group, "Receivables Account")?;
    let txn = Txn::new(ctx, source, "", "");
    let mut lines = vec![crate::posting::GlLine { account: receivables, amount: total, vat_amount: Decimal::ZERO, description: description.to_string(), gen_posting_type: "" }];
    lines.extend(gl.iter().map(|(acc, a)| crate::posting::GlLine { account: acc.clone(), amount: -*a, vat_amount: Decimal::ZERO, description: description.to_string(), gen_posting_type: "" }));
    crate::posting::post_gl(ctx, &crate::posting::GlDoc { posting_date: date, doc_type, doc_no, ext_doc_no: "", source_type: "Customer", source_no: customer, txn: &txn }, lines)?;
    post_party_entry(
        ctx,
        &SALES,
        &txn,
        &PartyEntry {
            party_no: customer,
            posting_date: date,
            document_date: Some(date),
            doc_type,
            doc_no,
            ext_doc_no: "",
            description,
            currency: "",
            amount: total,
            amount_lcy: total,
            sales_purch: Decimal::ZERO,
            due_date: Some(date),
            person: "",
            posting_group: group,
        },
    )?;
    Ok(())
}

/// Copies a document and its lines to the issued tables under `issued_no`, then deletes it.
fn file_issued(ctx: &mut Ctx, h: &Record, from: (&str, &str), to: (&str, &str), doc_field: &str, issued_no: &str) -> Result<Vec<Record>> {
    let (fh, fl) = (ctx.table(from.0)?, ctx.table(from.1)?);
    let (th, tl) = (ctx.table(to.0)?, ctx.table(to.1)?);
    let no = h.str("No.").to_string();
    let mut ih = th.init_record();
    crate::posting::copy_common(h, th, &mut ih);
    ih.set("No.", Value::text(issued_no));
    ih.set("Pre-Assigned No.", Value::text(&no));
    ih.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(th, ih)?;
    let lines = lines_of(ctx, from.1, doc_field, &no);
    for l in &lines {
        let mut il = tl.init_record();
        crate::posting::copy_common(l, tl, &mut il);
        il.set(doc_field, Value::text(issued_no));
        ctx.db.insert(tl, il)?;
        ctx.db.remove(&fl.name, &l.key(fl));
    }
    ctx.db.remove(&fh.name, &h.key(fh));
    Ok(lines)
}

fn reminder_entry(ctx: &mut Ctx, ty: &str, no: &str, h: &Record, l: &Record, level: i64, interest: Decimal, posted: bool) -> Result<()> {
    let t = ctx.table(REMINDER_ENTRY)?;
    let mut e = t.init_record();
    e.set("Entry No.", Value::Int(ctx.db.next_entry_no(REMINDER_ENTRY)));
    e.set("No.", Value::text(no));
    e.set("Type", Value::text(ty));
    e.set("Posting Date", h.get("Posting Date").clone());
    e.set("Document Date", h.get("Document Date").clone());
    e.set("Customer Entry No.", l.get("Entry No.").clone());
    e.set("Reminder Level", Value::Int(level));
    e.set("Interest Posted", Value::Bool(posted));
    e.set("Interest Amount", dec(interest));
    e.set("Customer Entry Document Type", l.get("Document Type").clone());
    e.set("Customer Entry Document No.", l.get("Document No.").clone());
    e.set("Remaining Amount", l.get("Remaining Amount").clone());
    e.set("Customer No.", h.get("Customer No.").clone());
    e.set("Due Date", h.get("Due Date").clone());
    e.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, e)?;
    Ok(())
}

fn issue_reminder_rec(ctx: &mut Ctx, h: Record) -> Result<String> {
    let no = h.str("No.").to_string();
    let lines = lines_of(ctx, REMINDER_LINE, "Reminder No.", &no);
    if !lines.iter().any(|l| l.str("Type") == "Customer Ledger Entry") {
        refuse!("There is nothing to issue on reminder {no}.");
    }
    let date = h.get("Posting Date").as_date().unwrap_or(ctx.work_date);
    crate::posting::check_posting_date(ctx, date)?;
    let issued_no = setup_series(ctx, "Issued Reminder Nos.")?;
    let terms = ctx.get_code(REMINDER_TERMS, h.str("Reminder Terms Code"))?;
    if terms.get("Post Additional Fee").as_bool() {
        let gl: Vec<(String, Decimal)> = lines.iter().filter(|l| l.str("Type") == "G/L Account").map(|l| (l.str("No.").to_string(), l.get("Amount").as_dec())).collect();
        post_charge(ctx, "REMINDER", h.str("Customer No."), h.str("Customer Posting Group"), date, "Reminder", &issued_no, h.str("Posting Description"), &gl)?;
    }
    let level = h.get("Reminder Level").as_int();
    let ct = ctx.table(CUST_LEDGER)?;
    for l in lines.iter().filter(|l| l.str("Type") == "Customer Ledger Entry") {
        reminder_entry(ctx, "Reminder", &issued_no, &h, l, level, Decimal::ZERO, false)?;
        let mut e = ctx.get(CUST_LEDGER, &Key(vec![l.get("Entry No.").clone()]))?;
        e.set("Last Issued Reminder Level", Value::Int(l.get("No. of Reminders").as_int()));
        ctx.db.modify(ct, e)?;
    }
    file_issued(ctx, &h, (REMINDER_HEADER, REMINDER_LINE), (ISSUED_REMINDER_HEADER, ISSUED_REMINDER_LINE), "Reminder No.", &issued_no)?;
    Ok(issued_no)
}

fn issue_reminder(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = doc_of(ctx, p, REMINDER_HEADER)?;
    let no = issue_reminder_rec(ctx, h)?;
    Ok(json!({"issued_no": no, "open": {"page": "issued-reminder", "key": no}, "message": format!("Reminder {no} was issued.")}))
}

fn issue_all(ctx: &mut Ctx, _p: &Json) -> Result<Json> {
    let all: Vec<Record> = ctx.db.rows(REMINDER_HEADER).cloned().collect();
    let mut issued = Vec::new();
    for h in all {
        issued.push(issue_reminder_rec(ctx, h)?);
    }
    Ok(json!({"issued": issued, "message": format!("{} reminder(s) were issued.", issued.len())}))
}

// ---- Finance charge memos -------------------------------------------------------------------------

/// BC's FinChrgMemoMake: interest on each overdue entry for the days it is late (Average Daily
/// Balance) or once (Balance Due), and the terms' additional fee.
fn fin_charge_lines(ctx: &Ctx, customer: &Record, date: NaiveDate) -> Result<(Vec<(Record, Decimal)>, Decimal, Record)> {
    let terms = ctx.get_code(FIN_CHARGE_TERMS, customer.str("Fin. Charge Terms Code"))?;
    let rate = terms.get("Interest Rate").as_dec();
    let period = Decimal::from(terms.get("Interest Period (Days)").as_int().max(1));
    let mut out = Vec::new();
    for e in open_debits(ctx, customer.str("No."))?.into_iter().filter(|e| e.str("Document Type") == "Invoice") {
        let due = e.get("Due Date").as_date().unwrap_or(date);
        if formula(terms.str("Grace Period"), due)? >= date {
            continue;
        }
        let rem = e.get("Remaining Amount").as_dec();
        let interest = if terms.str("Interest Calculation Method") == "Balance Due" {
            round_amount(rem * rate / Decimal::ONE_HUNDRED)
        } else {
            let days = Decimal::from((date - due).num_days().max(0));
            round_amount(rem * rate / Decimal::ONE_HUNDRED * days / period)
        };
        out.push((e, interest));
    }
    let total: Decimal = out.iter().map(|(_, i)| *i).sum();
    let fee = terms.get("Additional Fee (LCY)").as_dec();
    if out.is_empty() || total + fee < terms.get("Minimum Amount (LCY)").as_dec() {
        return Ok((Vec::new(), Decimal::ZERO, terms));
    }
    Ok((out, fee, terms))
}

fn create_fin_charges(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let date = date_param(p, "document_date", ctx.work_date)?;
    let only = opt_str(p, "customer_no").map(str::to_string);
    let customers: Vec<Record> = ctx
        .db
        .rows(CUSTOMER)
        .filter(|c| only.as_ref().is_none_or(|o| c.str("No.") == o) && !c.str("Fin. Charge Terms Code").is_empty() && c.str("Blocked") != "All")
        .cloned()
        .collect();
    let ht = ctx.table(FIN_CHARGE_HEADER)?;
    let mut created = Vec::new();
    for c in customers {
        if ctx.db.rows(FIN_CHARGE_HEADER).any(|h| h.str("Customer No.") == c.str("No.")) {
            continue;
        }
        let (lines, fee, terms) = fin_charge_lines(ctx, &c, date)?;
        if lines.is_empty() {
            continue;
        }
        let mut h = new_header(ctx, FIN_CHARGE_HEADER, "Fin. Chrg. Memo Nos.", &c, date, &[("Fin. Charge Terms Code", terms.get("Code").clone())])?;
        let no = h.str("No.").to_string();
        h.set("Due Date", Value::Date(formula(terms.str("Due Date Calculation"), date)?));
        h.set("Posting Description", Value::text(format!("Finance Charge Memo {no}")));
        let mut line_no = 0;
        for (e, interest) in &lines {
            line_no += 10000;
            let mut v = entry_line_values(e);
            v.push(("Amount", dec(*interest)));
            v.push(("Interest Rate", terms.get("Interest Rate").clone()));
            let desc = match terms.str("Line Description") {
                "" => format!("Interest on {} {}", e.str("Document Type"), e.str("Document No.")),
                d => d.replace("%1", e.str("Document Type")).replace("%2", e.str("Document No.")),
            };
            v.push(("Description", Value::text(desc)));
            add_line(ctx, FIN_CHARGE_LINE, "Finance Charge Memo No.", &no, line_no, &v)?;
        }
        if !fee.is_zero() {
            let account = posting_group_account(ctx, c.str("Customer Posting Group"), "Additional Fee Account")?;
            add_line(
                ctx,
                FIN_CHARGE_LINE,
                "Finance Charge Memo No.",
                &no,
                line_no + 10000,
                &[("Type", Value::text("G/L Account")), ("No.", Value::text(account)), ("Description", Value::text("Additional Fee")), ("Amount", dec(fee))],
            )?;
        }
        created.push(no);
        ctx.db.insert(ht, h)?;
    }
    Ok(json!({"memos": created, "message": format!("{} finance charge memo(s) were created.", created.len())}))
}

fn issue_fin_charge(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = doc_of(ctx, p, FIN_CHARGE_HEADER)?;
    let no = h.str("No.").to_string();
    let lines = lines_of(ctx, FIN_CHARGE_LINE, "Finance Charge Memo No.", &no);
    if lines.is_empty() {
        refuse!("There is nothing to issue on finance charge memo {no}.");
    }
    let date = h.get("Posting Date").as_date().unwrap_or(ctx.work_date);
    crate::posting::check_posting_date(ctx, date)?;
    let issued_no = setup_series(ctx, "Issued Fin. Chrg. M. Nos.")?;
    let terms = ctx.get_code(FIN_CHARGE_TERMS, h.str("Fin. Charge Terms Code"))?;
    let group = h.str("Customer Posting Group").to_string();
    let mut gl: Vec<(String, Decimal)> = Vec::new();
    let interest: Decimal = lines.iter().filter(|l| l.str("Type") == "Customer Ledger Entry").map(|l| l.get("Amount").as_dec()).sum();
    if terms.get("Post Interest").as_bool() && !interest.is_zero() {
        gl.push((posting_group_account(ctx, &group, "Interest Account")?, interest));
    }
    if terms.get("Post Additional Fee").as_bool() {
        gl.extend(lines.iter().filter(|l| l.str("Type") == "G/L Account").map(|l| (l.str("No.").to_string(), l.get("Amount").as_dec())));
    }
    post_charge(ctx, "FINCHRG", h.str("Customer No."), &group, date, "Finance Charge Memo", &issued_no, h.str("Posting Description"), &gl)?;
    let posted = terms.get("Post Interest").as_bool();
    for l in lines.iter().filter(|l| l.str("Type") == "Customer Ledger Entry") {
        reminder_entry(ctx, "Finance Charge Memo", &issued_no, &h, l, 0, l.get("Amount").as_dec(), posted)?;
    }
    file_issued(ctx, &h, (FIN_CHARGE_HEADER, FIN_CHARGE_LINE), (ISSUED_FIN_CHARGE_HEADER, ISSUED_FIN_CHARGE_LINE), "Finance Charge Memo No.", &issued_no)?;
    Ok(json!({"issued_no": issued_no, "open": {"page": "issued-finance-charge-memo", "key": issued_no}, "message": format!("Finance charge memo {issued_no} was issued.")}))
}

// ---- Customer statement ------------------------------------------------------------------------------

/// BC's Standard Statement: the balance before the period, the period's entries with a running
/// balance, the balance after, and the open entries' aging at the end.
fn statement(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = match (opt_str(p, "table"), p.get("key")) {
        (Some(CUSTOMER), Some(k)) => k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)),
        _ => opt_str(p, "no").map(str::to_string),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    let c = ctx.get_code(CUSTOMER, &no)?;
    let to = date_param(p, "to", ctx.work_date)?;
    let from = date_param(p, "from", crate::fiscal::period_of(ctx, to).0)?;
    let t = ctx.table(CUST_LEDGER)?;
    let mut entries: Vec<Record> = ctx.db.rows(CUST_LEDGER).filter(|e| e.str("Customer No.") == no).cloned().map(|r| crate::commands::read(ctx, t, r)).collect();
    entries.sort_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
    let opening: Decimal = entries.iter().filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d < from)).map(|e| e.get("Amount (LCY)").as_dec()).sum();
    let mut balance = opening;
    let money = |d: Decimal| format!("{:.2}", d.round_dp(2));
    let mut rows = vec![json!(["", "", "", format!("Balance as of {}", from.pred_opt().unwrap_or(from)), "", "", money(opening)])];
    for e in entries.iter().filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d >= from && d <= to)) {
        balance += e.get("Amount (LCY)").as_dec();
        rows.push(json!([
            e.get("Posting Date").to_string(),
            e.str("Document Type"),
            e.str("Document No."),
            e.str("Description"),
            e.get("Due Date").to_string(),
            money(e.get("Amount (LCY)").as_dec()),
            money(balance)
        ]));
    }
    rows.push(json!(["", "", "", format!("Balance as of {to}"), "", "", money(balance)]));
    // Aging of what is open at the end: not due, 1-30, 31-60, 61-90, over 90 days.
    let mut aging = [Decimal::ZERO; 5];
    for e in entries.iter().filter(|e| e.get("Open").as_bool() && e.get("Posting Date").as_date().is_some_and(|d| d <= to)) {
        let days = e.get("Due Date").as_date().map_or(0, |d| (to - d).num_days());
        let i = match days {
            ..=0 => 0,
            1..=30 => 1,
            31..=60 => 2,
            61..=90 => 3,
            _ => 4,
        };
        if let Some(a) = aging.get_mut(i) {
            *a += e.get("Remaining Amt. (LCY)").as_dec();
        }
    }
    let aging_rows: Vec<Json> = ["Not Due", "1-30 Days", "31-60 Days", "61-90 Days", "Over 90 Days"].iter().zip(aging.iter()).map(|(n, a)| json!([n, money(*a)])).collect();
    Ok(json!({"report": {
        "title": format!("Statement - {no} {} ({from} .. {to})", c.str("Name")),
        "columns": ["Posting Date", "Document Type", "Document No.", "Description", "Due Date", "Amount (LCY)", "Balance (LCY)"],
        "rows": rows,
        "sections": [{"caption": "Aging", "columns": ["", "Amount (LCY)"], "rows": aging_rows}]
    }}))
}

fn s_create() -> Json {
    json!({"type": "object", "properties": {"reminder_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"}, "document_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"}, "customer_no": {"type": "string"}}})
}

fn s_doc() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

fn s_statement() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "from": {"type": "string", "description": "YYYY-MM-DD; default the start of the period"}, "to": {"type": "string", "description": "YYYY-MM-DD; default the work date"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "reminder.create", label: "Create Reminders", group: "Sales", summary: "A reminder for every customer with overdue entries under their reminder terms.", mutates: true, params: s_create, run: create_reminders },
    CommandDef { id: "reminder.suggest_lines", label: "Suggest Reminder Lines", group: "Sales", summary: "Fill a reminder with the customer's overdue entries and the level's fee.", mutates: true, params: s_doc, run: suggest_lines },
    CommandDef { id: "reminder.issue", label: "Issue", group: "Sales", summary: "Issue a reminder: post its additional fee and file it.", mutates: true, params: s_doc, run: issue_reminder },
    CommandDef { id: "reminder.issue_all", label: "Issue Reminders", group: "Sales", summary: "Issue every open reminder.", mutates: true, params: s_doc, run: issue_all },
    CommandDef { id: "finance_charge.create", label: "Create Finance Charge Memos", group: "Sales", summary: "Interest (and a fee) on every customer's overdue invoices under their finance charge terms.", mutates: true, params: s_create, run: create_fin_charges },
    CommandDef { id: "finance_charge.issue", label: "Issue", group: "Sales", summary: "Issue a finance charge memo: post its interest and fee and file it.", mutates: true, params: s_doc, run: issue_fin_charge },
    CommandDef { id: "customer.statement", label: "Statement", group: "Sales", summary: "A customer's entries for a period with running balance and aging.", mutates: false, params: s_statement, run: statement },
];
