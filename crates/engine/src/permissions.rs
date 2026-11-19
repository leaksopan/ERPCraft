//! Permission checks, after BC: a user runs a command when one of their permission sets grants
//! Execute on it (or on `*`); the generic record commands also need Read / Insert / Modify /
//! Delete on the table. While the User table is empty everyone has SUPER, as in a new BC tenant.

use erpcraft_meta::Metadata;
use serde_json::Value as Json;

use crate::db::Database;
use crate::error::{EngineError, Result};
use crate::schema::tables::{ACCESS_CONTROL, PERMISSION, USER};

/// Commands every enabled user may run: they only describe the application or the session.
const ALWAYS: &[&str] = &[
    "command.list",
    "table.list",
    "table.describe",
    "page.list",
    "page.describe",
    "rolecenter.get",
    "search",
    "company.info",
    "session.workdate",
];

#[derive(Clone, Copy)]
enum Access {
    Read,
    Insert,
    Modify,
    Delete,
}

impl Access {
    fn field(self) -> &'static str {
        match self {
            Access::Read => "Read Permission",
            Access::Insert => "Insert Permission",
            Access::Modify => "Modify Permission",
            Access::Delete => "Delete Permission",
        }
    }
    fn verb(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Insert => "insert into",
            Access::Modify => "modify",
            Access::Delete => "delete from",
        }
    }
}

pub fn check(db: &Database, meta: &Metadata, user: &str, command: &str, params: &Json) -> Result<()> {
    if db.count(USER) == 0 {
        return Ok(());
    }
    let user = user.to_uppercase();
    let Some(u) = db.get_code(USER, &user) else {
        return Err(EngineError::permission(format!(
            "You do not have access to this company. User {user} has not been set up in the Users table."
        )));
    };
    if u.str("State") == "Disabled" {
        return Err(EngineError::permission(format!("User {user} is disabled.")));
    }
    if ALWAYS.contains(&command) {
        return Ok(());
    }
    let roles: Vec<String> = db.rows(ACCESS_CONTROL).filter(|r| r.str("User Name") == user).map(|r| r.str("Role ID").to_string()).collect();
    let perms: Vec<&erpcraft_meta::Record> = db.rows(PERMISSION).filter(|p| roles.iter().any(|r| r == p.str("Role ID"))).collect();
    let grants = |object_type: &str, name: &str, flag: &str| {
        perms.iter().any(|p| {
            p.str("Object Type") == object_type
                && (p.str("Object Name") == "*" || p.str("Object Name").eq_ignore_ascii_case(name))
                && p.get(flag).as_bool()
        })
    };
    if !grants("Command", command, "Execute Permission") {
        return Err(EngineError::permission(format!(
            "You do not have permission to run '{command}'. Ask your administrator for a permission set that includes it."
        )));
    }
    let access = match command {
        "record.list" | "record.get" | "record.lookup" => Some(Access::Read),
        "record.new" => Some(Access::Insert),
        "record.validate" | "record.modify" => Some(Access::Modify),
        "record.delete" => Some(Access::Delete),
        _ => None,
    };
    if let Some(access) = access
        && let Some(table) = table_of(meta, params)
        && !grants("Table Data", &table, access.field())
    {
        return Err(EngineError::permission(format!("You do not have permission to {} the {table} table.", access.verb())));
    }
    Ok(())
}

fn table_of(meta: &Metadata, params: &Json) -> Option<String> {
    if let Some(p) = params.get("page").and_then(Json::as_str) {
        return meta.page(p).and_then(|p| p.source_table.clone());
    }
    let t = params.get("table").and_then(Json::as_str)?;
    Some(meta.table(t).map_or_else(|| t.to_string(), |t| t.name.clone()))
}
