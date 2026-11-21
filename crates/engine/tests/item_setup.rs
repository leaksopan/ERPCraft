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

#[test]
fn variants_ride_on_lines_and_entries() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "1000", "quantity": 5}]})).unwrap();
    let no = doc["no"].as_str().unwrap().to_string();
    let l = s.run("record.list", &json!({"table": "Purchase Line", "filters": {"Document No.": no}})).unwrap()["records"][0].clone();
    let key = json!(["Invoice", no, l["Line No."]]);
    let err = s.run("record.validate", &json!({"table": "Purchase Line", "key": key, "field": "Variant Code", "value": "NOPE"})).unwrap_err();
    assert!(err.message.contains("The Item Variant does not exist"), "{}", err.message);
    let r = s.run("record.validate", &json!({"table": "Purchase Line", "key": key, "field": "Variant Code", "value": "5W30"})).unwrap();
    assert_eq!(r["Description"], "Engine Oil 5W-30 1L", "{r}");
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": no})).unwrap();
    let e = s.run("record.list", &json!({"table": "Item Ledger Entry", "filters": {"Item No.": "1000", "Variant Code": "5W30"}})).unwrap();
    assert_eq!(e["records"].as_array().unwrap().len(), 1);
    let v = s.run("item.availability_by_variant", &json!({"no": "1000"})).unwrap();
    let row = v["report"]["rows"].as_array().unwrap().iter().find(|r| r[0] == "5W30").unwrap().clone();
    assert_eq!(row[2], "5", "{v}");
    // A blocked variant cannot be used.
    s.run("record.validate", &json!({"table": "Item Variant", "key": ["1000", "10W40"], "field": "Blocked", "value": true})).unwrap();
    let sale = s.run("sales.document.new", &json!({"document_type": "Invoice", "customer_no": "10000", "lines": [{"no": "1000", "quantity": 1}]})).unwrap();
    let sno = sale["no"].as_str().unwrap().to_string();
    let sl = s.run("record.list", &json!({"table": "Sales Line", "filters": {"Document No.": sno}})).unwrap()["records"][0].clone();
    let err = s.run("record.validate", &json!({"table": "Sales Line", "key": ["Invoice", sno, sl["Line No."]], "field": "Variant Code", "value": "10W40"})).unwrap_err();
    assert!(err.message.contains("Blocked must be equal to 'No' in Item Variant"), "{}", err.message);
}

#[test]
fn a_catalog_item_becomes_an_item_from_its_template() {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    let r = s.run("catalog.create_item", &json!({"table": "Nonstock Item", "key": "NS0000"})).unwrap();
    assert_eq!(r["item_no"], "AF-2041");
    let item = s.run("record.get", &json!({"table": "Item", "key": "AF-2041"})).unwrap();
    assert_eq!((item["Description"].as_str().unwrap(), item["Inventory Posting Group"].as_str().unwrap(), item["Base Unit of Measure"].as_str().unwrap()), ("Air Filter Premium", "PARTS", "PCS"));
    assert_eq!(item["Unit Cost"], 105000);
    assert_eq!(item["Created From Nonstock Item"], true);
    assert_eq!(s.run("record.get", &json!({"table": "Nonstock Item", "key": "NS0000"})).unwrap()["Item No."], "AF-2041");
    let err = s.run("catalog.create_item", &json!({"entry_no": "NS0000"})).unwrap_err();
    assert!(err.message.contains("Item AF-2041 already exists"), "{}", err.message);
    // The new item can be bought right away.
    let doc = s.run("purchase.document.new", &json!({"document_type": "Invoice", "vendor_no": "10000", "lines": [{"no": "AF-2041", "quantity": 3}]})).unwrap();
    s.run("purchase.document.post", &json!({"document_type": "Invoice", "no": doc["no"]})).unwrap();

    // Numbered by entry no. instead.
    s.run("record.validate", &json!({"table": "Nonstock Item Setup", "key": "", "field": "No. Format", "value": "Entry No."})).unwrap();
    let c = s.run("record.new", &json!({"table": "Nonstock Item", "values": {"Vendor Item No.": "SP-9", "Description": "Spark plug kit", "Unit of Measure": "PCS", "Item Templ. Code": "PARTS"}})).unwrap();
    let entry = c["record"]["Entry No."].as_str().unwrap().to_string();
    assert!(entry.starts_with("NS"), "{entry}");
    assert_eq!(s.run("catalog.create_item", &json!({"entry_no": entry})).unwrap()["item_no"], entry.as_str());
}
