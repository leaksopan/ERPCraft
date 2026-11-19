//! The desktop app's control protocol, shared by the app (server side) and [`crate::Remote`]
//! (client side).
//!
//! `erpcraft --control [port]` listens on `127.0.0.1:<port>` (default 7979). Each line one way is a
//! request `{"id": …, "method": "…", "params": {…}}`; each line back is `{"id": …, "ok": true,
//! "result": …}` or `{"id": …, "ok": false, "error": {"kind": "…", "message": "…"}}`. `kind` is an
//! engine error kind (`not_found`, `invalid`, `refused`, `io`, `conflict`, `permission`,
//! `internal`). See docs/control.md.

use erpcraft_engine::{EngineError, ErrorKind};
use serde_json::{Value, json};

pub use crate::jsonrpc::MAX_LINE_BYTES;

/// Port of `erpcraft --control` without a value.
pub const DEFAULT_PORT: u16 = 7979;
/// Where clients connect without an address.
pub const DEFAULT_ADDR: &str = "127.0.0.1:7979";

/// One parsed request line.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Echoed back unchanged (`null` when the request had none).
    pub id: Value,
    pub method: String,
    /// Always an object (a missing or `null` `params` becomes `{}`).
    pub params: Value,
}

/// Parses one request line. On failure returns the id to answer with (`null` when it could not be
/// read) and the error.
pub fn parse_request(line: &str) -> Result<Request, (Value, EngineError)> {
    let msg: Value = serde_json::from_str(line).map_err(|e| (Value::Null, EngineError::invalid(format!("request is not valid JSON: {e}"))))?;
    let Some(obj) = msg.as_object() else {
        return Err((Value::Null, EngineError::invalid("a request must be a JSON object")));
    };
    let id = obj.get("id").cloned().unwrap_or(Value::Null);
    if !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return Err((Value::Null, EngineError::invalid("id must be a string or a number")));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Err((id, EngineError::invalid("missing method (a string such as \"engine.run\")")));
    };
    let params = match obj.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err((id, EngineError::invalid("params must be an object"))),
    };
    Ok(Request { id, method: method.to_string(), params })
}

/// The request line for `method` (what [`crate::Remote`] sends).
pub fn request_line(id: u64, method: &str, params: &Value) -> String {
    json!({"id": id, "method": method, "params": params}).to_string()
}

/// The reply object for a request.
pub fn reply(id: &Value, result: Result<Value, EngineError>) -> Value {
    match result {
        Ok(r) => json!({"id": id, "ok": true, "result": r}),
        Err(e) => json!({"id": id, "ok": false, "error": {"kind": e.kind_name(), "message": e.message}}),
    }
}

/// [`reply`] as one line (no trailing newline).
pub fn reply_line(id: &Value, result: Result<Value, EngineError>) -> String {
    reply(id, result).to_string()
}

/// The engine error kind for a wire name; unknown names are [`ErrorKind::Internal`].
pub fn kind_from_name(name: &str) -> ErrorKind {
    match name {
        "not_found" => ErrorKind::NotFound,
        "invalid" => ErrorKind::Invalid,
        "refused" => ErrorKind::Refused,
        "io" => ErrorKind::Io,
        "conflict" => ErrorKind::Conflict,
        "permission" => ErrorKind::Permission,
        _ => ErrorKind::Internal,
    }
}

/// Reads a reply object back into a result.
pub fn result_of(reply: &Value) -> Result<Value, EngineError> {
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
        Some(false) => {
            let err = reply.get("error");
            let kind = err.and_then(|e| e.get("kind")).and_then(Value::as_str).unwrap_or("internal");
            let message = err.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("the app reported an error without a message");
            Err(EngineError { kind: kind_from_name(kind), message: message.to_string() })
        }
        None => Err(EngineError::io("malformed reply from the ERPCraft app (no 'ok' field)")),
    }
}

/// Normalises a control address: `7979` → `127.0.0.1:7979`, `host:port` unchanged. `None` when it
/// is neither.
pub fn normalize_addr(s: &str) -> Option<String> {
    let s = s.trim();
    if let Ok(port) = s.parse::<u16>() {
        return Some(format!("127.0.0.1:{port}"));
    }
    let (host, port) = s.rsplit_once(':')?;
    if host.is_empty() || host.contains(char::is_whitespace) || port.parse::<u16>().is_err() {
        return None;
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_requests_and_rejects_garbage() {
        let r = parse_request(r#"{"id": 3, "method": "engine.run", "params": {"command": "company.info"}}"#).unwrap();
        assert_eq!(r.id, json!(3));
        assert_eq!(r.method, "engine.run");
        assert_eq!(r.params["command"], "company.info");
        let r = parse_request(r#"{"method": "ping"}"#).unwrap();
        assert_eq!((r.id, r.params), (Value::Null, json!({})));

        let (id, e) = parse_request("{nope").unwrap_err();
        assert_eq!((id, e.kind), (Value::Null, ErrorKind::Invalid));
        assert!(parse_request("[1]").is_err());
        assert!(parse_request(r#"{"id": [1], "method": "ping"}"#).is_err());
        let (id, _) = parse_request(r#"{"id": "a", "params": {}}"#).unwrap_err();
        assert_eq!(id, json!("a"));
        assert!(parse_request(r#"{"id": 1, "method": "ping", "params": 5}"#).is_err());
    }

    #[test]
    fn replies_round_trip_error_kinds() {
        let ok = reply(&json!(1), Ok(json!({"a": 1})));
        assert_eq!(result_of(&ok).unwrap(), json!({"a": 1}));
        for e in [
            EngineError::not_found("x"),
            EngineError::invalid("x"),
            EngineError::refused("x"),
            EngineError::io("x"),
            EngineError::permission("x"),
            EngineError::internal("x"),
            EngineError { kind: ErrorKind::Conflict, message: "x".into() },
        ] {
            let back = result_of(&reply(&json!("id"), Err(e.clone()))).unwrap_err();
            assert_eq!(back, e);
        }
        assert_eq!(result_of(&json!({"id": 1})).unwrap_err().kind, ErrorKind::Io);
    }

    #[test]
    fn addresses() {
        assert_eq!(normalize_addr("7979").as_deref(), Some("127.0.0.1:7979"));
        assert_eq!(normalize_addr("localhost:8000").as_deref(), Some("localhost:8000"));
        assert_eq!(normalize_addr("[::1]:7979").as_deref(), Some("[::1]:7979"));
        assert_eq!(normalize_addr("run"), None);
        assert_eq!(normalize_addr("host:99999"), None);
        assert_eq!(normalize_addr(":80"), None);
    }
}
