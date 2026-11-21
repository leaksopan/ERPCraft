//! Item journals (BC Item Jnl.-Post Batch): positive / negative adjustments, transfers between
//! locations (reclassification), and physical inventory (Calculate Inventory, count, post the
//! difference).

use std::collections::BTreeMap;

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::ctx::{Ctx, dec, round_amount};
use crate::error::{EngineError, Result};
use crate::ledger::Txn;
use crate::noseries;
use crate::posting::{GlDoc, GlLine, ItemEntry, check_posting_date, inventory_account, post_gl, update_item_cost};
use crate::refuse;
use crate::schema::inventory::{ITEM_JNL_BATCH, ITEM_JNL_LINE, ITEM_JNL_TEMPLATE};
use crate::schema::tables::{GEN_POSTING_SETUP, INVENTORY_SETUP, ITEM, ITEM_LEDGER};

fn batch_lines<'a>(ctx: &'a Ctx, template: &'a str, batch: &'a str) -> impl Iterator<Item = &'a Record> {
    ctx.db.rows(ITEM_JNL_LINE).filter(move |l| l.str("Journal Template Name") == template && l.str("Journal Batch Name") == batch)
}

pub fn on_insert(ctx: &mut Ctx, r: &mut Record) -> Result<()> {
    let template = r.str("Journal Template Name").to_string();
    let batch = r.str("Journal Batch Name").to_string();
    let tmpl = ctx.get_code(ITEM_JNL_TEMPLATE, &template)?;
    let b = ctx.get(ITEM_JNL_BATCH, &Key(vec![Value::text(&template), Value::text(&batch)]))?;
    let last = batch_lines(ctx, &template, &batch).max_by_key(|l| l.get("Line No.").as_int()).cloned();
    if r.get("Line No.").as_int() == 0 {
        r.set("Line No.", Value::Int(last.as_ref().map_or(0, |l| l.get("Line No.").as_int()).saturating_add(10000)));
    }
    if r.get("Posting Date").as_date().is_none() {
        r.set("Posting Date", Value::Date(ctx.work_date));
    }
    if r.str("Document No.").is_empty() {
        let no = match &last {
            Some(l) if !l.str("Document No.").is_empty() => l.str("Document No.").to_string(),
            _ if !b.str("No. Series").is_empty() => noseries::peek(ctx, b.str("No. Series"))?,
            _ => String::new(),
        };
        r.set("Document No.", Value::text(no));
    }
    if let Some(l) = &last
        && r.str("Location Code").is_empty()
    {
        r.set("Location Code", l.get("Location Code").clone());
    }
    match tmpl.str("Type") {
        "Transfer" => r.set("Entry Type", Value::text("Transfer")),
        _ if r.str("Entry Type") == "Purchase" => r.set("Entry Type", Value::text("Positive Adjmt.")),
        _ => {}
    }
    r.set("Source Code", tmpl.get("Source Code").clone());
    Ok(())
}

fn recalc(r: &mut Record) {
    let amount = round_amount(r.get("Quantity").as_dec() * r.get("Unit Amount").as_dec());
    r.set("Amount", dec(amount));
}

pub fn on_validate(ctx: &mut Ctx, r: &mut Record, field: &str) -> Result<()> {
    match field {
        "Item No." => {
            let no = r.str("Item No.").to_string();
            if no.is_empty() {
                return Ok(());
            }
            let it = ctx.get_code(ITEM, &no)?;
            if it.str("Type") != "Inventory" {
                refuse!("Item {no} is of type {}. Only inventory items can be posted in an item journal.", it.str("Type"));
            }
            if it.get("Blocked").as_bool() {
                refuse!("Blocked must be equal to 'No' in Item: No.={no}. Current value is 'Yes'.");
            }
            r.set("Description", it.get("Description").clone());
            r.set("Unit of Measure Code", it.get("Base Unit of Measure").clone());
            r.set("Unit Amount", it.get("Unit Cost").clone());
            r.set("Unit Cost", it.get("Unit Cost").clone());
            r.set("Gen. Prod. Posting Group", it.get("Gen. Prod. Posting Group").clone());
            r.set("Inventory Posting Group", it.get("Inventory Posting Group").clone());
            recalc(r);
        }
        "Quantity" => {
            if r.get("Quantity").as_dec() < Decimal::ZERO {
                refuse!("Quantity must be positive. Choose the entry type (Positive Adjmt. or Negative Adjmt.) for the direction.");
            }
            recalc(r);
        }
        "Unit Amount" => {
            r.set("Unit Cost", r.get("Unit Amount").clone());
            recalc(r);
        }
        "Unit Cost (Revalued)" => {
            if r.str("Value Entry Type") != "Revaluation" {
                refuse!("Value Entry Type must be equal to 'Revaluation' in Item Journal Line: Line No.={}.", r.get("Line No.").as_int());
            }
            if r.get("Unit Cost (Revalued)").as_dec() < Decimal::ZERO {
                refuse!("Unit Cost (Revalued) must not be negative.");
            }
            let v = round_amount(r.get("Quantity").as_dec() * r.get("Unit Cost (Revalued)").as_dec());
            r.set("Inventory Value (Revalued)", dec(v));
            r.set("Unit Amount", r.get("Unit Cost (Revalued)").clone());
            r.set("Amount", dec(v));
        }
        "Inventory Value (Revalued)" => {
            if r.str("Value Entry Type") != "Revaluation" {
                refuse!("Value Entry Type must be equal to 'Revaluation' in Item Journal Line: Line No.={}.", r.get("Line No.").as_int());
            }
            let v = r.get("Inventory Value (Revalued)").as_dec();
            if v < Decimal::ZERO {
                refuse!("Inventory Value (Revalued) must not be negative.");
            }
            let q = r.get("Quantity").as_dec();
            let unit = if q.is_zero() { Decimal::ZERO } else { (v / q).round_dp(5) };
            r.set("Unit Cost (Revalued)", dec(unit));
            r.set("Unit Amount", dec(unit));
            r.set("Amount", dec(v));
        }
        "Qty. (Phys. Inventory)" => {
            let diff = r.get("Qty. (Phys. Inventory)").as_dec() - r.get("Qty. (Calculated)").as_dec();
            r.set("Entry Type", Value::text(if diff < Decimal::ZERO { "Negative Adjmt." } else { "Positive Adjmt." }));
            r.set("Quantity", dec(diff.abs()));
            recalc(r);
        }
        "Entry Type" => {
            if matches!(r.str("Entry Type"), "Purchase" | "Sale") {
                refuse!("Use purchase and sales documents for {} entries. Item journals post adjustments and transfers.", r.str("Entry Type").to_lowercase());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Stock of `item` at `location` (all locations when `None`).
pub fn stock(ctx: &Ctx, item: &str, location: Option<&str>) -> Decimal {
    ctx.db
        .rows(ITEM_LEDGER)
        .filter(|e| e.str("Item No.") == item && location.is_none_or(|l| e.str("Location Code") == l))
        .map(|e| e.get("Quantity").as_dec())
        .sum()
}

/// Inventory Setup › Prevent Negative Inventory: refuses taking `qty` out of a location that
/// doesn't hold it.
pub fn check_negative(ctx: &Ctx, item: &str, location: &str, qty_out: Decimal) -> Result<()> {
    if !ctx.setup(INVENTORY_SETUP)?.get("Prevent Negative Inventory").as_bool() || qty_out <= Decimal::ZERO {
        return Ok(());
    }
    let have = stock(ctx, item, Some(location));
    if have < qty_out {
        let at = if location.is_empty() { String::new() } else { format!(" at location {location}") };
        refuse!("You have insufficient quantity of Item {item}{at} on inventory: {have} available, {qty_out} needed.");
    }
    Ok(())
}

/// Physical inventory: one line per item and location holding stock (or every inventory item at
/// `location`), with the calculated quantity to count against.
pub fn calculate_inventory(ctx: &mut Ctx, template: &str, batch: &str, location: Option<&str>) -> Result<Json> {
    let lt = ctx.table(ITEM_JNL_LINE)?;
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| i.str("Type") == "Inventory" && !i.get("Blocked").as_bool()).cloned().collect();
    let mut per: BTreeMap<(String, String), Decimal> = BTreeMap::new();
    for e in ctx.db.rows(ITEM_LEDGER) {
        if location.is_none_or(|l| e.str("Location Code") == l) {
            *per.entry((e.str("Item No.").to_string(), e.str("Location Code").to_string())).or_default() += e.get("Quantity").as_dec();
        }
    }
    if let Some(l) = location {
        for i in &items {
            per.entry((i.str("No.").to_string(), l.to_string())).or_default();
        }
    }
    let mut created = 0;
    for ((item, loc), qty) in per {
        if qty.is_zero() && location.is_none() {
            continue;
        }
        if !items.iter().any(|i| i.str("No.") == item) {
            continue;
        }
        let mut values = Map::new();
        values.insert("Journal Template Name".into(), json!(template));
        values.insert("Journal Batch Name".into(), json!(batch));
        let mut r = crate::records::create(ctx, lt, &values, None)?;
        r.set("Location Code", Value::text(&loc));
        crate::records::validate(ctx, lt, &mut r, "Item No.", &json!(item), false)?;
        r.set("Phys. Inventory", Value::Bool(true));
        r.set("Qty. (Calculated)", dec(qty));
        r.set("Qty. (Phys. Inventory)", dec(qty));
        r.set("Quantity", dec(Decimal::ZERO));
        r.set("Entry Type", Value::text("Positive Adjmt."));
        recalc(&mut r);
        ctx.db.modify(lt, r)?;
        created += 1;
    }
    Ok(json!({"lines": created, "message": format!("{created} lines were created. Enter the counted quantities in Qty. (Phys. Inventory), then post.")}))
}

/// BC's Calculate Inventory Value (report 5899, per item): a revaluation line per item (and
/// location) with its quantity on inventory and the value of it from the value entries.
pub fn calculate_inventory_value(ctx: &mut Ctx, template: &str, batch: &str, date: chrono::NaiveDate, item_no: Option<&str>, by_location: bool) -> Result<Json> {
    let tmpl = ctx.get_code(ITEM_JNL_TEMPLATE, template)?;
    if tmpl.str("Type") != "Revaluation" {
        refuse!("Type must be equal to 'Revaluation' in Item Journal Template: Name={template}. Current value is '{}'.", tmpl.str("Type"));
    }
    let lt = ctx.table(ITEM_JNL_LINE)?;
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| i.str("Type") == "Inventory" && item_no.is_none_or(|n| i.str("No.") == n)).cloned().collect();
    let mut created = 0;
    for item in items {
        let no = item.str("No.").to_string();
        let mut per: BTreeMap<String, (Decimal, Decimal)> = BTreeMap::new();
        let entries: Vec<Record> = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no && e.get("Posting Date").as_date().is_none_or(|d| d <= date)).cloned().collect();
        for e in &entries {
            let key = if by_location { e.str("Location Code").to_string() } else { String::new() };
            let value: Decimal = ctx
                .db
                .rows(crate::costing::VALUE_ENTRY)
                .filter(|v| v.get("Item Ledger Entry No.") == e.get("Entry No.") && v.get("Valuation Date").as_date().is_none_or(|d| d <= date))
                .map(|v| v.get("Cost Amount (Actual)").as_dec())
                .sum();
            let slot = per.entry(key).or_default();
            slot.0 += e.get("Quantity").as_dec();
            slot.1 += value;
        }
        for (loc, (qty, value)) in per {
            if qty <= Decimal::ZERO {
                continue;
            }
            let unit = (value / qty).round_dp(5);
            let mut values = Map::new();
            values.insert("Journal Template Name".into(), json!(template));
            values.insert("Journal Batch Name".into(), json!(batch));
            let mut r = crate::records::create(ctx, lt, &values, None)?;
            crate::records::validate(ctx, lt, &mut r, "Item No.", &json!(no), false)?;
            r.set("Location Code", Value::text(&loc));
            r.set("Posting Date", Value::Date(date));
            r.set("Entry Type", Value::text("Positive Adjmt."));
            r.set("Value Entry Type", Value::text("Revaluation"));
            r.set("Quantity", dec(qty));
            r.set("Inventory Value (Calculated)", dec(round_amount(value)));
            r.set("Inventory Value (Revalued)", dec(round_amount(value)));
            r.set("Unit Cost (Calculated)", dec(unit));
            r.set("Unit Cost (Revalued)", dec(unit));
            r.set("Unit Amount", dec(unit));
            r.set("Amount", dec(round_amount(value)));
            ctx.db.modify(lt, r)?;
            created += 1;
        }
    }
    Ok(json!({"lines": created, "message": format!("{created} lines were created. Enter the revalued unit costs or inventory values, then post.")}))
}

fn adjustment_account(ctx: &Ctx, l: &Record) -> Result<String> {
    let key = Key(vec![l.get("Gen. Bus. Posting Group").clone(), l.get("Gen. Prod. Posting Group").clone()]);
    ctx.db
        .get(GEN_POSTING_SETUP, &key)
        .map(|r| r.str("Inventory Adjmt. Account").to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            EngineError::refused(format!(
                "Inventory Adjmt. Account must have a value in General Posting Setup: Gen. Bus. Posting Group='{}',Gen. Prod. Posting Group='{}'.",
                l.str("Gen. Bus. Posting Group"),
                l.str("Gen. Prod. Posting Group")
            ))
        })
}

pub fn post(ctx: &mut Ctx, template: &str, batch: &str) -> Result<Json> {
    crate::approvals::check_journal_post(ctx, ITEM_JNL_LINE, template, batch)?;
    let lt = ctx.table(ITEM_JNL_LINE)?;
    let mut lines: Vec<Record> = batch_lines(ctx, template, batch).cloned().collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());
    let postable: Vec<Record> = lines.iter().filter(|l| !l.get("Quantity").as_dec().is_zero()).cloned().collect();
    if postable.is_empty() && !lines.iter().any(|l| l.get("Phys. Inventory").as_bool()) {
        refuse!("There is nothing to post.");
    }
    let source_code = ctx.get_code(ITEM_JNL_TEMPLATE, template)?.str("Source Code").to_string();
    let series = ctx.get(ITEM_JNL_BATCH, &Key(vec![Value::text(template), Value::text(batch)]))?.str("No. Series").to_string();
    let mut docs: Vec<String> = Vec::new();
    for l in &postable {
        let line = l.get("Line No.");
        for f in ["Item No.", "Document No."] {
            if l.str(f).is_empty() {
                refuse!("{f} must have a value in Item Journal Line: Line No.={line}.");
            }
        }
        let date = l.get("Posting Date").as_date().ok_or_else(|| EngineError::refused(format!("Posting Date must have a value on line {line}.")))?;
        check_posting_date(ctx, date)?;
        if !docs.contains(&l.str("Document No.").to_string()) {
            docs.push(l.str("Document No.").to_string());
        }
    }
    let mut entries = 0;
    for doc in &docs {
        if !series.is_empty() && noseries::peek(ctx, &series).ok().as_deref() == Some(doc.as_str()) {
            noseries::next(ctx, &series)?;
        }
        let txn = Txn::new(ctx, &source_code, template, batch);
        let mut gl: Vec<GlLine> = Vec::new();
        let doc_lines: Vec<&Record> = postable.iter().filter(|l| l.str("Document No.") == doc).collect();
        let date = doc_lines.first().and_then(|l| l.get("Posting Date").as_date()).unwrap_or(ctx.work_date);
        for l in doc_lines {
            let item = ctx.get_code(ITEM, l.str("Item No."))?;
            let qty = l.get("Quantity").as_dec();
            let loc = l.str("Location Code").to_string();
            let desc = l.str("Description").to_string();
            let unit = l.get("Unit Amount").as_dec();
            if l.str("Value Entry Type") == "Revaluation" {
                // The difference in value goes onto the inventory as a revaluation value entry.
                let diff = round_amount(l.get("Inventory Value (Revalued)").as_dec() - l.get("Inventory Value (Calculated)").as_dec());
                if !diff.is_zero() {
                    let applies = l.get("Applies-to Entry").as_int();
                    crate::costing::revalue(ctx, &item, &loc, applies, date, doc, &desc, diff)?;
                    let inv = inventory_account(ctx, &item, if loc.is_empty() { "" } else { &loc })?;
                    let adj = adjustment_account(ctx, l)?;
                    gl.push(GlLine { account: inv, amount: diff, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                    gl.push(GlLine { account: adj, amount: -diff, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                }
                // The item's cost follows (BC: the revalued unit cost for standard and average items).
                let it = ctx.table(ITEM)?;
                let mut i = ctx.get_code(ITEM, item.str("No."))?;
                if i.str("Costing Method") == "Standard" {
                    i.set("Standard Cost", l.get("Unit Cost (Revalued)").clone());
                }
                if loc.is_empty() {
                    i.set("Unit Cost", l.get("Unit Cost (Revalued)").clone());
                }
                ctx.db.modify(it, i)?;
                entries += 1;
                continue;
            }
            match l.str("Entry Type") {
                t @ ("Positive Adjmt." | "Negative Adjmt.") => {
                    let positive = t == "Positive Adjmt.";
                    if !positive {
                        check_negative(ctx, item.str("No."), &loc, qty)?;
                    }
                    let signed = if positive { qty } else { -qty };
                    let cost = round_amount(signed * unit);
                    let source = crate::tracking::source_of(ITEM_JNL_LINE, l);
                    let parts = crate::tracking::parts_for(ctx, &item, source.as_ref(), &loc, signed)?;
                    crate::tracking::post_parts(ctx, &ItemEntry { item: &item, posting_date: date, entry_type: t, doc_type: "", doc_no: doc, description: &desc, location: &loc, qty: signed, cost, sales_amount: Decimal::ZERO, uom: l.get("Unit of Measure Code"), source_type: "", source_no: "", variant: l.str("Variant Code") }, &parts)?;
                    if let Some(s) = &source {
                        crate::tracking::consume(ctx, s)?;
                    }
                    if positive && !qty.is_zero() {
                        update_item_cost(ctx, item.str("No."), qty, cost)?;
                    }
                    let inv = inventory_account(ctx, &item, &loc)?;
                    let adj = adjustment_account(ctx, l)?;
                    gl.push(GlLine { account: inv, amount: cost, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                    gl.push(GlLine { account: adj, amount: -cost, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                }
                "Transfer" => {
                    let to = l.str("New Location Code").to_string();
                    if to == loc {
                        refuse!("New Location Code must differ from Location Code on line {}.", l.get("Line No."));
                    }
                    check_negative(ctx, item.str("No."), &loc, qty)?;
                    let cost = round_amount(qty * item.get("Unit Cost").as_dec());
                    let source = crate::tracking::source_of(ITEM_JNL_LINE, l);
                    let out = crate::tracking::parts_for(ctx, &item, source.as_ref(), &loc, -qty)?;
                    let inbound: Vec<crate::tracking::Part> = out.iter().map(|p| crate::tracking::Part { qty: -p.qty, ..p.clone() }).collect();
                    crate::tracking::post_parts(ctx, &ItemEntry { item: &item, posting_date: date, entry_type: "Transfer", doc_type: "", doc_no: doc, description: &desc, location: &loc, qty: -qty, cost: -cost, sales_amount: Decimal::ZERO, uom: l.get("Unit of Measure Code"), source_type: "", source_no: "", variant: l.str("Variant Code") }, &out)?;
                    crate::tracking::post_parts(ctx, &ItemEntry { item: &item, posting_date: date, entry_type: "Transfer", doc_type: "", doc_no: doc, description: &desc, location: &to, qty, cost, sales_amount: Decimal::ZERO, uom: l.get("Unit of Measure Code"), source_type: "", source_no: "", variant: l.str("Variant Code") }, &inbound)?;
                    if let Some(s) = &source {
                        crate::tracking::consume(ctx, s)?;
                    }
                    let (from_acc, to_acc) = (inventory_account(ctx, &item, &loc)?, inventory_account(ctx, &item, &to)?);
                    if from_acc != to_acc {
                        gl.push(GlLine { account: to_acc, amount: cost, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                        gl.push(GlLine { account: from_acc, amount: -cost, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" });
                    }
                }
                other => refuse!("{other} entries can't be posted from an item journal. Use purchase and sales documents."),
            }
            entries += 1;
        }
        post_gl(ctx, &GlDoc { posting_date: date, doc_type: "", doc_no: doc, ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    }
    for l in &lines {
        ctx.db.remove(&lt.name, &l.key(lt));
    }
    Ok(json!({"posted_lines": entries, "message": "The journal lines were successfully posted."}))
}
