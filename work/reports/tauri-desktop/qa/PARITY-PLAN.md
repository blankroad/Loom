# Bench Light parity plan — desktop/integration QA (tauri-final-opus), 2026-11-15

The old dark visual results are **void** as visual acceptance. Functional proofs are kept: evidence/native-run4-visible and
ROOT CUA Enter/Tab `root-native-1`/`root-tab --tab`. Old dark QA windows were stopped (run4 cleanup; no QA processes,
ports 47941-2 free).

## Why native ≠ web today (root cause)
Tauri does not ship its own UI. It navigates the WKWebView to the daemon origin `http://localhost:<port>/ui`, which
serves the `ui/dist` embedded in `loomd`. The app therefore showed the old dark product `ui/src` (shell.css `--bg:#101419`).
The Bench Light pages under work/reports are standalone prototypes and were never in `ui/src`.
⇒ **Once A/B land Bench Light in `ui/src`, browser and Tauri are identical by construction.** One build is embedded in
loomd and loaded by both. Desktop adds only the launch/error page (now light, below).

## Shared token contract (A owns ui/src/shell/shell.css `:root`; B consumes, never redefines)
Canonical values are byte-identical across uart/eth/uds prototypes:
`--bg #ffffff · --sf #f6f8fa · --sf2/--hover #eef1f4 · --line #d1d9e0 · --line2 #e6eaef · --text #1f2328 ·
--muted #59636e · --faint #818b98 · --accent #0969da · --accent-ink #fff · --sel #ddf4ff · --selbd #54aeff ·
--warn #9a6700/--warn-bg #fff8c5/--warn-bd #d4a72c · --err #cf222e/--err-bg #ffebe9 · --ok #1a7f37/--ok-bg #dafbe1 ·
--svc-underline #fd8c73 · --radius 6px · --mono ui-monospace,SFMono-Regular,"SF Mono",Menlo,… ·
--ui -apple-system,…,"Noto Sans KR","Apple SD Gothic Neo",…` and `color-scheme: light`.
Recommendations:
1. **Keep the existing product token names** and remap them so current consumers turn light without per-panel churn:
   `--bg`→#fff, `--panel`→#fff, `--chrome`→#f6f8fa, `--raised`→#eef1f4, `--fg`→#1f2328, `--muted`→#59636e,
   `--line`→#d1d9e0, `--accent`→#0969da (`--accent` was teal #76c7c0), `--ok/--warn/--err` → canonical. Then **add** the
   canonical names (`--sf --sf2 --text --faint --sel --selbd --line2 --svc-underline --mono --ui --radius`,
   plus `-bg` variants). Usage today: `--line` ×89, `--muted` ×63, `--err` ×39, `--warn` ×33, `--accent` ×24, `--fg` ×18.
2. Re-tune the AGT/USR/DEV/SCR source-chip colours for a light background (the current `--usr` #e0b020 and `--dev` #9aa3ad
   fail contrast on white). Keep the names.
3. Dockview: replace the hard-coded dark `--dv-*` block (shell.css ~L410-421) with light values from the tokens
   (tab strip `--sf`, active tab `--bg` + `--text`, divider `--line`, sash active `--accent`).
4. Hard-coded hex in panel CSS must move to tokens: logic 34, video 11, eth 8, can 3, psu 2, uart 1.
   CAN/logic/video/psu are out of A/B scope. They must at least not stay dark: A should cover them through tokens, or ROOT assigns.
5. Terminal (xterm) theme stays independent (dark default allowed, per-terminal toggle as in UART REV6 `xterm.bg`
   #0f1216↔#ffffff); everything outside the terminal is light by default.
6. One theme only: no `prefers-color-scheme` auto-dark in the shell unless ROOT asks. Browser and WK must compute the
   same values (QA asserts `getComputedStyle(:root)` tokens in both).

## Layout contract (from canonical sources)
- **Shell (A):** header `[data-shell=header]` (≈44px): brand (22px dark logo + "Loom" 15px/700), `.svcnav` service
  tabs with the 2px `--svc-underline` on `aria-current=page`, conn chip with a 7px green dot, right-aligned STOP.
  Footer `[data-shell=status]` is a single thin status line. Module toolbar `.mtbar` uses `--sf` with a bottom `--line`.
  Rail icon buttons use `--sel` when pressed, with a 3px coral indicator. Dense: 12–13px UI text, 11.5px mono context, 24px icon
  buttons, 28px fields. No large WORKSPACE titles, cards or debug blocks. The current "통합 작업 공간" 24px heading,
  "레이아웃 (딥링크)" row and large sidebar explanatory text must go or be compacted.
- **UART (A):** REV6.1 structure: rail (세션/매크로), collapsible saved-session groups, port **dropdown** from real
  `uart.ports`, pane header `.phead` (alias/port/frame/state/ctr + TERM/HEX, pause, clear, search, theme, EOL, ⋯),
  timestamp gutter, recording pill, green ▶ macro + ⋯ overflow, `.mscope` 활성/연결된 모두/선택, `키 입력 대상` separate.
  Splits/tabs come from the real Dockview.
- **ETH/UDS (B):** dense packet list (`--row 20px`, light Wireshark-style colouring vars `--pc-*`), filter bar,
  detail tree + hex, capture/socket config; UDS preset/transport editor, raw/DID/service/history/response/trace
  (`--row 22px`, `--pend/--late`). Only DoIP+sim are claimed connected.

## DOM hooks QA automation depends on (keep, or tell QA the replacement before READY)
`[data-testid="loom-shell"]`, `[data-testid="rights"]`, `.uart-trace` with innerText rows `tick N`/`ack:…`,
`.xterm-helper-textarea`, routes `#/p/uart/trace?channel=<id>` and `#/p/uart/terminal?channel=<id>`, and panel ids
`uart.terminal|uart.trace|eth.packets|uds.console`. Launch page ids `#failed #message #retry` (desktop-owned, unchanged).

## Desktop changes made now (owned)
- `desktop/frontend/launch.css` → Bench Light tokens (light card, `--err-bg` message, `--accent` Retry). The old copy is in
  evidence/launch.css.olddark.bak.
- `desktop/src-tauri/src/main.rs` window `background_color` #0f1115 → #ffffff (no dark flash). rustfmt (2024) OK.
  The compile check comes with the final build.

## Acceptance checklist (QA runs after A+B READY; single build owner = QA)
1. `ui` build → `stage-loomd.sh --profile debug` (re-embed) → `verify-loomd` 6/6 → `build-app.sh` (ordinary
   `desktop/dist/Loom.app`) + an e2e copy for WK automation → `verify-dist.sh` / `verify-bundle.sh`. Asset digests are recorded.
2. Same daemon, same route, same viewport (1440×900): Ego Chromium vs WK (e2e) vs ordinary app (ROOT CUA screenshot).
   Assert the same `/ui` asset hashes (page `performance` entries → fetched digests), the same computed `:root` tokens,
   and the same top-level DOM hierarchy (header/svcnav/mtbar/rail/dock/status present, element counts).
3. Canonical vs product side-by-side per module (shell, UART, ETH, UDS) at the same size. A manual visual diff checklist:
   palette, density (row heights, font sizes), toolbar order, sidebar/rail, pane header controls. No fake data:
   every visible value comes from the pty/DoIP/pcap fixtures through the real daemon.
4. Functional regression on the new UI: live RX with marker and ticks, native CUA Enter/Tab (ROOT), CLI↔UI, macro
   active/all/selected via fixtures (2 pty devices), quit/reopen, error/Retry (light launch page), hygiene.
5. REPORT.html: canonical-vs-product and browser-vs-Tauri pairs, fingerprinted (git diff hash, ui/dist digest,
   loomd and app hashes), exact limitations.

## Measured canonical metrics (Ego space 14, emulation set + measured + screenshot in the SAME call; inner 1440×900 @2x)
Screenshots: parity/canonical-{uart,eth,uds}-1440x900.png (ROOT's reference: reference-uart-bench-light.png).
| Band | UART REV6.1 | ETH | UDS |
|---|---|---|---|
| `.shell-hdr` | 44px, #fff, top 0 | 44px #fff | 44px #fff |
| module toolbar | `.mtbar` 46px, #f6f8fa, top 44 | `.mtbar` 46px #f6f8fa | `.ctx-row` 38px + `#ctxTools` 29px (no .mtbar) |
| `.shell-status` | 26px, #f6f8fa, 11px mono, bottom | 26px | 26px |
| body font | 13px -apple-system / Noto Sans KR, bg #f6f8fa | 13px | 13px |
| UART details | rail 44px wide; side 264px #fff; splitter `.divider` 6px; tab 32px; `.phead` 31px; xterm 12.5px mono | chbar 59px, filter rows 26px, list head 24px, **packet row 20px** | |
Computed tokens equal the contract above (`--div 6px`, `--radius 6px`).
QA asserts product values against these (±1px), measured the same way in Ego and natively in WK.

## Relay: B (bench-diag-opus) plan, received 2026-11-15 (work/reports/tauri-desktop/bench-diag/STATUS.md)
B edits only ui/src/panels/{eth,uds}/** and keeps panel ids eth.packets/uds.console. B consumes product names
`--bg --fg --panel --chrome --line --accent --muted --raised --ok --warn --err` (so A must remap these to Primer light,
matching recommendation 1). Module `--wb-*` aliases are declared on the panel root only. **Request to A:** delete the shell.css
dark override `.workbench-main :is(.uart-head,.eth-status,.eth-mode,.test-head,.uds-targets,.psu-head){background:#18212b}`.
Panels render only their own 46px module toolbar. UDS adds the manifest action `connect` → `uds.connect`.

## Update (QA, after ROOT ownership extension)
- desktop main window: `.theme(Some(tauri::Theme::Light))` (per window, NSAppearance; no OS-wide setting) plus
  `background_color` #ffffff; launch/connecting/error page uses the light tokens. `cargo check` OK, rustfmt (2024) OK.
- Residual dark CSS (CSS only; pre-edit copies in evidence/css-before/):
  - CAN: `.ct-gaprow` #1e1e14 → `var(--warn-bg,#fff8c5)`; `.ct-loss` #3a1416 → `var(--err-bg,#ffebe9)`;
    `.ct-newrows` text #000 → `var(--fg,#1f2328)`.
  - Video: dark-tuned status/latch/error/frozen colours → `var(--ok|--warn|--err|--accent, Primer fallback)`.
    `.vd-stage` #000 and the `.vd-overlay` scrim are kept (intentional video canvas).
  - Logic: left alone. It is already a light palette (`--lg-*`), and its waveform canvas colours are intentional.
  - PSU: left alone (token fallbacks only).
  - These rely on A defining `--warn-bg`/`--err-bg` (canonical names); the fallbacks cover the gap until then.

## Acceptance criteria added (ROOT, relaying A's context-parity warning) — binding for the visual gate
1. **Entry paths, each checked fresh in both Ego and WK (none of them is forced through `#/s`):**
   - (a) **fresh app open with no route**, with one UART channel open: it must land in the UART service workbench (rail, sessions,
     macros, module toolbar) and **not** the generic integrated frame. The generic frame is allowed only for an explicit integrated
     layout;
   - (b) **agent deep links** `#/p/uart/trace?channel=dut` and `#/p/uart/terminal?channel=dut`: the shell derives the domain
     from the panel id and keeps the service workbench (rail and macros present), not a bare panel;
   - (c) the explicit integrated layout route, labelled as such.
2. **Structure, not paint:** pass requires the canonical hierarchy (header 44 / module toolbar 46 / status 26, rail +
   side + Dockview panes, 13px UI). Old structure with light colours is a FAIL.
3. **Same-daemon comparison:** compare the inner WKWebView snapshot at 1440×900 (WebDriver screenshot = content only)
   against Ego at 1440×900. The native macOS title bar/frame is excluded as legitimate. Viewport, dpr, tokens and bands
   are measured in the same call as the screenshot (`wk.mjs geom`; Ego emulation set + measure + screenshot in one round).
4. Canonical baseline = the Ego screenshots and geometry in parity/canonical-*.png (44/46/26).
Driver (desktop/scripts/e2e/wk.mjs, `node --check` OK): `geom ["sel|sel"]` measures without navigating (fresh entry);
`goto <route>` sets the agent deep link. Selector lists for rail/macros/mtbar come from A's DOM handoff.

## Fidelity rule (user, repeated): the selected web is the fixed source of truth, not inspiration
Preserve the exact spatial hierarchy, toolbar order and labels, dense spacing, panel controls/workflows, and session/macro scopes.
No new alternatives and no simplified replacements. **Every intentional deviation is listed below with a concrete technical
reason and ROOT approval.** QA fails any unlisted difference. The gate is canonical vs product (order, labels, geometry,
workflows), not only browser vs native or matching colours.

### Canonical control order (machine-extracted; parity/canonical-controls.json) — QA diffs product order against this
- **UART `.mtbar`:** 세션·매크로 패널 토글 | 새 UART 세션 | 오른쪽 분할 · 아래 분할 · 최대화 · pane 닫기 |
  키 입력 대상 [현재 세션 | 연결된 모두] | 새 매크로 · 내보내기 · 가져오기 | 레이아웃 초기화 · 단축키 · ctx(대상 alias port · pane N)
- **UART rail:** 저장된 세션 (Alt+1) · 매크로 (Alt+2). **Side:** 새 세션, 접기, 세션 검색, collapsible groups
  (n), per session: 활성 pane에 열기 / 오른쪽 분할에 열기 / 아래 분할에 열기 / 설정. **Macros:** green ▶ · ⋯ · `.mscope` 활성/연결된 모두/선택.
- **UART `.phead`:** TERM · HEX | 일시정지 · 지우기 · 시각 · 기록(rec) · EOL · 매크로 메뉴 · 검색 · 테마 · 포트 닫기 · 설정 · 더보기
- **UART status:** 활성 pane N › alias port baud frame state RX/TX · pane · 탭 · 매크로 실행 · 기록 · 단축키
- **ETH `.mtbar`:** 채널 추가 · 전체 시작 · 전체 정지 | 화면 일시정지 · 자동 스크롤 · 색상 규칙 | 따라가기 · 내보내기 · 지우기 |
  소켓 테스트 · 단축키 · ctx; `#chbar` per-channel chips (select · stop · menu) + 채널 추가; filter rows; list/detail/hex
- **UDS `.ctx-row`:** ECU select | transport presets (DoIP / CAN-TP / EthDiag) · 링크 · 연결 설정 · 주소 상세 · SIM 옵션;
  `#ctxTools`: 서비스 · ECU·DID · 명령·시퀀스 · 감시 · 프로젝트 · 기록 최대화 · 상세 최대화 · 레이아웃 메뉴
- **Not carried into product (by rule, not deviations):** the prototype "프로토타입 · 로컬 시뮬레이션" notes, the SIM add-channel
  wording, and simulated ports, devices and values. These are replaced by real daemon data/actions with the same control in the same place.
  An unsupported real capability (e.g. CAN-TP/EthDiag connect) keeps its slot, honestly disabled with a reason.

### Deviation register (owner proposes → ROOT approves; QA checks nothing else differs)
| # | Module | Canonical | Product | Technical reason | ROOT |
|---|---|---|---|---|---|
| — | — | — | — | (none approved yet) | — |

## REC / export file test (added to final checklist) + desktop download glue (ROOT review requested)
**Finding:** wry 0.57.2 (`wkwebview/navigation.rs`) answers `shouldPerformDownload` with **Cancel when no download handler
is installed**. In the current app every `<a download href=blob:…>` (REC/export) therefore silently does nothing. Blob
navigation is also Blocked by `nav::classify`.
**Glue written (desktop-owned; no remote IPC change; NOT compiled yet, per the no-build-until-A/B-READY rule):**
- `nav::download_allowed(url, port)` accepts `blob:` only when its creator origin is the selected daemon origin, `data:` only while
  connected, and the daemon origin (attachment) itself. It denies everything else. A unit test covers wrong port, foreign origin,
  `blob:tauri:`, `file:`, and the not-connected case.
- `with_policy(...).on_download`: Requested → policy check, keeping wry's destination `~/Downloads/<suggested name>`, which never
  overwrites (" (n)" suffix). Only the file name is logged. Finished → logs success and file name. e2e debug builds only:
  `LOOM_DESKTOP_QA_DOWNLOAD_DIR` redirects into the QA home (set by smoke-native.sh), so tests never touch the user's ~/Downloads.
- UX difference vs Chrome: there is no Save-As dialog in WK, and files go straight to ~/Downloads. If A's UI expects a picker, the
  UI should state "다운로드 폴더에 저장". A has been told not to depend on Chromium-only FSA.
**Checklist item (both Ego and WK, real fixture data):**
1. UART REC (.log and .jsonl), start → fixture traffic (marker + ticks) → stop → file saved; contents hold the marker and
   tick lines with timestamps. In WK: `smoke-native.sh check-download '<alias>-*.log' ack:<marker>`.
2. UART/ETH/UDS export buttons (JSON export of sessions/macros, packet CSV/pcap, UDS history): each yields a non-empty file
   with the expected header/rows.
3. Hygiene step also scans the copied downloads for the token or `#boot=`.
4. Negative: a page-initiated download from a non-daemon origin is denied (unit test, plus log line if observed).
