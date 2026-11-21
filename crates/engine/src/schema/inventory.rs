//! Phase 3, inventory: locations on master data, documents and item ledger entries, and item
//! journals (Item Journal 40, Item Reclass. Journal 393, Phys. Inventory Journal 392).

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, NavLink as L, PageDef, PageKind as K, TableDef};

use super::tables::{
    CUSTOMER, GEN_POSTING_SETUP, INVENTORY_SETUP, ITEM, NO_SERIES, PURCH_HEADER, PURCH_LINE, SALES_HEADER, SALES_LINE, VENDOR,
};

pub const LOCATION: &str = "Location";
pub const ITEM_JNL_TEMPLATE: &str = "Item Journal Template";
pub const ITEM_JNL_BATCH: &str = "Item Journal Batch";
pub const ITEM_JNL_LINE: &str = "Item Journal Line";

pub const ITEM_ENTRY_TYPES: &[&str] = &["Purchase", "Sale", "Positive Adjmt.", "Negative Adjmt.", "Transfer"];
pub const ITEM_TEMPLATE_TYPES: &[&str] = &["Item", "Transfer", "Phys. Inventory"];

/// (template, type, page id, page object id, caption, number series, source code).
pub const ITEM_JOURNALS: &[(&str, &str, &str, u32, &str, &str, &str)] = &[
    ("ITEM", "Item", "item-journal", 40, "Item Journals", "I-JNL", "ITEMJNL"),
    ("RECLASS", "Transfer", "item-reclass-journal", 393, "Item Reclass. Journals", "I-RCLSS", "RECLASSJNL"),
    ("PHYS INV", "Phys. Inventory", "phys-inventory-journal", 392, "Phys. Inventory Journals", "I-PHYS", "PHYSINVJNL"),
];

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(LOCATION, 14)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::text("Name", 100),
                F::text("Address", 100),
                F::text("City", 30),
                F::code("Post Code", 20),
                F::text("Contact", 100),
                F::text("Phone No.", 30),
            ])
            .key(&["Code"])
            .describe_by("Name")
            .pages("locations", None),
    );
    m.add_table(
        TableDef::new(ITEM_JNL_TEMPLATE, 82)
            .fields(vec![
                F::code("Name", 10).not_blank(),
                F::text("Description", 80),
                F::option("Type", ITEM_TEMPLATE_TYPES),
                F::code("No. Series", 20).relation(NO_SERIES),
                F::code("Source Code", 10),
            ])
            .key(&["Name"])
            .describe_by("Description")
            .pages("item-journal-templates", None),
    );
    m.add_table(
        TableDef::new(ITEM_JNL_BATCH, 233)
            .fields(vec![
                F::code("Journal Template Name", 10).relation(ITEM_JNL_TEMPLATE),
                F::code("Name", 10),
                F::text("Description", 100),
                F::code("No. Series", 20).relation(NO_SERIES),
            ])
            .key(&["Journal Template Name", "Name"])
            .describe_by("Description"),
    );
    m.add_table(
        TableDef::new(ITEM_JNL_LINE, 83)
            .fields(vec![
                F::code("Journal Template Name", 10).relation(ITEM_JNL_TEMPLATE),
                F::code("Journal Batch Name", 10),
                F::integer("Line No."),
                F::date("Posting Date"),
                F::option("Entry Type", ITEM_ENTRY_TYPES),
                F::code("Document No.", 20),
                F::code("Item No.", 20).relation(ITEM),
                F::text("Description", 100),
                F::code("Location Code", 10).relation(LOCATION),
                F::code("New Location Code", 10).relation(LOCATION),
                F::code("Unit of Measure Code", 10),
                F::decimal("Quantity"),
                F::decimal("Unit Amount"),
                F::decimal("Amount").read_only(),
                F::decimal("Unit Cost"),
                F::boolean("Phys. Inventory").read_only(),
                F::decimal("Qty. (Calculated)").read_only(),
                F::decimal("Qty. (Phys. Inventory)"),
                F::code("Gen. Bus. Posting Group", 20),
                F::code("Gen. Prod. Posting Group", 20),
                F::code("Inventory Posting Group", 20),
                F::code("Source Code", 10).read_only(),
            ])
            .key(&["Journal Template Name", "Journal Batch Name", "Line No."])
            .describe_by("Description"),
    );

    // Locations on master data, documents and lines; adjustment account; negative stock guard.
    for (table, fields) in [
        (CUSTOMER, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (VENDOR, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (SALES_HEADER, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (PURCH_HEADER, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (SALES_LINE, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (PURCH_LINE, vec![F::code("Location Code", 10).relation(LOCATION)]),
        (GEN_POSTING_SETUP, vec![F::code("Inventory Adjmt. Account", 20).relation(super::tables::GL_ACCOUNT)]),
        (INVENTORY_SETUP, vec![F::boolean("Prevent Negative Inventory"), F::boolean("Location Mandatory")]),
    ] {
        if let Some(t) = m.tables.get_mut(table) {
            t.fields.extend(fields);
        }
    }
    pages(m);
    navigation(m);
}

fn pages(m: &mut Metadata) {
    m.add_page(
        PageDef::new("locations", 15, "Locations", K::List, Some(LOCATION))
            .columns(&["Code", "Name", "Address", "City", "Contact", "Phone No."])
            .keywords("warehouse gudang location"),
    );
    m.add_page(
        PageDef::new("item-journal-templates", 102, "Item Journal Templates", K::List, Some(ITEM_JNL_TEMPLATE))
            .columns(&["Name", "Description", "Type", "No. Series", "Source Code"])
            .keywords("setup journal"),
    );
    for (template, ty, id, obj, caption, _, _) in ITEM_JOURNALS {
        let cols: &[&str] = match *ty {
            "Transfer" => &["Posting Date", "Document No.", "Item No.", "Description", "Location Code", "New Location Code", "Quantity", "Unit of Measure Code"],
            "Phys. Inventory" => &[
                "Posting Date",
                "Document No.",
                "Item No.",
                "Description",
                "Location Code",
                "Qty. (Calculated)",
                "Qty. (Phys. Inventory)",
                "Quantity",
                "Entry Type",
                "Unit Amount",
                "Amount",
            ],
            _ => &["Posting Date", "Entry Type", "Document No.", "Item No.", "Description", "Location Code", "Quantity", "Unit of Measure Code", "Unit Amount", "Amount"],
        };
        let mut p = PageDef::new(id, *obj, caption, K::List, Some(ITEM_JNL_LINE))
            .columns(cols)
            .view("Journal Template Name", template)
            .view("Journal Batch Name", "DEFAULT")
            .action(
                Act::command("Post and &Print", A::Home, "item_journal.post_and_print")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .icon("print")
                        .shortcut("Shift+F9")
                        .confirm("Do you want to post the journal lines and print the report?")
                        .tooltip("Finalize and prepare to print the document or journal. The values and quantities are posted to the related accounts."),
                )
                .action(
                    Act::command("Preview Posting", A::Process, "item_journal.preview_post")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .icon("preview")
                        .tooltip("Review the different types of entries that will be created when you post the document or journal."),
                )
                .action(
                    Act::command("Test Report", A::Report, "item_journal.test_report")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .icon("report")
                        .tooltip("View a test report so that you can find and correct any errors before you perform the actual posting of the journal or document."),
                )
                .action(
                    Act::command("Renumber Document Numbers", A::Process, "item_journal.renumber")
                        .param("template", template)
                        .param("batch", "DEFAULT")
                        .icon("adjust")
                        .tooltip("Resort the numbers in the Document No. column to avoid posting errors because the document numbers are not in sequence."),
                )
                .action(
                    Act::command("Post", A::Home, "item_journal.post")
                    .param("template", template)
                    .param("batch", "DEFAULT")
                    .shortcut("F9")
                    .icon("post")
                    .confirm("Do you want to post the journal lines?"),
            )
            .keywords("inventory stock adjustment opname");
        if *ty == "Phys. Inventory" {
            p = p.action(
                Act::command("Calculate Inventory", A::Process, "item_journal.calculate_inventory")
                    .param("template", template)
                    .param("batch", "DEFAULT")
                    .ask(&["location_code"])
                    .icon("stats")
                    .tooltip("Fill the journal with the current inventory per item and location, for counting."),
            );
        }
        m.add_page(p);
    }
    // Location on documents and lines.
    for (doc, lines) in [("sales-order", "sales-lines"), ("purchase-order", "purchase-lines")] {
        for id in [doc] {
            if let Some(p) = m.pages.get_mut(id)
                && let Some(g) = p.groups.iter_mut().find(|g| g.caption == "General")
            {
                g.fields.push("Location Code".to_string());
            }
        }
        if let Some(p) = m.pages.get_mut(lines) {
            let at = p.columns.iter().position(|c| c == "Quantity").unwrap_or(p.columns.len());
            p.columns.insert(at, "Location Code".to_string());
        }
    }
    for id in ["sales-invoice", "purchase-invoice", "sales-credit-memo", "purchase-credit-memo", "sales-quote", "purchase-quote"] {
        if let Some(g) = m.pages.get_mut(id).and_then(|p| p.groups.iter_mut().find(|g| g.caption == "General")) {
            g.fields.push("Location Code".to_string());
        }
    }
    for id in ["customer-card", "vendor-card"] {
        if let Some(g) = m.pages.get_mut(id).and_then(|p| p.groups.iter_mut().find(|g| g.caption == "Invoicing")) {
            g.fields.push("Location Code".to_string());
        }
    }
    if let Some(p) = m.pages.get_mut("item-ledger-entries") {
        let at = p.columns.iter().position(|c| c == "Quantity").unwrap_or(p.columns.len());
        p.columns.insert(at, "Location Code".to_string());
    }
    if let Some(g) = m.pages.get_mut("inventory-setup").and_then(|p| p.groups.first_mut()) {
        g.fields.extend(["Prevent Negative Inventory".to_string(), "Location Mandatory".to_string()]);
    }
    if let Some(p) = m.pages.get_mut("general-posting-setup") {
        p.columns.push("Inventory Adjmt. Account".to_string());
    }
    if let Some(p) = m.pages.remove("item-list") {
        m.add_page(
            p.action(Act::command("Inventory Valuation", A::Report, "report.inventory_valuation"))
                .action(Act::page("Item Journal", A::Navigate, "item-journal")),
        );
    }
}

fn navigation(m: &mut Metadata) {
    let Some(rc) = m.role_centers.get_mut(super::role_center::BUSINESS_MANAGER) else { return };
    if let Some(s) = rc.sections.iter_mut().find(|s| s.caption == "Inventory") {
        s.links.insert(1, L::new("Item Journals", "item-journal"));
        s.links.insert(2, L::new("Item Reclass. Journals", "item-reclass-journal"));
        s.links.insert(3, L::new("Phys. Inventory Journals", "phys-inventory-journal"));
        s.links.push(L::new("Locations", "locations"));
        s.links.push(L::new("Item Journal Templates", "item-journal-templates"));
    }
    if let Some(g) = rc.action_groups.iter_mut().find(|g| g.caption == "Reports") {
        g.links.push(L::command("Inventory Valuation", "report.inventory_valuation"));
    }
}
