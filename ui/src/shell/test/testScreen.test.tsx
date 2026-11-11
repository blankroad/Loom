// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { overrideEvent, SESSION, tc1Events, testRecord } from '../../panels/test/fixtures';
import type { Right } from '../../proto/provisional';
import { parseEvidenceRef } from '../../sdk/evidenceRef';
import { TestSessionStore } from '../../store/testSession';
import { ev, FakeNet, TEST_HELLO } from '../../store/testing/fakeNet';
import { createShellServices } from '../services';
import { ShellCtx } from '../ShellContext';
import { StopPanel } from '../StopPanel';
import { TestScreen } from './TestScreen';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let unbind: (() => void) | null = null;
beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  unbind?.();
  unbind = null;
  host.remove();
});

function mount(rights: Right = 'operate') {
  const net = new FakeNet();
  net.open({ ...TEST_HELLO, rights });
  const services = createShellServices(net, { autoTick: false });
  unbind = services.bind();
  act(() =>
    root.render(
      <ShellCtx.Provider value={services}>
        <StopPanel />
        <TestScreen />
      </ShellCtx.Provider>,
    ),
  );
  const feed = (evs = tc1Events()) => act(() => evs.forEach((e) => net.emitControl(e)));
  return { net, services, feed };
}
const row = (id: string) => host.querySelector<HTMLElement>(`[data-step-id="${id}"]`)!;
const btn = (t: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === t)!;

describe('TestSessionStore', () => {
  it('a rejected PASS never becomes the accepted verdict; history keeps both', () => {
    const s = new TestSessionStore(() => 0);
    s.applyMany(tc1Events());
    const s3 = s.step('S3')!;
    expect(s3.history.map((v) => (v.rejected ? `rejected:${v.rejected.code}` : v.effective))).toEqual(['rejected:E_OUT_OF_WINDOW', 'pass']);
    expect(s3.accepted?.effective).toBe('pass');
    expect(s.step('S1')!.accepted?.effective).toBe('done');
    expect(s.currentStepId).toBe('S5');
    expect(s.asks.get('q1')?.question).toContain('부팅 로고');
    expect(s.userInterventions).toBe(1);
  });

  it('late join via snapshot backlog reaches the same state; duplicates are ignored', () => {
    const live = new TestSessionStore(() => 0);
    live.applyMany(tc1Events());
    const late = new TestSessionStore(() => 0);
    late.applyMany(tc1Events()); // snapshot backlog
    late.applyMany(tc1Events()); // same events again on the control path
    const sig = (s: TestSessionStore) => s.steps.map((x) => `${x.id}:${x.status}:${x.accepted?.effective ?? '-'}:${x.history.length}:${x.evidence.length}`).join('|');
    expect(sig(late)).toBe(sig(live));
    expect(late.currentStepId).toBe(live.currentStepId);
    expect(late.userInterventions).toBe(1);
  });

  it('late join via reducer state object (snapshot_part.state)', () => {
    const s = new TestSessionStore(() => 0);
    s.applySnapshotState({ test: { session_id: SESSION, title: 'TC-1', state: 'RUNNING', current_step: 'S2', steps: [{ step_id: 'S1', n: 1, title: 'a', status: 'ended' }, { step_id: 'S2', n: 2, title: 'b', status: 'running' }] } });
    expect(s.sessionId).toBe(SESSION);
    expect(s.currentStep?.id).toBe('S2');
    expect(s.steps.map((x) => x.status)).toEqual(['ended', 'running']);
  });
});

describe('shell.test screen', () => {
  it('shows the rejected PASS (fabricated seq) next to the accepted verdict', () => {
    const { feed } = mount();
    feed();
    expect(row('S3').textContent).toContain('✓');
    act(() => row('S3').click());
    const detail = host.querySelector('[data-testid="test-detail"]')!;
    const rejected = detail.querySelector('[data-verdict-row="rejected"]')!;
    expect(rejected.textContent).toContain('PASS 거부 — E_OUT_OF_WINDOW: seq:17990 is before step S3 begin');
    expect(detail.querySelector('[data-verdict-row="accepted"]')!.textContent).toContain('원판정 pass / 확정 pass');
    expect(row('S4').dataset.verdict).toBe('fail');
    expect(row('S4').textContent).toContain('✗ 불일치');
  });

  it('override shows original and revised verdict side by side (by, previous)', () => {
    const { feed } = mount('approve');
    feed([...tc1Events(), overrideEvent()]);
    act(() => row('S4').click());
    const ov = host.querySelector('[data-verdict-row="override"]')!;
    expect(ov.textContent).toContain('원판정 fail / 확정 inconclusive / 수정 by viewer:tab-1 (이전 fail by claude-1)');
    expect(ov.textContent).toContain('사유: DTC는 이전 세션 잔여');
  });

  it('watch/operate cannot override; approve can, with a mandatory reason', () => {
    for (const r of ['watch', 'operate'] as const) {
      const { feed } = mount(r);
      feed();
      expect(btn('판정 수정…').disabled).toBe(true);
      act(() => root.render(<></>));
      unbind?.();
    }
    const { net, feed } = mount('approve');
    feed();
    act(() => row('S4').click());
    act(() => btn('판정 수정…').click());
    const form = host.querySelector<HTMLFormElement>('[data-testid="reason-form"]')!;
    const submit = [...form.querySelectorAll('button')].find((b) => b.type === 'submit')!;
    expect(submit.disabled).toBe(true); // reason required
    const input = form.querySelector<HTMLInputElement>('input')!;
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      setter.call(input, '재확인 결과 PASS');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const sel = form.querySelector<HTMLSelectElement>('select')!;
    act(() => {
      sel.value = 'pass';
      sel.dispatchEvent(new Event('change', { bubbles: true }));
    });
    act(() => form.requestSubmit());
    return act(async () => {}).then(() => {
      const call = net.calls.find((c) => c.method === 'test.verdict')!;
      expect(call.params).toEqual({ session: SESSION, step: 'S4', verdict: 'pass', override: true, reason: '재확인 결과 PASS' });
    });
  });

  it('ask dialog: docked (not the approval card), answers with test.answer, closes on test.answer event', async () => {
    const { net, feed } = mount();
    feed();
    const dock = host.querySelector<HTMLElement>('[data-testid="ask-dock"]')!;
    expect(dock.className).toBe('ask-dock');
    expect(dock.closest('.approval-overlay')).toBeNull();
    expect(dock.textContent).toContain('남은 120s');
    await act(async () => btn('예').click());
    expect(net.calls.find((c) => c.method === 'test.answer')!.params).toEqual({ session: SESSION, ask_id: 'q1', value: '예' });
    act(() => net.emitControl(testRecord('test.answer', 300, { ask_id: 'q1', value: '예' }, { origin: 'user' })));
    expect(host.querySelector('[data-testid="ask-dock"]')).toBeNull();
  });

  it('STOP: banner says finally is not run automatically, points to recovery; no test.* RPC is sent', async () => {
    const { net, feed } = mount();
    net.responders.set('core.safe_state.list', () => ({ items: [{ id: 'psu-off', domain: 'psu', channel: 'bench', label: 'psu.out bench off', kind: 'recover' }] }));
    feed();
    act(() => net.emitControl(ev({ kind: 'core.latch', payload: { on: true, missing: [], outputs_held: [{ domain: 'psu', channel: 'bench', label: '13.5 V', state: 'ON' }] } })));
    await act(async () => {});
    expect(host.querySelector('[data-testid="test-stop-banner"]')!.textContent).toContain('finally는 자동 실행하지 않습니다');
    expect(host.querySelector('.recover-btn')).not.toBeNull();
    expect(net.calls.filter((c) => c.method.startsWith('test.'))).toEqual([]);
  });

  it('REC FAILED → "PASS 판정 불가" banner; evidence column is honest when the verifier is absent', () => {
    const { net, feed } = mount();
    feed(tc1Events().filter((e) => e.kind !== 'test.evidence_checked'));
    expect(row('S2').textContent).toContain('검증기 P2');
    act(() => net.emitControl(ev({ kind: 'core.heartbeat', payload: { record_health: 'failed', latch: false } })));
    expect(host.querySelector('[data-testid="test-rec-failed"]')!.textContent).toContain('PASS 판정 불가');
  });

  it('[증거 열기] reveals the cited REF', () => {
    const { services, feed } = mount();
    const seen: string[] = [];
    services.uiHooks.onReveal?.((r) => {
      seen.push(`${r.domain}:${r.channel}:${r.epoch}:${r.seqA}`);
      return true;
    });
    feed();
    act(() => row('S2').click());
    act(() => [...host.querySelectorAll('button')].find((b) => b.textContent === '증거 열기')!.click());
    expect(seen).toEqual(['uart:dut:1:18231']);
  });
});

describe('evidence REF (contract 07 §7.2)', () => {
  it('parses the canonical @e<epoch>:seq form with range and pointer', () => {
    expect(parseEvidenceRef('uart:dut@e3:seq:10..12#/payload/state')).toEqual({
      domain: 'uart', channel: 'dut', epoch: 3n, seqA: 10n, seqB: 12n, requestId: null, pointer: '/payload/state',
    });
  });
  it('keeps the epoch-less @seq and the Loom @req forms', () => {
    expect(parseEvidenceRef('can:ch0@seq:5530')).toMatchObject({ epoch: null, seqA: 5530n, seqB: null });
    expect(parseEvidenceRef('uds:*@req:t-000418')).toMatchObject({ channel: '*', requestId: 't-000418' });
  });
  it('rejects bad pointers, inverted ranges and over-long refs', () => {
    expect(parseEvidenceRef('uart:dut@e1:seq:1#/a~2b')).toBeNull();
    expect(parseEvidenceRef('uart:dut@e1:seq:1#no-slash')).toBeNull();
    expect(parseEvidenceRef('uart:dut@seq:9..3')).toBeNull();
    expect(parseEvidenceRef(`uart:${'x'.repeat(600)}@seq:1`)).toBeNull();
    expect(parseEvidenceRef('Uart:dut@seq:1')).toBeNull();
  });
});
