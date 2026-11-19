#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Phase 3: locations, item journals (adjustment, reclassification, physical inventory), fixed
//! assets (acquisition by purchase / grant / opening balance, depreciation, disposal).

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn dec(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

fn balanced(s: &mut Session) {
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
    assert_eq!(s.run("report.balance_sheet", &json!({})).unwrap()["balanced"], true);
}

fn stock_at(s: &mut Session, item: &str, loc: &str) -> f64 {
    let r = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": item, "Location Code": loc}, "limit": 10000})).unwrap();
    r["records"].as_array().unwrap().iter().map(|e| dec(&e["Quantity"])).sum()
}

fn gl(s: &mut Session, acc: &str) -> f64 {
    dec(&s.run("record.get", &json!({"table": "G/L Account", "key": acc})).unwrap()["Balance"])
}

#[test]
fn locations_flow_from_customer_to_item_ledger() {
    let mut s = demo();
    // The demo moved 10 tires to Bandung, and Bandung customer 20000 already took 4 from there.
    let start = stock_at(&mut s, "1200", "BANDUNG");
    assert_eq!(start, 6.0);
    let r = s.run("sales.document.new", &json!({"customer_no": "20000", "lines": [{"no": "1200", "quantity": 3}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).unwrap();
    assert_eq!(h["Location Code"], "BANDUNG");
    s.run("sales.document.post", &json!({"no": no})).unwrap();
    assert_eq!(stock_at(&mut s, "1200", "BANDUNG"), start - 3.0);
    let avail = s.run("item.availability", &json!({"no": "1200"})).unwrap();
    assert!(avail["report"]["rows"].as_array().unwrap().iter().any(|r| r[0].as_str().unwrap().contains("at BANDUNG")));
    balanced(&mut s);
}

#[test]
fn negative_inventory_can_be_prevented() {
    let mut s = demo();
    s.run("record.modify", &json!({"table": "Inventory Setup", "key": "", "values": {"Prevent Negative Inventory": true}})).unwrap();
    let r = s.run("sales.document.new", &json!({"customer_no": "20000", "lines": [{"no": "1200", "quantity": 50}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let e = s.run("sales.document.post", &json!({"no": no})).unwrap_err();
    assert!(e.message.contains("insufficient quantity"), "{e}");
}

#[test]
fn item_journal_adjusts_stock_and_gl() {
    let mut s = demo();
    let inv_before = gl(&mut s, "10300");
    let adj_before = gl(&mut s, "50400");
    let stock = stock_at(&mut s, "1001", "MAIN");
    let line = s.run("record.new", &json!({"page": "item-journal", "values": {"Entry Type": "Negative Adjmt.", "Item No.": "1001", "Location Code": "MAIN", "Quantity": 2}})).unwrap();
    let unit = dec(&line["record"]["Unit Amount"]);
    assert!(unit > 0.0);
    assert_eq!(line["record"]["Document No."], "IJ00001");
    s.run("item_journal.post", &json!({"template": "ITEM"})).unwrap();
    assert_eq!(stock_at(&mut s, "1001", "MAIN"), stock - 2.0);
    assert_eq!(gl(&mut s, "10300"), inv_before - 2.0 * unit);
    assert_eq!(gl(&mut s, "50400"), adj_before + 2.0 * unit);
    balanced(&mut s);
}

#[test]
fn physical_inventory_posts_the_difference() {
    let mut s = demo();
    let r = s.run("item_journal.calculate_inventory", &json!({"template": "PHYS INV", "location_code": "MAIN"})).unwrap();
    assert!(r["lines"].as_i64().unwrap() >= 5);
    let lines = s.run("record.list", &json!({"page": "phys-inventory-journal", "filters": {"Item No.": "1000"}})).unwrap();
    let l = &lines["records"][0];
    let calculated = dec(&l["Qty. (Calculated)"]);
    let key = json!(["PHYS INV", "DEFAULT", l["Line No."]]);
    // Counted 1 less than the books say.
    let after = s
        .run("record.validate", &json!({"table": "Item Journal Line", "key": key, "field": "Qty. (Phys. Inventory)", "value": calculated - 1.0}))
        .unwrap();
    assert_eq!(after["Entry Type"], "Negative Adjmt.");
    assert_eq!(dec(&after["Quantity"]), 1.0);
    s.run("item_journal.post", &json!({"template": "PHYS INV"})).unwrap();
    assert_eq!(stock_at(&mut s, "1000", "MAIN"), calculated - 1.0);
    // Every counted line is gone, the unchanged ones too.
    assert_eq!(s.run("record.list", &json!({"page": "phys-inventory-journal", "limit": 0})).unwrap()["total"], 0);
    balanced(&mut s);
}

#[test]
fn demo_fixed_assets_were_acquired_three_ways_and_depreciated() {
    let mut s = demo();
    let fas = s.run("record.list", &json!({"table": "Fixed Asset"})).unwrap();
    assert_eq!(fas["total"], 4);
    for fa in fas["records"].as_array().unwrap() {
        assert_eq!(fa["Acquired"], true, "{fa}");
        assert!(dec(&fa["Depreciation"]) < 0.0, "{fa}");
        assert!(dec(&fa["Book Value"]) < dec(&fa["Acquisition Cost"]));
    }
    // The grant (hibah) and the opening balance went to their own accounts.
    assert_eq!(gl(&mut s, "20400"), -95_000_000.0);
    // Book value agrees with the G/L: acquisition cost + accumulated depreciation.
    let bv = dec(&s.run("report.fa_book_value", &json!({})).unwrap()["book_value"]);
    assert!((bv - (gl(&mut s, "10500") + gl(&mut s, "10510"))).abs() < 0.01);
    balanced(&mut s);
}

#[test]
fn depreciation_is_straight_line_on_a_360_day_year() {
    let mut s = demo();
    // A new asset bought on 1 January, 5 years: one month is 1/60 of the cost.
    let fa = s.run("record.new", &json!({"table": "Fixed Asset", "values": {"Description": "Wheel Balancer", "FA Subclass Code": "EQUIPMENT"}})).unwrap();
    let no = fa["record"]["No."].as_str().unwrap().to_string();
    s.run("record.modify", &json!({"table": "FA Depreciation Book", "key": [no, "COMPANY"], "values": {"Depreciation Starting Date": "2027-01-01", "No. of Depreciation Years": 5}})).unwrap();
    let book = s.run("record.get", &json!({"table": "FA Depreciation Book", "key": [no, "COMPANY"]})).unwrap();
    assert_eq!(book["Depreciation Ending Date"], "2031-12-31");
    s.run("record.new", &json!({"page": "fa-gl-journal", "values": {"Posting Date": "2027-01-01", "Account Type": "Fixed Asset", "Account No.": no,
        "FA Posting Type": "Acquisition Cost", "Amount": 60_000_000, "Bal. Account Type": "Bank Account", "Bal. Account No.": "OPERATING"}})).unwrap();
    s.run("journal.post", &json!({"template": "ASSETS"})).unwrap();
    let r = s.run("fa.calculate_depreciation", &json!({"date": "2027-01-31", "fa_no": no, "post": "Yes"})).unwrap();
    assert_eq!(r["lines"], 1);
    let book = s.run("record.get", &json!({"table": "FA Depreciation Book", "key": [no, "COMPANY"]})).unwrap();
    assert_eq!(dec(&book["Depreciation"]), -1_000_000.0);
    assert_eq!(book["Last Depreciation Date"], "2027-01-31");
    // February is 30 days on a 360-day year too: the same amount again.
    s.run("fa.calculate_depreciation", &json!({"date": "2027-02-28", "fa_no": no, "post": "Yes"})).unwrap();
    let book = s.run("record.get", &json!({"table": "FA Depreciation Book", "key": [no, "COMPANY"]})).unwrap();
    assert_eq!(dec(&book["Depreciation"]), -2_000_000.0);
    // Nothing more to depreciate for the same period.
    assert_eq!(s.run("fa.calculate_depreciation", &json!({"date": "2027-02-28", "fa_no": no})).unwrap()["lines"], 0);
    balanced(&mut s);
}

#[test]
fn disposal_books_gain_or_loss_and_closes_the_asset() {
    let mut s = demo();
    let fa = s.run("record.get", &json!({"table": "Fixed Asset", "key": "FA000010"})).unwrap();
    let bv = dec(&fa["Book Value"]);
    let gains_before = gl(&mut s, "40500");
    // Sold for 10 million above book value, paid into the bank.
    let proceeds = bv + 10_000_000.0;
    s.run("record.new", &json!({"page": "fa-gl-journal", "values": {"Account Type": "Fixed Asset", "Account No.": "FA000010", "FA Posting Type": "Disposal",
        "Description": "Sale of tow truck", "Amount": -proceeds, "Bal. Account Type": "Bank Account", "Bal. Account No.": "OPERATING"}})).unwrap();
    s.run("journal.post", &json!({"template": "ASSETS"})).unwrap();
    let fa = s.run("record.get", &json!({"table": "Fixed Asset", "key": "FA000010"})).unwrap();
    assert_eq!(dec(&fa["Book Value"]), 0.0);
    assert!((gl(&mut s, "40500") - (gains_before - 10_000_000.0)).abs() < 0.01);
    let book = s.run("record.get", &json!({"table": "FA Depreciation Book", "key": ["FA000010", "COMPANY"]})).unwrap();
    assert!(book["Disposal Date"].as_str().is_some());
    // A disposed asset takes no more postings.
    s.run("record.new", &json!({"page": "fa-gl-journal", "values": {"Account Type": "Fixed Asset", "Account No.": "FA000010", "FA Posting Type": "Depreciation",
        "Amount": -1, "Bal. Account Type": "G/L Account", "Bal. Account No.": "60500"}})).unwrap();
    let e = s.run("journal.post", &json!({"template": "ASSETS"})).unwrap_err();
    assert!(e.message.contains("disposed"), "{e}");
    balanced(&mut s);
}
