IN PROGRESS: CORE 선행 런타임까지 병합, main 50c109e Rust 전체 963 통과. 사용자 지시로 Sol 조율자→Opus 워커 구조 전환 및 별도 탭 스킬 제작 진행.

# 메인 통합 기록

- 시작 기준: main abe79dd, 2026-11-11 19:26 KST. 임시 ref 생성·삭제 성공(.git 쓰기 확인).
- 인계 기준: docs/impl/01-main-orchestrator-handoff.md §0·§4 및 핵심 원칙/마스터 F > E > C > B > A > D/플레이북. 팀 STATUS와 관련 P1 계획 확인.
- 상태 문서 스냅샷: 9827418.
- UI: main c4f70a3에 ui/main 728a99e까지 병합. fmt·clippy 및 pnpm check(358) 통과. workspace test는 proto unknown_fields_are_ignored에서 실패(arbitrary_precision 기능 조합); CORE 최소 수정 진행 중.
- UI E2E: 메인 sandbox에서 Chromium 시작이 macOS MachPortRendezvous Permission denied(1100)로 실패. UI 리드에게 동일 UI tree의 e2e/mock/replay 검증·SHA 포함 보고 요청. 아직 성공으로 판정하지 않음.
- CORE: 실제 loomd·IPC 런타임 선행 묶음 8e2b4dc의 MERGE READY 요청. R1 도메인 등록·PerChannel·입력·자산 연결이 P1 임계 경로.
- UDS: reports/p1-sol-review.md의 P1 반례 6건을 리드에게 우선 수정·재현 검증 요청(이전 응답의 거짓 confirmed/송신 취소 포함).

## P1 필수 증거

- [ ] 실제 loomd + UART sim: 에이전트 송신 → 장치 응답 → 웹 화면 → 사용자 개입.
- [ ] 실제 loomd + CAN sim: 위 네 요소와 화면 지연 증거.
- [ ] 실제 loomd + UDS sim: 위 네 요소, 거짓 성공 없는 응답 판정.
- [ ] 실제 loomd + eth sim: 패킷 목록·상세·송신·사용자 개입.
- [ ] TC-1 혼합 TC: 에이전트가 자연어 해석 후 도메인 명령으로 실행, 실 세션·저널 증거로 판정, 웹 테스트 화면 확인.
- [ ] 정식 30분 RSS 관문: 연속 증가 ≤ 10 MiB. 개발 10분 관측과 구별하며 실행 구성·SHA·수치를 남김.

모든 항목의 증거가 모이기 전에는 P1 DONE으로 표시하지 않는다. 팀 STATUS/blocked 상태를 2분 간격 감시하고, 준비 신호마다 스냅샷 → 병합 → 검사 → MERGED 기록 → 전 리드 통보를 반복한다.

## 19:39 후속 상태

- UI 동일 SHA 검증: reports/verify-main-c4f70a3.md. pnpm check 358·기본 e2e 4 통과/3 성능 skip. 확장 mock/replay는 리드 한도로 미실행.
- Claude 리드·opus 일부가 22시까지 사용량 한도로 대기. 가능한 Codex 워커에게 기존 범위의 CORE 긴급 수정·UART R1 연결 준비·TC-1 하네스 준비를 배정했다.
- 신규 준비 신호: UART 4258f75, ETH 29e4429, CORE 8e2b4dc. CORE는 ETH/UART 기존 호환 문제를 함께 해결해야 전체 시험 가능.
- 19:43: P3 선행 loom-viz(d16f32d)를 선택 revert한 7789f51로 전체 Rust 재검증 중. UI 자체 tree는 c4f70a3과 같으며 기존 단위·기본 브라우저 증거는 유효. P3 재통합 전 arbitrary_precision의 IPC 숫자 표현 영향 확인 필요.

- CAN aa74f0e → main 47a792e: Cargo.lock만 ours+metadata로 해결, fmt/clippy/workspace test 통과.
- 추가 P1 추적: CAN reports/m1-load.md의 worker 재생성 generation 재사용·유한 캡처 tail gap 미발행 반례는 아직 해소 전이며, 기본 green만으로 P1 수용을 선언하지 않는다.

- CORE 진단 확정: arbitrary_precision은 시험 도구뿐 아니라 실제 SDK IPC payload의 MessagePack native 숫자 표현도 바꾼다. docs/impl/core/reports/proto-ap-feature-analysis.md 참조. P3 선택 revert 유지, 진단용 시험 패치 미반영.

- UART 4258f75 → main afd8894: fmt/clippy/workspace test 통과. 다음은 TEST 071f298 병합.

- TEST 071f298 → main 8fe47f3: fmt/clippy/workspace test 통과. 초기 네 브랜치 통합 완료. UI 확장 mock/replay 및 실제 P1 시연·TC-1·30분 RSS는 미완료.

- ETH 29e4429 → main 43ad0f6: fmt/clippy/workspace test 전체 통과(19:54:57). CORE 런타임 선행 통합 준비.
- 토큰 점검 완료: reports/token-audit-2026-11-11.md. 재개 시 모든 팀은 tasks/token-budget.md 적용(의존성 미준비 반복·상시 모델 감시·P2/P3 선행 보류).

- 사용자 확정 구조(후속): main·팀 서브 오케스트레이터 = Sol 6.1, 구현 워커 = Opus 5.5. 오래된 큰 대화를 넘기지 않고 ORCHESTRATOR-HANDOFF.md로 새 조율자 세션을 시작한다. 기존 소스·세션 로그는 보존한다.
- 별도 스킬 탭 w8:t2(스킬 · Sol-Opus 운영): skill-orch Sol 6.1 medium / skill-opus Opus 5.5. 작업 지시·결과 경로 ~/.cache/sol-opus-orchestration-skill. 워커는 22시 한도 해제 대기.
