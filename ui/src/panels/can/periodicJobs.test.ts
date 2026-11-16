// B3 spec (Opus): CAN-2 job table + helpers. FUNCTIONS.md §B3-JOBS.
import { describe, expect, it } from 'vitest';
import { JobTable, STOPPED_KEEP, jobNumber, jobStateText, parseCycle, startParams } from './periodicJobs';

// shape of crates/loom-can periodic.rs job_json
const job = (n: number, o: Record<string, unknown> = {}) => ({
  job: `p${n}`, channel: 'ch0', id: 0x100 + n, ext: false, fd: false, data: '01', period_ms: 100, owner: `cli-${n}`,
  origin: 'agent', revision: 2, applied_revision: 2, count_left: null, lease_left_ms: 5000, sent: 10, missed: 1,
  lateness_p99_us: 12, state: 'running', ...o,
});
const st = (n: number, state: string, o: Record<string, unknown> = {}, channel = 'ch0') => ({
  kind: 'can.job_state', channel,
  payload: { job: `p${n}`, channel, id: 0x100 + n, ext: false, state, revision: 0, owner: `cli-${n}`, origin: 'user', ...o },
});

describe('helpers', () => {
  it('jobNumber', () => {
    expect(jobNumber('p12')).toBe(12);
    expect(jobNumber('x')).toBe(Number.MAX_SAFE_INTEGER);
  });
  it('jobStateText', () => {
    expect(jobStateText({ state: 'running', reason: null })).toBe('실행 중');
    expect(jobStateText({ state: 'paused', reason: 'stop_latch' })).toBe('일시정지 (stop_latch)');
    expect(jobStateText({ state: 'stopped', reason: null })).toBe('정지됨');
    expect(jobStateText({ state: 'stopped', reason: 'lease_expired' })).toBe('정지됨 (lease_expired)');
    expect(jobStateText({ state: 'odd', reason: 'r' })).toBe('odd (r)');
  });
  it('parseCycle', () => {
    expect(parseCycle('')).toEqual({ ok: true, ms: 0 });
    expect(parseCycle(' 0 ')).toEqual({ ok: true, ms: 0 });
    expect(parseCycle('100')).toEqual({ ok: true, ms: 100 });
    expect(parseCycle('60000')).toEqual({ ok: true, ms: 60000 });
    for (const bad of ['60001', '-1', '1.5', 'abc', '1e3']) {
      expect(parseCycle(bad)).toEqual({ ok: false, error: 'Cycle Time: 0(수동) 또는 1–60000 ms 정수' });
    }
  });
  it('startParams: cansend frame text + period', () => {
    expect(startParams({ params: { id: 0x300, ext: false, fd: false, brs: false, data: '0102' }, cycleMs: 100 }, 'ch0')).toEqual({
      ok: true, params: { channel: 'ch0', frame: '300#0102', period_ms: 100 },
    });
    expect(startParams({ params: { id: 0x18daf110, ext: true, fd: true, brs: true, data: 'aa' }, cycleMs: 20 }, 'ch0')).toEqual({
      ok: true, params: { channel: 'ch0', frame: '18DAF110##1AA', period_ms: 20 },
    });
    expect(startParams({ params: { id: 1, ext: false, fd: false, brs: false, data: '' }, cycleMs: 0 }, 'ch0')).toEqual({
      ok: false, error: '주기 0 = 수동 송신 — Cycle Time(ms)을 1–60000으로 정하세요',
    });
  });
});

describe('JobTable', () => {
  it('list read: rows of the channel, all owners, stats kept; bad shape leaves it unchanged', () => {
    const t = new JobTable();
    expect(t.loaded).toBe(false);
    expect(t.applyList({ jobs: [job(2), job(1, { state: 'paused', lease_left_ms: null, sent: null }), job(3, { channel: 'ch1' }), { junk: 1 }] }, 'ch0')).toBe(true);
    expect(t.loaded).toBe(true);
    expect(t.list().map((r) => r.job)).toEqual(['p1', 'p2']);
    expect(t.get('p2')).toEqual({
      job: 'p2', id: 0x102, ext: false, owner: 'cli-2', origin: 'agent', periodMs: 100, state: 'running', revision: 2,
      sent: 10, missed: 1, leaseLeftMs: 5000, reason: null,
    });
    expect(t.get('p1')).toMatchObject({ state: 'paused', leaseLeftMs: null, sent: null });
    const v = t.version;
    expect(t.applyList({ nope: [] }, 'ch0')).toBe(false);
    expect(t.applyList(null, 'ch0')).toBe(false);
    expect(t.version).toBe(v);
    expect(t.list().length).toBe(2);
  });

  it('events: started adds a row (period unknown), stopped keeps it with the reason, a later list drops dead active rows', () => {
    const t = new JobTable();
    expect(t.applyEvent(st(5, 'started'), 'ch0')).toBe(true);
    expect(t.get('p5')).toMatchObject({ state: 'running', periodMs: null, owner: 'cli-5', origin: 'user', sent: null });
    expect(t.applyEvent(st(5, 'stopped', { reason: 'channel_closed' }), 'ch0')).toBe(true);
    expect(t.get('p5')).toMatchObject({ state: 'stopped', reason: 'channel_closed' });
    expect(t.applyEvent(st(6, 'started', {}, 'ch1'), 'ch0')).toBe(false);
    expect(t.applyEvent({ kind: 'can.frame', channel: 'ch0', payload: {} }, 'ch0')).toBe(false);
    expect(t.applyEvent({ kind: 'can.job_state', channel: 'ch0', payload: { state: 'started' } }, 'ch0')).toBe(false);
    t.applyEvent(st(7, 'started'), 'ch0');
    t.applyList({ jobs: [job(8)] }, 'ch0');
    // p7 (active, not in the list any more) is gone; p5 (stopped) is kept; p8 from the list
    expect(t.list().map((r) => r.job)).toEqual(['p8', 'p5']);
  });

  it('patched/renewed keep the list stats; paused/resumed', () => {
    const t = new JobTable();
    t.applyList({ jobs: [job(1)] }, 'ch0');
    t.applyEvent(st(1, 'patched', { revision: 3, origin: 'agent', owner: 'cli-1' }), 'ch0');
    expect(t.get('p1')).toMatchObject({ state: 'running', revision: 3, periodMs: 100, sent: 10 });
    t.applyEvent(st(1, 'paused', { reason: 'stop_latch' }), 'ch0');
    expect(t.get('p1')).toMatchObject({ state: 'paused', reason: 'stop_latch' });
    t.applyEvent(st(1, 'started', { reason: 'resumed' }), 'ch0');
    expect(t.get('p1')).toMatchObject({ state: 'running', reason: 'resumed' });
  });

  it(`keeps at most ${STOPPED_KEEP} stopped rows, newest first`, () => {
    const t = new JobTable();
    for (let n = 1; n <= STOPPED_KEEP + 4; n++) {
      t.applyEvent(st(n, 'started'), 'ch0');
      t.applyEvent(st(n, 'stopped'), 'ch0');
    }
    const l = t.list();
    expect(l.length).toBe(STOPPED_KEEP);
    expect(l[0]!.job).toBe(`p${STOPPED_KEEP + 4}`);
    expect(t.get('p1')).toBeUndefined();
  });
});
