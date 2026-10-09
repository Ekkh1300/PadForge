//! Custom widgets: the live pad preview, axis bars, and the colour picker.

use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use padcore::output::XButtons;
use padcore::report::{Buttons, Ds4Report};

use crate::theme::*;

/// PlayStation face-button glyphs, drawn as glyphs rather than vector art so
/// they pick up the UI font instead of needing a bespoke icon set.
const GLYPH_TRIANGLE: &str = "\u{25B3}";
const GLYPH_CIRCLE: &str = "\u{25CB}";
const GLYPH_CROSS: &str = "\u{2715}";
const GLYPH_SQUARE: &str = "\u{25A1}";
use padcore::profile::hsv_to_rgb;

/// A DS4 shape drawn as vector paths: the silhouette, two sticks, the D-pad,
/// four face buttons, the shoulder pair and the glowing lightbar.
///
/// Drawn rather than shipped as a bitmap so it scales cleanly and can be tinted
/// live from telemetry.
pub struct PadPreview {
    /// Live input state.
    pub report: Ds4Report,
    /// Which XInput buttons are actually lit, so a remap is visible.
    pub output_buttons: XButtons,
    /// Lightbar colour.
    pub lightbar: Color32,
    /// Dim everything, e.g. while disconnected or paused.
    pub dimmed: bool,
}

impl PadPreview {
    pub fn new() -> Self {
        Self {
            report: Ds4Report::neutral(),
            output_buttons: XButtons::default(),
            lightbar: ACCENT,
            dimmed: false,
        }
    }

    /// Draw into the available space, preserving a 3:2-ish aspect ratio.
    pub fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        // The preview is capped so it shares the dashboard with the axis
        // readouts rather than pushing them below the fold. A taller window
        // gives it more room, but never more than the cap.
        let available = ui.available_size();
        let max_h = (available.y * 0.52).clamp(200.0, 380.0);
        let size = Vec2::new(available.x.min(max_h * 0.72).min(300.0), max_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);

        if self.dimmed {
            painter.rect_filled(rect, RADIUS_LG, BACKDROP);
            glow(&painter, rect.center(), size.x * 0.4, ACCENT_FAINT);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "waiting for a pad",
                FontId::proportional(14.0),
                TEXT_FAINT,
            );
            return response;
        }

        let scale = size.x / 340.0;
        let body_w = size.x * 0.72;
        let body_h = size.y * 0.56;
        let cx = rect.center().x;
        let top = rect.min.y + size.y * 0.06;

        // Lightbar glow first, so it sits behind the body.
        let bar_center = Pos2::new(cx, top + 10.0 * scale);
        glow(&painter, bar_center, 62.0 * scale, self.lightbar);

        // --- body ------------------------------------------------------
        let body_top_left = Pos2::new(cx - body_w / 2.0, top + 14.0 * scale);
        let body_rect = Rect::from_min_max(
            body_top_left,
            Pos2::new(cx + body_w / 2.0, top + 14.0 * scale + body_h),
        );
        // Rounded rectangle: egui's rect_filled takes a radius, which is exactly
        // the shape we want here.
        painter.rect_filled(body_rect, RADIUS_LG, SURFACE_RAISED);
        painter.rect_stroke(
            body_rect,
            RADIUS_LG,
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Middle,
        );

        // --- lightbar --------------------------------------------------
        let bar_w = body_w * 0.44;
        let bar_h = 4.0 * scale;
        let bar_rect = Rect::from_min_max(
            Pos2::new(cx - bar_w / 2.0, bar_center.y - bar_h / 2.0),
            Pos2::new(cx + bar_w / 2.0, bar_center.y + bar_h / 2.0),
        );
        painter.rect_filled(bar_rect, RADIUS_PILL, self.lightbar);

        let r = &self.report;
        let b = r.buttons;

        // --- sticks ----------------------------------------------------
        // Left stick sits above-left of centre, right stick below-right.
        let l_stick = Pos2::new(cx - body_w * 0.24, body_rect.center().y - body_h * 0.20);
        let r_stick = Pos2::new(cx + body_w * 0.24, body_rect.center().y + body_h * 0.20);
        draw_stick(
            &painter,
            l_stick,
            30.0 * scale,
            (r.left_x, r.left_y),
            ACCENT,
            TEXT_MUTED,
        );
        draw_stick(
            &painter,
            r_stick,
            26.0 * scale,
            (r.right_x, r.right_y),
            self.lightbar,
            TEXT_MUTED,
        );

        // --- d-pad -----------------------------------------------------
        let dpad = Pos2::new(cx + body_w * 0.26, body_rect.center().y - body_h * 0.20);
        draw_dpad(
            &painter,
            dpad,
            26.0 * scale,
            b.contains(Buttons::UP),
            b.contains(Buttons::DOWN),
            b.contains(Buttons::LEFT),
            b.contains(Buttons::RIGHT),
        );

        // --- face buttons ----------------------------------------------
        // Diamond layout: triangle on top, circle right, cross bottom,
        // square left. The fill shows the DS4 press; the ring shows whether the
        // button is also lit on the virtual pad, so a remap is visible here
        // without cross-checking the Output page.
        let face = Pos2::new(cx - body_w * 0.26, body_rect.center().y + body_h * 0.20);
        let fr = 24.0 * scale;
        let gap = fr * 1.42;
        let out = self.output_buttons;
        for (offset, glyph, mask, target, idle) in [
            (
                Vec2::new(0.0, -gap),
                GLYPH_TRIANGLE,
                Buttons::TRIANGLE,
                XButtons::Y,
                TEXT,
            ),
            (
                Vec2::new(gap, 0.0),
                GLYPH_CIRCLE,
                Buttons::CIRCLE,
                XButtons::B,
                TEXT,
            ),
            (
                Vec2::new(0.0, gap),
                GLYPH_CROSS,
                Buttons::CROSS,
                XButtons::A,
                ACCENT,
            ),
            (
                Vec2::new(-gap, 0.0),
                GLYPH_SQUARE,
                Buttons::SQUARE,
                XButtons::X,
                TEXT,
            ),
        ] {
            draw_face_button(
                &painter,
                face + offset,
                fr,
                glyph,
                b.contains(mask),
                out.any(target),
                idle,
            );
        }

        // --- shoulders and triggers ------------------------------------
        let shoulder_y = body_rect.max.y - body_h * 0.06;
        draw_pill(
            &painter,
            Pos2::new(cx - body_w * 0.34, shoulder_y),
            body_w * 0.22,
            13.0 * scale,
            "L2",
            r.l2,
            b.contains(Buttons::L2),
        );
        draw_pill(
            &painter,
            Pos2::new(cx + body_w * 0.34, shoulder_y),
            body_w * 0.22,
            13.0 * scale,
            "R2",
            r.r2,
            b.contains(Buttons::R2),
        );

        // --- system row ------------------------------------------------
        let sys_y = body_rect.max.y - 9.0 * scale;
        draw_system(
            &painter,
            Pos2::new(cx - body_w * 0.30, sys_y),
            7.0 * scale,
            "[]",
            b.contains(Buttons::SHARE),
        );
        draw_system(
            &painter,
            Pos2::new(cx, sys_y),
            8.0 * scale,
            "=",
            b.contains(Buttons::OPTIONS),
        );
        draw_system(
            &painter,
            Pos2::new(cx + body_w * 0.30, sys_y),
            7.0 * scale,
            "[]",
            r.touch.pad_clicked,
        );

        response
    }
}

impl Default for PadPreview {
    fn default() -> Self {
        Self::new()
    }
}

/// An analogue stick: outer ring plus a cap displaced by its axes.
fn draw_stick(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    (x, y): (f32, f32),
    active: Color32,
    idle: Color32,
) {
    painter.circle_filled(center, radius, SURFACE);
    painter.circle_stroke(center, radius, Stroke::new(1.0, BORDER));

    let magnitude = (x * x + y * y).sqrt().clamp(0.0, 1.0);
    let travel = radius * 0.48;
    // The report's Y is already inverted, so +y is up on screen.
    let cap = center + Vec2::new(x * travel, -y * travel);
    let cap_r = radius * 0.52;
    if magnitude > 0.02 {
        glow(painter, cap, cap_r * 1.5, active);
    }
    painter.circle_filled(cap, cap_r, if magnitude > 0.02 { active } else { idle });
}

/// A cross-shaped D-pad whose arms light up individually.
fn draw_dpad(
    painter: &egui::Painter,
    center: Pos2,
    r: f32,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
) {
    let arm = r * 0.42;
    let len = r * 0.95;
    let shape: [Rect; 4] = [
        Rect::from_center_size(
            Pos2::new(center.x, center.y - len * 0.55),
            Vec2::new(arm * 2.0, len),
        ),
        Rect::from_center_size(
            Pos2::new(center.x, center.y + len * 0.55),
            Vec2::new(arm * 2.0, len),
        ),
        Rect::from_center_size(
            Pos2::new(center.x - len * 0.55, center.y),
            Vec2::new(len, arm * 2.0),
        ),
        Rect::from_center_size(
            Pos2::new(center.x + len * 0.55, center.y),
            Vec2::new(len, arm * 2.0),
        ),
    ];
    for (i, rect) in shape.iter().enumerate() {
        let lit = match i {
            0 => up,
            1 => down,
            2 => left,
            _ => right,
        };
        painter.rect_filled(
            *rect,
            CornerRadius::same(3),
            if lit { ACCENT } else { SURFACE_HOVER },
        );
    }
}

/// One of the four face buttons.
fn draw_face_button(
    painter: &egui::Painter,
    center: Pos2,
    r: f32,
    glyph: &str,
    pressed: bool,
    mapped: bool,
    idle: Color32,
) {
    if pressed {
        glow(painter, center, r * 1.7, ACCENT);
    }
    painter.circle_filled(center, r, if pressed { ACCENT } else { SURFACE_HOVER });
    painter.circle_stroke(
        center,
        r,
        Stroke::new(
            if mapped && !pressed { 2.0 } else { 1.0 },
            if pressed || mapped { ACCENT } else { BORDER },
        ),
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(r * 1.15),
        if pressed { BACKDROP } else { idle },
    );
}

/// A trigger bar showing both analog pressure and its digital state.
fn draw_pill(
    painter: &egui::Painter,
    center: Pos2,
    w: f32,
    h: f32,
    label: &str,
    value: f32,
    pressed: bool,
) {
    let rect = Rect::from_center_size(center, Vec2::new(w, h));
    painter.rect_filled(rect, RADIUS_PILL, SURFACE);
    painter.rect_stroke(
        rect,
        RADIUS_PILL,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Middle,
    );

    // Fill from the centre outwards.
    let fill = rect.shrink(2.0);
    let amount = value.clamp(0.0, 1.0);
    let colour = if pressed { ACCENT } else { ACCENT_DIM };
    if amount > 0.01 {
        let half = fill.width() * amount * 0.5;
        let bar = Rect::from_min_max(
            Pos2::new(fill.center().x - half, fill.min.y),
            Pos2::new(fill.center().x + half, fill.max.y),
        );
        painter.rect_filled(bar, RADIUS_PILL, colour);
    }
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.0),
        if amount > 0.5 { BACKDROP } else { TEXT_MUTED },
    );
}

/// A small square system button.
fn draw_system(painter: &egui::Painter, center: Pos2, r: f32, glyph: &str, pressed: bool) {
    let rect = Rect::from_center_size(center, Vec2::splat(r * 2.0));
    painter.rect_filled(
        rect,
        CornerRadius::same(3),
        if pressed { ACCENT } else { SURFACE_HOVER },
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(r * 1.4),
        if pressed { BACKDROP } else { TEXT_MUTED },
    );
}

/// A bipolar bar for a stick axis, drawn around a centre line.
pub fn axis_bar(ui: &mut egui::Ui, label: &str, value: f32, accent: Color32) {
    let height = 8.0;
    // Room for the label line plus the bar, with a little breathing space so
    // adjacent bars do not look joined.
    const LABEL_H: f32 = 16.0;
    let total = LABEL_H + height + 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        TEXT_MUTED,
    );
    painter.text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{value:+.2}"),
        FontId::monospace(11.0),
        TEXT_MUTED,
    );

    let bar = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + LABEL_H),
        Pos2::new(rect.max.x, rect.min.y + LABEL_H + height),
    );
    painter.rect_filled(bar, RADIUS_PILL, BACKDROP);

    // Centre tick, so the neutral position is obvious.
    let cx = bar.center().x;
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(cx, bar.center().y), Vec2::new(1.0, height)),
        0.0,
        BORDER,
    );

    let v = value.clamp(-1.0, 1.0);
    if v.abs() > 0.005 {
        let w = bar.width() * v.abs() * 0.5;
        let fill = if v < 0.0 {
            Rect::from_min_max(Pos2::new(cx - w, bar.min.y), Pos2::new(cx, bar.max.y))
        } else {
            Rect::from_min_max(Pos2::new(cx, bar.min.y), Pos2::new(cx + w, bar.max.y))
        };
        painter.rect_filled(fill, RADIUS_PILL, accent);
    }
}

/// A unipolar 0..=1 bar for a trigger.
pub fn trigger_bar(ui: &mut egui::Ui, label: &str, value: f32, pressed: bool) {
    let height = 8.0;
    const LABEL_H: f32 = 16.0;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), LABEL_H + height + 6.0),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        TEXT_MUTED,
    );
    painter.text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0),
        FontId::monospace(11.0),
        TEXT_MUTED,
    );

    let bar = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + LABEL_H),
        Pos2::new(rect.max.x, rect.min.y + LABEL_H + height),
    );
    painter.rect_filled(bar, RADIUS_PILL, BACKDROP);
    let amount = value.clamp(0.0, 1.0);
    if amount > 0.005 {
        let w = bar.width() * amount;
        painter.rect_filled(
            Rect::from_min_max(bar.min, Pos2::new(bar.min.x + w, bar.max.y)),
            RADIUS_PILL,
            if pressed { ACCENT } else { ACCENT_DIM },
        );
    }
}

/// A compact battery indicator.
pub fn battery_pill(ui: &mut egui::Ui, level: u8, charging: bool) {
    let fraction = (level as f32 / 10.0).clamp(0.0, 1.0);
    let colour = battery_color(fraction);
    let text = if charging {
        format!("+ {}%", (fraction * 100.0) as u32)
    } else {
        format!("{}%", (fraction * 100.0) as u32)
    };
    pill(ui, &text, colour);
}

/// A horizontal sparkline of recent values, for gyro and latency readouts.
pub fn sparkline(ui: &mut egui::Ui, values: &[f32], range: f32, colour: Color32) {
    let height = 34.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, RADIUS_SM, BACKDROP);
    if values.len() < 2 {
        return;
    }

    // Zero line, so the trace is readable as a signed value.
    let mid = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.min.x, mid), Pos2::new(rect.max.x, mid)],
        Stroke::new(1.0, BORDER),
    );

    let step = rect.width() / (values.len() - 1).max(1) as f32;
    let half = rect.height() * 0.42;
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let normalised = (v / range.max(f32::EPSILON)).clamp(-1.0, 1.0);
            Pos2::new(rect.min.x + step * i as f32, mid - normalised * half)
        })
        .collect();

    // One polyline rather than N segments, so the tessellator joins them cleanly.
    if points.len() >= 2 {
        let stroke = egui::epaint::PathStroke::new(1.6, colour);
        painter.add(egui::Shape::closed_line(points, stroke));
    }
}

/// An HSV colour picker, since egui has no built-in one.
pub struct ColourPicker {
    /// Hue 0..=1, saturation 0..=1, value 0..=1.
    pub hsv: [f32; 3],
}

impl ColourPicker {
    pub fn new(rgb: [u8; 3]) -> Self {
        let (h, s, v) = rgb_to_hsv(rgb);
        Self { hsv: [h, s, v] }
    }

    /// Current colour as bytes.
    pub fn rgb(&self) -> [u8; 3] {
        hsv_to_rgb(self.hsv[0], self.hsv[1], self.hsv[2])
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let current = self.rgb();

        // Hue strip.
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 18.0),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let steps = 64;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let c = padcore::profile::hsv_to_rgb(t, 1.0, 1.0);
            let x0 = rect.min.x + rect.width() * (i as f32 / steps as f32);
            let x1 = rect.min.x + rect.width() * ((i + 1) as f32 / steps as f32);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(x1, rect.max.y)),
                0.0,
                Color32::from_rgb(c[0], c[1], c[2]),
            );
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let t = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                self.hsv[0] = t;
            }
        }
        let hx = rect.min.x + rect.width() * self.hsv[0];
        painter.circle_stroke(Pos2::new(hx, rect.center().y), 7.0, Stroke::new(2.0, TEXT));

        ui.add_space(SPACE_XS);

        // Saturation/value square for the selected hue.
        let (sq_rect, sq_resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 110.0),
            Sense::click_and_drag(),
        );
        let sq_painter = ui.painter_at(sq_rect);
        let hx_colour = padcore::profile::hsv_to_rgb(self.hsv[0], 1.0, 1.0);
        // Horizontal = saturation, vertical = value (inverted, brighter at top).
        for ix in 0..24 {
            for iy in 0..16 {
                let s = ix as f32 / 23.0;
                let v = 1.0 - iy as f32 / 15.0;
                let c = padcore::profile::hsv_to_rgb(self.hsv[0], s, v);
                let x0 = sq_rect.min.x + sq_rect.width() * (ix as f32 / 24.0);
                let x1 = sq_rect.min.x + sq_rect.width() * ((ix + 1) as f32 / 24.0);
                let y0 = sq_rect.min.y + sq_rect.height() * (iy as f32 / 16.0);
                let y1 = sq_rect.min.y + sq_rect.height() * ((iy + 1) as f32 / 16.0);
                sq_painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1)),
                    0.0,
                    Color32::from_rgb(c[0], c[1], c[2]),
                );
            }
        }
        let _ = hx_colour;
        if sq_resp.clicked() {
            if let Some(p) = sq_resp.interact_pointer_pos() {
                self.hsv[1] = ((p.x - sq_rect.min.x) / sq_rect.width()).clamp(0.0, 1.0);
                self.hsv[2] = (1.0 - (p.y - sq_rect.min.y) / sq_rect.height()).clamp(0.0, 1.0);
            }
        }
        let cursor = Pos2::new(
            sq_rect.min.x + sq_rect.width() * self.hsv[1],
            sq_rect.max.y - sq_rect.height() * self.hsv[2],
        );
        sq_painter.circle_stroke(cursor, 6.0, Stroke::new(2.0, TEXT));
        sq_painter.rect_stroke(
            sq_rect,
            CornerRadius::same(4),
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Middle,
        );

        ui.add_space(SPACE_SM);

        // Swatch preview plus a hex readout.
        let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
        ui.painter().rect_filled(
            swatch_rect,
            RADIUS_SM,
            Color32::from_rgb(current[0], current[1], current[2]),
        );
        ui.painter().rect_stroke(
            swatch_rect,
            RADIUS_SM,
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Middle,
        );
        ui.add_space(SPACE_SM);
        ui.label(
            egui::RichText::new(format!(
                "#{:02X}{:02X}{:02X}",
                current[0], current[1], current[2]
            ))
            .color(TEXT_MUTED)
            .monospace(),
        );
    }
}

/// RGB bytes to HSV, all 0..=1.
pub fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta.abs() < f32::EPSILON {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;

    let s = if max.abs() < f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (h.rem_euclid(1.0), s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_through_rgb() {
        for rgb in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [18, 52, 86],
            [200, 200, 200],
            [0, 0, 0],
            [255, 255, 255],
        ] {
            let (h, s, v) = rgb_to_hsv(rgb);
            let back = padcore::profile::hsv_to_rgb(h, s, v);
            for (a, b) in rgb.iter().zip(back.iter()) {
                assert!(
                    (*a as i32 - *b as i32).abs() <= 1,
                    "{rgb:?} round-tripped to {back:?}"
                );
            }
        }
    }

    #[test]
    fn picker_starts_from_a_colour() {
        let p = ColourPicker::new([0x35, 0xD0, 0xE8]);
        assert_eq!(p.rgb(), [0x35, 0xD0, 0xE8]);
    }
}
