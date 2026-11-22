//! Dark visual language for DiskScope.

use egui::{Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Stroke, TextStyle};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(10, 13, 18);
pub const TOPBAR: Color32 = Color32::from_rgb(12, 16, 23);
pub const SIDEBAR: Color32 = Color32::from_rgb(11, 15, 22);
pub const PANEL: Color32 = Color32::from_rgb(17, 22, 31);
pub const PANEL_HI: Color32 = Color32::from_rgb(21, 27, 38);
pub const BORDER: Color32 = Color32::from_rgb(29, 37, 49);
pub const BORDER_SOFT: Color32 = Color32::from_rgb(23, 30, 40);
pub const TEXT: Color32 = Color32::from_rgb(232, 237, 245);
pub const MUTED: Color32 = Color32::from_rgb(139, 151, 169);
pub const DIM: Color32 = Color32::from_rgb(95, 107, 124);
pub const ACCENT: Color32 = Color32::from_rgb(92, 148, 255);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(43, 74, 127);
pub const GREEN: Color32 = Color32::from_rgb(78, 210, 163);
pub const AMBER: Color32 = Color32::from_rgb(232, 168, 92);
pub const RED: Color32 = Color32::from_rgb(255, 107, 129);
pub const WIDGET: Color32 = Color32::from_rgb(23, 30, 41);
pub const WIDGET_HOVER: Color32 = Color32::from_rgb(30, 39, 51);
pub const WIDGET_ACTIVE: Color32 = Color32::from_rgb(38, 49, 63);
pub const SELECT: Color32 = Color32::from_rgb(27, 43, 69);

pub const SEMIBOLD: &str = "semibold";

pub fn semibold_family() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, semibold_family())
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(9, 12, 17);
    v.faint_bg_color = Color32::from_rgb(14, 18, 25);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = WIDGET;
    v.widgets.inactive.weak_bg_fill = WIDGET;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = WIDGET_HOVER;
    v.widgets.hovered.weak_bg_fill = WIDGET_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.bg_fill = WIDGET_ACTIVE;
    v.widgets.active.weak_bg_fill = WIDGET_ACTIVE;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_SOFT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.open.weak_bg_fill = WIDGET_HOVER;
    v.widgets.open.bg_fill = WIDGET_HOVER;

    ctx.all_styles_mut(|style| {
        style.visuals = v.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.menu_margin = Margin::same(6);

        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Heading, semibold(19.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let regular = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf").ok();
    let bold = std::fs::read(r"C:\Windows\Fonts\seguisb.ttf")
        .ok()
        .or_else(|| std::fs::read(r"C:\Windows\Fonts\segoeuib.ttf").ok());

    let mut have_regular = false;
    if let Some(bytes) = regular {
        fonts.font_data.insert("segoe".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        have_regular = true;
    }
    if let Some(bytes) = bold {
        fonts.font_data.insert("segoe_sb".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        let family = vec!["segoe_sb".to_owned(), "segoe".to_owned()];
        fonts.families.insert(semibold_family(), family);
    } else {
        fonts
            .families
            .insert(semibold_family(), fonts.families[&FontFamily::Proportional].clone());
    }
    if have_regular {
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "segoe".to_owned());
        }
        if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
            family.push("segoe".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn card() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
}

pub fn plain_card() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(12))
}

pub fn accent_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .color(Color32::from_rgb(8, 12, 20))
            .family(semibold_family())
            .size(13.0),
    )
    .fill(ACCENT)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(9))
    .min_size(egui::vec2(0.0, 32.0))
}

pub fn ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(TEXT).size(13.0))
        .fill(WIDGET)
        .stroke(Stroke::new(1.0, BORDER_SOFT))
        .corner_radius(CornerRadius::same(9))
        .min_size(egui::vec2(0.0, 32.0))
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(RED).size(13.0))
        .fill(Color32::from_rgb(38, 22, 28))
        .stroke(Stroke::new(1.0, Color32::from_rgb(66, 32, 42)))
        .corner_radius(CornerRadius::same(9))
        .min_size(egui::vec2(0.0, 32.0))
}

pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .color(DIM)
            .family(semibold_family()),
    );
    ui.add_space(4.0);
}

pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".to_owned();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.2} {}", UNITS[unit])
    } else if value < 100.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "0.8s", "42s", "3m 12s" — scan durations.
pub fn fmt_secs(secs: f64) -> String {
    if secs < 10.0 {
        format!("{secs:.1}s")
    } else if secs < 60.0 {
        format!("{secs:.0}s")
    } else {
        let m = (secs / 60.0).floor() as u64;
        let s = (secs % 60.0).round() as u64;
        format!("{m}m {s}s")
    }
}

pub fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "—".to_owned();
    }
    let p = part as f64 / whole as f64 * 100.0;
    if p >= 10.0 {
        format!("{p:.0}%")
    } else if p >= 1.0 {
        format!("{p:.1}%")
    } else if p > 0.0 {
        "<1%".to_owned()
    } else {
        "0%".to_owned()
    }
}

/// Rough text truncation for custom-painted rows (clipped anyway).
pub fn truncate(text: &str, max_px: f32, font_size: f32) -> String {
    if max_px <= 8.0 {
        return String::new();
    }
    let per_char = font_size * 0.53;
    let max_chars = (max_px / per_char).floor().max(1.0) as usize;
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn usage_bar(ui: &mut egui::Ui, used: u64, total: u64, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    paint_usage_bar(ui.painter(), rect, used, total);
}

pub fn paint_usage_bar(painter: &egui::Painter, rect: egui::Rect, used: u64, total: u64) {
    let radius = CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect, radius, Color32::from_rgb(25, 32, 43));
    if total == 0 {
        return;
    }
    let frac = (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
    let color = if frac > 0.92 {
        RED
    } else if frac > 0.78 {
        AMBER
    } else {
        ACCENT
    };
    let mut fill = rect;
    fill.set_width((rect.width() * frac).max(2.0));
    painter.rect_filled(fill, radius, color);
}
