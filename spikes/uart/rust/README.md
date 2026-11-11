# UART Rust feasibility probe

Research-only project for `docs/plan/uart/tasks/v2-sol.md`; not a Loom backend.
It uses Rust only and targets Unix PTYs. Windows UART behavior is not tested.

From this directory:

```sh
CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target" cargo build --release
CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target" cargo run --release
```

No `cargo install` or global configuration is required. `Cargo.lock` pins the
probe. Cargo caches and build output stay in this directory and are ignored.

Recorded run: 2026-11-10, macOS 27.0.1 (26A434), arm64, UID 501;
rustc 1.93.1 (01f6ddf75 2026-02-11), cargo 1.93.1 (083ac5135 2025-12-15).
See `results.txt` for the observed values. These are one local release-build
run, not hardware support or maximum-latency guarantees.

The probe checks:

- nix `openpty`, raw slave mode, ordinary serialport native open, and explicit
  test-only baud-zero fallback on macOS (PTYs reject `IOSSIOSPEED`). Never use
  this fallback for a physical UART.
- Non-cooperating second open, DTR/RTS and break API results, finite empty read.
- Exact 00–FF round trip, 4 MiB master-to-slave content and FNV-1a64, throughput.
  The master stays alive until the slave finishes draining.
- Two anchored backtracking-pathological patterns using Rust regex at 1/16/64
  KiB, 100 searches each; compiled and DFA cache limits of 256 KiB each.
- Backreference rejection, Unicode expansion size rejection, binary memmem.

The downloaded upstream source excerpts in ignored `.research/` were inspected
for planning; public versioned source links and distinctions between confirmed
facts and proposed behavior are in `docs/plan/uart/v2-sol.md`.
