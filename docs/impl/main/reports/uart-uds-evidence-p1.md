기능 / 상태 / 담당: UART/UDS 실제 journal evidence verifier / READY_TO_INTEGRATE (리뷰 1회 CHANGES_REQUESTED 반영) — 소유 반환 / Herdr Claude Opus foreground 직접 수행(하위 agent 없음, 독립 리뷰 없음 — 자체 리뷰만)

목적 / 완료 조건: `uart.text_contains`, `uds.dtc_absent`를 원본 `PublishedRecord`로 재평가하는 `EvidenceVerifier`를 구현하고 도메인 모듈에 등록. 완료.
비범위: TEST wire(`crates/loom-test`), UDS 이벤트 발행(origin), manifest/lock, index/commit.

기준: main 6d41c24 + 미커밋 STOP/RSS/TEST 변경 보존(다른 담당 diff 미변경, 추가만).

## 변경 파일 (미커밋)
- 신규 `crates/loom-uart/src/evidence.rs` — `UartEvidence` (`uart.text_contains`, CoverageNeed::Positive)
- 신규 `crates/loom-uds/src/evidence.rs` — `UdsEvidence` (`uds.dtc_absent`, CoverageNeed::Complete)
- 신규 `crates/loom-uart/tests/evidence.rs`(6 tests), `crates/loom-uds/tests/evidence.rs`(1 test, 실 UdsModule + sim ECU)
- 등록 최소 변경: `crates/loom-uart/src/lib.rs` (+`pub mod evidence` gateway feature), `crates/loom-uart/src/module.rs` (+use, +`.evidence_verifier(Arc::new(UartEvidence))`), `crates/loom-uds/src/lib.rs` (+`pub mod evidence`), `crates/loom-uds/src/module.rs` (+use, +`.evidence_verifier(Arc::new(UdsEvidence))`). UDS lib/module의 기존 STOP 담당 diff는 그대로.
- UART 실제 gateway 위치 확인: `loom-gateway/src/domains.rs`가 `loom_uart::Uart::new()` / `loom_uds::UdsModule` 등록 → 위 module.rs가 실제 등록 지점. manifest 변경 없음(base64는 `loom_proto` `PayloadBytes::decode`, DTC 파서는 기존 `loom_uds_codec` 사용).

## 술어 계약 (TEST plan 모델·`test-runtime-p1.md` 부록 기준)
공통: `args` = plan pred − `op` + `filter` + `ch`; 단일 키 형식 `{"<name>": {...}}` 중첩도 수용. 미지 인자·형식 오류 → `Inconclusive{"error: ..."}`(TEST에서 `error`). `filter.kinds`/`origins`/`channel`을 verifier가 직접 적용, `origins` 비면 기본 `["device"]`. role = `payload.attributed_origin` 우선, 없으면 `origin`. 예산은 256 이벤트 단위 `spend`, 초과 → `Inconclusive{"eval_budget: ..."}`.

`uart.text_contains{pattern|literal}`
- `uart.data` + `dir=rx` + role∈origins + 채널 일치만. TX는 증거 아님(tx_skipped 카운트).
- RX 조각 literal join은 채널별 `(stream epoch, producer_id, producer_epoch, generation)` 동일 + `rx_offset` 연속일 때만. gap 레코드(`source/record/display.gap`), `uart.state`, 읽을 수 없는 bytes(raw_ref·secret·redacted), offset 점프에서 끊김 → 손실 너머 매치 불가.
- 매치 → `Match{matched = 매치 바이트를 소유한 rx 레코드들}`, observed = seq·rx_offset 범위·카운트만(RX 텍스트 미포함: echo된 secret 노출 방지).
- 미매치: coverage incomplete 또는 창 안 손실 → Inconclusive; RX에 ESC 존재 → Inconclusive(ANSI strip 미평가); 그 외 완전 창 → Mismatch. 빈 창도 Mismatch(PASS 아님).
- 미지원(명시 error): `regex`, `ignore_case`.

`uds.dtc_absent{mask}` (mask = 정수 또는 `"0x09"` 문자열)
- 응답으로 인정: 비-late `uds.rx` `dir=rx` `payload.kind=positive` `59 02 …`, role∈origins, 같은 `request_id`의 요청 행(`uds.tx/pending/result`)이 창 안에 있고 채널·generation·bytes 일치, 요청이 `19 02 <mask>`(SPRMIB 무시)이며 요청 마스크와 ECU availability mask가 술어 mask 비트를 모두 포함, `uds.result.resp_bytes`가 있으면 rx bytes와 동일, `loom_uds_codec` 파서 통과.
- 같은 채널의 비-late 양성 `54`(ClearDTC) 응답 이후의 목록만 판정(TC-1 S5: clear → list). 이전 목록은 superseded 카운트.
- 판정: 인정 응답 중 status & mask ≠ 0 DTC가 하나라도 → Mismatch(불완전 창이어도 확정). 인정 응답 0 → Inconclusive(빈 창·row 없는 rx·다른 origin·마스크 부족 등 이유 기재). DTC 없음 + (coverage incomplete 또는 gap) → Inconclusive. 그 외 → `Match{matched = 인정된 uds.rx}` → TEST witness `device`.

## 검사 (CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target, 현재 미커밋 트리)
- `cargo test -p loom-uart -p loom-uds` — 전부 통과 (uart evidence 6, uds evidence 1, 기존 uart/uds 테스트 포함 e2e 7, uds 7, registry 3 …)
- `cargo clippy -p loom-uart -p loom-uds --tests` — 경고 0
- `rustfmt --check --edition 2024` 신규 4파일 + uart lib/module — OK (워크스페이스 전체 fmt 미실행)
- `cargo test -p loom-gateway --test test_services_smoke --test test_runtime_smoke --test uds_stop_smoke` — 6/2/1 통과 (gateway default feature에 uart·uds 포함 → 실제 daemon 등록 경로 컴파일·동작 확인)
- UDS 테스트는 실 `UdsModule`+sim ECU 드래프트를 gateway 규칙(producer `gateway.uds`, dir=rx → origin device)으로 레코드화: 기본 DTC(923400/0x09, C10000/0x2F) → Mismatch, `14FFFFFF` 후 목록 → Match(witness uds.rx device), dirty+clean 한 창 → Match, 불완전/gap/빈 창/rx-only(forged)/agent origin/late/마스크 0x01/row·rx 불일치 → Inconclusive, 잘못된 args → error.

## 남은 문제 / 다음 행동 (메인)
1. UART는 현재 `fence_unflushed(uart)`로 TEST가 verify 전에 uncertain 처리(부록 3항) → 실 daemon에서 `uart.text_contains` 경로는 fence 통일 후에만 도달. 메인 진행 중.
2. UDS rx origin: 메인이 `Services::publish` dir=Rx→device 보장으로 해소 진행 중. verifier는 role 엄격 적용(agent origin rx는 증거 아님) — 해소 전 실 daemon에서는 TC-1 S4가 Inconclusive.
3. UART literal 검색은 raw bytes(ANSI 미제거, loom-console 의존 추가 불가). ESC 포함 미매치는 Inconclusive로만 처리. worker expect matcher와 같은 정제가 필요하면 manifest에 `loom-console` 추가를 메인 판단 요청.
4. (해결, 리뷰 반영) 검색은 아래 "리뷰 반영" 참조.
5. 실 gateway end-to-end(TEST check → verifier → verified)로 TC-1 S2/S4 PASS를 보는 smoke는 미작성(TEST/gateway 소유 파일). 통합 시 메인 smoke에 추가 권장.
6. `uart.text_absent/count/sequence`, `uds.response_eq`는 범위 밖(미구현).

## 리뷰 반영 (CHANGES_REQUESTED → 수정)
- UART: `hay.windows(..).position`(O(N·M), 256 이벤트 사이 시간 체크 없음) → 채널 stream별 **KMP 오토마톤**(failure table 1회, 바이트당 O(1) 상각)으로 교체. 매치 바이트 소유 레코드/`rx_offset`은 최근 `pattern.len()` 바이트의 VecDeque로 추적(join·offset 계산 동일). chunk 안에서도 4096 바이트마다 `budget.spend(0)`로 시간 확인 → `Inconclusive{"eval_budget: ..."}`.
- UDS: clear pass와 응답 pass(각 레코드 최대 64 KiB hex decode)에 64 이벤트마다 `budget.spend(0)` 시간 확인 추가(이벤트 수 차감은 1st pass에서 1회만).
- 회귀 테스트:
  - UART `long_repeated_needle_is_linear`: 4096 B needle `a…ab` × 64 chunk·60 000 B `a`(≈3.8 MB) → Mismatch(< 5 s, 실측 수백 ms debug), 다음 chunk `b`로 join 매치 offset 정확.
  - UART `time_budget_is_checked_inside_long_chunks`: 만료된 budget·1 ms budget(65 이벤트 < 256 → chunk 내부 체크만이 멈출 수 있음) → eval_budget.
  - UDS `later_passes_respect_the_time_budget`: 300 × 16 KB 양성 rx, 만료/1 ms/이벤트 10개 budget → eval_budget.
  - 뮤테이션 확인: UART chunk 내부 체크, UDS 후속 pass 체크를 각각 임시 제거하면 1 ms 단언이 실패함을 확인 후 원복.
- 재검사: `cargo test -p loom-uart -p loom-uds` 전부 통과(uart evidence 8, uds evidence 2), clippy 경고 0, 신규 4파일 rustfmt OK, gateway `test_services_smoke`/`test_runtime_smoke`/`uds_stop_smoke` 6/2/1 통과.
- 실제 TC 통합(TC-1 S2/S4 live)은 tc1-live 담당. proto/manifest/lock 미변경 유지.

커밋·푸시: 없음(지시대로 index/commit/push 미조작). 세션·로그 보존, 실제 장비·기존 demo 미접촉.
