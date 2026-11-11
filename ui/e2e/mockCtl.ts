// Node-side control connection to the mock daemon for E2E (mock.agent / mock.inject).
// Uses the mock port directly (not the Vite proxy) so the browser's own connection is untouched.

import WebSocket from 'ws';

export const MOCK_URL = process.env.LOOM_MOCK_URL ?? 'ws://localhost:7421/rpc';

export class MockCtl {
  private nextId = 1;
  private pending = new Map<number, (m: { result?: unknown; error?: { message: string } }) => void>();
  private constructor(private readonly ws: WebSocket) {
    ws.on('message', (d) => {
      const m = JSON.parse(d.toString()) as { id?: number; result?: unknown; error?: { message: string } };
      if (typeof m.id === 'number') this.pending.get(m.id)?.(m);
    });
  }

  static async open(url = MOCK_URL): Promise<MockCtl> {
    const ws = new WebSocket(url);
    await new Promise((r, j) => ws.once('open', r).once('error', j));
    const c = new MockCtl(ws);
    await c.call('core.hello', { proto: 'loom/1.0', client_id: 'e2e-ctl', want_role: 'viewer', client: { kind: 'e2e', version: '0', tty: false } });
    return c;
  }

  call<R = unknown>(method: string, params: unknown = {}, timeoutMs = 10_000): Promise<R> {
    const id = this.nextId++;
    return new Promise<R>((resolve, reject) => {
      const t = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timeout`));
      }, timeoutMs);
      this.pending.set(id, (m) => {
        clearTimeout(t);
        this.pending.delete(id);
        if (m.error) reject(new Error(`${method}: ${m.error.message}`));
        else resolve(m.result as R);
      });
      this.ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }

  /** act as the agent (origin=agent) */
  agent<R = unknown>(method: string, params: unknown): Promise<R> {
    return this.call<R>('mock.agent', { method, params });
  }

  close(): void {
    this.ws.terminate();
  }
}
