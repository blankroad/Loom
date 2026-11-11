# can-opus P1 — `crates/loom-can/src/gateway.rs` + `cli.rs` (G1, K3) + 스킬 v0 (D0)

먼저 `docs/impl/can/tasks/p0-common.md`(공통 규칙 — 단, 이제 모든 crate는 **workspace 멤버**: 검사는 worktree 루트에서 `cargo fmt --all --check`, `cargo clippy -p loom-can --all-targets -- -D warnings`, `cargo test -p loom-can`). 경로는 항상 `/Users/ctmctm/workspace/Loom/...`.

**범위(이 파일들만 수정):** `crates/loom-can/src/gateway.rs`, `crates/loom-can/src/cli.rs`, `crates/loom-can/tests/gateway*.rs`·`tests/cli*.rs`(새로), `skills/loom-can/SKILL.md`(새로). `lib.rs`·`schema.rs`·`Cargo.toml`는 리드 소유 — 바꿀 것이 있으면 보고서에 요청(의존성 추가 포함). `worker.rs`는 다른 작업자가 동시에 만든다(건드리지 말 것). 보고서: `docs/impl/can/reports/p1-gateway-cli.md`.

## 읽을 것
- 견본 도메인 `crates/loom-echo/src/{lib,gateway,cli}.rs` — 이 형태를 따른다.
- 계약 `docs/contract/02-events.md`, `03-cli.md`, `04-safety.md`, `07-test-session.md`, `08-domain-integration.md`; `crates/loom-domain-api/src/*.rs`(Registry·hooks·ctx·verify), `crates/loom-cli-kit/src/lib.rs`, `crates/loom-testkit/src/*.rs`.
- CAN 스키마 v1: `crates/loom-can/src/{lib,schema}.rs` + `docs/impl/requests/can-ui.md`(UI에 약속한 모양 — 이대로).
- 계획: `docs/plan/can/plan.md` §4.1·§4.3(CLI), §5(패널 데이터), §9.5(증거), §10(위험 등급; **마스터 F1 = 기본 모두 자동**, 등급은 표시만).

## gateway.rs (G1, `DomainModule` `Can`)
1. 명령(RoleSet은 echo 기준):
   - `can.send`(write `Request`): `dry_run`이면 worker를 부르지 않고 `{risk, reason, bytes, policy:"auto"}` 반환(R0). 아니면 `SendParams::resolve_frame()`으로 먼저 검증(`E_USAGE`) 후 `ctx.worker(WORKER_KIND_SIM)?.write(rid, params, deadline)`. `expect`가 있으면 worker 결과 뒤 아래 expect 로직으로 응답 확인 → 결과에 `effective_verified|effect_unverified` + 증거.
   - `can.trace.query`(R0): `can-ui.md` §4 인자. `ctx.journal().scan`, 필터(kinds `can.frame`/`can.tx`, channel, ids, origins, dir), `max` 기본 100·상한 1000·64 KiB, 결과 `rows`·`next_cursor`·`truncated`·`range{seq_a,seq_b}`. **agent role이면 `core.activity{client_id, action:"read", domain:"can", channel, range}`를 `ctx.publish`**(viewer면 안 함).
   - `can.last`(R0): 채널의 id별 마지막 프레임(Fixed). 상한 있는 저널 역방향 스캔으로(최근 N초/N행 창, 상한 명시).
   - `can.expect`(R0, 대기형): `{channel, id, ext?, data?, mask?, origin?, after: cursor, within_ms ≤ 60000}`. **M1은 10 ms 간격 저널 폴링**(CORE에 `ctx.wait` 요청 중 — `docs/impl/requests/can-core.md` 1). 매치 → `evidence:[{ref:"can:<ch>@seq:<n>", kind:"can.frame", …요약}]`; 시간 초과 → 계약 E2의 exit 3 오류(`E_TIMEOUT`) + `evidence`에 timeout 창 ref(`kind:"can.wait_timeout"`). 대기 중 `core.activity{action:"wait", predicate}` 1회 publish.
   - `can.status`(R0): worker call 전달.
2. 이벤트 kind 등록: `can.frame`, `can.tx`, `can.channel_state`, `can.stats` — payload 스키마는 `schema.rs` 타입의 schemars 스키마, 고정 pointer(`/payload/id` u32, `/payload/ext` bool, `/payload/fd`, `/payload/dlc`, `/payload/len`, `/payload/bytes/data` string, `/payload/write_state` enum).
3. 위험 표 + `WriteHooks::classify`: sim 채널 = R1 "sim channel"; `count > 10` = R3 "finite burst > 10"(표시용); `default_action` 없음(F1 자동). `presend_check`는 채널 generation 확인.
4. Reducer(늦게 연 화면용 snapshot): 채널별 id당 마지막 프레임(Fixed, 채널 8·id 2,048 상한, 넘으면 가장 오래된 것 교체 + 카운트) + 채널 상태.
5. `EvidenceVerifier`: 술어 `can.frame_match{id, ext?, data?, mask?, dir?, origin?}`(존재 의미, coverage 규칙은 trait 따름).
6. `WorkerSpec`: kind `can-sim`, `Instances::Shared`, budgets(mem_soft 192 MiB, mem_hard 256 MiB — plan §9.4), 모든 플랫폼, `reopen_after_crash` = 수신만(가능한 값 중 가장 가까운 것).
7. `HelpSpec`(정본 `loom can help --json`): 요약·예시·규칙(장치를 다른 도구로 열지 말 것, UI 링크 안내).

## cli.rs (K3, `DomainCli` `CanCli`)
`loom can send <frame> [--channel ch0] [--ext] [--count N --gap MS] [--expect ID --within MS] [--dry-run]`, `trace [--since C] [--until C] [--ids a,b] [--origin agent,device] [--dir rx|tx] [--max N] [--peek]`, `last [--id X]`, `expect --id X [--data HEX --mask HEX] [--origin O] [--after C] --within MS`, `status`. 사람용 출력은 Trace 한 줄 형식(`812.004  tx AGT 0x300  1  01  sent`), 마지막 줄 요약(행 수·커서·잘림), `--json`은 봉투 그대로(`cli-kit` 렌더러 사용).

## 시험 (testkit, 실HW·실제 데몬 없이)
- `RegistryProbe`로 등록 결과(명령·kind·pointer·위험 표·worker spec·help) 검사.
- `HookHarness`로 classify(R1/R3)·presend·1 ms 예산.
- trace.query·expect·last: testkit의 가짜 journal/ctx(있으면) 또는 작은 테스트용 `JournalReader` 구현으로 필터·상한·next_cursor·activity publish·timeout 증거.
- evidence verifier: 일치·불일치·gap coverage.
- CLI: 인자 → RpcCall 변환과 사람용 렌더 골든.

## 스킬 v0 (`skills/loom-can/SKILL.md`)
plan §15.5 골자(≤ 60줄, "As a test step" 절 포함). M1에 없는 명령(`sig`, `check`, `stats`, `periodic`)은 "M2"로 표시하거나 빼라.

커밋하지 않는다. 끝나면 보고서를 쓰고 멈춘다.
