//! The base application's tables. Names, ids and field names follow Business Central's W1 Base
//! Application so data, pages and a later BC connector map one to one.

use erpcraft_meta::{FieldDef as F, Metadata, TableDef};

pub const CUSTOMER: &str = "Customer";
pub const VENDOR: &str = "Vendor";
pub const ITEM: &str = "Item";
pub const GL_ACCOUNT: &str = "G/L Account";
pub const SALES_HEADER: &str = "Sales Header";
pub const SALES_LINE: &str = "Sales Line";
pub const PURCH_HEADER: &str = "Purchase Header";
pub const PURCH_LINE: &str = "Purchase Line";
pub const SALES_INV_HEADER: &str = "Sales Invoice Header";
pub const SALES_INV_LINE: &str = "Sales Invoice Line";
pub const SALES_CR_HEADER: &str = "Sales Cr.Memo Header";
pub const SALES_CR_LINE: &str = "Sales Cr.Memo Line";
pub const PURCH_INV_HEADER: &str = "Purch. Inv. Header";
pub const PURCH_INV_LINE: &str = "Purch. Inv. Line";
pub const PURCH_CR_HEADER: &str = "Purch. Cr. Memo Hdr.";
pub const PURCH_CR_LINE: &str = "Purch. Cr. Memo Line";
pub const CUST_LEDGER: &str = "Cust. Ledger Entry";
pub const VEND_LEDGER: &str = "Vendor Ledger Entry";
pub const GL_ENTRY: &str = "G/L Entry";
pub const ITEM_LEDGER: &str = "Item Ledger Entry";
pub const BANK_LEDGER: &str = "Bank Account Ledger Entry";
pub const BANK_ACCOUNT: &str = "Bank Account";
pub const NO_SERIES: &str = "No. Series";
pub const COMPANY_INFO: &str = "Company Information";
pub const GL_SETUP: &str = "General Ledger Setup";
pub const SALES_SETUP: &str = "Sales & Receivables Setup";
pub const PURCH_SETUP: &str = "Purchases & Payables Setup";
pub const INVENTORY_SETUP: &str = "Inventory Setup";
pub const GEN_POSTING_SETUP: &str = "General Posting Setup";
pub const VAT_POSTING_SETUP: &str = "VAT Posting Setup";
pub const INVT_POSTING_SETUP: &str = "Inventory Posting Setup";
pub const CUST_POSTING_GROUP: &str = "Customer Posting Group";
pub const VEND_POSTING_GROUP: &str = "Vendor Posting Group";
pub const BANK_POSTING_GROUP: &str = "Bank Account Posting Group";
pub const PAYMENT_TERMS: &str = "Payment Terms";
pub const USER: &str = "User";
pub const PERMISSION_SET: &str = "Permission Set";
pub const PERMISSION: &str = "Permission";
pub const ACCESS_CONTROL: &str = "Access Control";
pub const USER_PERSONALIZATION: &str = "User Personalization";

pub const SALES_DOC_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order"];
pub const PURCH_DOC_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order"];
pub const DOC_STATUS: &[&str] = &["Open", "Released", "Pending Approval", "Pending Prepayment"];
pub const LINE_TYPES: &[&str] = &["", "G/L Account", "Item", "Resource", "Fixed Asset", "Charge (Item)"];
pub const LEDGER_DOC_TYPES: &[&str] =
    &["", "Payment", "Invoice", "Credit Memo", "Finance Charge Memo", "Reminder", "Refund"];

fn code_desc(name: &str, id: u32, page: &str) -> TableDef {
    TableDef::new(name, id)
        .fields(vec![F::code("Code", 20).not_blank(), F::text("Description", 100)])
        .key(&["Code"])
        .describe_by("Description")
        .pages(page, None)
}

fn setup(name: &str, id: u32, page: &str, fields: Vec<F>) -> TableDef {
    TableDef::new(name, id).field_def(F::code("Primary Key", 10)).fields(fields).key(&["Primary Key"]).pages(page, Some(page))
}

fn address(prefix: &str) -> Vec<F> {
    vec![
        F::text(&format!("{prefix}Address"), 100),
        F::text(&format!("{prefix}Address 2"), 50),
        F::text(&format!("{prefix}City"), 30),
        F::code(&format!("{prefix}Post Code"), 20),
        F::code(&format!("{prefix}Country/Region Code"), 10),
    ]
}

pub fn register(m: &mut Metadata) {
    system(m);
    // ---- setup & reference data -------------------------------------------------------------
    m.add_table(
        TableDef::new(PAYMENT_TERMS, 3)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::code("Due Date Calculation", 32).tooltip("Date formula: 30D, 2W, 1M, CM, 1M+CM"),
                F::code("Discount Date Calculation", 32),
                F::decimal("Discount %"),
                F::text("Description", 100),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("payment-terms", None),
    );
    m.add_table(
        TableDef::new("Currency", 4)
            .fields(vec![F::code("Code", 10).not_blank(), F::text("Description", 30), F::code("Symbol", 10)])
            .key(&["Code"])
            .describe_by("Description")
            .pages("currencies", None),
    );
    m.add_table(
        TableDef::new("Salesperson/Purchaser", 13)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Name", 50),
                F::text("E-Mail", 80),
                F::text("Phone No.", 30),
                F::decimal("Commission %"),
            ])
            .key(&["Code"])
            .describe_by("Name")
            .pages("salespersons", None),
    );
    m.add_table(
        TableDef::new("Unit of Measure", 204)
            .fields(vec![F::code("Code", 10).not_blank(), F::text("Description", 50)])
            .key(&["Code"])
            .describe_by("Description")
            .pages("units-of-measure", None),
    );
    m.add_table(code_desc("Item Category", 5722, "item-categories"));
    m.add_table(code_desc("Gen. Business Posting Group", 250, "gen-bus-posting-groups"));
    m.add_table(code_desc("Gen. Product Posting Group", 251, "gen-prod-posting-groups"));
    m.add_table(code_desc("VAT Business Posting Group", 323, "vat-bus-posting-groups"));
    m.add_table(code_desc("VAT Product Posting Group", 324, "vat-prod-posting-groups"));
    m.add_table(code_desc("Inventory Posting Group", 94, "inventory-posting-groups"));
    m.add_table(
        TableDef::new(CUST_POSTING_GROUP, 92)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Description", 100),
                F::code("Receivables Account", 20).relation(GL_ACCOUNT),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("customer-posting-groups", None),
    );
    m.add_table(
        TableDef::new(VEND_POSTING_GROUP, 93)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Description", 100),
                F::code("Payables Account", 20).relation(GL_ACCOUNT),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("vendor-posting-groups", None),
    );
    m.add_table(
        TableDef::new(BANK_POSTING_GROUP, 277)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Description", 100),
                F::code("G/L Account No.", 20).relation(GL_ACCOUNT),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("bank-posting-groups", None),
    );
    m.add_table(
        TableDef::new(GEN_POSTING_SETUP, 252)
            .fields(vec![
                F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
                F::code("Sales Account", 20).relation(GL_ACCOUNT),
                F::code("Sales Credit Memo Account", 20).relation(GL_ACCOUNT),
                F::code("Purch. Account", 20).relation(GL_ACCOUNT),
                F::code("Purch. Credit Memo Account", 20).relation(GL_ACCOUNT),
                F::code("COGS Account", 20).relation(GL_ACCOUNT),
            ])
            .key(&["Gen. Bus. Posting Group", "Gen. Prod. Posting Group"])
            .pages("general-posting-setup", None),
    );
    m.add_table(
        TableDef::new(VAT_POSTING_SETUP, 325)
            .fields(vec![
                F::code("VAT Bus. Posting Group", 20).relation("VAT Business Posting Group"),
                F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
                F::option("VAT Calculation Type", &["Normal VAT", "Reverse Charge VAT", "Full VAT", "Sales Tax"]),
                F::decimal("VAT %"),
                F::code("Sales VAT Account", 20).relation(GL_ACCOUNT),
                F::code("Purchase VAT Account", 20).relation(GL_ACCOUNT),
                F::text("Description", 100),
            ])
            .key(&["VAT Bus. Posting Group", "VAT Prod. Posting Group"])
            .pages("vat-posting-setup", None),
    );
    m.add_table(
        TableDef::new(INVT_POSTING_SETUP, 5813)
            .fields(vec![
                F::code("Location Code", 10),
                F::code("Invt. Posting Group Code", 20).relation("Inventory Posting Group"),
                F::code("Inventory Account", 20).relation(GL_ACCOUNT),
                F::text("Description", 100),
            ])
            .key(&["Location Code", "Invt. Posting Group Code"])
            .pages("inventory-posting-setup", None),
    );
    m.add_table(
        TableDef::new(NO_SERIES, 308)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Description", 100),
                F::code("Starting No.", 20),
                F::code("Ending No.", 20),
                F::code("Last No. Used", 20),
                F::integer("Increment-by No."),
                F::boolean("Default Nos."),
                F::boolean("Manual Nos."),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("no-series", None),
    );
    m.add_table(setup(
        COMPANY_INFO,
        79,
        "company-information",
        [
            vec![F::text("Name", 100), F::text("Name 2", 50)],
            address(""),
            vec![
                F::text("Phone No.", 30),
                F::text("E-Mail", 80),
                F::text("Home Page", 80),
                F::text("VAT Registration No.", 20),
                F::text("Bank Name", 100),
                F::text("Bank Branch No.", 20),
                F::text("Bank Account No.", 30),
                F::code("IBAN", 50),
                F::code("SWIFT Code", 20),
            ],
        ]
        .concat(),
    ));
    m.add_table(setup(
        GL_SETUP,
        98,
        "general-ledger-setup",
        vec![
            F::code("LCY Code", 10),
            F::text("Local Currency Description", 60),
            F::date("Allow Posting From"),
            F::date("Allow Posting To"),
            F::decimal("Amount Rounding Precision"),
            F::code("Payment Nos.", 20).relation(NO_SERIES),
        ],
    ));
    m.add_table(setup(
        SALES_SETUP,
        311,
        "sales-receivables-setup",
        vec![
            F::code("Customer Nos.", 20).relation(NO_SERIES),
            F::code("Quote Nos.", 20).relation(NO_SERIES),
            F::code("Order Nos.", 20).relation(NO_SERIES),
            F::code("Invoice Nos.", 20).relation(NO_SERIES),
            F::code("Credit Memo Nos.", 20).relation(NO_SERIES),
            F::code("Return Order Nos.", 20).relation(NO_SERIES),
            F::code("Posted Invoice Nos.", 20).relation(NO_SERIES),
            F::code("Posted Credit Memo Nos.", 20).relation(NO_SERIES),
            F::option("Credit Warnings", &["Both Warnings", "Credit Limit", "Overdue Balance", "No Warning"]),
            F::boolean("Stockout Warning"),
            F::boolean("Ext. Doc. No. Mandatory"),
        ],
    ));
    m.add_table(setup(
        PURCH_SETUP,
        312,
        "purchases-payables-setup",
        vec![
            F::code("Vendor Nos.", 20).relation(NO_SERIES),
            F::code("Quote Nos.", 20).relation(NO_SERIES),
            F::code("Order Nos.", 20).relation(NO_SERIES),
            F::code("Invoice Nos.", 20).relation(NO_SERIES),
            F::code("Credit Memo Nos.", 20).relation(NO_SERIES),
            F::code("Return Order Nos.", 20).relation(NO_SERIES),
            F::code("Posted Invoice Nos.", 20).relation(NO_SERIES),
            F::code("Posted Credit Memo Nos.", 20).relation(NO_SERIES),
            F::boolean("Ext. Doc. No. Mandatory"),
        ],
    ));
    m.add_table(setup(INVENTORY_SETUP, 313, "inventory-setup", vec![F::code("Item Nos.", 20).relation(NO_SERIES)]));

    // ---- master data -----------------------------------------------------------------------
    m.add_table(
        TableDef::new(GL_ACCOUNT, 15)
            .fields(vec![
                F::code("No.", 20).not_blank(),
                F::text("Name", 100),
                F::option("Account Type", &["Posting", "Heading", "Total", "Begin-Total", "End-Total"]),
                F::option("Income/Balance", &["Income Statement", "Balance Sheet"]),
                F::option("Account Category", &["", "Assets", "Liabilities", "Equity", "Income", "Cost of Goods Sold", "Expense"]),
                F::option("Debit/Credit", &["Both", "Debit", "Credit"]),
                F::text("Totaling", 250).tooltip("Account range for Total / End-Total accounts, e.g. 10100..10999"),
                F::integer("Indentation").read_only(),
                F::boolean("Direct Posting"),
                F::boolean("Blocked"),
                F::option("Gen. Posting Type", &["", "Purchase", "Sale"]),
                F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
                F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
                F::decimal("Net Change").flow(),
                F::decimal("Balance").flow(),
            ])
            .key(&["No."])
            .describe_by("Name")
            .pages("chart-of-accounts", Some("gl-account-card")),
    );
    m.add_table(
        TableDef::new(CUSTOMER, 18)
            .fields(
                [
                    vec![
                        F::code("No.", 20),
                        F::text("Name", 100),
                        F::text("Name 2", 50),
                        F::code("Search Name", 100).read_only(),
                    ],
                    address(""),
                    vec![
                        F::text("County", 30),
                        F::text("Phone No.", 30),
                        F::text("Mobile Phone No.", 30),
                        F::text("E-Mail", 80),
                        F::text("Home Page", 80),
                        F::text("Contact", 100),
                        F::code("Salesperson Code", 20).relation("Salesperson/Purchaser"),
                        F::decimal("Credit Limit (LCY)"),
                        F::option("Blocked", &["", "Ship", "Invoice", "All"]),
                        F::code("Bill-to Customer No.", 20).relation(CUSTOMER),
                        F::text("VAT Registration No.", 20),
                        F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                        F::code("VAT Bus. Posting Group", 20).relation("VAT Business Posting Group"),
                        F::code("Customer Posting Group", 20).relation(CUST_POSTING_GROUP),
                        F::code("Currency Code", 10).relation("Currency"),
                        F::boolean("Prices Including VAT"),
                        F::code("Payment Terms Code", 10).relation(PAYMENT_TERMS),
                        F::code("Payment Method Code", 10),
                        F::date("Last Date Modified").read_only(),
                        F::decimal("Balance (LCY)").flow(),
                        F::decimal("Balance Due (LCY)").flow(),
                        F::decimal("Sales (LCY)").flow(),
                        F::decimal("Payments (LCY)").flow(),
                        F::decimal("Outstanding Orders (LCY)").flow(),
                    ],
                ]
                .concat(),
            )
            .key(&["No."])
            .describe_by("Name")
            .pages("customer-list", Some("customer-card")),
    );
    m.add_table(
        TableDef::new(VENDOR, 23)
            .fields(
                [
                    vec![
                        F::code("No.", 20),
                        F::text("Name", 100),
                        F::text("Name 2", 50),
                        F::code("Search Name", 100).read_only(),
                    ],
                    address(""),
                    vec![
                        F::text("Phone No.", 30),
                        F::text("Mobile Phone No.", 30),
                        F::text("E-Mail", 80),
                        F::text("Home Page", 80),
                        F::text("Contact", 100),
                        F::code("Purchaser Code", 20).relation("Salesperson/Purchaser"),
                        F::option("Blocked", &["", "Payment", "All"]),
                        F::code("Pay-to Vendor No.", 20).relation(VENDOR),
                        F::text("VAT Registration No.", 20),
                        F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                        F::code("VAT Bus. Posting Group", 20).relation("VAT Business Posting Group"),
                        F::code("Vendor Posting Group", 20).relation(VEND_POSTING_GROUP),
                        F::code("Currency Code", 10).relation("Currency"),
                        F::boolean("Prices Including VAT"),
                        F::code("Payment Terms Code", 10).relation(PAYMENT_TERMS),
                        F::code("Payment Method Code", 10),
                        F::date("Last Date Modified").read_only(),
                        F::decimal("Balance (LCY)").flow(),
                        F::decimal("Balance Due (LCY)").flow(),
                        F::decimal("Purchases (LCY)").flow(),
                        F::decimal("Payments (LCY)").flow(),
                        F::decimal("Outstanding Orders (LCY)").flow(),
                    ],
                ]
                .concat(),
            )
            .key(&["No."])
            .describe_by("Name")
            .pages("vendor-list", Some("vendor-card")),
    );
    m.add_table(
        TableDef::new(ITEM, 27)
            .fields(vec![
                F::code("No.", 20),
                F::text("Description", 100),
                F::text("Description 2", 50),
                F::option("Type", &["Inventory", "Service", "Non-Inventory"]),
                F::code("Base Unit of Measure", 10).relation("Unit of Measure"),
                F::code("Item Category Code", 20).relation("Item Category"),
                F::boolean("Blocked"),
                F::boolean("Sales Blocked"),
                F::boolean("Purchasing Blocked"),
                F::decimal("Unit Price"),
                F::boolean("Price Includes VAT"),
                F::option("Costing Method", &["FIFO", "LIFO", "Specific", "Average", "Standard"]),
                F::decimal("Unit Cost"),
                F::decimal("Standard Cost"),
                F::decimal("Last Direct Cost").read_only(),
                F::decimal("Profit %"),
                F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
                F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
                F::code("Inventory Posting Group", 20).relation("Inventory Posting Group"),
                F::code("Vendor No.", 20).relation(VENDOR),
                F::text("Vendor Item No.", 50),
                F::decimal("Reorder Point"),
                F::decimal("Maximum Inventory"),
                F::date("Last Date Modified").read_only(),
                F::decimal("Inventory").flow(),
                F::decimal("Qty. on Sales Order").flow(),
                F::decimal("Qty. on Purch. Order").flow(),
            ])
            .key(&["No."])
            .describe_by("Description")
            .pages("item-list", Some("item-card")),
    );
    m.add_table(
        TableDef::new(BANK_ACCOUNT, 270)
            .fields(
                [
                    vec![F::code("No.", 20).not_blank(), F::text("Name", 100)],
                    address(""),
                    vec![
                        F::text("Phone No.", 30),
                        F::text("Contact", 100),
                        F::text("Bank Account No.", 30),
                        F::text("Bank Branch No.", 20),
                        F::code("IBAN", 50),
                        F::code("SWIFT Code", 20),
                        F::code("Currency Code", 10).relation("Currency"),
                        F::code("Bank Acc. Posting Group", 20).relation(BANK_POSTING_GROUP),
                        F::boolean("Blocked"),
                        F::decimal("Balance").flow(),
                    ],
                ]
                .concat(),
            )
            .key(&["No."])
            .describe_by("Name")
            .pages("bank-account-list", Some("bank-account-card")),
    );

    // ---- documents -------------------------------------------------------------------------
    m.add_table(sales_header());
    m.add_table(sales_line());
    m.add_table(purch_header());
    m.add_table(purch_line());
    m.add_table(posted_sales_header(SALES_INV_HEADER, 112, "posted-sales-invoices", "posted-sales-invoice"));
    m.add_table(posted_line(SALES_INV_LINE, 113, "Unit Price"));
    m.add_table(posted_sales_header(SALES_CR_HEADER, 114, "posted-sales-credit-memos", "posted-sales-credit-memo"));
    m.add_table(posted_line(SALES_CR_LINE, 115, "Unit Price"));
    m.add_table(posted_purch_header(PURCH_INV_HEADER, 122, "posted-purchase-invoices", "posted-purchase-invoice"));
    m.add_table(posted_line(PURCH_INV_LINE, 123, "Direct Unit Cost"));
    m.add_table(posted_purch_header(PURCH_CR_HEADER, 124, "posted-purchase-credit-memos", "posted-purchase-credit-memo"));
    m.add_table(posted_line(PURCH_CR_LINE, 125, "Direct Unit Cost"));

    // ---- ledgers ---------------------------------------------------------------------------
    m.add_table(
        TableDef::new(GL_ENTRY, 17)
            .fields(vec![
                F::integer("Entry No."),
                F::code("G/L Account No.", 20).relation(GL_ACCOUNT),
                F::text("G/L Account Name", 100),
                F::date("Posting Date"),
                F::option("Document Type", LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::text("Description", 100),
                F::decimal("Amount"),
                F::decimal("Debit Amount"),
                F::decimal("Credit Amount"),
                F::decimal("VAT Amount"),
                F::option("Gen. Posting Type", &["", "Purchase", "Sale", "Settlement"]),
                F::option("Source Type", &["", "Customer", "Vendor", "Bank Account", "Fixed Asset", "Employee"]),
                F::code("Source No.", 20),
                F::code("External Document No.", 35),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("general-ledger-entries", None),
    );
    m.add_table(
        TableDef::new(CUST_LEDGER, 21)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Customer No.", 20).relation(CUSTOMER),
                F::text("Customer Name", 100),
                F::date("Posting Date"),
                F::option("Document Type", LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::code("External Document No.", 35),
                F::text("Description", 100),
                F::code("Currency Code", 10),
                F::decimal("Amount"),
                F::decimal("Amount (LCY)"),
                F::decimal("Remaining Amount"),
                F::decimal("Sales (LCY)"),
                F::date("Due Date"),
                F::boolean("Open"),
                F::integer("Closed by Entry No."),
                F::code("Salesperson Code", 20),
                F::code("Customer Posting Group", 20),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("customer-ledger-entries", None),
    );
    m.add_table(
        TableDef::new(VEND_LEDGER, 25)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Vendor No.", 20).relation(VENDOR),
                F::text("Vendor Name", 100),
                F::date("Posting Date"),
                F::option("Document Type", LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::code("External Document No.", 35),
                F::text("Description", 100),
                F::code("Currency Code", 10),
                F::decimal("Amount"),
                F::decimal("Amount (LCY)"),
                F::decimal("Remaining Amount"),
                F::decimal("Purchase (LCY)"),
                F::date("Due Date"),
                F::boolean("Open"),
                F::integer("Closed by Entry No."),
                F::code("Purchaser Code", 20),
                F::code("Vendor Posting Group", 20),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("vendor-ledger-entries", None),
    );
    m.add_table(
        TableDef::new(ITEM_LEDGER, 32)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Item No.", 20).relation(ITEM),
                F::date("Posting Date"),
                F::option(
                    "Entry Type",
                    &["Purchase", "Sale", "Positive Adjmt.", "Negative Adjmt.", "Transfer", "Consumption", "Output"],
                ),
                F::option(
                    "Document Type",
                    &[
                        "",
                        "Sales Shipment",
                        "Sales Invoice",
                        "Sales Return Receipt",
                        "Sales Credit Memo",
                        "Purchase Receipt",
                        "Purchase Invoice",
                        "Purchase Return Shipment",
                        "Purchase Credit Memo",
                        "Transfer Shipment",
                        "Transfer Receipt",
                    ],
                ),
                F::code("Document No.", 20),
                F::text("Description", 100),
                F::code("Location Code", 10),
                F::decimal("Quantity"),
                F::decimal("Remaining Quantity"),
                F::decimal("Cost Amount (Actual)"),
                F::decimal("Sales Amount (Actual)"),
                F::code("Unit of Measure Code", 10),
                F::option("Source Type", &["", "Customer", "Vendor", "Item"]),
                F::code("Source No.", 20),
                F::boolean("Open"),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("item-ledger-entries", None),
    );
    m.add_table(
        TableDef::new(BANK_LEDGER, 271)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Bank Account No.", 20).relation(BANK_ACCOUNT),
                F::date("Posting Date"),
                F::option("Document Type", LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::text("Description", 100),
                F::decimal("Amount"),
                F::decimal("Remaining Amount"),
                F::option("Bal. Account Type", &["G/L Account", "Customer", "Vendor", "Bank Account", "Fixed Asset"]),
                F::code("Bal. Account No.", 20),
                F::boolean("Open"),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("bank-account-ledger-entries", None),
    );
}

fn sales_header() -> TableDef {
    TableDef::new(SALES_HEADER, 36)
        .fields(
            [
                vec![
                    F::option("Document Type", SALES_DOC_TYPES),
                    F::code("No.", 20),
                    F::code("Sell-to Customer No.", 20).relation(CUSTOMER),
                    F::text("Sell-to Customer Name", 100),
                ],
                address("Sell-to "),
                vec![
                    F::text("Sell-to Contact", 100),
                    F::text("Sell-to Phone No.", 30),
                    F::text("Sell-to E-Mail", 80),
                    F::code("Bill-to Customer No.", 20).relation(CUSTOMER),
                    F::text("Bill-to Name", 100),
                    F::date("Order Date"),
                    F::date("Posting Date"),
                    F::date("Document Date"),
                    F::date("Due Date"),
                    F::date("Requested Delivery Date"),
                    F::date("Promised Delivery Date"),
                    F::date("Shipment Date"),
                    F::code("External Document No.", 35),
                    F::text("Your Reference", 35),
                    F::code("Salesperson Code", 20).relation("Salesperson/Purchaser"),
                    F::code("Payment Terms Code", 10).relation(PAYMENT_TERMS),
                    F::code("Payment Method Code", 10),
                    F::code("Currency Code", 10).relation("Currency"),
                    F::boolean("Prices Including VAT"),
                    F::code("Customer Posting Group", 20).relation(CUST_POSTING_GROUP),
                    F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                    F::code("VAT Bus. Posting Group", 20).relation("VAT Business Posting Group"),
                    F::text("VAT Registration No.", 20),
                    F::text("Posting Description", 100),
                    F::text("Work Description", 250),
                    F::code("Assigned User ID", 50),
                    F::option("Status", DOC_STATUS).read_only(),
                    F::code("Quote No.", 20).read_only(),
                    F::code("Last Posting No.", 20).read_only(),
                    F::decimal("Amount").flow(),
                    F::decimal("Amount Including VAT").flow(),
                    F::decimal("VAT Amount").flow(),
                ],
            ]
            .concat(),
        )
        .key(&["Document Type", "No."])
        .describe_by("Sell-to Customer Name")
        .pages("sales-order-list", Some("sales-order"))
}

fn sales_line() -> TableDef {
    TableDef::new(SALES_LINE, 37)
        .fields(vec![
            F::option("Document Type", SALES_DOC_TYPES),
            F::code("Document No.", 20),
            F::integer("Line No."),
            F::code("Sell-to Customer No.", 20).read_only(),
            F::option("Type", LINE_TYPES),
            F::code("No.", 20),
            F::text("Description", 100),
            F::text("Description 2", 50),
            F::code("Unit of Measure Code", 10),
            F::decimal("Quantity"),
            F::decimal("Unit Price"),
            F::decimal("Line Discount %"),
            F::decimal("Line Discount Amount").read_only(),
            F::decimal("Line Amount").read_only(),
            F::decimal("Amount").read_only(),
            F::decimal("Amount Including VAT").read_only(),
            F::decimal("VAT %").read_only(),
            F::decimal("Unit Cost (LCY)").read_only(),
            F::decimal("Outstanding Quantity").read_only(),
            F::decimal("Qty. to Ship"),
            F::decimal("Quantity Shipped").read_only(),
            F::decimal("Qty. to Invoice"),
            F::decimal("Quantity Invoiced").read_only(),
            F::date("Shipment Date"),
            F::code("Gen. Bus. Posting Group", 20).read_only(),
            F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
            F::code("VAT Bus. Posting Group", 20).read_only(),
            F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
        ])
        .key(&["Document Type", "Document No.", "Line No."])
        .describe_by("Description")
}

fn purch_header() -> TableDef {
    TableDef::new(PURCH_HEADER, 38)
        .fields(
            [
                vec![
                    F::option("Document Type", PURCH_DOC_TYPES),
                    F::code("No.", 20),
                    F::code("Buy-from Vendor No.", 20).relation(VENDOR),
                    F::text("Buy-from Vendor Name", 100),
                ],
                address("Buy-from "),
                vec![
                    F::text("Buy-from Contact", 100),
                    F::code("Pay-to Vendor No.", 20).relation(VENDOR),
                    F::text("Pay-to Name", 100),
                    F::date("Order Date"),
                    F::date("Posting Date"),
                    F::date("Document Date"),
                    F::date("Due Date"),
                    F::date("Expected Receipt Date"),
                    F::code("Vendor Invoice No.", 35),
                    F::code("Vendor Cr. Memo No.", 35),
                    F::code("Vendor Order No.", 35),
                    F::text("Your Reference", 35),
                    F::code("Purchaser Code", 20).relation("Salesperson/Purchaser"),
                    F::code("Payment Terms Code", 10).relation(PAYMENT_TERMS),
                    F::code("Payment Method Code", 10),
                    F::code("Currency Code", 10).relation("Currency"),
                    F::boolean("Prices Including VAT"),
                    F::code("Vendor Posting Group", 20).relation(VEND_POSTING_GROUP),
                    F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
                    F::code("VAT Bus. Posting Group", 20).relation("VAT Business Posting Group"),
                    F::text("Posting Description", 100),
                    F::code("Assigned User ID", 50),
                    F::option("Status", DOC_STATUS).read_only(),
                    F::code("Quote No.", 20).read_only(),
                    F::code("Last Posting No.", 20).read_only(),
                    F::decimal("Amount").flow(),
                    F::decimal("Amount Including VAT").flow(),
                    F::decimal("VAT Amount").flow(),
                ],
            ]
            .concat(),
        )
        .key(&["Document Type", "No."])
        .describe_by("Buy-from Vendor Name")
        .pages("purchase-order-list", Some("purchase-order"))
}

fn purch_line() -> TableDef {
    TableDef::new(PURCH_LINE, 39)
        .fields(vec![
            F::option("Document Type", PURCH_DOC_TYPES),
            F::code("Document No.", 20),
            F::integer("Line No."),
            F::code("Buy-from Vendor No.", 20).read_only(),
            F::option("Type", LINE_TYPES),
            F::code("No.", 20),
            F::text("Description", 100),
            F::text("Description 2", 50),
            F::code("Unit of Measure Code", 10),
            F::decimal("Quantity"),
            F::decimal("Direct Unit Cost"),
            F::decimal("Line Discount %"),
            F::decimal("Line Discount Amount").read_only(),
            F::decimal("Line Amount").read_only(),
            F::decimal("Amount").read_only(),
            F::decimal("Amount Including VAT").read_only(),
            F::decimal("VAT %").read_only(),
            F::decimal("Outstanding Quantity").read_only(),
            F::decimal("Qty. to Receive"),
            F::decimal("Quantity Received").read_only(),
            F::decimal("Qty. to Invoice"),
            F::decimal("Quantity Invoiced").read_only(),
            F::date("Expected Receipt Date"),
            F::code("Gen. Bus. Posting Group", 20).read_only(),
            F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
            F::code("VAT Bus. Posting Group", 20).read_only(),
            F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
        ])
        .key(&["Document Type", "Document No.", "Line No."])
        .describe_by("Description")
}

fn posted_sales_header(name: &str, id: u32, list: &str, card: &str) -> TableDef {
    TableDef::new(name, id)
        .fields(vec![
            F::code("No.", 20),
            F::code("Sell-to Customer No.", 20).relation(CUSTOMER),
            F::text("Sell-to Customer Name", 100),
            F::text("Sell-to Address", 100),
            F::text("Sell-to City", 30),
            F::code("Bill-to Customer No.", 20).relation(CUSTOMER),
            F::text("Bill-to Name", 100),
            F::date("Posting Date"),
            F::date("Document Date"),
            F::date("Due Date"),
            F::code("Order No.", 20),
            F::code("Pre-Assigned No.", 20),
            F::code("External Document No.", 35),
            F::text("Your Reference", 35),
            F::code("Salesperson Code", 20),
            F::code("Payment Terms Code", 10),
            F::code("Currency Code", 10),
            F::boolean("Prices Including VAT"),
            F::code("Customer Posting Group", 20),
            F::text("Posting Description", 100),
            F::code("User ID", 50),
            F::decimal("Amount").flow(),
            F::decimal("Amount Including VAT").flow(),
            F::decimal("Remaining Amount").flow(),
            F::boolean("Closed").flow(),
        ])
        .key(&["No."])
        .describe_by("Sell-to Customer Name")
        .pages(list, Some(card))
}

fn posted_purch_header(name: &str, id: u32, list: &str, card: &str) -> TableDef {
    TableDef::new(name, id)
        .fields(vec![
            F::code("No.", 20),
            F::code("Buy-from Vendor No.", 20).relation(VENDOR),
            F::text("Buy-from Vendor Name", 100),
            F::text("Buy-from Address", 100),
            F::text("Buy-from City", 30),
            F::code("Pay-to Vendor No.", 20).relation(VENDOR),
            F::text("Pay-to Name", 100),
            F::date("Posting Date"),
            F::date("Document Date"),
            F::date("Due Date"),
            F::code("Order No.", 20),
            F::code("Pre-Assigned No.", 20),
            F::code("Vendor Invoice No.", 35),
            F::code("Vendor Cr. Memo No.", 35),
            F::code("Purchaser Code", 20),
            F::code("Payment Terms Code", 10),
            F::code("Currency Code", 10),
            F::boolean("Prices Including VAT"),
            F::code("Vendor Posting Group", 20),
            F::text("Posting Description", 100),
            F::code("User ID", 50),
            F::decimal("Amount").flow(),
            F::decimal("Amount Including VAT").flow(),
            F::decimal("Remaining Amount").flow(),
            F::boolean("Closed").flow(),
        ])
        .key(&["No."])
        .describe_by("Buy-from Vendor Name")
        .pages(list, Some(card))
}

fn posted_line(name: &str, id: u32, price_field: &str) -> TableDef {
    TableDef::new(name, id)
        .fields(vec![
            F::code("Document No.", 20),
            F::integer("Line No."),
            F::option("Type", LINE_TYPES),
            F::code("No.", 20),
            F::text("Description", 100),
            F::code("Unit of Measure Code", 10),
            F::decimal("Quantity"),
            F::decimal(price_field),
            F::decimal("Line Discount %"),
            F::decimal("Line Discount Amount"),
            F::decimal("Line Amount"),
            F::decimal("Amount"),
            F::decimal("Amount Including VAT"),
            F::decimal("VAT %"),
            F::decimal("Unit Cost (LCY)"),
            F::code("Gen. Prod. Posting Group", 20),
            F::code("VAT Prod. Posting Group", 20),
        ])
        .key(&["Document No.", "Line No."])
        .describe_by("Description")
}

/// Users and permissions, after BC's system tables (User 2000000120, Permission Set 2000000004,
/// Permission 2000000005, Access Control 2000000053, User Personalization 2000000073). Permissions
/// name tables and commands; `*` stands for all of them.
fn system(m: &mut Metadata) {
    m.add_table(
        TableDef::new(USER, 2000000120)
            .fields(vec![
                F::code("User Name", 50).not_blank(),
                F::text("Full Name", 80),
                F::option("State", &["Enabled", "Disabled"]),
                F::text("Contact Email", 250),
            ])
            .key(&["User Name"])
            .describe_by("Full Name")
            .pages("users", None),
    );
    m.add_table(
        TableDef::new(PERMISSION_SET, 2000000004)
            .fields(vec![F::code("Role ID", 20).not_blank(), F::text("Name", 30)])
            .key(&["Role ID"])
            .describe_by("Name")
            .pages("permission-sets", None),
    );
    m.add_table(
        TableDef::new(PERMISSION, 2000000005)
            .fields(vec![
                F::code("Role ID", 20).relation(PERMISSION_SET),
                F::option("Object Type", &["Table Data", "Command"]),
                F::text("Object Name", 80).tooltip("A table name, a command id, or * for all"),
                F::boolean("Read Permission"),
                F::boolean("Insert Permission"),
                F::boolean("Modify Permission"),
                F::boolean("Delete Permission"),
                F::boolean("Execute Permission"),
            ])
            .key(&["Role ID", "Object Type", "Object Name"])
            .pages("permissions", None),
    );
    m.add_table(
        TableDef::new(ACCESS_CONTROL, 2000000053)
            .fields(vec![F::code("User Name", 50).relation(USER), F::code("Role ID", 20).relation(PERMISSION_SET)])
            .key(&["User Name", "Role ID"])
            .pages("user-permission-sets", None),
    );
    m.add_table(
        TableDef::new(USER_PERSONALIZATION, 2000000073)
            .fields(vec![
                F::code("User Name", 50).relation(USER),
                F::code("Profile ID", 30),
                F::date("Work Date"),
            ])
            .key(&["User Name"])
            .pages("user-personalization", None),
    );
}
