# Tauri desktop implementation — c7112c6

User authorizes implementation with native foreground Opus workers in parallel. ROOT owns orchestration, review, Git index. Shared cwd, file ownership disjoint. Existing dirty files are unrelated and must be preserved. No subagents/background LLMs. Use claude-opus-5-5. Missing HERDR_ENV is explicitly waived by user. No production coding by ROOT.

Goal: actual macOS Tauri desktop application hosting current real Loom UI and same loomd as CLI/agents. Existing daemon reused; absent daemon starts from bundled/exact explicit binary. Normal window/app close leaves daemon/devices/tasks alive. No fixture replay as product. Native launch may issue its own fresh viewer bootstrap using local daemon token (never read a human login code); credentials kept out of logs/screenshots/reports. No device opens outside Loom. No global policy/auth relaxation. Do not kill user-owned daemons. Validation uses own isolated LOOM_HOME. Existing workspaces MAIN w8 and protected Ego tabs preserved. After handoff normal /exit + workspace close by ROOT; keep source and evidence.

Scope is phase 1: app launch/error-retry, existing UI, connect/start lifecycle, local packaging/build, meaningful native and CLI/UI shared-stream smoke. Do not redesign UI, implement debugger, add updates/tray/cloud features. Need honest verified vs unverified status, not just browser-only proof. Existing browser UI remains supported.

## Parallel owners

- `tauri-app-opus`, w2H: `desktop/**` EXCEPT host/ and scripts/ and README.md; owns desktop/Cargo.toml + Cargo.lock, src-tauri, launch/error frontend, icons/config. No root workspace Cargo files or ui/src changes without ROOT coordination. Separate desktop Cargo workspace (avoid polluting main workspace lock). Build actual macOS app, inspect official Tauri docs, native UI validation coordination. Must integrate host crate API below. Only this owner cargo builds the full desktop workspace; shared build target `/tmp/loom-tauri-build` to avoid giant repo caches. All build evidence in work/reports/tauri-desktop/app/. Rust build tests appropriate; no redundant full repo suite.
- `tauri-host-opus`, w2J: `desktop/host/**` ONLY + work/reports/tauri-desktop/host/. Pure Rust lifecycle crate `loom-desktop-host`, no Tauri dependency. Own independent lifecycle tests with local HTTP fixtures and executable process fixture as needed. Until desktop workspace created, can use a scratch test workspace; coordinate lock/build with app owner via status files. No root production file changes.
- `tauri-qa-opus`, w2K: `desktop/scripts/**`, desktop/README.md ONLY + work/reports/tauri-desktop/qa/. Packaging/preflight scripts must bundle actual loomd + existing UI correctly without rebuilding all modules needlessly. Survey actual dependencies and test feasibility; validate actual native app once owners READY. Independent source review and real isolated external UART fixture + CLI/shared UI smoke; CAN/UDS/video as time justified, minimum meaningful WKWebView keyboard/terminal and daemon persistence on close. CUA/native inspection APIs available (discover tool), Ego only for browser parity (exclusive QA lease). No browser-only substitution for native app. Must diagnose native inspection blockers and report exact evidence, no fake PASS. Cannot edit other owners' files, request fixes through brief files/ROOT. Do not install big SDK/global packages or modify user settings; ordinary task-local npm/Rust dependencies allowed.

## Frozen host API v1 (A/B implement consistently)

```
pub struct ConnectOptions {
  pub home: std::path::PathBuf,
  pub daemon_bin: std::path::PathBuf,
  pub ui_dir: Option<std::path::PathBuf>,
  pub startup_timeout: std::time::Duration,
}
pub struct DesktopSession { /* credentials private, no Debug exposing URL/token */ }
impl DesktopSession {
  pub fn viewer_url(&self) -> &str; // canonical http://localhost:PORT/ui/#boot=..., no logs
  pub fn port(&self) -> u16;
  pub fn pid(&self) -> u32;
  pub fn started(&self) -> bool;
}
pub fn connect(options: &ConnectOptions) -> Result<DesktopSession, HostError>;
// HostError Display redacted and user-actionable. No Drop kills daemon. std::process spawn stdio
// routed to protected own home/run/loomd.log, no pipe that blocks/reaps on app exit. Reuse existing
// daemon discovery + authenticated endpoint. Bounded timeouts. stale/race/auth/protocol errors explicit.
```

Daemon already serves real embedded UI at /ui/. Discovery `<home>/run/daemon.json` = {port,pid,token,version,proto}. `GET /api/v1/bootstrap/new` bearer token returns fresh url/code, current policy bounded rights. Canonical origin localhost. UI exchanges fragment and removes it. No broadened remote IPC capabilities. Desktop loads exact selected daemon origin; restrict navigation/new windows to appropriate app/local target and explicit external links. Need actual Tauri WKWebView check, not Ego Chromium assumption. Main shell current dark; this task keeps UI design.

Use official docs for current Tauri APIs, local source for Loom contracts. Files should be implementation, not just proposal. Artifacts: each owner STATUS.md and READY.json with scope/diff, commands/results, blockers, processes/paths/cleanup, handoff. Notify ROOT once on READY. Build coordinator may acquire a terminal lock; no parallel Cargo lock mutations. No Git by workers. No force or cleanup of other's work. Final next steps: ROOT reviews, sends fixes to owner, integration checks then scoped feature commit. Push not automatically requested for this new task.
