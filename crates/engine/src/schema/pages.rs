//! The base application's pages. Layouts (FastTabs, columns, actions) follow the matching Business
//! Central pages (Customer Card = page 21, Sales Order = page 42, …); object ids are kept for
//! mapping to BC later.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FactBoxDef, GroupDef as G, Metadata, PageDef, PageKind as K};

use super::tables::{
    BANK_ACCOUNT, BANK_LEDGER, CUST_LEDGER, CUSTOMER, GL_ACCOUNT, GL_ENTRY, ITEM, ITEM_LEDGER, PURCH_CR_HEADER,
    PURCH_CR_LINE, PURCH_HEADER, PURCH_INV_HEADER, PURCH_INV_LINE, PURCH_LINE, SALES_CR_HEADER, SALES_CR_LINE,
    SALES_HEADER, SALES_INV_HEADER, SALES_INV_LINE, SALES_LINE, VEND_LEDGER, VENDOR,
};

/// Sales / purchase document types that get their own list + document page:
/// (document type, list id, list object id, list caption, card id, card object id, card caption).
pub const SALES_DOC_PAGES: &[(&str, &str, u32, &str, &str, u32, &str)] = &[
    ("Quote", "sales-quotes", 9300, "Sales Quotes", "sales-quote", 41, "Sales Quote"),
    ("Order", "sales-order-list", 9305, "Sales Orders", "sales-order", 42, "Sales Order"),
    ("Invoice", "sales-invoice-list", 9301, "Sales Invoices", "sales-invoice", 43, "Sales Invoice"),
    ("Credit Memo", "sales-credit-memos", 9302, "Sales Credit Memos", "sales-credit-memo", 44, "Sales Credit Memo"),
];
pub const PURCH_DOC_PAGES: &[(&str, &str, u32, &str, &str, u32, &str)] = &[
    ("Quote", "purchase-quotes", 9306, "Purchase Quotes", "purchase-quote", 49, "Purchase Quote"),
    ("Order", "purchase-order-list", 9307, "Purchase Orders", "purchase-order", 50, "Purchase Order"),
    ("Invoice", "purchase-invoices", 9308, "Purchase Invoices", "purchase-invoice", 51, "Purchase Invoice"),
    (
        "Credit Memo",
        "purchase-credit-memos",
        9309,
        "Purchase Credit Memos",
        "purchase-credit-memo",
        52,
        "Purchase Credit Memo",
    ),
];

/// The document page for a sales or purchase header of `doc_type`.
pub fn document_page(table: &str, doc_type: &str) -> Option<&'static str> {
    let pages = match table {
        SALES_HEADER => SALES_DOC_PAGES,
        PURCH_HEADER => PURCH_DOC_PAGES,
        _ => return None,
    };
    pages.iter().find(|p| p.0 == doc_type).map(|p| p.4)
}

fn list(id: &str, obj: u32, caption: &str, table: &str, card: Option<&str>, cols: &[&str]) -> PageDef {
    let mut p = PageDef::new(id, obj, caption, K::List, Some(table)).columns(cols);
    if let Some(c) = card {
        p = p.card(c).read_only();
    }
    p
}

fn ledger(id: &str, obj: u32, caption: &str, table: &str, cols: &[&str]) -> PageDef {
    PageDef::new(id, obj, caption, K::List, Some(table)).columns(cols).read_only().keywords("entries ledger history")
}

pub fn register(m: &mut Metadata) {
    master_data(m);
    sales_documents(m);
    purchase_documents(m);
    posted_documents(m);
    ledgers(m);
    setup(m);
}

fn master_data(m: &mut Metadata) {
    m.add_page(
        list(
            "customer-list",
            22,
            "Customers",
            CUSTOMER,
            Some("customer-card"),
            &["No.", "Name", "Phone No.", "Contact", "Balance (LCY)", "Balance Due (LCY)", "Sales (LCY)", "Payments (LCY)"],
        )
        .factbox(FactBoxDef::new(
            "Customer Statistics",
            None,
            &["No.", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Sales (LCY)", "Payments (LCY)", "Credit Limit (LCY)"],
        ))
        .actions(customer_actions())
        .keywords("customer client debtor"),
    );
    m.add_page(
        PageDef::new("customer-card", 21, "Customer Card", K::Card, Some(CUSTOMER))
            .group(
                G::new(
                    "General",
                    &[
                        "No.",
                        "Name",
                        "Name 2",
                        "Balance (LCY)",
                        "Balance Due (LCY)",
                        "Credit Limit (LCY)",
                        "Blocked",
                        "Salesperson Code",
                        "Sales (LCY)",
                        "Last Date Modified",
                    ],
                )
                .promoted(&["No.", "Name", "Balance (LCY)"]),
            )
            .group(
                G::new(
                    "Address & Contact",
                    &[
                        "Address",
                        "Address 2",
                        "Country/Region Code",
                        "City",
                        "County",
                        "Post Code",
                        "Phone No.",
                        "Mobile Phone No.",
                        "E-Mail",
                        "Home Page",
                        "Contact",
                    ],
                )
                .promoted(&["City", "Phone No."]),
            )
            .group(
                G::new(
                    "Invoicing",
                    &[
                        "Bill-to Customer No.",
                        "VAT Registration No.",
                        "Gen. Bus. Posting Group",
                        "VAT Bus. Posting Group",
                        "Customer Posting Group",
                        "Currency Code",
                        "Prices Including VAT",
                    ],
                )
                .promoted(&["Customer Posting Group"])
                .collapsed(),
            )
            .group(G::new("Payments", &["Payment Terms Code", "Payment Method Code"]).promoted(&["Payment Terms Code"]).collapsed())
            .factbox(FactBoxDef::new(
                "Customer Statistics",
                None,
                &["No.", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Sales (LCY)", "Payments (LCY)", "Credit Limit (LCY)"],
            ))
            .actions(customer_actions()),
    );
    m.add_page(
        list(
            "vendor-list",
            27,
            "Vendors",
            VENDOR,
            Some("vendor-card"),
            &["No.", "Name", "Phone No.", "Contact", "Balance (LCY)", "Balance Due (LCY)", "Purchases (LCY)", "Payments (LCY)"],
        )
        .factbox(FactBoxDef::new(
            "Vendor Statistics",
            None,
            &["No.", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Purchases (LCY)", "Payments (LCY)"],
        ))
        .actions(vendor_actions())
        .keywords("vendor supplier creditor"),
    );
    m.add_page(
        PageDef::new("vendor-card", 26, "Vendor Card", K::Card, Some(VENDOR))
            .group(
                G::new(
                    "General",
                    &["No.", "Name", "Name 2", "Balance (LCY)", "Balance Due (LCY)", "Blocked", "Purchaser Code", "Last Date Modified"],
                )
                .promoted(&["No.", "Name", "Balance (LCY)"]),
            )
            .group(
                G::new(
                    "Address & Contact",
                    &["Address", "Address 2", "Country/Region Code", "City", "Post Code", "Phone No.", "Mobile Phone No.", "E-Mail", "Home Page", "Contact"],
                )
                .promoted(&["City", "Phone No."]),
            )
            .group(
                G::new(
                    "Invoicing",
                    &[
                        "Pay-to Vendor No.",
                        "VAT Registration No.",
                        "Gen. Bus. Posting Group",
                        "VAT Bus. Posting Group",
                        "Vendor Posting Group",
                        "Currency Code",
                        "Prices Including VAT",
                    ],
                )
                .promoted(&["Vendor Posting Group"])
                .collapsed(),
            )
            .group(G::new("Payments", &["Payment Terms Code", "Payment Method Code"]).promoted(&["Payment Terms Code"]).collapsed())
            .factbox(FactBoxDef::new(
                "Vendor Statistics",
                None,
                &["No.", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Purchases (LCY)", "Payments (LCY)"],
            ))
            .actions(vendor_actions()),
    );
    m.add_page(
        list(
            "item-list",
            31,
            "Items",
            ITEM,
            Some("item-card"),
            &["No.", "Description", "Type", "Inventory", "Base Unit of Measure", "Unit Cost", "Unit Price", "Vendor No.", "Item Category Code"],
        )
        .factbox(FactBoxDef::new(
            "Item Details",
            None,
            &["No.", "Inventory", "Qty. on Sales Order", "Qty. on Purch. Order", "Unit Price", "Unit Cost", "Last Direct Cost"],
        ))
        .actions(item_actions())
        .keywords("item product inventory stock spare part"),
    );
    m.add_page(
        PageDef::new("item-card", 30, "Item Card", K::Card, Some(ITEM))
            .group(
                G::new(
                    "Item",
                    &["No.", "Description", "Description 2", "Blocked", "Type", "Base Unit of Measure", "Item Category Code", "Last Date Modified"],
                )
                .promoted(&["No.", "Description", "Type"]),
            )
            .group(
                G::new("Inventory", &["Inventory", "Qty. on Purch. Order", "Qty. on Sales Order", "Reorder Point", "Maximum Inventory"])
                    .promoted(&["Inventory"]),
            )
            .group(
                G::new(
                    "Costs & Posting",
                    &[
                        "Costing Method",
                        "Standard Cost",
                        "Unit Cost",
                        "Last Direct Cost",
                        "Gen. Prod. Posting Group",
                        "VAT Prod. Posting Group",
                        "Inventory Posting Group",
                    ],
                )
                .promoted(&["Unit Cost"]),
            )
            .group(G::new("Prices & Sales", &["Unit Price", "Price Includes VAT", "Profit %", "Sales Blocked"]).promoted(&["Unit Price"]))
            .group(
                G::new("Replenishment", &["Vendor No.", "Vendor Item No.", "Purchasing Blocked"]).promoted(&["Vendor No."]).collapsed(),
            )
            .factbox(FactBoxDef::new(
                "Item Details",
                None,
                &["No.", "Inventory", "Qty. on Sales Order", "Qty. on Purch. Order", "Unit Price", "Unit Cost", "Last Direct Cost"],
            ))
            .actions(item_actions()),
    );
    m.add_page(
        list(
            "chart-of-accounts",
            16,
            "Chart of Accounts",
            GL_ACCOUNT,
            Some("gl-account-card"),
            &["No.", "Name", "Net Change", "Income/Balance", "Account Category", "Account Type", "Direct Posting", "Totaling", "Gen. Posting Type", "Balance"],
        )
        .action(Act::page("G/L Entries", A::Navigate, "general-ledger-entries").link("G/L Account No.", "No.").shortcut("Ctrl+F7"))
        .action(Act::command("Trial Balance", A::Report, "report.trial_balance"))
        .keywords("gl general ledger accounts coa"),
    );
    m.add_page(
        PageDef::new("gl-account-card", 17, "G/L Account Card", K::Card, Some(GL_ACCOUNT))
            .group(
                G::new(
                    "General",
                    &["No.", "Name", "Income/Balance", "Account Category", "Debit/Credit", "Account Type", "Totaling", "Indentation", "Blocked", "Direct Posting"],
                )
                .promoted(&["No.", "Name"]),
            )
            .group(
                G::new("Posting", &["Gen. Posting Type", "Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "VAT Prod. Posting Group"])
                    .collapsed(),
            )
            .factbox(FactBoxDef::new("G/L Account Statistics", None, &["No.", "Net Change", "Balance"]))
            .action(Act::page("G/L Entries", A::Navigate, "general-ledger-entries").link("G/L Account No.", "No.").shortcut("Ctrl+F7")),
    );
    m.add_page(
        list(
            "bank-account-list",
            371,
            "Bank Accounts",
            BANK_ACCOUNT,
            Some("bank-account-card"),
            &["No.", "Name", "Bank Account No.", "Currency Code", "Phone No.", "Contact", "Balance"],
        )
        .action(Act::page("Ledger Entries", A::Navigate, "bank-account-ledger-entries").link("Bank Account No.", "No.").shortcut("Ctrl+F7"))
        .keywords("bank cash"),
    );
    m.add_page(
        PageDef::new("bank-account-card", 370, "Bank Account Card", K::Card, Some(BANK_ACCOUNT))
            .group(G::new("General", &["No.", "Name", "Bank Account No.", "Bank Branch No.", "Balance", "Blocked"]).promoted(&["No.", "Name", "Balance"]))
            .group(G::new("Communication", &["Address", "Address 2", "City", "Post Code", "Country/Region Code", "Phone No.", "Contact"]).collapsed())
            .group(G::new("Posting", &["Currency Code", "Bank Acc. Posting Group", "IBAN", "SWIFT Code"]).collapsed())
            .action(Act::page("Ledger Entries", A::Navigate, "bank-account-ledger-entries").link("Bank Account No.", "No.").shortcut("Ctrl+F7")),
    );
}

fn customer_actions() -> Vec<Act> {
    vec![
        Act::command("Sales Quote", A::New, "sales.document.new").param("document_type", "Quote").icon("quote").tooltip("Offer items or services to a customer."),
        Act::command("Sales Order", A::New, "sales.document.new").param("document_type", "Order").icon("order").tooltip("Create a sales order for the customer."),
        Act::command("Sales Invoice", A::New, "sales.document.new").param("document_type", "Invoice").icon("invoice").tooltip("Create a sales invoice for the customer."),
        Act::command("Sales Credit Memo", A::New, "sales.document.new").param("document_type", "Credit Memo").icon("credit").tooltip("Create a sales credit memo for the customer."),
        Act::command("Register Payment", A::Process, "payment.customer.register")
            .icon("payment")
            .tooltip("Post a payment received from the customer and apply it to open invoices."),
        Act::page("Ledger E&ntries", A::Navigate, "customer-ledger-entries").link("Customer No.", "No.").shortcut("Ctrl+F7").icon("ledger"),
        Act::page("Sales Orders", A::Navigate, "sales-order-list").link("Sell-to Customer No.", "No.").icon("order"),
        Act::page("Posted Sales Invoices", A::Navigate, "posted-sales-invoices").link("Sell-to Customer No.", "No."),
        Act::command("Statistics", A::Report, "customer.statistics").shortcut("F7").icon("stats"),
    ]
}

fn vendor_actions() -> Vec<Act> {
    vec![
        Act::command("Purchase Quote", A::New, "purchase.document.new").param("document_type", "Quote").icon("quote"),
        Act::command("Purchase Order", A::New, "purchase.document.new").param("document_type", "Order").icon("order"),
        Act::command("Purchase Invoice", A::New, "purchase.document.new").param("document_type", "Invoice").icon("invoice"),
        Act::command("Purchase Credit Memo", A::New, "purchase.document.new").param("document_type", "Credit Memo").icon("credit"),
        Act::command("Make Payment", A::Process, "payment.vendor.register")
            .icon("payment")
            .tooltip("Post a payment to the vendor and apply it to open invoices."),
        Act::page("Ledger E&ntries", A::Navigate, "vendor-ledger-entries").link("Vendor No.", "No.").shortcut("Ctrl+F7").icon("ledger"),
        Act::page("Purchase Orders", A::Navigate, "purchase-order-list").link("Buy-from Vendor No.", "No.").icon("order"),
        Act::page("Posted Purchase Invoices", A::Navigate, "posted-purchase-invoices").link("Buy-from Vendor No.", "No."),
        Act::command("Statistics", A::Report, "vendor.statistics").shortcut("F7").icon("stats"),
    ]
}

fn item_actions() -> Vec<Act> {
    vec![
        Act::command("Copy Item", A::Home, "item.copy")
            .icon("copy")
            .ask(&["target_no"])
            .tooltip("Create a copy of the current item."),
        Act::command("Adjust Inventory", A::Home, "item.adjust_inventory")
            .icon("adjust")
            .ask(&["location_code", "new_inventory"])
            .tooltip("Increase or decrease the item's inventory quantity manually by entering a new quantity. Adjusting the inventory quantity manually might be relevant after a physical count or if you do not record purchased quantities."),
        Act::page("Ledger E&ntries", A::Navigate, "item-ledger-entries").link("Item No.", "No.").shortcut("Ctrl+F7").icon("ledger"),
        Act::command("Item Availability", A::Report, "item.availability").icon("stats"),
    ]
}

fn doc_actions(sales: bool, doc_type: &str) -> Vec<Act> {
    let p = if sales { "sales" } else { "purchase" };
    let mut v = Vec::new();
    if doc_type == "Quote" {
        v.push(
            Act::command("Make Order", A::Home, &format!("{p}.quote.make_order"))
                .icon("order")
                .tooltip("Convert the quote to an order.")
                .confirm("Do you want to convert the quote to an order?"),
        );
    } else {
        v.push(
            {
                let a = Act::command("Post...", A::Home, &format!("{p}.document.post"))
                    .shortcut("F9")
                    .icon("post")
                    .tooltip("Finalize the document by posting the amounts and quantities to the related accounts.");
                if doc_type == "Order" { a.ask(&["mode"]) } else { a.confirm("Do you want to post the document?") }
            },
        );
    }
    v.push(Act::command("Release", A::Home, &format!("{p}.document.release")).shortcut("Ctrl+F9").icon("release"));
    v.push(Act::command("Reopen", A::Home, &format!("{p}.document.reopen")).icon("reopen"));
    // BC's posting group: Post and New, Preview Posting, Test Report.
    if doc_type != "Quote" {
        let post_new = Act::command("Post and New", A::Process, &format!("{p}.document.post_and_new"))
            .shortcut("Alt+F9")
            .icon("post")
            .tooltip(if sales { "Post the sales document and create a new, empty one." } else { "Post the purchase document and create a new, empty one." });
        v.push(if doc_type == "Order" { post_new.ask(&["mode"]) } else { post_new.confirm("Do you want to post the document?") });
        let post_print = Act::command("Post and &Print", A::Process, &format!("{p}.document.post_and_print"))
            .shortcut("Shift+F9")
            .icon("print")
            .tooltip("Finalize and prepare to print the document or journal. The values and quantities are posted to the related accounts.");
        v.push(if doc_type == "Order" { post_print.ask(&["mode"]) } else { post_print.confirm("Do you want to post and print the document?") });
        v.push(
            Act::command("Preview Posting", A::Process, &format!("{p}.document.preview_post"))
                .icon("preview")
                .tooltip("Review the different types of entries that will be created when you post the document or journal."),
        );
        v.push(
            Act::command("Test Report", A::Report, &format!("{p}.document.test_report"))
                .icon("report")
                .tooltip("View a test report so that you can find and correct any errors before you perform the actual posting."),
        );
    }
    v.push(
        Act::command("Copy Document", A::Process, &format!("{p}.document.copy"))
            .icon("copy")
            .ask(&["from_document_type", "from_no", "include_header", "recalculate_lines"])
            .tooltip("Copy document lines and header information from another document to this document."),
    );
    v.push(Act::command("Statistics", A::Process, &format!("{p}.document.statistics")).shortcut("F7").icon("stats"));
    v.push(if sales {
        Act::command("Customer Statistics", A::Navigate, "sales.document.customer_statistics")
            .icon("stats")
            .tooltip("View statistical information about the customer, such as the value of posted entries.")
    } else {
        Act::command("Vendor Statistics", A::Navigate, "purchase.document.vendor_statistics")
            .icon("stats")
            .tooltip("View statistical information about the vendor, such as the value of posted entries.")
    });
    v.push(Act::command("Print...", A::Report, "document.print").shortcut("Ctrl+P").icon("print").tooltip("Print the document to PDF."));
    if sales && doc_type == "Order" {
        v.push(Act::command("Print Confirmation", A::Report, "document.print").icon("print").tooltip("Print an order confirmation."));
        v.push(
            Act::command("Work Order", A::Report, "document.print")
                .param("report", "work_order")
                .icon("print")
                .tooltip("Prepare to registers actual item quantities or time used in connection with the sales order."),
        );
    }
    if sales && matches!(doc_type, "Order" | "Invoice" | "Quote") {
        v.push(
            Act::command("Pro Forma Invoice", A::Report, "document.print")
                .param("report", "pro_forma")
                .icon("print")
                .tooltip("View or print the pro forma sales invoice."),
        );
    }
    if sales {
        v.push(Act::page("Customer", A::Navigate, "customer-card").link("No.", "Sell-to Customer No.").icon("customer"));
    } else {
        v.push(Act::page("Vendor", A::Navigate, "vendor-card").link("No.", "Buy-from Vendor No.").icon("vendor"));
    }
    v
}

trait Actions {
    fn actions(self, a: Vec<Act>) -> Self;
}

impl Actions for PageDef {
    fn actions(mut self, a: Vec<Act>) -> Self {
        self.actions.extend(a);
        self
    }
}

fn sales_documents(m: &mut Metadata) {
    for (doc_type, list_id, list_obj, list_caption, card_id, card_obj, card_caption) in SALES_DOC_PAGES {
        let date_col = if *doc_type == "Quote" { "Document Date" } else { "Posting Date" };
        m.add_page(
            list(
                list_id,
                *list_obj,
                list_caption,
                SALES_HEADER,
                Some(card_id),
                &[
                    "No.",
                    "Sell-to Customer No.",
                    "Sell-to Customer Name",
                    "External Document No.",
                    date_col,
                    "Due Date",
                    "Status",
                    "Amount",
                    "Amount Including VAT",
                ],
            )
            .view("Document Type", doc_type)
            .factbox(FactBoxDef::new(
                "Customer Details",
                Some("Sell-to Customer No."),
                &["No.", "Name", "Phone No.", "E-Mail", "Balance (LCY)", "Credit Limit (LCY)"],
            ))
            .actions(doc_actions(true, doc_type))
            .keywords(&format!("sales {doc_type} document")),
        );
        let mut general = vec![
            "Sell-to Customer No.",
            "Sell-to Customer Name",
            "Sell-to Address",
            "Sell-to Address 2",
            "Sell-to City",
            "Sell-to Post Code",
            "Sell-to Country/Region Code",
            "Sell-to Contact",
            "Sell-to Phone No.",
            "Sell-to E-Mail",
            "Document Date",
            "Posting Date",
        ];
        if *doc_type == "Order" {
            general.extend(["Order Date", "Requested Delivery Date", "Promised Delivery Date"]);
        }
        if *doc_type == "Quote" {
            general.retain(|f| *f != "Posting Date");
        }
        general.extend(["Due Date", "External Document No.", "Your Reference", "Salesperson Code", "Status", "Work Description"]);
        if *doc_type == "Order" {
            general.insert(2, "Quote No.");
        }
        m.add_page(
            PageDef::new(card_id, *card_obj, card_caption, K::Document, Some(SALES_HEADER))
                .view("Document Type", doc_type)
                .group(G::new("General", &general).promoted(&["Sell-to Customer No.", "Sell-to Customer Name", "Posting Date", "Due Date", "Status"]))
                .part("Lines", "sales-lines", &[("Document Type", "Document Type"), ("Document No.", "No.")])
                .group(
                    G::new(
                        "Invoice Details",
                        &[
                            "Bill-to Customer No.",
                            "Bill-to Name",
                            "Currency Code",
                            "Prices Including VAT",
                            "Gen. Bus. Posting Group",
                            "VAT Bus. Posting Group",
                            "Customer Posting Group",
                            "Payment Terms Code",
                            "Payment Method Code",
                            "VAT Registration No.",
                            "Posting Description",
                        ],
                    )
                    .promoted(&["Currency Code", "Payment Terms Code"])
                    .collapsed(),
                )
                .group(G::new("Shipping and Billing", &["Shipment Date", "Assigned User ID"]).collapsed())
                .factbox(FactBoxDef::new(
                    "Customer Statistics",
                    Some("Sell-to Customer No."),
                    &["No.", "Name", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)", "Credit Limit (LCY)"],
                ))
                .actions(doc_actions(true, doc_type)),
        );
    }
    m.add_page(
        PageDef::new("sales-lines", 46, "Lines", K::ListPart, Some(SALES_LINE)).columns(&[
            "Type",
            "No.",
            "Description",
            "Quantity",
            "Unit of Measure Code",
            "Unit Price",
            "Line Discount %",
            "Line Amount",
            "Qty. to Ship",
            "Quantity Shipped",
            "Qty. to Invoice",
            "Quantity Invoiced",
        ]),
    );
}

fn purchase_documents(m: &mut Metadata) {
    for (doc_type, list_id, list_obj, list_caption, card_id, card_obj, card_caption) in PURCH_DOC_PAGES {
        m.add_page(
            list(
                list_id,
                *list_obj,
                list_caption,
                PURCH_HEADER,
                Some(card_id),
                &[
                    "No.",
                    "Buy-from Vendor No.",
                    "Buy-from Vendor Name",
                    "Vendor Invoice No.",
                    "Posting Date",
                    "Due Date",
                    "Status",
                    "Amount",
                    "Amount Including VAT",
                ],
            )
            .view("Document Type", doc_type)
            .factbox(FactBoxDef::new(
                "Vendor Details",
                Some("Buy-from Vendor No."),
                &["No.", "Name", "Phone No.", "E-Mail", "Balance (LCY)"],
            ))
            .actions(doc_actions(false, doc_type))
            .keywords(&format!("purchase {doc_type} document buy")),
        );
        let vendor_doc_no = if *doc_type == "Credit Memo" { "Vendor Cr. Memo No." } else { "Vendor Invoice No." };
        let mut general = vec![
            "Buy-from Vendor No.",
            "Buy-from Vendor Name",
            "Buy-from Address",
            "Buy-from Address 2",
            "Buy-from City",
            "Buy-from Post Code",
            "Buy-from Country/Region Code",
            "Buy-from Contact",
            "Document Date",
            "Posting Date",
            "Due Date",
            vendor_doc_no,
            "Purchaser Code",
            "Status",
        ];
        if *doc_type == "Order" {
            general.extend(["Order Date", "Expected Receipt Date", "Vendor Order No.", "Quote No."]);
        }
        m.add_page(
            PageDef::new(card_id, *card_obj, card_caption, K::Document, Some(PURCH_HEADER))
                .view("Document Type", doc_type)
                .group(G::new("General", &general).promoted(&["Buy-from Vendor No.", "Buy-from Vendor Name", "Posting Date", "Status"]))
                .part("Lines", "purchase-lines", &[("Document Type", "Document Type"), ("Document No.", "No.")])
                .group(
                    G::new(
                        "Invoice Details",
                        &[
                            "Pay-to Vendor No.",
                            "Pay-to Name",
                            "Currency Code",
                            "Prices Including VAT",
                            "Gen. Bus. Posting Group",
                            "VAT Bus. Posting Group",
                            "Vendor Posting Group",
                            "Payment Terms Code",
                            "Payment Method Code",
                            "Posting Description",
                        ],
                    )
                    .promoted(&["Currency Code", "Payment Terms Code"])
                    .collapsed(),
                )
                .factbox(FactBoxDef::new(
                    "Vendor Statistics",
                    Some("Buy-from Vendor No."),
                    &["No.", "Name", "Balance (LCY)", "Balance Due (LCY)", "Outstanding Orders (LCY)"],
                ))
                .actions(doc_actions(false, doc_type)),
        );
    }
    m.add_page(
        PageDef::new("purchase-lines", 54, "Lines", K::ListPart, Some(PURCH_LINE)).columns(&[
            "Type",
            "No.",
            "Description",
            "Quantity",
            "Unit of Measure Code",
            "Direct Unit Cost",
            "Line Discount %",
            "Line Amount",
            "Qty. to Receive",
            "Quantity Received",
            "Qty. to Invoice",
            "Quantity Invoiced",
        ]),
    );
}

#[allow(clippy::too_many_arguments)]
fn posted_pair(
    m: &mut Metadata,
    (list_id, list_obj, list_caption): (&str, u32, &str),
    (card_id, card_obj, card_caption): (&str, u32, &str),
    (lines_id, lines_obj): (&str, u32),
    header: &str,
    line: &str,
    sales: bool,
) {
    let (party_no, party_name, ext) = if sales {
        ("Sell-to Customer No.", "Sell-to Customer Name", "External Document No.")
    } else {
        ("Buy-from Vendor No.", "Buy-from Vendor Name", "Vendor Invoice No.")
    };
    let ledger_page = if sales { "customer-ledger-entries" } else { "vendor-ledger-entries" };
    let party_page = if sales { "customer-card" } else { "vendor-card" };
    m.add_page(
        list(
            list_id,
            list_obj,
            list_caption,
            header,
            Some(card_id),
            &["No.", party_no, party_name, "Posting Date", "Due Date", "Amount", "Amount Including VAT", "Remaining Amount", "Closed", ext, "Order No."],
        )
        .action(Act::page("Ledger Entries", A::Navigate, ledger_page).link("Document No.", "No."))
        .action(Act::page("G/L Entries", A::Navigate, "general-ledger-entries").link("Document No.", "No."))
        .action(Act::command("Print...", A::Report, "document.print").icon("print"))
        .action(Act::command("Statistics", A::Process, "document.posted_statistics").shortcut("F7").icon("stats"))
        .keywords("posted history"),
    );
    m.add_page(
        PageDef::new(card_id, card_obj, card_caption, K::Document, Some(header))
            .read_only()
            .group(
                G::new(
                    "General",
                    &["No.", party_no, party_name, "Posting Date", "Document Date", "Due Date", "Order No.", "Pre-Assigned No.", ext, "Posting Description"],
                )
                .promoted(&[party_no, party_name, "Posting Date"]),
            )
            .part("Lines", lines_id, &[("Document No.", "No.")])
            .group(
                G::new("Invoice Details", &["Currency Code", "Prices Including VAT", "Payment Terms Code", "Amount", "Amount Including VAT", "Remaining Amount", "Closed"])
                    .collapsed(),
            )
            .action(Act::page("Ledger Entries", A::Navigate, ledger_page).link("Document No.", "No."))
            .action(Act::page("G/L Entries", A::Navigate, "general-ledger-entries").link("Document No.", "No."))
            .action(Act::page(if sales { "Customer" } else { "Vendor" }, A::Navigate, party_page).link("No.", party_no))
            .action(Act::command("Print...", A::Home, "document.print").shortcut("Ctrl+P").icon("print").tooltip("Print the document to PDF.")),
    );
    let price = if sales { "Unit Price" } else { "Direct Unit Cost" };
    m.add_page(
        PageDef::new(lines_id, lines_obj, "Lines", K::ListPart, Some(line))
            .read_only()
            .columns(&["Type", "No.", "Description", "Quantity", "Unit of Measure Code", price, "Line Discount %", "Line Amount", "Amount Including VAT"]),
    );
}

fn posted_documents(m: &mut Metadata) {
    posted_pair(
        m,
        ("posted-sales-invoices", 143, "Posted Sales Invoices"),
        ("posted-sales-invoice", 132, "Posted Sales Invoice"),
        ("posted-sales-invoice-lines", 133),
        SALES_INV_HEADER,
        SALES_INV_LINE,
        true,
    );
    posted_pair(
        m,
        ("posted-sales-credit-memos", 144, "Posted Sales Credit Memos"),
        ("posted-sales-credit-memo", 134, "Posted Sales Credit Memo"),
        ("posted-sales-cr-memo-lines", 135),
        SALES_CR_HEADER,
        SALES_CR_LINE,
        true,
    );
    posted_pair(
        m,
        ("posted-purchase-invoices", 146, "Posted Purchase Invoices"),
        ("posted-purchase-invoice", 138, "Posted Purchase Invoice"),
        ("posted-purchase-invoice-lines", 139),
        PURCH_INV_HEADER,
        PURCH_INV_LINE,
        false,
    );
    posted_pair(
        m,
        ("posted-purchase-credit-memos", 147, "Posted Purchase Credit Memos"),
        ("posted-purchase-credit-memo", 140, "Posted Purchase Credit Memo"),
        ("posted-purchase-cr-memo-lines", 141),
        PURCH_CR_HEADER,
        PURCH_CR_LINE,
        false,
    );
}

fn ledgers(m: &mut Metadata) {
    m.add_page(ledger(
        "customer-ledger-entries",
        25,
        "Customer Ledger Entries",
        CUST_LEDGER,
        &["Posting Date", "Document Type", "Document No.", "Customer No.", "Customer Name", "Description", "Amount", "Remaining Amount", "Due Date", "Open", "Entry No."],
    ));
    m.add_page(ledger(
        "vendor-ledger-entries",
        29,
        "Vendor Ledger Entries",
        VEND_LEDGER,
        &["Posting Date", "Document Type", "Document No.", "External Document No.", "Vendor No.", "Vendor Name", "Description", "Amount", "Remaining Amount", "Due Date", "Open", "Entry No."],
    ));
    m.add_page(ledger(
        "general-ledger-entries",
        20,
        "General Ledger Entries",
        GL_ENTRY,
        &["Posting Date", "Document Type", "Document No.", "G/L Account No.", "G/L Account Name", "Description", "Amount", "Debit Amount", "Credit Amount", "Source Type", "Source No.", "Entry No."],
    ));
    m.add_page(ledger(
        "item-ledger-entries",
        38,
        "Item Ledger Entries",
        ITEM_LEDGER,
        &["Posting Date", "Entry Type", "Document Type", "Document No.", "Item No.", "Description", "Quantity", "Remaining Quantity", "Sales Amount (Actual)", "Cost Amount (Actual)", "Entry No."],
    ));
    m.add_page(ledger(
        "bank-account-ledger-entries",
        372,
        "Bank Account Ledger Entries",
        BANK_LEDGER,
        &["Posting Date", "Document Type", "Document No.", "Bank Account No.", "Description", "Amount", "Bal. Account Type", "Bal. Account No.", "Open", "Entry No."],
    ));
}

fn setup(m: &mut Metadata) {
    let simple: &[(&str, u32, &str, &str, &[&str])] = &[
        ("payment-terms", 4, "Payment Terms", "Payment Terms", &["Code", "Due Date Calculation", "Discount Date Calculation", "Discount %", "Description"]),
        ("currencies", 5, "Currencies", "Currency", &["Code", "Description", "Symbol"]),
        ("salespersons", 14, "Salespeople/Purchasers", "Salesperson/Purchaser", &["Code", "Name", "E-Mail", "Phone No.", "Commission %"]),
        ("units-of-measure", 209, "Units of Measure", "Unit of Measure", &["Code", "Description"]),
        ("item-categories", 5730, "Item Categories", "Item Category", &["Code", "Description"]),
        ("gen-bus-posting-groups", 312, "Gen. Business Posting Groups", "Gen. Business Posting Group", &["Code", "Description"]),
        ("gen-prod-posting-groups", 313, "Gen. Product Posting Groups", "Gen. Product Posting Group", &["Code", "Description"]),
        ("vat-bus-posting-groups", 470, "VAT Business Posting Groups", "VAT Business Posting Group", &["Code", "Description"]),
        ("vat-prod-posting-groups", 471, "VAT Product Posting Groups", "VAT Product Posting Group", &["Code", "Description"]),
        ("inventory-posting-groups", 112, "Inventory Posting Groups", "Inventory Posting Group", &["Code", "Description"]),
        ("customer-posting-groups", 110, "Customer Posting Groups", "Customer Posting Group", &["Code", "Description", "Receivables Account"]),
        ("vendor-posting-groups", 111, "Vendor Posting Groups", "Vendor Posting Group", &["Code", "Description", "Payables Account"]),
        ("bank-posting-groups", 373, "Bank Account Posting Groups", "Bank Account Posting Group", &["Code", "Description", "G/L Account No."]),
        (
            "general-posting-setup",
            314,
            "General Posting Setup",
            "General Posting Setup",
            &["Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "Sales Account", "Sales Credit Memo Account", "Purch. Account", "Purch. Credit Memo Account", "COGS Account"],
        ),
        (
            "vat-posting-setup",
            472,
            "VAT Posting Setup",
            "VAT Posting Setup",
            &["VAT Bus. Posting Group", "VAT Prod. Posting Group", "VAT Calculation Type", "VAT %", "Sales VAT Account", "Purchase VAT Account", "Description"],
        ),
        (
            "inventory-posting-setup",
            5826,
            "Inventory Posting Setup",
            "Inventory Posting Setup",
            &["Location Code", "Invt. Posting Group Code", "Inventory Account", "Description"],
        ),
        ("no-series", 456, "No. Series", "No. Series", &["Code", "Description", "Starting No.", "Ending No.", "Last No. Used", "Increment-by No.", "Default Nos.", "Manual Nos."]),
        ("users", 9800, "Users", "User", &["User Name", "Full Name", "State", "Contact Email"]),
        ("permission-sets", 9802, "Permission Sets", "Permission Set", &["Role ID", "Name"]),
        (
            "permissions",
            9803,
            "Permissions",
            "Permission",
            &["Role ID", "Object Type", "Object Name", "Read Permission", "Insert Permission", "Modify Permission", "Delete Permission", "Execute Permission"],
        ),
        ("user-permission-sets", 9831, "User Permission Sets", "Access Control", &["User Name", "Role ID"]),
        ("user-personalization", 9173, "User Personalization", "User Personalization", &["User Name", "Profile ID", "Work Date"]),
    ];
    for (id, obj, caption, table, cols) in simple {
        m.add_page(list(id, *obj, caption, table, None, cols).keywords("setup"));
    }
    let cards: &[(&str, u32, &str, &str, &[(&str, &[&str])])] = &[
        (
            "company-information",
            1,
            "Company Information",
            "Company Information",
            &[
                ("General", &["Name", "Name 2", "Address", "Address 2", "City", "Post Code", "Country/Region Code", "VAT Registration No."]),
                ("Communication", &["Phone No.", "E-Mail", "Home Page"]),
                ("Payments", &["Bank Name", "Bank Branch No.", "Bank Account No.", "IBAN", "SWIFT Code"]),
            ],
        ),
        (
            "general-ledger-setup",
            118,
            "General Ledger Setup",
            "General Ledger Setup",
            &[("General", &["Allow Posting From", "Allow Posting To", "LCY Code", "Local Currency Description", "Amount Rounding Precision", "Payment Nos."])],
        ),
        (
            "sales-receivables-setup",
            459,
            "Sales & Receivables Setup",
            "Sales & Receivables Setup",
            &[
                ("General", &["Credit Warnings", "Stockout Warning", "Ext. Doc. No. Mandatory"]),
                (
                    "Number Series",
                    &["Customer Nos.", "Quote Nos.", "Order Nos.", "Invoice Nos.", "Credit Memo Nos.", "Return Order Nos.", "Posted Invoice Nos.", "Posted Credit Memo Nos."],
                ),
            ],
        ),
        (
            "purchases-payables-setup",
            460,
            "Purchases & Payables Setup",
            "Purchases & Payables Setup",
            &[
                ("General", &["Ext. Doc. No. Mandatory"]),
                (
                    "Number Series",
                    &["Vendor Nos.", "Quote Nos.", "Order Nos.", "Invoice Nos.", "Credit Memo Nos.", "Return Order Nos.", "Posted Invoice Nos.", "Posted Credit Memo Nos."],
                ),
            ],
        ),
        ("inventory-setup", 461, "Inventory Setup", "Inventory Setup", &[("Numbering", &["Item Nos."])]),
    ];
    for (id, obj, caption, table, groups) in cards {
        let mut p = PageDef::new(id, *obj, caption, K::Card, Some(table)).keywords("setup");
        for (g, fields) in *groups {
            p = p.group(G::new(g, fields));
        }
        m.add_page(p);
    }
}
