//! BC's Customer / Vendor Entry Statistics (pages 302, 303) and Statistics by Currencies
//! (pages 486, 487): the last document of each type, documents per period, and balances per
//! currency.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use erpcraft_meta::Record;
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str, read};
use crate::ctx::{Ctx, round_amount};
use crate::error::{EngineError, Result};
use crate::schema::tables::{CUST_LEDGER, CUSTOMER, PURCH_HEADER, PURCH_LINE, SALES_HEADER, SALES_LINE, VEND_LEDGER, VENDOR};

/// The customer or vendor side of the statistics.
struct Party {
    table: &'static str,
    ledger: &'static str,
    no_field: &'static str,
    title: &'static str,
    /// Document types whose amounts BC shows negated, so each shows with its usual sign.
    negate: &'static [&'static str],
    line_table: &'static str,
    header_table: &'static str,
    bill_field: &'static str,
    not_invoiced_qty: &'static str,
}

const CUST: Party = Party {
    table: CUSTOMER,
    ledger: CUST_LEDGER,
    no_field: "Customer No.",
    title: "Customer Entry Statistics",
    negate: &["Payment", "Credit Memo"],
    line_table: SALES_LINE,
    header_table: SALES_HEADER,
    bill_field: "Bill-to Customer No.",
    not_invoiced_qty: "Qty. Shipped Not Invoiced",
};

const VEND: Party = Party {
    table: VENDOR,
    ledger: VEND_LEDGER,
    no_field: "Vendor No.",
    title: "Vendor Entry Statistics",
    negate: &["Invoice", "Reminder", "Finance Charge Memo"],
    line_table: PURCH_LINE,
    header_table: PURCH_HEADER,
    bill_field: "Pay-to Vendor No.",
    not_invoiced_qty: "Qty. Rcd. Not Invoiced",
};

/// BC's page order of the document types.
const DOC_TYPES: [&str; 6] = ["Payment", "Invoice", "Credit Memo", "Reminder", "Finance Charge Memo", "Refund"];

fn party_no(ctx: &Ctx, p: &Json, party: &Party) -> Result<String> {
    let no = opt_str(p, "no")
        .map(str::to_string)
        .or_else(|| {
            (opt_str(p, "table") == Some(party.table)).then(|| p.get("key")).flatten().and_then(|k| match k {
                Json::Array(a) => a.first().and_then(Json::as_str).map(str::to_string),
                other => other.as_str().map(str::to_string),
            })
        })
        .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    ctx.get_code(party.table, &no)?;
    Ok(no)
}

/// The party's ledger entries with their FlowFields, oldest first.
fn entries(ctx: &Ctx, party: &Party, no: &str) -> Result<Vec<Record>> {
    let t = ctx.table(party.ledger)?;
    let mut v: Vec<Record> = ctx.db.rows(party.ledger).filter(|e| e.str(party.no_field) == no).cloned().map(|r| read(ctx, t, r)).collect();
    v.sort_by_key(|e| e.get("Entry No.").as_int());
    Ok(v)
}

fn money(d: Decimal) -> String {
    format!("{:.2}", round_amount(d))
}

/// This Period (the work date's month), This Year, Last Year and To Date.
fn periods(work: NaiveDate) -> [(String, NaiveDate, NaiveDate); 4] {
    let first = |y: i32, m: u32| NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(work);
    let month_start = first(work.year(), work.month());
    let next_month = if work.month() == 12 { first(work.year() + 1, 1) } else { first(work.year(), work.month() + 1) };
    let month_end = next_month.pred_opt().unwrap_or(work);
    let year = |y: i32| (first(y, 1), NaiveDate::from_ymd_opt(y, 12, 31).unwrap_or(work));
    let (ty0, ty1) = year(work.year());
    let (ly0, ly1) = year(work.year() - 1);
    [
        (month_start.format("%B %Y").to_string(), month_start, month_end),
        (format!("Fiscal Year {}", work.year()), ty0, ty1),
        (format!("Fiscal Year {}", work.year() - 1), ly0, ly1),
        ("To Date".to_string(), NaiveDate::MIN, work),
    ]
}

fn entry_statistics(ctx: &mut Ctx, p: &Json, party: &Party) -> Result<Json> {
    let no = party_no(ctx, p, party)?;
    let es = entries(ctx, party, &no)?;
    let sign = |ty: &str, d: Decimal| if party.negate.contains(&ty) { -d } else { d };

    let mut last_docs = Vec::new();
    for ty in DOC_TYPES {
        let last = es.iter().filter(|e| e.str("Document Type") == ty).max_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
        last_docs.push(match last {
            Some(e) => json!([
                ty,
                e.get("Posting Date").to_string(),
                e.str("Document No."),
                e.str("Currency Code"),
                money(sign(ty, e.get("Amount").as_dec())),
                money(sign(ty, e.get("Remaining Amount").as_dec()))
            ]),
            None => json!([ty, "", "", "", "", ""]),
        });
    }

    let ps = periods(ctx.work_date);
    let by_no: BTreeMap<i64, &Record> = es.iter().map(|e| (e.get("Entry No.").as_int(), e)).collect();
    let mut cols: Vec<Vec<Json>> = Vec::new();
    for (_, from, to) in &ps {
        let in_period: Vec<&Record> = es.iter().filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d >= *from && d <= *to)).collect();
        let mut col: Vec<Json> = DOC_TYPES.iter().map(|ty| json!(in_period.iter().filter(|e| e.str("Document Type") == *ty).count())).collect();
        if party.table == CUSTOMER {
            // Avg. Collection Period: days from invoice to the entry that closed it.
            let days: Vec<i64> = in_period
                .iter()
                .filter(|e| e.str("Document Type") == "Invoice" && !e.get("Open").as_bool())
                .filter_map(|e| {
                    let closer = by_no
                        .get(&e.get("Closed by Entry No.").as_int())
                        .copied()
                        .or_else(|| es.iter().find(|c| c.get("Closed by Entry No.").as_int() == e.get("Entry No.").as_int()))?;
                    Some((closer.get("Posting Date").as_date()? - e.get("Posting Date").as_date()?).num_days().max(0))
                })
                .collect();
            let avg = if days.is_empty() { Decimal::ZERO } else { Decimal::from(days.iter().sum::<i64>()) / Decimal::from(days.len() as i64) };
            col.push(json!(format!("{:.1}", avg.round_dp(1))));
            // Largest Balance (LCY): the highest running balance within the period.
            let mut bal: Decimal = es.iter().filter(|e| e.get("Posting Date").as_date().is_some_and(|d| d < *from)).map(|e| e.get("Amount (LCY)").as_dec()).sum();
            let mut highest = bal;
            let mut dated: Vec<&&Record> = in_period.iter().collect();
            dated.sort_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
            for e in dated {
                bal += e.get("Amount (LCY)").as_dec();
                highest = highest.max(bal);
            }
            col.push(json!(money(highest)));
        } else {
            // Payment discounts are not tracked yet.
            col.push(json!(money(Decimal::ZERO)));
            col.push(json!(money(Decimal::ZERO)));
        }
        cols.push(col);
    }
    let mut labels: Vec<&str> = vec!["Payments", "Invoices", "Credit Memos", "Reminder", "Finance Charge Memos", "Refund"];
    labels.extend(if party.table == CUSTOMER {
        ["Avg. Collection Period (Days)", "Largest Balance (LCY)"]
    } else {
        ["Pmt. Disc. Received (LCY)", "Pmt. Disc. Missed (LCY)"]
    });
    let doc_rows: Vec<Json> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut row = vec![json!(l)];
            row.extend(cols.iter().map(|c| c.get(i).cloned().unwrap_or_default()));
            json!(row)
        })
        .collect();

    // Remaining Amt. (LCY) of the open entries per document type.
    let remaining: Vec<Json> = DOC_TYPES
        .iter()
        .map(|ty| {
            let sum: Decimal = es.iter().filter(|e| e.get("Open").as_bool() && e.str("Document Type") == *ty).map(|e| e.get("Remaining Amt. (LCY)").as_dec()).sum();
            json!([ty, money(sign(ty, sum))])
        })
        .collect();

    let mut period_cols = vec![json!("")];
    period_cols.extend(ps.iter().map(|(name, _, _)| json!(name)));
    Ok(json!({"report": {
        "title": format!("{} - {no}", party.title),
        "columns": ["Last Documents", "Date", "Document No.", "Currency Code", "Amount", "Remaining Amount"],
        "rows": last_docs,
        "sections": [
            {"caption": "No. of Documents", "columns": period_cols, "rows": doc_rows},
            {"caption": "Remaining Amt. (LCY)", "columns": ["", "Remaining Amt. (LCY)"], "rows": remaining}
        ]
    }}))
}

/// BC's Statistics by Currencies: balance, outstanding orders, shipped (received) not invoiced
/// and balance due, per currency the party has entries or orders in.
fn by_currencies(ctx: &mut Ctx, p: &Json, party: &Party) -> Result<Json> {
    let no = party_no(ctx, p, party)?;
    let es = entries(ctx, party, &no)?;
    #[derive(Default)]
    struct Row {
        balance: Decimal,
        outstanding: Decimal,
        not_invoiced: Decimal,
        due: Decimal,
    }
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    rows.entry(String::new()).or_default();
    for e in es.iter().filter(|e| e.get("Open").as_bool()) {
        let r = rows.entry(e.str("Currency Code").to_string()).or_default();
        let rem = e.get("Remaining Amount").as_dec();
        r.balance += rem;
        if e.get("Due Date").as_date().is_some_and(|d| d <= ctx.work_date) {
            r.due += rem;
        }
    }
    let currency_of = |doc_type: &str, doc_no: &str| -> Option<(String, String)> {
        ctx.db
            .rows(party.header_table)
            .find(|h| h.str("Document Type") == doc_type && h.str("No.") == doc_no)
            .map(|h| (h.str("Currency Code").to_string(), h.str(party.bill_field).to_string()))
    };
    for l in ctx.db.rows(party.line_table).filter(|l| l.str("Document Type") == "Order") {
        let Some((cur, bill)) = currency_of("Order", l.str("Document No.")) else { continue };
        if bill != no {
            continue;
        }
        let qty = l.get("Quantity").as_dec();
        if qty.is_zero() {
            continue;
        }
        let per = l.get("Amount Including VAT").as_dec() / qty;
        let r = rows.entry(cur).or_default();
        r.outstanding += round_amount(per * l.get("Outstanding Quantity").as_dec());
        r.not_invoiced += round_amount(per * l.get(party.not_invoiced_qty).as_dec());
    }
    let caption = |code: &str| -> String {
        if code.is_empty() {
            return String::new();
        }
        ctx.db.rows("Currency").find(|c| c.str("Code") == code).map(|c| c.str("Description").to_string()).unwrap_or_default()
    };
    let out: Vec<Json> = rows
        .iter()
        .map(|(code, r)| {
            json!([
                code,
                caption(code),
                money(r.balance),
                money(r.outstanding),
                money(r.not_invoiced),
                money(r.balance + r.outstanding + r.not_invoiced),
                money(r.due)
            ])
        })
        .collect();
    let not_inv = if party.table == CUSTOMER { "Shipped Not Invoiced" } else { "Amt. Rcd. Not Invoiced" };
    let title = if party.table == CUSTOMER { "Customer Statistics by Currencies" } else { "Vendor Statistics by Currencies" };
    Ok(json!({"report": {
        "title": format!("{title} - {no}"),
        "columns": ["Code", "Description", "Balance", "Outstanding Orders", not_inv, "Total", "Balance Due"],
        "rows": out
    }}))
}

fn cust_entry_stats(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    entry_statistics(ctx, p, &CUST)
}
fn vend_entry_stats(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    entry_statistics(ctx, p, &VEND)
}
fn cust_by_currencies(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    by_currencies(ctx, p, &CUST)
}
fn vend_by_currencies(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    by_currencies(ctx, p, &VEND)
}

fn s_no() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "customer.entry_statistics",
        label: "Entry Statistics",
        group: "Sales",
        summary: "A customer's last documents, documents per period and remaining amounts.",
        mutates: false,
        params: s_no,
        run: cust_entry_stats,
    },
    CommandDef {
        id: "vendor.entry_statistics",
        label: "Entry Statistics",
        group: "Purchasing",
        summary: "A vendor's last documents, documents per period and remaining amounts.",
        mutates: false,
        params: s_no,
        run: vend_entry_stats,
    },
    CommandDef {
        id: "customer.statistics_by_currencies",
        label: "Statistics by Currencies",
        group: "Sales",
        summary: "A customer's balance, outstanding orders and amounts shipped not invoiced per currency.",
        mutates: false,
        params: s_no,
        run: cust_by_currencies,
    },
    CommandDef {
        id: "vendor.statistics_by_currencies",
        label: "Statistics by Currencies",
        group: "Purchasing",
        summary: "A vendor's balance, outstanding orders and amounts received not invoiced per currency.",
        mutates: false,
        params: s_no,
        run: vend_by_currencies,
    },
];
