//! VAT: 254 VAT Entry (page 315), VAT statements (255 template, 257 name, 256 line; pages 318,
//! 320, 317 with Preview) and Calc. and Post VAT Settlement.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

pub const VAT_ENTRY: &str = "VAT Entry";
pub const VAT_STMT_TEMPLATE: &str = "VAT Statement Template";
pub const VAT_STMT_NAME: &str = "VAT Statement Name";
pub const VAT_STMT_LINE: &str = "VAT Statement Line";

pub const VAT_TYPES: &[&str] = &["", "Purchase", "Sale", "Settlement"];
pub const LINE_TYPES: &[&str] = &["Account Totaling", "VAT Entry Totaling", "Row Totaling", "Description"];
pub const AMOUNT_TYPES: &[&str] = &["", "Amount", "Base", "Unrealized Amount", "Unrealized Base", "Non-Deductible Amount", "Non-Deductible Base", "Full Amount", "Full Base"];
pub const SIGN: &[&str] = &["Sign", "Opposite Sign"];

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(VAT_ENTRY, 254)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Gen. Bus. Posting Group", 20),
                F::code("Gen. Prod. Posting Group", 20),
                F::date("Posting Date"),
                F::code("Document No.", 20),
                F::option("Document Type", crate::schema::tables::LEDGER_DOC_TYPES),
                F::option("Type", VAT_TYPES),
                F::decimal("Base").auto_format(1),
                F::decimal("Amount").auto_format(1),
                F::option("VAT Calculation Type", &["Normal VAT", "Reverse Charge VAT", "Full VAT", "Sales Tax"]),
                F::code("Bill-to/Pay-to No.", 20),
                F::code("User ID", 50),
                F::code("Source Code", 10),
                F::integer("Closed by Entry No."),
                F::boolean("Closed"),
                F::code("Country/Region Code", 10),
                F::integer("Transaction No."),
                F::code("External Document No.", 35),
                F::code("VAT Bus. Posting Group", 20),
                F::code("VAT Prod. Posting Group", 20),
                F::text("VAT Registration No.", 20),
                F::date("Document Date"),
                F::date("VAT Reporting Date"),
            ])
            .key(&["Entry No."])
            .pages("vat-entries", None),
    );
    m.add_table(
        TableDef::new(VAT_STMT_TEMPLATE, 255)
            .fields(vec![F::code("Name", 10).not_blank(), F::text("Description", 80)])
            .key(&["Name"])
            .describe_by("Description")
            .pages("vat-statement-templates", None),
    );
    m.add_table(
        TableDef::new(VAT_STMT_NAME, 257)
            .fields(vec![F::code("Statement Template Name", 10).relation(VAT_STMT_TEMPLATE), F::code("Name", 10), F::text("Description", 100)])
            .key(&["Statement Template Name", "Name"])
            .pages("vat-statement-names", None),
    );
    m.add_table(
        TableDef::new(VAT_STMT_LINE, 256)
            .fields(vec![
                F::code("Statement Template Name", 10).relation(VAT_STMT_TEMPLATE),
                F::code("Statement Name", 10),
                F::integer("Line No."),
                F::code("Row No.", 10),
                F::text("Description", 100),
                F::option("Type", LINE_TYPES),
                F::text("Account Totaling", 30),
                F::option("Gen. Posting Type", VAT_TYPES),
                F::code("VAT Bus. Posting Group", 20),
                F::code("VAT Prod. Posting Group", 20),
                F::text("Row Totaling", 50),
                F::option("Amount Type", AMOUNT_TYPES),
                F::option("Calculate with", SIGN),
                F::boolean("Print"),
                F::option("Print with", SIGN),
                F::boolean("New Page"),
                F::text("Box No.", 30),
            ])
            .key(&["Statement Template Name", "Statement Name", "Line No."]),
    );

    m.add_page(list(
        "vat-entries",
        315,
        "VAT Entries",
        VAT_ENTRY,
        &["Posting Date", "Document Type", "Document No.", "Type", "Gen. Bus. Posting Group", "Gen. Prod. Posting Group", "VAT Bus. Posting Group", "VAT Prod. Posting Group", "Base", "Amount", "VAT Calculation Type", "Bill-to/Pay-to No.", "Closed", "Entry No."],
        false,
    ));
    m.add_page(list("vat-statement-templates", 318, "VAT Statement Templates", VAT_STMT_TEMPLATE, &["Name", "Description"], true));
    let mut names = list("vat-statement-names", 320, "VAT Statement Names", VAT_STMT_NAME, &["Name", "Description"], true);
    names.actions = vec![Act::page("Edit VAT Statement", A::Home, "vat-statement").link("Statement Template Name", "Statement Template Name").link("Statement Name", "Name").icon("edit")];
    m.add_page(names);
    let mut stmt = list(
        "vat-statement",
        317,
        "VAT Statement",
        VAT_STMT_LINE,
        &["Row No.", "Description", "Type", "Account Totaling", "Gen. Posting Type", "VAT Bus. Posting Group", "VAT Prod. Posting Group", "Row Totaling", "Amount Type", "Calculate with", "Print", "Print with", "New Page", "Box No."],
        true,
    );
    stmt.keywords = "vat statement tax return".to_string();
    stmt.actions = vec![
        Act::command("P&review", A::Home, "vat_statement.preview")
            .icon("preview")
            .ask(&["from", "to", "selection"])
            .tooltip("Preview the VAT statement report."),
        Act::command("Calc. and Post VAT Settlement", A::Process, "vat.settlement")
            .icon("post")
            .ask(&["from", "to", "posting_date", "document_no", "settlement_account"])
            .tooltip("Close open VAT entries and transfers purchase and sales VAT amounts to the VAT settlement account. For every VAT posting group, the batch job finds all the VAT entries in the VAT Entry table that are included in the filters in the definition window."),
    ];
    m.add_page(stmt);
}
