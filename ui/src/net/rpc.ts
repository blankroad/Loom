// RpcClient — JSON-RPC id matching over an injected `send`. Pure (no socket,
// no DOM): the core wires it to the WebSocket, tests to a fake.
//
// Rules (plan §3.4, p0-sol):
// - request frame > 64 KiB  -> rejected locally, never sent
// - every call has a timeout (default 10 s)
// - NOTHING is retried here. A call in flight when the socket drops is rejected
//   with `disconnected` (`maybeSent: true` — for a mutation the outcome is
//   unknown; the UI follows the request_id events instead of re-sending).
// - calls while the socket is not open are rejected (`not_connected`), not
//   queued: a queued STOP/keystroke replayed after reconnect would be a hidden
//   auto-retry.

import type { RpcError } from '../proto/provisional';
import { MAX_FRAME_BYTES, encodeRequest, rpcErrorFields, utf8Bytes } from './wire';

export type NetErrorCode =
  | 'not_connected'
  | 'disconnected'
  | 'timeout'
  | 'request_too_large'
  | 'too_many_in_flight'
  | 'aborted'
  | 'closed'
  | 'rpc';

/** Serializable form (crosses the worker boundary). */
export interface NetErrorShape {
  code: NetErrorCode;
  message: string;
  /** JSON-RPC error.code when code === 'rpc' */
  rpcCode?: number;
  /** error.data.code (e.g. 'rights', 'stop_latched') */
  dataCode?: string | null;
  data?: unknown;
  /** the request may have reached the daemon (disconnect/timeout after send) */
  maybeSent: boolean;
}

export class NetError extends Error implements NetErrorShape {
  readonly code: NetErrorCode;
  readonly rpcCode?: number;
  readonly dataCode?: string | null;
  readonly data?: unknown;
  readonly maybeSent: boolean;
  constructor(s: NetErrorShape) {
    super(s.message);
    this.name = 'NetError';
    this.code = s.code;
    if (s.rpcCode !== undefined) this.rpcCode = s.rpcCode;
    if (s.dataCode !== undefined) this.dataCode = s.dataCode;
    if (s.data !== undefined) this.data = s.data;
    this.maybeSent = s.maybeSent;
  }
  toShape(): NetErrorShape {
    const s: NetErrorShape = { code: this.code, message: this.message, maybeSent: this.maybeSent };
    if (this.rpcCode !== undefined) s.rpcCode = this.rpcCode;
    if (this.dataCode !== undefined) s.dataCode = this.dataCode;
    if (this.data !== undefined) s.data = this.data;
    return s;
  }
}

export interface Timers {
  setTimeout(cb: () => void, ms: number): unknown;
  clearTimeout(h: unknown): void;
}

export interface RpcClientOptions {
  /** returns false when the socket is not open */
  send(text: string): boolean;
  timers: Timers;
  defaultTimeoutMs?: number;
  /**
   * C2 bound on in-flight calls. Overflow policy: the NEW call is rejected
   * immediately with `too_many_in_flight` (existing calls are untouched).
   */
  maxInFlight?: number;
  maxRequestBytes?: number;
}

interface Pending {
  method: string;
  resolve(v: unknown): void;
  reject(e: NetError): void;
  timer: unknown;
}

export class RpcClient {
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly o: Required<RpcClientOptions>;

  constructor(o: RpcClientOptions) {
    this.o = { defaultTimeoutMs: 10_000, maxInFlight: 256, maxRequestBytes: MAX_FRAME_BYTES, ...o };
  }

  get inFlight(): number {
    return this.pending.size;
  }

  /** Returns the request id (for cancel) and the result promise. */
  start(method: string, params?: unknown, timeoutMs?: number): { id: number; done: Promise<unknown> } {
    const id = this.nextId++;
    const done = new Promise<unknown>((resolve, reject) => {
      if (this.pending.size >= this.o.maxInFlight) {
        reject(new NetError({ code: 'too_many_in_flight', message: `${method}: ${this.pending.size} calls in flight`, maybeSent: false }));
        return;
      }
      const text = encodeRequest(id, method, params);
      const bytes = utf8Bytes(text, this.o.maxRequestBytes);
      if (bytes > this.o.maxRequestBytes) {
        reject(new NetError({ code: 'request_too_large', message: `${method}: request ${bytes} B > ${this.o.maxRequestBytes} B`, maybeSent: false }));
        return;
      }
      if (!this.o.send(text)) {
        reject(new NetError({ code: 'not_connected', message: `${method}: not connected`, maybeSent: false }));
        return;
      }
      const ms = timeoutMs ?? this.o.defaultTimeoutMs;
      const timer = this.o.timers.setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new NetError({ code: 'timeout', message: `${method}: no response in ${ms} ms`, maybeSent: true }));
        }
      }, ms);
      this.pending.set(id, { method, resolve, reject, timer });
    });
    return { id, done };
  }

  call(method: string, params?: unknown, timeoutMs?: number): Promise<unknown> {
    return this.start(method, params, timeoutMs).done;
  }

  /** Response frame. Late responses (after timeout/cancel) are ignored. */
  handleResponse(id: number, result: unknown, error: RpcError | null): boolean {
    const p = this.pending.get(id);
    if (!p) return false;
    this.pending.delete(id);
    this.o.timers.clearTimeout(p.timer);
    if (error) {
      const f = rpcErrorFields(error);
      p.reject(new NetError({ code: 'rpc', message: `${p.method}: ${f.message}`, rpcCode: f.code, dataCode: f.dataCode, data: f.data, maybeSent: true }));
    } else {
      p.resolve(result);
    }
    return true;
  }

  /** Caller gave up (AbortSignal). There is no wire cancel in JSON-RPC; the response is dropped. */
  cancel(id: number): void {
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    this.o.timers.clearTimeout(p.timer);
    p.reject(new NetError({ code: 'aborted', message: `${p.method}: aborted`, maybeSent: true }));
  }

  /** Socket closed: every call in flight fails explicitly. Nothing is re-sent. */
  failAll(code: 'disconnected' | 'closed', reason: string): void {
    const all = [...this.pending.values()];
    this.pending.clear();
    for (const p of all) {
      this.o.timers.clearTimeout(p.timer);
      p.reject(new NetError({ code, message: `${p.method}: ${reason}`, maybeSent: true }));
    }
  }
}
