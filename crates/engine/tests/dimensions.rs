#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Dimensions: default dimensions on masters, sets on documents and lines, and the entries posting
//! makes carrying them (BC's DimensionManagement).

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn get(s: &mut Session, table: &str, key: Value) -> Value {
    s.run("record.get", &json!({"table": table, "key": key})).unwrap()
}

fn set_of(s: &mut Session, id: i64) -> Vec<(String, String)> {
    let r = s.run("record.list", &json!({"table": "Dimension Set Entry", "filters": {"Dimension Set ID": id.to_string()}})).unwrap();
    let mut v: Vec<(String, String)> = r["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["Dimension Code"].as_str().unwrap().to_string(), e["Dimension Value Code"].as_str().unwrap().to_string()))
        .collect();
    v.sort();
    v
}

#[test]
fn a_cards_global_dimension_is_its_default_dimension() {
    let mut s = demo();
    s.run("record.validate", &json!({"table": "Customer", "key": "30000", "field": "Global Dimension 2 Code", "value": "jkt"})).unwrap();
    let d = get(&mut s, "Default Dimension", json!([18, "30000", "AREA"]));
    assert_eq!(d["Dimension Value Code"], "JKT");
    s.run("record.validate", &json!({"table": "Customer", "key": "30000", "field": "Global Dimension 2 Code", "value": ""})).unwrap();
    assert!(s.run("record.get", &json!({"table": "Default Dimension", "key": [18, "30000", "AREA"]})).is_err());
    let bad = s.run("record.validate", &json!({"table": "Customer", "key": "30000", "field": "Global Dimension 2 Code", "value": "MARS"}));
    assert!(bad.unwrap_err().message.contains("has not been set up"));
}

#[test]
fn documents_take_the_customers_and_items_dimensions_and_posting_keeps_them() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Order", "customer_no": "20000", "lines": [{"no": "9000", "quantity": 1}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let h = get(&mut s, "Sales Header", json!(["Order", no]));
    assert_eq!((h["Shortcut Dimension 1 Code"].as_str(), h["Shortcut Dimension 2 Code"].as_str()), (Some("SALES"), Some("BDG")));
    let header_set = h["Dimension Set ID"].as_i64().unwrap();
    assert_eq!(set_of(&mut s, header_set), vec![("AREA".into(), "BDG".into()), ("DEPARTMENT".into(), "SALES".into())]);
    let lines = s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document Type": "Order", "Document No.": no}})).unwrap();
    let line = &lines["records"][0];
    assert_eq!(line["Shortcut Dimension 1 Code"], "SERVICE", "the service item's department wins on the line");
    assert_eq!(line["Shortcut Dimension 2 Code"], "BDG", "the area comes from the header");
    s.run("record.validate", &json!({"table": "Sales Header", "key": ["Order", no], "field": "Shortcut Dimension 2 Code", "value": "SBY"})).unwrap();
    let h = get(&mut s, "Sales Header", json!(["Order", no]));
    assert_eq!(set_of(&mut s, h["Dimension Set ID"].as_i64().unwrap()), vec![("AREA".into(), "SBY".into()), ("DEPARTMENT".into(), "SALES".into())]);
    let posted = s.run("sales.document.post", &json!({"document_type": "Order", "no": no})).unwrap();
    let inv = posted["invoice_no"].as_str().or_else(|| posted["posted_no"].as_str()).map(str::to_string);
    let gl = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Document No.": inv.clone().unwrap_or_default()}})).unwrap();
    let entries = gl["records"].as_array().unwrap();
    assert!(!entries.is_empty(), "posted {posted}");
    assert!(entries.iter().all(|e| e["Global Dimension 2 Code"] == "SBY" && e["Global Dimension 1 Code"] == "SALES"));
    let cle = s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv.unwrap_or_default()}})).unwrap();
    assert_eq!(cle["records"][0]["Global Dimension 2 Code"], "SBY");
}

#[test]
fn identical_combinations_share_one_set() {
    let mut s = demo();
    let a = s.run("sales.document.new", &json!({"document_type": "Quote", "customer_no": "10000"})).unwrap();
    let b = s.run("sales.document.new", &json!({"document_type": "Quote", "customer_no": "40000"})).unwrap();
    let ha = get(&mut s, "Sales Header", json!(["Quote", a["no"]]));
    let hb = get(&mut s, "Sales Header", json!(["Quote", b["no"]]));
    assert_eq!(ha["Dimension Set ID"], hb["Dimension Set ID"], "10000 and 40000 are both SALES / JKT");
}
