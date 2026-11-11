// @vitest-environment jsdom
// C-6 / C-8 (report p1-m1-accept §4): adapters read schema/v1 first, older shapes still accepted.
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it } from 'vitest';
import { CanTraceModel } from '../panels/can/traceModel';
import type { ActivityPayload } from '../proto/gen/activity_payload';
import type { SafeStateItem as GenItem } from '../proto/gen/safe_state_item';
import type { SafeStateList } from '../proto/gen/safe_state_list';
import type { SafeStateRunResult } from '../proto/gen/safe_state_run_result';
import { createShellServices } from '../shell/services';
import { ShellCtx } from '../shell/ShellContext';
import { StopPanel } from '../shell/StopPanel';
import { readActivity, readSafeStateList, readSafeStateRun } from './adapter';
import { ev, FakeNet, TEST_HELLO } from './testing/fakeNet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('core.activity (C-6)', () => {
  it('schema/v1 {client_id, method, summary, range{from,to}, waiting}', () => {
    const p: ActivityPayload = { client_id: 'agent:claude-1', method: 'can.trace.query', summary: '2 rows', range: { from: '5012', to: '5020' }, waiting: { predicate: 'id==0x301', deadline: '+100ms' } };
    const a = readActivity(ev({ kind: 'core.activity', origin: 'agent', channel: 'ch0', payload: { ...p } }));
    expect(a).toMatchObject({ clientId: 'agent:claude-1', actor: 'agent:claude-1', method: 'can.trace.query', verb: 'can.trace.query', domain: 'can', channel: 'ch0', summary: '2 rows', waiting: 'id==0x301 · +100ms' });
    expect(a.range).toEqual({ seqA: 5012n, seqB: 5020n });
  });

  it('CAN answer {action, domain, channel, range{seq_a, seq_b}} (numbers) and the earlier {actor, verb}', () => {
    const c = readActivity(ev({ kind: 'core.activity', origin: 'agent', payload: { client_id: 'agent:x', action: 'read', domain: 'can', channel: 'ch1', range: { seq_a: 9, seq_b: 3 } } }));
    expect(c).toMatchObject({ verb: 'read', domain: 'can', channel: 'ch1', range: { seqA: 3n, seqB: 9n }, waiting: null });
    const o = readActivity(ev({ kind: 'core.activity', origin: 'user', domain: 'uart', payload: { actor: 'viewer:web', verb: 'input', domain: 'uart', channel: 'dut' } }));
    expect(o).toMatchObject({ actor: 'viewer:web', verb: 'input', method: '', domain: 'uart' });
  });

  it('a canonical method takes priority over the older verb', () => {
    expect(readActivity(ev({ kind: 'core.activity', payload: { method: 'uart.input', verb: 'old', summary: '' } })).verb).toBe('uart.input');
  });

  it('CAN Trace agent-read bar reads range{from,to} + method domain', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    const p: ActivityPayload = { client_id: 'agent:claude-1', method: 'can.trace.query', summary: '', range: { from: '10', to: '20' } };
    expect(m.applyActivity(ev({ kind: 'core.activity', origin: 'agent', channel: 'ch0', payload: { ...p } }), 'ch0', 0)).toBe(true);
    expect(m.agentReadAt(15)).toMatchObject({ a: 10, b: 20, actor: 'agent:claude-1' });
    expect(m.applyActivity(ev({ kind: 'core.activity', origin: 'agent', payload: { ...p, method: 'uart.query' } }), 'ch0', 0)).toBe(false);
  });
});

describe('core.safe_state list / run (C-8)', () => {
  it('CORE R1 envelope preserves explicit kinds and blocked interlocks', () => {
    const list: SafeStateList = { items: [
      { id: 'psu.off', domain: 'psu', method: 'psu.out', description: 'Output off', kind: 'emergency_off', blocked: true },
      { id: 'psu.emergency_reset', domain: 'psu', method: 'psu.reset', description: 'Emergency reset', kind: 'recover', blocked: false },
    ] };
    expect(readSafeStateList(list).map(({ id, kind, blocked }) => ({ id, kind, blocked }))).toEqual([
      { id: 'psu.off', kind: 'emergency_off', blocked: '인터록' },
      { id: 'psu.emergency_reset', kind: 'recover', blocked: null },
    ]);
  });
  const replayList: GenItem[] = [
    { id: 'uart.dut_break', domain: 'uart', channel: 'dut', description: 'UART dut: send break', method: 'uart.break', blocked: false },
    { id: 'can.ch0_silence', domain: 'can', channel: 'ch0', description: 'CAN ch0 bus off-line', method: 'can.stop', blocked: true },
    { id: 'psu.bench_emergency_off', domain: 'psu', channel: 'bench', description: 'PSU bench 비상 OFF', method: 'psu.out' },
  ];

  it('bare array (CORE replay): description → label, blocked:bool, kind from emergency wording', () => {
    const l = readSafeStateList(replayList);
    expect(l.map((i) => [i.id, i.kind, i.blocked, i.label, i.method])).toEqual([
      ['uart.dut_break', 'recover', null, 'UART dut: send break', 'uart.break'],
      ['can.ch0_silence', 'recover', '인터록', 'CAN ch0 bus off-line', 'can.stop'],
      ['psu.bench_emergency_off', 'emergency_off', null, 'PSU bench 비상 OFF', 'psu.out'],
    ]);
  });

  it('older {items:[{label, kind, blocked:string}]} still read; explicit kind wins', () => {
    const l = readSafeStateList({ items: [{ id: 'psu.bench_off', domain: 'psu', channel: 'bench', label: 'PSU off', kind: 'emergency_off', blocked: null }, { id: 'x', domain: 'uart', label: 'L', blocked: 'lease' }] });
    expect(l.map((i) => [i.kind, i.blocked])).toEqual([
      ['emergency_off', null],
      ['recover', 'lease'],
    ]);
  });

  it('run: schema/v1 {items[{outcome, reason, finally_blocked, request_id}]} and older {results[{ok}]}', () => {
    const r: SafeStateRunResult = {
      items: [
        { id: 'a', outcome: 'sent', request_id: 'r-1' },
        { id: 'b', outcome: 'skipped', reason: 'interlock: door open' },
        { id: 'c', outcome: 'failed', reason: 'timeout', finally_blocked: true },
      ],
    };
    expect(readSafeStateRun(r).map((x) => [x.id, x.ok, x.outcome, x.blocked, x.error, x.finallyBlocked, x.requestId])).toEqual([
      ['a', true, 'sent', null, null, false, 'r-1'],
      ['b', false, 'skipped', 'interlock: door open', null, false, null],
      ['c', false, 'failed', null, 'timeout', true, null],
    ]);
    expect(readSafeStateRun({ results: [{ id: 'z', ok: false, blocked: 'lease' }] })[0]).toMatchObject({ outcome: 'skipped', blocked: 'lease' });
  });

  it('StopPanel with the replay shapes: list shown, blocked visible, run results, missing emergency path said', async () => {
    const net = new FakeNet();
    net.open(TEST_HELLO);
    net.responders.set('core.safe_state.list', () => replayList.slice(0, 2));
    net.responders.set('core.safe_state.run', () => ({ items: [{ id: 'uart.dut_break', outcome: 'sent', request_id: 'r-9' }] }));
    const services = createShellServices(net, { autoTick: false });
    const unbind = services.bind();
    const host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <ShellCtx.Provider value={services}>
          <StopPanel />
        </ShellCtx.Provider>,
      ),
    );
    act(() => net.emitControl(ev({ kind: 'core.latch', payload: { on: true, missing: [], outputs_held: [{ domain: 'psu', channel: 'bench', label: '13.5 V', state: 'ON' }] } })));
    await act(async () => {});
    const panel = host.querySelector('[data-testid="stop-panel"]')!;
    expect(panel.textContent).toContain('UART dut: send break');
    expect(panel.textContent).toContain('⛔ 막힘: 인터록');
    expect(panel.textContent).toContain('(경로 없음)');
    await act(async () => host.querySelector<HTMLButtonElement>('.recover-btn')!.click());
    const res = panel.querySelector<HTMLElement>('[data-outcome="sent"]')!;
    expect(res.textContent).toContain('✓ 보냄');
    expect(res.dataset.requestId).toBe('r-9');
    act(() => root.unmount());
    unbind();
    host.remove();
  });
});
