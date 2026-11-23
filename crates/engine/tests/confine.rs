//! RFC-0015 confined file access invariant tests: read/write root separation, sibling-prefix
//! rejection, `..` traversal rejection, and command-level enforcement on print & attachments.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use erpcraft_engine::{Confine, Session};
use serde_json::json;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("erpcraft-confine-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn requires_both_existing_directory_roots() {
    let base = temp_dir("roots");
    let read = base.join("read");
    let write = base.join("write");
    let missing = base.join("does-not-exist");
    let file = base.join("not-a-dir.txt");
    std::fs::create_dir_all(&read).unwrap();
    std::fs::create_dir_all(&write).unwrap();
    std::fs::write(&file, b"x").unwrap();

    assert!(Confine::require_both(&[], std::slice::from_ref(&write)).is_err());
    assert!(Confine::require_both(std::slice::from_ref(&read), &[]).is_err());
    assert!(Confine::require_both(std::slice::from_ref(&missing), std::slice::from_ref(&write)).is_err());
    assert!(Confine::require_both(std::slice::from_ref(&file), std::slice::from_ref(&write)).is_err());
    assert!(Confine::require_both(&[read], &[write]).is_ok());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enforces_read_write_separation_and_blocks_sibling_prefix_and_traversal() {
    let base = temp_dir("escape");
    let read = base.join("sandbox");
    let sibling = base.join("sandbox-other");
    let write = base.join("out");
    std::fs::create_dir_all(&read).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::create_dir_all(&write).unwrap();

    let ok_in = read.join("note.txt");
    let sib_in = sibling.join("secret.txt");
    std::fs::write(&ok_in, b"hello").unwrap();
    std::fs::write(&sib_in, b"secret").unwrap();

    let confine = Confine::require_both(std::slice::from_ref(&read), std::slice::from_ref(&write)).unwrap();

    // Reading inside read root works; sibling prefix and `..` traversal are refused.
    assert!(confine.check_read(&ok_in).is_ok());
    assert!(confine.check_read(&sib_in).is_err());
    assert!(confine.check_read(&read.join("..").join("sandbox-other").join("secret.txt")).is_err());

    // Writing into write root works (and reading from write root is allowed); writing into read-only
    // root, sibling prefix, or via `..` traversal is refused.
    let ok_out = write.join("invoice.pdf");
    assert!(confine.check_write(&ok_out).is_ok());
    std::fs::write(&ok_out, b"%PDF").unwrap();
    assert!(confine.check_read(&ok_out).is_ok());
    assert!(confine.check_write(&read.join("blocked.pdf")).is_err());
    assert!(confine.check_write(&sibling.join("blocked.pdf")).is_err());
    assert!(confine.check_write(&write.join("..").join("sandbox-other").join("escape.pdf")).is_err());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn commands_enforce_confinement_on_attachments_and_printing() {
    let base = temp_dir("cmds");
    let read = base.join("read");
    let write = base.join("write");
    let outside = base.join("outside");
    std::fs::create_dir_all(&read).unwrap();
    std::fs::create_dir_all(&write).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let allowed_file = read.join("spec.txt");
    let secret_file = outside.join("secret.txt");
    std::fs::write(&allowed_file, b"allowed").unwrap();
    std::fs::write(&secret_file, b"nope").unwrap();

    let confine = Confine::require_both(std::slice::from_ref(&read), std::slice::from_ref(&write)).unwrap();
    let company = write.join("company.erpcraft.json");
    let mut s = Session::open_source_confined(company.to_str().unwrap(), confine).unwrap();
    s.run("company.init", &json!({"demo": true})).unwrap();

    // Attaching a file outside read roots fails; inside read roots succeeds.
    assert!(
        s.run("attachment.attach_file", &json!({"table": "Customer", "key": "10000", "path": secret_file.to_str().unwrap()}))
            .is_err()
    );
    assert!(
        s.run("attachment.attach_file", &json!({"table": "Customer", "key": "10000", "path": allowed_file.to_str().unwrap()}))
            .is_ok()
    );

    // Printing to a path outside write roots (or into read-only root) fails; inside write root succeeds.
    let orders = s.run("record.list", &json!({"table": "Sales Header", "filters": {"Document Type": "Order"}})).unwrap();
    let order_no = orders["records"][0]["No."].as_str().unwrap().to_string();
    let order_key = json!(["Order", order_no]);
    assert!(
        s.run("document.print", &json!({"table": "Sales Header", "key": order_key, "path": outside.join("a.pdf").to_str().unwrap()}))
            .is_err()
    );
    assert!(
        s.run("document.print", &json!({"table": "Sales Header", "key": order_key, "path": read.join("a.pdf").to_str().unwrap()}))
            .is_err()
    );
    let ok_pdf = write.join("order.pdf");
    assert!(
        s.run("document.print", &json!({"table": "Sales Header", "key": order_key, "path": ok_pdf.to_str().unwrap()}))
            .is_ok()
    );
    assert!(ok_pdf.exists());

    let _ = std::fs::remove_dir_all(&base);
}
