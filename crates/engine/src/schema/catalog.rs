//! Catalog items (BC 5718 Nonstock Item "Catalog Item", 5719 Nonstock Item Setup, pages 5725 /
//! 5726 / 5732) and the item templates (1382 Item Templ., pages 1383 / 1384) new items are made
//! from.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const NONSTOCK_ITEM: &str = "Nonstock Item";
pub const NONSTOCK_SETUP: &str = "Nonstock Item Setup";
pub const ITEM_TEMPL: &str = "Item Templ.";
pub const NO_FORMATS: &[&str] = &["Vendor Item No.", "Mfr. + Vendor Item No.", "Vendor Item No. + Mfr.", "Entry No.", "Item No. Series"];

const ITEM: &str = crate::schema::tables::ITEM;

/// The item fields a template carries.
pub const TEMPLATE_FIELDS: &[&str] = &[
    "Type",
    "Base Unit of Measure",
    "Item Category Code",
    "Gen. Prod. Posting Group",
    "VAT Prod. Posting Group",
    "Inventory Posting Group",
    "Costing Method",
    "Reserve",
    "Item Tracking Code",
];

pub fn register(m: &mut Metadata) {
    if let Some(t) = m.tables.get_mut(ITEM) {
        for f in [F::boolean("Created From Nonstock Item").read_only(), F::code("Vendor Item No.", 50), F::decimal("Net Weight"), F::decimal("Gross Weight")] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
    if let Some(t) = m.tables.get_mut(crate::schema::tables::INVENTORY_SETUP)
        && t.field("Nonstock Item Nos.").is_none()
    {
        t.fields.push(F::code("Nonstock Item Nos.", 20).relation("No. Series").caption("Catalog Item Nos."));
    }
    m.add_table(
        TableDef::new(ITEM_TEMPL, 1382)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Description", 100),
                F::option("Type", &["Inventory", "Service", "Non-Inventory"]),
                F::code("Base Unit of Measure", 10).relation("Unit of Measure"),
                F::code("Item Category Code", 20),
                F::code("Gen. Prod. Posting Group", 20).relation("Gen. Product Posting Group"),
                F::code("VAT Prod. Posting Group", 20).relation("VAT Product Posting Group"),
                F::code("Inventory Posting Group", 20),
                F::option("Costing Method", &["FIFO", "LIFO", "Specific", "Average", "Standard"]),
                F::option("Reserve", &["Never", "Optional", "Always"]),
                F::code("Item Tracking Code", 10),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("item-templ-list", Some("item-templ-card")),
    );
    let mut tl = PageDef::new("item-templ-list", 1383, "Item Templates", PageKind::List, Some(ITEM_TEMPL));
    tl.columns = ["Code", "Description"].iter().map(|c| (*c).to_string()).collect();
    tl.card_page = Some("item-templ-card".to_string());
    tl.keywords = "item template".to_string();
    m.add_page(tl);
    m.add_page(
        PageDef::new("item-templ-card", 1384, "Item Template", PageKind::Card, Some(ITEM_TEMPL))
            .group(G::new("General", &["Code", "Description", "Type", "Base Unit of Measure", "Item Category Code"]))
            .group(G::new("Posting Details", &["Gen. Prod. Posting Group", "VAT Prod. Posting Group", "Inventory Posting Group", "Costing Method"]))
            .group(G::new("Item Tracking", &["Item Tracking Code", "Reserve"]))
            .action(Act::command("Copy Template", A::Process, "item_templ.copy").icon("copy").ask(&["from_code"]).tooltip("Copies all information to the current template from the selected one.")),
    );

    m.add_table(
        TableDef::new(NONSTOCK_SETUP, 5719)
            .fields(vec![F::code("Primary Key", 10), F::option("No. Format", NO_FORMATS), F::code("No. Format Separator", 1)])
            .key(&["Primary Key"])
            .pages("catalog-item-setup", None),
    );
    m.add_page(PageDef::new("catalog-item-setup", 5732, "Catalog Item Setup", PageKind::Card, Some(NONSTOCK_SETUP)).group(G::new("General", &["No. Format", "No. Format Separator"])));
    m.add_table(
        TableDef::new(NONSTOCK_ITEM, 5718)
            .fields(vec![
                F::code("Entry No.", 20),
                F::code("Manufacturer Code", 10),
                F::code("Vendor No.", 20).relation(crate::schema::tables::VENDOR),
                F::code("Vendor Item No.", 50),
                F::text("Description", 100),
                F::code("Unit of Measure", 10).relation("Unit of Measure"),
                F::decimal("Published Cost").auto_format(2),
                F::decimal("Negotiated Cost").auto_format(2),
                F::decimal("Unit Price").auto_format(2),
                F::decimal("Gross Weight"),
                F::decimal("Net Weight"),
                F::date("Last Date Modified").read_only(),
                F::code("Bar Code", 20),
                F::code("Item No.", 20).relation(ITEM).read_only(),
                F::code("Item Templ. Code", 20).relation(ITEM_TEMPL),
                F::code("Item Category Code", 20),
                F::code("No. Series", 20).read_only(),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("catalog-item-list", Some("catalog-item-card")),
    );
    let mut list = PageDef::new("catalog-item-list", 5726, "Catalog Item List", PageKind::List, Some(NONSTOCK_ITEM));
    list.columns = ["Entry No.", "Item No.", "Manufacturer Code", "Vendor No.", "Vendor Item No.", "Description", "Unit of Measure", "Published Cost", "Negotiated Cost", "Unit Price", "Item Templ. Code", "Last Date Modified"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    list.card_page = Some("catalog-item-card".to_string());
    list.keywords = "catalog nonstock item vendor".to_string();
    let create = Act::command("&Create Item", A::Process, "catalog.create_item").icon("new").tooltip("Convert the catalog item card to a normal item card, according to an item template that you choose.");
    list.actions = vec![
        create.clone(),
        Act::command("Catalog Item Sales", A::Report, "catalog.sales").icon("report").tooltip("View a list of item sales for each catalog item."),
        Act::command("Item Substitutions", A::Report, "catalog.substitutions").icon("report").tooltip("View or edit any substitute items that are set up to be traded instead of the item in case it is not available."),
    ];
    m.add_page(list);
    m.add_page(
        PageDef::new("catalog-item-card", 5725, "Catalog Item Card", PageKind::Card, Some(NONSTOCK_ITEM))
            .group(G::new("General", &["Entry No.", "Manufacturer Code", "Vendor No.", "Vendor Item No.", "Item Templ. Code", "Item No.", "Item Category Code", "Description", "Unit of Measure", "Last Date Modified", "Bar Code"]))
            .group(G::new("Invoicing", &["Published Cost", "Negotiated Cost", "Unit Price", "Gross Weight", "Net Weight"]))
            .action(create),
    );
}
