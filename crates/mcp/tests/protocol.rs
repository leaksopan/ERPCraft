#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The MCP server end to end on an in-memory company, one JSON-RPC line at a time.

use erpcraft_engine::Session;
use erpcraft_mcp::Server;
use serde_json::{Value, json};

fn server() -> Server {
    Server::headless(Session::in_memory())
}

fn send(s: &mut Server, msg: Value) -> Value {
    let out = s.handle_line(&msg.to_string()).expect("a reply");
    assert!(!out.contains('\n'), "replies are single lines");
    serde_json::from_str(&out).unwrap()
}

fn call(s: &mut Server, id: i64, tool: &str, args: Value) -> Value {
    let r = send(s, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": args}}));
    assert_eq!(r["id"], id);
    r["result"].clone()
}

fn initialized() -> Server {
    let mut s = server();
    send(&mut s, json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}));
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    s
}

#[test]
fn initialize_handshake_negotiates_the_version() {
    let mut s = server();
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}}}));
    assert_eq!(r["id"], 1);
    let res = &r["result"];
    assert_eq!(res["protocolVersion"], "2025-03-26");
    assert_eq!(res["serverInfo"]["name"], "erpcraft");
    assert!(res["capabilities"]["tools"].is_object());
    assert!(res["capabilities"]["resources"].is_object());
    assert!(res["capabilities"]["prompts"].is_object());
    assert!(res["instructions"].as_str().unwrap().contains("ERPCraft"));

    // An unknown version gets the newest one the server speaks.
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": "x", "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}));
    assert_eq!(r["id"], "x");
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");

    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}));
    assert_eq!(r["result"], json!({}));
}

#[test]
fn tools_list_has_one_tool_per_command() {
    let mut s = initialized();
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let post = tools.iter().find(|t| t["name"] == "sales_document_post").expect("sales_document_post");
    assert_eq!(post["inputSchema"]["type"], "object");
    assert!(post["inputSchema"]["properties"]["no"].is_object());
    assert!(post["description"].as_str().unwrap().starts_with("Post a sales order"));
    assert!(tools.iter().any(|t| t["name"] == "command_run"));
    assert!(tools.iter().any(|t| t["name"] == "command_list"));
    assert_eq!(tools.iter().filter(|t| t["name"] == "command_list").count(), 1);
    for c in erpcraft_engine::commands::all() {
        assert!(tools.iter().any(|t| t["name"] == erpcraft_mcp::tool_name(c.id)), "{}", c.id);
    }
    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{name}");
    }
}

#[test]
fn tools_call_runs_commands() {
    let mut s = initialized();
    let r = call(&mut s, 1, "company_init", json!({"demo": true}));
    assert_eq!(r["isError"], false, "{r}");
    let r = call(&mut s, 2, "record_list", json!({"table": "Customer"}));
    assert_eq!(r["isError"], false);
    assert_eq!(r["structuredContent"]["total"], 5);
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["records"].as_array().unwrap().len(), 5);

    // The generic tool reaches the same commands.
    let r = call(&mut s, 3, "command_run", json!({"command": "record.get", "params": {"table": "Customer", "key": "10000"}}));
    assert_eq!(r["isError"], false);
    assert_eq!(r["structuredContent"]["No."], "10000");

    // Create and post an order.
    let r = call(&mut s, 4, "sales_document_new", json!({"customer_no": "20000", "lines": [{"no": "1100", "quantity": 1}]}));
    let no = r["structuredContent"]["no"].as_str().unwrap().to_string();
    let r = call(&mut s, 5, "sales_document_post", json!({"no": no}));
    assert_eq!(r["isError"], false, "{r}");
    assert!(r["structuredContent"]["posted_no"].is_string());
}

#[test]
fn engine_errors_are_tool_errors() {
    let mut s = initialized();
    call(&mut s, 1, "company_init", json!({}));
    let r = call(&mut s, 2, "sales_document_new", json!({"customer_no": "NOPE", "lines": []}));
    assert_eq!(r["isError"], true, "{r}");
    assert!(!r["content"][0]["text"].as_str().unwrap().is_empty());
    let r = call(&mut s, 3, "command_run", json!({"command": "no.such.command"}));
    assert_eq!(r["isError"], true);
    let r = call(&mut s, 4, "command_run", json!({}));
    assert_eq!(r["isError"], true);

    // An unknown tool is a protocol error.
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "nope"}}));
    assert_eq!(r["error"]["code"], -32602);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "record_list", "arguments": [1]}}));
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn protocol_errors() {
    let mut s = server();
    let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);

    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "no/such"}));
    assert_eq!(r["error"]["code"], -32601);
    let r = send(&mut s, json!({"jsonrpc": "1.0", "id": 2, "method": "ping"}));
    assert_eq!(r["error"]["code"], -32600);
    assert_eq!(r["id"], 2);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": 5}));
    assert_eq!(r["error"]["code"], -32600);
    let r = send(&mut s, json!([{"jsonrpc": "2.0", "id": 4, "method": "ping"}]));
    assert_eq!(r["error"]["code"], -32600);
    let r = send(&mut s, json!("hello"));
    assert_eq!(r["error"]["code"], -32600);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": {"a": 1}, "method": "ping"}));
    assert_eq!(r["error"]["code"], -32600);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": {}}));
    assert_eq!(r["error"]["code"], -32602);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 6, "method": "prompts/get", "params": {"name": "explain_page"}}));
    assert_eq!(r["error"]["code"], -32602);

    // Notifications and responses are never answered, even unknown ones; blank lines are skipped.
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"company_init"}}"#), None);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#), None);
    assert_eq!(s.handle_line("   "), None);
}

#[test]
fn resources() {
    let mut s = initialized();
    let read = |s: &mut Server, uri: &str| send(s, json!({"jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": {"uri": uri}}));

    let r = read(&mut s, "erpcraft://table/Customer");
    let c = &r["result"]["contents"][0];
    assert_eq!(c["uri"], "erpcraft://table/Customer");
    assert_eq!(c["mimeType"], "application/json");
    let t: Value = serde_json::from_str(c["text"].as_str().unwrap()).unwrap();
    assert_eq!(t["name"], "Customer");
    assert!(t["fields"].as_array().unwrap().len() > 5);

    let r = read(&mut s, "erpcraft://table/Sales%20Header");
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("Document Type"));
    let r = read(&mut s, "erpcraft://page/21");
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("customer-card"));

    let r = read(&mut s, "erpcraft://table/Nope");
    assert_eq!(r["error"]["code"], -32002);
    let r = read(&mut s, "https://example.com");
    assert_eq!(r["error"]["code"], -32002);

    send(&mut s, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "company_init", "arguments": {}}}));
    let r = read(&mut s, "erpcraft://record/Customer/10000");
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("\"No.\": \"10000\""));
    let r = read(&mut s, "erpcraft://record/G%2FL%20Account/10100");
    assert!(r.get("result").is_some() || r["error"]["code"] == -32002, "{r}");
    let r = read(&mut s, "erpcraft://company");
    assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("work_date"));

    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}));
    assert_eq!(r["result"]["resources"].as_array().unwrap().len(), 4);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 4, "method": "resources/templates/list"}));
    assert_eq!(r["result"]["resourceTemplates"].as_array().unwrap().len(), 3);
}

#[test]
fn prompts() {
    let mut s = initialized();
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}));
    let names: Vec<&str> = r["result"]["prompts"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["create_sales_order", "month_end_review", "explain_page"]);
    let r = send(&mut s, json!({"jsonrpc": "2.0", "id": 2, "method": "prompts/get",
        "params": {"name": "create_sales_order", "arguments": {"customer": "10000", "items": "2 x 1100"}}}));
    let text = r["result"]["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("10000") && text.contains("sales_document_new"));
}

#[test]
fn serve_reads_lines_and_writes_only_replies() {
    let mut s = server();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#, "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\r\n",
        "\n",
        "garbage\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    );
    let mut out = Vec::new();
    s.serve(input.as_bytes(), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert_eq!(lines[2]["id"], 2);
    // 2024-11-05 clients get no structuredContent.
    assert_eq!(s.protocol_version(), "2024-11-05");
}
