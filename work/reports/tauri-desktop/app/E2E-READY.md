# app → QA: `e2e` WebDriver feature is in (answers qa/REQUEST-APP-QA-WEBDRIVER.md)

- Feature `e2e`, alias `qa-webdriver` (so `build-app.sh --debug --features qa-webdriver` works unchanged). Dependency `tauri-plugin-wdio-webdriver = "=1.5.1"` (optional; already in desktop/Cargo.lock).
- Debug only: `--features e2e` on a release build is a `compile_error!`. Never copy an e2e build to desktop/dist.
- The server starts only when `LOOM_DESKTOP_QA_WEBDRIVER_PORT` (preferred) or `TAURI_WEBDRIVER_PORT` is set. It binds 127.0.0.1:<port> (crate source `server/mod.rs`). On start it logs `loom-desktop: e2e WebDriver server on 127.0.0.1:<port> (test build)`.
- Smoke done by app owner (debug exe, port 4471, isolated home): `/status` ready. Session, title, url and execute/sync + execute/async all work. Results are in STATUS.md item 8.
- The dist bundle contains 0 `wdio` strings (re-check with `strings … | grep -c wdio`).
- Note: `/tmp/loom-tauri-build/debug/loom-desktop` is currently the e2e build.

## Update 07:50: new e2e knob + RX finding (rebuild your QA e2e app)
- New (e2e debug builds only): `LOOM_DESKTOP_QA_ON_TOP=1` keeps the main window always-on-top, so occlusion by other windows does not hide the page. It does not take focus.
- **The session is currently LOCKED** (CGSSessionScreenIsLocked=1, display asleep since 07:18:32). WebKit then hides every page and stops rAF, so the trace and terminal cannot draw ("수신 없음"/blank) whatever the window order. Run the RX/typing UI checks only after the user unlocks. Check first with `document.visibilityState=="visible"` via WebDriver.
- UI presence fix (`ui/src/shell/services.ts`): a page loaded hidden now reports `visible:false`. To test it before re-staging, build the UI to a separate dir (`npx vite build --outDir <dir>`) and launch with `LOOM_DESKTOP_UI_DIR=<dir>`. This takes effect only when the app starts the daemon (the host passes `--ui-dir`).
- Typing-test note from your log: `arguments[0].focus is not a function`. The element reference was not passed as a W3C element object (`{"element-6066-11e4-a52f-4a6b8cb7cd10": id}`) in execute args.
