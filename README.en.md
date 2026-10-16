<p align="center">
  <img src="icon.png" alt="PadForge" width="96">
</p>

<h1 align="center">PadForge</h1>

**Give your DualShock 4 a native voice on Windows.**

> [فارسی](README.md)

Windows games speak XInput; a DualShock 4 speaks HID. That mismatch is the whole
reason this app exists: PadForge reads the pad's real HID report, reshapes every
axis, remaps every button, and republishes the result as a virtual gamepad —
either an **Xbox 360** or a **DualShock 4**, whichever you pick.

Written in Rust, with an egui interface built for tuning rather than for showing
off.

---

## Screenshots

| Dashboard — live report, rumble, page shortcuts | Test — every part scored |
|---|---|
| <img src="docs/ui-dashboard.png" alt="Dashboard" width="420"> | <img src="docs/ui-test.png" alt="Test page" width="420"> |

| Output — what games see, and the pad's shape | Analog pedals — wheel and pedals |
|---|---|
| <img src="docs/ui-output.png" alt="Output page" width="420"> | <img src="docs/ui-racing.png" alt="Analog pedals page" width="420"> |

| Controller — sticks, triggers, lightbar | Settings |
|---|---|
| <img src="docs/ui-controller.png" alt="Controller page" width="420"> | <img src="docs/ui-settings.png" alt="Settings page" width="420"> |

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
- **Lightbar.** Colour, brightness, and animation — steady, breathe,
  flash, **rainbow**, blue, pulse fade, and **off** — written back to
  the pad. The speed is adjustable for **every animated mode**, rainbow
  included. `Off` keeps your colour and brightness underneath, so
  switching back restores what you had.
  **rainbow**, blue, pulse-fade), written back to the pad. The speed is
  adjustable for **every animated mode**, rainbow included.
- **Pick the output shape.** Whichever pad you plug in, Windows can see it as an
  **Xbox 360** or a **DualShock 4**. That is not cosmetic: a DualShock 4 is a HID
  device, so programs that talk to a pad directly — emulators, Steam, anything
  that wants Share and Options by name — see a real DS4, while games that only
  understand Xbox pads will not list it at all. The cost is stated on the same
  page: in the DualShock 4 shape a game's rumble does not come back, because the
  driver's DS4 target has no notification channel.
- **Virtual slot.** Publish as Player 1 to Player 4, or let the system choose.
  The driver picks the slot, so when the requested one is taken the app says
  honestly where the pad ended up rather than claiming the request took.
- **Analog pedals.** Draws the steering axis as a real wheel and turns it live from the
  pad or from a wheel, and samples the response curve on the actual motor filter
  rather than on a hand-written approximation — so if the map and the motor ever
  disagree, the map is to blame. Three presets, **Sim**, **Arcade** and **Drift**,
  each one a coherent shape across the wheel and both pedals.
- **Real wheels.** Any HID device that calls itself a joystick is read, with no
  table of supported models in the code. Every target is learned by moving that
  control, and each has its own deadzone and sensitivity. Controls are
  listed by what they are — *X axis*, *Z axis*, *X rotation* — because
  "Rx" in a dropdown tells you nothing.
- **Test page.** A pad can be assumed healthy and be wrong. Operate each control
  once and its row turns green and stays green — a switch that worked once works
  every time, and the one that did not is what you came to find. 20 parts, from
  the sticks and triggers through all four d-pad directions, the stick clicks,
  Share and Options, the touchpad and the motion sensors.
- **Calibration.** Learns each pad's resting stick position automatically, so a
  worn stick still reads centred.
- **Diagnostics.** The raw HID bytes of the last report next to every field they
  were read as, with the offset each came from in its own column, and a freeze
  button because a live buffer changes at 755 Hz. Every decoding mistake looks
  exactly like this page: the bytes are right and the field is read from the
  wrong place, and nothing anywhere disagrees.
- **Check an installation without starting it.** `padforge.exe --check` prints a
  checklist with a verdict, writes nothing, and exits non-zero if something is
  wrong. `--json` for a script. Useful on somebody else's machine, which is most
  of the time.
- **Degrades instead of failing.** If the virtual gamepad driver is missing,
  PadForge says so, names ViGEmBus and where to get it, and carries on as a
  monitor: the pad is still read, every control still works, the values are
  still visible.
- **A bug report in one button.** Settings then Report a bug collects the log,
  your settings and your profiles into one file in Downloads, with the device
  serial removed. A crash writes its own dump before the process disappears,
  because the default handler's message goes to a debugger nobody has attached
  to.
- **Your settings survive an upgrade.** Every file declares a schema version and
  is migrated forward. A file from a *newer* build is read but never written
  back, so running an old version cannot delete what it does not understand; a
  file that will not parse is copied aside before anything overwrites it; and
  one damaged profile does not cost you the others.
- **Stays out of the way.** Notification-area icon, close-to-tray, pause toggle,
  global hotkeys with an in-app capture UI, and per-user autostart.

## Requirements

- Windows 10 or 11.
- The **ViGEmBus** virtual gamepad driver, for input to reach games. Without it
  PadForge still runs as a viewer, and the installer and the Output page both say
  so.
  - 64-bit: <https://github.com/nefarius/ViGEmBus/releases>
- A Rust toolchain for building: <https://rustup.rs/>

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

Options: `--no-desktop-shortcut`, `--autostart`, `--no-launch`.

![the installer](docs/installer.png)

The installer is drawn with native Win32 and GDI rather than a GPU renderer: it
has no dependencies beyond `windows-sys`, and it works even on a machine whose
graphics driver is unhappy.

## Command line

```
padforge.exe                  open the window
padforge.exe --tray           start in the notification area, no window
padforge.exe --profile Name   start on that profile instead of the last one
padforge.exe --check          check the installation and exit; writes nothing
padforge.exe --check --json   the same, machine-readable
padforge.exe --help
```

`--check` opens no window, starts no engine and writes no files, which is what
makes it safe to run on somebody else's machine. Every check is reported
independently rather than stopping at the first failure, and the exit code is
non-zero only when something actually failed — a missing driver with
monitor-only chosen is a working installation that does one thing less.

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

Complete and tested: **279 tests pass**, `cargo fmt` and `cargo clippy` are clean
under `-D warnings`, and rustdoc builds without warnings. CI runs all of it on
every push.

**Tested against real hardware.** A DualShock 4 v2 arrived over Bluetooth and then
over USB, and found eight bugs that no unit test could have: all of them produced
clean data rather than an error, so nothing crashed and nothing looked wrong until
the values were checked against physics.

- The Bluetooth report id is `0x11`, not `0x01`. Only `0x01` was accepted, so
  every Bluetooth frame was discarded and a connected pad presented as one that
  never reports.
- Bluetooth sticks sit at offset 3, not 4. At offset 4 the d-pad byte is read as
  the right stick's X axis.
- **The gyro and accelerometer were swapped.** With the pad lying still the
  accelerometer read 0.003 g, which is not a state a pad can be in. Correct, it
  reads 0.99 g.
- A Bluetooth DS4 exposes several HID collections at once. The code opened the
  first, which succeeds and then **blocks in `read()` forever**. Silent interfaces
  are now identified and stepped over.
- **The output report never reached the pad.** The write path used the feature
  report buffer, which is 64 bytes on this pad, while the Bluetooth output report
  is 78 — so every write was refused. Input arrived in full, which is exactly why
  it looked like two separate bugs.
- **The USB report layout was wrong.** The report id landed in the wrong byte, so
  it was never `0x05`, and the RGB colour landed on offsets 1 to 3, where the
  capability mask lives.
- **The lightbar only ran Steady.** The mode ComboBox was the only control that
  never pushed the profile to the engine, and the engine takes its own copy of the
  lightbar once, when a profile is applied.
- **The app never noticed a change of connection.** Pull the pad off Bluetooth and
  plug it in over USB and the app searched for a pad forever.

That third bug is also why the mouse moved on its own: a wrong gyro offset reads
hundreds of times too large and feeds straight into `SendInput`. Measured with the
pad resting on a desk, the pointer moved **6 px in two seconds**; beforehand it was
on track for thousands.

Final measurement on the same pad: **529 reports at 755 Hz** over Bluetooth, 1.33 ms
between frames.

On method: the test fixtures are the bytes the device actually sent, and
`cargo run -p padcore --bin capture-fixture` regenerates them. The tests assert
physical plausibility rather than byte equality — a still pad reads about 1 g, and
neither 0 g nor 4 g is a state a pad can be in. A fixture typed by hand is exactly
what ends up enshrining the bug.

**Newest hardware check:** selecting the DualShock 4 output shape made Windows
enumerate a second Sony controller, `HID\VID_054C&PID_05C4`, while the physical pad
stayed on `VID_054C&PID_09CC`.

The interface is drawn with **wgpu / Direct3D 12** and has been seen rendering
correctly on this machine. Previously every egui/glow window on the AMD
RX 6700 XT rendered as a solid white rectangle — a minimal egui example sharing no
code renders identically white, so that was a graphics driver problem rather than
an application one. See [issue #1](https://github.com/Ekkh1300/PadForge/issues/1).

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
    output.rs      virtual gamepad publication (Xbox 360 or DualShock 4)
    wheel.rs       HID wheel parsing and target learning
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

## Is it safe to run?

PadForge is **not code-signed**, so Windows SmartScreen will warn the first time
you run the installer. Click **More info** > **Run anyway**.

A self-signed certificate would not help, and this is worth being precise about:
SmartScreen treats an untrusted publisher as *worse* than an unsigned one,
because the signature is present and the name on it means nothing. Clearing the
warning needs a certificate from a real authority, which costs money every year.
That is a decision for the maintainer, not something a build can do.

Every release ships a `SHA256SUMS.txt` beside the installer, so you can check
what you downloaded without trusting us:

```powershell
Get-FileHash .\PadForge-Setup.exe -Algorithm SHA256
Get-Content .\SHA256SUMS.txt
```

Both lines show the same hash. The release job computes it in the same run that
built the file, so the two cannot drift apart.

No telemetry is collected, and none would be even if it could be.

## Getting help

- Something does not work: press **Report a bug** first, then file an issue.
  The report carries the log and a summary, so the first reply will not be "can
  you send your log".
- How do I...: [Discussions](https://github.com/Ekkh1300/PadForge/discussions).
- To help: [CONTRIBUTING.md](CONTRIBUTING.md).
- What is planned, and what deliberately is not:
  [the roadmap](docs/roadmap.md).

## Licence

MIT. Written from scratch as a clean-room reimplementation: the *idea* of DS4Windows
was studied, but none of its code was copied. Behavioural details came from the
published DS4Windows source and from observing DS4 HID reports.

ViGEmBus is a separate, separately licensed driver project by nefarius and is not
distributed with PadForge: <https://github.com/nefarius/ViGEmBus>