#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Physical inventory orders: calculate lines, record counts, finish, post; counting periods.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn stock(s: &mut Session, item: &str, loc: &str) -> f64 {
    s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": item}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["Location Code"] == loc)
        .map(|e| d(&e["Quantity"]))
        .sum()
}

fn rows(s: &mut Session, table: &str, filters: Value) -> Vec<Value> {
    s.run("record.list", &json!({"table": table, "filters": filters})).unwrap()["records"].as_array().unwrap().clone()
}

fn new_order(s: &mut Session) -> String {
    let h = s.run("record.new", &json!({"table": "Phys. Invt. Order Header", "values": {"Description": "Year-end count", "Location Code": "MAIN"}})).unwrap();
    h["record"]["No."].as_str().unwrap().to_string()
}

#[test]
fn a_counted_difference_is_posted() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let before = stock(&mut s, "1000", "MAIN");
    assert!(before > 0.0, "the demo has engine oil at MAIN");
    let no = new_order(&mut s);
    assert!(no.starts_with("PI-ORD"), "{no}");
    let r = s.run("physinvt.calculate_lines", &json!({"no": no})).unwrap();
    assert!(r["lines"].as_i64().unwrap() > 0);
    let lines = rows(&mut s, "Phys. Invt. Order Line", json!({"Document No.": no}));
    let oil = lines.iter().find(|l| l["Item No."] == "1000").unwrap();
    assert_eq!(d(&oil["Qty. Expected (Base)"]), before);
    // Calculating again adds nothing.
    assert_eq!(s.run("physinvt.calculate_lines", &json!({"no": no})).unwrap()["lines"], 0);

    // Finishing before anything is recorded is refused.
    let err = s.run("physinvt.finish", &json!({"no": no})).unwrap_err();
    assert!(err.message.contains("has not been recorded"), "{}", err.message);

    let rec = s.run("physinvt.make_recording", &json!({"no": no})).unwrap();
    let rec_no = rec["recording_no"].as_i64().unwrap();
    let err = s.run("physinvt.finish_recording", &json!({"order_no": no, "recording_no": rec_no})).unwrap_err();
    assert!(err.message.contains("Recorded must be equal to 'Yes'"), "{}", err.message);
    // Count: two more engine oil than expected, everything else as expected.
    for l in rows(&mut s, "Phys. Invt. Record Line", json!({"Order No.": no})) {
        let expected = lines.iter().find(|o| o["Line No."] == l["Order Line No."]).map(|o| d(&o["Qty. Expected (Base)"])).unwrap();
        let counted = if l["Item No."] == "1000" { expected + 2.0 } else { expected };
        s.run("record.validate", &json!({"table": "Phys. Invt. Record Line", "key": [no, rec_no, l["Line No."]], "field": "Quantity", "value": counted})).unwrap();
    }
    s.run("physinvt.finish_recording", &json!({"table": "Phys. Invt. Record Header", "key": [no, rec_no]})).unwrap();
    s.run("physinvt.finish", &json!({"table": "Phys. Invt. Order Header", "key": no})).unwrap();
    let oil = rows(&mut s, "Phys. Invt. Order Line", json!({"Document No.": no, "Item No.": "1000"}))[0].clone();
    assert_eq!((oil["Entry Type"].as_str().unwrap(), d(&oil["Pos. Qty. (Base)"])), ("Positive Adjmt.", 2.0));
    let stats = s.run("physinvt.statistics", &json!({"no": no})).unwrap();
    assert_eq!(stats["report"]["rows"][3][1], "1", "{stats}");

    let posted = s.run("physinvt.post", &json!({"no": no})).unwrap();
    let pno = posted["posted_no"].as_str().unwrap().to_string();
    assert!(pno.starts_with("PI-PSTD"), "{pno}");
    assert_eq!(stock(&mut s, "1000", "MAIN"), before + 2.0);
    assert_eq!(rows(&mut s, "Phys. Invt. Order Header", json!({})).len(), 0);
    assert_eq!(rows(&mut s, "Pstd. Phys. Invt. Order Line", json!({"Document No.": pno})).len(), lines.len());
    assert_eq!(rows(&mut s, "Pstd. Phys. Invt. Record Hdr", json!({"Order No.": pno})).len(), 1);
    assert_eq!(rows(&mut s, "Item Journal Line", json!({})).len(), 0, "the posting batch is gone");
    let item = s.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap();
    assert!(item["Last Phys. Invt. Date"].is_string(), "{item}");
}

#[test]
fn moved_inventory_must_be_recalculated() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let no = new_order(&mut s);
    s.run("physinvt.calculate_lines", &json!({"no": no})).unwrap();
    let rec_no = s.run("physinvt.make_recording", &json!({"no": no})).unwrap()["recording_no"].as_i64().unwrap();
    for l in rows(&mut s, "Phys. Invt. Record Line", json!({"Order No.": no})) {
        s.run("record.validate", &json!({"table": "Phys. Invt. Record Line", "key": [no, rec_no, l["Line No."]], "field": "Quantity", "value": 1})).unwrap();
    }
    s.run("physinvt.finish_recording", &json!({"order_no": no, "recording_no": rec_no})).unwrap();
    s.run("physinvt.finish", &json!({"no": no})).unwrap();
    // A sale after counting.
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale["no"]})).unwrap();
    let err = s.run("physinvt.post", &json!({"no": no})).unwrap_err();
    assert!(err.message.contains("has changed since the expected quantity was calculated"), "{}", err.message);
    s.run("physinvt.reopen", &json!({"no": no})).unwrap();
    s.run("physinvt.calc_qty_expected", &json!({"no": no})).unwrap();
    s.run("physinvt.finish", &json!({"no": no})).unwrap();
    s.run("physinvt.post", &json!({"no": no})).unwrap();
    assert_eq!(stock(&mut s, "1000", "MAIN"), 1.0, "counted one");
    // Reopening a recording of a posted order is impossible: it is gone.
    assert_eq!(rows(&mut s, "Phys. Invt. Record Header", json!({})).len(), 0);
}

#[test]
fn counting_periods_pick_the_due_items() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("record.validate", &json!({"table": "Item", "key": "1000", "field": "Phys Invt Counting Period Code", "value": "FAST"})).unwrap();
    let no = new_order(&mut s);
    s.run("physinvt.calculate_counting_period", &json!({"no": no})).unwrap();
    let lines = rows(&mut s, "Phys. Invt. Order Line", json!({"Document No.": no}));
    assert!(!lines.is_empty() && lines.iter().all(|l| l["Item No."] == "1000"), "{lines:?}");
    let item = s.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap();
    let start = item["Next Counting Start Date"].as_str().unwrap().to_string();
    assert!(start.ends_with("-01"), "monthly counting starts next month: {start}");
    // Not due again on the same date.
    let no2 = new_order(&mut s);
    let err = s.run("physinvt.calculate_counting_period", &json!({"no": no2})).unwrap_err();
    assert!(err.message.contains("no items with a counting period due"), "{}", err.message);
}

#[test]
fn reports_of_open_and_posted_orders() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let no = new_order(&mut s);
    let h = s.run("record.get", &json!({"table": "Phys. Invt. Order Header", "key": no})).unwrap();
    assert!(h["Order Date"].is_null() && h["Posting Date"].is_string(), "BC's InitRecord sets only the posting date: {h}");
    s.run("physinvt.calculate_lines", &json!({"no": no})).unwrap();
    let rec_no = s.run("physinvt.make_recording", &json!({"no": no})).unwrap()["recording_no"].as_i64().unwrap();
    let sheet = s.run("physinvt.recording_report", &json!({"table": "Phys. Invt. Record Header", "key": [no, rec_no]})).unwrap();
    assert!(!sheet["report"]["sections"][0]["rows"].as_array().unwrap().is_empty());
    for l in rows(&mut s, "Phys. Invt. Record Line", json!({"Order No.": no})) {
        s.run("record.validate", &json!({"table": "Phys. Invt. Record Line", "key": [no, rec_no, l["Line No."]], "field": "Quantity", "value": 0})).unwrap();
    }
    s.run("physinvt.finish_recording", &json!({"order_no": no, "recording_no": rec_no})).unwrap();
    s.run("physinvt.finish", &json!({"no": no})).unwrap();
    let diff = s.run("physinvt.diff_report", &json!({"no": no})).unwrap();
    assert!(diff["report"]["rows"].as_array().unwrap().iter().all(|r| r[6] == "0"));
    let pno = s.run("physinvt.post", &json!({"no": no})).unwrap()["posted_no"].as_str().unwrap().to_string();
    let posted = s.run("physinvt.diff_report", &json!({"table": "Pstd. Phys. Invt. Order Hdr", "key": pno})).unwrap();
    assert_eq!(posted["report"]["rows"].as_array().unwrap().len(), diff["report"]["rows"].as_array().unwrap().len());
    assert!(posted["report"]["title"].as_str().unwrap().starts_with("Posted Phys. Invt. Order Diff."));
    s.run("physinvt.statistics", &json!({"no": pno, "posted": true})).unwrap();
    s.run("physinvt.recording_report", &json!({"table": "Pstd. Phys. Invt. Record Hdr", "key": [pno, rec_no]})).unwrap();
}
