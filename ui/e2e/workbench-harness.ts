// UI-workbench QA control harness (ui-qa owned). Starts ONE own loomd (fresh HOME, sim env,
// operate rights) against an explicit UI dir so baseline/final builds can be compared while
// other workers rebuild ui/dist. Never prints tokens or bootstrap URLs. Control file is 0600,
// removed on exit; the fresh HOME is removed on close (unlike real-smoke-server.ts).
//
// usage: node --import tsx e2e/workbench-harness.ts <ctl.json> <ui-dir> <loomd-bin>
import http from 'node:http';
import { spawn, type ChildProcess } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtemp, readFile, rm, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import WebSocket from 'ws';

const [CTL, UI_DIR, LOOMD] = process.argv.slice(2) as [string, string, string];
if (!CTL || !UI_DIR || !LOOMD) throw new Error('usage: workbench-harness.ts <ctl> <ui-dir> <loomd>');
const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const secret = randomBytes(24).toString('hex');

class Daemon {
  child: ChildProcess | null = null;
  ws: WebSocket | null = null;
  home = '';
  token = '';
  origin = '';
  id = 0;

  async start() {
    this.home = await mkdtemp(path.join(tmpdir(), 'loom-ui-workbench-'));
    this.child = spawn(LOOMD, ['--port', '0', '--env', 'sim', '--rights', 'operate', '--ui-dir', UI_DIR], {
      env: { ...process.env, LOOM_HOME: this.home },
      stdio: 'ignore',
    });
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline && !this.origin) {
      if (this.child.exitCode !== null) throw new Error('own daemon exited');
      try {
        const info = JSON.parse(await readFile(path.join(this.home, 'run/daemon.json'), 'utf8')) as { port: number; token: string };
        this.token = info.token;
        this.origin = `http://localhost:${info.port}`;
      } catch {
        await delay(25);
      }
    }
    if (!this.origin) throw new Error('own daemon startup timeout');
    this.ws = new WebSocket(this.origin.replace('http:', 'ws:') + '/rpc');
    await new Promise<void>((res, rej) => {
      this.ws!.once('open', () => res());
      this.ws!.once('error', rej);
      setTimeout(() => rej(new Error('ws open timeout')), 10_000);
    });
    const hello = await this.call('core.hello', {
      proto: 'loom/1.0', token: this.token, client_id: 'ui-workbench-qa', want_role: 'agent',
      client: { kind: 'test', version: '0', tty: false },
    });
    if (hello.role !== 'agent') throw new Error('agent auth failed');
  }

  call(method: string, params: object): Promise<Record<string, unknown>> {
    const id = ++this.id;
    const ws = this.ws!;
    return new Promise((resolve, reject) => {
      const read = (data: WebSocket.RawData) => {
        const reply = JSON.parse(String(data)) as { id?: number; error?: unknown; result?: Record<string, unknown> };
        if (reply.id !== id) return;
        clearTimeout(timer);
        ws.off('message', read);
        if (reply.error) resolve({ error: reply.error });
        else resolve(reply.result ?? {});
      };
      const timer = setTimeout(() => { ws.off('message', read); reject(new Error(`${method} timeout`)); }, 10_000);
      ws.on('message', read);
      ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }

  async loginURL(route: string) {
    const r = await fetch(`${this.origin}/api/v1/bootstrap/new?rights=operate&next=${encodeURIComponent(route)}`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
    if (!r.ok) throw new Error(`bootstrap ${r.status}`);
    return ((await r.json()) as { url: string }).url;
  }

  async close() {
    const child = this.child;
    if (!child) return;
    const ended = new Promise<void>((res) => {
      if (child.exitCode !== null || child.signalCode !== null) res();
      else child.once('exit', () => res());
    });
    try {
      if (this.ws?.readyState === WebSocket.OPEN) await this.call('core.shutdown', {}).catch(() => ({}));
    } finally {
      this.ws?.close();
      await Promise.race([ended, delay(5_000)]);
      if (child.exitCode === null && child.signalCode === null) {
        child.kill('SIGTERM');
        await Promise.race([ended, delay(5_000)]);
      }
    }
    if (child.exitCode === null && child.signalCode === null) throw new Error('daemon cleanup timeout; HOME preserved');
    await rm(this.home, { recursive: true, force: true });
  }
}

let d: Daemon | null = null;
async function handle(route: string, body: Record<string, unknown>): Promise<unknown> {
  switch (route) {
    case '/start': if (d) throw new Error('already running'); d = new Daemon(); await d.start(); return { ok: true };
    case '/login': return { url: await d!.loginURL(String(body.route)) };
    case '/call': return { result: await d!.call(String(body.method), (body.params ?? {}) as object) };
    case '/close': { const x = d; d = null; await x?.close(); return { ok: true }; }
    default: throw new Error('unknown route');
  }
}

const server = http.createServer((req, res) => {
  if (req.headers['x-ctl'] !== secret) { res.writeHead(403).end(); return; }
  let raw = '';
  req.on('data', (c) => { raw += c; });
  req.on('end', async () => {
    const route = req.url ?? '';
    try {
      const out = await handle(route, raw ? JSON.parse(raw) : {});
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(out));
      console.log(`ok ${route}`);
    } catch (e) {
      res.writeHead(500, { 'content-type': 'application/json' }).end(JSON.stringify({ error: String((e as Error).message) }));
      console.log(`fail ${route}: ${(e as Error).message}`);
    }
  });
});
server.listen(0, '127.0.0.1', async () => {
  const port = (server.address() as { port: number }).port;
  await writeFile(CTL, JSON.stringify({ port, secret }), { mode: 0o600 });
  console.log('harness listening (control file written)');
});
async function shutdown() {
  try { await d?.close(); } catch (e) { console.log(`close: ${(e as Error).message}`); }
  await unlink(CTL).catch(() => {});
  server.close();
  process.exit(0);
}
process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);
