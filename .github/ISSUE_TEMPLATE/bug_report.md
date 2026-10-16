# Reporting a bug

Thank you for reporting one. The most useful thing you can do is press
**Report a bug** in PadForge's Settings page first: it gathers everything below
into one file in your Downloads folder, with your device serial removed, ready
to attach.

## What to write

**What you did, and what happened instead.** Those two sentences are the report.
Not the diagnosis — we can do that part.

**What you expected.** "The stick drifts" and "the stick drifts after the app
has been open a day, but not at first start" are different bugs with the same
symptom, and the second sentence is the one that identifies it.

**The attached file.** Without it, the first thing we will ask for is the log,
so attaching it saves a round trip.

## What not to include

- **A video.** A GIF or screenshot of the *page* showing the wrong value is
  useful. A video of the physical pad is not — we cannot see it and it usually
  arrives as a 12 MB file that tells us less than a description does.
- **Your device serial.** PadForge removes it from the report. If you paste your
  settings file in by hand, remove it yourself: it is a stable hardware
  identifier, and posting one links every bug you ever file to the same pad.
- **Anything about a game specifically**, unless the game is the thing being
  reported. Which game somebody is playing is not our business, and a bug
  report is not the place to volunteer it.

## Before you file

Run `padforge.exe --check` and paste the output if it is failing. It answers
"would this installation work at all" without starting the app, and it usually
settles the question in one line — most reports from users who have not
installed the ViGEmBus driver are the same report.

## Bugs vs questions

If PadForge is doing what it says and you want it to do something else, that is
a feature request. Those are welcome, and the same form works — but say which
you mean, because "it does not work" and "it works but I would prefer" lead to
very different conversations.

## Things that will be closed without a fix

- **Rumble in the DualShock 4 output shape.** The driver's DS4 target has no
  notification channel, so a game's vibration cannot reach the pad. It is a
  limitation of the driver, not a bug in PadForge, and no change here will fix
  it. The Xbox 360 shape does rumble.
- **Games that only accept an Xbox controller**, while you have selected the
  DualShock 4 output shape. Switch on the Output page.