# UDS → UI 패널 명세 (v2.0)

docs/plan/02-core-principle.md 준수.

- 작성: uds-lead, 2026-11-10. 상태 **v0.1**: uds-opus 초안(`uds/v2-opus.md` §1·§3)을 옮겼고, 정본은 UDS `plan.md` v2.0 §5. 바뀌면 이 파일 상단에 변경 줄을 추가한다. UI 팀은 `## ui 의견` 절로 답해 주면 된다.
- 형식: `ui-panel-template.md`의 항목(데이터·개입·실시간성·수용 시험)을 아래 표와 M1 시험에 나눠 담았다. 패널 이름은 `uds.console`(U1–U4, M1) + `uds.values`(U5) · `uds.dtc`(U6) · `uds.targets`(U7) · `uds.recover`(U8) · `uds.jobs`(U9).
- 변경(2026-11-10, v0.2): 재연결 후 세션·보안은 stale 표시(U2), 트랜잭션 표시 링은 대상당 10k 행 또는 256 KiB·전역 8 MiB.
- 원칙(02): 에이전트 요청은 발행 순간(≤ 50 ms) 행으로 보이고, 같은 행이 응답과 함께(p95 ≤ 100 ms) 확정된다. 사용자는 이 패널들과 셸 STOP·승인 대기열로 언제든 개입한다.
- 예상 유입률: 정상 ≤ 50 ev/s(대상당 레이트 5 req/s × 이벤트 4–6개), 최대 ≈ 2k ev/s(sim 레이트 면제 측정, 다중 대상 폴링). 절대 드롭 금지: `uds.rx` 최종·`uds.timeout`·`uds.guard`·승인·`uds.state`(우선 제어 큐).

**공통 계약.** 모든 패널은 core `core.subscribe{filter: domain="uds"}` 하나를 구독한다(snapshot + `after_seq` 이음매, A5). UI 연결은 viewer role이므로 패널에서 나간 요청은 **origin=user**이며 에이전트와 같은 쓰기 파이프라인(classify → policy → 승인 → audit → presend_check → backend)을 지난다. 행 키는 `request_id`다. 같은 행을 `guard → queued → tx → pending×N → rx|timeout → verify`로 **갱신**하고, 행을 새로 추가하지 않는다(V-UDS-0). 셸이 소유하는 것: STOP, 승인 대기열, env 배지, LIVE/LAG/STALLED, `display.gap` 배너, 기록 탐색.

| ID | 패널 | 데이터 원천 (이벤트 kind·필드 / 질의 RPC) | 사용자 입력 → RPC |
|---|---|---|---|
| U1 | **트랜잭션 표** (V-UDS-1) | `uds.guard/queued/tx/pending/rx/verify/timeout/late/suppressed/aggregate` (plan §6.1 표). 열: 시각, origin(A/U/S/·), 대상, 경로, 서비스, 요약(`decoded`), 결과(✓ ✗NRC ⏳ ⌛ ⌛? ⛔ ⚠R2/R3 late ok_suppressed), `p2_ms`/`rtt_ms`, pending 수, `write_state`, 등급. 과거 구간: `uds.trace.query{since_cursor, filter, max_bytes}`, 기록 재생은 셸 | 행 선택 → 상세 서랍(hex, 디코드 트리, NRC 의미·다음 행동, `approval_ref`, 같은 request_id의 CAN ISO-TP 프레임 링크 V-UDS-5). 필터(대상·서비스·결과·origin), TesterPresent 접기 |
| U2 | **대상 상태 바** (V-UDS-4) | `uds.state` + snapshot: `link, epoch, backend(doip/cantp/canoe), session{value, confidence}, security{level, by_actor, since}, tp{on, ok_count, last_ok}, p2/p2*, lease{owner_origin, actor}, env, active_overrides[], queue{inflight, depth}`, `record.health` | `T` 제어권 가져오기 / `R` 반환(`uds.lease.takeover/release`), `!` 대상 abort(`uds.abort`), 대상 선택 |
| U3 | **요청 입력·미리보기** | 입력이 150 ms 멈추면 `uds.request{dry_run:true}`(viewer, 감사 없음, 레이트 제한) → `{bytes, service, did_name, risk, reason, needs{session, security}, current, diff}` | 입력 문법: `22 F190` / `read F190` / `write F1A0 02` / `session ext`. R0–R1은 Enter 한 번, R2+는 미리보기 확인 후 Enter 두 번(사용자 origin이라 R2 토큰 없음). R3는 셸 승인 카드로 감. 대상 미선택이면 송신 거부 |
| U4 | **승인 카드 내용** (셸 대기열에 넣는 UDS 필드) | `approval_binding` 훅이 `approval.display`(UDS JSON Schema)를 채움: `target{name, addr, env, backend}`, `session/security(+confidence, 해제 주체)`, `service`, `req_hex`, `decoded`, `diff{before, after, before_seq, before_age}`, `risk + reason`, `scope(once/session)`, `expires`, `requester{origin, actor}`, `stale_warning` | 셸 규칙(V-UDS-6): 포커스가 있을 때 `Y`/`N`, R3는 승인 코드 끝 2자 입력, 카드가 뜬 뒤 1 s 동안 입력 무시, 여러 건이면 목록에서 선택. 토큰 원문 미표시 |
| U5 | **DID 값/플롯** (V-UDS-2) | ① **모든** `uds.rx`의 `decoded` → "마지막 값 표"(DID·값·단위·시각·출처 origin). ② 폴링 job의 `uds.poll_sample{metrics[{name:"<tgt>.<DID>.<field>", value, unit}], quality}` → viz 시계열. 숫자가 아닌 값은 상태 표(변경 시각). `quality≠valid`는 선을 끊음 | 표의 행 → "지금 읽기"(R0), "플롯에 추가"(폴링 job 시작 폼) |
| U6 | **DTC 표** (V-UDS-3) | 가장 최근 `19 xx` `uds.rx.decoded.dtcs[{raw24, display?, status, bits{8}}]` + 요청 마스크·`complete` 플래그. 두 시점 diff(clear 전/후) | "다시 읽기"(R0, origin=user), "삭제"(R2, 미리보기 후 Enter 두 번), 행 → snapshot/ext 읽기(M2) |
| U7 | **대상 목록·연결 대화상자** | `uds.targets{}`, `uds.discover{…}` 결과(VIN/EID/LA/IP), 정책 등록 여부(`registered`, `allow_max`) | 연결 폼: sim 프로필 / DoIP IP·LA·SA / CAN 채널·ID·주소 방식 → `uds.connect`(R0). 미등록 대상은 첫 R1+ 때 등록 확인(plan §10.6) |
| U8 | **복구 송신** | `active_overrides[]`(비기본 세션, IO 0x2F 03, 0x28 disable, 0x85 off) + `uds.recover` 진행 단계(lease 확보 → PDU 종료 대기 → quarantine 확인 → 송신) | 단일 버튼/키 → `uds.recover`(user 전용, A15). STOP 상태에서도 safe_state만 허용 |
| U9 | **폴링 job** | core job 이벤트(`core.job.*`) + `uds.poll.stats{scheduled, sent, skipped, actual_period, nrc}` | 시작 폼(DID·주기·기간, 레이트 상한 표시), **에이전트 job도 정지 가능**(`uds.poll.stop`, 감사) |

**지연 목표** (정상 부하, 같은 호스트, plan §5.4를 웹 기준으로 상향):

| 경로 | 목표 |
|---|---|
| ECU 응답 수신(데몬 `ecu_rx`) → U1 행 확정 렌더 | **p95 ≤ 100 ms, p99 ≤ 200 ms** |
| `uds.tx` → U1 "진행 중" 행 | p95 ≤ 50 ms |
| 사용자 Enter/클릭 → `uds.tx` 발행 | p95 ≤ 50 ms |
| `poll_sample` → 플롯 점 | p95 ≤ 250 ms |
| U3 미리보기 (입력 정지 후) | p95 ≤ 150 ms |
| 늦은 합류: snapshot 100행 + 상태 렌더 | ≤ 1 s |

측정: 데몬 `ts.wall_ns`(같은 호스트) 대 브라우저 `performance.timeOrigin + now()`. 렌더 확정 시점에 `performance.mark('uds-row', request_id)`를 찍고, 시계 오프셋은 core-ui 시간 동기 RPC로 보정한다.

**고부하 표시** (plan §5.5 그대로): 폴링 그룹 1행(`×N`, skipped), 0x78은 같은 행 카운터(정체 시 빨강), TP는 상태 바 카운터, 0x36(sim)은 진행 행 1개. 집계는 `aggregate` 이벤트로 데몬이 만든다. UI가 임의로 합치지 않는다. 느린 구독자 드롭은 `display.gap{first_seq, last_seq, count, cause}` 배너로 알린다. 최종 결과·승인·STOP은 우선 제어 큐로 보내고 집계하지 않는다.
**늦은 합류**: snapshot의 UDS 절 = `targets[]`(U2 상태 전체), 최근 100개 파생 트랜잭션(`record_type: txn`), 대기 승인, 활성 job, DID 마지막 값. 이어서 `seq > watermark` 이벤트를 받는다. ECU 재연결은 없다.
**에이전트 연동**: 사용자 행동은 모두 에이전트 결과의 `others_since_last`로 전달된다. U1 상단에 "에이전트 claude-1 확인 위치 seq:431"을 표시해(데몬의 client 커서), 사용자가 자기 개입을 에이전트가 이미 봤는지 알 수 있게 한다. 뷰어가 0개면 에이전트 결과에 `no_viewer_attached` 경고가 붙는다.
**접근성·키보드 전용**: 모든 동작에 키를 둔다. 표 이동 `j/k`, 상세 `Enter`, 입력 `:`, `T/R/!`, 승인 `Y/N`, 셸 STOP `Ctrl-X`. TUI와 같은 키다. 색만으로 상태를 전하지 않는다(글리프 + 텍스트, 예 `✗ NRC 0x31`). `aria-live`로는 승인 요청·STOP·NRC만 읽고 일반 행은 읽지 않는다. 고대비 테마, 모션 줄이기. 이 키보드 경로는 M1 Playwright 시험에 포함한다.
**보안 주의 (UI·CORE)**: 웹 viewer 자격(D2 bootstrap → HttpOnly 쿠키)을 에이전트가 얻으면 브라우저 자동화로 스스로 승인할 수 있다. 그래서 bootstrap URL은 대화형 TTY나 데스크톱 셸에서만 발급하고, 스킬은 Loom UI에 브라우저 도구를 쓰는 것을 금지한다(D6 위협 모델 한계 안의 실수 방지).

02: 사용자는 에이전트 요청이 발행되는 순간(≤ 50 ms) 행을 보고, 응답과 함께(≤ 100 ms p95) 같은 행이 확정되는 것을 본다. U3·U4·U8·U9와 `T/R/!`·STOP으로 언제든 개입할 수 있다.

## M1 수용 시험 (UI 자동 시험 — Playwright, 실제 DOM)

**M1 범위**: Rust gateway 안의 `loom-uds` crate(자체 DoIP 전송, 트랜잭션 러너, classify/approval_binding/presend_check 훅, sim ECU), CLI `connect --sim/status/read-did/write-did/confirm/session/dtc list/raw/trace/abort/approval wait/help --json`, `wait-for`·`check`(4절) 최소형, 웹 패널 U1·U2·U3·U4(R2), U5 마지막 값 표, 기록·감사, 스킬 최소판.

**E2E-1 (웹)** — 사용자가 `loom ui open`(가칭, UI 팀)으로 브라우저 UI를 연 상태에서 시작한다.

| t | 에이전트 명령 | 사용자 화면 (U1·U2) | 자동화 확인 (Playwright, 실제 DOM) |
|---|---|---|---|
| t0 | `loom uds connect --sim bench --name ecu1` (데몬 자동 기동) | U2 `ecu1 ● sim DoIP 0x1001 sess=default? sec=locked lease free [SIM]` | 상태 바 텍스트, `data-env=sim` |
| t1 | `loom uds read-did F190 -t ecu1` | U1 행 `A ecu1 ReadDataByIdentifier F190 ⏳` → `✓ VIN="LOOMSIM…" 1 ms` | 행 `data-request-id`가 CLI JSON `request_id`와 같음. tx→행 ≤ 50 ms, rx→확정 ≤ 100 ms 표본 기록 |
| t2 | `write-did F1A0 --hex 01` → exit 5 → `confirm <tok>` (`r2_path=token_or_viewer` 회차) / 사용자 `Y` (뷰어 회차) / `auto` (sim 기본 회차) | U4 승인 카드(diff 00→01) → 행 `✓ R2 confirmed` → 되읽기 행 `effective_verified` | 세 경로 각각 통과. 카드에 토큰 원문 없음 |
| t3 | (사용자) U3에 `write F1A0 02` 입력 → 미리보기 → Enter 두 번 | 행 `U … F1A0 01→02 ✓`, 상태 바 `lease: user` | **키보드만으로** 입력. 에이전트 다음 결과 `others_since_last.count=1` |
| t4 | `loom uds session extended -t ecu1` | `uds.guard` 거부 행 `A … ⛔ E_LOCKED (lease: user)`(숨김 없음 불변식) | exit 6 `E_LOCKED`. 사용자 `R` 후 재시도 성공 |
| t5 | (사용자) `!` 대상 abort, 이어서 셸 STOP | 대기 큐 비움, "이미 전송됨 — 취소 불가", STOP 배지 | STOP 중 에이전트 쓰기는 거부 행 + 0이 아닌 종료 코드(정확한 코드는 CORE STOP 계약), 사용자 U3 송신은 허용 |
| t6 | 에이전트 CLI 종료 뒤 브라우저 새로고침 | snapshot 100행 + 상태 → 라이브 이어짐 | ≤ 1 s, ECU 재연결 이벤트 없음 |

**수용 기준 수치**: 응답→렌더 p95 ≤ 100 / p99 ≤ 200 ms(1,000회, Linux 전용 runner, sim 레이트 면제, Chromium 헤드리스 렌더 포함) · tx→행 p95 ≤ 50 ms · 사용자 입력→`uds.tx` p95 ≤ 50 ms · 늦은 합류 ≤ 1 s · 30분 연속 E2E 반복에서 데몬 RSS 증가 ≤ 10 MiB(C2 soak의 축소판, 24 h soak은 M2 공통 관문) · v1.2 안전 수용 (4)(5)(6) 유지(토큰, default_blocked/immutable_deny exit 4, 무응답 exit 7 + 늦은 응답 오매칭 없음, 기록 불능 시 agent 쓰기 exit 4와 사용자 송신 허용, 요청 필드 origin 무시) · CI 3 OS(지연 수치는 Linux만 판정). **헤드리스 구독자 수신이나 WS 메시지 확인만으로는 불합격** — DOM 행 텍스트와 `data-request-id`를 확인해야 한다.
**M1에 넣지 않는 것**: TUI, R3 승인, security/seed-key, CAN-TP(M1b), 폴링 플롯·job(U5 플롯·U9), DTC 패널(U6), 연결 대화상자(U7, CLI로 대체), 복구 송신(U8, M2), `canoe`(P4 선택), ODX, TLS, 플래시.

02: M1 수용 자체가 "사용자가 실제 브라우저에서 같은 행을 같은 순간 보고, 키보드로 개입한다"를 측정한다.

## UI 팀에 묻는 것
1. 행 갱신(`row_key=request_id`) 표를 셸 공통 컴포넌트로 줄 수 있는가(UART·CAN 트레이스와 공유)?
2. 승인 카드의 UDS 필드(`approval.display` JSON Schema)를 셸이 일반 렌더러로 그릴지, 도메인 렌더러를 받을지.
3. 렌더 확정 시각 보고(`performance.mark` + 시간 동기 RPC)를 공통 지연 하네스로 제공하는가.
4. 웹 viewer 자격 발급(bootstrap)을 대화형 TTY·데스크톱 셸에서만 하는 안(에이전트 자기 승인 방지)에 동의하는가.

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` v2.0 기준).
- 패널 구성(`uds.console` + values·dtc·targets·recover·jobs) 수용. **M1**: 트랜잭션 표·대상 바·명령줄(dry-run 미리보기)·[제어 가져오기]·[중단]·**R2 확인 카드**·마지막 값 표. DTC·세션·`active_overrides`·jobs·recover 패널은 M2.
- 질문 1: **예** — `request_id` 키 행 갱신 표(RowStore)를 셸 공통 컴포넌트로 제공(UART 트레이스·CAN TX·DLT CTRL·TEST 단계가 공유). 단계 칩(D4/A14)과 UDS 결과 기호(✓ ✗NRC ⏳ ⌛ ⌛? ⛔ late ok_suppressed)를 함께 표시.
- 질문 2: 기본은 **`approval.display` JSON Schema 일반 렌더러**, 필요하면 `registerApprovalRenderer('uds', …)`로 도메인 렌더러 선택 가능.
- 질문 3: **예** — 공통 지연 하네스 제공. 단 시계 비교는 wall이 아니라 `core.time`(d1/d2 mono) 오프셋 기준으로 바꿔 주세요(wall은 NTP 보정으로 튐). 보고는 `core.metrics.report{kind:"ui.render"}`.
- 질문 4: **동의(수정안)** — bootstrap은 TTY `loom ui`에서만(operate), 비TTY 경로는 가리키기·안내만, 원격은 TTY `loom ui pair`. R3/R4 승인은 M2부터 존재 확인(WebAuthn + 등록 게이트, 불가 호스트는 대역 외 코드). 데스크톱 셸은 M4 선택.
- 승인 입력 규칙(웹): 1 s 무장, 기본 포커스 거부, Enter·Esc는 승인하지 않음, 승인 = 버튼 또는 대문자 `Y`, R3는 요청 코드 끝 2자. 셸 STOP 키는 웹에서 `Ctrl-X` 대신 버튼 + `Ctrl-] x`(브라우저 잘라내기 충돌). 단일 글자 키(`j/k`, `T`, `R`, `!`, `Y/N`)는 패널 포커스·입력란 밖에서만.
- "에이전트 확인 위치 seq:N"은 CORE `core.agent_cursor` 이벤트로 요청함.
