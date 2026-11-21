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
use crate::schema::approvals::{APPROVAL_COMMENT, APPROVAL_ENTRY, RESTRICTED_RECORD, USER_SETUP, WORKFLOW, WORKFLOWS};
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
    if !e.str("Record ID to Approve").is_empty() {
        return decide_record(ctx, e, approve);
    }
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

// ---- Journals (BC's General / Item Journal Batch and General Journal Line approval workflows) ----

const GJ_BATCH_WF: &str = "MS-GJBAPW";
const GJ_LINE_WF: &str = "MS-GJLAPW";
const IJ_BATCH_WF: &str = "MS-IJBAPW";
const BATCH_RESTRICTION: &str = "The restriction was imposed because the journal batch requires approval.";
const LINE_RESTRICTION: &str = "The restriction was imposed because the line requires approval.";

fn enabled(ctx: &Ctx, code: &str) -> bool {
    ctx.db.get_code(WORKFLOW, code).is_some_and(|w| w.get("Enabled").as_bool())
}

/// The journal batch a command runs on.
struct Jnl {
    line_table: &'static str,
    batch_table: &'static str,
    template: String,
    batch: String,
}

fn jnl_tables(item: bool) -> (&'static str, &'static str) {
    if item { ("Item Journal Line", "Item Journal Batch") } else { ("Gen. Journal Line", "Gen. Journal Batch") }
}

fn jnl(p: &Json, item: bool) -> Jnl {
    let up = |k: &str, d: &str| opt_str(p, k).unwrap_or(d).to_uppercase();
    let (line_table, batch_table) = jnl_tables(item);
    Jnl { line_table, batch_table, template: up("template", if item { "ITEM" } else { "GENERAL" }), batch: up("batch", "DEFAULT") }
}

/// BC's Record ID text: `Gen. Journal Batch: GENERAL,DEFAULT`.
fn batch_record_id(j: &Jnl) -> String {
    format!("{}: {},{}", j.batch_table, j.template, j.batch)
}

fn line_record_id(table: &str, l: &Record) -> String {
    format!("{table}: {},{},{}", l.str("Journal Template Name"), l.str("Journal Batch Name"), l.get("Line No.").as_int())
}

fn record_entries(ctx: &Ctx, record_id: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(APPROVAL_ENTRY).filter(|e| e.str("Record ID to Approve") == record_id).cloned().collect();
    v.sort_by_key(|e| e.get("Entry No.").as_int());
    v
}

fn has_open(ctx: &Ctx, record_id: &str) -> bool {
    ctx.db.rows(APPROVAL_ENTRY).any(|e| e.str("Record ID to Approve") == record_id && matches!(e.str("Status"), "Open" | "Created"))
}

/// BC's RecordRestrictionMgt.RestrictRecordUsage.
fn restrict(ctx: &mut Ctx, record_id: &str, details: &str) -> Result<()> {
    if ctx.db.rows(RESTRICTED_RECORD).any(|r| r.str("Record ID") == record_id) {
        return Ok(());
    }
    let t = ctx.table(RESTRICTED_RECORD)?;
    let mut r = t.init_record();
    r.set("ID", Value::Int(ctx.db.rows(RESTRICTED_RECORD).map(|r| r.get("ID").as_int()).max().unwrap_or(0) + 1));
    r.set("Record ID", Value::text(record_id));
    r.set("Details", Value::text(details));
    ctx.db.insert(t, r).map(|_| ())
}

/// BC's AllowRecordUsage: the approval lifts the restriction.
fn allow(ctx: &mut Ctx, record_id: &str) -> Result<()> {
    let t = ctx.table(RESTRICTED_RECORD)?;
    let keys: Vec<Key> = ctx.db.rows(RESTRICTED_RECORD).filter(|r| r.str("Record ID") == record_id).map(|r| r.key(t)).collect();
    for k in keys {
        ctx.db.remove(&t.name, &k);
    }
    Ok(())
}

/// BC's CheckRecordHasUsageRestrictions: "You cannot use … for this action."
fn check_restriction(ctx: &Ctx, record_id: &str) -> Result<()> {
    if let Some(r) = ctx.db.rows(RESTRICTED_RECORD).find(|r| r.str("Record ID") == record_id) {
        refuse!("You cannot use {record_id} for this action.\n{}", r.str("Details"));
    }
    Ok(())
}

/// A new journal line in a batch under an enabled approval workflow restricts the batch (and,
/// under the line workflow, the line) until it is approved.
pub fn on_journal_line_insert(ctx: &mut Ctx, line_table: &str, l: &Record) -> Result<()> {
    let item = line_table == "Item Journal Line";
    let (_, batch_table) = jnl_tables(item);
    if enabled(ctx, if item { IJ_BATCH_WF } else { GJ_BATCH_WF }) {
        let rid = format!("{batch_table}: {},{}", l.str("Journal Template Name"), l.str("Journal Batch Name"));
        restrict(ctx, &rid, BATCH_RESTRICTION)?;
    }
    if !item && enabled(ctx, GJ_LINE_WF) {
        restrict(ctx, &line_record_id(line_table, l), LINE_RESTRICTION)?;
    }
    Ok(())
}

/// Posting a journal batch checks the restrictions on it and on its lines (BC's
/// OnCheckGenJournalLinePostRestrictions).
pub fn check_journal_post(ctx: &Ctx, line_table: &str, template: &str, batch: &str) -> Result<()> {
    let (_, batch_table) = jnl_tables(line_table == "Item Journal Line");
    check_restriction(ctx, &format!("{batch_table}: {template},{batch}"))?;
    for l in ctx.db.rows(line_table).filter(|l| l.str("Journal Template Name") == template && l.str("Journal Batch Name") == batch) {
        check_restriction(ctx, &line_record_id(line_table, l))?;
    }
    Ok(())
}

fn jnl_lines(ctx: &Ctx, j: &Jnl) -> Vec<Record> {
    let mut v: Vec<Record> =
        ctx.db.rows(j.line_table).filter(|l| l.str("Journal Template Name") == j.template && l.str("Journal Batch Name") == j.batch).cloned().collect();
    v.sort_by_key(|l| l.get("Line No.").as_int());
    v
}

/// The selected line (the action's `key`), for "Selected Journal Lines".
fn selected_lines(ctx: &Ctx, j: &Jnl, p: &Json) -> Result<Vec<Record>> {
    let t = ctx.table(j.line_table)?;
    match p.get("key") {
        Some(k) if opt_str(p, "table").is_none_or(|x| x == j.line_table) => {
            let key = Key::from_json(t, k).map_err(EngineError::invalid)?;
            Ok(vec![ctx.get(j.line_table, &key)?])
        }
        _ => refuse!("Select the journal lines to send for approval."),
    }
}

/// An approval entry for `record_id` to the sender's approver (journal approvals use the direct
/// approver only, as BC's journal templates do).
fn request(ctx: &mut Ctx, record_id: &str, table_id: u32, code: &str, doc_no: &str, amount: Decimal) -> Result<String> {
    let sender = ctx.user.to_uppercase();
    let Some(setup) = user_setup(ctx, &sender) else {
        refuse!("User ID {sender} does not exist in the Approval User Setup window.");
    };
    let approver = setup.str("Approver ID").to_uppercase();
    if approver.is_empty() {
        refuse!("Approver ID must have a value in User Setup: User ID={sender}. It cannot be zero or empty.");
    }
    let at = ctx.table(APPROVAL_ENTRY)?;
    let sent = now();
    let mut e = at.init_record();
    e.set("Entry No.", Value::Int(ctx.db.next_entry_no(APPROVAL_ENTRY)));
    e.set("Table ID", Value::Int(i64::from(table_id)));
    e.set("Document Type", Value::text(" "));
    e.set("Document No.", Value::text(doc_no));
    e.set("Sequence No.", Value::Int(1));
    e.set("Approval Code", Value::text(code));
    e.set("Sender ID", Value::text(&sender));
    e.set("Approver ID", Value::text(&approver));
    e.set("Status", Value::text("Open"));
    e.set("Date-Time Sent for Approval", Value::text(&sent));
    e.set("Last Date-Time Modified", Value::text(&sent));
    e.set("Last Modified By User ID", Value::text(&sender));
    e.set("Due Date", Value::Date(ctx.work_date));
    e.set("Amount", Value::Dec(amount));
    e.set("Amount (LCY)", Value::Dec(amount));
    e.set("Record ID to Approve", Value::text(record_id));
    ctx.db.insert(at, e)?;
    Ok(approver)
}

fn journal_send(ctx: &mut Ctx, p: &Json, item: bool) -> Result<Json> {
    let j = jnl(p, item);
    if !item && opt_str(p, "scope") == Some("lines") {
        if !enabled(ctx, GJ_LINE_WF) {
            refuse!("No approval workflow for this record type is enabled.");
        }
        let table_id = ctx.table(j.line_table)?.object_id;
        let mut to = Vec::new();
        for l in selected_lines(ctx, &j, p)? {
            let rid = line_record_id(j.line_table, &l);
            if !has_open(ctx, &rid) {
                to.push(request(ctx, &rid, table_id, GJ_LINE_WF, l.str("Document No."), l.get("Amount").as_dec())?);
                restrict(ctx, &rid, LINE_RESTRICTION)?;
            }
        }
        if to.is_empty() {
            refuse!("An approval request already exists.");
        }
        return Ok(json!({"approvers": to, "message": "An approval request has been sent."}));
    }
    let code = if item { IJ_BATCH_WF } else { GJ_BATCH_WF };
    if !enabled(ctx, code) {
        refuse!("No approval workflow for this record type is enabled.");
    }
    ctx.get(j.batch_table, &Key(vec![Value::text(&j.template), Value::text(&j.batch)]))?;
    let rid = batch_record_id(&j);
    let lines = jnl_lines(ctx, &j);
    if has_open(ctx, &rid) || lines.iter().any(|l| has_open(ctx, &line_record_id(j.line_table, l))) {
        refuse!("An approval request already exists.");
    }
    if lines.is_empty() {
        refuse!("There is nothing to approve.");
    }
    let amount: Decimal = if item { Decimal::ZERO } else { lines.iter().map(|l| l.get("Amount").as_dec()).filter(|a| a.is_sign_positive()).sum() };
    let table_id = ctx.table(j.batch_table)?.object_id;
    let to = request(ctx, &rid, table_id, code, "", amount)?;
    restrict(ctx, &rid, BATCH_RESTRICTION)?;
    Ok(json!({"approvers": [to], "message": "An approval request has been sent."}))
}

fn journal_cancel(ctx: &mut Ctx, p: &Json, item: bool) -> Result<Json> {
    let j = jnl(p, item);
    let lines_scope = !item && opt_str(p, "scope") == Some("lines");
    let ids: Vec<String> =
        if lines_scope { selected_lines(ctx, &j, p)?.iter().map(|l| line_record_id(j.line_table, l)).collect() } else { vec![batch_record_id(&j)] };
    let at = ctx.table(APPROVAL_ENTRY)?;
    let mut n = 0;
    for rid in &ids {
        for mut e in record_entries(ctx, rid).into_iter().filter(|e| matches!(e.str("Status"), "Open" | "Created")) {
            e.set("Status", Value::text("Canceled"));
            e.set("Last Date-Time Modified", Value::text(now()));
            e.set("Last Modified By User ID", Value::text(ctx.user.to_uppercase()));
            ctx.db.modify(at, e)?;
            n += 1;
        }
    }
    if n == 0 {
        refuse!("There is no approval request to cancel.");
    }
    let msg = if lines_scope { "The approval request for the selected record has been canceled." } else { "The approval request for the record has been canceled." };
    Ok(json!({"message": msg}))
}

/// Approve / reject an approval entry for a record (a journal batch or line). The last approval
/// lifts the record's restriction.
fn decide_record(ctx: &mut Ctx, mut e: Record, approve: bool) -> Result<Json> {
    let at = ctx.table(APPROVAL_ENTRY)?;
    let rid = e.str("Record ID to Approve").to_string();
    e.set("Status", Value::text(if approve { "Approved" } else { "Rejected" }));
    e.set("Last Date-Time Modified", Value::text(now()));
    e.set("Last Modified By User ID", Value::text(ctx.user.to_uppercase()));
    ctx.db.modify(at, e)?;
    let rest: Vec<Record> = record_entries(ctx, &rid).into_iter().filter(|x| x.str("Status") == "Created").collect();
    if !approve {
        for mut x in rest {
            x.set("Status", Value::text("Rejected"));
            ctx.db.modify(at, x)?;
        }
        return Ok(json!({"message": format!("The approval request for {rid} was rejected.")}));
    }
    if let Some(mut next) = rest.into_iter().next() {
        next.set("Status", Value::text("Open"));
        let to = next.str("Approver ID").to_string();
        ctx.db.modify(at, next)?;
        return Ok(json!({"message": format!("Approved. The request was sent on to {to}.")}));
    }
    allow(ctx, &rid)?;
    Ok(json!({"message": format!("{rid} has been approved.")}))
}

/// The approval entries of the batch and its lines.
fn journal_approvals(ctx: &mut Ctx, p: &Json, item: bool) -> Result<Json> {
    let j = jnl(p, item);
    let mut ids: Vec<String> = vec![batch_record_id(&j)];
    ids.extend(jnl_lines(ctx, &j).iter().map(|l| line_record_id(j.line_table, l)));
    let nos: Vec<String> = ctx
        .db
        .rows(APPROVAL_ENTRY)
        .filter(|e| ids.iter().any(|i| i == e.str("Record ID to Approve")))
        .map(|e| e.get("Entry No.").as_int().to_string())
        .collect();
    let filter = if nos.is_empty() { "0".to_string() } else { nos.join("|") };
    Ok(json!({"open": {"page": "approval-entries", "filters": [["Entry No.", filter]]}}))
}

/// BC's Comments on a journal: the batch's approval comments.
fn journal_comments(ctx: &mut Ctx, p: &Json, item: bool) -> Result<Json> {
    let j = jnl(p, item);
    ctx.get(j.batch_table, &Key(vec![Value::text(&j.template), Value::text(&j.batch)]))?;
    Ok(json!({"open": {"page": "approval-comments", "filters": [["Record ID to Approve", batch_record_id(&j)]]}}))
}

/// A new approval comment: numbered, stamped with the user and time, tied to its record's table.
pub fn on_comment_insert(ctx: &mut Ctx, r: &mut Record) {
    if r.get("Entry No.").as_int() == 0 {
        r.set("Entry No.", Value::Int(ctx.db.rows(APPROVAL_COMMENT).map(|c| c.get("Entry No.").as_int()).max().unwrap_or(0) + 1));
    }
    r.set("User ID", Value::text(ctx.user.to_uppercase()));
    r.set("Date and Time", Value::text(now()));
    comment_record_table(ctx, r);
}

/// An approval comment's Table ID: the table its Record ID to Approve names.
pub fn comment_record_table(ctx: &Ctx, r: &mut Record) {
    let table = r.str("Record ID to Approve").split(':').next().unwrap_or_default().to_string();
    if let Ok(t) = ctx.table(&table) {
        r.set("Table ID", Value::Int(i64::from(t.object_id)));
    }
}

macro_rules! per_journal {
    ($gen:ident, $item:ident, $f:ident) => {
        fn $gen(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, p, false)
        }
        fn $item(c: &mut Ctx, p: &Json) -> Result<Json> {
            $f(c, p, true)
        }
    };
}
per_journal!(gj_send, ij_send, journal_send);
per_journal!(gj_cancel, ij_cancel, journal_cancel);
per_journal!(gj_approvals, ij_approvals, journal_approvals);
per_journal!(gj_comments, ij_comments, journal_comments);

fn s_journal() -> Json {
    json!({"type": "object", "properties": {
        "template": {"type": "string"}, "batch": {"type": "string", "default": "DEFAULT"},
        "scope": {"type": "string", "enum": ["batch", "lines"], "default": "batch"},
        "table": {"type": "string"}, "key": {"description": "the selected journal line, for scope = lines"}}})
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
    cmd!("journal.send_approval_request", "Send Approval Request", "Finance", s_journal, gj_send, "Request approval of a general journal batch, or of the selected line (scope = lines)."),
    cmd!("journal.cancel_approval_request", "Cancel Approval Request", "Finance", s_journal, gj_cancel, "Cancel the approval request of a general journal batch or of the selected line."),
    cmd!("item_journal.send_approval_request", "Send Approval Request", "Inventory", s_journal, ij_send, "Request approval of an item journal batch."),
    cmd!("item_journal.cancel_approval_request", "Cancel Approval Request", "Inventory", s_journal, ij_cancel, "Cancel the approval request of an item journal batch."),
    CommandDef { id: "journal.approvals", label: "Approvals", group: "Finance", summary: "The approval entries of a general journal batch and its lines.", mutates: false, params: s_journal, run: gj_approvals },
    CommandDef { id: "item_journal.approvals", label: "Approvals", group: "Inventory", summary: "The approval entries of an item journal batch.", mutates: false, params: s_journal, run: ij_approvals },
    CommandDef { id: "journal.approval_comments", label: "Comments", group: "Finance", summary: "The approval comments of a general journal batch.", mutates: false, params: s_journal, run: gj_comments },
    CommandDef { id: "item_journal.approval_comments", label: "Comments", group: "Inventory", summary: "The approval comments of an item journal batch.", mutates: false, params: s_journal, run: ij_comments },
    cmd!("approval.approve", "Approve", "Approvals", s_entry, approve, "Approve an approval request sent to you; the last approval releases the document."),
    cmd!("approval.reject", "Reject", "Approvals", s_entry, reject, "Reject an approval request sent to you; the document reopens."),
    cmd!("approval.delegate", "Delegate", "Approvals", s_entry, delegate, "Pass an approval request sent to you to your substitute."),
];
