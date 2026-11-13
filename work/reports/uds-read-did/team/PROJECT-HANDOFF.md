# PROJECT handoff (item 10 · A · uds-workflow-opus, transferred from C) — FROZEN · READY_FOR_REVIEW (project-check 53/53)

Owner files only: `app/project.js`, `app/project.css`, `app/selfcheck/project-check.cjs`, this file, `qa/team/workflow-project-check.log`.
Contract: `team/PROJECT-CONTRACT.md` (live `H.coreProject` + `H.project.sections()` + `H.activity`). No core/B edits.

## Hashes (sha256)
- `app/project.js` `142de40ced73557463fc6151a5c0d30a939d4b63b15e0c84044b365bfd337c8d` (250 lines)
- `app/project.css` `4cac3d45c792469fb9414dfdeacb814f56b848e8597a74982a617d4c7cfaa7be`
- `app/selfcheck/project-check.cjs` `79f72a91b1ececeecd371d87cd66c6d7e5b45f2b6fbcebc36f91b02f18976ead`
- Depends on workflow rev 2 `app/workflow.js` `1c6161d6…5601` (throw-on-persist-failure + `defaults()`).

## Evidence (jsdom + fake clock — NOT browser)
- `node app/selfcheck/project-check.cjs` → **53/53 PASS** (log `qa/team/workflow-project-check.log`). It covers:
  - Round trip: custom ECU `ABS` + DID `D001` uint/scale/unit + SIM fixture + custom DoIP profile + workflow command/watch bound to ABS + trace/layout prefs.
    - Export goes through the real download.
    - A fresh page gets a preview (no mutation), then explicit apply. All sections are identical afterwards.
    - The imported custom ECU/profile/DID fixture answers through the engine (`62 D0 01 01 2C`, profile "ABS bench").
    - Reload persists the definitions, and nothing starts.
  - 11 invalid files are rejected, each with **zero mutation** of core, every extension and every localStorage key:
    - file-level: oversize (checked before parse), bad JSON, format, version, missing core, unknown section
    - section-level: bad catalog, profile referencing an absent ECU, bad workflow/trace/layout
  - Guard: refused with zero mutation while an offline-but-running activity or an online link exists. Nothing is auto-stopped or disconnected. The explicit [<key> 중지] / [모두 연결 해제] buttons clear it, and the running workflow watch is listed.
  - History + confirmed values are preserved.
  - Absent sections use the module's own validated `defaults()` (workflow/trace/layout). A registered section **without** `defaults()` is kept unchanged and labelled "현재 유지" in the preview.
  - Rollback, each case verified against memory **and** persisted keys:
    - permanent core quota
    - transient selective workflow-key quota → clean rollback
    - permanent selective workflow-key quota → `rollback-incomplete` naming `workflow`
    - thrown layout import → clean rollback
  - The workflow section throws while running and throws on storage failure.
- Scratch-copy run of every other suite: catalog 44, config 72, integration 43, raw 55, services 46, layout 49, trace 76 all PASS. check 68/70 (nav-link artefacts of the copy).

## Semantics (as implemented)
- File `{format:'loom-uds-project/1', version:1, exportedAt, app, sections:{core:{catalog,profiles,services}, <registered extension keys>}}`, ≤1 MB.
  - Allowed sections = `core` + exactly the extensions registered on this page, so export and import are symmetric. An unknown or unregistered key rejects the whole file.
  - Export carries definitions/prefs only: no history, events, confirmed values, logs, links or jobs.
- `LoomUdsProject.instance`:
  - `parse(text)` — byte size check before `JSON.parse`.
  - `plan(json)` — pure. Runs core validate, then every registered extension validate. A missing section → `impl.defaults()` validated, or else `keep`.
  - `apply(plan)` steps:
    1. `guard()` must be empty.
    2. Snapshot core + every extension, each re-validated so the rollback value is known-good. Otherwise refuse.
    3. `coreProject.apply`.
    4. Each extension `import`.
    5. `commit()===true`.
  - On any failure: restore every section's memory and persisted state from the snapshot (best effort), re-commit, and return `code:'rollback'`. If any restore step fails, return `code:'rollback-incomplete'` with a distinct `rollbackError` and a severe UI message.
  - Also exposed: `build()`, `exportProject()`.
- UI `#projectMount > #pj`:
  - Export summary `#pjSum` and `[data-pj="export"]`.
  - `[data-pj="choose"]` + `#pjFile`.
  - Preview `#pjPrev` shows per-section source: 파일 값 / 기본값 / 현재 유지.
  - Guard `.pj-guard` with `[data-pj="stop"][data-key]` and `[data-pj="disconnect"]`.
  - `[data-pj="apply"]` is disabled while the guard is non-empty; `[data-pj="cancel"]`; message `#pjMsg` (`.ok` / `.bad` / `.bad.severe`).

## Known limits
- Extension modules must throw from `import` on storage failure. Core `commit()` only covers the core key. Workflow does this now; ROOT has returned the same item to B for trace/layout.
- `rollback-incomplete` means the in-memory state was restored but some persisted key may not have been. This is shown and never reported as a clean rollback.

## Status
- FROZEN. ROOT native PASS: actual download/import, version 99 atomic reject, online guard with explicit disconnect, missing-extension default import. No further source changes unless ROOT returns a defect.


## ROOT acceptance — ACCEPTED / DONE
Final source/native acceptance: `../qa/team/REVIEW.md`. Owner process exited normally and owned workspace closed; files remain frozen. Prior checkpoint caveats are superseded by the final 625/625 source results and native evidence recorded there.
