# P0 보고 — explore-opus: `crates/loom-eth-filter` (2026-11-11)

**완료.** worktree `.worktrees/eth`, 커밋 안 함. 관문 3개 green: `cargo fmt --check` · `cargo clippy --all-targets -D warnings` · `cargo test`(golden 3 + limits 7 + doc 1).

## 무엇을 만들었나
plan §4.1 `loom-pktfilter` = Wireshark 표시 필터 부분집합. 외부 파서 crate 없이 직접 구현했다.
- 처리 순서: 토큰화(모든 토큰에 바이트 위치) → 재귀 하강 파서(AST) → 타입 검사(`loom-eth-decode` 필드 레지스트리 기준, 별칭 해석) → 평가기(`Decoded::field_values`만 사용).
- 연산자: `== != < <= > >=`와 `eq ne lt le gt ge`, `contains`, `matches`/`~`(`regex` crate, 백트래킹 없음), `in {a, b, 10..20}`(쉼표·공백 구분, `a .. b`도 가능), `and or not && || !`, 괄호.
- 슬라이스: `[i:n]`, `[i]`, `[i-j]`, `[:n]`, `[i:]`, 음수 시작(`[-1:1]`). 정수·불리언·시간 필드에 쓰면 위치 있는 오류다.
- 리터럴: 정수(10진·`0x`·`0b`, 타입 최댓값 검사), 불리언(true/false/1/0), MAC·바이트열(`:`·`-`·`.` 구분 또는 짝수 hex), IPv4/IPv6·CIDR(`ipnet`, `==`는 "포함"), 문자열(`\" \\ \n \t \r \0 \xHH`), 시간(초 소수, 9자리까지).
- **의미(Loom 정의)**: 다중 값은 "어느 하나". `a != b` = `not (a == b)` — 필드가 **없으면 참**(`doip.type != 0x8001`은 ARP에도 맞음). 이름만 쓰면 존재 검사. `frame.time_relative`·`time_delta`는 `EvalCtx{time_ref_ns, prev_ts_ns}`로 계산.
- **명시적 비지원 오류(위치 포함)**: 함수, 산술(`+ * % ^`), 비트 연산(`&`, `|`), 필드끼리 비교, `#` 레이어 연산자, 매크로(`$`, `${`), `===`/`!==`, 단일 `=`, `any`/`all`, 문자 리터럴 `'a'`, Cisco식 MAC `0200.0000.000a`, 집합의 빈 항목·끝 쉼표.
- `matches`는 따옴표 안 원문을 그대로 정규식으로 쓴다(`\d`, `\xF1`이 정규식 의미대로 동작). 바이트 필드(프로토콜·슬라이스·bytes)는 `regex::bytes`(unicode off), 문자열 필드는 `regex::Regex`다. 정규식 밖에서 모르는 이스케이프(`\q`)는 오류다.

## 공개 API
`Filter::compile(&str) -> Result<Filter, FilterError{pos, expected, msg}>` · `Filter::matches(&Decoded) -> bool`(시간 기준 0) · `Filter::matches_with(&Decoded, &EvalCtx)` · `Filter::fields_used() -> &[&'static str]`(정본 이름, 처음 쓴 순서, 푸시다운용) · `Filter::as_str()`.

## 상한 (C2)
| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_FILTER_LEN` | 4096 B | 위치 4096 오류 |
| `MAX_DEPTH`(괄호·`not` 중첩) | 64 | 넘는 토큰 위치에 오류 |
| `MAX_NODES`(시험·논리·집합 항목) | 1024 | 오류 |
| `MAX_SET_ITEMS` | 256 | 오류 |
| `MAX_REGEX_LEN` / `REGEX_SIZE_LIMIT`(컴파일·DFA 캐시) | 1 KiB / 256 KiB | 오류(빌드하지 않음) |

## 테스트
- golden 321개(일치 241 + 오류 위치 80), fixture 10종(DoIP 요청·응답·NRC·공지, VLAN SD, ARP, ICMP, IPv6, SYN, PDU 2개)을 decode crate로 디코드해 평가. 푸시다운 필드 목록, 시간 문맥, 상한 6종(길이·괄호·`not` 깊이·노드·집합·정규식).
- 무작위 필터 문자열 10만 회(토큰 조각 조합 + 임의 문자): panic 0, 오류 위치가 항상 입력 안, 컴파일 성공분은 3개 fixture에 평가.
- 첫 실행에서 실제 버그 2개를 고쳤다: `\xF1`이 문자 ñ(UTF-8 2바이트)로 바뀌어 바이트 정규식이 맞지 않던 것, `in {…, }` 끝 쉼표를 받던 것.

## 미해결 / 요청
- WASM 빌드는 아직 확인하지 않았다(M2). 의존성은 모두 순수 Rust다.
- Wireshark와의 결과 대조 시험은 tshark가 있는 환경에서만 가능하다(선택). 값 이름 리터럴(`someipsd.entry.type == offer`)은 지원하지 않는다(숫자만).
- **CORE workspace.dependencies 추가 요청**: `regex = "1.13.1"`, `ipnet = "2.12.2"`. `thiserror`는 CORE와 같은 2.0.18로 lock 고정. dev: `etherparse = "=0.21.0"`(decode와 같음).
