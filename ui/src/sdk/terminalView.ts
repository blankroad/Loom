// TerminalView — shared terminal component (UART terminal, later SSH/RTT consoles).
// plan §5.1.2, uart-ui §3 · §16.1 (xterm 6.0.0 facts).
//
// Device output is untrusted. Browser-side defenses (the server-side loom-console
// sanitizer is layer ①, owned by UART):
//  ② parser hooks swallow every query that would make xterm answer on its own —
//     DA1/DA2/DA3, DSR/CPR, DECRQM, window ops, XTVERSION, DECRQSS, XTGETTCAP,
//     OSC 4/10/11/12 colour queries ("?"). OSC 8 links, OSC 52 and title changes
//     (OSC 0/1/2) are ignored. The clipboard addon is never loaded.
//  ③ user-intent gate: onData/onBinary are forwarded only within INTENT_WINDOW_MS
//     after a real keydown / paste / composition event in this terminal; anything
//     else (emulator auto-responses, mouse reports nobody asked for) is dropped and
//     counted. readonly (watch) sends nothing at all.
// Input: coalesced for 5 ms; 0x03 (Ctrl-C) flushes immediately. Ctrl-C is always
// 0x03 — copy is Ctrl-Shift-C / Cmd-C. Prefix-key literals arrive as the shell's
// `loom:literal` event.
// Output (C2): ≤ MAX_PENDING_WRITE_BYTES (2 MiB) handed to xterm but not yet parsed,
// per terminal. Beyond that chunks are dropped from the SCREEN (the journal keeps
// them) and, once xterm catches up, a resync line is written: CAN (0x18) aborts any
// half-parsed escape sequence, SGR reset, then a reverse-video "화면 생략" line.

import { Terminal, type IDisposable } from '@xterm/xterm';
import { WebglAddon } from '@xterm/addon-webgl';
import '@xterm/xterm/css/xterm.css';

export const INTENT_WINDOW_MS = 100;
export const INPUT_COALESCE_MS = 5;
export const MAX_PENDING_WRITE_BYTES = 2 * 1024 * 1024;
export const MAX_PENDING_INPUT_BYTES = 64 * 1024;
export const LARGE_PASTE_BYTES = 4 * 1024;

export interface TerminalStats {
  /** queries swallowed by parser hooks (layer ②) */
  blockedQueries: number;
  /** onData/onBinary dropped by the intent gate (layer ③) */
  blockedAutoInput: number;
  /** keys dropped because the view is readonly */
  readonlyDrops: number;
  /** output bytes not drawn (pending cap exceeded) */
  droppedBytes: number;
  /** input bytes dropped because the input queue was full */
  droppedInputBytes: number;
  pendingWriteBytes: number;
  renderer: 'webgl' | 'dom' | 'none';
}

export interface Timers {
  setTimeout(cb: () => void, ms: number): unknown;
  clearTimeout(h: unknown): void;
}

export interface TerminalViewOptions {
  /** bytes to send to the device (already gated) */
  onInput(bytes: Uint8Array): void;
  /** paste larger than LARGE_PASTE_BYTES: owner confirms then sends (uart-ui §3.5) */
  onLargePaste?(text: string): void;
  readonly?: boolean;
  scrollback?: number;
  /** 'dom' skips the WebGL addon */
  renderer?: 'webgl' | 'dom';
  now?: () => number;
  timers?: Timers;
  /** test seam */
  createTerminal?: (o: ConstructorParameters<typeof Terminal>[0]) => Terminal;
}

const enc = new TextEncoder();

export class TerminalView {
  readonly term: Terminal;
  private readonly now: () => number;
  private readonly timers: Timers;
  private readonly disposables: IDisposable[] = [];
  private host: HTMLElement | null = null;
  private lastIntent = -Infinity;
  private readonly_: boolean;
  private paused = false;
  private pausedChunks: Uint8Array[] = [];
  private pausedBytes = 0;
  private inQueue: Uint8Array[] = [];
  private inBytes = 0;
  private inTimer: unknown = null;
  private pendingWrite = 0;
  private dropping = false;
  private dropBytesRun = 0;
  private webgl: WebglAddon | null = null;
  private _stats: TerminalStats = {
    blockedQueries: 0,
    blockedAutoInput: 0,
    readonlyDrops: 0,
    droppedBytes: 0,
    droppedInputBytes: 0,
    pendingWriteBytes: 0,
    renderer: 'none',
  };

  constructor(private readonly o: TerminalViewOptions) {
    this.now = o.now ?? (() => performance.now());
    this.timers = o.timers ?? {
      setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
      clearTimeout: (h) => globalThis.clearTimeout(h as ReturnType<typeof setTimeout>),
    };
    this.readonly_ = o.readonly ?? false;
    const opts = {
      scrollback: Math.min(50_000, o.scrollback ?? 10_000),
      allowProposedApi: true, // registerDecoration (gap marks) — version pinned, see §16.1
      disableStdin: this.readonly_,
      convertEol: false,
      windowOptions: {}, // all window reports off (xterm default, stated explicitly)
      cursorBlink: false,
      fontFamily: 'ui-monospace, Menlo, Consolas, monospace',
      fontSize: 13,
    };
    this.term = o.createTerminal ? o.createTerminal(opts) : new Terminal(opts);
    this.installParserHooks();
    this.disposables.push(this.term.onData((s) => this.fromTerminal(enc.encode(s))));
    this.disposables.push(this.term.onBinary((s) => this.fromTerminal(latin1(s))));
    this.term.attachCustomKeyEventHandler(this.onKeyEvent);
  }

  get stats(): Readonly<TerminalStats> {
    return { ...this._stats, pendingWriteBytes: this.pendingWrite };
  }
  get readonly(): boolean {
    return this.readonly_;
  }

  /** Mount into the DOM. WebGL first; on failure or context loss fall back to DOM. */
  open(el: HTMLElement): void {
    this.host = el;
    el.dataset.terminal = 'true'; // StopPanel won't steal focus from here
    el.addEventListener('paste', this.onPaste, true);
    for (const t of ['compositionstart', 'compositionupdate', 'compositionend']) el.addEventListener(t, this.noteUserIntent, true);
    el.addEventListener('loom:literal', this.onLiteral as EventListener);
    try {
      this.term.open(el);
    } catch (e) {
      // never fail silently: the panel keeps working (status line, palette) and says why
      const msg = document.createElement('div');
      msg.className = 'term-error';
      msg.textContent = `터미널을 열 수 없음: ${e instanceof Error ? e.message : String(e)}`;
      el.append(msg);
      return;
    }
    this._stats.renderer = 'dom';
    if (this.o.renderer !== 'dom') {
      try {
        const addon = new WebglAddon();
        addon.onContextLoss(() => {
          addon.dispose();
          this.webgl = null;
          this._stats.renderer = 'dom';
        });
        this.term.loadAddon(addon);
        this.webgl = addon;
        this._stats.renderer = 'webgl';
      } catch {
        this.webgl = null; // DOM renderer stays
      }
    }
  }

  /** Called on real user input events (keydown/paste/composition). */
  noteUserIntent = (): void => {
    this.lastIntent = this.now();
  };

  setReadonly(ro: boolean): void {
    this.readonly_ = ro;
    this.term.options.disableStdin = ro;
    if (ro) this.dropInputQueue();
  }

  /** Pause drawing (collection continues, bounded). Keys are not sent except 0x03. */
  setPaused(p: boolean): void {
    if (this.paused === p) return;
    this.paused = p;
    if (!p) {
      const chunks = this.pausedChunks;
      this.pausedChunks = [];
      this.pausedBytes = 0;
      for (const c of chunks) this.write(c);
    }
  }
  get isPaused(): boolean {
    return this.paused;
  }

  /** Device output. Never blocks; drops from the screen beyond the cap. */
  write(bytes: Uint8Array): void {
    if (bytes.length === 0) return;
    if (this.paused) {
      if (this.pausedBytes + bytes.length > MAX_PENDING_WRITE_BYTES) return this.dropScreen(bytes.length);
      this.pausedChunks.push(bytes);
      this.pausedBytes += bytes.length;
      return;
    }
    if (this.pendingWrite + bytes.length > MAX_PENDING_WRITE_BYTES) return this.dropScreen(bytes.length);
    if (this.dropping) return this.dropScreen(bytes.length); // keep dropping until drained
    this.pendingWrite += bytes.length;
    this.term.write(bytes, () => this.onWritten(bytes.length));
  }

  /** Mark a server-reported loss (display/record/source gap) without touching the byte stream. */
  markGap(text: string): void {
    this.decorate(text, 'term-gap');
  }

  /** "── 백로그 / 라이브 ──" divider after the snapshot. */
  markSeparator(text: string): void {
    this.decorate(text, 'term-sep');
  }

  /** Send one byte now (prefix literal, palette keys). Not subject to the intent gate. */
  sendBytes(bytes: Uint8Array): void {
    if (this.readonly_) {
      this._stats.readonlyDrops += bytes.length;
      return;
    }
    this.enqueueInput(bytes, true);
  }

  focus(): void {
    this.term.focus();
  }

  fit(cols: number, rows: number): void {
    if (cols > 1 && rows > 1) this.term.resize(cols, rows);
  }

  dispose(): void {
    if (this.inTimer !== null) this.timers.clearTimeout(this.inTimer);
    this.inQueue = [];
    if (this.host) {
      this.host.removeEventListener('paste', this.onPaste, true);
      for (const t of ['compositionstart', 'compositionupdate', 'compositionend'])
        this.host.removeEventListener(t, this.noteUserIntent, true);
      this.host.removeEventListener('loom:literal', this.onLiteral as EventListener);
    }
    this.disposables.forEach((d) => d.dispose());
    this.webgl?.dispose();
    this.term.dispose();
  }

  // ---- internals ----

  private installParserHooks(): void {
    const p = this.term.parser;
    const swallow = () => {
      this._stats.blockedQueries++;
      return true;
    };
    const csi: { prefix?: string; intermediates?: string; final: string }[] = [
      { final: 'c' }, // DA1
      { prefix: '>', final: 'c' }, // DA2
      { prefix: '=', final: 'c' }, // DA3
      { final: 'n' }, // DSR / CPR
      { prefix: '?', final: 'n' }, // DEC DSR
      { intermediates: '$', final: 'p' }, // DECRQM (ANSI)
      { prefix: '?', intermediates: '$', final: 'p' }, // DECRQM (DEC)
      { final: 't' }, // window ops / reports
      { prefix: '>', final: 'q' }, // XTVERSION
    ];
    for (const id of csi) this.disposables.push(p.registerCsiHandler(id, swallow));
    for (const id of [
      { intermediates: '$', final: 'q' }, // DECRQSS
      { intermediates: '+', final: 'q' }, // XTGETTCAP
    ]) {
      this.disposables.push(p.registerDcsHandler(id, swallow));
    }
    // colour queries answer with a report; colour SETs are harmless and allowed
    for (const n of [4, 10, 11, 12]) {
      this.disposables.push(p.registerOscHandler(n, (data) => (data.includes('?') ? swallow() : false)));
    }
    // links, clipboard, titles: ignored
    for (const n of [0, 1, 2, 8, 52]) this.disposables.push(p.registerOscHandler(n, swallow));
  }

  private onKeyEvent = (e: KeyboardEvent): boolean => {
    if (e.type !== 'keydown') return true;
    const k = e.key.toLowerCase();
    // copy: Ctrl-Shift-C / Cmd-C — never 0x03
    if ((e.ctrlKey && e.shiftKey && k === 'c') || (e.metaKey && k === 'c')) {
      const sel = this.term.getSelection();
      if (sel) void navigator.clipboard?.writeText(sel).catch(() => {});
      return false;
    }
    this.noteUserIntent();
    return true; // xterm turns it into onData (Ctrl-C → 0x03)
  };

  private onPaste = (e: ClipboardEvent): void => {
    const text = e.clipboardData?.getData('text') ?? '';
    if (text.length > LARGE_PASTE_BYTES && this.o.onLargePaste) {
      e.preventDefault();
      e.stopPropagation();
      this.o.onLargePaste(text);
      return;
    }
    this.noteUserIntent();
  };

  private onLiteral = (e: CustomEvent<{ byte?: number }>): void => {
    const b = e.detail?.byte;
    if (typeof b === 'number' && b >= 0 && b <= 0xff) this.sendBytes(new Uint8Array([b]));
  };

  private fromTerminal(bytes: Uint8Array): void {
    if (this.readonly_) {
      this._stats.readonlyDrops += bytes.length;
      return;
    }
    if (this.now() - this.lastIntent > INTENT_WINDOW_MS) {
      this._stats.blockedAutoInput++;
      return;
    }
    if (this.paused && !bytes.includes(0x03)) return; // PAUSED: keys not sent, Ctrl-C passes
    this.enqueueInput(bytes, false);
  }

  private enqueueInput(bytes: Uint8Array, immediate: boolean): void {
    if (this.inBytes + bytes.length > MAX_PENDING_INPUT_BYTES) {
      this._stats.droppedInputBytes += bytes.length;
      return;
    }
    this.inQueue.push(bytes);
    this.inBytes += bytes.length;
    if (immediate || bytes.includes(0x03)) return this.flushInput();
    if (this.inTimer === null) this.inTimer = this.timers.setTimeout(() => this.flushInput(), INPUT_COALESCE_MS);
  }

  private flushInput(): void {
    if (this.inTimer !== null) {
      this.timers.clearTimeout(this.inTimer);
      this.inTimer = null;
    }
    if (this.inBytes === 0) return;
    const out = new Uint8Array(this.inBytes);
    let off = 0;
    for (const c of this.inQueue) {
      out.set(c, off);
      off += c.length;
    }
    this.inQueue = [];
    this.inBytes = 0;
    this.o.onInput(out);
  }

  private dropInputQueue(): void {
    if (this.inTimer !== null) this.timers.clearTimeout(this.inTimer);
    this.inTimer = null;
    this.inQueue = [];
    this.inBytes = 0;
  }

  private dropScreen(n: number): void {
    this.dropping = true;
    this.dropBytesRun += n;
    this._stats.droppedBytes += n;
    if (this.pendingWrite === 0) this.resyncAfterDrop();
  }

  private onWritten(n: number): void {
    this.pendingWrite -= n;
    if (this.pendingWrite === 0 && this.dropping) this.resyncAfterDrop();
  }

  private resyncAfterDrop(): void {
    const n = this.dropBytesRun;
    this.dropping = false;
    this.dropBytesRun = 0;
    const line = `\x18\x1b[0m\r\n\x1b[7m ⋯ 화면 생략 ${n.toLocaleString('ko-KR')} B — 기록은 데몬 저널 \x1b[0m\r\n`;
    const b = enc.encode(line);
    this.pendingWrite += b.length;
    this.term.write(b, () => this.onWritten(b.length));
  }

  private decorate(text: string, cls: string): void {
    try {
      const marker = this.term.registerMarker(0);
      const deco = marker ? this.term.registerDecoration({ marker, width: this.term.cols, layer: 'top' }) : undefined;
      if (!deco) throw new Error('no decoration');
      deco.onRender((el) => {
        el.className = `${el.className.split(' term-')[0]} ${cls}`;
        if (el.textContent !== text) el.textContent = text;
      });
      this.lineMarks++;
    } catch {
      // proposed API unavailable → write a line at a parser-safe point (CAN + SGR reset)
      const b = enc.encode(`\x18\x1b[0m\r\n\x1b[7m ${text} \x1b[0m\r\n`);
      this.pendingWrite += b.length;
      this.term.write(b, () => this.onWritten(b.length));
      this.lineMarks++;
    }
  }

  /** gap/separator marks placed (decoration or fallback line) — tests & stats */
  lineMarks = 0;
}

function latin1(s: string): Uint8Array {
  const b = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) b[i] = s.charCodeAt(i) & 0xff;
  return b;
}

export function bytesToHex(b: Uint8Array): string {
  let s = '';
  for (const x of b) s += x.toString(16).padStart(2, '0');
  return s;
}

/** payload.bytes {encoding:'hex'|'base64'} → bytes (raw_ref → null: fetch not done here). */
export function decodePayloadBytes(v: unknown): Uint8Array | null {
  if (typeof v !== 'object' || v === null) return null;
  const { encoding, data } = v as { encoding?: unknown; data?: unknown };
  if (typeof data !== 'string') return null;
  if (encoding === 'hex') {
    if (data.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(data)) return null;
    const out = new Uint8Array(data.length / 2);
    for (let i = 0; i < out.length; i++) out[i] = parseInt(data.slice(i * 2, i * 2 + 2), 16);
    return out;
  }
  if (encoding === 'base64') {
    try {
      const bin = atob(data);
      return latin1(bin);
    } catch {
      return null;
    }
  }
  return null;
}
