//! Approvals (BC 91 User Setup, 454 Approval Entry, 1501 Workflow) with their pages, and the
//! approval actions on sales and purchase documents.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

pub const USER_SETUP: &str = "User Setup";
pub const APPROVAL_ENTRY: &str = "Approval Entry";
pub const WORKFLOW: &str = "Workflow";
pub const APPROVAL_COMMENT: &str = "Approval Comment Line";
pub const RESTRICTED_RECORD: &str = "Restricted Record";

/// BC's journal approval workflow templates: (code, description, category).
pub const JOURNAL_WORKFLOWS: &[(&str, &str, &str)] = &[
    ("MS-GJBAPW", "General Journal Batch Approval Workflow", "FIN"),
    ("MS-GJLAPW", "General Journal Line Approval Workflow", "FIN"),
    ("MS-IJBAPW", "Item Journal Batch Approval Workflow", "INV"),
];

pub const APPROVAL_STATUS: &[&str] = &["Created", "Open", "Canceled", "Rejected", "Approved"];
pub const APPROVAL_DOC_TYPES: &[&str] = &["Quote", "Order", "Invoice", "Credit Memo", "Blanket Order", "Return Order", " "];

/// BC's approval workflow templates per document: (sales?, document type, code, description).
pub const WORKFLOWS: &[(bool, &str, &str, &str)] = &[
    (true, "Quote", "MS-SQAPW", "Sales Quote Approval Workflow"),
    (true, "Order", "MS-SOAPW", "Sales Order Approval Workflow"),
    (true, "Invoice", "MS-SIAPW", "Sales Invoice Approval Workflow"),
    (true, "Credit Memo", "MS-SCMAPW", "Sales Credit Memo Approval Workflow"),
    (false, "Quote", "MS-PQAPW", "Purchase Quote Approval Workflow"),
    (false, "Order", "MS-POAPW", "Purchase Order Approval Workflow"),
    (false, "Invoice", "MS-PIAPW", "Purchase Invoice Approval Workflow"),
    (false, "Credit Memo", "MS-PCMAPW", "Purchase Credit Memo Approval Workflow"),
];

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(USER_SETUP, 91)
            .fields(vec![
                F::code("User ID", 50).not_blank(),
                F::code("Salespers./Purch. Code", 20),
                F::code("Approver ID", 50),
                F::integer("Sales Amount Approval Limit"),
                F::integer("Purchase Amount Approval Limit"),
                F::boolean("Unlimited Sales Approval"),
                F::boolean("Unlimited Purchase Approval"),
                F::code("Substitute", 50),
                F::text("E-Mail", 100).caption("Email"),
                F::boolean("Approval Administrator"),
            ])
            .key(&["User ID"])
            .pages("user-setup", None),
    );
    m.add_table(
        TableDef::new(APPROVAL_ENTRY, 454)
            .fields(vec![
                F::integer("Entry No."),
                F::integer("Table ID"),
                F::option("Document Type", APPROVAL_DOC_TYPES),
                F::code("Document No.", 20),
                F::integer("Sequence No."),
                F::code("Approval Code", 20),
                F::code("Sender ID", 50),
                F::code("Approver ID", 50),
                F::option("Status", APPROVAL_STATUS),
                F::text("Date-Time Sent for Approval", 30).read_only(),
                F::text("Last Date-Time Modified", 30).read_only(),
                F::code("Last Modified By User ID", 50),
                F::date("Due Date"),
                F::decimal("Amount").auto_format(1),
                F::decimal("Amount (LCY)").auto_format(1),
                F::code("Currency Code", 10),
                F::text("Record ID to Approve", 250),
            ])
            .key(&["Entry No."])
            .pages("approval-entries", None),
    );
    m.add_table(
        TableDef::new(APPROVAL_COMMENT, 455)
            .fields(vec![
                F::integer("Entry No."),
                F::integer("Table ID"),
                F::text("Record ID to Approve", 250),
                F::code("User ID", 50).read_only(),
                F::text("Date and Time", 30).read_only(),
                F::text("Comment", 250),
            ])
            .key(&["Entry No."])
            .pages("approval-comments", None),
    );
    m.add_table(
        TableDef::new(RESTRICTED_RECORD, 1550)
            .fields(vec![F::integer("ID"), F::text("Record ID", 250), F::text("Details", 250)])
            .key(&["ID"]),
    );
    m.add_table(
        TableDef::new(WORKFLOW, 1501)
            .fields(vec![F::code("Code", 20).not_blank(), F::text("Description", 100), F::boolean("Enabled"), F::code("Category", 20)])
            .key(&["Code"])
            .describe_by("Description")
            .pages("workflows", None),
    );

    m.add_page(list(
        "user-setup",
        119,
        "User Setup",
        USER_SETUP,
        &["User ID", "Approver ID", "Sales Amount Approval Limit", "Unlimited Sales Approval", "Purchase Amount Approval Limit", "Unlimited Purchase Approval", "Substitute", "E-Mail", "Approval Administrator"],
        true,
    ));
    let entry_cols = &["Document Type", "Document No.", "Sequence No.", "Status", "Sender ID", "Approver ID", "Amount", "Amount (LCY)", "Due Date", "Date-Time Sent for Approval"];
    m.add_page(list("approval-entries", 658, "Approval Entries", APPROVAL_ENTRY, entry_cols, false));
    let mut requests = list("requests-to-approve", 654, "Requests to Approve", APPROVAL_ENTRY, entry_cols, false);
    requests.actions = vec![
        Act::command("&Approve", A::Home, "approval.approve").icon("approve").tooltip("Approve the requested changes."),
        Act::command("&Reject", A::Home, "approval.reject").icon("reject").tooltip("Reject the approval request."),
        Act::command("&Delegate", A::Home, "approval.delegate").icon("delegate").tooltip("Delegate the approval to a substitute approver."),
    ];
    requests.keywords = "approve approval requests pending".to_string();
    m.add_page(requests);
    m.add_page(list("approval-comments", 660, "Approval Comments", APPROVAL_COMMENT, &["Comment", "User ID", "Date and Time"], true));
    let mut wf = list("workflows", 1500, "Workflows", WORKFLOW, &["Code", "Description", "Category", "Enabled"], true);
    wf.keywords = "approval workflow".to_string();
    m.add_page(wf);

    // Approval actions on journals: the batch, or (general journals) the selected lines.
    let journals: Vec<(String, String, bool)> = m
        .pages
        .values()
        .filter(|p| p.kind == PageKind::List)
        .filter_map(|p| {
            let item = match p.source_table.as_deref() {
                Some("Gen. Journal Line") => false,
                Some("Item Journal Line") => true,
                _ => return None,
            };
            let template = p.view.iter().find(|v| v.field == "Journal Template Name")?.value.clone();
            Some((p.id.clone(), template, item))
        })
        .collect();
    for (id, template, item) in journals {
        let Some(p) = m.pages.get_mut(&id) else { continue };
        let prefix = if item { "item_journal" } else { "journal" };
        let with = |a: Act| a.param("template", &template).param("batch", "DEFAULT");
        p.actions.push(
            with(Act::command("Send Approval Request: Journal Batch", A::Process, &format!("{prefix}.send_approval_request")))
                .param("scope", "batch")
                .icon("approve")
                .tooltip("Submit every line in this journal batch for approval, including filtered-out lines."),
        );
        p.actions.push(
            with(Act::command("Cancel Approval Request: Journal Batch", A::Process, &format!("{prefix}.cancel_approval_request")))
                .param("scope", "batch")
                .icon("reject")
                .tooltip("Withdraw approval requests for every line in this journal batch."),
        );
        if !item {
            p.actions.push(
                with(Act::command("Send Approval Request: Selected Journal Lines", A::Process, "journal.send_approval_request"))
                    .param("scope", "lines")
                    .icon("approve")
                    .tooltip("Send selected journal lines for approval."),
            );
            p.actions.push(
                with(Act::command("Cancel Approval Request: Selected Journal Lines", A::Process, "journal.cancel_approval_request"))
                    .param("scope", "lines")
                    .icon("reject")
                    .tooltip("Cancel sending selected journal lines for approval."),
            );
        }
        p.actions.push(
            with(Act::command("Approvals", A::Navigate, &format!("{prefix}.approvals")))
                .icon("approve")
                .tooltip("List pending approval requests for this batch with requester and status."),
        );
        p.actions.push(
            with(Act::command("Comments", A::Navigate, &format!("{prefix}.approval_comments")))
                .icon("comments")
                .tooltip("View or add comments for the record."),
        );
    }

    // Approval actions on sales and purchase documents (not on posted ones).
    let docs: Vec<(String, bool)> = m
        .pages
        .values()
        .filter(|p| p.kind == PageKind::Document)
        .filter_map(|p| match p.source_table.as_deref() {
            Some("Sales Header") => Some((p.id.clone(), true)),
            Some("Purchase Header") => Some((p.id.clone(), false)),
            _ => None,
        })
        .collect();
    for (id, sales) in docs {
        let Some(p) = m.pages.get_mut(&id) else { continue };
        let prefix = if sales { "sales" } else { "purchase" };
        p.actions.push(
            Act::command("Send A&pproval Request", A::Process, &format!("{prefix}.document.send_approval_request"))
                .icon("approve")
                .tooltip("Request approval of the document."),
        );
        p.actions.push(
            Act::command("Cancel Approval Re&quest", A::Process, &format!("{prefix}.document.cancel_approval_request"))
                .icon("reject")
                .tooltip("Cancel the approval request."),
        );
        p.actions.push(
            Act::page("Approvals", A::Navigate, "approval-entries")
                .link("Document Type", "Document Type")
                .link("Document No.", "No.")
                .icon("approve")
                .tooltip("View a list of the records that are waiting to be approved."),
        );
    }
}
