#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! PostgreSQL store, end to end. Runs when `ERPCRAFT_TEST_DATABASE_URL` points at a server (the
//! database is created if missing); each test uses its own schema and drops it afterwards.

use erpcraft_engine::{ErrorKind, Session};
use serde_json::json;

fn base_url() -> Option<String> {
    std::env::var("ERPCRAFT_TEST_DATABASE_URL").ok().filter(|s| !s.is_empty())
}

/// A schema of its own for one test; dropped on drop.
struct TestSchema {
    url: String,
    base: String,
    schema: String,
}

impl TestSchema {
    fn new(name: &str) -> Option<Self> {
        let base = base_url()?;
        let schema = format!("erpcraft_test_{name}_{}", std::process::id());
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{sep}schema={schema}");
        let t = Self { url, base, schema };
        t.drop_schema();
        Some(t)
    }

    fn open(&self, user: &str) -> Session {
        let mut s = Session::open_source(&self.url).unwrap();
        s.set_user(user);
        s
    }

    fn drop_schema(&self) {
        // The base URL may itself carry `?schema=`; the server only needs the connection part.
        let conn = self.base.split('?').next().unwrap_or(&self.base);
        if let Ok(mut c) = postgres::Client::connect(conn, postgres::NoTls) {
            let _ = c.batch_execute(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", self.schema));
        }
    }
}

impl Drop for TestSchema {
    fn drop(&mut self) {
        self.drop_schema();
    }
}

macro_rules! schema_or_skip {
    ($name:literal) => {
        match TestSchema::new($name) {
            Some(t) => t,
            None => {
                eprintln!("skipped: set ERPCRAFT_TEST_DATABASE_URL to run PostgreSQL tests");
                return;
            }
        }
    };
}

#[test]
fn demo_company_round_trips_through_postgres() {
    let t = schema_or_skip!("roundtrip");
    {
        let mut a = t.open("ADMIN");
        a.run("company.init", &json!({"demo": true})).unwrap();
    }
    let mut b = t.open("ADMIN");
    assert!(b.source().starts_with("postgres"), "{}", b.source());
    assert_eq!(b.run("record.list", &json!({"table": "Customer", "limit": 0})).unwrap()["total"], 5);
    assert_eq!(b.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
    let item = b.run("record.get", &json!({"table": "Item", "key": "1000"})).unwrap();
    assert_eq!(item["Inventory"], json!(34));
    // Posting writes headers, lines and ledger entries in one transaction.
    let r = b.run("sales.document.new", &json!({"customer_no": "30000", "lines": [{"no": "1100", "quantity": 1}]})).unwrap();
    let no = r["no"].as_str().unwrap().to_string();
    b.run("sales.document.post", &json!({"no": no})).unwrap();
    let mut c = t.open("ADMIN");
    assert_eq!(c.run("report.trial_balance", &json!({})).unwrap()["balanced"], true);
    assert!(c.run("record.get", &json!({"table": "Sales Header", "key": ["Order", no]})).is_err());
}

#[test]
fn sessions_see_each_other_and_conflicts_are_refused() {
    let t = schema_or_skip!("concurrency");
    let mut a = t.open("ADMIN");
    a.run("company.init", &json!({"demo": false})).unwrap();
    let mut b = t.open("ADMIN");

    // A creates a customer; B sees it on its next command.
    let c = a.run("record.new", &json!({"table": "Customer", "values": {"Name": "Shared Garage"}})).unwrap();
    let no = c["record"]["No."].as_str().unwrap().to_string();
    let seen = b.run("record.get", &json!({"table": "Customer", "key": no})).unwrap();
    assert_eq!(seen["Name"], "Shared Garage");

    // A changes it; B's change based on the old version is refused, then works after refresh.
    a.run("record.validate", &json!({"table": "Customer", "key": no, "field": "Name", "value": "Changed by A"})).unwrap();
    let mut stale = erpcraft_engine::Session::open_source(&t.url).unwrap();
    stale.set_user("ADMIN");
    a.run("record.validate", &json!({"table": "Customer", "key": no, "field": "City", "value": "Denpasar"})).unwrap();
    // `stale` loaded before A's City change but refreshes before its command, so it still works.
    stale.run("record.validate", &json!({"table": "Customer", "key": no, "field": "Phone No.", "value": "123"})).unwrap();
    let now = a.run("record.get", &json!({"table": "Customer", "key": no})).unwrap();
    assert_eq!(now["City"], "Denpasar");
    assert_eq!(now["Phone No."], "123");

    // A true conflict: a write based on a version another session replaced in between.
    let mut store = erpcraft_engine::store::postgres::Postgres::connect(&t.url, erpcraft_engine::schema::base_app()).unwrap();
    let mut db = erpcraft_engine::store::Store::load(&mut store, erpcraft_engine::schema::base_app()).unwrap();
    a.run("record.validate", &json!({"table": "Customer", "key": no, "field": "Name", "value": "A wins"})).unwrap();
    let ct = erpcraft_engine::schema::base_app().table("Customer").unwrap();
    let key = erpcraft_engine::meta::Key(vec![erpcraft_engine::meta::Value::text(&no)]);
    let mut row = db.get("Customer", &key).cloned().unwrap();
    row.set("Name", erpcraft_engine::meta::Value::text("B loses"));
    db.modify(ct, row).unwrap();
    let changes = db.take_changes();
    let e = erpcraft_engine::store::Store::save(&mut store, erpcraft_engine::schema::base_app(), &mut db, &changes).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Conflict, "{e}");
    assert!(e.message.contains("Another user has modified the record"), "{e}");
    assert_eq!(b.run("record.get", &json!({"table": "Customer", "key": no})).unwrap()["Name"], "A wins");

    // Deletions reach other sessions.
    a.run("record.delete", &json!({"table": "Customer", "key": no})).unwrap();
    assert!(b.run("record.get", &json!({"table": "Customer", "key": no})).is_err());
}

#[test]
fn permissions_and_work_date_are_per_user() {
    let t = schema_or_skip!("permissions");
    let mut admin = t.open("ADMIN");
    admin.run("company.init", &json!({"demo": false})).unwrap();
    // A read-only user.
    admin.run("record.new", &json!({"table": "User", "values": {"User Name": "VIEWER1", "Full Name": "Read Only"}})).unwrap();
    admin.run("record.new", &json!({"table": "Access Control", "values": {"User Name": "VIEWER1", "Role ID": "VIEWER"}})).unwrap();
    admin.run("session.workdate", &json!({"date": "2026-01-31"})).unwrap();

    let mut viewer = t.open("viewer1");
    assert!(viewer.run("record.list", &json!({"table": "Customer"})).is_ok());
    let e = viewer.run("record.new", &json!({"table": "Customer", "values": {"Name": "Nope"}})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Permission, "{e}");
    let e = viewer.run("sales.document.new", &json!({})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Permission, "{e}");

    let mut stranger = t.open("NOBODY");
    assert_eq!(stranger.run("record.list", &json!({"table": "Customer"})).unwrap_err().kind, ErrorKind::Permission);

    // The work date is remembered for ADMIN only.
    let again = t.open("ADMIN");
    assert_eq!(again.work_date().to_string(), "2026-01-31");
    assert_ne!(viewer.work_date().to_string(), "2026-01-31");
}
