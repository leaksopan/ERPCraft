//! Card and document pages: title, action bar, FastTabs (two-column fields), the lines part with
//! totals, and the FactBox pane.

use std::collections::HashMap;

use egui::{Align, Layout, RichText, Sense, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use erpcraft_engine::meta::{ActionDef, FieldDef, GroupDef, Key, PageDef, PageKind, PartDef, Record, TableDef, Value, format_decimal};
use serde_json::{Value as Json, json};

use crate::nav::{CardState, View};
use crate::theme::{self, TOKENS};
use crate::widgets::{self, Lookup};
use crate::{Dialog, ErpApp};

pub fn show(app: &mut ErpApp, ui: &mut Ui, mut state: CardState) {
    let depth = app.stack.len();
    let Some(page) = app.session.page(&state.page) else {
        app.back();
        return;
    };
    let Some(table) = app.session.table(&state.table) else { return };
    let Some(rec) = app.session.record(&table.name, &state.key) else {
        // Deleted or posted meanwhile.
        app.back();
        return;
    };
    let title = record_title(table, &rec);
    let ctx_key = state.key.clone();

    let is_setup = table.primary_key.first().map(String::as_str) == Some("Primary Key");
    let mut run: Option<ActionDef> = None;
    // BC opens cards as a sheet over the dimmed list / role center.
    egui::Frame::new().fill(TOKENS.scrim).inner_margin(egui::Margin { left: 28, right: 28, top: 14, bottom: 0 }).show(ui, |ui| {
        egui::Frame::new()
            .fill(TOKENS.page)
            .corner_radius(egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 })
            .inner_margin(egui::Margin { left: 20, right: 20, top: 10, bottom: 0 })
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size().max(egui::Vec2::ZERO));
                // Row 1: back, page caption, record commands, saved state.
                ui.horizontal(|ui| {
                    let back = ui.add(egui::Button::new(RichText::new("←").size(16.0).color(TOKENS.text)).corner_radius(14).min_size(vec2(28.0, 28.0)));
                    if back.on_hover_text("Back (Esc)").clicked() {
                        app.back();
                    }
                    ui.label(RichText::new(&page.caption).size(theme::SMALL).color(TOKENS.text_dim));
                    ui.add_space((ui.available_width() / 2.0 - 160.0).max(8.0));
                    if page.editable && !is_setup {
                        if page.kind == PageKind::Card && widgets::action_button(ui, "+", "").on_hover_text("New").clicked() {
                            app.new_record_on(&page.id);
                        }
                        if widgets::action_button(ui, "🗑", "").on_hover_text("Delete").clicked() {
                            app.dialog = Some(Dialog::Confirm {
                                text: format!("Delete {} {}?", table.caption, state.key.display()),
                                command: "record.delete".into(),
                                params: json!({"table": table.name, "key": state.key.to_json()}),
                            });
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !page.factboxes.is_empty()
                            && ui.add(egui::Button::new(RichText::new("ⓘ").size(16.0).color(TOKENS.accent)).frame(false)).on_hover_text("FactBox pane").clicked()
                        {
                            app.show_factbox = !app.show_factbox;
                        }
                        ui.label(RichText::new(if page.editable { "✓ Saved" } else { "Read-only" }).size(theme::SMALL).color(TOKENS.text_dim));
                    });
                });
                // Row 2: the record title.
                ui.horizontal(|ui| {
                    ui.add_space(36.0);
                    ui.label(RichText::new(&title).size(26.0).color(TOKENS.text));
                    if let Some(status) = table.field("Status").map(|_| rec.str("Status").to_string()) {
                        let fill = if status == "Open" { TOKENS.accent_soft } else { egui::Color32::from_rgb(0xFF, 0xF4, 0xCE) };
                        egui::Frame::new().fill(fill).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 2)).show(ui, |ui| {
                            ui.label(RichText::new(status).size(theme::SMALL));
                        });
                    }
                });
                ui.add_space(4.0);
                // Rows 3-4: action categories as tabs, then the actions of the chosen one.
                ui.horizontal(|ui| {
                    ui.add_space(36.0);
                    action_tabs(ui, page, &mut run);
                });
                ui.add_space(6.0);
                let with_fb = !page.factboxes.is_empty() && app.show_factbox && ui.available_width() > 900.0;
                if with_fb {
                    egui::Panel::right(("card_factbox", &page.id))
                        .resizable(false)
                        .exact_size(310.0)
                        .frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 0, bottom: 12 }))
                        .show(ui, |ui| factbox_pane(app, ui, page, Some(&rec)));
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt(("card_body", &page.id)).auto_shrink([false; 2]).show(ui, |ui| {
                        let mut groups = page.groups.iter();
                        if let Some(first) = groups.next() {
                            fasttab(app, ui, page, table, &rec, first);
                        }
                        for part in &page.parts {
                            lines_part(app, ui, page, &rec, part, &mut state);
                        }
                        for g in groups {
                            fasttab(app, ui, page, table, &rec, g);
                        }
                        ui.add_space(24.0);
                    });
                });
            });
    });
    if let Some(a) = run {
        app.run_action(&a, Some((&table.name, &ctx_key)));
    }

    for a in &page.actions {
        if let Some(s) = &a.shortcut
            && widgets::shortcut_pressed(ui.ctx(), s)
        {
            app.run_action(a, Some((&table.name, &ctx_key)));
        }
    }
    if app.stack.len() == depth
        && let Some(View::Card(s)) = app.stack.last_mut()
        && s.key == state.key
    {
        *s = state;
    }
}

/// BC's action bar on cards: category tabs (Home, Process, …) with the chosen category's actions
/// on the row below.
fn action_tabs(ui: &mut Ui, page: &PageDef, run: &mut Option<ActionDef>) {
    use erpcraft_engine::meta::ActionArea;
    let areas: Vec<ActionArea> = [ActionArea::Home, ActionArea::Process, ActionArea::New, ActionArea::Navigate, ActionArea::Report]
        .into_iter()
        .filter(|a| page.actions.iter().any(|x| x.area == *a))
        .collect();
    if areas.is_empty() {
        return;
    }
    let id = egui::Id::unique(("action_tab", &page.id));
    let mut current = ui.ctx().data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(areas.len() - 1);
    ui.vertical(|ui| {
        widgets::flat(ui);
        ui.horizontal(|ui| {
            for (i, a) in areas.iter().enumerate() {
                let sel = i == current;
                let t = RichText::new(a.caption()).color(if sel { TOKENS.accent } else { TOKENS.text });
                let r = ui.add(egui::Button::new(t).frame(false));
                if sel {
                    let rect = r.rect;
                    ui.painter().line_segment(
                        [rect.left_bottom() + vec2(6.0, 0.0), rect.right_bottom() - vec2(6.0, 0.0)],
                        egui::Stroke::new(2.0, TOKENS.accent),
                    );
                }
                if r.clicked() {
                    current = i;
                }
            }
        });
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, TOKENS.divider);
        ui.horizontal(|ui| {
            if let Some(area) = areas.get(current) {
                for a in page.actions.iter().filter(|x| x.area == *area) {
                    let r = widgets::action_button(ui, widgets::glyph(&a.icon), &a.caption.replace('&', ""));
                    let tip = match (&a.shortcut, a.tooltip.is_empty()) {
                        (Some(s), true) => s.clone(),
                        (Some(s), false) => format!("{} ({s})", a.tooltip),
                        (None, _) => a.tooltip.clone(),
                    };
                    let r = if tip.is_empty() { r } else { r.on_hover_text(tip) };
                    if r.clicked() {
                        *run = Some(a.clone());
                    }
                }
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, current));
}

/// The FactBox pane: a white card with a "Details" tab header and the page's FactBoxes.
pub fn factbox_pane(app: &ErpApp, ui: &mut Ui, page: &PageDef, rec: Option<&Record>) {
    widgets::card_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size().max(egui::Vec2::ZERO));
        ui.label(RichText::new("ⓘ  Details").color(TOKENS.text));
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 2.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, TOKENS.accent);
        ui.add_space(10.0);
        egui::ScrollArea::vertical().id_salt(("factbox", &page.id)).show(ui, |ui| factboxes(app, ui, page, rec));
    });
}

/// `10000 · Bengkel Maju Jaya`, `S-ORD1001 · Prima Auto Service`.
pub fn record_title(t: &TableDef, r: &Record) -> String {
    let key = if t.primary_key.first().map(String::as_str) == Some("Document Type") {
        r.str("No.").to_string()
    } else if t.primary_key.first().map(String::as_str) == Some("Primary Key") {
        return t.caption.clone();
    } else {
        r.key(t).display()
    };
    match t.description_field.as_deref().map(|f| r.str(f)).filter(|s| !s.is_empty()) {
        Some(d) => format!("{key} · {d}"),
        None => key,
    }
}

/// Lookup values for the relation fields among `fields`.
pub fn lookups<'a>(app: &ErpApp, t: &TableDef, fields: impl Iterator<Item = &'a str>) -> HashMap<String, Lookup> {
    let mut out = HashMap::new();
    for name in fields {
        let Some(f) = t.field(name) else { continue };
        let Some(rel) = &f.relation else { continue };
        if let Some(values) = table_values(app, rel) {
            out.insert(f.name.clone(), values);
        }
    }
    out
}

/// (code, description) of the first 500 records of a one-key table.
pub fn table_values(app: &ErpApp, table: &str) -> Option<Lookup> {
    let rt = app.session.table(table)?;
    let desc = rt.description_field.clone().unwrap_or_default();
    Some(
        app.session
            .db()
            .rows(&rt.name)
            .filter(|r| rt.name != "G/L Account" || r.str("Account Type") == "Posting")
            .take(500)
            .map(|r| (r.key(rt).0.first().map(ToString::to_string).unwrap_or_default(), r.str(&desc).to_string()))
            .collect(),
    )
}

fn tab_id(page: &PageDef, g: &GroupDef) -> String {
    format!("{}|{}", page.id, g.caption)
}

fn fasttab(app: &mut ErpApp, ui: &mut Ui, page: &PageDef, table: &TableDef, rec: &Record, g: &GroupDef) {
    let id = tab_id(page, g);
    let collapsed = g.collapsed != app.toggled_tabs.contains(&id);
    ui.add_space(10.0);
    let header = ui.horizontal(|ui| {
        ui.label(RichText::new(&g.caption).font(theme::bold(theme::SUBTITLE)).color(TOKENS.text));
        ui.label(RichText::new(if collapsed { "›" } else { "▾" }).color(TOKENS.text_dim));
        if collapsed {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let promoted: Vec<String> = g
                    .promoted
                    .iter()
                    .rev()
                    .filter_map(|f| table.field(f).map(|fd| widgets::display(rec.get(&fd.name), &fd.ty)))
                    .filter(|s| !s.is_empty())
                    .collect();
                for p in promoted {
                    ui.label(RichText::new(p).color(TOKENS.text_dim));
                }
            });
        }
    });
    let click = ui.interact(header.response.rect, ui.scope_id().with(("fasttab", &id)), Sense::click());
    if click.clicked() {
        if !app.toggled_tabs.remove(&id) {
            app.toggled_tabs.insert(id.clone());
        }
    }
    if click.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    widgets::divider(ui);
    if collapsed {
        return;
    }
    ui.add_space(6.0);
    let fields: Vec<&FieldDef> = g.fields.iter().filter_map(|f| table.field(f)).collect();
    let lookups = lookups(app, table, fields.iter().map(|f| f.name.as_str()));
    // Two columns like BC; one when the sheet is narrow (egui's columns can't take negative widths).
    let ncols = if ui.available_width() >= 520.0 { 2 } else { 1 };
    let half = if ncols == 2 { fields.len().div_ceil(2) } else { fields.len() };
    let mut commit: Option<(String, Json)> = None;
    let key = rec.key(table);
    let editable_page = page.editable;
    ui.columns(ncols, |cols| {
        for (ci, col) in cols.iter_mut().enumerate() {
            let slice = if ci == 0 { fields.get(..half) } else { fields.get(half..) };
            let Some(slice) = slice else { continue };
            egui::Grid::new(("fields", &id, ci)).num_columns(2).spacing(vec2(16.0, 6.0)).min_col_width(140.0).show(col, |ui| {
                for f in slice {
                    let caption = ui.label(RichText::new(&f.caption).color(TOKENS.text_dim));
                    if !f.tooltip.is_empty() {
                        caption.on_hover_text(&f.tooltip);
                    }
                    let editable = editable_page && f.editable && !table.primary_key.contains(&f.name);
                    let wid = format!("card|{}|{}|{}", table.name, key.display(), f.name);
                    let width = (ui.available_width()).clamp(160.0, 320.0);
                    if let Some(v) = widgets::field_editor(&mut app.bufs, ui, &wid, f, rec.get(&f.name), editable, lookups.get(&f.name), width) {
                        commit = Some((f.name.clone(), v));
                    }
                    ui.end_row();
                }
            });
        }
    });
    if let Some((field, value)) = commit {
        app.run("record.validate", json!({"table": table.name, "key": key.to_json(), "field": field, "value": value}));
    }
}

fn lines_part(app: &mut ErpApp, ui: &mut Ui, host: &PageDef, header: &Record, part: &PartDef, state: &mut CardState) {
    let Some(page) = app.session.page(&part.page) else { return };
    let Some(lt) = page.source_table.as_deref().and_then(|t| app.session.table(t)) else { return };
    let filters: Vec<(String, String)> = part.link.iter().map(|(sub, host_f)| (sub.clone(), header.get(host_f).to_string())).collect();
    let lines = app.session.page_records(page, &filters, "");
    let editable = page.editable && host.editable;
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        widgets::flat(ui);
        ui.label(RichText::new(&part.caption).font(theme::bold(theme::SUBTITLE)).color(TOKENS.text));
        ui.add_space(12.0);
        if editable {
            if widgets::action_button(ui, "+", "New Line").clicked() {
                let mut values = serde_json::Map::new();
                values.insert("Type".into(), json!("Item"));
                let mut v = serde_json::Map::new();
                for (sub, host_f) in &part.link {
                    v.insert(sub.clone(), header.get(host_f).to_json());
                }
                // Key fields go in "values" too: record.new applies them before inserting.
                for (k, val) in v {
                    values.insert(k, val);
                }
                match app.session.run("record.new", &json!({"table": lt.name, "values": values})) {
                    Ok(r) => state.selected_line = Key::from_json(lt, r.get("key").unwrap_or(&Json::Null)).ok(),
                    Err(e) => app.error(e.message),
                }
            }
            if widgets::action_button(ui, "🗑", "Delete Line").clicked() {
                if let Some(k) = state.selected_line.clone() {
                    app.run("record.delete", json!({"table": lt.name, "key": k.to_json()}));
                    state.selected_line = None;
                } else {
                    app.error("Select a line to delete.");
                }
            }
        }
    });
    widgets::divider(ui);
    let cols: Vec<&FieldDef> = page.columns.iter().filter_map(|c| lt.field(c)).collect();
    let items = table_values(app, "Item").unwrap_or_default();
    let accounts = table_values(app, "G/L Account").unwrap_or_default();
    let mut commit: Option<(Key, String, Json)> = None;
    let height = (lines.len() as f32 * 32.0 + 34.0).clamp(80.0, 420.0);
    egui::ScrollArea::horizontal().id_salt(("lines_scroll", &part.page)).show(ui, |ui| {
        ui.set_height(height);
        let mut b = TableBuilder::new(ui)
            .id_salt(("lines", &part.page, header.str("No.")))
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, true])
            .cell_layout(Layout::left_to_right(Align::Center));
        for f in &cols {
            let w = match f.name.as_str() {
                "Description" => 220.0,
                "Type" => 100.0,
                "No." => 100.0,
                "Unit of Measure Code" => 70.0,
                n if n.starts_with("Qty") || n.starts_with("Quantity") => 80.0,
                "Line Discount %" => 70.0,
                _ => 110.0,
            };
            b = b.column(Column::initial(w).at_least(50.0).clip(true));
        }
        b.header(26.0, |mut h| {
            for f in &cols {
                h.col(|ui| {
                    let layout = if f.ty.is_numeric() { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
                    ui.with_layout(layout, |ui| ui.label(RichText::new(&f.caption).size(theme::SMALL).color(TOKENS.text_dim).strong()));
                });
            }
        })
        .body(|body| {
            body.rows(32.0, lines.len(), |mut row| {
                let Some(l) = lines.get(row.index()) else { return };
                let key = l.key(lt);
                row.set_selected(state.selected_line.as_ref() == Some(&key));
                for f in &cols {
                    row.col(|ui| {
                        let wid = format!("line|{}|{}|{}", lt.name, key.display(), f.name);
                        let can_edit = editable && f.editable && !lt.primary_key.contains(&f.name);
                        let lookup = if f.name == "No." {
                            match l.str("Type") {
                                "Item" => Some(&items),
                                "G/L Account" => Some(&accounts),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let w = ui.available_width() - 4.0;
                        if let Some(v) = widgets::field_editor(&mut app.bufs, ui, &wid, f, l.get(&f.name), can_edit, lookup, w) {
                            commit = Some((key.clone(), f.name.clone(), v));
                        }
                    });
                }
                if row.response().clicked() {
                    state.selected_line = Some(key.clone());
                }
            });
        });
    });
    if lines.is_empty() {
        ui.label(RichText::new("No lines yet. Use + New Line.").italics().color(TOKENS.text_dim));
    }
    if let Some((key, field, value)) = commit {
        state.selected_line = Some(key.clone());
        app.run("record.validate", json!({"table": lt.name, "key": key.to_json(), "field": field, "value": value}));
    }
    totals(ui, header, &lines);
}

/// The totals area under the lines (BC: Subtotal / Total Excl. VAT / Total VAT / Total Incl. VAT).
fn totals(ui: &mut Ui, header: &Record, lines: &[Record]) {
    let amount = header.get("Amount").as_dec();
    let incl = header.get("Amount Including VAT").as_dec();
    let subtotal: rust_decimal::Decimal = lines.iter().map(|l| l.get("Line Amount").as_dec()).sum();
    let disc: rust_decimal::Decimal = lines.iter().map(|l| l.get("Line Discount Amount").as_dec()).sum();
    ui.add_space(10.0);
    let value_box = |ui: &mut Ui, v: rust_decimal::Decimal, strong: bool| {
        egui::Frame::new().fill(TOKENS.divider).corner_radius(2).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_width(150.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let t = RichText::new(format_decimal(v, 2));
                ui.label(if strong { t.font(theme::bold(theme::BODY)) } else { t });
            });
        });
    };
    egui::Grid::new(("totals", header.str("No."))).num_columns(4).spacing(vec2(16.0, 6.0)).min_col_width(140.0).show(ui, |ui| {
        let rows: [(&str, rust_decimal::Decimal, &str, rust_decimal::Decimal, bool); 3] = [
            ("Subtotal Excl. VAT", subtotal, "Total Excl. VAT", amount, false),
            ("Line Discount Amount", disc, "Total VAT", incl - amount, false),
            ("", rust_decimal::Decimal::ZERO, "Total Incl. VAT", incl, true),
        ];
        for (l1, v1, l2, v2, strong) in rows {
            if l1.is_empty() {
                ui.label("");
                ui.label("");
            } else {
                ui.label(RichText::new(l1).color(TOKENS.text_dim));
                value_box(ui, v1, false);
            }
            ui.label(RichText::new(l2).color(if strong { TOKENS.text } else { TOKENS.text_dim }));
            value_box(ui, v2, strong);
            ui.end_row();
        }
    });
}

/// The FactBoxes of `page` for record `rec`.
pub fn factboxes(app: &ErpApp, ui: &mut Ui, page: &PageDef, rec: Option<&Record>) {
    let Some(table) = page.source_table.as_deref().and_then(|t| app.session.table(t)) else { return };
    for fb in &page.factboxes {
        let (target_table, target) = match &fb.via {
            None => (Some(table), rec.cloned()),
            Some(field) => {
                let rel = table.field(field).and_then(|f| f.relation.clone());
                let tt = rel.as_deref().and_then(|t| app.session.table(t));
                let r = match (tt, rec) {
                    (Some(tt), Some(r)) if !r.str(field).is_empty() => app.session.record(&tt.name, &Key(vec![Value::text(r.str(field))])),
                    _ => None,
                };
                (tt, r)
            }
        };
        let rows: Vec<(String, String, bool)> = match (target_table, &target) {
            (Some(t), Some(r)) => fb
                .fields
                .iter()
                .filter_map(|f| t.field(f))
                .map(|f| (f.caption.clone(), widgets::display(r.get(&f.name), &f.ty), f.ty.is_numeric()))
                .collect(),
            _ => vec![("".into(), "(no record selected)".into(), false)],
        };
        widgets::factbox(ui, &fb.caption, &rows);
    }
}
