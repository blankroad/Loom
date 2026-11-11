import { describe, expect, it } from 'vitest';
import type { Right } from '../proto/provisional';
import { RenderScheduler } from '../store/renderScheduler';
import { RowStore } from '../store/rowStore';
import { ShellControlStore } from '../store/shellControl';
import { FakeNet, TEST_HELLO, writeEv } from '../store/testing/fakeNet';
import { ActionGate, GateError, type ConfirmRequest } from './actionGate';
import { NetError } from '../net/rpc';
import { createPanelContext, SimpleCursorBus, UndeclaredError } from './context';
import { DomainRpcError } from './rpcEnvelope';
import { ManifestError, validateManifest, type ActionSpec, type PanelManifest } from './manifest';

const send: ActionSpec = { id: 'send', rpc: 'can.send', class: 'request', right: 'operate', confirm: 'none' };
const abort: ActionSpec = { id: 'abort', rpc: 'uds.abort', class: 'request', right: 'operate', confirm: 'none', safetyDirection: 'reduce' };

function gateWith(rights: Right[], opts: { locked?: boolean; answer?: boolean } = {}) {
  const net = new FakeNet();
  const asked: ConfirmRequest[] = [];
  const gate = new ActionGate({
    net,
    rights: () => new Set(rights),
    writeLocked: () => opts.locked ?? false,
    confirmUi: {
      confirm: async (r) => {
        asked.push(r);
        return opts.answer ?? true;
      },
    },
  });
  return { gate, net, asked };
}

describe('ActionGate', () => {
  it('operate action passes with operate or approve, fails with watch', async () => {
    await expect(gateWith(['operate']).gate.run(send, {})).resolves.toBeDefined();
    await expect(gateWith(['approve']).gate.run(send, {})).resolves.toBeDefined();
    const w = gateWith(['watch']);
    await expect(w.gate.run(send, {})).rejects.toMatchObject({ code: 'E_RIGHTS' });
    expect(w.net.calls).toHaveLength(0);
  });

  it('safety-reducing actions: watch allowed, no confirm, allowed while STALLED', async () => {
    const g = gateWith(['watch'], { locked: true });
    await g.gate.run({ ...abort, confirm: 'none' }, { target: 'bcm' });
    expect(g.net.calls[0]?.method).toBe('uds.abort');
    expect(g.asked).toHaveLength(0);
  });

  it('locks normal writes while STALLED', async () => {
    const g = gateWith(['operate'], { locked: true });
    await expect(g.gate.run(send, {})).rejects.toMatchObject({ code: 'E_STALLED' });
  });

  it('enter_twice / card ask the shell; "no" cancels without an RPC', async () => {
    const yes = gateWith(['operate']);
    await yes.gate.run({ ...send, confirm: 'card' }, { id: 1 });
    expect(yes.asked[0]?.mode).toBe('card');
    expect(yes.net.calls).toHaveLength(1);

    const no = gateWith(['operate'], { answer: false });
    await expect(no.gate.run({ ...send, confirm: 'enter_twice' }, {})).rejects.toBeInstanceOf(GateError);
    expect(no.asked[0]?.mode).toBe('enter_twice');
    expect(no.net.calls).toHaveLength(0);
  });
});

const manifest = (over: Partial<PanelManifest> = {}): PanelManifest => ({
  id: 'demo.t',
  domain: 'demo',
  title: 'T',
  version: 1,
  milestone: 'M1',
  topics: [{ key: 'rows', filter: () => ({ domains: ['uds'] }), snapshot: 'required' }],
  queries: ['uds.trace.query'],
  actions: [send],
  memory: { maxBytes: 1024 },
  render: { maxHz: 20, budgetMs: 4 },
  ...over,
});

describe('manifest validation', () => {
  it('accepts a valid manifest', () => {
    expect(() => validateManifest(manifest())).not.toThrow();
  });
  it('rejects bad ids, duplicate keys, bad rpc names and confirmed safety actions', () => {
    expect(() => validateManifest(manifest({ id: 'Bad' }))).toThrow(ManifestError);
    expect(() => validateManifest(manifest({ id: 'other.t' }))).toThrow(/domain/);
    expect(() =>
      validateManifest(manifest({ topics: [manifest().topics[0]!, manifest().topics[0]!] })),
    ).toThrow(/duplicate topic/);
    expect(() => validateManifest(manifest({ queries: ['nodot'] }))).toThrow(/query/);
    expect(() => validateManifest(manifest({ actions: [{ ...abort, confirm: 'card' }] }))).toThrow(/safety/);
    expect(() => validateManifest(manifest({ memory: { maxBytes: 0 } }))).toThrow(/memory/);
  });
});

describe('PanelContext', () => {
  const make = (m: PanelManifest = manifest()) => {
    const net = new FakeNet();
    net.open();
    const shell = new ShellControlStore();
    shell.setConn({ status: 'open', hello: TEST_HELLO }, 0);
    const rows = new RowStore();
    const gate = new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } });
    const scheduler = new RenderScheduler({ raf: () => 1, cancelRaf: () => {}, now: () => 0 });
    const h = createPanelContext(m, {}, {
      net,
      gate,
      scheduler,
      rows,
      shell,
      cursor: new SimpleCursorBus(),
      ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} },
    });
    return { ...h, net, rows };
  };

  it('throws on undeclared topic, query and action (I-UI1)', async () => {
    const { ctx } = make();
    expect(() => ctx.store('nope')).toThrow(UndeclaredError);
    expect(() => ctx.query('core.events.query', {})).toThrow(UndeclaredError);
    await expect(ctx.act('nope', {})).rejects.toBeInstanceOf(UndeclaredError);
  });

  it('declared topic subscribes once, mirrors events and feeds write rows', () => {
    const { ctx, net, rows } = make();
    const m = ctx.store('rows');
    expect(ctx.store('rows')).toBe(m);
    expect(net.subs).toHaveLength(1);
    expect(net.subs[0]?.filter).toEqual({ domains: ['uds'] });
    net.subs[0]!.h.onSnapshotEnd?.('5');
    net.emitBatch(0, [writeEv('r9', 'sent', { seq: 6 })]);
    expect(m.snapshotDone).toBe(true);
    expect(m.tail(10)).toHaveLength(1);
    expect(rows.get('r9')?.state).toBe('sent');
  });

  it('act returns a WriteHandle tracking the row by request_id', async () => {
    const { ctx, net, rows } = make();
    net.responders.set('can.send', () => ({ request_id: 'r1' }));
    const wh = await ctx.act('send', { id: 0x300 });
    expect(wh.requestId).toBe('r1');
    const seen: string[] = [];
    wh.onChange((s) => seen.push(s));
    rows.apply(writeEv('r1', 'sent'));
    rows.apply(writeEv('r1', 'confirmed'));
    expect(seen).toEqual(['sent', 'confirmed']);
    expect(wh.state()).toBe('confirmed');
  });

  it('an RPC declared as both query and action may only be queried with dry_run:true', async () => {
    const { ctx, net } = make(manifest({ queries: ['can.send'] }));
    expect(() => ctx.query('can.send', { id: 1 })).toThrow(UndeclaredError);
    expect(() => ctx.query('can.send', { id: 1, dry_run: false })).toThrow(UndeclaredError);
    await ctx.query('can.send', { id: 1, dry_run: true });
    expect(net.calls).toEqual([{ method: 'can.send', params: { id: 1, dry_run: true } }]);
  });

  // loom_proto::Envelope as serialized by gateway daemon `envelope()` (schema_version, ok, cmd,
  // meta and others_since_last are always present; data/error/warnings are optional).
  const envOk = (cmd: string, data: unknown) => ({
    schema_version: 1,
    ok: true,
    cmd,
    data,
    meta: { truncated: false, record_health: 'ok', latch: false, viewers: { subscribed: 1, visible: 1 }, stream_epoch: '1' },
    others_since_last: { count: '0' },
  });
  const envFail = (cmd: string, error: unknown, data?: unknown) => {
    const base: Record<string, unknown> = envOk(cmd, null);
    delete base.data;
    return { ...base, ok: false, ...(data !== undefined ? { data } : {}), ...(error !== undefined ? { error } : {}) };
  };
  const ethManifest = () =>
    manifest({ queries: ['eth.show', 'eth.filter.check', 'eth.query', 'core.time', 'can.send'] });

  it('query unwraps a real domain success envelope to its data', async () => {
    const { ctx, net } = make(ethManifest());
    const data = { ref: 'eth:p0@seq:7', bytes: { encoding: 'hex', data: 'aabb' }, tree: [] };
    const env = envOk('eth.show', data);
    const snapshot = structuredClone(env);
    net.responders.set('eth.show', () => env);
    await expect(ctx.query('eth.show', { ref: 'eth:p0@seq:7' })).resolves.toBe(data);
    expect(env).toEqual(snapshot); // the raw result is not mutated
    // success with null data stays null (not the envelope)
    net.responders.set('eth.query', () => envOk('eth.query', null));
    await expect(ctx.query('eth.query', {})).resolves.toBeNull();
  });

  it('query rejects a failure envelope with the ErrorBody code/message, even with partial data', async () => {
    const { ctx, net } = make(ethManifest());
    net.responders.set('eth.show', () =>
      envFail('eth.show', { code: 'not_found', message: 'no such packet', hint: 'pick a newer row', retryable: false, detail: { ref: 'x' } }),
    );
    const e1 = await ctx.query('eth.show', { ref: 'x' }).then(
      () => null,
      (e: unknown) => e,
    );
    expect(e1).toBeInstanceOf(DomainRpcError);
    expect(e1).toMatchObject({ code: 'not_found', message: 'eth.show: no such packet', hint: 'pick a newer row', retryable: false, detail: { ref: 'x' } });

    // Partial failure (daemon `envelope(.., Err(b), Some(data))`): data present but still a rejection
    net.responders.set('can.send', () =>
      envFail('can.send', { code: 'partial', message: 'result not yet visible', request_id: 'r7' }, { request_id: 'r7', state: 'sent' }),
    );
    await expect(ctx.query('can.send', { dry_run: true })).rejects.toMatchObject({
      name: 'DomainRpcError',
      code: 'partial',
      requestId: 'r7',
      data: { request_id: 'r7', state: 'sent' },
    });

    // ok:false without an error body is still a failure
    net.responders.set('eth.query', () => envFail('eth.query', undefined));
    await expect(ctx.query('eth.query', {})).rejects.toMatchObject({ code: 'internal' });
  });

  it('query leaves bare core/mock results and business ok/data payloads untouched', async () => {
    const { ctx, net } = make(ethManifest());
    // eth.filter.check business payload {ok:false,pos} — not an envelope, not a rejection
    const check = { ok: false, pos: 3, expected: 'field', message: 'bad token' };
    net.responders.set('eth.filter.check', () => check);
    await expect(ctx.query('eth.filter.check', { expr: 'ip.' })).resolves.toBe(check);
    // business payload that has ok/data/cmd but no envelope meta/others_since_last
    const lookalike = { ok: true, cmd: 'x', data: { inner: 1 }, schema_version: 1 };
    net.responders.set('eth.query', () => lookalike);
    await expect(ctx.query('eth.query', {})).resolves.toBe(lookalike);
    // core RPC bare result
    const time = { mono_ns: '123', wall_ns: '456' };
    net.responders.set('core.time', () => time);
    await expect(ctx.query('core.time', {})).resolves.toBe(time);
    // non-object results pass through
    net.responders.set('eth.show', () => 'plain');
    await expect(ctx.query('eth.show', {})).resolves.toBe('plain');
  });

  it('query keeps undeclared/dry-run gates synchronous and preserves abort/network errors', async () => {
    const fake = new FakeNet();
    fake.open();
    const netErr = new NetError({ code: 'disconnected', message: 'eth.show: disconnected', maybeSent: true });
    const seen: { signal?: AbortSignal }[] = [];
    const net = {
      call: (method: string, _p?: unknown, opts?: { signal?: AbortSignal }) => {
        seen.push(opts ?? {});
        if (opts?.signal?.aborted) return Promise.reject(new NetError({ code: 'aborted', message: `${method}: aborted`, maybeSent: false }));
        return Promise.reject(netErr);
      },
      subscribe: fake.subscribe.bind(fake),
      clock: fake.clock.bind(fake),
    } as unknown as FakeNet;
    const shell = new ShellControlStore();
    shell.setConn({ status: 'open', hello: TEST_HELLO }, 0);
    const gate = new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } });
    const { ctx } = createPanelContext(ethManifest(), {}, {
      net,
      gate,
      scheduler: new RenderScheduler({ raf: () => 1, cancelRaf: () => {}, now: () => 0 }),
      rows: new RowStore(),
      shell,
      cursor: new SimpleCursorBus(),
      ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} },
    });
    expect(() => ctx.query('uds.request', {})).toThrow(UndeclaredError);
    expect(() => ctx.query('can.send', { dry_run: false })).toThrow(UndeclaredError);
    expect(seen).toHaveLength(0);

    await expect(ctx.query('eth.show', {})).rejects.toBe(netErr);
    const ac = new AbortController();
    ac.abort();
    await expect(ctx.query('eth.show', {}, ac.signal)).rejects.toMatchObject({ name: 'NetError', code: 'aborted' });
    expect(seen[1]?.signal).toBe(ac.signal);
  });

  it('act: a native failure envelope (even with partial data) rejects, makes no WriteHandle, one RPC', async () => {
    const { ctx, net } = make();
    let n = 0;
    net.responders.set('can.send', () => {
      n++;
      return envFail(
        'can.send',
        { code: 'partial', message: 'result not yet visible to viewers (hub stalled) — do not retry', request_id: 'r5', detail: { published: false } },
        { request_id: 'r5', state: 'sent' },
      );
    });
    let handle: unknown = 'unset';
    const err = await ctx.act('send', { id: 1 }).then(
      (h) => {
        handle = h;
        return null;
      },
      (e: unknown) => e,
    );
    expect(handle).toBe('unset');
    expect(err).toBeInstanceOf(DomainRpcError);
    expect(err).toMatchObject({ code: 'partial', requestId: 'r5', data: { request_id: 'r5', state: 'sent' } });
    expect(n).toBe(1);
    expect(net.calls).toHaveLength(1);

    net.responders.set('can.send', () => envFail('can.send', { code: 'stop_latched', message: 'STOP latched' }));
    await expect(ctx.act('send', { id: 1 })).rejects.toMatchObject({ code: 'stop_latched', message: 'can.send: STOP latched' });
    expect(net.calls).toHaveLength(2);
  });

  it('act / ActionGate keep the success shape (envelope or bare) and business ok:false payloads', async () => {
    const { ctx, net } = make();
    const env = envOk('can.send', { request_id: 'r2', state: 'sent' });
    net.responders.set('can.send', () => env);
    expect((await ctx.act('send', {})).requestId).toBe('r2');

    const g = gateWith(['operate']);
    g.net.responders.set('can.send', () => env);
    await expect(g.gate.run(send, {})).resolves.toBe(env); // envelope returned unchanged, not unwrapped
    const biz = { ok: false, request_id: 'r3' }; // bare business payload, not an envelope
    g.net.responders.set('can.send', () => biz);
    await expect(g.gate.run(send, {})).resolves.toBe(biz);
    expect(g.net.calls).toHaveLength(2);
  });

  it('rows() is limited to the panel domain', () => {
    const { ctx } = make();
    expect(() => ctx.rows({ domain: 'uds' })).toThrow(UndeclaredError);
    expect(ctx.rows({ domain: 'demo' }).list()).toEqual([]);
  });

  it('dispose closes subscriptions', () => {
    const { ctx, dispose, net } = make();
    ctx.store('rows');
    dispose();
    expect(net.subs[0]?.closed).toBe(true);
  });
});
