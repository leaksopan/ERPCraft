#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Recurring general journals: fixed and reversing lines, allocations, and the line moving on.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn gl(s: &mut Session, account: &str, date: &str) -> f64 {
    s.run("record.list", &json!({"table": "G/L Entry", "filters": {"G/L Account No.": account, "Posting Date": date}})).unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| d(&e["Amount"]))
        .sum()
}

#[test]
fn a_monthly_line_posts_with_its_allocations_and_moves_to_next_month() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    // Rent, split 60 / 40 between rent and utilities, paid from the bank.
    let line = s
        .run(
            "record.new",
            &json!({"page": "recurring-general-journal", "values": {"Posting Date": "2026-12-01", "Document No.": "RENT01", "Recurring Method": "F  Fixed",
                     "Recurring Frequency": "1M", "Account No.": "10120", "Description": "Monthly rent", "Credit Amount": 10_000_000}}),
        )
        .unwrap();
    let line_no = line["record"]["Line No."].clone();
    for (acc, pct) in [("60200", 60), ("60300", 40)] {
        s.run(
            "record.new",
            &json!({"table": "Gen. Jnl. Allocation", "values": {"Journal Template Name": "RECURRING", "Journal Batch Name": "DEFAULT", "Journal Line No.": line_no, "Account No.": acc, "Allocation %": pct}}),
        )
        .unwrap();
    }
    let r = s.run("journal.post", &json!({"template": "RECURRING"})).unwrap();
    assert_eq!(r["lines"], 1, "{r}");
    assert_eq!(gl(&mut s, "60200", "2026-12-01"), 6_000_000.0);
    assert_eq!(gl(&mut s, "60300", "2026-12-01"), 4_000_000.0);
    // The line stays, a month later with the next document number.
    let l = &s.run("record.list", &json!({"page": "recurring-general-journal"})).unwrap()["records"][0];
    assert_eq!((l["Posting Date"].as_str(), l["Document No."].as_str(), d(&l["Amount"])), (Some("2027-01-01"), Some("RENT02"), -10_000_000.0));
}

#[test]
fn reversing_lines_undo_the_next_day_and_unbalanced_lines_are_refused() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run(
        "record.new",
        &json!({"page": "recurring-general-journal", "values": {"Posting Date": "2026-11-30", "Document No.": "ACC01", "Recurring Method": "RV Reversing Variable",
                 "Recurring Frequency": "1M", "Account No.": "60300", "Description": "Accrued utilities", "Debit Amount": 2_500_000, "Bal. Account No.": "20200"}}),
    )
    .unwrap();
    s.run("journal.post", &json!({"template": "RECURRING"})).unwrap();
    assert_eq!(gl(&mut s, "60300", "2026-11-30"), 2_500_000.0);
    assert_eq!(gl(&mut s, "60300", "2026-12-01"), -2_500_000.0, "reversed the next day");
    let l = &s.run("record.list", &json!({"page": "recurring-general-journal"})).unwrap()["records"][0];
    assert_eq!(d(&l["Amount"]), 0.0, "a variable amount is cleared after posting");
    // A line without a balancing account or allocations doesn't post.
    s.run(
        "record.new",
        &json!({"page": "recurring-general-journal", "values": {"Posting Date": "2026-12-01", "Document No.": "X01", "Recurring Method": "F  Fixed",
                 "Recurring Frequency": "1M", "Account No.": "60300", "Debit Amount": 100}}),
    )
    .unwrap();
    assert!(s.run("journal.post", &json!({"template": "RECURRING"})).unwrap_err().message.contains("does not balance"));
}
