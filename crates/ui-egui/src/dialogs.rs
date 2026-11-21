//! Modal dialogs: errors, confirmations, parameter forms (generated from command schemas), new
//! records for inline lists, report tables, and Tell Me (Alt+Q).

use egui::{Align, Id, Layout, RichText, vec2};
use erpcraft_engine::meta::{PageDef, PageKind, TableDef};
use serde_json::{Map, Value as Json, json};

use crate::theme::{self, TOKENS};
use crate::ErpApp;

pub enum Dialog {
    Error(String),
    Confirm { text: String, command: String, params: Json },
    Form { title: String, submit: Submit, fields: Vec<FormField> },
    Report { title: String, columns: Vec<String>, rows: Vec<Vec<String>> },
    /// BC's Select items: tick items, OK adds a line for each.
    SelectItems { command: String, base: Json, items: Vec<(String, String, bool)> },
}

pub enum Submit {
    /// Run `command` with `base` params plus the form's fields.
    Command { command: String, base: Json },
    /// `record.new` for `table` with the form's fields as values.
    NewRecord { table: String },
}

pub struct FormField {
    pub name: String,
    pub caption: String,
    pub hint: String,
    pub value: String,
    /// Allowed values (JSON Schema `enum`): shown as a drop-down.
    pub options: Vec<String>,
}

#[derive(Default)]
pub struct TellMe {
    pub query: String,
    pub selected: usize,
}

impl Dialog {
    pub fn report(r: &Json) -> Self {
        let title = r.get("title").and_then(Json::as_str).unwrap_or("Report").to_string();
        let columns = r.get("columns").and_then(Json::as_array).map(|a| a.iter().map(cell).collect()).unwrap_or_default();
        let rows = r
            .get("rows")
            .and_then(Json::as_array)
            .map(|rows| rows.iter().map(|row| row.as_array().map(|a| a.iter().map(cell).collect()).unwrap_or_default()).collect())
            .unwrap_or_default();
        Dialog::Report { title, columns, rows }
    }

    /// A parameter form for `command`'s `fields` (names from its JSON schema).
    pub fn params(title: String, command: &str, base: Json, fields: Vec<String>) -> Self {
        let schema = erpcraft_engine::commands::find(command).map(|c| (c.params)()).unwrap_or(Json::Null);
        let fields = fields
            .into_iter()
            .map(|name| {
                let prop = schema.get("properties").and_then(|p| p.get(&name));
                let hint = prop.and_then(|p| p.get("description")).and_then(Json::as_str).unwrap_or_default().to_string();
                let value = base.get(&name).map(|v| match v {
                    Json::String(s) => s.clone(),
                    Json::Null => String::new(),
                    other => other.to_string(),
                });
                let boolean = prop.and_then(|p| p.get("type")).and_then(Json::as_str) == Some("boolean");
                let options: Vec<String> = if boolean {
                    vec!["Yes".to_string(), "No".to_string()]
                } else {
                    prop.and_then(|p| p.get("enum"))
                        .and_then(Json::as_array)
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                        .unwrap_or_default()
                };
                let default = prop.and_then(|p| p.get("default")).and_then(|d| match d {
                    Json::Bool(b) => Some(if *b { "Yes" } else { "No" }.to_string()),
                    other => other.as_str().map(str::to_string),
                });
                let value = value.filter(|v| !v.is_empty()).or(default).or_else(|| options.first().cloned()).unwrap_or_default();
                FormField { caption: caption(&name), name, hint, value, options }
            })
            .collect();
        Dialog::Form { title, submit: Submit::Command { command: command.to_string(), base }, fields }
    }

    /// "+ New" on an editable list: the list's columns as a form.
    pub fn new_record(page: &PageDef, table: &TableDef) -> Self {
        let fields = page
            .columns
            .iter()
            .filter_map(|c| table.field(c))
            .filter(|f| f.editable && !f.is_flow())
            .map(|f| {
                let options = match &f.ty {
                    erpcraft_engine::meta::FieldType::Option(o) => o.clone(),
                    _ => Vec::new(),
                };
                FormField { name: f.name.clone(), caption: f.caption.clone(), hint: f.ty.name(), value: options.first().cloned().unwrap_or_default(), options }
            })
            .collect();
        Dialog::Form { title: format!("New - {}", page.caption), submit: Submit::NewRecord { table: table.name.clone() }, fields }
    }
}

fn cell(v: &Json) -> String {
    match v {
        Json::String(s) => {
            // Numbers in reports come as decimal strings: group thousands.
            if let Ok(d) = s.parse::<rust_decimal::Decimal>()
                && s.contains(|c: char| c.is_ascii_digit())
                && !s.chars().all(|c| c.is_ascii_digit() && s.len() == 5)
            {
                return erpcraft_engine::meta::format_decimal(d, 2);
            }
            s.clone()
        }
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

/// `applies_to_doc_no` → "Applies-to Doc. No." style captions.
fn caption(name: &str) -> String {
    let mut out = String::new();
    for (i, w) in name.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let w = match w {
            "no" => "No.",
            "doc" => "Doc.",
            other => other,
        };
        let mut c = w.chars();
        if let Some(f) = c.next() {
            out.extend(f.to_uppercase());
            out.push_str(c.as_str());
        }
    }
    out
}

/// Parameters the user must still give before `command` can run: its required properties that
/// neither the action's params nor the record context provide.
pub fn missing_params(command: &str, params: &Map<String, Json>, has_context: bool) -> Vec<String> {
    let Some(cmd) = erpcraft_engine::commands::find(command) else { return Vec::new() };
    let schema = (cmd.params)();
    let required: Vec<String> =
        schema.get("required").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let context_covers = |n: &str| has_context && matches!(n, "no" | "customer_no" | "vendor_no" | "document_type" | "table" | "key");
    let mut out: Vec<String> = required.into_iter().filter(|r| !params.contains_key(r) && !context_covers(r)).collect();
    if !out.is_empty() {
        // Offer the optional ones too (e.g. applies-to document, posting date).
        if let Some(props) = schema.get("properties").and_then(Json::as_object) {
            for k in props.keys() {
                if !out.contains(k) && !params.contains_key(k) && !context_covers(k) {
                    out.push(k.clone());
                }
            }
        }
    }
    out
}

pub fn show(app: &mut ErpApp, ctx: &egui::Context) {
    tell_me(app, ctx);
    let Some(dialog) = app.dialog.take() else { return };
    let mut keep = true;
    let mut then_run: Option<(String, Json)> = None;
    let mut dialog = dialog;
    let modal = egui::Modal::new(Id::unique("erp_dialog")).show(ctx, |ui| {
        ui.set_max_width(560.0);
        match &mut dialog {
            Dialog::Error(msg) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠").size(22.0).color(TOKENS.danger));
                    ui.label(RichText::new("Something went wrong").font(theme::bold(theme::SUBTITLE)));
                });
                ui.add_space(6.0);
                ui.label(msg.as_str());
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, "OK").clicked() {
                        keep = false;
                    }
                });
            }
            Dialog::Confirm { text, command, params } => {
                ui.label(RichText::new(text.as_str()).size(theme::SUBTITLE));
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  No  ").clicked() {
                        keep = false;
                    }
                    if primary(ui, "Yes").clicked() {
                        then_run = Some((command.clone(), params.clone()));
                        keep = false;
                    }
                });
            }
            Dialog::Form { title, submit, fields } => {
                ui.label(RichText::new(title.as_str()).font(theme::bold(theme::SUBTITLE)));
                ui.add_space(8.0);
                egui::Grid::new("form").num_columns(2).spacing(vec2(14.0, 8.0)).show(ui, |ui| {
                    for f in fields.iter_mut() {
                        ui.label(RichText::new(&f.caption).color(TOKENS.text_dim));
                        if f.options.is_empty() {
                            ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(260.0).hint_text(&f.hint));
                        } else {
                            egui::ComboBox::from_id_salt(("form_opt", &f.name)).selected_text(if f.value.is_empty() { " " } else { f.value.as_str() }).width(260.0).show_ui(ui, |ui| {
                                for o in &f.options {
                                    ui.selectable_value(&mut f.value, o.clone(), if o.is_empty() { " " } else { o.as_str() });
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  Cancel  ").clicked() {
                        keep = false;
                    }
                    if primary(ui, "OK").clicked() {
                        keep = false;
                        let values: Map<String, Json> =
                            fields.iter().filter(|f| !f.value.trim().is_empty()).map(|f| (f.name.clone(), json!(f.value.trim()))).collect();
                        then_run = Some(match submit {
                            Submit::Command { command, base } => {
                                let mut p = base.as_object().cloned().unwrap_or_default();
                                p.extend(values);
                                (command.clone(), Json::Object(p))
                            }
                            Submit::NewRecord { table } => ("record.new".to_string(), json!({"table": table, "values": values})),
                        });
                    }
                });
            }
            Dialog::SelectItems { command, base, items } => {
                ui.label(RichText::new("Select items").font(theme::bold(theme::SUBTITLE)));
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("select_items").num_columns(2).spacing(vec2(14.0, 6.0)).show(ui, |ui| {
                        for (no, desc, on) in items.iter_mut() {
                            ui.checkbox(on, RichText::new(no.as_str()).color(TOKENS.accent));
                            ui.label(desc.as_str());
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  Cancel  ").clicked() {
                        keep = false;
                    }
                    let chosen: Vec<Json> = items.iter().filter(|(_, _, on)| *on).map(|(no, _, _)| json!(no)).collect();
                    if ui.add_enabled(!chosen.is_empty(), egui::Button::new("  OK  ").fill(TOKENS.accent)).clicked() {
                        keep = false;
                        let mut p = base.as_object().cloned().unwrap_or_default();
                        p.insert("items".into(), Json::Array(chosen));
                        then_run = Some((command.clone(), Json::Object(p)));
                    }
                });
            }
            Dialog::Report { title, columns, rows } => {
                ui.label(RichText::new(title.as_str()).font(theme::bold(theme::SUBTITLE)));
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                    egui::Grid::new("report").num_columns(columns.len()).striped(true).spacing(vec2(18.0, 5.0)).show(ui, |ui| {
                        for c in columns.iter() {
                            ui.label(RichText::new(c).size(theme::SMALL).color(TOKENS.text_dim).strong());
                        }
                        ui.end_row();
                        for r in rows.iter() {
                            for (i, c) in r.iter().enumerate() {
                                let numeric = i > 0 && c.chars().next().is_some_and(|ch| ch.is_ascii_digit() || ch == '-') && c.contains('.');
                                if numeric {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(c));
                                } else {
                                    ui.label(c);
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, "Close").clicked() {
                        keep = false;
                    }
                });
            }
        }
    });
    if modal.should_close() && then_run.is_none() {
        keep = false;
    }
    if keep {
        app.dialog = Some(dialog);
    }
    if let Some((c, p)) = then_run {
        app.run(&c, p);
    }
}

fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(format!("  {text}  ")).color(egui::Color32::WHITE)).fill(TOKENS.accent))
}

fn tell_me(app: &mut ErpApp, ctx: &egui::Context) {
    let Some(mut tm) = app.tell_me.take() else { return };
    let results = if tm.query.trim().is_empty() { Vec::new() } else { app.session.search_pages(&tm.query) };
    let mut open: Option<String> = None;
    let mut close = false;
    let (down, up, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    if down {
        tm.selected = (tm.selected + 1).min(results.len().saturating_sub(1));
    }
    if up {
        tm.selected = tm.selected.saturating_sub(1);
    }
    if enter && let Some(p) = results.get(tm.selected) {
        open = Some(p.id.clone());
    }
    let modal = egui::Modal::new(Id::unique("tell_me")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new("Tell me what you want to do").font(theme::bold(theme::SUBTITLE)));
        ui.add_space(6.0);
        let r = ui.add(egui::TextEdit::singleline(&mut tm.query).hint_text("e.g. sales orders, customers, chart of accounts").desired_width(f32::INFINITY));
        r.request_focus();
        if r.changed() {
            tm.selected = 0;
        }
        ui.add_space(8.0);
        if !results.is_empty() {
            ui.label(RichText::new("Pages and Tasks").size(theme::SMALL).color(TOKENS.text_dim).strong());
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, p) in results.iter().enumerate() {
                let kind = match p.kind {
                    PageKind::List => "Lists",
                    PageKind::Card => "Card",
                    PageKind::Document => "Documents",
                    PageKind::ListPart => "Part",
                };
                let row = ui.horizontal(|ui| {
                    let t = RichText::new(&p.caption);
                    let r = ui.selectable_label(i == tm.selected, if i == tm.selected { t.color(TOKENS.accent).strong() } else { t });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(kind).size(theme::SMALL).color(TOKENS.text_dim)));
                    r
                });
                if row.inner.clicked() {
                    open = Some(p.id.clone());
                }
            }
            if !tm.query.trim().is_empty() && results.is_empty() {
                ui.label(RichText::new("No results").italics().color(TOKENS.text_dim));
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if let Some(p) = open {
        app.open_page(&p, Vec::new(), None);
    } else if !close {
        app.tell_me = Some(tm);
    }
}
