# Contributing

PadForge is a single-person project with one rule that matters more than the
rest: **changes that can be checked are welcome, changes that can only be
checked on somebody's desk are not.**

That is not gatekeeping. It is the difference between a report that gets fixed
and one that sits. The project has found eight bugs on real hardware that no
test could have found — and every one of them came back from someone who was
already using it, which is the part that is hard to arrange.

## Getting it running

```sh
git clone https://github.com/Ekkh1300/PadForge
cd PadForge

# Windows, GNU target. The toolchain is what CI uses; see .github/workflows/ci.yml.
cargo test --workspace --target x86_64-pc-windows-gnu
cargo run   --release -p padforge --target x86_64-pc-windows-gnu
```

`cargo fmt` and `cargo clippy -D warnings` must both be clean. CI runs them with
`-D warnings`, so a warning is a failure.

## Before you open a pull request

**A change to a field, a config file, or the on-disk format needs a migration
and a test for it.** `crates/padcore/src/schema.rs` holds the migration chain
and the reasoning. Two rules that are easy to get wrong and expensive to get
wrong:

- A field from a **newer** build must never be written back. Loading it and
  saving normally deletes everything this build did not understand.
- A file that will not parse must be copied aside before anything overwrites it.

**The hot path does not allocate.** `hot_path_does_not_allocate` in
`crates/padcore/tests/perf.rs` counts allocations with a global allocator. A
per-report allocation is invisible in a throughput number that still fits the
budget and shows up as jitter in the HID thread, which is what causes dropped
reports. Run it with `--release`; it is far too noisy in a debug build.

**The performance job gates.** Measured on one machine: 128 ns for the slowest
stage, 590 ns for a full frame. The budgets are 2000 ns and 20000 ns. Those are
15x and 34x — tight enough to catch an algorithmic regression, loose enough that
a loaded CI runner does not fail a build for a reason that is not the code.

## Writing a test that is worth keeping

- **Round-trip the real types.** A test that deserialises into
  `serde_json::Value` proves the shape; it does not prove a user's settings
  survive. Build the old file by serialising the current one and removing keys,
  so the fixture cannot drift.
- **Name it after the failure, not the function.** `a_device_serial_never_reaches_the_archive`
  survives a refactor. `test_strip_serial` does not.
- **Prefer a test that fails for the right reason.** Two bugs in this project
  were found by tests that turned out to be wrong first — one had a bad fixture,
  one had a flag that was dropped. Both were more useful than no test.

## What needs explaining

Every decoder offset in `report.rs` carries a comment saying what went wrong
before. That is deliberate and it should continue: the wrong offset produces
clean data rather than an error, so the next person has nothing but the comment
to go on. If you fix one, say what was wrong.

## Commit messages

The release notes are generated from the log, so a `fix:` or `feat:` prefix
sorts your change into the right section of the changelog. Everything else is
listed under "Everything else" with a note on stderr — it is still published,
but it takes a human to sort it.

## Licence

MIT. PadForge was written from scratch as a clean-room reimplementation: the
*idea* of DS4Windows was studied, but none of its code was copied. Behavioural
details came from the published DS4Windows source and from watching DS4 HID
reports. If you contribute, your work is under the same licence.

ViGEmBus is a separate, separately licensed driver project by nefarius. It is
not distributed with PadForge.