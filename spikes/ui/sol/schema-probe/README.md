# Rust → JSON Schema → TS 경로 검증

2026-11-10 KST, 계획 근거용 작은 DTO만 사용. 제품 스키마가 아니다.

- Rust/cargo1.93.1, schemars1.2.3, ts-rs12.0.1, serde1.0.228(나머지는 Cargo.lock).
- json-schema-to-typescript16.0.0, Ajv8.20.0, TypeScript7.0.2(package-lock).
- `$schema=https://json-schema.org/draft/2020-12/schema`를 Ajv2020 strict로 검증했다.
- tagged enum, decimal-string tuple newtype, optional/null 필드만 다뤘다.
- Rust의 u64::MAX `18446744073709551615`, 2^53+1 `9007199254740993`이 JSON→TS에서 문자열 그대로 유지됐다.
- seq가 number이면 거부, unknown event tag·필수 data_hex 누락도 거부됐다.
- String newtype이므로 숫자 문자열 문법·u64 범위 자체의 런타임 검증은 이번 DTO에 없다. 실제 제품에서는 추가해야 한다.

## 직접 생성 경로에서 관측한 불일치

Rust `note: Option<String>` + `#[serde(skip_serializing_if="Option::is_none")]`에서 None을 직렬화하면 note 필드가 없다.

- schema→TS: `note?: string | null` → 실제 fixture를 TypeScript가 수용.
- ts-rs direct: `note: string | null` → 실제 fixture에 note가 없어 TypeScript 오류.
- 원문: [direct-type-drift.txt](out/direct-type-drift.txt).
- `serde(transparent)` unsupported warning도 [build-stderr.txt](build-stderr.txt)에 보존했다. 이 tuple newtype의 direct 출력은 string이었지만 warning을 무시해도 모든 serde 속성이 호환된다는 뜻은 아니다.
- ts-rs12.0.1의 `decl`에는 `&Config`가 필요했다. 예전 버전의 인자 없는 예제를 그대로 쓰면 compile되지 않는다.
- 완화: `#[ts(optional = nullable)]` 명시 등 wire DTO 규약과 golden serialize fixture. 이번에는 불일치를 숨기지 않고 그대로 보존했다.

## 재현

`spikes/ui/sol/`에서 실행한다. Cargo cache·target도 이 디렉터리 안에만 둔다.

```sh
CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/schema-probe/target" cargo run --manifest-path schema-probe/Cargo.toml --quiet 2> schema-probe/build-stderr.txt
node schema-probe/check.mjs
node schema-probe/check-type-drift.mjs
```

마지막 명령은 schema 경로 tsc 성공과 direct 경로의 예상된 tsc 실패를 모두 확인한 뒤 exit0을 반환한다.
원시 결과: [result.json](out/result.json), [type-drift-result.json](out/type-drift-result.json).
모든 serde 속성·generics·flatten·borrow·전체 Loom DTO·다언어 decoder 호환을 검증한 것은 아니다.
