//! BC's document approvals (Approvals Mgmt.): an enabled approval workflow makes a sales or
//! purchase document go through Send Approval Request, the approver chain from User Setup
//! (each approver's approver until one's limit covers the amount), and Approve / Reject /
//! Delegate on Requests to Approve. The last approval releases the document; a rejection or a
//! cancellation reopens it.

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, doc_ref, opt_str};
use crate::ctx::Ctx;
use crate::documents::{self, header_key};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::approvals::{APPROVAL_ENTRY, USER_SETUP, WORKFLOW, WORKFLOWS};
use crate::side::{PURCHASE, SALES, Side};

/// The enabled approval workflow for a document, if any.
pub fn workflow(ctx: &Ctx, side: &Side, doc_type: &str) -> Option<String> {
    let (_, _, code, _) = WORKFLOWS.iter().find(|(sales, t, _, _)| *sales == side.sales && *t == doc_type)?;
    ctx.db.get_code(WORKFLOW, code).filter(|w| w.get("Enabled").as_bool()).map(|_| (*code).to_string())
}

/// "Sales Order" / "Purchase Invoice".
fn doc_caption(side: &Side, doc_type: &str) -> String {
    format!("{} {doc_type}", if side.sales { "Sales" } else { "Purchase" })
}

/// BC's PrePostApprovalCheck: a document under an enabled workflow must be released first.
pub fn check_post(ctx: &Ctx, side: &Side, doc_type: &str, no: &str, status: &str) -> Result<()> {
    if workflow(ctx, side, doc_type).is_some() && status != "Released" {
        refuse!("{} {no} must be approved and released before you can perform this action.", doc_caption(side, doc_type));
    }
    Ok(())
}

/// BC refuses a manual Release while a workflow governs the document.
pub fn check_release(ctx: &Ctx, side: &Side, doc_type: &str) -> Result<()> {
    if workflow(ctx, side, doc_type).is_some() {
        refuse!("This document can only be released when the approval process is complete.");
    }
    Ok(())
}

fn now() -> String {
    chrono::Local::now().format("%d/%m/%Y %H:%M").to_string()
}

fn user_setup(ctx: &Ctx, user: &str) -> Option<Record> {
    ctx.db.get_code(USER_SETUP, user).cloned()
}

/// The approvers in order: the sender's approver, then each approver's approver while the amount
/// is over their limit (BC's "Approver" chain with "Approval Limits").
fn approver_chain(ctx: &Ctx, side: &Side, sender: &str, amount: Decimal) -> Result<Vec<String>> {
    let Some(setup) = user_setup(ctx, sender) else {
        refuse!("User ID {sender} does not exist in the Approval User Setup window.");
    };
    let mut chain = Vec::new();
    let mut next = setup.str("Approver ID").to_uppercase();
    if next.is_empty() {
        refuse!("Approver ID must have a value in User Setup: User ID={sender}. It cannot be zero or empty.");
    }
    let (limit_field, unlimited_field) =
        if side.sales { ("Sales Amount Approval Limit", "Unlimited Sales Approval") } else { ("Purchase Amount Approval Limit", "Unlimited Purchase Approval") };
    while chain.len() < 20 {
        if chain.contains(&next) {
            refuse!("The approver chain of {sender} loops at {next}. Check the Approver ID values in User Setup.");
        }
        chain.push(next.clone());
        let Some(a) = user_setup(ctx, &next) else { break };
        let covered = a.get(unlimited_field).as_bool() || Decimal::from(a.get(limit_field).as_int()) >= amount;
        let higher = a.str("Approver ID").to_uppercase();
        if covered || higher.is_empty() {
            break;
        }
        next = higher;
    }
    Ok(chain)
}

fn entries_for(ctx: &Ctx, table_id: i64, doc_type: &str, no: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx
        .db
        .rows(APPROVAL_ENTRY)
        .filter(|e| e.get("Table ID").as_int() == table_id && e.str("Document Type") == doc_type && e.str("Document No.") == no)
        .cloned()
        .collect();
    v.sort_by_key(|e| e.get("Sequence No.").as_int());
    v
}

fn set_status(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str, status: &str) -> Result<()> {
    let ht = ctx.table(side.header)?;
    let mut h = ctx.get(side.header, &header_key(doc_type, no))?;
    h.set("Status", Value::text(status));
    ctx.db.modify(ht, h)
}

fn send(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let Some(code) = workflow(ctx, side, &doc_type) else {
        refuse!("No approval workflow for this record type is enabled.");
    };
    let ht = ctx.table(side.header)?;
    let mut h = ctx.get(side.header, &header_key(&doc_type, &no))?;
    if h.str("Status") != "Open" {
        refuse!("Status must be equal to 'Open' in {}: Document Type={doc_type}, No.={no}. Current value is '{}'.", ht.caption, h.str("Status"));
    }
    crate::calc::calc_fields(ctx.db, ctx.work_date, side.header, &mut h);
    let amount = h.get("Amount Including VAT").as_dec();
    let sender = ctx.user.to_uppercase();
    let chain = approver_chain(ctx, side, &sender, amount)?;
    let at = ctx.table(APPROVAL_ENTRY)?;
    let sent = now();
    for (i, approver) in chain.iter().enumerate() {
        let mut e = at.init_record();
        e.set("Entry No.", Value::Int(ctx.db.next_entry_no(APPROVAL_ENTRY)));
        e.set("Table ID", Value::Int(i64::from(ht.object_id)));
        e.set("Document Type", Value::text(&doc_type));
        e.set("Document No.", Value::text(&no));
        e.set("Sequence No.", Value::Int(i as i64 + 1));
        e.set("Approval Code", Value::text(&code));
        e.set("Sender ID", Value::text(&sender));
        e.set("Approver ID", Value::text(approver));
        e.set("Status", Value::text(if i == 0 { "Open" } else { "Created" }));
        e.set("Date-Time Sent for Approval", Value::text(&sent));
        e.set("Last Date-Time Modified", Value::text(&sent));
        e.set("Last Modified By User ID", Value::text(&sender));
        e.set("Due Date", h.get("Due Date").clone());
        e.set("Amount", Value::Dec(amount));
        e.set("Amount (LCY)", Value::Dec(amount));
        e.set("Currency Code", h.get("Currency Code").clone());
        ctx.db.insert(at, e)?;
    }
    set_status(ctx, side, &doc_type, &no, "Pending Approval")?;
    Ok(json!({"status": "Pending Approval", "approvers": chain, "message": "An approval request has been sent."}))
}

fn cancel(ctx: &mut Ctx, side: &Side, p: &Json) -> Result<Json> {
    let (doc_type, no) = doc_ref(p, side)?;
    let ht = ctx.table(side.header)?;
    let at = ctx.table(APPROVAL_ENTRY)?;
    let pending: Vec<Record> =
        entries_for(ctx, i64::from(ht.object_id), &doc_type, &no).into_iter().filter(|e| matches!(e.str("Status"), "Open" | "Created")).collect();
    if pending.is_empty() {
        refuse!("There is no approval request to cancel.");
    }
    for mut e in pending {
        e.set("Status", Value::text("Canceled"));
        e.set("Last Date-Time Modified", Value::text(now()));
        e.set("Last Modified By User ID", Value::text(ctx.user.to_uppercase()));
        ctx.db.modify(at, e)?;
    }
    set_status(ctx, side, &doc_type, &no, "Open")?;
    Ok(json!({"status": "Open", "message": "The approval request for the record has been canceled."}))
}

/// The approval entry to act on: `entry_no` or the selected Approval Entry; only its approver (or
/// an approval administrator) may act on it.
fn entry_to_act_on(ctx: &Ctx, p: &Json) -> Result<Record> {
    let no = match (opt_str(p, "table"), p.get("key")) {
        (Some(APPROVAL_ENTRY), Some(k)) => k.as_i64().or_else(|| k.as_array().and_then(|a| a.first()).and_then(Json::as_i64)),
        _ => p.get("entry_no").and_then(Json::as_i64),
    }
    .ok_or_else(|| EngineError::invalid("missing parameter 'entry_no'"))?;
    let e = ctx.get(APPROVAL_ENTRY, &Key(vec![Value::Int(no)]))?;
    if e.str("Status") != "Open" {
        refuse!("Status must be equal to 'Open' in Approval Entry: Entry No.={no}. Current value is '{}'.", e.str("Status"));
    }
    let user = ctx.user.to_uppercase();
    let admin = user_setup(ctx, &user).is_some_and(|u| u.get("Approval Administrator").as_bool());
    if e.str("Approver ID") != user && !admin {
        refuse!("You can only approve, reject or delegate requests that are sent to you. Entry No. {no} is for {}.", e.str("Approver ID"));
    }
    Ok(e)
}

fn side_of(ctx: &Ctx, e: &Record) -> Result<&'static Side> {
    let id = e.get("Table ID").as_int();
    for side in [&SALES, &PURCHASE] {
        if ctx.table(side.header).is_ok_and(|t| i64::from(t.object_id) == id) {
            return Ok(side);
        }
    }
    refuse!("Approval Entry {} is not for a sales or purchase document.", e.get("Entry No.").as_int())
}

fn decide(ctx: &mut Ctx, p: &Json, approve: bool) -> Result<Json> {
    let mut e = entry_to_act_on(ctx, p)?;
    let side = side_of(ctx, &e)?;
    let at = ctx.table(APPROVAL_ENTRY)?;
    let (doc_type, no) = (e.str("Document Type").to_string(), e.str("Document No.").to_string());
    e.set("Status", Value::text(if approve { "Approved" } else { "Rejected" }));
    e.set("Last Date-Time Modified", Value::text(now()));
    e.set("Last Modified By User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.modify(at, e.clone())?;
    let rest: Vec<Record> = entries_for(ctx, e.get("Table ID").as_int(), &doc_type, &no).into_iter().filter(|x| x.str("Status") == "Created").collect();
    if !approve {
        for mut x in rest {
            x.set("Status", Value::text("Rejected"));
            ctx.db.modify(at, x)?;
        }
        set_status(ctx, side, &doc_type, &no, "Open")?;
        return Ok(json!({"status": "Open", "message": format!("{} {no} was rejected.", doc_caption(side, &doc_type))}));
    }
    if let Some(mut next) = rest.into_iter().next() {
        next.set("Status", Value::text("Open"));
        let to = next.str("Approver ID").to_string();
        ctx.db.modify(at, next)?;
        return Ok(json!({"status": "Pending Approval", "message": format!("Approved. The request was sent on to {to}.")}));
    }
    // The last approval releases the document.
    set_status(ctx, side, &doc_type, &no, "Open")?;
    documents::release_approved(ctx, side, &doc_type, &no)?;
    Ok(json!({"status": "Released", "message": format!("{} {no} is approved and released.", doc_caption(side, &doc_type))}))
}

fn approve(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    decide(ctx, p, true)
}

fn reject(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    decide(ctx, p, false)
}

/// BC's Delegate: the request goes to the approver's substitute.
fn delegate(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let mut e = entry_to_act_on(ctx, p)?;
    let approver = e.str("Approver ID").to_string();
    let substitute = user_setup(ctx, &approver).map(|u| u.str("Substitute").to_uppercase()).unwrap_or_default();
    if substitute.is_empty() {
        refuse!("Substitute must have a value in User Setup: User ID={approver}. It cannot be zero or empty.");
    }
    let at = ctx.table(APPROVAL_ENTRY)?;
    e.set("Approver ID", Value::text(&substitute));
    e.set("Last Date-Time Modified", Value::text(now()));
    e.set("Last Modified By User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.modify(at, e)?;
    Ok(json!({"message": format!("The request was delegated to {substitute}.")}))
}

fn s_doc() -> Json {
    json!({"type": "object", "properties": {"document_type": {"type": "string", "default": "Order"}, "no": {"type": "string"}}, "required": ["no"]})
}

fn s_entry() -> Json {
    json!({"type": "object", "properties": {"entry_no": {"type": "integer", "description": "the approval entry (or the selected one)"}}, "required": ["entry_no"]})
}

macro_rules! per_side {
    ($sales:ident, $purch:ident, $f:ident) => {
        fn $sales(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, &SALES, p)
        }
        fn $purch(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, &PURCHASE, p)
        }
    };
}
per_side!(sales_send, purch_send, send);
per_side!(sales_cancel, purch_cancel, cancel);

macro_rules! cmd {
    ($id:literal, $label:literal, $group:literal, $params:expr, $run:expr, $summary:literal) => {
        CommandDef { id: $id, label: $label, group: $group, summary: $summary, mutates: true, params: $params, run: $run }
    };
}

pub static COMMANDS: &[CommandDef] = &[
    cmd!("sales.document.send_approval_request", "Send Approval Request", "Sales", s_doc, sales_send, "Request approval of a sales document (needs an enabled approval workflow)."),
    cmd!("sales.document.cancel_approval_request", "Cancel Approval Request", "Sales", s_doc, sales_cancel, "Cancel a sales document's approval request."),
    cmd!("purchase.document.send_approval_request", "Send Approval Request", "Purchasing", s_doc, purch_send, "Request approval of a purchase document (needs an enabled approval workflow)."),
    cmd!("purchase.document.cancel_approval_request", "Cancel Approval Request", "Purchasing", s_doc, purch_cancel, "Cancel a purchase document's approval request."),
    cmd!("approval.approve", "Approve", "Approvals", s_entry, approve, "Approve an approval request sent to you; the last approval releases the document."),
    cmd!("approval.reject", "Reject", "Approvals", s_entry, reject, "Reject an approval request sent to you; the document reopens."),
    cmd!("approval.delegate", "Delegate", "Approvals", s_entry, delegate, "Pass an approval request sent to you to your substitute."),
];
