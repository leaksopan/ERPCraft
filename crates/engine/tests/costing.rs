#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Value entries, Adjust Cost - Item Entries and Post Inventory Cost to G/L.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn sum(s: &mut Session, table: &str, filters: Value, field: &str) -> f64 {
    s.run("record.list", &json!({"table": table, "filters": filters})).unwrap()["records"].as_array().unwrap().iter().map(|e| d(&e[field])).sum()
}

#[test]
fn every_item_entry_has_its_value_entry() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let ile = s.run("record.list", &json!({"table": "Item Ledger Entry"})).unwrap()["records"].as_array().unwrap().len();
    let ve = s.run("record.list", &json!({"table": "Value Entry"})).unwrap()["records"].as_array().unwrap().len();
    assert_eq!(ile, ve);
    assert_eq!(sum(&mut s, "Item Ledger Entry", json!({}), "Cost Amount (Actual)"), sum(&mut s, "Value Entry", json!({}), "Cost Amount (Actual)"));
}

#[test]
fn a_back_dated_purchase_revalues_the_sale_after_it() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    // Settle what the demo left first.
    s.run("inventory.adjust_cost", &json!({})).unwrap();
    let cogs_before = sum(&mut s, "G/L Entry", json!({"G/L Account No.": "50200"}), "Amount");
    let sales_cost_before = sum(&mut s, "Item Ledger Entry", json!({"Entry Type": "Sale"}), "Cost Amount (Actual)");
    // Sold today at the current unit cost...
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1001", "quantity": 4}]})).unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale["no"]})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    let sale_cost = sum(&mut s, "Item Ledger Entry", json!({"Document No.": inv}), "Cost Amount (Actual)");
    // ...but a dearer purchase dated yesterday changes the average that sale should carry.
    let buy = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1001", "quantity": 20}]})).unwrap();
    let bno = buy["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Purchase Header", "key": ["Invoice", bno], "field": "Posting Date", "value": "2026-11-20"})).unwrap();
    let line = &s.run("record.list", &json!({"table": "Purchase Line", "filters": {"Document No.": bno}})).unwrap()["records"][0];
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Invoice", bno, line["Line No."]], "field": "Direct Unit Cost", "value": 100_000})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": bno})).unwrap();

    let r = s.run("inventory.adjust_cost", &json!({"item_no": "1001"})).unwrap();
    assert!(r["adjusted"].as_i64().unwrap() >= 1, "{r}");
    let new_cost = sum(&mut s, "Item Ledger Entry", json!({"Document No.": inv}), "Cost Amount (Actual)");
    assert!(new_cost < sale_cost, "the sale now carries the higher average: {sale_cost} -> {new_cost}");
    // The difference is on the G/L (automatic cost posting): COGS moves with it.
    let cogs_after = sum(&mut s, "G/L Entry", json!({"G/L Account No.": "50200"}), "Amount");
    let sales_cost_after = sum(&mut s, "Item Ledger Entry", json!({"Entry Type": "Sale"}), "Cost Amount (Actual)");
    assert!(((cogs_after - cogs_before) + (sales_cost_after - sales_cost_before)).abs() < 0.01, "COGS follows the sales' adjusted cost");
    assert_eq!(sum(&mut s, "Item Ledger Entry", json!({}), "Cost Amount (Actual)"), sum(&mut s, "Value Entry", json!({}), "Cost Amount (Actual)"));
    // Adjusting again changes nothing; nothing is left to post.
    assert_eq!(s.run("inventory.adjust_cost", &json!({})).unwrap()["adjusted"], 0);
    assert_eq!(s.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);
    // The books still balance.
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    let row = |no: &str| d(&bs["report"]["rows"].as_array().unwrap().iter().find(|r| r[0] == no).unwrap()[2]);
    assert!((row("19") - row("40")).abs() < 0.01);
}

#[test]
fn without_automatic_cost_posting_the_cost_waits_for_post_inventory_cost() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("record.validate", &json!({"table": "Inventory Setup", "key": "", "field": "Automatic Cost Posting", "value": false})).unwrap();
    let r = s.run("inventory.adjust_cost", &json!({})).unwrap();
    let pending = s.run("inventory.post_cost_to_gl", &json!({})).unwrap();
    assert_eq!(pending["entries"], r["adjusted"], "{pending}");
    assert_eq!(s.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);
}
