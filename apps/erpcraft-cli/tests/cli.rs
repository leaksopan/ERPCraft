#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `erpcraft-cli` binary end to end on a temporary data file.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_erpcraft-cli");

struct TempData(PathBuf);

impl TempData {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("erpcraft-cli-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempData(dir.join("company.erpcraft.json"))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempData {
    fn drop(&mut self) {
        if let Some(d) = self.0.parent() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

fn cli(data: &TempData, args: &[&str]) -> Output {
    Command::new(BIN).arg("--data").arg(data.path()).args(args).env_remove("ERPCRAFT_DATA").output().unwrap()
}

fn ok(data: &TempData, args: &[&str]) -> String {
    let o = cli(data, args);
    assert!(o.status.success(), "{args:?} failed: {}\n{}", String::from_utf8_lossy(&o.stderr), String::from_utf8_lossy(&o.stdout));
    String::from_utf8(o.stdout).unwrap()
}

fn demo(name: &str) -> TempData {
    let d = TempData::new(name);
    let out = ok(&d, &["init", "--demo"]);
    assert!(out.contains("Demo company"), "{out}");
    assert!(d.path().exists());
    d
}

#[test]
fn init_and_list_customers() {
    let d = demo("list");
    let out = ok(&d, &["list", "Customer"]);
    assert!(out.contains("(5 records)"), "{out}");
    assert!(out.contains("10000"));
    let json: Value = serde_json::from_str(&ok(&d, &["list", "Customer", "--json"])).unwrap();
    assert_eq!(json["total"], 5);
    assert_eq!(json["records"].as_array().unwrap().len(), 5);

    let out = ok(&d, &["list", "Customer", "--filter", "No.=10000|20000", "--fields", "No.,Name,Balance (LCY)"]);
    assert!(out.contains("(2 records)"), "{out}");
    assert!(out.lines().next().unwrap().contains("Balance (LCY)"));

    // A second init refuses without --force: engine error, exit 1.
    let o = cli(&d, &["init"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error: "));
}

#[test]
fn get_describe_and_usage_errors() {
    let d = demo("get");
    let out = ok(&d, &["get", "Customer", "10000"]);
    assert!(out.lines().any(|l| l.starts_with("No. ") && l.ends_with("10000")), "{out}");
    assert!(ok(&d, &["describe", "Customer"]).contains("Balance (LCY)"));
    assert!(ok(&d, &["describe", "customer-card"]).contains("FastTab"));
    assert!(ok(&d, &["commands"]).contains("sales.document.post"));

    assert_eq!(cli(&d, &["frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&d, &["list"]).status.code(), Some(2));
    assert_eq!(cli(&d, &["list", "Customer", "--limit", "x"]).status.code(), Some(2));
    assert_eq!(cli(&d, &["run", "record.list", "{bad"]).status.code(), Some(2));
    assert_eq!(cli(&d, &["get", "Customer", "NOPE"]).status.code(), Some(1));
    assert!(ok(&d, &["--version"]).starts_with("erpcraft-cli "));
}

#[test]
fn run_new_order_and_post() {
    let d = demo("post");
    let r: Value = serde_json::from_str(&ok(
        &d,
        &["--json", "run", "sales.document.new", r#"{"customer_no": "20000", "lines": [{"no": "1100", "quantity": 2}]}"#],
    ))
    .unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    let out = ok(&d, &["post", "sales", &no]);
    assert!(out.contains("posted_no"), "{out}");
    // The order is gone from the data file; the posted invoice is there.
    assert_eq!(cli(&d, &["get", "Sales Header", &format!("Order|{no}")]).status.code(), Some(1));
    let out = ok(&d, &["run", "report.trial_balance"]);
    assert!(out.contains("Trial Balance") && out.contains("Total Debit"), "{out}");

    // Parameters from stdin.
    let mut child = Command::new(BIN)
        .args(["--data", d.path().to_str().unwrap(), "--json", "run", "record.get", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"table": "Item", "key": "1100"}"#).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    let item: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(item["No."], "1100");
}

#[test]
fn mcp_over_stdio() {
    let d = demo("mcp");
    let mut child = Command::new(BIN)
        .args(["mcp", "--data", d.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |msgs: &[Value]| -> Value {
        for m in msgs {
            writeln!(stdin, "{m}").unwrap();
        }
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC: {line:?}: {e}"))
    };
    let r = ask(&[json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}})]);
    assert_eq!(r["result"]["serverInfo"]["name"], "erpcraft");
    // The notification gets no reply, so the next line read is the tools/call result.
    let r = ask(&[
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "record_list", "arguments": {"table": "Customer", "limit": 0}}}),
    ]);
    assert_eq!(r["id"], 2);
    assert_eq!(r["result"]["structuredContent"]["total"], 5);
    drop(stdin);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("serving"));
}
