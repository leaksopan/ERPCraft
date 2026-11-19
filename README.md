# ERPCraft

A Business Central-style ERP written in pure Rust, with a native desktop app, a native CLI and a native MCP server on one engine.

ERPCraft follows the shape of [VectorCraft](https://github.com/storytold/vectorcraft): a headless engine where **everything is a command**, plus thin clients on top of it. Tables, pages and processes follow the Business Central W1 Base Application ([BCApps](https://github.com/microsoft/BCApps)): the same table names and field names, BC object ids, and the same document → posting → ledger flow.

## Workspace

| Crate | What it is |
| --- | --- |
| `crates/meta` | The object model: tables, fields, pages (List, Card, Document, ListPart), role centers, typed values. |
| `crates/engine` | The base application: schema and pages, triggers (`OnInsert`, `OnValidate`, `OnDelete`), FlowFields, number series, sales and purchase posting, payments, and the command registry. `Session::run` is the single entry point. |
| `crates/mcp` | MCP server, JSON-RPC 2.0 over stdio, hand-written. Every engine command is an MCP tool, and tables, pages and records are resources. |
| `crates/ui-egui` | The egui client in the BC web-client layout: app bar, role center (navigation, cues), list pages (action bar, grid, FactBox), card and document pages (FastTabs, lines, totals), and Tell Me (Alt+Q). |
| `apps/erpcraft` | Desktop app. |
| `apps/erpcraft-cli` | `erpcraft-cli`: run any command headless, inspect metadata, or serve MCP. |

## Quick start

```sh
cargo run -p erpcraft-cli -- init --demo          # creates company.erpcraft.json (Auto Craft demo company)
cargo run -p erpcraft-cli -- list sales-order-list
cargo run -p erpcraft-cli -- run report.trial_balance
cargo run -p erpcraft                              # desktop app on the same data file
cargo run -p erpcraft-cli -- mcp                   # MCP server on stdio for agents
```

`--data <file>` (or `ERPCRAFT_DATA`) picks the company file. The desktop app also takes `--open page[:key]`, e.g. `--open "sales-order:Order|S-ORD1002"`.

On Windows without the MSVC build tools, use the `stable-x86_64-pc-windows-gnullvm` toolchain with llvm-mingw.

## What works today

- **Master data:** customers, vendors, items, G/L accounts, bank accounts, and the setup tables (posting groups, VAT and General Posting Setup, number series, payment terms).
- **Sales and purchase documents:** quote, order, invoice and credit memo; release and reopen; Make Order; statistics.
- **Posting:** to the G/L, customer, vendor, item and bank ledgers, with partial shipment and receipt. A failed command rolls back completely.
- **Payments:** customer and vendor payments, applied to open invoices.
- **Reports:** trial balance, customer and vendor statistics, item availability.
- **Demo company:** "Auto Craft", an Indonesian auto-parts and workshop business in IDR with 11% PPN.

See [`docs/cli.md`](docs/cli.md) and [`docs/mcp.md`](docs/mcp.md).

## License

MIT OR Apache-2.0.
