#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Stockkeeping units (and planning by them) and the revaluation journal.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn rows(s: &mut Session, table: &str, filters: Value) -> Vec<Value> {
    s.run("record.list", &json!({"table": table, "filters": filters})).unwrap()["records"].as_array().unwrap().clone()
}

fn sum(s: &mut Session, table: &str, filters: Value, field: &str) -> f64 {
    rows(s, table, filters).iter().map(|r| d(&r[field])).sum()
}

#[test]
fn stockkeeping_units_are_created_and_planned_by() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let r = s.run("item.create_sku", &json!({"no": "1000"})).unwrap();
    let locations = rows(&mut s, "Location", json!({})).iter().filter(|l| l["Use As In-Transit"] != true).count();
    assert_eq!(r["created"].as_i64().unwrap() as usize, locations);
    // Again without replacing: nothing new.
    assert_eq!(s.run("item.create_sku", &json!({"no": "1000"})).unwrap()["created"], 0);
    let sku = s.run("record.get", &json!({"table": "Stockkeeping Unit", "key": ["MAIN", "1000", ""]})).unwrap();
    assert_eq!(sku["Description"], s.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap()["Description"]);
    let on_hand = sum(&mut s, "Item Ledger Entry", json!({"Item No.": "1000", "Location Code": "MAIN"}), "Quantity");
    assert_eq!(d(&sku["Inventory"]), on_hand);

    // MAIN reorders 50 at a time below 1000; the other locations do not plan.
    for (f, v) in [("Reordering Policy", json!("Fixed Reorder Qty.")), ("Reorder Point", json!(1000)), ("Reorder Quantity", json!(50))] {
        s.run("record.validate", &json!({"table": "Stockkeeping Unit", "key": ["MAIN", "1000", ""], "field": f, "value": v})).unwrap();
    }
    for sk in rows(&mut s, "Stockkeeping Unit", json!({"Item No.": "1000"})) {
        if sk["Location Code"] != "MAIN" {
            s.run("record.validate", &json!({"table": "Stockkeeping Unit", "key": [sk["Location Code"], "1000", ""], "field": "Reordering Policy", "value": ""})).unwrap();
        }
    }
    s.run("reqwksh.calculate_plan", &json!({"item_no": "1000"})).unwrap();
    let lines = rows(&mut s, "Requisition Line", json!({"No.": "1000"}));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["Location Code"], "MAIN");
    let q = d(&lines[0]["Quantity"]);
    assert!(q > 0.0 && (q % 50.0).abs() < 1e-9 && on_hand + q > 1000.0, "{q}");
}

#[test]
fn a_revaluation_raises_the_value_and_later_costs() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "RX"})).unwrap();
    let buy = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "RX", "quantity": 10}]})).unwrap();
    let bno = buy["no"].as_str().unwrap().to_string();
    let line = rows(&mut s, "Purchase Line", json!({"Document No.": bno}))[0].clone();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Invoice", bno, line["Line No."]], "field": "Direct Unit Cost", "value": 100_000})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": bno})).unwrap();
    s.run("inventory.adjust_cost", &json!({})).unwrap();

    let r = s.run("item_journal.calculate_inventory_value", &json!({"item_no": "RX"})).unwrap();
    assert_eq!(r["lines"], 1);
    let l = rows(&mut s, "Item Journal Line", json!({"Journal Template Name": "REVAL"}))[0].clone();
    assert_eq!((d(&l["Quantity"]), d(&l["Inventory Value (Calculated)"]), d(&l["Unit Cost (Calculated)"])), (10.0, 1_000_000.0, 100_000.0));
    let key = json!(["REVAL", "DEFAULT", l["Line No."]]);
    s.run("record.validate", &json!({"table": "Item Journal Line", "key": key, "field": "Unit Cost (Revalued)", "value": 120_000})).unwrap();
    let l = s.run("record.get", &json!({"table": "Item Journal Line", "key": key})).unwrap();
    assert_eq!(d(&l["Inventory Value (Revalued)"]), 1_200_000.0);
    let adj_before = sum(&mut s, "G/L Entry", json!({}), "Amount");
    s.run("item_journal.post", &json!({"template": "REVAL", "batch": "DEFAULT"})).unwrap();
    assert!(adj_before.abs() < 0.01 && sum(&mut s, "G/L Entry", json!({}), "Amount").abs() < 0.01, "the G/L balances");
    let reval = rows(&mut s, "Value Entry", json!({"Item No.": "RX", "Entry Type": "Revaluation"}));
    assert_eq!(reval.len(), 1);
    assert_eq!(d(&reval[0]["Cost Amount (Actual)"]), 200_000.0);
    assert_eq!(sum(&mut s, "Value Entry", json!({"Item No.": "RX"}), "Cost Amount (Actual)"), 1_200_000.0);
    assert_eq!(d(&s.run("record.get", &json!({"table": "Item", "key": "RX"})).unwrap()["Unit Cost"]), 120_000.0);

    // A sale after it costs the revalued average.
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "RX", "quantity": 5}]})).unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale["no"]})).unwrap();
    s.run("inventory.adjust_cost", &json!({"item_no": "RX"})).unwrap();
    let cost = sum(&mut s, "Item Ledger Entry", json!({"Document No.": posted["posted_no"]}), "Cost Amount (Actual)");
    assert_eq!(cost, -600_000.0);
    assert_eq!(sum(&mut s, "Value Entry", json!({"Item No.": "RX"}), "Cost Amount (Actual)"), 600_000.0, "half the revalued inventory is left");
}

#[test]
fn the_inventory_value_test_leaves_the_journal_empty() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let r = s.run("item_journal.calculate_inventory_value_test", &json!({})).unwrap();
    assert!(!r["report"]["rows"].as_array().unwrap().is_empty());
    assert_eq!(rows(&mut s, "Item Journal Line", json!({"Journal Template Name": "REVAL"})).len(), 0);
}
