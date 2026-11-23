//! Transfer orders: 5740 Transfer Header, 5741 Transfer Line, the posted 5744 / 5745 Transfer
//! Shipment and 5746 / 5747 Transfer Receipt, with their pages (5742, 5740, 5741, 5752, 5743,
//! 5744, 5753, 5745, 5746), and the in-transit locations.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const TRANSFER_HEADER: &str = "Transfer Header";
pub const TRANSFER_LINE: &str = "Transfer Line";
pub const TRANSFER_SHPT_HEADER: &str = "Transfer Shipment Header";
pub const TRANSFER_SHPT_LINE: &str = "Transfer Shipment Line";
pub const TRANSFER_RCPT_HEADER: &str = "Transfer Receipt Header";
pub const TRANSFER_RCPT_LINE: &str = "Transfer Receipt Line";

const LOCATION: &str = crate::schema::inventory::LOCATION;
const ITEM: &str = crate::schema::tables::ITEM;

fn header_fields() -> Vec<F> {
    vec![
        F::code("No.", 20),
        F::code("Transfer-from Code", 10).relation(LOCATION),
        F::text("Transfer-from Name", 100),
        F::code("Transfer-to Code", 10).relation(LOCATION),
        F::text("Transfer-to Name", 100),
        F::date("Posting Date"),
        F::date("Shipment Date"),
        F::date("Receipt Date"),
        F::code("In-Transit Code", 10).relation(LOCATION),
        F::code("Shipment Method Code", 10),
        F::code("External Document No.", 35),
        F::code("Assigned User ID", 50),
    ]
}

fn line_fields(doc_field: &str) -> Vec<F> {
    vec![
        F::code(doc_field, 20),
        F::integer("Line No."),
        F::code("Item No.", 20).relation(ITEM),
        F::code("Variant Code", 10),
        F::text("Description", 100),
        F::decimal("Quantity"),
        F::code("Unit of Measure Code", 10),
        F::decimal("Qty. per Unit of Measure"),
        F::decimal("Quantity (Base)"),
        F::code("Transfer-from Code", 10),
        F::code("Transfer-to Code", 10),
        F::code("In-Transit Code", 10),
        F::date("Shipment Date"),
        F::date("Receipt Date"),
        F::integer("Item Shpt. Entry No."),
        F::integer("Item Rcpt. Entry No."),
    ]
}

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], card: &str) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.card_page = Some(card.to_string());
    p
}

fn sub(id: &str, object_id: u32, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, "Lines", PageKind::ListPart, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    if let Some(t) = m.tables.get_mut(LOCATION)
        && t.field("Use As In-Transit").is_none()
    {
        t.fields.push(F::boolean("Use As In-Transit"));
    }
    if let Some(t) = m.tables.get_mut(crate::schema::tables::INVENTORY_SETUP) {
        for f in [F::code("Transfer Order Nos.", 20).relation("No. Series"), F::code("Posted Transfer Shpt. Nos.", 20).relation("No. Series"), F::code("Posted Transfer Rcpt. Nos.", 20).relation("No. Series")] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
    let mut h = header_fields();
    h.extend([F::option("Status", &["Open", "Released"]).read_only(), F::boolean("Direct Transfer"), F::code("Last Shipment No.", 20).read_only(), F::code("Last Receipt No.", 20).read_only()]);
    m.add_table(TableDef::new(TRANSFER_HEADER, 5740).fields(h).key(&["No."]).pages("transfer-orders", Some("transfer-order")));
    let mut l = line_fields("Document No.");
    l.extend([
        F::decimal("Qty. to Ship"),
        F::decimal("Quantity Shipped").read_only(),
        F::decimal("Qty. to Receive"),
        F::decimal("Quantity Received").read_only(),
        F::decimal("Qty. in Transit").read_only(),
        F::decimal("Outstanding Quantity").read_only(),
        F::boolean("Completely Shipped").read_only(),
        F::boolean("Completely Received").read_only(),
    ]);
    m.add_table(TableDef::new(TRANSFER_LINE, 5741).fields(l).key(&["Document No.", "Line No."]));
    let mut sh = header_fields();
    sh.push(F::code("Transfer Order No.", 20));
    m.add_table(TableDef::new(TRANSFER_SHPT_HEADER, 5744).fields(sh.clone()).key(&["No."]).pages("posted-transfer-shipments", Some("posted-transfer-shipment")));
    m.add_table(TableDef::new(TRANSFER_SHPT_LINE, 5745).fields(line_fields("Document No.")).key(&["Document No.", "Line No."]));
    m.add_table(TableDef::new(TRANSFER_RCPT_HEADER, 5746).fields(sh).key(&["No."]).pages("posted-transfer-receipts", Some("posted-transfer-receipt")));
    m.add_table(TableDef::new(TRANSFER_RCPT_LINE, 5747).fields(line_fields("Document No.")).key(&["Document No.", "Line No."]));

    let list_cols = &["No.", "Transfer-from Code", "Transfer-to Code", "In-Transit Code", "Status", "Direct Transfer", "Shipment Date", "Receipt Date"];
    let mut orders = list("transfer-orders", 5742, "Transfer Orders", TRANSFER_HEADER, list_cols, "transfer-order");
    orders.keywords = "transfer move stock location".to_string();
    m.add_page(orders);
    m.add_page(sub(
        "transfer-order-subform",
        5741,
        TRANSFER_LINE,
        &["Item No.", "Variant Code", "Description", "Quantity", "Unit of Measure Code", "Qty. to Ship", "Quantity Shipped", "Qty. to Receive", "Quantity Received", "Shipment Date", "Receipt Date"],
        true,
    ));
    m.add_page(
        PageDef::new("transfer-order", 5740, "Transfer Order", PageKind::Document, Some(TRANSFER_HEADER))
            .group(G::new("General", &["No.", "Transfer-from Code", "Transfer-to Code", "Direct Transfer", "In-Transit Code", "Posting Date", "Status"]))
            .part("Lines", "transfer-order-subform", &[("Document No.", "No.")])
            .group(G::new("Shipment", &["Shipment Date", "Shipment Method Code"]).collapsed())
            .action(
                Act::command("P&ost", A::Home, "transfer.post")
                    .icon("post")
                    .shortcut("F9")
                    .ask(&["mode"])
                    .tooltip("Post the transfer shipment, receipt, or direct transfer to inventory ledgers."),
            )
            .action(Act::command("Re&lease", A::Home, "transfer.release").icon("release").shortcut("Ctrl+F9").tooltip("Release this transfer order for shipping and receiving."))
            .action(Act::command("Reo&pen", A::Home, "transfer.reopen").icon("reopen").tooltip("Reopen this transfer order for editing."))
            .action(Act::page("S&hipments", A::Navigate, "posted-transfer-shipments").link("Transfer Order No.", "No.").icon("list").tooltip("View related posted transfer shipments."))
            .action(Act::page("Re&ceipts", A::Navigate, "posted-transfer-receipts").link("Transfer Order No.", "No.").icon("list").tooltip("View related posted transfer receipts.")),
    );
    let posted_cols = &["No.", "Transfer Order No.", "Transfer-from Code", "Transfer-to Code", "Posting Date"];
    let posted_lines = &["Item No.", "Variant Code", "Description", "Quantity", "Unit of Measure Code", "Shipment Date", "Receipt Date"];
    for (list_id, list_obj, list_caption, card_id, card_obj, card_caption, sub_id, sub_obj, header, line) in [
        ("posted-transfer-shipments", 5752, "Posted Transfer Shipments", "posted-transfer-shipment", 5743, "Posted Transfer Shipment", "posted-transfer-shpt-subform", 5744, TRANSFER_SHPT_HEADER, TRANSFER_SHPT_LINE),
        ("posted-transfer-receipts", 5753, "Posted Transfer Receipts", "posted-transfer-receipt", 5745, "Posted Transfer Receipt", "posted-transfer-rcpt-subform", 5746, TRANSFER_RCPT_HEADER, TRANSFER_RCPT_LINE),
    ] {
        m.add_page(list(list_id, list_obj, list_caption, header, posted_cols, card_id));
        m.add_page(sub(sub_id, sub_obj, line, posted_lines, false));
        m.add_page(
            PageDef::new(card_id, card_obj, card_caption, PageKind::Document, Some(header))
                .read_only()
                .group(G::new("General", &["No.", "Transfer Order No.", "Transfer-from Code", "Transfer-to Code", "Posting Date"]))
                .part("Lines", sub_id, &[("Document No.", "No.")]),
        );
    }
}
