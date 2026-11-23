//! Transfer orders (BC's TransferOrder-Post Shipment / Receipt / Transfer): stock leaves the
//! from-location for the in-transit location when shipped and arrives at the to-location when
//! received; a direct transfer does both at once. Item ledger entries are Transfer entries at
//! the item's unit cost.

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::posting::ItemEntry;
use crate::refuse;
use crate::schema::inventory::LOCATION;
use crate::schema::tables::{INVENTORY_SETUP, ITEM};
use crate::schema::transfer::{TRANSFER_HEADER, TRANSFER_LINE, TRANSFER_RCPT_HEADER, TRANSFER_RCPT_LINE, TRANSFER_SHPT_HEADER, TRANSFER_SHPT_LINE};

fn header_of(ctx: &Ctx, p: &Json) -> Result<Record> {
    let no = match (opt_str(p, "table"), p.get("key")) {
        (Some(TRANSFER_HEADER), Some(k)) => k.as_str().map(str::to_string).or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_str).map(str::to_string)),
        _ => opt_str(p, "no").map(str::to_string),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'no'"))?;
    ctx.get_code(TRANSFER_HEADER, &no)
}

fn lines_of(ctx: &Ctx, no: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(TRANSFER_LINE).filter(|l| l.str("Document No.") == no).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

fn per(l: &Record) -> Decimal {
    match l.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    }
}

// ---- triggers ----------------------------------------------------------------------------------

pub fn on_insert(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<bool> {
    match t.name.as_str() {
        TRANSFER_HEADER => {
            if r.str("No.").is_empty() {
                let series = crate::noseries::series_from_setup(ctx, INVENTORY_SETUP, "Transfer Order Nos.")?;
                r.set("No.", Value::text(crate::noseries::next(ctx, &series)?));
            }
            for f in ["Posting Date", "Shipment Date", "Receipt Date"] {
                if r.get(f).as_date().is_none() {
                    r.set(f, Value::Date(ctx.work_date));
                }
            }
            if r.str("In-Transit Code").is_empty()
                && let Some(l) = ctx.db.rows(LOCATION).find(|l| l.get("Use As In-Transit").as_bool())
            {
                r.set("In-Transit Code", l.get("Code").clone());
            }
            r.set("Status", Value::text("Open"));
            Ok(true)
        }
        TRANSFER_LINE => {
            if r.get("Line No.").as_int() == 0 {
                let no = r.str("Document No.").to_string();
                r.set("Line No.", Value::Int(lines_of(ctx, &no).iter().map(|l| l.get("Line No.").as_int()).max().unwrap_or(0) + 10000));
            }
            let h = ctx.get_code(TRANSFER_HEADER, r.str("Document No."))?;
            if h.str("Status") != "Open" {
                refuse!("Status must be equal to 'Open' in Transfer Header: No.={}. Current value is '{}'.", h.str("No."), h.str("Status"));
            }
            for f in ["Transfer-from Code", "Transfer-to Code", "In-Transit Code", "Shipment Date", "Receipt Date"] {
                r.set(f, h.get(f).clone());
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn update_quantities(r: &mut Record) {
    let qty = r.get("Quantity").as_dec();
    let shipped = r.get("Quantity Shipped").as_dec();
    let received = r.get("Quantity Received").as_dec();
    r.set("Quantity (Base)", dec(qty * per(r)));
    r.set("Outstanding Quantity", dec(qty - shipped));
    r.set("Qty. to Ship", dec(qty - shipped));
    r.set("Qty. in Transit", dec(shipped - received));
    r.set("Qty. to Receive", dec(shipped - received));
    r.set("Completely Shipped", Value::Bool(!qty.is_zero() && shipped == qty));
    r.set("Completely Received", Value::Bool(!qty.is_zero() && received == qty));
}

pub fn on_validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    match (t.name.as_str(), field) {
        (TRANSFER_HEADER, "Transfer-from Code" | "Transfer-to Code") => {
            if r.str("Status") != "Open" {
                refuse!("Status must be equal to 'Open' in Transfer Header: No.={}. Current value is '{}'.", r.str("No."), r.str("Status"));
            }
            let code = r.str(field).to_string();
            if !code.is_empty() {
                let loc = ctx.get_code(LOCATION, &code)?;
                if loc.get("Use As In-Transit").as_bool() {
                    refuse!("{field} must not be an in-transit location: {code} is one.");
                }
                let name_field = if field == "Transfer-from Code" { "Transfer-from Name" } else { "Transfer-to Name" };
                r.set(name_field, loc.get("Name").clone());
            }
            if !r.str("Transfer-from Code").is_empty() && r.str("Transfer-from Code") == r.str("Transfer-to Code") {
                refuse!("Transfer-from Code and Transfer-to Code cannot be the same.");
            }
            let lt = ctx.table(TRANSFER_LINE)?;
            for mut l in lines_of(ctx, r.str("No.")) {
                if !l.get("Quantity Shipped").as_dec().is_zero() {
                    refuse!("You cannot change {field} because the order has shipped lines.");
                }
                l.set(field, r.get(field).clone());
                ctx.db.modify(lt, l)?;
            }
        }
        (TRANSFER_HEADER, "In-Transit Code") => {
            let code = r.str("In-Transit Code").to_string();
            if !code.is_empty() && !ctx.get_code(LOCATION, &code)?.get("Use As In-Transit").as_bool() {
                refuse!("Use As In-Transit must be equal to 'Yes' in Location: Code={code}. Current value is 'No'.");
            }
        }
        (TRANSFER_LINE, "Item No.") => {
            let no = r.str("Item No.").to_string();
            if no.is_empty() {
                return Ok(());
            }
            let item = ctx.get_code(ITEM, &no)?;
            if item.str("Type") != "Inventory" && !item.str("Type").is_empty() {
                refuse!("Type must be equal to 'Inventory' in Item: No.={no}. Current value is '{}'.", item.str("Type"));
            }
            r.set("Description", item.get("Description").clone());
            r.set("Unit of Measure Code", item.get("Base Unit of Measure").clone());
            r.set("Qty. per Unit of Measure", dec(Decimal::ONE));
            update_quantities(r);
        }
        (TRANSFER_LINE, "Unit of Measure Code") => {
            let item = ctx.get_code(ITEM, r.str("Item No."))?;
            r.set("Qty. per Unit of Measure", dec(crate::triggers::qty_per_uom(ctx, &item, r.str("Unit of Measure Code"))?));
            update_quantities(r);
        }
        (TRANSFER_LINE, "Quantity") => {
            if r.get("Quantity").as_dec() < r.get("Quantity Shipped").as_dec() {
                refuse!("Quantity must not be less than the quantity already shipped ({}).", r.get("Quantity Shipped").as_dec());
            }
            update_quantities(r);
        }
        (TRANSFER_LINE, "Qty. to Ship") => {
            let max = r.get("Quantity").as_dec() - r.get("Quantity Shipped").as_dec();
            if r.get("Qty. to Ship").as_dec() > max {
                refuse!("You cannot ship more than {max} units.");
            }
        }
        (TRANSFER_LINE, "Qty. to Receive") => {
            let max = r.get("Qty. in Transit").as_dec();
            if r.get("Qty. to Receive").as_dec() > max {
                refuse!("You cannot receive more than {max} units.");
            }
        }
        _ => {}
    }
    Ok(())
}

// ---- commands -------------------------------------------------------------------------------------

fn set_status(ctx: &mut Ctx, p: &Json, status: &str) -> Result<Json> {
    let mut h = header_of(ctx, p)?;
    if status == "Released" {
        check_ready(ctx, &h)?;
    }
    h.set("Status", Value::text(status));
    let t = ctx.table(TRANSFER_HEADER)?;
    let no = h.str("No.").to_string();
    ctx.db.modify(t, h)?;
    Ok(json!({"status": status, "message": format!("Transfer order {no} is {}.", status.to_lowercase())}))
}

fn check_ready(ctx: &Ctx, h: &Record) -> Result<()> {
    for f in ["Transfer-from Code", "Transfer-to Code"] {
        if h.str(f).is_empty() {
            refuse!("{f} must have a value in Transfer Header: No.={}. It cannot be zero or empty.", h.str("No."));
        }
    }
    if !h.get("Direct Transfer").as_bool() && h.str("In-Transit Code").is_empty() {
        refuse!("In-Transit Code must have a value in Transfer Header: No.={}. It cannot be zero or empty.", h.str("No."));
    }
    if !lines_of(ctx, h.str("No.")).iter().any(|l| !l.get("Quantity").as_dec().is_zero()) {
        refuse!("There is nothing to release for transfer order {}.", h.str("No."));
    }
    Ok(())
}

fn release(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    set_status(ctx, p, "Released")
}

fn reopen(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    set_status(ctx, p, "Open")
}

/// The posted document header and lines for one shipment / receipt.
#[allow(clippy::too_many_arguments)]
fn file_posted(ctx: &mut Ctx, tables: (&str, &str), series_field: &str, h: &Record, lines: &[(Record, Decimal)]) -> Result<String> {
    let series = crate::noseries::series_from_setup(ctx, INVENTORY_SETUP, series_field)?;
    let no = crate::noseries::next(ctx, &series)?;
    let (ht, lt) = (ctx.table(tables.0)?, ctx.table(tables.1)?);
    let mut ph = ht.init_record();
    crate::posting::copy_common(h, ht, &mut ph);
    ph.set("No.", Value::text(&no));
    ph.set("Transfer Order No.", h.get("No.").clone());
    ctx.db.insert(ht, ph)?;
    for (l, q) in lines {
        let mut pl = lt.init_record();
        crate::posting::copy_common(l, lt, &mut pl);
        pl.set("Document No.", Value::text(&no));
        pl.set("Quantity", dec(*q));
        pl.set("Quantity (Base)", dec(*q * per(l)));
        ctx.db.insert(lt, pl)?;
    }
    Ok(no)
}

struct StockMove<'a> {
    header: &'a Record,
    line: &'a Record,
    from: &'a str,
    to: &'a str,
    qty: Decimal,
    doc_type: &'a str,
    doc_no: &'a str,
    shipping: bool,
}

fn move_stock(ctx: &mut Ctx, mv: &StockMove<'_>) -> Result<()> {
    let item = ctx.get_code(ITEM, mv.line.str("Item No."))?;
    let base = mv.qty * per(mv.line);
    crate::item_journal::check_negative(ctx, item.str("No."), mv.from, base)?;
    let cost = round_amount(base * item.get("Unit Cost").as_dec());
    let date = mv.header.get("Posting Date").as_date().unwrap_or(ctx.work_date);
    let uom = item.get("Base Unit of Measure").clone();
    // Lots / serials: the line's tracking when shipping, what is in transit when receiving.
    let source = crate::tracking::source_of(TRANSFER_LINE, mv.line);
    let out = if mv.shipping { crate::tracking::parts_for(ctx, &item, source.as_ref(), mv.from, -base)? } else { crate::tracking::parts_from_stock(ctx, &item, mv.from, -base) };
    let inbound: Vec<crate::tracking::Part> = out.iter().map(|p| crate::tracking::Part { qty: -p.qty, ..p.clone() }).collect();
    for (loc, q, c, parts) in [(mv.from, -base, -cost, &out), (mv.to, base, cost, &inbound)] {
        crate::tracking::post_parts(
            ctx,
            &ItemEntry { item: &item, posting_date: date, entry_type: "Transfer", doc_type: mv.doc_type, doc_no: mv.doc_no, description: mv.line.str("Description"), location: loc, qty: q, cost: c, sales_amount: Decimal::ZERO, uom: &uom, source_type: "", source_no: "", variant: mv.line.str("Variant Code") },
            parts,
        )?;
    }
    if mv.shipping && let Some(s) = &source {
        crate::tracking::consume(ctx, s)?;
    }
    Ok(())
}

/// BC's transfer posting: Ship, Receive, or (direct transfers) both at once.
fn post(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut h = header_of(ctx, p)?;
    check_ready(ctx, &h)?;
    let date = h.get("Posting Date").as_date().unwrap_or(ctx.work_date);
    crate::posting::check_posting_date(ctx, date)?;
    let no = h.str("No.").to_string();
    let direct = h.get("Direct Transfer").as_bool();
    let lines = lines_of(ctx, &no);
    let to_ship: Vec<(Record, Decimal)> = lines.iter().filter(|l| l.get("Qty. to Ship").as_dec() > Decimal::ZERO).map(|l| (l.clone(), l.get("Qty. to Ship").as_dec())).collect();
    let to_receive: Vec<(Record, Decimal)> = lines.iter().filter(|l| l.get("Qty. to Receive").as_dec() > Decimal::ZERO).map(|l| (l.clone(), l.get("Qty. to Receive").as_dec())).collect();
    let mode = opt_str(p, "mode").unwrap_or(if direct { "Ship and Receive" } else if to_ship.is_empty() { "Receive" } else { "Ship" }).to_string();
    let (ship, receive) = match mode.as_str() {
        "Ship" => (true, false),
        "Receive" => (false, true),
        _ => (true, true),
    };
    let from = h.str("Transfer-from Code").to_string();
    let to = h.str("Transfer-to Code").to_string();
    let transit = if direct { to.clone() } else { h.str("In-Transit Code").to_string() };
    let lt = ctx.table(TRANSFER_LINE)?;
    let mut result = json!({});
    if ship {
        if to_ship.is_empty() {
            refuse!("There is nothing to post.");
        }
        let shpt = file_posted(ctx, (TRANSFER_SHPT_HEADER, TRANSFER_SHPT_LINE), "Posted Transfer Shpt. Nos.", &h, &to_ship)?;
        for (l, q) in &to_ship {
            move_stock(ctx, &StockMove { header: &h, line: l, from: &from, to: &transit, qty: *q, doc_type: "Transfer Shipment", doc_no: &shpt, shipping: true })?;
            let mut nl = ctx.get(TRANSFER_LINE, &l.key(lt))?;
            nl.set("Quantity Shipped", dec(nl.get("Quantity Shipped").as_dec() + *q));
            if direct {
                nl.set("Quantity Received", dec(nl.get("Quantity Received").as_dec() + *q));
            }
            update_quantities(&mut nl);
            ctx.db.modify(lt, nl)?;
        }
        h.set("Last Shipment No.", Value::text(&shpt));
        result["shipment_no"] = json!(shpt);
    }
    if receive && !direct {
        // What is in transit now (a ship and receive receives what was just shipped too).
        let lines = lines_of(ctx, &no);
        let rcv: Vec<(Record, Decimal)> = if ship {
            lines.iter().filter(|l| l.get("Qty. in Transit").as_dec() > Decimal::ZERO).map(|l| (l.clone(), l.get("Qty. in Transit").as_dec())).collect()
        } else {
            to_receive.iter().map(|(l, q)| (ctx.get(TRANSFER_LINE, &l.key(lt)).unwrap_or_else(|_| l.clone()), *q)).collect()
        };
        if rcv.is_empty() {
            refuse!("There is nothing to post.");
        }
        for (l, q) in &rcv {
            if *q > l.get("Qty. in Transit").as_dec() {
                refuse!("You cannot receive more than {} units of {} on line {}.", l.get("Qty. in Transit").as_dec(), l.str("Item No."), l.get("Line No.").as_int());
            }
        }
        let rcpt = file_posted(ctx, (TRANSFER_RCPT_HEADER, TRANSFER_RCPT_LINE), "Posted Transfer Rcpt. Nos.", &h, &rcv)?;
        for (l, q) in &rcv {
            move_stock(ctx, &StockMove { header: &h, line: l, from: &transit, to: &to, qty: *q, doc_type: "Transfer Receipt", doc_no: &rcpt, shipping: false })?;
            let mut nl = ctx.get(TRANSFER_LINE, &l.key(lt))?;
            nl.set("Quantity Received", dec(nl.get("Quantity Received").as_dec() + *q));
            update_quantities(&mut nl);
            ctx.db.modify(lt, nl)?;
        }
        h.set("Last Receipt No.", Value::text(&rcpt));
        result["receipt_no"] = json!(rcpt);
    }
    // A fully received order is done.
    let lines = lines_of(ctx, &no);
    let ht = ctx.table(TRANSFER_HEADER)?;
    if lines.iter().all(|l| l.get("Quantity Received").as_dec() == l.get("Quantity").as_dec()) {
        for l in &lines {
            ctx.db.remove(&lt.name, &l.key(lt));
        }
        ctx.db.remove(&ht.name, &Key(vec![Value::text(&no)]));
        result["message"] = json!(format!("Transfer order {no} is fully shipped and received."));
    } else {
        h.set("Status", Value::text("Released"));
        ctx.db.modify(ht, h)?;
        result["message"] = json!(format!("Transfer order {no} was posted."));
    }
    Ok(result)
}

fn s_doc() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "table": {"type": "string"}, "key": {}}})
}

fn s_post() -> Json {
    json!({"type": "object", "properties": {"no": {"type": "string"}, "mode": {"type": "string", "enum": ["Ship", "Receive", "Ship and Receive"], "default": "Ship"}, "table": {"type": "string"}, "key": {}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "transfer.post", label: "Post", group: "Inventory", summary: "Ship and / or receive a transfer order (a direct transfer does both).", mutates: true, params: s_post, run: post },
    CommandDef { id: "transfer.release", label: "Release", group: "Inventory", summary: "Release a transfer order.", mutates: true, params: s_doc, run: release },
    CommandDef { id: "transfer.reopen", label: "Reopen", group: "Inventory", summary: "Reopen a released transfer order.", mutates: true, params: s_doc, run: reopen },
];
