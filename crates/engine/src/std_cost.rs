//! Standard cost worksheet (BC 5840 Standard Cost Worksheet Name, 5841 Standard Cost Worksheet,
//! reports 5851 Suggest Item Standard Cost, 5853 Copy Standard Cost Worksheet, 5855 Implement
//! Standard Cost Change): new standard costs are prepared per item, then implemented on the items
//! with revaluation journal lines for the inventory they value.

use erpcraft_meta::{Key, Record, Value};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::{Ctx, dec};
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::inventory::ITEM_JNL_LINE;
use crate::schema::tables::ITEM;

pub const WORKSHEET_NAME: &str = "Standard Cost Worksheet Name";
pub const WORKSHEET: &str = "Standard Cost Worksheet";

fn worksheet_of(p: &Json) -> String {
    opt_str(p, "worksheet").map(str::trim).filter(|s| !s.is_empty()).unwrap_or("DEFAULT").to_uppercase()
}

fn dec_param(p: &Json, k: &str, default: Decimal) -> Result<Decimal> {
    match p.get(k) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::String(s)) if s.trim().is_empty() => Ok(default),
        Some(Json::String(s)) => s.trim().parse::<Decimal>().map_err(|_| EngineError::invalid(format!("'{s}' is not a number."))),
        Some(v) => v.as_f64().and_then(|f| Decimal::try_from(f).ok()).ok_or_else(|| EngineError::invalid(format!("'{v}' is not a number."))),
    }
}

fn lines_of(ctx: &Ctx, ws: &str) -> Vec<Record> {
    let mut v: Vec<Record> = ctx.db.rows(WORKSHEET).filter(|l| l.str("Standard Cost Worksheet Name") == ws).cloned().collect();
    v.sort_by(|a, b| a.str("No.").cmp(b.str("No.")));
    v
}

/// BC's Suggest Item Standard Cost: a line per item with the new standard cost = the current one
/// times the adjustment factor (rounded to `rounding`).
fn suggest_item(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let ws = worksheet_of(p);
    ctx.get_code(WORKSHEET_NAME, &ws)?;
    let factor = dec_param(p, "standard_cost_factor", Decimal::ONE)?;
    let rounding = dec_param(p, "rounding_precision", Decimal::ZERO)?;
    let only = opt_str(p, "item_no").map(str::trim).filter(|s| !s.is_empty()).map(str::to_uppercase);
    let items: Vec<Record> = ctx.db.rows(ITEM).filter(|i| i.str("Type") == "Inventory" && only.as_ref().is_none_or(|o| i.str("No.") == o)).cloned().collect();
    let t = ctx.table(WORKSHEET)?;
    let mut n = 0;
    for item in items {
        let key = Key(vec![Value::text(&ws), Value::text("Item"), item.get("No.").clone()]);
        let mut l = ctx.db.get(WORKSHEET, &key).cloned().unwrap_or_else(|| t.init_record());
        let exists = !l.str("No.").is_empty();
        let std = item.get("Standard Cost").as_dec();
        let mut new = std * factor;
        new = if rounding > Decimal::ZERO { (new / rounding).round() * rounding } else { new.round_dp(5) };
        l.set("Standard Cost Worksheet Name", Value::text(&ws));
        l.set("Type", Value::text("Item"));
        l.set("No.", item.get("No.").clone());
        l.set("Description", item.get("Description").clone());
        l.set("Replenishment System", item.get("Replenishment System").clone());
        l.set("Standard Cost", dec(std));
        l.set("New Standard Cost", dec(new));
        l.set("Indirect Cost %", item.get("Indirect Cost %").clone());
        l.set("New Indirect Cost %", item.get("Indirect Cost %").clone());
        l.set("Overhead Rate", item.get("Overhead Rate").clone());
        l.set("New Overhead Rate", item.get("Overhead Rate").clone());
        l.set("Implemented", Value::Bool(false));
        if exists {
            ctx.db.modify(t, l)?;
        } else {
            ctx.db.insert(t, l)?;
        }
        n += 1;
    }
    Ok(json!({"lines": n, "message": format!("{n} line(s) were suggested.")}))
}

/// BC's Copy Standard Cost Worksheet: the lines of another worksheet.
fn copy(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let ws = worksheet_of(p);
    ctx.get_code(WORKSHEET_NAME, &ws)?;
    let from = opt_str(p, "from_worksheet").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::invalid("missing parameter 'from_worksheet'"))?.to_uppercase();
    if from == ws {
        refuse!("You cannot copy a worksheet onto itself.");
    }
    let t = ctx.table(WORKSHEET)?;
    let mut n = 0;
    for mut l in lines_of(ctx, &from) {
        l.set("Standard Cost Worksheet Name", Value::text(&ws));
        l.set("Implemented", Value::Bool(false));
        if ctx.db.get(&t.name, &l.key(t)).is_some() {
            ctx.db.modify(t, l)?;
        } else {
            ctx.db.insert(t, l)?;
        }
        n += 1;
    }
    Ok(json!({"lines": n, "message": format!("{n} line(s) were copied from {from}.")}))
}

/// BC's Implement Standard Cost Change: the items take their new standard costs; items costed at
/// standard get revaluation journal lines for the inventory they hold.
fn implement(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let ws = worksheet_of(p);
    let template = opt_str(p, "revaluation_template").unwrap_or("REVAL").to_uppercase();
    let batch = opt_str(p, "revaluation_batch").unwrap_or("DEFAULT").to_uppercase();
    let date = match opt_str(p, "posting_date").map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| EngineError::invalid(format!("'{s}' is not a date (YYYY-MM-DD).")))?,
        None => ctx.work_date,
    };
    let doc_no = opt_str(p, "document_no").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let todo: Vec<Record> = lines_of(ctx, &ws).into_iter().filter(|l| l.str("Type") == "Item" && !l.get("Implemented").as_bool()).collect();
    if todo.is_empty() {
        refuse!("There is nothing to implement in standard cost worksheet {ws}.");
    }
    let wt = ctx.table(WORKSHEET)?;
    let it = ctx.table(ITEM)?;
    let jt = ctx.table(ITEM_JNL_LINE)?;
    let mut revalued = 0;
    for l in &todo {
        let mut item = ctx.get_code(ITEM, l.str("No."))?;
        let new = l.get("New Standard Cost").as_dec();
        let standard = item.str("Costing Method") == "Standard";
        if standard && new != item.get("Standard Cost").as_dec() {
            // Revaluation lines for the inventory at the new cost.
            let before: Vec<i64> = ctx.db.rows(ITEM_JNL_LINE).filter(|j| j.str("Journal Template Name") == template && j.str("Journal Batch Name") == batch).map(|j| j.get("Line No.").as_int()).collect();
            crate::item_journal::calculate_inventory_value(ctx, &template, &batch, date, Some(l.str("No.")), false)?;
            let created: Vec<Record> = ctx
                .db
                .rows(ITEM_JNL_LINE)
                .filter(|j| j.str("Journal Template Name") == template && j.str("Journal Batch Name") == batch && !before.contains(&j.get("Line No.").as_int()))
                .cloned()
                .collect();
            for mut j in created {
                let v = crate::ctx::round_amount(j.get("Quantity").as_dec() * new);
                j.set("Unit Cost (Revalued)", dec(new));
                j.set("Inventory Value (Revalued)", dec(v));
                j.set("Unit Amount", dec(new));
                j.set("Amount", dec(v));
                if let Some(d) = &doc_no {
                    j.set("Document No.", Value::text(d));
                }
                ctx.db.modify(jt, j)?;
                revalued += 1;
            }
        }
        item.set("Standard Cost", dec(new));
        item.set("Indirect Cost %", l.get("New Indirect Cost %").clone());
        item.set("Overhead Rate", l.get("New Overhead Rate").clone());
        ctx.db.modify(it, item)?;
        let mut done = l.clone();
        done.set("Implemented", Value::Bool(true));
        ctx.db.modify(wt, done)?;
    }
    Ok(json!({"implemented": todo.len(), "revaluation_lines": revalued, "message": format!("{} standard cost(s) were implemented; {revalued} revaluation journal line(s) were created. Post the revaluation journal to revalue the inventory.", todo.len())}))
}

fn s_suggest() -> Json {
    json!({"type": "object", "properties": {"worksheet": {"type": "string", "default": "DEFAULT"}, "item_no": {"type": "string"},
        "standard_cost_factor": {"type": "number", "description": "Adjustment factor (1 keeps the cost)", "default": 1},
        "rounding_precision": {"type": "number", "description": "round the new cost to a multiple of this"}}})
}

fn s_copy() -> Json {
    json!({"type": "object", "properties": {"worksheet": {"type": "string", "default": "DEFAULT"}, "from_worksheet": {"type": "string"}}, "required": ["from_worksheet"]})
}

fn s_implement() -> Json {
    json!({"type": "object", "properties": {"worksheet": {"type": "string", "default": "DEFAULT"}, "posting_date": {"type": "string", "description": "YYYY-MM-DD"}, "document_no": {"type": "string"},
        "revaluation_template": {"type": "string", "default": "REVAL"}, "revaluation_batch": {"type": "string", "default": "DEFAULT"}}})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { id: "standard_cost.suggest_item", label: "Suggest Item Standard Cost", group: "Inventory", summary: "Suggest new standard costs for items, by an adjustment factor.", mutates: true, params: s_suggest, run: suggest_item },
    CommandDef { id: "standard_cost.copy", label: "Copy Standard Cost Worksheet", group: "Inventory", summary: "Copy the lines of another standard cost worksheet.", mutates: true, params: s_copy, run: copy },
    CommandDef { id: "standard_cost.implement", label: "Implement Standard Cost Changes", group: "Inventory", summary: "Set the items' new standard costs and prepare revaluation journal lines.", mutates: true, params: s_implement, run: implement },
];
