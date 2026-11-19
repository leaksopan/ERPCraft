//! List pages: header, action bar, filter chips, the grid and the FactBox pane.

use egui::{Align, Layout, RichText, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use erpcraft_engine::meta::{ActionArea, ActionDef, Key, PageDef, Record, TableDef};
use serde_json::json;

use crate::nav::{ListState, View};
use crate::theme::{self, TOKENS};
use crate::widgets;
use crate::{Dialog, ErpApp};

pub fn show(app: &mut ErpApp, ui: &mut Ui, mut state: ListState) {
    let depth = app.stack.len();
    let Some(page) = app.session.page(&state.page) else {
        app.back();
        return;
    };
    let Some(table) = page.source_table.as_deref().and_then(|t| app.session.table(t)) else { return };
    let rows = app.session.page_records(page, &state.filters, &state.search);
    if state.selected.as_ref().is_none_or(|k| !rows.iter().any(|r| &r.key(table) == k)) {
        state.selected = rows.first().map(|r| r.key(table));
    }
    let inline_edit = page.editable && page.card_page.is_none();

    // BC list header: one bar with "Caption: All ▾", search, New / Delete and the action menus.
    egui::Frame::new().fill(TOKENS.page).inner_margin(egui::Margin { left: 24, right: 16, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.set_width(ui.available_width().max(0.0));
        action_bar(app, ui, page, table, &mut state, inline_edit);
    });
    if !state.filters.is_empty() {
        egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 16, top: 6, bottom: 0 }).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Filters:").size(theme::SMALL).color(TOKENS.text_dim));
                let mut remove = None;
                for (i, (f, v)) in state.filters.iter().enumerate() {
                    let chip = egui::Button::new(RichText::new(format!("{f}: {v}  ✕")).size(theme::SMALL)).fill(TOKENS.accent_soft).corner_radius(10);
                    if ui.add(chip).clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    state.filters.remove(i);
                }
            });
        });
    }
    let has_factbox = !page.factboxes.is_empty() && app.show_factbox;
    if has_factbox {
        egui::Panel::right(("list_factbox", &page.id))
            .resizable(true)
            .default_size(300.0)
            .frame(egui::Frame::new().fill(TOKENS.canvas).inner_margin(egui::Margin { left: 12, right: 16, top: 12, bottom: 12 }))
            .show(ui, |ui| {
                let rec = state.selected.as_ref().and_then(|k| app.session.record(&table.name, k));
                crate::card::factbox_pane(app, ui, page, rec.as_ref());
            });
    }
    egui::CentralPanel::default().frame(egui::Frame::new().fill(TOKENS.canvas).inner_margin(egui::Margin { left: 24, right: 12, top: 12, bottom: 12 })).show(ui, |ui| {
        widgets::card_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size().max(egui::Vec2::ZERO));
            grid(app, ui, page, table, &rows, &mut state, inline_edit);
        });
    });

    // Keyboard shortcuts of the page's actions act on the selected row.
    for a in &page.actions {
        if let Some(s) = &a.shortcut
            && widgets::shortcut_pressed(ui.ctx(), s)
        {
            let key = state.selected.clone();
            app.run_action(a, key.as_ref().map(|k| (table.name.as_str(), k)));
        }
    }
    // Write the state back unless the action opened another page.
    if app.stack.len() == depth
        && let Some(View::List(s)) = app.stack.last_mut()
    {
        *s = state;
    }
}

fn action_bar(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, state: &mut ListState, inline_edit: bool) {
    let selected = state.selected.clone();
    let ctx_key = selected.as_ref().map(|k| (table.name.as_str(), k));
    let mut run: Option<ActionDef> = None;
    ui.horizontal(|ui| {
        widgets::flat(ui);
        ui.set_min_height(36.0);
        ui.label(RichText::new(format!("{}:", page.caption)).font(theme::bold(theme::BODY)).color(TOKENS.text));
        ui.label(RichText::new("All ▾").color(TOKENS.accent));
        widgets::vsep(ui);
        if widgets::action_button(ui, "🔍", "").on_hover_text("Search").clicked() {
            state.search_open = !state.search_open;
        }
        if state.search_open {
            let r = ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search").desired_width(180.0));
            if !r.has_focus() && state.search.is_empty() && ui.input(|i| i.pointer.any_click()) && !r.hovered() {
                state.search_open = false;
            }
        }
        if page.editable || page.card_page.is_some() {
            if widgets::action_button(ui, "+", "New").clicked() {
                if let Some(card) = &page.card_page {
                    app.new_record_on(card);
                } else if inline_edit {
                    app.dialog = Some(Dialog::new_record(page, table));
                }
            }
            if widgets::action_button(ui, "🗑", "Delete").clicked()
                && let Some(k) = &selected
            {
                app.dialog = Some(Dialog::Confirm {
                    text: format!("Delete {} {}?", table.caption, k.display()),
                    command: "record.delete".into(),
                    params: json!({"table": table.name, "key": k.to_json()}),
                });
            }
        }
        if let Some(card) = &page.card_page
            && widgets::action_button(ui, "✎", "Edit").clicked()
            && let Some(k) = &selected
        {
            let target = app.session.record(&table.name, k).and_then(|r| app.session.card_page_for(&table.name, &r)).unwrap_or_else(|| card.clone());
            app.open_page(&target, Vec::new(), Some(k.clone()));
        }
        action_menus(ui, page, &mut run);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !page.factboxes.is_empty() {
                let tip = if app.show_factbox { "Hide FactBox pane" } else { "Show FactBox pane" };
                if ui.add(egui::Button::new(RichText::new("ⓘ").size(16.0)).frame(false)).on_hover_text(tip).clicked() {
                    app.show_factbox = !app.show_factbox;
                }
            }
        });
    });
    if let Some(a) = run {
        app.run_action(&a, ctx_key);
    }
}

/// Promoted (Home) actions as buttons, then one drop-down per other action area.
pub fn action_menus(ui: &mut Ui, page: &PageDef, run: &mut Option<ActionDef>) {
    for a in page.actions.iter().filter(|a| a.area == ActionArea::Home) {
        let r = widgets::action_button(ui, widgets::glyph(&a.icon), &a.caption.replace('&', ""));
        let r = if a.tooltip.is_empty() { r } else { r.on_hover_text(&a.tooltip) };
        if r.clicked() {
            *run = Some(a.clone());
        }
    }
    for area in [ActionArea::Process, ActionArea::New, ActionArea::Navigate, ActionArea::Report] {
        let actions: Vec<&ActionDef> = page.actions.iter().filter(|a| a.area == area).collect();
        if actions.is_empty() {
            continue;
        }
        ui.menu_button(RichText::new(format!("{}  ▾", area.caption())).color(TOKENS.text), |ui| {
            ui.set_min_width(200.0);
            for a in actions {
                let mut label = a.caption.replace('&', "");
                if let Some(s) = &a.shortcut {
                    label = format!("{label}      {s}");
                }
                let r = ui.button(label);
                let r = if a.tooltip.is_empty() { r } else { r.on_hover_text(&a.tooltip) };
                if r.clicked() {
                    *run = Some(a.clone());
                    ui.close();
                }
            }
        });
    }
}

fn grid(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, rows: &[Record], state: &mut ListState, inline_edit: bool) {
    if rows.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("(There is nothing to show in this view)").italics().color(TOKENS.text_dim)));
        return;
    }
    let cols: Vec<&erpcraft_engine::meta::FieldDef> = page.columns.iter().filter_map(|c| table.field(c)).collect();
    let mut open: Option<Key> = None;
    let mut commit: Option<(Key, String, serde_json::Value)> = None;
    let lookups = crate::card::lookups(app, table, cols.iter().map(|f| f.name.as_str()));
    let mut builder = TableBuilder::new(ui)
        .id_salt(("list", &page.id))
        .striped(false)
        .resizable(false)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(Layout::left_to_right(Align::Center));
    for f in &cols {
        let w = match f.ty {
            erpcraft_engine::meta::FieldType::Decimal => 120.0,
            erpcraft_engine::meta::FieldType::Text(n) if n > 50 => 200.0,
            erpcraft_engine::meta::FieldType::Text(_) => 120.0,
            erpcraft_engine::meta::FieldType::Boolean | erpcraft_engine::meta::FieldType::Option(_) => 80.0,
            erpcraft_engine::meta::FieldType::Date => 95.0,
            _ => 100.0,
        };
        builder = builder.column(Column::initial(w).at_least(50.0).clip(true));
    }
    builder
        .header(28.0, |mut h| {
            for f in &cols {
                h.col(|ui| {
                    let layout = if f.ty.is_numeric() { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
                    ui.with_layout(layout, |ui| ui.label(RichText::new(&f.caption).size(theme::SMALL).color(TOKENS.text_dim).strong()));
                });
            }
        })
        .body(|body| {
            body.rows(30.0, rows.len(), |mut row| {
                let Some(r) = rows.get(row.index()) else { return };
                let key = r.key(table);
                row.set_selected(state.selected.as_ref() == Some(&key));
                for (i, f) in cols.iter().enumerate() {
                    row.col(|ui| {
                        let cell = ui.max_rect();
                        ui.painter().hline(egui::Rangef::new(cell.left() - 4.0, cell.right() + 4.0), cell.bottom(), egui::Stroke::new(1.0, TOKENS.divider));
                        let v = r.get(&f.name);
                        let editable = inline_edit && f.editable && !f.is_flow() && !table.primary_key.contains(&f.name);
                        if editable {
                            let id = format!("cell|{}|{}|{}", table.name, key.display(), f.name);
                            let lk = lookups.get(&f.name);
                            if let Some(val) = widgets::field_editor(&mut app.bufs, ui, &id, f, v, true, lk, ui.available_width() - 4.0) {
                                commit = Some((key.clone(), f.name.clone(), val));
                            }
                            return;
                        }
                        let text = widgets::display(v, &f.ty);
                        if i == 0 && page.card_page.is_some() {
                            if widgets::strong_link(ui, &text).clicked() {
                                open = Some(key.clone());
                            }
                        } else if f.ty.is_numeric() {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let t = RichText::new(text);
                                ui.label(if f.is_flow() { t.color(TOKENS.accent) } else { t });
                            });
                        } else {
                            ui.label(text);
                        }
                    });
                }
                let resp = row.response();
                if resp.clicked() {
                    state.selected = Some(key.clone());
                }
                if resp.double_clicked() && page.card_page.is_some() {
                    open = Some(key.clone());
                }
            });
        });
    let _ = vec2(0.0, 0.0);
    if let Some((key, field, value)) = commit {
        app.run("record.validate", json!({"table": table.name, "key": key.to_json(), "field": field, "value": value}));
    }
    if let Some(k) = open {
        state.selected = Some(k.clone());
        let target = app
            .session
            .record(&table.name, &k)
            .and_then(|r| app.session.card_page_for(&table.name, &r))
            .or_else(|| page.card_page.clone());
        if let Some(t) = target {
            app.open_page(&t, Vec::new(), Some(k));
        }
    }
}
