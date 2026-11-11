// Records a short viewer-side wire transcript from CORE's `loomd --fixture-replay` into
// fixtures/wire/replay-*.jsonl (same line format as gen-fixtures.ts) so src/net can be checked
// against the real contract server's frames in unit tests (contract.test.ts), without loomd.
//   cargo run -p loom-gateway --bin loomd -- --fixture-replay builtin --scenario all --echo-writes --port 4790
//   pnpm exec tsx tools/mock/capture-replay.ts [ws://localhost:4790/rpc]
// Bounds: ≤ 400 frames per file, heartbeat frames kept (they are part of the contract), 3 s.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';

const OUT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures', 'wire');
const MAX_FRAMES = 400;

async function capture(url: string, name: string, note: string, script: (call: (m: string, p: unknown) => Promise<unknown>) => Promise<void>): Promise<void> {
  const ws = new WebSocket(url);
  await new Promise((r, j) => ws.once('open', r).once('error', j));
  const t0 = Date.now();
  const lines: string[] = [JSON.stringify({ fixture: name, source: 'loomd --fixture-replay builtin --scenario all --echo-writes', note })];
  const waiters = new Map<number, (v: unknown) => void>();
  ws.on('message', (d) => {
    const msg = JSON.parse(d.toString()) as { id?: number; result?: unknown; error?: unknown };
    if (lines.length <= MAX_FRAMES) lines.push(JSON.stringify({ t_ms: Date.now() - t0, dir: 's2c', msg }));
    if (typeof msg.id === 'number') waiters.get(msg.id)?.(msg.result ?? msg.error);
  });
  let id = 0;
  const call = (method: string, params: unknown) =>
    new Promise<unknown>((resolve) => {
      const n = ++id;
      waiters.set(n, resolve);
      const msg = { jsonrpc: '2.0', id: n, method, params };
      lines.push(JSON.stringify({ t_ms: Date.now() - t0, dir: 'c2s', msg }));
      ws.send(JSON.stringify(msg));
    });
  await call('core.hello', { proto: 'loom/1.0', client_id: 'ui-capture', want_role: 'viewer', client: { kind: 'web', version: 'capture', tty: false } });
  await script(call);
  ws.close();
  fs.writeFileSync(path.join(OUT, `${name}.jsonl`), lines.join('\n') + '\n');
  console.log(`fixtures/wire/${name}.jsonl: ${lines.length - 1} frames`);
}

const url = process.argv[2] ?? 'ws://localhost:4790/rpc';
await capture(url, 'replay-late-join', 'subscribe can/ch0 with snapshot (1-based parts, items[core.snapshot]) then ~1.5 s live + heartbeat + control path', async (call) => {
  await call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: true, encoding: 'json', backlog: { max_events: 300, max_bytes: 1_048_576, max_age_ms: 600_000 } });
  await new Promise((r) => setTimeout(r, 1_500));
});
await capture(url, 'replay-writes', 'echo-writes uart.send / can.send / uds.request rows (requested→accepted→sent→confirmed, origin user) on the control path before the reply; core.stop/release; safe_state list/run; unknown method', async (call) => {
  await call('core.subscribe', { filter: { domains: ['uart', 'uds'] }, snapshot: false, encoding: 'json' });
  await call('uart.send', { channel: 'dut', data: 'uname -a' });
  await call('can.send', { channel: 'ch0', frame: '300#01' });
  await call('uds.request', { target: 'ecu1', hex: '22F190' });
  await call('core.safe_state.list', {});
  await call('core.safe_state.run', { ids: ['can.periodic_stop'] });
  await call('core.stop', { scope: 'all', reason: 'capture' });
  await call('core.stop.release', {});
  await call('can.tx.send', { channel: 'ch0', id: 768, data_hex: '01' });
  await new Promise((r) => setTimeout(r, 300));
});
