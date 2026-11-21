//! Item tracking: 6502 Item Tracking Code (pages 6502 / 6512), the item's tracking fields, the
//! lot / serial numbers of a document or journal line as 337 Reservation Entry ("Item Tracking
//! Lines"), the tracking on item ledger entries ("Item Tracking Entries", 6501).

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const TRACKING_CODE: &str = "Item Tracking Code";
pub const RESERVATION_ENTRY: &str = "Reservation Entry";

fn ensure(m: &mut Metadata, table: &str, fields: Vec<F>) {
    if let Some(t) = m.tables.get_mut(table) {
        for f in fields {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(TRACKING_CODE, 6502)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::text("Description", 100),
                F::boolean("SN Specific Tracking"),
                F::boolean("Lot Specific Tracking"),
                F::boolean("Use Expiration Dates"),
                F::boolean("Man. Expir. Date Entry Reqd."),
                F::boolean("Strict Expiration Posting"),
                F::boolean("SN Warehouse Tracking"),
                F::boolean("Lot Warehouse Tracking"),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("item-tracking-codes", Some("item-tracking-code-card")),
    );
    m.add_table(
        TableDef::new(RESERVATION_ENTRY, 337)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Item No.", 20).relation(crate::schema::tables::ITEM),
                F::code("Location Code", 10),
                F::decimal("Quantity (Base)"),
                F::decimal("Qty. to Handle (Base)"),
                F::option("Reservation Status", &["Reservation", "Tracking", "Surplus", "Prospect"]),
                F::text("Description", 100),
                F::date("Creation Date"),
                F::integer("Source Type"),
                F::integer("Source Subtype"),
                F::code("Source ID", 20),
                F::code("Source Batch Name", 10),
                F::integer("Source Ref. No."),
                F::text("Serial No.", 50),
                F::code("Lot No.", 50),
                F::date("Expiration Date"),
                F::boolean("Positive"),
            ])
            .key(&["Entry No."]),
    );
    ensure(m, crate::schema::tables::ITEM, vec![F::code("Item Tracking Code", 10).relation(TRACKING_CODE), F::code("Serial Nos.", 20).relation("No. Series"), F::code("Lot Nos.", 20).relation("No. Series")]);
    ensure(m, crate::schema::tables::ITEM_LEDGER, vec![F::text("Serial No.", 50), F::code("Lot No.", 50), F::date("Expiration Date"), F::option("Item Tracking", &["None", "Lot and Serial No.", "Lot No.", "Serial No."])]);

    // Costing: value entries, cost posting setup, the costing batch jobs on items.
    m.add_table(
        TableDef::new(crate::costing::VALUE_ENTRY, 5802)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Item No.", 20).relation(crate::schema::tables::ITEM),
                F::date("Posting Date"),
                F::option("Item Ledger Entry Type", &["Purchase", "Sale", "Positive Adjmt.", "Negative Adjmt.", "Transfer", "Consumption", "Output"]),
                F::code("Source No.", 20),
                F::code("Document No.", 20),
                F::text("Description", 100),
                F::code("Location Code", 10),
                F::code("Inventory Posting Group", 20),
                F::integer("Item Ledger Entry No."),
                F::decimal("Valued Quantity"),
                F::decimal("Item Ledger Entry Quantity"),
                F::decimal("Invoiced Quantity"),
                F::decimal("Cost per Unit"),
                F::decimal("Sales Amount (Actual)").auto_format(1),
                F::decimal("Cost Amount (Actual)").auto_format(1),
                F::decimal("Cost Posted to G/L").auto_format(1),
                F::option("Source Type", &["", "Customer", "Vendor", "Item"]),
                F::code("Gen. Bus. Posting Group", 20),
                F::code("Gen. Prod. Posting Group", 20),
                F::option("Document Type", &["", "Sales Shipment", "Sales Invoice", "Sales Return Receipt", "Sales Credit Memo", "Purchase Receipt", "Purchase Invoice", "Purchase Return Shipment", "Purchase Credit Memo", "Transfer Shipment", "Transfer Receipt"]),
                F::option("Entry Type", &["Direct Cost", "Revaluation", "Rounding", "Indirect Cost", "Variance"]),
                F::boolean("Adjustment"),
                F::code("Item Charge No.", 20),
                F::date("Valuation Date"),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .pages("value-entries", None),
    );
    ensure(m, crate::schema::tables::INVENTORY_SETUP, vec![F::boolean("Automatic Cost Posting"), F::option("Automatic Cost Adjustment", &["Never", "Day", "Week", "Month", "Quarter", "Year", "Always"])]);
    ensure(m, crate::schema::tables::ITEM, vec![F::decimal("Standard Cost")]);
    let mut values = PageDef::new("value-entries", 5802, "Value Entries", PageKind::List, Some(crate::costing::VALUE_ENTRY));
    values.columns = ["Posting Date", "Valuation Date", "Item Ledger Entry Type", "Entry Type", "Adjustment", "Document No.", "Item No.", "Location Code", "Valued Quantity", "Item Ledger Entry Quantity", "Cost per Unit", "Sales Amount (Actual)", "Cost Amount (Actual)", "Cost Posted to G/L", "Item Ledger Entry No.", "Entry No."]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    m.add_page(values);
    for page in ["item-card", "item-list"] {
        if let Some(p) = m.pages.get_mut(page) {
            p.actions.push(Act::page("&Value Entries", A::Navigate, "value-entries").link("Item No.", "No.").icon("ledger").tooltip("View the history of posted amounts that affect the value of the item. Value entries are created for every transaction with the item."));
        }
    }
    if let Some(p) = m.pages.get_mut("item-list") {
        p.actions.push(Act::command("Adjust Cost - Item Entries", A::Process, "inventory.adjust_cost").icon("adjust").confirm("Do you want to adjust the cost of every item?").tooltip("Adjust inventory values in value entries so that you use the correct adjusted cost for updating the general ledger and so that sales and profit statistics are up to date."));
        p.actions.push(Act::command("Post Inventory Cost to G/L", A::Process, "inventory.post_cost_to_gl").icon("post").ask(&["posting_date"]).tooltip("Post the quantity and value changes to the inventory in the item ledger entries and the value entries to the inventory accounts in the general ledger."));
    }
    if let Some(p) = m.pages.get_mut("item-ledger-entries") {
        p.actions.push(Act::page("&Value Entries", A::Navigate, "value-entries").link("Item Ledger Entry No.", "Entry No.").icon("ledger").shortcut("Ctrl+F7").tooltip("View the history of posted amounts that affect the value of the item. Value entries are created for every transaction with the item."));
    }
    let mut codes = PageDef::new("item-tracking-codes", 6502, "Item Tracking Codes", PageKind::List, Some(TRACKING_CODE));
    codes.columns = ["Code", "Description"].iter().map(|c| (*c).to_string()).collect();
    codes.card_page = Some("item-tracking-code-card".to_string());
    m.add_page(codes);
    m.add_page(
        PageDef::new("item-tracking-code-card", 6512, "Item Tracking Code Card", PageKind::Card, Some(TRACKING_CODE))
            .group(G::new("General", &["Code", "Description"]))
            .group(G::new("Serial No.", &["SN Specific Tracking", "SN Warehouse Tracking"]))
            .group(G::new("Lot No.", &["Lot Specific Tracking", "Lot Warehouse Tracking"]))
            .group(G::new("Misc.", &["Use Expiration Dates", "Man. Expir. Date Entry Reqd.", "Strict Expiration Posting"])),
    );
    let mut lines = PageDef::new("item-tracking-lines", 0, "Item Tracking Lines", PageKind::List, Some(RESERVATION_ENTRY));
    lines.columns = ["Serial No.", "Lot No.", "Expiration Date", "Quantity (Base)", "Qty. to Handle (Base)", "Item No.", "Location Code"].iter().map(|c| (*c).to_string()).collect();
    lines.editable = true;
    m.add_page(lines);
    let mut entries = PageDef::new("item-tracking-entries", 6501, "Item Tracking Entries", PageKind::List, Some(crate::schema::tables::ITEM_LEDGER));
    entries.columns = ["Posting Date", "Entry Type", "Document No.", "Item No.", "Location Code", "Serial No.", "Lot No.", "Expiration Date", "Quantity", "Remaining Quantity", "Entry No."].iter().map(|c| (*c).to_string()).collect();
    m.add_page(entries);

    // Lines that carry item tracking: the Line menu / journals get Item Tracking Lines.
    let track = Act::command("Item &Tracking Lines", A::Navigate, "tracking.lines")
        .icon("list")
        .shortcut("Ctrl+Alt+I")
        .tooltip("View or edit serial and lot numbers that are assigned to the item on the document or journal line.");
    let pages: Vec<String> = m
        .pages
        .values()
        .filter(|p| matches!(p.source_table.as_deref(), Some("Sales Line" | "Purchase Line" | "Item Journal Line" | "Transfer Line")))
        .map(|p| p.id.clone())
        .collect();
    for id in pages {
        if let Some(p) = m.pages.get_mut(&id) {
            p.actions.push(track.clone());
        }
    }
    for page in ["item-card", "item-list"] {
        if let Some(p) = m.pages.get_mut(page) {
            p.actions.push(Act::page("Item &Tracking Entries", A::Navigate, "item-tracking-entries").link("Item No.", "No.").icon("list").tooltip("View serial or lot numbers that are assigned to items."));
        }
    }
}
