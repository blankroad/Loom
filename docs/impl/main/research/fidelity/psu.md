# PSU fidelity — Keysight PathWave/BenchVue Power Supply → Loom `psu.gauge`

Status: **READY_TO_INTEGRATE — FROZEN** (MAIN review accepted; the gap-cause fix is applied). The worker makes no further edits. MAIN owns Ego, e2e, build and integration.
Owner: PSU fidelity worker, Claude **Opus 5.5** (`claude-opus-5-5`, self-reported by the runtime), foreground, no subagents, no git.
Base: main 26dc5e8. Accessed 2026-11-12. Commercial app **not run**, no hardware connected, no SCPI sent.

Evidence labels used below:
- **[V]** = seen directly in the official screenshot (I opened and inspected the image).
- **[D]** = stated in the official help text and not visible in a screenshot.
- **[I]** = my inference. It is unverified and must not be treated as reference behavior.
- **[C]** = current Loom code or the gateway, with file citation.

## 1. Primary reference visuals (stored, attributed)

All files below are in `work/reports/fidelity/assets/psu/`, with source URLs listed in `SOURCES.tsv`. Every file is an **official Keysight help screenshot (unaltered)**. They are kept only as review evidence and must never be shipped in Loom UI. There are no authored redraws yet.

| File | Help page | What it shows |
|---|---|---|
| `kh-pw-meter-view_more_779x479.png` | PW help › GUI/Meter View ("Instrument View") | 4-output N6705C demo. The callouts are 1 instrument selector, 2 output title bar, 3 readings, 4 basic settings + More, 5 Settings, 6 All On/All Off, 7 notifications, 8 Connected/pause. |
| `kh-pw-tour_1000x599.png` | PW help › Welcome (tour) | 2-output E36234A. The OVP/OCP block sits in the right half of each tile, and the view tabs are Instrument View / Data Logger / Arb List. |
| `kh-pw-data-log-features_732x486.png` | PW help › GUI/Data Log | Data Logger. Start/Stop are in the toolbar, with stacked V1/I1/P1 lanes on a shared time axis, chart tools, a preview/overview bar and a Marker Table. |
| `kh-pw-config_all_tabs_crop1_250x398.png` | PW help › Meter View Settings | Settings side nav: Source / Common / Output n (colour-swatched) / I/O Configuration. |
| `kh-pw-instrument-list_428x447.png` | PW help › Connect to Instruments | Select Instrument dropdown with Connected Instrument, filterable Available Instruments, refresh. |
| `kh-pw-connect_status{Y,N,V}.png` | same | Status glyphs: available, discovered-but-unavailable, virtual (demo). |
| `kh-pw-enable_limit_testing_409x168.png` | PW help › Data Log Settings | Limit testing. The upper/lower limits on a trace **turn that output OFF** when a limit is reached. |
| `kh-pw-deselect_instrument1_400x354.png` | PW help › Open Data Log | Opening a stored log (offline review). |
| `kh-html5-Log_Voltage_Current_637x256.png` | legacy BenchVue HTML5 help › Instrument Settings Tab | Legacy layout: Voltage Range, Show Advanced Options → Enable OVP + OVP Limit, and **Log voltage / Log current** check boxes at the tile foot. |

The PW help no longer has a separate "Instrument Settings" page. It merged that content into Instrument View plus the Settings side panel. The legacy HTML5 page is kept to show the trace-selection control.

## 2. Observed reference layout (what to be faithful to)

- **[V] Tiles side by side.** Each output is a full-height tile with a 1-px coloured frame and a filled number badge (1 yellow, 2 green, 3 blue, 4 purple), with the model next to the badge. The tiles sit left to right and share the width equally.
- **[V] Tile header.** It has a `Mode ▾` (Voltage priority) and `Status OFF` on the right, plus a toggle switch. The output state is shown both as **text under "Status"** and as the switch.
- **[V] Readback block.** This is the dominant element, taking about 40% of tile height. It is large, light-grey, right-aligned and stacked as **V / A / W**, with the unit in a separate aligned column (e.g. `11.914 mV · 1.000 mA · 0.012 mW`). Some models show only V and A (tour, 2-output). A setpoint is never shown in this block.
- **[V] Settings block below the readback.** It holds labelled boxed fields: `Voltage`, `Current` (or `Current Limit`, or `+/- Current Limit` + `Tracking limits`), `Range ▾`. A `∨ More / ∧ Less` disclosure reveals Measurement Range, `OVP Limit`, `☑ OCP`, `OCP Delay` and `OCP Delay Start ▾`. In the tour layout the OVP/OCP group sits in a **right sub-column separated by a vertical rule**.
- **[V] Global strip above the tiles.** It has `⚙ Settings  All On  All Off`. The view tabs are centred in the top bar, and the instrument/demo selector is at top-right.
- **[V] Status bar.** It shows `▣ Connected` at bottom-left (click = pause IO, [D]) and a notifications bubble at bottom-right.
- **[V] Data Logger.** Start ▶ (green) and Stop ■ (red) sit in the top toolbar. **Stacked lanes, one per trace** (V1, I1, P1), each has its own y-scale and a ground-glyph label, and all share an x "Time" axis in seconds. A minimap/overview bar with draggable range handles sits underneath. `Show ☐ Marker Table` is at the foot.
- **[D] Logging timing.** It is PC-clock driven, with the sample interval set to Minimum or Custom. Start is by button or at a set time, and stop is by button, elapsed time or sample count. By default only Output 1 is logged. Logging writes a file to disk.
- **[D] Disconnect and Pause.** Disconnect happens from the instrument list. Pause stops IO so the front panel can be used.

## 3. Fidelity matrix

| # | Reference area / control (evidence) | Actual interaction | Current Loom (file) | Proposed exact mapping | Retained Loom safety / data semantics | Intentional deviation / unimplemented backend |
|---|---|---|---|---|---|---|
| 1 | Output tile + colour frame + number badge (meter-view, tour [V]) | One tile per output, equal width | One `psu.gauge` per OPEN PSU alias in a `stack` grid [C `routes.ts:26`], plain text head [C `gaugePanel.ts:150`] | The panel root becomes a **framed tile**. A badge shows the alias (`rail`), the env chip `SIM` replaces the model number, and the frame colour comes from a panel-local palette indexed by the alias's stable order. | The alias is the identity, sorted stably; a tile never re-orders on update. | Tiles side by side would need the shell grid `stack`→`row`. That is a **shared change** (D1). Until then tiles stack vertically. No Keysight colours or logos are used. |
| 2 | Header `Status ON/OFF` text + toggle (meter-view #2 [V]) | Click the toggle → that output on/off | `출력 ON/OFF` button + R3 badge in the ctl row; head text `● 출력 ON` [C] | The header right side shows a state word (`출력 ON` / `출력 OFF` / `출력 ?`) above a **switch-styled button** (`role="switch"`, `aria-checked`) with the **R3 badge kept**. | `operate` right + write-lock gate unchanged. The button is **disabled when `out===null`** (no guessing). | BenchVue shows no risk badge; Loom keeps one. |
| 3 | (no reference equivalent) | — | `! 비상 OFF` in head, watch right, no confirm, `!` key [C `gaugePanel.ts:44,215`] | Keep it in the tile header to the **left of the switch**, styled as danger and visually separate from the ordinary toggle. | Safety direction: always enabled, even under write lock, with **channel-scoped** `{channel}`. Global STOP does **not** call it. The `STOP — 출력 유지 중` badge stays. | Loom-only. It is not a fidelity element and must not be merged into the switch. |
| 4 | Readback block V / A / W, large, right-aligned, unit column (meter-view #3 [V]) | Read-only, live | Single text line `설정 … · 실측 …` [C `gaugePanel.ts:160`] | Add a **readback block** in large tabular-num digits, right-aligned, with units in their own column. The `실측` caption is small, and below it go `age N ms`, `stale` (>2× expected interval [I], threshold from `poll_hz_actual` if available) and the sample count. | Missing values show `—` and are never 0. **P is labelled `(계산)` when derived** from V×I [C `psuModel.ts:52`] rather than reported. No synthetic telemetry. | Fixed 3 decimals instead of auto-scaled mV/mA/µA engineering units [I: BenchVue auto-scales]. Engineering formatting is panel-local, so it is OK to add. |
| 5 | Mode ▾ (priority) (meter-view #2 [V]) | Selects Voltage/Current priority | `mode` readback appended to the text line [C] | Read-only **mode chip** `CV` / `CC` / `—` beside the state word. It renders only when `psu.state.mode` is present. | It never infers CV/CC from V/I. | **No priority selector.** There is no RPC for it (`psu.set` takes volt/curr only [C `gateway.rs:306`]). |
| 6 | Voltage / Current setting fields below the readback (meter-view #4, tour [V]) | Boxed numeric fields. Apply timing is **not documented** (the legacy tip describes mouse-wheel step [D]) | Unlabelled `V` `A` inputs + `설정` button + R2 [C] | A labelled settings group: `전압 설정 [__] V`, `전류 제한 [__] A`, each showing the **current setpoint as a ghost value**. The `vmax`/`imax` hint comes from `psu.status`. The explicit `적용` button and R2 stay. | Typing never sends an RPC. Apply requires `operate`. Server limit violations surface as a toast and are not clamped client-side. | Explicit apply is a deliberate deviation for safety. No mouse-wheel stepping (it would risk accidental writes). |
| 7 | More ▾: Range, OVP Limit, OCP, OCP Delay (meter-view #4, tour, html5 [V]) | Disclosure reveals protection settings | `보호 ok/ovp/ocp/otp/unknown` text [C `model.rs:27`] | The disclosure is `보호 ▾`. Collapsed, it shows a **protection status chip** (`보호 정상` / `OVP` / `OCP` / `OTP` / `?`), with a warning style when not ok. Expanded, it shows a read-only line: `OVP/OCP 설정은 이 백엔드에서 미지원`. | The protection state is a separate field from mode and from output. | No OVP/OCP/range/delay **write controls**. No RPC exists, so none of these is rendered as working. |
| 8 | Data Logger stacked V/I/P lanes, shared time axis (data-log [V]) | Live chart while logging, with hover readout | One uPlot, dual y-axes V/A, 60 s ring, `spanGaps:false` [C `gaugePanel.ts:95`] | Move to **stacked lanes, one per quantity** (V, then I, then P only if reported or labelled as calculated), with a shared x axis in seconds. Each lane has its own scale, and hover cursor sync uses uPlot `cursor.sync`. | A `source.gap` breaks the line in **every** lane, and the loss count stays in the footer. The window stays labelled `60 s 창 (메모리)`. | No preview/minimap bar, markers or Marker Table (see row 9). |
| 9 | Data Logger Start/Stop, file name, interval, stop condition (data-log, Data Log Settings [D/V]) | Start/Stop writes a log file on the PC | None. The 60 s memory ring only [C] | **Not mapped in the panel.** The footer says `기록은 공용 녹화 기능 사용` only if the shared recording API exists (D2). | The 60 s plot is never called a "log". | Log file, interval, scheduled start and export are unimplemented here. They go to MAIN as shared recording/generation. |
| 10 | Log voltage / Log current checkboxes (html5 [V]); Trace Logging Selection [D] | Chooses the traces to chart/log | None | Lane visibility toggles `V ☑ I ☑ P ☐` in the chart header. They are **display-only** and persist per panel props. | Hiding a lane does not stop ingesting it. | It is not a logging selection (row 9). |
| 11 | All On / All Off (meter-view #6 [V]) | Enables/disables all outputs of the instrument | None in UI. The gateway supports channel-less `psu.out{state:off}` (per existing research) | **Not in the per-channel panel.** It is proposed to MAIN as a service-level `모든 PSU 출력 OFF` (safety direction) **and no All On** (D3). | It is distinct from global STOP. STOP still does not touch outputs. | All On is intentionally omitted because enabling many outputs at once is high-risk. |
| 12 | Instrument selector + status glyphs; Connected/pause in status bar (instrument-list, connect_status [V], Connect page [D]) | Discover/select/connect/disconnect, pause IO | Shell connection explorer / channel bar (shared). The panel shows `[SIM]` [C] | The panel **footer status bar** shows `▣ 연결됨 · sim` (from shared channel state, read-only), the gap count and the sample count. Connect/disconnect stays in the shell. | Connected and output are separate states. Leaving the service or closing the tab never disconnects or turns the output off. | No pause-IO. `psu.connect` is sim-only [C `handlers.rs:76`]. There is no real-instrument discovery. |
| 13 | Limit testing → output OFF; alerts on CC/CV/protection (enable_limit_testing [V], Data Log Settings [D]) | Automation that turns outputs OFF | Gateway evidence/wait conditions exist (`evidence.rs`), but no UI | **Out of scope** for this panel phase. It is listed so it is not silently "implemented" as a UI-only alert. | An automated output write must go through ActionGate/test runtime and must not be a panel timer. | Unimplemented. Arb List is also out of scope (no list RPC). |

## 4. Concrete panel layout plan (single tile, panel-local)

```
┌─[■ rail]  SIM  CV        출력 OFF  [! 비상 OFF] [◯━ 스위치] R3 ┐  ← header (row 1–3,5)
│                                      실측                       │
│                           12.003   V                           │  ← readback block,
│                            0.481   A                           │    ~40% height, right-aligned,
│                            5.77    W (계산)                     │    unit column; "—" if null
│                           age 120 ms · 보호 정상                │
├────────────────────────────────────────────────────────────────┤
│ 전압 설정 [ 12.00 ] V   전류 제한 [ 1.000 ] A   [적용] R2       │  ← settings (row 6)
│ 한도 30.0 V / 3.0 A                    보호 ▾ (OVP/OCP 미지원)   │  ← row 7
├────────────────────────────────────────────────────────────────┤
│ 60 s 창 (메모리)                 표시: V☑ I☑ P☐                 │
│ V ─────────╮   ╭──────                                         │  ← stacked lanes,
│ A ───────  ╰ gap ╯ ─────                                       │    shared x, sync cursor
│                                                    t (s)       │
├────────────────────────────────────────────────────────────────┤
│ ▣ 연결됨 · sim   측정 손실 0회   312점   [STOP — 출력 유지 중]   │  ← footer status bar
└────────────────────────────────────────────────────────────────┘
```

- **Narrow (<420 px).** The readback digits step down one size. The settings fields wrap to 2 rows. The header switch stays, and the emergency OFF button is never hidden.
- **Files touched on GO (owned only).**
  - `ui/src/panels/psu/gaugePanel.ts`: DOM restructure.
  - `psuModel.ts`: engineering formatting, `pDerived` flag, `stale` helper, multi-lane series.
  - New `ui/src/panels/psu/psu.css`: panel-scoped `.psu-panel …`, using only existing tokens.
  - `psu.test.ts`: added tests.
  - Manifest: add `queries:['psu.status']` for limits/poll Hz, if the SDK query path supports it. Otherwise the limits hint is omitted.

## 5. Acceptance checks (to run on GO; jsdom unit + main's Ego run)

1. With no samples, all readback cells render `—` with units. No `0.000` appears.
2. A `psu.state` without `mode` renders no mode chip. With `mode:"CC"` it renders `CC`.
3. P from `metrics.power` has no `(계산)` suffix. P from V×I has the suffix.
4. With `out===null`, the switch is disabled and the state word reads `출력 ?`.
5. With write lock or `watch`-only rights, the switch and `적용` are disabled, and `! 비상 OFF` is enabled. Clicking `! 비상 OFF` calls `emergencyOff` with `{channel, state:'off'}`.
6. With `stopLatched && out===true`, the footer shows `STOP — 출력 유지 중`, and **no** `psu.out` is issued by STOP.
7. Typing in the settings fields issues no `act`. `적용` issues one `set` with only the filled fields. Invalid input shows a toast and issues no act.
8. A `source.gap` inserts a null in every lane, and the loss count increments.
9. Lane toggles hide or show the plot series without changing series length or ingest.
10. No OVP/OCP/range/All On control exists in the DOM as an enabled input. There is no Keysight logo or wordmark, and no `PathWave`/`BenchVue` string, in the UI.
11. Two PSU aliases produce two tiles. Each tile's actions carry only its own channel.
12. The existing `psu.test.ts` assertions still pass unmodified (no test weakening).

## 6. Shared / cross-boundary dependencies (MAIN decides; I will not edit)

- **D1.** Shell grid for `psu-only` (`stack` → horizontal tiles `row`/auto-fit) in `ui/src/shell/routes.ts`. Without it, tiles stack.
- **D2.** Shared recording/generation API to map Data Logger Start/Stop. Until then, there is no logging UI in PSU.
- **D3.** Service-level "all PSU outputs OFF" (safety direction, `watch`, gateway channel-less `psu.out off`) as a shell/service action. All On is explicitly omitted.
- **D4.** Whether the panel SDK exposes `queries` (`psu.status` → `vmax/imax/poll_hz_actual`) to panels today. If not, the limits hint and the stale threshold are dropped.
- **D5.** Per-alias accent colours: if MAIN wants them as design tokens rather than panel-local CSS vars.
- **No gateway/RPC change is requested.** OVP/OCP/mode-priority/range writes stay unimplemented by design.

## 7. Open uncertainties

- BenchVue's apply-on-enter versus apply-on-blur behaviour is not documented. It is not inferred as reference behaviour.
- The stale threshold (2× poll interval) is my proposal, not a reference.
- Real instrument channel structure and protection latching are only known from the sim (`sim.rs`).

## 8. Implementation record (PHASE2, after MAIN GO)

MAIN decisions applied:
- **D1:** shell layout unchanged. Each alias is its own tile; the `psu-only` `stack` grid stays.
- **D2:** no recording controls. The trend is explicitly labelled `60초 관측 · 실측 추세 · 메모리 창 (저장 기록 아님)`.
- **D3:** no service-level all-OFF. The channel emergency OFF is unchanged.
- **D4:** `psu.status` is **omitted**. The query API exists, but the browser mock server (`ui/tools/mock/server.ts`) does not implement `psu.status`. A mount-time query would also add network traffic to every tile. As a result, there is no vmax/imax hint and no poll-rate-based stale threshold.
- **D5:** a panel-local accent `--psu-accent: var(--accent)`. I deliberately did not reuse the origin colours (`--agt/--usr/--scr`) so the accent does not imply origin meaning.

Files changed (all owned):
- `gaugePanel.ts`: DOM restructure, same manifest/actions.
- `psuModel.ts`: `pDerived`, `eng`, `parseSetpoint`, `modeLabel`, `protLabel`, and a P lane in `Series`, which stays backward compatible.
- `psu.css`: new. Every rule is scoped under `.psu-tile`.
- `fixtures.ts`: exports `rec`.
- `psu.test.ts`.

The legacy `.psu-*` rules in `shell/shell.css` are untouched; the scoped rules override them by specificity.

### Implemented mapping (rows from §3)

| Row | Implemented as | Retained semantics |
|---|---|---|
| 1 tile | `.psu-tile` frame + `PSU [alias] [SIM]` badge | One tile per alias; the shell decides placement |
| 2 status + toggle | `psu-out` word (`● 출력 ON` / `○ 출력 OFF` / `출력 ?`) + switch-styled button `psu-output-toggle` (`aria-pressed` true/false/mixed) + R3 | `operate` right, write-lock gate; disabled and no-op when the state is unknown |
| 3 emergency | `psu-emergency` in the header, red, left of the switch | `watch` right, no confirmation, `{channel, state:'off'}`; `!` key, ignored inside inputs |
| 4 meter | `psu-meter` with V/A/W rows, large tabular digits, right-aligned, unit column, auto m-prefix below 1 | `—` when unknown; W `(계산)` when V×I is derived; after a `source.gap` with no newer sample the last value is greyed and labelled `측정 손실 후 미갱신` (never zeroed) |
| 5 mode | `psu-mode` chip, hidden unless `psu.state.mode` is present | Never inferred |
| 6 setpoints | `psu-setpoints` block (`전압 설정`, `전류 제한`) separate from the meter; form `psu-set-v` / `psu-set-i`, setpoint shown as the placeholder, `미적용` pending note, `적용` + R2 | No RPC on typing or Enter; invalid or negative input is flagged and toasted with no RPC; the draft clears only on success |
| 7 protection | `psu-prot` chip `보호 정상/OVP/OCP/OTP/?`, warn style when not ok | Readback only; no OVP/OCP inputs |
| 8 trend | `psu-trend`: stacked lanes V/A/W, uPlot per lane with a shared per-tile cursor sync key, `spanGaps:false` | A gap inserts a null in every lane; loss count shown in the footer |
| 10 lane toggles | `psu-lane-v/i/p` (`aria-pressed`), W off by default | Display only; ingest and point count unchanged |
| 9/11/12/13 | Not implemented (per D2/D3, no backend) | — |

### Frozen browser selectors (`data-testid`)

The existing selectors are kept:
- `psu-head`: contains the output word, `STOP — 출력 유지 중` and the `비상 OFF` button, as e2e `test-screen.spec.ts` requires.
- `psu-vals`: contains the setpoint text, e.g. `12.00`.
- The classes `.psu-panel`, `.psu-stop`, `.psu-plot` and `.psu-emergency` are also kept.

New selectors:
- Header: `psu-badge`, `psu-out`, `psu-mode`, `psu-prot`, `psu-stop`, `psu-emergency`, `psu-output-toggle`.
- Meter: `psu-meter`, `psu-meter-v`, `psu-meter-i`, `psu-meter-p` (children `.psu-num`, `.psu-unit`, `.psu-note`), `psu-meter-age`.
- Setpoints: `psu-setpoints`, `psu-setpoint-v`, `psu-setpoint-i`.
- Set form: `psu-set-form`, `psu-set-v`, `psu-set-i`, `psu-apply`, `psu-set-pending`.
- Trend: `psu-trend`, `psu-lane-v`, `psu-lane-i`, `psu-lane-p` (toggles), `psu-lane-row-v`, `psu-lane-row-i`, `psu-lane-row-p`.
- Footer: `psu-foot`.

Button text: the apply button changed from `설정` to **`적용`**. The action id and label `set` and the RPC `psu.set` are unchanged. No other repository test or e2e test selects `설정`.

### Deviations from the reference (intentional)

- Settings apply only through an explicit `적용` button: there is no apply-on-enter or blur, and no mouse-wheel stepping.
- The toggle keeps the R3 badge, and its label is the action (`출력 OFF` while on).
- The channel emergency OFF has no reference equivalent.
- The following are omitted: the priority-mode selector, Range, OVP/OCP settings, More/Less, All On/Off, Data Logger Start/Stop, file naming, the minimap, markers, Arb List and the pause-IO control.
- Tiles stack (D1), there is a single accent colour (D5), and there are no Keysight colours, logos or names. A test asserts that no `BenchVue`, `PathWave` or `Keysight` text appears.
- Lane visibility is not persisted. There is no `setProps` path in the panel; this is a deliberate omission.

### Verification

- `npx vitest run src/panels/psu`: **16/16 passed** after the review fix. That is 14 panel interaction tests and 2 model tests.
- `npx tsc -b`: clean. `npx eslint src/panels/psu`: clean.
- Full `npx vitest run`: 458 tests passed, but 5 suites **failed to load**, all on `Failed to resolve import "./uart.css" from src/panels/uart/terminalPanel.ts`. That is a concurrent UART-owner edit and unrelated to PSU; I did not touch it.
- Not run by me: Playwright e2e, the actual Ego/browser visual check and the vite build (MAIN owns these). The real-canvas uPlot lane rendering has therefore **not been visually verified**. jsdom takes the `캔버스 없음` path.

### Handoff requests to MAIN
1. In the Ego run, check that the lanes render at tile width and that cursor sync works across V/A/W.
2. On narrow widths (<420 px), check that the meter digits clamp and that the emergency button stays visible.
3. Run e2e `psu.gauge (SimPsu bench)` against the mock daemon (the selectors are unchanged).
4. Optional shared follow-up: add `psu.status` to the mock server so a later pass can show vmax/imax hints (D4).

## 9. Review fix + freeze (READY_TO_INTEGRATE)

- **Fix.** The footer no longer labels every `source.gap` as an SCPI no-response.
  - It now reads `측정 손실 N회 (<cause> n, …) — 선을 끊어 표시`.
  - The cause comes from `source.gap.payload.cause`: `scpi_no_response` is shown as `SCPI 무응답`, any other cause is shown raw, and a missing cause is shown as `원인 미상`.
  - The total gap count and the null break inserted in every lane are unchanged. The fix is in `psuModel.gapCauseLabel/gapCauseSummary` and `gaugePanel.ts`.
- **Tests.**
  - The existing gap test now asserts `측정 손실 1회 (SCPI 무응답 1)`.
  - A new focused test mixes `scpi_no_response`, `ring_overflow` and a missing cause, then asserts the per-cause summary, the total of 3, and 6 points (3 samples + 3 break points).
  - The model test covers the label mapping.
  - No e2e or other source references the old footer text.
- **Final files (owned):**
  - `ui/src/panels/psu/gaugePanel.ts`
  - `ui/src/panels/psu/psuModel.ts`
  - `ui/src/panels/psu/psu.css` (new)
  - `ui/src/panels/psu/fixtures.ts`
  - `ui/src/panels/psu/psu.test.ts`
  - This doc
  - `work/reports/fidelity/assets/psu/*` (reference evidence only, not shipped)
- **Final checks:**
  - `npx vitest run src/panels/psu`: 16/16 passed.
  - `npx tsc -b`: clean.
  - `npx eslint src/panels/psu`: clean.
  - Full suite at the last run: every test that ran passed, but 5 suites failed to load on the UART owner's missing `uart.css` (not PSU).
- **Remaining for MAIN:**
  - Ego visual check of the lane canvases and narrow widths.
  - e2e `psu.gauge (SimPsu bench)`.
  - vite build.
  - Report and commit integration.

### MAIN native integration correction
Actual Ego found native set/output writes succeeded while the panel's prior push-only readback remained stale. MAIN added a channel-scoped, non-overlapping 1 Hz `psu.status` query (already registered by the daemon) to reconcile authoritative state and advance native simulator polling. It never synthesizes measurements from a setpoint. The query is aborted and timer cleared on panel disposal. A focused test covers alias parameters, confirmed state versus unknown measurements, and disposal. MAIN also improved dark-theme chart axes and lane height; actual Ego canvas rendering is checked separately. Native browser set=12 V, output ON and emergency OFF were observed successfully; aggregate final evidence takes precedence over earlier phase checkpoints.
