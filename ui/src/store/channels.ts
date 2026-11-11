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

/** Native ETH uses live/replay_done; UDS link is an object, PSU has connected.
 * Missing status and output on/off never imply a device connection.
 */
function channelState(p: Record<string, unknown>): { state: ChanState; raw: string } {
  const raw = (str(p.state) ?? str(p.status) ?? str(p.phase) ?? str(p.link) ?? str(obj(p.link).state) ?? '').toLowerCase();
  let state: ChanState = OPEN.has(raw) || raw === 'live' ? 'open' : CLOSED.has(raw) || raw === 'replay_done' ? 'closed' : 'unknown';
  if (typeof p.connected === 'boolean') state = p.connected ? 'open' : 'closed';
  if (typeof p.opened === 'boolean') state = p.opened ? 'open' : 'closed';
  return { state, raw };
}

export function readChannelSignal(ev: LoomEvent): ChannelSignal | null {
  if (!isStateKind(ev.kind)) return null;
  const domain = ev.domain || ev.kind.slice(0, ev.kind.indexOf('.'));
  if (NOT_DEVICE_DOMAINS.has(domain)) return null;
  const p = obj(ev.payload);
  if (p.write_state !== undefined) return null;
  const channel = ev.channel ?? str(p.channel) ?? str(p.target) ?? str(p.interface) ?? str(p.ch);
  if (!channel || !CHANNEL_RE.test(channel)) return null;
  const { state, raw } = channelState(p);
  const explicit = state !== 'unknown' && (/^(opened|open|closed|close)$/.test(raw) || typeof p.opened === 'boolean' || typeof p.connected === 'boolean' || ev.kind.endsWith('.channel_state'));
  const gen = typeof ev.generation === 'number' ? ev.generation : typeof p.generation === 'number' ? p.generation : null;
  return { domain, channel, state, explicit, generation: gen, origin: ev.origin, actor: ev.actor_id ?? ev.client_id ?? null };
}

/** Native SnapshotItem → reducer → payload/state. CAN adds a state/payload envelope. */
function snapshotLayers(item: unknown): Record<string, unknown>[] {
  const layers: Record<string, unknown>[] = [];
  let current = obj(item);
  for (let depth = 0; depth < 6; depth++) {
    layers.push(current);
    const next = current.payload ?? current.state;
    if (typeof next !== 'object' || next === null) break;
    current = obj(next);
  }
  return layers;
}

/** snapshot item → known channel (silent: it existed before this tab joined) */
export function readSnapshotChannel(item: unknown): { domain: string; channel: string; state: ChanState } | null {
  const layers = snapshotLayers(item);
  const it = layers[0]!;
  const kind = str(it.kind);
  const domain = str(it.domain) ?? (kind?.includes('.') ? kind.split('.')[0] : null);
  if (!domain || NOT_DEVICE_DOMAINS.has(domain) || layers.some((p) => p.write_state !== undefined)) return null;
  const channel = layers.map((p) => str(p.channel) ?? str(p.target) ?? str(p.interface) ?? str(p.ch)).find(Boolean);
  if (!channel || !CHANNEL_RE.test(channel)) return null;
  // Innermost state is authoritative; an envelope itself proves no connectivity.
  let state: ChanState = 'unknown';
  for (const p of layers.toReversed()) {
    state = channelState(p).state;
    if (state !== 'unknown') break;
  }
  return { domain, channel, state };
}

/** core.channels is always included by the hub, even with only core.activity subscribed. */
export function readSnapshotChannels(item: unknown): { domain: string; channel: string; state: ChanState; generation: number | null }[] {
  const it = obj(item);
  const entries = it.domain === 'core' && it.kind === 'core.channels'
    ? obj(it.state).channels
    : [item];
  if (!Array.isArray(entries)) return [];
  return entries.flatMap((entry) => {
    const channel = readSnapshotChannel(entry);
    if (!channel) return [];
    const generation = snapshotLayers(entry).map((p) => p.generation).find((g) => typeof g === 'number');
    return [{ ...channel, generation: typeof generation === 'number' ? generation : null }];
  });
}

/** Only implemented views are offered. Opening a view never opens a device connection. */
export interface ChannelView {
  panel: string;
  label: string;
  props: Record<string, string>;
}

export const EXPLORER_DOMAINS = ['uart', 'can', 'uds', 'eth', 'psu'] as const;

export function viewsForChannel(domain: string, channel: string): ChannelView[] {
  switch (domain) {
    case 'uart':
      return [
        { panel: 'uart.terminal', label: '터미널', props: { channel } },
        { panel: 'uart.trace', label: '트레이스', props: { channel } },
      ];
    case 'can':
      return [{ panel: 'can.trace', label: '트레이스 · 송신', props: { channel } }];
    case 'uds':
      return [{ panel: 'uds.console', label: '진단 콘솔', props: { targets: channel } }];
    case 'eth':
      return [{ panel: 'eth.packets', label: '패킷 · 캡처', props: { channel } }];
    case 'psu':
      return [{ panel: 'psu.gauge', label: '전압 · 전류', props: { channel } }];
    default:
      return [];
  }
}

/** Default remains compatible with automatic opening and existing deep links. */
export function defaultPanelFor(domain: string, channel: string): { panel: string; props: Record<string, string> } | null {
  const view = viewsForChannel(domain, channel)[0];
  return view ? { panel: view.panel, props: view.props } : null;
}

export const ORIGIN_SUBJECT: Record<string, string> = {
  agent: '에이전트가',
  user: '사용자가',
  script: '스크립트가',
  device: '장치가',
  external: '외부 도구가',
};
