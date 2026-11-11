# Result envelope (`--json`)

Type: `loom_proto::envelope::Envelope` (contract 03 §3.2). One JSON object per command on stdout.

```json
{"schema_version":1, "ok":true, "cmd":"echo.send",
 "data":{"written_bytes":5, "state":"sent", "request_id":"w12"},
 "meta":{"truncated":false, "record_health":"ok", "env":"bench", "latch":false,
         "viewers":{"subscribed":1, "visible":1}, "stream_epoch":"1794366827", "visibility":"visible"},
 "warnings":[{"code":"no_viewer_attached"}],
 "others_since_last":{"count":"2", "origins":["user"], "by_channel":{"echo:loop":"1"}, "kinds":{"echo.input":"1","core.latch":"1"}},
 "error":{"code":"E_STOPPED", "message":"…", "hint":"…", "request_id":"w13", "detail":{…}}}
```

- Absent optional field = empty. Big integers (seq, ns, counts) are **decimal strings**.
- `ok:false` ⇒ `error` is set; `data` may still be present (e.g. `E_PARTIAL` keeps the result).
- `others_since_last` is on **every** result, errors too: what the user, scripts, other agents,
  STOP or policy changes did since your previous call (your own events and device RX are not
  counted). `count > 0` ⇒ look with `loom events --since @last`.
- `meta.env`: `sim | bench | vehicle` (`vehicle` also warns `env_vehicle`). `meta.latch` = STOP.
- `meta.record_health`: `ok | degraded | failed` (failed ⇒ your writes are refused, exit 4).
- `meta.visibility`: `visible | not_visible | no_viewer`.
- Writes: `data.request_id` + `data.state` (`sent`, `confirmed`, …). The same request is visible
  as rows `requested → accepted → sent …` (`payload.write_state`, last row `payload.final:true`).
- Warning codes: `no_viewer_attached`, `no_visible_viewer`, `daemon_autostarted`,
  `record_degraded`, `env_vehicle`, `daemon_tied_to_parent_job`.
- Reads that hit a cap set `truncated` and `next_cursor` — continue with `--since <next_cursor>`.
