import { describe, expect, it } from 'vitest';
import type { Right } from '../proto/provisional';
import { RenderScheduler } from '../store/renderScheduler';
import { RowStore } from '../store/rowStore';
import { ShellControlStore } from '../store/shellControl';
import { FakeNet, TEST_HELLO, writeEv } from '../store/testing/fakeNet';
import { ActionGate, GateError, type ConfirmRequest } from './actionGate';
import { createPanelContext, SimpleCursorBus, UndeclaredError } from './context';
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
