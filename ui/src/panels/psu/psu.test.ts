// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

// uPlot reads matchMedia at module load; jsdom has none (lead: shared vitest setup requested)
vi.hoisted(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { psuFixture, rec } from './fixtures';
import './gaugePanel';
import { eng, gapCauseLabel, parseSetpoint, readMetrics, Series } from './psuModel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});
const btn = (t: string) => [...h!.host.querySelectorAll('button')].find((b) => b.textContent === t)!;
const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const txt = (id: string) => q(id).textContent!;
const flush = async () => {
  for (let k = 0; k < 5; k++) await Promise.resolve();
};
const type = (id: string, value: string) => {
  const i = q<HTMLInputElement>(id);
  i.value = value;
  i.dispatchEvent(new Event('input', { bubbles: true }));
};
const meas = (seq: number, t: number, m: { v?: number; i?: number; p?: number }) =>
  rec('psu.state', seq, t, {}, [
    ...(m.v !== undefined ? [{ name: 'voltage', value: m.v, unit: 'V' }] : []),
    ...(m.i !== undefined ? [{ name: 'current', value: m.i, unit: 'A' }] : []),
    ...(m.p !== undefined ? [{ name: 'power', value: m.p, unit: 'W' }] : []),
  ]);

describe('psu.gauge — Instrument View tile', () => {
  it('reconciles native status by channel without fabricating measurements, and stops polling on dispose', async () => {
    vi.useFakeTimers();
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.net.responders.set('psu.status', () => ({ data: { set_v: 12, set_i: 2, out: true, prot: 'ok', mode: 'cv' } }));
    await vi.advanceTimersByTimeAsync(1000);
    h.frame();
    expect(txt('psu-setpoint-v')).toContain('12.00 V');
    expect(txt('psu-out')).toContain('ON');
    expect(q('psu-meter-v').querySelector('.psu-num')!.textContent).toBe('—');
    expect(h.net.calls.filter((c) => c.method === 'psu.status').map((c) => c.params)).toEqual([{ ch: 'rail' }]);
    const calls = h.net.calls;
    const count = calls.length;
    h.dispose();
    h = null;
    await vi.advanceTimersByTimeAsync(2000);
    expect(calls).toHaveLength(count);
  });
  it('measured meter and setpoints are separate blocks; state chips come from psu.state', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture());
    expect(txt('psu-head')).toContain('● 출력 ON');
    expect(txt('psu-mode')).toBe('CV');
    expect(txt('psu-prot')).toBe('보호 정상');
    // targets
    expect(txt('psu-setpoint-v')).toContain('13.50 V');
    expect(txt('psu-setpoint-i')).toContain('2.00 A');
    // measured (dominant meter): V in volts, sub-1 A current in mA, reported power unmarked
    expect(q('psu-meter-v').querySelector('.psu-num')!.textContent).toMatch(/^13\.4\d\d$/);
    expect(q('psu-meter-v').querySelector('.psu-unit')!.textContent).toBe('V');
    expect(q('psu-meter-i').querySelector('.psu-num')!.textContent).toMatch(/^3\d\d\.\d$/);
    expect(q('psu-meter-i').querySelector('.psu-unit')!.textContent).toBe('mA');
    expect(q('psu-meter-p').querySelector('.psu-note')!.textContent).toBe('');
    // setpoint values are not inside the meter, measured values are not inside the setpoints
    expect(txt('psu-meter')).not.toContain('13.50');
    expect(txt('psu-setpoints')).not.toMatch(/13\.4\d\d/);
    // e2e contract: an agent set shows in psu-vals
    expect(txt('psu-vals')).toContain('13.50');
  });

  it('unknown stays dash: no samples, no state → "—", 출력 ?, no mode chip, toggle disabled', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    for (const k of ['v', 'i', 'p']) expect(q(`psu-meter-${k}`).querySelector('.psu-num')!.textContent).toBe('—');
    expect(txt('psu-meter')).not.toMatch(/\d/);
    expect(txt('psu-meter-age')).toBe('측정 없음');
    expect(txt('psu-setpoint-v')).toContain('— V');
    expect(txt('psu-out')).toBe('출력 ?');
    expect(q('psu-mode').hidden).toBe(true);
    expect(txt('psu-prot')).toBe('보호 ?');
    expect(q<HTMLButtonElement>('psu-output-toggle').disabled).toBe(true);
    expect(q('psu-output-toggle').getAttribute('aria-pressed')).toBe('mixed');
  });

  it('power derived from V×I is labelled (계산); CV/CC only when reported; protection warn only when reported', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([rec('psu.state', 1, 0, { set_v: 5, set_i: 1, out: false, prot: 'ok' }), meas(2, 1, { v: 5, i: 0.5 })]);
    expect(q('psu-meter-p').querySelector('.psu-note')!.textContent).toBe('(계산)');
    expect(q('psu-mode').hidden).toBe(true); // no mode in payload → not inferred from V/I
    h.batch([rec('psu.state', 3, 2, { mode: 'CC', prot: 'ocp' })]);
    expect(q('psu-mode').hidden).toBe(false);
    expect(txt('psu-mode')).toBe('CC');
    expect(txt('psu-prot')).toBe('보호 OCP');
    expect(q('psu-prot').classList.contains('warn')).toBe(true);
  });

  it('emergency OFF: allowed for watch, no confirmation, sends psu.out{state:"off"}; operate-only controls disabled', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' }, { hello: { rights: 'watch' } });
    h.batch(psuFixture(5));
    expect(btn('출력 OFF').disabled).toBe(true);
    expect(btn('적용').disabled).toBe(true);
    expect(q<HTMLInputElement>('psu-set-v').disabled).toBe(true);
    const off = btn('! 비상 OFF');
    expect(off.disabled).toBe(false);
    off.click();
    await Promise.resolve();
    expect(h.net.calls).toEqual([{ method: 'psu.out', params: { channel: 'bench', state: 'off' } }]);
  });

  it('`!` key on the focused panel triggers emergency OFF; `!` typed in a field does not', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    q('psu-set-v').dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await Promise.resolve();
    expect(h.net.calls).toEqual([]);
    h.host.querySelector<HTMLElement>('.psu-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await Promise.resolve();
    expect(h.net.calls.map((c) => c.method)).toEqual(['psu.out']);
  });

  it('ordinary output toggle (operate): sends the opposite state for this channel only', async () => {
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.batch([rec('psu.state', 1, 0, { set_v: 5, set_i: 1, out: false, prot: 'ok' })]);
    const t = q<HTMLButtonElement>('psu-output-toggle');
    expect(t.textContent).toBe('출력 ON');
    expect(t.getAttribute('aria-pressed')).toBe('false');
    t.click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'psu.out', params: { channel: 'rail', state: 'on' } }]);
  });

  it('STOP latched + output on → "STOP — 출력 유지 중" in the header, and STOP itself sends no psu.out', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(3));
    expect(h.host.querySelector<HTMLElement>('.psu-stop')!.hidden).toBe(true);
    h.shell.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: [] } }), 0);
    h.frame();
    expect(h.host.querySelector<HTMLElement>('.psu-stop')!.hidden).toBe(false);
    expect(q('psu-head').contains(q('psu-stop'))).toBe(true);
    expect(txt('psu-out')).toBe('● 출력 ON');
    expect(h.net.calls).toEqual([]);
  });

  it('typing never sends (incl. Enter); [적용] sends only the filled field; then clears the draft', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(3));
    expect(q<HTMLInputElement>('psu-set-v').placeholder).toBe('13.50');
    type('psu-set-v', '12');
    q('psu-set-v').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    h.frame();
    await flush();
    expect(h.net.calls).toEqual([]);
    expect(txt('psu-set-pending')).toContain('미적용');
    btn('적용').click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'psu.set', params: { channel: 'bench', v: 12 } }]);
    expect(q<HTMLInputElement>('psu-set-v').value).toBe('');
    expect(txt('psu-set-pending')).toBe('');
  });

  it('invalid or empty setpoints: flagged, toast, no RPC', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(3));
    btn('적용').click();
    type('psu-set-i', '-1');
    expect(q('psu-set-i').getAttribute('aria-invalid')).toBe('true');
    expect(txt('psu-set-pending')).toContain('0 이상');
    btn('적용').click();
    await flush();
    expect(h.net.calls).toEqual([]);
    expect(h.toasts).toEqual(['설정값을 확인하세요', '설정값을 확인하세요']);
  });

  it('SCPI no-response gap breaks the series (null point) and is counted', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture());
    expect(txt('psu-foot')).toContain('측정 손실 1회 (SCPI 무응답 1) — 선을 끊어 표시');
    expect(txt('psu-meter-age')).not.toContain('미갱신'); // samples resumed after the gap
  });

  it('gap footer reports the actual cause per gap (unknown/missing cause is not called SCPI); every gap breaks the lines', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([
      meas(1, 1, { v: 1, i: 0.1, p: 0.1 }),
      rec('source.gap', 2, 2, { count: 'unknown', cause: 'scpi_no_response' }),
      meas(3, 3, { v: 1, i: 0.1, p: 0.1 }),
      rec('source.gap', 4, 4, { count: 3, cause: 'ring_overflow' }),
      rec('source.gap', 5, 5, { count: 'unknown' }),
      meas(6, 6, { v: 1, i: 0.1, p: 0.1 }),
    ]);
    const foot = txt('psu-foot');
    expect(foot).toContain('측정 손실 3회 (SCPI 무응답 1, ring_overflow 1, 원인 미상 1)');
    expect(foot).toContain('6점'); // 3 samples + 3 null break points
  });

  it('a loss after the last sample keeps the last value but marks it stale (never zeroed)', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([meas(1, 1, { v: 3.3, i: 0.1, p: 0.33 }), rec('source.gap', 2, 2, { count: 'unknown', cause: 'scpi_no_response' })]);
    expect(q('psu-meter-v').querySelector('.psu-num')!.textContent).toBe('3.300');
    expect(txt('psu-meter-age')).toContain('측정 손실 후 미갱신');
    expect(q('psu-meter').classList.contains('stale')).toBe(true);
    h.batch([meas(3, 3, { v: 3.31, i: 0.1, p: 0.331 })]);
    expect(q('psu-meter').classList.contains('stale')).toBe(false);
  });

  it('60 s observation lanes: V/A shown, W hidden by default; toggles change display only', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(10));
    expect(txt('psu-trend')).toContain('60초 관측');
    expect(txt('psu-trend')).toContain('저장 기록 아님');
    expect(q('psu-lane-row-v').hidden).toBe(false);
    expect(q('psu-lane-row-p').hidden).toBe(true);
    const before = txt('psu-foot');
    q<HTMLButtonElement>('psu-lane-p').click();
    q<HTMLButtonElement>('psu-lane-v').click();
    h.frame();
    expect(q('psu-lane-row-p').hidden).toBe(false);
    expect(q('psu-lane-p').getAttribute('aria-pressed')).toBe('true');
    expect(q('psu-lane-row-v').hidden).toBe(true);
    expect(txt('psu-foot')).toBe(before);
    expect(h.net.calls).toEqual([]);
  });

  it('offers no unsupported control and no reference branding', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(3));
    const inputs = [...h.host.querySelectorAll('input')].map((i) => i.getAttribute('data-testid'));
    expect(inputs).toEqual(['psu-set-v', 'psu-set-i']);
    const labels = [...h.host.querySelectorAll('button')].map((b) => b.textContent);
    for (const l of labels) expect(l).not.toMatch(/All|OVP|OCP|기록 시작|로그|내보내기|Export|Range/i);
    expect(h.host.textContent).not.toMatch(/BenchVue|PathWave|Keysight/);
  });
});

describe('psu model', () => {
  it('Series keeps a 60 s window and inserts nulls at gaps in every lane', () => {
    const s = new Series();
    for (let t = 0; t <= 100; t++) s.push(t, 13.5, 0.3, 4.05);
    s.gap(101);
    const [t, v] = s.data();
    expect(t[0]).toBe(41);
    expect(v[v.length - 1]).toBeNull();
    expect(s.lane('p')[1].at(-1)).toBeNull();
    expect(s.lane('i')[1].length).toBe(t.length);
  });

  it('readMetrics flags derived power; eng/parseSetpoint never invent values', () => {
    expect(readMetrics(meas(1, 0, { v: 2, i: 0.5 }))).toEqual({ v: 2, i: 0.5, p: 1, pDerived: true });
    expect(readMetrics(meas(1, 0, { v: 2, i: 0.5, p: 0.9 }))?.pDerived).toBe(false);
    expect(readMetrics(meas(1, 0, { v: 2 }))).toEqual({ v: 2, i: null, p: null, pDerived: false });
    expect(eng(null, 'V')).toEqual({ num: '—', unit: 'V' });
    expect(eng(0, 'A')).toEqual({ num: '0.000', unit: 'A' });
    expect(eng(0.0042, 'A')).toEqual({ num: '4.200', unit: 'mA' });
    expect(parseSetpoint(' ')).toEqual({ ok: true, value: null });
    expect(parseSetpoint('1e1')).toEqual({ ok: true, value: 10 });
    expect(parseSetpoint('abc').ok).toBe(false);
    expect(parseSetpoint('-0.1').ok).toBe(false);
    expect(gapCauseLabel({ cause: 'scpi_no_response' })).toBe('SCPI 무응답');
    expect(gapCauseLabel({ cause: 'link_down' })).toBe('link_down');
    expect(gapCauseLabel({})).toBe('원인 미상');
    expect(gapCauseLabel(null)).toBe('원인 미상');
  });
});
