//! Item setup triggers: the base unit of measure always has an item unit of 1 (BC's Item
//! "Base Unit of Measure" OnValidate), and attributes, attribute values and extended texts are
//! numbered as BC numbers them.

use erpcraft_meta::{Key, Record, TableDef, Value};
use rust_decimal::Decimal;

use crate::ctx::{Ctx, dec};
use crate::error::Result;
use crate::refuse;
use crate::schema::item_setup::{EXT_TEXT_HEADER, EXT_TEXT_LINE, ITEM_ATTRIBUTE, ITEM_ATTRIBUTE_VALUE, ITEM_UOM};

fn max_of(ctx: &Ctx, table: &str, field: &str, same: impl Fn(&Record) -> bool) -> i64 {
    ctx.db.rows(table).filter(|r| same(r)).map(|r| r.get(field).as_int()).max().unwrap_or(0)
}

/// Insert triggers; `Some(())` when the table is one of these.
pub fn on_insert(ctx: &mut Ctx, t: &TableDef, r: &mut Record) -> Result<Option<()>> {
    match t.name.as_str() {
        ITEM_UOM => {
            if r.get("Qty. per Unit of Measure").as_dec().is_zero() {
                r.set("Qty. per Unit of Measure", dec(Decimal::ONE));
            }
        }
        ITEM_ATTRIBUTE if r.get("ID").as_int() == 0 => {
            r.set("ID", Value::Int(max_of(ctx, ITEM_ATTRIBUTE, "ID", |_| true) + 1));
        }
        ITEM_ATTRIBUTE_VALUE if r.get("ID").as_int() == 0 => {
            let a = r.get("Attribute ID").as_int();
            r.set("ID", Value::Int(max_of(ctx, ITEM_ATTRIBUTE_VALUE, "ID", |v| v.get("Attribute ID").as_int() == a) + 1));
        }
        EXT_TEXT_HEADER if r.get("Text No.").as_int() == 0 => {
            let (tn, no, lang) = (r.str("Table Name").to_string(), r.str("No.").to_string(), r.str("Language Code").to_string());
            r.set("Text No.", Value::Int(max_of(ctx, EXT_TEXT_HEADER, "Text No.", |h| h.str("Table Name") == tn && h.str("No.") == no && h.str("Language Code") == lang) + 1));
        }
        EXT_TEXT_LINE if r.get("Line No.").as_int() == 0 => {
            let (tn, no, lang, text) = (r.str("Table Name").to_string(), r.str("No.").to_string(), r.str("Language Code").to_string(), r.get("Text No.").as_int());
            let last = max_of(ctx, EXT_TEXT_LINE, "Line No.", |l| l.str("Table Name") == tn && l.str("No.") == no && l.str("Language Code") == lang && l.get("Text No.").as_int() == text);
            r.set("Line No.", Value::Int(last + 10000));
        }
        _ => return Ok(None),
    }
    Ok(Some(()))
}

pub fn on_validate(ctx: &mut Ctx, t: &TableDef, r: &mut Record, field: &str) -> Result<()> {
    match (t.name.as_str(), field) {
        (crate::schema::tables::ITEM, "Base Unit of Measure") => {
            let code = r.str("Base Unit of Measure").to_string();
            let no = r.str("No.").to_string();
            if code.is_empty() || no.is_empty() {
                return Ok(());
            }
            let ut = ctx.table(ITEM_UOM)?;
            let key = Key(vec![Value::text(&no), Value::text(&code)]);
            match ctx.db.get(ITEM_UOM, &key).cloned() {
                Some(u) if u.get("Qty. per Unit of Measure").as_dec() != Decimal::ONE => {
                    refuse!("The quantity per base unit of measure must be 1. {code} is set up with {} per unit of measure.", u.get("Qty. per Unit of Measure").as_dec());
                }
                Some(_) => {}
                None => {
                    let mut u = ut.init_record();
                    u.set("Item No.", Value::text(&no));
                    u.set("Code", Value::text(&code));
                    u.set("Qty. per Unit of Measure", dec(Decimal::ONE));
                    ctx.db.insert(ut, u)?;
                }
            }
        }
        (crate::schema::tables::SALES_LINE | crate::schema::tables::PURCH_LINE | crate::schema::inventory::ITEM_JNL_LINE | crate::schema::transfer::TRANSFER_LINE, "No." | "Item No.") => {
            r.set("Variant Code", Value::text(""));
        }
        (crate::schema::tables::SALES_LINE | crate::schema::tables::PURCH_LINE | crate::schema::inventory::ITEM_JNL_LINE | crate::schema::transfer::TRANSFER_LINE, "Variant Code") => {
            let code = r.str("Variant Code").to_string();
            if code.is_empty() {
                return Ok(());
            }
            let doc_line = matches!(t.name.as_str(), crate::schema::tables::SALES_LINE | crate::schema::tables::PURCH_LINE);
            if doc_line && r.str("Type") != "Item" {
                refuse!("Type must be equal to 'Item' in {}: Document Type={}, Document No.={}, Line No.={}. Current value is '{}'.", t.name, r.str("Document Type"), r.str("Document No."), r.get("Line No.").as_int(), r.str("Type"));
            }
            let item = r.str(if doc_line { "No." } else { "Item No." }).to_string();
            let Some(v) = ctx.db.get(crate::schema::item_setup::ITEM_VARIANT, &Key(vec![Value::text(&item), Value::text(&code)])).cloned() else {
                refuse!("The Item Variant does not exist. Identification fields and values: Item No.='{item}',Code='{code}'");
            };
            if v.get("Blocked").as_bool() {
                refuse!("Blocked must be equal to 'No' in Item Variant: Item No.={item}, Code={code}. Current value is 'Yes'.");
            }
            if t.name == crate::schema::tables::SALES_LINE && v.get("Sales Blocked").as_bool() {
                refuse!("You cannot sell Item Variant {code} for Item {item} because the Sales Blocked check box is selected on the item variant card.");
            }
            if t.name == crate::schema::tables::PURCH_LINE && v.get("Purchasing Blocked").as_bool() {
                refuse!("You cannot purchase Item Variant {code} for Item {item} because the Purchasing Blocked check box is selected on the item variant card.");
            }
            if !v.str("Description").is_empty() {
                r.set("Description", v.get("Description").clone());
                r.set("Description 2", v.get("Description 2").clone());
            }
        }
        (ITEM_UOM, "Qty. per Unit of Measure") => {
            let base = ctx.db.get_code(crate::schema::tables::ITEM, r.str("Item No.")).map(|i| i.str("Base Unit of Measure").to_string()).unwrap_or_default();
            let q = r.get("Qty. per Unit of Measure").as_dec();
            if q <= Decimal::ZERO {
                refuse!("Qty. per Unit of Measure must be greater than 0.");
            }
            if r.str("Code") == base && q != Decimal::ONE {
                refuse!("The quantity per base unit of measure must be 1.");
            }
        }
        _ => {}
    }
    Ok(())
}
