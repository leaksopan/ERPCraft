#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Reservations: a sales order reserves inventory and purchase orders; receiving and shipping
//! move and use the reservations up; order promising finds the earliest shipment date.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn order(s: &mut Session, sales: bool, qty: i64) -> (String, Value) {
    let (cmd, party) = if sales { ("sales.document.new", json!({"document_type": "Order", "customer_no": "10000", "lines": [{"no": "RX", "quantity": qty}]})) } else { ("purchase.document.new", json!({"document_type": "Order", "vendor_no": "10000", "lines": [{"no": "RX", "quantity": qty}]})) };
    let no = s.run(cmd, &party).unwrap()["no"].as_str().unwrap().to_string();
    let table = if sales { "Sales Line" } else { "Purchase Line" };
    let l = &s.run("record.list", &json!({"table": table, "filters": {"Document No.": no}})).unwrap()["records"][0];
    let key = json!(["Order", no, l["Line No."]]);
    (no, key)
}

/// BC's Reserve: the Reservation page, then Auto Reserve on it.
fn reserve(s: &mut Session, line: &Value) -> Result<Value, erpcraft_engine::EngineError> {
    let page = s.run("reservation.reserve", &json!({"table": "Sales Line", "key": line}))?;
    assert_eq!(page["open"]["page"], "reservation-card");
    s.run("reservation.auto_reserve", &json!({"table": "Reservation Request", "key": page["open"]["key"]}))
}

fn field(s: &mut Session, table: &str, key: &Value, f: &str) -> f64 {
    d(&s.run("record.get", &json!({"table": table, "key": key})).unwrap()[f])
}

#[test]
fn reservations_follow_receipts_and_shipments() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "RX"})).unwrap();
    // 10 on hand, 5 on order.
    let inv = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "RX", "quantity": 10}]})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": inv["no"]})).unwrap();
    let (po, po_line) = order(&mut s, false, 5);

    // 12 for the first order: 10 from inventory, 2 from the purchase order.
    let (so1, l1) = order(&mut s, true, 12);
    reserve(&mut s, &l1).unwrap();
    assert_eq!(field(&mut s, "Sales Line", &l1, "Reserved Quantity"), 12.0);
    assert_eq!(field(&mut s, "Purchase Line", &po_line, "Reserved Quantity"), 2.0);
    let item = s.run("record.get", &json!({"table": "Item", "key": "RX"})).unwrap();
    assert_eq!((d(&item["Reserved Qty. on Inventory"]), d(&item["Reserved Qty. on Purch. Orders"]), d(&item["Reserved Qty. on Sales Orders"])), (10.0, 2.0, 12.0));
    let err = reserve(&mut s, &l1).unwrap_err();
    assert!(err.message.contains("fully reserved"), "{}", err.message);

    // 4 for the second: only the 3 left on the purchase order.
    let (_so2, l2) = order(&mut s, true, 4);
    reserve(&mut s, &l2).unwrap();
    assert_eq!(field(&mut s, "Sales Line", &l2, "Reserved Quantity"), 3.0);
    let err = reserve(&mut s, &l2).unwrap_err();
    assert!(err.message.contains("nothing available to reserve"), "{}", err.message);
    // Tracking lines do not see reservations.
    let entries = s.run("record.list", &json!({"page": "reservation-entries"})).unwrap();
    assert_eq!(entries["records"].as_array().unwrap().len(), 6, "three pairs");

    // Receiving the purchase order moves its reservations onto the inventory.
    s.run("purchase.document.post", &json!({"document_type": "Order", "no": po, "mode": "Receive"})).unwrap();
    let item = s.run("record.get", &json!({"table": "Item", "key": "RX"})).unwrap();
    assert_eq!((d(&item["Reserved Qty. on Inventory"]), d(&item["Reserved Qty. on Purch. Orders"])), (15.0, 0.0));
    assert_eq!(field(&mut s, "Sales Line", &l1, "Reserved Quantity"), 12.0);

    // Shipping the first order uses its reservation up.
    s.run("sales.document.post", &json!({"document_type": "Order", "no": so1, "mode": "Ship"})).unwrap();
    let item = s.run("record.get", &json!({"table": "Item", "key": "RX"})).unwrap();
    assert_eq!((d(&item["Reserved Qty. on Inventory"]), d(&item["Reserved Qty. on Sales Orders"])), (3.0, 3.0));

    // Cancelling the second leaves nothing reserved.
    s.run("reservation.cancel", &json!({"table": "Sales Line", "key": l2})).unwrap();
    assert_eq!(s.run("record.list", &json!({"table": "Reservation Entry"})).unwrap()["records"].as_array().unwrap().len(), 0);
}

#[test]
fn order_promising_finds_the_earliest_date() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "RX"})).unwrap();
    let inv = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "RX", "quantity": 2}]})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": inv["no"]})).unwrap();
    let (_po, po_line) = order(&mut s, false, 5);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": po_line, "field": "Expected Receipt Date", "value": "2026-12-15"})).unwrap();
    let (so, so_line) = order(&mut s, true, 6);
    let r = s.run("sales.order_promising", &json!({"no": so})).unwrap();
    let row = &r["report"]["rows"][0];
    assert_eq!(row[5], "2026-12-15", "{r}");
    s.run("sales.order_promising", &json!({"no": so, "accept": true})).unwrap();
    let l = s.run("record.get", &json!({"table": "Sales Line", "key": so_line})).unwrap();
    assert_eq!(l["Shipment Date"], "2026-12-15", "{l}");
    // A big order cannot be promised at all, and leaves no date for the first one either.
    let (so_big, _) = order(&mut s, true, 50);
    let r = s.run("sales.order_promising", &json!({"no": so_big})).unwrap();
    assert_eq!(r["report"]["rows"][0][5], "", "{r}");
    assert_eq!(s.run("sales.order_promising", &json!({"no": so})).unwrap()["report"]["rows"][0][5], "");
}

#[test]
fn the_reservation_page_reserves_per_kind_of_supply() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "RX"})).unwrap();
    let inv = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "RX", "quantity": 4}]})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": inv["no"]})).unwrap();
    let (_po, _) = order(&mut s, false, 5);
    let (_so, line) = order(&mut s, true, 7);
    let page = s.run("reservation.reserve", &json!({"table": "Sales Line", "key": line})).unwrap();
    let key = page["open"]["key"].clone();
    let h = s.run("record.get", &json!({"table": "Reservation Request", "key": key})).unwrap();
    assert_eq!((d(&h["Quantity to Reserve"]), d(&h["Unreserved Quantity"])), (7.0, 7.0));
    let rows = s.run("record.list", &json!({"table": "Entry Summary"})).unwrap()["records"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let po_row = rows.iter().find(|r| r["Summary Type"] == "Purchase Order").unwrap().clone();
    let po_key = json!([po_row["Source Type"], po_row["Source Subtype"], po_row["Source ID"], po_row["Source Ref. No."], po_row["Entry No."]]);
    // From the purchase order line only.
    s.run("reservation.reserve_from_line", &json!({"table": "Entry Summary", "key": po_key})).unwrap();
    let h = s.run("record.get", &json!({"table": "Reservation Request", "key": key})).unwrap();
    assert_eq!((d(&h["Reserved Quantity"]), d(&h["Unreserved Quantity"])), (5.0, 2.0));
    let po_row = s.run("record.get", &json!({"table": "Entry Summary", "key": po_key})).unwrap();
    assert_eq!(d(&po_row["Current Reserved Quantity"]), 5.0);
    // Auto Reserve takes the rest from inventory; cancelling the purchase part leaves 2.
    s.run("reservation.auto_reserve", &json!({"table": "Reservation Request", "key": key})).unwrap();
    assert_eq!(d(&s.run("record.get", &json!({"table": "Reservation Request", "key": key})).unwrap()["Unreserved Quantity"]), 0.0);
    s.run("reservation.cancel_from_line", &json!({"table": "Entry Summary", "key": po_key})).unwrap();
    assert_eq!(d(&s.run("record.get", &json!({"table": "Reservation Request", "key": key})).unwrap()["Reserved Quantity"]), 2.0);
    // Reserve from Inventory on the line itself.
    s.run("reservation.cancel", &json!({"table": "Sales Line", "key": line})).unwrap();
    s.run("reservation.reserve_from_inventory", &json!({"table": "Sales Line", "key": line})).unwrap();
    assert_eq!(field(&mut s, "Sales Line", &line, "Reserved Quantity"), 4.0);
}
