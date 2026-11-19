# ERPCraft MCP server

ERPCraft ships a native [Model Context Protocol](https://modelcontextprotocol.io) server inside
`erpcraft-cli`. It speaks JSON-RPC 2.0 over stdio (one message per line, synchronous) and runs every
request through the same engine entry point as the desktop app and the CLI (`Session::run`), so an
agent can do anything a user can.

```
erpcraft-cli mcp --data <company file>
```

Only protocol messages are written to stdout; the startup line and any diagnostics go to stderr.
The data file is resolved like every CLI command: `--data`, else `$ERPCRAFT_DATA`, else
`./company.erpcraft.json`. A missing file starts an empty company; call `company_init` to create the
demo company. Every mutating command is a transaction (nothing changes on error) and is saved to
the data file immediately.

## Registering with Claude Code

```powershell
cargo build --release -p erpcraft-cli
claude mcp add erpcraft -- D:\Kerja\ERPCraft\target\release\erpcraft-cli.exe mcp --data D:\Kerja\ERPCraft\company.erpcraft.json
```

Or in a project `.mcp.json`:

```json
{
  "mcpServers": {
    "erpcraft": {
      "command": "D:\\Kerja\\ERPCraft\\target\\release\\erpcraft-cli.exe",
      "args": ["mcp", "--data", "D:\\Kerja\\ERPCraft\\company.erpcraft.json"]
    }
  }
}
```

Any other MCP client (Claude Desktop, Cursor, …) uses the same command and arguments.

Note: the desktop app and the MCP server each hold the company in memory and save the whole file
after every change. Don't let both write the same data file at the same time. (A future `Remote`
backend will let the MCP server drive the running desktop app instead; the server is already
written against a `Backend` trait for that.)

## Protocol

- Protocol versions: `2025-06-18` (preferred), `2025-03-26`, `2024-11-05`. An unknown requested
  version gets `2025-06-18`.
- Methods: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`,
  `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`,
  `logging/setLevel` (accepted, no-op).
- Errors: invalid JSON → `-32700` (id `null`); invalid request / batch / message over 16 MiB →
  `-32600`; unknown method → `-32601`; bad parameters or unknown tool/prompt → `-32602`; unknown
  resource → `-32002`. Notifications never get a response.
- `initialize` returns `instructions` that explain the model to the agent: BC W1 names, "everything
  is a command", BC filter syntax, how to post documents.

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
| `search` | `search` | Tell Me: find pages and commands |
| `record_list` | `record.list` | Records of a table or list page with BC filters, search, fields, limit/offset |
| `record_get` | `record.get` | One record by primary key, FlowFields calculated |
| `record_new`, `record_validate`, `record_modify`, `record_delete` | `record.*` | Create / change / delete with BC triggers |
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
| `explain_page` | `page` | Explain a page and its actions in BC terms |

## Trying it by hand

```powershell
'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}',
'{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"company_info"}}' |
  erpcraft-cli mcp --data company.erpcraft.json
```
