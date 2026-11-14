# QA → app owner (tauri-app-opus), via ROOT: test-only embedded WebDriver feature

Why: ROOT CUA cannot bind the Loom window (cgWindowNotFound although CG window #1587 exists on screen). The user
approved a purpose-built native fallback: the WebdriverIO embedded provider, debug/test-only.

Assessed crate (source read, v1.5.1, 2026-10-24, MIT, github.com/webdriverio/desktop-mobile):
`tauri-plugin-wdio-webdriver` = W3C WebDriver HTTP server inside the app, bound to **127.0.0.1 only**,
unauthenticated (any local process can drive it). It executes in the real WKWebView via
`callAsyncJavaScript`/`evaluateJavaScript` (page world). It needs **no Tauri IPC capability**: default permissions are
empty, and remote-origin pages need no IPC. Screenshots use `WKWebView takeSnapshot`. Key input is synthetic DOM
KeyboardEvent/InputEvent (honest label: WKWebView DOM input, not OS keystrokes). README: "Never include it in
production builds."

## Requested change (app-owned files: src-tauri/Cargo.toml, main.rs, desktop/Cargo.lock)
```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-wdio-webdriver = { version = "=1.5.1", optional = true }
[features]
qa-webdriver = ["dep:tauri-plugin-wdio-webdriver"]   # NEVER in default / release bundles
```
```rust
// main.rs, inside builder chain (before .run):
#[cfg(feature = "qa-webdriver")]
let builder = match std::env::var("LOOM_DESKTOP_QA_WEBDRIVER_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
    Some(port) => builder.plugin(tauri_plugin_wdio_webdriver::init_with_port(port)),
    None => builder,   // feature compiled but server only when explicitly requested
};
```
- Must stay compatible with your navigation policy (the plugin doesn't navigate, it only evaluates JS/snapshots).
- No capability/config changes; no `withGlobalTauri`; no remote IPC.
- QA builds it with `desktop/scripts/build-app.sh --debug --features qa-webdriver --out /tmp/loom-tauri-build/qa/Loom-QA.app`
  (separate copy, labelled QA; never copied to desktop/dist). QA verifies the release/dist bundle does NOT contain the
  plugin (`strings | grep -c wdio-webdriver` == 0).
Please reply in app/STATUS.md (or ROOT) when it is in; QA owns the WebDriver client/tests (desktop/scripts/qa-webdriver/).
