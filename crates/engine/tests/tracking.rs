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

/// Buys `qty` of 1000L in lot `lot` (with an expiration date when given); the posting's result.
fn buy_lot(s: &mut Session, lot: &str, qty: i64, expiration: Option<&str>) -> Result<Value, String> {
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1000L", "quantity": qty}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let (_, src) = line(s, "Purchase Line", "Invoice", &no);
    let mut v = src.clone();
    v["Lot No."] = json!(lot);
    v["Quantity (Base)"] = json!(qty);
    if let Some(e) = expiration {
        v["Expiration Date"] = json!(e);
    }
    s.run("record.new", &json!({"table": "Reservation Entry", "values": v})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).map_err(|e| e.message)
}

fn sell_lot(s: &mut Session, lot: &str, qty: i64) -> Result<Value, String> {
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000L", "quantity": qty}]})).unwrap();
    let no = sale["no"].as_str().unwrap().to_string();
    let (_, src) = line(s, "Sales Line", "Invoice", &no);
    track(s, &src, "", lot, qty);
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": no})).map_err(|e| e.message)
}

#[test]
fn lot_information_and_expiration_dates_rule_posting() {
    let mut s = demo();
    for (f, v) in [("Use Expiration Dates", json!(true)), ("Strict Expiration Posting", json!(true))] {
        s.run("record.validate", &json!({"table": "Item Tracking Code", "key": "LOTALL", "field": f, "value": v})).unwrap();
    }
    s.run("record.validate", &json!({"table": "Item", "key": "1000L", "field": "Expiration Calculation", "value": "30D"})).unwrap();
    buy_lot(&mut s, "L1", 5, None).unwrap();
    let e = &s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": "1000L", "Lot No.": "L1"}})).unwrap()["records"][0];
    let expected_exp = (s.work_date() + chrono::Duration::days(30)).to_string();
    assert_eq!(e["Expiration Date"], expected_exp, "the work date plus 30 days: {e}");
    // The same lot keeps one expiration date.
    let err = buy_lot(&mut s, "L1", 1, Some("2027-03-01")).unwrap_err();
    assert!(err.contains("Only one expiration date is allowed per lot number"), "{err}");

    // A blocked lot cannot be sold.
    s.run("record.new", &json!({"table": "Lot No. Information", "values": {"Item No.": "1000L", "Lot No.": "L1", "Description": "First batch", "Blocked": true}})).unwrap();
    let info = s.run("record.get", &json!({"table": "Lot No. Information", "key": ["1000L", "", "L1"]})).unwrap();
    assert_eq!(d(&info["Inventory"]), 5.0);
    let err = sell_lot(&mut s, "L1", 1).unwrap_err();
    assert!(err.contains("Blocked must be equal to 'No' in Lot No. Information"), "{err}");
    s.run("record.validate", &json!({"table": "Lot No. Information", "key": ["1000L", "", "L1"], "field": "Blocked", "value": false})).unwrap();
    sell_lot(&mut s, "L1", 1).unwrap();

    // An expired lot cannot be sold under strict expiration posting.
    buy_lot(&mut s, "OLD", 2, Some("2026-11-01")).unwrap();
    let err = sell_lot(&mut s, "OLD", 1).unwrap_err();
    assert!(err.contains("Expiration Date is before the posting date"), "{err}");
    let info = s.run("record.new", &json!({"table": "Lot No. Information", "values": {"Item No.": "1000L", "Lot No.": "OLD"}})).unwrap();
    assert_eq!(d(&info["record"]["Expired Inventory"]), 2.0, "{info}");

    // Information that must exist.
    s.run("record.validate", &json!({"table": "Item Tracking Code", "key": "LOTALL", "field": "Lot Info. Inbound Must Exist", "value": true})).unwrap();
    let err = buy_lot(&mut s, "NEW", 1, None).unwrap_err();
    assert!(err.contains("The Lot No. Information does not exist"), "{err}");
    s.run("record.new", &json!({"table": "Lot No. Information", "values": {"Item No.": "1000L", "Lot No.": "NEW"}})).unwrap();
    buy_lot(&mut s, "NEW", 1, None).unwrap();
}

#[test]
fn lot_information_is_copied_to_a_new_lot() {
    let mut s = demo();
    s.run("record.new", &json!({"table": "Lot No. Information", "values": {"Item No.": "1000L", "Lot No.": "A1", "Description": "Batch A", "Certificate Number": "C-77"}})).unwrap();
    let r = s.run("tracking.copy_info", &json!({"table": "Lot No. Information", "key": ["1000L", "", "A1"], "new_no": "A2"})).unwrap();
    assert_eq!(r["open"]["page"], "lot-no-information-card");
    let c = s.run("record.get", &json!({"table": "Lot No. Information", "key": ["1000L", "", "A2"]})).unwrap();
    assert_eq!((c["Description"].as_str().unwrap(), c["Certificate Number"].as_str().unwrap()), ("Batch A", "C-77"));
    assert!(s.run("tracking.copy_info", &json!({"table": "Lot No. Information", "key": ["1000L", "", "A1"], "new_no": "A2"})).is_err());
}

#[test]
fn lot_information_finds_its_entries_and_prints_a_label() {
    let mut s = demo();
    buy_lot(&mut s, "F1", 3, None).unwrap();
    s.run("record.new", &json!({"table": "Lot No. Information", "values": {"Item No.": "1000L", "Lot No.": "F1"}})).unwrap();
    let key = json!(["1000L", "", "F1"]);
    let nav = s.run("navigate", &json!({"table": "Lot No. Information", "key": key})).unwrap();
    assert_eq!(nav["report"]["rows"][0][1], 1, "{nav}");
    assert_eq!(nav["report"]["links"][0]["page"], "item-ledger-entries");
    let label = s.run("tracking.print_label", &json!({"table": "Lot No. Information", "key": key})).unwrap();
    assert_eq!(label["report"]["rows"][0][3], "F1");
}
