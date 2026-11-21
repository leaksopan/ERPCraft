#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Financial reports (BC's account schedules) on the demo company.

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
fn the_balance_sheet_balances() {
    let mut s = demo();
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    assert_eq!(bs["report"]["columns"], json!(["Row No.", "Description", "Balance", "Prior Year", "Change"]));
    let assets = num(&row(&bs, "19")[2]);
    let le = num(&row(&bs, "40")[2]);
    assert!(assets > 0.0, "{bs}");
    assert!((assets - le).abs() < 0.01, "assets {assets} = liabilities and equity {le}: {bs}");
    // Change = Balance - Prior Year.
    let prior = num(&row(&bs, "19")[3]);
    assert!((num(&row(&bs, "19")[4]) - (assets - prior)).abs() < 0.01);
    assert_eq!(row(&bs, "10")[2], "", "a heading shows no amount");
}

#[test]
fn the_income_statement_and_formulas_follow_the_entries() {
    let mut s = demo();
    let is = s.run("financial_report.overview", &json!({"table": "Financial Report", "key": "INCOME"})).unwrap();
    let revenue = num(&row(&is, "19")[2]);
    let gross = num(&row(&is, "30")[2]);
    let cogs = num(&row(&is, "20")[2]);
    assert!(revenue > 0.0, "{is}");
    assert!((gross - (revenue - cogs)).abs() < 0.01);
    // The Balance Sheet's net income row is the Income Statement's net income.
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    assert!((num(&row(&bs, "33")[2]) - num(&row(&is, "50")[2])).abs() < 0.01);
    // A period with nothing posted.
    let empty = s.run("financial_report.overview", &json!({"name": "INCOME", "from": "2020-01-01", "to": "2020-12-31"})).unwrap();
    assert_eq!(num(&row(&empty, "50")[2]), 0.0);
    // A broken formula names its row; an unknown report is refused.
    s.run("record.validate", &json!({"table": "Acc. Schedule Line", "key": ["INCOME", 80000], "field": "Totaling", "value": "19-"})).unwrap();
    assert!(s.run("financial_report.overview", &json!({"name": "INCOME"})).unwrap_err().message.contains("Row 30"));
    assert!(s.run("financial_report.overview", &json!({"name": "NOPE"})).is_err());
}

