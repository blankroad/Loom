# Shared target is not acceptable as final source provenance

ROOT full workspace test on shared target failed: loom-eth main source was compiled against cached loom-eth-decode whose rustc source location points to .worktrees/operator-can (old APIs). All foreground workers are now idle/stopped; root-only `cargo metadata` resolves main tree. Exact log: rust-next-workspace.log, exit101.

ROOT started a truly isolated target /tmp/loom-operator-root-check-610029e; compile/test log rust-isolated-workspace.log. No product edits or blanket cargo cache deletion.

Runtime clue: previously staged NEXT immutable binaries recognize RF1/B9 but eth.show lacks new tcp_rel and old large-journal eth.trace still returns the pre-C2 E_SCAN_LIMIT. Evidence eth/runtime/c2c3-root/03-old-epoch-show.json and 04-large-old-epoch-trace.json. This suggests cached pre-change ETH libraries in that prior staging; do not promote the NEXT daemon as verified C2 source. UI digest and binary SHA identify bytes, not correctness of Rust incremental dependency freshness. Other observed functional proofs remain tied to these exact binary bytes, not proof that every module's frozen Rust source was present.

Required next: finish isolated build/test; build CLI+loomd from same isolated target, stage without falling back to shared cached binaries, record hashes, then rerun actual C2. Investigate any remaining error before treating it as a source defect. Preserve old artifacts and reports.
