docs/plan/02-core-principle.md 준수

# CORE R1 — core-opus 지시

먼저 `docs/plan/core/tasks/r1-common.md`를 읽고 그대로 따른다. 이 파일은 당신 몫의 관점과 추가 산출물이다.

## 관점 (15절 전체를 쓰되 아래에 무게)
- **아키텍처**: 프로세스·스레드·태스크 구성도(ASCII), 데이터 흐름(읽기 경로, 쓰기 경로, STOP 경로, 승인 경로 각각), 장애 시 흐름(worker 크래시, 저널 디스크 가득, UI 끊김, CLI 타임아웃 중 데몬 재시작).
- **사용자/에이전트 UX**: `loom` CLI 명령 트리(코어 소유 명령: `daemon`, `stop-all`, `status`, `subscribe`/`events`, `record`, `run`, `test`, `confirm`, `help --json`, `skills sync` 등), 사람용·`--json` 출력 예시, 에러 예시, 자동 기동 체감. UI 앱과의 관계(UI가 코어에서 받는 것: 구독·질의·쓰기·승인 대기열·STOP·env 배지·gap 상태).
- **스킬 설계**: 공통 `loom` 스킬(데몬, 봉투·종료 코드, 승인·안전, `others_since_last` 대응, UI 열기 안내)과 `loom <domain> help --json`이 정본이 되는 구조(C8). SKILL.md 초안 골격.
- **안전 규칙**: 쓰기 파이프라인 단계별 의사코드(policy → confirm/approval → audit → presend_check → backend → events → reply), 도메인 훅 trait 시그니처(A2), 승인 객체 바인딩, 감사 해시 체인 형식, STOP latch 의미, 기록 불능 시 동작(A12), 위협 모델.
- **스키마·계약**: 이벤트 레코드 Rust 타입 초안(D3 필드 전부), RPC 메서드 이름 규칙, 봉투 타입, 오류 코드 enum, 도메인 등록 trait(`DomainRegistry` 등)의 Rust 시그니처 초안. **P0 stub crate 목록과 각 crate의 공개 API 초안**(도메인 팀이 첫 주에 import할 것).
- **테스트 세션 코어**(C7): 세션·단계·마커·증거 인용(`evidence: {stream_seq 범위, 필터, 기대}`) 검증 알고리즘, 러너(D12/A15)와의 관계.

## 추가 산출물
1. `docs/plan/core/r1-opus.md` — 15절 전체 초안 (주 산출물)
2. `docs/plan/_xteam/core-test.md` — **초안 v0.1 (CORE 작성)**: 테스트 세션 코어가 제공할 RPC/CLI(`loom test start/step/mark/verdict/evidence/end` 등), 증거 인용 형식과 Loom의 검증 규칙(존재·일치·불일치 시 판정), 단계 마커 이벤트, 일괄 사전 승인 바인딩, 보고서에 코어가 제공할 데이터. 파일 맨 아래에 빈 `## test 의견` 절을 남겨 TEST 기획(EXPLORE 팀)이 덧붙일 수 있게. 이미 파일이 있으면 덮어쓰지 말고 `## core 의견` 절로 덧붙인다.
