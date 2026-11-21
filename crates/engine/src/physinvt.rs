//! Physical inventory orders (BC's Inventory/Counting: Calc. Phys. Invt. Order Lines, Calculate
//! Counting Period, Make Phys. Invt. Recording, Phys. Invt. Rec.-Finish, Phys. Invt. Order-Finish,
//! Phys. Invt. Order-Post): an order lists the expected quantity per item and location; one or
//! more recordings collect the counted quantities; finishing the order compares them; posting
//! adjusts the inventory through the phys. inventory journal and keeps the order as posted.

use chrono::{Datelike, Months, NaiveDate};
use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::physinvt::*;
use crate::schema::tables::{INVENTORY_SETUP, ITEM, ITEM_LEDGER};

/// The phys. inventory journal template and the batch an order posts through.
const JNL_TEMPLATE: &str = "PHYS INV";
const JNL_BATCH: &str = "PHYS-ORDER";

fn flag(p: &Json, name: &str) -> bool {
    p.get(name).and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")))).unwrap_or(false)
}

fn key_part(p: &Json, i: usize) -> Option<String> {
    let k = p.get("key")?;
    match k {
        Json::Array(a) => a.get(i).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)),
        Json::String(s) if i == 0 => Some(s.clone()),
        _ => None,
    }
}

fn order_of(ctx: &Ctx, p: &Json) -> Result<Record> {
    let no = match opt_str(p, "table") {
        Some(ORDER_HEADER) => key_part(p, 0),
        _ => opt_str(p, "no").map(str::to_string),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    ctx.get_code(ORDER_HEADER, &no)
}

fn recording_of(ctx: &Ctx, p: &Json) -> Result<Record> {
    let (order, rec) = match opt_str(p, "table") {
        Some(RECORD_HEADER) => (key_part(p, 0), key_part(p, 1).and_then(|s| s.parse::<i64>().ok())),
        _ => (opt_str(p, "order_no").map(str::to_string), p.get("recording_no").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))),
    };
    let order = order.ok_or_else(|| EngineError::invalid("missing parameter 'order_no'"))?;
    let rec = rec.ok_or_else(|| EngineError::invalid("missing parameter 'recording_no'"))?;
    ctx.get(RECORD_HEADER, &Key(vec![Value::text(&order), Value::Int(rec)]))
}

fn order_lines(ctx: &Ctx, no: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(ORDER_LINE).filter(|l| l.str("Document No.") == no).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

fn recordings(ctx: &Ctx, no: &str) -> Vec<Record> {
    ctx.db.rows(RECORD_HEADER).filter(|r| r.str("Order No.") == no).cloned().collect()
}

fn record_lines(ctx: &Ctx, no: &str, rec: i64) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(RECORD_LINE).filter(|l| l.str("Order No.") == no && l.get("Recording No.").as_int() == rec).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

fn check_open(h: &Record) -> Result<()> {
    if h.str("Status") != "Open" {
        refuse!("Status must be equal to 'Open' in Phys. Invt. Order Header: No.={}. Current value is '{}'.", h.str("No."), h.str("Status"));
    }
    Ok(())
}

fn on_hand(ctx: &Ctx, item: &str, location: &str) -> Decimal {
    ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && e.str("Location Code") == location).map(|e| e.get("Quantity").as_dec()).sum()
}

fn last_entry(ctx: &Ctx, item: &str, location: &str) -> i64 {
    ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && e.str("Location Code") == location).map(|e| e.get("Entry No.").as_int()).max().unwrap_or(0)
}

// ---- triggers ----------------------------------------------------------------------------------

pub fn on_insert(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<bool> {
    match t.name.as_str() {
        ORDER_HEADER => {
            if r.str("No.").is_empty() {
                let series = crate::noseries::series_from_setup(ctx, INVENTORY_SETUP, "Phys. Invt. Order Nos.")?;
                r.set("No.", Value::text(crate::noseries::next(ctx, &series)?));
                r.set("No. Series", Value::text(&series));
            }
            if r.str("Posting No. Series").is_empty()
                && let Some(s) = ctx.db.rows(INVENTORY_SETUP).next().map(|s| s.str("Posted Phys. Invt. Order Nos.").to_string())
            {
                r.set("Posting No. Series", Value::text(&s));
            }
            for f in ["Order Date", "Posting Date"] {
                if r.get(f).as_date().is_none() {
                    r.set(f, Value::Date(ctx.work_date));
                }
            }
            r.set("Status", Value::text("Open"));
            Ok(true)
        }
        ORDER_LINE => {
            let h = ctx.get_code(ORDER_HEADER, r.str("Document No."))?;
            check_open(&h)?;
            if r.get("Line No.").as_int() == 0 {
                let next = order_lines(ctx, h.str("No.")).iter().map(|l| l.get("Line No.").as_int()).max().unwrap_or(0) + 10000;
                r.set("Line No.", Value::Int(next));
            }
            if r.str("Location Code").is_empty() {
                r.set("Location Code", h.get("Location Code").clone());
            }
            Ok(true)
        }
        RECORD_HEADER => {
            let h = ctx.get_code(ORDER_HEADER, r.str("Order No."))?;
            check_open(&h)?;
            if r.get("Recording No.").as_int() == 0 {
                let next = recordings(ctx, h.str("No.")).iter().map(|x| x.get("Recording No.").as_int()).max().unwrap_or(0) + 1;
                r.set("Recording No.", Value::Int(next));
            }
            if r.str("Location Code").is_empty() {
                r.set("Location Code", h.get("Location Code").clone());
            }
            r.set("Status", Value::text("Open"));
            Ok(true)
        }
        RECORD_LINE => {
            let key = Key(vec![r.get("Order No.").clone(), r.get("Recording No.").clone()]);
            let h = ctx.get(RECORD_HEADER, &key)?;
            if h.str("Status") != "Open" {
                refuse!("Status must be equal to 'Open' in Phys. Invt. Record Header: Order No.={}, Recording No.={}. Current value is '{}'.", h.str("Order No."), h.get("Recording No.").as_int(), h.str("Status"));
            }
            if r.get("Line No.").as_int() == 0 {
                let next = record_lines(ctx, h.str("Order No."), h.get("Recording No.").as_int()).iter().map(|l| l.get("Line No.").as_int()).max().unwrap_or(0) + 10000;
                r.set("Line No.", Value::Int(next));
            }
            if r.str("Location Code").is_empty() {
                r.set("Location Code", h.get("Location Code").clone());
            }
            if r.get("Qty. per Unit of Measure").as_dec().is_zero() {
                r.set("Qty. per Unit of Measure", dec(Decimal::ONE));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn fill_from_item(ctx: &Ctx, r: &mut Record, item: &Record, order_line: bool) {
    r.set("Description", item.get("Description").clone());
    r.set("Description 2", item.get("Description 2").clone());
    r.set("Shelf No.", item.get("Shelf No.").clone());
    if order_line {
        r.set("Base Unit of Measure Code", item.get("Base Unit of Measure").clone());
        r.set("Unit of Measure", item.get("Base Unit of Measure").clone());
        r.set("Unit Amount", item.get("Unit Cost").clone());
        r.set("Unit Cost", item.get("Unit Cost").clone());
        r.set("Gen. Prod. Posting Group", item.get("Gen. Prod. Posting Group").clone());
        r.set("Inventory Posting Group", item.get("Inventory Posting Group").clone());
        r.set("Item Category Code", item.get("Item Category Code").clone());
        r.set("Phys Invt Counting Period Code", item.get("Phys Invt Counting Period Code").clone());
        r.set("Qty. Expected (Base)", dec(Decimal::ZERO));
        r.set("Qty. Exp. Calculated", Value::Bool(false));
        if let Some(h) = ctx.db.get_code(ORDER_HEADER, r.str("Document No.")) {
            r.set("Gen. Bus. Posting Group", h.get("Gen. Bus. Posting Group").clone());
        }
    } else {
        r.set("Unit of Measure Code", item.get("Base Unit of Measure").clone());
        r.set("Qty. per Unit of Measure", dec(Decimal::ONE));
    }
}

pub fn on_validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    match (t.name.as_str(), field) {
        (ORDER_HEADER, _) if field != "Status" => check_open(r),
        (ORDER_LINE, "Item No." | "Location Code" | "Variant Code") => {
            let h = ctx.get_code(ORDER_HEADER, r.str("Document No."))?;
            check_open(&h)?;
            if field == "Item No." && !r.str("Item No.").is_empty() {
                let item = ctx.get_code(ITEM, r.str("Item No."))?;
                if item.str("Type") != "Inventory" {
                    refuse!("Type must be equal to 'Inventory' in Item: No.={}. Current value is '{}'.", item.str("No."), item.str("Type"));
                }
                fill_from_item(ctx, r, &item, true);
            } else {
                r.set("Qty. Expected (Base)", dec(Decimal::ZERO));
                r.set("Qty. Exp. Calculated", Value::Bool(false));
            }
            Ok(())
        }
        (RECORD_LINE, _) => {
            let key = Key(vec![r.get("Order No.").clone(), r.get("Recording No.").clone()]);
            if ctx.get(RECORD_HEADER, &key)?.str("Status") != "Open" {
                refuse!("Status must be equal to 'Open' in Phys. Invt. Record Header: Order No.={}, Recording No.={}. Current value is 'Finished'.", r.str("Order No."), r.get("Recording No.").as_int());
            }
            match field {
                "Item No." if !r.str("Item No.").is_empty() => {
                    let item = ctx.get_code(ITEM, r.str("Item No."))?;
                    fill_from_item(ctx, r, &item, false);
                }
                "Unit of Measure Code" if !r.str("Item No.").is_empty() => {
                    let item = ctx.get_code(ITEM, r.str("Item No."))?;
                    let per = crate::triggers::qty_per_uom(ctx, &item, r.str("Unit of Measure Code"))?;
                    r.set("Qty. per Unit of Measure", dec(per));
                    r.set("Quantity (Base)", dec(r.get("Quantity").as_dec() * per));
                }
                "Quantity" => {
                    let q = r.get("Quantity").as_dec();
                    if q < Decimal::ZERO {
                        refuse!("Quantity must not be negative.");
                    }
                    let per = match r.get("Qty. per Unit of Measure").as_dec() {
                        d if d.is_zero() => Decimal::ONE,
                        d => d,
                    };
                    r.set("Quantity (Base)", dec(q * per));
                    r.set("Recorded", Value::Bool(true));
                    r.set("Date Recorded", Value::Date(ctx.work_date));
                    r.set("Recorded by User ID", Value::text(ctx.user));
                }
                _ => {}
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ---- commands ----------------------------------------------------------------------------------

/// A new order line for an item at a location, with its expected quantity.
fn add_line(ctx: &mut Ctx, no: &str, item: &Record, location: &str) -> Result<()> {
    let lt = ctx.table(ORDER_LINE)?;
    let mut values = Map::new();
    values.insert("Document No.".into(), json!(no));
    values.insert("Location Code".into(), json!(location));
    let mut r = crate::records::create(ctx, lt, &values, None)?;
    r.set("Item No.", item.get("No.").clone());
    fill_from_item(ctx, &mut r, item, true);
    r.set("Location Code", Value::text(location));
    r.set("Qty. Expected (Base)", dec(on_hand(ctx, item.str("No."), location)));
    r.set("Qty. Exp. Calculated", Value::Bool(true));
    r.set("Last Item Ledger Entry No.", Value::Int(last_entry(ctx, item.str("No."), location)));
    ctx.db.modify(lt, r)?;
    Ok(())
}

/// Items (with their locations) to count: those holding stock, or every inventory item at the
/// location when zero quantities are wanted.
fn candidates(ctx: &Ctx, location: Option<&str>, zero_qty: bool, only: impl Fn(&Record) -> bool) -> Vec<(Record, String)> {
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| i.str("Type") == "Inventory" && !i.get("Blocked").as_bool() && only(i)).cloned().collect();
    let mut v = Vec::new();
    for item in items {
        let no = item.str("No.").to_string();
        let mut locs: Vec<String> = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no).map(|e| e.str("Location Code").to_string()).collect();
        locs.sort();
        locs.dedup();
        if let Some(l) = location {
            locs.retain(|x| x == l);
            if zero_qty && locs.is_empty() {
                locs.push(l.to_string());
            }
        }
        for loc in locs {
            if zero_qty || !on_hand(ctx, &no, &loc).is_zero() {
                v.push((item.clone(), loc));
            }
        }
    }
    v
}

fn fill_order(ctx: &mut Ctx, h: &Record, list: Vec<(Record, String)>) -> Result<i64> {
    let no = h.str("No.").to_string();
    let existing: Vec<(String, String)> = order_lines(ctx, &no).iter().map(|l| (l.str("Item No.").to_string(), l.str("Location Code").to_string())).collect();
    let mut n = 0;
    for (item, loc) in list {
        if existing.contains(&(item.str("No.").to_string(), loc.clone())) {
            continue;
        }
        add_line(ctx, &no, &item, &loc)?;
        n += 1;
    }
    Ok(n)
}

/// BC's Calc. Phys. Invt. Order Lines.
fn calculate_lines(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    check_open(&h)?;
    let location = opt_str(p, "location_code").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).or_else(|| Some(h.str("Location Code").to_string()).filter(|s| !s.is_empty()));
    let list = candidates(ctx, location.as_deref(), flag(p, "zero_qty"), |_| true);
    let n = fill_order(ctx, &h, list)?;
    Ok(json!({"lines": n, "message": format!("{n} line(s) were created.")}))
}

/// The next counting window after `from` for a count frequency per year (BC's
/// PhysInvtCountManagement.CalcPeriod): the year split into equal periods.
fn next_period(from: NaiveDate, frequency: i64) -> (NaiveDate, NaiveDate) {
    let freq = u32::try_from(frequency.clamp(1, 365)).unwrap_or(1);
    if 12 % freq == 0 {
        let months = 12 / freq;
        let start = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap_or(from).checked_add_months(Months::new(months)).unwrap_or(from);
        let end = start.checked_add_months(Months::new(months)).and_then(|d| d.pred_opt()).unwrap_or(start);
        (start, end)
    } else {
        let days = i64::from(365 / freq);
        let start = from + chrono::Duration::days(days);
        (start, start + chrono::Duration::days(days - 1))
    }
}

/// BC's Calculate Counting Period: lines for the items whose counting period is due on the
/// order date; their next counting window moves on.
fn calculate_counting_period(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    check_open(&h)?;
    let date = h.get("Order Date").as_date().unwrap_or(ctx.work_date);
    let due = |i: &Record| {
        !i.str("Phys Invt Counting Period Code").is_empty()
            && i.get("Next Counting Start Date").as_date().is_none_or(|s| s <= date)
            && i.get("Next Counting End Date").as_date().is_none_or(|e| e >= date || i.get("Next Counting Start Date").as_date().is_some_and(|s| s <= date))
    };
    let location = Some(h.str("Location Code").to_string()).filter(|s| !s.is_empty());
    let list = candidates(ctx, location.as_deref(), true, due);
    if list.is_empty() {
        refuse!("There are no items with a counting period due on {date}.");
    }
    let items: Vec<String> = {
        let mut v: Vec<String> = list.iter().map(|(i, _)| i.str("No.").to_string()).collect();
        v.dedup();
        v
    };
    let n = fill_order(ctx, &h, list)?;
    let it = ctx.table(ITEM)?;
    for no in items {
        let mut item = ctx.get_code(ITEM, &no)?;
        let freq = ctx.db.get_code(COUNTING_PERIOD, item.str("Phys Invt Counting Period Code")).map_or(1, |c| c.get("Count Frequency per Year").as_int());
        let (start, end) = next_period(date, freq);
        item.set("Last Counting Period Update", Value::Date(date));
        item.set("Next Counting Start Date", Value::Date(start));
        item.set("Next Counting End Date", Value::Date(end));
        ctx.db.modify(it, item)?;
    }
    Ok(json!({"lines": n, "message": format!("{n} line(s) were created.")}))
}

/// BC's Copy Phys. Invt. Order: the lines of another order (open or posted).
fn copy_document(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    check_open(&h)?;
    let from = opt_str(p, "from_no").ok_or_else(|| EngineError::invalid("missing parameter 'from_no'"))?.to_string();
    let source: Vec<(String, String)> = ctx
        .db
        .rows(ORDER_LINE)
        .filter(|l| l.str("Document No.") == from)
        .chain(ctx.db.rows(PSTD_ORDER_LINE).filter(|l| l.str("Document No.") == from))
        .map(|l| (l.str("Item No.").to_string(), l.str("Location Code").to_string()))
        .collect();
    if source.is_empty() {
        refuse!("Phys. Invt. Order {from} has no lines to copy.");
    }
    let mut list = Vec::new();
    for (item, loc) in source {
        list.push((ctx.get_code(ITEM, &item)?, loc));
    }
    let n = fill_order(ctx, &h, list)?;
    Ok(json!({"lines": n, "message": format!("{n} line(s) were copied.")}))
}

/// BC's Calc. Qty. Expected: the expected quantities again from the current inventory.
fn calc_qty_expected(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    check_open(&h)?;
    let lt = ctx.table(ORDER_LINE)?;
    for mut l in order_lines(ctx, h.str("No.")) {
        if l.str("Item No.").is_empty() {
            continue;
        }
        l.set("Qty. Expected (Base)", dec(on_hand(ctx, l.str("Item No."), l.str("Location Code"))));
        l.set("Qty. Exp. Calculated", Value::Bool(true));
        l.set("Last Item Ledger Entry No.", Value::Int(last_entry(ctx, l.str("Item No."), l.str("Location Code"))));
        ctx.db.modify(lt, l)?;
    }
    Ok(json!({"message": "The expected quantities were calculated."}))
}

/// BC's Make Phys. Invt. Recording: a recording with a line per order line.
fn make_recording(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    check_open(&h)?;
    let no = h.str("No.").to_string();
    let only_new = flag(p, "only_lines_not_in_recordings");
    let lines: Vec<Record> = order_lines(ctx, &no).into_iter().filter(|l| !l.str("Item No.").is_empty() && !(only_new && l.get("On Recording Lines").as_bool())).collect();
    if lines.is_empty() {
        refuse!("There are no lines to record on Phys. Invt. Order {no}.");
    }
    let ht = ctx.table(RECORD_HEADER)?;
    let mut values = Map::new();
    values.insert("Order No.".into(), json!(no));
    values.insert("Description".into(), json!(h.str("Description")));
    values.insert("Person Responsible".into(), json!(h.str("Person Responsible")));
    let rec = crate::records::create(ctx, ht, &values, None)?;
    let rec_no = rec.get("Recording No.").as_int();
    let rlt = ctx.table(RECORD_LINE)?;
    let olt = ctx.table(ORDER_LINE)?;
    for (i, l) in lines.iter().enumerate() {
        let mut r = rlt.init_record();
        r.set("Order No.", Value::text(&no));
        r.set("Recording No.", Value::Int(rec_no));
        r.set("Line No.", Value::Int((i64::try_from(i).unwrap_or(0) + 1) * 10000));
        r.set("Order Line No.", l.get("Line No.").clone());
        r.set("Item No.", l.get("Item No.").clone());
        r.set("Variant Code", l.get("Variant Code").clone());
        r.set("Location Code", l.get("Location Code").clone());
        r.set("Bin Code", l.get("Bin Code").clone());
        r.set("Description", l.get("Description").clone());
        r.set("Description 2", l.get("Description 2").clone());
        r.set("Unit of Measure Code", l.get("Base Unit of Measure Code").clone());
        r.set("Qty. per Unit of Measure", dec(Decimal::ONE));
        r.set("Shelf No.", l.get("Shelf No.").clone());
        ctx.db.insert(rlt, r)?;
        let mut m = l.clone();
        m.set("On Recording Lines", Value::Bool(true));
        ctx.db.modify(olt, m)?;
    }
    Ok(json!({"recording_no": rec_no, "open": {"page": "phys-inventory-recording", "key": [no, rec_no]}, "message": format!("Recording {rec_no} with {} line(s) was created.", lines.len())}))
}

/// BC's Phys. Invt. Rec.-Finish: every line recorded; the counted quantities go onto the order
/// lines (a line without one becomes a new order line, Recorded Without Order).
fn finish_recording(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut rh = recording_of(ctx, p)?;
    if rh.str("Status") != "Open" {
        refuse!("Status must be equal to 'Open' in Phys. Invt. Record Header: Order No.={}, Recording No.={}. Current value is '{}'.", rh.str("Order No."), rh.get("Recording No.").as_int(), rh.str("Status"));
    }
    let no = rh.str("Order No.").to_string();
    let rec_no = rh.get("Recording No.").as_int();
    let h = ctx.get_code(ORDER_HEADER, &no)?;
    check_open(&h)?;
    let lines: Vec<Record> = record_lines(ctx, &no, rec_no).into_iter().filter(|l| !l.str("Item No.").is_empty()).collect();
    if lines.is_empty() {
        refuse!("There are no Lines recorded.");
    }
    for l in &lines {
        if !l.get("Recorded").as_bool() {
            refuse!("Recorded must be equal to 'Yes' in Phys. Invt. Record Line: Order No.={no}, Recording No.={rec_no}, Line No.={}. Current value is 'No'.", l.get("Line No.").as_int());
        }
    }
    let olt = ctx.table(ORDER_LINE)?;
    let rlt = ctx.table(RECORD_LINE)?;
    for l in lines {
        // The order line of the same item, variant and location.
        let target = order_lines(ctx, &no)
            .into_iter()
            .find(|o| if l.get("Order Line No.").as_int() != 0 { o.get("Line No.").as_int() == l.get("Order Line No.").as_int() } else { o.str("Item No.") == l.str("Item No.") && o.str("Variant Code") == l.str("Variant Code") && o.str("Location Code") == l.str("Location Code") });
        let mut o = match target {
            Some(o) => o,
            None => {
                if !rh.get("Allow Recording Without Order").as_bool() {
                    refuse!("There is no Phys. Invt. Order Line for Item No. {} at Location {} on order {no}. Allow Recording Without Order on the recording to add it.", l.str("Item No."), l.str("Location Code"));
                }
                let item = ctx.get_code(ITEM, l.str("Item No."))?;
                add_line(ctx, &no, &item, l.str("Location Code"))?;
                let mut o = order_lines(ctx, &no).into_iter().last().ok_or_else(|| EngineError::refused("The order line could not be created."))?;
                o.set("Recorded Without Order", Value::Bool(true));
                o
            }
        };
        o.set("Qty. Recorded (Base)", dec(o.get("Qty. Recorded (Base)").as_dec() + l.get("Quantity (Base)").as_dec()));
        o.set("No. Finished Rec.-Lines", Value::Int(o.get("No. Finished Rec.-Lines").as_int() + 1));
        let mut rl = l.clone();
        rl.set("Order Line No.", o.get("Line No.").clone());
        rl.set("Recorded Without Order", o.get("Recorded Without Order").clone());
        ctx.db.modify(olt, o)?;
        ctx.db.modify(rlt, rl)?;
    }
    let rt = ctx.table(RECORD_HEADER)?;
    rh.set("Status", Value::text("Finished"));
    ctx.db.modify(rt, rh)?;
    let ht = ctx.table(ORDER_HEADER)?;
    let mut h = h;
    h.set("No. Finished Recordings", Value::Int(h.get("No. Finished Recordings").as_int() + 1));
    ctx.db.modify(ht, h)?;
    Ok(json!({"message": format!("Recording {rec_no} of order {no} is finished.")}))
}

/// BC's Phys. Invt. Rec.-Reopen: takes the recording's quantities back off the order lines.
fn reopen_recording(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut rh = recording_of(ctx, p)?;
    if rh.str("Status") != "Finished" {
        refuse!("Status must be equal to 'Finished' in Phys. Invt. Record Header: Order No.={}, Recording No.={}. Current value is '{}'.", rh.str("Order No."), rh.get("Recording No.").as_int(), rh.str("Status"));
    }
    let no = rh.str("Order No.").to_string();
    let rec_no = rh.get("Recording No.").as_int();
    let mut h = ctx.get_code(ORDER_HEADER, &no)?;
    let ht = ctx.table(ORDER_HEADER)?;
    if h.str("Status") == "Finished" {
        reopen_order(ctx, &mut h)?;
    }
    let olt = ctx.table(ORDER_LINE)?;
    for l in record_lines(ctx, &no, rec_no) {
        if let Some(mut o) = order_lines(ctx, &no).into_iter().find(|o| o.get("Line No.").as_int() == l.get("Order Line No.").as_int()) {
            o.set("Qty. Recorded (Base)", dec(o.get("Qty. Recorded (Base)").as_dec() - l.get("Quantity (Base)").as_dec()));
            o.set("No. Finished Rec.-Lines", Value::Int((o.get("No. Finished Rec.-Lines").as_int() - 1).max(0)));
            ctx.db.modify(olt, o)?;
        }
    }
    let rt = ctx.table(RECORD_HEADER)?;
    rh.set("Status", Value::text("Open"));
    ctx.db.modify(rt, rh)?;
    h.set("No. Finished Recordings", Value::Int((h.get("No. Finished Recordings").as_int() - 1).max(0)));
    ctx.db.modify(ht, h)?;
    Ok(json!({"message": format!("Recording {rec_no} of order {no} is open again.")}))
}

/// BC's Phys. Invt. Order-Finish: all recordings finished; each line's difference becomes a
/// positive or negative adjustment.
fn finish(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut h = order_of(ctx, p)?;
    check_open(&h)?;
    let no = h.str("No.").to_string();
    if let Some(r) = recordings(ctx, &no).into_iter().find(|r| r.str("Status") != "Finished") {
        refuse!("Status must be equal to 'Finished' in Phys. Invt. Record Header: Order No.={no}, Recording No.={}. Current value is 'Open'.", r.get("Recording No.").as_int());
    }
    let lt = ctx.table(ORDER_LINE)?;
    for mut l in order_lines(ctx, &no) {
        if l.str("Item No.").is_empty() {
            continue;
        }
        if !l.get("Qty. Exp. Calculated").as_bool() {
            refuse!("Qty. Exp. Calculated must be equal to 'Yes' in Phys. Invt. Order Line: Document No.={no}, Line No.={}. Current value is 'No'.", l.get("Line No.").as_int());
        }
        if l.get("No. Finished Rec.-Lines").as_int() == 0 {
            refuse!("The Phys. Invt. Order Line {} (item {}) has not been recorded.", l.get("Line No.").as_int(), l.str("Item No."));
        }
        let diff = l.get("Qty. Recorded (Base)").as_dec() - l.get("Qty. Expected (Base)").as_dec();
        let (ty, pos, neg) = if diff > Decimal::ZERO { ("Positive Adjmt.", diff, Decimal::ZERO) } else if diff < Decimal::ZERO { ("Negative Adjmt.", Decimal::ZERO, -diff) } else { ("", Decimal::ZERO, Decimal::ZERO) };
        l.set("Entry Type", Value::text(ty));
        l.set("Quantity (Base)", dec(diff.abs()));
        l.set("Pos. Qty. (Base)", dec(pos));
        l.set("Neg. Qty. (Base)", dec(neg));
        l.set("Without Difference", Value::Bool(diff.is_zero()));
        ctx.db.modify(lt, l)?;
    }
    let ht = ctx.table(ORDER_HEADER)?;
    h.set("Status", Value::text("Finished"));
    ctx.db.modify(ht, h)?;
    Ok(json!({"message": format!("Phys. Invt. Order {no} is finished.")}))
}

fn reopen_order(ctx: &mut Ctx, h: &mut Record) -> Result<()> {
    let lt = ctx.table(ORDER_LINE)?;
    for mut l in order_lines(ctx, h.str("No.")) {
        for f in ["Quantity (Base)", "Pos. Qty. (Base)", "Neg. Qty. (Base)"] {
            l.set(f, dec(Decimal::ZERO));
        }
        l.set("Entry Type", Value::text(""));
        l.set("Without Difference", Value::Bool(false));
        ctx.db.modify(lt, l)?;
    }
    h.set("Status", Value::text("Open"));
    let ht = ctx.table(ORDER_HEADER)?;
    ctx.db.modify(ht, h.clone())?;
    Ok(())
}

fn reopen(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut h = order_of(ctx, p)?;
    if h.str("Status") != "Finished" {
        refuse!("Status must be equal to 'Finished' in Phys. Invt. Order Header: No.={}. Current value is '{}'.", h.str("No."), h.str("Status"));
    }
    reopen_order(ctx, &mut h)?;
    Ok(json!({"message": format!("Phys. Invt. Order {} is open again.", h.str("No."))}))
}

fn report_rows(lines: &[Record]) -> Vec<Json> {
    lines
        .iter()
        .filter(|l| !l.str("Item No.").is_empty())
        .map(|l| {
            let n = |f: &str| l.get(f).as_dec().normalize().to_string();
            json!([l.str("Item No."), l.str("Description"), l.str("Location Code"), n("Qty. Expected (Base)"), n("Qty. Recorded (Base)"), l.str("Entry Type"), n("Pos. Qty. (Base)"), n("Neg. Qty. (Base)")])
        })
        .collect()
}

const REPORT_COLUMNS: [&str; 8] = ["Item No.", "Description", "Location Code", "Qty. Expected (Base)", "Qty. Recorded (Base)", "Entry Type", "Pos. Qty. (Base)", "Neg. Qty. (Base)"];

/// BC's Phys. Invt. Order - Test: what posting would adjust, with the order's problems.
fn test_report(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    let no = h.str("No.").to_string();
    let lines = order_lines(ctx, &no);
    let mut problems = Vec::new();
    if h.str("Status") != "Finished" {
        problems.push(json!([format!("Status must be equal to 'Finished' in Phys. Invt. Order Header: No.={no}. Current value is '{}'.", h.str("Status"))]));
    }
    for l in &lines {
        if !l.str("Item No.").is_empty() && l.get("No. Finished Rec.-Lines").as_int() == 0 {
            problems.push(json!([format!("Line {} (item {}) has not been recorded.", l.get("Line No.").as_int(), l.str("Item No."))]));
        }
    }
    Ok(json!({"report": {"title": format!("Phys. Invt. Order - Test {no}"), "columns": REPORT_COLUMNS, "rows": report_rows(&lines), "sections": [{"caption": "Warnings", "columns": ["Description"], "rows": problems}]}}))
}

/// BC's Phys. Invt. Order Statistics.
fn statistics(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    let lines: Vec<Record> = order_lines(ctx, h.str("No.")).into_iter().filter(|l| !l.str("Item No.").is_empty()).collect();
    let count = |f: &dyn Fn(&Record) -> bool| lines.iter().filter(|l| f(l)).count();
    let amount = |f: &dyn Fn(&Record) -> bool, q: &str| -> Decimal { lines.iter().filter(|l| f(l)).map(|l| crate::ctx::round_amount(l.get(q).as_dec() * l.get("Unit Amount").as_dec())).sum() };
    let pos = |l: &Record| l.str("Entry Type") == "Positive Adjmt.";
    let neg = |l: &Record| l.str("Entry Type") == "Negative Adjmt.";
    let none = |l: &Record| l.get("Without Difference").as_bool();
    let rows = vec![
        json!(["Lines", count(&|_| true).to_string(), ""]),
        json!(["Not recorded", count(&|l| l.get("No. Finished Rec.-Lines").as_int() == 0).to_string(), ""]),
        json!(["Without difference", count(&none).to_string(), "0.00"]),
        json!(["Positive difference", count(&pos).to_string(), format!("{:.2}", amount(&pos, "Pos. Qty. (Base)"))]),
        json!(["Negative difference", count(&neg).to_string(), format!("{:.2}", -amount(&neg, "Neg. Qty. (Base)"))]),
    ];
    Ok(json!({"report": {"title": format!("Phys. Invt. Order Statistics - {}", h.str("No.")), "columns": ["", "No. of Lines", "Amount"], "rows": rows}}))
}

/// BC's Phys. Invt. Order-Post: the differences post as phys. inventory adjustments; the order,
/// its lines and recordings move to the posted tables.
fn post(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let h = order_of(ctx, p)?;
    let no = h.str("No.").to_string();
    if h.str("Status") != "Finished" {
        refuse!("Status must be equal to 'Finished' in Phys. Invt. Order Header: No.={no}. Current value is '{}'.", h.str("Status"));
    }
    let date = h.get("Posting Date").as_date().ok_or_else(|| EngineError::refused(format!("Posting Date must have a value in Phys. Invt. Order Header: No.={no}.")))?;
    let lines = order_lines(ctx, &no);
    // The inventory must not have moved since the expected quantities were calculated.
    for l in lines.iter().filter(|l| !l.str("Item No.").is_empty()) {
        if last_entry(ctx, l.str("Item No."), l.str("Location Code")) != l.get("Last Item Ledger Entry No.").as_int() {
            refuse!("The inventory of item {} at location '{}' has changed since the expected quantity was calculated. Reopen the order and use Calc. Qty. Expected.", l.str("Item No."), l.str("Location Code"));
        }
    }
    let posted_no = if !h.str("Posting No.").is_empty() {
        h.str("Posting No.").to_string()
    } else if !h.str("Posting No. Series").is_empty() {
        crate::noseries::next(ctx, h.str("Posting No. Series"))?
    } else {
        no.clone()
    };
    // Through a phys. inventory journal batch of its own.
    let to_post: Vec<&Record> = lines.iter().filter(|l| !l.str("Entry Type").is_empty() && !l.get("Quantity (Base)").as_dec().is_zero()).collect();
    if !to_post.is_empty() {
        let bt = ctx.table(crate::schema::inventory::ITEM_JNL_BATCH)?;
        let mut b = bt.init_record();
        b.set("Journal Template Name", Value::text(JNL_TEMPLATE));
        b.set("Name", Value::text(JNL_BATCH));
        b.set("Description", Value::text(format!("Phys. Invt. Order {no}")));
        if ctx.db.get(&bt.name, &b.key(bt)).is_none() {
            ctx.db.insert(bt, b.clone())?;
        }
        let lt = ctx.table(crate::schema::inventory::ITEM_JNL_LINE)?;
        for (i, l) in to_post.iter().enumerate() {
            let mut values = Map::new();
            values.insert("Journal Template Name".into(), json!(JNL_TEMPLATE));
            values.insert("Journal Batch Name".into(), json!(JNL_BATCH));
            let mut r = crate::records::create(ctx, lt, &values, None)?;
            r.set("Line No.", Value::Int((i64::try_from(i).unwrap_or(0) + 1) * 10000));
            r.set("Location Code", l.get("Location Code").clone());
            crate::records::validate(ctx, lt, &mut r, "Item No.", &json!(l.str("Item No.")), false)?;
            r.set("Posting Date", Value::Date(date));
            r.set("Document No.", Value::text(&posted_no));
            r.set("Entry Type", l.get("Entry Type").clone());
            r.set("Quantity", l.get("Quantity (Base)").clone());
            r.set("Unit Amount", l.get("Unit Amount").clone());
            r.set("Amount", dec(crate::ctx::round_amount(l.get("Quantity (Base)").as_dec() * l.get("Unit Amount").as_dec())));
            r.set("Phys. Inventory", Value::Bool(true));
            r.set("Qty. (Calculated)", l.get("Qty. Expected (Base)").clone());
            r.set("Qty. (Phys. Inventory)", l.get("Qty. Recorded (Base)").clone());
            if !h.str("Gen. Bus. Posting Group").is_empty() {
                r.set("Gen. Bus. Posting Group", h.get("Gen. Bus. Posting Group").clone());
            }
            if !h.str("Reason Code").is_empty() {
                r.set("Reason Code", h.get("Reason Code").clone());
            }
            ctx.db.modify(lt, r)?;
        }
        crate::item_journal::post(ctx, JNL_TEMPLATE, JNL_BATCH)?;
        // Nothing of the batch stays behind.
        let left: Vec<Record> = ctx.db.rows(&lt.name).filter(|l| l.str("Journal Template Name") == JNL_TEMPLATE && l.str("Journal Batch Name") == JNL_BATCH).cloned().collect();
        for l in left {
            ctx.db.remove(&lt.name, &l.key(lt));
        }
        ctx.db.remove(&bt.name, &b.key(bt));
    }
    // Counted items remember the date.
    let it = ctx.table(ITEM)?;
    for l in lines.iter().filter(|l| !l.str("Item No.").is_empty()) {
        if let Some(mut item) = ctx.db.get_code(ITEM, l.str("Item No.")).cloned() {
            item.set("Last Phys. Invt. Date", Value::Date(date));
            ctx.db.modify(it, item)?;
        }
    }
    // The posted order and recordings.
    let pht = ctx.table(PSTD_ORDER_HEADER)?;
    let mut ph = pht.init_record();
    for f in &pht.fields {
        if h.get(&f.name) != &Value::default_for(&f.ty) && f.name != "No." && f.name != "No. Series" {
            ph.set(&f.name, h.get(&f.name).clone());
        }
    }
    ph.set("No.", Value::text(&posted_no));
    ph.set("Pre-Assigned No.", Value::text(&no));
    ph.set("No. Series", h.get("Posting No. Series").clone());
    ph.set("User ID", Value::text(ctx.user));
    ctx.db.insert(pht, ph)?;
    let plt = ctx.table(PSTD_ORDER_LINE)?;
    let olt = ctx.table(ORDER_LINE)?;
    for l in &lines {
        let mut pl = plt.init_record();
        for f in &plt.fields {
            pl.set(&f.name, l.get(&f.name).clone());
        }
        pl.set("Document No.", Value::text(&posted_no));
        ctx.db.insert(plt, pl)?;
        ctx.db.remove(&olt.name, &l.key(olt));
    }
    let prt = ctx.table(PSTD_RECORD_HEADER)?;
    let prlt = ctx.table(PSTD_RECORD_LINE)?;
    let rt = ctx.table(RECORD_HEADER)?;
    let rlt = ctx.table(RECORD_LINE)?;
    for r in recordings(ctx, &no) {
        let rec_no = r.get("Recording No.").as_int();
        let mut pr = prt.init_record();
        for f in &prt.fields {
            pr.set(&f.name, r.get(&f.name).clone());
        }
        pr.set("Order No.", Value::text(&posted_no));
        ctx.db.insert(prt, pr)?;
        for l in record_lines(ctx, &no, rec_no) {
            let mut pl = prlt.init_record();
            for f in &prlt.fields {
                pl.set(&f.name, l.get(&f.name).clone());
            }
            pl.set("Order No.", Value::text(&posted_no));
            ctx.db.insert(prlt, pl)?;
            ctx.db.remove(&rlt.name, &l.key(rlt));
        }
        ctx.db.remove(&rt.name, &r.key(rt));
    }
    let ht = ctx.table(ORDER_HEADER)?;
    ctx.db.remove(&ht.name, &h.key(ht));
    Ok(json!({"posted_no": posted_no, "message": format!("Phys. Invt. Order {no} was posted as {posted_no}.")}))
}

fn s_order() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

fn s_calc() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {},
        "location_code": {"type": "string", "description": "only this location (default: the order's)"},
        "zero_qty": {"type": "boolean", "description": "Items Not on Inventory: also lines for items without stock"}}})
}

fn s_copy() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}, "from_no": {"type": "string", "description": "the order (or posted order) to copy the lines of"}}, "required": ["from_no"]})
}

fn s_make() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {},
        "only_lines_not_in_recordings": {"type": "boolean", "description": "Only Lines Not in Recordings"}}})
}

fn s_recording() -> Json {
    json!({"type": "object", "properties": {"order_no": {"type": "string"}, "recording_no": {"type": "integer"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "physinvt.calculate_lines", label: "Calculate Lines", group: "Inventory", summary: "Add physical inventory lines for items that are currently recorded in inventory.", mutates: true, params: s_calc, run: calculate_lines },
    CommandDef { id: "physinvt.calculate_counting_period", label: "Calculate Counting Period", group: "Inventory", summary: "Add lines for items whose counting period is due.", mutates: true, params: s_order, run: calculate_counting_period },
    CommandDef { id: "physinvt.copy_document", label: "Copy Document", group: "Inventory", summary: "Copy the lines of another physical inventory order.", mutates: true, params: s_copy, run: copy_document },
    CommandDef { id: "physinvt.calc_qty_expected", label: "Calc. Qty. Expected", group: "Inventory", summary: "Recalculate the expected quantities from the inventory.", mutates: true, params: s_order, run: calc_qty_expected },
    CommandDef { id: "physinvt.make_recording", label: "Make New Recording", group: "Inventory", summary: "Create a recording with a line per order line.", mutates: true, params: s_make, run: make_recording },
    CommandDef { id: "physinvt.finish_recording", label: "Finish Recording", group: "Inventory", summary: "Finish a recording: its counted quantities go onto the order.", mutates: true, params: s_recording, run: finish_recording },
    CommandDef { id: "physinvt.reopen_recording", label: "Reopen Recording", group: "Inventory", summary: "Reopen a finished recording (and its order).", mutates: true, params: s_recording, run: reopen_recording },
    CommandDef { id: "physinvt.finish", label: "Finish", group: "Inventory", summary: "Finish the order: compare expected and recorded quantities.", mutates: true, params: s_order, run: finish },
    CommandDef { id: "physinvt.reopen", label: "Reopen", group: "Inventory", summary: "Reopen a finished physical inventory order.", mutates: true, params: s_order, run: reopen },
    CommandDef { id: "physinvt.test_report", label: "Test Report", group: "Inventory", summary: "The adjustments posting would make, with warnings.", mutates: false, params: s_order, run: test_report },
    CommandDef { id: "physinvt.statistics", label: "Statistics", group: "Inventory", summary: "Lines and amounts with and without differences.", mutates: false, params: s_order, run: statistics },
    CommandDef { id: "physinvt.post", label: "Post", group: "Inventory", summary: "Post the differences as inventory adjustments and file the order as posted.", mutates: true, params: s_order, run: post },
];
