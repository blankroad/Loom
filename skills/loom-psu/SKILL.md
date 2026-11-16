---
name: loom-psu
description: Control and measure a bench power supply (voltage/current setpoints, output on/off, power cycle, sleep-current and inrush checks) through the Loom daemon while the user watches the live gauge in the Loom UI and can hit emergency OFF. Use for powering the DUT, waiting for current/voltage conditions, journaled history stats (overnight sleep current), and as power steps of Loom integration tests. Backends: simulated supply, or an external SCPI endpoint on loopback TCP.
---
# loom-psu

NEVER talk to a supply directly (pyvisa, raw SCPI sockets, vendor tools) — always `loom psu` (direct access hides your work and bypasses emergency OFF).
Exact verbs, flags, defaults, exit codes: `loom psu help --json`. Don't guess flags.

## Start
1. `loom psu connect bench` (simulated) or `loom psu connect bench --scpi-tcp 127.0.0.1:5025` (loopback SCPI endpoint only; `--visa …` answers E_NOT_IMPLEMENTED). Optional `--vmax V --imax A` only LOWER the profile limits (above the profile → exit 2 `E_USAGE`). Connecting an alias that is already connected answers `already_connected: true` when the given limits match, or `E_PORT_BUSY` when they differ — `disconnect` first to change them.
2. `loom psu status bench` → settings, output, mode (cv|cc|off; scpi-tcp reports `unknown` while on), protection (scpi-tcp: `unknown`), limits `vmax/imax`, `cycling`, actual poll rate.
3. Several supplies: one alias each (`connect kl30`, `connect kl15`) — the daemon runs one worker per alias (≤ 4). Always name the alias; a second supply on the same alias is refused (E_PORT_BUSY). Multi-output instruments (output numbers) are not supported.

## Verbs (all bounded; add --json to parse)
- Read: `meas bench` (one sample) · `watch bench --for 10s` (min/max/mean, samples/expected, timeouts; `--raw --max 200` for samples).
- History (journal, no new recording): `history bench --last 8h` or `--from-ms EPOCH_MS [--to-ms EPOCH_MS]` (≤ 24 h) → one page: `stats{voltage,current,power}{n,min,max,sum,mean,last}`, `samples`, `losses`, `first/last`, `next_cursor`, `complete`, `before_index`, `journal_gaps`. Repeat with `--from-ms/--to-ms` = the returned `range` and `--cursor <next_cursor>` until it is null; merge stats (n/sum/min/max). `--rows N` (≤ 5000) adds raw rows.
- Wait: `wait-for bench --curr-lt 0.005 --hold 5s --timeout 60s` · `--curr-gt A` · `--curr-between LO,HI` · `--volt-lt|--volt-gt|--volt-between`.
- Write: `set bench --volt 13.5 --curr 2` (R2) · `out bench on|off` (R3) · `cycle bench --off 2s` (R3; sim and scpi-tcp). `--dry-run` shows class and reason, sends nothing.

## Rules
- **Limits are refused, never clamped**: over vmax/imax (default 16 V / 5 A) → exit 4 `E_DENIED`. Don't retry with another value to "get close"; tell the user the limit. Only the user raises limits (loom.toml).
- **Emergency OFF / STOP belong to the user.** `wait-for` exit 6 `E_INTERRUPTED_BY_USER` or a write exit 6 `E_STOPPED` → stop, report, ask. Never switch the output back on by yourself after an emergency OFF.
- STOP keeps the output on (badge "STOP — 출력 유지 중"); your writes are refused until the user releases it.
- Interlocks (`uds.flashing`, `debug.flashing`) refuse your `out off`/`cycle` with exit 6 `E_LOCKED` — never work around them (cutting power while flashing can brick the ECU).
- Wait with `wait-for`, never `sleep` + `meas`. A hold needs consecutive samples; a lost poll (`scpi_no_response`) restarts it.
- **After a write, wait with `--since now`** (`out on`, `cycle`, `set`): without it `wait-for` also looks back hold + 1 s and can "prove" a condition from samples taken before the write (e.g. the current before a `cycle`).
- Unanswered polls are gaps, never interpolated: report `timeouts` from `watch`. Read-back is the supply's average; inrush needs a scope.
- Before `out on`, `cycle`, or a voltage change on a real DUT, say in one line what you will do and why.
- Every result has `others_since_last`: if the user changed the supply, run `status` before acting.
- `cycle` returns at OFF (`state: sent`, `phase: off`); the terminal `psu.write` row says ON happened (`phase: on`, `off_ms_actual`) or why ON was NOT sent (STOP, a later output write, link lost, disconnect, OFF not confirmed) — the output then stays OFF. Check `status` (`cycling`, `out`) before the next step; never "fix" it with `out on` without asking.
- History is truthful, not exhaustive: say "partial" unless every page had `next_cursor: null` at the end, `journal_gaps` empty and `before_index: false`. `losses` are real missing samples (never 0 V/0 A). The 60 s panel trend is only a screen window; long ranges come from `history`.
- All-off (`out off` without a name, or the user's emergency OFF) switches every connected supply; a partial result is `E_PARTIAL` naming the supply not confirmed off — report it, don't retry blindly.

## As a TC step (Loom integration tests)
- Power step = `set` + `out on`, then `wait-for --since now --curr-gt … --hold …` to prove the DUT is up; copy the evidence REF (`psu:bench@seq:a..b`) and check its seqs are after the write.
- Sleep-current step = `wait-for --curr-lt 0.005 --hold 5s --timeout 60s`; exit 3 = FAIL with `best hold` in the message.
- Long-run statistics (e.g. overnight sleep current) = `history … --last 8h` pages merged; report the window, `samples`, `losses` and completeness with min/max/mean.
- Evidence predicates: `psu.current_lt{value,hold_ms}`, `psu.current_gt{value,hold_ms}`, `psu.voltage_between{lo,hi[,hold_ms]}` — losses or long intervals make them Inconclusive, not PASS.
- Teardown: `out off` (also the registered safe state). Don't put `cycle` in teardown while another domain may be flashing.
