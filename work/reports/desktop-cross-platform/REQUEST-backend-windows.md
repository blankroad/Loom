# REQUEST (CI → ROOT): backend Windows compile blockers

From: CI worker. To: ROOT (assign to the backend/video owner). CI worker has **not** touched any backend source.

## Evidence (supplemental cross-compile, not native MSVC)

Host macOS aarch64, rustc 1.93.1, target `x86_64-pc-windows-gnu`, linker `/opt/homebrew/bin/x86_64-w64-mingw32-gcc`,
isolated `CARGO_TARGET_DIR=/tmp/loom-cross-windows` (main `target/` untouched).

| Command | Result | Log |
|---|---|---|
| `cargo check --locked -p loom-gateway --bin loomd --target x86_64-pc-windows-gnu` | **FAIL** exit 101, `loom-video` (lib): 21 errors | `/tmp/loom-cross-windows/check-keepgoing.log` |
| same, `--no-default-features --features echo,uart,can,uds,eth,psu,logic,test` (video off) | **PASS** exit 0 | `/tmp/loom-cross-windows/check-novideo.log` |
| `cargo check --locked --keep-going --workspace --all-targets --target x86_64-pc-windows-gnu` | FAIL: `loom-video` lib + lib test (21), `loom-logic` test `engine` (2). Crates depending on `loom-video` (gateway, …) were not reached | `/tmp/loom-cross-windows/check-workspace.log` |

So `loomd` with default features **cannot be built for Windows today**. That is the only blocker found for the
Windows desktop sidecar. The existing `.github/workflows/ci.yml` `windows-latest` job (`cargo clippy/test --workspace`)
will fail for the same reason.

## Blockers

1. `crates/loom-video/src/worker.rs` + `media.rs`: the frame media side channel is a Unix domain socket used without
   any `cfg(unix)` guard. Specifically: `std::os::unix::*` (lines 9, 145, 469, 724, 769, 897, 912),
   `UnixListener`/`tokio::net::UnixStream` (media.rs:74), `Command::process_group` (146),
   `OpenOptions::mode` (728, 773), `Permissions::from_mode` (470, 913), `FileType::is_socket` (899).
2. `crates/loom-logic/tests/engine.rs:318-323`: the test fixture sets exec bits through `PermissionsExt` (test only;
   needs `#[cfg(unix)]` on that test or a Windows `.cmd`/`.exe` stub).

## Decision needed (backend owner, not CI)

- (a) **Minimal:** `cfg(unix)` the media server/client. On Windows, report `media_unavailable` (503) and keep
  capture/recording/snapshot paths that do not need the socket. Process group → `CREATE_NEW_PROCESS_GROUP` via
  `std::os::windows::process::CommandExt::creation_flags`, or no-op. File modes → no-op on Windows (per-user
  `%USERPROFILE%` ACLs).
- (b) **Full:** a portable transport (tokio named pipe `\\.\pipe\loom-video-<id>` or authenticated loopback TCP) with
  the same request/reply framing.

Note: AVFoundation capture is macOS-only anyway. On Windows/Linux only the loopback MJPEG + replay sources apply
until a dshow/v4l2 source exists, so (a) is enough for the first Windows desktop build.

Until this lands, `desktop.yml` builds Windows with **default features** (the honest target), so the Windows job
is expected to fail at the `loomd` step. ROOT may instead tell CI to build the Windows sidecar without `video` as a
temporary measure, but that would ship a Windows app without the video panel's backend. It needs explicit ROOT
approval, so CI does not do it by default.

Re-verify after the fix (CI worker can rerun on request):
`CARGO_TARGET_DIR=/tmp/loom-cross-windows CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo check --locked --workspace --all-targets --target x86_64-pc-windows-gnu`
