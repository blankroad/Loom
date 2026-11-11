docs/plan/02-core-principle.md 준수.

# Loom UI 앱 — 통합 계획서 (plan.md v2.0)

작성: ui-lead, 2026-11-10 · 입력: `r1-opus.md`(구조·UX·안전), `r1-sol.md` + `spikes/ui/sol/RESULTS.md`(기술 조사·실측), `r1b-opus-xteam.md`(타팀 정합·쟁점), `r2-*-review.md`(교차 리뷰), `_xteam/core-ui.md`(CORE 의견 포함), `_xteam/{can,uart,uds,explore}-ui.md`, `_xteam/ui-test.md`, `test/plan.md`, 마스터 §2.3 C1–C8.
우선순위: 마스터 C > B > A > D. 이 문서와 도메인 패널 명세가 충돌하면 **화면 프레임워크·렌더링·셸 규칙은 이 문서**, **패널 내용(무엇을 보여 주고 무엇을 호출하나)은 도메인 명세**가 정본이다. 결정 근거는 §16 결정 기록.
표기: **확인**(문서·레지스트리·소스 확인) · **측정**(스파이크 실측) · **목표**(수용 기준) · **[미확인]**(시험 전).

---

## 0. 핵심 루프

UI는 장치를 직접 열지 않는다. UI의 책임은 **에이전트가 장치에 쓰고 읽는 바로 그 데이터를 같은 스트림·같은 순간에 사용자에게 보여 주고, 사용자 개입을 에이전트와 같은 게이트로 장치에 보내는 것**이다.

| 요소 | UI 쪽 구현 | UI가 보장하는 것 | 검증 |
|---|---|---|---|
| 1 직접 접근 | 없음 — 장치는 `loomd` worker만 연다(D1·C1). 브라우저는 `loomd`의 viewer 클라이언트 | 브라우저가 장치를 **열 수 없다**: 패널 API에 소켓·fetch 없음, `Permissions-Policy`로 WebSerial/WebUSB/WebHID 차단, CSP `connect-src 'self'` (§4.3 불변식 I-UI1) | UI-T07 |
| 2 읽기 | `core.subscribe`(원자적 snapshot + live) → WS 1개 다중화 → Web Worker 디코드 → 상한 있는 도메인 스토어 → 패널 | 에이전트가 읽는 스트림과 **같은 seq**를 본다. 에이전트의 읽기 행위(trace·read·sig get)도 Activity와 "에이전트가 읽은 범위" 표시로 보인다 | UI-T01·T02 |
| 3 쓰기 | 패널 `ctx.act()` → 셸 ActionGate(자격·실수 방지 확인) → 에이전트와 같은 도메인 RPC. `origin=user`는 서버가 연결 자격으로 부여(D2) | 사용자 쓰기도 같은 정책·감사·이벤트 경로를 지나고, 결과는 D4/A14 쓰기 단계로 **같은 행에서** 갱신 | UI-T04·T09 |
| 4 사용자 실시간 표시 | 출처 배지(AGT/USR/DEV/SCR + EXT), 쓰기 단계 칩, `LIVE/LAG/STALLED/LIVE-DEGRADED`, `display.gap` 생략 행, 브라우저 측 lag 계측 | 장치→화면 p95: **UART 100 ms · CAN 150 ms(p99 300) · UDS 100 ms(p99 200) · 플롯 250 ms**. 고부하에서도 멈춤·조용한 드롭 없음 | UI-T03·T05 |

**비분리 보장**: UI는 데몬 하나에 붙는 일반 viewer 클라이언트다. 에이전트 전용 숨은 연결·토픽은 없다. UI를 언제 열어도 snapshot + live로 이어진다(늦은 합류 첫 화면 p95 < 1 s, UART 패널 < 500 ms). 뷰어가 없으면 에이전트 결과에 `no_viewer_attached`, 보이는 뷰어가 없으면 `no_visible_viewer`(CORE 제안 이름 — UART 초안의 `viewer_not_visible`은 이 이름으로 통일 요청).

### 0.1 종단 간 시나리오 E-UI-1 (UART — M1 수용 시험 골격)

환경: `loomd` + `uart-sim`(pty 가짜 장치 `bootshell`), 브라우저 탭 1개(operate 자격), 에이전트 Claude Code. 시각은 데몬 mono 기준(브라우저 값은 §3.5 시계 정렬로 환산).

| 시각 | 누가 | 동작 | 이벤트 흐름 | 사용자 화면 (목표) |
|---|---|---|---|---|
| T0 | 사용자 | 터미널에서 `loom ui` → 브라우저가 `http://localhost:<port>/ui/` 를 엶 (에이전트가 1시간째 작업 중이어도 됨) | `core.subscribe{filter{domains:[uart], channels:[dut]}, snapshot:true, max_batch_ms:10}` → `snapshot_part…` → `snapshot_end{H}` → live `>H` | < 500 ms: 터미널에 최근 백로그(줄 경계부터) + `── 백로그 / 라이브 ──` 장식, 입력권 칩 `비어 있음`, `LIVE lag 9ms` |
| T1 | 에이전트 | `loom uart send dut 'uname -a' --expect aarch64 --timeout 5s` | 정책 R1(sim auto) → `uart.write{request_id:r-88}` `requested→accepted→sent` (**publish-before-reply**) | ≤ 40 ms: 상태줄 `AGT ▶ "uname -a⏎" ◑보냄`, Activity 행 |
| T1+3 ms | 장치(sim) | 에코 + `Linux dut 6.6.21 … aarch64` | `uart.data` rx 청크 → 적응형 배치(조용한 구간 뒤 즉시) → Worker 디코드 → xterm write | **p95 < 100 ms** 화면 반영. 에이전트 결과 `matched`, `others_since_last.count=0` |
| T2 | 사용자 | 터미널 클릭 후 `dmesg -w⏎` | 첫 키 → `uart.input`(쓰기 클래스 ③, A1) → InputArbiter `input_owner=user` | 칩 `입력권: 나 (입력 중)` 노랑. 키 → 장치 에코 p95 < 100 ms |
| T3 | 에이전트 | `loom uart send dut 'reboot'` | 사용자 입력권 중 → 에이전트 send `deferred` | 칩 `⏳ 에이전트 대기 "reboot⏎" 0.0/10s` + [에이전트에게 반환 `Ctrl-] y`] |
| T4 | 사용자 | `Ctrl-C` (터미널 포커스 → 0x03) | 0x03 송신 → 프롬프트 재출현 → 입력권 반환 → 대기 send `sent` | 칩 `비어 있음`, `AGT ▶ "reboot⏎"`, 사용자 행에 `⇢ 전달 대기` |
| T5 | 에이전트 | (T3 결과 수신) | 결과 `others_since_last{count:2, origins:[user]}` + `NOTE: user sent 2 inputs` → CORE `core.agent_cursor` 전진 | 사용자 입력 행 표식이 `⇢ 에이전트에게 전달됨 (21:05:16)`으로 — 사용자는 **에이전트가 내 개입을 알았다는 사실**을 본다 |
| T6 | 사용자 | 상단 **[■ STOP]** 또는 `Ctrl-] x` | STOP RPC → agent·script 쓰기·잡 latch(A3), 우선 제어 큐로 `core.latch` | 1 프레임 안에 "STOP 요청 중…" 띠 → 서버 확인 시 "STOP — 잠김" + STOP 상태 패널(복구 송신 1키). 사용자 키 입력은 계속 가능 |

변형(같은 M1 수용 시험에 포함):
- **UDS E-UI-2**: 에이전트 `loom uds read-did bcm F190` → UDS 콘솔의 한 행이 `○요청 → ◔수락 → ◑보냄 → ●확정 F190 VIN=WVW…`로 진행(응답→렌더 p95 ≤ 100 ms). 사용자는 명령줄 `22 F190⏎`(USR 행), 또는 [제어 가져오기]로 lease 회수 → 에이전트 다음 R1+ 요청 exit 6 + `others_since_last`에 `lease → user`.
- **CAN E-UI-3**: 에이전트 `loom can send ch0 0x300#01` → Trace에 `AGT tx 0x300` 행, sim BCM 응답 `DEV rx 0x301` (p95 ≤ 150 ms). 사용자는 Trace 송신줄 `0x300#00⏎`.

### 0.2 지연 예산 (브라우저 쪽 분해) — 목표

구간: `t_dev`(worker ingest `ts.mono_ns`) → `t_send`(gateway WS write) → `t_recv`(Worker onmessage) → `t_dec` → `t_apply`(스토어) → `t_draw`(렌더 제출 후 다음 rAF = 화면 반영 근사). **UI 책임 = `t_recv → t_draw`**, CORE 책임 = `t_dev → t_send`(배치 대기 포함). 홉별 p95 합이 종단 p95를 증명하지는 않으므로 종단 값은 따로 잰다(§9.4).

| 경로 | CORE ingest→hub | 배치 대기 | WS 전송 | 디코드+반영 | 렌더 대기·제출 | 종단 목표 p95 / p99 | **UI 구간 목표 p95** |
|---|---|---|---|---|---|---|---|
| UART 터미널 | ≤ 10 | ≤ 10 (`max_batch_ms:10`) | ≤ 2 | ≤ 7 | ≤ 33 | **100 / 200** | ≤ 45 ms |
| UDS 콘솔 (최종 단계 = 제어 경로) | ≤ 10 | 적응형(조용하면 즉시) | ≤ 2 | ≤ 5 | ≤ 17 | **100 / 200** | ≤ 35 ms |
| CAN Trace | ≤ 20 | ≤ 40 | ≤ 5 | ≤ 15 | ≤ 33 | **150 / 300** (CAN 팀 강화 목표, 200 상한) | ≤ 50 ms |
| 플롯(신호·DID·추출값) | ≤ 20 | ≤ 40 | ≤ 5 | ≤ 20 | ≤ 50 | **250** (UART 추출 플롯 200) | ≤ 100 ms |
| 셸 제어(STOP·승인·lease) | — | 즉시 | ≤ 2 | ≤ 2 | ≤ 17 | 모든 탭 ≤ 100 ms | ≤ 20 ms |

- 근거(**측정**, `spikes/ui/sol/RESULTS.md`): M1 맥북 headless Chromium, 10k 프레임/s 합성 CAN, 2M행 링, 생성→다음 rAF **p95 33 ms / p99 34.5 ms**(JSON·column·MessagePack, DOM·canvas 6조합 모두 비슷). UI 구간 50 ms 목표가 현실적임을 보인다. 단 합성 5필드 행·렌더 proxy·단일 머신이라 제품 합격 근거는 아니다.
- **렌더 정책**: 정상 부하에서는 rAF마다 그린다(최대 60 Hz). **20 Hz 상한은 프레임 예산 초과(LIVE-DEGRADED) 때만** 건다 — CAN 150 ms 목표를 20 Hz 대기(50 ms)로 깎아 먹지 않기 위해(R1b C-3).
- 우선 제어 이벤트(쓰기 최종 단계·승인·STOP·상태·오류·gap)는 데이터 배치를 기다리지 않는다(서버 우선 큐 + 브라우저 제어 경로).

### 0.3 고부하·메모리 불변식
- 정상 부하에서 화면 드롭 0이 합격 조건이다. "계속 버리고 gap만 정확하면 통과"는 불합격.
- 넘치면 숨기지 않는다: `display.gap`(화면 생략, 회색) · `record.gap`(기록 손실, 빨강) · `source.gap`(장치·전송 손실, 빨강)을 구분 표시(A13).
- 표는 보이는 행만 그린다. 보관 행 수 ≠ 그리는 행 수. 오래된 데이터는 디스크 저널에서 질의한다 — 브라우저에 24시간 원본을 쌓지 않는다(C2).
- 모든 브라우저 버퍼는 바이트 상한이 있다(§6.5). 넘치면 오래된 것부터 버리고 "보관 범위 밖 → 질의로 조회" 표식.

---

### 0.4 v2.2 정합 — 마스터 §2.4 E · §2.5 F1·F2 (2026-11-10, 구현 착수 시점, 본문보다 우선)
| 근거 | 바뀌는 것 | 본문 영향 |
|---|---|---|
| **F1 기본 모두 자동** | 기본 정책 = R0–R4 자동 실행. 확인 토큰·뷰어 승인·존재 확인은 **기본 꺼짐**. 승인 대기열·승인 카드·존재 확인은 **정책으로 켰을 때만 나타나는 기능**으로 유지(서버가 `core.approval_requested`를 보낼 때만 렌더). M1 수용 시험의 "R2 확인 카드"는 `loom.toml`로 confirm을 켠 정책 fixture에서 시험 | §5.1.5, §10.3, UI-T08, §11 M1·M2 |
| F1 유지 항목 | 모든 쓰기 행에 **위험 등급 배지**(R0–R4) 표시, 실시간 표시·감사, STOP·비상 OFF·복구 송신, env 배지, 인터록 결과 표시 — 그대로 | §5.2.3에 등급 배지 열 추가 |
| E9 + F1 | rights watch/operate/approve 유지. 존재 확인(WebAuthn·대역 외 코드)은 **R4만, 그리고 정책으로 승인을 켰을 때만**. 등록 게이트는 R4 승인 첫 사용 전 1회 | §3.7, §10.3 B·B'·C는 선택 기능 |
| V5 + E9 | Playwright·에이전트 자동화 브라우저 세션 = **watch만**(bootstrap 쿠키 발급 안 함) | §3.7, §10.3 A |
| V1 | 원격 보기를 **M2로 앞당김**(localhost + SSH 터널 또는 Tailscale, LAN 직접 노출은 선택·인증 필수) | §1.2, §11 M2 |
| E8 | R-1·R-4·R-5·R-6·R-7 채택(웹 STOP 키, 출처 색, TUI 폐기, `request_id`+`write_state` 필수, localhost 오리진) | 변경 없음(본문이 이미 이 안) |
| E2·E3 | 종료 코드 3 = 조건 판정 불충족, 11 = `E_TEST_FAILED`, 6 `E_SESSION_PAUSED` — 테스트 화면 문구에 반영 | §5.1.6 |
| E7 | publish-before-reply = 허브 seq 부여 + 모든 viewer 우선 큐 enqueue 후 응답(렌더 보장 아님) | §3.6 그대로 |
| **F2 이더넷 1차** | **`eth.packets` 패널(Wireshark식: 가상화 목록·상세 트리·hex·표시 필터 바·UDS 링크 칩·재전송/편집 송신·캡처 정지·시간 기준 3종)을 UI-M1에 추가**. 내용 명세 `docs/plan/eth/xteam-draft-eth-ui.md`(→ `_xteam/eth-ui.md`). Trace 그리드·RowStore 재사용, 트리↔hex 바이트 범위 동기는 신규 컴포넌트. 스트림 따라가기·통계는 M2 | §1.2, §5.8, §11 UI-M1, §12 |

## 1. 범위 / 비범위

### 1.1 소유 경계
| UI 팀 | 도메인 팀 (UART·CAN·UDS·EXPLORE) | CORE 팀 | TEST(EXPLORE) |
|---|---|---|---|
| SPA 셸·패널 프레임워크·수명주기·레이아웃 | 패널 **내용 명세**(`_xteam/<comp>-ui.md`), 해석(DBC·NRC·DID·정규식·DLT) | 장치 worker·이벤트 허브·seq·snapshot·저널 | TC·판정·보고서 내용, `shell.test` 내용 명세 |
| Trace 그리드·플롯·터미널 렌더러, `loom.viz`(CAN에서 이관, C6) | 도메인 질의 RPC·min/max 집계·snapshot provider | 인증·전송·스키마 생성 원천·정적 자산 서빙 | 세션·증거 검증(CORE와) |
| 공통 셸: STOP·승인 대기열·env·연결/gap·Activity·마커·기록 탐색·테스트 실행 화면 | 도메인 승인 카드 본문 데이터(`approval.display`) | 정책·승인·감사·STOP latch | |
| 브라우저 지연 계측·UI 수용 시험·UI 포함 soak | 패널 수용 시험 fixture | 24 h soak 하네스(UI 탭 포함) | |

### 1.2 범위 (마일스톤 배정은 §11)
| 항목 | M | 핵심 루프 기여 |
|---|---|---|
| 웹 SPA(정적 자산 `loomd` 내장, 오프라인), 인증 bootstrap·자격 분리 | M1 | 3·4 |
| 공통 셸: 상단 바(env·연결·LIVE·REC·보는 사람·에이전트 활동), 전역 STOP·STOP 상태 패널·복구 송신, Activity, 고정 레이아웃 | M1 | 3·4 |
| 우선 제어 경로(승인·STOP·오류·쓰기 최종 단계) | M1 | 4 |
| 패널 프레임워크 `registerPanel` + manifest 검증 + 불변식 lint | M1 | 전부 |
| 1차 패널: UART 터미널(xterm.js), CAN Trace(Chrono·raw·송신줄), UDS 콘솔(점진 갱신 표·명령줄·제어 가져오기·R2 확인 카드·마지막 값 표) | **M1** | 전부 |
| 승인 대기열 + **R2 확인 카드** | M1 | 3(안전) |
| `shell.test` 최소판(계획 검토 모달·일괄 승인·단계·실시간 증거·판정 수정·단계 띠) + `psu.gauge`(sim 상태·비상 OFF) | **M1b** (TEST M1 TC-1) | 3·4 |
| (선택) `dlt.trace`, `la.capture` 상태·진행·디코드 표 — 도메인이 준비되면 | M1b(수용 기준 밖) / 아니면 M2 | 4 |
| R3/R4 승인·존재 확인(WebAuthn/대역 외 코드), 도킹·레이아웃 저장, 가리키기 토스트, CAN Fixed·IG·Data·Graphics 기본, UDS DTC·세션·overrides·jobs, UART 트레이스·hex, binary 전송, DLT 패널(M1b에 못 들어간 경우), **LA 전이 보존 레인 렌더러** | M2 | 3·4 |
| `loom.viz` 범용 렌더(파일·라이브), spec 패널, 통합 타임라인, 기록 탐색, Graphics 고급(커서·state·분할), 테스트 화면 고도화·보고서 뷰 | M3 | 4 |

### 1.3 후순위 / 비범위
| 항목 | 판단 | 이유 |
|---|---|---|
| Tauri 2 데스크톱 셸 | M4 선택 | 웹만으로 루프 성립. 브라우저 예약키 불만이 실측되면 얇은 셸(같은 SPA·같은 WS) |
| Replay 블록·Statistics·PNG export·영어·테마 편집·패널 팝아웃 | M4–M5 | 루프 직접 기여 작음 |
| 제3자 동적 JS 패널·원격 CDN | 비범위 | 오프라인·CSP·검토·스키마 일치 |
| 10^7 원본 점을 브라우저에서 매번 렌더 | 비범위 | 서버 min/max 질의로 해결 |
| LAN 직접 공개·인터넷 노출·다중 OS 계정 보안 경계 | 비범위(M5 검토) | 원격은 SSH 포워딩. D6 위협 모델 밖 |
| UI가 BLF/MF4/DBC/ODX를 직접 파싱 | 비범위 | 도메인·CORE 질의 결과만 받는다 |
| TUI 신규 구현 | 비범위 | §5.10 |

---

## 2. 사용 시나리오

### 2.1 에이전트 관점
| # | 상황 | 에이전트 행동 | UI에 보이는 것 |
|---|---|---|---|
| A1 | 세션 시작, 뷰어 없음 | 결과 경고 `no_viewer_attached` → 사용자에게 "터미널에서 `loom ui` 실행" 안내(스킬 규칙). `loom ui open uart/terminal --channel dut`는 열린 탭이 있을 때 가리키기만 | 사용자가 연 탭에 같은 스트림 |
| A2 | 특정 구간을 보여 주고 싶음 | `loom ui open can/trace --channel ch0 --filter 'id in 0x300,0x301' --at seq:5012 --reason "0x301 무응답"` | 모든 탭에 "가리키기" 토스트, 사용자가 [열기]를 눌러야 화면 변경(기본) |
| A3 | R3 동작(주기 송신) 필요 | `loom can periodic start …` → exit 5 + `approval_id a17 code ··7K` → 채팅에 "UI에서 a17 승인 부탁" | 승인 대기열 `⚠ 승인 1`. 에이전트는 UI를 조작하지 않는다(스킬 규칙 + §10.3) |
| A4 | TC 실행 | `loom test start tc/boot.tc.md` → 단계마다 `loom test step/verdict --evidence uart:dut@seq:1201..1288` | 테스트 실행 화면: 현재 단계·증거 검증 3항목·판정 |
| A5 | 사용자가 개입한 뒤 | 결과 `others_since_last{count:3, origins:[user]}` → `--since @last`로 읽고 다음 행동 | 사용자 행에 `⇢ 에이전트에게 전달됨` |
| A6 | 데이터만으로 플롯 (M3) | `loom viz show live:can:ch0 --y EngRPM,VehicleSpeed` | Graphics 패널에 같은 라이브 신호, `downsampled N→M` |

### 2.2 사용자 관점
| # | 상황 | 사용자 행동 | 결과 |
|---|---|---|---|
| U1 | 에이전트가 UART로 부팅 로그를 보는 중 | 터미널에서 직접 `printenv⏎` | 입력권 → 나, 에이전트 send 대기, 에이전트에게 `others_since_last` |
| U2 | 에이전트의 CAN 주기 송신이 이상함 | [■ STOP] 또는 `Ctrl-] x` | 전 도메인 agent 쓰기 latch, STOP 패널에서 복구 송신 1키 |
| U3 | UDS RoutineControl이 0x78 pending으로 오래 걸림 | 행을 봄(카운터·경과/한도), 필요하면 [중단 !] | 같은 행 `pending ×12 3.1s/5.0s` 갱신, 중단 시 `⌛? 불확정`(실패로 칠하지 않음) |
| U4 | 실차(`env=vehicle`)에서 DID 쓰기 R3 승인 (M2) | 모달에서 내용·hex·diff·env 확인 → 코드 2자 + 존재 확인 | 승인 1회, 감사에 방법 기록 |
| U5 | 벤치 PC(Windows/Linux)의 화면을 macOS에서 | `ssh -L 7420:localhost:7420 bench` → 벤치 터미널에서 `loom ui pair` 코드 입력 | 원격 탭도 같은 스트림, 상단 `보는 중: 원격 1` |
| U6 | CAN 버스 100% | Trace에서 "Fixed로 전환" 배너 수락(M2) | `LIVE-DEGRADED`, 생략 행 `⋯ 3,812 프레임 화면 생략 — 기록 정상` |
| U7 | TC 실행 중 수동 단계 (M1b) | "계기판 로고 보임?" 질문에 [예], 단계 판정 수정(사유 필수) | 원판정·수정 판정 병기, 사용자 조작 구간에 "판정 제외" 표시 |

---

## 3. 아키텍처

### 3.1 결정 요약
1. **웹 SPA를 `loomd` 바이너리에 내장**(빌드 시 번들, 런타임 CDN·네트워크 0). 경로는 CORE 제안: 정적 `/ui/*`, RPC `WS /rpc`, 대용량 `GET /api/v1/query/<id>`·`/api/v1/raw/<ref>`·`/api/v1/report/<sess>.*`.
2. **오리진 정본 `http://localhost:<port>`**(바인딩은 127.0.0.1 유지). `127.0.0.1`로 들어오면 `localhost`로 리디렉트 — WebAuthn RP ID는 IP를 허용하지 않기 때문(**확인**: W3C WebAuthn L3, R1b §4.1).
3. **탭 1개 = WS 1개**(구독 id로 다중화). WS·디코드는 전용 Web Worker, 메인 스레드는 스토어 projection·렌더만.
4. **두 경로**: 제어 경로(승인·STOP·상태·오류·쓰기 최종 단계 — 즉시, 집계 금지) / 데이터 경로(프레임·바이트·트랜잭션 — 배치·예산·생략 표시).
5. **패널은 코어 데이터에만 접근**: `PanelContext` 하나만 받는다. 소켓·fetch·RPC 클라이언트 없음.
6. 프레임워크 **React 19 + 외부 스토어**(고빈도 데이터는 React 밖) — §7.2, 결정 기록 UD-01.
7. 데스크톱 셸(Tauri)은 M4 선택.

### 3.2 프로세스 구성도
```text
 벤치 PC (Windows x64 / Linux x64)                                         (선택) macOS
┌──────────────────────────────────────────────────────────────────────┐   ┌───────────────────┐
│ loomd (Rust)                                                          │   │ 브라우저 탭       │
│  ├ HTTP: /ui/* (내장 SPA) · /bootstrap · /pair · /api/v1/*  ┐127.0.0.1 │◄──┤ ssh -L 7420:...   │
│  ├ WS /rpc: JSON-RPC 2.0 (+ 데이터 binary 프레임, M2 전)    ┘ :7420   │   └───────────────────┘
│  ├ 이벤트 허브: seq · snapshot · 구독자별 우선 큐 · 데이터 credit    │
│  ├ 정책·승인·감사 · STOP latch · 쓰기 파이프라인                     │
│  └ worker 감독 ── uart-worker ── can-worker ── (uds in gateway) ...  │
│        ▲ 장치는 여기서만 열림                                        │
└────────┼─────────────────────────────────────────────────────────────┘
         │ WS (role=agent, daemon.json token)   WS (role=viewer, 쿠키)        WS (role=viewer)
   ┌─────┴──────┐                      ┌──────────────────────────┐   ┌──────────────────────┐
   │ loom CLI   │ ← 에이전트            │ 브라우저 탭 1 (operate)   │   │ 탭 2 (watch, 관전)   │
   └────────────┘                      └──────────────────────────┘   └──────────────────────┘
```

### 3.3 브라우저 내부 구성도
```text
            WS /rpc (탭당 1개)
                │ text: JSON-RPC (제어·RPC·M1 이벤트)   binary: loom.columns.v1 (M2 전, CAN/UART 데이터 구독만)
┌───────────────▼───────────────────────────────────────────────────────────────┐
│ Net Worker (Web Worker)                                                        │
│  RpcClient(id 매칭·64 KiB 검사) · SubscriptionMux(sub_id, refcount)             │
│  Decoder(JSON / columns → TypedArray) · ClockSync(core.time) · 경계 검증(Ajv)   │
│  분류: CONTROL kind ─► 즉시 단건 postMessage ───────────────────────────────┐   │
│        DATA     kind ─► 도메인 링(Worker 소유, 바이트 상한) → viewport projection│   │
└───────────────┬────────────────────────────────────────────────────────────┼───┘
                │ projection (Transferable, ≤ 1/프레임)               control │
┌───────────────▼──────────────────────────────────────┐ ┌──────────────────▼────────┐
│ Main thread (React 19)                               │ │ ShellControlStore          │
│  PanelHost ← PanelContext(store view·query·act·…)    │ │  latch·approvals·health·   │
│  RowStore(request_id → 행, 단조 갱신) · WriteTracker │ │  leases·env·errors·presence│
│  RenderScheduler: rAF, 예산 8 ms/프레임,             │ │  → 셸 바 즉시 렌더          │
│   초과 시 패널별 20 Hz 강등 + LIVE-DEGRADED         │ │  (useSyncExternalStore)    │
│  명령형 렌더러: xterm · Trace 그리드 · uPlot          │ └────────────────────────────┘
│  ActionGate: 자격·실수 방지 확인 → Worker → RPC       │
└──────────────────────────────────────────────────────┘
```
- **원본 링은 Worker가 소유**하고 메인에는 보이는 범위의 projection만 보낸다(React 상태에 프레임 배열을 append하지 않음 — sol R1 §3.2).
- **구독 공유·상한**: 같은 토픽을 여러 패널이 요청하면 서버 구독 1개를 refcount로 공유. 서버 상한(CORE 연결당 32 구독, UART 데이터 구독 채널당 4·전체 16 — UART DR-V21)을 넘는 탭은 조용히 실패하지 않고 셸에 `구독 상한 — 다른 탭을 닫거나 이 탭은 읽기 전용 미러` 배너. 미러(서버가 기존 구독 배치를 복제 전송) 여부는 CORE·UART와 합의(CR-UI-17). 서버 필터(`core.subscribe{filter}`)로 대역을 줄이고 패널 고유 필터는 클라이언트 뷰.
- **숨은 탭**: `document.hidden`이면 렌더 중단, `core.viewer.presence{visible:false}` 보고, 데이터 credit 축소. 제어 구독은 유지. 복귀 시 최신 snapshot + gap 한 줄(`⋯ 숨김 중 12.4 s (51k 프레임) — 라이브로 이동`)로 점프(백로그 전체 재생 안 함).
- **패널 수명주기**: 닫기 → refcount 감소 → unsubscribe → Worker 포트·observer·타이머·WebGL 컨텍스트 해제. 100회 열기/닫기 후 자원 평탄(UI-T10).

### 3.4 RX/TX 흐름
- **RX**: 배치 도착(Worker) → 경계 검증 → 디코드 → (제어면 즉시) → 링 append → dirty 표시 → 다음 rAF에 projection → 렌더 → 렌더 표본 기록.
- **TX(요청형·잡형)**: 패널 `ctx.act(id, params)` → ActionGate(자격·위험 등급별 UI 확인) → `<domain>.<verb>` RPC → 서버가 `origin=user, client_id=viewer:<tab>` 부여 → 이벤트 `requested…` → 같은 행 진행. RPC 응답보다 이벤트가 먼저 온다(publish-before-reply) → **UI는 이벤트로만 행을 그린다**(응답은 오류 표시용).
- **TX(키 스트림, UART)**: xterm `onData` → **사용자 의도 게이트**(키·붙여넣기·조합 이벤트 직후의 onData만 통과, 에뮬레이터 자동 응답 폐기) → 5 ms 묶음 → `uart.input{channel, data_hex, input_seq}`. Ctrl-C(0x03)는 묶음 대기 없이 즉시.
- 재연결 시 조회는 재시도, **mutation은 자동 재전송하지 않는다**(`request_id`로 상태 조회, `outcome_unknown` 유지).

### 3.5 시계 정렬과 lag
1. Worker가 `core.time{}`를 연결 직후 8회, 이후 5 s 주기·가시성 복귀·재연결 때 호출. 브라우저 `b0`(요청), `b3`(응답) = `performance.timeOrigin + performance.now()`, 데몬 `d1`(수신 mono), `d2`(송신 mono), `clock_id`.
2. `theta = ((d1−b0)+(d2−b3))/2`, `delta = (b3−b0)−(d2−d1)`. 최근 60 s 창에서 **delta 최소 표본** 채택(NTP식). 불확실성 ±delta/2를 함께 보관.
3. `mono_est(t) = t + theta`. 큰 불연속·`clock_id`/epoch 변경·sleep 복귀 시 매핑 폐기 후 재측정. 표시값은 5 ms 넘는 변화를 1 s에 걸쳐 수렴(배지 깜빡임 방지).
4. u64 ns는 BigInt로 epoch 기준 차이를 먼저 구한 뒤 ms Number로 변환(2^53 정밀도).
5. **lag** = 아직 그리지 못한 가장 오래된 이벤트의 나이(`mono_est(now) − ts.mono_ns`). 받은 게 없으면 lag 0 + `idle 5.2s` 별도 표시(조용한 장치를 지연으로 오인하지 않음). 셸 배지 = 보이는 패널 lag의 최댓값.
6. 상태: **LIVE**(목표 이내·무손실) · **LAG**(경로 목표 초과, 노랑; 2 s 초과 빨강 + 최신으로 점프) · **STALLED**(heartbeat 1 s 주기, 1.5 s 무수신 = 회색 경고점, **3 s 무수신 = STALLED** — 다른 배지보다 우선, 쓰기 잠금) · **LIVE-DEGRADED**(드롭·축소·20 Hz 강등 중). 임계는 탭 throttle·GC 순간 정지로 깜빡이지 않도록(UD-23).
7. 원격(SSH 포워딩)이면 시계 불확실성을 툴팁에 표시하고 로컬 목표와 따로 보고.
8. 장치 HW 시계(CAN HW ts 등)를 이 공식으로 브라우저에 맞추지 않는다 — CORE의 `ts.source/quality`를 따른다.

### 3.6 렌더 보고 (수용 시험·soak용)
- 탭이 `core.metrics.report{kind:"ui.render", …}`(CORE 이름)로 10 s마다 bounded histogram을 보고: 패널별 `stage ∈ {receive, decode, apply, draw, next_raf}` p50/p95/p99, `rendered_through_seq`, `oldest_pending_age_ms`, `clock_err_ms`, `visible`, 생략 수.
- 시험 하네스용 표본 모드: `kind:"ui.render.sample"`로 표본 1/50 이벤트의 `(seq, stage, t)`를 보냄(측정 자체가 부하를 만들지 않게 상한).
- rAF 콜백은 paint **전**이고 다음 rAF도 실제 스캔아웃 증명이 아니다 — proxy라고 명시하고, 고정 runner에서 headed trace로 오차를 보정한다(sol §9.4).
- 에이전트 RPC는 렌더 ACK를 기다리지 않는다. `published_through_seq`(허브)와 `rendered_through_seq`(뷰어)는 다른 값이다.

### 3.7 인증·자격 흐름
```text
사용자 터미널(TTY)에서 `loom ui`
  └► loomd: 1회용 bootstrap 비밀 c (128 bit, 60 s, 1회) 생성
       └► OS 기본 브라우저 실행: http://localhost:7420/ui/#boot=c     ← 비밀은 fragment(서버 로그·Referer에 안 남음)
            └► SPA가 fragment를 읽어 POST /bootstrap{c} → 검증·소비
                 → Set-Cookie loom_v=<세션>; HttpOnly; SameSite=Strict; Path=/   (Referrer-Policy: no-referrer)
                 → 자격 = operate
비TTY(에이전트)에서 `loom ui` / `loom ui open …`
  └► 열린 탭이 있으면 가리키기(core.ui_focus)만. 없으면 비밀 없는 URL + 안내 출력 "사용자 터미널에서 `loom ui`"
     (CORE 수용: 비TTY stdout에 bootstrap 미출력. **CORE에 추가 요청**: 비TTY `loom ui`/`ui open`은 bootstrap을 발급해 브라우저를
      열지 않는다 — core-opus 초안의 "브라우저 없으면 OS 브라우저를 bootstrap 포함으로 연다"와 충돌, CR-UI-07)
쿠키 없는 브라우저가 딥링크를 열면 → `/ui/#/need-auth`: "터미널에서 `loom ui` 실행" 안내(비밀 없음), 인증 후 원래 딥링크로 복귀
원격·헤드리스: 벤치에서 사람이 `loom ui pair` (TTY 필수, 6자리 코드, 120 s) → 원격 브라우저 /pair에 입력 → 쿠키(기본 watch, 선택 operate)
WS /rpc 업그레이드: Origin=http://localhost:7420 · Host 검사 · 쿠키 → hello{want_role:viewer} → 서버가 role + rights 부여
```
**자격(viewer role 안의 rights, R-2)**:
| 자격 | 발급 | 허용 |
|---|---|---|
| watch | `loom ui pair` 기본, 원격 관전 | 보기, STOP, 비상 OFF, 거부, 중단(abort) — **안전 방향만** |
| operate | TTY `loom ui`, `loom ui pair --operate` | + 사용자 쓰기·입력·take-control·hold·복구 송신·마커·R2 확인·판정 수정 |
| approve (M2) | operate + **승인 장치 등록**(WebAuthn, 등록 게이트 §10.3) **또는** `approval.presence=out_of_band_code` 호스트(인증기 없는 Linux 등)에서 벤치 TTY `loom ui pair --approve` 1회 | + R3/R4 승인·정책 완화·`unlock`·STOP 해제. 승인 시점 존재 확인은 각각 WebAuthn assertion / 대역 외 코드 |

- TTY 검사는 실수 방지 수준이다(에이전트가 pseudo-TTY를 만들 수 있음 — 한계 명시, D6 위협 모델).
- 쿠키는 브라우저 프로필에 남으므로 사람은 처음 한 번만 `loom ui`를 터미널에서 실행하면 된다.

### 3.8 Tauri — 언제·왜 (M4 선택)
| 이유 | 웹으로 해결? |
|---|---|
| 터미널에서 Ctrl-W/T/N이 브라우저 단축키로 먹힘 | 부분: 키 팔레트·바이트 송신 버튼·`Ctrl-] w`(0x17) 대체, Keyboard Lock 전체화면 모드(M2, 지원 브라우저만 [미확인]) |
| OS 알림·트레이 승인 알림 | 웹 Notification API로 대부분 |
| 승인 존재 확인 | WebAuthn으로 웹에서 가능(§10.3) |
| WebView2(Windows)·WebKitGTK(Linux) 렌더 성능 | Tauri의 **위험**(미측정) |
결론: M1–M3는 브라우저(Chromium 계열 1급). Tauri는 같은 SPA·같은 WS의 얇은 셸로만, 별도 장치 경로 금지.

---

## 4. CLI / API 명세 초안

### 4.1 UI 관련 CLI (CORE `loom` CLI에 추가 요청)
| 명령 | 역할 | 위험 | M |
|---|---|---|---|
| `loom ui [--layout NAME] [--no-browser]` | TTY: bootstrap 포함 브라우저 열기(operate). 비TTY: 비밀 없는 URL·안내만 | R0 | M1 |
| `loom ui open <domain>/<panel> [--channel C] [--filter EXPR] [--at seq:N\|mark:X\|-5s] [--reason TEXT]` | 열린 탭들에 가리키기(`core.ui_focus` 이벤트). 탭 없으면 URL + `no_viewer_attached` | R0, 2 s당 1회 | M1(이벤트·URL) / M2(토스트) |
| `loom ui status --json` | 탭 수·visible·lag·자격·레이아웃 | R0 | M1 |
| `loom ui pair [--operate]` | **TTY 필수**, 원격 브라우저 페어링 코드 | — | M1 |
| `loom ui layout list\|get\|put NAME -f file.json` | `~/.loom/config/layouts/` 관리, put은 기존 이름 덮어쓰기 금지 | R0 | M2 |

출력 예(`--json`, D5 봉투):
```json
{"schema_version":1,"ok":true,"cmd":"ui.open",
 "data":{"focused_tabs":1,"url":"http://localhost:7420/ui/#/p/can/trace?channel=ch0&at=seq:5012","viewers":{"n":1,"visible":1,"max_lag_ms":38}},
 "meta":{"env":"sim"},"warnings":[],"others_since_last":{"count":0,"origins":[],"from_cursor":"seq:4409"}}
```
딥링크 형식: `/ui/#/p/<domain>/<panel>?<props>&at=<cursor>`, 레이아웃 `/ui/#/l/<name>`. 링크에 자격·승인 토큰을 넣지 않는다.

### 4.1.1 `loom viz` — "데이터만 주면 렌더링" (C6로 CAN에서 UI 소유 이관, M3; 이력은 `can/plan.md` §4.4)
| 명령 | 설명 | M |
|---|---|---|
| `loom viz show SRC [--x COL] [--y A,B] [--long --series COL --value COL] [--db X.dbc] [--kind auto\|line\|step\|state\|trace\|table] [--group p1:A,B p2:C] [--layout f.json] [--no-open]` | 데이터셋 등록 + 뷰 생성 + 열린 탭에 가리키기(`core.ui_focus`) → view id/URL 반환 후 CLI 종료. 큰 파일은 ingest job + 진행률 | M3 |
| SRC | 파일(csv / jsonl / parquet / blf / asc / mf4 / trc), `-`(stdin), `live:can:ch0`, `live:uart:<ch> --extract REGEX`, `live:uds:did:F40D`, `session:<id>` | M3 |
| `loom viz push VIEW --jsonl -` | 에이전트 계산 결과를 기존 뷰에 추가(짧은 배치, 64 KiB 단위) | M3 |
| `loom viz list` / `close VIEW` / `summary VIEW [--json]` | 관리, 에이전트용 요약 통계(min/max/mean/구간 — **원본 기준**, 화면 축소 데이터로 계산 금지) | M3 |
| `loom viz export VIEW --csv [--max-points]` / `--png` | 제한된 데이터 export / 스냅샷 | M3 / M5 |
- **자동 렌더 규칙(`--kind auto`)**: ① CAN 프레임 형태(id/data 열 또는 BLF/ASC/MF4 버스 로그) → Trace + (DB 있으면) 신호 패널 ② 시간형 열(`ts`, `time`, `timestamp`, datetime, 단조 수치) → x축, 없으면 행 인덱스 ③ 수치 → line(정수이고 고유값 ≤ 16이면 step), 문자열·enum 고유값 ≤ 32 → State, 그 외 → 표 ④ long 형식(`ts, series, value, unit`) 자동 인식 ⑤ `--y` 없고 수치 열 > 8 → 8개 표시 + 나머지 체크박스 ⑥ 공통 이벤트 envelope JSONL은 도메인 어댑터로(UDS 트랜잭션 → V-UDS-1 표). 애매하면 UI에서 열·시간 형식·단위 선택 → sidecar preset 저장.
- 처리 위치: 파일 ingest·인덱스·min/max 피라미드·질의는 **데몬 쪽 Rust 모듈 `loom-viz`**(UI 팀 소유 crate, CORE 저널·질의 API 위). 형식 어댑터는 도메인 소유(CAN: BLF/ASC/MF4/TRC, UART: 추출 규칙, UDS: 트랜잭션). 브라우저는 질의 결과만 그린다(§1.3 — UI가 파일 포맷을 직접 파싱하지 않음).

### 4.2 패널 프레임워크 API (TS 초안)
```ts
// @loom/panel-sdk — 패널이 import할 수 있는 유일한 런타임 패키지 (타입은 생성물 @loom/schema)
export type Right = 'watch' | 'operate' | 'approve';
export interface TopicSpec {
  key: string;                         // 패널 내부 이름
  filter: (props: Props) => SubscribeFilter;  // core.subscribe{filter}로 내려감 (domains, channels, kinds, origins)
  encoding?: 'json' | 'columns';       // 대량 레인만 columns (M2 전)
  snapshot: 'required' | 'none'; snapshotKind?: string;   // 도메인 SnapshotProvider 종류
  maxBatchMs?: number;                 // UART 터미널 10
  coalesceBy?: 'id';                   // CAN Fixed 등 화면용 최신값 축약 (coalesced_count 표시)
}
export interface ActionSpec {
  id: string;                          // 'input' | 'send' | 'takeControl' | 'abort' …
  rpc: `${string}.${string}`;          // 서버가 Registry로 검증
  class: 'request' | 'job' | 'stream'; // A1 쓰기 클래스
  right: Right;                        // 최소 자격
  confirm: 'none' | 'enter_twice' | 'card' | 'self_approval';   // UI 실수 방지 (서버 정책과 별개)
  safetyDirection?: 'reduce';          // STOP·abort·비상 OFF: watch로도, 확인 없음
}
export interface PanelManifest {
  id: string;                          // 'uart.terminal'
  domain: string; title: string; version: 1; milestone: 'M1'|'M1b'|'M2'|'M3'|'M4';
  props: JSONSchema;                   // 인스턴스 인자 — 레이아웃·딥링크 직렬화 대상
  topics: TopicSpec[]; queries: string[]; actions: ActionSpec[];
  memory: { maxBytes: number };        // 탭 데이터 예산에서 할당 (§6.5)
  render: { maxHz: number; budgetMs: number };
  shellSlots?: ('channel_bar' | 'approval_body' | 'evidence_ref')[];  // 셸 슬롯 기여 (R1b C-a·C-e)
}
export interface PanelContext {
  readonly props: Readonly<Props>; readonly rights: ReadonlySet<Right>; readonly env: 'sim'|'bench'|'vehicle';
  store<T>(topicKey: DeclaredTopic): StoreMirror<T>;           // manifest 밖 key → throw. 원본 링은 Worker, 메인은 최근 N행 미러 + projection 요청
  query<R>(rpc: DeclaredQuery, params: object, signal: AbortSignal): Promise<R>;  // manifest 밖 rpc → throw. 큰 결과는 셸이 /api/v1/query로
  act(actionId: DeclaredAction, params: object): Promise<WriteHandle>; // ActionGate 경유
  scheduler: { invalidate(): void; onFrame(cb: (budgetMs: number) => void): Unsubscribe };
  clock: { monoEst(): bigint; errMs(): number };
  cursor: { link(group: string): void; set(seq: bigint): void; on(cb: (seq: bigint) => void): Unsubscribe };
  ui: { reveal(ref: EvidenceRef): void; openPanel(id: string, props: object): void; mark(label: string): Promise<void>; toast(msg: string): void };
}
export interface StoreMirror<T> { snapshotDone: boolean; headSeq: bigint; gaps: GapMark[];
  tail(n: number): Iterable<T>;                                  // 메인 미러(동기)
  project(view: { fromSeq?: bigint; rows: number; filter?: string }): Promise<Projection<T>>;  // Worker 링에서 보이는 범위만
  onAppend(cb: (from: bigint, to: bigint) => void): Unsubscribe; }
// DeclaredTopic·DeclaredQuery·DeclaredAction = manifest와 생성 스키마의 문자열 유니언 → 오타가 컴파일 오류
// 렌더 표본(stage)은 RenderScheduler가 자동 기록 — 패널마다 계측 코드 불필요
export interface WriteHandle { requestId: string; state(): WriteState; onChange(cb: (s: WriteState) => void): Unsubscribe; }
export interface PanelInstance { mount(el: HTMLElement): void; resize(w: number, h: number): void; dispose(): void; }
export function registerPanel(m: PanelManifest, factory: (ctx: PanelContext) => PanelInstance): void;
export function registerApprovalRenderer(domain: string, render: (display: unknown, el: HTMLElement) => void): void; // 선택 — 기본은 JSON Schema 일반 렌더러
```
- 패널 구현은 명령형 `mount/dispose`(xterm·그리드·uPlot을 직접 소유). 셸·폼·모달은 React 컴포넌트.
- 공통 컴포넌트(셸 소유, 도메인 재사용): **RowStore 표**(request_id 키 점진 갱신 — UDS·CAN TX·DLT CTRL·TEST 단계), **TerminalView**(정제·의도 게이트·접두키 — UART·SSH·RTT), **TimeSeries**(uPlot 래퍼), **ChannelBar 슬롯**.
- `ctx.ui.reveal(ref)`: 증거 REF(TEST 형식 `<domain>:<channel>@seq:<a>[..<b>][#ptr]`)를 파싱해 해당 패널·seq로 이동.

### 4.3 불변식 I-UI1 "패널은 장치를 열 수 없다"의 강제
| 층 | 수단 | 막는 것 |
|---|---|---|
| API | 패널은 `PanelContext`만 받는다. `store/query/act`는 manifest 선언분만(런타임 throw) | 선언 안 된 토픽·쓰기 |
| 빌드 | `ui/panels/**`에 ESLint `no-restricted-globals`·`no-restricted-imports`: `WebSocket, fetch, XMLHttpRequest, EventSource, WebTransport, navigator.serial/usb/hid/bluetooth`, `@loom/net` 금지. 위반 = CI 실패 | 우회 코드 유입 |
| 브라우저 | `Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()` [디렉티브 지원 브라우저별 미확인 → M1 헤더 시험], CSP `default-src 'self'; connect-src 'self'; script-src 'self'; frame-ancestors 'none'` | WebSerial로 COM 직접 열기, 외부 송신 |
| 리뷰 | 도메인 팀 패널 PR은 UI 리뷰 필수, manifest diff 자동 표시 | — |
(WS 1회용 티켓은 철회 — 패널이 1st-party 정적 번들이라 lint·CSP로 충분, R1b C-6.)

### 4.4 등록 방식
- **정적 번들**: 모든 코드 패널은 저장소 `ui/panels/<domain>/`에서 함께 빌드되어 `loomd`에 내장. 패널 ID 허용 목록으로 lazy chunk import.
- 도메인 팀 기여: (1) 패널 명세 `_xteam/<comp>-ui.md` → UI 팀 구현(1차 패널), (2) 도메인 팀이 `ui/panels/<domain>/`에 PR(UI 리뷰), (3) **spec 패널**(JSON, 코드 없음, M3) — 타입 `table, trace_table, timeseries, state_lamps, gauge, console, logic_lanes, decode_table, timeline_lane, run_panel`. payload에 코드·HTML 금지, 텍스트는 항상 textContent.
- 동적 로드(임의 JS)는 비범위.

### 4.5 UI가 쓰는 코어 RPC·이벤트 (이름은 CORE 계약 v1이 정본 — 현재 합의 후보)
| 용도 | RPC / 이벤트 | 상태 |
|---|---|---|
| 연결 | `hello{proto, client_id, want_role, capabilities}` | CORE 수용 |
| 구독 | `core.subscribe{filter, snapshot\|after_cursor, max_batch_ms, encoding, coalesce_by?}` → `snapshot_part/snapshot_end{H}` → `core.events` | CORE 수용 |
| 시계 | `core.time{}` → `{d1_mono_ns, d2_mono_ns, wall_ns, clock_id}` | core-sol 수용, core-opus는 단일 mono — 계약 v1 확정 필요 |
| 존재·가시성 | `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}` | CORE 제안 채택 |
| 렌더 보고 | `core.metrics.report{kind:"ui.render"\|"ui.render.sample", …}` | CORE 제안 채택(도메인 별칭 `ui.latency_sample`·`core.ui.latency_report` 폐기 요청) |
| STOP | CORE의 정지 RPC(`core.stop` / `core.stop_all` 중 v1 확정) → 이벤트 `core.latch{on, by, origin, acked, missing, jobs_stopped, approvals_cancelled, outputs_held[]}`, 해제 `core.stop.release`(approve 자격, M1은 operate + 확인 카드) + **HTTP `POST /api/v1/stop` 대체 경로**(WS 끊김 중) | 이름 미정·대체 경로 요청 |
| 승인 | `core.approval_requested/resolved`, `core.approval.respond{approval_id, decision, presence?}` | CORE 수용, presence 일반화 요청 |
| 복구 송신 | `core.safe_state.list` + `core.safe_state.run{ids[]}`(항목별 인터록·`finally_blocked` 결과) | run 묶음 요청 |
| 에이전트 확인 위치 | `core.agent_cursor{client_id, from_seq, to_seq, others_count}` | 요청 |
| 가리키기 | `core.ui.focus{panel, props, at, reason}`(agent) → 이벤트 `core.ui_focus` | CORE 수용 |
| 활동 | `core.activity`(에이전트 읽기 범위 `range{seq_a, seq_b}`·`predicate` 포함) | 필드 추가 요청 |
| 마커 | `core.mark{label, note}` | CORE 수용 |
| 질의 | `core.events.query`, `GET /api/v1/query/<id>`(NDJSON, ≤ 256 MiB) | CORE 수용 |
| 재생 | `core.replay.open{range, speed}` — 같은 구독 경로, `REPLAY` 배지, 서버가 쓰기 거부 | M3 전 요청 |
| 레이아웃 | `core.ui.layout.list/get/put` | 요청 |
| 공통 제어 뷰 | snapshot·리듀서의 `control{channel, owner_origin, actor, kind, reason, ttl}`(도메인 lease·lock·job owner 정규화) | 요청 |

---

## 5. 사용자 뷰어 UX

### 5.1 와이어프레임

#### 5.1.1 공통 셸 — M1
```text
┌ Loom · bench-pc · [SIM] ──────────────────────────────────────────────────────────────────────────────────┐
│ ● LIVE lag 38ms │ REC ok │ 보는 중: 이 탭 + 원격 1 │ 에이전트 claude-1 활동 2s 전 │ ⚠ 승인 1 │ [■ STOP 전체] │
├ ch: UART dut ●115200 gen1 │ CAN ch0 ●virtual 500k 3% │ UDS bcm ●DoIP ext L1 ─ 레이아웃: m1-default ▾ ─ 자격: operate ┤
│ ┌ UART dut ─────────────────────────────────────┐ ┌ CAN ch0 Trace ───────────────────────────────────────┐ │
│ │ (§5.1.2)                                      │ │ (§5.1.3)                                             │ │
│ └───────────────────────────────────────────────┘ └──────────────────────────────────────────────────────┘ │
│ ┌ UDS 콘솔 (§5.1.4) ─────────────────────────────────────────────────────────────────────────────────────┐ │
│ └────────────────────────────────────────────────────────────────────────────────────────────────────────┘ │
├ Activity ─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 21:05:11.020 AGT claude-1 uart.send dut "uname -a⏎" ◑보냄 │ 21:05:11.300 AGT claude-1 can.trace 읽음 seq 4400–4409 │
│ 21:05:16.410 USR 나 uart ^C ⇢ 에이전트에게 전달됨         │ 21:05:20.402 AGT claude-1 uds.read-did F190 ●확정      │
└ Ctrl-] x STOP · Ctrl-] A 승인 · Ctrl-] m 마커 · Ctrl-] ? 도움말 ──────────────────────────────── ● 21:05:21 ┘
```
- **env 배지 생략 금지**(B4): `[SIM]` 회색, `[BENCH]` 파랑, `[VEHICLE]` 반전 주황 + 상단 바 테두리 색. 규칙은 bench와 같음.
- 오른쪽 아래 `● 21:05:21` = 1 Hz 하트비트 시계(데이터가 없어도 갱신 — 화면이 멈추면 사용자가 안다).
- **[■ STOP 전체]**: 모든 레이아웃·모든 자격(watch 포함)에서 같은 자리, 확인 없음.
- 채널 바(두 번째 줄) = 도메인 `channel_bar` 슬롯(CAN 채널 바·UART 채널·UDS 대상). 연결·세대(`generation`)·backend 표시.
- 상단 바 공간이 부족하면 뒤부터 생략: **env > STOP 띠 > 연결/LIVE > 승인 수 > REC > 보는 사람 > 에이전트 활동**.
- M1 레이아웃 = 고정 프리셋 3개(`m1-default`, `uart-only`, `can-uds`) + 패널 최대화. 도킹·저장은 M2.

#### 5.1.2 UART 터미널 — M1 (내용 명세 `_xteam/uart-ui.md`)
```text
┌ UART dut ● OPEN 115200 gen1 [SIM] ─ 입력권: 비어 있음 ─ [터미널|트레이스] [HOLD] [키 ▾] [⋯] ──────── LIVE 12ms ┐
│[    3.112] systemd[1]: Started Journal Service.                                                             │
│── 백로그 / 라이브 ── (장식, 스트림에 삽입하지 않음)                                                           │
│root@dut:~# uname -a                                                                                         │
│Linux dut 6.6.21 #1 SMP PREEMPT aarch64 GNU/Linux                                                            │
│▒ 화면 생략 1.2 MB (seq 81102–93220) — 기록 정상 ▒   (이후 파서·SGR 재동기화)                                   │
│root@dut:~# █                                                                                                │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ AGT ▶ "uname -a⏎" ◑보냄 2s 전 │ 에이전트: expect "aarch64" 3.1/5s │ rx 1.2 KB/s │ 차단 질의×0 링크×0 │ REC ok│
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 입력권 칩: `비어 있음`(회색) · `나 (입력 중)`(노랑, [에이전트에게 반환 `Ctrl-] y`]) · `다른 뷰어 tab-3` · `🔒 에이전트 잠금 "사유" 87s`([가져오기 `Ctrl-] o`]) · `⏳ 에이전트 대기 "reboot⏎" 3.2/10s` · `미완결 줄 남음`(주황, [지우기 `Ctrl-] c`]) · `HOLD`(빨강) · `PAUSED — 키 송신 안 됨`(Ctrl-C만 통과). 입력권·dirty·hold는 **서버 상태가 정본**(브라우저 종료 후에도 `input_dirty` 유지).
- 포커스 규칙: 터미널 포커스일 때만 원시 키 전달, 포커스 테두리 색으로 "지금 키가 장치로 간다"를 표시.
- **Ctrl-C = 항상 0x03**(UART 명세). 복사는 `Ctrl-Shift-C`/`Cmd-C`/메뉴.
- **[키 ▾] 제어키 팔레트**: 브라우저가 가로채는 Ctrl-W/T/N 등과 Break·DTR/RTS를 버튼으로(M1). Keyboard Lock 전체화면 모드는 M2.
- **장치 출력 3중 방어**: ① 서버 `loom-console` 정제기가 위험 escape·단말 질의 제거(UART 소유) ② xterm parser hook으로 자동 응답·처리 삼킴 — DA·DSR·DECRQSS·window report, **OSC 4/10/11/12 색 질의(`?`)의 보고 문자열**, OSC 8 링크 ③ **사용자 의도 게이트**(§3.4). readonly 탭은 자동 응답 송신도 0건(UI-T07). OSC 52(클립보드)는 xterm 6.0.0 core에 핸들러가 없고 clipboard addon을 로드할 때만 생긴다 → **clipboard addon 로드 금지**(**확인**: R2 opus F1·F2, 설치된 `@xterm/xterm@6.0.0` `src/common/InputHandler.ts` OSC 등록 목록 0·1·2·4·8·10·11·12·104·110–112).
- gap 표시: 기본 xterm decoration(행 장식, `registerDecoration`은 **제안 API — `allowProposedApi:true` + 버전 고정**, §16.1). 문제가 생기면 파서 리셋 지점에서 gap 줄 삽입(UART 명세 허용).
- 트레이스 모드(`uart.trace`, M1 최소): 행 = RX 줄/TX 이벤트, 열 = 시각·출처·내용(보이지 않는 문자 기호화), 링 20,000행.

#### 5.1.3 CAN Trace — M1 (Chrono·raw) / Fixed·신호 펼침·필터식 M2 (내용 명세 `_xteam/can-ui.md`)
```text
┌ CAN ch0 Trace · virtual 500k [SIM] load 3% ─ [Chrono|Fixed M2] [Sym M2|Raw] filter [id in 0x300,0x301 ] ─ LIVE 54ms ┐
│▕ t(s)      dir 출처        id     dlc data                     상태      비고                                     │
│▕ 812.004   tx  AGT         0x300  1   01                       ●확정     claude-1 r-311                           │
│█ 812.009   rx  DEV         0x301  1   01                                 sim:BCM                                  │
│█ 812.050   rx  DEV         0x310  8   01 00 00 00 00 00 00 00                                                     │
│  ⋯ 3,812 프레임 화면 생략 (seq 9001–12812) — 기록 정상                                                             │
│  818.731   tx  USR(나)     0x300  1   00                       ●확정     ⇢ 에이전트에게 전달됨                     │
├────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 송신 [0x300#00           ] [보내기 ⏎] R1(sim) │ ⏸ 일시정지 │ 보이는 5,000 / 탭 링 100k │ 수신 4.1k fps · 화면 2k 행/s │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 왼쪽 여백 `█` = **에이전트가 읽은 범위 막대**(`core.activity` range, CAN 명세 C-9) — 사용자가 "에이전트가 어디까지 봤나"를 안다.
- Chrono 표시 예산 2,000행/s, 보관 하한 100k·상한 2M(데이터 예산 안), 넘치면 생략 행. 고부하 시 "Fixed로 전환" 배너(M2).
- 송신줄 입력 검증: hex, 11/29bit, DLC 0–8, FD 길이 집합(CAN 명세).
- Fixed 키 = `(channel, id, extended, frame_class)`, 갱신 10–20 Hz, `coalesce_by:"id"` 구독 + `coalesced N` 표시, gap 이후 count·period는 `?`.

#### 5.1.4 UDS 콘솔 — M1 (내용 명세 `_xteam/uds-ui.md`)
```text
┌ UDS ─ bcm ● DoIP 0x1001 sess=extended✓ sec=L1🔓 TP ✓×143 P2 50/5000 [SIM] ─ 제어: AGT claude-1 [제어 가져오기 T] ┐
│        ecm ● CAN 7E0/7E8 sess=default? sec=🔒 TP off                         ─ 제어: 비어 있음   │ 에이전트 확인 seq:431 │
├────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 시각          출처 대상 서비스                 요약                          단계·결과          p2/rtt             │
│ 21:05:11.020  AGT  bcm DiagSessionControl      → extended                     ●확정 ✓            9/9ms              │
│ 21:05:20.402  AGT  bcm ReadDataByIdentifier    F190 VIN=WVWZZZ…               ●확정 ✓            12ms               │
│ 21:05:31.114  USR  ecm ReadDataByIdentifier    F40D VehicleSpeed=0 km/h       ●확정 ✓            16ms               │
│ 21:05:40.950  AGT  bcm RoutineControl          start FF01                     ◑pending ×3        3.1s/5.0s ▲        │
│ 21:05:44.002  AGT  bcm WriteDataByIdentifier   0101 KR→EU                     ⚠R2 확인 대기      만료 98s           │
│ 21:05:47.100  AGT  bcm ECUReset                hard                           ⌛? 불확정          P2* 초과           │
│ poll#3 F40D,F405 ×1,204 last 42 km/h p2 12/15/31 ms NRC 0 skipped 3   (집계 행, 1 s 갱신)                         │
├ 마지막 값: F190 WVWZZZ… (12s) · F40D 0 km/h (1s) · 0101 KR (3m) ──────────────────────────────────────────────────┤
│ 대상 [bcm ▾] > [22 F190                 ] ReadDataByIdentifier F190 VIN · R0  [보내기 ⏎] [중단 !] [폴링 정지]    │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 행 키 `request_id`. **단계 칩**(D4/A14: ○요청 ◔수락 ◑보냄 ●확정 ●●검증 ●?미검증 ✗실패 ⌛?불확정) + **결과 기호**(UDS: ✓ ✗NRC ⏳ ⌛ ⌛? ⛔ late ok_suppressed)를 함께.
- 명령줄: 입력 150 ms 정지 → `uds.request{dry_run:true}`(viewer, 감사 없음, 레이트 제한) 미리보기 p95 ≤ 150 ms. R2 이상은 Enter 두 번 또는 확인 카드.
- TesterPresent는 상태 바 카운터로 접힘. 폴링은 집계 행(최종 응답·실패·승인·정지 이벤트는 집계 금지).
- M1: 표·대상 바·명령줄·[제어 가져오기]·[중단]·**R2 확인 카드**·마지막 값 표. DTC·세션 패널·`active_overrides`·jobs·recover 패널은 M2.

#### 5.1.5 승인 대기열 / 승인 카드 — R2 카드 M1, R3/R4 모달 M2
```text
상단 ⚠ 승인 2 클릭 또는 Ctrl-] A
┌ 승인 대기열 (2) ──────────────────────────────────────────────────┐
│ ▶ a17 R3 AGT claude-1 can.periodic 0x2A0 @20ms ch0       만료 98s │
│   a18 R2 AGT claude-1 uds.write-did bcm 0101 KR→EU        만료 41s │
└────────────────────────────────────────────────────────────────────┘
┌ 승인 요청 a17 ──────────────────────────────────────────── [BENCH] ┐
│ 요청자   agent claude-1 (Claude Code · client cl-7f3a)               │
│ 동작     can.periodic.start → ch0 (gen 2)                            │
│ 내용     (도메인 렌더러 / approval.display 일반 렌더러)                │
│          0x2A0 EngineData @20ms · EngRPM 3000 · bytes 0B B8 5A 00 …   │
│ 등급     R3 · 범위 [이번 1회 ▾ | 세션 ≤ 10분]                          │
│ 사유     (에이전트 작성) "엔진 신호 모사"                               │
│ plan     sha256 9f3c…e1 · 사용 기한 10분 · 요청 만료 98s               │
│ ──────────────────────────────────────────────────────────────────── │
│ 요청 코드 끝 2자 [  ]  (a17-··7K)                                     │
│ [거부 (기본 포커스)]  [나중에]  [승인 — 본인 확인 🔐]                   │
│ 1.0 s 후 활성 · 이 화면은 사람만 조작합니다 (AI 에이전트: 조작 금지)    │
└──────────────────────────────────────────────────────────────────────┘
```
규칙(UD-08): 1.0 s 무장 지연, **기본 포커스 = 거부**, Enter·Esc는 절대 승인하지 않음(Esc = 보류), 승인은 버튼 클릭 또는 카드 포커스 상태 대문자 `Y`(키 자동 반복 무시), R3는 요청 코드 끝 2자 입력, R3/R4 승인 버튼은 존재 확인(M2). 모달 열린 동안 터미널 키 전달 중단(장치 출력은 계속). 여러 탭: 모두 알림, 먼저 응답한 쪽 유효, 나머지 탭은 "tab-3에서 승인됨" 토스트. 원시 토큰은 표시하지 않음(`approval_ref`만). `default_blocked`·하드 차단은 "차단됨 — 승인 불가" 안내만.
- **R2 확인 카드(M1)**: 같은 틀에서 존재 확인 없이 [확인] / [거부]. sim은 `auto`라 카드가 뜨지 않음(A8) — M1 수용 시험은 bench 정책 프로파일 fixture로 세 경로(token·viewer·auto)를 시험(UDS E2E t2).
- **사용자 본인이 시작하는 R3/R4**(CAN IG 주기 송신, UART 부트로더 시그널 등): "요청+승인"을 합친 **본인 확인 모달 1회**. 서버가 승인 객체를 만들고 같은 존재 확인을 요구한다(브라우저 자동화가 user로 위장해 R3를 스스로 확인하는 경로 차단, UD-09). viewer라는 이유로 deny·rate 검사를 건너뛰지 않는다.

#### 5.1.6 테스트 실행 화면 `shell.test` — M1b 최소판 (내용 명세 `_xteam/ui-test.md`, `test/plan.md` §5)
```text
┌ 테스트 TC-1 BOOT-CAN-DTC · 세션 ts-0042 · claude-1 · [BENCH] · RUNNING · 사전 승인 R2×3 R3×1 개시 기한 08:12 ── [⏸][■ 중단] ┐
│ 단계                                       상태      증거 (Loom 검증: 존재·창·술어 · gap)        판정                       │
│ 1 PSU ON, UART "login:" 대기 30s           ✓ 4.2s    uart:dut@seq:1201..1288  ✓ ✓ ✓ · gap 없음   pass                       │
│ 2 CAN VehState.PowerMode == 2 (5s)         ✓ 0.8s    can:ch0@seq:5530         ✓ ✓ ✓ · gap 없음   pass                       │
│ ▶3 UDS DTC 읽기 mask 0x09 → 0건             ◑ 12s     uds:bcm@seq:512 pending ×1                  —                          │
│ 4 (수동) 계기판 부팅 로고 확인              ○         —                                           사람 판정 필요              │
│ 5 UDS ClearDTC 후 재확인                    ○                                                                                │
├ 실시간 증거 (단계 3, 최근 2k행) ────────────────────┬ 판정 근거 (단계 2) ──────────────────────────────────────────────────┤
│ 21:07:01.204 AGT uds bcm 19 02 09 ◑pending ×1       │ 에이전트: "PowerMode=2 at can:ch0@seq:5530"                           │
│ 21:07:01.205 AGT can ch0 0x7E0 03 19 02 09 (ISO-TP) │ Loom: 존재 ✓ · 단계 창 안 ✓ · 술어 2 == 2 ✓ · 창 안 gap 없음           │
│ 21:07:02.010 USR can ch0 0x300 00   [판정 제외]      │ [증거 열기 → CAN Trace seq 5530]  [판정 수정…(사유 필수)]              │
├─────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────┤
│ 단계 띠: │▓1▓│▓2▓│▶3   │ (모든 레인 공통)  │ 질문: "계기판에 로고가 보입니까?" [예] [아니요] [직접 입력]  │ [보고서]       │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 판정 값 `pass | fail | inconclusive | blocked | skipped` + 자극 전용 단계 `done` + `incomplete`(TEST), 아이콘 ✓✗?⊘⏭▶·. 증거 검증 5상태 `verified / mismatch / not_found / uncertain / error`를 아이콘·문구 표로 고정. 판정은 `{proposed, effective, by, overrides}`로 원판정·사용자 수정을 병기. **UI는 판정하지 않는다** — 서버 검증 결과만 확정 판정으로 표시. 지어낸 seq로 `verdict pass` 시 거부 사유(`E_EVIDENCE_MISSING/MISMATCH/OUT_OF_WINDOW`, `agent_tx_only`, `inconclusive{cause:"eval_budget"}`)를 현재 단계 칸에 그대로.
- 세션은 동시 1개(`E_SESSION_ACTIVE` exit 6). 일괄 승인 `Approval{kind:TestPlan}`: 사용 시작 ≤ 10분, 세션 유효 `max_test_duration`(기본 1 h, ≤ 8 h), `max_uses` 잔량 원자 차감, 계획 변경·STOP·epoch 변경 시 무효 → 배너. **계획 밖 쓰기**는 개별 승인 + 행에 "계획 밖" 표식(core-test O7).
- **계획 검토 모달**: 원문 문장 ↔ 해석 단계 대조, 술어를 사람 문장 + AST로, 등급, 승인 묶음(R4 제외), plan hash → 계획 승인 + 일괄 승인. R3 포함 시 M2부터 존재 확인 대상.
- 사전 승인 기한 두 값: `개시 기한 08:12` → 시작 후 `세션 유효 52:10`, 횟수 잔량(`psu.out on 3/5`).
- 질문 대화상자는 승인 카드와 **다른 색·위치**(승인으로 오인 방지), 남은 시간 표시.
- `REC FAILED`면 상단에 `REC FAILED — PASS 판정 불가`.
- 단계 띠는 M1b에선 셸 하단 간이 타임라인, M3 통합 타임라인에서 모든 레인 공통.
- 러너(`loom run`) 실행도 같은 화면을 run 모드(판정 수정 대신 단계 결과)로 쓴다(EXPLORE 명세).

#### 5.1.7 STOP 후 상태 — M1
```text
┌ Loom · [BENCH] · ● LIVE ─ ■ STOP — agent·script 쓰기 잠김 (21:07:03, 나 tab-1) ─────────────────── [해제…(approve)] ┐
│ ┌ STOP 상태 ──────────────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │ 잠긴 것: 새 agent·script 쓰기 · 잡 3개 정지 (can p3 0x2A0, uds poll#2, run r-0012) · 대기 승인 1건 취소        │ │
│ │ 이미 드라이버에 넘어가 취소 못 했을 수 있음: CAN ≤ 2프레임 · UART 12 B · UDS r-77 ⌛?                           │ │
│ │ ⚠ STOP — 출력 유지 중:  psu bench ON 13.5 V 0.35 A  [비상 OFF !]                                              │ │
│ │ 복구 송신 (safe_state · 사용자 출처 · 인터록 검사):                                                            │ │
│ │   ☑ can.periodic.stop     ☑ uds.recover bcm (세션 default, IO 제어 해제)                                       │ │
│ │   ☐ psu.out bench off  ⛔ 막힘: uds.flashing 진행 중 PSU OFF 금지 (인터록)                                     │ │
│ │   [복구 송신 ⏎]    run r-0012의 finally는 자동 실행하지 않습니다 (A15)                                        │ │
│ │ 사용자 입력은 계속 가능합니다 (UART 키, 1회 송신, UDS 명령줄).                                                 │ │
│ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ │
```
- STOP 직후 STOP 상태 패널 열림, **[복구 송신 ⏎]에 포커스**(단 터미널에 입력 중이면 포커스를 뺏지 않고 띠만) — A15의 "복구 송신 1키". 실행은 `core.safe_state.run{ids[]}`, 항목별 결과(인터록 막힘·`finally_blocked`) 표시.
- [비상 OFF]: 확인 없음, 인터록은 경고만(A3), watch 자격 허용. 일반 복구 송신은 인터록·deny를 통과해야 함(A15) — 둘의 예외 차이는 CORE 정책 표가 정본(CR-UI-08).
- [해제…]: 위험 방향 → approve 자격(M1은 operate + 확인 카드, M2부터 approve). 자동 재개 없음. 데몬 재시작 후에도 latch 유지(STOP 띠 유지).
- STOP 버튼 클릭 → 1 프레임 안 "STOP 요청 중…"(낙관 표시) → 서버 `core.latch{acked}` 시 "STOP — 잠김"; 2 s 내 확인 없으면 빨강 "STOP 확인 안 됨 — 재시도"(클릭만으로 정지 성공을 칠하지 않음). WS가 끊겨 있으면 `POST /api/v1/stop`.

### 5.2 사용자 개입 UX

#### 5.2.1 개입 동작 표
| 동작 | 어디서 | 키 | 자격 | 쓰기 경로 | 에이전트가 받는 것 |
|---|---|---|---|---|---|
| UART 키 입력(원시) | 터미널 포커스 | 그대로 | operate | `uart.input`(③ 스트림) | 대기·`E_USER_ACTIVE`·`others_since_last` |
| 1회 송신(CAN/UDS/DLT) | 패널 명령줄 | Enter (R2+ 두 번/카드) | operate | 도메인 요청형 RPC | `others_since_last` |
| take-control | 패널 헤더 | UART `Ctrl-] o`, UDS `T` | operate | `uart.input.takeover` / `uds.lease.takeover` | exit 6 + lease 정보 |
| 입력권 반환·미완결 지우기·hold | 칩 버튼 | `Ctrl-] y / c / h` | operate | `uart.input.yield/clear`, `uart.hold` | 대기 send 진행 / `E_HELD_BY_USER` |
| 승인·거부 | 카드·모달 | `Ctrl-] A`, 승인 `Y`/버튼 | 승인 operate(R2)·approve(R3/R4) / 거부 watch | `core.approval.respond` | exit 0 / 5 `DENIED by user` |
| STOP | 상단 버튼 | `Ctrl-] x` (전역) | watch | CORE STOP RPC (+ HTTP 대체) | 이후 쓰기 exit 6 `E_STOPPED` |
| 비상 OFF | STOP 패널·도메인 패널 | 도메인 `!`(터미널 밖) | watch | 도메인 safe_state RPC | `others_since_last` |
| 복구 송신 | STOP 패널 | Enter | operate | `core.safe_state.run` | `others_since_last` |
| 마커 | 셸 | `Ctrl-] m` | operate | `core.mark`(user) | 커서 `mark:X` |
| 판정 수정·질문 답 | 테스트 화면 | — | operate | `test.verdict{overrides, reason}` / `test.answer` | 판정 revision / `test ask` 반환 |

- **전역 키**: 셸이 `window` keydown capture 단계에서 `Ctrl-]` 접두키를 먼저 받는다. 터미널 포커스여도 접두키 다음 한 글자는 셸이 해석(`Ctrl-] Ctrl-]` = 0x1D 리터럴). 단일 글자 키(`j/k`, `T`, `!`, `Y`)는 패널 포커스이고 입력란 밖일 때만.
- 웹에서 비터미널 STOP 키 **`Ctrl-X`는 채택하지 않음**(브라우저 잘라내기 충돌, R-1). STOP = 버튼 + `Ctrl-] x`.

#### 5.2.2 입력이 섞일 때의 표시
1. 출처 배지는 서버 이벤트 `origin`만 쓴다(클라이언트 추측 금지, 장치 에코는 DEV).
2. 출처 색(UD-07, 전 팀 통일): **AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라 · EXT 보라 점선**(EXT는 origin이 아니라 `external_actor` 부가 배지). 색 + 문자 병기(색만으로 상태 전달 금지). 내 입력 `USR(나)`, 다른 탭 `USR(tab-3)`.
3. 제어 칩은 채널 단위로 하나 — 도메인 lease·lock·job owner를 CORE 공통 `control` 뷰로 정규화: `제어: 비어 있음 | 나 | 다른 뷰어 | AGT <id> [사유, 남은 s]`.
4. **`⇢ 에이전트에게 전달됨` 표식**: 사용자 쓰기 행은 `⇢ 전달 대기` → 그 채널을 쓰는 에이전트의 다음 결과에 그 seq가 `others_since_last`로 포함되면(`core.agent_cursor`) `⇢ 에이전트에게 전달됨 (시각)`. UDS 상단 "에이전트 확인 위치 seq:N"도 같은 이벤트.
5. 에이전트가 대기 중이면(`deferred`) 칩에 대기 송신 원문 표시.
6. 같은 사용자 탭이 여러 개면 키를 먼저 친 탭이 writer. 다른 탭에서 키를 치면 `다른 뷰어 tab-1이 입력 중 — Ctrl-] o로 가져오기` 토스트, Ctrl-C는 어느 탭이든 즉시 통과(UART 다중 viewer 규칙의 최종안은 UART 서버 계약 하나로 합의 — UART 의존).

#### 5.2.3 쓰기 단계의 점진 갱신 (행 하나)
| 단계 | 기호·색 | 의미 | 화면 규칙 |
|---|---|---|---|
| requested | `○요청` 회색 | 접수(승인 대기 포함) | 행 생성. 승인 대기면 `⚠R2/R3 승인 대기 만료 98s` |
| accepted | `◔수락` 회색 | 정책 통과·큐 진입 | 큐 위치 `#2` |
| sent | `◑보냄` 청록 | 드라이버·소켓에 넘김(**wire 완료 아님**) | 경과 시간 클라이언트 측 증가 표시 |
| confirmed | `●확정` 초록 | 장치·버스 확인 | 0.5 s 하이라이트 |
| effective_verified / effect_unverified | `●●검증` / `●?미검증` | 되읽기 일치 / 불일치·불가 | 같은 행 |
| failed | `✗실패` 빨강 + 원인 | | 상세에 다음 행동 |
| outcome_unknown | `⌛?불확정` 주황 | 실행됐을 수 있음 | **실패로 칠하지 않음** |
규칙: (1) 키 = `request_id`(스키마 필수 필드 `request_id` + `payload.write_state`, UD-11 — kind 이름은 도메인 자유). (2) **단조 갱신** — 낮은 단계가 늦게 오면 무시. (3) 행 위치는 요청 시각 순 고정. (4) 성공 색은 `confirmed` 이후 — UART 성공 종단은 `sent`라 UART 행은 `◑보냄`에서 끝남(A14). (5) 최종 단계 이벤트는 제어 경로. (6) 0x78 pending은 같은 행 카운터, 마지막 pending 이후 정체를 빨강.

### 5.3 Activity
행 = 에이전트·스크립트·사용자 행동(쓰기 요청, **읽기 질의와 읽은 seq 범위**, 대기 시작/종료와 술어, 승인 요청, lease 변화, 가리키기). CORE `core.activity` 1 Hz 집계. 필터: 출처·도메인. 행 클릭 → 해당 패널·seq. M1은 하단 2줄 띠 + 펼치기.

### 5.4 연결·장애 표시
- `REC failed` → 빨강 배너 "기록 불능 — 에이전트 쓰기 중단됨, 사용자 입력은 가능"(A12).
- worker 크래시 → 해당 패널만 빨간 배너 + 재기동 횟수. `daemon restarted (epoch 3→4)` 배너 + 재snapshot.
- WS 끊김 → 지수 백오프 재연결(0.5→8 s), 동안 **쓰기 버튼 잠금**(STOP 제외 — HTTP 대체). 재연결 후 `after_cursor`로 이음, 못 이으면 gap 행 + 질의로 채움 시도.
- CORE 우선 큐 포화로 연결이 끊긴 경우(CORE 설계) → 정상 경로로 처리: 재연결 배너 + 재snapshot.
- `generation` 변경(장치 재연결) → 패널 내 세대 구분선 + `source.gap{cause}`.

### 5.5 기록 탐색·재생 (M3 / Replay M5)
시간 막대(세션 전체) + 마커 + gap 음영. 과거로 이동하면 패널이 `HISTORY` 모드(큰 배지), 라이브는 계속 받되 화면은 질의 결과. `[라이브로 ⏭]`. 재생(`replay:<session>`)은 서버가 쓰기 거부 + `REPLAY` 배지, 과거 AGT 행은 `original_origin` 표시.

### 5.6 마커·커서
`Ctrl-] m` 마커(user) → 모든 패널 시간축·트레이스에. 패널은 커서 그룹으로 연결(같은 시점 이동). 증거 REF 클릭 → `ctx.ui.reveal`.

### 5.7 레이아웃·가리키기
- 저장(M2): 데몬 `~/.loom/config/layouts/<name>.json`(1 MiB 상한, 버전 있는 앱 모델로 감싸 도킹 라이브러리 직렬화 변경을 migration). 탭 로컬 조정은 localStorage(실패해도 동작).
```json
{"loom_layout":1,"name":"can-analysis","env_hint":"bench",
 "root":{"split":"h","sizes":[60,40],"children":[
   {"panel":"can.trace","props":{"channel":"ch0","mode":"chrono","filter":"id in 0x300,0x301"}},
   {"split":"v","sizes":[50,50],"children":[
     {"panel":"can.graphics","props":{"signals":["VehicleSpeed","EngRPM"]},"cursor":"A"},
     {"panel":"uds.console","props":{"targets":["bcm"]}}]}]},
 "pinned":["core.activity"]}
```
- **새 채널 규칙(숨은 채널 방지, 모든 도메인 공통)**: 에이전트가 채널·대상을 새로 열면(`*.channel_state{opened}` 류) 모든 탭의 채널 바에 **항상** 추가 + 토스트 `에이전트가 uart dut2를 열었습니다 [패널 열기]`, 탭 설정 `auto_open_panels: on(기본) | off` — off여도 채널 바에는 남는다(UART 명세 Q13·`uart/plan.md` §3.12).
- 가리키기: `core.ui_focus` 이벤트 → 각 탭 토스트 `[열기 ⏎] [이 탭은 자동으로 따라가기 ☐] [무시 Esc]`. 탭 설정 `follow_agent: ask(기본) | auto | off`, auto라도 터미널 입력 중이면 보류. **에이전트는 화면을 (허락 시) 바꿀 수만 있고 버튼을 누를 수 없다.**

### 5.8 탐색 도메인 패널 (내용 명세 `_xteam/explore-ui.md`)
| 패널 | M | UI 요점 |
|---|---|---|
| `psu.gauge` | **M1b**(sim, 필수) | gauge spec 패널 + 실측 60 s 시계열 + `!` 비상 OFF(확인 없음) + "STOP — 출력 유지 중" 배지(STOP 패널 "출력 유지 장치" 목록의 데이터원). TEST TC-1 PSU 단계에 필요 |
| `dlt.trace` + 컨텍스트 트리 | M1b(도메인이 준비되면) / 아니면 M2 | Trace 그리드·RowStore 재사용이라 비용 작음. CTRL 행 점진 갱신(`requested→sent→confirmed(ACK ms)→effective_verified`), [요청][보고][대기] 3값, 장치→화면 p95 ≤ 150 ms, CTRL·gap 드롭 금지, MCNT `source.gap` |
| `la.capture` | 상태 배지·진행·디코드 표 M1b(도메인 준비 시) / **전이 보존 레인 렌더러 M2** | 캡처 중 "파형은 종료 후 표시" 배지 + 타임라인 음영(상태 p95 ≤ 250 ms), 캡처 후 전이 보존 레인(LTTB 금지, uPlot 부적합 → canvas 직접)·디코드 표·`uart.tx`/`can.tx` 정렬선·오프셋 라벨(`host-approx`), `s`/`x` 키, Logic 2 앱 조작은 Loom이 못 본다는 고정 안내 |
| `shell.console`(SSH) · `debug.watch` | 도메인 일정 | TerminalView·TimeSeries 재사용 |

### 5.9 접근성·공통 표시 규칙
색만으로 상태 전달 금지, `aria-live`는 승인·STOP·NRC·오류만, 고대비·모션 줄이기, 모든 동작에 버튼 경로, 키보드 경로는 M1 Playwright 시험에 포함. DOM에 `data-request-id`·`data-env`·`data-origin` 속성 규약(수용 시험이 DOM으로 확인).

### 5.10 TUI의 위치 (보조)
- 남김: **UART `loom uart attach` 터미널 모드**(herdr 패인, 원시 키, UART 팀 소유 — 웹과 같은 구독·같은 InputArbiter), SSH만 되는 환경의 `loom status --watch`(CORE).
- 안 만듦(R-5 요청): CAN `monitor`·UDS `view` Textual TUI(Python — C1과 충돌, 웹 M1이 대체). 승인은 웹에서만(TUI 승인 화면 없음).

---

## 6. 데이터 / 기록 포맷

### 6.1 UI가 소유하는 데이터
| 데이터 | 위치 | 비고 |
|---|---|---|
| 레이아웃 | 데몬 `~/.loom/config/layouts/*.json` | 사용자·에이전트 공유, 이름 충돌 시 새 이름 |
| 탭 로컬 설정(열 폭·follow_agent·테마) | localStorage (try/catch) | 실패해도 기본값 |
| 마커·승인 기록·판정 수정 | 데몬 이벤트·감사 로그 | UI가 따로 저장하지 않음 |
| WebAuthn 공개키(M2) | 데몬 `~/.loom/config/presence/` | 등록·삭제는 감사 + 모든 탭 알림 |
UI는 **기록을 직접 쓰지 않는다**. 모든 보존은 데몬 저널(D7).

### 6.2 전송 형식 (CORE와 합의, `_xteam/core-ui.md`)
1. 제어·RPC = JSON-RPC 텍스트 프레임(D2). 이벤트도 **M1은 JSON**(허브가 1회 직렬화 공유).
2. **M2 성능 관문 전**, CAN·UART **데이터 구독만** capability 협상 후 binary opt-in: `loom.columns.v1`(CORE 초안 헤더: magic/version/schema_hash/sub_id/batch_id/stream_epoch/count/first_seq/last_seq/hub_publish_mono_ns/daemon_wall_ns/flags/lengths + 공통 메타 사전 + 도메인 열). JSON fallback은 같은 의미 필드를 제공하고, binary에서 origin·seq·quality·request_id·source identity를 생략하지 않는다.
3. 근거(**측정**): UI S1 — 10k/s에서 JSON 53.2 B/행 vs column 24.2 B/행, decode p95 0.065 vs 0.015 ms/메시지, **종단 렌더 proxy 지연은 차이 없음**(p95 ≈ 33 ms). CORE 스파이크 — 서버→Rust 수신 p99 JSON 0.82–1.12 ms vs 열 binary 0.29–0.43 ms, 배치 크기 57 KB vs 14 KB. 결론: 지연보다 **대역·CPU·64 KiB 배치 상한 여유**가 binary의 이유이며, full D3·CAN FD 64 B 스키마로 M2 전에 재측정해 확정.
4. UI decoder 계약: 길이·offset·정렬·count·최대 메모리 검사 후 view 생성, unknown version 거부, 최대 u64 BigInt 경계. Rust/TS golden corpus + fuzz(가변 DLC·혼합 domain·빈 배치·잘린 offset)를 계약 관문으로. MessagePack은 UI binary codec으로 채택하지 않음(측정상 이득 없음).
5. permessage-deflate 기본 off(loopback). 원격에서 압축은 별도 측정 후 선택.
6. 64 KiB 초과 질의 결과는 `GET /api/v1/query/<id>` — 패널은 `ctx.query`만 부르고 셸이 경로 선택. Arrow IPC는 대용량 과거 질의·artifact에 한해 M3 검토.
7. u64/i64는 JSON decimal string(`type:string` 패턴), 브라우저는 BigInt로 처리.

### 6.3 스키마 → TS (C3)
- Rust serde + **schemars 1.2.3** → JSON Schema 2020-12 → **json-schema-to-typescript 16.0.0** → `ui/src/gen/*.ts`. CI: 생성물 diff ≠ 0이면 실패, schema hash 일치 검사, Rust serialize → JS validate roundtrip.
- 경계 검증: handshake·제어·snapshot·binary 헤더는 **Ajv(strict)** 런타임 검증(타입 생성 ≠ 런타임 검증).
- ts-rs 직접 경로는 쓰지 않는다 — 근거(**측정**, `spikes/ui/sol/schema-probe/`): ts-rs 12.0.1은 `skip_serializing_if=Option::is_none` 필드를 required로 생성해 실제 JSON fixture가 tsc에서 실패, schema 경로는 `note?: string|null`로 통과. u64 최대값·2^53+1 decimal string 보존 확인.
- CORE 스키마 fixture JSONL(정상·고부하·gap·쓰기 실패·`outcome_unknown`·late)을 UI 단위 시험 입력으로 그대로 사용.

### 6.3.1 viz 데이터셋 계약 (정본 이관: `_xteam/viz-requirements.md` 표준 입력 계약 + `can/plan.md` §6.4)
- **long JSONL/CSV**: `{"ts": <epoch s | ISO8601 | {"mono_ns","wall_ns"}>, "series": "…", "value": <num|str|bool>, "unit": "…", "quality": "valid|gap|invalid|…", "source": {"domain","channel","seq","rx_offset"?,"request_id"?}}`. **wide CSV**(`ts, a, b`)도 수용 — 시간 열·수치 열 명시 또는 탐지, 애매하면 선택 UI.
- 메타(선택): `series_id, display_name, value_type(number|enum|bool|string|bytes), unit, enum_map, time_source, quality, source_domain, session_id, source_seq`.
- 공통 이벤트 envelope JSONL(D3)은 도메인 어댑터로 자동 인식, 레코드의 `metrics[{name,value,unit}]`(탐색 CR-X2)는 도메인 무관 자동 플롯 후보.
- 렌더 규칙: `quality=gap|invalid`·`generation` 변경·추출 실패 구간은 **선을 끊는다**(NaN 보간 금지). 비수치(DID enum·bool·문자열)는 State/표. 서로 다른 시계의 비동기 신호를 union timestamp로 펼치지 않는다(임의 보간 금지). 시각 품질(CAN HW / UART `host_read_complete`)을 범례·툴팁에.
- 축소: 서버 `signal.query{t0,t1,px,mode:minmax}` → series당 ≤ min(4×px, 20000), first/last/min/max 시각 순서. 확대 시 재질의, 화면에 `downsampled N→M`. 커서·통계·TEST 판정은 원본으로 계산. LA 레인은 전이 보존 요약(LTTB 금지).
- 원문 역참조: source seq·UART `rx_offset`·UDS `request_id`를 끝까지 보존 → 점 클릭 → 원본 패널·seq(`ctx.ui.reveal`).
- 라이브 소스: `live:<domain>:…` = `core.subscribe` 구독 + 늦은 합류 snapshot + gap. **브라우저·viz는 장치를 다시 열지 않는다.**
- 지연: 데몬 수신 → 플롯 반영 p95 ≤ 250 ms(UART 추출값 200 ms). 규모 1 kHz × 수 시간(~10^7 점)은 서버 축소로.
- 우선순위(viz-requirements 수용): V-UDS-1 > V-UART-1 > V-UDS-2 > V-UDS-4 > V-UART-4/3 > V-UDS-3 > V-UDS-5 > V-UART-2/5.

### 6.4 이벤트 → 화면 매핑 공통 규칙
`origin` → 배지, `dir` → 화살표, `request_id`+`payload.write_state` → 단계 칩, `risk` → 등급 표시, `ts.quality/source` → 툴팁(host_read_complete vs HW ts), `metrics[]` → 자동 플롯 후보(viz), `purpose=tester_present|aggregate` → 접기, `generation` → 세대 구분선, `external_actor` → EXT 배지.

### 6.5 브라우저 메모리 상한 (C2) — 탭당 데이터 예산 256 MiB
| 저장소 | 기본 / 상한 | 넘칠 때 |
|---|---|---|
| CAN 원본 링(Worker, typed) | 100k행 / 채널, 데이터 예산 안에서 2M까지 | 오래된 것 버림 + "질의로 조회" |
| CAN Fixed 테이블 | 키 LRU 상한 4,096 | 퇴출 수 표시 |
| UART scrollback | 10,000줄 / 채널(사용자 설정 상한 50,000) | 오래된 줄 버림 |
| UART 원시 링 · 트레이스 링 | 4 MiB · 20,000행 / 채널 | 오래된 것 버림 |
| 터미널 수신·그리기 대기 | 2 MiB / 채널(UART DR-V22), 탭 합 8 MiB | 화면 생략 + gap + 파서·SGR 재동기화 |
| UDS 트랜잭션 | 진행 중 전부 + 완료 20,000행 / 32 MiB 중 먼저 | 완료된 오래된 행부터 |
| 플롯 질의 캐시 | 64 MiB byte-LRU | 취소·세대 검사 |
| 디코드 대기(in-flight) | 8 MiB / 128 배치 중 먼저 | 데이터 credit 축소 + 로컬 `display.gap` |
| 제어 projection | 4,096건 / 4 MiB | **조용한 드롭 금지** → 재snapshot·STALLED |
| Activity · 테스트 증거 링 | 5,000행 · 2,000행 | 오래된 것 버림 |
- 데이터 예산 ≠ 탭 RSS. **24 h soak 목표**(UI 탭 포함, CORE soak에 합류): 1 h 워밍업 뒤 23 h 동안 renderer RSS 증가 ≤ 64 MiB, post-GC JS heap 증가 ≤ 16 MiB, DOM 노드·listener 평탄. 절대 RSS 초기 목표 512 MiB/주요 작업 공간(M0에서 3-OS 기준 머신으로 보정).
- 근거(**측정**, 30분 soak 계획 중 720 s 표본까지 저장 — 750 s에서 파일 시스템 정지(EPERM)로 프로세스 종료, 미완): 10k/s·2M행 고정 링에서 post-GC heap 2.24 → 4.58 MiB(300 s 이후 +1.33 MiB), ArrayBuffer 46.3 MiB 고정, DOM 16 노드, seq gap 0. **메모리 수용 근거로는 부족하다**(R2 opus §2.3): 링을 미리 할당해 힙 증가가 작게 나오는 구조이고, 강제 GC 표본이라 자연 GC 피크·renderer RSS·GPU(xterm WebGL 아틀라스)·수명주기 누수가 빠졌다.
- 대체 측정(M0, 10분 규칙 안): **10분 압축 soak** = 실제 패널 구성(xterm + Trace + UDS 표 + 플롯) · CAN 10k/s + UART flood · 패널 열기/닫기 100회 · 재연결 20회 · 숨김/보임 전환, 시작·끝의 renderer RSS(CDP `SystemInfo.getProcessInfo` 또는 OS `ps`)·post-GC heap·DOM·listener 수 비교. 24 h는 전용 runner의 CORE soak 관문에 UI 탭 합류.

---

## 7. 라이브러리 선택과 근거

조사일 2026-11-10, npm `dist-tags.latest` 안정판 기준. 버전·라이선스·의존성 원본: `spikes/ui/sol/{npm,compat,additional,crates}-evidence.json`, 1차 소스 사본 `spikes/ui/sol/evidence/`. 원칙: 런타임 CDN 0, 라이선스 MIT/Apache/BSD만(GPL 계열 금지 — 단일 바이너리 내장), 버전 고정 + lockfile.

| 영역 | 결정 | 버전·라이선스 | 근거 | 대안·전환 조건 |
|---|---|---|---|---|
| 프레임워크 | **React 19 + 외부 스토어**(`useSyncExternalStore`), 상태 라이브러리 필수 아님 | react 19.3.1 MIT | 고빈도 경로는 프레임워크 밖(Worker 링 + 명령형 렌더러)이라 반응성 모델 차이가 지연을 좌우하지 않음. 도킹(dockview-react·FlexLayout)·가상화·접근성 생태계 직결, 구현 에이전트·유지 인력의 숙련도. 번들 69 KB gzip은 로컬 내장이라 무관 | SolidJS 1.9.18(기술상 유력, 통합 비용) — UD-01 |
| Trace 그리드 | **M1 DOM 가상화(TanStack Virtual) + 고정 행높이**, 렌더러 교체 가능 구조 | @tanstack/react-virtual 3.14.26 MIT | S1: 10k/s에서 DOM·canvas 지연 동일(p95 33 ms), draw p95 0.5 ms. DOM이 선택·복사·접근성·IME에 유리 | 자체 canvas — 동일 시나리오 full schema에서 DOM 예산 초과 재현 시. Glide Data Grid 6.0.3은 React 19 peer 미지원으로 보류 |
| 플롯 | **uPlot + 서버 min/max 질의** | uplot 1.6.32 MIT | 다축·커서 동기·missing data, Canvas2D. 원본은 서버, `signal.query{t0,t1,px,mode:minmax}` → series당 ≤ min(4×px, 20000) | webgl-plot 1.1.2 — 16신호 pan/zoom ≥ 30 fps 실패 재현 시 |
| 터미널 | **xterm.js 6 + WebGL addon, 실패·context lost 시 DOM fallback** | @xterm/xterm 6.0.0, addon-webgl 0.19.0 MIT | S2: 4.2 MB 쓰기 DOM 7.0 / WebGL 7.0 MiB/s(같음). addon-canvas 0.7.0은 xterm ^5 peer라 제외(xterm 6에서 canvas 렌더러 제거) | — |
| 도킹 | **M1 고정 split → M2 dockview** | dockview 8.9.0 MIT | `toJSON/fromJSON`·프레임워크 독립 core·무료 범위 split·tab·floating | flexlayout-react 0.11.1 MIT. Enterprise 기능은 무료라 약속 안 함 |
| 스키마 생성 | schemars → JSON Schema → json-schema-to-typescript + Ajv | 1.2.3 / 16.0.0 / Ajv 8.20.0 | §6.3 probe | ts-rs·specta 제외 |
| 정적 내장 | CORE 표준(`rust-embed` 8.13.0) | MIT | CORE 소유 | include_dir 0.7.4 |
| 존재 확인 | WebAuthn(브라우저 내장) | — | §10.3 | 대역 외 코드 |
| E2E·계측 | Playwright(브라우저는 프로젝트 밖 `~/.cache`) | 1.65.0 Apache-2.0 | CDP heap·DOM 관찰 | — |
| 번들러 | Vite(개발 서버 + 프로덕션 빌드) | [버전 M0 고정] | — | esbuild 단독 |
| 데스크톱(M4) | Tauri 2 | 2.12.5 MIT/Apache-2.0 | §3.8 | — |

---

## 8. HW / OS 지원 매트릭스

| 항목 | Windows x64 (벤치) | Linux x64 (벤치) | macOS (개발·원격 보기) |
|---|---|---|---|
| 브라우저 1급(성능 기준) | Chrome / Edge | Chrome / Chromium | Chrome |
| 브라우저 2급(기능 보장) | Firefox | Firefox | Safari·Firefox |
| 존재 확인(M2) | Windows Hello [미확인: 합성 입력 차단 동작] | **플랫폼 인증기 없음** → USB 보안 키, 없으면 대역 외 코드 | Touch ID [미확인] |
| 원격 보기 | — | — | `ssh -L 7420:localhost:7420 bench` + `loom ui pair` |
| 데몬이 브라우저 열기 | ShellExecute | `xdg-open`(GUI 세션 없으면 URL만) | `open` |
| Tauri(M4) | WebView2 | WebKitGTK (성능 위험) | WKWebView |
| CI | Playwright headless Chromium — 3 OS 모두 기능·정직성 시험. 지연 수용은 고정 runner에서만 | | |
- 장치 지원(PCAN FD·VN5640·SocketCAN·시리얼)은 도메인 팀 실HW 관문. UI는 backend·env·time source·generation·관측 가능 여부를 표시할 뿐이다.
- 이번 S1/S2 수치(macOS M1 headless Chromium)로 Windows/Linux·WebView 성능을 보장하지 않는다 — M0에서 Windows·Linux 기준 머신 측정.

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 단계별 백엔드
1. **UI-M0: CORE fixture replay stub gateway**(CORE P0 후반 산출물, WS 서버 모드)를 1순위로 쓴다. 늦어지면 UI 팀 **`loom-mock`**(TS, `ui/tools/mock`) — CORE 스키마 fixture JSONL을 시나리오대로 재생하는 WS 서버 + 쓰기 RPC 스크립트 응답 + 부하 생성(CAN 10k fps, UART 92 KB/s, UDS 0x78 연속) + gap·stall·disconnect·제어 큐 포화 주입.
2. **UI-M1: 실 `loomd` + sim worker**(CORE P1) — 같은 시험을 실데몬으로 다시. mock과 실데몬 둘 다 green이어야 M1.

### 9.2 시험 계층
| 계층 | 내용 | 도구 |
|---|---|---|
| 단위 | 스토어·RowStore 단조성·ClockSync·decoder·키 매핑·의도 게이트 | Vitest |
| 코덱·전송 | JSON/columns 동일 golden fixture, 잘린 frame·oversize·unknown schema·malformed offset → 거부·크래시 없음 | Vitest + fuzz |
| snapshot | join 중 RX·part 중 disconnect·epoch 변경 → 중복/누락 0 또는 명시 gap | Playwright + mock |
| 패널 계약 | manifest 위반 throw, ESLint 금지 규칙, CSP·Permissions-Policy 헤더 존재 | Vitest + lint |
| E2E 핵심 루프 | E-UI-1/2/3 자동화(에이전트 = CLI 또는 RPC 클라이언트, 사용자 = Playwright). **DOM의 `data-request-id`·출처·행 상태로 확인**(WS 수신만으로 성공 처리 금지) | Playwright |
| 지연 | 렌더 보고 + DOM/캔버스 관찰로 p50/p95/p99. 성능 판정은 고정 runner만, CI는 논리 판정 | Playwright + 하네스 |
| 정직성 | 부하·드롭 주입 → 생략 행 count = 서버 gap count, `LIVE-DEGRADED`·`STALLED`가 **실제로 화면에 뜨는지**, 3층 손실 각각 | Playwright + mock |
| 우선 제어 | 데이터 큐 포화 중 UDS 최종·승인·STOP이 묻히지 않음, 포화 disconnect면 명시 | Playwright + mock |
| 보안 | DA·DSR·DECRQSS·OSC 4/10/11/12 `?` 색 질의·DCS·XSS payload·Origin 위조 → 의도 없는 TX 0건, OSC 8 링크 비활성, clipboard addon 미로드, 권한 우회 없음 | Playwright |
| 안전 UX | 승인 무장 지연·기본 포커스 거부·Enter 비승인, watch로 쓰기 불가(서버도 거부), STOP 1 프레임, bootstrap 비밀 비노출 | Playwright |
| 수명주기 | 패널 100회 열기/닫기·도킹·hidden→visible → 구독·listener·Worker·GPU 자원 누수 없음 | Playwright + CDP |
| 배포 | 네트워크 차단·빈 캐시·단일 바이너리 → 모든 자산 로컬 | Playwright |
| soak | 24 h UI 포함(§6.5) — CORE soak 관문 합류 | 기준 머신 |
| 시각 회귀 | §5.1 와이어프레임 상태별 스크린샷 | Playwright |

### 9.3 M1 전에 다시 잴 것 (스파이크 S1이 UI-T03/T05/T10을 대신하지 못하는 부분, R2 opus §2)
| # | 측정 | S1의 한계 |
|---|---|---|
| P1 | **full D3 envelope(+CAN FD 64 B)** JSON vs `loom.columns.v1`, 10k/s 60 s | S1 행은 53 B, 실제는 ≈ 540 B 추정(10배) |
| P2 | **Worker 디코드 + Transferable** 경로로 같은 시험 | S1은 메인 스레드 디코드 |
| P3 | 생성기를 "독립 이벤트 발생 → 서버 적응형 배치"로 → 장치시각→렌더 | S1은 flush 때 행 생성(배치 대기 제외) |
| P4 | CORE stub 또는 작은 Rust 서버의 `core.time` d1/d2로 오프셋·오차폭 | S1은 같은 호스트 epoch |
| P5 | UART 폭주 + CAN 10k/s + UDS 진행 행 **동시** 3패널 패널별 p95 | S1 단독 |
| P6 | 사용자 입력 지연: keydown→`uart.input` 서버 수신, Enter→`uds.tx` | 미측정 |
| P7 | 정직성: 서버발 `display.gap`·`record.gap`·`source.gap` 각각, 데이터 포화 중 제어 표시 ≤ 1 프레임, STALLED·LAG·숨김 복귀 — **백그라운드 throttle을 끄는 플래그 없이** | S1의 gap은 클라이언트 자체 생성 |
| P8 | 10분 압축 soak(§6.5) | S1 soak 미완·사전 할당 |
| P9 | 고정 runner headed + CDP Tracing으로 렌더 proxy 오차 1회 보정 | proxy만 |
P1–P6은 UI-M0의 스파이크 작업(UI-S2), P7–P8은 UI-11 수용 시험에 포함.

### 9.4 실행 규칙 (재부팅 교훈)
- 개발 중 벤치마크·soak 스파이크는 **10분 이내**. 24 h soak는 CI/전용 머신에서만.
- Playwright 브라우저·npm 캐시는 **프로젝트 폴더 밖**(`~/.cache/loom-ui/`). 생성 결과 파일은 주기적 덮어쓰기 대신 append + 회전.
- 실HW 타이밍 관문(UART 115200/921600, PCAN FD, VN5640 CAN FD, DoIP)은 도메인 관문과 병행.

### 9.5 지연 측정 방법
- 렌더 보고(§3.6)와 시험 하네스 표본을 서버 수신 시각과 교차 확인. 브라우저 내부 값만으로 판정하지 않는다.
- 각 표본에 `stage`·seq 범위·`clock_err_ms`. 시계 불확실성이 목표보다 크면 "동기 불확실"로 보고(합격 처리 안 함).
- seed·rate·payload·CPU 제한·배치 설정·브라우저 플래그·가시성 상태를 artifact에 기록.

---

## 10. 안전 / 가드레일 (UI 측)

### 10.1 원칙
1. **안전 방향은 쉽게, 위험 방향은 어렵게**: STOP·비상 OFF·거부·중단 = 확인 없음, 모든 자격. 송신·승인·완화·해제 = 자격 + 확인.
2. **거짓 성공 금지**: 성공 색은 `confirmed` 이후, `outcome_unknown`은 실패와 다른 색, gap은 "수신 없음"처럼 보이지 않게, STOP은 서버 확인 전 "요청 중".
3. **장치 출력은 신뢰하지 않는 데이터**: 항상 textContent, 터미널 3중 방어, spec 패널에 HTML 금지.
4. **UI는 서버 정책을 대신하지 않는다**: UI 확인은 실수 방지일 뿐, 판정은 데몬 정책(D6). disabled 버튼은 설명용.
5. 자동 재연결은 관찰만 복구한다. mutation·주기 송신·승인·잡을 자동 재개하지 않는다.

### 10.2 인증·웹 보안
- bootstrap: fragment → POST 교환, 1회·60 s, Referrer-Policy no-referrer, URL·로그 redaction(CORE 합의). 쿠키 HttpOnly·SameSite=Strict(HTTPS면 Secure).
- Host/Origin allowlist를 정적·WS 업그레이드·HTTP artifact 전부에 적용(DNS rebinding·CSRF), `frame-ancestors 'none'`.
- agent token은 viewer로 승격되지 않는다. 요청의 origin·actor 필드는 무시(서버가 연결 자격으로 결정).
- 비밀값: `[secret 12 B]`, 승인 토큰 원문 미표시, seed/key 마스킹(UDS §6.1). `raw_ref`는 불투명 artifact ID(임의 파일 경로 접근 금지).
- 붙여넣기 4 KiB 초과(UART) → 검토 모달.

### 10.3 에이전트 자기 승인 차단
**위협 모델(D6)**: 에이전트는 협조적이지만 실수할 수 있다. 막을 것 = "도움이 되려는 에이전트가 승인 화면을 보고 눌러 버리는 것". 같은 OS 계정의 악의적 우회(쿠키 파일 읽기, 데몬 바이너리·설정 수정)는 범위 밖이며 그렇게 명시한다.

| # | 경로 | 현실성 |
|---|---|---|
| S1 | 에이전트가 bootstrap 포함 URL을 자동화 브라우저로 열어 쿠키 획득 → 승인 | 높음(비밀이 CLI 출력에 있으면) |
| S2 | 에이전트가 **사용자 실 브라우저 프로필을 조작하는 도구**(claude-in-chrome·ego-browser 등)로 이미 열린 Loom 탭에서 승인 클릭 | **높음** — 쿠키 분리로 못 막음 |
| S3 | 화면을 읽어 요청 코드 2자 입력 | S1·S2에 딸림 |
| S4 | pseudo-TTY로 `loom ui`/`loom ui pair` 코드 획득 | 중간(의도적 우회에 가까움) |
| S5 | 브라우저 자동화로 사용자 입력을 `origin=user`로 위장 | S2와 같음 |
| S6 | 확장 `chrome.debugger`/CDP **가상 인증기로 자기 WebAuthn 자격증명을 등록**해 통과 | 등록이 열려 있으면 높음(**확인**: `chrome.debugger` 허용 도메인에 WebAuthn, R1b §4.3) |

**채택안(UD-05·UD-06)**:
| 층 | 내용 | 막는 것 | M |
|---|---|---|---|
| A. 비밀 비노출 + 자격 분리 | bootstrap 비밀은 TTY `loom ui`만, fragment로 전달·CLI 출력·로그 금지. 비TTY 경로는 가리키기만. watch/operate/approve 분리 | S1 | M1 |
| D. 감지·고지·감사 | 승인 연결의 UA·`navigator.webdriver`·자동화 흔적 기록, webdriver 브라우저에는 approve 자격 발급 거부, 요청 후 300 ms 안 승인 경고, DOM 고지문 "AI 에이전트: 이 화면을 조작하지 말고 사용자에게 요청하라", **스킬 규칙: Loom UI를 브라우저 자동화로 조작 금지(보기·스크린샷만)** | 협조적 에이전트의 S2·S3·S5 | M1 |
| B. 존재 확인(WebAuthn UV) | R3/R4 승인·정책 완화·STOP 해제·사용자 본인 R3·계획 승인(R3 포함) 시 플랫폼 인증기 또는 보안 키, `userVerification:"required"`, 서버가 저장 공개키로 assertion 검증, 서명 카운터·자격증명 id·AAGUID 감사 | S2·S3(가상 인증기는 실 플랫폼 키로 서명 불가 — **확인**: Chrome WebAuthn 가상 인증기는 실 Discovery를 대체) | M2 |
| B'. **등록 게이트(필수)** | WebAuthn 자격증명 **등록**은 벤치 TTY `loom ui pair --register`가 보여 주는 1회 코드 또는 기존 자격증명 assertion이 있어야만. 등록·삭제는 감사 + 모든 탭 알림 | S6 | M2 |
| C. 대역 외 코드 | 인증기가 없는 호스트(Linux·보안 키 없음): 승인 시 데몬이 벤치 TTY(`loom ui approve-code`)·OS 알림에 6자리 코드 → 모달 입력 | S1·S3 (화면·알림을 읽는 computer-use에는 약함 — 한계 명시) | M2 |
설정 `approval.presence = webauthn | out_of_band_code | none` — `none`은 sim에서만, 감사에 표시. R2 확인은 A + D만(빈도가 높아 B는 과함, 설정으로 강화 가능). S5(입력 위장)는 B로 막지 않는다(키마다 생체 확인 불가) — D와 감사로 다루고 한계를 문서화.
CORE 승인 객체는 `presence{method: webauthn|oob|code, proof}`로 일반화 요청(CORE의 `presence_code`는 화면 표시 코드라 오클릭 방지용으로만, CR-UI-07).
**스파이크 S-O1(M2 전)**: (1) `http://localhost`에서 Chrome/Edge/Firefox 등록·UV, (2) 등록 게이트 상태에서 Playwright·확장 `chrome.debugger` 가상 인증기로 승인이 거부되는지, (3) macOS Touch ID 시트·Windows Hello 대화상자에 합성 입력이 들어가는지, (4) Linux 보안 키·GPM PIN 경로. 결과에 따라 Linux 기본값 확정.

### 10.4 기타 UI 안전 규칙
- 승인 카드: §5.1.5 규칙.
- env=vehicle: 상단 테두리 강조 + 승인 카드·송신 명령줄에 `VEHICLE` 표시(규칙은 bench와 같음, B4).
- STOP 후 finally 자동 실행 0, 복구 송신은 user 출처·인터록 통과(A15).
- 기록·감사 불능 시 agent·script 새 R1+만 차단되고 사용자 입력은 허용 + `REC FAILED` 배너(A12). 테스트 화면은 PASS 판정 불가 표시.

---

## 11. 마일스톤

| M | 단계 | 내용 | 수용 기준 |
|---|---|---|---|
| **UI-M0** | P0 (CORE stub 병행) | `ui/` 골격(Vite·React·lint·CSP), stub gateway 또는 `loom-mock`, Net Worker·스토어·RenderScheduler·ClockSync, 스키마 TS 생성·drift CI, 3-OS 기준 머신 지연·메모리 기준선 | mock 위 UART·CAN·UDS fixture 재생 렌더, drift CI green, Windows·Linux 기준 머신 S1 재측정 |
| **UI-M1 걷는 뼈대** | P1 관문 | 셸(상단 바·채널 바·STOP·STOP 상태·복구 송신·Activity·고정 레이아웃·접두키) + UART 터미널 + CAN Trace(Chrono·raw·송신줄) + UDS 콘솔(표·명령줄·제어 가져오기·R2 카드·마지막 값) + 승인 대기열(R2) + 인증(A+D) + 렌더 보고 | **아래 UI-T01–T10 전부 green (mock + 실 loomd sim)** |
| **UI-M1b** | TEST M1과 함께 | `shell.test` 최소판(계획 검토 모달·일괄 승인·단계·실시간 증거·검증 3항목·판정 수정·질문·간이 단계 띠·보고서 링크) + `psu.gauge`(sim, 비상 OFF) + (선택, 도메인 준비 시) `dlt.trace`·`la.capture` 상태·디코드 표 | `_xteam/ui-test.md` 수용 시험 5개 green(sim TC-1). 선택 패널은 수용 기준 밖 |
| UI-M2 | P2 | R3/R4 승인 모달·존재 확인(B·B'·C)·S-O1, 도킹·레이아웃 저장, 가리키기 토스트, binary 전송, CAN Fixed·IG·Data·Graphics 기본, UDS DTC·세션·overrides·jobs·recover, UART 트레이스·hex·Keyboard Lock, DLT·LA 패널, 탭 간 writer 탈취 | 자격별 승인 E2E, **자기 승인 차단 시험**(Playwright·가상 인증기로 approve 불가), full D3·FD 10k/s Trace p95 ≤ 150 ms, 16신호 pan/zoom ≥ 30 fps, 24 h soak(UI 포함) |
| UI-M3 | P3 | Graphics 고급·`loom.viz`(파일·라이브)·spec 패널·통합 타임라인·기록 탐색·테스트 화면 고도화·보고서 뷰 | viz 요구 V-UDS-1/2/4·V-UART-1/3/4 화면 시험, 증거 → 기록 구간 HISTORY 이동 |
| UI-M4 | — | Statistics, Tauri(선택), 원격 TLS, 영어, 팝아웃 | WebView2/WebKitGTK 수용 별도 |
| UI-M5 | — | Replay 블록, PNG export | — |

**UI-M1 수용 기준 (모두 자동 시험)**
| ID | 기준 |
|---|---|
| UI-T01 | 늦은 합류: 에이전트가 1,000 이벤트를 보낸 뒤 탭을 열면 1 s(UART 500 ms) 안에 snapshot 표시, 이후 live와 seq 중복·누락 0 |
| UI-T02 | 에이전트 `uart send` / `can send` / `uds read-did` 결과의 seq·request_id가 같은 패널 DOM 행(`data-request-id`)에 존재, 출처 AGT |
| UI-T03 | 지연(고정 runner, 정상 부하): UI 구간 p95 UART ≤ 45 · UDS ≤ 35 · CAN ≤ 50 ms. 종단 UART 100 / UDS 100(p99 200) / CAN 150(p99 300) ms. UDS tx→행 ≤ 50 ms, 사용자 Enter→`uds.tx` ≤ 50 ms, UART keydown→`sent` p95 < 20 ms(비부하) / **921600 폭주 중 Ctrl-C keydown→`sent` p95 < 100 ms** |
| UI-T04 | 사용자 개입 3종: UART 키(칩 `나`) → 에이전트 send 대기 → Ctrl-C → 에이전트 결과 `others_since_last.count ≥ 1` + 행 `⇢ 전달됨`; CAN 1회 송신 USR 행; UDS 명령줄 요청 + [제어 가져오기] → 에이전트 exit 6 |
| UI-T05 | 정직성: mock 구독 드롭 주입 → 생략 행 count = 서버 gap count + `LIVE-DEGRADED`; heartbeat 중단 3 s → `STALLED` + 쓰기 잠금; 재연결 후 이음; 3층 손실 문구 구분; 정상 부하 드롭 0; **UART flood(≈ 92 KB/s) 60 s: view skip 0·heartbeat 1 Hz 연속·메인 스레드 멈춤 없음, 넘치면 gap seq 정확** |
| UI-T06 | STOP: 클릭 → 1 프레임 안 띠, 서버 latch 확인 표시, 이후 에이전트 쓰기 exit 6, 사용자 UART 키 통과, 복구 송신 1키 실행(user 출처), WS 끊김 중 HTTP 대체 STOP 동작 |
| UI-T07 | 불변식·보안: 패널 lint 위반 fixture가 CI 실패, CSP·Permissions-Policy 헤더 존재, DA·DSR·DECRQSS·OSC 4/10/11/12 `?` 질의 fixture에서 의도 없는 TX 0건, OSC 8 링크 비활성, clipboard addon 미로드 확인, readonly 탭 자동 응답 0건 |
| UI-T08 | 자격: watch 탭 송신·입력 비활성 + 서버도 거부(직접 RPC 시험), watch로 STOP 가능; 비TTY `loom ui` 출력·데몬 로그에 bootstrap 비밀 없음; R2 카드 세 경로(token·viewer·auto) |
| UI-T09 | 쓰기 단계: UDS 0x78 ×5에서 한 행이 `○→◔→◑ pending×1..5→●`, 행 수 1; 역순 주입 시 단계 후퇴 없음; `outcome_unknown` 주황 |
| UI-T10 | 메모리·수명주기(10분 이내 개발 시험): CAN 5k fps + UART flood에서 데이터 예산 초과 없음·post-GC heap 기울기 평탄; 패널 100회 열기/닫기 후 listener·Worker 포트 원상 |

---

## 12. 구현 단계 작업 분해

| ID | 작업 | 담당 | 의존 | M |
|---|---|---|---|---|
| UI-00 | CORE DTO·RPC·snapshot·clock·loss fixture 합의 + 스키마 TS 생성·drift CI·Ajv 경계 검증 | Sol + CORE | CORE P0 스키마 export | M0 |
| UI-S2 | M1 전 재측정 스파이크 P1–P6(§9.3), 각 ≤ 10분, Windows·Linux 기준 머신 포함 | Sol | UI-03, CORE stub | M0 |
| UI-01 | `ui/` 골격(Vite·React·TS·ESLint 금지 규칙·CSP/Permissions-Policy 헤더 목록) | Sol | — | M0 |
| UI-02 | stub gateway 연동 / `loom-mock`(fixture 재생·부하·gap/stall/포화 주입·쓰기 스크립트) | Sol | UI-00 | M0 |
| UI-03 | Net Worker(RpcClient·SubscriptionMux·Decoder·ClockSync·제어 분류·도메인 링) | Sol | UI-02 | M0 |
| UI-04 | 메인 스토어·RowStore/WriteTracker·RenderScheduler·ShellControlStore | Opus | UI-03 | M0 |
| UI-05 | `@loom/panel-sdk`(registerPanel·PanelContext·ActionGate·manifest 검증)·PanelHost 수명주기 | Opus | UI-04 | M0 |
| UI-06 | 셸: 상단 바·채널 바·env·STOP·STOP 상태·복구 송신·Activity·고정 레이아웃·접두키·연결/장애 배너 | Opus | UI-05, CORE STOP·safe_state | M1 |
| UI-07 | 인증 화면(`/bootstrap` fragment 교환·`/pair`)·자격 표시·고지문·webdriver 감지(서버 쪽 CORE) | Opus | CORE D2 | M1 |
| UI-08 | UART 터미널(xterm·의도 게이트·parser hook·입력권 칩·gap 장식·키 팔레트·트레이스 최소) | Sol + UART | UI-05, UART 명세·`loom-console` | M1 |
| UI-09 | CAN Trace(TanStack 가상화·Worker 링 projection·생략 행·송신줄·읽은 범위 막대) | Sol + CAN | UI-05 | M1 |
| UI-10 | UDS 콘솔(RowStore 표·대상 바·명령줄 dry-run 미리보기·제어 가져오기·마지막 값) + 승인 대기열·R2 카드 | Opus + UDS | UI-05, UDS 이벤트 스키마 | M1 |
| UI-11 | 렌더 보고·지연 하네스·정직성·보안·E2E E-UI-1/2/3(UI-T01–T10) | Sol | UI-06–10 | M1 |
| UI-12 | `shell.test` 최소판 + `psu.gauge` | Opus + TEST | TEST/CORE test.* 스키마, UI-06 | M1b |
| UI-13 | R3/R4 모달·WebAuthn 등록 게이트·대역 외 코드·S-O1 스파이크 | Opus + CORE | CORE 승인 서비스 P2 | M2 |
| UI-14 | 도킹(dockview)·레이아웃 저장/migration·가리키기 토스트 | Opus | UI-06, CORE `core.ui.*` | M2 |
| UI-15 | binary `loom.columns.v1` decoder + golden/fuzz + full D3·FD 재측정 | Sol + CORE | CORE 열 프레임 | M2 |
| UI-16 | CAN Fixed·IG·Data·Graphics 기본(uPlot·min/max 질의) | Sol + CAN | UI-15, CAN 질의 RPC | M2 |
| UI-17 | UDS DTC·세션·overrides·jobs·recover, UART 트레이스·hex·Keyboard Lock | Sol + 도메인 | 도메인 명세 | M2 |
| UI-18 | DLT·LA 패널 | Sol + EXPLORE | EXPLORE P2 | M2 |
| UI-19 | 24 h soak(UI 포함)·RSS·3-OS 기준 머신·실HW 키 지연 | Sol + CORE | UI-11–18 | M2 |
| UI-20 | Graphics 고급·`loom.viz` 렌더(파일·라이브)·spec 패널 렌더러·통합 타임라인·기록 탐색 | Sol(렌더) + Opus(spec 계약) | UI-22, CORE replay | M3 |
| UI-22 | Rust `loom-viz` crate: 데이터셋 등록·CSV/JSONL/Parquet ingest·min/max 피라미드·`signal.query`·`loom viz` CLI 동사, 도메인 형식 어댑터 연결(CAN BLF/ASC/MF4/TRC) | Sol + CORE·CAN | CORE 저널·질의 API | M3 |
| UI-21 | 테스트 화면 고도화·보고서 뷰 | Opus + TEST | UI-12 | M3 |
임계 경로: CORE 스키마 fixture → UI-00/02/03 → UI-04/05 → UI-06/08/09/10 → UI-11 → P1 관문. UI-08/09/10은 UI-05 뒤 병렬.

---

## 13. 리스크
| 리스크 | 영향 | 대응 |
|---|---|---|
| CORE stub·스키마 지연 | UI-M0 정지 | `loom-mock` + 임시 fixture, 스키마 확정 시 생성 파이프라인으로 교체 |
| 합성 스파이크 수치의 과잉 일반화 | full D3·FD·다중 패널에서 목표 실패 | M0 Windows·Linux 기준 머신 재측정, M2 전 full schema 재측정, 렌더러 교체 가능 구조 |
| rAF를 실제 paint로 오인 | 종단 지연 과소평가 | stage 구분·headed trace 보정·clock 불확실성 표시 |
| 메인 스레드 정체·TCP HoL로 제어 이벤트 지연 | 승인·STOP 표시 늦음 | 프레임 예산, 제어 경로 분리, 서버 작은 배치·credit, STOP HTTP 대체 경로 |
| 브라우저가 터미널 키 가로챔 | 터미널 수준 UX 미달 | 키 팔레트·대체 키, Keyboard Lock(M2), 실측 후 Tauri(M4) |
| xterm 자동 응답·OSC 52 | 의도 없는 UART 쓰기·클립보드 | 3중 방어 + 악성 fixture 시험 |
| 브라우저 자동화로 자기 승인·입력 위장 | 안전 통제 약화 | §10.3 A·D(M1) + B·B'·C(M2), 스킬 규칙, 한계 명시 |
| WebAuthn localhost·Linux 제약 | 존재 확인 약화 | `localhost` 정본 오리진, S-O1, 대역 외 코드 대체 |
| 오래 켠 탭 메모리 증가 | C2 위반 | 모든 링 바이트 상한, soak에 UI 포함, 수명주기 시험 |
| 패널 기여가 불변식 우회 | 숨은 경로 | API·lint·CSP·리뷰 4층 |
| 도메인 명세·이벤트 이름 불일치 | 패널 재작업 | `request_id`+`write_state` 필수화, P0 스키마에서 이름 단일화(CR-UI-09) |
| 프레임워크 선택 이견(React vs Solid) | 재작성 비용 | 고빈도 경로를 프레임워크 밖에 두어 교체 비용을 셸·폼으로 한정 |
| 개발 머신 파일 IO 정지 재발 | 작업 중단 | 벤치 10분 이내·캐시 프로젝트 밖(§9.4) |

---

## 14. 사용자에게 물어볼 질문 (기본값으로 진행)
| # | 질문 | 기본값 |
|---|---|---|
| Q1 | UI를 주로 벤치 PC 화면에서 보나, 다른 PC(macOS)에서 원격으로 보나? | 둘 다. 원격은 SSH 포워딩 |
| Q2 | R3/R4 승인 때 Touch ID·Windows Hello·USB 보안 키 확인을 써도 되나? Linux 벤치에 보안 키가 있나? | 가능하면 사용, 없으면 벤치 터미널 6자리 코드 |
| Q3 | 에이전트가 화면을 "가리킬" 때 자동 전환 / 묻기? | 묻기 |
| Q4 | 터미널(herdr)에서만 일할 때도 승인은 웹에서 해도 되나? | 예(TUI 승인 화면 없음) |
| Q5 | 주 브라우저는? | Chromium 계열(Chrome/Edge) |
| Q6 | 화면 언어 | 한국어(영어 M4) |
| Q7 | 한 화면에 동시에 보는 채널·패널 규모 | UART 2 · CAN 2 · UDS 대상 20 · 플롯 16신호 (부하 프로파일 기준) |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 CORE에 요청 (`_xteam/core-ui.md` v0.2로 정리)
| ID | 요청 | CORE 현황 | 시점 |
|---|---|---|---|
| CR-UI-01 | JSON 기본 + 데이터 구독 binary capability(`loom.columns.v1`), M2 **초반** 확정 | 수용(M2 전) | P0 capability 예약 |
| CR-UI-02 | 서버 데이터 credit·구독별 유한 큐·작은 배치 사이 제어 우선 | 수용(512 KiB/8배치, full schema로 조정) | M1 |
| CR-UI-03 | `core.time` 응답에 d1/d2 mono 둘 다 + clock_id | core-sol 수용·core-opus 미반영 | P0 |
| CR-UI-04 | `core.metrics.report{kind:"ui.render"…}` 단일 스키마, 도메인 별칭 폐기 | 이름 제안 일치 | P0 |
| CR-UI-05 | 제어 큐 포화 시 disconnect + 재snapshot(조용한 드롭 금지) | 수용 | M1 |
| CR-UI-06 | replay 쓰기 서버 거부 | 수용 | replay 전 |
| CR-UI-07 | viewer rights(watch/operate/approve), 승인 `presence{method, proof}` 일반화, WebAuthn 등록 게이트·검증, 감사에 승인 방법·UA·webdriver | 부분(presence_code·viewer-tty role) | M1 rights / M2 presence |
| CR-UI-08 | STOP·비상 OFF·safe_state 인터록 예외를 정책 표로 명시 + `core.latch` payload(정지 잡·취소 승인·취소 불가 수·outputs_held) + HTTP `POST /api/v1/stop` | 부분 | P0 정책 표 / M1 |
| CR-UI-09 | bootstrap·stop·time·metrics·RPC 이름 P0 스키마에서 단일화 | 동의 | schema v1 |
| CR-UI-10 | 스키마 export + offline dist 내장(rust-embed)·CSP·`Permissions-Policy`·`localhost` 정본 리디렉트 | 부분(헤더·localhost 미언급) | P0 + M1 |
| CR-UI-11 | `core.agent_cursor` 이벤트, `core.activity`에 읽은 범위·술어 | 언급 없음 | M1 |
| CR-UI-12 | `core.safe_state.run{ids[]}` 묶음 실행 | 부분(list만) | M1 |
| CR-UI-13 | snapshot·리듀서의 공통 `control` 뷰 | 부분 | M1 |
| CR-UI-14 | P0 fixture replay stub gateway를 **WS 서버 모드**로 + fixture에 고부하·gap·`outcome_unknown`·late | 수용(형태 확인 필요) | P0 |
| CR-UI-15 | 24 h soak에 UI 탭 포함 | 미정 | M2 |
| CR-UI-16 | `core.ui.layout.*`, `core.replay.open` | 부분 | M2 / M3 |
| CR-UI-17 | 구독 상한 초과 탭 처리(읽기 전용 미러 또는 명시 거절) — UART 채널당 4·전체 16과 함께 | 미정 | M1 |
| CR-UI-18 | `loom-viz` crate가 쓸 저널 범위 질의·min/max 집계 API, 비TTY `loom ui`의 bootstrap 미발급 명시 | 미정 | M3 / M1 |

### 15.2 도메인 팀·TEST에 요청 (각 `_xteam/<comp>-ui.md`에 `## ui 의견`으로 회신 완료, 2026-11-10)
- **UART**: 다중 viewer writer 선점 규칙을 서버 계약 하나로(§5.2.2-6), 증거 REF를 TEST 형식(`uart:dut@seq:N`)으로, `loom-console` 정제기 출력 계약, 웹에서 쓸 수 없는 키 목록 확인.
- **CAN**: 출처 색 통일안(AGT 청록·USR 노랑) 동의 — UI가 렌더 소유이므로 이 안으로 확정 통보, 지연 회신 이름 `core.metrics.report`로, binary 열 필드 확정, `test.step_started/ended`(core-test O6).
- **UDS**: 시계 비교를 wall → `core.time` mono로, `approval.display` JSON Schema(일반 렌더러 입력), 명령줄 dry-run RPC 레이트, R3 코드 2자 + 웹 규칙(§5.1.5) 확인.
- **EXPLORE**: `psu.gauge` spec JSON, LA `la.lanes` 전이 보존 응답 형식, DLT CTRL 행 `request_id`+`write_state`.
- **TEST**: 이벤트 이름은 core-test O6 합의안을 UI가 그대로 구독(`test.session_started/ended`, `test.session_state`, `test.step_started/ended`, `test.mark`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask`, `test.approval`; RPC는 3토큰). `_xteam/ui-test.md`의 이름 정리 요청: `approval.grant` → `core.approval.respond`(묶음), `core.marker` → `core.mark`, `records.range` → `core.events.query`/HTTP, 요청의 `origin:user` 필드 삭제(서버 부여). 보고서 링크는 `loom://` 대신 `http://localhost:<port>/ui/#/records/<session>?seq=a..b`.
- **EXPLORE 이름 정리**: `job.progress` → `core.job_progress`, `la.capture.done` → `la.capture_done`(CORE kind 2토큰 규칙), `core.marker` → `core.mark`.

### 15.3 마스터 계약 변경 요청
| # | 대상 | 요청 |
|---|---|---|
| R-1 | D9 | 웹 비터미널 STOP 키 `Ctrl-X` 미채택. 웹 STOP = 버튼 + `Ctrl-] x` |
| R-2 | D2 | viewer role 안에 rights 3단(watch/operate/approve) |
| R-3 | D6 | R3/R4 승인에 존재 확인 기본(webauthn / 대역 외 코드, none은 sim만) + WebAuthn 등록 게이트 |
| R-4 | 출처 색 | AGT 청록·USR 노랑·DEV 회색·SCR 보라·EXT 보라 점선 전 팀 통일 |
| R-5 | D9/C6 | CAN `monitor`·UDS `view` Textual TUI 폐기, UART attach만 TUI 유지 |
| R-6 | D3 | 쓰기 이벤트는 kind와 무관하게 `request_id` + `payload.write_state` 필수 |
| R-7 | D2 | 웹 오리진 정본 `http://localhost:<port>`(WebAuthn RP ID), 127.0.0.1은 리디렉트 |

---

## 16. 결정 기록

| # | 쟁점 | 선택지 | 결정 | 이유 |
|---|---|---|---|---|
| UD-00 | v2.0 방향 전환 | — | UI 팀 신설, CAN의 웹·viz 소유 이관, M1 뷰어부터 웹(C6) | 사용자 지시 |
| UD-01 | 프레임워크 | React 19(sol) / SolidJS 등 미세 반응성(opus 성향) | **React 19 + 외부 스토어** | 고빈도 경로가 프레임워크 밖이라 반응성 이점이 지연을 좌우하지 않음(S1 렌더 proxy는 프레임워크 없이도 33 ms). 도킹·가상화·접근성 생태계, 유지 인력·구현 에이전트 숙련도. 번들 크기는 로컬 내장이라 무관. 교체 비용을 셸·폼으로 한정하는 구조 유지 |
| UD-02 | Trace 렌더러 | DOM 가상화 / canvas / Glide | **M1 DOM(TanStack Virtual)**, 교체 가능 구조 | S1에서 지연 동일, DOM이 선택·복사·접근성에 유리. Glide는 React 19 peer 미지원 |
| UD-03 | 전송 형식 | JSON / MessagePack / 열 binary / Arrow | **M1 JSON, M2 전 데이터 구독만 `loom.columns.v1` opt-in** | UI·CORE 두 스파이크 모두 지연 차이는 작고 대역·CPU·배치 여유가 이점. MessagePack은 이득 없음. Arrow는 대용량 질의용 |
| UD-04 | 스키마 → TS | schemars→JSON Schema→j-s-t-t / ts-rs / specta | **schema 경로 + Ajv 경계 검증** | C3 정본, probe에서 ts-rs optional 필드 오류 실측 |
| UD-05 | 웹 자격 발급 | TTY만(UDS) / 에이전트도 bootstrap 브라우저(CORE 초안) / 데몬이 열되 비밀 비노출(opus) | **TTY `loom ui`만 bootstrap(operate), 비TTY는 가리키기·안내만, `pair`로 원격(기본 watch)** | 에이전트가 화면을 띄워 주는 편의는 가리키기로 유지하면서 에이전트 경로로 쓰기 자격이 생기지 않게. 쿠키가 남아 사람은 처음 1회 |
| UD-06 | 존재 확인 | presence_code(CORE) / WebAuthn / 대역 외 | **`presence{method, proof}`, 기본 WebAuthn + 등록 게이트, 불가 시 대역 외 코드, 화면 코드는 오클릭 방지용만** | 화면 코드는 화면을 읽는 자동화가 통과, 등록이 열린 WebAuthn은 가상 인증기가 통과(R1b §4) |
| UD-07 | 출처 색 | CAN(AGT 주황·USR 파랑) / 다수안 | **다수안(AGT 청록·USR 노랑)** | UART·UDS·탐색·UI 두 초안 일치, 문자 병기라 CAN 전환 비용 최소, 렌더는 UI 소유 |
| UD-08 | 승인 입력 규칙 | 0.7 s(UART·opus) / 1 s + Y/N(UDS) | **1 s 무장, 기본 포커스 거부, Enter·Esc 비승인, 승인 = 버튼 또는 `Y`** | 웹은 마우스·키 반복이 섞여 TUI보다 오입력 위험이 크고 세 팀 규칙의 교집합 |
| UD-09 | 사용자 본인 R3/R4 | 셸 확인창 1회(CAN) / 대기열 본인 승인 | **요청+승인 합친 본인 확인 모달 1회 + 같은 존재 확인** | 자동화가 user로 위장해 R3를 스스로 확인하는 경로를 승인과 같은 수준으로 막음 |
| UD-10 | M1 승인 범위 | M1 승인 없음(opus) / R2 카드(UDS·CORE M1 R0–R2) | **M1 = 대기열 + R2 확인 카드, R3/R4·존재 확인 M2** | UDS M1 수용 시험과 CORE M1 범위가 R2 뷰어 경로를 요구 |
| UD-11 | 쓰기 행 키 | 도메인별 kind 개명 / 공통 필드 | **`request_id` + `payload.write_state` 필수, kind 자유** | UI 행 갱신에 충분하고 개명 비용 회피 |
| UD-12 | 테스트 화면 일정 | M3(opus R1) / M1b(sol·TEST) / M2(R1b) | **UI-M1b (TEST M1과 동시)** | TEST M1이 웹 화면으로 TC-1 시연을 요구. 걷는 뼈대(M1)를 키우지 않도록 별도 단계 |
| UD-13 | 렌더 상한 | 항상 20 Hz / rAF | **정상 rAF, 예산 초과 시만 20 Hz 강등** | CAN 150 ms 목표에서 20 Hz 대기(50 ms)를 아끼기 위해 |
| UD-14 | 메모리 상한 | 행 수 상한(opus) / 바이트 예산(sol) | **탭 데이터 예산 256 MiB 안에서 패널별 바이트·행 상한 병기** | 바이트 예산이 C2 판정 단위, 행 수는 UX 기본값 |
| UD-15 | 지연 보고 이름 | `core.render_ack` / `ui.latency_sample` / `core.ui.latency` / `core.metrics.report` | **`core.metrics.report{kind:"ui.render"}`** | CORE가 이미 제안, 이름 수를 늘리지 않음 |
| UD-16 | 복구 송신 API | `core.recover.*` / `core.safe_state.list` + 도메인 RPC | **`core.safe_state.list` + `core.safe_state.run{ids[]}`** | A15 "1키"는 여러 도메인 동작 묶음 실행 + 항목별 결과가 필요 |
| UD-17 | 가리키기 | `loom ui show`(opus) / `loom ui open <panel-ref>`(CORE·CAN) | **`loom ui open` 하나 + `core.ui_focus`, 기본 묻기** | 명령 하나로 스킬 단순화 |
| UD-18 | 웹 경로·오리진 | `/`·`/data`(opus) / `/ui/*`·`/api/v1/*`(CORE) | **경로는 CORE, 오리진은 `localhost` 정본** | 경로는 CORE 소유, 오리진은 WebAuthn RP ID 제약 |
| UD-19 | WS 1회용 티켓 | 유지 / 철회 | **철회** | 패널은 1st-party 정적 번들, lint·CSP로 충분 |
| UD-20 | 웹 STOP 키 | `Ctrl-X`(D9) / `Ctrl-] x` | **버튼 + `Ctrl-] x`** | `Ctrl-X` = 브라우저 잘라내기 |
| UD-21 | TUI | 도메인별 유지 / UART attach만 | **UART attach만**, CAN monitor·UDS view 폐기 요청 | C1(Rust)·C6(웹 1차)와 중복 |
| UD-22 | 스파이크 soak | 30분 재실행 / 720 s 결과 / 10분 압축 soak로 재정의 | **720 s 결과는 참고만, M0에서 10분 압축 soak(수명주기 churn + RSS)로 재정의** | 재부팅 공지의 10분 규칙, S1 soak는 사전 할당 링·강제 GC라 누수 탐지력 부족(R2 opus §2.3). 24 h는 전용 runner |
| UD-23 | STALLED 임계 | 1.5 s(core-opus) / 1 s(CAN) / 3 s(UDS·UI 초안) | **heartbeat 1 s, 1.5 s 회색 경고점, 3 s STALLED** | 탭 throttle·GC 순간 정지로 깜빡임 방지하면서 1.5 s에 이상을 알림 |
| UD-24 | 원본 링 위치 | 메인 스레드 스토어(opus) / Worker typed ring(sol) | **Worker 링 + 메인 미러(최근 N행) + projection 요청** | 메인 스레드를 디코드·보관에서 해방, 패널 API는 `StoreMirror`로 |
| UD-25 | xterm 위험 표면 | OSC 52 core 지원(sol R1) / addon 전용(R2 opus 소스 확인) | **clipboard addon 로드 금지 + OSC 4/10/11/12 색 질의 응답 차단 추가** | 설치된 6.0.0 소스의 OSC 등록 목록 확인 |
| UD-26 | 탐색 패널 일정 | 전부 M1(EXPLORE) / M2 | **`psu.gauge` M1b 필수, DLT·LA 상태·디코드 표는 도메인 준비 시 M1b, LA 레인 렌더러 M2** | TC-1을 닫으면서 걷는 뼈대를 키우지 않음 |
| UD-27 | STOP RPC 이름 | `core.stop_all`(core-sol·ui-sol) / `core.stop{scope}`(core-opus) | **CORE가 확정**(UI 추천 `core.stop{scope:"all"}` + CLI `loom stop-all`) | 범위 확장 여지. agent도 STOP 가능·해제는 viewer만은 세 초안 일치 |
| UD-28 | 리뷰 공정성 | ui-sol R2 미수행(샌드박스 경로 BLOCKED) | **리드가 r1-opus·r1b의 브라우저 동작 주장을 직접 대조**(§16.1) | Codex 워커가 재부팅 후 심볼릭 링크 cwd로 샌드박스 실행 불가 |
| UD-29 | `loom.viz` 처리 위치 | 브라우저 파싱 / 데몬 Rust 모듈 | **데몬 `loom-viz` crate(UI 팀 소유) + 도메인 형식 어댑터, 브라우저는 질의 결과만** | UI가 BLF/MF4를 파싱하지 않는다(§1.3), 10^7 점은 서버 축소, 에이전트 `viz summary`도 원본 기준 |
| UD-30 | 인증기 없는 호스트의 approve 자격 | 없음(R4 지적) / 대역 외 코드 경로 | **`loom ui pair --approve`(TTY) 1회 + 승인마다 대역 외 코드** | Linux 벤치에서도 R3/R4 승인·STOP 해제가 가능해야 함 |
| UD-31 | 새 채널 표시 | 사용자가 직접 열기 / 자동 | **채널 바 항상 추가 + 토스트 + `auto_open_panels` 기본 on** | 에이전트가 연 채널이 화면에 없으면 숨은 연결과 같다(02 원칙) |

### 16.1 리드 사실 확인 (ui-sol R2 대체, 설치된 `@xterm/xterm@6.0.0` 소스)
| 주장 | 결과 | 계획 반영 |
|---|---|---|
| OSC 52가 core에 있다(sol R1) | **아님** — `InputHandler.ts` 292–331행 OSC 등록 0·1·2·4·8·10·11·12·104·110·111·112 | clipboard addon 로드 금지(§5.1.2) |
| OSC 4/10/11/12 `?` 질의가 보고 문자열을 만든다(R2 opus F2) | 맞음 — `setOrReport*Color` 핸들러 | 차단 fixture 추가 |
| decoration API로 gap 장식(opus R1) | `registerDecoration`은 `_checkProposedApi()` 호출 → **`allowProposedApi:true` 필요한 제안 API**. `registerMarker`는 안정 | M1은 `allowProposedApi`를 켜고 버전 고정 + 실패 시 파서 리셋 지점 gap 줄 삽입(UART 명세 허용) 대체 경로. xterm 업그레이드 시 회귀 시험 |
| parser 훅으로 질의 응답 삼킴 | `terminal.parser.registerCsiHandler/OscHandler/DcsHandler`는 안정 API(`parser` getter에 proposed 검사 없음) | 3중 방어 ② 그대로 |
| 접두키 가로채기 | `attachCustomKeyEventHandler`(안정 API) + 셸 `window` capture 단계 | §5.2.1 |
| `Permissions-Policy: serial/usb/hid/bluetooth` 디렉티브·Keyboard Lock 지원 | [미확인] — 확인 수단(네트워크·브라우저 실험)이 이번 세션에 없음 | UI-T07 헤더 시험·M2 Keyboard Lock 시험에서 확인, 지원 안 하는 브라우저는 lint·CSP로 충분 |
| WebAuthn `localhost` RP ID·`chrome.debugger` WebAuthn 도메인 | R1b §4 1차 출처 인용(W3C, Chrome 문서) | S-O1에서 실측 |

### 16.2 남은 의견 차이
- 프레임워크(UD-01): ui-opus R1은 미세 반응성 계열을 선호했다. 결정은 React지만, M0 골격에서 셸 갱신 비용이 예산(제어 렌더 ≤ 20 ms)을 넘으면 재검토한다.
- 존재 확인의 Linux 기본값: S-O1 결과 전까지 대역 외 코드.

---

## 17. 핵심 원칙 체크리스트 (02-core-principle.md)
- [x] 맨 앞 **핵심 루프** 표(§0) — UI가 장치를 열지 않는 사실과 대신 보장하는 것.
- [x] **종단 간 시나리오**(§0.1 E-UI-1 + UDS·CAN 변형) — 구체 명령·이벤트·화면·지연·사용자 개입.
- [x] **실시간성 목표치**(§0.2) + 고부하 동작(§0.3·§3.5·§6.5) — 드롭 표시·다운샘플·3층 손실·STALLED.
- [x] **M1 = 걷는 뼈대**(§11) — sim 위 셸 + UART·CAN·UDS 최소 패널로 4요소, 부가 기능은 M1 이후.
- [x] **사용자 화면이 에이전트 세션과 분리되지 않음**(§0 비분리, §3.3 늦은 합류, §4.3 불변식).
- [x] 원칙에 기여하지 않는 기능은 후순위·비범위(§1.3).
