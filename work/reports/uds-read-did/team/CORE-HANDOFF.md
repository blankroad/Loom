# CORE handoff (C · uds-core-opus) — READY_FOR_REVIEW (core source frozen; changes only on returned defects)

Owner files: `app/engine.js, app.js, config.js, tools.js, index.html, styles.css, host.js, catalog.js/.css, services.js/.css`,
`app/selfcheck/harness.cjs` + existing checks + `catalog-check.cjs, services-check.cjs, integration-check.cjs`, this file, `qa/team/core-*.log`.
(`project.js/.css/project-check.cjs` → A, see `PROJECT-CONTRACT.md`.)

## FREEZE (final)
- Hashes: `qa/team/core-hashes.log` (full sha256 values). engine `bdd15437…` · app `499a0434…` · host `e4d6144d…` · catalog.js `5305aea5…` · catalog.css `e1b83fa1…` · services.js `0094d578…` · services.css `254bebeb…` · index `b673f165…` · styles `b44efb8b…`. tools.js and config.js are unchanged from base.
- Suites (`qa/team/core-suites.log`, jsdom, not browser):
  - Core: check 70/70 · raw 55/55 · config 72/72 (the 197 baseline, with 4 documented intentional changes) · integration 43/43 · services 46/46 · catalog 44/44.
  - Owners, run with the real host and every module loaded: workflow 117/117 · trace 76/76 · layout 49/49 · project 50/51.
  - The project failure is A's: a missing `trace` section is "kept" because B's trace section exposes no `defaults()`. This is an A/B decision for ROOT; core needs no change.
  - **Superseded (per ROOT):** B has since added `defaults()` plus strict import, and A's project check is now 53/53 (`qa/team/workflow-project-check.log`). The 50/51 above is from an earlier checkpoint and was not re-run by C.
- Integration (I1–I7, real modules):
  - every mount is populated by its owner;
  - TP + watch on one context never overlap (serial guard), and busy slots are skipped, not queued;
  - automation never steals the selection;
  - a trace pause freezes the display while the engine keeps receiving;
  - the project guard lists tp/workflow/online;
  - explicit stops cancel nothing;
  - reload restores definitions (custom ECU, watch) with zero running/TX/links/timers;
  - the `suppressed` status flows through the trace filters.
- 390 fix (ROOT catalog-390 review): ECU ids, DID hex, addresses, badges and DTC codes are `nowrap`. Catalog and DTC tables sit in `.ct-scroll` (contained horizontal scroll, `min-width` ≤560 only), and fixture bytes wrap only at byte spaces. Desktop stays dense. Needs a native re-check.

## Key selectors for native QA
- Tools row `#ctxTools`: `#connOverview [data-ov]`, `#sessChip`, `#toolTabs [data-tool=services|catalog|workflow|watch|project]`, drawer `#toolDrawer`, `#toolClose`.
- Services `#servicesMount`: `[data-sv=s1|s3|s2]`, `#svSpr`, `#svSess` (`#svS3`), `#svTpInt #svTpMode #svTpBtn #svTp #svTpOthers`, `#svMask #svMaskPre [data-sv=d1|d2|clr|restore] #svDtc #svMsg`.
- Catalog `#catalogMount`: `[data-ct=ecu-new]`, `#ctEcuForm #ce-id #ce-name #ce-doipLA #ce-canTx #ce-canRx`, rows `tr[data-ecu]` with `[data-ct=ecu-sel|ecu-edit|ecu-del]`, `#ctEcuDel [data-ct=ecu-disc|ecu-stopact|ecu-del-cancel]`, `[data-ct=did-new]`, `#ctDidForm #cd-did … #cd-fxEcu #cd-fx #cd-fxAll #cdPrev [data-ct=fx-zero|fx-clear]`, rows `tr[data-did]` with `[data-ct=did-use|did-edit|did-del]`.

## Known SIM limits (honest)
- Session state is per ECU×route context. Reconnect resets to an *assumed* default.
- Programming (02) is allowed only from a non-default session, otherwise `7F 10 22`. There is no SecurityAccess or flashing.
- P2server 50 / P2*server 5000 ms are metadata only and are never applied to the client profile.
- S3 5000 ms restarts when the SIM ECU *processes* a request. A timeout without a late reply means the ECU never saw the request.
- SPRMIB requests end as `suppressed` once SIM processing completes (at the SIM latency). An NRC under SPRMIB is still delivered (physical addressing).
- DTC:
  - Read: `19 01/02` only. `14 FFFFFF` only (other groups give `7F 14 31`) and only in a non-default session.
  - Clearing empties the SIM DTC memory. [SIM 고장 fixture 재주입] is a local SIM action with no TX.
  - DTC descriptions are fixture text, not a real DTC database.
- TP, services and watch use the current SIM options, the same as a manual read; the `disconnect` scenario will drop the link.
- Custom DIDs:
  - A user-defined SIM decoder, not ODX.
  - With no fixture on an ECU the read gives `7F 22 31`.
  - A fixture length ≠ the record length is flagged as a decode error with the exact bytes kept.
  - `txn.didDef` snapshots the definition, so old details don't change when it is redefined.
- Deleting an ECU removes its custom profiles. History, confirmed values and txn labels keep the old ECU id/name.
No Rust/Cargo/build/git/deps/browser. Pre-change core hashes: `qa/team/core-baseline.log`.

## Checkpoint 1 — shared host is LIVE (A/B: build against the real `host.js` now)
Hashes: host.js `bdd8290c…4413` · index.html `b673f165…7b` · app.js `36ba114a…1e92` · engine.js `2c9603d3…6f` · styles.css `b44efb8b…37a1` · harness.cjs `f5ce0918…4e`.
Evidence: `node app/selfcheck/integration-check.cjs` → **29/29 PASS** (jsdom, not browser). Baseline regressions unchanged: check 70/70, raw 55/55, config 72/72 (197) with host.js + A's current workflow.js loaded.

**Load order** (index.html): `engine, app, tools, config, host, catalog, services, project, workflow, trace-tools, layout`. CSS links: `styles, catalog, services, project, workflow, trace-tools, layout` (layout.css last so B can override). `window.LoomWorkbench` exists synchronously when your script runs. `uds:host-ready` is also dispatched on `document`, but you don't need it.
**Harness**: `selfcheck/harness.cjs` now evals the scripts in index.html order and skips files that don't exist yet. It exports `{boot, fakeClock, DIR, FILES, ALL}`. Your file is loaded in every core check as soon as it exists, so keep it bootable at all times.

### H API (frozen object, `H.version === 1`)
| member | semantics |
|---|---|
| `H.engine`, `H.constants` | the live engine `E` and `window.LoomUds`. `U.ECUS`/`U.DIDS` are the same arrays the catalog will mutate (custom entries carry `custom:true`). |
| `H.clock` | `{now,set,clear}` = engine clock (fake clock in checks). Use it for all timers. |
| `H.context()` | `{ecu, transport, profile, link, gen, activeTxnId}` copy of the *current* context. `H.contextOf(ecu,tr)` does the same for any context (null if unknown). |
| `H.editor()` | read-only `{mode, didText, rawText}` (R1 delivered). |
| `H.compose({mode, didText?, rawText?})` | fills the editor and switches mode; **never sends**. Rejects bad mode, didText over 64 chars, rawText over RAW_TEXT_MAX. |
| `H.send({ecu?,transport?,mode,didText?|rawText?,source?,sim?,select?})` | goes through the same `E.read`/`E.sendRaw`. Defaults to the current context. Returns `{ok,txn}` or `{ok:false,code:'offline'\|'busy'\|'invalid'\|'route',error}`. No auto-connect, no queue. `txn.source` is stamped (R2 delivered; `/^[a-z][a-z0-9:_-]{0,39}$/`, otherwise `'manual'`). Only `source:'manual'` (or `select:true`) selects the row; automation never steals the selection. `sim` overrides the SIM options for that single request; the default is the current SIM options, like a manual read. |
| `H.cancelOwned(txnId, source?)` | cancels only when `E.ctx(tx).active === txnId` (and `tx.source === source` if given). Codes: `gone/not-owner/not-active`. Never touches other owners' requests. |
| `H.selectTxn(id)`, `H.refresh()`, `H.schedule()` | select a row; render synchronously; or render on the next rAF. |
| `H.subscribe(fn)` → unsub | events: `{type:'txn', txn, created?, late?}`, `{type:'link', context:{ecu,transport,link,gen,cause}}`, `{type:'config'}`, `{type:'clear'}`, `{type:'context', context}` (user switched target), `{type:'tool', tool}`. Coming in phase 2: `{type:'catalog', action, ecu?, did?}`, `{type:'session', ...}`. One event per engine emission, so you never see duplicate TX. |
| `H.onRender(fn)` → unsub | runs after each core render. If you call `H.refresh()` from inside it, core runs exactly one follow-up pass and does not loop. Patch only your own subtree, and don't rebuild focused forms. |
| `H.setTraceView({rows(baseRows), byId(id)} \| null)` | `rows` gets the ECU-scope-filtered engine rows and returns the visible rows. Detail resolves via `byId` first, then `E.byId`. If the current selection is not visible, it moves to the last visible row, or to none (detail cleared). |
| `H.project.register(key, {export, validate, import})` → unreg | reserved keys: `catalog, profiles, services, core`. Extension keys: `workflow, trace, layout`. `validate` must be pure. |
| `H.activity.register(key, {list, stop})` → unreg; `H.activity.list()`, `.forEcu(ecu)`, `.stop(key)` | `list()` returns RUNNING items `[{ecu?,transport?,kind,label?}]`. Include offline-but-running items. Core never calls `stop()` implicitly; only an explicit user button does. Core registers `tp` (TesterPresent). |
| `H.tools.open(k)/close()/current()/names` | tool drawer tabs: `services, catalog, workflow, watch, project`. |
| `H.ui.{esc,toast,download,stamp,fmtT}` | shared helpers (download = the real Blob anchor). |

### Mount slots (core-owned containers; you own only the subtree inside)
- `#workflowMount` (drawer tab "명령·시퀀스") and `#watchMount` (drawer tab "감시") → A.
- `#traceToolsMount` → B. A full-width strip in the history pane, between `.pane-h` and `.tx-scroll` (class `mount mount-strip`). Keep it compact (one row). Put the offline log viewer in your own collapsible section or dialog inside it.
- `#layoutToolsMount` → B. Inline, at the right end of the new `.ctx-tools` row in `#ctxbar` (class `mount mount-inline`).
- `#catalogMount`, `#servicesMount`, `#projectMount` → C.
- Drawer: `#toolDrawer` overlays the right side of `.mod-uds` (absolute; fixed full-screen under 560px). It never changes the `.wsp` grid, so B's column widths stay B's. Panels scroll internally.

## Plan — phase 2 core features (in this order)
1. **Engine catalog** (item 4/5): `E.catalog` with pure `validate(snapshot)` (checks a candidate registry: ECUs + DIDs + fixtures **together with profiles**, never mutating the live `U.ECUS/DIDS`), plus `load/exportStore/addEcu/updateEcu/deleteEcu/addDid/updateDid/deleteDid/setFixture`. Presets are immutable. Bounded: 32 custom ECUs, 64 custom DIDs, fixture payload ≤ 255 B. Custom DID fields: name, hex, byteOffset, length, type (`ascii, bytes, uint, sint`), endianness, scale, offset, unit, expected record length. Per-ECU SIM fixture payload (labelled SIM, never ODX). ECU delete is refused while any of its contexts is online or busy, or `H.activity.forEcu(ecu)` is non-empty. Existing txns keep their snapshots.
   - Persistence: additive `catalog` key inside `loom.udsReadDid.v1` (schema stays 2, written in the same single `setItem` as profiles, so it's atomic). Boot already hydrates the catalog **before** `saved.ecu` validation and `E.loadConfig` (wired in app.js).
2. **Engine services** (items 9/11), SIM ECU state per context:
   - `0x10 01/02/03`: programming only from extended, otherwise `7F 10 22`. Positive `50 xx 00 32 01 F4` = server P2 50 ms / P2* 5000 ms, shown as **ECU-reported metadata only**. These values are never applied to the client profile, whose P2 stays at the SIM-scaled 1000 ms (per the ROOT timing note). S3 = 5000 ms on the engine clock.
   - `0x3E 00/80`: SPRMIB → new terminal status `suppressed`.
   - `0x19 01/02 mask` from a per-ECU fixture DTC table. `0x14 FFFFFF` only in extended session, otherwise `7F 14 22`.
   - Raw `10/3E/19/14` hit the same state. Non-0x22 positives never touch confirmed DID values.
   - Tester view per context: `confirmed / unknown / expired(S3) / reset(reconnect)`.
3. `services.js`: session controls, TP (explicit start/stop/interval/mode, skips when busy, shows cadence, no auto-resume), DTC view (read / mask / status bits / diff / SIM clear).
4. `catalog.js`: ECU + DID CRUD with preview. `project.js`: one versioned file `loom-uds-project/1` (≤ 1 MB). It validates every section first, refuses while any activity runs or any affected link is online, then applies with rollback on throw or storage failure.
5. Checks: catalog/services/project/integration. Then intentional expectation updates in raw-check (`3E 80`, `3E`, `10 03`, `19 02 FF` no longer return `7F xx 11`), each documented with its rationale here.

## Notes for ROOT
- WORKFLOW-HANDOFF says project import "replaces definitions after stopping owned automations". The ROOT contract says the import guard requires all activities to be **already stopped** (no auto-stop). The core coordinator refuses import while `H.activity.list()` is non-empty, so A's import is only called once nothing is running. Please confirm A drops the implicit stop.
- Core regression checks now also load A/B files as they land, so a broken A/B file shows up as a core regression. I report those to ROOT and don't edit the files.

## Checkpoint 1b (ROOT trace additions) — delivered
- `H.traceScope()` → `{filter: 'all'|'ecu', ecu}`, the base scope the trace view is applied on top of.
- Optional `traceView.timeText(tx)` → string is used for the history `.c-t` cell (fallback: absolute `HH:MM:SS.mmm`).
- 200 ms tick: `elapsed(txById(live.id) || live)`, so a paused clone's frozen `tEnd` holds while the live request continues.

## Checkpoint 2 — engine catalog + services + services.js (READY for A/B use)
- New terminal status **`suppressed`** (SPRMIB request, no response bytes; `txn.final === null`). Include it in your terminal-state handling. `E.terminal()` already counts it as terminal.
- Events: `{type:'catalog', action}` (ECU/DID registry changed; `U.ECUS`/`U.DIDS` mutated in place), `{type:'session', ecu}`, `{type:'dtc', ecu}`.
- `E.session(ecu,tr)` → `{server:{session,name,s3Due}, tester:{session,name,status,t,txnId?,no?,p2srv?,p2starsrv?,note?}}`.
- `txn.didDef` = the DID definition snapshot at request time (decode stays stable if the catalog changes later).
- `services.js` registers activity key `tp` (kind `tester-present`, with ecu/transport). Automation sources used by core: `service`, `tester-present`.
- Intentional expectation changes (raw-check B5/E17): `3E 80` → `suppressed`; `3E` → `7F 3E 13`. Storage checks now allow `loom.udsReadDid.<module>.v1` extension keys (check.cjs storage, config-check after-save).
- `node app/selfcheck/services-check.cjs` → 46/46 (jsdom).

## Status
- [x] host.js + mounts + script/CSS links + harness order (checkpoint 1, 1b)
- [x] engine catalog API · [x] engine services · [x] services.js + services-check
- [x] catalog.js + catalog-check · project.js/css/check → **A** (PROJECT-CONTRACT.md; `H.coreProject` live)
- [x] cross-module integration (I1–I7) · [x] 390 table fix · [x] freeze hashes — awaiting ROOT native QA
- [x] ROOT native QA of C catalog/services PASS: DID old decode 2.3 → new 14.6, online delete guard, session/TP/DTC, and the shared SIM on CAN-TP 0x19 / EthDiag 0x10. **FINAL. C is idle and waiting; no further features or tests.**


## ROOT acceptance — ACCEPTED / DONE
Final source/native acceptance: `../qa/team/REVIEW.md`. Owner process exited normally and owned workspace closed; files remain frozen. Prior checkpoint caveats are superseded by the final 625/625 source results and native evidence recorded there.
