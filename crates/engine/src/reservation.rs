//! Reservations (BC's Reservation Management, Reserve / Cancel Reservation, Reservation Entries,
//! Order Promising): a sales order line reserves quantity from the inventory at its location or
//! from open purchase order lines. A reservation is a pair of Reservation Entries with status
//! Reservation: the demand side (negative, the sales line) and the supply side (positive, the item
//! ledger entry or purchase line), numbered n and n + 1. Shipping the sales line uses the
//! reservation up; receiving the purchase line moves its supply side onto the new inventory.

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::tables::{ITEM, ITEM_LEDGER, PURCH_LINE, SALES_HEADER, SALES_LINE};
use crate::schema::tracking::RESERVATION_ENTRY;
use crate::tracking::{Source, source_of};

pub const RESERVATION: &str = "Reservation";
pub const REQUEST: &str = "Reservation Request";
pub const ENTRY_SUMMARY: &str = "Entry Summary";
/// Source Type of a supply on inventory (table 32, Item Ledger Entry).
const ILE: i64 = 32;

fn per(l: &Record) -> Decimal {
    match l.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    }
}

fn is_reservation(e: &Record) -> bool {
    e.str("Reservation Status") == RESERVATION
}

fn of_source(e: &Record, s: &Source) -> bool {
    e.get("Source Type").as_int() == s.table
        && e.get("Source Subtype").as_int() == s.subtype
        && e.str("Source ID") == s.id
        && e.str("Source Batch Name") == s.batch
        && e.get("Source Ref. No.").as_int() == s.line
}

/// The reservation entries of a line (either side).
fn entries_of(ctx: &Ctx, s: &Source) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(RESERVATION_ENTRY).filter(|e| is_reservation(e) && of_source(e, s)).cloned().collect();
    v.sort_by_key(|e| e.get("Entry No.").as_int());
    v
}

/// The other half of a reservation pair.
fn pair_of(ctx: &Ctx, e: &Record) -> Option<Record> {
    let no = e.get("Entry No.").as_int();
    let other = if e.get("Positive").as_bool() { no - 1 } else { no + 1 };
    ctx.db.rows(RESERVATION_ENTRY).find(|x| x.get("Entry No.").as_int() == other && is_reservation(x) && x.get("Positive").as_bool() != e.get("Positive").as_bool()).cloned()
}

/// The reserved base quantity of a line (positive), from the database of a read.
pub fn reserved_base(db: &crate::db::Database, table: &str, l: &Record) -> Decimal {
    let Some(s) = source_of(table, l) else { return Decimal::ZERO };
    db.rows(RESERVATION_ENTRY).filter(|e| is_reservation(e) && of_source(e, &s)).map(|e| e.get("Quantity (Base)").as_dec().abs()).sum()
}

/// Reserved quantities of an item: (on inventory, on sales orders, on purchase orders).
pub fn item_reserved(db: &crate::db::Database, item: &str) -> (Decimal, Decimal, Decimal) {
    let mut v = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    for e in db.rows(RESERVATION_ENTRY).filter(|e| is_reservation(e) && e.str("Item No.") == item) {
        let q = e.get("Quantity (Base)").as_dec().abs();
        match e.get("Source Type").as_int() {
            ILE => v.0 += q,
            37 => v.1 += q,
            39 => v.2 += q,
            _ => {}
        }
    }
    v
}

fn on_hand(ctx: &Ctx, item: &str, location: &str) -> Decimal {
    ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && e.str("Location Code") == location).map(|e| e.get("Quantity").as_dec()).sum()
}

/// Inventory at a location not yet reserved.
pub fn available_inventory(ctx: &Ctx, item: &str, location: &str) -> Decimal {
    let reserved: Decimal = ctx
        .db
        .rows(RESERVATION_ENTRY)
        .filter(|e| is_reservation(e) && e.get("Source Type").as_int() == ILE && e.str("Item No.") == item && e.str("Location Code") == location)
        .map(|e| e.get("Quantity (Base)").as_dec())
        .sum();
    on_hand(ctx, item, location) - reserved
}

fn line_of(ctx: &Ctx, p: &Json) -> Result<(String, Record)> {
    let table = opt_str(p, "table").unwrap_or(SALES_LINE).to_string();
    if table != SALES_LINE && table != PURCH_LINE {
        refuse!("Reservations are for sales and purchase lines.");
    }
    let t = ctx.table(&table)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    Ok((table.clone(), ctx.get(&table, &key)?))
}

struct Pair<'a> {
    item: &'a str,
    description: &'a str,
    demand: &'a Source,
    demand_location: &'a str,
    shipment: Option<NaiveDate>,
    supply: Source,
    supply_location: &'a str,
    receipt: Option<NaiveDate>,
    qty: Decimal,
}

/// Inserts a reservation pair (demand n, supply n + 1).
fn insert_pair(ctx: &mut Ctx, p: &Pair) -> Result<()> {
    let t = ctx.table(RESERVATION_ENTRY)?;
    let no = ctx.db.next_entry_no(RESERVATION_ENTRY);
    for (positive, s, location, qty, n) in [(false, p.demand, p.demand_location, -p.qty, no), (true, &p.supply, p.supply_location, p.qty, no + 1)] {
        let mut e = t.init_record();
        e.set("Entry No.", Value::Int(n));
        e.set("Positive", Value::Bool(positive));
        e.set("Item No.", Value::text(p.item));
        e.set("Location Code", Value::text(location));
        e.set("Quantity (Base)", dec(qty));
        e.set("Qty. to Handle (Base)", dec(qty));
        e.set("Reservation Status", Value::text(RESERVATION));
        e.set("Description", Value::text(p.description));
        e.set("Creation Date", Value::Date(ctx.work_date));
        e.set("Source Type", Value::Int(s.table));
        e.set("Source Subtype", Value::Int(s.subtype));
        e.set("Source ID", Value::text(&s.id));
        e.set("Source Batch Name", Value::text(&s.batch));
        e.set("Source Ref. No.", Value::Int(s.line));
        if let Some(d) = p.shipment {
            e.set("Shipment Date", Value::Date(d));
        }
        if let Some(d) = p.receipt {
            e.set("Expected Receipt Date", Value::Date(d));
        }
        e.set("Created By", Value::text(ctx.user));
        ctx.db.insert(t, e)?;
    }
    Ok(())
}

/// Reserves what is not reserved yet of a sales order line: first from inventory at its
/// location, then from purchase order lines by expected receipt date. Returns what was reserved.
pub fn reserve_line(ctx: &mut Ctx, l: &Record) -> Result<Decimal> {
    reserve_line_from(ctx, l, None)
}

/// As `reserve_line`, from one kind of supply only (32 inventory, 39 purchase lines).
pub fn reserve_line_from(ctx: &mut Ctx, l: &Record, only: Option<i64>) -> Result<Decimal> {
    if l.str("Document Type") != "Order" || l.str("Type") != "Item" {
        refuse!("You can only reserve items on sales order lines.");
    }
    let item_no = l.str("No.").to_string();
    let item = ctx.get_code(ITEM, &item_no)?;
    if item.str("Reserve") == "Never" {
        refuse!("Reserve must not be Never in Item No.={item_no}.");
    }
    let Some(demand) = source_of(SALES_LINE, l) else { return Ok(Decimal::ZERO) };
    let need = l.get("Outstanding Quantity").as_dec() * per(l) - reserved_base(ctx.db, SALES_LINE, l);
    if need <= Decimal::ZERO {
        refuse!("The line is fully reserved.");
    }
    let location = l.str("Location Code").to_string();
    let description = l.str("Description").to_string();
    let shipment = l.get("Shipment Date").as_date();
    let mut left = need;
    // Inventory first: the latest receipt at the location stands for it.
    let from_stock = if only.is_none_or(|o| o == ILE) { available_inventory(ctx, &item_no, &location).min(left) } else { Decimal::ZERO };
    if from_stock > Decimal::ZERO {
        let ile = ctx
            .db
            .rows(ITEM_LEDGER)
            .filter(|e| e.str("Item No.") == item_no && e.str("Location Code") == location && e.get("Quantity").as_dec() > Decimal::ZERO)
            .map(|e| e.get("Entry No.").as_int())
            .max()
            .unwrap_or(0);
        let supply = Source { table: ILE, subtype: 0, id: String::new(), batch: String::new(), line: ile };
        insert_pair(ctx, &Pair { item: &item_no, description: &description, demand: &demand, demand_location: &location, shipment, supply, supply_location: &location, receipt: None, qty: from_stock })?;
        left -= from_stock;
    }
    // Then open purchase order lines, earliest first.
    let mut purchases: Vec<Record> = ctx
        .db
        .rows(PURCH_LINE)
        .filter(|p| p.str("Document Type") == "Order" && p.str("Type") == "Item" && p.str("No.") == item_no && p.str("Location Code") == location)
        .cloned()
        .collect();
    purchases.sort_by_key(|p| (p.get("Expected Receipt Date").as_date(), p.str("Document No.").to_string(), p.get("Line No.").as_int()));
    for p in purchases {
        if left <= Decimal::ZERO || only.is_some_and(|o| o != 39) {
            break;
        }
        let free = p.get("Outstanding Quantity").as_dec() * per(&p) - reserved_base(ctx.db, PURCH_LINE, &p);
        let take = free.min(left);
        if take <= Decimal::ZERO {
            continue;
        }
        let Some(supply) = source_of(PURCH_LINE, &p) else { continue };
        let receipt = p.get("Expected Receipt Date").as_date();
        insert_pair(ctx, &Pair { item: &item_no, description: &description, demand: &demand, demand_location: &location, shipment, supply, supply_location: &location, receipt, qty: take })?;
        left -= take;
    }
    Ok(need - left)
}

/// BC's Reserve on a sales order line: the Reservation page for it.
fn reserve(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (table, l) = line_of(ctx, p)?;
    if table != SALES_LINE || l.str("Document Type") != "Order" || l.str("Type") != "Item" {
        refuse!("You can only reserve items on sales order lines.");
    }
    let item = ctx.get_code(ITEM, l.str("No."))?;
    if item.str("Reserve") == "Never" {
        refuse!("Reserve must not be Never in Item No.={}.", l.str("No."));
    }
    let s = source_of(SALES_LINE, &l).ok_or_else(|| EngineError::refused("This line cannot be reserved."))?;
    refresh_request(ctx, &l, &s)?;
    Ok(json!({"open": {"page": "reservation-card", "key": [s.table, s.subtype, s.id, s.line]}}))
}

/// BC's Reserve from Inventory: reserve the line from inventory, without the page.
fn reserve_from_inventory(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (table, l) = line_of(ctx, p)?;
    if table != SALES_LINE {
        refuse!("You can only reserve items on sales order lines.");
    }
    let got = reserve_line_from(ctx, &l, Some(ILE))?;
    if got.is_zero() {
        refuse!("There is nothing available to reserve.");
    }
    Ok(json!({"reserved": got.to_string(), "message": format!("{} {} was reserved from inventory.", got.normalize(), l.str("No."))}))
}

/// The page's header and summary lines for a demand line, recalculated.
fn refresh_request(ctx: &mut Ctx, l: &Record, s: &Source) -> Result<()> {
    let item = l.str("No.").to_string();
    let location = l.str("Location Code").to_string();
    let need = l.get("Outstanding Quantity").as_dec() * per(l);
    let reserved = reserved_base(ctx.db, SALES_LINE, l);
    let rt = ctx.table(REQUEST)?;
    let mut h = rt.init_record();
    for (f, v) in [("Source Type", Value::Int(s.table)), ("Source Subtype", Value::Int(s.subtype)), ("Source ID", Value::text(&s.id)), ("Source Ref. No.", Value::Int(s.line))] {
        h.set(f, v);
    }
    h.set("Item No.", Value::text(&item));
    h.set("Variant Code", l.get("Variant Code").clone());
    h.set("Location Code", Value::text(&location));
    h.set("Shipment Date", l.get("Shipment Date").clone());
    h.set("Description", l.get("Description").clone());
    h.set("Quantity to Reserve", dec(need));
    h.set("Reserved Quantity", dec(reserved));
    h.set("Unreserved Quantity", dec(need - reserved));
    h.set("Caption", Value::text(format!("Reservation - {} {} {item}", l.str("Document Type"), l.str("Document No."))));
    if ctx.db.get(&rt.name, &h.key(rt)).is_some() {
        ctx.db.modify(rt, h)?;
    } else {
        ctx.db.insert(rt, h)?;
    }
    // Summary lines: inventory, then purchase orders.
    let st = ctx.table(ENTRY_SUMMARY)?;
    let old: Vec<Record> = ctx.db.rows(ENTRY_SUMMARY).filter(|e| of_source(e, s)).cloned().collect();
    for e in old {
        ctx.db.remove(&st.name, &e.key(st));
    }
    let mine = entries_of(ctx, s);
    let supplies: Vec<Record> = mine.iter().filter_map(|e| pair_of(ctx, e)).collect();
    let current = |table: i64| -> Decimal { supplies.iter().filter(|x| x.get("Source Type").as_int() == table).map(|x| x.get("Quantity (Base)").as_dec()).sum() };
    let inv_total = on_hand(ctx, &item, &location);
    let inv_reserved = inv_total - available_inventory(ctx, &item, &location);
    let purchases: Vec<Record> = ctx.db.rows(PURCH_LINE).filter(|p| p.str("Document Type") == "Order" && p.str("Type") == "Item" && p.str("No.") == item && p.str("Location Code") == location).cloned().collect();
    let po_total: Decimal = purchases.iter().map(|p| p.get("Outstanding Quantity").as_dec() * per(p)).sum();
    let po_reserved: Decimal = purchases.iter().map(|p| reserved_base(ctx.db, PURCH_LINE, p)).sum();
    let rows = [(1, ILE, "Item Ledger Entry", inv_total, inv_reserved), (2, 39, "Purchase Order", po_total, po_reserved)];
    for (n, table, caption, total, res) in rows {
        if total.is_zero() && res.is_zero() {
            continue;
        }
        let mut e = st.init_record();
        for (f, v) in [("Source Type", Value::Int(s.table)), ("Source Subtype", Value::Int(s.subtype)), ("Source ID", Value::text(&s.id)), ("Source Ref. No.", Value::Int(s.line))] {
            e.set(f, v);
        }
        e.set("Entry No.", Value::Int(n));
        e.set("Table ID", Value::Int(table));
        e.set("Summary Type", Value::text(caption));
        e.set("Total Quantity", dec(total));
        e.set("Total Reserved Quantity", dec(res));
        e.set("Total Available Quantity", dec(total - res));
        e.set("Current Reserved Quantity", dec(current(table)));
        ctx.db.insert(st, e)?;
    }
    Ok(())
}

/// The demand line and its source behind a Reservation page row (header or summary line).
fn request_of(ctx: &Ctx, p: &Json) -> Result<(Record, Source, Option<Record>)> {
    let table = opt_str(p, "table").unwrap_or(REQUEST);
    let t = ctx.table(table)?;
    let rec = ctx.get(table, &Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?)?;
    let s = Source { table: rec.get("Source Type").as_int(), subtype: rec.get("Source Subtype").as_int(), id: rec.str("Source ID").to_string(), batch: String::new(), line: rec.get("Source Ref. No.").as_int() };
    let doc_type = crate::schema::tables::SALES_DOC_TYPES.get(usize::try_from(s.subtype).unwrap_or(0)).copied().unwrap_or("Order");
    let l = ctx.get(SALES_LINE, &Key(vec![Value::text(doc_type), Value::text(&s.id), Value::Int(s.line)]))?;
    let summary = (table == ENTRY_SUMMARY).then_some(rec);
    Ok((l, s, summary))
}

/// BC's Reserve from Current Line: from the selected summary line's kind of supply.
fn reserve_from_line(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (l, s, summary) = request_of(ctx, p)?;
    let only = summary.as_ref().map(|e| e.get("Table ID").as_int());
    let got = reserve_line_from(ctx, &l, only)?;
    if got.is_zero() {
        refuse!("There is nothing available to reserve.");
    }
    refresh_request(ctx, &l, &s)?;
    Ok(json!({"reserved": got.to_string()}))
}

/// BC's Auto Reserve: the first available quantity, inventory first.
fn auto_reserve(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (l, s, _) = request_of(ctx, p)?;
    let got = reserve_line(ctx, &l)?;
    if got.is_zero() {
        refuse!("There is nothing available to reserve.");
    }
    refresh_request(ctx, &l, &s)?;
    Ok(json!({"reserved": got.to_string()}))
}

/// BC's Cancel Reservation from Current Line: the line's reservations on that kind of supply.
fn cancel_from_line(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (l, s, summary) = request_of(ctx, p)?;
    let only = summary.as_ref().map(|e| e.get("Table ID").as_int());
    let t = ctx.table(RESERVATION_ENTRY)?;
    let mut total = Decimal::ZERO;
    for e in entries_of(ctx, &s) {
        let Some(o) = pair_of(ctx, &e) else { continue };
        if only.is_some_and(|x| o.get("Source Type").as_int() != x) {
            continue;
        }
        total += e.get("Quantity (Base)").as_dec().abs();
        ctx.db.remove(&t.name, &o.key(t));
        ctx.db.remove(&t.name, &e.key(t));
    }
    if total.is_zero() {
        refuse!("There are no reservations to cancel.");
    }
    refresh_request(ctx, &l, &s)?;
    Ok(json!({"message": format!("The reservation of {} {} was canceled.", total.normalize(), l.str("No."))}))
}

/// BC's Available to Reserve: the supply behind a summary line.
fn available(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (l, _, summary) = request_of(ctx, p)?;
    match summary.map(|e| e.get("Table ID").as_int()) {
        Some(39) => Ok(json!({"report": {"title": format!("Available - Purchase Lines - {}", l.str("No.")), "columns": ["Document No.", "Line No.", "Expected Receipt Date", "Outstanding Qty. (Base)", "Reserved Qty. (Base)"], "rows":
            ctx.db.rows(PURCH_LINE).filter(|x| x.str("Document Type") == "Order" && x.str("Type") == "Item" && x.str("No.") == l.str("No.") && x.str("Location Code") == l.str("Location Code"))
                .map(|x| json!([x.str("Document No."), x.get("Line No.").as_int(), x.get("Expected Receipt Date").as_date().map(|d| d.to_string()).unwrap_or_default(), (x.get("Outstanding Quantity").as_dec() * per(x)).normalize().to_string(), reserved_base(ctx.db, PURCH_LINE, x).normalize().to_string()]))
                .collect::<Vec<Json>>()}})),
        _ => Ok(json!({"open": {"page": "item-ledger-entries", "filters": [["Item No.", l.str("No.")], ["Location Code", l.str("Location Code")], ["Open", "Yes"]]}})),
    }
}

/// Reservation Entries from the Reservation page: those of its demand line.
fn entries_of_request(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (_, s, _) = request_of(ctx, p)?;
    Ok(json!({"open": {"page": "reservation-entries", "filters": [
        ["Source Type", s.table.to_string()], ["Source Subtype", s.subtype.to_string()], ["Source ID", s.id], ["Source Ref. No.", s.line.to_string()]
    ]}}))
}

/// Removes every reservation of a line, with the other half of each.
pub fn cancel_for(ctx: &mut Ctx, table: &str, l: &Record) -> Result<Decimal> {
    let Some(s) = source_of(table, l) else { return Ok(Decimal::ZERO) };
    let t = ctx.table(RESERVATION_ENTRY)?;
    let mut total = Decimal::ZERO;
    for e in entries_of(ctx, &s) {
        total += e.get("Quantity (Base)").as_dec().abs();
        if let Some(o) = pair_of(ctx, &e) {
            ctx.db.remove(&t.name, &o.key(t));
        }
        ctx.db.remove(&t.name, &e.key(t));
    }
    Ok(total)
}

fn cancel(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (table, l) = line_of(ctx, p)?;
    let total = cancel_for(ctx, &table, &l)?;
    if total.is_zero() {
        refuse!("There are no reservations to cancel.");
    }
    Ok(json!({"message": format!("The reservation of {} {} was canceled.", total.normalize(), l.str("No."))}))
}

fn entries(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (table, l) = line_of(ctx, p)?;
    let s = source_of(&table, &l).ok_or_else(|| EngineError::refused("This line has no reservations."))?;
    Ok(json!({"open": {"page": "reservation-entries", "filters": [
        ["Source Type", s.table.to_string()], ["Source Subtype", s.subtype.to_string()], ["Source ID", s.id], ["Source Ref. No.", s.line.to_string()]
    ]}}))
}

/// Reduces a reservation pair by `take` (removing it when nothing is left).
fn reduce_pair(ctx: &mut Ctx, e: &Record, take: Decimal) -> Result<()> {
    let t = ctx.table(RESERVATION_ENTRY)?;
    for mut x in [Some(e.clone()), pair_of(ctx, e)].into_iter().flatten() {
        let q = x.get("Quantity (Base)").as_dec();
        let left = q.abs() - take;
        if left <= Decimal::ZERO {
            ctx.db.remove(&t.name, &x.key(t));
        } else {
            let signed = if q < Decimal::ZERO { -left } else { left };
            x.set("Quantity (Base)", dec(signed));
            x.set("Qty. to Handle (Base)", dec(signed));
            ctx.db.modify(t, x)?;
        }
    }
    Ok(())
}

/// After an item ledger entry was posted for a document line (`qty` signed, in the base unit):
/// a shipped sales line uses its reservations up; a received purchase line hands its
/// reservations over to the new inventory (entry `ile`).
pub fn on_posted(ctx: &mut Ctx, table: &str, l: &Record, ile: i64, qty: Decimal) -> Result<()> {
    let Some(s) = source_of(table, l) else { return Ok(()) };
    let mut left = qty.abs();
    let list = entries_of(ctx, &s);
    if list.is_empty() {
        return Ok(());
    }
    let t = ctx.table(RESERVATION_ENTRY)?;
    for e in list {
        if left <= Decimal::ZERO {
            break;
        }
        let q = e.get("Quantity (Base)").as_dec().abs();
        let take = q.min(left);
        left -= take;
        if table == SALES_LINE && qty < Decimal::ZERO {
            reduce_pair(ctx, &e, take)?;
        } else if table == PURCH_LINE && qty > Decimal::ZERO && e.get("Positive").as_bool() {
            if take == q {
                let mut moved = e.clone();
                moved.set("Source Type", Value::Int(ILE));
                moved.set("Source Subtype", Value::Int(0));
                moved.set("Source ID", Value::text(""));
                moved.set("Source Ref. No.", Value::Int(ile));
                ctx.db.modify(t, moved)?;
            } else if let Some(demand) = pair_of(ctx, &e) {
                // Split: the received part becomes a pair of its own on the new inventory.
                reduce_pair(ctx, &e, take)?;
                let ds = Source {
                    table: demand.get("Source Type").as_int(),
                    subtype: demand.get("Source Subtype").as_int(),
                    id: demand.str("Source ID").to_string(),
                    batch: demand.str("Source Batch Name").to_string(),
                    line: demand.get("Source Ref. No.").as_int(),
                };
                let supply = Source { table: ILE, subtype: 0, id: String::new(), batch: String::new(), line: ile };
                insert_pair(
                    ctx,
                    &Pair {
                        item: e.str("Item No."),
                        description: e.str("Description"),
                        demand: &ds,
                        demand_location: demand.str("Location Code"),
                        shipment: demand.get("Shipment Date").as_date(),
                        supply,
                        supply_location: e.str("Location Code"),
                        receipt: None,
                        qty: take,
                    },
                )?;
            }
        }
    }
    Ok(())
}

// ---- Order promising (BC page 99000959, Available-to-Promise) ------------------------------------

/// The earliest date from which `qty` of the line can be shipped without any later requirement
/// running short: the projected inventory, without the line itself, stays at or above `qty`.
fn earliest(ctx: &Ctx, l: &Record, qty: Decimal) -> Option<NaiveDate> {
    let no = l.str("No.");
    let doc = l.str("Document No.");
    let line = l.get("Line No.").as_int();
    let moves: Vec<crate::item_stats::Movement> =
        crate::item_stats::movements(ctx, no).into_iter().filter(|m| !(m.source == SALES_LINE && m.doc == doc && m.line == line)).collect();
    let mut balance: Decimal = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no).map(|e| e.get("Quantity").as_dec()).sum();
    // Projected inventory after each date; candidates are the work date and each receipt date.
    let mut points: Vec<(NaiveDate, Decimal)> = vec![(ctx.work_date, balance)];
    for m in &moves {
        balance += m.qty;
        let d = m.date.max(ctx.work_date);
        match points.last_mut() {
            Some(last) if last.0 == d => last.1 = balance,
            _ => points.push((d, balance)),
        }
    }
    (0..points.len()).find(|&i| points[i..].iter().all(|(_, b)| *b >= qty)).map(|i| points[i].0)
}

fn order_promising(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let doc_type = opt_str(p, "document_type").unwrap_or("Order").to_string();
    let no = opt_str(p, "no").ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?.to_string();
    let header = ctx.get(SALES_HEADER, &Key(vec![Value::text(&doc_type), Value::text(&no)]))?;
    let accept = p.get("accept").and_then(Json::as_bool).unwrap_or(false);
    let lt = ctx.table(SALES_LINE)?;
    let mut lines: Vec<Record> = ctx.db.rows(SALES_LINE).filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no && l.str("Type") == "Item").cloned().collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    if lines.is_empty() {
        refuse!("There are no item lines to promise.");
    }
    let requested = header.get("Requested Delivery Date").as_date();
    let mut rows = Vec::new();
    let mut late = 0;
    for l in &lines {
        let qty = l.get("Outstanding Quantity").as_dec() * per(l);
        let date = earliest(ctx, l, qty);
        let shown = date.map_or_else(String::new, |d| d.to_string());
        if date.is_none() {
            late += 1;
        }
        rows.push(json!([l.str("No."), l.str("Description"), qty.normalize().to_string(), requested.map_or_else(String::new, |d| d.to_string()), l.get("Shipment Date").as_date().map_or_else(String::new, |d| d.to_string()), shown]));
        if accept && let Some(d) = date {
            let mut m = l.clone();
            m.set("Shipment Date", Value::Date(d));
            ctx.db.modify(lt, m)?;
        }
    }
    let note = if late > 0 { format!("{late} line(s) cannot be promised from inventory and open orders.") } else { String::new() };
    if accept {
        return Ok(json!({"message": if note.is_empty() { "The earliest shipment dates were accepted.".to_string() } else { note }}));
    }
    Ok(json!({"report": {"title": format!("Order Promising Lines - {doc_type} {no}"), "columns": ["Item No.", "Description", "Quantity", "Requested Delivery Date", "Planned Delivery Date", "Earliest Shipment Date"], "rows": rows}, "message": note}))
}

fn s_line() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string", "default": "Sales Line"}, "key": {}}, "required": ["key"]})
}

fn s_promise() -> Json {
    json!({"type": "object", "properties": {"document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"},
        "accept": {"type": "boolean", "description": "set each line's shipment date to its earliest shipment date"}}, "required": ["no"]})
}

fn s_row() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string", "default": "Reservation Request"}, "key": {}}, "required": ["key"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "reservation.reserve", label: "Reserve", group: "Inventory", summary: "Open the Reservation page of a sales order line.", mutates: true, params: s_line, run: reserve },
    CommandDef { id: "reservation.reserve_from_inventory", label: "Reserve from Inventory", group: "Inventory", summary: "Reserve a sales order line from inventory.", mutates: true, params: s_line, run: reserve_from_inventory },
    CommandDef { id: "reservation.reserve_from_line", label: "Reserve from Current Line", group: "Inventory", summary: "Reserve from the selected summary line's supply.", mutates: true, params: s_row, run: reserve_from_line },
    CommandDef { id: "reservation.auto_reserve", label: "Auto Reserve", group: "Inventory", summary: "Reserve the first available quantity for the line.", mutates: true, params: s_row, run: auto_reserve },
    CommandDef { id: "reservation.cancel_from_line", label: "Cancel Reservation from Current Line", group: "Inventory", summary: "Cancel the line's reservations on the selected supply.", mutates: true, params: s_row, run: cancel_from_line },
    CommandDef { id: "reservation.available", label: "Available to Reserve", group: "Inventory", summary: "The supply behind a summary line.", mutates: false, params: s_row, run: available },
    CommandDef { id: "reservation.entries_of_request", label: "Reservation Entries", group: "Inventory", summary: "The reservation entries of the page's line.", mutates: false, params: s_row, run: entries_of_request },
    CommandDef { id: "reservation.cancel", label: "Cancel Reservation", group: "Inventory", summary: "Cancel the reservations of a sales or purchase line.", mutates: true, params: s_line, run: cancel },
    CommandDef { id: "reservation.entries", label: "Reservation Entries", group: "Inventory", summary: "The reservation entries of a sales or purchase line.", mutates: false, params: s_line, run: entries },
    CommandDef { id: "sales.order_promising", label: "Order Promising", group: "Sales", summary: "The earliest date each item line of a sales order can be shipped; accept to set the shipment dates.", mutates: true, params: s_promise, run: order_promising },
];
