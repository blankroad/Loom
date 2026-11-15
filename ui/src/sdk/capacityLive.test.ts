// @vitest-environment jsdom
// TASK-CAN-LIVE regression, end to end over the real wire path: a fake loomd that enforces the native
// per-connection subscription cap (hub MAX_SUBS_PER_CONN = 8 → E_BUSY), the real inline NetClient + mux,
// the SDK panel context (visibility lease) and the real can.trace panel, fed with the QA marker's
// native records (qa/CAN-MARKER.json: 300#C47A7A2663DE839A → device 0x301 exact copy, 0x310 follows).
//
// Defect reproduced: after a zero-channel tour, retained hidden docks held every server slot, the bound
// CAN panel was parked forever ("ring 1 (gap)", 0 frames). Now the visible panel takes the slot of the
// least-recently-visible hidden dock (after the daemon confirms the release), and gets snapshot + live.

import { afterEach, describe, expect, it } from 'vitest';
import type { LoomEvent, SubscribeFilter } from '../proto/provisional';
import type { CoreDeps, SocketEvents } from '../net/core';
import { createNetClient } from '../net/index';
import { matchesFilter } from '../net/wire';
import { ManualTimers, tick } from '../net/testing/fixtures';
import { ActivityStore } from '../store/activityStore';
import { RenderScheduler } from '../store/renderScheduler';
import { RowStore } from '../store/rowStore';
import { ShellControlStore } from '../store/shellControl';
import { SHELL_STREAM_FILTER } from '../shell/services';
import { registerCanPanels } from '../panels/can';
import NATIVE from '../panels/can/fixtures/qaCanMarker.native.json';
import { FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { ActionGate } from './actionGate';
import { createPanelContext, SimpleCursorBus, type PanelContextHandle } from './context';
import type { PanelManifest } from './manifest';
import { getPanel } from './registry';
import type { PanelInstance } from './registry';

registerCanPanels();

const MAX_SUBS_PER_CONN = 8;
const HELLO = { role: 'viewer', rights: 'operate', conn_id: 'c1', stream_epoch: '1794728047', server: { version: 'mock', build: 'x' }, env: 'sim', latch: false, viewers: { subscribed: 1, visible: 1 }, capabilities: [] };

interface Req { id: number; method: string; params?: Record<string, unknown> }

/** loomd stand-in: journal + per-connection subscription cap + snapshot/live delivery */
function fakeDaemon() {
  const timers = new ManualTimers();
  let sock: { ev: SocketEvents } | null = null;
  const journal: LoomEvent[] = [];
  const subs = new Map<string, SubscribeFilter>();
  let nextSub = 0;
  const log = { busy: 0, maxHeld: 0, subscribes: [] as { filter: SubscribeFilter; snapshot: boolean; after_seq?: string }[], unsubscribes: 0 };
  const send = (m: unknown) => sock!.ev.onText(JSON.stringify(m));
  const note = (method: string, params: unknown) => send({ jsonrpc: '2.0', method, params });
  const handle = (r: Req) => {
    const reply = (result: unknown) => send({ jsonrpc: '2.0', id: r.id, result });
    switch (r.method) {
      case 'core.hello':
        return reply(HELLO);
      case 'core.subscribe': {
        const p = r.params as { filter: SubscribeFilter; snapshot: boolean; after_seq?: string };
        log.subscribes.push(p);
        if (subs.size >= MAX_SUBS_PER_CONN) {
          log.busy++;
          return send({ jsonrpc: '2.0', id: r.id, error: { code: -32000, message: 'subscription limit', data: { code: 'E_BUSY' } } });
        }
        const id = `sub${++nextSub}`;
        subs.set(id, p.filter);
        log.maxHeld = Math.max(log.maxHeld, subs.size);
        reply({ sub_id: id, stream_epoch: HELLO.stream_epoch });
        if (p.snapshot) {
          const backlog = journal.filter((e) => matchesFilter(p.filter, e));
          note('core.snapshot_part', { sub: id, n: 1, total: 1, events: backlog, items: [] });
          note('core.snapshot_end', { sub: id, stream_seq: journal.length ? journal[journal.length - 1]!.seq : '0' });
        }
        return;
      }
      case 'core.unsubscribe':
        log.unsubscribes++;
        subs.delete((r.params as { sub_id: string }).sub_id);
        return reply({});
      default:
        return reply({});
    }
  };
  const deps: CoreDeps = {
    openSocket: (_url, e) => {
      sock = { ev: e };
      return { send: (t) => queueMicrotask(() => handle(JSON.parse(t) as Req)), close: () => undefined };
    },
    now: () => 0,
    timers,
  };
  /** publish native records: journal + live core.events to every matching subscription */
  const publish = (evs: LoomEvent[]) => {
    journal.push(...evs);
    for (const [id, f] of subs) {
      const m = evs.filter((e) => matchesFilter(f, e));
      if (m.length) note('core.events', { sub: id, first: m[0]!.seq, last: m[m.length - 1]!.seq, lag_ms: 0, events: m });
    }
  };
  return { deps, publish, log, held: () => subs.size, open: () => sock!.ev.onOpen() };
}

async function settle(frames: (() => void)[]) {
  for (let i = 0; i < 30; i++) {
    await tick();
    frames.splice(0).forEach((cb) => cb());
  }
}

/** a module panel's data topic exactly as the real manifest declares it (filters copied from the panels) */
const moduleManifest = (id: string, filter: SubscribeFilter): PanelManifest => ({
  id,
  domain: id.split('.')[0]!,
  title: id,
  version: 1,
  milestone: 'M1',
  propsSchema: { type: 'object', properties: {} },
  topics: [{ key: 't', filter: () => filter, snapshot: 'required' }],
  queries: [],
  actions: [],
  memory: { maxBytes: 1 << 20 },
  render: { maxHz: 30, budgetMs: 8 },
});

describe('CAN live after a retained-dock tour (8 subscriptions per connection)', () => {
  const open: { h: PanelContextHandle; inst?: PanelInstance; host?: HTMLElement }[] = [];
  afterEach(() => {
    for (const o of open.splice(0)) {
      o.inst?.dispose();
      o.h.dispose();
      o.host?.remove();
    }
  });

  it('visible bound CAN receives snapshot + ongoing agent/device frames; hidden docks lend slots; no E_BUSY; ≤ 8 held', async () => {
    const d = fakeDaemon();
    const frames: (() => void)[] = [];
    const net = createNetClient('ws://localhost:7421/rpc', { inline: true, deps: d.deps, frames: { request: (cb) => frames.push(cb) } });
    await tick();
    d.open();
    await settle(frames);
    expect(net.state.status).toBe('open');

    let clock = 0;
    const now = () => (clock += 16);
    let pending: (() => void) | null = null;
    const scheduler = new RenderScheduler({ raf: (cb) => ((pending = () => cb(now())), 1), cancelRaf: () => (pending = null), now });
    const draw = () => {
      for (let i = 0; i < 4 && pending; i++) {
        const p = pending;
        pending = null;
        p();
      }
    };
    const shell = new ShellControlStore();
    shell.setConn(net.state, 0);
    const rows = new RowStore({ now });
    const deps = {
      net,
      gate: new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } }),
      scheduler,
      rows,
      shell,
      cursor: new SimpleCursorBus(),
      ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} },
      agentCursor: new ActivityStore(),
      now,
      absNowMs: now,
    };

    // the shell's single pinned stream (Activity + test), as services.ts subscribes it
    const shellEvents: LoomEvent[] = [];
    net.subscribe(SHELL_STREAM_FILTER, { snapshot: true, pinned: true }, { onBatch: (b) => shellEvents.push(...b.events) });
    await settle(frames);

    // zero-channel tour + bound views: each dock is shown, then retained hidden when the user moves on
    const tour: [string, SubscribeFilter][] = [
      ['uart.terminal', { domains: ['uart'], channels: ['qa-uart'] }],
      ['uart.terminal', { domains: ['uart'], channels: ['qa-uart2'] }],
      ['eth.packets', { domains: ['eth'], channels: ['qa-eth'], ext: { eth_filter: 'doip' } }],
      ['uds.console', { domains: ['uds'] }],
      ['video.workbench', { domains: ['video'], kinds: ['video.state', 'video.frame'] }],
      ['logic.analyzer', { domains: ['logic'], kinds: ['logic.state', 'logic.capture'] }],
      ['psu.gauge', { domains: ['psu'], channels: ['qa-psu'] }],
      ['eth.packets', { domains: ['eth'], channels: ['qa-eth2'], ext: { eth_filter: 'udp' } }],
    ];
    const docks: PanelContextHandle[] = [];
    for (const [i, [id, f]] of tour.entries()) {
      const h = createPanelContext(moduleManifest(id, f), {}, deps, `${id}#${i}`);
      open.push({ h });
      h.ctx.store('t');
      await settle(frames);
      h.setVisible(false);
      docks.push(h);
    }
    expect(d.held()).toBe(MAX_SUBS_PER_CONN); // shell + 7 docks; the 8th dock took the oldest hidden one's slot

    // bound CAN opened from the channel bar (visible)
    const reg = getPanel('can.trace')!;
    const ch = createPanelContext(reg.manifest, { channel: 'qa-can' }, deps, 'can.trace#qa');
    const inst = reg.factory(ch.ctx);
    const host = document.createElement('div');
    document.body.append(host);
    inst.mount(host);
    open.push({ h: ch, inst, host });
    await settle(frames);
    draw();
    const subsBefore = d.log.subscribes.length;
    expect(d.log.subscribes.at(-1)).toMatchObject({ filter: { domains: ['can'], channels: ['qa-can'] }, snapshot: true });
    expect(d.log.subscribes.some((s) => JSON.stringify(s.filter) === JSON.stringify({ domains: ['core'], kinds: ['core.activity'] }))).toBe(false);
    expect(host.querySelector<HTMLElement>('[data-testid="can-trace"]')!.dataset.subscription).toBe('live');

    // the QA marker, native records in order: periodic 0x310, agent tx 0x300 (w8), device 0x301 copy, 0x310 follows C4
    const native = NATIVE as unknown as LoomEvent[];
    for (const e of native) {
      d.publish([e]);
      await settle(frames);
    }
    d.publish([{ ...native[0]!, kind: 'core.activity', domain: 'core', channel: null, origin: 'agent', seq: '1978', payload: { actor: 'qa', verb: 'read', domain: 'can', channel: 'qa-can', range: { seq_a: '1972', seq_b: '1973' }, summary: 'read' } } as LoomEvent]);
    await settle(frames);
    draw();
    expect(d.log.subscribes.length).toBe(subsBefore); // ongoing frames: no resubscribe churn

    const rowsEls = [...host.querySelectorAll<HTMLElement>('.ct-row')].filter((e) => !e.hidden);
    const ids = rowsEls.map((r) => `${r.dataset.dir}:${r.dataset.origin}:${r.dataset.id}`);
    expect(ids).toContain('tx:agent:0x300');
    expect(ids).toContain('rx:device:0x301');
    expect(ids.filter((x) => x === 'rx:device:0x310').length).toBeGreaterThanOrEqual(2);
    const agentRow = rowsEls.find((r) => r.dataset.id === '0x300')!;
    expect(agentRow.dataset.requestId).toBe('w8');
    expect(shellEvents.some((e) => e.kind === 'core.activity')).toBe(true); // shell Activity still fed

    // hidden → shown: the oldest docks get a fresh snapshot again (they never lose their instance)
    ch.setVisible(false);
    docks[0]!.setVisible(true);
    await settle(frames);
    const re = d.log.subscribes.at(-1)!;
    expect(re).toMatchObject({ filter: { domains: ['uart'], channels: ['qa-uart'] }, snapshot: true });
    expect(re.after_seq).toBeUndefined();

    expect(d.log.busy).toBe(0);
    expect(d.log.maxHeld).toBeLessThanOrEqual(MAX_SUBS_PER_CONN);
    net.close();
  });

  it('panel visibility drives the lease of every topic, including topics opened lazily while hidden', () => {
    const net = new FakeNet();
    net.open({ ...TEST_HELLO, conn_id: 'c-me' });
    const shell = new ShellControlStore();
    shell.setConn(net.state, 0);
    const now = () => 0;
    const m: PanelManifest = { ...moduleManifest('can.trace', { domains: ['can'], channels: ['a'] }), topics: [
      { key: 'a', filter: () => ({ domains: ['can'], channels: ['a'] }), snapshot: 'required' },
      { key: 'b', filter: () => ({ domains: ['core'], kinds: ['core.activity'] }), snapshot: 'none' },
    ] };
    const h = createPanelContext(m, {}, {
      net, scheduler: new RenderScheduler({ raf: () => 1, cancelRaf: () => {}, now }), rows: new RowStore({ now }), shell,
      gate: new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } }),
      cursor: new SimpleCursorBus(), ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} }, agentCursor: new ActivityStore(), now, absNowMs: now,
    }, 'p1');
    open.push({ h });
    h.ctx.store('a');
    h.setVisible(true); // already visible: no message
    h.setVisible(false);
    h.ctx.store('b'); // opened while hidden
    h.setVisible(false);
    h.setVisible(true);
    // F4: the hidden open carries its lease in the subscribe itself (never "active, then inactive")
    expect(net.subs.map((s) => s.opts.active)).toEqual([undefined, false]);
    expect(net.subs.map((s) => s.active)).toEqual([[false, true], [true]]);
  });

  it('F4 end to end: at the cap, a topic opened lazily by a hidden panel never merges or re-snapshots visible ones', async () => {
    const d = fakeDaemon();
    const frames: (() => void)[] = [];
    const net = createNetClient('ws://localhost:7421/rpc', { inline: true, deps: d.deps, frames: { request: (cb) => frames.push(cb) } });
    await tick();
    d.open();
    await settle(frames);
    const shell = new ShellControlStore();
    shell.setConn(net.state, 0);
    const now = () => 0;
    const deps = {
      net, scheduler: new RenderScheduler({ raf: () => 1, cancelRaf: () => {}, now }), rows: new RowStore({ now }), shell,
      gate: new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } }),
      cursor: new SimpleCursorBus(), ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} }, agentCursor: new ActivityStore(), now, absNowMs: now,
    };
    net.subscribe(SHELL_STREAM_FILTER, { snapshot: true, pinned: true }, { onBatch: () => {} });
    // Integrated: 7 visible streams, two of them mergeable (UART u0/u1) → shell + 7 = 8
    const visible: [string, SubscribeFilter][] = [
      ['uart.terminal', { domains: ['uart'], channels: ['u0'] }],
      ['uart.terminal', { domains: ['uart'], channels: ['u1'] }],
      ['eth.packets', { domains: ['eth'], channels: ['qa-eth'] }],
      ['uds.console', { domains: ['uds'] }],
      ['can.trace', { domains: ['can'], channels: ['qa-can'] }],
      ['psu.gauge', { domains: ['psu'], channels: ['qa-psu'] }],
      ['logic.analyzer', { domains: ['logic'], channels: ['la0'] }],
    ];
    const mirrors = visible.map(([id, f], i) => {
      const h = createPanelContext(moduleManifest(id, f), {}, deps, `${id}#${i}`);
      open.push({ h });
      return h.ctx.store('t');
    });
    const resyncs = () => mirrors.flatMap((m) => m.gaps.filter((g) => g.kind === 'resync').map((g) => g.cause));
    await settle(frames);
    expect(d.held()).toBe(MAX_SUBS_PER_CONN);
    expect(resyncs()).toEqual([]);
    const subsBefore = d.log.subscribes.length;
    // a retained panel, hidden before it ever opened its topic, opens it lazily now
    const hidden = createPanelContext(moduleManifest('video.workbench', { domains: ['video'], channels: ['cam0'] }), {}, deps, 'video#hidden');
    open.push({ h: hidden });
    hidden.setVisible(false);
    hidden.ctx.store('t');
    await settle(frames);
    expect(d.log.subscribes.length).toBe(subsBefore); // 0 merges / 0 visible re-snapshots
    expect(d.log.unsubscribes).toBe(0);
    expect(resyncs()).toEqual([]); // no "merged" / "suspended_hidden" on any visible panel
    expect(d.log.busy).toBe(0);
    net.close();
  });
});
