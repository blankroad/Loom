// UART Bench workbench state — the agreed REV6.1 web workbench (work/reports/uart-workbench/opus) wired to
// the real daemon. Local, user-editable preferences (saved sessions, macros, groups, filter, side panel, per-tab
// EOL/timestamp/theme) live in localStorage; everything about devices comes from the engine:
//   saved session ▶ open  → uart.open {target, alias, config{baud,data_bits,parity,stop_bits,flow}}
//   port dropdown         → uart.ports
//   close (⏏)             → uart.close {channel}
//   keys / macro send     → the terminal panel's own uart.input path (same as typing)
//   macro wait            → uart.expect {from:"now"} registered before the preceding send
// Nothing here fabricates devices, ports or RX. Macros observe STOP (latch), write lock, rights and port state
// before every step; recipients are resolved once at launch; no retries / retransmits.

import type { ActionSpec } from '../../sdk/manifest';
import { unwrapDomainResult } from '../../sdk/rpcEnvelope';
import { Emitter } from '../../store/emitter';
import type { ShellServices } from '../../shell/services';

export type Parity = 'N' | 'E' | 'O';
export type Flow = 'none' | 'RTS/CTS' | 'XON/XOFF';
export type EolKey = 'CR' | 'LF' | 'CRLF' | 'none';
export const EOLS: Record<EolKey, string> = { CR: '\r', LF: '\n', CRLF: '\r\n', none: '' };
export const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1000000];
export const COLORS = ['#0969da', '#bf8700', '#8250df', '#1b7c83', '#cf222e', '#1a7f37', '#57606a'];

export interface Session {
  id: string;
  alias: string;
  group: string;
  port: string;
  baud: number;
  dataBits: 5 | 6 | 7 | 8;
  parity: Parity;
  stopBits: 1 | 2;
  flow: Flow;
  eol: EolKey;
  ts: boolean;
  color: string;
}
export type MacroStep = { type: 'send'; text: string; eol: 'session' | EolKey } | { type: 'delay'; ms: number } | { type: 'wait'; text: string; ms: number };
export interface Macro {
  id: string;
  name: string;
  hotkey: string;
  pinned: boolean;
  vars: { name: string; def: string }[];
  steps: MacroStep[];
  /** fixed recipients (session ids); empty = the panel scope */
  fixed: string[];
}
export type ScopeMode = 'active' | 'all' | 'selected';
export const MS_LABEL: Record<ScopeMode, string> = { active: '활성', all: '연결된 모두', selected: '선택' };

/** What a mounted uart.terminal panel offers the workbench (registered per channel). */
/** Terminal views by channel. Each workspace (integrated, UART module) is a retained dock, so one channel can have
 * more than one live terminal view; every lookup resolves to ONE preferred view (active, else visible, else newest),
 * so macros/broadcast address a channel once. */
export class TerminalRegistry {
  private by = new Map<string, TerminalHandle[]>();
  add(h: TerminalHandle): void {
    const list = this.by.get(h.channel) ?? [];
    if (!list.includes(h)) list.push(h);
    this.by.set(h.channel, list);
  }
  /** true when that was the channel's last view */
  remove(h: TerminalHandle): boolean {
    const list = (this.by.get(h.channel) ?? []).filter((x) => x !== h);
    if (list.length) { this.by.set(h.channel, list); return false; }
    this.by.delete(h.channel);
    return true;
  }
  get(channel: string): TerminalHandle | undefined {
    const list = this.by.get(channel);
    if (!list?.length) return undefined;
    return list.find((t) => t.isActive()) ?? [...list].reverse().find((t) => t.visible()) ?? list[list.length - 1];
  }
  /** one preferred view per channel */
  values(): TerminalHandle[] {
    return [...this.by.keys()].flatMap((ch) => { const t = this.get(ch); return t ? [t] : []; });
  }
  /** every view of a channel (redraw all of them) */
  all(channel: string): readonly TerminalHandle[] { return this.by.get(channel) ?? []; }
}

export interface TerminalHandle {
  channel: string;
  /** null = writable; otherwise the reason (보기 전용, 닫힘, 쓰기 잠김 …) */
  blocked(): string | null;
  /** send through the panel's uart.input path (same path as typing); returns false if refused */
  sendRaw(bytes: Uint8Array): boolean;
  path(): string | null;
  frame(): string;
  status(): string;
  counters(): { rx: number; tx: number };
  focus(): void;
  activate(): void;
  /** the dock group (pane) this terminal sits in, for `pane N` labels */
  paneId(): string | null;
  isActive(): boolean;
  /** shown (its dock tab is the visible one) */
  visible(): boolean;
  /** the panel re-renders its header/run strip */
  invalidate(): void;
}

export interface RunStep { st: 'pending' | 'run' | 'ok' | 'fail' | 'skip'; note?: string; text: string }
export interface MacroRun {
  id: string;
  gid: string;
  macroId: string;
  name: string;
  channel: string;
  status: 'running' | 'done' | 'failed' | 'stopped';
  i: number;
  steps: RunStep[];
  err?: string;
  reason?: string;
  abort: () => void;
}

interface Persisted {
  sessions: Session[];
  macros: Macro[];
  ui: { side: 'sessions' | 'macros' | null; closedGroups: string[]; filter: string };
  tab: Record<string, { eol?: EolKey; ts?: boolean; theme?: 'dark' | 'light' }>;
  varVals: Record<string, Record<string, string>>;
  recPrefs: Record<string, { format: 'log' | 'jsonl'; rx: boolean; tx: boolean; fileTs: boolean }>;
}

const LS_KEY = 'loom.uartBench.v1';
const ALIAS_RE = /^[a-z][a-z0-9_-]{0,31}$/;
let seq = 0;
export const nid = (p: string): string => p + Date.now().toString(36).slice(-4) + (++seq).toString(36);
export const frameOf = (s: Pick<Session, 'baud' | 'dataBits' | 'parity' | 'stopBits'>): string => `${s.baud} ${s.dataBits}${s.parity}${s.stopBits}`;
export const subst = (text: string, vars: Record<string, string>): string => text.replace(/\$\{(\w+)\}/g, (_, n: string) => vars[n] ?? '');
export function stepText(st: MacroStep, vars?: Record<string, string>): string {
  const v = (x: string) => (vars ? subst(x, vars) : x);
  if (st.type === 'send') return `send ${st.text === '' ? '⏎' : JSON.stringify(v(st.text))}${st.eol && st.eol !== 'session' ? ` +${st.eol}` : ''}`;
  if (st.type === 'delay') return `delay ${st.ms}ms`;
  return `wait ${JSON.stringify(v(st.text))} ≤${st.ms}ms`;
}

const enc = new TextEncoder();
const toHex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
const OPEN_SPEC: ActionSpec = { id: 'uart.bench.open', rpc: 'uart.open', class: 'request', right: 'operate', confirm: 'none', label: 'UART 세션 열기' };
const CLOSE_SPEC: ActionSpec = { id: 'uart.bench.close', rpc: 'uart.close', class: 'request', right: 'operate', confirm: 'none', label: 'UART 포트 닫기' };

function sanitizeSession(x: Partial<Session> & Record<string, unknown>): Session | null {
  if (typeof x.alias !== 'string' || !ALIAS_RE.test(x.alias) || typeof x.port !== 'string' || !x.port.trim()) return null;
  const parity = x.parity === 'E' || x.parity === 'O' ? x.parity : 'N';
  const eol = x.eol === 'LF' || x.eol === 'CRLF' || x.eol === 'none' ? x.eol : 'CR';
  const flow = x.flow === 'RTS/CTS' || x.flow === 'XON/XOFF' ? x.flow : 'none';
  const baud = Number(x.baud);
  const db = Number(x.dataBits);
  return {
    id: typeof x.id === 'string' && x.id ? x.id : nid('s-'),
    alias: x.alias,
    group: typeof x.group === 'string' ? x.group : '',
    port: x.port.trim(),
    baud: Number.isSafeInteger(baud) && baud > 0 && baud <= 4_000_000 ? baud : 115200,
    dataBits: (db === 5 || db === 6 || db === 7 ? db : 8) as Session['dataBits'],
    parity,
    stopBits: Number(x.stopBits) === 2 ? 2 : 1,
    flow,
    eol,
    ts: x.ts !== false,
    color: typeof x.color === 'string' && /^#[0-9a-f]{6}$/i.test(x.color) ? x.color : COLORS[0]!,
  };
}
function sanitizeMacro(x: Record<string, unknown>): Macro | null {
  if (typeof x.name !== 'string' || !Array.isArray(x.steps)) return null;
  const steps: MacroStep[] = [];
  for (const s of x.steps as Record<string, unknown>[]) {
    if (s?.type === 'send') steps.push({ type: 'send', text: String(s.text ?? ''), eol: (['CR', 'LF', 'CRLF', 'none'].includes(String(s.eol)) ? s.eol : 'session') as 'session' | EolKey });
    else if (s?.type === 'delay') steps.push({ type: 'delay', ms: Math.max(0, Math.min(600_000, Number(s.ms) || 0)) });
    else if (s?.type === 'wait') steps.push({ type: 'wait', text: String(s.text ?? ''), ms: Math.max(1, Math.min(600_000, Number(s.ms) || 1000)) });
  }
  const fixed = Array.isArray(x.fixed) ? (x.fixed as unknown[]).filter((v): v is string => typeof v === 'string') : typeof x.target === 'string' && x.target !== 'active' ? [x.target] : [];
  return {
    id: typeof x.id === 'string' && x.id ? x.id : nid('m-'),
    name: x.name.slice(0, 80),
    hotkey: typeof x.hotkey === 'string' ? x.hotkey : '',
    pinned: x.pinned === true,
    vars: Array.isArray(x.vars) ? (x.vars as Record<string, unknown>[]).filter((v) => typeof v?.name === 'string').map((v) => ({ name: String(v.name), def: String(v.def ?? '') })) : [],
    steps,
    fixed,
  };
}

export class UartBench extends Emitter {
  sessions: Session[] = [];
  macros: Macro[] = [];
  ui: Persisted['ui'] = { side: 'sessions', closedGroups: [], filter: '' };
  tab: Persisted['tab'] = {};
  varVals: Persisted['varVals'] = {};
  recPrefs: Persisted['recPrefs'] = {};
  /** macro execution scope — not persisted (reload starts in 활성), as REV6 */
  ms: { mode: ScopeMode; sel: string[] } = { mode: 'active', sel: [] };
  /** keyboard broadcast (키 입력 대상) — not persisted; recipients frozen when switched on */
  bc: { on: boolean; ids: string[]; excluded: { alias: string; why: string }[] } = { on: false, ids: [], excluded: [] };
  runs = new Map<string, MacroRun>(); // channel → latest run
  private terminals = new TerminalRegistry();
  private lastActive: string | null = null;
  services: ShellServices | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly storage: Storage | null = typeof localStorage === 'undefined' ? null : localStorage) {
    super();
    this.load();
  }

  // ---------- persistence ----------
  private load(): void {
    try {
      const raw = this.storage?.getItem(LS_KEY);
      if (!raw) return;
      const p = JSON.parse(raw) as Partial<Persisted>;
      this.sessions = (p.sessions ?? []).map((s) => sanitizeSession(s as Session & Record<string, unknown>)).filter((s): s is Session => s !== null);
      this.macros = (p.macros ?? []).map((m) => sanitizeMacro(m as unknown as Record<string, unknown>)).filter((m): m is Macro => m !== null);
      if (p.ui) this.ui = { side: p.ui.side === 'macros' || p.ui.side === null ? p.ui.side : 'sessions', closedGroups: Array.isArray(p.ui.closedGroups) ? p.ui.closedGroups : [], filter: typeof p.ui.filter === 'string' ? p.ui.filter : '' };
      this.tab = p.tab ?? {};
      this.varVals = p.varVals ?? {};
      this.recPrefs = p.recPrefs ?? {};
    } catch {
      /* storage blocked or corrupt: start empty */
    }
  }
  private persist(): void {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      try {
        const p: Persisted = { sessions: this.sessions, macros: this.macros, ui: this.ui, tab: this.tab, varVals: this.varVals, recPrefs: this.recPrefs };
        this.storage?.setItem(LS_KEY, JSON.stringify(p));
      } catch {
        /* quota / blocked: preferences stay in memory */
      }
    }, 120);
  }
  bump(save = false): void {
    if (save) this.persist();
    this.changed();
  }

  attach(services: ShellServices): () => void {
    this.services = services;
    let wasLatched = services.shell.latch.on;
    const off = services.shell.subscribe(() => {
      const latched = services.shell.latch.on;
      if (latched && !wasLatched) this.stopAll('STOP으로 중단');
      wasLatched = latched;
      if (this.bc.on) this.pruneBroadcast();
    });
    // STOP is safety-direction: the click itself (optimistic 'requesting') already stops running macros
    const offStop = services.stop.subscribe(() => {
      if (services.stop.phase !== 'idle') this.stopAll('STOP으로 중단');
    });
    return () => {
      off();
      offStop();
      if (this.services === services) this.services = null;
    };
  }

  // ---------- terminals ----------
  registerTerminal(h: TerminalHandle): () => void {
    this.terminals.add(h);
    this.changed();
    return () => {
      // another workspace (integrated / UART module dock) may still show this channel: only the LAST view closing
      // ends a running macro or drops it from the broadcast recipients
      if (this.terminals.remove(h)) {
        const run = this.runs.get(h.channel);
        if (run?.status === 'running') this.stopRun(h.channel, '보기 닫힘으로 중단');
        if (this.bc.on) this.pruneBroadcast();
      }
      this.changed();
    };
  }
  terminal(channel: string): TerminalHandle | undefined {
    return this.terminals.get(channel);
  }
  get openTerminals(): TerminalHandle[] {
    return this.terminals.values().sort((a, b) => a.channel.localeCompare(b.channel));
  }
  noteActive(channel: string): void {
    if (this.lastActive === channel) return;
    this.lastActive = channel;
    this.changed();
  }
  activeTerminal(): TerminalHandle | null {
    const live = this.openTerminals.find((t) => t.isActive());
    if (live) return live;
    const last = this.lastActive ? this.terminals.get(this.lastActive) : undefined;
    if (last?.visible()) return last;
    return this.openTerminals.find((t) => t.visible()) ?? last ?? null;
  }
  sessionFor(channel: string): Session | undefined {
    return this.sessions.find((s) => s.alias === channel);
  }
  tabPrefs(channel: string): { eol: EolKey; ts: boolean; theme: 'dark' | 'light' } {
    const s = this.sessionFor(channel);
    const t = this.tab[channel] ?? {};
    return { eol: t.eol ?? s?.eol ?? 'CR', ts: t.ts ?? s?.ts ?? true, theme: t.theme ?? 'dark' };
  }
  setTabPref(channel: string, patch: Persisted['tab'][string]): void {
    this.tab[channel] = { ...this.tab[channel], ...patch };
    this.bump(true);
    for (const t of this.terminals.all(channel)) t.invalidate();
  }

  // ---------- engine ----------
  private need(): ShellServices {
    if (!this.services) throw new Error('작업대가 데몬 연결에 아직 붙지 않았습니다');
    return this.services;
  }
  channelState(alias: string): 'open' | 'closed' | 'unknown' | 'none' {
    return this.services?.shell.channels.get(`uart/${alias}`)?.state ?? 'none';
  }
  async listPorts(): Promise<{ path: string; vid?: number | null; pid?: number | null; manufacturer?: string | null; product?: string | null; serial?: string | null }[]> {
    const s = this.need();
    const r = unwrapDomainResult<{ ports?: unknown }>('uart.ports', await s.net.call('uart.ports', {}));
    const ports = (r && Array.isArray(r.ports) ? r.ports : []) as Record<string, unknown>[];
    return ports.filter((p) => typeof p.path === 'string').slice(0, 256).map((p) => ({
      path: p.path as string,
      vid: typeof p.vid === 'number' ? p.vid : null,
      pid: typeof p.pid === 'number' ? p.pid : null,
      manufacturer: typeof p.manufacturer === 'string' ? p.manufacturer : null,
      product: typeof p.product === 'string' ? p.product : null,
      serial: typeof p.serial === 'string' ? p.serial : null,
    }));
  }
  /** Open a saved session through the real engine; the dock shows its terminal when the channel opens. */
  async openSession(s: Session): Promise<void> {
    const sv = this.need();
    if (this.channelState(s.alias) === 'open') return;
    const used = this.terminals.values().find((t) => t.path() === s.port && t.channel !== s.alias && t.status().toLowerCase() === 'open');
    if (used) throw new Error(`${s.port}은(는) 이미 ${used.channel}이(가) 열고 있습니다 — 포트는 한 탭만 점유`);
    const config = { baud: s.baud, data_bits: s.dataBits, parity: { N: 'none', E: 'even', O: 'odd' }[s.parity], stop_bits: String(s.stopBits), flow: { none: 'none', 'RTS/CTS': 'hardware', 'XON/XOFF': 'software' }[s.flow] };
    await sv.gate.run(OPEN_SPEC, { target: s.port, alias: s.alias, config });
  }
  async closeChannel(channel: string): Promise<void> {
    const sv = this.need();
    const run = this.runs.get(channel);
    if (run?.status === 'running') this.stopRun(channel, '포트 닫힘으로 중단');
    await sv.gate.run(CLOSE_SPEC, { channel });
  }

  // ---------- keyboard broadcast (키 입력 대상) ----------
  bcWhy(t: TerminalHandle | undefined): string {
    if (!t) return '닫힘';
    return t.blocked() ?? '';
  }
  setBroadcast(on: boolean): void {
    if (on) {
      this.bc = { on: true, ids: [], excluded: [] };
      for (const t of this.openTerminals) {
        const why = this.bcWhy(t);
        if (why) this.bc.excluded.push({ alias: t.channel, why });
        else this.bc.ids.push(t.channel);
      }
      if (!this.bc.ids.length) {
        this.bc = { on: false, ids: [], excluded: [] };
        this.services?.toasts.push('연결된 쓰기 가능 세션이 없어 현재 세션 입력을 유지합니다');
      }
    } else this.bc = { on: false, ids: [], excluded: [] };
    for (const t of this.terminals.values()) t.invalidate();
    this.changed();
  }
  private pruneBroadcast(): void {
    const before = this.bc.ids.length;
    this.bc.ids = this.bc.ids.filter((id) => {
      const why = this.bcWhy(this.terminals.get(id));
      if (why) this.bc.excluded.push({ alias: id, why });
      return !why;
    });
    if (this.bc.on && !this.bc.ids.length) {
      this.bc = { on: false, ids: [], excluded: [] };
      this.services?.toasts.push('연결된 모두: 남은 수신자가 없어 현재 세션으로 돌아갑니다');
    }
    if (before !== this.bc.ids.length) this.changed();
  }
  /** Key bytes typed in `from`. Returns true when the broadcast handled them (else the panel sends normally). */
  routeKeys(from: string, bytes: Uint8Array, mapEnter: (channel: string, b: Uint8Array) => Uint8Array): boolean {
    if (!this.bc.on) return false;
    if (!this.bc.ids.includes(from)) {
      this.services?.toasts.push(`${from}: 연결된 모두 수신자가 아니라 키 입력을 보내지 않았습니다`, { key: `bc-out-${from}`, ttlMs: 4_000 });
      return true;
    }
    for (const id of this.bc.ids) {
      const t = this.terminals.get(id);
      if (t && !this.bcWhy(t)) t.sendRaw(mapEnter(id, bytes));
    }
    return true;
  }

  // ---------- macro scope ----------
  resolveScope(m: Pick<Macro, 'fixed'>, launchFrom?: string): { ids: string[]; excluded: { alias: string; why: string }[]; unresolved: string[]; label: string } {
    const excluded: { alias: string; why: string }[] = [];
    const unresolved: string[] = [];
    let candidates: string[];
    let prefix: string;
    if (m.fixed.length) {
      prefix = '고정';
      candidates = [];
      for (const sid of m.fixed) {
        const s = this.sessions.find((x) => x.id === sid);
        if (!s) unresolved.push(sid);
        else candidates.push(s.alias);
      }
      if (unresolved.length) return { ids: [], excluded, unresolved, label: `고정 · 실행 차단(미해결) → ${candidates.join(', ') || '없음'} · 제외 미해결 ${unresolved.join(', ')}(저장된 세션 없음)` };
    } else if (this.ms.mode === 'active') {
      prefix = MS_LABEL.active;
      const a = launchFrom ? this.terminals.get(launchFrom) : this.activeTerminal();
      candidates = a ? [a.channel] : [];
    } else if (this.ms.mode === 'all') {
      prefix = MS_LABEL.all;
      candidates = this.openTerminals.map((t) => t.channel);
    } else {
      prefix = MS_LABEL.selected;
      candidates = [...this.ms.sel];
    }
    const ids: string[] = [];
    for (const c of candidates) {
      const t = this.terminals.get(c);
      const why = !t ? '열려 있지 않음' : this.runs.get(c)?.status === 'running' ? '실행 중' : this.bcWhy(t);
      if (why) excluded.push({ alias: c, why });
      else ids.push(c);
    }
    const label = `${prefix} → ${ids.length ? ids.join(', ') : '없음'}${ids.length > 1 ? ` (${ids.length})` : ''}${excluded.length ? ` · 제외 ${excluded.map((e) => `${e.alias}(${e.why})`).join(', ')}` : ''}`;
    return { ids, excluded, unresolved, label };
  }

  // ---------- macro execution ----------
  missingVars(m: Macro): { name: string; def: string }[] {
    const vals = this.varVals[m.id] ?? {};
    return m.vars.filter((v) => (vals[v.name] ?? v.def) === '');
  }
  /** ▶ — immediate run on the resolved recipients (one independent run per recipient). */
  requestRun(m: Macro, launchFrom?: string, varsOverride?: Record<string, string>): { started: string[]; refused: string | null } {
    const sv = this.services;
    if (!sv) return { started: [], refused: '데몬 연결 없음' };
    if (sv.shell.latch.on || sv.stop.phase !== 'idle') return this.refuse('STOP 잠김 — 매크로를 시작하지 않습니다');
    if (sv.shell.status.writeLocked) return this.refuse('쓰기 잠김(STALLED) — 매크로를 시작하지 않습니다');
    const scope = this.resolveScope(m, launchFrom);
    if (scope.unresolved.length) return this.refuse(`실행 차단: ${m.name} 고정 대상 미해결 (${scope.unresolved.join(', ')})`);
    if (!scope.ids.length) return this.refuse(`${m.name}: 보낼 대상 없음${scope.excluded.length ? ` · 제외 ${scope.excluded.map((e) => `${e.alias}(${e.why})`).join(', ')}` : ''}`);
    const vars: Record<string, string> = {};
    const saved = this.varVals[m.id] ?? {};
    for (const v of m.vars) vars[v.name] = varsOverride?.[v.name] ?? saved[v.name] ?? v.def;
    const gid = nid('g');
    for (const ch of scope.ids) this.startRun(m, ch, vars, gid);
    if (scope.excluded.length) sv.toasts.push(`${m.name} → ${scope.ids.join(', ')} · 제외 ${scope.excluded.map((e) => `${e.alias}(${e.why})`).join(', ')}`);
    return { started: scope.ids, refused: null };
  }
  private refuse(text: string): { started: string[]; refused: string } {
    this.services?.toasts.push(text);
    return { started: [], refused: text };
  }
  private startRun(m: Macro, channel: string, vars: Record<string, string>, gid: string): void {
    const ctl = new AbortController();
    const run: MacroRun = {
      id: nid('r'),
      gid,
      macroId: m.id,
      name: m.name,
      channel,
      status: 'running',
      i: 0,
      steps: m.steps.map((st) => ({ st: 'pending', text: stepText(st, vars) })),
      abort: () => ctl.abort(),
    };
    this.runs.set(channel, run);
    this.changed();
    void this.execute(m, run, vars, ctl.signal);
  }
  private blockReason(channel: string): string | null {
    const sv = this.services;
    if (!sv) return '데몬 연결 없음';
    if (sv.shell.latch.on || sv.stop.phase !== 'idle') return 'STOP으로 중단';
    if (sv.shell.status.writeLocked) return '쓰기 잠김(STALLED)';
    const t = this.terminals.get(channel);
    if (!t) return '보기 닫힘으로 중단';
    const why = t.blocked();
    return why ? `${why}(으)로 중단` : null;
  }
  /** generation of the channel's current connection (null = unknown); a run is bound to the one it started on */
  private generationOf(channel: string): number | null {
    return this.services?.shell.channels.get(`uart/${channel}`)?.generation ?? null;
  }
  private async execute(m: Macro, run: MacroRun, vars: Record<string, string>, signal: AbortSignal): Promise<void> {
    const sv = this.services!;
    const eolOf = (st: Extract<MacroStep, { type: 'send' }>) => EOLS[st.eol === 'session' ? this.tabPrefs(run.channel).eol : st.eol];
    // REV6.1 recipient rule: recipients are fixed at launch — a disconnect/re-open of the same alias (new
    // generation) never continues this run.
    const gen0 = this.generationOf(run.channel);
    const halt = (): string | null => {
      if (signal.aborted) return run.reason ?? '중지됨';
      const blocked = this.blockReason(run.channel);
      if (blocked) return blocked;
      const g = this.generationOf(run.channel);
      if (gen0 !== null && g !== gen0) return '다시 연결됨 — 이어서 보내지 않음';
      return null;
    };
    // a pre-registered wait (send→wait): its own abort, always consumed (no unhandled rejection, no leaked waiter)
    let pendingExpect: { text: string; p: Promise<unknown>; ctl: AbortController } | null = null;
    const dropExpect = () => {
      if (!pendingExpect) return;
      pendingExpect.ctl.abort();
      pendingExpect.p.catch(() => undefined);
      pendingExpect = null;
    };
    // pattern kinds are the daemon's `literal | regex | prompt` (loom-uart-worker PatternKind; the CLI's
    // patterns_json sends {kind:"literal"}). A macro wait is exact text → literal.
    const expect = (text: string, ms: number) => {
      const ctl = new AbortController();
      const onAbort = () => ctl.abort();
      signal.addEventListener('abort', onAbort, { once: true });
      const p = sv.net.call('uart.expect', { channel: run.channel, patterns: [{ kind: 'literal', pattern: text }], from: 'now', timeout_ms: ms }, { signal: ctl.signal })
        .finally(() => signal.removeEventListener('abort', onAbort));
      // handled from birth: STOP/abort can reject it before the wait step awaits it (awaiting still throws)
      p.catch(() => undefined);
      return { text, p, ctl };
    };
    const touch = () => {
      this.changed();
      for (const t of this.terminals.all(run.channel)) t.invalidate();
    };
    const stopWith = (reason: string) => {
      run.status = 'stopped';
      run.reason = reason;
    };
    try {
      for (let i = 0; i < m.steps.length; i++) {
        const st = m.steps[i]!;
        run.i = i;
        const h0 = halt();
        if (h0) { stopWith(h0); break; }
        run.steps[i]!.st = 'run';
        touch();
        if (st.type === 'send') {
          // a wait right after this send is registered BEFORE transmitting (uart.expect from:"now"),
          // so a fast response cannot slip between send and wait
          const next = m.steps[i + 1];
          if (next?.type === 'wait') {
            pendingExpect = expect(subst(next.text, vars), next.ms);
            // give the registration a head start on the same connection
            await new Promise((r) => setTimeout(r, 0));
          }
          // re-check right before the bytes leave: STOP, abort, write lock, port state or a new generation may
          // have arrived during the yield above
          const h1 = halt();
          if (h1) { dropExpect(); stopWith(h1); break; }
          const bytes = enc.encode(subst(st.text, vars) + eolOf(st));
          const t = this.terminals.get(run.channel);
          if (!t || !t.sendRaw(bytes)) { dropExpect(); throw new Error(`송신 거부 (${t?.blocked() ?? '보기 없음'})`); }
          run.steps[i]!.st = 'ok';
        } else if (st.type === 'delay') {
          await new Promise<void>((resolve, reject) => {
            const h = setTimeout(resolve, st.ms);
            signal.addEventListener('abort', () => { clearTimeout(h); reject(new DOMException('stopped', 'AbortError')); }, { once: true });
          });
          run.steps[i]!.st = 'ok';
        } else {
          const text = subst(st.text, vars);
          let w = pendingExpect as { text: string; p: Promise<unknown>; ctl: AbortController } | null;
          if (!w || w.text !== text) { dropExpect(); w = expect(text, st.ms); }
          pendingExpect = null;
          const data = unwrapDomainResult<{ matched?: unknown }>('uart.expect', await w.p) ?? {};
          // the worker always reports `matched` (true = hit, false = timeout) — nothing is inferred
          if (data.matched !== true) {
            run.steps[i]!.st = 'fail';
            run.steps[i]!.note = `시간 초과 ${st.ms}ms`;
            run.status = 'failed';
            run.err = `${JSON.stringify(text)} 시간 초과 ${st.ms}ms`;
            break;
          }
          run.steps[i]!.st = 'ok';
        }
        touch();
      }
      if (run.status === 'running') run.status = 'done';
    } catch (e) {
      if (signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) {
        run.status = 'stopped';
        run.reason ??= '중지됨';
      } else {
        const msg = e instanceof Error ? e.message : String(e);
        const timeout = /timeout|E_TIMEOUT|시간/i.test(msg);
        run.status = 'failed';
        run.err = timeout ? '시간 초과' : msg;
        const cur = run.steps[run.i];
        if (cur) { cur.st = 'fail'; cur.note = run.err; }
      }
    } finally {
      dropExpect();
    }
    for (const s of run.steps) if (s.st === 'pending' || s.st === 'run') s.st = run.status === 'done' ? 'ok' : 'skip';
    touch();
  }
  stopRun(channel: string, reason = '중지됨'): void {
    const run = this.runs.get(channel);
    if (!run || run.status !== 'running') return;
    run.reason = reason;
    run.status = 'stopped';
    run.abort();
    this.changed();
    for (const t of this.terminals.all(channel)) t.invalidate();
  }
  stopMacroAll(macroId: string): void {
    for (const r of this.runs.values()) if (r.macroId === macroId && r.status === 'running') this.stopRun(r.channel, '전체 중지');
  }
  stopAll(reason = '전체 중지'): void {
    for (const r of this.runs.values()) if (r.status === 'running') this.stopRun(r.channel, reason);
  }
  dismissRun(channel: string): void {
    const r = this.runs.get(channel);
    if (r && r.status !== 'running') this.runs.delete(channel);
    this.changed();
    for (const t of this.terminals.all(channel)) t.invalidate();
  }
  get runningCount(): number {
    return [...this.runs.values()].filter((r) => r.status === 'running').length;
  }

  // ---------- CRUD ----------
  saveSession(s: Session): string | null {
    const clean = sanitizeSession(s as Session & Record<string, unknown>);
    if (!clean) return '별칭은 영문 소문자로 시작하는 1–32자(영문·숫자·_·-), 포트는 비울 수 없습니다';
    if (this.sessions.some((x) => x.alias === clean.alias && x.id !== clean.id)) return `별칭 ${clean.alias}이(가) 이미 있습니다`;
    if (this.sessions.some((x) => x.port === clean.port && x.id !== clean.id)) return `포트 ${clean.port}은(는) 이미 저장된 세션(${this.sessions.find((x) => x.port === clean.port && x.id !== clean.id)!.alias})이 사용합니다`;
    const i = this.sessions.findIndex((x) => x.id === clean.id);
    if (i >= 0) this.sessions[i] = clean;
    else this.sessions.push(clean);
    this.bump(true);
    return null;
  }
  deleteSession(id: string): void {
    this.sessions = this.sessions.filter((s) => s.id !== id);
    this.bump(true);
  }
  saveMacro(m: Macro): void {
    const clean = sanitizeMacro(m as unknown as Record<string, unknown>);
    if (!clean) return;
    const i = this.macros.findIndex((x) => x.id === clean.id);
    if (i >= 0) this.macros[i] = clean;
    else this.macros.push(clean);
    this.bump(true);
  }
  deleteMacro(id: string): void {
    this.macros = this.macros.filter((m) => m.id !== id);
    this.bump(true);
  }
  exportJson(): string {
    return JSON.stringify({ loomUart: 1, sessions: this.sessions, macros: this.macros }, null, 2);
  }
  /** Accepts this workbench's export and the REV6 prototype export ({sessions, macros}); fixed ids are remapped. */
  importJson(text: string): string {
    const p = JSON.parse(text) as { sessions?: unknown[]; macros?: unknown[] };
    const idMap = new Map<string, string>();
    let ns = 0;
    let nm = 0;
    let skipped = 0;
    for (const raw of p.sessions ?? []) {
      const s = sanitizeSession(raw as Session & Record<string, unknown>);
      if (!s) { skipped++; continue; }
      const existing = this.sessions.find((x) => x.alias === s.alias);
      const srcId = (raw as { id?: string }).id ?? s.id;
      if (existing) { idMap.set(srcId, existing.id); continue; }
      const id = nid('s-');
      idMap.set(srcId, id);
      this.sessions.push({ ...s, id });
      ns++;
    }
    let unresolved = 0;
    for (const raw of p.macros ?? []) {
      const m = sanitizeMacro(raw as Record<string, unknown>);
      if (!m) { skipped++; continue; }
      m.id = nid('m-');
      m.fixed = m.fixed.map((sid) => idMap.get(sid) ?? (this.sessions.some((s) => s.id === sid) ? sid : (unresolved++, sid)));
      this.macros.push(m);
      nm++;
    }
    this.bump(true);
    return `가져오기: 세션 ${ns}개, 매크로 ${nm}개${skipped ? ` · 건너뜀 ${skipped}개` : ''}${unresolved ? ` · 고정 대상 ${unresolved}개 미해결 — 해당 매크로는 실행되지 않음` : ''}`;
  }

  /** used by the panel when the user types: Enter (CR from xterm) → this tab's EOL; other bytes untouched */
  mapEnter(channel: string, b: Uint8Array): Uint8Array {
    const eol = EOLS[this.tabPrefs(channel).eol];
    if (eol === '\r' || !b.includes(0x0d)) return b;
    const e = enc.encode(eol);
    const out: number[] = [];
    for (const x of b) if (x === 0x0d) out.push(...e); else out.push(x);
    return new Uint8Array(out);
  }
  static hex = toHex;
}

/** one workbench per page (shared by the shell chrome and every uart.terminal panel) */
export const uartBench = new UartBench();
