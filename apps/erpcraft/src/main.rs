//! ERPCraft desktop app: `erpcraft [--data company.erpcraft.json]`.
//!
//! The data file defaults to `ERPCRAFT_DATA`, else `company.erpcraft.json` in the current
//! directory. A missing file opens an empty company with a "create demo company" start page.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use erpcraft_engine::Session;
use erpcraft_ui_egui::{ErpApp, theme};

struct App {
    inner: ErpApp,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.inner.ui(ui);
    }
}

fn arg_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn data_path() -> PathBuf {
    if let Some(p) = arg_value("--data") {
        return PathBuf::from(p);
    }
    std::env::var("ERPCRAFT_DATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("company.erpcraft.json"))
}

fn main() -> ExitCode {
    let path = data_path();
    let session = match Session::open(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", path.display());
            return ExitCode::from(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ERPCraft").with_inner_size([1400.0, 860.0]).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "ERPCraft",
        options,
        Box::new(|cc| {
            theme::install_system_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            theme::apply(&cc.egui_ctx);
            let mut inner = ErpApp::new(session);
            // `--open page[:key]` opens a page at start (key parts separated by '|').
            if let Some(spec) = arg_value("--open") {
                inner.open_spec(&spec);
            }
            Ok(Box::new(App { inner }))
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
