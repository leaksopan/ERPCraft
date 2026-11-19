//! The base setup every company needs, and the "Auto Craft" demo company: an Indonesian
//! auto-parts and workshop business (IDR, PPN 11%). Names are fictional.

use erpcraft_meta::Record;
use serde_json::{Map, Value as Json, json};

use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::records;
use crate::schema::tables::{BANK_ACCOUNT, CUSTOMER, GL_ACCOUNT, ITEM, VENDOR};
use crate::side::{PURCHASE, SALES};

fn raw(ctx: &mut Ctx, table: &str, values: Json) -> Result<()> {
    let t = ctx.table(table)?;
    let r = Record::from_json(t, &values).map_err(EngineError::invalid)?;
    ctx.db.insert(t, r)?;
    Ok(())
}

fn create(ctx: &mut Ctx, table: &str, values: Json) -> Result<()> {
    let t = ctx.table(table)?;
    let m: Map<String, Json> = values.as_object().cloned().unwrap_or_default();
    records::create(ctx, t, &m, None)?;
    Ok(())
}

/// Number series, posting setup, chart of accounts: what BC's "base" configuration package holds.
pub fn setup(ctx: &mut Ctx) -> Result<()> {
    raw(
        ctx,
        "Company Information",
        json!({"Primary Key": "", "Name": "Auto Craft Demo", "Address": "Jl. Otomotif Raya No. 12", "City": "Jakarta Selatan",
               "Post Code": "12950", "Country/Region Code": "ID", "Phone No.": "+62 21 555 0123", "E-Mail": "info@autocraft.example",
               "VAT Registration No.": "01.234.567.8-901.000", "Bank Name": "Bank Demo Nusantara", "Bank Account No.": "123-456-7890"}),
    )?;
    let series: &[(&str, &str, &str)] = &[
        ("CUST", "Customers", "C00010"),
        ("VEND", "Vendors", "V00010"),
        ("ITEM", "Items", "I00010"),
        ("S-QUO", "Sales Quotes", "S-QUO1001"),
        ("S-ORD", "Sales Orders", "S-ORD1001"),
        ("S-INV", "Sales Invoices", "S-INV1001"),
        ("S-CR", "Sales Credit Memos", "S-CR1001"),
        ("S-RET", "Sales Return Orders", "S-RET1001"),
        ("PS-INV", "Posted Sales Invoices", "PS-INV103001"),
        ("PS-CR", "Posted Sales Credit Memos", "PS-CR104001"),
        ("P-QUO", "Purchase Quotes", "P-QUO1001"),
        ("P-ORD", "Purchase Orders", "P-ORD1001"),
        ("P-INV", "Purchase Invoices", "P-INV1001"),
        ("P-CR", "Purchase Credit Memos", "P-CR1001"),
        ("P-RET", "Purchase Return Orders", "P-RET1001"),
        ("PP-INV", "Posted Purchase Invoices", "PP-INV108001"),
        ("PP-CR", "Posted Purchase Credit Memos", "PP-CR109001"),
        ("PMT", "Payments", "PMT00001"),
    ];
    for (code, desc, start) in series {
        raw(ctx, "No. Series", json!({"Code": code, "Description": desc, "Starting No.": start, "Increment-by No.": 1, "Default Nos.": true}))?;
    }
    raw(ctx, "General Ledger Setup", json!({"Primary Key": "", "LCY Code": "IDR", "Local Currency Description": "Indonesian Rupiah", "Amount Rounding Precision": 0.01, "Payment Nos.": "PMT"}))?;
    raw(
        ctx,
        "Sales & Receivables Setup",
        json!({"Primary Key": "", "Customer Nos.": "CUST", "Quote Nos.": "S-QUO", "Order Nos.": "S-ORD", "Invoice Nos.": "S-INV",
               "Credit Memo Nos.": "S-CR", "Return Order Nos.": "S-RET", "Posted Invoice Nos.": "PS-INV", "Posted Credit Memo Nos.": "PS-CR",
               "Credit Warnings": "Both Warnings", "Stockout Warning": true}),
    )?;
    raw(
        ctx,
        "Purchases & Payables Setup",
        json!({"Primary Key": "", "Vendor Nos.": "VEND", "Quote Nos.": "P-QUO", "Order Nos.": "P-ORD", "Invoice Nos.": "P-INV",
               "Credit Memo Nos.": "P-CR", "Return Order Nos.": "P-RET", "Posted Invoice Nos.": "PP-INV", "Posted Credit Memo Nos.": "PP-CR",
               "Ext. Doc. No. Mandatory": false}),
    )?;
    raw(ctx, "Inventory Setup", json!({"Primary Key": "", "Item Nos.": "ITEM"}))?;

    // Chart of accounts.
    let coa: &[(&str, &str, &str, &str, &str, &str)] = &[
        ("10000", "BALANCE SHEET", "Heading", "Balance Sheet", "", ""),
        ("10100", "Assets", "Begin-Total", "Balance Sheet", "Assets", ""),
        ("10110", "Cash", "Posting", "Balance Sheet", "Assets", ""),
        ("10120", "Bank - Operating Account", "Posting", "Balance Sheet", "Assets", ""),
        ("10200", "Accounts Receivable", "Posting", "Balance Sheet", "Assets", ""),
        ("10300", "Inventory - Spare Parts", "Posting", "Balance Sheet", "Assets", ""),
        ("10400", "Purchase VAT (PPN Masukan)", "Posting", "Balance Sheet", "Assets", ""),
        ("10999", "Total Assets", "End-Total", "Balance Sheet", "Assets", "10100..10999"),
        ("20100", "Liabilities", "Begin-Total", "Balance Sheet", "Liabilities", ""),
        ("20200", "Accounts Payable", "Posting", "Balance Sheet", "Liabilities", ""),
        ("20300", "Sales VAT (PPN Keluaran)", "Posting", "Balance Sheet", "Liabilities", ""),
        ("20999", "Total Liabilities", "End-Total", "Balance Sheet", "Liabilities", "20100..20999"),
        ("30100", "Equity", "Begin-Total", "Balance Sheet", "Equity", ""),
        ("30200", "Owner's Capital", "Posting", "Balance Sheet", "Equity", ""),
        ("30300", "Retained Earnings", "Posting", "Balance Sheet", "Equity", ""),
        ("30999", "Total Equity", "End-Total", "Balance Sheet", "Equity", "30100..30999"),
        ("40000", "INCOME STATEMENT", "Heading", "Income Statement", "", ""),
        ("40100", "Revenue", "Begin-Total", "Income Statement", "Income", ""),
        ("40200", "Sales - Spare Parts", "Posting", "Income Statement", "Income", ""),
        ("40300", "Sales - Workshop Services", "Posting", "Income Statement", "Income", ""),
        ("40400", "Sales Returns & Allowances", "Posting", "Income Statement", "Income", ""),
        ("40999", "Total Revenue", "End-Total", "Income Statement", "Income", "40100..40999"),
        ("50100", "Cost of Goods Sold", "Begin-Total", "Income Statement", "Cost of Goods Sold", ""),
        ("50200", "COGS - Spare Parts", "Posting", "Income Statement", "Cost of Goods Sold", ""),
        ("50300", "Purchases - Services & Supplies", "Posting", "Income Statement", "Cost of Goods Sold", ""),
        ("50999", "Total Cost of Goods Sold", "End-Total", "Income Statement", "Cost of Goods Sold", "50100..50999"),
        ("60100", "Operating Expenses", "Begin-Total", "Income Statement", "Expense", ""),
        ("60200", "Rent", "Posting", "Income Statement", "Expense", ""),
        ("60300", "Utilities", "Posting", "Income Statement", "Expense", ""),
        ("60400", "Salaries & Wages", "Posting", "Income Statement", "Expense", ""),
        ("60999", "Total Operating Expenses", "End-Total", "Income Statement", "Expense", "60100..60999"),
        ("99999", "NET INCOME", "Total", "Income Statement", "", "40000..69999"),
    ];
    let mut pending: Vec<(String, &str, &str)> = Vec::new();
    for (no, name, ty, ib, cat, totaling) in coa {
        let indent = match *ty {
            "Heading" => 0,
            "Begin-Total" | "End-Total" => 1,
            "Total" => 0,
            _ => 2,
        };
        raw(
            ctx,
            GL_ACCOUNT,
            json!({"No.": no, "Name": name, "Account Type": ty, "Income/Balance": ib, "Account Category": cat, "Totaling": totaling,
                   "Indentation": indent, "Direct Posting": *ty == "Posting"}),
        )?;
        // Revenue and expense accounts carry posting groups, so G/L account lines on documents get
        // VAT, as BC's demo accounts do.
        if *ty == "Posting" && *ib == "Income Statement" {
            let gen_prod = if no.starts_with("402") || no.starts_with("502") { "PARTS" } else { "SERVICES" };
            let gen_type = if no.starts_with('4') { "Sale" } else { "Purchase" };
            pending.push((no.to_string(), gen_prod, gen_type));
        }
    }
    for (code, desc) in [("DOMESTIC", "Domestic customers and vendors"), ("EXPORT", "Foreign customers and vendors")] {
        raw(ctx, "Gen. Business Posting Group", json!({"Code": code, "Description": desc}))?;
        raw(ctx, "VAT Business Posting Group", json!({"Code": code, "Description": desc}))?;
    }
    for (code, desc) in [("PARTS", "Spare parts"), ("SERVICES", "Workshop services")] {
        raw(ctx, "Gen. Product Posting Group", json!({"Code": code, "Description": desc}))?;
    }
    for (code, desc) in [("STANDARD", "PPN 11%"), ("ZERO", "No VAT")] {
        raw(ctx, "VAT Product Posting Group", json!({"Code": code, "Description": desc}))?;
    }
    for bus in ["DOMESTIC", "EXPORT"] {
        raw(ctx, "General Posting Setup", json!({"Gen. Bus. Posting Group": bus, "Gen. Prod. Posting Group": "PARTS",
            "Sales Account": "40200", "Sales Credit Memo Account": "40400", "Purch. Account": "50300", "Purch. Credit Memo Account": "50300", "COGS Account": "50200"}))?;
        raw(ctx, "General Posting Setup", json!({"Gen. Bus. Posting Group": bus, "Gen. Prod. Posting Group": "SERVICES",
            "Sales Account": "40300", "Sales Credit Memo Account": "40400", "Purch. Account": "50300", "Purch. Credit Memo Account": "50300", "COGS Account": "50200"}))?;
        for prod in ["STANDARD", "ZERO"] {
            let pct = if bus == "DOMESTIC" && prod == "STANDARD" { 11 } else { 0 };
            raw(ctx, "VAT Posting Setup", json!({"VAT Bus. Posting Group": bus, "VAT Prod. Posting Group": prod, "VAT Calculation Type": "Normal VAT",
                "VAT %": pct, "Sales VAT Account": "20300", "Purchase VAT Account": "10400", "Description": format!("{bus} / {prod}")}))?;
        }
    }
    let gt = ctx.table(GL_ACCOUNT)?;
    for (no, gen_prod, gen_type) in pending {
        let mut acc = ctx.get_code(GL_ACCOUNT, &no)?;
        acc.set("Gen. Posting Type", erpcraft_meta::Value::text(gen_type));
        acc.set("Gen. Prod. Posting Group", erpcraft_meta::Value::text(gen_prod));
        acc.set("VAT Prod. Posting Group", erpcraft_meta::Value::text("STANDARD"));
        ctx.db.modify(gt, acc)?;
    }
    raw(ctx, "Inventory Posting Group", json!({"Code": "PARTS", "Description": "Spare parts inventory"}))?;
    raw(ctx, "Inventory Posting Setup", json!({"Location Code": "", "Invt. Posting Group Code": "PARTS", "Inventory Account": "10300", "Description": "Spare parts"}))?;
    raw(ctx, "Customer Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic customers", "Receivables Account": "10200"}))?;
    raw(ctx, "Vendor Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic vendors", "Payables Account": "20200"}))?;
    raw(ctx, "Bank Account Posting Group", json!({"Code": "OPERATING", "Description": "Operating accounts", "G/L Account No.": "10120"}))?;
    for (code, formula, desc) in [("COD", "0D", "Cash on delivery"), ("14 DAYS", "14D", "Net 14 days"), ("30 DAYS", "30D", "Net 30 days"), ("CM", "CM", "End of current month")] {
        raw(ctx, "Payment Terms", json!({"Code": code, "Due Date Calculation": formula, "Description": desc}))?;
    }
    for (code, desc, sym) in [("USD", "US Dollar", "$"), ("EUR", "Euro", "€"), ("SGD", "Singapore Dollar", "S$")] {
        raw(ctx, "Currency", json!({"Code": code, "Description": desc, "Symbol": sym}))?;
    }
    for (code, desc) in [("PCS", "Piece"), ("SET", "Set"), ("LTR", "Liter"), ("HOUR", "Hour")] {
        raw(ctx, "Unit of Measure", json!({"Code": code, "Description": desc}))?;
    }
    for (code, desc) in [("OIL", "Oils & Lubricants"), ("BRAKE", "Brake System"), ("TIRE", "Tires & Wheels"), ("ELECTRIC", "Electrical"), ("SERVICE", "Workshop Services")] {
        raw(ctx, "Item Category", json!({"Code": code, "Description": desc}))?;
    }
    for (code, name) in [("AR", "Ari Ramadhan"), ("DS", "Dina Saputri"), ("PB", "Purchasing - Budi")] {
        raw(ctx, "Salesperson/Purchaser", json!({"Code": code, "Name": name}))?;
    }
    Ok(())
}

pub fn master_data(ctx: &mut Ctx) -> Result<()> {
    let customers: &[(&str, &str, &str, &str, &str, i64)] = &[
        ("10000", "Bengkel Maju Jaya", "Jl. Raya Bogor Km 21", "Jakarta Timur", "30 DAYS", 50_000_000),
        ("20000", "Garasi Sentosa Motor", "Jl. Gatot Subroto 88", "Bandung", "14 DAYS", 25_000_000),
        ("30000", "Prima Auto Service", "Jl. Ahmad Yani 5", "Surabaya", "30 DAYS", 75_000_000),
        ("40000", "Fleet Logistik Nusantara", "Kawasan Industri Pulogadung Blok C", "Jakarta Timur", "CM", 150_000_000),
        ("50000", "Mitra Karya Otomotif", "Jl. Malioboro 120", "Yogyakarta", "COD", 0),
    ];
    for (no, name, addr, city, terms, limit) in customers {
        create(
            ctx,
            CUSTOMER,
            json!({"No.": no, "Name": name, "Address": addr, "City": city, "Country/Region Code": "ID", "Phone No.": "+62 21 7000 0000",
                   "Contact": "Purchasing Dept.", "Salesperson Code": if no.starts_with('1') || no.starts_with('3') { "AR" } else { "DS" },
                   "Credit Limit (LCY)": limit, "Gen. Bus. Posting Group": "DOMESTIC", "VAT Bus. Posting Group": "DOMESTIC",
                   "Customer Posting Group": "DOMESTIC", "Payment Terms Code": terms}),
        )?;
    }
    let vendors: &[(&str, &str, &str, &str)] = &[
        ("10000", "PT Sumber Oli Utama", "Cikarang", "30 DAYS"),
        ("20000", "CV Rem Andalan", "Tangerang", "30 DAYS"),
        ("30000", "Global Tire Supply", "Jakarta Utara", "14 DAYS"),
        ("40000", "Kelistrikan Mobil Abadi", "Semarang", "30 DAYS"),
    ];
    for (no, name, city, terms) in vendors {
        create(
            ctx,
            VENDOR,
            json!({"No.": no, "Name": name, "City": city, "Country/Region Code": "ID", "Purchaser Code": "PB",
                   "Gen. Bus. Posting Group": "DOMESTIC", "VAT Bus. Posting Group": "DOMESTIC", "Vendor Posting Group": "DOMESTIC",
                   "Payment Terms Code": terms}),
        )?;
    }
    let items: &[(&str, &str, &str, &str, &str, i64, i64, &str)] = &[
        ("1000", "Engine Oil 5W-30 (4L)", "Inventory", "PCS", "OIL", 450_000, 330_000, "10000"),
        ("1001", "Oil Filter", "Inventory", "PCS", "OIL", 85_000, 52_000, "10000"),
        ("1100", "Brake Pad Set - Front", "Inventory", "SET", "BRAKE", 650_000, 410_000, "20000"),
        ("1101", "Brake Disc Rotor", "Inventory", "PCS", "BRAKE", 1_250_000, 820_000, "20000"),
        ("1200", "Tire 195/65 R15", "Inventory", "PCS", "TIRE", 1_100_000, 780_000, "30000"),
        ("1300", "Spark Plug Iridium", "Inventory", "PCS", "ELECTRIC", 145_000, 90_000, "40000"),
        ("1301", "Car Battery 12V 45Ah", "Inventory", "PCS", "ELECTRIC", 1_350_000, 950_000, "40000"),
        ("9000", "Periodic Service Labor", "Service", "HOUR", "SERVICE", 250_000, 0, ""),
    ];
    for (no, desc, ty, uom, cat, price, cost, vendor) in items {
        create(
            ctx,
            ITEM,
            json!({"No.": no, "Description": desc, "Type": ty, "Base Unit of Measure": uom, "Item Category Code": cat,
                   "Unit Price": price, "Unit Cost": cost, "Costing Method": "Average",
                   "Gen. Prod. Posting Group": if *ty == "Service" { "SERVICES" } else { "PARTS" },
                   "VAT Prod. Posting Group": "STANDARD", "Inventory Posting Group": if *ty == "Service" { "" } else { "PARTS" },
                   "Vendor No.": vendor, "Reorder Point": if *ty == "Service" { 0 } else { 10 }}),
        )?;
    }
    create(
        ctx,
        BANK_ACCOUNT,
        json!({"No.": "OPERATING", "Name": "Operating Account - Bank Demo", "Bank Account No.": "123-456-7890", "City": "Jakarta",
               "Bank Acc. Posting Group": "OPERATING"}),
    )?;
    Ok(())
}

/// A few documents so every list and cue has something to show: stock received, an invoice
/// posted and partly paid, open orders and a quote.
pub fn transactions(ctx: &mut Ctx) -> Result<()> {
    let po = crate::documents::new_document(
        ctx,
        &PURCHASE,
        "Order",
        "10000",
        &obj(json!({"Vendor Invoice No.": "SOU-INV-2201"})),
        &[json!({"no": "1000", "quantity": 40}), json!({"no": "1001", "quantity": 60})],
    )?;
    post(ctx, false, &po)?;
    let po = crate::documents::new_document(
        ctx,
        &PURCHASE,
        "Order",
        "20000",
        &obj(json!({"Vendor Invoice No.": "RA-7781"})),
        &[json!({"no": "1100", "quantity": 25}), json!({"no": "1101", "quantity": 12})],
    )?;
    post(ctx, false, &po)?;
    let po = crate::documents::new_document(
        ctx,
        &PURCHASE,
        "Order",
        "30000",
        &obj(json!({"Vendor Invoice No.": "GTS-0091"})),
        &[json!({"no": "1200", "quantity": 32})],
    )?;
    post(ctx, false, &po)?;

    let so = crate::documents::new_document(
        ctx,
        &SALES,
        "Order",
        "10000",
        &obj(json!({"External Document No.": "PO-BMJ-118"})),
        &[json!({"no": "1000", "quantity": 6}), json!({"no": "1001", "quantity": 6}), json!({"no": "9000", "quantity": 4})],
    )?;
    let posted = post(ctx, true, &so)?;
    crate::posting::payment::register(
        ctx,
        &SALES,
        &crate::posting::payment::Payment {
            party_no: "10000",
            amount: rust_decimal::Decimal::new(2_000_000, 0),
            bank_account: "OPERATING",
            posting_date: ctx.work_date,
            applies_to_doc_no: Some(&posted),
            description: None,
        },
    )?;
    crate::documents::new_document(
        ctx,
        &SALES,
        "Order",
        "30000",
        &Map::new(),
        &[json!({"no": "1100", "quantity": 4}), json!({"no": "1101", "quantity": 2, "line_discount": 5})],
    )?;
    crate::documents::new_document(ctx, &SALES, "Order", "40000", &Map::new(), &[json!({"no": "1200", "quantity": 16}), json!({"no": "9000", "quantity": 8})])?;
    crate::documents::new_document(ctx, &SALES, "Quote", "20000", &Map::new(), &[json!({"no": "1301", "quantity": 3}), json!({"no": "1300", "quantity": 12})])?;
    crate::documents::new_document(ctx, &SALES, "Invoice", "50000", &Map::new(), &[json!({"no": "1000", "quantity": 2})])?;
    crate::documents::new_document(ctx, &PURCHASE, "Order", "40000", &Map::new(), &[json!({"no": "1301", "quantity": 10}), json!({"no": "1300", "quantity": 50})])?;
    Ok(())
}

fn obj(j: Json) -> Map<String, Json> {
    j.as_object().cloned().unwrap_or_default()
}

/// Posts a document created by `new_document`; returns the posted number.
fn post(ctx: &mut Ctx, sales: bool, created: &Json) -> Result<String> {
    let no = created.get("no").and_then(Json::as_str).unwrap_or_default().to_string();
    let side = if sales { &SALES } else { &PURCHASE };
    let r = crate::posting::post_document(ctx, side, "Order", &no)?;
    Ok(r.get("posted_no").and_then(Json::as_str).unwrap_or_default().to_string())
}
