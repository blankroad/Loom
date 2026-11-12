# UART fidelity — CoolTerm (primary) → Loom `ui/src/panels/uart`

Status: **IMPLEMENTED — FROZEN for MAIN review** (Phase 1 plan + product GO pass; see §9) · owner: UART fidelity worker · model: Claude Opus 5.5 (`claude-opus-5-5`, verified from session model ID) · base `26dc5e8` · 2026-11-12
No product source touched. No subagents, no git, no hardware, CoolTerm not run.

Evidence labels: **[V]** seen in an official screenshot · **[D]** stated in the official help text · **[I]** our inference · **[L]** fact from Loom code.

## 1. Sources inspected

| ID | Source | What it shows |
| --- | --- | --- |
| S-mac / S-win / S-linux | `work/reports/fidelity/assets/uart/coolterm-official-{mac,win,linux}.png` (official screenshots, from <https://freeware.the-meiers.org/CoolTerm_Screenshot.html>; attribution in `SOURCES.md`) | Three stacked windows titled `Plain Text Mode.stc`, `Hex Mode.stc` and `Chart Mode.stc`, a **Send String** window, the toolbar, and the status bar with LEDs. Same layout on all 3 OSes. |
| H | <https://www.freeware.the-meiers.org/CoolTermHelp/> (single-page official manual; it contains no UI screenshots) | Quick Start, view modes, copy/paste, status LEDs, menus, Serial Port options, Terminal Mode Raw/Line, Enter Key Emulation, Send String, Toolbar, Status Bar options |
| L | `ui/src/panels/uart/{terminalPanel,tracePanel,uartState}.ts`, `crates/loom-uart/src/commands.rs`, `crates/loom-uart-worker/src/worker.rs` | Current Loom behavior and backend contract |

Not used as primary sources: Tera Term and PuTTY. They are cited only in the earlier `../benchmark-uart.md`.

## 2. Exact reference layout ([V] unless marked)

```
┌ title: "<settings-file>.stc"  (one window = one port/connection) ─────────────────────────┐
│ TOOLBAR (icon above label on mac/win; icon beside label on Linux, with a ▾ overflow at the right edge)   │
│ [New][Open][Save]  ·  [⇄ Disconnect]  ·  [Options]  ·  [✕ Clear Data]  ·  [View ▾]  ⟷flex⟷ [Macros ▾][Help] │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ DATA AREA. Plain: white, monospace, flush-left "This is a test".                                │
│ Hex: rows with light-blue zebra stripes; left = hex bytes "54 68 69 73 20 …";                    │
│      a vertical rule; right = the ASCII "This is a test"; the current row is tinted.                  │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ ▾ usbserial-FTDGUV4P / 115200 8-N-1                       ● TX  ● RTS  ● DTR  ● DCD          │
│   Connected 00:10:56, 32,418 / 32,356 bytes               ● RX  ● CTS  ● DSR  ● RI           │
└──────────────────────────────────────────────────────────────────────────────────────────┘
Send String window: title "Send String (Hex Mode.stc)"; (○ Plain ◉ Hex) at top left, [Send] at top right,
then a multi-line monospace text box with "54 68 69 …".
```

- The status bar has two lines: `port / baud data-parity-stop` on top and `Connected HH:MM:SS, <n> / <n> bytes` below. [V] The help lists the connection timer and send/receive byte counts as status-bar options. [D]
- The LED grid is 4 columns × 2 rows. Green means active and dark means inactive. **RTS** and **DTR** have bold labels, which marks them as clickable toggles. [V] Clicking TX toggles BREAK; a red RX LED means a framing or BREAK error. [D]
- `View` is a menu button (Plain / Hex / Chart). The display mode applies to the whole window, and **typing still sends keys in every mode**. [D]
- Raw Mode sends every key immediately. Line Mode shows a command-line field inside the window that sends one line on Enter, with ↑/↓ history and a history button. **There is no screenshot of Line Mode; this is [D] only.**
- Enter Key Emulation (CR / LF / CR+LF / None / Custom hex) applies to keys and to Send String. Shift+Enter sends CR, Alt+Enter sends LF, and Shift+Alt+Enter sends CR+LF. [D]
- Send String: the Plain/Hex choice can switch at any time. Hex needs 2 digits per byte, spaces optional. Shift+Enter sends. **Send is disabled while the port is closed.** Several Send String windows can be open at once. [D]
- Copying in Hex view puts `48 65 6C …` on the clipboard. Pasted data is sent immediately. [D]
- The port cannot be changed while the connection is open; other settings can. Connect and Disconnect use the same toolbar toggle. [D]
- Clear Data clears the receive buffer. Pause Display pauses only the screen. Capture has Start/Stop/Pause/Resume. [D]

## 3. Current Loom facts relevant to the mapping ([L])

- The header is `UART <ch> ● <status> <baud> …`, followed by the input-lease chip and the buttons `반환` (return) / `지우기` / `가져오기` (take) / `HOLD` / `⏸` / `키 ▾`. The footer shows the last send and its stage, expect, rx B/s, blocked counts, `화면 생략` (dropped on screen), `REC <health>`, and watch/STALLED.
- **Name collision:** the `지우기` button calls `uart.input.clear` (it clears the pending input line). It is **not** a view clear like CoolTerm's "Clear Data".
- The `uart.config` event already carries `baud`, `data_bits`, `parity`, `stop_bits` and `flow`. `uartState.reducePortState` keeps only `baud`.
- `uart.signal` takes `{kind: break|dtr|rts, value?, duration_ms?}`. The daemon hook classes it as R2 ("line signal"). The panel action registry sets `confirm: 'none'` for it.
- **No backend reports CTS / DSR / DCD / RI, or the actual DTR/RTS levels.** No daemon-authoritative TX/RX byte counter was found; the panel computes rx B/s from the events it sees.
- `tracePanel` receives the raw bytes (`decodePayloadBytes`) before it symbolizes them, so a Hex projection is possible without backend work. It currently keeps only text rows (a 20k-row ring).
- `uart.input` is a stream with `data_hex` limited to 8192 hex chars, protected by the input lease. Larger input goes through `inputLarge` (request + confirmation card).

## 4. Fidelity matrix

| # | Reference area / control (evidence) | Actual reference interaction | Current Loom | Proposed mapping (exact) | Retained Loom safety / data semantics | Intentional deviation / backend gap |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Window = one connection; title is the settings name [V S-*] | One window per port | One `uart.terminal` tab per alias (`mcu`, `aux`) | Keep the tab per alias. The panel's first toolbar slot shows the alias as a bold label, plus the port path as a secondary label. | The alias is the target for every action. Switching tabs never opens/closes the port or moves the input lease. | No "New/Open/Save" settings files. Loom profiles are owned by the daemon and connection explorer. |
| 2 | Toolbar: icon over label, flex spacer, ▾ overflow on narrow width [V S-mac, S-linux] | Click actions; `…Menu` buttons open pop-ups [D] | Text buttons inline with the port text | A panel-local **toolbar row**: `[View ▾ Terminal/Hex] [Send String…] [Clear view] [Pause]` ⟷ `[lease chip][Take][Return][HOLD]` `[Keys ▾]`. Icons sit over 11px labels at ≥720px, show icon-only with a tooltip below that, and collapse into a `▾` overflow under 480px. | Lease/HOLD buttons keep their current RPCs, disabled states and Ctrl-] shortcuts. | Connect/Disconnect and Options are **not** panel buttons (see #9). No Macros/Help. |
| 3 | Plain-text view [V] | Shows text as it arrives; no VT emulation [D] | xterm VT (interprets escapes, blocks automatic replies) | **Keep the xterm VT** as the "Terminal" view (the default). | VT reply blocking, `화면 생략`, gap handling. | Deliberate: Loom is a VT console (it serves the user's boot shells). CoolTerm's "Filter ANSI" option has no equivalent. |
| 4 | Hex view: zebra rows, hex ⎸ ASCII, current-row tint [V S-mac/win/linux] | Window-wide mode; keys still sent [D] | None | `View ▾ → Hex` swaps the panel body for a **HexView**: 16 bytes per row, columns `time · dir(RX/TX, origin) · hex · │ · ASCII` ("." for non-printables), zebra stripes, and the selected row tinted. The xterm stays mounted (hidden) so its state persists. The buffer is a byte ring (proposed 1 MiB) with an evicted-byte count. Gap rows are shown as separators. | RX and TX are never merged into one row. Gap and eviction are visible. The view shows only what this panel received; it is not the journal. | Hex view is **read-only for keystrokes** (keys do not send). The status chip says "Hex view: typing goes to Terminal/Send String". Reason: no hidden key path. Deviates from [D]. |
| 5 | Copy in Hex → `48 65 6C …` [D] | Copy matches the current view mode | xterm copy only | Copying a HexView selection produces space-separated uppercase hex. Copying in Terminal view is unchanged. | Large paste still goes through the `inputLarge` card. | "Hex clipboard pastes as binary" is **not** adopted (it would be a hidden send path). |
| 6 | Send String window: Plain/Hex radio, Send at top right, multi-line box, target in the title [V S-*] | Shift+Enter sends; Send disabled while port closed; Enter emulation applies [D] | None (keys only) | A **Send String drawer** docked under the terminal, collapsed by default. Its title is `Send String (mcu)`. It has `(○ Plain ◉ Hex)` and a `Line end: None/CR/LF/CRLF` selector beside `[Send]`, a monospace textarea, and a **final-bytes preview** (`n B: 54 68 … 0D`). Shift+Enter or Enter-with-modifier sends. Invalid hex shows an inline error and disables Send. | Sends through the **same `uart.input` path and input lease**: no lease or a STALLED/watch state disables Send. Over 4096 B goes through the existing `inputLarge` confirmation card. The send shows in the footer's last-send line. | One drawer per panel, not several windows. The "Terminate Send String" preference is merged into the line-end selector. |
| 7 | Raw vs Line terminal mode [D only; no screenshot] | Line: field in the window, Enter sends + Enter-emulation, ↑↓ history | Raw only (xterm) | **Raw stays the default and the only keystroke mode.** Line Mode becomes a **drawer option** `Enter sends line` with ↑/↓ history (in memory, per alias, not persisted). The Terminal never silently turns into a composer. | Keystrokes in xterm stay immediate under the lease. The drawer path is visibly separate. | No separate window-level Line Mode: two keystroke models would conflict with input arbitration. |
| 8 | Enter Key Emulation and Shift/Alt+Enter overrides [D] | Configurable bytes for Enter | xterm sends CR (default VT behavior) | Terminal view: a read-only hint `Enter→CR` in the toolbar. The configurable line end applies **only** to the Send String drawer. | VT key encoding is unchanged. | Shift/Alt+Enter overrides are not adopted in the VT, because they would conflict with xterm key handling. |
| 9 | Status bar line 1: `port / 115200 8-N-1` [V] | Shows settings; ▾ disclosure | `UART mcu ● open 115200 gen1 [SIM]` in the header | A panel **status bar** at the bottom. Line 1: `mcu · /dev/ttyUSB0 / 115200 8-N-1 · open · gen 1 · SIM`, formatted from the full `uart.config`. The ▾ disclosure expands the lease/expect/agent details. A **`Options…` link** opens this alias in the shared connection explorer. | Generation and env (SIM/real) stay visible. Settings cannot be edited in the panel. | Changing port/baud stays with the connection explorer (a shared-shell dependency). No Connect/Disconnect toggle in the panel, because port lifecycle is shared state. |
| 10 | Status bar line 2: `Connected 00:10:56, 32,418 / 32,356 bytes` [V] | Timer and byte counts | rx B/s and other facts in the footer | Line 2: `open for HH:MM:SS (since uart.state seen) · TX n B / RX n B (this view)` plus the existing rx B/s, `화면 생략`, blocked counts and `REC <health>`. | The counts are labeled **"this view"** and reset on restore or a gap. They are never shown as device counters. | Gap: no daemon-authoritative byte counters. Proposed for main/backend. |
| 11 | LED grid TX RX / RTS CTS / DTR DSR / DCD RI; bold = clickable [V] | Click RTS/DTR toggles the line; click TX toggles BREAK [D] | `Break` in `키 ▾`, `uart.signal`, confirm `none` | **TX/RX activity LEDs** derived from `uart.data` events (a short pulse, label `activity`). **RTS/DTR** shown as explicit buttons whose state is `last set: on/off/unknown`. CTS/DSR/DCD/RI are **not rendered**. Break stays a **labeled button** (not hidden on an LED) and shows the target alias. | Break/RTS/DTR go through `uart.signal` with an operate right. They are disabled in watch/STALLED. Proposal: set confirm to `card` for RTS/DTR toggles (R2: they can reset MCUs). | Gap: there is no modem-input readback (CTS/DSR/DCD/RI) and no DTR/RTS readback. Never draw them as live. |
| 12 | Clear Data / Pause Display [D; Clear Data V] | Clear the receive buffer; pause only the screen | `지우기` = `uart.input.clear`; `⏸` = local pause | Add a **`Clear view`** button (xterm.clear plus a HexView reset; local only). Rename the existing `지우기` to **`입력 줄 지우기`** (clear input line), with its tooltip naming `uart.input.clear`, and move it into the lease group. `Pause` keeps today's behavior. | The journal and REC are never cleared. Pause keeps receiving and recording. | Capture Start/Stop is **not** mapped: Loom records continuously, and `REC` shows health, not a toggle. |

## 5. Concrete panel layout plan (`uart.terminal`)

```
┌ toolbar ─────────────────────────────────────────────────────────────────────────┐
│ mcu  /dev/ttyUSB0 │ [▤ View▾] [⇪ Send String] [⌫ Clear view] [⏸ Pause]   ⟷   [● YOU have input ▾ Take/Return/입력 줄 지우기] [HOLD] [Keys▾] │
├ body (Terminal = xterm VT | Hex = HexView) ─────────────────────────────────────────┤
│                                                                                  │
├ Send String drawer (collapsed; opened by toolbar) ──────────────────────────────────┤
│ Send String (mcu)   (○ Plain ◉ Hex)   Line end [CR ▾]  ☐ Enter sends line   [Send] │
│ ┌textarea mono┐   preview: 15 B  54 68 69 73 20 … 0D        error: "odd hex digit" │
├ status bar ─────────────────────────────────────────────────────────────────────────┤
│ ▾ mcu · /dev/ttyUSB0 / 115200 8-N-1 · open · gen 1 · SIM   Options…      ●TX ●RX  RTS: last on  DTR: unknown  [Break] │
│   open 00:10:56 · TX 1,204 B / RX 32,356 B (this view) · rx 1.2 KB/s · REC ok · AGT ▶ "reboot" acked │
└───────────────────────────────────────────────────────────────────────────────────┘
```
- At 390px the toolbar shows icons only plus a `▾` overflow (as in S-linux). The alias and the lease chip are always visible. The drawer opens as a bottom sheet. The status bar line 1 truncates the path first and always keeps the alias and `8-N-1`.
- Files (all inside `ui/src/panels/uart/`): `terminalPanel.ts` (toolbar, status bar, drawer wiring), new `hexView.ts`, `sendString.ts` (pure parse/encode/preview + DOM), `uartState.ts` (keep full config, counters), panel-local `uart.css` (scoped `.uart-panel …`), tests in `uart.test.ts` plus new `hexView.test.ts` and `sendString.test.ts`.

## 6. Acceptance checks (for GO)

1. Unit test: `uart.config {baud:115200,data_bits:8,parity:none,stop_bits:"1"}` renders as `115200 8-N-1`. Even and odd parity render as `E`/`O`. Missing fields show `?` and are never invented.
2. The Send String parser accepts `54 68`, `5468` and `54-68`. `5` and `GZ` fail with a message and Send stays disabled. The preview equals the bytes passed to `act('input', {data_hex})`. Line end is appended exactly (CR=0D, LF=0A, CRLF=0D0A, None).
3. The drawer Send is disabled when there is no lease, in watch mode, when STALLED, or when the port status is not `open`. Over 4096 B routes to `inputLarge` (card). The xterm keystroke path is unchanged (existing tests stay green, none weakened).
4. HexView: 16 B/row. RX and TX rows are separate. A gap event inserts a separator row and resets the line. A non-printable byte shows `.` in the ASCII column. Copying a selection gives `48 65 6C`. Ring eviction shows `evicted N B`.
5. Switching View Terminal↔Hex keeps the xterm content and lease state. Keys typed in Hex view send nothing (asserted with an action spy).
6. `Clear view` makes no RPC. `입력 줄 지우기` calls `uart.input.clear` only. REC and the journal are untouched.
7. The LED area never renders CTS/DSR/DCD/RI. RTS/DTR show `unknown` until the user sets them. Break and RTS/DTR are disabled without the operate right.
8. 390px: the alias, lease chip, `View` and the overflow are visible; the drawer is reachable; no horizontal scroll (checked by main in Ego).

## 7. Shared / main dependencies (not editable by this worker)

- **D1 (shell):** a deep link from the panel's `Options…` to the connection explorer for this alias. A route or event in the shared shell is needed.
- **D2 (ActionGate policy):** decide whether `uart.signal` RTS/DTR (and Break) should use `confirm:'card'`. Today it is `none` while the daemon risk is R2. This is a panel-local registry change, but it is a safety-policy decision for main.
- **D3 (backend, optional):** daemon byte counters and modem-line state (CTS/DSR/DCD/RI plus actual RTS/DTR) on `uart.state`. Until then they are shown as gaps, not drawn.
- **D4 (SDK):** none required. If HexView should reuse `VirtualTable`, the existing SDK export is enough; no SDK edits.
- Global STOP, ActionGate, rights, generation, gaps and recording semantics are unchanged. No UDS/CAN/ETH/PSU contact.

## 8. Not verified

- CoolTerm was not run. Line Mode's look is unknown (no screenshot). Real UART hardware was not used. The Chart mode is out of scope for UART fidelity (no Loom backend for it).

## 9. Product GO pass — what was implemented (2026-11-12)

MAIN decisions applied: **D1** Options deep link deferred, so no inert toolbar controls. **D2** existing action policy kept; no RTS/DTR controls and no new confirm policy (Break stays in `키 ▾` with its current gate). **D3** counters labeled as "observed" (`관측`), local window only.

Files (all under `ui/src/panels/uart/`; no SDK, shell or global CSS edits):
- `terminalPanel.ts`: CoolTerm-style toolbar `[View 터미널|HEX] [Send String] [화면 지우기] [일시정지] ⟷ [입력권 chip · 가져오기 · 반환 · 미완결 줄 지우기 · HOLD · 키 ▾]`, Terminal/HEX body switch, Send String drawer, and a two-line status bar (line 1: `.uart-port` text unchanged + `path / 8-N-1 · 흐름 x` + `연결 hh:mm:ss` (only when the open event's wall time was seen) + `관측 TX n B / RX n B` + TX/RX activity dots; line 2: the existing `data-testid=uart-status` footer, unchanged).
- `byteLog.ts`: the HEX model, built from the **original `uart.data` payload bytes** (never from `symbolize`d text). 16 B/row; RX and TX on separate rows (also split by origin); gap/overwrite separator rows. Bounded by `MAX_LOG_BYTES` = 256 KiB **and** `MAX_LOG_ROWS` = 20 000 (MAIN review fix: `gap()` now evicts too). Eviction is chunked; `evictedBytes` counts only payload bytes and `evictedRows` counts rows.
- `sendString.ts`: Text/HEX parse/encode with an explicit line ending (None/CR/LF/CR+LF; default CR for Text, None for HEX) and a preview of the exact final bytes. It routes ≤ 4096 B to the existing `input` action (`uart.input` stream, shared `input_seq`) and > 4096 B to the existing `inputLarge` action (confirmation card). No new RPCs.
- `uartState.ts`: keeps `path`, `data_bits`, `parity`, `stop_bits` and `flow` from `uart.state`/`uart.config` or the snapshot (optional fields, never invented). Adds `frameText()` and `isOpenStatus()`.
- `uart.css`: scoped to `.uart-panel` (container query collapses tool labels at ≤ 720 px pane width).
- Tests: `byteLog.test.ts` (original bytes, byte/row caps, gap-only regression, exact eviction accounting), `sendString.test.ts`, `uartFidelity.test.ts` (status bar, HEX vs Terminal, the two distinct clears, Send String Text/HEX/large/blocked). The existing `uart.test.ts` is unchanged and passes.

Verification (local): `tsc --noEmit -p tsconfig.app.json` is clean, `eslint src/panels/uart` is clean, and `vitest run src/panels/uart src/shell` gives **18 files / 173 tests passed**. No browser/Ego run, no hardware, no build: MAIN owns the aggregate build, Ego and report.

Preserved: `data-testid` `input-chip` / `uart-status`, `.uart-port` text format, all `uartActions` ids/RPCs/confirm policy, the xterm keystroke path, large-paste confirmation, input lease/HOLD/watch/STALLED read-only behavior, and the STOP/latch line (`createLatchLine`).

Intentional deviations (freeze list):
1. The HEX view is **read-only for keystrokes** (focus leaves xterm on switch). CoolTerm sends keys in every view; Loom keeps one visible key path.
2. No window-level Line Mode. The Send String drawer covers explicit line sending; there is no ↑/↓ history (not implemented).
3. The `Enter` key in xterm is unchanged (VT default CR). The line ending applies only to Send String. Shift/Alt+Enter overrides were not adopted.
4. No Connect/Disconnect/Options/Capture/Macros/Chart and no RTS/DTR/CTS/DSR/DCD/RI LEDs. The TX/RX dots show **observed event activity**, not modem lines.
5. Renamed the existing button `지우기` → `미완결 줄 지우기` (same `clear` action → `uart.input.clear`). The new `화면 지우기` is local only (xterm buffer + HEX view) and leaves the observed counters alone.
6. Send String is disabled while the display is paused (matching the terminal's PAUSED key block), when the port is not `open`, under watch, and when STALLED.
7. Hex clipboard: copying a single selected row (`선택 행 HEX 복사`) gives `48 65 6C`. A hex clipboard pasted into the terminal is **not** converted to binary.

Requests for MAIN (not done by this worker): e2e/Ego visual baselines that show the old header may need refreshing; the 390 px layout should be checked in Ego; and decide whether `.workbench-main .uart-head` padding in shell.css should account for the taller icon toolbar.

### MAIN native integration correction
Actual Ego exposed xterm canvas overlapping the Send String toolbar. MAIN clips the terminal body, prevents sibling controls from shrinking, and fits the actual remaining body via ResizeObserver (disposed with the panel). Native Text/HEX send, invalid HEX rejection and original TX bytes were checked. Earlier panel-size estimates and fixed drawer-height assumptions are superseded.
