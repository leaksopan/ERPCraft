//! MCP resources: read-only views of metadata and records under `erpcraft://` URIs.

use erpcraft_engine::{EngineError, ErrorKind, schema};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::jsonrpc::{INTERNAL_ERROR, INVALID_PARAMS, RESOURCE_NOT_FOUND, RpcError};

const MIME: &str = "application/json";

pub fn list() -> Value {
    json!({"resources": [
        {"uri": "erpcraft://company", "name": "company", "title": "Company Information",
         "description": "Company information, work date and user (company.info).", "mimeType": MIME},
        {"uri": "erpcraft://tables", "name": "tables", "title": "Tables",
         "description": "Every table with its BC object id, primary key and record count (table.list).", "mimeType": MIME},
        {"uri": "erpcraft://pages", "name": "pages", "title": "Pages",
         "description": "Every page: id, BC object id, type and source table (page.list).", "mimeType": MIME},
        {"uri": "erpcraft://commands", "name": "commands", "title": "Commands",
         "description": "Every command with its parameter schema (command.list).", "mimeType": MIME},
    ]})
}

pub fn templates() -> Value {
    json!({"resourceTemplates": [
        {"uriTemplate": "erpcraft://table/{name}", "name": "table", "title": "Table definition",
         "description": "Fields, types, options and relations of a table, e.g. erpcraft://table/Sales%20Header (table.describe).", "mimeType": MIME},
        {"uriTemplate": "erpcraft://page/{id}", "name": "page", "title": "Page definition",
         "description": "Layout of a page by id or BC object id, e.g. erpcraft://page/customer-card or erpcraft://page/21 (page.describe).", "mimeType": MIME},
        {"uriTemplate": "erpcraft://record/{table}/{key}", "name": "record",
         "title": "Record", "description": "One record with FlowFields calculated. Table and key are URL-encoded; multi-field keys are joined by '|', e.g. erpcraft://record/Sales%20Header/Order%7CS-ORD1001 (record.get).", "mimeType": MIME},
    ]})
}

/// `resources/read`: the JSON text of one resource.
pub fn read(backend: &mut dyn Backend, uri: &str) -> Result<Value, RpcError> {
    let (command, params) = resolve(uri)?;
    let result = backend.run(command, &params).map_err(|e| engine_error(uri, &e))?;
    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
    Ok(json!({"contents": [{"uri": uri, "mimeType": MIME, "text": text}]}))
}

/// The command and parameters behind a URI.
fn resolve(uri: &str) -> Result<(&'static str, Value), RpcError> {
    let not_found = || RpcError::new(RESOURCE_NOT_FOUND, format!("Resource not found: {uri}"));
    let rest = uri.strip_prefix("erpcraft://").ok_or_else(not_found)?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    match rest.trim_end_matches('/') {
        "company" => return Ok(("company.info", json!({}))),
        "tables" => return Ok(("table.list", json!({}))),
        "pages" => return Ok(("page.list", json!({}))),
        "commands" => return Ok(("command.list", json!({}))),
        _ => {}
    }
    if let Some(name) = rest.strip_prefix("table/") {
        let name = decode(name).ok_or_else(not_found)?;
        return Ok(("table.describe", json!({"table": name})));
    }
    if let Some(id) = rest.strip_prefix("page/") {
        let id = decode(id).ok_or_else(not_found)?;
        return Ok(("page.describe", json!({"page": id})));
    }
    if let Some(path) = rest.strip_prefix("record/") {
        let (table, key) = split_record_path(path).ok_or_else(not_found)?;
        return Ok(("record.get", json!({"table": table, "key": key})));
    }
    Err(not_found())
}

/// `{table}/{key}`. The table should be encoded (`G%2FL%20Entry`), but an unencoded `/` in a table
/// name is accepted too: the first split whose prefix names a table wins.
fn split_record_path(path: &str) -> Option<(String, String)> {
    let meta = schema::base_app();
    let splits: Vec<usize> = path.match_indices('/').map(|(i, _)| i).collect();
    for i in &splits {
        let (t, k) = (path.get(..*i)?, path.get(i + 1..)?);
        let table = decode(t)?;
        if meta.table(&table).is_some() {
            return Some((table, decode(k)?));
        }
    }
    // Unknown table: let record.get report it.
    let i = *splits.first()?;
    Some((decode(path.get(..i)?)?, decode(path.get(i + 1..)?)?))
}

/// Percent-decoding (`%20` → space). `None` for broken escapes or non-UTF-8 results.
pub fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = *bytes.get(i)?;
        if b == b'%' {
            let hex = s.get(i + 1..i + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encoding for building resource URIs (unreserved characters stay as they are).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn engine_error(uri: &str, e: &EngineError) -> RpcError {
    match e.kind {
        ErrorKind::NotFound => RpcError::new(RESOURCE_NOT_FOUND, format!("{uri}: {}", e.message)),
        ErrorKind::Invalid => RpcError::new(INVALID_PARAMS, format!("{uri}: {}", e.message)),
        ErrorKind::Refused | ErrorKind::Io | ErrorKind::Internal => RpcError::new(INTERNAL_ERROR, format!("{uri}: {}", e.message)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn percent_coding_roundtrips() {
        assert_eq!(decode("Sales%20Header").unwrap(), "Sales Header");
        assert_eq!(decode(&encode("G/L Entry")).unwrap(), "G/L Entry");
        assert_eq!(decode("%zz"), None);
        assert_eq!(decode("%4"), None);
        assert_eq!(decode("%E2%82%AC").unwrap(), "€");
    }

    #[test]
    fn record_paths_accept_raw_slashes_in_table_names() {
        assert_eq!(split_record_path("G/L Entry/1"), Some(("G/L Entry".into(), "1".into())));
        assert_eq!(split_record_path("Sales%20Header/Order%7CS-1"), Some(("Sales Header".into(), "Order|S-1".into())));
        assert_eq!(split_record_path("NoKey"), None);
    }
}
