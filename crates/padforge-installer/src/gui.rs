//! The installer's window, drawn entirely by hand.
//!
//! Built with plain Win32 and GDI rather than a toolkit or a GPU renderer, for
//! two reasons. It keeps the installer dependency-free beyond `windows-sys`, and
//! GDI cannot be broken by a wedged graphics driver — which is not hypothetical:
//! the machine this was written on renders every OpenGL window white, so a
//! GPU-backed installer would have been unusable there and unverifiable here.
//!
//! Everything is owner-drawn, including the checkboxes and buttons. Stock
//! controls would each need a WM_CTLCOLOR handler to match the palette, and this
//! way the whole dialog is one paint function with no z-order surprises and no
//! flicker on hover.

#![allow(non_snake_case)]

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU8, Ordering};

use windows_sys::Win32::Foundation::{GetLastError, HWND as HWND_T, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{FillRect, InvalidateRect, UpdateWindow, HDC};
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
// WM_MOUSELEAVE is a documented constant rather than a literal here.
const WM_MOUSELEAVE: u32 = 0x02A3;

use crate::gdi::{self, darken, lighten, palette, Owned, Painter};

/// Window size. The dialog is deliberately narrow: it is one decision, not a
/// wizard, and a wide box with three checkboxes reads as a settings page.
const W: i32 = 560;
const H: i32 = 452;

/// The clickable regions, laid out from the client size and hit-tested against.
#[derive(Clone, Copy)]
struct Layout {
    card: RECT,
    folder: RECT,
    browse: RECT,
    desktop: RECT,
    autostart: RECT,
    launch: RECT,
    install: RECT,
    cancel: RECT,
    /// Where the status line sits, below the card.
    status: RECT,
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

impl Layout {
    /// Card top, shared between the layout and the painter so the panel cannot
    /// drift away from the controls that are supposed to sit inside it.
    const CARD_TOP: i32 = 126;
    /// Padding inside the card.
    const PAD: i32 = 18;

    fn new(w: i32, h: i32) -> Self {
        const M: i32 = 28;
        let inner = w - M * 2;
        let row = 34;
        let button_h = 36;

        let card = inner - Self::PAD * 2;
        let left = M + Self::PAD;

        let mut y = Self::CARD_TOP + 58;
        let folder = rect(left, y, card - 122, row);
        let browse = rect(left + card - 110, y, 110, row);

        y += row + 16;
        let desktop = rect(left, y, card, 20);

        y += 26;
        let autostart = rect(left, y, card, 20);

        y += 26;
        let launch = rect(left, y, card, 20);

        let by = h - 62;
        let cancel = rect(w - M - 100, by, 100, button_h);
        let install = rect(w - M - 100 - 12 - 168, by, 168, button_h);

        // The card ends just below the last option, rather than being stretched
        // to a fixed height. A panel with empty space under its contents reads
        // as a mistake, and the height here has to track the row spacing.
        let card_bottom = launch.bottom + Self::PAD;
        let card_rect = rect(M, Self::CARD_TOP, inner, card_bottom - Self::CARD_TOP);

        // The status line sits between the card and the buttons, with room for
        // two lines so a long warning wraps instead of running off the window.
        let status = rect(M, card_bottom + 14, inner, 40);

        Self {
            card: card_rect,
            folder,
            browse,
            desktop,
            autostart,
            launch,
            install,
            cancel,
            status,
        }
    }

    /// The region a point falls in, if any.
    fn hit(&self, x: i32, y: i32) -> Option<Region> {
        let all = [
            (self.folder, Region::Folder),
            (self.browse, Region::Browse),
            (self.desktop, Region::Desktop),
            (self.autostart, Region::Autostart),
            (self.launch, Region::Launch),
            (self.install, Region::Install),
            (self.cancel, Region::Cancel),
        ];
        all.iter()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, region)| *region)
    }
}

/// What a click landed on.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Region {
    Folder,
    Browse,
    Desktop,
    Autostart,
    Launch,
    Install,
    Cancel,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Choose,
    Working,
    Done,
}

#[derive(Clone, Copy, PartialEq)]
enum StatusKind {
    Info,
    Good,
    Warn,
    Bad,
}

impl StatusKind {
    fn colour(self) -> i32 {
        match self {
            StatusKind::Info => palette::MUTED,
            StatusKind::Good => palette::ACCENT,
            StatusKind::Warn => palette::WARN,
            StatusKind::Bad => palette::BAD,
        }
    }
}

/// Everything the window owns, hanging off the HWND.
struct State {
    folder: PathBuf,
    desktop: bool,
    autostart: bool,
    launch: bool,
    status: String,
    status_kind: StatusKind,
    phase: Phase,
    hover: Option<Region>,
    pressed: Option<Region>,
    /// Tracked so a hover highlight goes away when the pointer leaves the window.
    tracking: bool,
    fonts: Fonts,
}

impl State {
    fn new(folder: PathBuf) -> Self {
        let mut status = format!(
            "{} will be installed for your user account.",
            crate::APP_NAME
        );
        let mut kind = StatusKind::Info;

        // Say up front that games will not see the pad without the driver, rather
        // than leaving the user to work it out after installing.
        if !crate::vigem_installed() {
            status = "Note: games need the ViGEmBus driver, which is not installed yet. \
                      PadForge will still install and run."
                .to_string();
            kind = StatusKind::Warn;
        }

        Self {
            folder,
            desktop: true,
            autostart: false,
            launch: true,
            status,
            status_kind: kind,
            phase: Phase::Choose,
            hover: None,
            pressed: None,
            tracking: false,
            fonts: Fonts::new(),
        }
    }

    /// Whether the folder and option controls respond.
    fn interactive(&self) -> bool {
        self.phase == Phase::Choose
    }

    fn toggle(&mut self, region: Region) {
        match region {
            Region::Desktop => self.desktop = !self.desktop,
            Region::Autostart => self.autostart = !self.autostart,
            Region::Launch => self.launch = !self.launch,
            _ => {}
        }
    }
}

/// Fonts, created once. Rebuilding them on every WM_PAINT is real GDI churn for
/// something that never changes.
struct Fonts {
    title: Owned,
    lead: Owned,
    body: Owned,
    small: Owned,
    button: Owned,
    mono: Owned,
}

impl Fonts {
    fn new() -> Self {
        Self {
            title: gdi::font(24, 700),
            lead: gdi::font(14, 400),
            body: gdi::font(14, 400),
            small: gdi::font(13, 400),
            button: gdi::font(14, 600),
            mono: gdi::font(13, 400),
        }
    }
}

/// How the window closed. A window proc cannot hand a value back to its caller,
/// so the decision is parked here.
static EXIT: AtomicU8 = AtomicU8::new(0);

/// Show the dialog and return the chosen folder, or `None` if cancelled.
pub fn run(folder: PathBuf) -> Result<PathBuf, String> {
    // Needed for the shell folder picker. Entering it here rather than in main
    // keeps the silent and command-line paths from paying for it.
    // SAFETY: called before any COM use on this thread.
    unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32) };

    // SAFETY: HINSTANCE is a pointer-sized value, and a null instance is what
    // a plain executable passes: the class is registered for this process only,
    // which is all a single-window dialog needs.
    let instance: windows_sys::Win32::Foundation::HINSTANCE =
        unsafe { std::mem::transmute(0isize) };

    let class_name = wide("PadForgeInstaller");
    let title = wide("Install PadForge");

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        // SAFETY: a null instance loads the shared system cursor.
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        // A real brush, not null. With a null background the window manager keeps
        // the surface in the default white, and the dialog flashes white before
        // the first paint. WM_ERASEBKGND below fills it properly, but the class
        // brush is what covers the window during creation and resize, when no
        // WM_PAINT has happened yet.
        hbrBackground: gdi::static_brush(palette::BG),
        lpszMenuName: std::ptr::null(),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hIcon: null_mut(),
        hIconSm: null_mut(),
    };

    // SAFETY: `wc` is fully initialised.
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 {
        return Err(format!(
            "could not register a window class (error {})",
            unsafe { GetLastError() }
        ));
    }

    let state_ptr = Box::into_raw(Box::new(State::new(folder.clone())));

    // SAFETY: standard creation against the class just registered.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            W,
            H,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        )
    };
    if hwnd.is_null() {
        let err = unsafe { GetLastError() };
        // SAFETY: recovering the box we just leaked into a raw pointer.
        drop(unsafe { Box::from_raw(state_ptr) });
        return Err(format!("could not create the window (error {err})"));
    }

    // Attached before any message can arrive, which is why the pointer is set
    // immediately after creation rather than in WM_CREATE.
    // SAFETY: hwnd is live and the pointer is ours to hand over.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };

    centre(hwnd, W, H);

    // SAFETY: standard show and repaint.
    unsafe {
        ShowWindow(hwnd, SW_SHOWNORMAL);
        UpdateWindow(hwnd);
    }

    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG.
        let got = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if got <= 0 {
            break;
        }
        // SAFETY: a standard dispatch.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let installed = EXIT.load(Ordering::SeqCst) == 1;

    // If WM_NCDESTROY somehow did not run, do not leak the state.
    // SAFETY: reading back the pointer that was set above.
    let leftover = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State };
    if !leftover.is_null() {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(leftover));
        }
    }
    // SAFETY: the window is already destroyed; this is belt and braces.
    unsafe { DestroyWindow(hwnd) };

    if installed {
        Ok(folder)
    } else {
        Err("cancelled".to_string())
    }
}

fn wide(text: &str) -> Vec<u16> {
    let mut v: Vec<u16> = std::ffi::OsStr::new(text).encode_wide().collect();
    v.push(0);
    v
}

unsafe fn state_of(hwnd: HWND_T) -> Option<&'static mut State> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    // The window outlives every reference taken here: the state is detached in
    // WM_NCDESTROY, and no message after that can reach it.
    ptr.as_mut()
}

/// # Safety
/// Called by Windows with a live HWND.
unsafe extern "system" fn window_proc(
    hwnd: HWND_T,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            // Filling here rather than letting the class brush do it keeps the
            // background correct even if the class brush is ever replaced, and it
            // avoids a flicker between the erase and the paint.
            let dc = wparam as HDC;
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: a live RECT and window, with the DC Windows supplied.
            unsafe {
                GetClientRect(hwnd, &mut r);
                let b = gdi::brush(palette::BG);
                FillRect(dc, &r, b.raw());
            }
            // 1 means "handled", so Windows does not fall back to the class brush.
            1
        }
        WM_PAINT => {
            if let Some(s) = state_of(hwnd) {
                repaint(hwnd, s);
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(s) = state_of(hwnd) {
                on_move(hwnd, s, lparam);
            }
            0
        }
        WM_LBUTTONDOWN => {
            if let Some(s) = state_of(hwnd) {
                on_press(hwnd, s, lparam);
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(s) = state_of(hwnd) {
                on_release(hwnd, s, lparam);
            }
            0
        }
        WM_MOUSELEAVE => {
            if let Some(s) = state_of(hwnd) {
                s.hover = None;
                s.tracking = false;
                invalidate(hwnd);
            }
            0
        }
        WM_KEYDOWN => {
            // Enter installs, Escape cancels: both are what a keyboard user
            // expects and neither works without them.
            match wparam as u16 {
                VK_RETURN => {
                    if let Some(s) = state_of(hwnd) {
                        if s.interactive() {
                            activate(hwnd, s, Region::Install);
                        }
                    }
                }
                VK_ESCAPE => {
                    EXIT.store(0, Ordering::SeqCst);
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            EXIT.store(0, Ordering::SeqCst);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn point_of(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam as u32;
    (
        (packed & 0xFFFF) as i16 as i32,
        (packed >> 16) as i16 as i32,
    )
}

fn hit_at(hwnd: HWND_T, s: &State, lparam: LPARAM) -> Option<Region> {
    if !s.interactive() {
        return None;
    }
    let (x, y) = point_of(lparam);
    let (w, h) = client_size(hwnd);
    Layout::new(w, h).hit(x, y)
}

fn on_move(hwnd: HWND_T, s: &mut State, lparam: LPARAM) {
    let hit = hit_at(hwnd, s, lparam);
    let mut changed = hit != s.hover;
    // Clear the pressed state when the pointer leaves the region it went down
    // on, so dragging off a button cancels it the way every other button behaves.
    if let Some(active) = s.pressed {
        if hit != Some(active) {
            s.pressed = None;
            changed = true;
        }
    }

    if !s.tracking {
        // Needed so WM_MOUSELEAVE arrives at all.
        track_leave(hwnd);
        s.tracking = true;
    }

    if changed {
        s.hover = hit;
        invalidate(hwnd);
    }
}

fn on_press(hwnd: HWND_T, s: &mut State, lparam: LPARAM) {
    let hit = hit_at(hwnd, s, lparam);
    s.pressed = hit;
    invalidate(hwnd);
}

fn on_release(hwnd: HWND_T, s: &mut State, lparam: LPARAM) {
    let hit = hit_at(hwnd, s, lparam);
    let was = s.pressed;
    s.pressed = None;
    // Only act when the press and release landed on the same region, so dragging
    // off a button cancels it the way every other button behaves.
    if hit.is_some() && hit == was {
        if let Some(region) = hit {
            activate(hwnd, s, region);
        }
    }
    invalidate(hwnd);
}

/// React to a completed click.
fn activate(hwnd: HWND_T, s: &mut State, region: Region) {
    match region {
        Region::Desktop | Region::Autostart | Region::Launch => {
            s.toggle(region);
            invalidate(hwnd);
        }
        Region::Browse => match browse_for_folder(hwnd) {
            Some(dir) => {
                s.folder = dir;
                invalidate(hwnd);
            }
            None => invalidate(hwnd),
        },
        Region::Folder => {
            // Clicking the field is a hint, not an action: there is no inline
            // editing, so open the picker where a real edit would start.
            if let Some(dir) = browse_for_folder(hwnd) {
                s.folder = dir;
            }
            invalidate(hwnd);
        }
        Region::Cancel => {
            EXIT.store(0, Ordering::SeqCst);
            // SAFETY: standard destruction.
            unsafe { DestroyWindow(hwnd) };
        }
        Region::Install => do_install(hwnd, s),
    }
}

/// Run the install, reporting progress through the status line.
fn do_install(hwnd: HWND_T, s: &mut State) {
    if !s.interactive() {
        return;
    }
    s.phase = Phase::Working;
    s.status = format!("Installing {}...", crate::APP_NAME);
    s.status_kind = StatusKind::Info;
    s.hover = None;
    s.pressed = None;
    invalidate(hwnd);
    // Paint before blocking, so the status line is actually on screen.
    pump(hwnd);

    let folder = s.folder.clone();
    let desktop = s.desktop;
    let autostart = s.autostart;
    let launch = s.launch;

    let outcome = std::thread::Builder::new()
        .name("padforge-install".into())
        .spawn(move || crate::install_into(&folder, desktop, autostart, launch))
        .map_err(|e| e.to_string())
        .and_then(|handle| {
            handle
                .join()
                .map_err(|_| "the installer stopped unexpectedly".to_string())?
        });

    match outcome {
        Ok(()) => {
            s.phase = Phase::Done;
            s.status = format!("{} is installed.", crate::APP_NAME);
            s.status_kind = StatusKind::Good;
            // Leave the dialog up showing what happened, rather than vanishing:
            // the user gets to read it, and Finish closes the window.
        }
        Err(e) => {
            s.phase = Phase::Choose;
            s.status = e;
            s.status_kind = StatusKind::Bad;
        }
    }
    invalidate(hwnd);
}

/// Force a synchronous repaint, so status text appears before a blocking call.
fn pump(hwnd: HWND_T) {
    // SAFETY: a standard update; the state is attached and live.
    unsafe {
        UpdateWindow(hwnd);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Draw the whole dialog.
fn repaint(hwnd: HWND_T, s: &State) {
    // SAFETY: begin and end are paired below.
    let (p, ps) = unsafe { Painter::begin(hwnd) };
    let ui = Ui {
        p: &p,
        fonts: &s.fonts,
        state: s,
    };
    let (w, h) = p.size();
    let l = Layout::new(w, h);

    p.fill(palette::BG);

    // --- heading ---------------------------------------------------------
    p.text(28, 26, crate::APP_NAME, palette::TEXT, &s.fonts.title);
    p.text(
        28,
        62,
        "Give your DualShock 4 a native voice on Windows.",
        palette::MUTED,
        &s.fonts.lead,
    );
    p.line(28, 96, w - 28, 96, palette::LINE);

    // --- the card --------------------------------------------------------
    p.fill_rect(&l.card, palette::PANEL);
    p.stroke_rect(&l.card, palette::LINE);

    p.text(
        l.card.left + Layout::PAD,
        l.card.top + 14,
        "Destination",
        palette::TEXT,
        &s.fonts.body,
    );
    p.text(
        l.card.left + Layout::PAD,
        l.card.top + 34,
        "No administrator rights are needed.",
        palette::MUTED,
        &s.fonts.small,
    );

    // --- folder row ------------------------------------------------------
    let editable = s.interactive();
    p.fill_rect(&l.folder, palette::BUTTON);
    p.stroke_rect(
        &l.folder,
        if editable && s.hover == Some(Region::Folder) {
            palette::ACCENT
        } else {
            palette::LINE
        },
    );
    // Clipped to the field: an install path can be long enough to run under the
    // Browse button, and GDI does not clip for us.
    p.text_clipped(
        l.folder.left + 10,
        l.folder.top + 9,
        &s.folder.to_string_lossy(),
        palette::TEXT,
        &s.fonts.mono,
        l.folder.right - 8,
    );
    draw_button(
        &ui,
        &l.browse,
        "Browse...",
        ButtonStyle::neutral(),
        Region::Browse,
    );

    // --- options ---------------------------------------------------------
    for (r, checked, label) in [
        (l.desktop, s.desktop, "Create a Desktop shortcut"),
        (l.autostart, s.autostart, "Start PadForge when I sign in"),
        (l.launch, s.launch, "Run PadForge when the install finishes"),
    ] {
        draw_checkbox(&p, &r, checked, editable, label, &s.fonts);
    }

    // --- status ----------------------------------------------------------
    // Wrapped rather than clipped: the driver warning is long, and truncating it
    // would hide the one thing the user most needs to read.
    p.text_wrapped(
        &l.status,
        &s.status,
        s.status_kind.colour(),
        &s.fonts.small,
        18,
    );

    // --- buttons ---------------------------------------------------------
    if s.phase == Phase::Done {
        draw_button(
            &ui,
            &l.install,
            "Finish",
            ButtonStyle::primary(),
            Region::Install,
        );
        // Finishing closes the window; a Cancel button next to it would be
        // ambiguous, so it is left out of the Done state.
        return;
    }

    draw_button(
        &ui,
        &l.install,
        if s.phase == Phase::Working {
            "Installing..."
        } else {
            "Install"
        },
        ButtonStyle::primary(),
        Region::Install,
    );
    draw_button(
        &ui,
        &l.cancel,
        "Cancel",
        ButtonStyle::neutral(),
        Region::Cancel,
    );

    // SAFETY: balances Painter::begin above.
    unsafe { p.end(&ps) };
}

/// How a button is painted: its fill colour and the colour of its label.
///
/// Bundled into a struct because a button has several interacting visual states
/// and passing them as loose arguments makes it easy to swap two of them without
/// noticing.
struct ButtonStyle {
    /// The fill at rest.
    fill: i32,
    /// The label colour on that fill.
    label: i32,
}

impl ButtonStyle {
    /// The accent button: the one action the dialog is for.
    fn primary() -> Self {
        Self {
            fill: palette::ACCENT,
            label: palette::BG,
        }
    }

    /// A neutral button: everything that is not the main action.
    fn neutral() -> Self {
        Self {
            fill: palette::BUTTON,
            label: palette::TEXT,
        }
    }
}

/// What a paint helper needs: the device context, the window's fonts, and the
/// window's state for hover and enablement.
///
/// Bundled so the drawing helpers take one argument instead of four, and so it
/// is impossible for one to read a font set that belongs to a different window.
struct Ui<'a> {
    p: &'a Painter,
    fonts: &'a Fonts,
    state: &'a State,
}

/// A button. The label is centred by measuring it, because Segoe UI's width
/// varies enough that a fixed offset is visibly off for longer labels.
fn draw_button(ui: &Ui<'_>, r: &RECT, label: &str, style: ButtonStyle, region: Region) {
    let state = ui.state;
    // Cancel stays live throughout: a dialog that is installing still has to be
    // closable, even though the folder and options are locked while it works.
    let enabled = state.interactive() || matches!(region, Region::Cancel);
    let hover = enabled && state.hover == Some(region);
    let pressed = enabled && state.pressed == Some(region);

    let fill = if !enabled {
        palette::PANEL
    } else if pressed {
        darken(style.fill, 30)
    } else if hover {
        lighten(style.fill, 20)
    } else {
        style.fill
    };
    ui.p.fill_rect(r, fill);
    if !enabled {
        ui.p.stroke_rect(r, palette::LINE);
    }

    let text = if enabled { style.label } else { palette::MUTED };
    let width = ui.p.measure(label, &ui.fonts.button);
    let x = r.left + (r.right - r.left - width) / 2;
    let y = r.top + (r.bottom - r.top - 17) / 2;
    ui.p.text(x, y, label, text, &ui.fonts.button);
}

/// A checkbox with its label, drawn as a square rather than the stock 3D bevel.
fn draw_checkbox(p: &Painter, r: &RECT, checked: bool, enabled: bool, label: &str, fonts: &Fonts) {
    const BOX: i32 = 18;
    let box_rect = RECT {
        left: r.left,
        top: r.top + 1,
        right: r.left + BOX,
        bottom: r.top + 1 + BOX,
    };

    p.fill_rect(
        &box_rect,
        if checked {
            palette::ACCENT
        } else {
            palette::BUTTON
        },
    );
    p.stroke_rect(
        &box_rect,
        if checked {
            palette::ACCENT
        } else {
            palette::LINE
        },
    );

    if checked {
        // Two strokes rather than a glyph, so the tick stays crisp at any DPI and
        // does not depend on a font having the character.
        p.line(
            box_rect.left + 4,
            box_rect.top + 9,
            box_rect.left + 7,
            box_rect.bottom - 4,
            palette::BG,
        );
        p.line(
            box_rect.left + 7,
            box_rect.bottom - 4,
            box_rect.right - 4,
            box_rect.top + 4,
            palette::BG,
        );
    }

    let colour = if enabled {
        palette::TEXT
    } else {
        palette::MUTED
    };
    p.text(r.left + BOX + 12, r.top + 2, label, colour, &fonts.body);
}

fn client_size(hwnd: HWND_T) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a live RECT and window.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}

fn invalidate(hwnd: HWND_T) {
    // SAFETY: a null rect means the whole window.
    unsafe { InvalidateRect(hwnd, null_mut(), 0) };
}

fn track_leave(hwnd: HWND_T) {
    // SAFETY: a standard tracking request with a null rect, meaning the whole
    // client area.
    unsafe {
        let mut t = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            dwHoverTime: 0,
            hwndTrack: hwnd,
        };
        TrackMouseEvent(&mut t);
        let _ = hwnd;
    }
}

fn centre(hwnd: HWND_T, w: i32, h: i32) {
    // SAFETY: plain metrics queries.
    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (sw - w) / 2;
    // A third of the way down rather than half: slightly above centre reads as
    // deliberate, and leaves room for the taskbar.
    let y = (sh - h) / 3;
    // SAFETY: a standard reposition without a size change.
    unsafe { SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER) };
}

/// The shell folder picker.
fn browse_for_folder(owner: HWND_T) -> Option<PathBuf> {
    use windows_sys::Win32::UI::Shell::{
        ILFree, SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
        BROWSEINFOW,
    };

    let title = wide("Choose where PadForge should be installed");
    let bi = BROWSEINFOW {
        hwndOwner: owner,
        pidlRoot: null_mut(),
        pszDisplayName: std::ptr::null_mut(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };

    // SAFETY: `bi` is fully initialised and points at a 260-unit buffer.
    let pidl = unsafe { SHBrowseForFolderW(&bi) };
    if pidl.is_null() {
        return None;
    }

    let mut buf = [0u16; 260];
    // SAFETY: `pidl` came from the picker and `buf` is 260 units, as required.
    let ok = unsafe { SHGetPathFromIDListW(pidl, buf.as_mut_ptr()) };

    // The PIDL must be freed whichever way this goes.
    // SAFETY: a PIDL from SHBrowseForFolderW is freed by this, and only this.
    unsafe { ILFree(pidl) };

    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}
