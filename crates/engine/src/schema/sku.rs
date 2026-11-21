//! Stockkeeping units (BC 5700 Stockkeeping Unit, pages 5700 card / 5701 list, report 5706
//! Create Stockkeeping Unit): an item's costs and planning parameters per location and variant.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const SKU: &str = "Stockkeeping Unit";

const ITEM: &str = crate::schema::tables::ITEM;
const LOCATION: &str = crate::schema::inventory::LOCATION;

/// The fields a new stockkeeping unit takes over from its item.
pub const FROM_ITEM: &[&str] = &[
    "Description",
    "Description 2",
    "Shelf No.",
    "Unit Cost",
    "Standard Cost",
    "Last Direct Cost",
    "Vendor No.",
    "Vendor Item No.",
    "Lead Time Calculation",
    "Reorder Point",
    "Maximum Inventory",
    "Reorder Quantity",
    "Reordering Policy",
    "Safety Stock Quantity",
    "Minimum Order Quantity",
    "Order Multiple",
    "Replenishment System",
    "Phys Invt Counting Period Code",
];

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(SKU, 5700)
            .fields(vec![
                F::code("Item No.", 20).not_blank().relation(ITEM),
                F::code("Variant Code", 10),
                F::code("Location Code", 10).not_blank().relation(LOCATION),
                F::text("Description", 100),
                F::text("Description 2", 50),
                F::code("Shelf No.", 10),
                F::decimal("Unit Cost").auto_format(2),
                F::decimal("Standard Cost").auto_format(2),
                F::decimal("Last Direct Cost").auto_format(2),
                F::code("Vendor No.", 20).relation(crate::schema::tables::VENDOR),
                F::text("Vendor Item No.", 50),
                F::code("Lead Time Calculation", 20),
                F::decimal("Reorder Point"),
                F::decimal("Maximum Inventory"),
                F::decimal("Reorder Quantity"),
                F::option("Reordering Policy", &["", "Fixed Reorder Qty.", "Maximum Qty.", "Order", "Lot-for-Lot"]),
                F::decimal("Safety Stock Quantity"),
                F::decimal("Minimum Order Quantity"),
                F::decimal("Order Multiple"),
                F::option("Replenishment System", &["Purchase", "Prod. Order", "Transfer", "Assembly"]),
                F::code("Transfer-from Code", 10).relation(LOCATION),
                F::code("Phys Invt Counting Period Code", 10).relation(crate::schema::physinvt::COUNTING_PERIOD),
                F::date("Last Counting Period Update").read_only(),
                F::date("Next Counting Start Date").read_only(),
                F::date("Next Counting End Date").read_only(),
                F::decimal("Inventory").flow(),
                F::decimal("Qty. on Purch. Order").flow(),
                F::decimal("Qty. on Sales Order").flow(),
                F::decimal("Qty. in Transit").flow(),
                F::date("Last Date Modified").read_only(),
            ])
            .key(&["Location Code", "Item No.", "Variant Code"])
            .describe_by("Description")
            .pages("stockkeeping-unit-list", Some("stockkeeping-unit-card")),
    );
    let mut list = PageDef::new("stockkeeping-unit-list", 5701, "Stockkeeping Unit List", PageKind::List, Some(SKU));
    list.columns = ["Item No.", "Location Code", "Variant Code", "Replenishment System", "Description", "Inventory", "Reordering Policy", "Unit Cost", "Last Direct Cost"].iter().map(|c| (*c).to_string()).collect();
    list.card_page = Some("stockkeeping-unit-card".to_string());
    list.keywords = "sku stockkeeping location planning".to_string();
    m.add_page(list);
    m.add_page(
        PageDef::new("stockkeeping-unit-card", 5700, "Stockkeeping Unit Card", PageKind::Card, Some(SKU))
            .group(G::new("General", &["Item No.", "Variant Code", "Location Code", "Description", "Shelf No.", "Last Date Modified"]))
            .group(G::new("Inventory", &["Inventory", "Qty. on Purch. Order", "Qty. on Sales Order", "Qty. in Transit"]))
            .group(G::new("Invoicing", &["Standard Cost", "Unit Cost", "Last Direct Cost"]))
            .group(G::new("Replenishment", &["Replenishment System", "Lead Time Calculation", "Vendor No.", "Vendor Item No.", "Transfer-from Code"]))
            .group(G::new("Planning", &["Reordering Policy", "Safety Stock Quantity", "Reorder Point", "Reorder Quantity", "Maximum Inventory", "Minimum Order Quantity", "Order Multiple"]))
            .group(G::new("Warehouse", &["Phys Invt Counting Period Code", "Last Counting Period Update", "Next Counting Start Date", "Next Counting End Date"]).collapsed())
            .action(Act::page("Item Ledger E&ntries", A::Navigate, "item-ledger-entries").link("Item No.", "Item No.").link("Location Code", "Location Code").icon("list").shortcut("Ctrl+F7").tooltip("View the history of transactions that have been posted for the selected record.")),
    );
    for page in ["item-card", "item-list"] {
        if let Some(p) = m.pages.get_mut(page) {
            p.actions.push(Act::page("Stockkeepin&g Units", A::Navigate, "stockkeeping-unit-list").link("Item No.", "No.").icon("list").tooltip("Open the item's SKUs to view or edit instances of the item at different locations or with different variants."));
            p.actions.push(
                Act::command("&Create Stockkeeping Unit", A::Process, "item.create_sku")
                    .icon("new")
                    .ask(&["location_code", "replace_previous"])
                    .tooltip("Create an instance of the item for each location that is set up."),
            );
        }
    }
}
