#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The Business Central connector against a tiny in-test HTTP server that plays the Microsoft
//! token endpoint and the BC API v2.0 (OData JSON, paging with `@odata.nextLink`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use erpcraft_engine::Session;
use serde_json::{Value, json};

const SECRET: &str = "test-secret-Zq81xLmK";
const TOKEN: &str = "test-token-Pw55aBcD";
const COMPANY: &str = "11111111-2222-3333-4444-555555555555";
const ROOT: &str = "/api/t1/Production/api/v2.0";

#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

type Handler = Arc<dyn Fn(&Req, &str) -> (u16, String) + Send + Sync>;

struct Server {
    base: String,
    log: Arc<Mutex<Vec<Req>>>,
}

fn serve(h: Handler) -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let (b, lg) = (base.clone(), log.clone());
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            handle(s, &h, &b, &lg);
        }
    });
    Server { base, log }
}

fn handle(mut s: TcpStream, h: &Handler, base: &str, log: &Mutex<Vec<Req>>) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    if r.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l).unwrap_or(0) == 0 || l.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0usize);
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).unwrap();
    let req = Req { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() };
    let (status, out) = h(&req, base);
    log.lock().unwrap().push(req);
    let resp = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
    let _ = s.flush();
}

/// The token endpoint, and a 401 for API calls without the token; `None` lets the API answer.
fn auth(r: &Req) -> Option<(u16, String)> {
    if r.path == "/token" {
        let ok = r.method == "POST"
            && r.body.contains("grant_type=client_credentials")
            && r.body.contains(&format!("client_secret={SECRET}"))
            && r.body.contains("client_id=cid");
        return Some(if ok {
            (200, json!({"token_type": "Bearer", "expires_in": 3599, "access_token": TOKEN}).to_string())
        } else {
            (400, json!({"error": "invalid_client", "error_description": "AADSTS7000215: Invalid client secret provided."}).to_string())
        });
    }
    if r.header("Authorization") != Some(&format!("Bearer {TOKEN}")) {
        return Some((401, json!({"error": {"code": "Unauthorized", "message": "no token"}}).to_string()));
    }
    None
}

fn set_secret() {
    erpcraft_engine::bc::client::set_secret_override(SECRET);
}

fn demo(base: &str) -> Session {
    set_secret();
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s.run(
        "bc.setup",
        &json!({"tenant_id": "t1", "client_id": "cid", "company_id": COMPANY,
            "api_base_url": format!("{base}/api"), "token_url": format!("{base}/token")}),
    )
    .unwrap();
    s
}

fn get(s: &mut Session, table: &str, key: &str) -> Value {
    s.run("record.get", &json!({"table": table, "key": key})).unwrap()
}

#[test]
fn test_connection_lists_companies() {
    let srv = serve(Arc::new(|r, _| {
        if let Some(a) = auth(r) {
            return a;
        }
        if r.path == format!("{ROOT}/companies") {
            return (200, json!({"value": [{"id": COMPANY, "name": "CRONUS", "displayName": "CRONUS USA, Inc.", "systemVersion": "25.0"}]}).to_string());
        }
        (404, "{}".into())
    }));
    let mut s = demo(&srv.base);
    let r = s.run("bc.test_connection", &json!({})).unwrap();
    assert_eq!(r["companies"][0]["id"], COMPANY, "{r:#}");
    assert_eq!(r["companies"][0]["displayName"], "CRONUS USA, Inc.");
    assert_eq!(r["selected"]["name"], "CRONUS");
    let log = srv.log.lock().unwrap();
    assert_eq!(log[0].path, "/token");
    assert!(log[1].header("Authorization").unwrap().starts_with("Bearer "));
}

#[test]
fn import_customers_pages_truncates_and_updates() {
    let long_name = "Ü".repeat(150);
    let changed = Arc::new(AtomicBool::new(false));
    let ch = changed.clone();
    let srv = serve(Arc::new(move |r, base| {
        if let Some(a) = auth(r) {
            return a;
        }
        let url = format!("{ROOT}/companies({COMPANY})/customers");
        if r.path == url {
            let name = if ch.load(Ordering::SeqCst) { "Adatum Corporation (renamed)" } else { "Adatum Corporation" };
            return (
                200,
                json!({
                    "@odata.context": "x",
                    "value": [
                        {"id": "aaaaaaaa-0000-0000-0000-000000000001", "number": "c-100", "displayName": name,
                         "addressLine1": "192 Market Square", "city": "Atlanta", "postalCode": "31772", "country": "US",
                         "phoneNumber": "555-0100", "email": "robert.townes@contoso.com", "website": "", "blocked": " ",
                         "creditLimit": 0, "taxRegistrationNumber": "", "@odata.etag": "W/\"1\""},
                        {"id": "aaaaaaaa-0000-0000-0000-000000000002", "number": "C-101", "displayName": long_name, "blocked": "Invoice"}
                    ],
                    "@odata.nextLink": format!("{base}{url}?$skiptoken=2")
                })
                .to_string(),
            );
        }
        if r.path == format!("{url}?$skiptoken=2") {
            return (
                200,
                json!({"value": [
                    {"number": "C-102", "displayName": "Trey Research", "blocked": "All", "creditLimit": 5000.5},
                    {"displayName": "no number"}
                ]})
                .to_string(),
            );
        }
        (404, json!({"error": {"code": "BadRequest_NotFound", "message": r.path}}).to_string())
    }));
    let mut s = demo(&srv.base);
    let r = s.run("bc.import", &json!({"entity": "customers"})).unwrap();
    assert_eq!(r["read"], 4, "{r:#}");
    assert_eq!(r["created"], 3, "{r:#}");
    assert_eq!(r["skipped"], 1, "{r:#}");
    assert_eq!(r["errors"].as_array().unwrap().len(), 1);

    let c = get(&mut s, "Customer", "C-100");
    assert_eq!(c["Name"], "Adatum Corporation");
    assert_eq!(c["City"], "Atlanta");
    assert_eq!(c["Country/Region Code"], "US");
    assert_eq!(c["E-Mail"], "robert.townes@contoso.com");
    // Posting groups come from the first customer of the demo company.
    let first = get(&mut s, "Customer", "10000");
    assert_eq!(c["Customer Posting Group"], first["Customer Posting Group"]);
    assert!(!c["Customer Posting Group"].as_str().unwrap().is_empty());

    let c = get(&mut s, "Customer", "C-101");
    assert_eq!(c["Name"].as_str().unwrap().chars().count(), 100);
    assert_eq!(c["Blocked"], "Invoice");
    let c = get(&mut s, "Customer", "C-102");
    assert_eq!(c["Blocked"], "All");
    assert_eq!(c["Credit Limit (LCY)"].as_f64().unwrap(), 5000.5);

    let setup = get(&mut s, "BC Connection Setup", "");
    assert!(!setup["Last Import At"].as_str().unwrap().is_empty());

    // Second run: one record changed in BC.
    changed.store(true, Ordering::SeqCst);
    let r = s.run("bc.import", &json!({"entity": "customers"})).unwrap();
    assert_eq!(r["created"], 0, "{r:#}");
    assert_eq!(r["updated"], 1, "{r:#}");
    assert_eq!(r["unchanged"], 2, "{r:#}");
    assert_eq!(get(&mut s, "Customer", "C-100")["Name"], "Adatum Corporation (renamed)");
    let log = s.run("record.list", &json!({"table": "BC Sync Log"})).unwrap();
    assert_eq!(log["total"], 2);

    // A limit stops after that many records (and says so).
    let r = s.run("bc.import", &json!({"entity": "customers", "limit": 1})).unwrap();
    assert_eq!(r["read"], 1);
    assert_eq!(r["truncated"], true);
}

#[test]
fn import_items_creates_units_of_measure() {
    let srv = serve(Arc::new(|r, _| {
        if let Some(a) = auth(r) {
            return a;
        }
        if r.path == format!("{ROOT}/companies({COMPANY})/items") {
            return (
                200,
                json!({"value": [
                    {"number": "BC-1000", "displayName": "Bicycle", "type": "Inventory", "baseUnitOfMeasureCode": "PCS",
                     "unitPrice": 4000, "unitCost": 350.596, "blocked": false},
                    {"number": "BC-SVC", "displayName": "Installation", "type": "Non_Inventory", "baseUnitOfMeasureCode": "boxx7",
                     "unitPrice": 12.5, "blocked": true}
                ]})
                .to_string(),
            );
        }
        (404, "{}".into())
    }));
    let mut s = demo(&srv.base);
    let r = s.run("bc.import", &json!({"entity": "items"})).unwrap();
    assert_eq!(r["created"], 2, "{r:#}");
    let i = get(&mut s, "Item", "BC-SVC");
    assert_eq!(i["Type"], "Non-Inventory");
    assert_eq!(i["Base Unit of Measure"], "BOXX7");
    assert_eq!(i["Blocked"], true);
    assert_eq!(i["Unit Price"].as_f64().unwrap(), 12.5);
    let uom = get(&mut s, "Unit of Measure", "BOXX7");
    assert_eq!(uom["Code"], "BOXX7");
    let i = get(&mut s, "Item", "BC-1000");
    assert_eq!(i["Unit Cost"].as_f64().unwrap(), 350.596);
    assert!(!i["Inventory Posting Group"].as_str().unwrap().is_empty());
}

#[test]
fn api_401_is_a_clean_error_without_secrets() {
    let srv = serve(Arc::new(|r, _| {
        if let Some(a) = auth(r) {
            return a;
        }
        // A hostile / careless server that echoes credentials back.
        (401, json!({"error": {"code": "Authentication_InvalidCredentials", "message": format!("bad {TOKEN} for {SECRET}")}}).to_string())
    }));
    let mut s = demo(&srv.base);
    let e = s.run("bc.import", &json!({"entity": "customers"})).unwrap_err();
    assert!(e.message.contains("401"), "{}", e.message);
    assert!(e.message.contains("Authentication_InvalidCredentials"), "{}", e.message);
    assert!(!e.message.contains(SECRET) && !e.message.contains(TOKEN), "{}", e.message);
    // Nothing was written: the command rolled back.
    let setup = get(&mut s, "BC Connection Setup", "");
    assert_eq!(setup["Last Import At"], "");

    // A bad secret at the token endpoint is reported the same careful way.
    let e = s.run("bc.setup", &json!({"client_id": "other"})).and_then(|_| s.run("bc.test_connection", &json!({}))).unwrap_err();
    assert!(e.message.contains("invalid_client"), "{}", e.message);
    assert!(!e.message.contains(SECRET), "{}", e.message);

    // The secret is refused as a setup field.
    assert!(s.run("bc.setup", &json!({"client_secret": "x"})).is_err());
}

#[test]
fn next_link_outside_the_api_is_not_followed() {
    let srv = serve(Arc::new(|r, _| {
        if let Some(a) = auth(r) {
            return a;
        }
        (200, json!({"value": [{"number": "X1", "displayName": "x"}], "@odata.nextLink": "http://evil.example/steal"}).to_string())
    }));
    let mut s = demo(&srv.base);
    let e = s.run("bc.import", &json!({"entity": "vendors"})).unwrap_err();
    assert!(e.message.contains("outside the configured API"), "{}", e.message);
    assert!(s.run("record.get", &json!({"table": "Vendor", "key": "X1"})).is_err());
}

#[test]
fn export_customers_posts_new_and_patches_changed() {
    let srv = serve(Arc::new(|r, _| {
        if let Some(a) = auth(r) {
            return a;
        }
        let url = format!("{ROOT}/companies({COMPANY})/customers");
        match r.method.as_str() {
            "GET" if r.path == url => (
                200,
                json!({"value": [{"id": "bbbbbbbb-0000-0000-0000-000000000001", "number": "10000", "displayName": "Old name",
                    "@odata.etag": "W/\"JzQ0OzE7MDsn\""}]})
                .to_string(),
            ),
            "POST" if r.path == url => (201, r.body.clone()),
            "PATCH" if r.path == format!("{url}(bbbbbbbb-0000-0000-0000-000000000001)") => (200, r.body.clone()),
            _ => (400, json!({"error": {"code": "BadRequest", "message": "unexpected"}}).to_string()),
        }
    }));
    let mut s = demo(&srv.base);
    let total = s.run("record.list", &json!({"table": "Customer"})).unwrap()["total"].as_i64().unwrap();
    let r = s.run("bc.export", &json!({"entity": "customers"})).unwrap();
    assert_eq!(r["updated"], 1, "{r:#}");
    assert_eq!(r["created"].as_i64().unwrap(), total - 1, "{r:#}");
    let log = srv.log.lock().unwrap();
    let patch = log.iter().find(|q| q.method == "PATCH").unwrap();
    assert_eq!(patch.header("If-Match"), Some("W/\"JzQ0OzE7MDsn\""));
    let body: Value = serde_json::from_str(&patch.body).unwrap();
    assert!(body.get("number").is_none());
    assert_ne!(body["displayName"], "Old name");
    let post = log.iter().find(|q| q.method == "POST" && q.path != "/token").unwrap();
    let body: Value = serde_json::from_str(&post.body).unwrap();
    assert!(body["number"].is_string() && body["blocked"].is_string());
}
