//! Small BC-style building blocks: action-bar buttons, links, field editors, FactBoxes.

use std::collections::HashMap;

use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};
use erpcraft_engine::meta::{FieldDef, FieldType, Value};
use serde_json::{Value as Json, json};

use crate::theme::{self, TOKENS};

/// A flat action-bar button: glyph + caption in the accent colour.
pub fn action_button(ui: &mut Ui, glyph: &str, caption: &str) -> egui::Response {
    let text = match (glyph.is_empty(), caption.is_empty()) {
        (true, _) => caption.to_string(),
        (false, true) => glyph.to_string(),
        (false, false) => format!("{glyph}  {caption}"),
    };
    ui.add(egui::Button::new(RichText::new(text).color(TOKENS.text).size(theme::BODY)).frame(false).min_size(vec2(0.0, 30.0)))
}

/// Glyph for an action icon name (Unicode symbols available in Segoe UI Symbol / egui's fonts).
pub fn glyph(icon: &str) -> &'static str {
    match icon {
        "new" => "+",
        "edit" => "✎",
        "delete" => "🗑",
        "post" => "⏵",
        "release" => "🔓",
        "reopen" => "↺",
        "stats" => "📊",
        "ledger" => "☰",
        "order" | "invoice" | "quote" | "credit" => "📄",
        "payment" => "💳",
        "customer" | "vendor" => "👤",
        "search" => "🔍",
        "print" => "🖶",
        "filter" => "⏷",
        _ => "",
    }
}

/// Teal, underlined-on-hover text that acts as a link.
pub fn link(ui: &mut Ui, text: &str) -> egui::Response {
    let r = ui.add(egui::Label::new(RichText::new(text).color(TOKENS.accent)).sense(Sense::click()));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

pub fn strong_link(ui: &mut Ui, text: &str) -> egui::Response {
    let r = ui.add(egui::Label::new(RichText::new(text).color(TOKENS.accent).strong()).sense(Sense::click()));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// BC's blue notification bar with a dismiss ×.
pub fn notice_bar(ui: &mut Ui, text: &str, on_close: impl FnOnce()) {
    egui::Frame::new().fill(TOKENS.notice).inner_margin(egui::Margin::symmetric(16, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ⓘ").color(TOKENS.accent));
            ui.label(text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("✕").frame(false)).clicked() {
                    on_close();
                }
            });
        });
    });
}

/// White rounded surface (BC places grids, FactBoxes and cards on these).
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(TOKENS.page)
        .corner_radius(6)
        .inner_margin(egui::Margin::same(12))
        .shadow(egui::epaint::Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(22) })
}

/// Makes buttons and menu buttons in `ui` borderless (BC's command bars and navigation menus).
pub fn flat(ui: &mut Ui) {
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.weak_bg_fill = TOKENS.row_hover;
    w.open.bg_stroke = Stroke::NONE;
    w.open.weak_bg_fill = TOKENS.divider;
    w.active.bg_stroke = Stroke::NONE;
}

/// A short vertical separator in action bars.
pub fn vsep(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TOKENS.border);
    ui.add_space(4.0);
}

/// A thin horizontal divider.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TOKENS.divider);
}

/// Display text of a value: amounts with thousands separators, dates dd/mm/yyyy.
pub fn display(v: &Value, ty: &FieldType) -> String {
    match (v, ty) {
        (Value::Bool(_), _) => v.display(ty),
        (_, FieldType::Integer) => v.to_string(),
        _ => v.display(ty),
    }
}

/// Right-aligned for numbers.
pub fn is_numeric(ty: &FieldType) -> bool {
    ty.is_numeric()
}

/// Lookup values for a relation field: (code, description).
pub type Lookup = Vec<(String, String)>;

/// Edits one field. Returns the new value (as JSON) when the user commits a change: Enter / focus
/// out for text, selection for options and lookups, click for booleans.
pub fn field_editor(
    bufs: &mut HashMap<String, String>,
    ui: &mut Ui,
    id: &str,
    f: &FieldDef,
    v: &Value,
    editable: bool,
    lookup: Option<&Lookup>,
    width: f32,
) -> Option<Json> {
    // Narrow grid cells can report no room at all; egui panics on negative widths.
    let width = if width.is_finite() { width.max(24.0) } else { 120.0 };
    let shown = display(v, &f.ty);
    if !editable || f.is_flow() {
        let text = RichText::new(if shown.is_empty() && f.ty.is_numeric() { "0.00".to_string() } else { shown });
        let text = if f.is_flow() { text.color(TOKENS.accent) } else { text.color(TOKENS.text) };
        let layout = if is_numeric(&f.ty) { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
        ui.allocate_ui_with_layout(vec2(width, 24.0), layout, |ui| {
            ui.add_space(8.0);
            ui.add(egui::Label::new(text).truncate());
        });
        return None;
    }
    match &f.ty {
        FieldType::Option(opts) => {
            let mut sel = v.as_str().to_string();
            let before = sel.clone();
            egui::ComboBox::from_id_salt(id).selected_text(if sel.is_empty() { " " } else { sel.as_str() }).width(width).show_ui(ui, |ui| {
                for o in opts {
                    ui.selectable_value(&mut sel, o.clone(), if o.is_empty() { " " } else { o.as_str() });
                }
            });
            (sel != before).then(|| json!(sel))
        }
        FieldType::Boolean => {
            let mut b = v.as_bool();
            let r = toggle(ui, &mut b);
            r.changed().then(|| json!(b))
        }
        _ => {
            let mut out = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let is_date = f.ty == FieldType::Date;
                let lookup_w = if lookup.is_some() || is_date { 24.0 } else { 0.0 };
                let buf = bufs.entry(id.to_string()).or_insert_with(|| shown.clone());
                let mut te = egui::TextEdit::singleline(buf).desired_width((width - lookup_w).max(16.0)).id_salt(id);
                if is_numeric(&f.ty) {
                    te = te.align(egui::Align2::RIGHT_CENTER);
                }
                let r = ui.add(te);
                let changed = *buf != shown;
                if r.lost_focus() && changed {
                    out = Some(json!(buf.clone()));
                    bufs.remove(id);
                } else if !r.has_focus() && !changed {
                    bufs.remove(id);
                }
                if is_date {
                    ui.menu_button("📅", |ui| {
                        if let Some(d) = calendar(ui, id, v.as_date()) {
                            out = Some(json!(d.format("%Y-%m-%d").to_string()));
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Choose a date");
                    if out.is_some() {
                        bufs.remove(id);
                    }
                }
                if let Some(values) = lookup {
                    ui.menu_button("▾", |ui| {
                        ui.set_min_width(260.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            if ui.selectable_label(false, RichText::new("(blank)").italics()).clicked() {
                                out = Some(json!(""));
                                ui.close();
                            }
                            for (code, desc) in values {
                                let label = if desc.is_empty() { code.clone() } else { format!("{code}   {desc}") };
                                if ui.selectable_label(code == v.as_str(), label).clicked() {
                                    out = Some(json!(code));
                                    ui.close();
                                }
                            }
                        });
                    });
                    if out.is_some() {
                        bufs.remove(id);
                    }
                }
            });
            out
        }
    }
}

/// A field of an editable card in view mode: the value in a read-only input box (CoreERP shows
/// `Input readOnly`); clicking it switches the card to edit mode.
pub fn view_value(ui: &mut Ui, f: &FieldDef, v: &Value, width: f32) -> egui::Response {
    let width = if width.is_finite() { width.max(24.0) } else { 120.0 };
    let text = display(v, &f.ty);
    let inner = egui::Frame::new()
        .fill(TOKENS.table_header)
        .stroke(Stroke::new(1.0, TOKENS.divider))
        .corner_radius(2)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width(width - 16.0);
            let layout = if is_numeric(&f.ty) { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
            ui.with_layout(layout, |ui| ui.add(egui::Label::new(RichText::new(text).color(TOKENS.text)).truncate()));
        });
    inner.response.interact(Sense::click())
}

/// BC-style toggle switch for Boolean fields.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let size = vec2(36.0, 18.0);
    let (rect, mut r) = ui.allocate_exact_size(size, Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(r.id, *on);
    let radius = rect.height() / 2.0;
    let fill = if *on { TOKENS.accent } else { TOKENS.page };
    let stroke = if *on { Stroke::new(1.0, TOKENS.accent) } else { Stroke::new(1.0, TOKENS.input_line) };
    ui.painter().rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if *on { Color32::WHITE } else { TOKENS.input_line };
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), radius - 4.0, knob);
    r
}

/// A FactBox: bold caption and label / value rows (amounts in teal, as BC drill-downs).
pub fn factbox(ui: &mut Ui, caption: &str, rows: &[(String, String, bool)]) {
    ui.label(RichText::new(caption).font(theme::bold(15.0)).color(TOKENS.text));
    ui.add_space(6.0);
    egui::Grid::new(("factbox", caption)).num_columns(2).spacing(vec2(12.0, 7.0)).min_col_width(110.0).show(ui, |ui| {
        for (label, value, numeric) in rows {
            ui.label(RichText::new(label).color(TOKENS.text_dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let t = RichText::new(value);
                ui.label(if *numeric { t.color(TOKENS.accent) } else { t.color(TOKENS.text) });
            });
            ui.end_row();
        }
    });
    ui.add_space(14.0);
}

/// Parses BC-style shortcut text ("F9", "Ctrl+F7") and reports whether it was pressed this frame.
pub fn shortcut_pressed(ctx: &egui::Context, shortcut: &str) -> bool {
    let mut mods = egui::Modifiers::NONE;
    let mut key = None;
    for part in shortcut.split('+') {
        match part.trim() {
            "Ctrl" => mods = mods | egui::Modifiers::COMMAND,
            "Shift" => mods = mods | egui::Modifiers::SHIFT,
            "Alt" => mods = mods | egui::Modifiers::ALT,
            k => key = egui::Key::from_name(k),
        }
    }
    match key {
        Some(k) => ctx.input_mut(|i| i.consume_key(mods, k)),
        None => false,
    }
}

/// A month calendar (BC's date picker): ‹ month year ›, weeks from Monday, today underlined.
/// Returns the day the user picked.
pub fn calendar(ui: &mut Ui, id: &str, current: Option<chrono::NaiveDate>) -> Option<chrono::NaiveDate> {
    use chrono::{Datelike, Months, NaiveDate};
    let today = chrono::Local::now().date_naive();
    let mid = egui::Id::unique(("calendar", id));
    let first = |d: NaiveDate| d.with_day(1).unwrap_or(d);
    let mut month: NaiveDate = ui.ctx().data(|d| d.get_temp::<NaiveDate>(mid)).unwrap_or_else(|| first(current.unwrap_or(today)));
    let mut picked = None;
    ui.set_min_width(230.0);
    ui.horizontal(|ui| {
        if ui.button("‹").clicked() {
            month = month.checked_sub_months(Months::new(1)).unwrap_or(month);
        }
        ui.with_layout(Layout::left_to_right(Align::Center).with_main_justify(true), |ui| {
            ui.label(RichText::new(month.format("%B %Y").to_string()).strong());
        });
        if ui.button("›").clicked() {
            month = month.checked_add_months(Months::new(1)).unwrap_or(month);
        }
    });
    egui::Grid::new(("calendar_grid", id)).num_columns(7).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for d in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
            ui.label(RichText::new(d).size(theme::SMALL).color(TOKENS.text_dim));
        }
        ui.end_row();
        let offset = month.weekday().num_days_from_monday();
        for _ in 0..offset {
            ui.label("");
        }
        let mut day = month;
        let mut col = offset;
        while day.month() == month.month() {
            let selected = current == Some(day);
            let mut text = RichText::new(day.day().to_string());
            if day == today {
                text = text.underline();
            }
            if selected {
                text = text.color(Color32::WHITE);
            }
            let b = egui::Button::new(text).min_size(vec2(28.0, 24.0)).fill(if selected { TOKENS.accent } else { Color32::TRANSPARENT });
            if ui.add(b).clicked() {
                picked = Some(day);
            }
            col += 1;
            if col == 7 {
                ui.end_row();
                col = 0;
            }
            match day.succ_opt() {
                Some(n) => day = n,
                None => break,
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.link("Today").clicked() {
            picked = Some(today);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(mid, month));
    picked
}

/// A card caption followed by BC's dotted leader up to the value column. Clicking it opens BC's
/// callout: the caption in bold over the field's description.
pub fn caption_leader(ui: &mut Ui, text: &str, tooltip: &str, width: f32) -> egui::Response {
    let sense = if tooltip.is_empty() { Sense::hover() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 22.0), sense);
    let font = egui::FontId::proportional(theme::BODY);
    let galley = ui.painter().layout(text.to_string(), font, TOKENS.text_dim, width - 12.0);
    let top = rect.center().y - galley.size().y / 2.0;
    let text_w = galley.size().x;
    let baseline = top + galley.size().y - 4.0;
    ui.painter().galley(egui::pos2(rect.left(), top), galley, TOKENS.text_dim);
    let mut x = rect.left() + text_w + 6.0;
    while x < rect.right() - 2.0 {
        ui.painter().circle_filled(egui::pos2(x, baseline), 0.9, TOKENS.input_line);
        x += 6.0;
    }
    if !tooltip.is_empty() {
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(360.0)
            .gap(6.0)
            .frame(
                egui::Frame::new()
                    .fill(TOKENS.page)
                    .stroke(Stroke::new(1.0, TOKENS.divider))
                    .corner_radius(4)
                    .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(40) })
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(|ui| {
                ui.set_max_width(328.0);
                ui.label(RichText::new(text).font(theme::bold(theme::BODY)).color(TOKENS.text));
                ui.add_space(6.0);
                ui.label(RichText::new(tooltip).color(TOKENS.text));
            });
    }
    resp
}

/// BC shows "(LCY)" in captions as the local currency symbol: "Balance (LCY)" → "Balance (Rp)".
pub fn lcy_caption(caption: &str, symbol: &str) -> String {
    if symbol.is_empty() { caption.to_string() } else { caption.replace("(LCY)", &format!("({symbol})")) }
}
