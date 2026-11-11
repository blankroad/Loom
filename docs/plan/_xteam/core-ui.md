# CORE ↔ UI 데이터 계약 (UI → CORE 요구 초안)

docs/plan/02-core-principle.md 준수.

작성: ui-lead, 2026-11-10 — v0.1 초안 → **v0.2 (UI plan.md v2.0 반영, 아래 "v0.2 요약"이 v0.1 본문보다 우선)**. CORE 팀은 아래에 `## core 의견` 절을 덧붙여 주세요. 이 절(`## ui 요구`)은 UI 리드만 수정합니다.

## ui 요구 v0.2 요약 (CORE 의견 반영 — 정본: `docs/plan/ui/plan.md` §4.5·§15.1·§16)

CORE 의견(core-sol)에 대부분 동의한다: JSON 기본 + M2 전 데이터 구독만 binary opt-in, 구독별 유한 큐·제어 큐 포화 시 disconnect+재snapshot, `core.time`, `core.metrics.report`, `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}`, `no_viewer_attached`/`no_visible_viewer` 분리, fragment→POST bootstrap, `coalesce_by:id` opt-in, replay 쓰기 거부, schemars→JSON Schema→json-schema-to-typescript, rust-embed, fixture replay stub gateway. 남은 요청·이견만 적는다.

| ID | 요청 | 이유 / 상태 |
|---|---|---|
| CR-UI-01 | `loom.columns.v1` binary를 **M2 초반**에 확정(CAN U-CAN-2가 M2 전 전환 요구). UI 스파이크도 지연보다 대역·CPU·배치 여유가 이점이라는 결과 — full D3·FD 64 B로 공동 재측정(UI-S2 P1) | 일정만 |
| CR-UI-03 | `core.time` 응답에 **d1(수신)·d2(송신) mono 둘 다** + clock_id (core-opus 초안은 단일 mono) | 서버 처리 시간을 오프셋에서 제외 |
| CR-UI-07 | viewer role 안에 **rights 3단 watch/operate/approve**. TTY `loom ui`만 bootstrap(operate), 비TTY는 가리키기·안내만 — CORE가 수용한 것은 "비TTY stdout에 비밀 미출력"까지이고, **비TTY 호출이 bootstrap 포함 브라우저를 여는 경로(core-opus 초안)는 없애 달라는 요청**, `loom ui pair`(TTY, 원격, 기본 watch). 승인 객체에 **`presence{method: webauthn|oob|code, proof}`** 자리 + WebAuthn 공개키 등록·assertion 검증 + **등록 게이트**(TTY 1회 코드 또는 기존 자격증명) + 감사에 승인 방법·UA·`navigator.webdriver`. CORE 입장("사람 존재 증명이라 쓰지 않는다")에 동의하되, 등록 게이트가 있는 WebAuthn은 D6의 대상(도움 되려는 자동화)을 실제로 막으므로 **옵션으로 계약에 자리**를 요청. `presence_code`(화면 표시 코드)는 오클릭 방지용으로만 | M1 rights / M2 presence |
| CR-UI-08 | STOP·비상 OFF·safe_state 인터록 예외를 정책 표로. `core.latch` payload에 정지 잡·취소 승인·드라이버 취소 불가 수·`outputs_held[]`. **HTTP `POST /api/v1/stop`** 대체 경로(WS 끊김 중). STOP RPC 이름은 CORE 확정(UI 추천 `core.stop{scope:"all"}` + CLI `loom stop-all`) | P0 정책 표 / M1 |
| CR-UI-10 | 응답 헤더 CSP·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`·Referrer-Policy, **오리진 정본 `http://localhost:<port>`**(127.0.0.1 → 리디렉트, 바인딩은 127.0.0.1) — WebAuthn RP ID는 IP 불가. CORE 예시 URL `127.0.0.1:47123` 변경 요청 | M1 |
| CR-UI-11 | `core.agent_cursor{client_id, from_seq, to_seq, others_count}` 이벤트(에이전트가 사용자 개입을 받은 지점 — UI `⇢ 전달됨`, UDS "에이전트 확인 위치"), `core.activity`에 읽은 범위 `range{seq_a, seq_b}`·대기 술어 | M1 |
| CR-UI-12 | `core.safe_state.run{ids[]}` 묶음 실행 + 항목별 인터록·`finally_blocked` 결과 (A15 "복구 송신 1키") | M1 |
| CR-UI-13 | snapshot·리듀서의 공통 `control{channel, owner_origin, actor, kind, reason, ttl}` 뷰(도메인 lease·lock·job owner 정규화 — 이벤트는 도메인 것 그대로) | M1 |
| CR-UI-14 | fixture replay stub gateway를 **WS 서버 모드**로(UI가 브라우저로 붙음) + fixture에 고부하·gap 3층·`outcome_unknown`·late·제어 큐 포화 | P0 |
| CR-UI-15 | 24 h soak에 UI 탭(headless Chromium) 포함 | M2 |
| CR-UI-16 | `core.ui.layout.list/get/put`, `core.replay.open{range, speed}` | M2 / M3 |
| 이름 | 가리키기 이벤트 `core.ui_focus`(core-opus) vs `ui.focus_request`(core-sol) — 하나로. 렌더 보고는 `core.metrics.report{kind:"ui.render"|"ui.render.sample"}`로 도메인 별칭(`ui.latency_sample`, `core.ui.latency_report`, `core.clock.ping`) 폐기 | P0 스키마 |
| 규칙 | 쓰기 이벤트는 kind와 무관하게 **`request_id` + `payload.write_state` 스키마 필수**(UI 행 갱신 키) | P0 스키마 |
| 헤드룸 | STALLED 판정: heartbeat 1 s, UI는 1.5 s 회색 경고·3 s STALLED | 정보 |

## ui 요구 (v0.1 본문 — 위 요약과 다르면 요약 우선)

전제: 마스터 D2(JSON-RPC 2.0 over WS, role·origin), A5(적응형 배치·64 KiB·snapshot 분할·HTTP GET 대용량), D3(이벤트 레코드), C3(Rust 타입 정본 → JSON Schema → TS). 아래는 그 위에 UI가 추가로 필요한 것.

### U1. 연결·인증
- 브라우저는 **WS 연결 1개**로 모든 패널을 다중화한다(구독 id로 구분). 탭 여러 개 = 연결 여러 개(각각 viewer).
- 웹 bootstrap: `loom ui`(CLI)가 일회성 토큰 URL을 만들어 브라우저를 연다 → `GET /bootstrap?t=…`가 **HttpOnly·SameSite=Strict 쿠키** 발급 후 `/`로 리다이렉트, 토큰 즉시 폐기. WS `hello`는 쿠키로 viewer role 획득. 쿠키 없는 WS는 agent role도 받지 못함(agent는 `daemon.json` 토큰).
- 원격 보기(벤치 PC ↔ macOS): 기본은 SSH 터널(`loomd`는 127.0.0.1만 bind). 직접 LAN bind는 선택 기능으로 TLS + 페어링 코드 — CORE가 bind 옵션을 둘지 의견 요청.
- **자기 승인 완화용 훅**: 승인 RPC가 "승인 챌린지"(뷰어 화면에만 표시되는 짧은 코드, 또는 OS 사용자 확인)를 요구할 수 있게 승인 객체에 `challenge` 필드 자리를 둬 주세요(방식은 UI R3에서 확정).
- 뷰어 존재 보고: 브라우저가 `viewer.presence{visible: bool}`를 보낸다(백그라운드 탭은 렌더가 멈춤). `no_viewer_attached` 경고는 "보이는 뷰어 0"을 기준으로 할지 결정 필요.

### U2. 구독
- `core.subscribe{filter{domain, channel, kinds[]}, after_seq | snapshot:true, max_batch_ms, encoding}` — 패널별 구독. 구독 단위로 드롭·`display.gap`.
- **우선 제어 큐 이벤트(응답·쓰기 상태·승인·STOP·상태·오류)는 데이터 구독과 별개로 셸 전역 구독 1개로 항상 수신**, 데이터 배치에 밀리지 않게 별도 프레임으로 먼저 보낸다.
- snapshot은 도메인별 "패널 복원 상태"를 포함해야 늦은 합류가 성립한다: UART 화면 백로그(줄 경계), CAN Fixed 테이블(ID별 최신)·신호 최신값, UDS 세션·진행 중 트랜잭션, 승인 대기 목록, 입력권 소유자·hold, STOP latch, env 배지, record.health, 실행 중 테스트 세션·job.
- 구독별 통계(큐 깊이, 드롭 수, 마지막 flush 시각)를 heartbeat에 실어 UI가 `LIVE/LAG/STALLED/LIVE-DEGRADED`를 계산하게.
- (검토 요청) 서버 쪽 축약 옵션: CAN Fixed 뷰처럼 "키별 최신값만" 필요한 구독에 `coalesce_by:"id"` + 축약 카운트 명시. 축약은 화면용이고 기록·에이전트에는 영향 없음, 화면에 `coalesced N` 표시(거짓말 금지).

### U3. 고속 전송 형식
- 제어·RPC = JSON-RPC 텍스트 프레임 유지.
- **고속 데이터 배치는 바이너리 WS 프레임 opt-in**(`encoding:"binary"`): 헤더 `{sub_id, codec, first_seq, last_seq, count, hub_publish_mono_ns}` + 본문. codec 후보(JSON / MessagePack / 열 단위 프레임)는 UI 스파이크(10k 프레임/s) 결과로 v0.2에서 제안. CORE는 프레임 종류를 확장 가능하게만 잡아 주세요.
- 64 KiB 배치 상한이 10k 프레임/s CAN FD(64 B payload)에서 배치 수를 얼마나 늘리는지 같이 측정 예정.

### U4. 시계·지연 측정
- heartbeat(1 s)와 각 배치에 `daemon_mono_ns`, `daemon_wall_ns` 포함 → 브라우저가 `performance.now()`와의 오프셋을 왕복 최소값으로 추정(`core.time` RPC 왕복도 제공 요청).
- UI가 계산하는 lag = (추정 데몬 현재 mono) − (화면에 아직 못 그린 가장 오래된 이벤트의 `ts.mono_ns`). 장치→화면 p95는 이 값 + 렌더 완료 시점으로 측정해 UI가 `core.metrics.report`로 보고(수용 시험·soak 집계용).

### U5. 질의·기록·재생
- 범위 질의·export·viz 대용량: 같은 오리진 **HTTP GET 스트리밍**(쿠키 검사), NDJSON 또는 바이너리. 커서 문법은 D5(`seq:N`, `mark:X`, 시각).
- 재생: 저널을 같은 이벤트 스키마로 다시 흘려 주는 `core.replay.open{range, speed}` → 라이브와 같은 구독 경로(패널 코드 하나로 라이브·재생 모두). 화면에 `REPLAY` 배지, 재생 중 송신은 기본 off.
- 마커: `core.mark{label}` (origin=user) — 모든 도메인 타임라인에 공통.

### U6. 쓰기·개입
- UI의 모든 쓰기는 에이전트와 **같은 도메인 RPC**(origin=user는 서버가 연결 자격으로 부여). 쓰기 상태(D4/A14)는 `request_id` 행 갱신 이벤트로.
- viewer 전용 RPC: 승인/거부, `*.input`(대화형 입력 스트림), take-control/hold, 비상 OFF, 정책 완화 승인, `core.stop_all`, 복구 송신(A15).
- 에이전트 → 사용자 화면 "가리키기": `loom ui open <panel> [--filter …] [--at seq:N]` → 이벤트 `ui.focus_request{panel, args, actor}`를 뷰어에 발행(뷰어가 따를지는 사용자 설정). 레이아웃 저장: `~/.loom/config/layouts/*.json` 읽기·쓰기 RPC.

### U7. 스키마·빌드
- CORE가 빌드 산출물로 **JSON Schema 묶음 + 생성 TS 패키지**(이벤트·RPC params/result·오류 코드)를 내보내고, UI CI는 스키마 해시 불일치 시 실패. `schema_version` 증가 규칙 공유.
- `loomd`가 UI 정적 자산을 바이너리에 내장해 `/`에서 제공(오프라인, CDN 금지, 엄격한 CSP). 개발 모드는 Vite dev 서버 프록시 허용(`loomd --ui-dev-proxy`).
- P0 stub: UI M1이 CORE 완성 전에 시작할 수 있게 **가짜 이벤트 생성기가 붙은 stub `loomd`**(스키마 fixture 재생)를 첫 주 산출물에 포함해 주세요.

### U8. 테스트 세션 (TEST 기획 합의 후 확정)
- 테스트 세션·단계·판정·증거 인용 이벤트를 일반 이벤트 스트림으로(`test.*`), 단계 마커가 모든 도메인 타임라인에 보이도록.

## core 의견

(CORE 팀 작성)

## core 의견 — R1 Sol 초안 v0.1

2026-11-10 · UI의 위 `ui 요구`는 그대로 보존했다. 아래는 **합의 전 CORE 제안**이며 CORE 리드 통합본을 대체하지 않는다. 근거: [CORE 독립 초안](../core/r1-sol.md), [실측 보고서](../../../spikes/core/README.md), [최종 환경·원본](../../../spikes/core/runs/20261109T231749Z/environment.json). UI Sol 초안의 JSON 기본·binary opt-in·decimal string·schema drift 방침과 정렬한다.

### 연결·인증·존재 (U1)

- 같은 오리진·loopback 단일 포트에서 탭당 WS 1개, 다중 sub_id를 수용한다. CLI는 discovery의 agent token으로 `hello{proto,token,client_id,want_role}`; 웹은 HttpOnly/SameSite=Strict 쿠키로 hello. **role은 서버 발급**, 요청 origin/actor 사칭 필드는 무시/거절한다. agent token으로 want_role=viewer를 보내도 승격하지 않는다.
- `loom ui`의 once bootstrap TTL60 s. URL query 원문이 로그/Referer에 남지 않도록 fragment→POST 교환을 우선 추천한다. UI 제안 GET `/bootstrap?t=...`는 로그제외·no-store·Referrer-Policy:no-referrer·즉시 리다이렉트·once 소비 조건으로만 수용한다. HTTP loopback에서 Secure 쿠키는 브라우저별 실검증, TLS에서는 Secure 필수.
- TTY·challenge·브라우저 쿠키만으로 같은 OS 사용자에게서 악의적인 브라우저 자동화를 막는다고 주장하지 않는다. 명시 승인 포커스/재확인·agent/viewer 자격 분리로 실수 방지, 더 강한 경계는 OS 사용자 확인/별도 principal 선택이다. challenge는 확장 필드로 수용하되 사람 존재 증명이라 쓰지 않는다.
- 원격은 SSH tunnel 기본, LAN listener/TLS pairing은 M1 비범위. 정확한 forwarded Host/Origin allowlist를 설정한다.
- 명칭 정렬 제안: `core.viewer.presence{visible,rendered_through_seq,oldest_pending_age_ms}`. `no_viewer_attached`=인증 연결0, `no_visible_viewer`=보이는 뷰어0을 분리한다. visible은 사람 존재가 아니라 탭 가시성 정보다.

### snapshot·구독·배압 (U2)

`core.subscribe{filter, snapshot:true|after_cursor, max_batch_ms, encoding, coalesce_by?}`. 단순 after_seq에는 stream_epoch를 반드시 결합한다. 큐는 구독별512 KiB/최대8배치, 8연결/32구독 admission 상한부터 시작하되 **최종 큐 크기는 full-schema 시험으로 조정**한다. 8배치의 순간 지터 흡수 한계가 아래 실험에서 드러났다.

허브 sequencer가 H 시점 provider의 불변 snapshot과 live 구독을 원자 고정한다. `snapshot_part{snapshot_id,part,count,H}` → `snapshot_end{H}` → live `>H`. pending live가1 MiB 또는2 s를 넘으면 snapshot을 중단하고 명시 재동기화한다. H 캡처 뒤 도메인 상태를 다시 읽어 섞지 않는다.

snapshot 내용은 U2 요청 수용: UART 줄경계 백로그(UTF8/VT 상태 불명 구간 표시), CAN Fixed/신호 최신값, UDS targets/최근100 txn/진행행·DID, approvals·lease/hold·STOP·env·record.health·jobs/test. 도메인이 `SnapshotProvider`를 등록하고 UI가 장치를 다시 열지 않는다.

셸 전역 제어 구독1개는 데이터 sub와 독립이다. 쓰기상태/최종응답·승인·STOP·오류·연결·gap을 별도 텍스트 프레임으로 먼저 보내되 request별 causal fence를 유지한다. 제어 큐256 KiB/256건은 정상 drop 금지, 포화하면 그 연결을 해제하고 snapshot 복구한다. 무한 제어 큐는 금지하며 STOP mailbox는 별도다. 단일 TCP에서 이미 전송 중인 프레임 뒤 HOL까지 없어진다고 약속하지 않는다(64 KiB cap·send timeout100 ms).

`coalesce_by:id`는 Fixed/최신값 화면에 opt-in 수용. `coalesced_count`와 관측 범위/quality를 포함하며 원본 저널·CLI에는 영향 없다. 빈 이벤트/필터가 만든 seq 불연속은 gap이 아니다. worker→gateway 화면 미러 손실은 stream_seq가 없을 수 있으므로 `display.gap`의 source 범위를 사용한다.

### 고속 전송 (U3) — 실측과 결정 범위

최종 실험: macOS M1, release, 합성64 B payload·200건 batch·100k events/s 목표·300k건/실행·조합별 2회, 서버/클라이언트 별도 프로세스. 아래 지연은 **서버 생성→Rust WS 수신**이며 UI 렌더/장치 ingest/저널/JSON-RPC 봉투는 제외다.

| 형식 | 최대 batch B | 1 client 수신 p99 ms | 4 client 수신 p99 ms | 4 client 서버 peak RSS MiB |
|---|---:|---:|---:|---:|
| JSON 숫자 byte 배열 | 57,519 | 0.818–0.873 | 0.895–1.124 | 3.94–4.08 |
| postcard 행 binary | 15,202 | 0.328–0.346 | 0.385–0.426 | 3.56–3.62 |
| 실험 고정 열 binary | 14,416 | 0.293–0.321 | 0.319–0.431 | 3.48–3.52 |

느린 1개(250 ms마다 1배치)를 둔 최종 실행에서도 정상 3개는 각 300k건·중복0, p99 JSON≤1.230 ms/행binary≤0.428 ms/열binary≤0.364 ms였다. 느린 연결은 queue full41–42배치·send timeout1회로 종료된다. **초기 별도 실행**의 열/4 client에서는 정상 3개가각 400건을 놓쳤다(8배치 큐·full6회). 이 실패는 [원본](../../../spikes/core/runs/20261109T230951Z/ws-column-4-2.json)에 보존하며 안정성 통과로 숨기지 않는다. 스파이크에는 실제 display.gap 알림/우선 제어 구현이 없어 제품 무손실 제어를 검증한 것은 아니다.

**추천:** M1은 JSON-RPC 제어와 JSON 이벤트 기본, M2 전 고속 CAN/UART data 구독만 capability로 binary opt-in. 64 KiB는 모든 encoding의 실제 wire frame 전체에 적용. 첫 idle 이벤트 즉시 flush, UART max10 ms·일반 max40 ms·max200건·max64 KiB 중 먼저 도달한 조건. 10k/s라면 count200만으로도 최소 50배치/s이나, full-schema 크기가327 B/event를 넘으면 바이트 cap 때문에 더 자주 flush한다. 위 축약 JSON은 실제 풍부한 레코드가 아니다.

binary 후보 `loom.columns.v1` 헤더:

```text
magic/version/schema_hash/sub_id/batch_id/stream_epoch/count/
first_seq/last_seq/hub_publish_mono_ns/daemon_wall_ns/flags/lengths
+ 공통 메타 dictionary
+ CAN id/ts/dir/origin/flags/dlc/len/source_seq/stream_seq delta/
  data_offsets/raw_bytes 열 (domain schema로 명세)
```

u64는 LE와 정렬/offset을 명세하고 브라우저 BigInt 경계를 둔다. 불연속 seq는 delta/명시 열, nullable는 validity bitmap, origin/quality/request_id/source identity를 의미상 보존한다. 실험의 고정 열 코드는 이 메타데이터가 없어 **제품 decoder로 재사용 금지**. 가변 DLC·혼합 domain·빈 batch·unknown version·잘린 offset·overflow·최대 u64의 Rust/TS golden corpus+fuzz를 P0/P2 계약 관문으로 둔다. MessagePack는 worker/Python에 추천하지만 UI binary codec으로 자동 확정하지 않는다. permessage-deflate 기본 off.

### 시계·지연 (U4)

명칭은 `core.time`(daemon receive/send mono와 wall·clock_id)·`core.metrics.report`로 통일 제안한다. UART의 `core.clock.ping`/`core.ui.latency_report`, CAN의 `ui.latency_sample`은 별칭을 각각 만들지 않고 P0 스키마에서 정리한다.

브라우저는 RTT 최소 표본으로 offset·오차폭을 추정하고 performance.now와 서버 mono를 직접 빼지 않는다. 배치의 source ts·publish ts·clock quality를 보존한다. lag=가장 오래된 **미렌더** 이벤트 age, idle 별도. 렌더 보고는 source ref·stream cursor·WS 수신/decoder/상태/렌더 제출 시각·coverage·clock error를 담고 10 s마다 bounded histogram으로 보고한다. rAF/write callback은 관측 가능한 렌더 제출 proxy이며 실제 화면 스캔아웃까지 보장한다고 쓰지 않는다.

`publish-before-reply`는 허브 발행/큐 접수의 순서 보장이다. 결과 `published_through_seq`와 viewer `rendered_through_seq`를 구별한다. 느린 viewer 렌더 ACK를 agent RPC의 무한 전제에 두지 않는다. 실제 보임을 요구하는 자동 시험은 렌더 ACK/DOM·같은 request_id를 별도로 검증한다.

### 질의·쓰기·개입 (U5–U6)

대용량 query/export/raw_ref는 같은 오리진 authenticated HTTP GET 스트리밍, opaque artifact id·상한·cursor·checksum 제공. 임의 파일경로 접근 금지. replay는 별도 stream namespace·REPLAY 배지·기본 TX0, original origin과 현재 replay actor를 분리한다. `core.mark`와 test step marker는 공통 타임라인으로 보낸다.

UI 쓰기는 같은 domain RPC, 요청 origin은 서버가 user로 정한다. `core.approval.respond`/정책 완화/입력 lease·recover는 viewer 전용. **STOP은 agent CLI `loom stop-all`에서도 가능**하므로 viewer 전용으로 한정하지 않는다. `core.stop_all`은 agent/script 신규 TX·모든 job을 latch하고 사용자 대화형 입력은 허용한다. 사용자 STOP 후 finally 자동 실행0·출력 유지·safe_state 복구 버튼을 적용한다. 별도 emergency OFF와 일반 recover의 interlock 규칙을 구별한다. R3 사용자 IG도 본인 확인1회로 같은 승인 객체를 소비하는 안에 동의하되, viewer라는 이유만으로 risk/deny/rate 검사를 생략하지 않는다.

`ui.focus_request`는 권고 이벤트이며 UI가 사용자 설정으로 수용한다. layouts는 서버 검증·파일 크기 상한·지정 디렉터리 접근만 허용한다. focus/레이아웃은 M1 핵심 루프 다음 우선순위로 둔다.

### 스키마·빌드·TEST (U7–U8)

Rust serde+schemars1.2.3 → JSON Schema → **json-schema-to-typescript16.0.0(MIT)**·datamodel-code-generator0.91.0(MIT)를 추천한다. quicktype은 대안이며 ts-rs/specta 직접 TS 경로는 C3 정본 경로에서 제외한다. 버전·공식 README/npm/PyPI 원본은 spikes/core/evidence에 보존했다. 실제 생성 호환성은 P0 관문이다. Schema draft2020-12 지원을 검증하고 필요하면 동등한 draft7 subset을 명시한다. u64/i64는 JSON decimal-string newtype, binary는 정수 열이다. schema/hash·generated diff·Rust→JS/Python fixture CI가 필요하며 TS 타입과 runtime validation은 별도다.

정적 UI 자산은 rust-embed 후보로 loomd에 임베드하고 CSP/offline/MIME/ETag·자산 hash를 검증한다. dev proxy는 정확한 origin을 허용하고 prod와 분리한 모드다. P0 앞부분에는 schema/core-api/worker-sdk stub+fixture, 후반에는 fixture replay stub gateway를 제공해 UI가 병렬 개발하도록 한다.

TEST는 [core-test 초안](core-test.md)의 session/step/citation/검증 결과를 같은 `test.*` 스트림으로 받는다. 증거 없는 PASS를 거부하고 원판정·사용자 assertion·수정 revision·source ref/범위 링크를 구별한다. 레이아웃·최소 패널·실제 브라우저 렌더 하네스는 UI 소유, 도메인 payload 해석은 도메인 소유다.

## core 답변 (core-lead R3 통합, 2026-11-10)
위 `core 의견 — R1 Sol 초안 v0.1`을 기반으로 R2 교차 리뷰를 거쳐 확정한 CORE 입장. 정본 `docs/plan/core/plan.md` v2.0 §3.4·§4·§5.

- **U1 연결·인증**: 브라우저 WS 1개에 패널 구독 다중화(연결당 ≤ 8 구독, 전체 ≤ 32, viewer 연결 ≤ 8). bootstrap = `loom ui`가 **URL fragment로 일회성 코드 → 페이지가 POST로 교환** → HttpOnly·SameSite=Strict 쿠키(60 s·1회용). UI 초안의 `GET /bootstrap?t=`는 `no-store`·`Referrer-Policy: no-referrer`·로그 제외·즉시 리다이렉트 조건일 때만 — **UI 팀 선택 요청**. 쿠키 없는 WS는 agent role도 못 받음(agent는 `daemon.json` 토큰). 승인 객체 `presence{method: code|webauthn|oob, proof}`(CR-UI-07, M1 code R3 2자/R4 4자 — 오클릭 방지용, M2 WebAuthn + 등록 게이트), viewer rights watch/operate/approve(M1). 원격: 127.0.0.1 + SSH 터널, LAN bind·TLS·페어링은 M5. presence: `core.viewer.presence{channel?, visible}`, 경고는 `no_viewer_attached`(구독 0)와 `no_visible_viewer`(보이는 패널 0) 둘 다.
- **U2 구독**: `core.subscribe{filter, after_seq | snapshot:true, backlog, max_batch_ms, encoding, coalesce_by?}`. **셸 전역 제어 구독은 연결마다 자동 1개**(응답·write_state·승인·STOP·상태·오류·gap), 데이터 배치보다 먼저 별도 프레임, 드롭 금지(넘치면 연결 종료 → 재snapshot, STOP mailbox 1칸). snapshot = 도메인 리듀서의 패널 복원 상태(UI 목록 전부 수용). heartbeat 1 s에 `sub_stats{depth, drops, last_flush}` + `daemon_mono_ns/wall_ns`. `coalesce_by:"id"` 수용(`coalesced_count` 명시, 화면 전용).
- **U3 고속 형식**: 제어·RPC = JSON 텍스트. M1 데이터도 JSON. **M2 `encoding:"binary"` = `loom.columns.v1`**(헤더 magic/version/sub_id/batch_id/epoch/count/first·last seq/publish mono·wall/flags + 열 본문, 필터 후 seq는 delta 열 필수, 정수 LE, u64는 BigInt 경계 명시). 제어·쓰기 최종 결과는 binary에만 싣지 않음. 프레임 종류는 `codec` 필드로 확장 가능. UI 스파이크(10k fps) 결과로 열 목록 확정 — UI v0.2 대기.
- **U4 시계**: `core.time` → `{recv_mono_ns(d1), send_mono_ns(d2), wall_ns, clock_id}`(우선 처리), 배치·heartbeat에 daemon mono/wall. `core.metrics.report{channel, p50, p95, p99, n, skew_err_ms, rendered_through_seq, oldest_pending_age_ms}`. 브라우저 화면 게이트는 **M1 합격 조건**.
- **U5 질의·재생**: HTTP GET NDJSON(쿠키), `/api/v1/query|raw|report`, 응답 ≤ 256 MiB, 동시 4. 재생 `core.replay.open{range, speed}` = M3, 같은 구독 경로, `REPLAY` 배지, 재생 채널 쓰기 거부. `core.mark` 공통.
- **U6 쓰기·개입**: 같은 도메인 RPC, origin은 연결 자격. viewer 전용: 승인·`*.input`·hold·비상 OFF·정책 완화·STOP 해제·`core.safe_state.run`·worker 재시작. `loom ui open` → `ui.focus_request{panel, filter, at, actor}`(UI 이름 채택). 레이아웃 `core.ui.layout.list/get/put` = M2.
- **U7 스키마·빌드**: schemars → JSON Schema(draft 2020-12) + json-schema-to-typescript(버전 고정) 생성 TS 패키지, 스키마 해시를 hello·UI 번들에 넣어 불일치 시 CI 실패. 정적 자산 rust-embed, CSP 엄격, `loomd --ui-dev-proxy`. **P0 Day 5 `loomd --fixture-replay`**(fixture 이벤트 WS 재생 stub) 제공.
- **U8 테스트**: `test.*` 일반 스트림, 단계 마커 M1, 나머지 `core-test.md` 합의대로 P2.
- 남은 합의: bootstrap 형식 선택, presence(WebAuthn 등록 게이트) UX, binary 열 목록(UI 스파이크 후).

- **v0.2 반영(21:23 갱신분)**: CR-UI-01 binary는 M2 초반 확정(full D3·FD 64 B 공동 재측정), CR-UI-08 `core.latch` payload(정지 잡·취소 승인·취소 불가 수·`outputs_held[]`) + `POST /api/v1/stop`, CR-UI-10 오리진 정본 `http://localhost:<port>`(127.0.0.1 바인딩, 리디렉트) + CSP·Permissions-Policy·Referrer-Policy, CR-UI-11 `core.agent_cursor`·`core.activity.range`, CR-UI-12 `core.safe_state.run{ids[]}`, CR-UI-13 공통 `control{}` 뷰, CR-UI-14 fixture replay = WS 서버 모드 + 고부하·gap·outcome_unknown·포화 fixture, CR-UI-15 soak UI 탭(M2), CR-UI-16 `core.ui.layout.*`(M2)·`core.replay.open`(M3). 전부 수용(plan.md §3.4·§4.7·§9.6).
