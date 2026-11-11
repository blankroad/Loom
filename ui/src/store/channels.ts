// Channel signals for the "no hidden channel" rule (plan §5.7 UD-31, UART spec Q13).
// Any domain state event (`<domain>.state`, `<domain>.*_state` — priority kinds in
// loom-proto `is_priority_kind`) names a channel/target the agent is using. The shell
// adds it to every tab's channel bar; an explicit open (or re-open) also raises a toast.
//
// Provisional payload reading (report p1-m1b §4, ask CORE/domains):
//   channel  = ev.channel ?? payload.channel ?? payload.target ?? payload.interface
//   state    = payload.state | payload.status | payload.phase | payload.link | payload.opened:boolean
//   open     ∈ opened open running started connected up active capturing attached
//   closed   ∈ closed close stopped disconnected down released removed detached
// Write rows (payload.write_state) are not channel signals.

import type { LoomEvent, Origin } from '../proto/provisional';

export type ChanState = 'open' | 'closed' | 'unknown';

export interface ChannelSignal {
  domain: string;
  channel: string;
  state: ChanState;
  /** the event said so explicitly (opened/closed words), not just "a state report exists" */
  explicit: boolean;
  generation: number | null;
  origin: Origin;
  actor: string | null;
}

const NOT_DEVICE_DOMAINS = new Set(['core', 'display', 'record', 'source', 'test', 'run', 'ui']);
const OPEN = new Set(['opened', 'open', 'running', 'started', 'connected', 'up', 'active', 'capturing', 'attached']);
const CLOSED = new Set(['closed', 'close', 'stopped', 'disconnected', 'down', 'released', 'removed', 'detached']);
const CHANNEL_RE = /^[A-Za-z0-9_.-]{1,64}$/;

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const str = (v: unknown): string | null => (typeof v === 'string' && v ? v : null);

export function isStateKind(kind: string): boolean {
  const dot = kind.indexOf('.');
  if (dot <= 0) return false;
  const ev = kind.slice(dot + 1);
  return ev === 'state' || ev.endsWith('_state');
}

export function readChannelSignal(ev: LoomEvent): ChannelSignal | null {
  if (!isStateKind(ev.kind)) return null;
  const domain = ev.domain || ev.kind.slice(0, ev.kind.indexOf('.'));
  if (NOT_DEVICE_DOMAINS.has(domain)) return null;
  const p = obj(ev.payload);
  if (p.write_state !== undefined) return null;
  const channel = ev.channel ?? str(p.channel) ?? str(p.target) ?? str(p.interface);
  if (!channel || !CHANNEL_RE.test(channel)) return null;
  const raw = (str(p.state) ?? str(p.status) ?? str(p.phase) ?? str(p.link) ?? '').toLowerCase();
  let state: ChanState = OPEN.has(raw) ? 'open' : CLOSED.has(raw) ? 'closed' : 'unknown';
  if (p.opened === true) state = 'open';
  if (p.opened === false) state = 'closed';
  const explicit = state !== 'unknown' && (/^(opened|open|closed|close)$/.test(raw) || typeof p.opened === 'boolean' || ev.kind.endsWith('.channel_state'));
  const gen = typeof ev.generation === 'number' ? ev.generation : typeof p.generation === 'number' ? p.generation : null;
  return { domain, channel, state, explicit, generation: gen, origin: ev.origin, actor: ev.actor_id ?? ev.client_id ?? null };
}

/** snapshot item → known channel (silent: it existed before this tab joined) */
export function readSnapshotChannel(item: unknown): { domain: string; channel: string; state: ChanState } | null {
  const it = obj(item);
  const domain = str(it.domain);
  if (!domain || NOT_DEVICE_DOMAINS.has(domain)) return null;
  const st = obj(it.state);
  const channel = str(it.channel) ?? str(st.channel) ?? str(st.target);
  if (!channel || !CHANNEL_RE.test(channel)) return null;
  const raw = (str(st.state) ?? str(st.status) ?? '').toLowerCase();
  return { domain, channel, state: OPEN.has(raw) ? 'open' : CLOSED.has(raw) ? 'closed' : 'unknown' };
}

/** default panel for a domain's channel (null = no panel yet → bar + toast only) */
export function defaultPanelFor(domain: string, channel: string): { panel: string; props: Record<string, string> } | null {
  switch (domain) {
    case 'uart':
      return { panel: 'uart.terminal', props: { channel } };
    case 'can':
      return { panel: 'can.trace', props: { channel } };
    case 'uds':
      return { panel: 'uds.console', props: { targets: channel } };
    case 'eth':
      return { panel: 'eth.packets', props: { channel } };
    case 'psu':
      return { panel: 'psu.gauge', props: { channel } };
    default:
      return null;
  }
}

export const ORIGIN_SUBJECT: Record<string, string> = {
  agent: '에이전트가',
  user: '사용자가',
  script: '스크립트가',
  device: '장치가',
  external: '외부 도구가',
};
