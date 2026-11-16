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
import {
  addPage,
  coverageText,
  CSV_MAX_ROWS,
  csvText,
  dateClock,
  historyParams,
  isComplete,
  mergeHistStat,
  readHistoryPage,
  runHistory,
  type HistoryAgg,
} from './psuHistory';
import './gaugePanel';
import {
  chartColors,
  checkSetpoint,
  clockLabel,
  eng,
  gapCauseLabel,
  limitNum,
  limitText,
  modeLabel,
  parseSetpoint,
  protLabel,
  readMetrics,
  readoutAt,
  readoutText,
  reduceState,
  Series,
  wallS,
  xTickLabels,
  yRange,
  type PsuState,
} from './psuModel';

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
    expect(txt('psu-prot')).toBe('보호 미보고'); // PSU-05: readable, not a bare '?'
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

  it('emergency OFF: allowed for watch, no confirmation, sends the fixed-OFF psu.off{channel} (B5); operate-only controls disabled', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' }, { hello: { rights: 'watch' } });
    h.batch(psuFixture(5));
    expect(btn('출력 OFF').disabled).toBe(true);
    expect(btn('적용').disabled).toBe(true);
    expect(q<HTMLInputElement>('psu-set-v').disabled).toBe(true);
    const off = btn('! 비상 OFF');
    expect(off.disabled).toBe(false);
    // B5-U2: truthful tooltip — no unconditional OFF promise (policy may refuse; interlocks only warn)
    expect(off.title).toBe('bench 채널만 확인 없이 출력 OFF 요청 (보기 자격 가능 · OFF 전용 · 인터록은 경고만) — 정책이 막으면 거부될 수 있습니다');
    off.click();
    await Promise.resolve();
    // B5 (ROOT-UI-OFF-FAILURE.md): psu.out needs operate on the server; the watch action is psu.off, OFF only.
    expect(h.net.calls).toEqual([{ method: 'psu.off', params: { channel: 'bench' } }]);
  });

  it('`!` key on the focused panel triggers emergency OFF; `!` typed in a field does not', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    q('psu-set-v').dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await Promise.resolve();
    expect(h.net.calls).toEqual([]);
    h.host.querySelector<HTMLElement>('.psu-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await Promise.resolve();
    expect(h.net.calls.map((c) => c.method)).toEqual(['psu.off']);
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
    expect(txt('psu-trend')).toContain('60 s 화면 창 (긴 구간은 아래 기록 조회)'); // PSU-03: no longer "not stored"
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
    expect(inputs).toEqual(['psu-set-v', 'psu-set-i', 'psu-cycle-ms', 'psu-hist-from', 'psu-hist-to']);
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

describe('psu.gauge — D-P1 provenance, D-P2 disconnect keeps last confirmed output visible', () => {
  const ext = { ch: 'bench', backend: 'scpi-tcp', address: '127.0.0.1:54374', idn: 'LOOM,LOOM-TEST-FIXTURE,0,1', set_v: 12, set_i: 1, out: true, out_known: true, connected: true, link: 'up' };
  it('header shows env as environment plus the real backend/address/IDN', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([rec('psu.state', 10, 0, ext)]);
    expect(txt('psu-env')).toBe('환경 SIM');
    expect(txt('psu-source')).toBe('scpi-tcp · 127.0.0.1:54374 · IDN LOOM,LOOM-TEST-FIXTURE,0,1');
    expect(txt('psu-out')).toBe('● 출력 ON');
  });
  it('live disconnect (control path): last confirmed ON stays with a stale qualifier, writes disabled, never shown OFF', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    const live = rec('psu.state', 10, 0, ext);
    h.batch([live]);
    expect(q<HTMLButtonElement>('psu-output-toggle').disabled).toBe(false);
    h.net.emitControl(rec('psu.state', 11, 1, { ...ext, connected: false, link: 'lost' }));
    h.frame();
    expect(txt('psu-out')).toContain('출력 ON');
    expect(txt('psu-out')).toContain('연결 끊김');
    expect(txt('psu-out')).not.toContain('OFF');
    expect(q<HTMLButtonElement>('psu-output-toggle').disabled).toBe(true);
    expect(q<HTMLButtonElement>('psu-emergency').disabled).toBe(false); // safety direction kept
    // an older backlog row (connected) delivered later does not hide the disconnect
    h.batch([live]);
    expect(txt('psu-out')).toContain('연결 끊김');
    btn('출력 OFF').click();
    expect(h.net.calls.filter((c) => c.method === 'psu.out')).toHaveLength(0);
  });
  it('out_known:false is unknown, never OFF', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([rec('psu.state', 10, 0, { ...ext, out: false, out_known: false })]);
    expect(txt('psu-out')).toBe('출력 ?');
  });
});

// ---------------------------------------------------------------------------------------------
// Operator upgrade batch 1 (work/reports/operator-upgrade/psu/FUNCTIONS.md). Spec tests — Sonnet
// implements the bodies; these assertions are not to be weakened.

const EMPTY: PsuState = { setV: null, setI: null, out: null, prot: null, mode: null };
const settled = (extra: Record<string, unknown> = {}) => rec('psu.state', 1, 0, { set_v: 5, set_i: 1, out: false, prot: 'ok', ...extra });

describe('PSU-01 active limits in the gauge', () => {
  it('setpoint block shows the reported limits; unknown limits say 미보고 (never the profile default guessed)', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([settled()]);
    expect(txt('psu-limits')).toContain('한도');
    expect(txt('psu-limits')).toContain('미보고');
    expect(txt('psu-limits')).not.toContain('16');
    h.batch([rec('psu.state', 2, 1, { vmax: 9, imax: 2.5 })]);
    expect(txt('psu-limits')).toContain('9.00 V / 2.50 A');
    expect(q('psu-setpoints').contains(q('psu-limits'))).toBe(true);
  });

  it('over-limit input is flagged before [적용] and never sent; a value equal to the limit is sent', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([settled({ vmax: 9, imax: 2 })]);
    type('psu-set-v', '13.5');
    expect(q('psu-set-v').getAttribute('aria-invalid')).toBe('true');
    expect(q('psu-set-i').getAttribute('aria-invalid')).toBe('false');
    expect(txt('psu-set-pending')).toBe('13.5 V — 한도 9.00 V 초과');
    expect(q('psu-set-pending').classList.contains('err')).toBe(true);
    btn('적용').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.set')).toEqual([]);
    expect(h.toasts).toEqual(['설정값을 확인하세요']);
    type('psu-set-v', '9');
    type('psu-set-i', '2.0001');
    expect(q('psu-set-v').getAttribute('aria-invalid')).toBe('false');
    expect(txt('psu-set-pending')).toBe('2.0001 A — 한도 2.00 A 초과');
    type('psu-set-i', '');
    btn('적용').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.set')).toEqual([{ method: 'psu.set', params: { channel: 'bench', v: 9 } }]);
  });

  it('limit not reported: no client-side limit check — the gateway stays the final guard', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([settled()]);
    type('psu-set-v', '100');
    expect(q('psu-set-v').getAttribute('aria-invalid')).toBe('false');
    btn('적용').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.set')).toEqual([{ method: 'psu.set', params: { channel: 'bench', v: 100 } }]);
  });

  it('limits from psu.status (scpi-tcp/sim status payload) reach the gauge too', async () => {
    vi.useFakeTimers();
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.net.responders.set('psu.status', () => ({ data: { set_v: 12, set_i: 1, out: false, prot: 'unknown', mode: 'off', vmax: 12, imax: 3 } }));
    await vi.advanceTimersByTimeAsync(1000);
    h.frame();
    expect(txt('psu-limits')).toContain('12.00 V / 3.00 A');
  });
});

describe('PSU-02 trend readout', () => {
  it('readout shows the latest sample with its wall clock, values and the reported setpoints', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    expect(txt('psu-readout')).toBe('측정 없음');
    h.batch(psuFixture(10));
    expect(txt('psu-readout')).toMatch(/^최신 \d\d:\d\d:\d\d\.\d{3} · 13\.4\d\d V · 3\d\d\.\d mA · \d\.\d{3} W · 설정 13\.50 V \/ 2\.00 A$/);
    expect(q('psu-trend').contains(q('psu-readout'))).toBe(true);
  });

  it('a loss as the newest row reads as a loss, never as zero', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([meas(1, 1, { v: 3.3, i: 0.1, p: 0.33 }), rec('source.gap', 2, 2, { count: 'unknown', cause: 'scpi_no_response' })]);
    expect(txt('psu-readout')).toMatch(/^최신 \d\d:\d\d:\d\d\.\d{3} · 측정 손실 \(선 끊김\)$/);
  });
});

describe('PSU-05 mode / protection labels', () => {
  it('mode "off" duplicates the output word → hidden; "unknown" → 모드 미보고; chips carry an accessible name', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([settled({ mode: 'off', prot: 'unknown' })]);
    expect(q('psu-mode').hidden).toBe(true);
    expect(txt('psu-prot')).toBe('보호 미보고');
    expect(q('psu-prot').getAttribute('aria-label')).toBe('보호 상태: 보호 미보고');
    h.batch([rec('psu.state', 2, 1, { out: true, mode: 'unknown' })]);
    expect(q('psu-mode').hidden).toBe(false);
    expect(txt('psu-mode')).toBe('모드 미보고');
    expect(q('psu-mode').getAttribute('aria-label')).toBe('동작 모드: 모드 미보고');
    h.batch([rec('psu.state', 3, 2, { mode: 'cc' })]);
    expect(txt('psu-mode')).toBe('CC');
    expect(q('psu-mode').getAttribute('aria-label')).toBe('동작 모드: CC');
  });
});

describe('psu model — operator upgrade batch 1', () => {
  it('reduceState keeps limits/cycling across partial updates and ignores invalid values', () => {
    let s = reduceState(EMPTY, rec('psu.state', 1, 0, { vmax: 9, imax: 2, cycling: true }));
    expect([s.vmax, s.imax, s.cycling]).toEqual([9, 2, true]);
    s = reduceState(s, rec('psu.state', 2, 1, { set_v: 5 }));
    expect([s.vmax, s.imax, s.cycling]).toEqual([9, 2, true]);
    s = reduceState(s, rec('psu.state', 3, 2, { vmax: 0, imax: 'x', cycling: 'yes' }));
    expect([s.vmax, s.imax, s.cycling]).toEqual([9, 2, true]);
    s = reduceState(s, rec('psu.state', 4, 3, { vmax: -1, imax: Number.NaN, cycling: false }));
    expect([s.vmax, s.imax, s.cycling]).toEqual([9, 2, false]);
    expect(reduceState(EMPTY, rec('psu.state', 1, 0, {})).vmax ?? null).toBeNull();
  });

  it('limitNum / limitText', () => {
    expect(limitNum(16)).toBe('16.00');
    expect(limitNum(2.5)).toBe('2.50');
    expect(limitNum(9.005)).toBe('9.005');
    expect(limitNum(0.1)).toBe('0.10');
    expect(limitText(EMPTY)).toBe('미보고');
    expect(limitText({ ...EMPTY, vmax: 9, imax: null })).toBe('9.00 V / — A');
    expect(limitText({ ...EMPTY, vmax: 9, imax: 2.5 })).toBe('9.00 V / 2.50 A');
  });

  it('checkSetpoint: empty = absent, strict > limit (equal passes), syntax errors keep the old message', () => {
    expect(checkSetpoint('', 9, 'V')).toEqual({ ok: true, value: null });
    expect(checkSetpoint(' 9 ', 9, 'V')).toEqual({ ok: true, value: 9 });
    expect(checkSetpoint('9.0000001', 9, 'V')).toEqual({ ok: false, reason: 'over', message: '9.0000001 V — 한도 9.00 V 초과' });
    expect(checkSetpoint('2.6', 2.5, 'A')).toEqual({ ok: false, reason: 'over', message: '2.6 A — 한도 2.50 A 초과' });
    expect(checkSetpoint('-1', 9, 'V')).toEqual({ ok: false, reason: 'negative', message: '0 이상의 숫자를 입력하세요' });
    expect(checkSetpoint('abc', null, 'A')).toEqual({ ok: false, reason: 'nan', message: '0 이상의 숫자를 입력하세요' });
    expect(checkSetpoint('100', null, 'V')).toEqual({ ok: true, value: 100 });
  });

  it('modeLabel / protLabel', () => {
    expect(modeLabel(null)).toBeNull();
    expect(modeLabel('off')).toBeNull();
    expect(modeLabel('OFF')).toBeNull();
    expect(modeLabel('unknown')).toBe('모드 미보고');
    expect(modeLabel('cv')).toBe('CV');
    expect(modeLabel(' cc ')).toBe('CC');
    expect(modeLabel('foo')).toBe('foo');
    expect(protLabel(null)).toEqual({ text: '보호 미보고', warn: false });
    expect(protLabel('unknown')).toEqual({ text: '보호 미보고', warn: false });
    expect(protLabel('ok')).toEqual({ text: '보호 정상', warn: false });
    expect(protLabel('ovp')).toEqual({ text: '보호 OVP', warn: true });
  });

  it('yRange never falls back to 0–100 for flat or zero data', () => {
    expect(yRange(0, 0, 16)).toEqual([0, 16]);
    expect(yRange(0, 0, null)).toEqual([0, 1]);
    expect(yRange(null, null, 5)).toEqual([0, 5]);
    expect(yRange(null, null, null)).toEqual([0, 1]);
    const [a, b] = yRange(13.5, 13.5, 16);
    expect(a).toBeCloseTo(13.23, 9);
    expect(b).toBeCloseTo(13.77, 9);
    const [c, d] = yRange(0.35, 0.39, 5);
    expect(c).toBeCloseTo(0.346, 9);
    expect(d).toBeCloseTo(0.394, 9);
    expect(yRange(0, 2, 5)).toEqual([0, 2.2]);
    const [e, f] = yRange(-1, 1, null);
    expect(e).toBeCloseTo(-1.2, 9);
    expect(f).toBeCloseTo(1.2, 9);
    const [g, k] = yRange(0.0004, 0.0004, 5); // flat tiny current: ±1 mA floor, not below 0
    expect(g).toBe(0);
    expect(k).toBeCloseTo(0.0014, 9);
  });

  it('clockLabel is local 24 h wall time, zero padded; wallS reads ts.wall_ns', () => {
    const at = new Date(2026, 10, 15, 8, 5, 7, 250).getTime() / 1000;
    expect(clockLabel(at)).toBe('08:05:07');
    expect(clockLabel(at, true)).toBe('08:05:07.250');
    expect(clockLabel(new Date(2026, 10, 15, 20, 38, 17, 999).getTime() / 1000 + 0.0004, true)).toBe('20:38:17.999');
    const ev = rec('psu.state', 1, 2, {});
    expect(wallS(ev)).toBe(Number((1_762_770_000_000_000_000n + 2_000_000_000n) / 1_000_000n) / 1000);
    expect(wallS({ ...ev, ts: { ...ev.ts, wall_ns: 'x' } })).toBeNull();
  });

  it('Series keeps wall/setpoint refs index-aligned, prunes them together and nulls setpoints at gaps', () => {
    const s = new Series();
    for (let t = 0; t <= 100; t++) s.push(t, 13.5, 0.3, 4.05, { wallS: 1000 + t, setV: 13.5, setI: 2 });
    s.gap(101, 1101);
    expect(s.t[0]).toBe(41);
    expect(s.w[0]).toBe(1041);
    for (const arr of [s.v, s.i, s.p, s.w, s.sv, s.si]) expect(arr.length).toBe(s.t.length);
    expect(s.w.at(-1)).toBe(1101);
    expect(s.sv.at(-1)).toBeNull();
    expect(s.si.at(-1)).toBeNull();
    const [tv, v, sv] = s.laneData('v');
    expect(tv).toBe(s.t);
    expect(v).toBe(s.v);
    expect(sv).toBe(s.sv);
    expect(s.laneData('i')[2]).toBe(s.si);
    expect(s.laneData('p')).toHaveLength(2);
    const bare = new Series();
    bare.push(1, 1, 1);
    expect([bare.w[0], bare.sv[0], bare.si[0]]).toEqual([null, null, null]);
  });

  it('readoutAt / readoutText', () => {
    const W = new Date(2026, 10, 15, 20, 38, 17, 200).getTime() / 1000;
    const s = new Series();
    s.push(10, 13.49, 0.352, 4.75, { wallS: W, setV: 13.5, setI: 2 });
    s.gap(11, W + 1);
    s.push(12, null, 0.4, null, { wallS: W + 2 });
    expect(readoutAt(s, 3)).toBeNull();
    expect(readoutAt(s, -1)).toBeNull();
    expect(readoutAt(s, 0.5)).toBeNull();
    expect(readoutAt(s, 1)?.gap).toBe(true);
    expect(readoutText(readoutAt(s, 0), '커서')).toBe('커서 20:38:17.200 · 13.490 V · 352.0 mA · 4.750 W · 설정 13.50 V / 2.00 A');
    expect(readoutText(readoutAt(s, 1), '커서')).toBe('커서 20:38:18.200 · 측정 손실 (선 끊김)');
    expect(readoutText(readoutAt(s, 2), '최신')).toBe('최신 20:38:19.200 · — V · 400.0 mA · — W');
    expect(readoutText(null, '최신')).toBe('측정 없음');
    expect(readoutText(null, '커서')).toBe('');
    const mono = new Series();
    mono.push(3.5, 1, 0.1, 0.1);
    expect(readoutText(readoutAt(mono, 0), '커서')).toBe('커서 t 3.500 s · 1.000 V · 100.0 mA · 100.0 mW');
  });

  it("xTickLabels uses each point's own wall time plus the mono offset — a wall jump is not smeared over the window", () => {
    const W = new Date(2026, 10, 15, 9, 0, 0, 0).getTime() / 1000;
    const s = new Series();
    s.push(100, 1, 1, 1, { wallS: W });
    s.push(110, 1, 1, 1, { wallS: W + 3600 }); // wall stepped +1 h (NTP) between samples
    expect(xTickLabels(s, [99, 100, 105, 110, 112])).toEqual(['08:59:59', '09:00:00', '09:00:05', '10:00:00', '10:00:02']);
    const noWall = new Series();
    noWall.push(5, 1, 1);
    noWall.push(10, 1, 1);
    expect(xTickLabels(noWall, [5, 10])).toEqual(['-5 s', '0 s']);
    expect(xTickLabels(new Series(), [1.5])).toEqual(['2']);
  });

  it('chartColors reads theme tokens (trimmed) with the old dark palette as fallback', () => {
    const tokens: Record<string, string> = { '--muted': ' #555 ', '--line': '#ddd' };
    expect(chartColors((t) => tokens[t] ?? '')).toEqual({ axis: '#555', grid: '#ddd', tick: '#ddd', key: '#555|#ddd|#ddd' });
    expect(chartColors(() => '')).toEqual({ axis: '#a9b8c9', grid: '#293849', tick: '#405267', key: '#a9b8c9|#293849|#405267' });
  });
});

// ---------------------------------------------------------------------------------------------
// Operator upgrade batch 4 (FUNCTIONS.md §Batch 4): PSU-04 cycle control, PSU-03 history section.

describe('PSU-04 power cycle control', () => {
  const on = (extra: Record<string, unknown> = {}) => rec('psu.state', 1, 0, { set_v: 12, set_i: 1, out: true, prot: 'ok', ...extra });
  it('one R3 action: psu.cycle{channel, off_ms} — never two UI clicks', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([on()]);
    const b = q<HTMLButtonElement>('psu-cycle');
    expect(b.textContent).toBe('전원 사이클');
    expect(b.disabled).toBe(false);
    expect(q('psu-cycle').parentElement!.textContent).toContain('R3');
    expect(q<HTMLInputElement>('psu-cycle-ms').value).toBe('1000');
    type('psu-cycle-ms', '2500');
    b.click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'psu.cycle', params: { channel: 'bench', off_ms: 2500 } }]);
  });
  it('disabled when off / unknown / cycling / invalid off time / watch right', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch([on({ out: false })]);
    expect(q<HTMLButtonElement>('psu-cycle').disabled).toBe(true);
    h.batch([rec('psu.state', 2, 1, { out: true, cycling: true })]);
    expect(q<HTMLButtonElement>('psu-cycle').disabled).toBe(true);
    expect(q('psu-cycle').textContent).toBe('사이클 중…');
    h.batch([rec('psu.state', 3, 2, { cycling: false })]);
    expect(q<HTMLButtonElement>('psu-cycle').disabled).toBe(false);
    for (const bad of ['0', '60001', '1.5', 'x', '']) {
      type('psu-cycle-ms', bad);
      h.frame();
      expect(q('psu-cycle-ms').getAttribute('aria-invalid'), bad).toBe('true');
      expect(q<HTMLButtonElement>('psu-cycle').disabled, bad).toBe(true);
    }
    type('psu-cycle-ms', '60000');
    h.frame();
    expect(q<HTMLButtonElement>('psu-cycle').disabled).toBe(false);
  });
  it('watch right cannot cycle; emergency OFF unaffected', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' }, { hello: { rights: 'watch' } });
    h.batch([on()]);
    expect(q<HTMLButtonElement>('psu-cycle').disabled).toBe(true);
    expect(q<HTMLButtonElement>('psu-emergency').disabled).toBe(false);
  });
});

const HF = new Date(2026, 10, 15, 20, 0, 0, 0).getTime();
const HT = HF + 900_000;
const stat = (n: number, min: number, max: number, sum: number, last: number) => ({ n, min, max, sum, mean: sum / n, last });
const rawPage = (o: Record<string, unknown> = {}) => ({
  ch: 'bench',
  now_ms: HT,
  range: { from_ms: HF, to_ms: HT },
  samples: 3,
  losses: 1,
  stats: { voltage: stat(3, 13.4, 13.6, 40.5, 13.5), current: stat(3, 0.3, 0.5, 1.2, 0.4), power: stat(3, 4.02, 6.8, 16.2, 5.4) },
  first: { seq: 10, wall_ms: HF + 100 },
  last: { seq: 12, wall_ms: HF + 500 },
  next_cursor: null,
  before_index: false,
  index: { oldest_ms: HF - 60_000, dropped: false },
  journal_gaps: [],
  scanned_bytes: 1234,
  complete: true,
  ...o,
});

describe('psu history model (B4)', () => {
  it('readHistoryPage validates the RPC shape (bare or {data}); malformed → null, never zeros', () => {
    const p = readHistoryPage(rawPage())!;
    expect(p).toEqual({
      fromMs: HF, toMs: HT, nowMs: HT, samples: 3, losses: 1,
      v: { n: 3, min: 13.4, max: 13.6, sum: 40.5, last: 13.5 },
      i: { n: 3, min: 0.3, max: 0.5, sum: 1.2, last: 0.4 },
      p: { n: 3, min: 4.02, max: 6.8, sum: 16.2, last: 5.4 },
      first: { seq: 10, wallMs: HF + 100 }, last: { seq: 12, wallMs: HF + 500 },
      nextCursor: null, beforeIndex: false, indexOldestMs: HF - 60_000, journalGaps: 0, rows: [],
    });
    expect(readHistoryPage({ data: rawPage() })).toEqual(p);
    const empty = readHistoryPage(rawPage({ samples: 0, stats: { voltage: null, current: null, power: null }, first: null, last: null }))!;
    expect([empty.v, empty.first]).toEqual([null, null]);
    const withRows = readHistoryPage(rawPage({ rows: [{ seq: 10, wall_ms: HF + 100, v: 13.4, i: 0.3, p: 4.02 }] }))!;
    // B6: an older daemon sends no provenance → null (unknown), never a guess from current state
    expect(withRows.rows).toEqual([{ seq: 10, wallMs: HF + 100, v: 13.4, i: 0.3, p: 4.02, ref: null, generation: null, clockId: null }]);
    const prov = readHistoryPage(rawPage({ rows: [
      { seq: 10, wall_ms: HF + 100, v: 13.4, i: 0.3, p: 4.02, ref: 'e1794774986:seq:10', generation: 4, clock_id: 'proc-61566' },
      { seq: 10, wall_ms: HF + 200, v: 13.4, i: 0.3, p: 4.02, ref: 'e1794778200:seq:10', generation: null, clock_id: 'proc-9' },
      { seq: 11, wall_ms: HF + 300, v: 13.4, i: 0.3, p: 4.02, ref: 'seq:11', generation: -1, clock_id: '' },
      { seq: 12, wall_ms: HF + 400, v: 13.4, i: 0.3, p: 4.02, ref: 7, generation: 1.5, clock_id: 3 },
    ] }))!;
    expect(prov.rows.map((r) => [r.ref, r.generation, r.clockId])).toEqual([
      ['e1794774986:seq:10', 4, 'proc-61566'],
      ['e1794778200:seq:10', null, 'proc-9'],
      [null, null, null],
      [null, null, null],
    ]);
    expect(readHistoryPage(rawPage({ journal_gaps: [{ cause: 'ring_evicted', count: 'unknown' }] }))!.journalGaps).toBe(1);
    for (const bad of [null, 'x', {}, rawPage({ range: null }), rawPage({ samples: -1 }), rawPage({ next_cursor: 5 }),
      rawPage({ stats: { voltage: { n: 1 }, current: null, power: null } }), rawPage({ rows: [{ seq: 1, wall_ms: 2, v: 'x', i: 0, p: 0 }] })]) {
      expect(readHistoryPage(bad)).toBeNull();
    }
  });

  it('mergeHistStat / addPage merge pages exactly and keep the first page window', () => {
    const a = { n: 2, min: 1, max: 3, sum: 4, last: 3 };
    const b = { n: 1, min: 0.5, max: 0.5, sum: 0.5, last: 0.5 };
    expect(mergeHistStat(a, b)).toEqual({ n: 3, min: 0.5, max: 3, sum: 4.5, last: 0.5 });
    expect(a).toEqual({ n: 2, min: 1, max: 3, sum: 4, last: 3 }); // inputs untouched
    expect(mergeHistStat(null, b)).toEqual(b);
    expect(mergeHistStat(a, null)).toEqual(a);
    expect(mergeHistStat(null, null)).toBeNull();
    const p1 = readHistoryPage(rawPage({ next_cursor: 'e1:seq:13', journal_gaps: [{ cause: 'ring_evicted' }] }))!;
    const p2 = readHistoryPage(rawPage({ range: { from_ms: HF, to_ms: HT }, samples: 1, losses: 0, first: { seq: 13, wall_ms: HF + 700 }, last: { seq: 13, wall_ms: HF + 700 },
      stats: { voltage: stat(1, 13.7, 13.7, 13.7, 13.7), current: stat(1, 0.2, 0.2, 0.2, 0.2), power: stat(1, 2.74, 2.74, 2.74, 2.74) } }))!;
    const one = addPage(null, p1);
    expect([one.pages, one.done]).toEqual([1, false]);
    const agg = addPage(one, p2);
    expect(agg).toMatchObject({ fromMs: HF, toMs: HT, nowMs: HT, pages: 2, samples: 4, losses: 1, journalGaps: 1, done: true, cut: null,
      first: { seq: 10, wallMs: HF + 100 }, last: { seq: 13, wallMs: HF + 700 }, beforeIndex: false });
    expect(agg.v).toEqual({ n: 4, min: 13.4, max: 13.7, sum: 54.2, last: 13.7 });
    expect(isComplete(agg)).toBe(false); // a journal gap on page 1
  });

  it('isComplete and coverageText state every partial reason (never "complete" by default lookback)', () => {
    const base = addPage(null, readHistoryPage(rawPage())!);
    expect(isComplete(base)).toBe(true);
    const range = `요청 ${dateClock(HF)} – ${dateClock(HT)} · 실제 ${dateClock(HF + 100)} – ${dateClock(HF + 500)} · 샘플 3 · 측정 손실 1`;
    expect(coverageText(base)).toBe(`${range} · 완전 (요청 구간 전체 조회)`);
    const partial: HistoryAgg = { ...base, beforeIndex: true, journalGaps: 2, cut: 'aborted', done: false };
    expect(isComplete(partial)).toBe(false);
    expect(coverageText(partial)).toBe(`${range} · 부분: 색인 시작 ${dateClock(HF - 60_000)} 이전 미포함, 저널 손실 2건, 중지됨`);
    expect(coverageText({ ...base, done: false })).toBe(`${range} · 부분: 미완료`);
    expect(coverageText({ ...base, cut: 'row_cap' })).toBe(`${range} · 부분: CSV 행 상한 ${CSV_MAX_ROWS} 도달`);
    expect(coverageText({ ...base, cut: 'page_cap' })).toBe(`${range} · 부분: 페이지 상한 200 도달`);
    expect(coverageText({ ...base, beforeIndex: true, indexOldestMs: null })).toBe(`${range} · 부분: 색인 시작 없음 이전 미포함`);
    expect(coverageText({ ...base, nowMs: HT - 60_000 })).toBe(`${range} · 완전 (요청 구간 전체 조회) · 끝 시각이 조회 시각 이후 — ${dateClock(HT - 60_000)}까지만 존재`);
    const none = { ...base, first: null, last: null, samples: 0 };
    expect(coverageText(none)).toContain('실제 — – — · 샘플 0');
  });

  it('dateClock is local MM-DD HH:MM:SS', () => {
    expect(dateClock(new Date(2026, 0, 5, 7, 8, 9, 999).getTime())).toBe('01-05 07:08:09');
  });

  it('historyParams', () => {
    expect(historyParams('bench', { lastMs: 900_000 }, 0, null)).toEqual({ ch: 'bench', last_ms: 900_000 });
    expect(historyParams('bench', { fromMs: 1, toMs: 2 }, 5000, 'e1:seq:9')).toEqual({ ch: 'bench', from_ms: 1, to_ms: 2, rows: 5000, cursor: 'e1:seq:9' });
  });

  it('runHistory pins every later page to the first page range (a preset never slides between pages)', async () => {
    const calls: Record<string, unknown>[] = [];
    const pages = [rawPage({ next_cursor: 'e1:seq:13' }), rawPage({ next_cursor: 'e1:seq:20' }), rawPage()];
    const query = async (p: Record<string, unknown>) => {
      calls.push(p);
      return { data: pages[calls.length - 1] };
    };
    const seen: number[] = [];
    const r = await runHistory(query, 'bench', { lastMs: 900_000 }, { rows: 0, maxPages: 200, maxRows: CSV_MAX_ROWS, signal: new AbortController().signal, onPage: (a) => seen.push(a.pages) });
    expect(calls).toEqual([
      { ch: 'bench', last_ms: 900_000 },
      { ch: 'bench', from_ms: HF, to_ms: HT, cursor: 'e1:seq:13' },
      { ch: 'bench', from_ms: HF, to_ms: HT, cursor: 'e1:seq:20' },
    ]);
    expect(seen).toEqual([1, 2, 3]);
    expect(r.agg).toMatchObject({ pages: 3, samples: 9, done: true, cut: null });
  });

  it('runHistory caps: pages, CSV rows (truncated, flagged), abort; malformed page throws', async () => {
    const row = (seq: number) => ({ seq, wall_ms: HF + seq, v: 1, i: 0.1, p: 0.1 });
    const endless = async () => rawPage({ next_cursor: 'e1:seq:99', rows: [row(1), row(2), row(3)] });
    const sig = () => new AbortController().signal;
    const pc = await runHistory(endless, 'b', { lastMs: 1000 }, { rows: 0, maxPages: 2, maxRows: 10, signal: sig() });
    expect(pc.agg).toMatchObject({ pages: 2, cut: 'page_cap', done: false });
    const rc = await runHistory(endless, 'b', { lastMs: 1000 }, { rows: 3, maxPages: 200, maxRows: 7, signal: sig() });
    expect(rc.rows).toHaveLength(7);
    expect(rc.agg).toMatchObject({ pages: 3, cut: 'row_cap' });
    const ac = new AbortController();
    const ab = await runHistory(async () => {
      ac.abort();
      return rawPage({ next_cursor: 'e1:seq:2' });
    }, 'b', { lastMs: 1000 }, { rows: 0, maxPages: 200, maxRows: 10, signal: ac.signal });
    expect(ab.agg).toBeNull(); // the page that arrived after the abort is discarded
    await expect(runHistory(async () => ({ nope: 1 }), 'b', { lastMs: 1000 }, { rows: 0, maxPages: 2, maxRows: 10, signal: sig() })).rejects.toThrow('psu.history');
  });

  it('csvText: metadata line, header, ISO + epoch ms per row; quoted coverage', () => {
    const agg = addPage(null, readHistoryPage(rawPage())!);
    const t = csvText('bench', agg, [
      { seq: 10, wallMs: HF + 100, v: 13.4, i: 0.3, p: 4.02, ref: 'e1794774986:seq:10', generation: 4, clockId: 'proc-61566' },
      { seq: 10, wallMs: HF + 200, v: 13.5, i: 0.3, p: 4.05, ref: null, generation: null, clockId: null },
    ]);
    const lines = t.split('\n');
    // RFC 4180: quote only when needed — complete coverage has no comma …
    expect(lines[0]).toBe(`# loom psu history,ch=bench,from=${new Date(HF).toISOString()},to=${new Date(HT).toISOString()},samples=3,rows=2,losses=1,complete=true,coverage=${coverageText(agg)}`);
    // B6: existing columns kept, provenance columns appended; unknown provenance = empty cells
    expect(lines[1]).toBe('seq,wall_ms,wall_iso,v,i,p,ref,generation,clock_id');
    expect(lines[2]).toBe(`10,${HF + 100},${new Date(HF + 100).toISOString()},13.4,0.3,4.02,e1794774986:seq:10,4,proc-61566`);
    expect(lines[3]).toBe(`10,${HF + 200},${new Date(HF + 200).toISOString()},13.5,0.3,4.05,,,`);
    expect(lines[4]).toBe('');
    expect(lines).toHaveLength(5);
    // … a partial one lists reasons with commas → quoted, inner quotes doubled
    const partial: HistoryAgg = { ...agg, beforeIndex: true, journalGaps: 1 };
    const head = csvText('bench', partial, []).split('\n')[0]!;
    expect(head.endsWith(`,complete=false,"coverage=${coverageText(partial).replace(/"/g, '""')}"`)).toBe(true);
    expect(coverageText(partial)).toContain(', ');
  });
});

describe('PSU-03 history section in the gauge', () => {
  it('bound gauge only; preset → psu.history pages → stats + coverage; read-only (watch ok)', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' }, { hello: { rights: 'watch' } });
    let n = 0;
    h.net.responders.set('psu.history', () => ({ data: n++ === 0 ? rawPage({ next_cursor: 'e1:seq:13' }) : rawPage() }));
    expect(txt('psu-hist-status')).toBe('구간을 고르면 저널에 기록된 측정으로 통계를 냅니다');
    expect(q('psu-hist-stats').hidden).toBe(true);
    expect(q<HTMLButtonElement>('psu-hist-csv').disabled).toBe(true);
    q<HTMLButtonElement>('psu-hist-preset-15m').click();
    await flush();
    await flush();
    const hc = h.net.calls.filter((c) => c.method === 'psu.history').map((c) => c.params);
    expect(hc).toEqual([{ ch: 'bench', last_ms: 900_000 }, { ch: 'bench', from_ms: HF, to_ms: HT, cursor: 'e1:seq:13' }]);
    expect(txt('psu-hist-status')).toBe('2쪽 조회 완료');
    expect(txt('psu-hist-coverage')).toContain('샘플 6');
    expect(txt('psu-hist-coverage')).toContain('완전');
    expect(q('psu-hist-stats').hidden).toBe(false);
    expect(txt('psu-hist-row-v')).toBe('전압13.400 V13.600 V13.500 V13.500 V');
    expect(q<HTMLButtonElement>('psu-hist-csv').disabled).toBe(false);
  });

  it('CSV reads row pages of the last queried window and says "requested", not "saved"', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.net.responders.set('psu.history', (p) => ({
      data: (p as { rows?: number }).rows ? rawPage({ rows: [{ seq: 10, wall_ms: HF + 100, v: 13.4, i: 0.3, p: 4.02 }] }) : rawPage(),
    }));
    const blobs: Blob[] = [];
    const orig = URL.createObjectURL;
    URL.createObjectURL = ((b: Blob) => {
      blobs.push(b);
      return 'blob:x';
    }) as typeof URL.createObjectURL;
    try {
      q<HTMLButtonElement>('psu-hist-preset-1h').click();
      await flush();
      q<HTMLButtonElement>('psu-hist-csv').click();
      await flush();
      await flush();
      const hc = h.net.calls.filter((c) => c.method === 'psu.history').map((c) => c.params);
      expect(hc[1]).toEqual({ ch: 'bench', from_ms: HF, to_ms: HT, rows: 5000 });
      expect(blobs).toHaveLength(1);
      expect((await blobs[0]!.text()).split('\n')[1]).toBe('seq,wall_ms,wall_iso,v,i,p,ref,generation,clock_id');
      expect(h.toasts).toEqual(['CSV 다운로드 요청됨 — 브라우저 다운로드 위치를 확인하세요 (1행)']);
    } finally {
      URL.createObjectURL = orig;
    }
  });

  it('custom range is validated before any RPC; RPC failure is shown, not zeros', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    type('psu-hist-from', '2026-11-15T21:00');
    type('psu-hist-to', '2026-11-15T20:00');
    q<HTMLButtonElement>('psu-hist-range').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.history')).toEqual([]);
    expect(txt('psu-hist-status')).toBe('시작/끝 시각을 확인하세요 (시작 < 끝, 최대 24시간)');
    h.net.responders.set('psu.history', () => {
      throw new Error('E_USAGE window longer than 86400000 ms');
    });
    type('psu-hist-from', '2026-11-15T20:00');
    type('psu-hist-to', '2026-11-15T21:00');
    q<HTMLButtonElement>('psu-hist-range').click();
    await flush();
    const hc = h.net.calls.filter((c) => c.method === 'psu.history').map((c) => c.params);
    expect(hc).toEqual([{ ch: 'bench', from_ms: new Date(2026, 10, 15, 20, 0).getTime(), to_ms: new Date(2026, 10, 15, 21, 0).getTime() }]);
    expect(txt('psu-hist-status')).toBe('조회 실패: E_USAGE window longer than 86400000 ms');
    expect(q('psu-hist-stats').hidden).toBe(true);
  });

  it('unbound tile has no history section', () => {
    h = mountPanel('psu.gauge', {});
    expect(h.host.querySelector('[data-testid="psu-history"]')).toBeNull();
  });
});
