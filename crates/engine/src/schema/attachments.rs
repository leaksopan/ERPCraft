//! Document Attachment (BC 1173) with the Attached Documents page, and the Attachments, Attach a
//! file and Attach as PDF actions on cards, documents and posted documents.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, FieldType, Metadata, PageDef, PageKind, TableDef};

pub const DOC_ATTACHMENT: &str = "Document Attachment";
pub const ATTACH_DOC_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order"];
const FILE_TYPES: &[&str] = &["", "Image", "PDF", "Word", "Excel", "PowerPoint", "Email", "XML", "Other"];

/// Tables whose pages get attachments: masters, documents and posted documents.
const OWNERS: &[&str] = &[
    "Customer",
    "Vendor",
    "Item",
    "G/L Account",
    "Bank Account",
    "Fixed Asset",
    "Sales Header",
    "Purchase Header",
    "Sales Invoice Header",
    "Sales Cr.Memo Header",
    "Sales Shipment Header",
    "Purch. Inv. Header",
    "Purch. Cr. Memo Hdr.",
    "Purch. Rcpt. Header",
];

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(DOC_ATTACHMENT, 1173)
            .fields(vec![
                F::integer("ID"),
                F::integer("Table ID"),
                F::code("No.", 20),
                F::text("Attached Date", 30).read_only(),
                F::text("File Name", 250),
                F::new("File Type", FieldType::option(FILE_TYPES)).read_only(),
                F::text("File Extension", 30).read_only(),
                F::new("Content", FieldType::Blob).read_only(),
                F::code("User", 50).read_only(),
                F::boolean("Document Flow Purchase"),
                F::boolean("Document Flow Sales"),
                F::new("Document Type", FieldType::option(ATTACH_DOC_TYPES)),
                F::integer("Line No."),
            ])
            .key(&["Table ID", "No.", "Document Type", "Line No.", "ID"])
            .pages("attached-documents", None),
    );
    let mut page = PageDef::new("attached-documents", 1173, "Attached Documents", PageKind::List, Some(DOC_ATTACHMENT));
    page.columns = ["File Name", "File Extension", "File Type", "User", "Attached Date", "Document Flow Purchase", "Document Flow Sales"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    page.actions.push(Act::command("View", A::Home, "attachment.view").icon("preview").tooltip("View the file."));
    m.add_page(page);

    let owners: Vec<(String, u32, bool, bool)> = m
        .pages
        .values()
        .filter(|p| matches!(p.kind, PageKind::Card | PageKind::Document))
        .filter_map(|p| {
            let t = m.tables.get(p.source_table.as_deref()?)?;
            OWNERS.contains(&t.name.as_str()).then(|| (p.id.clone(), t.object_id, t.field("Document Type").is_some(), p.kind == PageKind::Document && p.editable))
        })
        .collect();
    for (id, table_id, has_doc_type, open_document) in owners {
        let Some(p) = m.pages.get_mut(&id) else { continue };
        if p.actions.iter().any(|a| a.caption == "Attachments") {
            continue;
        }
        let mut list = Act::page("Attachments", A::Navigate, "attached-documents")
            .link("Table ID", &format!("={table_id}"))
            .link("No.", "No.")
            .icon("attach")
            .tooltip("Add a file as an attachment. You can attach images as well as documents.");
        list = if has_doc_type { list.link("Document Type", "Document Type") } else { list.link("Document Type", "=Quote") };
        p.actions.push(list);
        p.actions.push(Act::command("Attach a file...", A::Process, "attachment.attach_file").icon("attach").ask(&["path"]).tooltip("Attach a file to this record."));
        if open_document || p.source_table.as_deref().is_some_and(|t| t.contains("Inv.") || t.contains("Invoice") || t.contains("Cr.") || t.contains("Shipment") || t.contains("Rcpt.")) {
            p.actions.push(
                Act::command("Attach as PDF", A::Report, "document.attach_as_pdf")
                    .icon("print")
                    .tooltip("Create a PDF file and attach it to the document."),
            );
        }
    }
}
