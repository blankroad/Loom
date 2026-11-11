# TEST P1 — sol session (2026-11-11)

## 구현·결정
- 코드: `.worktrees/test`의 `crates/loom-test/`, `skills/loom-test/`; format에 `inspect_plan`/trait-object 검사 API만 확장. 커밋하지 않음.
- `TestModule: DomainModule`, 22개 RPC, 10개 이벤트와 bounded reducer 구현. import/report/freeze는 명시적 P2 `E_NOT_IMPLEMENTED`.
- PLANNED→WAITING_APPROVAL→RUNNING⇄PAUSED→STOPPED/DONE. F1 기본 자동 통과; 정책 승인은 CORE hook의 실제 grant만 수용.
- TC/plan 텍스트·경로 파싱, 실제 CommandTable 위험 요약, plan amendment 시 승인·증거·시도 바인딩 초기화. TC만 있으면 동적 단계 마커 허용.
- `VerifierEngine::check/decide` 주입. 기본 NotWired는 PASS/done 불가. predicate crate에는 의존하지도 수정하지도 않음.
- 빈 PASS·own TX·미등록 expect·반복 검증 누락·불명 기록 상태/fence·잘못된 REF/range/filter·stale 검사 결과를 거절.
- override는 CallCtx viewer approve + 명시 flag/reason만. flag 생략·동시 판정으로 기존 accepted를 바꾸는 우회도 commit 시 거절.
- 거부 판정은 effective:null/rejected로 발행하고 기존 accepted 유지. 이벤트 publish 뒤 해당 상태를 반영; 실패 시 이미 발행한 prefix만 유지.
- 단계 자동 종료·bounded retry·분기/복구 집계·0/11/8/6 종료 코드·note/ask/answer/wait 구현. 사용자 abort는 STOPPED/incomplete/6.
- 정상 종료/agent 취소는 계획 finally 필요; viewer abort만 실행 여부 선택. 전역 STOP은 cleanup 송신 안 함.
- finally 미연결은 NotImplemented. dispatch 후 불명 결과는 자동 재송신 금지; 완료 후 종료 이벤트 저장 실패는 기록만 재시도. cleanup 뒤 resume 차단.
- 상수: active 기본1/최대4, 단계500, REF64, control1024(backpressure), 검사2, parser1, waiter64, 메모4KiB×1000, 열린 질문16, 최근 증거200.
- 64MiB는 엔진 RSS 목표와 논리 ledger 상한: 세션/임시 plan 12MiB + 요청·mutation 선차감, terminal 보존 최대8/예산에 따라 조기 퇴거.
- 실제 RSS 보장은 미측정(공유 프로세스·verifier·transport/pin 별도). 취소된 blocking task도 종료까지 permit/charge 보유. README에 한계 명시.
- CLI DomainCli·사람 출력·JSON 봉투·독립 `loom-test plan check` 실행 가능. `plan show <session>`은 RPC; YAML/--local은 로컬 파일.
- 명령표 없는 로컬 검사는 구조만 검사하고 “명령 존재 미검사” 경고; 가짜 위험 등급 없음. 스킬 36줄, Python 없이 frontmatter/상한/미완성 표식 검증.

## 검증
- 담당 crate 29개(엔진1/세션22/CLI6), format 14개(파서별 결정적 무작위 입력 10만 포함) 통과.
- TC-1 fixture start→plan.show 위험 수치, 모든 상태, PAUSED gate, 역할/증거 상한, 빈 PASS, 집계, reducer, CLI golden/실제 로컬 실행 시험.
- 추가: 4 client/STOP 반복 보존, control1024 포화·복구, 검사 동시2·pause/amend 경쟁, publish 실패, cleanup 중복 방지, 암묵 override 거절.
- 최종 workspace: cargo fmt --all --check 통과; clippy --workspace --all-targets -- -D warnings 통과; test --workspace 709 통과/0 실패/2 ignored.

## CORE 요청·질문
- 최신 `core-answers-p1.md`에서 for_core("test") 사용 승인 확인. 현재 API는 CORE가 만든 Registry를 받아 등록하며 loomd/loom 연결은 CORE 소유.
- RegistryProbe::run은 예약 domain을 거절함: 음성 시험 + for_core registry 실제 handler 시험으로 대체. 예약 domain용 probe 진입점 요청.
- CORE가 실제 CommandTable/argument schema/classify, journal 전수 조회·pin·worker fence·record health·STOP·승인/use-count adapter를 연결해야 실제 장치 검증 가능.
- 모든 도메인 쓰기에 check_client_write 적용, 허브 test_session/test_step/attempt 태그·operator_action·pause 요약·override 감사 연결 요청.
- finally는 정상 RPC/정책/기록 경로를 쓰되 ending gate 안에서 실행할 명시적 cleanup scope와 STOP/시간/잔량 검사 필요. 기본 hook은 송신하지 않음.
- CLI host는 DomainCli 등록 후 run_local을 daemon 전송 전에 호출해야 함(현 cli-kit에 local hook 없음); TEST에는 별도 장치 worker 없음.
- UI는 U64 timeout/revision/risk-count 문자열, plan_changed 단계 초기화, abort의 명시 run_finally, 다중 세션 선택을 수용해야 함.
- CORE workspace.dependencies 신규 외부 의존 요청 없음. session은 기존 workspace 의존과 format path만 사용.
- 최신 CORE 후속 결정: serde-saphyr 2.1.1 단일화·proto canonical 정본/feature 조각은 현재 test/main에 미합류. R1 통합 시 format YAML 전환·해시 동일성 및 feature API 정렬 필요.
- 실제 TC-1 sim E2E·내구 보존·RSS 측정은 CORE runtime 연결 후 관문이며 이번 가짜 CallCtx 시험을 그 결과로 주장하지 않음. 사용자에게 추가 질문 없음.
