//! ERPCraft on the web: the same egui client as the desktop app, compiled to WebAssembly.
//!
//! Speaks the Craft web contracts (`craft-web-api` on `window.erpcraft`, `craft-web-host` when
//! framed with `?host=parent` or `data-host="page"`, and `craft-platform::browser` for PDF
//! downloads). The company runs in browser memory on the demo company; there is no direct
//! PostgreSQL or local file access on the web.
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    use craft_platform::browser::{DownloadOptions, PreparedDownload};
    use craft_web_api::methods::Methods;
    use craft_web_api::page::{PageApi, Request};
    use craft_web_host::web::{FileData, Host, Message as HostMessage};
    use erpcraft_ui_egui::{ErpApp, theme};
    use serde_json::{Value as Json, json};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::wasm_bindgen;

    struct Slot {
        reply: Option<Json>,
        waker: Option<Waker>,
    }

    struct ReplyFuture(Rc<RefCell<Slot>>);

    impl Future for ReplyFuture {
        type Output = Json;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut slot = self.0.borrow_mut();
            if let Some(reply) = slot.reply.take() {
                Poll::Ready(reply)
            } else {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    struct ApiCall {
        method: String,
        params: Json,
        slot: Rc<RefCell<Slot>>,
    }

    struct App {
        inner: ErpApp,
        api_calls: Rc<RefCell<VecDeque<ApiCall>>>,
        host: Option<Rc<Host>>,
        host_inbox: Rc<RefCell<VecDeque<HostMessage>>>,
    }

    impl App {
        fn handle_api(&mut self, method: &str, params: &Json) -> Json {
            let res = match method {
                "engine.execute" => {
                    let cmd = params.get("command").and_then(Json::as_str).unwrap_or_default();
                    let p = params.get("params").cloned().unwrap_or_else(|| json!({}));
                    self.inner.session.run(cmd, &p)
                }
                "engine.commands" => self.inner.session.run("command.list", &json!({})),
                other => self.inner.session.run(other, params),
            };
            match res {
                Ok(result) => json!({"ok": true, "result": result}),
                Err(e) => json!({"ok": false, "error": e.message}),
            }
        }

        fn drain_api(&mut self) {
            let calls: Vec<ApiCall> = self.api_calls.borrow_mut().drain(..).collect();
            for call in calls {
                let reply = self.handle_api(&call.method, &call.params);
                let mut slot = call.slot.borrow_mut();
                slot.reply = Some(reply);
                if let Some(w) = slot.waker.take() {
                    w.wake();
                }
            }
        }

        fn drain_host(&mut self) {
            let msgs: Vec<HostMessage> = self.host_inbox.borrow_mut().drain(..).collect();
            for msg in msgs {
                if let HostMessage::Open { name, data } = msg {
                    if let FileData::Bytes(arr) = data {
                        let bytes = arr.to_vec();
                        match serde_json::from_slice::<Json>(&bytes).ok().and_then(|j| erpcraft_engine::db::Database::from_json(self.inner.session.meta(), &j).ok()) {
                            Some(_) => self.inner.notice = Some(format!("Received {name} from host.")),
                            None => {
                                if let Some(host) = &self.host {
                                    let _ = host.post_failed(&format!("Cannot open {name}: unsupported company file."), None);
                                }
                            }
                        }
                    } else if let Some(host) = &self.host {
                        let _ = host.post_failed(&format!("Cannot open {name}: streamed files are not supported yet."), None);
                    }
                }
            }
        }

        fn drain_downloads(&mut self) {
            for dl in self.inner.pending_downloads.drain(..) {
                if let Ok(prep) = PreparedDownload::bytes(&dl.name, &dl.mime, &dl.bytes) {
                    let _ = prep.click(DownloadOptions::default());
                }
            }
        }
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.drain_api();
            self.drain_host();
            self.inner.ui(ui);
            self.drain_downloads();
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
            let canvas_el: web_sys::Element = canvas.clone().into();
            let host = Host::connect("erpcraft", &canvas_el).map(Rc::new);
            let runner = eframe::WebRunner::new();
            let _ = runner
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |cc| {
                        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
                        theme::install_system_fonts(&cc.egui_ctx);
                        theme::apply(&cc.egui_ctx);

                        let api_calls: Rc<RefCell<VecDeque<ApiCall>>> = Rc::new(RefCell::new(VecDeque::new()));
                        let queue = api_calls.clone();
                        let ctx_api = cc.egui_ctx.clone();
                        let request: Request = Rc::new(move |method: String, params: Json| {
                            let slot = Rc::new(RefCell::new(Slot { reply: None, waker: None }));
                            queue.borrow_mut().push_back(ApiCall { method, params, slot: slot.clone() });
                            ctx_api.request_repaint();
                            Box::pin(ReplyFuture(slot))
                        });
                        let methods = Methods {
                            execute: Some("engine.execute"),
                            commands: Some("engine.commands"),
                            doc_inspect: None,
                            ui_inspect: None,
                            screenshot: None,
                        };
                        let _ = Rc::new(PageApi::new("erpcraft", methods, request)).install();

                        let host_inbox: Rc<RefCell<VecDeque<HostMessage>>> = Rc::new(RefCell::new(VecDeque::new()));
                        if let Some(h) = &host {
                            let inbox = host_inbox.clone();
                            let ctx_host = cc.egui_ctx.clone();
                            h.listen(move |m| {
                                inbox.borrow_mut().push_back(m);
                                ctx_host.request_repaint();
                            });
                            let _ = h.post_ready(env!("CARGO_PKG_VERSION"), false);
                        }

                        Ok(Box::new(App {
                            inner: ErpApp::new(super::demo_session()),
                            api_calls,
                            host,
                            host_inbox,
                        }))
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
