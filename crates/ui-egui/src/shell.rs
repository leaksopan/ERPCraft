//! The window chrome: the dark app bar, and on the role center the navigation bar (company name,
//! embedded links, ☰ menu) and the action bar (creation actions, New / Payments / Reports menus).

use egui::{Align, Align2, Layout, RichText, Sense, Ui, vec2};
use serde_json::json;

use crate::theme::{self, TOKENS};
use crate::widgets;
use crate::{Dialog, ErpApp};

pub fn app_bar(app: &mut ErpApp, ui: &mut Ui) {
    egui::Panel::top("app_bar")
        .exact_size(44.0)
        .frame(egui::Frame::new().fill(TOKENS.app_bar).inner_margin(egui::Margin::symmetric(12, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let white = TOKENS.app_bar_text;
                ui.label(RichText::new("⣿").color(white).size(18.0));
                ui.add_space(6.0);
                let brand = ui.add(egui::Label::new(RichText::new("ERPCraft").font(theme::bold(15.0)).color(white)).sense(Sense::click()));
                if brand.clicked() {
                    app.home();
                }
                ui.label(RichText::new("Business Central").color(white).size(15.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let initials: String =
                        app.session.user().split(|c: char| !c.is_alphanumeric()).filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
                    let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 15.0, TOKENS.accent);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, initials, egui::FontId::proportional(12.0), white);
                    ui.add_space(8.0);
                    let icon = |ui: &mut Ui, g: &str, tip: &str| {
                        ui.add(egui::Button::new(RichText::new(g).color(white).size(16.0)).frame(false)).on_hover_text(tip)
                    };
                    if icon(ui, "?", "Help").clicked() {
                        app.notice = Some("Help: Alt+Q searches pages and tasks. Esc goes back. F9 posts, Ctrl+F7 opens entries.".into());
                    }
                    if icon(ui, "⚙", "Settings").clicked() {
                        app.dialog = Some(Dialog::params(
                            "My Settings".into(),
                            "session.workdate",
                            json!({"date": app.session.work_date().to_string()}),
                            vec!["date".into()],
                        ));
                    }
                    let _ = icon(ui, "🔔", "Notifications");
                    if icon(ui, "🔍", "Tell me what you want to do (Alt+Q)").clicked() {
                        app.tell_me = Some(crate::dialogs::TellMe::default());
                    }
                });
            });
        });
}

pub fn role_center_bars(app: &mut ErpApp, ui: &mut Ui) {
    let Some(rc) = app.session.meta().role_center(&app.role_center) else { return };
    let company = app
        .session
        .record("Company Information", &erpcraft_engine::meta::Key(vec![erpcraft_engine::meta::Value::text("")]))
        .map(|r| r.str("Name").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "My Company".to_string());
    let on_role_center = matches!(app.stack.last(), Some(crate::View::RoleCenter) | None);
    // Row 1: company name | section menus (Finance ⌄, Cash Management ⌄, …) | ☰.
    egui::Panel::top("nav_bar")
        .exact_size(50.0)
        .frame(egui::Frame::new().fill(TOKENS.page).inner_margin(egui::Margin::symmetric(24, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let name = ui.add(egui::Label::new(RichText::new(&company).size(19.0).color(TOKENS.text)).sense(Sense::click()));
                if name.clicked() {
                    app.home();
                }
                ui.add_space(10.0);
                let (sep, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                ui.painter().rect_filled(sep, 0.0, TOKENS.border);
                ui.add_space(10.0);
                let mut open = None;
                for s in &rc.sections {
                    ui.menu_button(RichText::new(format!("{} ⌄", s.caption)).color(TOKENS.text), |ui| {
                        ui.set_min_width(220.0);
                        for l in &s.links {
                            if ui.button(&l.caption).clicked() {
                                open = Some(l.page.clone());
                                ui.close();
                            }
                        }
                    });
                }
                ui.add_space(8.0);
                let (sep, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                ui.painter().rect_filled(sep, 0.0, TOKENS.border);
                let r = ui.add(egui::Button::new(RichText::new("☰").size(16.0).color(TOKENS.text)).frame(false)).on_hover_text("Navigation menu");
                if r.clicked() {
                    app.nav_menu_open = !app.nav_menu_open;
                }
                let rect = r.rect;
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("nav_menu_anchor"), rect.left_bottom()));
                if let Some(p) = open {
                    app.open_page(&p, Vec::new(), None);
                }
            });
        });
    if !on_role_center {
        return;
    }
    // Row 2: the embedded lists as teal links.
    egui::Panel::top("nav_links")
        .exact_size(34.0)
        .frame(egui::Frame::new().fill(TOKENS.page).inner_margin(egui::Margin::symmetric(24, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                for l in &rc.embedding {
                    if widgets::link(ui, &l.caption).clicked() {
                        app.open_page(&l.page, Vec::new(), None);
                    }
                }
            });
        });
    egui::Panel::top("rc_actions")
        .exact_size(40.0)
        .frame(
            egui::Frame::new()
                .fill(TOKENS.page)
                .inner_margin(egui::Margin::symmetric(24, 0))
                .stroke(egui::Stroke::new(1.0, TOKENS.divider)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                for l in &rc.creation {
                    if widgets::action_button(ui, "", &l.caption).clicked() {
                        app.open_page(&l.page, Vec::new(), None);
                    }
                }
                ui.add_space(6.0);
                for g in &rc.action_groups {
                    ui.menu_button(RichText::new(format!("{}  ⌄", g.caption)).color(TOKENS.text), |ui| {
                        for l in &g.links {
                            if ui.button(&l.caption).clicked() {
                                app.open_page(&l.page, Vec::new(), None);
                                ui.close();
                            }
                        }
                    });
                }
            });
        });
}

/// The ☰ flyout: every navigation section as a column of links.
pub fn nav_menu(app: &mut ErpApp, ctx: &egui::Context) {
    if !app.nav_menu_open {
        return;
    }
    let Some(rc) = app.session.meta().role_center(&app.role_center) else { return };
    let anchor = ctx.data(|d| d.get_temp::<egui::Pos2>(egui::Id::unique("nav_menu_anchor"))).unwrap_or(egui::pos2(24.0, 90.0));
    let mut open_page = None;
    let area = egui::Area::new(egui::Id::unique("nav_menu")).order(egui::Order::Foreground).fixed_pos(egui::pos2(24.0, anchor.y + 4.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(TOKENS.page).inner_margin(egui::Margin::same(20)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for s in &rc.sections {
                    ui.vertical(|ui| {
                        ui.set_min_width(170.0);
                        ui.label(RichText::new(&s.caption).font(theme::bold(theme::BODY)).color(TOKENS.text));
                        ui.add_space(4.0);
                        for l in &s.links {
                            if widgets::link(ui, &l.caption).clicked() {
                                open_page = Some(l.page.clone());
                            }
                        }
                    });
                    ui.add_space(16.0);
                }
            });
        });
    });
    if let Some(p) = open_page {
        app.nav_menu_open = false;
        app.open_page(&p, Vec::new(), None);
    } else if ctx.input(|i| i.pointer.any_click()) && !area.response.contains_pointer() && !area.response.hovered() {
        let pointer_in = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
        if !pointer_in && ctx.input(|i| i.pointer.primary_released()) {
            // Clicks outside close the menu (the ☰ button toggles it itself).
            let on_button = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.y < anchor.y && p.y > anchor.y - 46.0);
            if !on_button {
                app.nav_menu_open = false;
            }
        }
    }
}
