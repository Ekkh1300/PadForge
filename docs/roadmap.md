# Roadmap

Kept short on purpose. A roadmap longer than this is a list of things nobody is
going to do, and an inaccurate one is worse than none — it dates the moment
anything slips.

## Done

Things that shipped, so they do not get proposed twice:

- DualShock 4 over USB and Bluetooth, including the sensors, touchpad and
  battery
- Axis shaping: deadzone, response curve, anti-deadzone, sensitivity,
  inversion, temporal smoothing
- Full button remapping, stick-click emulation, per-game profiles with
  automatic switching
- Motion aiming to mouse and to either stick, including the 1€ filter
- Lightbar colour, brightness and seven animation modes, with speed for all of
  them
- Rumble, dual-shape output (Xbox 360 or DualShock 4), virtual slot selection
- Steering wheels, learned per control, with three response presets
- A test page that checks all 20 parts of the pad
- Output monitoring, latency and per-axis plots
- Installation and uninstaller, no administrator rights needed
- Configuration versioning with migrations and recovery from a damaged file
- `padforge --check` for headless validation
- A one-button bug report that redacts the device serial

## Next

Ordered by how much they are worth, not by how hard they are.

**Better failure messages when the driver is missing.** It falls back to
monitor-only and says so, but it cannot yet tell the difference between ViGEmBus
not being installed and it being installed and refusing to start — which are
different problems with the same symptom.

**Per-axis calibration for drift that develops.** The resting centre is captured
once. A stick that starts drifting a week later is a real failure mode with no
way to re-capture without restarting.

**Lightbar effects that read the pad.** Pulse on the heartbeat, flash on a hit,
a colour that follows the trigger. All possible — the pad accepts a colour
750 times a second — and none built, because none was asked for.

**Russian and Japanese documentation.** The interface is already structured for
it. Japanese needs a font with CJK glyphs embedded, which is a binary size and
licensing decision rather than a translation one, so it is not a weekend.

## Deliberately not planned

**Force feedback in the DualShock 4 output shape.** The driver's DS4 target has
no notification channel for incoming rumble. It cannot be fixed from this side at
any effort. The Xbox 360 shape does rumble, and that is the honest answer rather
than a workaround.

**Telemetry.** No usage data is collected and none will be. It would need a
server, a privacy policy and a consent flow, and there is nothing that could be
learned from it that the bug reports do not already say.

**Code signing.** SmartScreen will warn until PadForge has a certificate from a
real authority. That costs money every year and is a decision for the
maintainer. In the meantime `SHA256SUMS.txt` ships with every release, so a
download can be checked without trusting us.

**A second platform.** Linux and macOS would both need a different virtual
gamepad, which means a different driver, which is a different project.