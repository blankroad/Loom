# Sol — UART bench console

2026-11-13 · one standalone UART mockup · human-operated desk, no agent UI.

## Scoped official research

[D] means documented behavior; [V] means directly viewed official image; [I] is a design inference. Two existing official MobaXterm images were inspected with the native image tool after source freeze (see below); no browser was started.

| Official source, retrieved 2026-11-13 | Observation | Adopt / reject for this module |
| --- | --- | --- |
| [MobaXterm documentation](https://mobaxterm.mobatek.net/documentation.html), §§1.2, 1.3, 2.1 | [D] Saved sessions are accessible in the sidebar; tabs expose independent sessions. Documented split modes offer two or four terminals. Multi-execution writes to multiple terminals. | [I] Adopt saved-session proximity, compact connection metadata and terminal-first density. Use arbitrary recursive splits for the requested flexibility. Omit broadcast: every send and macro has one named connection target. |
| [Windows Terminal panes](https://learn.microsoft.com/en-us/windows/terminal/panes) and [actions / zoom](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions#zoom-a-pane) | [D] Splits act on the focused pane; focus has an accent border. Panes support resizing, movement and zoom. | [I] Adopt focus border, split-focused-pane and maximize/restore. Expose labeled toolbar actions and draggable dividers so routine work does not require memorized key bindings. |
| [WezTerm pane:split](https://wezterm.org/config/lua/pane/split.html) | [D] A pane can split in four directions with a configurable size; `top_level` distinguishes a tab-level split from a pane-local split. | [I] Use a recursive split tree and minimum sizes. Keep two explicit directions (right / down) to limit toolbar width; either can be nested repeatedly. Do not expose Lua or shell scripting in a user macro. |

Additional official image URLs resolved (not visually examined): [MobaXterm main window](https://mobaxterm.mobatek.net/doc/mobaxterm-main-window.png), [session manager](https://mobaxterm.mobatek.net/doc/network-sessions-manager.png), [split button](https://mobaxterm.mobatek.net/doc/split-mode.png). Microsoft [pane creation GIF](https://learn.microsoft.com/en-us/windows/terminal/images/open-panes.gif) and [focus GIF](https://learn.microsoft.com/en-us/windows/terminal/images/navigate-panes.gif) could not be displayed by the research tool (unsupported GIF); no visual claims depend on them. No third-party screenshot or asset was embedded.

### Direct visual benchmark, evidence-only addition after source freeze

Read-only local images were opened with `view_image`; attribution was checked against [shared SOURCES.md](../../ui-style-survey/shared-references/SOURCES.md). These observations validate the frozen direction; they did not trigger source changes.

- [V] [MobaXterm tabbed terminal](https://mobaxterm.mobatek.net/img/moba/features/feature-terminal.png), local `../../ui-style-survey/shared-references/moba-terminal.png`: light menu/Quick connect/session-tree chrome surrounds a large black terminal. A narrow left strip labels Sessions, Tools and Macros vertically. The session tree uses tight rows with small folder icons; the selected terminal has a numbered tab and a nearby plus control. Colored timestamp / host / prompt text stays within the terminal. [I] Adopt the light frame / dark output distinction, compact persistent session access, tab identity and adjacent open control. Reject copying the dated icon palette and colored banner text: this UART module uses restrained state colors and readable raw records.
- [V] [MobaXterm multi-execution](https://mobaxterm.mobatek.net/img/moba/features/feature-multiexec.png), local `../../ui-style-survey/shared-references/moba-multiexec.png`: four black terminals form a 2×2 arrangement with thin light dividers and separate scrollbars. A conspicuous light top banner explicitly states that commands are typed to all terminals; an Exit multi-execution mode button sits at its right. [I] Adopt simultaneous terminal visibility and clear pane boundaries. Reject broadcast for this design because the user must distinguish similarly named UART connections; the frozen runner instead shows and locks one alias + T-number + endpoint. Reject a fixed 2×2-only interaction: recursive splitting supports the requested layouts. The screenshot establishes simultaneous display, not evidence of arbitrary nested resizing; that behavior remains our implementation choice.

Copyright of the reference images remains with Mobatek. Images are referenced only and were not copied into the module.

## One design: Bench console / tabbed split tree

A 232px collapsible library places saved sessions above user macros. The main work surface starts with mcu + aux, each with its own connection tab, framing, input, echo and output view. Selecting a pane controls splits and new tabs. Tabs carry unique T01/T02 identifiers, so two copies of `mcu` are distinct targets. Native tab drag/drop preserves the connection, its theme, settings and output; an emptied source pane collapses.

The macro runner stays a compact light strip below the terminals. The user selects a named sequence, sees its steps, confirms an exact open connection, and runs or stops it. The selected target is locked for a run; stop aborts the current delay and blocks subsequent sends. Creation, edit, duplicate, delete, ordered send/delay steps, a text variable and shortcut assignment are available through a conventional sequence editor. Local persistence is automatic; JSON export is available.

Information density takes priority over decoration: timestamps and raw boot/telemetry/error output, byte counts, per-connection baud/framing, and narrow control rows. TEXT and HEX share the same simulated records; search, pause and clear operate on the view. Labels remain in the toolbar rather than creating a large command palette or chat surface.

## Shared outer shell and component boundary

User steering fixed the shared shell to [Fable F2 Bench Light](../../ui-style-survey/fable/index.html?design=F2); its [survey](../../ui-style-survey/fable/survey.md) was explicitly permitted as a read-only shell reference. This prototype uses the F2 white / #f6f8fa / #d1d9e0 / #1f2328 / #0969da chrome, Loom identity and compact service/status line. Only UART is present in navigation. Other services and real devices are not represented as active.

Eventual integration boundary: the shell owns global branding, service navigation, environment/connection summary and global status. The UART module owns its library, recursive pane tree, tabs, connection settings, terminal theme, output/input and macro lifecycle. `index.html`'s `.app-header` is the standalone shell adapter; `.app-body` is the UART module region. `#shell-connection` is a compact summary seam. `window.UARTDesk` is a prototype state/verification API, not a production transport contract. Existing product sources and implementation plans are unchanged.

Only terminal output surfaces are dark by default. Each connection can switch Dark / Light independently; the sessions, settings, macros, controls and shared outer chrome stay light. At 390px, the library starts collapsed and the same recursive tree flows vertically; desktop split orientation remains preserved in state. At intermediate widths an oversized pane tree can scroll to preserve readable minimums.
