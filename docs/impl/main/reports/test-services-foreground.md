docs/plan/02-core-principle.md 준수
# TEST 실제 Registry command/risk 어댑터

- 기능 / 상태 / 담당: TEST 실제 Registry command/risk adapter / **READY_TO_INTEGRATE** / Herdr Claude opus 직접 수행, 자체 리뷰만(독립 리뷰 없음). 하위·내장 에이전트 사용 없음.
- 기준: main `302a5d5`. 검증은 **HEAD 고정 scratch overlay**(git archive, 실제 트리·index 무변경)에서 공유 파일 patch를 적용해 수행. 실제 트리의 UDS worker 병렬 변경(loom-uds, daemon.rs, domains.rs, registry.rs, uds_tc1_smoke.rs 등)은 읽기만 했고 수정하지 않았다. 검증 시점 실제 트리의 loom-uds는 작업 중이라 컴파일되지 않았다(`UdsManager::new` 인자 불일치).
- `CARGO_TARGET_DIR=~/.cache/loom-main-target`. 명시 한도·기한 없음, 사용량 미상. index/commit/push/reset/clean 없음. 기존 미커밋·데모 STOP 보존.

## 결과 요약

`TestModule::new()`(commands=None) 대신 실제 도메인 Registry에서 만든 command/risk table + `PredicateEngine::from_registries`를 주입한다. 이제 TC-1은 첫 단계 `E_NOT_IMPLEMENTED`가 아니라 **실제 등록 명령표 기준 plan check**에 도달하고, fixture가 실제 등록과 다른 4곳을 위치와 함께 `E_USAGE`로 보고한다. 실제 등록 명령으로 쓴 plan은 RUNNING까지 시작되며 위험 요약은 도메인 hook 결과(PSU 출력 R3)다. source fence/recording health/approval/finally는 `NotWiredHooks`로 유지되어 PASS/DONE은 확인되지 않는다(`E_EVIDENCE_MISSING`).

## 소유 파일 (실제 트리에 작성)

- 신규 `crates/loom-gateway/src/test_services.rs` (839줄, sha256 `24e6cc4a…414d71c`). `mod` 선언 전까지 컴파일되지 않으므로 현재 빌드에 영향 없음.
- 본 보고서.

### 어댑터 동작 (`RegistryCommands`)

| 항목 | 실제 근거 | 가짜/기본값 금지 처리 |
| --- | --- | --- |
| lookup | `Registry::commands()` 전체(TEST 자신 제외: plan이 `test.*`를 구동 불가) | 내장 명령 목록 없음. 미등록은 `unknown_command` |
| waiting | 등록 params schema의 `required`에 `timeout_ms` | — |
| 인자 검증 | 등록 `RoleSet.script`, `write_class`(Job/Interactive는 gateway가 아직 `E_NOT_IMPLEMENTED`로 거부하므로 plan에서도 거부), `Bounds.max_wait` ≥ plan deadline, 등록 params JSON Schema | schema 검사기는 도메인이 실제 쓰는 키워드 전부(type/enum/const/properties/required/additionalProperties/min·max*/items/anyOf/oneOf/allOf/not/$ref/$defs/정수 format) 지원. 검사 못 하는 assertion 키워드는 **실패 처리**(현재 `uart.open.alias`의 `pattern`만 해당: 값이 있으면 거부). 평가 예산 100k 노드/깊이 64 |
| RPC params | plan `timeout`/`timeout_ms`(포맷상 동일 runner deadline)를 빼고, schema가 `timeout_ms`를 선언하면 그 값으로 넣음 — `rpc_params()` 공개. **향후 step/finally dispatcher는 같은 함수를 써야 검사=송신이 됨** | — |
| 위험 | 도메인 `WriteHooks::classify`(gateway write pipeline이 부르는 같은 hook), origin=Script, channel 키는 `daemon::call_channel`과 동일. `RiskProvider`에 env가 없으므로 sim/bench/vehicle 모두 분류해 **최댓값**(CAN send: sim R1, 실장비 R2 → R2) | read(`write_class: None`)=R0은 Registry 계약. hook 없는 write는 gateway와 같은 R1. 미등록/분류불가 = R4+immutable_deny |
| block | hook `BlockClass::DefaultBlocked`→default_blocked, `HardDeny`/`ImmutableDeny`→immutable_deny (예: UDS 0x86) | — |
| safe_state | 도메인이 등록한 `SafeStateSpec`(method 동일 + spec args 전부 일치)만 | read는 safe_state 아님 |

`test_module(regs)` = `TestModule::with_services(SessionConfig::default(), RegistryCommands, PredicateEngine::from_registries(regs), NotWiredHooks)`.

## 실제 결과 (overlay, HEAD 302a5d5 + 아래 patch)

| 명령 | 결과 |
| --- | --- |
| `cargo test -p loom-gateway --locked --offline` | 전체 PASS: lib 13(신규 test_services 6 포함), can_eth_smoke 2, daemon 9, hub 5, ipc 6, journal 4, r1 5, replay 8, **test_services_smoke 2(신규)**, uds_tc1_smoke 2(1개 기대값 갱신) |
| `cargo test -p loom-test -p loom-test-format --locked --offline` | 전체 PASS (변경 없음) |
| `cargo clippy -p loom-gateway --all-targets --locked --offline -- -D warnings` | PASS |
| `cargo clippy -p loom-gateway --no-default-features --features echo … -D warnings` | PASS |
| `cargo check -p loom-gateway --no-default-features --features test` | PASS, 경고 1(`inproc` 미사용 매크로 — 기존부터 있던 것, 본 변경과 무관). 최초 patch의 타입 추론 오류는 클로저 타입 명시로 수정 |
| `cargo fmt -p loom-gateway --check` | PASS |
| `git apply --check` (현재 실제 트리 파일 사본, UDS 미커밋 포함) | 6파일 모두 적용 가능(uds_tc1_smoke hunk offset 30) |

실제 gateway(격리 HOME의 production `daemon::start`, WebSocket agent+viewer) 응답:

- TC-1 fixture `test.start` → `E_USAGE "plan check failed"`, 세션 미생성. detail.errors:
  1. `/steps/1/actions/0` arguments: `uart.expect` 필수 `channel`, `patterns` 없음 (fixture는 `port`, `pattern`)
  2. `/steps/2/actions/0` unknown_command: `can.sig.wait-for` (등록명은 `can.sig.wait_for`, 인자 `expr`/`within_ms`)
  3. `/steps/4/actions/0` unknown_command: `uds.dtc.clear` (UDS 미등록)
  4. `/finally/0/actions/0` unsafe_finally: `psu.out off` = R3(safe_state=true) — 포맷 규칙은 finally에 R0–R1 AND safe_state 요구
- 실제 등록 명령 plan(`psu.out{ch,state}`, `uart.expect{channel,patterns,timeout}`, `can.sig.wait_for{expr,within_ms,timeout}`, `uds.dtc.list{target,mask}`, finally 없음) → `RUNNING`, `risk_summary {R0:4,R3:2}`, viewer가 `test.session_started` 라이브 수신. `test.step S2` 선행 → `E_DENIED`; `test.step S1` → `source_fence_available:false`; `verdict done` → `E_EVIDENCE_MISSING`; `verdict pass` → `E_EVIDENCE_MISSING`; status에 effective PASS/DONE 없음.

## 공유 파일 제안 patch (메인 통합용)

파일 위치(전체 385줄): `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/775ceb84-2aad-4e22-9043-1beb3b3a1b38/scratchpad/test-services-shared.patch` (sha256 `25eace68…b3b291832`). 아래에 전문 수록.

| 파일 | 변경 |
| --- | --- |
| `crates/loom-gateway/Cargo.toml` | `loom-test-format` optional path dep, `test` feature에 추가 (`loom-test`가 format trait을 재수출하지 않음) |
| `Cargo.lock` | loom-gateway deps에 `loom-test-format` 1줄 |
| `crates/loom-gateway/src/lib.rs` | `#[cfg(feature = "test")] pub mod test_services;` |
| `crates/loom-gateway/src/domains.rs` | `#[cfg(feature="test")]` 한 줄만 교체: 앞서 등록된 registry로 `test_services::test_module` 생성, 실패 시 로그 후 TEST 비활성. UDS worker의 `builtin_with_clock` 변경과 겹치지 않음 |
| `crates/loom-gateway/tests/uds_tc1_smoke.rs` | `tc1_start_reports_current_gateway_wiring` 기대값 `E_NOT_IMPLEMENTED`→`E_USAGE`만 (UDS worker가 바꾼 다른 테스트와 hunk 분리) |
| 신규 `crates/loom-gateway/tests/test_services_smoke.rs` | 위 gateway 종단 시험 2개. patch 전엔 실패하므로 실제 트리에 미리 두지 않았다 |

통합 명령(메인):

```sh
cd /Users/ctmctm/workspace/Loom
git apply --check /private/tmp/claude-501/-Users-ctmctm-workspace-Loom/775ceb84-2aad-4e22-9043-1beb3b3a1b38/scratchpad/test-services-shared.patch
git apply /private/tmp/claude-501/-Users-ctmctm-workspace-Loom/775ceb84-2aad-4e22-9043-1beb3b3a1b38/scratchpad/test-services-shared.patch
export CARGO_TARGET_DIR=~/.cache/loom-main-target
cargo test -p loom-gateway --locked --offline
cargo clippy -p loom-gateway --all-targets --locked --offline -- -D warnings
cargo fmt -p loom-gateway --check
```

UDS domain 변경이 들어간 뒤 다시 돌릴 것: 단위/종단 시험이 `uds.dtc.list {target, mask}` 등록·R0 분류와 `uds.request 0x86` block에 의존한다. UDS가 이름/스키마를 바꾸면 그 assertion만 맞추면 된다(어댑터 코드는 Registry 필드만 읽어 영향 없음).

공유 patch는 위 경로에 보존했고 메인이 적용했다. 최종 소스와 변경 검증은 통합 커밋을 기준으로 한다.


## 남은 hooks·후속 (명시적 unavailable 유지)

| 항목 | 현재 | 후속 소유 |
| --- | --- | --- |
| source fence (`SessionHooks::fence`) | `None` → 창 unknown → 검증 `uncertain` | CORE: journal 시퀀스 fence 제공 |
| recording health | `Unknown` → PASS 불가 | CORE: daemon `record_health()` 연결 |
| STOP (`stop_latched`) | `false`(gateway 자체 STOP latch가 쓰기는 막음, TEST 세션 상태엔 미반영) | CORE: daemon latch 연결 |
| approval | `E_NOT_IMPLEMENTED` (F1 Auto 모드라 start는 진행) | CORE: approval adapter |
| finally dispatcher | `E_NOT_IMPLEMENTED` | CORE: 정상 RPC 경로로 dispatch, `RegistryCommands::rpc_params` 재사용 |
| step action dispatch | 없음(plan action은 TEST가 실행하지 않음) | CORE/TEST |
| `presend_check`/interlock | plan 시점 미호출(PSU interlock·UART 프로필 등 상태 의존 분류는 미리보기) | gateway가 실제 송신 시 재분류·presend 수행(기존 경로) |

## 결정 필요 사항 (기본값으로 진행, 판단은 메인/소유자)

1. **TC-1 finally 충돌**: PSU 도메인은 `psu.out off`를 R3(등록 safe state)로 분류, 포맷은 finally에 R0–R1 요구. 위험을 낮추지 않았다. 선택지: (a) 포맷 규칙을 "등록 safe-state면 등급 무관 허용"으로 변경(TEST format 소유), (b) PSU가 OFF 등급 재검토(PSU 소유). 기본: 현 상태 유지 → TC-1 finally 거부.
2. **TC-1 fixture 갱신**: `can.sig.wait-for`→`can.sig.wait_for{expr,within_ms}`, `uart.expect{channel,patterns}`, `psu.out{ch}`(`name`은 PSU schema에 additionalProperties 제한이 없어 통과하지만 무시될 수 있음), `uds.dtc.clear`는 UDS 등록 필요(또는 `uds.request 14…`). fixture는 loom-test-format 소유라 수정하지 않았다.
3. **plan env vs daemon env**: 종단 시험에서 plan `env: sim`인데 daemon meta는 `bench`였고 `test.start`는 이를 비교하지 않는다. 이번 범위 밖, TEST/CORE 후속.
4. `uds.connect`, `can.isotp.open` 등은 도메인이 read(write_class None)로 등록해 R0로 분류된다. Registry 선언을 그대로 따른 것이며 등급 변경은 각 도메인 소유.

## 자체 리뷰 메모

- 거짓 PASS 경로 없음: verifier는 실제 도메인 verifier지만 fence/recording health가 없어 PASS/DONE 거부를 종단으로 확인.
- 기본 R0 없음: R0는 Registry read 계약에서만, 미등록·분류불가는 R4+deny.
- 위험 과소평가 방지: env 최댓값, block OR.
- 남은 위험: 자체 JSON Schema 부분 검사기(라이브러리 없음). 도메인이 새 키워드를 쓰면 통과가 아니라 거부되므로 안전 측으로 실패한다.

## 메인 통합 판정

- shared patch를 fe31df5(UDS+preview 통합) 위에 적용. source 모듈·등록 명령/역할·위험분류·미지원 schema fail-closed·safe-state 조건을 메인이 독립 리뷰했다.
- 실제 main `cargo test -p loom-gateway --locked --offline`: 58 PASS, exit 0. UDS 실도메인/순수preview와 TEST adapter 종단 시험 함께 통과. /private/tmp/loom-test-services-integration-test.log
- 실제 main gateway all-target clippy -D warnings, fmt/diff check PASS. /private/tmp/loom-test-services-integration-clippy.log.
- 어댑터 코드 통합은 DONE. TC-1 실제 실행/증거 수용은 DONE 아님. fixture API 차이·finally risk 충돌·source fence/record health/STOP/approval/finally/step dispatch 후속은 위 표대로 남음.
- 미지원 pattern schema로 UART open(alias 포함) plan은 현재 거절한다. schema를 통과했다고 허위 판정하지 않으나 후속 지원 필요.
- 담당 소유권 메인 인수, Herdr 세션/원본 worktree 보존. 로컬 기능 커밋, push 없음.
