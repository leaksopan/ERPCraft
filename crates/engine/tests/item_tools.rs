#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! BC's Item Card functions: Adjust Inventory and Copy Item.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn num(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

fn stock_at(s: &mut Session, item: &str, loc: &str) -> f64 {
    let r = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": item, "Location Code": loc}, "limit": 10000})).unwrap();
    r["records"].as_array().unwrap().iter().map(|e| num(&e["Quantity"])).sum()
}

#[test]
fn adjust_inventory_posts_the_difference() {
    let mut s = demo();
    let before = stock_at(&mut s, "1000", "MAIN");
    s.run("item.adjust_inventory", &json!({"item_no": "1000", "location_code": "MAIN", "new_inventory": before + 6.0})).unwrap();
    assert!((stock_at(&mut s, "1000", "MAIN") - (before + 6.0)).abs() < 1e-9);
    s.run("item.adjust_inventory", &json!({"item_no": "1000", "location_code": "MAIN", "new_inventory": "2"})).unwrap();
    assert!((stock_at(&mut s, "1000", "MAIN") - 2.0).abs() < 1e-9, "a lower count posts a negative adjustment");
    let same = s.run("item.adjust_inventory", &json!({"item_no": "1000", "location_code": "MAIN", "new_inventory": 2})).unwrap();
    assert!(same["message"].as_str().unwrap().contains("already"));
    assert!(s.run("item.adjust_inventory", &json!({"item_no": "1000", "new_inventory": -1})).is_err());
    let open = s.run("record.list", &json!({"table": "Item Journal Line", "limit": 1000})).unwrap();
    assert!(open["records"].as_array().unwrap().iter().all(|l| !l["Journal Batch Name"].as_str().unwrap_or("").starts_with("ADJ-")), "posted lines leave the batch");
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
}

#[test]
fn copy_item_creates_a_new_item_with_the_same_setup() {
    let mut s = demo();
    let r = s.run("item.copy", &json!({"item_no": "1000", "target_no": "1000-C"})).unwrap();
    assert_eq!(r["no"], "1000-C");
    let src = s.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap();
    let copy = s.run("record.get", &json!({"table": "Item", "key": "1000-C"})).unwrap();
    for f in ["Description", "Base Unit of Measure", "Unit Price", "Gen. Prod. Posting Group", "Inventory Posting Group"] {
        assert_eq!(copy[f], src[f], "{f}");
    }
    assert_eq!(num(&copy["Inventory"]), 0.0, "stock is not copied");
    assert!(s.run("item.copy", &json!({"item_no": "1000", "target_no": "1000-C"})).is_err());
    let auto = s.run("item.copy", &json!({"item_no": "1001"})).unwrap();
    assert!(!auto["no"].as_str().unwrap().is_empty());
}
