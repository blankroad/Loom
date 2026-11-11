docs/plan/02-core-principle.md 준수

# CORE R4 최종 점검 — core-opus

대상: `docs/plan/core/plan.md` **2026-11-10 21:23 판(1,147행, 129,102 B)** — 점검 도중 리드가 TEST R4 요청을 반영해 갱신했으므로 그 판을 기준으로 다시 확인했다. 대조: `00-brief.md` §4, `02-core-principle.md`, `core/brief.md`, `r2-sol-review.md`, `r2-opus-review.md`, `_xteam/{can,uart,uds}-core.md`, `core-ui.md`(21:23 v0.2 요약), `core-test.md`(test 의견), `test-domains.md`(uart·uds 의견), `ui-test.md`, `uart/plan.md` v2.0, 마스터 §2(C4.1 포함). plan.md는 수정하지 않았다. 스파이크 없음.

## 판정: `PASS with fixes`

구조·원칙·R2 지적 반영은 통과. **P0 3건은 C00(Day 1–2 `loom-proto` 타입) 동결 전에 고쳐야 한다** — 둘은 타팀 계획과의 계약 모양 충돌이고, 하나는 plan 작성 직후(21:23) 갱신된 UI 요구의 스키마 항목이다.

---

## 수정 필요 목록

| # | 우선 | 위치 | 현재 요지 | 제안 문장 |
|---|---|---|---|---|
| 1 | **P0** | §3.1 도메인 표 UART 행, §9.3 UART 행, §3.9 `WorkerSpec` | "worker `uart`(포트 전부 1 프로세스)", 예산 `48 + 8×포트` | UART v2(`uart/plan.md` §3.2 DR-V13)는 **포트당 worker 1개(최대 8), worker hard 64 MiB**이며 "인스턴스 모델은 CORE supervisor와 P0 합의 [가정]"이라 적었다. → "`WorkerSpec.instances = PerChannel{max} | Shared`. UART = PerChannel(max 8, soft 48 / hard 64 MiB per worker, C2 크래시 격리). 감독자 전체 admission 1 GiB 안에서 UART 8개 = hard 512 MiB를 허용하려면 기본 번들 계산을 갱신." 로 바꾸고 §9.3 UART 행·기본 번들 합(416 MiB)도 다시 계산 |
| 2 | **P0** | §6.1 "증거 경로 정본 … `/payload/data`, `/data/<field>`", `EventRecord.payload`(바이트), 최상위 `write_state` | 구조 필드는 `data`, `payload`는 바이트, `write_state`는 최상위 | 도메인·UI가 이미 다른 모양으로 고정했다: UART `/payload/state`·`/payload/rx_offset`, UDS `/payload/sid`·`/payload/decoded/…`(test-domains 의견), UI "쓰기 이벤트는 `request_id` + **`payload.write_state`** 필수"(core-ui v0.2 규칙). → 둘 중 하나로 통일하는 문장을 넣는다. 권고: "**`payload` = 도메인 구조 객체(스키마는 kind별), 원시 바이트는 `payload.bytes{encoding, data}`, 쓰기 상태는 `payload.write_state`**로 정한다. `data` 필드는 없앤다." (도메인 2팀·UI·TEST가 이미 이 모양) — 그리고 CR-C6를 같은 문장으로 갱신 |
| 3 | **P0** | §3.4 role 표·승인 객체 `challenge{kind:"code"|"os_confirm"}`, §15.2 core-ui 행 | U1–U8 수용, challenge 필드 | core-ui v0.2(21:23)의 스키마 요구가 빠졌다: CR-UI-07 **viewer rights 3단 `watch / operate / approve`**와 승인 객체 **`presence{method: webauthn|oob|code, proof}`** 자리(M1 rights, M2 WebAuthn). → role 표에 "viewer.rights ∈ {watch, operate, approve}; TTY `loom ui`=operate bootstrap, `loom ui pair`=watch 기본" 행 추가, `challenge`를 `presence`로 개명(code = 오클릭 방지용) |
| 4 | P1 | §15.2 core-ui 행 | v0.1 기준 반영 | core-ui v0.2의 나머지를 행으로 추가: CR-UI-08 `core.latch` payload(정지 잡·취소 승인·취소 불가 수·`outputs_held[]`) + **HTTP `POST /api/v1/stop`**(WS 끊김 중 STOP), CR-UI-11 `core.agent_cursor`·`core.activity` 읽은 범위, CR-UI-12 **`core.safe_state.run{ids[]}`**(현재 `core.recover{domain,target}` 단건과 불일치), CR-UI-13 공통 `control{…}` 뷰, CR-UI-14 fixture에 고부하·gap 3층·outcome_unknown·제어 큐 포화, CR-UI-16 이름 **`core.ui.layout.*`**(현재 `core.layout.*`), `core.metrics.report{kind}` |
| 5 | P1 | §4.2 출력 예시, §3.4 | `http://127.0.0.1:47123/` | CR-UI-10: "UI 오리진 정본 = `http://localhost:<port>`(바인딩은 127.0.0.1, 127.0.0.1 접속은 리디렉트 — WebAuthn RP ID가 IP 불가)". 예시 URL 2곳 교체 + Host/Origin allowlist에 `localhost:<port>` |
| 6 | P1 | §0.4 첫 행 vs §0.2 홉 6–8 | 코어 구간 UART p95 ≤ 25 ms | 같은 구간(host-ingest → WS 송신)의 홉 예산 합이 10 + 2 + 15 = **27 ms**로 목표를 넘는다. → 홉 8을 "≤ 13 ms(배치 10 + 송신 3)"로 줄이거나 §0.4를 "≤ 30 ms"로 |
| 7 | P1 | §9.3 gateway 장부 | "≈ 259 MiB" | 나열 항목 합은 **251 MiB**(32+32+2+6+8+18+2+2+4+32+32+8+64+8+1). 또 Q12 우선 큐는 "연결당"이고 Q15는 64 연결인데 장부는 8×0.25만 셈. → "셸 전역 제어 구독은 **viewer 연결에만**(agent 단발 연결 없음)"을 Q12에 명시하고 합계를 251로(또는 누락 항목을 적고 259로) |
| 8 | P1 | §11 M0 수용 "(미구현은 `E_NOT_IMPLEMENTED`)" vs §4.4 | enum에 없음 | §4.4 1번 묶음에 `E_NOT_IMPLEMENTED`(exit 1) 추가, CR-C4 목록에도 |
| 9 | P1 | §10.9 일괄 사전 승인 "세션 `max_test_duration` 기본 1 h", RunPlan | A9 확장인데 CR 없음 | A9는 "승인 객체 사용 기한 10분 + job renew"만 정한다. 묶음 승인이 세션 동안(≤ 1 h, 설정 ≤ 8 h) 유효한 것은 확장 → **CR-C18 "A9 확장: TestPlan·RunPlan 승인은 시작 ≤ 10분, 이후 세션/run 종료·max_test_duration·STOP·정책/epoch 변경 중 먼저 오는 때까지"** 추가 |
| 10 | P1 | §3.9·§11 M0 (점검 8) | testkit = FakeWorker만, "`cargo add`" | (a) **가져오는 방법**이 없다 — "Day 2에 `core/p0` 브랜치 태그 `core-p0-d2`, 도메인은 workspace path 의존(같은 저장소 worktree)" 같은 한 줄 필요. (b) gateway 쪽 도메인(UDS 세션 관리자, CAN 질의 모듈)은 첫 주에 돌려 볼 하네스가 없다 → testkit에 **`RegistryProbe`(register() 호출 + 이름·스키마 검사)와 `HookHarness`(fixture `WriteRequest`로 classify·approval_binding·presend·within_pattern 호출)** 추가 — 순수 함수라 제품 허브·정책이 필요 없음. (c) `CallCtx`·`JournalReader`가 §15.2 uart #9 답에는 있으나 §3.9 `loom-domain-api` 목록에 없다 → 목록에 추가(P0는 시그니처만) |
| 11 | P1 | §9.2 큐 표 | "표에 없는 큐는 merge 금지" | TEST R4 요청 8의 **test 제어 큐 1k(backpressure, 드롭 금지)**·메모 1k/세션이 §10.9에만 있고 표에 없다 → Q29 행 추가 |
| 12 | P2 | §4.4 묶음 7 `E_BUSY` | gateway RPC admission 초과를 "장치·전송 오류(7)"로 | D5의 7은 장치·전송. admission 초과는 레이트 성격 → 4(`E_RATE_LIMIT`과 같은 묶음) 또는 별도 근거 명시 |
| 13 | P2 | §3.3 재기동 `RxOnly`(CAN) | 재기동 시 열려 있던 CAN 채널 RX 재개 | can-core v2.0 갱신: "**`autostart` 채널만 장치 identity 재검증 후** RX 재개". D8(데몬은 장치를 자동으로 열지 않음)과도 맞추려면 이 조건을 `RxOnly` 정의에 넣는다 |
| 14 | P2 | §10.3 의사코드 0) | latch 거절이 `Requested` 발행 전에 return | STOP 중 에이전트의 쓰기 시도도 사용자 화면에 보여야 한다(Sol R2 §3.1 6) → latch·기록 불능 거절도 `write_row(rid, Failed).reason("stopped")` 발행. 또 조기 return 경로마다 `idem.release/complete` — 지금은 Confirm·Approve·Deny 경로에서 pending 예약이 남는다 |
| 15 | P2 | §6.2 M1 codec = JSON | worker는 IPC에 MessagePack, 저널에 JSON | CAN worker hot path에서 레코드를 두 번 인코딩한다(CAN 계획이 우려한 worker 내 JSON 비용). §9.3 CAN 예산·C13에 "M1 이중 인코딩 비용 측정" 한 줄, 큰 경우 CR-C14 앞당김 근거로 |
| 16 | P2 | §9.6 soak RSS 관문 | worker ≤ 1 MiB/h, max(16 MiB, 5 %) | UART v2 §9.5는 worker **≤ 0.25 MiB/h·창 차 ≤ 8 MiB**로 더 엄격. "도메인이 더 엄격한 관문을 두면 그것이 우선" 한 줄 |
| 17 | P2 | §15.1 순서 | CR-C17이 CR-C16 앞 | 번호순 정렬(가독성) |
| 18 | P2 | §15.3 / C05 | `core-test.md`·`can-core.md`의 `## core 답변` | can-core는 `## core 답변`이 아직 비어 있고, core-test는 R1 core 의견 이후 갱신 안 됨 → C05 산출물에 "plan v2.0 §15.2를 각 파일 `## core 답변`으로 옮김" 명시 |

---

## 점검 1 — 00-brief §4 15절

| 절 | 판정 | 위치 |
|---|---|---|
| 1 범위/비범위 | 통과 | §1 (후순위 표에 C4.1 반영) |
| 2 시나리오(에이전트·사용자 각 3+) | 통과 | §2.1 6개, §2.2 6개 |
| 3 아키텍처 | 통과 | §3 구성도·데이터 흐름 4경로·장애 흐름 |
| 4 CLI/API | 통과 | §4 명령 트리·출력·봉투·오류·RPC 표 (`--json` 예시는 r1-opus 참조로 대체 — 허용) |
| 5 뷰어 UX | 통과 | §5 (코어 관점, UI는 UI 팀) |
| 6 데이터/기록 | 부분 | §6 — payload 모양 충돌(수정 #2) |
| 7 라이브러리 | 통과 | §7 (max_stable 기준, 버전 정정 반영) |
| 8 HW/OS | 통과 | §8 |
| 9 시뮬·테스트 | 통과 | §9 (큐 표·장부·T1–T15·soak·격리) |
| 10 안전 | 통과 | §10 |
| 11 마일스톤 | 통과 | §11 측정 가능한 수용 기준 |
| 12 작업 분해 | 통과 | §12 C00–C30, O/S·의존·대기 팀 |
| 13 리스크 | 통과 | §13 (파일 시스템 정지 재발 포함) |
| 14 질문 | 통과 | §14 Q1–Q6 |
| 15 CR/의존 | 부분 | §15 — core-ui v0.2·A9 CR 누락(수정 #3 #4 #9) |

## 점검 2 — 02 원칙 체크리스트

| 항목 | 판정 | 근거 |
|---|---|---|
| 맨 앞 핵심 루프 표 | 통과 | §0.1 |
| 종단 간 시나리오(명령·홉·ms·개입) | 통과 | §0.2(홉별 예산·큐, 개입 3종과 목표), §0.3 |
| 실시간성 목표·고부하 동작 | 부분 | §0.4–0.5, §9.2 — 홉 합 27 ms > 코어 목표 25 ms(수정 #6) |
| M1 = 걷는 뼈대 | 통과 | §0.6, §11 — 실제 브라우저 화면 게이트와 코어 게이트 둘 다 M1 조건 |
| 화면이 에이전트 세션과 분리되지 않음 | 통과 | §3.7 I1–I6, T1·T2·T7, `others_since_last` 클라이언트 전역 |
| 비핵심 후순위·비범위 | 통과 | §1.3 |

## 점검 3 — core/brief 범위 10항목

| # | 항목 | 판정 | 위치 |
|---|---|---|---|
| 1 | 프로세스 구조·판정 기준 | 부분 | §3.1 W1–W5 + 보조 기준 + UDS 지터 관문. UART 인스턴스 모델 충돌(수정 #1) |
| 2 | worker 프로토콜 | 통과 | §3.3 전송·2 lane·msgpack map·64 KiB·크레딧·covers·감독·예산 OS별 |
| 3 | IPC·허브·UI 전송 | 통과 | §3.4 role·mapping 정본·snapshot·셸 전역 구독·binary M2 |
| 4 | 저널 | 통과 | §6.2 비교·CRC32C·내구성 2 워터마크·세그먼트 GC·디스크 산술 |
| 5 | CLI·자동 기동·기동 시간 | 통과 | §4.1·§4.6 (실측-mac 3.3 ms, OS별 목표, 하위 게이트) |
| 6 | 정책·승인·감사·STOP·파이프라인 | 통과 | §10.3 원자 예약, §3.5(S) 순서, §6.3 감사 |
| 7 | 스키마 정본 | 부분 | §7 도구·draft 고정 — payload 모양 미통일(수정 #2) |
| 8 | 러너·테스트 세션 코어 | 통과 | §10.9 (expect 결속·coverage·witness·TEST R4 1–8) |
| 9 | C2 성능·메모리 | 부분 | §9.2–9.7 — 장부 합·Q12 범위·test 큐(수정 #7 #11) |
| 10 | 빌드·배포·경계 | 통과 | §7, §3.9 안정 API 5 crate, CI 3 OS, 2 바이너리 |

## 점검 4 — R2 지적 반영

| 지적 | 판정 | 위치 / 비고 |
|---|---|---|
| Sol F1 구독 예산 합산 | 반영(부분) | Q11·Q15·§9.3 — 합계·Q12 범위 오차(수정 #7) |
| Sol F2 UART 예산식 | 반영 → **재충돌** | §9.3 — UART v2 포트당 worker와 불일치(수정 #1) |
| Sol F3 mapping 정본 | 반영 | §3.4, §6.2, T14 |
| Sol F4 publish 의미 | 반영 | §3.3, CR-C16, `E_PARTIAL`, T2 |
| Sol F5 원본 손실 = A12 | 반영 | §3.6, §10.5, Q2 |
| Sol F6 승인 원자 예약 | 반영 | §10.3, T12 |
| Sol F7 STOP 순서 | 반영 | §3.5(S), T13 |
| Sol F8 D7 CR | 반영 | CR-C14 |
| Sol F9 CRC32C | 반영 | §6.2 |
| Sol F10 Job = commit | 반영 | §3.3, §8 |
| Sol F11 FD 부하·UDS 보장 범위 | 반영 | §0.3, §9.1 |
| Sol F12 "10배 여유" 확대 해석 | 반영 | 해당 문장 삭제, [실측-mac] 표기 |
| Sol F13 fallible 디코더 | 반영 | §6.1, T15 |
| Sol F14 훅 동기 | 반영 | §3.9 |
| Sol F15 세그먼트 GC | 반영 | §6.2 |
| Sol O-1 expect 결속 | 반영 | §10.9, `E_PLAN_MISMATCH` |
| Sol O-2 coverage 분류 | 반영 | §10.9 Positive/Complete/Measure |
| Sol O-3 witness 규칙 | 반영 | §10.9 (R-8 유지) |
| Sol O-4 pin lease·digest | 반영 | §10.9 ④⑥ |
| Sol O-5 max_uses 원자 차감 | 반영 | §10.9 TC-C3, TEST R4-4 |
| Opus E1–E4 crate 버전 | 반영 | §7 (max_stable 기준 명시) |
| Opus E5 구독 큐 | 반영 | Q11 시간 기반 |
| Opus E6 CLI 측정 | 반영 | §4.6 |
| Opus E7 재기동 채널 | 반영(부분) | `reopen_after_crash` — CAN 조건 보강(수정 #13) |
| Opus E8 `others_since_last` 범위 | 반영 | §4.3 |
| Opus E9 C4.1 | 반영 | §1.3, §3.1, §8 |
| Opus E10 CAN 최소 프레임 | 반영 | §0.3, §9.1 |
| Opus S1–S2 | 반영 | = F1·F3 |
| Opus S3 예약 용량 | 반영 | §3.3 |
| Opus S4–S9 | 반영 | = F5·F6·F7·F8·F9·F10 |
| Opus S10 과잉 해석 | 반영 | = F12 |
| Opus S11–S13 | 반영 | = F14·F15·O-3 |
| Opus S14 fixture replay | 반영 | §3.9 `--fixture-replay`, C04 |
| Opus S15 viewer-tty | 반영 | §3.4 M1 웹만 — 단 core-ui v0.2 rights 3단 미반영(수정 #3) |
| Opus S16 이름 | 반영 | `core.time`·`core.metrics.report` |
| Opus S17 C4.1 행 삭제 | 반영 | §3.1 |
| Opus S18 홉 합 ≠ 종단 | 반영 | §0.2 마지막 행 — 단 홉 합이 코어 목표 초과(수정 #6) |

누락 0, 의도적 미반영 0. 부분 3건은 위 수정 목록에 있다.

## 점검 5 — 내부 모순

| 항목 | 판정 | 내용 |
|---|---|---|
| §0.2 홉 큐 ↔ §9.2 | 통과 | 홉 1–8의 큐 상한이 Q16·Q5·Q7·Q12·Q1·Q3·Q9·Q11과 모두 일치 |
| §0.2 홉 예산 ↔ §0.4 | **불일치** | 홉 6–8 합 27 ms > UART 코어 p95 25 ms(수정 #6). RPC→worker 3 ms = 홉 1+2 일치 |
| §9.2 ↔ §9.3 장부 | **불일치** | 합 251 vs 259, Q12 연결 범위(수정 #7). Q20 "질의 버퍼 8 MiB/건"은 장부에만 있음 — Q20에 추가 권고 |
| 오류·종료 코드 | 부분 | §4.4 ↔ CR-C4·CR-C17 일치. `E_NOT_IMPLEMENTED` 누락(#8), `E_BUSY` 묶음(#12) |
| RPC·이벤트 이름 | 부분 | 2/3토큰 규칙 일관(`test.step_started` 이벤트 / `test.step.begin` RPC). UI v0.2와 `core.layout`↔`core.ui.layout`, `core.recover`↔`core.safe_state.run` 불일치(#4). STOP 이름 `core.stop`은 UI 추천과 일치 |
| 역할 | 통과 | `core.stop.release`·`approval.respond`·`recover`·`worker.restart`·`policy.apply`·override = web viewer, T6에 viewer-tty 포함, §10.2·§10.7과 일치 |
| 단계 배치 | 통과 | W6 = M2(§3.3·§3.6·§11·C24·§15.2·§16 #19 일관), binary = M2, msgpack 저널 = M2(CR-C14), replay = M3, test 마커 = M1(C19)·검증 = M2(C27) |
| gap 원인 이름 | 통과 | gateway 재시작 공백 = `source.gap{cause:"gateway_restart"}`(§3.3·§13·§15.2 일관, can-core v2.0과 일치) |

## 점검 6 — §15.2 ↔ 타팀 파일 현재 내용

| 파일(갱신 시각) | 판정 | 비고 |
|---|---|---|
| `uart-core.md`(21:11, v0.2) | 통과 | #9 "core-test O3·O4로 해결" — plan 행이 JournalReader·예산까지 답함. #12 `core.time` 일치 |
| `uart/plan.md` v2.0 | **불일치** | 포트당 worker(수정 #1), 더 엄격한 soak(#16) |
| `can-core.md`(21:15, v2.0 갱신) | 부분 | W6 M2·source.gap·witness·Emitter non-blocking 일치. RxOnly 조건(#13), `## core 답변` 빈칸(#18) |
| `uds-core.md`(08:24) | 통과 | C-1–C-13 전부 행 있음 |
| `core-test.md`(21:10, test 의견) | 통과 | R4 1–8 전부 §10.9 반영, exit 11 = CR-C17. 큐 표 누락(#11) |
| `core-ui.md`(21:23, v0.2 요약) | **불일치** | plan 작성(21:21) 직후 갱신 — CR-UI-07·08·10·11·12·13·14·16·이름·규칙 미반영(#3 #4 #5) |
| `test-domains.md`(21:15) | **불일치** | 도메인 JSON Pointer `/payload/…` ↔ plan `/data/…`(#2). `data.evidence[]` DTO·REF epoch 제안은 반영 |
| `ui-test.md`(08:20) | 통과 | RPC·역할·M1 마커 범위 반영, 이름 정렬 명시 |

## 점검 7 — 마스터 D/A/B/C 위반과 CR

| 결정 | 판정 | CR |
|---|---|---|
| D7 JSONL 정본 | 변경 제안 | CR-C14(M1은 내용 동일한 JSON 프레임, M2 msgpack) ✓ |
| D7 보존·감사 | 명문화 요청 | CR-C15 ✓ |
| A6 종료 코드 3 | 확장 제안 | CR-C12 ✓ (임시 규칙 명시) |
| D5 종료 코드 11·신규 코드 | 확장 제안 | CR-C17, CR-C4 ✓ — `E_NOT_IMPLEMENTED` 추가 필요(#8), `E_BUSY` 묶음 근거(#12) |
| D2 publish-before-reply | 의미 정밀화 | CR-C16 ✓ |
| D2 role(viewer 자격) | 변경 | CR-C1 ✓ — UI rights 3단 반영 시 CR-C1 문장 갱신(#3) |
| **A9 승인 기한** | **확장인데 CR 없음** | 수정 #9 (CR-C18 신설) |
| D8 장치 자동 open 금지 | 경계 | CAN `RxOnly` — `autostart` 채널 한정이면 위반 아님(#13) |
| A3·A12·A15·B4·C1·C3·C4.1·C6·C7·C8 | 위반 없음 | — |

## 점검 8 — P0 첫 주가 도메인 팀 관점에서 바로 시작 가능한가

| 질문 | 판정 | 근거·보완 |
|---|---|---|
| 무엇을 import하나 | 통과 | `loom-proto`, `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`(§3.9 안정 표면) |
| 언제 | 통과 | Day 1–2 타입 + compile-only trait, Day 3–5 testkit·echo·schema, Day 5 replay stub(§0.6·§11·§12 일관) |
| 어떻게 가져오나 | **부분** | 브랜치·태그·path 의존 방법 없음(#10a) |
| worker 쪽 첫 주 시험 | 통과 | FakeWorker + `loom-echo` 복사 시작 |
| gateway 쪽(UDS 세션 관리자·CAN 질의 모듈) 첫 주 시험 | **부분** | 하네스 없음 — `RegistryProbe`·`HookHarness`(#10b) |
| 타입이 바뀌지 않을까 | **부분** | payload 모양·worker 인스턴스 모델·presence 필드가 미정(#1–#3) — Day 2 전에 결정해야 도메인 첫 PR이 재작업 없이 간다 |
| UI 팀 | 통과 | `--fixture-replay`(WS) Day 5. fixture 내용 확장은 #4(CR-UI-14) |
