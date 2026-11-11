// Test double for NetClient (src/net/api.ts) — CORE contract v1 shapes.
// Lets store/sdk/shell tests run without the real Worker client. Not for production.

import type {
  ClockEstimate,
  ConnState,
  NetClient,
  Subscription,
  SubscriptionHandlers,
  SubscribeOptions,
} from '../../net/api';
import type {
  EventsBatch,
  HelloResult,
  LoomEvent,
  Origin,
  SubscribeFilter,
  WriteState,
} from '../../proto/provisional';

export interface RecordedCall {
  method: string;
  params: unknown;
}

export const TEST_HELLO: HelloResult = {
  role: 'viewer',
  rights: 'operate',
  conn_id: 'c-test',
  stream_epoch: '1',
  server: { version: 'test', build: 'test' },
  env: 'sim',
  latch: false,
  viewers: { subscribed: 1, visible: 1 },
  capabilities: [],
};

export class FakeNet implements NetClient {
  state: ConnState = { status: 'connecting', attempt: 0 };
  calls: RecordedCall[] = [];
  httpStops = 0;
  visible: boolean[] = [];
  subs: { id: string; filter: SubscribeFilter; opts: SubscribeOptions; h: SubscriptionHandlers; closed: boolean }[] = [];
  /** method → result (or throw) */
  responders = new Map<string, (params: unknown) => unknown>();
  private stateCbs = new Set<(s: ConnState) => void>();
  private controlCbs = new Set<(ev: LoomEvent) => void>();
  private nextSub = 1;

  open(hello: HelloResult = TEST_HELLO): void {
    this.setState({ status: 'open', hello });
  }
  setState(s: ConnState): void {
    this.state = s;
    for (const cb of this.stateCbs) cb(s);
  }
  emitControl(ev: LoomEvent): void {
    for (const cb of this.controlCbs) cb(ev);
  }
  emitBatch(subIndex: number, events: LoomEvent[], extra: Partial<EventsBatch> = {}): void {
    const s = this.subs[subIndex];
    if (!s) throw new Error(`no sub ${subIndex}`);
    s.h.onBatch({
      sub: s.id,
      first: events[0]?.seq ?? '0',
      last: events[events.length - 1]?.seq ?? '0',
      lag_ms: 0,
      events,
      ...extra,
    });
  }

  onState(cb: (s: ConnState) => void): () => void {
    this.stateCbs.add(cb);
    return () => this.stateCbs.delete(cb);
  }
  onControl(cb: (ev: LoomEvent) => void): () => void {
    this.controlCbs.add(cb);
    return () => this.controlCbs.delete(cb);
  }
  async call<R = unknown>(method: string, params?: unknown): Promise<R> {
    this.calls.push({ method, params });
    const r = this.responders.get(method);
    return (r ? r(params) : {}) as R;
  }
  subscribe(filter: SubscribeFilter, opts: SubscribeOptions, h: SubscriptionHandlers): Subscription {
    const rec = { id: `sub-${this.nextSub++}`, filter, opts, h, closed: false };
    this.subs.push(rec);
    return {
      id: rec.id,
      close: () => {
        rec.closed = true;
      },
    };
  }
  clock(): ClockEstimate {
    return { thetaNs: 0n, errMs: 0, clockId: 'test', synced: true };
  }
  setVisible(v: boolean): void {
    this.visible.push(v);
  }
  async stopViaHttp(): Promise<void> {
    this.httpStops++;
  }
  close(): void {
    this.setState({ status: 'closed', reason: 'closed' });
  }
}

let seqCounter = 0;
/** v1 PublishedRecord with optional fields omitted. */
export function ev(partial: Partial<LoomEvent> & { kind: string }): LoomEvent {
  seqCounter++;
  return {
    schema_version: 1,
    session_id: 's-test',
    domain: partial.kind.split('.')[0] ?? 'core',
    epoch: '1',
    seq: String(seqCounter),
    producer_id: 'test',
    producer_epoch: '1',
    source_seq: String(seqCounter),
    ts: { mono_ns: String(seqCounter * 1_000_000), wall_ns: '1762770000000000000', clock_id: 't', source: 'gateway' },
    origin: 'device',
    env: 'sim',
    payload: {},
    ...partial,
  };
}

export function writeEv(
  requestId: string,
  state: WriteState,
  opts: { seq?: number; monoMs?: number; origin?: Origin; payload?: Record<string, unknown>; kind?: string } = {},
): LoomEvent {
  const base = ev({ kind: opts.kind ?? 'uds.request' });
  return {
    ...base,
    ...(opts.seq !== undefined ? { seq: String(opts.seq) } : {}),
    ts: opts.monoMs !== undefined ? { ...base.ts, mono_ns: String(opts.monoMs * 1_000_000) } : base.ts,
    origin: opts.origin ?? 'agent',
    request_id: requestId,
    payload: { write_state: state, ...(opts.payload ?? {}) },
  };
}
