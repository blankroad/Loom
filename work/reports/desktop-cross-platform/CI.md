# CI worker — desktop cross-platform (Ubuntu + Windows + macOS)

State: **READY_FOR_REVIEW — frozen** (2026-11-15). Native foreground only; no subagents. I did not touch Git or
the index, and nothing was pushed or dispatched.

This report replaces the initial proposal. The packaging contract the workflow uses is the published one
(`INTERFACE.md` P1–P5: `package.mjs`, `artifacts.json`, `LOOM_DESKTOP_TARGET_DIR`).

## Files (owned / ROOT-assigned)

| File | Change |
|---|---|
| `.github/workflows/desktop.yml` | **new** (246 lines). Matrix `ubuntu-22.04` / `windows-2022` / `macos-15`, all required. sha256 2d67b834… |
| `desktop/README.md` | cross-platform section: packages table, deb-only Ubuntu, Windows video unsupported, CI, Docker harness |
| `crates/loom-video/src/worker.rs` | ROOT-assigned minimal portability: `cfg(unix)` on the media socket server, process group, file modes. Non-Unix `open` refuses first with `E_UNSUPPORTED` |
| `crates/loom-video/src/media.rs` | ROOT-assigned: Unix-socket client `cfg(unix)`; non-Unix `ask` → `503 media_unavailable` |
| `crates/loom-video/tests/fixture_e2e.rs` | `#![cfg(unix)]` (Unix socket + /tmp fixture) |
| `crates/loom-video/tests/platform_unsupported.rs` | **new**, `#![cfg(not(unix))]`: open → `E_UNSUPPORTED` before ffmpeg; frame/meta → 503 `media_unavailable` |
| `crates/loom-logic/tests/engine.rs` | ROOT-assigned: `#!/bin/sh` fake-engine helper + its 3 tests and the `PathBuf` import `cfg(unix)` |
| `work/reports/desktop-cross-platform/docker/{Dockerfile.ubuntu2204,run-ubuntu.sh,ubuntu-build.sh}` | **new** evidence harness (not product) |
| `REQUEST-backend-windows.md`, `REQUEST-packaging-appimage.md`, `INTERFACE.md` § CI, `evidence/**` | coordination + logs |

Unix behavior of `loom-video` is unchanged: every guard is `cfg(unix)` around the original code. Two small helpers
were factored out with identical Unix bodies: `create_private_new` (0600 `create_new`) and `ensure_private_dir`
(0700).

## Results (real command exit codes, never the wrapper's)

### Windows — supplemental cross-compile `x86_64-pc-windows-gnu` (not MSVC, not native)

mingw `x86_64-w64-mingw32-gcc`, isolated `CARGO_TARGET_DIR=/tmp/loom-cross-windows`. Logs: `evidence/windows-gnu-cross/`.

| Command | Before fix | After fix |
|---|---|---|
| `cargo check --locked -p loom-gateway --bin loomd --target …gnu` (default features) | **101**: loom-video 21 errors | **0** |
| `cargo check --locked --keep-going --workspace --all-targets --target …gnu` | **101**: loom-video lib/test, loom-logic test | **0** |
| `cargo clippy --locked --no-deps -p loom-video -p loom-logic --all-targets --target …gnu -- -D warnings` | — | **0** |
| `cargo build --locked --release -p loom-gateway --bin loomd --target …gnu` | — | **0**, `loomd.exe` PE32+ x86-64 |
| `cargo test --locked --no-run -p loom-video -p loom-logic --target …gnu` | — | **0**, `platform_unsupported-*.exe` linked |

Intermediate rc=101 (`a-…`/`e-…` logs): an over-gated `AsyncReadExt` import, fixed. Clippy without `--no-deps` hits
a **pre-existing** `loom-console` `large_enum_variant` (`c-clippy-video-logic.log`). That is outside this task and
was not changed. **Windows tests were not executed** (no Windows host, no wine).

### macOS host — Unix behavior preserved (`CARGO_TARGET_DIR=/tmp/loom-ci-host-target`)

`evidence/host-unix-tests/`.

- `cargo test --locked -p loom-video -p loom-logic` **rc=0**: loom-logic lib 15, engine 10 (incl. the 3
  `cfg(unix)` fake-engine tests), gateway 4; loom-video lib 12, `fixture_e2e` 4, `platform_unsupported` 0
  (Unix).
- `fixture_e2e -- --nocapture` rc=0, **no SKIP**: real Homebrew ffmpeg + Pillow 12.0.0 + loopback MJPEG
  fixture, no device. This exercises snapshot/recording-marker `create_private_new`, `ensure_private_dir`, the
  Unix media socket (0600 asserted) and process-group stop.
- `cargo clippy --locked --no-deps -p loom-video -p loom-logic --all-targets -- -D warnings` **rc=0**.
- rustfmt: the crate was already not rustfmt-clean at HEAD (29 hunks in worker.rs). My edited lines follow the
  existing style and add no new drift in their own lines.

### Ubuntu 22.04 arm64 — real Docker build (image `loom-ci-ubuntu2204:tauri`, 2.38 GB, toolchain only)

The repo is mounted **read-only**. A filtered snapshot is built in volume `loom-cross-ubuntu-root-check-cache`
(shared with ROOT's check). target, node_modules, `.git`, `work/` and `spikes/` are never copied. No privileges
were added, and the user's existing containers were not touched.

| Attempt | Result | Evidence |
|---|---|---|
| 1 | `05-dist` **rc=1**: `.deb` sidecar PASS; AppImage sidecar FAIL (linuxdeploy patchelf adds `RUNPATH $ORIGIN/../lib`; `.text/.rodata/.data` and build-id identical; `NO_STRIP=true` gives the same mutated sha bfce1276…) | `evidence/ubuntu-attempt1/`, `REQUEST-packaging-appimage.md` |
| 2 | `01-provenance` **rc=1**: harness bug (`find` exits 1 on the missing `ui/public` + pipefail). No dist | `evidence/ubuntu-final/attempt2-provenance-fail.run.log` |
| 3 | all rc=0 up to `13-deb-launch-smoke` **rc=1**: harness bug (root can't read another uid's `/proc/<pid>/exe`: Docker drops CAP_SYS_PTRACE). Product started the daemon, `/ui/` 200, daemon survived. **Superseded** | `evidence/ubuntu-attempt3-superseded/` |
| **4 (final build)** | **16/16 required steps rc=0** | `evidence/ubuntu-final/steps.tsv` + logs |
| **smoke (same .deb, fresh container, no rebuild)** | **8/8 required steps rc=0** | `evidence/ubuntu-final/steps-smoke.tsv`, `smoke-*.log` |

Final build steps (rc): snapshot 0 · provenance 0 (923 files, `snapshot.sha256`) · `npm ci` 0 · `pnpm install
--frozen-lockfile` 0 · `npm --prefix desktop test` 0 · `npm --prefix desktop run dist` 0 (strict, deb-only) ·
`package.mjs check` 0 · `cargo test --locked` desktop host+app 0 · manifest `verified=true` + kinds=`deb` only 0 ·
`dpkg-deb -c` 0 · `dpkg -i` 0 · `package.mjs verify /usr/bin/loomd` 0 · installed `/usr/bin/loomd` `cmp` staged
0 · launch smoke 0 · artifacts 0 · snapshot drift 0.

Launch smoke: app runs as unprivileged user `loomsmoke` under Xvfb with a private `LOOM_HOME`. It passes only
if the app is alive, the daemon exe (read as the same uid) is `/usr/bin/loomd`, `GET /ui/` returns 200, and the
daemon is alive after the app is killed. Smoke result: `exe=/usr/bin/loomd`, `GET /ui/ -> 200`, app alive yes,
daemon alive after app exit yes. This is a **limited headless smoke**, not visible-UI or hardware QA.

Toolchain recorded in `01-provenance.log`: rustc 1.93.1, Node 24.22.2, pnpm 11.11.0, Ubuntu 22.04,
libwebkit2gtk-4.1-dev from the 22.04 archive.

**Delivered artifact** (gitignored, not in Git):

| | |
|---|---|
| host | `desktop/dist/linux-arm64/Loom_0.1.0_arm64.deb` (12 750 150 bytes) |
| container (manifest path) | `/out/dist/Loom_0.1.0_arm64.deb` = host `/tmp/loom-ci-ubuntu/out/dist/…` |
| sha256 (manifest == host copy) | `9b28d36c61f9f3286dfd10676a3b705fab76f41156a995d817b41c96192884c0` |
| manifest | `desktop/dist/linux-arm64/artifacts.container.json` (unmodified), mapping in `HOST-MAP.json` |
| staged loomd sha256 / ui_dist_digest | `d8fcdf23…0809602` / `643e5952…6b0f05` |

`desktop/dist/Loom.app` is preserved. The `.deb` is not byte-reproducible across builds (attempt 3 dbcf9564… vs
attempt 4 9b28d36c…). The delivered file is exactly the one the smoke tested.

**Source drift after the build:** `run-ubuntu.sh compare` rc=1, with **only `desktop/README.md`** changed. These are
the doc-only edits made after the build at ROOT's request (cross-bundling wording, CI launch-smoke scope, Docker
modes). README is not a build input, so the binaries were not rebuilt (`evidence/ubuntu-final/compare-after-doc-edits.log`).
Before those edits, the snapshot matched the current source (923 files).

### Workflow checks

- actionlint **1.7.7** (official release binary, checksum-verified, in scratchpad): `actionlint -shellcheck=
  .github/workflows/desktop.yml` **rc=0** on the final file. **shellcheck integration not run**: downloading the
  shellcheck binary was refused by the permission classifier, and I did not work around that. `run:` scripts
  were checked only by actionlint's built-in checks.
- ROOT independent check: official actionlint **v1.7.12**,
  `/tmp/loom-actionlint/actionlint -shellcheck= .github/workflows/desktop.yml` → **rc=0, no diagnostics**
  (empty `actionlint.log`). The YAML, Actions schema and expression validation passed. shellcheck was explicitly
  disabled (not installed). ROOT credential scan of the cross-platform docs/logs: 0 potential real-token patterns.
  The host artifact sha `9b28d36c…` matches `HOST-MAP.json`.
- The manifest → upload-list step was **executed locally**, extracted from the parsed YAML, with a temp
  `GITHUB_OUTPUT`. `verified:true` gives rc 0 and real `0x0a`-separated `paths<<__EOF__` lines (od dump checked).
  `verified:false` gives rc 1. A sha mismatch gives rc 1 and nothing is written. The source has single-escaped
  `\n` in a JS template literal.

## Workflow design (`.github/workflows/desktop.yml`)

Pins: Rust 1.93.1, Node 24.22.2, pnpm 11.11.0, runners `ubuntu-22.04` / `windows-2022` / `macos-15`.
`npm ci` + `pnpm --frozen-lockfile` + `cargo --locked`. Steps: packaging tests → (Windows)
`cargo test -p loom-video --test platform_unsupported` → `npm --prefix desktop run dist` (CI=true ⇒ strict) →
`package.mjs check` → desktop `cargo test` → manifest gate (`verified===true`, sha re-check) → per OS:

- **Linux**: `dpkg-deb -x`, `verify` the extracted `loomd`, and an Xvfb launch smoke (exe == extracted `loomd`,
  `/ui/`, survives app exit). An `always()` cleanup kills only recorded pids and processes from the extracted path.
- **Windows**: silent NSIS `/S /D=<temp>`, `verify` the installed `loomd.exe`, and a launch smoke (daemon `Path` ==
  installed `loomd.exe`, `/ui/`, survives app exit). An `always()` cleanup touches only processes from the
  private install dir.
- **macOS**: `codesign --verify --deep --strict` + `verify Loom.app/Contents/MacOS/loomd`. **No app launch.**
- Upload `artifacts[].abs_path` + manifest + a mode-preserving tar (relative path; Git Bash tar would read
  `D:\` as host:path).

## Unverified / limitations

- **The workflow has never run on GitHub** (no push/dispatch authorization). The MSVC build, NSIS bundling, the
  silent install, the Windows launch smoke and `platform_unsupported` execution are **unverified until CI runs**.
  The GNU cross-build is supplemental evidence only.
- Ubuntu x64 is unverified locally (Docker gave arm64 only). CI covers x64 on `ubuntu-22.04`.
- Windows video capture is **unsupported** (explicit `E_UNSUPPORTED` / 503). A Windows media transport is
  separate work.
- AppImage is experimental/unverified and not a deliverable (ROOT decision; packaging made Linux deb-only).
- The macOS visible-UI QA (final2) is still pending on the locked screen. Nothing here replaces it.
- shellcheck was not run (see above).

## Next (ROOT)

1. Push/dispatch `desktop.yml` when authorized. The first run gives the real Windows MSVC/NSIS evidence.
2. If the Windows launch smoke fails because of runner GUI-session limits (not the product), decide whether to
   keep it required.
3. Optional: an arm64 Ubuntu runner (`ubuntu-22.04-arm`), if the repo plan allows it.
