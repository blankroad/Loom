// Actual-domain memory soak. Browser rendering/paint is a separate mandatory P1 gate.
import { writeFile, rename, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import WebSocket from 'ws';
import { RealSmoke } from '../e2e/real-smoke-server';

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => typeof v === 'object' && v !== null ? v as Obj : {};
const num = (v: unknown): number | null => {
  if (typeof v !== 'number' && typeof v !== 'string') return null;
  const n = Number(v); return Number.isFinite(n) && n >= 0 ? n : null;
};
const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const minutes = Number(process.env.LOOM_SOAK_MINUTES ?? '30');
if (!(minutes > 0 && minutes <= 60)) throw new Error('LOOM_SOAK_MINUTES must be 0..60');
const out = path.resolve(process.env.LOOM_SOAK_OUT ?? '/private/tmp/loom-p1-soak.json');
const samples: { t: number; key: string; rss: number | null }[] = [];
const events: Record<string, number> = {};
const cacheSamples: { t: number; records: number | null; order_entries: number | null; json_bytes: number | null; cost_bytes: number | null }[] = [];
const errors: string[] = [];
let errorCount = 0;
const error = (message: string) => { errorCount++; if (errors.length < 64) errors.push(message.slice(0, 160)); };
const repo = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
const source = {
  head: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  diff_sha256: createHash('sha256').update(execFileSync('git', ['-C', repo, 'diff', '--binary', 'HEAD', '--', 'crates', 'ui', 'Cargo.lock'])).digest('hex'),
  binary_sha256: createHash('sha256').update(await readFile(process.env.LOOMD_BIN ?? '../target/debug/loomd')).digest('hex'),
};
const stats = { uart: 0, can_frames_requested: 0, uds: 0, eth_tx: 0, psu: 0, queries: 0, stop_cycles: 0, slow_disconnects: 0, slow_reconnects: 0 };
function checked(method: string, result: Obj): Obj {
  if (result.ok === false || result.error) throw new Error(`${method}: ${String(obj(result.error).code ?? 'error')}`);
  return result.data && typeof result.data === 'object' ? obj(result.data) : result;
}
class Viewer {
  ws: WebSocket | null = null;
  private next = 0;
  private generation = 0;
  private closing = false;
  private waiting = new Map<number, { resolve: (v: Obj) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  async start(server: RealSmoke, name: string) {
    const generation = ++this.generation;
    this.closing = false;
    const url = new URL(await server.loginURL('/'));
    const code = new URLSearchParams(url.hash.slice(1)).get('boot');
    const response = await fetch(`${server.origin}/api/v1/bootstrap`, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code }),
    });
    const cookie = response.headers.get('set-cookie')?.split(';')[0];
    if (response.status !== 204 || !cookie) throw new Error('viewer bootstrap failed');
    const ws = this.ws = new WebSocket(server.origin.replace('http:', 'ws:') + '/rpc', {
      headers: { Origin: server.origin, Cookie: cookie }, maxPayload: 8 * 1024 * 1024,
    });
    ws.on('message', (raw) => {
      if (generation !== this.generation) return;
      try {
        const v = obj(JSON.parse(String(raw)));
        const pending = this.waiting.get(Number(v.id));
        if (pending) {
          clearTimeout(pending.timer); this.waiting.delete(Number(v.id));
          if (v.error) pending.reject(new Error(`viewer RPC ${String(obj(obj(v.error).data).code ?? 'error')}`));
          else pending.resolve(obj(v.result));
        } else if (v.method === 'core.events') {
          const batch = obj(v.params).events;
          for (const rawEvent of Array.isArray(batch) ? batch : []) {
            const event = obj(rawEvent);
            const key = `${name}:${String(event.domain).slice(0, 20)}:${String(event.kind).slice(0, 40)}`;
            if (Object.hasOwn(events, key) || Object.keys(events).length < 96) events[key] = (events[key] ?? 0) + 1;
          }
        }
      } catch { error('viewer malformed frame'); }
    });
    ws.on('close', () => {
      if (generation !== this.generation) return;
      if (!this.closing) { if (name === 'soak-fast') error('normal viewer closed'); else stats.slow_disconnects++; }
      for (const p of this.waiting.values()) { clearTimeout(p.timer); p.reject(new Error('viewer closed')); }
      this.waiting.clear();
    });
    ws.on('error', () => error('viewer socket error'));
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('viewer open timeout')), 5000);
      ws.once('open', () => { clearTimeout(timer); resolve(); });
      ws.once('error', () => { clearTimeout(timer); reject(new Error('viewer open failed')); });
    });
    checked('core.hello', await this.call('core.hello', { proto: 'loom/1.0', client_id: name, want_role: 'viewer', client: { kind: 'bench', version: '1', tty: false } }));
    checked('core.subscribe', await this.call('core.subscribe', { filter: {}, snapshot: false, max_batch_ms: 10 }));
  }
  call(method: string, params: Obj): Promise<Obj> {
    if (this.waiting.size >= 16 || this.ws?.readyState !== WebSocket.OPEN) return Promise.reject(new Error('viewer unavailable/full'));
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.waiting.delete(id); reject(new Error('viewer reply timeout')); }, 5000);
      this.waiting.set(id, { resolve, reject, timer });
      this.ws!.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }
  close() { this.closing = true; this.ws?.resume(); this.ws?.close(); }
}
const median = (a: number[]) => { a.sort((x, y) => x - y); return a.length ? a[Math.floor(a.length / 2)]! : null; };
const analyse = (duration: number) => [...new Set(samples.map((s) => s.key))].map((key) => {
  const rows = samples.filter((s) => s.key === key && s.t >= 120);
  const good = rows.filter((s): s is typeof s & { rss: number } => s.rss !== null);
  const first = median(good.filter((s) => s.t < 420).map((s) => s.rss));
  const last = median(good.filter((s) => s.t >= duration - 300).map((s) => s.rss));
  const expected = Math.max(1, Math.floor(duration - 120));
  const coverage = Math.min(1, good.length / expected);
  const growthMiB = first !== null && last !== null ? (last - first) / 1024 ** 2 : null;
  const firstCoverage = good.filter((s) => s.t < 420).length / 300;
  const lastCoverage = good.filter((s) => s.t >= duration - 300).length / 300;
  const span = good.length ? good[good.length - 1]!.t - good[0]!.t : 0;
  const enough = duration >= 1800 && coverage >= .9 && firstCoverage >= .9 && lastCoverage >= .9 && span >= 600 && first !== null && last !== null;
  return { key, coverage, firstCoverage, lastCoverage, span, samples: good.length, first_median: first, last_median: last, growthMiB, verdict: !enough ? 'insufficient' : growthMiB! <= 10 ? 'pass' : 'fail' };
});

const server = new RealSmoke();
const fast = new Viewer();
const slow = new Viewer();
let duration = 0;
const call = async (method: string, params: Obj) => checked(method, await server.call(method, params));
async function save(final: boolean) {
  const analysis = analyse(duration);
  const report = { source, configuration: { minutes, profile: 'actual UART+CAN+UDS+Ethernet sims; two WS viewers; self RSS of daemon and every supervised worker', browser_rendering: false, rss_source: 'self sampling via status/worker health', process_tree: 'configured supervised Loom processes; no arbitrary external process enumeration' }, duration, stats, errorCount, errors, events, analysis, samples, cacheSamples, verdict: !final || duration < 1800 ? 'insufficient' : errorCount || !analysis.length || analysis.some((p) => p.verdict !== 'pass') ? 'fail' : 'pass' };
  await writeFile(out + '.partial.json', JSON.stringify(report));
  if (final) await rename(out + '.partial.json', out);
}
try {
  await server.start();
  await fast.start(server, 'soak-fast'); await slow.start(server, 'soak-slow');
  await call('uart.open', { target: 'sim://bootshell?split=0', alias: 'dut', profile: 'linux-shell' });
  await call('uart.expect', { channel: 'dut', patterns: [{ kind: 'literal', pattern: 'login: ' }], timeout_ms: 3000 });
  await call('uart.send', { channel: 'dut', data: Buffer.from('root').toString('hex'), eol: 'cr' });
  await call('uart.send', { channel: 'dut', data: Buffer.from('secret').toString('hex'), eol: 'cr', secret: true });
  await call('can.open', { channel: 'ch0' });
  await call('uds.connect', { name: 'ecu', sim: 'bench' });
  await call('eth.capture.start', { iface: 'sim0', ch: 'cap', backend: 'sim' });
  await call('psu.connect', { ch: 'bench', backend: 'sim' });
  const started = performance.now();
  let nextSample = 0, nextSlow = 2000, nextSecond = 0, nextMinute = 60_000;
  let paused = false;
  while (performance.now() - started < minutes * 60_000) {
    const tick = performance.now();
    duration = (tick - started) / 1000;
    try {
      await call('uart.send', { channel: 'dut', data: Buffer.from('echo p1').toString('hex'), eol: 'cr' }); stats.uart++;
      await call('can.send', { channel: 'ch0', frame: '300#01', count: 10 }); stats.can_frames_requested += 10;
      if (tick - started >= nextSecond) {
        await call('uds.request', { target: 'ecu', bytes_hex: '22F190' }); stats.uds++;
        await call('psu.meas', { ch: 'bench' }); stats.psu++;
        await call('eth.send', { ch: 'cap', kind: 'raw', hex: 'ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002' }); stats.eth_tx++;
        await call('core.events.query', { filter: { kinds: ['uds.rx'] }, max: 4, peek: true }); stats.queries++;
        nextSecond = tick - started + 1000;
      }
      if (tick - started >= nextSample) {
        const status = await call('core.status', {});
        const cache = obj(status.hub_cache);
        cacheSamples.push({ t: duration, records: num(cache.records), order_entries: num(cache.order_entries), json_bytes: num(cache.json_bytes), cost_bytes: num(cache.cost_bytes) });
        if (cacheSamples.length > 3601) throw new Error('cache sample cap exceeded');
        if (num(cache.records) !== null && cache.order_entries !== cache.records) throw new Error('hub eviction metadata does not match retained records');
        const ownRss = num(status.process_rss_bytes);
        samples.push({ t: duration, key: `daemon:${String(status.pid)}`, rss: ownRss && ownRss > 0 ? ownRss : null });
        if (!Array.isArray(status.workers) || status.workers.length > 32) throw new Error('invalid supervised worker set');
        for (const raw of Array.isArray(status.workers) ? status.workers : []) {
          const w = obj(raw);
          const rss = w.rss_available === true && (num(w.health_age_ms) ?? Infinity) <= 3000 && w.state === 'ready' ? num(w.rss_bytes) : null;
          samples.push({ t: duration, key: `${String(w.instance ?? w.kind)}:${String(w.pid)}:${String(w.producer_epoch)}`, rss: rss && rss > 0 ? rss : null });
        }
        if (samples.length > 60 * 60 * 33) throw new Error('RSS sample cap exceeded');
        nextSample = tick - started + 1000;
      }
      if (tick - started >= nextSlow) {
        if (slow.ws?.readyState === WebSocket.CLOSED) { await slow.start(server, 'soak-slow'); stats.slow_reconnects++; paused = false; }
        if (paused) slow.ws?.resume(); else slow.ws?.pause();
        paused = !paused; nextSlow = tick - started + 2000;
      }
      if (tick - started >= nextMinute) {
        slow.ws?.resume(); paused = false;
        checked('core.stop', await fast.call('core.stop', {}));
        const blocked = await server.call('uart.send', { channel: 'dut', data: '78', eol: 'cr' });
        if (obj(blocked.error).code !== 'E_STOPPED') throw new Error('STOP failed to block agent');
        checked('core.stop.release', await fast.call('core.stop.release', {}));
        stats.stop_cycles++; await save(false);
        console.log(JSON.stringify({ elapsed_s: Math.floor(duration), commands: stats, errors: errorCount }));
        nextMinute = tick - started + 60_000;
      }
    } catch (e) { error(e instanceof Error ? e.message : 'load error'); if (errorCount >= 16) throw new Error('load failure cap exceeded', { cause: e }); }
    await delay(Math.max(0, 100 - (performance.now() - tick)));
  }
  duration = (performance.now() - started) / 1000;
  for (const domain of ['uart', 'can', 'uds', 'eth', 'psu']) if (!Object.keys(events).some((k) => k.startsWith(`soak-fast:${domain}:`))) error(`no live ${domain} events`);
  await save(true);
  if (minutes >= 30 && (errorCount || analyse(duration).some((p) => p.verdict !== 'pass'))) process.exitCode = 1;
  console.log(JSON.stringify({ out, duration, verdict: analyse(duration).map((a) => ({ key: a.key, growthMiB: a.growthMiB, verdict: a.verdict })), errors: errorCount }));
} catch (e) {
  error(e instanceof Error ? e.message : 'fatal soak error'); await save(true); process.exitCode = 1;
} finally { fast.close(); slow.close(); await server.close(); }
