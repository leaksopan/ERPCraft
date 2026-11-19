//! Sales and purchase document actions: create, add lines, release / reopen, make order,
//! statistics. Posting is in [`crate::posting`].

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Map, Value as Json, json};

use crate::calc::calc_fields;
use crate::ctx::{Ctx, dec, test_field};
use crate::error::Result;
use crate::posting::document_page;
use crate::records;
use crate::refuse;
use crate::side::Side;

pub fn header_key(doc_type: &str, no: &str) -> Key {
    Key(vec![Value::text(doc_type), Value::text(no)])
}

/// Creates a document for `party_no` (may be empty) and optional lines.
pub fn new_document(ctx: &mut Ctx, side: &Side, doc_type: &str, party_no: &str, values: &Map<String, Json>, lines: &[Json]) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    if !crate::schema::tables::SALES_DOC_TYPES.contains(&doc_type) {
        refuse!("'{doc_type}' is not a valid document type. Valid types: {}", crate::schema::tables::SALES_DOC_TYPES.join(", "));
    }
    let mut v = Map::new();
    v.insert("Document Type".into(), json!(doc_type));
    if !party_no.is_empty() {
        v.insert(side.party_no.into(), json!(party_no));
    }
    for (k, val) in values {
        v.insert(k.clone(), val.clone());
    }
    let header = records::create(ctx, ht, &v, None)?;
    let no = header.str("No.").to_string();
    for l in lines {
        add_line(ctx, side, doc_type, &no, l)?;
    }
    Ok(json!({
        "document_type": doc_type,
        "no": no,
        "open": {"page": document_page(side, doc_type), "key": [doc_type, no]},
        "message": format!("{} {doc_type} {no} created.", if side.sales { "Sales" } else { "Purchase" }),
    }))
}

/// Adds a line: `{"type": "Item", "no": "1000", "quantity": 2, "unit_price": 10, "line_discount": 5}`.
/// Field names of the line table are accepted too.
pub fn add_line(ctx: &mut Ctx, side: &Side, doc_type: &str, doc_no: &str, spec: &Json) -> Result<Record> {
    let lt = ctx.table(side.line)?;
    let Some(spec) = spec.as_object() else {
        refuse!("A line must be a JSON object like {{\"type\":\"Item\",\"no\":\"1000\",\"quantity\":1}}.");
    };
    ctx.get(side.header, &header_key(doc_type, doc_no))?;
    let mut r = lt.init_record();
    r.set("Document Type", Value::text(doc_type));
    r.set("Document No.", Value::text(doc_no));
    let mut r = records::insert(ctx, lt, r)?;
    let alias = |k: &str| -> String {
        match k {
            "type" => "Type".into(),
            "no" => "No.".into(),
            "quantity" | "qty" => "Quantity".into(),
            "unit_price" | "price" | "direct_unit_cost" => side.price.into(),
            "line_discount" | "discount" => "Line Discount %".into(),
            "description" => "Description".into(),
            other => other.to_string(),
        }
    };
    let mut fields: Map<String, Json> = spec.iter().map(|(k, v)| (alias(k), v.clone())).collect();
    if !fields.contains_key("Type") && fields.contains_key("No.") {
        fields.insert("Type".into(), json!("Item"));
    }
    // Validate in the order a user would type: Type, No., Quantity, price, discount, the rest.
    let order = ["Type", "No.", "Description", "Quantity", side.price, "Line Discount %"];
    for f in order {
        if let Some(v) = fields.remove(f) {
            records::validate(ctx, lt, &mut r, f, &v, true)?;
        }
    }
    for (f, v) in &fields {
        records::validate(ctx, lt, &mut r, f, v, true)?;
    }
    records::modify(ctx, lt, r)
}

pub fn release(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    let mut h = ctx.get(side.header, &header_key(doc_type, no))?;
    if h.str("Status") == "Released" {
        return Ok(json!({"message": format!("{doc_type} {no} is already released.")}));
    }
    test_field(ht, &h, side.party_no)?;
    let has_qty = ctx
        .db
        .rows(side.line)
        .any(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no && !l.str("Type").is_empty() && !l.get("Quantity").as_dec().is_zero());
    if !has_qty {
        refuse!("There is nothing to release for the document of type {doc_type} with the number {no}.");
    }
    h.set("Status", Value::text("Released"));
    ctx.db.modify(ht, h)?;
    Ok(json!({"status": "Released", "message": format!("{doc_type} {no} released.")}))
}

pub fn reopen(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    let mut h = ctx.get(side.header, &header_key(doc_type, no))?;
    h.set("Status", Value::text("Open"));
    ctx.db.modify(ht, h)?;
    Ok(json!({"status": "Open", "message": format!("{doc_type} {no} reopened.")}))
}

/// Converts a quote into an order (BC "Make Order"): a new order with the quote's header and
/// lines; the quote is deleted.
pub fn make_order(ctx: &mut Ctx, side: &Side, quote_no: &str) -> Result<Json> {
    let ht = ctx.table(side.header)?;
    let lt = ctx.table(side.line)?;
    let quote = ctx.get(side.header, &header_key("Quote", quote_no))?;
    test_field(ht, &quote, side.party_no)?;
    let party = ctx.get_code(side.party_table, quote.str(side.party_no))?;
    if party.str("Blocked") == "All" {
        refuse!("You cannot create an order when {} {} is blocked with type All.", side.party_caption, party.str("No."));
    }
    let mut v = Map::new();
    v.insert("Document Type".into(), json!("Order"));
    v.insert(side.party_no.into(), quote.get(side.party_no).to_json());
    let mut order = records::create(ctx, ht, &v, None)?;
    let order_no = order.str("No.").to_string();
    for f in ["Your Reference", "External Document No.", "Requested Delivery Date", "Work Description", side.person, "Payment Terms Code", "Currency Code", "Prices Including VAT"] {
        if ht.field(f).is_some() && !quote.get(f).is_blank() {
            order.set(f, quote.get(f).clone());
        }
    }
    order.set("Quote No.", Value::text(quote_no));
    ctx.db.modify(ht, order)?;
    let lines: Vec<Record> =
        ctx.db.rows(side.line).filter(|l| l.str("Document Type") == "Quote" && l.str("Document No.") == quote_no).cloned().collect();
    for mut l in lines {
        let old_key = l.key(lt);
        l.set("Document Type", Value::text("Order"));
        l.set("Document No.", Value::text(&order_no));
        let outstanding = l.get("Quantity").as_dec();
        l.set("Outstanding Quantity", dec(outstanding));
        l.set(side.qty_handle, dec(outstanding));
        l.set("Qty. to Invoice", dec(outstanding));
        ctx.db.insert(lt, l)?;
        ctx.db.remove(side.line, &old_key);
    }
    ctx.db.remove(side.header, &quote.key(ht));
    Ok(json!({
        "no": order_no,
        "open": {"page": document_page(side, "Order"), "key": ["Order", order_no]},
        "message": format!("Quote {quote_no} has been converted to order {order_no}."),
    }))
}

/// Document statistics (F7): amounts, VAT, quantities, profit for sales.
pub fn statistics(ctx: &mut Ctx, side: &Side, doc_type: &str, no: &str) -> Result<Json> {
    let mut h = ctx.get(side.header, &header_key(doc_type, no))?;
    calc_fields(ctx.db, ctx.work_date, side.header, &mut h);
    let lines: Vec<Record> =
        ctx.db.rows(side.line).filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no).cloned().collect();
    let qty: Decimal = lines.iter().map(|l| l.get("Quantity").as_dec()).sum();
    let cost: Decimal = lines.iter().map(|l| l.get("Quantity").as_dec() * l.get("Unit Cost (LCY)").as_dec()).sum();
    let amount = h.get("Amount").as_dec();
    let mut rows = vec![
        json!(["Amount", amount.to_string()]),
        json!(["VAT Amount", h.get("VAT Amount").as_dec().to_string()]),
        json!(["Amount Including VAT", h.get("Amount Including VAT").as_dec().to_string()]),
        json!(["Quantity", qty.to_string()]),
        json!(["No. of Lines", lines.len()]),
    ];
    if side.sales {
        let profit = amount - crate::ctx::round_amount(cost);
        let pct = if amount.is_zero() { Decimal::ZERO } else { crate::ctx::round_amount(profit * Decimal::ONE_HUNDRED / amount) };
        rows.push(json!(["Cost (LCY)", crate::ctx::round_amount(cost).to_string()]));
        rows.push(json!(["Profit (LCY)", profit.to_string()]));
        rows.push(json!(["Profit %", pct.to_string()]));
    }
    Ok(json!({"report": {"title": format!("{} {doc_type} Statistics - {no}", if side.sales { "Sales" } else { "Purchase" }), "columns": ["", "Value"], "rows": rows}}))
}
