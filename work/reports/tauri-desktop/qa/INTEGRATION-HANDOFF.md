# INTEGRATION-HANDOFF — desktop/integration QA (tauri-final-opus) → next integration QA context

2026-11-15 ~11:25 KST. **Role handoff, not goal done.** Bench Light parity is NOT accepted yet. The final build has not
been run (waiting for UI READY). No commits (ROOT owns Git).

## State of the world
- No QA processes are running (no app, daemon, fixture, waiter or WebDriver; ports 47941-47943 free).
- **desktop/target (3.7 GiB) removed.** It was created by my `cargo check`/`test`/`clippy` at 11:04-11:17 without
  `CARGO_TARGET_DIR` and held only build cache plus tauri-build's copy of the staged loomd (sha 49803a53…). Nothing else was removed.
  **Every future desktop cargo command:** `CARGO_TARGET_DIR=/tmp/loom-tauri-build`, under the build-app mkdir lock
  `/tmp/loom-tauri-build/.cargo-desktop.lock` (build-app.sh takes it automatically; take it manually for ad-hoc tests).
- Kept: /tmp/loom-tauri-build (warm cache), /tmp/loom-tauri-build/qa/Loom-QA.app (OLD dark e2e build, functional
  reference only), desktop/dist/Loom.app (OLD dark ordinary build; must be rebuilt).
- **Ego space 14** (exclusive QA lease from ROOT, owner agent):
  - **p1** = canonical UART REV6.1 `http://127.0.0.1:64785/uart-workbench/opus/index.html`. Keep it until ROOT's review.
  - **p2** = canonical UDS page (ETH/UDS measurement page), reusable for product comparisons.
  Do not create another space. Ego emulation resets per round, so **set the emulation, measure and screenshot in ONE call**.
  Use the `ego-browser skill` stdout guide only; never read a TMP copy outside the repo.

## Source changes by this role (uncommitted; `git diff -- desktop ui/src/panels/can ui/src/panels/video`)
| File | Change |
|---|---|
| desktop/frontend/launch.css | Bench Light launch/connecting/error page (tokens = canonical). Old dark copy: qa/evidence/launch.css.olddark.bak |
| desktop/src-tauri/src/main.rs | main window `.theme(Some(Theme::Light))` (per window) + `background_color` #fff; `.on_download` (REC/export); **UI identity guard** before navigate |
| desktop/src-tauri/src/nav.rs | `download_allowed()` (blob from daemon origin / data while connected / daemon origin) + unit test |
| desktop/src-tauri/src/ui_identity.rs (new) | `EXPECTED_INDEX` (from OUT_DIR), `verdict()`, Korean `mismatch_message()`, `unknown_message()`, tests |
| desktop/src-tauri/build.rs | copies staged `binaries/loomd.ui-index.html` → `OUT_DIR/ui-index.html` (empty if not staged) |
| desktop/host/src/http.rs | `get_public()` (no auth; bearer request bytes unchanged) |
| desktop/host/src/lib.rs | `pub fn daemon_ui_index(port, timeout)` → `/ui/index.html` bytes; 2 local-server unit tests |
| desktop/scripts/stage-loomd.sh | writes `binaries/loomd.ui-index.html` (= ui/dist/index.html) at stage; `--check` fails if it differs |
| desktop/scripts/smoke-native.sh | `reopen --plain`; e2e downloads → `$HOME_DIR/downloads` (`LOOM_DESKTOP_QA_DOWNLOAD_DIR`); `check-download`; `ui-mismatch`; cleanup covers lqm.* |
| desktop/scripts/e2e/wk.mjs | `geom` (no-nav geometry/tokens/bands/selector counts + shot), `goto <route>` |
| ui/src/panels/can/can.css | gap/loss rows → `var(--warn-bg/--err-bg, Primer fallback)`; new-rows text → `--fg` (pre-edit copy in qa/evidence/css-before/) |
| ui/src/panels/video/video.css | status/latch/error/frozen colours → tokens with fallbacks; black stage/overlay kept (intentional) |
Remote IPC is unchanged: commands are still only `launch_state` and `retry`, and the capability scope is untouched.

### UI identity guard (user requirement: never silently show an old UI from a reused daemon)
After `connect()` and the origin check, the app GETs `http://127.0.0.1:<port>/ui/index.html` (public, 5 s bounded) and compares
the bytes with the staged index embedded at build time. Vite's index names every content-hashed asset, so equal bytes mean the same build.
- Match → navigate. Mismatch or unreadable → launch page Failed view with a Korean, actionable message (pid, port,
  `LOOM_HOME='<home>' loom daemon stop`, then Retry) and the line "자동으로 중지하지 않습니다". The daemon is never stopped.
- Unstaged build: debug builds warn and continue; release builds fail with `unknown_message()`.
- Retry and Connection → Reconnect go through the same `connect_and_open`, so the guard re-runs.
- **Fix after ROOT review (11:3x):** the home in the stop command is now POSIX single-quoted (`sh_single_quote`: `'` → `'\''`).
  New test `stop_command_quotes_home_for_the_shell` runs the generated line through a real `/bin/sh`, with
  `printenv LOOM_HOME` in place of `loom daemon stop`, for the path `/Users/u/O'Brien Lab/$x \`y\` \z/.loom`, and asserts an exact round trip.

### Local tests run (desktop crates only; no production build)
- `cargo test -p loom-desktop-host --lib`: **3/3 PASS** (http parse, fetches index without credentials, non-200 error).
- `cargo test -p loom-desktop`: **14/14 PASS** (config 6, nav 6 incl. `downloads_only_from_daemon_page`, ui_identity 2).
- After the quoting fix: `CARGO_TARGET_DIR=/tmp/loom-tauri-build cargo test --locked -p loom-desktop ui_identity` under the
  `.cargo-desktop.lock` mkdir lock → **3/3 PASS** (verdicts, message, shell round trip). The lock was released and no
  desktop/target was created. No full build was run.
- `cargo clippy -p loom-desktop -p loom-desktop-host --all-targets --features loom-desktop/e2e`: clean. `cargo build
  --features e2e` compiles. rustfmt (2024) clean. `bash -n` on all scripts, `node --check wk.mjs`: OK.
- NOTE: these ran in desktop/target (now removed), not /tmp/loom-tauri-build; that cache will recompile once.
- **Unfinished guard tests (run with the final build):**
  - (1) host integration test against a real daemon (`host/tests/real_loomd.rs` not extended);
  - (2) native `smoke-native.sh ui-mismatch` (old-UI daemon via `loomd --ui-dir` → refusal + daemon kept), with a WK
    `launch-failed` screenshot of the Korean message;
  - (3) positive path: fresh home and reused current daemon → Match → navigate;
  - (4) the `on_download` handler has not been exercised natively (no UI REC/export in WK yet).
  The full host lifecycle tests (`host/tests/*.rs`) and the old ROOT 34-test combined run were not rerun after the `http.rs`
  refactor. Rerun them with the final build.

## Coordination state
- A (bench-uart-opus, shell + UART): still implementing; contracts in qa/PARITY-PLAN.md (tokens, Dockview, DOM hooks,
  entry paths).
- B (bench-diag-opus): **UDS_FROZEN**. Its handoff for the new UDS owner is `work/reports/tauri-desktop/bench-diag/UDS-HANDOFF.md`
  (relay to ROOT). The only UDS file touched is the new unused `ui/src/panels/uds/udsPrefs.ts`. The shared helper is
  `ui/src/workbench/eth/bench.ts/.css`. B continues ETH only (39/39 eth tests, tsc+eslint clean) and will send READY.
- B asked A (via this QA) to remove the shell.css `.workbench-main :is(.uart-head,…){background:#18212b}` override (in PARITY-PLAN).

## Exact final build + native parity recipe (after A, ETH and UDS are all READY; QA is the single build owner)
```sh
cd /Users/ctmctm/workspace/Loom
# 0. preconditions
swift desktop/scripts/lib/screen_state.swift          # locked=false displayAsleep=false
pgrep -fl "Loom-QA.app|uart_fixture|lqa\.|lqe\.|lqm\." || echo none
# 1. UI checks are the owners'; QA confirms once: pnpm --dir ui typecheck && pnpm --dir ui lint && pnpm --dir ui test
# 2. single production UI build + loomd re-embed + identity stage (cleans loom-ui-assets, verify-loomd 6/6+)
desktop/scripts/stage-loomd.sh --profile debug --force-ui   # writes binaries/loomd.ui-index.html
desktop/scripts/stage-loomd.sh --check
# 3. desktop tests in the shared target (take the lock if not via build-app.sh)
CARGO_TARGET_DIR=/tmp/loom-tauri-build cargo test --manifest-path desktop/Cargo.toml --workspace
# 4. ordinary app (the acceptance object) + e2e test copy
desktop/scripts/build-app.sh --release            # or --debug; then: (cd desktop && npm run dist) → desktop/dist/Loom.app
desktop/scripts/verify-dist.sh desktop/dist/Loom.app
desktop/scripts/build-app.sh --debug --features e2e --out /tmp/loom-tauri-build/qa/Loom-QA.app
QA_ALLOW_WEBDRIVER=1 desktop/scripts/verify-bundle.sh /tmp/loom-tauri-build/qa/Loom-QA.app
# 5. functional suite on the NEW UI (selectors may need updating from A's DOM handoff)
QA_ON_TOP=1 desktop/scripts/smoke-native.sh start --app /tmp/loom-tauri-build/qa/Loom-QA.app --wd 47941
desktop/scripts/e2e/run-visible-suite.sh
desktop/scripts/smoke-native.sh ui-mismatch       # guard negative path (WD 47943)
node desktop/scripts/e2e/wk.mjs --wd 47943 --out <run>/wk-uim launch-failed
desktop/scripts/smoke-native.sh cleanup && mv work/reports/tauri-desktop/qa/evidence/native-current work/reports/tauri-desktop/qa/evidence/native-run5-benchlight
```
6. **Parity (binding criteria: qa/PARITY-PLAN.md):**
   - Same daemon, 1440×900. Inner WK snapshot (`wk.mjs geom`, frame excluded) vs Ego space 14 (emulate + measure +
     shot in one call) vs canonical `parity/canonical-*.png`.
   - Entry paths: (a) fresh app open, no route → UART service workbench; (b) agent deep links
     `#/p/uart/{trace,terminal}?channel=dut` keep the workbench (rail/macros); (c) the explicit integrated layout.
   - Geometry 44/46/26, 13px, 6px splitter, tokens. Control order and labels vs `parity/canonical-controls.json`.
   - Deviation register in PARITY-PLAN (each needs a technical reason + ROOT approval).
   - Assert identical `/ui` asset digests in browser and WK.
7. **REC/export:** UART REC .log/.jsonl, then ETH/UDS exports. `smoke-native.sh check-download '<glob>' <marker>` in WK; the same
   files in Ego. The hygiene step scans the copies.
8. **ROOT CUA on a stable plain window** (`smoke-native.sh reopen --plain`): native Enter/Tab
   (`check-typed root-native-N [--tab]`) and Connection → Reconnect. Write app path/pid/home/port to
   NATIVE-REQUEST.md immediately after start; hold until NATIVE_CHECKS_DONE.
9. REPORT.html: canonical-vs-product and browser-vs-WK pairs, fingerprints (git diff hash, ui/dist digest, loomd/app
   sha), limitations. The current REPORT.html (run4) documents OLD dark-UI functional results only.

## Preserved evidence and functional proofs (not visual acceptance)
- evidence/native-run4-visible/: run4 results (live RX, CLI↔UI, quit/reopen, error/Retry, hygiene: PASS; WebDriver
  send-keys printable text: driver FAIL).
- ROOT CUA native Enter/Tab: `root-native-1`, `root-tab --tab` PASS.
- REPORT.html + report-assets/ (run4, old UI). parity/canonical-{uart,eth,uds}-1440x900.png, canonical-controls.json.
- PARITY-PLAN.md (contracts, measured metrics, acceptance criteria, deviation register, REC/download glue).
- STATUS.md / READY.json are superseded by this file for the next context.
