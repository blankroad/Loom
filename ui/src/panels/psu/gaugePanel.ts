// psu.gauge (M1b, sim; _xteam/explore-ui.md). Same data source as the STOP panel's
// "출력 유지 장치" (psu.state / outputs_held).
//
// Layout follows the Keysight BenchVue/PathWave Power Supply *Instrument View* output tile
// (reference + deviations: docs/impl/main/research/fidelity/psu.md). Loom branding only.
//   header       alias · SIM · output state · CV/CC (reported only) · protection (reported only)
//                [STOP — 출력 유지 중] [! 비상 OFF] [출력 ON/OFF switch] R3
//   meter        measured V / A / W — dominant, right-aligned, "—" when unknown;
//                W marked "(계산)" when derived from V×I rather than reported
//   setpoints    target voltage / current limit, visually separate from the meter, plus the active
//                profile limits vmax/imax as reported (PSU-01; over-limit input flagged before [적용])
//   set form     typed values never send; only [적용] (R2) does — no Enter / keystroke apply
//   60 s view    stacked measured V / A / W lanes (logger-inspired), a source.gap breaks every
//                lane (no interpolation). A memory window — NOT a persistent data log. Axis labels are the
//                local wall clock of each point (ts.wall_ns + mono offset), dashed line = reported setpoint,
//                readout line = latest row or the synced cursor row (PSU-02).
// Actions:
//   [! 비상 OFF]  psu.off{channel} (fixed-OFF endpoint, B5) — safety direction: any right incl. watch, NO
//                 confirmation, interlocks only warn (A3). Key `!` when the panel has focus.
//                 Channel-scoped. Global STOP never calls it (STOP keeps outputs: badge).
//   [출력 ON/OFF] psu.out — R3 badge (F1: policy default auto, badge always shown)
//   [적용]        psu.set{v?, i?} — R2 badge
//   [연결 해제]   psu.disconnect{ch} — closes the link only; the output is held as-is (operate)
//   [전원 사이클] psu.cycle{off_ms} — R3; OFF → wait → ON runs on the worker (PSU-04)
//   기록 조회     psu.history pages → stats / CSV of the journaled samples (PSU-03, psuHistory.ts)
// Not offered (no backend): OVP/OCP/range/priority-mode writes, All On, a separate logger, Arb list.
//
// Unbound (no `channel` prop = PSU module tab before any supply is chosen): the same tile with empty
// values plus the psu.connect section (psuConnect.ts). No subscription, no psu.status poll, and no
// psu.out / psu.set at all — there is no implicit "bench" target. Emergency OFF only exists for an
// explicitly bound alias, where it stays enabled even when the link state is lost/unknown.

import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { CHANNEL_PROP, registerPanel, type LoomEvent, type PanelContext, type PanelInstance } from '../../sdk';
import './psu.css';
import { createPsuConnect } from './psuConnect';
import { createPsuHistory } from './psuHistory';
import {
  chartColors,
  checkSetpoint,
  EMPTY_MEAS,
  eng,
  fmt,
  gapCauseLabel,
  gapCauseSummary,
  limitNum,
  limitText,
  modeLabel,
  monoS,
  protLabel,
  readMetrics,
  readoutAt,
  readoutText,
  outText,
  reduceState,
  sourceLabel,
  Series,
  wallS,
  xTickLabels,
  yRange,
  WINDOW_S,
  type Lane,
  type Meas,
  type PsuState,
} from './psuModel';

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const tid = <E extends HTMLElement>(e: E, id: string): E => {
  e.setAttribute('data-testid', id);
  return e;
};

const LANES: readonly { key: Lane; label: string; unit: string; stroke: string }[] = [
  { key: 'v', label: '전압', unit: 'V', stroke: '#19b3ad' },
  { key: 'i', label: '전류', unit: 'A', stroke: '#e0b020' },
  { key: 'p', label: '전력', unit: 'W', stroke: '#a3b1c2' },
];
const LANE_H = 88;
let syncSeq = 0;

export function createPsuGauge(ctx: PanelContext): PanelInstance {
  const bound = typeof ctx.props.channel === 'string' && ctx.props.channel ? ctx.props.channel : null;
  /** display / param alias — only ever sent when `bound` (every write path checks it) */
  const ch = bound ?? '';
  // unbound: never-fed mirror, no subscription (ctx.store is lazy)
  const mirror: Pick<ReturnType<PanelContext['store']>, 'after' | 'onAppend' | 'headSeq'> = bound
    ? ctx.store('psu')
    : { after: () => [], onAppend: () => () => {}, headSeq: 0n };
  // emergency OFF stays enabled regardless (safety direction) — for a bound alias
  const hasRight = () => ctx.rights.has('operate') || ctx.rights.has('approve');
  const canOperate = () => bound !== null && hasRight() && !ctx.writeLocked;
  let state: PsuState = { setV: null, setI: null, out: null, prot: null, mode: null };
  /** a device-reported state (psu.state or psu.status) arrived for this alias — set/output need it */
  let confirmed = false;
  /** seq of the newest psu.state applied (control path and backlog share it: older never overwrite) */
  let stateSeq = -1n;
  const applyState = (ev: LoomEvent): boolean => {
    if (ev.kind !== 'psu.state') return false;
    let seq: bigint;
    try {
      seq = BigInt(ev.seq);
    } catch {
      return false;
    }
    if (seq <= stateSeq) return false;
    stateSeq = seq;
    state = reduceState(state, ev);
    confirmed = true;
    return true;
  };
  let meas: Meas = { ...EMPTY_MEAS };
  let staleAfterGap = false; // a loss arrived after the last measurement
  let statusPending = false;
  const statusAbort = new AbortController();
  const series = new Series();
  let lastSeq = -1n;
  let gaps = 0;
  const gapCauses = new Map<string, number>(); // label → count, from source.gap.payload.cause
  const shown: Record<Lane, boolean> = { v: true, i: true, p: false };
  const plots = new Map<Lane, uPlot>();
  let cursorIdx: number | null = null;
  let colorKey = '';
  let plotW = 0;
  let plotErr: string | null = null;
  const syncKey = `psu-${ch}-${++syncSeq}`;

  const act = (id: string, params: object) =>
    // defence in depth: no alias → no device command, whatever triggered it
    (bound === null ? Promise.reject(new Error('PSU 별칭 미선택')) : ctx.act(id, params)).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- DOM: header ----
  const root = el('div', 'psu-panel psu-tile');
  root.tabIndex = 0;
  const head = tid(el('div', 'psu-head'), 'psu-head');
  const ident = el('span', 'psu-ident');
  const badge = tid(el('span', 'psu-badge', bound ?? '별칭 미선택'), 'psu-badge');
  badge.title = bound ? `PSU 별칭 ${bound}` : 'PSU 별칭이 선택되지 않았습니다 — 아래에서 연결하거나 열린 별칭을 보세요';
  const envChip = tid(el('span', 'psu-env', `환경 ${(ctx.env ?? '?').toUpperCase()}`), 'psu-env');
  envChip.title = '데몬 환경 배지 (백엔드/장치 종류가 아님)';
  const srcChip = tid(el('span', 'psu-src'), 'psu-source');
  srcChip.title = '장치가 보고한 백엔드 · 주소 · *IDN?';
  ident.append(el('span', 'psu-kind', 'PSU'), ' ', badge, ' ', envChip, ' ', srcChip);
  const outWord = tid(el('span', 'psu-out'), 'psu-out');
  const modeChip = tid(el('span', 'psu-chip psu-mode'), 'psu-mode');
  modeChip.title = '장치가 보고한 동작 모드 (추정하지 않음)';
  const protChip = tid(el('span', 'psu-chip psu-prot'), 'psu-prot');
  protChip.title = '장치가 보고한 보호 상태 (OVP/OCP 설정은 이 백엔드에서 미지원)';
  const stopBadge = tid(el('span', 'psu-stop', 'STOP — 출력 유지 중'), 'psu-stop');
  stopBadge.title = '전역 STOP은 자동화 쓰기를 멈출 뿐 PSU 출력을 끄지 않습니다';
  stopBadge.hidden = true;
  const emergency = tid(el('button', 'danger psu-emergency', '! 비상 OFF'), 'psu-emergency');
  emergency.type = 'button';
  emergency.title = bound
    ? `${bound} 채널만 확인 없이 출력 OFF 요청 (보기 자격 가능 · OFF 전용 · 인터록은 경고만) — 정책이 막으면 거부될 수 있습니다`
    : '별칭 미선택 — 끌 대상이 없습니다 (연결된 별칭의 게이지에서 항상 사용 가능)';
  const outBtn = tid(el('button', 'psu-switch', '출력 ON'), 'psu-output-toggle');
  outBtn.type = 'button';
  const r3 = el('span', 'risk risk-R3', 'R3');
  const disconnectBtn = tid(el('button', 'psu-disconnect', '연결 해제'), 'psu-disconnect');
  disconnectBtn.type = 'button';
  disconnectBtn.title = 'psu.disconnect — 링크만 닫습니다. 출력은 그대로 유지됩니다 (끄려면 비상 OFF/출력 OFF)';
  disconnectBtn.hidden = bound === null;
  const acts = el('span', 'psu-head-act');
  acts.append(stopBadge, emergency, outBtn, r3, disconnectBtn);
  head.append(ident, outWord, modeChip, protChip, acts);

  // ---- DOM: meter (measured) + setpoints (targets) ----
  const vals = tid(el('div', 'psu-vals'), 'psu-vals');
  const meter = tid(el('section', 'psu-meter'), 'psu-meter');
  meter.setAttribute('aria-label', '실측값');
  const meterCap = el('div', 'psu-cap', '실측');
  const readRow = (k: Lane) => {
    const row = tid(el('div', 'psu-read'), `psu-meter-${k}`);
    const n = el('span', 'psu-num');
    const u = el('span', 'psu-unit');
    const note = el('span', 'psu-note');
    row.append(n, u, note);
    return { row, n, u, note };
  };
  const rV = readRow('v');
  const rI = readRow('i');
  const rP = readRow('p');
  const meterMeta = tid(el('div', 'psu-meta'), 'psu-meter-age');
  meter.append(meterCap, rV.row, rI.row, rP.row, meterMeta);

  const sets = tid(el('section', 'psu-setpoints'), 'psu-setpoints');
  sets.setAttribute('aria-label', '설정값 (목표)');
  const setRow = (label: string, id: string) => {
    const row = tid(el('div', 'psu-setrow'), id);
    const v = el('span', 'psu-setval');
    row.append(el('span', 'psu-setlabel', label), v);
    return v;
  };
  const sV = setRow('전압 설정', 'psu-setpoint-v');
  const sI = setRow('전류 제한', 'psu-setpoint-i');
  const limRow = tid(el('div', 'psu-setrow psu-limits'), 'psu-limits');
  const limVal = el('span', 'psu-setval');
  limRow.append(el('span', 'psu-setlabel', '한도'), limVal);
  limRow.title = '프로필 한도 (연결 시 낮춘 값 포함) — 넘는 설정은 장치로 보내지 않습니다';
  sets.append(el('div', 'psu-cap', '설정 (목표값)'), sV.parentElement!, sI.parentElement!, limRow);
  vals.append(meter, sets);

  // ---- DOM: set form (explicit apply only) ----
  const ctl = tid(el('div', 'psu-ctl'), 'psu-set-form');
  const field = (label: string, unit: string, id: string) => {
    const wrap = el('label', 'psu-field');
    const input = tid(Object.assign(el('input'), { inputMode: 'decimal', autocomplete: 'off' }), id);
    input.setAttribute('aria-label', label === '전압 설정' ? '설정 전압' : '설정 전류');
    wrap.append(el('span', 'psu-setlabel', label), input, el('span', 'psu-unit-s', unit));
    return input;
  };
  const vIn = field('전압 설정', 'V', 'psu-set-v');
  const iIn = field('전류 제한', 'A', 'psu-set-i');
  const setBtn = tid(el('button', 'psu-apply', '적용'), 'psu-apply');
  setBtn.type = 'button';
  setBtn.title = '입력한 값만 장치에 보냅니다. 입력 중에는 아무것도 보내지 않습니다.';
  const r2 = el('span', 'risk risk-R2', 'R2');
  const pending = tid(el('span', 'psu-pending'), 'psu-set-pending');
  pending.setAttribute('aria-live', 'polite');
  const cyc = tid(el('span', 'psu-cycle'), 'psu-cycle-wrap');
  const cycMs = tid(Object.assign(el('input'), { inputMode: 'numeric', value: '1000', autocomplete: 'off' }), 'psu-cycle-ms');
  cycMs.setAttribute('aria-label', 'OFF 시간 (ms)');
  const cycBtn = tid(el('button', undefined, '전원 사이클'), 'psu-cycle');
  cycBtn.type = 'button';
  cyc.append(el('span', 'psu-setlabel', 'OFF ms'), cycMs, cycBtn, el('span', 'risk risk-R3', 'R3'));
  ctl.append(vIn.parentElement!, iIn.parentElement!, setBtn, r2, pending, cyc);
  /** OFF time in ms (whole number 1..60000) or null */
  const validMs = (): number | null => {
    const t = cycMs.value.trim();
    if (!/^\d+$/.test(t)) return null;
    const n = Number(t);
    return n >= 1 && n <= 60_000 ? n : null;
  };

  // ---- DOM: 60 s observation (stacked lanes) ----
  const trend = tid(el('section', 'psu-trend'), 'psu-trend');
  const trendHead = el('div', 'psu-trend-head');
  const toggles = el('span', 'psu-lane-toggles');
  const laneBtn = new Map<Lane, HTMLButtonElement>();
  for (const l of LANES) {
    const b = tid(el('button', 'psu-lane-btn', `${l.unit}`), `psu-lane-${l.key}`);
    b.type = 'button';
    b.title = `${l.label} 레인 표시/숨김 (수집은 계속)`;
    b.addEventListener('click', () => {
      shown[l.key] = !shown[l.key];
      ctx.scheduler.invalidate();
    });
    laneBtn.set(l.key, b);
    toggles.append(b);
  }
  const readout = tid(el('span', 'psu-readout'), 'psu-readout');
  trendHead.append(
    el('span', 'psu-trend-title', `${WINDOW_S}초 관측`),
    el('span', 'psu-trend-note muted', '실측 추세 · 60 s 화면 창 (긴 구간은 아래 기록 조회)'),
    readout,
    toggles,
  );
  const plotHost = el('div', 'psu-plot psu-lanes');
  const laneEl = new Map<Lane, { wrap: HTMLDivElement; host: HTMLDivElement }>();
  for (const l of LANES) {
    const wrap = tid(el('div', 'psu-lane'), `psu-lane-row-${l.key}`);
    const host = el('div', 'psu-lane-plot');
    wrap.append(el('span', 'psu-lane-label', l.unit), host);
    laneEl.set(l.key, { wrap, host });
    plotHost.append(wrap);
  }
  const plotMsg = el('div', 'psu-plot-msg muted');
  plotMsg.hidden = true;
  plotHost.append(plotMsg);
  trend.append(trendHead, plotHost);
  const foot = tid(el('div', 'psu-foot muted'), 'psu-foot');
  const connect = bound ? null : createPsuConnect(ctx);
  root.dataset.bound = String(bound !== null);
  const placeholder = bound === null
    ? tid(el('div', 'psu-placeholder muted', '연결하면 실측·설정·추세가 여기에 표시됩니다'), 'psu-placeholder')
    : null;
  const history = bound !== null ? createPsuHistory(ctx, bound) : null;
  root.append(
    ...(connect ? [connect.root] : []),
    head,
    ...(placeholder ? [placeholder] : []),
    vals,
    ctl,
    trend,
    ...(history ? [history.root] : []),
    foot,
  );
  if (bound === null) {
    for (const e of [outWord, modeChip, protChip, acts, vals, ctl, trend, foot]) e.hidden = true;
  }

  // ---- behaviour ----
  const emergencyOff = () => {
    if (bound !== null) void act('emergencyOff', { channel: bound });
  };
  emergency.addEventListener('click', emergencyOff);
  outBtn.addEventListener('click', () => {
    if (bound === null || state.out === null) return; // no alias / unknown state: never guess a target or direction
    void act('output', { channel: bound, state: state.out ? 'off' : 'on' });
  });
  disconnectBtn.addEventListener('click', () => {
    if (bound !== null) void act('disconnect', { ch: bound });
  });
  const updatePending = () => {
    const v = checkSetpoint(vIn.value, state.vmax ?? null, 'V');
    const i = checkSetpoint(iIn.value, state.imax ?? null, 'A');
    vIn.setAttribute('aria-invalid', String(!v.ok));
    iIn.setAttribute('aria-invalid', String(!i.ok));
    if (!v.ok || !i.ok) {
      pending.textContent = !v.ok ? v.message : !i.ok ? i.message : '';
      pending.className = 'psu-pending err';
      return;
    }
    const diff = (x: number | null, cur: number | null) => x !== null && x !== cur;
    const dirty = diff(v.value, state.setV) || diff(i.value, state.setI);
    pending.textContent = dirty ? '미적용 — [적용]을 눌러야 장치로 보냅니다' : '';
    pending.className = dirty ? 'psu-pending dirty' : 'psu-pending';
  };
  cycMs.addEventListener('input', () => {
    cycMs.setAttribute('aria-invalid', String(validMs() === null));
    ctx.scheduler.invalidate();
  });
  cycBtn.addEventListener('click', () => {
    const n = validMs();
    if (bound === null || n === null || !canOperate() || state.connected === false || !confirmed || state.out !== true || state.cycling === true) return;
    void act('cycle', { channel: bound, off_ms: n });
  });
  vIn.addEventListener('input', updatePending);
  iIn.addEventListener('input', updatePending);
  setBtn.addEventListener('click', () => {
    if (bound === null || !confirmed) return;
    const v = checkSetpoint(vIn.value, state.vmax ?? null, 'V');
    const i = checkSetpoint(iIn.value, state.imax ?? null, 'A');
    if (!v.ok || !i.ok || (v.value === null && i.value === null)) {
      ctx.ui.toast('설정값을 확인하세요');
      return;
    }
    if (bound === null) return;
    void act('set', { channel: bound, ...(v.value !== null ? { v: v.value } : {}), ...(i.value !== null ? { i: i.value } : {}) }).then((r) => {
      if (r === null) return; // failure toasted; keep the typed values for correction
      vIn.value = '';
      iIn.value = '';
      updatePending();
    });
  });
  root.addEventListener('keydown', (e) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT') return; // Enter / typing in a field never applies
    if (e.key === '!') {
      e.preventDefault();
      emergencyOff();
    }
  });

  const makePlots = (w: number) => {
    if (bound === null || plots.size || plotErr) return;
    plotW = w;
    // no 2D canvas (headless / jsdom) → say so instead of failing inside uPlot's draw loop
    if (!document.createElement('canvas').getContext('2d')) {
      plotErr = '캔버스 없음';
      plotMsg.textContent = '플롯 사용 불가: 캔버스 없음 (값은 위에 표시)';
      plotMsg.hidden = false;
      return;
    }
    try {
      const c = chartColors((t) => getComputedStyle(root).getPropertyValue(t));
      colorKey = c.key;
      const laneLimit = (k: Lane): number | null => {
        if (k === 'v') return state.vmax ?? null;
        if (k === 'i') return state.imax ?? null;
        return state.vmax && state.imax ? state.vmax * state.imax : null;
      };
      for (const l of LANES) {
        const p = new uPlot(
          {
            width: Math.max(200, w - 28),
            height: LANE_H,
            scales: { x: { time: false }, y: { range: (_u, mn, mx) => yRange(mn, mx, laneLimit(l.key)) } },
            cursor: { sync: { key: syncKey } },
            hooks: {
              setCursor: [
                (u) => {
                  const k = u.cursor.idx;
                  cursorIdx = typeof k === 'number' ? k : null;
                  ctx.scheduler.invalidate();
                },
              ],
            },
            series: [
              { label: 't' },
              { label: l.unit, stroke: l.stroke, spanGaps: false },
              ...(l.key === 'p' ? [] : [{ label: '설정', stroke: l.stroke, width: 1, dash: [4, 4], spanGaps: false }]),
            ],
            axes: [
              { size: 26, stroke: c.axis, grid: { stroke: c.grid }, ticks: { stroke: c.tick }, font: '11px ui-monospace, monospace', values: (_u, splits) => xTickLabels(series, splits) },
              { size: 52, stroke: c.axis, grid: { stroke: c.grid }, ticks: { stroke: c.tick }, font: '11px ui-monospace, monospace' },
            ],
            legend: { show: false },
          },
          series.laneData(l.key),
          laneEl.get(l.key)!.host,
        );
        plots.set(l.key, p);
      }
    } catch (e) {
      for (const p of plots.values()) p.destroy();
      plots.clear();
      plotErr = e instanceof Error ? e.message : String(e);
      plotMsg.textContent = `플롯 사용 불가: ${plotErr}`;
      plotMsg.hidden = false;
    }
  };

  const setRead = (r: { n: HTMLElement; u: HTMLElement; note: HTMLElement; row: HTMLElement }, x: number | null, unit: string, note = '') => {
    const f = eng(x, unit);
    r.n.textContent = f.num;
    r.u.textContent = f.unit;
    r.note.textContent = note;
    r.row.classList.toggle('unknown', x === null);
  };

  const draw = (): bigint => {
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      applyState(ev);
      const m = readMetrics(ev);
      if (m) {
        meas = { ...m, atMs: performance.now() };
        staleAfterGap = false;
        series.push(monoS(ev), m.v, m.i, m.p, { wallS: wallS(ev), setV: state.setV, setI: state.setI });
      }
      if (ev.kind === 'source.gap') {
        gaps++;
        const cause = gapCauseLabel(ev.payload);
        gapCauses.set(cause, (gapCauses.get(cause) ?? 0) + 1);
        staleAfterGap = meas.atMs !== null;
        series.gap(monoS(ev), wallS(ev)); // break every lane — never interpolate across a loss
      }
    }
    // header
    const ow = outText(state);
    outWord.textContent = ow.text;
    outWord.className = `psu-out ${ow.cls}`;
    srcChip.textContent = sourceLabel(state);
    root.classList.toggle('psu-disconnected', state.connected === false);
    const mode = modeLabel(state.mode);
    modeChip.textContent = mode ?? '';
    modeChip.hidden = bound === null || mode === null;
    if (mode !== null) modeChip.setAttribute('aria-label', `동작 모드: ${mode}`);
    const prot = protLabel(state.prot);
    protChip.textContent = prot.text;
    protChip.setAttribute('aria-label', `보호 상태: ${prot.text}`);
    protChip.classList.toggle('warn', prot.warn);
    stopBadge.hidden = !(ctx.stopLatched && state.out === true);
    // a disconnected supply is read-only here (its last state stays visible); emergency OFF stays (safety direction)
    const op = canOperate() && state.connected !== false;
    outBtn.disabled = !op || state.out === null;
    outBtn.textContent = state.out ? '출력 OFF' : '출력 ON';
    outBtn.setAttribute('aria-pressed', state.out === null ? 'mixed' : String(state.out));
    outBtn.classList.toggle('on', state.out === true);
    outBtn.title = state.connected === false
      ? '장치 연결 끊김 — 쓰기 불가 (마지막 확인 상태만 표시)'
      : state.out === null ? '출력 상태 미확인 — 토글 불가' : `출력 ${state.out ? '끄기' : '켜기'} (operate 권한, 쓰기 잠금 적용)`;
    // safety direction: always available for a bound alias — even when the link is lost/unknown
    emergency.disabled = bound === null;
    disconnectBtn.disabled = !canOperate();
    connect?.update(hasRight() && !ctx.writeLocked, hasRight());
    root.classList.toggle('psu-is-on', state.out === true);
    // meter
    setRead(rV, meas.v, 'V');
    setRead(rI, meas.i, 'A');
    setRead(rP, meas.p, 'W', meas.p !== null && meas.pDerived ? '(계산)' : '');
    rP.row.title = meas.pDerived ? '장치가 전력을 보고하지 않아 V×I로 계산' : '';
    meter.classList.toggle('stale', staleAfterGap);
    meterMeta.textContent =
      meas.atMs === null
        ? '측정 없음'
        : `${staleAfterGap ? '측정 손실 후 미갱신 · 마지막 값 · ' : ''}age ${Math.round(performance.now() - meas.atMs)} ms`;
    // setpoints
    sV.textContent = fmt(state.setV, 'V', 2);
    sI.textContent = fmt(state.setI, 'A', 2);
    limVal.textContent = limitText(state);
    vIn.title = state.vmax ? `한도 ${limitNum(state.vmax)} V 이하` : '한도 미보고 — 데몬이 최종 확인';
    iIn.title = state.imax ? `한도 ${limitNum(state.imax)} A 이하` : '한도 미보고 — 데몬이 최종 확인';
    vIn.placeholder = state.setV === null ? 'V' : state.setV.toFixed(2);
    iIn.placeholder = state.setI === null ? 'A' : state.setI.toFixed(2);
    // setpoints go only to a bound alias whose state the device has reported (never a blind write)
    setBtn.disabled = !op || !confirmed;
    const cms = validMs();
    cycMs.disabled = !op;
    cycMs.setAttribute('aria-invalid', String(cms === null));
    cycBtn.disabled = !op || !confirmed || state.out !== true || state.cycling === true || cms === null;
    cycBtn.textContent = state.cycling === true ? '사이클 중…' : '전원 사이클';
    cycBtn.title = state.cycling === true
      ? 'OFF 구간 — ON 예정 (STOP·다른 출력 조작·링크 손실 시 ON을 보내지 않음)'
      : state.out !== true ? '출력 ON 상태에서만 사이클' : `출력 OFF → ${cms ?? '?'} ms → ON (장치 작업자가 수행, R3)`;
    setBtn.title = bound === null ? '별칭 미선택 — 연결 후 설정' : !confirmed ? '장치 상태 미확인 — 상태 보고 후 적용 가능' : '입력한 값만 장치에 보냅니다. 입력 중에는 아무것도 보내지 않습니다.';
    vIn.disabled = !op;
    iIn.disabled = !op;
    updatePending();
    // lanes
    const latest = readoutText(readoutAt(series, series.t.length - 1), '최신');
    readout.textContent = cursorIdx !== null ? readoutText(readoutAt(series, cursorIdx), '커서') || latest : latest;
    if (plots.size > 0) {
      const c = chartColors((t) => getComputedStyle(root).getPropertyValue(t));
      if (c.key !== colorKey) {
        for (const p of plots.values()) p.destroy();
        plots.clear();
        makePlots(plotW);
      }
    }
    for (const l of LANES) {
      laneEl.get(l.key)!.wrap.hidden = !shown[l.key];
      const b = laneBtn.get(l.key)!;
      b.setAttribute('aria-pressed', String(shown[l.key]));
      plots.get(l.key)?.setData(series.laneData(l.key));
    }
    foot.textContent = `${gaps ? `측정 손실 ${gaps}회 (${gapCauseSummary(gapCauses)}) — 선을 끊어 표시 · ` : ''}${WINDOW_S} s 창 · ${series.t.length}점`;
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  // psu.state is a priority kind: live changes (incl. disconnect) arrive on the control path
  const offControl = bound === null ? () => {} : ctx.onControl((ev) => {
    if ((ev.channel ?? (ev.payload as { ch?: unknown } | null)?.ch) !== bound) return;
    if (applyState(ev)) ctx.scheduler.invalidate();
  });
  // The native simulator advances measurements when polled. Authoritative status also
  // reconciles setpoints/output after writes; never infer them from the user's draft.
  const refreshStatus = () => {
    if (bound === null || statusPending || statusAbort.signal.aborted) return;
    statusPending = true;
    void ctx.query('psu.status', { ch: bound }, statusAbort.signal).then((result) => {
      if (statusAbort.signal.aborted) return;
      const envelope = result as Record<string, unknown>;
      const payload = envelope.data && typeof envelope.data === 'object' ? envelope.data : envelope;
      state = reduceState(state, { kind: 'psu.state', payload } as Parameters<typeof reduceState>[1]);
      confirmed = true;
      ctx.scheduler.invalidate();
    }).catch(() => { /* keep observed state; unavailable status is never converted to zero */ })
      .finally(() => { statusPending = false; });
  };
  const tick = setInterval(() => { refreshStatus(); ctx.scheduler.invalidate(); }, 1_000);

  return {
    mount(host) {
      host.append(root);
      makePlots(host.clientWidth - 12);
      ctx.scheduler.invalidate();
    },
    resize(w) {
      plotW = w - 12;
      if (!plots.size) makePlots(w - 12);
      else for (const p of plots.values()) p.setSize({ width: Math.max(200, w - 40), height: LANE_H });
    },
    dispose() {
      clearInterval(tick);
      statusAbort.abort();
      connect?.dispose();
      history?.dispose();
      offFrame();
      offAppend();
      offControl();
      for (const p of plots.values()) p.destroy();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'psu.gauge',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'psu',
    title: '벤치 전원 (PSU)',
    version: 1,
    milestone: 'M1b',
    topics: [
      {
        key: 'psu',
        // unbound panels never open this topic; '' would match no channel (no implicit "bench")
        filter: (p) => ({ domains: ['psu'], channels: [typeof p.channel === 'string' ? p.channel : ''] }),
        snapshot: 'required',
        mirrorRows: 1_000,
      },
    ],
    queries: ['psu.status', 'psu.history'],
    actions: [
      // two actions on the same RPC: the gate applies each spec's rights/safety rule
      { id: 'emergencyOff', rpc: 'psu.off', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '비상 OFF' },
      { id: 'output', rpc: 'psu.out', class: 'request', right: 'operate', confirm: 'none', label: '출력 토글' },
      { id: 'set', rpc: 'psu.set', class: 'request', right: 'operate', confirm: 'none', label: '설정' },
      // PSU-04: OFF → off_ms → ON on the worker (never two UI clicks); R3 like the output toggle
      { id: 'cycle', rpc: 'psu.cycle', class: 'request', right: 'operate', confirm: 'none', label: '전원 사이클' },
      // module application: explicit connect (unbound) / disconnect (bound; output held). Never on navigation.
      { id: 'connect', rpc: 'psu.connect', class: 'request', right: 'operate', confirm: 'none', label: 'PSU 연결' },
      { id: 'disconnect', rpc: 'psu.disconnect', class: 'request', right: 'operate', confirm: 'none', label: 'PSU 연결 해제' },
    ],
    memory: { maxBytes: 2 * 1024 * 1024 },
    render: { maxHz: 10, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createPsuGauge,
);
