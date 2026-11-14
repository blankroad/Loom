# tauri-host-opus (w2J) — STATUS

State: **READY** (2026-11-15) · baseline c7112c6 · no Git used · no subagents

## Scope / diff (all new files, owner `desktop/host/**` only)

| file | lines | what |
|---|---|---|
| desktop/host/Cargo.toml | 13 | `loom-desktop-host`, plain-version deps `serde`, `serde_json`; no `[workspace]`, no dev-deps |
| desktop/host/src/lib.rs | 455 | `ConnectOptions`, `DesktopSession`, `connect`, discovery probe, start/instance locks, detached spawn, reaper |
| desktop/host/src/error.rs | 146 | `HostError` (redacted, user-actionable `Display`, `is_retryable`) |
| desktop/host/src/http.rs | 135 | bounded std-only HTTP/1.1 GET (content-length + chunked, 64 KiB cap) + unit test |
| desktop/host/tests/lifecycle.rs | 675 | 19 lifecycle tests + 2 re-exec fixture entries (process fixture daemon / fake app client) |
| desktop/host/tests/real_loomd.rs | 172 | real `loomd` + `loom` test in isolated temp LOOM_HOME (env-gated) |

No root workspace / ui/src / other owners' files touched. Nothing in desktop/Cargo.lock was
written by me (builds used a scratch workspace, see Commands).

## API (frozen v1, implemented exactly) + additive extras

```rust
pub struct ConnectOptions { pub home: PathBuf, pub daemon_bin: PathBuf, pub ui_dir: Option<PathBuf>, pub startup_timeout: Duration }
impl ConnectOptions { pub const DEFAULT_STARTUP_TIMEOUT: Duration /* 15 s */ }       // extra
pub struct DesktopSession { /* private; Debug shows viewer_url: "<redacted>" */ }
impl DesktopSession {
  pub fn viewer_url(&self) -> &str; pub fn port(&self) -> u16; pub fn pid(&self) -> u32; pub fn started(&self) -> bool;
  pub fn daemon_version(&self) -> &str; pub fn log_path(&self) -> &Path; pub fn origin(&self) -> String; // extras
}
pub fn connect(options: &ConnectOptions) -> Result<DesktopSession, HostError>;
pub fn default_home() -> Option<PathBuf>;   // extra: LOOM_HOME else ~/.loom, identical to the CLI
#[non_exhaustive] pub enum HostError { InvalidOptions, Home, DaemonBinaryMissing, Spawn, DaemonExited,
  StartupTimeout, Unresponsive, AuthRejected, ProtocolMismatch, Protocol }  // + is_retryable()
```

## Behaviour

1. Fast path (no locks): read `<home>/run/daemon.json`; require `proto` `loom/1.*`; `GET
   /api/v1/bootstrap/new` (Host `127.0.0.1:PORT`, Bearer token, no `rights` param → current policy
   rights; no policy relaxation). Accept only `http://localhost:{port}/ui/#boot=<code>` with
   printable ASCII. → `started()=false`.
2. Otherwise take `run/start.lock` (same protocol/wait ≤2 s as `loom`; held until ready so a
   concurrent CLI waits instead of double-spawning). Re-probe; then test `run/loomd.lock`
   (instance lock, released immediately). Lock held → a live daemon owns the home: never spawn,
   wait. Lock free → discovery is stale/absent → spawn.
3. Spawn: `daemon_bin` exact path (never PATH; must be an executable file), args `run`
   [`--ui-dir <canonical dir>` only when `ui_dir` has `index.html`], env `LOOM_HOME=home`, cwd
   `home`, stdin `/dev/null`, stdout+stderr appended to `<home>/run/loomd.log` (created 0600; `run/`
   created 0700), own process group (`process_group(0)`). Non-secret start marker line in the log.
   Wait for `daemon.json` with the child's pid + valid bootstrap. Child is reaped by a detached
   background thread (no zombie; never signalled; does not keep the app alive).
4. Never stops/kills a daemon: no `Drop`, timeout leaves the started daemon running (retry reuses).
5. Whole call bounded by `startup_timeout`; each HTTP probe ≤ min(2 s, remaining).
6. Errors: exited-during-start (status + log path), startup timeout, unresponsive owner, auth
   rejected (live owner, 401/403), protocol mismatch (live owner, immediate), bad bootstrap answer;
   stale discovery (dead port / foreign daemon on reused port / bad proto with free instance lock)
   is replaced, not reported. No error/Debug contains token, code, cookie or URL. Crate never logs.

## Commands / results (macOS, Darwin 27.0.0, cargo 1.93.1)

Scratch workspace (desktop lock untouched): `$SCRATCH/host-ws/Cargo.toml` =
`[workspace] members=["."]` package depending on `loom-desktop-host = { path = ".../desktop/host" }`.

```
cd $SCRATCH/host-ws
CARGO_TARGET_DIR=/tmp/loom-tauri-build/host cargo clippy -p loom-desktop-host --tests -- -D warnings   # clean
rustfmt --edition 2024 desktop/host/src/*.rs desktop/host/tests/*.rs                                  # repo rustfmt.toml (max_width 110)
LOOM_DESKTOP_HOST_LOOMD=$REPO/target/debug/loomd LOOM_DESKTOP_HOST_LOOM=$REPO/target/debug/loom \
  CARGO_TARGET_DIR=/tmp/loom-tauri-build/host cargo test -p loom-desktop-host -- --nocapture
```
Run twice: unit 1/1, lifecycle 21/21 (0.81 s), real_loomd 1/1 (0.54 s / 0.49 s) — logs
`test-run-1.log`, `test-run-2.log` here. Secret grep over logs: only a test *name* matches "token".

Verified by tests (fixture = this test binary re-exec'd through a `#!/bin/sh` wrapper acting as loomd):
reuse without spawn (nonexistent bin) · fresh code per connect · chunked answer · start absent daemon
(argv `run`, LOOM_HOME, cwd, pgid==pid, stdout+stderr in loomd.log, log 0600, run 0700) · drop keeps
daemon alive + reconnect reuses · `--ui-dir` only when starting · invalid options fail fast w/o spawn ·
missing/non-exec bin · stale dead port / foreign 401 daemon / unowned bad proto replaced · live 401 →
AuthRejected (bounded, no spawn) · live bad proto → ProtocolMismatch (no spawn) · off-origin URL →
Protocol (redacted) · early exit → DaemonExited w/ log · hang → StartupTimeout bounded (0.8 s ±) and
left running · 4 concurrent connects → exactly 1 spawn · **daemon outlives the app process exiting** ·
**daemon outlives SIGTERM+SIGKILL of the app's whole process group** · redacted actionable errors.

Real `loomd` (target/debug/loomd, built 2026-11-15 07:23, `loomd 0.1.0`), isolated temp LOOM_HOME:
started in 113 ms on an OS port (49201) · `/ui/` on `localhost` origin serves the selected
`--ui-dir` · minted code redeems once (`POST /api/v1/bootstrap` 204 + `loom_session` cookie), second
redeem 410 · drop → daemon alive, reconnect `started=false`, same pid/port, new code ·
`loom --json daemon status` (same LOOM_HOME, LOOM_NO_AUTOSTART=1) reports the same pid · log 0600, no
token/code in log. Test SIGTERMs only its own daemon; verified gone afterwards.

## Not verified here (other owners)

- Native WKWebView loading `viewer_url` (localhost → daemon binds 127.0.0.1 only; IPv6-first
  resolution fallback must be observed natively) — app/QA.
- Real app quit (Cmd-Q / window close) keeping the daemon alive — host-level equivalent proven
  above (process exit, group kill); native app proof is QA's.
- Windows path compiles by construction only (cfg), not built/tested.

## Handoff notes for app owner

- `connect` blocks up to `startup_timeout` → keep it off the main thread (src-tauri already uses a
  connect thread ✔). Codes are single-use, TTL 60 s: on Retry/reload call `connect` again.
- `HostError::to_string()` is safe to show and log; `is_retryable()` false → hide/relabel Retry.
- `ui_dir` is ignored when a daemon is reused (the running daemon keeps its own UI source).
- `default_home()` matches the CLI; a Finder-launched app has no LOOM_HOME → `~/.loom`.

## Processes / paths / cleanup

- Build target: /tmp/loom-tauri-build/host (mine only; deletable). Scratch workspace:
  /private/tmp/claude-501/-Users-ctmctm-workspace-Loom/7b7e1ac1-e3d1-4254-91db-f0f3445347d8/scratchpad/host-ws
- Temp homes `$TMPDIR/ldh-*` removed by tests (verified none left). No fixture or test daemon
  processes left (verified with ps). User daemons (`.worktrees/core` loomd 94847/95850) and the app
  owner's `/tmp/loom-app-iso` loomd 91840 untouched.
