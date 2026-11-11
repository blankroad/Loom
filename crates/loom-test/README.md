# loom-test

TEST session state/RPC module and `DomainCli` fragment. Device writes continue through the same CORE dispatcher, policy, journal and domain RPCs used by the UI. This crate does not run a second device path or depend on the in-progress predicate crate.

CORE constructs `Registry::for_core("test")` and passes it to `TestModule::register_into` (or `DomainModule::register`). CORE approved this namespace in `docs/impl/requests/core-answers-p1.md`; the host still owns registration. `RegistryProbe::run` currently rejects reserved domains; integration tests verify that rejection and then use a CORE-owned registry to exercise the real registered handlers.

`TestModule::with_services` accepts the actual Registry-backed `loom_test_format::CommandTable`, `VerifierEngine` and `SessionHooks`. The command table must use real command metadata, argument schemas and domain risk classification. A plan cannot start/amend without it. A TC alone can start and record up to 500 markers; automatic PASS/done still needs a registered plan. F1 defaults to immediate approval transitions; `ApprovalMode::Required` waits for the CORE hook's authoritative grant. Amendment invalidates old grants and evidence bindings.

`VerifierEngine::check/decide` runs off the control path with at most two blocking evaluations. Implementations must spend `CheckCtx.budget` cooperatively (200 ms / 200k events), requery each entire registered expectation window through `ctx.journal()`, enforce source roles/attempt fences/coverage, compute digest and pin verified evidence. Witness selection never defines the evaluation population. The default `NotWired` rejects PASS/done with E_NOT_IMPLEMENTED. Engine guards also reject empty PASS, own-TX-only proof, missing iteration/expectation coverage, unknown recording health and absent source fences. Only viewer authority from CallCtx can assert/override; client-supplied role fields confer no rights.

CORE must attach session/step/attempt tags at its publish boundary, supply real fences and recording/STOP health, and call `check_client_write` for **every** device-domain write. `bindings()` exposes client/session associations. Agent/script writes are refused while paused or awaiting approval; viewers keep their normal direct-control rights, subject to CORE's global STOP. Session metadata alone cannot intercept other registries. `set_stop` never runs finally; stopped snapshots are retained/evicted with other terminal sessions.

`SessionHooks::finally` must dispatch the statically checked R0–R1 safe-state actions through normal policy/recorded RPCs, enforce the original plan's use counts/timeouts and STOP between actions, and return `{complete:true}` only after confirmed completion. CORE needs an explicit scoped cleanup context because ordinary owner writes are gated while ending. Agent cancellation runs planned cleanup; only viewer abort can choose to skip it. The default hook is E_NOT_IMPLEMENTED. Cancellation/error after dispatch leaves an unknown outcome and forbids automatic replay. Confirmed cleanup is remembered if terminal-event publication fails; retry only commits metadata. Resume/device work stays blocked after cleanup until an explicit amendment or terminal disposition.

All state/evidence/verdict events publish before their corresponding mutation is committed. If a multi-event RPC fails, the published prefix remains visible. `TestReducer` folds published events into bounded late-join snapshots; rejected proposals preserve the accepted verdict. U64 coordinates/revisions/timeouts serialize as decimal strings. RPC `test.wait` uses separate bounded slots so answers and pause remain available. Ask expiry is observed by status/answer/wait rather than a background timer.

For a daemon-free local checker:

```sh
cargo run -p loom-test -- plan check crates/loom-test-format/tests/fixtures/TC-1.plan.yaml --json
```

The `loom` host registers `TestCli` and invokes `cli::run_local` before daemon dispatch for local plan check/show. `DomainCli` currently has no local-execution hook, hence the explicit helper and executable. Without a registry, `inspect_plan` performs structural checks and warns “명령 존재 미검사”; it emits no invented risk grades. `plan show <session>` dispatches to the daemon and includes real risk counts; a .yaml/.yml target or `--local` selects a local file. `import/report/freeze` return explicit P2 E_NOT_IMPLEMENTED. End envelopes retain exit 0/11/8/6; errors retain the common 4/5/3 mappings.

## Bounds and memory target

All ceilings are exported constants. Defaults: 1 active session (configured ≤4), 500 markers, 64 citations, 1024 concurrent controls (E_LOCKED on overflow, no silent drop), 2 verifications, 1 parser, 64 waiters, 4 KiB × 1000 notes, 16 open asks, 200 recent evidence rows, 8 verdict audit rows per step and ≤8 terminal snapshots subject to earlier byte-budget eviction. Full audit remains CORE's journal; this module is not durable storage.

The engine RSS **target** is 64 MiB. A conservative logical ledger charges 12 MiB per parsed session/temporary plan plus serialized in-flight requests and retained mutations before commitment, rejecting new work at 64 MiB. Blocking tasks retain their permits/charges after caller cancellation. This ledger is not a measured allocator/RSS guarantee: transport buffers, verifier allocations, journal pin storage and the shared process are outside it. Production CORE integration must measure/enforce the RSS target with the real verifier. Source files are regular UTF-8 files ≤1 MiB; JSON depth/nodes/collections inherit the format bounds; event payloads ≤2 MiB; snapshots retain compact summaries rather than full witnesses.
