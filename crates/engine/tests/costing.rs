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

fn rows(s: &mut Session, table: &str, filters: Value) -> Vec<Value> {
    s.run("record.list", &json!({"table": table, "filters": filters, "limit": 10000})).unwrap()["records"].as_array().unwrap().clone()
}

fn copy_item(s: &mut Session, from: &str, to: &str) {
    s.run("item.copy", &json!({"no": from, "target_no": to})).unwrap();
}

fn setup_settled_demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("inventory.adjust_cost", &json!({})).unwrap();
    s
}

fn assert_balanced(s: &mut Session) {
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
    assert_eq!(s.run("report.balance_sheet", &json!({})).unwrap()["balanced"], true);
}

fn valuation(s: &mut Session) -> (f64, f64, f64, f64) {
    let v = s.run("report.inventory_valuation", &json!({})).unwrap();
    (d(&v["value"]), d(&v["actual_value"]), d(&v["expected_value"]), d(&v["gl_value"]))
}

fn line_of(s: &mut Session, table: &str, doc_type: &str, doc_no: &str) -> Value {
    rows(s, table, json!({"Document Type": doc_type, "Document No.": doc_no}))[0].clone()
}

fn assign_lot(s: &mut Session, doc_type: &str, doc_no: &str, line_no: i64, lot: &str, qty: i64) {
    let subtype = ["Quote", "Order", "Invoice", "Credit Memo"].iter().position(|t| *t == doc_type).unwrap();
    s.run(
        "record.new",
        &json!({
            "table": "Reservation Entry",
            "values": {
                "Source Type": 39,
                "Source Subtype": subtype,
                "Source ID": doc_no,
                "Source Ref. No.": line_no,
                "Lot No.": lot,
                "Quantity (Base)": qty
            }
        }),
    )
    .unwrap();
}

#[test]
fn receive_then_invoice_with_price_and_discount_increase_reconciles_subledger_and_gl() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1300", "P1");
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P1"}, "lines": [{"no": "P1", "quantity": 10, "direct_unit_cost": 10_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let rcpt = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap();
    let rcpt_no = rcpt["receipt_no"].as_str().unwrap().to_string();

    let rcpt_lines = rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": rcpt_no}));
    assert_eq!(rcpt_lines.len(), 1);
    assert_eq!(d(&rcpt_lines[0]["Quantity"]), 10.0);
    assert_eq!(d(&rcpt_lines[0]["Quantity Invoiced"]), 0.0);
    assert_eq!(d(&rcpt_lines[0]["Qty. Rcd. Not Invoiced"]), 10.0);

    let iles = rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no, "Item No.": "P1"}));
    assert_eq!(iles.len(), 1);
    assert_eq!(d(&iles[0]["Cost Amount (Actual)"]), 0.0);
    assert_eq!(d(&iles[0]["Cost Amount (Expected)"]), 100_000.0);
    assert_eq!(d(&iles[0]["Invoiced Quantity"]), 0.0);
    assert_eq!(iles[0]["Completely Invoiced"], false);

    let ves = rows(&mut s, "Value Entry", json!({"Item Ledger Entry No.": iles[0]["Entry No."]}));
    assert_eq!(ves.len(), 1);
    assert_eq!(ves[0]["Expected Cost"], true);
    assert_eq!(d(&ves[0]["Cost Amount (Expected)"]), 100_000.0);
    assert_eq!(d(&ves[0]["Cost Posted to G/L"]), 0.0);

    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 100_000.0, gl_base, 100_000.0, gl_base));
    assert_eq!(s.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0, "uninvoiced receipts must not post to G/L");

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    let pkey = json!(["Order", po_no, pl["Line No."]]);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Direct Unit Cost", "value": 15_000})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Line Discount %", "value": 20})).unwrap();
    let inv = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap();
    let inv_no = inv["posted_no"].as_str().unwrap().to_string();

    let rcpt_lines = rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": rcpt_no}));
    assert_eq!(d(&rcpt_lines[0]["Quantity Invoiced"]), 10.0);
    assert_eq!(d(&rcpt_lines[0]["Qty. Rcd. Not Invoiced"]), 0.0);

    let iles = rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no, "Item No.": "P1"}));
    assert_eq!(d(&iles[0]["Cost Amount (Expected)"]), 0.0);
    assert_eq!(d(&iles[0]["Cost Amount (Actual)"]), 120_000.0);
    assert_eq!(d(&iles[0]["Invoiced Quantity"]), 10.0);
    assert_eq!(iles[0]["Completely Invoiced"], true);

    let ile_filter = json!({"Item Ledger Entry No.": iles[0]["Entry No."]});
    assert_eq!(sum(&mut s, "Value Entry", ile_filter.clone(), "Cost Amount (Actual)"), 120_000.0);
    assert_eq!(sum(&mut s, "Value Entry", ile_filter.clone(), "Cost Amount (Expected)"), 0.0);
    assert_eq!(sum(&mut s, "Value Entry", ile_filter, "Cost Posted to G/L"), 120_000.0);
    assert_eq!(sum(&mut s, "G/L Entry", json!({"Document No.": inv_no, "G/L Account No.": "10300"}), "Amount"), 120_000.0);

    let item = s.run("record.get", &json!({"table": "Item", "key": "P1"})).unwrap();
    assert_eq!(d(&item["Last Direct Cost"]), 12_000.0);
    assert_eq!(d(&item["Unit Cost"]), 12_000.0);

    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 120_000.0, gl_base + 120_000.0, 0.0, gl_base + 120_000.0));
    assert_eq!(s.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);
    assert_balanced(&mut s);
}

#[test]
fn negative_price_delta_between_receive_and_invoice_reduces_cost_and_matches_gl() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1300", "P2");
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P2"}, "lines": [{"no": "P2", "quantity": 10, "direct_unit_cost": 12_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let rcpt_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Order", po_no, pl["Line No."]], "field": "Direct Unit Cost", "value": 9_000})).unwrap();
    s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap();

    let ile = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no, "Item No.": "P2"}))[0];
    assert_eq!(d(&ile["Cost Amount (Actual)"]), 90_000.0);
    assert_eq!(d(&ile["Cost Amount (Expected)"]), 0.0);
    assert_eq!(ile["Completely Invoiced"], true);

    let item = s.run("record.get", &json!({"table": "Item", "key": "P2"})).unwrap();
    assert_eq!(d(&item["Last Direct Cost"]), 9_000.0);
    assert_eq!(d(&item["Unit Cost"]), 9_000.0);

    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 90_000.0, gl_base + 90_000.0, 0.0, gl_base + 90_000.0));
    assert_balanced(&mut s);
}

#[test]
fn sale_before_invoice_adjusts_cogs_and_waits_for_post_cost_when_automatic_is_off() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1300", "P3A");
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P3A"}, "lines": [{"no": "P3A", "quantity": 10, "direct_unit_cost": 10_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap();

    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "P3A", "quantity": 4}]})).unwrap();
    let sale_inv = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale["no"]})).unwrap()["posted_no"].as_str().unwrap().to_string();
    assert_eq!(sum(&mut s, "Item Ledger Entry", json!({"Document No.": sale_inv}), "Cost Amount (Actual)"), -40_000.0);
    let cogs_before = sum(&mut s, "G/L Entry", json!({"G/L Account No.": "50200"}), "Amount");

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Order", po_no, pl["Line No."]], "field": "Direct Unit Cost", "value": 12_000})).unwrap();
    s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap();

    let adj = s.run("inventory.adjust_cost", &json!({"item_no": "P3A"})).unwrap();
    assert_eq!(adj["adjusted"], 1, "{adj}");
    assert_eq!(sum(&mut s, "Item Ledger Entry", json!({"Document No.": sale_inv}), "Cost Amount (Actual)"), -48_000.0);
    let cogs_after = sum(&mut s, "G/L Entry", json!({"G/L Account No.": "50200"}), "Amount");
    assert_eq!(cogs_after - cogs_before, 8_000.0);
    let (val, _, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, expected, gl_val), (gl_base + 72_000.0, 0.0, gl_base + 72_000.0));
    assert_eq!(s.run("inventory.adjust_cost", &json!({"item_no": "P3A"})).unwrap()["adjusted"], 0);
    assert_eq!(s.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);
    assert_balanced(&mut s);

    let mut s2 = setup_settled_demo();
    copy_item(&mut s2, "1300", "P3B");
    let (_, _, _, gl_base2) = valuation(&mut s2);
    s2.run("record.validate", &json!({"table": "Inventory Setup", "key": "", "field": "Automatic Cost Posting", "value": false})).unwrap();

    let po2 = s2
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P3B"}, "lines": [{"no": "P3B", "quantity": 10, "direct_unit_cost": 10_000}]}),
        )
        .unwrap();
    let po2_no = po2["no"].as_str().unwrap().to_string();
    s2.run("purchase.document.post", &json!({"no": po2_no, "mode": "Receive"})).unwrap();
    assert_eq!(s2.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);

    let sale2 = s2.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "P3B", "quantity": 4}]})).unwrap();
    s2.run("sales.document.post", &json!({"document_type": "Invoice", "no": sale2["no"]})).unwrap();

    s2.run("purchase.document.reopen", &json!({"no": po2_no})).unwrap();
    let pl2 = line_of(&mut s2, "Purchase Line", "Order", &po2_no);
    s2.run("record.validate", &json!({"table": "Purchase Line", "key": ["Order", po2_no, pl2["Line No."]], "field": "Direct Unit Cost", "value": 12_000})).unwrap();
    s2.run("purchase.document.post", &json!({"no": po2_no, "mode": "Invoice"})).unwrap();
    assert_eq!(s2.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0, "purchase invoice already posted its inventory debit to G/L");

    assert_eq!(s2.run("inventory.adjust_cost", &json!({"item_no": "P3B"})).unwrap()["adjusted"], 1);
    let posted = s2.run("inventory.post_cost_to_gl", &json!({})).unwrap();
    assert_eq!(posted["entries"], 1, "posts only the COGS adjustment");
    assert_eq!(s2.run("inventory.post_cost_to_gl", &json!({})).unwrap()["entries"], 0);
    let (val2, _, expected2, gl_val2) = valuation(&mut s2);
    assert_eq!((val2, expected2, gl_val2), (gl_base2 + 72_000.0, 0.0, gl_base2 + 72_000.0));
    assert_balanced(&mut s2);
}

#[test]
fn partial_receipts_and_partial_invoices_allocate_fifo_without_rounding_drift() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1300", "P4");
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P4-1"}, "lines": [{"no": "P4", "quantity": 10, "direct_unit_cost": 10_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    let pkey = json!(["Order", po_no, pl["Line No."]]);

    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Qty. to Receive", "value": 4})).unwrap();
    let r1 = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Qty. to Receive", "value": 6})).unwrap();
    let r2 = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Direct Unit Cost", "value": 12_000})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Qty. to Invoice", "value": 5})).unwrap();
    let inv1 = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap()["posted_no"].as_str().unwrap().to_string();

    let rl1 = &rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": r1}))[0];
    let rl2 = &rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": r2}))[0];
    assert_eq!((d(&rl1["Quantity Invoiced"]), d(&rl1["Qty. Rcd. Not Invoiced"])), (4.0, 0.0));
    assert_eq!((d(&rl2["Quantity Invoiced"]), d(&rl2["Qty. Rcd. Not Invoiced"])), (1.0, 5.0));

    let ile1 = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": r1}))[0];
    let ile2 = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": r2}))[0];
    assert_eq!((d(&ile1["Cost Amount (Actual)"]), d(&ile1["Cost Amount (Expected)"]), ile1["Completely Invoiced"].clone()), (48_000.0, 0.0, json!(true)));
    assert_eq!((d(&ile2["Cost Amount (Actual)"]), d(&ile2["Cost Amount (Expected)"]), ile2["Completely Invoiced"].clone()), (12_000.0, 50_000.0, json!(false)));

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Header", "key": ["Order", po_no], "field": "Vendor Invoice No.", "value": "INV-P4-2"})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Direct Unit Cost", "value": 11_000})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Qty. to Invoice", "value": 5})).unwrap();
    let inv2 = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap()["posted_no"].as_str().unwrap().to_string();

    let rl2_final = &rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": r2}))[0];
    assert_eq!((d(&rl2_final["Quantity Invoiced"]), d(&rl2_final["Qty. Rcd. Not Invoiced"])), (6.0, 0.0));
    let ile2_final = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": r2}))[0];
    assert_eq!((d(&ile2_final["Cost Amount (Actual)"]), d(&ile2_final["Cost Amount (Expected)"]), ile2_final["Completely Invoiced"].clone()), (67_000.0, 0.0, json!(true)));

    let gl_debit = sum(&mut s, "G/L Entry", json!({"Document No.": inv1, "G/L Account No.": "10300"}), "Amount")
        + sum(&mut s, "G/L Entry", json!({"Document No.": inv2, "G/L Account No.": "10300"}), "Amount");
    assert_eq!(gl_debit, 115_000.0);
    assert_eq!(48_000.0 + d(&ile2_final["Cost Amount (Actual)"]), gl_debit);

    copy_item(&mut s, "1300", "P4R");
    let po_r = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P4R"}, "lines": [{"no": "P4R", "quantity": 3, "direct_unit_cost": 100}]}),
        )
        .unwrap();
    let po_r_no = po_r["no"].as_str().unwrap().to_string();
    let plr = line_of(&mut s, "Purchase Line", "Order", &po_r_no);
    let pkey_r = json!(["Order", po_r_no, plr["Line No."]]);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey_r, "field": "Qty. to Receive", "value": 1})).unwrap();
    let ra = s.run("purchase.document.post", &json!({"no": po_r_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey_r, "field": "Qty. to Receive", "value": 2})).unwrap();
    let rb = s.run("purchase.document.post", &json!({"no": po_r_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();
    s.run("purchase.document.reopen", &json!({"no": po_r_no})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey_r, "field": "Direct Unit Cost", "value": 33.33333})).unwrap();
    s.run("purchase.document.post", &json!({"no": po_r_no, "mode": "Invoice"})).unwrap();

    let ca = d(&rows(&mut s, "Item Ledger Entry", json!({"Document No.": ra}))[0]["Cost Amount (Actual)"]);
    let cb = d(&rows(&mut s, "Item Ledger Entry", json!({"Document No.": rb}))[0]["Cost Amount (Actual)"]);
    assert_eq!((ca, cb, ca + cb), (33.33, 66.67, 100.0));

    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 115_100.0, gl_base + 115_100.0, 0.0, gl_base + 115_100.0));
    assert_balanced(&mut s);
}

#[test]
fn multi_lot_tracked_receipt_settles_all_lot_iles_to_actual_cost() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1000", "P5L");
    s.run("record.validate", &json!({"table": "Item", "key": "P5L", "field": "Item Tracking Code", "value": "LOTALL"})).unwrap();
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "10000", "values": {"Vendor Invoice No.": "INV-P5L"}, "lines": [{"no": "P5L", "quantity": 10, "direct_unit_cost": 10_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    let line_no = pl["Line No."].as_i64().unwrap();
    assign_lot(&mut s, "Order", &po_no, line_no, "L1", 4);
    assign_lot(&mut s, "Order", &po_no, line_no, "L2", 6);

    let rcpt_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();
    let iles_before = rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no}));
    assert_eq!(iles_before.len(), 2);

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Order", po_no, line_no], "field": "Direct Unit Cost", "value": 12_000})).unwrap();
    let inv_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap()["posted_no"].as_str().unwrap().to_string();

    let iles_after = rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no}));
    assert_eq!(iles_after.len(), 2);
    for ile in &iles_after {
        assert_eq!(d(&ile["Cost Amount (Expected)"]), 0.0);
        assert_eq!(ile["Completely Invoiced"], true);
        assert_eq!(d(&ile["Invoiced Quantity"]), d(&ile["Quantity"]));
    }
    let total_lot_actual: f64 = iles_after.iter().map(|e| d(&e["Cost Amount (Actual)"])).sum();
    assert_eq!(total_lot_actual, 120_000.0);
    assert_eq!(sum(&mut s, "G/L Entry", json!({"Document No.": inv_no, "G/L Account No.": "10300"}), "Amount"), 120_000.0);
    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 120_000.0, gl_base + 120_000.0, 0.0, gl_base + 120_000.0));
    assert_balanced(&mut s);

    let chg = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "20000", "lines": []})).unwrap();
    let chg_no = chg["no"].as_str().unwrap().to_string();
    let fl = s.run("record.new", &json!({"table": "Purchase Line", "values": {"Document Type": "Invoice", "Document No.": chg_no, "Type": "Charge (Item)", "No.": "FREIGHT", "Quantity": 1, "Direct Unit Cost": 10_000}})).unwrap();
    let fkey = fl["key"].clone();
    s.run("purchase.charge.get_receipt_lines", &json!({"table": "Purchase Line", "key": fkey, "document_no": rcpt_no})).unwrap();
    s.run("purchase.charge.suggest", &json!({"table": "Purchase Line", "key": fkey})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": chg_no})).unwrap();
    let iles_chg = rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no}));
    assert_eq!(iles_chg.iter().map(|e| d(&e["Cost Amount (Actual)"])).sum::<f64>(), 130_000.0);
    assert_eq!(d(&iles_chg[0]["Cost Amount (Actual)"]), 52_000.0);
    assert_eq!(d(&iles_chg[1]["Cost Amount (Actual)"]), 78_000.0);
}

#[test]
fn uom_conversion_fx_change_and_post_receipt_uom_guard() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1000", "P6");
    s.run("record.new", &json!({"table": "Item Unit of Measure", "values": {"Item No.": "P6", "Code": "BOX", "Qty. per Unit of Measure": 12}})).unwrap();
    s.run(
        "record.new",
        &json!({"table": "Vendor", "values": {"No.": "VEUR", "Name": "Euro Supplier", "Currency Code": "EUR", "Gen. Bus. Posting Group": "EXPORT", "VAT Bus. Posting Group": "EXPORT", "Vendor Posting Group": "DOMESTIC", "Location Code": "MAIN"}}),
    )
    .unwrap();
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "VEUR", "values": {"Posting Date": "2026-10-15", "Vendor Invoice No.": "INV-EUR-1"}, "lines": [{"no": "P6", "quantity": 2}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let pl0 = line_of(&mut s, "Purchase Line", "Order", &po_no);
    let pkey0 = json!(["Order", po_no, pl0["Line No."]]);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey0, "field": "Unit of Measure Code", "value": "BOX"})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey0, "field": "Direct Unit Cost", "value": 120})).unwrap();
    let rcpt_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();

    let rl = &rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": rcpt_no}))[0];
    assert_eq!(d(&rl["Qty. per Unit of Measure"]), 12.0);
    let ile = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no}))[0];
    assert_eq!((d(&ile["Quantity"]), d(&ile["Cost Amount (Expected)"])), (24.0, 4_200_000.0));

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    let pkey = json!(["Order", po_no, pl["Line No."]]);
    for (field, val) in [("Unit of Measure Code", json!("PCS")), ("No.", json!("1001")), ("Type", json!("G/L Account"))] {
        assert!(s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": field, "value": val})).is_err(), "{field} must be locked after receive");
    }

    s.run(
        "record.new",
        &json!({"table": "Currency Exchange Rate", "values": {"Currency Code": "EUR", "Starting Date": "2026-11-01", "Exchange Rate Amount": 1, "Relational Exch. Rate Amount": 18_000, "Adjustment Exch. Rate Amount": 1, "Relational Adjmt Exch Rate Amt": 18_000}}),
    )
    .unwrap();
    s.run("record.validate", &json!({"table": "Purchase Header", "key": ["Order", po_no], "field": "Posting Date", "value": "2026-11-15"})).unwrap();
    s.run("record.validate", &json!({"table": "Purchase Line", "key": pkey, "field": "Direct Unit Cost", "value": 130})).unwrap();
    let inv_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap()["posted_no"].as_str().unwrap().to_string();

    let ile_after = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no}))[0];
    assert_eq!(d(&ile_after["Cost Amount (Actual)"]), 4_680_000.0);
    assert_eq!(d(&ile_after["Cost Amount (Expected)"]), 0.0);
    assert_eq!(ile_after["Completely Invoiced"], true);
    assert_eq!(sum(&mut s, "G/L Entry", json!({"Document No.": inv_no, "G/L Account No.": "10300"}), "Amount"), 4_680_000.0);

    let item = s.run("record.get", &json!({"table": "Item", "key": "P6"})).unwrap();
    assert_eq!(d(&item["Last Direct Cost"]), 195_000.0);
    assert_eq!(d(&item["Unit Cost"]), 195_000.0);
    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 4_680_000.0, gl_base + 4_680_000.0, 0.0, gl_base + 4_680_000.0));
    assert_balanced(&mut s);

    let so = s.run("sales.document.new", &json!({"customer_no": "10000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    let so_no = so["no"].as_str().unwrap().to_string();
    s.run("sales.document.post", &json!({"no": so_no, "mode": "Ship"})).unwrap();
    s.run("sales.document.reopen", &json!({"no": so_no})).unwrap();
    let sl = line_of(&mut s, "Sales Line", "Order", &so_no);
    assert!(s.run("record.validate", &json!({"table": "Sales Line", "key": ["Order", so_no, sl["Line No."]], "field": "Unit of Measure Code", "value": "BOX"})).is_err());
}

#[test]
fn item_charge_before_invoicing_goods_preserves_freight_and_matches_gl() {
    let mut s = setup_settled_demo();
    copy_item(&mut s, "1300", "P7");
    let (_, _, _, gl_base) = valuation(&mut s);

    let po = s
        .run(
            "purchase.document.new",
            &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "INV-P7"}, "lines": [{"no": "P7", "quantity": 10, "direct_unit_cost": 10_000}, {"no": "9000", "quantity": 2, "direct_unit_cost": 5_000}]}),
        )
        .unwrap();
    let po_no = po["no"].as_str().unwrap().to_string();
    let rcpt_no = s.run("purchase.document.post", &json!({"no": po_no, "mode": "Receive"})).unwrap()["receipt_no"].as_str().unwrap().to_string();

    let chg_doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "20000", "lines": []})).unwrap();
    let chg_no = chg_doc["no"].as_str().unwrap().to_string();
    let fl = s.run("record.new", &json!({"table": "Purchase Line", "values": {"Document Type": "Invoice", "Document No.": chg_no, "Type": "Charge (Item)", "No.": "FREIGHT", "Quantity": 1, "Direct Unit Cost": 20_000}})).unwrap();
    let fkey = fl["key"].clone();
    s.run("purchase.charge.get_receipt_lines", &json!({"table": "Purchase Line", "key": fkey, "document_no": rcpt_no})).unwrap();
    s.run("purchase.charge.suggest", &json!({"table": "Purchase Line", "key": fkey})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": chg_no})).unwrap();

    let ile_mid = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no, "Item No.": "P7"}))[0];
    assert_eq!((d(&ile_mid["Cost Amount (Actual)"]), d(&ile_mid["Cost Amount (Expected)"])), (20_000.0, 100_000.0));
    assert_eq!(d(&s.run("record.get", &json!({"table": "Item", "key": "P7"})).unwrap()["Unit Cost"]), 12_000.0);

    s.run("purchase.document.reopen", &json!({"no": po_no})).unwrap();
    let pl = line_of(&mut s, "Purchase Line", "Order", &po_no);
    s.run("record.validate", &json!({"table": "Purchase Line", "key": ["Order", po_no, pl["Line No."]], "field": "Direct Unit Cost", "value": 12_000})).unwrap();
    s.run("purchase.document.post", &json!({"no": po_no, "mode": "Invoice"})).unwrap();

    let ile_end = &rows(&mut s, "Item Ledger Entry", json!({"Document No.": rcpt_no, "Item No.": "P7"}))[0];
    assert_eq!(d(&ile_end["Cost Amount (Actual)"]), 140_000.0);
    assert_eq!(d(&ile_end["Cost Amount (Expected)"]), 0.0);
    assert_eq!(ile_end["Completely Invoiced"], true);

    let item = s.run("record.get", &json!({"table": "Item", "key": "P7"})).unwrap();
    assert_eq!(d(&item["Last Direct Cost"]), 12_000.0);
    assert_eq!(d(&item["Unit Cost"]), 14_000.0);

    let svc_rcpt = &rows(&mut s, "Purch. Rcpt. Line", json!({"Document No.": rcpt_no, "No.": "9000"}))[0];
    assert_eq!((d(&svc_rcpt["Quantity Invoiced"]), d(&svc_rcpt["Qty. Rcd. Not Invoiced"])), (2.0, 0.0));

    let (val, actual, expected, gl_val) = valuation(&mut s);
    assert_eq!((val, actual, expected, gl_val), (gl_base + 140_000.0, gl_base + 140_000.0, 0.0, gl_base + 140_000.0));
    assert_balanced(&mut s);
}
