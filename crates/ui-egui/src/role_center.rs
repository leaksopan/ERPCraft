//! The role center body: headline, activity cues, and a few list parts.

use chrono::Timelike;
use egui::{Align2, RichText, Sense, Ui, vec2};
use erpcraft_engine::meta::format_decimal;
use rust_decimal::Decimal;

use crate::theme::{self, TOKENS};
use crate::widgets;
use crate::ErpApp;

pub fn show(app: &mut ErpApp, ui: &mut Ui) {
    if app.session.is_empty() {
        empty_company(app, ui);
        return;
    }
    let Some(rc) = app.session.meta().role_center(&app.role_center) else { return };
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 20)).show(ui, |ui| {
            // Headline.
            let hour = chrono::Local::now().hour();
            let greet = match hour {
                0..=11 => "Good morning",
                12..=17 => "Good afternoon",
                _ => "Good evening",
            };
            ui.label(RichText::new("HEADLINE").size(theme::SMALL).color(TOKENS.text_dim));
            ui.label(RichText::new(format!("{greet}, {}!", app.session.user())).size(26.0).color(TOKENS.text));
            ui.label(
                RichText::new(format!("Here is the business at a glance. Work date: {}", app.session.work_date().format("%d %B %Y")))
                    .color(TOKENS.text_dim),
            );
            ui.add_space(22.0);
            let mut open = None;
            // "Activities": amount KPIs, BC style (caption, big number, coloured bar, "See more").
            section_header(ui, "Activities");
            ui.horizontal_wrapped(|ui| {
                for c in rc.cue_groups.iter().flat_map(|g| g.cues.iter()).filter(|c| c.measure != "count") {
                    let v = app.session.cue_value(c);
                    if kpi_cue(ui, &c.caption, v, c.measure.starts_with("overdue")) {
                        open = Some(c.page.clone());
                    }
                    ui.add_space(36.0);
                }
            });
            // Count cues: teal tiles, one row per group.
            for g in &rc.cue_groups {
                let counts: Vec<_> = g.cues.iter().filter(|c| c.measure == "count").collect();
                if counts.is_empty() {
                    continue;
                }
                ui.add_space(18.0);
                widgets::divider(ui);
                ui.add_space(10.0);
                ui.label(RichText::new(&g.caption).font(theme::bold(theme::BODY)).color(TOKENS.text));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for c in counts {
                        let v = app.session.cue_value(c);
                        if cue_tile(ui, &c.caption, v).clicked() {
                            open = Some(c.page.clone());
                        }
                    }
                });
            }
            if let Some(p) = open {
                app.open_page(&p, Vec::new(), None);
                return;
            }
            ui.add_space(26.0);
            if ui.available_width() > 300.0 { ui.columns(2, |cols| {
                mini_list(app, &mut cols[0], "Open Sales Orders", "sales-order-list", &["No.", "Sell-to Customer Name", "Amount Including VAT"]);
                mini_list(app, &mut cols[1], "Top Customers by Balance", "customer-list", &["No.", "Name", "Balance (LCY)"]);
            }); }
        });
    });
}

fn section_header(ui: &mut Ui, caption: &str) {
    ui.label(RichText::new(format!("{caption} ⌄")).size(20.0).color(TOKENS.text));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TOKENS.text_dim);
    ui.add_space(10.0);
}

/// An amount KPI: caption, large light number, a coloured bar (red when something is overdue),
/// and "› See more". Returns true when clicked.
fn kpi_cue(ui: &mut Ui, caption: &str, v: Decimal, alert: bool) -> bool {
    let mut clicked = false;
    ui.vertical(|ui| {
        ui.set_width(200.0);
        ui.label(RichText::new(caption).color(TOKENS.text_dim));
        let r = ui.add(egui::Label::new(RichText::new(compact_amount(v)).size(46.0).color(TOKENS.text)).sense(Sense::click()));
        clicked |= r.clicked();
        let color = if alert && !v.is_zero() { egui::Color32::from_rgb(0xE8, 0x6B, 0x6B) } else { egui::Color32::from_rgb(0xA4, 0xA1, 0x1C) };
        let (bar, _) = ui.allocate_exact_size(vec2(100.0, 4.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, color);
        ui.add_space(6.0);
        clicked |= widgets::link(ui, "›  See more").clicked();
    });
    clicked
}

/// A count cue: teal tile, caption on top, big number, thin line, chevron.
fn cue_tile(ui: &mut Ui, caption: &str, v: Decimal) -> egui::Response {
    let size = vec2(120.0, 136.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let fill = if r.hovered() { TOKENS.accent_hover } else { TOKENS.cue };
    ui.painter().rect_filled(rect, 0.0, fill);
    let galley = ui.painter().layout(caption.to_string(), egui::FontId::proportional(theme::BODY), TOKENS.cue_text, rect.width() - 18.0);
    ui.painter().galley(rect.left_top() + vec2(9.0, 8.0), galley, TOKENS.cue_text);
    ui.painter().text(rect.left_top() + vec2(9.0, 46.0), Align2::LEFT_TOP, v.round().to_string(), egui::FontId::proportional(40.0), TOKENS.cue_text);
    let line_y = rect.bottom() - 34.0;
    ui.painter().line_segment([egui::pos2(rect.left() + 9.0, line_y), egui::pos2(rect.right() - 9.0, line_y)], egui::Stroke::new(1.0, egui::Color32::from_white_alpha(150)));
    ui.painter().text(egui::pos2(rect.left() + 10.0, rect.bottom() - 16.0), Align2::LEFT_CENTER, "›", egui::FontId::proportional(16.0), TOKENS.cue_text);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// 12,345,678 → "12.3M" (BC shows large cue amounts abbreviated).
fn compact_amount(v: Decimal) -> String {
    let f = v.to_string().parse::<f64>().unwrap_or(0.0);
    let a = f.abs();
    if a >= 1e9 {
        format!("{:.1}B", f / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", f / 1e6)
    } else if a >= 1e3 {
        format!("{:.0}K", f / 1e3)
    } else {
        format!("{f:.0}")
    }
}

fn mini_list(app: &mut ErpApp, ui: &mut Ui, caption: &str, page_id: &str, cols: &[&str]) {
    let Some(page) = app.session.page(page_id) else { return };
    let Some(table) = page.source_table.as_deref().and_then(|t| app.session.table(t)) else { return };
    let mut rows = app.session.page_records(page, &[], "");
    if page_id == "customer-list" {
        rows.sort_by(|a, b| b.get("Balance (LCY)").as_dec().cmp(&a.get("Balance (LCY)").as_dec()));
    }
    rows.truncate(6);
    let mut open = None;
    egui::Frame::new().fill(TOKENS.page).stroke(egui::Stroke::new(1.0, TOKENS.border)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width().max(0.0));
        ui.horizontal(|ui| {
            if widgets::strong_link(ui, caption).clicked() {
                open = Some((page_id.to_string(), None));
            }
        });
        ui.add_space(6.0);
        egui::Grid::new(("mini", page_id)).num_columns(cols.len()).striped(true).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            for c in cols {
                ui.label(RichText::new(*c).size(theme::SMALL).color(TOKENS.text_dim));
            }
            ui.end_row();
            for r in &rows {
                for (i, c) in cols.iter().enumerate() {
                    let ty = table.field(c).map(|f| f.ty.clone());
                    let v = r.get(c);
                    let text = match (&ty, v) {
                        (_, erpcraft_engine::meta::Value::Dec(d)) => format_decimal(*d, 2),
                        (Some(t), v) => v.display(t),
                        (None, v) => v.to_string(),
                    };
                    if i == 0 {
                        if widgets::link(ui, &text).clicked() {
                            open = Some((page.card_page.clone().unwrap_or_default(), Some(r.key(table))));
                        }
                    } else if matches!(v, erpcraft_engine::meta::Value::Dec(_)) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(text));
                    } else {
                        ui.label(text);
                    }
                }
                ui.end_row();
            }
            if rows.is_empty() {
                ui.label(RichText::new("(There is nothing to show in this view)").italics().color(TOKENS.text_dim));
                ui.end_row();
            }
        });
    });
    if let Some((p, key)) = open {
        app.open_page(&p, Vec::new(), key);
    }
}

fn empty_company(app: &mut ErpApp, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.label(RichText::new("Welcome to ERPCraft").size(28.0));
        ui.add_space(8.0);
        ui.label(RichText::new("This company has no data yet.").color(TOKENS.text_dim));
        ui.add_space(20.0);
        if ui.add(egui::Button::new(RichText::new("  Create demo company (Auto Craft)  ").color(egui::Color32::WHITE)).fill(TOKENS.accent)).clicked() {
            app.run("company.init", serde_json::json!({"demo": true}));
        }
        ui.add_space(8.0);
        if ui.button("  Start with an empty company (base setup only)  ").clicked() {
            app.run("company.init", serde_json::json!({"demo": false}));
        }
    });
}
