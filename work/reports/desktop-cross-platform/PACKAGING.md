# Packaging (cross-platform) — worker report

State: **READY_FOR_REVIEW — scope FROZEN** (2026-11-15). Linux is deb-only (ROOT decision). CI worker: ready for the
cached rerun and the real `.deb` install/launch checks. Interface: `INTERFACE.md` § Packaging (P1–P5).

## What changed (owned files only)

| File | Change |
|---|---|
| `desktop/scripts/package.mjs` (new) | Portable CLI: `stage`, `check`, `verify`, `bundle`, `dist`. Enforces Node >= 24. |
| `desktop/scripts/portable/lib.mjs` (new) | Sidecar naming (`loomd-<triple>[.exe]`), explicit per-OS bundles, desktop target dir (`LOOM_DESKTOP_TARGET_DIR` > `CARGO_TARGET_DIR` > `/tmp/loom-tauri-build` or `%TEMP%\loom-tauri-build`; refuses anything inside `desktop/`), shell-compatible dir digest, mkdir lock, no-shell spawn (cmd.exe only for fixed literal args to `.cmd` shims), process-group stop. |
| `desktop/scripts/portable/stage.mjs` (new) | Fresh `pnpm build`, `cargo clean -p loom-ui-assets`, `cargo build --locked -p loom-gateway --bin loomd` (main workspace, `CARGO_TARGET_DIR` removed; `LOOM_STAGE_TARGET_DIR` override). Writes the staged sidecar + `loomd.ui-index.html` + `loomd.stamp.json` (same schema as `stage-loomd.sh`, plus `staged_by`). `check` = the shell rules plus `target == host triple`. `verify` = private temp `LOOM_HOME`, byte-compares every `/ui/` file, checks CSP, **fails if the daemon.json pid != the spawned pid**, stops only its own pid (bounded), removes only its own temp home. |
| `desktop/scripts/portable/bundle.mjs` (new) | Requires `check`. Touches the staged sidecar (forces tauri-build to re-copy it), then `node tauri.js build --bundles <default> --ci [--debug] -- --locked` under `<target>/.cargo-desktop.lock`, with the build child in its own process group. On a signal the tree is stopped and the lock is released only once that tree is confirmed gone, otherwise it is kept and the pids are printed. Checks: Cargo.lock unchanged, `<target>/<profile>/loomd[.exe]` == staged, exactly one artifact from this build per kind. Contents: mac → `verify-bundle.sh`; deb → `dpkg-deb -x`, `usr/bin/loomd` sha == staged, app binary next to it; nsis → `7z x`, `loomd.exe` sha == staged, app exe next to it. Tool missing ⇒ WARN ⇒ `verified:false`; strict (default when `CI=true`) ⇒ exit 1. Copies to `--out-dir` (default `desktop/dist`; mac `Loom.app` + `codesign --verify --deep` after the copy) and writes `artifacts.json` (typed, with real hashes). |
| `desktop/scripts/portable/test/*.test.mjs` + `fake-loomd.mjs`, `helpers.mjs` (new) | 23 `node:test` tests. The fake daemon runs via `process.execPath` (no shebang/exec-bit assumption). |
| `desktop/src-tauri/tauri.linux.conf.json` (new) | `targets: ["deb"]`, deb section `devel`. |
| `desktop/src-tauri/tauri.windows.conf.json` (new) | `targets: ["nsis"]`, icon list with `icons/icon.ico`, `downloadBootstrapper`, NSIS `currentUser`. |
| `desktop/src-tauri/icons/icon.ico` (new) | 6-size ICO from the existing `icon.png` (`tauri icon`; no other icon replaced). Needed by tauri-build's Windows resource. |
| `desktop/package.json`, `package-lock.json` | Every script is portable `node scripts/package.mjs …`: `test`, `stage`, `stage:debug`, `check`, `verify`, `build:app`, `build:app:debug`, `dist`, `dist:debug`, `tauri`. `engines.node >=24`. The POSIX-only `${VAR:-}`/`rm`/`ditto` scripts are removed. |
| `desktop/README.md` (CI-owned; edited per ROOT) | Platform table/notes: deb is the only Linux format; AppImage is unsupported, with a pointer to the evidence. |

Base `tauri.conf.json` is unchanged (mac `targets: ["app"]`, `externalBin: ["binaries/loomd"]`). The mac shell
scripts are unchanged.

## Evidence (native, this Mac: arm64, Node 25.6.1; Ubuntu 22.04 aarch64 container, Node 24.22.2)

| Command | Result |
|---|---|
| `npm --prefix desktop test` (macOS) | 23/23 pass |
| same in `loom-ci-ubuntu2204:tauri`, repo mounted **read-only**, `--network none` | 23/23 pass, 0 skipped (the shasum cross-check ran) |
| `node desktop/scripts/package.mjs check` on the **shell-staged** sidecar | `current`: Node digest == `stage-loomd.sh` digest |
| `node desktop/scripts/package.mjs stage --profile debug` | fresh pnpm build + cargo; verify 6/6 files, CSP yes, PASS, `terminate-own-pid`; UI digest `643e5952…` (unchanged), loomd `5d44ac19…` |
| `stage-loomd.sh --check` + `verify-loomd.sh` on the Node staging | current / PASS (both directions interoperate) |
| `package.mjs bundle --debug --out-dir <scratch>` | waited for the lock; tauri app bundle; `target:sidecar`, `verify-bundle.sh` (identity across ad-hoc re-sign, embedded UI, codesign), copy codesign: all PASS; manifest `verified:true` |
| `verify-dist.sh <scratch>/Loom.app` (legacy strict) on the Node-copied app | PASS (LC_UUID + text/const sections identical) |
| SIGTERM to `package.mjs bundle` mid-build | build tree stopped, `no build processes`, lock released after the confirmation, no leaked verify home |
| Re-bundle after restoring the prior staging with `cp -p` | first try: `target:sidecar FAIL` (stale tauri-build copy, mtime). Fixed with the pre-build touch; rerun PASS |
| Ubuntu (CI worker run, root log `/tmp/loom-ci-ubuntu/out/05-dist.log`, preserved at `evidence/appimage-strict-fail-05-dist.log`) | stage PASS; **deb sidecar sha == staged PASS**; AppImage sidecar `bfce1276…` != staged `836863dd…` → FAIL (linuxdeploy rewrites the ELF) → AppImage removed from supported/allowed bundles |

Prior QA state preserved: after my mac runs I restored the original `loomd-aarch64-apple-darwin`, stamp and
ui-index (sha `7efffad8…`). Then `stage-loomd.sh --check` = current and `verify-dist.sh desktop/dist/Loom.app` = PASS.
`desktop/dist/` was never written (all outputs went to the scratchpad). `ui/dist` was rebuilt with the identical digest.

## Unverified / limits

- **Windows: nothing run natively.** NSIS bundling, the 7z inspection, `.cmd` routing for pnpm, `taskkill /T` and
  path handling are covered statically and by unit tests only (`_spawnSpec`, win32 target-dir paths, env case). The
  first real proof is the CI Windows runner.
- Ubuntu full `npm run dist` (deb-only) after the freeze: CI worker reruns. The deb passed the exact check in the earlier run.
- Release-profile `dist` was not run on the Mac (debug only, so the prior QA staging stays intact). The release path
  differs only in the `--release` flags.
- On a signal, the CLI exit code may be 1 rather than 130 when the build child's failure is reported first. The lock
  behaviour is the same either way.
- deb installs `/usr/bin/loomd` (ROOT-approved). A future Loom CLI package would collide; document it then.

## Next

CI: cached Ubuntu rerun of `npm --prefix desktop run dist` (deb) plus the install/launch checks; Windows runner for
NSIS. ROOT: review, then integrate.
