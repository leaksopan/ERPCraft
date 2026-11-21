#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Fiscal years: Create Year, Close Year, Close Income Statement, and budgets in financial reports.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn row<'a>(rep: &'a Value, no: &str) -> &'a Vec<Value> {
    rep["report"]["rows"].as_array().unwrap().iter().find(|r| r[0] == no).and_then(Value::as_array).unwrap()
}

fn num(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}

#[test]
fn create_year_adds_twelve_periods_once() {
    let mut s = demo();
    let r = s.run("accounting_period.create_year", &json!({"starting_date": "2027-01-01"})).unwrap();
    assert_eq!(r["periods"].as_array().unwrap().len(), 12);
    let p = s.run("record.get", &json!({"table": "Accounting Period", "key": "2027-01-01"})).unwrap();
    assert_eq!(p["New Fiscal Year"], true);
    assert_eq!(p["Name"], "January");
    assert!(s.run("accounting_period.create_year", &json!({"starting_date": "2027-01-01"})).unwrap_err().message.contains("already exists"));
}

#[test]
fn closing_a_year_moves_its_result_to_retained_earnings() {
    let mut s = demo();
    // Rent paid in December 2025.
    s.run("record.new", &json!({"page": "general-journal", "values": {"Posting Date": "2025-12-15", "Document No.": "R2025", "Account No.": "60200", "Debit Amount": 1_000_000, "Bal. Account No.": "10110"}})).unwrap();
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
    assert!(
        s.run("accounting_period.close_income_statement", &json!({"fiscal_year_ending_date": "2025-12-31", "document_no": "CLOSE25", "retained_earnings_account": "30300"}))
            .unwrap_err()
            .message
            .contains("must be closed")
    );
    let r = s.run("accounting_period.close_year", &json!({})).unwrap();
    assert!(r["message"].as_str().unwrap().contains("2025-01-01 to 2025-12-31"), "{r}");
    let p = s.run("record.get", &json!({"table": "Accounting Period", "key": "2025-06-01"})).unwrap();
    assert_eq!((p["Closed"].as_bool(), p["Date Locked"].as_bool()), (Some(true), Some(true)));
    // 2026 can't close: no later year.
    assert!(s.run("accounting_period.close_year", &json!({})).unwrap_err().message.contains("create a new fiscal year"));

    let r = s.run("accounting_period.close_income_statement", &json!({"document_no": "CLOSE25", "retained_earnings_account": "30300"})).unwrap();
    assert_eq!(r["lines"], 1, "{r}");
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
    let closing = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Document No.": "CLOSE25"}})).unwrap();
    assert!(closing["records"].as_array().unwrap().iter().all(|e| e["Source Code"] == "CLSINCOME"));

    // The 2025 income statement still shows the rent: the closing entry falls after the year.
    let is = s.run("financial_report.overview", &json!({"name": "INCOME", "from": "2025-01-01", "to": "2025-12-31"})).unwrap();
    assert_eq!(num(&row(&is, "41")[2]), 1_000_000.0, "{is}");
    // Now: the rent sits in retained earnings, and the balance sheet still balances.
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    assert_eq!(num(&row(&bs, "32")[2]), -1_000_000.0, "{bs}");
    assert!((num(&row(&bs, "19")[2]) - num(&row(&bs, "40")[2])).abs() < 0.01);
}

#[test]
fn the_income_statement_compares_with_the_budget() {
    let mut s = demo();
    let is = s.run("financial_report.overview", &json!({"name": "INCOME", "from": "2026-01-01", "to": "2026-03-31"})).unwrap();
    let rent = row(&is, "41");
    assert_eq!(num(&rent[6]), 45_000_000.0, "three months of rent budget: {is}");
    assert!((num(&rent[7]) - (num(&rent[2]) - 45_000_000.0)).abs() < 0.01, "variance = actual - budget");
    assert_eq!(num(&row(&is, "11")[6]), 60_000_000.0, "revenue budgets show positive like the actuals");
}
