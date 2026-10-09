# PadForge

**Give your DualShock 4 a native voice on Windows.**

> [فارسی](README.md) — این راهنما به فارسی است.

Windows games speak XInput; a DualShock 4 speaks HID. That mismatch is the whole
reason this app exists: PadForge reads the pad's real HID report, reshapes every
axis, remaps every button, and republishes the result as a virtual Xbox 360 gamepad
that any game already knows how to talk to.

Written in Rust, with an egui interface built for tuning rather than for showing
off.

---

## Installing

Download `PadForge-Setup.exe` from the releases page and run it. A small dialog
opens: pick where to install (or accept the default) and tick the shortcuts you
want. Nothing needs administrator rights.

It installs for your user account into `%LOCALAPPDATA%\Programs\PadForge` and
registers itself under **Settings ‹ Apps** so Windows can uninstall it normally.
Your profiles in `%APPDATA%\PadForge` are left alone, so reinstalling picks up
where you left off.

For unattended installs:

```
PadForge-Setup.exe /S                 silent, default choices
PadForge-Setup.exe /D=C:\PadForge     install elsewhere
PadForge-Setup.exe --uninstall        remove it
PadForge-Setup.exe --help             this text
```

You will also need the **ViGEmBus** driver for input to reach games; see below.

![the installer](docs/installer.png)

The installer is drawn with native Win32 and GDI rather than a GPU renderer: it
has no dependencies beyond `windows-sys`, and it works even on a machine whose
graphics driver is unhappy.

## What it does

- **Reads the real thing.** Both USB and Bluetooth report layouts are decoded from
  the raw HID bytes, including sticks, triggers, the touchpad, the motion sensors,
  and battery level.
- **Shapes every axis.** Deadzone, response curve (linear, exponential, custom
  bezier, stepped), anti-deadzone, sensitivity, inversion, and temporal smoothing,
  applied as a fixed four-stage chain.
- **Remaps everything.** Every DS4 control to any XInput button, with optional
  modifier gating. Two preset layouts, or set it up by hand.
- **Emulates stick clicks.** Bind a stick direction to L3/R3, the way the DS4's
  own click does.
- **Motion aiming.** Gyro to mouse, to either stick, or to the triggers, with
  deadzone, rate limiting, a gain cap, and three smoothing strategies including
  the 1€ filter. Mouse output goes through `SendInput`, so it is subject to the
  same UIPI integrity checks as real hardware.
- **Touchpad routing.** As a pointer, as a D-pad, or as swipe gestures, with the
  pad click mapped to left or right mouse button.
- **Profiles.** Per-game bundles of every setting above, switchable manually, by
  hotkey, or automatically when a matching program comes to the foreground.
  Import and export as JSON.
- **Lightbar.** Colour, brightness, and animation (steady, breathe, flash,
  rainbow, blue, pulse-fade), written back to the pad.
- **Calibration.** Learns each pad's resting stick position automatically, so a
  worn stick still reads centred.
- **Stays out of the way.** Notification-area icon, close-to-tray, pause toggle,
  global hotkeys with an in-app capture UI, and per-user autostart.

## Requirements

- Windows 10 or 11.
- The **ViGEmBus** virtual gamepad driver, for input to reach games. Without it
  PadForge still runs as a viewer, and the installer and the Output page both say
  so.
  - 64-bit: <https://github.com/nefarius/ViGEmBus/releases>
- A Rust toolchain for building: <https://rustup.rs/>

## Building

```sh
cargo build --release
```

The binary lands at `target/release/padforge.exe`. Run it, or start it hidden in
the notification area:

```sh
padforge.exe --tray
```

### Building the installer

```sh
build-installer.cmd          # Windows
```

This produces `dist/PadForge-Setup.exe`, a single self-contained file with the
application baked in. From Cargo it is two commands, and the order matters because
the second embeds the first's output:

```sh
cargo build -p padforge --release
cargo build -p padforge-installer --release
```

The icon is generated rather than checked in as an opaque blob, so it can be
edited:

```sh
python tools/make_icon.py
```

## Testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

The suite covers the parts where being subtly wrong is invisible until a game
misbehaves: HID report decoding for both transports, the axis filter chain at its
boundaries, gyro integration and its filters, gesture detection and re-arming,
profile store invariants, and report quantisation.

There is also a performance suite, ignored by default so `cargo test` stays quick:

```sh
cargo test -p padcore --release --test perf -- --ignored --nocapture
```

A DS4 reports at up to 1000 Hz, so the whole per-report chain has a millisecond a
second to spend. Measured on an idle machine, release build:

| stage | per sample |
|---|---|
| axis filter, exponential smoothing | 75 ns |
| axis filter, 16-wide weighted window | 82 ns |
| gyro integrate + filter | 72 ns |
| gyro to pointer | 127 ns |
| touchpad routing | 29 ns |
| **four axes + gyro + pointer, whole loop** | **626 ns/frame** |

About 1600x under the frame budget, or 0.06% of one core. The hot path also
asserts it performs **zero allocations** once warmed, because per-report
allocation shows up as HID thread jitter long before it shows up as throughput
loss.

## Status

Complete and tested: 123 tests pass, `cargo fmt` and `cargo clippy` are clean under
`-D warnings`, and rustdoc builds without warnings. CI runs all of it on every
push.

One thing is **not** verified: how the main interface actually looks. Every
egui/glow window on the machine this was developed on renders as a solid white
rectangle. That is a graphics driver problem rather than an application one — a
minimal egui example sharing no code renders identically white. See
[issue #1](https://github.com/Ekkh1300/PadForge/issues/1).

Four capabilities from DS4Windows are also not ported yet, listed in
[issue #2](https://github.com/Ekkh1300/PadForge/issues/2).

## Where state lives

Everything PadForge writes goes under `%APPDATA%\PadForge`:

```
settings.json      app settings
profiles/          profile store, plus any imported profiles
logs/              rolling log file
```

Deleting that folder is a complete uninstall of its data.

## How it is put together

```
crates/
  padcore/         the engine, with no UI dependency
    report.rs      DS4 HID report decoding
    device.rs      discovery and streaming
    filters.rs     deadzone / curve / smoothing
    gyro.rs        motion integration and filtering
    pointer.rs     gyro/touchpad to real mouse input
    touchpad.rs    touchpad routing and gestures
    mapping.rs     control and target enums
    output.rs      virtual gamepad publication
    profile.rs     profiles and persistence
    engine.rs      the loop that ties it together
  padforge/        the egui application
    app.rs         window, navigation, page routing
    theme.rs       palette and widget styling
    widgets/       live pad preview, bars, colour picker
    pages/         one module per screen
  padforge-installer/
    main.rs        install, uninstall, argument handling
    gui.rs         the installer dialog
    gdi.rs         GDI drawing helpers
    win.rs         shell links and the per-user registry
    build.rs       embeds padforge.exe into the installer
tools/
  make_icon.py     generates the application icon
  check_*.py       verify the PE resources and the shell's icon loader
```

Three threads, with a deliberate split:

```
HID thread        engine thread                     UI thread
+-------------+   +---------------------------+     +------------+
| decode      |-->| filter -> map -> output   |---->| telemetry  |
| report      |   | gyro, touchpad, lightbar |     | commands   |
+-------------+   +---------------------------+<----+------------+
```

The pointer maths in `pointer.rs` is a deliberate port of DS4Windows'
`MouseCursor`, including the rate-not-angle convention and the
truncate-toward-zero remainder cutoff that keeps negative motion unbiased.

The installer's shell-link vtable is declared by hand, because `windows-sys` does
not bind `IShellLink`. A round-trip test guards the layout: writing a shortcut and
reading it back is what proves the offsets are right, and getting them wrong is a
hard crash rather than a wrong answer.

## Licence

MIT. Written from scratch as a clean-room reimplementation: the *idea* of DS4Windows
was studied, but none of its code was copied. Behavioural details came from the
published DS4Windows source and from observing DS4 HID reports.

ViGEmBus is a separate, separately licensed driver project by nefarius and is not
distributed with PadForge: <https://github.com/nefarius/ViGEmBus>