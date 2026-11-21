//! ERPCraft engine: the base application behind one command registry.
//!
//! - [`schema`]: tables, pages and role centers (metadata, BC W1 names and ids).
//! - [`records`] + `triggers`: validation, `OnInsert` / `OnValidate` / `OnDelete`.
//! - [`posting`]: sales / purchase posting and payments into G/L, customer, vendor, item and bank
//!   ledgers.
//! - [`commands`]: every action as a command; [`Session::run`] executes one transactionally.
//!
//! Clients (the egui app, `erpcraft-cli`, the MCP server) only render metadata and call commands,
//! so a behaviour exists in all of them as soon as it exists here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

mod calc;
pub mod commands;
mod ctx;
pub mod dateformula;
pub mod db;
mod demo;
mod documents;
pub mod bc;
pub mod approvals;
pub mod dimensions;
pub mod doc_tools;
pub mod item_tools;
pub mod navigate;
mod error;
pub mod fa;
pub mod item_journal;
pub mod journal;
pub mod ledger;
pub mod filter;
mod noseries;
mod permissions;
pub mod print;
pub mod reports;
pub mod reverse;
pub mod posting;
pub mod records;
pub mod schema;
mod session;
mod side;
pub mod store;
mod triggers;

pub use commands::CommandDef;
pub use ctx::Ctx;
pub use error::{EngineError, ErrorKind, Result};
pub use session::Session;

/// The meta crate, re-exported so clients depend on one engine version.
pub use erpcraft_meta as meta;
