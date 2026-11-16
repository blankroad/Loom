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
  ARCHIVE_EDGE_PAGES,
  ARCHIVE_NOTE,
  ARCHIVE_READABLE,
  ARCHIVE_ROW_PAGES,
  archiveLabel,
  archiveNextStart,
  archivePrevStart,
  archiveSeekStart,
  archiveWindowAdd,
  archiveWindowAfterRow,
  archiveWindowCoverage,
  archiveWindowFull,
  archiveWindowLimit,
  archiveWindowStart,
  archiveWindowText,
  type ArchiveRowWindow,
  CaptureData,
  NOT_LOADED_DETAIL,
  num,
  readArchiveEdges,
  readArchiveRows,
  trackLabel,
  triggerText,
  type TriggerEdgeName,
  annCoverageText,
  CaptureBook,
  CAPTURE_CHANNELS,
  cliLine,
  coverageNote,
  decodeCoverage,
  DEFAULT_CONFIG,
  estimatePublishLoad,
  followHeadSample,
  rawCoverage,
  sanitizeConfig,
  type BitOrder,
  type CsPolarity,
  type SpiAnnotate,
  type UartLine,
  type UartParity,
  type UartStopBits,
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
  type StartConfig,
} from './logicModel';
import {
  linesToText,
  measureCursors,
  pulseAt,
  RADIXES,
  relocateHit,
  rowsToCsv,
  rowsToText,
  searchRows,
  splitLines,
  formatWord,
  type CopyScope,
  type Cursors,
  type Radix,
  type SearchMode,
  type SearchResult,
  type TextLine,
} from './logicTools';
import { ANN_H, AXIS_H, GUTTER, LANE_H, paint, type Palette, type View } from './waveform';
import './logic.css';

export const LOGIC_KINDS = ['logic.capture_state', 'logic.edges', 'logic.annotation', 'logic.annotations', 'source.gap', 'record.gap'] as const;
const DEMO_PATTERNS = ['graycode', 'sigrok', 'random', 'incremental', 'walking-one', 'walking-zero', 'squid', 'all-low', 'all-high'];
// backend bounds 1 kHz – 10 MHz (CONTRACT §9)
const RATES = [1_000, 10_000, 100_000, 200_000, 500_000, 1_000_000, 2_000_000, 4_000_000, 8_000_000, 10_000_000];
const BAUDS = [9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600, 1_000_000];
// L2 decoder option tables (value, label) — FUNCTIONS.md B1-P2. Values are the StartConfig / backend tokens.
const UART_LINE_OPTS: [UartLine, string][] = [
  ['rx', 'RX 라인'],
  ['tx', 'TX 라인'],
  ['both', 'RX + TX (한 캡처에 두 방향)'],
];
// B5 trigger form
const TRIGGER_EDGE_OPTS: [TriggerEdgeName, string][] = [
  ['falling', '하강 (1→0)'],
  ['rising', '상승 (0→1)'],
  ['either', '양쪽 에지'],
];
const TRIGGER_WAIT_TEXT = '트리거 대기 중 — 트리거 이후의 원시 파형과 디코드만 게시·보관됩니다';
const UART_DATA_BITS_OPTS: [string, string][] = [5, 6, 7, 8, 9].map((n) => [String(n), `${n} bit`]);
const UART_PARITY_OPTS: [UartParity, string][] = [
  ['none', '없음 (N)'],
  ['even', '짝수 (E)'],
  ['odd', '홀수 (O)'],
];
const UART_STOP_OPTS: [UartStopBits, string][] = [
  ['1', '1'],
  ['1.5', '1.5'],
  ['2', '2'],
];
const UART_ORDER_OPTS: [BitOrder, string][] = [
  ['lsb-first', 'LSB 먼저 (표준)'],
  ['msb-first', 'MSB 먼저'],
];
/** value = String(cpol * 2 + cpha) */
const SPI_MODE_OPTS: [string, string][] = [
  ['0', 'mode 0 (CPOL 0 · CPHA 0)'],
  ['1', 'mode 1 (CPOL 0 · CPHA 1)'],
  ['2', 'mode 2 (CPOL 1 · CPHA 0)'],
  ['3', 'mode 3 (CPOL 1 · CPHA 1)'],
];
const SPI_ORDER_OPTS: [BitOrder, string][] = [
  ['msb-first', 'MSB 먼저 (표준)'],
  ['lsb-first', 'LSB 먼저'],
];
const SPI_CS_POL_OPTS: [CsPolarity, string][] = [
  ['active-low', '액티브 로우 (표준)'],
  ['active-high', '액티브 하이'],
];
const SPI_ANNOTATE_OPTS: [SpiAnnotate, string][] = [
  ['mosi', 'MOSI'],
  ['miso', 'MISO'],
];
const ADV_SUMMARY = '고급 디코더 옵션 — 기본 8N1 · LSB / SPI mode 0 · 8 bit · MSB · CS 로우';
// B3a (L3/L4) table tools — FUNCTIONS.md B3-P*.
const RADIX_KEY = 'loom.logic.radix';
const RADIX_OPTS: [Radix, string][] = [
  ['hex', 'HEX'],
  ['ascii', 'ASCII'],
  ['dec', 'DEC'],
  ['bin', 'BIN'],
];
const TABLE_MODE_OPTS: [string, string][] = [
  ['rows', '행 (디코드 단어)'],
  ['lines', '줄 (CR/LF)'],
];
const SEARCH_MODE_OPTS: [SearchMode, string][] = [
  ['text', '텍스트'],
  ['bytes', '바이트 (hex)'],
];
const COPY_SCOPE_OPTS: [CopyScope, string][] = [
  ['all', '보유 전체'],
  ['cursors', '커서 A–B 구간'],
  ['selection', '선택 행/줄'],
];
const COPY_FORMAT_OPTS: [string, string][] = [
  ['txt', '텍스트'],
  ['csv', 'CSV'],
];
// B3b archive view / export
const VIEW_SOURCE_OPTS: [string, string][] = [
  ['live', '보기: 이 패널 수신분'],
  ['archive', '보기: 캡처 아카이브 (전체 기록)'],
];
const EXPORT_KIND_OPTS: [string, string][] = [
  ['annotations:csv', '디코드 CSV'],
  ['annotations:txt', '디코드 텍스트'],
  ['edges:csv', '원시 전이 CSV'],
];
const CURSOR_HINT = 'A/B 커서: 클릭=A · Shift+클릭=B · 1/2 키 · Esc 지우기';
const LINE_END_LABEL: Record<TextLine['end'], string> = { lf: 'LF', cr: 'CR', crlf: 'CRLF', none: '—', split: '잘림', note: '오류/주석' };
const isRadix = (v: unknown): v is Radix => typeof v === 'string' && (RADIXES as readonly string[]).includes(v);
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
  return sanitizeConfig(loadJson(CFG_KEY, isCfg));
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
  let followStopped: string | null = null;
  let width = 800;
  let hoverX: number | null = null;
  let selected: Annotation | null = null;
  let paintKey = '';
  let tableKey = '';
  // B3a state
  let radix: Radix = loadJson(RADIX_KEY, isRadix) ?? 'hex';
  let tableMode: 'rows' | 'lines' = 'rows';
  let cursors: Cursors = { a: null, b: null };
  let hoverY: number | null = null;
  let searchPattern = '';
  let searchMode: SearchMode = 'text';
  let searchCase = false;
  let searchRes: SearchResult | null = null;
  let searchIdx = -1;
  let searchKey = '';
  let searchQKey = '';
  let lineItems: TextLine[] = [];
  let selectedLine: TextLine | null = null;
  let hasClass = false;
  let tableSrc: 'live' | 'archive' = 'live';
  let heldErr = 0;
  let heldErrKey = '';
  // B3b state (archive view)
  let viewSrc: 'live' | 'archive' = 'live';
  let archCap: CaptureData | null = null;
  let archFor: string | null = null;
  let archRows: ArchiveRowWindow = archiveWindowStart(null);
  let archWin: { from: number; to: number } | null = null;
  let archRowsBusy = false;
  let archEdgesBusy = false;
  let archErr: string | null = null;
  let archGen = 0;
  let exportLine = '';

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
  const trigChip = el('span', 'lg-trigger');
  trigChip.setAttribute('data-testid', 'logic-trigger');
  trigChip.hidden = true;
  status.append(capSel, stateChip, srcBadge, trigChip, meta, stopBtn);

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
  const fMosi = sel('mosi', pinOpts(true));
  const fLine = sel('uart_line', UART_LINE_OPTS);
  const fTx = sel('uart_tx', pinOpts());
  const fMiso = sel('miso', pinOpts(true));
  const fCs = sel('cs', pinOpts(true));
  const fGlitch = inp('glitch_every', 'number');
  fGlitch.min = '0';
  fGlitch.max = '1000';
  fGlitch.step = '1';
  const fTrigOn = inp('trigger_on', 'checkbox');
  const fTrigCh = sel('trigger_ch', pinOpts());
  const fTrigEdge = sel('trigger_edge', TRIGGER_EDGE_OPTS);
  const fTrigPre = inp('trigger_pre_ms', 'number');
  fTrigPre.min = '0';
  fTrigPre.max = '1000';
  fTrigPre.step = '1';
  const fTrigPost = inp('trigger_post_ms', 'number');
  fTrigPost.min = '1';
  fTrigPost.step = '1';
  const lSource = field('소스', fSource, 'lg-wide');
  const lDriver = field('드라이버', fDriver);
  const lConn = field('연결', fConn);
  const lPattern = field('demo 패턴', fPattern);
  const lText = field('합성 텍스트', fText);
  const lPeriod = field('프레임 주기 (ms)', fPeriod);
  const lRate = field('샘플레이트', fRate);
  const lDuration = field('캡처 시간 (s)', fDuration);
  const lDecoder = field('디코더', fDecoder);
  const lGlitch = field('글리치 (n번째 바이트마다 프레임 오류, 0=끔)', fGlitch);
  const lTrigOn = field('소프트웨어 트리거', fTrigOn);
  const lTrigCh = field('트리거 채널', fTrigCh);
  const lTrigEdge = field('트리거 에지', fTrigEdge);
  const lTrigPre = field('트리거 앞 (ms)', fTrigPre);
  const lTrigPost = field('트리거 뒤 (ms)', fTrigPost);
  const lBaud = field('baudrate', fBaud);
  const lLine = field('UART 라인', fLine);
  const lRx = field('RX', fRx);
  const lTx = field('TX', fTx);
  const lClk = field('CLK', fClk);
  const lMosi = field('MOSI', fMosi);
  const lMiso = field('MISO', fMiso);
  const lCs = field('CS#', fCs);
  const acqNote = el('p', 'lg-hint', `수집: ${CAPTURE_CHANNELS[0]}–${CAPTURE_CHANNELS[CAPTURE_CHANNELS.length - 1]} 8채널 고정 (1 byte/sample). 표시 레인은 아래 보기 막대에서 고릅니다.`);
  const deviceNote = el('p', 'lg-hint lg-device-note', '실제 장비: 첫 slice는 fx2lafw 8-bit 계열만 허용 — 실물 호환은 아직 검증되지 않았습니다. Saleae Logic16 / Logic Pro 16은 미지원. USB 스캔은 하지 않습니다.');
  cfgGrid.append(lSource, lDriver, lConn, lPattern, lText, lPeriod, lGlitch, lRate, lDuration, lTrigOn, lTrigCh, lTrigEdge, lTrigPre, lTrigPost, lDecoder, lBaud, lLine, lRx, lTx, lClk, lMosi, lMiso, lCs);
  // advanced decoder options (collapsed by default; opened automatically when a saved value is non-default)
  const advBox = el('details', 'lg-adv');
  advBox.setAttribute('data-testid', 'logic-decoder-advanced');
  const advSum = el('summary', undefined, ADV_SUMMARY);
  const advGrid = el('div', 'lg-cfg-grid');
  const fDataBits = sel('uart_data_bits', UART_DATA_BITS_OPTS);
  const fParity = sel('uart_parity', UART_PARITY_OPTS);
  const fStop = sel('uart_stop_bits', UART_STOP_OPTS);
  const fUartOrder = sel('uart_bit_order', UART_ORDER_OPTS);
  const fSpiMode = sel('spi_mode', SPI_MODE_OPTS);
  const fWordsize = inp('spi_wordsize', 'number');
  fWordsize.min = '1';
  fWordsize.max = '32';
  fWordsize.step = '1';
  const fSpiOrder = sel('spi_bit_order', SPI_ORDER_OPTS);
  const fCsPol = sel('spi_cs_polarity', SPI_CS_POL_OPTS);
  const fAnnotate = sel('spi_annotate', SPI_ANNOTATE_OPTS);
  const lDataBits = field('데이터 비트', fDataBits);
  const lParity = field('패리티', fParity);
  const lStop = field('정지 비트', fStop);
  const lUartOrder = field('비트 순서', fUartOrder);
  const lSpiMode = field('SPI 모드', fSpiMode);
  const lWordsize = field('워드 (bit)', fWordsize);
  const lSpiOrder = field('비트 순서', fSpiOrder);
  const lCsPol = field('CS 극성', fCsPol);
  const lAnnotate = field('디코드 표시 라인', fAnnotate);
  const uartAdv = [lDataBits, lParity, lStop, lUartOrder];
  const spiAdv = [lSpiMode, lWordsize, lSpiOrder, lCsPol, lAnnotate];
  advGrid.append(...uartAdv, ...spiAdv);
  advBox.append(advSum, advGrid);
  const cfgWarnings = el('ul', 'lg-warnings');
  cfgWarnings.setAttribute('data-testid', 'logic-config-warnings');
  cfgWarnings.setAttribute('role', 'status');
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
  const capBtn = el('button', 'lg-capcheck', '엔진 확인');
  capBtn.type = 'button';
  capBtn.setAttribute('data-testid', 'logic-engine-check');
  capBtn.title = 'logic.status (읽기 전용) — 설치된 sigrok-cli와 Loom 지원 소스/디코더를 확인합니다. 장치를 열거나 USB를 스캔하지 않습니다.';
  const capRow = el('div', 'lg-caprow');
  capRow.append(capBtn, capLine);
  const archNote = el('p', 'lg-archive-note', ARCHIVE_NOTE);
  archNote.setAttribute('data-testid', 'logic-archive-note');
  archNote.hidden = true;
  cfgFoot.append(startBtn, cfgProblems, cfgWarnings, archNote);
  cfgBox.append(cfgSum, acqNote, cfgGrid, advBox, deviceNote, cfgFoot, el('span', 'lg-field-label', '같은 캡처를 CLI로'), cli, capRow);
  // module tab (no pinned capture): the source/config controls are visible before anything runs.
  // Opening it this way is UI only — the engine check below is never triggered by it.
  if (!pinned) cfgBox.open = true;

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
  const lineHost = el('div', 'lg-lines');
  lineHost.setAttribute('data-testid', 'logic-lines');
  lineHost.hidden = true;
  const cursorBar = el('div', 'lg-cursors');
  cursorBar.setAttribute('data-testid', 'logic-cursors');
  const cursorText = el('span', 'lg-cursors-text lg-muted', CURSOR_HINT);
  const cursorClear = btn('지우기', '커서 지우기 (Esc)', 'logic-cursor-clear');
  cursorBar.append(cursorText, cursorClear);
  const toolsBar = el('div', 'lg-tools');
  const tid = <E extends HTMLElement>(e: E, id: string): E => {
    e.setAttribute('data-testid', id);
    return e;
  };
  const radixSel = tid(sel('radix', RADIX_OPTS), 'logic-radix');
  radixSel.setAttribute('aria-label', '표시 진법');
  radixSel.value = radix;
  const modeSel = tid(sel('table_mode', TABLE_MODE_OPTS), 'logic-table-mode');
  const searchInp = tid(inp('search', 'search'), 'logic-search');
  searchInp.placeholder = '검색 — 텍스트 login: · 바이트 4C 4F';
  const searchModeSel = tid(sel('search_mode', SEARCH_MODE_OPTS), 'logic-search-mode');
  const caseChk = tid(inp('search_case', 'checkbox'), 'logic-search-case');
  const caseLabel = el('label');
  caseLabel.append(caseChk, '대소문자');
  const searchPrev = btn('◀', '이전 결과 (Shift+Enter)', 'logic-search-prev');
  const searchNext = btn('▶', '다음 결과 (Enter)', 'logic-search-next');
  const searchStatus = el('span', 'lg-search-status');
  searchStatus.setAttribute('data-testid', 'logic-search-status');
  searchStatus.setAttribute('role', 'status');
  const copyScopeSel = tid(sel('copy_scope', COPY_SCOPE_OPTS), 'logic-copy-scope');
  const copyFormatSel = tid(sel('copy_format', COPY_FORMAT_OPTS), 'logic-copy-format');
  const copyBtn = btn('복사', '범위·완전성 메타데이터와 함께 클립보드로 복사', 'logic-copy');
  toolsBar.append(radixSel, modeSel, searchInp, searchModeSel, caseLabel, searchPrev, searchNext, searchStatus, copyScopeSel, copyFormatSel, copyBtn);
  // B3b archive bar (right after the counters)
  const archBar = el('div', 'lg-archive');
  archBar.setAttribute('data-testid', 'logic-archive');
  const chip = tid(el('span', 'lg-archive-chip'), 'logic-archive-state');
  const srcSel = tid(sel('view_source', VIEW_SOURCE_OPTS), 'logic-view-source');
  const prevBtn = btn('◀ 이전 구간', '아카이브의 앞쪽 창 (최대 20,000행)', 'logic-archive-prev');
  const moreBtn = btn('디코드 더 불러오기', '이 창에 아카이브 행을 더 불러옵니다 (창 최대 20,000행)', 'logic-archive-more');
  const nextBtn = btn('다음 구간 ▶', '아카이브의 다음 창 (최대 20,000행)', 'logic-archive-next');
  const seekInp = tid(inp('archive_seek', 'number'), 'logic-archive-seek');
  seekInp.min = '1';
  seekInp.step = '1';
  seekInp.placeholder = '행 번호';
  seekInp.classList.add('lg-archive-seek');
  const seekBtn = btn('이동', '이 행부터 창을 엽니다 (최대 20,000행)', 'logic-archive-seek-go');
  for (const e of [prevBtn, nextBtn, seekInp, seekBtn]) e.classList.add('lg-archive-nav');
  const winLabel = tid(el('span', 'lg-archive-window'), 'logic-archive-window');
  winLabel.setAttribute('role', 'status');
  const expKind = tid(sel('export_kind', EXPORT_KIND_OPTS), 'logic-export-kind');
  const expBtn = btn('파일로 내보내기 (daemon)', '아카이브에서 daemon 쪽 파일로 씁니다 — 브라우저 다운로드가 아닙니다', 'logic-export');
  const expLine = el('span', 'lg-export-result');
  expLine.setAttribute('data-testid', 'logic-export-result');
  expLine.setAttribute('role', 'status');
  archBar.append(chip, srcSel, prevBtn, moreBtn, nextBtn, seekInp, seekBtn, winLabel, expKind, expBtn, expLine);
  const rawBox = el('details', 'lg-rawlines');
  rawBox.setAttribute('data-testid', 'logic-raw-lines');
  const rawSum = el('summary');
  const rawPre = el('pre');
  rawBox.append(rawSum, rawPre);
  root.append(status, counters, archBar, latchEl, notice, cfgBox, viewBar, waveHost, readout, cursorBar, legend, annHead, toolsBar, tableHost, lineHost, rawBox);

  // ---- helpers ----
  const current = (): CaptureData | null => {
    const id = chosen ?? book.newestId;
    return id ? (book.captures.get(id) ?? null) : null;
  };
  const rateOf = (c: CaptureData | null) => c?.samplerate ?? null;
  /** the capture the view shows: the archive copy while the archive view is active for the current capture */
  const shownNow = (): CaptureData | null => {
    const c = current();
    return viewSrc === 'archive' && archCap && c && archFor === c.id ? archCap : c;
  };
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
      cursors = { a: null, b: null };
      searchIdx = -1;
      selectedLine = null;
      followUi(true);
    }
    if (spp) return;
    const r = rateOf(c);
    if (!r && !c.chunks.length && !c.annotations.length) return;
    spp = Math.max(MIN_SPP, (r ? r * DEFAULT_WINDOW_S : 10_000) / width);
  };
  const zoom = (factor: number, anchorX = width) => {
    const c = shownNow();
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
    const c = shownNow();
    if (!c) return;
    const from = Math.max(c.evictedTo, c.range?.start ?? 0);
    const to = Math.max(c.headSample, from + 1);
    spp = (to - from) / width;
    s0 = from;
    clampView();
    setFollow(false);
  };
  const centerSpan = (start: number, end: number) => {
    const c = shownNow();
    syncView(c);
    if (!c || !spp) return;
    const span = Math.max(1, end - start);
    if (span / spp > width * 0.8 || span / spp < 8) spp = Math.max(MIN_SPP, (span * 4) / width);
    s0 = (start + end) / 2 - (width / 2) * spp;
    clampView();
    setFollow(false);
  };
  const centerOn = (a: Annotation) => centerSpan(a.start, a.end);

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
    fLine.value = cfg.uartLine;
    fRx.value = String(cfg.uartRx);
    fTx.value = String(cfg.uartTx);
    fDataBits.value = String(cfg.uartDataBits);
    fParity.value = cfg.uartParity;
    fStop.value = cfg.uartStopBits;
    fUartOrder.value = cfg.uartBitOrder;
    fClk.value = String(cfg.spiClk);
    fMosi.value = cfg.spiMosi === null ? '' : String(cfg.spiMosi);
    fSpiMode.value = String(cfg.spiCpol * 2 + cfg.spiCpha);
    fWordsize.value = String(cfg.spiWordsize);
    fSpiOrder.value = cfg.spiBitOrder;
    fCsPol.value = cfg.spiCsPolarity;
    fAnnotate.value = cfg.spiAnnotate;
    fMiso.value = cfg.spiMiso === null ? '' : String(cfg.spiMiso);
    fCs.value = cfg.spiCs === null ? '' : String(cfg.spiCs);
    fGlitch.value = String(cfg.glitchEvery);
    fTrigOn.checked = cfg.triggerOn;
    fTrigCh.value = String(cfg.triggerCh);
    fTrigEdge.value = cfg.triggerEdge;
    fTrigPre.value = String(cfg.triggerPreMs);
    fTrigPost.value = String(cfg.triggerPostMs);
    const advNonDefault = () =>
      cfg.uartDataBits !== DEFAULT_CONFIG.uartDataBits ||
      cfg.uartParity !== DEFAULT_CONFIG.uartParity ||
      cfg.uartStopBits !== DEFAULT_CONFIG.uartStopBits ||
      cfg.uartBitOrder !== DEFAULT_CONFIG.uartBitOrder ||
      cfg.spiCpol !== DEFAULT_CONFIG.spiCpol ||
      cfg.spiCpha !== DEFAULT_CONFIG.spiCpha ||
      cfg.spiWordsize !== DEFAULT_CONFIG.spiWordsize ||
      cfg.spiBitOrder !== DEFAULT_CONFIG.spiBitOrder ||
      cfg.spiCsPolarity !== DEFAULT_CONFIG.spiCsPolarity ||
      cfg.spiAnnotate !== DEFAULT_CONFIG.spiAnnotate;
    if (advNonDefault()) advBox.open = true;
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
      uartLine: fLine.value as UartLine,
      uartRx: Number(fRx.value),
      uartTx: Number(fTx.value),
      uartDataBits: Number(fDataBits.value),
      uartParity: fParity.value as UartParity,
      uartStopBits: fStop.value as UartStopBits,
      uartBitOrder: fUartOrder.value as BitOrder,
      glitchEvery: Number(fGlitch.value),
      triggerOn: fTrigOn.checked,
      triggerCh: Number(fTrigCh.value),
      triggerEdge: fTrigEdge.value as TriggerEdgeName,
      triggerPreMs: Number(fTrigPre.value),
      triggerPostMs: Number(fTrigPost.value),
      spiClk: Number(fClk.value),
      spiMosi: pin(fMosi),
      spiCpol: (Number(fSpiMode.value) >> 1) as 0 | 1,
      spiCpha: (Number(fSpiMode.value) & 1) as 0 | 1,
      spiWordsize: Number(fWordsize.value),
      spiBitOrder: fSpiOrder.value as BitOrder,
      spiCsPolarity: fCsPol.value as CsPolarity,
      spiAnnotate: fAnnotate.value as SpiAnnotate,
      spiMiso: pin(fMiso),
      spiCs: pin(fCs),
    };
    saveJson(CFG_KEY, cfg);
    ctx.scheduler.invalidate();
  };
  writeForm();
  cfgBox.addEventListener('change', readForm);
  cfgBox.addEventListener('input', readForm);

  let capPending = false;
  let cfgTouched = false;
  let cfgAutoCollapsed = false;
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
      })
      .finally(() => {
        capPending = false;
        capBtn.disabled = false;
      });
  };
  // only a user gesture asks the engine: clicking the summary to open it, or [엔진 확인].
  // (A `toggle` listener would also fire for the default-open above → an RPC on tab open.)
  const checkEngine = () => {
    if (capPending) return;
    capPending = true;
    capBtn.disabled = true;
    refreshCapability();
  };
  cfgSum.addEventListener('click', () => {
    cfgTouched = true;
    if (!cfgBox.open && !capability) checkEngine(); // about to open
  });
  capBtn.addEventListener('click', checkEngine);

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
    hoverY = e.clientY - waveHost.getBoundingClientRect().top;
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
  waveHost.addEventListener('pointerup', (e) => {
    if (drag && Math.abs(e.clientX - drag.x) <= 2 && spp && localX(e) >= 0) {
      const smp = Math.floor(s0 + localX(e) * spp);
      cursors = e.shiftKey ? { ...cursors, b: smp } : { ...cursors, a: smp };
      ctx.scheduler.invalidate();
    }
    drag = null;
  });
  waveHost.addEventListener('pointercancel', endDrag);
  waveHost.addEventListener('pointerleave', () => {
    hoverX = null;
    hoverY = null;
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
      s0 = shownNow()?.evictedTo ?? 0;
      setFollow(false);
    } else if ((k === '1' || k === '2') && hoverX !== null && hoverX >= 0 && spp) {
      const smp = Math.floor(s0 + hoverX * spp);
      cursors = k === '1' ? { ...cursors, a: smp } : { ...cursors, b: smp };
      ctx.scheduler.invalidate();
    } else if (k === 'Escape') {
      cursors = { a: null, b: null };
      ctx.scheduler.invalidate();
    } else return;
    e.preventDefault();
  });
  const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(() => ctx.scheduler.invalidate()) : null;
  ro?.observe(waveHost);

  // ---- annotation table ----
  let tableItems: Annotation[] = [];
  const makeRowTable = (withClass: boolean) => {
    const cols: Column<Annotation>[] = [
      { key: 'samples', header: 'sample 시작–끝', width: '13em', cell: (a) => ({ text: `${fmtN(a.start)}–${fmtN(a.end)}`, cls: 'lg-num' }) },
      { key: 't', header: '시각 (sample clock)', width: '9em', cell: (a) => ({ text: a.tStartNs !== null ? fmtSeconds(a.tStartNs / 1e9, 4) : '?', cls: 'lg-num' }) },
      { key: 'dur', header: '길이', width: '6.5em', cell: (a) => ({ text: a.tStartNs !== null && a.tEndNs !== null ? fmtSeconds((a.tEndNs - a.tStartNs) / 1e9, 2) : `${fmtN(a.end - a.start)} smp`, cls: 'lg-num' }) },
      { key: 'dec', header: '디코더', width: '7em', cell: (a) => trackLabel(a.decoder, current()?.info?.decoderTracks) },
    ];
    if (withClass) {
      cols.push({ key: 'cls', header: 'class', width: '7em', cell: (a) => (a.cls === null ? { text: '—', cls: 'lg-muted', title: 'sigrok-cli가 class를 제공하지 않음 (임의 생성하지 않음)' } : a.cls) });
    }
    cols.push({ key: 'text', header: '내용', width: '1fr', cell: (a) => ({ text: formatWord(a.text, radix), cls: a.cls === '_raw' ? 'lg-rawline' : 'lg-anntext', title: a.text }) });
    return new VirtualTable<Annotation>(tableHost, {
      columns: cols,
      keyOf: (a) => a.key,
      rowAttrs: (a) => ({ 'data-start': String(a.start), 'data-decoder': a.decoder }),
      rowClass: (a) => (a.role === 'error' ? 'lg-annerr' : ''),
      onNeedRender: () => ctx.scheduler.invalidate(),
      onSelect: (key) => {
        selected = key ? (tableItems.find((a) => a.key === key) ?? null) : null;
        if (selected) {
          centerOn(selected);
          cursors = { a: selected.start, b: selected.end };
        }
        ctx.scheduler.invalidate();
      },
      emptyText: '디코드 결과 없음',
      ariaLabel: '디코드 결과',
    });
  };
  let table = makeRowTable(false);
  const lineTable = new VirtualTable<TextLine>(lineHost, {
    columns: [
      { key: 's', header: 'sample 시작', width: '10em', cell: (l) => ({ text: fmtN(l.startSample), cls: 'lg-num' }) },
      {
        key: 't',
        header: '시각 (sample clock)',
        width: '9em',
        cell: (l) => {
          const r = rateOf(shownNow());
          return { text: r ? fmtSeconds(l.startSample / r, 4) : '?', cls: 'lg-num' };
        },
      },
      { key: 'dec', header: '디코더', width: '7em', cell: (l) => l.decoder },
      { key: 'text', header: '줄 (ASCII)', width: '1fr', cell: (l) => ({ text: l.text, cls: 'lg-anntext' }) },
      { key: 'end', header: '끝', width: '5em', cell: (l) => ({ text: LINE_END_LABEL[l.end], cls: 'lg-lineend' }) },
    ],
    keyOf: (l) => l.key,
    rowAttrs: (l) => ({ 'data-start': String(l.startSample) }),
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: (key) => {
      const l = key ? (lineItems.find((x) => x.key === key) ?? null) : null;
      selectedLine = l;
      if (l) {
        centerSpan(l.startSample, l.endSample);
        cursors = { a: l.startSample, b: l.endSample };
      }
      ctx.scheduler.invalidate();
    },
    emptyText: '디코드 결과 없음',
    ariaLabel: '디코드 줄',
  });

  // ---- table tools: search / copy (B3a) ----
  const jump = (dir: 1 | -1) => {
    const hits = searchRes?.hits ?? [];
    const n = hits.length;
    if (!n) return;
    searchIdx = searchIdx < 0 ? (dir > 0 ? 0 : n - 1) : (searchIdx + dir + n) % n;
    const hit = hits[searchIdx]!;
    if (tableMode === 'rows') {
      table.select(hit.firstKey);
      table.reveal(hit.firstKey);
    } else {
      const l = lineItems.find((x) => x.rowKeys.includes(hit.firstKey));
      if (l) {
        lineTable.select(l.key);
        lineTable.reveal(l.key);
      }
    }
    centerSpan(hit.startSample, hit.endSample);
    cursors = { a: hit.startSample, b: hit.endSample };
    setFollow(false);
  };
  const copy = () => {
    const c = shownNow();
    const live = current();
    if (!c || !live) {
      ctx.ui.toast('복사할 캡처가 없습니다');
      return;
    }
    const scope = copyScopeSel.value as CopyScope;
    const format = copyFormatSel.value;
    const rate = rateOf(c);
    const lo = cursors.a !== null && cursors.b !== null ? Math.min(cursors.a, cursors.b) : 0;
    const hi = cursors.a !== null && cursors.b !== null ? Math.max(cursors.a, cursors.b) : 0;
    const needCursors = () => {
      if (cursors.a === null || cursors.b === null) {
        ctx.ui.toast('커서 A와 B를 먼저 놓으세요');
        return false;
      }
      return true;
    };
    let rowItems: Annotation[] = [];
    let lineSel: TextLine[] = [];
    let first: number | undefined;
    let last: number | undefined;
    let count = 0;
    if (tableMode === 'rows') {
      if (scope === 'all') rowItems = c.annotations;
      else if (scope === 'cursors') {
        if (!needCursors()) return;
        rowItems = c.annotations.filter((a) => a.start >= lo && a.start < hi);
      } else {
        if (!selected) {
          ctx.ui.toast('선택된 행이 없습니다');
          return;
        }
        rowItems = [selected];
      }
      count = rowItems.length;
      first = rowItems[0]?.start;
      last = rowItems[rowItems.length - 1]?.end;
    } else {
      if (scope === 'all') lineSel = lineItems;
      else if (scope === 'cursors') {
        if (!needCursors()) return;
        lineSel = lineItems.filter((l) => l.startSample >= lo && l.startSample < hi);
      } else {
        if (!selectedLine) {
          ctx.ui.toast('선택된 줄이 없습니다');
          return;
        }
        lineSel = [selectedLine];
      }
      count = lineSel.length;
      first = lineSel[0]?.startSample;
      last = lineSel[lineSel.length - 1]?.endSample;
    }
    const archiveMode = c === archCap;
    const meta = {
      captureId: live.id,
      samplerate: rate,
      source: archiveMode ? ('archive' as const) : ('panel' as const),
      scope,
      fromSample: scope === 'cursors' ? lo : (first ?? 0),
      toSample: scope === 'cursors' ? hi : (last ?? 0),
      coverage: archiveMode ? archiveWindowCoverage(live.info?.archive, archRows, c.evictedAnnotations) : decodeCoverage(live).state,
      ...(archiveMode && archRows.first !== null && archRows.last !== null
        ? { archiveRows: { first: archRows.first, last: archRows.last, total: live.info?.archive?.annotationRows ?? null } }
        : {}),
    };
    let text: string;
    if (tableMode === 'rows') text = format === 'csv' ? rowsToCsv(rowItems, meta) : rowsToText(rowItems, radix, meta);
    else if (format === 'csv') {
      const keys = new Set(lineSel.flatMap((l) => l.rowKeys));
      text = rowsToCsv(c.annotations.filter((a) => keys.has(a.key)), meta);
    } else text = linesToText(lineSel, meta);
    const unit = tableMode === 'rows' ? '행' : '줄';
    const cb = (navigator as { clipboard?: Clipboard }).clipboard;
    if (!cb) {
      ctx.ui.toast('복사 실패: 클립보드 권한 없음');
      return;
    }
    let p: Promise<void>;
    try {
      p = cb.writeText(text);
    } catch {
      ctx.ui.toast('복사 실패: 클립보드 권한 없음');
      return;
    }
    p.then(
      () => ctx.ui.toast(`복사됨: ${fmtN(count)}${unit}`),
      () => ctx.ui.toast('복사 실패: 클립보드 권한 없음'),
    );
  };
  radixSel.addEventListener('change', () => {
    radix = radixSel.value as Radix;
    saveJson(RADIX_KEY, radix);
    ctx.scheduler.invalidate();
  });
  modeSel.addEventListener('change', () => {
    tableMode = modeSel.value === 'lines' ? 'lines' : 'rows';
    ctx.scheduler.invalidate();
  });
  searchInp.addEventListener('input', () => {
    searchPattern = searchInp.value;
    ctx.scheduler.invalidate();
  });
  searchInp.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    jump(e.shiftKey ? -1 : 1);
  });
  searchModeSel.addEventListener('change', () => {
    searchMode = searchModeSel.value === 'bytes' ? 'bytes' : 'text';
    ctx.scheduler.invalidate();
  });
  caseChk.addEventListener('change', () => {
    searchCase = caseChk.checked;
    ctx.scheduler.invalidate();
  });
  searchPrev.addEventListener('click', () => jump(-1));
  searchNext.addEventListener('click', () => jump(1));
  copyBtn.addEventListener('click', copy);
  cursorClear.addEventListener('click', () => {
    cursors = { a: null, b: null };
    ctx.scheduler.invalidate();
  });

  // ---- archive view (B3b) ----
  const loadRows = async () => {
    if (archRowsBusy || archRows.eof || archiveWindowFull(archRows) || !archCap) return;
    const gen = archGen;
    const id = archFor;
    const cap = archCap;
    if (id === null) return;
    archRowsBusy = true;
    try {
      for (let i = 0; i < ARCHIVE_ROW_PAGES; i++) {
        const limit = archiveWindowLimit(archRows);
        if (limit === 0) break;
        const after = archiveWindowAfterRow(archRows);
        const r = await ctx.query<unknown>('logic.archive.read', {
          capture_id: id,
          kind: 'annotations',
          limit,
          ...(after !== null ? { after_row: after } : {}),
        });
        if (gen !== archGen) return;
        const pg = readArchiveRows(r, id, cap.samplerate ?? current()?.samplerate ?? null);
        cap.addAnnotations(pg.rows, 0);
        cap.addRawLines(pg.rawLines, pg.rejected);
        archRows = archiveWindowAdd(archRows, pg);
        ctx.scheduler.invalidate();
        if (archRows.eof || archiveWindowFull(archRows) || pg.count === 0) break;
      }
    } catch (e) {
      if (gen === archGen) archErr = `아카이브 읽기 실패: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      if (gen === archGen) archRowsBusy = false;
      ctx.scheduler.invalidate();
    }
  };
  const loadEdges = async (from: number, to: number) => {
    if (archEdgesBusy || !archCap) return;
    const gen = archGen;
    const id = archFor;
    const cap = archCap;
    if (id === null) return;
    archEdgesBusy = true;
    archWin = { from, to };
    cap.clearChunks();
    let cur = from;
    let more = false;
    try {
      for (let i = 0; i < ARCHIVE_EDGE_PAGES; i++) {
        const r = await ctx.query<unknown>('logic.archive.read', { capture_id: id, kind: 'edges', from_sample: cur, to_sample: to, limit: 2048 });
        if (gen !== archGen) return;
        const pg = readArchiveEdges(r, id);
        for (const ch of pg.chunks) cap.addChunk(ch);
        if (pg.nextFrom === null) {
          more = false;
          break;
        }
        cur = pg.nextFrom;
        more = cur < to;
        if (!more) break;
      }
      if (more) cap.markNotLoaded(cur, to, NOT_LOADED_DETAIL);
    } catch (e) {
      if (gen === archGen) archErr = `아카이브 읽기 실패: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      if (gen === archGen) archEdgesBusy = false;
      ctx.scheduler.invalidate();
    }
  };
  const openWindow = (startAfter: number | null) => {
    if (archFor === null || !archCap) return;
    archGen++;
    const keep = archCap.info;
    archCap = new CaptureData(archFor);
    archCap.info = keep;
    archRows = archiveWindowStart(startAfter);
    archRowsBusy = false;
    archEdgesBusy = false;
    archWin = null;
    archErr = null;
    selected = null;
    searchIdx = -1;
    searchKey = '';
    ctx.scheduler.invalidate();
    void loadRows();
  };
  const resetArchiveView = (c: CaptureData) => {
    archGen++;
    archCap = new CaptureData(c.id);
    archFor = c.id;
    archRows = archiveWindowStart(null);
    archWin = null;
    archErr = null;
    // a request still in flight belongs to the previous generation: its result is ignored, so it must not block this one
    archRowsBusy = false;
    archEdgesBusy = false;
    selected = null;
    cursors = { a: null, b: null };
    setFollow(false);
    void loadRows();
  };
  srcSel.addEventListener('change', () => {
    viewSrc = srcSel.value === 'archive' ? 'archive' : 'live';
    const c = current();
    if (viewSrc === 'archive' && c) resetArchiveView(c);
    ctx.scheduler.invalidate();
  });
  moreBtn.addEventListener('click', () => {
    void loadRows();
  });
  prevBtn.addEventListener('click', () => {
    const s = archivePrevStart(archRows);
    if (s) openWindow(s.startAfter);
  });
  nextBtn.addEventListener('click', () => {
    const s = archiveNextStart(archRows);
    if (s) openWindow(s.startAfter);
  });
  const go = () => {
    const t = current()?.info?.archive?.annotationRows ?? null;
    const s = archiveSeekStart(Number(seekInp.value), t);
    if (!s) {
      ctx.ui.toast(`행 번호는 1–${t === null ? '?' : fmtN(t)} 범위의 정수입니다`);
      return;
    }
    openWindow(s.startAfter);
  };
  seekBtn.addEventListener('click', go);
  seekInp.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    go();
  });
  expBtn.addEventListener('click', () => {
    const c = current();
    if (!c) return;
    const [kind, format] = expKind.value.split(':');
    ctx
      .act('export', { capture_id: c.id, kind, format })
      .then((h) => {
        const r = obj(h.result);
        exportLine = `daemon에 저장됨: ${typeof r.path === 'string' ? r.path : '?'} · ${fmtN(num(r.rows) ?? 0)}${kind === 'edges' ? '개 전이' : '행'} · ${r.complete === true ? '완전' : '일부'}${r.limit_hit === true ? ' (상한 도달)' : ''}`;
      })
      .catch((e: unknown) => {
        exportLine = `내보내기 실패: ${e instanceof Error ? e.message : String(e)}`;
      })
      .finally(() => ctx.scheduler.invalidate());
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
    if (!cfgAutoCollapsed && c) {
      cfgAutoCollapsed = true;
      if (!cfgTouched && !pinned) cfgBox.open = false;
    }
    // B3b: which capture the view/tables/search/copy act on (live facts below keep using `c`)
    const readable = !!c && ARCHIVE_READABLE.includes(c.info?.archive?.state ?? 'unknown');
    if (!readable && viewSrc === 'archive') {
      viewSrc = 'live';
      srcSel.value = 'live';
    }
    if (viewSrc === 'archive' && c && archFor !== c.id) resetArchiveView(c);
    const shown: CaptureData | null = viewSrc === 'archive' && archCap && c && archFor === c.id ? archCap : c;
    if (shown && shown === archCap && c) archCap.info = c.info;
    const info = c?.info ?? null;
    const st = info?.state ?? 'unknown';
    const dCov = c ? decodeCoverage(c) : null;
    const rCov = c ? rawCoverage(c) : null;
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
    const trig = info?.trigger ?? null;
    trigChip.hidden = !trig;
    trigChip.textContent = triggerText(trig);
    trigChip.dataset.state = trig?.state ?? '';
    stopBtn.disabled = !c || !(st === 'running' || st === 'starting' || st === 'stopping' || st === 'unknown') || stopRequested === c.id;

    // archive status / view source / export (B3b)
    const archLabel = archiveLabel(c?.info?.archive);
    chip.textContent = archLabel.text;
    chip.dataset.tone = archLabel.tone;
    chip.title = archLabel.title;
    archBar.hidden = !c;
    srcSel.disabled = !readable;
    expBtn.disabled = !readable || ctx.writeLocked;
    const navHidden = viewSrc !== 'archive';
    prevBtn.hidden = nextBtn.hidden = seekInp.hidden = seekBtn.hidden = winLabel.hidden = navHidden;
    moreBtn.hidden = navHidden || archRows.eof || archiveWindowFull(archRows);
    moreBtn.disabled = archRowsBusy;
    prevBtn.disabled = archRowsBusy || archivePrevStart(archRows) === null;
    nextBtn.disabled = archRowsBusy || archiveNextStart(archRows) === null;
    seekBtn.disabled = archRowsBusy;
    winLabel.textContent = archiveWindowText(archRows, c?.info?.archive?.annotationRows ?? null, archRowsBusy);
    expLine.textContent = exportLine;

    // counters + loss
    if (c) {
      const k = info?.counters;
      const head = c.headSample;
      const lagMs = c.lastHostMonoNs !== null && st === 'running' ? Number(ctx.clock.monoEstNs() - c.lastHostMonoNs) / 1e6 : null;
      const dropEdges = Math.max(k?.droppedEdges ?? 0, c.droppedEdges);
      const dropAnn = Math.max(k?.droppedAnnotations ?? 0, c.droppedAnnotations);
      const ek = `${shown?.id ?? ''}|${viewSrc}|${shown?.version ?? 0}`;
      if (ek !== heldErrKey) {
        heldErrKey = ek;
        heldErr = shown ? shown.annotations.filter((a) => a.role === 'error').length : 0;
      }
      const errs = Math.max(k?.errorRows ?? 0, heldErr);
      const parts: [string, string, boolean][] = [
        ['samples', `샘플 ${fmtN(Math.max(k?.samples ?? 0, head))}${rate ? ` (${fmtSampleTime(Math.max(k?.samples ?? 0, head), rate, 2)})` : ''}`, false],
        ['edges', `전이 ${fmtN(Math.max(k?.edges ?? 0, c.edgeCount))}`, false],
        ['annotations', `디코드 ${fmtN(Math.max(k?.annotations ?? 0, c.annotations.length))}`, false],
        ['dropped_edges', `전이 생략 ${fmtN(dropEdges)}`, dropEdges > 0],
        ['dropped_annotations', `디코드 손실 ${fmtN(dropAnn)}`, dropAnn > 0],
        ['error_rows', `디코드 오류 ${fmtN(errs)}`, errs > 0],
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
      notes.push(`${st === 'failed' ? '실패' : '정지'}: ${why}${exit}${info.error ? ` · ${info.error}` : ''}${coverageNote(dCov!, rCov!)}`);
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
    lBaud.hidden = lLine.hidden = cfg.decoder !== 'uart';
    lRx.hidden = cfg.decoder !== 'uart' || cfg.uartLine === 'tx';
    lTx.hidden = cfg.decoder !== 'uart' || cfg.uartLine === 'rx';
    lGlitch.hidden = !(cfg.source === 'synthetic' && cfg.decoder === 'uart');
    for (const l of [lTrigCh, lTrigEdge, lTrigPre, lTrigPost]) l.hidden = !cfg.triggerOn;
    lClk.hidden = lMosi.hidden = lMiso.hidden = lCs.hidden = cfg.decoder !== 'spi';
    for (const l of uartAdv) l.hidden = cfg.decoder !== 'uart';
    for (const l of spiAdv) l.hidden = cfg.decoder !== 'spi';
    advBox.hidden = cfg.decoder === 'none';
    const estimate = estimatePublishLoad(cfg, shownChannels());
    cfgWarnings.replaceChildren(...estimate.warnings.map((t) => el('li', undefined, t)));
    archNote.hidden = estimate.warnings.length === 0;
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
        : '[엔진 확인]을 누르면 logic.status로 sigrok-cli·지원 소스를 확인합니다 (장치 열기/스캔 없음)';

    // view
    const w = Math.max(100, (waveHost.clientWidth || 900) - GUTTER);
    width = w;
    const ls = lanes(shown);
    const decs = decodersOf(shown);
    const labels: Record<string, string> = Object.fromEntries(decs.map((d) => [d, trackLabel(d, info?.decoderTracks)]));
    const trigSample = info?.trigger?.state === 'fired' ? info.trigger.sample : null;
    syncView(shown);
    if (shown === archCap && follow) followUi(false); // the archive view never follows live data
    if (shown && follow && spp) s0 = Math.max(shown.evictedTo, followHeadSample(shown) - width * spp);
    clampView();
    if (c && follow && spp && (st === 'stopped' || st === 'failed') && followStopped !== c.id) {
      followStopped = c.id;
      followUi(false);
    }
    const v = view();
    if (shown && shown === archCap && spp && !archEdgesBusy) {
      const need = [Math.max(0, Math.floor(s0)), Math.ceil(s0 + width * spp)] as const;
      const span = need[1] - need[0];
      if (!archWin || need[0] < archWin.from || need[1] > archWin.to) void loadEdges(Math.max(0, need[0] - span), need[1] + span);
    }
    windowInfo.textContent = c && spp ? `${fmtSampleTime(v.s0, rate)} – ${fmtSampleTime(v.s0 + v.spp * v.width, rate)} · ${v.spp >= 1 ? `${+v.spp.toFixed(1)} smp/px` : `${+(1 / v.spp).toFixed(1)} px/smp`}` : '';
    const heightPx = AXIS_H + ls.length * LANE_H + Math.max(1, decs.length) * ANN_H + 4;
    empty.hidden = !!shown?.chunks.length || !!shown?.annotations.length;
    empty.textContent = info?.trigger?.state === 'armed' ? TRIGGER_WAIT_TEXT : !c ? '캡처를 시작하면 채널별 원시 파형과 디코드 결과가 여기에 그려집니다' : st === 'failed' ? '원시 데이터 없음 — 캡처 실패' : '원시 데이터 대기 중…';
    waveHost.setAttribute(
      'aria-label',
      c ? `파형: ${ls.map((l) => l.name).join(', ')} · ${windowInfo.textContent} · 디코드 트랙 ${decs.map((d) => labels[d]).join(', ') || '없음'}` : '파형 없음',
    );
    const pk = `${shown?.id ?? ''}|${shown === archCap ? 'A' : 'L'}|${shown?.version ?? 0}|${v.s0}|${v.spp}|${v.width}|${laneMask}|${hoverX}|${selected?.key ?? ''}|${decs.join(',')}|${cursors.a}|${cursors.b}|${trigSample}|${decs.map((d) => labels[d]).join(',')}`;
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
        paint(g, { cap: shown, lanes: ls, decoders: decs.length ? decs : [], view: v, rate, palette: readPalette(root), hoverX, selected, cursors, decoderLabels: labels, triggerSample: trigSample }, heightPx);
      }
    }
    // hover readout
    if (shown && hoverX !== null && hoverX >= 0 && spp) {
      const s = Math.floor(v.s0 + hoverX * v.spp);
      const st2 = shown.stateAt(s);
      const levels = ls.map((l) => `${l.name}=${st2 === null ? '?' : (st2 >> l.bit) & 1}`).join(' ');
      const ai = shown.annotationAt(s);
      const hit = shown.annotations.slice(ai, ai + 8).filter((a) => a.start <= s && s < Math.max(a.end, a.start + 1));
      let pulse = '';
      if (hoverY !== null) {
        const li = Math.floor((hoverY - AXIS_H) / LANE_H);
        const lane = li >= 0 && li < ls.length ? ls[li] : undefined;
        const pm = lane ? pulseAt(shown, lane.bit, s, rate) : null;
        if (lane && pm) pulse = ` · ${lane.name} ${pm.text}`;
      }
      readout.textContent = `sample ${fmtN(s)} · ${fmtSampleTime(s, rate, 4)} · ${levels}${st2 === null ? ' (데이터 없음)' : ''}${hit.length ? ` · 디코드 ${hit.map((a) => `${a.decoder}: ${a.text}`).join(' | ')}` : ''}${pulse}`;
    } else {
      readout.textContent = c ? '파형 위로 마우스 — sample·시각·레벨 · 휠 확대/축소 · 끌기/Shift+휠 이동 · ←/→ +/- 0 F' : '';
    }

    // cursor bar
    const cm = measureCursors(cursors, rate);
    const oneSet = cursors.a !== null ? cursors.a : cursors.b;
    const cursorHint = !cm && oneSet === null;
    cursorText.textContent = cm ? cm.text : oneSet !== null ? `${cursors.a !== null ? 'A' : 'B'} ${fmtSampleTime(oneSet, rate, 4)}` : CURSOR_HINT;
    cursorText.classList.toggle('lg-muted', cursorHint);

    // annotation table
    const hc = !!shown && shown.annotations.some((a) => a.cls !== null);
    if (hc !== hasClass || viewSrc !== tableSrc) {
      // a fresh row pool: the previous source's pooled rows must not linger (hidden but still in the DOM)
      table.dispose();
      tableHost.replaceChildren();
      table = makeRowTable(hc);
      hasClass = hc;
      tableSrc = viewSrc;
      tableKey = '';
    }
    const tk = `${shown?.id ?? ''}|${viewSrc}|${shown?.version ?? 0}|${radix}|${tableMode}`;
    if (tk !== tableKey) {
      tableKey = tk;
      if (tableMode === 'rows') {
        tableItems = shown ? shown.annotations.slice() : [];
        table.setItems(tableItems);
        if (follow) table.followTail();
      } else {
        lineItems = shown ? splitLines(shown.annotations) : [];
        lineTable.setItems(lineItems);
        if (follow) lineTable.followTail();
      }
    }
    tableHost.hidden = tableMode !== 'rows';
    lineHost.hidden = !tableHost.hidden;
    if (tableMode === 'rows') table.render();
    else lineTable.render();

    // search
    const qk = `${shown?.id ?? ''}|${viewSrc}|${archGen}|${searchMode}|${searchCase}|${searchPattern}`;
    const sk = `${qk}|${shown?.version ?? 0}`;
    if (sk !== searchKey) {
      const prev = qk === searchQKey && searchRes && searchIdx >= 0 ? (searchRes.hits[searchIdx] ?? null) : null;
      searchKey = sk;
      searchQKey = qk;
      searchRes = shown && searchPattern ? searchRows(shown.annotations, { mode: searchMode, pattern: searchPattern, caseSensitive: searchCase }) : null;
      searchIdx = relocateHit(prev, searchRes?.hits ?? []);
    }
    searchStatus.className = searchRes?.error ? 'lg-search-status lg-bad' : 'lg-search-status';
    searchStatus.textContent = searchRes?.error ? searchRes.error : searchRes ? `${searchIdx + 1}/${searchRes.hits.length}${searchRes.truncated ? '+' : ''}` : '';
    if (
      shown &&
      shown === archCap &&
      searchRes &&
      !searchRes.error &&
      archRows.first !== null &&
      archRows.last !== null &&
      archiveWindowCoverage(c?.info?.archive, archRows, archCap.evictedAnnotations) !== 'complete'
    ) {
      searchStatus.textContent += ` · 창 행 ${fmtN(archRows.first + 1)}–${fmtN(archRows.last + 1)} 안에서`;
    }

    const drops = c ? Math.max(c.info?.counters?.droppedAnnotations ?? 0, c.droppedAnnotations) : 0;
    if (shown && shown === archCap) {
      const total = c?.info?.archive?.annotationRows ?? null;
      annInfo.textContent = `${fmtN(shown.annotations.length)}행 · 아카이브에서 불러옴 · ${archiveWindowText(archRows, total, archRowsBusy)}${archErr ? ` · ${archErr}` : ''} · 디코더 ${decs.join(', ') || '—'} · 해석 결과(원시 아님)`;
    } else {
      annInfo.textContent = c
        ? `${fmtN(c.annotations.length)}행${annCoverageText(dCov!)}${c.evictedAnnotations ? ` · 이전 ${fmtN(c.evictedAnnotations)}행 보관 밖` : ''}${drops ? ` · 손실 ${fmtN(drops)}` : ''} · 디코더 ${decs.join(', ') || (info?.decoders.join(', ') ?? '') || '—'} · 해석 결과(원시 아님)`
        : '';
    }
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
      lineTable.dispose();
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
    queries: ['logic.status', 'logic.archive.list', 'logic.archive.read'],
    actions: [
      // request-class R1 (CONTRACT §9: not an SDK job/renewal)
      { id: 'start', rpc: 'logic.start', class: 'request', right: 'operate', confirm: 'none', label: '로직 캡처 시작' },
      // STOP of an observation capture is safety direction: any right, never confirmed (eth.capture.stop pattern)
      { id: 'stop', rpc: 'logic.stop', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '로직 캡처 정지' },
      // B3b: evidence copy written inside the daemon-side archive (R0, watchers allowed; never a browser download)
      { id: 'export', rpc: 'logic.export', class: 'request', right: 'watch', confirm: 'none', label: '로직 아카이브 내보내기' },
    ],
    memory: { maxBytes: 32 * 1024 * 1024 },
    render: { maxHz: 30, budgetMs: 6, lagTargetMs: 250 },
  },
  createLogicAnalyzer,
);
