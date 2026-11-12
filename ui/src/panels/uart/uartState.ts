// UART channel state for the terminal panel, read from uart.* events
// (uart-ui §2.1). The SERVER InputArbiter is the source of truth — the panel only
// displays owner / dirty / hold / lock / agent waits (uart-ui `## ui 의견`).
//
// Provisional payloads (p0-common + uart-ui; confirm with UART):
//   uart.input_owner {owner:'none'|'user'|'agent', viewer?, dirty, hold}
//   uart.hold        {on}
//   uart.lock        {on, reason?, ttl_ms?, actor?}
//   uart.agent_activity {kind:'deferred_send'|'expect'|'idle', text?, elapsed_ms?, timeout_ms?}
//   uart.state / uart.config {status?, baud?, ...}  (generation from the record)

import type { LoomEvent } from '../../sdk';

export interface Waiting {
  text: string;
  elapsedMs: number;
  timeoutMs: number | null;
  /** client clock when received — elapsed keeps counting locally */
  atMs: number;
}

export interface UartInputState {
  owner: 'none' | 'user' | 'agent';
  viewer: string | null;
  dirty: boolean;
  hold: boolean;
  lock: { reason: string; ttlMs: number | null; actor: string | null; atMs: number } | null;
  deferred: Waiting | null;
  expect: Waiting | null;
}

export interface UartPortState {
  status: string;
  baud: number | null;
  generation: number | null;
  /** port target as reported by uart.state/uart.config (e.g. /dev/ttyUSB0, sim://bootshell) */
  path?: string;
  dataBits?: number;
  parity?: string;
  stopBits?: string;
  flow?: string;
}

export const INITIAL_INPUT: UartInputState = {
  owner: 'none',
  viewer: null,
  dirty: false,
  hold: false,
  lock: null,
  deferred: null,
  expect: null,
};

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const s = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

/** Apply one uart.* control/state event. Returns the new state (or the same object). */
export function reduceInput(st: UartInputState, ev: LoomEvent, nowMs: number): UartInputState {
  return reduceInputState(st, ev.kind, ev.payload, nowMs);
}

function reduceInputState(st: UartInputState, kind: string, payload: unknown, nowMs: number): UartInputState {
  const p = obj(payload);
  switch (kind) {
    case 'uart.input_owner': {
      const owner = p.owner === 'user' || p.owner === 'agent' ? p.owner : 'none';
      return {
        ...st,
        owner,
        viewer: s(p.viewer),
        dirty: p.dirty === true,
        hold: typeof p.hold === 'boolean' ? p.hold : st.hold,
      };
    }
    case 'uart.hold':
      return { ...st, hold: p.on === true };
    case 'uart.lock':
      return p.on === true
        ? { ...st, lock: { reason: s(p.reason) ?? '', ttlMs: n(p.ttl_ms), actor: s(p.actor), atMs: nowMs } }
        : { ...st, lock: null };
    case 'uart.agent_activity': {
      const w: Waiting = { text: s(p.text) ?? '', elapsedMs: n(p.elapsed_ms) ?? 0, timeoutMs: n(p.timeout_ms), atMs: nowMs };
      if (p.kind === 'deferred_send') return { ...st, deferred: w };
      if (p.kind === 'expect') return { ...st, expect: w };
      if (p.kind === 'idle') return { ...st, deferred: null, expect: null };
      return st;
    }
    default:
      return st;
  }
}

export function reducePort(st: UartPortState, ev: LoomEvent): UartPortState {
  if (ev.kind !== 'uart.state' && ev.kind !== 'uart.config') return st;
  return reducePortState(st, ev.payload, ev.generation);
}

function reducePortState(st: UartPortState, payload: unknown, generation?: number | null): UartPortState {
  const p = obj(payload);
  const c = obj(p.config);
  const next: UartPortState = {
    status: s(p.state) ?? s(p.status) ?? st.status,
    baud: n(c.baud) ?? n(p.baud) ?? st.baud,
    generation: generation ?? n(p.generation) ?? st.generation,
  };
  // optional fields stay absent until a real event/snapshot reports them (never invented)
  const path = s(p.path) ?? s(c.path) ?? st.path;
  const dataBits = n(c.data_bits) ?? n(p.data_bits) ?? st.dataBits;
  const parity = s(c.parity) ?? s(p.parity) ?? st.parity;
  const stopBits = stopText(c.stop_bits) ?? stopText(p.stop_bits) ?? st.stopBits;
  const flow = s(c.flow) ?? s(p.flow) ?? st.flow;
  if (path !== undefined) next.path = path;
  if (dataBits !== undefined) next.dataBits = dataBits;
  if (parity !== undefined) next.parity = parity;
  if (stopBits !== undefined) next.stopBits = stopBits;
  if (flow !== undefined) next.flow = flow;
  return next;
}

const stopText = (v: unknown): string | null => (typeof v === 'string' ? v : typeof v === 'number' && Number.isFinite(v) ? String(v) : null);

const PARITY_LETTER: Record<string, string> = { none: 'N', even: 'E', odd: 'O', mark: 'M', space: 'S' };

/**
 * CoolTerm-style frame notation "8-N-1" from the reported config. Unknown parts are '?';
 * returns null when none of data bits / parity / stop bits has been reported (never assumes 8-N-1).
 */
export function frameText(p: UartPortState): string | null {
  if (p.dataBits === undefined && p.parity === undefined && p.stopBits === undefined) return null;
  const par = p.parity === undefined ? '?' : (PARITY_LETTER[p.parity.toLowerCase()] ?? p.parity);
  return `${p.dataBits ?? '?'}-${par}-${p.stopBits ?? '?'}`;
}

/** true when the reported port status means the port is open for sending */
export const isOpenStatus = (status: string): boolean => status.toLowerCase() === 'open';

/** Native UART reducer state; metadata restoration never invents device bytes/events. */
export function restoreUartState(value: unknown, nowMs: number): { port: UartPortState; input: UartInputState } {
  const state = obj(value);
  let port = reducePortState({ status: '?', baud: null, generation: null }, state.state, n(state.generation));
  port = reducePortState(port, state.config);
  let input = INITIAL_INPUT;
  for (const [key, kind] of [['input_owner', 'uart.input_owner'], ['lock', 'uart.lock'], ['agent_activity', 'uart.agent_activity']] as const) {
    if (state[key] !== undefined) input = reduceInputState(input, kind, state[key], nowMs);
  }
  return { port, input };
}

export type ChipKind = 'free' | 'me' | 'other' | 'lock' | 'deferred' | 'dirty' | 'hold' | 'paused';

export interface Chip {
  kind: ChipKind;
  text: string;
  /** which buttons make sense now */
  canYield: boolean;
  canClear: boolean;
  canTakeover: boolean;
}

function secs(w: Waiting, nowMs: number): string {
  const e = (w.elapsedMs + Math.max(0, nowMs - w.atMs)) / 1000;
  return w.timeoutMs ? `${e.toFixed(1)}/${(w.timeoutMs / 1000).toFixed(0)}s` : `${e.toFixed(1)}s`;
}

/**
 * Input-lease chip (plan §5.1.2). Priority: PAUSED(local) > HOLD > agent lock >
 * agent waiting on my line > me > other viewer > dirty line > free.
 */
export function inputChip(st: UartInputState, myConnId: string | null, paused: boolean, nowMs: number): Chip {
  const mine = st.owner === 'user' && st.viewer !== null && st.viewer === myConnId;
  const base = { canYield: mine, canClear: st.dirty, canTakeover: st.lock !== null || (st.owner === 'user' && !mine) };
  if (paused) return { ...base, kind: 'paused', text: 'PAUSED — 키 송신 안 됨 (Ctrl-C만 통과)' };
  if (st.hold) return { ...base, kind: 'hold', text: 'HOLD — 에이전트 쓰기 보류' };
  if (st.lock) {
    const left =
      st.lock.ttlMs !== null ? ` ${Math.max(0, Math.round((st.lock.ttlMs - (nowMs - st.lock.atMs)) / 1000))}s` : '';
    return { ...base, kind: 'lock', text: `🔒 에이전트 잠금 "${st.lock.reason}"${left}` };
  }
  if (st.deferred) return { ...base, kind: 'deferred', text: `⏳ 에이전트 대기 "${st.deferred.text}" ${secs(st.deferred, nowMs)}` };
  if (mine) return { ...base, kind: 'me', text: '입력권: 나 (입력 중)' };
  if (st.owner === 'user') return { ...base, kind: 'other', text: `입력권: 다른 뷰어 ${st.viewer ?? '?'}` };
  if (st.dirty) return { ...base, kind: 'dirty', text: '미완결 줄 남음' };
  if (st.owner === 'agent') return { ...base, kind: 'other', text: '입력권: 에이전트' };
  return { ...base, kind: 'free', text: '입력권: 비어 있음' };
}

export function expectText(st: UartInputState, nowMs: number): string | null {
  return st.expect ? `에이전트: expect "${st.expect.text}" ${secs(st.expect, nowMs)}` : null;
}

/** Visible-symbol rendering for status lines and the trace (never raw control bytes). */
export function symbolize(b: Uint8Array | string): string {
  const str = typeof b === 'string' ? b : new TextDecoder('utf-8', { fatal: false }).decode(b);
  let out = '';
  for (const ch of str) {
    const c = ch.codePointAt(0) as number;
    if (c === 0x0d) out += '⏎';
    else if (c === 0x0a) out += '␊';
    else if (c === 0x09) out += '⇥';
    else if (c === 0x1b) out += '␛';
    else if (c < 0x20) out += `^${String.fromCharCode(c + 0x40)}`;
    else if (c === 0x7f) out += '^?';
    else out += ch;
  }
  return out;
}
