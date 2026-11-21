#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Item tracking: lots and serial numbers on purchase, sale and transfer.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

/// The demo with a lot-tracked copy of engine oil (1000L) and a serial-tracked battery (1301S).
fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    for (from, to, code) in [("1000", "1000L", "LOTALL"), ("1301", "1301S", "SNALL")] {
        s.run("item.copy", &json!({"no": from, "target_no": to})).unwrap();
        s.run("record.validate", &json!({"table": "Item", "key": to, "field": "Item Tracking Code", "value": code})).unwrap();
    }
    s
}

/// A document line's key and its tracking source fields.
fn line(s: &mut Session, table: &str, doc_type: &str, no: &str) -> (Value, Value) {
    let l = &s.run("record.list", &json!({"table": table, "filters": {"Document No.": no}})).unwrap()["records"][0];
    let key = json!([doc_type, no, l["Line No."]]);
    let subtype = ["Quote", "Order", "Invoice", "Credit Memo"].iter().position(|t| *t == doc_type).unwrap();
    let src = json!({"Source Type": if table == "Sales Line" { 37 } else { 39 }, "Source Subtype": subtype, "Source ID": no, "Source Ref. No.": l["Line No."]});
    (key, src)
}

fn track(s: &mut Session, src: &Value, serial: &str, lot: &str, qty: i64) {
    let mut v = src.clone();
    v["Serial No."] = json!(serial);
    v["Lot No."] = json!(lot);
    v["Quantity (Base)"] = json!(qty);
    s.run("record.new", &json!({"table": "Reservation Entry", "values": v})).unwrap();
}

fn on_hand(s: &mut Session, item: &str, loc: &str, serial: &str, lot: &str) -> f64 {
    s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": item}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["Location Code"] == loc && e["Serial No."] == serial && e["Lot No."] == lot)
        .map(|e| d(&e["Quantity"]))
        .sum()
}

#[test]
fn lots_are_bought_split_sold_and_checked() {
    let mut s = demo();
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1000L", "quantity": 10}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Purchase Header", "key": ["Invoice", no], "field": "Vendor Invoice No.", "value": "V-LOT-1"})).ok();
    let err = s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap_err();
    assert!(err.message.contains("You must assign a lot number for item 1000L"), "{}", err.message);
    let (_, src) = line(&mut s, "Purchase Line", "Invoice", &no);
    track(&mut s, &src, "", "L1", 6);
    track(&mut s, &src, "", "L2", 3);
    let err = s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap_err();
    assert!(err.message.contains("is currently 9. It must be 10"), "{}", err.message);
    track(&mut s, &src, "", "L2", 1);
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    assert_eq!((on_hand(&mut s, "1000L", "MAIN", "", "L1"), on_hand(&mut s, "1000L", "MAIN", "", "L2")), (6.0, 4.0));
    let left = s.run("record.list", &json!({"table": "Reservation Entry"})).unwrap();
    assert_eq!(left["records"].as_array().unwrap().len(), 0, "the tracking is used up");

    // Selling 5 of lot L2 is more than it has.
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000L", "quantity": 5}]})).unwrap();
    let sno = sale["no"].as_str().unwrap().to_string();
    let (_, ssrc) = line(&mut s, "Sales Line", "Invoice", &sno);
    track(&mut s, &ssrc, "", "L2", 5);
    let err = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sno})).unwrap_err();
    assert!(err.message.contains("cannot be fully applied"), "{}", err.message);
    let e = &s.run("record.list", &json!({"table": "Reservation Entry"})).unwrap()["records"][0];
    s.run("record.validate", &json!({"table": "Reservation Entry", "key": e["Entry No."], "field": "Lot No.", "value": "L1"})).unwrap();
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sno})).unwrap();
    assert_eq!(on_hand(&mut s, "1000L", "MAIN", "", "L1"), 1.0);
    let entries = s.run("record.list", &json!({"page": "item-tracking-entries", "filters": {"Item No.": "1000L"}})).unwrap();
    assert!(entries["records"].as_array().unwrap().iter().all(|e| !e["Lot No."].as_str().unwrap().is_empty()));
}

#[test]
fn serial_numbers_are_unique_on_inventory() {
    let mut s = demo();
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "40000", "lines": [{"no": "1301S", "quantity": 2}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let (_, src) = line(&mut s, "Purchase Line", "Invoice", &no);
    track(&mut s, &src, "SN-001", "", 1);
    track(&mut s, &src, "SN-002", "", 1);
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    assert_eq!(on_hand(&mut s, "1301S", "MAIN", "SN-001", ""), 1.0);
    // SN-002 is still here: buying it again is refused.
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "40000", "lines": [{"no": "1301S", "quantity": 1}]})).unwrap();
    let no2 = doc["no"].as_str().unwrap().to_string();
    let (_, src2) = line(&mut s, "Purchase Line", "Invoice", &no2);
    track(&mut s, &src2, "SN-002", "", 1);
    let err = s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no2})).unwrap_err();
    assert!(err.message.contains("Serial No. SN-002 is already on inventory"), "{}", err.message);
}

#[test]
fn a_transfer_carries_its_lots_through_transit() {
    let mut s = demo();
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1000L", "quantity": 4}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let (_, src) = line(&mut s, "Purchase Line", "Invoice", &no);
    track(&mut s, &src, "", "L7", 4);
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let h = s.run("record.new", &json!({"table": "Transfer Header", "values": {"Transfer-from Code": "MAIN", "Transfer-to Code": "BANDUNG"}})).unwrap();
    let tno = h["record"]["No."].as_str().unwrap().to_string();
    let l = s.run("record.new", &json!({"table": "Transfer Line", "values": {"Document No.": tno, "Item No.": "1000L", "Quantity": 3}})).unwrap();
    let tsrc = json!({"Source Type": 5741, "Source Subtype": 0, "Source ID": tno, "Source Ref. No.": l["record"]["Line No."]});
    track(&mut s, &tsrc, "", "L7", 3);
    s.run("transfer.post", &json!({"no": tno, "mode": "Ship"})).unwrap();
    assert_eq!(on_hand(&mut s, "1000L", "TRANSIT", "", "L7"), 3.0);
    s.run("transfer.post", &json!({"no": tno, "mode": "Receive"})).unwrap();
    assert_eq!((on_hand(&mut s, "1000L", "TRANSIT", "", "L7"), on_hand(&mut s, "1000L", "BANDUNG", "", "L7")), (0.0, 3.0));
}
