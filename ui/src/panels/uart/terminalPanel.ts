// uart.terminal (M1) — uart-ui §1–4, plan §5.1.2; CoolTerm-fidelity layout
// (docs/impl/main/research/fidelity/uart.md).
// Pane = the agreed REV6.1 web pane (work/reports/uart-workbench/opus): .phead (alias · port · frame · state ·
// RX/TX + TERM|HEX · 일시정지 · 지우기 · 시각 · REC · EOL · 매크로 · 검색 · Send String · 테마 · 포트 · 설정 · ⋯),
// receive-time gutter (journal host time, UI metadata only), per-terminal theme, macro run strip.
// Existing contracts kept: input lease/HOLD/palette (⋯ row), Send String drawer, HEX byte view, agent line.
// Body:   TerminalView (device bytes as-is through the 3-layer defense) OR the byte (HEX) view of
//         the original uart.data bytes. Keys are sent only from the terminal view.
// Drawer: Send String (Text/HEX + explicit line ending) → existing uart.input / inputLarge path.
// Status bar line 1: UART alias ● status baud gen [ENV] · path / 8-N-1 · flow · open time · TX/RX observed · activity.
// Status bar line 2: last agent send + stage · agent expect wait · rx rate · blocked counts · REC.

import {
  CHANNEL_PROP,
  createLatchLine,
  bytesToHex,
  decodePayloadBytes,
  fmtWall,
  gapText,
  originCell,
  registerPanel,
  STAGE_CHIP,
  TerminalView,
  VirtualTable,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type WriteState,
} from '../../sdk';
import './uart.css';
import { ByteLog, rowAscii, rowHex, rowsToHex, type ByteRow } from './byteLog';
import { createSendString, routeFor } from './sendString';
import { compileQuery, DEFAULT_SEARCH_OPTIONS, searchBuffer, searchCountText, stepHit, type SearchHit, type SearchOptions } from './scrollSearch';
import { connClass, headerMode, identityPlan, identityTitle, toolPlacement, type HeaderMode, type ToolId } from './paneLayout';
import { compileRules, HIGHLIGHT_CSS, HIGHLIGHT_MAX_DECORATIONS, matchLine, type CompiledRule } from './highlight';
import { downloadText, EXPORT_QUERY, EXPORT_RECENT_MINUTES, runExport, summaryText, type ExportFormat, type ExportProgress, type ExportRange, type ExportSummary } from './journalExport';
// The UART workbench (shell-side) never gives this panel daemon access: the panel registers a handle whose sends
// go through its own ctx.act('input') path; the workbench only supplies local prefs, scopes and run state.
import { EOLS, uartBench, type EolKey } from '../../workbench/uart/workbench';
import { icon } from '../../workbench/uart/icons';
import { DEFAULT_REC_PREFS, fmtTs, hasFilePicker, Recorder, type RecFormat, type RecPrefs } from '../../workbench/uart/recorder';
import {
  expectText,
  frameText,
  INITIAL_INPUT,
  inputChip,
  isOpenStatus,
  portExtrasText,
  PORT_KINDS,
  reduceInput,
  reducePort,
  restoreUartState,
  symbolize,
  type UartInputState,
  type UartPortState,
} from './uartState';

const PALETTE: { label: string; byte?: number; signal?: string; title: string; value?: boolean; ms?: number; line?: boolean }[] = [
  { label: 'Ctrl-W', byte: 0x17, title: '단어 지우기 (브라우저가 가로채는 키)' },
  { label: 'Ctrl-T', byte: 0x14, title: '0x14' },
  { label: 'Ctrl-N', byte: 0x0e, title: '0x0E' },
  { label: 'Ctrl-]', byte: 0x1d, title: '0x1D 리터럴' },
  { label: 'Esc', byte: 0x1b, title: '0x1B' },
  { label: 'Tab', byte: 0x09, title: '0x09' },
  { label: 'Ctrl-C', byte: 0x03, title: '0x03 (즉시)' },
  { label: 'Break', signal: 'break', title: 'UART break 신호 (uart.signal)' },
  // B6 (U6): reset-style line pulses — they can reset the board, so they go through a confirm card (action `signalLine`)
  { label: 'DTR 펄스', signal: 'dtr', value: false, ms: 100, line: true, title: 'DTR 100 ms 내렸다 올리기 (보드 리셋에 쓰임 · 확인 후 전송)' },
  { label: 'RTS 펄스', signal: 'rts', value: false, ms: 100, line: true, title: 'RTS 100 ms 내렸다 올리기 (부트 모드 진입에 쓰임 · 확인 후 전송)' },
];

const RX_WINDOW_MS = 1_000;

const hms = (ms: number): string => {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (x: number) => String(x).padStart(2, '0');
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
};

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

const XTHEME = {
  dark: { background: '#0f1216', foreground: '#d0d7de', cursor: '#d0d7de', cursorAccent: '#0f1216', selectionBackground: 'rgba(56,139,253,0.45)', black: '#484f58', red: '#ff7b72', green: '#56d364', yellow: '#e3b341', blue: '#79c0ff', magenta: '#d2a8ff', cyan: '#76e3ea', white: '#d0d7de', brightBlack: '#6e7781', brightRed: '#ffa198', brightGreen: '#7ee787', brightYellow: '#f2cc60', brightBlue: '#a5d6ff', brightMagenta: '#e2c5ff', brightCyan: '#b3f0ff', brightWhite: '#ffffff' },
  light: { background: '#ffffff', foreground: '#1f2328', cursor: '#1f2328', cursorAccent: '#ffffff', selectionBackground: '#b6e3ff', black: '#1f2328', red: '#cf222e', green: '#1a7f37', yellow: '#9a6700', blue: '#0969da', magenta: '#8250df', cyan: '#1b7c83', white: '#6e7781', brightBlack: '#8c959f', brightRed: '#a40e26', brightGreen: '#116329', brightYellow: '#7d4e00', brightBlue: '#0550ae', brightMagenta: '#6639ba', brightCyan: '#1b7c83', brightWhite: '#424a53' },
};
const fmtBytes = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} kB` : `${(n / 1048576).toFixed(2)} MB`);
const CONN_LABEL: Record<string, string> = { open: '연결됨', closed: '닫힘', disconnected: '장치 없음', reconnecting: '재연결 대기', '?': '상태 확인 중' };
const PAUSE_CAP = 2 * 1024 * 1024;

export function createUartTerminal(ctx: PanelContext): PanelInstance {
  const channel = typeof ctx.props.channel === 'string' ? ctx.props.channel : 'dut';
  const now = () => performance.now();
  const mirror = ctx.store('term');
  const hasWriteRight = () => ctx.rights.has('operate') || ctx.rights.has('approve');
  /** U-3: STALLED / disconnected → read-only like the shell's write lock. A port that is not OPEN (closed,
   * disconnected, unknown — e.g. a retained history view after STOP/disconnect) is read-only too. */
  // Both read the engine-fed port model (syncPort), never the last DRAWN state: a background dock tab is not
  // drawn (RenderScheduler skips hidden panels) but must still know whether its port is open.
  const canWrite = () => (syncPort(), hasWriteRight() && !ctx.writeLocked && isOpenStatus(port.status));
  const blockedReason = (): string | null => (syncPort(), !hasWriteRight() ? '읽기 전용' : ctx.writeLocked ? '쓰기 잠김' : !isOpenStatus(port.status) ? (CONN_LABEL[port.status] ?? port.status) : null);

  let input: UartInputState = INITIAL_INPUT;
  let port: UartPortState = { status: '?', baud: null, generation: null };
  let lastSeq = -1n;
  /** model-only cursors of syncPort (independent of the draw cursors lastSeq/snapshotVersion) */
  let portSnapVersion = 0;
  let portScanSeq = -1n;
  /** seq of the newest port state applied (live control, backlog or snapshot high-water): older never overwrite */
  let portSeq = -1n;
  let lastOverwritten = 0;
  let gapsSeen = 0;
  let separatorDone = false;
  let snapshotVersion = 0;
  let paused = false;
  let lastSend: { origin: string; text: string; requestId: string | null; state: WriteState | null; atMs: number } | null = null;
  const rx: { atMs: number; n: number }[] = []; // bounded by RX_WINDOW_MS pruning
  let inputSeq = 0;
  let lastErrToast = 0;
  let view: 'terminal' | 'hex' = 'terminal';
  // observed in this panel's local window only (not device/daemon totals)
  let txObserved = 0;
  let rxObserved = 0;
  let countsPartial = false;
  let openSinceWallMs: number | null = null;
  let leaseOpen = false;
  let rec: Recorder | null = null;
  let recPicking = false;
  let recDialog: HTMLDialogElement | null = null;
  // U1-B (B5): journal export of a past range — running export (cancel) and the last result summary
  let jx: { ac: AbortController; progress: ExportProgress } | null = null;
  let jxLast: ExportSummary | null = null;
  /** dialog selection kept across re-renders of the REC dialog */
  const jxSel: { kind: ExportRange['kind']; minutes: number } = { kind: 'retained', minutes: 15 };
  // U5b (B6): compiled highlight rules of this channel's session and the live decorations (oldest disposed first)
  let hlRules: CompiledRule[] = [];
  let hlKey = '';
  const hlDecos: { dispose(): void }[] = [];
  /** B6 §B6.4: recompile when the session's rules change; decorate logical lines from buffer row `fromRow` on. */
  function applyHighlights(fromRow: number): void {
    const session = uartBench.sessionFor(channel);
    const key = JSON.stringify([session?.highlights ?? [], session?.scrollback ?? null]);
    let from = fromRow;
    if (key !== hlKey) {
      hlKey = key;
      const { compiled, errors } = compileRules(session?.highlights ?? []);
      hlRules = compiled;
      for (const d of hlDecos.splice(0)) {
        try {
          d.dispose();
        } catch {
          /* already disposed */
        }
      }
      try {
        term.term.options.scrollback = session?.scrollback ?? 10_000;
      } catch {
        /* terminal not open (tests) */
      }
      wrap.dataset.hlRules = String(hlRules.length);
      wrap.dataset.hlErrors = String(errors.length);
      from = 0; // new rules: the whole buffer is decorated again
    }
    if (hlRules.length === 0) return;
    try {
      const buf = term.term.buffer.active;
      const cursorAbs = buf.baseY + buf.cursorY;
      for (let row = Math.max(0, from); row < buf.length; row++) {
        const text = buf.getLine(row)?.translateToString(true);
        if (!text) continue;
        for (const span of matchLine(text, hlRules)) {
          try {
            const marker = term.term.registerMarker(row - cursorAbs);
            if (!marker) continue;
            const deco = term.term.registerDecoration({ marker, x: span.start, width: span.end - span.start, backgroundColor: HIGHLIGHT_CSS[span.color] });
            hlDecos.push(deco ?? marker);
            while (hlDecos.length > HIGHLIGHT_MAX_DECORATIONS) hlDecos.shift()?.dispose();
          } catch {
            /* decorations are best-effort; writing is never affected */
          }
        }
      }
    } catch {
      /* terminal not open / buffer unavailable */
    }
  }
  const bytes = new ByteLog();
  const wallOf = (ev: LoomEvent): number => {
    try {
      return Number(BigInt(ev.ts.wall_ns) / 1_000_000n);
    } catch {
      return 0;
    }
  };

  const send = (b: Uint8Array): boolean => {
    if (!canWrite()) return false; // never emit input for a port that is not open (panel-level, before the gate)
    ctx
      .act('input', { channel, data_hex: bytesToHex(b), input_seq: ++inputSeq })
      .catch((e: unknown) => {
        if (now() - lastErrToast > 5_000) {
          lastErrToast = now();
          ctx.ui.toast(`UART 입력 실패: ${e instanceof Error ? e.message : String(e)}`);
        }
      });
    return true;
  };
  /** typed keys: Enter → this tab's EOL; 키 입력 대상 = 연결된 모두 routes to every frozen recipient */
  const keyInput = (b: Uint8Array) => {
    if (uartBench.routeKeys(channel, b, (ch, x) => uartBench.mapEnter(ch, x))) return;
    send(uartBench.mapEnter(channel, b));
  };

  const term = new TerminalView({
    readonly: !canWrite(),
    onInput: keyInput,
    onLargePaste: (text) => sendLarge(new TextEncoder().encode(text), '붙여넣기 취소'),
  });
  // REV6.1 terminal metrics and per-terminal theme (outer shell stays light)
  term.term.options.fontFamily = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace';
  term.term.options.fontSize = 12.5;
  term.term.options.lineHeight = 1.2;
  let theme: 'dark' | 'light' = uartBench.tabPrefs(channel).theme;
  term.term.options.theme = XTHEME[theme];
  /** > 4 KiB goes through a confirm card (ActionGate 'card') — paste and Send String alike */
  function sendLarge(b: Uint8Array, failText: string): void {
    ctx
      .act('inputLarge', { channel, data_hex: bytesToHex(b), input_seq: ++inputSeq })
      .catch((e: unknown) => ctx.ui.toast(`${failText}: ${e instanceof Error ? e.message : String(e)}`));
  }

  // ---- DOM (REV6.1 pane: .phead · .search · .term-wrap · run strip) ----
  const root = el('div', 'uart-panel');
  root.dataset.channel = channel;
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`));
  const btn = (label: string, title: string, on: () => void, cls = 'uart-btn') => {
    const b = el('button', cls, label);
    b.type = 'button';
    b.title = title;
    b.addEventListener('click', on);
    return b;
  };
  const ibtn = (actName: string, ico: string, title: string, on: (b: HTMLButtonElement) => void, testid?: string) => {
    const b = el('button', 'ibtn');
    b.type = 'button';
    b.dataset.act = actName;
    b.title = title;
    b.setAttribute('aria-label', title.split(' (')[0]!);
    b.innerHTML = icon(ico);
    if (testid) b.setAttribute('data-testid', testid);
    b.addEventListener('click', () => on(b));
    return b;
  };
  const pill = (actName: string, label: string, title: string, on: () => void, testid?: string) => {
    const b = el('button', 'pill', label);
    b.type = 'button';
    b.dataset.act = actName;
    b.title = title;
    if (testid) b.setAttribute('data-testid', testid);
    b.addEventListener('click', on);
    return b;
  };

  const head = el('div', 'phead');
  head.dataset.tab = channel;
  const target = el('div', 'ph-target');
  target.title = '이 pane의 실제 대상';
  const dot = el('span', 'ph-dot');
  const aliasEl = el('span', 'ph-alias', channel);
  const pathEl = el('span', 'ph-port');
  const frameEl = el('span', 'ph-frame');
  const bcChip = el('span', 'chip');
  const stateEl = el('span', 'ph-state');
  stateEl.dataset.role = 'state';
  const countEl = el('span', 'ph-ctr');
  countEl.dataset.role = 'ctr';
  countEl.setAttribute('data-testid', 'uart-counts');
  const portEl = el('span', 'uart-port sr-only'); // accessible one-line summary (status · baud · generation · env)
  const chipEl = el('button', 'pill uart-chip');
  chipEl.type = 'button';
  chipEl.setAttribute('data-testid', 'input-chip');
  chipEl.title = '입력권 상태 — 눌러서 입력권·특수 키 도구 열기';
  chipEl.addEventListener('click', () => { leaseOpen = !leaseOpen; ctx.scheduler.invalidate(); });
  target.append(dot, aliasEl, pathEl, frameEl, bcChip, stateEl, countEl, portEl);

  const tools = el('div', 'ph-tools uart-tools');
  tools.setAttribute('role', 'toolbar');
  tools.setAttribute('aria-label', `UART ${channel} 도구 막대`);
  const viewGroup = el('span', 'seg uart-viewsw');
  viewGroup.setAttribute('role', 'group');
  viewGroup.setAttribute('aria-label', '보기 형식');
  const termViewBtn = el('button', undefined, 'TERM');
  termViewBtn.type = 'button';
  termViewBtn.dataset.act = 'mode';
  termViewBtn.dataset.mode = 'text';
  termViewBtn.title = '실제 터미널 (xterm) — 키 입력이 장치로 즉시 전송됨';
  termViewBtn.setAttribute('data-testid', 'uart-view-terminal');
  termViewBtn.addEventListener('click', () => setView('terminal'));
  const hexViewBtn = el('button', undefined, 'HEX');
  hexViewBtn.type = 'button';
  hexViewBtn.dataset.act = 'mode';
  hexViewBtn.dataset.mode = 'hex';
  hexViewBtn.title = '송수신 원본 바이트 HEX 보기 (키 입력 안 보냄)';
  hexViewBtn.setAttribute('data-testid', 'uart-view-hex');
  hexViewBtn.addEventListener('click', () => setView('hex'));
  viewGroup.append(termViewBtn, hexViewBtn);
  const pauseBtn = pill('pause', '일시정지', '화면 갱신 일시정지 (수신은 계속 기록)', () => setPaused(!paused), 'uart-pause');
  const clearViewBtn = pill('clear', '지우기', '화면만 지우기 — 장치·데몬 저널·서버 입력 줄에는 영향 없음 (로그·카운터 보존)', () => {
    try {
      term.term.clear();
    } catch {
      /* not opened yet */
    }
    for (const m of tsMarks) m.mk.dispose();
    tsMarks = [];
    lastMark = null;
    bytes.clear();
    gutterSoon();
    ctx.scheduler.invalidate();
  }, 'uart-clear-view');
  const tsBtn = pill('ts-toggle', '시각', '행 앞 수신 시각 표시/숨기기 — 데몬 저널의 호스트 수신 시각 HH:mm:ss.SSS · 장치 바이트는 바뀌지 않음', () => {
    uartBench.setTabPref(channel, { ts: !uartBench.tabPrefs(channel).ts });
  });
  const recBtn = pill('rec', 'REC', 'UART 기록 (이 탭의 원본 수신/송신을 .log 또는 .jsonl로)', () => openRecDialog());
  recBtn.classList.add('rec');
  const eolSel = el('select', 'eol-sel');
  eolSel.dataset.act = 'eol';
  eolSel.title = 'Enter 시 보낼 줄 끝 (이 탭)';
  eolSel.setAttribute('aria-label', '줄 끝');
  for (const k of Object.keys(EOLS)) eolSel.append(new Option(k, k));
  eolSel.addEventListener('change', () => uartBench.setTabPref(channel, { eol: eolSel.value as EolKey }));
  const macroBtn = ibtn('macros-menu', 'macro', `매크로 실행 → ${channel}`, (b) => {
    const r = b.getBoundingClientRect();
    uartBench.noteActive(channel);
    window.dispatchEvent(new CustomEvent('loom:uart-macro-menu', { detail: { channel, x: r.right - 240, y: r.bottom + 4 } }));
  });
  const searchBtn = ibtn('search', 'search', '검색 (터미널 스크롤백)', () => setSearch(!searchOpen));
  const sendStrBtn = ibtn('send-string', 'send', 'Send String — Text/HEX와 줄 끝을 지정해 전송 (uart.input)', () => setDrawer(!drawer.isOpen), 'uart-send-string-toggle');
  const themeBtn = ibtn('theme', 'sun', '이 터미널만 밝게', () => {
    theme = theme === 'dark' ? 'light' : 'dark';
    uartBench.setTabPref(channel, { theme });
    applyTheme();
  });
  const plugBtn = ibtn('disconnect', 'unplug', '포트 닫기', () => {
    if (isOpenStatus(port.status)) {
      void uartBench.closeChannel(channel).catch((e: unknown) => ctx.ui.toast(`포트 닫기 실패: ${e instanceof Error ? e.message : String(e)}`));
    } else window.dispatchEvent(new CustomEvent('loom:uart-reopen', { detail: { channel } }));
  });
  const gearBtn = ibtn('settings', 'gear', `${channel} 세션 설정`, () => window.dispatchEvent(new CustomEvent('loom:uart-edit-session', { detail: { channel } })));
  const moreBtn = ibtn('more', 'more', '더보기: 입력권·특수 키, 로그 저장', () => { leaseOpen = !leaseOpen; ctx.scheduler.invalidate(); });
  tools.append(viewGroup, pauseBtn, clearViewBtn, tsBtn, recBtn, eolSel, macroBtn, searchBtn, sendStrBtn, themeBtn, plugBtn, gearBtn, moreBtn);
  head.append(target, chipEl, tools);
  // U7 (FUNCTIONS §B1.4): every header tool by id; applyHeaderMode moves these same nodes between header and ⋯ row
  const toolEls: Record<ToolId, HTMLElement> = { view: viewGroup, pause: pauseBtn, clear: clearViewBtn, ts: tsBtn, rec: recBtn, eol: eolSel, macro: macroBtn, search: searchBtn, send: sendStrBtn, theme: themeBtn, plug: plugBtn, gear: gearBtn, more: moreBtn };
  head.dataset.mode = 'full';
  aliasEl.title = channel;

  // input lease / special keys (existing product controls; shown on demand or when the lease needs attention)
  const lease = el('div', 'uart-lease');
  lease.setAttribute('role', 'group');
  lease.setAttribute('aria-label', '입력권');
  const yieldBtn = btn('반환', '입력권을 에이전트에게 반환 (Ctrl-] y)', () => void act('yield', { channel }), 'pill');
  // server-side dirty input line (uart.input.clear) — NOT the local screen clear above
  const clearBtn = btn('미완결 줄 지우기', '장치로 보낸 미완결 입력 줄을 서버에서 지움 (uart.input.clear, Ctrl-] c) — 화면 지우기 아님', () => void act('clear', { channel }), 'pill');
  clearBtn.setAttribute('data-testid', 'uart-input-clear');
  const takeBtn = btn('가져오기', '입력권 가져오기 (Ctrl-] o)', () => void act('takeover', { channel }), 'pill');
  const holdBtn = btn('HOLD', '에이전트 쓰기 보류 (Ctrl-] h)', () => void act(input.hold ? 'unhold' : 'hold', { channel }), 'pill');
  const leaseLbl = el('span', 'uart-lease-l', '입력권');
  const keysLbl = el('span', 'uart-lease-l', '특수 키');
  const palette = el('span', 'uart-palette');
  for (const k of PALETTE) {
    palette.append(
      btn(k.label, k.title, () => {
        if (k.byte !== undefined) term.sendBytes(new Uint8Array([k.byte]));
        else if (k.signal) {
          void act(k.line ? 'signalLine' : 'signal', {
            channel,
            kind: k.signal,
            ...(k.value !== undefined ? { value: k.value } : {}),
            ...(k.ms ? { duration_ms: k.ms } : {}),
          });
        }
        if (view === 'terminal') term.focus();
      }, 'pill'),
    );
  }
  lease.append(leaseLbl, takeBtn, yieldBtn, clearBtn, holdBtn, keysLbl, palette);
  const overflow = el('span', 'uart-overflow');
  overflow.setAttribute('role', 'group');
  overflow.setAttribute('aria-label', '숨긴 도구');
  overflow.hidden = true;
  const overflowLbl = el('span', 'uart-lease-l', '도구');
  lease.prepend(overflow);
  // U7: move the same tool nodes between the header and the ⋯ row by header width (listeners stay attached)
  function applyHeaderMode(): void {
    const m = headerMode(head.clientWidth);
    if (m === null || m === headMode) return;
    headMode = m;
    head.dataset.mode = m;
    const p = toolPlacement(m);
    tools.replaceChildren(...p.header.map((id) => toolEls[id]));
    overflow.replaceChildren(overflowLbl, ...p.overflow.map((id) => toolEls[id]));
    overflow.hidden = p.overflow.length === 0;
    moreBtn.title = p.overflow.length ? '더보기: 숨긴 도구 · 입력권 · 특수 키' : '더보기: 입력권·특수 키, 로그 저장';
    ctx.scheduler.invalidate();
  }
  const headObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => applyHeaderMode());

  // search (xterm scrollback; Enter next, Shift+Enter previous)
  let searchOpen = false;
  let hits: SearchHit[] = [];
  const searchOpts: SearchOptions = { ...DEFAULT_SEARCH_OPTIONS }; // toggled in place by setSearchOption (FUNCTIONS §B1.3)
  let headMode: HeaderMode = 'full';
  let hitAt = -1;
  const search = el('div', 'search');
  search.dataset.tab = channel;
  const sq = el('input');
  sq.type = 'search';
  sq.dataset.act = 'search-q';
  sq.placeholder = '스크롤백 검색 (Enter 다음, Shift+Enter 이전)';
  sq.setAttribute('aria-label', '터미널 검색');
  const scount = el('span', 'count');
  scount.dataset.role = 'count';
  const sclose = ibtn('search-close', 'x', '검색 닫기', () => setSearch(false));
  sq.title = 'xterm 스크롤백 안에서 검색 · 줄바꿈된 긴 줄은 이어서 검색 · 더 오래된 기록은 데몬 저널';
  const caseBtn = pill('search-case', 'Aa', '대소문자 구분 (켜면 정확히 일치)', () => setSearchOption('caseSensitive'), 'uart-search-case');
  caseBtn.setAttribute('aria-pressed', String(searchOpts.caseSensitive));
  const regexBtn = pill('search-regex', '.*', '정규식 (JavaScript 문법) — 복잡한 패턴은 화면을 잠시 멈출 수 있음', () => setSearchOption('regex'), 'uart-search-regex');
  regexBtn.setAttribute('aria-pressed', String(searchOpts.regex));
  const searchHint = el('span', 'search-hint', '정규식: 복잡한 패턴은 화면을 잠시 멈출 수 있습니다 · 최대 10,000건 · 스크롤백 범위만');
  searchHint.dataset.role = 'search-hint';
  searchHint.hidden = !searchOpts.regex;
  search.append(sq, caseBtn, regexBtn, searchHint, scount, sclose);
  function setSearchOption(key: keyof SearchOptions): void {
    searchOpts[key] = !searchOpts[key];
    caseBtn.setAttribute('aria-pressed', String(searchOpts.caseSensitive));
    regexBtn.setAttribute('aria-pressed', String(searchOpts.regex));
    searchHint.hidden = !searchOpts.regex;
    hitAt = -1;
    runSearch(1);
    sq.focus();
  }
  const runSearch = (dir: 1 | -1) => {
    const cq = compileQuery(sq.value, searchOpts);
    const res = searchBuffer(term.term.buffer.active, term.term.cols, cq);
    hits = res.hits;
    if (cq.kind === 'error') scount.dataset.state = 'error';
    else delete scount.dataset.state;
    if (!hits.length) {
      hitAt = -1;
      scount.textContent = searchCountText(cq, res, -1);
      term.term.clearSelection();
      return;
    }
    hitAt = stepHit(hitAt, hits.length, dir);
    const h = hits[hitAt]!;
    term.term.select(h.col, h.row, h.cells);
    term.term.scrollToLine(Math.max(0, h.row - Math.floor(term.term.rows / 2)));
    scount.textContent = searchCountText(cq, res, hitAt);
  };
  sq.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); runSearch(e.shiftKey ? -1 : 1); }
    else if (e.key === 'Escape') { e.preventDefault(); setSearch(false); }
  });
  sq.addEventListener('input', () => { hitAt = -1; runSearch(1); });
  function setSearch(open: boolean): void {
    searchOpen = open;
    search.classList.toggle('open', open);
    searchBtn.setAttribute('aria-pressed', String(open));
    if (open) sq.focus();
    else { term.term.clearSelection(); if (view === 'terminal') term.focus(); }
  }

  // terminal surface: .term-wrap > .term.xslot > (.tsgutter + .xhost) ; HEX view; connection overlay
  const wrap = el('div', 'term-wrap');
  wrap.dataset.tab = channel;
  const slot = el('div', 'term xslot');
  slot.dataset.view = 'xterm';
  slot.dataset.tab = channel;
  slot.setAttribute('aria-label', `${channel} 터미널`);
  const gutter = el('div', 'tsgutter');
  gutter.setAttribute('aria-hidden', 'true');
  gutter.title = '호스트 수신 시각 HH:mm:ss.SSS (데몬 저널 시각, 장치 출력 아님)';
  const body = el('div', 'xhost uart-body');
  slot.append(gutter, body);
  const bytesHost = el('div', 'uart-bytes');
  bytesHost.hidden = true;
  bytesHost.setAttribute('data-testid', 'uart-bytes');
  const bytesInfo = el('div', 'uart-bytes-info');
  const bytesCopy = btn('선택 행 HEX 복사', '선택한 행의 원본 바이트를 "48 65 6C" 형식으로 복사', () => {
    const key = table.selected;
    const row = key ? bytes.rows.find((r) => r.key === key) : undefined;
    if (!row) return;
    const text = rowsToHex([row]);
    void navigator.clipboard?.writeText(text).then(
      () => ctx.ui.toast(`HEX 복사: ${text}`),
      () => ctx.ui.toast('클립보드 쓰기 실패'),
    );
  }, 'pill');
  bytesCopy.disabled = true;
  const bytesMeta = el('span', 'uart-bytes-meta');
  bytesInfo.append(bytesMeta, bytesCopy);
  const tableHost = el('div', 'uart-bytes-table');
  bytesHost.append(bytesInfo, tableHost);
  const overlay = el('div', 'conn-overlay');
  overlay.hidden = true;
  const overlayMsg = el('span', 'msg');
  const overlayBtn = el('button', 'btn sm blue');
  overlayBtn.type = 'button';
  overlayBtn.dataset.act = 'connect';
  overlayBtn.innerHTML = `${icon('plug')}다시 연결`;
  overlayBtn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('loom:uart-reopen', { detail: { channel } })));
  overlay.append(overlayMsg, overlayBtn);
  wrap.append(slot, bytesHost, overlay);
  const table = new VirtualTable<ByteRow>(tableHost, {
    columns: [
      { key: 't', header: '시각', width: '8.5em', cell: (r) => (r.wallMs ? fmtWall(r.wallMs) : '') },
      {
        key: 'd',
        header: '방향',
        width: '5.5em',
        cell: (r) => (r.kind === 'gap' ? '' : { ...originCell(r.origin), text: `${r.dir === 'tx' ? 'TX' : 'RX'} ${originCell(r.origin).text}` }),
      },
      { key: 'h', header: 'HEX', width: '50ch', cell: (r) => (r.kind === 'gap' ? { text: r.text ?? '', cls: 'gap-text' } : { text: rowHex(r), cls: 'mono uart-hex' }) },
      { key: 'a', header: 'ASCII', width: 'minmax(17ch, 1fr)', cell: (r) => (r.kind === 'gap' ? '' : { text: rowAscii(r), cls: 'mono uart-ascii' }) },
    ],
    keyOf: (r) => r.key,
    rowAttrs: (r) => ({ 'data-dir': r.dir ?? 'gap', 'data-origin': r.origin }),
    rowClass: (r) => (Number(r.key) % 2 === 0 ? 'uart-zebra' : ''),
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: (key) => {
      bytesCopy.disabled = key === null;
    },
    emptyText: '바이트 없음',
    ariaLabel: `UART ${channel} HEX 보기`,
  });

  const drawer = createSendString({
    channel,
    gate: () => {
      if (!hasWriteRight()) return { ok: false, reason: '보기 전용(watch)' };
      if (ctx.writeLocked) return { ok: false, reason: '쓰기 잠김(STALLED)' };
      if (!isOpenStatus(port.status)) return { ok: false, reason: `포트 ${port.status}` };
      if (paused) return { ok: false, reason: '화면 일시정지 중' };
      return { ok: true };
    },
    submit: (b) => {
      if (routeFor(b.length) === 'inputLarge') sendLarge(b, 'Send String 취소');
      else send(b);
    },
    onClose: () => setDrawer(false),
  });

  // macro run strip (REV6.1 .runstrip inside the target pane)
  const runStrip = el('div', 'runstrip');
  runStrip.hidden = true;
  let runKey = '';
  const renderRun = () => {
    const r = uartBench.runs.get(channel);
    const key = r ? `${r.id}:${r.status}:${r.i}:${r.steps.map((x) => x.st).join('')}:${r.reason ?? ''}:${r.err ?? ''}` : '';
    if (key === runKey) return;
    runKey = key;
    runStrip.hidden = !r;
    if (!r) { runStrip.replaceChildren(); return; }
    runStrip.className = `runstrip st-${r.status}`;
    runStrip.dataset.status = r.status;
    runStrip.dataset.run = r.id;
    const stTxt = { running: `실행 중 · ${r.i + 1}/${r.steps.length}`, done: '완료', failed: `실패 · ${r.err ?? ''}`, stopped: `${r.reason ?? '중지됨'} · step ${r.i + 1}에서` }[r.status];
    const top = el('div', 'run-top');
    const name = el('span', 'rname');
    name.innerHTML = icon('macro');
    name.append(` ${r.name}`);
    const tgt = el('span', 'rtarget', `→ ${channel}${port.path ? ` · ${port.path}` : ''}`);
    const st = el('span', `rstat ${r.status}`, stTxt);
    st.dataset.role = 'rstat';
    top.append(name, tgt, st);
    if (r.status === 'running') {
      const b = el('button', 'btn-stop');
      b.type = 'button';
      b.dataset.act = 'stop-macro';
      b.title = '이 대상의 매크로 중지: 이후 step은 송신하지 않음 (이미 보낸 바이트는 장치에 남음)';
      b.innerHTML = `${icon('stop')}중지`;
      b.addEventListener('click', () => uartBench.stopRun(channel));
      top.append(b);
    } else {
      const b = ibtn('dismiss-run', 'x', '실행 결과 닫기', () => uartBench.dismissRun(channel));
      top.append(b);
    }
    const steps = el('div', 'steps');
    r.steps.forEach((x, i) => {
      const sp = el('span', `step ${x.st === 'pending' ? '' : x.st}`);
      sp.dataset.step = String(i);
      sp.title = `${x.text}${x.note ? ` — ${x.note}` : ''}`;
      sp.append(el('span', undefined, `${i + 1}. ${x.text}`));
      if (x.note) sp.append(el('span', undefined, x.note));
      steps.append(sp);
    });
    // replace, never append: the strip shows exactly the channel's latest run (a stale '실행 중' row must not
    // survive the run's own failed/stopped/done re-render)
    runStrip.replaceChildren(top, steps);
  };

  // agent activity line (existing contract: last agent/script send + stage, expect wait — the user sees the agent)
  const status = el('div', 'uart-statusbar');
  const foot = el('div', 'uart-foot');
  foot.setAttribute('data-testid', 'uart-status');
  status.append(foot);
  root.append(createLatchLine(ctx), head, lease, search, wrap, drawer.root, runStrip, status);

  function applyTheme(): void {
    wrap.className = `term-wrap t-${theme}`;
    wrap.dataset.theme = theme;
    term.term.options.theme = XTHEME[theme];
    themeBtn.innerHTML = icon('contrast');
    themeBtn.title = `이 터미널만 ${theme === 'dark' ? '밝게' : '어둡게'}`;
    themeBtn.setAttribute('aria-label', `터미널 테마 전환 (현재 ${theme === 'dark' ? '어두움' : '밝음'})`);
  }
  applyTheme();

  function setView(v: 'terminal' | 'hex'): void {
    if (view === v) return;
    view = v;
    const toHex = v === 'hex';
    if (toHex && document.activeElement instanceof HTMLElement && wrap.contains(document.activeElement)) document.activeElement.blur();
    slot.hidden = toHex;
    bytesHost.hidden = !toHex;
    if (!toHex) {
      fitBody();
      term.focus();
    } else table.followTail();
    ctx.scheduler.invalidate();
  }
  function setDrawer(open: boolean): void {
    drawer.setOpen(open);
    fitBody();
    sendStrBtn.setAttribute('aria-expanded', String(open));
    sendStrBtn.setAttribute('aria-pressed', String(open));
    if (!open && view === 'terminal') term.focus();
    ctx.scheduler.invalidate();
  }

  // ---- display pause (the panel holds chunks with their journal time; recording/counters continue) ----
  let held: { b: Uint8Array; t: number }[] = [];
  let heldBytes = 0;
  let heldDropped = 0;
  function setPaused(p: boolean): void {
    paused = p;
    term.setPaused(p); // input semantics (keys not sent except 0x03)
    if (!p) {
      const q = held;
      held = [];
      heldBytes = 0;
      for (const c of q) writeChunk(c.b, c.t);
      if (heldDropped) { term.markGap(`· 표시 누락 ${heldDropped} B (일시정지 버퍼 초과 — 기록·저널은 온전)`); heldDropped = 0; }
    }
    ctx.scheduler.invalidate();
  }

  // ---- receive-time gutter (REV4): xterm markers per row, stamped with the journal host time ----
  let tsMarks: { mk: { line: number; isDisposed: boolean; dispose(): void }; t: number }[] = [];
  let lastMark: { line: number; isDisposed: boolean } | null = null;
  let gRaf = 0;
  const markRows = (t: number, nl: number) => {
    const xt = term.term;
    const b = xt.buffer.active;
    const cur = b.baseY + b.cursorY;
    const start = lastMark && !lastMark.isDisposed ? lastMark.line + 1 : Math.max(0, cur - nl);
    const end = b.cursorX > 0 ? cur : cur - 1; // an empty cursor row is stamped when its first byte arrives
    for (let y = start; y <= end; y++) {
      let mk: { line: number; isDisposed: boolean; dispose(): void } | undefined;
      try {
        mk = xt.registerMarker(y - cur);
      } catch {
        mk = undefined;
      }
      if (!mk) continue;
      tsMarks.push({ mk, t });
      lastMark = mk;
    }
    if (tsMarks.length > 12_000) tsMarks = tsMarks.filter((m) => !m.mk.isDisposed);
    gutterSoon();
  };
  const cell = (): { width: number; height: number } => {
    try {
      const c = (term.term as unknown as { _core: { _renderService: { dimensions: { css: { cell: { width: number; height: number } } } } } })._core._renderService.dimensions.css.cell;
      if (c && c.width > 0 && c.height > 0) return c;
    } catch {
      /* private API changed: estimate */
    }
    return { width: 7.6, height: Math.ceil(12.5 * 1.2 * 1.18) };
  };
  function gutterSoon(): void {
    if (gRaf || typeof requestAnimationFrame === 'undefined') return;
    gRaf = requestAnimationFrame(() => { gRaf = 0; renderGutter(); });
  }
  function renderGutter(): void {
    const on = uartBench.tabPrefs(channel).ts;
    slot.dataset.ts = on ? 'on' : 'off';
    if (!on || view !== 'terminal') return;
    const xt = term.term;
    const b = xt.buffer.active;
    const ch = cell().height;
    const map = new Map<number, number>();
    for (const m of tsMarks) if (!m.mk.isDisposed) map.set(m.mk.line, m.t);
    const frag = document.createDocumentFragment();
    for (let i = 0; i < xt.rows; i++) {
      const y = b.viewportY + i;
      const l = b.getLine(y);
      const tt = l && !l.isWrapped ? map.get(y) : undefined;
      const r = el('div', 'tsr', tt ? fmtTs(tt) : l?.isWrapped ? '↳' : '');
      r.style.height = `${ch}px`;
      r.style.lineHeight = `${ch}px`;
      frag.append(r);
    }
    gutter.replaceChildren(frag);
    gutter.dataset.rows = String(xt.rows);
  }
  term.term.onScroll(() => gutterSoon());
  /** device bytes → xterm unchanged; the gutter stamp is UI metadata applied after the chunk is parsed */
  function writeChunk(b: Uint8Array, t: number): void {
    let firstRow = 0;
    try {
      const buf = term.term.buffer.active;
      firstRow = buf.baseY + buf.cursorY;
    } catch {
      /* terminal not open (tests) */
    }
    term.write(b);
    let nl = 0;
    for (const x of b) if (x === 0x0a) nl++;
    try {
      term.term.write('', () => {
        markRows(t, nl);
        applyHighlights(firstRow);
      });
    } catch {
      /* terminal not open (tests) */
    }
  }

  // Fit the actual remaining body so xterm's canvas cannot cover sibling controls.
  const fitBody = () => {
    if (view !== 'terminal') return;
    // only the real host box counts (the gutter and header reduce it); never the dock size
    const w = body.clientWidth;
    const h = body.clientHeight;
    const c = cell();
    if (w > 0 && h > 0) term.fit(Math.max(20, Math.floor((w - 12) / c.width)), Math.max(3, Math.floor(h / c.height)));
    gutterSoon();
  };
  const fitSoon = () => {
    if (typeof requestAnimationFrame === 'undefined') return fitBody();
    requestAnimationFrame(() => fitBody());
  };
  const bodyObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(fitBody);

  const offAppend = mirror.onAppend(() => {
    // the port model follows the engine even while this tab is hidden; recipients/scope labels re-resolve
    if (syncPort()) uartBench.bump();
    ctx.scheduler.invalidate();
  });
  const tick = setInterval(() => ctx.scheduler.invalidate(), 1_000); // counters keep moving
  const offBench = uartBench.subscribe(() => ctx.scheduler.invalidate());

  // ---- REC (portable live-save) ----
  const recState = (): string => (recPicking ? 'picking' : rec ? rec.state : 'idle');
  const recChanged = () => { ctx.scheduler.invalidate(); if (recDialog?.open) fillRecDialog(); };
  async function startRec(sink: 'file' | 'buffer', prefs: RecPrefs): Promise<void> {
    uartBench.recPrefs[channel] = prefs;
    uartBench.bump(true);
    if (sink === 'buffer') { rec = Recorder.buffer(channel, prefs, recChanged); recChanged(); return; }
    recPicking = true;
    recChanged();
    try {
      const r = await Recorder.file(channel, prefs, recChanged);
      if (r) rec = r;
      else ctx.ui.toast('파일 선택 취소 — 기록하지 않음');
    } catch (e) {
      ctx.ui.toast(`기록 시작 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      recPicking = false;
      recChanged();
    }
  }
  /** B5 §B5.2: run one journal export (one at a time) and hand the file to the download path. */
  async function startJournalExport(range: ExportRange, format: ExportFormat): Promise<void> {
    if (jx) return;
    const ac = new AbortController();
    jx = { ac, progress: { phase: 'locate', pages: 0, events: 0, outBytes: 0 } };
    recChanged();
    try {
      const r = await runExport({
        query: (rpc, p, sig) => ctx.query(rpc, p, sig),
        channel,
        range,
        format,
        signal: ac.signal,
        nowMs: Date.now(),
        onProgress: (p) => {
          if (jx) jx.progress = p;
          recChanged();
        },
      });
      jxLast = r;
      if (r.incomplete.includes('cancelled')) {
        ctx.ui.toast('저널 내보내기 취소 — 파일을 만들지 않았습니다');
      } else {
        const d = downloadText(r.fileName, r.text, format);
        ctx.ui.toast(
          d.requested
            ? `다운로드 요청: ${r.fileName} (${fmtBytes(d.bytes)}) · ${summaryText(r)} — 저장 완료 여부는 브라우저/앱의 다운로드에서 확인하세요`
            : `다운로드를 요청할 수 없습니다 (${r.fileName}) — 이 환경은 다운로드를 지원하지 않습니다`,
        );
      }
    } catch (e) {
      ctx.ui.toast(`저널 내보내기 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      jx = null;
      recChanged();
    }
  }
  function fillRecDialog(): void {
    const d = recDialog;
    if (!d) return;
    const prefs: RecPrefs = { ...DEFAULT_REC_PREFS, ...uartBench.recPrefs[channel] };
    const st = recState();
    const running = st === 'recording' || st === 'picking' || st === 'stopping';
    d.innerHTML = `<form method="dialog" novalidate>
      <div class="dlg-h">${icon('file')}<h2>UART 기록 — ${channel}</h2><span class="pill rec rec-${st}" data-role="recstat">${({ idle: '대기', picking: '파일 선택 중', recording: rec?.sink === 'file' ? '● 파일 기록 중' : '● 버퍼 기록 중', stopping: '저장 중…', closed: '종료됨', error: '오류' } as Record<string, string>)[st]}</span></div>
      <div class="dlg-b">
        <fieldset ${running ? 'disabled' : ''}><legend>형식</legend><div class="chk-row">
          <label class="chk"><input type="radio" name="format" value="log" ${prefs.format === 'log' ? 'checked' : ''}> 읽기 쉬운 텍스트 · 줄 단위 (.log)</label>
          <label class="chk"><input type="radio" name="format" value="jsonl" ${prefs.format === 'jsonl' ? 'checked' : ''}> 원문 · 조각 그대로 즉시 (.jsonl)</label></div>
          <div class="chk-row" style="margin-top:6px">
          <label class="chk"><input type="checkbox" name="rx" ${prefs.rx ? 'checked' : ''}> 수신 RX</label>
          <label class="chk"><input type="checkbox" name="tx" ${prefs.tx ? 'checked' : ''}> 송신 TX</label>
          <label class="chk"><input type="checkbox" name="fileTs" ${prefs.fileTs || prefs.format === 'jsonl' ? 'checked' : ''} ${prefs.format === 'jsonl' ? 'disabled' : ''}> 줄마다 시각 (.log)</label></div></fieldset>
        <div class="hint" data-role="recbytes"></div>
        <pre class="preview" data-role="rectail"></pre>
        <div class="err-text" data-role="err"></div>
        <p class="hint">데몬 저널은 이 기록과 별개로 항상 남습니다(헤더 REC). 파일 기록은 이 브라우저가 파일 저장 선택기를 제공할 때만 가능하며${hasFilePicker() ? '' : ' — <b>이 환경에는 없어 버퍼 기록 후 내보내기를 씁니다</b>'}. 새로 고침해도 기록은 자동으로 다시 시작하지 않습니다.</p>
        <fieldset data-role="jx" ${jx ? 'disabled' : ''}><legend>지난 구간 내보내기 (데몬 저널)</legend><div class="chk-row">
          <label class="chk"><input type="radio" name="jxrange" value="retained" ${jxSel.kind === 'retained' ? 'checked' : ''}> 보존 범위 전체</label>
          <label class="chk"><input type="radio" name="jxrange" value="recent" ${jxSel.kind === 'recent' ? 'checked' : ''}> 최근 <select data-act="jx-minutes">${EXPORT_RECENT_MINUTES.map((m) => `<option value="${m}" ${m === jxSel.minutes ? 'selected' : ''}>${m}분</option>`).join('')}</select></label>
          <label class="chk"><input type="radio" name="jxrange" value="generation" ${jxSel.kind === 'generation' ? 'checked' : ''} ${port.generation === null ? 'disabled' : ''}> 현재 연결 (gen ${port.generation ?? '?'})</label></div></fieldset>
        <div class="hint" data-role="jxstat"></div>
        <p class="hint">REC를 켜기 전 구간도 데몬 저널 보존 범위 안이면 내보낼 수 있습니다. 형식은 위 선택(.log/.jsonl)을 따릅니다. 이전 데몬 실행의 기록은 포함하지 않으며, 파일 머리글에 실제 범위·완전성·공백을 적습니다.</p>
        <fieldset data-role="nlog"><legend>네이티브 로그 파일 (데몬 쪽, 계속 기록)</legend>
          <div class="hint">$LOOM_HOME/uart/logs/${channel}/ — 열기 옵션 또는 여기서 시작 · 용량 한도에서 멈춤(삭제 없음) · 파일에 stream seq 없음</div>
          <div class="hint" data-role="nlogstat"></div>
          <div class="chk-row">${port.log?.active ? '<button type="button" class="btn" data-act="nlog-stop">로그 중지</button>' : '<button type="button" class="btn" data-act="nlog-start">로그 시작</button>'}</div></fieldset>
      </div>
      <div class="dlg-f">
        ${rec && rec.sink === 'buffer' && rec.state !== 'recording' && rec.state !== 'stopping' ? '<button type="button" class="btn" data-act="rec-export">' + icon('down') + '내보내기</button>' : ''}
        ${jx ? '<button type="button" class="btn" data-act="jx-cancel">' + icon('stop') + '내보내기 취소</button>' : '<button type="button" class="btn" data-act="jx-start">' + icon('down') + '저널에서 내보내기</button>'}
        <span class="grow"></span>
        <button type="button" class="btn" data-act="rec-close">닫기</button>
        ${running ? '<button type="button" class="btn danger" data-act="rec-stop">' + icon('stop') + '기록 중지</button>'
          : `<button type="button" class="btn" data-act="rec-buffer" title="파일이 아님 — 메모리 5 MiB, 끝난 뒤 내보내기">버퍼에 기록</button><button type="button" class="btn primary" data-act="rec-file" ${hasFilePicker() ? '' : 'disabled title="이 환경에는 파일 저장 선택기가 없습니다"'}>파일에 기록…</button>`}
      </div></form>`;
    const form = d.querySelector('form')!;
    const fmt = () => (form.querySelector<HTMLInputElement>('input[name=format]:checked')?.value === 'jsonl' ? 'jsonl' : 'log') as RecFormat;
    const read = (): RecPrefs => ({ format: fmt(), rx: !!form.querySelector<HTMLInputElement>('input[name=rx]')?.checked, tx: !!form.querySelector<HTMLInputElement>('input[name=tx]')?.checked, fileTs: !!form.querySelector<HTMLInputElement>('input[name=fileTs]')?.checked });
    form.querySelectorAll<HTMLInputElement>('input[name=format]').forEach((i) => i.addEventListener('change', () => { uartBench.recPrefs[channel] = read(); fillRecDialog(); }));
    const bytesEl = form.querySelector<HTMLElement>('[data-role=recbytes]')!;
    const tailEl = form.querySelector<HTMLElement>('[data-role=rectail]')!;
    const errEl = form.querySelector<HTMLElement>('[data-role=err]')!;
    if (rec) {
      bytesEl.textContent = `${rec.name} · RX ${fmtBytes(rec.rxBytes)} · TX ${fmtBytes(rec.txBytes)} · ${rec.sink === 'file' ? `저장됨 ${fmtBytes(rec.committed)} · 쓰는 중 ${fmtBytes(rec.pending)}${rec.lastSavedAt ? ` · 마지막 저장 ${fmtTs(rec.lastSavedAt)}` : ''}` : `버퍼 ${fmtBytes(rec.committed)}`}${rec.reason ? ` · ${rec.reason}` : ''}`;
      tailEl.textContent = rec.tail.join('\n') || '(아직 줄 없음)';
      errEl.textContent = rec.error ?? '';
    } else tailEl.hidden = true;
    const jxStat = form.querySelector<HTMLElement>('[data-role=jxstat]')!;
    if (jx) {
      const pr = jx.progress;
      jxStat.textContent = pr.phase === 'locate' ? '범위 찾는 중…' : `저널 읽는 중 · 페이지 ${pr.pages} · ${pr.events}건 · ${fmtBytes(pr.outBytes)}`;
    } else jxStat.textContent = jxLast ? `마지막 내보내기: ${summaryText(jxLast)}` : '';
    form.querySelectorAll<HTMLInputElement>('input[name=jxrange]').forEach((i) =>
      i.addEventListener('change', () => {
        if (i.checked) jxSel.kind = i.value as ExportRange['kind'];
      }),
    );
    form.querySelector<HTMLSelectElement>('[data-act=jx-minutes]')?.addEventListener('change', (e) => {
      const m = Number((e.target as HTMLSelectElement).value);
      if (Number.isFinite(m)) jxSel.minutes = m;
    });
    d.querySelector('[data-act=jx-start]')?.addEventListener('click', () => {
      const range: ExportRange =
        jxSel.kind === 'recent'
          ? { kind: 'recent', minutes: jxSel.minutes }
          : jxSel.kind === 'generation' && port.generation !== null
            ? { kind: 'generation', generation: port.generation }
            : { kind: 'retained' };
      void startJournalExport(range, fmt());
    });
    d.querySelector('[data-act=jx-cancel]')?.addEventListener('click', () => jx?.ac.abort());
    const nlogStat = form.querySelector<HTMLElement>('[data-role=nlogstat]')!;
    nlogStat.textContent = port.log?.active ? `기록 중 · ${port.log.file ?? ''}` : port.log ? `중지됨 (${port.log.stopped ?? '-'})` : '꺼짐';
    d.querySelector('[data-act=nlog-start]')?.addEventListener('click', () => void act('logStart', { channel, format: fmt() }));
    d.querySelector('[data-act=nlog-stop]')?.addEventListener('click', () => void act('logStop', { channel }));
    d.querySelector('[data-act=rec-close]')?.addEventListener('click', () => d.close());
    d.querySelector('[data-act=rec-buffer]')?.addEventListener('click', () => void startRec('buffer', read()));
    d.querySelector('[data-act=rec-file]')?.addEventListener('click', () => void startRec('file', read()));
    d.querySelector('[data-act=rec-stop]')?.addEventListener('click', () => void rec?.stop('사용자 중지'));
    d.querySelector('[data-act=rec-export]')?.addEventListener('click', () => {
      if (!rec) return;
      const r = rec.exportBuffer();
      ctx.ui.toast(r.requested ? `다운로드 요청: ${r.name} (${fmtBytes(r.bytes)}) — 저장 완료 여부는 브라우저/앱의 다운로드에서 확인하세요` : `다운로드를 요청할 수 없습니다 (${r.name}) — 이 환경은 다운로드를 지원하지 않습니다`);
    });
  }
  function openRecDialog(): void {
    if (!recDialog) {
      const layer = document.createElement('div');
      layer.className = 'ub-layer';
      recDialog = document.createElement('dialog');
      recDialog.setAttribute('aria-label', 'UART 기록');
      recDialog.id = `dlgRec-${channel}`;
      layer.append(recDialog);
      document.body.append(layer);
    }
    fillRecDialog();
    try {
      recDialog.showModal();
    } catch {
      recDialog.setAttribute('open', '');
    }
  }

  const consume = (): void => {
    const t = now();
    syncPort(); // port state first (model), then the drawn views
    const snapshot = mirror.snapshot;
    if (snapshot && snapshot.version !== snapshotVersion) {
      snapshotVersion = snapshot.version;
      for (const item of snapshot.items) {
        if (item.domain !== 'uart' || item.kind !== 'uart.snapshot') continue;
        const state = item.state as Record<string, unknown> | null;
        if (!state || (item.channel ?? state.channel) !== channel) continue;
        input = restoreUartState(state, t).input;
      }
    }
    if (mirror.overwritten !== lastOverwritten) {
      const text = `⋯ 화면 생략 ${mirror.overwritten - lastOverwritten}건 (패널 버퍼) — 기록은 데몬 저널`;
      term.markGap(text);
      bytes.gap(text);
      countsPartial = true;
      lastOverwritten = mirror.overwritten;
    }
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (snapshot && lastSeq <= snapshot.throughSeq && ev.kind !== 'uart.data' && ev.kind !== 'uart.write') continue;
      handle(ev, t);
    }
    while (gapsSeen < mirror.gaps.length) {
      const g = mirror.gaps[gapsSeen++];
      if (g) {
        term.markGap(gapText(g));
        bytes.gap(gapText(g));
        rec?.note(`데몬 보고 누락: ${gapText(g)}`);
        countsPartial = true;
      }
    }
    if (mirror.snapshotDone && !separatorDone) {
      separatorDone = true;
      term.markSeparator('── 백로그 / 라이브 ──');
    }
  };

  const handle = (ev: LoomEvent, t: number): void => {
    const p = ev.payload as Record<string, unknown>;
    if (ev.kind === 'uart.data') {
      const data = decodePayloadBytes(p.bytes ?? p);
      if (!data) return;
      // byte view + observed counters use the ORIGINAL payload bytes (never the symbolized text)
      const dir = ev.dir === 'tx' ? 'tx' : 'rx';
      const wall = wallOf(ev);
      bytes.append(dir, ev.origin, wall, data);
      if (separatorDone) rec?.feed(dir, data, wall || Date.now()); // live chunks only — the backlog predates REC
      if (dir === 'tx') txObserved += data.length;
      else rxObserved += data.length;
      if (ev.dir === 'tx') {
        if (ev.origin !== 'device') {
          // the tx bytes of the send already shown keep its stage (control and data paths
          // may deliver the write row and the tx bytes in either order)
          const same = lastSend !== null && ev.request_id && lastSend.requestId === ev.request_id;
          // an echo without request_id (e.g. raw user keys) right after a write row of the same origin
          // keeps that row's request_id instead of blanking the status line's data-request-id
          const keep = !ev.request_id && lastSend !== null && lastSend.requestId !== null && lastSend.origin === ev.origin && t - lastSend.atMs < 1000;
          lastSend = {
            origin: ev.origin,
            text: symbolize(data),
            requestId: ev.request_id ?? (keep ? lastSend!.requestId : null),
            state: same || keep ? (lastSend?.state ?? null) : null,
            atMs: same || keep ? (lastSend?.atMs ?? t) : t,
          };
        }
        return; // the device echo arrives as rx — never draw tx bytes ourselves
      }
      if (paused) {
        if (heldBytes + data.length > PAUSE_CAP) heldDropped += data.length;
        else { held.push({ b: data, t: wall || Date.now() }); heldBytes += data.length; }
      } else writeChunk(data, wall || Date.now());
      rx.push({ atMs: t, n: data.length });
      return;
    }
    if (ev.kind === 'uart.write') {
      const ws = p.write_state as WriteState | undefined;
      if (lastSend && ev.request_id && lastSend.requestId === ev.request_id) lastSend.state = ws ?? lastSend.state;
      else if (ev.origin === 'agent' || ev.origin === 'script') {
        const b = decodePayloadBytes(p.bytes);
        lastSend = {
          origin: ev.origin,
          text: b ? symbolize(b) : typeof p.text === 'string' ? symbolize(p.text) : '…',
          requestId: ev.request_id ?? null,
          state: ws ?? null,
          atMs: t,
        };
      }
      return;
    }
    applyPort(ev);
    input = reduceInput(input, ev, t);
  };
  /**
   * Model-only port sync: applies the snapshot's port state and every uart.state / uart.config row the mirror
   * holds, whether or not this panel is drawn (a hidden dock tab is never drawn, yet the workbench asks it for
   * writability). Incremental and idempotent (own cursors + applyPort's seq guard); renders nothing. Unknown stays
   * unknown until the engine reports — it is never treated as open.
   */
  function syncPort(): boolean {
    const before = port.status;
    const snapshot = mirror.snapshot;
    if (snapshot && snapshot.version !== portSnapVersion) {
      portSnapVersion = snapshot.version;
      for (const item of snapshot.items) {
        if (item.domain !== 'uart' || item.kind !== 'uart.snapshot') continue;
        const state = item.state as Record<string, unknown> | null;
        if (!state || (item.channel ?? state.channel) !== channel) continue;
        // the snapshot is as of throughSeq: a newer live port state (e.g. disconnected) already applied wins
        if (snapshot.throughSeq >= portSeq) {
          port = restoreUartState(state, now()).port;
          portSeq = snapshot.throughSeq;
        }
      }
    }
    for (const ev of mirror.after(portScanSeq)) {
      portScanSeq = BigInt(ev.seq);
      if (snapshot && portScanSeq <= snapshot.throughSeq) continue; // already folded into the snapshot
      applyPort(ev);
    }
    return port.status !== before;
  }
  /** uart.state / uart.config with a per-port seq guard (control path and data/backlog path share it) */
  function applyPort(ev: LoomEvent): boolean {
    if (!PORT_KINDS.includes(ev.kind)) return false;
    let seq: bigint;
    try {
      seq = BigInt(ev.seq);
    } catch {
      return false;
    }
    if (seq <= portSeq) return false;
    portSeq = seq;
    const before = port.status;
    port = reducePort(port, ev);
    if (ev.kind === 'uart.log' && recDialog?.open) fillRecDialog();
    if (ev.kind === 'uart.state' && port.status !== before) {
      openSinceWallMs = isOpenStatus(port.status) ? wallOf(ev) || null : null;
      if (before !== '?' && !isOpenStatus(port.status)) {
        // REV4: disconnect/unplug stops and closes REC with the reason; reconnect never resumes it
        if (rec && rec.state === 'recording') void rec.stop(/disconn|lost/i.test(port.status) ? '장치 분리로 기록 종료' : '포트 닫힘으로 기록 종료');
        uartBench.stopRun(channel, /disconn|lost/i.test(port.status) ? '장치 분리로 중단' : '포트 닫힘으로 중단');
      }
    }
    return true;
  }
  // Live port state is a priority kind: it reaches data subscriptions only via snapshot/backlog, so apply it from
  // the control path too (same seq guard; an older backlog row or snapshot never overrides it).
  const offControl = ctx.onControl((ev) => {
    if ((ev.channel ?? (ev.payload as { channel?: unknown } | null)?.channel) !== channel) return;
    const before = port.status;
    if (!applyPort(ev)) return;
    if (port.status !== before) uartBench.bump(); // recipients re-resolve even when this tab is hidden
    ctx.scheduler.invalidate();
  });

  const groupEl = () => root.closest<HTMLElement>('.dv-groupview');
  const offRegister = uartBench.registerTerminal({
    channel,
    blocked: blockedReason,
    sendRaw: (b) => send(b),
    path: () => port.path ?? null,
    frame: () => (port.baud ? `${port.baud} ${(frameText(port) ?? '').replace(/-/g, '')}`.trim() : ''),
    status: () => port.status,
    counters: () => ({ rx: rxObserved, tx: txObserved }),
    focus: () => term.focus(),
    activate: () => { root.closest('.dv-groupview')?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); term.focus(); },
    paneId: () => groupEl()?.dataset.paneNo ?? null,
    isActive: () => !!groupEl()?.classList.contains('dv-active-group') && root.offsetParent !== null,
    visible: () => root.isConnected && root.offsetParent !== null,
    invalidate: () => ctx.scheduler.invalidate(),
  });
  const noteActive = () => uartBench.noteActive(channel);
  root.addEventListener('focusin', noteActive);
  root.addEventListener('pointerdown', noteActive);

  const draw = (): bigint => {
    consume();
    const t = now();
    const writable = canWrite();
    if (term.readonly === writable) term.setReadonly(!writable);
    const sess = uartBench.sessionFor(channel);
    const prefs = uartBench.tabPrefs(channel);
    if (prefs.theme !== theme) { theme = prefs.theme; applyTheme(); }
    root.style.setProperty('--tag', sess?.color ?? 'var(--accent)');
    dot.style.setProperty('--tag', sess?.color ?? 'var(--muted)');
    portEl.textContent = `UART ${channel} ● ${port.status}${port.baud ? ` ${port.baud}` : ''}${
      port.generation !== null ? ` gen${port.generation}` : ''
    } [${(ctx.env ?? '?').toUpperCase()}]`;
    const ip = identityPlan(headMode);
    pathEl.textContent = port.path ?? sess?.port ?? '';
    pathEl.hidden = !ip.path;
    countEl.hidden = !ip.counts;
    const frame = frameText(port);
    const fullFrame = [port.baud ? `${port.baud} ${frame ? frame.replace(/-/g, '') : ''}`.trim() : '', port.flow ?? '', prefs.eol].filter(Boolean).join(' · ');
    frameEl.textContent = ip.frame === 'full' ? fullFrame : port.baud ? String(port.baud) : '';
    frameEl.title = `${port.path ?? ''}${frame ? ` / ${frame}` : ''}${openSinceWallMs !== null && isOpenStatus(port.status) ? ` · 연결 ${hms(Date.now() - openSinceWallMs)}` : ''}${port.generation !== null ? ` · gen${port.generation}` : ''}`;
    const cc = connClass(port.status);
    dot.dataset.conn = cc;
    target.title = identityTitle({ channel, stateLabel: CONN_LABEL[port.status] ?? port.status, path: port.path ?? sess?.port ?? null, frame: fullFrame, rx: fmtBytes(rxObserved), tx: fmtBytes(txObserved) });
    stateEl.className = `ph-state ${cc}`;
    stateEl.textContent = CONN_LABEL[port.status] ?? port.status;
    const bc = uartBench.bc;
    bcChip.hidden = !bc.on;
    if (bc.on) {
      const inn = bc.ids.includes(channel);
      bcChip.className = `chip ${inn ? 'bc-in' : 'bc-out'}`;
      bcChip.textContent = inn ? 'ALL' : 'ALL 제외';
      bcChip.title = inn ? '연결된 모두 송신 수신자' : '연결된 모두 송신에서 제외됨';
    }
    countEl.innerHTML = '';
    countEl.append('RX ', el('b', undefined, fmtBytes(rxObserved)), ' · TX ', el('b', undefined, fmtBytes(txObserved)));
    if (countsPartial) countEl.append(' (누락 구간 있음)');
    countEl.title = `이 패널이 받은 이벤트(백로그 포함) 기준 — 장치·데몬 누계 아님 · 관측 TX ${txObserved.toLocaleString('ko-KR')} B / RX ${rxObserved.toLocaleString('ko-KR')} B`;
    const chip = inputChip(input, ctx.connId, paused, t);
    chipEl.textContent = chip.text;
    chipEl.dataset.kind = chip.kind;
    // the lease row appears when the user opens it or the lease needs attention (agent lock/deferred/HOLD/dirty/other)
    const attention = chip.kind === 'hold' || chip.kind === 'lock' || chip.kind === 'deferred' || chip.kind === 'dirty' || chip.kind === 'other';
    chipEl.hidden = !(attention || chip.kind === 'paused');
    lease.hidden = !leaseOpen;
    moreBtn.setAttribute('aria-pressed', String(leaseOpen));
    moreBtn.setAttribute('aria-expanded', String(leaseOpen));
    yieldBtn.disabled = !writable || !chip.canYield;
    clearBtn.disabled = !writable || !chip.canClear;
    takeBtn.disabled = !writable || !chip.canTakeover;
    holdBtn.disabled = !writable;
    holdBtn.textContent = input.hold ? 'HOLD 해제' : 'HOLD';
    holdBtn.setAttribute('aria-pressed', String(input.hold));
    pauseBtn.setAttribute('aria-pressed', String(paused));
    pauseBtn.textContent = paused ? `재개 +${fmtBytes(heldBytes)}` : '일시정지';
    tsBtn.setAttribute('aria-pressed', String(prefs.ts));
    if (eolSel.value !== prefs.eol) eolSel.value = prefs.eol;
    termViewBtn.setAttribute('aria-pressed', String(view === 'terminal'));
    hexViewBtn.setAttribute('aria-pressed', String(view === 'hex'));
    sendStrBtn.setAttribute('aria-expanded', String(drawer.isOpen));
    const open = isOpenStatus(port.status);
    plugBtn.dataset.act = open ? 'disconnect' : 'connect';
    plugBtn.innerHTML = icon(open ? 'unplug' : 'plug');
    plugBtn.title = open ? '포트 닫기' : '포트 열기 (저장된 세션 설정으로)';
    plugBtn.setAttribute('aria-label', plugBtn.title);
    plugBtn.disabled = !hasWriteRight();
    const rs = recState();
    recBtn.dataset.recState = rs;
    recBtn.className = `pill rec rec-${rs}`;
    recBtn.textContent = rs === 'recording' && rec ? (rec.sink === 'file' ? `● 저장 ${fmtBytes(rec.committed)}` : `● 버퍼 ${fmtBytes(rec.committed)}`) : rs === 'error' ? 'REC 오류' : 'REC';
    drawer.update();
    // connection overlay (REV6.1): a closed/lost port keeps its log, reconnect is explicit
    const showOverlay = port.status !== '?' && !open;
    overlay.hidden = !showOverlay;
    overlay.dataset.conn = cc;
    if (showOverlay) {
      overlayMsg.innerHTML = '';
      const bmsg = el('b', undefined, `${port.path ?? channel} ${CONN_LABEL[port.status] ?? port.status}.`);
      overlayMsg.append(bmsg, ' 로그는 남아 있습니다.');
      overlayBtn.hidden = !hasWriteRight();
    }
    renderRun();
    const tsAttr = prefs.ts ? 'on' : 'off';
    if (slot.dataset.ts !== tsAttr) { slot.dataset.ts = tsAttr; fitSoon(); } // the gutter changes the host width

    if (view === 'hex' && !paused) {
      table.setItems(bytes.rows);
      table.render();
    }
    bytesMeta.textContent = `원본 바이트 ${bytes.bytes.toLocaleString('ko-KR')} B · ${bytes.rows.length.toLocaleString('ko-KR')}행${
      bytes.evictedRows ? ` · 보기 범위 밖 ${bytes.evictedBytes.toLocaleString('ko-KR')} B/${bytes.evictedRows.toLocaleString('ko-KR')}행` : ''
    } · 키 입력은 터미널 보기에서만 전송${paused ? ' · 일시정지' : ''}`;

    while (rx.length && t - (rx[0] as { atMs: number }).atMs > RX_WINDOW_MS) rx.shift();
    const rate = rx.reduce((a, x) => a + x.n, 0);
    const st = term.stats;
    const parts: string[] = [];
    if (lastSend) {
      const tag = lastSend.origin === 'agent' ? 'AGT' : lastSend.origin === 'script' ? 'SCR' : 'USR';
      const stage = lastSend.state ? STAGE_CHIP[lastSend.state] : '';
      parts.push(`${tag} ▶ "${lastSend.text}" ${stage} ${Math.round((t - lastSend.atMs) / 1000)}s 전`);
    }
    const ex = expectText(input, t);
    if (ex) parts.push(ex);
    parts.push(`rx ${rate < 1024 ? `${rate} B/s` : `${(rate / 1024).toFixed(1)} KB/s`}`);
    const ex2 = portExtrasText(port);
    if (ex2) parts.push(ex2);
    if (st.blockedQueries || st.blockedAutoInput) parts.push(`차단 질의×${st.blockedQueries} 자동응답×${st.blockedAutoInput}`);
    if (st.droppedBytes) parts.push(`화면 생략 ${st.droppedBytes} B`);
    if (ctx.recordHealth !== 'ok') parts.push(`REC ${ctx.recordHealth}`);
    if (!hasWriteRight()) parts.push('보기 전용(watch)');
    else if (!writable) parts.push('쓰기 잠김(STALLED)');
    foot.textContent = parts.join(' │ ');
    // U-2: the status line names the last send (UI-T02 compares it with the RPC request_id)
    if (lastSend?.requestId) foot.dataset.requestId = lastSend.requestId;
    else delete foot.dataset.requestId;
    if (lastSend) {
      foot.dataset.origin = lastSend.origin;
      if (lastSend.state) foot.dataset.state = lastSend.state;
      else delete foot.dataset.state;
    } else {
      delete foot.dataset.origin;
      delete foot.dataset.state;
    }
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);

  return {
    mount(host) {
      host.append(root);
      applyHeaderMode();
      headObserver?.observe(head);
      term.open(body);
      bodyObserver?.observe(body);
      fitSoon();
      ctx.scheduler.invalidate();
    },
    resize() {
      applyHeaderMode();
      fitSoon();
      ctx.scheduler.invalidate();
    },
    dispose() {
      clearInterval(tick);
      jx?.ac.abort();
      for (const d of hlDecos.splice(0)) {
        try {
          d.dispose();
        } catch {
          /* already disposed */
        }
      }
      if (gRaf && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(gRaf);
      bodyObserver?.disconnect();
      headObserver?.disconnect();
      table.dispose();
      offAppend();
      offControl();
      offFrame();
      offBench();
      offRegister();
      if (rec && rec.state === 'recording') {
        void rec.stop('탭 닫힘으로 기록 종료');
        ctx.ui.toast(`${channel}: 탭을 닫아 기록을 종료했습니다`);
      }
      recDialog?.parentElement?.remove();
      root.removeEventListener('focusin', noteActive);
      root.removeEventListener('pointerdown', noteActive);
      term.dispose();
      root.remove();
    },
  };
}

const uartActions = (
  [
    ['input', 'uart.input', 'stream', 'none'],
    ['inputLarge', 'uart.input', 'request', 'card'],
    ['yield', 'uart.input.yield', 'request', 'none'],
    ['clear', 'uart.input.clear', 'request', 'none'],
    ['takeover', 'uart.input.takeover', 'request', 'none'],
    ['hold', 'uart.hold', 'request', 'none'],
    ['unhold', 'uart.unhold', 'request', 'none'],
    ['signal', 'uart.signal', 'request', 'none'],
    ['signalLine', 'uart.signal', 'request', 'card'],
    ['logStart', 'uart.log.start', 'request', 'none'],
    ['logStop', 'uart.log.stop', 'request', 'none'],
  ] as const
).map(([id, rpc, cls, confirm]) => ({ id, rpc, class: cls, right: 'operate' as const, confirm }));

registerPanel(
  {
    id: 'uart.terminal',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'uart',
    title: 'UART 터미널',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'term',
        filter: (p) => ({ domains: ['uart'], channels: [typeof p.channel === 'string' ? p.channel : 'dut'] }),
        snapshot: 'required',
        maxBatchMs: 10,
        mirrorRows: 4_000,
      },
    ],
    queries: [EXPORT_QUERY],
    actions: uartActions,
    memory: { maxBytes: 8 * 1024 * 1024 }, // 2 MiB draw queue + scrollback + mirror
    render: { maxHz: 60, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createUartTerminal,
);
