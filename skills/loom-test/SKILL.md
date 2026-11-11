---
name: loom-test
description: Run device integration TCs from natural-language or Markdown test cases using Loom sessions and copied event evidence. Use for UART/CAN/UDS/DLT/LA/PSU device verification, not ordinary software unit tests.
---
# Loom TEST
Use `loom <domain> help --json` for installed command syntax. User and agent must see the same device RPCs and journal events; never open a separate device path.

Preserve TC sentences in `.tc.md`. Write `.plan.yaml` with bounded commands, expectations and anchors, then run `loom test plan check <plan>`.
Local checking without a registry warns that command existence/risk were not checked; it is not execution approval. Start with `loom test start <tc> --plan <plan>`.
F1 is automatic by default: show the risk summary and proceed. Wait for approval only when policy actually returns exit 5; never approve as the user or alter policy to bypass it.
Use `manual` for visual/sound/human judgments. Machine PASS requires the registered verifier; `E_NOT_IMPLEMENTED`/NotWired is not verification.

For each stage, `loom test step <session> <id>` → planned domain commands → `loom test step <session> <id> --end`.
Use bounded `expect`, `wait-for`, or capture waits; do not sleep to infer a result. Bind the assertion to the planned stimulus/attempt, not to when a later query happens.
After reboot, UART example: `loom uart expect dut 'login:' --from seq:<copied-reset-response-seq> --timeout 30`.
For UDS, wait for `link == connected` with a bounded `uds wait-for` before the next read; check installed help for its target/timeout syntax. Do not resend an outcome_unknown write.
Read `others_since_last` on every result. Record user intervention with `loom test note`; it may make evidence inconclusive even after the session resumes.

REFs are copied only from command `data.evidence[]`/check results. Never invent seqs, values, timestamps or digests.
Your own TX is a stimulus/anchor, not evidence of DUT behavior. A stimulus-only stage uses `done` only after a confirmed-write reference is verified.
Use `loom test check <session> <step> --evidence <copied-REF>` before proposing a verdict. `--auto` suggests candidates; it does not authorize an automatic PASS.
With no evidence, do not try pass: record `inconclusive` with a reason. If verification rejects evidence, fix the evidence/window or remain inconclusive; do not keep retrying pass.
A gap/retention/no-record window cannot prove absence, exact counts or timing. Use verifier measurements, not numbers calculated by the agent.
Use `loom test verdict <session> <step> <verdict> --evidence <REF> --reason <reason>`; the server decides the effective verdict.
Agent override is forbidden. Human assertions/overrides remain explicitly labeled and require the viewer rights enforced by the server.

Follow planned `on_fail` and bounded retry counts. For a new write, amend the plan first; a new revision invalidates all previous approval and evidence binding.
Pause exit 6 blocks session-client writes; status/checks can still be read. Wait for resume rather than changing clients.
On global STOP, stop immediately. Never run finally automatically; safe-state recovery belongs to the user/CORE path.
For a viewer abort, the TEST card supplies the explicit `run_finally` choice. Agent cancellation runs planned finally normally; do not invent a user choice to skip it.

Ask through `loom test ask` only when TC meanings differ materially, observation needs a person, intervention prevents attribution, or a real policy/device error leaves a decision the agent cannot resolve.
An R4 badge or vehicle environment alone does not require an extra question under F1. Use bounded question timeout; answer RPCs are for the viewer.
The ask result is an ask id; `loom test wait <session> --until answer --ask-id <id> --timeout 60s` waits for the answer.
Finish with `loom test end`. Exit 0=passed, 11=failed, 8=inconclusive/incomplete, 6=STOP/pause. Copy the recorded result and evidence references into the summary.
Import, HTML/JUnit report and freeze are P2 until the installed service supports them; report E_NOT_IMPLEMENTED honestly.
