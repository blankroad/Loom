# P1 anchor contract — anchor.where kind별 schema 검증 (TC1 finding 3)

상태: **DONE (단위/통합 테스트 범위)**. browser PASS 주장 없음. commit/push 없음.

## 원인
`PredicateEngine::anchor_ts`가 `Expr::check(&self.common.specs)`로 검증했다. `common.specs`는 모든
도메인의 `PointerSpec`을 평탄화한 목록이고, `Expr::check`는 path가 같은 **첫 spec**만 쓴다.
정본 schema 확인:
- `crates/loom-uart/src/kinds.rs:149` — UART 포트 상태 `/payload/state` enum `closed|opening|open|…`
  (같은 도메인의 다른 kind `:208`도 `/payload/state` enum `sent|held|discarded`)
- `crates/loom-psu/src/gateway.rs:151` — `psu.write` `/payload/state` enum `on|off`
- can/eth도 `/payload/state`를 String/다른 의미로 등록

그래서 `anchor: {kind: psu.write, where: /payload/state == on}`가 UART enum으로 거부됐다.

## 수정 (`crates/loom-test/src/wire.rs`만)
- `PredicateEngine`에 kind별 pointer 목록(`kinds`) 추가. `from_registries`가 `EventKindSpec.kind/pointers`로
  채운다. 테스트·외부용 `with_kinds(Vec<(kind, pointers)>)` 추가 (`new` 시그니처 불변).
- `scoped_specs(kinds, domain)`: 후보 kind = `anchor.kind`/`filter.kinds`(지정 시), 없으면 domain의 모든
  kind, 그것도 없으면 모든 kind. 후보 kind들이 같은 path를 **다른 type/unit**으로 등록하면 그 path는
  ambiguous로 빼고 조건이 그 field를 쓰면 **fail closed** (`field X has different schemas in kinds A, B — name one kind`).
- `anchor.where`: 위 범위로 검증. kind 생략 + 충돌 field → error. 미등록 kind → payload field가 없어
  `unknown field`로 거부(기존 unknown field 검증 유지). enum/type/unit 검증은 기존 `Expr::check` 그대로.
- per-kind 정보 없는 엔진(`new`만)은 평탄 목록에 같은 ambiguity 규칙 적용(첫 spec 선택 안 함).

## 필터 조건 일반 검증 — 같은 문제 있음
- `common_check`(expect의 `exists/hold/…`의 `where`)도 `c.check(&self.common.specs)`로 같은 버그.
  wire.rs 안이라 같은 `scoped_specs(filter.kinds, filter.domain)` 적용. `Citation::exprs_mut`가 비공개라
  ambiguous field는 범위에서 빠져 `unknown field` error가 되고, 메시지에 ambiguity 설명을 덧붙인다.
- **소유 범위 밖, 미수정 (보고만)**: `loom-test-predicate/src/pred.rs` `CommonVerifier::verify`
  (CORE `test.exists` 등 직접 경로)도 평탄 `specs`로 검증 → 같은 오검증 가능. 수정하려면 그 crate에
  kind 범위 정보(또는 `Citation` exprs 접근)가 필요. 담당 팀 확인 필요.

## 회귀 테스트 (`crates/loom-test/tests/wire.rs`)
실제 등록 순서처럼 UART kind를 먼저 둔 `kind_specs()` + `with_kinds` 엔진, TC-1 plan의 S2 anchor만 교체:
- `anchor_where_uses_the_schema_of_the_anchor_kind` — `psu.write state == on` → PASS (수정 전 실패 재현 확인)
- `anchor_where_value_from_another_kinds_enum_is_rejected` — `psu.write state == opening` → error(`on|off`)
- `anchor_where_without_kind_on_a_conflicting_field_fails_closed` — kind 생략 → error(`different schemas`, 두 kind 명시)
- `anchor_where_unknown_field_or_kind_is_still_rejected` — 다른 kind의 field / 미등록 kind → error
- `tc1_still_passes_with_per_kind_schemas` — 필터 범위 검증 적용 후 TC1 S1–S3 PASS
- 반증: `with_kinds` 비우면 위 anchor 테스트 4개 FAILED 확인 후 원복.

## 검증 (CARGO_INCREMENTAL=0, CARGO_TARGET_DIR=$HOME/.cache/loom-main-target)
- `rustfmt --edition 2024` (두 파일), `cargo clippy -p loom-test --all-targets -D warnings` 통과
- `cargo test -p loom-test` 전부 통과 (wire 22/22 포함)
- `cargo test -p loom-gateway --test tc1_live_smoke --test test_runtime_smoke --test test_services_smoke --test uds_tc1_smoke`
  통과 (1+6+2+3) — 실제 registry(`from_registries`) 경로
- 30분 soak / binary 미실행·미변경. 다른 모듈/Cargo/lock 미수정. (working tree의 `loom-can` diff는 타 팀 작업, 미접촉)

## 주의
- 이전에는 filter kind가 등록하지 않은 field도 다른 kind 등록만 있으면 통과했다. 이제 filter kind 범위 밖
  field는 `unknown field`로 거부된다(의도된 strictness). 실 plan에서 이로 인한 신규 error가 나오면 plan의
  kind/field 불일치다.
