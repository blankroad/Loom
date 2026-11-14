# Independent review — desktop app shell (by tauri-host-opus, read-only)

Date: 2026-11-15 · Scope: `desktop/src-tauri/src/{main.rs,config.rs,nav.rs}`, `src-tauri/build.rs`,
`src-tauri/capabilities/launch.json`, `src-tauri/tauri.conf.json`, `desktop/frontend/launch.js`
(read to check the IPC/render sinks). No files modified, nothing built or run.

Checked and found OK (no action): host API use matches v1. `connect` runs on the `loom-connect`
thread, not the main thread. The viewer URL is checked against `is_daemon_origin(port)` before
`navigate`. Only redacted `HostError` text, pid, port and home are logged. The capability is
`local: true`, limited to window `main`, with only the two app-manifest commands, so the daemon
origin, `child-*` windows and remote pages get no IPC. The launch page renders with `textContent`
only. The nav policy blocks `file:`/`data:`/`javascript:`/custom schemes, needs the exact
`http://localhost:<port>` origin, and rejects userinfo. `open_external` passes the URL as a single
argv element to `/usr/bin/open`, with no shell. Nothing in the app stops the daemon.

## Findings

### F1 (medium, functional) — after a successful connect there is no way back to Retry; a daemon restart strands the window

- Where: `main.rs:65-72` (`retry` acts only in `LaunchView::Failed`), `main.rs:124` and `135`
  (`port` and `Connected` are never reset), `main.rs:132-141`. There is no page-load or failure
  hook and no menu action.
- Scenario: the app connects. Then the daemon stops (`loom daemon stop`, a crash, or a restart by
  CLI or agent). The UI cannot reconnect:
  - Viewer sessions live in daemon memory (`loom-gateway/src/auth.rs` `sessions: Mutex<Vec<..>>`),
    so the old `loom_session` cookie is invalid for any new daemon. `/rpc` answers 401 with "viewer
    cookie required (open the bootstrap URL)".
  - If the new daemon gets another port (47123 is taken, or the OS assigns one), the pinned
    `launch.port` classifies the new origin as External. A reload then opens the system browser
    instead of the app window.
  - The daemon page has no IPC by design, so it cannot call `retry`. The launch state stays
    `Connected`, so `retry` is a no-op anyway.

  The user must quit and relaunch the app. This sits inside phase-1 scope ("app launch/error-retry",
  "connect/start lifecycle").
- Fix (any one):
  - (a) Add a native "Reconnect" menu item or accelerator. It sets the state to `Connecting`, clears
    `launch.port`, navigates `main` back to the bundled launch page, and calls `start_attempt`.
    Each successful connect mints a fresh code.
  - (b) Also hook `on_page_load` (or a failed load of the daemon origin) to fall back to (a)
    automatically.
  - At minimum, let `retry` run from `Connected` once the window is back on the launch page.

### F2 (low, security hardening) — any top-level http(s) navigation by the daemon page launches the system browser, including script or redirect navigations without a click

- Where: `main.rs:154-161` (`Nav::External` in `on_navigation` calls `nav::open_external`).
- Scenario: `on_navigation` cannot tell a user click from `location = ...`, a `<meta refresh>` or an
  HTTP redirect. Content that runs or is served on the daemon origin can therefore make the app
  open arbitrary external URLs repeatedly, with no user gesture. The pinned old port after a daemon
  restart (F1) feeds the same path: `http://localhost:<new>/ui/` is sent to the browser.
- Fix:
  - In `on_navigation`, return `false` for `External` without opening anything.
  - Open external links only through `on_new_window` (`target=_blank`). WKWebView blocks gesture-less
    `window.open` by default, so that path needs a user action.
  - Alternatively, rate-limit or de-duplicate `open_external`.

  If the UI relies on plain same-tab external links, document that they need `target=_blank`.

## Not findings (left to QA+ROOT native checks)

- WKWebView resolving `localhost` → `::1` first (the daemon binds 127.0.0.1 only), ATS treatment of
  `http://localhost`, and download behaviour for daemon artifact links (no `on_download` handler is
  set). None of these can be decided by reading the code; they need a native run.
