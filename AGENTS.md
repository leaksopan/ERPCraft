# ERPCraft: instructions for agents

## Non-negotiables

- **Everything is a command.** User-visible behaviour is a command in `crates/engine/src/commands.rs`, with an id, a label, a JSON-schema for its params and a `run` function, plus tests. The UI, the CLI and MCP all go through `Session::run`. Never put business rules in a client.
- **Never crash.** Shipped code doesn't panic: clippy denies `unwrap`/`expect`/`panic!`/`todo!`/`unreachable!` outside tests. Treat files, MCP messages and command params as hostile input. `Session::run` catches panics and rolls back, but that is a last resort, not a substitute for `Result`.
- **Transactions.** A command runs on a copy of the database and replaces it only on success, so a failing command leaves no trace (the same as an AL write transaction).
- **BC parity.** Table, field and page names follow the Business Central W1 Base Application (BCApps), and object ids are kept in `object_id`. Error texts follow BC's wording where they can. Behaviour comes from the BCApps AL source.
- **Metadata drives the UI.** New tables and pages go into `crates/engine/src/schema/`; `Metadata::check` (run by the tests) catches typos.
- **The UI is thin.** egui panels render metadata and call commands. Colours come from `crates/ui-egui/src/theme.rs`.
- **Rust only.** No handwritten JS/TS and no webview shell. Following VectorCraft, the desktop app comes first and a wasm build of the same egui code comes later.

## Checks before committing

```sh
cargo build --workspace
cargo test --workspace
```
