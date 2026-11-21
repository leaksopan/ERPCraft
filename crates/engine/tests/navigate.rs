#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! BC's Navigate ("Find entries..."): related entries of a document number.

use erpcraft_engine::Session;
use serde_json::json;

#[test]
fn finds_the_posted_invoice_and_its_entries() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let r = s.run("navigate", &json!({"table": "Sales Invoice Header", "key": "PS-INV103001"})).unwrap();
    let rows = r["report"]["rows"].as_array().unwrap();
    let tables: Vec<&str> = rows.iter().map(|row| row[0].as_str().unwrap()).collect();
    // Item entries of an order carry the shipment number, as in BC.
    for t in ["Sales Invoice Header", "G/L Entry", "Cust. Ledger Entry", "Detailed Cust. Ledg. Entry"] {
        assert!(tables.contains(&t), "{t} in {tables:?}");
    }
    assert_eq!(rows[0][1], 1, "one posted invoice");
    let links = r["report"]["links"].as_array().unwrap();
    assert_eq!(links.len(), rows.len());
    assert!(links.iter().any(|l| l["page"] == "general-ledger-entries"));
    let none = s.run("navigate", &json!({"document_no": "NOPE"})).unwrap();
    assert!(none["report"]["rows"].as_array().unwrap().is_empty());
    assert!(none["message"].as_str().unwrap().contains("no posted entries"));
}
