// @vitest-environment jsdom
// B3 spec (Opus): CAN-2 job table view. FUNCTIONS.md §B3-VIEW.
import { describe, expect, it, vi } from 'vitest';
import { JobTable } from './periodicJobs';
import { PeriodicView } from './periodicView';

const job = (n: number, o: Record<string, unknown> = {}) => ({
  job: `p${n}`, channel: 'ch0', id: 0x100 + n, ext: false, period_ms: 100, owner: `cli-${n}`, origin: 'agent',
  revision: 0, lease_left_ms: 4200, sent: 10, missed: 1, state: 'running', ...o,
});
const setup = (mine: string[] = []) => {
  const t = new JobTable();
  const hooks = { isMine: (j: string) => mine.includes(j), stop: vi.fn(), takeover: vi.fn(), refresh: vi.fn() };
  const v = new PeriodicView(t, hooks);
  document.body.append(v.root);
  return { t, v, hooks };
};
const q = <T extends HTMLElement = HTMLElement>(v: PeriodicView, id: string) => v.root.querySelector<T>(`[data-testid="${id}"]`)!;
const rows = (v: PeriodicView) => [...v.root.querySelectorAll<HTMLElement>('.crx-job')];

describe('PeriodicView', () => {
  it('not read yet vs empty', () => {
    const { t, v, hooks } = setup();
    expect(v.root.dataset.testid).toBe('can-periodic');
    v.render();
    expect(q(v, 'can-periodic-empty').textContent).toBe('목록을 아직 읽지 않음 — 새로고침');
    t.applyList({ jobs: [] }, 'ch0');
    v.render();
    expect(q(v, 'can-periodic-empty').textContent).toBe('이 채널에 데몬 주기 송신 작업 없음');
    expect(q(v, 'can-periodic-empty').hidden).toBe(false);
    q(v, 'can-periodic-refresh').click();
    expect(hooks.refresh).toHaveBeenCalledTimes(1);
  });

  it('rows: own job → 정지; other owner → 넘겨받기 (read-only otherwise); stopped → no action', () => {
    const { t, v, hooks } = setup(['p1']);
    t.applyList({ jobs: [job(1), job(2, { owner: 'agent-x#3', ext: true, id: 0x18daf110, sent: null, lease_left_ms: null })] }, 'ch0');
    t.applyEvent({ kind: 'can.job_state', channel: 'ch0', payload: { job: 'p3', state: 'stopped', reason: 'lease_expired', owner: 'o', origin: 'user', id: 1, ext: false } }, 'ch0');
    v.render();
    const r = rows(v);
    expect(r.map((e) => e.dataset.job)).toEqual(['p1', 'p2', 'p3']);
    const cells = (e: HTMLElement) => [...e.children].map((c) => c.textContent);
    expect(cells(r[0]!)).toEqual(['p1', '0x101', '100', 'agent cli-1', '실행 중', '10 / 1', '4.2 s', '정지']);
    expect(cells(r[1]!)).toEqual(['p2', '18DAF110x', '100', 'agent agent-x#3', '실행 중', '? / 1', '—', '넘겨받기']);
    expect(cells(r[2]!)).toEqual(['p3', '0x001', '?', 'user o', '정지됨 (lease_expired)', '? / ?', '—', '']);
    expect(r[0]!.dataset.mine).toBe('true');
    expect(r[1]!.dataset.mine).toBe('false');
    expect(r[1]!.title).toContain('다른 소유자');
    r[0]!.querySelector<HTMLButtonElement>('[data-testid="can-job-stop"]')!.click();
    expect(hooks.stop).toHaveBeenCalledWith('p1');
    r[1]!.querySelector<HTMLButtonElement>('[data-testid="can-job-takeover"]')!.click();
    expect(hooks.takeover).toHaveBeenCalledWith('p2');
    expect(r[1]!.querySelector('[data-testid="can-job-stop"]')).toBeNull();
  });

  it('not writable: takeover disabled, stop still enabled; STOP banner', () => {
    const { t, v } = setup(['p1']);
    t.applyList({ jobs: [job(1), job(2)] }, 'ch0');
    v.setWritable(false);
    v.render();
    expect(v.root.querySelector<HTMLButtonElement>('[data-testid="can-job-takeover"]')!.disabled).toBe(true);
    expect(v.root.querySelector<HTMLButtonElement>('[data-testid="can-job-stop"]')!.disabled).toBe(false);
    expect(q(v, 'can-periodic-stop').hidden).toBe(true);
    v.setLatched(true);
    expect(q(v, 'can-periodic-stop').hidden).toBe(false);
    expect(q(v, 'can-periodic-stop').textContent).toBe('STOP — 주기 송신 정지됨 (새 시작 불가 · 정지는 가능)');
  });

  it('status line; render only redraws on change unless forced', () => {
    const { t, v } = setup();
    v.setStatus('목록 읽기 실패: x', true);
    expect(q(v, 'can-periodic-status').textContent).toBe('목록 읽기 실패: x');
    expect(q(v, 'can-periodic-status').classList.contains('err')).toBe(true);
    t.applyList({ jobs: [job(1)] }, 'ch0');
    v.render();
    const before = rows(v)[0]!;
    v.render();
    expect(rows(v)[0]).toBe(before);
    v.render(true);
    expect(rows(v).length).toBe(1);
  });

  it('device text is never parsed as HTML', () => {
    const { t, v } = setup();
    t.applyList({ jobs: [job(1, { owner: '<img src=x onerror=alert(1)>' })] }, 'ch0');
    v.render();
    expect(v.root.querySelector('img')).toBeNull();
    expect(rows(v)[0]!.children[3]!.textContent).toBe('agent <img src=x onerror=alert(1)>');
  });
});
