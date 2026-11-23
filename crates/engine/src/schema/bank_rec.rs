//! Bank account reconciliation: 273 Bank Acc. Reconciliation and 274 its lines (pages 388, 379,
//! 380), 275 / 276 the posted Bank Account Statement (pages 389, 383, 384), and the statement
//! fields of the bank account ledger entries.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, CalcFormula, CalcFunc, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const BANK_REC: &str = "Bank Acc. Reconciliation";
pub const BANK_REC_LINE: &str = "Bank Acc. Reconciliation Line";
pub const BANK_STMT: &str = "Bank Account Statement";
pub const BANK_STMT_LINE: &str = "Bank Account Statement Line";
pub const STATEMENT_STATUS: &[&str] = &["Open", "Bank Acc. Entry Applied", "Check Entry Applied", "Closed"];

fn line_fields() -> Vec<F> {
    vec![
        F::code("Bank Account No.", 20).relation("Bank Account"),
        F::code("Statement No.", 20),
        F::integer("Statement Line No."),
        F::code("Document No.", 20),
        F::date("Transaction Date"),
        F::text("Description", 100),
        F::decimal("Statement Amount").auto_format(1),
        F::decimal("Difference").auto_format(1).read_only(),
        F::decimal("Applied Amount").auto_format(1).read_only(),
        F::integer("Applied Entries").read_only(),
        F::date("Value Date"),
        F::code("Check No.", 20),
    ]
}

pub fn register(m: &mut Metadata) {
    if let Some(t) = m.tables.get_mut(crate::schema::tables::BANK_ACCOUNT) {
        for f in [F::code("Last Statement No.", 20), F::decimal("Balance Last Statement").auto_format(1)] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
    if let Some(t) = m.tables.get_mut(crate::schema::tables::BANK_LEDGER) {
        for f in [F::option("Statement Status", STATEMENT_STATUS), F::code("Statement No.", 20), F::integer("Statement Line No.")] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
    let sum_lines = |field: &str| CalcFormula::new(CalcFunc::Sum, BANK_REC_LINE, field, &[("Bank Account No.", "Bank Account No."), ("Statement No.", "Statement No.")], &[]);
    m.add_table(
        TableDef::new(BANK_REC, 273)
            .fields(vec![
                F::code("Bank Account No.", 20).relation("Bank Account"),
                F::code("Statement No.", 20),
                F::decimal("Statement Ending Balance").auto_format(1),
                F::date("Statement Date"),
                F::decimal("Balance Last Statement").auto_format(1),
                F::decimal("Total Balance on Bank Account")
                    .calc(CalcFormula::new(CalcFunc::Sum, crate::schema::tables::BANK_LEDGER, "Amount", &[("Bank Account No.", "Bank Account No.")], &[]))
                    .auto_format(1),
                F::decimal("Total Applied Amount").calc(sum_lines("Applied Amount")).auto_format(1),
                F::decimal("Total Transaction Amount").calc(sum_lines("Statement Amount")).auto_format(1),
                F::decimal("Total Difference").calc(sum_lines("Difference")).auto_format(1),
            ])
            .key(&["Bank Account No.", "Statement No."])
            .pages("bank-acc-reconciliation-list", Some("bank-acc-reconciliation")),
    );
    m.add_table(TableDef::new(BANK_REC_LINE, 274).fields(line_fields()).key(&["Bank Account No.", "Statement No.", "Statement Line No."]));
    m.add_table(
        TableDef::new(BANK_STMT, 275)
            .fields(vec![
                F::code("Bank Account No.", 20).relation("Bank Account"),
                F::code("Statement No.", 20),
                F::decimal("Statement Ending Balance").auto_format(1),
                F::date("Statement Date"),
                F::decimal("Balance Last Statement").auto_format(1),
                F::text("Bank Account Name", 100),
                F::decimal("G/L Balance at Posting Date").auto_format(1),
            ])
            .key(&["Bank Account No.", "Statement No."])
            .pages("bank-account-statement-list", Some("bank-account-statement")),
    );
    m.add_table(TableDef::new(BANK_STMT_LINE, 276).fields(line_fields()).key(&["Bank Account No.", "Statement No.", "Statement Line No."]));

    let line_cols = &["Transaction Date", "Value Date", "Document No.", "Check No.", "Description", "Statement Amount", "Applied Amount", "Difference", "Applied Entries"];
    let mut list = PageDef::new("bank-acc-reconciliation-list", 388, "Bank Account Reconciliations", PageKind::List, Some(BANK_REC));
    list.columns = ["Bank Account No.", "Statement No.", "Statement Date", "Balance Last Statement", "Statement Ending Balance"].iter().map(|c| (*c).to_string()).collect();
    list.card_page = Some("bank-acc-reconciliation".to_string());
    list.keywords = "bank reconciliation statement".to_string();
    m.add_page(list);
    let mut lines = PageDef::new("bank-acc-reconciliation-lines", 380, "Lines", PageKind::ListPart, Some(BANK_REC_LINE));
    lines.columns = line_cols.iter().map(|c| (*c).to_string()).collect();
    lines.editable = true;
    m.add_page(lines);
    m.add_page(
        PageDef::new("bank-acc-reconciliation", 379, "Bank Acc. Reconciliation", PageKind::Document, Some(BANK_REC))
            .group(G::new("General", &["Bank Account No.", "Statement No.", "Statement Date", "Balance Last Statement", "Statement Ending Balance"]))
            .part("Lines", "bank-acc-reconciliation-lines", &[("Bank Account No.", "Bank Account No."), ("Statement No.", "Statement No.")])
            .group(G::new("Totals", &["Total Balance on Bank Account", "Total Transaction Amount", "Total Applied Amount", "Total Difference"]))
            .action(
                Act::command("&Suggest Lines", A::Process, "bank_rec.suggest_lines")
                    .icon("new")
                    .tooltip("Populate bank statement lines from open bank ledger entries."),
            )
            .action(
                Act::command("Match Automatically", A::Process, "bank_rec.match")
                    .icon("apply")
                    .tooltip("Match bank statement lines to open bank ledger entries by date and amount."),
            )
            .action(
                Act::command("Remove Match", A::Process, "bank_rec.remove_match")
                    .icon("reopen")
                    .tooltip("Clear existing matches between statement lines and ledger entries."),
            )
            .action(
                Act::command("P&ost", A::Home, "bank_rec.post")
                    .icon("post")
                    .shortcut("F9")
                    .confirm("Do you want to post the bank account reconciliation?")
                    .tooltip("Post the reconciled bank statement and mark matched ledger entries as closed."),
            ),
    );

    let mut stmts = PageDef::new("bank-account-statement-list", 389, "Bank Account Statements", PageKind::List, Some(BANK_STMT));
    stmts.columns = ["Bank Account No.", "Statement No.", "Statement Date", "Balance Last Statement", "Statement Ending Balance"].iter().map(|c| (*c).to_string()).collect();
    stmts.card_page = Some("bank-account-statement".to_string());
    m.add_page(stmts);
    let mut slines = PageDef::new("bank-account-statement-lines", 384, "Lines", PageKind::ListPart, Some(BANK_STMT_LINE));
    slines.columns = line_cols.iter().map(|c| (*c).to_string()).collect();
    m.add_page(slines);
    m.add_page(
        PageDef::new("bank-account-statement", 383, "Bank Account Statement", PageKind::Document, Some(BANK_STMT))
            .read_only()
            .group(G::new("General", &["Bank Account No.", "Statement No.", "Statement Date", "Balance Last Statement", "Statement Ending Balance"]))
            .part("Lines", "bank-account-statement-lines", &[("Bank Account No.", "Bank Account No."), ("Statement No.", "Statement No.")]),
    );
}
