MERGED into main (c4f70a3; P3 보류 7789f51, 11-11 19:45) — 이전: IN PROGRESS: main c4f70a3 UI 검증 — pnpm check 통과(358)·e2e 기본 4 통과/3 skip, e2e:mock·e2e:replay는 리드 한도로 미실행(reports/verify-main-c4f70a3.md). 보류: #boot= 유지(CORE R1 Q18 확정), `/` vs `/ui/` 자산 경로는 core-lead와 문서 합의 후 수정.
MERGE READY: 19c6276 — UI P1 M1(sim·mock 기준): 셸·UART·CAN·UDS·eth 패널, shell.test·psu.gauge(M1b), 생성 타입(schema/v1), crates/loom-ui-assets, M1 수용 스위트. ui/와 crates/loom-ui-assets만 변경(+ Cargo.lock 새 항목: rust-embed·mime_guess).
- 시험: `pnpm check`(schema drift + tsc + eslint + vitest 325) · e2e 기본 4(+3 perf skip) · **e2e:mock 44 통과 + 1 skip(M1 수용 UI-T01–T10, eth 1–7, shell.test 5, psu)** · **e2e:replay 11(CORE `loomd --fixture-replay`; 2건은 CORE 차이 C-4·C-5로 test.fail 표시)** · `cargo test/clippy/fmt -p loom-ui-assets`.
- 남은 P1 관문 조건: CORE P1 실제 gateway에 `loom_ui_assets::router()` merge(요청함) 후 실 loomd + sim 도메인 worker로 같은 수용 스위트, 실 브라우저 CSP 확인. CORE 차이 C-3–C-10 회신 대기(requests/ui-core.md).
- 진행 중(미커밋): ui-opus = 수명주기 DOM 증가 확인(T10 1,137→2,156) + 도킹 E2E · ui-sol = 같은 D3 배치로 JSON vs columns 비교(Node) — 디코더(728a99e)가 JSON보다 느릴 수 있어 채택 판단 근거용.
- MERGE READY 이후 커밋(병합해도 무해): b514d83 dockview-core 의존성 · d16f32d crates/loom-viz 데이터셋 엔진(P3 선행, 시험 23) · a96dc47 도킹·레이아웃 저장(e2e:mock 44+1 skip 유지) · 728a99e `loom.columns.v1` 제안 디코더·골든·퍼즈(net 미연결, 제안서 docs/impl/ui/proposals/). main 병합 시 loom-ui-assets 의존성을 CORE의 workspace deps(29f20c3)로 전환 예정.
- 요청 파일: ui-core(Q1–Q21, C-1–C-12, assets mount), ui-uart, ui-uds, ui-eth, ui-can(반영), ui-test, ui-explore, ui-domains.

- main Rust fmt/clippy/workspace test 전체 통과(7789f51). pnpm check 358 및 기본 e2e 4 통과/3 성능 skip. 확장 e2e:mock/replay는 리드 한도로 대기하며 P1 수용 완료로 보지 않는다. P3 loom-viz는 arbitrary_precision이 MessagePack 기능 조합을 깨뜨려 d16f32d만 선택 revert; UI 브랜치 원본 보존.
