//! The look: PhotoCraft's "Pro" dark palette, so the dashboard sits beside the other Redrob apps
//! without looking like a different family. Chrome, canvas and dock are three greys; one accent;
//! two signal colours (danger, warning) plus the crop's own green.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Visuals};

pub const CHROME: Color32 = Color32::from_rgb(50, 50, 50);
pub const CANVAS: Color32 = Color32::from_rgb(40, 40, 40);
pub const DOCK: Color32 = Color32::from_rgb(30, 30, 30);
pub const CARD: Color32 = Color32::from_rgb(50, 50, 50);
pub const FIELD: Color32 = Color32::from_rgb(36, 36, 36);
pub const FIELD_BORDER: Color32 = Color32::from_rgb(74, 74, 74);
pub const HOVER: Color32 = Color32::from_rgb(66, 66, 66);
pub const PRESSED: Color32 = Color32::from_rgb(78, 78, 78);
pub const TEXT: Color32 = Color32::from_rgb(222, 222, 222);
pub const TEXT_DIM: Color32 = Color32::from_rgb(178, 178, 178);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(128, 128, 128);
pub const ACCENT: Color32 = Color32::from_rgb(55, 142, 240);
pub const SEPARATOR: Color32 = Color32::from_rgb(30, 30, 30);
pub const TAB_STRIP: Color32 = Color32::from_rgb(38, 38, 38);
pub const ROW_SELECTED: Color32 = Color32::from_rgb(82, 82, 82);
pub const DANGER: Color32 = Color32::from_rgb(236, 91, 98);
pub const WARNING: Color32 = Color32::from_rgb(232, 176, 70);
pub const GOOD: Color32 = Color32::from_rgb(92, 184, 92);
/// Chat bubbles.
pub const BUBBLE_AGENT: Color32 = Color32::from_rgb(44, 44, 44);
pub const BUBBLE_USER: Color32 = Color32::from_rgb(38, 66, 104);

pub const RADIUS: u8 = 4;
pub const TOOL_STRIP_WIDTH: f32 = 44.0;
pub const DOCK_WIDTH: std::ops::RangeInclusive<f32> = 260.0..=520.0;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = CHROME;
    v.window_fill = CARD;
    v.window_stroke = Stroke::new(1.0, SEPARATOR);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = CARD;
    v.code_bg_color = FIELD;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARNING;
    v.error_fg_color = DANGER;
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;
    let r = CornerRadius::same(3);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    w.noninteractive.corner_radius = r;
    for (wv, bg, stroke) in [
        (&mut w.inactive, FIELD, FIELD_BORDER),
        (&mut w.hovered, HOVER, FIELD_BORDER),
        (&mut w.active, PRESSED, ACCENT),
        (&mut w.open, HOVER, FIELD_BORDER),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.0, TEXT);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(10.5)),
            (TextStyle::Body, FontId::proportional(12.0)),
            (TextStyle::Button, FontId::proportional(12.0)),
            (TextStyle::Heading, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(24.0, 24.0);
        s.spacing.slider_width = 150.0;
        s.spacing.combo_width = 120.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.icon_width = 14.0;
        s.visuals.indent_has_left_vline = false;
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.interact_handle_opacity = 1.0;
        scroll.interact_background_opacity = 0.4;
        scroll.floating_width = 4.0;
        s.spacing.scroll = scroll;
    });
}

/// A panel title in the dock, PhotoCraft style: small caps-ish heading on the tab strip colour.
pub fn panel_title(ui: &mut egui::Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TAB_STRIP);
    ui.painter().text(rect.left_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, title, FontId::proportional(11.5), TEXT_DIM);
}

/// A rounded card on the canvas.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, SEPARATOR))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(14))
        .show(ui, add)
        .inner
}
