//! ERPCraft's native MCP (Model Context Protocol) server.
//!
//! Hand-written JSON-RPC 2.0 over stdio, one message per line, synchronous. Every engine command is
//! an MCP tool (`sales.document.post` → `sales_document_post`), generated from the command registry
//! so new commands appear without changes here; `command_run` runs any command by id. Metadata and
//! records are also resources (`erpcraft://table/{name}`, `erpcraft://record/{table}/{key}`, …).
//!
//! The server talks to a [`Backend`]: [`Headless`] runs commands in-process on a data file. A
//! remote backend driving the running desktop app can implement the same trait.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

mod backend;
pub mod jsonrpc;
mod prompts;
pub mod resources;
mod server;

pub use backend::{Backend, CommandInfo, Headless};
pub use server::{INSTRUCTIONS, PROTOCOL_VERSIONS, SERVER_NAME, Server, tool_name};
