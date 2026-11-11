docs/plan/02-core-principle.md 준수.

# UI P1 M1 보고 — 수용 시험 스위트 + mock 시나리오 (ui-sol 대행)

작성: ui-sol 대행(Claude 서브에이전트), 2026-11-11. 작업 공간 `.worktrees/ui`(브랜치 `ui/main`, 기준 `2aa7e59`), **커밋하지 않음**, git stash 쓰지 않음. 수정 범위: `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`, `ui/src/net/**`(api.ts 제외). ui-opus·리드 소유 파일과 Rust crate는 건드리지 않았다(아래 §6의 `Cargo.lock` 참고).

## 0. 결과 요약

| 항목 | 결과 |
|---|---|
| `cd ui && pnpm check` (schema·tsc·eslint·vitest) | **통과** — 28 파일 308 시험(ui-opus 것 포함) |
| 내 범위 vitest | net 7 파일(신규 `contract.test.ts` 6 + core 4 + mux 3), mock 2 파일 31(신규 6) |
| **Playwright `mock` 프로젝트** (45) | **42 통과 · 1 실패 · 1 예상 실패 · 1 건너뜀** (3.3분, UART flood 60 s 포함) |
| **Playwright `replay` 프로젝트** (11, `@replay` 태그) | **9 통과 · 2 예상 실패(CORE 계약 차이)** (≈ 50 s) |
| 기존 `pnpm e2e`(smoke·can.spec) | 4/4 통과(mock 변경 후에도) |

- **실패 1 (mock)**: eth 수용 2 — 진짜 UI 결함. `VirtualTable`이 남는 풀 행에 `hidden` 속성만 주는데 `shell.css`의 `.vt-row{display:grid}`가 그것을 덮어 **숨긴 행이 화면에 그대로 보인다**. 배지는 "완전(저널 기준) · 2행"인데 화면에는 6행(이전 라이브 행)이 보임. 한 줄 수정: `.vt-row[hidden]{display:none}`(ui-opus, §5 U-1). 같은 원인으로 UDS 콘솔에서 한 request_id의 옛 단계 행(◑보냄)이 겹쳐 보이는 것도 봤다(T09 관찰기는 `[hidden]`을 따로 세어 판정에서 분리).
- **예상 실패(`test.fail`, 고쳐지면 Playwright가 "예상 실패가 통과"로 알려 준다)**: T07 CSP·Permissions-Policy 헤더(Vite dev 서버엔 없음, loomd가 /ui/*를 서빙하는 P1 몫), replay의 gap 3층(C-4), replay의 watch STOP(C-5).
- 건너뜀 1: T02 replay 전용 시험이 mock 프로젝트에서 skip(의도).

## 1. 두 백엔드 구성 (`ui/e2e/m1.config.ts`, 신규)

리드 소유 `playwright.config.ts`는 고치지 않고 **e2e 폴더 안에 별도 설정**을 뒀다(전용 포트라 사람·ui-opus가 7420/7421에 띄워 둔 dev 서버를 잘못 재사용하지 않음).

| 프로젝트 | 경로 | 비고 |
|---|---|---|
| `mock` | Vite :7440 → `pnpm mock` :7441 | 모든 시험 |
| `replay` | Vite :7450, 페이지는 loomd에 **직접** 연결(DEV 전용 `?loom_daemon=`) | `grep: /@replay/` |
| | `loomd --fixture-replay builtin --scenario all --echo-writes` :4790 | 기본 |
| | `… --scenario normal --rights watch` :4791 | 자격 시험(T06·T08) |
| | `… --scenario saturate` :4792 | `slow_consumer` 재연결(T05) |

loomd는 `cargo run -q -p loom-gateway --bin loomd`(작업공간 루트, 이미 빌드되어 있으면 수 초). 첫 `cargo build -p loom-gateway`는 19 s였다.

**DEV 전용 시험 훅(`src/net/index.ts devWsUrl`, production 번들에는 없음)**: 페이지 query `?loom_daemon=ws://localhost:<port>/rpc`(루프백 ws만 허용), `?loom_rights=watch|operate|approve`(→ WS `?rights=`, mock 전용). 단위 시험 있음.

## 2. 시험 목록과 결과

DOM으로 판정한다(`data-request-id`·`data-origin`·`data-state`·`data-seq`·`data-gap`·문구·색). Node 쪽 `Daemon` 클라이언트(`e2e/m1/lib.ts`)는 에이전트 역할(mock.agent)·주입·**기준 구독자**(DOM과 비교할 seq 목록)로만 쓴다.

| ID | 시험 (파일) | mock | replay | 측정·비고 |
|---|---|---|---|---|
| UI-T01 | CAN: 열기→snapshot 1 s, 이후 seq 중복·누락 0 (`t01`) | ✓ | ✓ | net open→첫 행 mock 34 ms / replay 135 ms. 일시정지 후 가상 목록 전체를 스크롤해 `data-seq` 수집 → 단조 증가 + 기준 구독자 대비 누락 0(snapshot→live 경계 포함) |
| UI-T01 | UART trace: 백로그 500 ms (`t01`) | ✓ | ✓ | 34 / 84 ms |
| UI-T02 | CAN `can.send` → 같은 request_id 행·AGT (`t02`) | ✓ | — | |
| UI-T02 | UDS read-did → 같은 request_id 행·AGT·확정 | ✓ | — | |
| UI-T02 | UART `uart.send` → 상태줄 `AGT ▶ "uname -a⏎"` | ✓ | — | 터미널에 request_id DOM 속성 없음 → §5 U-2 요청(속성이 생기면 시험이 자동으로 비교) |
| UI-T02 | replay 생성기 agent UDS 행(`replay-uds-N`)이 기준 request_id로 AGT 행 | — | ✓ | `uds.tx/rx/timeout` kind도 request_id로 한 행 |
| UI-T03 | CAN 행 등장 vs `ts.mono_ns`(시계 불확실성 안) (`t03`) | ✓ | ✓ | mock n=87 p50 44 / p95 57 ms ±0.15 · replay n=894 p50 29 / p95 53 ms ±0.05 (수치 판정은 `LOOM_PERF=1`) |
| UI-T03 | UART 표식 줄 / UDS 최종 단계 | ✓ / ✓ | — | UART p95 42 ms, UDS p95 67 ms |
| UI-T03 | 렌더 보고 `core.metrics.report{ui.render}` + `ui.render.sample` | ✓ | — | 패널 uart.terminal·can.trace·uds.console 보고 수신(mock이 연결별 64건 보관) |
| UI-T04 | UART 키→칩 `나`→에이전트 대기 칩→Ctrl-C→`others_since_last≥1`+`⇢ 에이전트에게 전달됨` (`t04`) | ✓ | — | V5: 자동화 브라우저는 loomd 상대로 watch 고정 → 사용자 쓰기는 mock에서만 |
| UI-T04 | CAN 송신줄 1회 → `USR(나)` 행 확정 | ✓ | — | |
| UI-T04 | UDS 명령줄 `22 F190`(dry-run은 행 안 생김) + [제어 가져오기] → 에이전트 R2 요청 exit 6·행 ✗ | ✓ | — | |
| UI-T05 | gap 3층 문구·색(display 회색 계열, record/source 빨강 `.ct-loss`) (`t05`) | ✓ | 예상 실패 | C-4 |
| UI-T05 | heartbeat 3 s 중단 → STALLED 배지·배너·쓰기 잠금(송신 RPC 0) → 복구 | ✓ | — | 송신 입력칸은 STALLED 중에도 활성(Enter 시 거절) → §5 U-3 |
| UI-T05 | 재연결 이음: 끊김 뒤 CAN seq 중복·누락 0 | ✓ | ✓ | replay는 `saturate` 데몬의 `core.closed{slow_consumer}` → 재연결 배너 → 행 계속, 중복 0 |
| UI-T05 | UART flood 92 KB/s × 60 s: 화면 생략 0·heartbeat 시계 연속·STALLED 0·rAF 최대 공백 < 1 s | ✓ | — | 최대 rAF 공백 130 ms. `LOOM_FLOOD_S`로 단축 가능 |
| UI-T06 | 클릭 → 1 프레임 안 띠 → latch → STOP 패널 (`t06`) | ✓ | ✓ | 클릭 직후 다음 rAF에서 `data-phase` = requesting/locked |
| UI-T06 | STOP 중 에이전트 쓰기 거부 행 ✗·사용자 UART 키 통과·복구 송신 1키(Enter) | ✓ | — | 인터록 막힌 항목은 "막힘"으로 보임 |
| UI-T06 | WS 끊김 중 HTTP `POST /api/v1/stop` | ✓ | — | replay에 HTTP STOP 없음(C-7) |
| UI-T06 | watch 탭 STOP 가능·해제 불가 | ✓ | 예상 실패 | C-5 |
| UI-T07 | 장치 질의(DA1·DA2·DSR·CPR·DECRQSS·OSC 4/10/11/12 `?`) → 의도 없는 TX 0(서버 집계)·차단 수 표시 (`t07`) | ✓ | — | |
| UI-T07 | readonly(watch) 탭: 질의 + 키 입력 → `uart.input` 0 | ✓ | — | |
| UI-T07 | 패널 lint 위반 fixture → ESLint 실패(I-UI1 규칙 3종) | ✓ | (node) | `fixtures/lint/bad-panel.ts.txt`를 `src/panels/bad/badPanel.ts` 경로로 lintText |
| UI-T07 | CSP·Permissions-Policy 헤더 | 예상 실패 | — | §6 L-3 |
| UI-T08 | watch 탭: 송신·입력 비활성, STOP 활성 + 서버도 거부(직접 RPC) (`t08`) | ✓ | ✓ | replay는 :4791 watch 데몬 |
| UI-T08 | R2 카드: 자동(카드 없음) / 정책 켬 → 승인 대기열 카드 → 승인 → 쓰기 진행 | ✓ | — | 토큰 경로는 CLI 쪽 |
| UI-T09 | UDS 0x78 ×5 한 행 `pending ×1..5 → ●`, 행 수 1(MutationObserver) (`t09`) | ✓ | — | |
| UI-T09 | 역순 주입 → 단계 후퇴 없음 | ✓ | — | |
| UI-T09 | `outcome_unknown` 주황 | ✓ | ✓ | rgb(224,160,48) |
| UI-T10 | 패널 100회 열기/닫기 → listener·노드·서버 구독 원상 (`t10`) | ✓ | ✓ | listener 213→213, 서버 구독 3→3(replay는 heartbeat `sub_stats`), heap +1.6 MiB |
| eth 1 | 캡처 + read-did → DoIP 행(`data-seq`·`data-request-id` = 기준) → `↔UDS` 칩 → UDS 콘솔 행 선택·포커스 (`eth`) | ✓ | — | replay에 eth 없음 |
| eth 2 | 필터 `doip.type == 0x8001` → 완전 행 수 = `eth.query` 수, 잘못된 식 밑줄 + "위치 12: 값 필요" | **✗** | — | U-1(숨긴 행 표시) |
| eth 3 | 편집 후 송신(DID 변경) → USR tx 행 + NIC 관찰 ✓ → 에이전트 다음 결과 `others_since_last=1` | ✓ | — | |
| eth 4 | 200 kpps 재생 → "표시 1/1000 샘플링 · 기록 100 %" → 일시정지 → 완전 행 수 = `eth.query` | ✓ | — | |
| eth 5 | 커널 드롭 → 빨간 틈 행 + 배지 증가 = `source.gap.count` | ✓ | — | |
| eth 6 | 두 번째 브라우저 늦은 합류 → 같은 마지막 seq·캡처 상태 | ✓ | — | |
| eth 7 | STOP → 에이전트 `eth.send` 거부, 캡처 행 계속 | ✓ | — | STOP 패널이 목록을 밀어 패널이 "스크롤백"으로 오인 → 완전 모드로 전환됨(§5 U-4) |
| test 1–5 | 단계 begin → ▶·띠 / 지어낸 seq PASS 거부 표시·판정 불변 / 늦은 합류 동일 / STOP 시 finally 자동 실행 안 함 + 복구 키 / 에이전트 판정 수정 거부·approve 뷰어 수정(사유 필수) (`test-screen`) | ✓ ×5 | — | |
| psu | watch 탭 비상 OFF: 확인 없음·STOP 중 가능·출력 OFF, 에이전트 `psu set` 즉시 표시, STOP 중 에이전트 거부 | ✓ | — | |

## 3. mock 시나리오·계약 맞춤 (`ui/tools/mock/**`)

- **eth** (`eth.ts`, 신규): sim tap `lan1` — `eth.capture.start/stop` → `eth.capture_state{state, sim, drop_stats, ts_precision, started_mono_ns}`; mock `uds.request`마다 DoIP 요청/응답 `eth.pkt`(+`link{request_id, role, confidence}`, `attributed_origin`); `eth.filter.check`(`doip`, `doip.type == 0x8001`, `doip.type ==` → `{ok:false,pos:12,expected:'값'}`), 구독 `filter.ext.eth_filter` 푸시다운, `eth.query`(목록: 저널 링 ≤ 200k에서 `range.to`까지 최근 ≤ 2,000행 + `truncated` / 상세: 69 B DoIP 프레임 + `{off,len}` 트리); 고부하 재생(seq를 `Hub.reserve`로 확보해 **모든 패킷이 저널에 seq를 갖고**, 라이브엔 1/N + `eth.batch{covered_first_seq, covered_last_seq, total, shown}`); 커널 드롭 `source.gap{cause:kernel_drop}`; `eth.send` dry-run(행 없음) / 실송신(`eth.tx` 단계 + `tx_request_id` 캡처 복사본); 캡처 중 SOME/IP-SD 배경 250 ms.
- **psu** (`psu.ts`): SimPsu `bench` 1 Hz `psu.state` + `metrics`, `psu.out`/`psu.set` → `psu.cmd` 쓰기 행, `psu.out{off}` = watch 허용, STOP의 `outputs_held`, safe_state `psu.bench_off`(emergency_off), `mock.inject psu_gap`.
- **test** (`testRun.ts`): TC-1(지어낸 seq PASS 거부·사용자 메모·mismatch→fail·질문), `test.pause/resume/skip/retry/answer/end/plan.show`, `test.verdict{override}`(viewer approve만, 사유 필수; 에이전트 → `E_DENIED`), 에이전트 `test.step`·`test.verdict`.
- **ui-opus 요청(M-1~M-5) 반영**: UDS dry-run = 미리보기만(행·감사 없음), NRC 종결 = `failed`, `uart.input_owner.viewer` = `hello.conn_id`, `uart.agent_activity{deferred_send→idle}`, 사용자 줄 완료·Ctrl-C에 `core.activity{input}` + 입력권 반환, 에이전트 결과 `others_since_last{count, origins}`, `uds.lease.takeover` → `uds.state.lease` + 에이전트 R1+ 요청 `lease_preempted`(exit 6). `mock_pending`/`mock_pending_ms`로 0x78 횟수 조절.
- **CAN v1(`docs/impl/requests/can-ui.md`)**: `can.send{frame | id,ext,fd,brs,esi,data, count, gap_ms, dry_run}`, `can.frame{id, ext, fd, brs, esi, rtr, dlc(코드), len, bytes, frame_index}`, 에코 `can.frame`에 같은 request_id, dry-run `{risk:R1, reason:'sim channel', bytes, policy:'auto'}`, 에이전트 `can.trace.query` → `core.activity{action:'read', range}`. **결정**: `can.tx.send`는 패널 전환(ui-opus 작업 중) 동안 같은 처리기의 별칭으로 남김(근거: CAN 답 §3 + 기존 패널·e2e가 아직 그 이름). 시나리오·fixture 생성기는 `can.send`로.
- **계약 맞춤**: snapshot part **1부터**(schema/v1 "1-based", loomd 동일) — net·mock·fixture 모두. `others_count` 문자열(u64). STOP(`core.stop`)은 **모든 자격**(v1.1), 해제 operate. dry-run은 자격 무관(R0 질의). safe_state id `<domain>.<name>`, 결과는 schema/v1 모양 + 지금 셸 어댑터가 읽는 옛 필드(상위 집합, §4 C-8).
- 시험 보조: `mock.inject` `late_backlog`·`uart_queries`·`refuse_ws`(HTTP STOP 시험)·`conn_stats{reports}`(연결별 RPC 횟수·구독 수·presence·metrics 보고)·`reset`·`auth_required`(쿠키 없는 업그레이드 401)·`eth_*`·`psu_gap`·`test_tc1`.
- C2: 연결별 호출 집계 ≤ 64 메서드, 보고 링 64, eth 저널 200k(넘치면 오래된 것 버림 + 질의 `truncated`), 질의 ≤ 2,000행, 플러드 1개, UART 사용자 줄 256자, UDS lease ≤ 32 대상, test 단계 고정 5.

## 4. CORE(replay)와의 계약 차이 — 재현 절차 포함

재현 공통: `cargo run -p loom-gateway --bin loomd -- --fixture-replay builtin --scenario all --echo-writes --port 4790 --allow-origin http://localhost:7450` 후 `pnpm exec playwright test -c e2e/m1.config.ts --project=replay`. 실제 프레임은 `fixtures/wire/replay-{late-join,writes}.jsonl`(`tools/mock/capture-replay.ts`로 녹취)에 있고 `src/net/contract.test.ts`가 현재 모양을 고정한다(CORE가 바뀌면 시험 차이로 드러남).

| # | 차이 | 영향 | 조치 |
|---|---|---|---|
| C-1 | **Vite 프록시 경유 WS가 403**: 프록시가 `Host: localhost:<vite>`를 넘기고 loomd는 자기 Host만 허용(`replay/mod.rs allowed`) | 지시대로 `LOOM_DAEMON` 프록시로는 연결 불가 | 시험은 DEV 훅으로 직접 연결. **리드: `vite.config.ts` `/rpc`·`/api` 프록시에 `changeOrigin: true`**(그러면 Origin은 그대로라 `--allow-origin http://localhost:7420`로 통과). 또는 CORE가 루프백 Host 허용 |
| C-2 | snapshot_part `n` 1부터 | net은 0부터 가정 → 서버 재-snapshot 감지 불가였음 | **net·mock 고침**(계약 쪽) |
| C-3 | replay는 우선(제어) kind를 **제어 경로로만** 보내고 데이터 구독엔 안 보냄. mock은 둘 다 | 쓰기 행(이제 모든 단계가 제어)·gap·activity가 데이터 구독 미러에 없음 → 미러에 기대는 패널(예: CAN Trace TX 행 생성)이 실데몬에서 다르게 동작할 수 있음. 사용자 쓰기는 V5 때문에 replay에서 브라우저 시험 불가라 아직 DOM 증거 없음 | **CORE·리드에 질의**: 계약 01 §1.5에 "데이터 구독에도 실리는가" 명시 요청. 결정 전까지 mock은 그대로(둘 다) 둠 — 근거: 계약 문장 없음, net은 두 경우 모두 처리(dedup) |
| C-4 | gap 이벤트의 `domain` = kind 접두(`display`/`record`/`source`), channel `ch0` | CAN Trace(domain `can` 구독)에 replay gap이 **안 보임**(T05 예상 실패) | CORE: gap은 **영향받은 스트림**의 domain/channel로(UI·mock·eth 패널 가정) 요청 |
| C-5 | `core.stop`이 `Rights::Operate` 요구 | watch 탭 STOP 거부(계약 v1.1은 모든 자격) | CORE 수정 요청(T06 예상 실패) |
| C-6 | `core.activity` payload: schema/v1 `{client_id, method, summary, range{from,to}, waiting?}` ↔ 셸 어댑터 `{actor, verb, domain, channel, range{seq_a,seq_b}}` ↔ CAN 답 `action` | 셋이 다름 | mock은 상위 집합을 보냄. 셸 어댑터 정리(ui-opus) + CAN 답과 schema 일치(CORE·CAN) |
| C-7 | `POST /api/v1/stop` 없음 | WS 끊김 중 STOP(T06)을 replay에서 못 함 | CORE P1 게이트웨이 몫으로 확인 요청 |
| C-8 | `core.safe_state.list` 결과가 **맨 배열**(스키마엔 목록 래퍼 정의 없음), 항목 `{description, blocked:bool, method}`; run 결과 `{items[{id, outcome, finally_blocked, reason, request_id}]}` | 셸 어댑터는 `{items}`·`label`·`kind`·`blocked:string`, run `{results[{ok}]}`를 읽음 → replay에서 STOP 패널 복구 목록이 비고 결과가 안 보임 | CORE: list 결과 래퍼 확정 + `kind`(recover/emergency_off) 필드 필요 여부. ui-opus: 어댑터를 schema 필드로. mock은 둘 다 줌 |
| C-9 | `snapshot:false`이고 `after_seq` 없는 구독에도 snapshot part를 보냄 | net은 라이브 중 n=1 part를 서버 재-snapshot으로 보고 resync 1회(무해하나 불필요) | CORE: `snapshot:false`면 생략 요청 |
| C-10 | `backlog`는 `max_events·max_bytes·max_age_ms` 셋 다 필수(하나라도 빠지면 −32602) | net은 backlog를 안 보내 영향 없음 | 계약에 기본값·선택 여부 명시 요청 |
| C-11 | 구독 상한 초과 시 `E_BUSY`(연결 8·전체 32). 패널을 빠르게 열고 닫으면 **응답 대기 중인 구독**까지 서버가 셈 | T10에서 마지막 패널이 `subscribe_failed:E_BUSY` → 5 s 뒤 재시도 | **net 고침**: 닫았지만 서버가 아직 쥐고 있는 슬롯(진행 중 subscribe·응답 전 unsubscribe)을 상한에 셈 + 해제한 구독의 늦은 프레임은 orphan이 아니라 버림. 이후 replay T10 통과 |
| C-12 | hello `capabilities`에 `mock`이 없어 셸이 Playwright 탭을 watch로 내림(V5, ui-opus) | replay에서 사용자 쓰기·echo-writes를 **브라우저로** 시험 불가 | 결정: 우회(webdriver 플래그 끄기)는 하지 않음(안전 기능). Node 쪽 echo-write 행 모양은 `contract.test.ts`로 확인. 리드·ui-opus: `fixture-replay` 기능을 mock처럼 예외로 볼지 결정 요청 |
| C-13 | UDS 이벤트 kind `uds.tx`/`uds.rx`/`uds.timeout`(UDS plan과 같음) ↔ mock·provisional `uds.request` | UDS 콘솔은 request_id로 묶어 둘 다 표시(T02 replay 통과) | UDS 팀 스키마 답(CAN처럼)이 오면 mock kind를 맞춤 |
| C-14 | replay에 `can.tx.send` 없음(`can.send`만) | CAN 패널이 아직 `can.tx.send` 호출 | ui-opus가 `can.send`로 전환 중(리드 공지) |

## 5. 리드·ui-opus에 요청 (패널 DOM·동작 — 내 범위 밖)

- **U-1 (결함)** `src/shell/shell.css`: `.vt-row[hidden]{display:none}` 한 줄. 지금은 VirtualTable이 숨긴 풀 행(옛 `data-*`·문구 그대로)이 화면에 남아 보인다 — eth 완전 모드 행 수 거짓, UDS 한 요청이 두 행처럼 보임. eth 2 시험이 이것으로 실패 중.
- **U-2** UART 터미널 상태줄 `[data-testid=uart-status]`에 마지막 송신의 `data-request-id`(+ `data-origin`) — UI-T02 "같은 request_id" 판정용(시험은 속성이 생기면 자동 비교).
- **U-3** STALLED(writeLocked) 중 CAN 송신 입력칸·버튼 `disabled`(지금은 활성이고 Enter 때 거절 문구). UDS 명령줄도 같은지 확인.
- **U-4** eth.packets: STOP 패널 등 **레이아웃 변화로 생긴 scroll 이벤트**를 사용자 스크롤백으로 보고 완전 모드로 바뀜. 사용자 입력(wheel/key/pointer) 뒤의 스크롤만 스크롤백으로.
- **U-5** fixture-replay(`capabilities:['fixture-replay']`)를 V5 자동화 강등 예외에 넣을지(C-12).

## 6. 리드가 넣을 스크립트·설정

1. `package.json` scripts:
   ```json
   "e2e:mock": "playwright test -c e2e/m1.config.ts --project=mock",
   "e2e:replay": "playwright test -c e2e/m1.config.ts --project=replay",
   "e2e:m1": "playwright test -c e2e/m1.config.ts",
   "fixtures:replay": "tsx tools/mock/capture-replay.ts"
   ```
   (`e2e:replay`는 첫 실행 때 `cargo run`이 loomd를 빌드한다 — 미리 `cargo build -p loom-gateway` 권장.)
2. 기본 `playwright.config.ts`에 `testIgnore: ['m1/**']`(그렇지 않으면 `pnpm e2e`가 m1 스위트를 7420/7421 mock으로 한 번 더 돈다 — 동작은 하지만 UART flood 60 s 포함).
3. `vite.config.ts` 프록시 `/rpc`·`/api`에 `changeOrigin: true`(C-1).
4. **api.ts 시그니처 변경(구현은 이미 net에 있음, ui-opus 요청 §4)**:
   ```ts
   // SubscriptionHandlers
   onBatch(batch: EventsBatch, timing?: DeliveryTiming): void;
   // NetClient
   setVisible(visible: boolean, extra?: PresenceExtra): void;
   ```
   `DeliveryTiming = { recv_ms: number; decode_ms: number }`(Worker onmessage 시각 = 병합된 서버 배치 중 가장 이른 것, `performance.timeOrigin + now` 기준 / 파싱 시간 합) — `src/net/client.ts`에서 export, `src/net/index.ts`가 재수출. `PresenceExtra = { channel?, rendered_through_seq?, oldest_pending_age_ms? }`(`src/net/wire.ts`) — net이 최근 값을 들고 있다가 재연결 직후 presence에 실어 보냄(셸의 두 번째 presence RPC 불필요). 지금은 1-인자 핸들러도 그대로 동작(두 번째 인자는 무시됨).
5. **L-3**: CSP·Permissions-Policy — Vite dev는 헤더가 없다. dev에서도 보려면 `vite.config.ts` `server.headers`에 `Content-Security-Policy`(connect-src 'self' ws://localhost:*)·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()` 추가하거나, loomd가 /ui/*를 서빙할 때 시험 대상으로 삼음.
6. `Cargo.lock`: main 병합 뒤 잠금 파일이 Cargo.toml과 어긋나 있어 `cargo build/run`이 매번 고쳐 쓴다(eth crate 의존성). 나는 실행 후 `git checkout -- Cargo.lock`으로 되돌렸다 — 갱신된 잠금 파일 커밋 필요(CORE/리드).
7. 의존성 추가 없음.

## 7. net 변경 (`ui/src/net/**`)

- `wire.ts`: snapshot part 1부터(`first = n === 1`), `presenceParams(visible, extra)`(스키마 필드만 복사·검증), `events` Inbound에 `recvMs/decodeMs`.
- `core.ts`: 텍스트 프레임마다 수신·파싱 시각 → mux → `DeliveryQueue`(병합 시 recv = 최소, decode = 합) → `client.ts`가 onBatch 두 번째 인자로. `setVisible(v, extra)` 최근 값 보관·재연결 시 재전송. **쿠키 없는 업그레이드 거절**: 핸드셰이크 전에 닫히면 같은 URL에 HTTP GET 한 번(`probeAuth`, 2 s, same-origin 쿠키, redirect manual) → 401/403이면 `closed{reason:'unauthorized'}` + 재연결 중단(셸이 `#/need-auth`로). 400 등(업그레이드 전용 경로)은 평소 backoff. 결정 근거: 브라우저 WebSocket은 업그레이드 HTTP 상태를 노출하지 않음, 계약에 인증 확인 엔드포인트 없음 → 같은 경로 HTTP 상태로 판정(loomd P1이 쿠키 검사를 업그레이드 추출 전에 하면 401이 보임). mock `auth_required`로 재현 가능.
- `mux.ts`: 서버가 아직 쥔 슬롯 계산(`draining`) + 해제한 구독의 늦은 프레임 무시(FIFO 64) — C-11.
- `index.ts`: DEV 훅 `devWsUrl`, 타입 재수출.
- 시험: `contract.test.ts`(replay 실프레임 6), core(타이밍·presence 재전송·DEV 훅·unauthorized), mux(1-based, 상한 버스트, 해제 구독 프레임), 2aa7e59 반영(모든 쓰기 단계가 제어 경로).

## 8. 남은 것 / 비범위

- T03 수치 판정은 고정 러너에서 `LOOM_PERF=1`로(여기선 논리 판정만, 단일 맥북·dev 빌드).
- T08 bootstrap 비밀 비노출(비TTY `loom ui` 출력·데몬 로그)은 CLI/loomd 쪽 시험(P1).
- T07 OSC 8 링크 비활성·clipboard addon 미로드는 ui-opus 단위 시험이 덮음(브라우저 시험 추가 안 함).
- replay 쪽 사용자 개입(T04)·eth·test·psu는 V5와 replay 도메인 부재로 실데몬(P1 `loomd` + sim worker)에서 다시.
- 작업 중 ui-opus의 동시 편집으로 Vite HMR 재로드가 한 번 UART flood 측정을 깨뜨렸다 → 관측기를 init script로 바꾸고 재로드가 있으면 시험이 그 사실을 실패 사유로 말하게 했다.
