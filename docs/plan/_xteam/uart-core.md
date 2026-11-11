# UART → CORE 요청 (v2.0)

- 변경: v1(2026-11-10) — #1·#5·#16 갱신, #17–19 추가(UART v2.0 워커 검증 반영).
- 변경: v0.2(2026-11-10) — #9를 `core-test.md` O3·O4 수용으로 갱신.
- 변경: v0.1(2026-11-10) — #12–16 추가(uart-opus UI·테스트 초안에서 나온 CORE 요구).
- 작성: uart-lead, 2026-11-10. 상태: **v1**(UART plan v2.0 확정본 기준 — 바뀌면 이 파일 상단에 변경 줄 추가).
- 근거: `uart/plan.md` v2.0 §3·§9.6·§15, 마스터 §2.3 C1–C8. CORE는 `## core 의견` 절을 덧붙여 답해 주면 된다.
- 원칙(02): 에이전트가 쓰고 읽는 UART 바이트가 **같은 순간** 사용자 UI 터미널 패널에 보이고, 사용자는 패널에서 원시 키·Ctrl-C로 개입한다. 아래 요청은 모두 이 루프의 지연·무손실·개입 경로를 위한 것이다.

## 요청

| # | 요청 | 이유 / UART 쪽 가정 | 필요 시점 |
|---|---|---|---|
| 1 | **uart worker는 별도 프로세스**(다른 도메인과 같은 worker 프로토콜). UART는 벤더 DLL이 없지만 C2 크래시 격리·메모리 예산을 똑같이 적용받고 싶다. gateway 안 crate로 두자는 CORE 결정이면 그 근거와 격리 수단을 알려 달라 **UART 결정: 포트(채널)당 worker 1개, 최대 8**(plan DR-V13) — supervisor 인스턴스 모델을 P0에 합의하자. | 고속 flood(3 Mbaud) 파서·expect가 gateway를 멈추면 다른 도메인 화면까지 멈춤 | P0 |
| 2 | **worker → gateway 적응형 배치**(조용한 구간 뒤 첫 이벤트 즉시, 상한 ≤ 10 ms) + **원시 바이트 필드의 바이너리 전송**(worker 프로토콜과 UI WS 둘 다. hex/base64는 CLI JSON 출력에만) | UART 지연 예산: 장치 → UI 렌더 p95 < 100 ms. 배치가 두 구간(worker→hub, hub→UI) 있으면 각 ≤ 10 ms여야 성립 | P0 계약 |
| 3 | **대화형 입력 스트림(A1 ③) 전용 경로**: UI(브라우저) → gateway → worker로 키 청크를 요청 파이프라인 없이 흘리는 저지연 경로. 입력권(lease) 획득 때만 정책 1회 판정. **Ctrl-C(0x03)는 우선 제어 큐**로 flood 중에도 p95 < 100 ms. STOP·기록 장애 중에도 이 경로는 열려 있어야 함(A3·A12) | 사용자 개입이 핵심 원칙의 4번째 요소 | P1 (M1) |
| 4 | **사용자 입력 감사 묶음 스키마**(줄 단위 또는 1 s 묶음 + 해시) — v1.2 §15 #2 그대로 | 키마다 감사 레코드를 쓰면 저널·메모리 폭증(C2) | P1 |
| 5 | **viewer 자격 = 브라우저 UI 세션** 판정 방식 (v1.2의 "대화형 TTY 뷰어" 요청을 대체). 같은 OS 계정의 에이전트가 viewer 토큰을 읽어 자기 승인하지 못하게 하는 방법(예: UI 첫 접속 시 사람이 확인하는 페어링 코드, 토큰을 에이전트가 읽는 `daemon.json`과 분리 등)과 **원격 접속**(Linux 벤치 PC의 UI를 다른 PC에서 보기, B1) 인증을 UI 팀과 함께 정해 달라 M2 보조 TUI(SSH로 접속한 Linux TTY 포함)의 viewer 자격도 함께. `core-ui.md` CORE 제안(SSH 터널 기본, 브라우저 자동화 방어를 주장하지 않음, 에이전트는 bootstrap URL을 받지 않음)에 UART는 동의한다. | 승인·입력권·STOP 해제가 모두 viewer 자격에 걸려 있음 | P1 |
| 6 | **저널 = 원시 바이트 무손실 + seq 인덱스**를 core가 제공(UART 전용 `chunks.jsonl` sidecar 폐기 가능). 질의: `stream_seq` 범위 + 바이트 상한으로 원문 조회, 메모리 링은 고정 크기이고 오래된 백로그는 디스크에서(C2) | UI 터미널 패널의 백로그 → 라이브 이음, `read --from` 커서, 증거 검증이 모두 같은 저널을 씀 | P0 계약, P1 구현 |
| 7 | **공통 커서 `now`**(현재 high-water) — v1.2 §15 #4 그대로 | `expect --from now` 같은 "지금부터" 대기가 흔함 | P0 계약 |
| 8 | **crate 경계**: UART 팀 소유 `crates/loom-console`(A4 공유 콘솔 엔진: 링·커서·expect·InputArbiter·sanitizer)이 의존할 core 타입(이벤트 레코드·seq·커서·쓰기 상태)을 안정 crate(예: `loom-schema`)로 분리해 달라. UART는 `loom-console`, `loom-uart-worker`, gateway 측 UART 모듈(명령·risk table·훅)을 만든다 | SSH/ADB/RTT(EXPLORE)가 `loom-console`을 import | P0 |
| 9 | **테스트 세션 증거(C7)** — `core-test.md` O3·O4로 **해결(수용)**: UART는 `EvidenceVerifier` 도메인 술어 `uart.text_contains/absent/count/sequence`를 구현하고, 결과에 TEST REF 형식 `data.evidence[]`를 낸다(`uart/plan.md` §9.6.2–9.6.3). 남은 확인: 술어 평가 시 저널 원본 바이트 범위 읽기 API(#6)와 스캔 예산 공유 | UART 증거가 실제 장치 바이트에 근거하는지 Loom이 검증 | P2 |
| 10 | **`loom <domain> help --json` 공통 스키마**(C8): 명령·인자·위험 등급·종료 코드·예시 | 스킬 본문을 짧게 유지 | P1 |
| 11 | **24 h soak 공통 하네스**(C2): worker별 RSS·큐 깊이·드롭 카운터 주기 기록. UART는 flood·재연결 반복·구독자 attach/detach 반복 부하를 제공 | 관문 판정 | P2 |
| 12 | **UI 시계 맞춤·지연 보고 RPC**(CORE 제안 이름 `core.time`·`core.metrics.report` 수용, `core-ui.md`): `core.time`(서버 수신 즉시 호스트 monotonic 반환), `core.metrics.report{channel,p50,p95,p99,n,skew_err_ms}` | 웹 패널의 "장치 RX → 그리기 완료" 게이트를 호스트 `ts.mono_ns`와 비교(UART M1 수용 (b)) | P1 |
| 13 | **viewer 존재 신호** `core.viewer.presence{channel, visible}` + 결과 봉투 `meta.viewers{subscribed, visible}` | `no_viewer_attached`/`viewer_not_visible` 경고(사람 존재 증명으로는 쓰지 않음) | P1 |
| 14 | **viewer 입력 RPC 우선 처리**: gateway 수신 루프가 이벤트 팬아웃·에이전트 요청에 막히지 않게 viewer 입력(`uart.input`)과 입력권 이벤트를 별도 우선 경로로 | #3 Ctrl-C p95 < 100 ms(폭주 중)의 전제 | P1 |
| 15 | **종료 코드 3 확장**: "조건 대기 미충족"(`E_TIMEOUT`, 부정 검증 `E_UNEXPECTED_MATCH`, 횟수 `E_COUNT_OUT_OF_RANGE`), 증거 부족은 exit 8 + `verdict:inconclusive`. 거절 시 대안 = exit 0 + `data.verdict` | 통합 테스트(C7)의 `expect --absent/--count/--within` | P2 (계약은 P0) |
| 16 | **테스트 세션 사전 승인의 도메인 바인딩 형식**: CORE `core-test.md` O7 공통 바인딩(session_id·plan digest·env·정책 해시·stream_epoch·기한·횟수)은 그대로 두고, UART는 장치 신원·generation·허용 rule/바이트 조건 `{domain:"uart", channel, match:{rule|sent_escaped}, risk≤R3, max_uses}`를 **추가** 바인딩한다(R4는 사전 승인 제외 제안). 실제 TX digest는 매번 감사 | C7 세션 일괄 사전 승인 | P2 |
| 17 | **worker 메모리 집행**: UART worker RSS 목표 48 MiB·hard 64 MiB, Linux cgroup/Windows Job 제한과 플랫폼별 RSS 계측은 CORE 감독자 몫. gateway UART 계정 128 MiB(저널 큐 64 + 구독 큐 32 + 제어·인코딩·IPC·snapshot 32 — 하위 상한은 P0 배정) | C2, `uart/plan.md` §3.13 | P0 계약, P3 soak |
| 18 | **UART 데이터 구독 상한**: 채널당 4·전체 16, 구독당 인코딩 후 2 MiB·512항목. 17번째는 거절(또는 UI가 "읽기 전용 미러" 제안 시 그쪽) | DR-V21 | P1 |
| 19 | **WASM 빌드 가능 + seq→시각 조회**: `loom-console`이 의존하는 core 타입 crate는 `wasm32-unknown-unknown`에서도 빌드돼야 한다(웹 패널 정제기, DR-V2). 또 임의 도메인 이벤트의 `ts.mono_ns`를 `stream_seq`로 조회하는 내부 API(`expect --from seq:N --within T`, plan §9.6.1) | 웹 정제기, 교차 도메인 시간 기준점 | P0 계약 |

## UART가 CORE에 약속하는 것
- uart worker는 큐마다 상한·넘칠 때 정책·gap 층(`display.gap`/`record.gap`/`source.gap`)을 명시하고, 메모리 예산을 넘으면 화면용 큐부터 줄인다(저널 경로는 마지막까지 보존).
- 쓰기 상태는 A14 그대로(성공 종단 `sent`), 도메인 훅 `classify`/`approval_binding`/`presend_check`(A2) + 증거 술어 `EvidenceVerifier`(`uart.text_contains/absent/count/sequence`) 제공.
- **UART Python 0 확정**(pyserial → Rust `serialport` 4.10.1 + native 어댑터, `uart/plan.md` §7.6, DR-V19).

## core 답변
core-lead, 2026-11-10 (R3 통합) — 정본 `docs/plan/core/plan.md` v2.0 §15.2.

| # | 답 |
|---|---|
| 1 | 수용 — uart worker 별도 프로세스(판정 기준 W3: 취소 불가 블로킹). 포트 수 예산식: soft 48 + 8×포트 / hard 96 + 16×포트 MiB, 넘으면 open 거절(§9.3) |
| 2 | worker→gateway 적응형 배치(조용한 뒤 즉시, ≤ 10 ms) 수용, worker 프로토콜 바이트는 msgpack bin. **UI WS는 M1 JSON(base64), M2 협상 binary** — 의도적 단계 차이(지연 예산 안) |
| 3·14 | 수용 — viewer `uart.input`은 연결 태스크가 스폰 없이 worker ctl에 직접, Ctrl-C는 worker 우선 mailbox. STOP·`record.health=failed` 중에도 열림. 목표 UI 키 → worker p95 ≤ 5 ms, flood 중 Ctrl-C → sent p95 < 100 ms |
| 4 | 수용 — `input.batch{lease_id, channel, origin:user, actor_id, source_range, start/end_mono_ns, byte_count, lines:[{n,len,sha256}], redacted, line_complete, record_health}`, 줄 종결 또는 1 s/4 KiB(CR-C10) |
| 5 | 수용 — M1 viewer = 웹 bootstrap(URL fragment → POST 교환) → HttpOnly·SameSite=Strict 쿠키, 토큰 파일과 분리. 승인에 `presence{method, proof}`(M1 code = 화면에만 보이는 코드, 오클릭 방지용; M2 WebAuthn 옵션). 원격은 SSH 터널, LAN·TLS·페어링은 M5. TTY viewer는 M2에 필요한 도메인이 있을 때만(입력·hold·STOP만) |
| 6 | 수용 — worker 직접 세그먼트 저널 + gateway mapping 로그, 링은 고정 크기, 백로그는 디스크. `chunks.jsonl` sidecar 불필요 |
| 7 | 수용 — `now` = 접수 시 허브 high-water(CR-C3) |
| 8 | 수용 — 안정 crate `loom-proto`(이벤트·seq·커서·쓰기 상태), `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`. P0 Day 1–2 compile-only 공개 |
| 9 | 수용 — 원본 바이트 범위 읽기 `JournalReader::scan(query, byte_limit)`, 스캔 예산은 검증기 공통(검사당 ≤ 200k 이벤트·64 MiB·2 s) |
| 10 | 수용 — `HelpSpec`에서 생성 |
| 11 | 수용 — CORE 하네스, UART는 flood·재연결·attach/detach 부하 제공 |
| 12 | 수용 — `core.time`, `core.metrics.report`(이름 그대로) |
| 13 | 수용 — `core.viewer.presence`, `meta.viewers{subscribed, visible}`, 경고 `no_viewer_attached`/`no_visible_viewer` 구분 |
| 15 | CR-C12로 메인에 올림, **CORE 권고 승인**. 확정 전 임시 = exit 0 + `data.matched=false` |
| 16 | 수용 — `WriteHooks::within_pattern` + 사용권 원자 예약, R4 제외 |
