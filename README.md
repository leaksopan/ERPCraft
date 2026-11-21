# ERPCraft

A Business Central-style ERP written in pure Rust, with a native desktop app, a native CLI and a native MCP server on one engine.

ERPCraft follows the shape of [VectorCraft](https://github.com/storytold/vectorcraft): a headless engine where **everything is a command**, plus thin clients on top of it. Tables, pages and processes follow the Business Central W1 Base Application ([BCApps](https://github.com/microsoft/BCApps)): the same table names and field names, BC object ids, and the same document → posting → ledger flow.

## Workspace

| Crate | What it is |
| --- | --- |
| `crates/meta` | The object model: tables, fields, pages (List, Card, Document, ListPart), role centers, typed values. |
| `crates/engine` | The base application: schema and pages, storage (PostgreSQL / JSON), permissions, triggers (`OnInsert`, `OnValidate`, `OnDelete`), FlowFields, number series, sales and purchase posting, payments, and the command registry. `Session::run` is the single entry point. |
| `crates/mcp` | MCP server, JSON-RPC 2.0 over stdio, hand-written. Every engine command is an MCP tool, and tables, pages and records are resources. |
| `crates/ui-egui` | The egui client in the BC web-client layout: app bar, role center (navigation, cues), list pages (action bar, grid, FactBox), card and document pages (FastTabs, lines, totals), and Tell Me (Alt+Q). |
| `apps/erpcraft` | Desktop app. |
| `apps/erpcraft-web` | The same egui app compiled to WebAssembly, on an in-memory demo company ([`docs/web.md`](docs/web.md)). |
| `apps/erpcraft-cli` | `erpcraft-cli`: run any command headless, inspect metadata, or serve MCP. |

## Quick start

```sh
cargo run -p erpcraft-cli -- init --demo          # creates company.erpcraft.json (Auto Craft demo company)
cargo run -p erpcraft-cli -- list sales-order-list
cargo run -p erpcraft-cli -- run report.trial_balance
cargo run -p erpcraft                              # desktop app on the same data file
cargo run -p erpcraft-cli -- mcp                   # MCP server on stdio for agents
```

**PostgreSQL** is the production store: pass `--db postgres://user:pass@localhost:5432/erpcraft` or set `ERPCRAFT_DATABASE_URL`. Every BC table becomes a real PostgreSQL table, and several users and processes can work at once, with BC's "Another user has modified the record" protection ([`docs/storage.md`](docs/storage.md)). Without a database URL, `--data <file>` (or `ERPCRAFT_DATA`) uses a JSON company file. The desktop app also takes `--open page[:key]`, e.g. `--open "sales-order:Order|S-ORD1002"`.

On Windows without the MSVC build tools, use the `stable-x86_64-pc-windows-gnullvm` toolchain with llvm-mingw.

## Roadmap

The road to full Business Central parity, how it is measured and the to-do list: [`docs/roadmap.md`](docs/roadmap.md).

## What works today

- **Master data:** customers, vendors, items, G/L accounts, bank accounts, and the setup tables (posting groups, VAT and General Posting Setup, number series, payment terms).
- **Sales and purchase documents:** quote, order, invoice and credit memo; release and reopen; Make Order; statistics.
- **Posting:** to the G/L, customer, vendor, item and bank ledgers, with G/L registers and transaction numbers. Orders post Ship / Receive and Invoice separately (posted shipments and receipts) or together. A failed command rolls back completely.
- **Journals:** General, Cash Receipt and Payment journals with balancing per document, number series, Applies-to and a balancing bank account.
- **Applications:** detailed customer and vendor ledger entries, Apply / Unapply Entries, Reverse Transaction for journal postings.
- **Inventory:** locations on items, documents and ledgers; item journals for adjustments, transfers between locations and physical inventory (stock count); optional negative-stock guard ([`docs/inventory-and-fixed-assets.md`](docs/inventory-and-fixed-assets.md)).
- **Fixed assets:** cards with depreciation books; acquisition by purchase invoice, payment, grant (hibah) or opening balance; straight-line depreciation on BC's 360-day year; disposal with gain or loss.
- **Payments:** customer and vendor payments, applied to open invoices.
- **Reports:** Balance Sheet, Income Statement, Aged Accounts Receivable / Payable, Inventory Valuation, Fixed Asset Book Value, trial balance, customer and vendor statistics, item availability ([`docs/finance.md`](docs/finance.md)).
- **Printing:** sales and purchase documents, posted invoices, credit memos, shipments and receipts print to PDF (`document.print`, Ctrl+P), with a built-in PDF writer ([`docs/printing-and-lists.md`](docs/printing-and-lists.md)).
- **List pages:** BC's filter pane with filter expressions, sorting by column, choosing columns, and a calendar date picker on date fields.
- **Business Central connector:** test the connection, import and export customers, vendors and items (and import G/L accounts) through the BC API v2.0 with OAuth client credentials ([`docs/bc-connector.md`](docs/bc-connector.md)).
- **Users and permissions** after BC's User, Permission Set and Permission tables, with a work date saved per user.
- **Demo company:** "Auto Craft", an Indonesian auto-parts and workshop business in IDR with 11% PPN.

See [`docs/cli.md`](docs/cli.md) and [`docs/mcp.md`](docs/mcp.md).

## License

MIT OR Apache-2.0.
