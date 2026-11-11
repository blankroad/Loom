docs/plan/02-core-principle.md 준수.

# UI R4 최종 점검 — `docs/plan/ui/plan.md` v2.0 (점검: ui-opus, 2026-11-10)

대상: `ui/plan.md`(965줄, 21:22 저장본). `plan.md`는 고치지 않았다.
대조: `02-core-principle.md`, `00-brief.md` §4, `ui/brief.md`, `r1-opus.md`·`r1b-opus-xteam.md`·`r2-opus-review.md`, `_xteam/{can,uart,uds,explore}-ui.md`(uart-ui v1.2 21:11, can-ui 21:15 포함), `ui-test.md`, `core-ui.md`(v0.2 요약 + core 의견), `core-test.md`, `core/r1-opus.md`.
결론 먼저: **구조·원칙·결정 기록은 탄탄하다.** 막는 문제는 `loom.viz` 명세 공백 1건(높음). 그 밖에 안전 관련 중간 4건(Linux에서 approve 자격을 얻을 길이 없음, 보안 fixture가 틀린 위험을 시험, 비TTY bootstrap을 "CORE 수용"으로 과대 기재, UART 새 채널 자동 표시 누락)과 이름·참조 오류가 있다.

---

## 1. 02 핵심 원칙 체크리스트

| # | 항목 | 판정 | 근거 절 | 비고 |
|---|---|---|---|---|
| 1 | 맨 앞 "핵심 루프" 표 | 통과 | §0 | UI가 장치를 열지 않는 사실과 대신 보장하는 것(I-UI1)까지 명시 |
| 2 | 종단 간 시나리오 1개 이상 | 통과 | §0.1 E-UI-1 + UDS·CAN 변형 | 명령·이벤트·화면·ms·개입(T2–T6) 모두 있음 |
| 3 | 실시간성 목표 + 고부하 동작 | 통과(보완 권고) | §0.2·§0.3·§3.5·§6.5·UI-T03/T05 | UART 폭주(921600) 중 화면 정직성·Ctrl-C 지연이 M1 수용 시험에 없다(#9) |
| 4 | M1 = 걷는 뼈대 | 통과 | §11 UI-M1, §1.2 | sim 위 셸 + UART·CAN·UDS 최소 패널. 부가 기능 M1b 이후로 분리 |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음 | 통과(보완 권고) | §0 비분리, §3.3, §4.3 | 에이전트가 **새 채널을 열 때 UI가 자동으로 보여 주는 규칙**이 없다 — 숨은 채널 위험(#5) |
| 6 | 비기여 기능 후순위·비범위 | 통과 | §1.3 | |

## 2. `00-brief.md` §4 15절

| 절 | 위치 | 판정 |
|---|---|---|
| 1 범위/비범위 | §1 | 있음 |
| 2 시나리오(에이전트·사용자 각 3+) | §2.1(6)·§2.2(7) | 있음 |
| 3 아키텍처 | §3 | 있음(구성도 2개) |
| 4 CLI/API | §4 | 있음 — 단 `loom viz` 명령 없음(#1) |
| 5 뷰어 UX | §5 | 있음(와이어프레임 7개) |
| 6 데이터/기록 | §6 | 있음 |
| 7 라이브러리 | §7 | 있음(버전·라이선스·근거) |
| 8 HW/OS | §8 | 있음 |
| 9 시뮬레이션·테스트 | §9 | 있음 |
| 10 안전 | §10 | 있음 |
| 11 마일스톤 | §11 | 있음(수용 기준 UI-T01–T10) |
| 12 작업 분해 | §12 | 있음(담당·의존) |
| 13 리스크 | §13 | 있음 |
| 14 질문 | §14 | 있음(기본값 포함) |
| 15 계약 변경·의존 | §15 | 있음 |
15절 모두 있고 비어 있지 않다. §16 결정 기록·§17 체크리스트 추가.

## 3. `ui/brief.md` 범위 1–8

| # | 범위 | 다룬 곳 | 판정 |
|---|---|---|---|
| 1 | 앱 형태·Tauri·원격 인증 | §3.1·§3.7·§3.8·§8 | 충분 |
| 2 | 기술 선택 근거 | §7·§16 UD-01–04 | 충분 |
| 3 | 공통 셸(레이아웃·STOP·비상 OFF·승인 대기열·env·연결/gap/LAG·기록 탐색·마커·테스트 화면) | §5.1.1·§5.1.5–5.1.7·§5.4–5.7 | 충분 |
| 4 | 패널 프레임워크·데이터 계약 | §4.2–§4.5 | 충분 |
| 5 | 1차 패널(UART·CAN·UDS·LA·DLT·**`loom.viz`**) | §5.1.2–5.1.4·§5.8 / viz는 §1.2·§11 M3 한 줄씩 | **미흡** — viz 입력 계약·명령·자동 렌더 규칙 없음(#1) |
| 6 | 성능 목표·gap 배지 | §0.2·§3.5·UI-T03/T05 | 충분 |
| 7 | CORE 합의(전송 형식·스키마→TS) | §6.2·§6.3·§15.1 | 충분 |
| 8 | M1 시연 | §0.1·§11 | 충분 |

---

## 4. 문제 목록

### 4.1 내용 공백·안전

| # | 심각도 | 위치 | 문제 | 수정 제안 |
|---|---|---|---|---|
| 1 | **높음** | §1.1·§1.2·§4.1·§6 | C6로 `loom.viz` 소유가 CAN에서 UI로 넘어왔는데, plan에는 viz의 **입력 계약**(long/wide JSONL·CSV, `ts/series/value/unit/quality/source`, 메타 `value_type·enum_map`), **`loom viz show/push/list/summary/export` 명령**, **자동 렌더 규칙**(`--kind auto`: 시간 열 탐지·수치→line/step·enum→State·공통 envelope→도메인 어댑터), 라이브 소스(`live:can:ch0`, `live:uart:dut --extract`), quality/gap 선 끊기가 없다. §2.1 A6이 `loom viz show`를 쓰는데 §4.1에 명령이 없다. 사용자 원 요구 #2("데이터만 받으면 렌더링")의 정본이 사라진 상태 | §4.6 "`loom viz`"(CAN plan §4.4 표 이관) + §6.6 "viz 데이터셋 계약"(CAN plan §6.4·`viz-requirements.md` 표준 입력 계약 이관) 짧게 신설. M3 그대로, 정본 소유를 UI로 명시 |
| 2 | 중간 | §3.7 자격 표, §8 존재 확인 행, §10.3 C | approve 자격 = "operate + **승인 장치 등록(WebAuthn)**". 그런데 Linux 벤치(플랫폼 인증기 없음)에서 보안 키도 없으면 대역 외 코드(C)를 쓰기로 했다 → **approve 자격을 얻는 경로가 정의돼 있지 않다.** 그 호스트에서는 R3/R4 승인·STOP 해제·정책 완화가 아무도 못 하는 상태가 된다 | approve 발급 = "WebAuthn 등록 **또는** `approval.presence=out_of_band_code` 설정 시 TTY `loom ui pair --approve` 1회". 승인 시점 존재 확인은 각각 assertion / 대역 외 코드 |
| 3 | 중간 | §9.2 보안 행, §11 UI-T07 | 보안 fixture가 "DA/DSR/**OSC52**". §16.1·§5.1.2 스스로 확인했듯 OSC 52는 xterm 6.0.0 core에 핸들러가 없어 시험해도 TX가 나올 수 없다. 실제 자동 응답 경로인 **OSC 4/10/11/12 `?` 색 질의, DECRQSS**, 그리고 OSC 8 링크 처리가 수용 시험에 없다(R2 F1·F2 반영 누락) | UI-T07·§9.2: "DA·DSR·DECRQSS·OSC 4/10/11/12 `?` 질의 → 의도 없는 TX 0건, OSC 8 링크 비활성, clipboard addon 미로드 확인"으로 교체 |
| 4 | 중간 | `_xteam/core-ui.md` v0.2 요약 CR-UI-07, plan §3.7 "(CORE: 비TTY stdout에는 bootstrap 미출력)" | "비TTY는 가리키기·안내만 — **이미 CORE 수용**"으로 적었지만 core-opus §4.1은 "브라우저가 없으면 **OS 브라우저를 연다(bootstrap 포함)**"이다. CORE가 수용한 것은 "비TTY stdout에 비밀을 **출력하지 않음**"까지이고, 비TTY 호출이 브라우저를 열어 operate 쿠키를 주는 경로는 남아 있다(UD-05와 충돌) | "수용"을 "요청"으로 고치고 CORE 계약 v1에 "비TTY `loom ui`/`ui open`은 bootstrap 발급 안 함"을 명시 요청. UDS(`uds-ui` 보안 주의·Q4)와 같은 입장이라 근거가 충분 |
| 5 | 중간 | §5.2·§5.7 (해당 규칙 없음) | UART 명세 v1.2 Q13·`uart/plan.md` §3.12: 에이전트가 **새 채널을 열면 모든 탭에 토스트 + 작업 공간에 패널 자동 추가**(끄더라도 채널 목록에는 남음) — 숨은 채널 방지 요구. plan에 없다. 02 원칙 5와 직결 | §5.7에 "새 채널 규칙": `*.channel_state{opened}` 수신 → 채널 바에 항상 추가 + 토스트 + `auto_open_panels: on(기본)\|off`. 모든 도메인 공통으로 |
| 6 | 중간 | §3.3 구독 공유, §3.2 다중 탭 | UART 서버 상한 "UART 데이터 구독 채널당 4·전체 16"(UART DR-V21, uart-ui Q15). 탭마다 WS·구독을 따로 두는 설계라 5번째 탭은 거절된다. plan에 정책 없음 | 상한 초과 탭 = 거절 대신 "읽기 전용 미러"(서버가 다른 구독의 배치를 복제 전송) 또는 셸에 `구독 상한 — 다른 탭을 닫으세요` 명시. CORE·UART와 합의 항목으로 §15.1에 추가 |
| 7 | 중간 | §1.2·§11 UI-M1b·§12 UI-18 ↔ §5.8·UD-26 | DLT·LA 패널을 §1.2·§11·§12는 **M2**, §5.8·UD-26은 "도메인 준비 시 **M1b**"로 적었다. §11 M1b 내용·수용 기준에는 없다. EXPLORE 명세는 셋 다 **M1** | §11 M1b에 "(선택) `dlt.trace`·`la.capture` 상태·디코드 표 — 도메인 준비 시, M1b 수용 기준 밖" 한 줄 + §1.2 같은 표기. EXPLORE `## ui 의견`으로 M1→M1b/M2 이유 회신 |
| 8 | 중간 | `_xteam/{can,uart,uds,explore}-ui.md`, `ui-test.md`의 `## ui 의견` | 다섯 파일 모두 **ui 의견 절이 비어 있다**. plan §15.2는 "회신 예정". 도메인 팀은 아직 옛 이름·옛 규칙으로 구현 계획 중(§4.3 표) | 리드가 각 파일에 §15.2 내용을 회신. DONE 뒤 xteam 작업으로 해도 되지만 STATUS에 미완으로 남길 것 |
| 9 | 중간 | §11 UI-T03·UI-T05 | UART 명세의 M1 성능 요구 — **921600 연속(≈ 92 KB/s) 중 정상 부하 view skip 0·heartbeat 1 Hz·메인 스레드 멈춤 없음**, **폭주 중 keydown→`sent` p95 < 100 ms** — 가 UI 수용 시험에 없다(UI-T03은 비부하 < 20 ms만, UI-T10은 메모리만) | UI-T05에 "UART flood 60 s: view skip 0, heartbeat 연속, 넘치면 gap seq 정확", UI-T03에 "폭주 중 Ctrl-C keydown→`sent` p95 < 100 ms" 추가 |

### 4.2 내부 일관성

| # | 심각도 | 위치 | 문제 | 수정 제안 |
|---|---|---|---|---|
| 10 | 중간 | §15.2 CAN 항목 | "`test.step_begin/end` 2토큰"을 CAN에 요청 — **폐기된 이름**(R2 쟁점 11에서 철회). 같은 절 TEST 항목·§5.1.6·core-test O6은 `test.step_started/ended` | CAN 항목을 "`test.step_started/ended`(core-test O6)"로 |
| 11 | 낮음 | §0.3 "(§6.4)", §4.2 manifest 주석 "(§6.4)", §13 마지막 행 "(§9.3)" | 메모리 상한 표는 §6.5, 실행 규칙은 §9.4 | 참조 번호 수정 |
| 12 | 낮음 | §4.5 STOP 행 "해제 `core.stop.release`(approve 자격)" ↔ §5.1.7 "M1은 operate + 확인 카드, M2부터 approve" | M1에서 approve 자격이 없으므로 §4.5만 보면 M1에 STOP을 풀 수 없다 | §4.5에 "(M1: operate + 확인 카드)" |
| 13 | 낮음 | §4.5 시계 행 "CORE 수용(d1/d2 둘 다 요청)", §15.1 CR-UI-03 "수용" ↔ `core-ui.md` v0.2 CR-UI-03 "core-opus 초안은 단일 mono" | 수용 여부가 문서마다 다름. core-sol만 d1/d2 | "core-sol 수용, core-opus 미반영 — 계약 v1 확정 필요" |
| 14 | 낮음 | §6.5 "터미널 write 대기 1 MiB/채널" ↔ uart-ui v1.2 "수신·그리기 대기 **2 MiB**로 통일(DR-V22)" | 수치 불일치 | UART 값 2 MiB로(탭 합 상한은 재계산) |
| 15 | 낮음 | §4.2 TS 초안 | R2 합친 API의 `snapshotKind`(sol)와 `act(actionId: DeclaredAction)` 타입 좁히기가 빠짐 — `act(actionId: string)` | `TopicSpec`에 `snapshotKind`, `act`에 `DeclaredAction` |
| 16 | 낮음 | §0 "`no_visible_viewer`(CORE 제안 이름)" ↔ uart-core #13 `viewer_not_visible` | 경고 이름 둘 | CORE 이름으로 UART에 통보(§15.2 UART 항목에 추가) |
| — | 확인 | §0.2 ↔ UI-T03 | UART 45 / UDS 35 / CAN 50 ms UI 구간, 종단 100/100(200)/150(300) — 일치. §0.2 표의 열 합도 각 목표 이내 | 문제 없음 |
| — | 확인 | M 배정 §1.2 ↔ §5 ↔ §11 ↔ §12 | #7 외 일치(R2 카드 M1, R3/R4 M2, `shell.test`·`psu.gauge` M1b, Graphics 기본 M2·고급 M3, 도킹 M2, Tauri M4) | |
| — | 확인 | §16 결정 ↔ 본문 | UD-01–28 본문과 모순 없음(#10은 §15.2 쪽 오류) | |

### 4.3 타팀 명세와 아직 충돌하는 것 (plan이 정한 안과 다른 채 남은 상대 문서)

| # | 심각도 | 상대 문서 | 충돌 | 수정 제안 |
|---|---|---|---|---|
| 17 | 중간 | `can-ui.md` §5.1, U-CAN-7 | 출처 색 AGT **주황**·USR **파랑**(D50 유지), 지연 회신 `ui.latency_sample` — plan UD-07·UD-15와 다름. 21:15 갱신에도 그대로 | ui 의견으로 UD-07·UD-15 확정 통보(#8). R-4 마스터 요청이 결정되면 CAN 표 수정 |
| 18 | 중간 | `uds-ui.md` 접근성 절, 지연 측정 | 셸 STOP `Ctrl-X`, 데몬 `ts.wall_ns` 대 브라우저 시각 비교 — plan UD-20·§3.5와 다름 | ui 의견으로 회신. UDS Q4(TTY만 bootstrap)는 plan과 같다고 답 |
| 19 | 낮음 | `explore-ui.md` | `core.marker`·`job.progress`·`la.capture.done`(3토큰), M1 범위 | §15.2 요청 그대로 ui 의견으로 + #7 |
| 20 | 낮음 | `ui-test.md` | `approval.grant`·`records.range`·`core.marker`, 요청의 `origin:user` 필드 | §15.2 요청 그대로 ui 의견으로 |
| 21 | 낮음 | `uart-ui.md` v1.2 Q14 | 딥링크 `/#/uart/<ch>` 제안 ↔ plan `/ui/#/p/uart/terminal?channel=dut`. **자격 없는 브라우저가 딥링크를 열 때 동작**(bootstrap 안내 페이지)이 plan에 없음 | §4.1에 "쿠키 없음 → `/ui/#/need-auth`: `터미널에서 loom ui 실행` 안내(비밀 없음)" 추가, UART에 딥링크 형식 회신 |
| 22 | 낮음 | `core-ui.md` core 의견(core-sol) ↔ core-opus | STOP RPC 이름, 가리키기 이벤트(`core.ui_focus`/`ui.focus_request`), `core.time` 필드 — CORE 내부 미확정. plan은 "CORE 확정"으로 올바르게 열어 둠 | CORE 계약 v1 확정 시 §4.5 갱신(조치는 CORE) |
| — | 해결됨 | `test-domains.md`·`uart-ui` v1.1 | 증거 REF를 TEST 형식으로, `core.time`·`core.metrics.report` 이름 채택 | |

### 4.4 R1·R1b·R2 지적의 반영 상태
- 반영됨: 자격 3단·비밀 비노출·등록 게이트·`localhost` 오리진(R1b §4), 쟁점 1–16 대부분(UD-05–UD-27), R2 F1–F5·F7, §2.1 재측정 P1–P9, 10분 압축 soak, Worker 링 + 미러, 공통 컴포넌트(RowStore·TerminalView·ChannelBar·approval 렌더러·reveal), TEST 상태값·이벤트 이름, EXPLORE 패널 일정.
- **잘못 반영**: #3(보안 fixture가 OSC52), #10(`test.step_begin/end`), #4("CORE 수용" 과대 기재).
- **빠짐**: #2(Linux approve 경로 — r1b §4.5-4에서 대역 외 코드를 기본으로 정했으나 자격 발급과 연결 안 됨), #15(합친 API 일부), R2 §2.1의 "폭주 중 입력 지연"(#9).

---

## 5. DONE 가능 여부
**조건부 가능** — #1(`loom.viz` 명령·입력 계약 절 신설)과 중간 #2·#3·#4·#5·#10을 반영하면 DONE으로 표시해도 된다. #6·#7·#9는 같은 개정에 넣기를 권하고, #8·#17–#21(타팀 `## ui 의견` 회신)은 DONE 뒤 xteam 작업으로 STATUS에 남기면 된다.
