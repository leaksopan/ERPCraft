#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Document approvals: workflow on, Send Approval Request, the approver chain with limits,
//! Approve / Reject / Delegate, and Release / Post refusing until approved.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.set_user("SALESREP");
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("record.validate", &json!({"table": "Workflow", "key": "MS-SOAPW", "field": "Enabled", "value": true})).unwrap();
    for user in ["MANAGER", "DIRECTOR", "DEPUTY"] {
        s.run("record.new", &json!({"table": "User", "values": {"User Name": user, "Full Name": user, "State": "Enabled"}})).unwrap();
        s.run("record.new", &json!({"table": "Access Control", "values": {"User Name": user, "Role ID": "SUPER"}})).unwrap();
    }
    for (user, approver, limit, sub) in [("SALESREP", "MANAGER", 0, ""), ("MANAGER", "DIRECTOR", 1_000_000, "DEPUTY"), ("DIRECTOR", "", 0, ""), ("DEPUTY", "", 0, "")] {
        s.run(
            "record.new",
            &json!({"table": "User Setup", "values": {"User ID": user, "Approver ID": approver, "Sales Amount Approval Limit": limit, "Unlimited Sales Approval": user == "DIRECTOR", "Substitute": sub}}),
        )
        .unwrap();
    }
    s
}

fn status(s: &mut Session, no: &str) -> String {
    s.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).unwrap()["Status"].as_str().unwrap().to_string()
}

fn open_entry(s: &mut Session, no: &str) -> Value {
    let r = s.run("record.list", &json!({"table": "Approval Entry", "filters": {"Document No.": no, "Status": "Open"}})).unwrap();
    r["records"][0].clone()
}

#[test]
fn a_large_order_goes_up_the_chain_and_is_released_by_the_last_approval() {
    let mut s = demo();
    let release = s.run("sales.document.release", &json!({"document_type": "Order", "no": "S-ORD1002"}));
    assert!(release.unwrap_err().message.contains("approval process is complete"));
    let post = s.run("sales.document.post", &json!({"document_type": "Order", "no": "S-ORD1002"}));
    assert!(post.unwrap_err().message.contains("must be approved and released"));

    let r = s.run("sales.document.send_approval_request", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert_eq!(r["approvers"], json!(["MANAGER", "DIRECTOR"]), "5.5 million is over the manager's 1 million limit");
    assert_eq!(status(&mut s, "S-ORD1002"), "Pending Approval");

    let e = open_entry(&mut s, "S-ORD1002");
    assert!(s.run("approval.approve", &json!({"entry_no": e["Entry No."]})).is_err(), "only the approver may approve");
    s.set_user("MANAGER");
    let r = s.run("approval.approve", &json!({"entry_no": e["Entry No."]})).unwrap();
    assert_eq!(r["status"], "Pending Approval");
    s.set_user("DIRECTOR");
    let e = open_entry(&mut s, "S-ORD1002");
    assert_eq!(e["Approver ID"], "DIRECTOR");
    let r = s.run("approval.approve", &json!({"entry_no": e["Entry No."]})).unwrap();
    assert_eq!(r["status"], "Released");
    assert_eq!(status(&mut s, "S-ORD1002"), "Released");
    s.run("sales.document.post", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
}

#[test]
fn reject_reopens_and_cancel_withdraws() {
    let mut s = demo();
    s.run("sales.document.send_approval_request", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    s.set_user("MANAGER");
    let e = open_entry(&mut s, "S-ORD1002");
    s.run("approval.reject", &json!({"entry_no": e["Entry No."]})).unwrap();
    assert_eq!(status(&mut s, "S-ORD1002"), "Open");
    s.set_user("SALESREP");
    s.run("sales.document.send_approval_request", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    s.run("sales.document.cancel_approval_request", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert_eq!(status(&mut s, "S-ORD1002"), "Open");
    let left = s.run("record.list", &json!({"table": "Approval Entry", "filters": {"Document No.": "S-ORD1002", "Status": "Open|Created"}})).unwrap();
    assert!(left["records"].as_array().unwrap().is_empty());
}

#[test]
fn delegate_passes_the_request_to_the_substitute() {
    let mut s = demo();
    s.run("sales.document.send_approval_request", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    s.set_user("MANAGER");
    let e = open_entry(&mut s, "S-ORD1002");
    s.run("approval.delegate", &json!({"entry_no": e["Entry No."]})).unwrap();
    assert_eq!(open_entry(&mut s, "S-ORD1002")["Approver ID"], "DEPUTY");
}

#[test]
fn without_a_workflow_nothing_changes() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run("sales.document.release", &json!({"document_type": "Order", "no": "S-ORD1002"})).unwrap();
    assert!(s.run("sales.document.send_approval_request", &json!({"document_type": "Order", "no": "S-ORD1003"})).unwrap_err().message.contains("No approval workflow"));
}

fn jline(s: &mut Session, values: Value) -> Value {
    s.run("record.new", &json!({"page": "general-journal", "values": values})).unwrap()
}

fn open_for(s: &mut Session, record: &str) -> Value {
    let r = s.run("record.list", &json!({"table": "Approval Entry", "filters": {"Record ID to Approve": record, "Status": "Open"}})).unwrap();
    r["records"][0].clone()
}

#[test]
fn a_journal_batch_under_the_batch_workflow_posts_only_once_approved() {
    let mut s = demo();
    s.run("record.validate", &json!({"table": "Workflow", "key": "MS-GJBAPW", "field": "Enabled", "value": true})).unwrap();
    jline(&mut s, json!({"Account No.": "60200", "Debit Amount": 300}));
    jline(&mut s, json!({"Account No.": "10110", "Credit Amount": 300}));
    let post = s.run("journal.post", &json!({"template": "GENERAL"}));
    let msg = post.unwrap_err().message;
    assert!(msg.contains("You cannot use Gen. Journal Batch: GENERAL,DEFAULT for this action."), "{msg}");
    assert!(msg.contains("journal batch requires approval"));

    let r = s.run("journal.send_approval_request", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(r["approvers"], json!(["MANAGER"]));
    assert!(s.run("journal.send_approval_request", &json!({"template": "GENERAL"})).unwrap_err().message.contains("already exists"));
    // A comment on the batch.
    let c = s.run("journal.approval_comments", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(c["open"]["page"], "approval-comments");
    s.run("record.new", &json!({"table": "Approval Comment Line", "values": {"Record ID to Approve": "Gen. Journal Batch: GENERAL,DEFAULT", "Comment": "Rent for November"}})).unwrap();
    let cl = s.run("record.list", &json!({"table": "Approval Comment Line"})).unwrap();
    assert_eq!(cl["records"][0]["User ID"], "SALESREP");
    assert_eq!(cl["records"][0]["Table ID"], 232);

    let e = open_for(&mut s, "Gen. Journal Batch: GENERAL,DEFAULT");
    s.set_user("MANAGER");
    let r = s.run("approval.approve", &json!({"entry_no": e["Entry No."]})).unwrap();
    assert!(r["message"].as_str().unwrap().contains("has been approved"));
    s.set_user("SALESREP");
    s.run("journal.post", &json!({"template": "GENERAL"})).unwrap();
    // New lines restrict the batch again.
    jline(&mut s, json!({"Account No.": "60200", "Debit Amount": 100}));
    assert!(s.run("journal.post", &json!({"template": "GENERAL"})).is_err());
}

#[test]
fn journal_line_approval_cancel_and_reject() {
    let mut s = demo();
    s.run("record.validate", &json!({"table": "Workflow", "key": "MS-GJLAPW", "field": "Enabled", "value": true})).unwrap();
    let a = jline(&mut s, json!({"Account No.": "60200", "Debit Amount": 300, "Bal. Account No.": "10110"}));
    let key = a["key"].clone();
    let msg = s.run("journal.post", &json!({"template": "GENERAL"})).unwrap_err().message;
    assert!(msg.contains("Gen. Journal Line: GENERAL,DEFAULT,") && msg.contains("line requires approval"), "{msg}");
    assert!(s.run("journal.send_approval_request", &json!({"template": "GENERAL"})).unwrap_err().message.contains("No approval workflow"));
    s.run("journal.send_approval_request", &json!({"template": "GENERAL", "scope": "lines", "table": "Gen. Journal Line", "key": key})).unwrap();
    let r = s.run("journal.cancel_approval_request", &json!({"template": "GENERAL", "scope": "lines", "table": "Gen. Journal Line", "key": key})).unwrap();
    assert_eq!(r["message"], "The approval request for the selected record has been canceled.");
    s.run("journal.send_approval_request", &json!({"template": "GENERAL", "scope": "lines", "table": "Gen. Journal Line", "key": key})).unwrap();
    let rid = format!("Gen. Journal Line: GENERAL,DEFAULT,{}", key[2]);
    let e = open_for(&mut s, &rid);
    s.set_user("MANAGER");
    s.run("approval.reject", &json!({"entry_no": e["Entry No."]})).unwrap();
    s.set_user("SALESREP");
    assert!(s.run("journal.post", &json!({"template": "GENERAL"})).is_err(), "rejected stays restricted");
    let ap = s.run("journal.approvals", &json!({"template": "GENERAL"})).unwrap();
    assert_eq!(ap["open"]["page"], "approval-entries");
}
