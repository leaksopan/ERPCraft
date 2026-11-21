#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Phase 2: journals, applications, reversal, separate shipping / invoicing, financial reports.

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

fn count(s: &mut Session, table: &str) -> u64 {
    s.run("record.list", &json!({"table": table, "limit": 0})).unwrap()["total"].as_u64().unwrap()
}

fn balanced(s: &mut Session) {
    assert_eq!(s.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
}

/// A journal line through `record.new` on the journal page, as the UI creates it.
fn jnl_line(s: &mut Session, page: &str, values: Value) -> Value {
    s.run("record.new", &json!({"page": page, "values": values})).unwrap()["record"].clone()
}

/// The open invoice of customer 20000 posted by the demo (overdue).
fn open_invoice(s: &mut Session, customer: &str) -> (String, f64, i64) {
    let r = s
        .run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Customer No.": customer, "Document Type": "Invoice", "Open": "true"}}))
        .unwrap();
    let e = &r["records"][0];
    (e["Document No."].as_str().unwrap().to_string(), dec(&e["Remaining Amount"]), e["Entry No."].as_i64().unwrap())
}

#[test]
fn cash_receipt_journal_applies_payment_to_invoice() {
    let mut s = demo();
    let (inv, remaining, _) = open_invoice(&mut s, "20000");
    assert!(remaining > 0.0);
    let line = jnl_line(&mut s, "cash-receipt-journal", json!({"Account Type": "Customer", "Account No.": "20000", "Applies-to Doc. No.": inv}));
    // Applies-to fills the amount and the batch gives the bank as balancing account.
    assert_eq!(dec(&line["Amount"]), -remaining);
    assert_eq!(line["Bal. Account No."], "OPERATING");
    assert_eq!(line["Document Type"], "Payment");
    assert_eq!(line["Document No."], "GR00001");
    let registers = count(&mut s, "G/L Register");
    let r = s.run("journal.post", &json!({"template": "CASHRCPT"})).unwrap();
    assert_eq!(r["transactions"], 1);
    assert_eq!(count(&mut s, "G/L Register"), registers + 1);
    assert_eq!(s.run("record.list", &json!({"page": "cash-receipt-journal", "limit": 0})).unwrap()["total"], 0);
    let (_, _, _) = (inv.clone(), 0, 0);
    let inv_entry = s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Document No.": inv}})).unwrap();
    assert_eq!(inv_entry["records"][0]["Open"], false);
    assert_eq!(dec(&inv_entry["records"][0]["Remaining Amount"]), 0.0);
    // Initial entries + an application pair.
    let dtld = s.run("record.list", &json!({"table": "Detailed Cust. Ledg. Entry", "filters": {"Customer No.": "20000", "Entry Type": "Application"}})).unwrap();
    assert_eq!(dtld["total"], 2);
    // The number series moved on.
    let next = jnl_line(&mut s, "cash-receipt-journal", json!({}));
    assert_eq!(next["Document No."], "GR00002");
    balanced(&mut s);
}

#[test]
fn unbalanced_general_journal_is_refused() {
    let mut s = demo();
    jnl_line(&mut s, "general-journal", json!({"Account No.": "60200", "Debit Amount": 500000}));
    let check = s.run("journal.check", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(check["ok"], false);
    let e = s.run("journal.post", &json!({"template": "GENERAL"})).unwrap_err();
    assert!(e.message.contains("is out of balance by 500000"), "{e}");
    // Balance it with a second line in the same document: rent paid from cash.
    jnl_line(&mut s, "general-journal", json!({"Account No.": "10110", "Credit Amount": 500000}));
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
    let rent = s.run("record.get", &json!({"table": "G/L Account", "key": "60200"})).unwrap();
    assert_eq!(dec(&rent["Balance"]), 500000.0);
    balanced(&mut s);
}

#[test]
fn unapply_reopens_and_apply_closes_again() {
    let mut s = demo();
    // Customer 10000: the demo payment of 2,000,000 is applied to its invoice.
    let pay = s.run("record.list", &json!({"table": "Cust. Ledger Entry", "filters": {"Customer No.": "10000", "Document Type": "Payment"}})).unwrap();
    let pay_no = pay["records"][0]["Entry No."].as_i64().unwrap();
    let (_, before, inv_entry) = open_invoice(&mut s, "10000");
    s.run("customer.entries.unapply", &json!({"entry_no": pay_no})).unwrap();
    let inv = s.run("record.get", &json!({"table": "Cust. Ledger Entry", "key": inv_entry})).unwrap();
    assert_eq!(dec(&inv["Remaining Amount"]), before + 2_000_000.0);
    let p = s.run("record.get", &json!({"table": "Cust. Ledger Entry", "key": pay_no})).unwrap();
    assert_eq!(p["Open"], true);
    assert_eq!(dec(&p["Remaining Amount"]), -2_000_000.0);
    // Apply again from the ledger entry page (selected entry as context, oldest open first).
    s.run("customer.entries.apply", &json!({"table": "Cust. Ledger Entry", "key": [pay_no]})).unwrap();
    let inv = s.run("record.get", &json!({"table": "Cust. Ledger Entry", "key": inv_entry})).unwrap();
    assert_eq!(dec(&inv["Remaining Amount"]), before);
    // Balances never changed: applications move no money.
    balanced(&mut s);
}

#[test]
fn reverse_journal_transaction_but_not_documents() {
    let mut s = demo();
    let bank_before = dec(&s.run("record.get", &json!({"table": "Bank Account", "key": "OPERATING"})).unwrap()["Balance"]);
    jnl_line(&mut s, "payment-journal", json!({"Account Type": "Vendor", "Account No.": "30000", "Amount": 1000000}));
    s.run("journal.post", &json!({"template": "PAYMENT"})).unwrap();
    let pay = s.run("record.list", &json!({"table": "Vendor Ledger Entry", "filters": {"Vendor No.": "30000", "Document Type": "Payment"}})).unwrap();
    let e = &pay["records"][0];
    assert_eq!(dec(&e["Amount"]), 1_000_000.0);
    let entry = e["Entry No."].as_i64().unwrap();
    let bank_mid = dec(&s.run("record.get", &json!({"table": "Bank Account", "key": "OPERATING"})).unwrap()["Balance"]);
    assert_eq!(bank_mid, bank_before - 1_000_000.0);
    // Reverse from the selected vendor ledger entry.
    s.run("transaction.reverse", &json!({"table": "Vendor Ledger Entry", "key": [entry]})).unwrap();
    let e = s.run("record.get", &json!({"table": "Vendor Ledger Entry", "key": entry})).unwrap();
    assert_eq!(e["Reversed"], true);
    assert_eq!(e["Open"], false);
    let bank_after = dec(&s.run("record.get", &json!({"table": "Bank Account", "key": "OPERATING"})).unwrap()["Balance"]);
    assert_eq!(bank_after, bank_before);
    assert!(s.run("transaction.reverse", &json!({"table": "Vendor Ledger Entry", "key": [entry]})).is_err());
    balanced(&mut s);
    // An invoice posted from a document is corrected with a credit memo, not reversed.
    let (_, _, inv_entry) = open_invoice(&mut s, "20000");
    let err = s.run("transaction.reverse", &json!({"table": "Cust. Ledger Entry", "key": [inv_entry]})).unwrap_err();
    assert!(err.message.contains("credit memo"), "{err}");
}

#[test]
fn ship_then_invoice_an_order() {
    let mut s = demo();
    let r = s.run("sales.document.new", &json!({"customer_no": "30000", "lines": [{"no": "1100", "quantity": 4}, {"no": "9000", "quantity": 2}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let stock_before = dec(&s.run("record.get", &json!({"table": "Item", "key": "1100"})).unwrap()["Inventory"]);
    let gl_before = count(&mut s, "G/L Entry");

    let shipped = s.run("sales.document.post", &json!({"no": no, "mode": "Ship"})).unwrap();
    let shipment = shipped["shipment_no"].as_str().unwrap().to_string();
    assert!(shipped.get("posted_no").is_none());
    assert_eq!(shipped["source_document_deleted"], false);
    assert_eq!(count(&mut s, "G/L Entry"), gl_before, "shipping alone posts nothing to the G/L");
    let stock_mid = dec(&s.run("record.get", &json!({"table": "Item", "key": "1100"})).unwrap()["Inventory"]);
    assert_eq!(stock_mid, stock_before - 4.0);
    let sh = s.run("record.get", &json!({"table": "Sales Shipment Header", "key": shipment})).unwrap();
    assert_eq!(sh["Order No."], json!(no));
    let line = s.run("record.get", &json!({"table": "Sales Line", "key": ["Order", no, 10000]})).unwrap();
    assert_eq!(dec(&line["Quantity Shipped"]), 4.0);
    assert_eq!(dec(&line["Qty. Shipped Not Invoiced"]), 4.0);
    assert_eq!(dec(&line["Qty. to Ship"]), 0.0);
    assert_eq!(dec(&line["Qty. to Invoice"]), 4.0);

    let invoiced = s.run("sales.document.post", &json!({"no": no, "mode": "Invoice"})).unwrap();
    assert!(invoiced["posted_no"].as_str().is_some());
    assert_eq!(invoiced["source_document_deleted"], true);
    let stock_after = dec(&s.run("record.get", &json!({"table": "Item", "key": "1100"})).unwrap()["Inventory"]);
    assert_eq!(stock_after, stock_mid, "invoicing a shipped order moves no stock");
    balanced(&mut s);
}

#[test]
fn cannot_invoice_more_than_shipped() {
    let mut s = demo();
    let r = s.run("sales.document.new", &json!({"customer_no": "30000", "lines": [{"no": "1100", "quantity": 4}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    s.run("record.validate", &json!({"table": "Sales Line", "key": ["Order", no, 10000], "field": "Qty. to Ship", "value": 1})).unwrap();
    s.run("record.validate", &json!({"table": "Sales Line", "key": ["Order", no, 10000], "field": "Qty. to Invoice", "value": 3})).unwrap();
    let e = s.run("sales.document.post", &json!({"no": no})).unwrap_err();
    assert!(e.message.contains("cannot invoice more than you have shipped"), "{e}");
}

#[test]
fn receive_then_invoice_a_purchase_order() {
    let mut s = demo();
    let r = s
        .run("purchase.document.new", &json!({"vendor_no": "40000", "values": {"Vendor Invoice No.": "KMA-9001"}, "lines": [{"no": "1300", "quantity": 10}]}))
        .unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let before = dec(&s.run("record.get", &json!({"table": "Item", "key": "1300"})).unwrap()["Inventory"]);
    let rec = s.run("purchase.document.post", &json!({"no": no, "mode": "Receive"})).unwrap();
    assert!(rec["receipt_no"].as_str().is_some());
    assert_eq!(dec(&s.run("record.get", &json!({"table": "Item", "key": "1300"})).unwrap()["Inventory"]), before + 10.0);
    let inv = s.run("purchase.document.post", &json!({"no": no, "mode": "Invoice"})).unwrap();
    assert_eq!(inv["source_document_deleted"], true);
    balanced(&mut s);
}

#[test]
fn financial_reports() {
    let mut s = demo();
    let bs = s.run("report.balance_sheet", &json!({})).unwrap();
    assert_eq!(bs["balanced"], true, "{bs:#}");
    let is = s.run("report.income_statement", &json!({"from": "2000-01-01"})).unwrap();
    // A year of depreciation on the demo's fixed assets is part of the result.
    let rows = is["report"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r[0].as_str().unwrap().contains("60500 Depreciation Expense") && dec(&r[1]) > 0.0), "{is:#}");
    // Customer 20000's demo invoice is 40 days old with 14 days terms: 26 days overdue.
    let ar = s.run("report.aged_receivables", &json!({})).unwrap();
    let row = ar["report"]["rows"].as_array().unwrap().iter().find(|r| r[0].as_str().unwrap().starts_with("20000")).unwrap().clone();
    assert!(dec(&row[2]) > 0.0, "1-30 days bucket: {row}");
    let ap = s.run("report.aged_payables", &json!({})).unwrap();
    assert!(dec(&ap["total"]) > 0.0);
}

#[test]
fn journal_preview_test_report_and_renumber() {
    let mut s = demo();
    jnl_line(&mut s, "general-journal", json!({"Account No.": "60200", "Debit Amount": 500000}));
    let bad = s.run("journal.test_report", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(bad["ok"], false);
    jnl_line(&mut s, "general-journal", json!({"Account No.": "10110", "Credit Amount": 500000}));
    let ok = s.run("journal.test_report", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(ok["ok"], true, "{ok}");
    let gl_before = count(&mut s, "G/L Entry");
    let preview = s.run("journal.preview_post", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(preview["report"]["rows"][0], json!(["G/L Entry", 2]));
    assert_eq!(count(&mut s, "G/L Entry"), gl_before, "preview posts nothing");
    // Renumber: give the second line its own number, then renumber puts them in sequence.
    let lines = s.run("record.list", &json!({"page": "general-journal"})).unwrap();
    let second = &lines["records"][1];
    s.run(
        "record.validate",
        &json!({"table": "Gen. Journal Line", "key": [second["Journal Template Name"], second["Journal Batch Name"], second["Line No."]], "field": "Document No.", "value": "ZZ999"}),
    )
    .unwrap();
    s.run("journal.renumber", &json!({"template": "GENERAL"})).unwrap();
    let after = s.run("record.list", &json!({"page": "general-journal"})).unwrap();
    let nos: Vec<&str> = after["records"].as_array().unwrap().iter().map(|l| l["Document No."].as_str().unwrap()).collect();
    assert_eq!(nos.len(), 2);
    assert_ne!(nos[0], nos[1], "two documents stay two documents");
    assert!(!nos.contains(&"ZZ999"));
}

#[test]
fn journal_post_and_print_card_and_ledger_entries() {
    let mut s = demo();
    jnl_line(&mut s, "general-journal", json!({"Account No.": "60200", "Debit Amount": 300}));
    jnl_line(&mut s, "general-journal", json!({"Account No.": "10110", "Credit Amount": 300}));
    let lines = s.run("record.list", &json!({"page": "general-journal"})).unwrap();
    let l = &lines["records"][0];
    let key = json!([l["Journal Template Name"], l["Journal Batch Name"], l["Line No."]]);
    let card = s.run("journal.line.card", &json!({"table": "Gen. Journal Line", "key": key})).unwrap();
    assert_eq!(card["open"], json!({"page": "gl-account-card", "key": "60200"}));
    let le = s.run("journal.line.ledger_entries", &json!({"table": "Gen. Journal Line", "key": key})).unwrap();
    assert_eq!(le["open"]["page"], "general-ledger-entries");
    assert_eq!(le["open"]["filters"], json!([["G/L Account No.", "60200"]]));
    let r = s.run("journal.post_and_print", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(r["report"]["title"], "G/L Register");
    assert_eq!(r["report"]["rows"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(s.run("record.list", &json!({"page": "general-journal"})).unwrap()["records"].as_array().unwrap().len(), 0);
}
