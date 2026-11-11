DONE v2.0
- 산출물: `docs/plan/core/plan.md`(15절 + 결정 기록 25행, 핵심 루프 표 맨 앞), R1 `r1-opus.md`·`r1-sol.md`, R2 `r2-sol-review.md`·`r2-opus-review.md`, R4 `r4-check.md`(PASS with fixes → 수정 18건 전부 반영), 스파이크 `spikes/core/`(worker IPC·WS 배치·저널 sync·CLI 왕복), `_xteam` 7개 파일에 `## core 답변`.
- 핵심 결정: `loomd` gateway + 판정 기준(W1–W5)에 따른 worker(CAN·UART 포트당·seed-key·LA는 worker, UDS는 gateway 전용 런타임 + 지터 p99 ≤ 2 ms 관문). worker 프로토콜 = 로컬 소켓 2개(ctl/data) + MessagePack map/bin + 64 KiB 프레임 + 바이트 크레딧, 원본 저널은 worker가 직접 기록.
- 신뢰성 수정(R2 P0): publish-before-reply = 허브 enqueue 정의(미발행 시 `E_PARTIAL`), stream mapping 로그 = 정본, 원본 손실 = 기록 불능(A12), 승인 원자 예약, STOP은 메모리 latch → worker → 영속 순서.
- C2: 큐 상한 표 Q1–Q29, gateway 바이트 장부 251 MiB(soft 256/hard 384), worker admission 1 GiB, 24 h soak 관문(수명 구간별 ≤ 1 MiB/h AND 중앙값 증가 ≤ max(16 MiB, 5 %)) — 잠정, 기준 머신 측정 후 확정.
- CLI: 동기 클라이언트, macOS 실측 p95 3.3 ms(생성→RPC→종료), 목표 왕복 30/60 ms(Linux·macOS/Windows).
- P0: Day 1–2 `loom-proto` + compile-only trait(태그 `core-p0-d2`), Day 3–5 testkit(FakeWorker·RegistryProbe·HookHarness)·echo·스키마/TS·`loomd --fixture-replay`(`core-p0-d5`).
- 메인 결정 요청 18건(§15.1 CR-C1–C18). 우선 결정 필요: CR-C12(exit 3 확장 — UART·UDS·TEST 공통), CR-C17(exit 11 `E_TEST_FAILED`), CR-C6(payload = 구조 객체), CR-C14·C15(저널 codec·보존 — CAN 5k fps JSON이면 20 GiB를 약 2 h에 채움), CR-C16(publish-before-reply 의미).
- 사용자 질문 Q1–Q6(§14): 벤치 PC 공유 여부, 원격 보기 빈도, 에이전트 샌드박스, soak 기준 머신 사양, 보존 우선순위(30일 vs 20 GiB), 승인 코드 입력 UX.
- 운영 메모: core-sol(Codex)은 재부팅 후 샌드박스 writable root가 옛 경로(`/Users/ctmctm/Desktop/01_PROJECT/Loom`, 링크)에 고정돼 모든 명령이 샌드박스 밖 승인을 요구 → 리드가 거절하고 R3 확인·R4를 core-opus로 처리. 구현 단계 전에 core-sol 세션을 새 경로에서 재시작해야 함.
