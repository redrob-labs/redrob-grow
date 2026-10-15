# Contributing a crop recipe

A recipe without a grow behind it is a guess. This repository exists for the numbers, so the rule
is one line long:

**One new crop = one pull request = one recipe TOML + one `runs/` folder from a real shelf.**

## What a pull request carries

1. `recipes/<crop>-<variety>.toml`. Stages from day 0, target bands per stage, light schedule,
   `notes` the agent reads, `sources` you took the bands from. Add `[stages.hard]` only for a
   ceiling that kills the crop (rice pollen at 35 C); it can only tighten the guard's floor.
2. `runs/<sown_on>-<recipe-id>-<shelf>/` exactly as `grow-node` wrote it: `run.json`,
   `cycles.jsonl` (every cycle, do not thin it), `photos/` (at least one per week),
   `harvest.json` with fresh weight and plant count, and `operator.jsonl` if you talked to the
   agent. Do not edit these files; if a cycle is embarrassing it stays.
3. One paragraph in `docs/RESULTS.md`: crop, days, in-band ratio, kWh (measured if you had a
   metering plug), kWh per gram, what you would change in the recipe next time.

Register the recipe in `crates/recipe/src/lib.rs` (`builtin`) so `grow-node recipes` lists it
and `cargo test` validates it; the test fails on inverted bands, out-of-order stages, or a stage
past `days`.

## What is not a contribution

- A recipe copied from a seed packet or a paper with no run behind it. Open a discussion instead.
- A run on the simulated rig. The simulator is for development; its numbers are not data.
- Edits to another contributor's `runs/` folder.

## Privacy

`cycles.jsonl` holds sensor values and the agent's reasoning, nothing about you. `run.json` has an
`operator` field: put a handle there, or leave it empty. Photos show the shelf; check what else
they show before you commit them.

## Licence

By contributing you license the recipe and the run data under the repository's licence
(MIT OR Apache-2.0). The data is the point of the project; it has to be free to reuse.
