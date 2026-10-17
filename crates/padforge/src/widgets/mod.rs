//! Custom widgets: the live pad preview, axis bars, and the colour picker.

use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use padcore::formula::Inputs;
use padcore::output::XButtons;
use padcore::report::{Buttons, Ds4Report};

use crate::theme::*;

/// PlayStation face-button glyphs, drawn as glyphs rather than vector art so
/// they pick up the UI font instead of needing a bespoke icon set.
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
    /// Draw where the pad is being touched, not just that it is.
    ///
    /// Off by default: on the dashboard the preview is small enough that a
    /// five-pixel dot is noise, and the state it conveys — touched, clicked —
    /// is already two colours of fill. The Test page turns it on, because
    /// there the question it answers is the whole point: a touchpad that
    /// reports a touch and never moves is a different fault from one that
    /// reports nothing, and only the position shows which.
    pub touch_point: bool,
    /// Widest the drawing may get, in points.
    ///
    /// The dashboard keeps it small so it shares its card with the axis
    /// readouts. The Test page is nothing but the preview, so it asks for
    /// more and gets a drawing big enough to see a button fail on.
    pub max_width: f32,
}

impl PadPreview {
    pub fn new() -> Self {
        Self {
            report: Ds4Report::neutral(),
            output_buttons: XButtons::default(),
            lightbar: ACCENT,
            dimmed: false,
            touch_point: false,
            max_width: 310.0,
        }
    }

    /// Draw into the available space, preserving a 3:2-ish aspect ratio.
    pub fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        // The preview is capped so it shares the dashboard with the axis
        // readouts rather than pushing them below the fold. Landscape, because
        // a controller is wider than it is tall: a portrait box leaves a strip
        // of nothing hanging under the grips.
        let available = ui.available_size();
        // The drawing's own proportion, used both to size it and to ask how
        // much room a given width needs — otherwise the height cap and the
        // height it actually takes are two numbers that can disagree.
        let aspect = 0.60;
        let box_w = available
            .x
            .min(available.y / aspect)
            .clamp(200.0, self.max_width);
        // Just tall enough for the silhouette: the drawing runs from the
        // triggers near the top edge to the bottom of the grips, and the
        // leftover was a band of nothing between them and the next row.
        let size = Vec2::new(box_w, box_w * aspect);
        // Take the whole row and draw into the middle of it. A left-aligned box
        // of this width left a third of the card empty on the right and pushed
        // the controller against its left edge.
        let (row, response) =
            ui.allocate_exact_size(Vec2::new(available.x, size.y), Sense::hover());
        let rect = Rect::from_min_size(
            Pos2::new(
                row.min.x + ((row.width() - size.x) * 0.5).max(0.0),
                row.min.y,
            ),
            size,
        );
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

        let w = size.x;
        let h = size.y;
        let scale = size.x / 340.0;
        let cx = rect.center().x;
        // Room above the body for the triggers and the lightbar.
        //
        // Not 96% of the box: the grips bulge past the slab on both sides, so a
        // body that nearly fills the box puts the widest part of the controller
        // outside the clip and cuts each grip off with a straight vertical edge.
        let body_w = w * 0.92;
        let body_h = body_w * 0.44;
        let body_top = rect.min.y + h * 0.14;
        let body_rect = Rect::from_min_max(
            Pos2::new(cx - body_w / 2.0, body_top),
            Pos2::new(cx + body_w / 2.0, body_top + body_h),
        );
        // A lobe hanging from each lower corner. The slab alone is a box; the
        // pair of grips is what makes the outline read as a controller, which
        // is what the preview was failing to do.
        let grip_r = body_w * 0.16;
        let grip_cy = body_rect.max.y - grip_r * 0.35;
        let grips = [
            Pos2::new(body_rect.min.x + grip_r * 0.85, grip_cy),
            Pos2::new(body_rect.max.x - grip_r * 0.85, grip_cy),
        ];

        // Lightbar glow first, so it sits behind the body. Wider and stronger
        // than the strip itself, because that spill onto the shell is the whole
        // reason a lightbar reads as a lightbar rather than as a coloured line.
        let bar_center = Pos2::new(cx, body_rect.min.y);
        glow(&painter, bar_center, h * 0.30, self.lightbar);
        // A second, tighter bloom on the strip itself.
        glow(&painter, bar_center, h * 0.10, self.lightbar);

        // --- body ------------------------------------------------------
        //
        // Every part is stroked first and filled afterwards, so a fill covers
        // the stroke wherever two parts overlap and only the outline of the
        // whole silhouette survives. Stroking them as they are drawn instead
        // leaves the seam of each grip running across the middle of the pad.
        //
        // The fill is darker than the card and the line brighter than both: at
        // the same fill the body was the same colour as the card it sat on, so
        // the outline was all there was of it.
        //
        // A single traced outline was tried and is the wrong call here. The
        // DualShock's silhouette is not convex -- the notch between the grips
        // turns the bottom into a W -- and egui's only path fill is a convex
        // one, so a traced outline closes over the notch and the grips
        // disappear into it. A rectangle with two lobes keeps the shape
        // readable, and the lobes are what make it a controller at all.
        for grip in grips {
            painter.circle_stroke(grip, grip_r, Stroke::new(1.5, SHELL_EDGE));
        }
        painter.rect_stroke(
            body_rect,
            RADIUS_LG,
            Stroke::new(1.5, SHELL_EDGE),
            egui::StrokeKind::Middle,
        );
        // A soft drop shadow under the whole silhouette. Without it the pad is
        // a flat outline sitting on a flat card, which is what made it read as
        // a diagram rather than as an object.
        for grip in grips {
            painter.circle_filled(
                grip + Vec2::new(0.0, 3.0 * scale),
                grip_r,
                Color32::from_black_alpha(95),
            );
        }
        painter.rect_filled(
            body_rect.translate(Vec2::new(0.0, 3.0 * scale)),
            RADIUS_LG,
            Color32::from_black_alpha(95),
        );
        for grip in grips {
            painter.circle_filled(grip, grip_r, SHELL);
        }
        painter.rect_filled(body_rect, RADIUS_LG, SHELL);

        // A vertical gradient across the shell, in a few bands.
        //
        // A flat fill is what made the preview read as a diagram. The bands are
        // coarse on purpose: at this size the steps are invisible and the
        // overall darkening towards the grips is what the eye reads as a curved
        // surface catching light from above. Eight is more than enough and
        // costs eight rectangles rather than a shader.
        const BANDS: usize = 8;
        for band in 0..BANDS {
            let k = (band + 1) as f32 / BANDS as f32;
            let top = body_rect.min.y + body_rect.height() * band as f32 / BANDS as f32;
            let band_rect = Rect::from_min_max(
                Pos2::new(body_rect.min.x, top),
                Pos2::new(
                    body_rect.max.x,
                    top + body_rect.height() / BANDS as f32 + 1.0,
                ),
            );
            // Each band keeps the body's own corner radius, and bands past the
            // bottom are skipped, so the gradient does not square off the
            // corners the fill gave it.
            if band_rect.min.y >= body_rect.max.y {
                break;
            }
            painter.rect_filled(band_rect, RADIUS_LG, SHELL.lerp_to_gamma(SHELL_SHADE, k));
        }
        // The gradient is square-cornered, so the rounded outline goes back on
        // top of it.
        painter.rect_stroke(
            body_rect,
            RADIUS_LG,
            Stroke::new(1.5, SHELL_EDGE),
            egui::StrokeKind::Middle,
        );

        // A highlight along the top edge: the one hard cue that the surface
        // curves away from the light rather than merely being darker below.
        let hi = Rect::from_min_max(
            Pos2::new(body_rect.min.x + body_w * 0.06, body_rect.min.y + 1.5),
            Pos2::new(body_rect.max.x - body_w * 0.06, body_rect.min.y + 3.0),
        );
        painter.rect_filled(hi, RADIUS_PILL, Color32::from_white_alpha(28));
        // --- lightbar --------------------------------------------------
        // The strip itself, plus a blurred halo drawn underneath it, so the
        // colour appears to come from inside the shell rather than being
        // painted on top of it.
        let bar_w = body_w * 0.26;
        let bar_h = (4.0 * scale).max(3.0);
        let bar_rect = Rect::from_min_max(
            Pos2::new(cx - bar_w / 2.0, bar_center.y - bar_h),
            Pos2::new(cx + bar_w / 2.0, bar_center.y + bar_h * 0.5),
        );
        glow(&painter, bar_rect.center(), bar_w * 0.9, self.lightbar);
        painter.rect_filled(bar_rect, RADIUS_PILL, self.lightbar);
        // Specular line along the top of the strip, as it has in life.
        let spec = Rect::from_min_max(
            Pos2::new(bar_rect.min.x + bar_w * 0.1, bar_rect.min.y),
            Pos2::new(bar_rect.max.x - bar_w * 0.1, bar_rect.min.y + 1.0),
        );
        painter.rect_filled(spec, RADIUS_PILL, Color32::from_white_alpha(70));

        let r = &self.report;
        let b = r.buttons;
        let body_cy = body_rect.center().y;

        // --- triggers ----------------------------------------------------
        // Above the body, on the top edge where the hardware has them. They
        // used to sit along the bottom, below the grips, which is nowhere.
        let trig_y = body_rect.min.y - h * 0.055;
        let trig_w = body_w * 0.20;
        let trig_x = body_w * 0.33;
        draw_pill(
            &painter,
            Pos2::new(cx - trig_x, trig_y),
            trig_w,
            15.0 * scale,
            "L2",
            r.l2,
            b.contains(Buttons::L2),
        );
        draw_pill(
            &painter,
            Pos2::new(cx + trig_x, trig_y),
            trig_w,
            15.0 * scale,
            "R2",
            r.r2,
            b.contains(Buttons::R2),
        );

        // L1 and R1: a thin bar along the top of each shoulder, under its
        // trigger. Neither had a place on the preview before. They sit tight
        // against the top edge, because a finger's worth of gap here is what
        // the top face button used to run into.
        for (side, mask) in [(cx - trig_x, Buttons::L1), (cx + trig_x, Buttons::R1)] {
            draw_shoulder(
                &painter,
                Rect::from_center_size(
                    Pos2::new(side, body_rect.min.y + 6.0 * scale),
                    Vec2::new(trig_w, 8.0 * scale),
                ),
                b.contains(mask),
                if mask == Buttons::L1 { "L1" } else { "R1" },
            );
        }

        // --- touchpad ----------------------------------------------------
        // Centred, between the two clusters, where the hardware has it, and
        // it lights up with the state it reports: touched, then clicked.
        let touch_size = Vec2::new(body_w * 0.17, body_h * 0.44);
        let touch_rect = Rect::from_center_size(Pos2::new(cx, body_cy - body_h * 0.05), touch_size);
        let (touch_fill, touch_edge) = if r.touch.pad_clicked {
            (ACCENT_DIM, ACCENT)
        } else if r.touch.pad_touched {
            (ACCENT_FAINT, ACCENT_DIM)
        } else {
            // Recessed, not flush: the pad has the body's colour, so a touchpad
            // drawn in it would have been a rectangle of nothing.
            (BACKDROP, BORDER)
        };
        painter.rect_filled(touch_rect, RADIUS_SM, touch_fill);
        painter.rect_stroke(
            touch_rect,
            RADIUS_SM,
            Stroke::new(1.0, touch_edge),
            egui::StrokeKind::Middle,
        );
        if self.touch_point && r.touch.pad_touched {
            // Where, not just whether. The coordinates are already normalised
            // over the active area, so this is a direct mapping and a finger
            // crossing the pad leaves the dot crossing with it.
            let dot = Pos2::new(
                touch_rect.left() + r.touch.x.clamp(0.0, 1.0) * touch_rect.width(),
                touch_rect.top() + r.touch.y.clamp(0.0, 1.0) * touch_rect.height(),
            );
            painter.circle_filled(dot, 3.5 * scale, ACCENT);
        }

        // --- d-pad -------------------------------------------------------
        // Upper left, where it is on the pad. The preview had the d-pad on the
        // right and the face buttons on the left: a mirrored controller, which
        // is why it did not read as one.
        //
        // The clusters sit in the upper half and the sticks in the lower, with
        // the radii chosen so nothing touches: at the first sizing the bottom
        // face button ran straight into the right stick, and at the second the
        // top one ran under the R1 bar.
        let cluster_y = body_cy - body_h * 0.16;
        draw_dpad(
            &painter,
            Pos2::new(cx - body_w * 0.30, cluster_y),
            body_h * 0.20,
            b.contains(Buttons::UP),
            b.contains(Buttons::DOWN),
            b.contains(Buttons::LEFT),
            b.contains(Buttons::RIGHT),
        );

        // --- face buttons -----------------------------------------------
        // Diamond layout: triangle on top, circle right, cross bottom,
        // square left. The fill shows the DS4 press; the ring shows whether the
        // button is also lit on the virtual pad, so a remap is visible here
        // without cross-checking the Output page.
        let face = Pos2::new(cx + body_w * 0.30, cluster_y);
        let fr = body_h * 0.095;
        let gap = fr * 1.42;
        let out = self.output_buttons;
        for (offset, which, mask, target, idle) in [
            (
                Vec2::new(0.0, -gap),
                Face::Triangle,
                Buttons::TRIANGLE,
                XButtons::Y,
                TEXT,
            ),
            (
                Vec2::new(gap, 0.0),
                Face::Circle,
                Buttons::CIRCLE,
                XButtons::B,
                TEXT,
            ),
            (
                Vec2::new(0.0, gap),
                Face::Cross,
                Buttons::CROSS,
                XButtons::A,
                ACCENT,
            ),
            (
                Vec2::new(-gap, 0.0),
                Face::Square,
                Buttons::SQUARE,
                XButtons::X,
                TEXT,
            ),
        ] {
            draw_face_button(
                &painter,
                face + offset,
                fr,
                which,
                b.contains(mask),
                out.any(target),
                idle,
            );
        }

        // --- sticks ------------------------------------------------------
        // Under the two clusters, left and right, as on the hardware.
        let stick_y = body_cy + body_h * 0.30;
        draw_stick(
            &painter,
            Pos2::new(cx - body_w * 0.30, stick_y),
            body_h * 0.19,
            (r.left_x, r.left_y),
            ACCENT,
            TEXT_MUTED,
        );
        draw_stick(
            &painter,
            Pos2::new(cx + body_w * 0.30, stick_y),
            body_h * 0.19,
            (r.right_x, r.right_y),
            self.lightbar,
            TEXT_MUTED,
        );

        // --- system row --------------------------------------------------
        // Share left of the touchpad, Options right of it, PS underneath.
        let sys_y = body_cy - body_h * 0.05;
        let sys_x = touch_size.x * 0.5 + 14.0;
        draw_system(
            &painter,
            Pos2::new(cx - sys_x, sys_y),
            6.0 * scale,
            "[]",
            b.contains(Buttons::SHARE),
        );
        draw_system(
            &painter,
            Pos2::new(cx + sys_x, sys_y),
            6.0 * scale,
            "=",
            b.contains(Buttons::OPTIONS),
        );
        // The PS button carries no bit of its own in the report, so it is drawn
        // as it sits rather than as a state that would never change.
        let ps = Pos2::new(cx, body_cy + body_h * 0.36);
        painter.circle_filled(ps, 9.0 * scale, SURFACE_HOVER);
        painter.circle_stroke(ps, 9.0 * scale, Stroke::new(1.0, BORDER));

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
    // The well is darker than the body it is cut into, so it reads as a socket
    // rather than as a ring drawn on a flat plate.
    painter.circle_filled(center, radius, BACKDROP);
    painter.circle_stroke(center, radius, Stroke::new(1.0, BORDER));

    let magnitude = (x * x + y * y).sqrt().clamp(0.0, 1.0);
    let travel = radius * 0.48;
    // The report's Y is already inverted, so +y is up on screen.
    let cap = center + Vec2::new(x * travel, -y * travel);
    let cap_r = radius * 0.52;
    let deflected = magnitude > 0.02;
    if deflected {
        glow(painter, cap, cap_r * 1.5, active);
    }
    // A dark cap in a lighter well, so the two sticks do not sit on the body
    // as pale discs; the accent only arrives when the stick actually moves.
    painter.circle_filled(cap, cap_r, if deflected { active } else { SURFACE_HOVER });
    painter.circle_stroke(
        cap,
        cap_r,
        Stroke::new(1.0, if deflected { active } else { idle }),
    );
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

/// Which face button a glyph belongs to.
///
/// Drawn as shapes rather than as text: the geometric symbols the default egui
/// fonts ship with are a partial set, and the triangle and the cross came out
/// as whatever the font fallback happened to offer -- a hollow box and a dash
/// -- which is a poor way to show which button is down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Face {
    Triangle,
    Circle,
    Cross,
    Square,
}

impl Face {
    /// The glyph, in the button's current colour.
    fn draw(&self, painter: &egui::Painter, center: Pos2, r: f32, colour: Color32) {
        let d = r * 0.46;
        let stroke = Stroke::new(1.6, colour);
        match self {
            Face::Triangle => {
                let pts = vec![
                    Pos2::new(center.x, center.y - d),
                    Pos2::new(center.x + d * 0.92, center.y + d * 0.72),
                    Pos2::new(center.x - d * 0.92, center.y + d * 0.72),
                ];
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::TRANSPARENT,
                    stroke,
                ));
            }
            Face::Circle => {
                painter.circle_stroke(center, d * 0.86, stroke);
            }
            Face::Cross => {
                painter.line_segment(
                    [
                        Pos2::new(center.x - d * 0.8, center.y - d * 0.8),
                        Pos2::new(center.x + d * 0.8, center.y + d * 0.8),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(center.x + d * 0.8, center.y - d * 0.8),
                        Pos2::new(center.x - d * 0.8, center.y + d * 0.8),
                    ],
                    stroke,
                );
            }
            Face::Square => {
                painter.rect_stroke(
                    Rect::from_center_size(center, Vec2::splat(d * 1.5)),
                    2.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
    }
}

/// One of the four face buttons.
fn draw_face_button(
    painter: &egui::Painter,
    center: Pos2,
    r: f32,
    face: Face,
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
    let colour = if pressed {
        BACKDROP
    } else if mapped {
        ACCENT
    } else {
        idle
    };
    face.draw(painter, center, r, colour);
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

/// A shoulder button: a thin bar along the top of the body, under its trigger.
///
/// L1 and R1 had no place on the preview at all before, so a press of either
/// only showed up in the list of controls underneath.
fn draw_shoulder(painter: &egui::Painter, rect: Rect, pressed: bool, label: &str) {
    let edge = if pressed { ACCENT } else { BORDER };
    painter.rect_filled(
        rect,
        RADIUS_PILL,
        if pressed { ACCENT } else { SURFACE_HOVER },
    );
    painter.rect_stroke(
        rect,
        RADIUS_PILL,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Middle,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(7.0),
        if pressed { BACKDROP } else { TEXT_FAINT },
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

/// A stick's live position, drawn as a dot inside its travel circle.
///
/// A bar says how far one axis has moved; it cannot say where the stick *is*,
/// and the difference is the whole question when a binding feels wrong. The
/// circle is the stick's full travel, so one resting off-centre is visible
/// without reading its number, and a diagonal reads as a diagonal.
pub fn stick_plot(ui: &mut egui::Ui, label: &str, x: f32, y: f32, accent: Color32) {
    const LABEL_H: f32 = 16.0;
    const SIZE: f32 = 96.0;
    const FOOT_H: f32 = 14.0;
    let total = LABEL_H + SIZE + FOOT_H;
    let width = ui.available_width().max(SIZE);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        TEXT_MUTED,
    );

    let field = Rect::from_min_size(
        Pos2::new(rect.center().x - SIZE * 0.5, rect.min.y + LABEL_H),
        Vec2::new(SIZE, SIZE),
    );
    painter.rect_filled(field, RADIUS_SM, BACKDROP);

    // The travel circle and a neutral crosshair, so "off-centre" is something
    // the eye can see rather than something the number has to say.
    let centre = field.center();
    let radius = SIZE * 0.5 - 4.0;
    painter.rect_stroke(
        field,
        RADIUS_SM,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    painter.circle_stroke(centre, radius, Stroke::new(1.0, BORDER));
    let tick = radius * 0.2;
    painter.line_segment(
        [
            Pos2::new(centre.x - tick, centre.y),
            Pos2::new(centre.x + tick, centre.y),
        ],
        Stroke::new(1.0, BORDER),
    );
    painter.line_segment(
        [
            Pos2::new(centre.x, centre.y - tick),
            Pos2::new(centre.x, centre.y + tick),
        ],
        Stroke::new(1.0, BORDER),
    );

    // Clamped to the travel circle: a stick reporting past its own physical
    // range should read as clamped rather than be drawn off the panel.
    let (dx, dy) = (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0));
    let magnitude = (dx * dx + dy * dy).sqrt();
    let scale = if magnitude > 1.0 {
        radius / magnitude
    } else {
        radius
    };
    // Negated: XInput's Y is up, screen coordinates' Y is down.
    let dot = Pos2::new(centre.x + dx * scale, centre.y - dy * scale);
    painter.circle_filled(dot, 5.0, accent);
    painter.circle_stroke(dot, 5.0, Stroke::new(1.0, BACKDROP));

    painter.text(
        Pos2::new(rect.center().x, field.bottom() + 2.0),
        egui::Align2::CENTER_TOP,
        format!("{dx:+.2}  {dy:+.2}"),
        FontId::monospace(10.0),
        TEXT_FAINT,
    );
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
///
/// It takes the whole status rather than a level and a flag: the pad counts to
/// a different ceiling depending on whether it is being charged, so a level on
/// its own does not say how much charge is left.
pub fn battery_pill(ui: &mut egui::Ui, battery: &padcore::report::Battery) {
    let fraction = battery.fraction();
    let colour = battery_color(fraction);
    let text = if battery.charging {
        format!("+ {}%", (fraction * 100.0) as u32)
    } else {
        format!("{}%", (fraction * 100.0) as u32)
    };
    pill(ui, &text, colour);
}

/// How the pad is plugged in, given its own colour and its own drawn glyph.
///
/// Wired and wireless used to be told apart by eleven-point faint text, which
/// is the one distinction this app cannot afford to make subtle: a pad that has
/// changed transport is exactly the thing a user notices before we do. Each
/// transport now has a hue that no other status uses, so the answer is
/// available without reading.
pub fn transport_pill(ui: &mut egui::Ui, transport: padcore::report::Transport) {
    use padcore::report::Transport;
    const SIZE: f32 = 11.5;
    const PAD_X: f32 = 9.0;
    const PAD_Y: f32 = 3.0;
    const ICON_W: f32 = 8.0;
    const GAP: f32 = 5.0;

    let (label, colour) = match transport {
        Transport::Usb => ("USB", ACCENT),
        Transport::Bluetooth => ("Bluetooth", WIRELESS),
    };
    let font_id = FontId::proportional(SIZE);
    // Measured through the painter rather than `Ui::fonts`, whose reader hands
    // out a shared reference: laying text out needs the mutable one.
    let text = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id.clone(), colour)
        .size();
    let height = PAD_Y * 2.0 + text.y;
    let width = PAD_X * 2.0 + ICON_W + GAP + text.x;

    // Reserved at an exact size rather than wrapped in a `Frame`. A frame added
    // inside the header's right-to-left row expands to whatever width is still
    // free, which drew this badge as a strip running the full width of the
    // window. Asking for the size outright makes it the same shape wherever it
    // lands, which is the only way a badge can be trusted in two different
    // layouts.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, RADIUS_PILL, colour.gamma_multiply(0.14));
    painter.rect_stroke(
        rect,
        RADIUS_PILL,
        Stroke::new(1.0, colour.gamma_multiply(0.45)),
        egui::StrokeKind::Middle,
    );

    let icon = Rect::from_center_size(
        Pos2::new(rect.left() + PAD_X + ICON_W * 0.5, rect.center().y),
        Vec2::new(ICON_W, height - PAD_Y * 2.0),
    );
    transport_icon(&painter, icon, transport, colour);
    painter.text(
        Pos2::new(icon.right() + GAP, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font_id,
        colour,
    );
}

/// Draw a transport glyph inside `rect`: a plug for USB, the Bluetooth rune for
/// Bluetooth.
///
/// The rune is one continuous stroke — down-right, down-left across the stave,
/// to the bottom, back up the stave, then the mirrored chevron — so it is a
/// single polyline rather than an approximation. No font PadForge ships carries
/// a Bluetooth symbol, and a missing glyph would render as a box, which is
/// worse than no icon at all.
fn transport_icon(
    painter: &egui::Painter,
    rect: Rect,
    transport: padcore::report::Transport,
    colour: Color32,
) {
    use padcore::report::Transport;
    let c = rect.center();
    let (w, h) = (rect.width() * 0.5, rect.height() * 0.5);
    let point = |x: f32, y: f32| Pos2::new(c.x + x * w, c.y + y * h);
    let stroke = Stroke::new(1.35, colour);

    match transport {
        Transport::Bluetooth => {
            let pts = vec![
                point(0.0, -1.0),
                point(0.62, -0.42),
                point(-0.62, 0.42),
                point(0.0, 1.0),
                point(0.0, -1.0),
                point(0.62, 0.42),
                point(-0.62, -0.42),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        // A plug rather than the USB trident: the trident needs curves to read
        // correctly at this size, and a cable with a connector body survives
        // being nine pixels wide.
        Transport::Usb => {
            painter.line_segment([point(0.0, 1.0), point(0.0, -0.15)], stroke);
            let body = Rect::from_center_size(
                point(0.0, -0.55),
                Vec2::new(rect.width() * 0.9, rect.height() * 0.7),
            );
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Middle);
            let pin = Stroke::new(1.35, colour);
            painter.line_segment([point(-0.28, -0.85), point(-0.28, -0.62)], pin);
            painter.line_segment([point(0.28, -0.85), point(0.28, -0.62)], pin);
        }
    }
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

    // One polyline rather than N segments, so the tessellator joins them
    // cleanly. Open, not closed: a closed path draws a line from the newest
    // sample back to the oldest, which for a latency trace is a straight bar
    // across the plot that reads as data but is not.
    if points.len() >= 2 {
        let stroke = egui::epaint::PathStroke::new(1.6, colour);
        painter.add(egui::Shape::line(points, stroke));
    }
}

/// The response curve of one axis: stick position along the bottom, what
/// actually reaches the game up the side, across the full travel of both.
///
/// Drawn rather than described because the shape is the whole point. A pair of
/// sliders for deadzone and sensitivity cannot say what their combination does,
/// and racing is the case where the combination is the thing being tuned — a
/// driver feels a curve, not two numbers. This is also the only view in the app
/// that shows the two halves of a bipolar axis agreeing with each other.
pub fn response_curve(
    ui: &mut egui::Ui,
    settings: &padcore::profile::AxisSettings,
    live: f32,
    height: f32,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    // The plot sits in its own recessed panel with a hairline, so it reads as a
    // graph area rather than as a hole cut in the card. Without the border the
    // axes float in the middle of nothing and the eye has nothing to measure
    // them against.
    painter.rect_filled(rect, RADIUS_SM, BACKDROP);
    painter.rect_stroke(
        rect,
        RADIUS_SM,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );

    const PAD: f32 = 10.0;
    let plot = Rect::from_min_max(
        Pos2::new(rect.min.x + PAD, rect.min.y + PAD),
        Pos2::new(rect.max.x - PAD, rect.max.y - PAD),
    );
    if plot.width() < 8.0 || plot.height() < 8.0 {
        return;
    }
    let at = |x: f32, y: f32| {
        Pos2::new(
            plot.center().x + x * plot.width() * 0.5,
            plot.center().y - y * plot.height() * 0.5,
        )
    };

    // The dead zone as a filled band, not just a flat stretch of curve. A flat
    // slope is ambiguous -- it could be a dead zone or a very gentle curve --
    // whereas a shaded region says outright that nothing happens in here.
    let dz = settings.deadzone.clamp(0.0, 1.0);
    if dz > 0.001 {
        let band = Rect::from_min_max(at(-dz, 0.0), at(dz, 0.0));
        painter.rect_filled(band, 0.0, ACCENT_FAINT.gamma_multiply(0.7));
    }

    // The identity line, so a reshaped curve has something to be measured
    // against: above it the axis is quicker off centre, below it slower.
    painter.line_segment([at(-1.0, -1.0), at(1.0, 1.0)], Stroke::new(1.0, BORDER));
    // Centre guides, so the dead zone reads as a region rather than as a slope
    // that happens to be flat.
    painter.line_segment([at(0.0, -1.0), at(0.0, 1.0)], Stroke::new(1.0, BORDER));
    painter.line_segment([at(-1.0, 0.0), at(1.0, 0.0)], Stroke::new(1.0, BORDER));

    const STEPS: u32 = 96;
    let formula = settings.formula();
    let mut points = Vec::with_capacity(STEPS as usize + 1);
    for i in 0..=STEPS {
        let x = (i as f32 / STEPS as f32) * 2.0 - 1.0;
        // A fresh filter per sample: smoothing is stateful, and letting it carry
        // between samples would draw a curve the user can never actually get.
        let mut filter = settings.to_filter();
        let y = filter.apply_with_formula(x, formula.as_ref(), &Inputs::default());
        points.push(at(x, y.clamp(-1.0, 1.0)));
    }
    // Fill between the curve and the centre line first, so the curve itself is
    // drawn over it rather than the fill hiding it. A curve with the area under
    // it shaded is a shape; the same curve as a bare line is a scribble, and
    // the shaded part is what shows at a glance how much travel is being thrown
    // away to the dead zone.
    if points.len() > 1 {
        let mut area = points.clone();
        area.push(at(points[points.len() - 1].x, 0.0));
        area.push(at(points[0].x, 0.0));
        painter.add(egui::Shape::convex_polygon(
            area,
            ACCENT.gamma_multiply(0.10),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        points,
        egui::epaint::PathStroke::new(2.5, ACCENT),
    ));

    // Where the pad actually is right now, so the trace and the live reading can
    // be checked against each other instead of taken on trust.
    let mut filter = settings.to_filter();
    let shaped =
        filter.apply_with_formula(live.clamp(-1.0, 1.0), formula.as_ref(), &Inputs::default());
    let dot = at(live.clamp(-1.0, 1.0), shaped.clamp(-1.0, 1.0));
    glow(&painter, dot, 16.0, ACCENT);
    painter.circle_filled(dot, 3.5, TEXT);
}

/// A steering wheel, turned to wherever the stick currently is.
///
/// Drawn rather than reduced to a number because the thing being tuned on the
/// racing page is a feel, and the fastest way to check a feel is to watch the
/// wheel return to centre the way a car would.
pub fn steering_wheel(ui: &mut egui::Ui, angle: f32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let r = (size * 0.5 - 6.0).max(10.0);
    // A stick reaches full lock at ninety degrees either way, so the whole
    // travel is half a turn. That is what makes a quick correction look quick.
    let a = angle.clamp(-1.0, 1.0).to_radians() * 90.0;
    let (sin, cos) = (a.sin(), a.cos());
    // Rotate a point from wheel space into screen space.
    let at = |x: f32, y: f32| Pos2::new(c.x + x * cos - y * sin, c.y + x * sin + y * cos);

    glow(&painter, c, r * 1.6, ACCENT);

    // The rim is a bar rather than a ring: a ring looks the same whichever way
    // it is turned, which is the one thing this has to show.
    //
    // Drawn as a stack of three strokes rather than one. A single flat stroke
    // is a line, and a line is not an object. The dark underlay and the lit
    // upper edge are what make it read as a rim with a round section catching
    // light from above -- the same cue as the controller shell's gradient.
    painter.line_segment([at(-r, 0.0), at(r, 0.0)], Stroke::new(12.0, SHELL_SHADE));
    painter.line_segment([at(-r, 0.0), at(r, 0.0)], Stroke::new(8.5, SHELL));
    painter.line_segment(
        [at(-r * 0.94, -2.0), at(r * 0.94, -2.0)],
        Stroke::new(2.0, SHELL_EDGE),
    );

    // Grip pads where the hands go, in accent so the rim reads as a wheel at
    // a glance rather than as a plain line.
    for sign in [-1.0f32, 1.0] {
        painter.line_segment(
            [at(sign * r * 0.68, -3.0), at(sign * r * 0.99, -3.0)],
            Stroke::new(6.0, ACCENT.gamma_multiply(0.6)),
        );
    }

    // Spokes: a dark underlay and a lit edge, like the rim, so the whole wheel
    // is built from one rule rather than each part having its own.
    for (from, to) in [
        ((-r * 0.66, 0.0), (-r * 0.26, r * 0.66)),
        ((r * 0.66, 0.0), (r * 0.26, r * 0.66)),
    ] {
        painter.line_segment(
            [at(from.0, from.1), at(to.0, to.1)],
            Stroke::new(5.0, SHELL_SHADE),
        );
        painter.line_segment(
            [at(from.0, from.1), at(to.0, to.1)],
            Stroke::new(2.5, SHELL_EDGE),
        );
    }

    // Hub, shaded the same way, with an accent pin at its centre.
    painter.circle_filled(c, r * 0.27, SHELL_SHADE);
    painter.circle_filled(c, r * 0.23, SHELL);
    painter.circle_stroke(c, r * 0.23, Stroke::new(1.5, ACCENT.gamma_multiply(0.75)));
    painter.circle_filled(c, r * 0.07, ACCENT);

    // Straight ahead, marked, so centre is a position rather than a guess.
    painter.line_segment([at(0.0, -r), at(0.0, -r + 7.0)], Stroke::new(3.0, ACCENT));
    painter.circle_filled(at(0.0, -r + 3.0), 2.5, ACCENT);
}

/// A preset chip: one shape, one click, rounded like the status pills so the
/// racing page does not introduce a shape the rest of the app never uses.
pub fn preset_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(TEXT).size(13.0))
            .fill(SURFACE_HOVER)
            .stroke(Stroke::new(1.0, ACCENT_DIM))
            .corner_radius(RADIUS_PILL)
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

/// An on/off switch, drawn rather than a ticked box.
///
/// For booleans that are *settings* — intended to persist and be reflected
/// elsewhere, which a ticked box does not say as directly as a pill sitting on
/// one of two states. The dense inline option rows on the racing and controller
/// pages keep their checkboxes deliberately: those read as compact options, and
/// a switch of this width inside them would push everything else out.
///
/// Click and keyboard (Space while focused) both flip it, like egui's checkbox.
///
/// `Sense::click`, not a bare `hover`: in egui 0.36 the click sense already
/// carries focusability, so keyboard activation needs no separate
/// constructor, and the response reports `clicked()` exactly like a button.
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());

    let clicked = response.clicked()
        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space)));
    if clicked {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *on,
            if *on { "on" } else { "off" },
        )
    });

    let painter = ui.painter_at(rect);

    // Track: recessed and dark when off, accent when on, a shade lighter under
    // the pointer so it never needs guessing that it is a control. The stroke
    // is what keeps the off state from melting into the card it sits on —
    // `TRACK` under `SURFACE_RAISED` is the groove pairing from the sliders.
    let track = if *on {
        ACCENT
    } else if response.hovered() {
        SURFACE_HOVER
    } else {
        TRACK
    };
    painter.rect_filled(rect, RADIUS_PILL, track);
    painter.rect_stroke(
        rect,
        RADIUS_PILL,
        Stroke::new(1.0, if *on { ACCENT } else { BORDER }),
        egui::StrokeKind::Middle,
    );

    // Knob at whichever end of its travel the state names.
    let inset = size.y * 0.14;
    let knob_r = size.y * 0.5 - inset;
    let knob_pos = if *on {
        Pos2::new(rect.right() - inset - knob_r, rect.center().y)
    } else {
        Pos2::new(rect.left() + inset + knob_r, rect.center().y)
    };
    if *on {
        glow(&painter, knob_pos, knob_r * 2.0, ACCENT);
    }
    // Dark on the accent when on; light when off, on the dark track. Either
    // way the knob is the highest-contrast thing in the widget, which is what
    // makes the state readable before the shape is.
    painter.circle_filled(knob_pos, knob_r, if *on { BACKDROP } else { TEXT });
    painter.circle_stroke(
        knob_pos,
        knob_r,
        Stroke::new(1.0, if *on { BACKDROP } else { BORDER }),
    );

    response
}

/// The header's pause / resume control: an icon, not "||||" typed into a
/// string.
///
/// The previous version set the Button's text to four vertical bars or a
/// greater-than, which is exactly what it looks like too — throughput dragged
/// through a font glyph set that has none of the shapes this button means.
/// Drawn here, the two states are distinguishable at a glance and the colour
/// logic stays in one place.
pub fn pause_play(ui: &mut egui::Ui, paused: bool) -> egui::Response {
    let size = Vec2::new(34.0, 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());

    let toggled = response.clicked()
        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space)));
    if toggled {
        // The caller decides what "toggled" means; we only pass on that a
        // deliberate activation happened. Mirroring the contract of a plain
        // Button's `clicked()` keeps the call site unchanged in shape.
        response.mark_changed();
    }

    let painter = ui.painter_at(rect);

    // Paused reads as a warning state (the pad is not passing through), which
    // is the colour rule the text version already had.
    let (fill, edge, icon) = if paused {
        (WARNING, WARNING, BACKDROP)
    } else if response.hovered() {
        (SURFACE_HOVER, ACCENT_DIM, TEXT)
    } else {
        (SURFACE_RAISED, BORDER, TEXT)
    };
    painter.rect_filled(rect, RADIUS_MD, fill);
    painter.rect_stroke(
        rect,
        RADIUS_MD,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Middle,
    );

    let c = rect.center();
    // A hair bigger under the pointer: live feedback that costs nothing.
    let slack = if response.hovered() { 0.6 } else { 0.0 };

    if paused {
        // Play: one triangle, filled, pointing right — the only glyph a
        // triangle is unambiguous as. Slightly taller than wide, as optically
        // corrective rather than geometrically exact.
        let w = 9.0;
        let h = 11.0;
        let points = vec![
            Pos2::new(c.x - w * 0.4, c.y - h * 0.62),
            Pos2::new(c.x - w * 0.4, c.y + h * 0.62),
            Pos2::new(c.x + w * 0.64, c.y),
        ];
        painter.add(egui::Shape::convex_polygon(points, icon, Stroke::NONE));
    } else {
        // Pause: two bars.
        let bar = Vec2::new(3.2, 11.0 + slack);
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(c.x - 3.4, c.y), bar),
            RADIUS_SM,
            icon,
        );
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(c.x + 3.4, c.y), bar),
            RADIUS_SM,
            icon,
        );
    }

    response
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
        // Held down anywhere on the strip, whether or not the pointer has
        // moved since it landed — because `clicked` alone fires on release, a
        // press-and-sweep did nothing until the finger came back up, and
        // releasing inside a two-pixel strip is not how anyone picks a hue.
        // The same rule governs the square below.
        let pressing = response.is_pointer_button_down_on();
        if pressing {
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
        if sq_resp.is_pointer_button_down_on() {
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

        // Swatch preview next to its hex readout, on one row. The swatch used
        // to sit alone on a line of its own above the number, which read as
        // two unrelated facts rather than one colour and its name.
        ui.horizontal(|ui| {
            let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
            ui.painter_at(swatch_rect).rect_filled(
                swatch_rect,
                RADIUS_SM,
                Color32::from_rgb(current[0], current[1], current[2]),
            );
            ui.painter_at(swatch_rect).rect_stroke(
                swatch_rect,
                RADIUS_SM,
                Stroke::new(1.0, BORDER),
                egui::StrokeKind::Middle,
            );
            ui.label(
                egui::RichText::new(format!(
                    "#{:02X}{:02X}{:02X}",
                    current[0], current[1], current[2]
                ))
                .color(TEXT_MUTED)
                .monospace(),
            );
        });
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
    use egui::{pos2, vec2, RawInput, Rect};

    /// One headless egui pass. The font-atlas delta each pass produces has no
    /// renderer to receive it here, so it is cleared before the pass is
    /// dropped — epaint asserts on unapplied deltas and the assert is right.
    fn pass(ctx: &egui::Context, input: &RawInput, draw: impl FnMut(&mut egui::Ui)) {
        let mut out = ctx.run_ui(input.clone(), draw);
        out.textures_delta.clear();
    }

    fn input(size: egui::Vec2, events: Vec<egui::Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
            events,
            ..RawInput::default()
        }
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

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

    /// A press-and-sweep on the picker must move the colour as the pointer
    /// moves, not only when the finger comes back up — and a release that ends
    /// off the strip must not undo the sweep that happened under it.
    ///
    /// The first version of the picker answered `clicked` only. `clicked`
    /// fires on release, so the natural gesture (press, sweep, let go) applied
    /// the hue from the *release point, if the release landed inside the strip;
    /// a release that drifted even a pixel outside it applied nothing at all.
    /// A click at 15% of the hue strip, a sweep to 85%, and a release outside
    /// the strip says which behaviour is running: the value here ends at 85%
    /// now, and used to end at 0 (the starting hue, red), because neither the
    /// press nor the sweep had counted.
    #[test]
    fn dragging_the_picker_moves_the_colour_even_off_the_strip() {
        let ctx = egui::Context::default();
        let hsv = std::cell::Cell::new([0.0f32, 1.0, 1.0]);
        let strip = std::cell::Cell::new(Rect::NOTHING);

        let size = vec2(240.0, 300.0);
        // Warm the layout so the strip's position is stable, then ask again
        // for the same geometry and note where the strip sits.
        pass(&ctx, &input(size, vec![]), |ui| {
            let top = ui.cursor().top();
            let left = ui.min_rect().left();
            let w = ui.available_width();
            strip.set(Rect::from_min_size(pos2(left, top), vec2(w, 18.0)));
            let mut picker = ColourPicker { hsv: hsv.get() };
            picker.show(ui);
            hsv.set(picker.hsv);
        });
        pass(&ctx, &input(size, vec![]), |ui| {
            let mut picker = ColourPicker { hsv: hsv.get() };
            picker.show(ui);
            hsv.set(picker.hsv);
        });

        let s = strip.get();
        assert!(
            s.height() >= 17.0,
            "the hue strip is where it is drawn, got {s:?}"
        );

        // A real drag is not one event: the press settles in one pass, the
        // sweep is felt while the button is still down, and the release comes
        // later — possibly outside the strip. Committed as separate passes
        // because a single pass containing press+move+release hands egui a
        // pointer that is already up by the time anything renders, which is
        // not how a human drags.
        //
        // The middle pass is the one that counts: the hue must follow the
        // sweep *while the pointer is down*. The first version of the picker
        // only answered `clicked`, which fires on release — so nothing here
        // would move it, and the release below, landing outside the strip,
        // would not either. The value would have stayed at the starting red.
        let down = pos2(s.left() + s.width() * 0.15, s.center().y);
        let sweep = pos2(s.left() + s.width() * 0.85, s.center().y);
        let release = pos2(s.center().x, s.bottom() + 80.0);
        let press_and_sweep = vec![
            egui::Event::PointerButton {
                pos: down,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(sweep),
        ];
        pass(&ctx, &input(size, press_and_sweep), |ui| {
            let mut picker = ColourPicker { hsv: hsv.get() };
            picker.show(ui);
            hsv.set(picker.hsv);
        });

        let [h, _s, _v] = hsv.get();
        assert!(
            (h - 0.85).abs() < 0.05,
            "a press-and-sweep left the hue at {h:.3}, expected about 0.85"
        );

        let release_events = vec![egui::Event::PointerButton {
            pos: release,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }];
        pass(&ctx, &input(size, release_events), |ui| {
            let mut picker = ColourPicker { hsv: hsv.get() };
            picker.show(ui);
            hsv.set(picker.hsv);
        });
        let [h2, _s2, _v2] = hsv.get();
        assert!(
            (h2 - 0.85).abs() < 0.05,
            "a release outside the strip undid the sweep: hue is {h2:.3}"
        );
    }

    /// Click on the switch flips it, a click elsewhere does not. The second
    /// half is what the previous checkbox-free layout could not say: that a
    /// row's label click does not reach its neighbour's control.
    #[test]
    fn a_click_flips_the_switch_and_a_miss_does_not() {
        let ctx = egui::Context::default();
        let on = std::cell::Cell::new(false);
        let centre = std::cell::Cell::new(pos2(-1.0, -1.0));

        for _ in 0..2 {
            pass(&ctx, &input(vec2(200.0, 80.0), vec![]), |ui| {
                let mut v = on.get();
                let r = toggle_switch(ui, &mut v);
                on.set(v);
                centre.set(r.rect.center());
            });
        }

        assert!(!on.get(), "drawing alone must not toggle the switch");
        pass(&ctx, &input(vec2(200.0, 80.0), click(centre.get())), |ui| {
            let mut v = on.get();
            toggle_switch(ui, &mut v);
            on.set(v);
        });
        assert!(on.get(), "clicking the switch toggles it");

        pass(
            &ctx,
            &input(vec2(200.0, 80.0), click(pos2(180.0, 60.0))),
            |ui| {
                let mut v = on.get();
                toggle_switch(ui, &mut v);
                on.set(v);
            },
        );
        assert!(on.get(), "a click that lands elsewhere must not toggle it");
    }

    /// The header control only has to report that it was activated; the app
    /// decides what pause means. Expressed this way, the icon redraw and the
    /// semantics stay testable without a window.
    #[test]
    fn the_pause_control_reports_activation_on_click() {
        let ctx = egui::Context::default();
        let activated = std::cell::Cell::new(false);
        let centre = std::cell::Cell::new(pos2(-1.0, -1.0));

        for _ in 0..2 {
            pass(&ctx, &input(vec2(200.0, 80.0), vec![]), |ui| {
                let r = pause_play(ui, false);
                centre.set(r.rect.center());
                activated.set(r.changed());
            });
        }
        assert!(
            !activated.get(),
            "drawing the control alone must not activate it"
        );

        pass(&ctx, &input(vec2(200.0, 80.0), click(centre.get())), |ui| {
            let r = pause_play(ui, false);
            activated.set(r.changed());
        });
        assert!(activated.get(), "clicking the control activates it");
    }
}
