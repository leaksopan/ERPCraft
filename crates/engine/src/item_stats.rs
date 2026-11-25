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

/// The location a view is limited to: `location_code`, or the stockkeeping unit it runs from.
fn location_of(p: &Json) -> Option<String> {
    opt_str(p, "location_code").map(str::to_string).or_else(|| {
        (opt_str(p, "table") == Some(crate::schema::sku::SKU)).then(|| p.get("key").and_then(Json::as_array).and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)).flatten()
    })
}

fn item_no(ctx: &Ctx, p: &Json) -> Result<String> {
    let sku_item = (opt_str(p, "table") == Some(crate::schema::sku::SKU)).then(|| p.get("key").and_then(Json::as_array).and_then(|a| a.get(1)).and_then(Json::as_str).map(str::to_string)).flatten();
    let no = opt_str(p, "no")
        .map(str::to_string)
        .or(sku_item)
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
                cost -= crate::costing::entry_valuation(e);
            }
            let q = e.get("Quantity").as_dec();
            if q.is_sign_negative() { sold -= q } else { returned += q }
        }
        (sales, cost, sold, returned)
    };
    let ps = periods(ctx, ctx.work_date);
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
    let value: Decimal = entries(ctx, &no).map(crate::costing::entry_valuation).sum();
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
                let amount = if *ty == "Sale" { e.get("Sales Amount (Actual)").as_dec() } else { crate::costing::entry_valuation(e).abs() };
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
                    pa += crate::costing::entry_valuation(e);
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

// ---- Item availability by event / period / location (BC pages 5530, 157, 492) -------------------

/// One expected movement of an item: (date, location, quantity +in / -out, what it is).
pub(crate) struct Movement {
    pub date: NaiveDate,
    pub location: String,
    pub qty: Decimal,
    pub what: String,
    /// The line it comes from: table, document no., line no.
    pub source: &'static str,
    pub doc: String,
    pub line: i64,
    pub variant: String,
}

fn per(l: &Record) -> Decimal {
    match l.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    }
}

/// Open sales / purchase / transfer lines of an item as future movements (BC's gross
/// requirements and scheduled receipts).
pub(crate) fn movements(ctx: &Ctx, no: &str) -> Vec<Movement> {
    let mut v = Vec::new();
    let work = ctx.work_date;
    for (table, sign, date_field) in [("Sales Line", Decimal::NEGATIVE_ONE, "Shipment Date"), ("Purchase Line", Decimal::ONE, "Expected Receipt Date")] {
        for l in ctx.db.rows(table).filter(|l| l.str("Type") == "Item" && l.str("No.") == no && l.str("Document Type") == "Order") {
            let q = l.get("Outstanding Quantity").as_dec() * per(l);
            if q.is_zero() {
                continue;
            }
            let side = if table == "Sales Line" { "Sales Order" } else { "Purchase Order" };
            let source = if table == "Sales Line" { "Sales Line" } else { "Purchase Line" };
            v.push(Movement {
                date: l.get(date_field).as_date().unwrap_or(work),
                location: l.str("Location Code").to_string(),
                qty: sign * q,
                what: format!("{side} {}", l.str("Document No.")),
                source,
                doc: l.str("Document No.").to_string(),
                line: l.get("Line No.").as_int(),
                variant: l.str("Variant Code").to_string(),
            });
        }
    }
    for l in ctx.db.rows(crate::schema::transfer::TRANSFER_LINE).filter(|l| l.str("Item No.") == no) {
        let out = (l.get("Quantity").as_dec() - l.get("Quantity Shipped").as_dec()) * per(l);
        let inn = (l.get("Quantity").as_dec() - l.get("Quantity Received").as_dec()) * per(l);
        let doc = l.str("Document No.");
        if !out.is_zero() {
            v.push(Movement { date: l.get("Shipment Date").as_date().unwrap_or(work), location: l.str("Transfer-from Code").to_string(), qty: -out, what: format!("Transfer Shipment {doc}"), source: "Transfer Line", doc: doc.to_string(), line: l.get("Line No.").as_int(), variant: l.str("Variant Code").to_string() });
        }
        if !inn.is_zero() {
            v.push(Movement { date: l.get("Receipt Date").as_date().unwrap_or(work), location: l.str("Transfer-to Code").to_string(), qty: inn, what: format!("Transfer Receipt {doc}"), source: "Transfer Line", doc: doc.to_string(), line: l.get("Line No.").as_int(), variant: l.str("Variant Code").to_string() });
        }
    }
    v.sort_by(|a, b| a.date.cmp(&b.date).then(b.qty.cmp(&a.qty)));
    v
}

fn on_hand(ctx: &Ctx, no: &str, location: Option<&str>) -> Decimal {
    entries(ctx, no).filter(|e| location.is_none_or(|l| e.str("Location Code") == l)).map(|e| e.get("Quantity").as_dec()).sum()
}

fn n(d: Decimal) -> String {
    d.normalize().to_string()
}

/// BC's Item Availability by Event: today's inventory, then every open order's movement with the
/// projected inventory after it.
fn by_event(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let loc = location_of(p);
    let mut balance = on_hand(ctx, &no, loc.as_deref());
    let mut rows = vec![json!([ctx.work_date.to_string(), "Inventory", "", "", n(balance)])];
    for m in movements(ctx, &no).into_iter().filter(|m| loc.as_ref().is_none_or(|l| &m.location == l)) {
        balance += m.qty;
        let (gross, receipt) = if m.qty < Decimal::ZERO { (n(-m.qty), String::new()) } else { (String::new(), n(m.qty)) };
        rows.push(json!([m.date.to_string(), m.what, gross, receipt, n(balance)]));
    }
    Ok(json!({"report": {"title": format!("Item Availability by Event - {no}"), "columns": ["Date", "Source", "Gross Requirement", "Scheduled Receipt", "Projected Inventory"], "rows": rows}}))
}

/// BC's Item Availability by Periods: per month from the work date, the requirements, receipts
/// and the projected available balance at the end of it.
fn by_period(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let loc = location_of(p);
    let moves: Vec<Movement> = movements(ctx, &no).into_iter().filter(|m| loc.as_ref().is_none_or(|l| &m.location == l)).collect();
    let mut balance = on_hand(ctx, &no, loc.as_deref());
    let first = NaiveDate::from_ymd_opt(ctx.work_date.year(), ctx.work_date.month(), 1).unwrap_or(ctx.work_date);
    let mut rows = Vec::new();
    for i in 0..6u32 {
        let start = first.checked_add_months(Months::new(i)).unwrap_or(first);
        let end = start.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(start);
        // Anything overdue counts in the first period.
        let in_period = |m: &&Movement| (i == 0 && m.date < start) || (m.date >= start && m.date <= end);
        let gross: Decimal = moves.iter().filter(in_period).filter(|m| m.qty < Decimal::ZERO).map(|m| -m.qty).sum();
        let receipts: Decimal = moves.iter().filter(in_period).filter(|m| m.qty > Decimal::ZERO).map(|m| m.qty).sum();
        balance += receipts - gross;
        rows.push(json!([start.to_string(), start.format("%B %Y").to_string(), n(gross), n(receipts), n(balance)]));
    }
    Ok(json!({"report": {"title": format!("Item Availability by Periods - {no}"), "columns": ["Period Start", "Period Name", "Gross Requirement", "Scheduled Receipt", "Projected Available Balance"], "rows": rows}}))
}

/// BC's Item Availability by Location: per location, inventory, requirements, receipts and the
/// projected available balance.
fn by_availability_location(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let moves = movements(ctx, &no);
    let mut locs: Vec<(String, String)> = vec![(String::new(), "(no location)".to_string())];
    locs.extend(ctx.db.rows("Location").map(|l| (l.str("Code").to_string(), l.str("Name").to_string())));
    let rows: Vec<Json> = locs
        .iter()
        .filter_map(|(code, name)| {
            let inv = on_hand(ctx, &no, Some(code));
            let gross: Decimal = moves.iter().filter(|m| m.location == *code && m.qty < Decimal::ZERO).map(|m| -m.qty).sum();
            let receipts: Decimal = moves.iter().filter(|m| m.location == *code && m.qty > Decimal::ZERO).map(|m| m.qty).sum();
            if inv.is_zero() && gross.is_zero() && receipts.is_zero() {
                return None;
            }
            Some(json!([code, name, n(inv), n(gross), n(receipts), n(inv + receipts - gross)]))
        })
        .collect();
    Ok(json!({"report": {"title": format!("Item Availability by Location - {no}"), "columns": ["Code", "Name", "Inventory", "Gross Requirement", "Scheduled Receipt", "Projected Available Balance"], "rows": rows}}))
}

/// BC's Item Availability by Variant: per variant, inventory, requirements, receipts and the
/// projected available balance.
fn by_variant(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let no = item_no(ctx, p)?;
    let moves = movements(ctx, &no);
    let mut variants: Vec<(String, String)> = vec![(String::new(), "(no variant)".to_string())];
    variants.extend(ctx.db.rows(crate::schema::item_setup::ITEM_VARIANT).filter(|v| v.str("Item No.") == no).map(|v| (v.str("Code").to_string(), v.str("Description").to_string())));
    let rows: Vec<Json> = variants
        .iter()
        .map(|(code, name)| {
            let inv: Decimal = entries(ctx, &no).filter(|e| e.str("Variant Code") == code).map(|e| e.get("Quantity").as_dec()).sum();
            let gross: Decimal = moves.iter().filter(|m| m.variant == *code && m.qty < Decimal::ZERO).map(|m| -m.qty).sum();
            let receipts: Decimal = moves.iter().filter(|m| m.variant == *code && m.qty > Decimal::ZERO).map(|m| m.qty).sum();
            json!([code, name, n(inv), n(gross), n(receipts), n(inv + receipts - gross)])
        })
        .collect();
    Ok(json!({"report": {"title": format!("Item Availability by Variant - {no}"), "columns": ["Code", "Description", "Inventory", "Gross Requirement", "Scheduled Receipt", "Projected Available Balance"], "rows": rows}}))
}

fn s_no() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "item.statistics", label: "Item Statistics", group: "Inventory", summary: "An item's inventory value, net sales, gross margin, return rate and sales growth per period.", mutates: false, params: s_no, run: statistics },
    CommandDef { id: "item.entry_statistics", label: "Entry Statistics", group: "Inventory", summary: "An item's most recently posted receipt, purchase, shipment, sale and adjustments.", mutates: false, params: s_no, run: entry_statistics },
    CommandDef { id: "item.turnover", label: "Turnover", group: "Inventory", summary: "An item's purchases and sales per month for the last twelve months.", mutates: false, params: s_no, run: turnover },
    CommandDef { id: "item.availability_by_event", label: "Event", group: "Inventory", summary: "An item's projected inventory after each open order, by date.", mutates: false, params: s_no, run: by_event },
    CommandDef { id: "item.availability_by_period", label: "Period", group: "Inventory", summary: "An item's requirements, receipts and projected balance per month.", mutates: false, params: s_no, run: by_period },
    CommandDef { id: "item.availability_by_variant", label: "Variant", group: "Inventory", summary: "An item's inventory, requirements, receipts and projected balance per variant.", mutates: false, params: s_no, run: by_variant },
    CommandDef { id: "item.availability_by_location", label: "Location", group: "Inventory", summary: "An item's inventory, requirements, receipts and projected balance per location.", mutates: false, params: s_no, run: by_availability_location },
    CommandDef { id: "item.by_location", label: "Items by Location", group: "Inventory", summary: "An item's inventory at each location.", mutates: false, params: s_no, run: by_location },
];
