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

#[test]
fn set_dimension_filter_limits_entries_to_matching_sets() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "20000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let f = s.run("dimension.set_filter", &json!({"dimension_code": "area", "value_filter": "BDG"})).unwrap();
    assert_eq!(f["filter"]["field"], "Dimension Set ID");
    let value = f["filter"]["value"].as_str().unwrap().to_string();
    let all = s.run("record.list", &json!({"table": "G/L Entry"})).unwrap()["records"].as_array().unwrap().len();
    let hit = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Dimension Set ID": value}})).unwrap();
    let hit = hit["records"].as_array().unwrap();
    assert!(!hit.is_empty() && hit.len() < all, "{} of {all}", hit.len());
    assert!(hit.iter().all(|e| e["Global Dimension 2 Code"] == "BDG"));
    // Two dimensions: both must match; a value nobody has filters everything out.
    let both = s.run("dimension.set_filter", &json!({"lines": [{"dimension_code": "AREA", "value_filter": "BDG"}, {"dimension_code": "DEPARTMENT", "value_filter": "ADM"}]})).unwrap();
    let none = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Dimension Set ID": both["filter"]["value"]}})).unwrap();
    assert_eq!(none["records"].as_array().unwrap().len(), 0);
    let bad = s.run("dimension.set_filter", &json!({"dimension_code": "NOPE", "value_filter": "X"}));
    assert!(bad.unwrap_err().message.contains("does not exist"));
}

#[test]
fn dimensions_multiple_sets_and_clears_a_default_on_every_selected_record() {
    let mut s = demo();
    let keys = json!(["10000", "20000", "30000"]);
    let r = s.run("dimension.multiple", &json!({"table": "Customer", "keys": keys, "dimension_code": "PROJECT", "dimension_value_code": "fleet"})).unwrap();
    assert_eq!(r["records"], 3);
    for no in ["10000", "20000", "30000"] {
        let d = get(&mut s, "Default Dimension", json!([18, no, "PROJECT"]));
        assert_eq!(d["Dimension Value Code"], "FLEET");
    }
    let rows = r["report"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r[0] == "PROJECT" && r[1] == "FLEET"));
    // A global dimension goes through the card's field.
    s.run("dimension.multiple", &json!({"table": "Customer", "keys": keys, "dimension_code": "AREA", "dimension_value_code": "SBY"})).unwrap();
    assert_eq!(get(&mut s, "Customer", json!("20000"))["Global Dimension 2 Code"], "SBY");
    // Different values show as (Conflict); a blank value removes the dimension from all.
    s.run("record.validate", &json!({"table": "Customer", "key": "10000", "field": "Global Dimension 2 Code", "value": "JKT"})).unwrap();
    let r = s.run("dimension.multiple", &json!({"table": "Customer", "keys": keys})).unwrap();
    assert!(r["report"]["rows"].as_array().unwrap().iter().any(|r| r[0] == "AREA" && r[1] == "(Conflict)"), "{r}");
    s.run("dimension.multiple", &json!({"table": "Customer", "keys": keys, "dimension_code": "PROJECT", "dimension_value_code": ""})).unwrap();
    assert!(s.run("record.get", &json!({"table": "Default Dimension", "key": [18, "30000", "PROJECT"]})).is_err());
    assert!(s.run("dimension.multiple", &json!({"table": "Customer", "keys": keys, "dimension_code": "PROJECT", "dimension_value_code": "MARS"})).is_err());
}
