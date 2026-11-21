//! Reminders and finance charge memos: terms (292 Reminder Terms, 293 Reminder Level, 5 Finance
//! Charge Terms), documents (295 / 296 Reminder, 302 / 303 Finance Charge Memo), issued documents
//! (297 / 298, 304 / 305) and 300 Reminder/Fin. Charge Entry, with their pages.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, CalcFormula, CalcFunc, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const REMINDER_TERMS: &str = "Reminder Terms";
pub const REMINDER_LEVEL: &str = "Reminder Level";
pub const FIN_CHARGE_TERMS: &str = "Finance Charge Terms";
pub const REMINDER_HEADER: &str = "Reminder Header";
pub const REMINDER_LINE: &str = "Reminder Line";
pub const ISSUED_REMINDER_HEADER: &str = "Issued Reminder Header";
pub const ISSUED_REMINDER_LINE: &str = "Issued Reminder Line";
pub const FIN_CHARGE_HEADER: &str = "Finance Charge Memo Header";
pub const FIN_CHARGE_LINE: &str = "Finance Charge Memo Line";
pub const ISSUED_FIN_CHARGE_HEADER: &str = "Issued Fin. Charge Memo Header";
pub const ISSUED_FIN_CHARGE_LINE: &str = "Issued Fin. Charge Memo Line";
pub const REMINDER_ENTRY: &str = "Reminder/Fin. Charge Entry";

const CUSTOMER_T: &str = crate::schema::tables::CUSTOMER;
pub const LINE_TYPES: &[&str] = &["", "G/L Account", "Customer Ledger Entry", "Line Fee"];

fn header_fields(level: bool) -> Vec<F> {
    let mut v = vec![
        F::code("No.", 20),
        F::code("Customer No.", 20).relation("Customer"),
        F::text("Name", 100),
        F::text("Address", 100),
        F::text("City", 30),
        F::code("Post Code", 20),
        F::code("Country/Region Code", 10),
        F::date("Posting Date"),
        F::date("Document Date"),
        F::date("Due Date"),
        F::code("Currency Code", 10).relation("Currency"),
        F::code("Customer Posting Group", 20),
        F::text("Posting Description", 100),
        F::text("Your Reference", 35),
        F::code("Pre-Assigned No.", 20),
        F::code("User ID", 50),
    ];
    if level {
        v.push(F::code("Reminder Terms Code", 10).relation(REMINDER_TERMS));
        v.push(F::integer("Reminder Level"));
    } else {
        v.push(F::code("Fin. Charge Terms Code", 10).relation(FIN_CHARGE_TERMS));
    }
    v
}

fn line_fields(doc_field: &str) -> Vec<F> {
    vec![
        F::code(doc_field, 20),
        F::integer("Line No."),
        F::option("Type", LINE_TYPES),
        F::integer("Entry No."),
        F::code("No.", 20),
        F::date("Posting Date"),
        F::date("Document Date"),
        F::date("Due Date"),
        F::option("Document Type", crate::schema::tables::LEDGER_DOC_TYPES),
        F::code("Document No.", 20),
        F::text("Description", 100),
        F::decimal("Original Amount").auto_format(1),
        F::decimal("Remaining Amount").auto_format(1),
        F::decimal("Amount").auto_format(1),
        F::decimal("Interest Rate"),
        F::integer("No. of Reminders"),
    ]
}

fn sum_lines(table: &str, doc_field: &str, field: &str, fixed: &[(&str, &str)]) -> CalcFormula {
    CalcFormula::new(CalcFunc::Sum, table, field, &[(doc_field, "No.")], fixed)
}

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], card: Option<&str>, editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.card_page = card.map(str::to_string);
    p.editable = editable;
    p
}

#[allow(clippy::too_many_arguments)]
fn document_pages(m: &mut Metadata, ids: (&str, u32, &str, u32, &str, u32), header: &str, line: &str, doc_field: &str, caption: (&str, &str), issued: bool, actions: Vec<Act>) {
    let (list_id, list_obj, card_id, card_obj, sub_id, sub_obj) = ids;
    m.add_page(list(list_id, list_obj, caption.0, header, &["No.", "Customer No.", "Name", "Currency Code", "Posting Date"], Some(card_id), false));
    let mut sub = PageDef::new(sub_id, sub_obj, "Lines", PageKind::ListPart, Some(line));
    sub.columns = ["Type", "Entry No.", "Posting Date", "Document Date", "Due Date", "Document Type", "Document No.", "Description", "Original Amount", "Remaining Amount", "Amount"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    sub.editable = !issued;
    m.add_page(sub);
    let mut card = PageDef::new(card_id, card_obj, caption.1, PageKind::Document, Some(header))
        .group(G::new("General", &["No.", "Customer No.", "Name", "Address", "City", "Post Code", "Posting Date", "Document Date", "Due Date"]))
        .part("Lines", sub_id, &[(doc_field, "No.")]);
    if issued {
        card = card.read_only();
    }
    for a in actions {
        card = card.action(a);
    }
    m.add_page(card);
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(REMINDER_TERMS, 292)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::text("Description", 100),
                F::boolean("Post Interest"),
                F::boolean("Post Additional Fee"),
                F::integer("Max. No. of Reminders"),
                F::decimal("Minimum Amount (LCY)").auto_format(1),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("reminder-terms", None),
    );
    m.add_table(
        TableDef::new(REMINDER_LEVEL, 293)
            .fields(vec![
                F::code("Reminder Terms Code", 10).relation(REMINDER_TERMS),
                F::integer("No."),
                F::code("Grace Period", 20),
                F::decimal("Additional Fee (LCY)").auto_format(1),
                F::boolean("Calculate Interest"),
                F::code("Due Date Calculation", 20),
            ])
            .key(&["Reminder Terms Code", "No."]),
    );
    m.add_table(
        TableDef::new(FIN_CHARGE_TERMS, 5)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::decimal("Interest Rate"),
                F::decimal("Minimum Amount (LCY)").auto_format(1),
                F::decimal("Additional Fee (LCY)").auto_format(1),
                F::text("Description", 100),
                F::option("Interest Calculation Method", &["Average Daily Balance", "Balance Due"]),
                F::integer("Interest Period (Days)"),
                F::code("Grace Period", 20),
                F::code("Due Date Calculation", 20),
                F::boolean("Post Interest"),
                F::boolean("Post Additional Fee"),
                F::text("Line Description", 100),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("finance-charge-terms", None),
    );
    let rem_sum = |field: &str, fixed: &[(&str, &str)]| sum_lines(REMINDER_LINE, "Reminder No.", field, fixed);
    let mut rh = header_fields(true);
    rh.push(F::decimal("Remaining Amount").calc(rem_sum("Remaining Amount", &[])).auto_format(1));
    rh.push(F::decimal("Additional Fee").calc(rem_sum("Amount", &[("Type", "G/L Account")])).auto_format(1));
    m.add_table(TableDef::new(REMINDER_HEADER, 295).fields(rh).key(&["No."]).pages("reminder-list", Some("reminder")));
    m.add_table(TableDef::new(REMINDER_LINE, 296).fields(line_fields("Reminder No.")).key(&["Reminder No.", "Line No."]));
    let mut ih = header_fields(true);
    ih.push(F::decimal("Remaining Amount").calc(sum_lines(ISSUED_REMINDER_LINE, "Reminder No.", "Remaining Amount", &[])).auto_format(1));
    ih.push(F::decimal("Additional Fee").calc(sum_lines(ISSUED_REMINDER_LINE, "Reminder No.", "Amount", &[("Type", "G/L Account")])).auto_format(1));
    m.add_table(TableDef::new(ISSUED_REMINDER_HEADER, 297).fields(ih).key(&["No."]).pages("issued-reminder-list", Some("issued-reminder")));
    m.add_table(TableDef::new(ISSUED_REMINDER_LINE, 298).fields(line_fields("Reminder No.")).key(&["Reminder No.", "Line No."]));
    let fin_sum = |table: &str, field: &str, fixed: &[(&str, &str)]| sum_lines(table, "Finance Charge Memo No.", field, fixed);
    let mut fh = header_fields(false);
    fh.push(F::decimal("Remaining Amount").calc(fin_sum(FIN_CHARGE_LINE, "Remaining Amount", &[])).auto_format(1));
    fh.push(F::decimal("Interest Amount").calc(fin_sum(FIN_CHARGE_LINE, "Amount", &[("Type", "Customer Ledger Entry")])).auto_format(1));
    fh.push(F::decimal("Additional Fee").calc(fin_sum(FIN_CHARGE_LINE, "Amount", &[("Type", "G/L Account")])).auto_format(1));
    m.add_table(TableDef::new(FIN_CHARGE_HEADER, 302).fields(fh).key(&["No."]).pages("finance-charge-memo-list", Some("finance-charge-memo")));
    m.add_table(TableDef::new(FIN_CHARGE_LINE, 303).fields(line_fields("Finance Charge Memo No.")).key(&["Finance Charge Memo No.", "Line No."]));
    let mut ifh = header_fields(false);
    ifh.push(F::decimal("Interest Amount").calc(fin_sum(ISSUED_FIN_CHARGE_LINE, "Amount", &[("Type", "Customer Ledger Entry")])).auto_format(1));
    ifh.push(F::decimal("Additional Fee").calc(fin_sum(ISSUED_FIN_CHARGE_LINE, "Amount", &[("Type", "G/L Account")])).auto_format(1));
    m.add_table(TableDef::new(ISSUED_FIN_CHARGE_HEADER, 304).fields(ifh).key(&["No."]).pages("issued-fin-charge-memo-list", Some("issued-finance-charge-memo")));
    m.add_table(TableDef::new(ISSUED_FIN_CHARGE_LINE, 305).fields(line_fields("Finance Charge Memo No.")).key(&["Finance Charge Memo No.", "Line No."]));
    m.add_table(
        TableDef::new(REMINDER_ENTRY, 300)
            .fields(vec![
                F::integer("Entry No."),
                F::code("No.", 20),
                F::option("Type", &["Reminder", "Finance Charge Memo"]),
                F::date("Posting Date"),
                F::date("Document Date"),
                F::integer("Customer Entry No."),
                F::integer("Reminder Level"),
                F::boolean("Interest Posted"),
                F::decimal("Interest Amount").auto_format(1),
                F::option("Customer Entry Document Type", crate::schema::tables::LEDGER_DOC_TYPES).caption("Document Type"),
                F::code("Customer Entry Document No.", 20).caption("Document No."),
                F::decimal("Remaining Amount").auto_format(1),
                F::code("Customer No.", 20),
                F::code("User ID", 50),
                F::date("Due Date"),
            ])
            .key(&["Entry No."])
            .pages("reminder-fin-charge-entries", None),
    );
    for (table, fields) in [
        (crate::schema::tables::SALES_SETUP, vec![F::code("Reminder Nos.", 20).relation("No. Series"), F::code("Issued Reminder Nos.", 20).relation("No. Series"), F::code("Fin. Chrg. Memo Nos.", 20).relation("No. Series"), F::code("Issued Fin. Chrg. M. Nos.", 20).relation("No. Series")]),
        (CUSTOMER_T, vec![F::code("Reminder Terms Code", 10).relation(REMINDER_TERMS), F::code("Fin. Charge Terms Code", 10).relation(FIN_CHARGE_TERMS)]),
        ("Customer Posting Group", vec![F::code("Additional Fee Account", 20).relation("G/L Account"), F::code("Interest Account", 20).relation("G/L Account")]),
    ] {
        if let Some(t) = m.tables.get_mut(table) {
            for f in fields {
                if t.field(&f.name).is_none() {
                    t.fields.push(f);
                }
            }
        }
    }
    if let Some(t) = m.tables.get_mut(crate::schema::tables::CUST_LEDGER) {
        if t.field("Last Issued Reminder Level").is_none() {
            t.fields.push(F::integer("Last Issued Reminder Level"));
        }
    }

    m.add_page(list("reminder-terms", 431, "Reminder Terms", REMINDER_TERMS, &["Code", "Description", "Max. No. of Reminders", "Post Interest", "Post Additional Fee", "Minimum Amount (LCY)"], None, true));
    if let Some(p) = m.pages.get_mut("reminder-terms") {
        p.actions.push(Act::page("&Levels", A::Navigate, "reminder-levels").link("Reminder Terms Code", "Code").icon("list"));
    }
    m.add_page(list("reminder-levels", 432, "Reminder Levels", REMINDER_LEVEL, &["No.", "Grace Period", "Due Date Calculation", "Additional Fee (LCY)", "Calculate Interest"], None, true));
    m.add_page(list(
        "finance-charge-terms",
        6,
        "Finance Charge Terms",
        FIN_CHARGE_TERMS,
        &["Code", "Description", "Interest Calculation Method", "Interest Rate", "Interest Period (Days)", "Minimum Amount (LCY)", "Additional Fee (LCY)", "Grace Period", "Due Date Calculation", "Post Interest", "Post Additional Fee"],
        None,
        true,
    ));
    m.add_page(list("reminder-fin-charge-entries", 444, "Reminder/Fin. Charge Entries", REMINDER_ENTRY, &["Posting Date", "Type", "No.", "Reminder Level", "Customer Entry Document Type", "Customer Entry Document No.", "Remaining Amount", "Interest Amount", "Interest Posted", "Entry No."], None, false));

    document_pages(
        m,
        ("reminder-list", 436, "reminder", 434, "reminder-subform", 435),
        REMINDER_HEADER,
        REMINDER_LINE,
        "Reminder No.",
        ("Reminders", "Reminder"),
        false,
        vec![
            Act::command("&Issue", A::Home, "reminder.issue").icon("post").shortcut("F9").confirm("Do you want to issue the reminder?").tooltip("Post the specified reminder entries according to your specifications in the Reminder Terms window."),
            Act::command("&Suggest Reminder Lines", A::Process, "reminder.suggest_lines").icon("new").tooltip("Create reminder lines in existing reminders for any overdue payments based on information in the Reminder window."),
        ],
    );
    if let Some(p) = m.pages.get_mut("reminder-list") {
        p.actions.push(Act::command("&Create Reminders", A::Process, "reminder.create").icon("new").ask(&["reminder_date"]).tooltip("Create reminders for one or more customers with overdue payments."));
        p.actions.push(Act::command("Issue Reminders", A::Process, "reminder.issue_all").icon("post").confirm("Do you want to issue all the reminders?").tooltip("Issue all the reminders."));
        p.keywords = "reminder overdue dunning collection".to_string();
    }
    document_pages(m, ("issued-reminder-list", 440, "issued-reminder", 438, "issued-reminder-lines", 439), ISSUED_REMINDER_HEADER, ISSUED_REMINDER_LINE, "Reminder No.", ("Issued Reminders", "Issued Reminder"), true, vec![]);
    document_pages(
        m,
        ("finance-charge-memo-list", 448, "finance-charge-memo", 446, "fin-charge-memo-subform", 447),
        FIN_CHARGE_HEADER,
        FIN_CHARGE_LINE,
        "Finance Charge Memo No.",
        ("Finance Charge Memos", "Finance Charge Memo"),
        false,
        vec![Act::command("&Issue", A::Home, "finance_charge.issue").icon("post").shortcut("F9").confirm("Do you want to issue the finance charge memo?").tooltip("Post the specified finance charge entries according to your specifications in the Finance Charge Terms window.")],
    );
    if let Some(p) = m.pages.get_mut("finance-charge-memo-list") {
        p.actions.push(Act::command("C&reate Finance Charge Memos", A::Process, "finance_charge.create").icon("new").ask(&["document_date"]).tooltip("Create finance charge memos for one or more customers with overdue payments."));
        p.keywords = "finance charge interest overdue".to_string();
    }
    document_pages(
        m,
        ("issued-fin-charge-memo-list", 452, "issued-finance-charge-memo", 450, "issued-fin-charge-memo-lines", 451),
        ISSUED_FIN_CHARGE_HEADER,
        ISSUED_FIN_CHARGE_LINE,
        "Finance Charge Memo No.",
        ("Issued Fin. Charge Memos", "Issued Finance Charge Memo"),
        true,
        vec![],
    );
    // Customers: their statement.
    for page in ["customer-card", "customer-list"] {
        if let Some(p) = m.pages.get_mut(page) {
            p.actions.push(
                Act::command("Statement", A::Report, "customer.statement")
                    .icon("report")
                    .ask(&["from", "to"])
                    .tooltip("View a list of a customer's transactions for a selected period, for example, to send to the customer at the close of an accounting period. You can choose to have all overdue balances displayed regardless of the period specified, or you can choose to include an aging band."),
            );
        }
    }
}
