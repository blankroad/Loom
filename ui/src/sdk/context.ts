// PanelContext implementation (plan §4.2). The only handle a panel gets.
// Undeclared topic / query / action → throws UndeclaredError (I-UI1, plan §4.3).

import type { NetClient, Subscription } from '../net/api';
import type { Env, LoomEvent, Right, WriteState } from '../proto/provisional';
import { daemonNowNs, LagTracker, RenderMetrics } from '../store/renderMetrics';
import type { RenderScheduler, PanelHandle } from '../store/renderScheduler';
import type { RowStore } from '../store/rowStore';
import type { ShellControlStore } from '../store/shellControl';
import type { ActionGate } from './actionGate';
import type { ActionSpec, PanelManifest, Props } from './manifest';
import { MIRROR_ROWS_DEFAULT } from './manifest';
import type { EvidenceRef } from './evidenceRef';
import { createRowView, type RowFilter, type RowView } from './rowView';
import { unwrapDomainResult } from './rpcEnvelope';
import { EventMirror, type StoreMirror } from './storeMirror';

export type Unsubscribe = () => void;

export class UndeclaredError extends Error {
  constructor(panelId: string, what: string, name: string) {
    super(`panel ${panelId}: ${what} "${name}" is not declared in its manifest (I-UI1)`);
    this.name = 'UndeclaredError';
  }
}

export interface WriteHandle {
  /** null when the RPC result carried no request_id */
  readonly requestId: string | null;
  state(): WriteState | null;
  onChange(cb: (s: WriteState) => void): Unsubscribe;
}

export interface CursorBus {
  link(panelId: string, group: string): void;
  set(panelId: string, seq: bigint): void;
  on(panelId: string, cb: (seq: bigint) => void): Unsubscribe;
}

export interface ShellUiHooks {
  /** TEST evidence REF or `<domain>:<channel>@req:<id>` → the panel that owns it selects + scrolls to it */
  reveal(ref: string): void;
  /** panels listen for reveals; return true when this panel handled it */
  onReveal?(cb: (ref: EvidenceRef) => boolean): Unsubscribe;
  openPanel(id: string, props: Props): void;
  toast(msg: string): void;
}

export interface PanelContext {
  readonly panelId: string;
  readonly props: Readonly<Props>;
  readonly rights: ReadonlySet<Right>;
  readonly env: Env | null;
  /** this connection's id (hello.conn_id) — tells "my" input lease from another viewer's */
  readonly connId: string | null;
  readonly recordHealth: 'ok' | 'degraded' | 'failed' | 'disabled';
  /** global STOP latch is on (output-holding panels show "STOP — 출력 유지 중") */
  readonly stopLatched: boolean;
  /** STALLED or not connected: the shell refuses writes (STOP / safety-direction excepted) — disable inputs */
  readonly writeLocked: boolean;
  /** STOP latch, read-only + change subscription (agent/script writes locked, user input allowed) */
  latch: LatchInfo;
  store(topicKey: string): StoreMirror<LoomEvent>;
  /** replace the topic subscription with one carrying `filter.ext` (null = none); returns the NEW mirror */
  resubscribe(topicKey: string, ext: Record<string, unknown> | null): StoreMirror<LoomEvent>;
  /** request_id write rows of this panel's domain (shared RowStore, read-only) */
  rows(filter: RowFilter): RowView;
  /** highest seq any agent has acknowledged (core.agent_cursor); null = none yet */
  agentCursor: { seq(): bigint | null; on(cb: () => void): Unsubscribe };
  query<R = unknown>(rpc: string, params: object, signal?: AbortSignal): Promise<R>;
  act(actionId: string, params: object): Promise<WriteHandle>;
  scheduler: {
    invalidate(): void;
    onFrame(cb: (budgetMs: number) => bigint | void): Unsubscribe;
    /** the shell scheduler throttled this panel (LIVE-DEGRADED): draw over budget or load > 300 ms/s */
    readonly degraded: boolean;
    onDegradedChange(cb: (degraded: boolean) => void): Unsubscribe;
    /** ms of work done outside the frame callback (ingest) — counts toward the degrade load rule */
    reportWork(ms: number): void;
    /** events this panel received but chose not to show (display budget) — goes to the render report */
    reportOmitted(n: number): void;
  };
  clock: { monoEstNs(): bigint; errMs(): number };
  cursor: { link(group: string): void; set(seq: bigint): void; on(cb: (seq: bigint) => void): Unsubscribe };
  ui: Omit<ShellUiHooks, 'onReveal'> & {
    mark(label: string): Promise<void>;
    /** handle reveals addressed to this panel (e.g. UDS console selects a request_id row) */
    onReveal(cb: (ref: EvidenceRef) => boolean): Unsubscribe;
  };
}

export interface LatchInfo {
  readonly on: boolean;
  readonly by: string | null;
  readonly origin: string | null;
  /** called when on/by/origin change */
  subscribe(cb: () => void): Unsubscribe;
}

export interface ContextDeps {
  net: Pick<NetClient, 'call' | 'subscribe' | 'clock'>;
  gate: ActionGate;
  scheduler: RenderScheduler;
  rows: RowStore;
  shell: ShellControlStore;
  cursor: CursorBus;
  ui: ShellUiHooks;
  /** ActivityStore (agent cursor). Optional for tests. */
  agentCursor?: { readonly agentCursor: bigint; subscribe(cb: () => void): Unsubscribe };
  /** tab-wide lag/render metrics (shell); a private one is used when absent (tests) */
  metrics?: RenderMetrics;
  /** monotonic ms (performance.now) */
  now?: () => number;
  /** absolute browser ms on the same base as the net clock (performance.timeOrigin + now) */
  absNowMs?: () => number;
}

const MARK_ACTION: ActionSpec = {
  id: 'mark',
  rpc: 'core.mark',
  class: 'request',
  right: 'operate',
  confirm: 'none',
  label: '마커',
};

export interface PanelContextHandle {
  ctx: PanelContext;
  /** dock tab shown/hidden: hidden panels are not drawn and do not count for the LIVE badge */
  setVisible(visible: boolean): void;
  dispose(): void;
}

/** `instanceId` lets the same panel type appear twice (e.g. two UART channels). */
export function createPanelContext(
  m: PanelManifest,
  props: Props,
  deps: ContextDeps,
  instanceId: string = m.id,
): PanelContextHandle {
  const mirrors = new Map<string, EventMirror>();
  const subByKey = new Map<string, Subscription>();
  const unsubs: Unsubscribe[] = [];
  let frameCb: ((budgetMs: number) => bigint | void) | null = null;
  const now = deps.now ?? (() => performance.now());
  const absNow = deps.absNowMs ?? (() => performance.timeOrigin + performance.now());
  const metrics = deps.metrics ?? new RenderMetrics();
  const channel = typeof props.channel === 'string' ? props.channel : null;
  const pm = metrics.panel(instanceId, channel);
  // over MAX_PANELS: still track lag (freshness), just not reported
  const lag = pm?.lag ?? new LagTracker();
  const handle: PanelHandle = deps.scheduler.register(
    instanceId,
    (budget) => {
      const t0 = now();
      const seq = frameCb?.(budget);
      const t1 = now();
      const through = typeof seq === 'bigint' ? seq : null;
      lag.drawn(through, t0, t1, (uiMs) => metrics.add(instanceId, 'ui', uiMs));
      metrics.sampleDraw(instanceId, through, t1);
      return seq;
    },
    { maxHz: m.render.maxHz },
  );

  // shell-derived ctx getters (rights, env, recordHealth, stopLatched) must redraw the panel
  const shellKey = () =>
    `${deps.shell.latch.on}|${deps.shell.recordHealth}|${deps.shell.env}|${[...deps.shell.rights].join(',')}|${deps.shell.status.writeLocked}`;
  let lastShellKey = shellKey();
  unsubs.push(
    deps.shell.subscribe(() => {
      const k = shellKey();
      if (k !== lastShellKey) {
        lastShellKey = k;
        handle.invalidate();
      }
    }),
  );

  // browser-side lag (renderMetrics.ts), polled by the shell tick
  deps.shell.setPanelFreshness(instanceId, () => {
    const clock = deps.net.clock();
    const r = lag.read(now(), clock.synced ? daemonNowNs(clock, absNow()) : null, clock);
    return {
      lagMs: r.lagMs,
      targetMs: m.render.lagTargetMs ?? 150,
      degraded: handle.degraded || [...mirrors.values()].some((mi) => mi.gaps.length > 0 && !mi.snapshotDone),
      visible: handle.visible,
      idleMs: r.idleMs,
      basis: r.basis,
      clockSynced: r.clockSynced,
      clockErrMs: r.clockErrMs,
      serverLagMs: r.serverLagMs,
    };
  });

  /** open (or reopen) a declared topic; `ext` = domain filter extension (e.g. eth_filter) */
  const open = (topicKey: string, ext: Record<string, unknown> | null): EventMirror => {
    const spec = m.topics.find((t) => t.key === topicKey);
    if (!spec) throw new UndeclaredError(m.id, 'topic', topicKey);
    const mirror = new EventMirror(spec.mirrorRows ?? MIRROR_ROWS_DEFAULT);
    mirrors.set(topicKey, mirror);
    const base = spec.filter(props);
    const sub = deps.net.subscribe(
      ext ? { ...base, ext } : base,
      {
        snapshot: spec.snapshot === 'required',
        ...(spec.maxBatchMs !== undefined ? { maxBatchMs: spec.maxBatchMs } : {}),
        ...(spec.encoding !== undefined ? { encoding: spec.encoding } : {}),
        ...(spec.coalesceBy !== undefined ? { coalesceBy: spec.coalesceBy } : {}),
      },
      {
        onSnapshotPart: (p) => {
          mirror.snapshotPart(p);
          deps.shell.noteSnapshotItems(p.items ?? []);
          deps.rows.applyMany(p.events ?? []);
        },
        onSnapshotEnd: (h) => {
          mirror.snapshotEnd(h);
          handle.invalidate();
        },
        onBatch: (b) => {
          const t0 = now();
          const clock = deps.net.clock();
          const recvAge = LagTracker.receiveAge(b.events, clock.synced ? daemonNowNs(clock, absNow()) : null, clock);
          if (recvAge !== null) metrics.add(instanceId, 'receive', recvAge);
          const lost = mirror.overwritten;
          mirror.batch(b);
          deps.rows.applyMany(b.events);
          metrics.add(instanceId, 'apply', now() - t0);
          lag.receive(b.events, t0, b.lag_ms);
          metrics.sampleReceive(instanceId, b.events, t0);
          if (pm) {
            const dg = b.display_gap as { count?: unknown } | null | undefined;
            if (dg && typeof dg.count === 'number') pm.omitted += dg.count;
            pm.omitted += mirror.overwritten - lost;
          }
          handle.invalidate();
        },
        onResync: (reason) => {
          mirror.resync(reason);
          handle.invalidate();
        },
      },
    );
    subByKey.set(topicKey, sub);
    return mirror;
  };

  const ctx: PanelContext = {
    panelId: instanceId,
    props: Object.freeze({ ...props }),
    get rights() {
      return deps.shell.rights;
    },
    get env() {
      return deps.shell.env;
    },
    get connId() {
      return deps.shell.hello?.conn_id ?? null;
    },
    get recordHealth() {
      return deps.shell.recordHealth;
    },
    get stopLatched() {
      return deps.shell.latch.on;
    },
    get writeLocked() {
      return deps.shell.status.writeLocked;
    },
    latch: {
      get on() {
        return deps.shell.latch.on;
      },
      get by() {
        return deps.shell.latch.by;
      },
      get origin() {
        return deps.shell.latch.origin;
      },
      subscribe(cb) {
        const key = () => `${deps.shell.latch.on}|${deps.shell.latch.by ?? ''}|${deps.shell.latch.origin ?? ''}`;
        let last = key();
        const off = deps.shell.subscribe(() => {
          const k = key();
          if (k !== last) {
            last = k;
            cb();
          }
        });
        unsubs.push(off);
        return off;
      },
    },

    rows(filter) {
      // a panel may only view rows of its own domain
      if (filter.domain !== m.domain) throw new UndeclaredError(m.id, 'row domain', filter.domain);
      return createRowView(deps.rows, filter);
    },

    agentCursor: {
      seq: () => {
        const c = deps.agentCursor?.agentCursor;
        return c === undefined || c < 0n ? null : c;
      },
      on: (cb) => deps.agentCursor?.subscribe(cb) ?? (() => {}),
    },

    store(topicKey) {
      return mirrors.get(topicKey) ?? open(topicKey, null);
    },

    resubscribe(topicKey, ext) {
      // close the old server subscription first (contract v1: ≤ 8 per connection)
      subByKey.get(topicKey)?.close();
      subByKey.delete(topicKey);
      mirrors.delete(topicKey);
      return open(topicKey, ext);
    },

    query<R = unknown>(rpc: string, params: object, signal?: AbortSignal): Promise<R> {
      if (!m.queries.includes(rpc)) throw new UndeclaredError(m.id, 'query', rpc);
      // An RPC declared both as query and as action (e.g. `uds.request` dry-run preview)
      // may only be queried with dry_run:true — the real write must go through act()/ActionGate.
      if (m.actions.some((a) => a.rpc === rpc) && (params as { dry_run?: unknown }).dry_run !== true) {
        throw new UndeclaredError(m.id, 'non-dry-run query of action rpc', rpc);
      }
      return deps.net
        .call<unknown>(rpc, params, signal ? { signal } : undefined)
        .then((r) => unwrapDomainResult<R>(rpc, r));
    },

    async act(actionId, params) {
      const spec = m.actions.find((a) => a.id === actionId);
      if (!spec) throw new UndeclaredError(m.id, 'action', actionId);
      const result = await deps.gate.run<{ request_id?: unknown; data?: { request_id?: unknown } }>(spec, params);
      // bare result or D5 envelope {data:{request_id, write_state, …}} (CAN schema v1 §3)
      const rid = result?.request_id ?? result?.data?.request_id;
      const requestId = typeof rid === 'string' ? rid : null;
      return writeHandle(deps.rows, requestId);
    },

    scheduler: {
      invalidate: () => handle.invalidate(),
      get degraded() {
        return handle.degraded;
      },
      onDegradedChange(cb) {
        const off = handle.onDegradedChange(cb);
        unsubs.push(off);
        return off;
      },
      reportWork: (ms) => handle.addWork(ms),
      reportOmitted: (n) => {
        if (pm && n > 0) pm.omitted += n;
      },
      onFrame(cb) {
        frameCb = cb;
        return () => {
          if (frameCb === cb) frameCb = null;
        };
      },
    },

    clock: {
      monoEstNs() {
        const c = deps.net.clock();
        const ms = performance.timeOrigin + performance.now();
        return BigInt(Math.round(ms * 1e6)) + c.thetaNs;
      },
      errMs: () => deps.net.clock().errMs,
    },

    cursor: {
      link: (group) => deps.cursor.link(instanceId, group),
      set: (seq) => deps.cursor.set(instanceId, seq),
      on: (cb) => deps.cursor.on(instanceId, cb),
    },

    ui: {
      reveal: (ref) => deps.ui.reveal(ref),
      openPanel: (id, p) => deps.ui.openPanel(id, p),
      toast: (msg) => deps.ui.toast(msg),
      onReveal: (cb) => {
        const off = deps.ui.onReveal?.(cb) ?? (() => {});
        unsubs.push(off);
        return off;
      },
      async mark(label) {
        await deps.gate.run(MARK_ACTION, { label });
      },
    },
  };

  return {
    ctx,
    setVisible(v) {
      handle.setVisible(v);
    },
    dispose() {
      for (const s of subByKey.values()) s.close();
      subByKey.clear();
      for (const u of unsubs) u();
      handle.dispose();
      deps.shell.setPanelFreshness(instanceId, null);
      metrics.remove(instanceId);
      mirrors.clear();
    },
  };
}

function writeHandle(rows: RowStore, requestId: string | null): WriteHandle {
  return {
    requestId,
    state: () => (requestId ? (rows.get(requestId)?.state ?? null) : null),
    onChange(cb) {
      if (!requestId) return () => {};
      let last: WriteState | null = rows.get(requestId)?.state ?? null;
      return rows.subscribe(() => {
        const s = rows.get(requestId)?.state ?? null;
        if (s && s !== last) {
          last = s;
          cb(s);
        }
      });
    },
  };
}

/** Simple cursor-group bus (plan §5.6). Bounded by the number of open panels. */
export class SimpleCursorBus implements CursorBus {
  private groupOf = new Map<string, string>();
  private cbs = new Map<string, Set<(seq: bigint) => void>>();
  link(panelId: string, group: string): void {
    this.groupOf.set(panelId, group);
  }
  set(panelId: string, seq: bigint): void {
    const g = this.groupOf.get(panelId);
    if (!g) return;
    for (const [pid, grp] of this.groupOf) {
      if (grp === g && pid !== panelId) for (const cb of this.cbs.get(pid) ?? []) cb(seq);
    }
  }
  on(panelId: string, cb: (seq: bigint) => void): Unsubscribe {
    let s = this.cbs.get(panelId);
    if (!s) this.cbs.set(panelId, (s = new Set()));
    s.add(cb);
    return () => {
      s.delete(cb);
    };
  }
}
