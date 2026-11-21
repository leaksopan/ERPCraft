#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Item Statistics, Item Entry Statistics, Item Turnover and Items by Location.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn row<'a>(rows: &'a Value, first: &str) -> &'a Vec<Value> {
    rows.as_array().unwrap().iter().find(|r| r[0] == first).and_then(Value::as_array).unwrap()
}

#[test]
fn item_statistics_follow_the_posted_sale() {
    let mut s = demo();
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 2}]})).unwrap();
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();
    let st = s.run("item.statistics", &json!({"table": "Item", "key": "1000"})).unwrap();
    let sales = &st["report"]["sections"][0];
    let net = row(&sales["rows"], "Net Sales (LCY)");
    assert!(net[1].as_str().unwrap().parse::<f64>().unwrap() > 0.0, "{st}");
    assert_eq!(net[1], net[4], "this period's sale is also its lifetime sale");
    let margin: f64 = row(&sales["rows"], "Gross Margin (%)")[1].as_str().unwrap().parse().unwrap();
    assert!(margin > 0.0 && margin < 100.0, "{margin} {st}");

    let es = s.run("item.entry_statistics", &json!({"no": "1000"})).unwrap();
    let sale = row(&es["report"]["rows"], "Sales");
    assert_eq!(sale[3], "2", "{es}");

    let t = s.run("item.turnover", &json!({"no": "1000"})).unwrap();
    let rows = t["report"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    assert!(rows[11][4].as_str().unwrap().parse::<i64>().unwrap() >= 2, "this month holds the sale");

    let l = s.run("item.by_location", &json!({"no": "1000"})).unwrap();
    assert!(l["report"]["rows"].as_array().unwrap().len() >= 1);
    assert!(s.run("item.turnover", &json!({"no": "NOPE"})).is_err());
}
