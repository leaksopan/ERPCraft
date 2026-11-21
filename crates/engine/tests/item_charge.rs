#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Item charges: freight on a purchase invoice goes onto the items' cost.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

#[test]
fn freight_is_assigned_and_lands_on_the_item_cost() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1000", "quantity": 10}, {"no": "1001", "quantity": 10}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let fl = s
        .run("record.new", &json!({"table": "Purchase Line", "values": {"Document Type": "Invoice", "Document No.": no, "Type": "Charge (Item)", "No.": "FREIGHT", "Quantity": 1, "Direct Unit Cost": 600_000}}))
        .unwrap();
    let fkey = fl["key"].clone();
    assert_eq!(fl["record"]["Description"], "Freight charges");
    assert!(s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap_err().message.contains("You must assign item charge FREIGHT"));
    let open = s.run("purchase.charge.assignment", &json!({"table": "Purchase Line", "key": fkey})).unwrap();
    assert_eq!(open["open"]["page"], "item-charge-assignment-purch");
    s.run("purchase.charge.suggest", &json!({"table": "Purchase Line", "key": fkey, "method": "Equally"})).unwrap();
    let a = s.run("record.list", &json!({"table": "Item Charge Assignment (Purch)", "filters": {"Document No.": no}})).unwrap();
    let rows = a["records"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((d(&rows[0]["Qty. to Assign"]), d(&rows[0]["Amount to Assign"])), (0.5, 300_000.0));
    let posted = s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    // Each item's receipt entry carries its 300,000 share of the freight.
    let entries = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Document No.": inv, "Item No.": "1000"}})).unwrap();
    let ile = &entries["records"][0];
    let values = s.run("record.list", &json!({"table": "Value Entry", "filters": {"Item Ledger Entry No.": ile["Entry No."].to_string(), "Item Charge No.": "FREIGHT"}})).unwrap();
    assert_eq!(d(&values["records"][0]["Cost Amount (Actual)"]), 300_000.0);
    let line_cost = d(&ile["Cost Amount (Actual)"]);
    assert!(line_cost > 300_000.0, "{ile}");
    // The books still balance; nothing went to a purchases account.
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    let row = |no: &str| d(&bs["report"]["rows"].as_array().unwrap().iter().find(|r| r[0] == no).unwrap()[2]);
    assert!((row("19") - row("40")).abs() < 0.01);
}
