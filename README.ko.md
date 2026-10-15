# Redrob Grow

레드롭 그로우는 사람을 위해 지은 방에서 작물을 살려 두는 에이전트 재배기입니다.

English: [README.md](./README.md)

## 무엇인가

사무실은 나쁜 온실입니다. 에어컨은 식물이 아니라 사람 출퇴근을 따라 돌고, 그 바람은 건조하고,
CO2는 사람이 많을 때 올라가고 밤에 떨어지고, 믿을 수 있는 빛은 직접 산 LED 하나뿐입니다.
레드롭 그로우는 작물 레시피와 값싼 장비 선반 사이에 앉아, 그래도 식물을 목표 범위 안에 붙잡아
두고, 방의 하루 패턴을 배우고, 한 일을 전부 적어 한 번의 재배를 데이터로 만드는 작은 프로그램입니다.

같이 들어 있는 작물은 아무도 실내 수치를 공개하지 않은 것들입니다. **새싹 무**(7일, 장비 점검용),
**버터헤드 상추**(35일, 에이전트가 처음 제대로 운전하는 작물), **벼**(110일, 신동진, 주식).
상추 레시피는 수백 개 있지만 공개된 실내 벼 데이터는 하나도 없습니다. 이 저장소는 그 빈 자리를 위한 것입니다.

처음부터 만든 Rust 워크스페이스 하나:

| 크레이트 | 하는 일 |
|---|---|
| `grow-recipe` | 작물 레시피를 TOML로: 단계, 목표 범위, 조명 시간. 검증·내장. |
| `grow-hal` | 센서와 스위치를 트레이트 하나 뒤에. 개발·테스트용 사무실 시뮬레이션 리그, [Tasmota](https://tasmota.github.io/) 스마트플러그·센서 노드 HTTP 리그(파이 없음, 납땜 없음, 플러그가 kWh를 직접 잼), 라즈베리파이 [Mycodo](https://github.com/kizniche/Mycodo) REST API 리그. |
| `grow-safety` | 코드로 박은 한계. 모든 명령을 최대 켜짐 시간으로 자르고, 쿨다운을 강제하고, 이미 뜨거운 물은 더 못 데우게 거부하고, 위반한 스위치는 강제로 끕니다. 제안을 받지 않습니다. |
| `grow-agent` | 루프: 읽기, 단계 확인, 방 학습(시간대 48칸, 평일·주말 따로), 제안, 가드 통과, 실행, 보고. 플래너 둘이 한 트레이트 뒤에: 결정적 규칙, 그리고 같은 관측·규칙의 제안·운영자 메시지·최신 사진을 도구 넷(`switch_on`, `switch_off`, `note`, `ask_human`)과 함께 모델에 주고 답이 없으면 규칙으로 돌아오는 `EnginePlanner`. |
| `grow-run` | 재배 1회 = 폴더 1개: `run.json`, `cycles.jsonl`(사이클마다 한 줄), `photos/`, `harvest.json`, 그램당 kWh 요약. |
| `grow-node` | 선반 위에서 도는 바이너리. `grow-node init`, `grow-node run node.toml`, 루프백 `GET /state`·`POST /answer`, 대시보드가 찾을 수 있게 LAN에 UDP 인사. 모델은 기본으로 같은 기계의 Redrob Code 데몬, 아니면 아무 OpenAI 호환 `/v1`. |
| `grow-app` | 데스크톱 대시보드(egui): 범위 대비 측정값, 에이전트가 무엇을 왜 했는지, 가드 경보, 48시간 차트, 노드 여러 개 나란히, 에이전트의 노트·질문과 답장 칸, `node.toml`을 써 주는 5단계 설정 마법사. |

JavaScript 없음, 웹뷰 없음, 클라우드 없음. 노드는 선반의 컴퓨터에서 돌고 대시보드는 같은 LAN에서 그것과 이야기합니다.

## 장비 없이 먼저 돌려 보기

```sh
cargo run -p grow-app
```

대시보드가 시뮬레이션 선반으로 열립니다. 여름 또는 겨울 한국 사무실, 기본 작물은 상추, 빨리 감기.
작물을 바꿔 보고, 에어컨이 켜지기 한 시간 전에 히터가 미리 데우는 것을 보고, 명령마다 붙은 이유를 읽어 보세요.

```sh
cargo run -p grow-node -- init          # 시뮬레이션 선반용 node.toml 작성
cargo run -p grow-node -- run           # http://127.0.0.1:7474/state 제공
```

대시보드의 **grow-node over HTTP** 소스를 그 주소로 두면 됩니다.

## 누가 결정하나

노드는 사이클마다(기본 10분) 선반을 읽고 레시피 단계를 확인한 뒤 플래너에게 묻습니다. **규칙 플래너**는
데드밴드 제어에 사무실 습관 둘(에어컨 켜지기 전에 미리 가열, 사람이 CO2를 내쉬는 시간으로 조명 이동)을
더한 것입니다. **엔진 플래너**는 규칙이 본 모든 것과 규칙이 할 일을 모델에 보내고, 모델은 도구 호출로
답합니다. 무엇이 돌아오든 `grow-safety`를 거칩니다. 자르고, 거부하고, 강제로 끄고, 아무에게도 묻지
않습니다. 모델은 운영자에게 질문할 수 있고(`ask_human`), 대시보드에 뜬 질문에 답하면 다음 사이클
프롬프트에 들어갑니다.

```toml
[planner]
kind = "engine"        # 또는 "rules"
endpoint = ""          # 비우면 이 기계의 Redrob Code 데몬, 아니면 아무 OpenAI 호환 /v1
model = "redrob/auto"
photo_dir = "photos"   # scripts/snap.sh가 채움. 최신 사진이 함께 감
```

## 진짜 선반에 올리기

레시피가 가정하는 선반은 약 60~90만 원, 철제 선반 하나 크기입니다: [docs/HARDWARE.md](docs/HARDWARE.md).
가장 싼 배선은 스위치마다 Tasmota 스마트플러그 하나(조명·히터·팬·펌프), 측정값이 필요하면 센서를 I2C로
붙인 Tasmota ESP32 하나입니다. 라즈베리파이 Mycodo도 됩니다. 대시보드 **Setup** 탭에서 선반 이름, 작물,
장비, 스위치마다 플러그 주소(또는 Mycodo 입력·출력 unique id), 두뇌를 고르고 `node.toml`을 저장한 뒤
선반 컴퓨터에서 `grow-node run node.toml`을 실행합니다. 루프가 돌기 시작하고 `runs/` 폴더가 채워지고,
대시보드 **Find on LAN**에 노드가 뜨고, 노드 둘은 나란히 보여 A/B 재배가 됩니다.

노드의 HTTP API는 인증이 없고 기본으로 루프백에만 묶입니다. 다른 기계에서는 SSH 터널로 접근하고,
열린 네트워크에 두지 마세요.

## 상태

식물 없이 할 수 있는 것은 다 했습니다: 단계별 하드 상한이 든 레시피, 사무실 시뮬레이션 리그, Tasmota·Mycodo
리그, 안전 가드, 방 학습이 든 규칙 플래너, 운영자 대화가 되는 엔진 플래너, kWh 실측이 든 재배 로그, LAN
발견이 되는 노드 바이너리, 다중 노드 보기와 설정 마법사가 든 대시보드, 릴리즈 워크플로. 테스트 59개,
Linux·Windows CI. 아직 진짜 식물을 기른 적은 없습니다. [docs/RESULTS.md](docs/RESULTS.md)의 첫 줄이
1단계입니다. 단계와 통과 조건: [docs/ROADMAP.ko.md](docs/ROADMAP.ko.md). 작물 추가:
[docs/CONTRIBUTING-RECIPES.md](docs/CONTRIBUTING-RECIPES.md).

## 라이선스

MIT 또는 Apache-2.0 중 선택. Copyright 2026 Janghoon Lee (Redrob) and contributors.
