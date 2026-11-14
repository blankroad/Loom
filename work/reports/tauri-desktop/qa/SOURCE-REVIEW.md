# QA independent source review: desktop/ (snapshot 2026-11-15 ~07:35 KST)

Scope read: desktop/src-tauri/src/{main,nav,config}.rs, capabilities/launch.json, tauri.conf.json,
desktop/host/src/{lib,http,error}.rs (spawn / redaction / HTTP paths). No edits. These are findings, not fixes.

## Looks correct
- **IPC surface**: the `launch` capability is `local: true`, windows `["main"]`, and only allows `launch_state` and
  `retry`. The remote daemon origin gets no IPC. `withGlobalTauri: false`.
- **Navigation**: an exact `http://localhost:<selected port>` origin is required (no 127.0.0.1, no https, no
  userinfo, no other port). Other schemes are blocked. After the latest nav.rs revision, external http(s) opens in
  the browser only for new-window requests that come from a user gesture; top-level external navigation is denied
  (no script-driven browser launches). External URLs are passed to `/usr/bin/open` as a single argv element, with
  no shell.
- **Viewer URL**: it is checked against the session port before navigating. It is never printed. stderr gets only
  the pid, port and home (smoke `hygiene` PASS: no token or boot fragment in app stdout/stderr, loomd.log, evidence
  or process args).
- **Host**: `DesktopSession` Debug redacts `viewer_url`. HostError says it never carries the token, code or URL.
  The bearer token goes only in the header of a bounded HTTP/1.1 GET to 127.0.0.1. The daemon is spawned with
  stdin `/dev/null` and stdout/stderr going to `<home>/run/loomd.log` (mode 0600), in its own process group.
  A background reaper prevents zombies, and nothing kills the daemon on Drop or app exit.
- **Retry**: an `AtomicBool` guard stops concurrent attempts. Retry is a no-op unless the state is Failed.

## Findings / suggestions (severity: low unless noted)
1. **setsid parity (low)**: the CLI spawns `loomd` "own process group / session". The host uses only
   `process_group(0)`. For LaunchServices launches this is fine: there is no controlling terminal. If
   `loom-desktop` is run directly from a terminal, the daemon stays in the terminal's session. A hangup normally
   reaches only the foreground group, but `setsid()` (via `pre_exec`) would match the CLI exactly.
2. **Daemon restart while the window is open (phase-1 limitation)**: if the user runs `loom daemon stop` and then
   `start`, the new daemon may bind a different port. The page loses its socket, and navigation to the new origin
   is not allowed until the app relaunches or retries. The launch page Retry only appears on the Failed state, so a
   connected window cannot reconnect without a relaunch. Document it, or add "Reconnect" later.
3. **Finder `PATH` (documented in README)**: a daemon *started by the app* inherits launchd's minimal `PATH`, so
   video and logic cannot find Homebrew `ffmpeg`/`sigrok-cli` unless `[video] ffmpeg` / `[logic] sigrok_cli` are
   set as absolute paths. UART, CAN, UDS and ETH are unaffected.
4. **Codesign (packaging)**: `codesign --verify --deep --strict` fails on the debug bundle ("code has no resources
   but signature indicates they must be present"). The linker ad-hoc signs each Mach-O, but the bundle is not
   sealed. Local launch works (verified). For distribution you need a signing identity and notarization (out of
   scope). Optionally, `bundle.macOS.signingIdentity: "-"` seals an ad-hoc bundle for cleaner local verification.
5. **Stale embedded UI (fixed in scripts, MEDIUM process risk)**: `target/debug/loomd` embedded an older UI
   (`index-DVbGndTA.js` vs dist `index-COcBrDfS.js`). Any packaging path that skips `stage-loomd.sh`/`verify-loomd.sh`
   can ship a stale UI. `build-app.sh` refuses unless the staged binary passes the check.
6. **Generated files**: resolved. `desktop/.gitignore` (app owner) ignores `src-tauri/binaries/`, `src-tauri/gen/schemas/`
   and `node_modules/`.
