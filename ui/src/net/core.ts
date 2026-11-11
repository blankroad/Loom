// NetCore — owns the WebSocket, hello, reconnect, RPC, subscriptions, decode
// budget and clock. Runs inside net.worker.ts in the browser, inline in tests.
// Talks to the main thread only through CoreIn/CoreOut messages.

import type { LoomEvent } from '../proto/provisional';
import type { ConnState } from './api';
import { DEFAULT_LIMITS, DeliveryQueue, type BudgetLimits } from './budget';
import { ClockSync } from './clock';
import { SubscriptionMux, type MuxLimits } from './mux';
import type { CoreIn, CoreOut } from './protocol';
import { NetError, RpcClient, type Timers } from './rpc';
import type { LocalCtx } from './wire';
import { HELLO_METHOD, eventKey, heartbeatEvent, helloEpoch, helloParams, parseInbound, presenceParams, readHello, readTime, type PresenceExtra } from './wire';

export interface SocketLike {
  send(text: string): void;
  close(code?: number, reason?: string): void;
}

export interface SocketEvents {
  onOpen(): void;
  onText(text: string): void;
  /** binary frames: `loom.columns.v1` is M2; counted and ignored in M1 */
  onBinary?(): void;
  onClose(reason: string): void;
}

export interface CoreDeps {
  openSocket(url: string, ev: SocketEvents): SocketLike;
  /** ms on the epoch timeline: performance.timeOrigin + performance.now() */
  now(): number;
  timers: Timers;
  budget?: Partial<BudgetLimits>;
  mux?: Partial<MuxLimits>;
  /**
   * After a WS handshake fails (socket closed before it ever opened) the browser cannot see the
   * HTTP status, so the core asks: plain HTTP GET of the same URL → status (null = unknown).
   * 401/403 → `closed{reason:'unauthorized'}` and no more reconnects (shell → #/need-auth).
   * Optional: without it the core keeps the normal reconnect backoff.
   */
  probeAuth?(wsUrl: string): Promise<number | null>;
}

/** HTTP statuses of a refused upgrade that mean "no viewer credential" */
const AUTH_STATUSES = new Set([401, 403]);

/** Reconnect backoff: 0.5, 1, 2, 4, 8, 8, ... s */
export function backoffMs(attempt: number): number {
  return Math.min(8_000, 500 * 2 ** Math.max(0, attempt - 1));
}

const CLOCK_BURST = 8;
const CLOCK_PERIOD_MS = 5_000;
const HELLO_TIMEOUT_MS = 5_000;
const TIME_TIMEOUT_MS = 2_000;
/** methods the core owns; main must use subscribe() */
const RESERVED = new Set([HELLO_METHOD, 'core.subscribe', 'core.unsubscribe']);

/**
 * Control-path dedup: the same control event can arrive on the control sub and
 * on data subs. Bounded FIFO of keys; overflow evicts the oldest key (worst
 * case a duplicate reaches onControl — stores apply events idempotently).
 */
class Dedup {
  private readonly set = new Set<string>();
  private readonly ring: string[] = [];
  constructor(private readonly cap = 4096) {}
  firstTime(key: string): boolean {
    if (this.set.has(key)) return false;
    this.set.add(key);
    this.ring.push(key);
    if (this.ring.length > this.cap) this.set.delete(this.ring.shift()!);
    return true;
  }
  clear(): void {
    this.set.clear();
    this.ring.length = 0;
  }
}

export class NetCore {
  private sock: SocketLike | null = null;
  private socketOpen = false;
  /** bumps whenever a socket is abandoned; late callbacks from old sockets are ignored */
  private sockGen = 0;
  private userClosed = false;
  private attempt = 0;
  /** consecutive failed connections since the last successful hello (drives the backoff) */
  private failures = 0;
  private reconnectTimer: unknown;
  private clockTimer: unknown;
  private clockGen = 0;
  private visible = true;
  private presenceExtra: PresenceExtra | null = null;
  private lastEpoch: string | null = null;
  private state: ConnState = { status: 'connecting', attempt: 1 };
  private ctx: LocalCtx;
  private pullWaiting = false;
  private readonly rpc: RpcClient;
  private readonly queue: DeliveryQueue;
  private readonly mux: SubscriptionMux;
  private readonly clock = new ClockSync();
  private readonly dedup = new Dedup();
  /** main call id -> rpc id (bounded by RpcClient.maxInFlight; entries removed on settle) */
  private readonly calls = new Map<number, number>();
  readonly stats = { invalidFrames: 0, binaryFrames: 0, otherNotifications: 0 };

  constructor(
    private readonly url: string,
    private readonly clientId: string,
    private readonly deps: CoreDeps,
    private readonly post: (m: CoreOut) => void,
  ) {
    this.ctx = { sessionId: '', epoch: '0', clientId };
    this.rpc = new RpcClient({
      send: (text) => {
        if (!this.socketOpen || !this.sock) return false;
        this.sock.send(text);
        return true;
      },
      timers: deps.timers,
    });
    this.queue = new DeliveryQueue((eid) => this.mux.resync(eid, 'client_queue_overflow'), { ...DEFAULT_LIMITS, ...deps.budget });
    this.mux = new SubscriptionMux({
      call: (m, p) => this.rpc.call(m, p),
      queue: this.queue,
      control: (ev) => this.control(ev, true),
      ctx: () => this.ctx,
      timers: deps.timers,
      ...(deps.mux ? { limits: deps.mux } : {}),
    });
  }

  start(): void {
    this.connect();
  }

  handle(m: CoreIn): void {
    switch (m.t) {
      case 'init':
        return; // handled by the worker shell
      case 'call':
        this.onCall(m.id, m.method, m.params, m.timeoutMs);
        return;
      case 'cancel': {
        const rid = this.calls.get(m.id);
        if (rid !== undefined) this.rpc.cancel(rid);
        return;
      }
      case 'sub':
        this.mux.subscribe(m.hid, m.filter, m.opts);
        this.maybeDeliver();
        return;
      case 'unsub':
        this.mux.unsubscribe(m.hid);
        return;
      case 'pull':
        this.pullWaiting = true;
        this.maybeDeliver();
        return;
      case 'visible':
        this.setVisible(m.visible, m.extra);
        return;
      case 'close':
        this.close();
        return;
    }
  }

  // ---- connection ---------------------------------------------------------

  private setState(s: ConnState): void {
    this.state = s;
    this.post({ t: 'state', state: s });
  }

  private connect(): void {
    this.attempt++;
    this.setState({ status: 'connecting', attempt: this.attempt });
    this.socketOpen = false;
    const gen = ++this.sockGen;
    try {
      this.sock = this.deps.openSocket(this.url, {
        onOpen: () => gen === this.sockGen && this.onSocketOpen(),
        onText: (t) => gen === this.sockGen && this.onText(t),
        onBinary: () => void this.stats.binaryFrames++,
        onClose: (r) => gen === this.sockGen && this.onSocketClose(r),
      });
    } catch (e) {
      this.onSocketClose(`open_failed:${String(e)}`);
    }
  }

  private onSocketOpen(): void {
    this.socketOpen = true;
    const sock = this.sock;
    this.rpc
      .call(HELLO_METHOD, helloParams(this.clientId), HELLO_TIMEOUT_MS)
      .then((raw) => {
        if (this.sock !== sock || !this.socketOpen) return;
        const hello = readHello(raw);
        if (!hello) throw new Error('malformed hello result');
        const epoch = helloEpoch(hello);
        if (this.lastEpoch !== null && this.lastEpoch !== epoch) {
          this.clock.reset(); // epoch change discards the mapping
          this.dedup.clear();
        }
        this.lastEpoch = epoch;
        // hello carries no session_id in contract v1; local rows take it from the stream later
        this.ctx = { sessionId: this.ctx.sessionId, epoch, clientId: this.clientId };
        this.attempt = 0;
        this.failures = 0;
        this.setState({ status: 'open', hello });
        this.mux.onOpen();
        // the latest presence (visibility + render fields) is re-sent on every new connection
        if (!this.visible || this.presenceExtra) this.rpc.call('core.viewer.presence', presenceParams(this.visible, this.presenceExtra)).catch(() => undefined);
        this.startClock(CLOCK_BURST);
      })
      .catch((e: unknown) => {
        if (this.sock !== sock) return;
        sock?.close(4000, `hello_failed`);
        this.onSocketClose(`hello_failed:${e instanceof Error ? e.message : String(e)}`);
      });
  }

  private onSocketClose(reason: string): void {
    if (!this.sock && !this.socketOpen && this.state.status !== 'connecting') return;
    const neverOpened = !this.socketOpen;
    this.sockGen++; // anything still arriving on this socket is stale
    this.sock = null;
    this.socketOpen = false;
    this.clockGen++;
    this.deps.timers.clearTimeout(this.clockTimer);
    this.rpc.failAll(this.userClosed ? 'closed' : 'disconnected', reason);
    this.mux.onDisconnect();
    if (this.userClosed) return;
    this.failures++;
    const retryInMs = backoffMs(this.failures);
    this.setState({ status: 'reconnecting', attempt: this.attempt, retryInMs, reason });
    this.deps.timers.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = this.deps.timers.setTimeout(() => this.connect(), retryInMs);
    if (neverOpened && this.deps.probeAuth) this.checkAuth();
  }

  /** one probe per failed handshake (bounded by the reconnect backoff) */
  private checkAuth(): void {
    const gen = this.sockGen;
    this.deps.probeAuth!(this.url).then(
      (status) => {
        if (status === null || !AUTH_STATUSES.has(status)) return;
        // still not connected (a later attempt may have succeeded meanwhile)
        if (this.userClosed || this.state.status === 'open' || (this.sockGen !== gen && this.socketOpen)) return;
        this.userClosed = true; // no further reconnects: a reload after `loom ui` starts over
        this.deps.timers.clearTimeout(this.reconnectTimer);
        const s = this.sock;
        this.sockGen++;
        this.sock = null;
        this.socketOpen = false;
        s?.close(1000, 'unauthorized');
        this.rpc.failAll('closed', 'unauthorized');
        this.mux.onDisconnect();
        this.setState({ status: 'closed', reason: 'unauthorized' });
      },
      () => undefined,
    );
  }

  private close(): void {
    if (this.userClosed) return;
    this.userClosed = true;
    this.deps.timers.clearTimeout(this.reconnectTimer);
    const s = this.sock;
    this.onSocketCloseFinal();
    s?.close(1000, 'client_closed');
  }

  private onSocketCloseFinal(): void {
    this.sockGen++;
    this.sock = null;
    this.socketOpen = false;
    this.clockGen++;
    this.deps.timers.clearTimeout(this.clockTimer);
    this.rpc.failAll('closed', 'client closed');
    this.mux.onDisconnect();
    this.setState({ status: 'closed', reason: 'client_closed' });
  }

  // ---- inbound ------------------------------------------------------------

  private onText(text: string): void {
    const recvMs = this.deps.now();
    const m = parseInbound(text);
    if (m.t === 'events') {
      m.recvMs = recvMs;
      m.decodeMs = Math.max(0, this.deps.now() - recvMs);
    }
    switch (m.t) {
      case 'response':
        this.rpc.handleResponse(m.id, m.result, m.error);
        break;
      case 'part':
      case 'end':
      case 'events':
        this.mux.onNotification(m);
        break;
      case 'control':
        // the connection's automatic priority subscription (contract 01 §1.5)
        for (const ev of m.events) {
          if (!this.ctx.sessionId) this.ctx = { ...this.ctx, sessionId: ev.session_id };
          this.control(ev, true);
        }
        break;
      case 'heartbeat':
        this.control(heartbeatEvent(this.ctx, m.hb), false);
        break;
      case 'closed': {
        // e.g. slow_consumer: the server drops us; reconnect + resume/snapshot
        const s = this.sock;
        this.onSocketClose(`server_closed:${m.reason}`);
        s?.close(1000, 'server_closed');
        break;
      }
      case 'other':
        this.stats.otherNotifications++;
        break;
      case 'invalid':
        this.stats.invalidFrames++;
        break;
    }
    this.maybeDeliver();
  }

  private control(ev: LoomEvent, dedup: boolean): void {
    if (dedup && !this.dedup.firstTime(eventKey(ev))) return;
    this.post({ t: 'control', ev });
  }

  private maybeDeliver(): void {
    if (!this.pullWaiting || this.queue.size === 0) return;
    this.pullWaiting = false;
    const d = this.queue.drain();
    for (const g of d.gaps) this.post({ t: 'control', ev: g });
    this.post({ t: 'deliver', items: d.items });
  }

  // ---- calls ----------------------------------------------------------------

  private onCall(id: number, method: string, params: unknown, timeoutMs: number | undefined): void {
    const fail = (e: NetError) => this.post({ t: 'result', id, ok: false, error: e.toShape() });
    if (RESERVED.has(method)) {
      fail(new NetError({ code: 'rpc', rpcCode: -32601, message: `${method} is owned by the net layer (use subscribe())`, maybeSent: false }));
      return;
    }
    if (this.state.status !== 'open') {
      fail(new NetError({ code: 'not_connected', message: `${method}: ${this.state.status}`, maybeSent: false }));
      return;
    }
    const { id: rid, done } = this.rpc.start(method, params, timeoutMs);
    this.calls.set(id, rid);
    done.then(
      (value) => {
        this.calls.delete(id);
        this.post({ t: 'result', id, ok: true, value });
      },
      (e: unknown) => {
        this.calls.delete(id);
        fail(e instanceof NetError ? e : new NetError({ code: 'rpc', message: String(e), maybeSent: true }));
      },
    );
  }

  // ---- presence & clock -----------------------------------------------------

  /**
   * Visibility (+ optional render fields: rendered_through_seq, oldest_pending_age_ms, channel).
   * The latest values are kept (one object, replaced) and re-sent after a reconnect, so the
   * shell needs no second presence RPC. Sent when visibility changes or extra fields are given.
   */
  private setVisible(v: boolean, extra?: PresenceExtra): void {
    const changed = v !== this.visible;
    if (!changed && !extra) return;
    this.visible = v;
    if (extra) this.presenceExtra = presenceParams(v, extra);
    if (this.state.status !== 'open') return; // sent after the next hello
    this.rpc.call('core.viewer.presence', presenceParams(v, this.presenceExtra)).catch(() => undefined);
    if (v && changed) this.startClock(CLOCK_BURST); // re-measure on visibility return
  }

  private startClock(burst: number): void {
    const gen = ++this.clockGen;
    this.deps.timers.clearTimeout(this.clockTimer);
    const loop = async () => {
      for (let i = 0; i < burst && gen === this.clockGen; i++) await this.sampleClock();
      const tick = () => {
        if (gen !== this.clockGen) return;
        void this.sampleClock().finally(() => {
          if (gen === this.clockGen) this.clockTimer = this.deps.timers.setTimeout(tick, CLOCK_PERIOD_MS);
        });
      };
      if (gen === this.clockGen) this.clockTimer = this.deps.timers.setTimeout(tick, CLOCK_PERIOD_MS);
    };
    void loop();
  }

  private async sampleClock(): Promise<void> {
    const b0 = this.deps.now();
    let raw: unknown;
    try {
      raw = await this.rpc.call('core.time', {}, TIME_TIMEOUT_MS);
    } catch {
      return;
    }
    const b3 = this.deps.now();
    const t = readTime(raw);
    if (!t) return;
    if (this.clock.add({ b0Ms: b0, b3Ms: b3, d1Ns: t.d1, d2Ns: t.d2, clockId: t.clockId })) {
      this.post({ t: 'clock', est: this.clock.estimate() });
    }
  }
}

