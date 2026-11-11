---
name: loom-can
description: Read, send and check CAN / CAN FD traffic through the Loom daemon (M1 sim bus with a fake BCM; PCAN-USB FD and Vector VN5640 later). Use for CAN traces, sending frames, waiting for a response frame with evidence, DBC signal values and waiting for signal conditions, the last frame per id, and CAN steps in integration tests. The user watches and intervenes live in the Loom UI.
---
# Loom CAN

The daemon owns the CAN hardware. Use `loom can …` only — never open the device with other tools or scripts.
Full command reference (authoritative): `loom can help --json`.

## Workflow
1. `loom can status` — channels, generation, drops, STOP latch, fake ECU nodes.
2. If a result warns `no_viewer_attached` / `no_visible_viewer`: ask the user to open the Loom UI (`loom ui open can`) before you send.
3. Observe first: `loom can trace --since @last --max 50`, `loom can last --id 0x310`.
4. Send: `loom can send 300#01` (cansend syntax: `300#01`, `18DAF110#01`, `123##1AABB` = FD, `300#R` = remote).
   - Preview risk and bytes without sending: `--dry-run`.
   - Send and check the response in one step: `--expect 0x301 --within 100` → `write_state: effective_verified` (or `effect_unverified` if no response).
5. Wait, don't poll: `loom can expect --id 0x301 --data 01 --after <cursor> --within 200` (exit 3 on timeout).
   Pass `--after` (e.g. the `next_cursor` you got) so an old frame cannot satisfy the wait.
6. Outputs are bounded (100 rows default, 1,000 / 64 KiB max). Narrow with `--ids 0x300-0x30F`, `--origin device`, `--dir rx`, or continue with `--since <next_cursor>`.

## Signals (DBC)
- Load once per channel: `loom can db load ./body.dbc -c ch0` (check `warnings` / `blocked`); `loom can db info`.
- Current value: `loom can sig get EngineData.EngRPM` → `EngRPM = 3200 rpm  (EngineData 0x100, age 18 ms)` + evidence. Exit 3 = no value in the window (default 10 s).
- Wait for a condition: `loom can sig wait-for "EngRPM > 3000" --timeout 10` (ops `> >= < <= == !=`, or `Sig between 11.5 14.5`). Only new samples after the call (or `--after <cursor>`) count; device frames by default (`--any-origin` to include yours).
- Decoded rows: `loom can trace --decode`, `loom can last --decode`. Use `Message.Signal` when a name is in several messages.
- Trends: don't dump numbers — give the user the Graphics link `loom ui open can/graphics?sig=EngRPM,VehicleSpeed`.

## Periodic / IG
- Start: `loom can periodic start --msg EngineData --set EngRPM=3000` (period = DBC cycle time) or `loom can periodic start 300#01 --period 100`. **Remember the job id** it prints.
- Change one signal while it runs: `loom can sig set EngineData.EngRPM=3500` (other signals keep their bytes). `loom can periodic list` shows jobs, owners, revisions.
- When done: `loom can periodic stop --mine`. STOP ends all periodic jobs; they do not restart by themselves.
- If the user changes a job in the UI (IG), you see it in `others_since_last` / `NOTE:`; mention it, don't fight it.

## Rules
- Exit codes: 2 usage → fix the command. 3 → the condition did not happen in time (report it, don't retry blindly). 4 denied → report, don't retry. 6 → the user intervened, STOP is on, or a lock — tell the user and wait. 7 → device/worker problem (`loom can status`). 8 → result has a gap: report the gap before concluding. 9 → `loom daemon status`.
- `NOTE:` / `others_since_last` means the user acted on the bus. Mention it and adapt; never silently overwrite their frames.
- Never edit `loom.toml` to relax policy and never drive the UI's approval buttons.
- Emergency stop of agent/script sends: `loom stop-all` (the fake ECU keeps running; user sends still work).
- Say `env=vehicle` when you report a send on a vehicle channel.
- UDS diagnostics: use `loom uds`, not raw frames. `loom can isotp …` is ISO-TP transport debugging only; an open ISO-TP endpoint reserves its tx id (raw sends on it → exit 4 `tx_reserved`).

## As a test step (loom-test)
- Judge "the device responded" with `loom can expect` (or `send --expect`), a signal condition with `loom can sig wait-for`; copy `data.evidence[]` / `refs` (e.g. `can:ch0@e1:seq:5013`, `can:ch0@e1:seq:42#/signals/EngRPM`) into the verdict. Never type seq numbers or values yourself.
- A timeout result cites the scanned window (`scanned: can:ch0@e1:seq:10..14`); a timeout alone is not proof that nothing happened.
- TEST plan filters default to `origins: [device]`. To inspect an agent TX frame, explicitly name `origins: [agent]`; this still cannot prove device behavior. Predicate origin and plan filter must both match.
- Your own sent frame (`AGT`, `can.tx`) is never evidence of device behavior. Gap (exit 8) or user intervention in the window → not PASS.

## Not yet (M2b)
Window checks (`check`), timing (`stats --expect-period`), real hardware (PCAN, Vector).
