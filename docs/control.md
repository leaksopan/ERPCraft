# Control channel

The desktop app can be driven while it runs, the way VectorCraft's `--control` works: start it with
`--control` and agents, scripts and `erpcraft-cli` send it requests over a loopback socket. Commands
run in the app's own session (the same `Session::run` as a click), and the window shows the effect.

```
erpcraft --control            # 127.0.0.1:7979
erpcraft --control 8000       # another port
erpcraft --db postgres://erp:secret@localhost/erp --control
```

The app binds `127.0.0.1` only, never another interface. A busy port is reported at start
(`error: cannot listen on 127.0.0.1:7979: …`). There is no authentication: anyone who can open a
loopback connection on the machine can drive the app, so only use it on your own machine.

## Clients

`erpcraft-cli --connect [addr]` sends every subcommand to the running app instead of opening the
company itself (`addr` is `host:port` or a port; default `127.0.0.1:7979`):

```powershell
erpcraft --control                                   # in one terminal (or from the Start menu)
erpcraft-cli --connect list Customer
erpcraft-cli --connect run sales.document.new '{\"customer_no\": \"20000\", \"lines\": [{\"no\": \"1100\", \"quantity\": 2}]}'
erpcraft-cli --connect ui open "sales-order-list>sales-order:Order|S-ORD1001"
erpcraft-cli --connect ui screenshot D:\tmp\order.png
erpcraft-cli --connect ui state
erpcraft-cli --connect 8000 ui back
erpcraft-cli --connect mcp                           # MCP server on the running app, see mcp.md
```

`--connect` excludes `--db`/`--data` (the app already has its company) and `--workdate` (which
would change the app's work date; run `erpcraft-cli --connect workdate 2026-12-31` to do that on
purpose).

Without any client, from PowerShell:

```powershell
$c = [System.Net.Sockets.TcpClient]::new('127.0.0.1', 7979)
$s = $c.GetStream(); $w = [System.IO.StreamWriter]::new($s); $r = [System.IO.StreamReader]::new($s)
$w.WriteLine('{"id":1,"method":"engine.run","params":{"command":"company.info"}}'); $w.Flush()
$r.ReadLine()
$c.Close()
```

## Protocol

Newline-delimited JSON over TCP, one request per line, one reply line per request, in order. Lines
longer than 16 MiB, invalid UTF-8 and invalid JSON get an error reply (id `null` when the id could
not be read) and the connection stays open. A connection may send any number of requests; at most
16 connections are served at once.

Request:

```json
{"id": 1, "method": "engine.run", "params": {"command": "record.list", "params": {"table": "Customer"}}}
```

`id` is a number, a string or absent (`null`) and is echoed back. `params` is an object (absent is
`{}`).

Replies:

```json
{"id": 1, "ok": true, "result": {"total": 5, "records": [...]}}
{"id": 2, "ok": false, "error": {"kind": "not_found", "message": "Customer NOPE does not exist."}}
```

`kind` is the engine error kind: `not_found`, `invalid`, `refused`, `io`, `conflict`,
`permission`, `internal`. Protocol errors use `invalid` (bad JSON, missing method) and `not_found`
(unknown method); `io` also means the app did not answer in time (60 s; 5 s for screenshots).

## Methods

| Method | Params | Result |
| --- | --- | --- |
| `ping` | — | `{"pong": true, "app": "erpcraft", "version": …}` (answered without the UI thread) |
| `engine.run` | `command`, `params` (object, optional) | The command's result. A command that changes data is shown the way the UI shows its own: `message` in the notice bar, `open` navigates (a new order opens its card), a card whose record is gone closes. Reads don't pop anything up. Errors go back to the caller, not into an error dialog. |
| `engine.commands` | — | `command.list`: every command with its parameter schema |
| `ui.open` | `spec` | Opens a page: `customer-list`, `customer-card:10000`, `sales-order:Order\|S-ORD1001` (multi-field keys joined by `\|`), or `list>card:key` to open a card over its list (‹ › then step through the list). A card page without a key starts a new record, like "+ New". Returns `ui.state`. |
| `ui.back` | — | Closes the top page (Esc). Returns `ui.state`. |
| `ui.home` | — | Back to the role center. Returns `ui.state`. |
| `ui.state` | — | `stack` (role center, lists with `page`/`filters`/`search`/`selected`, cards with `page`/`table`/`key`/`key_text`/`editing`/`expanded`/`list_page`), `top`, `dialog` (`error`/`confirm`/`form`/`report` or `null`), `notice`, `source` (password hidden), `revision`, `work_date` |
| `ui.screenshot` | `path` | Writes a PNG of the window (8-bit RGBA, physical pixels) to `path` (relative paths are relative to the app's working directory) and returns `{path, width, height}` with the absolute path. Fails with `io` after 5 s if no frame was painted. |

## How it works

`apps/erpcraft/src/control.rs`: one thread accepts connections; each connection has a thread that
reads lines (bounded at 16 MiB), parses them and hands each request to the UI thread through an mpsc
channel, then wakes it with `request_repaint`. The UI thread runs the queued requests in
`eframe::App::logic` before the frame is drawn (eframe calls `logic` even while the window is
minimised) and answers through a channel per request; a connection thread never touches the
session. A screenshot is requested with `Context::request_screenshot`; the image comes back to the
connection thread, which encodes the PNG off the UI thread. The protocol helpers (parsing, replies,
error kinds) are in `erpcraft_mcp::control`, shared with the `Remote` client.

The window also refreshes itself about every 2 seconds (`Session::refresh`), so changes saved by
other sessions on the same PostgreSQL company (another user, `erpcraft-cli --db`, a headless MCP
server) appear without input.
