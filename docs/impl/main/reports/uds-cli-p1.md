# uds-cli-p1 — UDS product CLI fragment

상태: **READY_TO_INTEGRATE** (2026-11-12, Herdr foreground Claude Opus, 하위 agent 없음)
기준: main 3bac3f1 + 미커밋(다른 담당 변경 보존), main이 적용한 loom-uds/loom-cli manifest features.

## 변경 파일 (소유 범위만)
| 파일 | 내용 |
|---|---|
| `crates/loom-uds/src/cli.rs` (신규) | `UdsCli: DomainCli` — `connect / status / read-did / write-did / session / dtc list / raw / trace / abort` |
| `crates/loom-uds/src/lib.rs` | cfg 최소: `cli`(feature cli), runtime 전용 `handlers/hooks/lanes/manager/module`, `pub use UdsModule`, `sim_support`에 `#[cfg(feature="runtime")]`. pure 모듈(`events, evidence, payload, policy, risk, session, text`)은 상시. uds-impl의 `evidence`/`sim_support` 추가분 그대로 보존 |
| `crates/loom-cli/src/lib.rs` | `domains()`에 `#[cfg(feature="uds")] CliOf::<loom_uds::cli::UdsCli>` 2줄만 |
| `crates/loom-uds/tests/cli.rs` (신규) | 변환·거부·렌더·**등록 스키마 정합** 테스트 7개 (cli-only 빌드에서 6개) |

module/evidence/manager/handlers/hooks/기존 tests 무수정. Cargo manifest/lock 무수정.

## CLI → RPC (정본 = `module.rs` params 스키마 + handlers가 실제 읽는 키)
| CLI | RPC | params |
|---|---|---|
| `connect --name N (--sim [bench] \| --host IP) [--port] [--la HEX] [--tester-la HEX] [--activation HEX] [--no-reconnect] [--timeout-ms]` | `uds.connect` | `name, sim:"bench", host, port, la, tester_la, activation, auto_reconnect:false, timeout_ms` (주소는 숫자로 전송) |
| `status [-t T]` | `uds.status` | `target` |
| `read-did DID… [-t] [--timeout-ms]` (1–64) | `uds.read_did` | `dids:[str], target, timeout_ms` |
| `write-did DID --hex HEX [-t] [--timeout-ms]` | `uds.write_did` | `did, data_hex, target, timeout_ms` (CLI wait = 2×timeout+5s: read-back 포함) |
| `session NAME\|HEX [-t]` | `uds.session` | `session, target` |
| `dtc list [--mask HEX] [-t]` | `uds.dtc.list` | `mask:number, target` |
| `raw HEX… [--dry-run] [-t] [--timeout-ms]` | `uds.request` | `bytes_hex, dry_run:true, target, timeout_ms` (text 문법 아님, task대로 bytes_hex) |
| `trace [-t] [--tail 1..1000]` | `uds.trace` | `target, tail` |
| `abort [-t] [--scope all\|agent]` | `uds.abort` | `target, scope` |
| `help [--json]` | `core.help` | loom-cli 공통 처리 (fragment 무관) |

- 형태 검사는 gateway와 **같은 pure 함수** `text::encode_typed`/`text::parse_did`(codec만 사용)로 수행 → CLI가 거부하는 것 = loomd가 `E_USAGE`로 거부할 것. 위험 분류, dry-run 결과, STOP, lease, NRC/timeout/outcome_unknown 처리는 전부 gateway에 남김(예: `raw 11 01`은 CLI가 막지 않음).
- `--json`/envelope/exit code/`--idempotency-key`(`_loom` 삽입)/`NOTE:`·`WARNING:`은 loom-cli·cli-kit 공통 경로 그대로.
- 사람용 렌더: 실패 envelope의 `error.detail.row`도 출력(NRC 이름, TIMEOUT, BLOCKED), `outcome_unknown`이면 "do not resend" 경고.

## I1 / 영향 검사 (CARGO_INCREMENTAL=0, target ~/.cache/loom-main-target, --locked --offline)
- `loom-xtask boundaries`: **통과**. `cargo tree -p loom-cli` 에 uds 쪽은 `loom-uds`+`loom-uds-codec`만 (tokio/doip/runner/sim/worker 없음).
- `clippy -p loom-uds`: default / `--no-default-features --features cli` / `--features runtime` / 무 feature — 경고 0. `clippy -p loom-uds -p loom-cli --all-targets` 경고 0.
- `cargo test -p loom-uds`: 전부 통과 (unit 2, cli 7, e2e 7, evidence 2, fixtures 2, uds 7). cli-only feature로 tests/cli 6 통과.
- `cargo test -p loom-cli`: 통과 (lib 4, cli 8, test_feature 1). `cargo check -p loom-gateway --all-targets` 통과 (gateway 기본 runtime 동작 불변: loom-uds default features).
- 실제 바이너리 help-only smoke(빈 LOOM_HOME): `loom uds help`, `loom uds raw --help` 정상; `loom --json uds raw '22 F1 9'` → `E_USAGE` exit 2, 데몬 미기동. 데몬 연동 smoke는 main 통합 몫.

## Manifest 제안
- **추가 변경 불필요.** main 적용분(default=[runtime,cli], runtime=sim/doip/runner/tokio/tracing optional, cli=clap/cli-kit optional, loom-cli `uds` feature)으로 모든 조합이 빌드됨. tests/cli.rs는 `#![cfg(feature="cli")]`, 스키마 정합 테스트는 `#[cfg(feature="runtime")]`.

## 발견한 정본 불일치 (uds-impl/main 판단 필요, CLI는 스키마 기준으로 노출 안 함)
1. `uds.session`, `uds.dtc.list`: handler `send()`는 `timeout_ms`를 읽지만 스키마에 없음 → CLI `--timeout-ms` 미제공(정합 테스트가 거부 고정). 필요하면 uds-impl이 스키마에 추가 후 CLI 2줄 추가.
2. `uds.trace`: 스키마 `since`를 handler가 무시(항상 tail). → CLI `--since` 미제공. **skills/loom-uds/SKILL.md**의 `trace --since <cursor>`(표, 절차 4)는 실제와 불일치 — 제안: `loom events --since <from_cursor> --domain uds` 로 교체하거나 handler에 since 구현.
3. `uds.status.probe`: handler 무시 → CLI 미제공.
4. `uds.request.functional`: handler가 `E_UNSUPPORTED`(M3b) → 미제공. `uds.lease.takeover/release`는 viewer 전용 → 에이전트 CLI 미제공.

## 잔여
- SKILL.md 표와 CLI는 위 2번 외 일치(`connect --sim bench --name`, `write-did --hex`, `dtc list --mask 09`=hex). SKILL.md 수정은 소유 밖이라 제안만.
