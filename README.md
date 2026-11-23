# ERPCraft

An open-source, clean-room ERP application written in pure Rust for the Craft suite, with a native desktop app, a WebAssembly web build, a headless CLI, and a confined MCP server over one transactional engine.

Built on [`storytold/craft-libs`](https://github.com/storytold/craft-libs) and [`storytold/egui`](https://github.com/storytold/egui) following the Craft architecture: **everything is a command**, zero `unsafe` (`unsafe_code = "forbid"`), no panics in production code, and RFC-0015 confined filesystem access for automation.

## Workspace

| Crate | What it is |
| --- | --- |
| `crates/meta` | Metadata object model: tables, fields, computed fields, pages (List, Card, Document, ListPart), role centers, typed values, and filter expressions. |
| `crates/engine` | Transactional business engine: accounting, sales, purchasing, inventory, fixed assets, PostgreSQL and atomic JSON storage (`craft-atomic`), permissions, triggers, posting routines, PDF authoring, RFC-0015 confinement, and the command registry (`Session::run`). |
| `crates/mcp` | JSON-RPC 2.0 MCP server over stdio and loopback control protocol. Every registered engine command is exposed as an MCP tool. |
| `crates/ui-egui` | Thin immediate-mode UI rendering engine metadata: workspace home, list grids with filtering and column picker, cards and documents with collapsible sections, editable subforms, details pane, and command search (`Alt+Q`). |
| `apps/erpcraft` | Native desktop app using `craft-window`, `craft-dialogs`, and `craft-settings`. |
| `apps/erpcraft-web` | WebAssembly build using `craft-web-api` (`window.erpcraft`), `craft-web-host`, and `craft-platform::browser` for PDF downloads ([`docs/web.md`](docs/web.md)). |
| `apps/erpcraft-cli` | Headless CLI and RFC-0015 confined MCP server (`--automation-read-root`, `--automation-write-root`). |

## Quick start

```sh
cargo run -p erpcraft-cli -- init --demo          # creates company.erpcraft.json (demo company)
cargo run -p erpcraft-cli -- list sales-order-list
cargo run -p erpcraft-cli -- run report.trial_balance
cargo run -p erpcraft                              # desktop app on the same data file
cargo run -p erpcraft-cli -- mcp --automation-read-root . --automation-write-root .
```

**Storage:** Pass `--db postgres://user:pass@localhost:5432/erpcraft` (or set `ERPCRAFT_DATABASE_URL`) for multi-user PostgreSQL storage with optimistic concurrency ([`docs/storage.md`](docs/storage.md)). Without a database URL, `--data <file>` (or `ERPCRAFT_DATA`) stores the company atomically in a JSON file via `craft-atomic`.

## Security & Standards

- Follows **RFC-0015** confined file access: `erpcraft-cli mcp` refuses to start without `--automation-read-root` and `--automation-write-root` and blocks all reads/writes outside those canonicalized roots.
- Zero `unsafe` code (`#![forbid(unsafe_code)]`) and panics denied across all production crates.
- Clean-room implementation; see [`ATTRIBUTION.md`](ATTRIBUTION.md) and [`AGENTS.md`](AGENTS.md).

## What works today

- **Finance & Accounting:** chart of accounts, general/cash receipt/payment/recurring journals, financial reports, budgets, accounting periods & year-end close, multi-currency exchange rates, VAT statements & settlement, bank account reconciliation, reminders & finance charge memos, dimensions, and transaction reversal.
- **Sales & Purchasing:** quotes, orders, invoices, credit memos, partial ship/receive and invoice posting, posted shipments/receipts/invoices/credit memos, payment application & discounts, approvals, comments, and document attachments.
- **Inventory:** locations, item & reclassification journals, transfer orders (in-transit & direct), item tracking (lot & serial numbers, expiration rules, lot/serial info cards), reservations & order promising, item charges, stockkeeping units, item variants/UOM/references/substitutions/catalog items, physical inventory orders & recordings, requisition worksheet, and costing (Average, FIFO, LIFO, Standard, revaluation journal, standard cost worksheet).
- **Fixed Assets:** asset cards, depreciation books, acquisition, straight-line depreciation, and disposal.
- **Printing & Web:** built-in pure-Rust PDF generator for documents (opens natively on desktop, downloads via `craft-platform::browser` on WASM), plus full WebAssembly build with `window.erpcraft` (`craft-web-api`) and Craft host bridge (`craft-web-host`).

## Roadmap

See [`docs/roadmap.md`](docs/roadmap.md) and [`docs/rfc-storytold-erpcraft.md`](docs/rfc-storytold-erpcraft.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
