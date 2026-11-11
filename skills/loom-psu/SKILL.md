---
name: loom-psu
description: Control and measure a bench power supply (voltage/current setpoints, output on/off, power cycle, sleep-current and inrush checks) through the Loom daemon while the user watches the live gauge in the Loom UI and can hit emergency OFF. Use for powering the DUT, waiting for current/voltage conditions, and as power steps of Loom integration tests. M1 drives a simulated supply.
---
# loom-psu

NEVER talk to a supply directly (pyvisa, raw SCPI sockets, vendor tools) — always `loom psu` (direct access hides your work and bypasses emergency OFF).
Exact verbs, flags, defaults, exit codes: `loom psu help --json`. Don't guess flags.

## Start
1. `loom psu connect bench` (M1: simulated supply; `--visa …` answers E_NOT_IMPLEMENTED until a supply is bought).
2. `loom psu status bench` → settings, output, mode (cv|cc|off), protection (ok|ovp|ocp|otp), limits, actual poll rate.

## Verbs (all bounded; add --json to parse)
- Read: `meas bench` (one sample) · `watch bench --for 10s` (min/max/mean, samples/expected, timeouts; `--raw --max 200` for samples).
- Wait: `wait-for bench --curr-lt 0.005 --hold 5s --timeout 60s` · `--curr-gt A` · `--curr-between LO,HI` · `--volt-lt|--volt-gt|--volt-between`.
- Write: `set bench --volt 13.5 --curr 2` (R2) · `out bench on|off` (R3) · `cycle bench --off 2s` (R3). `--dry-run` shows class and reason, sends nothing.

## Rules
- **Limits are refused, never clamped**: over vmax/imax (default 16 V / 5 A) → exit 4 `E_DENIED`. Don't retry with another value to "get close"; tell the user the limit. Only the user raises limits (loom.toml).
- **Emergency OFF / STOP belong to the user.** `wait-for` exit 6 `E_INTERRUPTED_BY_USER` or a write exit 6 `E_STOPPED` → stop, report, ask. Never switch the output back on by yourself after an emergency OFF.
- STOP keeps the output on (badge "STOP — 출력 유지 중"); your writes are refused until the user releases it.
- Interlocks (`uds.flashing`, `debug.flashing`) refuse your `out off`/`cycle` with exit 6 `E_LOCKED` — never work around them (cutting power while flashing can brick the ECU).
- Wait with `wait-for`, never `sleep` + `meas`. A hold needs consecutive samples; a lost poll (`scpi_no_response`) restarts it.
- Unanswered polls are gaps, never interpolated: report `timeouts` from `watch`. Read-back is the supply's average; inrush needs a scope.
- Before `out on`, `cycle`, or a voltage change on a real DUT, say in one line what you will do and why.
- Every result has `others_since_last`: if the user changed the supply, run `status` before acting.

## As a TC step (Loom integration tests)
- Power step = `set` + `out on`, then `wait-for --curr-gt … --hold …` to prove the DUT is up; copy the evidence REF (`psu:bench@seq:a..b`).
- Sleep-current step = `wait-for --curr-lt 0.005 --hold 5s --timeout 60s`; exit 3 = FAIL with `best hold` in the message.
- Evidence predicates: `psu.current_lt{value,hold_ms}`, `psu.current_gt{value,hold_ms}`, `psu.voltage_between{lo,hi[,hold_ms]}` — losses or long intervals make them Inconclusive, not PASS.
- Teardown: `out off` (also the registered safe state). Don't put `cycle` in teardown while another domain may be flashing.
