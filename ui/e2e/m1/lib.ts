// Shared helpers for the UI-M1 acceptance suite (e2e/m1.config.ts).
// - backend(): 'mock' | 'replay' from the Playwright project metadata (default mock, so the
//   specs also run under the lead's default config against :7420/:7421)
// - Daemon: a Node-side JSON-RPC client straight to the daemon (not through the page). On mock
//   it can act as the agent (mock.agent) and inject faults; on both it is the REFERENCE
//   subscriber that DOM results are compared with. Assertions are made on the DOM
//   (data-request-id / data-origin / data-seq / text) — never on WS receipt alone.
// e2e/ is typechecked with node libs only: page callbacks cast globalThis to the small `G` type.

import { expect, test, type Page } from '@playwright/test';
import WebSocket from 'ws';

export type Backend = 'mock' | 'replay';

export function backend(): Backend {
  return (test.info().project.metadata as { backend?: Backend } | undefined)?.backend ?? 'mock';
}

/** channel / target names per backend (mock: uart dut · can ch0 · uds bcm; replay: dut · ch0 · ecu1) */
export function names(b: Backend = backend()): { uart: string; can: string; uds: string } {
  return b === 'replay' ? { uart: 'dut', can: 'ch0', uds: 'ecu1' } : { uart: 'dut', can: 'ch0', uds: 'bcm' };
}

export const DAEMON_URL: Record<Backend, string> = {
  mock: process.env.LOOM_MOCK_URL ?? 'ws://localhost:7421/rpc',
  replay: process.env.LOOM_REPLAY_URL ?? 'ws://localhost:4790/rpc',
};
/** second / third replay daemons (e2e/m1.config.ts) */
export const REPLAY_WATCH_URL = 'ws://localhost:4791/rpc';
export const REPLAY_SATURATE_URL = 'ws://localhost:4792/rpc';

/** Minimal browser globals used inside page.evaluate (e2e has no DOM lib). */
export interface El {
  getAttribute(n: string): string | null;
  textContent: string | null;
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  querySelectorAll(sel: string): ArrayLike<El>;
  querySelector(sel: string): El | null;
  dispatchEvent(e: unknown): boolean;
  click(): void;
}
export interface NetProbe {
  state: { status: string; hello?: { env?: string; rights?: string; conn_id?: string; latch?: boolean } };
  clock(): { thetaNs: bigint; errMs: number; synced: boolean; clockId: string };
}
export interface G {
  document: El & { body: El };
  __loomNet?: NetProbe;
  requestAnimationFrame(cb: (t: number) => void): number;
  performance: { now(): number; timeOrigin: number };
  MutationObserver: new (cb: (m: unknown[]) => void) => { observe(n: El, o: Record<string, unknown>): void; disconnect(): void };
  setTimeout(cb: () => void, ms: number): number;
  __m1?: Record<string, unknown>;
}

/**
 * Page query for a backend. Replay: the page connects to loomd DIRECTLY (DEV hook
 * `?loom_daemon=`), because through the Vite proxy loomd answers 403 — the proxy forwards
 * `Host: localhost:<vite port>` and loomd only accepts its own Host (report §4 C-1; fix =
 * `changeOrigin: true` on the `/rpc` proxy in vite.config.ts, lead-owned).
 */
export function pageQuery(extra: Record<string, string> = {}, daemon?: string): string {
  const q = new URLSearchParams(extra);
  const d = daemon ?? (backend() === 'replay' ? DAEMON_URL.replay : undefined);
  if (d) q.set('loom_daemon', d);
  const s = q.toString();
  return s ? `?${s}` : '';
}

/** Open the app at `hash` (e.g. '/p/can/trace?channel=ch0'), wait until the net is open. */
export async function openApp(page: Page, hash: string, query = pageQuery()): Promise<string[]> {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(`./${query}#${hash}`);
  await waitOpen(page);
  return errors;
}

export async function waitOpen(page: Page, timeout = 15_000): Promise<void> {
  await page.waitForFunction(() => (globalThis as unknown as G).__loomNet?.state.status === 'open', null, { timeout });
}

export async function netStatus(page: Page): Promise<string> {
  return page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.status ?? 'none');
}

// ---------------------------------------------------------------------------------------------
// Node-side daemon client
// ---------------------------------------------------------------------------------------------

export interface Ev {
  seq: string;
  epoch: string;
  kind: string;
  domain: string;
  channel?: string;
  origin: string;
  request_id?: string;
  payload: Record<string, unknown>;
  ts: { mono_ns: string };
}

type Msg = { id?: number; method?: string; params?: Record<string, unknown>; result?: unknown; error?: { code: number; message: string; data?: Record<string, unknown> } };

export class RpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly data?: Record<string, unknown>,
  ) {
    super(message);
  }
}

/** Events of one reference subscription (C2: ≤ 200,000 kept, oldest dropped). */
export class RefSub {
  readonly events: Ev[] = [];
  h: bigint | null = null;
  dropped = 0;
  push(e: Ev): void {
    this.events.push(e);
    if (this.events.length > 200_000) {
      this.events.splice(0, 10_000);
      this.dropped += 10_000;
    }
  }
}

export class Daemon {
  private nextId = 1;
  private readonly pending = new Map<number, (m: Msg) => void>();
  private readonly subs = new Map<string, RefSub>();
  /** priority path events (sub "control"); C2: ≤ 20,000 kept */
  readonly control: Ev[] = [];
  hello: Record<string, unknown> = {};

  private constructor(private readonly ws: WebSocket) {
    ws.on('message', (d) => {
      const m = JSON.parse(d.toString()) as Msg;
      if (typeof m.id === 'number') {
        this.pending.get(m.id)?.(m);
        return;
      }
      const p = m.params ?? {};
      const sub = typeof p.sub === 'string' ? p.sub : '';
      const evs = (Array.isArray(p.events) ? p.events : []) as Ev[];
      if (m.method === 'core.events' && sub === 'control') {
        for (const e of evs) this.control.push(e);
        if (this.control.length > 20_000) this.control.splice(0, 1_000);
        return;
      }
      const s = this.subs.get(sub);
      if (!s) return;
      if (m.method === 'core.snapshot_part' || m.method === 'core.events') for (const e of evs) s.push(e);
      if (m.method === 'core.snapshot_end') s.h = BigInt(String(p.stream_seq));
    });
  }

  static async open(url = DAEMON_URL[backend()], clientId = 'e2e-ref'): Promise<Daemon> {
    const ws = new WebSocket(url);
    await new Promise((r, j) => ws.once('open', r).once('error', j));
    const d = new Daemon(ws);
    d.hello = await d.call<Record<string, unknown>>('core.hello', { proto: 'loom/1.0', client_id: clientId, want_role: 'viewer', client: { kind: 'web', version: 'e2e', tty: false } });
    return d;
  }

  call<R = unknown>(method: string, params: unknown = {}, timeoutMs = 15_000): Promise<R> {
    const id = this.nextId++;
    return new Promise<R>((resolve, reject) => {
      const t = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timeout`));
      }, timeoutMs);
      this.pending.set(id, (m) => {
        clearTimeout(t);
        this.pending.delete(id);
        if (m.error) reject(new RpcError(m.error.code, `${method}: ${m.error.message}`, m.error.data));
        else resolve(m.result as R);
      });
      this.ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }

  /** mock only: act as the agent (origin = agent) */
  agent<R = Record<string, unknown>>(method: string, params: unknown, timeoutMs?: number): Promise<R> {
    return this.call<R>('mock.agent', { method, params }, timeoutMs);
  }

  /** mock only: fault / load injection */
  inject<R = unknown>(kind: string, params: Record<string, unknown> = {}): Promise<R> {
    return this.call<R>('mock.inject', { kind, ...params });
  }

  async subscribe(filter: Record<string, unknown>, snapshot = true): Promise<RefSub> {
    const r = await this.call<{ sub_id: string }>('core.subscribe', { filter, snapshot, encoding: 'json', max_batch_ms: 10 });
    const s = new RefSub();
    this.subs.set(r.sub_id, s);
    return s;
  }

  close(): void {
    this.ws.terminate();
  }
}

// ---------------------------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------------------------

/**
 * Walk a virtualised grid top to bottom and collect `data-seq` of every row (rows without it —
 * gap rows — are skipped). Works for VirtualTable and the CAN trace view (fixed row heights).
 */
export async function scanGridSeqs(page: Page, scrollSel: string, rowSel: string): Promise<string[]> {
  return page.evaluate(
    async ({ scrollSel, rowSel }) => {
      const g = globalThis as unknown as G;
      const sc = g.document.querySelector(scrollSel);
      if (!sc) return [];
      const frame = () => new Promise<void>((r) => g.requestAnimationFrame(() => g.requestAnimationFrame(() => r())));
      const seen = new Map<string, number>();
      const order: string[] = [];
      sc.scrollTop = 0;
      sc.dispatchEvent(new (globalThis as unknown as { Event: new (t: string) => unknown }).Event('scroll'));
      await frame();
      for (let guard = 0; guard < 5_000; guard++) {
        const rows = sc.querySelectorAll(rowSel);
        for (let i = 0; i < rows.length; i++) {
          const s = rows[i]!.getAttribute('data-seq');
          if (s === null) continue;
          if (!seen.has(s)) order.push(s);
          seen.set(s, (seen.get(s) ?? 0) + 1);
        }
        if (sc.scrollTop + sc.clientHeight >= sc.scrollHeight - 1) break;
        sc.scrollTop = sc.scrollTop + Math.max(18, sc.clientHeight - 36);
        sc.dispatchEvent(new (globalThis as unknown as { Event: new (t: string) => unknown }).Event('scroll'));
        await frame();
      }
      return order;
    },
    { scrollSel, rowSel },
  );
}

/** seqs strictly increasing (no duplicate, no reordering) */
export function expectIncreasing(seqs: string[]): void {
  for (let i = 1; i < seqs.length; i++) {
    if (!(BigInt(seqs[i]!) > BigInt(seqs[i - 1]!))) throw new Error(`seq order broken at ${i}: ${seqs[i - 1]} → ${seqs[i]}`);
  }
}

/** every reference seq inside [lo, hi] of the DOM list is present in the DOM list */
export function missingInRange(dom: string[], ref: string[]): string[] {
  if (!dom.length) return ref;
  const lo = BigInt(dom[0]!);
  const hi = BigInt(dom[dom.length - 1]!);
  const have = new Set(dom);
  return ref.filter((s) => BigInt(s) >= lo && BigInt(s) <= hi && !have.has(s));
}

export async function poll<T>(fn: () => Promise<T> | T, ok: (v: T) => boolean, timeoutMs = 10_000, stepMs = 50): Promise<T> {
  const end = Date.now() + timeoutMs;
  let v = await fn();
  while (!ok(v)) {
    if (Date.now() > end) throw new Error(`poll timeout (last value ${JSON.stringify(v, (_k, x: unknown) => (typeof x === 'bigint' ? String(x) : x))})`);
    await new Promise((r) => setTimeout(r, stepMs));
    v = await fn();
  }
  return v;
}

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** mock-only test guard */
export function mockOnly(why = 'needs mock.agent / mock.inject (no agent or fault injection in fixture replay)'): void {
  test.skip(backend() !== 'mock', why);
}

export { expect, test };
