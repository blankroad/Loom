# UDS v2.0 초안 — uds-opus: UI 패널·TUI 강등·M1·통합 테스트·스킬·CORE 요청

작성: uds-opus, 2026-11-10. 기준: 마스터 §2.3 C1–C8 > B > A > D, `uds/plan.md` v1.2. plan.md는 고치지 않는다. 1절은 `_xteam/uds-ui.md`로, 6절은 `_xteam/uds-core.md`로 그대로 옮길 수 있게 썼다.
표기: "02:" 줄 = 사용자가 무엇을, 언제 보고, 어떻게 개입하는가.

---

## 1. UDS 콘솔 패널 명세 (UI 팀 전달용 → `_xteam/uds-ui.md`)

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

---

## 2. TUI 강등 범위

- **남기는 것 (보조, `loom uds view`, Rust TUI 키트 — 프레임워크는 CORE/UI 결정, 예 ratatui)**: U1 축약 표, U2 상태 바, `:` 송신과 미리보기, `T/R/!`, `Ctrl-X` STOP, 승인 카드(R2 `Y`, R3 끝 2자 — TTY도 viewer role이므로 규칙 동일), 복구 송신 1키, `display.gap`/LAG/STALLED, compact/ascii.
- **빼는 것**: DID 플롯, DTC diff, 연결 대화상자, 폴링 job 폼, 기록 재생. 웹이나 CLI로 한다.
- **M1 수용에서 TUI는 빠진다.** TUI는 M2에 smoke 시험(트레이스 행·`:` 송신·STOP 1회)만 둔다. 패널 데이터 계약(1절)이 같으므로 TUI는 별도 데이터 경로를 만들지 않는다.

02: 터미널만 쓰는 사용자도 같은 스트림을 보고 승인·STOP·복구로 개입할 수 있다. 1차 품질 관문은 웹이다.

---

## 3. M1 걷는 뼈대 재정의 (Rust + 웹 UI)

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

---

## 4. 통합 테스트 지원 (C7)

**UDS가 제공할 대기·검증 명령** (모두 R0, 레이트 적용, 종료 코드는 D5·A6 그대로):

| 명령 | 의미 | 결과 |
|---|---|---|
| `loom uds check <pred> -t T` | 지금 1회 읽고 판정 | 0 일치 / 3 불일치(`E_NO_MATCH`) / 7 무응답 / 10 NRC |
| `loom uds wait-for <pred> -t T --timeout 30s [--every 500ms]` | 조건이 맞을 때까지 반복 읽기(폴링 레이트 안에서) | 0 / 3 `E_TIMEOUT` |
| `read-did F1A0 --expect 02` · `raw 22F1A1 --expect-nrc 31` · `raw <hex> --expect-resp 62F1A0??` | 읽기와 기대값을 한 명령으로 | 위와 같음 |

`<pred>` 문법(작게 유지, 확장은 `help --json` `checks`에 기록):
- `did:F1A0 == 0x02` · `did:F40D >= 10` · `did:0101.region == EU` · `did:F190 ~ "^WVW"`
- `dtc:none [mask=0x09]` · `dtc:present B1234-00 [bit=confirmed]` · `dtc:absent U0100-00`
- `session == extended` · `security == L1` · `link == connected`

모든 결과의 `data.evidence = {request_ids[], seq_range, kinds[], predicate, observed, expected, complete}`.

**증거로 인용 가능한 이벤트**:

| 이벤트 | 판정에 쓰는 필드 |
|---|---|
| `uds.rx` | `request_id, seq, req_hex, resp_hex, positive, nrc, decoded, state_after, timing` |
| `uds.verify` | `write_state=effective_verified \| effect_unverified, readback_request_id` |
| `uds.timeout` | `stage, verdict, may_have_executed` |
| `uds.state` | `session/security/link` 변화와 `epoch` |
| `uds.guard` | 거부·승인 판정 |
| `uds.poll_sample` | `metrics, quality` |
| `uds.check`(신규, `dir:null`) | 위 판정 레코드 자체 — 무엇을 근거로 판정했는지 남김 |

**부재 증명 규칙**: `dtc:none`/`absent`는 다음을 모두 만족하는 `19 02` 긍정 응답 하나가 있어야 성립한다. ① `complete:true`(잘림·gap 없음). ② 요청 마스크가 판정 비트를 포함. ③ 그 단계 마커 seq 이후. 아니면 `inconclusive`.

**세션 사전 일괄 승인**: `loom test start`가 UDS에 `uds.plan_summary{intents[]}`를 요청한다. 에이전트가 TC에서 뽑은 "의도 목록"(대상·서비스·DID/RID·값 범위)을 받아 단계별 `classify`, 필요 세션·보안, `default_blocked`/`immutable_deny` 여부, 값이 동적인지를 돌려준다. 승인은 바이트가 아니라 **패턴 범위**(예: `ecu1 2E F1A0 값∈DB limits, ≤ 5회, 세션 종료까지`)로 묶는다. R3는 정책 `allow_r3_batch`가 없으면 단계마다 승인한다.

**TC 예시 1 (UDS 단독)** — "BCM sim의 F1A0을 2로 바꾸면 되읽기 2, 소프트 리셋 뒤에도 2, 확인된 DTC 없음."

| 단계 | 에이전트 명령 | 인용 증거 |
|---|---|---|
| 1 | `loom test step 1 "쓰기"` → `loom uds write-did F1A0 --hex 02 -t ecu1` (일괄 승인 범위 안) | `uds.rx` 6E, `uds.verify` |
| 2 | `loom uds reset soft -t ecu1` → `loom uds wait-for "link == connected" --timeout 10s` | `uds.rx` 51 01, `uds.state` epoch+1 |
| 3 | `loom uds check "did:F1A0 == 0x02"` | `uds.check` → `uds.rx` seq |
| 4 | `loom uds check "dtc:none mask=0x08"` | `19 02 08` 응답, `complete:true` |
| 판정 | `loom test verdict pass --evidence uds:seq:812-840` | Loom이 구간 안의 술어를 다시 평가 |

**TC 예시 2 (UART + CAN + UDS)** — "UART 콘솔에서 `reboot` → 부팅 프롬프트 → CAN NM 메시지 재개 → UDS 연결 복구 후 확인된 DTC 없음."

| 단계 | 에이전트 명령 (명령 이름은 각 팀 v2 기준, 가칭) | 인용 증거 |
|---|---|---|
| 1 | `loom uart send dut "reboot\n"` | `uart` tx seq |
| 2 | `loom uart expect dut "login:" --timeout 90s` | `uart` rx 매치 seq |
| 3 | `loom can sig wait-for NM_BCM.Active == 1 --timeout 10s` | `can` 프레임 seq + 신호값 |
| 4 | `loom uds wait-for "link == connected" -t bcm --timeout 20s` (전송은 재연결해 epoch를 올리되 요청 재전송 없음) | `uds.state` |
| 5 | `loom uds check "dtc:none mask=0x09" -t bcm` | `uds.check` + `19 02` 응답 |
| 판정 | `loom test verdict pass --evidence uart:seq:a-b,can:seq:c-d,uds:seq:e-f` | 도메인별 검증기 |

TC 2는 UDS에 하나를 더 요구한다. ECU가 재부팅해 링크가 끊기면 **링크만 자동 재연결**(정책 `auto_reconnect = true`, 기본 bench on)하고 미완료 요청은 `outcome_unknown`으로 둔다(재전송 없음). 세션과 보안은 `assumed default/locked`로 초기화한다.

**TEST 팀에 요청**: ① 단계 마커 `test.step{step_id, seq_start}`와 증거의 "단계 구간 안" 규칙. ② 술어 문법을 도메인 소유 + `help --json` `checks`로 발견. ③ `inconclusive` 판정. ④ 보고서에 인용 트랜잭션 행(U1 형식)과 기록 구간 링크. ⑤ `loom-test` 스킬의 "의도 목록 → plan_summary → 일괄 승인" 절차.
**CORE에 요청**: 6절 C-6·C-7.

02: 사용자는 테스트 실행 화면(셸)에서 단계와 인용 증거 행을 실시간으로 보고, 일괄 승인 범위를 시작 때 확인하며, STOP·복구로 개입한다. 증거 없는 PASS는 Loom이 거부한다.

---

## 5. 스킬 개정 (C8)

**`skills/loom-uds/SKILL.md` 골격 (≤ 50줄)**
```markdown
---
name: loom-uds
description: ECU 진단(UDS over DoIP/CAN-TP) — DID 읽기/쓰기, DTC, 세션, 리셋, 루틴, 검증(check/wait-for).
  "ECU", "진단", "DTC", "DID", "UDS", "DoIP", "seed key"에 사용.
---
# loom-uds  (공통 규칙은 `loom` 스킬, 테스트 절차는 `loom-test` 스킬)
정본: `loom uds help --json` (명령·인자·등급·종료 코드·예시·checks). 모르면 먼저 조회.
모든 ECU 통신은 `loom uds …`로만. UDS 라이브러리를 직접 쓰거나 Loom UI를 브라우저 도구로 조작하지 말 것.
## 절차
1. `loom uds status` — 대상·세션(confidence)·lease·viewers. viewers 0이면 사용자에게 Loom UI를 열어 달라고 요청.
2. 대상이 없으면 사용자가 준 주소로만 `connect`.
3. 읽기 먼저 → 쓰기(첫 호출이 미리보기, exit 5면 동의를 받은 뒤 `confirm`) → 되읽기 또는 `check`.
4. 결과의 others_since_last.count > 0 이면 `trace --since <from_cursor>`로 확인.
5. 끝나면 비기본 세션을 `session default`로.
## UDS 분기
5 E_CONFIRM_REQUIRED → 동의 후 confirm / E_APPROVAL_REQUIRED·E_NO_APPROVER → UI 승인 요청 후 `approval wait`
6 E_LOCKED → 제어권 반환 요청 · E_QUARANTINE → 해제 조건 보고
7 E_NO_RESPONSE → write_state=outcome_unknown이면 재전송 금지
10 E_NRC → nrc_name·hint 보고, 같은 요청 반복 금지
## 금지
default_blocked·immutable_deny 시도, raw 우회, 정책·DB 등급 하향, 0x27 재시도, 동의 추정, 뷰어 키 대리 입력
```

**`loom uds help --json` 스키마** (Rust clap 정의 + Registry 위험표에서 생성 — 정본 하나):
```json
{"schema_version":1,"domain":"uds","build":"…",
 "commands":[{"name":"write-did","synopsis":"loom uds write-did <DID> (--hex H|--value V) -t T",
   "args":[{"name":"DID","type":"hex16","required":true},{"name":"--hex","type":"hexbytes"}],
   "risk":{"default":"R2","raise":[{"when":"db.protected|identity_did|no_db","to":"R3"}]},
   "approval":"r2_path","mutates":true,"waits":false,"since":"M1",
   "exit_codes":[0,2,4,5,6,7,10],"evidence_kinds":["uds.rx","uds.verify"],
   "examples":[{"cmd":"loom uds write-did F1A0 --hex 01 -t ecu1","expect":"exit 5 → confirm"}]}],
 "checks":[{"name":"did","grammar":"did:<DID>[.<field>] <op> <value>","ops":["==","!=",">=","<=","~"]},
           {"name":"dtc","grammar":"dtc:none|present <code>|absent <code> [mask=] [bit=]","absence_rule":"complete 19 02 after step"}],
 "exit_codes":{"10":{"code":"E_NRC","agent_action":"report; no repeat"}},
 "risk_table":"loom uds help risk --json"}
```

**`loom-test`와의 경계**: `loom-uds`는 UDS 동작을 안전하게 수행하는 방법과 술어 문법(help `checks`)을 맡는다. `loom-test`는 TC 해석, 의도 목록과 일괄 승인, 단계 마커, 증거 인용, 판정·보고, 실패 시 행동(STOP → 복구 송신은 사용자에게 요청)을 맡는다. UDS 스킬에는 TC 절차를 넣지 않는다.

02: 스킬 1번 단계가 UI가 열려 있는지(viewers)를 확인하고, 4번 단계가 사용자 개입을 반영하게 한다.

---

## 6. UDS가 CORE에 할 요청 (Rust 기준 → `_xteam/uds-core.md`)

| # | 요청 | 내용 |
|---|---|---|
| C-1 | 쓰기 파이프라인 훅 (A2) | `trait UdsHooks { fn classify(&Req, &TargetCtx) -> Classification{risk, reason, block_class}; fn approval_binding(&Req) -> Binding{digest_fields, display: serde_json::Value /* approval.display 스키마 */}; fn presend_check(&Req, &Approval) -> Result<(), Presend{E_QUARANTINE, E_LOCKED, STALE_VALUE, EPOCH_CHANGED}> }`. 동기·논블로킹, 호출당 ≤ 1 ms |
| C-2 | 승인 digest 필드 | 대상 물리 주소, `epoch`, 요청 바이트 해시, 화면 표시 현재값 해시 + `before_seq`, `lease_id`, 정책·DB 해시. 무효화 트리거(같은 DID 쓰기, 세션·보안 변화)를 도메인이 core에 알리는 `invalidate(approval_id, reason)` |
| C-3 | 잡 API (A1) | `start/update/renew/stop(job_id)`, plan(대상·DID·주기·기간·최대 레이트), STOP latch, `core.job.*` 이벤트, **사용자 정지 허용**(에이전트 job 포함), idle gating 콜백(TesterPresent가 인플라이트 중이면 건너뜀) |
| C-4 | safe_state·복구 | `register_safe_state(domain, action, builder)`, 공통 `active_overrides` 발행 형식, user 전용 `core.recover{target}`(인터록·deny 통과, `finally_blocked`) |
| C-5 | 이벤트·스트림 | D3 필드 + 행 갱신 규칙(`row_key=request_id`, kind 순서), `metrics[]`, 우선 제어 큐에 `uds.rx/timeout/guard`, 도메인 **snapshot provider** trait(UDS 절: 대상 상태·최근 100 txn·DID 마지막 값·대기 승인), viewer role 전용 `dry_run` 미리보기(감사 제외·레이트 제한), client 커서 저장(`LOOM_CLIENT`, TTL 24 h)과 봉투 `others_since_last`, 브라우저 지연 측정용 시간 동기 RPC |
| C-6 | 테스트 세션 증거 | `core.events.get{seq_range, domain, request_id, kinds}`(저널 조회), `test.step` 마커, 도메인 **`verify_evidence(pred, &[Event]) -> Match\|Mismatch\|Inconclusive`** 훅(술어를 Loom이 재평가), 증거 구간이 단계 마커 안인지 검사, `complete` 플래그 존중 |
| C-7 | 패턴 범위 승인 | 테스트 세션 일괄 승인 객체 = 도메인 패턴(대상·서비스·DID·값 범위·횟수·만료). 사용할 때마다 도메인 `classify`·`presend_check`로 범위 안인지 재확인 |
| C-8 | 프로세스·메모리 (C2) | UDS는 gateway crate(D1). **seed-key DLL은 worker 프로세스 안에서만**(32bit DLL은 32bit worker), 시간 상한 2 s. 상한: 대상 큐 32, 대상당 txn 링 10k 행(넘치면 오래된 것부터 버리고 저널에서 조회), 폴링 샘플 배치. 상한 초과 시 drop/block 정책을 core 큐 타입으로 표기 |
| C-9 | 역할·보안 | 웹 bootstrap URL은 TTY·데스크톱 셸에서만 발급(에이전트 자기 승인 방지), viewer role 요청은 origin=user로 고정, 요청 필드 origin 무시 |
| C-10 | 기록 상태 | `record.health` 구독과 `E_RECORDING_FAILED` 판정을 core가 하고, UDS는 agent·script R1+만 막는다(A12) |

02: C-1·C-5는 사용자가 에이전트와 같은 순간 같은 행을 보게 하는 배관이다. C-3·C-4·C-9는 사용자가 언제든 job을 멈추고 복구하며, 에이전트가 사용자를 사칭할 수 없게 한다.
