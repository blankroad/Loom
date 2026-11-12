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
} from '../../sdk';
import './can.css';
import { formatFrame, parseSendLine } from './sendLine';
import {
  bytesForRows,
  CanTraceModel,
  DISPLAY_BUDGET_ROWS_PER_S,
  dlcForLen,
  F_HIDDEN,
  K_FRAME,
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

export function channelOf(props: Readonly<Record<string, unknown>>): string {
  const c = props.channel ?? props.ch;
  return typeof c === 'string' && c ? c : 'ch0';
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

/** "0x300, 301 18DAF110" → id set; null = all; throws a message on bad input */
export function parseIdFilter(s: string): Set<number> | null {
  const parts = s.split(/[\s,]+/).filter(Boolean);
  if (parts.length === 0) return null;
  const out = new Set<number>();
  for (const p of parts) {
    const d = p.replace(/^0x/i, '').replace(/x$/i, '');
    if (!/^[0-9a-fA-F]{1,8}$/.test(d)) throw new Error(`id "${p}"`);
    out.add(parseInt(d, 16));
    if (out.size > 256) throw new Error('id ≤ 256개');
  }
  return out;
}

export function createCanTrace(ctx: PanelContext, opts: CanTraceOptions = {}): PanelInstance {
  const channel = channelOf(ctx.props);
  const now = opts.now ?? (() => performance.now());
  const model = new CanTraceModel({ capacity: rowsOf(ctx.props), now, recordOk: () => ctx.recordHealth === 'ok' });
  const mirror = ctx.store('can');
  const actMirror = ctx.store('activity');
  const rowView = ctx.rows({ domain: 'can', channel });
  const mine = new Set<string>(); // FIFO ≤ MY_REQ_MAX
  const isMine = (rid: string) => mine.has(rid);

  // ---------------- DOM ----------------
  const root = el('div', 'can-trace');
  root.dataset.testid = 'can-trace';
  root.dataset.channel = channel;
  const top = el('div', 'ct-bar-top');
  const chLabel = el('span', 'mono', `CAN ${channel}`);
  const envBadge = el('span', 'muted');
  const live = el('span', 'ct-live');
  live.dataset.testid = 'can-live';
  const pauseBtn = el('button', undefined, '⏸ 일시정지');
  pauseBtn.type = 'button';
  pauseBtn.title = '렌더만 정지 (수집 계속) — Space';
  pauseBtn.dataset.testid = 'can-pause';
  const idFilter = el('input', 'ct-filter-ids');
  idFilter.placeholder = 'id 필터 0x300,0x301';
  idFilter.setAttribute('aria-label', 'id 필터 (쉼표로 구분, Enter)');
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
  top.append(chLabel, envBadge, tabs, live, pauseBtn, idFilter, dirSel, orgWrap, agentInfo);

  const tableHost = el('div', 'ct-host');
  tableHost.style.flex = '1';
  tableHost.style.minHeight = '0';
  tableHost.style.display = 'flex';
  const view = new TraceView(tableHost, model, {
    onNeedRender: () => ctx.scheduler.invalidate(),
    rowState: (rid) => (rowView.get(rid)?.state as WState | undefined) ?? null,
    now,
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
  let curIds: ReadonlySet<number> | null = null;
  const rxtx = new RxTxView(summary, txList, {
    channel,
    idFilter: () => curIds,
    sendEntry: (uid) => sendEntry(uid),
    openTrace: (r) => {
      idFilter.value = r.ext ? `${r.id.toString(16).toUpperCase().padStart(8, '0')}x` : `0x${r.id.toString(16).toUpperCase().padStart(3, '0')}`;
      applyFilter();
      setView('trace');
    },
    draftChanged: (p) => schedulePreview(p),
    now,
  });
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

  root.append(createLatchLine(ctx), top, tableHost, rxtx.root, send, status);
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
    for (const ev of evs) {
      applyOne(ev);
      lastSeq = BigInt(ev.seq);
      lastEpoch = ev.epoch;
    }
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
    let ids: Set<number> | null;
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
    if (!e) return;
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
    if (!params) {
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
    if (!input.value.trim()) {
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
    const writable = hasRight && !ctx.writeLocked;
    if (input.disabled === writable) {
      input.disabled = !writable;
      sendBtn.disabled = !writable;
      input.placeholder = writable ? '0x300#00 · 18DAF110#02 10 03 · fd 0x123#…' : hasRight ? '연결 STALLED — 쓰기 잠김' : '송신은 operate 권한 필요';
    }
    rxtx.setWritable(writable);
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
    if (model.omitted) parts.push(`화면 생략 누적 ${fmtN(model.omitted)}`);
    if (model.evicted) parts.push(`보관 범위 밖 ${fmtN(model.evicted)} → 질의`);
    setText(status, parts.join(' · '));
  };

  const offFrame = ctx.scheduler.onFrame(() => {
    view.render();
    updateStatus();
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
      clearInterval(tick);
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
      filter: (p) => ({ domains: ['can'], channels: [channelOf(p)] }),
      snapshot: 'required',
      mirrorRows: MIRROR_ROWS,
    },
    {
      // identical (filter, options) to the shell's Activity subscription → the net layer
      // shares ONE server subscription (contract v1: ≤ 8 per viewer connection)
      key: 'activity',
      filter: () => ({ kinds: ['core.activity'] }),
      snapshot: 'required',
    },
  ],
  // can.send is also a query, but only with dry_run:true (risk preview; SDK enforces it)
  queries: ['can.trace.query', 'can.send'],
  actions: [{ id: 'send', rpc: 'can.send', class: 'request', right: 'operate', confirm: 'none', label: 'CAN 1회 송신' }],
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
