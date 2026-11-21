#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! VAT entries from posting, the VAT statement, and Calc. and Post VAT Settlement.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn row<'a>(rep: &'a Value, no: &str) -> &'a Vec<Value> {
    rep["report"]["rows"].as_array().unwrap().iter().find(|r| r[0] == no).and_then(Value::as_array).unwrap()
}

fn vat_sum(s: &mut Session, ty: &str, field: &str) -> f64 {
    let r = s.run("record.list", &json!({"table": "VAT Entry", "filters": {"Type": ty}})).unwrap();
    r["records"].as_array().unwrap().iter().map(|e| d(&e[field])).sum()
}

#[test]
fn posting_writes_vat_entries_and_the_statement_adds_them_up() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap();
    let e = &s.run("record.list", &json!({"table": "VAT Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    assert_eq!(e["Type"], "Sale");
    let (base, amount) = (d(&e["Base"]), d(&e["Amount"]));
    assert!(base < 0.0 && amount < 0.0, "sales VAT is a credit: {e}");
    assert_eq!(e["Bill-to/Pay-to No."], "10000");
    // The VAT entry matches the G/L VAT line.
    let gl = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Document No.": inv, "G/L Account No.": "20300"}})).unwrap();
    let gl_vat: f64 = gl["records"].as_array().unwrap().iter().map(|e| d(&e["Amount"])).sum();
    assert!((gl_vat - amount).abs() < 0.01);

    let st = s.run("vat_statement.preview", &json!({"template": "VAT", "name": "DEFAULT"})).unwrap();
    let output = d(&row(&st, "10")[3]);
    let input = d(&row(&st, "20")[3]);
    assert!((output + vat_sum(&mut s, "Sale", "Amount")).abs() < 0.01, "{st}");
    assert!((input - vat_sum(&mut s, "Purchase", "Amount")).abs() < 0.01);
    assert!((d(&row(&st, "30")[3]) - (output - input)).abs() < 0.01, "VAT to pay = output - input");
}

#[test]
fn the_vat_settlement_closes_the_period_and_moves_the_net_amount() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let open_net: f64 = vat_sum(&mut s, "Sale", "Amount") + vat_sum(&mut s, "Purchase", "Amount");
    assert!(s.run("vat.settlement", &json!({"document_no": "VATSET", "settlement_account": ""})).unwrap_err().message.contains("settlement account"));
    let test = s.run("vat.settlement", &json!({"document_no": "VATSET", "settlement_account": "20500", "post": false})).unwrap();
    assert!(test["report"]["title"].as_str().unwrap().contains("Not Posted"));
    assert_eq!(s.run("record.list", &json!({"table": "VAT Entry", "filters": {"Type": "Settlement"}})).unwrap()["records"].as_array().unwrap().len(), 0);

    let r = s.run("vat.settlement", &json!({"document_no": "VATSET", "settlement_account": "20500"})).unwrap();
    assert!((d(&r["total"]) - open_net).abs() < 0.01, "{r}");
    let open = s.run("record.list", &json!({"table": "VAT Entry", "filters": {"Closed": "false"}})).unwrap();
    assert_eq!(open["records"].as_array().unwrap().len(), 0, "every VAT entry is closed");
    let settle: f64 = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"Document No.": "VATSET", "G/L Account No.": "20500"}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| d(&e["Amount"]))
        .sum();
    assert!((settle - open_net).abs() < 0.01);
    // Only open entries: nothing left; the balance sheet still balances.
    let st = s.run("vat_statement.preview", &json!({"template": "VAT", "name": "DEFAULT", "selection": "Open"})).unwrap();
    assert_eq!(d(&row(&st, "30")[3]), 0.0);
    let bs = s.run("financial_report.overview", &json!({"name": "BALANCE"})).unwrap();
    assert!((d(&row(&bs, "19")[2]) - d(&row(&bs, "40")[2])).abs() < 0.01);
    assert_eq!(s.run("vat.settlement", &json!({"document_no": "VATSET2", "settlement_account": "20500"})).unwrap()["message"], "There is no open VAT to settle.");
}
