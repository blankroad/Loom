// Only owns a fresh HOME and child. No existing demo, mock/replay, token printing or STOP release.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export class RealSmoke {
  private child: ChildProcess | null = null;
  private ws: WebSocket | null = null;
  private token = '';
  private id = 0;
  origin = '';

  async start() {
    const home = await mkdtemp(path.join(tmpdir(), 'loom-browser-smoke-'));
    this.child = spawn(
      process.env.LOOMD_BIN ?? path.join(repo, 'target/debug/loomd'),
      ['--port', '0', '--env', 'sim', '--rights', 'operate', '--ui-dir', path.join(repo, 'ui/dist')],
      { env: { ...process.env, LOOM_HOME: home }, stdio: 'ignore' },
    );
    let failure: Error | null = null;
    this.child.on('error', (error) => { failure = error; });
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      if (failure) throw failure;
      if (this.child.exitCode !== null) throw new Error('own daemon exited');
      try {
        const info = JSON.parse(await readFile(path.join(home, 'run/daemon.json'), 'utf8')) as {
          port: number;
          token: string;
        };
        this.token = info.token;
        this.origin = `http://localhost:${info.port}`;
        break;
      } catch {
        await delay(25);
      }
    }
    if (!this.origin) throw new Error('own daemon startup timeout');
    this.ws = new WebSocket(this.origin.replace('http:', 'ws:') + '/rpc');
    await new Promise<void>((resolve, reject) => {
      const ws = this.ws!;
      const timer = setTimeout(() => {
        ws.off('open', opened);
        ws.off('error', failed);
        reject(new Error('agent WebSocket open timeout'));
      }, 10_000);
      const opened = () => {
        clearTimeout(timer);
        ws.off('error', failed);
        resolve();
      };
      const failed = (error: Error) => {
        clearTimeout(timer);
        ws.off('open', opened);
        reject(error);
      };
      ws.once('open', opened);
      ws.once('error', failed);
    });
    const hello = await this.call('core.hello', {
      proto: 'loom/1.0', token: this.token, client_id: 'real-browser-smoke', want_role: 'agent',
      client: { kind: 'test', version: '0', tty: false },
    });
    if (hello.role !== 'agent') throw new Error('agent auth failed');
  }

  call(method: string, params: object): Promise<Record<string, unknown>> {
    const id = ++this.id;
    const ws = this.ws!;
    return new Promise((resolve, reject) => {
      const read = (data: WebSocket.RawData) => {
        const reply = JSON.parse(String(data)) as {
          id?: number;
          error?: unknown;
          result?: Record<string, unknown>;
        };
        if (reply.id !== id) return;
        clearTimeout(timer);
        ws.off('message', read);
        if (reply.error) reject(new Error(`${method}: ${JSON.stringify(reply.error)}`));
        else resolve(reply.result ?? {});
      };
      const timer = setTimeout(() => {
        ws.off('message', read);
        reject(new Error(`${method} timeout`));
      }, 10_000);
      ws.on('message', read);
      ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }

  async loginURL(route: string) {
    const response = await fetch(
      `${this.origin}/api/v1/bootstrap/new?rights=operate&next=${encodeURIComponent(route)}`,
      { headers: { Authorization: `Bearer ${this.token}` } },
    );
    if (!response.ok) throw new Error(`bootstrap ${response.status}`);
    return (await response.json() as { url: string }).url;
  }

  async close() {
    const child = this.child;
    if (!child) return;
    const ended = new Promise<void>((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) resolve();
      else child.once('exit', () => resolve());
    });
    try {
      if (this.ws?.readyState === WebSocket.OPEN) await this.call('core.shutdown', {});
    } finally {
      this.ws?.close();
      // Allow the acknowledged shutdown to seal journals before falling back to SIGTERM.
      await Promise.race([ended, delay(5_000)]);
      if (child.exitCode === null && child.signalCode === null) {
        child.kill('SIGTERM');
        await Promise.race([ended, delay(5_000)]);
      }
    }
    if (child.exitCode === null && child.signalCode === null) {
      throw new Error('own daemon cleanup timeout; HOME preserved');
    }
  }
}

if (process.argv.includes('--preflight')) {
  const server = new RealSmoke();
  try {
    await server.start();
    if (!(await fetch(`${server.origin}/ui/`)).ok) throw new Error('UI not served');
    if (!(await server.loginURL('/p/uart/trace?channel=dut')).includes('#boot=')) {
      throw new Error('bootstrap URL absent');
    }
    const opened = await server.call('uart.open', {
      target: 'sim://bootshell?split=0', alias: 'dut', profile: 'linux-shell',
    });
    if (opened.ok !== true) throw new Error(`UART open ${JSON.stringify(opened)}`);
    console.log('PASS: real daemon, built UI served, official bootstrap issued, UART sim opened; browser not executed');
  } finally {
    await server.close();
  }
}
