# WORKFLOW handoff (A · uds-workflow-opus) — FROZEN rev 2 · READY_FOR_REVIEW (workflow-check 117/117)

Owner files only: `app/workflow.js`, `app/workflow.css`, `app/selfcheck/workflow-check.cjs`, this file, log `qa/team/workflow-check.log`.
No edits to C core (engine/app/tools/config/index/styles/host/catalog/services/project) or B files. No git/deps/browser/research.

## Hashes (sha256) — rev 2
- `app/workflow.js` `1c6161d682ad137064063e8e5976c944bb13e6fb714d288b48b03f8437d25601`. Rev 1 was `9a147dfd…053b`, accepted natively. Before editing I verified that rev 1 + my summary edit reverted == `9a147dfd…`, so there are no foreign edits.
- `app/workflow.css` `cfc97602f30b50ecd14804b9789967e2e7c8bc36522c7c364d3e8a7c9cb2c170` (unchanged)
- `app/selfcheck/workflow-check.cjs` `3e6a5db7fafca69cd377f146c8be5deb0cddf5fac5dce12289fba0ea190e45b4`

### Rev 2 changes (ROOT-returned only)
1. Sequence run summary off-by-one: a completed run showed `반복 1/1 · 단계 1/2`, because loop had advanced and idx had wrapped. It now shows `반복 n/n · 단계 k/k 완료`; failed/stopped runs show the step they reached. 2 new asserts.
2. P-atomic: `persist()` returns a boolean. Project `import` reverts memory and **throws** on storage failure, so the coordinator can roll back. Covered in project-check (transient and permanent selective `loom.udsReadDid.workflow.v1` quota).
3. Project registration adds `defaults: () => emptyDoc()` (generic coordinator default hook, same as B's).

## Evidence (source-local jsdom + fake engine clock — NOT browser, NOT final integration)
- `node app/selfcheck/workflow-check.cjs` → **117/117 PASS, host: real host.js** (checkpoint 1). The result is written to `qa/team/workflow-check.log`. The stub host is used only if host.js is absent.
- Scratch-copy run of every other suite (so other owners' result files are not overwritten): catalog 44/44, config 72/72, integration 43/43, raw 55/55, services 46/46, layout 49/49, trace 76/76. check 68/70: the 2 failures are nav-link artefacts of the copy (identical without workflow.js).

## Integration (C) — nothing required beyond checkpoint 1
- Bootstraps once at script load if `window.LoomWorkbench` exists; otherwise waits once for the `uds:host-ready` listener. No polling. Exposes `LoomUdsWorkflow.{model, create, mountUI, boot, instance, ui}`.
- Owns the subtrees `#workflowMount > #wf` and `#watchMount > #wt` only. Uses H: engine, constants, clock, context, editor, compose, send, cancelOwned, selectTxn, subscribe, onRender (ECU-list change only), project.register('workflow'), activity.register('workflow'), tools, ui.{esc,toast,download,stamp}.

## Selectors (for ROOT native QA)
- Library: tabs `#wft-commands / #wft-recent / #wft-sequences`. Bar: `[data-wf="save-current"]`, `[data-wf="export"]`, `[data-wf="import"]` (+ `#wfFile`), notice `#wfNote` (+ `[data-wf="undo-import"]`).
- Commands: `#wfFolder`, `#wfSearch`, `[data-wf="cmd-new"]`, list `#wfCmdList li[data-cid]`. Row buttons `cmd-load` (to editor), `cmd-run`, `cmd-edit`, `cmd-dup`, `cmd-del`. Row status `.wf-st` (`#no status · target`, clickable → select txn).
- Command editor `#wfCmdEdit`: `#wfeName #wfeFolder #wfeDesc #wfeMode [data-m] #wfeDid #wfeRaw #wfeTarget #wfePrev #wfeErr`, submit = 저장, `[data-wf="cmd-cancel"]`, Esc closes and keeps the draft.
- Recent: `#wfRecent li[data-ri]` → `rec-load`, `rec-save`, `#no` link.
- Sequences: `[data-wf="seq-new"]`, `#wfSeqList li[data-sid]` → `seq-run / seq-stop / seq-edit / seq-dup / seq-del / seq-sel`. Editor `#wfSeqEdit`: `#wfsName #wfsTarget #wfsLoops #wfsStop #wfsSteps tr[data-j] [data-sf=mode|didText|rawText|cmdId|delayMs|status|prefix] [data-sa=up|down|rm]`, `[data-wf="seq-add-step"]`, `#wfsErr`. Run view `#wfRun` (current step `li.cur`, results `.wf-res`).
- Watch: add form `#wtAdd` (`#wtDid` with datalist, `#wtTarget`, `#wtInt`), `[data-wt="stop-all"]`, `#wtMsg`. Rows `#wtBody tr[data-wid]` with cells `[data-c=value|age|status|src|mm|spark|cad|btn]`, `select[data-wt="int"]`, `[data-wt=start|stop|del|goto]`, sparkline `svg.wt-spark`.

## Behaviour (as implemented)
- **Commands (1)**: name ≤40 / folder ≤32 / description ≤200 (escaped). The request is a DID (normalized hex4) or exact Raw (normalized). Optional target binding. Run = `H.send(source:'command')` against the binding, or else the current context at run time. Offline/busy errors are shown on the row, and nothing is ever cancelled.
  - **ROOT fix applied**: an explicit [실행] passes `select:true` only when the execution context equals `H.context()`. A bound other-ECU run neither switches the target nor steals the selection.
  - Load = `H.compose` with zero TX. Save-current reads `H.editor()` into a new draft.
  - Per-item drafts survive editing another item. Library editing never touches the main editor.
  - Max 200. A command referenced by a sequence cannot be deleted.
- **Recent (1)**: runtime only, 20 unique (ecu, route, mode, bytes), newest first, fed by subscribe. It survives history clear/eviction and is never persisted/exported/imported. Actions: to editor (zero TX, target stays current) and save as a command bound to the recorded target.
- **Watch (7)**:
  - Starting: explicit start only, and it requires the link to be online (no auto-connect). The first poll fires at start, then fixed-rate slots (200 ms–60 s).
  - Concurrency: at most one in-flight request per context. A busy slot (own previous poll still pending, manual/other request, or a sequence on that context) is skipped and counted; there is no queue. Two rows on one context get one bounded deferral, fired 100 ms after the completing poll.
  - Display: actual cadence (mean of the last 12 sends), sent count, skips (busy/sequence).
  - Value/unit come from the engine **confirmed** entry produced by that exact txn. Positive-but-not-applied → status `stale`. NRC/timeout/cancelled retain the old value and its age.
  - Charts: min/max and an SVG sparkline of the last 60 numeric samples. Non-numeric values say 비수치 instead of drawing a chart.
  - Stopping: link loss stops the watch with no auto-resume on reconnect. Stop = no further polls; an in-flight poll completes normally (never cancelled).
  - Rendering: runtime ticks patch cells only, so a focused control is never rebuilt.
- **Sequences (8)**:
  - Steps: DID / Raw / saved command (resolved and frozen at start), delay before the step (0–60000 ms; ≥100 ms yield between steps), expected status `positive|nrc|response|any`, optional response hex prefix.
  - Bounds: loops 1–100, ≤50 steps, steps×loops ≤1000.
  - Run: explicit Run/Stop, target frozen at start (switching the UI target mid-run has no effect). Strictly sequential: the next step goes only after the owned txn is terminal (via subscribe).
  - Errors: fail-fast by default; with it off, failures are recorded and the run continues (`done-errors`).
  - Busy handling: if the context is busy with this workflow's own watch poll, the step waits for it. If it is busy with anyone else, or offline, the step fails and nothing is cancelled.
  - Stop cancels only through `H.cancelOwned(txnId,'sequence')`.
  - Never selects rows and creates no synthetic TX.
- **Persistence**: `loom.udsReadDid.workflow.v1` = `{format:'loom-uds-workflow/1', commands, watches, sequences, prefs{tab,folder}}`. Nothing is written at boot. Corrupt data or an unknown version → defaults plus a visible notice, and the original value is kept until the next save. Storage failure is reported.
- **Import/project**:
  - Standalone JSON import (≤512 KB): whole-file validation, then replace with an undo option. It is **refused while anything runs** and starts nothing.
  - Project section `workflow`: `validate` is pure. `import` is **stop-first** — it throws if anything is running (→ coordinator rollback) and never stops jobs implicitly.
  - Target ECU ids are checked structurally on import, since the project may also bring ECUs. Existence is checked at run/start, and a missing ECU is shown as `(없음)`.
- **Activity**: `H.activity.register('workflow')` lists every running watch/sequence `{ecu,transport,kind:'watch'|'sequence',label}`. `stop()` stops them all; only core's explicit button calls it.

## Known limits / notes
- Session-dependent expectations (e.g. `10 03`) follow whatever the engine SIM returns. Nothing hard-codes non-0x22 NRCs (checks use `response`/`any`). The C `suppressed` status (3E 80) is labelled. `response` does not count `suppressed`; use `any` for that case.
- A manual Read on a context with a fast watch may hit `busy` from the engine (no priority queue, by design). Stop the watch or use a longer interval. Watch/sequence skip whenever a manual request is in flight.
- Watch/sequence values use the current SIM options (same as a manual read). There is no per-automation SIM override.

## ROOT native QA so far
- ROOT first PASS: save-current/load/offline refuse/run/recent→editor; screenshot `qa/team/workflow-first-1440.png`. Gap "explicit run leaves selection empty" → fixed (see above); re-check pending.
- QA command `c1` in ROOT's browser storage belongs to ROOT's cleanup.

## Status
- [x] model + validation · [x] runtime · [x] UI · [x] check 117/117 · [x] hashes rev 2 — source frozen pending ROOT review/native QA (changes only on returned defects)


## ROOT acceptance — ACCEPTED / DONE
Final source/native acceptance: `../qa/team/REVIEW.md`. Owner process exited normally and owned workspace closed; files remain frozen. Prior checkpoint caveats are superseded by the final 625/625 source results and native evidence recorded there.
