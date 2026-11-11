# Evidence predicate spike

50 fixed synthetic JSONL events mix UART text, CAN VehicleSpeed samples and UDS
responses. `src/main.rs` compares pinned `cel 0.15.0` and `serde_json_path 0.7.2`.
Each engine runs 10 assertion cases: positive/negative/inclusive-boundary exists,
positive/short/violated hold, gap, epoch, stale interval, and wrong payload type.
The actual query pair benchmark and diagnostics are in `run.log`.

The seq range is inclusive. Time uses monotonic milliseconds. A sampled hold
needs consecutive in-range samples, unchanged epoch and <=150 ms sample gaps.
Unrelated events do not break the signal interval, but source gaps do. A missing
or nonnumeric value is an error. This is not proof of physical continuity
between samples. Complete journal coverage is assumed only for this fixture;
a production verifier must check retention, source/record gaps and schema first.

From the repository root (no global Cargo installation or cache changes):

```sh
env CARGO_HOME="$PWD/spikes/explore/.cargo-home" CARGO_TARGET_DIR="$PWD/spikes/explore/v2-evidence/target" cargo build --release --locked --manifest-path spikes/explore/v2-evidence/Cargo.toml
./spikes/explore/v2-evidence/target/release/loom-evidence-spike
```

Initial build/process timings are recorded in `../v2-facts/spike-results.json`.
Build timings include dependency downloads and compilation; they are one-host
observations, not benchmarks of the language runtimes alone. The fixed fixture
is embedded at compile time; the spike binary itself requires no Python.
