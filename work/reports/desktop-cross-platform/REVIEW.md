# ROOT acceptance checklist

- [x] One shared UI source; fresh embedded assets staged with exact index identity. No debug WebDriver in shipped bundle configuration.
- [x] Portable npm commands remove POSIX-only environment syntax and macOS file-copy assumptions from Windows builds. Legacy explicit mac scripts remain available.
- [x] Windows `.exe`, target triple, ICO, HOME/USERPROFILE and native opener paths implemented; GNU compile/link verified. MSVC/NSIS runner configured; actual native execution pending.
- [x] Ubuntu 22.04/aarch64 deb built/installed, exact sidecar/UI bytes and startup/survival smoke passed. AppImage failed strict proof and is rejected as unsupported; x64 execution remains a CI step.
- [x] macOS debug app build passed strict signature/code/embedded-UI verification. Original Mac staging and deliverable preserved.
- [x] Lifecycle/origin/launcher IPC/download/UI identity policies preserved and tests reviewed. Native external openers receive one safe URL argument.
- [x] CI honors locked workspaces, Node/pnpm pins and checked artifact manifests. Actions validation passed. Remote CI/upload not run yet.
- [x] Relevant runtime/backend/packaging tests passed. Unchanged UI digest 643e5952…; 686 UI tests were not repeated. Locked-screen native visible acceptance remains pending.
- [x] All foreground workers returned, normal `/exit` verified via `agent_not_found`, own workspaces closed. Index empty, unrelated work preserved.

Windows native installer/UI cannot be declared validated from a GNU cross-check. Linux container checks do not prove physical capture hardware or visible desktop interaction. Existing macOS final2 evidence remains separated from any new build.
