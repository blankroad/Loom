MERGED into main (43ad0f6, 11-11 19:55) — 이전: MERGE READY: 29e4429 — eth: core-eth.md 1번 대응(gap.domain eth|source 임시 허용, 이 이행 커밋 한정). main(c4f70a3) 병합 후 fmt/clippy/test green(loom-proto msgpack unknown_fields_are_ignored 1회 실패 후 재실행 통과 — 간헐, CORE 확인 필요).
- TODO(추적): CORE R1이 main에 병합되면 즉시 crates/loom-eth/src/worker/tests.rs gap.domain 판정을 "eth"만 허용으로 되돌린다.
- M1 실제 loomd sim·웹 E2E: CORE R1 도메인 등록 대기.
