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
| `grow-hal` | Sensors and switches behind one trait. A simulated office rig for development and tests; [Tasmota](https://tasmota.github.io/) smart plugs and sensor nodes over HTTP (no Pi, no soldering, plugs meter their own kWh); a Raspberry Pi running [Mycodo](https://github.com/kizniche/Mycodo) over its REST API. |
| `grow-safety` | Hard limits in code. Clips every command to a maximum on-time, enforces cooldowns, refuses to heat water that is already too hot, forces offending switches off. Takes no suggestions. |
| `grow-agent` | The loop: observe, read the stage, learn the room (48 hour-of-day bins, weekday and weekend apart), propose, pass the guard, act, report. Two planners behind one trait: deterministic rules, and `EnginePlanner`, which hands the same observation, the rules' own proposal, the operator's messages and the latest photo to a model through four tools (`switch_on`, `switch_off`, `note`, `ask_human`) and falls back to the rules when the model does not answer. |
| `grow-run` | A grow is a folder: `run.json`, `cycles.jsonl` (one report per cycle), `photos/`, `harvest.json`, and a summary with kWh per gram. |
| `grow-node` | The binary on the shelf. `grow-node init`, `grow-node run node.toml`, `GET /state` and `POST /answer` on loopback, a UDP hello on the LAN so the dashboard finds it. The model is the shared Redrob Code daemon on the same machine by default, or any OpenAI-compatible `/v1`. |
| `grow-app` | The desktop dashboard (egui): readings against bands, what the agent decided and why, the guard's alarms, a 48 h chart, several nodes side by side, the agent's notes and questions with a reply box, and a five-step setup wizard that writes `node.toml`. |

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

## Who decides

Every cycle (ten minutes by default) the node reads the shelf, works out the recipe stage, and
asks a planner. The **rule planner** is deadband control plus two office habits (pre-heat before
the air conditioner comes on, move the light into the hours when people breathe CO2 into the
room). The **engine planner** sends the model everything the rules saw plus what the rules would
do, and the model answers with tool calls. Whatever comes back goes through `grow-safety`, which
clips, refuses and forces off without asking anyone. The model can ask the operator a question
(`ask_human`); the dashboard shows it and your reply reaches the next cycle's prompt.

```toml
[planner]
kind = "engine"        # or "rules"
endpoint = ""          # empty: the Redrob Code daemon on this machine; else any OpenAI-compatible /v1
model = "redrob/auto"
photo_dir = "photos"   # scripts/snap.sh fills it; the newest photo rides along
```

## Put it on a real shelf

The shelf that the recipes assume costs about 600,000 to 900,000 KRW and fits one rack:
[docs/HARDWARE.md](docs/HARDWARE.md). The cheapest wiring is a Tasmota smart plug per switch
(light, heater, fan, pump) and, if you want readings, one ESP32 running Tasmota with the sensors
on I2C; or a Raspberry Pi running Mycodo. Open the dashboard's **Setup** tab: name the shelf, pick
the crop, pick the hardware, give each switch its plug address (or each Mycodo input and output its
unique id), pick the brain, save `node.toml`, and run `grow-node run node.toml` on the shelf's
computer. The loop starts, the folder under `runs/` fills, the dashboard's **Find on LAN** lists
the node, and two nodes show side by side for an A/B grow.

The node's HTTP API is unauthenticated and bound to loopback by default. Reach it from another
machine over an SSH tunnel; do not put it on the open network.

## Status

Everything that can be done without a plant is done: recipes with per-stage hard ceilings,
simulated office rig, Tasmota and Mycodo rigs, safety guard, rule planner with room learning,
engine planner with operator dialogue, run log with measured kWh, node binary with LAN discovery,
dashboard with multi-node view and setup wizard, release workflow. 59 tests, Linux and Windows CI.
No real plant has been grown with it yet; the first row of [docs/RESULTS.md](docs/RESULTS.md) is
Stage 1. Stages and their pass conditions: [docs/ROADMAP.ko.md](docs/ROADMAP.ko.md). To add a
crop: [docs/CONTRIBUTING-RECIPES.md](docs/CONTRIBUTING-RECIPES.md).

## License

MIT OR Apache-2.0, at your option. Copyright 2026 Janghoon Lee (Redrob) and contributors.
