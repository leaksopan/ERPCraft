//! BC's Document Attachment (1173): files attached to a card, document or posted document, kept in
//! the company (base64 in a Blob field) so they travel with it to PostgreSQL. Attach a file, View
//! (written to a temp file for the app to open) and Attach as PDF (the document's own print).

use erpcraft_meta::{Key, Record, TableDef, Value};
use serde_json::{Value as Json, json};

use crate::commands::{CommandDef, opt_str};
use crate::ctx::Ctx;
use crate::error::{EngineError, Result};
use crate::refuse;
use crate::schema::attachments::{ATTACH_DOC_TYPES, DOC_ATTACHMENT};

/// The largest file attached (BC's own limit is 350 MB per file on the database; a company file
/// or a row this size is already more than ERPCraft keeps comfortably).
const MAX_BYTES: usize = 50 * 1024 * 1024;

/// The record a file belongs to: (Table ID, No., Document Type).
fn owner(ctx: &Ctx, p: &Json) -> Result<(&'static TableDef, Record)> {
    let table = opt_str(p, "table").ok_or_else(|| EngineError::invalid("missing parameter 'table'"))?;
    let t = ctx.table(table)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    Ok((t, ctx.get(&t.name, &key)?))
}

fn doc_type(t: &TableDef, r: &Record) -> String {
    match t.field("Document Type") {
        Some(_) if ATTACH_DOC_TYPES.contains(&r.str("Document Type")) => r.str("Document Type").to_string(),
        // Cards and posted documents file under BC's first value.
        _ => ATTACH_DOC_TYPES[0].to_string(),
    }
}

fn file_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" => "Image",
        "pdf" => "PDF",
        "doc" | "docx" => "Word",
        "xls" | "xlsx" | "csv" => "Excel",
        "ppt" | "pptx" => "PowerPoint",
        "msg" | "eml" => "Email",
        "xml" => "XML",
        _ => "Other",
    }
}

fn store(ctx: &mut Ctx, t: &TableDef, owner: &Record, file_name: &str, bytes: &[u8]) -> Result<Json> {
    if bytes.len() > MAX_BYTES {
        refuse!("The file is {} MB. Files of up to {} MB can be attached.", bytes.len() / (1024 * 1024), MAX_BYTES / (1024 * 1024));
    }
    let at = ctx.table(DOC_ATTACHMENT)?;
    let no = owner.str("No.").to_string();
    let dt = doc_type(t, owner);
    let table_id = i64::from(t.object_id);
    let id = ctx
        .db
        .rows(DOC_ATTACHMENT)
        .filter(|a| a.get("Table ID").as_int() == table_id && a.str("No.") == no && a.str("Document Type") == dt)
        .map(|a| a.get("ID").as_int())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let path = std::path::Path::new(file_name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut r = at.init_record();
    r.set("Table ID", Value::Int(table_id));
    r.set("No.", Value::text(&no));
    r.set("Document Type", Value::text(&dt));
    r.set("Line No.", Value::Int(0));
    r.set("ID", Value::Int(id));
    r.set("File Name", Value::text(stem.chars().take(250).collect::<String>()));
    r.set("File Extension", Value::text(ext.chars().take(30).collect::<String>()));
    r.set("File Type", Value::text(file_type(&ext)));
    r.set("Attached Date", Value::text(chrono::Local::now().format("%d/%m/%Y %H:%M").to_string()));
    r.set("User", Value::text(ctx.user.to_uppercase()));
    r.set("Content", Value::text(crate::print::base64(bytes)));
    ctx.db.insert(at, r)?;
    Ok(json!({"id": id, "message": format!("{stem}.{ext} was attached to {} {no}.", t.caption)}))
}

fn attach_file(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, r) = owner(ctx, p)?;
    let path = opt_str(p, "path").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| EngineError::invalid("missing parameter 'path'"))?;
    let resolved = ctx.check_read(std::path::Path::new(path))?;
    let meta = std::fs::metadata(&resolved).map_err(|e| EngineError::io(format!("{path}: {e}")))?;
    if meta.len() > MAX_BYTES as u64 {
        refuse!("The file is {} MB. Files of up to {} MB can be attached.", meta.len() / (1024 * 1024), MAX_BYTES / (1024 * 1024));
    }
    let bytes = std::fs::read(&resolved).map_err(|e| EngineError::io(format!("{path}: {e}")))?;
    store(ctx, t, &r, path, &bytes)
}

/// BC's Attach as PDF: the document as it prints, attached to itself.
fn attach_as_pdf(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let (t, r) = owner(ctx, p)?;
    let key = r.key(t);
    let (bytes, title, _) = crate::print::render(ctx, &t.name, &key, None)?;
    let name: String = title.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '.') { c } else { '_' }).collect();
    store(ctx, t, &r, &format!("{}.pdf", name.trim()), &bytes)
}

fn decode(s: &str) -> Result<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = A.iter().position(|a| *a == c).ok_or_else(|| EngineError::invalid("The attachment's content is damaged."))?;
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

/// BC's View / Download: the file written to the temp folder, for the app to open.
fn view(ctx: &mut Ctx, p: &Json) -> Result<Json> {
    let t = ctx.table(DOC_ATTACHMENT)?;
    let key = Key::from_json(t, p.get("key").unwrap_or(&Json::Null)).map_err(EngineError::invalid)?;
    let a = ctx.get(DOC_ATTACHMENT, &key)?;
    let bytes = decode(a.str("Content"))?;
    let name: String = format!("{}.{}", a.str("File Name"), a.str("File Extension"))
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '.' | '_') { c } else { '_' })
        .collect();
    let dir = match ctx.confine.and_then(|c| c.write_roots().first()) {
        Some(root) => root.to_path_buf(),
        None => std::env::temp_dir().join("ERPCraft"),
    };
    let path = ctx.write_file(&dir.join(name), &bytes)?;
    Ok(json!({"open_file": path.display().to_string(), "bytes": bytes.len()}))
}

fn s_owner_path() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}, "path": {"type": "string", "description": "the file to attach"}}, "required": ["table", "key", "path"]})
}
fn s_owner() -> Json {
    json!({"type": "object", "properties": {"table": {"type": "string"}, "key": {}}, "required": ["table", "key"]})
}
fn s_view() -> Json {
    json!({"type": "object", "properties": {"key": {"description": "the Document Attachment's key"}}, "required": ["key"]})
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "attachment.attach_file",
        label: "Attach a file",
        group: "Attachments",
        summary: "Attach a file to a card, document or posted document.",
        mutates: true,
        params: s_owner_path,
        run: attach_file,
    },
    CommandDef {
        id: "document.attach_as_pdf",
        label: "Attach as PDF",
        group: "Attachments",
        summary: "Print a document and attach the PDF to it.",
        mutates: true,
        params: s_owner,
        run: attach_as_pdf,
    },
    CommandDef { id: "attachment.view", label: "View", group: "Attachments", summary: "Write an attached file out so it can be opened.", mutates: false, params: s_view, run: view },
];

#[cfg(test)]
mod tests {
    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0, 255, 128, 7, 9]] {
            assert_eq!(super::decode(&crate::print::base64(data)).unwrap(), data);
        }
    }
}
