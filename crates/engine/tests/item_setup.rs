#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Item units of measure, variants, attributes and the item card links.

use erpcraft_engine::Session;
use serde_json::{Value, json};

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

fn d(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap_or(0.0)).or_else(|| v.as_f64()).unwrap_or(0.0)
}

fn inventory(s: &mut Session, item: &str) -> f64 {
    d(&s.run("record.get", &json!({"table": "Item", "key": item})).unwrap()["Inventory"])
}

#[test]
fn selling_by_the_box_prices_and_moves_stock_in_the_base_unit() {
    let mut s = demo();
    let base = s.run("record.get", &json!({"table": "Item Unit of Measure", "key": ["1000", "PCS"]})).unwrap();
    assert_eq!(d(&base["Qty. per Unit of Measure"]), 1.0, "the base unit exists with 1");
    let doc = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let line = &s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document No.": no}})).unwrap()["records"][0];
    let key = json!(["Invoice", no, line["Line No."]]);
    s.run("record.validate", &json!({"table": "Sales Line", "key": key, "field": "Unit of Measure Code", "value": "BOX"})).unwrap();
    let line = s.run("record.get", &json!({"table": "Sales Line", "key": key})).unwrap();
    assert_eq!((d(&line["Qty. per Unit of Measure"]), d(&line["Quantity (Base)"]), d(&line["Unit Price"])), (12.0, 12.0, 450_000.0 * 12.0));
    assert!(s.run("record.validate", &json!({"table": "Sales Line", "key": key, "field": "Unit of Measure Code", "value": "SET"})).unwrap_err().message.contains("Item Unit of Measure does not exist"));
    let before = inventory(&mut s, "1000");
    s.run("sales.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    assert_eq!(inventory(&mut s, "1000"), before - 12.0, "a box is 12 pieces");
}

#[test]
fn variants_attributes_and_the_base_unit_rule() {
    let mut s = demo();
    s.run("record.new", &json!({"table": "Item Variant", "values": {"Item No.": "1200", "Code": "WINTER", "Description": "Winter tire"}})).unwrap();
    let a = s.run("record.new", &json!({"table": "Item Attribute", "values": {"Name": "Viscosity", "Type": "Option"}})).unwrap();
    let id = a["record"]["ID"].as_i64().unwrap();
    assert_eq!(id, 1);
    let v = s.run("record.new", &json!({"table": "Item Attribute Value", "values": {"Attribute ID": id, "Value": "5W-30"}})).unwrap();
    s.run("record.new", &json!({"table": "Item Attribute Value Mapping", "values": {"Table ID": 27, "No.": "1000", "Item Attribute ID": id, "Item Attribute Value ID": v["record"]["ID"]}})).unwrap();
    let m = s.run("record.list", &json!({"page": "item-attribute-mapping", "filters": {"No.": "1000"}})).unwrap();
    assert_eq!((m["records"][0]["Attribute"].as_str(), m["records"][0]["Value"].as_str()), (Some("Viscosity"), Some("5W-30")));
    let err = s.run("record.validate", &json!({"table": "Item Unit of Measure", "key": ["1000", "PCS"], "field": "Qty. per Unit of Measure", "value": 2}));
    assert!(err.unwrap_err().message.contains("must be 1"));
    let page = s.run("page.get", &json!({"id": "item-card"})).unwrap_or(json!(null));
    let _ = page;
}
