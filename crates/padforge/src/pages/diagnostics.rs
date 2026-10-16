//! Input diagnostics: what the pad sent, next to what that was read as.
//!
//! Every decoding bug found on real hardware had the same shape. The bytes were
//! fine. The field that mattered was read from the wrong offset. And nothing
//! anywhere disagreed: the app drew a stick that moved when you moved it, the
//! numbers looked plausible, and the only way to tell the gyro from the
//! accelerometer was to leave the pad on a desk and notice that a stationary
//! object cannot read 0.003 g.
//!
//! A page that shows both halves side by side turns that from "the motion
//! aiming feels wrong" into a visible disagreement between two rows, which is
//! the only kind of bug that can be diagnosed from somebody else's description
//! of it.

use crate::theme::*;
use egui::{Color32, RichText};

use padcore::engine::{EngineCommand, Telemetry};
use padcore::report::{Buttons, Ds4Report};

/// Which transport's layout to annotate against.
///
/// The two are genuinely different documents — the Bluetooth report starts one
/// byte later — so an annotation that is right for one and wrong for the other
/// is worse than none. The page reads this from what is actually connected and
/// says which it used.
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();

    ui.add_space(SPACE_SM);
    ui.label(
        RichText::new(heading_text(&telemetry))
            .color(TEXT)
            .size(17.0)
            .strong(),
    );
    ui.label(
        RichText::new(
            "Every decoding mistake looks like this page: the bytes are right and \
             the field is read from the wrong place. Move a control and watch its \
             byte change.",
        )
        .color(TEXT_FAINT)
        .size(12.5),
    );
    ui.add_space(SPACE_MD);

    if !telemetry.connected || telemetry.raw.current.is_empty() {
        card_quiet().show(ui, |ui| {
            ui.label(
                RichText::new(if telemetry.connected {
                    "Connected, but no report has arrived yet."
                } else {
                    "No controller is connected."
                })
                .color(TEXT_MUTED)
                .size(12.5),
            );
        });
        return;
    }

    controls(ui, ctx, &telemetry);
    ui.add_space(SPACE_MD);

    // The hex dump and the decoded fields, side by side. That adjacency is the
    // whole point of the page.
    let free = ui.available_width() - ui.spacing().item_spacing.x;
    ui.columns(2, |cols| {
        raw_dump(&mut cols[0], &telemetry);
        decoded(&mut cols[1], &telemetry);
    });
    let _ = free;

    ui.add_space(SPACE_MD);
    layout_note(ui, &telemetry);
}

fn heading_text(t: &Telemetry) -> &'static str {
    match t.transport {
        Some(padcore::report::Transport::Bluetooth) => "Input diagnostics - Bluetooth",
        Some(padcore::report::Transport::Usb) => "Input diagnostics - USB",
        None => "Input diagnostics",
    }
}

fn controls(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &Telemetry) {
    card_quiet().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} reports", t.raw.count))
                    .color(TEXT)
                    .size(12.5)
                    .monospace(),
            );
            if let Some(tr) = t.transport {
                pill(ui, tr.label(), TEXT_MUTED);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if t.raw.is_frozen {
                    if secondary_button(ui, "Live").clicked() {
                        ctx.engine.send(EngineCommand::SetRawFrozen(false));
                    }
                } else if secondary_button(ui, "Freeze").clicked() {
                    // Frozen because the live buffer changes at 755 Hz, which is
                    // faster than a person can read a number off a screen.
                    ctx.engine.send(EngineCommand::SetRawFrozen(true));
                }
            });
        });

        if t.raw.is_frozen {
            ui.label(
                RichText::new("Held still. Move a control and watch the live report instead.")
                    .color(ACCENT)
                    .size(11.5),
            );
        }
    });
}

/// The bytes, 16 to a line, with the offsets a reader will need to compare them
/// against a driver document.
fn raw_dump(ui: &mut egui::Ui, t: &Telemetry) {
    let bytes: &[u8] = if t.raw.is_frozen {
        t.raw.frozen.as_deref().unwrap_or(&[])
    } else {
        &t.raw.current
    };

    card().show(ui, |ui| {
        heading(ui, "RAW REPORT");
        ui.add_space(SPACE_SM);

        if bytes.is_empty() {
            ui.label(
                RichText::new("nothing captured")
                    .color(TEXT_FAINT)
                    .size(12.0),
            );
            return;
        }

        let mono = |s: String, c: Color32| RichText::new(s).color(c).size(11.0).monospace();

        // The offsets live in the left column, which is the half a reader looks
        // at when comparing against documentation, and the bytes in the right.
        egui::Grid::new("hexdump")
            .num_columns(2)
            .spacing([SPACE_MD, 1.0])
            .show(ui, |ui| {
                for (row, chunk) in bytes.chunks(16).enumerate() {
                    let offset = row * 16;
                    ui.label(mono(format!("{offset:04x}"), TEXT_FAINT));

                    let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                    ui.label(mono(hex.join(" "), TEXT));

                    ui.end_row();
                }
            });

        ui.add_space(SPACE_SM);
        if let Some(id) = bytes.first() {
            ui.label(
                RichText::new(format!(
                    "report id 0x{id:02x}  ({})  ·  {} bytes",
                    report_id_name(*id),
                    bytes.len()
                ))
                .color(TEXT_MUTED)
                .size(11.5),
            );
        }
    });
}

fn report_id_name(id: u8) -> &'static str {
    match id {
        0x01 => "USB input report",
        0x11 => "Bluetooth input report",
        _ => "not a report PadForge knows",
    }
}

/// The decoded fields, each showing the byte it came from next to the value it
/// became.
///
/// The column of offsets is not decoration: "gyro pitch reads 0.31 rad/s" is
/// useless on its own, and "gyro pitch comes from bytes 14-15" is a claim that
/// can be checked against the bytes on the left.
fn decoded(ui: &mut egui::Ui, t: &Telemetry) {
    let r: &Ds4Report = &t.report;

    card().show(ui, |ui| {
        heading(ui, "DECODED");
        ui.add_space(SPACE_SM);

        let o = offsets(t.transport);

        egui::Grid::new("decoded")
            .num_columns(3)
            .spacing([SPACE_MD, 2.0])
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, name: &str, at: String, value: String| {
                    ui.label(RichText::new(name).color(TEXT_MUTED).size(11.5));
                    ui.label(RichText::new(at).color(TEXT_FAINT).size(11.0).monospace());
                    ui.label(RichText::new(value).color(TEXT).size(11.5).monospace());
                    ui.end_row();
                };

                row(ui, "left stick x", hex_at(o.left_x), f3(r.left_x));
                row(ui, "left stick y", hex_at(o.left_y), f3(r.left_y));
                row(ui, "right stick x", hex_at(o.right_x), f3(r.right_x));
                row(ui, "right stick y", hex_at(o.right_y), f3(r.right_y));
                row(ui, "hat / face", hex_at(o.hat_and_face), hat_and_face(r));
                row(ui, "shoulders", hex_at(o.shoulders), shoulders(r));
                row(
                    ui,
                    "flags + counter",
                    hex_at(o.flags),
                    format!("0x{:04x}", r.buttons.raw()),
                );
                row(ui, "L2", hex_at(o.l2), f3(r.l2));
                row(ui, "R2", hex_at(o.r2), f3(r.r2));
                row(
                    ui,
                    "gyro yaw",
                    hex_at(o.gyro),
                    format!("{:.3} rad/s", r.gyro.yaw),
                );
                row(
                    ui,
                    "gyro pitch",
                    hex_at(o.gyro + 2),
                    format!("{:.3} rad/s", r.gyro.pitch),
                );
                row(
                    ui,
                    "gyro roll",
                    hex_at(o.gyro + 4),
                    format!("{:.3} rad/s", r.gyro.roll),
                );
                row(ui, "accel", hex_at(o.accel), format!("{:.3} g", accel_g(r)));
                row(
                    ui,
                    "battery",
                    hex_at(o.battery),
                    format!("{:.0}%", r.battery.fraction() * 100.0),
                );
                row(
                    ui,
                    "touchpad",
                    hex_at(o.touch),
                    if r.touch.pad_touched {
                        "touched"
                    } else {
                        "clear"
                    }
                    .to_string(),
                );
            });

        // The one reading that is worth calling out rather than listing: a pad
        // lying still reads 1 g, not 0. A stationary object cannot read 0.003 g,
        // so a small number here means the axes are swapped.
        if accel_g(r) > 0.05 && accel_g(r) < 0.5 {
            ui.add_space(SPACE_SM);
            ui.label(
                RichText::new(
                    "The accelerometer is reading near zero. A pad on a desk reads about \
                     1 g, so this usually means the gyro and accelerometer are swapped \
                     rather than that the pad is falling.",
                )
                .color(WARNING)
                .size(11.5),
            );
        }
    });
}

/// Byte offset at which the analog block starts, for this transport.
///
/// Delegates to [`padcore::report::FieldOffsets`], which is the same table the
/// decoder reads. The first version of this page hard-coded 3 and 4; the real
/// values are 1 and 3, and a test comparing the two caught it — which is the
/// whole reason the page exists, and the reason it must not carry its own copy
/// of the numbers.
fn offsets(transport: Option<padcore::report::Transport>) -> padcore::report::FieldOffsets {
    padcore::report::FieldOffsets::for_transport(transport)
}

fn hex_at(offset: usize) -> String {
    format!("@{offset:02x}")
}

/// Magnitude of the accelerometer vector, in g.
///
/// Shown as a number because the number is the diagnosis: a pad lying on a desk
/// reads about 1 g, so a reading near zero means the axes are swapped rather
/// than that the pad is in free fall.
fn accel_g(r: &Ds4Report) -> f32 {
    let a = r.accel;
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn f3(v: f32) -> String {
    format!("{v:+.3}")
}

/// The face buttons and the d-pad share one byte, so both have to be named or
/// the row is unreadable.
fn hat_and_face(r: &Ds4Report) -> String {
    let b = r.buttons;
    let mut on: Vec<&str> = Vec::new();
    for (mask, name) in [
        (Buttons::TRIANGLE, "triangle"),
        (Buttons::CIRCLE, "circle"),
        (Buttons::CROSS, "cross"),
        (Buttons::SQUARE, "square"),
    ] {
        if b.any(mask) {
            on.push(name);
        }
    }
    for (mask, name) in [
        (Buttons::UP, "hat up"),
        (Buttons::RIGHT, "hat right"),
        (Buttons::DOWN, "hat down"),
        (Buttons::LEFT, "hat left"),
    ] {
        if b.any(mask) {
            on.push(name);
        }
    }
    if on.is_empty() {
        "-".into()
    } else {
        on.join(" + ")
    }
}

fn shoulders(r: &Ds4Report) -> String {
    let b = r.buttons;
    let mut on: Vec<&str> = Vec::new();
    for (mask, name) in [
        (Buttons::L1, "L1"),
        (Buttons::R1, "R1"),
        (Buttons::L2, "L2"),
        (Buttons::R2, "R2"),
        (Buttons::SHARE, "share"),
        (Buttons::OPTIONS, "options"),
        (Buttons::L3, "L3"),
        (Buttons::R3, "R3"),
    ] {
        if b.any(mask) {
            on.push(name);
        }
    }
    if on.is_empty() {
        "-".into()
    } else {
        on.join(" ")
    }
}

/// The layout, stated rather than assumed.
///
/// A page that shows offsets without saying which layout it is using is a page
/// that will be misread the first time somebody connects the other transport.
fn layout_note(ui: &mut egui::Ui, t: &Telemetry) {
    let transport = t.transport.map(|x| x.label()).unwrap_or("none");
    let o = offsets(t.transport);

    card_quiet().show(ui, |ui| {
        heading(ui, "HOW THIS IS READ");
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(format!(
                "Report id 0x{:02x}, analog block starts at byte {}. \
                 Length is deliberately ignored: Windows pads a read out to the \
                 device's declared input report length, so a Bluetooth payload \
                 arrives in a 128-byte buffer whatever its real size, and a \
                 length check picks the wrong layout for whichever transport has \
                 the longer declaration.",
                t.raw.current.first().copied().unwrap_or(0),
                o.left_x
            ))
            .color(TEXT_MUTED)
            .size(11.5),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(format!(
                "Currently connected over {}. The two layouts are different \
                 documents: the Bluetooth report starts one byte later, which is \
                 why the offsets above move when the connection changes.",
                transport
            ))
            .color(TEXT_FAINT)
            .size(11.5),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets this page prints must be the offsets the decoder reads.
    ///
    /// Not a tautology: this is the page's first version of this test, and it
    /// failed. The page hard-coded 3 and 4 for where the analog block starts;
    /// the decoder uses 1 and 3. So the page was printing offsets that belonged
    /// to no report at all — which is exactly the bug class it exists to expose,
    /// committed by the page itself.
    ///
    /// It now reads `FieldOffsets`, which the decoder also reads, and this test
    /// is what keeps that true.
    #[test]
    fn the_offset_this_page_prints_is_the_one_the_decoder_reads() {
        for (transport, report_id) in [
            (padcore::report::Transport::Usb, 0x01u8),
            (padcore::report::Transport::Bluetooth, 0x11u8),
        ] {
            let o = offsets(Some(transport));

            // A distinct, asymmetric value per axis, so reading the wrong one
            // cannot look like reading the right one.
            let mut buf = vec![0u8; 64];
            buf[0] = report_id;
            buf[o.left_x] = 10;
            buf[o.left_y] = 20;
            buf[o.right_x] = 30;
            buf[o.right_y] = 40;
            buf[o.l2] = 200;
            buf[o.r2] = 220;

            let r = padcore::report::parse(&buf).expect("should decode");

            assert_eq!(r.raw_left_x, 10, "{transport:?}: left x");
            assert_eq!(r.raw_left_y, 20, "{transport:?}: left y");
            assert_eq!(r.raw_right_x, 30, "{transport:?}: right x");
            assert_eq!(r.raw_right_y, 40, "{transport:?}: right y");
            assert!((r.l2 - 200.0 / 255.0).abs() < 0.01, "{transport:?}: L2");
            assert!((r.r2 - 220.0 / 255.0).abs() < 0.01, "{transport:?}: R2");
        }
    }

    #[test]
    fn the_two_transports_really_do_start_two_bytes_apart() {
        // Not one, as this page first claimed. Over Bluetooth the pad sends a
        // report id and a CRC/flags byte before the same field block.
        let usb = offsets(Some(padcore::report::Transport::Usb));
        let bt = offsets(Some(padcore::report::Transport::Bluetooth));
        assert_eq!(usb.left_x, 1);
        assert_eq!(bt.left_x, 3);
        assert_eq!(bt.left_x - usb.left_x, 2);
    }

    #[test]
    fn both_report_ids_are_named_and_anything_else_is_not_claimed() {
        assert_eq!(report_id_name(0x01), "USB input report");
        assert_eq!(report_id_name(0x11), "Bluetooth input report");
        assert_eq!(report_id_name(0x05), "not a report PadForge knows");
    }

    #[test]
    fn a_decoder_that_only_knows_one_transport_is_the_bug_this_page_catches() {
        // Found on real hardware: only 0x01 was accepted, so every Bluetooth
        // frame was discarded and a connected pad presented as one that never
        // reports. Kept because the page is built on the assumption that both
        // decode, and that assumption should be checked.
        for id in [0x01u8, 0x11u8] {
            let mut buf = vec![0u8; 64];
            buf[0] = id;
            assert!(
                padcore::report::parse(&buf).is_some(),
                "report id {id:#04x} must decode"
            );
        }
    }

    #[test]
    fn a_button_row_names_everything_that_is_down() {
        // Through a real report rather than by constructing the bitmask: this
        // way the test also proves the decoder puts a face button and an L2
        // press where the row reader expects to find them.
        let o = offsets(Some(padcore::report::Transport::Usb));
        let mut buf = vec![0u8; 64];
        buf[0] = 0x01;
        buf[o.hat_and_face] = 0x20; // cross, in the high nibble
                                    // Bit 2, not bit 6. The shoulder byte runs L1, R1, L2, R2, share,
                                    // options, L3, R3 from bit 0 up — and bits 2 and 3 used to be decoded
                                    // as R3 and L3, so pressing a trigger reported a thumbstick click to
                                    // the game while clicking either thumbstick did nothing.
        buf[o.shoulders] = 1 << 2;
        let r = padcore::report::parse(&buf).expect("should decode");

        let row = hat_and_face(&r);
        assert!(row.contains("cross"), "{row}");
        assert!(!row.contains("circle"), "{row}");

        let shoulders = shoulders(&r);
        assert!(shoulders.contains("L2"), "{shoulders}");
        assert!(!shoulders.contains("R2"), "{shoulders}");
    }
}
