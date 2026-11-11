DONE v2.0
- `plan.md` v2.0 = 마스터 §2.3 C1–C8 반영(바뀐 절 중심 재작성, 1800줄). 근거: `v2-opus.md`(구조·UI 패널·테스트·스킬), `v2-sol.md`(Rust crate·실현성·C2, 스파이크 `spikes/uart/rust/`), 검증 `v2-check-opus.md`·`v2-check-sol.md` 필수 항목 전부 반영. 결정 기록 §16.2 DR-V0–V25.
- Rust(C1): `serialport` 4.10.1 + native 어댑터(POSIX nonblocking·절대 deadline, Windows COMMTIMEOUTS, parity M/S·stop 1.5 확장), Rust `regex`+`memchr`, `vte` 정제기. **UART Python 0**. crate: `loom-console`(A4 공유, WASM 빌드)·`loom-uart-worker`·`loom-uart`(gateway). **포트당 worker 프로세스**(최대 8).
- UI(C6): **웹 UART 패널이 M1 1차**(xterm.js 터미널 + 트레이스 탭, 상태줄 배지, HOLD·입력 반환·takeover, 다중 탭 단일 writer). xterm.js 자동 응답이 사용자 키로 장치에 주입되는 문제 → 3중 방어. 지연 게이트 = 장치 RX → write cb 뒤 첫 rAF p95 < 100 ms(시계 맞춤). TUI는 M2 보조. 패널 명세 `_xteam/uart-ui.md` v1.2.
- C2: 큐 표(byte·count 동시 상한, 넘칠 때 정책·gap 층), worker RSS 48/64 MiB, 구독 채널당 4·전체 16, 패널 그리기 대기 2 MiB, 24 h 혼합 soak 관문(M3, 재발 방지 공지에 따라 개발 중 측정은 10분 이내).
- 통합 테스트(C7·C8): `expect --within/--absent/--count/--sequence`, 결과 `data.evidence[]`(TEST REF `uart:dut@seq:a..b`), CORE `EvidenceVerifier`용 술어 `uart.text_*`, TC-MIX-01 예. 스킬 ≤ 40줄 + `loom uart help --json` 정본. `_xteam/test-domains.md`에 `## uart 의견`(전부 수용).
- 타팀 요청: `_xteam/uart-core.md` v1(#1–19: 포트당 worker, viewer 자격, 입력 우선 경로, WASM 빌드 가능 core 타입, seq→시각 조회, exit 3 확장(미확정), 메모리 집행 등), `_xteam/uart-ui.md` 질문 15개(정제기 WASM, 셸 M1 승인 대기열 여부 등). CORE 제안 이름 `core.time`·`core.metrics.report` 수용.
- 남은 조건: UI 셸 M1이 늦으면 UART M1 화면이 없으므로 TUI 터미널 모드를 P1 임시 화면으로 앞당긴다(§13). 02 핵심 원칙 6항목 유지(v2.0 재판정).
