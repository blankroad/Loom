// @vitest-environment jsdom
// B3 spec (Opus): CAN-2 periodic send wired into the panel. FUNCTIONS.md §B3-RXTX / §B3-PANEL.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { CAN_TRACE_MANIFEST } from './canTrace';
import { registerCanPanels } from './index';
import { TxList } from './txList';

registerCanPanels();

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
});

const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`);
const all = (id: string) => [...h!.host.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  h!.frame();
};
const saved = (entries: { frame: string; cycle_ms: number }[]) =>
  localStorage.setItem(
    'loom.can.txlist.v1:ch0',
    JSON.stringify({ format: 'loom.can.txlist', version: 1, channel: 'ch0', saved_at: '2026-11-16T00:00:00.000Z', entries: entries.map((e) => ({ ...e, comment: '' })) }),
  );
const listJob = (n: number, o: Record<string, unknown> = {}) => ({
  job: `p${n}`, channel: 'ch0', id: 0x200 + n, ext: false, period_ms: 50, owner: 'agent-1#7', origin: 'agent',
  revision: 0, lease_left_ms: null, sent: 3, missed: 0, state: 'running', ...o,
});
const jobState = (job: string, state: string, o: Record<string, unknown> = {}) =>
  ev({ kind: 'can.job_state', channel: 'ch0', origin: 'user', payload: { job, channel: 'ch0', id: 0x300, ext: false, state, revision: 0, owner: 'ui', origin: 'user', ...o } });
const calls = (m: string) => h!.net.calls.filter((c) => c.method === m).map((c) => c.params);
const txRow = (i: number) => all('can-tx-row')[i]!;
const jobRow = (job: string) => h!.host.querySelector<HTMLElement>(`.crx-job[data-job="${job}"]`);

describe('TxList cycle (B3)', () => {
  it('upsert stores cycleMs when given, keeps it when omitted on an edit', () => {
    const t = new TxList();
    const p = { channel: 'ch0', id: 0x300, ext: false, fd: false, data: '01' } as Parameters<TxList['upsert']>[1];
    const r = t.upsert(null, p, '0x300#01', '', 100);
    expect(r.ok && t.get(r.uid)!.cycleMs).toBe(100);
    const uid = (r as { uid: number }).uid;
    const v = t.contentVersion;
    t.upsert(uid, p, '0x300#01', 'x');
    expect(t.get(uid)!.cycleMs).toBe(100);
    t.upsert(uid, p, '0x300#01', 'x', 0);
    expect(t.get(uid)!.cycleMs).toBe(0);
    expect(t.contentVersion).toBeGreaterThan(v);
    const r2 = t.upsert(null, p, '0x300#01', '');
    expect(r2.ok && t.get(r2.uid)!.cycleMs).toBe(0);
  });
});

describe('CAN-2 periodic send in the panel', () => {
  it('manifest: start/stop/takeover are job actions; stop is safety-reducing; takeover asks a card', () => {
    const a = (id: string) => CAN_TRACE_MANIFEST.actions.find((x) => x.id === id);
    expect(a('periodicStart')).toMatchObject({ rpc: 'can.periodic.start', class: 'job', right: 'operate', confirm: 'none' });
    expect(a('periodicStop')).toMatchObject({ rpc: 'can.periodic.stop', class: 'job', safetyDirection: 'reduce', confirm: 'none' });
    expect(a('periodicTakeover')).toMatchObject({ rpc: 'can.periodic.takeover', class: 'job', right: 'operate', confirm: 'card' });
  });

  it('unbound: no job table, no list read', () => {
    h = mountPanel('can.trace', { view: 'rxtx' });
    expect(tid('can-periodic')).toBeNull();
    expect(calls('can.periodic.list')).toEqual([]);
  });

  it('bound mount: one list read; jobs of other owners are read-only with 넘겨받기', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.periodic.list', () => ({ jobs: [listJob(2)] }));
    // the mount read went out before the responder was set; refresh reads again
    expect(calls('can.periodic.list')).toEqual([{ channel: 'ch0' }]);
    tid('can-periodic-refresh')!.click();
    await flush();
    expect(calls('can.periodic.list').length).toBe(2);
    expect(tid('can-periodic-status')!.textContent).toBe('목록 읽음 — 작업 1개');
    const r = jobRow('p2')!;
    expect(r.dataset.mine).toBe('false');
    expect(r.querySelector('[data-testid="can-job-stop"]')).toBeNull();
    expect(r.querySelector('[data-testid="can-job-takeover"]')).not.toBeNull();
  });

  it('start from a saved entry (Cycle Time > 0) → can.periodic.start; own job → stop; TX row shows the job', async () => {
    saved([{ frame: '300#0102', cycle_ms: 100 }, { frame: '301#01', cycle_ms: 0 }]);
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.periodic.start', () => ({ job: 'p7', revision: 0 }));
    h.net.responders.set('can.periodic.stop', () => ({ job: 'p7', state: 'stopped' }));
    expect(txRow(0).children[4]!.textContent).toBe('100 ms');
    expect(txRow(1).children[4]!.textContent).toBe('수동');
    const start = tid<HTMLButtonElement>('can-tx-periodic-start')!;
    const stop = tid<HTMLButtonElement>('can-tx-periodic-stop')!;
    txRow(1).click();
    h.frame();
    expect(start.disabled).toBe(true); // manual entry
    txRow(0).click();
    h.frame();
    expect(start.disabled).toBe(false);
    expect(stop.disabled).toBe(true);
    start.click();
    await flush();
    expect(calls('can.periodic.start')).toEqual([{ channel: 'ch0', frame: '300#0102', period_ms: 100 }]);
    expect(tid('can-periodic-status')!.textContent).toBe('주기 시작 p7 · 100 ms');
    h.net.emitControl(jobState('p7', 'started'));
    h.frame();
    expect(jobRow('p7')!.dataset.mine).toBe('true');
    expect(txRow(0).children[6]!.textContent).toBe('주기 p7 · 실행 중');
    expect(txRow(0).dataset.job).toBe('p7');
    expect(start.disabled).toBe(true); // already running
    expect(stop.disabled).toBe(false);
    stop.click();
    await flush();
    expect(calls('can.periodic.stop')).toEqual([{ job: 'p7' }]);
    expect(jobRow('p7')!.dataset.state).toBe('stopped');
    expect(txRow(0).children[6]!.textContent).toBe('주기 (정지)');
    expect(start.disabled).toBe(false);
  });

  it('a failed start says why and records no job', async () => {
    saved([{ frame: '300#01', cycle_ms: 10 }]);
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.periodic.start', () => {
      throw new Error('E_LOCKED id reserved');
    });
    txRow(0).click();
    h.frame();
    tid('can-tx-periodic-start')!.click();
    await flush();
    expect(tid('can-periodic-status')!.textContent).toBe('✗ 주기 시작 실패: E_LOCKED id reserved');
    expect(tid<HTMLButtonElement>('can-tx-periodic-stop')!.disabled).toBe(true);
  });

  it('editor Cycle Time: validated, saved with the entry', () => {
    saved([{ frame: '300#01', cycle_ms: 0 }]);
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    txRow(0).click();
    tid('can-tx-edit')!.click();
    const cyc = tid<HTMLInputElement>('can-tx-cycle')!;
    expect(cyc.value).toBe('0');
    cyc.value = '70000';
    cyc.dispatchEvent(new Event('input', { bubbles: true }));
    expect(tid<HTMLButtonElement>('can-tx-ok')!.disabled).toBe(true);
    expect(tid('can-tx-error')!.textContent).toBe('Cycle Time: 0(수동) 또는 1–60000 ms 정수');
    cyc.value = '250';
    cyc.dispatchEvent(new Event('input', { bubbles: true }));
    expect(tid<HTMLButtonElement>('can-tx-ok')!.disabled).toBe(false);
    tid('can-tx-ok')!.click();
    h.frame();
    expect(txRow(0).children[4]!.textContent).toBe('250 ms');
  });

  it('takeover → card-confirmed can.periodic.takeover; the new job is mine; list re-read', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.periodic.list', () => ({ jobs: [listJob(2)] }));
    tid('can-periodic-refresh')!.click();
    await flush();
    h.net.responders.set('can.periodic.takeover', () => ({ job: 'p9', previous_job: 'p2', revision: 0 }));
    h.net.responders.set('can.periodic.list', () => ({ jobs: [listJob(9, { owner: 'ui', origin: 'user' })] }));
    jobRow('p2')!.querySelector<HTMLButtonElement>('[data-testid="can-job-takeover"]')!.click();
    await flush();
    await flush();
    expect(calls('can.periodic.takeover')).toEqual([{ job: 'p2' }]);
    expect(calls('can.periodic.list').length).toBe(3);
    expect(jobRow('p9')!.dataset.mine).toBe('true');
    expect(tid('can-periodic-status')!.textContent).toMatch(/^목록 읽음|^넘겨받음: p2 → p9/);
  });

  it('STOP latch: banner, start disabled, own stop still possible', async () => {
    saved([{ frame: '300#01', cycle_ms: 10 }]);
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' }, { hello: { latch: true } });
    txRow(0).click();
    h.frame();
    expect(tid('can-periodic-stop')!.hidden).toBe(false);
    expect(tid<HTMLButtonElement>('can-tx-periodic-start')!.disabled).toBe(true);
  });
});
