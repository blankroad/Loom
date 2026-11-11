docs/plan/02-core-principle.md 준수
# UDS / TC-1 gateway 스모크

- 기능 / 상태 / 담당: UDS·TC-1 현재 연결 진단 / READY_TO_INTEGRATE (진단 시험만) / uds_tc1_smoke 직접 수행, 자체 리뷰. 독립 리뷰 없음.
- 기준: main `1a2e817`, `/Users/ctmctm/workspace/Loom`. root UART 변경과 CAN/ETH 작업은 건드리지 않았다.
- 목적: 현재 실제 gateway RPC를 통해 UDS 송신과 TC-1 시작이 가능한지 확인. 비범위: UDS 코드 이식·제품 구현, 기존 사용자 daemon/STOP 변경, 전체 P1 수용.
- 소유 파일: 신규 `crates/loom-gateway/tests/uds_tc1_smoke.rs`, 본 보고서. 미커밋 인계, index/commit/push 조작 없음.
- 실행: 격리 tempfile HOME의 production `daemon::start`, 실제 WebSocket client. gateway는 시험 프로세스 안에서 실행되며 등록된 IPC worker는 실제 `loomd worker` subprocess다. 브라우저 UI 렌더 시험은 아니다. 해당 UDS API는 worker 자체가 등록되지 않았으므로 장치 I/O까지 도달하지 않는다.

## 실제 결과

`cargo test -p loom-gateway --test uds_tc1_smoke --locked --offline -- --nocapture`: **2 PASS**, 0.89 s. 이는 미구현을 명시적으로 거부하는 진단 테스트 통과이며 UDS/TC-1 기능 통과를 뜻하지 않는다.

| RPC | 실제 결과 |
| --- | --- |
| `uds.status {target:ecu}` | `E_NOT_IMPLEMENTED: not implemented yet: uds.status` |
| `uds.connect {target:ecu,transport:doip,addr:127.0.0.1}` | `E_NOT_IMPLEMENTED: not implemented yet: uds.connect` |
| `uds.request {target:ecu,hex:22F190}` | `E_NOT_IMPLEMENTED: not implemented yet: uds.request (send)` |
| `test.start` + 저장소 TC-1.tc.md / TC-1.plan.yaml | `E_NOT_IMPLEMENTED: actual Registry command/risk table must be injected` |

- 로그: `/private/tmp/loom-uds-tc1-smoke.log`.
- `cargo clippy -p loom-gateway --test uds_tc1_smoke --locked --offline -- -D warnings`: PASS. 로그 `/private/tmp/loom-uds-tc1-smoke-clippy.log`. 생성 시험의 불필요 import를 제거한 뒤 통과.
- `CARGO_TARGET_DIR=/Users/ctmctm/.cache/loom-main-target`. 명시 한도·기한 없음, 제공된 사용량 미상.

## 원인과 가장 짧은 다음 단계

1. main `crates/loom-uds/src/module.rs`의 `UdsModule`은 unit struct로 `NotYet` handlers와 dry-run 전용 request handler를 등록한다. gateway `domains.rs`도 UDS worker 없이 `load(..., &loom_uds::UdsModule, &[])`만 호출한다. prepared `b23171a`는 runner/DoIP 13파일만 바꾸므로 그 커밋 이식만으로 위 RPC는 연결되지 않는다.
2. 기존 `.worktrees/uds`에는 domain `module/hooks/lib/Cargo.toml` 미커밋과 신규 `handlers/manager/lanes/events/policy`, load crate가 별도로 있다. 이 변경은 보존했으며 읽기만 했다. runner/DoIP 안전 수정 이식 후 이 domain 변경을 독립 범위로 검토·검증하고 gateway API 생성자/등록을 맞춰야 UDS 종단 간 스모크가 가능하다.
3. `TestModule::new()`는 `commands=None`, 기본 PredicateEngine, `NotWiredHooks`로 생성된다(`crates/loom-test/src/engine.rs`). gateway는 그대로 등록한다. 첫 차단은 실제 Registry command/risk table 주입 누락이다. 다음에는 production verifier에 도메인 verifier 목록과 PredicateSpec, SessionHooks의 source fence/recording-health/STOP/승인/정상 RPC finally 연결이 필요하다. 기본값으로 TC-1 PASS를 선언하면 안 된다.
4. TC-1의 전원/부팅/CAN/UDS 가상 DUT 상태 연동과 실제 저널 evidence는 별도 수용 항목이다. 이번 진단은 그 상태를 바꾸지 않았다.

후속 소유자: 메인. root가 본 신규 테스트·보고서를 검토하고 진단 변경을 통합할 수 있다. 기존 사용자 세션·장비·데모 STOP은 그대로다.
