//! Visual language for PadForge.
//!
//! The palette is built around a near-black slate base with a single cyan
//! accent, borrowed from the lightbar glow the app is ultimately about. Every
//! colour is defined once here so the whole UI stays coherent, and the theme is
//! applied in one place (`install`) rather than widget by widget.

use egui::epaint::MarginF32;
use egui::{Color32, CornerRadius, FontId, Stroke, Vec2};

/// Base surface, darkest.
pub const BACKDROP: Color32 = Color32::from_rgb(0x0A, 0x0C, 0x10);
/// Panel surface.
pub const SURFACE: Color32 = Color32::from_rgb(0x12, 0x15, 0x1B);
/// Raised surface: cards, inputs.
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x18, 0x1C, 0x24);
/// Surface for hover states.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x22, 0x27, 0x31);
/// Hairline separators and outlines.
pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x2C, 0x38);

/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xEA, 0xF2);
/// Secondary text: labels, hints.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8B, 0x94, 0xA6);
/// Tertiary text: disabled states.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5A, 0x62, 0x72);

/// The lightbar cyan, used for anything active.
pub const ACCENT: Color32 = Color32::from_rgb(0x35, 0xD0, 0xE8);
/// Softer accent for large fills where full saturation would be loud.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1B, 0x6E, 0x86);
/// Accent at low alpha, for glows and tints.
pub const ACCENT_FAINT: Color32 = Color32::from_rgb(0x14, 0x3D, 0x4A);

pub const SUCCESS: Color32 = Color32::from_rgb(0x4A, 0xD9, 0x8C);
pub const WARNING: Color32 = Color32::from_rgb(0xF2, 0xB0, 0x4A);
pub const DANGER: Color32 = Color32::from_rgb(0xF2, 0x6D, 0x6D);

/// Colour for a battery charge fraction.
pub fn battery_color(fraction: f32) -> Color32 {
    if fraction <= 0.15 {
        DANGER
    } else if fraction <= 0.35 {
        WARNING
    } else {
        SUCCESS
    }
}

/// Slider rail and drag-handle track, deliberately darker than the card surface
/// so the track reads as a groove.
pub const TRACK: Color32 = Color32::from_rgb(0x0E, 0x11, 0x17);
/// The drag handle on a slider.
pub const HANDLE: Color32 = Color32::from_rgb(0xE6, 0xEA, 0xF2);

/// Corner radii, so rounding is consistent across the app.
pub const RADIUS_SM: CornerRadius = CornerRadius::same(6);
pub const RADIUS_MD: CornerRadius = CornerRadius::same(10);
pub const RADIUS_LG: CornerRadius = CornerRadius::same(14);
/// Fully rounded, for pills and status chips. `CornerRadius` stores `u8`, so the
/// maximum is 255 rather than an arbitrary large number.
pub const RADIUS_PILL: CornerRadius = CornerRadius::same(u8::MAX);

/// Standard spacing scale.
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 14.0;
pub const SPACE_LG: f32 = 22.0;

/// Apply the PadForge look to an egui context.
///
/// egui 0.36 keys styles per [`egui::Theme`], so the dark style is built from the
/// dark default and then applied to both themes: PadForge is dark-only, and a
/// system light-mode switch must not wash out the palette.
pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();

    // Type scale: a clear jump between headings and body, and a slightly larger
    // body size than egui's default because this is a dense control panel.
    style.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(21.0));
    style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));

    style.spacing.item_spacing = Vec2::new(SPACE_SM, SPACE_SM);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating_allocated_width = 8.0;
    style.spacing.interact_size.y = 26.0;

    // Widgets: a flat, low-chrome look. Emphasis comes from the accent rather
    // than from borders and shadows everywhere.
    style.visuals.dark_mode = true;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.weak_text_color = Some(TEXT_MUTED);
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.extreme_bg_color = BACKDROP;
    style.visuals.code_bg_color = SURFACE_RAISED;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.warn_fg_color = WARNING;
    style.visuals.error_fg_color = DANGER;
    style.visuals.window_fill = SURFACE;
    style.visuals.window_stroke = Stroke::new(1.0, BORDER);
    style.visuals.window_corner_radius = RADIUS_LG;
    style.visuals.menu_corner_radius = RADIUS_MD;
    style.visuals.panel_fill = SURFACE;
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };

    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    style.visuals.widgets.noninteractive.corner_radius = RADIUS_SM;

    // Slider rails are painted with inactive.bg_fill; it must differ from
    // the card surface or the track disappears.
    style.visuals.widgets.inactive.bg_fill = TRACK;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, HANDLE);
    style.visuals.widgets.inactive.corner_radius = RADIUS_SM;

    style.visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.hovered.corner_radius = RADIUS_SM;

    style.visuals.widgets.active.bg_fill = ACCENT_DIM;
    style.visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, BACKDROP);
    style.visuals.widgets.active.corner_radius = RADIUS_SM;

    style.visuals.widgets.open.bg_fill = SURFACE_HOVER;
    style.visuals.widgets.open.weak_bg_fill = SURFACE_HOVER;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.open.corner_radius = RADIUS_SM;

    // Selection highlight, used by text edits and lists.
    style.visuals.selection.bg_fill = ACCENT_FAINT;

    // Slider-specific: the accent fills the used portion, so a partially-set
    // slider reads at a glance. The handle keeps egui's default shape.
    style.visuals.slider_trailing_fill = true;
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.set_style_of(theme, style.clone());
        ctx.set_visuals_of(theme, style.visuals.clone());
    }
}

/// A subtle card frame used throughout the UI.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(RADIUS_MD)
        .inner_margin(MarginF32::same(SPACE_MD))
}

/// A full-bleed section separator.
pub fn divider(ui: &mut egui::Ui) {
    ui.add_space(SPACE_XS);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, BORDER);
    ui.add_space(SPACE_XS);
}

/// Draw a soft radial glow, used behind the controller preview and the logo.
///
/// A handful of concentric discs approximates a radial falloff closely enough at
/// this scale, and avoids depending on a shader.
pub fn glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    const STEPS: u32 = 6;
    for i in (1..=STEPS).rev() {
        let t = i as f32 / STEPS as f32;
        let r = radius * t;
        let alpha = (color.a() as f32 * 0.16 * (1.0 - t)).clamp(0.0, 255.0) as u8;
        painter.circle_filled(
            center,
            r,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
        );
    }
}

/// Primary action button: accent fill, dark label.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text)
                .color(BACKDROP)
                .strong()
                .size(14.0),
        )
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(RADIUS_SM)
        .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// Secondary button: raised surface, standard label.
pub fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(TEXT).size(14.0))
            .fill(SURFACE_RAISED)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(RADIUS_SM)
            .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// A small status pill, e.g. "Connected" or "Paused".
pub fn pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    let text = egui::RichText::new(label).color(color).size(11.5).strong();
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(RADIUS_PILL)
        .inner_margin(MarginF32::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.label(text);
        });
}

/// Section heading with a hairline underneath.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(SPACE_SM);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .color(TEXT_FAINT)
            .size(11.0)
            .strong(),
    );
    divider(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_colours_follow_the_level() {
        assert_eq!(battery_color(0.0), DANGER);
        assert_eq!(battery_color(0.2), WARNING);
        assert_eq!(battery_color(0.9), SUCCESS);
    }

    #[test]
    fn spacing_reaches_the_style() {
        // Comparing the constants directly would be a const-true assertion, so
        // check that the theme actually applies one instead.
        let ctx = egui::Context::default();
        install(&ctx);
        let style = ctx.style_of(egui::Theme::Dark);
        assert_eq!(style.spacing.item_spacing.y, SPACE_SM);
        assert_eq!(style.spacing.button_padding.y, 8.0);
    }

    #[test]
    fn install_sets_a_dark_theme() {
        let ctx = egui::Context::default();
        install(&ctx);
        assert!(ctx.style_of(egui::Theme::Dark).visuals.dark_mode);
        // The accent must survive into the widget visuals, otherwise nothing
        // would ever light up.
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.widgets.hovered.bg_stroke.color,
            ACCENT_DIM
        );
    }

    #[test]
    fn light_theme_is_also_overridden() {
        // The app is dark-only, so the light theme must not fall back to the
        // stock palette.
        let ctx = egui::Context::default();
        install(&ctx);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.override_text_color,
            Some(TEXT)
        );
    }
}
