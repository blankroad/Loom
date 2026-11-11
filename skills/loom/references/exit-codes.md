# Exit codes → what to do

Mapping source: `loom_proto::error::ErrorCode::exit_code` (contract 03 §3.3).

| Exit | Meaning | Codes | Do |
|---|---|---|---|
| 0 | success | | continue; still read `warnings` and `others_since_last` |
| 1 | internal / not implemented | `E_INTERNAL` `E_NOT_IMPLEMENTED` | report to the user; do not work around with other tools |
| 2 | usage / config | `E_USAGE` `E_UNKNOWN_METHOD` `E_CONFIG` `E_SCHEMA` … | fix the arguments from `loom <domain> help --json` |
| 3 | condition not met | `E_TIMEOUT` `E_NO_MATCH` `E_UNEXPECTED_MATCH` `E_COUNT_OUT_OF_RANGE` | the device did not show what you expected — a finding, not a crash |
| 4 | refused | `E_DENIED` `E_RATE_LIMIT` `E_BUSY` `E_RECORDING_FAILED` `E_EVIDENCE_*` … | policy/recording said no. `E_RECORDING_FAILED`: recording is broken, writes are refused until fixed — tell the user |
| 5 | confirmation / approval (only when the user enabled it) | `E_CONFIRM_REQUIRED` `E_APPROVAL_*` | tell the user what waits for them; never approve yourself |
| 6 | user, lock or STOP | `E_STOPPED` `E_USER_ACTIVE` `E_LOCKED` `E_LEASE` `E_HELD_BY_USER` `E_QUARANTINE` … | the user is in control: stop and ask. Never request a STOP release |
| 7 | device / transport | `E_NO_RESPONSE` `E_DISCONNECTED` `E_NOT_OPEN` `E_PORT_BUSY` `E_WORKER_DOWN` … | for a write check `error.detail.write_state` (`outcome_unknown` = may have been sent): look with `loom events --since @last`, do not resend blindly |
| 8 | partial | `E_PARTIAL` `E_CURSOR_GAP` `E_SCAN_LIMIT` | `E_PARTIAL{published:false}`: the device acted but the screen may lag — do **not** retry. Gap/limit: narrow the range or continue from `next_cursor` |
| 9 | no daemon / protocol | `E_DAEMON` `E_PROTO_MISMATCH` | ask the user to run `loom daemon start` (or check `loom doctor`) |
| 10 | device negative response | `E_NRC` (+`detail.nrc`) | the ECU answered "no" — a result, read the NRC |
| 11 | test verdict failed | `E_TEST_FAILED` | only from `loom test end` / `loom run --test` |
