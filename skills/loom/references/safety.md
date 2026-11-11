# Safety model (contract 04, decision F1)

- **Automatic by default.** Every write R0–R4 runs without asking. Risk is still classified by the
  domain, shown in the UI, written to the event (`risk`) and to the audit log. The user may turn
  on `confirm` / `approve` / `deny` per domain in `loom.toml`; then you see exit 5 or exit 4.
- **Risk classes**: R0 read · R1 reversible (UART input, sim send) · R2 state change (DID write,
  DTC clear, reset) · R3 dangerous (security access, IO control, periodic send, PSU on) ·
  R4 irreversible (flash, erase). Environment (`sim|bench|vehicle`) never changes the class.
- **STOP** (`loom stop-all`, UI button, `POST /api/v1/stop`): latches every new agent/script write
  and job. User input is not blocked; outputs are not switched off. You may STOP; **only a user in
  the UI can release it.** While latched your writes fail with exit 6 `E_STOPPED` and the refused
  write is still shown to the user (`requested → failed`). STOP survives daemon restarts.
- **Recording failure** (audit/journal cannot write): `record_health=failed`, agent/script R1+
  writes are refused (exit 4 `E_RECORDING_FAILED`); reads and user input continue.
- **No auto-retry.** A write without a final answer may have reached the device. Check the rows
  (`loom events --since @last --kind <domain>.write`) before doing anything else.
- **Interlocks** registered by domains (e.g. no power cycle while flashing) stay on unless the
  user disables them.
- Everything is in the tamper-evident audit log (`~/.loom/audit/`); this is detection, not a
  security boundary against a malicious local agent — the point is that the user can see and stop.
