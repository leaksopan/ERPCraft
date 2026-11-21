//! List pages: header, action bar, filter chips, the grid and the FactBox pane.

use egui::{Align, Layout, RichText, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use erpcraft_engine::meta::{ActionArea, ActionDef, Key, PageDef, Record, TableDef};
use serde_json::json;

use crate::nav::{ListState, View};
use crate::theme::{self, TOKENS};
use crate::widgets;
use crate::{Dialog, ErpApp};

pub fn show(app: &mut ErpApp, ui: &mut Ui, idx: usize, mut state: ListState) {
    let depth = app.stack.len();
    let Some(page) = app.session.page(&state.page) else {
        app.back();
        return;
    };
    let Some(table) = page.source_table.as_deref().and_then(|t| app.session.table(t)) else { return };
    let mut rows = app.session.page_records(page, &state.filters, &state.search);
    if let Some((field, desc)) = &state.sort {
        rows.sort_by(|a, b| {
            let o = a.get(field).cmp(b.get(field));
            if *desc { o.reverse() } else { o }
        });
    }
    if state.selected.as_ref().is_none_or(|k| !rows.iter().any(|r| &r.key(table) == k)) {
        state.selected = rows.first().map(|r| r.key(table));
    }
    let inline_edit = page.editable && page.card_page.is_none();

    // BC list header: one bar with "Caption: All ▾", search, New / Delete and the action menus.
    egui::Frame::new().fill(TOKENS.page).inner_margin(egui::Margin { left: 24, right: 16, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.set_width(ui.available_width().max(0.0));
        action_bar(app, ui, page, table, &mut state, inline_edit);
    });
    if state.filter_pane {
        egui::Panel::left(("filter_pane", &page.id))
            .resizable(false)
            .exact_size(260.0)
            .frame(egui::Frame::new().fill(TOKENS.page).inner_margin(egui::Margin { left: 24, right: 12, top: 12, bottom: 12 }))
            .show(ui, |ui| filter_pane(app, ui, table, &mut state));
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
            // Wide lists scroll sideways, like BC's grid.
            egui::ScrollArea::horizontal().id_salt(("list_h", &page.id)).auto_shrink([false, false]).show(ui, |ui| {
                grid(app, ui, page, table, &rows, &mut state, inline_edit);
            });
        });
    });

    // Keyboard shortcuts of the page's actions act on the selected row (not while a card is open
    // over the list).
    for a in &page.actions {
        if let Some(s) = &a.shortcut
            && ui.is_enabled()
            && widgets::shortcut_pressed(ui.ctx(), s)
        {
            let key = state.selected.clone();
            app.run_action(a, key.as_ref().map(|k| (table.name.as_str(), k)));
        }
    }
    // Write the state back unless the action opened another page.
    if app.stack.len() == depth
        && let Some(View::List(s)) = app.stack.get_mut(idx)
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
            columns_menu(app, ui, page, table);
            let filter_tip = if state.filter_pane { "Hide filter pane" } else { "Show filter pane (Shift+F3)" };
            let fill = if state.filters.is_empty() { TOKENS.text } else { TOKENS.accent };
            if ui.add(egui::Button::new(RichText::new("⏷").size(15.0).color(fill)).frame(false)).on_hover_text(filter_tip).clicked() {
                state.filter_pane = !state.filter_pane;
            }
        });
    });
    if let Some(a) = run {
        app.run_action(&a, ctx_key);
    }
}

/// BC's list action bar: one drop-down per promoted category ("Home ∨", "Customer ∨") and "⋯"
/// with every action; without categories, Home actions as buttons and a drop-down per area.
pub fn action_menus(ui: &mut Ui, page: &PageDef, run: &mut Option<ActionDef>) {
    if !page.categories.is_empty() {
        for tab in crate::card::action_tab_list(page) {
            let caption = if tab.caption == "More options" { "⋯".to_string() } else { format!("{}  ▾", tab.caption) };
            action_menu(ui, &caption, &tab.actions, run);
        }
        return;
    }
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
        action_menu(ui, &format!("{}  ▾", area.caption()), &actions, run);
    }
}

fn action_menu(ui: &mut Ui, caption: &str, actions: &[&ActionDef], run: &mut Option<ActionDef>) {
    ui.menu_button(RichText::new(caption).color(TOKENS.text), |ui| {
        ui.set_min_width(200.0);
        for a in actions {
            let mut label = a.caption.replace('&', "");
            if let Some(s) = &a.shortcut {
                label = format!("{label}      {s}");
            }
            let r = ui.button(label);
            let r = if a.tooltip.is_empty() { r } else { r.on_hover_text(&a.tooltip) };
            if r.clicked() {
                *run = Some((*a).clone());
                ui.close();
            }
        }
    });
}

fn grid(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, rows: &[Record], state: &mut ListState, inline_edit: bool) {
    if rows.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("(There is nothing to show in this view)").italics().color(TOKENS.text_dim)));
        return;
    }
    let chosen = app.columns.get(&page.id).cloned().unwrap_or_else(|| page.columns.clone());
    let cols: Vec<&erpcraft_engine::meta::FieldDef> = chosen.iter().filter_map(|c| table.field(c)).collect();
    let lcy = app.lcy_symbol();
    let mut open: Option<Key> = None;
    let mut commit: Option<(Key, String, serde_json::Value)> = None;
    let lookups = crate::card::lookups(app, table, cols.iter().map(|f| f.name.as_str()));
    let mut builder = TableBuilder::new(ui)
        .id_salt(("list", &page.id))
        .striped(false)
        .resizable(false)
        .sense(egui::Sense::click())
        .auto_shrink([true, false])
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
                    let arrow = match &state.sort {
                        Some((s, desc)) if *s == f.name => if *desc { " ▼" } else { " ▲" },
                        _ => "",
                    };
                    let r = ui.with_layout(layout, |ui| {
                        ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", widgets::lcy_caption(&f.caption, &lcy))).size(theme::SMALL).color(TOKENS.text_dim).strong()).sense(egui::Sense::click()))
                    });
                    let r = r.inner.on_hover_text("Sort by this column");
                    if r.clicked() {
                        state.sort = match &state.sort {
                            Some((s, false)) if *s == f.name => Some((f.name.clone(), true)),
                            Some((s, true)) if *s == f.name => None,
                            _ => Some((f.name.clone(), false)),
                        };
                    }
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
            let list = state.clone();
            app.open_card_from_list(&t, k, &list);
        }
    }
}

/// BC's filter pane: "Filter list by" with one expression per field (`10000..20000`, `<>0`,
/// `*text*`, `''`), a field picker to add more.
fn filter_pane(app: &mut ErpApp, ui: &mut Ui, table: &TableDef, state: &mut ListState) {
    ui.label(RichText::new("Filter list by:").font(theme::bold(theme::BODY)).color(TOKENS.text));
    ui.add_space(6.0);
    let mut remove = None;
    for (i, (field, expr)) in state.filters.iter_mut().enumerate() {
        let caption = table.field(field).map_or(field.clone(), |f| f.caption.clone());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("× {caption}")).color(TOKENS.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("✕").frame(false)).on_hover_text("Remove filter").clicked() {
                    remove = Some(i);
                }
            });
        });
        let id = format!("filter|{}|{field}", table.name);
        let buf = app.bufs.entry(id.clone()).or_insert_with(|| expr.clone());
        let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY).hint_text("e.g. 10000..20000, <>0, *oil*").id_salt(&id));
        if r.lost_focus() || (r.changed() && !r.has_focus()) {
            *expr = buf.clone();
            app.bufs.remove(&id);
        }
        ui.add_space(8.0);
    }
    if let Some(i) = remove {
        state.filters.remove(i);
    }
    ui.add_space(4.0);
    ui.menu_button(RichText::new("+ Filter...").color(TOKENS.accent), |ui| {
        ui.set_min_width(220.0);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for f in &table.fields {
                if state.filters.iter().any(|(n, _)| *n == f.name) {
                    continue;
                }
                if ui.button(&f.caption).clicked() {
                    state.filters.push((f.name.clone(), String::new()));
                    ui.close();
                }
            }
        });
    });
    ui.add_space(12.0);
    ui.label(RichText::new("Filters use Business Central syntax: = value, <>, >, <, a..b, a|b, *text*, '' for blank.").size(theme::SMALL).color(TOKENS.text_dim));
}

/// Personalize › columns: show or hide any field of the table as a column.
fn columns_menu(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef) {
    let current = app.columns.get(&page.id).cloned().unwrap_or_else(|| page.columns.clone());
    let mut next = current.clone();
    ui.menu_button(RichText::new("⚙").size(15.0), |ui| {
        ui.set_min_width(240.0);
        ui.label(RichText::new("Columns").font(theme::bold(theme::BODY)));
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for f in &table.fields {
                let mut on = next.contains(&f.name);
                if ui.checkbox(&mut on, &f.caption).changed() {
                    if on {
                        next.push(f.name.clone());
                    } else {
                        next.retain(|c| *c != f.name);
                    }
                }
            }
        });
        ui.separator();
        if ui.button("Reset to the page's columns").clicked() {
            next = page.columns.clone();
            ui.close();
        }
    })
    .response
    .on_hover_text("Choose columns");
    if next != current {
        // Keep the table's field order for added columns.
        let mut ordered: Vec<String> = current.iter().filter(|c| next.contains(c)).cloned().collect();
        for f in &table.fields {
            if next.contains(&f.name) && !ordered.contains(&f.name) {
                ordered.push(f.name.clone());
            }
        }
        app.columns.insert(page.id.clone(), ordered);
    }
}
