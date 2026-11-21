//! Dimensions (BC 348 Dimension, 349 Dimension Value, 352 Default Dimension, 480 Dimension Set
//! Entry) and their pages; the G/L Setup's global and shortcut dimensions; Dimension Set ID on
//! documents, journals and entries; a Dimensions action on the master cards.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

pub const DIMENSION: &str = "Dimension";
pub const DIM_VALUE: &str = "Dimension Value";
pub const DEFAULT_DIM: &str = "Default Dimension";
pub const DIM_SET_ENTRY: &str = "Dimension Set Entry";

/// Tables that carry a dimension set: documents, journals, posted documents and entries.
const WITH_DIM_SET: &[&str] = &[
    "Sales Header",
    "Sales Line",
    "Purchase Header",
    "Purchase Line",
    "Gen. Journal Line",
    "Item Journal Line",
    "Sales Invoice Header",
    "Sales Invoice Line",
    "Sales Cr.Memo Header",
    "Sales Cr.Memo Line",
    "Sales Shipment Header",
    "Sales Shipment Line",
    "Purch. Inv. Header",
    "Purch. Inv. Line",
    "Purch. Cr. Memo Hdr.",
    "Purch. Cr. Memo Line",
    "Purch. Rcpt. Header",
    "Purch. Rcpt. Line",
    "G/L Entry",
    "Cust. Ledger Entry",
    "Vendor Ledger Entry",
    "Item Ledger Entry",
    "Bank Account Ledger Entry",
    "FA Ledger Entry",
];

/// Master cards with default dimensions: (card page, table id).
pub const MASTERS: &[(&str, u32)] =
    &[("customer-card", 18), ("vendor-card", 23), ("item-card", 27), ("gl-account-card", 15), ("bank-account-card", 270), ("fixed-asset-card", 5600)];

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str]) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(DIMENSION, 348)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::text("Name", 30),
                F::text("Code Caption", 80),
                F::text("Filter Caption", 80),
                F::text("Description", 100),
                F::boolean("Blocked"),
            ])
            .key(&["Code"])
            .describe_by("Name")
            .pages("dimensions", None),
    );
    m.add_table(
        TableDef::new(DIM_VALUE, 349)
            .fields(vec![
                F::code("Dimension Code", 20).relation(DIMENSION).not_blank(),
                F::code("Code", 20).not_blank(),
                F::text("Name", 50),
                F::option("Dimension Value Type", &["Standard", "Heading", "Total", "Begin-Total", "End-Total"]),
                F::text("Totaling", 250),
                F::boolean("Blocked"),
                F::integer("Global Dimension No.").read_only(),
            ])
            .key(&["Dimension Code", "Code"])
            .describe_by("Name")
            .pages("dimension-values", None),
    );
    m.add_table(
        TableDef::new(DEFAULT_DIM, 352)
            .fields(vec![
                F::integer("Table ID"),
                F::code("No.", 20),
                F::code("Dimension Code", 20).relation(DIMENSION).not_blank(),
                F::code("Dimension Value Code", 20),
                F::option("Value Posting", &["", "Code Mandatory", "Same Code", "No Code"]),
            ])
            .key(&["Table ID", "No.", "Dimension Code"])
            .pages("default-dimensions", None),
    );
    m.add_table(
        TableDef::new(DIM_SET_ENTRY, 480)
            .fields(vec![
                F::integer("Dimension Set ID"),
                F::code("Dimension Code", 20).relation(DIMENSION),
                F::code("Dimension Value Code", 20),
                F::integer("Dimension Value ID"),
                F::text("Dimension Name", 30),
                F::text("Dimension Value Name", 50),
                F::integer("Global Dimension No."),
            ])
            .key(&["Dimension Set ID", "Dimension Code"])
            .pages("dimension-set-entries", None),
    );

    let mut dims = list("dimensions", 536, "Dimensions", DIMENSION, &["Code", "Name", "Code Caption", "Filter Caption", "Description", "Blocked"]);
    dims.actions.push(Act::page("Dimension &Values", A::Navigate, "dimension-values").link("Dimension Code", "Code").icon("list"));
    dims.keywords = "dimension department project area analysis".to_string();
    m.add_page(dims);
    m.add_page(list("dimension-values", 537, "Dimension Values", DIM_VALUE, &["Code", "Name", "Dimension Value Type", "Totaling", "Blocked"]));
    m.add_page(list("default-dimensions", 540, "Default Dimensions", DEFAULT_DIM, &["Dimension Code", "Dimension Value Code", "Value Posting"]));
    let mut set = list("dimension-set-entries", 479, "Dimension Set Entries", DIM_SET_ENTRY, &["Dimension Code", "Dimension Name", "Dimension Value Code", "Dimension Value Name"]);
    set.editable = false;
    m.add_page(set);

    if let Some(t) = m.tables.get_mut("General Ledger Setup") {
        let mut add = |f: F| {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        };
        add(F::code("Global Dimension 1 Code", 20).relation(DIMENSION));
        add(F::code("Global Dimension 2 Code", 20).relation(DIMENSION));
        for n in 1..=8 {
            let f = F::code(&format!("Shortcut Dimension {n} Code"), 20).relation(DIMENSION);
            // 1 and 2 are the global dimensions.
            add(if n <= 2 { f.read_only() } else { f });
        }
    }
    for table in WITH_DIM_SET {
        if let Some(t) = m.tables.get_mut(*table)
            && t.field("Dimension Set ID").is_none()
        {
            t.fields.push(F::integer("Dimension Set ID").read_only());
        }
    }
    // Every master has its two global dimensions (shown on BC's cards under Show more or not at all).
    let masters: Vec<String> = m.tables.values().filter(|t| MASTERS.iter().any(|(_, id)| *id == t.object_id)).map(|t| t.name.clone()).collect();
    for name in masters {
        if let Some(t) = m.tables.get_mut(&name) {
            for n in 1..=2 {
                let f = format!("Global Dimension {n} Code");
                if t.field(&f).is_none() {
                    t.fields.push(F::code(&f, 20));
                }
            }
        }
    }
    // Documents, posted documents and entries show their dimension set.
    let with_set: Vec<String> = m
        .pages
        .values()
        .filter(|p| p.source_table.as_deref().is_some_and(|t| WITH_DIM_SET.contains(&t)) && p.kind != PageKind::ListPart)
        .filter(|p| !p.actions.iter().any(|a| a.caption == "Dimensions"))
        .map(|p| p.id.clone())
        .collect();
    for id in with_set {
        if let Some(p) = m.pages.get_mut(&id) {
            p.actions.push(
                Act::page("Dimensions", A::Navigate, "dimension-set-entries")
                    .link("Dimension Set ID", "Dimension Set ID")
                    .shortcut("Alt+D")
                    .icon("dimensions")
                    .tooltip("View or edit dimensions, such as area, project, or department, that you can assign to sales and purchase documents to distribute costs and analyze transaction history."),
            );
        }
    }
    // Ledger entry lists: Set Dimension Filter.
    let entry_lists: Vec<String> = m
        .pages
        .values()
        .filter(|p| p.kind == PageKind::List)
        .filter(|p| p.source_table.as_deref().is_some_and(|t| t.ends_with("Ledger Entry") || t == "G/L Entry"))
        .filter(|p| p.source_table.as_deref().and_then(|t| m.tables.get(t)).is_some_and(|t| t.field("Dimension Set ID").is_some()))
        .map(|p| p.id.clone())
        .collect();
    for id in entry_lists {
        if let Some(p) = m.pages.get_mut(&id) {
            p.actions.push(
                Act::command("Set Dimension Filter", A::Navigate, "dimension.set_filter")
                    .ask(&["dimension_code", "value_filter"])
                    .icon("filter")
                    .tooltip("Limit the entries according to the dimension filters that you specify."),
            );
        }
    }
    for (page, table_id) in MASTERS {
        if let Some(p) = m.pages.get_mut(*page) {
            p.actions.push(
                Act::page("Dimensions", A::Navigate, "default-dimensions")
                    .link("Table ID", &format!("={table_id}"))
                    .link("No.", "No.")
                    .shortcut("Alt+D")
                    .icon("dimensions")
                    .tooltip("View or edit the single set of dimensions that are set up for the selected record."),
            );
        }
    }
}
