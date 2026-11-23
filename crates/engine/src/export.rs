//! Structured data import and export via `craft-csv` and `craft-xlsx`.
//!
//! All filesystem reads and writes go through [`Ctx::read_file`] and [`Ctx::write_file`], so
//! RFC-0015 confinement and crash-safe atomic writes apply automatically.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use craft_xlsx::{Cell, CellRef, CellValue, Column, Document, Limits, Package, Row, SheetEntry, SheetKind, StringItem, Value as XlsxValue, Visibility, Worksheet};
use erpcraft_meta::{FieldDef, FieldType, Record, TableDef, Value};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value as Json, json};

use crate::commands::{CommandDef, filters_param, list_records, opt_str, table_param};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::print::{base64, decode_base64};
use crate::records;
use crate::refuse;

const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_CSV_FIELDS: usize = 512;
const MAX_CSV_ROWS: usize = 100_000;

fn select_fields<'a>(t: &'a TableDef, page: Option<&erpcraft_meta::PageDef>, p: &Json) -> Result<Vec<&'a FieldDef>> {
    if let Some(arr) = p.get("fields").and_then(Json::as_array) {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let name = item.as_str().ok_or_else(|| EngineError::invalid("fields must be strings"))?;
            let f = t.field(name).ok_or_else(|| EngineError::not_found(format!("Field '{name}' does not exist in {}.", t.name)))?;
            out.push(f);
        }
        if out.is_empty() {
            refuse!("fields must name at least one column.");
        }
        return Ok(out);
    }
    if let Some(pg) = page
        && !pg.columns.is_empty()
    {
        let cols: Vec<&FieldDef> = pg.columns.iter().filter_map(|c| t.field(c)).collect();
        if !cols.is_empty() {
            return Ok(cols);
        }
    }
    Ok(t.fields.iter().collect())
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Text(s) => s.clone(),
        Value::Dec(d) => d.normalize().to_string(),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
        Value::Date(d) => d.format("%Y-%m-%d").to_string(),
    }
}

fn sanitize_sheet_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if matches!(c, '\\' | '/' | '?' | '*' | '[' | ']' | ':') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('\'');
    let fallback = if trimmed.is_empty() { "Data" } else { trimmed };
    fallback.chars().take(31).collect()
}

fn csv_delimiter(p: &Json) -> Result<char> {
    match opt_str(p, "delimiter") {
        None | Some(",") | Some("comma") => Ok(','),
        Some(";") | Some("semicolon") => Ok(';'),
        Some("\t") | Some("tab") => Ok('\t'),
        Some(other) => refuse!("Unsupported delimiter '{other}'. Use ',', ';', or 'tab'."),
    }
}

fn encode_csv(fields: &[&FieldDef], rows: &[Record], delim: char) -> String {
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(delim);
        }
        craft_csv::push_field(&mut out, &f.name, delim);
    }
    out.push_str("\r\n");
    for r in rows {
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                out.push(delim);
            }
            let text = value_text(r.get(&f.name));
            craft_csv::push_field(&mut out, &text, delim);
        }
        out.push_str("\r\n");
    }
    out
}

fn encode_xlsx(sheet_name: &str, fields: &[&FieldDef], rows: &[Record]) -> Result<Vec<u8>> {
    let mut doc = Document::default();
    let mut string_index: HashMap<String, u32> = HashMap::new();
    let mut intern = |text: &str, strings: &mut Vec<StringItem>| -> Result<u32> {
        if let Some(&idx) = string_index.get(text) {
            return Ok(idx);
        }
        let idx = u32::try_from(strings.len()).map_err(|_| EngineError::invalid("shared string table overflow"))?;
        strings.push(StringItem { text: Arc::from(text), ..Default::default() });
        string_index.insert(text.to_string(), idx);
        Ok(idx)
    };

    let mut sheet_rows: Vec<Row> = Vec::with_capacity(rows.len() + 1);
    let mut max_widths: Vec<usize> = fields.iter().map(|f| f.name.chars().count()).collect();

    let mut header_cells = Vec::with_capacity(fields.len());
    for (col_idx, f) in fields.iter().enumerate() {
        let col = u32::try_from(col_idx).map_err(|_| EngineError::invalid("too many columns"))?;
        let sid = intern(&f.name, &mut doc.shared_strings)?;
        header_cells.push(Cell { at: CellRef::new(0, col), value: CellValue::Shared(sid), ..Default::default() });
    }
    sheet_rows.push(Row { index: 0, cells: header_cells, ..Default::default() });

    for (row_offset, rec) in rows.iter().enumerate() {
        let row_num = u32::try_from(row_offset + 1).map_err(|_| EngineError::invalid("too many rows"))?;
        let mut cells = Vec::with_capacity(fields.len());
        for (col_idx, f) in fields.iter().enumerate() {
            let col = u32::try_from(col_idx).map_err(|_| EngineError::invalid("too many columns"))?;
            let val = rec.get(&f.name);
            let display_len = value_text(val).chars().count();
            if let Some(w) = max_widths.get_mut(col_idx) {
                *w = (*w).max(display_len);
            }
            let cell_val = match val {
                Value::Null => CellValue::Empty,
                Value::Dec(d) => match d.to_f64() {
                    Some(n) if n.is_finite() => CellValue::Number(n),
                    _ => CellValue::Shared(intern(&d.normalize().to_string(), &mut doc.shared_strings)?),
                },
                Value::Int(i) => CellValue::Number(*i as f64),
                Value::Bool(b) => CellValue::Bool(*b),
                Value::Date(d) => CellValue::Shared(intern(&d.format("%Y-%m-%d").to_string(), &mut doc.shared_strings)?),
                Value::Text(s) if s.is_empty() => CellValue::Empty,
                Value::Text(s) => CellValue::Shared(intern(s, &mut doc.shared_strings)?),
            };
            if !matches!(cell_val, CellValue::Empty) {
                cells.push(Cell { at: CellRef::new(row_num, col), value: cell_val, ..Default::default() });
            }
        }
        sheet_rows.push(Row { index: row_num, cells, ..Default::default() });
    }

    let columns: Vec<Column> = max_widths
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            let col = u32::try_from(i).ok()?;
            Some(Column { first: col, last: col, width: Some((*w).clamp(8, 48) as f64 + 2.0), custom_width: true, ..Default::default() })
        })
        .collect();

    let mut ws = Worksheet { rows: sheet_rows, ..Default::default() };
    ws.part.columns = columns;

    doc.workbook.sheets.push(SheetEntry {
        name: sanitize_sheet_name(sheet_name),
        sheet_id: 1,
        visibility: Visibility::Visible,
        rel_id: String::new(),
        kind: SheetKind::Worksheet,
        part: None,
    });
    doc.worksheets.push(Some(ws));

    doc.write().map_err(|e| EngineError::io(format!("failed to encode xlsx: {e}")))
}

fn export_csv(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, page) = table_param(ctx, p)?;
    let fields = select_fields(t, page, p)?;
    let filters = filters_param(p);
    let search = opt_str(p, "search").unwrap_or("");
    let rows = list_records(ctx.db, ctx.work_date, t, page, &filters, search);
    let delim = csv_delimiter(p)?;
    let csv = encode_csv(&fields, &rows, delim);
    let col_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    let mut out = json!({"table": t.name, "rows": rows.len(), "columns": col_names, "bytes": csv.len()});
    match opt_str(p, "path") {
        Some(path) => {
            if !path.to_ascii_lowercase().ends_with(".csv") {
                refuse!("The file name must end in .csv.");
            }
            let written = ctx.write_file(Path::new(path), csv.as_bytes())?;
            out["path"] = json!(written.display().to_string());
            out["message"] = json!(format!("Exported {} row(s) of {} to {}.", rows.len(), t.name, written.display()));
        }
        None => out["csv"] = json!(csv),
    }
    Ok(out)
}

fn export_xlsx(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, page) = table_param(ctx, p)?;
    let fields = select_fields(t, page, p)?;
    let filters = filters_param(p);
    let search = opt_str(p, "search").unwrap_or("");
    let rows = list_records(ctx.db, ctx.work_date, t, page, &filters, search);
    let sheet_name = sanitize_sheet_name(opt_str(p, "sheet_name").unwrap_or(&t.name));
    let bytes = encode_xlsx(&sheet_name, &fields, &rows)?;
    let col_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    let mut out = json!({"table": t.name, "sheet_name": sheet_name, "rows": rows.len(), "columns": col_names, "bytes": bytes.len()});
    match opt_str(p, "path") {
        Some(path) => {
            if !path.to_ascii_lowercase().ends_with(".xlsx") {
                refuse!("The file name must end in .xlsx.");
            }
            let written = ctx.write_file(Path::new(path), &bytes)?;
            out["path"] = json!(written.display().to_string());
            out["message"] = json!(format!("Exported {} row(s) of {} to {}.", rows.len(), t.name, written.display()));
        }
        None => out["xlsx_base64"] = json!(base64(&bytes)),
    }
    Ok(out)
}

fn map_headers(t: &TableDef, header_row: &[String]) -> Result<Vec<Option<String>>> {
    let mut mapped = Vec::with_capacity(header_row.len());
    let mut any_known = false;
    for raw in header_row {
        let name = raw.trim();
        if name.is_empty() {
            mapped.push(None);
            continue;
        }
        if let Some(f) = t.fields.iter().find(|f| f.name.eq_ignore_ascii_case(name) || f.caption.eq_ignore_ascii_case(name)) {
            if f.is_flow() {
                mapped.push(None);
            } else {
                mapped.push(Some(f.name.clone()));
                any_known = true;
            }
        } else {
            refuse!("Unknown column '{name}' for table '{}'.", t.name);
        }
    }
    if !any_known {
        refuse!("Import header must match at least one stored field of '{}'.", t.name);
    }
    Ok(mapped)
}

fn cell_to_json(field: &FieldDef, raw: &str) -> Json {
    let trimmed = raw.trim();
    match &field.ty {
        FieldType::Boolean => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => json!(true),
            "false" | "no" | "0" | "" => json!(false),
            _ => json!(trimmed),
        },
        FieldType::Integer => trimmed.parse::<i64>().map(|i| json!(i)).unwrap_or_else(|_| json!(trimmed)),
        _ => json!(trimmed),
    }
}

fn insert_mapped_rows(ctx: &mut Ctx, t: &TableDef, headers: &[Option<String>], data_rows: &[Vec<String>], dry_run: bool) -> Result<Json> {
    let mut imported = 0usize;
    let mut skipped_blank = 0usize;
    let mut keys: Vec<Json> = Vec::new();
    for (row_idx, row) in data_rows.iter().enumerate() {
        if row.iter().all(|c| c.trim().is_empty()) {
            skipped_blank += 1;
            continue;
        }
        let mut values = Map::new();
        for (col_idx, field_opt) in headers.iter().enumerate() {
            let Some(field_name) = field_opt else { continue };
            let Some(cell) = row.get(col_idx) else { continue };
            if cell.trim().is_empty() {
                continue;
            }
            let Some(field_def) = t.field(field_name) else { continue };
            values.insert(field_name.clone(), cell_to_json(field_def, cell));
        }
        if values.is_empty() {
            skipped_blank += 1;
            continue;
        }
        if !dry_run {
            let rec = records::create(ctx, t, &values, None)
                .map_err(|e| EngineError { kind: e.kind, message: format!("Row {}: {}", row_idx + 2, e.message) })?;
            keys.push(rec.key(t).to_json());
        }
        imported += 1;
    }
    Ok(json!({
        "table": t.name,
        "imported": imported,
        "skipped_blank": skipped_blank,
        "dry_run": dry_run,
        "keys": keys,
        "message": if dry_run {
            format!("Validated {imported} row(s) for {} (dry run).", t.name)
        } else {
            format!("Imported {imported} row(s) into {}.", t.name)
        },
    }))
}

fn import_csv(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let text = match (opt_str(p, "path"), p.get("csv").and_then(Json::as_str)) {
        (Some(path), _) => {
            let bytes = ctx.read_file(Path::new(path), MAX_IMPORT_BYTES)?;
            craft_csv::decode_text(&bytes)
        }
        (None, Some(csv)) => csv.strip_prefix('\u{feff}').unwrap_or(csv).to_string(),
        (None, None) => return Err(EngineError::invalid("provide either 'path' or 'csv'")),
    };
    let delim = match opt_str(p, "delimiter") {
        Some(_) => csv_delimiter(p)?,
        None => char::from(craft_csv::sniff_delimiter(&text, '"')),
    };
    let rows = craft_csv::records(&text, delim, '"', MAX_CSV_FIELDS, MAX_CSV_ROWS);
    let Some((header, data)) = rows.split_first() else {
        refuse!("CSV input is empty.");
    };
    let headers = map_headers(t, header)?;
    let dry_run = p.get("dry_run").and_then(Json::as_bool).unwrap_or(false);
    insert_mapped_rows(ctx, t, &headers, data, dry_run)
}

fn xlsx_value_to_string(v: &XlsxValue) -> String {
    match v {
        XlsxValue::Empty => String::new(),
        XlsxValue::Number(n) => craft_xlsx::num(*n),
        XlsxValue::Text(s) => s.to_string(),
        XlsxValue::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
        XlsxValue::Error(e) => e.as_str().to_string(),
    }
}

fn import_xlsx(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, _) = table_param(ctx, p)?;
    let bytes = match (opt_str(p, "path"), opt_str(p, "xlsx_base64")) {
        (Some(path), _) => ctx.read_file(Path::new(path), MAX_IMPORT_BYTES)?,
        (None, Some(b64)) => decode_base64(b64)?,
        (None, None) => return Err(EngineError::invalid("provide either 'path' or 'xlsx_base64'")),
    };
    let mut pkg = Package::open(&bytes, &Limits::default()).map_err(|e| EngineError::invalid(format!("invalid xlsx workbook: {e}")))?;
    let wb = pkg.workbook().map_err(|e| EngineError::invalid(format!("invalid xlsx workbook: {e}")))?;
    let strings = pkg.shared_strings().map_err(|e| EngineError::invalid(format!("invalid xlsx shared strings: {e}")))?;
    let sheet_entry = match opt_str(p, "sheet") {
        Some(name) => wb
            .sheets
            .iter()
            .find(|s| s.kind == SheetKind::Worksheet && s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| EngineError::not_found(format!("Worksheet '{name}' does not exist in workbook.")))?,
        None => wb
            .sheets
            .iter()
            .find(|s| s.kind == SheetKind::Worksheet)
            .ok_or_else(|| EngineError::invalid("workbook contains no worksheets"))?,
    }
    .clone();

    let mut raw_rows: Vec<(u32, Vec<(u32, String)>)> = Vec::new();
    let mut max_col = 0usize;
    pkg.sheet(&sheet_entry, |row| {
        let mut cells = Vec::with_capacity(row.cells.len());
        for c in &row.cells {
            let text = xlsx_value_to_string(&c.value.resolve(&strings, wb.date_1904));
            if !text.is_empty() {
                max_col = max_col.max(c.at.col as usize + 1);
            }
            cells.push((c.at.col, text));
        }
        raw_rows.push((row.index, cells));
        Ok(())
    })
    .map_err(|e| EngineError::invalid(format!("failed reading worksheet: {e}")))?;

    if raw_rows.is_empty() {
        refuse!("Worksheet '{}' is empty.", sheet_entry.name);
    }
    let cols = max_col.min(MAX_CSV_FIELDS);
    let mut matrix: Vec<Vec<String>> = Vec::with_capacity(raw_rows.len());
    for (_, cells) in raw_rows {
        let mut line = vec![String::new(); cols];
        for (col, text) in cells {
            if let Some(slot) = line.get_mut(col as usize) {
                *slot = text;
            }
        }
        matrix.push(line);
    }
    let Some((header, data)) = matrix.split_first() else {
        refuse!("Worksheet '{}' is empty.", sheet_entry.name);
    };
    let headers = map_headers(t, header)?;
    let dry_run = p.get("dry_run").and_then(Json::as_bool).unwrap_or(false);
    insert_mapped_rows(ctx, t, &headers, data, dry_run)
}

fn s_export_csv() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"},
        "page": {"type": "string", "description": "Optional list page id (applies its view and default columns)"},
        "filters": {"type": "object", "additionalProperties": {"type": "string"}},
        "search": {"type": "string"},
        "fields": {"type": "array", "items": {"type": "string"}},
        "delimiter": {"type": "string", "enum": [",", ";", "tab"], "default": ","},
        "path": {"type": "string", "description": "Write CSV to this file (must end in .csv); omitted returns inline csv text"}}})
}

fn s_export_xlsx() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"},
        "page": {"type": "string", "description": "Optional list page id (applies its view and default columns)"},
        "filters": {"type": "object", "additionalProperties": {"type": "string"}},
        "search": {"type": "string"},
        "fields": {"type": "array", "items": {"type": "string"}},
        "sheet_name": {"type": "string"},
        "path": {"type": "string", "description": "Write workbook to this file (must end in .xlsx); omitted returns xlsx_base64"}}})
}

fn s_import_csv() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"},
        "path": {"type": "string", "description": "Path to a CSV file (subject to RFC-0015 read roots)"},
        "csv": {"type": "string", "description": "Inline CSV text"},
        "delimiter": {"type": "string", "enum": [",", ";", "tab"], "description": "Optional delimiter override (auto-detected via craft-csv when omitted)"},
        "dry_run": {"type": "boolean", "default": false}},
        "required": ["table"]})
}

fn s_import_xlsx() -> Json {
    json!({"type": "object", "properties": {
        "table": {"type": "string"},
        "path": {"type": "string", "description": "Path to an .xlsx workbook (subject to RFC-0015 read roots)"},
        "xlsx_base64": {"type": "string", "description": "Inline base64-encoded .xlsx workbook"},
        "sheet": {"type": "string", "description": "Worksheet name (default: first worksheet)"},
        "dry_run": {"type": "boolean", "default": false}},
        "required": ["table"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "data.export_csv",
        label: "Export to CSV",
        group: "Data",
        summary: "Export records of a table or list page to RFC-4180 CSV via craft-csv.",
        mutates: false,
        params: s_export_csv,
        run: export_csv,
    },
    CommandDef {
        id: "data.export_xlsx",
        label: "Export to Excel (.xlsx)",
        group: "Data",
        summary: "Export records of a table or list page to an Excel .xlsx workbook via craft-xlsx.",
        mutates: false,
        params: s_export_xlsx,
        run: export_xlsx,
    },
    CommandDef {
        id: "data.import_csv",
        label: "Import from CSV",
        group: "Data",
        summary: "Import rows from a CSV file or text into a table with full transactional validation via craft-csv.",
        mutates: true,
        params: s_import_csv,
        run: import_csv,
    },
    CommandDef {
        id: "data.import_xlsx",
        label: "Import from Excel (.xlsx)",
        group: "Data",
        summary: "Import rows from an Excel .xlsx workbook into a table with full transactional validation via craft-xlsx.",
        mutates: true,
        params: s_import_xlsx,
        run: import_xlsx,
    },
];
