# RFC-XXXX: Add ERPCraft to the Craft Apps suite

- Status: Draft (ready to open as a PR in `storytold/craft-contrib`)
- Comment period: 7 days (new application proposal per RFC-0000)

## Summary

Propose adding **ERPCraft** — a clean-room, pure-Rust Enterprise Resource Planning (ERP) application covering finance/accounting, sales, purchasing, inventory, and fixed assets — to the `storytold` Craft Apps ecosystem alongside GridCraft, WordCraft, and DeckCraft.

## Motivation

Small and mid-sized businesses need an open-source, fast, local-first and self-hostable ERP that runs natively on desktop, in the browser (WASM), and headlessly for automation without Electron, webviews, or proprietary vendor lock-in.

The Craft suite already provides office productivity apps (WordCraft, GridCraft, DeckCraft, PdfCraft) built on a shared pure-Rust architecture (`craft-libs` + `storytold/egui`). Adding ERPCraft extends the suite into business operations and accounting with natural interoperability across Craft apps (e.g. exporting financial reports and budgets via `craft-xlsx`/`craft-csv`, printing invoices via `craft-print`, and embedding via `craft-web-host`).

## Architecture & Craft Standards Compliance

1. **Pure Rust, thin UI, command-driven engine:**
   - `crates/meta` + `crates/engine`: transactional business engine where every user-visible action is a registered command executed through `Session::run`.
   - `crates/ui-egui`: immediate-mode UI rendering metadata with zero business logic.
   - `apps/erpcraft`: native desktop client using `craft-window`, `craft-dialogs`, and `craft-settings`.
   - `apps/erpcraft-cli`: headless CLI and MCP server.
   - `apps/erpcraft-web`: WebAssembly client using `craft-web-api` (`window.erpcraft`), `craft-web-host`, and `craft-platform::browser`.
2. **Safety & reliability:**
   - `#![forbid(unsafe_code)]` workspace-wide.
   - Clippy denies `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable` outside tests.
   - Crash-safe atomic saves via `craft-atomic`; optimistic concurrency on PostgreSQL.
3. **RFC-0015 confined file access:**
   - `erpcraft-cli mcp` refuses to start without `--automation-read-root` and `--automation-write-root`.
   - All command file reads/writes are canonicalized and checked against configured roots, failing closed on traversal, sibling prefixes, read-only root writes, and `--connect` bridge mode.
4. **Clean-room & licensing (`standards/clean-room.md`, `standards/licences.md`, `standards/naming.md`):**
   - Dual-licensed `MIT OR Apache-2.0`; all dependencies conform to `standards/licences.md` (`deny.toml`).
   - All schemas, UI labels, tooltips, posting logic, and PDF layouts are clean-room original implementations with neutral ERP terminology (no proprietary product branding, copied strings, or generated third-party code).
   - `AGENTS.md` adopts `standards/app-agents.md` verbatim; `ATTRIBUTION.md` and `NOTICE` are included.

## Shared `craft-libs` Integration Plan

- **Integrated now:** `craft-atomic`, `craft-dialogs`, `craft-platform` (`browser`), `craft-settings`, `craft-window` (`egui`), `craft-web-api`, `craft-web-host`, plus `storytold/egui` and `storytold/winit`.
- **Next:** `craft-xlsx` / `craft-csv` (financial report and budget import/export), `craft-print` (native OS printing), `craft-i18n` (translations), `craft-keymap`, `craft-menu`, `craft-update`, and `storytold/craft-common-actions` CI workflows.

## Open questions for review

1. Repository naming (`storytold/erpcraft`) and initial maintainer sponsorship from `craft-app-team`.
2. Whether any reusable accounting/ERP primitives (e.g. RFC-0015 path confinement once `craft-confine` lands, or general ledger / decimal formatting helpers) should be promoted into `craft-libs`.
3. Confirming clean git history preparation before transfer/import into `storytold`.
