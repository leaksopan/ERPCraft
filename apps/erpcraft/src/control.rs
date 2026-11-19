//! The control channel: `erpcraft --control [port]` lets agents and scripts drive the running app
//! over newline-delimited JSON on `127.0.0.1:<port>` (protocol in `erpcraft_mcp::control`,
//! docs/control.md).
//!
//! One thread accepts connections; each connection gets a thread that reads requests and hands
//! them to the UI thread as [`Job`]s (an mpsc channel plus a repaint request). The UI thread runs
//! them in [`Control::poll`] before drawing the frame, because the [`Session`] lives in [`ErpApp`]
//! on that thread, and answers through a channel per request. Connection threads never touch the
//! session, and they give up waiting after a timeout, so a stuck UI can't hang a client forever.
//!
//! [`Session`]: erpcraft_engine::Session

use std::io::{self, BufReader, BufWriter, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use erpcraft_engine::{EngineError, commands, store};
use erpcraft_mcp::control::{self, MAX_LINE_BYTES};
use erpcraft_mcp::jsonrpc::{Line, read_line_bounded};
use erpcraft_ui_egui::{Dialog, ErpApp, View};
use serde_json::{Value, json};

/// How long a connection waits for the UI thread to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a connection waits for a screenshot.
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(5);
/// Connections served at once; more are told so and closed.
const MAX_CONNECTIONS: usize = 16;

/// Wakes the UI thread (`egui::Context::request_repaint` in the app, a no-op in tests).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the UI thread sends back for a request.
pub enum Reply {
    Done(Result<Value, EngineError>),
    /// A screenshot of the window; the connection thread encodes and writes it.
    Image(Arc<egui::ColorImage>),
}

/// A request waiting for the UI thread.
pub struct Job {
    pub method: String,
    pub params: Value,
    pub reply: mpsc::Sender<Reply>,
}

/// What [`dispatch`] decided.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Reply(Result<Value, EngineError>),
    /// Take a screenshot of the next frame (needs the egui context, so the caller does it).
    Screenshot,
}

/// Binds the control port on the loopback interface only.
pub fn listen(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

/// Starts accepting connections on `listener`; requests arrive on the returned receiver.
pub fn start(listener: TcpListener, wake: Waker) -> io::Result<mpsc::Receiver<Job>> {
    let (tx, rx) = mpsc::channel();
    let active = Arc::new(AtomicUsize::new(0));
    thread::Builder::new().name("erpcraft-control".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                let mut s = stream;
                let line = control::reply_line(&Value::Null, Err(EngineError::refused(format!("too many control connections (at most {MAX_CONNECTIONS})"))));
                let _ = s.write_all(format!("{line}\n").as_bytes());
                continue;
            }
            active.fetch_add(1, Ordering::SeqCst);
            let (tx, wake, active2) = (tx.clone(), wake.clone(), active.clone());
            let spawned = thread::Builder::new().name("erpcraft-control-conn".into()).spawn(move || {
                serve_connection(stream, &tx, &wake);
                active2.fetch_sub(1, Ordering::SeqCst);
            });
            if spawned.is_err() {
                active.fetch_sub(1, Ordering::SeqCst);
            }
        }
    })?;
    Ok(rx)
}

fn serve_connection(stream: TcpStream, jobs: &mpsc::Sender<Job>, wake: &Waker) {
    let _ = stream.set_nodelay(true);
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    loop {
        let line = match read_line_bounded(&mut reader, MAX_LINE_BYTES) {
            Ok(Some(l)) => l,
            Ok(None) | Err(_) => return,
        };
        let reply = match line {
            Line::Text(t) if t.trim().is_empty() => continue,
            Line::Text(t) => handle_line(&t, jobs, wake),
            Line::TooLong => control::reply_line(&Value::Null, Err(EngineError::invalid(format!("request longer than {MAX_LINE_BYTES} bytes")))),
            Line::NotUtf8 => control::reply_line(&Value::Null, Err(EngineError::invalid("request is not valid UTF-8"))),
        };
        if writer.write_all(reply.as_bytes()).and_then(|()| writer.write_all(b"\n")).and_then(|()| writer.flush()).is_err() {
            return;
        }
    }
}

/// Answers one request line (on a connection thread).
pub fn handle_line(line: &str, jobs: &mpsc::Sender<Job>, wake: &Waker) -> String {
    let req = match control::parse_request(line) {
        Ok(r) => r,
        Err((id, e)) => return control::reply_line(&id, Err(e)),
    };
    let result = match req.method.as_str() {
        "ping" => Ok(json!({"pong": true, "app": "erpcraft", "version": env!("CARGO_PKG_VERSION")})),
        "ui.screenshot" => screenshot(&req.params, jobs, wake),
        _ => ask_ui(&req.method, &req.params, jobs, wake, REPLY_TIMEOUT).and_then(|r| match r {
            Reply::Done(r) => r,
            Reply::Image(_) => Err(EngineError::internal("unexpected screenshot")),
        }),
    };
    control::reply_line(&req.id, result)
}

fn ask_ui(method: &str, params: &Value, jobs: &mpsc::Sender<Job>, wake: &Waker, timeout: Duration) -> Result<Reply, EngineError> {
    let (tx, rx) = mpsc::channel();
    jobs.send(Job { method: method.to_string(), params: params.clone(), reply: tx }).map_err(|_| EngineError::io("the app is closing"))?;
    wake();
    rx.recv_timeout(timeout).map_err(|e| match e {
        mpsc::RecvTimeoutError::Timeout => EngineError::io(format!("the app did not answer {method} within {} s", timeout.as_secs())),
        mpsc::RecvTimeoutError::Disconnected => EngineError::io(format!("the app dropped {method} (closing?)")),
    })
}

fn screenshot(params: &Value, jobs: &mpsc::Sender<Job>, wake: &Waker) -> Result<Value, EngineError> {
    let path = params.get("path").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
    let Some(path) = path else { return Err(EngineError::invalid("ui.screenshot needs a 'path' for the PNG file")) };
    let path = std::path::absolute(path).map_err(|e| EngineError::invalid(format!("bad path '{path}': {e}")))?;
    match ask_ui("ui.screenshot", params, jobs, wake, SCREENSHOT_TIMEOUT)? {
        Reply::Image(image) => {
            write_png(&path, &image)?;
            Ok(json!({"path": path.to_string_lossy(), "width": image.size[0], "height": image.size[1]}))
        }
        Reply::Done(r) => r.and(Err(EngineError::internal("the app answered a screenshot without an image"))),
    }
}

/// Writes `image` as an 8-bit RGBA PNG.
pub fn write_png(path: &Path, image: &egui::ColorImage) -> Result<(), EngineError> {
    let fail = |e: String| EngineError::io(format!("cannot write {}: {e}", path.display()));
    let [w, h] = image.size;
    let (Ok(width), Ok(height)) = (u32::try_from(w), u32::try_from(h)) else { return Err(fail("image too large".into())) };
    if width == 0 || height == 0 || image.pixels.len() != w * h {
        return Err(fail("empty screenshot".into()));
    }
    let data: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let file = std::fs::File::create(path).map_err(|e| fail(e.to_string()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| fail(e.to_string()))?;
    writer.write_image_data(&data).map_err(|e| fail(e.to_string()))?;
    writer.finish().map_err(|e| fail(e.to_string()))
}

/// The UI-thread side: runs queued requests on the app.
pub struct Control {
    jobs: mpsc::Receiver<Job>,
}

impl Control {
    pub fn new(jobs: mpsc::Receiver<Job>) -> Self {
        Self { jobs }
    }

    /// Runs every waiting request. Call once per frame before drawing (from `App::logic`, which
    /// eframe also calls while the window is hidden).
    pub fn poll(&self, app: &mut ErpApp, ctx: &egui::Context) {
        while let Ok(job) = self.jobs.try_recv() {
            match dispatch(app, &job.method, &job.params) {
                Outcome::Reply(r) => {
                    let _ = job.reply.send(Reply::Done(r));
                }
                Outcome::Screenshot => {
                    let tx = job.reply;
                    ctx.request_screenshot(move |image| {
                        let _ = tx.send(Reply::Image(image));
                    });
                }
            }
            ctx.request_repaint();
        }
    }
}

/// Runs one control method on the app (UI thread). No window needed, so tests call it directly.
pub fn dispatch(app: &mut ErpApp, method: &str, params: &Value) -> Outcome {
    let result = match method {
        "engine.run" => engine_run(app, params),
        "engine.commands" => app.session.run("command.list", &json!({})),
        "ui.open" => ui_open(app, params),
        "ui.back" => {
            app.back();
            Ok(state(app))
        }
        "ui.home" => {
            app.home();
            Ok(state(app))
        }
        "ui.state" => Ok(state(app)),
        "ui.screenshot" => return Outcome::Screenshot,
        other => Err(EngineError::not_found(format!(
            "Unknown control method '{other}'. Methods: ping, engine.run, engine.commands, ui.open, ui.back, ui.home, ui.state, ui.screenshot."
        ))),
    };
    Outcome::Reply(result)
}

fn engine_run(app: &mut ErpApp, params: &Value) -> Result<Value, EngineError> {
    let Some(command) = params.get("command").and_then(Value::as_str) else {
        return Err(EngineError::invalid("engine.run needs a 'command' (a command id such as record.list)"));
    };
    let cmd_params = match params.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err(EngineError::invalid("engine.run 'params' must be an object")),
    };
    let r = app.session.run(command, &cmd_params)?;
    // Changes show in the window the way the UI shows its own: a message in the notice bar, an
    // `open` navigates. Reads (lists, reports) don't pop anything up in front of the user.
    if commands::find(command).is_some_and(|c| c.mutates) {
        app.handle_result(&r);
    }
    Ok(r)
}

fn ui_open(app: &mut ErpApp, params: &Value) -> Result<Value, EngineError> {
    let spec = params.get("spec").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if spec.is_empty() {
        return Err(EngineError::invalid("ui.open needs a 'spec': page, page:key or list>card:key (e.g. customer-card:10000)"));
    }
    for part in spec.split('>') {
        let page = part.split_once(':').map_or(part, |(p, _)| p).trim();
        if app.session.page(page).is_none() {
            return Err(EngineError::not_found(format!("Page '{page}' does not exist.")));
        }
    }
    let had_error = matches!(app.dialog, Some(Dialog::Error(_)));
    let depth = app.stack.len();
    app.open_spec(spec);
    if !had_error && let Some(Dialog::Error(msg)) = &app.dialog {
        let msg = msg.clone();
        app.dialog = None;
        return Err(EngineError::invalid(msg));
    }
    if app.stack.len() == depth {
        return Err(EngineError::not_found(format!("Nothing opened for '{spec}' (check the record key).")));
    }
    Ok(state(app))
}

/// The open pages, newest last, plus what is shown over them.
pub fn state(app: &ErpApp) -> Value {
    let stack: Vec<Value> = app
        .stack
        .iter()
        .map(|v| match v {
            View::RoleCenter => json!({"view": "role_center", "role_center": app.role_center}),
            View::List(l) => json!({
                "view": "list",
                "page": l.page,
                "filters": l.filters.iter().map(|(f, v)| json!([f, v])).collect::<Vec<_>>(),
                "search": l.search,
                "selected": l.selected.as_ref().map(|k| k.to_json()),
            }),
            View::Card(c) => json!({
                "view": "card",
                "page": c.page,
                "table": c.table,
                "key": c.key.to_json(),
                "key_text": c.key.display(),
                "editing": c.editing,
                "expanded": c.expanded,
                "selected_line": c.selected_line.as_ref().map(|k| k.to_json()),
                "list_page": c.nav.as_ref().map(|n| n.list_page.clone()),
            }),
        })
        .collect();
    let dialog = match &app.dialog {
        None => Value::Null,
        Some(Dialog::Error(m)) => json!({"kind": "error", "message": m}),
        Some(Dialog::Confirm { text, command, .. }) => json!({"kind": "confirm", "text": text, "command": command}),
        Some(Dialog::Form { title, .. }) => json!({"kind": "form", "title": title}),
        Some(Dialog::Report { title, .. }) => json!({"kind": "report", "title": title}),
    };
    json!({
        "stack": stack,
        "top": stack.last().cloned().unwrap_or(Value::Null),
        "dialog": dialog,
        "notice": app.notice,
        "source": store::redact(&app.session.source()),
        "revision": app.session.revision(),
        "work_date": app.session.work_date().to_string(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    use erpcraft_engine::Session;
    use erpcraft_mcp::{Backend, Remote};

    fn demo_app() -> ErpApp {
        let mut s = Session::in_memory();
        s.run("company.init", &json!({"demo": true})).unwrap();
        ErpApp::new(s)
    }

    fn reply(o: Outcome) -> Result<Value, EngineError> {
        match o {
            Outcome::Reply(r) => r,
            Outcome::Screenshot => panic!("unexpected screenshot"),
        }
    }

    #[test]
    fn dispatch_drives_the_app() {
        let mut app = demo_app();
        let st = reply(dispatch(&mut app, "ui.state", &json!({}))).unwrap();
        assert_eq!(st["stack"][0]["view"], "role_center");
        assert_eq!(st["source"], "memory");

        let st = reply(dispatch(&mut app, "ui.open", &json!({"spec": "customer-list>customer-card:10000"}))).unwrap();
        assert_eq!(st["stack"].as_array().unwrap().len(), 3, "{st}");
        assert_eq!(st["top"]["page"], "customer-card");
        assert_eq!(st["top"]["key"], json!(["10000"]));
        assert_eq!(st["top"]["list_page"], "customer-list");

        let st = reply(dispatch(&mut app, "ui.back", &json!({}))).unwrap();
        assert_eq!(st["top"]["view"], "list");
        let st = reply(dispatch(&mut app, "ui.home", &json!({}))).unwrap();
        assert_eq!(st["stack"].as_array().unwrap().len(), 1);

        assert_eq!(reply(dispatch(&mut app, "ui.open", &json!({"spec": "no-such-page"}))).unwrap_err().kind, erpcraft_engine::ErrorKind::NotFound);
        assert!(reply(dispatch(&mut app, "ui.open", &json!({}))).is_err());
        assert!(app.dialog.is_none(), "errors go to the caller, not into a dialog");
        assert_eq!(reply(dispatch(&mut app, "nope", &json!({}))).unwrap_err().kind, erpcraft_engine::ErrorKind::NotFound);
        assert_eq!(dispatch(&mut app, "ui.screenshot", &json!({"path": "x.png"})), Outcome::Screenshot);
    }

    #[test]
    fn engine_run_shows_changes_in_the_window() {
        let mut app = demo_app();
        let list = reply(dispatch(&mut app, "engine.run", &json!({"command": "record.list", "params": {"table": "Customer"}}))).unwrap();
        assert_eq!(list["total"], 5);
        assert!(app.notice.is_none() && app.dialog.is_none());

        let r = reply(dispatch(
            &mut app,
            "engine.run",
            &json!({"command": "sales.document.new", "params": {"customer_no": "20000", "lines": [{"no": "1100", "quantity": 1}]}}),
        ))
        .unwrap();
        assert!(r.get("open").is_some(), "{r}");
        let st = state(&app);
        assert_eq!(st["top"]["view"], "card", "{st}");
        assert_eq!(st["top"]["table"], "Sales Header");

        let e = reply(dispatch(&mut app, "engine.run", &json!({"command": "record.get", "params": {"table": "Customer", "key": "NOPE"}}))).unwrap_err();
        assert_eq!(e.kind, erpcraft_engine::ErrorKind::NotFound);
        assert!(reply(dispatch(&mut app, "engine.run", &json!({"params": {}}))).is_err());
        assert!(reply(dispatch(&mut app, "engine.run", &json!({"command": "company.info", "params": 3}))).is_err());
        let cmds = reply(dispatch(&mut app, "engine.commands", &json!({}))).unwrap();
        assert!(cmds["commands"].as_array().unwrap().len() > 10);
    }

    #[test]
    fn handle_line_answers_protocol_errors_without_the_ui() {
        let (tx, _rx) = mpsc::channel();
        let wake: Waker = Arc::new(|| {});
        let r: Value = serde_json::from_str(&handle_line("{oops", &tx, &wake)).unwrap();
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["kind"], "invalid");
        let r: Value = serde_json::from_str(&handle_line(r#"{"id": 9, "method": "ping"}"#, &tx, &wake)).unwrap();
        assert_eq!((r["id"].clone(), r["ok"].clone(), r["result"]["pong"].clone()), (json!(9), json!(true), json!(true)));
        let r: Value = serde_json::from_str(&handle_line(r#"{"id": 1, "method": "ui.screenshot", "params": {}}"#, &tx, &wake)).unwrap();
        assert_eq!(r["error"]["kind"], "invalid");
    }

    /// The whole channel over a real socket: a fake UI loop on another thread runs `dispatch`, a
    /// `Remote` client (what `erpcraft-cli --connect` and `mcp --connect` use) talks to it.
    #[test]
    fn socket_round_trip_with_a_fake_ui_loop() {
        let listener = listen(0).unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        let jobs = start(listener, Arc::new(|| {})).unwrap();
        thread::spawn(move || {
            let mut app = demo_app();
            while let Ok(job) = jobs.recv() {
                let reply = match dispatch(&mut app, &job.method, &job.params) {
                    Outcome::Reply(r) => Reply::Done(r),
                    Outcome::Screenshot => Reply::Image(Arc::new(egui::ColorImage::filled([3, 2], egui::Color32::from_rgb(10, 20, 30)))),
                };
                let _ = job.reply.send(reply);
            }
        });

        let mut remote = Remote::connect(&addr.to_string()).unwrap();
        assert_eq!(remote.call("ping", &json!({})).unwrap()["pong"], true);
        assert!(remote.commands().iter().any(|c| c.id == "record.list"));
        assert_eq!(remote.run("record.list", &json!({"table": "Item"})).unwrap()["total"].as_u64().map(|n| n > 0), Some(true));
        assert_eq!(remote.run("nope", &json!({})).unwrap_err().kind, erpcraft_engine::ErrorKind::NotFound);
        let st = remote.ui("ui.open", &json!({"spec": "item-card:1000"})).unwrap();
        assert_eq!(st["top"]["page"], "item-card", "{st}");

        let path = std::env::temp_dir().join(format!("erpcraft-control-test-{}.png", std::process::id()));
        let shot = remote.ui("ui.screenshot", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!((shot["width"].clone(), shot["height"].clone()), (json!(3), json!(2)));
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let _ = std::fs::remove_file(&path);

        // Garbage on the wire gets an error line, and the connection stays usable.
        let mut raw = TcpStream::connect(addr).unwrap();
        raw.write_all(b"not json\n{\"id\":2,\"method\":\"ui.state\"}\n").unwrap();
        let mut lines = io::BufRead::lines(BufReader::new(raw));
        let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(first["ok"], false);
        let second: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!((second["id"].clone(), second["ok"].clone()), (json!(2), json!(true)));
    }

    #[test]
    fn screenshot_times_out_when_the_ui_never_paints() {
        let (tx, rx) = mpsc::channel::<Job>();
        let wake: Waker = Arc::new(|| {});
        // A UI that takes the job but never answers (hidden window, no frames).
        let holder = thread::spawn(move || {
            let job = rx.recv().unwrap();
            thread::sleep(SCREENSHOT_TIMEOUT + Duration::from_millis(500));
            drop(job);
        });
        let start = std::time::Instant::now();
        let r: Value = serde_json::from_str(&handle_line(r#"{"id": 1, "method": "ui.screenshot", "params": {"path": "never.png"}}"#, &tx, &wake)).unwrap();
        assert_eq!(r["error"]["kind"], "io", "{r}");
        assert!(start.elapsed() < SCREENSHOT_TIMEOUT + Duration::from_secs(2));
        holder.join().unwrap();
    }
}
