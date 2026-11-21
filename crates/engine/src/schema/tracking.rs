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
