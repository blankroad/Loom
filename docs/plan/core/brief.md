# CORE 팀 브리프 (v2.0 신규) — Rust 코어

먼저 읽기: `02-core-principle.md`(최우선), `00-master-plan.md`(특히 §2 D1–D14, §2.1 A1–A16, §2.2 B, **§2.3 C1–C8**), `00-brief.md`, `01-lead-playbook.md`. 참고: `uart/plan.md`, `can/plan.md`, `uds/plan.md`, `explore/plan.md`의 core 관련 절과 각 팀이 CORE에 한 요청(§15 등), `_xteam/*.md`.

## 왜 Rust인가 (사용자 경험)
사용자는 Python을 코어로 썼을 때 **느리고 메모리 문제로 자꾸 죽는** 경험을 했다. 이 계획의 1순위 품질 목표는 **오래 켜 두어도 죽지 않고 메모리가 늘지 않으며, 장치→화면 지연이 낮은 코어**다.

## 핵심 원칙 (놓치지 말 것)
에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간에 UI에서 보고 개입한다. 코어는 이 루프의 배관이다: 장치 worker → 이벤트 허브 → (에이전트 CLI, 사용자 UI) 동시 전달, 쓰기는 정책·승인·감사를 거쳐 worker로.

## 범위
1. 프로세스 구조: `loomd` gateway + 장치 worker 프로세스(D1 유지·재검토). Rust 도메인을 gateway 안 crate로 둘지 worker 프로세스로 둘지 기준(벤더 DLL 크래시 격리, 타이밍 전용 스레드).
2. **worker 프로토콜**: Rust worker와 Python 어댑터 worker(CANoe COM 등)가 같은 프로토콜. 전송(파이프·소켓·공유 메모리), 직렬화(바이너리 권장 근거), 흐름 제어, 감독·재기동, 메모리 예산.
3. IPC(D2: JSON-RPC over WS, role·origin), 이벤트 허브(seq·snapshot·우선 큐·gap·적응형 배치 A5), 고속 데이터의 UI 전송(바이너리 프레임·열 단위 등 — UI 팀과 `_xteam/core-ui.md`에서 합의).
4. 저널·기록(D7): 무손실 디스크 저널, 인덱스, 조회, 재생, 보존. 형식 선택 근거(자체 세그먼트 vs SQLite vs Arrow/Parquet 등).
5. CLI `loom`(D5 봉투·종료 코드·커서), 데몬 자동 기동(D8), 기동 시간 목표.
6. 정책·승인·감사(D6, A8–A12, B4), STOP(A3), 쓰기 파이프라인과 도메인 훅(A1, A2, A14).
7. 스키마 정본 = Rust 타입 → JSON Schema → TS/Python 생성(C3).
8. 러너 코어(D12)와 **테스트 세션 코어**(C7) — 세션·단계·증거 인용 검증의 코어 부분. 형식·스킬은 TEST 기획(EXPLORE 팀)과 `_xteam/core-test.md`에서 합의.
9. C2 성능·메모리: 큐 상한 표, 메모리 예산, 24 h soak 관문, 크래시 격리 시험, 측정 하네스.
10. 빌드·배포: Windows x64·Linux x64 실HW, macOS 개발. 단일 바이너리 배포, CI 3 OS, crate 구조(workspace), 도메인 팀과의 경계(`crates/loom-core` 등 CORE만 수정).

## 산출물
`docs/plan/core/plan.md` (00-brief §4의 15절 형식, 맨 위에 핵심 루프 표). **P0 첫 산출물 = 스키마 + 인터페이스 stub crate**를 도메인 팀이 첫 주에 쓸 수 있게 하는 계획. 스파이크는 `spikes/core/`(예: tokio WS 처리량, 저널 쓰기 속도, worker 프로토콜 왕복 지연).
