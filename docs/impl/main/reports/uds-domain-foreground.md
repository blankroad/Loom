docs/plan/02-core-principle.md 준수
# READY_TO_INTEGRATE — UDS 실제 domain 연결 (foreground)

- 기능 / 상태 / 담당: UDS 실제 domain 연결 인수 / **READY_TO_INTEGRATE** / Herdr Claude Opus 5.5 포어그라운드 직접 수행. 하위 에이전트 없음, 자체 리뷰만(독립 리뷰 없음).
- 기준: 시작 main `302a5d5`. 작업 중 main이 `3a01ecf`(UI `bytes_hex`)를 커밋. 최종 검증은 `3a01ecf` 작업 트리에서 했다. commit/push/index/merge/cherry-pick/reset/clean은 하지 않았다. `.worktrees/uds`는 읽기만 했고 미커밋 상태를 보존했다.
- 이식 원천: `.worktrees/uds`의 미커밋 domain P1-b(`module/hooks/lib/Cargo.toml` diff와 신규 `events/handlers/lanes/manager/policy`, `tests/e2e.rs`). `patch -p1`과 파일 복사로 working tree에만 반영했다. runner/DoIP는 main과 동일함을 확인했고 변경하지 않았다. `loom-uds-load`는 범위 밖이라 이식하지 않았다.

## 결과 (실제 sim, 실제 gateway)

새 `crates/loom-gateway/tests/uds_domain_smoke.rs`: 격리 tempfile HOME, production `daemon::start`, 실제 WebSocket agent와 viewer를 쓴다. `uds.connect {sim:"bench"}`는 loopback `DoipEntity` 뒤의 `SimEcu::bench`에 실제 `DoipClient`와 runner로 연결한다.

| 단계 | 실제 결과 |
|---|---|
| 연결 전 `uds.status` / `uds.request` | `E_NOT_OPEN` (가짜 성공 없음) |
| `uds.connect {sim:"lab"}` | `E_USAGE` (알 수 없는 profile) |
| `uds.connect {name:ecu1, sim:"bench"}` | ok, generation 1, backend sim, entity `127.0.0.1:<port>` |
| `uds.request 22 F1 90` (agent) | `confirmed`, resp `62f190`+`LOOMSIM0000000001`, decoded VIN |
| viewer 실시간 행(같은 request_id) | `uds.tx requested → accepted → sent(bytes 22f190, origin agent) → uds.rx positive(같은 bytes) → uds.result confirmed final:true`, seq 순증, 중복 없음 |
| 시간축 | row `t_send_mono_ns`와 이벤트 `ts.mono_ns`(hub) 차이 < 1 s 단언. 실측은 동일 축 |
| `22 12 34` | `E_NRC "NRC 0x31 requestOutOfRange"`, row failed, resp `7f2231` |
| 사용자 개입 | viewer `uds.session extended` → confirmed, user lease. agent `uds.session default` → presend `E_LOCKED`(미송신). viewer `uds.lease.release` 후 agent → confirmed |

main이 별도로 수정한 `uds_tc1_smoke.rs::uds_sim_request_reaches_live_viewer`(STOP → agent `E_STOPPED`, user confirmed, 재시작 후 journal 동일 복구)도 본 구현에서 PASS한다.

## 변경 파일 (소유 범위)

- `crates/loom-uds/Cargo.toml`: deps `loom-uds-doip`, `loom-uds-runner`, `tokio`, `tracing`. dev-deps `loom-worker-sdk`, `tokio[test-util]`.
- `crates/loom-uds/src/{lib,hooks,module}.rs`(수정), `src/{events,handlers,lanes,manager,policy}.rs`(신규), `tests/e2e.rs`(신규). `tests/uds.rs`는 HEAD와 동일(무변경).
- `crates/loom-gateway/tests/uds_domain_smoke.rs`(신규), 본 보고서, 아래 patch 파일 2개.

worktree 원본 대비 조정한 부분:
1. **gateway 무변경 연결**: `UdsModule`을 unit struct로 유지했다. `register()`마다 새 `UdsManager`를 만들어 handler와 hook이 공유한다. 그래서 `domains.rs`의 `load(&mut out, &loom_uds::UdsModule, &[])`가 그대로 동작하고 **domains.rs/gateway manifest 변경 제안은 불필요**하다. 테스트나 STOP 연결용으로 `UdsModule::with_manager(Arc<UdsManager>) -> ManagedUdsModule`을 추가했다.
2. **시계**: `SystemClock` 대신 `HubClock::from_ctx(ctx)`를 쓴다. connect 때 `CallCtx::now_mono_ns()`(CORE R1 API, hub 시계)에 고정하고 runner `MonoClock`과 같은 기준을 쓴다. e2e Fake는 `now_mono_ns`를 구현한다.
3. **중복 행 제거**: gateway write path가 call의 request_id로 이미 `requested`/`accepted`를 발행한다. domain이 같은 rid로 다시 발행하면 viewer에서 `accepted → requested` 역행(거짓 상태)이 실측됐다. 그래서 core 소유 rid에서는 domain이 이 둘을 생략한다. 하위 요청(`<rid>.verify`)은 전체 순서를 그대로 발행하고 e2e로 단언했다.
4. `uds.abort` 스키마에 `scope` 추가(handler가 이미 읽던 필드). `connect.sim`은 `true` 또는 `"bench"`를 받는다(SKILL.md `--sim bench`와 일치). 다른 값은 `E_USAGE`.
5. CORE R1 API 호환: e2e `ScanResult.scanned_through`.

## 명령과 결과 (`CARGO_TARGET_DIR=~/.cache/loom-main-target`, offline)

- `cargo fmt -p loom-uds -- --check`, `rustfmt --edition 2024 --check crates/loom-gateway/tests/uds_domain_smoke.rs`: PASS
- `cargo clippy -p loom-uds --all-targets --locked --offline -- -D warnings`: PASS
- `cargo clippy -p loom-gateway --test uds_domain_smoke --locked --offline -- -D warnings`: PASS
- `cargo test -p loom-uds -p loom-uds-runner -p loom-uds-doip --locked --offline`: **117 PASS / 0 FAIL** (domain 17 = unit 2 + e2e 6 + fixtures 2 + uds 7)
- `cargo test -p loom-gateway --locked --offline`: **49 PASS / 0 FAIL** (`uds_domain_smoke` 1, `uds_tc1_smoke` 2, r1, daemon, ipc, journal, replay, hub, can_eth 포함)
- `uds_domain_smoke` 5회 반복 5/5 PASS. `cargo check --workspace --all-targets --locked --offline` PASS. `git diff --check` PASS.
- 로그: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/8ee23a41-8364-4feb-b290-6d19155b0853/scratchpad/{clippy-uds,clippy-smoke,test-uds-crates,test-gateway,check-ws}.log`

## 공유 파일 — 메인 결정 요청

1. **`Cargo.lock`(+5줄, loom-uds 의존 목록만)**: `crates/loom-uds/Cargo.toml` 변경에 따라 cargo가 자동 갱신했고 working tree에 남겨 두었다. 되돌리면 이 tree의 `--locked` 빌드와 main이 동시에 돌리는 uds_tc1_smoke가 깨지기 때문이다. 같은 내용을 `docs/impl/main/reports/uds-domain-foreground.Cargo.lock.patch`로 분리했다. 원치 않으면 `patch -R -p1 < 해당 파일`로 되돌릴 수 있다. **지시(공유 manifest는 제안만)와 어긋나는 점을 명시한다.**
2. **`uds-domain-foreground.gateway-dry-run.patch`: UNSAFE, 적용 금지.** main 리뷰 판정대로, 모든 `Request + dry_run:true`를 read로 보내면 dry_run을 무시하는 다른 domain handler가 STOP·승인·audit 없이 실제 송신할 수 있다. 반례 기록용으로만 남긴다(파일 머리에 UNSAFE 표기). gateway 소스에는 적용하지 않았다(`patch --dry-run`만 실행). 현재 `daemon.rs` 미커밋 변경(`dry_run_previews`)은 main 작업이며 손대지 않았다.

### dry_run 반례 (정확한 재현)

viewer가 `uds.request {target:"ecu1", bytes_hex:"2EF1A002", dry_run:true}`를 보낸다(UI 콘솔 미리보기와 같은 호출). 현 gateway(`3a01ecf` 기준)는 이를 write path로 처리한다. 그 결과 viewer에 같은 rid로 `uds.tx requested(user)`, `uds.tx accepted(reason: 0x2E WDBI)`, `uds.tx confirmed final:true`가 보이고, 응답 envelope에 `state:"confirmed"`가 붙는다. R2 쓰기가 **송신되지 않았는데 confirmed로 표시**된다(거짓 성공). 또 write.intent R2 audit와 presend(lease) 검사가 미리보기에도 적용된다. domain 쪽 dry-run 결과 자체는 순수하다(이벤트 없음, `dry_run:true`, risk R2). main의 Registry preview 선언이 들어오면 `uds.request`의 preview 선언(roles) 추가가 loom-uds 쪽 후속 1줄 작업이다.

## 남은 문제 (미해결, 범위 밖)

- STOP 시 **진행 중** UDS 요청 중단은 미연결이다. `UdsManager::stop_latch()`가 있으나 gateway STOP에서 호출하는 Registry hook이 없다. 새 agent 쓰기는 core가 `E_STOPPED`로 막는다(검증됨).
- agent의 첫 R1+ 쓰기가 lease를 agent로 잡고 release 전까지 유지한다(status에 표시). user는 막지 않는다.
- 링크 손실·재연결 등 배경 이벤트는 connect 호출의 ctx(origin/client)로 발행된다.
- `uds.disconnect`가 없다. sim entity와 target은 daemon 수명 동안 유지된다(최대 16).
- connect 파라미터가 `sim/host/port/la/tester_la/activation`으로 바뀌었다(구 스키마 `transport/ip/sa` 폐기). CLI에는 uds 매핑이 없다. functional addressing은 `E_UNSUPPORTED`다.
- 미검증: 실하드웨어(금지), CAN-TP, 브라우저 UI 렌더(본 스모크는 WebSocket 수준). DoIP 실장비 경로는 sim과 같은 client/runner 경로이지만 실측은 하지 않았다.

## 다음 행동

메인이 위 소유 파일과 Cargo.lock patch를 검토·통합하고, dry_run은 Registry preview로 해결한 뒤 `uds.request` preview 선언 여부를 결정한다. 본 담당은 반환 후 비활성 대기한다(추가 기능 시작 안 함).

## 메인 통합 판정

- 소유권 메인 인수, 원본 worktree/Herdr 세션 보존. 준비 결과 자체 리뷰에 더해 메인이 실제 송수신·STOP·저널·preview 관문을 독립 검토.
- CORE `Registry::dry_run_preview(method, roles)` 명시 선언과 gateway read routing 추가, UDS만 opt-in. 등록되지 않은 명령에 dry_run을 붙여도 write 관문을 통과해야 함. UDS preview는 READ 권한, 송신/행/승인·audit intent 없음. unsafe blanket patch 미적용.
- main `uds_tc1_smoke` 3/3 PASS: UDS 22F190 confirmed/live viewer/STOP agent 차단+user 허용/journal 동일 복구; watch·agent의 STOP 중 순수 preview(UDS행 없음); echo.send dry_run 위장 송신 E_STOPPED, watch의 실제 UDS송신 E_DENIED. /private/tmp/loom-uds-domain-preview-test.log
- main domain-api+UDS test PASS, gateway/domain-api/UDS all-target clippy -D warnings PASS. /private/tmp/loom-uds-final-{crates,clippy}.log.
- 이 판정은 domain+preview 코드 통합 DONE이다. 진행중 UDS 취소·브라우저 수용·TEST hooks 및 정식 RSS는 미완료. push 없음.
