# Upstream

This repository is **not a fork**. Every line is written here, from scratch, in Rust.

The projects below are read for their methods and, in two cases, driven over their own network
APIs. None of their code is copied, vendored or linked.

| Area | Reference | Role here |
|---|---|---|
| Sensor drivers, pin control, output timers on a Raspberry Pi | [Mycodo](https://github.com/kizniche/Mycodo) (GPL-3.0) | runtime dependency on the real shelf, reached over its REST API only |
| Smart plug firmware with power metering | Tasmota, ESPHome | planned MQTT rig, reached over MQTT only |
| Crop recipe idea (climate as data, stage by stage) | MIT OpenAg `openag_brain` (archived 2018), OpenFarm (archived 2025) | design reference; their recipe files are not reused |
| Farm record keeping | farmOS | planned export target |
| Agent-driven cultivation on Mycodo | mycodo-hermes-skill (MIT) | design reference for the decision cycle and safety verification step |
| Hydroponic lettuce bands | Cornell CEA Hydroponic Lettuce Handbook | recipe numbers, cited in the recipe |
| Rice in plant factories | Frontiers in Plant Science 2026; Plants (MDPI) 2026 | recipe numbers, cited in the recipe |

Trademarks of the projects above belong to their owners and appear here only to name them.
