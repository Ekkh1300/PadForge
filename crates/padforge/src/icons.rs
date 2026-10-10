//! Icons, generated at runtime rather than shipped as image files.
//!
//! A tray icon and a window icon are tiny, and generating them keeps the binary
//! free of binary assets while staying crisp at any size.

/// The PadForge mark: two interlocking rings on a dark rounded square.
fn mark(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let cx = s / 2.0;
    let cy = s / 2.0;

    // Background: a rounded square, slightly lighter than pure black so it reads
    // against a dark taskbar.
    let radius = s * 0.22;
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            if rounded_contains(fx, fy, s, radius) {
                let i = ((y * size + x) * 4) as usize;
                px[i] = 0x12;
                px[i + 1] = 0x15;
                px[i + 2] = 0x1B;
                px[i + 3] = 0xFF;
            }
        }
    }

    // Two rings: an outer one for the pad, an inner one for the bridge. The
    // rings are clamped to the background, so they never spill past the edges.
    let ring_outer = s * 0.30;
    let ring_inner = s * 0.14;
    let thickness = (s * 0.055).max(1.0);
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            if !rounded_contains(fx, fy, s, radius) {
                continue;
            }
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();

            let on_outer = (d - ring_outer).abs() <= thickness / 2.0;
            let on_inner = (d - ring_inner).abs() <= thickness / 2.0;
            if on_outer || on_inner {
                let i = ((y * size + x) * 4) as usize;
                px[i] = 0x35;
                px[i + 1] = 0xD0;
                px[i + 2] = 0xE8;
                px[i + 3] = 0xFF;
            }
        }
    }

    px
}

/// Is this point inside a rounded square of side `s` with corner `radius`?
fn rounded_contains(x: f32, y: f32, s: f32, radius: f32) -> bool {
    let inset = s * 0.04;
    let min = inset;
    let max = s - inset;
    if x < min || x > max || y < min || y > max {
        return false;
    }
    // Only the four corners need checking.
    let cx = if x < min + radius {
        min + radius
    } else if x > max - radius {
        max - radius
    } else {
        x
    };
    let cy = if y < min + radius {
        min + radius
    } else if y > max - radius {
        max - radius
    } else {
        y
    };
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

/// 64×64 RGBA pixels for the window icon.
pub fn app_icon_rgba() -> Vec<u8> {
    mark(64)
}

/// Tray icon pixels. Windows renders tray icons at 16 or 32, so produce a square
/// image and let the shell downscale.
pub fn tray_icon_rgba(size: u32) -> Vec<u8> {
    mark(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_have_a_valid_pixel_buffer() {
        let rgba = app_icon_rgba();
        assert_eq!(rgba.len(), 64 * 64 * 4);
        // The corners are cut away by the rounded square, so transparency there
        // is expected; what matters is that *some* pixels are drawn.
        let opaque = rgba.chunks_exact(4).filter(|p| p[3] == 0xFF).count();
        assert!(
            opaque > 64 * 64 / 2,
            "icon is mostly transparent: {opaque} px"
        );
        // And that nothing is drawn outside the rounded square's alpha range.
        assert!(rgba.chunks_exact(4).all(|p| p[3] == 0 || p[3] == 0xFF));
    }

    #[test]
    fn tray_icon_scales() {
        for size in [16u32, 32, 48] {
            assert_eq!(tray_icon_rgba(size).len(), (size * size * 4) as usize);
        }
    }

    #[test]
    fn rounded_square_rejects_corners() {
        let s = 100.0;
        // Just inside the middle of the shape.
        assert!(rounded_contains(50.0, 50.0, s, 20.0));
        // The far corner is outside.
        assert!(!rounded_contains(0.5, 0.5, s, 20.0));
        // Outside the inset edge.
        assert!(!rounded_contains(0.1, 50.0, s, 20.0));
    }

    #[test]
    fn accent_pixels_exist() {
        // The rings must actually be drawn, otherwise the icon is a blank square.
        let rgba = app_icon_rgba();
        let accents = rgba
            .chunks_exact(4)
            .filter(|p| p[0] == 0x35 && p[1] == 0xD0 && p[2] == 0xE8)
            .count();
        assert!(
            accents > 100,
            "expected the rings to be visible, got {accents} px"
        );
    }

    /// Read one frame out of the committed `tools/padforge.ico`.
    ///
    /// An ICONDIR entry is a byte for width, a byte for height (0 meaning 256),
    /// then colour count, reserved, planes, bit count, size and offset. Reading
    /// the header at the wrong width is how a frame silently comes back as
    /// something other than the size that was asked for.
    fn ico_frame(size: u8) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tools")
            .join("padforge.ico");
        let data = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
        assert!(
            data.len() >= 6,
            "too short to be an .ico: {}",
            path.display()
        );

        let count = u16::from_le_bytes([data[4], data[5]]);
        for i in 0..count {
            let at = 6 + i as usize * 16;
            assert!(at + 16 <= data.len(), "icon directory runs past the file");
            let width = data[at];
            let wanted = if width == 0 { 256 } else { width as u32 };
            let len = u32::from_le_bytes(data[at + 8..at + 12].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(data[at + 12..at + 16].try_into().unwrap()) as usize;
            if wanted == size as u32 {
                assert!(
                    off + len <= data.len(),
                    "frame points past the end of the file"
                );
                return data[off..off + len].to_vec();
            }
        }
        panic!("the .ico has no {size}px frame");
    }

    /// Decode a 32bpp DIB frame to RGBA, bottom-up as every DIB is stored.
    fn decode_frame(frame: &[u8], size: u32) -> Vec<u8> {
        assert!(frame.len() >= 40, "frame is shorter than a bitmap header");
        let width = i32::from_le_bytes(frame[4..8].try_into().unwrap()) as u32;
        let height = i32::from_le_bytes(frame[8..12].try_into().unwrap()) as u32;
        // biSize is the first field and is always 40; width starts at four and
        // height at eight, and the height counts the bitmap plus the AND mask.
        // Reading from offset zero reports a 40x64 icon, which is what a
        // mis-decoded frame looks like when described as dimensions.
        assert_eq!((width, height), (size, size * 2), "unexpected DIB header");
        let bits = u16::from_le_bytes(frame[14..16].try_into().unwrap());
        assert_eq!(bits, 32, "expected a 32bpp frame");

        let mut out = vec![0u8; (size * size * 4) as usize];
        let stride = (size * 4) as usize;
        for y in 0..size {
            let src = 40 + ((size - 1 - y) as usize) * stride;
            let dst = (y as usize) * stride;
            for x in 0..size {
                let s = src + (x as usize) * 4;
                let d = dst + (x as usize) * 4;
                // Stored BGRA, read back as RGBA.
                out[d] = frame[s + 2];
                out[d + 1] = frame[s + 1];
                out[d + 2] = frame[s];
                out[d + 3] = frame[s + 3];
            }
        }
        out
    }

    /// The .ico on disk -- which is what the desktop shortcut, the Start Menu,
    /// the installer and the file's properties all show -- has to be this very
    /// mark, not merely something that also looks like an app icon.
    ///
    /// It was not: the generator drew a controller illustration while the window
    /// drew rings, so the shortcut and the title bar were two different pictures
    /// of two different programs. Nothing failed when that happened, because
    /// both files were perfectly valid, which is exactly why this check exists.
    ///
    /// The tolerance is the antialiasing the generator does and the runtime does
    /// not. Within the shape there are only two colours, so every opaque pixel
    /// has to be the square, the accent, or a blend of those two -- anything
    /// else means different artwork, and a controller silhouette fails it
    /// immediately on its greys and lightbar blue.
    #[test]
    fn the_committed_ico_is_this_mark() {
        const SQUARE: [f32; 3] = [0x12 as f32, 0x15 as f32, 0x1B as f32];
        const ACCENT: [f32; 3] = [0x35 as f32, 0xD0 as f32, 0xE8 as f32];

        let size = 64u32;
        let ico = decode_frame(&ico_frame(size as u8), size);
        let ours = mark(size);
        assert_eq!(ico.len(), ours.len());

        // The blend parameter, and the distance off the segment, for one pixel.
        // The parameter is also how the ring's *area* is counted further down:
        // a pixel that is half accent contributes half, which is the only way to
        // compare an antialiased file with a runtime that does not antialias.
        let accent = |px: &[u8]| {
            const D: [f32; 3] = [
                ACCENT[0] - SQUARE[0],
                ACCENT[1] - SQUARE[1],
                ACCENT[2] - SQUARE[2],
            ];
            let len2: f32 = D.iter().map(|v| v * v).sum();
            let p = [
                px[0] as f32 - SQUARE[0],
                px[1] as f32 - SQUARE[1],
                px[2] as f32 - SQUARE[2],
            ];
            let t =
                (p.iter().zip(D.iter()).map(|(p, d)| p * d).sum::<f32>() / len2).clamp(0.0, 1.0);
            let drift = (0..3)
                .map(|i| (p[i] - t * D[i]).abs())
                .fold(0.0f32, |acc, v| acc + v * v)
                .sqrt();
            (t, drift)
        };

        let (mut off_shape, mut off_colour) = (0, 0);
        let (mut area_ico, mut area_ours) = (0.0f32, 0.0f32);
        for (a, b) in ico.chunks_exact(4).zip(ours.chunks_exact(4)) {
            // Edges are judged by how far apart they are rather than by whether
            // they agree, because the generator antialiases and the runtime does
            // not: a boundary pixel is opaque on one side and half transparent on
            // the other, which is the two renderings agreeing about where the
            // edge is. Only a pixel one side considers solid and the other
            // considers empty is a real disagreement about the shape.
            let ico_solid = a[3] >= 250;
            let ico_gone = a[3] <= 8;
            let ours_solid = b[3] >= 250;
            let ours_gone = b[3] <= 8;
            if (ico_solid && ours_gone) || (ico_gone && ours_solid) {
                off_shape += 1;
                continue;
            }
            if !ico_solid || !ours_solid {
                continue;
            }

            // Colour judged by how far the pixel sits from the straight line
            // between the mark's two colours, not by a parameter read off the
            // red channel. Red spans 35 counts and blue spans 205, so a
            // half-count rounding in red becomes a five-count error once it is
            // multiplied out to blue -- rounding that belongs to any resampler
            // was being read as different artwork. The segment distance does not
            // care which channel happens to be the narrow one.
            let (t, drift) = accent(a);
            if drift > 4.0 {
                off_colour += 1;
            }
            area_ico += t;
            area_ours += accent(b).0;
        }

        let pixels = (size * size) as i32;
        assert_eq!(
            off_colour, 0,
            "{off_colour} of {pixels} pixels are not the mark's two colours or a blend of them"
        );
        assert!(
            off_shape <= pixels / 50,
            "{off_shape} of {pixels} pixels disagree about where the edge is"
        );
        // Ring thickness as an area rather than a count of pure-accent pixels:
        // counting pixels that are *purely* accent counts every antialiased edge
        // of the .ico as missing and reports its rings as a quarter thinner than
        // they are, when the two drawings are in fact the same width -- 622
        // against 640 square sixteenths of accent at this size.
        assert!(
            area_ico * 100.0 >= area_ours * 90.0,
            "the .ico's rings are a different width: {area_ico:.0} of accent area against {area_ours:.0}"
        );
    }
}
