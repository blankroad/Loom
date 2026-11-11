# TC-1 browser interop: opt-in fixture begin/release handshake

- Feature / status / role: test-harness prep for the real TC-1 browser gate / **READY_TO_INTEGRATE** / Herdr Claude Opus 5.5 foreground, done directly (no sub-agents). Self-review only.
- **This is not a browser PASS.** No browser was run. The handshake was exercised with a file-only driver (no browser). The browser run (Playwright/Ego) stays with main.
- Base: main `3bac3f1` + the accepted `tc1-live-p1` test + other owners' uncommitted WIP, kept as-is. No index/commit/push. No changes to other sources, manifests or the lock file.

## Changed files (owned)

| File | Change |
|---|---|
| `crates/loom-gateway/tests/tc1_live_smoke.rs` | Opt-in `Browser` handshake (`LOOM_TC1_BROWSER_DIR`), `D::inproc_ui(ui_dir)` (`D::inproc()` = `inproc_ui(None)`), two hook points in the test |
| `docs/impl/main/reports/tc1-browser-interop-p1.md` | This report |

## Protocol (driver = main's Node process)

The driver creates a fresh private directory (`mkdtemp`, 0700, empty) and starts the test with `LOOM_TC1_BROWSER_DIR=<dir>` (optionally `LOOM_TC1_UI_DIR=<abs>/ui/dist`).

| # | Test (Rust) | Driver |
|---|---|---|
| 0 | Checks the dir: absolute, a real directory (not a link), mode `& 0o077 == 0`, **empty**. Otherwise it fails immediately. With `LOOM_TC1_UI_DIR`: requires `index.html` and sets `cfg.ui_dir` (fresh UI is served at `/ui/`) | — |
| 1 | Bench setup + the unchanged power-OFF precondition, then **before `test.start`** writes `ready.json` (0600, written to a temp file and renamed, so it is never partial): `{"origin":"http://localhost:<port>","url":"<origin>/ui/#boot=<code>&next=%2Fp%2Fshell%2Ftest","route":"/p/shell/test"}`. `session_id` is omitted (the session does not exist yet). `<code>` = `auth.issue_code(Operate)`, one-time, 60 s TTL. The URL has the same form as `server.rs::bootstrap_new` (`canonical_origin`, `#boot=…&next=…`). `next` is URL-encoded like `ui/e2e/real-smoke-server.ts`. The UI's `readBootFragment` decodes it to `/p/shell/test` | Reads `ready.json`, **deletes it**, opens the URL, waits until the web client has subscribed |
| 2 | Waits **≤ 60 s** for `begin`, otherwise fails (`browser handshake: no \`begin\` within 60s`). Then the unchanged TC-1 run (`test.start` … `test.end`) | Creates `begin` |
| 3 | After `test.end` = `passed` and `test.session_ended`: writes `result.json` (0600) `{"result":"passed","session_id":"ts-…"}`, with no token, code or URL. Waits **≤ 60 s** for `release`, otherwise fails | Takes the screenshot, then creates `release` |
| 4 | The unchanged post-session power-cycle checks and the existing cleanup (`fx.stop`, drops, `d.shutdown`). The process exits | Checks its child's exit status |

- Waiting: millisecond file polling on the test thread. Each turn reads the open WebSocket clients for 25 ms each. No network, no model calls, no extra threads. **Why the clients are read**: the daemon pings every 20 s and closes a connection after 40 s of silence (`conn.rs` `PING`/`IDLE`). Without reading, the idle agent socket would be closed during a 60 s wait. Reading lets tungstenite send the pongs.
- No secret is printed: the test's stdout/stderr only shows `[browser] ready.json written; waiting ≤ 60s for begin`, `begin seen`, `result.json written…`, `release seen`. Panic messages contain no secrets either.
- Not changed: the TC plan, steps, assertions and judgment, finally/STOP (none), cleanup, and the Unix-only cfg.
- **Default (no env)**: `Browser::from_env()` = `None` → `D::inproc_ui(None)` leaves `cfg.ui_dir` untouched and both hooks are skipped, so the code path is the same as before. Per the task, the default run was not repeated. In browser mode, the same TC-1 path passed (below).
- `LOOM_TC1_UI_DIR` only takes effect together with `LOOM_TC1_BROWSER_DIR` (decision: fresh UI is only needed by the browser; the default stays strictly unchanged).

## Verification (no browser)

Build/static checks (shared cache, `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target`, `--locked --offline`, no new target/tree copy):
`cargo clippy -p loom-gateway --test tc1_live_smoke -- -D warnings` PASS, `cargo test … --test tc1_live_smoke --no-run` PASS, `rustfmt --edition 2024 --check` (this file) PASS.

File-only driver `drive.sh` (scratchpad, not in the repo) runs the built test binary with `LOOM_TC1_BROWSER_DIR=<mktemp -d>` and `LOOM_TC1_UI_DIR=<repo>/ui/dist`:

| Case | Result |
|---|---|
| begin → release | dir 0700. `ready.json` at +2 s, mode **600**, keys `origin/route/url`, no `session_id`. URL form `http://localhost:<port>/ui/#boot=…&next=…`, and `next` decodes to `/p/shell/test`. The code and URL do **not** appear in the test output. `GET /ui/` (Host `localhost:<port>`) is byte-identical to `ui/dist/index.html`. `POST /api/v1/bootstrap {code}` → **204** + `loom_session` cookie (live one-time operate code; this consumed it, browser stand-in). No `test.start` before `begin`. `result.json` at +11 s, mode 600: `{"result":"passed","session_id":"ts-18e707bba2b798e0-89122-1"}`. The test was still held 3 s later, and the post-session part had not run. After `release`: **exit 0, `1 passed`** (18.3 s; the waits make up the difference from about 11.7 s) |
| no begin | `ready.json` written, then **exit 101 after 61 s**: `browser handshake: no \`begin\` within 60s`. No leftover process |
| unsafe dir | Mode 755 → fails immediately (`must be private (0700)`). Pre-existing `begin` → fails immediately (`must be a freshly created, empty directory`) |

The temporary directories were deleted.

## Notes for main (browser run)

- The code is one-time with a 60 s TTL. The driver should open the URL soon after `ready.json` appears (the same 60 s window as `begin`). Do not redeem it any other way, as the drive test above did.
- Create `begin` only after the web client is connected and `core.subscribe` has succeeded on `/p/shell/test`. Then the session start is seen live (`test.session_started` → steps → `test.session_ended passed`).
- `result.json` = gate for "TC ended passed". Take the screenshot and create `release` within 60 s. Then check that the test process exits with 0.
- The test also asserts `passed` itself. If any step fails, the test panics before `result.json`, and the driver's own bounded wait must handle that (child exit ≠ 0).
- Only `result.json` and screenshots may be shared. `ready.json` is deleted by the driver after use (the test does not delete it).

## Remaining

- The browser-side LIVE/passed screenshot (main, separate track; the app auto-approval block is still unresolved and is not bypassed here).
- Role released after this return; session/pane kept.
