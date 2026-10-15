<p align="center">
  <img src="https://github.com/Ekkh1300/PadForge/releases/download/v1.0.1/icon-512.png" alt="PadForge" width="128">
</p>

## PadForge 1.0.1

Give your DualShock 4 a native voice on Windows.

Windows games speak XInput; a DualShock 4 speaks HID. That mismatch is the whole
reason this app exists: PadForge reads the pad's real HID report, reshapes every
axis, remaps every button, and republishes the result as a virtual gamepad —
either an **Xbox 360** or a **DualShock 4**, whichever you pick.

### Install

<p align="center">
  <img src="https://github.com/Ekkh1300/PadForge/releases/download/v1.0.1/installer.png" alt="The PadForge installer" width="420">
</p>

Run `PadForge-Setup.exe`. A small dialog opens: pick where to install (or accept
the default) and tick the shortcuts you want.

The install is for your own user account and **needs no administrator rights**.
It goes to `%LOCALAPPDATA%\Programs\PadForge` and registers itself under
**Settings ‹ Apps** so Windows can uninstall it normally. Your profiles in
`%APPDATA%\PadForge` are left alone, so reinstalling picks up where you left off.

You also need the **ViGEmBus** driver, without which input never reaches a
game. The installer checks for it and offers to open the download page if it is
missing:

<https://github.com/nefarius/ViGEmBus/releases> — the 64-bit build.

### What it does

- **Reads the real thing.** Both USB and Bluetooth report layouts are decoded
  from the raw HID bytes, including sticks, triggers, the touchpad, the motion
  sensors, and battery level.
- **Shapes every axis.** Deadzone, response curve (linear, exponential, custom
  bezier, stepped), anti-deadzone, sensitivity, inversion, and temporal
  smoothing, applied as a fixed four-stage chain.
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
- **Lightbar.** Colour, brightness, and animation — steady, breathe, flash,
  rainbow, blue, pulse fade, and **off** — written back to the pad. The speed is
  adjustable for every animated mode, rainbow included.
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
- **Analog steering and pedals.** Draws the steering axis as a real wheel and
  turns it live from the pad or from a wheel, and samples the response curve on
  the actual motor filter rather than on a hand-written approximation — so if the
  map and the motor ever disagree, the map is to blame. Three presets, **Sim**,
  **Arcade** and **Drift**, each one a coherent shape across the wheel and both
  pedals.
- **Real wheels.** Any HID device that calls itself a joystick is read, with no
  table of supported models in the code. Every target is learned by moving that
  control, and each has its own deadzone and sensitivity. Controls are listed by
  what they are — *X axis*, *Z axis*, *X rotation* — because "Rx" in a dropdown
  tells you nothing.
- **Test page.** A pad can be assumed healthy and be wrong. Operate each control
  once and its row turns green and stays green — a switch that worked once works
  every time, and the one that did not is what you came to find. 20 parts, from
  the sticks and triggers through all four d-pad directions, the stick clicks,
  Share and Options, the touchpad and the motion sensors.
- **Calibration.** Learns each pad's resting stick position automatically, so a
  worn stick still reads centred.
- **Stays out of the way.** Notification-area icon, close-to-tray, pause toggle,
  global hotkeys with an in-app capture UI, and per-user autostart.

### Fixed in 1.0.1

Six reports, all from someone actually using it:

- **The lightbar only ever ran Steady.** The mode picker was the one control on
  the card that never pushed the profile to the engine — colour, brightness and
  speed all did. The engine keeps its own copy of the lightbar, taken once when a
  profile is applied, so the setting moved in the store and the light never
  changed. The cycle hotkey had the same defect. Both are fixed, and a test now
  fails if any animated mode stops moving.
- **Rainbow had no speed slider.** This one was mine: Rainbow was grouped with
  Blue as "fixed colour", and that one flag was being asked two different
  questions — whether a colour picker is worth showing, and whether speed does
  anything. Rainbow builds its own colours, so a picker would be a lie, but it
  is very much animated and its speed is the whole character of the mode. The
  failure was invisible: a Rainbow with no speed control still looks exactly like
  a Rainbow. The two questions are separate now, and a test checks the flag
  against what the light actually does.
- **There was no off.** If the colour itself was the problem, the only route to a
  dark lightbar was to keep nudging brightness down until it was black — which is
  not the same thing. A pad draws no current at all when its light is off, and on
  a light-coloured shell the difference is visible from across a room. `Off` is a
  real mode now: it keeps your colour and brightness underneath, so switching
  back restores what you had, and it hides the brightness and speed sliders
  because neither does anything.
- **The installer left a console behind.** The install itself was always sound
  and always finished quickly; what lingered was a black console window that
  stayed until PadForge closed. The launched app inherited the installer's
  console, so it is now started detached. If you script an install, note that
  PowerShell's `Start-Process -Wait` waits on the whole child tree, which
  includes PadForge; pass `--no-launch` for a scripted install.
- **The quiet text was unreadable.** Every hint under every control was drawn at
  2.8:1 against the card — the colour looked like information and was not. It is
  now above 4.5:1 everywhere, and a test computes the ratio for every text tier
  on every surface so it cannot quietly come back.
- **The four Settings cards did not line up.** Two `ui.columns` calls back to
  back: each fills its columns to the height of the tallest card and returns, so
  the second row started about 500 px below the first. Both rows are now measured
  from a shared top.

Also new: the interface has a hierarchy — three card weights instead of one, so
a page has somewhere for the eye to land first; the navigation says what each
page is for on hover; "What games see" plots both sticks as live dots rather than
bars, which cannot show a diagonal; the Racing page is renamed **Analog pedals**,
because it does analogous work for a pad and the old name described only the
wheel case; and the controller preview and the wheel are drawn with real volume —
gradients, shadows, and a lightbar that spills onto the shell the way it does in
life.

### Status

Complete and tested: **234 tests pass**, `cargo fmt` and `cargo clippy` are clean
under `-D warnings`, and rustdoc builds without warnings. CI runs all of it on
every push.

**Tested against real hardware.** A DualShock 4 v2 arrived over Bluetooth and
then over USB, and found eight bugs that no unit test could have: all of them
produced clean data rather than an error, so nothing crashed and nothing looked
wrong until the values were checked against physics.

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
  report buffer, which is 64 bytes on this pad, while the Bluetooth output
  report is 78 — so every write was refused. Input arrived in full, which is
  exactly why it looked like two separate bugs.
- **The USB report layout was wrong.** The report id landed in the wrong byte, so
  it was never `0x05`, and the RGB colour landed on offsets 1 to 3, where the
  capability mask lives.
- **The lightbar only ran Steady.** See above.
- **The app never noticed a change of connection.** Pull the pad off Bluetooth and
  plug it in over USB and the app searched for a pad forever.

That third bug is also why the mouse moved on its own: a wrong gyro offset reads
hundreds of times too large and feeds straight into `SendInput`. Measured with the
pad resting on a desk, the pointer moved **6 px in two seconds**; beforehand it was
on track for thousands.

Final measurement on the same pad: **529 reports at 755 Hz** over Bluetooth, 1.33 ms
between frames.

**Newest hardware check:** selecting the DualShock 4 output shape made Windows
enumerate a second Sony controller, `HID\VID_054C&PID_05C4`, while the physical pad
stayed on `VID_054C&PID_09CC`.

The interface is drawn with **wgpu / Direct3D 12** and has been seen rendering
correctly on this machine. See [issue #1](https://github.com/Ekkh1300/PadForge/issues/1).

### Licence

MIT. Written from scratch as a clean-room reimplementation: the *idea* of
DS4Windows was studied, but none of its code was copied. Behavioural details came
from the published DS4Windows source and from observing DS4 HID reports.

ViGEmBus is a separate, separately licensed driver project by nefarius and is not
distributed with PadForge: <https://github.com/nefarius/ViGEmBus>