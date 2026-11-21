#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Document attachments: attach a file to a card or document, view it back, Attach as PDF.

use erpcraft_engine::Session;
use serde_json::json;

fn demo() -> Session {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true})).unwrap();
    s
}

#[test]
fn a_file_attached_to_a_customer_comes_back_unchanged() {
    let mut s = demo();
    let dir = std::env::temp_dir().join("erpcraft-attach-test");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("contract scan.png");
    let bytes: Vec<u8> = (0u16..3000).map(|i| (i % 251) as u8).collect();
    std::fs::write(&src, &bytes).unwrap();
    let r = s.run("attachment.attach_file", &json!({"table": "Customer", "key": "10000", "path": src.display().to_string()})).unwrap();
    assert_eq!(r["id"], 1);
    let list = s.run("record.list", &json!({"table": "Document Attachment", "filters": {"Table ID": "18", "No.": "10000"}})).unwrap();
    let a = &list["records"][0];
    assert_eq!((a["File Name"].as_str(), a["File Extension"].as_str(), a["File Type"].as_str()), (Some("contract scan"), Some("png"), Some("Image")));
    assert_eq!(a["Document Type"], "Quote", "cards file under BC's first document type");
    let v = s.run("attachment.view", &json!({"key": [18, "10000", "Quote", 0, 1]})).unwrap();
    assert_eq!(std::fs::read(v["open_file"].as_str().unwrap()).unwrap(), bytes);
    let second = s.run("attachment.attach_file", &json!({"table": "Customer", "key": "10000", "path": src.display().to_string()})).unwrap();
    assert_eq!(second["id"], 2);
    assert!(s.run("attachment.attach_file", &json!({"table": "Customer", "key": "10000", "path": dir.join("missing.pdf").display().to_string()})).is_err());
}

#[test]
fn attach_as_pdf_files_the_print_under_the_document() {
    let mut s = demo();
    s.run("document.attach_as_pdf", &json!({"table": "Sales Header", "key": ["Order", "S-ORD1002"]})).unwrap();
    let list = s.run("record.list", &json!({"table": "Document Attachment", "filters": {"No.": "S-ORD1002"}})).unwrap();
    let a = &list["records"][0];
    assert_eq!((a["File Extension"].as_str(), a["File Type"].as_str(), a["Document Type"].as_str()), (Some("pdf"), Some("PDF"), Some("Order")));
    assert!(a["File Name"].as_str().unwrap().starts_with("Order Confirmation"));
}
