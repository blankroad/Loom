// loom-mock: a fake `loomd` speaking the provisional contract v1 so the UI can
// be built and tested before CORE's fixture-replay gateway exists.
//   pnpm mock                      port 7421 (LOOM_MOCK_PORT), all scenarios
//   LOOM_MOCK_SCENARIOS=none pnpm mock      no background traffic
//   WS /rpc?rights=watch|operate|approve    (mock-only way to test rights)
//   GET /healthz · POST /api/v1/stop
// Test-only RPCs: mock.agent{method, params} (act as the agent),
// mock.inject{kind, ...} (faults/load), mock.emit{...} (publish one event).
// Domains: uart (bootshell dut), can (sim BCM ch0), uds (bcm), eth (sim tap lan1, eth.ts),
// psu (SimPsu bench, psu.ts), test (TC-1 session, testRun.ts).

import http from 'node:http';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type RawData, type WebSocket } from 'ws';
import type { HelloResult, Right, U64 } from '../../src/proto/provisional';
import { PROTO, RIGHT_RANK } from '../../src/proto/provisional';
import { EthSim } from './eth';
import { Conn, Hub, MAX_SUBS, realClock, type Clock, type Emit } from './hub';
import { PsuSim } from './psu';
import { listScenarios, loadScenario, play } from './scenario';
import { MockRpcError, SAFE_STATES, Sim, type Actor } from './sim';
import { TestRun } from './testRun';

export interface MockOptions {
  port?: number;
  host?: string;
  /** scenario file names in fixtures/scenarios; default all; [] = none */
  scenarios?: string[];
  clock?: Clock;
  log?: (m: string) => void;
}

export interface MockServer {
  port: number;
  url: string;
  hub: Hub;
  sim: Sim;
  eth: EthSim;
  psu: PsuSim;
  test: TestRun;
  close(): Promise<void>;
}

const MAX_FRAME = 64 * 1024;
const AGENT: Actor = { origin: 'agent', clientId: 'agent:mock' };

/** methods that change device or daemon state, with the right they need */
const NEEDS: Record<string, Right> = {
  'uart.input': 'operate',
  'uart.input.yield': 'operate',
  'uart.input.clear': 'operate',
  'uart.input.takeover': 'operate',
  'uart.send': 'operate',
  'can.send': 'operate', // CAN v1 (docs/impl/requests/can-ui.md §3)
  'can.tx.send': 'operate', // provisional alias until the panel moves to can.send
  'uds.request': 'operate',
  'uds.lease.takeover': 'operate',
  // contract v1.1: STOP (and emergency OFF) for every right incl. watch; release = operate
  'core.stop_all': 'watch',
  'core.stop': 'watch',
  'core.stop.release': 'operate',
  'eth.send': 'operate',
  'eth.capture.start': 'operate',
  'eth.capture.stop': 'watch', // safety direction
  'psu.set': 'operate',
  // psu.out: off = watch (emergency OFF), on = operate — checked in dispatch
  'test.pause': 'operate',
  'test.resume': 'operate',
  'test.skip': 'operate',
  'test.retry': 'operate',
  'test.answer': 'operate',
  'test.end': 'operate',
  'test.verdict': 'approve', // viewers may only override, with approve
  'core.safe_state.run': 'operate',
  'core.mark': 'operate',
  'core.approval.respond': 'operate',
};

type Params = Record<string, unknown>;
type Reply = { result: unknown; after?: () => void };

export async function startMock(o: MockOptions = {}): Promise<MockServer> {
  const log = o.log ?? ((m: string) => console.log(`[mock] ${m}`));
  const hub = new Hub(o.clock ?? realClock);
  const sim = new Sim(hub);
  const eth = new EthSim(hub, sim);
  const psu = new PsuSim(hub, sim);
  const test = new TestRun(hub);
  /** WS upgrades are refused until this Date.now() (inject refuse_ws: "WS down" for HTTP STOP tests) */
  let refuseWsUntil = 0;
  /** inject auth_required: viewer upgrades without the `loom_viewer` cookie get HTTP 401 (need-auth flow) */
  let authRequired = false;
  let scenarioAgentPaused = false; // mock.inject{kind:"pause_scenario_agent"} — keeps background agent sends out of a test window
  const hasCookie = (req: http.IncomingMessage) => /(?:^|;\s*)loom_viewer=/.test(req.headers.cookie ?? '');
  let connSeq = 0;
  const stops: (() => void)[] = [];

  const actorOf = (c: Conn): Actor => ({ origin: c.origin, clientId: c.role === 'agent' ? `agent:${c.clientId}` : `viewer:${c.clientId}`, connId: c.id });

  const stopAll = (by: Actor): Record<string, unknown> => {
    const cancelled = [...sim.approvals.keys()];
    for (const id of cancelled) sim.resolveApproval(id, 'cancelled', by.clientId);
    sim.stopAll();
    hub.latch = true;
    const payload = {
      on: true,
      by: by.clientId,
      origin: by.origin,
      jobs_stopped: ['job:uart-expect-1'],
      approvals_cancelled: cancelled,
      driver_uncancellable: false,
      outputs_held: psu.held(),
      missing: [],
      persisted: true,
    };
    hub.state.set('core.latch', payload);
    hub.publish({ domain: 'core', kind: 'core.latch', origin: by.origin, client_id: by.clientId, payload });
    return { latch: true };
  };

  /** device/write RPCs shared by viewers and mock.agent */
  const writeRpc = async (a: Actor, method: string, p: Params): Promise<unknown> => {
    switch (method) {
      case 'uart.input':
        return sim.uartInput(a, p);
      case 'uart.input.yield':
        return sim.uartOwner(a, 'yield', p.channel as string | undefined);
      case 'uart.input.clear':
        return sim.uartOwner(a, 'clear', p.channel as string | undefined);
      case 'uart.input.takeover':
        return sim.uartOwner(a, 'takeover', p.channel as string | undefined);
      case 'uart.send':
        return sim.uartSend(a, p);
      case 'can.send':
      case 'can.tx.send': // provisional alias (same handler)
        return sim.canSend(a, p);
      case 'can.trace.query':
        return sim.canTraceQuery(a, p as { channel?: string; max?: number });
      case 'uds.request': {
        const r = (await sim.udsRequest(a, p)) as Record<string, unknown>;
        // DoIP target: the sim tap captures request/response (linked by request_id) before the reply
        if (typeof r.request_id === 'string') eth.onUds(String(p.target ?? 'bcm'), r.request_id, a.origin, String(p.hex ?? '').replace(/\s+/g, '').toLowerCase(), typeof r.resp_hex === 'string' ? r.resp_hex : null);
        return r;
      }
      case 'uds.lease.takeover':
        return sim.udsTakeover(a, (p.target as string) ?? 'bcm');
      case 'eth.capture.start':
        return eth.captureStart(a, p);
      case 'eth.capture.stop':
        return eth.captureStop(a, p);
      case 'eth.send':
        return eth.send(a, p);
      case 'eth.query':
        return eth.query(p);
      case 'eth.filter.check':
        return eth.checkFilterRpc(String(p.expr ?? ''));
      case 'eth.ifaces':
        return { ifaces: [{ name: 'lo', alias: 'sim tap', can_capture: true }, { name: 'en0', can_capture: false }] };
      case 'eth.export':
        return { job_id: `job-export-${hub.seq}`, request_id: `r-export-${hub.seq}` };
      case 'psu.out':
        return psu.out(a, p);
      case 'psu.set':
        return psu.set(a, p);
      default:
        throw new MockRpcError(-32601, `method not found: ${method}`);
    }
  };

  const inject = (c: Conn, p: Params): unknown => {
    const kind = p.kind as string;
    const seq = hub.seq;
    const domain = (p.domain as string) ?? 'can';
    const channel = (p.channel as string) ?? 'ch0';
    const from = String(seq > 5n ? seq - 4n : 1n);
    switch (kind) {
      case 'display_gap':
        return hub.publish({ domain, channel, kind: 'display.gap', origin: 'device', payload: { seq_range: { from, to: String(seq) }, count: String(p.count ?? 5), cause: 'injected' } }).seq;
      case 'record_gap':
        return hub.publish({ domain, channel, kind: 'record.gap', origin: 'device', payload: { seq_range: { from, to: String(seq) }, count: String(p.count ?? 5), cause: 'disk_full' } }).seq;
      case 'source_gap':
        // before the hub: producer range only, no invented stream seqs (contract 02 §2.4)
        return hub.publish({ domain, channel, kind: 'source.gap', origin: 'device', payload: { producer_range: { producer_id: `${domain}-sim`, producer_epoch: '1', from: '1000', to: '1036' }, count: String(p.count ?? 37), cause: 'driver_overrun' } }).seq;
      case 'stall_heartbeat':
        hub.hbStalledUntil = Number(hub.clock.mono() / 1_000_000n) + Math.min(Number(p.ms ?? 4000), 60_000);
        return { stalled_ms: p.ms ?? 4000 };
      case 'disconnect':
      case 'slow_consumer': {
        let n = 0;
        for (const x of hub.conns) {
          if (x === c && p.self !== true) continue;
          if (kind === 'slow_consumer') x.notify('core.closed', { reason: 'slow_consumer' });
          x.ws.terminate();
          n++;
        }
        return { dropped: n };
      }
      case 'epoch_change': {
        hub.newEpoch();
        hub.publish({ domain: 'core', kind: 'core.epoch_changed', origin: 'device', payload: { stream_epoch: hub.epochStr } });
        for (const x of hub.conns) if (x !== c) x.ws.terminate();
        return { stream_epoch: hub.epochStr };
      }
      case 'flood_can':
        sim.startFlood('can', Math.min(Number(p.fps ?? 1000), 10_000), Number(p.seconds ?? 5));
        return { ok: true };
      case 'flood_stats':
        return { can: sim.floodEmitted.get('can') ?? 0, uart: sim.floodEmitted.get('uart') ?? 0, running: sim.floodRunning() };
      case 'flood_uart':
        sim.startFlood('uart', Math.min(Number(p.bytes_per_s ?? 92_000), 1_000_000), Number(p.seconds ?? 5));
        return { ok: true };
      case 'burst_can': {
        const n = Math.min(Number(p.count ?? 1000), 20_000);
        for (let i = 0; i < n; i++) sim.canFrame('device', channel, 0x100 + (i % 16), Uint8Array.from([i & 0xff, i >> 8]), 'rx');
        return { last_seq: String(hub.seq) };
      }
      case 'policy_confirm':
        sim.policyConfirm = p.on !== false;
        return { policy_confirm: sim.policyConfirm };
      case 'outcome_unknown':
        sim.armOutcomeUnknown = true;
        return { armed: true };
      case 'late_backlog': {
        // UI-T01: `count` events (alternating UART lines on dut and CAN frames on ch0) before a tab opens
        const n = Math.min(Number(p.count ?? 1000), 20_000);
        for (let i = 0; i < n; i++) {
          if (i % 2 === 0) hub.publish({ domain: 'uart', channel: 'dut', kind: 'uart.data', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'base64', data: Buffer.from(`[backlog ${i}] boot line\r\n`, 'latin1').toString('base64') } } });
          else sim.canFrame('device', 'ch0', 0x500 + (i % 8), Uint8Array.from([i & 0xff, (i >> 8) & 0xff]), 'rx');
        }
        return { last_seq: String(hub.seq) };
      }
      case 'uart_queries': {
        // UI-T07: terminal query sequences a device could send (DA1/DA2/DSR/CPR/DECRQSS/OSC 4/10/11/12 '?')
        const seqs = ['\x1b[c', '\x1b[>c', '\x1b[5n', '\x1b[6n', '\x1bP$qm\x1b\\', '\x1b]4;1;?\x07', '\x1b]10;?\x07', '\x1b]11;?\x07', '\x1b]12;?\x07'];
        const text = `queries:${seqs.join('')}done\r\n`;
        return hub.publish({ domain: 'uart', channel: String(p.channel ?? 'dut'), kind: 'uart.data', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'base64', data: Buffer.from(text, 'latin1').toString('base64') } } }).seq;
      }
      case 'auth_required':
        authRequired = p.on !== false;
        return { auth_required: authRequired };
      case 'refuse_ws':
        // "WS down" window: new upgrades fail until then; combine with {kind:'disconnect'}
        refuseWsUntil = Date.now() + Math.min(Number(p.ms ?? 3000), 30_000);
        return { until_ms: p.ms ?? 3000 };
      case 'conn_stats':
        // per viewer connection: rights, open subscriptions, RPC calls by method (E2E assertions)
        return {
          conns: [...hub.conns]
            .filter((x) => x.hello && x !== c)
            .map((x) => ({ id: x.id, client_id: x.clientId, role: x.role, rights: x.rights, subs: x.subs.size, calls: Object.fromEntries(x.calls), presence: x.presence, ...(p.reports === true ? { reports: x.reports } : {}) })),
        };
      case 'pause_scenario_agent':
        scenarioAgentPaused = p.on !== false;
        return { ok: true, paused: scenarioAgentPaused };
      case 'reset':
        // test isolation: release latch-free state the previous test left behind
        authRequired = false;
        scenarioAgentPaused = false;
        for (const id of [...sim.approvals.keys()]) sim.resolveApproval(id, 'cancelled', 'mock-reset');
        sim.udsLease.clear();
        sim.policyConfirm = false;
        sim.armOutcomeUnknown = false;
        refuseWsUntil = 0;
        return { ok: true };
      case 'eth_flood':
        eth.startFlood(String(p.ch ?? 'lan1'), Math.min(Number(p.pps ?? 100_000), 1_000_000), Number(p.seconds ?? 3), Number(p.sample ?? 1000));
        return { ok: true };
      case 'eth_flood_stats':
        return eth.floodStats;
      case 'eth_kernel_drop':
        return eth.kernelDrop(String(p.ch ?? 'lan1'), Math.max(1, Math.min(Number(p.count ?? 37), 1_000_000))).seq;
      case 'psu_gap':
        psu.gap();
        return { ok: true };
      case 'test_tc1':
        return test.runTc1(Number(p.gap_ms ?? 10));
      default:
        throw new MockRpcError(-32602, `unknown inject kind: ${kind}`);
    }
  };

  const dispatch = async (c: Conn, method: string, p: Params): Promise<Reply> => {
    if (method === 'core.hello') {
      if (p.proto !== PROTO) throw new MockRpcError(-32602, `proto ${String(p.proto)} != ${PROTO}`, { code: 'proto_mismatch' });
      c.clientId = typeof p.client_id === 'string' ? p.client_id : `c${connSeq}`;
      c.role = p.want_role === 'agent' ? 'agent' : 'viewer';
      c.rights = c.role === 'agent' ? 'approve' : c.queryRights;
      const viewers = [...hub.conns].filter((x) => x.hello && x.role === 'viewer');
      const hello: HelloResult = {
        role: c.role,
        conn_id: c.id,
        stream_epoch: hub.epochStr,
        server: { version: 'loom-mock 0.1', build: 'mock' },
        env: 'sim',
        latch: hub.latch,
        viewers: { subscribed: viewers.length + (c.role === 'viewer' ? 1 : 0), visible: viewers.filter((x) => x.visible).length + 1 },
        capabilities: ['mock'],
      };
      if (c.role === 'viewer') hello.rights = c.rights;
      c.hello = hello;
      hub.startHeartbeat(c);
      return { result: hello };
    }
    if (!c.hello) throw new MockRpcError(-32000, 'core.hello required first', { code: 'hello_required' });
    // a dry_run preview is a read-only (R0) query with no rows: any right (CAN v1 §5, uds-ui U3)
    const need = p.dry_run === true ? undefined : NEEDS[method];
    if (need && RIGHT_RANK[c.rights] < RIGHT_RANK[need]) {
      throw new MockRpcError(-32003, `${method} needs ${need}`, { code: 'rights', hint: `this connection has ${c.rights}` });
    }
    switch (method) {
      case 'psu.out':
        if (p.state === 'on' && RIGHT_RANK[c.rights] < RIGHT_RANK.operate) throw new MockRpcError(-32003, 'psu.out on needs operate', { code: 'rights', hint: `this connection has ${c.rights}` });
        return { result: await writeRpc(actorOf(c), method, p) };
      case 'test.plan.show':
      case 'test.pause':
      case 'test.resume':
      case 'test.skip':
      case 'test.retry':
      case 'test.answer':
      case 'test.verdict':
      case 'test.end':
        return { result: test.viewer(actorOf(c), method, p) };
      case 'core.subscribe': {
        if (c.role === 'viewer' && c.subs.size >= MAX_SUBS) throw new MockRpcError(-32004, `≤ ${MAX_SUBS} subscriptions per viewer connection`, { code: 'sub_limit' });
        const { sub, after } = hub.subscribe(c, p as { after_seq?: U64 });
        return { result: { sub_id: sub.id, stream_epoch: hub.epochStr }, after };
      }
      case 'core.unsubscribe':
        return { result: { ok: hub.unsubscribe(c, String(p.sub_id)) } };
      case 'core.time': {
        const recv = hub.clock.mono();
        return { result: { recv_mono_ns: String(recv), send_mono_ns: String(hub.clock.mono()), wall_ns: String(hub.clock.wall()), clock_id: 'mock-mono' } };
      }
      case 'core.viewer.presence':
        c.visible = p.visible !== false;
        c.presence = p;
        return { result: { ok: true } };
      case 'core.metrics.report':
        c.reports.push(p); // C2: last 64 kept
        if (c.reports.length > 64) c.reports.splice(0, c.reports.length - 64);
        return { result: { ok: true } };
      case 'core.stop':
      case 'core.stop_all':
        return { result: stopAll(actorOf(c)) };
      case 'core.stop.release': {
        hub.latch = false;
        const a = actorOf(c);
        const payload = { on: false, by: a.clientId, origin: a.origin, jobs_stopped: [], approvals_cancelled: [], driver_uncancellable: false, outputs_held: [], missing: [], persisted: true };
        hub.state.set('core.latch', payload);
        hub.publish({ domain: 'core', kind: 'core.latch', origin: a.origin, client_id: a.clientId, payload });
        return { result: { latch: false } };
      }
      case 'core.safe_state.list':
        // schema/v1 SafeStateItem {id, domain, channel, method, description, blocked:boolean}
        // + provisional `label`/`kind` the shell adapter still reads (report p1-m1-accept §4)
        return {
          result: {
            items: [
              ...SAFE_STATES.map((x) => ({ id: x.id, domain: x.domain, channel: x.channel, method: x.method, description: x.label, blocked: false, label: x.label, kind: 'recover' })),
              { id: `psu.${psu.channel}_off`, domain: 'psu', channel: psu.channel, method: 'psu.out', description: `PSU ${psu.channel} 비상 OFF`, blocked: false, label: `PSU ${psu.channel} 비상 OFF`, kind: 'emergency_off' },
            ],
          },
        };
      case 'core.safe_state.run': {
        const ids = Array.isArray(p.ids) ? (p.ids as string[]) : [];
        const a = actorOf(c);
        // schema/v1 SafeStateRunResult{items:[{id, outcome:sent|skipped|failed, finally_blocked, reason?, request_id?}]}
        // + provisional `results:[{id, ok, blocked?, error?}]` the shell adapter still reads
        const items: Record<string, unknown>[] = [];
        const results: Record<string, unknown>[] = [];
        for (const id of ids.slice(0, 32)) {
          if (id === `psu.${psu.channel}_off`) {
            const r = await psu.out(a, { state: 'off' });
            items.push({ id, outcome: 'sent', finally_blocked: false, request_id: r.request_id });
            results.push({ id, ok: true });
            continue;
          }
          const s = SAFE_STATES.find((x) => x.id === id);
          if (!s) {
            items.push({ id, outcome: 'failed', finally_blocked: false, reason: 'unknown_id' });
            results.push({ id, ok: false, error: 'unknown_id' });
          } else if (id === 'uds.default_session') {
            items.push({ id, outcome: 'skipped', finally_blocked: true, reason: 'interlock: ignition off (sim)' });
            results.push({ id, ok: false, blocked: 'interlock: ignition off (sim)' });
          } else {
            sim.activity(a, 'safe_state', s.domain, s.channel, s.label);
            items.push({ id, outcome: 'sent', finally_blocked: false });
            results.push({ id, ok: true });
          }
        }
        return { result: { items, results } };
      }
      case 'core.mark': {
        const a = actorOf(c);
        const ev = hub.publish({ domain: 'core', kind: 'core.mark', origin: a.origin, client_id: a.clientId, payload: { label: String(p.label ?? ''), note: p.note ?? '' } });
        return { result: { seq: ev.seq } };
      }
      case 'core.approval.respond': {
        const ok = sim.resolveApproval(String(p.approval_id), p.decision === 'approve' ? 'approve' : 'deny', actorOf(c).clientId);
        if (!ok) throw new MockRpcError(-32005, 'no such approval', { code: 'not_found' });
        return { result: { ok: true } };
      }
      case 'mock.agent': {
        const m = String(p.method);
        const ap = (p.params as Params) ?? {};
        if (m === 'core.stop_all' || m === 'core.stop') return { result: stopAll(AGENT) };
        if (m === 'test.verdict') return { result: await test.agentVerdict(ap) };
        if (m === 'test.step') return { result: test.agentStep(ap) };
        if (m === 'test.run_tc1') return { result: await test.runTc1(Number(ap.gap_ms ?? 10)) };
        return { result: await writeRpc(AGENT, m, ap) };
      }
      case 'mock.inject':
        return { result: inject(c, p) };
      case 'mock.emit':
        return { result: { seq: hub.publish(p as unknown as Emit).seq } };
      default:
        return { result: await writeRpc(actorOf(c), method, p) };
    }
  };

  const onMessage = (c: Conn, data: RawData) => {
    let msg: { id?: unknown; method?: unknown; params?: unknown };
    try {
      msg = JSON.parse(data.toString()) as typeof msg;
    } catch {
      c.send({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'parse error' } });
      return;
    }
    if (Array.isArray(msg)) {
      c.send({ jsonrpc: '2.0', id: null, error: { code: -32600, message: 'batch arrays are rejected in v1' } });
      return;
    }
    const id = msg.id as number | string | undefined;
    if (typeof msg.method !== 'string') {
      c.send({ jsonrpc: '2.0', id: id ?? null, error: { code: -32600, message: 'invalid request' } });
      return;
    }
    const method = msg.method;
    c.countCall(method);
    dispatch(c, method, (msg.params ?? {}) as Params).then(
      (r) => {
        hub.flushAll();
        if (id !== undefined) c.send({ jsonrpc: '2.0', id, result: r.result });
        r.after?.(); // e.g. snapshot parts after the subscribe reply
      },
      (e: unknown) => {
        hub.flushAll();
        const err = e instanceof MockRpcError ? { code: e.code, message: e.message, ...(e.data ? { data: e.data } : {}) } : { code: -32603, message: String(e) };
        if (id !== undefined) c.send({ jsonrpc: '2.0', id, error: err });
      },
    );
  };

  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    if (url.pathname === '/rpc' && authRequired && !hasCookie(req)) {
      res.writeHead(401, { 'content-type': 'text/plain' }).end('viewer cookie required');
      return;
    }
    if (url.pathname === '/healthz') {
      res.writeHead(200, { 'content-type': 'text/plain' }).end('ok');
      return;
    }
    if (url.pathname === '/api/v1/stop' && req.method === 'POST') {
      // WS-down STOP fallback; the mock has no cookie auth
      req.resume();
      req.on('end', () => {
        const r = stopAll({ origin: 'user', clientId: 'viewer:http' });
        res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(r));
      });
      return;
    }
    res.writeHead(404).end();
  });

  const wss = new WebSocketServer({ noServer: true, maxPayload: MAX_FRAME, perMessageDeflate: false });
  server.on('upgrade', (req, socket, head) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    if (url.pathname !== '/rpc') {
      socket.destroy();
      return;
    }
    if (authRequired && !hasCookie(req) && url.searchParams.get('ctl') !== '1') {
      socket.end('HTTP/1.1 401 Unauthorized\r\n\r\n');
      return;
    }
    if (Date.now() < refuseWsUntil) {
      socket.end('HTTP/1.1 503 Service Unavailable\r\n\r\n');
      return;
    }
    wss.handleUpgrade(req, socket, head, (ws: WebSocket) => {
      const q = url.searchParams.get('rights');
      const rights: Right = q === 'watch' || q === 'approve' ? q : 'operate';
      const c = new Conn(`conn-${++connSeq}`, ws, rights);
      hub.addConn(c);
      ws.on('message', (d) => onMessage(c, d));
      ws.on('close', () => hub.dropConn(c));
      ws.on('error', () => hub.dropConn(c));
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(o.port ?? 7421, o.host ?? 'localhost', () => resolve());
  });
  const port = (server.address() as { port: number }).port;

  const names = o.scenarios ?? listScenarios();
  if (names.length) {
    // background devices of the default (scenario) run: PSU 1 Hz, eth SOME/IP-SD while capturing
    psu.start();
    eth.startBackground();
  }
  for (const n of names) {
    const sc = loadScenario(n);
    stops.push(
      play(
        sc.lines,
        {
          emit: (e) => hub.publish(e),
          // scenario agent sends can be paused per test (mock.inject{kind:'pause_scenario_agent'})
          agent: (m, p) =>
            scenarioAgentPaused ? Promise.resolve({ skipped: 'scenario agent paused' }) : m === 'core.stop_all' ? Promise.resolve(stopAll(AGENT)) : writeRpc(AGENT, m, p),
          call: (name, args) => name === 'bootLog' && sim.bootLog(args[0] ?? 'dut'),
        },
        log,
      ),
    );
    log(`scenario ${sc.name}: ${sc.lines.length} lines`);
  }

  return {
    port,
    url: `ws://${o.host ?? 'localhost'}:${port}/rpc`,
    hub,
    sim,
    eth,
    psu,
    test,
    close: async () => {
      stops.forEach((s) => s());
      sim.stopAll();
      eth.stop();
      psu.stop();
      for (const c of [...hub.conns]) {
        hub.dropConn(c);
        c.ws.terminate();
      }
      await new Promise<void>((r) => wss.close(() => r()));
      await new Promise<void>((r) => server.close(() => r()));
    },
  };
}

// CLI
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const env = process.env.LOOM_MOCK_SCENARIOS;
  const scenarios = env === undefined ? undefined : env === 'none' || env === '' ? [] : env.split(',');
  startMock({ port: Number(process.env.LOOM_MOCK_PORT ?? 7421), ...(scenarios ? { scenarios } : {}) }).then(
    (m) => console.log(`[mock] loom-mock listening on http://localhost:${m.port} (WS /rpc, /healthz, POST /api/v1/stop)`),
    (e: unknown) => {
      console.error('[mock] failed to start', e);
      process.exit(1);
    },
  );
}
