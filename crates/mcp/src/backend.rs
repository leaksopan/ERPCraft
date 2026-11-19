//! Where commands run. The MCP server only speaks to a [`Backend`], so the same protocol layer can
//! drive a headless session on a data file or database ([`Headless`]) or the running desktop app
//! over its loopback control port ([`Remote`], see docs/control.md).

use std::io::{BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use erpcraft_engine::{EngineError, Session, commands};
use serde_json::{Value, json};

use crate::control;
use crate::jsonrpc::{self, Line};

/// One engine command as the server advertises it (an MCP tool).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandInfo {
    pub id: String,
    pub label: String,
    pub group: String,
    pub summary: String,
    pub mutates: bool,
    /// JSON Schema object of the parameters.
    pub params: Value,
}

impl CommandInfo {
    /// Reads one entry of `command.list`'s `commands` array.
    pub fn from_json(j: &Value) -> Option<Self> {
        let text = |k: &str| j.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            id: text("id")?,
            label: text("label").unwrap_or_default(),
            group: text("group").unwrap_or_default(),
            summary: text("summary").unwrap_or_default(),
            mutates: j.get("mutates").and_then(Value::as_bool).unwrap_or(true),
            params: j.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({"type": "object"})),
        })
    }
}

/// Something that runs ERPCraft commands.
pub trait Backend {
    /// Runs one command (the engine's single entry point, `Session::run`).
    fn run(&mut self, command: &str, params: &Value) -> Result<Value, EngineError>;

    /// Every command the backend offers. The default asks the backend itself (`command.list`), which
    /// is what a remote backend does; [`Headless`] reads the registry directly.
    fn commands(&mut self) -> Vec<CommandInfo> {
        self.run("command.list", &json!({}))
            .ok()
            .and_then(|r| r.get("commands").and_then(Value::as_array).map(|a| a.iter().filter_map(CommandInfo::from_json).collect()))
            .unwrap_or_default()
    }

    /// A short description for logs ("headless D:\\data\\company.erpcraft.json").
    fn describe(&self) -> String;

    /// Whether there is a window behind the backend (the running desktop app), so the `ui_*` tools
    /// make sense.
    fn has_ui(&self) -> bool {
        false
    }

    /// Runs a UI method of the control protocol (`ui.open`, `ui.state`, `ui.screenshot`, …).
    fn ui(&mut self, method: &str, _params: &Value) -> Result<Value, EngineError> {
        Err(EngineError::not_found(format!("{method} needs the running desktop app (erpcraft --control, then erpcraft-cli mcp --connect)")))
    }
}

/// Runs commands in-process on a [`Session`] (a data file or an in-memory company).
pub struct Headless {
    session: Session,
}

impl Headless {
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// Opens (or starts) the company data file at `path`.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        Session::open(path).map(Self::new)
    }

    pub fn in_memory() -> Self {
        Self::new(Session::in_memory())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }
}

impl Backend for Headless {
    fn run(&mut self, command: &str, params: &Value) -> Result<Value, EngineError> {
        self.session.run(command, params)
    }

    fn commands(&mut self) -> Vec<CommandInfo> {
        commands::all()
            .iter()
            .map(|c| CommandInfo {
                id: c.id.to_string(),
                label: c.label.to_string(),
                group: c.group.to_string(),
                summary: c.summary.to_string(),
                mutates: c.mutates,
                params: (c.params)(),
            })
            .collect()
    }

    fn describe(&self) -> String {
        format!("headless {}", self.session.source())
    }
}

/// How long to wait for the app to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for one reply (a long posting on a slow database still fits).
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// Drives the running desktop app (`erpcraft --control [port]`) over its loopback control port:
/// every command runs in the app's session, and the app shows the effect.
pub struct Remote {
    addr: String,
    conn: Option<Connection>,
    next_id: u64,
}

impl Remote {
    /// Connects to `addr` (`127.0.0.1:7979`, or just a port).
    pub fn connect(addr: &str) -> Result<Self, EngineError> {
        let addr = control::normalize_addr(addr).ok_or_else(|| EngineError::invalid(format!("'{addr}' is not a control address (host:port or a port)")))?;
        let conn = Self::open(&addr)?;
        Ok(Self { addr, conn: Some(conn), next_id: 1 })
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn open(addr: &str) -> Result<Connection, EngineError> {
        let unreachable = |why: String| EngineError::io(format!("cannot reach the ERPCraft app at {addr}: {why} (start it with: erpcraft --control)"));
        let addrs: Vec<_> = addr.to_socket_addrs().map_err(|e| unreachable(e.to_string()))?.collect();
        let mut last = String::from("no address");
        for a in addrs {
            match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| unreachable(e.to_string()))?;
                    let reader = stream.try_clone().map_err(|e| unreachable(e.to_string()))?;
                    return Ok(Connection { reader: BufReader::new(reader), writer: stream });
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(unreachable(last))
    }

    /// Sends one control request and waits for its reply.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, EngineError> {
        // A connection that broke earlier is re-opened before sending, never while a request may
        // already have run (that would run it twice).
        let mut conn = match self.conn.take() {
            Some(c) => c,
            None => Self::open(&self.addr)?,
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        match Self::exchange(&mut conn, id, method, params) {
            Ok(reply) => {
                self.conn = Some(conn);
                control::result_of(&reply)
            }
            Err(e) => Err(EngineError::io(format!("lost the connection to the ERPCraft app at {}: {e}", self.addr))),
        }
    }

    fn exchange(conn: &mut Connection, id: u64, method: &str, params: &Value) -> Result<Value, String> {
        let mut line = control::request_line(id, method, params);
        line.push('\n');
        conn.writer.write_all(line.as_bytes()).and_then(|()| conn.writer.flush()).map_err(|e| e.to_string())?;
        loop {
            let text = match jsonrpc::read_line_bounded(&mut conn.reader, control::MAX_LINE_BYTES).map_err(|e| e.to_string())? {
                None => return Err("the app closed the connection".to_string()),
                Some(Line::Text(t)) => t,
                Some(Line::TooLong) => return Err(format!("reply longer than {} bytes", control::MAX_LINE_BYTES)),
                Some(Line::NotUtf8) => return Err("reply is not valid UTF-8".to_string()),
            };
            if text.trim().is_empty() {
                continue;
            }
            let reply: Value = serde_json::from_str(&text).map_err(|e| format!("reply is not valid JSON: {e}"))?;
            // Replies to other ids (none are expected on a private connection) are skipped.
            if reply.get("id").and_then(Value::as_u64) == Some(id) || reply.get("id").is_some_and(Value::is_null) {
                return Ok(reply);
            }
        }
    }
}

impl Backend for Remote {
    fn run(&mut self, command: &str, params: &Value) -> Result<Value, EngineError> {
        let params = if params.is_null() { json!({}) } else { params.clone() };
        self.call("engine.run", &json!({"command": command, "params": params}))
    }

    fn commands(&mut self) -> Vec<CommandInfo> {
        self.call("engine.commands", &json!({}))
            .ok()
            .and_then(|r| r.get("commands").and_then(Value::as_array).map(|a| a.iter().filter_map(CommandInfo::from_json).collect()))
            .unwrap_or_default()
    }

    fn describe(&self) -> String {
        format!("the ERPCraft app at {}", self.addr)
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn ui(&mut self, method: &str, params: &Value) -> Result<Value, EngineError> {
        self.call(method, params)
    }
}
