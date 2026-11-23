# ERPCraft MCP server

ERPCraft ships a native [Model Context Protocol](https://modelcontextprotocol.io) server inside
`erpcraft-cli`. It speaks JSON-RPC 2.0 over stdio (one message per line, synchronous) and runs every
request through the same engine entry point as the desktop app and the CLI (`Session::run`), so an
agent can do anything a user can.

```
erpcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir> --data <company file>
erpcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir> --db postgres://user:password@host/db
erpcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir> --connect [addr]   # drive the running desktop app
```

Per Craft RFC-0015, `erpcraft-cli mcp` refuses to start unless at least one
`--automation-read-root` and one `--automation-write-root` are provided (both flags are repeatable).
All file reads and writes performed by MCP tools (such as attachments and printed PDFs) are
canonicalized and confined to those directories.

Only protocol messages are written to stdout; the startup line and any diagnostics go to stderr.
The company is resolved like every CLI command: `--db`, else `$ERPCRAFT_DATABASE_URL`, else
`--data`, else `$ERPCRAFT_DATA`, else `./company.erpcraft.json`. A missing file starts an empty
company; call `company_init` to create the demo company. Every mutating command is a transaction
(nothing changes on error) and is saved immediately.

## Registering with an MCP client

```sh
cargo build --release -p erpcraft-cli
```

In a project `.mcp.json` (adjust paths to your local build and allowed automation directories):

```json
{
  "mcpServers": {
    "erpcraft": {
      "command": "/path/to/erpcraft-cli",
      "args": [
        "mcp",
        "--automation-read-root", "/path/to/workspace/in",
        "--automation-write-root", "/path/to/workspace/out",
        "--data", "/path/to/company.erpcraft.json"
      ]
    }
  }
}
```

Any MCP client (Claude Code, Claude Desktop, Cursor, …) uses the same command and arguments.

Note: with a data file, the desktop app and a headless MCP server each hold the company in memory
and save the whole file after every change. Don't let both write the same data file at the same
time: either share a PostgreSQL company (`--db`; the app refreshes every 2 seconds and shows the
agent's changes), or let the MCP server drive the running app (below).

## Driving the running desktop app

Start the app with `--control` and point the MCP server at it with `--connect`:

```sh
erpcraft --control                      # 127.0.0.1:7979
erpcraft-cli mcp --automation-read-root ./in --automation-write-root ./out --connect
```

The server then uses the `Remote` backend: every tool call is a control request (`engine.run`) to
the app, runs in the app's session and shows up in the window (a created order opens its card, the
message appears in the notice bar). The tool list comes from the app (`engine.commands`). Three more
tools appear only in this mode:

| Tool | What it does |
| --- | --- |
| `ui_open` `{spec}` | Open a page: `customer-list`, `customer-card:10000`, `sales-order-list>sales-order:Order\|S-ORD1001` |
| `ui_state` | The open pages (stack, keys, edit mode), the dialog and notice shown |
| `ui_screenshot` `{path?}` | Save a PNG of the window (default `erpcraft-screenshot.png` in the temp directory); the result has the path as text and the image as an MCP `image` content block (base64 PNG, up to 8 MiB) |

If the app is not running, `--connect` fails at start with "cannot reach the ERPCraft app at …". If
the app closes later, tool calls return `io` errors; the next call reconnects. The protocol is in
[control.md](control.md).

## Protocol

- Protocol versions: `2025-06-18` (preferred), `2025-03-26`, `2024-11-05`. An unknown requested
  version gets `2025-06-18`.
- Methods: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`,
  `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`,
  `logging/setLevel` (accepted, no-op).
- Errors: invalid JSON → `-32700` (id `null`); invalid request / batch / message over 16 MiB →
  `-32600`; unknown method → `-32601`; bad parameters or unknown tool/prompt → `-32602`; unknown
  resource → `-32002`. Notifications never get a response.
- `initialize` returns `instructions` that explain the model to the agent: ERPCraft schema names, "everything
  is a command", filter syntax, how to post documents (plus the `ui_*` tools with `--connect`).

## Tools

Every engine command is a tool named by replacing `.` with `_`. The tool list is generated from the
engine's command registry, so a new command shows up without changes to the server. The input
schema is the command's parameter schema; the description is its summary. Read-only commands carry
`readOnlyHint: true`.

| Tool | Command | What it does |
| --- | --- | --- |
| `command_run` | (any) | Run any command by id: `{"command": "record.list", "params": {...}}` |
| `command_list` | `command.list` | Every command with its parameter schema |
| `table_list`, `table_describe` | `table.*` | Tables with record counts; fields, options, relations |
| `page_list`, `page_describe` | `page.*` | Pages; layout, columns, parts, actions |
| `rolecenter_get` | `rolecenter.get` | Role center navigation and cue values |
| `search` | `search` | Quick search: find pages and commands |
| `record_list` | `record.list` | Records of a table or list page with filters, search, fields, limit/offset |
| `record_get` | `record.get` | One record by primary key, FlowFields calculated |
| `record_new`, `record_validate`, `record_modify`, `record_delete` | `record.*` | Create / change / delete with field validation triggers |
| `record_lookup` | `record.lookup` | Allowed values of a field |
| `company_init`, `company_info` | `company.*` | Create the (demo) company; company info + work date |
| `session_workdate` | `session.workdate` | Read or set the work date for this server session |
| `sales_document_new`, `sales_line_add`, `sales_document_release`, `sales_document_reopen`, `sales_document_post`, `sales_quote_make_order`, `sales_document_statistics` | `sales.*` | Sales quotes, orders, invoices, credit memos |
| `purchase_document_new`, `purchase_line_add`, `purchase_document_release`, `purchase_document_reopen`, `purchase_document_post`, `purchase_quote_make_order`, `purchase_document_statistics` | `purchase.*` | Purchase documents |
| `payment_customer_register`, `payment_vendor_register` | `payment.*` | Post and apply payments |
| `customer_statistics`, `vendor_statistics`, `item_availability`, `report_trial_balance` | reports | Statistics and reports (`report` = title, columns, rows) |

Results: `content[0].text` is the pretty-printed JSON result; with protocol `2025-06-18` the same
object is also in `structuredContent`. Engine errors (unknown customer, status not Open, unbalanced
posting, …) come back as a normal result with `isError: true` and the message as text, so the agent
can read and correct them.

Example:

```json
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"record_list","arguments":{"table":"Cust. Ledger Entry","filters":{"Open":"true","Due Date":"<2026-11-30"}}}}
```

## Resources

| URI | Content |
| --- | --- |
| `erpcraft://company` | `company.info` |
| `erpcraft://tables` | `table.list` |
| `erpcraft://pages` | `page.list` |
| `erpcraft://commands` | `command.list` |
| `erpcraft://table/{name}` | `table.describe`, e.g. `erpcraft://table/Sales%20Header` |
| `erpcraft://page/{id}` | `page.describe`, e.g. `erpcraft://page/customer-card` or `erpcraft://page/21` |
| `erpcraft://record/{table}/{key}` | `record.get`; table and key URL-encoded, multi-field keys joined by `\|`, e.g. `erpcraft://record/Sales%20Header/Order%7CS-ORD1001` |

All resources are `application/json`.

## Prompts

| Prompt | Arguments | Workflow |
| --- | --- | --- |
| `create_sales_order` | `customer`, `items` | Resolve customer and items, check availability, create the order, show statistics, post only after confirmation |
| `month_end_review` | `as_of` (optional) | Trial balance, overdue receivables / payables, unposted documents; read-only |
| `explain_page` | `page` | Explain a page and its actions in accounting and ERP terms |

## Trying it by hand

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"company_info"}}' | \
  erpcraft-cli mcp --automation-read-root . --automation-write-root . --data company.erpcraft.json
```
