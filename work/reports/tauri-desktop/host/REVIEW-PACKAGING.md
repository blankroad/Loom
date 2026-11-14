# Independent review: desktop packaging and smoke scripts (by tauri-host-opus, read-only)

Date: 2026-11-15 · Scope: `desktop/scripts/{stage-loomd,verify-loomd,build-app,verify-bundle,preflight,smoke-native}.sh`,
`desktop/scripts/lib/{uart_fixture.py,winlist.swift}`. No files modified, nothing built or run.
Existing QA evidence was read only to confirm the codesign row.

Checked, no action needed:
- A debug `loomd` really embeds the UI. `loom-ui-assets` enables rust-embed's `debug-embed`, so
  `verify-loomd` byte-comparing `/ui/*` against `ui/dist` proves identity and does not read from
  disk at runtime.
- `--check` requires sha, dist digest and `embedded_ui_verified`, so stale staging is caught.
- `verify-loomd` only signals the pid it spawned, uses a private 0700 home, and never prints the token.
- verify-bundle reports codesign as a WARN, not a fake PASS. QA evidence shows "code has no
  resources…" for the local ad-hoc build. That is acceptable for local launch, and the bundled
  `loomd` is actually executed by `verify-loomd --app`, so an exec-breaking signature would still FAIL.
- The fixture pty is the only device touched, and no device opens outside Loom.

## Findings (most severe first)

### P1 (high, smoke validity): `find_app` can return the bundled daemon or a worker as `APP_PID`

- Where: `smoke-native.sh:45-46` (`pgrep -f "^$APP/Contents/MacOS/"`, newest = `sort -rn`), used
  at `:75` (start), `:115` (reopen) and `:126`.
- Why: the pattern matches every executable in `Contents/MacOS/`, not just the app.
  - The host starts `Contents/MacOS/loomd` with argv[0] equal to that full path and env
    `LOOM_HOME=<home>`, and its `loomd worker …` children inherit both. The step-`start` assertion at
    `:78` (`exe_of DAEMON_PID == $APP/Contents/MacOS/loomd`) passing proves the daemon's command
    line starts with `$APP/Contents/MacOS/`.
  - `start` resolves `APP_PID` right after `has_app` first succeeds. The daemon spawns about 0.1 s
    after the app, and its workers spawn later still with higher pids. "Highest pid with our
    LOOM_HOME" is therefore often the daemon or a worker. The same holds in `reopen` whenever a worker
    is spawned on demand during the wait.
- Consequences:
  - `after-close` waits for the wrong pid. It fails "app pid still running", or passes only when a
    worker exits for an unrelated reason.
  - `cleanup` sends SIGTERM to "the app" and actually stops the QA daemon or a worker (still an
    isolated home, but the identity is wrong).
  - The native-close persistence result is not trustworthy until this is fixed.
- Fix: match only the main executable, e.g. `exe="$(/usr/libexec/PlistBuddy -c 'Print
  :CFBundleExecutable' "$APP/Contents/Info.plist")"`, then use `pgrep -f "^$APP/Contents/MacOS/$exe( |$)"`.
  Or exclude `loomd` explicitly, and escape the regex metacharacters in `$APP` (the `.` in
  `Loom.app`). Re-run `start`/`after-close`/`reopen` and check that the recorded `APP_PID`'s `comm`
  is the app executable.

### P2 (medium): `build-app.sh` claims to fail on a `desktop/Cargo.lock` change, but only warns and never passes `--locked`

- Where: `build-app.sh:12` (doc: "fails if desktop/Cargo.lock would change"), `:57-61`.
- Why:
  - `tauri build` runs cargo without `--locked`, so a QA or ROOT run can silently resolve and
    rewrite the app owner's lock file. That breaks the lock ownership rule and lets dependency
    versions drift without review.
  - The post-hoc check only prints a WARNING. It also skips the check entirely when no lock existed
    (`before == none`), so the build can create a lock.
- Fix: pass the runner flag, `"$tauri" build --bundles app --ci [--debug] -- --locked`, and make a
  changed or created lock an `exit 1`.

### P3 (medium): build-app exports `CARGO_TARGET_DIR` before staging, so `stage-loomd.sh` builds the MAIN workspace into `/tmp/loom-tauri-build`

- Where: `build-app.sh:20` (export) runs before `:33-34` (`stage-loomd.sh --from|--profile`).
  `stage-loomd.sh:100-105`: `cargo clean -p loom-ui-assets`, `cargo build --locked -p loom-gateway`,
  and `cargo metadata` all inherit it.
- Why:
  - `build-app.sh --stage-profile …` compiles the whole `loom-gateway` dependency tree from scratch
    into the desktop target, not the repo `target/` that the stage script documents ("its own target
    dir"). The task explicitly says not to rebuild modules needlessly.
  - It runs before the `.cargo-desktop.lock` mkdir lock is taken, so it contends with the app
    owner's desktop cargo runs on the same target directory.
  - Running `stage-loomd.sh` standalone versus via build-app produces two separate main-workspace
    builds.
- Fix: in `stage-loomd.sh`, run the main-workspace cargo commands with `env -u CARGO_TARGET_DIR`
  (or an explicit `--target-dir "$repo/target"`, and the same for `cargo metadata`). Or have
  build-app export `CARGO_TARGET_DIR` only after staging.

### P4 (low-medium): smoke waits are iteration-counted, not deadline-bounded

- Where: `smoke-native.sh:48` (`wait_for secs cmd` = `secs*5` tries plus 0.2 s sleeps).
- Why: each probe has its own unbounded latency, so a "30 s" wait can run for minutes:
  - `has_viewer` and `no_viewer` call `loom status --json` against the daemon. When the daemon is
    hung, each call can block until the CLI's own RPC timeout.
  - `has_app` runs `pgrep`, then one `ps` per candidate.

  For example, `wait_for 30 has_viewer` makes 150 tries, each possibly several seconds long. A
  daemon hang then stalls the smoke instead of reporting FAIL with a time.
- Fix: track a deadline with `$SECONDS` (`local end=$((SECONDS+secs)); while ((SECONDS<end))`) and
  bound each probe, e.g. `cli --timeout …` if the CLI supports it, or wrap the probe with a
  per-call watchdog.

### P5 (low, hygiene): the `hygiene` step puts the token in a process's argv

- Where: `smoke-native.sh:152` (`ps … | grep -F -- "$tok"`).
- Why: while it runs, the `grep` process's own command line contains the daemon token, which is
  visible to same-user `ps`. That is the very thing the step checks for. It is filtered out with
  `grep -v grep`, so it never reports itself, but it is a real transient leak.
- Fix: feed the pattern on stdin or a fd, e.g. `grep -F -f <(printf '%s\n' "$tok")`.
