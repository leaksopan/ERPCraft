//! Colours and text styles, modelled on the Business Central web client (Fluent): dark app bar,
//! white pages on a light grey canvas, teal accent for links, actions and cues.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub app_bar: Color32,
    pub app_bar_text: Color32,
    pub canvas: Color32,
    pub page: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub cue: Color32,
    pub cue_text: Color32,
    pub danger: Color32,
    pub notice: Color32,
    pub input_line: Color32,
    pub scrim: Color32,
    pub table_header: Color32,
    /// BC's list grid: row lines, header captions, cell text, links, header hover.
    pub grid_line: Color32,
    pub grid_header: Color32,
    pub grid_text: Color32,
    pub link: Color32,
    pub link_selected: Color32,
    pub header_hover: Color32,
}

pub const TOKENS: Tokens = Tokens {
    app_bar: Color32::from_rgb(0x20, 0x1F, 0x1E),
    app_bar_text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    canvas: Color32::from_rgb(0xF3, 0xF2, 0xF1),
    page: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    text: Color32::from_rgb(0x32, 0x31, 0x30),
    text_dim: Color32::from_rgb(0x60, 0x5E, 0x5C),
    border: Color32::from_rgb(0xE1, 0xDF, 0xDD),
    divider: Color32::from_rgb(0xED, 0xEB, 0xE9),
    accent: Color32::from_rgb(0x00, 0x84, 0x89),
    accent_hover: Color32::from_rgb(0x00, 0x5B, 0x60),
    accent_soft: Color32::from_rgb(0xE0, 0xF2, 0xF3),
    row_hover: Color32::from_rgb(0xF3, 0xF2, 0xF1),
    row_selected: Color32::from_rgb(0xB2, 0xE9, 0xED),
    cue: Color32::from_rgb(0x00, 0x8C, 0x93),
    cue_text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    danger: Color32::from_rgb(0xA4, 0x26, 0x2C),
    notice: Color32::from_rgb(0xE8, 0xF3, 0xFC),
    input_line: Color32::from_rgb(0x8A, 0x88, 0x86),
    scrim: Color32::from_rgb(0xC8, 0xC6, 0xC4),
    table_header: Color32::from_rgb(0xFA, 0xF9, 0xF8),
    grid_line: Color32::from_rgb(0xD3, 0xD6, 0xDA),
    grid_header: Color32::from_rgb(0x50, 0x5C, 0x6D),
    grid_text: Color32::from_rgb(0x21, 0x21, 0x21),
    link: Color32::from_rgb(0x00, 0x7E, 0x87),
    link_selected: Color32::from_rgb(0x00, 0x68, 0x6F),
    header_hover: Color32::from_rgb(0xF3, 0xF2, 0xF1),
};

pub const BODY: f32 = 13.5;
pub const SMALL: f32 = 12.0;
pub const TITLE: f32 = 22.0;
pub const SUBTITLE: f32 = 16.0;

/// Applies the light BC look to the context.
pub fn apply(ctx: &egui::Context) {
    let t = TOKENS;
    let mut v = Visuals::light();
    v.panel_fill = t.canvas;
    v.window_fill = t.page;
    v.extreme_bg_color = t.page;
    v.faint_bg_color = Color32::from_rgb(0xFA, 0xF9, 0xF8);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.row_selected;
    // Selected rows keep dark text (BC highlights the row, not the text).
    v.selection.stroke = Stroke::new(1.0, t.text);
    v.override_text_color = Some(t.text);
    v.window_corner_radius = CornerRadius::same(4);
    v.window_stroke = Stroke::new(1.0, t.border);
    v.menu_corner_radius = CornerRadius::same(2);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(2);
    }
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    // Buttons and menu buttons are flat, as on BC's bars; inputs keep their border.
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = t.page;
    v.widgets.hovered.weak_bg_fill = t.row_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.input_line);
    v.widgets.active.weak_bg_fill = t.divider;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    ctx.set_visuals(v);

    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(SMALL, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(BODY, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(BODY, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(TITLE, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(BODY, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.interact_size.y = 26.0;
    ctx.set_global_style(style);
}

/// Loads Segoe UI from the system when present (Windows), so text looks like the BC web client.
/// The font is read from the user's machine at run time and never shipped.
pub fn install_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var("WINDIR").map(|w| std::path::PathBuf::from(w).join("Fonts")).unwrap_or_default();
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe-bold", "segoeuib.ttf"), ("segoe-symbol", "seguisym.ttf")] {
        let path = dir.join(file);
        // Fonts are a few MB; anything much larger is not a font we want to load.
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() < 40 * 1024 * 1024
            && let Ok(bytes) = std::fs::read(&path)
        {
            fonts.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            if name == "segoe-bold" {
                fonts.families.insert(FontFamily::Name("bold".into()), vec![name.to_string()]);
            } else if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
                if name == "segoe" {
                    list.insert(0, name.to_string());
                } else {
                    list.push(name.to_string());
                }
            }
        }
    }
    if !fonts.families.contains_key(&FontFamily::Name("bold".into())) {
        let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        fonts.families.insert(FontFamily::Name("bold".into()), fallback);
    }
    // Always set: the "bold" family must exist even without Segoe UI (e.g. on the web).
    ctx.set_fonts(fonts);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
