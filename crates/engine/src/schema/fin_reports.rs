//! Financial reports (BC's account schedules): row definitions (84 Acc. Schedule Name, 85 Acc.
//! Schedule Line), column definitions (333 Column Layout Name, 334 Column Layout) and the
//! financial reports that pair them (88 Financial Report), with their pages.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

pub const ACC_SCHED_NAME: &str = "Acc. Schedule Name";
pub const ACC_SCHED_LINE: &str = "Acc. Schedule Line";
pub const COLUMN_LAYOUT_NAME: &str = "Column Layout Name";
pub const COLUMN_LAYOUT: &str = "Column Layout";
pub const FINANCIAL_REPORT: &str = "Financial Report";

pub const TOTALING_TYPES: &[&str] = &["Posting Accounts", "Total Accounts", "Formula", "Set Base For Percent", "Account Category"];
pub const LINE_SHOW: &[&str] = &["Yes", "No", "If Any Column Not Zero", "When Positive Balance", "When Negative Balance"];
pub const AMOUNT_TYPES: &[&str] = &["Net Amount", "Debit Amount", "Credit Amount"];
pub const ROW_TYPES: &[&str] = &["Net Change", "Balance at Date", "Beginning Balance"];
pub const COLUMN_TYPES: &[&str] =
    &["Formula", "Net Change", "Balance at Date", "Beginning Balance", "Year to Date", "Rest of Fiscal Year", "Entire Fiscal Year", "Month to Date"];
pub const ENTRY_TYPES: &[&str] = &["Entries", "Budget Entries"];
pub const COLUMN_SHOW: &[&str] = &["Always", "Never", "When Positive", "When Negative"];
pub const ROUNDING: &[&str] = &["None", "1", "1000", "1000000"];

fn list(id: &str, object_id: u32, caption: &str, table: &str, columns: &[&str], editable: bool) -> PageDef {
    let mut p = PageDef::new(id, object_id, caption, PageKind::List, Some(table));
    p.columns = columns.iter().map(|c| (*c).to_string()).collect();
    p.editable = editable;
    p
}

pub fn register(m: &mut Metadata) {
    m.add_table(
        TableDef::new(ACC_SCHED_NAME, 84)
            .fields(vec![
                F::code("Name", 10).not_blank(),
                F::text("Description", 80),
                F::code("Default Column Layout", 10).relation(COLUMN_LAYOUT_NAME),
                F::code("Analysis View Name", 10),
            ])
            .key(&["Name"])
            .describe_by("Description")
            .pages("account-schedule-names", None),
    );
    m.add_table(
        TableDef::new(ACC_SCHED_LINE, 85)
            .fields(vec![
                F::code("Schedule Name", 10).relation(ACC_SCHED_NAME),
                F::integer("Line No."),
                F::code("Row No.", 10),
                F::text("Description", 100),
                F::text("Totaling", 250),
                F::option("Totaling Type", TOTALING_TYPES),
                F::boolean("New Page"),
                F::integer("Indentation"),
                F::option("Show", LINE_SHOW),
                F::boolean("Bold"),
                F::boolean("Italic"),
                F::boolean("Underline"),
                F::boolean("Double Underline"),
                F::boolean("Show Opposite Sign"),
                F::option("Row Type", ROW_TYPES),
                F::option("Amount Type", AMOUNT_TYPES),
            ])
            .key(&["Schedule Name", "Line No."]),
    );
    m.add_table(
        TableDef::new(COLUMN_LAYOUT_NAME, 333)
            .fields(vec![F::code("Name", 10).not_blank(), F::text("Description", 80), F::code("Analysis View Name", 10)])
            .key(&["Name"])
            .describe_by("Description")
            .pages("column-layout-names", None),
    );
    m.add_table(
        TableDef::new(COLUMN_LAYOUT, 334)
            .fields(vec![
                F::code("Column Layout Name", 10).relation(COLUMN_LAYOUT_NAME),
                F::integer("Line No."),
                F::code("Column No.", 10),
                F::text("Column Header", 30),
                F::option("Column Type", COLUMN_TYPES),
                F::option("Ledger Entry Type", ENTRY_TYPES),
                F::option("Amount Type", AMOUNT_TYPES),
                F::code("Formula", 80),
                F::code("Comparison Date Formula", 20),
                F::boolean("Show Opposite Sign"),
                F::option("Show", COLUMN_SHOW),
                F::option("Rounding Factor", ROUNDING),
            ])
            .key(&["Column Layout Name", "Line No."]),
    );
    m.add_table(
        TableDef::new(FINANCIAL_REPORT, 88)
            .fields(vec![
                F::code("Name", 10).not_blank(),
                F::text("Description", 80),
                F::code("Financial Report Row Group", 10).relation(ACC_SCHED_NAME).caption("Row Definition"),
                F::code("Financial Report Column Group", 10).relation(COLUMN_LAYOUT_NAME).caption("Column Definition"),
                F::text("DateFilter", 250).caption("Date Filter"),
            ])
            .key(&["Name"])
            .describe_by("Description")
            .pages("financial-reports", None),
    );

    let mut reports = list("financial-reports", 108, "Financial Reports", FINANCIAL_REPORT, &["Name", "Description", "Financial Report Row Group", "Financial Report Column Group"], true);
    reports.keywords = "account schedule balance sheet income statement financial report".to_string();
    reports.actions = vec![
        Act::command("View Financial Report", A::Home, "financial_report.overview")
            .icon("report")
            .tooltip("View the financial report with the row and column definitions it uses."),
        Act::page("Edit Row Definition", A::Process, "account-schedule").link("Schedule Name", "Financial Report Row Group").icon("edit").tooltip("Edit the row definition of this financial report."),
        Act::page("Edit Column Definition", A::Process, "column-layout")
            .link("Column Layout Name", "Financial Report Column Group")
            .icon("edit")
            .tooltip("Edit the column definition of this financial report."),
        Act::page("Row Definitions", A::Navigate, "account-schedule-names").icon("list"),
        Act::page("Column Definitions", A::Navigate, "column-layout-names").icon("list"),
    ];
    m.add_page(reports);

    let mut names = list("account-schedule-names", 103, "Row Definitions", ACC_SCHED_NAME, &["Name", "Description", "Default Column Layout", "Analysis View Name"], true);
    names.actions = vec![Act::page("Edit Row Definition", A::Home, "account-schedule").link("Schedule Name", "Name").icon("edit").tooltip("Edit the rows of this row definition.")];
    m.add_page(names);
    m.add_page(list(
        "account-schedule",
        104,
        "Row Definition",
        ACC_SCHED_LINE,
        &["Row No.", "Description", "Totaling Type", "Totaling", "Row Type", "Amount Type", "Show Opposite Sign", "Show", "Bold", "Italic", "Underline", "Double Underline", "New Page", "Indentation"],
        true,
    ));
    let mut cols = list("column-layout-names", 488, "Column Definitions", COLUMN_LAYOUT_NAME, &["Name", "Description", "Analysis View Name"], true);
    cols.actions = vec![Act::page("Edit Column Definition", A::Home, "column-layout").link("Column Layout Name", "Name").icon("edit").tooltip("Edit the columns of this column definition.")];
    m.add_page(cols);
    m.add_page(list(
        "column-layout",
        489,
        "Column Definition",
        COLUMN_LAYOUT,
        &["Column No.", "Column Header", "Column Type", "Ledger Entry Type", "Amount Type", "Formula", "Show Opposite Sign", "Comparison Date Formula", "Show", "Rounding Factor"],
        true,
    ));
}
