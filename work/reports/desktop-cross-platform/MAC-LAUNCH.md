# Current-source macOS launch

State DONE — current-source ordinary release built, strictly verified and visibly launched by ROOT. User-facing app and daemon intentionally remain running. ROOT owns visible app operation; foreground Opus 5.5 owned build/verification only. Screen unlocked/awake. Shared dirty source; no code edits/Git operations. Final2 evidence remains historical; Ubuntu artifact preserved.

Worker: run `npm --prefix desktop run dist` from repo (release daemon + release app, existing portable script handles lock/cache). Save direct log and actual rc here in sibling mac-current-build.log. Then run strict `desktop/scripts/verify-dist.sh desktop/dist/Loom.app --manifest work/reports/desktop-cross-platform/mac-current-manifest.json`, save log/rc. Do not re-run passed UI/tests or introduce fixes without reporting actual blocker. No UI launch, no CUA/Ego operation, no subagents/Agent/Task/background LLM. Build/cache local `/tmp/loom-tauri-build`, no desktop/target; no user daemons stopped, no settings changed. Update this file READY_FOR_REVIEW with hashes, exact results and any blocker. ROOT then launches user-facing ordinary app and inspects UI. No fixed token budget requested; keep task bounded to build+strict verification. ROOT will request normal /exit and close workspace after return.

## Worker result — READY_FOR_REVIEW (2026-11-15T06:48:52Z)

ROOT accepted build evidence below. Native CUA selected the exact `desktop/dist/Loom.app`, then raised/focused Loom. AX + screenshot show Bench Light, actual daemon viewer `localhost:59563/ui/#/l/m1-default`, `[BENCH] 연결됨`, viewer 1/1 approve, zero active interfaces and a truthful empty workspace. No interface/device was opened and no fake data added. Default user home `/Users/ctmctm/.loom` had no discovery before launch. The app started bundled daemon PID 52646 on port 59563; app PID 52634. These are the requested running deliverable, not disposable QA fixtures; leave them open for the user.

This confirms native launch/visible UI only, not the pending macro/split/export/keyboard-broadcast acceptance flows. The e2e app still needs a matching rebuild before those flows. Prior screen-lock blocker was cleared for this observation; no OS setting changed. Build worker returned and normal `/exit` → `agent_not_found` verified; own workspace w20 closed by ROOT.

| Step | Command | Actual rc | Log |
|---|---|---|---|
| Build | `npm --prefix desktop run dist` (release) | **0** (`mac-current-build.rc`) | `mac-current-build.log` (06:46:15Z → 06:48:41Z) |
| Strict verify | `desktop/scripts/verify-dist.sh desktop/dist/Loom.app --manifest work/reports/desktop-cross-platform/mac-current-manifest.json` | **0** (`mac-current-verify.rc`) | `mac-current-verify.log`, manifest `mac-current-manifest.json` (+ `.bundle.json`) |

Build: ui/dist rebuilt (pnpm) and loom-ui-assets cleaned to force re-embed; `cargo build --locked --release -p loom-gateway --bin loomd` from the current (dirty) working tree; Tauri release build in `CARGO_TARGET_DIR=/tmp/loom-tauri-build`; all bundle checks PASS; Cargo.lock unchanged (--locked).

Hashes:
- `desktop/dist/Loom.app` artifact sha256 `97ebd3d798190df023022656e928a36d340f5a9335903ed5df2300237de6198c` (`desktop/dist/artifacts.json`)
- main exe `loom-desktop` sha256 `3d74e8134cdfb1b60dadbdc3d0f7cfe26bb7faea3347aee588bf323cbee3b3d4`, LC_UUID `4B750976-D2F2-38FB-924F-EC91C57FE828`
- loomd staged (pre-sign) `e56e10e2484deaea2b555e71cba612f42ead433d542fcdfbf66c6cf558b795cb` = stamp; bundled (ad-hoc re-signed) `d4cfa7bb513d0196ea282f36fc69b163d84ac10aaa8edce3c4a810994399b24f`; LC_UUID `42293DB8-C5B6-3316-BC8A-398E8F5A3F5D` both; `__TEXT,__text` `77f9ed0b…9e4089` and `__TEXT,__const` `be938b61…9ee3885` equal → identity PASS
- ui_dist digest `643e595265ae92e70219cfadf0eff7c037948586547d4aedb8a26372996b0f05`; embedded /ui compared=6 mismatched=0, CSP yes
- Strict checks: codesign valid (adhoc, runtime flag, no TeamIdentifier), no QA WebDriver, no credentials, no home data. Not notarized (no Apple credentials — expected for a local build).

Linux artifact preserved, unchanged sha256 before/after:
- `linux-arm64/Loom_0.1.0_arm64.deb` `9b28d36c61f9f3286dfd10676a3b705fab76f41156a995d817b41c96192884c0`
- `linux-arm64/artifacts.container.json` `a80c84f46f7dfc8b47fccc05a523123f79d18ce8a11409cf0a49183851df738a`
- `linux-arm64/HOST-MAP.json` `ddc3b07bc48168a6e9ab98d58206f04eb5e8eecc1ccb291960ec4abc4ea60221`

Notes / no blockers: manifest `baseline a8253c1` is HEAD; the binary was built from the dirty working tree on top of it, not from a clean commit. The stage step built loomd in the workspace `target/` (by design of stage-loomd; not `desktop/target`). Rebuilt files: staged sidecar `desktop/src-tauri/binaries/loomd-aarch64-apple-darwin` + stamp, `ui/dist`. Not done (per task): no UI launch, no tests re-run, no code edits, no Git, no daemons stopped, no subagents. ROOT may launch `desktop/dist/Loom.app`.
