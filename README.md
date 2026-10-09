# PadForge

**Give a PlayStation DualShock 4 a native voice on Windows.**

Windows games speak XInput. A DualShock 4 speaks HID. That mismatch is the
whole reason this app exists: PadForge reads the pad's real HID report, reshapes
every axis, remaps every button, and republishes the result as a virtual Xbox 360
gamepad that any game already knows how to talk to.

Written in Rust, with an egui interface built for tuning rather than for
showing off.

---

## What it does

- **Reads the real thing.** Both USB and Bluetooth report layouts are decoded
  from the raw HID bytes, including sticks, triggers, the touchpad, the motion
  sensors, and battery level.
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
  global hotkeys with an in-app capture UI, and per-user autostart with no
  administrator rights needed.

## Requirements

- Windows 10 or 11.
- The **ViGEmBus** virtual gamepad driver, for input to reach games. Without it
  PadForge still runs as a viewer, and the Output page says so.
  - 64-bit: <https://github.com/nefarius/ViGEmBus/releases>
- A Rust toolchain for building: <https://rustup.rs/>

## Installing

If you were given a `PadForge-Setup.exe`, run it. It asks where to install and
whether to add shortcuts; nothing needs administrator rights.

```
PadForge-Setup.exe                 install, asking a few questions
PadForge-Setup.exe /S              install silently with the defaults
PadForge-Setup.exe /D=<folder>     install somewhere else
PadForge-Setup.exe --uninstall     remove it again
```

It installs for your user account only, into `%LOCALAPPDATA%\Programs\PadForge`
by default, and registers itself under **Settings > Apps** so Windows can
uninstall it normally. Your profiles in `%APPDATA%\PadForge` are left alone, so
reinstalling picks up where you left off.

## Building

```sh
cargo build --release
```

The binary lands at `target/release/padforge.exe`.

Run it, or start it hidden in the notification area:

```sh
padforge.exe --tray
```

### Building the installer

```sh
build-installer.cmd          # Windows
```

This produces `dist/PadForge-Setup.exe`, a single self-contained file with the
application baked in. From Cargo the same thing is two commands, and the order
matters because the second embeds the first's output:

```sh
cargo build -p padforge --release
cargo build -p padforge-installer --release
```

## Testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

The test suite covers the parts where being subtly wrong is invisible until a
game misbehaves: HID report decoding for both transports, the axis filter chain
at its boundaries, gyro integration and its filters, gesture detection and
re-arming, profile store invariants, and report quantisation.

There is also a performance suite, ignored by default so `cargo test` stays
quick:

```sh
cargo test -p padcore --release --test perf -- --ignored --nocapture
```

The DS4 reports at up to 1000 Hz, so the whole per-report chain has a
millisecond a second to spend. Measured on an idle machine, release build:

| stage | per sample |
|---|---|
| axis filter, exponential smoothing | 75 ns |
| axis filter, 16-wide weighted window | 82 ns |
| gyro integrate + filter | 72 ns |
| gyro to pointer | 127 ns |
| touchpad routing | 29 ns |
| **four axes + gyro + pointer, whole loop** | **626 ns/frame** |

That is about 1600x under the frame budget, or 0.06% of one core. The hot path
also asserts it performs **zero allocations** once warmed, which is what keeps
the HID thread free of the jitter that causes dropped reports.

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
    win.rs         shortcuts and the per-user registry
    build.rs       embeds padforge.exe into the installer
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
`MouseCursor`, because that is what makes gyro aiming on a DS4 feel right. Two
details matter: the DS4 reports angular *rate* rather than angle, so each
report is converted to a position delta and discarded rather than integrated
(which would drift); and the coefficients are calibrated against the raw report
units of 1/16 deg/s, so the module works in counts rather than degrees.

The engine never blocks on the UI. Telemetry goes over a bounded channel with
`try_send`, so a busy interface drops a visual frame instead of ever applying
back-pressure to input.

## Honest limitations

- Games will not see the pad without the ViGEmBus driver installed.
- Pointer injection needs the process to be at least as trusted as the window
  that has focus. Windows refuses `SendInput` across an integrity-level
  boundary, which is the most common reason gyro aiming silently does nothing;
  PadForge detects the refusal and says so in the UI.
- Bluetooth transport is detected for display and for choosing the lightbar
  report layout, but the raw L2CAP path that DS4Windows uses for lower latency
  is not implemented.
- Bluetooth is read through the HID stack Windows already exposes, so a pad
  already paired with Windows works without extra setup.
- Bluetooth transport is detected for display and for choosing the lightbar
  report layout, but the raw L2CAP path that DS4Windows uses for lower latency
  is not implemented.

## Licence

MIT.