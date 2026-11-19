//! ERPCraft on the web: the same egui client as the desktop app, compiled to WebAssembly. The
//! company lives in the browser's memory (the Auto Craft demo, created at start); there is no
//! PostgreSQL or file access on the web.
//!
//! Build: `cargo build -p erpcraft-web --target wasm32-unknown-unknown --release`, then
//! `wasm-bindgen --target web --out-dir apps/erpcraft-web/www/pkg target/wasm32-unknown-unknown/release/erpcraft_web.wasm`
//! and serve `apps/erpcraft-web/www` (see docs/web.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

use erpcraft_engine::Session;

/// The demo company the web app starts with.
pub fn demo_session() -> Session {
    let mut s = Session::in_memory();
    s.set_user("DEMO");
    // A failure leaves an empty company, where the start page offers to create one.
    let _ = s.run("company.init", &serde_json::json!({"demo": true}));
    s
}

#[cfg(target_arch = "wasm32")]
mod web {
    use erpcraft_ui_egui::{ErpApp, theme};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::wasm_bindgen;

    struct App {
        inner: ErpApp,
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.inner.ui(ui);
        }
    }

    /// Starts the app on the `<canvas id="erpcraft">` of the page.
    #[wasm_bindgen(start)]
    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("erpcraft"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                return;
            };
            let runner = eframe::WebRunner::new();
            let _ = runner
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(|cc| {
                        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
                        theme::install_system_fonts(&cc.egui_ctx);
                        theme::apply(&cc.egui_ctx);
                        Ok(Box::new(App { inner: ErpApp::new(super::demo_session()) }))
                    }),
                )
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_web_demo_company_balances() {
        let mut s = super::demo_session();
        assert_eq!(s.run("report.trial_balance", &serde_json::json!({})).unwrap()["balanced"], true);
    }
}
