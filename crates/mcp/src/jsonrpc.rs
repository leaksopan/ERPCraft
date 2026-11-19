//! JSON-RPC 2.0 pieces: error codes, response builders and a bounded line reader for the
//! newline-delimited stdio transport.

use std::io::{self, BufRead};

use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// MCP: `resources/read` of a URI the server doesn't have.
pub const RESOURCE_NOT_FOUND: i64 = -32002;

/// Longest message the server accepts (one line).
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }
}

pub fn success(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn failure(id: &Value, e: &RpcError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message}})
}

/// One line read from the transport.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Text(String),
    /// Longer than the limit; the rest of it was discarded.
    TooLong,
    /// Not valid UTF-8.
    NotUtf8,
}

/// Reads up to the next `\n`, never buffering more than `max` bytes. `Ok(None)` at end of input.
pub fn read_line_bounded<R: BufRead>(r: &mut R, max: usize) -> io::Result<Option<Line>> {
    let mut buf: Vec<u8> = Vec::new();
    let mut too_long = false;
    let mut saw_any = false;
    loop {
        let chunk = match r.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if chunk.is_empty() {
            break;
        }
        saw_any = true;
        let (part, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        if !too_long {
            if buf.len() + part.len() > max {
                too_long = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(part);
            }
        }
        let used = done.unwrap_or(chunk.len());
        r.consume(used);
        if done.is_some() {
            break;
        }
    }
    if !saw_any {
        return Ok(None);
    }
    if too_long {
        return Ok(Some(Line::TooLong));
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(match String::from_utf8(buf) {
        Ok(s) => Line::Text(s),
        Err(_) => Line::NotUtf8,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn bounded_reader_splits_lines_and_drops_long_ones() {
        let data = b"abc\r\n0123456789\nxy";
        let mut r = io::BufReader::with_capacity(4, &data[..]);
        assert_eq!(read_line_bounded(&mut r, 5).unwrap(), Some(Line::Text("abc".into())));
        assert_eq!(read_line_bounded(&mut r, 5).unwrap(), Some(Line::TooLong));
        assert_eq!(read_line_bounded(&mut r, 5).unwrap(), Some(Line::Text("xy".into())));
        assert_eq!(read_line_bounded(&mut r, 5).unwrap(), None);
    }

    #[test]
    fn bounded_reader_flags_invalid_utf8() {
        let data = b"\xff\xfe\n";
        let mut r = io::BufReader::new(&data[..]);
        assert_eq!(read_line_bounded(&mut r, 100).unwrap(), Some(Line::NotUtf8));
    }
}
