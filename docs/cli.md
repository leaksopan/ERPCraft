# erpcraft-cli

`erpcraft-cli` runs ERPCraft headless. Every subcommand is a thin wrapper over an engine command
(`Session::run`), the same entry point the desktop app and the MCP server use, so anything you can
do in the app you can script. With `--connect` the same subcommands run in the running desktop app
instead ([control.md](control.md)).

```
erpcraft-cli [--db <url> | --data <file> | --connect [addr]] [--json] [--workdate <date>] <command> [args]
```

## Global options

| Option | Meaning |
| --- | --- |
| `--db <url>` | PostgreSQL company, `postgres://user:password@host[:port]/db`. Default: `$ERPCRAFT_DATABASE_URL`. Wins over `--data`. Printed sources hide the password. |
| `--data <file>` | Company data file. Default: `$ERPCRAFT_DATA`, else `./company.erpcraft.json`. A missing file is an empty company, saved on the first change. |
| `--connect [addr]` | Run in the running desktop app (`erpcraft --control`) instead of opening the company here. `addr` is `host:port` or a port; default `127.0.0.1:7979`. A word after `--connect` that is not an address is left alone (`--connect run …`); use `--connect=addr` to be explicit. Excludes `--db`, `--data` and `--workdate`. |
| `--json` | Print the raw JSON result instead of formatted text. |
| `--workdate <date>` | Work date (`YYYY-MM-DD`) for this invocation. Posting uses it as the default posting date. |
| `--automation-read-root <dir>` | Allowed directory for reading files (repeatable; mandatory for `mcp` per RFC-0015). |
| `--automation-write-root <dir>` | Allowed directory for writing files (repeatable; mandatory for `mcp` per RFC-0015). |
| `-h`, `--help` / `-V`, `--version` | Help / version. |

Global options may appear anywhere on the line. Use `--` to pass an argument that starts with `-`.

The company is chosen as everywhere in ERPCraft: `--db`, else `$ERPCRAFT_DATABASE_URL`, else
`--data`, else `$ERPCRAFT_DATA`, else `./company.erpcraft.json`.

## Commands

| Command | Engine command | Notes |
| --- | --- | --- |
| `init [--demo\|--empty] [--force]` | `company.init` | Demo company (Auto Craft) by default; `--force` replaces existing data. |
| `commands` | `command.list` | Grouped; `*` marks commands that change data. |
| `tables` | `table.list` | Name, object id, record count, primary key. |
| `pages` | `page.list` | Page id, object id, type, source table. |
| `describe <table-or-page>` | `page.describe` / `table.describe` | A page id (`customer-card`) is described as a page, anything else as a table. |
| `list <table\|page> [--filter Field=expr ...] [--search text] [--fields a,b] [--limit n] [--offset n]` | `record.list` | Aligned table. A list page id (`sales-order-list`) applies its view and its columns; for a table the columns of its list page are used, else the key and first fields. Numbers are right-aligned with thousands separators. |
| `get <table> <key> [--all]` | `record.get` | `Field ..... Value` lines in table field order; blank values hidden unless `--all`. Multi-field keys are joined by `\|`: `get "Sales Header" "Order\|S-ORD1001"`. |
| `run <command.id> [json \| @file.json \| -]` | any | Parameters as a JSON object literal, a file, or stdin. A `report` result is printed as a table; a `message` is printed with the result's scalar fields; anything else as JSON. |
| `post sales\|purchase <no> [--type Order]` | `sales.document.post` / `purchase.document.post` | `--type` is Order, Invoice or Credit Memo. |
| `workdate [date]` | `session.workdate` | Shows the work date. The work date is not stored in the data file; use `--workdate` on each command. |
| `search <query>` | `search` | Search pages and commands. |
| `mcp` | — | Confined MCP server on stdio (requires `--automation-read-root` and `--automation-write-root`); see [mcp.md](mcp.md). |
| `ui state\|back\|home` / `ui open <spec>` / `ui click <x> <y>` / `ui hover <x> <y>` / `ui screenshot <file.png>` | `ui.*` control methods | Drive the running app's window (needs `--connect`). `open` takes `page`, `page:key` or `list>card:key`. `click` and `hover` take window pixels, as the screenshot shows them. |
| `help` | — | Usage. |

Filters support standard range and pattern expressions per field: `10000`, `<>Open`, `10000|20000`, `>100`,
`2026-01-01..2026-01-31`, `*text*`, `''` (blank). Quote them in the shell: `--filter "Status=<>Open"`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Engine error (`error: <message>` on stderr): not found, invalid value, refused by a business rule, I/O (including "cannot reach the ERPCraft app" with `--connect`) |
| 2 | Usage error: unknown command or option, bad arguments, invalid JSON parameters |

## Examples

```powershell
erpcraft-cli init --demo
erpcraft-cli list Customer
erpcraft-cli list sales-order-list --filter "Sell-to Customer No.=30000"
erpcraft-cli list "G/L Entry" --fields "Entry No.,G/L Account No.,Amount" --limit 20
erpcraft-cli get Customer 10000
erpcraft-cli run sales.document.new '{\"customer_no\": \"20000\", \"lines\": [{\"no\": \"1100\", \"quantity\": 2}]}'
erpcraft-cli post sales S-ORD1004
erpcraft-cli run payment.customer.register '{\"customer_no\": \"20000\", \"amount\": 1443000}'
erpcraft-cli run report.trial_balance
erpcraft-cli --workdate 2026-12-31 run sales.document.post @post.json
erpcraft-cli --db postgres://erp:secret@localhost/erp tables
erpcraft-cli --connect list sales-order-list
erpcraft-cli --connect ui open customer-card:10000
erpcraft-cli --connect ui screenshot D:\tmp\customer.png
```

(In PowerShell 5.1, inner double quotes in a JSON argument must be escaped as `\"`; or put the
JSON in a file and pass `@file.json`, or pipe it and pass `-`.)
