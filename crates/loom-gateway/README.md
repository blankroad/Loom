# loom-gateway (`loomd`)

`loomd` (or `loomd run`) is the real daemon (hub, auth, dispatcher, write pipeline, STOP, audit, journal, workers). `loomd --fixture-replay` is a contract v1 WebSocket server fed by generated events, for UI development (**no auth, 127.0.0.1 only**).

```sh
# from the workspace root
cargo run -p loom-gateway --bin loomd -- --fixture-replay builtin --port 4790 \
  --scenario all --echo-writes --allow-origin http://localhost:5173
# Vite dev proxy: /rpc → ws://localhost:4790/rpc   (origin http://localhost:4790 is always allowed)
```

- `--scenario normal|highload|gaps|late|saturate|all` (`all` = normal + gaps + 1,000-event late-join backlog), `--speed 2.0` runs generators twice as fast.
- `--fixture-replay <dir>` loops the `event_*.json` files of a directory instead (e.g. `crates/loom-proto/tests/fixtures`).
- `--echo-writes`: `uart.send`, `uart.input`, `can.send`, `uds.request` produce `requested→accepted→sent→confirmed` rows (origin `user`, `quality:["simulated"]`) **before** the reply; other domain methods answer `E_NOT_IMPLEMENTED`. `core.stop`/`core.stop.release` publish `core.latch`; `core.safe_state.list/run` return two fixed items.
- `--rights watch|operate|approve` (default operate). Priority events arrive as `core.events{sub:"control"}`; heartbeat every 1 s; `core.time` returns d1/d2.

## Adding a domain's worker (IPC by default, `[workers] mode = "inproc"` for tests)

1. In the domain crate, implement `loom_worker_sdk::Worker` (see `loom-echo/src/worker.rs`) and register a `WorkerSpec{kind, …}` in `DomainModule::register`.
2. In `src/domains.rs` `builtin()`, add an in-process constructor and pair it with the kind: `decls(&registry, &[(KIND, inproc_fn, None)])` (`Some(path)` = separate executable such as `loom-eth-worker`).
3. In `worker_main()`, add `KIND => loom_worker_sdk::run_worker(YourWorker::default())` — the gateway spawns `loomd worker <kind> --instance … --producer-epoch … --sock-dir … --journal-dir …`.
4. The worker writes its own journal into the daemon's session (`records/<date>/s<epoch>/<instance>/<producer_epoch>/`); the gateway only publishes its events and supervises it (restart, quarantine, `core.worker_state`).
5. Put the domain behind a cargo feature in `crates/loom-gateway/Cargo.toml` like `echo`.

## Subscription defaults (UI C-9·C-10)

- `snapshot:true` → `core.snapshot_part{n,total}`… `core.snapshot_end` after the RPC response (state items incl. `core.snapshot` and `core.channels`, then backlog events).
- `snapshot:false` without `after_seq` → live events only, **no** snapshot frames. With `after_seq` + `stream_epoch` → resume (ring) or a snapshot with `display.gap{resume_out_of_ring}` / `core.epoch_changed`.
- `backlog` is optional: default `max_events = 1000` (newest matching events in the ring, oldest first); `max_bytes`/`max_age_ms` are accepted and not enforced in M1.
