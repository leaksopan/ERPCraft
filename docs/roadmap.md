# ERPCraft Roadmap

ERPCraft is a clean-room, pure-Rust ERP application designed for the Craft Apps ecosystem (`storytold/craft-libs` + `storytold/egui`).

## Guiding principles

- **Clean room.** All schemas, captions, tooltips, business logic, and tests are original work authored from accounting and ERP specifications. No code, strings, object IDs, or assets are copied or generated from proprietary ERP products.
- **Shared Craft infrastructure.** Reusable platform, file-format, printing, localization, and web runtime layers come from [`storytold/craft-libs`](https://github.com/storytold/craft-libs).
- **Everything is a command.** Every user action goes through `Session::run` in `crates/engine`, so desktop, web, CLI, and confined MCP stay in lockstep.
- **Never crash & confined automation.** `unsafe_code = "forbid"`, no panics outside tests, and full RFC-0015 confined file access for agent entry points.

## Milestones

### Craft Ecosystem Integration (Current)
- [x] Pin `storytold/craft-libs` (single `main` rev) and `storytold/egui` + `storytold/winit` patches
- [x] `craft-atomic` for crash-safe company & PDF file writes
- [x] `craft-dialogs`, `craft-window`, and `craft-settings` in desktop app
- [x] `craft-web-api`, `craft-web-host`, and `craft-platform::browser` PDF download in WASM build
- [x] RFC-0015 confined file access (`--automation-read-root`, `--automation-write-root`) + escape tests
- [x] Clean-room schema separation (removed external code generation and parity tooling)
- [x] Governance & legal files (`AGENTS.md`, `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`, `deny.toml`)
- [ ] `craft-xlsx` / `craft-csv` for Excel & CSV export/import (Financial Reports, Budgets, Data Packages)
- [ ] `craft-print` for direct OS print spooler submission
- [ ] `craft-i18n` for multi-language localization (EN + ID)
- [ ] `craft-keymap`, `craft-menu`, `craft-quit`, and `craft-update`
- [ ] Full keyboard accessibility pass (RFC-0014) and `storytold/craft-common-actions` CI

### ERP Functional Modules
- [x] **Core Finance:** G/L, journals, recurring journals, financial reports, fiscal periods & closing, currencies, VAT statements, bank reconciliation, reminders & finance charges, dimensions
- [x] **Core Sales & Purchasing:** quotes, orders, invoices, credit memos, partial ship/receive/invoice, payments & applications, approvals, comments, attachments, PDF printing
- [x] **Core Inventory:** locations, item & reclass journals, transfers, lot/serial tracking & info cards, reservations & order promising, item charges, SKUs, variants/UOM/catalog items, physical inventory orders/recordings, costing (Average/FIFO/LIFO/Standard, revaluation, standard cost worksheet), requisition worksheet
- [x] **Core Fixed Assets:** asset cards, depreciation books, acquisition, straight-line depreciation, disposal
- [ ] **Inventory completion:** item application entries, item tracing, document line tracking, inventory receipts/shipments, inventory periods, item budgets & analysis reports
- [ ] **Sales & Purchasing completion:** return orders, blanket orders, prepayments, document archiving, invoice cancel/correct, price lists, incoming documents
- [ ] **Warehouse:** bins, zones, warehouse receipts/shipments, put-aways, picks, movements
- [ ] **Assembly & Manufacturing:** BOMs, assembly orders, routings, work centers, production orders, MRP planning worksheet
- [ ] **Service & Projects:** service items/contracts/orders, jobs/projects, time sheets, WIP
- [ ] **CRM & HR:** contacts, opportunities, campaigns, employees, absences, expense ledger
