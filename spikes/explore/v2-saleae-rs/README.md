# Saleae Rust gRPC generation spike

`proto/saleae.proto` is copied unmodified from Saleae's public repository.
`upstream.json` records the exact commit, source URLs and SHA-256 checksums.
The upstream Apache-2.0 license is retained at `proto/LICENSE`.

`build.rs` generates only client bindings using `tonic-prost-build 0.14.9` and
the locally available protoc 29.6. `src/main.rs` type-checks a generated
GetAppInfo RPC and handles a refused connection to localhost:10430 without
panic, with bounded connection/outer deadlines. If a server is listening, the
no-app precondition fails and the spike exits nonzero without calling an RPC.
No device or Logic 2 app was used. Full capture compatibility was not tested.

From the repository root:

```sh
env CARGO_HOME="$PWD/spikes/explore/.cargo-home" CARGO_TARGET_DIR="$PWD/spikes/explore/v2-saleae-rs/target" cargo build --release --locked --manifest-path spikes/explore/v2-saleae-rs/Cargo.toml
./spikes/explore/v2-saleae-rs/target/release/loom-saleae-rs-spike
```

The generated source is under `target/release/build/*/out/`.
`cold-build.log`, `warm-build.log`, `run.log` and
`../v2-facts/spike-results.json` preserve the initial measurements.
