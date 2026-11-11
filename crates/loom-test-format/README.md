# loom-test-format

Pure library: caller supplies UTF-8 strings/bytes; no files, RPCs, clocks, templates or test execution.

- `parse_tc` / `parse_tc_bytes` → `TcDoc`. Frontmatter fields are optional. `description` is the **exact complete Markdown body**, including its original table; this keeps all prose/extra columns available. Steps extract original Markdown cell slices, byte spans and one-based source lines. Only tables in `단계`/`Steps` sections are interpreted.
- `parse_plan` / `parse_plan_bytes` → immutable `Plan`. `parse_plan_with_tc(source, &doc)` checks/binds the TC source digest. Syntax/schema errors are `Diagnostic {code, message, path, location {line,column,offset}}`. Semantic positions point to the containing step and carry a JSON pointer; YAML syntax errors use the backend mark. Flow-style steps share their containing line.
- `check_plan(&plan, &commands)` → `Report`. Caller implements `CommandTable: RiskProvider` using the real command registry/classifiers, including blocking flags and safe-state classification. `validate_args` is an optional domain schema hook. `is_valid()` means static validity, **not** approval or an evidence verdict. Check `tc_hash_verified` before binding approval to a TC, and `requires_legacy_adapter` before execution.
- Predicates/measurement descriptors are JSON objects retained without interpretation. The predicate crate must validate/evaluate them. Origins default to `[device]`. Anchors refer to the current/earlier step, never old seqs/tokens.

## One step schema

A TC plan requires `loom_tc_plan: 1` and step `id`; scenarios use `loom_scenario: 1` and may omit ids (`S1…`, finally `F1…`). A step has one `<domain>.<verb>: {args}` or `do: [{command: {args}}, …]`, optional `tc_step`, `expect`, `on_fail`, `when`, `retry`, `repeat`, `manual`, `measure`, `save`, and optional default `anchor`. Mixed direct/do or unknown step fields are errors. Arrays/objects and numeric bounds are checked, not coerced.

`on_fail` accepts `continue|stop_tc|stop_session` or a one-key mapping including `goto: id`. `when` reads an earlier step's verdict. Success fallthrough and failure goto form one acyclic graph; backward goto is forbidden even without a second explicit goto. Finally cannot goto into the main body. Each cleanup command must be both R0–R1 **and** `safe_state`. This parser never runs cleanup on STOP or abort.

`retry.count` is additional retries: risk max uses = `(1 + retry.count) × repeat.count`. Every command on both conditional paths and in finally is classified; no agent-supplied summary is trusted. Delays are runner builtins, not RPCs: `sleep: 2s`, `wait: 3s`, or `{duration_ms: 2000}`. Durations are positive integer milliseconds or integer `ms|s|m` strings, at most one hour. Waiting registry commands need `args.timeout[_ms]`; manual prompts need `manual.timeout[_ms]`. Conflicting timeout forms are errors.

The UDS §6.5 fixture preserves globals (`target`, `db`, `vars`, `on_error`), saved variable names and `${…}` expressions. Global target is applied to UDS invocation args for classification. Its old `expect: {DID.field: value}` is preserved as opaque `pred`, marked `legacy`, warned and **requires UDS adapter translation before execution**; no fictitious predicate operator is introduced. `loom-uds-runner` currently has no YAML loader/fixture; this fixture uses its UDS commands and the plan's published scenario.

## Hash contract shared with predicate integration

`source_sha256` is SHA-256 over exact source bytes (BOM/CRLF included); output is 64 lowercase hex digits, no prefix. `canonical_json(value)` recursively sorts object keys by Rust UTF-8 lexical order, preserves array order, uses serde_json number/string serialization, and emits UTF-8 JSON without whitespace. `canonical_sha256(value)` hashes exactly those bytes with **sha2 = 0.11.1**. This is the requested simple JCS, **not full RFC 8785** (UTF-16 key ordering/ECMAScript float normalization are not implemented).

Plan hashing removes only root `plan_hash` and untrusted `risk_summary`; TC binding inserts/validates `tc: ID@sha256:<exact source digest>`. Other metadata, commands, order, predicates and limits remain in the digest. The source hash still records those removed fields. If supplied, `plan_hash` must match. Predicate callers should use this same helper over their canonical JSON AST; no AST-specific rewrite occurs here. Freeze must rebind actual sessions/fences/approvals; this library provides no freeze execution or evidence validation.

## Bounds

| Item | Ceiling / overflow |
|---|---|
| Input / canonical JSON | 1 MiB each; error before parsing/output growth |
| Steps | 500 total, including finally; located error at first excess |
| Markdown cell / manual prompt | 4096 source bytes; error, never truncate prose |
| YAML/JSON depth / nodes | 32 / 65,536, including expanded aliases; error before descending/retaining |
| Expanded AST charge | 8 MiB; 128 B/node + scalar/key bytes, charged before copies |
| YAML lexical tokens / indent | 131,072 / 256 columns; conservative preflight protects backend event allocation |
| Collection / scalar | 8192 items / 64 KiB; error |
| Commands / expectations | 64 each per step; error |
| Retry / repeat | 3 additional / 100 iterations; error |
| Timeout / delay | 3,600,000 ms; error, no unlimited waits |
| Diagnostics / risk entries | 1024 / 500×64; errors stay invalid, registry messages capped at 4 KiB |

Duplicate YAML keys, non-string keys, nonfinite numbers, custom tags, multiple documents and recursive aliases are errors. Syntax is YAML's JSON-compatible subset; preflight raw lexical limits can reject unusually verbose quoting/indentation earlier than semantic limits.

Dependency review (2026-11-11): [serde_yaml](https://crates.io/crates/serde_yaml) 0.9.34+deprecated, MIT/Apache-2.0, 2024-03-25; [upstream archived/unmaintained](https://github.com/dtolnay/serde-yaml). [serde_yaml_ng](https://crates.io/crates/serde_yaml_ng) 0.10.0, MIT, 2024-05-26. Selected [serde_norway](https://crates.io/crates/serde_norway) 0.9.42, MIT/Apache-2.0, 2024-12-21, non-archived fork with a later release; this does not imply a current maintenance guarantee. [pulldown-cmark](https://crates.io/crates/pulldown-cmark) 0.13.6, MIT, 2026-10-30; [sha2](https://crates.io/crates/sha2) 0.11.1, MIT/Apache-2.0, 2026-10-12. All three are exact versions; workspace serde/serde_json are inherited. No unsafe code is introduced here; YAML's dependency internally uses its libyaml port.

`inspect_plan(&plan)` returns `StructuralReport` for daemon-free checks without a command registry. It validates available structure/flow/types and explicit timeouts, warns “명령 존재 미검사”, and deliberately exposes no risk summary or execution approval. `check_plan` also accepts a trait object for CORE injection.
