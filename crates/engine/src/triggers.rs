//! Table triggers (AL `OnInsert`, `OnModify`, `OnDelete`, field `OnValidate`). They run on a copy
//! of the record; [`crate::records`] stores it afterwards.

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;

use crate::ctx::{Ctx, dec, round_amount, round_unit, test_field_eq};
use crate::error::Result;
use crate::noseries;
use crate::refuse;
use crate::schema::tables::{
    BANK_ACCOUNT, BANK_LEDGER, CUST_LEDGER, CUSTOMER, GL_ACCOUNT, GL_ENTRY, INVENTORY_SETUP, ITEM, ITEM_LEDGER,
    PAYMENT_TERMS, PURCH_CR_HEADER, PURCH_CR_LINE, PURCH_INV_HEADER, PURCH_INV_LINE, SALES_CR_HEADER, SALES_CR_LINE,
    SALES_INV_HEADER, SALES_INV_LINE, VAT_POSTING_SETUP, VEND_LEDGER, VENDOR,
};
use crate::side::Side;

pub fn on_insert(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<()> {
    if matches!(t.name.as_str(), crate::schema::comments::COMMENT_LINE | crate::schema::comments::SALES_COMMENT | crate::schema::comments::PURCH_COMMENT) {
        comment_setup_new_line(ctx, t, r);
        return Ok(());
    }
    if t.name == crate::schema::finance::GEN_JNL_LINE {
        crate::journal::on_insert(ctx, r)?;
        return crate::approvals::on_journal_line_insert(ctx, &t.name, r);
    }
    if t.name == crate::schema::inventory::ITEM_JNL_LINE {
        crate::item_journal::on_insert(ctx, r)?;
        return crate::approvals::on_journal_line_insert(ctx, &t.name, r);
    }
    if crate::tracking::on_insert(ctx, t, r) {
        return Ok(());
    }
    if crate::transfer::on_insert(ctx, t, r)? {
        return Ok(());
    }
    if let Some(done) = crate::item_setup::on_insert(ctx, t, r)? {
        return Ok(done);
    }
    if t.name == crate::recurring::ALLOCATION {
        crate::recurring::on_allocation_insert(ctx, r);
        return Ok(());
    }
    if t.name == crate::schema::bank_rec::BANK_REC {
        return crate::bank_rec::on_insert(ctx, r);
    }
    if t.name == crate::schema::fiscal::GL_BUDGET_ENTRY {
        crate::fiscal::on_budget_insert(ctx, r);
        return Ok(());
    }
    if t.name == crate::schema::fiscal::ACCOUNTING_PERIOD {
        crate::fiscal::on_period_insert(r);
        return Ok(());
    }
    if t.name == crate::schema::approvals::APPROVAL_COMMENT {
        crate::approvals::on_comment_insert(ctx, r);
        return Ok(());
    }
    if t.name == crate::schema::fixed_assets::FIXED_ASSET {
        return crate::fa::on_insert(ctx, r);
    }
    match t.name.as_str() {
        CUSTOMER | VENDOR | ITEM => {
            if r.str("No.").is_empty() {
                let (setup, field) = match t.name.as_str() {
                    CUSTOMER => (crate::schema::tables::SALES_SETUP, "Customer Nos."),
                    VENDOR => (crate::schema::tables::PURCH_SETUP, "Vendor Nos."),
                    _ => (INVENTORY_SETUP, "Item Nos."),
                };
                let series = noseries::series_from_setup(ctx, setup, field)?;
                let no = noseries::next(ctx, &series)?;
                r.set("No.", Value::text(no));
            }
            touch_master(ctx, t, r);
        }
        _ => {}
    }
    if let Some(side) = Side::for_header(&t.name) {
        let doc_type = r.str("Document Type").to_string();
        if r.str("No.").is_empty() {
            let series = noseries::series_from_setup(ctx, side.setup, side.nos_field(&doc_type))?;
            let no = noseries::next(ctx, &series)?;
            r.set("No.", Value::text(no));
        }
        let wd = Value::Date(ctx.work_date);
        r.set("Document Date", wd.clone());
        if doc_type != "Quote" {
            r.set("Posting Date", wd.clone());
        }
        if matches!(doc_type.as_str(), "Order" | "Quote") {
            r.set("Order Date", wd);
        }
        r.set("Status", Value::text("Open"));
        r.set("Assigned User ID", Value::text(ctx.user.to_uppercase()));
        r.set("Posting Description", Value::text(format!("{doc_type} {}", r.str("No."))));
    }
    if let Some(side) = Side::for_line(&t.name) {
        let header = line_header(ctx, side, r)?;
        test_status_open(ctx, side, &header)?;
        if r.get("Line No.").as_int() == 0 {
            let last = ctx
                .db
                .rows(side.line)
                .filter(|l| same_doc(l, r))
                .map(|l| l.get("Line No.").as_int())
                .max()
                .unwrap_or(0);
            r.set("Line No.", Value::Int(last.saturating_add(10000)));
        }
        r.set(side.party_no, header.get(side.party_no).clone());
        r.set("Gen. Bus. Posting Group", header.get("Gen. Bus. Posting Group").clone());
        r.set("VAT Bus. Posting Group", header.get("VAT Bus. Posting Group").clone());
        if r.str("Location Code").is_empty() {
            r.set("Location Code", header.get("Location Code").clone());
        }
    }
    Ok(())
}

pub fn on_modify(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<()> {
    if matches!(t.name.as_str(), CUSTOMER | VENDOR | ITEM) {
        touch_master(ctx, t, r);
    }
    Ok(())
}

pub fn on_delete(ctx: &mut Ctx, t: &TableDef, r: &Record) -> Result<()> {
    let name = t.name.as_str();
    if name == crate::schema::fixed_assets::FIXED_ASSET {
        return crate::fa::on_delete_fa(ctx, r);
    }
    if matches!(name, GL_ENTRY | CUST_LEDGER | VEND_LEDGER | ITEM_LEDGER | BANK_LEDGER | crate::schema::fixed_assets::FA_LEDGER) {
        refuse!("You cannot delete {} entries. Ledger entries are permanent; post a correction instead.", t.caption);
    }
    if matches!(
        name,
        SALES_INV_HEADER
            | SALES_INV_LINE
            | SALES_CR_HEADER
            | SALES_CR_LINE
            | PURCH_INV_HEADER
            | PURCH_INV_LINE
            | PURCH_CR_HEADER
            | PURCH_CR_LINE
    ) {
        refuse!("You cannot delete a posted document ({}). Create a corrective credit memo instead.", t.caption);
    }
    match name {
        CUSTOMER | VENDOR => {
            let side = if name == CUSTOMER { &crate::side::SALES } else { &crate::side::PURCHASE };
            let no = r.str("No.");
            if ctx.db.rows(side.header).any(|h| h.str(side.party_no) == no || h.str(side.bill_no) == no) {
                refuse!("You cannot delete {} {no} because there is at least one outstanding {} document for it.", t.caption, if side.sales { "sales" } else { "purchase" });
            }
            if ctx.db.rows(side.ledger).any(|e| e.str(side.ledger_party) == no && e.get("Open").as_bool()) {
                refuse!("You cannot delete {} {no} because it has open ledger entries.", t.caption);
            }
        }
        ITEM => {
            let no = r.str("No.");
            if ctx.db.rows(ITEM_LEDGER).any(|e| e.str("Item No.") == no) {
                refuse!("You cannot delete Item {no} because it has ledger entries.");
            }
            for side in [&crate::side::SALES, &crate::side::PURCHASE] {
                if ctx.db.rows(side.line).any(|l| l.str("Type") == "Item" && l.str("No.") == no) {
                    refuse!("You cannot delete Item {no} because there are one or more outstanding documents that include this item.");
                }
            }
        }
        GL_ACCOUNT => {
            let no = r.str("No.");
            if ctx.db.rows(GL_ENTRY).any(|e| e.str("G/L Account No.") == no) {
                refuse!("You cannot delete G/L Account {no} because it has ledger entries.");
            }
        }
        BANK_ACCOUNT => {
            let no = r.str("No.");
            if ctx.db.rows(BANK_LEDGER).any(|e| e.str("Bank Account No.") == no) {
                refuse!("You cannot delete Bank Account {no} because it has ledger entries.");
            }
        }
        _ => {}
    }
    if let Some(side) = Side::for_header(name) {
        let keys: Vec<Key> = {
            let lt = ctx.table(side.line)?;
            ctx.db.rows(side.line).filter(|l| same_doc_header(l, r)).map(|l| l.key(lt)).collect()
        };
        for k in keys {
            ctx.db.remove(side.line, &k);
        }
    }
    if let Some(side) = Side::for_line(name) {
        // A line whose header is already gone (deleted with it) needs no status check.
        if let Ok(header) = line_header(ctx, side, r) {
            test_status_open(ctx, side, &header)?;
        }
    }
    Ok(())
}

/// Field `OnValidate`: runs after the new value is set; `old` is the previous value.
pub fn on_validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str, old: &Value) -> Result<()> {
    crate::dimensions::on_validate(ctx, t, r, field)?;
    crate::item_setup::on_validate(ctx, t, r, field)?;
    crate::transfer::on_validate(ctx, t, r, field)?;
    crate::tracking::on_validate(ctx, t, r, field)?;
    crate::item_charge::on_validate(ctx, t, r, field);
    if t.name == crate::schema::bank_rec::BANK_REC_LINE {
        crate::bank_rec::on_line_validate(ctx, r, field);
    }
    if t.name == crate::schema::approvals::APPROVAL_COMMENT && field == "Record ID to Approve" {
        crate::approvals::comment_record_table(ctx, r);
    }
    if matches!(t.name.as_str(), CUSTOMER | VENDOR) && field == "Name" {
        r.set("Search Name", Value::text(r.str("Name").to_uppercase()));
    }
    if t.name == ITEM && field == "Description" {
        r.set("Search Name", Value::text(r.str("Description").to_uppercase()));
    }
    if t.name == crate::schema::finance::GEN_JNL_LINE {
        return crate::journal::on_validate(ctx, r, field);
    }
    if t.name == crate::schema::inventory::ITEM_JNL_LINE {
        return crate::item_journal::on_validate(ctx, r, field);
    }
    if t.name == crate::schema::fixed_assets::FIXED_ASSET {
        return crate::fa::on_validate_fa(ctx, r, field);
    }
    if t.name == crate::schema::fixed_assets::FA_DEPR_BOOK {
        return crate::fa::on_validate_book(ctx, r, field);
    }
    if let Some(side) = Side::for_header(&t.name) {
        validate_header(ctx, side, t, r, field, old)?;
        return crate::dimensions::after_header_validate(ctx, side, t, r, field);
    }
    if let Some(side) = Side::for_line(&t.name) {
        validate_line(ctx, side, r, field, old)?;
        return crate::dimensions::after_line_validate(ctx, side, r, field);
    }
    Ok(())
}

fn touch_master(ctx: &Ctx, t: &TableDef, r: &mut Record) {
    r.set("Last Date Modified", Value::Date(ctx.work_date));
    let name_field = if t.name == ITEM { "Description" } else { "Name" };
    if t.field("Search Name").is_some() {
        r.set("Search Name", Value::text(r.str(name_field).to_uppercase()));
    }
}

fn same_doc(a: &Record, b: &Record) -> bool {
    a.get("Document Type") == b.get("Document Type") && a.get("Document No.") == b.get("Document No.")
}

fn same_doc_header(line: &Record, header: &Record) -> bool {
    line.get("Document Type") == header.get("Document Type") && line.get("Document No.") == header.get("No.")
}

/// The header of a document line.
pub fn line_header(ctx: &Ctx, side: &Side, line: &Record) -> Result<Record> {
    ctx.get(side.header, &Key(vec![line.get("Document Type").clone(), line.get("Document No.").clone()]))
}

/// BC's UOMMgt.GetQtyPerUnitOfMeasure: 1 for the base unit (or none), else the item's unit.
pub fn qty_per_uom(ctx: &Ctx, item: &Record, code: &str) -> Result<Decimal> {
    if code.is_empty() || code == item.str("Base Unit of Measure") {
        return Ok(Decimal::ONE);
    }
    let no = item.str("No.");
    match ctx.db.get(crate::schema::item_setup::ITEM_UOM, &Key(vec![Value::text(no), Value::text(code)])) {
        Some(u) if !u.get("Qty. per Unit of Measure").as_dec().is_zero() => Ok(u.get("Qty. per Unit of Measure").as_dec()),
        Some(_) => refuse!("Qty. per Unit of Measure must have a value in Item Unit of Measure: Item No.={no},Code={code}. It cannot be zero or empty."),
        None => refuse!("The Item Unit of Measure does not exist. Identification fields and values: Item No.='{no}',Code='{code}'"),
    }
}

/// AL `TestStatusOpen`.
pub fn test_status_open(ctx: &Ctx, side: &Side, header: &Record) -> Result<()> {
    let t = ctx.table(side.header)?;
    test_field_eq(t, header, "Status", "Open")
}

fn validate_header(ctx: &mut Ctx, side: &Side, t: &TableDef, r: &mut Record, field: &str, old: &Value) -> Result<()> {
    let status_fields =
        [side.party_no, side.bill_no, "Prices Including VAT", "Currency Code", "Gen. Bus. Posting Group", "VAT Bus. Posting Group"];
    if status_fields.contains(&field) && r.get(field) != old {
        test_field_eq(t, r, "Status", "Open")?;
    }
    match field {
        f if f == side.party_no => {
            let no = r.str(side.party_no).to_string();
            if r.get(side.party_no) != old && !old.is_blank() && has_lines(ctx, side, r) {
                refuse!(
                    "You cannot change {} because the document already has lines. Delete the lines first.",
                    side.party_no
                );
            }
            if no.is_empty() {
                return Ok(());
            }
            let party = ctx.get_code(side.party_table, &no)?;
            if party.str("Blocked") == "All" {
                refuse!("You cannot create this type of document when {} {no} is blocked with type All.", side.party_caption);
            }
            r.set(side.party_name, party.get("Name").clone());
            for f in ["Address", "Address 2", "City", "Post Code", "Country/Region Code"] {
                r.set(&format!("{}{f}", side.addr), party.get(f).clone());
            }
            r.set(&format!("{}Contact", side.addr), party.get("Contact").clone());
            if side.sales {
                r.set("Sell-to Phone No.", party.get("Phone No.").clone());
                r.set("Sell-to E-Mail", party.get("E-Mail").clone());
            }
            r.set("Location Code", party.get("Location Code").clone());
            let person_src = if side.sales { "Salesperson Code" } else { "Purchaser Code" };
            r.set(side.person, party.get(person_src).clone());
            let bill = match party.str(side.party_bill_field) {
                "" => no,
                b => b.to_string(),
            };
            r.set(side.bill_no, Value::text(bill));
            apply_bill_to(ctx, side, r)
        }
        f if f == side.bill_no => apply_bill_to(ctx, side, r),
        "Posting Date" => {
            update_currency_factor(ctx, r)?;
            update_due_date(ctx, r)
        }
        "Document Date" | "Payment Terms Code" => update_due_date(ctx, r),
        "Location Code" => {
            test_field_eq(t, r, "Status", "Open")?;
            let lt = ctx.table(side.line)?;
            let lines: Vec<Record> = ctx.db.rows(side.line).filter(|l| same_doc_header(l, r)).cloned().collect();
            for mut l in lines {
                if !l.get(side.qty_handled).as_dec().is_zero() {
                    continue;
                }
                l.set("Location Code", r.get("Location Code").clone());
                ctx.db.modify(lt, l)?;
            }
            Ok(())
        }
        "Currency Code" => {
            update_currency_factor(ctx, r)?;
            update_lines(ctx, side, r)
        }
        "Prices Including VAT" | "VAT Bus. Posting Group" | "Gen. Bus. Posting Group" => update_lines(ctx, side, r),
        _ => Ok(()),
    }
}

fn has_lines(ctx: &Ctx, side: &Side, header: &Record) -> bool {
    ctx.db.rows(side.line).any(|l| same_doc_header(l, header))
}

fn apply_bill_to(ctx: &mut Ctx, side: &Side, r: &mut Record) -> Result<()> {
    let no = r.str(side.bill_no).to_string();
    if no.is_empty() {
        return Ok(());
    }
    let bill = ctx.get_code(side.party_table, &no)?;
    r.set(side.bill_name, bill.get("Name").clone());
    for f in ["Gen. Bus. Posting Group", "VAT Bus. Posting Group", "Currency Code", "Prices Including VAT", "Payment Terms Code", "Payment Method Code"] {
        r.set(f, bill.get(f).clone());
    }
    r.set(side.posting_group, bill.get(side.posting_group).clone());
    if side.sales {
        r.set("VAT Registration No.", bill.get("VAT Registration No.").clone());
    }
    update_due_date(ctx, r)?;
    update_currency_factor(ctx, r)?;
    update_lines(ctx, side, r)
}

/// BC's UpdateCurrencyFactor: the rate of the posting date (or work date); 0 for LCY.
fn update_currency_factor(ctx: &Ctx, r: &mut Record) -> Result<()> {
    let code = r.str("Currency Code").to_string();
    if code.is_empty() {
        r.set("Currency Factor", dec(Decimal::ZERO));
        return Ok(());
    }
    let date = r.get("Posting Date").as_date().unwrap_or(ctx.work_date);
    r.set("Currency Factor", dec(crate::currency::factor(ctx, &code, date)?));
    Ok(())
}

fn update_due_date(ctx: &Ctx, r: &mut Record) -> Result<()> {
    let Some(base) = r.get("Document Date").as_date().or_else(|| r.get("Posting Date").as_date()) else {
        return Ok(());
    };
    let terms = r.str("Payment Terms Code").to_string();
    let (formula, disc_formula, disc) = if terms.is_empty() {
        (String::new(), String::new(), Decimal::ZERO)
    } else {
        let t = ctx.get_code(PAYMENT_TERMS, &terms)?;
        (t.str("Due Date Calculation").to_string(), t.str("Discount Date Calculation").to_string(), t.get("Discount %").as_dec())
    };
    r.set("Due Date", Value::Date(crate::dateformula::calc(&formula, base)?));
    // BC: Payment Discount % and Pmt. Discount Date come with the payment terms.
    r.set("Payment Discount %", dec(disc));
    r.set("Pmt. Discount Date", if disc.is_zero() { Value::Null } else { Value::Date(crate::dateformula::calc(&disc_formula, base)?) });
    Ok(())
}

/// Re-derives every line of the document after a header change that affects prices or VAT.
fn update_lines(ctx: &mut Ctx, side: &Side, header: &Record) -> Result<()> {
    let lt = ctx.table(side.line)?;
    let lines: Vec<Record> = ctx.db.rows(side.line).filter(|l| same_doc_header(l, header)).cloned().collect();
    for mut l in lines {
        l.set("Gen. Bus. Posting Group", header.get("Gen. Bus. Posting Group").clone());
        l.set("VAT Bus. Posting Group", header.get("VAT Bus. Posting Group").clone());
        l.set(side.party_no, header.get(side.party_no).clone());
        calc_line_amounts(ctx, side, header, &mut l)?;
        ctx.db.modify(lt, l)?;
    }
    Ok(())
}

fn validate_line(ctx: &mut Ctx, side: &Side, r: &mut Record, field: &str, old: &Value) -> Result<()> {
    let header = line_header(ctx, side, r)?;
    if r.get(field) != old {
        test_status_open(ctx, side, &header)?;
    }
    let doc_type = r.str("Document Type").to_string();
    match field {
        "Type" => {
            for f in ["No.", "Description", "Description 2", "Unit of Measure Code", "Gen. Prod. Posting Group", "VAT Prod. Posting Group"] {
                r.set(f, Value::text(""));
            }
            for f in [side.price, "Unit Cost (LCY)", "VAT %"] {
                r.set(f, dec(Decimal::ZERO));
            }
            calc_line_amounts(ctx, side, &header, r)
        }
        "No." => {
            let no = r.str("No.").to_string();
            if no.is_empty() {
                return calc_line_amounts(ctx, side, &header, r);
            }
            match r.str("Type") {
                "Item" => {
                    let item = ctx.get_code(ITEM, &no)?;
                    if item.get("Blocked").as_bool() {
                        refuse!("Blocked must be equal to 'No' in Item: No.={no}. Current value is 'Yes'.");
                    }
                    let blocked_field = if side.sales { "Sales Blocked" } else { "Purchasing Blocked" };
                    if item.get(blocked_field).as_bool() {
                        refuse!("You cannot {} Item {no} because it is {}.", if side.sales { "sell" } else { "purchase" }, blocked_field.to_lowercase());
                    }
                    r.set("Description", item.get("Description").clone());
                    r.set("Description 2", item.get("Description 2").clone());
                    // BC: the item's sales / purchase unit of measure, else its base unit.
                    let uom = match item.str(if side.sales { "Sales Unit of Measure" } else { "Purch. Unit of Measure" }) {
                        "" => item.str("Base Unit of Measure").to_string(),
                        u => u.to_string(),
                    };
                    r.set("Unit of Measure Code", Value::text(&uom));
                    r.set("Qty. per Unit of Measure", dec(qty_per_uom(ctx, &item, &uom)?));
                    r.set("Gen. Prod. Posting Group", item.get("Gen. Prod. Posting Group").clone());
                    r.set("VAT Prod. Posting Group", item.get("VAT Prod. Posting Group").clone());
                    if side.sales {
                        r.set("Unit Cost (LCY)", item.get("Unit Cost").clone());
                    }
                    let vat = vat_percent(ctx, &header, r);
                    let base_price = if side.sales {
                        item.get("Unit Price").as_dec()
                    } else {
                        match item.get("Last Direct Cost").as_dec() {
                            d if d.is_zero() => item.get("Unit Cost").as_dec(),
                            d => d,
                        }
                    };
                    let price = convert_price(base_price, item.get("Price Includes VAT").as_bool() && side.sales, header.get("Prices Including VAT").as_bool(), vat);
                    // In the document's currency (BC: the LCY price times the currency factor).
                    let f = header.get("Currency Factor").as_dec();
                    let price = if header.str("Currency Code").is_empty() || f.is_zero() { price } else { (price * f).round_dp(5) };
                    // Per the line's unit of measure.
                    let per = r.get("Qty. per Unit of Measure").as_dec();
                    let price = if per.is_zero() || per == Decimal::ONE { price } else { (price * per).round_dp(5) };
                    r.set(side.price, dec(price));
                }
                "Charge (Item)" => {
                    let c = ctx.get_code(crate::item_charge::ITEM_CHARGE, &no)?;
                    r.set("Description", c.get("Description").clone());
                    r.set("Gen. Prod. Posting Group", c.get("Gen. Prod. Posting Group").clone());
                    r.set("VAT Prod. Posting Group", c.get("VAT Prod. Posting Group").clone());
                }
                "G/L Account" => {
                    let acc = ctx.get_code(GL_ACCOUNT, &no)?;
                    if acc.str("Account Type") != "Posting" {
                        refuse!("Account Type must be equal to 'Posting' in G/L Account: No.={no}. Current value is '{}'.", acc.str("Account Type"));
                    }
                    if acc.get("Blocked").as_bool() {
                        refuse!("Blocked must be equal to 'No' in G/L Account: No.={no}. Current value is 'Yes'.");
                    }
                    r.set("Description", acc.get("Name").clone());
                    r.set("Gen. Prod. Posting Group", acc.get("Gen. Prod. Posting Group").clone());
                    r.set("VAT Prod. Posting Group", acc.get("VAT Prod. Posting Group").clone());
                }
                "Fixed Asset" if !side.sales => {
                    let fa = ctx.get_code(crate::schema::fixed_assets::FIXED_ASSET, &no)?;
                    if fa.get("Blocked").as_bool() || fa.get("Inactive").as_bool() {
                        refuse!("Fixed Asset {no} is blocked or inactive.");
                    }
                    r.set("Description", fa.get("Description").clone());
                    // Posting groups of the acquisition cost account, as BC takes them.
                    let book = crate::fa::default_book(ctx)?;
                    let group = ctx
                        .db
                        .get(crate::schema::fixed_assets::FA_DEPR_BOOK, &Key(vec![Value::text(&no), Value::text(&book)]))
                        .map(|b| b.str("FA Posting Group").to_string())
                        .unwrap_or_default();
                    if let Some(acc) = ctx
                        .db
                        .get_code(crate::schema::fixed_assets::FA_POSTING_GROUP, &group)
                        .and_then(|g| ctx.db.get_code(GL_ACCOUNT, g.str("Acquisition Cost Account")))
                    {
                        r.set("Gen. Prod. Posting Group", acc.get("Gen. Prod. Posting Group").clone());
                        r.set("VAT Prod. Posting Group", acc.get("VAT Prod. Posting Group").clone());
                    }
                }
                "" => {}
                other => refuse!("Lines of type {other} are not supported yet. Use Item, G/L Account or (on purchases) Fixed Asset."),
            }
            if r.get("Quantity").as_dec().is_zero() && r.str("Type") != "" {
                r.set("Quantity", dec(Decimal::ONE));
                set_outstanding(side, &doc_type, r);
            }
            calc_line_amounts(ctx, side, &header, r)
        }
        "Unit of Measure Code" if r.str("Type") == "Item" && !r.str("No.").is_empty() => {
            let item = ctx.get_code(ITEM, r.str("No."))?;
            let old_per = match r.get("Qty. per Unit of Measure").as_dec() {
                d if d.is_zero() => Decimal::ONE,
                d => d,
            };
            let per = qty_per_uom(ctx, &item, r.str("Unit of Measure Code"))?;
            r.set("Qty. per Unit of Measure", dec(per));
            // BC: the unit price is per unit of measure.
            let price = r.get(side.price).as_dec() / old_per * per;
            r.set(side.price, dec(price.round_dp(5)));
            calc_line_amounts(ctx, side, &header, r)
        }
        "Quantity" => {
            let qty = r.get("Quantity").as_dec();
            let invoiced = r.get("Quantity Invoiced").as_dec();
            let handled = r.get(side.qty_handled).as_dec();
            if qty.abs() < invoiced.abs() {
                refuse!("You cannot reduce Quantity below the quantity already invoiced ({invoiced}).");
            }
            if qty.abs() < handled.abs() {
                refuse!("You cannot reduce Quantity below the quantity already {} ({handled}).", if side.sales { "shipped" } else { "received" });
            }
            if qty.is_sign_negative() && !qty.is_zero() {
                refuse!("Quantity must be positive. Use a credit memo or return order for returns.");
            }
            set_outstanding(side, &doc_type, r);
            calc_line_amounts(ctx, side, &header, r)
        }
        "Line Discount %" => {
            let d = r.get("Line Discount %").as_dec();
            if d < Decimal::ZERO || d > Decimal::ONE_HUNDRED {
                refuse!("The value must be greater than or equal to 0 and less than or equal to 100. Value: {d}");
            }
            calc_line_amounts(ctx, side, &header, r)
        }
        f if f == side.price => {
            if r.get(side.price).as_dec().is_sign_negative() && !r.get(side.price).as_dec().is_zero() {
                refuse!("{} must not be negative.", side.price);
            }
            calc_line_amounts(ctx, side, &header, r)
        }
        f if f == side.qty_handle => {
            let q = r.get(side.qty_handle).as_dec();
            let outstanding = r.get("Outstanding Quantity").as_dec();
            if q > outstanding || q < Decimal::ZERO {
                refuse!("You cannot {} more than {outstanding} units.", if side.sales { "ship" } else { "receive" });
            }
            // BC InitQtyToInvoice: what is shipped and not invoiced, plus what ships now.
            let to_invoice = r.get(side.qty_handled).as_dec() + q - r.get("Quantity Invoiced").as_dec();
            r.set("Qty. to Invoice", dec(to_invoice));
            Ok(())
        }
        "Qty. to Invoice" => {
            let q = r.get("Qty. to Invoice").as_dec();
            let max = r.get("Quantity").as_dec() - r.get("Quantity Invoiced").as_dec();
            if q > max || q < Decimal::ZERO {
                refuse!("You cannot invoice more than {max} units.");
            }
            Ok(())
        }
        "VAT Prod. Posting Group" | "Gen. Prod. Posting Group" => calc_line_amounts(ctx, side, &header, r),
        _ => Ok(()),
    }
}

/// BC InitOutstanding / InitQtyToShip / InitQtyToInvoice: outstanding is what is not shipped
/// (received) yet; Qty. to Invoice covers what is shipped and not invoiced plus what ships now.
fn set_outstanding(side: &Side, doc_type: &str, r: &mut Record) {
    let handled = r.get(side.qty_handled).as_dec();
    let outstanding = r.get("Quantity").as_dec() - handled;
    r.set("Outstanding Quantity", dec(outstanding));
    if doc_type != "Quote" && doc_type != "Blanket Order" {
        r.set(side.qty_handle, dec(outstanding));
        r.set("Qty. to Invoice", dec(handled + outstanding - r.get("Quantity Invoiced").as_dec()));
    }
}

/// VAT % for a line, from VAT Posting Setup (header VAT Bus. Ã— line VAT Prod.). 0 when not set up;
/// posting checks the setup exists.
pub fn vat_percent(ctx: &Ctx, header: &Record, line: &Record) -> Decimal {
    let key = Key(vec![header.get("VAT Bus. Posting Group").clone(), line.get("VAT Prod. Posting Group").clone()]);
    ctx.db.get(VAT_POSTING_SETUP, &key).map_or(Decimal::ZERO, |s| s.get("VAT %").as_dec())
}

/// Converts a price between "including VAT" and "excluding VAT".
fn convert_price(price: Decimal, price_incl: bool, doc_incl: bool, vat: Decimal) -> Decimal {
    let factor = Decimal::ONE + vat / Decimal::ONE_HUNDRED;
    match (price_incl, doc_incl) {
        (false, true) => round_unit(price * factor),
        (true, false) if !factor.is_zero() => round_unit(price / factor),
        _ => price,
    }
}

/// Line Amount, Amount, Amount Including VAT and VAT % from quantity, price and discount.
pub fn calc_line_amounts(ctx: &Ctx, side: &Side, header: &Record, r: &mut Record) -> Result<()> {
    let vat = if r.str("No.").is_empty() { Decimal::ZERO } else { vat_percent(ctx, header, r) };
    let qty = r.get("Quantity").as_dec();
    // Quantity (Base) = Quantity × Qty. per Unit of Measure.
    let per = match r.get("Qty. per Unit of Measure").as_dec() {
        d if d.is_zero() => Decimal::ONE,
        d => d,
    };
    r.set("Quantity (Base)", dec(qty * per));
    let price = r.get(side.price).as_dec();
    let gross = round_amount(qty * price);
    let disc = round_amount(gross * r.get("Line Discount %").as_dec() / Decimal::ONE_HUNDRED);
    let line_amount = gross - disc;
    let (amount, incl) = if header.get("Prices Including VAT").as_bool() {
        let factor = Decimal::ONE + vat / Decimal::ONE_HUNDRED;
        let amount = if factor.is_zero() { line_amount } else { round_amount(line_amount / factor) };
        (amount, line_amount)
    } else {
        (line_amount, line_amount + round_amount(line_amount * vat / Decimal::ONE_HUNDRED))
    };
    r.set("VAT %", dec(vat));
    r.set("Line Discount Amount", dec(disc));
    r.set("Line Amount", dec(line_amount));
    r.set("Amount", dec(amount));
    r.set("Amount Including VAT", dec(incl));
    Ok(())
}

/// BC's SetUpNewLine for comment lines: the next line number under the same record, dated today.
fn comment_setup_new_line(ctx: &Ctx, t: &TableDef, r: &mut Record) {
    if r.get("Line No.").as_int() == 0 {
        let group: Vec<&str> = t.primary_key.iter().filter(|k| *k != "Line No.").map(String::as_str).collect();
        let last = ctx
            .db
            .rows(&t.name)
            .filter(|o| group.iter().all(|k| o.get(k).to_string() == r.get(k).to_string()))
            .map(|o| o.get("Line No.").as_int())
            .max()
            .unwrap_or(0);
        r.set("Line No.", Value::Int(last.saturating_add(10000)));
    }
    if r.get("Date").as_date().is_none() {
        r.set("Date", Value::Date(ctx.work_date));
    }
}
