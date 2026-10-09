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
        assert!(opaque > 64 * 64 / 2, "icon is mostly transparent: {opaque} px");
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
        assert!(accents > 100, "expected the rings to be visible, got {accents} px");
    }
}