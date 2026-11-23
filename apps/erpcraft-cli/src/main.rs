//! `erpcraft-cli`: run any ERPCraft engine command headless, inspect metadata, and serve MCP over
//! stdio. Every subcommand is a thin wrapper over `Session::run`, the same entry point the desktop
//! app and the MCP server use. With `--connect` the commands run in the running desktop app
//! instead (its control channel, docs/control.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

mod args;
mod render;

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use erpcraft_engine::meta::TableDef;
use erpcraft_engine::{Confine, EngineError, Session, schema, store};
use erpcraft_mcp::{Backend, Headless, Remote, Server};
use serde_json::{Map, Value, json};

use args::Args;

/// Largest parameter file / stdin the CLI reads for `run`.
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Columns shown by `list` when no page or `--fields` says otherwise.
const DEFAULT_COLUMNS: usize = 8;

const USAGE: &str = "\
erpcraft-cli - ERPCraft ERP on the command line

USAGE:
    erpcraft-cli [--db <url> | --data <file> | --connect [addr]] [--json] [--workdate <date>] <command> [args]

COMMANDS:
    init [--demo|--empty] [--force]        Create the company (demo data by default)
    commands                               List every engine command, grouped
    tables                                 List tables with record counts
    pages                                  List pages
    describe <table-or-page>               Fields of a table, or layout of a page
    list <table|page> [--filter Field=expr ...] [--search text] [--fields a,b] [--limit n] [--offset n]
                                           Records as a table (filter syntax, e.g. --filter \"No.=10000..20000\")
    get <table> <key> [--all]              One record; multi-field keys joined by '|' (Order|S-ORD1001)
    run <command.id> [json | @file.json | -]
                                           Run any command with JSON parameters (- reads stdin)
    post sales|purchase <no> [--type Order]
                                           Post a document (Order, Invoice, Credit Memo)
    workdate [date]                        Show (or set for this invocation) the work date
    search <query>                         Find pages and commands
    mcp                                    Serve MCP (JSON-RPC over stdio; requires --automation-read-root
                                           and --automation-write-root per RFC-0015)
    ui state|back|home|open <spec>|click <x> <y>|hover <x> <y>|screenshot <file.png>
                                           Drive the running app's window (needs --connect)
    help                                   This text

GLOBAL OPTIONS:
    --db <url>           PostgreSQL company, postgres://user:password@host/db
                         (default: $ERPCRAFT_DATABASE_URL; wins over --data)
    --data <file>        Company data file (default: $ERPCRAFT_DATA, else ./company.erpcraft.json)
    --connect [addr]     Run in the running desktop app (erpcraft --control) instead;
                         addr is host:port or a port (default 127.0.0.1:7979)
    --json               Print the raw JSON result instead of formatted text
    --workdate <date>    Work date (YYYY-MM-DD) for this invocation; posting uses it
    --automation-read-root <dir>   Allowed directory for reading files (repeatable; required for mcp)
    --automation-write-root <dir>  Allowed directory for writing files (repeatable; required for mcp)
    -h, --help           Help;  -V, --version  Version

EXIT CODES:
    0 ok, 1 engine error, 2 usage error
";

enum Failure {
    Usage(String),
    Engine(EngineError),
}

impl From<EngineError> for Failure {
    fn from(e: EngineError) -> Self {
        Failure::Engine(e)
    }
}

type Outcome = Result<String, Failure>;

fn usage<T>(m: impl Into<String>) -> Result<T, Failure> {
    Err(Failure::Usage(m.into()))
}

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => return fail_usage(&e),
    };
    if args.version {
        return emit(&format!("erpcraft-cli {}\n", env!("CARGO_PKG_VERSION")));
    }
    if args.help || args.positional.first().is_some_and(|c| c == "help") {
        return emit(USAGE);
    }
    if args.positional.is_empty() {
        let _ = io::stderr().write_all(USAGE.as_bytes());
        return ExitCode::from(2);
    }
    match dispatch(&args) {
        Ok(text) => emit(&text),
        Err(Failure::Usage(m)) => fail_usage(&m),
        Err(Failure::Engine(e)) => {
            let _ = writeln!(io::stderr(), "error: {}", e.message);
            ExitCode::from(1)
        }
    }
}

fn emit(text: &str) -> ExitCode {
    let mut out = io::stdout().lock();
    // A closed pipe (`| head`) is not an error worth a message.
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    ExitCode::SUCCESS
}

fn fail_usage(m: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "error: {m}\nRun 'erpcraft-cli help' for usage.");
    ExitCode::from(2)
}

/// The company source: `--db`, `$ERPCRAFT_DATABASE_URL`, `--data`, `$ERPCRAFT_DATA`, else
/// `./company.erpcraft.json`.
fn source(args: &Args) -> String {
    let data = args.data.as_ref().map(|p| p.to_string_lossy().into_owned());
    store::resolve_source(args.db.as_deref(), data.as_deref())
}

/// Where the subcommands' engine commands run.
enum Conn {
    /// In this process, on the company source.
    Local(Session),
    /// In the running desktop app (`--connect`).
    Remote(Remote),
}

impl Conn {
    fn run(&mut self, command: &str, params: &Value) -> Result<Value, EngineError> {
        match self {
            Conn::Local(s) => s.run(command, params),
            Conn::Remote(r) => r.run(command, params),
        }
    }

    /// What the commands run on, safe to print.
    fn describe(&self) -> String {
        match self {
            Conn::Local(s) => store::redact(&s.source()),
            Conn::Remote(r) => r.describe(),
        }
    }
}

fn confine_opt(args: &Args) -> Result<Option<Confine>, Failure> {
    if args.read_roots.is_empty() && args.write_roots.is_empty() {
        return Ok(None);
    }
    Ok(Some(Confine::new(&args.read_roots, &args.write_roots)?))
}

fn open(args: &Args) -> Result<Conn, Failure> {
    let confine = confine_opt(args)?;
    let mut c = match &args.connect {
        Some(addr) => {
            if confine.is_some() {
                return usage("--connect is refused while --automation-read-root / --automation-write-root confinement is active (RFC-0015)");
            }
            if args.db.is_some() || args.data.is_some() {
                return usage("--connect runs in the app's company; it does not take --db or --data");
            }
            if args.workdate.is_some() {
                return usage("--workdate would change the running app's work date; use 'workdate <date>' to do that on purpose");
            }
            Conn::Remote(Remote::connect(addr)?)
        }
        None => match confine {
            Some(c) => Conn::Local(Session::open_source_confined(&source(args), c)?),
            None => Conn::Local(Session::open_source(&source(args))?),
        },
    };
    if let Some(d) = &args.workdate {
        c.run("session.workdate", &json!({"date": d}))?;
    }
    Ok(c)
}

fn dispatch(args: &Args) -> Outcome {
    let cmd = args.positional.first().map(String::as_str).unwrap_or("");
    let rest: Vec<&str> = args.positional.iter().skip(1).map(String::as_str).collect();
    match cmd {
        "init" => init(args, &rest),
        "commands" => no_more(cmd, args, &rest).and_then(|()| commands(args)),
        "tables" => no_more(cmd, args, &rest).and_then(|()| tables(args)),
        "pages" => no_more(cmd, args, &rest).and_then(|()| pages(args)),
        "describe" => describe(args, &rest),
        "list" => list(args, &rest),
        "get" => get(args, &rest),
        "run" => run(args, &rest),
        "post" => post(args, &rest),
        "workdate" => workdate(args, &rest),
        "search" => search(args, &rest),
        "mcp" => mcp(args, &rest),
        "ui" => ui(args, &rest),
        other => usage(format!("unknown command '{other}'")),
    }
}

fn no_more(cmd: &str, args: &Args, rest: &[&str]) -> Result<(), Failure> {
    args.only(cmd, &[], &[]).map_err(Failure::Usage)?;
    if let Some(extra) = rest.first() {
        return usage(format!("'{cmd}' takes no arguments (got '{extra}')"));
    }
    Ok(())
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    s.push('\n');
    s
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn rows_of<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

// ---- subcommands ----------------------------------------------------------------------------

fn init(args: &Args, rest: &[&str]) -> Outcome {
    args.only("init", &[], &["demo", "empty", "force"]).map_err(Failure::Usage)?;
    if !rest.is_empty() {
        return usage("'init' takes no arguments");
    }
    if args.flag("demo") && args.flag("empty") {
        return usage("--demo and --empty exclude each other");
    }
    let mut s = open(args)?;
    let r = s.run("company.init", &json!({"demo": !args.flag("empty"), "force": args.flag("force")}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    Ok(format!("{}\nCompany: {}\n", str_of(&r, "message"), s.describe()))
}

fn commands(args: &Args) -> Outcome {
    let mut s = open(args)?;
    let r = s.run("command.list", &json!({}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    let cmds = rows_of(&r, "commands");
    let mut groups: Vec<&str> = Vec::new();
    for c in cmds {
        let g = str_of(c, "group");
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let w = cmds.iter().map(|c| str_of(c, "id").len()).max().unwrap_or(0);
    let mut out = String::new();
    for g in groups {
        let _ = writeln!(out, "{g}");
        for c in cmds.iter().filter(|c| str_of(c, "group") == g) {
            let marker = if c.get("mutates").and_then(Value::as_bool).unwrap_or(false) { "*" } else { " " };
            let _ = writeln!(out, "  {:w$} {marker} {}", str_of(c, "id"), str_of(c, "summary"));
        }
        out.push('\n');
    }
    out.push_str("* changes data. Run one with: erpcraft-cli run <command.id> '{...}'\n");
    Ok(out)
}

fn tables(args: &Args) -> Outcome {
    let mut s = open(args)?;
    let r = s.run("table.list", &json!({}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    let rows: Vec<Vec<String>> = rows_of(&r, "tables")
        .iter()
        .map(|t| {
            let key = t.get("key").and_then(Value::as_array).map(|k| k.iter().map(|v| render::value(v, None)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            vec![str_of(t, "name").to_string(), render::value(&t["id"], None), render::value(&t["records"], None), key]
        })
        .collect();
    let mut out = String::new();
    render::table(&mut out, &h(&["Table", "Id", "Records", "Primary Key"]), &[render::Align::Left, render::Align::Right, render::Align::Right, render::Align::Left], &rows);
    Ok(out)
}

fn pages(args: &Args) -> Outcome {
    let mut s = open(args)?;
    let r = s.run("page.list", &json!({}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    let rows: Vec<Vec<String>> = rows_of(&r, "pages")
        .iter()
        .map(|p| {
            vec![str_of(p, "id").to_string(), render::value(&p["object_id"], None), str_of(p, "type").to_string(), str_of(p, "table").to_string(), str_of(p, "caption").to_string()]
        })
        .collect();
    let mut out = String::new();
    render::table(&mut out, &h(&["Page", "Id", "Type", "Table", "Caption"]), &[render::Align::Left, render::Align::Right], &rows);
    Ok(out)
}

fn h(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

fn describe(args: &Args, rest: &[&str]) -> Outcome {
    args.only("describe", &[], &[]).map_err(Failure::Usage)?;
    let [name] = rest else { return usage("usage: describe <table-or-page>") };
    let mut s = open(args)?;
    let is_page = schema::base_app().page(name).is_some();
    let r = if is_page { s.run("page.describe", &json!({"page": name}))? } else { s.run("table.describe", &json!({"table": name}))? };
    if args.json {
        return Ok(pretty(&r));
    }
    let mut out = String::new();
    if is_page {
        let _ = writeln!(out, "Page {} \"{}\" ({}, BC page {})", str_of(&r, "id"), str_of(&r, "caption"), str_of(&r, "type"), render::value(&r["object_id"], None));
        if !str_of(&r, "table").is_empty() {
            let _ = writeln!(out, "Source table: {}", str_of(&r, "table"));
        }
        if !str_of(&r, "card_page").is_empty() {
            let _ = writeln!(out, "Opens: {}", str_of(&r, "card_page"));
        }
        for v in rows_of(&r, "view") {
            let _ = writeln!(out, "View: {} = {}", str_of(v, "field"), str_of(v, "value"));
        }
        let cols = joined(&r["columns"]);
        if !cols.is_empty() {
            let _ = writeln!(out, "Columns: {cols}");
        }
        for g in rows_of(&r, "groups") {
            let _ = writeln!(out, "Section {}: {}", str_of(g, "caption"), joined(&g["fields"]));
        }
        for p in rows_of(&r, "parts") {
            let _ = writeln!(out, "Part {}: page {}", str_of(p, "caption"), str_of(p, "page"));
        }
        for f in rows_of(&r, "factboxes") {
            let _ = writeln!(out, "Detail {}: {}", str_of(f, "caption"), joined(&f["fields"]));
        }
        let actions: Vec<Vec<String>> = rows_of(&r, "actions")
            .iter()
            .map(|a| {
                let target = if str_of(a, "command").is_empty() { format!("open {}", str_of(a, "open_page")) } else { str_of(a, "command").to_string() };
                vec![str_of(a, "caption").to_string(), str_of(a, "area").to_string(), target]
            })
            .collect();
        if !actions.is_empty() {
            out.push('\n');
            render::table(&mut out, &h(&["Action", "Area", "Runs"]), &[], &actions);
        }
    } else {
        let _ = writeln!(out, "Table {} (BC table {})  key: {}", str_of(&r, "name"), render::value(&r["id"], None), joined(&r["key"]));
        let rows: Vec<Vec<String>> = rows_of(&r, "fields")
            .iter()
            .map(|f| {
                let mut notes = Vec::new();
                if f.get("flowfield").and_then(Value::as_bool) == Some(true) {
                    notes.push("FlowField".to_string());
                }
                if f.get("editable").and_then(Value::as_bool) == Some(false) {
                    notes.push("read-only".to_string());
                }
                if !str_of(f, "relation").is_empty() {
                    notes.push(format!("-> {}", str_of(f, "relation")));
                }
                if f.get("options").is_some() {
                    notes.push(format!("options: {}", joined(&f["options"])));
                }
                vec![str_of(f, "name").to_string(), str_of(f, "type").to_string(), notes.join("; ")]
            })
            .collect();
        out.push('\n');
        render::table(&mut out, &h(&["Field", "Type", "Notes"]), &[], &rows);
    }
    Ok(out)
}

fn joined(v: &Value) -> String {
    v.as_array().map(|a| a.iter().map(|x| render::value(x, None)).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

/// A table name or a page id (`list` accepts both).
fn target(name: &str) -> (Value, Option<&'static TableDef>, Vec<String>) {
    let meta = schema::base_app();
    if let Some(p) = meta.page(name) {
        let t = p.source_table.as_deref().and_then(|t| meta.table(t));
        return (json!({"page": p.id}), t, p.columns.clone());
    }
    let t = meta.table(name);
    let cols = t.map(default_columns).unwrap_or_default();
    (json!({"table": name}), t, cols)
}

/// The columns of the table's list page, else its key and first fields.
fn default_columns(t: &TableDef) -> Vec<String> {
    let meta = schema::base_app();
    if let Some(p) = t.lookup_page.as_deref().and_then(|p| meta.page(p))
        && !p.columns.is_empty()
    {
        return p.columns.clone();
    }
    let mut cols = t.primary_key.clone();
    for f in &t.fields {
        if cols.len() >= DEFAULT_COLUMNS {
            break;
        }
        if !f.is_flow() && !cols.contains(&f.name) {
            cols.push(f.name.clone());
        }
    }
    cols
}

fn list(args: &Args, rest: &[&str]) -> Outcome {
    args.only("list", &["filter", "search", "fields", "limit", "offset"], &[]).map_err(Failure::Usage)?;
    let [name] = rest else { return usage("usage: list <table|page> [--filter Field=expr ...] [--search text] [--fields a,b] [--limit n]") };
    let mut s = open(args)?;
    let (mut params, t, mut columns) = target(name);
    let mut filters = Map::new();
    for f in args.option_all("filter") {
        let Some((field, expr)) = f.split_once('=') else { return usage(format!("--filter needs Field=expression, got '{f}'")) };
        filters.insert(field.trim().to_string(), Value::String(expr.to_string()));
    }
    if !filters.is_empty() {
        params["filters"] = Value::Object(filters);
    }
    if let Some(q) = args.option("search") {
        params["search"] = json!(q);
    }
    if let Some(f) = args.option("fields") {
        columns = f.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect();
        params["fields"] = json!(columns);
    }
    for k in ["limit", "offset"] {
        if let Some(v) = args.option(k) {
            let n: u64 = v.trim().parse().map_err(|_| Failure::Usage(format!("--{k} needs a whole number, got '{v}'")))?;
            params[k] = json!(n);
        }
    }
    let r = s.run("record.list", &params)?;
    if args.json {
        return Ok(pretty(&r));
    }
    let records = rows_of(&r, "records");
    let mut out = String::new();
    if columns.is_empty() {
        columns = records.first().and_then(Value::as_object).map(|o| o.keys().take(DEFAULT_COLUMNS).cloned().collect()).unwrap_or_default();
    }
    render::records(&mut out, t, &columns, records);
    let total = r.get("total").and_then(Value::as_u64).unwrap_or(0);
    let offset = r.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let shown = records.len() as u64;
    if shown == total {
        let _ = writeln!(out, "({total} record{})", if total == 1 { "" } else { "s" });
    } else {
        let _ = writeln!(out, "({}-{} of {total} records; use --limit / --offset for more)", if shown == 0 { offset } else { offset + 1 }, offset + shown);
    }
    Ok(out)
}

fn get(args: &Args, rest: &[&str]) -> Outcome {
    args.only("get", &[], &[]).map_err(Failure::Usage)?;
    let [table, key] = rest else { return usage("usage: get <table> <key>   (multi-field keys: Order|S-ORD1001)") };
    let mut s = open(args)?;
    let r = s.run("record.get", &json!({"table": table, "key": key}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    let t = schema::base_app().table(table);
    let empty = Map::new();
    let obj = r.as_object().unwrap_or(&empty);
    // Table field order (the JSON object itself is sorted by name).
    let mut names: Vec<&str> = t.map(|t| t.fields.iter().map(|f| f.name.as_str()).filter(|n| obj.contains_key(*n)).collect()).unwrap_or_default();
    names.extend(obj.keys().map(String::as_str).filter(|k| t.is_none_or(|t| t.field(k).is_none())));
    let pairs: Vec<(String, String)> = names
        .into_iter()
        .filter_map(|k| obj.get(k).map(|v| (k, v)))
        .filter(|(_, v)| args.all || !render::is_blank(v))
        .map(|(k, v)| (k.to_string(), render::value(v, t.and_then(|t| t.field(k)).map(|f| &f.ty))))
        .collect();
    let mut out = String::new();
    render::card(&mut out, &pairs);
    Ok(out)
}

fn read_capped<R: Read>(r: R, what: &str) -> Result<String, Failure> {
    let mut buf = Vec::new();
    r.take(MAX_INPUT_BYTES + 1).read_to_end(&mut buf).map_err(|e| Failure::Usage(format!("cannot read {what}: {e}")))?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return usage(format!("{what} is larger than {MAX_INPUT_BYTES} bytes"));
    }
    String::from_utf8(buf).map_err(|_| Failure::Usage(format!("{what} is not valid UTF-8")))
}

fn run(args: &Args, rest: &[&str]) -> Outcome {
    args.only("run", &[], &[]).map_err(Failure::Usage)?;
    let (command, text) = match rest {
        [c] => (*c, String::new()),
        [c, "-"] => (*c, read_capped(io::stdin().lock(), "stdin")?),
        [c, p] => match p.strip_prefix('@') {
            Some(path) => {
                let checked = match confine_opt(args)? {
                    Some(confine) => confine.check_read(std::path::Path::new(path))?,
                    None => std::path::PathBuf::from(path),
                };
                let f = std::fs::File::open(&checked).map_err(|e| Failure::Usage(format!("cannot open {path}: {e}")))?;
                (*c, read_capped(f, path)?)
            }
            None => (*c, (*p).to_string()),
        },
        _ => return usage("usage: run <command.id> [json | @file.json | -]"),
    };
    let params: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| Failure::Usage(format!("parameters are not valid JSON: {e}")))?
    };
    if !params.is_object() {
        return usage("parameters must be a JSON object, e.g. '{\"table\": \"Customer\"}'");
    }
    let mut s = open(args)?;
    let r = s.run(command, &params)?;
    Ok(if args.json { pretty(&r) } else { result_text(&r) })
}

/// A command result for people: a report as a table, a message plus the scalar fields, otherwise
/// the JSON.
fn result_text(r: &Value) -> String {
    let mut out = String::new();
    if let Some(rep) = r.get("report") {
        render::report(&mut out, rep);
    }
    if let Some(m) = r.get("message").and_then(Value::as_str) {
        let _ = writeln!(out, "{m}");
        let scalars: Vec<(String, String)> = r
            .as_object()
            .map(|o| {
                o.iter()
                    .filter(|(k, v)| !matches!(k.as_str(), "message" | "open" | "report") && !v.is_object() && !v.is_array() && !v.is_null())
                    .map(|(k, v)| (k.clone(), render::value(v, None)))
                    .collect()
            })
            .unwrap_or_default();
        render::card(&mut out, &scalars);
    }
    if out.is_empty() {
        return pretty(r);
    }
    out
}

fn post(args: &Args, rest: &[&str]) -> Outcome {
    args.only("post", &["type"], &[]).map_err(Failure::Usage)?;
    let [side, no] = rest else { return usage("usage: post sales|purchase <no> [--type Order|Invoice|Credit Memo]") };
    let command = match *side {
        "sales" => "sales.document.post",
        "purchase" | "purchasing" => "purchase.document.post",
        other => return usage(format!("post: expected 'sales' or 'purchase', got '{other}'")),
    };
    let mut s = open(args)?;
    let r = s.run(command, &json!({"document_type": args.option("type").unwrap_or("Order"), "no": no}))?;
    Ok(if args.json { pretty(&r) } else { result_text(&r) })
}

fn workdate(args: &Args, rest: &[&str]) -> Outcome {
    args.only("workdate", &[], &[]).map_err(Failure::Usage)?;
    let params = match rest {
        [] => json!({}),
        [d] => json!({"date": d}),
        _ => return usage("usage: workdate [YYYY-MM-DD]"),
    };
    let mut s = open(args)?;
    let r = s.run("session.workdate", &params)?;
    if !rest.is_empty() && matches!(s, Conn::Local(_)) {
        let _ = writeln!(io::stderr(), "note: the work date is not stored in the data file; pass --workdate <date> to each command (or set it in an MCP session with session_workdate).");
    }
    Ok(if args.json { pretty(&r) } else { format!("Work date: {}\n", str_of(&r, "work_date")) })
}

fn search(args: &Args, rest: &[&str]) -> Outcome {
    args.only("search", &[], &[]).map_err(Failure::Usage)?;
    if rest.is_empty() {
        return usage("usage: search <query>");
    }
    let mut s = open(args)?;
    let r = s.run("search", &json!({"query": rest.join(" ")}))?;
    if args.json {
        return Ok(pretty(&r));
    }
    let mut out = String::new();
    let pages: Vec<Vec<String>> = rows_of(&r, "pages").iter().map(|p| vec![str_of(p, "page").to_string(), str_of(p, "caption").to_string(), str_of(p, "type").to_string()]).collect();
    let cmds: Vec<Vec<String>> = rows_of(&r, "commands").iter().map(|c| vec![str_of(c, "command").to_string(), str_of(c, "label").to_string()]).collect();
    if !pages.is_empty() {
        render::table(&mut out, &h(&["Page", "Caption", "Type"]), &[], &pages);
    }
    if !cmds.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        render::table(&mut out, &h(&["Command", "Label"]), &[], &cmds);
    }
    if out.is_empty() {
        out.push_str("Nothing found.\n");
    }
    Ok(out)
}

fn ui(args: &Args, rest: &[&str]) -> Outcome {
    args.only("ui", &[], &[]).map_err(Failure::Usage)?;
    const UI_USAGE: &str = "usage: ui state | back | home | open <spec> | click <x> <y> | hover <x> <y> | screenshot <file.png>   (with --connect)";
    let (method, params) = match rest {
        ["state"] => ("ui.state", json!({})),
        ["back"] => ("ui.back", json!({})),
        ["home"] => ("ui.home", json!({})),
        ["open", spec] => ("ui.open", json!({"spec": spec})),
        ["screenshot", path] => ("ui.screenshot", json!({"path": path})),
        [verb @ ("click" | "hover"), x, y] => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => (if *verb == "click" { "ui.click" } else { "ui.hover" }, json!({"x": x, "y": y})),
            _ => return usage(UI_USAGE),
        },
        _ => return usage(UI_USAGE),
    };
    let no_app = "'ui' drives the running desktop app: start it with 'erpcraft --control' and pass --connect";
    if args.connect.is_none() {
        return usage(no_app);
    }
    let Conn::Remote(mut remote) = open(args)? else { return usage(no_app) };
    let r = remote.ui(method, &params)?;
    if method == "ui.screenshot" && !args.json {
        return Ok(format!("Saved {} ({} x {})
", str_of(&r, "path"), render::value(&r["width"], None), render::value(&r["height"], None)));
    }
    Ok(pretty(&r))
}

fn mcp(args: &Args, rest: &[&str]) -> Outcome {
    no_more("mcp", args, rest)?;
    // RFC-0015: agent entry points are confined and refuse to start without both roots.
    let confine = Confine::require_both(&args.read_roots, &args.write_roots).map_err(|e| Failure::Usage(e.message))?;
    if args.connect.is_some() {
        return usage("mcp --connect is refused in confined mode (RFC-0015: target desktop process must enforce matching confinement)");
    }
    let mut session = Session::open_source_confined(&source(args), confine)?;
    if let Some(d) = &args.workdate {
        session.run("session.workdate", &json!({"date": d}))?;
    }
    let backend: Box<dyn Backend> = Box::new(Headless::new(session));
    let mut server = Server::new(backend);
    let _ = writeln!(io::stderr(), "erpcraft-cli mcp: serving {} on stdio", server.backend().describe());
    let stdin = io::stdin();
    let stdout = io::stdout();
    server.serve(stdin.lock(), stdout.lock()).map_err(|e| Failure::Engine(EngineError::io(format!("MCP transport: {e}"))))?;
    Ok(String::new())
}
