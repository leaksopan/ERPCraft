#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `Remote` backend and the MCP server on top of it, against a small in-test server that speaks
//! the desktop app's control protocol over a real loopback socket.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

use erpcraft_engine::{ErrorKind, Session};
use erpcraft_mcp::control::{self, parse_request, reply_line};
use erpcraft_mcp::{Backend, Remote, Server};
use serde_json::{Value, json};

/// A fake app: engine methods run on an in-memory session, `ui.*` answers canned values.
fn fake_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let mut session = Session::in_memory();
        let mut stack: Vec<String> = vec!["role-center".into()];
        // One client at a time, like a single MCP server.
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut writer = stream.try_clone().unwrap();
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let out = match parse_request(&line) {
                    Err((id, e)) => reply_line(&id, Err(e)),
                    Ok(req) => {
                        let p = &req.params;
                        let result = match req.method.as_str() {
                            "engine.run" => session.run(p["command"].as_str().unwrap_or(""), &p["params"]),
                            "engine.commands" => session.run("command.list", &json!({})),
                            "ui.open" => {
                                stack.push(p["spec"].as_str().unwrap_or("").to_string());
                                Ok(json!({"stack": stack}))
                            }
                            "ui.state" => Ok(json!({"stack": stack})),
                            "ui.screenshot" => {
                                let path = p["path"].as_str().unwrap_or("").to_string();
                                std::fs::write(&path, b"\x89PNG\r\n\x1a\nfake").unwrap();
                                Ok(json!({"path": path, "width": 1, "height": 1}))
                            }
                            other => Err(erpcraft_engine::EngineError::not_found(format!("Unknown method '{other}'"))),
                        };
                        reply_line(&req.id, result)
                    }
                };
                if writer.write_all(format!("{out}\n").as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    addr
}

#[test]
fn remote_runs_commands_and_maps_errors() {
    let addr = fake_app();
    let mut r = Remote::connect(&addr).unwrap();
    assert!(r.has_ui());
    assert!(r.describe().contains(&addr));
    let commands = r.commands();
    assert!(commands.iter().any(|c| c.id == "sales.document.post"), "{} commands", commands.len());

    let init = r.run("company.init", &json!({"demo": true})).unwrap();
    assert!(init["message"].as_str().unwrap().contains("Demo"), "{init}");
    let list = r.run("record.list", &json!({"table": "Customer"})).unwrap();
    assert_eq!(list["total"], 5);

    let e = r.run("no.such.command", &json!({})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    let e = r.run("record.get", &json!({"table": "Customer", "key": "NOPE"})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    // The connection survives errors.
    assert_eq!(r.run("record.list", &json!({"table": "Customer", "limit": 1})).unwrap()["records"].as_array().unwrap().len(), 1);
    assert_eq!(r.ui("ui.state", &json!({})).unwrap()["stack"], json!(["role-center"]));
}

#[test]
fn connect_failures_are_io_errors() {
    // A port nobody listens on: bind, read the port, drop the listener.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let e = Remote::connect(&format!("127.0.0.1:{port}")).err().unwrap();
    assert_eq!(e.kind, ErrorKind::Io);
    assert!(e.message.contains("erpcraft --control"), "{}", e.message);
    assert_eq!(Remote::connect("not an address").err().unwrap().kind, ErrorKind::Invalid);
}

fn send(s: &mut Server, msg: Value) -> Value {
    serde_json::from_str(&s.handle_line(&msg.to_string()).unwrap()).unwrap()
}

#[test]
fn mcp_over_remote_has_ui_tools() {
    let addr = fake_app();
    let mut s = Server::new(Box::new(Remote::connect(&addr).unwrap()));
    let init = send(&mut s, json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("ui_open"));

    let tools = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    for name in ["ui_open", "ui_state", "ui_screenshot", "record_list", "command_run"] {
        assert!(tools.iter().any(|t| t["name"] == name), "{name}");
    }

    let call = |s: &mut Server, id: i64, name: &str, args: Value| {
        send(s, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}))["result"].clone()
    };
    let r = call(&mut s, 2, "ui_open", json!({"spec": "customer-card:10000"}));
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["stack"], json!(["role-center", "customer-card:10000"]));

    let shot: PathBuf = std::env::temp_dir().join(format!("erpcraft-remote-test-{}.png", std::process::id()));
    let r = call(&mut s, 3, "ui_screenshot", json!({"path": shot.to_string_lossy()}));
    assert_eq!(r["isError"], false, "{r}");
    let content = r["content"].as_array().unwrap();
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    assert!(content[1]["data"].as_str().unwrap().starts_with("iVBORw0KGg"));
    let _ = std::fs::remove_file(&shot);

    let r = call(&mut s, 4, "command_run", json!({"command": "nope"}));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"].as_str().unwrap().starts_with("not_found error"));
}

#[test]
fn headless_has_no_ui_tools() {
    let mut s = Server::headless(Session::in_memory());
    let tools = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    assert!(!tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"].as_str().unwrap().starts_with("ui_")));
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "ui_open", "arguments": {"spec": "x"}}}));
    assert!(r["error"].is_object(), "{r}");
    assert_eq!(control::DEFAULT_ADDR, "127.0.0.1:7979");
}
