#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Customer / Vendor Entry Statistics and Statistics by Currencies (BC pages 302, 303, 486, 487).

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn row<'a>(rows: &'a Value, first: &str) -> &'a Vec<Value> {
    rows.as_array().unwrap().iter().find(|r| r[0] == first).and_then(Value::as_array).unwrap()
}

#[test]
fn customer_entry_statistics_show_the_last_invoice_and_counts() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let inv = posted["posted_no"].as_str().or_else(|| posted["invoice_no"].as_str()).unwrap().to_string();
    let r = s.run("customer.entry_statistics", &json!({"table": "Customer", "key": "10000"})).unwrap();
    let rep = &r["report"];
    assert_eq!(rep["title"], "Customer Entry Statistics - 10000");
    let last_inv = row(&rep["rows"], "Invoice");
    assert_eq!(last_inv[2], inv.as_str(), "{rep}");
    assert!(last_inv[4].as_str().unwrap().parse::<f64>().unwrap() > 0.0, "invoices show positive");
    let docs = &rep["sections"][0];
    assert_eq!(docs["caption"], "No. of Documents");
    assert_eq!(docs["columns"].as_array().unwrap().len(), 5);
    let invoices = row(&docs["rows"], "Invoices");
    let to_date = invoices[4].as_i64().unwrap();
    assert!(to_date >= 1);
    assert!(docs["rows"].as_array().unwrap().iter().any(|r| r[0] == "Largest Balance (LCY)"));
    let rem = row(&rep["sections"][1]["rows"], "Invoice");
    assert!(rem[1].as_str().unwrap().parse::<f64>().unwrap() > 0.0);
}

#[test]
fn vendor_entry_statistics_and_currencies() {
    let mut s = demo();
    let r = s.run("vendor.entry_statistics", &json!({"no": "10000"})).unwrap();
    assert!(r["report"]["sections"][0]["rows"].as_array().unwrap().iter().any(|r| r[0] == "Pmt. Disc. Received (LCY)"));
    let doc = s.run("sales.document.new", &json!({"document_type": "Order", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    assert!(doc["no"].is_string());
    let c = s.run("customer.statistics_by_currencies", &json!({"no": "10000"})).unwrap();
    let lcy = row(&c["report"]["rows"], "");
    let outstanding: f64 = lcy[3].as_str().unwrap().parse().unwrap();
    assert!(outstanding > 0.0, "{c}");
    let total: f64 = lcy[5].as_str().unwrap().parse().unwrap();
    let parts: f64 = (2..=4).map(|i| lcy[i].as_str().unwrap().parse::<f64>().unwrap()).sum();
    assert!((total - parts).abs() < 0.001);
    let v = s.run("vendor.statistics_by_currencies", &json!({"no": "10000"})).unwrap();
    assert_eq!(v["report"]["columns"][4], "Amt. Rcd. Not Invoiced");
    assert!(s.run("customer.entry_statistics", &json!({"no": "NOPE"})).is_err());
}
