# Cross-platform desktop — shared interface notes

Each owner edits only its own section. Disagreements: add a line under "Open questions" naming
the section owner; ROOT arbitrates.

## Runtime (owner: runtime worker — `desktop/host/**`, `desktop/src-tauri/src/**`, src-tauri Cargo.toml, desktop/Cargo.lock)

Status: READY_FOR_REVIEW (2026-11-15), frozen. Final evidence: `RUNTIME.md`.

### R1. Sidecar location the app resolves at run time (packaging must produce this)

Build input stays the Tauri v2 `externalBin` convention (unchanged config key `"binaries/loomd"`):

| Platform | staged build input (packaging) | installed path the app looks for |
|---|---|---|
| macOS | `desktop/src-tauri/binaries/loomd-aarch64-apple-darwin` (or `x86_64-apple-darwin`) | `Loom.app/Contents/MacOS/loomd` |
| Windows | `desktop/src-tauri/binaries/loomd-x86_64-pc-windows-msvc.exe` | `<install dir>\loomd.exe` (same dir as the app `.exe`) |
| Ubuntu deb | `desktop/src-tauri/binaries/loomd-x86_64-unknown-linux-gnu` (or `aarch64-…`) | `/usr/bin/loomd` (next to `/usr/bin/loom-desktop`) |
| AppImage | same as deb | `$APPDIR/usr/bin/loomd` — see R3 |

Rule implemented in `src-tauri/src/config.rs`: `LOOM_DESKTOP_DAEMON_BIN` (absolute) else
`current_exe().parent() / ("loomd" + std::env::consts::EXE_SUFFIX)`. No `PATH` search, no triple
suffix at run time (Tauri strips it when bundling and when `tauri-build` copies sidecars into the
cargo target dir for `cargo run`/`tauri dev`).

### R2. UI identity guard (unchanged, keep it)

`src-tauri/build.rs` embeds `desktop/src-tauri/binaries/loomd.ui-index.html` (fixed,
triple-independent name). Release builds without it refuse to navigate (`Verdict::Unknown` →
error); debug builds warn. Packaging's portable stager must keep writing exactly this file (the
`/ui/` bytes served by the staged daemon) plus `loomd.stamp.json`, for every target. One staged
triple at a time is fine: the identity file is per build, not per triple.

### R3. Linux AppImage: daemon must not run from the FUSE mount

The daemon is detached and outlives the app; an AppImage's `$APPDIR` mount disappears when the app
exits (the daemon would then SIGBUS / fail to re-exec its `loomd worker` children). When
`APPIMAGE` + `APPDIR` are set and the daemon path is inside `$APPDIR`, the runtime copies it
to `<LOOM_HOME>/desktop/bin/loomd-<fnv64 of bytes>` and spawns that copy. The copy is lazy
(`loom_desktop_host::connect_with` prepare hook): it runs only when this connect starts a daemon
(under `run/start.lock`), never when an existing daemon is reused, so a read-only home with a
running daemon still works. A cached copy is reused only when it is a regular file (no symlink),
owned by the user, mode exactly 0755, byte-for-byte equal to the bundled daemon, under
home/`desktop`/`desktop/bin` dirs owned by the user and not group/other-writable. Otherwise it is
replaced through a `create_new` temp file + atomic rename (the name hash is never trusted alone). It also strips `$APPDIR`-pointing env entries (GTK/GIO/PATH
entries set by AppRun) from the daemon and from external-browser launches. Nothing for packaging
to do; deb is unaffected (stable `/usr/bin/loomd`).

### R4. Home resolution (matches `loomd` `Home::from_env` and `loom` CLI `loom_home`)

`LOOM_HOME`, else `HOME/.loom`, else `USERPROFILE/.loom` (Windows GUI launches normally have no
`HOME`). Desktop treats empty values as unset (backend does not — reported, not changed).

### R5. External links (new on Windows/Linux)

Only `http(s)` new-window requests: macOS `/usr/bin/open <url>`; Windows `ShellExecuteW("open",
url)` (no `cmd /c start`); Linux `xdg-open <url>` (argv, no shell). Requires a Windows-only
`windows-sys` dependency in `src-tauri/Cargo.toml` (already in the lock graph via Tauri).

### R6. Commands CI / packaging can run for runtime

- `cargo test --locked -p loom-desktop-host` (lifecycle tests are `cfg(unix)`; Windows runs the
  portable unit tests only).
- `cargo test --locked -p loom-desktop` (config/nav/ui_identity unit tests; needs a staged or
  empty `binaries/loomd.ui-index.html`, missing file → empty identity).
- Windows GNU cross check locally: `cargo check --locked --target x86_64-pc-windows-gnu -p
  loom-desktop-host -p loom-desktop` (runtime worker runs it under the desktop cargo lock). It
  needs `binaries/loomd-x86_64-pc-windows-gnu.exe` because `tauri-build` checks `externalBin`
  for the target triple. For compile only, the runtime worker staged CI's cross-built loomd.exe
  temporarily and removed it afterwards.
- **Request to CI (Linux, in your existing Docker harness, no new harness from runtime):** run
  `cargo clippy --locked -p loom-desktop-host -p loom-desktop --all-targets -- -D warnings` and
  `cargo test --locked -p loom-desktop-host -p loom-desktop`. These are the only builds that
  compile the `cfg(target_os = "linux")` paths: the AppImage wiring in `main.rs` and `xdg-open` in
  `nav.rs`. macOS and Windows checks cannot cover them.

## Packaging (owner: packaging worker)

Status: implemented and FROZEN (2026-11-15, deb-only Linux), READY_FOR_REVIEW; evidence in `PACKAGING.md`. CI: ready for the cached rerun plus the real .deb install/launch checks.
Ack R1/R2/R3: the staged names and installed placement match (confirmed from source: tauri-build 2.7.4
`copy_binaries` strips `-<triple>`; tauri-utils 2.10.2 `external_binaries` appends `.exe` for Windows).

### P1. Portable entrypoint (Node >= 24 enforced, builtins only, no shell, macOS / Ubuntu / Windows)

    node desktop/scripts/package.mjs <command> [options]      # npm --prefix desktop run <script>

| command | does | exit |
|---|---|---|
| `stage [--profile release\|debug] [--from <loomd>] [--reuse-ui]` | fresh `pnpm build` of `ui/` (`pnpm install --frozen-lockfile` first if `ui/node_modules` is missing), `cargo clean -p loom-ui-assets`, `cargo build --locked -p loom-gateway --bin loomd` in the MAIN workspace (its own target dir, `CARGO_TARGET_DIR` removed from the child env), copy to `binaries/loomd-<host triple>[.exe]`, write `loomd.ui-index.html` + `loomd.stamp.json`, then `verify` | 0 / 1 |
| `check` | staged sidecar is current and verified (same rules as `stage-loomd.sh --check`) | 0 current, 3 stale |
| `verify [<loomd>]` | runs `<loomd> run --port 0` under a private temp `LOOM_HOME`, byte-compares every `ui/dist` file at `/ui/<path>`, checks the CSP header, stops only the pid it spawned; sets `embedded_ui_verified` in the stamp | 0 / 1 |
| `bundle [--debug] [--bundles a,b] [--out-dir D] [--strict\|--no-strict]` | requires `check`; `tauri build --bundles <defaults> --ci -- --locked` under the desktop cargo lock; checks the sidecar inside each produced bundle; copies installables to the out dir; writes the manifest | 0 / 1 |
| `dist [--debug] [--out-dir D]` | `stage` (release unless `--debug`) + `bundle` | 0 / 1 |

Strict (default when `CI` is `true`/`1`, or `--strict`): any WARN row (bundle contents not inspected) ⇒ manifest `verified:false` and exit 1. Non-strict local runs print a WARNING and keep `verified:false`. On SIGINT/SIGTERM/SIGHUP the build child (its own POSIX process group; `taskkill /T` on Windows) is stopped and the desktop lock is released only once that tree is confirmed gone. Otherwise the lock is kept and a message names the pids. `verify` fails if `daemon.json` names a pid other than the one it spawned.

### P2. Default bundles and output paths (also set in `tauri.linux.conf.json` / `tauri.windows.conf.json`)

- macOS `app` → `<out>/Loom.app` (same path as legacy `npm run dist`; `verify-dist.sh` still applies)
- Linux `deb` → `<out>/<tauri name>.deb` (`Loom_<ver>_<arch>.deb`). **Only** deb is supported on Ubuntu (ROOT decision, 2026-11-15). AppImage is experimental and not accepted by `--bundles`: the first real Ubuntu 22.04 aarch64 run failed the exact sidecar check because linuxdeploy rewrites the ELF (strip/RUNPATH), so `usr/bin/loomd` became `bfce1276…` vs staged `836863dd…`. Evidence: `evidence/appimage-strict-fail-05-dist.log`. The deb passed with the exact hash in the same run.
- Windows `nsis` → `<out>/<tauri name>-setup.exe`
- `<out>` defaults to `desktop/dist`. **Manifest `<out>/artifacts.json`**: `{schema:1, generated_at, platform, target, profile, version, tauri_cli, loomd_sha256, loomd_profile, ui_dist_digest, baseline, verified, strict, artifacts:[{kind, path, abs_path, sha256, sha256_kind:'file'|'tree', bytes, built_from}], checks:[{check, result:PASS|WARN|FAIL|INFO, detail}]}`. `path` is relative to the repo with `/` separators when the file is inside the repo, otherwise absolute. `tree` (the .app directory) = sha256 over sorted `<sha256>  <rel>\n` lines.

### P3. Environment and prerequisites

- Desktop cargo target = `LOOM_DESKTOP_TARGET_DIR` > `CARGO_TARGET_DIR` > default (any path outside `desktop/`; default `/tmp/loom-tauri-build` on macOS/Linux, `%TEMP%\loom-tauri-build` on Windows); lock `<that>/.cargo-desktop.lock` (mkdir, shared with `build-app.sh`). Never `desktop/target`.
- Tauri CLI = the pinned devDependency in `desktop/node_modules/@tauri-apps/cli`, run as `node …/tauri.js` (no `.cmd` shim). CI runs `npm ci --prefix desktop`.
- CI provides: rustc >= 1.93, Node >= 24 (ui `engines`), pnpm 11.11.0 (ui `packageManager`), and the Linux Tauri prerequisites (`libwebkit2gtk-4.1-dev`, …). No `NO_STRIP` handling: it is not needed for deb.
- Accepted `--bundles` = exactly the defaults (macOS `app`, Linux `deb`, Windows `nsis`); dmg/rpm/msi/appimage are rejected.
- Bundle checks that need tools: `dpkg-deb` (Ubuntu deb) and `7z` for NSIS contents (preinstalled on GitHub Windows runners). If a tool is missing the check reports WARN rather than a fake PASS.

### P5. Exact commands for CI (reply to CI.md proposal; these replace its placeholders)

Run from the repo root on every OS (bash or pwsh — no shell syntax inside the scripts):

    npm ci --prefix desktop                                   # pinned @tauri-apps/cli 2.12.4
    pnpm --dir ui install --frozen-lockfile                   # optional: stage installs if ui/node_modules is missing
    npm --prefix desktop test                                 # node --test portable packaging tests (fast, no cargo)
    npm --prefix desktop run dist                             # = node desktop/scripts/package.mjs dist
    #   stage --profile release (fresh UI + loomd + verify) → tauri build of the platform defaults →
    #   bundle checks → desktop/dist/<installables> + desktop/dist/artifacts.json
    node desktop/scripts/package.mjs check                    # exit 0 current, 3 stale

npm scripts (all portable, all `node scripts/package.mjs …`): `test`, `stage`, `stage:debug`, `check`, `verify`, `build:app` (= `bundle`, release, platform defaults, requires a current verified stage), `build:app:debug`, `dist`, `dist:debug`, `tauri`.
- Target dir precedence for the **desktop** build: `LOOM_DESKTOP_TARGET_DIR` > `CARGO_TARGET_DIR` > the default in P3. So CI's `CARGO_TARGET_DIR=${{ runner.temp }}/loom-tauri-build` is honoured. The **main-workspace** loomd build always runs with `CARGO_TARGET_DIR` removed, so it uses `<repo>/target`, the same as `stage-loomd.sh`. `LOOM_STAGE_TARGET_DIR` (must differ from the desktop target) overrides it, e.g. for containers.
- Manifest name/shape: `desktop/dist/artifacts.json` (not `manifest.json`). Each artifact has `path` (relative to the repo, `/` separators) **and** `abs_path`. CI should upload `artifacts[].abs_path` plus the manifest file itself. The manifest is written only after every bundle check passed.
- `--out-dir <dir>` moves the installables and the manifest out of `desktop/dist`.
- The old POSIX-only npm scripts are gone. `build:app`, `build:app:debug` and `dist` are now the portable commands; on macOS, `dist` still produces `desktop/dist/Loom.app`. The explicit mac shell scripts (`desktop/scripts/*.sh`) are unchanged and interoperate (same stamp, same digest). (C4: there is no `dist:mac`; the earlier mention was wrong.)

### P4. Answer to the open question

ROOT approved `/usr/bin/loomd` for the initial build (2026-11-15). A future CLI package collision must be documented.

Earlier note:

The deb `/usr/bin/loomd` name comes from Tauri (`externalBin` basename). No separate Loom CLI deb exists today, so I'm keeping it and listing it as a known limitation.

## CI (owner: CI worker)

Status: **READY_FOR_REVIEW, frozen.** Workflow written (`.github/workflows/desktop.yml`, actionlint rc=0), not run remotely (no push/dispatch). Ubuntu arm64 `.deb` built and smoke-tested in Docker (all required rc=0). Evidence: `CI.md`.

### C1. What the workflow consumes (P1–P5 as published, nothing else)

`npm ci --prefix desktop` → `pnpm --dir ui install --frozen-lockfile` → `npm --prefix desktop test` →
`npm --prefix desktop run dist` (CI=true ⇒ strict) → `node desktop/scripts/package.mjs check` →
`cargo test --locked --manifest-path desktop/Cargo.toml -p loom-desktop-host -p loom-desktop` →
reads `desktop/dist/artifacts.json`: re-hashes each file artifact, uploads `artifacts[].abs_path` + the
manifest + a mode-preserving tar of `desktop/dist`. Then `package.mjs verify <loomd inside the installer>`:
the deb is extracted, NSIS is installed silently with `/S /D=<temp>`, and `Loom.app/Contents/MacOS/loomd` is
checked in place.

### C2. Env CI sets

`LOOM_DESKTOP_TARGET_DIR=${{ github.workspace }}/.desktop-target` (so rust-cache can restore it). The
main-workspace loomd uses `<repo>/target` (P5). No `CARGO_TARGET_DIR` is set. Docker harness:
`LOOM_STAGE_TARGET_DIR=/cache/target`, `LOOM_DESKTOP_TARGET_DIR=/cache/desktop-target`,
`CARGO_HOME=/cache/cargo`, `APPIMAGE_EXTRACT_AND_RUN=1`.

### C3. Runtime expectations CI asserts (runtime: shout if any is wrong)

- Linux: the extracted `usr/bin/loom-desktop`, launched under Xvfb with a private `LOOM_HOME`, spawns
  `/proc/<pid>/exe == <extract>/usr/bin/loomd` (R1). That daemon serves `/ui/` and is still alive after
  the app is killed.
- Windows: the app exe is the one non-`loomd.exe`, non-`uninstall.exe` `.exe` in the install dir. The daemon
  process `Path` is that dir's `loomd.exe`, and it survives the app exit.
- `run/daemon.json` has `pid` and `port` (same fields as verify-loomd uses).

### C4. Packaging nits seen by CI

- P5 mentions `npm run dist:mac`, but `desktop/package.json` currently has no `dist:mac` script. CI does
  not use it.
- Linux is deb-only (P2 updated by packaging). CI asserts `artifacts.json` `verified===true` before upload.

## Open questions

- (packaging) deb installs the sidecar as `/usr/bin/loomd`; that collides with any separately
  packaged Loom CLI `loomd`. Acceptable for now? Runtime needs only "next to the app binary".
