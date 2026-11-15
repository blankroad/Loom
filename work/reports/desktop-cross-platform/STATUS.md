# Windows / Ubuntu desktop build

State: IMPLEMENTED — local integration accepted; Windows native CI execution pending. ROOT orchestrates; foreground Opus 5.5 runtime (w2X), packaging (w2Y), CI (w2Z) owned disjoint scopes. All three returned, normal `/exit` → `agent_not_found` verified, and only those three workspaces were closed. MAIN w8 preserved. User requested Windows and Ubuntu builds in addition to macOS, with the same shared UI.

Baseline: a8253c13de7a298f20054037a8fbf8b10ccf7db6 plus existing uncommitted Bench Light work. `preexisting-desktop.diff` records pre-task tracked desktop/workflow changes; do not overwrite or indiscriminately stage unrelated work.

Native platform proof: macOS host available; Docker Linux/aarch64 available. Windows native runner execution not yet available locally. No remote push/dispatch performed. Existing final2 macOS visible acceptance remains pending locked-screen state, and its fingerprints describe the prior build only. Cross-platform runtime/build changes require a fresh matching build before accepting the new application.

Task files and worker returns: TASK-runtime.md → RUNTIME.md; TASK-packaging.md → PACKAGING.md; TASK-ci.md → CI.md. ROOT owns integration/status/Git. No worker may spawn subagents or alter the index.

Primary references: https://v2.tauri.app/start/prerequisites/ ; https://v2.tauri.app/develop/sidecar/ ; https://v2.tauri.app/distribute/debian/ ; https://v2.tauri.app/distribute/appimage/ .

ROOT review accepted runtime RUNTIME.md: real cargo rc=0 for macOS 26 desktop unit tests + 4 host unit / 23 lifecycle tests, desktop release check, macOS and Windows GNU clippy, Windows GNU compile and test executable linking. Windows tests were NOT executed. The gated real_loomd test did not run an actual daemon in that worker's test invocation; do not count it as new integration proof. Normal `/exit` → agent_not_found verified, only own w2X closed.

Native Ubuntu 22.04/aarch64 root backend `cargo check --locked -p loom-gateway --bin loomd`: PASS exit0; `linux-root-check.log`. Windows default-feature daemon release executable and all-targets workspace cross check: PASS, exact logs/rc in CI worker return. Initial Windows failure was video's Unix APIs; CI owner implements explicit non-Unix unsupported open and media503 while preserving Unix behavior. An unrelated existing loom-console clippy warning is outside task scope.

First Ubuntu full build produced both `.deb` and AppImage. `.deb` sidecar exact SHA256 check PASS. AppImage strict check FAIL because linuxdeploy rewrites ELF content (RUNPATH/layout); no identity waiver. ROOT narrowed supported Ubuntu packaging to `.deb` (the user did not require AppImage). AppImage remains experimental/unverified groundwork, not a supported result. Packaging defaults/tests were updated, and the matching final-source `.deb` pipeline plus installation/start/survival smoke passed. First failed build evidence is preserved.

## Final integration evidence

- `npm --prefix desktop run dist` is the common native build command. Windows → NSIS; Ubuntu → deb; macOS → app. `.github/workflows/desktop.yml` configures all three native runners, locked installs/builds, strict embedded UI/package identity checks and artifact upload.
- Packaging: 23 portable tests passed on macOS and Ubuntu; actual Mac debug bundle passed strict legacy verification. Original Mac final2 staging and `desktop/dist/Loom.app` were restored/preserved; no claim that this original artifact contains the new runtime source.
- Final Ubuntu 22.04/aarch64: release deb built, exact staged sidecar and installed sidecar verified, UI 6/6 bytes + CSP verified, desktop unit/lifecycle tests passed (26+4+23; the gated real-daemon test was skipped), deb installed. Normal unprivileged Xvfb app launch started `/usr/bin/loomd`, `/ui/` returned 200, app remained alive and daemon survived app termination. This is lifecycle smoke, not full interactive UI or hardware acceptance.
- Build snapshot matched 923 files at completion. Later drift is **only desktop/README.md**, a documented prose correction; binaries were not rebuilt for that documentation-only change. Original snapshot and comparison are retained in `evidence/ubuntu-final/`.
- Deliverable: `desktop/dist/linux-arm64/Loom_0.1.0_arm64.deb`, 12,750,150 bytes; SHA256 **9b28d36c61f9f3286dfd10676a3b705fab76f41156a995d817b41c96192884c0**, independently checked by ROOT against `HOST-MAP.json`. Container manifest retained unchanged as `artifacts.container.json`.
- Windows GNU: default-feature release loomd.exe, workspace all-targets check, scoped video/logic clippy and test executable linking passed. Desktop Windows cfg check/clippy/test linking passed. **No Windows tests, MSVC installer, or Windows app launch ran locally.** Native workflow execution is pending; Ubuntu x64 is likewise configured, not locally executed.
- Unix backend video/logic tests and scoped clippy: rc0, `evidence/host-unix-tests/`. ROOT Actions validation: official actionlint 1.7.12, `actionlint -shellcheck= .github/workflows/desktop.yml`, rc0. Shellcheck not run. `git diff --check` clean. Credential pattern scan of new reports/logs: 0 possible real tokens.
- Windows video capture open explicitly returns Unsupported and live media routes return 503 until its Unix media transport is ported. No module silently omitted from the default build. AppImage is rejected by the supported packaging CLI rather than reported verified.

Git: existing mixed uncommitted Bench Light/desktop work preserved, index untouched, no commit/push/remote dispatch in this task. Broad macOS visible acceptance remains pending unlock and a fresh matching Mac build; see the ROOT update in `../tauri-desktop/qa/RESUME-FINAL2.md`. All limitations above are explicit rather than promoted to a pass.

Cleanup: no own Docker containers remain (`--rm`); pre-existing `duet-remote-opus-chaos` / `duet-remote-opus-ssh` were untouched. `desktop/target` absent. Reusable build caches/toolchain image and output evidence retained for subsequent platform work. No user daemon, browser space or OS setting was changed. New cross-platform source is handed back to ROOT; model usage/billing telemetry was not provided.
