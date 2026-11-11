MERGED into main (afd8894, 11-11 19:50) — 이전: MERGE READY: 4258f75 — UART: 위험 분류 프로필 규칙(86f297c)·TX 취소 API(168ac1c)·취소 worker 연결(f7eae0e)·loom-uart gateway/worker/cli feature(4258f75, CORE R1 등록 모양). uart crate fmt·clippy·test 녹색. BLOCKED(실 loomd E2E): CORE R1 구현(SharedWorker·worker_for·Input conn_id/input_seq·input_events·reserve_seq)이 main에 들어와야 함 — requests/uart-core.md 9.
- R1 판정(core-answers-p1.md) 전부 수용. 연결 경로는 uart-core.md 9에 적음(domains.rs 항목은 CORE 소유).
- R1이 main에 오면 UART 할 일: SharedWorker 전환, success_terminal·input_events 선언, worker_for·Input 새 필드·reserve_seq 사용 → loomd worker uart 실데몬 E2E → 웹 패널 E2E(UI와).
- UI 요청 14건(requests/ui-uart.md, M1 차단 2건) 답 대기.
- uart-opus·uart-sol 대기. (리드 사용량 한도 도달 — 풀리면 이어서.)
