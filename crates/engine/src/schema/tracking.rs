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
                F::integer("Applies-to Entry"),
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
    // Planning: the item's reordering fields and the requisition worksheet.
    ensure(
        m,
        crate::schema::tables::ITEM,
        vec![
            F::option("Reordering Policy", &["", "Fixed Reorder Qty.", "Maximum Qty.", "Order", "Lot-for-Lot"]),
            F::decimal("Reorder Point"),
            F::decimal("Reorder Quantity"),
            F::decimal("Maximum Inventory"),
            F::decimal("Safety Stock Quantity"),
            F::decimal("Minimum Order Quantity"),
            F::decimal("Order Multiple"),
            F::code("Lead Time Calculation", 20),
            F::option("Replenishment System", &["Purchase", "Prod. Order", "", "Assembly", "Transfer"]),
        ],
    );
    m.add_table(
        TableDef::new(crate::reqwksh::REQ_TEMPLATE, 244)
            .fields(vec![F::code("Name", 10).not_blank(), F::text("Description", 80), F::option("Type", &["Req.", "For. Labor", "Planning"]), F::boolean("Recurring")])
            .key(&["Name"])
            .describe_by("Description")
            .pages("req-worksheet-templates", None),
    );
    m.add_table(
        TableDef::new(crate::reqwksh::REQ_NAME, 245)
            .fields(vec![F::code("Worksheet Template Name", 10).relation(crate::reqwksh::REQ_TEMPLATE), F::code("Name", 10), F::text("Description", 100)])
            .key(&["Worksheet Template Name", "Name"]),
    );
    m.add_table(
        TableDef::new(crate::reqwksh::REQ_LINE, 246)
            .fields(vec![
                F::code("Worksheet Template Name", 10),
                F::code("Journal Batch Name", 10),
                F::integer("Line No."),
                F::option("Type", &["", "G/L Account", "Item"]),
                F::code("No.", 20),
                F::text("Description", 100),
                F::decimal("Quantity"),
                F::decimal("Original Quantity").read_only(),
                F::code("Vendor No.", 20).relation(crate::schema::tables::VENDOR),
                F::decimal("Direct Unit Cost"),
                F::date("Due Date"),
                F::date("Order Date"),
                F::code("Location Code", 10),
                F::code("Unit of Measure Code", 10),
                F::decimal("Qty. per Unit of Measure"),
                F::decimal("Quantity (Base)"),
                F::option("Action Message", &["", "New", "Change Qty.", "Reschedule", "Resched. & Chg. Qty.", "Cancel"]),
                F::boolean("Accept Action Message"),
                F::option("Replenishment System", &["Purchase", "Prod. Order", "", "Assembly", "Transfer"]),
            ])
            .key(&["Worksheet Template Name", "Journal Batch Name", "Line No."]),
    );
    let mut templates = PageDef::new("req-worksheet-templates", 293, "Req. Worksheet Templates", PageKind::List, Some(crate::reqwksh::REQ_TEMPLATE));
    templates.columns = ["Name", "Description", "Recurring"].iter().map(|c| (*c).to_string()).collect();
    templates.editable = true;
    m.add_page(templates);
    let mut wksh = PageDef::new("req-worksheet", 291, "Requisition Worksheets", PageKind::List, Some(crate::reqwksh::REQ_LINE))
        .view("Worksheet Template Name", "REQ")
        .view("Journal Batch Name", "DEFAULT");
    wksh.columns = ["Type", "No.", "Action Message", "Accept Action Message", "Description", "Location Code", "Original Quantity", "Quantity", "Unit of Measure Code", "Direct Unit Cost", "Order Date", "Due Date", "Vendor No."].iter().map(|c| (*c).to_string()).collect();
    wksh.editable = true;
    wksh.keywords = "requisition reorder planning purchase suggestion".to_string();
    wksh.actions = vec![
        Act::command("Calculate Plan", A::Process, "reqwksh.calculate_plan").icon("new").ask(&["order_date"]).tooltip("Use a batch job to help you calculate a supply plan for items and stockkeeping units that have the Replenishment System field set to Purchase or Transfer."),
        Act::command("Carry &Out Action Message", A::Process, "reqwksh.carry_out").icon("post").confirm("Do you want to carry out the action messages and create purchase orders?").tooltip("Use a batch job to help you create actual supply orders from the order proposals."),
    ];
    m.add_page(wksh);
    // Item charges and their purchase assignment.
    m.add_table(
        TableDef::new(crate::item_charge::ITEM_CHARGE, 5800)
            .fields(vec![F::code("No.", 20).not_blank(), F::text("Description", 100), F::code("Gen. Prod. Posting Group", 20), F::code("VAT Prod. Posting Group", 20), F::code("Search Description", 100)])
            .key(&["No."])
            .describe_by("Description")
            .pages("item-charges", None),
    );
    m.add_table(
        TableDef::new(crate::item_charge::ASSIGNMENT, 5805)
            .fields(vec![
                F::option("Document Type", crate::schema::tables::PURCH_DOC_TYPES),
                F::code("Document No.", 20),
                F::integer("Document Line No."),
                F::integer("Line No."),
                F::code("Item Charge No.", 20),
                F::code("Item No.", 20),
                F::text("Description", 100),
                F::decimal("Qty. to Assign"),
                F::decimal("Qty. Assigned").read_only(),
                F::decimal("Unit Cost"),
                F::decimal("Amount to Assign").auto_format(1).read_only(),
                F::option("Applies-to Doc. Type", crate::item_charge::APPLIES_TO_TYPES),
                F::code("Applies-to Doc. No.", 20),
                F::integer("Applies-to Doc. Line No."),
            ])
            .key(&["Document Type", "Document No.", "Document Line No.", "Line No."]),
    );
    let mut charges = PageDef::new("item-charges", 5800, "Item Charges", PageKind::List, Some(crate::item_charge::ITEM_CHARGE));
    charges.columns = ["No.", "Description", "Gen. Prod. Posting Group", "VAT Prod. Posting Group", "Search Description"].iter().map(|c| (*c).to_string()).collect();
    charges.editable = true;
    charges.keywords = "freight item charge landed cost".to_string();
    m.add_page(charges);
    let mut assign = PageDef::new("item-charge-assignment-purch", 5805, "Item Charge Assignment (Purch)", PageKind::List, Some(crate::item_charge::ASSIGNMENT));
    assign.columns = ["Applies-to Doc. Type", "Applies-to Doc. No.", "Applies-to Doc. Line No.", "Item No.", "Description", "Qty. to Assign", "Qty. Assigned", "Amount to Assign"].iter().map(|c| (*c).to_string()).collect();
    assign.editable = true;
    assign.actions = vec![
        Act::command("Suggest Item &Charge Assignment", A::Process, "purchase.charge.suggest").icon("apply").ask(&["method"]).tooltip("Use a function that assigns and distributes the item charge when the document has more than one line of type Item."),
    ];
    m.add_page(assign);
    if let Some(p) = m.pages.get_mut("purchase-lines") {
        p.actions.push(Act::command("Item Charge &Assignment", A::Navigate, "purchase.charge.assignment").icon("list").shortcut("Alt+I").tooltip("Record additional direct costs, for example for freight. This action is available only for Charge (Item) line types."));
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
    // Reservations: the entries' dates, the reserved quantities, the Reservation Entries page.
    ensure(m, RESERVATION_ENTRY, vec![F::date("Shipment Date"), F::date("Expected Receipt Date"), F::code("Created By", 50)]);
    for line in [crate::schema::tables::SALES_LINE, crate::schema::tables::PURCH_LINE] {
        ensure(m, line, vec![F::decimal("Reserved Quantity").flow(), F::decimal("Reserved Qty. (Base)").flow()]);
    }
    ensure(
        m,
        crate::schema::tables::ITEM,
        vec![F::decimal("Reserved Qty. on Inventory").flow(), F::decimal("Reserved Qty. on Sales Orders").flow(), F::decimal("Reserved Qty. on Purch. Orders").flow()],
    );
    let mut res = PageDef::new("reservation-entries", 497, "Reservation Entries", PageKind::List, Some(RESERVATION_ENTRY)).view("Reservation Status", "Reservation");
    res.columns = ["Reservation Status", "Item No.", "Location Code", "Quantity (Base)", "Positive", "Source Type", "Source Subtype", "Source ID", "Source Ref. No.", "Shipment Date", "Expected Receipt Date", "Description", "Creation Date", "Created By", "Entry No."]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    res.keywords = "reservation reserved".to_string();
    res.actions = vec![Act::command("Cancel Reservation", A::Process, "reservation.cancel").icon("delete").confirm("Do you want to cancel the reservation of this line?").tooltip("Cancel the selected reservation entry.")];
    m.add_page(res);
    if let Some(p) = m.pages.get_mut("sales-lines") {
        p.actions.push(Act::command("&Reserve", A::Process, "reservation.reserve").icon("apply").tooltip("Reserve the quantity of the selected item that is required on the document line from which you opened this page."));
        p.actions.push(Act::command("Cancel Reservation", A::Process, "reservation.cancel").icon("delete").confirm("Do you want to cancel the reservations of this line?").tooltip("Cancel the reservations of the line."));
        p.actions.push(Act::command("Reservation Entries", A::Navigate, "reservation.entries").icon("list").tooltip("View the entries for every reservation that is made, either manually or automatically."));
    }
    if let Some(p) = m.pages.get_mut("purchase-lines") {
        p.actions.push(Act::command("Reservation Entries", A::Navigate, "reservation.entries").icon("list").tooltip("View the entries for every reservation that is made, either manually or automatically."));
    }
    if let Some(p) = m.pages.get_mut("sales-order") {
        p.actions.push(Act::command("Order &Promising", A::Process, "sales.order_promising").icon("calendar").tooltip("Calculate the shipment and delivery dates based on the item's known and expected availability dates, and then promise the dates to the customer."));
    }
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
