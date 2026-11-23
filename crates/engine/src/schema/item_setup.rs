//! Item setup: 5404 Item Unit of Measure, 5401 Item Variant, 5722 Item Category, 5777 Item
//! Reference, 5715 Item Substitution, 279 / 280 Extended Text, 30 Item Translation and the item
//! attributes (7500, 7501, 7505), with their pages and the item card's links to them.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, CalcFormula, CalcFunc, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const ITEM_UOM: &str = "Item Unit of Measure";
pub const ITEM_VARIANT: &str = "Item Variant";
pub const ITEM_CATEGORY: &str = "Item Category";
pub const ITEM_REFERENCE: &str = "Item Reference";
pub const ITEM_SUBSTITUTION: &str = "Item Substitution";
pub const EXT_TEXT_HEADER: &str = "Extended Text Header";
pub const EXT_TEXT_LINE: &str = "Extended Text Line";
pub const ITEM_TRANSLATION: &str = "Item Translation";
pub const ITEM_ATTRIBUTE: &str = "Item Attribute";
pub const ITEM_ATTRIBUTE_VALUE: &str = "Item Attribute Value";
pub const ITEM_ATTRIBUTE_MAPPING: &str = "Item Attribute Value Mapping";

const ITEM: &str = crate::schema::tables::ITEM;

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str]) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = true;
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(ITEM_UOM, 5404)
            .fields(vec![
                F::code("Item No.", 20).relation(ITEM),
                F::code("Code", 10).relation("Unit of Measure"),
                F::decimal("Qty. per Unit of Measure"),
                F::decimal("Qty. Rounding Precision"),
                F::decimal("Length"),
                F::decimal("Width"),
                F::decimal("Height"),
                F::decimal("Cubage"),
                F::decimal("Weight"),
            ])
            .key(&["Item No.", "Code"]),
    );
    m.add_table(
        TableDef::new(ITEM_VARIANT, 5401)
            .fields(vec![F::code("Code", 10), F::code("Item No.", 20).relation(ITEM), F::text("Description", 100), F::text("Description 2", 50), F::boolean("Blocked"), F::boolean("Sales Blocked"), F::boolean("Purchasing Blocked")])
            .key(&["Item No.", "Code"]),
    );
    m.add_table(
        TableDef::new(ITEM_CATEGORY, 5722)
            .fields(vec![F::code("Code", 20).not_blank(), F::code("Parent Category", 20), F::text("Description", 100), F::integer("Indentation"), F::integer("Presentation Order"), F::boolean("Has Children")])
            .key(&["Code"])
            .describe_by("Description")
            .pages("item-categories", Some("item-category-card")),
    );
    m.add_table(
        TableDef::new(ITEM_REFERENCE, 5777)
            .fields(vec![
                F::code("Item No.", 20).relation(ITEM),
                F::code("Variant Code", 10),
                F::code("Unit of Measure", 10),
                F::option("Reference Type", &["", "Customer", "Vendor", "Bar Code"]),
                F::code("Reference Type No.", 20),
                F::code("Reference No.", 50),
                F::text("Description", 100),
                F::text("Description 2", 50),
                F::date("Starting Date"),
                F::date("Ending Date"),
            ])
            .key(&["Item No.", "Variant Code", "Unit of Measure", "Reference Type", "Reference Type No.", "Reference No."]),
    );
    m.add_table(
        TableDef::new(ITEM_SUBSTITUTION, 5715)
            .fields(vec![
                F::option("Type", &["Item", "Nonstock Item"]),
                F::code("No.", 20),
                F::code("Variant Code", 10),
                F::option("Substitute Type", &["Item", "Nonstock Item"]),
                F::code("Substitute No.", 20).relation(ITEM),
                F::code("Substitute Variant Code", 10),
                F::text("Description", 100),
                F::decimal("Inventory").calc(CalcFormula::new(CalcFunc::Sum, crate::schema::tables::ITEM_LEDGER, "Quantity", &[("Item No.", "Substitute No.")], &[])),
                F::boolean("Interchangeable"),
                F::boolean("Condition"),
            ])
            .key(&["Type", "No.", "Variant Code", "Substitute Type", "Substitute No.", "Substitute Variant Code"]),
    );
    let ext_table_names = &["Standard Text", "G/L Account", "Item", "Resource"];
    m.add_table(
        TableDef::new(EXT_TEXT_HEADER, 279)
            .fields(vec![
                F::option("Table Name", ext_table_names),
                F::code("No.", 20),
                F::code("Language Code", 10),
                F::integer("Text No."),
                F::date("Starting Date"),
                F::date("Ending Date"),
                F::boolean("All Language Codes"),
                F::text("Description", 100),
                F::boolean("Sales Quote"),
                F::boolean("Sales Invoice"),
                F::boolean("Sales Order"),
                F::boolean("Sales Credit Memo"),
                F::boolean("Purchase Quote"),
                F::boolean("Purchase Invoice"),
                F::boolean("Purchase Order"),
                F::boolean("Purchase Credit Memo"),
            ])
            .key(&["Table Name", "No.", "Language Code", "Text No."])
            .pages("extended-text-list", Some("extended-text")),
    );
    m.add_table(
        TableDef::new(EXT_TEXT_LINE, 280)
            .fields(vec![F::option("Table Name", ext_table_names), F::code("No.", 20), F::code("Language Code", 10), F::integer("Text No."), F::integer("Line No."), F::text("Text", 100)])
            .key(&["Table Name", "No.", "Language Code", "Text No.", "Line No."]),
    );
    m.add_table(
        TableDef::new(ITEM_TRANSLATION, 30)
            .fields(vec![F::code("Item No.", 20).relation(ITEM), F::code("Variant Code", 10), F::code("Language Code", 10), F::text("Description", 100), F::text("Description 2", 50)])
            .key(&["Item No.", "Variant Code", "Language Code"]),
    );
    m.add_table(
        TableDef::new(ITEM_ATTRIBUTE, 7500)
            .fields(vec![F::integer("ID"), F::text("Name", 250), F::boolean("Blocked"), F::option("Type", &["Option", "Text", "Integer", "Decimal", "Date"]), F::text("Unit of Measure", 30)])
            .key(&["ID"])
            .describe_by("Name")
            .pages("item-attributes", None),
    );
    m.add_table(
        TableDef::new(ITEM_ATTRIBUTE_VALUE, 7501)
            .fields(vec![
                F::integer("Attribute ID").relation(ITEM_ATTRIBUTE),
                F::integer("ID"),
                F::text("Value", 250),
                F::decimal("Numeric Value"),
                F::date("Date Value"),
                F::boolean("Blocked"),
                F::text("Attribute Name", 250).calc(CalcFormula::new(CalcFunc::Lookup, ITEM_ATTRIBUTE, "Name", &[("ID", "Attribute ID")], &[])),
            ])
            .key(&["Attribute ID", "ID"]),
    );
    m.add_table(
        TableDef::new(ITEM_ATTRIBUTE_MAPPING, 7505)
            .fields(vec![
                F::integer("Table ID"),
                F::code("No.", 20),
                F::integer("Item Attribute ID").relation(ITEM_ATTRIBUTE),
                F::integer("Item Attribute Value ID"),
                F::text("Attribute", 250).calc(CalcFormula::new(CalcFunc::Lookup, ITEM_ATTRIBUTE, "Name", &[("ID", "Item Attribute ID")], &[])),
                F::text("Value", 250).calc(CalcFormula::new(
                    CalcFunc::Lookup,
                    ITEM_ATTRIBUTE_VALUE,
                    "Value",
                    &[("Attribute ID", "Item Attribute ID"), ("ID", "Item Attribute Value ID")],
                    &[],
                )),
            ])
            .key(&["Table ID", "No.", "Item Attribute ID"]),
    );
    for table in [crate::schema::tables::SALES_LINE, crate::schema::tables::PURCH_LINE, crate::schema::inventory::ITEM_JNL_LINE] {
        if let Some(t) = m.tables.get_mut(table) {
            for f in [F::decimal("Qty. per Unit of Measure").read_only(), F::decimal("Quantity (Base)").read_only()] {
                if t.field(&f.name).is_none() {
                    t.fields.push(f);
                }
            }
        }
    }
    if let Some(t) = m.tables.get_mut(ITEM) {
        for f in [F::code("Item Category Code", 20).relation(ITEM_CATEGORY), F::code("Base Unit of Measure", 10).relation("Unit of Measure"), F::code("Sales Unit of Measure", 10), F::code("Purch. Unit of Measure", 10)] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }

    m.add_page(list("item-units-of-measure", 5404, "Item Units of Measure", ITEM_UOM, &["Item No.", "Code", "Qty. per Unit of Measure", "Qty. Rounding Precision", "Height", "Width", "Length", "Cubage", "Weight"]));
    m.add_page(list("item-variants", 5401, "Item Variants", ITEM_VARIANT, &["Item No.", "Code", "Description", "Description 2", "Blocked", "Sales Blocked", "Purchasing Blocked"]));
    let mut cats = list("item-categories", 5730, "Item Categories", ITEM_CATEGORY, &["Code", "Description", "Parent Category"]);
    cats.card_page = Some("item-category-card".to_string());
    cats.editable = false;
    m.add_page(cats);
    m.add_page(PageDef::new("item-category-card", 5733, "Item Category Card", PageKind::Card, Some(ITEM_CATEGORY)).group(G::new("General", &["Code", "Description", "Parent Category"])));
    m.add_page(list("item-reference-entries", 5737, "Item Reference Entries", ITEM_REFERENCE, &["Reference Type", "Reference Type No.", "Reference No.", "Variant Code", "Unit of Measure", "Description", "Description 2", "Starting Date", "Ending Date"]));
    m.add_page(list("item-substitution-entry", 5716, "Item Substitution Entry", ITEM_SUBSTITUTION, &["Substitute Type", "Substitute No.", "Substitute Variant Code", "Description", "Inventory", "Interchangeable", "Condition"]));
    let mut ext_list = list("extended-text-list", 391, "Extended Text List", EXT_TEXT_HEADER, &["Language Code", "All Language Codes", "Description", "Starting Date", "Ending Date"]);
    ext_list.card_page = Some("extended-text".to_string());
    ext_list.editable = false;
    m.add_page(ext_list);
    let mut ext_lines = PageDef::new("extended-text-lines", 387, "Lines", PageKind::ListPart, Some(EXT_TEXT_LINE));
    ext_lines.columns = vec!["Text".to_string()];
    ext_lines.editable = true;
    m.add_page(ext_lines);
    m.add_page(
        PageDef::new("extended-text", 386, "Extended Text", PageKind::Document, Some(EXT_TEXT_HEADER))
            .group(G::new("General", &["Language Code", "All Language Codes", "Description", "Starting Date", "Ending Date"]))
            .part("Lines", "extended-text-lines", &[("Table Name", "Table Name"), ("No.", "No."), ("Language Code", "Language Code"), ("Text No.", "Text No.")])
            .group(G::new("Sales", &["Sales Quote", "Sales Order", "Sales Invoice", "Sales Credit Memo"]))
            .group(G::new("Purchases", &["Purchase Quote", "Purchase Order", "Purchase Invoice", "Purchase Credit Memo"])),
    );
    m.add_page(list("item-translations", 35, "Item Translations", ITEM_TRANSLATION, &["Variant Code", "Language Code", "Description", "Description 2"]));
    let mut attrs = list("item-attributes", 7500, "Item Attributes", ITEM_ATTRIBUTE, &["ID", "Name", "Type", "Unit of Measure", "Blocked"]);
    attrs.actions = vec![Act::page("Item Attribute &Values", A::Navigate, "item-attribute-values").link("Attribute ID", "ID").icon("list")];
    m.add_page(attrs);
    m.add_page(list("item-attribute-values", 7501, "Item Attribute Values", ITEM_ATTRIBUTE_VALUE, &["ID", "Value", "Blocked"]));
    m.add_page(list("item-attribute-mapping", 0, "Item Attributes", ITEM_ATTRIBUTE_MAPPING, &["Item Attribute ID", "Attribute", "Item Attribute Value ID", "Value"]));

    let links = [
        Act::page("&Units of Measure", A::Navigate, "item-units-of-measure").link("Item No.", "No.").icon("list").tooltip("Configure alternative units of measure and conversion factors for this item."),
        Act::page("Va&riants", A::Navigate, "item-variants").link("Item No.", "No.").icon("list").tooltip("Manage item variants such as size or color under a single item number."),
        Act::page("Item Re&ferences", A::Navigate, "item-reference-entries").link("Item No.", "No.").icon("list").tooltip("Map external customer, vendor, or barcode numbers to this item."),
        Act::page("Substituti&ons", A::Navigate, "item-substitution-entry").link("Type", "=Item").link("No.", "No.").icon("list").tooltip("Define replacement items that can be offered when this item is out of stock."),
        Act::page("E&xtended Texts", A::Navigate, "extended-text-list").link("Table Name", "=Item").link("No.", "No.").icon("list").tooltip("Configure supplemental text lines that can be inserted on document lines."),
        Act::page("Translations", A::Navigate, "item-translations").link("Item No.", "No.").icon("list").tooltip("Maintain language-specific descriptions for this item."),
        Act::page("Attributes", A::Navigate, "item-attribute-mapping").link("Table ID", "=27").link("No.", "No.").icon("list").tooltip("Assign descriptive attributes to categorize and filter this item."),
    ];
    for page in ["item-card", "item-list"] {
        if let Some(p) = m.pages.get_mut(page) {
            p.actions.extend(links.iter().cloned());
        }
    }
}
