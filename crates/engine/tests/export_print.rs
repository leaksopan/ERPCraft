//! Integration tests for `craft-csv` / `craft-xlsx` data import & export and `craft-print`
//! system printing commands, including RFC-0015 confinement and transactional rollback.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use erpcraft_engine::{Confine, Session};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let dir = std::env::temp_dir().join(format!("erpcraft-export-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn demo() -> Result<Session, Box<dyn std::error::Error>> {
    let mut s = Session::in_memory();
    s.run("company.init", &json!({"demo": true}))?;
    Ok(s)
}

#[test]
fn csv_export_and_import_round_trip_with_transactional_rollback() -> TestResult {
    let mut s = demo()?;

    // Export customers with explicit fields.
    let exp = s.run(
        "data.export_csv",
        &json!({"table": "Customer", "fields": ["No.", "Name", "City", "Credit Limit (LCY)"]}),
    )?;
    let csv_text = exp["csv"].as_str().ok_or("csv string")?;
    assert!(exp["rows"].as_u64().unwrap_or(0) > 0);
    assert!(csv_text.starts_with("No.,Name,City,Credit Limit (LCY)\r\n"));

    // Import semicolon-delimited CSV with quoted field containing semicolon (auto-sniffed by craft-csv).
    let before = s.run("record.list", &json!({"table": "Customer"}))?["total"].as_u64().ok_or("total")?;
    let imp = s.run(
        "data.import_csv",
        &json!({
            "table": "Customer",
            "csv": "No.;Name;City;Credit Limit (LCY)\r\nCUST-CSV1;\"Alpha; Beta Co.\";Jakarta;2500000\r\nCUST-CSV2;Gamma Parts;Bandung;500000\r\n"
        }),
    )?;
    assert_eq!(imp["imported"], 2);
    let after = s.run("record.list", &json!({"table": "Customer"}))?["total"].as_u64().ok_or("total")?;
    assert_eq!(after, before + 2);
    let c1 = s.run("record.get", &json!({"table": "Customer", "key": "CUST-CSV1"}))?;
    assert_eq!(c1["Name"], "Alpha; Beta Co.");

    // Transactional rollback: row 2 duplicates CUST-CSV1, so row 1 (CUST-CSV3) must NOT persist.
    assert!(
        s.run(
            "data.import_csv",
            &json!({
                "table": "Customer",
                "csv": "No.,Name\r\nCUST-CSV3,Should Roll Back\r\nCUST-CSV1,Duplicate Key\r\n"
            }),
        )
        .is_err()
    );
    assert!(s.run("record.get", &json!({"table": "Customer", "key": "CUST-CSV3"})).is_err());
    Ok(())
}

#[test]
fn xlsx_export_and_import_round_trip() -> TestResult {
    let mut s = demo()?;
    s.run("record.new", &json!({"table": "Location", "values": {"Code": "XLSX-1", "Name": "Bali Hub", "Use As In-Transit": false}}))?;
    s.run("record.new", &json!({"table": "Location", "values": {"Code": "XLSX-2", "Name": "Transit Sea", "Use As In-Transit": true}}))?;

    // Export only the newly created locations to inline xlsx_base64 and round-trip into a fresh session.
    let exp = s.run(
        "data.export_xlsx",
        &json!({
            "table": "Location",
            "filters": {"Code": "XLSX-*"},
            "fields": ["Code", "Name", "Use As In-Transit"],
            "sheet_name": "Warehouses"
        }),
    )?;
    let b64 = exp["xlsx_base64"].as_str().ok_or("xlsx_base64")?;
    assert_eq!(exp["rows"], 2);
    assert!(exp["bytes"].as_u64().unwrap_or(0) > 100);
    assert_eq!(exp["sheet_name"], "Warehouses");

    let mut fresh = demo()?;
    let imp = fresh.run(
        "data.import_xlsx",
        &json!({"table": "Location", "xlsx_base64": b64, "sheet": "Warehouses"}),
    )?;
    assert_eq!(imp["imported"], 2);
    let loc1 = fresh.run("record.get", &json!({"table": "Location", "key": "XLSX-1"}))?;
    let loc2 = fresh.run("record.get", &json!({"table": "Location", "key": "XLSX-2"}))?;
    assert_eq!(loc1["Name"], "Bali Hub");
    assert_eq!(loc1["Use As In-Transit"], false);
    assert_eq!(loc2["Name"], "Transit Sea");
    assert_eq!(loc2["Use As In-Transit"], true);
    Ok(())
}

#[test]
fn export_and_import_enforce_rfc0015_confinement() -> TestResult {
    let base = temp_dir("confine")?;
    let read = base.join("read");
    let write = base.join("write");
    let outside = base.join("outside");
    std::fs::create_dir_all(&read)?;
    std::fs::create_dir_all(&write)?;
    std::fs::create_dir_all(&outside)?;

    let confine = Confine::require_both(std::slice::from_ref(&read), std::slice::from_ref(&write))?;
    let company = write.join("company.erpcraft.json");
    let mut s = Session::open_source_confined(company.to_str().ok_or("utf-8")?, confine)?;
    s.run("company.init", &json!({"demo": true}))?;

    // Exporting outside write roots or into read-only root is refused; inside write root succeeds.
    assert!(
        s.run("data.export_csv", &json!({"table": "Location", "path": outside.join("loc.csv").to_str().ok_or("utf-8")?}))
            .is_err()
    );
    assert!(
        s.run("data.export_xlsx", &json!({"table": "Location", "path": read.join("loc.xlsx").to_str().ok_or("utf-8")?}))
            .is_err()
    );
    let ok_csv = write.join("loc.csv");
    let ok_xlsx = write.join("loc.xlsx");
    s.run("data.export_csv", &json!({"table": "Location", "path": ok_csv.to_str().ok_or("utf-8")?}))?;
    s.run("data.export_xlsx", &json!({"table": "Location", "path": ok_xlsx.to_str().ok_or("utf-8")?}))?;
    assert!(ok_csv.exists());
    assert!(ok_xlsx.exists());

    // Importing from outside read roots is refused; inside read roots succeeds.
    let outside_csv = outside.join("bad.csv");
    let allowed_csv = read.join("ok.csv");
    std::fs::write(&outside_csv, b"Code,Name\r\nOUT,Outside\r\n")?;
    std::fs::write(&allowed_csv, b"Code,Name\r\nSANDBOX,Sandbox Warehouse\r\n")?;
    assert!(
        s.run("data.import_csv", &json!({"table": "Location", "path": outside_csv.to_str().ok_or("utf-8")?}))
            .is_err()
    );
    let r = s.run("data.import_csv", &json!({"table": "Location", "path": allowed_csv.to_str().ok_or("utf-8")?}))?;
    assert_eq!(r["imported"], 1);
    assert_eq!(s.run("record.get", &json!({"table": "Location", "key": "SANDBOX"}))?["Name"], "Sandbox Warehouse");

    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}

#[test]
fn print_printers_and_system_print_dry_run_work() -> TestResult {
    let mut s = demo()?;
    let printers = s.run("print.printers", &json!({}))?;
    assert_eq!(printers["supported"], true);
    assert!(printers["printers"].is_array());

    let orders = s.run("record.list", &json!({"table": "Sales Header", "filters": {"Document Type": "Order"}}))?;
    let order_no = orders["records"][0]["No."].as_str().ok_or("order No.")?.to_string();
    let dry = s.run(
        "document.print_system",
        &json!({"table": "Sales Header", "key": ["Order", order_no], "copies": 2, "dry_run": true}),
    )?;
    assert_eq!(dry["dry_run"], true);
    assert!(dry["pages"].as_u64().unwrap_or(0) >= 1);
    assert!(!dry["preview"].as_array().ok_or("preview array")?.is_empty());
    Ok(())
}
