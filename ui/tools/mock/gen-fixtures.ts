// Regenerates fixtures/wire/*.jsonl — recorded viewer-side wire transcripts of
// the mock (provisional contract v1), used as unit-test input by the stores and
// as a shape gate (fixtures.test.ts).
//   pnpm exec tsx tools/mock/gen-fixtures.ts
// Each file: line 1 = header, then {"t_ms", "dir": "c2s"|"s2c", "msg"} per frame.
// The mock runs with a counter clock, so ts values are stable across runs;
// batch boundaries can still shift if timing changes (regenerate, then review the diff).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';
import type { Clock } from './hub';
import { startMock } from './server';

const OUT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures', 'wire');
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

class CounterClock implements Clock {
  private n = 0n;
  mono(): bigint {
    this.n += 100_000n; // +0.1 ms per reading
    return 5_000_000_000n + this.n;
  }
  wall(): bigint {
    return 1_762_000_000_000_000_000n + this.n;
  }
}

interface Rec {
  t_ms: number;
  dir: 'c2s' | 's2c';
  msg: Record<string, unknown>;
}

class Peer {
  readonly log: Rec[] = [];
  private id = 0;
  private readonly t0 = Date.now();
  private constructor(
    readonly ws: WebSocket,
    private readonly record: boolean,
  ) {
    ws.on('message', (d) => {
      const msg = JSON.parse(d.toString()) as Record<string, unknown>;
      if (this.record && msg.method !== 'core.heartbeat') this.log.push({ t_ms: Date.now() - this.t0, dir: 's2c', msg });
      if (typeof msg.id === 'number') this.replies.set(msg.id, msg);
      this.waiters.splice(0).forEach((w) => w());
    });
  }
  private waiters: (() => void)[] = [];
  static async open(url: string, record: boolean, rights = 'operate'): Promise<Peer> {
    const ws = new WebSocket(`${url}?rights=${rights}`);
    await new Promise((r, j) => ws.once('open', r).once('error', j));
    const p = new Peer(ws, record);
    await p.call('core.hello', { proto: 'loom/1.0', client_id: record ? 'ui-fixture' : 'ctl', want_role: 'viewer', client: { kind: 'web', version: '0.0.0-p0', tty: false } });
    return p;
  }
  async call(method: string, params: unknown = {}): Promise<Record<string, unknown>> {
    const id = ++this.id;
    const msg = { jsonrpc: '2.0', id, method, params };
    if (this.record) this.log.push({ t_ms: Date.now() - this.t0, dir: 'c2s', msg });
    this.ws.send(JSON.stringify(msg));
    for (;;) {
      const hit = this.seen(id);
      if (hit) return hit;
      await new Promise<void>((r) => {
        this.waiters.push(r);
        setTimeout(r, 50);
      });
    }
  }
  /** generator-only, a few dozen calls per run */
  private readonly replies = new Map<number, Record<string, unknown>>();
  private seen(id: number): Record<string, unknown> | undefined {
    return this.replies.get(id);
  }
  close() {
    this.ws.terminate();
  }
}

type Script = (viewer: Peer, ctl: Peer) => Promise<void>;

const FIXTURES: Record<string, { note: string; script: Script }> = {
  'uart-normal': {
    note: 'E-UI-1: subscribe uart/dut with snapshot; agent uart.send "uname -a" (stages + tx + device echo); user keystrokes "ls\\r" (uart.input, owner -> user)',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['uart'], channels: ['dut'] }, snapshot: true, max_batch_ms: 10 });
      await ctl.call('mock.agent', { method: 'uart.send', params: { channel: 'dut', data: 'uname -a' } });
      await sleep(60);
      await v.call('uart.input', { channel: 'dut', data_hex: Buffer.from('ls\r').toString('hex'), input_seq: 1 });
      await sleep(60);
    },
  },
  'can-bcm': {
    note: 'E-UI-3: agent 0x300#01 -> sim BCM 0x301 (device rx); user 0x300#00 from the viewer (origin user)',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: false });
      await ctl.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', frame: '300#01' } });
      await sleep(30);
      await v.call('can.send', { channel: 'ch0', frame: '300#00' });
      await sleep(30);
    },
  },
  'uds-pending': {
    note: 'E-UI-2: agent read-did F190; sent with pending_count 1..3 (NRC 0x78) then confirmed with VIN; user 3E00 (TesterPresent)',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['uds'] }, snapshot: false });
      await ctl.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '22F190' } });
      await v.call('uds.request', { target: 'bcm', hex: '3E00' });
      await sleep(20);
    },
  },
  gap: {
    note: 'A13 three loss layers on can/ch0: display.gap and record.gap with seq_range, source.gap with producer_range only (no invented stream seqs)',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: false });
      for (let i = 0; i < 5; i++) await ctl.call('mock.emit', { domain: 'can', channel: 'ch0', kind: 'can.frame', dir: 'rx', origin: 'device', payload: { id: 256, ext: false, fd: false, dlc: 1, bytes: { encoding: 'hex', data: '0' + i } } });
      for (const kind of ['display_gap', 'record_gap', 'source_gap']) await ctl.call('mock.inject', { kind, channel: 'ch0' });
      await sleep(20);
    },
  },
  'outcome-unknown': {
    note: 'write that ends in outcome_unknown (no confirmation before timeout): the UI must keep it as unknown, never retry',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: false });
      await ctl.call('mock.inject', { kind: 'outcome_unknown' });
      await ctl.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', frame: '310#FF' } });
      await sleep(20);
    },
  },
  'late-join': {
    note: 'late join: 1,000 CAN frames already published, then subscribe with snapshot -> snapshot_part n=1..total (≤ 200 events / 60 KB each) -> snapshot_end{stream_seq:H} -> live > H',
    script: async (v, ctl) => {
      await ctl.call('mock.inject', { kind: 'burst_can', count: 1000, channel: 'ch0' });
      await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: true });
      await sleep(20);
      await ctl.call('mock.emit', { domain: 'can', channel: 'ch0', kind: 'can.frame', dir: 'rx', origin: 'device', payload: { id: 1, ext: false, fd: false, dlc: 0, bytes: { encoding: 'hex', data: '' } } });
      await sleep(20);
    },
  },
  'stop-latch': {
    note: 'STOP: viewer core.stop_all -> core.latch on the control sub; agent write refused (requested -> failed, stop_latched); user write still allowed; release',
    script: async (v, ctl) => {
      await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: false });
      await v.call('core.stop_all');
      await ctl.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', frame: '300#01' } });
      await v.call('can.send', { channel: 'ch0', frame: '300#00' });
      await v.call('core.stop.release');
      await sleep(20);
    },
  },
};

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  for (const [name, f] of Object.entries(FIXTURES)) {
    const mock = await startMock({ port: 0, host: '127.0.0.1', scenarios: [], clock: new CounterClock(), log: () => undefined });
    const ctl = await Peer.open(mock.url, false);
    const viewer = await Peer.open(mock.url, true);
    await f.script(viewer, ctl);
    viewer.close();
    ctl.close();
    await mock.close();
    const header = { fixture: name, contract: 'loom/1.0 (ui provisional v1)', generated_by: 'tools/mock/gen-fixtures.ts', note: f.note };
    const body = [header, ...viewer.log].map((l) => JSON.stringify(l)).join('\n') + '\n';
    fs.writeFileSync(path.join(OUT, `${name}.jsonl`), body);
    console.log(`fixtures/wire/${name}.jsonl: ${viewer.log.length} frames`);
  }
}

main().catch((e: unknown) => {
  console.error(e);
  process.exit(1);
});
