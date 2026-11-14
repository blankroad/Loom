# Loom desktop (macOS, Tauri 2)

A native macOS window around the **real Loom UI served by `loomd`**. The window shows exactly what a browser
shows at `http://localhost:<port>/ui/`, connected to **the same daemon** the `loom` CLI and agents use.

- On launch, the app reuses a daemon already running for the selected `LOOM_HOME`. If none is running, it starts
  the bundled `loomd` (or `LOOM_DESKTOP_DAEMON_BIN`), detached, with logs going to `<home>/run/loomd.log`.
- The app gets a fresh viewer bootstrap from the daemon using the local discovery token. It never asks for, or
  reads, a human login code. The viewer URL and token are never logged or shown.
- Closing the window or quitting the app (Cmd-Q) **does not stop the daemon**. Open channels, devices, tasks and
  recordings keep running. Stop the daemon explicitly with `loom daemon stop`.
- The browser UI (`loom ui`) remains fully supported and can run alongside the app.

Phase-1 scope: launch, error/Retry, connect/start lifecycle, local packaging. Not included: updates, tray,
signing/notarization, the debugger.

## Layout

| Path | Owner | What |
|---|---|---|
| `src-tauri/` | app | Tauri shell (`loom-desktop`), navigation policy, launch page wiring, `tauri.conf.json` |
| `frontend/` | app | local launch / error / Retry page (the only page with IPC) |
| `host/` | host | `loom-desktop-host`: pure-Rust daemon discovery/start/bootstrap (no Tauri) |
| `scripts/` | QA | preflight, staging, bundle build/verify, native smoke |
| `src-tauri/binaries/loomd-<triple>` | generated | staged sidecar (do not commit), plus `loomd.stamp.json` provenance |

## Build

```sh
desktop/scripts/preflight.sh                       # read-only feasibility check (installs nothing)
desktop/scripts/stage-loomd.sh --profile debug     # or --profile release, or --from <prebuilt loomd>
desktop/scripts/build-app.sh --debug               # or --release
# → /tmp/loom-tauri-build/<debug|release>/bundle/macos/Loom.app
```

- **`stage-loomd.sh`** rebuilds `ui/dist` only when UI sources are newer. It then builds `loomd` in the main
  workspace with `--locked`, so the root `Cargo.lock` is never changed. The binary is copied to
  `src-tauri/binaries/loomd-<host-triple>` (Tauri `bundle.externalBin`), which becomes
  `Loom.app/Contents/MacOS/loomd`. One binary is enough: workers re-execute it as `loomd worker <kind>`.
- **The UI is compiled into `loomd`** (`loom-ui-assets` uses rust-embed on `ui/dist`). rust-embed does **not**
  notice new hashed asset files, so a cached build can silently ship an old UI. This happened with
  `target/debug/loomd` on 2026-11-15. To prevent it, `stage-loomd.sh` always cleans the `loom-ui-assets` crate,
  which is cheap: only that crate and the gateway recompile. It then runs **`verify-loomd.sh`**, which starts the
  binary in a throwaway `LOOM_HOME` and compares every `/ui/*` response byte-for-byte with `ui/dist`. A failed
  comparison fails the stage. `--from` stages a prebuilt binary and records its provenance (path, mtime, sha256).
  It is still verified, never trusted on mtime alone.
- **`build-app.sh`** refuses to bundle unless `stage-loomd.sh --check` reports a current, verified binary. It uses
  the pinned `@tauri-apps/cli` (task-local, never global) with `CARGO_TARGET_DIR=/tmp/loom-tauri-build`, under the
  lock `/tmp/loom-tauri-build/.cargo-desktop.lock`. It finishes by running `verify-bundle.sh`.
- **`verify-bundle.sh <Loom.app>`** checks: bundle id, the main executable, and the bundled `loomd` identity. A
  raw-hash match with the stamp is accepted, and so is a match after signing: Tauri re-signs nested code, so the
  check reproduces the deterministic ad-hoc re-sign of the staged binary and compares the bytes. `verify-dist.sh`
  adds an LC_UUID + `__text`/`__const` check. It also checks that the embedded UI is identical to `ui/dist`, that no `daemon.json`, logs, journals, policy files
  or boot fragments were bundled, and the codesign state. The codesign state is reported as found, not faked.

Phase-1 signing scope is ad-hoc only (`signingIdentity "-"`): the bundle passes `codesign --verify --deep --strict`
and launches locally. Developer ID signing and notarization are excluded, so redistribution to other Macs
(Gatekeeper acceptance) is unverified.

## Run

The stable local deliverable is `desktop/dist/Loom.app` (`cd desktop && npm ci && npm run dist`, checked by
`desktop/scripts/verify-dist.sh`).

```sh
open desktop/dist/Loom.app                                                      # ~/.loom (your normal daemon)
open -n -a desktop/dist/Loom.app --env LOOM_HOME=/path/to/isolated-home         # isolated home
```

**Connection → Reconnect** (Cmd-Shift-R) returns the window to the launch page and connects again. It reuses or
starts the daemon and mints a fresh viewer bootstrap. Use it after the daemon was restarted (for example
`loom daemon stop` then `start`): the old viewer cookie dies with the daemon, and the port may change.

| Variable | Meaning |
|---|---|
| `LOOM_HOME` | Daemon home (default `~/.loom`). Same rule as `loom` and `loomd`. |
| `LOOM_DESKTOP_DAEMON_BIN` | Exact absolute `loomd` to start when none is running (no `PATH` search, no fallback). |
| `LOOM_DESKTOP_UI_DIR` | Passed as `loomd --ui-dir` **only when the app starts the daemon** (UI development). |
| `LOOM_DESKTOP_STARTUP_TIMEOUT_MS` | Startup wait, clamped to 1–120 s (default 20 s). |

If startup fails, for example because the binary is missing or the daemon crashed, the window shows a redacted,
actionable message and a **Retry** button. See `<home>/run/loomd.log`.

### External engines and the Finder `PATH`

Apps launched from Finder or the Dock get the minimal launchd `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), **not**
your shell `PATH`. A daemon **started by the app** therefore will not find Homebrew tools such as `ffmpeg`
(video) or `sigrok-cli` (logic) in `/opt/homebrew/bin`. These engines are **not bundled**.

Options:
- Start the daemon from a shell first (`loom daemon start`). The app then reuses it, along with its `PATH`.
- Set an absolute engine path in `$LOOM_HOME/loom.toml`. Video supports `[video] ffmpeg = "/opt/homebrew/bin/ffmpeg"`,
  and logic supports
  `[logic] sigrok_cli = "/opt/homebrew/bin/sigrok-cli"`. Both must be absolute paths.
- Launch with `open -a Loom.app --env PATH="/opt/homebrew/bin:$PATH"`.

UART, CAN, UDS and Ethernet need no external engines.

## Validation (QA)

```sh
QA_HOME_ROOT=$TMPDIR desktop/scripts/smoke-native.sh start   # fresh isolated home + pty UART fixture + app
desktop/scripts/smoke-native.sh check-typed <text> [--tab]   # after typing in the native terminal
desktop/scripts/smoke-native.sh after-close                  # after Cmd-Q: daemon/channel/device still alive
desktop/scripts/smoke-native.sh reopen                       # same daemon reused
desktop/scripts/smoke-native.sh error-start                  # 2nd app, absent explicit daemon bin → error view
desktop/scripts/smoke-native.sh error-fix                    # put the binary in place, then click Retry
desktop/scripts/smoke-native.sh error-verify                 # Retry started exactly that daemon, viewer attached
desktop/scripts/smoke-native.sh hygiene                      # no token/boot fragment in logs/evidence/args
desktop/scripts/smoke-native.sh cleanup                      # stops ONLY the QA homes' daemons/apps/fixture
```

`lib/uart_fixture.py` is an external pty "device". Loom opens it like any serial port, and the fixture records
every byte it receives. That gives byte-level proof of keystrokes typed into the native WKWebView terminal and
of CLI sends. Native UI input (typing, clicks, Cmd-Q) comes from a human or a native automation tool, between
script steps. The scripts identify their own processes by `LOOM_HOME` and never signal anything else: not
other daemons, not other app instances.

### Native WKWebView automation (test builds only)

`cargo` feature `e2e` (debug builds only; enabling it in release is a `compile_error!`) links
`tauri-plugin-wdio-webdriver`, the embedded provider of `@wdio/tauri-service`. The WebDriver server runs **only**
when `LOOM_DESKTOP_QA_WEBDRIVER_PORT` is set, and binds 127.0.0.1 only. `LOOM_DESKTOP_QA_ON_TOP=1` keeps the
window on top, so macOS occlusion does not pause WebKit.

```sh
desktop/scripts/build-app.sh --debug --features e2e --out /tmp/loom-tauri-build/qa/Loom-QA.app   # test build
QA_ON_TOP=1 desktop/scripts/smoke-native.sh start --app /tmp/loom-tauri-build/qa/Loom-QA.app --wd 47941
desktop/scripts/e2e/run-visible-suite.sh            # RX, keys/Tab, CLI→UI, quit/reopen, error/Retry, hygiene
node desktop/scripts/e2e/wk.mjs --wd 47941 --out /tmp/wk ready
node desktop/scripts/e2e/wk.mjs --wd 47941 --out /tmp/wk rx ack:qa-cli-1
node desktop/scripts/e2e/wk.mjs --wd 47941 --out /tmp/wk type qa-tab --tab
```

- `wk.mjs` has no npm dependencies. It never reads the page URL, title, source or cookies.
- Key input is WebDriver "element send keys", which the plugin turns into DOM events inside the real WKWebView. It is
  not OS-level keystrokes.
- Screenshots are WKWebView snapshots.
- The page must actually be visible: unlocked session, display awake, window not covered. Otherwise WebKit correctly
  reports `document.hidden` and stops `requestAnimationFrame`, and the trace and terminal panels do not render.
  `lib/screen_state.swift` checks lock/sleep; `run-visible-suite.sh` refuses to run while locked (exit 4).
- **Never distribute an e2e build.** `verify-bundle.sh` fails on the plugin unless `QA_ALLOW_WEBDRIVER=1`.

### Distribution check

`desktop/scripts/verify-dist.sh [desktop/dist/Loom.app]` runs a strict check: `codesign --verify --deep --strict`
must pass, and no WebDriver plugin may be present. Signing rewrites `loomd`'s bytes, so identity across signing is
proven differently: LC_UUID plus sha256 of the `__TEXT,__text` and `__TEXT,__const` sections must match the staged
pre-sign binary. The bundled `loomd` must also pass `verify-loomd`. The script writes
`work/reports/tauri-desktop/qa/evidence/dist-manifest.json` with the pre-sign and final hashes.
