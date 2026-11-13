# TRACE-HANDOFF — worker B (uds-trace-opus): items 2 · 3 · 6

Status: **READY_FOR_REVIEW** (source freeze below; awaiting ROOT native acceptance). The plan follows; files were written incrementally and stayed runnable.
B owns only these files:
- `app/trace-tools.js`, `app/trace-tools.css`, `app/layout.js`, `app/layout.css`
- `app/selfcheck/trace-check.cjs`, `app/selfcheck/layout-check.cjs`
- this handoff, and `qa/team/trace-*.log`

B touches no file owned by C or A. B does not use git, a browser, installs, or any subagents.

## Plan
1. **trace-tools.js · filters and pause (item 2)**
   - Built against `H.setTraceView({rows, byId})`.
   - Filters: hex/text search, SID, DID, NRC, status, route, profile.
   - Shows a count, a clear-filters button, and a filter error when the input is invalid (an invalid filter is never applied silently).
   - Pause freezes the displayed rows as deep-cloned snapshots, and `byId` resolves the frozen detail. The engine, the recording and new events keep running, and the toolbar shows how many events arrived while paused.
   - Resume drops the freeze.
2. **trace-tools.js · continuous log (item 3)**
   - Recording starts and stops only on explicit user action.
   - Storage is an IndexedDB store, with a memory fallback whose notice says so honestly.
   - Records are JSONL: header, ctx, tx, ev, status, clear and mark. Records are diffed from `H.subscribe` events, so the 200-row history eviction, the filters and pause do not affect what is captured.
   - Records are flushed in chunks on the engine clock (fake-clock testable).
   - Segments rotate at a record/byte cap; retention is a maximum segment count and total size.
   - A write or quota failure stops the recording and shows an error. The UI never claims a false "recording".
   - After a reload, recording stays stopped. Segments are listed, and the selected one can be reopened, exported, or deleted.
3. **trace-tools.js · offline viewer**
   - A separate read-only overlay, with no engine access and no send/replay.
   - Opens a stored segment or an imported file: our JSONL/JSON, or the existing `loom-uds-read-did-trace/1` export.
   - Import is size-capped and record-capped, validated atomically (the whole file is rejected with the line number of the first error), and sanitized.
   - All rendering is escaped.
4. **layout.js (item 6)**
   - Width handles: DID|center, center|values, and the detail seq|fields and fields|bytes columns.
   - The handles respond to pointer drag and keyboard, carry `role=separator` and aria values, and are clamped at desktop widths. They are disabled at ≤1180 or ≤560 as appropriate.
   - Maximize/restore for history and for detail; Esc restores. Reset. Named layouts.
   - Prefs are stored in `loom.udsReadDid.layout.v1`, and the project section is `layout`.
   - The core's vertical history divider is untouched.
5. **Checks:** `trace-check.cjs` and `layout-check.cjs`. They use jsdom and a fake clock, and fall back to a stub host until `host.js` exists. They are source checks only, not browser proof.

## API notes / requests to ROOT/C
- **Uses:** `H.setTraceView` (installed once at boot and again on filter/pause/time changes, never from `onRender`), `H.traceScope()` (explicit base scope used to freeze rows), `H.subscribe`, `H.onRender` (patches only its own toolbar), `H.clock`, `H.activity.register('trace-log', …)` (one global `{kind:'recording'}` item while starting, recording or stopping), and `H.project.register('trace', …)`.
- **Request to C (optional, not required):** core row time cell. `view.timeText(tx)` returns `+s.mmm` relative to the first visible row when the user picks [상대]; `view.timeMode()` returns `abs|rel`. Today the core always prints the absolute time, so the live [절대/상대] toggle only affects stored prefs until C reads `timeText`. The offline viewer applies its own toggle fully.
- **Request to C:** the elapsed tick (`[data-el]`, 200 ms) writes live elapsed text over frozen rows while paused. Frozen clones carry `tEnd = pause time`, so the tick should skip ids where `traceView.byId(id)` returns a frozen row.

## Progress log
- Plan written.
- **Checkpoint T1: runnable** (`app/trace-tools.js`, `app/trace-tools.css`). Filters, pause, recorder, segments and viewer all work in the real host via jsdom (memory store). ROOT native T1 found a start deadlock: `start()` chained `flush()` inside `R.q`, flush returned that same chain, and the state stayed `starting`. **Fixed T1b:**
  - `enqueue(fn, onErr)` appends exactly one op and returns that op's promise (resolves true/false, never rejects).
  - Queued ops never call flush/enqueue.
  - `start()` enqueues the segment meta, then calls `flush()` from outside the chain. It claims `recording` only after both writes succeed.
  - `stop()` → `flushTo` → retention.
  - Smoke run: start → `recording`; stop → `closed` with 19 records; JSONL round-trips through `parseLog`.
  - ROOT failed evidence `qa/team/trace-start-first-fail.png` and segment `R69ivex-94611570-001spv` are kept. On the next boot that segment is listed as `중단됨`.
- **T1b also includes:**
  - Pause freezes rows using the explicit `H.traceScope()`.
  - `tx` log records carry the immutable `route.values` (`config`) and `route.fields`. These are validated on import and shown in the viewer.
  - Status and event kinds accept any lower-case token, e.g. `suppressed`, which renders as "응답 억제".
  - `tx.source` is logged.
- **Checkpoint L1: layout runnable** (`app/layout.js`, `app/layout.css`).
  - `#layoutToolsMount` holds [기록 ⤢] [상세 ⤢] [레이아웃] (a popover with reset and named save/load/delete, max 12).
  - Four `.lo-h` separators sit absolutely inside `#wsp`/`#detail`, so there is no grid track.
  - Handles move with pointer drag and ←→ (Shift ×4); Home or double-click resets.
  - Clamps: DID 160–420 px, values 220–480 px, centre ≥ 480 px, detail seq 15–45 %, bytes 18–45 %, middle ≥ 25 %.
  - The values and detail handles are hidden ≤1180; the DID handle ≤860; all handles ≤560. Maximize is ignored ≤560.
  - Esc restores.
  - Prefs live in `loom.udsReadDid.layout.v1`; the project section is `layout` with pure `validate`.
  - The core `#divHist`/histH is untouched.

- **T2 (from ROOT reviews):**
  - When the recording activity starts or stops, one `H.schedule()` runs so the header `자동 N` badge refreshes. It is called from `renderRec`, never from `onRender`.
  - Strict project import (P-atomic): `trace.import` and `layout.import` write localStorage **first** and **throw** on failure (QuotaExceededError etc.) before any state changes, so the coordinator can roll back.
  - Ordinary convenience writes stay best-effort, but a failure is visible: a toast for trace, the `#loMenuBtn` title and the popover message for layout.
  - Both registrations now have `defaults()`, which returns a fresh object that passes the section's own `validate`.

## Contract notes for ROOT / A (B cannot message A; please relay)
- **Project section shape used by B:** `{export(), defaults(), validate(json) (pure), import(value) (strict: persist-then-apply, throws on storage failure)}`. A's `workflow` section should behave the same, so that the coordinator's rollback sees untouched state when a storage write fails.
- **Missing section in a project file:** the coordinator should run `validate(defaults())` and then import that value. `trace` defaults to `{v:1,timeMode:'abs',viewerTime:'rel'}`; `layout` to `{v:1,cur:{all null},named:[]}`.
- **Activity:** `H.activity` key `trace-log` returns `[{kind:'recording',label}]` while starting, recording or stopping. `stop()` is the same as [기록 중지]. Project import never starts or resumes recording, and the log store is never part of a project.
- **Still open for C (optional):** (1) honour `traceView.timeText(tx)` in the row time cell for the live [상대] toggle; (2) the elapsed tick should not overwrite rows that are frozen while paused.
- **Detail vertical drag** is the core `#divHist` (C-owned). B leaves it alone; B's handles are the four vertical (column) separators only.

## SOURCE_FREEZE — `qa/team/trace-freeze-sha256.log`
```
37a1124bfe9f8a6e3124b3e5a5310549bccb25bc4b8a8e0f28f576bda1475423  trace-tools.js
c1198c1e0350277faa649ce1423eb93026b98596c0de0eebe1246a9313b57f6e  trace-tools.css
ae8fdc5923493ce1558e241f9829951fdbdc82f0a313bbf1789003bf85d897a6  layout.js
0a2b6ab0b395bf12c3994cc98f92a317d1551162213a27b4aed8175891988b7e  layout.css
808bdef5c3244ae32cdc6b7b159de2e6304e824bb3646848ca6f0438260690ea  selfcheck/trace-check.cjs
e757bbaa128074b7a7c974409c25e6232e5f72d295b233ded5f081dc4310016a  selfcheck/layout-check.cjs
```
Verify from `app/` with `shasum -a 256 -c ../qa/team/trace-freeze-sha256.log`.

## Checks (source-local jsdom + fake clock; not browser proof)
| Command (repo root) | Result | Log |
|---|---|---|
| `node work/reports/uds-read-did/app/selfcheck/trace-check.cjs` | **76/76 PASS** | `qa/team/trace-check-result.log`, `qa/team/trace-checks.log` |
| `node work/reports/uds-read-did/app/selfcheck/layout-check.cjs` | **49/49 PASS** | `qa/team/trace-layout-check-result.log` |
| core `check` / `config-check` / `raw-check`, run once with B modules loaded | 70/70 · 72/72 · 55/55 PASS | `qa/team/trace-core-regression.log` |

**What `trace-check` covers:**
- **A — filter predicates:** byte-aligned hex, text, SID, DID, NRC (final and 0x78), status, route and profile. An invalid field is reported and not applied.
- **B — live DOM:** count; an empty result clears the selection and detail; aria-invalid; clear; the existing JSON export is not affected by the filter.
- **C — pause:**
  - the engine continues and the rows freeze;
  - the frozen detail and elapsed time are immutable;
  - the new-event counter works and filters apply to frozen rows;
  - the explicit `traceScope` is honoured for `ecu` and `all`;
  - resume returns to live rows; `timeText` works.
- **D — recorder (fake IndexedDB that persists across reloads):**
  - `starting` is not claimed as recording; the start promise resolves to `recording`; the activity is registered.
  - The log keeps every txn even past 200-row history eviction, during pause or filtering, including pending/nrc/timeout/late×2/cancel events, ctx records, header and marks.
  - Stored bytes equal the meta; tx records carry config, fields, profile and timing; raw bytes are exact.
  - JSONL download is the exact stored text; JSON download works; the viewer rebuilds 205 txns with no engine effect, and its filter, detail, Esc and missing send controls are all checked.
  - Delete needs a confirm and deletes only the selected segment.
  - After a reload, recording is not resumed and the segment shows as `interrupted`.
  - A quota failure gives a visible error, no false recording and no activity.
  - A start failure is reported; rotation produces self-contained parts and retention works; an IndexedDB open failure falls back to memory with an honest notice.
- **E — import:**
  - Atomic rejection with the line number keeps the previous model.
  - Missing header, unknown format, orphan records, wrong types, overlong strings, the size cap and bad config are all rejected.
  - A `suppressed` token is accepted and bad tokens are rejected.
  - Script/HTML in a file is escaped.
  - A trace/1 export opens read-only with exact bytes.
  - Project section: validation is pure, import is strict and throws (state untouched), `defaults()` works, and the export carries no log data.
- **F — static:** no send/network APIs; onRender does not refresh; the viewer overlay is fixed.

**What `layout-check` covers:**
- 4 separators with role/aria;
- clamps, centre minimum and detail-middle minimum;
- keyboard ±16 / Shift ±64, Home, and handle direction;
- maximize/restore, Esc, and Esc ignored inside the drawer;
- the collapse rail is respected; breakpoints 1180, 860 and 390 (no maximize, no handles);
- reset and named layouts (cap 12, validation, popover);
- pure validate, strict throwing import, `defaults()`;
- reload, unknown version and corrupt prefs;
- static CSS: absolute handles, own vars only, media gates.

## Selectors (ROOT native)
- **Trace strip, `#traceToolsMount > #ttRoot`:**
  - inputs `#ttQ #ttSid #ttDid #ttNrc #ttStatus #ttRoute #ttProfile`, button `#ttClear`;
  - count `#ttCount`; message `#ttMsg` (pause notice, filter errors, empty result);
  - time toggle `#ttTime [data-tm=abs|rel]`; pause button `#ttPause[aria-pressed]`; `#ttRoot.paused`.
- **Recorder:** `#tlRec[aria-pressed]`, status `#tlSt`, toggle `#tlSegBtn` (`#tlSegN`). In `#tlSegs`: radios `input[name=tlseg]`, plus `[data-tl=open|jsonl|json|del]` (`del` needs two clicks).
- **Viewer, `#tlViewer[role=dialog]`:** `#tvFile`, `#tvClose`, `#tvSrc`, filters `#tvQ #tvSid #tvDid #tvNrc #tvStatus #tvRoute`, `#tvTime`, `#tvList .tv-r[data-id]`, `#tvDet`, `#tvMsg`.
- **Layout:**
  - buttons `#loMaxHist`, `#loMaxDet`, `#loMenuBtn`; popover `#loPop` with `[data-lo=reset|save|load|del]` and `#loName`;
  - handles `#loHDid`, `#loHVals` (in `#wsp`) and `#loHSeq`, `#loHBytes` (in `#detail`);
  - classes `#wsp.lo-did/.lo-vals/.lo-max-hist/.lo-max-detail`, `#detail.lo-cols`.
- **Debug API:** `window.LoomUdsTrace` and `window.LoomUdsLayout`.

## Known limits
- The log is durable **browser-local storage (IndexedDB)**, not an OS file stream. A file appears only through an explicit download. If IndexedDB is unavailable, logging falls back to memory and says so.
- **Limits:** a segment is 20k records or 4 MB; retention is 24 segments or 32 MB (memory: 8 MB). Import is capped at 16 MB and 200k records; strings at 400 chars; hex at 4100 B.
- **`snap` flag:** in-flight txns at start or rotation are logged from the live state and flagged `snap`. The viewer labels them.
- **Pause:** pausing freezes the set of rows (scope through `H.traceScope`). Clearing history while paused does not clear the frozen view. The elapsed tick overwrite is a C item.
- **Live [상대] time** needs C to use `timeText`. The offline viewer handles it fully.
- **Layout geometry** cannot be measured in jsdom; drag and handle placement need native proof. ROOT native already reported 1440/768/390 with no overflow and handles disabled as expected.
- **Status tokens:** future values (e.g. `suppressed`) are accepted as lower-case tokens and rendered escaped as-is, with a Korean label where one is known.


## ROOT acceptance — ACCEPTED / DONE
Final source/native acceptance: `../qa/team/REVIEW.md`. Owner process exited normally and owned workspace closed; files remain frozen. Prior checkpoint caveats are superseded by the final 625/625 source results and native evidence recorded there.
