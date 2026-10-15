# 로드맵

각 단계는 기계적으로 판정되는 통과 조건을 가집니다. 조건이 초록이 아니면 그 단계는 끝난 것이 아닙니다.
이 저장소의 목적은 코드가 아니라 **재배 데이터**입니다. 단계 번호가 올라갈수록 코드보다 `runs/` 폴더가 커져야 합니다.

## 0단계: 부트스트랩 -- 완료 (2026-10-14)

- 레시피 3종(새싹 무·버터헤드 상추·벼 신동진) TOML + 검증
- 사무실 시뮬레이션 리그(에어컨 출퇴근, 건조, CO2 낮밤, 램프 열, 저수조 드리프트)
- Mycodo REST 리그(가짜 서버 테스트)
- 안전 가드(하드 한계·최대 켜짐·쿨다운·강제 OFF)
- 규칙 플래너 + 방 학습(48칸 EMA, 선제 가열)
- 재배 로그(폴더·JSONL·kWh/g 요약)
- grow-node(init/run/recipes, 루프백 /state)
- 대시보드(Shelf·Setup·Recipes, 설정 마법사 -> node.toml)

통과: `cargo test --workspace` 41개 초록, clippy `-D warnings` 초록, Linux·Windows CI 초록.
시뮬레이션에서 겨울 사무실 상추 35일 수온 in-band 75% 이상, 벼 110일 경보 0.

## 코드로 미리 끝낸 것 -- 2026-10-15

1~4단계에서 선반 없이 되는 부분은 전부 들어갔다. 남은 것은 모두 **진짜 식물·진짜 장비·진짜 날짜**다.

- 모델 플래너 `grow-agent::EnginePlanner`: 관측·단계·규칙 플래너의 제안·운영자 메시지·최신 사진을
  OpenAI 호환 `/v1/chat/completions`에 `tools::specs()` 4개와 함께 보내고 `tool_calls`를 받아 가드로
  넘긴다. 실패하면 규칙 플래너 결과를 쓰고 그 사실을 `notes`에 남긴다. 펌프는 모델이 빼먹어도 규칙에서
  가져온다(규칙 2). `node.toml` `[planner] kind = "engine"`, endpoint 비우면 같은 기계의 Redrob Code
  데몬(engine-client, 앱 이름 `grow`), 아니면 아무 `/v1` + 환경변수 키.
- 운영자 대화: `ask_human`은 노드 `/state`의 `questions`에 열려 있고, 대시보드의 입력창 -> `POST /answer`
  -> 다음 사이클 프롬프트. `runs/<id>/operator.jsonl`에 남는다.
- Tasmota 리그(`grow-hal::tasmota`, HTTP): 플러그 하나 = 스위치 하나, `PulseTime`으로 플러그가 스스로 끈다.
  `ENERGY.Total` 합이 `Snapshot.energy_kwh`로 들어가 요약의 `kwh_measured`가 된다(1단계 kWh 실측 게이트).
  MQTT 대신 HTTP를 택한 이유: 브로커가 하나 더 필요 없고 플러그 펌웨어 그대로 된다.
- 단계별 하드 상한: 레시피 `[stages.hard]`가 가드의 플로어를 좁힌다(넓히지는 못함). 벼 출수기 공기 35 C가
  코드가 됐다(3단계).
- 발견: 노드가 UDP 7475로 `{"grow":1,name,listen}`를 5초마다 방송, 대시보드 **Find on LAN**(4단계 mDNS 대신).
- 대시보드: 노드 여러 개 나란히(2단계 A/B), Brain 패널(플래너 배지·노트·질문·답장), 마법사 5단계(Brain).
- `scripts/snap.sh`: 사진 2회/일, 최신 1장이 프롬프트에 붙는다.
- 릴리즈 워크플로(org 템플릿, Windows OIDC 서명, Linux x64+aarch64 라즈베리파이), 레시피 기여 가이드.

확인된 것: `grow-node`가 실제 Redrob Code 데몬에 붙어 요청을 보낸다(2026-10-15 실측). 데몬 v0.6.4의
`/v1/chat/completions`가 `LLMClient` 서비스 누락으로 500을 내는 버그를 찾아 redrob-code PR #107으로 고쳤고,
고친 데몬에서는 요청이 공급자까지 간다(이 호스트는 Console 자격증명이 없어 401에서 멈춤). 모델이 실제로
스위치를 고르는 첫 기록은 1단계 선반에서 나온다.

## 1단계: 첫 진짜 재배 -- 새싹 무 7일

목적: 장비가 돈다는 증명. 작물 결과는 보지 않습니다.

- 선반 조립, 라즈베리파이 + Mycodo, 센서 4종(SHT31·SCD41·DS18B20·EC/pH), 스마트플러그 3개
- 마법사로 `node.toml` 작성, `grow-node run` 7일 무중단
- 카메라 하루 2장 `photos/`에 저장(스크립트 `scripts/snap.sh`)
- Tasmota 스마트플러그 리그(`grow-hal` feature `tasmota`, HTTP) -- 코드 완료, 플러그 전력 측정값으로 kWh 실측

통과: `runs/<id>/cycles.jsonl` 1,000줄 이상 끊김 없음(최대 간격 < 2x period), 전송 오류 비율 < 1%,
`harvest.json` 작성. kWh 실측과 추정치 차이 20% 이내.

## 2단계: 상추 35일, 규칙 vs 사람

목적: 에이전트가 사람보다 못하지 않다는 숫자.

- 같은 선반 두 통: A는 규칙 플래너, B는 사람이 하루 1회 손 조절(기록은 둘 다 자동)
- 양액 도징 펌프 2개(영양·pH다운) 추가, 가드 쿨다운 실측 조정
- 모델 플래너(`grow-agent` `EnginePlanner`) -- 코드 완료. 규칙 플래너는 폴백
- 대시보드: 두 통 나란히 -- 코드 완료. 일별 in-band 비율·kWh는 요약에, 사진 타임라인은 남음

통과: A의 in-band 비율 >= B, 수확 생체중 A >= 0.9 x B, 경보 0, 가드 거부 사유 전부 로그에 있음.
결과표 `docs/RESULTS.md`에 커밋.

## 3단계: 벼 110일

목적: 세상에 없는 데이터 1건.

- 깊은 통, 수온 히터, 200W급 LED, 수위 센서
- 출수기 35도 상한 가드(`[stages.hard]`) -- 코드 완료. 광주기 전환은 레시피 단계가 이미 함
- 중간 사진으로 생육 단계 전환 제안 -- 사진 첨부·`note`/`ask_human` 코드 완료, 판단 품질은 실측

통과: 수확 쌀 무게·kWh/g·일수를 `docs/RESULTS.md`에, `runs/` 폴더 전체 커밋, 식물공장 논문(95일)과 비교 한 줄.

## 4단계: 남이 쓸 수 있게

- 라즈베리파이: Linux aarch64 tar.gz 릴리즈 -- 코드 완료. Redrob OS 모듈은 남음. 노드 발견은 UDP 방송 -- 완료
- Linux/Windows/macOS 릴리즈(OIDC 서명, org 릴리즈 워크플로) -- 워크플로 완료, 첫 태그 전. macOS 서명·공증 남음
- 레시피 기여 가이드(`docs/CONTRIBUTING-RECIPES.md`) -- 완료

통과: 외부 기여자 1명이 자기 선반에서 1회 재배를 올림.

## 하지 않는 것

- 로봇(파종·수확): 구매하거나 나중에
- 센서 하드웨어 설계: 시판품 사용
- 농장 기록 시스템: farmOS에 붙임(내보내기)
- 서버·클라우드·계정: 없음
