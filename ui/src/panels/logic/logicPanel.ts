// logic.analyzer — Loom-native logic analyzer panel (CONTRACT: work/reports/logic-live/CONTRACT.md, §8 frozen).
//
//   status    capture picker · state · source badge (real demo / device / SYNTHETIC) · rate · lanes · decoders
//             counters (samples, edges, annotations, drops, truncated lines) · lag · stop reason / exit / stderr
//             [■ 캡처 정지] (safety direction) · STOP latch line
//   config    source · driver (fx2lafw only) · pattern / text · samplerate · decoder (uart / spi + pins) · duration
//             → logic.start (right operate; disabled for watch / write-locked). Equivalent CLI line shown.
//   view      zoom − + · fit · live follow · lane visibility (display only — acquisition is always D0..D7)
//   waveform  RAW lanes (sample-exact steps, dense columns, hatched UNKNOWN) | DECODED tracks per decoder id
//   table     decoded annotations: sample start–end · t · duration · decoder · class · text → click centres the view
//
// Lifecycle (logic.capture_state, priority kind) arrives through ctx.onControl AND snapshot/backlog; a per-capture
// seq guard means an older state never overrides a newer one. Stopped captures stay viewable (retained in memory).

import {
  CHANNEL_PROP,
  registerPanel,
  VirtualTable,
  type Column,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
} from '../../sdk';
import {
  CaptureBook,
  CAPTURE_CHANNELS,
  cliLine,
  DEFAULT_CONFIG,
  fmtRate,
  fmtSampleTime,
  fmtSeconds,
  MAX_CHANNELS,
  readAnnotations,
  readCapability,
  readEdges,
  readSnapshotItem,
  REASON_LABEL,
  SOURCE_BADGE,
  startParams,
  STATE_LABEL,
  SUPPORTED_DEVICE_DRIVERS,
  UNSUPPORTED_DEVICE_NOTES,
  validateConfig,
  type Annotation,
  type Capability,
  type CaptureData,
  type StartConfig,
} from './logicModel';
import { ANN_H, AXIS_H, GUTTER, LANE_H, paint, type Palette, type View } from './waveform';
import './logic.css';

export const LOGIC_KINDS = ['logic.capture_state', 'logic.edges', 'logic.annotation', 'logic.annotations', 'source.gap', 'record.gap'] as const;
const DEMO_PATTERNS = ['graycode', 'sigrok', 'random', 'incremental', 'walking-one', 'walking-zero', 'squid', 'all-low', 'all-high'];
// backend bounds 1 kHz – 10 MHz (CONTRACT §9)
const RATES = [1_000, 10_000, 100_000, 200_000, 500_000, 1_000_000, 2_000_000, 4_000_000, 8_000_000, 10_000_000];
const BAUDS = [9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600, 1_000_000];
const CFG_KEY = 'loom.logic.config';
const LANES_KEY = 'loom.logic.lanes';
const MIN_SPP = 1 / 64; // ≤ 64 px per sample
/** default visible window on the sample clock */
export const DEFAULT_WINDOW_S = 0.01;
const MAX_SPP = 2 ** 24;

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const fmtN = (n: number) => n.toLocaleString('ko-KR');
const seqOf = (s: string): bigint => (/^\d{1,20}$/.test(s) ? BigInt(s) : -1n);

function loadJson<T>(key: string, check: (v: unknown) => v is T): T | null {
  try {
    const v = JSON.parse(localStorage.getItem(key) ?? 'null') as unknown;
    return check(v) ? v : null;
  } catch {
    return null;
  }
}
function saveJson(key: string, v: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* per-viewer convenience only */
  }
}
const isCfg = (v: unknown): v is Partial<StartConfig> => typeof v === 'object' && v !== null;
const isLanes = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v > 0 && v < 1 << MAX_CHANNELS;

function loadConfig(): StartConfig {
  const saved = loadJson(CFG_KEY, isCfg) ?? {};
  const c = { ...DEFAULT_CONFIG };
  for (const k of Object.keys(c) as (keyof StartConfig)[]) {
    const v = (saved as Record<string, unknown>)[k];
    if (v !== undefined && (typeof v === typeof c[k] || (v === null && (k === 'spiMiso' || k === 'spiCs')))) (c as Record<string, unknown>)[k] = v;
  }
  if (c.source !== 'synthetic' && c.source !== 'sigrok-demo' && c.source !== 'sigrok-device') c.source = DEFAULT_CONFIG.source;
  return c;
}

function readPalette(root: HTMLElement): Palette {
  const cs = typeof getComputedStyle === 'function' ? getComputedStyle(root) : null;
  const v = (name: string, fb: string) => cs?.getPropertyValue(name).trim() || fb;
  return {
    bg: v('--lg-wave-bg', '#fdfdfb'),
    grid: v('--lg-grid', '#e2e5e9'),
    fg: v('--lg-fg', '#1d2733'),
    muted: v('--lg-muted', '#66717f'),
    hi: v('--lg-hi', '#0f7b5f'),
    lo: v('--lg-lo', '#0f7b5f'),
    dense: v('--lg-dense', '#6fb9a3'),
    unknown: v('--lg-unknown', '#c08a2b'),
    ann: v('--lg-ann', '#3d5fc4'),
    annText: v('--lg-ann-text', '#1b2a5c'),
    cursor: v('--lg-cursor', '#d0402b'),
    sel: v('--lg-sel', '#d0402b'),
  };
}

export function createLogicAnalyzer(ctx: PanelContext): PanelInstance {
  const pinned = typeof ctx.props.channel === 'string' ? ctx.props.channel : null;
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  const mirror = ctx.store('logic');
  const book = new CaptureBook();
  let lastSeq = -1n;
  let gapsSeen = 0;
  let snapshotVersion = -1;
  let streamGaps = 0;
  /** user picked a capture in the selector; null = follow newest */
  let chosen: string | null = pinned;
  let cfg = loadConfig();
  let laneMask = loadJson(LANES_KEY, isLanes) ?? 0xff;
  let capability: Capability | null = null;
  let capErr: string | null = null;
  let starting = false;
  let stopRequested: string | null = null;
  // view
  let spp = 0; // 0 = not initialised yet
  let s0 = 0;
  let follow = true;
  let width = 800;
  let hoverX: number | null = null;
  let selected: Annotation | null = null;
  let paintKey = '';
  let tableKey = '';

  // ---- DOM ----
  const root = el('div', 'lg-panel');
  root.tabIndex = -1;
  const status = el('div', 'lg-status');
  status.setAttribute('data-testid', 'logic-status');
  const capSel = el('select', 'lg-capsel');
  capSel.setAttribute('aria-label', '캡처 선택');
  capSel.setAttribute('data-testid', 'logic-capture-select');
  const stateChip = el('span', 'lg-chip');
  stateChip.setAttribute('data-testid', 'logic-state');
  const srcBadge = el('span', 'lg-badge');
  srcBadge.setAttribute('data-testid', 'logic-source');
  const meta = el('span', 'lg-meta');
  meta.setAttribute('data-testid', 'logic-meta');
  const stopBtn = el('button', 'lg-stop', '■ 캡처 정지');
  stopBtn.type = 'button';
  stopBtn.title = '캡처 정지 — 안전 방향 (보기 권한도 가능)';
  stopBtn.setAttribute('data-testid', 'logic-stop');
  status.append(capSel, stateChip, srcBadge, meta, stopBtn);

  const counters = el('div', 'lg-counters');
  counters.setAttribute('data-testid', 'logic-counters');
  const notice = el('div', 'lg-notice');
  notice.setAttribute('data-testid', 'logic-notice');
  notice.setAttribute('role', 'status');
  const latchEl = el('div', 'lg-latch');
  latchEl.setAttribute('data-testid', 'logic-latch');

  // config
  const cfgBox = el('details', 'lg-config');
  cfgBox.setAttribute('data-testid', 'logic-config');
  const cfgSum = el('summary', undefined, '새 캡처 설정');
  const cfgGrid = el('div', 'lg-cfg-grid');
  const field = (label: string, input: HTMLElement, cls = ''): HTMLLabelElement => {
    const l = el('label', `lg-field ${cls}`.trim());
    l.append(el('span', 'lg-field-label', label), input);
    return l;
  };
  const sel = (name: string, opts: [string, string, boolean?][]): HTMLSelectElement => {
    const s = el('select');
    s.name = name;
    for (const [v, t, dis] of opts) {
      const o = el('option', undefined, t);
      o.value = v;
      if (dis) o.disabled = true;
      s.append(o);
    }
    return s;
  };
  const inp = (name: string, type = 'text'): HTMLInputElement => {
    const i = el('input');
    i.name = name;
    i.type = type;
    return i;
  };
  const pinOpts = (none = false): [string, string][] => [...(none ? ([['', '없음']] as [string, string][]) : []), ...CAPTURE_CHANNELS.map((c, i) => [String(i), c] as [string, string])];

  const fSource = sel('source', [
    ['synthetic', 'synthetic — Loom 생성 파형 + 실제 libsigrok 디코드'],
    ['sigrok-demo', 'sigrok-demo — libsigrok demo 드라이버 실제 acquisition'],
    ['sigrok-device', 'sigrok-device — 실제 장비 (드라이버 명시)'],
  ]);
  const fDriver = sel('driver', [
    ['', '드라이버 선택…'],
    ...SUPPORTED_DEVICE_DRIVERS.map((d) => [d, `${d} (8-bit, 실물 미검증)`] as [string, string]),
    ...UNSUPPORTED_DEVICE_NOTES.map((u) => [u.driver, `${u.driver} — ${u.note}`, true] as [string, string, boolean]),
  ]);
  const fConn = inp('conn');
  fConn.placeholder = '선택: conn=bus.addr';
  const fPattern = sel('pattern', [['', '패턴 지정 안 함 (기본 패턴 · 샘플레이트 선택 가능)'], ...DEMO_PATTERNS.map((p) => [p, `${p} (200 kHz 고정)`] as [string, string])]);
  const fText = inp('text');
  fText.maxLength = 512;
  fText.title = '{n} = 4자리 프레임 번호, \\r \\n 이스케이프 사용 가능 (≤ 256 B)';
  const fPeriod = inp('frame_period', 'number');
  fPeriod.min = '1';
  fPeriod.max = '10000';
  const fRate = sel('samplerate', RATES.map((r) => [String(r), fmtRate(r)]));
  const fDuration = inp('duration', 'number');
  fDuration.min = '0.1';
  fDuration.max = '3600';
  fDuration.step = '0.1';
  const fDecoder = sel('decoder', [
    ['uart', 'UART (libsigrok uart)'],
    ['spi', 'SPI (libsigrok spi)'],
    ['none', '디코드 없음 (원시만)'],
  ]);
  const fBaud = sel('baud', BAUDS.map((b) => [String(b), String(b)]));
  const fRx = sel('rx', pinOpts());
  const fClk = sel('clk', pinOpts());
  const fMosi = sel('mosi', pinOpts());
  const fMiso = sel('miso', pinOpts(true));
  const fCs = sel('cs', pinOpts(true));
  const lSource = field('소스', fSource, 'lg-wide');
  const lDriver = field('드라이버', fDriver);
  const lConn = field('연결', fConn);
  const lPattern = field('demo 패턴', fPattern);
  const lText = field('합성 텍스트', fText);
  const lPeriod = field('프레임 주기 (ms)', fPeriod);
  const lRate = field('샘플레이트', fRate);
  const lDuration = field('캡처 시간 (s)', fDuration);
  const lDecoder = field('디코더', fDecoder);
  const lBaud = field('baudrate', fBaud);
  const lRx = field('RX', fRx);
  const lClk = field('CLK', fClk);
  const lMosi = field('MOSI', fMosi);
  const lMiso = field('MISO', fMiso);
  const lCs = field('CS#', fCs);
  const acqNote = el('p', 'lg-hint', `수집: ${CAPTURE_CHANNELS[0]}–${CAPTURE_CHANNELS[CAPTURE_CHANNELS.length - 1]} 8채널 고정 (1 byte/sample). 표시 레인은 아래 보기 막대에서 고릅니다.`);
  const deviceNote = el('p', 'lg-hint lg-device-note', '실제 장비: 첫 slice는 fx2lafw 8-bit 계열만 허용 — 실물 호환은 아직 검증되지 않았습니다. Saleae Logic16 / Logic Pro 16은 미지원. USB 스캔은 하지 않습니다.');
  cfgGrid.append(lSource, lDriver, lConn, lPattern, lText, lPeriod, lRate, lDuration, lDecoder, lBaud, lRx, lClk, lMosi, lMiso, lCs);
  const cfgFoot = el('div', 'lg-cfg-foot');
  const startBtn = el('button', 'lg-start', '▶ 캡처 시작');
  startBtn.type = 'button';
  startBtn.setAttribute('data-testid', 'logic-start');
  const cfgProblems = el('ul', 'lg-problems');
  cfgProblems.setAttribute('data-testid', 'logic-config-problems');
  const cli = el('code', 'lg-cli');
  cli.setAttribute('data-testid', 'logic-cli');
  const capLine = el('p', 'lg-hint lg-capline');
  capLine.setAttribute('data-testid', 'logic-capability');
  cfgFoot.append(startBtn, cfgProblems);
  cfgBox.append(cfgSum, acqNote, cfgGrid, deviceNote, cfgFoot, el('span', 'lg-field-label', '같은 캡처를 CLI로'), cli, capLine);

  // view toolbar
  const viewBar = el('div', 'lg-viewbar');
  const btn = (t: string, title: string, testid: string) => {
    const b = el('button', undefined, t);
    b.type = 'button';
    b.title = title;
    b.setAttribute('data-testid', testid);
    return b;
  };
  const zoomOut = btn('−', '축소 (-)', 'logic-zoom-out');
  const zoomIn = btn('+', '확대 (+)', 'logic-zoom-in');
  const fitBtn = btn('전체', '전체 보기 (0)', 'logic-fit');
  const followBtn = btn('● 라이브 따라가기', '최신 샘플 따라가기 (F / End)', 'logic-follow');
  followBtn.setAttribute('aria-pressed', 'true');
  const windowInfo = el('span', 'lg-window');
  windowInfo.setAttribute('data-testid', 'logic-window');
  const laneBox = el('span', 'lg-lanes');
  laneBox.setAttribute('aria-label', '표시 레인');
  const laneChecks = CAPTURE_CHANNELS.map((name, bit) => {
    const l = el('label', 'lg-lane-toggle');
    const c = el('input');
    c.type = 'checkbox';
    c.dataset.bit = String(bit);
    c.checked = (laneMask & (1 << bit)) !== 0;
    l.append(c, name);
    laneBox.append(l);
    return c;
  });
  viewBar.append(zoomOut, zoomIn, fitBtn, followBtn, windowInfo, laneBox);

  const waveHost = el('div', 'lg-wave');
  waveHost.tabIndex = 0;
  waveHost.setAttribute('role', 'img');
  waveHost.setAttribute('data-testid', 'logic-wave');
  const canvas = el('canvas');
  const readout = el('div', 'lg-readout');
  readout.setAttribute('data-testid', 'logic-readout');
  const empty = el('div', 'lg-empty');
  empty.setAttribute('data-testid', 'logic-empty');
  waveHost.append(canvas, empty);
  const legend = el('div', 'lg-legend');
  for (const [cls, t] of [
    ['lg-key-raw', '원시 파형 (logic.edges, sample 정확)'],
    ['lg-key-dense', '1px 안에 전이 ≥2 (밀집)'],
    ['lg-key-unknown', '데이터 없음 — 생략/손실 구간'],
    ['lg-key-ann', '디코드 해석 (libsigrok PD 구간)'],
  ] as const) {
    const s = el('span', 'lg-key');
    s.append(el('i', cls), t);
    legend.append(s);
  }

  const annHead = el('div', 'lg-ann-head');
  const annTitle = el('strong', undefined, '디코드 결과');
  const annInfo = el('span', 'lg-muted');
  annInfo.setAttribute('data-testid', 'logic-ann-info');
  annHead.append(annTitle, annInfo);
  const tableHost = el('div', 'lg-table');
  const rawBox = el('details', 'lg-rawlines');
  rawBox.setAttribute('data-testid', 'logic-raw-lines');
  const rawSum = el('summary');
  const rawPre = el('pre');
  rawBox.append(rawSum, rawPre);
  root.append(status, counters, latchEl, notice, cfgBox, viewBar, waveHost, readout, legend, annHead, tableHost, rawBox);

  // ---- helpers ----
  const current = (): CaptureData | null => {
    const id = chosen ?? book.newestId;
    return id ? (book.captures.get(id) ?? null) : null;
  };
  const rateOf = (c: CaptureData | null) => c?.samplerate ?? null;
  const lanes = (c: CaptureData | null) => {
    const names = c?.info?.channels.length ? c.info.channels : CAPTURE_CHANNELS;
    // capture `channels` = display selection made at start (names "D3" → bit 3); laneMask hides more locally
    return names
      .map((name) => ({ bit: /^D[0-7]$/.test(name) ? Number(name.slice(1)) : -1, name }))
      .filter((l) => l.bit >= 0 && l.bit < MAX_CHANNELS && (laneMask & (1 << l.bit)) !== 0);
  };
  /** display channels sent with start (acquisition stays D0..D7) */
  const shownChannels = () => CAPTURE_CHANNELS.filter((_, bit) => (laneMask & (1 << bit)) !== 0);
  const decodersOf = (c: CaptureData | null): string[] => {
    const seen: string[] = [];
    for (const a of c?.annotations ?? []) if (!seen.includes(a.decoder)) seen.push(a.decoder);
    return seen;
  };
  const view = (): View => ({ s0, spp: spp || 1, width });
  /** spp = 0 is the "not sized yet" sentinel and is never clamped into a real zoom */
  const clampView = () => {
    if (spp > 0) spp = Math.min(MAX_SPP, Math.max(MIN_SPP, spp));
    if (s0 < 0) s0 = 0;
  };
  const followUi = (on: boolean) => {
    follow = on;
    followBtn.setAttribute('aria-pressed', String(on));
    followBtn.classList.toggle('on', on);
  };
  const setFollow = (on: boolean) => {
    followUi(on);
    ctx.scheduler.invalidate();
  };
  /** capture the current zoom belongs to */
  let viewCap: string | null = null;
  /**
   * View rule: a user zoom/pan belongs to one capture. When the shown capture changes (a new latest capture while
   * following, or a pick in the selector) the view resets to the default window + live follow. The default window
   * (10 ms of sample clock, or 10 000 samples without a rate) is sized only once the capture has a samplerate or data;
   * until then spp stays 0 (no capture / nothing to size by → no fake zoom).
   */
  const syncView = (c: CaptureData | null) => {
    if (!c) return;
    if (viewCap !== c.id) {
      viewCap = c.id;
      spp = 0;
      s0 = 0;
      selected = null;
      followUi(true);
    }
    if (spp) return;
    const r = rateOf(c);
    if (!r && !c.chunks.length && !c.annotations.length) return;
    spp = Math.max(MIN_SPP, (r ? r * DEFAULT_WINDOW_S : 10_000) / width);
  };
  const zoom = (factor: number, anchorX = width) => {
    const c = current();
    syncView(c);
    if (!c || !spp) return;
    const anchor = s0 + anchorX * spp;
    spp *= factor;
    clampView();
    s0 = anchor - anchorX * spp;
    clampView();
    ctx.scheduler.invalidate();
  };
  const pan = (px: number) => {
    if (!spp) return;
    s0 += px * spp;
    clampView();
    setFollow(false);
  };
  const fit = () => {
    const c = current();
    if (!c) return;
    const from = Math.max(c.evictedTo, c.range?.start ?? 0);
    const to = Math.max(c.headSample, from + 1);
    spp = (to - from) / width;
    s0 = from;
    clampView();
    setFollow(false);
  };
  const centerOn = (a: Annotation) => {
    const c = current();
    syncView(c);
    if (!c || !spp) return;
    const span = Math.max(1, a.end - a.start);
    if (span / spp > width * 0.8 || span / spp < 8) spp = Math.max(MIN_SPP, (span * 4) / width);
    s0 = (a.start + a.end) / 2 - (width / 2) * spp;
    clampView();
    setFollow(false);
  };

  // ---- config binding ----
  const writeForm = () => {
    fSource.value = cfg.source;
    fDriver.value = cfg.driver;
    fConn.value = cfg.conn;
    fPattern.value = cfg.pattern;
    fText.value = cfg.text;
    fPeriod.value = String(cfg.framePeriodMs);
    fRate.value = RATES.includes(cfg.samplerate) ? String(cfg.samplerate) : String(DEFAULT_CONFIG.samplerate);
    fDuration.value = String(cfg.durationS);
    fDecoder.value = cfg.decoder;
    fBaud.value = String(cfg.uartBaud);
    fRx.value = String(cfg.uartRx);
    fClk.value = String(cfg.spiClk);
    fMosi.value = String(cfg.spiMosi);
    fMiso.value = cfg.spiMiso === null ? '' : String(cfg.spiMiso);
    fCs.value = cfg.spiCs === null ? '' : String(cfg.spiCs);
  };
  const readForm = () => {
    const pin = (s: HTMLSelectElement) => (s.value === '' ? null : Number(s.value));
    cfg = {
      ...cfg,
      source: fSource.value as StartConfig['source'],
      driver: fDriver.value,
      conn: fConn.value,
      pattern: fPattern.value,
      text: fText.value,
      framePeriodMs: Number(fPeriod.value),
      samplerate: Number(fRate.value),
      durationS: Number(fDuration.value),
      decoder: fDecoder.value as StartConfig['decoder'],
      uartBaud: Number(fBaud.value),
      uartRx: Number(fRx.value),
      spiClk: Number(fClk.value),
      spiMosi: Number(fMosi.value),
      spiMiso: pin(fMiso),
      spiCs: pin(fCs),
    };
    saveJson(CFG_KEY, cfg);
    ctx.scheduler.invalidate();
  };
  writeForm();
  cfgBox.addEventListener('change', readForm);
  cfgBox.addEventListener('input', readForm);

  const refreshCapability = () => {
    ctx
      .query<unknown>('logic.status', {})
      .then((r) => {
        capability = readCapability(r);
        capErr = null;
        ctx.scheduler.invalidate();
      })
      .catch((e: unknown) => {
        capErr = e instanceof Error ? e.message : String(e);
        ctx.scheduler.invalidate();
      });
  };
  cfgBox.addEventListener('toggle', () => {
    if (cfgBox.open && !capability) refreshCapability();
  });

  // ---- actions ----
  startBtn.addEventListener('click', () => {
    readForm();
    if (validateConfig(cfg).length || !canOperate() || starting) return;
    starting = true;
    chosen = pinned; // the new capture becomes the followed one; syncView resets the view when its id arrives
    setFollow(true);
    ctx
      .act('start', startParams(cfg, shownChannels()))
      .then(() => ctx.ui.toast('캡처 시작 요청'))
      .catch((e: unknown) => ctx.ui.toast(`캡처 시작 실패: ${e instanceof Error ? e.message : String(e)}`))
      .finally(() => {
        starting = false;
        ctx.scheduler.invalidate();
      });
    ctx.scheduler.invalidate();
  });
  stopBtn.addEventListener('click', () => {
    const c = current();
    if (!c) return;
    stopRequested = c.id;
    ctx
      .act('stop', { capture_id: c.id })
      .catch((e: unknown) => {
        stopRequested = null;
        ctx.ui.toast(`캡처 정지 실패: ${e instanceof Error ? e.message : String(e)}`);
      })
      .finally(() => ctx.scheduler.invalidate());
    ctx.scheduler.invalidate();
  });
  capSel.addEventListener('change', () => {
    chosen = capSel.value === '' ? null : capSel.value;
    viewCap = null; // explicit pick → default window
    selected = null;
    setFollow(true);
  });

  // ---- view interaction ----
  zoomIn.addEventListener('click', () => zoom(0.5, follow ? width : width / 2));
  zoomOut.addEventListener('click', () => zoom(2, follow ? width : width / 2));
  fitBtn.addEventListener('click', fit);
  followBtn.addEventListener('click', () => setFollow(!follow));
  laneBox.addEventListener('change', () => {
    laneMask = laneChecks.reduce((m, c, i) => (c.checked ? m | (1 << i) : m), 0);
    if (!laneMask) {
      laneMask = 1;
      laneChecks[0]!.checked = true;
    }
    saveJson(LANES_KEY, laneMask);
    ctx.scheduler.invalidate();
  });
  const localX = (e: MouseEvent) => e.clientX - waveHost.getBoundingClientRect().left - GUTTER;
  waveHost.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
      pan((e.shiftKey ? e.deltaY : e.deltaX) || 0);
      return;
    }
    const x = Math.max(0, Math.min(width, localX(e)));
    zoom(e.deltaY > 0 ? 1.25 : 0.8, follow ? width : x);
  }, { passive: false });
  let drag: { x: number; s0: number } | null = null;
  waveHost.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    drag = { x: e.clientX, s0 };
    waveHost.setPointerCapture?.(e.pointerId);
  });
  waveHost.addEventListener('pointermove', (e) => {
    hoverX = localX(e);
    if (drag && spp) {
      const dx = e.clientX - drag.x;
      if (Math.abs(dx) > 2) {
        s0 = Math.max(0, drag.s0 - dx * spp);
        if (follow) setFollow(false);
      }
    }
    ctx.scheduler.invalidate();
  });
  const endDrag = () => {
    drag = null;
  };
  waveHost.addEventListener('pointerup', endDrag);
  waveHost.addEventListener('pointercancel', endDrag);
  waveHost.addEventListener('pointerleave', () => {
    hoverX = null;
    ctx.scheduler.invalidate();
  });
  waveHost.addEventListener('keydown', (e) => {
    const k = e.key;
    if (k === 'ArrowLeft' || k === 'ArrowRight') pan((k === 'ArrowLeft' ? -1 : 1) * width * 0.2);
    else if (k === '+' || k === '=') zoom(0.5, follow ? width : width / 2);
    else if (k === '-' || k === '_') zoom(2, follow ? width : width / 2);
    else if (k === '0') fit();
    else if (k === 'f' || k === 'F' || k === 'End') setFollow(true);
    else if (k === 'Home') {
      s0 = current()?.evictedTo ?? 0;
      setFollow(false);
    } else return;
    e.preventDefault();
  });
  const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(() => ctx.scheduler.invalidate()) : null;
  ro?.observe(waveHost);

  // ---- annotation table ----
  const cols: Column<Annotation>[] = [
    { key: 'samples', header: 'sample 시작–끝', width: '13em', cell: (a) => ({ text: `${fmtN(a.start)}–${fmtN(a.end)}`, cls: 'lg-num' }) },
    { key: 't', header: '시각 (sample clock)', width: '9em', cell: (a) => ({ text: a.tStartNs !== null ? fmtSeconds(a.tStartNs / 1e9, 4) : '?', cls: 'lg-num' }) },
    { key: 'dur', header: '길이', width: '6.5em', cell: (a) => ({ text: a.tStartNs !== null && a.tEndNs !== null ? fmtSeconds((a.tEndNs - a.tStartNs) / 1e9, 2) : `${fmtN(a.end - a.start)} smp`, cls: 'lg-num' }) },
    { key: 'dec', header: '디코더', width: '7em', cell: (a) => a.decoder },
    { key: 'cls', header: 'class', width: '7em', cell: (a) => (a.cls === null ? { text: '—', cls: 'lg-muted', title: 'sigrok-cli가 class를 제공하지 않음 (임의 생성하지 않음)' } : a.cls) },
    { key: 'text', header: '내용', width: '1fr', cell: (a) => ({ text: a.text, cls: a.cls === '_raw' ? 'lg-rawline' : 'lg-anntext' }) },
  ];
  let tableItems: Annotation[] = [];
  const table = new VirtualTable<Annotation>(tableHost, {
    columns: cols,
    keyOf: (a) => a.key,
    rowAttrs: (a) => ({ 'data-start': String(a.start), 'data-decoder': a.decoder }),
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: (key) => {
      selected = key ? (tableItems.find((a) => a.key === key) ?? null) : null;
      if (selected) centerOn(selected);
      ctx.scheduler.invalidate();
    },
    emptyText: '디코드 결과 없음',
    ariaLabel: '디코드 결과',
  });

  // ---- ingest ----
  const handle = (ev: LoomEvent) => {
    const p = obj(ev.payload);
    const seq = seqOf(ev.seq);
    const chan = ev.channel ?? null;
    switch (ev.kind) {
      case 'logic.capture_state':
        book.applyState(p, chan, seq);
        return;
      case 'logic.edges': {
        const mono = /^\d{1,20}$/.test(ev.ts?.mono_ns ?? '') ? BigInt(ev.ts.mono_ns) : null;
        const c = readEdges(p, chan, seq, mono);
        if (c) book.get(c.captureId).addChunk(c);
        return;
      }
      case 'logic.annotation':
      case 'logic.annotations': {
        const id = (typeof p.capture_id === 'string' && p.capture_id) || chan;
        const rate = id ? (book.captures.get(id)?.samplerate ?? null) : null;
        const r = readAnnotations(ev.kind, p, chan, seq, rate);
        if (r.captureId) {
          const cap = book.get(r.captureId);
          cap.addAnnotations(r.rows, r.dropped);
          cap.addRawLines(r.rawLines, r.rejected);
        }
        return;
      }
      case 'source.gap':
      case 'record.gap':
        streamGaps++;
        for (const c of book.captures.values()) if (c.info?.state === 'running') c.noteStreamGap();
        return;
      default:
        return;
    }
  };
  const applySnapshot = () => {
    const snap = mirror.snapshot;
    if (!snap || snap.version === snapshotVersion) return;
    snapshotVersion = snap.version;
    for (const item of snap.items) {
      for (const sc of readSnapshotItem(item)) {
        if (pinned && sc.id !== pinned) continue;
        // snapshot payloads carry no record seq: they are as of the snapshot high-water H (seq ≤ H later = stale)
        if (sc.state) book.applyState(sc.state, sc.id, snap.throughSeq);
        const cap = book.get(sc.id);
        for (const e of sc.edges) {
          const c = readEdges(e, sc.id, -1n, null);
          if (c) cap.addChunk(c);
        }
        if (sc.annotations.length) {
          const r = readAnnotations('logic.annotations', { capture_id: sc.id, annotations: sc.annotations }, sc.id, -1n, cap.samplerate);
          cap.addAnnotations(r.rows, 0); // keyed by span: backlog/live duplicates are not double-listed
          cap.addRawLines(r.rawLines, r.rejected);
        }
      }
    }
  };
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const offControl = ctx.onControl((ev) => {
    if (ev.kind !== 'logic.capture_state') return;
    const p = obj(ev.payload);
    const id = (typeof p.capture_id === 'string' && p.capture_id) || ev.channel;
    if (pinned && id !== pinned) return;
    if (book.applyState(p, ev.channel ?? null, seqOf(ev.seq))) ctx.scheduler.invalidate();
  });
  const offLatch = ctx.latch.subscribe(() => ctx.scheduler.invalidate());

  // ---- draw ----
  const ctx2d = (): CanvasRenderingContext2D | null => {
    try {
      return canvas.getContext('2d');
    } catch {
      return null;
    }
  };
  let lastCapsKey = '';
  const renderCapSelect = () => {
    const list = book.list();
    const key = `${pinned ?? ''}|${chosen ?? ''}|${book.newestId ?? ''}|${list.map((c) => `${c.id}:${c.info?.state ?? '?'}`).join(',')}`;
    if (key === lastCapsKey) return;
    lastCapsKey = key;
    const opts: HTMLOptionElement[] = [];
    if (!pinned) {
      const o = el('option', undefined, book.newestId ? `최신 캡처 따라가기 (${book.newestId})` : '최신 캡처 따라가기');
      o.value = '';
      opts.push(o);
    }
    for (const c of list) {
      if (pinned && c.id !== pinned) continue;
      const o = el('option', undefined, `${c.id} · ${STATE_LABEL[c.info?.state ?? 'unknown']}`);
      o.value = c.id;
      opts.push(o);
    }
    if (pinned && !list.some((c) => c.id === pinned)) {
      const o = el('option', undefined, `${pinned} · 기록 대기`);
      o.value = pinned;
      opts.push(o);
    }
    capSel.replaceChildren(...opts);
    capSel.value = chosen ?? '';
    capSel.disabled = !!pinned;
  };

  const draw = (): bigint => {
    const t0 = performance.now();
    applySnapshot();
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      handle(ev);
    }
    while (gapsSeen < mirror.gaps.length) {
      gapsSeen++;
      streamGaps++;
      for (const c of book.captures.values()) if (c.info?.state === 'running' || c.info?.state === 'starting') c.noteStreamGap();
    }
    ctx.scheduler.reportWork(performance.now() - t0);

    const c = current();
    const info = c?.info ?? null;
    const st = info?.state ?? 'unknown';
    if (stopRequested && (st === 'stopped' || st === 'failed' || c?.id !== stopRequested)) stopRequested = null;
    renderCapSelect();

    // status line
    status.dataset.state = c ? st : 'none';
    stateChip.dataset.state = st;
    stateChip.textContent = c ? `● ${STATE_LABEL[st]}${stopRequested && st === 'running' ? ' · 정지 요청됨' : ''}` : '캡처 없음';
    const src = info?.source ?? null;
    const badge = src ? SOURCE_BADGE[src] : null;
    const synthetic = src === 'synthetic' || (info?.quality.includes('synthetic') ?? false);
    srcBadge.hidden = !src;
    srcBadge.className = `lg-badge ${badge?.cls ?? 'lg-src-other'}${synthetic ? ' lg-synthetic' : ''}`;
    srcBadge.textContent = badge ? badge.text : (src ?? '');
    srcBadge.title = badge?.title ?? '';
    srcBadge.dataset.source = src ?? '';
    const rate = rateOf(c);
    meta.textContent = c
      ? [
          info?.driver ? `driver ${info.driver}` : null,
          fmtRate(rate),
          `${(info?.channels.length ? info.channels : CAPTURE_CHANNELS).length}ch`,
          info?.decoders.length ? `디코더 ${info.decoders.join(' · ')}` : '디코더 없음',
          info?.maxDurationMs ? `상한 ${fmtSeconds(info.maxDurationMs / 1000, 1)}` : null,
          info?.requestedBy ? `시작 ${info.requestedBy === 'agent' ? 'AGT' : info.requestedBy === 'user' ? 'USR' : info.requestedBy === 'script' ? 'SCR' : info.requestedBy}` : null,
        ]
          .filter(Boolean)
          .join(' · ')
      : pinned
        ? `${pinned} — 이 캡처의 기록을 기다리는 중`
        : '활성 캡처 없음 — 아래 "새 캡처 설정"에서 시작하거나 에이전트 `loom logic start` 결과가 여기 표시됩니다';
    stopBtn.disabled = !c || !(st === 'running' || st === 'starting' || st === 'stopping' || st === 'unknown') || stopRequested === c.id;

    // counters + loss
    if (c) {
      const k = info?.counters;
      const head = c.headSample;
      const lagMs = c.lastHostMonoNs !== null && st === 'running' ? Number(ctx.clock.monoEstNs() - c.lastHostMonoNs) / 1e6 : null;
      const dropEdges = Math.max(k?.droppedEdges ?? 0, c.droppedEdges);
      const dropAnn = Math.max(k?.droppedAnnotations ?? 0, c.droppedAnnotations);
      const parts: [string, string, boolean][] = [
        ['samples', `샘플 ${fmtN(Math.max(k?.samples ?? 0, head))}${rate ? ` (${fmtSampleTime(Math.max(k?.samples ?? 0, head), rate, 2)})` : ''}`, false],
        ['edges', `전이 ${fmtN(Math.max(k?.edges ?? 0, c.edgeCount))}`, false],
        ['annotations', `디코드 ${fmtN(Math.max(k?.annotations ?? 0, c.annotations.length))}`, false],
        ['dropped_edges', `전이 생략 ${fmtN(dropEdges)}`, dropEdges > 0],
        ['dropped_annotations', `디코드 손실 ${fmtN(dropAnn)}`, dropAnn > 0],
        ['truncated_lines', `잘린 줄 ${fmtN(k?.truncatedLines ?? 0)}`, (k?.truncatedLines ?? 0) > 0],
        ['unparsed_lines', `해석 못 한 줄 ${fmtN(Math.max(k?.unparsedLines ?? 0, c.rawLinesTotal))}`, Math.max(k?.unparsedLines ?? 0, c.rawLinesTotal) > 0],
        ['rejected', `형식 오류 ${fmtN(c.malformedEdges + c.rejectedAnnotations + c.rejectedChunks)}`, c.malformedEdges + c.rejectedAnnotations + c.rejectedChunks > 0],
        ['stream_gaps', `스트림 손실 ${fmtN(streamGaps)}`, streamGaps > 0],
        ['lag', lagMs === null ? '수신 지연 —' : `수신 지연 ${lagMs < 0 ? 0 : Math.round(lagMs)} ms`, lagMs !== null && lagMs > 500],
      ];
      counters.replaceChildren(
        ...parts.map(([key, t, bad]) => {
          const s = el('span', bad ? 'lg-ctr lg-bad' : 'lg-ctr', t);
          s.dataset.counter = key;
          return s;
        }),
      );
      counters.hidden = false;
    } else counters.hidden = true;

    // stop / fail details, honesty notes
    const notes: string[] = [];
    if (info && (st === 'stopped' || st === 'failed')) {
      const why = info.reason ? (REASON_LABEL[info.reason] ?? info.reason) : '사유 미보고';
      const exit = info.exitText ? ` · ${info.exitText}` : '';
      notes.push(`${st === 'failed' ? '실패' : '정지'}: ${why}${exit}${info.error ? ` · ${info.error}` : ''} — 기록은 이 패널에 남아 있습니다`);
    }
    if (synthetic) notes.push('SYNTHETIC: 원시 파형은 Loom이 생성한 이상 파형입니다. 디코드는 실제 libsigrok PD 결과입니다.');
    if (src === 'sigrok-demo') notes.push('sigrok demo 드라이버: acquisition 경로는 실제이나 패턴 신호라 디코드 내용은 의미가 없습니다.');
    if (c && c.evictedTo > 0) notes.push(`보관 상한으로 sample ${fmtN(c.evictedTo)} 이전 원시 데이터는 화면에서 제외됨 (저널: loom logic read)`);
    notice.replaceChildren(...notes.map((n) => el('div', n.startsWith('실패') ? 'lg-note lg-note-err' : 'lg-note', n)));
    if (info?.stderrTail && (st === 'failed' || st === 'stopped')) {
      const d = el('details', 'lg-stderr');
      d.append(el('summary', undefined, 'stderr (마지막 4 KiB)'), el('pre', undefined, info.stderrTail));
      notice.append(d);
    }
    notice.hidden = notice.childElementCount === 0;
    latchEl.hidden = !ctx.latch.on;
    latchEl.textContent = ctx.latch.on
      ? `STOP 래치${ctx.latch.by ? ` (${ctx.latch.by})` : ''} — 에이전트의 캡처 시작은 거부되고 실행 중 캡처는 정지됩니다(reason stop_latch). 사용자 시작은 가능합니다.`
      : '';

    // config
    const problems = validateConfig(cfg);
    const showFor = (src2: StartConfig['source'][], ...labels: HTMLElement[]) => labels.forEach((l) => (l.hidden = !src2.includes(cfg.source)));
    showFor(['sigrok-device'], lDriver, lConn, deviceNote);
    showFor(['sigrok-demo'], lPattern);
    showFor(['synthetic'], lText, lPeriod);
    lBaud.hidden = lRx.hidden = cfg.decoder !== 'uart';
    lClk.hidden = lMosi.hidden = lMiso.hidden = lCs.hidden = cfg.decoder !== 'spi';
    const op = canOperate();
    const running = book.list().some((x) => x.info?.state === 'running' || x.info?.state === 'starting');
    startBtn.disabled = !op || problems.length > 0 || starting;
    startBtn.title = !op ? (ctx.writeLocked ? '쓰기 잠김 (연결 끊김/STALLED)' : '조작 권한(operate) 필요 — 보기 권한에서는 시작할 수 없습니다') : running ? '실행 중 캡처가 있으면 backend가 Busy로 거부합니다' : '';
    const pr = [...problems];
    if (!op) pr.push(ctx.writeLocked ? '쓰기 잠김 — 시작 불가' : '보기 권한 — 시작 불가 (정지는 가능)');
    else if (running) pr.push('실행 중 캡처가 있습니다 — 동시 캡처는 1개 (먼저 정지)');
    cfgProblems.replaceChildren(...pr.map((t) => el('li', undefined, t)));
    cli.textContent = cliLine(cfg, shownChannels());
    fRate.disabled = cfg.source === 'sigrok-demo' && !!cfg.pattern;
    lRate.title = fRate.disabled ? 'demo 패턴은 libsigrok demo 기본 200 kHz에서만 동작합니다' : '';
    capLine.textContent = capErr
      ? `logic.status 실패: ${capErr}`
      : capability
        ? `sigrok-cli ${capability.enginePath ? `${capability.enginePath}${capability.engineVersion ? ` (${capability.engineVersion.split('\n')[0]})` : ''}` : `없음${capability.engineError ? ` (${capability.engineError})` : ''} — synthetic 외 소스 불가`} · 구현 소스 ${capability.sources.join(', ') || '—'} · 디코더 ${capability.decoders.join(', ') || '—'} · 장비 ${capability.deviceDrivers.join(', ') || '—'} ${capability.deviceVerified ? '(검증됨)' : '(실물 미검증)'}${capability.compiledDrivers.length ? ` · 설치된 sigrok 컴파일 드라이버 ${capability.compiledDrivers.length}개(지원 의미 아님)` : ''}`
        : '설정을 열면 logic.status로 엔진 상태를 확인합니다';

    // view
    const w = Math.max(100, (waveHost.clientWidth || 900) - GUTTER);
    width = w;
    const ls = lanes(c);
    const decs = decodersOf(c);
    syncView(c);
    if (c && follow && spp) s0 = Math.max(c.evictedTo, c.headSample - width * spp);
    clampView();
    const v = view();
    windowInfo.textContent = c && spp ? `${fmtSampleTime(v.s0, rate)} – ${fmtSampleTime(v.s0 + v.spp * v.width, rate)} · ${v.spp >= 1 ? `${+v.spp.toFixed(1)} smp/px` : `${+(1 / v.spp).toFixed(1)} px/smp`}` : '';
    const heightPx = AXIS_H + ls.length * LANE_H + Math.max(1, decs.length) * ANN_H + 4;
    empty.hidden = !!c?.chunks.length || !!c?.annotations.length;
    empty.textContent = !c ? '캡처를 시작하면 채널별 원시 파형과 디코드 결과가 여기에 그려집니다' : st === 'failed' ? '원시 데이터 없음 — 캡처 실패' : '원시 데이터 대기 중…';
    waveHost.setAttribute(
      'aria-label',
      c ? `파형: ${ls.map((l) => l.name).join(', ')} · ${windowInfo.textContent} · 디코드 트랙 ${decs.join(', ') || '없음'}` : '파형 없음',
    );
    const pk = `${c?.id ?? ''}|${c?.version ?? 0}|${v.s0}|${v.spp}|${v.width}|${laneMask}|${hoverX}|${selected?.key ?? ''}|${decs.join(',')}`;
    if (pk !== paintKey) {
      paintKey = pk;
      const dpr = typeof devicePixelRatio === 'number' ? devicePixelRatio : 1;
      const W = GUTTER + width;
      if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(heightPx * dpr)) {
        canvas.width = Math.round(W * dpr);
        canvas.height = Math.round(heightPx * dpr);
        canvas.style.width = `${W}px`;
        canvas.style.height = `${heightPx}px`;
      }
      const g = ctx2d();
      if (g) {
        g.setTransform(dpr, 0, 0, dpr, 0, 0);
        paint(g, { cap: c, lanes: ls, decoders: decs.length ? decs : [], view: v, rate, palette: readPalette(root), hoverX, selected }, heightPx);
      }
    }
    // hover readout
    if (c && hoverX !== null && hoverX >= 0 && spp) {
      const s = Math.floor(v.s0 + hoverX * v.spp);
      const st2 = c.stateAt(s);
      const levels = ls.map((l) => `${l.name}=${st2 === null ? '?' : (st2 >> l.bit) & 1}`).join(' ');
      const ai = c.annotationAt(s);
      const hit = c.annotations.slice(ai, ai + 8).filter((a) => a.start <= s && s < Math.max(a.end, a.start + 1));
      readout.textContent = `sample ${fmtN(s)} · ${fmtSampleTime(s, rate, 4)} · ${levels}${st2 === null ? ' (데이터 없음)' : ''}${hit.length ? ` · 디코드 ${hit.map((a) => `${a.decoder}: ${a.text}`).join(' | ')}` : ''}`;
    } else {
      readout.textContent = c ? '파형 위로 마우스 — sample·시각·레벨 · 휠 확대/축소 · 끌기/Shift+휠 이동 · ←/→ +/- 0 F' : '';
    }

    // annotation table
    const tk = `${c?.id ?? ''}|${c?.version ?? 0}`;
    if (tk !== tableKey) {
      tableKey = tk;
      tableItems = c ? c.annotations.slice() : [];
      table.setItems(tableItems);
      if (follow) table.followTail();
    }
    table.render();
    const drops = c ? Math.max(c.info?.counters?.droppedAnnotations ?? 0, c.droppedAnnotations) : 0;
    annInfo.textContent = c
      ? `${fmtN(c.annotations.length)}행${c.evictedAnnotations ? ` · 이전 ${fmtN(c.evictedAnnotations)}행 보관 밖` : ''}${drops ? ` · 손실 ${fmtN(drops)}` : ''} · 디코더 ${decs.join(', ') || (info?.decoders.join(', ') ?? '') || '—'} · 해석 결과(원시 아님)`
      : '';
    rawBox.hidden = !c?.rawLinesTotal;
    if (c?.rawLinesTotal) {
      rawSum.textContent = `디코더가 낸 해석 불가 줄 ${fmtN(c.rawLinesTotal)}개 (sample 없음 — 파형에 그리지 않음)`;
      rawPre.textContent = c.rawLines.join('\n');
    }
    return lastSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      offAppend();
      offControl();
      offLatch();
      offFrame();
      ro?.disconnect();
      table.dispose();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'logic.analyzer',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'logic',
    title: '로직 분석기',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'logic',
        filter: (p) => ({
          domains: ['logic'],
          ...(typeof p.channel === 'string' ? { channels: [p.channel] } : {}),
          kinds: [...LOGIC_KINDS],
        }),
        snapshot: 'required',
        maxBatchMs: 50,
        mirrorRows: 4_000,
      },
    ],
    queries: ['logic.status'],
    actions: [
      // request-class R1 (CONTRACT §9: not an SDK job/renewal)
      { id: 'start', rpc: 'logic.start', class: 'request', right: 'operate', confirm: 'none', label: '로직 캡처 시작' },
      // STOP of an observation capture is safety direction: any right, never confirmed (eth.capture.stop pattern)
      { id: 'stop', rpc: 'logic.stop', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '로직 캡처 정지' },
    ],
    memory: { maxBytes: 32 * 1024 * 1024 },
    render: { maxHz: 30, budgetMs: 6, lagTargetMs: 250 },
  },
  createLogicAnalyzer,
);
