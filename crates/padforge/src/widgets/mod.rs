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

        // Lightbar glow first, so it sits behind the body.
        let bar_center = Pos2::new(cx, body_rect.min.y);
        glow(&painter, bar_center, h * 0.26, self.lightbar);

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
        for grip in grips {
            painter.circle_stroke(grip, grip_r, Stroke::new(1.5, TEXT_FAINT));
        }
        painter.rect_stroke(
            body_rect,
            RADIUS_LG,
            Stroke::new(1.5, TEXT_FAINT),
            egui::StrokeKind::Middle,
        );
        for grip in grips {
            painter.circle_filled(grip, grip_r, SURFACE);
        }
        painter.rect_filled(body_rect, RADIUS_LG, SURFACE);

        // --- lightbar --------------------------------------------------
        let bar_w = body_w * 0.26;
        let bar_h = (4.0 * scale).max(3.0);
        let bar_rect = Rect::from_min_max(
            Pos2::new(cx - bar_w / 2.0, bar_center.y - bar_h),
            Pos2::new(cx + bar_w / 2.0, bar_center.y + bar_h * 0.5),
        );
        painter.rect_filled(bar_rect, RADIUS_PILL, self.lightbar);

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
    painter.rect_filled(rect, RADIUS_SM, BACKDROP);

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
    painter.add(egui::Shape::line(
        points,
        egui::epaint::PathStroke::new(2.0, ACCENT),
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
    painter.circle_filled(c, r, BACKDROP);
    painter.circle_stroke(c, r, Stroke::new(2.5, ACCENT.gamma_multiply(0.7)));

    // The rim is a bar rather than a ring: a ring looks the same whichever way
    // it is turned, which is the one thing this has to show.
    painter.line_segment([at(-r, 0.0), at(r, 0.0)], Stroke::new(5.0, TEXT_MUTED));
    painter.line_segment(
        [at(-r * 0.62, 0.0), at(-r * 0.2, r * 0.55)],
        Stroke::new(2.5, TEXT_FAINT),
    );
    painter.line_segment(
        [at(r * 0.62, 0.0), at(r * 0.2, r * 0.55)],
        Stroke::new(2.5, TEXT_FAINT),
    );
    painter.circle_filled(c, r * 0.22, SURFACE_HOVER);
    painter.circle_stroke(c, r * 0.22, Stroke::new(1.5, ACCENT));

    // Straight ahead, marked, so centre is a position rather than a guess.
    painter.line_segment([at(0.0, -r), at(0.0, -r + 7.0)], Stroke::new(3.0, ACCENT));
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
