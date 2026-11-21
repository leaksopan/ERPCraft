//! Item tracking: 6502 Item Tracking Code (pages 6502 / 6512), the item's tracking fields, the
//! lot / serial numbers of a document or journal line as 337 Reservation Entry ("Item Tracking
//! Lines"), the tracking on item ledger entries ("Item Tracking Entries", 6501).

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const TRACKING_CODE: &str = "Item Tracking Code";
pub const RESERVATION_ENTRY: &str = "Reservation Entry";
pub const LOT_INFO: &str = "Lot No. Information";
pub const SERIAL_INFO: &str = "Serial No. Information";

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
    ensure(
        m,
        crate::schema::tables::ITEM,
        vec![F::code("Item Tracking Code", 10).relation(TRACKING_CODE), F::code("Serial Nos.", 20).relation("No. Series"), F::code("Lot Nos.", 20).relation("No. Series"), F::code("Expiration Calculation", 20)],
    );
    ensure(
        m,
        TRACKING_CODE,
        vec![F::boolean("SN Info. Inbound Must Exist"), F::boolean("SN Info. Outbound Must Exist"), F::boolean("Lot Info. Inbound Must Exist"), F::boolean("Lot Info. Outbound Must Exist")],
    );
    // Lot / serial number information cards (6505 / 6504, pages 6505 / 6508, 6504 / 6509).
    for (table, id, no_field, list_id, list_obj, list_caption, card_id, card_obj, card_caption) in [
        (LOT_INFO, 6505, "Lot No.", "lot-no-information-list", 6508, "Lot No. Information List", "lot-no-information-card", 6505, "Lot No. Information Card"),
        (SERIAL_INFO, 6504, "Serial No.", "serial-no-information-list", 6509, "Serial No. Information List", "serial-no-information-card", 6504, "Serial No. Information Card"),
    ] {
        m.add_table(
            TableDef::new(table, id)
                .fields(vec![
                    F::code("Item No.", 20).not_blank().relation(crate::schema::tables::ITEM),
                    F::code("Variant Code", 10),
                    F::code(no_field, 50).not_blank(),
                    F::text("Description", 100),
                    F::option("Test Quality", &["", "Good", "Average", "Bad"]),
                    F::code("Certificate Number", 20),
                    F::boolean("Blocked"),
                    F::decimal("Inventory").flow(),
                    F::decimal("Expired Inventory").flow(),
                ])
                .key(&["Item No.", "Variant Code", no_field])
                .describe_by("Description")
                .pages(list_id, Some(card_id)),
        );
        let entries = Act::page("Item &Tracking Entries", A::Navigate, "item-tracking-entries").link("Item No.", "Item No.").link(no_field, no_field).icon("list").tooltip("View serial, lot or package numbers that are assigned to items.");
        let mut l = PageDef::new(list_id, list_obj, list_caption, PageKind::List, Some(table));
        l.actions = vec![entries.clone()];
        l.columns = ["Item No.", "Variant Code", no_field, "Description", "Test Quality", "Certificate Number", "Blocked", "Inventory", "Expired Inventory"].iter().map(|c| (*c).to_string()).collect();
        l.card_page = Some(card_id.to_string());
        l.keywords = "lot serial batch tracking information".to_string();
        m.add_page(l);
        m.add_page(
            PageDef::new(card_id, card_obj, card_caption, PageKind::Card, Some(table))
                .group(G::new("General", &["Item No.", "Variant Code", no_field, "Description", "Test Quality", "Certificate Number", "Blocked"]))
                .group(G::new("Inventory", &["Inventory", "Expired Inventory"]))
                .action(Act::page("&Item Tracing", A::Navigate, "item-tracking-entries").link("Item No.", "Item No.").link(no_field, no_field).icon("list").tooltip("Trace where a lot or serial number assigned to the item was used, for example, to find which lot a defective component came from or to find all the customers that have received items containing the defective component."))
                .action(entries)
                .action(
                    Act::command("Copy &Info", A::Process, "tracking.copy_info")
                        .icon("copy")
                        .ask(&["new_no"])
                        .tooltip(if table == LOT_INFO { "Copy the information record from the old lot number." } else { "Copy the information record from the old serial number." }),
                ),
        );
    }
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
    if let Some(p) = m.pages.get_mut("item-charge-assignment-purch") {
        p.actions.push(
            Act::command("Get &Receipt Lines", A::Process, "purchase.charge.get_receipt_lines")
                .icon("list")
                .ask(&["document_no"])
                .tooltip("Select a posted purchase receipt for the item that you want to assign the item charge to, for example, if you received an invoice for the item charge after you posted the original purchase receipt."),
        );
    }
    if let Some(p) = m.pages.get_mut("purchase-lines") {
        p.actions.push(Act::command("Item Charge &Assignment", A::Navigate, "purchase.charge.assignment").icon("list").shortcut("Alt+I").tooltip("Record additional direct costs, for example for freight. This action is available only for Charge (Item) line types."));
    }
    // The sales side (5809, page 5814).
    m.add_table(
        TableDef::new(crate::item_charge::ASSIGNMENT_SALES, 5809)
            .fields(vec![
                F::option("Document Type", crate::schema::tables::SALES_DOC_TYPES),
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
                F::option("Applies-to Doc. Type", &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", "Shipment", "Return Receipt"]),
                F::code("Applies-to Doc. No.", 20),
                F::integer("Applies-to Doc. Line No."),
            ])
            .key(&["Document Type", "Document No.", "Document Line No.", "Line No."]),
    );
    let mut sassign = PageDef::new("item-charge-assignment-sales", 5814, "Item Charge Assignment (Sales)", PageKind::List, Some(crate::item_charge::ASSIGNMENT_SALES));
    sassign.columns = ["Applies-to Doc. Type", "Applies-to Doc. No.", "Applies-to Doc. Line No.", "Item No.", "Description", "Qty. to Assign", "Qty. Assigned", "Amount to Assign"].iter().map(|c| (*c).to_string()).collect();
    sassign.editable = true;
    sassign.actions = vec![
        Act::command("Get &Shipment Lines", A::Process, "sales.charge.get_shipment_lines").icon("list").ask(&["document_no"]).tooltip("Select multiple shipments to the same customer because you want to combine them on one invoice."),
        Act::command("Suggest Item &Charge Assignment", A::Process, "sales.charge.suggest")
            .icon("apply")
            .ask(&["method"])
            .tooltip("Use a function that assigns and distributes the item charge when the document has more than one line of type Item. You can select between four distribution methods. "),
    ];
    m.add_page(sassign);
    if let Some(p) = m.pages.get_mut("sales-lines") {
        p.actions.push(Act::command("Item Charge &Assignment", A::Navigate, "sales.charge.assignment").icon("list").shortcut("Alt+I").tooltip("Record additional direct costs, for example for freight. This action is available only for Charge (Item) line types."));
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
    // BC's Reservation page (498): the line to reserve for over its supply, one summary line per
    // kind of supply (BC's temporary Entry Summary, here kept per demand line).
    m.add_table(
        TableDef::new(crate::reservation::REQUEST, 0)
            .fields(vec![
                F::integer("Source Type"),
                F::integer("Source Subtype"),
                F::code("Source ID", 20),
                F::integer("Source Ref. No."),
                F::code("Item No.", 20).read_only(),
                F::code("Variant Code", 10).read_only(),
                F::code("Location Code", 10).read_only(),
                F::date("Shipment Date").read_only(),
                F::text("Description", 100).read_only(),
                F::decimal("Quantity to Reserve").read_only(),
                F::decimal("Reserved Quantity").read_only(),
                F::decimal("Unreserved Quantity").read_only(),
                F::text("Caption", 250).read_only(),
            ])
            .key(&["Source Type", "Source Subtype", "Source ID", "Source Ref. No."])
            .describe_by("Description"),
    );
    m.add_table(
        TableDef::new(crate::reservation::ENTRY_SUMMARY, 338)
            .fields(vec![
                F::integer("Source Type"),
                F::integer("Source Subtype"),
                F::code("Source ID", 20),
                F::integer("Source Ref. No."),
                F::integer("Entry No."),
                F::integer("Table ID"),
                F::text("Summary Type", 80).read_only(),
                F::decimal("Total Quantity").read_only(),
                F::decimal("Total Reserved Quantity").read_only(),
                F::decimal("Qty. Alloc. in Warehouse").read_only().caption("Qty. Allocated in Warehouse"),
                F::decimal("Res. Qty. on Picks & Shipmts.").read_only().caption("Reserved Qty. on Picks and Shipments"),
                F::decimal("Total Available Quantity").read_only(),
                F::decimal("Non-specific Reserved Qty.").read_only(),
                F::decimal("Current Reserved Quantity").read_only(),
            ])
            .key(&["Source Type", "Source Subtype", "Source ID", "Source Ref. No.", "Entry No."]),
    );
    let mut summary = PageDef::new("reservation", 498, "Reservation", PageKind::ListPart, Some(crate::reservation::ENTRY_SUMMARY));
    summary.columns = ["Summary Type", "Total Quantity", "Total Reserved Quantity", "Qty. Alloc. in Warehouse", "Total Available Quantity", "Current Reserved Quantity"].iter().map(|c| (*c).to_string()).collect();
    summary.actions = vec![
        Act::command("&Available to Reserve", A::Navigate, "reservation.available").icon("list").tooltip("View all the item ledger entries and other supply that is available to reserve for the line."),
        Act::command("&Reservation Entries", A::Navigate, "reservation.entries_of_request").icon("list").tooltip("View all reservations that are made for the item, either manually or automatically."),
        Act::command("Reserve from Current Line", A::Process, "reservation.reserve_from_line").icon("apply").tooltip("Reserve the quantity that is available from the current summary line."),
        Act::command("&Auto Reserve", A::Process, "reservation.auto_reserve").icon("apply").tooltip("Automatically reserve the first available quantity for the item on the line."),
        Act::command("Cancel Reservation from Current Line", A::Process, "reservation.cancel_from_line").icon("delete").confirm("Do you want to cancel the reservations from the current summary line?").tooltip("Cancel the selected reservation entry."),
    ];
    m.add_page(summary);
    m.add_page(
        PageDef::new("reservation-card", 0, "Reservation", PageKind::Document, Some(crate::reservation::REQUEST))
            .read_only()
            .group(G::new("General", &["Item No.", "Shipment Date", "Description", "Quantity to Reserve", "Reserved Quantity", "Unreserved Quantity"]))
            .part("Lines", "reservation", &[("Source Type", "Source Type"), ("Source Subtype", "Source Subtype"), ("Source ID", "Source ID"), ("Source Ref. No.", "Source Ref. No.")])
            .group(G::new("Filters", &["Variant Code", "Location Code"]).collapsed())
            .action(Act::command("Reserve from Current Line", A::Home, "reservation.reserve_from_line").icon("apply").tooltip("Reserve the quantity that is available from the current summary line."))
            .action(Act::command("&Auto Reserve", A::Home, "reservation.auto_reserve").icon("apply").tooltip("Automatically reserve the first available quantity for the item on the line."))
            .action(Act::command("Cancel Reservation from Current Line", A::Home, "reservation.cancel_from_line").icon("delete").tooltip("Cancel the selected reservation entry.")),
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
        p.actions.push(Act::command("&Reserve...", A::Process, "reservation.reserve").icon("apply").tooltip("Reserve the quantity of the selected item that is required on the document line from which you opened this page."));
        p.actions.push(Act::command("Reserve from &Inventory", A::Process, "reservation.reserve_from_inventory").icon("apply").tooltip("Reserve items for the selected line from inventory."));
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
            p.actions.push(Act::page("Lot No. Information", A::Navigate, "lot-no-information-list").link("Item No.", "No.").icon("list").tooltip("View lot number information for the item."));
            p.actions.push(Act::page("Serial No. Information", A::Navigate, "serial-no-information-list").link("Item No.", "No.").icon("list").tooltip("View serial number information for the item."));
        }
    }
}
