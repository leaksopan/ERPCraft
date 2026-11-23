#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Payment discounts: the terms' discount on the document and entry, granted when paid in time.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn gl(s: &mut Session, account: &str) -> f64 {
    s.run("record.list", &json!({"table": "G/L Entry", "filters": {"G/L Account No.": account}})).unwrap()["records"].as_array().unwrap().iter().map(|e| d(&e["Amount"])).sum()
}

#[test]
fn paying_within_the_discount_period_grants_the_discount() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("record.validate", &json!({"table": "Customer", "key": "30000", "field": "Payment Terms Code", "value": "2% 8 DAYS"})).unwrap();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "30000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Invoice", doc["no"]]})).unwrap();
    assert_eq!(d(&h["Payment Discount %"]), 2.0);
    let expected_disc_date = (s.work_date() + chrono::Duration::days(8)).to_string();
    assert_eq!(h["Pmt. Discount Date"], expected_disc_date, "8 days after the document date");
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    let e = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    let (amount, disc) = (d(&e["Amount"]), d(&e["Original Pmt. Disc. Possible"]));
    assert!((disc - (amount * 0.02 * 100.0).round() / 100.0).abs() < 0.01, "2% of {amount}: {disc}");
    // Paid on time, less the discount: the invoice closes and the discount is an expense.
    s.run(
        "record.new",
        &json!({"page": "general-journal", "values": {"Document No.": "PAY-DISC", "Account Type": "Customer", "Account No.": "30000", "Amount": -(amount - disc),
                 "Bal. Account No.": "10120", "Applies-to Doc. Type": "Invoice", "Applies-to Doc. No.": inv}}),
    )
    .unwrap();
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
    let e = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    assert_eq!((e["Open"].as_bool(), d(&e["Remaining Amount"])), (Some(false), 0.0));
    assert!((gl(&mut s, "60900") - disc).abs() < 0.01);
    // Unapplied, the invoice is open for its full amount and the discount possible again.
    let pay = s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": "PAY-DISC"}})).unwrap()["records"][0]["Entry No."].clone();
    s.run("customer.entries.unapply", &json!({"entry_no": pay})).unwrap();
    let e = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    assert_eq!((e["Open"].as_bool(), d(&e["Remaining Amount"]), d(&e["Remaining Pmt. Disc. Possible"])), (Some(true), amount, disc));
    assert!(gl(&mut s, "60900").abs() < 0.01);
}
