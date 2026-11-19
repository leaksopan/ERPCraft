#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End-to-end business flows through `Session::run`, the same entry point the UI, CLI and MCP use.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn dec(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

#[test]
fn demo_company_is_consistent_and_balanced() {
    let mut s = demo();
    let tb = s.run("report.trial_balance", &json!({})).unwrap();
    assert_eq!(tb["balanced"], true, "{tb:#}");
    let customers = s.run("record.list", &json!({"table": "Customer"})).unwrap();
    assert_eq!(customers["total"], 5);
    // 10000 bought, then paid 2,000,000.
    let c = s.run("record.get", &json!({"table": "Customer", "key": "10000"})).unwrap();
    assert!(dec(&c["Balance (LCY)"]) > 0.0);
    assert_eq!(dec(&c["Payments (LCY)"]), 2_000_000.0);
    let item = s.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap();
    assert_eq!(dec(&item["Inventory"]), 34.0); // 40 received, 6 sold
}

#[test]
fn sales_order_posts_to_all_ledgers() {
    let mut s = demo();
    let r = s
        .run("sales.document.new", &json!({"document_type": "Order", "customer_no": "20000",
            "lines": [{"no": "1100", "quantity": 2}, {"type": "G/L Account", "no": "40300", "quantity": 1, "unit_price": 100000}]}))
        .unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).unwrap();
    // 2 × 650,000 + 100,000 = 1,400,000; PPN 11% = 154,000.
    assert_eq!(dec(&h["Amount"]), 1_400_000.0);
    assert_eq!(dec(&h["Amount Including VAT"]), 1_554_000.0);
    assert_eq!(h["Due Date"].as_str().is_some(), true);

    let gl_before = s.run("record.list", &json!({"table": "G/L Entry", "limit": 0})).unwrap()["total"].as_u64().unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Order", "no": no})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    assert_eq!(posted["source_document_deleted"], true);
    assert!(s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).is_err());

    let pinv = s.run("record.get", &json!({"table": "Sales Invoice Header", "key": inv})).unwrap();
    assert_eq!(dec(&pinv["Amount Including VAT"]), 1_554_000.0);
    assert_eq!(dec(&pinv["Remaining Amount"]), 1_554_000.0);
    let gl_after = s.run("record.list", &json!({"table": "G/L Entry", "limit": 0})).unwrap()["total"].as_u64().unwrap();
    // receivable, 2 revenue lines, 2 VAT lines, COGS + inventory.
    assert_eq!(gl_after - gl_before, 7);
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);

    // Pay it in full: the invoice closes.
    s.run("payment.customer.register", &json!({"customer_no": "20000", "amount": 1554000, "applies_to_doc_no": inv})).unwrap();
    let pinv = s.run("record.get", &json!({"table": "Sales Invoice Header", "key": inv})).unwrap();
    assert_eq!(pinv["Closed"], true);
}

#[test]
fn partial_shipment_keeps_the_order() {
    let mut s = demo();
    let r = s.run("sales.document.new", &json!({"customer_no": "30000", "lines": [{"no": "1200", "quantity": 4}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Sales Line", "key": ["Order", no, 10000], "field": "Qty. to Ship", "value": 1})).unwrap();
    let p = s.run("sales.document.post", &json!({"no": no})).unwrap();
    assert_eq!(p["source_document_deleted"], false);
    let line = s.run("record.get", &json!({"table": "Sales Line", "key": ["Order", no, 10000]})).unwrap();
    assert_eq!(dec(&line["Quantity Invoiced"]), 1.0);
    assert_eq!(dec(&line["Outstanding Quantity"]), 3.0);
}

#[test]
fn failed_command_rolls_back() {
    let mut s = demo();
    let before = s.run("record.list", &json!({"table": "Sales Header", "limit": 0})).unwrap()["total"].clone();
    // Unknown item on the second line: the whole command fails, the new order is not kept.
    let e = s.run("sales.document.new", &json!({"customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}, {"no": "NOPE", "quantity": 1}]}));
    assert!(e.is_err());
    let after = s.run("record.list", &json!({"table": "Sales Header", "limit": 0})).unwrap()["total"].clone();
    assert_eq!(before, after);
}

#[test]
fn business_rules_refuse() {
    let mut s = demo();
    // Released orders can't take new lines.
    let r = s.run("sales.document.new", &json!({"customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    s.run("sales.document.release", &json!({"no": no})).unwrap();
    let e = s.run("sales.line.add", &json!({"no": no, "line_no": "1001", "quantity": 1})).unwrap_err();
    assert!(e.message.contains("Status must be equal to 'Open'"), "{e}");
    // Blocked customers can't get documents.
    s.run("record.validate", &json!({"table": "Customer", "key": "50000", "field": "Blocked", "value": "All"})).unwrap();
    assert!(s.run("sales.document.new", &json!({"customer_no": "50000"})).is_err());
    // Relations are checked.
    let e = s.run("record.validate", &json!({"table": "Customer", "key": "10000", "field": "Payment Terms Code", "value": "XYZ"})).unwrap_err();
    assert!(e.message.contains("cannot be found in the related table"), "{e}");
    // Ledger entries can't be deleted.
    assert!(s.run("record.delete", &json!({"table": "G/L Entry", "key": 1})).is_err());
}

#[test]
fn quote_becomes_order_and_new_customer_gets_a_number() {
    let mut s = demo();
    let quotes = s.run("record.list", &json!({"page": "sales-quotes"})).unwrap();
    let q = quotes["records"][0]["No."].as_str().unwrap().to_string();
    let r = s.run("sales.quote.make_order", &json!({"document_type": "Quote", "no": q})).unwrap();
    let order = r["no"].as_str().unwrap().to_string();
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", order]})).unwrap();
    assert_eq!(h["Quote No."], json!(q));

    let c = s.run("record.new", &json!({"table": "Customer", "values": {"Name": "Bengkel Baru"}})).unwrap();
    assert_eq!(c["record"]["No."], "C00010");
    assert_eq!(c["record"]["Search Name"], "BENGKEL BARU");
}

#[test]
fn data_file_roundtrip() {
    let dir = std::env::temp_dir().join(format!("erpcraft-test-{}", std::process::id()));
    let path = dir.join("company.json");
    let _ = std::fs::remove_file(&path);
    {
        let mut s = Session::open(&path).unwrap();
        s.run("company.init", &json!({})).unwrap();
    }
    let mut s = Session::open(&path).unwrap();
    assert_eq!(s.run("record.list", &json!({"table": "Item", "limit": 0})).unwrap()["total"], 8);
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
    let _ = std::fs::remove_dir_all(&dir);
}
