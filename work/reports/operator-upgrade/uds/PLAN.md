# UDS/DoIP operator upgrade — PLAN (Opus module lead)

Status: **PLAN_READY_BATCH1** (batch 1 = U4 + U3 + U7, UI only). Batches 2–4 are planned here at contract level and
get their FUNCTIONS.md sections (B2-*, B3-*, B4-*) before each handoff. Not DONE until every batch is accepted.

- Baseline `610029e`, worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-uds`, reviewed input
  `work/reports/usability-benchmark/RETURN-uds.md` (U1–U8), constraints `COMMON.md v1` (incl. "Function-contract edge cases").
- Roles (ROOT correction 2026-11-15): Opus = architecture, types, signatures, imports, static metadata/DOM construction,
  CSS, tests, review. Sonnet (`upgrade-uds-sonnet`, w3P:p2) = product function bodies only, including callbacks, event
  handlers, reducers and RPC wiring functions. Opus writes no product function body.
- Intent: a usable operator tool at the existing information density. No vendor-suite cloning, no ODX/CDD import, no
  CAN-TP/EthDiag/VIR, no flashing.

## 1. Finding coverage

| ID | Pri | Decision | Batch | Main owned files |
|---|---|---|---|---|
| U4 | P1 | Fix table tracks: response column last, rows fixed min-width (no `max-content`), cells `min-width:0`. Time column fits ms (108px, ISO title). ECU 72px with full-name title. Export buttons get their own class and spacing. Hex dump 8/16 bytes by pane width. | **B1** | consolePanel.ts, uds.css, udsModel.ts |
| U3 | P1 | Decode the ISO 14229-1 status bits (8, fixed abbreviations). Show availability mask (byte 3 of `59 02`). Filter all/confirmed/testFailed/pending, response order kept. Raw code and status byte unchanged; no SAE re-mapping. UI only. | **B1** | udsModel.ts, consolePanel.ts, uds.css |
| U7 | P2 | Target with no profile: branch the link-down text; add a "이 대상으로 프로필 만들기" button. `openCfg` builds a NEW draft from uds.state (name, LA from `addr`, backend sim) and never falls back to `presets[0]`. Host stays empty, so the user must fill it. | **B1** | udsPrefs.ts, udsModel.ts, consolePanel.ts |
| U8 | P2 | Engine: `RowPayload` gets optional `generation`, `backend`, `addr`, stamped once when the row is created from the TargetSession (constant across the row's events). UI: route column, detail and export use the row stamp. Rows without a stamp show `?` with the tooltip "요청 이벤트에 경로 미기록". Nothing is back-filled from current state. | B2 engine, B4 UI | payload.rs, manager.rs, fixtures; udsModel.ts, consolePanel.ts |
| U6 | P1 | New RPCs `uds.disconnect {target, abort?}` and `uds.forget {target}`, plus LinkState `closed`. `uds.connect` on a Closed or dead-Down target replaces it with a fresh link and continues the generation (no reuse of gen numbers). Forget publishes `uds.state{forgotten:true}`; the reducer and the UI drop the target. UI gets [해제]/[목록에서 제거] buttons and a "재연결 필요" badge. | B2 engine, B4 UI | manager.rs, session.rs, payload.rs, module.rs, handlers.rs, cli.rs; consolePanel.ts |
| U1 | P1 | New RPC `uds.tester_present {target, on, interval_ms, suppress=true}`, default off. One job per target, daemon-owned task. Skips while the gate is busy or other traffic happened within the interval. Stops on link down, generation change, disconnect/forget, STOP latch (all owners), a lease taken by another client, or 3 consecutive failures. Never auto-restarts, also not after STOP release or reconnect. One `uds.result` row per exchange (`purpose:"tester_present"`), folded by the UI. `uds.state.tester_present` gains `owner`, `suppress`, `stopped_cause`. Fix the catalog comment and the double-count in the TP cell. | B2 engine, B4 UI | new `tester_present.rs`, manager.rs, lanes.rs (`try_acquire`), session.rs, payload.rs, module.rs, handlers.rs, cli.rs; udsModel.ts, consolePanel.ts |
| U5 | P1 | Attach the existing `loom_diag` v1 DB per target, inline: `uds.db.set {target, db}` (≤48 KiB serialized because the RPC max is 64 KiB) and `uds.db.clear`. The daemon never reads a path. `decoded` uses the DB first, then the standard table. Rows and state carry `db_hash`. Risk never lowered (unchanged `risk.rs`). DB names are untrusted text and rendered with textContent. CLI `loom uds db set --file` reads client-side. UI: DB in the project file, validate-preview, attach. | B3 engine, B4 UI | new `diagdb.rs`, manager.rs, payload.rs, module.rs, handlers.rs, cli.rs; udsTools.ts, udsPrefs.ts |
| U2 | P1 | (a) UI minimum: on a `67 xx` row, buttons to copy the seed and fill a `27 (xx+1)` template, never sent. (b) Engine `uds.security_unlock {target, level(odd), provider}`: the provider is configured ONLY in loom.toml `[[uds.key_providers]]` {name, command(abs path), args, timeout_ms ≤10000, levels}, read through `ctx.config()`; an RPC can only name it. It is spawned with a cleared env and seed hex on stdin. Stdout is one hex line (≤1024 hex chars); stdout/stderr capped at 4 KiB; killed on timeout; never retried. An all-zero seed means already unlocked (no key sent). The key request is a normal row `<rid>.key`, R3. The composite is classified R3 through the 0x27 Even rule (`typed(..,"uds.security_unlock")`), and the gateway policy, STOP and lease apply. Provider stdout/stderr are untrusted, never echoed beyond 200 chars, never parsed as anything but hex. | B3 engine, B4 UI | new `keyprovider.rs`, risk.rs (method tag only), hooks.rs, text.rs, policy.rs, handlers.rs, module.rs, cli.rs, Cargo.toml (tokio `process` feature) |

Shared and other-owner needs: **none required**. Detail:
- UDS payload schemas are not in `schema/v1` (checked). They are generated from owned Rust types (`fixtures/*.json`).
- `crates/loom-gateway/tests/uds_domain_smoke.rs` and `ui/src/sdk/sdk.test.ts` mention only existing methods; adding methods does not break them.
- `crates/loom-uds/Cargo.toml` (owned) adds the `process` feature to the existing workspace tokio. Same precedent as loom-worker-host. No new crate; Cargo.lock unchanged.
- New panel actions (`disconnect`, `forget`, `tp`, `unlock`, `db_set`) are declared in the owned `registerPanel` manifest.
- ROOT confirmed: `ui/src/sdk/manifest.ts validateManifest` checks the RPC regex only. There is no central method
  allow-list, so new `uds.*` methods declared in this module need no SDK change. ROOT runs gen-schema --check and any
  central catalog regeneration once at integration; generated types are not edited here.
- Not changed: CORE ErrorCode (existing codes reused), core journal/HTTP/artifact, desktop, root Cargo.toml, schema/v1.

## 2. Cross-cutting rules (all batches)

- **Truthful provenance:** generation, route and db_hash are stamped when the event is made. Unknown stays unknown (`?`/null), never reconstructed from current state. New optional fields use `skip_serializing_if` and are tested for backward compatibility: old fixtures still deserialize, and the UI renders old rows.
- **STOP:** stop_latch stays synchronous and bounded. It also stops TP jobs. STOP release resumes nothing. Disconnect and forget send no ECU bytes and are allowed during STOP (reduce direction).
- **Lease:** agent/script cannot disconnect, forget, start TP or unlock while the user holds the lease (`E_HELD_BY_USER`). A user may always do so.
- **Bounds:** TP interval 500–30000 ms. Provider timeout ≤10 s, output ≤4 KiB. Inline DB ≤48 KiB. Targets ≤16 (closed ones count until forgotten). TP failures before auto-stop: 3.
- **Untrusted text** (ECU/DB/provider) goes in via textContent only (the existing `el()` helper). No innerHTML with such data.
- No timers in the browser for device traffic. TP is a daemon job.

## 3. Batches, file leases, order

| Batch | Sonnet lease (files) | Opus during lease | Gate |
|---|---|---|---|
| **B1** U4+U3+U7 | function bodies in `ui/src/panels/uds/{consolePanel.ts,udsModel.ts,udsPrefs.ts}` (uds.css + static scaffold done by Opus before the grant) | writes B2 FUNCTIONS + scaffolds B2 types/signatures/tests in `crates/loom-uds/**` (not leased in B1); never touches the B1 files | `vitest run --maxWorkers=2 src/panels/uds` all green, `tsc -b` clean, eslint on changed files |
| B2 U8+U6+U1 engine | `crates/loom-uds/src/{manager.rs,session.rs,payload.rs,events.rs,lanes.rs,module.rs,handlers.rs,cli.rs,tester_present.rs}` | plans B3 (diagdb.rs/keyprovider.rs new files; scaffold only after B2 lease returns for shared files) | `cargo test -p loom-uds` (CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target, CARGO_BUILD_JOBS=2), clippy -p loom-uds, fixtures regenerated by the existing test |
| B3 U5+U2 engine | `crates/loom-uds/src/{diagdb.rs,keyprovider.rs,hooks.rs,text.rs,policy.rs,risk.rs(method tag),manager.rs,handlers.rs,module.rs,cli.rs}`, `skills/loom-uds/**` | plans B4 | cargo tests incl. a provider fixture script under `crates/loom-uds/tests/fixtures/` (Opus-authored) |
| B4 UI for U8/U6/U1/U5/U2 | `ui/src/panels/uds/**` | review | vitest + tsc + eslint; ROOT GUI captures (U4 alignment, U7-2, U3 DTC, TP toggle, disconnect) |

Sonnet works one batch at a time. Each return gets a diff review plus targeted tests by Opus. Corrections go only
through the named Herdr surface.

## 4. Regression examples (from RETURN-uds, become tests)

1. U4: a VIN row (20 B response) and a 3 B NRC row in one table. Header and every row have identical
   `gridTemplateColumns`, the response is the last cell, and status/elapsed are visible without horizontal scrolling (ROOT GUI).
2. U3: `59 02 FF 92 34 00 09 C1 00 00 2F` → 923400 TF·CDTC, C10000 TF·TFTOC·PDTC·CDTC·TFSLC, mask 0xFF; pending filter → C10000 only.
3. U7: `qa-native-ecu` (no profile, link down) → no "[연결]로 다시 연결"; [연결 설정] opens a new draft with ecu=qa-native-ecu,
   LA 1001, empty host, and stored presets untouched.
4. U8: rows of gen 1 and gen 2 (reconnect) export different `generation`; reopening the panel shows the stamped route, not `?`.
   A legacy row without a stamp → `?` + tooltip.
5. U6: connect sim → disconnect → state `closed`, no reconnect after the entity drops. Connect with new params → gen 2, `replaced:true`.
   Forget → target gone from status/snapshot. Disconnect while a write is in flight without `abort` → E_BUSY, nothing changed.
   With `abort` → the write ends outcome_unknown, then closed.
6. U1: TP on 500 ms over sim → ≥2 `purpose:tester_present` rows, `3E 80`, ok. A user request in flight → that tick skipped.
   Link loss → job stopped, `stopped_cause:"link down"`; after reconnect still off. STOP latch → stopped; release → still off.
   Lease takeover by another client → stopped. Interval 100 → E_USAGE.
7. U5: DB with DID 0101 uint scale 0.1 unit V → `22 0101` → decoded `{name, value:12.3, unit:"V"}`, and the row has `db_hash`.
   49 KiB DB → E_USAGE. A DB with `writable:true` does not lower `2E` risk.
8. U2: provider script returns the key → `27 01`/`27 02 <key>` rows, security L1 confirmed. Zero seed → no key row.
   Provider timeout, oversize output or non-hex output → no key sent, error E_CONFIG/E_TIMEOUT, row `<rid>.key` never created.
   Provider not in loom.toml → E_CONFIG. Agent with user lease → E_HELD_BY_USER before any byte.

## 5. Risks / open decisions (recorded, defaults chosen)

- STOP also stops user-owned TP jobs (COMMON says "stop on STOP"; keepalive is automatic traffic). Reduce direction.
- The existing S3 `tick()` is still never called in production. Not wired here (it would assume S3 = 5 s). Noted, out of scope.
- The seed/key bytes stay visible in rows, like today's manual flow (transparency principle). The provider binary itself is never published.
- U7 reconnect of a dead Down target only works after B2 (connect-replace). Until then B1 creates the profile but `uds.connect`
  still answers `existing:true`. B4 adds the "재연결 필요" badge.
