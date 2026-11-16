// can.trace (M1) — can-ui §5.3, plan §5.1.3: Chrono · raw columns · send line ·
// omission rows · agent-read range bar. Event/RPC shapes = CAN schema v1
// (docs/impl/requests/can-ui.md): send = `can.send` (structured params, sendLine.ts),
// one row per send merged by request_id (traceModel.ts), len/DLC/RTR/frame_index columns,
// risk badge from `can.send{…, dry_run:true}` → {risk, reason, bytes, policy} (debounced).
//
// Data path: ctx.store('can') (can domain, this channel, snapshot+live) → on every append
// the new events go straight into the column ring (traceModel.ts) — collection never
// depends on drawing, so Pause and hidden tabs lose nothing the mirror still holds.
// Drawing happens only in the RenderScheduler frame callback (rAF; the scheduler throttles
// to 20 Hz when the draw exceeds its budget).
//
// LIVE-DEGRADED: the shell scheduler decides (ctx.scheduler.degraded — draw > 8 ms in 3
// consecutive frames, or draw + reported ingest work > 300 ms per second; recovery after 30
// cheap frames with the load back under 75 ms/s). Ingest time is reported with
// ctx.scheduler.reportWork. While degraded, plain frames
// beyond 2,000 rows/s are kept in the ring but not shown, counted on one omission row
// per run ("⋯ N 프레임 화면 생략 … — 기록 정상|기록 확인 안 됨").
//
// Bounds (C2): ring per traceModel.ts (default 100k rows, `rows` prop up to 2M);
// mirror 20,000 events; my request ids MY_REQ_MAX (256, FIFO); send history
// HISTORY_MAX (50, FIFO); recent gap keys GAP_KEYS_MAX (64, FIFO); latency histogram
// LAT_BUCKETS fixed buckets.

import {
  CHANNEL_PROP,
  createLatchLine,
  GateError,
  registerPanel,
  type GapMark,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type PanelManifest,
  type RowView,
  type StoreMirror,
} from '../../sdk';
import './can.css';
import { createCanConnect, type CanConnectSection } from './canConnect';
import { formatFrame, parseSendLine } from './sendLine';
import {
  AGENT_RANGES,
  bytesForRows,
  CanTraceModel,
  DISPLAY_BUDGET_ROWS_PER_S,
  dlcForLen,
  F_HIDDEN,
  K_FRAME,
  K_GAP,
  ROWS_DEFAULT,
  ROWS_MAX,
  type Origin,
  type TraceFilter,
  type WState,
} from './traceModel';
import { TraceView } from './traceView';
import { RxSummary, RX_SUMMARY_MAX_IDS } from './rxSummary';
import { RxTxView } from './rxtxView';
import { TxList } from './txList';
import type { CanSendParams } from './sendLine';
// B1 (CAN-1 time modes, CAN-3 transmit-list persistence) — FUNCTIONS.md §B1-PANEL
import { defaultKV } from './kv';
import { readTimeMode, refLabel, TIME_LABEL, TIME_MODES, writeTimeMode, type TimeMode, type TimeRef } from './timeFmt';
import { loadTxList, saveTxList } from './txStore';
// B2 (CAN-8 id ranges, CAN-7 row detail, CAN-6 channel close) — FUNCTIONS.md §B2-PANEL
import { parseIdFilter, type IdFilter, type IdMatch } from './idFilter';
import { rowDetailFields, rowDetailText, rowFacts, type RowFacts } from './rowDetail';
import { channelStateOf, closeDoneText, closeImpact, closeImpactLines, type CloseImpact } from './closeChannel';
// B4 (CAN-4): export to a daemon-side BLF/ASC file — FUNCTIONS.md §B4-PANEL
import { exportRequest, exportResultLines, ringSeqs, type ExportFormatUi, type ExportScope } from './exportReq';
// B3 (CAN-2): daemon periodic jobs — FUNCTIONS.md §B3-PANEL
import { JobTable, startParams } from './periodicJobs';
import { PeriodicView } from './periodicView';
// B5 (CAN-5): DBC chip / load / names / on-demand decode — FUNCTIONS.md §B5-PANEL
import { DB_ENCODINGS, DB_PAGE_SIZE, DbNames, dbChip, dbLoadParams, decodedLines, parseDbInfo, type DbEncoding, type DbInfo } from './dbView';

/** B2: the id-filter parser moved to idFilter.ts (ranges); re-exported for existing importers */
export { parseIdFilter };

export const MY_REQ_MAX = 256;
export const HISTORY_MAX = 50;
const GAP_KEYS_MAX = 64;
export const MIRROR_ROWS = 20_000;
/** Receive/Transmit view redraw cap (Hz) — keeps a 1,024-id summary inside the frame budget */
export const RXTX_RENDER_HZ = 10;
const RXTX_SLOW_MS = 2;
/** send-line risk preview: dry-run after the input is quiet this long */
export const DRY_RUN_DEBOUNCE_MS = 250;
/** latency histogram: 0.5 ms buckets up to 2 s + overflow */
const LAT_BUCKET_MS = 0.5;
const LAT_BUCKETS = 4_001;
/** at most this many rows are timed per frame (the rest are skipped, not guessed) */
const LAT_MAX_PER_FRAME = 4_096;

export interface CanTraceStats {
  rows: number;
  capacity: number;
  evicted: number;
  displayed: number;
  framesIn: number;
  omitted: number;
  gapRows: { kind: string; first: number | null; last: number | null; count: number | 'unknown'; cause: string }[];
  degraded: boolean;
  paused: boolean;
  lat: { n: number; p50: number; p95: number; p99: number; max: number; clockErrMs: number };
  /** flood frame counters seen (data byte 0..1 little-endian), for gap accounting in perf tests */
  floodSeen: number;
}

export interface CanTraceOptions {
  /** ms clock (budget, "N s 전"); default performance.now */
  now?: () => number;
  /** tests: force the display-budget mode */
  forceDegraded?: boolean;
  /** dry-run debounce timer (tests); default setTimeout */
  setTimeout?: (cb: () => void, ms: number) => unknown;
  clearTimeout?: (h: unknown) => void;
  /** B1: wall clock ms for transmit-list saved_at (tests); default Date.now */
  wallNow?: () => number;
}

/** RPC result → its data (D5 envelope `{data}` or the bare object) */
const dataOf = (r: unknown): Record<string, unknown> => {
  const o = typeof r === 'object' && r !== null ? (r as Record<string, unknown>) : {};
  return typeof o.data === 'object' && o.data !== null ? (o.data as Record<string, unknown>) : o;
};

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
const setText = (e: HTMLElement, t: string) => {
  if (e.textContent !== t) e.textContent = t;
};
const fmtN = (n: number) => n.toLocaleString('ko-KR');

/** bound channel (`channel`, legacy `ch`); null = unbound module application (no implicit ch0) */
export function channelOf(props: Readonly<Record<string, unknown>>): string | null {
  const c = props.channel ?? props.ch;
  return typeof c === 'string' && c ? c : null;
}

/** Never-fed mirror for the unbound module: no subscription is opened (ctx.store is lazy). */
const idleMirror = (): StoreMirror<LoomEvent> => ({
  snapshot: null,
  snapshotDone: true,
  headSeq: 0n,
  gaps: [],
  tail: () => [],
  after: () => [],
  overwritten: 0,
  project: async () => ({ rows: [], fromSeq: null, toSeq: null, truncated: false }),
  onAppend: () => () => {},
});
const IDLE_ROWS: RowView = { list: () => [], get: () => undefined, onChange: () => () => {}, evicted: 0, rejected: 0 };

/**
 * Bound stream state from the mirror: 'waiting' while the newest mark is the net layer's
 * subscription-cap park and no snapshot has arrived; 'live' otherwise; 'unbound' without a channel.
 */
export function subWaitOf(mirror: Pick<StoreMirror<LoomEvent>, 'gaps' | 'snapshotDone'>, bound: boolean): 'unbound' | 'waiting' | 'live' {
  if (!bound) return 'unbound';
  const g = mirror.gaps[mirror.gaps.length - 1];
  return !mirror.snapshotDone && g?.kind === 'resync' && g.cause.startsWith('subscription_cap') ? 'waiting' : 'live';
}

export type CanView = 'rxtx' | 'trace';
/** initial view: `view: "rxtx"` opens the Receive/Transmit view; default stays the chronological trace */
export function viewOf(props: Readonly<Record<string, unknown>>): CanView {
  return props.view === 'rxtx' ? 'rxtx' : 'trace';
}

/** gap payload/mark count → number | 'unknown' */
const gapCount = (v: unknown): number | 'unknown' => {
  const n = typeof v === 'number' ? v : typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : NaN;
  return Number.isFinite(n) ? n : 'unknown';
};

export function rowsOf(props: Readonly<Record<string, unknown>>): number {
  const v = Number(props.rows);
  return Number.isFinite(v) && v > 0 ? Math.min(ROWS_MAX, Math.round(v)) : ROWS_DEFAULT;
}


export function createCanTrace(ctx: PanelContext, opts: CanTraceOptions = {}): PanelInstance {
  const bound = channelOf(ctx.props);
  /** unbound: the full application with empty data; alias-dependent operations stay disabled */
  const channel = bound ?? '';
  const now = opts.now ?? (() => performance.now());
  const model = new CanTraceModel({ capacity: rowsOf(ctx.props), now, recordOk: () => ctx.recordHealth === 'ok' });
  const mirror = bound ? ctx.store('can') : idleMirror();
  const actMirror = bound ? ctx.store('activity') : idleMirror();
  const rowView = bound ? ctx.rows({ domain: 'can', channel: bound }) : IDLE_ROWS;
  const mine = new Set<string>(); // FIFO ≤ MY_REQ_MAX
  const isMine = (rid: string) => mine.has(rid);

  // ---------------- DOM ----------------
  const root = el('div', 'can-trace');
  root.dataset.testid = 'can-trace';
  root.dataset.channel = channel;
  root.dataset.bound = String(bound !== null);
  const kv = defaultKV();
  let timeMode: TimeMode = readTimeMode(kv);
  let userRef: TimeRef | null = null;
  const timeRef = (): TimeRef | null => userRef ?? model.firstFrameRef;
  const wallNow = opts.wallNow ?? (() => Date.now());
  const top = el('div', 'ct-bar-top');
  const chLabel = el('span', 'mono', bound ? `CAN ${bound}` : 'CAN · 채널 미선택');
  const envBadge = el('span', 'muted');
  const live = el('span', 'ct-live');
  live.dataset.testid = 'can-live';
  const pauseBtn = el('button', undefined, '⏸ 일시정지');
  pauseBtn.type = 'button';
  pauseBtn.title = '렌더만 정지 (수집 계속) — Space';
  pauseBtn.dataset.testid = 'can-pause';
  const idFilter = el('input', 'ct-filter-ids');
  idFilter.placeholder = 'id 0x300, 0x7E0-0x7EF';
  idFilter.setAttribute('aria-label', 'id 필터 (16진수 · 쉼표/공백 구분 · 범위 a-b · Enter)');
  const filterNorm = el('span', 'ct-filter-norm');
  filterNorm.dataset.testid = 'can-filter-norm';
  filterNorm.hidden = true;
  filterNorm.title = 'UI 필터의 숫자는 16진수입니다 (CLI --id 의 숫자만 쓴 값은 10진수)';
  const closedBadge = el('span', 'ct-closed', '채널 닫힘');
  closedBadge.dataset.testid = 'can-closed';
  closedBadge.hidden = true;
  closedBadge.title = '데몬 채널이 닫혔습니다 — 다시 열려면 CAN 모듈에서 채널 열기';
  const closeBtn = el('button', 'ct-close-btn', '채널 닫기…');
  closeBtn.type = 'button';
  closeBtn.dataset.testid = 'can-close';
  closeBtn.title = '데몬에서 이 CAN 채널을 닫습니다 — 모든 사용자·에이전트의 주기 송신·ISO-TP가 멈춤. 탭 ×는 보기만 닫음';
  const dirSel = el('select');
  dirSel.setAttribute('aria-label', '방향 필터');
  for (const [v, t] of [['all', 'rx+tx'], ['rx', 'rx'], ['tx', 'tx']] as const) {
    const o = el('option', undefined, t);
    o.value = v;
    dirSel.append(o);
  }
  const orgBoxes = new Map<Origin, HTMLInputElement>();
  const orgWrap = el('span');
  for (const [o, t] of [['agent', 'AGT'], ['user', 'USR'], ['device', 'DEV'], ['script', 'SCR']] as const) {
    const lab = el('label');
    const cb = el('input');
    cb.type = 'checkbox';
    cb.checked = true;
    orgBoxes.set(o, cb);
    lab.append(cb, ` ${t}`);
    orgWrap.append(lab);
  }
  const agentInfo = el('span', 'ct-agent');
  agentInfo.dataset.testid = 'can-agent-read';
  const tabs = el('span', 'ct-tabs');
  tabs.dataset.testid = 'can-view-tabs';
  tabs.setAttribute('role', 'tablist');
  const tabRxTx = el('button', 'ct-tab', '수신/송신');
  tabRxTx.dataset.testid = 'can-view-rxtx';
  const tabTrace = el('button', 'ct-tab', '트레이스');
  tabTrace.dataset.testid = 'can-view-trace';
  for (const b of [tabRxTx, tabTrace]) {
    b.type = 'button';
    b.setAttribute('role', 'tab');
  }
  tabs.append(tabRxTx, tabTrace);
  const timeSel = el('select');
  timeSel.dataset.testid = 'can-time-mode';
  timeSel.setAttribute('aria-label', '시각 표시 방식');
  for (const mode of TIME_MODES) {
    const o = el('option', undefined, TIME_LABEL[mode]);
    o.value = mode;
    timeSel.append(o);
  }
  timeSel.value = timeMode;
  const timeBox = el('span', 'ct-timebox');
  timeBox.dataset.testid = 'can-time-refbox';
  const refBtn = el('button', undefined, '기준점 = 선택 행');
  refBtn.type = 'button';
  refBtn.dataset.testid = 'can-time-ref';
  refBtn.title = '트레이스에서 클릭한 행을 상대 시각 0으로';
  const refReset = el('button', undefined, '시작 기준');
  refReset.type = 'button';
  refReset.dataset.testid = 'can-time-ref-reset';
  const refLbl = el('span', 'ct-time-ref-label');
  refLbl.dataset.testid = 'can-time-ref-label';
  timeBox.append(refBtn, refReset, refLbl);
  let dbInfo: DbInfo | null = null;
  const names = new DbNames();
  let dbReading = false;
  let dbAgain = false;
  let decoding = false;
  const dbChipBtn = el('button', 'ct-db ct-db-unknown', 'DB: ?');
  dbChipBtn.type = 'button';
  dbChipBtn.dataset.testid = 'can-db';
  const exportBtn = el('button', undefined, '내보내기…');
  exportBtn.type = 'button';
  exportBtn.dataset.testid = 'can-export';
  exportBtn.title = 'BLF/ASC 파일을 데몬 컴퓨터에 만듭니다 (브라우저 다운로드 아님)';
  top.append(chLabel, ...(bound ? [closedBadge] : []), envBadge, ...(bound ? [dbChipBtn] : []), tabs, live, pauseBtn, timeSel, timeBox, idFilter, filterNorm, dirSel, orgWrap, agentInfo, ...(bound ? [exportBtn, closeBtn] : []));

  const tableHost = el('div', 'ct-host');
  tableHost.style.flex = '1';
  tableHost.style.minHeight = '0';
  tableHost.style.display = 'flex';
  const view = new TraceView(tableHost, model, {
    onNeedRender: () => ctx.scheduler.invalidate(),
    rowState: (rid) => (rowView.get(rid)?.state as WState | undefined) ?? null,
    now,
    timeMode: () => timeMode,
    timeRef,
    onOpenDetail: (abs) => openDetail(abs),
  });
  const onTimeMode = (v: string): void => {
    if ((TIME_MODES as readonly string[]).includes(v)) {
      timeMode = v as TimeMode;
      writeTimeMode(kv, timeMode);
      ctx.scheduler.invalidate();
    }
  };
  const onSetRef = (): void => {
    const abs = view.selected;
    if (abs < 0) return;
    const t = model.timeAt(abs);
    userRef = { monoMs: t.monoMs, clock: t.clock, seq: t.seq, kind: 'row' };
    ctx.scheduler.invalidate();
  };
  const onResetRef = (): void => {
    userRef = null;
    ctx.scheduler.invalidate();
  };
  timeSel.addEventListener('change', () => onTimeMode(timeSel.value));
  refBtn.addEventListener('click', () => onSetRef());
  refReset.addEventListener('click', () => onResetRef());

  // ---------------- row detail (CAN-7) ----------------
  const copyText = (t: string): Promise<void> =>
    typeof navigator !== 'undefined' && navigator.clipboard?.writeText ? navigator.clipboard.writeText(t) : Promise.reject(new Error('clipboard 없음'));
  const detailBox = el('div', 'ct-detail');
  detailBox.dataset.testid = 'can-row-detail';
  detailBox.setAttribute('role', 'region');
  detailBox.setAttribute('aria-label', '행 상세');
  detailBox.hidden = true;
  const detailFields = el('div', 'ct-detail-fields');
  const detailTools = el('div', 'ct-detail-tools');
  const detailCopy = el('button', undefined, '복사');
  detailCopy.type = 'button';
  detailCopy.dataset.testid = 'can-row-detail-copy';
  const detailClose = el('button', undefined, '닫기');
  detailClose.type = 'button';
  detailClose.dataset.testid = 'can-row-detail-close';
  const detailMsg = el('span', 'ct-detail-msg');
  detailMsg.dataset.testid = 'can-row-detail-msg';
  detailMsg.setAttribute('aria-live', 'polite');
  const detailDecode = el('button', undefined, '디코드');
  detailDecode.type = 'button';
  detailDecode.dataset.testid = 'can-row-detail-decode';
  detailDecode.title = '이 행을 채널 DBC로 디코드 (저널 1행 질의)';
  detailDecode.disabled = true;
  const detailDecoded = el('div', 'ct-detail-decoded');
  detailDecoded.dataset.testid = 'can-row-detail-decoded';
  detailTools.append(detailDecode, detailCopy, detailClose, detailMsg);
  detailBox.append(detailFields, detailTools, detailDecoded);
  let detail: RowFacts | null = null;
  const openDetail = (abs: number): void => {
    const f = rowFacts(model, abs);
    if (f === null) return;
    detail = f;
    detailFields.replaceChildren(
      ...rowDetailFields(f).map((d) => {
        const s = el('span', 'ct-detail-f');
        s.dataset.k = d.key;
        s.append(el('b', 'ct-detail-k', d.label), el('span', 'ct-detail-v', d.value));
        return s;
      }),
    );
    setText(detailMsg, '');
    detailMsg.classList.remove('err');
    detailDecoded.replaceChildren();
    detailBox.hidden = false;
    syncDecode();
  };
  const closeDetail = (): void => {
    detail = null;
    detailBox.hidden = true;
    detailFields.replaceChildren();
    detailDecoded.replaceChildren();
    syncDecode();
  };
  const copyDetail = (): void => {
    if (detail === null) return;
    void copyText(rowDetailText(detail)).then(
      () => {
        setText(detailMsg, '복사함');
        detailMsg.classList.remove('err');
      },
      () => {
        setText(detailMsg, '복사 실패 — 상세 줄에서 직접 선택해 복사하세요');
        detailMsg.classList.add('err');
      },
    );
  };
  detailCopy.addEventListener('click', () => copyDetail());
  detailClose.addEventListener('click', () => closeDetail());
  detailBox.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeDetail();
    }
    e.stopPropagation();
  });

  const send = el('div', 'ct-send');
  const sendLabel = el('span', undefined, '송신');
  const input = el('input');
  input.placeholder = '0x300#00 · 18DAF110#02 10 03 · fd 0x123#…';
  input.setAttribute('aria-label', 'CAN 송신 (Enter)');
  input.dataset.testid = 'can-send-input';
  input.spellcheck = false;
  input.autocomplete = 'off';
  const sendBtn = el('button', undefined, '보내기 ⏎');
  sendBtn.type = 'button';
  const risk = el('span', 'ct-risk', 'R?');
  risk.dataset.testid = 'can-risk';
  const RISK_HINT = '등급은 데몬이 dry-run으로 알려 준 값 (F1: 기본 정책 모두 자동, 표시만). STOP 중에도 사용자 송신은 허용(A3)';
  risk.title = `입력하면 등급을 미리 봅니다 — ${RISK_HINT}`;
  const msg = el('span', 'ct-msg');
  msg.setAttribute('aria-live', 'polite');
  msg.dataset.testid = 'can-send-msg';
  send.append(sendLabel, input, sendBtn, risk, msg);

  const status = el('div', 'ct-status');
  status.dataset.testid = 'can-status';
  // Receive/Transmit view (PCAN-View analogue). Placed AFTER the trace host so the trace grid
  // stays the first [role=grid] in the panel (e2e scans rely on it).
  const summary = new RxSummary({ maxIds: RX_SUMMARY_MAX_IDS, nowMs: now() });
  const txList = new TxList();
  let curIds: IdMatch | null = null;
  let curIdFilter: IdFilter | null = null;
  let curDir: 'all' | 'rx' | 'tx' = 'all';
  let curOrigins: ReadonlySet<Origin> | null = null;
  // ---------------- periodic jobs (CAN-2; daemon jobs, no browser timers) ----------------
  const jobs = new JobTable();
  const myJobs = new Set<string>();
  const entryJob = new Map<number, string>();
  let jobsSeen = -1;
  const rxtx = new RxTxView(summary, txList, {
    channel,
    startPeriodic: (uid) => void startEntry(uid),
    stopPeriodic: (uid) => {
      const j = entryJob.get(uid);
      if (j) void stopJob(j);
    },
    periodicOf: (uid) => {
      const j = entryJob.get(uid);
      if (!j) return null;
      const r = jobs.get(j);
      return { job: j, state: r ? r.state : 'requested', reason: r ? r.reason : null };
    },
    idFilter: () => curIds,
    sendEntry: (uid) => sendEntry(uid),
    openTrace: (r) => {
      idFilter.value = r.ext ? `${r.id.toString(16).toUpperCase().padStart(8, '0')}x` : `0x${r.id.toString(16).toUpperCase().padStart(3, '0')}`;
      applyFilter();
      setView('trace');
    },
    draftChanged: (p) => schedulePreview(p),
    now,
    listChanged: () => ctx.scheduler.invalidate(),
    wallNow,
    copyText,
    msgName: (id, ext) => names.nameOf(id, ext),
  });
  const perView = bound
    ? new PeriodicView(jobs, {
        isMine: (j) => myJobs.has(j),
        stop: (j) => void stopJob(j),
        takeover: (j) => void takeoverJob(j),
        refresh: () => void refreshJobs(),
      })
    : null;
  if (perView) rxtx.periodicHost.append(perView.root);
  const say = (t: string, err: boolean): void => perView?.setStatus(t, err);
  const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
  const refreshJobs = async (): Promise<void> => {
    if (!bound) return;
    say('목록 읽는 중…', false);
    try {
      const v = await ctx.query('can.periodic.list', { channel });
      if (jobs.applyList(v, channel)) say(`목록 읽음 — 작업 ${jobs.list().filter((r) => r.state !== 'stopped').length}개`, false);
      else say('목록 형식을 알 수 없음', true);
    } catch (e) {
      say(`목록 읽기 실패: ${errText(e)}`, true);
    } finally {
      ctx.scheduler.invalidate();
    }
  };
  const startEntry = async (uid: number): Promise<void> => {
    const e = txList.get(uid);
    if (!e) return;
    if (ctx.stopLatched) {
      say('STOP 중 — 주기 송신을 시작할 수 없음', true);
      return;
    }
    const cur = entryJob.get(uid);
    if (cur && jobs.get(cur)?.state !== 'stopped') {
      say(`이미 실행 중: ${cur}`, true);
      return;
    }
    const sp = startParams(e, channel);
    if (!sp.ok) {
      say(sp.error, true);
      return;
    }
    try {
      const hd = await ctx.act('periodicStart', sp.params);
      const job = (hd.result as { job?: unknown } | undefined)?.job;
      if (typeof job === 'string') {
        myJobs.add(job);
        entryJob.set(uid, job);
        say(`주기 시작 ${job} · ${e.cycleMs} ms`, false);
      } else say('시작됨 — 작업 id 없음 (목록 새로고침)', true);
    } catch (err) {
      say(`✗ 주기 시작 실패: ${errText(err)}`, true);
    } finally {
      rxtx.periodicChanged();
      rxtx.render();
      perView?.render(true);
      ctx.scheduler.invalidate();
    }
  };
  const stopJob = async (job: string): Promise<void> => {
    try {
      await ctx.act('periodicStop', { job });
      jobs.applyEvent({ kind: 'can.job_state', channel, payload: { job, state: 'stopped' } }, channel);
      say(`정지됨 ${job}`, false);
    } catch (err) {
      say(`✗ 정지 실패 (${job}): ${errText(err)}`, true);
    } finally {
      rxtx.periodicChanged();
      rxtx.render();
      perView?.render(true);
      ctx.scheduler.invalidate();
    }
  };
  const takeoverJob = async (job: string): Promise<void> => {
    try {
      const hd = await ctx.act('periodicTakeover', { job });
      const nj = (hd.result as { job?: unknown } | undefined)?.job;
      if (typeof nj === 'string') {
        myJobs.add(nj);
        for (const [u, j] of entryJob) if (j === job) entryJob.set(u, nj);
        say(`넘겨받음: ${job} → ${nj}`, false);
      }
      await refreshJobs();
    } catch (err) {
      if (err instanceof GateError && err.code === 'E_CANCELLED') say('넘겨받기 취소됨', false);
      else say(`✗ 넘겨받기 실패: ${errText(err)}`, true);
    } finally {
      perView?.render(true);
      ctx.scheduler.invalidate();
    }
  };
  if (bound) void refreshJobs();
  let savedVersion = -1;
  const persistLoad = (): void => {
    if (!bound) {
      rxtx.setPersist('채널 미선택 — 송신 목록을 저장하지 않음', 'off');
    } else {
      const r = loadTxList(kv, channel);
      if (r.ok) {
        if (r.items.length) txList.load(r.items, 'replace');
        let text = r.items.length ? `이 브라우저에 저장된 목록 ${r.items.length}개 불러옴 (${channel})` : `이 브라우저에 저장 (${channel}) — 저장된 목록 없음`;
        if (r.skipped.length) text += ` · 건너뜀 ${r.skipped.length}`;
        if (r.dropped) text += ` · 상한 초과 ${r.dropped}`;
        rxtx.setPersist(text, 'ok');
      } else {
        rxtx.setPersist(`저장된 목록을 읽지 못함: ${r.error}`, 'err');
      }
    }
    savedVersion = txList.contentVersion;
    rxtx.render();
  };
  persistLoad();
  const persistTick = (): void => {
    if (!bound || txList.contentVersion === savedVersion) return;
    savedVersion = txList.contentVersion;
    const r = saveTxList(kv, channel, txList.entries, wallNow());
    if (r.ok) rxtx.setPersist(`이 브라우저에 저장됨 (${channel}, ${txList.entries.length}개)`, 'ok');
    else rxtx.setPersist(`저장 안 됨: ${r.error}`, 'err');
  };
  let lastRxtxDraw = Number.NEGATIVE_INFINITY;
  let lastRxtxCost = 0;
  let curView: CanView = viewOf(ctx.props);
  const setView = (v: CanView) => {
    curView = v;
    root.dataset.view = v;
    tableHost.style.display = v === 'trace' ? 'flex' : 'none';
    rxtx.root.hidden = v !== 'rxtx';
    tabTrace.setAttribute('aria-selected', String(v === 'trace'));
    tabRxTx.setAttribute('aria-selected', String(v === 'rxtx'));
    if (v === 'rxtx') rxtx.render();
    ctx.scheduler.invalidate();
  };
  tabRxTx.addEventListener('click', () => setView('rxtx'));
  tabTrace.addEventListener('click', () => setView('trace'));

  // unbound module: interface/connect section above the (empty) tools
  const connect: CanConnectSection | null = bound ? null : createCanConnect(ctx, () => curView);
  // ---------------- channel close (CAN-6; bound panels only) ----------------
  const closeBox = el('div', 'ct-closebox');
  closeBox.dataset.testid = 'can-close-box';
  closeBox.setAttribute('role', 'group');
  closeBox.setAttribute('aria-label', '채널 닫기 확인');
  closeBox.hidden = true;
  const closeLines = el('div', 'ct-closebox-lines');
  closeLines.dataset.testid = 'can-close-lines';
  const closeRow = el('div', 'ct-closebox-row');
  const closeGo = el('button', 'ct-close-go', '채널 닫기');
  closeGo.type = 'button';
  closeGo.dataset.testid = 'can-close-confirm';
  const closeCancel = el('button', undefined, '취소');
  closeCancel.type = 'button';
  closeCancel.dataset.testid = 'can-close-cancel';
  const closeMsg = el('span', 'ct-close-msg');
  closeMsg.dataset.testid = 'can-close-msg';
  closeMsg.setAttribute('aria-live', 'polite');
  closeRow.append(closeGo, closeCancel, closeMsg);
  closeBox.append(closeLines, closeRow);
  let closing: 'idle' | 'reading' | 'preview' | 'acting' = 'idle';
  let impact: CloseImpact | null = null;
  let chState: 'open' | 'closed' | null = null;
  let closeAbort: AbortController | null = null;
  const setCloseLines = (lines: string[]): void => {
    closeLines.replaceChildren(...lines.map((l) => el('div', undefined, l)));
  };
  const openClosePreview = async (): Promise<void> => {
    if (!bound || closing !== 'idle') return;
    closing = 'reading';
    impact = null;
    setCloseLines(['읽는 중 — 주기 송신 목록 · 채널 상태']);
    setText(closeMsg, '');
    closeMsg.classList.remove('err');
    closeGo.disabled = true;
    closeCancel.disabled = false;
    closeBox.hidden = false;
    const ac = new AbortController();
    closeAbort = ac;
    const [l, s] = await Promise.allSettled([ctx.query('can.periodic.list', { channel }, ac.signal), ctx.query('can.status', { channel }, ac.signal)]);
    if (closing !== 'reading' || ac.signal.aborted) return;
    impact = closeImpact(channel, l, s);
    setCloseLines(closeImpactLines(impact));
    closing = 'preview';
    closeGo.disabled = false;
    ctx.scheduler.invalidate();
  };
  const confirmClose = async (): Promise<void> => {
    if (closing !== 'preview') return;
    closing = 'acting';
    closeGo.disabled = closeCancel.disabled = true;
    setText(closeMsg, '닫는 중…');
    try {
      const hd = await ctx.act('close', { channel });
      ctx.ui.toast(closeDoneText(channel, hd.result, impact));
      chState = 'closed';
      closing = 'idle';
      impact = null;
      closeBox.hidden = true;
      setText(closeMsg, '');
    } catch (e) {
      closing = 'preview';
      closeGo.disabled = closeCancel.disabled = false;
      if (e instanceof GateError && e.code === 'E_CANCELLED') {
        setText(closeMsg, '취소됨 — 채널은 열려 있음');
        closeMsg.classList.remove('err');
      } else {
        setText(closeMsg, `✗ 닫기 실패: ${e instanceof Error ? e.message : String(e)}`);
        closeMsg.classList.add('err');
      }
    } finally {
      ctx.scheduler.invalidate();
    }
  };
  const cancelClose = (): void => {
    closeAbort?.abort();
    closeAbort = null;
    closing = 'idle';
    impact = null;
    closeBox.hidden = true;
    setText(closeMsg, '');
    closeBtn.focus();
    ctx.scheduler.invalidate();
  };
  closeBtn.addEventListener('click', () => void openClosePreview());
  closeGo.addEventListener('click', () => void confirmClose());
  closeCancel.addEventListener('click', () => cancelClose());
  closeBox.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && closing !== 'acting') {
      e.preventDefault();
      cancelClose();
    }
    e.stopPropagation();
  });
  const offCtl = bound
    ? ctx.onControl((ev) => {
        const st = channelStateOf(ev, channel);
        if (st) {
          chState = st;
          ctx.scheduler.invalidate();
        }
        if (jobs.applyEvent(ev, channel)) ctx.scheduler.invalidate();
      })
    : () => {};

  // ---------------- export (CAN-4; bound panels only) ----------------
  const exportBox = el('div', 'ct-exportbox');
  exportBox.dataset.testid = 'can-export-box';
  exportBox.setAttribute('role', 'group');
  exportBox.setAttribute('aria-label', '내보내기');
  exportBox.hidden = true;
  const expRow1 = el('div', 'ct-exportbox-row');
  const fmtSel = el('select');
  fmtSel.dataset.testid = 'can-export-format';
  for (const [v, t] of [['blf', 'BLF'], ['asc', 'ASC']] as const) {
    const o = el('option', undefined, t);
    o.value = v;
    fmtSel.append(o);
  }
  const scopeSel = el('select');
  scopeSel.dataset.testid = 'can-export-scope';
  for (const [v, t] of [['ring', '이 패널 보관 범위'], ['recent', '최근 10분']] as const) {
    const o = el('option', undefined, t);
    o.value = v;
    scopeSel.append(o);
  }
  const useFilterLab = el('label');
  const useFilterCb = el('input');
  useFilterCb.type = 'checkbox';
  useFilterCb.dataset.testid = 'can-export-usefilter';
  useFilterLab.append(useFilterCb, ' 현재 id·방향·출처 필터 적용');
  const pathIn = el('input', 'ct-export-path');
  pathIn.dataset.testid = 'can-export-path';
  pathIn.placeholder = '비우면 데몬 임시 폴더에 새 파일 (데몬 컴퓨터 절대 경로)';
  pathIn.setAttribute('aria-label', '데몬 쪽 저장 경로');
  expRow1.append(fmtSel, scopeSel, useFilterLab, pathIn);
  const expRow2 = el('div', 'ct-exportbox-row');
  const goBtn = el('button', undefined, '내보내기');
  goBtn.type = 'button';
  goBtn.dataset.testid = 'can-export-go';
  const copyBtn = el('button', undefined, '경로 복사');
  copyBtn.type = 'button';
  copyBtn.dataset.testid = 'can-export-copy';
  copyBtn.hidden = true;
  const closeExp = el('button', undefined, '닫기');
  closeExp.type = 'button';
  closeExp.dataset.testid = 'can-export-close';
  const expMsg = el('span', 'ct-export-msg');
  expMsg.dataset.testid = 'can-export-msg';
  expMsg.setAttribute('aria-live', 'polite');
  expRow2.append(goBtn, copyBtn, closeExp, expMsg);
  const expResult = el('div', 'ct-export-result');
  expResult.dataset.testid = 'can-export-result';
  exportBox.append(expRow1, expRow2, expResult);
  let exporting = false;
  let exportPath: string | null = null;
  const openExport = (): void => {
    exportBox.hidden = false;
    setText(expMsg, '');
    expMsg.classList.remove('err');
  };
  const closeExport = (): void => {
    exportBox.hidden = true;
  };
  const runExport = async (): Promise<void> => {
    if (!bound || exporting) return;
    const rs = ringSeqs(model, K_GAP);
    const req = exportRequest({
      channel,
      format: fmtSel.value as ExportFormatUi,
      scope: scopeSel.value as ExportScope,
      ringFirstSeq: rs.first,
      ringLastSeq: rs.last,
      useFilter: useFilterCb.checked,
      ids: curIdFilter,
      dir: curDir,
      origins: curOrigins,
      path: pathIn.value,
    });
    if (!req.ok) {
      setText(expMsg, req.error);
      expMsg.classList.add('err');
      return;
    }
    exporting = true;
    goBtn.disabled = true;
    setText(expMsg, `내보내는 중… (${req.scopeText}, 최대 600초)`);
    expMsg.classList.remove('err');
    try {
      const hd = await ctx.act('export', req.params);
      const v = exportResultLines(hd.result);
      expResult.replaceChildren(...v.lines.map((l) => el('div', undefined, l)));
      expResult.classList.toggle('warn', v.warn);
      exportPath = v.path;
      copyBtn.hidden = v.path === null;
      setText(expMsg, '');
    } catch (e) {
      setText(expMsg, `✗ 내보내기 실패: ${e instanceof Error ? e.message : String(e)}`);
      expMsg.classList.add('err');
    } finally {
      exporting = false;
      goBtn.disabled = false;
      ctx.scheduler.invalidate();
    }
  };
  copyBtn.addEventListener('click', () => {
    if (exportPath === null) return;
    void copyText(exportPath).then(
      () => setText(expMsg, '경로를 복사함'),
      () => {
        setText(expMsg, '복사 실패 — 결과 줄에서 직접 선택하세요');
        expMsg.classList.add('err');
      },
    );
  });
  exportBtn.addEventListener('click', () => openExport());
  goBtn.addEventListener('click', () => void runExport());
  closeExp.addEventListener('click', () => closeExport());
  exportBox.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeExport();
    }
    e.stopPropagation();
  });

  // ---------------- DBC (CAN-5; bound panels only) ----------------
  const dbBox = el('div', 'ct-dbbox');
  dbBox.dataset.testid = 'can-db-box';
  dbBox.setAttribute('role', 'group');
  dbBox.setAttribute('aria-label', 'DBC');
  dbBox.hidden = true;
  const dbPath = el('input', 'ct-db-path');
  dbPath.dataset.testid = 'can-db-path';
  dbPath.placeholder = '/abs/path/body.dbc';
  dbPath.setAttribute('aria-label', 'DBC 경로 (데몬 컴퓨터 절대 경로)');
  const dbEnc = el('select');
  dbEnc.dataset.testid = 'can-db-encoding';
  const encLabel: Record<DbEncoding, string> = { auto: '인코딩 자동', utf8: 'UTF-8', windows1252: 'Windows-1252' };
  for (const enc of DB_ENCODINGS) {
    const o = el('option', undefined, encLabel[enc]);
    o.value = enc;
    dbEnc.append(o);
  }
  const dbLoadBtn = el('button', undefined, '불러오기');
  dbLoadBtn.type = 'button';
  dbLoadBtn.dataset.testid = 'can-db-load';
  const dbCloseBtn = el('button', undefined, '닫기');
  dbCloseBtn.type = 'button';
  dbCloseBtn.dataset.testid = 'can-db-close';
  const dbMsg = el('span', 'ct-db-msg');
  dbMsg.dataset.testid = 'can-db-msg';
  dbMsg.setAttribute('aria-live', 'polite');
  dbBox.append(el('span', undefined, 'DBC 경로 (데몬 컴퓨터)'), dbPath, dbEnc, dbLoadBtn, dbCloseBtn, dbMsg);
  const syncDecode = (): void => {
    detailDecode.disabled = !(bound && dbInfo?.loaded === true && detail !== null && detail.kind === 'frame' && detail.seq > 0) || decoding;
  };
  const applyDbInfo = (i: DbInfo | null): void => {
    dbInfo = i;
    if (i === null) return;
    const sha = i.loaded ? i.sha : null;
    if (sha !== names.sha) {
      names.reset(sha);
      rxtx.namesChanged();
      if (sha) void readNames(sha);
    }
  };
  const readDbInfo = async (): Promise<void> => {
    if (!bound) return;
    if (dbReading) {
      dbAgain = true;
      return;
    }
    dbReading = true;
    try {
      const v = await ctx.query('can.db.info', { channel });
      applyDbInfo(parseDbInfo(v, channel));
    } catch {
      applyDbInfo(null);
    } finally {
      dbReading = false;
      if (dbAgain) {
        dbAgain = false;
        void readDbInfo();
      }
      ctx.scheduler.invalidate();
    }
  };
  const readNames = async (sha: string): Promise<void> => {
    let cursor: number | null = 0;
    try {
      while (cursor !== null) {
        const v = await ctx.query('can.db.catalog', { channel, cursor, max: DB_PAGE_SIZE });
        if (names.sha !== sha) return;
        cursor = names.addPage(v);
      }
    } catch (e) {
      setText(dbMsg, `메시지 이름 읽기 실패: ${e instanceof Error ? e.message : String(e)}`);
      dbMsg.classList.add('err');
    } finally {
      rxtx.namesChanged();
      ctx.scheduler.invalidate();
    }
  };
  const loadDb = async (): Promise<void> => {
    if (!(ctx.rights.has('operate') || ctx.rights.has('approve'))) {
      setText(dbMsg, 'DBC 불러오기는 operate 권한 필요');
      dbMsg.classList.add('err');
      return;
    }
    const lp = dbLoadParams(dbPath.value, dbEnc.value as DbEncoding, channel);
    if (!lp.ok) {
      setText(dbMsg, lp.error);
      dbMsg.classList.add('err');
      return;
    }
    dbLoadBtn.disabled = true;
    setText(dbMsg, '불러오는 중…');
    dbMsg.classList.remove('err');
    try {
      const r = await ctx.query('can.db.load', lp.params);
      const i = parseDbInfo({ channels: [r] }, channel);
      applyDbInfo(i);
      const base = i?.path?.split(/[\\/]/).filter(Boolean).pop() ?? '?';
      setText(dbMsg, `불러옴: ${base} · 메시지 ${i?.messages ?? '?'}개${(r as { changed?: unknown } | null)?.changed === false ? ' (같은 파일)' : ''}`);
    } catch (e) {
      setText(dbMsg, `✗ 불러오기 실패: ${e instanceof Error ? e.message : String(e)}`);
      dbMsg.classList.add('err');
    } finally {
      dbLoadBtn.disabled = false;
      ctx.scheduler.invalidate();
    }
  };
  const decodeDetail = async (): Promise<void> => {
    const f = detail;
    if (!bound || f === null || decoding) return;
    decoding = true;
    syncDecode();
    let lines: string[];
    try {
      const r = await ctx.query('can.trace.query', { channel, since: `seq:${f.seq}`, until: `seq:${f.seq}`, max: 1, decode: true });
      const rows = (r as { rows?: unknown } | null)?.rows;
      const row = Array.isArray(rows) ? (rows as Record<string, unknown>[]).find((x) => typeof x === 'object' && x !== null && String(x.seq) === String(f.seq)) : undefined;
      lines = row ? decodedLines(row.decoded) : ['저널에서 이 행을 찾지 못함 (보관 범위 밖일 수 있음)'];
    } catch (e) {
      lines = [`✗ 디코드 실패: ${e instanceof Error ? e.message : String(e)}`];
    } finally {
      decoding = false;
    }
    if (detail === f) detailDecoded.replaceChildren(...lines.map((l) => el('div', undefined, l)));
    syncDecode();
    ctx.scheduler.invalidate();
  };
  dbChipBtn.addEventListener('click', () => {
    dbBox.hidden = false;
    void readDbInfo();
  });
  dbLoadBtn.addEventListener('click', () => void loadDb());
  dbCloseBtn.addEventListener('click', () => {
    dbBox.hidden = true;
  });
  dbBox.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      dbBox.hidden = true;
    }
    e.stopPropagation();
  });
  detailDecode.addEventListener('click', () => void decodeDetail());
  if (bound) void readDbInfo();

  root.append(createLatchLine(ctx), ...(connect ? [connect.root] : []), top, ...(bound ? [closeBox, exportBox, dbBox] : []), tableHost, detailBox, rxtx.root, send, status);
  setView(curView);

  // ---------------- ingest ----------------
  let lastSeq = -1n;
  let lastEpoch = '';
  let resyncPending = false;
  let lastMark: GapMark | null = null;
  const gapKeys: string[] = []; // FIFO ≤ GAP_KEYS_MAX
  let ingestMs = 0;
  let floodSeen = 0;
  /** first abs row that arrived live (after snapshot_end); snapshot backlog is never timed */
  let liveFromAbs = Number.POSITIVE_INFINITY;

  const rememberGapKey = (k: string) => {
    gapKeys.push(k);
    if (gapKeys.length > GAP_KEYS_MAX) gapKeys.shift();
  };
  const seqStr = (v: unknown) => (typeof v === 'string' || typeof v === 'number' || typeof v === 'bigint' ? String(v) : 'null');

  const applyOne = (ev: LoomEvent) => {
    if (ev.kind === 'display.gap' || ev.kind === 'record.gap' || ev.kind === 'source.gap') {
      const pl = (ev.payload ?? {}) as { seq_range?: { from?: unknown; to?: unknown }; count?: unknown };
      const r = pl.seq_range;
      rememberGapKey(`${ev.kind}:${seqStr(r?.from)}:${seqStr(r?.to)}`);
      summary.noteGap(gapCount(pl.count));
    }
    if (ev.kind === 'can.frame') {
      const p = ev.payload as { id?: unknown };
      if (typeof p.id === 'number' && p.id >= 0x400 && p.id < 0x440) floodSeen++;
      summary.apply(ev);
    }
    model.apply(ev, isMine);
  };

  const consumeCan = () => {
    const t0 = now();
    let evs: readonly LoomEvent[];
    if (resyncPending) {
      // after a resync the epoch may have changed (seqs restart): scan the mirror once and
      // take what is newer than what we hold (same epoch: seq > last; other epoch: all)
      evs = mirror.tail(MIRROR_ROWS).filter((e) => e.epoch !== lastEpoch || BigInt(e.seq) > lastSeq);
      if (evs.length > 0 || mirror.snapshotDone) resyncPending = false;
    } else {
      evs = mirror.after(lastSeq);
    }
    let dbChanged = false;
    for (const ev of evs) {
      applyOne(ev);
      if (bound && ev.kind === 'can.db_changed' && ev.channel === channel) dbChanged = true;
      lastSeq = BigInt(ev.seq);
      lastEpoch = ev.epoch;
    }
    if (dbChanged) void readDbInfo();
    // gap marks not carried as events: batch display_gap summaries and resyncs
    const marks = mirror.gaps;
    let from = lastMark ? marks.lastIndexOf(lastMark) + 1 : 0;
    if (from < 0) from = 0;
    for (let i = from; i < marks.length; i++) {
      const g = marks[i]!;
      if (g.kind === 'resync') {
        model.addGap({ kind: 'resync', first: null, last: null, count: 'unknown', cause: g.cause, recordOk: false, producerOnly: false, local: true });
        summary.noteGap('unknown');
        resyncPending = true;
        continue;
      }
      const key = `${g.kind}:${g.firstSeq === null ? 'null' : String(g.firstSeq)}:${g.lastSeq === null ? 'null' : String(g.lastSeq)}`;
      if (gapKeys.includes(key)) continue; // already shown from its event
      rememberGapKey(key);
      summary.noteGap(typeof g.count === 'number' ? g.count : 'unknown');
      model.addGap({
        kind: g.kind,
        first: g.firstSeq === null ? null : Number(g.firstSeq),
        last: g.lastSeq === null ? null : Number(g.lastSeq),
        count: g.count,
        cause: g.cause,
        recordOk: ctx.recordHealth === 'ok',
        producerOnly: false,
        local: false,
      });
    }
    if (marks.length) lastMark = marks[marks.length - 1]!;
    if (resyncPending || !mirror.snapshotDone) liveFromAbs = Number.POSITIVE_INFINITY;
    else if (liveFromAbs === Number.POSITIVE_INFINITY) liveFromAbs = model.total;
    ingestMs += now() - t0;
    ctx.scheduler.invalidate();
  };

  let lastActSeq = -1n;
  // agent reads made before this panel mounted (or before a reload's shell snapshot): the live-only
  // activity topic has no replay, so seed once from the shell's bounded Activity ring (read-only, no
  // subscription/call). lastActSeq dedups the live topic against the seeded seqs.
  if (bound) {
    for (const r of ctx.agentReads(AGENT_RANGES, channel)) {
      if (r.seq > lastActSeq) lastActSeq = r.seq;
      model.addAgentRange(Number(r.seqA), Number(r.seqB), r.actor, now());
    }
  }
  const consumeActivity = () => {
    let changed = false;
    for (const ev of actMirror.after(lastActSeq)) {
      lastActSeq = BigInt(ev.seq);
      if (model.applyActivity(ev, channel, now())) changed = true;
    }
    if (changed) ctx.scheduler.invalidate();
  };

  const offCan = mirror.onAppend(consumeCan);
  const offAct = actMirror.onAppend(consumeActivity);
  const offRows = rowView.onChange(() => ctx.scheduler.invalidate());

  // ---------------- controls ----------------
  const applyFilter = () => {
    let ids: IdFilter | null;
    try {
      ids = parseIdFilter(idFilter.value);
      idFilter.removeAttribute('aria-invalid');
    } catch (e) {
      idFilter.setAttribute('aria-invalid', 'true');
      showMsg(`필터 오류: ${(e as Error).message}`, true);
      return;
    }
    const origins = new Set<Origin>();
    for (const [o, cb] of orgBoxes) if (cb.checked) origins.add(o);
    const f: TraceFilter = {
      ids,
      dir: dirSel.value as TraceFilter['dir'],
      origins: origins.size === 4 ? null : origins,
    };
    model.setFilter(f);
    curIds = ids;
    curIdFilter = ids;
    curDir = f.dir as 'all' | 'rx' | 'tx';
    curOrigins = f.origins;
    filterNorm.hidden = ids === null;
    setText(filterNorm, ids ? `적용: ${ids.text}` : '');
    ctx.scheduler.invalidate();
  };
  idFilter.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') applyFilter();
  });
  idFilter.addEventListener('change', applyFilter);
  dirSel.addEventListener('change', applyFilter);
  for (const cb of orgBoxes.values()) cb.addEventListener('change', applyFilter);

  let pausedAt = 0;
  let pausedFrames = 0;
  const togglePause = () => {
    const p = !view.paused;
    view.setPaused(p);
    pausedAt = now();
    pausedFrames = model.framesIn;
    setText(pauseBtn, p ? '▶ 재개' : '⏸ 일시정지');
    pauseBtn.setAttribute('aria-pressed', String(p));
    ctx.scheduler.invalidate();
  };
  pauseBtn.addEventListener('click', togglePause);
  root.addEventListener('keydown', (e) => {
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA') return;
    if (e.key === ' ') {
      e.preventDefault();
      togglePause();
    } else if (e.key === 'End') {
      e.preventDefault();
      view.jumpToLive();
    }
  });

  // ---------------- send line ----------------
  const history: string[] = [];
  let histPos = -1;
  const showMsg = (t: string, err = false) => {
    setText(msg, t);
    msg.classList.toggle('err', err);
  };
  const doSend = () => {
    if (!bound) return; // unbound: nothing to send to (input is disabled too)
    const r = parseSendLine(input.value, channel);
    if (!r.ok) {
      input.setAttribute('aria-invalid', 'true');
      showMsg(r.error, true);
      return;
    }
    input.removeAttribute('aria-invalid');
    history.push(input.value.trim());
    if (history.length > HISTORY_MAX) history.shift();
    histPos = -1;
    input.value = '';
    schedulePreview(null);
    sendParams(r.params, r.canonical);
  };
  /** the one gated send path (send line and transmit list): ctx.act('send') → ActionGate */
  const sendParams = (params: CanSendParams, shown: string, done?: (rid: string | null, err: string | null) => void) => {
    showMsg(`USR ▶ ${shown} …`);
    ctx.act('send', params).then(
      (h) => {
        if (h.requestId) {
          mine.add(h.requestId);
          if (mine.size > MY_REQ_MAX) mine.delete(mine.values().next().value as string);
          model.markMine(h.requestId);
          const st = model.stateOf(h.requestId);
          showMsg(`USR ▶ ${shown} → ${h.requestId}${st ? ` (${st})` : ''}`);
        } else showMsg(`USR ▶ ${shown} 보냄 (request_id 없음)`);
        done?.(h.requestId ?? null, null);
        ctx.scheduler.invalidate();
      },
      (e: unknown) => {
        const code = e instanceof GateError ? e.code : ((e as { code?: string; dataCode?: string }).dataCode ?? (e as { code?: string }).code);
        const why = code === 'E_RIGHTS' ? 'operate 권한 필요' : code === 'E_STALLED' ? '연결 STALLED — 쓰기 잠김' : String((e as Error).message ?? e);
        showMsg(`✗ ${shown} 실패: ${why}`, true);
        done?.(null, why);
        ctx.scheduler.invalidate();
      },
    );
  };
  const sendEntry = (uid: number) => {
    const e = txList.get(uid);
    if (!e || !bound) return;
    rxtx.say(`▶ ${e.text} …`);
    sendParams({ ...e.params }, e.text, (rid, err) => {
      if (err === null) {
        txList.noteSent(uid, rid);
        rxtx.say(`▶ ${e.text} → ${rid ?? 'request_id 없음'}`);
      } else {
        txList.noteFailed(uid, err);
        rxtx.say(`✗ ${e.text}: ${err}`, true);
      }
      rxtx.render();
    });
  };
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      doSend();
    } else if (e.key === 'ArrowUp' && history.length) {
      e.preventDefault();
      histPos = histPos < 0 ? history.length - 1 : Math.max(0, histPos - 1);
      input.value = history[histPos] ?? '';
    } else if (e.key === 'ArrowDown' && histPos >= 0) {
      e.preventDefault();
      histPos = histPos + 1 >= history.length ? -1 : histPos + 1;
      input.value = histPos < 0 ? '' : (history[histPos] ?? '');
    }
  });
  // ---------------- risk preview (dry-run, debounced) ----------------
  const later = opts.setTimeout ?? ((cb: () => void, ms: number) => setTimeout(cb, ms));
  const cancel = opts.clearTimeout ?? ((h: unknown) => clearTimeout(h as ReturnType<typeof setTimeout>));
  let previewTimer: unknown = null;
  let previewAbort: AbortController | null = null;
  let previewGen = 0;
  const setRisk = (text: string, title: string, state: 'idle' | 'pending' | 'ok' | 'err') => {
    setText(risk, text);
    risk.title = title;
    risk.dataset.state = state;
  };
  const schedulePreview = (params: object | null) => {
    if (previewTimer !== null) cancel(previewTimer);
    previewTimer = null;
    previewAbort?.abort();
    previewAbort = null;
    const gen = ++previewGen;
    if (!params || !bound) {
      setRisk('R?', `입력하면 등급을 미리 봅니다 — ${RISK_HINT}`, 'idle');
      return;
    }
    setRisk('R…', '등급 확인 중 (dry-run)', 'pending');
    previewTimer = later(() => {
      previewTimer = null;
      const ac = new AbortController();
      previewAbort = ac;
      ctx.query('can.send', { ...params, dry_run: true }, ac.signal).then(
        (res) => {
          if (gen !== previewGen) return; // input changed meanwhile
          const d = dataOf(res);
          const r = typeof d.risk === 'string' ? d.risk : '?';
          const why = typeof d.reason === 'string' ? d.reason : '';
          const pol = typeof d.policy === 'string' ? d.policy : '?';
          setRisk(r, `${r}${why ? ` — ${why}` : ''} · 정책 ${pol}${typeof d.bytes === 'number' ? ` · ${d.bytes} B` : ''} · ${RISK_HINT}`, 'ok');
        },
        (e: unknown) => {
          if (gen !== previewGen || ac.signal.aborted) return;
          setRisk('R?', `등급 미리보기 실패: ${e instanceof Error ? e.message : String(e)}`, 'err');
        },
      );
    }, DRY_RUN_DEBOUNCE_MS);
  };

  input.addEventListener('input', () => {
    if (!input.value.trim() || !bound) {
      input.removeAttribute('aria-invalid');
      schedulePreview(null);
      return;
    }
    const r = parseSendLine(input.value, channel);
    if (r.ok) {
      input.removeAttribute('aria-invalid');
      const n = r.params.data.length / 2;
      showMsg(`→ ${formatFrame(r.params.id, r.params.ext, r.params.fd, r.params.brs === true, r.params.data)} (${r.params.ext ? '29bit' : '11bit'}${r.params.fd ? `, FD${r.params.brs ? ' BRS' : ''}` : ''}, ${n} B, DLC ${dlcForLen(n)})`);
      schedulePreview(r.params);
    } else {
      showMsg(r.error, true);
      schedulePreview(null);
    }
  });
  sendBtn.addEventListener('click', doSend);

  // ---------------- degrade / stats / latency ----------------
  let winStart = now();
  let winFrames = model.framesIn;
  let winDisp = model.dispAppended;
  let omittedReported = model.omitted;
  ctx.scheduler.onDegradedChange(() => ctx.scheduler.invalidate());
  let fpsIn = 0;
  let dispRate = 0;
  const hist = new Uint32Array(LAT_BUCKETS);
  let latN = 0;
  let latMax = 0;
  let latFrom = 0;
  const raf = typeof requestAnimationFrame === 'function' ? requestAnimationFrame : null;
  const monoNowMs = () => {
    const ns = ctx.clock.monoEstNs();
    return Number(ns / 1000n) / 1000;
  };

  const sampleLatency = () => {
    // generation (ts.mono_ns) → next rAF after the draw that put the rows in the list
    const to = model.total;
    let from = Math.max(latFrom, model.oldestAbs, liveFromAbs);
    latFrom = to;
    if (!raf || from >= to || view.paused || !view.follow) return;
    if (to - from > LAT_MAX_PER_FRAME) from = to - LAT_MAX_PER_FRAME;
    raf(() => {
      const t = monoNowMs();
      for (let abs = from; abs < to; abs++) {
        if (!model.has(abs) || model.kindAt(abs) !== K_FRAME || model.flagsAt(abs) & F_HIDDEN) continue;
        const lat = t - model.monoAt(abs);
        if (!(lat >= 0)) continue;
        hist[Math.min(LAT_BUCKETS - 1, Math.floor(lat / LAT_BUCKET_MS))]!++;
        latN++;
        if (lat > latMax) latMax = lat;
      }
    });
  };
  const quantile = (q: number) => {
    if (latN === 0) return 0;
    const target = q * latN;
    let acc = 0;
    for (let i = 0; i < LAT_BUCKETS; i++) {
      acc += hist[i]!;
      if (acc >= target) return (i + 1) * LAT_BUCKET_MS;
    }
    return latMax;
  };

  const stats = (): CanTraceStats => {
    const gapRows: CanTraceStats['gapRows'] = [];
    model.dispTrim();
    for (let k = 0; k < model.dispCount && gapRows.length < 1_000; k++) {
      const g = model.gapAt(model.dispAt(k));
      if (g) gapRows.push({ kind: g.kind, first: g.first, last: g.last, count: g.count, cause: g.cause });
    }
    return {
      rows: model.count,
      capacity: model.capacity,
      evicted: model.evicted,
      displayed: model.dispCount,
      framesIn: model.framesIn,
      omitted: model.omitted,
      gapRows,
      degraded: model.degraded,
      paused: view.paused,
      lat: { n: latN, p50: quantile(0.5), p95: quantile(0.95), p99: quantile(0.99), max: latMax, clockErrMs: ctx.clock.errMs() },
      floodSeen,
    };
  };

  const updateStatus = () => {
    const t = now();
    if (t - winStart >= 1000) {
      const dt = (t - winStart) / 1000;
      fpsIn = (model.framesIn - winFrames) / dt;
      dispRate = (model.dispAppended - winDisp) / dt;
      winStart = t;
      winFrames = model.framesIn;
      winDisp = model.dispAppended;
    }
    model.setDegraded(opts.forceDegraded === true || ctx.scheduler.degraded);
    if (model.omitted > omittedReported) {
      ctx.scheduler.reportOmitted(model.omitted - omittedReported);
      omittedReported = model.omitted;
    }
    // U-3: same lock as the shell's ActionGate (STALLED / disconnected) — disabled, not "refused on Enter"
    const hasRight = ctx.rights.has('operate') || ctx.rights.has('approve');
    connect?.update(hasRight && !ctx.writeLocked, hasRight);
    // unbound: no alias to send to — every send path stays disabled until a channel is bound
    const writable = bound !== null && hasRight && !ctx.writeLocked;
    if (bound) {
      closedBadge.hidden = chState !== 'closed';
      closeBtn.disabled = !(hasRight && !ctx.writeLocked) || closing !== 'idle' || chState === 'closed';
      exportBtn.disabled = ctx.writeLocked;
      const c = dbChip(dbInfo);
      setText(dbChipBtn, c.text);
      dbChipBtn.title = c.title;
      const cls = `ct-db ${c.cls}`;
      if (dbChipBtn.className !== cls) dbChipBtn.className = cls;
    }
    syncDecode();
    if (input.disabled === writable) {
      input.disabled = !writable;
      sendBtn.disabled = !writable;
      input.placeholder = writable
        ? '0x300#00 · 18DAF110#02 10 03 · fd 0x123#…'
        : !bound ? '채널을 연결하거나 선택하면 송신할 수 있습니다' : hasRight ? '연결 STALLED — 쓰기 잠김' : '송신은 operate 권한 필요';
    }
    rxtx.setWritable(writable);
    if (perView) {
      perView.setWritable(writable);
      perView.setLatched(ctx.stopLatched);
      rxtx.setLatched(ctx.stopLatched);
      if (jobs.version !== jobsSeen) {
        jobsSeen = jobs.version;
        rxtx.periodicChanged();
        if (curView === 'rxtx') rxtx.render();
      }
      perView.render();
    }
    // Receive list redraw: every frame while cheap; once a draw costs > RXTX_SLOW_MS (large id
    // set under load) at most RXTX_RENDER_HZ. Aggregation itself is per event, in ingest.
    if (curView === 'rxtx' && (lastRxtxCost <= RXTX_SLOW_MS || t - lastRxtxDraw >= 1000 / RXTX_RENDER_HZ)) {
      lastRxtxDraw = t;
      rxtx.render();
      lastRxtxCost = now() - t;
    }
    setText(envBadge, ctx.env ? ctx.env.toUpperCase() : '');
    let lt: string;
    let lc: string;
    if (view.paused) {
      lt = `⏸ PAUSED ${((t - pausedAt) / 1000).toFixed(1)} s +${fmtN(model.framesIn - pausedFrames)}`;
      lc = 'ct-live paused';
    } else if (model.degraded) {
      lt = `LIVE-DEGRADED · 표시 ≤ ${fmtN(DISPLAY_BUDGET_ROWS_PER_S)}행/s`;
      lc = 'ct-live degraded';
    } else {
      lt = 'LIVE';
      lc = 'ct-live';
    }
    setText(live, lt);
    if (live.className !== lc) live.className = lc;
    const ar = model.agentRanges[model.agentRanges.length - 1];
    setText(agentInfo, ar ? `▌에이전트 읽음 seq ${ar.a}–${ar.b} · ${Math.max(0, Math.round((t - ar.atMs) / 1000))} s 전` : '');
    const parts = [
      `보이는 ${fmtN(view.listCount)} / 링 ${fmtN(model.count)} (상한 ${fmtN(model.capacity)})`,
      `수신 ${fmtN(Math.round(fpsIn))} fps · 화면 ${fmtN(Math.round(dispRate))} 행/s`,
    ];
    // genuine capacity: the bound stream is waiting for a server subscription slot (net mux parked it)
    const wait = subWaitOf(mirror, bound !== null);
    if (root.dataset.subscription !== wait) root.dataset.subscription = wait;
    if (wait === 'waiting') {
      parts.unshift('구독 대기 — 이 연결의 서버 구독 8개를 보이는 패널이 모두 쓰는 중 (다른 보기를 숨기거나 닫으면 자동 연결)');
      setText(live, '구독 대기');
      if (live.className !== 'ct-live degraded') live.className = 'ct-live degraded';
    }
    if (model.omitted) parts.push(`화면 생략 누적 ${fmtN(model.omitted)}`);
    if (model.evicted) parts.push(`보관 범위 밖 ${fmtN(model.evicted)} → 질의`);
    setText(status, parts.join(' · '));
    timeBox.hidden = timeMode !== 'rel';
    refBtn.disabled = view.selected < 0;
    setText(refLbl, refLabel(timeRef()));
    if (timeSel.value !== timeMode) timeSel.value = timeMode;
  };

  const offFrame = ctx.scheduler.onFrame(() => {
    view.render();
    updateStatus();
    persistTick();
    sampleLatency();
    // ingest work since the last frame counts toward the scheduler's load rule
    ctx.scheduler.reportWork(ingestMs);
    ingestMs = 0;
    // keep "N s 전" / PAUSED clocks moving at ≤ 1 Hz cost only while they are shown
    return lastSeq < 0n ? undefined : lastSeq;
  });
  const tick = setInterval(() => ctx.scheduler.invalidate(), 1_000);

  type Probe = Record<string, { stats(): CanTraceStats; model: CanTraceModel; view: TraceView; summary: RxSummary; txList: TxList }>;
  const g = globalThis as unknown as { __loomCan?: Probe };
  if (import.meta.env.DEV) (g.__loomCan ??= {})[ctx.panelId] = { stats, model, view, summary, txList };

  // catch up with anything already in the mirrors (shared subscription replay)
  consumeCan();
  consumeActivity();

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      persistTick();
      closeAbort?.abort();
      offCtl();
      clearInterval(tick);
      connect?.dispose();
      if (previewTimer !== null) cancel(previewTimer);
      previewAbort?.abort();
      offFrame();
      offCan();
      offAct();
      offRows();
      view.dispose();
      root.remove();
      if (g.__loomCan) delete g.__loomCan[ctx.panelId];
    },
  };
}

export const CAN_TRACE_MANIFEST: PanelManifest = {
  id: 'can.trace',
  propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP, ch: CHANNEL_PROP, rows: { type: 'integer', minimum: 1, maximum: 2_000_000 }, view: { type: 'string', enum: ['rxtx', 'trace'] } } },
  domain: 'can',
  title: 'CAN 트레이스',
  version: 1,
  milestone: 'M1',
  topics: [
    {
      key: 'can',
      // unbound panels never open this topic (ctx.store is not called); '' would match no channel anyway
      filter: (p) => ({ domains: ['can'], channels: [channelOf(p) ?? ''] }),
      snapshot: 'required',
      mirrorRows: MIRROR_ROWS,
    },
    {
      // agent-read ranges: live core.activity (the daemon publishes it as domain core). Contained in the
      // shell's single stream {domains:[core,test]} and live-only, so the net layer serves it from that
      // stream (narrow) and it never costs one of the connection's 8 server subscriptions.
      key: 'activity',
      filter: () => ({ domains: ['core'], kinds: ['core.activity'] }),
      snapshot: 'none',
    },
  ],
  // can.send is also a query, but only with dry_run:true (risk preview; SDK enforces it)
  // can.status: unbound module lists already-open channels (explicit button, read-only)
  // B2 (CAN-6): can.periodic.list + can.status are read once right before the close preview (no polling)
  // B5 (CAN-5): DB state/catalog reads; can.db.load is a read RPC (UI asks for operate as mistake prevention)
  queries: ['can.trace.query', 'can.send', 'can.status', 'can.periodic.list', 'can.db.info', 'can.db.catalog', 'can.db.load'],
  actions: [
    { id: 'send', rpc: 'can.send', class: 'request', right: 'operate', confirm: 'none', label: 'CAN 1회 송신' },
    // unbound module: explicit open of the selected backend (can_sim only); navigation never calls it
    { id: 'open', rpc: 'can.open', class: 'request', right: 'operate', confirm: 'none', label: 'CAN 채널 열기' },
    // B2 (CAN-6): bound panel closes the daemon channel (all users/agents); the panel shows the impact first
    {
      id: 'close',
      rpc: 'can.close',
      class: 'request',
      right: 'operate',
      confirm: 'card',
      label: 'CAN 채널 닫기 — 이 채널의 주기 송신·ISO-TP·버스트가 모든 사용자·에이전트에서 멈춤',
    },
    // B4 (CAN-4): journal → BLF/ASC file on the daemon machine (read; same class/right as eth.export)
    { id: 'export', rpc: 'can.export', class: 'job', right: 'watch', confirm: 'none', label: 'CAN 내보내기 (BLF/ASC, 데몬 쪽 파일)' },
    // B3 (CAN-2): server periodic jobs (no browser timers). Stop is safety-reducing; takeover asks a card.
    { id: 'periodicStart', rpc: 'can.periodic.start', class: 'job', right: 'operate', confirm: 'none', label: 'CAN 주기 송신 시작 (데몬 작업)' },
    { id: 'periodicStop', rpc: 'can.periodic.stop', class: 'job', right: 'operate', confirm: 'none', safetyDirection: 'reduce', label: 'CAN 주기 송신 정지' },
    {
      id: 'periodicTakeover',
      rpc: 'can.periodic.takeover',
      class: 'job',
      right: 'operate',
      confirm: 'card',
      label: 'CAN 주기 송신 작업 넘겨받기 — 원래 소유자는 이 작업을 더 조작할 수 없음',
    },
  ],
  // ring at the 2M upper bound + mirror; default 100k rows uses ~1/20 of this
  memory: { maxBytes: Math.ceil(bytesForRows(ROWS_MAX) + 16 * 1024 * 1024) },
  render: { maxHz: 60, budgetMs: 8, lagTargetMs: 150 },
};

let registered = false;
/** Registers the CAN panels once (idempotent — safe if both panels/index.ts and a test entry call it). */
export function registerCanTrace(): void {
  if (registered) return;
  registered = true;
  registerPanel(CAN_TRACE_MANIFEST, (ctx) => createCanTrace(ctx));
}
