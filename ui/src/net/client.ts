// MainNetClient — the main-thread side of the net layer. Implements NetClient
// (api.ts) by exchanging CoreIn/CoreOut messages with a NetCore that lives in
// a Web Worker (or inline, for tests / environments without Worker).
//
// Data path: pull-based. The proxy sends `pull`, the core answers with one
// `deliver` holding everything decoded since (≤ 1 batch per subscription), and
// the next `pull` is only sent on the next frame. A busy or hidden main thread
// therefore stops pulling and the core's decode budget (budget.ts) takes over.
// Control path: `control` messages are dispatched the moment they arrive.

import type { EventsBatch, LoomEvent, SubscribeFilter } from '../proto/provisional';
import type { ClockEstimate, ConnState, NetClient, Subscription, SubscribeOptions, SubscriptionHandlers } from './api';
import type { DeliveryItem } from './budget';
import { UNSYNCED } from './clock';
import type { CoreIn, CoreOut } from './protocol';
import { NetError } from './rpc';
import { matchesFilter, partEvents, rebatch, stopHttpBody, withPartEvents, type PresenceExtra } from './wire';

/**
 * Receive/decode timing of a delivered batch (2nd argument of onBatch): `recv_ms` = earliest
 * Worker onmessage of the server batches merged into it, `decode_ms` = their parse time; ms on the
 * performance.timeOrigin + performance.now() basis (same as ClockEstimate). UI segment = t_recv → t_draw.
 */
export interface DeliveryTiming {
  recv_ms: number;
  decode_ms: number;
}

export interface CorePort {
  post(m: CoreIn): void;
  listen(cb: (m: CoreOut) => void, onError: (reason: string) => void): void;
  terminate(): void;
}

export interface FrameScheduler {
  request(cb: () => void): void;
}

export interface MainOptions {
  clientId: string;
  /** POST target for the STOP fallback (same origin as the WS) */
  stopUrl: string;
  frames: FrameScheduler;
  fetch?: typeof fetch;
}

interface Handle {
  h: SubscriptionHandlers;
  filter: SubscribeFilter;
  /** shares a broader server subscription: filter events client-side */
  narrow: boolean;
}

interface PendingCall {
  resolve(v: unknown): void;
  reject(e: unknown): void;
  cleanup(): void;
}

export class MainNetClient implements NetClient {
  private _state: ConnState = { status: 'connecting', attempt: 1 };
  private est: ClockEstimate = UNSYNCED;
  private readonly stateCbs = new Set<(s: ConnState) => void>();
  private readonly controlCbs = new Set<(ev: LoomEvent) => void>();
  private readonly handlers = new Map<number, Handle>();
  private readonly bound = new Map<number, Set<number>>();
  private readonly calls = new Map<number, PendingCall>();
  private nextCall = 1;
  private nextHid = 1;
  private closed = false;

  constructor(
    private readonly port: CorePort,
    private readonly o: MainOptions,
  ) {
    port.listen(
      (m) => this.onCore(m),
      (reason) => this.fatal(reason),
    );
    port.post({ t: 'pull' });
  }

  get state(): ConnState {
    return this._state;
  }

  onState(cb: (s: ConnState) => void): () => void {
    this.stateCbs.add(cb);
    return () => this.stateCbs.delete(cb);
  }

  onControl(cb: (ev: LoomEvent) => void): () => void {
    this.controlCbs.add(cb);
    return () => this.controlCbs.delete(cb);
  }

  call<R = unknown>(method: string, params?: unknown, opts?: { signal?: AbortSignal; timeoutMs?: number }): Promise<R> {
    if (this.closed) return Promise.reject(new NetError({ code: 'closed', message: `${method}: client closed`, maybeSent: false }));
    const signal = opts?.signal;
    if (signal?.aborted) return Promise.reject(new NetError({ code: 'aborted', message: `${method}: aborted`, maybeSent: false }));
    const id = this.nextCall++;
    return new Promise<R>((resolve, reject) => {
      const onAbort = () => {
        this.calls.delete(id);
        this.port.post({ t: 'cancel', id });
        reject(new NetError({ code: 'aborted', message: `${method}: aborted`, maybeSent: true }));
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      this.calls.set(id, {
        resolve: resolve as (v: unknown) => void,
        reject,
        cleanup: () => signal?.removeEventListener('abort', onAbort),
      });
      const m: CoreIn = { t: 'call', id, method };
      if (params !== undefined) m.params = params;
      if (opts?.timeoutMs !== undefined) m.timeoutMs = opts.timeoutMs;
      this.port.post(m);
    });
  }

  subscribe(filter: SubscribeFilter, opts: SubscribeOptions, h: SubscriptionHandlers): Subscription {
    const hid = this.nextHid++;
    this.handlers.set(hid, { h, filter, narrow: false });
    this.port.post({ t: 'sub', hid, filter, opts });
    let open = true;
    return {
      id: `h${hid}`,
      close: () => {
        if (!open) return;
        open = false;
        this.handlers.delete(hid);
        for (const s of this.bound.values()) s.delete(hid);
        if (!this.closed) this.port.post({ t: 'unsub', hid });
      },
    };
  }

  clock(): ClockEstimate {
    return this.est;
  }

  /**
   * NetClient.setVisible + optional ViewerPresence fields (rendered_through_seq,
   * oldest_pending_age_ms, channel). The core keeps the latest and re-sends them after
   * reconnect. (api.ts signature change requested from the lead — report p1-m1-accept §6.)
   */
  setVisible(visible: boolean, extra?: PresenceExtra): void {
    if (!this.closed) this.port.post(extra ? { t: 'visible', visible, extra } : { t: 'visible', visible });
  }

  /** WS-down STOP fallback. Never retried automatically. */
  async stopViaHttp(): Promise<void> {
    const f = this.o.fetch ?? globalThis.fetch.bind(globalThis);
    let r: Response;
    try {
      r = await f(this.o.stopUrl, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: stopHttpBody(this.o.clientId),
        credentials: 'same-origin',
      });
    } catch (e) {
      throw new NetError({ code: 'disconnected', message: `POST stop: ${String(e)}`, maybeSent: true });
    }
    if (!r.ok) throw new NetError({ code: 'rpc', rpcCode: r.status, message: `POST stop: HTTP ${r.status}`, maybeSent: true });
  }

  close(): void {
    if (this.closed) return;
    this.port.post({ t: 'close' });
    this.closed = true;
    // let the core flush its 'closed' state, then drop the worker
    setTimeout(() => this.port.terminate(), 0);
  }

  // ---------------------------------------------------------------------------

  private onCore(m: CoreOut): void {
    switch (m.t) {
      case 'state':
        this._state = m.state;
        for (const cb of [...this.stateCbs]) safe(() => cb(m.state));
        return;
      case 'control':
        for (const cb of [...this.controlCbs]) safe(() => cb(m.ev));
        return;
      case 'result': {
        const p = this.calls.get(m.id);
        if (!p) return; // aborted locally
        this.calls.delete(m.id);
        p.cleanup();
        if (m.ok) p.resolve(m.value);
        else p.reject(new NetError(m.error));
        return;
      }
      case 'clock':
        this.est = m.est;
        return;
      case 'deliver':
        for (const item of m.items) this.dispatch(item);
        if (!this.closed) this.o.frames.request(() => !this.closed && this.port.post({ t: 'pull' }));
        return;
    }
  }

  private dispatch(item: DeliveryItem): void {
    if (item.k === 'bind') {
      const hd = this.handlers.get(item.hid);
      if (!hd) return;
      hd.narrow = item.narrow;
      let s = this.bound.get(item.eid);
      if (!s) this.bound.set(item.eid, (s = new Set()));
      s.add(item.hid);
      return;
    }
    const targets = item.hid !== undefined ? [item.hid] : [...(this.bound.get(item.eid) ?? [])];
    for (const hid of targets) {
      const hd = this.handlers.get(hid);
      if (!hd) continue;
      const { h, filter, narrow } = hd;
      switch (item.k) {
        case 'part':
          safe(() => h.onSnapshotPart?.(narrow ? withPartEvents(item.part, partEvents(item.part).filter((e) => matchesFilter(filter, e))) : item.part));
          break;
        case 'end':
          safe(() => h.onSnapshotEnd?.(item.h));
          break;
        case 'batch': {
          // 2nd argument (receive/decode timing) — optional extension of SubscriptionHandlers.onBatch
          // (api.ts change requested from the lead); 1-argument handlers simply ignore it.
          const onBatch = h.onBatch.bind(h) as (b: EventsBatch, t?: DeliveryTiming) => void;
          const timing: DeliveryTiming | undefined = item.recvMs !== undefined ? { recv_ms: item.recvMs, decode_ms: item.decodeMs ?? 0 } : undefined;
          if (!narrow) {
            safe(() => onBatch(item.batch, timing));
            break;
          }
          // gaps of the shared stream stay visible to every narrow handle
          const evs = item.batch.events.filter((e) => matchesFilter(filter, e) || e.kind.endsWith('.gap'));
          if (evs.length || item.batch.display_gap) safe(() => onBatch(rebatch(item.batch, evs), timing));
          break;
        }
        case 'resync':
          safe(() => h.onResync?.(item.reason));
          break;
      }
    }
  }

  private fatal(reason: string): void {
    this._state = { status: 'closed', reason };
    for (const cb of [...this.stateCbs]) safe(() => cb(this._state));
    for (const p of this.calls.values()) {
      p.cleanup();
      p.reject(new NetError({ code: 'closed', message: reason, maybeSent: true }));
    }
    this.calls.clear();
  }
}

/** One throwing listener must not starve the others. */
function safe(fn: () => void): void {
  try {
    fn();
  } catch (e) {
    console.error('[loom-net] handler threw', e);
  }
}
