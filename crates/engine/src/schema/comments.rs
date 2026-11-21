//! Comments (BC 97 Comment Line, 44 Sales Comment Line, 43 Purch. Comment Line), their comment
//! sheets, and the Co&mments action on master cards and documents.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, FieldType, Metadata, PageDef, PageKind, TableDef};

pub const COMMENT_LINE: &str = "Comment Line";
pub const SALES_COMMENT: &str = "Sales Comment Line";
pub const PURCH_COMMENT: &str = "Purch. Comment Line";

const TABLE_NAMES: &[&str] = &[
    "G/L Account",
    "Customer",
    "Vendor",
    "Item",
    "Resource",
    "Job",
    "Resource Group",
    "Bank Account",
    "Campaign",
    "Fixed Asset",
    "Insurance",
    "Nonstock Item",
    "IC Partner",
    "Vendor Agreement",
    "Customer Agreement",
];
const SALES_TYPES: &[&str] =
    &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", "Shipment", "Posted Invoice", "Posted Credit Memo", "Posted Return Receipt"];
const PURCH_TYPES: &[&str] =
    &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", "Receipt", "Posted Invoice", "Posted Credit Memo", "Posted Return Shipment"];

/// Master cards: (page, Table Name).
const MASTER_CARDS: &[(&str, &str)] = &[
    ("customer-card", "Customer"),
    ("vendor-card", "Vendor"),
    ("item-card", "Item"),
    ("gl-account-card", "G/L Account"),
    ("bank-account-card", "Bank Account"),
    ("fixed-asset-card", "Fixed Asset"),
];

fn sheet(id: &str, object_id: u32, caption: &str, table: &str) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = vec!["Date".into(), "Comment".into(), "Code".into()];
    p
}

fn comment_fields(first: F) -> Vec<F> {
    vec![first, F::code("No.", 20), F::integer("Line No."), F::date("Date"), F::code("Code", 10), F::text("Comment", 80)]
}

fn comments_action(page: &str) -> Act {
    Act::page("Co&mments", A::Navigate, page)
        .icon("comment")
        .tooltip("View or add comments for the record.")
}

/// "Find entries..." (BC's Navigate) on posted documents and entry lists.
fn find_entries(m: &mut Metadata) {
    const TABLES: &[&str] = &[
        "Sales Shipment Header",
        "Sales Invoice Header",
        "Sales Cr.Memo Header",
        "Purch. Rcpt. Header",
        "Purch. Inv. Header",
        "Purch. Cr. Memo Hdr.",
        "G/L Entry",
        "Cust. Ledger Entry",
        "Vendor Ledger Entry",
        "Item Ledger Entry",
        "Bank Account Ledger Entry",
        "FA Ledger Entry",
        "Detailed Cust. Ledg. Entry",
        "Detailed Vendor Ledg. Entry",
    ];
    for p in m.pages.values_mut() {
        if p.source_table.as_deref().is_some_and(|t| TABLES.contains(&t)) && !p.actions.iter().any(|a| a.caption == "Find entries...") {
            p.actions.push(
                Act::command("Find entries...", A::Navigate, "navigate")
                    .shortcut("Ctrl+Shift+I")
                    .icon("navigate")
                    .tooltip("Find entries and documents that exist for the document number and posting date on the selected document."),
            );
        }
    }
}

pub fn register(m: &mut Metadata) {
    find_entries(m);
    m.add_table(
        TableDef::new(COMMENT_LINE, 97)
            .fields(comment_fields(F::new("Table Name", FieldType::option(TABLE_NAMES))))
            .key(&["Table Name", "No.", "Line No."])
            .pages("comment-sheet", None),
    );
    for (name, id, types, page) in [(SALES_COMMENT, 44, SALES_TYPES, "sales-comment-sheet"), (PURCH_COMMENT, 43, PURCH_TYPES, "purch-comment-sheet")] {
        let mut fields = comment_fields(F::new("Document Type", FieldType::option(types)));
        fields.push(F::integer("Document Line No."));
        m.add_table(TableDef::new(name, id).fields(fields).key(&["Document Type", "No.", "Document Line No.", "Line No."]).pages(page, None));
    }
    m.add_page(sheet("comment-sheet", 124, "Comment Sheet", COMMENT_LINE));
    m.add_page(sheet("sales-comment-sheet", 67, "Sales Comment Sheet", SALES_COMMENT));
    m.add_page(sheet("purch-comment-sheet", 66, "Purch. Comment Sheet", PURCH_COMMENT));

    for (page, table_name) in MASTER_CARDS {
        if let Some(p) = m.pages.get_mut(*page) {
            p.actions.push(comments_action("comment-sheet").link("Table Name", &format!("={table_name}")).link("No.", "No."));
        }
    }
    // Documents keep their own type; posted documents are filed as "Posted Invoice" and so on.
    let documents: Vec<(String, Option<&str>, bool)> = m
        .pages
        .values()
        .filter_map(|p| {
            let t = p.source_table.as_deref()?;
            let (sales, posted) = match t {
                "Sales Header" => (true, None),
                "Purchase Header" => (false, None),
                "Sales Invoice Header" | "Purch. Inv. Header" => (t.starts_with("Sales"), Some("Posted Invoice")),
                "Sales Cr.Memo Header" | "Purch. Cr. Memo Hdr." => (t.starts_with("Sales"), Some("Posted Credit Memo")),
                "Sales Shipment Header" => (true, Some("Shipment")),
                "Purch. Rcpt. Header" => (false, Some("Receipt")),
                _ => return None,
            };
            matches!(p.kind, PageKind::Document | PageKind::Card).then(|| (p.id.clone(), posted, sales))
        })
        .collect();
    for (id, posted, sales) in documents {
        let Some(p) = m.pages.get_mut(&id) else { continue };
        if p.actions.iter().any(|a| a.caption == "Co&mments") {
            continue;
        }
        let a = comments_action(if sales { "sales-comment-sheet" } else { "purch-comment-sheet" });
        let a = match posted {
            Some(t) => a.link("Document Type", &format!("={t}")),
            None => a.link("Document Type", "Document Type"),
        };
        p.actions.push(a.link("No.", "No.").link("Document Line No.", "=0"));
    }
}
