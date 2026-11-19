//! Posting: turning documents and payments into posted documents and ledger entries. Sales and
//! purchase share one routine ([`post_document`]) parameterised by [`Side`].

pub mod payment;

use chrono::NaiveDate;
use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::ctx::{Ctx, dec, round_amount, round_unit, test_field};
use crate::error::{EngineError, Result};
use crate::noseries;
use crate::refuse;
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

/// Writes G/L entries for `lines`; refuses an unbalanced transaction or an account that can't be
/// posted to.
pub fn post_gl(ctx: &mut Ctx, doc: &GlDoc, lines: Vec<GlLine>) -> Result<()> {
    let total: Decimal = lines.iter().map(|l| l.amount).sum();
    if !total.is_zero() {
        refuse!(
            "The transaction cannot be completed because it will cause inconsistencies in the G/L Entry table. It is out of balance by {total}."
        );
    }
    let t = ctx.table(GL_ENTRY)?;
    let mut entry_no = ctx.db.next_entry_no(GL_ENTRY);
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
        e.set("User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.insert(t, e)?;
        entry_no = entry_no.saturating_add(1);
    }
    Ok(())
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

/// Posts a sales or purchase document. Orders post what is in Qty. to Invoice (shipping /
/// receiving the same quantity); invoices and credit memos post in full and are deleted.
pub fn post_document(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    let lt = ctx.table(side.line)?;
    let header = ctx.get(side.header, &Key(vec![Value::text(doc_type), Value::text(no)]))?;
    if matches!(doc_type, "Quote" | "Blanket Order") {
        refuse!("A {doc_type} cannot be posted. Convert it to an order first (Make Order).");
    }
    let credit = matches!(doc_type, "Credit Memo" | "Return Order");
    let is_order = matches!(doc_type, "Order" | "Return Order");
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
    if blocked == "All" || (side.sales && blocked == "Invoice") || (side.sales && is_order && blocked == "Ship") {
        refuse!("You cannot post this type of document when {} {party_no} is blocked with type {blocked}.", side.party_caption);
    }
    let setup = ctx.setup(side.setup)?;
    if setup.get("Ext. Doc. No. Mandatory").as_bool() {
        let f = if side.sales { "External Document No." } else { Side::vendor_doc_no_field(doc_type) };
        test_field(ht, &header, f)?;
    }
    let party_account = if side.sales {
        setup_account(ctx, CUST_POSTING_GROUP, Key(vec![header.get(side.posting_group).clone()]), "Receivables Account", "receivables")?
    } else {
        setup_account(ctx, VEND_POSTING_GROUP, Key(vec![header.get(side.posting_group).clone()]), "Payables Account", "payables")?
    };

    let lines: Vec<Record> = ctx
        .db
        .rows(side.line)
        .filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no)
        .cloned()
        .collect();
    let qty_to_post = |l: &Record| if is_order { l.get("Qty. to Invoice").as_dec() } else { l.get("Outstanding Quantity").as_dec() };
    let postable: Vec<Record> = lines
        .iter()
        .filter(|l| !l.str("Type").is_empty() && !l.str("No.").is_empty() && !qty_to_post(l).is_zero())
        .cloned()
        .collect();
    if postable.is_empty() {
        refuse!("There is nothing to post.");
    }

    let nos_field = if credit { "Posted Credit Memo Nos." } else { "Posted Invoice Nos." };
    let series = noseries::series_from_setup(ctx, side.setup, nos_field)?;
    let posted_no = noseries::next(ctx, &series)?;
    let (ph_table, pl_table) = if credit { (side.cr_header, side.cr_line) } else { (side.inv_header, side.inv_line) };
    let pht = ctx.table(ph_table)?;
    let plt = ctx.table(pl_table)?;
    let ledger_doc_type = if credit { "Credit Memo" } else { "Invoice" };
    let gen_type = if side.sales { "Sale" } else { "Purchase" };
    let description = header.str("Posting Description").to_string();
    let ext_doc_no = if side.sales {
        header.str("External Document No.").to_string()
    } else {
        header.str(Side::vendor_doc_no_field(doc_type)).to_string()
    };

    let mut gl: Vec<GlLine> = Vec::new();
    let mut total_amount = Decimal::ZERO;
    let mut total_incl = Decimal::ZERO;
    let mut ile_no = ctx.db.next_entry_no(ITEM_LEDGER);
    let ilt = ctx.table(ITEM_LEDGER)?;

    for l in &postable {
        let qty = qty_to_post(l);
        let full = l.get("Quantity").as_dec();
        let ratio = |field: &str| {
            let v = l.get(field).as_dec();
            if qty == full || full.is_zero() { v } else { round_amount(v * qty / full) }
        };
        let line_amount = ratio("Line Amount");
        let amount = ratio("Amount");
        let incl = ratio("Amount Including VAT");
        let vat = incl - amount;
        total_amount += amount;
        total_incl += incl;
        let line_no = l.get("Line No.").as_int();
        let gen_key = Key(vec![l.get("Gen. Bus. Posting Group").clone(), l.get("Gen. Prod. Posting Group").clone()]);
        let mut item: Option<Record> = None;
        let account = match l.str("Type") {
            "G/L Account" => l.str("No.").to_string(),
            "Item" => {
                let it = ctx.get_code(ITEM, l.str("No."))?;
                let acc = if side.sales {
                    let f = if credit { "Sales Credit Memo Account" } else { "Sales Account" };
                    setup_account(ctx, GEN_POSTING_SETUP, gen_key.clone(), f, "sales")?
                } else if it.str("Type") == "Inventory" {
                    setup_account(
                        ctx,
                        INVT_POSTING_SETUP,
                        Key(vec![Value::text(""), it.get("Inventory Posting Group").clone()]),
                        "Inventory Account",
                        "inventory",
                    )?
                } else {
                    let f = if credit { "Purch. Credit Memo Account" } else { "Purch. Account" };
                    setup_account(ctx, GEN_POSTING_SETUP, gen_key.clone(), f, "purchases")?
                };
                item = Some(it);
                acc
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

        // Inventory: item ledger entry and, for sales, cost of goods sold.
        if let Some(it) = item
            && it.str("Type") == "Inventory"
        {
            // Quantity sign: sales invoice / purchase credit memo take stock out.
            let out = side.sales != credit;
            let signed_qty = if out { -qty } else { qty };
            let (cost, sales_amount) = if side.sales {
                let unit_cost = it.get("Unit Cost").as_dec();
                (round_amount(unit_cost * signed_qty), if credit { -amount } else { amount })
            } else {
                (if credit { -amount } else { amount }, Decimal::ZERO)
            };
            let mut e = ilt.init_record();
            e.set("Entry No.", Value::Int(ile_no));
            ile_no = ile_no.saturating_add(1);
            e.set("Item No.", Value::text(it.str("No.")));
            e.set("Posting Date", Value::Date(posting_date));
            e.set("Entry Type", Value::text(if side.sales { "Sale" } else { "Purchase" }));
            let ile_doc = match (side.sales, credit) {
                (true, false) => "Sales Invoice",
                (true, true) => "Sales Credit Memo",
                (false, false) => "Purchase Invoice",
                (false, true) => "Purchase Credit Memo",
            };
            e.set("Document Type", Value::text(ile_doc));
            e.set("Document No.", Value::text(&posted_no));
            e.set("Description", Value::text(&line_desc));
            e.set("Quantity", dec(signed_qty));
            e.set("Remaining Quantity", dec(signed_qty.max(Decimal::ZERO)));
            e.set("Open", Value::Bool(signed_qty > Decimal::ZERO));
            e.set("Cost Amount (Actual)", dec(cost));
            e.set("Sales Amount (Actual)", dec(sales_amount));
            e.set("Unit of Measure Code", l.get("Unit of Measure Code").clone());
            e.set("Source Type", Value::text(if side.sales { "Customer" } else { "Vendor" }));
            e.set("Source No.", Value::text(&party_no));
            if side.sales && !cost.is_zero() {
                let cogs = setup_account(ctx, GEN_POSTING_SETUP, gen_key, "COGS Account", "cost of goods sold")?;
                let inv = setup_account(
                    ctx,
                    INVT_POSTING_SETUP,
                    Key(vec![Value::text(""), it.get("Inventory Posting Group").clone()]),
                    "Inventory Account",
                    "inventory",
                )?;
                gl.push(GlLine { account: cogs, amount: -cost, vat_amount: Decimal::ZERO, description: line_desc.clone(), gen_posting_type: "" });
                gl.push(GlLine { account: inv, amount: cost, vat_amount: Decimal::ZERO, description: line_desc.clone(), gen_posting_type: "" });
            }
            ctx.db.insert(ilt, e)?;
            if !side.sales && !credit && !qty.is_zero() {
                update_item_cost(ctx, it.str("No."), qty, amount)?;
            }
        }

        // Posted line.
        let mut pl = plt.init_record();
        copy_common(l, plt, &mut pl);
        pl.set("Document No.", Value::text(&posted_no));
        pl.set("Quantity", dec(qty));
        pl.set("Line Amount", dec(line_amount));
        pl.set("Line Discount Amount", dec(ratio("Line Discount Amount")));
        pl.set("Amount", dec(amount));
        pl.set("Amount Including VAT", dec(incl));
        ctx.db.insert(plt, pl)?;
    }

    // Receivable / payable for the whole document.
    gl.push(GlLine { account: party_account, amount: g * total_incl, vat_amount: Decimal::ZERO, description: description.clone(), gen_posting_type: "" });
    let bill = ctx.get_code(side.party_table, &bill_no)?;
    let source_type = if side.sales { "Customer" } else { "Vendor" };
    post_gl(
        ctx,
        &GlDoc { posting_date, doc_type: ledger_doc_type, doc_no: &posted_no, ext_doc_no: &ext_doc_no, source_type, source_no: &bill_no },
        gl,
    )?;

    // Customer / vendor ledger entry.
    let let_ = ctx.table(side.ledger)?;
    let mut e = let_.init_record();
    e.set("Entry No.", Value::Int(ctx.db.next_entry_no(side.ledger)));
    e.set(side.ledger_party, Value::text(&bill_no));
    e.set(if side.sales { "Customer Name" } else { "Vendor Name" }, bill.get("Name").clone());
    e.set("Posting Date", Value::Date(posting_date));
    e.set("Document Type", Value::text(ledger_doc_type));
    e.set("Document No.", Value::text(&posted_no));
    e.set("External Document No.", Value::text(&ext_doc_no));
    e.set("Description", Value::text(&description));
    e.set("Currency Code", header.get("Currency Code").clone());
    e.set("Amount", dec(g * total_incl));
    e.set("Amount (LCY)", dec(g * total_incl));
    e.set("Remaining Amount", dec(g * total_incl));
    e.set(if side.sales { "Sales (LCY)" } else { "Purchase (LCY)" }, dec(if side.sales { g * total_amount } else { -g * total_amount }));
    e.set("Due Date", header.get("Due Date").clone());
    e.set("Open", Value::Bool(!total_incl.is_zero()));
    e.set(side.person, header.get(side.person).clone());
    e.set(side.posting_group, header.get(side.posting_group).clone());
    e.set("User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.insert(let_, e)?;

    // Posted header.
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

    // Update or remove the source document.
    let mut fully_posted = true;
    for mut l in lines {
        let qty = if postable.iter().any(|p| p.get("Line No.") == l.get("Line No.")) { qty_to_post(&l) } else { Decimal::ZERO };
        let invoiced = l.get("Quantity Invoiced").as_dec() + qty;
        let outstanding = l.get("Quantity").as_dec() - invoiced;
        l.set("Quantity Invoiced", dec(invoiced));
        l.set(side.qty_handled, dec(l.get(side.qty_handled).as_dec() + qty));
        l.set("Outstanding Quantity", dec(outstanding));
        l.set(side.qty_handle, dec(outstanding));
        l.set("Qty. to Invoice", dec(outstanding));
        if !outstanding.is_zero() && !l.str("Type").is_empty() {
            fully_posted = false;
        }
        ctx.db.modify(lt, l)?;
    }
    let hkey = header.key(ht);
    if !is_order || fully_posted {
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
        h.set("Last Posting No.", Value::text(&posted_no));
        ctx.db.modify(ht, h)?;
    }
    let page = posted_page(side, credit);
    Ok(json!({
        "posted_table": ph_table,
        "posted_no": posted_no,
        "amount": total_amount.to_string(),
        "amount_including_vat": total_incl.to_string(),
        "source_document_deleted": !is_order || fully_posted,
        "open": {"page": page, "key": [posted_no]},
        "message": format!("{} {no} was posted as {} {posted_no}.", doc_type, if credit { "credit memo" } else { "invoice" }),
    }))
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
fn update_item_cost(ctx: &mut Ctx, item_no: &str, qty: Decimal, amount: Decimal) -> Result<()> {
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
