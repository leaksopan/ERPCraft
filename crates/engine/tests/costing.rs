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

/// Buys `qty` of `item` at `unit` on `date`.
fn buy_at(s: &mut Session, item: &str, qty: i64, unit: i64, date: &str) {
    let buy = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": item, "quantity": qty}]})).unwrap();
    let bno = buy["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Purchase Header", "key": ["Invoice", bno], "field": "Posting Date", "value": date})).unwrap();
    let line = &s.run("record.list", &json!({"table": "Purchase Line", "filters": {"Document No.": bno}})).unwrap()["records"][0];
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Invoice", bno, line["Line No."]], "field": "Direct Unit Cost", "value": unit})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": bno})).unwrap();
}

fn layered_sale_cost(method: &str) -> (f64, f64) {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "FX"})).unwrap();
    s.run("record.validate", &json!({"table": "Item", "key": "FX", "field": "Costing Method", "value": method})).unwrap();
    buy_at(&mut s, "FX", 10, 100_000, "2026-11-19");
    buy_at(&mut s, "FX", 10, 200_000, "2026-11-20");
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "FX", "quantity": 15}]})).unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale["no"]})).unwrap();
    s.run("inventory.adjust_cost", &json!({"item_no": "FX"})).unwrap();
    let cost = sum(&mut s, "Item Ledger Entry", json!({"Document No.": posted["posted_no"]}), "Cost Amount (Actual)");
    let unit = d(&s.run("record.get", &json!({"table": "Item", "key": "FX"})).unwrap()["Unit Cost"]);
    // Adjusting again changes nothing.
    assert_eq!(s.run("inventory.adjust_cost", &json!({"item_no": "FX"})).unwrap()["adjusted"], 0);
    (cost, unit)
}

#[test]
fn fifo_and_lifo_use_up_the_cost_layers() {
    assert_eq!(layered_sale_cost("FIFO"), (-2_000_000.0, 200_000.0), "10 at 100,000 then 5 at 200,000; 5 at 200,000 left");
    assert_eq!(layered_sale_cost("LIFO"), (-2_500_000.0, 100_000.0), "10 at 200,000 then 5 at 100,000; 5 at 100,000 left");
}

#[test]
fn the_standard_cost_worksheet_revalues_standard_items() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("item.copy", &json!({"no": "1300", "target_no": "ST"})).unwrap();
    s.run("record.validate", &json!({"table": "Item", "key": "ST", "field": "Costing Method", "value": "Standard"})).unwrap();
    s.run("record.validate", &json!({"table": "Item", "key": "ST", "field": "Standard Cost", "value": 100_000})).unwrap();
    buy_at(&mut s, "ST", 10, 100_000, "2026-11-20");
    s.run("standard_cost.suggest_item", &json!({"item_no": "ST", "standard_cost_factor": 1.1, "rounding_precision": 1000})).unwrap();
    let l = s.run("record.get", &json!({"table": "Standard Cost Worksheet", "key": ["DEFAULT", "Item", "ST"]})).unwrap();
    assert_eq!((d(&l["Standard Cost"]), d(&l["New Standard Cost"])), (100_000.0, 110_000.0));
    let r = s.run("standard_cost.implement", &json!({"document_no": "STD-1"})).unwrap();
    assert_eq!(r["revaluation_lines"], 1, "{r}");
    assert_eq!(d(&s.run("record.get", &json!({"table": "Item", "key": "ST"})).unwrap()["Standard Cost"]), 110_000.0);
    let j = &s.run("record.list", &json!({"table": "Item Journal Line", "filters": {"Journal Template Name": "REVAL"}})).unwrap()["records"][0];
    assert_eq!((d(&j["Inventory Value (Calculated)"]), d(&j["Inventory Value (Revalued)"])), (1_000_000.0, 1_100_000.0));
    s.run("item_journal.post", &json!({"template": "REVAL", "batch": "DEFAULT"})).unwrap();
    assert_eq!(sum(&mut s, "Value Entry", json!({"Item No.": "ST"}), "Cost Amount (Actual)"), 1_100_000.0);
    assert!(s.run("standard_cost.implement", &json!({})).unwrap_err().message.contains("nothing to implement"));
}
