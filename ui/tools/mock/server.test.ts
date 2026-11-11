import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import WebSocket from 'ws';
import type { EventsBatch, LoomEvent } from '../../src/proto/provisional';
import { startMock, type MockServer } from './server';

interface Frame {
  id?: number;
  method?: string;
  params?: Record<string, unknown>;
  result?: Record<string, unknown>;
  error?: { code: number; message: string; data?: { code?: string } };
}

class Client {
  readonly frames: Frame[] = [];
  private nextId = 1;
  private waiters: (() => void)[] = [];
  private constructor(readonly ws: WebSocket) {
    ws.on('message', (d) => {
      this.frames.push(JSON.parse(d.toString()) as Frame);
      this.waiters.splice(0).forEach((w) => w());
    });
  }
  static async open(url: string, opts: { rights?: string; role?: 'viewer' | 'agent'; hello?: boolean } = {}): Promise<Client> {
    const ws = new WebSocket(opts.rights ? `${url}?rights=${opts.rights}` : url);
    await new Promise((r, j) => ws.once('open', r).once('error', j));
    const c = new Client(ws);
    if (opts.hello !== false) await c.call('core.hello', { proto: 'loom/1.0', client_id: `t${Math.random().toString(16).slice(2, 6)}`, want_role: opts.role ?? 'viewer', client: { kind: 'test', version: '0', tty: false } });
    return c;
  }
  send(method: string, params: unknown = {}): number {
    const id = this.nextId++;
    this.ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    return id;
  }
  async call(method: string, params: unknown = {}): Promise<Frame> {
    const id = this.send(method, params);
    return this.waitFor((f) => f.id === id);
  }
  async waitFor(pred: (f: Frame) => boolean, ms = 3000): Promise<Frame> {
    const end = Date.now() + ms;
    for (;;) {
      const hit = this.frames.find(pred);
      if (hit) return hit;
      if (Date.now() > end) throw new Error(`timeout waiting; last frames: ${JSON.stringify(this.frames.slice(-3)).slice(0, 600)}`);
      await new Promise<void>((r) => {
        this.waiters.push(r);
        setTimeout(r, 50);
      });
    }
  }
  /** events received on data subscription `sub` (default: all data subs, not the control sub) */
  events(sub?: string): LoomEvent[] {
    return this.frames.filter((f) => f.method === 'core.events' && (sub === undefined ? f.params?.sub !== 'control' : f.params?.sub === sub)).flatMap((f) => (f.params as unknown as EventsBatch).events);
  }
  index(pred: (f: Frame) => boolean): number {
    return this.frames.findIndex(pred);
  }
  close() {
    this.ws.terminate();
  }
}

let mock: MockServer;
beforeAll(async () => {
  mock = await startMock({ port: 0, host: '127.0.0.1', scenarios: [], log: () => undefined });
});
afterAll(async () => {
  await mock.close();
});

const isWrite = (rid: string) => (f: Frame) => f.method === 'core.events' && (f.params as unknown as EventsBatch).events.some((e) => e.request_id === rid);

describe('loom-mock', () => {
  it('requires core.hello first, reports rights from ?rights=', async () => {
    const raw = await Client.open(mock.url, { hello: false });
    expect((await raw.call('core.time')).error?.data?.code).toBe('hello_required');
    const h = await raw.call('core.hello', { proto: 'loom/1.0', client_id: 'x', want_role: 'viewer', client: { kind: 't', version: '0', tty: false } });
    expect(h.result).toMatchObject({ role: 'viewer', rights: 'operate', env: 'sim', latch: false });
    const w = await Client.open(mock.url, { rights: 'watch', hello: false });
    expect((await w.call('core.hello', { proto: 'loom/1.0', client_id: 'w', want_role: 'viewer', client: { kind: 't', version: '0', tty: false } })).result?.rights).toBe('watch');
    const t = await raw.call('core.time');
    expect(t.result).toMatchObject({ clock_id: 'mock-mono' });
    expect(BigInt(t.result!.send_mono_ns as string) >= BigInt(t.result!.recv_mono_ns as string)).toBe(true);
    raw.close();
    w.close();
  });

  it('E-UI-1: agent uart.send -> viewer sees write stages, tx and device echo; all events precede the reply', async () => {
    const viewer = await Client.open(mock.url);
    const sub = await viewer.call('core.subscribe', { filter: { domains: ['uart'], channels: ['dut'] }, snapshot: true, max_batch_ms: 10 });
    const subId = sub.result!.sub_id as string;
    // reply, then snapshot parts, then end
    await viewer.waitFor((f) => f.method === 'core.snapshot_end' && f.params?.sub === subId);
    expect(viewer.index((f) => f.id === sub.id)).toBeLessThan(viewer.index((f) => f.method === 'core.snapshot_part'));
    // the agent: a separate connection, subscribed too, so publish-before-reply is observable on its socket
    const agent = await Client.open(mock.url);
    const asub = await agent.call('core.subscribe', { filter: { domains: ['uart'] }, snapshot: false });
    const reply = await agent.call('mock.agent', { method: 'uart.send', params: { channel: 'dut', data: 'uname -a' } });
    const rid = reply.result!.request_id as string;
    expect(reply.result!.write_state).toBe('sent');
    const writes = agent.events(asub.result!.sub_id as string).filter((e) => e.request_id === rid && e.kind === 'uart.write');
    expect(writes.map((e) => (e.payload as { write_state: string }).write_state)).toEqual(['requested', 'accepted', 'sent']);
    const lastWriteFrame = agent.frames.map(isWrite(rid)).lastIndexOf(true);
    expect(lastWriteFrame).toBeLessThan(agent.index((f) => f.id === reply.id)); // publish-before-reply
    // viewer: same rows, origin agent, then device echo with the uname output
    await viewer.waitFor(() => viewer.events(subId).some((e) => e.kind === 'uart.data' && e.dir === 'rx' && Buffer.from((e.payload as { bytes: { data: string } }).bytes.data, 'base64').toString().includes('aarch64')));
    const evs = viewer.events(subId);
    expect(evs.filter((e) => e.request_id === rid).every((e) => e.origin === 'agent')).toBe(true);
    const tx = evs.find((e) => e.kind === 'uart.data' && e.dir === 'tx')!;
    expect(tx.origin).toBe('agent');
    const seqs = evs.map((e) => BigInt(e.seq));
    expect([...seqs].sort((a, b) => (a < b ? -1 : 1))).toEqual(seqs); // in order on a data sub
    // agent cursor arrives on the automatic control subscription
    await viewer.waitFor((f) => f.method === 'core.events' && f.params?.sub === 'control' && (f.params as unknown as EventsBatch).events[0]!.kind === 'core.agent_cursor');
    viewer.close();
    agent.close();
  });

  it('user write gets origin=user from the connection; watch is refused', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['can'] }, snapshot: false });
    const r = await v.call('can.send', { channel: 'ch0', frame: '300#00', origin: 'agent' });
    expect(r.result?.write_state).toBe('confirmed');
    const rows = v.events().filter((e) => e.request_id === r.result!.request_id);
    expect(rows.every((e) => e.origin === 'user' && e.client_id?.startsWith('viewer:'))).toBe(true);
    const w = await Client.open(mock.url, { rights: 'watch' });
    const denied = await w.call('can.send', { channel: 'ch0', frame: '300#00' });
    expect(denied.error?.data?.code).toBe('rights');
    v.close();
    w.close();
  });

  it('CAN E-UI-3: 0x300 from the agent -> sim BCM answers 0x301 (device rx)', async () => {
    const v = await Client.open(mock.url);
    const s = await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['ch0'] }, snapshot: false });
    await v.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', frame: '300#01' } });
    await v.waitFor(() => v.events(s.result!.sub_id as string).some((e) => e.kind === 'can.frame' && (e.payload as { id: number }).id === 0x301));
    const evs = v.events(s.result!.sub_id as string);
    expect(evs.find((e) => e.kind === 'can.frame' && e.dir === 'tx')).toMatchObject({ origin: 'agent' });
    expect(evs.find((e) => (e.payload as { id: number }).id === 0x301)).toMatchObject({ origin: 'device', dir: 'rx' });
    // the tx echo carries the request_id of its can.tx row (Trace merges them into one row)
    const rid = evs.find((e) => e.kind === 'can.tx')?.request_id;
    expect(rid).toBeTruthy();
    expect(evs.find((e) => e.kind === 'can.frame' && e.dir === 'tx')?.request_id).toBe(rid);
    v.close();
  });

  it('CAN trace query: agent read -> core.activity with the seq range read', async () => {
    const v = await Client.open(mock.url);
    const s = await v.call('core.subscribe', { filter: { kinds: ['core.activity'] }, snapshot: false });
    await v.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', id: 0x300, data: '02' } });
    const r = await v.call('mock.agent', { method: 'can.trace.query', params: { channel: 'ch0', max: 5 } });
    const res = r.result as { rows: LoomEvent[]; frames: LoomEvent[]; seq_a: string; seq_b: string };
    expect(res.rows).toEqual(res.frames);
    expect(res.frames.length).toBeGreaterThan(0);
    expect(res.frames.length).toBeLessThanOrEqual(5);
    await v.waitFor(() => v.events(s.result!.sub_id as string).some((e) => (e.payload as { action?: string }).action === 'read'));
    const act = v.events(s.result!.sub_id as string).find((e) => (e.payload as { action?: string }).action === 'read')!;
    expect(act.payload).toMatchObject({ method: 'can.trace.query', action: 'read', domain: 'can', channel: 'ch0', range: { seq_a: res.seq_a, seq_b: res.seq_b, from: res.seq_a, to: res.seq_b } });
    expect(act.origin).toBe('agent');
    v.close();
  });

  it('UDS E-UI-2: read-did F190 -> pending x3 (0x78) -> confirmed with VIN', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['uds'] }, snapshot: false });
    const r = await v.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '22F190' } });
    const rows = v.events().filter((e) => e.request_id === r.result!.request_id).map((e) => e.payload as { write_state: string; pending_count: number; resp_hex?: string });
    expect(rows.map((p) => `${p.write_state}:${p.pending_count}`)).toEqual(['requested:0', 'accepted:0', 'sent:0', 'sent:1', 'sent:2', 'sent:3', 'confirmed:3']);
    expect(Buffer.from(rows.at(-1)!.resp_hex!.slice(6), 'hex').toString()).toBe('WVWZZZ1KZAW000001');
    v.close();
  });

  it('STOP: latch on control path; agent writes refused (requested->failed); user writes still allowed; release', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['can'] }, snapshot: false });
    await v.call('core.stop_all');
    const latch = await v.waitFor((f) => f.params?.sub === 'control' && (f.params as unknown as EventsBatch).events[0]!.kind === 'core.latch');
    expect((latch.params as unknown as EventsBatch).events[0]!.payload).toMatchObject({ on: true, outputs_held: [{ domain: 'psu' }] });
    const refused = await v.call('mock.agent', { method: 'can.send', params: { channel: 'ch0', id: 0x300 } });
    expect(refused.error?.data?.code).toBe('stop_latched');
    const rid = (refused.error?.data as { request_id: string }).request_id;
    expect(v.events().filter((e) => e.request_id === rid).map((e) => (e.payload as { write_state: string }).write_state)).toEqual(['requested', 'failed']);
    expect((await v.call('can.send', { channel: 'ch0', id: 0x300 })).result?.write_state).toBe('confirmed');
    await v.call('core.stop.release');
    expect(mock.hub.latch).toBe(false);
    v.close();
  });

  it('late join: 1,000 events, then snapshot in parts (n/total) -> end{stream_seq:H} -> live > H', async () => {
    const ctl = await Client.open(mock.url);
    await ctl.call('mock.inject', { kind: 'burst_can', count: 1000, channel: 'late' });
    const v = await Client.open(mock.url);
    const s = await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['late'] }, snapshot: true });
    const subId = s.result!.sub_id as string;
    const end = await v.waitFor((f) => f.method === 'core.snapshot_end' && f.params?.sub === subId);
    const parts = v.frames.filter((f) => f.method === 'core.snapshot_part' && f.params?.sub === subId);
    // parts are bounded by 200 events AND 60 KB, numbered 0..total-1
    const N = parts[0]!.params!.total as number;
    expect(N).toBeGreaterThanOrEqual(5);
    expect(parts.map((p) => p.params!.n)).toEqual([...Array(N).keys()].map((i) => i + 1));
    expect(parts.every((p) => p.params!.total === N && (p.params!.events as unknown[]).length <= 200 && JSON.stringify(p).length < 64 * 1024)).toBe(true);
    expect(parts.flatMap((p) => p.params!.events as LoomEvent[])).toHaveLength(1000);
    const H = BigInt(end.params!.stream_seq as string);
    await ctl.call('mock.emit', { domain: 'can', channel: 'late', kind: 'can.frame', dir: 'rx', origin: 'device', payload: { id: 1 } });
    await v.waitFor((f) => f.method === 'core.events' && f.params?.sub === subId);
    expect(v.events(subId).every((e) => BigInt(e.seq) > H)).toBe(true);
    ctl.close();
    v.close();
  });

  it('resume: after_seq in ring replays missed events; out of ring -> display.gap{resume_out_of_ring} + snapshot', async () => {
    const ctl = await Client.open(mock.url);
    const emit = () => ctl.call('mock.emit', { domain: 'can', channel: 'res', kind: 'can.frame', dir: 'rx', origin: 'device', payload: {} });
    const a = (await emit()).result!.seq as string;
    await emit();
    await emit();
    const v = await Client.open(mock.url);
    const s = await v.call('core.subscribe', { filter: { domains: ['can'], channels: ['res'] }, snapshot: false, after_seq: a });
    await v.waitFor((f) => f.method === 'core.events' && f.params?.sub === s.result!.sub_id);
    expect(v.events(s.result!.sub_id as string).map((e) => BigInt(e.seq) - BigInt(a))).toEqual([1n, 2n]);
    // push the cursor out of the 20,000-event ring
    await ctl.call('mock.inject', { kind: 'burst_can', count: 20_000, channel: 'other' });
    const v2 = await Client.open(mock.url);
    const s2 = await v2.call('core.subscribe', { filter: { domains: ['can'], channels: ['res'] }, snapshot: false, after_seq: a });
    const id2 = s2.result!.sub_id as string;
    await v2.waitFor((f) => f.method === 'core.snapshot_end' && f.params?.sub === id2);
    const gap = v2.events(id2)[0]!;
    expect(gap.kind).toBe('display.gap');
    expect(gap.payload).toMatchObject({ cause: 'resume_out_of_ring', count: 'unknown' });
    ctl.close();
    v.close();
    v2.close();
  });

  it('enforces ≤ 8 subscriptions per viewer connection', async () => {
    const v = await Client.open(mock.url);
    for (let i = 0; i < 8; i++) expect((await v.call('core.subscribe', { filter: { channels: [`c${i}`] }, snapshot: false })).result).toBeDefined();
    expect((await v.call('core.subscribe', { filter: {}, snapshot: false })).error?.data?.code).toBe('sub_limit');
    v.close();
  });

  it('injections: 3 loss layers (source.gap without stream seqs), outcome_unknown, policy approvals', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['can'] }, snapshot: false });
    for (const kind of ['display_gap', 'record_gap', 'source_gap']) await v.call('mock.inject', { kind });
    const gaps = v.events().filter((e) => e.kind.endsWith('.gap'));
    expect(gaps.map((e) => e.kind)).toEqual(['display.gap', 'record.gap', 'source.gap']);
    expect(gaps[2]!.payload).toHaveProperty('producer_range');
    expect(gaps[2]!.payload).not.toHaveProperty('seq_range');
    expect(gaps.every((g) => typeof (g.payload as { count: unknown }).count === 'string')).toBe(true);

    await v.call('mock.inject', { kind: 'outcome_unknown' });
    expect((await v.call('mock.agent', { method: 'can.send', params: { id: 0x123 } })).result?.write_state).toBe('outcome_unknown');

    await v.call('mock.inject', { kind: 'policy_confirm', on: true });
    const pending = v.call('mock.agent', { method: 'can.send', params: { id: 0x124 } });
    const req = await v.waitFor((f) => f.params?.sub === 'control' && (f.params as unknown as EventsBatch).events[0]!.kind === 'core.approval_requested');
    const approvalId = ((req.params as unknown as EventsBatch).events[0]!.payload as { approval_id: string }).approval_id;
    await v.call('core.approval.respond', { approval_id: approvalId, decision: 'approve' });
    expect((await pending).result?.write_state).toBe('confirmed');
    await v.call('mock.inject', { kind: 'policy_confirm', on: false });
    v.close();
  });

  it('heartbeat every 1 s with contract v1 fields; stall_heartbeat suppresses it', async () => {
    const v = await Client.open(mock.url);
    const hb = await v.waitFor((f) => f.method === 'core.heartbeat', 2500);
    expect(Object.keys(hb.params!).sort()).toEqual(['daemon_mono_ns', 'daemon_wall_ns', 'latch', 'record_health', 'stream_seq_hw']);
    await v.call('mock.inject', { kind: 'stall_heartbeat', ms: 1500 });
    const n = v.frames.length;
    await new Promise((r) => setTimeout(r, 1200));
    expect(v.frames.slice(n).some((f) => f.method === 'core.heartbeat')).toBe(false);
    v.close();
  }, 8000);
});

describe('loom-mock M1 additions', () => {
  let mock: MockServer;
  beforeAll(async () => {
    mock = await startMock({ port: 0, scenarios: [], log: () => undefined });
  });
  afterAll(async () => {
    await mock.close();
  });

  it('CAN v1: can.send frame syntax, frame payload {dlc code, len, brs/esi/rtr}, dry_run has no rows', async () => {
    const v = await Client.open(mock.url, { rights: 'watch' });
    const s = await v.call('core.subscribe', { filter: { domains: ['can'] }, snapshot: false });
    const dry = await v.call('can.send', { channel: 'ch0', frame: '123##1' + '00'.repeat(12), dry_run: true });
    expect(dry.result).toMatchObject({ risk: 'R1', reason: 'sim channel', policy: 'auto' });
    const o = await Client.open(mock.url);
    const r = await o.call('can.send', { channel: 'ch0', frame: '123##1' + '00'.repeat(12), count: 2 });
    expect(r.result?.write_state).toBe('confirmed');
    await v.waitFor(() => v.events(s.result!.sub_id as string).filter((e) => e.kind === 'can.frame' && e.request_id === r.result!.request_id).length === 2);
    const echoes = v.events(s.result!.sub_id as string).filter((e) => e.kind === 'can.frame' && e.request_id === r.result!.request_id);
    expect(echoes[0]!.payload).toMatchObject({ id: 0x123, fd: true, brs: true, esi: false, rtr: false, dlc: 9, len: 12, frame_index: 0 });
    expect(echoes[1]!.payload).toMatchObject({ frame_index: 1 });
    expect(v.events().filter((e) => e.kind === 'can.tx' && e.request_id === undefined)).toEqual([]); // dry run: no rows
    const bad = await o.call('can.send', { channel: 'ch0', frame: '123#0102', brs: true });
    expect(bad.error?.data?.code).toBe('E_USAGE');
    v.close();
    o.close();
  });

  it('UDS: dry-run is a preview (no rows); NRC ends failed; pending count; user lease → agent exit 6', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['uds'] }, snapshot: false });
    const dry = await v.call('uds.request', { target: 'bcm', hex: '22F190', dry_run: true });
    expect(dry.result).toMatchObject({ dry_run: true, service: 'ReadDataByIdentifier', did_name: 'VIN', risk: 'R0' });
    expect(v.events()).toEqual([]);
    const nrc = await v.call('uds.request', { target: 'bcm', hex: '2E F1 A0 01' });
    expect(nrc.result?.write_state).toBe('failed');
    const p5 = await v.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '22F190', mock_pending: 5, mock_pending_ms: 5 } });
    const rows = v.events().filter((e) => e.request_id === p5.result!.request_id);
    expect(rows.map((e) => (e.payload as { pending_count: number }).pending_count).at(-1)).toBe(5);
    await v.call('uds.lease.takeover', { target: 'bcm' });
    const preempted = await v.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '1003' } });
    expect(preempted.error?.data).toMatchObject({ code: 'lease_preempted', exit_code: 6 });
    expect((await v.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '22F190', mock_pending: 0 } })).result?.write_state).toBe('confirmed'); // R0 reads still allowed
    await v.call('mock.inject', { kind: 'reset' });
    v.close();
  });

  it('UART: viewer = conn_id, deferred agent send → agent_activity, Ctrl-C releases, others_since_last', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['uart'] }, snapshot: false });
    await v.call('uart.input', { channel: 'dut', data_hex: '6c73', input_seq: 1 });
    const owner = v.events().find((e) => e.kind === 'uart.input_owner')!;
    expect((owner.payload as { viewer: string }).viewer).toMatch(/^conn-\d+$/);
    const agent = v.call('mock.agent', { method: 'uart.send', params: { channel: 'dut', data: 'reboot' } });
    await v.waitFor(() => v.events().some((e) => e.kind === 'uart.agent_activity' && (e.payload as { kind: string }).kind === 'deferred_send'));
    await v.call('uart.input', { channel: 'dut', data_hex: '03', input_seq: 2 });
    const r = await agent;
    expect(r.result).toMatchObject({ write_state: 'sent', others_since_last: { count: 1, origins: ['user'] } });
    expect(v.events().some((e) => e.kind === 'uart.agent_activity' && (e.payload as { kind: string }).kind === 'idle')).toBe(true);
    v.close();
  });

  it('safe_state: schema/v1 items + run result items (and the provisional fields), psu emergency OFF', async () => {
    const v = await Client.open(mock.url, { rights: 'watch' });
    const list = (await v.call('core.safe_state.list')).result as { items: { id: string; method: string; description: string; blocked: boolean; kind: string }[] };
    expect(list.items.every((i) => /^[a-z]+\.[a-z_]+$/.test(i.id) && typeof i.method === 'string' && typeof i.blocked === 'boolean')).toBe(true);
    expect(list.items.find((i) => i.kind === 'emergency_off')?.id).toBe('psu.bench_off');
    const o = await Client.open(mock.url);
    const run = (await o.call('core.safe_state.run', { ids: ['psu.bench_off', 'uds.default_session', 'nope'] })).result as { items: { id: string; outcome: string; finally_blocked: boolean }[]; results: unknown[] };
    expect(run.items.map((i) => `${i.id}:${i.outcome}:${i.finally_blocked}`)).toEqual(['psu.bench_off:sent:false', 'uds.default_session:skipped:true', 'nope:failed:false']);
    expect(mock.psu.state.out).toBe(false);
    // STOP for every right (contract v1.1), release needs operate
    expect((await v.call('core.stop', { scope: 'all' })).result).toMatchObject({ latch: true });
    expect((await v.call('core.stop.release')).error?.data?.code).toBe('rights');
    await o.call('core.stop.release');
    v.close();
    o.close();
  });

  it('eth: capture, DoIP packets linked to the UDS row, filter check, complete query, kernel drop, send', async () => {
    const v = await Client.open(mock.url);
    await v.call('core.subscribe', { filter: { domains: ['eth'], channels: ['lan1'] }, snapshot: false });
    await v.call('mock.agent', { method: 'eth.capture.start', params: { ch: 'lan1', iface: 'lo' } });
    const u = await v.call('mock.agent', { method: 'uds.request', params: { target: 'bcm', hex: '22F190', mock_pending: 0 } });
    const pkts = v.events().filter((e) => e.kind === 'eth.pkt');
    expect(pkts.map((e) => (e.payload as { link: { request_id: string; role: string } }).link)).toEqual([
      expect.objectContaining({ request_id: u.result!.request_id, role: 'request', confidence: 'exact' }),
      expect.objectContaining({ request_id: u.result!.request_id, role: 'response' }),
    ]);
    expect((await v.call('eth.filter.check', { expr: 'doip.type ==' })).result).toMatchObject({ ok: false, pos: 12, expected: '값' });
    expect((await v.call('eth.filter.check', { expr: 'doip.type == 0x8001' })).result).toEqual({ ok: true });
    const q = (await v.call('eth.query', { ch: 'lan1', filter: 'doip.type == 0x8001', max: 2000 })).result as { rows: unknown[] };
    expect(q.rows).toHaveLength(2);
    const drop = await v.call('mock.inject', { kind: 'eth_kernel_drop', count: 42 });
    expect(v.events().find((e) => e.seq === (drop.result as unknown as string))).toMatchObject({ kind: 'source.gap', domain: 'eth', payload: { count: '42', cause: 'kernel_drop' } });
    const dry = await v.call('eth.send', { kind: 'raw', ch: 'lan1', hex: '00'.repeat(60), dry_run: true });
    expect(dry.result).toMatchObject({ dry_run: true, risk: 'R1' });
    const sent = await v.call('eth.send', { kind: 'raw', ch: 'lan1', hex: '00'.repeat(60) });
    expect(v.events().some((e) => e.kind === 'eth.pkt' && (e.payload as { tx_request_id?: string }).tx_request_id === sent.result!.request_id)).toBe(true);
    await v.call('mock.agent', { method: 'eth.capture.stop', params: { ch: 'lan1' } });
    v.close();
  });

  it('test session TC-1: rejected fabricated PASS, override only for approve viewers, agent override denied', async () => {
    const a = await Client.open(mock.url, { rights: 'approve' });
    await a.call('core.subscribe', { filter: { domains: ['test'] }, snapshot: false });
    const run = await a.call('mock.agent', { method: 'test.run_tc1', params: { gap_ms: 1 } });
    const session = (run.result as { session: string }).session;
    const rejected = a.events().find((e) => e.kind === 'test.verdict' && (e.payload as { rejected?: unknown }).rejected);
    expect(rejected?.payload).toMatchObject({ step_id: 'S3', rejected: { code: 'E_OUT_OF_WINDOW' } });
    const denied = await a.call('mock.agent', { method: 'test.verdict', params: { step: 'S4', verdict: 'pass', override: true } });
    expect(denied.error?.data?.code).toBe('E_DENIED');
    const op = await Client.open(mock.url);
    expect((await op.call('test.verdict', { session, step: 'S4', verdict: 'inconclusive', override: true, reason: 'x' })).error?.data?.code).toBe('rights');
    expect((await a.call('test.verdict', { session, step: 'S4', verdict: 'inconclusive', override: true, reason: '잔여 DTC' })).result).toEqual({ ok: true });
    const ov = a.events().filter((e) => e.kind === 'test.verdict').at(-1)!;
    expect(ov.payload).toMatchObject({ override: true, effective: 'inconclusive', previous: { effective: 'fail' } });
    await a.call('test.end', { session, abort: true });
    a.close();
    op.close();
  });
});
