//! Physical inventory orders (BC's Inventory/Counting): 5875 / 5876 Phys. Invt. Order Header /
//! Line, 5877 / 5878 Phys. Invt. Record Header / Line, the posted 5879–5882, 7381 Phys. Invt.
//! Counting Period, with their pages (5875–5890, 7381).

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const ORDER_HEADER: &str = "Phys. Invt. Order Header";
pub const ORDER_LINE: &str = "Phys. Invt. Order Line";
pub const RECORD_HEADER: &str = "Phys. Invt. Record Header";
pub const RECORD_LINE: &str = "Phys. Invt. Record Line";
pub const PSTD_ORDER_HEADER: &str = "Pstd. Phys. Invt. Order Hdr";
pub const PSTD_ORDER_LINE: &str = "Pstd. Phys. Invt. Order Line";
pub const PSTD_RECORD_HEADER: &str = "Pstd. Phys. Invt. Record Hdr";
pub const PSTD_RECORD_LINE: &str = "Pstd. Phys. Invt. Record Line";
pub const COUNTING_PERIOD: &str = "Phys. Invt. Counting Period";

const ITEM: &str = crate::schema::tables::ITEM;
const LOCATION: &str = crate::schema::inventory::LOCATION;
pub const ENTRY_TYPES: &[&str] = &["", "Positive Adjmt.", "Negative Adjmt."];

fn ensure(m: &mut Metadata, table: &str, fields: Vec<F>) {
    if let Some(t) = m.tables.get_mut(table) {
        for f in fields {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
}

fn order_header_fields() -> Vec<F> {
    vec![
        F::code("No.", 20),
        F::text("Description", 100),
        F::date("Order Date"),
        F::date("Posting Date"),
        F::code("Person Responsible", 20),
        F::code("Reason Code", 10),
        F::code("Gen. Bus. Posting Group", 20).relation("Gen. Business Posting Group"),
        F::code("Shortcut Dimension 1 Code", 20),
        F::code("Shortcut Dimension 2 Code", 20),
        F::code("Location Code", 10).relation(LOCATION),
        F::code("Bin Code", 20),
        F::integer("No. Finished Recordings").read_only(),
    ]
}

fn order_line_fields() -> Vec<F> {
    vec![
        F::code("Document No.", 20),
        F::integer("Line No."),
        F::boolean("On Recording Lines").read_only(),
        F::code("Item No.", 20).relation(ITEM),
        F::code("Variant Code", 10),
        F::code("Location Code", 10).relation(LOCATION),
        F::code("Bin Code", 20),
        F::text("Description", 100),
        F::text("Description 2", 50),
        F::text("Unit of Measure", 50),
        F::code("Base Unit of Measure Code", 10).read_only(),
        F::decimal("Qty. Expected (Base)").read_only(),
        F::boolean("Qty. Exp. Calculated").read_only(),
        F::boolean("Use Item Tracking").read_only(),
        F::integer("Last Item Ledger Entry No.").read_only(),
        F::decimal("Unit Amount").auto_format(2),
        F::decimal("Unit Cost").auto_format(2),
        F::integer("No. Finished Rec.-Lines").read_only(),
        F::decimal("Qty. Recorded (Base)").read_only(),
        F::decimal("Quantity (Base)").read_only(),
        F::option("Entry Type", ENTRY_TYPES).read_only(),
        F::decimal("Pos. Qty. (Base)").read_only(),
        F::decimal("Neg. Qty. (Base)").read_only(),
        F::boolean("Without Difference").read_only(),
        F::boolean("Recorded Without Order").read_only(),
        F::code("Shelf No.", 10),
        F::code("Gen. Bus. Posting Group", 20),
        F::code("Gen. Prod. Posting Group", 20),
        F::code("Inventory Posting Group", 20),
        F::code("Item Category Code", 20),
        F::code("Phys Invt Counting Period Code", 10).relation(COUNTING_PERIOD),
        F::option("Phys Invt Counting Period Type", &["", "Item", "SKU"]),
    ]
}

fn record_header_fields() -> Vec<F> {
    vec![
        F::code("Order No.", 20),
        F::integer("Recording No."),
        F::text("Description", 100),
        F::code("Person Responsible", 20),
        F::boolean("Allow Recording Without Order"),
        F::date("Date Recorded"),
        F::code("Person Recorded", 20),
        F::code("Recorded by User ID", 50).read_only(),
        F::code("Location Code", 10).relation(LOCATION),
        F::code("Bin Code", 20),
    ]
}

fn record_line_fields() -> Vec<F> {
    vec![
        F::code("Order No.", 20),
        F::integer("Recording No."),
        F::integer("Line No."),
        F::integer("Order Line No.").read_only(),
        F::boolean("Recorded Without Order").read_only(),
        F::code("Item No.", 20).relation(ITEM),
        F::code("Variant Code", 10),
        F::code("Location Code", 10).relation(LOCATION),
        F::code("Bin Code", 20),
        F::text("Description", 100),
        F::text("Description 2", 50),
        F::code("Unit of Measure Code", 10),
        F::decimal("Quantity"),
        F::decimal("Quantity (Base)").read_only(),
        F::decimal("Qty. per Unit of Measure").read_only(),
        F::boolean("Recorded"),
        F::code("Shelf No.", 10),
        F::date("Date Recorded"),
        F::code("Person Recorded", 20),
        F::code("Recorded by User ID", 50).read_only(),
        F::code("Serial No.", 50),
        F::code("Lot No.", 50),
    ]
}

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], card: &str) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.card_page = Some(card.to_string());
    p
}

fn sub(id: &str, object_id: u32, caption: &str, kind: PageKind, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, kind, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    ensure(
        m,
        crate::schema::tables::INVENTORY_SETUP,
        vec![F::code("Phys. Invt. Order Nos.", 20).relation("No. Series"), F::code("Posted Phys. Invt. Order Nos.", 20).relation("No. Series")],
    );
    ensure(
        m,
        ITEM,
        vec![
            F::code("Phys Invt Counting Period Code", 10).relation(COUNTING_PERIOD),
            F::date("Last Counting Period Update").read_only(),
            F::date("Next Counting Start Date").read_only(),
            F::date("Next Counting End Date").read_only(),
            F::date("Last Phys. Invt. Date").read_only(),
        ],
    );
    m.add_table(
        TableDef::new(COUNTING_PERIOD, 7381)
            .fields(vec![F::code("Code", 10).not_blank(), F::text("Description", 100), F::integer("Count Frequency per Year")])
            .key(&["Code"])
            .describe_by("Description")
            .pages("phys-invt-counting-periods", None),
    );
    let mut h = order_header_fields();
    h.extend([
        F::option("Status", &["Open", "Finished"]).read_only(),
        F::code("No. Series", 20).read_only(),
        F::code("Posting No. Series", 20).relation("No. Series"),
        F::code("Posting No.", 20),
        F::code("Last Posting No.", 20).read_only(),
    ]);
    m.add_table(TableDef::new(ORDER_HEADER, 5875).fields(h).key(&["No."]).describe_by("Description").pages("physical-inventory-orders", Some("physical-inventory-order")));
    m.add_table(TableDef::new(ORDER_LINE, 5876).fields(order_line_fields()).key(&["Document No.", "Line No."]));
    let mut rh = record_header_fields();
    rh.push(F::option("Status", &["Open", "Finished"]).read_only());
    m.add_table(TableDef::new(RECORD_HEADER, 5877).fields(rh).key(&["Order No.", "Recording No."]).describe_by("Description").pages("phys-inventory-recording-list", Some("phys-inventory-recording")));
    m.add_table(TableDef::new(RECORD_LINE, 5878).fields(record_line_fields()).key(&["Order No.", "Recording No.", "Line No."]));
    let mut ph = order_header_fields();
    ph.extend([F::code("Pre-Assigned No.", 20), F::code("No. Series", 20), F::code("User ID", 50)]);
    m.add_table(TableDef::new(PSTD_ORDER_HEADER, 5879).fields(ph).key(&["No."]).describe_by("Description").pages("posted-phys-invt-order-list", Some("posted-phys-invt-order")));
    m.add_table(TableDef::new(PSTD_ORDER_LINE, 5880).fields(order_line_fields()).key(&["Document No.", "Line No."]));
    m.add_table(TableDef::new(PSTD_RECORD_HEADER, 5881).fields(record_header_fields()).key(&["Order No.", "Recording No."]).pages("posted-phys-invt-rec-list", Some("posted-phys-invt-recording")));
    m.add_table(TableDef::new(PSTD_RECORD_LINE, 5882).fields(record_line_fields()).key(&["Order No.", "Recording No.", "Line No."]));

    let mut periods = PageDef::new("phys-invt-counting-periods", 7381, "Phys. Invt. Counting Periods", PageKind::List, Some(COUNTING_PERIOD));
    periods.columns = ["Code", "Description", "Count Frequency per Year"].iter().map(|c| (*c).to_string()).collect();
    periods.editable = true;
    periods.keywords = "cycle counting period".to_string();
    m.add_page(periods);

    // Orders.
    let mut orders = list("physical-inventory-orders", 5876, "Physical Inventory Orders", ORDER_HEADER, &["No.", "Description", "Status", "Order Date", "Posting Date", "No. Finished Recordings", "Location Code", "Person Responsible"], "physical-inventory-order");
    orders.keywords = "physical inventory count stocktake".to_string();
    m.add_page(orders);
    let line_cols = &[
        "Item No.",
        "Variant Code",
        "Location Code",
        "Bin Code",
        "Description",
        "Base Unit of Measure Code",
        "Qty. Expected (Base)",
        "Qty. Exp. Calculated",
        "Qty. Recorded (Base)",
        "No. Finished Rec.-Lines",
        "Entry Type",
        "Quantity (Base)",
        "Unit Amount",
        "Recorded Without Order",
        "Without Difference",
        "Shelf No.",
        "Phys Invt Counting Period Code",
    ];
    m.add_page(sub("physical-inventory-order-subf", 5877, "Lines", PageKind::ListPart, ORDER_LINE, line_cols, true));
    m.add_page(sub("physical-inventory-order-lines", 5878, "Physical Inventory Order Lines", PageKind::List, ORDER_LINE, line_cols, false));
    m.add_page(
        PageDef::new("physical-inventory-order", 5875, "Physical Inventory Order", PageKind::Document, Some(ORDER_HEADER))
            .group(G::new("General", &["No.", "Description", "Status", "Order Date", "Posting Date", "Person Responsible", "Location Code", "Bin Code", "No. Finished Recordings"]))
            .part("Lines", "physical-inventory-order-subf", &[("Document No.", "No.")])
            .group(G::new("Posting", &["Gen. Bus. Posting Group", "Reason Code", "Shortcut Dimension 1 Code", "Shortcut Dimension 2 Code", "Posting No. Series", "Posting No.", "Last Posting No."]).collapsed())
            .action(Act::command("Statistics", A::Navigate, "physinvt.statistics").icon("stats").shortcut("F7").tooltip("View statistical information about the physical inventory order."))
            .action(Act::page("&Recordings", A::Navigate, "phys-inventory-recording-list").link("Order No.", "No.").icon("list").tooltip("View all recordings for the physical inventory order."))
            .action(Act::command("&Calculate Lines", A::Process, "physinvt.calculate_lines").icon("new").ask(&["location_code", "zero_qty"]).tooltip("Add physical inventory lines for items that are currently recorded in inventory."))
            .action(
                Act::command("Calculate Counting &Period", A::Process, "physinvt.calculate_counting_period")
                    .icon("new")
                    .tooltip("Add physical inventory order lines for items and SKUs that are set up with counting periods. See the Phys Invt Counting Period Code field on the item card or SKU card."),
            )
            .action(Act::command("Copy &Document", A::Process, "physinvt.copy_document").icon("copy").ask(&["from_no"]).tooltip("Copy Document."))
            .action(
                Act::command("Calc. &Qty. Expected", A::Process, "physinvt.calc_qty_expected")
                    .icon("refresh")
                    .tooltip("Update the value in the Qty. Expected (Base) field on the lines with any inventory changes made since you created the order. When you have used this function, the Qty. Exp. Calculated check box is selected."),
            )
            .action(
                Act::command("Make New &Recording", A::Process, "physinvt.make_recording")
                    .icon("new")
                    .ask(&["only_lines_not_in_recordings"])
                    .tooltip("Create a physical inventory recording document for the lines on the order. On the request page that appears, you can select to only create recording lines for items that are not already on other recordings for the order."),
            )
            .action(Act::command("Fi&nish", A::Process, "physinvt.finish").icon("release").shortcut("Ctrl+F9").tooltip("Indicate that the counting is completed. This is only possible if all related recordings are set to Finished."))
            .action(Act::command("Reo&pen", A::Process, "physinvt.reopen").icon("reopen").tooltip("Change the status of the physical inventory order from Finished to Open."))
            .action(Act::command("Test Report", A::Report, "physinvt.test_report").icon("report").tooltip("Preview the inventory counting that will be recorded when you choose the Post action."))
            .action(
                Act::command("P&ost", A::Home, "physinvt.post")
                    .icon("post")
                    .shortcut("F9")
                    .confirm("Do you want to post the order?")
                    .tooltip("Finalize the document or journal by posting the amounts and quantities to the related accounts in your company books."),
            ),
    );

    // Recordings.
    let rec_cols = &["Order No.", "Recording No.", "Description", "Status", "Person Responsible", "Date Recorded", "Person Recorded", "Location Code"];
    m.add_page(list("phys-inventory-recording-list", 5880, "Phys. Inventory Recording List", RECORD_HEADER, rec_cols, "phys-inventory-recording"));
    let rec_line_cols = &["Item No.", "Variant Code", "Location Code", "Bin Code", "Description", "Serial No.", "Lot No.", "Quantity", "Unit of Measure Code", "Recorded", "Shelf No.", "Date Recorded", "Person Recorded", "Recorded Without Order"];
    m.add_page(sub("phys-invt-recording-subform", 5881, "Lines", PageKind::ListPart, RECORD_LINE, rec_line_cols, true));
    m.add_page(sub("phys-invt-recording-lines", 5882, "Phys. Invt. Recording Lines", PageKind::List, RECORD_LINE, rec_line_cols, false));
    m.add_page(
        PageDef::new("phys-inventory-recording", 5879, "Phys. Inventory Recording", PageKind::Document, Some(RECORD_HEADER))
            .group(G::new("General", &["Order No.", "Recording No.", "Description", "Status", "Person Responsible", "Allow Recording Without Order", "Date Recorded", "Person Recorded", "Location Code", "Bin Code"]))
            .part("Lines", "phys-invt-recording-subform", &[("Order No.", "Order No."), ("Recording No.", "Recording No.")])
            .action(
                Act::command("Fi&nish", A::Process, "physinvt.finish_recording")
                    .icon("release")
                    .tooltip("Indicate that counting is finished. After this, you can no longer change the physical inventory order. When finishing the physical inventory order, the expected quantity and the recorded quantities are compared and the differences calculated."),
            )
            .action(Act::command("Reo&pen", A::Process, "physinvt.reopen_recording").icon("reopen").tooltip("Reopen the recording. This also reopens the related physical inventory order.")),
    );

    // Posted orders and recordings.
    m.add_page(list("posted-phys-invt-order-list", 5884, "Posted Phys. Invt. Order List", PSTD_ORDER_HEADER, &["No.", "Description", "Order Date", "Posting Date", "Pre-Assigned No.", "No. Finished Recordings", "Location Code"], "posted-phys-invt-order"));
    m.add_page(sub("posted-phys-invt-order-subf", 5885, "Lines", PageKind::ListPart, PSTD_ORDER_LINE, line_cols, false));
    m.add_page(sub("posted-phys-invt-order-lines", 5886, "Posted Phys. Invt. Order Lines", PageKind::List, PSTD_ORDER_LINE, line_cols, false));
    m.add_page(
        PageDef::new("posted-phys-invt-order", 5883, "Posted Phys. Invt. Order", PageKind::Document, Some(PSTD_ORDER_HEADER))
            .read_only()
            .group(G::new("General", &["No.", "Description", "Order Date", "Posting Date", "Pre-Assigned No.", "Person Responsible", "Location Code", "No. Finished Recordings", "User ID"]))
            .part("Lines", "posted-phys-invt-order-subf", &[("Document No.", "No.")])
            .action(Act::page("&Recordings", A::Navigate, "posted-phys-invt-rec-list").link("Order No.", "No.").icon("list").tooltip("View all recordings for the physical inventory order.")),
    );
    m.add_page(list("posted-phys-invt-rec-list", 5888, "Posted Phys. Invt. Rec. List", PSTD_RECORD_HEADER, rec_cols, "posted-phys-invt-recording"));
    m.add_page(sub("posted-phys-invt-rec-subf", 5889, "Lines", PageKind::ListPart, PSTD_RECORD_LINE, rec_line_cols, false));
    m.add_page(sub("posted-phys-invt-rec-lines", 5890, "Posted Phys. Invt. Rec. Lines", PageKind::List, PSTD_RECORD_LINE, rec_line_cols, false));
    m.add_page(
        PageDef::new("posted-phys-invt-recording", 5887, "Posted Phys. Invt. Recording", PageKind::Document, Some(PSTD_RECORD_HEADER))
            .read_only()
            .group(G::new("General", &["Order No.", "Recording No.", "Description", "Person Responsible", "Date Recorded", "Person Recorded", "Location Code"]))
            .part("Lines", "posted-phys-invt-rec-subf", &[("Order No.", "Order No."), ("Recording No.", "Recording No.")]),
    );
}
