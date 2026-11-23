//! The MCP server: JSON-RPC 2.0 requests in, responses out, one message per line.

use std::io::{self, BufRead, Write};

use serde_json::{Map, Value, json};

use crate::backend::{Backend, CommandInfo, Headless};
use crate::jsonrpc::{self, INVALID_REQUEST, Line, MAX_LINE_BYTES, METHOD_NOT_FOUND, PARSE_ERROR, RpcError};
use crate::{prompts, resources};

/// Protocol revisions the server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const SERVER_NAME: &str = "erpcraft";

pub const INSTRUCTIONS: &str = "ERPCraft is a pure-Rust ERP (finance, sales, purchasing, inventory, fixed assets) running \
headless on one company data file or PostgreSQL database. Everything is a command: every tool here is one engine command (command_run runs any \
command by id, command_list lists them with parameter schemas), and the desktop app, web app, and erpcraft-cli call the same commands.\n\
- Standard ERP table and field names are used: Customer, Item, Sales Header, Sales Line, Cust. Ledger Entry, \
G/L Entry; field names like \"No.\", \"Balance (LCY)\". Use table_describe / page_describe (or the erpcraft://table/{name} \
and erpcraft://page/{id} resources) to see fields and options.\n\
- Read data with record_list {table or page, filters, search, fields, limit}. Filters support expressions per field: \
\"10000\", \"<>Open\", \"10000|20000\", \">100\", \"2026-01-01..2026-01-31\", \"*text*\". A list page id (sales-order-list) applies \
its view. record_get takes the primary key (an array for multi-field keys, e.g. [\"Order\", \"S-ORD1001\"]).\n\
- Change data with record_new / record_modify / record_validate (fields are validated by table triggers). Each command is \
a transaction: on error nothing changes.\n\
- Documents: sales_document_new (with lines), sales_line_add, sales_document_release, sales_document_post (ships and \
invoices an order); purchase_* mirror them. Payments: payment_customer_register, payment_vendor_register. Reports: \
customer_statistics, item_availability, report_trial_balance.\n\
- If the company is empty, company_init {demo: true} creates the demo company. Posting uses the work date \
(session_workdate).";

/// Added to the instructions when the server drives the running desktop app.
const UI_INSTRUCTIONS: &str = "\n- This server drives the running ERPCraft desktop app: commands run in the app's session and \
the app shows their effect. ui_open {spec} opens a page (\"customer-list\", \"customer-card:10000\", \
\"sales-order-list>sales-order:Order|S-ORD1001\"), ui_state shows the open pages, ui_screenshot {path} saves a PNG \
of the window.";

/// Name of the generic tool that runs any command by id.
const RUN_TOOL: &str = "command_run";
/// The `ui_*` tools a running app offers: tool name, control method, title, description.
const UI_TOOLS: &[(&str, &str, &str, &str)] = &[
    ("ui_open", "ui.open", "Open Page", "Open a page in the running app: a page id (customer-list), page:key (customer-card:10000; multi-field keys joined by '|'), or list>card:key to open a card over its list."),
    ("ui_state", "ui.state", "UI State", "The pages open in the running app (page stack, keys, edit mode, open dialog, notice)."),
    ("ui_screenshot", "ui.screenshot", "Screenshot", "Save a PNG screenshot of the running app's window to 'path' (default: erpcraft-screenshot.png in the temp directory) and return it."),
];

/// What a tool runs.
#[derive(Clone, Debug)]
enum ToolKind {
    /// `command_run`: any command by id.
    RunAny,
    /// One engine command.
    Command(String),
    /// A control-protocol UI method of the running app.
    Ui(&'static str),
}

/// One MCP tool: an engine command, the generic `command_run`, or a `ui_*` tool.
#[derive(Clone, Debug)]
struct Tool {
    name: String,
    kind: ToolKind,
    title: String,
    description: String,
    schema: Value,
    read_only: bool,
}

impl Tool {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.schema,
            "annotations": {"title": self.title, "readOnlyHint": self.read_only, "destructiveHint": !self.read_only, "openWorldHint": false},
        })
    }
}

/// The MCP tool name of a command id: `sales.document.post` → `sales_document_post`.
pub fn tool_name(command_id: &str) -> String {
    command_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

pub struct Server {
    backend: Box<dyn Backend>,
    protocol: String,
    tools: Option<Vec<Tool>>,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, protocol: PROTOCOL_VERSIONS.first().copied().unwrap_or("2025-06-18").to_string(), tools: None }
    }

    /// A server on an in-process session.
    pub fn headless(session: erpcraft_engine::Session) -> Self {
        Self::new(Box::new(Headless::new(session)))
    }

    pub fn backend(&self) -> &dyn Backend {
        self.backend.as_ref()
    }

    /// The protocol version agreed in `initialize`.
    pub fn protocol_version(&self) -> &str {
        &self.protocol
    }

    /// Serves newline-delimited JSON-RPC until the input ends. Only protocol messages are written to
    /// `output`.
    pub fn serve<R: BufRead, W: Write>(&mut self, mut input: R, mut output: W) -> io::Result<()> {
        while let Some(line) = jsonrpc::read_line_bounded(&mut input, MAX_LINE_BYTES)? {
            let reply = match line {
                Line::Text(text) => self.handle_line(&text),
                Line::TooLong => Some(error_line(&RpcError::new(INVALID_REQUEST, format!("message longer than {MAX_LINE_BYTES} bytes")))),
                Line::NotUtf8 => Some(error_line(&RpcError::new(PARSE_ERROR, "Parse error: message is not valid UTF-8"))),
            };
            if let Some(reply) = reply {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Handles one message line; `None` when nothing is to be sent back (notifications, blank lines).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle_message(&msg).map(|v| to_line(&v)),
            Err(e) => Some(error_line(&RpcError::new(PARSE_ERROR, format!("Parse error: {e}")))),
        }
    }

    /// Handles one parsed message.
    pub fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            let why = if msg.is_array() { "batch requests are not supported" } else { "a request must be a JSON object" };
            return Some(jsonrpc::failure(&Value::Null, &RpcError::new(INVALID_REQUEST, format!("Invalid Request: {why}"))));
        };
        let id = obj.get("id");
        let valid_id = matches!(id, Some(Value::String(_) | Value::Number(_)));
        let reply_id = if valid_id { id.cloned().unwrap_or(Value::Null) } else { Value::Null };
        let invalid = |why: &str| Some(jsonrpc::failure(&reply_id, &RpcError::new(INVALID_REQUEST, format!("Invalid Request: {why}"))));

        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return invalid("jsonrpc must be \"2.0\"");
        }
        let Some(method_value) = obj.get("method") else {
            // A response to a request we sent (we send none) or garbage: never answer a response.
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            return invalid("missing method");
        };
        let Some(method) = method_value.as_str() else {
            return invalid("method must be a string");
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        if !(params.is_null() || params.is_object() || params.is_array()) {
            return invalid("params must be an object or array");
        }
        match id {
            // Notifications (notifications/initialized, notifications/cancelled, …) get no reply,
            // and a synchronous server has finished every request before it reads the next line,
            // so there is nothing to cancel.
            None => None,
            Some(_) if !valid_id => invalid("id must be a string or a number"),
            Some(_) => Some(match self.request(method, &params) {
                Ok(result) => jsonrpc::success(&reply_id, result),
                Err(e) => jsonrpc::failure(&reply_id, &e),
            }),
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.tools().iter().map(Tool::to_json).collect::<Vec<_>>()})),
            "tools/call" => self.call_tool(params),
            "resources/list" => Ok(resources::list()),
            "resources/templates/list" => Ok(resources::templates()),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).ok_or_else(|| RpcError::invalid_params("missing 'uri'"))?;
                resources::read(self.backend.as_mut(), uri)
            }
            "prompts/list" => Ok(prompts::list()),
            "prompts/get" => prompts::get(params),
            "logging/setLevel" => Ok(json!({})),
            _ => Err(RpcError::new(METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let mut instructions = INSTRUCTIONS.to_string();
        if self.backend.has_ui() {
            instructions.push_str(UI_INSTRUCTIONS);
        }
        let chosen = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).or(PROTOCOL_VERSIONS.first()).copied().unwrap_or("2025-06-18");
        self.protocol = chosen.to_string();
        json!({
            "protocolVersion": chosen,
            "capabilities": {
                "tools": {"listChanged": false},
                "resources": {"subscribe": false, "listChanged": false},
                "prompts": {"listChanged": false},
            },
            "serverInfo": {"name": SERVER_NAME, "title": "ERPCraft", "version": env!("CARGO_PKG_VERSION")},
            "instructions": instructions,
        })
    }

    fn tools(&mut self) -> &[Tool] {
        if self.tools.is_none() {
            let has_ui = self.backend.has_ui();
            self.tools = Some(build_tools(&self.backend.commands(), has_ui));
        }
        self.tools.as_deref().unwrap_or(&[])
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(Value::as_str).ok_or_else(|| RpcError::invalid_params("missing tool 'name'"))?;
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => return Err(RpcError::invalid_params("tool 'arguments' must be an object")),
        };
        let tool = self.tools().iter().find(|t| t.name == name).cloned().ok_or_else(|| RpcError::invalid_params(format!("Unknown tool: {name}")))?;
        let (command, cmd_params) = match &tool.kind {
            ToolKind::Command(c) => (c.clone(), args),
            ToolKind::RunAny => match args.get("command").and_then(Value::as_str) {
                Some(c) => (c.to_string(), args.get("params").cloned().unwrap_or(Value::Null)),
                None => return Ok(tool_error("command_run needs a 'command' (a command id such as record.list)")),
            },
            ToolKind::Ui(method) => return Ok(self.call_ui(method, args)),
        };
        Ok(match self.backend.run(&command, &cmd_params) {
            Ok(result) => self.tool_success(&result),
            Err(e) => tool_error(&format!("{} error: {}", e.kind_name(), e.message)),
        })
    }

    fn call_ui(&mut self, method: &str, mut args: Value) -> Value {
        if method == "ui.screenshot" && args.get("path").and_then(Value::as_str).is_none_or(|p| p.trim().is_empty()) {
            let default = std::env::temp_dir().join("erpcraft-screenshot.png");
            args["path"] = json!(default.to_string_lossy());
        }
        match self.backend.ui(method, &args) {
            Ok(result) => {
                let mut out = self.tool_success(&result);
                if method == "ui.screenshot"
                    && let Some(image) = result.get("path").and_then(Value::as_str).and_then(png_base64)
                    && let Some(content) = out.get_mut("content").and_then(Value::as_array_mut)
                {
                    content.push(json!({"type": "image", "data": image, "mimeType": "image/png"}));
                }
                out
            }
            Err(e) => tool_error(&format!("{} error: {}", e.kind_name(), e.message)),
        }
    }

    fn tool_success(&self, result: &Value) -> Value {
        let text = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
        let mut out = json!({"content": [{"type": "text", "text": text}], "isError": false});
        // structuredContent arrived in 2025-06-18; older clients only read `content`.
        if result.is_object() && self.protocol.as_str() >= "2025-06-18" {
            out["structuredContent"] = result.clone();
        }
        out
    }
}

fn tool_error(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

fn build_tools(commands: &[CommandInfo], has_ui: bool) -> Vec<Tool> {
    let mut tools = vec![Tool {
        name: RUN_TOOL.to_string(),
        kind: ToolKind::RunAny,
        title: "Run Command".to_string(),
        description: "Run any ERPCraft command by id (see command_list), e.g. {\"command\": \"record.list\", \"params\": {\"table\": \"Customer\"}}.".to_string(),
        schema: json!({"type": "object", "properties": {
            "command": {"type": "string", "description": "command id, e.g. sales.document.post"},
            "params": {"type": "object", "description": "the command's parameters"}}, "required": ["command"]}),
        read_only: false,
    }];
    for c in commands {
        let name = tool_name(&c.id);
        if name.is_empty() || name.len() > 64 || tools.iter().any(|t| t.name == name) {
            continue;
        }
        let mut schema = if c.params.is_object() { c.params.clone() } else { json!({}) };
        if schema.get("type").is_none() {
            schema["type"] = json!("object");
        }
        let description = if c.group.is_empty() { format!("{} (command {})", c.summary, c.id) } else { format!("{} [{}; command {}]", c.summary, c.group, c.id) };
        tools.push(Tool { name, kind: ToolKind::Command(c.id.clone()), title: c.label.clone(), description, schema, read_only: !c.mutates });
    }
    if has_ui {
        for (name, method, title, description) in UI_TOOLS {
            let schema = match *method {
                "ui.open" => json!({"type": "object", "properties": {
                    "spec": {"type": "string", "description": "page, page:key or list>card:key, e.g. customer-card:10000"}}, "required": ["spec"]}),
                "ui.screenshot" => json!({"type": "object", "properties": {
                    "path": {"type": "string", "description": "where to write the PNG (absolute path recommended)"}}}),
                _ => json!({"type": "object", "properties": {}}),
            };
            let read_only = *method == "ui.state";
            tools.retain(|t| t.name != *name);
            tools.push(Tool { name: (*name).to_string(), kind: ToolKind::Ui(method), title: (*title).to_string(), description: (*description).to_string(), schema, read_only });
        }
    }
    tools
}

/// Largest screenshot attached to a tool result as an image.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// The PNG at `path`, base64-encoded, if it is readable and small enough.
fn png_base64(path: &str) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    if len == 0 || len > MAX_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    bytes.starts_with(b"\x89PNG").then(|| base64(&bytes))
}

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn to_line(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| String::from(r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#))
}

fn error_line(e: &RpcError) -> String {
    to_line(&jsonrpc::failure(&Value::Null, e))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        for (input, expected) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn ui_tools_only_with_a_window() {
        assert!(!build_tools(&[], false).iter().any(|t| t.name.starts_with("ui_")));
        let tools = build_tools(&[], true);
        for name in ["ui_open", "ui_state", "ui_screenshot"] {
            assert!(tools.iter().any(|t| t.name == name), "{name}");
        }
    }
}
