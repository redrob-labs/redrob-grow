# Redrob Grow

Redrob Grow is an agentic grower that keeps crops alive in rooms built for people.

한국어: [README.ko.md](./README.ko.md)

## What it is

An office is a bad greenhouse. The air conditioner follows the people, not the plant; its air is
dry; CO2 climbs when the room is full and falls at night; the only light you can count on is the
LED you bought. Redrob Grow is the small program that sits between a crop recipe and a shelf of
cheap hardware and keeps the plant inside its targets anyway, learns the room's daily pattern, and
writes down everything it did so a grow becomes data.

The crops it ships with are the ones nobody publishes indoor numbers for: **radish sprouts**
(7 days, to check the rig), **butterhead lettuce** (35 days, the first crop the agent really
drives) and **paddy rice** (110 days, a Korean japonica, the staple). Lettuce recipes exist by
the hundred; open indoor rice data does not exist at all. That is the gap this repository is for.

One Rust workspace, built from scratch:

| Crate | Does |
|---|---|
| `grow-recipe` | A crop recipe as TOML: stages, target bands, light schedule. Validated, embedded. |
| `grow-hal` | Sensors and switches behind one trait. A simulated office rig for development and tests; a Raspberry Pi running [Mycodo](https://github.com/kizniche/Mycodo) over its REST API for the real shelf. |
| `grow-safety` | Hard limits in code. Clips every command to a maximum on-time, enforces cooldowns, refuses to heat water that is already too hot, forces offending switches off. Takes no suggestions. |
| `grow-agent` | The loop: observe, read the stage, learn the room (48 hour-of-day bins, weekday and weekend apart), propose, pass the guard, act, report. A deterministic rule planner today; the model-backed planner uses the same observation and the same four tools. |
| `grow-run` | A grow is a folder: `run.json`, `cycles.jsonl` (one report per cycle), `photos/`, `harvest.json`, and a summary with kWh per gram. |
| `grow-node` | The binary on the shelf. `grow-node init`, `grow-node run node.toml`, `GET /state` on loopback. |
| `grow-app` | The desktop dashboard (egui): readings against bands, what the agent decided and why, the guard's alarms, a 48 h chart, and a four-step setup wizard that writes `node.toml`. |

No JavaScript, no web view, no cloud. The node runs on the shelf's own computer and the
dashboard talks to it on your LAN.

## Try it with no hardware

```sh
cargo run -p grow-app
```

The dashboard opens on the simulated office shelf: a winter or summer Korean office, lettuce by
default, fast-forwarded. Switch recipes, watch the heater pre-heat an hour before the air
conditioner comes on, read the reasons the agent gives for each command.

```sh
cargo run -p grow-node -- init          # writes node.toml for the simulated shelf
cargo run -p grow-node -- run           # serves http://127.0.0.1:7474/state
```

Point the dashboard's **grow-node over HTTP** source at that address.

## Put it on a real shelf

The shelf that the recipes assume costs about 600,000 to 900,000 KRW and fits one rack:
[docs/HARDWARE.md](docs/HARDWARE.md). Set up the Raspberry Pi with Mycodo, create an API key,
then open the dashboard's **Setup** tab: name the shelf, pick the crop, paste the Mycodo address
and key, paste the unique id of each input and output, save `node.toml`, and run `grow-node run
node.toml` on the Pi. The loop starts, the folder under `runs/` fills, and the dashboard shows the
shelf.

The node's HTTP API is unauthenticated and bound to loopback by default. Reach it from another
machine over an SSH tunnel; do not put it on the open network.

## Status

Stage 0 (bootstrap) done: recipes, simulated office rig, Mycodo rig, safety guard, rule planner
with room learning, run log, node binary, dashboard with setup wizard. 41 tests, Linux and
Windows CI. No real plant has been grown with it yet; the first lettuce run is Stage 1. Stages and
their pass conditions: [docs/ROADMAP.ko.md](docs/ROADMAP.ko.md).

## License

MIT OR Apache-2.0, at your option. Copyright 2026 Janghoon Lee (Redrob) and contributors.
