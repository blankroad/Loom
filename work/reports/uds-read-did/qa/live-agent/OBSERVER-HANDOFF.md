# OBSERVER HANDOFF — uds-live-observer-opus (w1M) / FINAL: UI phase ACCEPTED by ROOT, stopped, waiting normal exit

Verification only. No implementation/A/ROOT file edits, no ECU requests, no mark/connect/disconnect/
server control, no skill installs, no Git, no child/background tasks. Own files: this file +
`observer-*.jsonl|.json|.txt` in this directory.

## Pre-BEGIN read-only checks (2026-11-14, no daemon RPC sent, no follow started)
- Skills read: `skills/loom-uds/SKILL.md`, `skills/loom-uds/references/live-logs.md`, `skills/loom/SKILL.md`
  (`.claude/skills/loom-uds` byte-identical to `skills/loom-uds`).
- CLI `/tmp/loom-uds-live-build/debug/loom` sha256 `d54e4134f10b84fe3ce4c1e5e0e01cdf677cd317677b1c22c92143a6430a3689`
  = A's final default-features build (HANDOFF.md). `events --help` (local clap only) confirms
  `--follow --timeout --since --peek --max --domain --channel --kind`.
- `lsof`: loomd pid 95239 LISTEN 127.0.0.1:52575; a python3 LISTEN 127.0.0.1:52570 (assumed ROOT's
  external DoIP server; not touched). `LOOM_HOME=/tmp/loom-uds-live-20261114/home` exists.
- Target `dual` not verified (status would be an RPC); taken from ROOT's instruction.
- If ROOT notifies a different final binary path/sha, I swap `B=` and record its sha before BEGIN.

## Planned observation (runs only after ROOT says BEGIN; one bounded foreground run, ≤30 s)
```
cd /Users/ctmctm/workspace/Loom/work/reports/uds-read-did/qa/live-agent
export LOOM_HOME=/tmp/loom-uds-live-20261114/home LOOM_CLIENT=independent-uds-observer LOOM_NO_AUTOSTART=1
B=/tmp/loom-uds-live-build/debug/loom; shasum -a 256 $B > observer-cli.sha256.txt
( $B --json events --follow --timeout 30 --since now --domain uds --channel dual --peek --max 200 \
    2>observer-follow.stderr.txt; echo $? > observer-follow.exit.txt ) | python3 -I -u -c '<stamper>'
```
- No `--kind` filter: all uds rows on `dual` (tx/rx/result/state) are captured. `--peek` + own
  `LOOM_CLIENT` → sender's `@last`/`others_since_last` and my own `@last` untouched.
- Stamper (stdlib only, no sockets): for every stdout line, as it arrives while the follow runs:
  - appends the line verbatim to `observer-follow.jsonl` (the RAW NDJSON = primary evidence), flush;
  - appends `{line_no, recv_wall_ns, recv_mono_ns, bytes, count, stream_epoch, stream_seq_hw, range}`
    to `observer-follow.recv.jsonl`;
  - on the first line (initial poll, expected `count:0`) writes `observer-ready.json`
    `{ready:true, recv_wall_ns, stream_epoch, stream_seq_hw, pid}` = **ready-before-request signal**.
    ROOT starts the request phase only after this file exists (poll it or I'll say READY in pane);
    every row with `seq > stream_seq_hw` in it is provably new, not history.
- After exit (no renewal without ROOT instruction): offline read of my own jsonl only →
  `observer-correlation.json`: per `request_id` the rows that exist (`kind`, `epoch`, `seq`,
  `ts.wall_ns`, `recv_wall_ns`, `payload.bytes.data`, `service`/`risk`, `write_state`, `nrc`/`nrc_name`,
  `late`, `rtt_ms`, `uds.state` link/cause), REF `uds:dual@e<epoch>:seq:<seq>`, dedupe by
  (epoch, seq), flags `seq_gt_ready_hw`, `received_while_follow_running`, exit code, stderr, warnings
  (`no_viewer_attached` / `meta.viewers` reported as-is, not proof a human watched).
- Resume cursor recorded (not used unless ROOT asks): `data.next_cursor` if last line truncated,
  else `e<stream_epoch>:seq:<max(range.to, stream_seq_hw)+1>`.

## Acceptance I will report against
A request ROOT sends after `observer-ready.json` must appear in `observer-follow.jsonl` with
`recv_wall_ns` before the follow exited, `seq > ready hw`, and the same request_id/epoch/seq/bytes
that ROOT's UI DOM crosscheck shows. Missing rows → reported as missing/inconclusive, never filled
from history (`--since seq:0`) or DOM. Exit 8/9, epoch change, `link down` reported as-is.

## Run 1 result (ROOT BEGIN; binary unchanged d54e4134…3a689, target dual; one 30 s run, not renewed)
- Window 2026-11-14 08:45:16 → 08:45:46 KST. exit `0` (timeout). stderr EMPTY (0 bytes): in `--json`
  mode `no_viewer_attached` is NOT on stderr; it is in every envelope `warnings`, with
  `meta.viewers {subscribed:0, visible:0}` and `meta.visibility:"no_viewer"` on both lines (reported as-is).
- Ready: line 1 `count:0`, `stream_epoch 1794612979`, `stream_seq_hw 131` → `observer-ready.json`
  (recv_wall_ns 1794613516582772000).
- Line 2 received +11.915 s after ready, ~18 s before the follow exited (live pipe, not post-exit):
  `count 6`, `range 146..155`, hw 155, not truncated. Holes 132–145, 149–152 = other channels/domains under filter.
- All 6 rows have seq > ready hw 131; 0 duplicates by (epoch, seq). Event ts → my receive: 133–151 ms.

| request_id | kind | REF | bytes | outcome |
|---|---|---|---|---|
| w15 | uds.tx | uds:dual@e1794612979:seq:146 | 22f190 | ReadDataByIdentifier R0, sent |
| w15 | uds.rx (device) | uds:dual@e1794612979:seq:147 | 62f1904c4f4f4d494e5445524f5030303030303031 | positive, late:false |
| w15 | uds.result | uds:dual@e1794612979:seq:148 | 22f190 | confirmed, rtt 1.888 ms |
| w16 | uds.tx | uds:dual@e1794612979:seq:153 | 22ffff | ReadDataByIdentifier R0, sent |
| w16 | uds.rx (device) | uds:dual@e1794612979:seq:154 | 7f2231 | negative NRC 0x31 requestOutOfRange, late:false |
| w16 | uds.result | uds:dual@e1794612979:seq:155 | 22ffff | failed NRC 0x31 requestOutOfRange, rtt 0.465 ms |

- Senders: `client_id` dual-sender#2951 (w15), dual-sender#2952 (w16), origin agent (tx/result) / device (rx).
  No uds.state row arrived in the window. Event wall ts: w15 tx +11.764 s, w16 tx +11.782 s after ready.
- Resume cursor (not used): `e1794612979:seq:156`.
- Files: RAW `observer-follow.jsonl` (sha256 `3fe81103a804e011d9a396052e5b74019582e3a51a3a2df5756118d347d7bc49`,
  2 lines), `observer-follow.recv.jsonl`, `observer-ready.json`, `observer-correlation.json`,
  `observer-follow.stderr.txt` (empty), `observer-follow.exit.txt`, `observer-cli.sha256.txt`, `observer-start.txt`/`-end.txt`.

## Verdict
- CLI-observer side PASS: new requests w15/w16 sent after ready appeared in the running observer with
  real epoch/seq/bytes/request_id; not from history, not from DOM.
- UI side NOT judged by me: ROOT reports the UI had no auth → UI receive FAIL/pending. Overall acceptance
  ("observer AND same backend UI") stays OPEN until ROOT gets a simultaneous UI arrival after auth and
  crosschecks these request_ids/seqs. No viewer was subscribed during this window (meta.viewers 0/0).
- Did not read `dual-connect.json` or any ROOT/A file; no ECU request, no mark, no Git.

## Run 2 — BEGIN_UI (viewer attached) — separate files, run-1 files untouched
- Binary `/Users/ctmctm/workspace/Loom/target/debug/loom` sha256 d54e4134…3a689 (checked before start; abort-on-mismatch).
  Same command/env as run 1 (`--since now --domain uds --channel dual --peek --max 200 --follow --timeout 30`).
- Window 08:54:00 → 08:54:30 KST, exit `0`, stderr EMPTY. Both lines: `meta.viewers {subscribed:1, visible:1}`,
  `meta.visibility:"visible"`, `warnings` absent (no `no_viewer_attached` this time).
- Ready: line 1 `count:0`, epoch 1794612979, `stream_seq_hw 514` → `observer-ui-ready.json`
  (recv_wall_ns in file; `observer-ready.json` from run 1 kept as is, 08:45).
- Line 2 received +12.110 s after ready, during the run: `count 6`, range 531..538, hw 538, not truncated.
  All seq > 514, 0 duplicates. Event ts → my receive 169–186 ms.

| request_id | kind | REF | bytes | outcome |
|---|---|---|---|---|
| w24 | uds.tx | uds:dual@e1794612979:seq:531 | 22f190 | R0 ReadDataByIdentifier, sent |
| w24 | uds.rx (device) | uds:dual@e1794612979:seq:532 | 62f1904c4f4f4d494e5445524f5030303030303031 | positive, late:false |
| w24 | uds.result | uds:dual@e1794612979:seq:533 | 22f190 | confirmed, rtt 2.459 ms |
| w25 | uds.tx | uds:dual@e1794612979:seq:536 | 22ffff | R0, sent |
| w25 | uds.rx (device) | uds:dual@e1794612979:seq:537 | 7f2231 | negative NRC 0x31 requestOutOfRange, late:false |
| w25 | uds.result | uds:dual@e1794612979:seq:538 | 22ffff | failed NRC 0x31 requestOutOfRange, rtt 0.686 ms |

- Senders `dual-ui-sender#9831` (w24), `#9832` (w25). No uds.state row in window. Resume cursor (unused) `e1794612979:seq:539`.
- Files: RAW `observer-ui-follow.jsonl` (sha256 `842cdfd2e8cc424cab51795dac3b25dfcd08eb2dc1564d13d619d5db611c0aa6`, 2 lines),
  `observer-ui-follow.recv.jsonl`, `observer-ui-ready.json`, `observer-ui-correlation.json` (per-row ts.wall_ns for UI compare),
  `observer-ui-follow.stderr.txt` (empty), `observer-ui-follow.exit.txt`, `observer-ui-cli.sha256.txt`, `observer-ui-start/end.txt`.
- Verdict (observer side): PASS — w24/w25 appeared live with viewers 1/1 reported by the same daemon.
  The daemon's viewer count is not proof of what the UI rendered; UI row/PDU match at the same
  request_id/seq is ROOT's crosscheck.

## UI phase — ACCEPTED (ROOT crosscheck, ROOT-owned evidence; not produced or re-verified by me)
- ROOT `dual-UIcompare.json` (this directory): machine PASS — w24/w25, 6 events (tx/rx/result each)
  match this observer's exact epoch/seq (e1794612979 seq 531–533, 536–538) and the actual UI DOM shows
  the same request_id + response hex (VIN `62F190…`, NRC `7F2231`), send meta viewers 1/1.
- ROOT screenshots `uds-ui-vin-detail.png`, `uds-ui-nrc.png` (VIN visually inspected by ROOT).
- ROOT conclusion: real UI watch connection to loomd 52575, not a mock. My side: the observer
  `observer-ui-follow.jsonl` is the independent CLI half; I did not read ROOT files, UI DOM, source RPC or auth.
- Run 1 (no viewer) stays as CLI-only PASS with UI FAIL/pending at that time; superseded for the UI axis by run 2.

## Status
FINAL. Two bounded runs total, no follow running, no further follow. Waiting for ROOT normal exit.
