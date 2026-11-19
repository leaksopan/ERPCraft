//! Financial reports: Balance Sheet, Income Statement, Aged Accounts Receivable / Payable. Each
//! returns `{"report": {title, columns, rows}}` like the other report commands.

use chrono::NaiveDate;
use erpcraft_meta::Record;
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::Ctx;
use crate::error::Result;
use crate::schema::tables::{GL_ACCOUNT, GL_ENTRY};
use crate::side::Side;

fn balance(ctx: &Ctx, account: &str, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Decimal {
    ctx.db
        .rows(GL_ENTRY)
        .filter(|e| {
            e.str("G/L Account No.") == account
                && e.get("Posting Date").as_date().is_some_and(|d| from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t))
        })
        .map(|e| e.get("Amount").as_dec())
        .sum()
}

fn posting_accounts<'a>(ctx: &'a Ctx, income_balance: &'a str) -> impl Iterator<Item = &'a Record> {
    ctx.db.rows(GL_ACCOUNT).filter(move |a| a.str("Account Type") == "Posting" && a.str("Income/Balance") == income_balance)
}

fn money(d: Decimal) -> String {
    d.round_dp(2).to_string()
}

/// Balance Sheet at `date`: assets, liabilities and equity by account, with the year's net income
/// shown under equity (it is not closed into retained earnings yet).
pub fn balance_sheet(ctx: &Ctx, date: NaiveDate) -> Result<Json> {
    let mut rows = Vec::new();
    let mut totals = [Decimal::ZERO; 3];
    for (i, (category, sign)) in [("Assets", Decimal::ONE), ("Liabilities", Decimal::NEGATIVE_ONE), ("Equity", Decimal::NEGATIVE_ONE)].iter().enumerate() {
        rows.push(json!([category, "", ""]));
        for a in posting_accounts(ctx, "Balance Sheet").filter(|a| a.str("Account Category") == *category) {
            let b = balance(ctx, a.str("No."), None, Some(date)) * sign;
            if b.is_zero() {
                continue;
            }
            if let Some(t) = totals.get_mut(i) {
                *t += b;
            }
            rows.push(json!([format!("    {} {}", a.str("No."), a.str("Name")), money(b), ""]));
        }
        if *category == "Equity" {
            let income: Decimal = -posting_accounts(ctx, "Income Statement").map(|a| balance(ctx, a.str("No."), None, Some(date))).sum::<Decimal>();
            if let Some(t) = totals.get_mut(i) {
                *t += income;
            }
            rows.push(json!(["    Net Income (current period)", money(income), ""]));
        }
        rows.push(json!([format!("Total {category}"), "", money(totals.get(i).copied().unwrap_or_default())]));
    }
    let [assets, liabilities, equity] = totals;
    rows.push(json!(["Total Liabilities and Equity", "", money(liabilities + equity)]));
    Ok(json!({
        "report": {"title": format!("Balance Sheet as of {date}"), "columns": ["", "Amount", "Total"], "rows": rows},
        "balanced": assets == liabilities + equity,
    }))
}

/// Income Statement for `from..=to`: revenue, cost of goods sold, gross profit, expenses, net
/// income.
pub fn income_statement(ctx: &Ctx, from: NaiveDate, to: NaiveDate) -> Result<Json> {
    let mut rows = Vec::new();
    let section = |rows: &mut Vec<Json>, category: &str, sign: Decimal| -> Decimal {
        rows.push(json!([category, "", ""]));
        let mut total = Decimal::ZERO;
        for a in posting_accounts(ctx, "Income Statement").filter(|a| a.str("Account Category") == category) {
            let b = balance(ctx, a.str("No."), Some(from), Some(to)) * sign;
            if b.is_zero() {
                continue;
            }
            total += b;
            rows.push(json!([format!("    {} {}", a.str("No."), a.str("Name")), money(b), ""]));
        }
        rows.push(json!([format!("Total {category}"), "", money(total)]));
        total
    };
    let revenue = section(&mut rows, "Income", Decimal::NEGATIVE_ONE);
    let cogs = section(&mut rows, "Cost of Goods Sold", Decimal::ONE);
    rows.push(json!(["Gross Profit", "", money(revenue - cogs)]));
    let expenses = section(&mut rows, "Expense", Decimal::ONE);
    let net = revenue - cogs - expenses;
    rows.push(json!(["Net Income", "", money(net)]));
    Ok(json!({
        "report": {"title": format!("Income Statement {from} to {to}"), "columns": ["", "Amount", "Total"], "rows": rows},
        "net_income": money(net),
    }))
}

/// Aged Accounts Receivable / Payable at `date`: open remaining amounts per customer / vendor by
/// days past due (Not Due, 1-30, 31-60, 61-90, Over 90).
pub fn aged(ctx: &Ctx, side: &Side, date: NaiveDate) -> Result<Json> {
    // Vendor remaining amounts are negative (credit); show what we owe as positive.
    let sign = if side.sales { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    let mut parties: Vec<(String, String, [Decimal; 6])> = Vec::new();
    for e in ctx.db.rows(side.ledger).filter(|e| e.get("Open").as_bool() && e.get("Posting Date").as_date().is_some_and(|d| d <= date)) {
        let rem = e.get("Remaining Amount").as_dec() * sign;
        if rem.is_zero() {
            continue;
        }
        let overdue = e.get("Due Date").as_date().map_or(0, |d| (date - d).num_days());
        let bucket = match overdue {
            i64::MIN..=0 => 0,
            1..=30 => 1,
            31..=60 => 2,
            61..=90 => 3,
            _ => 4,
        };
        let no = e.str(side.ledger_party).to_string();
        let name = e.str(if side.sales { "Customer Name" } else { "Vendor Name" }).to_string();
        let idx = match parties.iter().position(|p| p.0 == no) {
            Some(i) => i,
            None => {
                parties.push((no, name, [Decimal::ZERO; 6]));
                parties.len() - 1
            }
        };
        if let Some(p) = parties.get_mut(idx) {
            if let Some(b) = p.2.get_mut(bucket) {
                *b += rem;
            }
            if let Some(t) = p.2.get_mut(5) {
                *t += rem;
            }
        }
    }
    parties.sort_by(|a, b| a.0.cmp(&b.0));
    let mut grand = [Decimal::ZERO; 6];
    let mut rows: Vec<Json> = Vec::new();
    for (no, name, b) in &parties {
        for (g, v) in grand.iter_mut().zip(b.iter()) {
            *g += *v;
        }
        let mut row = vec![json!(format!("{no} {name}"))];
        row.extend(b.iter().map(|v| json!(money(*v))));
        rows.push(Json::Array(row));
    }
    let mut total = vec![json!("Total")];
    total.extend(grand.iter().map(|v| json!(money(*v))));
    rows.push(Json::Array(total));
    let title = if side.sales { "Aged Accounts Receivable" } else { "Aged Accounts Payable" };
    Ok(json!({
        "report": {"title": format!("{title} as of {date}"), "columns": [side.party_caption, "Not Due", "1-30 Days", "31-60 Days", "61-90 Days", "Over 90 Days", "Balance"], "rows": rows},
        "total": money(grand[5]),
    }))
}
