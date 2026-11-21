//! Posting: turning documents and payments into posted documents and ledger entries. Sales and
//! purchase share one routine ([`post_document`]) parameterised by [`Side`]. Orders post in two
//! independent steps, as in BC: Ship / Receive (posted shipment or receipt, item ledger entries)
//! and Invoice (posted invoice, G/L, customer / vendor ledger).

pub mod payment;

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec, round_amount, round_unit, test_field};
use crate::error::{EngineError, Result};
use crate::ledger::{PartyEntry, Txn, post_party_entry, register};
use crate::noseries;
use crate::refuse;
use crate::schema::finance::{PURCH_RCPT_HEADER, PURCH_RCPT_LINE, SALES_SHIPMENT_HEADER, SALES_SHIPMENT_LINE};
use crate::schema::pages::{PURCH_DOC_PAGES, SALES_DOC_PAGES};
use crate::schema::tables::{
    CUST_POSTING_GROUP, GEN_POSTING_SETUP, GL_ACCOUNT, GL_ENTRY, GL_SETUP, INVT_POSTING_SETUP, ITEM, ITEM_LEDGER,
    VAT_POSTING_SETUP, VEND_POSTING_GROUP,
};
use crate::side::Side;

/// One G/L amount to post.
pub struct GlLine {
    pub account: String,
    pub amount: Decimal,
    pub vat_amount: Decimal,
    pub description: String,
    pub gen_posting_type: &'static str,
}

/// What every G/L entry of one transaction shares.
pub struct GlDoc<'a> {
    pub posting_date: NaiveDate,
    pub doc_type: &'a str,
    pub doc_no: &'a str,
    pub ext_doc_no: &'a str,
    pub source_type: &'a str,
    pub source_no: &'a str,
    pub txn: &'a Txn,
}

/// What to post of an order (BC's Ship / Invoice / Ship and Invoice).
#[derive(Clone, Copy, Debug)]
pub struct PostMode {
    pub ship: bool,
    pub invoice: bool,
}

impl PostMode {
    pub const BOTH: PostMode = PostMode { ship: true, invoice: true };

    /// "Ship and Invoice" (default), "Ship" / "Receive", "Invoice".
    pub fn parse(s: Option<&str>) -> Result<Self> {
        match s.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            None | Some("") | Some("ship and invoice") | Some("receive and invoice") | Some("both") => Ok(Self::BOTH),
            Some("ship") | Some("receive") => Ok(PostMode { ship: true, invoice: false }),
            Some("invoice") => Ok(PostMode { ship: false, invoice: true }),
            Some(other) => Err(EngineError::invalid(format!("'{other}' is not a posting option. Use Ship and Invoice, Ship / Receive, or Invoice."))),
        }
    }
}

/// Refuses a posting date outside General Ledger Setup › Allow Posting From / To.
pub fn check_posting_date(ctx: &Ctx, date: NaiveDate) -> Result<()> {
    let s = ctx.setup(GL_SETUP)?;
    let from = s.get("Allow Posting From").as_date();
    let to = s.get("Allow Posting To").as_date();
    if from.is_some_and(|f| date < f) || to.is_some_and(|t| date > t) {
        refuse!("The Posting Date {date} is not within your range of allowed posting dates (General Ledger Setup).");
    }
    Ok(())
}

/// Writes G/L entries for `lines` and a G/L Register; refuses an unbalanced transaction or an
/// account that can't be posted to. Returns the first and last entry numbers written.
pub fn post_gl(ctx: &mut Ctx, doc: &GlDoc, lines: Vec<GlLine>) -> Result<Option<(i64, i64)>> {
    let total: Decimal = lines.iter().map(|l| l.amount).sum();
    if !total.is_zero() {
        refuse!(
            "The transaction cannot be completed because it will cause inconsistencies in the G/L Entry table. It is out of balance by {total}."
        );
    }
    let t = ctx.table(GL_ENTRY)?;
    let first = ctx.db.next_entry_no(GL_ENTRY);
    let mut entry_no = first;
    for l in lines.into_iter().filter(|l| !l.amount.is_zero()) {
        if l.account.is_empty() {
            refuse!("An account to post to is missing for '{}'. Check the posting setup.", l.description);
        }
        let acc = ctx.get_code(GL_ACCOUNT, &l.account)?;
        if acc.str("Account Type") != "Posting" {
            refuse!("Account Type must be equal to 'Posting' in G/L Account: No.={}. Current value is '{}'.", l.account, acc.str("Account Type"));
        }
        if acc.get("Blocked").as_bool() {
            refuse!("Blocked must be equal to 'No' in G/L Account: No.={}. Current value is 'Yes'.", l.account);
        }
        let mut e = t.init_record();
        e.set("Entry No.", Value::Int(entry_no));
        e.set("G/L Account No.", Value::text(&l.account));
        e.set("G/L Account Name", acc.get("Name").clone());
        e.set("Posting Date", Value::Date(doc.posting_date));
        e.set("Document Type", Value::text(doc.doc_type));
        e.set("Document No.", Value::text(doc.doc_no));
        e.set("External Document No.", Value::text(doc.ext_doc_no));
        e.set("Description", Value::text(&l.description));
        e.set("Amount", dec(l.amount));
        e.set("Debit Amount", dec(l.amount.max(Decimal::ZERO)));
        e.set("Credit Amount", dec((-l.amount).max(Decimal::ZERO)));
        e.set("VAT Amount", dec(l.vat_amount));
        e.set("Gen. Posting Type", Value::text(l.gen_posting_type));
        e.set("Source Type", Value::text(doc.source_type));
        e.set("Source No.", Value::text(doc.source_no));
        e.set("Transaction No.", Value::Int(doc.txn.no));
        e.set("Source Code", Value::text(&doc.txn.source_code));
        e.set("User ID", Value::text(ctx.user.to_uppercase()));
        crate::ledger::stamp_dims(ctx, t, &mut e, doc.txn.dim_set);
        ctx.db.insert(t, e)?;
        entry_no = entry_no.saturating_add(1);
    }
    if entry_no == first {
        return Ok(None);
    }
    register(ctx, doc.txn, first, entry_no - 1)?;
    Ok(Some((first, entry_no - 1)))
}

/// Copies the fields `dst` shares with `src` by name (stored fields only).
pub fn copy_common(src: &Record, dst_t: &TableDef, dst: &mut Record) {
    for f in dst_t.fields.iter().filter(|f| !f.is_flow()) {
        let v = src.get(&f.name);
        if !matches!(v, Value::Null) || f.ty == erpcraft_meta::FieldType::Date {
            dst.set(&f.name, v.clone());
        }
    }
}

fn setup_account(ctx: &Ctx, table: &str, key: Key, field: &str, what: &str) -> Result<String> {
    let t = ctx.table(table)?;
    let Some(r) = ctx.db.get(&t.name, &key) else {
        refuse!("The {} does not exist. Identification fields and values: {}", t.caption, crate::db::key_text(t, &key));
    };
    let acc = r.str(field).to_string();
    if acc.is_empty() {
        refuse!("{field} must have a value in {}: {}. It cannot be zero or empty. ({what})", t.caption, crate::db::key_text(t, &key));
    }
    Ok(acc)
}

/// The inventory G/L account of `item` at `location` (Inventory Posting Setup), falling back to
/// the blank location.
pub fn inventory_account(ctx: &Ctx, item: &Record, location: &str) -> Result<String> {
    let group = item.get("Inventory Posting Group").clone();
    if !location.is_empty()
        && let Some(r) = ctx.db.get(INVT_POSTING_SETUP, &Key(vec![Value::text(location), group.clone()]))
        && !r.str("Inventory Account").is_empty()
    {
        return Ok(r.str("Inventory Account").to_string());
    }
    setup_account(ctx, INVT_POSTING_SETUP, Key(vec![Value::text(""), group]), "Inventory Account", "inventory")
}

/// One item ledger entry to write.
pub struct ItemEntry<'a> {
    pub item: &'a Record,
    pub posting_date: NaiveDate,
    pub entry_type: &'a str,
    pub doc_type: &'a str,
    pub doc_no: &'a str,
    pub description: &'a str,
    pub location: &'a str,
    /// Signed: positive into stock, negative out.
    pub qty: Decimal,
    pub cost: Decimal,
    pub sales_amount: Decimal,
    pub uom: &'a Value,
    pub source_type: &'a str,
    pub source_no: &'a str,
}

pub fn post_item(ctx: &mut Ctx, e: &ItemEntry) -> Result<i64> {
    let t = ctx.table(ITEM_LEDGER)?;
    let no = ctx.db.next_entry_no(ITEM_LEDGER);
    let mut r = t.init_record();
    r.set("Entry No.", Value::Int(no));
    r.set("Item No.", Value::text(e.item.str("No.")));
    r.set("Posting Date", Value::Date(e.posting_date));
    r.set("Entry Type", Value::text(e.entry_type));
    r.set("Document Type", Value::text(e.doc_type));
    r.set("Document No.", Value::text(e.doc_no));
    r.set("Description", Value::text(e.description));
    r.set("Location Code", Value::text(e.location));
    r.set("Quantity", dec(e.qty));
    r.set("Remaining Quantity", dec(e.qty.max(Decimal::ZERO)));
    r.set("Open", Value::Bool(e.qty > Decimal::ZERO));
    r.set("Cost Amount (Actual)", dec(e.cost));
    r.set("Sales Amount (Actual)", dec(e.sales_amount));
    r.set("Unit of Measure Code", e.uom.clone());
    r.set("Source Type", Value::text(e.source_type));
    r.set("Source No.", Value::text(e.source_no));
    ctx.db.insert(t, r)?;
    Ok(no)
}

/// Item ledger entry of a document line for `qty` (signed).
#[allow(clippy::too_many_arguments)]
fn post_item_entry(
    ctx: &mut Ctx,
    side: &Side,
    item: &Record,
    posting_date: NaiveDate,
    doc_type: &str,
    doc_no: &str,
    description: &str,
    location: &str,
    qty: Decimal,
    cost: Decimal,
    sales_amount: Decimal,
    uom: &Value,
    party_no: &str,
) -> Result<i64> {
    if qty < Decimal::ZERO {
        crate::item_journal::check_negative(ctx, item.str("No."), location, -qty)?;
    }
    post_item(
        ctx,
        &ItemEntry {
            item,
            posting_date,
            entry_type: if side.sales { "Sale" } else { "Purchase" },
            doc_type,
            doc_no,
            description,
            location,
            qty,
            cost,
            sales_amount,
            uom,
            source_type: if side.sales { "Customer" } else { "Vendor" },
            source_no: party_no,
        },
    )
}

/// Line amounts for `qty` of a line whose full amounts are for `Quantity`.
fn portion(l: &Record, field: &str, qty: Decimal) -> Decimal {
    let full = l.get("Quantity").as_dec();
    let v = l.get(field).as_dec();
    if qty == full || full.is_zero() { v } else { round_amount(v * qty / full) }
}

fn is_item_inventory(ctx: &Ctx, l: &Record) -> Result<Option<Record>> {
    if l.str("Type") != "Item" {
        return Ok(None);
    }
    let it = ctx.get_code(ITEM, l.str("No."))?;
    Ok((it.str("Type") == "Inventory").then_some(it))
}

/// Posts a sales or purchase document. Orders post what is in Qty. to Ship / Qty. to Receive and
/// Qty. to Invoice, as `mode` says; an order is deleted once every line is fully shipped and
/// invoiced. Invoices and credit memos post in full and are deleted.
pub fn post_document(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str, mode: PostMode) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    let lt = ctx.table(side.line)?;
    let header = ctx.get(side.header, &Key(vec![Value::text(doc_type), Value::text(no)]))?;
    if matches!(doc_type, "Quote" | "Blanket Order") {
        refuse!("A {doc_type} cannot be posted. Convert it to an order first (Make Order).");
    }
    crate::approvals::check_post(ctx, side, doc_type, no, header.str("Status"))?;
    let credit = matches!(doc_type, "Credit Memo" | "Return Order");
    let is_order = doc_type == "Order";
    let mode = if is_order { mode } else { PostMode::BOTH };
    // g: sign of the party (receivable / payable) amount; revenue / expense take -g.
    let g = if side.sales != credit { Decimal::ONE } else { Decimal::NEGATIVE_ONE };

    test_field(ht, &header, side.party_no)?;
    test_field(ht, &header, side.bill_no)?;
    test_field(ht, &header, "Posting Date")?;
    test_field(ht, &header, side.posting_group)?;
    let posting_date = header.get("Posting Date").as_date().ok_or_else(|| EngineError::refused("Posting Date must have a value."))?;
    check_posting_date(ctx, posting_date)?;

    let party_no = header.str(side.party_no).to_string();
    let bill_no = header.str(side.bill_no).to_string();
    let party = ctx.get_code(side.party_table, &party_no)?;
    let blocked = party.str("Blocked");
    if blocked == "All" || (side.sales && mode.invoice && blocked == "Invoice") || (side.sales && mode.ship && blocked == "Ship") {
        refuse!("You cannot post this type of document when {} {party_no} is blocked with type {blocked}.", side.party_caption);
    }
    let setup = ctx.setup(side.setup)?;
    if mode.invoice && setup.get("Ext. Doc. No. Mandatory").as_bool() {
        let f = if side.sales { "External Document No." } else { Side::vendor_doc_no_field(doc_type) };
        test_field(ht, &header, f)?;
    }

    let lines: Vec<Record> =
        ctx.db.rows(side.line).filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no).cloned().collect();
    let real = |l: &Record| !l.str("Type").is_empty() && !l.str("No.").is_empty();
    // Item ledger entries count in the base unit of measure.
    let per = |l: &Record| match l.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    };
    let shipped = |l: &Record| l.get(side.qty_handled).as_dec();
    let invoiced = |l: &Record| l.get("Quantity Invoiced").as_dec();
    // Quantities to post per line.
    let to_ship = |l: &Record| -> Decimal {
        if !real(l) || !mode.ship {
            Decimal::ZERO
        } else if is_order {
            l.get(side.qty_handle).as_dec()
        } else {
            l.get("Quantity").as_dec() - invoiced(l)
        }
    };
    let to_invoice = |l: &Record| -> Decimal {
        if !real(l) || !mode.invoice {
            Decimal::ZERO
        } else if is_order {
            l.get("Qty. to Invoice").as_dec()
        } else {
            l.get("Quantity").as_dec() - invoiced(l)
        }
    };
    for l in &lines {
        if is_order && to_invoice(l) > shipped(l) - invoiced(l) + to_ship(l) {
            refuse!(
                "You cannot invoice more than you have {} on line {} ({} {}).",
                if side.sales { "shipped" } else { "received" },
                l.get("Line No."),
                l.str("No."),
                l.str("Description")
            );
        }
    }
    let any_ship = is_order && lines.iter().any(|l| !to_ship(l).is_zero());
    let any_invoice = lines.iter().any(|l| !to_invoice(l).is_zero());
    if !any_ship && !any_invoice {
        refuse!("There is nothing to post.");
    }
    let description = header.str("Posting Description").to_string();
    // A document in a currency: amounts convert line by line at its factor; the party entry
    // keeps the currency amount and the sum of the lines in LCY.
    let currency_code = header.str("Currency Code").to_string();
    let factor = match (currency_code.is_empty(), header.get("Currency Factor").as_dec()) {
        (true, _) => Decimal::ONE,
        (false, f) if !f.is_zero() => f,
        (false, _) => crate::currency::factor(ctx, &currency_code, posting_date)?,
    };
    let lcy = |d: Decimal| crate::currency::to_lcy(d, factor);
    let mut result = serde_json::Map::new();
    let mut messages = Vec::new();
    // Item ledger entry per line created by shipping now (an invoice-only posting has none).
    let mut shipment_no = String::new();

    // ---- Ship / Receive (orders) ----------------------------------------------------------
    if any_ship {
        let (sh_t, sl_t, nos_field, ile_doc) = if side.sales {
            (SALES_SHIPMENT_HEADER, SALES_SHIPMENT_LINE, "Posted Shipment Nos.", "Sales Shipment")
        } else {
            (PURCH_RCPT_HEADER, PURCH_RCPT_LINE, "Posted Receipt Nos.", "Purchase Receipt")
        };
        let series = noseries::series_from_setup(ctx, side.setup, nos_field)?;
        shipment_no = noseries::next(ctx, &series)?;
        let sht = ctx.table(sh_t)?;
        let slt = ctx.table(sl_t)?;
        let mut sh = sht.init_record();
        copy_common(&header, sht, &mut sh);
        sh.set("No.", Value::text(&shipment_no));
        sh.set("Order No.", Value::text(no));
        if !side.sales {
            sh.set("Vendor Shipment No.", header.get("Vendor Order No.").clone());
        }
        sh.set("User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.insert(sht, sh)?;
        for l in lines.iter().filter(|l| !to_ship(l).is_zero()) {
            let q = to_ship(l);
            let mut ile = 0;
            if let Some(it) = is_item_inventory(ctx, l)? {
                let qb = q * per(l);
                let (qty, cost) = if side.sales {
                    (-qb, round_amount(-qb * it.get("Unit Cost").as_dec()))
                } else {
                    (qb, lcy(portion(l, "Amount", q)))
                };
                ile = post_item_entry(ctx, side, &it, posting_date, ile_doc, &shipment_no, l.str("Description"), l.str("Location Code"), qty, cost, Decimal::ZERO, l.get("Unit of Measure Code"), &party_no)?;
                if !side.sales {
                    update_item_cost(ctx, it.str("No."), qb, cost)?;
                }
            }
            let mut sl = slt.init_record();
            copy_common(l, slt, &mut sl);
            sl.set("Document No.", Value::text(&shipment_no));
            sl.set("Quantity", dec(q));
            sl.set("Quantity Invoiced", dec(Decimal::ZERO));
            sl.set(if side.sales { "Qty. Shipped Not Invoiced" } else { "Qty. Rcd. Not Invoiced" }, dec(q));
            sl.set("Order No.", Value::text(no));
            sl.set("Order Line No.", l.get("Line No.").clone());
            sl.set("Item Ledger Entry No.", Value::Int(ile));
            ctx.db.insert(slt, sl)?;
        }
        result.insert(if side.sales { "shipment_no" } else { "receipt_no" }.into(), json!(shipment_no));
        messages.push(format!("{} {shipment_no}", if side.sales { "shipment" } else { "receipt" }));
    }

    // ---- Invoice -----------------------------------------------------------------------------
    let mut posted_no = String::new();
    if any_invoice {
        let party_account = if side.sales {
            setup_account(ctx, CUST_POSTING_GROUP, Key(vec![header.get(side.posting_group).clone()]), "Receivables Account", "receivables")?
        } else {
            setup_account(ctx, VEND_POSTING_GROUP, Key(vec![header.get(side.posting_group).clone()]), "Payables Account", "payables")?
        };
        let nos_field = if credit { "Posted Credit Memo Nos." } else { "Posted Invoice Nos." };
        let series = noseries::series_from_setup(ctx, side.setup, nos_field)?;
        posted_no = noseries::next(ctx, &series)?;
        let (ph_table, pl_table) = if credit { (side.cr_header, side.cr_line) } else { (side.inv_header, side.inv_line) };
        let pht = ctx.table(ph_table)?;
        let plt = ctx.table(pl_table)?;
        let ledger_doc_type = if credit { "Credit Memo" } else { "Invoice" };
        let gen_type = if side.sales { "Sale" } else { "Purchase" };
        let ext_doc_no =
            if side.sales { header.str("External Document No.").to_string() } else { header.str(Side::vendor_doc_no_field(doc_type)).to_string() };
        let mut txn = Txn::new(ctx, if side.sales { "SALES" } else { "PURCHASES" }, "", "");
        txn.dim_set = header.get("Dimension Set ID").as_int();
        let mut gl: Vec<GlLine> = Vec::new();
        let mut fa_specs: Vec<crate::fa::FaEntrySpec> = Vec::new();
        let (mut total_amount, mut total_incl) = (Decimal::ZERO, Decimal::ZERO);
        let (mut total_amount_lcy, mut total_incl_lcy) = (Decimal::ZERO, Decimal::ZERO);
        let mut vat_specs: Vec<crate::vat::VatSpec> = Vec::new();

        for l in lines.iter().filter(|l| !to_invoice(l).is_zero()) {
            let qty = to_invoice(l);
            let amount_fcy = portion(l, "Amount", qty);
            let incl_fcy = portion(l, "Amount Including VAT", qty);
            total_amount += amount_fcy;
            total_incl += incl_fcy;
            // From here on: LCY.
            let amount = lcy(amount_fcy);
            let incl = lcy(incl_fcy);
            let vat = incl - amount;
            total_amount_lcy += amount;
            total_incl_lcy += incl;
            let line_no = l.get("Line No.").as_int();
            let gen_key = Key(vec![l.get("Gen. Bus. Posting Group").clone(), l.get("Gen. Prod. Posting Group").clone()]);
            let inventory_item = is_item_inventory(ctx, l)?;
            let account = match l.str("Type") {
                "G/L Account" => l.str("No.").to_string(),
                "Fixed Asset" if !side.sales => {
                    let (acc, spec) = crate::fa::acquisition_from_document(ctx, l.str("No."), if credit { -amount } else { amount }, posting_date, &line_desc_for(l, &description))?;
                    fa_specs.push(spec);
                    acc
                }
                "Item" => {
                    if side.sales {
                        let f = if credit { "Sales Credit Memo Account" } else { "Sales Account" };
                        setup_account(ctx, GEN_POSTING_SETUP, gen_key.clone(), f, "sales")?
                    } else if let Some(it) = &inventory_item {
                        inventory_account(ctx, it, l.str("Location Code"))?
                    } else {
                        let f = if credit { "Purch. Credit Memo Account" } else { "Purch. Account" };
                        setup_account(ctx, GEN_POSTING_SETUP, gen_key.clone(), f, "purchases")?
                    }
                }
                other => refuse!("Lines of type {other} cannot be posted yet (line {line_no})."),
            };
            let line_desc = if l.str("Description").is_empty() { description.clone() } else { l.str("Description").to_string() };
            gl.push(GlLine { account, amount: -g * amount, vat_amount: -g * vat, description: line_desc.clone(), gen_posting_type: gen_type });
            let vat_key = Key(vec![header.get("VAT Bus. Posting Group").clone(), l.get("VAT Prod. Posting Group").clone()]);
            if !vat.is_zero() {
                let f = if side.sales { "Sales VAT Account" } else { "Purchase VAT Account" };
                let vat_acc = setup_account(ctx, VAT_POSTING_SETUP, vat_key, f, "VAT")?;
                gl.push(GlLine { account: vat_acc, amount: -g * vat, vat_amount: Decimal::ZERO, description: line_desc.clone(), gen_posting_type: gen_type });
            } else if ctx.db.get(VAT_POSTING_SETUP, &vat_key).is_none() {
                let vt = ctx.table(VAT_POSTING_SETUP)?;
                refuse!("The VAT Posting Setup does not exist. Identification fields and values: {}", crate::db::key_text(vt, &vat_key));
            }
            crate::vat::add(
                &mut vat_specs,
                crate::vat::VatSpec {
                    gen_bus: l.str("Gen. Bus. Posting Group").to_string(),
                    gen_prod: l.str("Gen. Prod. Posting Group").to_string(),
                    vat_bus: header.str("VAT Bus. Posting Group").to_string(),
                    vat_prod: l.str("VAT Prod. Posting Group").to_string(),
                    vat_type: if side.sales { "Sale" } else { "Purchase" },
                    base: -g * amount,
                    amount: -g * vat,
                },
            );
            if let Some(it) = &inventory_item {
                // Invoices and credit memos move stock now; orders moved it when shipping.
                if !is_order {
                    let out = side.sales != credit;
                    let signed = if out { -qty * per(l) } else { qty * per(l) };
                    let cost = if side.sales { round_amount(it.get("Unit Cost").as_dec() * signed) } else if credit { -amount } else { amount };
                    let ile_doc = match (side.sales, credit) {
                        (true, false) => "Sales Invoice",
                        (true, true) => "Sales Credit Memo",
                        (false, false) => "Purchase Invoice",
                        (false, true) => "Purchase Credit Memo",
                    };
                    let sales_amount = if side.sales { if credit { -amount } else { amount } } else { Decimal::ZERO };
                    post_item_entry(ctx, side, it, posting_date, ile_doc, &posted_no, &line_desc, l.str("Location Code"), signed, cost, sales_amount, l.get("Unit of Measure Code"), &party_no)?;
                    if !side.sales && !credit && !qty.is_zero() {
                        update_item_cost(ctx, it.str("No."), qty * per(l), amount)?;
                    }
                }
                // Cost of goods sold for what is invoiced.
                if side.sales {
                    let cost = round_amount(it.get("Unit Cost").as_dec() * qty * per(l)) * if credit { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
                    if !cost.is_zero() {
                        let cogs = setup_account(ctx, GEN_POSTING_SETUP, gen_key.clone(), "COGS Account", "cost of goods sold")?;
                        let inv = inventory_account(ctx, it, l.str("Location Code"))?;
                        gl.push(GlLine { account: cogs, amount: cost, vat_amount: Decimal::ZERO, description: line_desc.clone(), gen_posting_type: "" });
                        gl.push(GlLine { account: inv, amount: -cost, vat_amount: Decimal::ZERO, description: line_desc.clone(), gen_posting_type: "" });
                    }
                }
            }
            let mut pl = plt.init_record();
            copy_common(l, plt, &mut pl);
            pl.set("Document No.", Value::text(&posted_no));
            pl.set("Quantity", dec(qty));
            pl.set("Line Amount", dec(portion(l, "Line Amount", qty)));
            pl.set("Line Discount Amount", dec(portion(l, "Line Discount Amount", qty)));
            pl.set("Amount", dec(amount_fcy));
            pl.set("Amount Including VAT", dec(incl_fcy));
            ctx.db.insert(plt, pl)?;
        }

        gl.push(GlLine { account: party_account, amount: g * total_incl_lcy, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" });
        let source_type = if side.sales { "Customer" } else { "Vendor" };
        post_gl(
            ctx,
            &GlDoc { posting_date, doc_type: ledger_doc_type, doc_no: &posted_no, ext_doc_no: &ext_doc_no, source_type, source_no: &bill_no, txn: &txn },
            gl,
        )?;
        for spec in &fa_specs {
            crate::fa::post_entry(ctx, &txn, spec, posting_date, ledger_doc_type, &posted_no, &ext_doc_no)?;
        }
        crate::vat::post(
            ctx,
            &txn,
            &crate::vat::VatDoc {
                posting_date,
                document_date: header.get("Document Date").as_date(),
                doc_type: ledger_doc_type,
                doc_no: &posted_no,
                ext_doc_no: &ext_doc_no,
                bill_to: &bill_no,
                vat_reg_no: header.str("VAT Registration No."),
                country: header.str(&format!("{}Country/Region Code", side.addr)),
            },
            &vat_specs,
        )?;
        let due = header.get("Due Date").as_date();
        let currency = header.str("Currency Code").to_string();
        let person = header.str(side.person).to_string();
        let group = header.str(side.posting_group).to_string();
        let party_entry = post_party_entry(
            ctx,
            side,
            &txn,
            &PartyEntry {
                party_no: &bill_no,
                posting_date,
                document_date: header.get("Document Date").as_date(),
                doc_type: ledger_doc_type,
                doc_no: &posted_no,
                ext_doc_no: &ext_doc_no,
                description: &description,
                currency: &currency,
                amount: g * total_incl,
                amount_lcy: g * total_incl_lcy,
                sales_purch: if side.sales { g * total_amount_lcy } else { -g * total_amount_lcy },
                due_date: due,
                person: &person,
                posting_group: &group,
            },
        )?;
        // Payment discount possible (BC: % of the amount including VAT, until the discount date).
        let disc_pct = header.get("Payment Discount %").as_dec();
        if !disc_pct.is_zero() {
            let let_ = ctx.table(side.ledger)?;
            let mut e = ctx.get(side.ledger, &Key(vec![Value::Int(party_entry)]))?;
            let disc = round_amount(g * total_incl * disc_pct / Decimal::ONE_HUNDRED);
            e.set("Pmt. Discount Date", header.get("Pmt. Discount Date").clone());
            e.set("Original Pmt. Disc. Possible", dec(disc));
            e.set("Remaining Pmt. Disc. Possible", dec(disc));
            ctx.db.modify(let_, e)?;
        }
        let mut ph = pht.init_record();
        copy_common(&header, pht, &mut ph);
        ph.set("No.", Value::text(&posted_no));
        if is_order {
            ph.set("Order No.", Value::text(no));
        } else {
            ph.set("Pre-Assigned No.", Value::text(no));
        }
        ph.set("User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.insert(pht, ph)?;
        copy_comments(ctx, side, doc_type, no, if credit { "Posted Credit Memo" } else { "Posted Invoice" }, &posted_no)?;
        result.insert("posted_table".into(), json!(ph_table));
        result.insert("posted_no".into(), json!(posted_no));
        result.insert("amount".into(), json!(total_amount.to_string()));
        result.insert("amount_including_vat".into(), json!(total_incl.to_string()));
        messages.push(format!("{} {posted_no}", if credit { "credit memo" } else { "invoice" }));
    }

    // ---- Update or remove the source document ------------------------------------------------
    let mut complete = true;
    for mut l in lines {
        if !real(&l) {
            ctx.db.modify(lt, l)?;
            continue;
        }
        let s = to_ship(&l);
        let i = to_invoice(&l);
        let qty = l.get("Quantity").as_dec();
        let new_shipped = if is_order { shipped(&l) + s } else { invoiced(&l) + i };
        let new_invoiced = invoiced(&l) + i;
        let outstanding = qty - new_shipped;
        l.set(side.qty_handled, dec(new_shipped));
        l.set("Quantity Invoiced", dec(new_invoiced));
        l.set(if side.sales { "Qty. Shipped Not Invoiced" } else { "Qty. Rcd. Not Invoiced" }, dec(new_shipped - new_invoiced));
        l.set("Outstanding Quantity", dec(outstanding));
        l.set(side.qty_handle, dec(outstanding));
        l.set("Qty. to Invoice", dec(new_shipped + outstanding - new_invoiced));
        if new_shipped != qty || new_invoiced != qty {
            complete = false;
        }
        ctx.db.modify(lt, l)?;
    }
    let hkey = header.key(ht);
    let deleted = !is_order || complete;
    if deleted {
        let keys: Vec<Key> = ctx
            .db
            .rows(side.line)
            .filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no)
            .map(|l| l.key(lt))
            .collect();
        for k in keys {
            ctx.db.remove(side.line, &k);
        }
        ctx.db.remove(side.header, &hkey);
    } else {
        let mut h = header.clone();
        h.set("Last Posting No.", Value::text(if posted_no.is_empty() { &shipment_no } else { &posted_no }));
        ctx.db.modify(ht, h)?;
    }
    let open = if !posted_no.is_empty() {
        json!({"page": posted_page(side, credit), "key": [posted_no]})
    } else {
        json!({"page": if side.sales { "posted-sales-shipment" } else { "posted-purchase-receipt" }, "key": [shipment_no]})
    };
    result.insert("source_document_deleted".into(), json!(deleted));
    result.insert("open".into(), open);
    result.insert("message".into(), json!(format!("{doc_type} {no} was posted: {}.", messages.join(" and "))));
    Ok(Json::Object(result))
}

fn line_desc_for(l: &Record, fallback: &str) -> String {
    if l.str("Description").is_empty() { fallback.to_string() } else { l.str("Description").to_string() }
}

pub fn posted_page(side: &Side, credit: bool) -> &'static str {
    match (side.sales, credit) {
        (true, false) => "posted-sales-invoice",
        (true, true) => "posted-sales-credit-memo",
        (false, false) => "posted-purchase-invoice",
        (false, true) => "posted-purchase-credit-memo",
    }
}

/// The document page of a sales / purchase document type.
pub fn document_page(side: &Side, doc_type: &str) -> &'static str {
    let pages = if side.sales { SALES_DOC_PAGES } else { PURCH_DOC_PAGES };
    pages.iter().find(|p| p.0 == doc_type).map_or(if side.sales { "sales-order" } else { "purchase-order" }, |p| p.4)
}

/// Average cost after a purchase of `qty` for `amount`; also records the last direct cost.
pub fn update_item_cost(ctx: &mut Ctx, item_no: &str, qty: Decimal, amount: Decimal) -> Result<()> {
    let it = ctx.table(ITEM)?;
    let mut item = ctx.get_code(ITEM, item_no)?;
    // Inventory before this receipt (its entry is already written).
    let inv_after: Decimal = ctx.db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == item_no).map(|e| e.get("Quantity").as_dec()).sum();
    let inv_before = (inv_after - qty).max(Decimal::ZERO);
    let old_cost = item.get("Unit Cost").as_dec();
    let unit = round_unit(amount / qty);
    let new_cost =
        if inv_before + qty > Decimal::ZERO { round_unit((inv_before * old_cost + amount) / (inv_before + qty)) } else { unit };
    item.set("Last Direct Cost", dec(unit));
    item.set("Unit Cost", dec(new_cost));
    ctx.db.modify(it, item)
}

/// BC's CopyCommentLines: the document's comments go with it to the posted document.
fn copy_comments(ctx: &mut Ctx, side: &Side, from_type: &str, from_no: &str, to_type: &str, to_no: &str) -> Result<()> {
    let table = if side.sales { crate::schema::comments::SALES_COMMENT } else { crate::schema::comments::PURCH_COMMENT };
    let t = ctx.table(table)?;
    let comments: Vec<Record> = ctx.db.rows(table).filter(|c| c.str("Document Type") == from_type && c.str("No.") == from_no).cloned().collect();
    for mut c in comments {
        c.set("Document Type", Value::text(to_type));
        c.set("No.", Value::text(to_no));
        ctx.db.insert(t, c)?;
    }
    Ok(())
}
