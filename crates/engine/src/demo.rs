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

/// Permission sets and the user who creates the company, as SUPER (BC gives the first user
/// SUPER, so the company is never left without an administrator).
fn security(ctx: &mut Ctx) -> Result<()> {
    raw(ctx, "Permission Set", json!({"Role ID": "SUPER", "Name": "All permissions"}))?;
    raw(ctx, "Permission Set", json!({"Role ID": "VIEWER", "Name": "Read data and run reports"}))?;
    raw(
        ctx,
        "Permission",
        json!({"Role ID": "SUPER", "Object Type": "Table Data", "Object Name": "*", "Read Permission": true, "Insert Permission": true,
               "Modify Permission": true, "Delete Permission": true}),
    )?;
    raw(ctx, "Permission", json!({"Role ID": "SUPER", "Object Type": "Command", "Object Name": "*", "Execute Permission": true}))?;
    raw(ctx, "Permission", json!({"Role ID": "VIEWER", "Object Type": "Table Data", "Object Name": "*", "Read Permission": true}))?;
    for cmd in [
        "record.list",
        "record.get",
        "record.lookup",
        "customer.statistics",
        "vendor.statistics",
        "item.availability",
        "report.trial_balance",
        "sales.document.statistics",
        "purchase.document.statistics",
    ] {
        raw(ctx, "Permission", json!({"Role ID": "VIEWER", "Object Type": "Command", "Object Name": cmd, "Execute Permission": true}))?;
    }
    let user = ctx.user.to_uppercase();
    raw(ctx, "User", json!({"User Name": user, "Full Name": ctx.user, "State": "Enabled"}))?;
    raw(ctx, "Access Control", json!({"User Name": user, "Role ID": "SUPER"}))?;
    Ok(())
}

/// Number series, posting setup, chart of accounts: what BC's "base" configuration package holds.
pub fn setup(ctx: &mut Ctx) -> Result<()> {
    security(ctx)?;
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
        ("PS-SHPT", "Posted Sales Shipments", "102001"),
        ("PP-RCPT", "Posted Purchase Receipts", "107001"),
        ("G-JNL", "General Journal", "G00001"),
        ("G-RCPT", "Cash Receipts Journal", "GR00001"),
        ("G-PMT", "Payment Journal", "GP00001"),
    ];
    for (code, desc, start) in series {
        raw(ctx, "No. Series", json!({"Code": code, "Description": desc, "Starting No.": start, "Increment-by No.": 1, "Default Nos.": true}))?;
    }
    raw(ctx, "General Ledger Setup", json!({"Primary Key": "", "LCY Code": "IDR", "Local Currency Symbol": "Rp", "Local Currency Description": "Indonesian Rupiah", "Amount Rounding Precision": 0.01, "Payment Nos.": "PMT"}))?;
    raw(
        ctx,
        "Sales & Receivables Setup",
        json!({"Primary Key": "", "Customer Nos.": "CUST", "Quote Nos.": "S-QUO", "Order Nos.": "S-ORD", "Invoice Nos.": "S-INV",
               "Credit Memo Nos.": "S-CR", "Return Order Nos.": "S-RET", "Posted Invoice Nos.": "PS-INV", "Posted Credit Memo Nos.": "PS-CR", "Posted Shipment Nos.": "PS-SHPT",
               "Credit Warnings": "Both Warnings", "Stockout Warning": true}),
    )?;
    raw(
        ctx,
        "Purchases & Payables Setup",
        json!({"Primary Key": "", "Vendor Nos.": "VEND", "Quote Nos.": "P-QUO", "Order Nos.": "P-ORD", "Invoice Nos.": "P-INV",
               "Credit Memo Nos.": "P-CR", "Return Order Nos.": "P-RET", "Posted Invoice Nos.": "PP-INV", "Posted Credit Memo Nos.": "PP-CR", "Posted Receipt Nos.": "PP-RCPT",
               "Ext. Doc. No. Mandatory": false}),
    )?;
    raw(ctx, "Inventory Setup", json!({"Primary Key": "", "Item Nos.": "ITEM"}))?;
    for (template, ty, _, _, caption, series) in crate::schema::finance::JOURNALS {
        let source = match *template {
            "CASHRCPT" => "CASHRECJNL",
            "PAYMENT" => "PAYMENTJNL",
            _ => "GENJNL",
        };
        raw(ctx, "Gen. Journal Template", json!({"Name": template, "Description": caption, "Type": ty, "No. Series": series, "Source Code": source}))?;
        raw(ctx, "Gen. Journal Batch", json!({"Journal Template Name": template, "Name": "DEFAULT", "Description": "Default journal", "No. Series": series}))?;
    }

    // Chart of accounts.
    let coa: &[(&str, &str, &str, &str, &str, &str)] = &[
        ("10000", "BALANCE SHEET", "Heading", "Balance Sheet", "", ""),
        ("10100", "Assets", "Begin-Total", "Balance Sheet", "Assets", ""),
        ("10110", "Cash", "Posting", "Balance Sheet", "Assets", ""),
        ("10120", "Bank - Operating Account", "Posting", "Balance Sheet", "Assets", ""),
        ("10200", "Accounts Receivable", "Posting", "Balance Sheet", "Assets", ""),
        ("10300", "Inventory - Spare Parts", "Posting", "Balance Sheet", "Assets", ""),
        ("10400", "Purchase VAT (PPN Masukan)", "Posting", "Balance Sheet", "Assets", ""),
        ("10500", "Fixed Assets - Vehicles & Equipment", "Posting", "Balance Sheet", "Assets", ""),
        ("10510", "Accumulated Depreciation", "Posting", "Balance Sheet", "Assets", ""),
        ("10999", "Total Assets", "End-Total", "Balance Sheet", "Assets", "10100..10999"),
        ("20100", "Liabilities", "Begin-Total", "Balance Sheet", "Liabilities", ""),
        ("20200", "Accounts Payable", "Posting", "Balance Sheet", "Liabilities", ""),
        ("20300", "Sales VAT (PPN Keluaran)", "Posting", "Balance Sheet", "Liabilities", ""),
        ("20400", "Deferred Grant Income (Hibah)", "Posting", "Balance Sheet", "Liabilities", ""),
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
        ("40500", "Gain on Disposal of Assets", "Posting", "Income Statement", "Income", ""),
        ("40999", "Total Revenue", "End-Total", "Income Statement", "Income", "40100..40999"),
        ("50100", "Cost of Goods Sold", "Begin-Total", "Income Statement", "Cost of Goods Sold", ""),
        ("50200", "COGS - Spare Parts", "Posting", "Income Statement", "Cost of Goods Sold", ""),
        ("50300", "Purchases - Services & Supplies", "Posting", "Income Statement", "Cost of Goods Sold", ""),
        ("50400", "Inventory Adjustments", "Posting", "Income Statement", "Cost of Goods Sold", ""),
        ("50999", "Total Cost of Goods Sold", "End-Total", "Income Statement", "Cost of Goods Sold", "50100..50999"),
        ("60100", "Operating Expenses", "Begin-Total", "Income Statement", "Expense", ""),
        ("60200", "Rent", "Posting", "Income Statement", "Expense", ""),
        ("60300", "Utilities", "Posting", "Income Statement", "Expense", ""),
        ("60400", "Salaries & Wages", "Posting", "Income Statement", "Expense", ""),
        ("60500", "Depreciation Expense", "Posting", "Income Statement", "Expense", ""),
        ("60600", "Loss on Disposal of Assets", "Posting", "Income Statement", "Expense", ""),
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
    for prod in ["PARTS", "SERVICES"] {
        // Item journals post with a blank business group (BC: Gen. Bus. Posting Group '' on the line).
        raw(ctx, "General Posting Setup", json!({"Gen. Bus. Posting Group": "", "Gen. Prod. Posting Group": prod, "COGS Account": "50200", "Inventory Adjmt. Account": "50400"}))?;
    }
    for bus in ["DOMESTIC", "EXPORT"] {
        raw(ctx, "General Posting Setup", json!({"Gen. Bus. Posting Group": bus, "Gen. Prod. Posting Group": "PARTS",
            "Sales Account": "40200", "Sales Credit Memo Account": "40400", "Purch. Account": "50300", "Purch. Credit Memo Account": "50300", "COGS Account": "50200", "Inventory Adjmt. Account": "50400"}))?;
        raw(ctx, "General Posting Setup", json!({"Gen. Bus. Posting Group": bus, "Gen. Prod. Posting Group": "SERVICES",
            "Sales Account": "40300", "Sales Credit Memo Account": "40400", "Purch. Account": "50300", "Purch. Credit Memo Account": "50300", "COGS Account": "50200", "Inventory Adjmt. Account": "50400"}))?;
        for prod in ["STANDARD", "ZERO"] {
            let pct = if bus == "DOMESTIC" && prod == "STANDARD" { 11 } else { 0 };
            raw(ctx, "VAT Posting Setup", json!({"VAT Bus. Posting Group": bus, "VAT Prod. Posting Group": prod, "VAT Calculation Type": "Normal VAT",
                "VAT %": pct, "Sales VAT Account": "20300", "Purchase VAT Account": "10400", "Description": format!("{bus} / {prod}")}))?;
        }
    }
    pending.push(("10500".to_string(), "SERVICES", "Purchase"));
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
    for (code, name, city) in [("MAIN", "Gudang Pusat Jakarta", "Jakarta Timur"), ("BANDUNG", "Gudang Bandung", "Bandung")] {
        raw(ctx, "Location", json!({"Code": code, "Name": name, "City": city}))?;
        raw(ctx, "Inventory Posting Setup", json!({"Location Code": code, "Invt. Posting Group Code": "PARTS", "Inventory Account": "10300", "Description": format!("Spare parts at {name}")}))?;
    }
    for (code, desc, start) in [("I-JNL", "Item Journal", "IJ00001"), ("I-RCLSS", "Item Reclass. Journal", "IR00001"), ("I-PHYS", "Phys. Inventory Journal", "IP00001"), ("FA-JNL", "FA G/L Journal", "FAJ00001")] {
        raw(ctx, "No. Series", json!({"Code": code, "Description": desc, "Starting No.": start, "Increment-by No.": 1}))?;
    }
    for (template, ty, _, _, caption, series, source) in crate::schema::inventory::ITEM_JOURNALS {
        raw(ctx, "Item Journal Template", json!({"Name": template, "Description": caption, "Type": ty, "No. Series": series, "Source Code": source}))?;
        raw(ctx, "Item Journal Batch", json!({"Journal Template Name": template, "Name": "DEFAULT", "Description": "Default journal", "No. Series": series}))?;
    }
    raw(ctx, "Gen. Journal Template", json!({"Name": "ASSETS", "Description": "Fixed Asset G/L Journal", "Type": "Assets", "No. Series": "FA-JNL", "Source Code": "FAGLJNL"}))?;
    raw(ctx, "Gen. Journal Batch", json!({"Journal Template Name": "ASSETS", "Name": "DEFAULT", "Description": "Default journal", "No. Series": "FA-JNL"}))?;
    fixed_asset_setup(ctx)?;
    raw(ctx, "Customer Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic customers", "Receivables Account": "10200"}))?;
    raw(ctx, "Vendor Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic vendors", "Payables Account": "20200"}))?;
    raw(ctx, "Bank Account Posting Group", json!({"Code": "OPERATING", "Description": "Operating accounts", "G/L Account No.": "10120"}))?;
    for (code, formula, desc) in [("COD", "0D", "Cash on delivery"), ("14 DAYS", "14D", "Net 14 days"), ("30 DAYS", "30D", "Net 30 days"), ("CM", "CM", "End of current month")] {
        raw(ctx, "Payment Terms", json!({"Code": code, "Due Date Calculation": formula, "Description": desc}))?;
    }
    for (code, desc, sym) in [("USD", "US Dollar", "$"), ("EUR", "Euro", "â‚¬"), ("SGD", "Singapore Dollar", "S$")] {
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

/// Depreciation book, posting groups, classes and locations for fixed assets.
fn fixed_asset_setup(ctx: &mut Ctx) -> Result<()> {
    raw(ctx, "Depreciation Book", json!({"Code": "COMPANY", "Description": "Company book (commercial)", "G/L Integration - Acq. Cost": true,
        "G/L Integration - Depreciation": true, "G/L Integration - Disposal": true}))?;
    raw(ctx, "No. Series", json!({"Code": "FA", "Description": "Fixed Assets", "Starting No.": "FA000010", "Increment-by No.": 10}))?;
    raw(ctx, "FA Setup", json!({"Primary Key": "", "Default Depr. Book": "COMPANY", "Fixed Asset Nos.": "FA"}))?;
    for code in ["VEHICLES", "EQUIPMENT"] {
        raw(ctx, "FA Posting Group", json!({"Code": code, "Acquisition Cost Account": "10500", "Accum. Depreciation Account": "10510",
            "Depreciation Expense Acc.": "60500", "Gains Acc. on Disposal": "40500", "Losses Acc. on Disposal": "60600"}))?;
    }
    raw(ctx, "FA Class", json!({"Code": "TANGIBLE", "Name": "Tangible Assets"}))?;
    raw(ctx, "FA Subclass", json!({"Code": "VEHICLE", "Name": "Vehicles", "FA Class Code": "TANGIBLE", "Default FA Posting Group": "VEHICLES"}))?;
    raw(ctx, "FA Subclass", json!({"Code": "EQUIPMENT", "Name": "Workshop Equipment", "FA Class Code": "TANGIBLE", "Default FA Posting Group": "EQUIPMENT"}))?;
    for (code, name) in [("JKT-WS", "Workshop Jakarta"), ("BDG-WS", "Workshop Bandung")] {
        raw(ctx, "FA Location", json!({"Code": code, "Name": name}))?;
    }
    Ok(())
}

pub fn master_data(ctx: &mut Ctx) -> Result<()> {
    let mut pending_batches: Vec<erpcraft_meta::Record> = Vec::new();
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
                   "Customer Posting Group": "DOMESTIC", "Payment Terms Code": terms,
                   "Location Code": if *city == "Bandung" { "BANDUNG" } else { "MAIN" }}),
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
                   "Payment Terms Code": terms, "Location Code": "MAIN"}),
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
    for template in ["CASHRCPT", "PAYMENT"] {
        let t = ctx.table("Gen. Journal Batch")?;
        let key = erpcraft_meta::Key(vec![erpcraft_meta::Value::text(template), erpcraft_meta::Value::text("DEFAULT")]);
        if let Some(mut b) = ctx.db.get(&t.name, &key).cloned() {
            b.set("Bal. Account Type", erpcraft_meta::Value::text("Bank Account"));
            b.set("Bal. Account No.", erpcraft_meta::Value::text("OPERATING"));
            // The bank account is created just below; the batch only names it.
            pending_batches.push(b);
        }
    }
    create(
        ctx,
        BANK_ACCOUNT,
        json!({"No.": "OPERATING", "Name": "Operating Account - Bank Demo", "Bank Account No.": "123-456-7890", "City": "Jakarta",
               "Bank Acc. Posting Group": "OPERATING"}),
    )?;
    let bt = ctx.table("Gen. Journal Batch")?;
    for b in pending_batches {
        ctx.db.modify(bt, b)?;
    }
    let assets: &[(&str, &str, &str, &str, &str, i64)] = &[
        ("Tow Truck Isuzu Elf", "VEHICLE", "JKT-WS", "B 9123 KXA", "", 8),
        ("Hydraulic Car Lift 4T", "EQUIPMENT", "JKT-WS", "HL4-22918", "", 10),
        ("Diagnostic Scanner Pro", "EQUIPMENT", "BDG-WS", "DSP-00731", "", 4),
        ("Air Compressor 100L", "EQUIPMENT", "BDG-WS", "AC100-5521", "40000", 5),
    ];
    let ft = ctx.table("Fixed Asset")?;
    let bk = ctx.table("FA Depreciation Book")?;
    for (desc, sub, loc, serial, vendor, years) in assets {
        let r = crate::records::create(
            ctx,
            ft,
            &obj(json!({"Description": desc, "FA Subclass Code": sub, "FA Location Code": loc, "Serial No.": serial, "Vendor No.": vendor})),
            None,
        )?;
        let key = erpcraft_meta::Key(vec![r.get("No.").clone(), erpcraft_meta::Value::text("COMPANY")]);
        crate::records::update(ctx, bk, &key, &obj(json!({"Depreciation Method": "Straight-Line", "No. of Depreciation Years": years})))?;
    }
    demo_dimensions(ctx)
}

/// Department and area as the global dimensions, project as shortcut dimension 3, and default
/// dimensions on customers, vendors and items, set the way a user would: through the card's
/// Global Dimension fields, which BC turns into default dimensions.
/// BC's approval workflow templates, disabled until someone enables one (as in a new company).
fn demo_workflows(ctx: &mut Ctx) -> Result<()> {
    for (_, _, code, description) in crate::schema::approvals::WORKFLOWS {
        raw(ctx, "Workflow", json!({"Code": code, "Description": description, "Enabled": false, "Category": "APPROVAL"}))?;
    }
    for (code, description, category) in crate::schema::approvals::JOURNAL_WORKFLOWS {
        raw(ctx, "Workflow", json!({"Code": code, "Description": description, "Enabled": false, "Category": category}))?;
    }
    Ok(())
}

fn demo_dimensions(ctx: &mut Ctx) -> Result<()> {
    demo_workflows(ctx)?;
    let dims: &[(&str, &str, &str, &[(&str, &str)])] = &[
        ("DEPARTMENT", "Department", "Department Code", &[("ADM", "Administration"), ("SALES", "Sales"), ("SERVICE", "Workshop Service")]),
        ("AREA", "Area", "Area Code", &[("JKT", "Jakarta"), ("BDG", "Bandung"), ("SBY", "Surabaya"), ("YOG", "Yogyakarta")]),
        ("PROJECT", "Project", "Project Code", &[("FLEET", "Fleet Contracts"), ("RETAIL", "Retail Workshops")]),
    ];
    for (n, (code, name, caption, values)) in dims.iter().enumerate() {
        raw(ctx, "Dimension", json!({"Code": code, "Name": name, "Code Caption": caption, "Filter Caption": format!("{name} Filter"), "Description": name}))?;
        for (v, vname) in *values {
            raw(ctx, "Dimension Value", json!({"Dimension Code": code, "Code": v, "Name": vname, "Global Dimension No.": n + 1}))?;
        }
    }
    let gl = ctx.table("General Ledger Setup")?;
    let key = erpcraft_meta::Key(vec![erpcraft_meta::Value::text("")]);
    if let Some(mut setup) = ctx.db.get("General Ledger Setup", &key).cloned() {
        for (f, v) in [
            ("Global Dimension 1 Code", "DEPARTMENT"),
            ("Global Dimension 2 Code", "AREA"),
            ("Shortcut Dimension 1 Code", "DEPARTMENT"),
            ("Shortcut Dimension 2 Code", "AREA"),
            ("Shortcut Dimension 3 Code", "PROJECT"),
        ] {
            setup.set(f, erpcraft_meta::Value::text(v));
        }
        ctx.db.modify(gl, setup)?;
    }
    let ct = ctx.table(CUSTOMER)?;
    for (no, area) in [("10000", "JKT"), ("20000", "BDG"), ("30000", "SBY"), ("40000", "JKT"), ("50000", "YOG")] {
        let key = erpcraft_meta::Key(vec![erpcraft_meta::Value::text(no)]);
        crate::records::update(ctx, ct, &key, &obj(json!({"Global Dimension 1 Code": "SALES", "Global Dimension 2 Code": area})))?;
    }
    let vt = ctx.table("Vendor")?;
    let vendor_nos: Vec<String> = ctx.db.rows("Vendor").map(|v| v.str("No.").to_string()).collect();
    for no in vendor_nos {
        let key = erpcraft_meta::Key(vec![erpcraft_meta::Value::text(&no)]);
        crate::records::update(ctx, vt, &key, &obj(json!({"Global Dimension 1 Code": "ADM"})))?;
    }
    let it = ctx.table("Item")?;
    let items: Vec<(String, String)> = ctx.db.rows("Item").map(|i| (i.str("No.").to_string(), i.str("Type").to_string())).collect();
    for (no, ty) in items {
        let key = erpcraft_meta::Key(vec![erpcraft_meta::Value::text(&no)]);
        let dept = if ty == "Service" { "SERVICE" } else { "SALES" };
        crate::records::update(ctx, it, &key, &obj(json!({"Global Dimension 1 Code": dept})))?;
    }
    Ok(())
}

/// A few documents so every list and cue has something to show: stock received, an invoice
/// posted and partly paid, open orders and a quote.
/// A general-journal-style line in `template` / DEFAULT, validated like the UI does.
fn jnl(ctx: &mut Ctx, template: &str, values: Json) -> Result<()> {
    let t = ctx.table("Gen. Journal Line")?;
    let mut v = obj(values);
    v.insert("Journal Template Name".into(), json!(template));
    v.insert("Journal Batch Name".into(), json!("DEFAULT"));
    crate::records::create(ctx, t, &v, None)?;
    Ok(())
}

/// Opening balance and fixed assets, about a year back: bought (bank), received as a grant
/// (hibah) and taken over as an opening balance, then depreciated up to last month.
fn opening_and_assets(ctx: &mut Ctx) -> Result<()> {
    let ago = |days: u64| ctx.work_date.checked_sub_days(chrono::Days::new(days)).map(|d| d.to_string()).unwrap_or_default();
    let (d400, d395, d200) = (ago(400), ago(395), ago(200));
    jnl(ctx, "GENERAL", json!({"Posting Date": d400, "Account Type": "Bank Account", "Account No.": "OPERATING", "Description": "Opening balance - owner's capital",
        "Debit Amount": 1_500_000_000, "Bal. Account Type": "G/L Account", "Bal. Account No.": "30200"}))?;
    crate::journal::post(ctx, "GENERAL", "DEFAULT")?;
    let fa = |i: usize| format!("FA0000{}0", i);
    jnl(ctx, "ASSETS", json!({"Posting Date": d395, "Account Type": "Fixed Asset", "Account No.": fa(1), "FA Posting Type": "Acquisition Cost",
        "Description": "Purchase tow truck", "Amount": 385_000_000, "Bal. Account Type": "Bank Account", "Bal. Account No.": "OPERATING"}))?;
    jnl(ctx, "ASSETS", json!({"Posting Date": d395, "Account Type": "Fixed Asset", "Account No.": fa(2), "FA Posting Type": "Acquisition Cost",
        "Description": "Car lift received as grant (hibah)", "Amount": 95_000_000, "Bal. Account Type": "G/L Account", "Bal. Account No.": "20400"}))?;
    jnl(ctx, "ASSETS", json!({"Posting Date": d395, "Account Type": "Fixed Asset", "Account No.": fa(3), "FA Posting Type": "Acquisition Cost",
        "Description": "Scanner taken over (opening balance)", "Amount": 28_000_000, "Bal. Account Type": "G/L Account", "Bal. Account No.": "30200"}))?;
    crate::journal::post(ctx, "ASSETS", "DEFAULT")?;
    // The compressor comes on a purchase invoice from the vendor.
    let pi = crate::documents::new_document(
        ctx,
        &PURCHASE,
        "Invoice",
        "40000",
        &obj(json!({"Posting Date": d200, "Document Date": d200, "Vendor Invoice No.": "KMA-AC-0042"})),
        &[json!({"type": "Fixed Asset", "no": fa(4), "quantity": 1, "direct_unit_cost": 18_500_000})],
    )?;
    post_as(ctx, false, "Invoice", &pi)?;
    // Depreciation up to the end of last month.
    let first = chrono::NaiveDate::from_ymd_opt(chrono::Datelike::year(&ctx.work_date), chrono::Datelike::month(&ctx.work_date), 1);
    if let Some(end) = first.and_then(|d| d.pred_opt()) {
        crate::fa::calculate_depreciation(ctx, end, true, None)?;
    }
    Ok(())
}

pub fn transactions(ctx: &mut Ctx) -> Result<()> {
    opening_and_assets(ctx)?;
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

    let t = ctx.table("Item Journal Line")?;
    let r = crate::records::create(
        ctx,
        t,
        &obj(json!({"Journal Template Name": "RECLASS", "Journal Batch Name": "DEFAULT", "Item No.": "1200", "Location Code": "MAIN",
                    "New Location Code": "BANDUNG", "Quantity": 10})),
        None,
    )?;
    let _ = r;
    crate::item_journal::post(ctx, "RECLASS", "DEFAULT")?;
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

    // Older invoices that are now past due, so the overdue cues have something to show.
    let old = |days: u64| ctx.work_date.checked_sub_days(chrono::Days::new(days)).map(|d| d.to_string()).unwrap_or_default();
    let (d40, d45) = (old(40), old(45));
    let si = crate::documents::new_document(
        ctx,
        &SALES,
        "Invoice",
        "20000",
        &obj(json!({"Document Date": d40, "Posting Date": d40, "External Document No.": "GSM-PO-0931"})),
        &[json!({"no": "1200", "quantity": 4})],
    )?;
    post_as(ctx, true, "Invoice", &si)?;
    let pi = crate::documents::new_document(
        ctx,
        &PURCHASE,
        "Invoice",
        "40000",
        &obj(json!({"Document Date": d45, "Posting Date": d45, "Vendor Invoice No.": "KMA-0457"})),
        &[json!({"no": "1300", "quantity": 20})],
    )?;
    post_as(ctx, false, "Invoice", &pi)?;
    Ok(())
}

fn post_as(ctx: &mut Ctx, sales: bool, doc_type: &str, created: &Json) -> Result<String> {
    let no = created.get("no").and_then(Json::as_str).unwrap_or_default().to_string();
    let side = if sales { &SALES } else { &PURCHASE };
    let r = crate::posting::post_document(ctx, side, doc_type, &no, crate::posting::PostMode::BOTH)?;
    Ok(r.get("posted_no").and_then(Json::as_str).unwrap_or_default().to_string())
}

fn obj(j: Json) -> Map<String, Json> {
    j.as_object().cloned().unwrap_or_default()
}

/// Posts a document created by `new_document`; returns the posted number.
fn post(ctx: &mut Ctx, sales: bool, created: &Json) -> Result<String> {
    let no = created.get("no").and_then(Json::as_str).unwrap_or_default().to_string();
    let side = if sales { &SALES } else { &PURCHASE };
    let r = crate::posting::post_document(ctx, side, "Order", &no, crate::posting::PostMode::BOTH)?;
    Ok(r.get("posted_no").and_then(Json::as_str).unwrap_or_default().to_string())
}
