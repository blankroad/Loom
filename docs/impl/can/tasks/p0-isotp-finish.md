# can-opus — `crates/loom-can-isotp` 마무리 (can-sol 대신)

먼저 `docs/impl/can/tasks/p0-common.md`(공통 규칙)와 원래 지시 `docs/impl/can/tasks/p0-sol-isotp.md`를 읽는다. can-sol이 이 crate를 거의 다 만들었지만 재시작으로 오프라인이다. 범위: **`crates/loom-can-isotp/` 폴더와 보고서 `docs/impl/can/reports/p0-isotp.md`만.** 경로는 항상 `/Users/ctmctm/workspace/Loom/...`(옛 Desktop 경로 금지).

## 현재 상태 (리드 확인)
- 미커밋. `src/codec.rs`, `src/engine.rs`, `src/lib.rs`, `tests/protocol.rs`(시험 24개), `README.md`.
- `cargo fmt --check`·`clippy -D warnings` 통과, `cargo test` 24개 통과(디버그 빌드 58 s — 너무 느림).
- abort 세대(`generation`) 울타리는 있다(`on_frame_in_generation`). can-sol의 마지막 미적용 단계: **"stale-FC handle guard + regression"** — RX가 새 SF/FF로 교체되면 세대는 그대로인데 이전 수신에 대한 FC 요청 handle이 남을 수 있다(`engine.rs` 217행 주석 참고). 그 handle이 제출·확인 처리되거나 새 수신 상태를 오염시키지 않게 막고 회귀 시험을 추가한다.

## 할 일
1. 원래 지시의 요구 1–10을 코드와 대조해 **빠진 것·틀린 것**을 찾는다(특히 STmin 예약값 → 127 ms, FD SF escape, FF 4096 거절, WAIT/`wft_max`, N_Bs/N_Cr, 결함 훅 5종, 출력 큐 상한, `DriverAccepted`/`BusConfirmed` 규칙). 고칠 것은 고치고 시험을 붙인다.
2. 위 stale-FC handle guard + 회귀 시험.
3. **시험 시간**: 디버그 58 s의 원인(아마 큰 왕복 행렬·proptest)을 찾아 기본 `cargo test`가 ~10 s 안에 끝나게(무거운 행렬은 `#[ignore]`로 두고 보고서에 실행법, 또는 케이스 수 축소 — 커버리지를 잃지 않게 경계값은 남김).
4. 공개 API가 README와 맞는지 확인.
5. 보고서 `docs/impl/can/reports/p0-isotp.md`: 공개 API 요약, 시험 목록·결과, can-sol 작업 대비 고친 점, 계획과 다른 점, 리드 결정 요청.

품질 기준은 공통 규칙 그대로(fmt·clippy·test, unsafe 금지, C2 상한). 커밋하지 않는다. 끝나면 멈춘다.
