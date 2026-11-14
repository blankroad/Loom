# Live UDS logs while requests run

Read this when you must watch UDS traffic as it happens (another agent, a test, or the user
sending requests) or prove what the ECU answered. Installed `loom help --json` / `loom uds help
--json` win over this page.

## Observer
```
LOOM_CLIENT=uds-observer loom --json events --follow --timeout 120 \
  --since now --domain uds --channel <target> [--kind uds.tx --kind uds.rx --kind uds.result --kind uds.state] [--max 200]
```
- Output is JSON Lines: one envelope per poll that returned events, plus the first poll even
  when empty (it carries `warnings`, `stream_epoch`, `stream_seq_hw`). Polls ~200 ms; a capped
  page (`data.truncated`) is continued at once from `data.next_cursor`, so `--max` bounds each
  line, not the total. Lines are flushed as printed.
- `--timeout` is the whole follow (capped at 600 s). Exit 0 at the timeout, even with no events.
- Run the observer under its own `LOOM_CLIENT` so it never moves the sender's `@last` /
  `others_since_last`; add `--peek` if it must not move its own `@last` either.
- `channel` = UDS target name. Seq numbers have holes under a filter (other domains, `core.*`): a
  hole is not loss.

## Correlating one request
Rows of one admitted request share the top-level `request_id` (e.g. `w1`); correlate whichever
rows exist. Not every request has every row: a refused or offline request may have no `uds.tx`;
a suppressed positive response or a timeout has no `uds.rx`; NRC 0x78 pending gives several.
- `uds.tx` — bytes sent, `risk`, `service`, `payload.write_state:"sent"`. Alone it proves nothing.
- `uds.rx` (`origin:"device"`) — `payload.kind` `positive|negative`, `nrc`/`nrc_name`, `late`.
  `late:true` arrived after the request ended: never a result.
- `uds.result` (`payload.final:true`) — `write_state` (`confirmed`/`failed`/`sent`+`ok_suppressed`/
  `outcome_unknown`), `rtt_ms`, `nrc_name`, `timeout`. The outcome is this terminal row (or the
  command's own result), never the presence or absence of an RX.
- `uds.state` — link `up|down` + `cause`, session/security `confidence` (`stale` after a loss).
Evidence REF = `uds:<target>@e<epoch>:seq:<seq>` from the returned event's own `epoch`/`seq`.
De-duplicate by (`epoch`, `seq`), never by payload.

## Resume after the observer stops
- From the last printed line: `data.next_cursor` if `data.truncated`, else
  `--since e<stream_epoch>:seq:<max(range.to, stream_seq_hw)+1>`. Seq cursors are inclusive.
  This re-reads nothing already printed and skips nothing.
- Or reuse the same `LOOM_CLIENT` without `--peek` and `--since @last` (kept in daemon memory
  only; lost when the daemon restarts).
- Do not resume with `--since now` — it drops whatever happened while you were away.

## Failures
- Exit 8 `E_CURSOR_GAP`: the cursor is older than the ring and the recording (or another epoch
  that was not recorded). Report the gap; restart from `now` only after saying events may be
  missing. `E_SCAN_LIMIT`: continue from the given cursor/`resume_from`. A result with
  `unreadable` ranges names holes the recording could not supply.
- A different `stream_epoch` = the daemon restarted; seqs restart. An earlier-epoch cursor
  reads that epoch's recording only — start a new follow for the current epoch.
- Exit 9 / `E_DAEMON` mid-follow: lines already printed stay valid; resume as above.
- ECU/server loss shows as `uds.state` link `down` (e.g. cause `early eof`); later requests fail
  (exit 7). Do not reconnect unless the user asked; with `--no-reconnect` it stays down.
- Human mode prints events on stdout and warnings on stderr; use `--json` for parsing.

## What is (not) visible
- `warnings: no_viewer_attached` / `meta.visibility:"no_viewer"`: no human UI is subscribed.
  For an already authorized isolated SIM/bench check, a JSON observer is enough; mention it, no
  approval loop. A native observer is not proof that a human watched.
- Only `loomd` traffic appears here. A browser workbench running its local simulation (or any
  UI without a loomd backend) shows none of these rows, and its rows are not evidence.
