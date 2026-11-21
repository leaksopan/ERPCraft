//! Fiscal years and budgets: 50 Accounting Period (page 100 with Create Year, Close Year and
//! Close Income Statement), 95 G/L Budget Name and 96 G/L Budget Entry (pages 121, 120).

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

pub const ACCOUNTING_PERIOD: &str = "Accounting Period";
pub const GL_BUDGET_NAME: &str = "G/L Budget Name";
pub const GL_BUDGET_ENTRY: &str = "G/L Budget Entry";

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(ACCOUNTING_PERIOD, 50)
            .fields(vec![F::date("Starting Date"), F::text("Name", 10), F::boolean("New Fiscal Year"), F::boolean("Closed").read_only(), F::boolean("Date Locked").read_only()])
            .key(&["Starting Date"])
            .pages("accounting-periods", None),
    );
    m.add_table(
        TableDef::new(GL_BUDGET_NAME, 95)
            .fields(vec![F::code("Name", 10).not_blank(), F::text("Description", 80), F::boolean("Blocked")])
            .key(&["Name"])
            .describe_by("Description")
            .pages("gl-budget-names", None),
    );
    m.add_table(
        TableDef::new(GL_BUDGET_ENTRY, 96)
            .fields(vec![
                F::integer("Entry No."),
                F::code("Budget Name", 10).relation(GL_BUDGET_NAME),
                F::code("G/L Account No.", 20).relation("G/L Account"),
                F::date("Date"),
                F::code("Global Dimension 1 Code", 20),
                F::code("Global Dimension 2 Code", 20),
                F::decimal("Amount").auto_format(1),
                F::text("Description", 100),
                F::code("User ID", 50).read_only(),
                F::date("Last Date-Modified").read_only().caption("Last Date Modified"),
            ])
            .key(&["Entry No."]),
    );

    let mut periods = list("accounting-periods", 100, "Accounting Periods", ACCOUNTING_PERIOD, &["Starting Date", "Name", "New Fiscal Year", "Closed", "Date Locked"], true);
    periods.keywords = "fiscal year period close year".to_string();
    periods.actions = vec![
        Act::command("&Create Year", A::Process, "accounting_period.create_year")
            .icon("new")
            .ask(&["starting_date", "no_of_periods", "period_length"])
            .tooltip("Open a new fiscal year and define its accounting periods so you can start posting documents."),
        Act::command("C&lose Year", A::Process, "accounting_period.close_year")
            .icon("close")
            .confirm("This function closes the fiscal year. Once the fiscal year is closed it cannot be opened again, and the periods in the fiscal year cannot be changed. Do you want to close the fiscal year?")
            .tooltip("Close the current fiscal year. A confirmation message will display that tells you which year will be closed. You cannot reopen the year after it has been closed."),
        Act::command("Close Income Statement", A::Process, "accounting_period.close_income_statement")
            .icon("post")
            .ask(&["fiscal_year_ending_date", "document_no", "retained_earnings_account", "post_to_retained_earnings"])
            .tooltip("Start the transfer of the year's results to an account in the balance sheet and close the income statement accounts."),
        Act::page("Trial Balance by &Period", A::Report, "chart-of-accounts").icon("report"),
    ];
    m.add_page(periods);
    let mut names = list("gl-budget-names", 121, "G/L Budgets", GL_BUDGET_NAME, &["Name", "Description", "Blocked"], true);
    names.keywords = "budget".to_string();
    names.actions = vec![Act::page("G/L Budget Entries", A::Navigate, "gl-budget-entries").link("Budget Name", "Name").icon("list")];
    m.add_page(names);
    m.add_page(list(
        "gl-budget-entries",
        120,
        "G/L Budget Entries",
        GL_BUDGET_ENTRY,
        &["Budget Name", "Date", "G/L Account No.", "Description", "Global Dimension 1 Code", "Global Dimension 2 Code", "Amount", "Entry No."],
        true,
    ));
    if let Some(t) = m.tables.get_mut(crate::schema::fin_reports::COLUMN_LAYOUT) {
        t.fields.push(F::code("Budget Name", 10).relation(GL_BUDGET_NAME));
    }
}
