# Runtime worker — Windows + Ubuntu desktop runtime

State: **READY_FOR_REVIEW** (frozen 2026-11-15). Native foreground, no subagents, no Git index
or commit operations, no OS or user-daemon changes. Desktop cargo lock
`/tmp/loom-tauri-build/.cargo-desktop.lock` was taken with mkdir for the whole cargo phase and
**released** before return.

Owned files changed: `desktop/host/src/lib.rs`, `desktop/host/tests/lifecycle.rs`,
`desktop/host/tests/real_loomd.rs`, `desktop/src-tauri/src/{main,config,nav,ui_identity}.rs`,
`desktop/src-tauri/src/appimage.rs` (new), `desktop/src-tauri/Cargo.toml`, `desktop/Cargo.lock`
(+1 line: the `windows-sys 0.61.2` edge, a version already in the lock). Patch of all of them vs
HEAD (including earlier uncommitted Bench Light work in those files, which is preserved):
`runtime-logs/runtime-vs-HEAD.diff`. `ui_identity.rs` and `appimage.rs` are untracked, so they
appear as whole files. I did not touch backend crates, `desktop/scripts`, `tauri*.conf.json`,
icons, or `docker/`.

## What was wrong and what changed

| # | Problem (macOS-only assumption) | Fix |
|---|---|---|
| 1 | `config::resolve` used `LOOM_HOME` → `HOME` only. A Windows app started from Explorer/Start has no `HOME`, so it failed with "neither LOOM_HOME nor HOME is set". | `LOOM_HOME` → `HOME/.loom` → `USERPROFILE/.loom`, the same order as `loomd` `Home::from_env` and CLI `loom_home`. Empty values count as unset (backend difference noted below). |
| 2 | The bundled daemon was looked up as `loomd` next to the exe, but Windows installs `loomd.exe`. | `DAEMON_FILE_NAME = "loomd" + EXE_SUFFIX`. The sidecar layout is agreed with packaging in INTERFACE R1 (packaging confirmed it from tauri-build/tauri-utils source). |
| 3 | `nav::open_external` returned `Unsupported` off macOS, so external links did nothing. | Windows: `ShellExecuteW("open", url)` (windows-sys, no `cmd /c start`). Linux/BSD: `/usr/bin/xdg-open` (then `/usr/local/bin`, then `PATH`) with the URL as one argv element. macOS unchanged. New `is_external_link` gate: http(s) only, a host is required, and URLs carrying credentials are refused. |
| 4 | Windows spawn did not try to leave the app's job object. A launcher job with kill-on-close (some terminals/IDEs) would kill the daemon along with the app. | `spawn_detached`: `DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP \| CREATE_NO_WINDOW \| CREATE_BREAKAWAY_FROM_JOB`. On `ERROR_ACCESS_DENIED` (job forbids breakaway) it retries without breakaway, the same policy as `loom`'s `spawn_daemon`. |
| 5 | Linux AppImage: the detached daemon would run from the FUSE `$APPDIR` mount, which vanishes when the app exits (page-in faults; `loomd worker` re-exec via `current_exe` fails). The AppRun GTK/GIO/PATH env would also leak into the daemon and the browser. | New `appimage.rs`. `connect_with` prepare hook: only when connect is **about to start** a daemon (holding `start.lock`), copy to `<home>/desktop/bin/loomd-<fnv64>`. Reuse requires a regular file (no symlink), our uid, mode exactly 0755, byte-for-byte equal contents, and a home/desktop/bin chain owned by us and not group/other-writable. Otherwise the copy goes through a `create_new` temp file + atomic rename. Reusing an existing daemon never writes, so a read-only home works (ROOT review). `ConnectOptions::daemon_env` strips `$APPDIR` entries for the daemon; `xdg-open` launches get the same fixups. `LOOM_HOME` is always forced last. |
| 6 | The mismatch error suggested only a POSIX `LOOM_HOME='…' loom daemon stop`, and its round-trip test ran `/bin/sh` on every OS. | Windows suggests `$env:LOOM_HOME='…'; loom daemon stop` in PowerShell, doubling `'` and U+2018–U+201B. The `/bin/sh` test is `cfg(unix)`; a `cfg(windows)` powershell.exe round-trip was added (not run: no Windows host). The "unknown identity" message now points at `npm --prefix desktop run dist`. |
| 7 | Config tests used `/tmp/...` as "absolute", which is not absolute on Windows. | Test helper `abs()` adds a `C:` prefix on Windows; daemon-name assertions are suffix-aware. |
| — | Doc/comments said macOS / WKWebView / ⇧⌘R / ~/Downloads only. | Now platform-neutral. wry 0.57.2 source confirms that WebKitGTK and WebView2 default to the user's Downloads folder with non-overwriting names, so `on_download` policy is unchanged. |

Preserved and unchanged in behaviour: launcher-only IPC (`capabilities/launch.json`), exact
`http://localhost:<port>` origin pinning, navigation/new-window/download policy, the release-build
UI identity guard (`build.rs`, `ui_identity::verdict` untouched), "never stop the daemon",
token/URL redaction, the e2e feature compile guard, and macOS `/usr/bin/open`. The launch page URL
was already right per platform (`http://tauri.localhost` on Windows, `tauri://localhost`
elsewhere).

## Evidence (each log ends with the real `rc=` of the cargo process; no pipeline statuses)

All logs are in `runtime-logs/`. macOS = this host (aarch64-apple-darwin, Darwin 27).

| Log | Command | rc | Result |
|---|---|---|---|
| `mac-host-test.log` | `cargo test --locked -p loom-desktop-host` (TARGET `/tmp/loom-tauri-build`) | 0 | unit 4 passed, lifecycle **23 passed** (new: `daemon_env_overrides_apply_but_never_replace_loom_home`, `prepare_hook_runs_only_when_starting_and_its_binary_is_spawned`), real_loomd 1 passed (gated; no real daemon run) |
| `mac-desktop-test.log` | `cargo test --locked -p loom-desktop` | 0 | **26 passed** (config 8, nav 8, ui_identity 5, appimage 5). Run after the final edit. |
| `mac-clippy.log` | `cargo clippy --locked -p loom-desktop-host -p loom-desktop --all-targets -- -D warnings` | 0 | clean. Run after the final edit; the earlier `type_complexity` on my boxed closure is fixed with a `PrepareBox` alias. |
| `mac-release-check.log` | `cargo check --locked --release -p loom-desktop` | 0 | release cfg compiles (`windows_subsystem`, no e2e). Ran before the final no-behaviour edit (type alias + closure annotation); that edit was compiled by the clippy runs. |
| `win-gnu-check.log` | `cargo check --locked --target x86_64-pc-windows-gnu -p loom-desktop-host -p loom-desktop --all-targets` (TARGET `/tmp/loom-tauri-build-wingnu`, linker `/opt/homebrew/bin/x86_64-w64-mingw32-gcc`) | 0 | Windows cfg compiles. Ran before the final no-behaviour edit. |
| `win-gnu-clippy.log` | same as above, with `clippy … -- -D warnings` | 0 | clean (ShellExecuteW, breakaway spawn, PowerShell quoting). Run after the final edit. |
| `win-gnu-test-build.log` | `cargo test --locked --target x86_64-pc-windows-gnu … --no-run` | 0 | 4 Windows test executables linked with mingw. **Not executed.** |

Superseded earlier runs (piped through `tail`, so their exit status is unproven; informal smoke
only): the first macOS host/desktop tests, the first `win-gnu-check` with `--offline` (failed:
`jiff-tzdb` not cached, then fetched normally), and the second, which failed only because
`binaries/loomd-x86_64-pc-windows-gnu.exe` was missing for `tauri-build`.

Windows sidecar used for compile only (ROOT-authorised): I copied CI's cross-built
`/tmp/loom-cross-windows/x86_64-pc-windows-gnu/release/loomd.exe` (PE32+ x86-64, sha256
`e10fc8c2…dea17b`) to `desktop/src-tauri/binaries/loomd-x86_64-pc-windows-gnu.exe`, ran the checks,
then **deleted it**. It was never executed. `loomd.ui-index.html` hashes the same before and after.
`loomd-aarch64-apple-darwin` and `loomd.stamp.json` changed while I worked, but my only
`binaries/` operations were adding and removing the `.exe`, so this is most likely a re-stage by
packaging (`stage` writes these files). Packaging/ROOT should confirm.

## Not verified (honest gaps)

- **No Windows execution**: no native MSVC build, no run of the Windows test binaries, no WebView2
  window, ShellExecuteW, breakaway spawn, or PowerShell round-trip at run time. The GNU
  check/clippy/link only proves the Windows cfg code type-checks and links. The installer target is
  MSVC on CI.
- **No Linux compile by me**: the `cfg(target_os = "linux")` lines (AppImage wiring in `main.rs`,
  `xdg-open` in `nav.rs`) are not compiled on macOS or Windows. Per ROOT, the CI worker owns the
  Docker Linux harness, so I did not start one. Request (INTERFACE R6): CI runs `cargo clippy
  --locked -p loom-desktop-host -p loom-desktop --all-targets -- -D warnings` and `cargo test
  --locked -p loom-desktop-host -p loom-desktop` in `loom-ci-ubuntu-pkg`. The `appimage` module's
  logic is unit-tested on macOS (`cfg(any(linux, all(unix, test)))`).
- No real AppImage run (FUSE mount lifetime, daemon surviving app exit), no deb install, no
  `xdg-open` browser launch.
- No visible macOS UI acceptance (screen locked; no polling). macOS behaviour changes are limited
  to the new prepare hook, `daemon_env` (empty on macOS) and doc text. Lifecycle tests cover the
  hook's reuse and start paths.

## Backend observations (read only, no backend change made or needed for the desktop)

1. `loomd` `Home::from_env`, CLI `loom_home` and worker-sdk `loom_home` use a set-but-empty
   `LOOM_HOME`/`HOME` as is (a relative home), while the desktop treats empty as unset. Suggestion:
   filter empties in the backend too. Low priority, and a desktop-started daemon always gets an
   explicit absolute `LOOM_HOME`.
2. CLI Windows `detach` retries without breakaway after *any* spawn error. The desktop retries only
   on ERROR_ACCESS_DENIED. Harmless difference.
3. Windows native backend blockers (Unix sockets for worker lanes, video) are in the CI worker's
   audit, not mine.

## Next (for ROOT)

- Get Linux clippy/test results from CI (R6 request), then the native Windows MSVC CI run of
  `cargo test --locked -p loom-desktop-host -p loom-desktop` plus the NSIS bundle.
- Any real AppImage smoke should check: start the app → quit → `loomd` pid still alive, running
  from `~/.loom/desktop/bin/loomd-*`.
- A fresh macOS build is needed before any new visible acceptance. The prior final2 fingerprints do
  not cover this change.
