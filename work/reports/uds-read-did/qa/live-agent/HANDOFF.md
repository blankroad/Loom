# live-agent HANDOFF (Opus, w1K:p1) — SOURCE FROZEN, final CLI ready

## Final CLI (default features; for atomic replace of repo target/debug/loom by ROOT)
- `/tmp/loom-uds-live-build/debug/loom` sha256 `d54e4134f10b84fe3ce4c1e5e0e01cdf677cd317677b1c22c92143a6430a3689`
  (9.7 MB: debuginfo=0). All domains present (echo uart can uds eth psu test).
- `CARGO_INCREMENTAL=0 RUSTFLAGS=-Cdebuginfo=0 CARGO_TARGET_DIR=/tmp/loom-uds-live-build cargo build -q -p loom-cli --bin loom`
  No loomd/gateway built; repo target/ and Cargo.lock untouched; no Git ops.
- Superseded: build 1 uds-only `f3bea8c1…2cc8` (ROOT after-fix.json/resume.json). Copy left at
  `/tmp/loom-uds-live-build/loom-uds-only-build1`. Follow code identical to final; final adds only
  tests (cfg(test)), skill text and the embedded reference. Whole `/tmp/loom-uds-live-build` = ROOT cleanup.

## Source hashes (final)
- crates/loom-cli/src/lib.rs `6769ed162f02c580c29f348b2baf80904bda24559bc941a207df912cc5c6e69e`
- crates/loom-cli/src/skills.rs `469e083eeb63f11b7ea328afc067d2782c05045053d51e1788c8b87c8df89e6c`
- skills/loom-uds/SKILL.md `5d59327ac675a2bc709fb5a5061e788af347755d32857b00bad352a5c7eb8f58`
- skills/loom-uds/references/live-logs.md (new) `0d8b86b5979fa8a5a239b2d184cd187f598613020fc66eceb2c4ff8896f13049`
- Pre-edit = HEAD 47db65d (source-before.sha256); build 1 lib.rs in source-build1.sha256.

## Changes
1. `follow_resume()` (lib.rs): next poll = `next_cursor` if truncated; truncated w/o cursor →
   `range.to+1`; else `e<stream_epoch>:seq:<max(range.to, stream_seq_hw)+1>` (empty pages too).
   Old: empty first page left `since="now"` → re-resolved to hw every poll → silent total loss
   (ROOT before-fix.json). Seq inclusive (gateway `seq:N` → after N-1) so +1 correct; unchanged
   pagination; hub actor returns events+hw atomically, non-truncated = scanned through hw.
2. Truncated page → next poll without 200 ms sleep (each line still ≤ --max / 60 KiB).
3. loom-uds SKILL.md: frontmatter description quoted (was invalid YAML: `M1:`), live-log routing,
   removed claim that the user watches a "Loom UI UDS console".
4. references/live-logs.md: observer command, isolation, request_id correlation (rows that exist;
   terminal uds.result is the outcome), resume formula, gap/epoch/daemon/server-loss, visibility,
   no UI-backend claim. Embedded in skills.rs FILES.

## Checks (source-checks.log, quick-validate.log)
- `cargo test -p loom-cli --lib` (uds-only and default): 6 passed. New: `follow_tests::empty_now_follow_sees_later_events_once_across_capped_pages`,
  `follow_tests::capped_page_continues_at_next_cursor_not_high_water` (model of gateway cursor semantics: now/inclusive seq/max/filter/hw).
- Mutation: reverting to old range.to-only logic fails test (`left: "now", right: "e7:seq:7"`).
- quick_validate skills/loom-uds: valid. `skills sync` into disposable project: 24 copies, both
  roots byte-identical loom-uds SKILL+live-logs, repeat = unchanged; project removed.
- Native evidence is ROOT's (before-fix/after-fix/resume .json); source-local tests above are not native.

## Limitations
- Not run: crates/loom-cli/tests/cli.rs (builds loomd), clippy, workspace tests.
- Follow from an earlier-epoch cursor stays on that epoch's recording (pre-existing; documented).
- Empty polls print nothing; resume from last printed line's hw re-scans but cannot duplicate.
- Browser workbench still has no loomd backend; nothing here claims it.
