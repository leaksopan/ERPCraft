#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Requisition worksheet: Calculate Plan by reordering policy, Carry Out Action Message.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn set(s: &mut Session, item: &str, field: &str, value: Value) {
    s.run("record.validate", &json!({"table": "Item", "key": item, "field": field, "value": value})).unwrap();
}

fn on_hand(s: &mut Session, item: &str) -> f64 {
    d(&s.run("record.get", &json!({"table": "Item", "key": item})).unwrap()["Inventory"])
}

#[test]
fn plan_carry_out_and_plan_again() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let stock = on_hand(&mut s, "1100");
    set(&mut s, "1100", "Reordering Policy", json!("Fixed Reorder Qty."));
    set(&mut s, "1100", "Reorder Point", json!(stock + 10.0));
    set(&mut s, "1100", "Reorder Quantity", json!(6));
    set(&mut s, "1200", "Reordering Policy", json!("Maximum Qty."));
    let tires = on_hand(&mut s, "1200");
    set(&mut s, "1200", "Reorder Point", json!(tires + 1.0));
    set(&mut s, "1200", "Maximum Inventory", json!(tires + 25.0));
    s.run("reqwksh.calculate_plan", &json!({})).unwrap();
    let lines = s.run("record.list", &json!({"page": "req-worksheet"})).unwrap();
    let rows = lines["records"].as_array().unwrap();
    let qty = |no: &str| d(&rows.iter().find(|r| r["No."] == no).unwrap()["Quantity"]);
    // Projected = on hand + open purchase orders - open sales orders (the demo has some).
    let mut open = |table: &str| -> f64 {
        s.run("record.list", &json!({"table": table, "filters": {"Document Type": "Order", "No.": "1100"}})).unwrap()["records"].as_array().unwrap().iter().map(|l| d(&l["Outstanding Quantity"])).sum()
    };
    let projected = stock + open("Purchase Line") - open("Sales Line");
    let q = qty("1100");
    assert!(q % 6.0 == 0.0 && projected + q > stock + 10.0 && projected + q - 6.0 <= stock + 10.0, "lots of 6 just past the reorder point: {q} from {projected}");
    assert!(qty("1200") >= 25.0 - 0.001, "up to the maximum inventory");
    assert!(rows.iter().all(|r| r["Action Message"] == "New" && r["Accept Action Message"] == true));
    let r = s.run("reqwksh.carry_out", &json!({})).unwrap();
    assert_eq!(r["orders"].as_array().unwrap().len(), 2, "one per vendor: {r}");
    let po = s.run("record.list", &json!({"table": "Purchase Line", "filters": {"Document Type": "Order", "No.": "1100"}})).unwrap();
    assert!(po["records"].as_array().unwrap().iter().any(|l| d(&l["Quantity"]) == q), "the order has the planned quantity");
    // The open orders cover it now.
    s.run("reqwksh.calculate_plan", &json!({})).unwrap();
    assert_eq!(s.run("record.list", &json!({"page": "req-worksheet"})).unwrap()["records"].as_array().unwrap().len(), 0);
}
