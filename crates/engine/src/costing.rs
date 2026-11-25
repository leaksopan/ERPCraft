//! Costing: Value Entries (BC 5802) behind every item ledger entry, Adjust Cost - Item Entries
//! (report 795) across Average / FIFO / LIFO / Specific / Standard, and Post Inventory Cost to
//! G/L (report 1002) for actual cost entries not yet on the G/L.

use std::collections::{HashMap, VecDeque};

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec, round_amount, round_unit};
use crate::error::Result;
use crate::ledger::Txn;
use crate::posting::{GlDoc, GlLine, post_gl};
use crate::refuse;
use crate::schema::tables::{CUSTOMER, GEN_POSTING_SETUP, INVENTORY_SETUP, ITEM, ITEM_LEDGER, VENDOR};

pub const VALUE_ENTRY: &str = "Value Entry";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostRecognition {
    Actual { posted_to_gl: bool },
    Expected,
}

pub struct ReceiptCostSettlement<'a> {
    pub receipt_no: &'a str,
    pub receipt_line_no: i64,
    pub first_ile_no: i64,
    pub qty_base: Decimal,
    pub actual_cost_lcy: Decimal,
    pub inv_doc_type: &'a str,
    pub inv_doc_no: &'a str,
    pub inv_line_no: i64,
    pub posting_date: NaiveDate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettlementDelta {
    pub net_cost_delta_lcy: Decimal,
    pub settled_qty_base: Decimal,
}

pub struct DirectCostEntry<'a> {
    pub ile_no: i64,
    pub charge_no: &'a str,
    pub cost_lcy: Decimal,
    pub sales_amount_lcy: Decimal,
    pub doc_type: &'a str,
    pub doc_no: &'a str,
    pub doc_line_no: i64,
    pub posting_date: NaiveDate,
}

pub fn entry_valuation(ile: &Record) -> Decimal {
    ile.get("Cost Amount (Actual)").as_dec() + ile.get("Cost Amount (Expected)").as_dec()
}

fn gen_bus_group(ctx: &Ctx, source_type: &str, source_no: &str) -> String {
    match source_type {
        "Customer" => ctx.db.get_code(CUSTOMER, source_no).map(|c| c.str("Gen. Bus. Posting Group").to_string()).unwrap_or_default(),
        "Vendor" => ctx.db.get_code(VENDOR, source_no).map(|c| c.str("Gen. Bus. Posting Group").to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

fn initial_value_entry(ctx: &Ctx, ile_no: i64) -> Option<Record> {
    ctx.db.rows(VALUE_ENTRY).find(|v| v.get("Item Ledger Entry No.").as_int() == ile_no && v.str("Entry Type") != "Revaluation").cloned()
}

fn init_direct_value_entry(ctx: &Ctx, ile: &Record, src: Option<&Record>) -> Result<Record> {
    let vt = ctx.table(VALUE_ENTRY)?;
    let mut v = vt.init_record();
    for f in ["Item No.", "Variant Code", "Location Code", "Source Type", "Source No.", "Description"] {
        v.set(f, ile.get(f).clone());
    }
    v.set("Item Ledger Entry Type", ile.get("Entry Type").clone());
    v.set("Item Ledger Entry No.", ile.get("Entry No.").clone());
    v.set("Entry Type", Value::text("Direct Cost"));
    v.set("Valuation Date", ile.get("Posting Date").clone());
    v.set("User ID", Value::text(ctx.user.to_uppercase()));
    if let Some(s) = src {
        for f in ["Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "Inventory Posting Group"] {
            v.set(f, s.get(f).clone());
        }
    } else {
        let item = ctx.get_code(ITEM, ile.str("Item No."))?;
        v.set("Gen. Bus. Posting Group", Value::text(gen_bus_group(ctx, ile.str("Source Type"), ile.str("Source No."))));
        v.set("Gen. Prod. Posting Group", item.get("Gen. Prod. Posting Group").clone());
        v.set("Inventory Posting Group", item.get("Inventory Posting Group").clone());
    }
    Ok(v)
}

fn append_value_entry(ctx: &mut Ctx, ile: &mut Record, mut ve: Record) -> Result<Record> {
    let vt = ctx.table(VALUE_ENTRY)?;
    if ve.get("Entry No.").as_int() == 0 {
        ve.set("Entry No.", Value::Int(ctx.db.next_entry_no(VALUE_ENTRY)));
    }
    let new_actual = ile.get("Cost Amount (Actual)").as_dec() + ve.get("Cost Amount (Actual)").as_dec();
    let new_expected = ile.get("Cost Amount (Expected)").as_dec() + ve.get("Cost Amount (Expected)").as_dec();
    let new_invoiced = ile.get("Invoiced Quantity").as_dec() + ve.get("Invoiced Quantity").as_dec();
    let ile_qty = ile.get("Quantity").as_dec();
    let completely = if ile_qty > Decimal::ZERO { new_invoiced == ile_qty } else { true };
    ile.set("Cost Amount (Actual)", dec(new_actual));
    ile.set("Cost Amount (Expected)", dec(new_expected));
    ile.set("Invoiced Quantity", dec(new_invoiced));
    ile.set("Completely Invoiced", Value::Bool(completely));
    ctx.db.insert(vt, ve.clone())?;
    let it = ctx.table(ITEM_LEDGER)?;
    ctx.db.modify(it, ile.clone())?;
    Ok(ve)
}

/// The value entry of a new item ledger entry (actual or expected cost).
pub fn on_item_entry(ctx: &mut Ctx, ile: &mut Record, cost: Decimal, recognition: CostRecognition) -> Result<()> {
    let qty = ile.get("Quantity").as_dec();
    let mut v = init_direct_value_entry(ctx, ile, None)?;
    for f in ["Posting Date", "Document Type", "Document No.", "Document Line No."] {
        v.set(f, ile.get(f).clone());
    }
    v.set("Item Ledger Entry Quantity", dec(qty));
    v.set("Valued Quantity", dec(qty));
    v.set("Cost per Unit", dec(if qty.is_zero() { Decimal::ZERO } else { round_unit(cost / qty) }));
    match recognition {
        CostRecognition::Actual { posted_to_gl } => {
            v.set("Expected Cost", Value::Bool(false));
            v.set("Invoiced Quantity", dec(qty));
            v.set("Cost Amount (Actual)", dec(cost));
            v.set("Cost Amount (Expected)", dec(Decimal::ZERO));
            v.set("Cost Posted to G/L", dec(if posted_to_gl { cost } else { Decimal::ZERO }));
        }
        CostRecognition::Expected => {
            v.set("Expected Cost", Value::Bool(true));
            v.set("Invoiced Quantity", dec(Decimal::ZERO));
            v.set("Cost Amount (Actual)", dec(Decimal::ZERO));
            v.set("Cost Amount (Expected)", dec(cost));
            v.set("Cost Posted to G/L", dec(Decimal::ZERO));
        }
    }
    v.set("Sales Amount (Actual)", ile.get("Sales Amount (Actual)").clone());
    append_value_entry(ctx, ile, v)?;
    Ok(())
}

pub fn doc_line_iles(ctx: &Ctx, doc_type: &str, doc_no: &str, doc_line_no: i64, fallback_ile_no: i64) -> Vec<Record> {
    let mut matched: Vec<Record> = if doc_line_no != 0 {
        ctx.db
            .rows(ITEM_LEDGER)
            .filter(|e| e.str("Document Type") == doc_type && e.str("Document No.") == doc_no && e.get("Document Line No.").as_int() == doc_line_no)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    if matched.is_empty()
        && fallback_ile_no != 0
        && let Some(ile) = ctx.db.rows(ITEM_LEDGER).find(|e| e.get("Entry No.").as_int() == fallback_ile_no)
    {
        matched.push(ile.clone());
    }
    matched.sort_by_key(|e| e.get("Entry No.").as_int());
    matched
}

pub fn receipt_line_iles(ctx: &Ctx, receipt_no: &str, receipt_line_no: i64, fallback_ile_no: i64) -> Vec<Record> {
    doc_line_iles(ctx, "Purchase Receipt", receipt_no, receipt_line_no, fallback_ile_no)
}

pub fn settle_purchase_receipt(ctx: &mut Ctx, req: &ReceiptCostSettlement<'_>) -> Result<SettlementDelta> {
    if req.qty_base <= Decimal::ZERO {
        return Ok(SettlementDelta::default());
    }
    let open: Vec<Record> = receipt_line_iles(ctx, req.receipt_no, req.receipt_line_no, req.first_ile_no)
        .into_iter()
        .filter(|e| e.get("Quantity").as_dec() - e.get("Invoiced Quantity").as_dec() > Decimal::ZERO)
        .collect();
    let total_avail: Decimal = open.iter().map(|e| e.get("Quantity").as_dec() - e.get("Invoiced Quantity").as_dec()).sum();
    if total_avail < req.qty_base {
        refuse!("Cannot settle {} base units on receipt {} line {}: only {} remain uninvoiced.", req.qty_base, req.receipt_no, req.receipt_line_no, total_avail);
    }
    let mut left_qty = req.qty_base;
    let mut allocs: Vec<(Record, Decimal, Decimal)> = Vec::new();
    for ile in open {
        if left_qty.is_zero() {
            break;
        }
        let uninvoiced = ile.get("Quantity").as_dec() - ile.get("Invoiced Quantity").as_dec();
        let take = left_qty.min(uninvoiced);
        allocs.push((ile, take, uninvoiced));
        left_qty -= take;
    }
    let mut left_actual = req.actual_cost_lcy;
    let mut delta = SettlementDelta::default();
    let n = allocs.len();
    for (i, (mut ile, take_base, ile_uninvoiced)) in allocs.into_iter().enumerate() {
        let slice_actual = if i + 1 == n { left_actual } else { round_amount(req.actual_cost_lcy * take_base / req.qty_base) };
        left_actual -= slice_actual;
        let rem_expected = ile.get("Cost Amount (Expected)").as_dec();
        let expected_rev = if take_base == ile_uninvoiced { rem_expected } else { round_amount(rem_expected * take_base / ile_uninvoiced) };
        let src = initial_value_entry(ctx, ile.get("Entry No.").as_int());
        let mut v = init_direct_value_entry(ctx, &ile, src.as_ref())?;
        v.set("Posting Date", Value::Date(req.posting_date));
        v.set("Document Type", Value::text(req.inv_doc_type));
        v.set("Document No.", Value::text(req.inv_doc_no));
        v.set("Document Line No.", Value::Int(req.inv_line_no));
        v.set("Item Ledger Entry Quantity", dec(Decimal::ZERO));
        v.set("Valued Quantity", dec(take_base));
        v.set("Invoiced Quantity", dec(take_base));
        v.set("Cost per Unit", dec(if take_base.is_zero() { Decimal::ZERO } else { round_unit(slice_actual / take_base) }));
        v.set("Expected Cost", Value::Bool(false));
        v.set("Cost Amount (Actual)", dec(slice_actual));
        v.set("Cost Amount (Expected)", dec(-expected_rev));
        v.set("Cost Posted to G/L", dec(slice_actual));
        v.set("Sales Amount (Actual)", dec(Decimal::ZERO));
        append_value_entry(ctx, &mut ile, v)?;
        delta.net_cost_delta_lcy += slice_actual - expected_rev;
        delta.settled_qty_base += take_base;
    }
    Ok(delta)
}

pub fn add_direct_cost(ctx: &mut Ctx, req: &DirectCostEntry<'_>) -> Result<()> {
    let mut ile = ctx.get(ITEM_LEDGER, &Key(vec![Value::Int(req.ile_no)]))?;
    if !req.sales_amount_lcy.is_zero() {
        ile.set("Sales Amount (Actual)", dec(ile.get("Sales Amount (Actual)").as_dec() + req.sales_amount_lcy));
    }
    let src = initial_value_entry(ctx, req.ile_no);
    let mut v = init_direct_value_entry(ctx, &ile, src.as_ref())?;
    v.set("Posting Date", Value::Date(req.posting_date));
    v.set("Document Type", Value::text(req.doc_type));
    v.set("Document No.", Value::text(req.doc_no));
    v.set("Document Line No.", Value::Int(req.doc_line_no));
    v.set("Item Ledger Entry Quantity", dec(Decimal::ZERO));
    v.set("Valued Quantity", dec(Decimal::ZERO));
    v.set("Invoiced Quantity", dec(Decimal::ZERO));
    v.set("Expected Cost", Value::Bool(false));
    v.set("Cost Amount (Actual)", dec(req.cost_lcy));
    v.set("Cost Amount (Expected)", dec(Decimal::ZERO));
    v.set("Cost Posted to G/L", dec(req.cost_lcy));
    v.set("Sales Amount (Actual)", dec(req.sales_amount_lcy));
    v.set("Item Charge No.", Value::text(req.charge_no));
    v.set("Description", Value::text(format!("Item charge {}", req.charge_no)));
    append_value_entry(ctx, &mut ile, v)?;
    if !req.cost_lcy.is_zero() {
        recalc_item_unit_cost(ctx, ile.str("Item No."))?;
    }
    Ok(())
}

/// A revaluation (BC's Revaluation value entry, posted from the revaluation journal): `amount`
/// added to the value of the item's inventory (at `location`, all when blank) on `date`. It is
/// booked on the applied entry, else on the latest inbound entry; the moving average takes it
/// from its valuation date on.
#[allow(clippy::too_many_arguments)]
pub fn revalue(ctx: &mut Ctx, item: &Record, location: &str, applies_to: i64, date: chrono::NaiveDate, doc_no: &str, description: &str, amount: Decimal) -> Result<()> {
    let no = item.str("No.");
    let ile = if applies_to != 0 {
        ctx.db.rows(ITEM_LEDGER).find(|e| e.get("Entry No.").as_int() == applies_to && e.str("Item No.") == no).cloned()
    } else {
        ctx.db
            .rows(ITEM_LEDGER)
            .filter(|e| e.str("Item No.") == no && (location.is_empty() || e.str("Location Code") == location) && e.get("Quantity").as_dec() > Decimal::ZERO)
            .max_by_key(|e| e.get("Entry No.").as_int())
            .cloned()
    };
    let Some(ile) = ile else {
        refuse!("There is no inventory of item {no} to revalue.");
    };
    // The newest entry so far: the revaluation counts after it.
    let after = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no).map(|e| e.get("Entry No.").as_int()).max().unwrap_or(0);
    let t = ctx.table(VALUE_ENTRY)?;
    let mut v = t.init_record();
    v.set("Entry No.", Value::Int(ctx.db.next_entry_no(VALUE_ENTRY)));
    for f in ["Item No.", "Location Code", "Source Type", "Source No."] {
        v.set(f, ile.get(f).clone());
    }
    v.set("Posting Date", Value::Date(date));
    v.set("Valuation Date", Value::Date(date));
    v.set("Document No.", Value::text(doc_no));
    v.set("Description", Value::text(description));
    v.set("Item Ledger Entry Type", ile.get("Entry Type").clone());
    v.set("Item Ledger Entry No.", ile.get("Entry No.").clone());
    v.set("Item Ledger Entry Quantity", dec(Decimal::ZERO));
    v.set("Valued Quantity", dec(item_qty(ctx, no, location)));
    v.set("Cost Amount (Actual)", dec(amount));
    v.set("Cost Posted to G/L", dec(amount));
    v.set("Entry Type", Value::text("Revaluation"));
    v.set("Gen. Prod. Posting Group", item.get("Gen. Prod. Posting Group").clone());
    v.set("Inventory Posting Group", item.get("Inventory Posting Group").clone());
    v.set("Applies-to Entry", Value::Int(after));
    v.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(t, v)?;
    Ok(())
}

fn item_qty(ctx: &Ctx, item: &str, location: &str) -> Decimal {
    ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item && (location.is_empty() || e.str("Location Code") == location)).map(|e| e.get("Quantity").as_dec()).sum()
}

fn setup_account(ctx: &Ctx, bus: &str, prod: &str, field: &str) -> Result<String> {
    let s = ctx.get(GEN_POSTING_SETUP, &Key(vec![Value::text(bus), Value::text(prod)]))?;
    match s.str(field) {
        "" => refuse!("{field} must have a value in General Posting Setup: Gen. Bus. Posting Group='{bus}',Gen. Prod. Posting Group='{prod}'."),
        a => Ok(a.to_string()),
    }
}

/// The G/L lines of a value entry's cost not yet on the G/L: inventory against COGS (sales),
/// inventory adjustments (adjustments) or, for transfers, nothing (both sides are inventory).
fn gl_for(ctx: &Ctx, v: &Record, amount: Decimal) -> Result<Vec<GlLine>> {
    let item = ctx.get_code(ITEM, v.str("Item No."))?;
    let inventory = crate::posting::inventory_account(ctx, &item, v.str("Location Code"))?;
    let desc = format!("Cost adjustment {} {}", v.str("Item No."), v.str("Document No."));
    let mut lines = vec![GlLine { account: inventory, amount, vat_amount: Decimal::ZERO, description: desc.clone(), gen_posting_type: "" }];
    let contra = match v.str("Item Ledger Entry Type") {
        _ if v.str("Entry Type") == "Revaluation" => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "Inventory Adjmt. Account")?),
        "Sale" => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "COGS Account")?),
        "Positive Adjmt." | "Negative Adjmt." => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "Inventory Adjmt. Account")?),
        "Purchase" => Some(setup_account(ctx, v.str("Gen. Bus. Posting Group"), v.str("Gen. Prod. Posting Group"), "Purch. Account")?),
        _ => None,
    };
    if let Some(c) = contra {
        lines.push(GlLine { account: c, amount: -amount, vat_amount: Decimal::ZERO, description: desc, gen_posting_type: "" });
    }
    Ok(lines)
}

/// The cost of an item ledger entry, without revaluations (they count from their own date).
fn entry_cost(ctx: &Ctx, ile_no: i64) -> Decimal {
    ctx.db
        .rows(VALUE_ENTRY)
        .filter(|v| v.get("Item Ledger Entry No.").as_int() == ile_no && v.str("Entry Type") != "Revaluation")
        .map(entry_valuation)
        .sum()
}

/// Takes `qty` out of the cost layers (oldest first for FIFO, newest first for LIFO); what the
/// layers cannot cover is valued at `fallback` per unit. Returns the cost taken.
fn take_layers(layers: &mut VecDeque<(Decimal, Decimal)>, qty: Decimal, lifo: bool, fallback: Decimal) -> Decimal {
    let mut need = qty;
    let mut cost = Decimal::ZERO;
    while need > Decimal::ZERO {
        let layer = if lifo { layers.back_mut() } else { layers.front_mut() };
        let Some(l) = layer else {
            cost += need * fallback;
            break;
        };
        let take = need.min(l.0);
        let part = if take == l.0 { l.1 } else { round_amount(l.1 * take / l.0) };
        cost += part;
        l.0 -= take;
        l.1 -= part;
        need -= take;
        if l.0 <= Decimal::ZERO {
            if lifo {
                layers.pop_back();
            } else {
                layers.pop_front();
            }
        }
    }
    cost
}

/// Spreads a revaluation over the open cost layers by quantity (the last taking the rounding).
fn revalue_layers(layers: &mut VecDeque<(Decimal, Decimal)>, amount: Decimal) {
    let total: Decimal = layers.iter().map(|l| l.0).sum();
    if total <= Decimal::ZERO {
        return;
    }
    let mut left = amount;
    let n = layers.len();
    for (i, l) in layers.iter_mut().enumerate() {
        let share = if i + 1 == n { left } else { round_amount(amount * l.0 / total) };
        l.1 += share;
        left -= share;
    }
}

struct ItemEvaluation {
    adjustments: Vec<(Record, Decimal)>,
    unit_cost: Option<Decimal>,
}

fn evaluate_item(ctx: &Ctx, item: &Record) -> ItemEvaluation {
    let no = item.str("No.");
    let mut ledger: Vec<Record> = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no).cloned().collect();
    ledger.sort_by_key(|e| (e.get("Posting Date").as_date(), e.get("Entry No.").as_int()));
    if ledger.is_empty() {
        return ItemEvaluation { adjustments: Vec::new(), unit_cost: None };
    }
    let standard = item.str("Costing Method") == "Standard";
    let layered = matches!(item.str("Costing Method"), "FIFO" | "LIFO" | "Specific");
    let lifo = item.str("Costing Method") == "LIFO";
    let mut layers: VecDeque<(Decimal, Decimal)> = VecDeque::new();
    let (mut qty, mut cost) = (Decimal::ZERO, Decimal::ZERO);
    let mut shipped: HashMap<String, VecDeque<Decimal>> = HashMap::new();
    let mut revals: VecDeque<(Option<NaiveDate>, i64, Decimal)> = {
        let mut v: Vec<(Option<NaiveDate>, i64, Decimal)> = ctx
            .db
            .rows(VALUE_ENTRY)
            .filter(|v| v.str("Item No.") == no && v.str("Entry Type") == "Revaluation")
            .map(|v| (v.get("Valuation Date").as_date(), v.get("Applies-to Entry").as_int(), v.get("Cost Amount (Actual)").as_dec()))
            .collect();
        v.sort_by_key(|r| (r.0, r.1));
        v.into()
    };
    let mut adjustments = Vec::new();
    for e in ledger {
        let (d, n) = (e.get("Posting Date").as_date(), e.get("Entry No.").as_int());
        while let Some(&(rd, after, amount)) = revals.front() {
            if rd < d || (rd == d && after < n) {
                cost += amount;
                if layered {
                    revalue_layers(&mut layers, amount);
                }
                revals.pop_front();
            } else {
                break;
            }
        }
        let q = e.get("Quantity").as_dec();
        let current = entry_cost(ctx, n);
        let transfer = e.str("Entry Type") == "Transfer";
        let target = if q < Decimal::ZERO {
            let unit = if standard {
                item.get("Standard Cost").as_dec()
            } else if qty > Decimal::ZERO {
                cost / qty
            } else {
                item.get("Unit Cost").as_dec()
            };
            let t = if layered {
                -round_amount(take_layers(&mut layers, -q, lifo, item.get("Unit Cost").as_dec()))
            } else {
                round_amount(q * unit)
            };
            if transfer {
                shipped.entry(e.str("Document No.").to_string()).or_default().push_back(-t / -q);
            }
            Some(t)
        } else if transfer {
            shipped.get_mut(e.str("Document No.")).and_then(VecDeque::pop_front).map(|unit| round_amount(q * unit))
        } else {
            None
        };
        let value = target.unwrap_or(current);
        if layered && q > Decimal::ZERO {
            layers.push_back((q, value));
        }
        if let Some(t) = target
            && t != current
        {
            adjustments.push((e, t));
        }
        qty += q;
        cost += value;
    }
    for r in &revals {
        cost += r.2;
        if layered {
            revalue_layers(&mut layers, r.2);
        }
    }
    if layered {
        let (lq, lc) = layers.iter().fold((Decimal::ZERO, Decimal::ZERO), |a, l| (a.0 + l.0, a.1 + l.1));
        if lq > Decimal::ZERO {
            qty = lq;
            cost = lc;
        }
    }
    let unit_cost = if !standard && qty > Decimal::ZERO { Some(round_unit(cost / qty)) } else { None };
    ItemEvaluation { adjustments, unit_cost }
}

pub fn recalc_item_unit_cost(ctx: &mut Ctx, item_no: &str) -> Result<()> {
    let item = ctx.get_code(ITEM, item_no)?;
    if let Some(unit) = evaluate_item(ctx, &item).unit_cost {
        let itt = ctx.table(ITEM)?;
        let mut updated = item;
        updated.set("Unit Cost", dec(unit));
        ctx.db.modify(itt, updated)?;
    }
    Ok(())
}

/// BC's Adjust Cost - Item Entries: walking each item's entries by date, an outbound entry is
/// valued at the running average of what came in before it (Average), at the cost layers it
/// uses up (FIFO / LIFO; Specific as FIFO), or at the standard cost (Standard); a transfer's
/// receipt takes its shipment's cost. Differences become adjustment value entries.
fn adjust_cost(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let only = opt_str(p, "item_no").map(str::to_string);
    let automatic = ctx.setup(INVENTORY_SETUP)?.get("Automatic Cost Posting").as_bool();
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| only.as_ref().is_none_or(|o| i.str("No.") == o) && i.str("Type") != "Service").cloned().collect();
    let itt = ctx.table(ITEM)?;
    let txn = Txn::new(ctx, "INVTADJMT", "", "");
    let mut gl: Vec<GlLine> = Vec::new();
    let mut adjusted = 0;
    let date = ctx.work_date;
    for item in items {
        let no = item.str("No.").to_string();
        let eval = evaluate_item(ctx, &item);
        for (mut ile, t) in eval.adjustments {
            let ile_no = ile.get("Entry No.").as_int();
            let current = entry_cost(ctx, ile_no);
            let diff = t - current;
            if diff.is_zero() {
                continue;
            }
            let src = initial_value_entry(ctx, ile_no);
            let mut v = init_direct_value_entry(ctx, &ile, src.as_ref())?;
            for f in ["Document Type", "Document No.", "Document Line No."] {
                v.set(f, ile.get(f).clone());
            }
            v.set("Posting Date", Value::Date(date.max(ile.get("Posting Date").as_date().unwrap_or(date))));
            v.set("Item Ledger Entry Quantity", dec(Decimal::ZERO));
            v.set("Valued Quantity", dec(Decimal::ZERO));
            v.set("Invoiced Quantity", dec(Decimal::ZERO));
            v.set("Expected Cost", Value::Bool(false));
            v.set("Cost Amount (Actual)", dec(diff));
            v.set("Cost Amount (Expected)", dec(Decimal::ZERO));
            v.set("Adjustment", Value::Bool(true));
            if automatic {
                gl.extend(gl_for(ctx, &v, diff)?);
                v.set("Cost Posted to G/L", dec(diff));
            }
            append_value_entry(ctx, &mut ile, v)?;
            adjusted += 1;
        }
        if let Some(unit) = eval.unit_cost {
            let mut i = ctx.get_code(ITEM, &no)?;
            i.set("Unit Cost", dec(unit));
            ctx.db.modify(itt, i)?;
        }
    }
    if !gl.is_empty() {
        post_gl(ctx, &GlDoc { posting_date: date, doc_type: "", doc_no: "ADJCOST", ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    }
    Ok(json!({"adjusted": adjusted, "message": format!("{adjusted} item ledger entries were adjusted.")}))
}

/// BC's Post Inventory Cost to G/L: every actual value entry's cost not yet on the G/L.
fn post_inventory_cost(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let date = match opt_str(p, "posting_date").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| crate::error::EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?,
        None => ctx.work_date,
    };
    let pending: Vec<Record> = ctx
        .db
        .rows(VALUE_ENTRY)
        .filter(|v| !v.get("Expected Cost").as_bool() && v.get("Cost Amount (Actual)").as_dec() != v.get("Cost Posted to G/L").as_dec())
        .cloned()
        .collect();
    if pending.is_empty() {
        return Ok(json!({"entries": 0, "message": "There is nothing to post to the general ledger."}));
    }
    let vt = ctx.table(VALUE_ENTRY)?;
    let txn = Txn::new(ctx, "INVTPCOST", "", "");
    let mut gl = Vec::new();
    let mut rows = Vec::new();
    for mut v in pending.clone() {
        let amount = v.get("Cost Amount (Actual)").as_dec() - v.get("Cost Posted to G/L").as_dec();
        gl.extend(gl_for(ctx, &v, amount)?);
        rows.push(json!([v.get("Entry No.").as_int(), v.str("Item No."), v.str("Item Ledger Entry Type"), format!("{:.2}", amount)]));
        v.set("Cost Posted to G/L", v.get("Cost Amount (Actual)").clone());
        ctx.db.modify(vt, v)?;
    }
    post_gl(ctx, &GlDoc { posting_date: date, doc_type: "", doc_no: "INVTCOST", ext_doc_no: "", source_type: "", source_no: "", txn: &txn }, gl)?;
    Ok(json!({"entries": pending.len(), "report": {"title": "Post Inventory Cost to G/L", "columns": ["Value Entry No.", "Item No.", "Entry Type", "Cost"], "rows": rows},
              "message": format!("{} value entries were posted to the general ledger.", pending.len())}))
}

fn s_adjust() -> Json {
    json!({"type": "object", "properties": {"item_no": {"type": "string", "description": "one item; all when blank"}}})
}

fn s_post() -> Json {
    json!({"type": "object", "properties": {"posting_date": {"type": "string", "description": "YYYY-MM-DD; default the work date"}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "inventory.adjust_cost", label: "Adjust Cost - Item Entries", group: "Inventory", summary: "Value outbound item entries at the running average cost and post the differences.", mutates: true, params: s_adjust, run: adjust_cost },
    CommandDef { id: "inventory.post_cost_to_gl", label: "Post Inventory Cost to G/L", group: "Inventory", summary: "Post the inventory cost of value entries not yet on the general ledger.", mutates: true, params: s_post, run: post_inventory_cost },
];
