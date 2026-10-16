# Discussions

GitHub Discussions is enabled for questions that are not bugs.

**Which one is which:**

- *Something does not work* → an issue, and press **Report a bug** first.
- *How do I…* → a discussion.
- *It works but I would prefer it did something else* → a feature request, and
  the situation matters more than the solution you have in mind.

Some things that come up, and where they live:

- **Per-game profiles.** The auto-switch rules, what can be different between
  them, and why a rule stops matching.
- **Wheel setups.** Learning a control, the presets, and why a named axis might
  not be the one you meant.
- **Motion aiming.** Why the pointer drifts, why a profile that felt right stops
  feeling right, and what the deadzone and gain actually do.
- **Diagnostics.** The Diagnostics page shows raw HID bytes next to the values
  they were read as. Most "my stick does X" questions are answered by moving a
  control and watching which byte changes.

**Before asking about a bug**, run `padforge.exe --check` and paste the output.
It answers "would this installation work" in one line and settles most first
questions.