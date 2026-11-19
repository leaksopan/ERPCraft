//! ERPCraft's egui client, laid out like the Business Central web client: app bar, role center
//! (navigation, action bar, cues), list pages (action bar, grid, FactBox pane), card and document
//! pages (FastTabs, lines part, totals), Tell Me search.
//!
//! The UI is thin: it renders engine metadata and changes data only through
//! [`erpcraft_engine::Session::run`], like the CLI and the MCP server.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

mod card;
mod dialogs;
mod list;
mod nav;
mod role_center;
mod shell;
pub mod theme;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use erpcraft_engine::Session;
use erpcraft_engine::meta::{ActionDef, Key, PageKind};
use serde_json::{Map, Value as Json, json};

pub use dialogs::Dialog;
pub use nav::View;

/// How often the window pulls in what other sessions (another user, the CLI, an agent) saved to
/// the same PostgreSQL company.
const REFRESH_EVERY: Duration = Duration::from_secs(2);

/// The application state behind one window.
pub struct ErpApp {
    pub session: Session,
    pub stack: Vec<View>,
    pub dialog: Option<Dialog>,
    /// Edit buffers of text fields being typed in, by widget id.
    pub bufs: HashMap<String, String>,
    /// Informational message bar (BC's blue notification).
    pub notice: Option<String>,
    pub tell_me: Option<dialogs::TellMe>,
    pub nav_menu_open: bool,
    pub role_center: String,
    /// FastTabs the user toggled away from their default (page id + caption).
    pub toggled_tabs: HashSet<String>,
    pub show_factbox: bool,
    /// `egui` time (seconds) of the next [`Session::refresh`].
    next_refresh: f64,
    /// A failed refresh was shown in the notice bar; don't repeat it every 2 seconds.
    refresh_failed: bool,
}

impl ErpApp {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            stack: vec![View::RoleCenter],
            dialog: None,
            bufs: HashMap::new(),
            notice: None,
            tell_me: None,
            nav_menu_open: false,
            role_center: erpcraft_engine::schema::role_center::BUSINESS_MANAGER.to_string(),
            toggled_tabs: HashSet::new(),
            show_factbox: true,
            next_refresh: 0.0,
            refresh_failed: false,
        }
    }

    /// Draws one frame into `ui` (the window's root).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.auto_refresh(ui.ctx());
        self.shortcuts(ui.ctx());
        shell::app_bar(self, ui);
        // The base page (role center or a list) stays underneath; cards stack over it.
        let base = self.stack.iter().rposition(|v| !matches!(v, View::Card(_))).unwrap_or(0);
        let base_is_rc = matches!(self.stack.get(base), Some(View::RoleCenter) | None);
        shell::role_center_bars(self, ui, base_is_rc);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::TOKENS.canvas)).show(ui, |ui| {
            if let Some(n) = self.notice.clone() {
                widgets::notice_bar(ui, &n, || self.notice = None);
            }
            let rect = ui.available_rect_before_wrap();
            let cards: Vec<usize> = (base + 1..self.stack.len()).collect();
            let mut base_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("base_view"));
            if !cards.is_empty() {
                base_ui.disable();
            }
            match self.stack.get(base).cloned() {
                Some(View::List(state)) => list::show(self, &mut base_ui, base, state),
                _ => role_center::show(self, &mut base_ui),
            }
            if cards.is_empty() {
                return;
            }
            // CoreERP page stack: 22% scrim over the content area, cards at most 1328 px wide,
            // each card underneath shifted 80 px left (7 px when expanded) and dimmed.
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_black_alpha(56));
            let narrow = rect.width() < 1408.0;
            let count = cards.len();
            for (i, idx) in cards.iter().copied().enumerate() {
                let depth = count - 1 - i;
                let active = depth == 0;
                if narrow && !active {
                    continue;
                }
                let Some(View::Card(state)) = self.stack.get(idx).cloned() else { continue };
                let inset = if narrow {
                    0.0
                } else if state.expanded {
                    40.0
                } else {
                    ((rect.width() - 1328.0) / 2.0).max(0.0)
                };
                let shift = if narrow { 0.0 } else { depth.min(3) as f32 * if state.expanded { 7.0 } else { 80.0 } };
                let r = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + inset - shift, rect.top()),
                    egui::pos2(rect.right() - inset - shift, rect.bottom()),
                );
                let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt(("page_dialog", idx)));
                if !active {
                    cui.disable();
                }
                card::show(self, &mut cui, idx, state, card::Layer { active, count, narrow });
                if !active {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(46));
                }
                if self.stack.len() <= idx {
                    break;
                }
            }
        });
        shell::nav_menu(self, ui.ctx());
        dialogs::show(self, ui.ctx());
    }

    /// About every [`REFRESH_EVERY`], pulls in other sessions' changes (a no-op for a data file) and
    /// schedules the next frame, so the window follows a shared company without input.
    fn auto_refresh(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now >= self.next_refresh {
            self.next_refresh = now + REFRESH_EVERY.as_secs_f64();
            match self.session.refresh() {
                Ok(_) => self.refresh_failed = false,
                Err(e) => {
                    if !self.refresh_failed {
                        self.notice = Some(format!("Could not refresh the company data: {}", e.message));
                    }
                    self.refresh_failed = true;
                }
            }
        }
        ctx.request_repaint_after(REFRESH_EVERY);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (alt_q, esc) = ctx.input_mut(|i| {
            (i.consume_key(egui::Modifiers::ALT, egui::Key::Q), i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        });
        if alt_q {
            self.tell_me = Some(dialogs::TellMe::default());
        }
        if esc {
            if self.dialog.is_some() {
                self.dialog = None;
            } else if self.tell_me.is_some() {
                self.tell_me = None;
            } else if self.nav_menu_open {
                self.nav_menu_open = false;
            } else {
                self.back();
            }
        }
    }

    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.bufs.clear();
        }
    }

    pub fn home(&mut self) {
        self.stack.truncate(1);
        self.bufs.clear();
    }

    /// Opens a page: a list (with filters) or, for card / document pages, the record `key`.
    pub fn open_page(&mut self, page_id: &str, filters: Vec<(String, String)>, key: Option<Key>) {
        let Some(page) = self.session.page(page_id) else {
            self.error(format!("Page '{page_id}' does not exist."));
            return;
        };
        self.nav_menu_open = false;
        self.bufs.clear();
        match (page.kind, key) {
            (PageKind::Card | PageKind::Document, Some(key)) => self.stack.push(View::card(page_id, key)),
            (PageKind::Card | PageKind::Document, None) => {
                // A setup card shows the single record; other cards start a new record.
                let Some(table) = page.source_table.as_deref().and_then(|t| self.session.table(t)) else { return };
                if table.primary_key.first().map(String::as_str) == Some("Primary Key") {
                    let key = Key(vec![erpcraft_engine::meta::Value::text("")]);
                    if self.session.record(&table.name, &key).is_none() {
                        let r = self.session.run("record.new", &json!({"table": table.name}));
                        if let Err(e) = r {
                            self.error(e.message);
                            return;
                        }
                    }
                    self.stack.push(View::card(page_id, key));
                } else {
                    self.new_record_on(page_id);
                }
            }
            _ => self.stack.push(View::list(page_id, filters)),
        }
    }

    /// "+ New" on a card / document page: inserts a record (number series) and opens it in edit
    /// mode, as CoreERP does for new records.
    pub fn new_record_on(&mut self, card_page: &str) {
        match self.session.run("record.new", &json!({"page": card_page})) {
            Ok(r) => {
                self.handle_result(&r);
                if let Some(View::Card(c)) = self.stack.last_mut() {
                    c.editing = true;
                }
            }
            Err(e) => self.error(e.message),
        }
    }

    /// Opens a card from a list row; ‹ › on the card then step through that list.
    pub fn open_card_from_list(&mut self, page: &str, key: Key, list: &nav::ListState) {
        self.open_page(page, Vec::new(), Some(key));
        if let Some(View::Card(c)) = self.stack.last_mut() {
            c.nav = Some(nav::RecordNav { list_page: list.page.clone(), filters: list.filters.clone(), search: list.search.clone() });
        }
    }

    /// Opens `page` or `page:key` (multi-field keys joined by `|`), e.g. `customer-card:10000`.
    pub fn open_spec(&mut self, spec: &str) {
        // `list>card:key` opens the list, then the card over it (with ‹ › through the list).
        if let Some((list, card)) = spec.split_once('>') {
            self.open_spec(list);
            let Some(View::List(state)) = self.stack.last().cloned() else { return };
            let Some((page, key)) = card.split_once(':') else { return };
            let key = self
                .session
                .page(page)
                .and_then(|p| p.source_table.as_deref())
                .and_then(|t| self.session.table(t))
                .and_then(|t| Key::from_json(t, &json!(key)).ok());
            if let Some(key) = key {
                self.open_card_from_list(page, key, &state);
            }
            return;
        }
        let (page, key) = match spec.split_once(':') {
            Some((p, k)) => (p, Some(k)),
            None => (spec, None),
        };
        let key = key.and_then(|k| {
            let t = self.session.page(page)?.source_table.as_deref()?;
            let t = self.session.table(t)?;
            Key::from_json(t, &json!(k)).ok()
        });
        self.open_page(page, Vec::new(), key);
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.dialog = Some(Dialog::Error(msg.into()));
    }

    /// Runs a command and shows its outcome (navigation, report, message or error).
    pub fn run(&mut self, command: &str, params: Json) {
        match self.session.run(command, &params) {
            Ok(r) => self.handle_result(&r),
            Err(e) => self.error(e.message),
        }
    }

    pub fn handle_result(&mut self, r: &Json) {
        if let Some(rep) = r.get("report") {
            self.dialog = Some(Dialog::report(rep));
        }
        if let Some(msg) = r.get("message").and_then(Json::as_str) {
            self.notice = Some(msg.to_string());
        }
        if let Some(open) = r.get("open").filter(|o| !o.is_null()) {
            let page = open.get("page").and_then(Json::as_str).unwrap_or_default().to_string();
            let key = self
                .session
                .page(&page)
                .and_then(|p| p.source_table.as_deref())
                .and_then(|t| self.session.table(t))
                .and_then(|t| Key::from_json(t, open.get("key").unwrap_or(&Json::Null)).ok());
            if let Some(key) = key {
                // Posting deletes the document being shown: replace it instead of stacking.
                if let Some(View::Card(c)) = self.stack.last()
                    && self.session.record(&c.table, &c.key).is_none()
                {
                    self.stack.pop();
                }
                self.open_page(&page, Vec::new(), Some(key));
            }
        } else if let Some(View::Card(c)) = self.stack.last()
            && self.session.record(&c.table, &c.key).is_none()
        {
            // The record shown no longer exists (deleted / posted): go back.
            self.stack.pop();
        }
    }

    /// Runs a page action for the current record (`table` + `key` as context).
    pub fn run_action(&mut self, a: &ActionDef, context: Option<(&str, &Key)>) {
        if let Some(page) = &a.open_page {
            let rec = context.and_then(|(t, k)| self.session.record(t, k));
            let filters: Vec<(String, String)> =
                a.link.iter().map(|(target, source)| (target.clone(), rec.as_ref().map(|r| r.get(source).to_string()).unwrap_or_default())).collect();
            let target = self.session.page(page);
            let is_card = target.is_some_and(|p| matches!(p.kind, PageKind::Card | PageKind::Document));
            if is_card {
                let key = target
                    .and_then(|p| p.source_table.as_deref())
                    .and_then(|t| self.session.table(t))
                    .map(|t| Key(t.primary_key.iter().map(|k| {
                        let v = filters.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).unwrap_or_default();
                        erpcraft_engine::meta::Value::text(v)
                    }).collect()));
                self.open_page(page, Vec::new(), key);
            } else {
                self.open_page(page, filters, None);
            }
            return;
        }
        let Some(command) = &a.command else { return };
        let mut params = Map::new();
        for (k, v) in &a.params {
            params.insert(k.clone(), json!(v));
        }
        if let Some((t, k)) = context {
            params.insert("table".into(), json!(t));
            params.insert("key".into(), k.to_json());
        }
        let mut missing = dialogs::missing_params(command, &params, context.is_some());
        for f in &a.ask {
            if !missing.contains(f) {
                missing.push(f.clone());
            }
        }
        if !missing.is_empty() {
            self.dialog = Some(Dialog::params(a.caption.replace('&', ""), command, Json::Object(params), missing));
            return;
        }
        if let Some(q) = &a.confirm {
            self.dialog = Some(Dialog::Confirm { text: q.clone(), command: command.clone(), params: Json::Object(params) });
            return;
        }
        self.run(command, Json::Object(params));
    }
}
