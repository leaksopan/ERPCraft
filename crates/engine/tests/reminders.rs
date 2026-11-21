#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Reminders (levels, fees), finance charge memos (interest) and the customer statement.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    for (f, v) in [("Reminder Terms Code", "DOMESTIC"), ("Fin. Charge Terms Code", "1.5 DOM.")] {
        s.run("record.validate", &json!({"table": "Customer", "key": "20000", "field": f, "value": v})).unwrap();
    }
    s
}

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn gl(s: &mut Session, account: &str) -> f64 {
    s.run("record.list", &json!({"table": "G/L Entry", "filters": {"G/L Account No.": account}})).unwrap()["records"].as_array().unwrap().iter().map(|e| d(&e["Amount"])).sum()
}

#[test]
fn reminders_climb_the_levels_and_the_second_charges_a_fee() {
    let mut s = demo();
    let r = s.run("reminder.create", &json!({"reminder_date": "2026-12-31"})).unwrap();
    let nos = r["reminders"].as_array().unwrap().clone();
    assert_eq!(nos.len(), 1, "only customer 20000 has reminder terms: {r}");
    let no = nos[0].as_str().unwrap().to_string();
    let h = s.run("record.get", &json!({"table": "Reminder Header", "key": no})).unwrap();
    assert_eq!((h["Customer No."].as_str(), h["Reminder Level"].as_i64()), (Some("20000"), Some(1)));
    assert!(d(&h["Remaining Amount"]) > 0.0, "{h}");
    assert_eq!(d(&h["Additional Fee"]), 0.0, "level 1 has no fee");
    let issued = s.run("reminder.issue", &json!({"no": no})).unwrap();
    assert_eq!(issued["open"]["page"], "issued-reminder");
    assert!(s.run("record.get", &json!({"table": "Reminder Header", "key": no})).is_err(), "the reminder is filed");
    let entries = s.run("record.list", &json!({"table": "Reminder/Fin. Charge Entry", "filters": {"Customer No.": "20000"}})).unwrap();
    assert!(!entries["records"].as_array().unwrap().is_empty());

    // The next reminder is level 2, with its 50,000 fee posted to the customer.
    let r = s.run("reminder.create", &json!({"reminder_date": "2026-12-31"})).unwrap();
    let no = r["reminders"][0].as_str().unwrap().to_string();
    let h = s.run("record.get", &json!({"table": "Reminder Header", "key": no})).unwrap();
    assert_eq!(h["Reminder Level"], 2);
    assert_eq!(d(&h["Additional Fee"]), 50_000.0);
    let issued = s.run("reminder.issue", &json!({"no": no})).unwrap();
    let ino = issued["issued_no"].as_str().unwrap();
    let cle = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": ino}})).unwrap()["records"][0];
    assert_eq!((cle["Document Type"].as_str(), d(&cle["Amount"])), (Some("Reminder"), 50_000.0));
    assert_eq!(gl(&mut s, "40800"), -50_000.0);
}

#[test]
fn finance_charges_charge_interest_for_the_days_late() {
    let mut s = demo();
    let r = s.run("finance_charge.create", &json!({"document_date": "2026-12-31"})).unwrap();
    let no = r["memos"][0].as_str().unwrap().to_string();
    let lines = s.run("record.list", &json!({"table": "Finance Charge Memo Line", "filters": {"Finance Charge Memo No.": no}})).unwrap();
    let line = &lines["records"][0];
    let (rem, interest) = (d(&line["Remaining Amount"]), d(&line["Amount"]));
    let due = chrono::NaiveDate::parse_from_str(line["Due Date"].as_str().unwrap(), "%Y-%m-%d").unwrap();
    let days = (chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap() - due).num_days() as f64;
    assert!((interest - rem * 0.015 * days / 30.0).abs() < 0.01, "1.5% per 30 days for {days} days: {interest}");
    assert!(line["Description"].as_str().unwrap().ends_with("is overdue"));
    s.run("finance_charge.issue", &json!({"no": no})).unwrap();
    assert!((gl(&mut s, "40900") + interest).abs() < 0.01, "the interest is income");
}

#[test]
fn the_statement_shows_the_period_with_a_running_balance_and_aging() {
    let mut s = demo();
    let st = s.run("customer.statement", &json!({"table": "Customer", "key": "20000", "from": "2026-01-01"})).unwrap();
    let rows = st["report"]["rows"].as_array().unwrap();
    let closing = d(&rows.last().unwrap()[6]);
    let customer = s.run("record.get", &json!({"table": "Customer", "key": "20000"})).unwrap();
    assert!((closing - d(&customer["Balance (LCY)"])).abs() < 0.01, "{st}");
    let aging: f64 = st["report"]["sections"][0]["rows"].as_array().unwrap().iter().map(|r| d(&r[1])).sum();
    assert!((aging - closing).abs() < 0.01, "the aging adds up to the balance");
}
