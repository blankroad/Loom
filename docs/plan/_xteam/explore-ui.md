# EXPLORE → UI: 탐색 도메인 패널 명세 (초안 v0, 2026-11-10)

작성: explore-lead. 정본: `docs/plan/explore/plan.md` §0.4(v2.0), §5.2(DLT), §5.5 L(Saleae LA), §5.1(PSU). 형식은 `ui-panel-template.md`를 따릅니다. UI 팀 의견은 `## ui 의견`에 남겨 주세요.

탐색 도메인은 실장비 보유가 LA(Saleae)뿐입니다. M1에서 실제로 쓰는 패널은 `psu.gauge`(sim, 계약 검증용), `dlt.trace`, `la.capture` 세 개입니다.

## 패널: dlt.trace (+ 컨텍스트 트리)
- 목적: DLT 로그를 실시간으로 보고, 컨텍스트 로그 레벨 변경(에이전트·사용자)과 그 반영 여부를 확인·개입한다.
- 마일스톤: M1에서 트레이스 표와 컨텍스트 트리, 레벨 변경을 낸다. 이후 non-verbose 해석과 마커 검색을 더한다.
- 대응 기능: COVESA DLT Viewer.

### 데이터 (읽기)
- 구독 kind:
  - `dlt.log`: ecu, apid, ctid, level, ts_dev, args_text, mcnt
  - `dlt.ctrl`: request_id, service, state(A14), ack_ms
  - `dlt.context`: apid, ctid, requested_level, reported_level, pending
  - `source.gap{cause: mcnt_discontinuity | daemon_buffer_overflow | reconnect}`
- snapshot: 컨텍스트 트리 전체, 최근 2k 로그 행.
- 질의 RPC: `dlt.query{range, filter{apid, ctid, level, text}, limit ≤ 1000}`, `dlt.contexts`.
- 유입률: 정상 수백 msg/s, 최대 10k msg/s.
- 표시 규칙:
  - 열: 시각, ecu, apid, ctid, level 색, 텍스트.
  - 같은 `request_id`의 CTRL 행은 단계별로 점진 갱신한다: requested → sent → confirmed(ACK ms) → effective_verified | effect_unverified.
  - 컨텍스트 트리는 [요청][보고][대기] 3값을 보인다.
- 해석은 도메인 payload를 그대로 표시한다(UI가 DLT를 디코드하지 않음).

### 개입 (쓰기)
| 사용자 입력 | 호출 RPC | 위험 등급 | 확인/승인 방식 | 쓰기 상태 표시 |
|---|---|---|---|---|
| 컨텍스트 레벨 변경(트리 우클릭) | `dlt.level{ecu, apid, ctid, level}` | R1 | 없음 | CTRL 행 A14 단계 |
| 전체 레벨(0xF08) | `dlt.level{all}` | R2 | 뷰어 Y(사용자 본인 입력이므로 확인 1회) | 배너 |
| 마커 | `core.marker` | R0 | — | 타임라인 마커 |

### 실시간성·정직성
- 장치 수신 → 화면 p95 ≤ 150 ms.
- 레벨 반영은 libdlt 약 0.5 s 주기 때문에 최대 2 s 관측창을 둔다.
- 고부하 시 화면 행은 축약하고 `display.gap`을 단다. **CTRL 행과 gap 이벤트는 드롭 금지.**
- 배지: 연결·epoch, MCNT 불연속 `source.gap`, `record.health`.

### 수용 시험
- sim에서 에이전트 `loom dlt level …` → 150 ms 안에 CTRL 행 [agent]가 나타난다.
- 2 s 안에 `effective_verified`로 갱신된다.
- 사용자가 트리에서 레벨을 바꾸면 [user] 행이 생기고, 에이전트 다음 결과의 `others_since_last`에 잡힌다.

## 패널: la.capture (Saleae — 캡처형)
- 목적: LA 캡처의 진행 상태를 실시간으로 보고, 캡처 후 디지털 레인·디코드 표를 다른 도메인 송신 이벤트와 같은 시간축에 놓아 "보낸 것이 선에 나갔나"를 확인한다.
- 마일스톤:
  - M1: 상태 바, 결과 레인(전이 보존), 디코드 표, 정렬 라벨, 정지·취소·재디코드.
  - 이후: 아날로그 채널, 반복 캡처 비교.
- 대응 기능: Saleae Logic 2. 단 **캡처 중 파형은 없음**(API로 스트리밍 불가).

### 데이터 (읽기)
- 구독 kind:
  - `job.progress{state: ARMED | WAITING_TRIGGER | CAPTURING | STOPPING | PROCESSING | READY | FAILED, elapsed, total}`
  - `la.capture.done{cap, duration, rate, channels, transitions, end_to_ready_ms, raw_ref}`
  - `la.decode{cap, analyzer, rows[≤ 배치 64 KiB]}`
  - `la.align{cap, ref_seq, offset_ms, quality: host-approx}`
  - `source.gap{cause: capture_gap | backend_down}`
- snapshot: 진행 중 캡처 상태, 마지막 캡처 요약·디코드 최근 500행.
- 질의 RPC:
  - `la.lanes{cap, t0, t1, px}`: 전이 보존 요약을 돌려준다(LTTB 금지). 원본은 `raw_ref` HTTP GET.
  - `la.decode.query{cap, range, limit ≤ 1000}`.
- 유입률: 이벤트는 캡처당 수 개 + 디코드 배치. 원본 파형은 이벤트로 보내지 않는다(A5).
- 표시 규칙:
  - 캡처 중에는 진행 막대와 배지 "캡처 중 — 파형은 종료 후 표시"를 띄우고, 통합 타임라인에 캡처 창 범위를 음영으로 칠한다.
  - 캡처 후에는 채널 별칭 레인과 디코드 표(시각, 분석기, 값 hex·ASCII)를 보인다. 정렬된 `uart.tx`/`can.tx` 행과의 연결선과 오프셋 라벨을 단다.

### 개입 (쓰기)
| 사용자 입력 | 호출 RPC | 위험 등급 | 확인/승인 방식 | 쓰기 상태 표시 |
|---|---|---|---|---|
| 캡처 시작(설정 폼) | `la.capture{…}` | R1 | 없음 | job.progress |
| 정지 `s` (루핑이면 직전 버퍼 확보) | `la.stop{cap}` | R1 | 없음 | STOPPING → READY |
| 취소 `x` | `la.cancel{cap}` | R1 | 없음 | FAILED(cancelled) |
| 분석기 추가·baud 변경 재디코드 | `la.decode{cap, analyzer, settings}` | R0 | 없음 | 표 갱신 [user] |
| 트리거·채널 설정 변경 | `la.config{…}` | R1 | 최근 30 s 에이전트 변경과 충돌 시 안내 | — |

### 실시간성·정직성
- 상태 변화 → 화면 p95 ≤ 250 ms. 캡처 종료 → 레인·표 표시 ≤ 3 s(2 s·4ch·≤ 10 MS/s 기준, 미실측).
- 반복 캡처 창 사이 공백은 `capture_gap`으로 표시하고 이어 그리지 않는다.
- Logic 2 앱이 종료되면 STALLED 배지 + `backend_down`을 띄운다.
- 앱에서 사람이 직접 조작한 내용은 Loom이 볼 수 없다. 패널에 안내 문구를 고정한다.

### 수용 시험
- SimLa에서 에이전트 `la capture --no-wait` → 250 ms 안에 WAITING_TRIGGER가 표시된다.
- `uart send` → 종료 3 s 안에 레인·표가 나타나고, 정렬 라벨이 붙는다.
- 사용자 `s` → 에이전트 `la wait`가 부분 결과와 `others_since_last`를 받는다.

## 패널: psu.gauge (SimPsu — 계약 검증용, 실장비는 구매 시)
- 목적: 설정·실측·출력 상태를 보고 사용자가 즉시 비상 OFF 한다.
- 데이터:
  - 구독 kind: `psu.state{set_v, set_i, out, prot}`, `metrics[{voltage, current, power}]`(실측 주기·age), `source.gap{scpi_no_response}`
  - snapshot: 최신 상태, 최근 60 s 시계열
- 개입:
  - `!` 비상 OFF: `psu.out{off}`, 확인 없음, 인터록은 경고만(A3).
  - 출력 토글: `psu.out`(R3, 뷰어 승인).
  - 값 편집: `psu.set`(R2, 사용자 본인 입력).
- 실시간성: 수신 → 화면 p95 ≤ 100 ms. STOP 후 "STOP — 출력 유지 중" 배지.
- 수용: 에이전트 `psu set` → 100 ms 안에 [agent] 행이 나타난다. 사용자 `!` → 에이전트 `wait-for`가 exit 6.

## 기타 (M1 이후)
- `shell.console`: UART 팀 `uart.terminal` 패널을 재사용한다. 탭 = PTY, exec#N, journal. 상단에 host fingerprint·입력 소유자를 표시한다.
- `debug.watch`(프로브 구매 시): watch 표 + 플롯(실측 주기·coherent), 코어 상태 램프, RTT는 `uart.terminal` 재사용. `!` = resume 또는 reset(프로파일).
- 러너 run 패널: 별도 패널 없이 **테스트 실행 화면(`_xteam/ui-test.md`)을 run 모드로** 쓴다(판정 수정 대신 단계 결과만).

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` v2.0 §5.8).
- `psu.gauge`: **UI-M1b 필수**(TEST TC-1 PSU 단계·STOP 패널 "출력 유지 장치" 데이터원). gauge spec 패널 + 실측 60 s + `!` 비상 OFF(확인 없음, watch 자격). spec JSON을 주세요.
- `dlt.trace`: Trace 그리드·RowStore 재사용이라 비용이 작다 — **DLT 도메인이 준비되면 M1b**, 아니면 M2. CTRL 행은 `request_id` + `payload.write_state`로(스키마 필수 규칙).
- `la.capture`: 상태 배지·진행·디코드 표는 도메인 준비 시 M1b, **전이 보존 레인 렌더러는 M2**(uPlot 부적합 → canvas 직접). `la.lanes` 응답 형식(전이 보존 요약)을 확정해 주세요.
- `shell.console`은 셸 공통 TerminalView(UART 터미널과 같은 컴포넌트), run 패널은 테스트 실행 화면 run 모드 — 수용.
- **이름 정리 요청**(CORE kind 2토큰 규칙): `job.progress` → `core.job_progress`, `la.capture.done` → `la.capture_done`, `core.marker` → `core.mark`.
