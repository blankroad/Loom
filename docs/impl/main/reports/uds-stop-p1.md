docs/plan/02-core-principle.md 준수
# READY_TO_INTEGRATE — CORE STOP → 진행중 UDS 취소 (uds-stop-p1)

- 기능 / 상태 / 담당: 실제 CORE STOP → 진행중 UDS 취소 / **READY_TO_INTEGRATE** / Herdr Claude Opus 5.5 포어그라운드 직접 수행. 하위 agent 없음, 자체 리뷰만(독립 리뷰 없음).
- 기준: main `6d41c24` + 메인 미커밋 scaffold(`WriteHooks::on_latch` 기본 noop, `Daemon::stop()`이 `latch.store` 직후 모든 domain `hooks.on_latch(on)` 호출). **scaffold 두 파일(`loom-domain-api/src/hooks.rs`, `loom-gateway/src/daemon.rs`)은 읽기만 했고 수정하지 않았다.**
- commit/push/index/merge/reset/clean 없음. 실장비·기존 demo STOP 미접촉. 공유 manifest/lock 미수정.

## 결과 (실제 gateway + 실제 DoIP, STOP은 viewer `core.stop`)

신규 `crates/loom-gateway/tests/uds_stop_smoke.rs`: 격리 tempfile HOME, production `daemon::start`, 실제 WebSocket agent 2개 + viewer 2개. 테스트가 loopback `DoipEntity`(실제 DoIP entity) 뒤에 bench 가상 ECU를 두고 `0x2E`에 0x78 pending 63회×200 ms 결함을 주입한다. agent는 `uds.connect {host:"127.0.0.1", port, la:0x1001}`로 실제 DoIP client/runner 경로로 연결한다. STOP은 `core.stop` RPC → `Daemon::stop` → `WriteHooks::on_latch(true)` 경로로만 들어간다(크레이트 내부 `stop_latch` 직접 호출 없음).

| 단계 | 실측 |
|---|---|
| agent `uds.request 2E F1 A0 01` | viewer에 `uds.tx sent(2ef1a001, origin agent)` + `uds.pending` 2회 → **진행중(handoff 후)** |
| agent2 `uds.read_did F18C` | gateway `accepted`, UDS 대상 큐에서 대기, `sent` 없음 |
| user(viewer2) `uds.request 22 F1 90` | user lane에서 대기 |
| viewer `core.stop` | `on:true`, RPC 8.4 ms |
| 진행중 agent write | `E_STOPPED`, `detail.write_state = outcome_unknown`, `row.write_state = outcome_unknown`. viewer 행 `requested→accepted→sent→rx→pending→rx→pending→uds.result outcome_unknown(final)`. **confirmed 없음, final 1개** |
| 대기중 agent read | `E_STOPPED`, row `failed`, reason `aborted before sending (Stop)`, viewer `sent` 0회 |
| STOP 중 새 agent 요청 | gateway가 `E_STOPPED`(domain 미도달) |
| STOP 중 user read | **confirmed**, VIN `LOOMSIM0000000001` (중단된 write의 quarantine quiet 대기 후 송신) |
| audit `write.result` (w1) | `state: "outcome_unknown"` (이전에는 `failed`로 기록됐을 경로) |
| `core.stop.release` 후 1.5 s | write `sent` 여전히 1회, read `sent` 0회, `2ef1a001` 총 송신 1회 → **자동 재송신 없음** |
| release 후 agent `3E 00` | confirmed |

## 구현 (소유 범위만)

1. `UdsHooks::on_latch` (`hooks.rs`): `on=true` → `UdsManager::stop_latch()`, `on=false` → `release_latch()`(거부 플래그만 해제). `UdsHooks::with_manager(Arc<UdsManager>)` 추가, `module.rs` 등록이 이를 사용한다. 동기, 짧은 std lock만, await·장비 I/O 없음. e2e에서 `on_latch(true)` < 10 ms 단언.
2. `UdsManager.stopped: Mutex<bool>` (`manager.rs`): **경합 창 차단.** gateway latch 검사를 STOP 직전에 통과한 agent 호출이 handler 안에 있을 수 있다. agent/script 요청은 (a) lease 전에 한 번, (b) runner 투입 직전에 **같은 lock 아래에서** 플래그 확인 + `RunnerHandle::try_submit`(동기)을 한다. `stop_latch`는 같은 lock으로 플래그를 먼저 세우고 그다음 gate 대기열 취소 + runner abort를 한다. 그래서 요청은 abort 전에 runner에 들어가서 취소되거나, 플래그를 보고 미송신 `failed` + `E_STOPPED`로 거부된다. STOP 후 송신은 없다.
3. `stop_latch` 요약에 runner 큐 대기 취소 수도 더한다(기존에는 버림).
4. `Outcome::into_result`: 에러 detail 최상위에 `write_state`를 추가했다. gateway는 실패 호출의 상태를 `detail.write_state`에서 읽는다(audit `write.result`, fallback 행). 없으면 `failed`로 기록되어 handoff 후 중단된 write가 audit에 거짓 `failed`로 남았다.
5. `write_did` 읽기 검증(`<rid>.verify`)이 STOP/lease/큐 가득으로 실행 불가하면 `?`로 에러를 전파하지 않는다. confirmed write를 `effect_unverified`로 남긴다. 확정된 쓰기가 실패로 바뀌는 거짓을 막는다.
6. `loom_uds::sim_support` (`#[doc(hidden)]`): `loom_uds_doip`, `loom_uds_sim` 재노출. gateway manifest를 바꾸지 않고 smoke가 느린 ECU를 실제 entity 뒤에 둘 수 있게 한다. 대안은 아래 제안 1.

user 트래픽·safe-state recovery(Abort lane)·`uds.abort`는 변경 없음. release는 거부 플래그만 내리며 취소된 호출은 이미 `E_STOPPED`로 반환되어 재개할 대상이 없다.

## 변경 파일

- `crates/loom-uds/src/{hooks,manager,module,handlers,lib}.rs`
- `crates/loom-uds/tests/e2e.rs`: `on_latch_hook_stops_agent_and_script_release_resends_nothing` 추가. 등록된 `WriteHooks`로 STOP한다. script write in-flight → `outcome_unknown`, agent read 대기 → `failed` 미송신, STOP 중 새 agent → 미송신 거부, user confirmed, release 후 재송신 없음, agent 재개.
- `crates/loom-gateway/tests/uds_stop_smoke.rs` (신규), 본 보고서.

## 명령과 결과 (`CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target`, offline, locked)

- `cargo fmt -p loom-uds -- --check`, `rustfmt --edition 2024 --check uds_stop_smoke.rs`, `git diff --check`: PASS
- `cargo clippy -p loom-uds --all-targets -- -D warnings`: PASS
- `cargo test -p loom-uds -p loom-uds-runner -p loom-uds-doip`: **118 PASS / 0 FAIL** (UDS e2e 7 포함)
- `uds_stop_smoke`: 총 **4/4 PASS**. 기본 feature 1회(7.9 s), TEST 제외 feature 3회(7.6–7.8 s).
- gateway(TEST feature 제외): can_eth 3, daemon 9, hub 5, ipc 6, journal 4, replay 8, uds_domain_smoke 1, uds_tc1_smoke 2/3 PASS. `uds_sim_request_reaches_live_viewer`(STOP agent 차단/user 허용/journal 복구)도 PASS. 실패 4건(`r1::domain_smoke_rpc`, `test_services_smoke` 2, `tc1_start_reports_current_gateway_wiring`)은 전부 `test` domain 부재(`method not found: test.start`)이고 feature 제외의 산물이다.
- 로그: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/374ed98f-bda6-491d-87ac-4431aa29c77e/scratchpad/{test-uds-crates,stop-smoke,clippy-uds,clippy-smoke,test-gateway,test-gateway-rest}.log`

### 검증 환경 주의

- **TEST feature 제외 이유**: 검증 도중 `crates/loom-test/src/engine.rs`가 TEST 작업으로 미완 상태(`step_end` 인자 3개 중 2개 전달 E0061, dead `transition`)가 되어 기본 feature gateway가 컴파일되지 않았다. TEST 파일은 수정 금지라 `--no-default-features --features echo,uart,can,uds,eth,psu`로 검증했다. smoke 자체는 그 전에 기본 feature에서 PASS했다. **TEST 복구 후 메인이 기본 feature `cargo test -p loom-gateway`를 1회 재실행할 것을 권한다.**
- 디스크 정리 중 shared target의 `deps/`가 비고 `.fingerprint/`만 남아 `extern location ... does not exist`가 2회 발생했다. 다른 cargo가 lock을 잡고 재구축한 뒤 lock 직렬화로 재실행해 해결했다. 새 격리 target은 만들지 않았다.
- `tests/uds.rs::every_defined_sid_has_a_rule_and_all_256_classify_fast`가 load average 15.7에서 1회 1.28 ms > 1 ms로 실패했다. 단독 재실행 3/3 PASS. classify는 본 변경과 무관하며 debug 빌드 wall-clock 단언의 부하 민감성이다.

## 메인 결정/제안 (공유 파일, 적용 안 함)

1. **gateway dev-dependency 대안**: `sim_support` 재노출 대신 `crates/loom-gateway/Cargo.toml` `[dev-dependencies]`에 `loom-uds-doip = { path = "../loom-uds-doip" }`, `loom-uds-sim = { path = "../loom-uds-sim" }`를 추가할 수 있다(Cargo.lock 갱신 동반). 채택하면 smoke의 `use loom_uds::sim_support::…` 2줄을 직접 경로로 바꾸고 `sim_support`를 제거한다.
2. **재시작 시 latched**: `daemon::start`는 latch 파일이 on이어도 `on_latch(true)`를 부르지 않는다. 현재는 무해하다(부팅 직후 UDS 대상 없음, gateway latch 검사가 새 agent 쓰기 차단). 다만 재시작 후 latched 상태로 연결된 대상에서는 위 경합 창 차단이 gateway 검사에만 의존한다. 메인 scaffold의 start latched 분기에서 domain hooks에 `on_latch(true)` 1회 호출을 권한다.
3. **STOP 가시성(선택)**: `core.latch` payload(`acked`, `jobs_stopped` 등)에 UDS 취소 결과가 나타나지 않는다(`on_latch`가 `()` 반환). 사용자 화면에는 각 요청 행의 `outcome_unknown`/`failed`가 실시간으로 보이므로 정확성 문제는 아니다. 요약을 원하면 `on_latch` 반환형 확장이 필요하다(domain-api 변경, 메인 결정).

## 자체 리뷰 / 남은 문제

- lock 순서: `stopped` → runner 내부 lock(try_submit)만 있다. `stop_latch`는 `stopped`를 놓은 뒤 lanes/runner lock을 잡는다. 역순 경로가 없어 교착이 없다. std lock을 await 너머로 잡지 않는다.
- handoff 전 취소는 `failed`(미송신), handoff 후 write는 `outcome_unknown` + runner U(quarantine)다. 늦은 응답은 기존처럼 `uds.rx late`로 표시된다.
- `uds.connect`(routing activation)와 `uds.status`/`uds.trace`는 write class가 아니라 STOP 중에도 agent가 호출할 수 있다(기존 동작, 진단 요청 송신 없음).
- 미검증: 실하드웨어(금지), CAN-TP, 브라우저 UI 렌더(WebSocket 수준까지 검증).

## 다음 행동

메인이 소유 파일을 검토·통합하고 제안 1–3을 결정한다. TEST 복구 후 기본 feature gateway 테스트를 재실행한다. 본 담당은 역할 반환 후 대기한다.
