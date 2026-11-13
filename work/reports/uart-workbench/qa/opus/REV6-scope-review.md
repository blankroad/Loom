# Opus REV6 macro execution scope — READ-ONLY static review (not native browser verification)

- Reviewer: QA (claude-opus-5-5). Source read only. No browser, because Ego 15 is user-controlled. No tests rerun.
- Reviewed: app **db469ee2** / styles **ddf7c8a3** / index **4c077401**. This equals `opus/selfcheck/source-sha256-rev6.txt`. Owner's selfcheck: 117/117 PASS.

## Focus areas
| Area | Evidence (app.js) | Static verdict |
|---|---|---|
| Scope vs keyboard independence | Macro scope `MS` (l.94) is separate from keyboard `BC`. `keyInput()` uses only `BC`. `macroRecipients()` never reads `BC`. The scope note says "▶·단축키 매크로에만 적용 — 키 입력 대상(툴바)과 별개". | OK |
| Preview == resolver | The panel preview (`data-role=ms-recipients`, l.914) and each macro row label (`tgt = macroTargetLabel(m)`) both come from `macroRecipients()`. `requestRun()` resolves through the same function. The preview re-renders on `setActiveLeaf` (l.646), on structural changes, and on scope/checkbox changes. | OK |
| All entry points honour All/Selected | Sidebar ▶ / pane menu (`run-macro`, l.1681) and hotkey (l.1756) → `requestRun()`. The vars dialog re-enters `requestRun()`. The pane-menu `hintTab` only sets the active pane (matters for 활성 only). No direct `runMacro` call bypasses the resolver. | OK |
| Fixed override migration | Stored legacy `target:<sid>` → `fixed:[sid]` (`migrateMacro`, l.92). The row shows a `고정` chip and a label "고정 → alias". The editor offers explicit radios: panel scope (default) / 고정 (wins). Precedence is fixed > panel, and it is documented and visible. | OK, except F5 (import) |
| Run snapshot | `runGroup()` copies `R.ids` into `GROUPS[gid]`. Recipients are resolved once per execution. Tabs already running another macro are skipped and listed with "다른 매크로 실행 중". | OK |
| Stop | Per pane: `stop-macro`. Per macro: `stop-macro-all` stops every running instance of that macro (sidebar 전체 중지). Global: `stop-all`. Esc stops the active tab's run. | OK |
| Disconnect / no resume | Unchanged from REV3: `disconnect()` aborts that tab's run, and reconnect never resumes it. | OK (natively unverified since phase A) |
| Selected checkbox state and focus | Checkboxes are keyed by session id. Ineligible ones are shown `disabled` with the reason. Selected-but-not-open sessions are listed as "열려 있지 않음 — 제외". `withFocus(renderSide)` keeps focus via `data-fkey`. An empty selection on ▶ gives a toast and moves focus to the first enabled checkbox. | OK |

## Concrete finding
- **F5 (medium): JSON import silently drops a fixed target that refers to an imported custom session. The macro then follows the panel scope, which may be All, without notice.**
  - **Code:** `importData()` (l.1520-1541) pushes imported sessions with a new id (`nid('s-')`). It then filters each macro's `fixed` with `sess(sid)` against the old ids. A fixed target pointing at a session imported in the same file therefore never survives. The returned `{m, s, skipped}` and the UI say nothing about it.
  - **Repro (static):**
    1. In browser A, create a session `qa-bridge` (id `profile-…`) and a macro fixed to it.
    2. Export.
    3. In a profile without `qa-bridge`, import that JSON.
    4. Result: the session is created with a new id, the macro has `fixed: []` and no `고정` chip, and ▶ follows the panel scope (활성/연결된 모두/선택).
  - Built-in sessions (`s-mcu` and the other defaults) keep their ids, so default fixtures don't expose it. That is why the selfcheck "import adds macro" does not cover it.
  - **Expected:** remap fixed targets to the imported sessions' new ids (by old id or alias). If a target can't be resolved, keep a visibly unresolved `고정` state or report "고정 대상 N개 해제" in the import result. Never fall back silently to the panel scope.
- No other definite defects found in the requested focus areas.

## Native checks still required (S1–S10 in `qa/opus/STATUS.md`)
All scope behaviour above is static only. Browser acceptance needs Ego 15 control returned and the final hash. Add **S7b**: import an exported macro fixed to a custom session into a profile without that session, and assert that the fixed target is kept or the loss is reported visibly.

## REV6.1 F5 delta: provisional static re-review (on-disk app 1b3b0e1c; no rev6.1 manifest yet; final disposition pending the final hash)
- **Payload mapping** (`importData`):
  - `idMap` maps each payload session id to either a new local id (`nid('s-')`) or an existing session with the same alias.
  - Macro `fixed` and legacy `target` ids are remapped through `idMap`. Unmapped ids are **kept**, with no filtering and no fallback, and counted as `unresolved`. The import toast reports `remapped` and `unresolved`.
  - A payload id that collides with a local id but carries a different alias maps to the newly imported session, not the local one. Correct.
- **No silent widening:**
  - `deleteSession` no longer strips the deleted id from `fixed`. The macro becomes visibly 미해결 and blocked, with a toast.
  - `fixed` stays non-empty, so `macroRecipients` never falls through to the panel scope.
- **Atomic block:** `requestRun` blocks the whole macro if any fixed id has no stored session, whether the macro has one fixed target or several (known + unknown), and sends nothing. Every entry point goes through `requestRun` (▶/pane menu l.1693, hotkey l.1768, vars dialog l.1185), and no direct `runMacro` call bypasses it. The row chip gets a `u…` (unresolved) class, and the snapshot exposes `unresolvedFixed`.
- **Preview:** the head reads `고정 · 실행 차단(미해결)`. Observation (low, not a defect): with a mixed set the label continues `→ mcu · 제외 미해결 …`, so it still names a recipient although nothing will be sent. Suggest `→ 없음 (차단)`. Recipients shown and actual sends otherwise agree, because the head states that the run is blocked.
- **Provisional result: no definite defect in the F5 delta.** Record the final F5 disposition and the native S7b check against the final hash.

## REV6.1 FINAL F5 disposition: RESOLVED (static) — app cbc41b96 / styles e3a02266 / index 4c077401 (equals `opus/selfcheck/source-sha256-rev6.1.txt`)
- The final functions (`importData`, `macroRecipients`, `macroTargetLabel`, `requestRun`, `deleteSession`) were re-read. The logic matches the provisional review:
  - `idMap` remap is applied on import.
  - Unresolved fixed ids are kept and counted, never widened to the panel scope.
  - Deleting a session leaves it unresolved and blocked.
  - Any unresolved fixed id blocks the whole macro with zero sends, for every entry point.
- Owner evidence: `rev61-mixed.cjs` 3/3 (jsdom). Mixed `[aux, unresolved]` gives zero TX on every session, including the connected aux, plus a block toast. The full suite of 122/122 ran before the mixed-block correction.
- **No definite defect.** One observation is retained, low and with no source change requested: the mixed preview reads `고정 · 실행 차단(미해결) → aux · 제외 …`; the head states the block, but a recipient name still follows.
- This is static review plus owner jsdom only. **Native S1–S10 + S7b are still BLOCKED** (Ego 15 user-controlled). No browser PASS is claimed.
