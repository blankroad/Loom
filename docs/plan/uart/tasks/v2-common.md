docs/plan/02-core-principle.md 준수 (최우선)

# v2.0 재계획 공통 지시 — uart-opus · uart-sol

## 배경 (반드시 먼저 읽기)
1. `docs/plan/02-core-principle.md` — 에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 **같은 데이터를 같은 순간 UI에서** 보고 개입한다.
2. `docs/plan/00-master-plan.md` **§2.3 C1–C8** (우선순위 C > B > A > D). 요지:
   - C1 코어·장치 경로 = **Rust**(`loomd`, `loom` CLI 단일 바이너리, UART 백엔드 포함). Python은 대안이 없을 때만, 별도 프로세스 어댑터.
   - C2 모든 큐 상한 + 넘칠 때 정책(drop·block·spill), 링 버퍼 고정, 원본은 디스크 저널로, worker 메모리 예산, **24 h soak**(CAN 고부하 + UART flood + UDS 폴링 동시)에서 RSS 증가 상한이 관문, worker 크래시 격리.
   - C3 D2·D3·D4/A14·D5·D6·D7·A1–A16·B1·B4·B6 계약은 유지. 스키마 정본 = Rust 타입(serde) → JSON Schema → TS. D10(Python entry point)은 폐기 → Rust 도메인 crate 등록 + 외부 어댑터 worker 프로토콜(CORE가 결정).
   - C6 사용자 화면 = **Loom UI 앱(웹, `loomd`가 127.0.0.1로 제공, 같은 WS 프로토콜)이 1차**. 공통 셸(STOP·승인 대기열·env 배지·연결/gap 상태·기록 탐색·테스트 실행 화면) + 도메인 패널. **M1 뷰어부터 웹 UI**, TUI는 보조. UI 팀(w7)이 셸·패널 프레임워크 소유, **UART 팀은 패널 내용 명세와 데이터 질의 소유**.
   - C7 TC 기반·자연어 기반 **통합 테스트**: 사람이 자연어/TC 표로 쓰고 에이전트가 `loom` 명령으로 실행, 단계 판정은 **증거(이벤트 seq 범위 등) 인용**, Loom이 인용 증거 검증, 보고서, 세션 일괄 사전 승인.
   - C8 스킬 = 도메인 스킬 + 공통 `loom` + `loom-test`. 스킬 본문은 짧게, 상세는 `loom <domain> help --json`이 정본.
3. `docs/plan/04-v2-tasks.md` (공통 1–5, UART 항목).
4. 현재 계획서 `docs/plan/uart/plan.md` v1.2 (1328줄 — 필요한 절만 골라 읽기: §0, §2.0, §3, §4, §5, §9, §11, §12, §16.1).
5. 새 팀 브리프: `docs/plan/core/brief.md`, `docs/plan/ui/brief.md`. 그 폴더의 진행 중 초안(`plan.md`, `r1-*.md` 등)이 있으면 **읽기만** 해도 된다.

## 규칙
- 쓰기는 `docs/plan/uart/` 안의 지정 출력 파일만. 다른 팀 폴더·`_xteam/`은 읽기 전용(리드가 `_xteam`에 옮긴다).
- 사실은 [확인](문서·소스·실험 근거 명시) / [추정] / [제안] 표기. 버전·날짜는 실제 확인한 것만.
- 기존 v1.2 설계 중 언어와 무관한 것(입력 중재 규칙, 엄격 신원, 세대 경계, 커서, 위험 매핑, 손실 3층 등)은 **유지**가 기본이다. 바꿀 때만 이유를 쓴다.
- 짧고 바로 붙여 넣을 수 있는 한국어 문안으로. 리드가 plan.md에 통합한다.
