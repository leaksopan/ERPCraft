//! Where commands run. The MCP server only speaks to a [`Backend`], so the same protocol layer can
//! drive a headless session on a data file today ([`Headless`]) and, later, the running desktop app
//! over a loopback control port (a `Remote` backend implementing the same trait).

use std::path::Path;

use erpcraft_engine::{EngineError, Session, commands};
use serde_json::{Value, json};

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
        match self.session.path() {
            Some(p) => format!("headless {}", p.display()),
            None => "headless (in memory)".to_string(),
        }
    }
}
