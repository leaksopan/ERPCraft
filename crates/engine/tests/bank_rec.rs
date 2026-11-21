#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Bank account reconciliation: suggest, match, post.

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

fn new_rec(s: &mut Session) -> Value {
    let r = s.run("record.new", &json!({"table": "Bank Acc. Reconciliation", "values": {"Bank Account No.": "OPERATING"}})).unwrap();
    r["key"].clone()
}

#[test]
fn suggest_post_and_the_next_statement() {
    let mut s = demo();
    let key = new_rec(&mut s);
    assert_eq!(key, json!(["OPERATING", "1"]), "the first statement is 1");
    let r = s.run("bank_rec.suggest_lines", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap();
    let n = r["lines"].as_i64().unwrap();
    assert!(n > 0, "{r}");
    let rec = s.run("record.get", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap();
    let total = d(&rec["Total Transaction Amount"]);
    assert_eq!(d(&rec["Total Difference"]), 0.0, "every suggested line is applied: {rec}");
    // A wrong ending balance is refused.
    assert!(s.run("bank_rec.post", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap_err().message.contains("Statement Ending Balance"));
    s.run("record.validate", &json!({"table": "Bank Acc. Reconciliation", "key": key, "field": "Statement Ending Balance", "value": total})).unwrap();
    let posted = s.run("bank_rec.post", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap();
    assert_eq!(posted["open"]["page"], "bank-account-statement");
    let open = s.run("record.list", &json!({"table": "Bank Account Ledger Entry", "filters": {"Bank Account No.": "OPERATING", "Open": "true"}})).unwrap();
    assert_eq!(open["records"].as_array().unwrap().len(), 0, "every entry is closed");
    let bank = s.run("record.get", &json!({"table": "Bank Account", "key": "OPERATING"})).unwrap();
    assert_eq!(bank["Last Statement No."], "1");
    assert_eq!(d(&bank["Balance Last Statement"]), total);
    let lines = s.run("record.list", &json!({"table": "Bank Account Statement Line", "filters": {"Statement No.": "1"}})).unwrap();
    assert_eq!(lines["records"].as_array().unwrap().len() as i64, n);
    // The next reconciliation continues from there.
    let next = new_rec(&mut s);
    assert_eq!(next, json!(["OPERATING", "2"]));
    let rec2 = s.run("record.get", &json!({"table": "Bank Acc. Reconciliation", "key": next})).unwrap();
    assert_eq!(d(&rec2["Balance Last Statement"]), total);
}

#[test]
fn match_automatically_pairs_lines_with_entries_of_the_same_amount() {
    let mut s = demo();
    let key = new_rec(&mut s);
    let e = &s.run("record.list", &json!({"table": "Bank Account Ledger Entry", "filters": {"Bank Account No.": "OPERATING"}})).unwrap()["records"][0];
    let amount = d(&e["Amount"]);
    s.run(
        "record.new",
        &json!({"table": "Bank Acc. Reconciliation Line", "values": {"Bank Account No.": "OPERATING", "Statement No.": "1", "Statement Line No.": 10000,
                 "Transaction Date": e["Posting Date"], "Description": "From the bank", "Statement Amount": amount}}),
    )
    .unwrap();
    let line = s.run("record.get", &json!({"table": "Bank Acc. Reconciliation Line", "key": ["OPERATING", "1", 10000]})).unwrap();
    assert_eq!(d(&line["Difference"]), amount, "unmatched: all of it is a difference");
    let r = s.run("bank_rec.match", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap();
    assert_eq!(r["matched"], 1);
    let line = s.run("record.get", &json!({"table": "Bank Acc. Reconciliation Line", "key": ["OPERATING", "1", 10000]})).unwrap();
    assert_eq!((d(&line["Applied Amount"]), d(&line["Difference"]), line["Applied Entries"].as_i64()), (amount, 0.0, Some(1)));
    s.run("bank_rec.remove_match", &json!({"table": "Bank Acc. Reconciliation", "key": key})).unwrap();
    let line = s.run("record.get", &json!({"table": "Bank Acc. Reconciliation Line", "key": ["OPERATING", "1", 10000]})).unwrap();
    assert_eq!(d(&line["Applied Amount"]), 0.0);
}
