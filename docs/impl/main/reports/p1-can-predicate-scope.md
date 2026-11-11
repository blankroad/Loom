# P1 CAN predicate source scope — 독립 리뷰 (읽기전용)

docs/plan/02-core-principle.md 준수. 대상: 작업트리 미커밋 diff
`crates/loom-can/src/{gateway,signals}.rs`, `crates/loom-can/tests/{gateway,signals}.rs`.
소스/index/commit 변경 없음. 재빌드·재검사 하지 않음(root 로그로 판단).

## 결론: 승인 (차단 이슈 없음), P2 1건·P3 3건 권고

## 확인한 것

| 관점 | 결과 | 근거 |
|---|---|---|
| 거짓 PASS | 닫힘 | frame_match: `scope.matches(rec) && spec.matches(rec)` (gateway.rs:2301). signal_range: `!scope.matches(rec) → continue` 후 표본 계산 (signals.rs:1067). 범위 밖 레코드는 Match 근거·`samples` 카운트 모두에서 빠짐. |
| 타도메인 허용 | 닫힘 | `filter.domain != "can"` → Inconclusive, `matches()`가 `rec.domain == "can"` 강제. |
| 미지 키 | 닫힘 | `PredicateScope`·`MatchArgs` 모두 `deny_unknown_fields`; `filter`/`ch`만 제거 후 술어 parse. TEST plan parser(loom-test-format plan.rs:701)가 filter 키를 같은 5개(domain/channel/kinds/origins/request_id)로 제한하고, `filter_json`(wire.rs:391)도 그 5개만 내보냄 → 정상 TEST 입력이 거짓 Inconclusive 되는 경로 없음. origins 문자열 집합(device/agent/user/script)도 `Origin` serde snake_case와 일치. |
| ch 합치 | 정상 | `ch` 비문자열 → Inconclusive, `filter.channel`과 불일치 → Inconclusive, 없으면 `ch`로 채움. TEST는 `or_insert_with`라 pred 자체 `ch`가 우선되지만 불일치 시 fail-closed. |
| origin 직접인자 | 기존대로 교집합 | frame_match는 `FrameSpec.origin`, signal_range는 기존 `origin` 검사 유지. |
| 완전/불완전 창 | 계약대로 | 범위 밖만 남은 완전창 → Mismatch, 불완전창 → Inconclusive (기존 분기 그대로). TEST 창은 domain/channel 상위집합이므로 그 부분집합에 대한 complete 판정은 유효. |
| 예산 상한 | 변경 없음 | `budget.spend(chunk.len())`는 scope 필터 전 전체 청크 기준 그대로(보수적). parse는 O(1). |
| device witness 경계 | 변경 없음 | `EventRef`/`observed`/witness 생성 코드 무변경. |
| 기존 호출 회귀 | 없음(아래 P2 제외) | filter/ch 없는 기존 args는 기본 scope(=can 도메인만)로 이전과 동일. FrameSpec은 원래 `can.frame`만 매칭. root 로그: `-p loom-can` 전 스위트 통과(gateway 23, signals 23, isotp 15, periodic 16, worker 17, cli 14 등, 실패 0). |

## 권고

**P2 — 정적으로 빈 scope가 Mismatch(FAIL)로 보임.** plan parser는 `origins` 생략 시 `["device"]`를 기본값으로 넣음(plan.rs:715). 이번 변경으로 이 기본값이 처음 실제로 적용되어, `filter.origins` 없이 `pred: {op: can.frame_match, dir: tx}` 또는 `origin: agent`를 쓴 plan은 완전창에서 항상 Mismatch가 됨(이전엔 agent tx 프레임과 Match). 방향은 fail-safe(거짓 PASS 아님)이고 TC-1 live plan은 `origins: [device]`를 명시해 영향 없음. 다만 `pred.origin ∉ filter.origins` 또는 `kinds`에 `can.frame` 없음은 parse 시점에 판별 가능한 작성 오류이므로, 장비 실패처럼 보이는 Mismatch보다 `Inconclusive{reason: "origin agent outside filter.origins"}` 같은 진단이 낫다. 최소한 plan 작성 문서에 "agent 송신 검증은 filter.origins에 agent 명시" 한 줄 추가 권장.

**P3 — 테스트 공백.** 다음 경로는 미검증: `filter.domain != can` → Inconclusive, `ch`/`filter.channel` 불일치 → Inconclusive, scope 배제 + 불완전창 → Inconclusive(Mismatch 아님), signal_range의 `request_id`/`kinds` 배제. 또한 root 로그는 `loom-can`만 돌림 — loom-test `domain_check`(wire.rs:733)가 실제 `filter_json`+`ch`를 CanVerifier에 넘기는 종단 테스트(TC1 frame_match 재현)가 있으면 이번 수정의 원인 경로를 고정할 수 있음.

**P3 — 문서 주석.** `CanVerifier` 주석(gateway.rs:2239)의 인자 목록에 `filter`/`ch` scope 언급 없음.

**P3 — 중복 정리(선택).** signal_range는 `PredicateScope::parse`와 `condition_from_args`가 각각 `filter`/`ch`를 처리. 동작 문제는 없으나 한 곳(예: `parse`가 남은 args를 반환)으로 모으면 두 술어의 제거 규칙이 갈라질 위험이 줄어듦.

## 범위 밖 관찰 (조치 불요)

- `domain_check`의 user_intervention 판정은 scope와 무관하게 domain 전체를 봄 — 기존 동작, 이번 변경 대상 아님.
- 메모리 soak binary·권한 제약은 손대지 않음.
