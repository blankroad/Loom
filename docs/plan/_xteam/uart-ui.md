# UART → UI 패널 명세 (v2.0)

- 변경: v1.2(2026-11-10) — 질문 13–15 추가, 그리기 대기 2 MiB로 통일(DR-V22).
- 변경: v1.1(2026-11-10) — §6 템플릿 보완(유입률·snapshot·드롭 금지·전송 형식·원격·수용 시험), RPC 이름 `core.time`·`core.metrics.report`, 증거 REF를 TEST 형식으로.
- 작성: uart-lead(uart-opus 초안 검토), 2026-11-10. 상태: **v1** — v0 요구 요약을 대체. UI 팀은 맨 끝에 `## ui 의견` 절로 답해 주면 된다. 바뀌면 이 목록 맨 위에 변경 줄을 추가한다.
- docs/plan/02-core-principle.md 준수: 에이전트가 UART에 보낸 바이트와 장치 응답이 **같은 순간** 패널에 보이고, 사용자는 패널에서 원시 키·Ctrl-C로 바로 개입한다. 에이전트만 보는 데이터 없음.
- 정본 상세는 `uart/plan.md` §5(v2.0). 이 파일은 UI 팀이 구현에 바로 쓸 계약 요약이다. 관련 CORE 요청: `_xteam/uart-core.md`.
- 이벤트 payload 인코딩: 원시 바이트는 CORE·UI가 `_xteam/core-ui.md`에서 정하는 바이너리 프레임을 우선 쓴다(UART 요청 `uart-core.md` #2). JSON 경로에서는 `{encoding: base64|hex|raw_ref, data}`.

### 1. 패널 목록
| 패널 id | 내용 | M | 비고 |
|---|---|---|---|
| `uart.terminal` | xterm.js 터미널(장치 화면), 툴바, 상태줄 | M1 | 채널당 1개 기본, 같은 탭에 여러 개 가능(입력은 포커스 패널만) |
| `uart.trace` | 출처 행 표(가상 스크롤), 쓰기 상태 점진 갱신, hex(M2), 필터·검색(M2) | M1 최소 | `uart.terminal`의 탭으로 둬도 됨 |
| `uart.channels` | 셸 장치 목록에 넣을 UART 채널 행(상태·env·입력권·최근 에이전트 동작·viewer 수) | M1 | 새 채널 자동 표시 트리거 |
| `uart.approval_card` | 공통 승인 카드의 UART 본문 렌더러(plan §5.12) | M1/M2 | 셸 대기열 안 |
| `uart.test_evidence` | 테스트 화면에서 UART 증거 ref → 해당 줄 강조·이동 | M2 | TEST 화면 위젯 |

### 2. 데이터 계약
**2.1 구독 (`core.subscribe`)**
```
{filter:{domain:"uart", channel:"dut"}, backlog:{lines:2000, from_line_boundary:true}, max_batch_ms:10}
→ snapshot_part n/N … snapshot_end{stream_seq:H} → core.events(seq > H)
```
| 이벤트 kind | 패널 용도 | 우선 제어 큐 |
|---|---|---|
| `uart.data` (dir rx/tx, origin, client_id, request_id, payload base64/hex/raw_ref) | 터미널 렌더(rx만 — tx는 장치 에코로 돌아옴), 트레이스 행, 여백 표식(tx origin=agent/script) | 아니오(드롭 시 `display.gap`) |
| `uart.write` (같은 request_id의 상태 갱신) | 트레이스 결과 열, 상태줄 `A▶` | 예 |
| `uart.state`, `uart.config` | 머리줄 상태·gen·baud, 세대 배너 | 예 |
| `uart.input_owner`, `uart.hold`, `uart.lock` | 입력권·HOLD·락 표시 | 예 |
| `uart.agent_activity` | `⏳ expect …`, `⏳ 에이전트 대기 …` | 예 |
| `uart.transfer` | 대용량 전송 진행률·취소 | 예 |
| `uart.signal`, `uart.mark` | 트레이스 행, 타임라인 | 예 |
| `uart.expect_result` | 트레이스 행, 테스트 증거 | 예 |
| `uart.input_ack` (input_seq, sent/held/discarded, reason) | 키 전송 지연·보류 토스트 | 예 |
| `uart.input_suspect` | 감사 표시(자동 응답 의심) | 예 |
| `display.gap`, `record.gap`, `record.health`, `source.gap` | gap 3층·REC 배지, gap 줄 | 예 |
| 공통 승인·STOP·safety 이벤트 | 승인 배지, STOP 배너 | 예 |

**2.2 질의 RPC (R0)**
| RPC | 용도 | 비고 |
|---|---|---|
| `uart.status{channel}` | 패널 초기 머리줄·입력권·REC·viewer 수 | |
| `uart.lines{channel, from, until, limit≤2000, dir?, origin?}` | 트레이스 과거 행(서버 줄 조립 = 에이전트 `read`와 같은 경계) | 큰 범위는 같은 오리진 HTTP GET/`raw_ref`(A5) |
| `uart.grep{channel, pattern, from, until, max_matches}` | 전체 기록 검색 | M2 |
| `uart.ports{}` | 장치 목록(사용자가 직접 열기 — M2 `open` 버튼) | |
| `uart.help{}` | 패널 도움말·버튼 가용 여부(`help --json`과 같은 원본) | |
| `core.time{}` | 시계 맞춤 | CORE |

**2.3 쓰기 RPC (viewer 연결만 — origin=user, 서버가 자격으로 결정)**
| RPC | 쓰기 클래스 | 버튼/키 | M |
|---|---|---|---|
| `uart.input{channel, data_hex, input_seq}` | ③ 대화형 | 키·붙여넣기·^C·제어키 팔레트 | M1 |
| `uart.input.yield` / `.clear` / `.takeover` | 중재 | ↩ / ⌫ / [입력 가져오기] | M1 |
| `uart.hold` / `uart.unhold` | 중재 | ⏸ HOLD | M1 |
| `uart.cancel{request_id}` | 중재 | 전송/대기 취소 `✕` | M2 |
| `uart.signal{channel, kind, …}` | ① 요청형(클릭 = R2 확인, R3은 셸 확인 모달) | ⚡ 메뉴 | M2 |
| `uart.mark{channel, label}` | R0 | ⌖ | M2 |
| `uart.release` / `uart.reclaim` | ① R2 | ⚡ 메뉴 | M2 |
| `core.viewer.presence{channel, visible}` | — | 탭 가시성 변경 시 | M1 (CORE) |
| `core.metrics.report{…}` | — | 10 s마다 | M1 (CORE) |
- 승인·STOP RPC는 셸 공통(UART 없음).

### 3. 렌더·보안 요구
1. **스트림은 원본, 정제는 패널에서**: `term.write` 직전에 `loom-console` 렌더 정제기(WASM 권장, 대안 TS + 골든 코퍼스 동치 시험). 차단 목록은 plan §5.4 표.
2. **자동 응답 3중 방어**(plan §5.5): 정제기 질의 제거 + `parser.register*Handler` 삼킴 + 사용자 의도 게이트(키/입력/붙여넣기/조합 이벤트 밖의 `onData` 폐기·카운트). `windowOptions` 기본(전부 false) 유지, clipboard·image 애드온 로드 금지, `linkHandler`는 M1에서 OSC 8을 정제기가 지우므로 불필요.
3. 읽기 전용 = `disableStdin: true`.
4. 장치 바이트 사이에 로컬 텍스트 삽입 금지 — 구분선·세대 배너는 장식/오버레이, gap 줄만 리셋 지점에서 허용.
5. Ctrl-C = 항상 0x03(복사는 Ctrl-Shift-C/Cmd-C). 붙여넣기 4 KiB 초과·여러 줄(장치 bracketed paste 꺼짐) 확인 모달.
6. 접두키 `Ctrl-]`(설정 가능) + 모든 동작 버튼화 + 제어키 팔레트 + 전체화면 Keyboard Lock. `Ctrl-] x`는 셸 STOP 동작 호출.
7. 승인 요청이 와도 터미널 포커스·키를 가로채지 않는다(카드 버튼 0.7 s 무장 지연).
8. 패널 API에 장치 열기 능력 없음(구독·질의·쓰기 RPC만).

### 4. 성능 요구 (보이는 탭 기준)
| 지표 | 목표 | 측정 |
|---|---|---|
| 장치 RX → xterm 그리기 완료 | p95 < 100 ms, p99 < 200 ms 보고 | `ts.mono_ns` vs write cb 뒤 첫 rAF(시계 맞춤) |
| 패널 렌더 예산 | 수신 → `write` ≤ 1 프레임, parser ≤ 5 ms/프레임(≤ 64 KiB 묶음) | 브라우저 하네스 |
| keydown → TX `sent` | p95 < 20 ms(비부하), < 100 ms(921600 폭주) | `event.timeStamp` vs worker mono |
| 패널 열기 → 첫 화면(백로그 2000줄) | p95 < 500 ms | |
| 입력권·HOLD·STOP 표시 | 이벤트 → 모든 탭 ≤ 100 ms | |
| 921600 연속(≈ 92 KB/s) | 정상 부하 view skip 0, 넘치면 `display.gap` 정확, heartbeat 1 Hz, 메인 스레드 멈춤 없음 | `flood` 60 s |
| 메모리(C2) | xterm scrollback 10 000줄(상한 50 000), 트레이스 링 20 000행, 수신·그리기 대기 2 MiB — 탭 힙 증가 상한은 UI 팀 예산에 따름. 24 h soak에 UI 탭 1개 포함 요청 | 힙 스냅샷 |

### 5. UI 팀에 묻는 질문
1. 렌더 정제기를 **WASM**(Rust `loom-console` 공유)으로 받을 수 있나? 번들 크기·빌드 파이프라인 제약은? 거절이면 TS 포팅 + 골든 코퍼스로 간다.
2. xterm.js 버전·렌더러(WebGL/DOM)·애드온 목록 확정 시 공유 부탁 — 장식(decoration)/marker API를 안정 API로 쓸 수 있는지(`allowProposedApi` 필요 여부).
3. **사용자 의도 게이트**를 패널마다 구현할지, 셸의 공통 터미널 컴포넌트(SSH·DLT 셸 등도 재사용)에 넣을지. 우리는 공통 컴포넌트를 권장.
4. 셸 M1에 **승인 대기열**이 들어가나? 들어가면 UART M1에서 R3/R4 승인 경로를 포함한다(DR-V10).
5. 터미널 포커스 중 셸 전역 단축키(STOP 등)는 어떻게 받나? `Ctrl-] x`를 셸 STOP 동작으로 연결해도 되나?
6. 전체화면 + Keyboard Lock 패널 모드, 데스크톱 셸(Tauri) 일정은?
7. 여러 탭의 viewer 표시 이름(`탭 n`)과 원격 viewer의 사용자 이름은 셸이 주나, CORE가 주나?
8. 원격 viewer 기본 읽기 전용(입력 활성화 토글)에 동의하나?
9. 사용자 R3 시그널(부트로더 진입)용 **사용자 본인 확인 모달**을 공통 컴포넌트로 줄 수 있나?
10. 테스트 실행 화면의 증거 ref(`uart:dut@seq:a..b`(TEST REF)) 클릭 → UART 패널 해당 위치 이동 — 패널 간 "위치로 이동" 공통 API가 있나?
11. 브라우저 E2E 도구(Playwright 등)와 CI 3 OS에서 헤드리스 브라우저 실행 방식 — UART M1 (a)(b)(c)를 같은 하네스로 돌리고 싶다.
12. 에이전트가 브라우저 자동화로 UI를 열 때 viewer 자격이 발급되나? UART `no_viewer_attached`·승인 판단에 영향(우리는 viewer 수를 사람 존재 증명으로 쓰지 않음).
13. **새 채널 패널 자동 추가 + 모든 탭 토스트**(끄더라도 장치 목록에는 남김, plan §3.12) — 숨은 채널 방지 요구다.
14. `no_viewer_attached`의 `ui_url` 딥링크(`/#/uart/<ch>`)와 `loom ui open` 패널 표기(`uart.terminal --channel dut --at seq:N`?)를 정해 달라. viewer 자격이 없는 브라우저에서 딥링크를 열면 bootstrap 안내로 보내는가? (에이전트는 bootstrap URL을 받지 않는다.)
15. 구독 상한(채널당 4·전체 16, DR-V21)을 넘는 탭은 거절 대신 "읽기 전용 미러"로 보여 줄지 제안해 달라.

### 6. 템플릿(`ui-panel-template.md`) 보완 항목
- **예상 유입률**(채널당, 10 ms 배치 후): 115200 ≈ 11.5 KB/s → 정상 수십 ev/s / 921600 ≈ 92 KB/s → ~100 ev/s / 3 Mbaud(실측 후 지원) ≈ 300 KB/s → ~100 ev/s(배치당 ≤ 16 KiB). 8채널 동시가 상한.
- **늦은 합류 snapshot**(UART `SnapshotProvider`): 줄 경계 백로그 2000줄(UTF-8·VT 상태 불명 구간 표시), 채널 상태·세대·config, 입력권·`input_dirty`·hold·락, 진행 중 에이전트 동작(expect·대기 send), REC·gap 카운터 — `core-ui.md` U2 수용안과 같음.
- **절대 드롭 금지**: 2.1 표의 "우선 제어 큐 = 예" 이벤트 전부. `uart.data`만 드롭 가능(드롭 시 `display.gap`).
- **전송 형식**: M1은 JSON 이벤트(payload base64), M2 전 `uart.data` 구독만 바이너리 opt-in(CORE `core-ui.md` 제안 수용). 지연 예산(plan §2.0②)은 JSON 기준으로 이미 계산.
- **원격 보기**: CORE 제안대로 SSH 터널 기본(`loomd`는 127.0.0.1만). 원격 viewer 기본 읽기 전용 + `입력 활성화` 토글(§5 질문 8).
- **수용 시험(UI 자동 시험)** — UART M1 수용 기준과 1:1:
  | UART M1 | UI 자동 시험 |
  |---|---|
  | (a) | sim `bootshell`에서 에이전트 `loom uart send` → 패널 터미널에 같은 바이트, 패널 Ctrl-C → 에이전트 `others_since_last{origins:[user]}` |
  | (b) | 115200·921600 장치 RX → write cb 뒤 첫 rAF p95 < 100 ms(`core.time` 시계 맞춤) |
  | (c) | `flood` 60 s: heartbeat 1 Hz, `LIVE-DEGRADED`·gap 숫자 정확, 탭 메모리 상한 작동, Ctrl-C p95 < 100 ms |
  | (d) | 늦은 열기·새로 고침·두 번째 탭 seq 누락·중복 0, 두 탭 버퍼 동일 |
  | (f2) | `ansi` 질의 시퀀스 후 TX 저널 user origin 바이트 0, 두 탭 동시 타이핑 → 보류 + [입력 가져오기] |
  | (h) | 패널 API 표면에 장치 open 경로 0 |

### 7. CORE에 함께 요청 (참고)
`core.time`, `core.viewer.presence`, `core.metrics.report`, viewer 입력 RPC 우선 경로(수신 루프가 팬아웃에 막히지 않음), 도메인 `verify_evidence` 훅, exit 3 = 조건 대기 미충족 확장, 테스트 세션 사전 승인의 도메인 바인딩 형식.

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` v2.0 기준).
- 패널 id 5개 수용(`uart.terminal`·`uart.trace` M1, `uart.channels`는 셸 `channel_bar` 슬롯, `approval_card`·`test_evidence`는 셸 슬롯 렌더러).
- 이벤트 kind·RPC 이름은 UART 목록을 그대로 쓴다(`uart.data`, `uart.write`, `uart.input{channel, data_hex, input_seq}`, `uart.input.yield/clear/takeover`, `uart.hold/unhold` …).
- 지연·메모리 수치 수용: RX→그리기 p95 < 100 ms, keydown→`sent` p95 < 20 ms(폭주 중 < 100), 패널 열기 → 첫 화면(2,000줄) < 500 ms, scrollback 10,000줄(상한 50,000), 트레이스 링 20,000행, 수신·그리기 대기 2 MiB/채널(DR-V22).
- Ctrl-C = 항상 0x03(복사는 Ctrl-Shift-C/Cmd-C) 수용. 3중 방어 수용 — **사용자 의도 게이트는 셸 공통 TerminalView**에 두어 SSH·RTT 콘솔이 재사용. 추가: xterm 6.0.0 core는 **OSC 4/10/11/12 색 질의에 보고 문자열을 만든다** → parser hook 차단 목록에 추가. OSC 52는 core에 없고 clipboard addon을 로드하지 않는다.
- gap 장식: `registerDecoration`은 xterm 제안 API(`allowProposedApi`)라 버전 고정 + 실패 시 파서 리셋 지점 gap 줄 삽입(당신 명세의 대안)으로 간다.
- 키보드: 제어키 팔레트·모든 동작 버튼화 M1, Keyboard Lock 전체화면 모드 M2.
- 증거 REF는 TEST 형식(`uart:dut@seq:a..b`)으로 통일 확인.
- **요청**: 다중 viewer writer 선점 규칙을 서버(InputArbiter) 계약 하나로 확정해 주세요 — UI는 서버 상태(`input_owner`·`input_dirty`·hold)를 정본으로 표시만 한다. `loom-console` 정제기의 출력 계약(청크 경계 보존, gap 뒤 SGR 리셋 시점)도 함께.
