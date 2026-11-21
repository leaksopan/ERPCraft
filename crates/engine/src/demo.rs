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
        ("G-REC", "Recurring Journal", "REC00001"),
        ("T-ORD", "Transfer Orders", "T-ORD1001"),
        ("T-SHPT", "Posted Transfer Shipments", "T-SHPT1001"),
        ("T-RCPT", "Posted Transfer Receipts", "T-RCPT1001"),
        ("PI-ORD", "Phys. Invt. Orders", "PI-ORD1001"),
        ("PI-PSTD", "Posted Phys. Invt. Orders", "PI-PSTD1001"),
        ("S-REM", "Reminders", "S-REM1001"),
        ("S-IREM", "Issued Reminders", "S-IREM1001"),
        ("S-FIN", "Finance Charge Memos", "S-FIN1001"),
        ("S-IFIN", "Issued Finance Charge Memos", "S-IFIN1001"),
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
               "Reminder Nos.": "S-REM", "Issued Reminder Nos.": "S-IREM", "Fin. Chrg. Memo Nos.": "S-FIN", "Issued Fin. Chrg. M. Nos.": "S-IFIN",
               "Credit Warnings": "Both Warnings", "Stockout Warning": true}),
    )?;
    raw(
        ctx,
        "Purchases & Payables Setup",
        json!({"Primary Key": "", "Vendor Nos.": "VEND", "Quote Nos.": "P-QUO", "Order Nos.": "P-ORD", "Invoice Nos.": "P-INV",
               "Credit Memo Nos.": "P-CR", "Return Order Nos.": "P-RET", "Posted Invoice Nos.": "PP-INV", "Posted Credit Memo Nos.": "PP-CR", "Posted Receipt Nos.": "PP-RCPT",
               "Ext. Doc. No. Mandatory": false}),
    )?;
    raw(ctx, "Inventory Setup", json!({"Primary Key": "", "Item Nos.": "ITEM", "Automatic Cost Posting": true, "Transfer Order Nos.": "T-ORD", "Posted Transfer Shpt. Nos.": "T-SHPT", "Posted Transfer Rcpt. Nos.": "T-RCPT", "Phys. Invt. Order Nos.": "PI-ORD", "Posted Phys. Invt. Order Nos.": "PI-PSTD"}))?;
    for (template, ty, _, _, caption, series) in crate::schema::finance::JOURNALS {
        let source = match *template {
            "CASHRCPT" => "CASHRECJNL",
            "PAYMENT" => "PAYMENTJNL",
            _ => "GENJNL",
        };
        raw(ctx, "Gen. Journal Template", json!({"Name": template, "Description": caption, "Type": ty, "No. Series": series, "Source Code": source, "Recurring": *template == "RECURRING"}))?;
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
        ("20500", "VAT Settlement", "Posting", "Balance Sheet", "Liabilities", ""),
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
        ("40600", "Realized Exchange Rate Gains", "Posting", "Income Statement", "Income", ""),
        ("40700", "Unrealized Exchange Rate Gains", "Posting", "Income Statement", "Income", ""),
        ("40800", "Reminder Fees", "Posting", "Income Statement", "Income", ""),
        ("40900", "Finance Charge Interest", "Posting", "Income Statement", "Income", ""),
        ("40950", "Payment Discounts Received", "Posting", "Income Statement", "Income", ""),
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
        ("60700", "Realized Exchange Rate Losses", "Posting", "Income Statement", "Expense", ""),
        ("60800", "Unrealized Exchange Rate Losses", "Posting", "Income Statement", "Expense", ""),
        ("60900", "Payment Discounts Granted", "Posting", "Income Statement", "Expense", ""),
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
    // Stock on its way between warehouses.
    raw(ctx, "Location", json!({"Code": "TRANSIT", "Name": "In transit (own trucks)", "Use As In-Transit": true}))?;
    raw(ctx, "Inventory Posting Setup", json!({"Location Code": "TRANSIT", "Invt. Posting Group Code": "PARTS", "Inventory Account": "10300", "Description": "Spare parts in transit"}))?;
    for (code, desc, start) in [("I-JNL", "Item Journal", "IJ00001"), ("I-RCLSS", "Item Reclass. Journal", "IR00001"), ("I-PHYS", "Phys. Inventory Journal", "IP00001"), ("I-REVAL", "Revaluation Journal", "IRV00001"), ("FA-JNL", "FA G/L Journal", "FAJ00001")] {
        raw(ctx, "No. Series", json!({"Code": code, "Description": desc, "Starting No.": start, "Increment-by No.": 1}))?;
    }
    for (template, ty, _, _, caption, series, source) in crate::schema::inventory::ITEM_JOURNALS {
        raw(ctx, "Item Journal Template", json!({"Name": template, "Description": caption, "Type": ty, "No. Series": series, "Source Code": source}))?;
        raw(ctx, "Item Journal Batch", json!({"Journal Template Name": template, "Name": "DEFAULT", "Description": "Default journal", "No. Series": series}))?;
    }
    raw(ctx, "Gen. Journal Template", json!({"Name": "ASSETS", "Description": "Fixed Asset G/L Journal", "Type": "Assets", "No. Series": "FA-JNL", "Source Code": "FAGLJNL"}))?;
    raw(ctx, "Gen. Journal Batch", json!({"Journal Template Name": "ASSETS", "Name": "DEFAULT", "Description": "Default journal", "No. Series": "FA-JNL"}))?;
    fixed_asset_setup(ctx)?;
    raw(ctx, "Customer Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic customers", "Receivables Account": "10200", "Additional Fee Account": "40800", "Interest Account": "40900",
                                                   "Payment Disc. Debit Acc.": "60900", "Payment Disc. Credit Acc.": "60900"}))?;
    // Reminder terms (three levels, a fee from the second) and finance charge terms of 1.5% a month.
    raw(ctx, "Reminder Terms", json!({"Code": "DOMESTIC", "Description": "Domestic customers", "Post Additional Fee": true, "Post Interest": true, "Max. No. of Reminders": 3}))?;
    for (no, grace, fee, due) in [(1, "5D", 0, "7D"), (2, "7D", 50_000, "7D"), (3, "7D", 100_000, "5D")] {
        raw(ctx, "Reminder Level", json!({"Reminder Terms Code": "DOMESTIC", "No.": no, "Grace Period": grace, "Additional Fee (LCY)": fee, "Due Date Calculation": due}))?;
    }
    raw(
        ctx,
        "Finance Charge Terms",
        json!({"Code": "1.5 DOM.", "Description": "1.5% for domestic customers", "Interest Rate": 1.5, "Interest Period (Days)": 30, "Interest Calculation Method": "Average Daily Balance",
               "Grace Period": "5D", "Due Date Calculation": "1M", "Post Interest": true, "Post Additional Fee": true, "Line Description": "%1 %2 is overdue"}),
    )?;
    raw(ctx, "Vendor Posting Group", json!({"Code": "DOMESTIC", "Description": "Domestic vendors", "Payables Account": "20200", "Payment Disc. Debit Acc.": "40950", "Payment Disc. Credit Acc.": "40950"}))?;
    raw(ctx, "Bank Account Posting Group", json!({"Code": "OPERATING", "Description": "Operating accounts", "G/L Account No.": "10120"}))?;
    for (code, formula, desc) in [("COD", "0D", "Cash on delivery"), ("14 DAYS", "14D", "Net 14 days"), ("30 DAYS", "30D", "Net 30 days"), ("CM", "CM", "End of current month")] {
        raw(ctx, "Payment Terms", json!({"Code": code, "Due Date Calculation": formula, "Description": desc}))?;
    }
    raw(ctx, "Payment Terms", json!({"Code": "2% 8 DAYS", "Due Date Calculation": "1M", "Discount Date Calculation": "8D", "Discount %": 2, "Description": "Net 1 month, 2% within 8 days"}))?;
    // Currencies with their gain / loss accounts and rates (IDR per unit) for 2025 and 2026.
    for (code, desc, sym, r2025, r2026) in [("USD", "US Dollar", "$", 15_500, 16_000), ("EUR", "Euro", "€", 16_800, 17_500), ("SGD", "Singapore Dollar", "S$", 11_500, 12_000)] {
        raw(
            ctx,
            "Currency",
            json!({"Code": code, "Description": desc, "Symbol": sym, "Realized Gains Acc.": "40600", "Unrealized Gains Acc.": "40700",
                   "Realized Losses Acc.": "60700", "Unrealized Losses Acc.": "60800", "Amount Rounding Precision": 0.01, "Invoice Rounding Precision": 0.01}),
        )?;
        for (date, rate) in [("2025-01-01", r2025), ("2026-01-01", r2026)] {
            raw(
                ctx,
                "Currency Exchange Rate",
                json!({"Currency Code": code, "Starting Date": date, "Exchange Rate Amount": 1, "Relational Exch. Rate Amount": rate,
                       "Adjustment Exch. Rate Amount": 1, "Relational Adjmt Exch Rate Amt": rate}),
            )?;
        }
    }
    for (code, desc) in [("PCS", "Piece"), ("SET", "Set"), ("LTR", "Liter"), ("HOUR", "Hour"), ("BOX", "Box")] {
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
                   "Vendor No.": vendor, "Reorder Point": if *ty == "Service" { 0 } else { 10 }, "Reserve": "Optional"}),
        )?;
    }
    // The requisition worksheet.
    raw(ctx, "Req. Wksh. Template", json!({"Name": "REQ", "Description": "Requisition Worksheet", "Type": "Req."}))?;
    raw(ctx, "Requisition Wksh. Name", json!({"Worksheet Template Name": "REQ", "Name": "DEFAULT", "Description": "Default worksheet"}))?;
    // Item charges: freight and insurance on purchases.
    for (no, desc) in [("FREIGHT", "Freight charges"), ("INSURANCE", "Transport insurance")] {
        raw(ctx, "Item Charge", json!({"No.": no, "Description": desc, "Gen. Prod. Posting Group": "PARTS", "VAT Prod. Posting Group": "STANDARD", "Search Description": desc.to_uppercase()}))?;
    }
    // Counting periods for cycle counting.
    for (code, desc, freq) in [("FAST", "Fast moving items", 12), ("SLOW", "Slow moving items", 2)] {
        raw(ctx, "Phys. Invt. Counting Period", json!({"Code": code, "Description": desc, "Count Frequency per Year": freq}))?;
    }
    // Item tracking codes: by lot, by serial number.
    raw(ctx, "Item Tracking Code", json!({"Code": "LOTALL", "Description": "Lot specific tracking", "Lot Specific Tracking": true, "Lot Warehouse Tracking": true}))?;
    raw(ctx, "Item Tracking Code", json!({"Code": "SNALL", "Description": "Serial number specific tracking", "SN Specific Tracking": true, "SN Warehouse Tracking": true}))?;
    // Engine oil also comes by the box of 12.
    raw(ctx, "Item Unit of Measure", json!({"Item No.": "1000", "Code": "BOX", "Qty. per Unit of Measure": 12}))?;
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

/// A Balance Sheet and an Income Statement on the demo chart of accounts, as BC's financial
/// reports (row and column definitions).
fn demo_financial_reports(ctx: &mut Ctx) -> Result<()> {
    use crate::schema::fin_reports::{ACC_SCHED_LINE, ACC_SCHED_NAME, COLUMN_LAYOUT, COLUMN_LAYOUT_NAME, FINANCIAL_REPORT};
    // (row no., description, totaling type, totaling, opposite sign, bold, indentation, row type)
    type Row<'a> = (&'a str, &'a str, &'a str, &'a str, bool, bool, i64, &'a str);
    let balance: &[Row] = &[
        ("10", "Assets", "Posting Accounts", "", false, true, 0, ""),
        ("11", "Cash and Bank", "Posting Accounts", "10110..10120", false, false, 1, ""),
        ("12", "Accounts Receivable", "Posting Accounts", "10200", false, false, 1, ""),
        ("13", "Inventory", "Posting Accounts", "10300", false, false, 1, ""),
        ("14", "Prepaid VAT", "Posting Accounts", "10400", false, false, 1, ""),
        ("15", "Fixed Assets, Net", "Posting Accounts", "10500..10510", false, false, 1, ""),
        ("19", "Total Assets", "Formula", "11..15", false, true, 0, ""),
        ("20", "Liabilities", "Posting Accounts", "", false, true, 0, ""),
        ("21", "Accounts Payable", "Posting Accounts", "20200", true, false, 1, ""),
        ("22", "VAT Payable", "Posting Accounts", "20300|20500", true, false, 1, ""),
        ("23", "Deferred Income", "Posting Accounts", "20400", true, false, 1, ""),
        ("29", "Total Liabilities", "Formula", "21..23", false, true, 0, ""),
        ("30", "Equity", "Posting Accounts", "", false, true, 0, ""),
        ("31", "Owner's Capital", "Posting Accounts", "30200", true, false, 1, ""),
        ("32", "Retained Earnings", "Posting Accounts", "30300", true, false, 1, ""),
        ("33", "Net Income for the Year", "Posting Accounts", "40000..69999", true, false, 1, "Balance at Date"),
        ("39", "Total Equity", "Formula", "31..33", false, true, 0, ""),
        ("40", "Total Liabilities and Equity", "Formula", "29+39", false, true, 0, ""),
    ];
    let income: &[Row] = &[
        ("10", "Revenue", "Posting Accounts", "", false, true, 0, ""),
        ("11", "Sales - Spare Parts", "Posting Accounts", "40200", true, false, 1, ""),
        ("12", "Sales - Workshop Services", "Posting Accounts", "40300", true, false, 1, ""),
        ("13", "Sales Returns & Allowances", "Posting Accounts", "40400", true, false, 1, ""),
        ("14", "Other Income", "Posting Accounts", "40500..40998", true, false, 1, ""),
        ("19", "Total Revenue", "Formula", "11..14", false, true, 0, ""),
        ("20", "Cost of Goods Sold", "Posting Accounts", "50200..50400", false, false, 0, ""),
        ("30", "Gross Profit", "Formula", "19-20", false, true, 0, ""),
        ("40", "Operating Expenses", "Posting Accounts", "", false, true, 0, ""),
        ("41", "Rent", "Posting Accounts", "60200", false, false, 1, ""),
        ("42", "Utilities", "Posting Accounts", "60300", false, false, 1, ""),
        ("43", "Salaries & Wages", "Posting Accounts", "60400", false, false, 1, ""),
        ("44", "Depreciation", "Posting Accounts", "60500", false, false, 1, ""),
        ("45", "Other Expenses", "Posting Accounts", "60600..60998", false, false, 1, ""),
        ("49", "Total Operating Expenses", "Formula", "41..45", false, true, 0, ""),
        ("50", "Net Income", "Formula", "30-49", false, true, 0, ""),
    ];
    for (name, desc, rows) in [("BALANCE", "Balance Sheet", balance), ("INCOME", "Income Statement", income)] {
        raw(ctx, ACC_SCHED_NAME, json!({"Name": name, "Description": desc}))?;
        for (i, (no, d, ty, tot, opp, bold, ind, row_type)) in rows.iter().enumerate() {
            let row_type = if row_type.is_empty() { "Net Change" } else { row_type };
            raw(
                ctx,
                ACC_SCHED_LINE,
                json!({"Schedule Name": name, "Line No.": (i as i64 + 1) * 10000, "Row No.": no, "Description": d, "Totaling Type": ty, "Totaling": tot,
                       "Show Opposite Sign": opp, "Bold": bold, "Indentation": ind, "Row Type": row_type, "Show": "Yes", "Amount Type": "Net Amount"}),
            )?;
        }
    }
    // (column no., header, type, comparison date formula, formula)
    type Col<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);
    let balance_cols: &[Col] = &[("A", "Balance", "Balance at Date", "", ""), ("B", "Prior Year", "Balance at Date", "-1Y", ""), ("C", "Change", "Formula", "", "A-B")];
    let period_cols: &[Col] = &[
        ("A", "Net Change", "Net Change", "", ""),
        ("B", "Prior Year", "Net Change", "-1Y", ""),
        ("C", "Change", "Formula", "", "A-B"),
        ("D", "Year to Date", "Year to Date", "", ""),
        ("E", "Budget", "Net Change", "", ""),
        ("F", "Variance", "Formula", "", "A-E"),
    ];
    for (name, desc, cols) in [("BALANCE", "Balance and prior year", balance_cols), ("PERIOD", "Period, prior year and year to date", period_cols)] {
        raw(ctx, COLUMN_LAYOUT_NAME, json!({"Name": name, "Description": desc}))?;
        for (i, (no, header, ty, cmp, formula)) in cols.iter().enumerate() {
            raw(
                ctx,
                COLUMN_LAYOUT,
                json!({"Column Layout Name": name, "Line No.": (i as i64 + 1) * 10000, "Column No.": no, "Column Header": header, "Column Type": ty,
                       "Comparison Date Formula": cmp, "Formula": formula, "Ledger Entry Type": if *header == "Budget" { "Budget Entries" } else { "Entries" },
                       "Budget Name": if *header == "Budget" { "2026" } else { "" }, "Amount Type": "Net Amount", "Show": "Always", "Rounding Factor": "None"}),
            )?;
        }
    }
    // Fiscal years 2025 and 2026 in months, and a 2026 budget.
    for year in [2025, 2026] {
        for month in 1..=12u32 {
            if let Some(d) = chrono::NaiveDate::from_ymd_opt(year, month, 1) {
                raw(ctx, "Accounting Period", json!({"Starting Date": d.to_string(), "Name": d.format("%B").to_string(), "New Fiscal Year": month == 1}))?;
            }
        }
    }
    raw(ctx, "G/L Budget Name", json!({"Name": "2026", "Description": "Budget 2026"}))?;
    let mut entry = 0;
    for (account, monthly) in [("40200", -20_000_000i64), ("40300", -15_000_000), ("50200", 12_000_000), ("60200", 15_000_000), ("60300", 3_000_000), ("60400", 60_000_000)] {
        for month in 1..=12u32 {
            entry += 1;
            if let Some(d) = chrono::NaiveDate::from_ymd_opt(2026, month, 1) {
                raw(ctx, "G/L Budget Entry", json!({"Entry No.": entry, "Budget Name": "2026", "G/L Account No.": account, "Date": d.to_string(), "Amount": monthly, "Description": "Budget 2026"}))?;
            }
        }
    }
    // A VAT statement: sales and purchase VAT and their bases, and the VAT to pay.
    raw(ctx, "VAT Statement Template", json!({"Name": "VAT", "Description": "VAT Statement"}))?;
    raw(ctx, "VAT Statement Name", json!({"Statement Template Name": "VAT", "Name": "DEFAULT", "Description": "Monthly VAT return (PPN)"}))?;
    // (row, description, type, posting type, amount type, row totaling, calculate opposite, print opposite, box)
    let vat_rows: [(&str, &str, &str, &str, &str, &str, bool, bool, &str); 6] = [
        ("10", "Output VAT (PPN Keluaran)", "VAT Entry Totaling", "Sale", "Amount", "", true, false, "1"),
        ("20", "Input VAT (PPN Masukan)", "VAT Entry Totaling", "Purchase", "Amount", "", true, true, "2"),
        ("30", "VAT to Pay", "Row Totaling", "", "", "10..20", false, false, "3"),
        ("40", "Taxable Sales", "VAT Entry Totaling", "Sale", "Base", "", true, false, "4"),
        ("50", "Taxable Purchases", "VAT Entry Totaling", "Purchase", "Base", "", false, false, "5"),
        ("60", "Settled VAT", "VAT Entry Totaling", "Settlement", "Amount", "", false, false, ""),
    ];
    for (i, (row, desc, ty, posting, amount_type, totaling, calc_opp, print_opp, boxno)) in vat_rows.iter().enumerate() {
        raw(
            ctx,
            "VAT Statement Line",
            json!({"Statement Template Name": "VAT", "Statement Name": "DEFAULT", "Line No.": (i as i64 + 1) * 10000, "Row No.": row, "Description": desc,
                   "Type": ty, "Gen. Posting Type": posting, "Amount Type": amount_type, "Row Totaling": totaling,
                   "Calculate with": if *calc_opp { "Opposite Sign" } else { "Sign" }, "Print": true,
                   "Print with": if *print_opp { "Opposite Sign" } else { "Sign" }, "Box No.": boxno}),
        )?;
    }
    raw(ctx, FINANCIAL_REPORT, json!({"Name": "BALANCE", "Description": "Balance Sheet", "Financial Report Row Group": "BALANCE", "Financial Report Column Group": "BALANCE"}))?;
    raw(ctx, FINANCIAL_REPORT, json!({"Name": "INCOME", "Description": "Income Statement", "Financial Report Row Group": "INCOME", "Financial Report Column Group": "PERIOD"}))?;
    Ok(())
}

fn demo_dimensions(ctx: &mut Ctx) -> Result<()> {
    demo_workflows(ctx)?;
    demo_financial_reports(ctx)?;
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
