// ShellControlStore — small, always-current shell state fed by the control path
// (plan §3.3 ShellControlStore, §3.5 freshness, §5.4 banners). Pure TS; React
// reads it via useStoreVersion().
//
// Freshness (plan §3.5-6, UD-23):
//   heartbeat every 1 s · ≥ 1.5 s silent → grey warning dot · ≥ 3 s → STALLED
//   STALLED outranks every other badge and raises `writeLocked` (shell disables
//   all write buttons except STOP / safety-reducing actions).
//   Otherwise: LAG if any visible panel's lag exceeds its path target (red when
//   > 2 s), else LIVE-DEGRADED if a panel is dropping/downsampling/throttled,
//   else LIVE.
// Lag is the BROWSER-side lag (renderMetrics.ts): oldest received-but-undrawn event's
// age. Panels register a freshness provider that the 250 ms tick polls, because lag
// grows while nothing arrives. Nothing pending → lag 0 + idle; the server batch lag_ms
// is kept for the tooltip only. Clock not synced or ± error above the smallest visible
// target → clockUncertain ("동기 불확실", never counted as LIVE proof — §9.5).

import type { ConnState } from '../net/api';
import type { Env, HelloResult, LoomEvent, Right } from '../proto/provisional';
import { RIGHT_RANK } from '../proto/provisional';
import {
  readApproval,
  readApprovalResolvedId,
  readGap,
  readHeartbeat,
  readLatch,
  readRecordHealth,
  readViewers,
  readWorkerState,
  type ApprovalView,
  type GapView,
  type LatchView,
  type RecordHealth,
  type WorkerStateView,
} from './adapter';
import { readChannelSignal, readSnapshotChannels, type ChanState, type ChannelSignal } from './channels';
import { Emitter } from './emitter';

export const HEARTBEAT_WARN_MS = 1_500;
export const HEARTBEAT_STALL_MS = 3_000;
export const LAG_SEVERE_MS = 2_000;

export type LiveBadge = 'LIVE' | 'LAG' | 'STALLED' | 'LIVE-DEGRADED';

export interface LiveStatus {
  badge: LiveBadge;
  /** grey dot: heartbeat late (≥ 1.5 s) but not yet STALLED */
  warnDot: boolean;
  /** STALLED or not connected → shell locks write actions (STOP excepted) */
  writeLocked: boolean;
  lagMs: number;
  lagSevere: boolean;
  sinceHeartbeatMs: number | null;
  /** all visible panels idle → ms since the most recent receipt (null = something pending / never) */
  idleMs: number | null;
  clockUncertain: boolean;
  clockErrMs: number | null;
  /** max server-side batch lag_ms (tooltip) */
  serverLagMs: number;
  /** a visible panel's lag is receipt-based (clock not usable for it) */
  lagFromReceipt: boolean;
}

export interface PanelFreshness {
  lagMs: number;
  targetMs: number;
  degraded: boolean;
  visible: boolean;
  idleMs?: number | null;
  basis?: 'mono' | 'recv' | 'none';
  clockSynced?: boolean;
  clockErrMs?: number;
  serverLagMs?: number;
}

export function computeLiveStatus(i: {
  connOpen: boolean;
  lastHeartbeatMs: number | null;
  nowMs: number;
  panels: Iterable<PanelFreshness>;
}): LiveStatus {
  const since = i.lastHeartbeatMs === null ? null : Math.max(0, i.nowMs - i.lastHeartbeatMs);
  let lagMs = 0;
  let over = false;
  let degraded = false;
  let idleMs: number | null = null;
  let anyPending = false;
  let minTarget = Number.POSITIVE_INFINITY;
  let clockErrMs: number | null = null;
  let unsynced = false;
  let serverLagMs = 0;
  let lagFromReceipt = false;
  for (const p of i.panels) {
    if (!p.visible) continue;
    lagMs = Math.max(lagMs, p.lagMs);
    if (p.lagMs > p.targetMs) over = true;
    if (p.degraded) degraded = true;
    minTarget = Math.min(minTarget, p.targetMs);
    serverLagMs = Math.max(serverLagMs, p.serverLagMs ?? 0);
    if (p.basis === 'recv') lagFromReceipt = true;
    if (p.basis === 'mono' || p.basis === 'recv') anyPending = true;
    else if (typeof p.idleMs === 'number') idleMs = idleMs === null ? p.idleMs : Math.min(idleMs, p.idleMs);
    if (p.clockSynced === false) unsynced = true;
    if (typeof p.clockErrMs === 'number' && Number.isFinite(p.clockErrMs)) clockErrMs = Math.max(clockErrMs ?? 0, p.clockErrMs);
  }
  const clockUncertain = unsynced || (clockErrMs !== null && clockErrMs > minTarget);
  const base = {
    lagMs,
    lagSevere: lagMs > LAG_SEVERE_MS,
    sinceHeartbeatMs: since,
    idleMs: anyPending ? null : idleMs,
    clockUncertain,
    clockErrMs,
    serverLagMs,
    lagFromReceipt,
  };
  if (!i.connOpen || since === null || since >= HEARTBEAT_STALL_MS) {
    return { ...base, badge: 'STALLED', warnDot: false, writeLocked: true };
  }
  const warnDot = since >= HEARTBEAT_WARN_MS;
  if (over) return { ...base, badge: 'LAG', warnDot, writeLocked: false };
  if (degraded) return { ...base, badge: 'LIVE-DEGRADED', warnDot, writeLocked: false };
  return { ...base, badge: 'LIVE', warnDot, writeLocked: false };
}

export interface ChannelEntry {
  domain: string;
  channel: string;
  generation: number | null;
  state: ChanState;
  /** client ms when this tab first learned of it live (null = known at join / snapshot) */
  addedLiveMs: number | null;
  /** client ms of the last generation change (device reconnect) */
  genChangedMs: number | null;
}

/** a channel the agent (or anyone) opened while this tab was watching */
export interface ChannelOpened {
  entry: ChannelEntry;
  signal: ChannelSignal;
  /** true = first time this tab sees it; false = re-opened after closed */
  isNew: boolean;
}

export interface ShellErrorView {
  atMs: number;
  message: string;
}


/** what the top bar shows — tick() notifies only when this changes (≤ 4 Hz) */
function statusKey(s: LiveStatus): string {
  return [
    s.badge,
    s.warnDot,
    s.writeLocked,
    Math.round(s.lagMs / 10),
    s.idleMs === null ? '-' : Math.floor(s.idleMs / 100),
    s.clockUncertain,
    s.lagFromReceipt,
    s.sinceHeartbeatMs === null ? '-' : Math.floor(s.sinceHeartbeatMs / 100),
  ].join('|');
}

/** rights are hierarchical: approve ⊃ operate ⊃ watch (plan §3.7). */
export function hasRight(rights: ReadonlySet<Right>, need: Right): boolean {
  for (const r of rights) if (RIGHT_RANK[r] >= RIGHT_RANK[need]) return true;
  return false;
}

const NO_LATCH: LatchView = {
  on: false,
  by: null,
  origin: null,
  acked: false,
  missing: [],
  persisted: false,
  jobsStopped: [],
  approvalsCancelled: 0,
  uncancellable: [],
  outputsHeld: [],
};

// C2 bounds for shell collections (overflow policy noted per field)
const MAX_APPROVALS = 256; // over: oldest pending dropped from the VIEW + counted (server stays authoritative)
const MAX_GAPS = 50; // ring: oldest overwritten
const MAX_WORKERS = 64; // over: new workers ignored + counted
/** after a (re)connect, implicit channel reports are treated as pre-existing (no toast) */
export const JOIN_GRACE_MS = 3_000;
const MAX_CHANNELS = 64; // over: new channels ignored + counted
const MAX_ERRORS = 20; // ring

export class ShellControlStore extends Emitter {
  conn: ConnState = { status: 'connecting', attempt: 0 };
  hello: HelloResult | null = null;
  latch: LatchView = NO_LATCH;
  recordHealth: RecordHealth = 'ok';
  viewers: { subscribed: number; visible: number } | null = null;
  approvals = new Map<string, ApprovalView>();
  workers = new Map<string, WorkerStateView>();
  channels = new Map<string, ChannelEntry>();
  gaps: GapView[] = [];
  errors: ShellErrorView[] = [];
  epochChange: { atMs: number } | null = null;
  overflow = { approvals: 0, workers: 0, channels: 0 };
  /** navigator.webdriver: V5 watch cap unless the server explicitly declares F1 Auto (or mock) */
  automation = false;
  private automationPolicyStale = false;
  private openedListeners = new Set<(o: ChannelOpened) => void>();
  /** client ms the connection last opened (join grace for the new-channel toast) */
  private connOpenedMs: number | null = null;

  private lastHeartbeatMs: number | null = null;
  private panels = new Map<string, PanelFreshness | (() => PanelFreshness)>();
  private nowMs = 0;
  private _status: LiveStatus = computeLiveStatus({
    connOpen: false,
    lastHeartbeatMs: null,
    nowMs: 0,
    panels: [],
  });

  get status(): LiveStatus {
    return this._status;
  }
  get env(): Env | null {
    return this.hello?.env ?? null;
  }
  /** the server is the dev mock (hello.capabilities 'mock') */
  get mockServer(): boolean {
    return this.hello?.capabilities?.includes('mock') === true;
  }
  /** Automation without a current F1 Auto declaration acts as watch (V5). */
  get automationCapped(): boolean {
    return this.automation && !this.mockServer && (this.automationPolicyStale || this.hello?.capabilities?.includes('policy/auto') !== true);
  }
  get rights(): ReadonlySet<Right> {
    const r = this.hello?.rights; // v1: single ordered level
    if (r && this.automationCapped) return new Set<Right>(['watch']);
    return new Set(r ? [r] : []);
  }
  setAutomation(on: boolean): void {
    if (this.automation === on) return;
    this.automation = on;
    this.changed();
  }
  onChannelOpened(cb: (o: ChannelOpened) => void): () => void {
    this.openedListeners.add(cb);
    return () => this.openedListeners.delete(cb);
  }
  can(need: Right): boolean {
    return hasRight(this.rights, need);
  }

  setConn(s: ConnState, nowMs: number): void {
    const wasOpen = this.conn.status === 'open';
    this.conn = s;
    if (s.status === 'open' && !wasOpen) this.connOpenedMs = nowMs;
    if (s.status === 'open') {
      if (this.hello && this.hello.stream_epoch !== s.hello.stream_epoch) {
        // A new daemon stream cannot prove that devices from the previous bench
        // remain connected. Keep discovery history until fresh state confirms it.
        for (const channel of this.channels.values()) {
          if (channel.state === 'open') channel.state = 'unknown';
          channel.generation = null;
          channel.genChangedMs = null;
          channel.addedLiveMs = null;
        }
      }
      if (!wasOpen || this.hello?.conn_id !== s.hello.conn_id || this.hello?.stream_epoch !== s.hello.stream_epoch) this.automationPolicyStale = false;
      this.hello = s.hello;
      // a fresh connection counts as a heartbeat; the 1 s heartbeat follows
      this.lastHeartbeatMs = nowMs;
      // v1 hello carries viewers and the current latch. A latch reported by hello is
      // established daemon state (persisted), not a pending request → shown as locked;
      // detail (jobs, outputs held) arrives with the next core.latch / snapshot.
      this.viewers = readViewers(s.hello.viewers);
      if (s.hello.latch && !this.latch.on) this.latch = { ...NO_LATCH, on: true, acked: true, persisted: true };
      if (!s.hello.latch && this.latch.on) this.latch = NO_LATCH;
    }
    this.recompute(nowMs);
    this.changed();
  }

  /** Called for every control-path event (NetClient.onControl). */
  applyControl(ev: LoomEvent, nowMs: number): void {
    const k = ev.kind;
    const sig = readChannelSignal(ev);
    if (sig) this.applyChannelSignal(sig, nowMs);
    else if (ev.domain && ev.channel) this.noteChannel(ev.domain, ev.channel, ev.generation ?? null, nowMs);
    if (k === 'core.heartbeat') {
      this.lastHeartbeatMs = nowMs;
      const hb = readHeartbeat(ev);
      if (hb.recordHealth) this.recordHealth = hb.recordHealth;
      if (hb.latch === false && this.latch.on) this.latch = NO_LATCH;
    } else if (k === 'core.policy_changed') {
      // A fresh hello is required before trusting an earlier Auto declaration again.
      this.automationPolicyStale = true;
    } else if (k === 'core.latch') {
      const l = readLatch(ev);
      this.latch = l.on ? l : NO_LATCH;
    } else if (k === 'core.approval_requested') {
      const a = readApproval(ev);
      if (a) this.addApproval(a);
    } else if (k === 'core.approval_resolved') {
      const id = readApprovalResolvedId(ev);
      if (id) this.approvals.delete(id);
    } else if (k === 'core.worker_state') {
      const w = readWorkerState(ev);
      if (this.workers.has(w.worker) || this.workers.size < MAX_WORKERS) this.workers.set(w.worker, w);
      else this.overflow.workers++;
    } else if (k === 'record.health') {
      const h = readRecordHealth((ev.payload as { health?: unknown }).health);
      if (h) this.recordHealth = h;
    } else if (k === 'core.epoch_changed') {
      this.epochChange = { atMs: nowMs };
    } else if (k === 'core.viewers') {
      this.viewers = readViewers(ev.payload);
    } else if (k === 'core.error') {
      const msg = (ev.payload as { message?: unknown }).message;
      this.errors.push({ atMs: nowMs, message: typeof msg === 'string' ? msg : 'error' });
      if (this.errors.length > MAX_ERRORS) this.errors.shift();
    } else {
      const g = readGap(ev);
      if (g) {
        this.gaps.push(g);
        if (this.gaps.length > MAX_GAPS) this.gaps.shift();
      }
    }
    this.recompute(nowMs);
    this.changed();
  }

  /** Batch-level record health (EventBatch.record_health). */
  setRecordHealth(h: RecordHealth): void {
    if (h === this.recordHealth) return;
    this.recordHealth = h;
    this.changed();
  }

  /** any control event naming a channel: add to the bar silently, track generation */
  noteChannel(domain: string, channel: string, generation: number | null, nowMs: number | null = null): ChannelEntry | null {
    const key = `${domain}/${channel}`;
    const cur = this.channels.get(key);
    if (cur) {
      if (generation !== null && cur.generation !== generation) {
        if (cur.generation !== null) cur.genChangedMs = nowMs ?? this.nowMs;
        cur.generation = generation;
      }
      return cur;
    }
    if (this.channels.size >= MAX_CHANNELS) {
      this.overflow.channels++;
      return null;
    }
    const e: ChannelEntry = { domain, channel, generation, state: 'unknown', addedLiveMs: nowMs, genChangedMs: null };
    this.channels.set(key, e);
    return e;
  }

  /** a domain state event: bar always; open (new or re-open) → onChannelOpened listeners (toast / auto-open) */
  applyChannelSignal(sig: ChannelSignal, nowMs: number): void {
    const existed = this.channels.has(`${sig.domain}/${sig.channel}`);
    const e = this.noteChannel(sig.domain, sig.channel, sig.generation, nowMs);
    if (!e) return;
    const before = e.state;
    if (sig.state !== 'unknown') e.state = sig.state;
    // A report establishes identity, not connectivity (e.g. PSU output-off telemetry).
    // Keep unknown until an explicit connection state is available.
    const opened = e.state === 'open' && before !== 'open';
    // join grace: right after (re)connecting, periodic state reports describe channels that
    // were already open before this tab — bar only. An explicit "opened" still toasts.
    const inGrace = this.connOpenedMs !== null && nowMs - this.connOpenedMs < JOIN_GRACE_MS;
    if (!existed && inGrace && !sig.explicit) {
      e.addedLiveMs = null;
      return;
    }
    if (opened) for (const cb of [...this.openedListeners]) cb({ entry: e, signal: sig, isNew: !existed });
  }

  /** late join: channels from snapshot items are added without toasts (they predate this tab) */
  noteSnapshotItems(items: readonly unknown[]): void {
    let changed = false;
    for (const it of items) {
      for (const c of readSnapshotChannels(it)) {
        const e = this.noteChannel(c.domain, c.channel, c.generation, null);
        if (!e) continue;
        if (c.state !== 'unknown') e.state = c.state;
        changed = true;
      }
    }
    if (changed) this.changed();
  }

  /** Panels report lag/degraded (a value, or a provider polled on every tick); the badge is the worst visible panel. */
  setPanelFreshness(panelId: string, f: PanelFreshness | (() => PanelFreshness) | null): void {
    if (f) this.panels.set(panelId, f);
    else this.panels.delete(panelId);
    this.recompute(this.nowMs);
    this.changed();
  }

  /** Periodic re-evaluation (shell runs it every 250 ms). Notifies only on change. */
  tick(nowMs: number): void {
    const before = statusKey(this._status);
    this.recompute(nowMs);
    if (statusKey(this._status) !== before) this.changed();
  }

  dismissEpochChange(): void {
    this.epochChange = null;
    this.changed();
  }

  private addApproval(a: ApprovalView): void {
    if (!this.approvals.has(a.approvalId) && this.approvals.size >= MAX_APPROVALS) {
      const oldest = this.approvals.keys().next().value;
      if (oldest !== undefined) this.approvals.delete(oldest);
      this.overflow.approvals++;
    }
    this.approvals.set(a.approvalId, a);
  }

  private recompute(nowMs: number): void {
    this.nowMs = nowMs;
    this._status = computeLiveStatus({
      connOpen: this.conn.status === 'open',
      lastHeartbeatMs: this.lastHeartbeatMs,
      nowMs,
      panels: [...this.panels.values()].map((p) => (typeof p === 'function' ? p() : p)),
    });
  }
}
