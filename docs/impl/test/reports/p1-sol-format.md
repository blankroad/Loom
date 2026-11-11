# TEST P1 보고 — explore-sol: loom-test-format (2026-11-11)

**리드 메모**: format 구현·자체 관문 완료, workspace test 662개 PASS. workspace fmt/clippy는 다른 담당 predicate 파일 때문에 FAIL(아래). 배정 범위 밖 코드는 수정하지 않음.
코드: `/Users/ctmctm/workspace/Loom/.worktrees/test/crates/loom-test-format/`만. 순수 라이브러리, 파일/장치 IO·실행·가짜 CORE 없음. 커밋 없음. Cargo.lock은 Cargo가 두 신규 TEST member/의존성 등록으로 갱신.

## 의존 결정 (구현 전에 crates.io·원 저장소 확인)
| crate | 확인한 판 / 최근 릴리스 / 라이선스 | 결정 |
|---|---|---|
| [serde_yaml](https://crates.io/crates/serde_yaml) | 0.9.34+deprecated / 2024-03-25 / MIT OR Apache-2.0 | [원 저장소](https://github.com/dtolnay/serde-yaml) archived·유지보수 중단 명시, 제외 |
| [serde_yaml_ng](https://crates.io/crates/serde_yaml_ng) | 0.10.0 / 2024-05-26 / MIT | 비교 후보, Norway보다 릴리스 오래됨 |
| [serde_norway](https://crates.io/crates/serde_norway) | 0.9.42 / 2024-12-21 / MIT OR Apache-2.0 | `=0.9.42` 선택; non-archived fork·후보 중 더 최근 릴리스(현재 유지보수 보장은 아님) |
| [pulldown-cmark](https://crates.io/crates/pulldown-cmark) | 0.13.6 / 2026-10-30 / MIT | `=0.13.6`, GFM tables·offset iterator |
| [sha2](https://crates.io/crates/sha2) | 0.11.1 / 2026-10-12 / MIT OR Apache-2.0 | `=0.11.1`; serde/serde_json은 workspace 상속 |
다운로드 정상 완료. CARGO_HOME=`$test_worktree/.cache/cargo`, CARGO_TARGET_DIR=`$test_worktree/target`; 이후 검사는 --offline.

## 공개 API·완료 범위
- `parse_tc[_bytes]` → TcDoc(meta/description/steps/source_sha256), 선택 머리말·자연어만 입력, 한/영 표 별칭. 셀 원래 Markdown·byte span·source_line 보존. description은 손실 방지를 위해 표도 포함한 원래 본문 전체.
- `parse_plan[_bytes]`, `parse_plan_with_tc` → immutable Plan/getters. TC hash 확인/결속, 공유 단계·do/expect/on_fail/when/retry/repeat/manual/measure/finally, 누락·타입·중복 YAML 키 오류.
- `CommandTable: RiskProvider` + `check_plan` → Report. 명령 조회/선택 인자 스키마 hook, timeout, id/참조/순환, blocked 및 finally 검사. pred/measure JSON은 보존하고 해석하지 않음.
- 위험은 실제 호출자 classify 결과만 사용; 분기 양쪽·finally 포함, 각 명령 max_uses=`repeat.count × (1+retry.count)`. risk_summary는 무시·경고. 정적 valid는 승인/증거 PASS가 아님.
- TC-1~3의 .tc.md/.plan.yaml 쌍, UDS bcm-coding 시나리오, 순환/timeout/501단계/표 열 누락 fixture 포함. TC-1은 계획 §2 YAML과 실제 TC 원문 SHA-256으로 결속.

## 상한·결정
- 파일/정규 JSON 각각1MiB, steps+finally 합계500, 셀/질문4KiB, AST 깊이32·노드65,536·확장 charge8MiB(노드당128B+문자열), collection8192·scalar64KiB, 행동/expect 각각64, 단계id/메서드128B.
- retry는 추가3회, repeat1..100회, timeout/지연1..3,600,000ms. 진단1024·등록기 오류문4KiB, 위험 항목≤500×64. 초과는 오류(원문 자르지 않음); AST·별칭 복사와 JSON 출력 전에 예산 검사.
- YAML backend의 문서 이벤트 선할당도 lexical token131,072/indent256/flow-depth32 사전 검사로 제한. 비문자 키·비유한 수·custom tag·다중 문서·재귀 별칭 거부. source syntax는 backend mark, 정적 오류는 해당 단계 행+JSON pointer.
- sleep/wait는 유한 러너 지연으로 분리(R0), RPC 아님. 대기 명령은 CommandSpec.waiting 기준 timeout 필수; manual에도 timeout 필수. finally는 R0–R1 **AND safe_state**, 뒤로 goto 금지(정상 fallthrough도 순환 검사).
- UDS runner에는 YAML loader/fixture가 없어 UDS 계획 §6.5 시나리오를 fixture로 사용. target/save/vars/on_error를 보존·반영. 구식 expect map은 원값 pred+legacy 표시/경고, `requires_legacy_adapter=true`; 실행 전 UDS 어댑터 변환이 필요하며 검증 완료로 위장하지 않음.

## 해시 규약 (predicate 담당도 동일 적용 요청)
- `canonical_json(Value)`: 객체 키 재귀 UTF-8 사전순 정렬, 배열 순서 유지, serde_json 숫자/문자열 직렬화, 공백 없는 UTF-8. `canonical_sha256`=그 bytes의 SHA-256, 접두 없는 소문자 hex64. sha2 exact `=0.11.1` 공통 사용 요청.
- plan은 root plan_hash/risk_summary만 제외; tc 원문 hash 결속 후 계산. 입력 plan_hash가 있으면 검산. source_sha256은 CRLF/BOM 포함 원래 bytes 그대로. full RFC8785(UTF-16 키 순서/ECMAScript float)는 아님. 기존 seq/토큰/시각의 freeze 재바인딩은 CORE 러너 몫.

## 검증·남은 관문
- format 시험14개 PASS: TC3쌍·UDS·별칭/무머리말/무표·원문/위치·해시 golden·분기/횟수·형식 상한·YAML depth/token/alias/확장바이트·bounded JSON output. 두 파서 각각10만 회 임의 bytes+UTF-8/구조 변형 입력 panic 없음.
- cargo test --workspace --offline PASS(662개). cargo fmt -p loom-test-format --check 및 cargo clippy -p loom-test-format --all-targets --offline -- -D warnings PASS. diff/새 파일 공백 검사 PASS.
- 필수 workspace fmt FAIL: loom-test-predicate/src/{ast,units}.rs의 포맷 차이. 필수 workspace clippy FAIL: 같은 crate units.rs:29 `Num::cmp`, should_implement_trait. 로그 `/tmp/loom-test-format-final-{0,1,2,3,4}.log`.

## 요청·질문
- CORE workspace.dependencies에 위 exact YAML/Markdown/sha2 판 등록, Registry→CommandTable/RiskProvider 연결, Diagnostic→CLI E_USAGE(차단 분류는 정책 오류) 매핑 요청. root Cargo.toml은 수정하지 않음.
- predicate 쪽 JSON AST에 위 canonical_sha256 helper/규약·sha2 exact 판 적용 필요. DSL 타입/의미 검증은 predicate/도메인 verifier에 위임; plan 정적 valid만으로 실행 승인/PASS를 만들면 안 됨.
- 질문: manual.timeout 확장·UDS legacy expect 어댑터의 정식 공유 스키마 반영은 리드가 결정. 구현을 막는 질문 없음; README에 사용법/결정/상한 상세 수록.
