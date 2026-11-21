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

fn order_shipped(s: &mut Session, sales: bool) -> (String, String) {
    let (cmd, party, field) = if sales { ("sales.document.new", "customer_no", "shipment_no") } else { ("purchase.document.new", "vendor_no", "receipt_no") };
    let doc = s.run(cmd, &json!({"document_type": "Order", party: "10000", "lines": [{"no": "1000", "quantity": 4}, {"no": "1001", "quantity": 4}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let post = if sales { "sales.document.post" } else { "purchase.document.post" };
    let r = s.run(post, &json!({"document_type": "Order", "no": no, "mode": if sales { "Ship" } else { "Receive" }})).unwrap();
    (no, r[field].as_str().unwrap().to_string())
}

#[test]
fn a_later_freight_invoice_gets_receipt_lines() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let (_, receipt) = order_shipped(&mut s, false);
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "20000", "lines": []})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let fl = s.run("record.new", &json!({"table": "Purchase Line", "values": {"Document Type": "Invoice", "Document No.": no, "Type": "Charge (Item)", "No.": "FREIGHT", "Quantity": 1, "Direct Unit Cost": 80_000}})).unwrap();
    let fkey = fl["key"].clone();
    let r = s.run("purchase.charge.get_receipt_lines", &json!({"table": "Purchase Line", "key": fkey, "document_no": receipt})).unwrap();
    assert!(r["message"].as_str().unwrap().starts_with("2 line(s)"), "{r}");
    s.run("purchase.charge.suggest", &json!({"table": "Purchase Line", "key": fkey})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let values = s.run("record.list", &json!({"table": "Value Entry", "filters": {"Item Charge No.": "FREIGHT"}})).unwrap();
    let rows = values["records"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().map(|v| d(&v["Cost Amount (Actual)"])).sum::<f64>(), 80_000.0);
    // On the receipt's entries.
    let receipt_entries: Vec<String> = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Document No.": receipt}})).unwrap()["records"].as_array().unwrap().iter().map(|e| e["Entry No."].to_string()).collect();
    assert!(rows.iter().all(|v| receipt_entries.contains(&v["Item Ledger Entry No."].to_string())));
}

#[test]
fn a_sales_charge_lands_on_the_shipments_sales_amount() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let (_, shipment) = order_shipped(&mut s, true);
    let ship_entries = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Document No.": shipment}})).unwrap()["records"].clone();
    let before: f64 = ship_entries.as_array().unwrap().iter().map(|e| d(&e["Sales Amount (Actual)"])).sum();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": []})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let fl = s.run("record.new", &json!({"table": "Sales Line", "values": {"Document Type": "Invoice", "Document No.": no, "Type": "Charge (Item)", "No.": "FREIGHT", "Quantity": 1, "Unit Price": 50_000}})).unwrap();
    let fkey = fl["key"].clone();
    let err = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap_err();
    assert!(err.message.contains("You must assign item charge FREIGHT"), "{}", err.message);
    s.run("sales.charge.get_shipment_lines", &json!({"table": "Sales Line", "key": fkey, "document_no": shipment})).unwrap();
    s.run("sales.charge.suggest", &json!({"table": "Sales Line", "key": fkey, "method": "By Amount"})).unwrap();
    let a = s.run("record.list", &json!({"table": "Item Charge Assignment (Sales)", "filters": {"Document No.": no}})).unwrap();
    assert_eq!(a["records"].as_array().unwrap().len(), 2);
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let after: f64 = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Document No.": shipment}})).unwrap()["records"].as_array().unwrap().iter().map(|e| d(&e["Sales Amount (Actual)"])).sum();
    assert_eq!(after - before, 50_000.0);
    let values = s.run("record.list", &json!({"table": "Value Entry", "filters": {"Item Charge No.": "FREIGHT"}})).unwrap();
    assert_eq!(values["records"].as_array().unwrap().iter().map(|v| d(&v["Sales Amount (Actual)"])).sum::<f64>(), 50_000.0);
}
