// psu.gauge (M1b, sim; _xteam/explore-ui.md). Same data source as the STOP panel's
// "출력 유지 장치" (psu.state / outputs_held).
//   [! 비상 OFF]  psu.out{state:'off'} — safety direction: any right incl. watch, NO
//                 confirmation, interlocks only warn (A3). Key `!` when the panel has focus.
//   [출력 ON/OFF] psu.out — R3 badge (F1: policy default auto, badge always shown)
//   [설정]        psu.set{v?, i?} — R2 badge
//   60 s plot     measured V / I (uPlot), a source.gap breaks the line (no interpolation)

import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { CHANNEL_PROP, registerPanel, type PanelContext, type PanelInstance } from '../../sdk';
import { fmt, monoS, readMetrics, reduceState, Series, type Meas, type PsuState } from './psuModel';

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

export function createPsuGauge(ctx: PanelContext): PanelInstance {
  const ch = typeof ctx.props.channel === 'string' ? ctx.props.channel : 'bench';
  const mirror = ctx.store('psu');
  // emergency OFF stays enabled regardless (safety direction)
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  let state: PsuState = { setV: null, setI: null, out: null, prot: null, mode: null };
  let meas: Meas = { v: null, i: null, p: null, atMs: null };
  const series = new Series();
  let lastSeq = -1n;
  let gaps = 0;
  let plot: uPlot | null = null;
  let plotErr: string | null = null;

  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- DOM ----
  const root = el('div', 'psu-panel');
  root.tabIndex = 0;
  const head = el('div', 'psu-head');
  head.setAttribute('data-testid', 'psu-head');
  const stopBadge = el('span', 'psu-stop', 'STOP — 출력 유지 중');
  stopBadge.hidden = true;
  const emergency = el('button', 'danger psu-emergency', '! 비상 OFF');
  emergency.type = 'button';
  emergency.title = '확인 없이 즉시 출력 OFF (보기 자격도 가능, 인터록은 경고만)';
  const vals = el('div', 'psu-vals');
  vals.setAttribute('data-testid', 'psu-vals');
  const ctl = el('div', 'psu-ctl');
  const outBtn = el('button', undefined, '출력 ON');
  outBtn.type = 'button';
  const r3 = el('span', 'risk risk-R3', 'R3');
  const vIn = Object.assign(el('input'), { placeholder: 'V', inputMode: 'decimal' });
  vIn.setAttribute('aria-label', '설정 전압');
  const iIn = Object.assign(el('input'), { placeholder: 'A', inputMode: 'decimal' });
  iIn.setAttribute('aria-label', '설정 전류');
  const setBtn = el('button', undefined, '설정');
  setBtn.type = 'button';
  const r2 = el('span', 'risk risk-R2', 'R2');
  ctl.append(outBtn, r3, ' ', vIn, iIn, setBtn, r2);
  const plotHost = el('div', 'psu-plot');
  const foot = el('div', 'muted');
  root.append(head, vals, ctl, plotHost, foot);

  const emergencyOff = () => void act('emergencyOff', { channel: ch, state: 'off' });
  emergency.addEventListener('click', emergencyOff);
  outBtn.addEventListener('click', () => void act('output', { channel: ch, state: state.out ? 'off' : 'on' }));
  setBtn.addEventListener('click', () => {
    const v = vIn.value.trim() === '' ? null : Number(vIn.value);
    const i = iIn.value.trim() === '' ? null : Number(iIn.value);
    if ((v !== null && !Number.isFinite(v)) || (i !== null && !Number.isFinite(i)) || (v === null && i === null)) {
      ctx.ui.toast('설정값을 확인하세요');
      return;
    }
    void act('set', { channel: ch, ...(v !== null ? { v } : {}), ...(i !== null ? { i } : {}) });
  });
  root.addEventListener('keydown', (e) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT') return;
    if (e.key === '!') {
      e.preventDefault();
      emergencyOff();
    }
  });

  const makePlot = (w: number, h: number) => {
    if (plot || plotErr) return;
    // no 2D canvas (headless / jsdom) → say so instead of failing inside uPlot's draw loop
    if (!document.createElement('canvas').getContext('2d')) {
      plotErr = '캔버스 없음';
      plotHost.textContent = '플롯 사용 불가: 캔버스 없음 (값은 위에 표시)';
      return;
    }
    try {
      plot = new uPlot(
        {
          width: Math.max(200, w),
          height: Math.max(80, h),
          scales: { x: { time: false } },
          series: [
            { label: 't (s)' },
            { label: 'V', stroke: '#19b3ad', scale: 'V', spanGaps: false },
            { label: 'A', stroke: '#e0b020', scale: 'A', spanGaps: false },
          ],
          axes: [{}, { scale: 'V', label: 'V' }, { scale: 'A', side: 1, label: 'A', grid: { show: false } }],
          legend: { show: true },
        },
        series.data(),
        plotHost,
      );
    } catch (e) {
      plotErr = e instanceof Error ? e.message : String(e);
      plotHost.textContent = `플롯 사용 불가: ${plotErr}`;
    }
  };

  const draw = (): bigint => {
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (ev.kind === 'psu.state') state = reduceState(state, ev);
      const m = readMetrics(ev);
      if (m) {
        meas = { ...m, atMs: performance.now() };
        series.push(monoS(ev), m.v, m.i);
      }
      if (ev.kind === 'source.gap') {
        gaps++;
        series.gap(monoS(ev)); // break the line — never interpolate across a loss
      }
    }
    const outText = state.out === null ? '출력 ?' : state.out ? '● 출력 ON' : '○ 출력 OFF';
    head.replaceChildren(
      el('span', undefined, `PSU ${ch} [${(ctx.env ?? '?').toUpperCase()}] `),
      el('span', `psu-out ${state.out ? 'on' : 'off'}`, outText),
      el('span', `psu-prot ${state.prot && state.prot !== 'ok' ? 'warn' : ''}`, ` 보호 ${state.prot ?? '?'}`),
      ' ',
      stopBadge,
      ' ',
      emergency,
    );
    stopBadge.hidden = !(ctx.stopLatched && state.out === true);
    const age = meas.atMs === null ? '' : ` (age ${Math.round(performance.now() - meas.atMs)} ms)`;
    vals.textContent = `설정 ${fmt(state.setV, 'V', 2)} / ${fmt(state.setI, 'A', 2)}${state.mode ? ` ${state.mode}` : ''} · 실측 ${fmt(meas.v, 'V')} ${fmt(meas.i, 'A')} ${fmt(meas.p, 'W', 2)}${age}`;
    const op = canOperate();
    outBtn.disabled = !op || state.out === null;
    outBtn.textContent = state.out ? '출력 OFF' : '출력 ON';
    setBtn.disabled = !op;
    vIn.disabled = !op;
    iIn.disabled = !op;
    emergency.disabled = false; // safety direction: always available
    foot.textContent = `${gaps ? `측정 손실(SCPI 무응답) ${gaps}회 — 선을 끊어 표시 · ` : ''}60 s 창 · ${series.t.length}점`;
    plot?.setData(series.data());
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const tick = setInterval(() => ctx.scheduler.invalidate(), 1_000); // age counter

  return {
    mount(host) {
      host.append(root);
      makePlot(host.clientWidth - 12, 140);
      ctx.scheduler.invalidate();
    },
    resize(w) {
      if (!plot) makePlot(w - 12, 140);
      else plot.setSize({ width: Math.max(200, w - 12), height: 140 });
    },
    dispose() {
      clearInterval(tick);
      offFrame();
      offAppend();
      plot?.destroy();
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
    queries: [],
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
