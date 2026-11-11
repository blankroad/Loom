# 계약 변경 이력

형식: 날짜 · 버전 · 변경 · 근거 · 영향 팀. 파괴적 변경은 **[BREAKING]** 표시.

## 2026-11-10 · v1.0 (P0, `core-p0-d2`)
- 계약 v1 최초 분리(E12). 마스터 §2 D1–D14, §2.1 A1–A16, §2.2 B1–B7, §2.3 C1–C8(+C4.1), §2.4 E1–E12, §2.5 F1·F2를 통합.
- 주요 내용: F1 기본 모두 자동(확인·승인·존재 확인은 설정으로), 종료 코드 3 확장(E2)·11 `E_TEST_FAILED`(E3), payload = kind별 구조 객체(`payload.bytes`, `payload.write_state`), stream_epoch/generation 분리, publish-before-reply 의미(E7), 저널 세그먼트 + CRC32C + codec id(E5), 보존·감사 계정(E6), viewer rights 3단(E9), localhost 오리진(E8), UART 포트당 worker.
- 타입 정본: `crates/loom-proto` v0.1.0.

## 2026-11-11 · v1.0.1 (문서 보정, 타입 변경 없음)
- 03: 봉투 예시를 실제 직렬화(선택 필드 생략)로, `meta.visibility` 추가 설명. 근거: `loom-proto` 3ab73b6 구현.
- 01: `core.snapshot_part/end` 페이로드 형식 정의(타입은 d5), RPC 이름 2–3토큰 명시.
- 02: U64 JSON 정규형(선행 0 금지, 숫자 입력 거절).
- 07: LA 사후 생성물 REF 파서는 P2.

## 2026-11-11 · v1.1 (UI 질문 Q1–Q4 반영, `core-p0-d5`와 동기 예정)
- 01: 셸 전역 제어 구독 와이어 = `core.events{sub:"control"}`, 우선 kind 판정 함수 공유. snapshot/heartbeat/closed 타입은 `loom-proto`(d5).
- 01·04: **STOP·비상 OFF는 watch viewer도 가능**(안전 방향, 에이전트도 가능). STOP 해제는 operate. 근거: UI Q3.
- 04: 존재 확인을 켠 R4는 approve rights와 별도로 WebAuthn 등록 게이트 필요(UI Q4).
- 영향: UI(권한 표), CORE gateway.

## 2026-11-11 · v1.2 (UI Q7–Q13, eth 요청)
- 01: subscribe 응답이 snapshot보다 먼저(Q8), `snapshot_part.n`은 1부터·형식 하나(Q7), 재연결 `after_seq`+`stream_epoch`·서버 판단 resync(Q9), 우선 kind는 control로만 한 번(Q10), `core.closed.retry_after_ms`(Q12, loom-proto `Closed`에 필드 추가).
- 02: 쓰기 종단 행 `payload.final: true`(Q11, 도메인이 성공 종단 선언), gap 이벤트의 domain·channel = 영향받은 스트림(Q13), eth gap cause·`ts.source` 4개(`pcap_host`, `pcap_adapter`, `replay`, `sim` — loom-proto `TsSource`에 추가).
- 01: eth worker는 별도 실행 파일 `loom-eth-worker`(cap_net_raw 최소 권한). 06: 저널 codec 레지스트리 `json/1`·`msgpack/1`·`pcap-epb/1`.
- workspace 의존성: regex, etherparse, pcap, ipnet 추가(eth 요청).

## 2026-11-11 · v1.3 (P1 gateway·journal 구현 결정)
- 01: 도메인 RPC 결과 = JSON-RPC `result`에 봉투 전체, idempotency 키 `params._loom.idempotency_key`, stream_epoch = 데몬 시작 unix 초.
- 02: `Registry::success_terminal(method, state)`로 성공 종단 선언(loom-domain-api 추가, 비파괴), `<domain>.input` 이벤트·RPC는 코어 제공.
- 06: active 세그먼트 `.lj.open`, hint 레코드, CRC 범위 명시.

## 2026-11-11 · v1.4 (worker 프로토콜 1.1, 저널 채널 상한)
- 05: `loom-worker/1.1` — Job*·Input에 선택 `call_id`(와이어 하위 호환). **Rust 구조체 리터럴은 `call_id: None` 추가 필요** — 영향: uart(`crates/loom-uart-worker/tests/worker_e2e.rs`, `docs/impl/requests/core-uart.md`).
- 06: 저널 writer가 채널(class)별 상한을 따로 집행(한 채널 폭주가 다른 채널 기록을 막지 않음), `GapReport.class` 선택 필드.
- 경계 검사 `loom-xtask`(CLI가 장치 crate를 링크하지 않음, 도메인은 안정 API만, lints 상속)와 3 OS CI 설정 추가.

## 2026-11-11 · v1.5 (정상 종료)
- 05: 정상 종료 = `Latch{reason:"shutdown"}` 후 연결 닫기 → worker 즉시 seal·종료(장치를 바로 놓음), shutdown latch는 비영속. 크래시는 orphan 경로 유지. 와이어 변경 없음.
