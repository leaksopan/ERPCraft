//! ERPCraft desktop app:
//! `erpcraft [--db <postgres-url> | --data <file>] [--control [port]] [--open <spec>] [--size WxH]`.
//!
//! The company comes from `--db`, else `ERPCRAFT_DATABASE_URL`, else `--data`, else
//! `ERPCRAFT_DATA`, else `company.erpcraft.json` in the current directory. A missing file opens an
//! empty company with a "create demo company" start page. `--control` lets agents drive the running
//! app over a loopback socket (docs/control.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod control;

use std::process::ExitCode;
use std::sync::Arc;

use erpcraft_engine::{Session, store};
use erpcraft_mcp::control::DEFAULT_PORT;
use erpcraft_ui_egui::{ErpApp, theme};

use control::Control;

const USAGE: &str = "\
erpcraft - ERPCraft desktop app

USAGE:
    erpcraft [--db <postgres-url> | --data <file>] [--control [port]] [--open <spec>] [--size WxH]

OPTIONS:
    --db <url>         PostgreSQL company (default: $ERPCRAFT_DATABASE_URL)
    --data <file>      Company data file (default: $ERPCRAFT_DATA, else ./company.erpcraft.json)
    --control [port]   Let agents drive the app on 127.0.0.1:<port> (default 7979); see docs/control.md
    --open <spec>      Open a page at start: page, page:key or list>card:key
    --size <WxH>       Window size, e.g. 1700x950
    -h, --help         This text
";

/// Command-line options.
#[derive(Debug, Default, PartialEq)]
struct Options {
    db: Option<String>,
    data: Option<String>,
    /// `Some(port)` with `--control`.
    control: Option<u16>,
    open: Option<String>,
    size: Option<[f32; 2]>,
    help: bool,
}

impl Options {
    fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
        let mut o = Options::default();
        let mut it = args.into_iter().peekable();
        while let Some(a) = it.next() {
            let (name, inline) = match a.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
                _ => (a.clone(), None),
            };
            let mut value = |name: &str| -> Result<String, String> {
                match inline.clone() {
                    Some(v) => Ok(v),
                    None => it.next().ok_or_else(|| format!("option {name} needs a value")),
                }
            };
            match name.as_str() {
                "--db" => o.db = Some(value("--db")?),
                "--data" | "-d" => o.data = Some(value("--data")?),
                "--open" => o.open = Some(value("--open")?),
                "--size" => {
                    let v = value("--size")?;
                    let size = v.split_once('x').and_then(|(w, h)| Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?]));
                    o.size = Some(size.filter(|[w, h]| *w >= 1.0 && *h >= 1.0).ok_or_else(|| format!("--size needs WIDTHxHEIGHT, got '{v}'"))?);
                }
                "--control" => {
                    // The port is optional: `--control`, `--control 8000`, `--control=8000`.
                    let port = match inline {
                        Some(v) => Some(v),
                        None => it.next_if(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())),
                    };
                    o.control = Some(match port {
                        Some(p) => p.trim().parse::<u16>().map_err(|_| format!("--control needs a port number (0-65535), got '{p}'"))?,
                        None => DEFAULT_PORT,
                    });
                }
                "--help" | "-h" => o.help = true,
                other => return Err(format!("unknown argument '{other}'")),
            }
        }
        Ok(o)
    }
}

struct App {
    inner: ErpApp,
    control: Option<Control>,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Control requests run here, before the frame is drawn; eframe calls `logic` even while
        // the window is hidden, so a minimised app still answers.
        if let Some(c) = &self.control {
            c.poll(&mut self.inner, ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.inner.ui(ui);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(events) = self.inner.synthetic_input.pop_front() {
            raw_input.events.extend(events);
            ctx.request_repaint();
        }
    }
}

fn main() -> ExitCode {
    let opts = match Options::parse(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if opts.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let source = store::resolve_source(opts.db.as_deref(), opts.data.as_deref());
    let session = match Session::open_source(&source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", store::redact(&source));
            return ExitCode::from(1);
        }
    };
    // Bind before opening the window so a busy port is reported at once.
    let listener = match opts.control.map(control::listen).transpose() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot listen on 127.0.0.1:{}: {e}", opts.control.unwrap_or(DEFAULT_PORT));
            return ExitCode::from(1);
        }
    };
    let size = opts.size.unwrap_or([1400.0, 860.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ERPCraft").with_inner_size(size).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "ERPCraft",
        options,
        Box::new(move |cc| {
            theme::install_system_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            theme::apply(&cc.egui_ctx);
            let mut inner = ErpApp::new(session);
            if let Some(spec) = &opts.open {
                inner.open_spec(spec);
            }
            let control = match listener {
                Some(listener) => {
                    let addr = listener.local_addr()?;
                    let ctx = cc.egui_ctx.clone();
                    let jobs = control::start(listener, Arc::new(move || ctx.request_repaint()))?;
                    eprintln!("erpcraft: control channel on {addr} ({})", store::redact(&inner.session.source()));
                    Some(Control::new(jobs))
                }
                None => None,
            };
            Ok(Box::new(App { inner, control }))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn parse(a: &[&str]) -> Result<Options, String> {
        Options::parse(a.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn options() {
        let o = parse(&["--db", "postgres://u:p@h/db", "--control"]).unwrap();
        assert_eq!((o.db.as_deref(), o.control), (Some("postgres://u:p@h/db"), Some(7979)));
        assert_eq!(parse(&["--control", "8000", "--data", "x.json"]).unwrap().control, Some(8000));
        assert_eq!(parse(&["--control=0"]).unwrap().control, Some(0));
        let o = parse(&["--control", "--open", "customer-list", "--size", "1700x950"]).unwrap();
        assert_eq!((o.control, o.open.as_deref(), o.size), (Some(7979), Some("customer-list"), Some([1700.0, 950.0])));
        assert_eq!(parse(&["--data=a.json"]).unwrap().data.as_deref(), Some("a.json"));
        assert!(parse(&["--control", "99999"]).is_err());
        assert!(parse(&["--control=abc"]).is_err());
        assert!(parse(&["--size", "big"]).is_err());
        assert!(parse(&["--db"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert_eq!(parse(&[]).unwrap(), Options::default());
    }
}
