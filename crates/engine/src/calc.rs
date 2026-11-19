//! FlowFields (AL `CalcFields`): values summed from other tables when a record is read.

use chrono::NaiveDate;
use erpcraft_meta::{Record, Value};
use rust_decimal::Decimal;

use crate::db::Database;
use crate::schema::tables::{
    BANK_ACCOUNT, BANK_LEDGER, CUST_LEDGER, CUSTOMER, GL_ACCOUNT, GL_ENTRY, ITEM, ITEM_LEDGER, PURCH_CR_HEADER,
    PURCH_CR_LINE, PURCH_HEADER, PURCH_INV_HEADER, PURCH_INV_LINE, PURCH_LINE, SALES_CR_HEADER, SALES_CR_LINE,
    SALES_HEADER, SALES_INV_HEADER, SALES_INV_LINE, SALES_LINE, VEND_LEDGER, VENDOR,
};

fn sum<'a>(rows: impl Iterator<Item = &'a Record>, field: &str) -> Decimal {
    rows.map(|r| r.get(field).as_dec()).sum()
}

/// Calculates every FlowField of `r` (a record of `table`).
pub fn calc_fields(db: &Database, work_date: NaiveDate, table: &str, r: &mut Record) {
    let no = r.str("No.").to_string();
    let set = |r: &mut Record, f: &str, d: Decimal| r.set(f, Value::Dec(d));
    match table {
        CUSTOMER => {
            let entries: Vec<&Record> = db.rows(CUST_LEDGER).filter(|e| e.str("Customer No.") == no).collect();
            set(r, "Balance (LCY)", sum(entries.iter().copied(), "Remaining Amount"));
            set(
                r,
                "Balance Due (LCY)",
                sum(entries.iter().copied().filter(|e| e.get("Due Date").as_date().is_some_and(|d| d <= work_date)), "Remaining Amount"),
            );
            set(r, "Sales (LCY)", sum(entries.iter().copied(), "Sales (LCY)"));
            set(r, "Payments (LCY)", -sum(entries.iter().copied().filter(|e| e.str("Document Type") == "Payment"), "Amount"));
            set(r, "Outstanding Orders (LCY)", outstanding(db, SALES_LINE, "Sell-to Customer No.", &no));
        }
        VENDOR => {
            let entries: Vec<&Record> = db.rows(VEND_LEDGER).filter(|e| e.str("Vendor No.") == no).collect();
            set(r, "Balance (LCY)", -sum(entries.iter().copied(), "Remaining Amount"));
            set(
                r,
                "Balance Due (LCY)",
                -sum(entries.iter().copied().filter(|e| e.get("Due Date").as_date().is_some_and(|d| d <= work_date)), "Remaining Amount"),
            );
            set(r, "Purchases (LCY)", sum(entries.iter().copied(), "Purchase (LCY)"));
            set(r, "Payments (LCY)", sum(entries.iter().copied().filter(|e| e.str("Document Type") == "Payment"), "Amount"));
            set(r, "Outstanding Orders (LCY)", outstanding(db, PURCH_LINE, "Buy-from Vendor No.", &no));
        }
        ITEM => {
            set(r, "Inventory", sum(db.rows(ITEM_LEDGER).filter(|e| e.str("Item No.") == no), "Quantity"));
            let on_order = |line_table: &str| {
                sum(
                    db.rows(line_table).filter(|l| l.str("Document Type") == "Order" && l.str("Type") == "Item" && l.str("No.") == no),
                    "Outstanding Quantity",
                )
            };
            set(r, "Qty. on Sales Order", on_order(SALES_LINE));
            set(r, "Qty. on Purch. Order", on_order(PURCH_LINE));
        }
        GL_ACCOUNT => {
            let bal = gl_balance(db, r);
            set(r, "Balance", bal);
            set(r, "Net Change", bal);
        }
        BANK_ACCOUNT => {
            set(r, "Balance", sum(db.rows(BANK_LEDGER).filter(|e| e.str("Bank Account No.") == no), "Amount"));
        }
        SALES_HEADER | PURCH_HEADER => {
            let line_table = if table == SALES_HEADER { SALES_LINE } else { PURCH_LINE };
            let doc_type = r.str("Document Type").to_string();
            let lines: Vec<&Record> =
                db.rows(line_table).filter(|l| l.str("Document Type") == doc_type && l.str("Document No.") == no).collect();
            let amount = sum(lines.iter().copied(), "Amount");
            let incl = sum(lines.iter().copied(), "Amount Including VAT");
            set(r, "Amount", amount);
            set(r, "Amount Including VAT", incl);
            set(r, "VAT Amount", incl - amount);
        }
        SALES_INV_HEADER | SALES_CR_HEADER | PURCH_INV_HEADER | PURCH_CR_HEADER => {
            let (line_table, ledger, sign) = match table {
                SALES_INV_HEADER => (SALES_INV_LINE, CUST_LEDGER, Decimal::ONE),
                SALES_CR_HEADER => (SALES_CR_LINE, CUST_LEDGER, Decimal::NEGATIVE_ONE),
                PURCH_INV_HEADER => (PURCH_INV_LINE, VEND_LEDGER, Decimal::NEGATIVE_ONE),
                _ => (PURCH_CR_LINE, VEND_LEDGER, Decimal::ONE),
            };
            let lines: Vec<&Record> = db.rows(line_table).filter(|l| l.str("Document No.") == no).collect();
            set(r, "Amount", sum(lines.iter().copied(), "Amount"));
            set(r, "Amount Including VAT", sum(lines.iter().copied(), "Amount Including VAT"));
            let remaining = sign
                * sum(
                    db.rows(ledger).filter(|e| e.str("Document No.") == no && e.str("Document Type") != "Payment"),
                    "Remaining Amount",
                );
            set(r, "Remaining Amount", remaining);
            r.set("Closed", Value::Bool(remaining.is_zero()));
        }
        crate::schema::fixed_assets::FIXED_ASSET | crate::schema::fixed_assets::FA_DEPR_BOOK => crate::fa::calc_fields(db, table, r),
        _ => {}
    }
}

fn outstanding(db: &Database, line_table: &str, party_field: &str, no: &str) -> Decimal {
    db.rows(line_table)
        .filter(|l| l.str("Document Type") == "Order" && l.str(party_field) == no)
        .map(|l| {
            let qty = l.get("Quantity").as_dec();
            if qty.is_zero() {
                Decimal::ZERO
            } else {
                crate::ctx::round_amount(l.get("Amount Including VAT").as_dec() * l.get("Outstanding Quantity").as_dec() / qty)
            }
        })
        .sum()
}

/// Balance of a G/L account; Total / End-Total accounts sum the Posting accounts in `Totaling`.
fn gl_balance(db: &Database, acc: &Record) -> Decimal {
    let no = acc.str("No.");
    match acc.str("Account Type") {
        "Posting" => sum(db.rows(GL_ENTRY).filter(|e| e.str("G/L Account No.") == no), "Amount"),
        "Total" | "End-Total" => {
            let ranges: Vec<(String, String)> = acc
                .str("Totaling")
                .split('|')
                .filter(|s| !s.trim().is_empty())
                .map(|part| match part.split_once("..") {
                    Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                    None => (part.trim().to_string(), part.trim().to_string()),
                })
                .collect();
            let in_range = |n: &str| ranges.iter().any(|(a, b)| n >= a.as_str() && n <= b.as_str());
            sum(db.rows(GL_ENTRY).filter(|e| in_range(e.str("G/L Account No."))), "Amount")
        }
        _ => Decimal::ZERO,
    }
}
