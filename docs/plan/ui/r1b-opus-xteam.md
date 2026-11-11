docs/plan/02-core-principle.md 준수.

# UI R1b — ui-opus 정합 점검표 (도메인 패널 명세 · CORE 초안 · TEST 초안 ↔ `r1-opus.md`)

작성: ui-opus, 2026-11-10. `r1-opus.md`는 고치지 않았다(R3에서 리드가 통합).
읽은 것: `_xteam/can-ui.md`(v2.0), `_xteam/uart-ui.md`(v1), `_xteam/uds-ui.md`(v0.1), `_xteam/core-ui.md`(v0.1), `_xteam/ui-panel-template.md`(v0.1), `_xteam/uart-core.md`, `_xteam/uds-core.md`, `_xteam/can-core.md`, `core/r1-opus.md`(§0–§8 — **§9 이후는 파일에 없음**, 그래서 soak·테스트·안전 상세는 "언급 없음"으로 적었다), `test/r1-opus.md`.
표기: **판정** 열 = 누가 맞나(도메인 / r1-opus / CORE / TEST / 둘 다 / 미정). **제안** 열 = R3 통합안. ⚑ = §5 쟁점으로 올림.

---

## 1. 도메인 패널 명세 ↔ r1-opus 차이표

### 1.1 UART (`_xteam/uart-ui.md` ↔ r1-opus §5.1.2·§4)
| # | 항목 | 도메인 명세 | r1-opus | 판정 | 제안 |
|---|---|---|---|---|---|
| U-1 | 패널 구성 | `uart.terminal`(M1) · `uart.trace`(M1 최소, 별도 패널 또는 탭) · `uart.channels`(M1) · `uart.approval_card`(M1/M2) · `uart.test_evidence`(M2) | 터미널 패널 안의 [터미널\|트레이스] 토글 하나 | 도메인 | 패널 id 5개 수용. `uart.trace`는 독립 패널 id로 두고 터미널 패널에서 탭처럼 열 수 있게. `approval_card`·`test_evidence`는 패널이 아니라 **셸 슬롯 렌더러**(§1.4 공통 C-a) |
| U-2 | 이벤트 kind | `uart.data`(rx/tx), `uart.write`, `uart.state`, `uart.config`, `uart.input_owner`, `uart.hold`, `uart.lock`, `uart.agent_activity`, `uart.transfer`, `uart.signal`, `uart.mark`, `uart.expect_result`, `uart.input_ack`, `uart.input_suspect` | `uart.rx`, `uart.tx`, `uart.lease`, `deferred` (임의 명칭) | 도메인 | r1-opus 명칭 폐기, UART 목록 채택. CORE `uart.write`와도 일치 |
| U-3 | 입력 RPC | `uart.input{channel, data_hex, input_seq}` + `uart.input.yield/.clear/.takeover`, `uart.hold/unhold`, `uart.cancel` | `uart.input{channel, bytes_hex, client_seq}`, 접두키만 정의 | 도메인 | UART 이름 채택. 접두키(`Ctrl-] y/c/o/h/k`)는 이 RPC를 부르는 셸 단축키로 매핑 |
| U-4 | Ctrl-C | 항상 0x03, 복사는 Ctrl-Shift-C/Cmd-C | 선택 영역 있으면 복사(미정) | 도메인 | UART 채택(터미널 의미 우선, 개입 키가 복사로 새면 안 됨) |
| U-5 | 장치 출력 정제 | `term.write` 전 `loom-console` 렌더 정제기(WASM 권장) + 자동 응답 3중 방어(정제기 질의 제거 + `register*Handler` 삼킴 + **사용자 의도 게이트**: 키/붙여넣기/조합 이벤트 밖의 `onData` 폐기) | CSI 핸들러로 DA/DSR 응답 삼킴만 | 도메인(더 강함) | 3중 방어 채택. 의도 게이트는 **셸 공통 터미널 컴포넌트**에 둔다(SSH·DLT 셸 재사용 — UART Q3에 동의). WASM 여부 ⚑ |
| U-6 | gap 표시 | gap 줄은 파서 리셋 지점에서 스트림에 삽입 허용, 그 외 장식/오버레이 | xterm decoration만 | 둘 다 가능 | 기본 decoration, decoration API가 proposed API면(`allowProposedApi`, UART Q2) UART 방식으로 대체 — ui-sol 확인 |
| U-7 | 지연·성능 | RX→그리기 p95 < 100 ms · keydown→`sent` p95 < 20 ms(폭주 중 < 100) · 패널 열기→첫 화면(2,000줄) p95 < 500 ms · 입력권/HOLD/STOP 이벤트→모든 탭 ≤ 100 ms · parser ≤ 5 ms/프레임 | RX→그리기 p95 < 100 ms, 늦은 합류 < 1 s, UI 구간 ≤ 45 ms | 도메인(더 구체) | UART 수치 수용. UART 패널의 늦은 합류는 500 ms로 강화, 다른 패널 1 s 유지 |
| U-8 | 메모리 | scrollback 10,000줄(상한 50,000) · 트레이스 링 20,000행 · 그리기 백로그 4 MiB | scrollback 10,000 · 원시 링 4 MiB | 거의 일치 | 트레이스 링 20,000행 추가, scrollback 사용자 설정 상한 50,000 |
| U-9 | 원격 viewer | 기본 읽기 전용(입력 활성 토글) | 원격 기본 watch | 일치 | — |
| U-10 | 사용자 R3 시그널(부트로더 진입) | 셸 공통 "사용자 본인 확인 모달" 요청 | 정의 없음 | 미정 ⚑ | §5 쟁점 4 |
| U-11 | 키보드 | Keyboard Lock 전체화면 모드 + 제어키 팔레트 + 모든 동작 버튼화 | Tauri M4, 대체 키 | 도메인 | 제어키 팔레트·버튼화 M1 수용, Keyboard Lock 전체화면 모드 M2(지원 브라우저 확인 후) |
| U-12 | 증거 ref | `uart:dut:ev:N` | — | TEST 형식과 충돌 ⚑ | TEST REF `uart:dut@seq:N` 채택(§3) |

### 1.2 CAN (`_xteam/can-ui.md` ↔ r1-opus §5.1.3)
| # | 항목 | 도메인 명세 | r1-opus | 판정 | 제안 |
|---|---|---|---|---|---|
| C-1 | 출처 색 | AGT **주황** / USR **파랑** / DEV 회색 / EXT 보라 (CAN D50 유지) | AGT 청록 / USR 노랑 / DEV 회색 / SCR 보라 / EXT 보라 점선 | 미정 ⚑ | UART·UDS·탐색이 청록/노랑 → r1-opus안. CAN 동의 필요(쟁점 1) |
| C-2 | M 배정 | 채널 바 M1(최소) · Trace M1(Chrono+raw+송신+생략+배지) · Fixed·신호 펼침·필터식·ISO-TP 묶음 M2 · **Graphics M2(기본 플롯)**/M3(커서·state·분할) · IG M2(1회·주기·patch·stop)/M3(takeover) · Data M2 · Statistics M4 | Trace M1, Fixed·IG·Data M2, Graphics M3 | 도메인 | Graphics 기본 플롯을 M2로 당김(데이터 RPC가 M2에 생김). **채널 상태 바**를 셸 상단 띠의 도메인 슬롯으로 M1에 추가 |
| C-3 | 웹 지연 게이트 | 장치→paint **p95 < 150 / p99 < 300 ms**, 브라우저 디코드+반영 ≤ 20 + paint ≤ 16 ms | 목표 150(상한 200), UI 구간 ≤ 85 ms(20 Hz 스케줄러 대기 50 ms 포함) | 도메인 | 150/300 수용. 그러려면 **정상 부하에서는 rAF마다 렌더(최대 60 Hz)하고, 20 Hz 상한은 예산 초과(고부하·LIVE-DEGRADED) 때만**. UI 구간 목표 ≤ 50 ms로 조정 |
| C-4 | 보존·처리량 | 클라이언트 ≥ 100k행, Chrono 2,000행/s, Fixed ≤ 2,000 ID 10 Hz | 탭 링 200k, 2,000행/s, Fixed 20 Hz 상한 | 일치(r1-opus가 상한 쪽) | 보존 하한 100k·상한 200k로 명시 |
| C-5 | TX 행 이벤트 | `can.tx`(request_id 행 갱신) | 일반 `request_id` + `write_state` | 이름 차이 ⚑ | UI는 kind와 무관하게 `request_id` + `payload.write_state`로 행 갱신(쟁점 10) |
| C-6 | 지연 회신 | `ui.latency_sample{source_id, paint_mono_est}` 표본 1/50 | `core.render_ack` 1 Hz | 이름 4종 충돌 ⚑ | 쟁점 7 |
| C-7 | 딥링크 | `/#/can/trace?ch=ch0&since=mark:s2`, `/#/can/graphics?sig=A,B&t0=…`, `loom ui open <path>` | `#/l/<layout>?panel=…`, `loom ui show PANEL …` | 도메인 + CORE | `loom ui open <panel-ref>` 하나로(`ui show` 폐기), URL `/#/p/<domain>/<panel>?…` 형식(쟁점 9) |
| C-8 | 사용자 시작 R3(IG 주기 송신) | "본인이 viewer이므로 셸 확인창 1회" | 정의 없음 | 미정 ⚑ | 쟁점 4 |
| C-9 | 에이전트 표시 | Trace 왼쪽 여백에 에이전트가 읽은 범위 막대(`agent read seq a..b`), Graphics에 `wait-for` 임계 점선, Data에 `read by agent 3s ago` | Activity 행만 | 도메인 | 수용(좋은 개입 단서). 필요한 것: `core.activity`에 `range{seq_a, seq_b}`·`predicate` 필드 — CORE 요청 |
| C-10 | 입력 검증 | hex 11/29bit, CC 0–8, FD 길이 집합, DB 위젯 | 송신 입력줄만 | 도메인 | 수용(U-CAN-6) |
| C-11 | 열 블록 필드 | ts_mono_ns u64, ch u8, dir/flags u8, id u32, len u8, data offset/bytes, origin u8, external u8, source id | 비슷(+request_id 사전 인덱스) | 일치 | 합쳐서 `core-ui.md` v0.2로 |
| C-12 | 테스트 마커 | `test.step` → Trace 전폭 행·Graphics 수직선·Activity | `test.step_marker` | 이름 ⚑ | TEST 이름으로(쟁점 11) |

### 1.3 UDS (`_xteam/uds-ui.md` ↔ r1-opus §5.1.4)
| # | 항목 | 도메인 명세 | r1-opus | 판정 | 제안 |
|---|---|---|---|---|---|
| D-1 | 패널 구성 | `uds.console`(U1 표·U2 대상 바·U3 입력·U4 승인 카드) + `uds.values`(U5) · `uds.dtc`(U6) · `uds.targets`(U7) · `uds.recover`(U8) · `uds.jobs`(U9) | `uds.console` 하나 | 도메인 | 수용 |
| D-2 | **M1 범위** | U1·U2·U3·**U4(R2 승인 카드 — token/viewer/auto 세 경로 시험)**·**U5 마지막 값 표** | 표·명령줄·제어 가져오기·중단. 승인은 M2 | 도메인 ⚑ | M1에 R2 확인 카드(존재 확인 없음) 포함, R3/R4는 M2(쟁점 3). U5 마지막 값 표 M1 수용 |
| D-3 | 행 키·단계 | `row_key=request_id`, `guard → queued → tx → pending×N → rx\|timeout → verify`, 결과 기호 `✓ ✗NRC ⏳ ⌛ ⌛? ⛔ ⚠R2/R3 late ok_suppressed` | `○◔◑●` 단계 칩 + 같은 기호 일부 | 둘 다 | 기호는 UDS 결과 기호 + D4 단계 칩을 함께(칩 = 단계, 기호 = 결과). `late`·`ok_suppressed` 추가 |
| D-4 | 미리보기 | 입력 150 ms 정지 → `uds.request{dry_run:true}`(viewer, 감사 없음, 레이트 제한), p95 ≤ 150 ms | "서버 dry-run RPC" | 일치 | — |
| D-5 | 지연 | tx→진행 행 ≤ 50 ms · rx→확정 p95 ≤ 100 / p99 ≤ 200 · 사용자 Enter→`uds.tx` ≤ 50 · 늦은 합류 ≤ 1 s | rx→확정 100 ms, UI 구간 ≤ 35 ms | 일치 | tx→행 50 ms·Enter→tx 50 ms를 UI 시험(UI-T03)에 추가 |
| D-6 | 시계 | 데몬 `ts.wall_ns` ↔ 브라우저 `timeOrigin+now()` | 데몬 mono ↔ 브라우저, `core.time` 최소 RTT | r1-opus | wall 시계는 NTP 보정으로 튈 수 있음 → mono + `core.time` 오프셋 사용을 UDS에 요청 |
| D-7 | 승인 키 | 카드 포커스 시 `Y`/`N`, R3 코드 끝 2자, **1 s** 입력 무시, 여러 건이면 목록 | 0.7 s, 기본 포커스 = 거부, Esc = 보류 | 미정 ⚑ | 쟁점 2 |
| D-8 | 단축키 | `j/k`, `Enter`, `:`, `T/R/!`, `Y/N`, **셸 STOP `Ctrl-X`** | 웹에서 `Ctrl-X` STOP 미채택(잘라내기 충돌) | r1-opus ⚑ | 웹 STOP = 버튼 + `Ctrl-] x`. 단일 글자 키는 패널 포커스·입력란 밖에서만 |
| D-9 | 접근성 | 색만으로 상태 전달 금지, `aria-live`는 승인·STOP·NRC만, 고대비·모션 줄이기, 키보드 경로를 M1 Playwright 시험에 | 색+문자만 | 도메인 | 셸 공통 접근성 규칙으로 승격 |
| D-10 | 에이전트 확인 위치 | U1 상단 "에이전트 claude-1 확인 위치 seq:431"(데몬 client 커서) | 행마다 `⇢ 에이전트에게 전달됨` | 같은 요구 | 둘 다 `core.agent_cursor` 이벤트 하나로 구현(CORE 요청 C-8) |
| D-11 | bootstrap 발급 | **TTY·데스크톱 셸에서만**(에이전트 자기 승인 방지) | 데몬이 직접 브라우저 열기(에이전트도 `loom ui open` 가능), 비밀 비노출 | 미정 ⚑ | 쟁점 5 |
| D-12 | 수용 시험 | DOM 행 텍스트 + `data-request-id`로 확인, WS 수신만으로는 불합격 | 화면 관찰 + render_ack | 일치 | `data-request-id`·`data-env` 속성 규약을 셸 공통으로 |

### 1.4 세 도메인 공통으로 드러난 UI 프레임워크 요구 (r1-opus에 없거나 약함)
| # | 요구 | 출처 | 제안 |
|---|---|---|---|
| C-a | **승인 카드 본문 도메인 렌더러**(UART `approval_card`, UDS `approval.display` JSON Schema, CAN plan·바이트·신호 범위 슬롯) | 3팀 | 셸은 공통 틀(요청자·등급·env·만료·버튼)만 그리고 본문은 `registerApprovalRenderer(domain, schema, render)` 또는 JSON Schema 기반 일반 렌더러. 기본 = 일반 렌더러(UDS Q2), 도메인 렌더러는 선택 |
| C-b | **행 갱신 표 공통 컴포넌트**(request_id 키) | UDS Q1 | 수용 — `WriteTracker` + 가상 스크롤 표를 셸 컴포넌트로 |
| C-c | **"위치로 이동" 공통 API**(증거 ref → 패널·seq) | UART Q10, TEST | `ctx.ui.reveal(ref)` 추가. ref 형식은 TEST REF |
| C-d | **공통 터미널 컴포넌트**(정제기·의도 게이트·접두키) | UART Q3 | 수용 |
| C-e | 셸 상단 띠의 **도메인 채널 슬롯**(CAN 채널 바, UDS 대상 바, UART 채널 행) | CAN §5.2, UART `uart.channels` | manifest에 `shellSlots: ['channel_bar']` 추가 |

---

## 2. CORE 초안 ↔ UI 요구 차이표

### 2.1 `core-ui.md` U1–U8
| 항목 | 요구 요지 | CORE 초안 상태 | UI 제안 |
|---|---|---|---|
| U1 연결·인증 | WS 1개 다중화, bootstrap→HttpOnly 쿠키, 원격 SSH, 승인 `challenge` 자리, `viewer.presence{visible}` | **수용**: 쿠키 viewer, bootstrap은 **URL fragment**로만·1회·60 s, 비TTY stdout에는 미출력, 연결당 구독 ≤ 32. **다르게**: `challenge` 대신 `presence_code`(viewer 직렬화에만 나오는 코드). `viewer-tty` role 신설(R3/R4 승인 불가). **언급 없음**: `viewer.presence`, LAN bind(0.0.0.0은 비범위 M5) | fragment 방식 채택(서버 로그·Referer에 안 남음 → `core-ui.md`의 `?t=` 쿼리안보다 낫다). `presence_code`는 브라우저 자동화가 화면에서 읽을 수 있어 S2를 못 막음 → **`presence{method: code\|webauthn\|oob, proof}`로 일반화** CORE에 요청(§4·쟁점 6). `viewer.presence` 계속 요청 |
| U2 구독 | 패널별 구독, 우선 제어 별도 프레임, snapshot에 패널 복원 상태, 구독 통계 heartbeat, `coalesce_by` | **수용**: filter{domains,channels,kinds,origins}·backlog·max_batch_ms·encoding, 구독자별 우선 큐(넘치면 연결 종료 → 재snapshot), 리듀서 snapshot(채널·승인·job·lease·latch·env·record·worker·test/run), 링 밖 재개 시 `display.gap{resume_out_of_ring}`, `core.epoch_changed`. **부분**: heartbeat에 `stream_seq_hw, mono_ns, record_health, latch`만(구독별 큐 깊이·드롭 수 없음). **언급 없음**: `coalesce_by` | 우선 큐 초과 시 "연결 종료 후 재snapshot"을 UI가 정상 경로로 처리(재연결 배너 + 이음). 구독 통계는 배치의 `lag_ms`·`display_gap`으로 충분 — 요구 철회 가능. `coalesce_by`는 Fixed 화면 M2 측정 후 재요청 |
| U3 고속 형식 | binary opt-in, 헤더 `{sub_id, codec, first_seq, last_seq, count, hub_publish_mono_ns}` | **부분 수용**: 허브는 JSON 1회 직렬화 공유(`Arc<[u8]>`), 바이너리 열 프레임은 구독 옵션 **M2–M3** | 일정 수용(M1 JSON). 단 CAN U-CAN-2가 "M2 전" 전환을 요구 → CORE M2 초반으로 못 박기 요청 |
| U4 시계·지연 | heartbeat·배치에 mono/wall, `core.time`, `core.metrics.report` | **수용**: `core.time{}`→`{mono_ns, wall_ns, clock_id}`, heartbeat `mono_ns`, 배치 `lag_ms`. **언급 없음**: 지연 보고 RPC, 배치의 송신 시각 | 지연 보고 RPC 이름 하나로 확정 요청(쟁점 7) |
| U5 질의·재생·마커 | HTTP GET 스트리밍, `core.replay.open`, `core.mark` | **수용**: `GET /api/v1/query/<id>`(60 s, NDJSON, ≤ 256 MiB), `/api/v1/raw/<ref>`, `/api/v1/report/<sess>.*`, `core.mark{label,note}`, `core.events.query`. **언급 없음**: `core.replay.open` | r1-opus의 `/data/<id>` 폐기, CORE 경로 채택. 재생 RPC는 M3 전에 요청 |
| U6 쓰기·개입 | 같은 도메인 RPC, viewer 전용 RPC 목록, 가리키기 `ui.focus_request`, 레이아웃 RPC | **수용**: origin=user by role, `core.stop`(전부)·`core.stop.release`(viewer), `core.approval.respond`(viewer web), `core.worker.restart`(viewer), `core.policy.apply`(viewer web), `core.ui.focus{panel, filter, at}`(agent) → 이벤트 `core.ui_focus`, 화면을 강제로 바꾸지 않음. **다르게**: 복구 송신 = `core.safe_state.list` → 도메인 RPC. **언급 없음**: 레이아웃 RPC | 이름은 CORE 채택(`core.stop`, `core.ui_focus`). 복구 송신 1키는 `core.safe_state.run{ids[]}` 묶음 실행 추가 요청(쟁점 13). 레이아웃 RPC 요청 유지 |
| U7 스키마·빌드 | JSON Schema + TS 패키지, 해시 불일치 CI, 내장, dev 프록시, stub loomd | **수용**: `loom-schema`(JSON Schema 내보내기) stub crate, rust-embed 8.13.0, `/ui/*` 정적. **부분**: P0 `loom-testkit` FakeGateway·`loom-echo` sim — WS로 UI에 붙는 가짜 데몬인지 불명. **언급 없음**: TS 생성 담당, dev 프록시 | TS 생성은 UI CI가 담당(JSON Schema 입력). FakeGateway를 **WS 서버 모드**로 실행 가능하게 요청 — 안 되면 UI `loom-mock`(r1-opus UI-02)로 간다. 정적 경로 `/ui/*` 수용(r1-opus `/` 수정) |
| U8 테스트 세션 | `test.*` 일반 스트림, 단계 마커 모든 타임라인 | **위임**: `_xteam/core-test.md` §4(파일 아직 없음), 예약 domain `test`·`run` | TEST 쪽 정리(§3) 후 CORE가 kind 2토큰 규칙으로 확정 |

### 2.2 r1-opus §15.1 C-1 ~ C-15
| # | 요청 | CORE 초안 | UI 제안 |
|---|---|---|---|
| C-1 | `core.time` | **수용** (`{mono_ns, wall_ns, clock_id}`) | 맞춤. UART `core.clock.ping` 요청은 이 이름으로 정리 |
| C-2 | `render_ack` + 뷰어별 lag 집계 + `viewer_hidden` | 언급 없음 | 쟁점 7 이름 확정 후 재요청 |
| C-3 | `viewer_state{visible}` | 언급 없음 | UART #13 `core.viewer.presence` + 봉투 `meta.viewers{subscribed, visible}`와 합쳐 하나로 |
| C-4 | 우선 kind 목록 + 배치 메타 | **수용**: 응답·write_state·승인·STOP·상태·오류·gap 우선, 배치 `lag_ms`·`display_gap`·`record_health`. 송신 시각 없음 | 송신 시각은 `lag_ms`로 대체 가능 → 철회 |
| C-5 | 열 블록 binary | 부분(M2–M3) | §2.1 U3 |
| C-6 | 자격 3단(watch/operate/approve), 비밀 비노출, `loom ui pair`, WS 1회용 티켓 | **다르게**: role = agent / viewer / viewer-tty / script. 비밀 비노출 **수용**(fragment·비TTY 미출력). `pair`·티켓 **언급 없음** | viewer 안에 rights 필드(watch/operate/approve) 추가 요청 — 쟁점 5와 묶음. 티켓은 "패널이 쿠키로 두 번째 WS를 연다"를 막는 장치인데 패널 코드가 1st-party 정적 번들이라 lint로 충분 → **철회 가능**(우선순위 낮춤) |
| C-7 | 존재 확인 훅 + 감사(UA·webdriver) | **다르게**: `presence_code`(코드 일치 검사), role redaction으로 agent 직렬화에서 제거 | 일반화 요청(쟁점 6). 감사 필드는 CORE §6.3(감사 로그)에 추가 요청 |
| C-8 | `core.agent_cursor` | 언급 없음(봉투 `others_since_last`·client 커서 저장은 있음 — UDS C-5) | 커서 전진 시 이벤트 발행 요청. UDS D-10과 같은 요구 |
| C-9 | STOP 상태 이벤트 + HTTP `/stop` 대체 | **다르게(이름)**: `core.stop{scope, reason}` → `core.latch{on, by, origin}` → `core.latch{acked, missing}`, `latch.json` fsync, 리듀서 `outputs_held[]`, agent도 STOP 가능·해제는 viewer만, 이후 agent 쓰기 **exit 6 `E_STOPPED`**. HTTP 대체 없음 | CORE 이름·종료 코드 채택(r1-opus의 `core.stop_all`·`stop_state`·"exit 4/6" 폐기). "정지한 잡·취소 승인·드라이버 취소 불가 수"는 `core.latch` payload에 추가 요청. `/stop` HTTP 대체는 WS 끊김 중 STOP 수단으로 계속 요청(저비용) |
| C-10 | 복구 송신 공통 | 다르게: `core.safe_state.list` + 도메인 RPC. UDS는 `core.recover{target}` 요청 | 쟁점 13 |
| C-11 | `ui.open/show/present`, 레이아웃 RPC, 데몬이 브라우저 열기 | **수용**: `core.ui.focus`, 브라우저 없으면 OS 브라우저(bootstrap 포함). 레이아웃 없음 | `ui show` 폐기하고 CORE 모델 채택. 단 "에이전트가 연 브라우저에 어떤 자격을 주나"는 쟁점 5 |
| C-12 | CSP·Permissions-Policy·`localhost` 정본 | 언급 없음. 예시 URL이 `http://127.0.0.1:47123/` | 헤더 요청 유지. 오리진은 WebAuthn 때문에 `localhost` 정본 필요 — §4 결과로 근거 첨부 |
| C-13 | 24 h soak에 UI 탭 | 언급 없음(§9 미작성) | 유지 |
| C-14 | 스키마 fixture 첫 주 | **수용**(`loom-testkit` fixture, `loom-echo`) | fixture에 고부하·gap·`outcome_unknown` 시나리오 포함 요청 |
| C-15 | 공통 lease 이벤트 | 부분: 리듀서에 lease 포함, 이벤트 형식 없음. 도메인은 `uart.input_owner/lock`, `uds.lease`, `can.job_state` | 이벤트 통일 대신 **snapshot·리듀서의 공통 `control` 뷰**(`{channel, owner_origin, actor, kind, reason, ttl}`)만 요청 — 이벤트는 도메인 것 그대로, UI 어댑터가 정규화 |

### 2.3 CORE 초안에서 UI가 새로 맞출 것
- 이벤트 이름: `core.latch`, `core.approval_requested/resolved`, `core.ui_focus`, `core.activity`(1 Hz 집계), `core.worker_state`, `core.epoch_changed`, `core.write_recovered`, `core.heartbeat`, `core.env`.
- 장애 화면: worker 크래시(해당 패널만 빨간 배너 + 재기동 횟수), `daemon restarted (epoch 3→4)` 배너, latch는 재시작 뒤에도 유지(STOP 띠 유지).
- `epoch` = `stream_epoch`, 장치·링크 세대는 `generation`(CORE §6.1.1) — UI 세대 배너는 `generation`을 본다.
- STOP 해제 동사 = `core.stop.release`(viewer). r1-opus "재개(tx resume)"를 이 RPC로.
- UI 끊김 시 입력 lease 5 s 유예 후 해제(CORE §3.6) — UART `input_dirty` 규칙과 함께 표시.

---

## 3. TEST 초안 ↔ 테스트 실행 화면 (r1-opus §5.1.6·§5.9)

| 항목 | TEST 초안 | r1-opus | 판정 | 제안 |
|---|---|---|---|---|
| 이벤트 이름 | `test.session.start/end`, `test.step.begin/end`, `test.verdict`, `test.note`, `test.ask` (TC-C1) — CAN은 `test.step` | `test.session_state`, `test.step_state`, `test.step_marker`, `test.verdict`, `test.ask`, `test.report` | 둘 다 수정 ⚑ | CORE §4.7 규칙(kind 2토큰 `<domain>.<event>`)에 맞춰 `test.session_start/end`, `test.session_state`, `test.step_begin/end`, `test.verdict`, `test.note`, `test.ask`, `test.answer`, `test.report_ready` (쟁점 11) |
| 세션 상태 | PLANNED / WAITING_APPROVAL / RUNNING / PAUSED / STOPPED / DONE | 없음 | TEST | 채택 |
| 판정값 | `pass | fail | inconclusive | blocked | skipped`, 아이콘 ✓✗?⊘⏭ | PASS / FAIL / inconclusive | TEST | 채택. TC 판정 = 필수 단계 최악(`fail > blocked > inconclusive > pass`, R-6) — UI는 서버 값을 표시만 |
| 증거 검증 형식 | `verified{exists, in_window, predicate, detail}` + 창 안 gap이면 "없음·시간 내" 주장 불가 | `check{exists, matches, detail}` | TEST | 3항목 `존재 · 창 · 술어` + gap 상태 한 칸(`gap: 없음 / record.gap 있음`) |
| 증거 REF | `<domain>:<channel>@seq:<a>[..<b>][#<pointer>]`, `la:<dev>@capture:<n>#row:a..b` | 형식 없음 | TEST ⚑ | 채택. UART `uart:dut:ev:N` 정리 요청. 셸 `ctx.ui.reveal(ref)`가 파싱 |
| 판정 거부 표시 | exit 4 `E_EVIDENCE_MISSING/MISMATCH/OUT_OF_WINDOW` + hint | "PASS 대신 inconclusive 주황" | TEST | 거부 사유를 현재 단계 칸에 그대로(예: `seq:17990은 S3 시작 1.2 s 전`) |
| 사람 개입 | 일시정지/계속, 중단(=`end --abort`), 건너뛰기(사유), 재시도, **판정 수정(사유 필수, viewer만, 원판정 보존 R-5)**, `ask` 응답, **증거 후보 클릭 → 그 이벤트로 검증 요청**, `manual` 단계는 사용자만 판정(R-7), 사용자 이벤트 "판정 제외" 표시 | 일시정지·건너뛰기·중단·수동 PASS/FAIL·질문 응답 | TEST(더 완전) | 판정 수정·후보 클릭·판정 제외 표시 추가. 일시정지 중 user 조작은 `test.note{operator_action}` 자동 → 해당 창 판정에 경고 배지 |
| 계획 검토 | **계획 검토 모달**: 원문 문장 ↔ 해석 단계 대조, 등급, 승인 묶음, plan hash → 계획 승인 + 일괄 승인(R4 제외) | 사전 승인 요약 모달 | TEST | 채택. 이 모달은 R3를 포함하므로 **존재 확인 대상**(쟁점 6). 술어를 사람 문장으로 렌더링해 표시(TEST 리스크 대응) |
| 사전 승인 기한 | "세션 시작 10분 내 개시, 세션 중 유효, `max_test_duration` 1 h"(A9 확장 요청) | "8:12 남음" 하나 | TEST | 두 값 표시: `개시 기한 08:12` → 시작 후 `세션 유효 52:10`, 횟수 잔량(`psu.out on 3/5`) |
| 지연 | 상태 변경 → ≤ 200 ms, 명령 발행 → ≤ 100 ms, 실시간 증거는 도메인 목표, LA는 캡처 후 | 언급 없음 | TEST | 수용 |
| 보고서 링크 | `/reports/<sess>`, 증거 링크 `loom://records/<session>?seq=a..b` | 보고서 링크 | 수정 ⚑ | 사용자 지정 스킴 `loom://`은 브라우저가 열지 못함 → `http://localhost:<port>/ui/#/records/<session>?seq=a..b`. 보고서 경로는 CORE `/api/v1/report/<sess>.html`로 통일 |
| M 배정 | **TEST M1 = sim TC-1을 웹 UI 테스트 화면으로 실행**(계획 검토·일괄 승인·단계·증거·판정 수정·보고서) | 테스트 화면 UI-M3 | 충돌 ⚑ | 쟁점 14 |
| 타임라인 단계 띠 | 모든 레인에 단계 띠(TEST M1 수용 ②) | 통합 타임라인 M3 | 충돌 | 테스트 화면 최소판에 "단계 띠가 있는 간이 타임라인" 포함, 범용 통합 타임라인은 M3 |
| 실행 전 질문 조건 | env=vehicle R2+ 시작 전, 같은 단계 2회 inconclusive 등 → `ask` | — | TEST | 질문 대화상자는 승인 모달과 다른 색·위치(승인으로 오인 방지), 남은 시간 표시 |
| 기록 불능 | A12 + **pass 판정도 중단**(TC-C8) | — | TEST | 테스트 화면 상단에 `REC FAILED — PASS 판정 불가` |

---

## 4. 자기 승인 차단 근거 보강 (데스크 조사)

조사 방법: 웹 문서·명세·소스 열람(2026-11-10). 직접 실험은 하지 않았다(스파이크 S-O1로 남김). 신뢰도: **확인**(1차 출처 본문) / **추론** / **[미확인]**.

**결론**: r1-opus §10.3의 추천(B = WebAuthn 존재 확인)은 유효하다. 단, 조건이 두 가지 붙는다. ① 오리진을 `localhost`로 서비스해야 한다(127.0.0.1에서는 WebAuthn이 실패한다). ② **자격증명 등록 자체를 사람만 할 수 있게 막아야 한다.** 등록이 열려 있으면 에이전트가 가상 인증기로 자기 자격증명을 등록해 통과한다. r1-opus는 ②를 빠뜨렸다.

### 4.1 (a) `http://localhost` — 보안 컨텍스트와 RP ID
| 사실 | 신뢰도 | 출처 |
|---|---|---|
| `http://localhost`와 `http://127.0.0.1` 모두 "potentially trustworthy" = 보안 컨텍스트(`127.0.0.0/8`, `::1`, `localhost`) | 확인 | https://w3c.github.io/webappsec-secure-contexts/ |
| WebAuthn L3는 "origin의 host가 localhost이고 scheme이 http"인 경우를 명시적으로 허용하고 `http://localhost:8000`을 유효 오리진 예로 든다 | 확인 | https://www.w3.org/TR/webauthn-3/ |
| **IP 주소는 RP ID가 될 수 없다**("Only the domain format of host is allowed"). 유효 도메인이 아니면 `create()/get()`이 `SecurityError` → 127.0.0.1은 보안 컨텍스트지만 WebAuthn은 실패 | 확인 | 같은 명세 |
| Chrome·Firefox의 `http://localhost` 동작 | 추론(명세·개발 가이드 다수, 코드 미확인) | — |
| Safari: WebKit 버그 302813 재현이 `http://localhost`에서 등록 동작을 전제 → Safari도 동작 | 추론 | https://bugs.webkit.org/show_bug.cgi?id=302813 |
→ UI 결정: 오리진 정본 `http://localhost:<port>`(바인딩은 127.0.0.1 유지), `127.0.0.1`로 들어오면 `localhost`로 리디렉트. CORE 초안의 예시 URL(`127.0.0.1:47123`)을 바꿔야 한다(쟁점 15).

### 4.2 (b) Playwright/Puppeteer · CDP 가상 인증기
| 사실 | 신뢰도 | 출처 |
|---|---|---|
| CDP `WebAuthn.enable` = "start intercepting credential storage and retrieval with a virtual authenticator" | 확인 | https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/WebAuthn.pdl |
| 활성화하면 "브라우저를 실제 Authenticator Discovery에서 떼어 내고 Virtual Discovery를 꽂는다" — 실 인증기를 **대체**하며 중계하지 않는다 | 확인 | https://developer.chrome.com/blog/webauthn-tab/ |
| 따라서 가상 인증기는 Touch ID/Windows Hello에 등록된 키로 서명할 수 없다. 서버가 저장한 공개키로 서명을 검증하면 가상 assertion은 실패한다 | 추론(강함) | 위 두 출처 |
| **위험**: 에이전트가 새 **등록**을 일으킬 수 있으면 가상 자격증명을 등록하고 그것으로 승인할 수 있다. 플랫폼 passkey는 보통 attestation `none`이라 attestation으로 실/가상을 가르기 어렵다 | 추론 | — |
| Playwright·Puppeteer는 raw CDP 세션으로만, Chromium에서만 이 도메인을 쓴다 | 확인(2차 출처) | https://www.corbado.com/blog/passkeys-e2e-playwright-testing-webauthn-virtual-authenticator |
| macOS Chrome의 Touch ID 자격증명은 브라우저 프로필에 묶인다(다른 프로필 것은 "must be ignored") → 새 자동화 프로필에서는 보이지 않음 | 확인(과거 소스 스냅샷) | https://chromium.googlesource.com/chromium/src/+/c3323af925e5d404bc1371f22a45e61c5abeb7e7/device/fido/mac/credential_metadata.h |
| 가상 인증기가 없으면 passkey 선택기·OS 프롬프트는 페이지 DOM 밖(브라우저/OS UI)이라 사람이 응답해야 한다 | 추론(명시 문서 못 찾음) | — |

### 4.3 (c) 확장 기반 자동화 · computer-use(화면 클릭)
| 사실 | 신뢰도 | 출처 |
|---|---|---|
| **`chrome.debugger` 허용 도메인 목록에 WebAuthn이 있다** → 확장 기반 에이전트(사용자 실 프로필 조작형)도 그 탭에 가상 인증기를 붙일 수 있다 | 확인 | https://developer.chrome.com/docs/extensions/reference/api/debugger |
| 그래도 실 자격증명은 못 쓴다. 등록이 열려 있으면 (b)와 같은 우회 | 추론 | — |
| macOS: High Sierra 무렵부터 SecurityAgent·키체인 프롬프트에 대한 합성 클릭을 차단(과거 우회 CVE-2015-5943 등은 패치) | 확인(오래된 자료) | https://objective-see.org/blog/blog_0x36.html |
| 현재 Chrome/iCloud 키체인 Touch ID 시트에 "암호 사용" 대체가 있는지, 합성 입력이 그 칸에 들어가는지 | [미확인] | — |
| Windows: 2026-01 Windows Security 자격 대화상자(KB5074109)가 "일반 사용자 권한으로 실행되는 다른 앱을 차단" — KeePass 자동 입력이 막힘 | 확인(KeePass 관리자) | https://sourceforge.net/p/keepass/bugs/2413 |
| RPA 포럼: 같은 대화상자에서 UI Automation·후킹 차단 보고 | 확인(포럼만) | https://community.blueprism.com/t5/Product-Forum/Windows-Security-Popup-CredentialUIBroker-exe/m-p/124518 |
| 패치된 Windows 11에서 SendInput으로 Hello PIN을 칠 수 없다는 Microsoft 공식 문서 | [미확인] | — |
| 주의: 2026-08 보고 — 사용자 세션의 악성코드가 Windows Hello for Business 키를 PIN·생체 없이 사용(Entra ID). 소비자 passkey에 해당하는지 | [미확인] | https://startwithidentity.com/blog/2026-08-07-windows-hello-for-business-keys-abused-for-entra-id-persistence.md |
| 데몬 바이너리·설정을 고칠 수 있는 에이전트에게는 어떤 UI 게이트도 무의미 | 추론 | — (D6 위협 모델 밖) |

### 4.4 (d) Linux의 플랫폼 인증기 현실
| 사실 | 신뢰도 | 출처 |
|---|---|---|
| Linux Chrome에는 OS 생체 플랫폼 인증기가 없다. passkey는 Google Password Manager(GPM)에 저장, 휴대폰 교차 기기(hybrid)는 모든 Chrome 플랫폼에서 동작 | 확인 | https://developers.google.com/identity/passkeys/supported-environments |
| GPM은 Android 기기가 없는 사용자에게 데스크톱에서 처음 passkey를 만들거나 쓸 때 PIN 설정을 요구 | 확인 | https://developer.chrome.com/blog/passkeys-gpm-desktop |
| Linux에서 매 assertion마다 GPM PIN을 다시 묻는지 / GPM passkey가 RP ID `localhost`에서 동작하는지 / X11·Wayland 합성 입력이 그 PIN 칸에 들어가는지 | [미확인] (합성 입력에는 macOS·Windows보다 약할 것으로 추론) | — |
| USB 보안 키: Chrome은 기본 지원, Firefox는 USB HID CTAP2 지원(약 109 이후). Linux에는 시스템 전역 FIDO API가 없어 브라우저가 각자 구현 | 확인 | https://bugzilla.mozilla.org/show_bug.cgi?id=1530373 , https://developers.yubico.com/WebAuthn/WebAuthn_Browser_Support/ |
| Firefox(Linux)는 플랫폼 인증기·hybrid 없음 → **보안 키만** (hybrid 버그 1543674 미해결 P3) | 확인 | https://bugzilla.mozilla.org/show_bug.cgi?id=1543674 |
| Linux에서 가장 강한 사람 존재 요소 = USB 보안 키(터치 + PIN) | 추론 | — |

### 4.5 r1-opus §10.3에 반영할 수정
1. **등록 게이트(신규, 필수)**: WebAuthn 자격증명 등록은 (i) 벤치 TTY `loom ui pair --register`가 보여 주는 1회 코드, 또는 (ii) 이미 등록된 자격증명의 assertion이 있어야만 허용. 등록·삭제는 감사 + 모든 탭에 알림("새 승인 장치가 등록됨"). 이게 없으면 확장·CDP 가상 인증기로 B가 무력화된다.
2. **승인 시 서버 검증**: assertion은 등록 때 저장한 공개키로 검증, `userVerification: "required"`, 서명 카운터 기록. 승인 감사에 자격증명 id·AAGUID(있으면)·UA 기록.
3. **오리진 `localhost` 정본**(4.1).
4. **Linux 기본값**: 보안 키가 있으면 webauthn, 없으면 `out_of_band_code`(벤치 TTY 또는 OS 알림 — 화면을 읽는 computer-use에는 약하다는 한계 명시). GPM PIN 경로는 [미확인] 항목이 풀릴 때까지 기본값으로 쓰지 않는다.
5. 스파이크 S-O1 항목 갱신: (1) `localhost`에서 Chrome/Edge/Firefox 등록·UV, (2) 등록 게이트 상태에서 Playwright·확장 `chrome.debugger` 가상 인증기로 승인이 거부되는지, (3) macOS Touch ID 시트·Windows Hello 대화상자에 합성 입력이 들어가는지, (4) Linux 보안 키·GPM PIN 경로.

---

## 5. R3 통합 때 리드가 결정할 쟁점

| # | 쟁점 | 선택지 | 추천 | 이유 (한 줄) |
|---|---|---|---|---|
| 1 | 출처 색 | CAN(AGT 주황·USR 파랑) / 다수안(AGT 청록·USR 노랑·SCR 보라·EXT 보라 점선) | **다수안** | UART·UDS·탐색·r1-opus가 이미 청록/노랑이고 문자 병기라 CAN 전환 비용이 가장 작다 |
| 2 | 승인 입력 규칙 | 무장 지연 0.7 s(UART·r1-opus) / 1 s(UDS); 키 `Y/N`(UDS) / 기본 포커스 거부 + Enter=거부 | **1 s, 승인은 대문자 `Y` 또는 버튼만, Enter·Esc는 승인하지 않음, 기본 포커스 거부** | 웹은 마우스 이동·키 반복이 섞여 TUI보다 오입력 위험이 크고, 세 팀 규칙의 교집합이다 |
| 3 | M1 승인 범위 | M1 승인 없음(r1-opus) / M1 R2 카드(UDS E2E t2, CORE M1 R0–R2) | **M1 = 승인 대기열 + R2 확인 카드(존재 확인 없음), R3/R4·존재 확인은 M2** | UDS M1 수용 시험과 CORE M1 범위가 R2 뷰어 경로를 이미 요구한다 |
| 4 | 사용자 본인이 시작하는 R3/R4 (CAN IG 주기 송신, UART 부트로더 시그널) | 셸 확인창 1회(CAN) / 승인 대기열에 넣고 본인 승인(CORE "1클릭+코드") | **"요청+승인" 합친 본인 확인 모달 1회 + 승인과 같은 존재 확인** | 브라우저 자동화가 user로 위장해 R3를 스스로 확인하는 경로(S5)를 승인과 같은 수준으로 막는다 |
| 5 | 웹 자격 발급 주체 | TTY·데스크톱 셸에서만(UDS) / 에이전트 `loom ui open`도 bootstrap 포함 브라우저 열기(CORE) / 데몬이 열되 비밀 비노출(r1-opus) | **에이전트가 연 브라우저 = watch만. operate 승격 = 브라우저 프로필당 1회 사람 행동(TTY `loom ui pair` 코드 또는 WebAuthn 등록), approve = 승인마다 존재 확인** | 에이전트가 사용자에게 화면을 띄워 주는 편의(02 원칙)를 지키면서, 에이전트 경로로 쓰기 자격이 생기지 않게 한다. 참고: Linux `xdg-open`은 `$BROWSER`를 따르므로 에이전트가 자동 기동한 데몬은 에이전트 환경을 물려받을 수 있다 — 비밀이 든 URL을 OS 오프너에 넘기는 방식만으로는 의도적 우회를 못 막는다(위협 모델 밖이지만 문서화) |
| 6 | 존재 확인 형식 | `presence_code`(CORE, 화면 표시 코드) / WebAuthn / 대역 외 코드 | **`presence{method, proof}` 일반화. 기본 webauthn(가능 호스트) + 등록 게이트 필수, 불가하면 oob 코드, `code`(화면 표시)는 오클릭 방지용으로만** | 화면에 보이는 코드는 화면을 읽는 자동화가 통과하고, 등록이 열린 WebAuthn은 확장의 가상 인증기가 통과한다(§4.3) |
| 7 | 지연 보고 RPC | `ui.latency_sample`(CAN) / `core.ui.latency_report`(UART) / `core.metrics.report`(core-ui) / `core.render_ack`(r1-opus) | **`core.ui.latency` 하나: `{mode: sample\|report, …}` — sample은 표본 이벤트별 paint 시각(시험 하네스), report는 10 s 집계(p50/p95/p99, skew_err)** | 같은 측정을 네 이름으로 만들면 CORE가 네 번 구현한다 |
| 8 | 시계 RPC | `core.time`(CORE·r1-opus) / `core.clock.ping`(UART) / wall 기준(UDS) | **`core.time` + mono 기준** | CORE가 이미 수용, wall은 NTP 보정으로 튄다 |
| 9 | 가리키기·딥링크 | `loom ui open <panel-ref>`(CORE·CAN) / `loom ui show`(r1-opus); URL 형식 | **`loom ui open <panel-ref>` 하나, 이벤트 `core.ui_focus`, URL `/ui/#/p/<domain>/<panel>?<props>&at=<cursor>`, 기본은 묻기 토스트** | 명령 하나가 "열기"와 "가리키기"를 겸하면 스킬이 단순하다 |
| 10 | 쓰기 행 이벤트 | `can.tx` / `uart.write` / `uds.guard…verify` / CORE `<domain>.write` | **kind는 도메인 자유, 대신 `request_id` + `payload.write_state`를 스키마 수준 필수로** | UI 행 갱신은 이 두 필드면 충분하고 도메인 kind 개명 비용을 피한다 |
| 11 | 테스트 이벤트 이름 | `test.step.begin`(TEST, 3토큰) / `test.step`(CAN) / `test.step_marker`(r1-opus) | **`test.step_begin`/`test.step_end` 등 2토큰** | CORE §4.7이 kind를 2토큰으로 정했고 Registry가 위반 시 기동 실패한다 |
| 12 | 증거 REF 형식 | TEST `<domain>:<ch>@seq:a..b#ptr` / UART `uart:dut:ev:N` | **TEST 형식** | 검증기·보고서·UI 이동이 같은 문자열을 써야 한다 |
| 13 | 복구 송신 API | `core.safe_state.list` + 도메인 RPC(CORE) / `core.recover{target}`(UDS) / `core.recover.list/run`(r1-opus) | **`core.safe_state.list` + `core.safe_state.run{ids[]}`(인터록 결과·`finally_blocked` 항목별 반환)** | A15 "1키"는 여러 도메인 동작을 한 번에 보내야 하고, 결과를 항목별로 보여야 한다 |
| 14 | 테스트 실행 화면 일정 | TEST M1 / UI M3 | **UI M2에 최소판(단계 표·현재 단계·실시간 증거·검증 3항목·판정 수정·질문·계획 검토 모달·간이 단계 띠), M3에 통합 타임라인·보고서 뷰** | TEST M1은 CORE 세션 엔진(P2) 뒤라 UI M2와 같은 시점이고, P1 걷는 뼈대를 키우지 않는다 |
| 15 | 웹 기본 경로·오리진 | `/ui/*`·`/api/v1/*`·`127.0.0.1`(CORE) / `/`·`/data`·`localhost`(r1-opus) | **경로는 CORE, 오리진 이름은 `localhost` 정본** | 경로는 CORE가 소유, 오리진은 WebAuthn RP ID 제약(§4) 때문에 UI가 요구 |
