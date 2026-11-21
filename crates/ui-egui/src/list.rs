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
        // A list opened over another page (a drill-down, a Related action) gets BC's ← back.
        let can_back = idx > 0 && app.stack.get(..idx).is_some_and(|below| below.iter().any(|v| !matches!(v, View::RoleCenter)));
        action_bar(app, ui, page, table, &mut state, inline_edit, can_back);
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
            app.selection = state.marked.clone();
            app.run_action(a, key.as_ref().map(|k| (table.name.as_str(), k)));
            app.selection.clear();
        }
    }
    // Write the state back unless the action opened another page.
    if app.stack.len() == depth
        && let Some(View::List(s)) = app.stack.get_mut(idx)
    {
        *s = state;
    }
}

fn action_bar(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, state: &mut ListState, inline_edit: bool, can_back: bool) {
    let selected = state.selected.clone();
    let ctx_key = selected.as_ref().map(|k| (table.name.as_str(), k));
    let mut run: Option<ActionDef> = None;
    ui.horizontal(|ui| {
        widgets::flat(ui);
        ui.set_min_height(36.0);
        if can_back {
            let back = egui::Button::new(RichText::new("←").size(15.0).color(TOKENS.accent_hover)).fill(TOKENS.accent_soft).corner_radius(14).min_size(vec2(28.0, 28.0));
            if ui.add(back).on_hover_text("Back (Esc)").clicked() {
                app.back();
                return;
            }
        }
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
        let tabbed = page.worksheet && !page.categories.is_empty();
        if (page.editable || page.card_page.is_some()) && !tabbed {
            if widgets::action_button(ui, "+", "New").clicked() {
                if let Some(card) = &page.card_page {
                    app.new_record_on(card);
                } else if inline_edit {
                    app.dialog = Some(Dialog::new_record(page, table, &state.filters));
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
        if !tabbed {
            action_menus(ui, page, &mut run);
        }
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
    // BC's worksheets (journals): Manage | Home | Line | ... tabs, the chosen one's actions below.
    if page.worksheet && !page.categories.is_empty() {
        let tabs: Vec<crate::card::ActionTab> = crate::card::action_tab_list(page).into_iter().filter(|t| t.caption != "More options").collect();
        let id = egui::Id::unique(("worksheet_tab", &page.id));
        let home = tabs.iter().position(|t| t.caption == "Home").map_or(0, |i| i + 1);
        let mut current = ui.ctx().data(|d| d.get_temp::<usize>(id)).unwrap_or(home).min(tabs.len());
        ui.horizontal(|ui| {
            widgets::flat(ui);
            let captions: Vec<String> = std::iter::once("Manage".to_string()).chain(tabs.iter().map(|t| t.caption.clone())).collect();
            for (i, c) in captions.iter().enumerate() {
                let sel = i == current;
                let r = ui.add(egui::Button::new(RichText::new(c).color(if sel { TOKENS.accent } else { TOKENS.text })).frame(false));
                if sel {
                    ui.painter().line_segment([r.rect.left_bottom() + vec2(6.0, 0.0), r.rect.right_bottom() - vec2(6.0, 0.0)], egui::Stroke::new(2.0, TOKENS.accent));
                }
                if r.clicked() {
                    current = i;
                }
            }
            let all: Vec<&ActionDef> = page.actions.iter().collect();
            action_menu(ui, "More options", &all, &mut run);
        });
        ui.horizontal(|ui| {
            widgets::flat(ui);
            ui.set_min_height(32.0);
            if current == 0 {
                if page.editable && widgets::action_button(ui, "+", "New Line").clicked() && inline_edit {
                    app.dialog = Some(Dialog::new_record(page, table, &state.filters));
                }
                if widgets::action_button(ui, "🗑", "Delete Line").clicked()
                    && let Some(k) = &selected
                {
                    app.dialog = Some(Dialog::Confirm {
                        text: format!("Delete {} {}?", table.caption, k.display()),
                        command: "record.delete".into(),
                        params: json!({"table": table.name, "key": k.to_json()}),
                    });
                }
            } else if let Some(tab) = tabs.get(current - 1) {
                crate::card::action_row(ui, tab, &mut run);
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, current));
    }
    if let Some(a) = run {
        app.selection = state.marked.clone();
        app.run_action(&a, ctx_key);
        app.selection.clear();
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

pub(crate) fn action_menu(ui: &mut Ui, caption: &str, actions: &[&ActionDef], run: &mut Option<ActionDef>) {
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

/// What a row's ⋮ menu asked for.
enum RowAct {
    Action(ActionDef),
    SelectMore,
    Toggle,
    View,
    Edit,
    Delete,
}

/// BC's list grid: 34 px rows with a line above each, the key column as a link, a ⋮ column with
/// the row's actions, headers with a menu (sort, filter, What's this?), one framed current cell,
/// arrow keys to move between rows.
fn grid(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, rows: &[Record], state: &mut ListState, inline_edit: bool) {
    use erpcraft_engine::meta::FieldType;
    if rows.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("(There is nothing to show in this view)").italics().color(TOKENS.text_dim)));
        return;
    }
    let chosen = app.columns.get(&page.id).cloned().unwrap_or_else(|| page.columns.clone());
    let cols: Vec<&erpcraft_engine::meta::FieldDef> = chosen.iter().filter_map(|c| table.field(c)).collect();
    let captions: Vec<String> = cols.iter().map(|f| app.field_caption(f, &f.caption)).collect();
    let mut open: Option<(Key, bool)> = None;
    let mut commit: Option<(Key, String, serde_json::Value)> = None;
    let mut row_act: Option<(Key, RowAct)> = None;
    let lookups = crate::card::lookups(app, table, cols.iter().map(|f| f.name.as_str()));

    // Arrow keys move the current row when nothing else takes the keyboard.
    let current = state.selected.as_ref().and_then(|k| rows.iter().position(|r| r.key(table) == *k));
    let mut scroll_to = None;
    if ui.is_enabled() && !ui.ctx().egui_wants_keyboard_input() {
        let (down, up) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp)));
        let next = match (current, down, up) {
            (Some(i), true, _) => Some((i + 1).min(rows.len() - 1)),
            (Some(i), _, true) => Some(i.saturating_sub(1)),
            (None, true, _) => Some(0),
            _ => None,
        };
        if let Some(n) = next
            && let Some(r) = rows.get(n)
        {
            state.selected = Some(r.key(table));
            scroll_to = Some(n);
        }
    }
    // The key column shows BC's ↑ while the list is in its key order.
    let key_col = table.primary_key.last().cloned().unwrap_or_default();
    let filter_value = state.selected.as_ref().and_then(|k| rows.iter().find(|r| r.key(table) == *k)).cloned();

    // Row lines are drawn after the table, one across its full width (BC draws a line between rows,
    // not under each cell).
    let mut row_tops: Vec<f32> = Vec::new();
    let mut span = (f32::MAX, f32::MIN);
    let mut body_clip = egui::Rect::NOTHING;
    let grid_rect = ui.max_rect();
    ui.spacing_mut().item_spacing.y = 0.0;
    // Column edges stay invisible until hovered (egui draws resize lines with this stroke).
    ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    let ui_mods = ui.input(|i| i.modifiers);
    let mut builder = TableBuilder::new(ui)
        .id_salt(("list", &page.id))
        .striped(false)
        // BC's columns can be dragged wider or narrower at the header's edge.
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([true, false])
        .cell_layout(Layout::left_to_right(Align::Center));
    if let Some(n) = scroll_to {
        builder = builder.scroll_to_row(n, None);
    }
    for (i, f) in cols.iter().enumerate() {
        let w = match f.ty {
            FieldType::Decimal => 110.0,
            FieldType::Text(n) if n > 50 => 210.0,
            FieldType::Text(_) => 120.0,
            FieldType::Boolean | FieldType::Option(_) => 90.0,
            FieldType::Date => 95.0,
            _ => 90.0,
        };
        builder = builder.column(Column::initial(w).at_least(40.0).clip(true));
        if i == 0 {
            builder = builder.column(Column::exact(30.0).resizable(false));
        }
    }
    builder
        .header(58.0, |mut h| {
            for (i, f) in cols.iter().enumerate() {
                let caption = captions.get(i).cloned().unwrap_or_default();
                h.col(|ui| header_cell(ui, f, &caption, &key_col, state, filter_value.as_ref()));
                if i == 0 {
                    h.col(|_| {});
                }
            }
        })
        .body(|body| {
            body.rows(34.0, rows.len(), |mut row| {
                let Some(r) = rows.get(row.index()) else { return };
                let key = r.key(table);
                let selected = state.selected.as_ref() == Some(&key);
                row.set_selected(selected || state.marked.contains(&key));
                for (i, f) in cols.iter().enumerate() {
                    let (_, cell_resp) = row.col(|ui| {
                        let cell = ui.max_rect();
                        span.0 = span.0.min(cell.left());
                        span.1 = span.1.max(cell.right());
                        body_clip = body_clip.union(ui.clip_rect());
                        if i == 0 {
                            row_tops.push(cell.top());
                        }
                        if selected && state.cell == Some(i) {
                            ui.painter().rect_stroke(cell.shrink2(vec2(0.5, 2.0)), 0.0, egui::Stroke::new(1.0, TOKENS.grid_text), egui::StrokeKind::Inside);
                        }
                        let v = r.get(&f.name);
                        let editable = inline_edit && f.editable && !f.is_flow() && !table.primary_key.contains(&f.name);
                        if editable {
                            let id = format!("cell|{}|{}|{}", table.name, key.display(), f.name);
                            if let Some(val) = widgets::field_editor(&mut app.bufs, ui, &id, f, v, true, lookups.get(&f.name), ui.available_width() - 4.0) {
                                commit = Some((key.clone(), f.name.clone(), val));
                            }
                            return;
                        }
                        match grid_value(ui, f, v, i == 0 && page.card_page.is_some(), selected) {
                            Some(CellClick::Key) => open = Some((key.clone(), false)),
                            Some(CellClick::DrillDown) => crate::queue_drill_down(ui.ctx(), f, r),
                            None => {}
                        }
                    });
                    if cell_resp.clicked() {
                        state.cell = Some(i);
                    }
                    if i == 0 {
                        row.col(|ui| {
                            let cell = ui.max_rect();
                            span.1 = span.1.max(cell.right());
                            let hovered = ui
                                .ctx()
                                .pointer_hover_pos()
                                .is_some_and(|p| p.y >= cell.top() && p.y < cell.bottom() && grid_rect.x_range().contains(p.x) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()));
                            if state.select_more {
                                // Select More: a check mark per row instead of the ⋮ menu.
                                let mark = if state.marked.contains(&key) { "☑" } else { "☐" };
                                if ui.add(egui::Button::new(RichText::new(mark).size(15.0).color(TOKENS.grid_text)).frame(false)).clicked() {
                                    row_act = Some((key.clone(), RowAct::Toggle));
                                }
                            } else if let Some(act) = row_menu(ui, page, selected || hovered, state.marked.len() > 1) {
                                row_act = Some((key.clone(), act));
                            }
                        });
                    }
                }
                let resp = row.response();
                if resp.clicked() {
                    let mods = ui_mods;
                    if state.select_more || mods.command {
                        // Ctrl+click (or Select More): mark or unmark this row.
                        if state.marked.is_empty()
                            && let Some(cur) = state.selected.clone()
                            && cur != key
                        {
                            state.marked.push(cur);
                        }
                        if let Some(i) = state.marked.iter().position(|k| *k == key) {
                            state.marked.remove(i);
                        } else {
                            state.marked.push(key.clone());
                        }
                    } else if mods.shift {
                        // Shift+click: everything from the current row to this one.
                        let from = state.selected.as_ref().and_then(|k| rows.iter().position(|r| r.key(table) == *k)).unwrap_or(0);
                        let to = row.index();
                        let (a, b) = if from <= to { (from, to) } else { (to, from) };
                        state.marked = rows.get(a..=b).unwrap_or_default().iter().map(|r| r.key(table)).collect();
                    } else {
                        state.marked.clear();
                    }
                    state.selected = Some(key.clone());
                }
                if resp.double_clicked() && page.card_page.is_some() {
                    open = Some((key.clone(), false));
                }
            });
        });
    if span.0 < span.1 {
        let painter = ui.painter().with_clip_rect(body_clip);
        for y in row_tops.iter().copied().chain(std::iter::once(row_tops.last().copied().unwrap_or(0.0) + 34.0)) {
            painter.hline(egui::Rangef::new(span.0, span.1), y, egui::Stroke::new(1.0, TOKENS.grid_line));
        }
    }
    if let Some((key, field, value)) = commit {
        app.run("record.validate", json!({"table": table.name, "key": key.to_json(), "field": field, "value": value}));
    }
    if let Some((key, act)) = row_act {
        state.selected = Some(key.clone());
        match act {
            RowAct::Action(a) => {
                app.selection = if state.marked.contains(&key) { state.marked.clone() } else { Vec::new() };
                app.run_action(&a, Some((table.name.as_str(), &key)));
                app.selection.clear();
            }
            RowAct::SelectMore => {
                state.select_more = !state.select_more;
                if state.select_more && !state.marked.contains(&key) {
                    state.marked.push(key.clone());
                }
                if !state.select_more {
                    state.marked.clear();
                }
            }
            RowAct::Toggle => {
                if let Some(i) = state.marked.iter().position(|k| *k == key) {
                    state.marked.remove(i);
                } else {
                    state.marked.push(key);
                }
            }
            RowAct::View => open = Some((key, false)),
            RowAct::Edit => open = Some((key, true)),
            RowAct::Delete => {
                app.dialog = Some(Dialog::Confirm {
                    text: format!("Delete {} {}?", table.caption, key.display()),
                    command: "record.delete".into(),
                    params: json!({"table": table.name, "key": key.to_json()}),
                });
            }
        }
    }
    if let Some((k, edit)) = open {
        state.selected = Some(k.clone());
        let target = app
            .session
            .record(&table.name, &k)
            .and_then(|r| app.session.card_page_for(&table.name, &r))
            .or_else(|| page.card_page.clone());
        if let Some(t) = target {
            let list = state.clone();
            app.open_card_from_list(&t, k, &list);
            if edit && let Some(View::Card(c)) = app.stack.last_mut() {
                c.editing = true;
            }
        }
    }
}

/// One header cell: the caption (wrapping up to three lines, at the bottom), BC's sort arrow, a
/// grey background and ⌄ on hover, and a menu on click.
fn header_cell(ui: &mut Ui, f: &erpcraft_engine::meta::FieldDef, caption: &str, key_col: &str, state: &mut ListState, row: Option<&Record>) {
    let rect = ui.max_rect();
    let id = ui.scope_id().with(("header", &f.name));
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, 0.0, TOKENS.header_hover);
    }
    let arrow = match &state.sort {
        Some((s, desc)) if *s == f.name => if *desc { " ↓" } else { " ↑" },
        None if f.name == key_col => " ↑",
        _ => "",
    };
    let caption = caption.to_string();
    let text = format!("{caption}{arrow}{}", if hovered { "  ⌄" } else { "" });
    let layout = if f.ty.is_numeric() { Layout::bottom_up(Align::Max) } else { Layout::bottom_up(Align::Min) };
    ui.with_layout(layout, |ui| {
        ui.add_space(9.0);
        ui.add(egui::Label::new(RichText::new(text).size(theme::SMALL).color(TOKENS.grid_header)).wrap());
    });
    let resp = ui.interact(rect, id, egui::Sense::click());
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let whats_this = id.with("whats_this");
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(whats_this, false));
    }
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(220.0)
        .layout(Layout::top_down_justified(Align::LEFT))
        .show(|ui| {
            menu_style(ui);
            if ui.ctx().data(|d| d.get_temp::<bool>(whats_this)).unwrap_or(false) {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_max_width(320.0);
                    ui.label(RichText::new(&caption).font(theme::bold(theme::BODY)).color(TOKENS.text));
                    ui.add_space(6.0);
                    let tip = if f.tooltip.is_empty() { "No description is available for this field." } else { f.tooltip.as_str() };
                    ui.label(RichText::new(tip).color(TOKENS.text));
                });
                return;
            }
            let filtered = state.filters.iter().any(|(n, _)| *n == f.name);
            if ui.button("↑   Ascending").clicked() {
                state.sort = Some((f.name.clone(), false));
                ui.close();
            }
            if ui.button("↓   Descending").clicked() {
                state.sort = Some((f.name.clone(), true));
                ui.close();
            }
            if ui.button("⏷   Filter...").clicked() {
                if !filtered {
                    state.filters.push((f.name.clone(), String::new()));
                }
                state.filter_pane = true;
                ui.close();
            }
            if ui.add_enabled(row.is_some(), egui::Button::new("⏷   Filter to this value")).clicked()
                && let Some(r) = row
            {
                let v = r.get(&f.name).to_string();
                let expr = if v.is_empty() { "''".to_string() } else { v };
                state.filters.retain(|(n, _)| *n != f.name);
                state.filters.push((f.name.clone(), expr));
                state.filter_pane = true;
                ui.close();
            }
            if ui.add_enabled(filtered, egui::Button::new("✕   Clear filter")).clicked() {
                state.filters.retain(|(n, _)| *n != f.name);
                ui.close();
            }
            if ui.button("?   What's this?").clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(whats_this, true));
            }
        });
}

/// BC's context menus: full-width, flat items that highlight on hover.
fn menu_style(ui: &mut Ui) {
    let s = ui.style_mut();
    s.spacing.button_padding = vec2(10.0, 7.0);
    s.spacing.item_spacing.y = 0.0;
    s.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    s.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    s.visuals.widgets.hovered.weak_bg_fill = TOKENS.header_hover;
    s.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    s.visuals.widgets.active.weak_bg_fill = TOKENS.border;
}

/// What a click on a cell's value asked for.
enum CellClick {
    Key,
    DrillDown,
}

/// A FlowField value as a link: teal, pointer on hover, underlined on hover.
fn drill_link(ui: &mut Ui, t: RichText, align_right: bool) -> bool {
    let draw = |ui: &mut Ui| {
        let r = ui.add(egui::Label::new(t).sense(egui::Sense::click()));
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        r.clicked()
    };
    if align_right {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.0);
            draw(ui)
        })
        .inner
    } else {
        draw(ui)
    }
}

/// One cell's value, and what a click on it asked for.
fn grid_value(ui: &mut Ui, f: &erpcraft_engine::meta::FieldDef, v: &erpcraft_engine::meta::Value, key_link: bool, selected: bool) -> Option<CellClick> {
    use erpcraft_engine::meta::FieldType;
    let link = if selected { TOKENS.link_selected } else { TOKENS.link };
    let text = widgets::display_field(v, f);
    let body = |t: &str| RichText::new(t).size(14.0);
    if key_link {
        let mut t = body(&text).color(link);
        if selected {
            t = t.underline();
        }
        let r = ui.add(egui::Label::new(t).sense(egui::Sense::click()));
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        return r.clicked().then_some(CellClick::Key);
    }
    if f.is_flow() && f.drilldown.is_some() {
        return drill_link(ui, body(&text).color(link), f.ty.is_numeric()).then_some(CellClick::DrillDown);
    }
    match &f.ty {
        // A stored Boolean is a read-only check box; a calculated one reads Yes / No as a link.
        FieldType::Boolean if !f.is_flow() => {
            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect(rect, 1.0, egui::Color32::from_rgb(0xEB, 0xED, 0xEF), egui::Stroke::new(1.0, TOKENS.grid_line), egui::StrokeKind::Inside);
            if v.as_bool() {
                let p = |x: f32, y: f32| egui::pos2(rect.left() + x * rect.width(), rect.top() + y * rect.height());
                ui.painter().line(vec![p(0.22, 0.52), p(0.42, 0.72), p(0.78, 0.3)], egui::Stroke::new(1.5, TOKENS.text_dim));
            }
        }
        _ if f.ty.is_numeric() => {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                let t = body(&text);
                ui.label(if f.is_flow() { t.color(link) } else { t.color(TOKENS.grid_text) });
            });
        }
        _ if f.is_flow() => {
            ui.label(body(&text).color(link));
        }
        _ => {
            let mut t = body(&text).color(TOKENS.grid_text);
            if selected && f.relation.is_some() && !text.is_empty() {
                t = t.underline();
            }
            ui.label(t);
        }
    }
    None
}

/// The ⋮ of a row (shown on hover and on the current row): the page's actions by category, then
/// View, Edit and Delete.
fn row_menu(ui: &mut Ui, page: &PageDef, show: bool, several: bool) -> Option<RowAct> {
    let resp = ui.add_visible(show, egui::Button::new(RichText::new("⋮").size(16.0).color(TOKENS.grid_text)).frame(false));
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(240.0)
        .layout(Layout::top_down_justified(Align::LEFT))
        .show(|ui| {
            menu_style(ui);
            for tab in crate::card::action_tab_list(page).into_iter().filter(|t| t.caption != "More options") {
                ui.label(RichText::new(&tab.caption).font(theme::bold(theme::SMALL)).color(TOKENS.text));
                for a in tab.actions {
                    if ui.button(format!("{}   {}", widgets::glyph(&a.icon), a.caption.replace('&', ""))).clicked() {
                        out = Some(RowAct::Action(a.clone()));
                        ui.close();
                    }
                }
            }
            ui.separator();
            if ui.button(if several { "☑   Select More (on)" } else { "☑   Select More" }).clicked() {
                out = Some(RowAct::SelectMore);
                ui.close();
            }
            if page.card_page.is_some() {
                if ui.button("👁   View").clicked() {
                    out = Some(RowAct::View);
                    ui.close();
                }
                if ui.button("✎   Edit").clicked() {
                    out = Some(RowAct::Edit);
                    ui.close();
                }
            }
            if (page.editable || page.card_page.is_some()) && ui.button("🗑   Delete").clicked() {
                out = Some(RowAct::Delete);
                ui.close();
            }
        });
    out
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
            for f in table.fields.iter().filter(|f| f.ty != erpcraft_engine::meta::FieldType::Blob) {
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
