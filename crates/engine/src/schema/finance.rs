//! Phase 2 of the base application: general journals, detailed ledger entries (applications),
//! G/L registers and transactions, posted shipments and receipts. Names and object ids follow BC
//! (Gen. Journal Line = 81, Detailed Cust. Ledg. Entry = 379, G/L Register = 45, Sales Shipment
//! Header = 110, …).

use erpcraft_meta::{
    ActionArea as A, ActionDef as Act, FactBoxDef, FieldDef as F, GroupDef as G, Metadata, NavLink as L, PageDef, PageKind as K,
    TableDef,
};

use super::tables::{
    BANK_LEDGER, CUST_LEDGER, CUSTOMER, GL_ENTRY, LEDGER_DOC_TYPES, LINE_TYPES, NO_SERIES, PURCH_LINE,
    SALES_LINE, VEND_LEDGER, VENDOR,
};

pub const GEN_JNL_TEMPLATE: &str = "Gen. Journal Template";
pub const GEN_JNL_BATCH: &str = "Gen. Journal Batch";
pub const GEN_JNL_LINE: &str = "Gen. Journal Line";
pub const DTLD_CUST_LEDGER: &str = "Detailed Cust. Ledg. Entry";
pub const DTLD_VEND_LEDGER: &str = "Detailed Vendor Ledg. Entry";
pub const GL_REGISTER: &str = "G/L Register";
pub const SALES_SHIPMENT_HEADER: &str = "Sales Shipment Header";
pub const SALES_SHIPMENT_LINE: &str = "Sales Shipment Line";
pub const PURCH_RCPT_HEADER: &str = "Purch. Rcpt. Header";
pub const PURCH_RCPT_LINE: &str = "Purch. Rcpt. Line";

pub const ACCOUNT_TYPES: &[&str] = &["G/L Account", "Customer", "Vendor", "Bank Account"];
pub const TEMPLATE_TYPES: &[&str] = &["General", "Sales", "Purchases", "Cash Receipts", "Payments"];
pub const DTLD_ENTRY_TYPES: &[&str] = &["", "Initial Entry", "Application"];

/// The standard journals: (template, type, page id, page object id, caption, number series).
pub const JOURNALS: &[(&str, &str, &str, u32, &str, &str)] = &[
    ("GENERAL", "General", "general-journal", 39, "General Journals", "G-JNL"),
    ("CASHRCPT", "Cash Receipts", "cash-receipt-journal", 255, "Cash Receipt Journals", "G-RCPT"),
    ("PAYMENT", "Payments", "payment-journal", 256, "Payment Journals", "G-PMT"),
];

pub fn register(m: &mut Metadata) {
    tables(m);
    extend_ledgers(m);
    pages(m);
    navigation(m);
}

fn tables(m: &mut Metadata) {
    m.add_table(
        TableDef::new(GEN_JNL_TEMPLATE, 80)
            .fields(vec![
                F::code("Name", 10).not_blank(),
                F::text("Description", 80),
                F::option("Type", TEMPLATE_TYPES),
                F::code("No. Series", 20).relation(NO_SERIES),
                F::code("Source Code", 10),
            ])
            .key(&["Name"])
            .describe_by("Description")
            .pages("general-journal-templates", None),
    );
    m.add_table(
        TableDef::new(GEN_JNL_BATCH, 232)
            .fields(vec![
                F::code("Journal Template Name", 10).relation(GEN_JNL_TEMPLATE),
                F::code("Name", 10),
                F::text("Description", 100),
                F::option("Bal. Account Type", ACCOUNT_TYPES),
                F::code("Bal. Account No.", 20),
                F::code("No. Series", 20).relation(NO_SERIES),
            ])
            .key(&["Journal Template Name", "Name"])
            .describe_by("Description")
            .pages("general-journal-batches", None),
    );
    m.add_table(
        TableDef::new(GEN_JNL_LINE, 81)
            .fields(vec![
                F::code("Journal Template Name", 10).relation(GEN_JNL_TEMPLATE),
                F::code("Journal Batch Name", 10),
                F::integer("Line No."),
                F::date("Posting Date"),
                F::option("Document Type", LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::code("External Document No.", 35),
                F::option("Account Type", ACCOUNT_TYPES),
                F::code("Account No.", 20),
                F::text("Description", 100),
                F::decimal("Debit Amount"),
                F::decimal("Credit Amount"),
                F::decimal("Amount"),
                F::option("Bal. Account Type", ACCOUNT_TYPES),
                F::code("Bal. Account No.", 20),
                F::option("Applies-to Doc. Type", LEDGER_DOC_TYPES),
                F::code("Applies-to Doc. No.", 20),
                F::date("Due Date"),
                F::code("Source Code", 10).read_only(),
            ])
            .key(&["Journal Template Name", "Journal Batch Name", "Line No."])
            .describe_by("Description"),
    );
    for (name, id, entry, party, party_table) in [
        (DTLD_CUST_LEDGER, 379, "Cust. Ledger Entry No.", "Customer No.", CUSTOMER),
        (DTLD_VEND_LEDGER, 380, "Vendor Ledger Entry No.", "Vendor No.", VENDOR),
    ] {
        let applied = if party_table == CUSTOMER { "Applied Cust. Ledger Entry No." } else { "Applied Vend. Ledger Entry No." };
        m.add_table(
            TableDef::new(name, id)
                .fields(vec![
                    F::integer("Entry No."),
                    F::integer(entry),
                    F::option("Entry Type", DTLD_ENTRY_TYPES),
                    F::date("Posting Date"),
                    F::option("Document Type", LEDGER_DOC_TYPES),
                    F::code("Document No.", 20),
                    F::decimal("Amount"),
                    F::decimal("Amount (LCY)"),
                    F::decimal("Debit Amount"),
                    F::decimal("Credit Amount"),
                    F::code(party, 20).relation(party_table),
                    F::integer("Transaction No."),
                    F::integer("Application No."),
                    F::integer(applied),
                    F::boolean("Unapplied"),
                    F::integer("Unapplied by Entry No."),
                    F::date("Initial Entry Due Date"),
                    F::code("User ID", 50),
                ])
                .key(&["Entry No."])
                .pages(if party_table == CUSTOMER { "detailed-cust-ledger-entries" } else { "detailed-vendor-ledger-entries" }, None),
        );
    }
    m.add_table(
        TableDef::new(GL_REGISTER, 45)
            .fields(vec![
                F::integer("No."),
                F::integer("From Entry No."),
                F::integer("To Entry No."),
                F::date("Creation Date"),
                F::code("Source Code", 10),
                F::code("User ID", 50),
                F::code("Journal Batch Name", 10),
                F::code("Journal Templ. Name", 10),
                F::integer("Transaction No."),
                F::boolean("Reversed"),
            ])
            .key(&["No."])
            .pages("gl-registers", None),
    );
    for (header, hid, line, lid, sales) in [
        (SALES_SHIPMENT_HEADER, 110, SALES_SHIPMENT_LINE, 111, true),
        (PURCH_RCPT_HEADER, 120, PURCH_RCPT_LINE, 121, false),
    ] {
        let (party_no, party_name, party_table, list, card) = if sales {
            ("Sell-to Customer No.", "Sell-to Customer Name", CUSTOMER, "posted-sales-shipments", "posted-sales-shipment")
        } else {
            ("Buy-from Vendor No.", "Buy-from Vendor Name", VENDOR, "posted-purchase-receipts", "posted-purchase-receipt")
        };
        m.add_table(
            TableDef::new(header, hid)
                .fields(vec![
                    F::code("No.", 20),
                    F::code(party_no, 20).relation(party_table),
                    F::text(party_name, 100),
                    F::date("Posting Date"),
                    F::date("Document Date"),
                    F::code("Order No.", 20),
                    F::code(if sales { "External Document No." } else { "Vendor Shipment No." }, 35),
                    F::code(if sales { "Salesperson Code" } else { "Purchaser Code" }, 20),
                    F::text("Posting Description", 100),
                    F::code("User ID", 50),
                ])
                .key(&["No."])
                .describe_by(party_name)
                .pages(list, Some(card)),
        );
        let handled_not_invoiced = if sales { "Qty. Shipped Not Invoiced" } else { "Qty. Rcd. Not Invoiced" };
        m.add_table(
            TableDef::new(line, lid)
                .fields(vec![
                    F::code("Document No.", 20),
                    F::integer("Line No."),
                    F::option("Type", LINE_TYPES),
                    F::code("No.", 20),
                    F::text("Description", 100),
                    F::code("Unit of Measure Code", 10),
                    F::decimal("Quantity"),
                    F::decimal("Quantity Invoiced"),
                    F::decimal(handled_not_invoiced),
                    F::code("Order No.", 20),
                    F::integer("Order Line No."),
                    F::integer("Item Ledger Entry No."),
                ])
                .key(&["Document No.", "Line No."])
                .describe_by("Description"),
        );
    }
}

/// Transaction numbers, reversal and source codes on the ledgers; shipped / received but not yet
/// invoiced quantities on document lines.
fn extend_ledgers(m: &mut Metadata) {
    for name in [GL_ENTRY, CUST_LEDGER, VEND_LEDGER, BANK_LEDGER] {
        if let Some(t) = m.tables.get_mut(name) {
            t.fields.extend([
                F::integer("Transaction No."),
                F::code("Source Code", 10),
                F::boolean("Reversed"),
                F::integer("Reversed by Entry No."),
                F::integer("Reversed Entry No."),
            ]);
        }
    }
    for name in [CUST_LEDGER, VEND_LEDGER] {
        if let Some(t) = m.tables.get_mut(name) {
            t.fields.push(F::code("Journal Batch Name", 10));
        }
    }
    for (setup, field) in [(super::tables::SALES_SETUP, "Posted Shipment Nos."), (super::tables::PURCH_SETUP, "Posted Receipt Nos.")] {
        if let Some(t) = m.tables.get_mut(setup) {
            t.fields.push(F::code(field, 20).relation(NO_SERIES));
        }
    }
    if let Some(t) = m.tables.get_mut(SALES_LINE) {
        t.fields.push(F::decimal("Qty. Shipped Not Invoiced").read_only());
    }
    if let Some(t) = m.tables.get_mut(PURCH_LINE) {
        t.fields.push(F::decimal("Qty. Rcd. Not Invoiced").read_only());
    }
}

fn pages(m: &mut Metadata) {
    for (template, _, id, obj, caption, _) in JOURNALS {
        m.add_page(
            PageDef::new(id, *obj, caption, K::List, Some(GEN_JNL_LINE))
                .columns(&[
                    "Posting Date",
                    "Document Type",
                    "Document No.",
                    "External Document No.",
                    "Account Type",
                    "Account No.",
                    "Description",
                    "Debit Amount",
                    "Credit Amount",
                    "Bal. Account Type",
                    "Bal. Account No.",
                    "Applies-to Doc. Type",
                    "Applies-to Doc. No.",
                ])
                .view("Journal Template Name", template)
                .view("Journal Batch Name", "DEFAULT")
                .action(
                    Act::command("Post", A::Home, "journal.post")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .shortcut("F9")
                        .icon("post")
                        .tooltip("Finalize the journal by posting the amounts to the accounts.")
                        .confirm("Do you want to post the journal lines?"),
                )
                .action(
                    Act::command("Preview Balance", A::Process, "journal.check")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .icon("stats")
                        .tooltip("Check that every document in the journal balances, without posting."),
                )
                .keywords("journal payment receipt gl entry posting"),
        );
    }
    let ledger_actions = |p: PageDef, party: &str| {
        p.action(
            Act::command("Apply Entries", A::Process, &format!("{party}.entries.apply"))
                .icon("payment")
                .tooltip("Apply the selected open entry to other open entries of the same account."),
        )
        .action(
            Act::command("Unapply Entries", A::Process, &format!("{party}.entries.unapply"))
                .icon("reopen")
                .tooltip("Undo the last application of the selected entry.")
                .confirm("Do you want to unapply the entries?"),
        )
        .action(
            Act::command("Reverse Transaction", A::Process, "transaction.reverse")
                .icon("reopen")
                .tooltip("Reverse the journal transaction the selected entry belongs to.")
                .confirm("Do you want to reverse the transaction?"),
        )
    };
    let take = |m: &mut Metadata, id: &str| m.pages.remove(id);
    if let Some(p) = take(m, "customer-ledger-entries") {
        let p = ledger_actions(p, "customer")
            .action(Act::page("Detailed Ledger Entries", A::Navigate, "detailed-cust-ledger-entries").link("Cust. Ledger Entry No.", "Entry No."));
        m.add_page(p);
    }
    if let Some(p) = take(m, "vendor-ledger-entries") {
        let p = ledger_actions(p, "vendor")
            .action(Act::page("Detailed Ledger Entries", A::Navigate, "detailed-vendor-ledger-entries").link("Vendor Ledger Entry No.", "Entry No."));
        m.add_page(p);
    }
    if let Some(p) = take(m, "general-ledger-entries") {
        m.add_page(p.action(
            Act::command("Reverse Transaction", A::Process, "transaction.reverse")
                .icon("reopen")
                .confirm("Do you want to reverse the transaction?"),
        ));
    }
    m.add_page(
        PageDef::new("detailed-cust-ledger-entries", 573, "Detailed Cust. Ledg. Entries", K::List, Some(DTLD_CUST_LEDGER))
            .read_only()
            .columns(&[
                "Posting Date",
                "Entry Type",
                "Document Type",
                "Document No.",
                "Customer No.",
                "Amount",
                "Cust. Ledger Entry No.",
                "Applied Cust. Ledger Entry No.",
                "Unapplied",
                "Transaction No.",
                "Entry No.",
            ])
            .keywords("entries application"),
    );
    m.add_page(
        PageDef::new("detailed-vendor-ledger-entries", 574, "Detailed Vendor Ledg. Entries", K::List, Some(DTLD_VEND_LEDGER))
            .read_only()
            .columns(&[
                "Posting Date",
                "Entry Type",
                "Document Type",
                "Document No.",
                "Vendor No.",
                "Amount",
                "Vendor Ledger Entry No.",
                "Applied Vend. Ledger Entry No.",
                "Unapplied",
                "Transaction No.",
                "Entry No.",
            ])
            .keywords("entries application"),
    );
    m.add_page(
        PageDef::new("gl-registers", 116, "G/L Registers", K::List, Some(GL_REGISTER))
            .read_only()
            .columns(&["No.", "Creation Date", "Source Code", "Journal Batch Name", "From Entry No.", "To Entry No.", "Transaction No.", "Reversed", "User ID"])
            .action(Act::page("General Ledger", A::Navigate, "general-ledger-entries").link("Transaction No.", "Transaction No."))
            .keywords("register audit"),
    );
    m.add_page(
        PageDef::new("general-journal-templates", 101, "General Journal Templates", K::List, Some(GEN_JNL_TEMPLATE))
            .columns(&["Name", "Description", "Type", "No. Series", "Source Code"])
            .keywords("setup journal"),
    );
    m.add_page(
        PageDef::new("general-journal-batches", 251, "General Journal Batches", K::List, Some(GEN_JNL_BATCH))
            .columns(&["Journal Template Name", "Name", "Description", "Bal. Account Type", "Bal. Account No.", "No. Series"])
            .keywords("setup journal"),
    );
    for (sales, list, list_obj, list_caption, card, card_obj, card_caption, lines, lines_obj, header, line) in [
        (
            true,
            "posted-sales-shipments",
            142,
            "Posted Sales Shipments",
            "posted-sales-shipment",
            130,
            "Posted Sales Shipment",
            "posted-sales-shipment-lines",
            131,
            SALES_SHIPMENT_HEADER,
            SALES_SHIPMENT_LINE,
        ),
        (
            false,
            "posted-purchase-receipts",
            145,
            "Posted Purchase Receipts",
            "posted-purchase-receipt",
            136,
            "Posted Purchase Receipt",
            "posted-purchase-receipt-lines",
            137,
            PURCH_RCPT_HEADER,
            PURCH_RCPT_LINE,
        ),
    ] {
        let (no, name, ext, person) = if sales {
            ("Sell-to Customer No.", "Sell-to Customer Name", "External Document No.", "Salesperson Code")
        } else {
            ("Buy-from Vendor No.", "Buy-from Vendor Name", "Vendor Shipment No.", "Purchaser Code")
        };
        let not_invoiced = if sales { "Qty. Shipped Not Invoiced" } else { "Qty. Rcd. Not Invoiced" };
        m.add_page(
            PageDef::new(list, list_obj, list_caption, K::List, Some(header))
                .card(card)
                .read_only()
                .columns(&["No.", no, name, "Posting Date", "Order No.", ext])
                .action(Act::page("Item Ledger Entries", A::Navigate, "item-ledger-entries").link("Document No.", "No."))
                .keywords("posted history shipment receipt"),
        );
        m.add_page(
            PageDef::new(card, card_obj, card_caption, K::Document, Some(header))
                .read_only()
                .group(G::new("General", &["No.", no, name, "Posting Date", "Document Date", "Order No.", ext, person, "Posting Description"]).promoted(&[no, name, "Posting Date"]))
                .part("Lines", lines, &[("Document No.", "No.")])
                .factbox(FactBoxDef::new(if sales { "Customer Details" } else { "Vendor Details" }, Some(no), &["No.", "Name", "Phone No.", "Balance (LCY)"]))
                .action(Act::page("Item Ledger Entries", A::Navigate, "item-ledger-entries").link("Document No.", "No.")),
        );
        m.add_page(
            PageDef::new(lines, lines_obj, "Lines", K::ListPart, Some(line))
                .read_only()
                .columns(&["Type", "No.", "Description", "Quantity", "Unit of Measure Code", "Quantity Invoiced", not_invoiced, "Order No."]),
        );
    }
    for (page, field) in [("sales-receivables-setup", "Posted Shipment Nos."), ("purchases-payables-setup", "Posted Receipt Nos.")] {
        if let Some(g) = m.pages.get_mut(page).and_then(|p| p.groups.iter_mut().find(|g| g.caption == "Number Series")) {
            g.fields.push(field.to_string());
        }
    }
    // Order lines show what is shipped / received but not invoiced.
    for (page, field) in [("sales-lines", "Qty. Shipped Not Invoiced"), ("purchase-lines", "Qty. Rcd. Not Invoiced")] {
        if let Some(p) = m.pages.get_mut(page) {
            p.columns.push(field.to_string());
        }
    }
    // Reports on the lists where BC offers them.
    if let Some(p) = m.pages.remove("chart-of-accounts") {
        m.add_page(
            p.action(Act::command("Balance Sheet", A::Report, "report.balance_sheet"))
                .action(Act::command("Income Statement", A::Report, "report.income_statement")),
        );
    }
    if let Some(p) = m.pages.remove("customer-list") {
        m.add_page(p.action(Act::command("Aged Accounts Receivable", A::Report, "report.aged_receivables")));
    }
    if let Some(p) = m.pages.remove("vendor-list") {
        m.add_page(p.action(Act::command("Aged Accounts Payable", A::Report, "report.aged_payables")));
    }
}

fn navigation(m: &mut Metadata) {
    let Some(rc) = m.role_centers.get_mut(super::role_center::BUSINESS_MANAGER) else { return };
    for s in &mut rc.sections {
        match s.caption.as_str() {
            "Finance" => {
                s.links.insert(1, L::new("General Journals", "general-journal"));
                s.links.push(L::new("G/L Registers", "gl-registers"));
                s.links.push(L::new("General Journal Templates", "general-journal-templates"));
            }
            "Cash Management" => {
                s.links.insert(0, L::new("Cash Receipt Journals", "cash-receipt-journal"));
                s.links.insert(1, L::new("Payment Journals", "payment-journal"));
            }
            "Sales" => s.links.push(L::new("Posted Sales Shipments", "posted-sales-shipments")),
            "Purchasing" => s.links.push(L::new("Posted Purchase Receipts", "posted-purchase-receipts")),
            _ => {}
        }
    }
    for g in &mut rc.action_groups {
        match g.caption.as_str() {
            "Payments" => {
                g.links.insert(0, L::new("Cash Receipt Journal", "cash-receipt-journal"));
                g.links.insert(1, L::new("Payment Journal", "payment-journal"));
            }
            "Reports" => {
                g.links.push(L::command("Balance Sheet", "report.balance_sheet"));
                g.links.push(L::command("Income Statement", "report.income_statement"));
                g.links.push(L::command("Aged Accounts Receivable", "report.aged_receivables"));
                g.links.push(L::command("Aged Accounts Payable", "report.aged_payables"));
            }
            _ => {}
        }
    }
}
