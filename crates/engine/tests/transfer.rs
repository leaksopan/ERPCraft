#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Transfer orders: ship to in-transit, receive at the destination, direct transfers.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn stock(s: &mut Session, item: &str, loc: &str) -> f64 {
    s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": item, "Location Code": loc}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| d(&e["Quantity"]))
        .sum()
}

fn new_order(s: &mut Session, direct: bool, qty: i64) -> String {
    let h = s.run("record.new", &json!({"table": "Transfer Header", "values": {"Transfer-from Code": "MAIN", "Transfer-to Code": "BANDUNG", "Direct Transfer": direct}})).unwrap();
    let no = h["record"]["No."].as_str().unwrap().to_string();
    s.run("record.new", &json!({"table": "Transfer Line", "values": {"Document No.": no, "Item No.": "1000", "Quantity": qty}})).unwrap();
    no
}

#[test]
fn ship_then_receive_through_in_transit() {
    let mut s = demo();
    let (main0, bdg0) = (stock(&mut s, "1000", "MAIN"), stock(&mut s, "1000", "BANDUNG"));
    let no = new_order(&mut s, false, 5);
    let h = s.run("record.get", &json!({"table": "Transfer Header", "key": no})).unwrap();
    assert_eq!(h["In-Transit Code"], "TRANSIT");
    let line = &s.run("record.list", &json!({"table": "Transfer Line", "filters": {"Document No.": no}})).unwrap()["records"][0];
    let lkey = json!([no, line["Line No."]]);
    // Ship 3 of 5.
    s.run("record.validate", &json!({"table": "Transfer Line", "key": lkey, "field": "Qty. to Ship", "value": 3})).unwrap();
    let r = s.run("transfer.post", &json!({"no": no, "mode": "Ship"})).unwrap();
    assert!(r["shipment_no"].is_string(), "{r}");
    assert_eq!(stock(&mut s, "1000", "MAIN"), main0 - 3.0);
    assert_eq!(stock(&mut s, "1000", "TRANSIT"), 3.0);
    let l = s.run("record.get", &json!({"table": "Transfer Line", "key": lkey})).unwrap();
    assert_eq!((d(&l["Quantity Shipped"]), d(&l["Qty. in Transit"]), d(&l["Qty. to Ship"])), (3.0, 3.0, 2.0));
    assert!(s.run("record.validate", &json!({"table": "Transfer Line", "key": lkey, "field": "Qty. to Receive", "value": 4})).is_err(), "only 3 are in transit");
    s.run("transfer.post", &json!({"no": no, "mode": "Receive"})).unwrap();
    assert_eq!(stock(&mut s, "1000", "TRANSIT"), 0.0);
    assert_eq!(stock(&mut s, "1000", "BANDUNG"), bdg0 + 3.0);
    // The rest, shipped and received at once: the order is then done.
    let r = s.run("transfer.post", &json!({"no": no, "mode": "Ship and Receive"})).unwrap();
    assert!(r["message"].as_str().unwrap().contains("fully"), "{r}");
    assert!(s.run("record.get", &json!({"table": "Transfer Header", "key": no})).is_err());
    assert_eq!(stock(&mut s, "1000", "BANDUNG"), bdg0 + 5.0);
    let shipments = s.run("record.list", &json!({"table": "Transfer Shipment Header", "filters": {"Transfer Order No.": no}})).unwrap();
    assert_eq!(shipments["records"].as_array().unwrap().len(), 2);
}

#[test]
fn direct_transfers_and_the_rules() {
    let mut s = demo();
    let main0 = stock(&mut s, "1000", "MAIN");
    let no = new_order(&mut s, true, 2);
    s.run("transfer.post", &json!({"no": no})).unwrap();
    assert_eq!(stock(&mut s, "1000", "MAIN"), main0 - 2.0);
    assert_eq!(stock(&mut s, "1000", "TRANSIT"), 0.0, "a direct transfer skips in-transit");
    let h = s.run("record.new", &json!({"table": "Transfer Header", "values": {"Transfer-from Code": "MAIN"}})).unwrap();
    let no2 = h["record"]["No."].clone();
    let same = s.run("record.validate", &json!({"table": "Transfer Header", "key": no2, "field": "Transfer-to Code", "value": "MAIN"}));
    assert!(same.unwrap_err().message.contains("cannot be the same"));
    let transit = s.run("record.validate", &json!({"table": "Transfer Header", "key": no2, "field": "Transfer-to Code", "value": "TRANSIT"}));
    assert!(transit.is_err(), "an in-transit location is not a destination");
}

