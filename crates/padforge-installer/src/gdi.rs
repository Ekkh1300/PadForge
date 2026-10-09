//! Small drawing helpers over GDI.
//!
//! The installer draws with GDI rather than a GPU renderer on purpose: GDI is
//! available on every Windows machine, needs nothing from the display driver
//! beyond the basics, and cannot be broken by a wedged OpenGL state. That last
//! point is not hypothetical — the machine this was written on renders every glow
//! window white, so a GPU-backed installer would have been both unusable there
//! and unverifiable here.
//!
//! Handles are owned and released on drop. Selecting an object always restores
//! the previous one first, because GDI has a small per-process handle budget and
//! a leaked brush is a slow failure that shows up long after the mistake.

#![allow(non_snake_case)]

use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{HWND, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
    GetTextExtentPoint32W, LineTo, MoveToEx, Rectangle, SelectObject, SetBkMode, SetTextColor,
    TextOutW, HDC, HGDIOBJ, LOGFONTW, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

/// A colour as `0x00BBGGRR`, which is what GDI expects.
pub const fn rgb(r: u8, g: u8, b: u8) -> i32 {
    (r as i32) | ((g as i32) << 8) | ((b as i32) << 16)
}

/// The installer's palette, matching the application's dark theme.
pub mod palette {
    use super::rgb;

    /// Window background.
    pub const BG: i32 = rgb(0x14, 0x17, 0x1D);
    /// Raised panel, for the card.
    pub const PANEL: i32 = rgb(0x1B, 0x1F, 0x27);
    /// Hairline borders.
    pub const LINE: i32 = rgb(0x2B, 0x32, 0x3E);
    /// Primary text.
    pub const TEXT: i32 = rgb(0xE8, 0xEC, 0xF2);
    /// Secondary text.
    pub const MUTED: i32 = rgb(0x8C, 0x97, 0xA8);
    /// Accent, for the primary button and the focus ring.
    pub const ACCENT: i32 = rgb(0x56, 0xB6, 0xFF);
    /// Warning, for the driver notice.
    pub const WARN: i32 = rgb(0xE8, 0xB3, 0x39);
    /// Danger, for failures.
    pub const BAD: i32 = rgb(0xE0, 0x6C, 0x75);
    /// Neutral button face and the folder field.
    pub const BUTTON: i32 = rgb(0x24, 0x2A, 0x34);
}

/// Owns a GDI object and deletes it on drop.
pub struct Owned(HGDIOBJ);

impl Owned {
    pub fn raw(&self) -> HGDIOBJ {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this handle came from a Create* call and DeleteObject has
            // not been called for it. GDI ignores a delete of an object that is
            // still selected, so a missed deselect cannot become a double free.
            unsafe { DeleteObject(self.0) };
        }
    }
}

/// A solid fill brush, owned and freed on drop.
pub fn brush(colour: i32) -> Owned {
    // SAFETY: a plain GDI call with a valid colour.
    unsafe { Owned(CreateSolidBrush(colour as u32)) }
}

/// A solid brush that is never freed, for use as a window class background.
///
/// `hbrBackground` is read by the window manager at arbitrary times, including
/// before the creating object exists and after it is gone, so it cannot be an
/// owned handle. Freeing it would leave the class pointing at a recycled handle
/// and Windows would paint with whatever another allocation put there.
pub fn static_brush(colour: i32) -> HGDIOBJ {
    // SAFETY: a plain GDI call. The leak is intentional and bounded: exactly one
    // of these exists for the lifetime of the process.
    unsafe { CreateSolidBrush(colour as u32) }
}

/// A pen. A width of zero asks for the thinnest line the driver will draw, which
/// is what a hairline wants.
pub fn pen(colour: i32, width: i32) -> Owned {
    // SAFETY: PS_SOLID with a valid colour and a non-negative width.
    unsafe { Owned(CreatePen(PS_SOLID, width.max(1), colour as u32)) }
}

/// A font at the given height in logical pixels.
///
/// `weight` is a GDI weight (400 regular, 700 bold). The face is Segoe UI, the
/// modern Windows UI font, and GDI substitutes silently when it is missing so the
/// text still renders rather than disappearing.
pub fn font(height: i32, weight: i32) -> Owned {
    let mut spec = LOGFONTW {
        lfHeight: height,
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: weight,
        lfItalic: 0,
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: 0, // DEFAULT_CHARSET
        lfOutPrecision: 0,
        lfClipPrecision: 0,
        lfQuality: 0,
        lfPitchAndFamily: 0,
        lfFaceName: [0; 32],
    };
    for (slot, unit) in spec
        .lfFaceName
        .iter_mut()
        .zip(std::ffi::OsStr::new("Segoe UI").encode_wide())
    {
        *slot = unit;
    }
    // SAFETY: `spec` is fully initialised and the face name is NUL-padded.
    unsafe { Owned(CreateFontIndirectW(&spec)) }
}

/// Selects a GDI object and restores the previous one on drop.
///
/// GDI requires the original to be reselected before the object is deleted, and
/// omitting that is the classic cause of a handle that flickers as it leaks.
pub struct Selected<'a> {
    dc: HDC,
    previous: HGDIOBJ,
    #[allow(dead_code)]
    object: &'a Owned,
}

impl<'a> Selected<'a> {
    pub fn brush(dc: HDC, object: &'a Owned) -> Self {
        // SAFETY: both handles are valid; the previous value is restored in Drop.
        let previous = unsafe { SelectObject(dc, object.raw()) };
        Self {
            dc,
            previous,
            object,
        }
    }

    pub fn pen(dc: HDC, object: &'a Owned) -> Self {
        // SAFETY: as above.
        let previous = unsafe { SelectObject(dc, object.raw()) };
        Self {
            dc,
            previous,
            object,
        }
    }

    pub fn font(dc: HDC, object: &'a Owned) -> Self {
        // SAFETY: as above.
        let previous = unsafe { SelectObject(dc, object.raw()) };
        Self {
            dc,
            previous,
            object,
        }
    }
}

impl Drop for Selected<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.previous` is what the DC held before, so reselecting it
        // releases ours, which is what makes the Owned's Drop safe.
        unsafe { SelectObject(self.dc, self.previous) };
    }
}

/// A device context borrowed for the duration of a paint.
pub struct Painter {
    hwnd: HWND,
    pub dc: HDC,
}

impl Painter {
    /// Begin painting. Must be paired with [`Painter::end`].
    ///
    /// # Safety
    /// `hwnd` must be a live window, and `end` must be called.
    pub unsafe fn begin(hwnd: HWND) -> (Painter, PAINTSTRUCT) {
        let mut ps = std::mem::zeroed::<PAINTSTRUCT>();
        let dc = BeginPaint(hwnd, &mut ps);
        (Painter { hwnd, dc }, ps)
    }

    /// # Safety
    /// Must be called once, on the same window as `begin`.
    pub unsafe fn end(&self, ps: &PAINTSTRUCT) {
        EndPaint(self.hwnd, ps);
    }

    /// The whole client area.
    pub fn size(&self) -> (i32, i32) {
        let r = self.client_rect();
        (r.right - r.left, r.bottom - r.top)
    }

    /// The client rectangle, in client coordinates.
    pub fn client_rect(&self) -> RECT {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a live RECT and window.
        unsafe { GetClientRect(self.hwnd, &mut r) };
        r
    }

    /// Fill the entire client area.
    pub fn fill(&self, colour: i32) {
        // The client rect has to be fetched and passed. FillRect dereferences
        // its lpRect unconditionally: a null pointer does not mean "everything",
        // it faults a few bytes into the RECT, which surfaces as a
        // STATUS_FATAL_USER_CALLBACK_EXCEPTION out of the window proc with an
        // address around 0x9a and no clue as to why.
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a live RECT and window.
        unsafe { GetClientRect(self.hwnd, &mut r) };

        let b = brush(colour);
        // SAFETY: `r` is filled in and the brush outlives the call.
        unsafe { FillRect(self.dc, &r, b.raw()) };
    }

    /// Fill a rectangle.
    pub fn fill_rect(&self, rect: &RECT, colour: i32) {
        let b = brush(colour);
        // SAFETY: the rect is borrowed, not copied, and the brush outlives the
        // call. FillRect does not write through it, so a shared reference is
        // enough despite the binding's *mut.
        unsafe { FillRect(self.dc, rect, b.raw()) };
    }

    /// Draw a rectangle outline without touching the inside.
    ///
    /// `Rectangle` fills as well as strokes, using whatever brush is currently
    /// selected into the DC. That brush defaults to white, so calling `Rectangle`
    /// with only a pen selected paints a white box over everything drawn so far.
    /// The interior brush is therefore set to the same colour as the pen, and the
    /// panel is filled separately.
    pub fn stroke_rect(&self, rect: &RECT, colour: i32) {
        let p = pen(colour, 1);
        let _pen = Selected::pen(self.dc, &p);
        let interior = brush(colour);
        let _brush = Selected::brush(self.dc, &interior);
        // SAFETY: the four coordinates come from a live RECT, and both the pen and
        // the brush stay selected for the duration of the call.
        unsafe {
            Rectangle(self.dc, rect.left, rect.top, rect.right, rect.bottom);
        }
    }

    /// Draw text at an absolute position.
    pub fn text(&self, x: i32, y: i32, s: &str, colour: i32, f: &Owned) {
        let _sel = Selected::font(self.dc, f);
        // SAFETY: TRANSPARENT mode keeps the background from being painted, which
        // is what lets text sit on the dark background without a box behind it.
        unsafe {
            SetBkMode(self.dc, TRANSPARENT as i32);
            SetTextColor(self.dc, colour as u32);
            let wide = std::ffi::OsStr::new(s).encode_wide().collect::<Vec<u16>>();
            if wide.is_empty() {
                return;
            }
            TextOutW(self.dc, x, y, wide.as_ptr(), wide.len() as i32);
        }
    }

    /// Draw text truncated to fit within `max_x`, with an ellipsis if it does not.
    ///
    /// Clipping matters here: an install path can be long enough to run under the
    /// button beside it, and Windows does not clip TextOutW for us.
    pub fn text_clipped(&self, x: i32, y: i32, s: &str, colour: i32, f: &Owned, max_x: i32) {
        let mut text = s.to_string();
        if self.measure(&text, f) <= max_x - x {
            self.text(x, y, &text, colour, f);
            return;
        }
        // Trim from the end, leaving room for the ellipsis character.
        while !text.is_empty() {
            text.pop();
            let candidate = format!("{text}\u{2026}");
            if self.measure(&candidate, f) <= max_x - x {
                self.text(x, y, &candidate, colour, f);
                return;
            }
        }
        self.text(x, y, "\u{2026}", colour, f);
    }

    /// Draw text wrapped inside a rectangle.
    ///
    /// GDI has no wrapping of its own, and the status line is the one place a
    /// long sentence has to survive: the ViGEmBus warning is the whole reason the
    /// line exists, so truncating it would defeat the point. The rectangle is
    /// passed whole rather than as four coordinates because "wrap inside this
    /// box" is the actual intent, and a caller should not have to remember which
    /// pair of the four numbers was the width.
    pub fn text_wrapped(&self, box_rect: &RECT, s: &str, colour: i32, f: &Owned, line_height: i32) {
        let _sel = Selected::font(self.dc, f);
        // SAFETY: TRANSPARENT mode keeps the background from being painted.
        unsafe { SetBkMode(self.dc, TRANSPARENT as i32) };

        let x = box_rect.left;
        let y = box_rect.top;
        let width = box_rect.right - x;
        let max_y = box_rect.bottom;
        if width <= 0 {
            return;
        }

        let mut line = String::new();
        let mut top = y;
        // A word longer than the line, such as a path, is broken rather than
        // allowed to overflow.
        for word in s.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };

            if self.measure(&candidate, f) <= width {
                line = candidate;
                continue;
            }

            // The word does not fit on the current line. Flush what is there, then
            // start a new line with the word.
            if !line.is_empty() {
                self.text(x, top, &line, colour, f);
                top += line_height;
                if top + line_height > max_y {
                    return;
                }
                line.clear();
            }

            // The word alone may still be too wide.
            let mut piece: String = word.to_string();
            while self.measure(&piece, f) > width && piece.len() > 1 {
                // Drop one character at a time from the end. This is only hit for
                // long unbroken strings, so the cost does not matter.
                piece.pop();
                if piece.ends_with(' ') {
                    piece.pop();
                }
            }
            self.text(x, top, &piece, colour, f);
            top += line_height;
            if top + line_height > max_y {
                return;
            }
            // Whatever is left of the word continues on the next line.
            let remainder: String = word.chars().skip(piece.chars().count()).collect();
            line = remainder.trim_start().to_string();
        }

        if !line.is_empty() && top + line_height <= max_y {
            self.text(x, top, &line, colour, f);
        }
    }

    /// Measure a string's width in pixels.
    pub fn measure(&self, s: &str, f: &Owned) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let _sel = Selected::font(self.dc, f);
        let wide = std::ffi::OsStr::new(s).encode_wide().collect::<Vec<u16>>();
        let mut size = SIZE { cx: 0, cy: 0 };
        // SAFETY: `size` is a live SIZE and the font is selected.
        let ok =
            unsafe { GetTextExtentPoint32W(self.dc, wide.as_ptr(), wide.len() as i32, &mut size) };
        if ok == 0 {
            0
        } else {
            size.cx
        }
    }

    /// A straight line.
    pub fn line(&self, x1: i32, y1: i32, x2: i32, y2: i32, colour: i32) {
        let p = pen(colour, 1);
        let _sel = Selected::pen(self.dc, &p);
        let mut dummy = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        // SAFETY: both points are valid and the pen is selected for the call.
        unsafe {
            MoveToEx(self.dc, x1, y1, &mut dummy);
            LineTo(self.dc, x2, y2);
        }
    }
}

/// Ask the shell to refresh this session's icon cache.
///
/// Explorer caches file icons hard. After an installer writes a new binary, the
/// old generic glyph can persist until sign-out unless the cache is told that
/// something changed.
pub fn refresh_icon_cache() {
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: SHChangeNotify with SHCNF_IDLIST takes no paths.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}

/// Lighten a colour by `amount` on each channel.
pub fn lighten(colour: i32, amount: i32) -> i32 {
    let (r, g, b) = split(colour);
    pack(r + amount, g + amount, b + amount)
}

/// Darken a colour by `amount` on each channel.
pub fn darken(colour: i32, amount: i32) -> i32 {
    let (r, g, b) = split(colour);
    pack(r - amount, g - amount, b - amount)
}

fn split(colour: i32) -> (i32, i32, i32) {
    (colour & 0xFF, (colour >> 8) & 0xFF, (colour >> 16) & 0xFF)
}

fn pack(r: i32, g: i32, b: i32) -> i32 {
    (r.clamp(0, 255)) | (g.clamp(0, 255) << 8) | (b.clamp(0, 255) << 16)
}
