# loom-eth-capture (P0)

Core-independent capture building blocks. The default build needs no native capture library or device permissions.

- `CaptureSource` provides interface metadata, timeout-bounded reads, optional transmit, cumulative drop statistics and timestamp source.
- `ReplaySource<R: Read + Seek + Send + Debug>` streams pcapng or classic pcap. `ReplaySpeed::Max` runs without pacing; `Realtime { factor }` multiplies original time intervals (2.0 is twice as slow). `ReplayOptions::repeat` rewinds without accumulating the file. Empty repeat sources terminate. Frames retain per-packet interface and section identity.
- `SimTapSource` and its cloneable `SimTapHandle` accept TCP payloads and UDP datagrams. `Direction::Tx` means client to server, `Rx` means server to client. SYN and FIN each consume one sequence number. After both FINs, a final empty `push_tcp` ACK retires the connection; `forget_connection` explicitly removes abandoned state. Rejected queue writes do not advance sequence state. Sim data is marked `TimestampSource::Sim`.
- `CaptureLoop::start` owns a source on a thread. Clone `queue()` for the consumer and `stop_handle()` to interrupt pacing/poll waits. Call `join()` to observe errors and final accounting. `restart()` stops/joins the previous thread, continues source sequence and monotonic origin, and emits a CaptureRestart gap.

The raw queue defaults to 128 MiB / 512,000 frames; `QueueConfig` may lower these limits. Byte accounting includes the reserved pointer ring, frame objects, and owned vector/string capacities. The gap deque separately holds at most 1,024 reports. Overflow coalesces reports into a Mixed/Coalesced summary; its sequence range bounds the lost records and may include successful frames, while `counts.record` is the exact dropped-record count. Never treat that bounding range as a contiguous loss when `coalesced` is true.

Stats are sampled at startup, every 100 ms even without traffic, and at shutdown. Last known counters survive unavailable intervals to avoid double counting on recovery. Counter decreases produce an unknown CaptureRestart gap. StatsUnavailable means loss cannot be established, not that zero packets were lost. It is emitted on entering/changing an unknown state rather than repeatedly flooding the gap channel.

`CaptureSource::next` implementations must honor their requested timeout. Built-in waits share an interruptible stop signal. Arbitrary user-supplied blocking `Read`/`CaptureSource` implementations cannot be forcibly interrupted by safe Rust; use ordinary local capture files and honor the source contract.

Enable `live` to compile the optional `pcap =2.5.0` backend. Settings are applied before activation. Because the crate's safe precision setter discards errors and exposes no precision getter, this implementation writes a 24-byte global pcap header from the same active handle in a private temporary directory, reads its magic/snaplen, and cleans it up. Nano rejection or an observed micro resolution triggers an explicit micro reopen. Unknown precision fails closed. Native buffer size is a requested value, not a measured allocation. This path has only been compiled and linted on this Mac; no device was opened. The live-open test is ignored and requires an explicit `LOOM_TEST_INTERFACE` plus human-configured permissions. Native driver stats are conservatively Partial; a zero interface-drop value is represented as unknown.

From this crate directory, every Cargo command uses these caches:

```sh
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo fmt --check
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo clippy --all-targets -- -D warnings
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo test
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo check --features live
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo clippy --all-targets --features live -- -D warnings
CARGO_HOME="$HOME/.cache/loom-eth-cargo" CARGO_TARGET_DIR="$HOME/.cache/loom-target/eth" cargo run --release --example throughput
```

`throughput.stdout.txt` is the final local measurement: 200,000 records per frame size, virtual classic pcap input retaining one packet template, two threads, an 8 MiB / 4,096 frame queue, complete consumer/drop accounting. It excludes NIC capture, protocol decoding, journaling and UI. Short burst results and observed record drops do not establish sustained no-loss throughput.
