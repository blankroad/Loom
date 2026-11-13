# PROJECT contract (item 10) — owner A · uds-workflow-opus (transferred from C, project not yet started by C)

A owns these files: `app/project.js`, `app/project.css`, `app/selfcheck/project-check.cjs`. They are already linked in index.html (after services.js, before workflow.js), the mount is `#projectMount` (drawer tab "프로젝트"), and harness.cjs loads them as soon as they exist.
C keeps engine/catalog/services/host and integration. A must not edit core files. Ask C for API changes (via ROOT/handoff).

## Core API for the coordinator (live in host.js now)
`H.coreProject`:
- `export()` → `{catalog, profiles, services}`. Plain JSON of the current core definitions: custom ECUs/DIDs/SIM fixtures, custom connection profiles + active map, and TP/DTC prefs. Presets are never included.
- `validate(core)` → `{ok, value?, section?, error?}`. **Pure.** It validates catalog and profiles TOGETHER against the candidate ECU set, so profiles may reference custom ECUs that exist only in the file. It never touches the live `U.ECUS`/`U.DIDS` or the store. An absent subsection (`null`) means an empty default.
- `guard()` → `[{kind:'activity'|'online'|'busy', key?, ecu, transport, label}]`. Every running `H.activity` item (TP, watch, sequence, recording) plus every online or busy context. Import must be refused while this list is non-empty. It never auto-stops anything.
- `apply(value)` → `{ok, error?}`. Takes only a value from `validate()`. It replaces the core definitions and refuses (no mutation) if any context is online or busy.
- `commit()` → `true|false`. Writes core prefs to localStorage; `false` means quota or unavailable storage.
- `disconnectAll()` → count. Only for an explicit user button in the guard notice.

Extension sections: `H.project.sections()` → `Map(key → {export, validate, import})`. Keys: `workflow` (A) and `trace`, `layout` (B). Reserved: `catalog, profiles, services, core`.

## Required coordinator semantics
1. File: `{format:'loom-uds-project/1', version:1, exportedAt, sections:{core:{catalog,profiles,services}, workflow?, trace?, layout?}}`. Reject unknown format/version. Cap the size (≤ 1 MB, checked before parse). By default, export no captured log bodies, history, links or running jobs.
2. Export uses the real download (`H.ui.download`, JSON).
3. Import: parse → `H.coreProject.validate(sections.core)` → every present extension's `validate()` (missing extension section = that module's default, **validated too**) → `guard()` must be empty (otherwise show the reasons and offer explicit [stop]/[disconnect] buttons; never auto-stop) → snapshot = `coreProject.export()` + every extension `export()` → apply `coreProject.apply()` then each extension `import()` → `coreProject.commit()` must be `true`. If any step throws or returns not-ok, or commit returns false: restore the snapshot (`coreProject.apply(validate(snapshot).value)` + extension `import(validate(snap).value)`), then commit again, and show a visible error.
4. Import restores definitions only. No connect, send, watch, sequence, TP or recording starts. The history, confirmed values and live contexts stay. If the current ECU no longer exists, core falls back to the first ECU (no TX).
5. Fake-clock and jsdom check in `project-check.cjs`: invalid file → zero mutation of every section; quota failure → rollback; guard refusal; round trip with a custom ECU + profile + DID + fixture.
