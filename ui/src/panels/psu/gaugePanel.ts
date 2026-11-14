// psu.gauge (M1b, sim; _xteam/explore-ui.md). Same data source as the STOP panel's
// "출력 유지 장치" (psu.state / outputs_held).
//
// Layout follows the Keysight BenchVue/PathWave Power Supply *Instrument View* output tile
// (reference + deviations: docs/impl/main/research/fidelity/psu.md). Loom branding only.
//   header       alias · SIM · output state · CV/CC (reported only) · protection (reported only)
//                [STOP — 출력 유지 중] [! 비상 OFF] [출력 ON/OFF switch] R3
//   meter        measured V / A / W — dominant, right-aligned, "—" when unknown;
//                W marked "(계산)" when derived from V×I rather than reported
//   setpoints    target voltage / current limit, visually separate from the meter
//   set form     typed values never send; only [적용] (R2) does — no Enter / keystroke apply
//   60 s view    stacked measured V / A / W lanes (logger-inspired), a source.gap breaks every
//                lane (no interpolation). A memory window — NOT a persistent data log.
// Actions:
//   [! 비상 OFF]  psu.out{state:'off'} — safety direction: any right incl. watch, NO
//                 confirmation, interlocks only warn (A3). Key `!` when the panel has focus.
//                 Channel-scoped. Global STOP never calls it (STOP keeps outputs: badge).
//   [출력 ON/OFF] psu.out — R3 badge (F1: policy default auto, badge always shown)
//   [적용]        psu.set{v?, i?} — R2 badge
// Not offered (no backend): OVP/OCP/range/priority-mode writes, All On, logging/export, Arb list.

import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { CHANNEL_PROP, registerPanel, type LoomEvent, type PanelContext, type PanelInstance } from '../../sdk';
import './psu.css';
import {
  EMPTY_MEAS,
  eng,
  fmt,
  gapCauseLabel,
  gapCauseSummary,
  modeLabel,
  monoS,
  parseSetpoint,
  protLabel,
  readMetrics,
  outText,
  reduceState,
  sourceLabel,
  Series,
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
  const ch = typeof ctx.props.channel === 'string' ? ctx.props.channel : 'bench';
  const mirror = ctx.store('psu');
  // emergency OFF stays enabled regardless (safety direction)
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  let state: PsuState = { setV: null, setI: null, out: null, prot: null, mode: null };
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
  let plotErr: string | null = null;
  const syncKey = `psu-${ch}-${++syncSeq}`;

  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- DOM: header ----
  const root = el('div', 'psu-panel psu-tile');
  root.tabIndex = 0;
  const head = tid(el('div', 'psu-head'), 'psu-head');
  const ident = el('span', 'psu-ident');
  const badge = tid(el('span', 'psu-badge', ch), 'psu-badge');
  badge.title = `PSU 별칭 ${ch}`;
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
  emergency.title = `${ch} 채널만 확인 없이 즉시 출력 OFF (보기 자격도 가능, 인터록은 경고만)`;
  const outBtn = tid(el('button', 'psu-switch', '출력 ON'), 'psu-output-toggle');
  outBtn.type = 'button';
  const r3 = el('span', 'risk risk-R3', 'R3');
  const acts = el('span', 'psu-head-act');
  acts.append(stopBadge, emergency, outBtn, r3);
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
  sets.append(el('div', 'psu-cap', '설정 (목표값)'), sV.parentElement!, sI.parentElement!);
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
  ctl.append(vIn.parentElement!, iIn.parentElement!, setBtn, r2, pending);

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
  trendHead.append(
    el('span', 'psu-trend-title', `${WINDOW_S}초 관측`),
    el('span', 'psu-trend-note muted', '실측 추세 · 메모리 창 (저장 기록 아님)'),
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
  root.append(head, vals, ctl, trend, foot);

  // ---- behaviour ----
  const emergencyOff = () => void act('emergencyOff', { channel: ch, state: 'off' });
  emergency.addEventListener('click', emergencyOff);
  outBtn.addEventListener('click', () => {
    if (state.out === null) return; // unknown state: never guess a direction
    void act('output', { channel: ch, state: state.out ? 'off' : 'on' });
  });
  const updatePending = () => {
    const v = parseSetpoint(vIn.value);
    const i = parseSetpoint(iIn.value);
    vIn.setAttribute('aria-invalid', String(!v.ok));
    iIn.setAttribute('aria-invalid', String(!i.ok));
    if (!v.ok || !i.ok) {
      pending.textContent = '0 이상의 숫자를 입력하세요';
      pending.className = 'psu-pending err';
      return;
    }
    const diff = (x: number | null, cur: number | null) => x !== null && x !== cur;
    const dirty = diff(v.value, state.setV) || diff(i.value, state.setI);
    pending.textContent = dirty ? '미적용 — [적용]을 눌러야 장치로 보냅니다' : '';
    pending.className = dirty ? 'psu-pending dirty' : 'psu-pending';
  };
  vIn.addEventListener('input', updatePending);
  iIn.addEventListener('input', updatePending);
  setBtn.addEventListener('click', () => {
    const v = parseSetpoint(vIn.value);
    const i = parseSetpoint(iIn.value);
    if (!v.ok || !i.ok || (v.value === null && i.value === null)) {
      ctx.ui.toast('설정값을 확인하세요');
      return;
    }
    void act('set', { channel: ch, ...(v.value !== null ? { v: v.value } : {}), ...(i.value !== null ? { i: i.value } : {}) }).then((r) => {
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
    if (plots.size || plotErr) return;
    // no 2D canvas (headless / jsdom) → say so instead of failing inside uPlot's draw loop
    if (!document.createElement('canvas').getContext('2d')) {
      plotErr = '캔버스 없음';
      plotMsg.textContent = '플롯 사용 불가: 캔버스 없음 (값은 위에 표시)';
      plotMsg.hidden = false;
      return;
    }
    try {
      for (const l of LANES) {
        const p = new uPlot(
          {
            width: Math.max(200, w - 28),
            height: LANE_H,
            scales: { x: { time: false } },
            cursor: { sync: { key: syncKey } },
            series: [{ label: 't (s)' }, { label: l.unit, stroke: l.stroke, spanGaps: false }],
            axes: [
              { size: 26, stroke: '#a9b8c9', grid: { stroke: '#293849' }, ticks: { stroke: '#405267' }, font: '11px ui-monospace, monospace' },
              { size: 52, stroke: '#a9b8c9', grid: { stroke: '#293849' }, ticks: { stroke: '#405267' }, font: '11px ui-monospace, monospace' },
            ],
            legend: { show: false },
          },
          series.lane(l.key),
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
        series.push(monoS(ev), m.v, m.i, m.p);
      }
      if (ev.kind === 'source.gap') {
        gaps++;
        const cause = gapCauseLabel(ev.payload);
        gapCauses.set(cause, (gapCauses.get(cause) ?? 0) + 1);
        staleAfterGap = meas.atMs !== null;
        series.gap(monoS(ev)); // break every lane — never interpolate across a loss
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
    modeChip.hidden = mode === null;
    const prot = protLabel(state.prot);
    protChip.textContent = prot.text;
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
    emergency.disabled = false; // safety direction: always available
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
    vIn.placeholder = state.setV === null ? 'V' : state.setV.toFixed(2);
    iIn.placeholder = state.setI === null ? 'A' : state.setI.toFixed(2);
    setBtn.disabled = !op;
    vIn.disabled = !op;
    iIn.disabled = !op;
    updatePending();
    // lanes
    for (const l of LANES) {
      laneEl.get(l.key)!.wrap.hidden = !shown[l.key];
      const b = laneBtn.get(l.key)!;
      b.setAttribute('aria-pressed', String(shown[l.key]));
      plots.get(l.key)?.setData(series.lane(l.key));
    }
    foot.textContent = `${gaps ? `측정 손실 ${gaps}회 (${gapCauseSummary(gapCauses)}) — 선을 끊어 표시 · ` : ''}${WINDOW_S} s 창 · ${series.t.length}점`;
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  // psu.state is a priority kind: live changes (incl. disconnect) arrive on the control path
  const offControl = ctx.onControl((ev) => {
    if ((ev.channel ?? (ev.payload as { ch?: unknown } | null)?.ch) !== ch) return;
    if (applyState(ev)) ctx.scheduler.invalidate();
  });
  // The native simulator advances measurements when polled. Authoritative status also
  // reconciles setpoints/output after writes; never infer them from the user's draft.
  const refreshStatus = () => {
    if (statusPending || statusAbort.signal.aborted) return;
    statusPending = true;
    void ctx.query('psu.status', { ch }, statusAbort.signal).then((result) => {
      if (statusAbort.signal.aborted) return;
      const envelope = result as Record<string, unknown>;
      const payload = envelope.data && typeof envelope.data === 'object' ? envelope.data : envelope;
      state = reduceState(state, { kind: 'psu.state', payload } as Parameters<typeof reduceState>[1]);
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
      if (!plots.size) makePlots(w - 12);
      else for (const p of plots.values()) p.setSize({ width: Math.max(200, w - 40), height: LANE_H });
    },
    dispose() {
      clearInterval(tick);
      statusAbort.abort();
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
        filter: (p) => ({ domains: ['psu'], channels: [typeof p.channel === 'string' ? p.channel : 'bench'] }),
        snapshot: 'required',
        mirrorRows: 1_000,
      },
    ],
    queries: ['psu.status'],
    actions: [
      // two actions on the same RPC: the gate applies each spec's rights/safety rule
      { id: 'emergencyOff', rpc: 'psu.out', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '비상 OFF' },
      { id: 'output', rpc: 'psu.out', class: 'request', right: 'operate', confirm: 'none', label: '출력 토글' },
      { id: 'set', rpc: 'psu.set', class: 'request', right: 'operate', confirm: 'none', label: '설정' },
    ],
    memory: { maxBytes: 2 * 1024 * 1024 },
    render: { maxHz: 10, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createPsuGauge,
);
