//! Printing documents to PDF: quotes, order confirmations, invoices, credit memos, purchase
//! orders, shipments and receipts, in the layout of BC's standard reports (company block, title
//! and document details, bill-to address, lines, totals, payment details).

mod pdf;

use erpcraft_meta::{Key, Record, format_decimal};
use rust_decimal::Decimal;
use serde_json::{Value as Json, json};

use crate::calc::calc_fields;
use crate::commands::CommandDef;
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::finance::{PURCH_RCPT_HEADER, PURCH_RCPT_LINE, SALES_SHIPMENT_HEADER, SALES_SHIPMENT_LINE};
use crate::schema::tables::{
    COMPANY_INFO, GL_SETUP, PAYMENT_TERMS, PURCH_CR_HEADER, PURCH_CR_LINE, PURCH_HEADER, PURCH_INV_HEADER, PURCH_INV_LINE, PURCH_LINE,
    SALES_CR_HEADER, SALES_CR_LINE, SALES_HEADER, SALES_INV_HEADER, SALES_INV_LINE, SALES_LINE,
};
use pdf::{BLACK, GREY, Pdf, Rgb, fit};

const ACCENT: Rgb = Rgb(0.0, 0.52, 0.54);
const LIGHT: Rgb = Rgb(0.93, 0.93, 0.92);
const LEFT: f32 = 50.0;
const RIGHT: f32 = 545.0;
const BOTTOM: f32 = 770.0;

/// How one document type prints.
struct Kind {
    title: &'static str,
    line_table: &'static str,
    /// Line → header link: (line field, header field) pairs.
    link: &'static [(&'static str, &'static str)],
    sales: bool,
    prices: bool,
    price_field: &'static str,
}

fn kind(table: &str, doc_type: &str) -> Option<Kind> {
    let doc = &[("Document Type", "Document Type"), ("Document No.", "No.")][..];
    let posted = &[("Document No.", "No.")][..];
    Some(match (table, doc_type) {
        (SALES_HEADER, "Quote") => Kind { title: "Sales - Quote", line_table: SALES_LINE, link: doc, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_HEADER, "Order") => Kind { title: "Order Confirmation", line_table: SALES_LINE, link: doc, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_HEADER, "Invoice") => Kind { title: "Pro Forma Invoice", line_table: SALES_LINE, link: doc, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_HEADER, _) => Kind { title: "Sales - Credit Memo (Draft)", line_table: SALES_LINE, link: doc, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_INV_HEADER, _) => Kind { title: "Sales - Invoice", line_table: SALES_INV_LINE, link: posted, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_CR_HEADER, _) => Kind { title: "Sales - Credit Memo", line_table: SALES_CR_LINE, link: posted, sales: true, prices: true, price_field: "Unit Price" },
        (SALES_SHIPMENT_HEADER, _) => Kind { title: "Sales - Shipment", line_table: SALES_SHIPMENT_LINE, link: posted, sales: true, prices: false, price_field: "" },
        (PURCH_HEADER, "Quote") => Kind { title: "Purchase - Quote", line_table: PURCH_LINE, link: doc, sales: false, prices: true, price_field: "Direct Unit Cost" },
        (PURCH_HEADER, "Order") => Kind { title: "Purchase Order", line_table: PURCH_LINE, link: doc, sales: false, prices: true, price_field: "Direct Unit Cost" },
        (PURCH_HEADER, _) => Kind { title: "Purchase Document (Draft)", line_table: PURCH_LINE, link: doc, sales: false, prices: true, price_field: "Direct Unit Cost" },
        (PURCH_INV_HEADER, _) => Kind { title: "Purchase - Invoice", line_table: PURCH_INV_LINE, link: posted, sales: false, prices: true, price_field: "Direct Unit Cost" },
        (PURCH_CR_HEADER, _) => Kind { title: "Purchase - Credit Memo", line_table: PURCH_CR_LINE, link: posted, sales: false, prices: true, price_field: "Direct Unit Cost" },
        (PURCH_RCPT_HEADER, _) => Kind { title: "Purchase - Receipt", line_table: PURCH_RCPT_LINE, link: posted, sales: false, prices: false, price_field: "" },
        _ => return None,
    })
}

fn amount(d: Decimal) -> String {
    format_decimal(d, 2)
}

fn date(r: &Record, f: &str) -> String {
    r.get(f).as_date().map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_default()
}

/// Renders the document `key` of `table` as a PDF.
pub fn render(ctx: &Ctx, table: &str, key: &Key, report: Option<&str>) -> Result<(Vec<u8>, String, usize)> {
    let t = ctx.table(table)?;
    let mut h = ctx.get(&t.name, key)?;
    calc_fields(ctx.db, ctx.work_date, &t.name, &mut h);
    let Some(mut k) = kind(&t.name, h.str("Document Type")) else {
        refuse!("{} cannot be printed. Print sales and purchase documents, posted invoices, credit memos, shipments or receipts.", t.caption);
    };
    // BC's other reports on an open sales document: Pro Forma Invoice (prices) and Work Order (no
    // prices, for the workshop).
    match report {
        Some("pro_forma") if k.sales && t.name == SALES_HEADER => k.title = "Pro Forma Invoice",
        Some("work_order") if k.sales && t.name == SALES_HEADER => {
            k.title = "Work Order";
            k.prices = false;
        }
        None | Some("default") => {}
        Some(other) => refuse!("Report '{other}' is not available for {}. Use default, pro_forma or work_order.", t.caption),
    }
    let company = ctx.setup(COMPANY_INFO)?;
    let lcy = match ctx.setup(GL_SETUP)?.str("LCY Code") {
        "" => "LCY".to_string(),
        c => c.to_string(),
    };
    let mut lines: Vec<Record> = ctx
        .db
        .rows(k.line_table)
        .filter(|l| k.link.iter().all(|(lf, hf)| l.get(lf) == h.get(hf)))
        .cloned()
        .collect();
    lines.sort_by_key(|l| l.get("Line No.").as_int());

    let mut p = Pdf::new();
    let doc_no = h.str("No.").to_string();
    let (party_no_f, party_name_f, addr) = if k.sales {
        ("Sell-to Customer No.", "Sell-to Customer Name", "Sell-to ")
    } else {
        ("Buy-from Vendor No.", "Buy-from Vendor Name", "Buy-from ")
    };

    // Company block (left) and title block (right).
    let header = |p: &mut Pdf, page: usize| {
        p.text(LEFT, 60.0, 15.0, true, BLACK, company.str("Name"));
        let mut y = 76.0;
        for line in [
            company.str("Address").to_string(),
            [company.str("Post Code"), company.str("City")].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" "),
            if company.str("Phone No.").is_empty() { String::new() } else { format!("Phone {}", company.str("Phone No.")) },
            if company.str("VAT Registration No.").is_empty() { String::new() } else { format!("NPWP / VAT Reg. No. {}", company.str("VAT Registration No.")) },
        ] {
            if !line.is_empty() {
                p.text(LEFT, y, 8.5, false, GREY, &line);
                y += 12.0;
            }
        }
        p.text_right(RIGHT, 60.0, 17.0, true, ACCENT, k.title);
        p.text_right(RIGHT, 78.0, 10.0, true, BLACK, &doc_no);
        p.text_right(RIGHT, 92.0, 8.0, false, GREY, &format!("Page {page}"));
    };
    header(&mut p, 1);

    // Bill-to / buy-from address and document details.
    let mut y = 150.0;
    p.text(LEFT, y - 14.0, 8.0, false, GREY, if k.sales { "Bill-to" } else { "Vendor" });
    let party_lines = [
        h.str(party_name_f).to_string(),
        h.str(&format!("{addr}Address")).to_string(),
        [h.str(&format!("{addr}Post Code")), h.str(&format!("{addr}City"))].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" "),
    ];
    for (i, l) in party_lines.iter().filter(|l| !l.is_empty()).enumerate() {
        p.text(LEFT, y, if i == 0 { 10.5 } else { 9.0 }, i == 0, BLACK, l);
        y += 13.0;
    }
    let mut details: Vec<(&str, String)> = vec![
        (if k.sales { "Customer No." } else { "Vendor No." }, h.str(party_no_f).to_string()),
        ("Document Date", date(&h, "Document Date")),
    ];
    if !date(&h, "Posting Date").is_empty() {
        details.push(("Posting Date", date(&h, "Posting Date")));
    }
    if k.prices && !date(&h, "Due Date").is_empty() {
        details.push(("Due Date", date(&h, "Due Date")));
    }
    for f in ["External Document No.", "Vendor Invoice No.", "Order No.", "Your Reference"] {
        if t.field(f).is_some() && !h.str(f).is_empty() {
            details.push((f, h.str(f).to_string()));
        }
    }
    let person = if k.sales { "Salesperson Code" } else { "Purchaser Code" };
    if t.field(person).is_some() && !h.str(person).is_empty() {
        details.push((if k.sales { "Salesperson" } else { "Purchaser" }, h.str(person).to_string()));
    }
    let mut dy = 136.0;
    for (label, value) in &details {
        p.text(360.0, dy, 8.5, false, GREY, label);
        p.text_right(RIGHT, dy, 8.5, false, BLACK, value);
        dy += 12.0;
    }
    y = y.max(dy) + 18.0;

    // Lines.
    let cols: Vec<(&str, f32, bool)> = if k.prices {
        vec![("No.", LEFT, false), ("Description", 110.0, false), ("Quantity", 330.0, true), ("Unit", 340.0, false), ("Unit Price", 430.0, true), ("Disc. %", 470.0, true), ("Amount", RIGHT, true)]
    } else {
        vec![("No.", LEFT, false), ("Description", 110.0, false), ("Quantity", 420.0, true), ("Unit", 435.0, false)]
    };
    let table_head = |p: &mut Pdf, y: f32| {
        p.rect(LEFT - 4.0, y - 11.0, RIGHT - LEFT + 8.0, 16.0, LIGHT);
        for (c, x, right) in &cols {
            if *right {
                p.text_right(*x, y, 8.0, true, BLACK, c);
            } else {
                p.text(*x, y, 8.0, true, BLACK, c);
            }
        }
    };
    table_head(&mut p, y);
    y += 18.0;
    let mut page = 1;
    for l in &lines {
        if y > BOTTOM {
            p.new_page();
            page += 1;
            header(&mut p, page);
            y = 130.0;
            table_head(&mut p, y);
            y += 18.0;
        }
        let comment = l.str("Type").is_empty();
        p.text(LEFT, y, 8.5, false, BLACK, &fit(l.str("No."), 8.5, false, 56.0));
        let desc_w = if k.prices { 170.0 } else { 290.0 };
        p.text(110.0, y, 8.5, false, BLACK, &fit(l.str("Description"), 8.5, false, desc_w));
        if !comment {
            let qty = l.get("Quantity").as_dec();
            if k.prices {
                p.text_right(330.0, y, 8.5, false, BLACK, &format_decimal(qty, 0));
                p.text(340.0, y, 8.5, false, BLACK, l.str("Unit of Measure Code"));
                p.text_right(430.0, y, 8.5, false, BLACK, &amount(l.get(k.price_field).as_dec()));
                let disc = l.get("Line Discount %").as_dec();
                if !disc.is_zero() {
                    p.text_right(470.0, y, 8.5, false, BLACK, &format_decimal(disc, 0));
                }
                p.text_right(RIGHT, y, 8.5, false, BLACK, &amount(l.get("Line Amount").as_dec()));
            } else {
                p.text_right(420.0, y, 8.5, false, BLACK, &format_decimal(qty, 0));
                p.text(435.0, y, 8.5, false, BLACK, l.str("Unit of Measure Code"));
            }
        }
        y += 14.0;
    }
    p.line(LEFT - 4.0, y - 6.0, RIGHT + 4.0, y - 6.0, 0.6, GREY);

    // Totals.
    if k.prices {
        if y > BOTTOM - 70.0 {
            p.new_page();
            page += 1;
            header(&mut p, page);
            y = 130.0;
        }
        let excl = h.get("Amount").as_dec();
        let incl = h.get("Amount Including VAT").as_dec();
        let rows = [(format!("Total {lcy} Excl. VAT"), excl, false), ("VAT Amount".to_string(), incl - excl, false), (format!("Total {lcy} Incl. VAT"), incl, true)];
        y += 8.0;
        for (label, v, strong) in rows {
            p.text(360.0, y, 9.0, strong, BLACK, &label);
            p.text_right(RIGHT, y, 9.0, strong, if strong { ACCENT } else { BLACK }, &amount(v));
            y += 14.0;
        }
        y += 14.0;
        // Payment details.
        let terms = h.str("Payment Terms Code");
        if !terms.is_empty() {
            let desc = ctx.db.get_code(PAYMENT_TERMS, terms).map(|r| r.str("Description").to_string()).unwrap_or_default();
            p.text(LEFT, y, 8.5, false, GREY, &format!("Payment Terms: {}", if desc.is_empty() { terms.to_string() } else { desc }));
            y += 12.0;
        }
        if k.sales && !company.str("Bank Account No.").is_empty() {
            p.text(LEFT, y, 8.5, false, GREY, &format!("Please pay to {} account {}", company.str("Bank Name"), company.str("Bank Account No.")));
        }
    }
    let pages = p.page_count();
    Ok((p.finish(), format!("{} {doc_no}", k.title), pages))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (RFC 4648) for returning PDFs over JSON.
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64 (RFC 4648), ignoring ASCII whitespace.
pub fn decode_base64(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = B64.iter().position(|a| *a == c).ok_or_else(|| EngineError::invalid("invalid base64 payload"))?;
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn s_print() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string", "description": "Sales Header, Purchase Header, Sales Invoice Header, Sales Cr.Memo Header, Purch. Inv. Header, Sales Shipment Header, …"},
        "key": {"description": "the document's key, e.g. [\"Order\", \"S-ORD1001\"] or \"PS-INV103001\""},
        "path": {"type": "string", "description": "write the PDF here (must end in .pdf); without it the PDF comes back as base64"},
        "report": {"type": "string", "enum": ["default", "pro_forma", "work_order"], "default": "default",
                   "description": "sales documents: Pro Forma Invoice or Work Order instead of the document itself"}},
        "required": ["table", "key"]})
}

fn print_document(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let table = p.get("table").and_then(Json::as_str).ok_or_else(|| EngineError::invalid("missing parameter 'table'"))?;
    let t = ctx.table(table)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    let (bytes, title, pages) = render(ctx, &t.name, &key, p.get("report").and_then(Json::as_str))?;
    let mut out = json!({"title": title, "pages": pages, "bytes": bytes.len()});
    match p.get("path").and_then(Json::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => {
            if !path.to_ascii_lowercase().ends_with(".pdf") {
                refuse!("The file name must end in .pdf.");
            }
            let written = ctx.write_file(std::path::Path::new(path), &bytes)?;
            out["path"] = json!(written.display().to_string());
            out["message"] = json!(format!("{title} was saved as {}.", written.display()));
        }
        None => out["pdf_base64"] = json!(base64(&bytes)),
    }
    Ok(out)
}

pub static COMMANDS: &[CommandDef] = &[CommandDef {
    id: "document.print",
    label: "Print",
    group: "Documents",
    summary: "Print a sales or purchase document, posted invoice, credit memo, shipment or receipt to PDF.",
    mutates: false,
    params: s_print,
    run: print_document,
}];

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
