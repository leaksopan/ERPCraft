#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Currencies: documents in EUR post LCY to the G/L, payments at another rate post a realized
//! gain or loss, Adjust Exchange Rates posts unrealized ones, and unapplying undoes them.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    // Customer 50000 buys in euro; the euro rises on November 1.
    s.run("record.validate", &json!({"table": "Customer", "key": "50000", "field": "Currency Code", "value": "EUR"})).unwrap();
    s.run(
        "record.new",
        &json!({"table": "Currency Exchange Rate", "values": {"Currency Code": "EUR", "Starting Date": "2026-11-01", "Exchange Rate Amount": 1, "Relational Exch. Rate Amount": 18000, "Adjustment Exch. Rate Amount": 1, "Relational Adjmt Exch Rate Amt": 18000}}),
    )
    .unwrap();
    s
}

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap()).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn gl_balance(s: &mut Session, account: &str) -> f64 {
    let r = s.run("record.list", &json!({"table": "G/L Entry", "filters": {"G/L Account No.": account}})).unwrap();
    r["records"].as_array().unwrap().iter().map(|e| d(&e["Amount"])).sum()
}

/// An October invoice (rate 17,500) for one item; returns (invoice no., EUR incl. VAT, LCY).
fn invoice(s: &mut Session) -> (String, f64, f64) {
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "50000"})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Sales Header", "key": ["Invoice", no], "field": "Posting Date", "value": "2026-10-15"})).unwrap();
    let h = s.run("record.get", &json!({"table": "Sales Header", "key": ["Invoice", no]})).unwrap();
    assert!((d(&h["Currency Factor"]) - 1.0 / 17500.0).abs() < 1e-12, "{h}");
    s.run("sales.document.add_line", &json!({"document_type": "Invoice", "no": no, "type": "Item", "item_no": "1000", "quantity": 2})).ok();
    if s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document No.": no}})).unwrap()["records"].as_array().unwrap().is_empty() {
        s.run("record.new", &json!({"table": "Sales Line", "values": {"Document Type": "Invoice", "Document No.": no, "Type": "Item", "No.": "1000", "Quantity": 2}})).unwrap();
    }
    let line = &s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document No.": no}})).unwrap()["records"][0];
    let price = d(&line["Unit Price"]);
    assert!((price - 450_000.0 / 17_500.0).abs() < 0.001, "the LCY price in euro: {price}");
    let posted = s.run("sales.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let inv = posted["posted_no"].as_str().unwrap().to_string();
    let e = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    assert_eq!(e["Currency Code"], "EUR");
    let (eur, lcy) = (d(&e["Amount"]), d(&e["Amount (LCY)"]));
    assert!((lcy - eur * 17_500.0).abs() < 1.0, "EUR {eur} at 17,500 = {lcy}");
    (inv, eur, lcy)
}

fn pay(s: &mut Session, inv: &str, eur: f64) {
    s.run(
        "record.new",
        &json!({"page": "general-journal", "values": {"Posting Date": "2026-11-21", "Document No.": "PAY-EUR", "Account Type": "Customer", "Account No.": "50000",
                 "Currency Code": "EUR", "Amount": -eur, "Bal. Account No.": "10120", "Applies-to Doc. Type": "Invoice", "Applies-to Doc. No.": inv}}),
    )
    .unwrap();
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
}

#[test]
fn an_invoice_in_euro_posts_lcy_and_its_payment_a_realized_gain() {
    let mut s = demo();
    let receivables_before = gl_balance(&mut s, "10200");
    let (inv, eur, lcy) = invoice(&mut s);
    assert!((gl_balance(&mut s, "10200") - receivables_before - lcy).abs() < 0.01, "receivables grow by the LCY amount");
    pay(&mut s, &inv, eur);
    // Paid in November at 18,000: the customer paid more LCY than invoiced.
    let gain = -gl_balance(&mut s, "40600");
    assert!((gain - (eur * 18_000.0 - lcy)).abs() < 1.0, "realized gain {gain}");
    let e = &s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap()["records"][0];
    assert_eq!(e["Open"], false);
    assert!((gl_balance(&mut s, "10200") - receivables_before).abs() < 0.01, "receivables back where they were");
    // Unapplying takes the gain back off.
    let pay_entry = s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": "PAY-EUR"}})).unwrap()["records"][0]["Entry No."].clone();
    s.run("customer.entries.unapply", &json!({"entry_no": pay_entry})).unwrap();
    assert!(gl_balance(&mut s, "40600").abs() < 0.01);
}

#[test]
fn adjust_exchange_rates_posts_unrealized_and_the_payment_reverses_it() {
    let mut s = demo();
    let (inv, eur, lcy) = invoice(&mut s);
    let r = s.run("currency.adjust_exchange_rates", &json!({"ending_date": "2026-11-20", "currency_code": "EUR"})).unwrap();
    assert_eq!(r["entries"], 1, "{r}");
    let unrealized = -gl_balance(&mut s, "40700");
    assert!((unrealized - (eur * 18_000.0 - lcy)).abs() < 1.0, "unrealized gain {unrealized}");
    // Running it again at the same rate changes nothing.
    assert_eq!(s.run("currency.adjust_exchange_rates", &json!({"ending_date": "2026-11-20", "currency_code": "EUR"})).unwrap()["entries"], 0);
    pay(&mut s, &inv, eur);
    assert!(gl_balance(&mut s, "40700").abs() < 0.01, "the unrealized gain is reversed");
    assert!((-gl_balance(&mut s, "40600") - unrealized).abs() < 1.0, "and realized");
    let c = s.run("record.get", &json!({"table": "Currency", "key": "EUR"})).unwrap();
    assert_eq!(c["Last Date Adjusted"], "2026-11-20");
}

#[test]
fn applying_entries_of_different_currencies_is_refused() {
    let mut s = demo();
    let (inv, eur, _) = invoice(&mut s);
    s.run(
        "record.new",
        &json!({"page": "general-journal", "values": {"Posting Date": "2026-11-21", "Document No.": "PAY-USD", "Account Type": "Customer", "Account No.": "50000",
                 "Currency Code": "USD", "Amount": -eur, "Bal. Account No.": "10120", "Applies-to Doc. Type": "Invoice", "Applies-to Doc. No.": inv}}),
    )
    .unwrap();
    let r = s.run("journal.post", &json!({"template": "GENERAL"}));
    assert!(r.unwrap_err().message.contains("same currency"));
}
