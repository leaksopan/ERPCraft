//! BC's item statistics: Item Statistics (page "Item Statistics 2"), Item Entry Statistics,
//! Item Turnover and Items by Location, from the item ledger entries.

use chrono::{Datelike, Months, NaiveDate};
use erpcraft_meta::Record;
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::party_stats::{money, periods};
use crate::schema::tables::{ITEM, ITEM_LEDGER};

fn item_no(ctx: &Ctx, p: &Json) -> Result<String> {
    let no = opt_str(p, "no")
        .map(str::to_string)
        .or_else(|| {
            (opt_str(p, "table") == Some(ITEM)).then(|| p.get("key")).flatten().and_then(|k| match k {
                Json::Array(a) => a.first().and_then(Json::as_str).map(str::to_string),
                other => other.as_str().map(str::to_string),
            })
        })
        .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    ctx.get_code(ITEM, &no)?;
    Ok(no)
}

fn entries<'a>(ctx: &'a Ctx, no: &'a str) -> impl Iterator<Item = &'a Record> + 'a {
    ctx.db.rows(ITEM_LEDGER).filter(move |e| e.str("Item No.") == no)
}

fn within(e: &Record, from: NaiveDate, to: NaiveDate) -> bool {
    e.get("Posting Date").as_date().is_some_and(|d| d >= from && d <= to)
}

fn pct(d: Decimal) -> String {
    format!("{:.1}", d.round_dp(1))
}

/// BC's ItemStatistics codeunit, on item ledger entries: net sales, gross margin, return rate and
/// sales growth against the same length of time before.
fn statistics(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let sales_in = |from: NaiveDate, to: NaiveDate| -> (Decimal, Decimal, Decimal, Decimal) {
        let mut sales = Decimal::ZERO;
        let mut cost = Decimal::ZERO;
        let mut sold = Decimal::ZERO;
        let mut returned = Decimal::ZERO;
        for e in entries(ctx, &no).filter(|e| e.str("Entry Type") == "Sale" && within(e, from, to)) {
            let amount = e.get("Sales Amount (Actual)").as_dec();
            sales += amount;
            // Shipped, not yet invoiced: no sales amount yet, so its cost stays out of the margin
            // (BC counts the expected amounts of both; item entries here keep actual amounts).
            if !amount.is_zero() {
                cost -= e.get("Cost Amount (Actual)").as_dec();
            }
            let q = e.get("Quantity").as_dec();
            if q.is_sign_negative() { sold -= q } else { returned += q }
        }
        (sales, cost, sold, returned)
    };
    let ps = periods(ctx.work_date);
    let mut rows: Vec<Vec<Json>> = vec![
        vec![json!("Sales Growth Rate (%)")],
        vec![json!("Net Sales (LCY)")],
        vec![json!("Gross Margin (%)")],
        vec![json!("Return Rate (%)")],
    ];
    for (i, (_, from, to)) in ps.iter().enumerate() {
        let (sales, cost, sold, returned) = sales_in(*from, *to);
        // The period before: the previous month, or the previous year.
        let growth = if i < 3 {
            let back = if i == 0 { Months::new(1) } else { Months::new(12) };
            let (prev, ..) = sales_in(from.checked_sub_months(back).unwrap_or(*from), to.checked_sub_months(back).unwrap_or(*to));
            if prev.is_zero() { Decimal::ZERO } else { (sales - prev) / prev * Decimal::ONE_HUNDRED }
        } else {
            Decimal::ZERO
        };
        let margin = if sales.is_zero() { Decimal::ZERO } else { (sales - cost) / sales * Decimal::ONE_HUNDRED };
        let rate = if sold.is_zero() { Decimal::ZERO } else { returned / sold * Decimal::ONE_HUNDRED };
        rows[0].push(json!(if i < 3 { pct(growth) } else { String::new() }));
        rows[1].push(json!(money(sales)));
        rows[2].push(json!(pct(margin)));
        rows[3].push(json!(pct(rate)));
    }
    let value: Decimal = entries(ctx, &no).map(|e| e.get("Cost Amount (Actual)").as_dec()).sum();
    let names = ["This Fiscal Period", "This Fiscal Year", "Last Fiscal Year", "Lifetime"];
    let mut cols = vec![json!("")];
    cols.extend(names.iter().zip(ps.iter()).map(|(n, (label, ..))| json!(if *n == "This Fiscal Period" { format!("{n} ({label})") } else { (*n).to_string() })));
    Ok(json!({"report": {
        "title": format!("Item Statistics - {no}"),
        "columns": ["General", "Value"],
        "rows": [["Current Inventory Value (LCY)", money(value)], ["Expired Inventory Value (LCY)", money(Decimal::ZERO)]],
        "sections": [{"caption": "Sales", "columns": cols, "rows": rows}]
    }}))
}

/// BC's Item Entry Statistics: the most recently posted receipt, purchase invoice, positive
/// adjustment, shipment, sale and negative adjustment.
fn entry_statistics(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let last = |ty: &str, sign: Option<bool>| -> Option<&Record> {
        entries(ctx, &no)
            .filter(|e| e.str("Entry Type") == ty)
            .filter(|e| sign.is_none_or(|positive| e.get("Quantity").as_dec().is_sign_positive() == positive))
            .max_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()))
    };
    let kinds: [(&str, &str, Option<bool>, bool); 6] = [
        ("Receipt", "Purchase", Some(true), false),
        ("Purchases", "Purchase", None, false),
        ("Positive Adjustment", "Positive Adjmt.", None, false),
        ("Shipment", "Sale", Some(false), true),
        ("Sales", "Sale", None, true),
        ("Negative Adjustment", "Negative Adjmt.", None, true),
    ];
    let rows: Vec<Json> = kinds
        .iter()
        .map(|(caption, ty, sign, negate)| match last(ty, *sign) {
            Some(e) => {
                let q = e.get("Quantity").as_dec();
                let amount = if *ty == "Sale" { e.get("Sales Amount (Actual)").as_dec() } else { e.get("Cost Amount (Actual)").as_dec().abs() };
                json!([caption, e.get("Posting Date").to_string(), e.str("Document No."), (if *negate { -q } else { q }).normalize().to_string(), money(amount)])
            }
            None => json!([caption, "", "", "", ""]),
        })
        .collect();
    Ok(json!({"report": {
        "title": format!("Item Entry Statistics - {no}"),
        "columns": ["Most Recently Posted", "Date", "Document No.", "Quantity", "Amount (LCY)"],
        "rows": rows
    }}))
}

/// BC's Item Turnover: purchases and sales per month, the twelve months up to the work date.
fn turnover(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let work = ctx.work_date;
    let this_month = NaiveDate::from_ymd_opt(work.year(), work.month(), 1).unwrap_or(work);
    let mut rows = Vec::new();
    for back in (0..12u32).rev() {
        let start = this_month.checked_sub_months(Months::new(back)).unwrap_or(this_month);
        let end = start.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(start);
        let (mut pq, mut pa, mut sq, mut sa) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        for e in entries(ctx, &no).filter(|e| within(e, start, end)) {
            match e.str("Entry Type") {
                "Purchase" => {
                    pq += e.get("Quantity").as_dec();
                    pa += e.get("Cost Amount (Actual)").as_dec();
                }
                "Sale" => {
                    sq -= e.get("Quantity").as_dec();
                    sa += e.get("Sales Amount (Actual)").as_dec();
                }
                _ => {}
            }
        }
        rows.push(json!([start.to_string(), start.format("%B").to_string(), pq.normalize().to_string(), money(pa), sq.normalize().to_string(), money(sa)]));
    }
    Ok(json!({"report": {
        "title": format!("Item Turnover - {no}"),
        "columns": ["Period Start", "Period Name", "Purchases (Qty.)", "Purchases (LCY)", "Sales (Qty.)", "Sales (LCY)"],
        "rows": rows
    }}))
}

/// BC's Items by Location: the item's inventory at each location.
fn by_location(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let mut locs: Vec<(String, String)> = ctx.db.rows("Location").map(|l| (l.str("Code").to_string(), l.str("Name").to_string())).collect();
    locs.insert(0, (String::new(), "(no location)".to_string()));
    let rows: Vec<Json> = locs
        .iter()
        .map(|(code, name)| {
            let q: Decimal = entries(ctx, &no).filter(|e| e.str("Location Code") == code).map(|e| e.get("Quantity").as_dec()).sum();
            (code, name, q)
        })
        .filter(|(code, _, q)| !code.is_empty() || !q.is_zero())
        .map(|(code, name, q)| json!([code, name, q.normalize().to_string()]))
        .collect();
    Ok(json!({"report": {"title": format!("Items by Location - {no}"), "columns": ["Location Code", "Name", "Inventory"], "rows": rows}}))
}

fn s_no() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "item.statistics", label: "Item Statistics", group: "Inventory", summary: "An item's inventory value, net sales, gross margin, return rate and sales growth per period.", mutates: false, params: s_no, run: statistics },
    CommandDef { id: "item.entry_statistics", label: "Entry Statistics", group: "Inventory", summary: "An item's most recently posted receipt, purchase, shipment, sale and adjustments.", mutates: false, params: s_no, run: entry_statistics },
    CommandDef { id: "item.turnover", label: "Turnover", group: "Inventory", summary: "An item's purchases and sales per month for the last twelve months.", mutates: false, params: s_no, run: turnover },
    CommandDef { id: "item.by_location", label: "Items by Location", group: "Inventory", summary: "An item's inventory at each location.", mutates: false, params: s_no, run: by_location },
];
