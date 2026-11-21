#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! BC's document functions: Preview Posting, Test Report, Post and New, Copy Document, and the
//! Pro Forma Invoice / Work Order prints.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn count(s: &mut Session, table: &str) -> usize {
    s.run("record.list", &json!({"table": table, "limit": 100000})).unwrap()["records"].as_array().unwrap().len()
}

fn lines(s: &mut Session, doc_type: &str, no: &str) -> Vec<Value> {
    s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document Type": doc_type, "Document No.": no}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn preview_posting_lists_the_entries_and_posts_nothing() {
    let mut s = demo();
    let gl_before = count(&mut s, "G/L Entry");
    let r = s.run("sales.document.preview_post", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    let rows = r["report"]["rows"].as_array().unwrap();
    let related: Vec<&str> = rows.iter().map(|row| row[0].as_str().unwrap()).collect();
    assert!(related.contains(&"G/L Entry") && related.contains(&"Cust. Ledger Entry") && related.contains(&"Item Ledger Entry"), "{related:?}");
    let gl_new = r["entries"]["G/L Entry"].as_array().unwrap();
    let total: f64 = gl_new.iter().map(|e| e["Amount"].as_f64().or_else(|| e["Amount"].as_str().and_then(|v| v.parse().ok())).unwrap()).sum();
    assert!(total.abs() < 0.001, "the previewed G/L entries balance");
    assert_eq!(count(&mut s, "G/L Entry"), gl_before, "nothing was posted");
    assert_eq!(s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"]})).unwrap()["No."], "S-ORD1002");
}

#[test]
fn test_report_says_what_would_stop_posting() {
    let mut s = demo();
    let ok = s.run("sales.document.test_report", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert_eq!(ok["ok"], true);
    s.run("record.validate", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"], "field": "Posting Date", "value": ""})).unwrap();
    let bad = s.run("sales.document.test_report", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert_eq!(bad["ok"], false);
    assert!(bad["report"]["rows"][0][0].as_str().unwrap().contains("Posting Date"));
}

#[test]
fn post_and_new_posts_and_opens_an_empty_order() {
    let mut s = demo();
    let r = s.run("sales.document.post_and_new", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    let new_no = r["no"].as_str().unwrap().to_string();
    assert_ne!(new_no, "S-ORD1002");
    assert_eq!(r["open"]["page"], "sales-order");
    assert!(s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"]})).is_err(), "the order was posted");
    assert!(lines(&mut s, "Order", &new_no).is_empty());
}

#[test]
fn copy_document_copies_lines_and_customer_from_a_posted_invoice() {
    let mut s = demo();
    let new = s.run("sales.document.new", &json!({"document_type": "Order"})).unwrap();
    let no = new["no"].as_str().unwrap().to_string();
    let r = s
        .run("sales.document.copy", &json!({"document_type": "Order", "no": no, "from_document_type": "Posted Invoice", "from_no": "PS-INV103001", "include_header": "Yes"}))
        .unwrap();
    assert!(r["lines"].as_u64().unwrap() > 0);
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).unwrap();
    assert_eq!(h["Sell-to Customer No."], "10000");
    let src = s.run("record.list", &json!({"table": "Sales Invoice Line", "filters": {"Document No.": "PS-INV103001"}})).unwrap()["records"].as_array().unwrap().clone();
    let copied = lines(&mut s, "Order", &no);
    assert_eq!(copied.len(), src.len());
    assert_eq!(copied[0]["No."], src[0]["No."]);
    assert_eq!(copied[0]["Unit Price"], src[0]["Unit Price"], "prices are copied without Recalculate Lines");
    assert!(s.run("sales.document.copy", &json!({"document_type": "Order", "no": no, "from_document_type": "Order", "from_no": no})).is_err());
}

#[test]
fn pro_forma_and_work_order_prints() {
    let mut s = demo();
    let pf = s.run("document.print", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"], "report": "pro_forma"})).unwrap();
    assert!(pf["title"].as_str().unwrap().starts_with("Pro Forma Invoice"));
    let wo = s.run("document.print", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"], "report": "work_order"})).unwrap();
    assert!(wo["title"].as_str().unwrap().starts_with("Work Order"));
    assert!(s.run("document.print", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"], "report": "nonsense"})).is_err());
}

#[test]
fn select_items_adds_a_line_per_item() {
    let mut s = demo();
    let before = lines(&mut s, "Order", "S-ORD1002").len();
    let r = s.run("sales.document.select_items", &json!({"document_type": "Order", "no": "S-ORD1002", "items": ["1000", "1001"]})).unwrap();
    assert_eq!(r["lines"], 2);
    let after = lines(&mut s, "Order", "S-ORD1002");
    assert_eq!(after.len(), before + 2);
    assert_eq!(after.last().unwrap()["No."], "1001");
    assert!(s.run("sales.document.select_items", &json!({"document_type": "Order", "no": "S-ORD1002", "items": []})).is_err());
}

#[test]
fn comments_number_their_lines_and_go_with_the_posted_invoice() {
    let mut s = demo();
    for text in ["Deliver before noon", "Call the workshop first"] {
        s.run("record.new", &json!({"table": "Sales Comment Line", "values": {"Document Type": "Order", "No.": "S-ORD1002", "Comment": text}})).unwrap();
    }
    let list = s.run("record.list", &json!({"table": "Sales Comment Line", "filters": {"No.": "S-ORD1002"}})).unwrap();
    let lines: Vec<i64> = list["records"].as_array().unwrap().iter().map(|c| c["Line No."].as_i64().unwrap()).collect();
    assert_eq!(lines, vec![10000, 20000]);
    let posted = s.run("sales.document.post", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    let copied = s.run("record.list", &json!({"table": "Sales Comment Line", "filters": {"Document Type": "Posted Invoice", "No.": inv}})).unwrap();
    assert_eq!(copied["records"].as_array().unwrap().len(), 2);
    let master = s.run("record.new", &json!({"table": "Comment Line", "values": {"Table Name": "Customer", "No.": "10000", "Comment": "Prefers WhatsApp"}})).unwrap();
    assert_eq!(master["record"]["Line No."], 10000);
}

#[test]
fn post_and_print_and_posted_statistics() {
    let mut s = demo();
    let r = s.run("sales.document.post_and_print", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert!(r["title"].as_str().unwrap().starts_with("Sales - Invoice"), "{r}");
    assert!(!r["pdf_base64"].as_str().unwrap().is_empty());
    let inv = r["posted_no"].as_str().unwrap().to_string();
    let st = s.run("document.posted_statistics", &json!({"table": "Sales Invoice Header", "key": inv})).unwrap();
    let rows = st["report"]["rows"].as_array().unwrap();
    assert_eq!(rows[0][0], "Amount");
    assert!(rows.iter().any(|row| row[0] == "Profit %"));
    assert!(s.run("document.posted_statistics", &json!({"table": "Customer", "key": "10000"})).is_err());
}

#[test]
fn customer_and_vendor_statistics_on_documents() {
    let mut s = demo();
    let order = s.run("record.list", &json!({"page": "sales-order-list"})).unwrap();
    let o = &order["records"][0];
    let r = s.run("sales.document.customer_statistics", &json!({"document_type": "Order", "no": o["No."]})).unwrap();
    assert!(r.to_string().contains("Balance (LCY)"), "{r}");
    let po = s.run("record.list", &json!({"page": "purchase-order-list"})).unwrap();
    let p = &po["records"][0];
    let r = s.run("purchase.document.vendor_statistics", &json!({"table": "Purchase Header", "key": ["Order", p["No."]]})).unwrap();
    assert!(r.to_string().contains("Purchases (LCY)"), "{r}");
}

#[test]
fn post_batch_posts_what_it_can_and_lists_the_rest() {
    let mut s = demo();
    let good = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    let empty = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "20000"})).unwrap();
    let r = s
        .run(
            "sales.document.post_batch",
            &json!({"document_type": "Invoice", "nos": [good["no"], empty["no"]], "posting_date": "2026-11-30", "replace_posting_date": true}),
        )
        .unwrap();
    assert_eq!(r["posted"], json!([good["no"]]), "{r}");
    assert_eq!(r["report"]["title"], "Error Messages");
    assert_eq!(r["report"]["rows"][0][0], empty["no"]);
    assert!(s.run("record.get", &json!({"table": "Sales Header", "key": ["Invoice", good["no"]]})).is_err(), "posted and gone");
    assert!(s.run("record.get", &json!({"table": "Sales Header", "key": ["Invoice", empty["no"]]})).is_ok(), "the failure leaves its document");
    let inv = s.run("record.list", &json!({"table": "Sales Invoice Header", "filters": {"Pre-Assigned No.": good["no"]}})).unwrap();
    assert_eq!(inv["records"][0]["Posting Date"], "2026-11-30");
    let bad = s.run("sales.document.post_batch", &json!({"document_type": "Order", "ship": false, "invoice": false}));
    assert!(bad.unwrap_err().message.contains("Ship or Invoice"));
    assert!(s.run("sales.document.post_batch", &json!({"document_type": "Invoice", "replace_posting_date": true})).unwrap_err().message.contains("Enter the posting date"));
}
