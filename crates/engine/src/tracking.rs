//! Item tracking (BC's Item Tracking Lines / ItemJnlPostLine tracking checks): the lot and serial
//! numbers of a line are Reservation Entries pointing at it (source type = table id, subtype =
//! document type, ID, batch, line no.). Posting a tracked item splits the item ledger entry per
//! lot / serial number; inbound serial numbers must be new, outbound ones (and lots) on stock.

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::posting::{ItemEntry, post_item};
use crate::refuse;
use crate::schema::tables::{ITEM, ITEM_LEDGER, SALES_DOC_TYPES};
use crate::schema::tracking::{LOT_INFO, RESERVATION_ENTRY, SERIAL_INFO, TRACKING_CODE};

/// The line a set of tracking entries belongs to.
#[derive(Clone, Debug)]
pub struct Source {
    pub table: i64,
    pub subtype: i64,
    pub id: String,
    pub batch: String,
    pub line: i64,
}

/// One lot / serial part of a quantity (signed like the quantity).
#[derive(Clone, Debug)]
pub struct Part {
    pub qty: Decimal,
    pub serial: String,
    pub lot: String,
    pub expiration: Option<NaiveDate>,
}

fn doc_type_index(t: &str) -> i64 {
    SALES_DOC_TYPES.iter().position(|x| *x == t).map_or(0, |i| i as i64)
}

/// The tracking source of a document / journal line.
pub fn source_of(table: &str, l: &Record) -> Option<Source> {
    let (t, subtype, id, batch) = match table {
        "Sales Line" => (37, doc_type_index(l.str("Document Type")), l.str("Document No."), ""),
        "Purchase Line" => (39, doc_type_index(l.str("Document Type")), l.str("Document No."), ""),
        "Item Journal Line" => (83, 0, l.str("Journal Template Name"), l.str("Journal Batch Name")),
        "Transfer Line" => (5741, 0, l.str("Document No."), ""),
        _ => return None,
    };
    Some(Source { table: t, subtype, id: id.to_string(), batch: batch.to_string(), line: l.get("Line No.").as_int() })
}

fn source_line(ctx: &Ctx, s: &Source) -> Option<(String, Record)> {
    let (table, key) = match s.table {
        37 | 39 => (
            if s.table == 37 { "Sales Line" } else { "Purchase Line" },
            Key(vec![Value::text(SALES_DOC_TYPES.get(usize::try_from(s.subtype).unwrap_or(0)).copied().unwrap_or("Order")), Value::text(&s.id), Value::Int(s.line)]),
        ),
        83 => ("Item Journal Line", Key(vec![Value::text(&s.id), Value::text(&s.batch), Value::Int(s.line)])),
        5741 => ("Transfer Line", Key(vec![Value::text(&s.id), Value::Int(s.line)])),
        _ => return None,
    };
    ctx.db.get(table, &key).cloned().map(|r| (table.to_string(), r))
}

fn entries_of(ctx: &Ctx, s: &Source) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(RESERVATION_ENTRY)
        .filter(|e| {
            e.str("Reservation Status") != crate::reservation::RESERVATION
                && e.get("Source Type").as_int() == s.table
                && e.get("Source Subtype").as_int() == s.subtype
                && e.str("Source ID") == s.id
                && e.str("Source Batch Name") == s.batch
                && e.get("Source Ref. No.").as_int() == s.line
        })
        .cloned()
        .collect();
    v.sort_by_key(|e| e.get("Entry No.").as_int());
    v
}

/// (serial specific, lot specific) for an item; None when not tracked.
pub fn tracking_of(ctx: &Ctx, item: &Record) -> Option<(bool, bool)> {
    let code = item.str("Item Tracking Code");
    if code.is_empty() {
        return None;
    }
    ctx.db.get_code(TRACKING_CODE, code).map(|c| (c.get("SN Specific Tracking").as_bool(), c.get("Lot Specific Tracking").as_bool())).filter(|(s, l)| *s || *l)
}

/// Stock of an item at a location with a serial / lot number.
fn on_hand(ctx: &Ctx, item: &str, location: &str, serial: &str, lot: &str) -> Decimal {
    ctx.db
        .rows(ITEM_LEDGER)
        .filter(|e| e.str("Item No.") == item && e.str("Location Code") == location && e.str("Serial No.") == serial && e.str("Lot No.") == lot)
        .map(|e| e.get("Quantity").as_dec())
        .sum()
}

/// The parts `qty` (signed) of `item` posts as: the line's tracking (checked as BC's
/// ItemJnlPostLine does), or the whole quantity for an untracked item.
pub fn parts_for(ctx: &Ctx, item: &Record, source: Option<&Source>, location: &str, qty: Decimal) -> Result<Vec<Part>> {
    let whole = vec![Part { qty, serial: String::new(), lot: String::new(), expiration: None }];
    let Some((sn, lot)) = tracking_of(ctx, item) else { return Ok(whole) };
    if qty.is_zero() {
        return Ok(whole);
    }
    let no = item.str("No.");
    let entries = source.map(|s| entries_of(ctx, s)).unwrap_or_default();
    let handle: Decimal = entries.iter().map(|e| e.get("Qty. to Handle (Base)").as_dec()).sum();
    if entries.is_empty() {
        if sn {
            refuse!("You must assign a serial number for item {no}.");
        }
        refuse!("You must assign a lot number for item {no}.");
    }
    if handle != qty.abs() {
        refuse!("The quantity to handle in the item tracking assigned to the document line for item {no} is currently {handle}. It must be {}.", qty.abs());
    }
    let sign = if qty.is_sign_negative() { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
    let mut parts = Vec::new();
    for e in entries.iter().filter(|e| !e.get("Qty. to Handle (Base)").as_dec().is_zero()) {
        let (serial, lot_no) = (e.str("Serial No.").to_string(), e.str("Lot No.").to_string());
        if sn && serial.is_empty() {
            refuse!("You must assign a serial number for item {no}.");
        }
        if lot && lot_no.is_empty() {
            refuse!("You must assign a lot number for item {no}.");
        }
        let q = e.get("Qty. to Handle (Base)").as_dec();
        if !serial.is_empty() && q != Decimal::ONE {
            refuse!("Quantity must be -1, 0 or 1 when Serial No. is stated.");
        }
        if sign > Decimal::ZERO {
            if !serial.is_empty() && ctx.db.rows(ITEM_LEDGER).filter(|x| x.str("Item No.") == no && x.str("Serial No.") == serial).map(|x| x.get("Quantity").as_dec()).sum::<Decimal>() > Decimal::ZERO {
                refuse!("Serial No. {serial} is already on inventory.");
            }
        } else if on_hand(ctx, no, location, &serial, &lot_no) < q {
            refuse!("Item Tracking Serial No. {serial} Lot No. {lot_no} for Item No. {no} Variant  cannot be fully applied.");
        }
        parts.push(Part { qty: q * sign, serial, lot: lot_no, expiration: e.get("Expiration Date").as_date() });
    }
    Ok(parts)
}

/// The lots / serials a quantity leaves `location` with when nothing names them (stock in
/// transit being received): oldest first.
pub fn parts_from_stock(ctx: &Ctx, item: &Record, location: &str, qty: Decimal) -> Vec<Part> {
    if tracking_of(ctx, item).is_none() || qty.is_zero() {
        return vec![Part { qty, serial: String::new(), lot: String::new(), expiration: None }];
    }
    let no = item.str("No.");
    let mut keys: Vec<(String, String, Option<NaiveDate>)> = Vec::new();
    let mut ledger: Vec<&Record> = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no && e.str("Location Code") == location).collect();
    ledger.sort_by_key(|e| e.get("Entry No.").as_int());
    for e in ledger.iter().filter(|e| e.get("Quantity").as_dec() > Decimal::ZERO) {
        let k = (e.str("Serial No.").to_string(), e.str("Lot No.").to_string(), e.get("Expiration Date").as_date());
        if !keys.iter().any(|x| x.0 == k.0 && x.1 == k.1) {
            keys.push(k);
        }
    }
    let mut left = qty.abs();
    let sign = if qty.is_sign_negative() { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
    let mut parts = Vec::new();
    for (serial, lot, exp) in keys {
        if left.is_zero() {
            break;
        }
        let have = on_hand(ctx, no, location, &serial, &lot);
        let take = have.min(left);
        if take > Decimal::ZERO {
            parts.push(Part { qty: take * sign, serial, lot, expiration: exp });
            left -= take;
        }
    }
    if !left.is_zero() {
        parts.push(Part { qty: left * sign, serial: String::new(), lot: String::new(), expiration: None });
    }
    parts
}

/// Inventory (and inventory past its expiration date on the work date) of a lot / serial number.
pub fn calc_info(db: &crate::db::Database, work_date: NaiveDate, table: &str, r: &mut Record) {
    let field = if table == LOT_INFO { "Lot No." } else { "Serial No." };
    let (item, no, variant) = (r.str("Item No.").to_string(), r.str(field).to_string(), r.str("Variant Code").to_string());
    let rows: Vec<&Record> = db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && e.str(field) == no && e.str("Variant Code") == variant).collect();
    let inventory: Decimal = rows.iter().map(|e| e.get("Quantity").as_dec()).sum();
    let expired: Decimal = rows.iter().filter(|e| e.get("Expiration Date").as_date().is_some_and(|d| d < work_date)).map(|e| e.get("Quantity").as_dec()).sum();
    r.set("Inventory", Value::Dec(inventory));
    r.set("Expired Inventory", Value::Dec(expired.max(Decimal::ZERO)));
}

/// BC's CheckItemTrackingInformation: an information card must exist when the tracking code
/// says so, and a blocked one stops posting.
fn check_info(ctx: &Ctx, code: &Record, item: &str, p: &Part, inbound: bool) -> Result<()> {
    for (table, field, value, must) in [
        (SERIAL_INFO, "Serial No.", &p.serial, code.get(if inbound { "SN Info. Inbound Must Exist" } else { "SN Info. Outbound Must Exist" }).as_bool()),
        (LOT_INFO, "Lot No.", &p.lot, code.get(if inbound { "Lot Info. Inbound Must Exist" } else { "Lot Info. Outbound Must Exist" }).as_bool()),
    ] {
        if value.is_empty() {
            continue;
        }
        match ctx.db.get(table, &Key(vec![Value::text(item), Value::text(""), Value::text(value)])) {
            Some(info) if info.get("Blocked").as_bool() => {
                refuse!("Blocked must be equal to 'No' in {table}: Item No.={item}, Variant Code=, {field}={value}. Current value is 'Yes'.");
            }
            None if must => {
                refuse!("The {table} does not exist. Identification fields and values: Item No.='{item}',Variant Code='',{field}='{value}'");
            }
            _ => {}
        }
    }
    Ok(())
}

/// The expiration date of a lot / serial number on stock (its newest inbound entry with one).
fn stock_expiration(ctx: &Ctx, item: &str, p: &Part) -> Option<(i64, NaiveDate)> {
    ctx.db
        .rows(ITEM_LEDGER)
        .filter(|x| x.str("Item No.") == item && x.str("Serial No.") == p.serial && x.str("Lot No.") == p.lot && x.get("Quantity").as_dec() > Decimal::ZERO)
        .filter_map(|x| x.get("Expiration Date").as_date().map(|d| (x.get("Entry No.").as_int(), d)))
        .max_by_key(|(n, _)| *n)
}

/// The expiration date a part posts with (BC: inbound from the tracking line, the lot's existing
/// date or the item's Expiration Calculation; outbound from the stock it leaves, which strict
/// expiration posting refuses once expired).
fn expiration_of(ctx: &Ctx, code: &Record, item: &Record, e: &ItemEntry, p: &Part) -> Result<Option<NaiveDate>> {
    if !code.get("Use Expiration Dates").as_bool() {
        return Ok(p.expiration);
    }
    let no = item.str("No.");
    let existing = if p.lot.is_empty() { None } else { stock_expiration(ctx, no, &Part { serial: String::new(), ..p.clone() }).or_else(|| stock_expiration(ctx, no, p)) };
    if p.qty > Decimal::ZERO {
        if let (Some(given), Some((_, have))) = (p.expiration, existing)
            && given != have
        {
            refuse!("Only one expiration date is allowed per lot number. {} currently has two different expiration dates: {} and {}.", p.lot, have.format("%m/%d/%Y"), given.format("%m/%d/%Y"));
        }
        let date = p.expiration.or(existing.map(|(_, d)| d));
        let date = match date {
            Some(d) => Some(d),
            None if !item.str("Expiration Calculation").is_empty() => Some(crate::dateformula::calc(item.str("Expiration Calculation"), e.posting_date)?),
            None => None,
        };
        if date.is_none() && code.get("Man. Expir. Date Entry Reqd.").as_bool() {
            refuse!("Expiration Date must have a value in Item Tracking Lines: Item No.={no}, Serial No.={}, Lot No.={}. It cannot be zero or empty.", p.serial, p.lot);
        }
        return Ok(date);
    }
    let Some((entry, date)) = stock_expiration(ctx, no, p) else { return Ok(None) };
    if code.get("Strict Expiration Posting").as_bool() && e.posting_date > date && !matches!(e.entry_type, "Negative Adjmt." | "Transfer") && !matches!(e.doc_type, "Purchase Return Shipment" | "Purchase Credit Memo") {
        refuse!("Expiration Date is before the posting date in Item Ledger Entry: Entry No.={entry}.");
    }
    Ok(Some(date))
}

/// Posts `e` once per part (cost shared by quantity, the last part taking the rounding) and
/// stamps the tracking on each item ledger entry. Returns the first entry no.
pub fn post_parts(ctx: &mut Ctx, e: &ItemEntry, parts: &[Part]) -> Result<i64> {
    let t = ctx.table(ITEM_LEDGER)?;
    let code = if e.item.str("Item Tracking Code").is_empty() { None } else { ctx.db.get_code(TRACKING_CODE, e.item.str("Item Tracking Code")).cloned() };
    let mut parts: Vec<Part> = parts.to_vec();
    if let Some(code) = &code {
        for p in &mut parts {
            if p.serial.is_empty() && p.lot.is_empty() {
                continue;
            }
            check_info(ctx, code, e.item.str("No."), p, p.qty > Decimal::ZERO)?;
            p.expiration = expiration_of(ctx, code, e.item, e, p)?;
        }
    }
    let parts = &parts[..];
    let mut first = 0;
    let mut cost_left = e.cost;
    let mut sales_left = e.sales_amount;
    for (i, p) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let share = |total: Decimal| if e.qty.is_zero() { Decimal::ZERO } else { round_amount(total * p.qty / e.qty) };
        let cost = if last { cost_left } else { share(e.cost) };
        let sales = if last { sales_left } else { share(e.sales_amount) };
        cost_left -= cost;
        sales_left -= sales;
        let no = post_item(ctx, &ItemEntry { qty: p.qty, cost, sales_amount: sales, ..*e })?;
        if first == 0 {
            first = no;
        }
        if !p.serial.is_empty() || !p.lot.is_empty() {
            let mut r = ctx.get(ITEM_LEDGER, &Key(vec![Value::Int(no)]))?;
            r.set("Serial No.", Value::text(&p.serial));
            r.set("Lot No.", Value::text(&p.lot));
            if let Some(d) = p.expiration {
                r.set("Expiration Date", Value::Date(d));
            }
            let kind = match (p.serial.is_empty(), p.lot.is_empty()) {
                (false, false) => "Lot and Serial No.",
                (true, false) => "Lot No.",
                _ => "Serial No.",
            };
            r.set("Item Tracking", Value::text(kind));
            ctx.db.modify(t, r)?;
        }
    }
    Ok(first)
}

/// After posting: the handled tracking is used up; what is left stays for the next posting.
pub fn consume(ctx: &mut Ctx, source: &Source) -> Result<()> {
    let t = ctx.table(RESERVATION_ENTRY)?;
    for mut e in entries_of(ctx, source) {
        let left = e.get("Quantity (Base)").as_dec() - e.get("Qty. to Handle (Base)").as_dec();
        if left <= Decimal::ZERO {
            ctx.db.remove(&t.name, &e.key(t));
        } else {
            e.set("Quantity (Base)", dec(left));
            e.set("Qty. to Handle (Base)", dec(left));
            ctx.db.modify(t, e)?;
        }
    }
    Ok(())
}

/// A line's tracking goes when the line goes.
pub fn delete_for(ctx: &mut Ctx, source: &Source) {
    if let Ok(t) = ctx.table(RESERVATION_ENTRY) {
        for e in entries_of(ctx, source) {
            ctx.db.remove(&t.name, &e.key(t));
        }
    }
}

// ---- triggers & commands --------------------------------------------------------------------------

pub fn on_insert(ctx: &Ctx, t: &TableDef, r: &mut Record) -> bool {
    if t.name != RESERVATION_ENTRY {
        return false;
    }
    if r.get("Entry No.").as_int() == 0 {
        r.set("Entry No.", Value::Int(ctx.db.next_entry_no(RESERVATION_ENTRY)));
    }
    r.set("Reservation Status", Value::text("Surplus"));
    r.set("Creation Date", Value::Date(ctx.work_date));
    let s = Source { table: r.get("Source Type").as_int(), subtype: r.get("Source Subtype").as_int(), id: r.str("Source ID").to_string(), batch: r.str("Source Batch Name").to_string(), line: r.get("Source Ref. No.").as_int() };
    if let Some((table, l)) = source_line(ctx, &s) {
        let item_field = if table == "Sales Line" || table == "Purchase Line" { "No." } else { "Item No." };
        r.set("Item No.", l.get(item_field).clone());
        r.set("Location Code", l.get(if table == "Transfer Line" { "Transfer-from Code" } else { "Location Code" }).clone());
        r.set("Description", l.get("Description").clone());
        let credit = matches!(l.str("Document Type"), "Credit Memo" | "Return Order");
        let positive = match table.as_str() {
            "Sales Line" => credit,
            "Purchase Line" => !credit,
            "Item Journal Line" => l.str("Entry Type") == "Positive Adjmt.",
            _ => false,
        };
        r.set("Positive", Value::Bool(positive));
    }
    true
}

pub fn on_validate(ctx: &Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    if t.name != RESERVATION_ENTRY {
        return Ok(());
    }
    match field {
        "Quantity (Base)" => {
            let q = r.get("Quantity (Base)").as_dec();
            if q < Decimal::ZERO {
                refuse!("Quantity (Base) must not be negative.");
            }
            if !r.str("Serial No.").is_empty() && q > Decimal::ONE {
                refuse!("Quantity must be -1, 0 or 1 when Serial No. is stated.");
            }
            r.set("Qty. to Handle (Base)", dec(q));
        }
        "Serial No." if !r.str("Serial No.").is_empty() => {
            r.set("Quantity (Base)", dec(Decimal::ONE));
            r.set("Qty. to Handle (Base)", dec(Decimal::ONE));
            // A serial number is unique on the line.
            let (serial, no) = (r.str("Serial No.").to_string(), r.get("Entry No.").as_int());
            let s = Source { table: r.get("Source Type").as_int(), subtype: r.get("Source Subtype").as_int(), id: r.str("Source ID").to_string(), batch: r.str("Source Batch Name").to_string(), line: r.get("Source Ref. No.").as_int() };
            if entries_of(ctx, &s).iter().any(|e| e.str("Serial No.") == serial && e.get("Entry No.").as_int() != no) {
                refuse!("Serial No. {serial} is already on this line.");
            }
        }
        "Qty. to Handle (Base)" => {
            if r.get("Qty. to Handle (Base)").as_dec() > r.get("Quantity (Base)").as_dec() {
                refuse!("Qty. to Handle (Base) cannot be more than {}.", r.get("Quantity (Base)").as_dec());
            }
        }
        _ => {}
    }
    Ok(())
}

/// BC's Item Tracking Lines for the selected line.
fn lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let table = opt_str(p, "table").ok_or_else(|| EngineError::invalid("missing parameter 'table'"))?;
    let t = ctx.table(table)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    let l = ctx.get(table, &key)?;
    let item_no = if matches!(table, "Sales Line" | "Purchase Line") {
        if l.str("Type") != "Item" {
            refuse!("Item tracking is for item lines.");
        }
        l.str("No.").to_string()
    } else {
        l.str("Item No.").to_string()
    };
    let item = ctx.get_code(ITEM, &item_no)?;
    if tracking_of(ctx, &item).is_none() {
        refuse!("Item Tracking Code must have a value in Item: No.={item_no}. It cannot be zero or empty.");
    }
    let s = source_of(table, &l).ok_or_else(|| EngineError::refused(format!("{table} lines have no item tracking.")))?;
    Ok(json!({"open": {"page": "item-tracking-lines", "filters": [
        ["Source Type", s.table.to_string()], ["Source Subtype", s.subtype.to_string()], ["Source ID", s.id], ["Source Batch Name", s.batch], ["Source Ref. No.", s.line.to_string()],
        ["Reservation Status", "Surplus".to_string()]
    ]}}))
}

fn s_line() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}}, "required": ["table", "key"]})
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "tracking.lines",
    label: "Item Tracking Lines",
    group: "Inventory",
    summary: "The lot and serial numbers of a document or journal line.",
    mutates: false,
    params: s_line,
    run: lines,
}];
