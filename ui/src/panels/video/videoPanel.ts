// video.workbench — Loom-native video capture panel (CONTRACT: work/reports/video-capture/CONTRACT.md).
//
//   status   capture · state · source (label only, no query) · size@fps · STOP latch · [■ 닫기] (safety, watch ok)
//   preview  canvas fed by ctx.media(GET /api/v1/video/{id}/frame) → SHA-256 of the received bytes in the browser →
//            createImageBitmap → drawn. The *drawn* frame id + computed hash are shown (not <img> headers).
//            [고정] freezes the drawn frame (id + hash stay visible for agent comparison via `loom video frame --id`).
//   health   frames · measured/requested fps · render fps · last receive age · stale · dup/drop · oversize/corrupt
//   actions  snapshot (drawn/frozen frame) · record start/stop · open form (explicit source; avfoundation unverified)
// Lifecycle/health/snapshot/record are metadata events (journal); frames never come through the event stream.

import { CHANNEL_PROP, registerPanel, type LoomEvent, type PanelContext, type PanelInstance } from '../../sdk';
import {
  artifactFileName,
  artifactUrl,
  cliLine,
  DEFAULT_FORM,
  fmtBytes,
  fmtWallNs,
  formFromCapture,
  frameUrl,
  healthLine,
  hex,
  LIVE_STATES,
  openParams,
  PIN_TTL_MS,
  pinText,
  pinTick,
  RateMeter,
  readPinError,
  readPinResult,
  readSources,
  REASON_LABEL,
  ringStep,
  snapshotBlock,
  SOURCES,
  sourceListText,
  STATE_LABEL,
  validateForm,
  VIDEO_KINDS,
  VideoBook,
  wide,
  type OpenForm,
  type PinView,
  type SnapshotView,
  type SourceKind,
  type SourceList,
} from './videoModel';
import {
  b64Bytes,
  evidenceText,
  historyText,
  inspectLine,
  METRIC_LABEL,
  METRICS,
  readEvidenceManifest,
  ROI_HISTORY_MAX,
  ROI_KINDS,
  RoiBook,
  roiFormSpec,
  type EvidenceManifest,
  type RoiForm,
} from './roiView';
import './video.css';

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const tid = <T extends HTMLElement>(e: T, id: string): T => {
  e.setAttribute('data-testid', id);
  return e;
};
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const seqOf = (s: string): bigint => (/^\d{1,20}$/.test(s) ? BigInt(s) : -1n);
const FETCH_TIMEOUT_MS = 3000;

interface Drawn {
  captureId: string;
  id: number;
  epoch: string;
  sha: string;
  headerSha: string;
  recvUtcNs: string;
  drawnAtMs: number;
  bytes: number;
}

/** video.capabilities → one truthful line: FFmpeg presence + implemented / unverified / unsupported sources */
export function capabilityText(r: unknown): string {
  const d = obj(obj(r).data ?? r);
  const ff = obj(d.ffmpeg);
  const engine = typeof ff.path === 'string' && ff.path
    ? `FFmpeg ${ff.path}${typeof ff.version === 'string' && ff.version ? ` (${ff.version})` : ''}${typeof ff.error === 'string' && ff.error ? ` · 오류 ${ff.error}` : ''}`
    : `FFmpeg 없음${typeof ff.error === 'string' && ff.error ? ` (${ff.error})` : ''} — 캡처 불가`;
  const src = obj(d.sources);
  const parts = Object.entries(src).map(([k, v]) => {
    const o = obj(v);
    if (o.implemented !== true) return `${k} 미지원${typeof o.reason === 'string' ? `(${o.reason})` : ''}`;
    return o.device_verified === false ? `${k} 구현·실물 미검증` : `${k} 구현`;
  });
  const audio = obj(d.audio).available === true ? '오디오 가능' : '오디오 미지원';
  return [engine, parts.join(', ') || '소스 정보 없음', audio].join(' · ');
}

let panelSeq = 0;

/** Append an option unless one with `value` already exists (never removes). */
function ensureOption(sel: HTMLSelectElement, value: string, label: string): void {
  for (const o of Array.from(sel.options)) if (o.value === value) return;
  const o = el('option', undefined, label);
  o.value = value;
  sel.append(o);
}

export function createVideoWorkbench(ctx: PanelContext): PanelInstance {
  const pinned = typeof ctx.props.channel === 'string' ? ctx.props.channel : null;
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  const mirror = ctx.store('video');
  const book = new VideoBook();
  let lastSeq = -1n;
  let snapshotVersion = -1;
  let form: OpenForm = { ...DEFAULT_FORM };
  let drawn: Drawn | null = null;
  let frozen: Drawn | null = null;
  let pending: { bmp: ImageBitmap; meta: Drawn } | null = null;
  let inflight = false;
  let fetchErr: string | null = null;
  let sameSkipped = 0;
  let fetched = 0;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let lastFetchedId = -1;
  let lastFetchedCapture = '';
  let finalFetchDone = '';
  let openErr: string | null = null;
  const render = new RateMeter();
  const listN = ++panelSeq;
  let reuseNoteText: string | null = null;
  let sources: SourceList | null = null;
  let sourcesErr: string | null = null;
  let sourcesPending = false;
  let replayRequested = false;
  type ViewState = { artifactId: string; frameId: number; expectedSha: string; sha: string; ok: boolean };
  let viewing: ViewState | null = null;
  let pendingView: { bmp: ImageBitmap; view: ViewState } | null = null;
  let viewPending = false;
  let snapsKey = '';
  const roiBook = new RoiBook();
  let evid: { m: EvidenceManifest; i: number; shas: string[]; oks: boolean[] } | null = null;
  let roiRowsKey = '';
  let roiOverlayKey = '';
  let roiHistKey = '';
  let evidKey = '';
  let pin: PinView | null = null;
  let navBusy = false;
  let pendingNav: { bmp: ImageBitmap } | null = null;

  // ---- DOM ----
  const root = el('div', 'vd-panel');
  const status = tid(el('div', 'vd-status'), 'video-status');
  const stateChip = tid(el('span', 'vd-chip'), 'video-state');
  const srcEl = tid(el('span', 'vd-src'), 'video-source');
  const closeBtn = tid(el('button', 'vd-stop', '■ 캡처 닫기'), 'video-close');
  closeBtn.type = 'button';
  closeBtn.title = '수집 정지 + FFmpeg 정리 (안전 방향, 보기 권한 가능)';
  const reuseBtn = tid(el('button', 'vd-reuse', '↺ 같은 설정으로 다시 열기'), 'video-reuse');
  reuseBtn.type = 'button';
  reuseBtn.title = '직전 캡처의 소스·크기·fps로 폼을 채웁니다 (열기는 직접 누름)';
  status.append(stateChip, srcEl, reuseBtn, closeBtn);
  const latchEl = tid(el('div', 'vd-latch'), 'video-latch');
  const errEl = tid(el('div', 'vd-error'), 'video-error');

  const stage = el('div', 'vd-stage');
  const canvas = tid(el('canvas', 'vd-canvas'), 'video-canvas');
  canvas.width = 1280;
  canvas.height = 720;
  const overlay = tid(el('div', 'vd-overlay'), 'video-overlay');
  stage.append(canvas, overlay);

  const ident = tid(el('div', 'vd-ident'), 'video-ident');
  const frameIdEl = tid(el('span', 'vd-mono'), 'video-frame-id');
  const shaEl = tid(el('span', 'vd-mono vd-sha'), 'video-frame-sha256');
  const frozenEl = tid(el('span', 'vd-frozen'), 'video-frozen');
  const pinEl = tid(el('span', 'vd-pin'), 'video-pin');
  ident.append(frameIdEl, shaEl, frozenEl, pinEl);

  const healthEl = tid(el('div', 'vd-health'), 'video-health');
  const tools = el('div', 'vd-tools');
  const freezeBtn = tid(el('button', undefined, '⏸ 고정'), 'video-freeze');
  const snapBtn = tid(el('button', undefined, '📷 스냅샷'), 'video-snapshot-btn');
  const recBtn = tid(el('button', undefined, '● 녹화 시작'), 'video-record-btn');
  for (const b of [freezeBtn, snapBtn, recBtn]) b.type = 'button';
  const prevBtn = tid(el('button', 'vd-ring-nav', '◀'), 'video-ring-prev');
  prevBtn.title = '이전 frame (링/보존)';
  const nextBtn = tid(el('button', 'vd-ring-nav', '▶'), 'video-ring-next');
  nextBtn.title = '다음 frame (링)';
  for (const b of [prevBtn, nextBtn]) b.type = 'button';
  tools.append(freezeBtn, prevBtn, nextBtn, snapBtn, recBtn);
  const recEl = tid(el('div', 'vd-record'), 'video-record');
  const snapsEl = tid(el('div', 'vd-snaps'), 'video-snapshots');
  const audioEl = tid(el('div', 'vd-note', '오디오: 미지원 (video-only) · RTSP/DeckLink: 미지원 · 캡처보드/TCC: 미검증 · Windows: 캡처 미지원 (데몬이 video.open을 E_UNSUPPORTED로 거부)'), 'video-limits');

  // open form
  const cfgBox = tid(el('details', 'vd-config'), 'video-config');
  cfgBox.append(el('summary', undefined, '새 캡처 열기 (명시적 소스)'));
  const grid = el('div', 'vd-cfg-grid');
  const srcSel = el('select');
  srcSel.name = 'source';
  for (const s of SOURCES) {
    const o = el('option', undefined, s === 'avfoundation' ? 'avfoundation (실물 미검증)' : s);
    o.value = s;
    srcSel.append(o);
  }
  const urlIn = el('input');
  urlIn.name = 'url';
  const nameIn = el('input');
  nameIn.name = 'name';
  nameIn.placeholder = 'clip.mp4';
  const devIn = el('input');
  devIn.name = 'device';
  devIn.placeholder = '정확한 장치 이름';
  const sizeSel = el('select');
  sizeSel.name = 'size';
  for (const [w, h] of [[640, 360], [1280, 720], [1920, 1080]] as const) {
    const o = el('option', undefined, `${w}×${h}`);
    o.value = `${w}x${h}`;
    sizeSel.append(o);
  }
  const fpsSel = el('select');
  fpsSel.name = 'fps';
  for (const f of [15, 30, 60]) {
    const o = el('option', undefined, `${f} fps`);
    o.value = String(f);
    fpsSel.append(o);
  }
  const lab = (t: string, i: HTMLElement) => {
    const l = el('label', 'vd-field');
    l.append(el('span', undefined, t), i);
    return l;
  };
  const urlL = lab('URL', urlIn);
  const nameL = lab('replay 파일', nameIn);
  const devL = lab('장치', devIn);
  const devLoadBtn = tid(el('button', undefined, '장치 목록 불러오기'), 'video-device-load');
  devLoadBtn.type = 'button';
  devLoadBtn.title = 'video.sources --local: AVFoundation 장치 열거 — macOS 카메라 권한 요청이 뜰 수 있음 (장치는 열지 않음)';
  const replayList = tid(el('datalist'), 'video-replay-list');
  replayList.id = `vd-replay-${listN}`;
  nameIn.setAttribute('list', replayList.id);
  const deviceList = tid(el('datalist'), 'video-device-list');
  deviceList.id = `vd-device-${listN}`;
  devIn.setAttribute('list', deviceList.id);
  const reuseNote = tid(el('div', 'vd-note'), 'video-reuse-note');
  const srcListEl = tid(el('span', 'vd-note'), 'video-source-list');
  const openBtn = tid(el('button', 'vd-open', '열기'), 'video-open');
  openBtn.type = 'button';
  const formErr = tid(el('div', 'vd-form-err'), 'video-form-error');
  /** last video.open refusal from the daemon (e.g. E_UNSUPPORTED on Windows) — stays until the next open */
  const openErrEl = tid(el('div', 'vd-form-err'), 'video-open-error');
  openErrEl.setAttribute('role', 'status');
  const cli = tid(el('code', 'vd-cli'), 'video-cli');
  const capRow = el('div', 'vd-caprow');
  const capBtn = tid(el('button', undefined, '엔진 확인'), 'video-engine-check');
  capBtn.type = 'button';
  capBtn.title = 'video.capabilities (읽기 전용) — FFmpeg 경로/버전과 구현·검증된 소스를 확인합니다. 장치·카메라를 열지 않습니다.';
  const capEl = tid(el('span', 'vd-note'), 'video-capability');
  capRow.append(capBtn, capEl);
  grid.append(lab('소스', srcSel), urlL, nameL, devL, devLoadBtn, lab('크기', sizeSel), lab('fps', fpsSel), openBtn);
  cfgBox.append(grid, formErr, reuseNote, openErrEl, cli, capRow, srcListEl, replayList, deviceList);
  // module tab (no pinned capture): the explicit source form is visible before anything is opened (UI only)
  if (!pinned) cfgBox.open = true;

  // ---- ROI inspection (B6) ----
  const SVGNS = 'http://www.w3.org/2000/svg';
  const roiSvg = document.createElementNS(SVGNS, 'svg');
  roiSvg.setAttribute('class', 'vd-roi-overlay');
  roiSvg.setAttribute('data-testid', 'video-roi-overlay');
  roiSvg.setAttribute('viewBox', '0 0 1 1');
  roiSvg.setAttribute('preserveAspectRatio', 'none');
  stage.append(roiSvg);
  const roiBox = tid(el('div', 'vd-roi'), 'video-roi');
  const roiMeta = tid(el('div', 'vd-roi-meta'), 'video-roi-meta');
  const roiList = tid(el('div', 'vd-roi-list'), 'video-roi-list');
  const roiEditor = tid(el('div', 'vd-roi-editor'), 'video-roi-editor');
  const roiName = el('input');
  roiName.name = 'roi-name';
  const roiRect = el('input');
  roiRect.name = 'roi-rect';
  roiRect.placeholder = 'x,y,w,h';
  const roiMetric = el('select');
  roiMetric.name = 'roi-metric';
  for (const m of METRICS) {
    const o = el('option', undefined, METRIC_LABEL[m] ?? m);
    o.value = m;
    roiMetric.append(o);
  }
  roiMetric.value = 'mean_abs_diff';
  const roiEnter = el('input');
  roiEnter.name = 'roi-enter';
  const roiExit = el('input');
  roiExit.name = 'roi-exit';
  const roiDwell = el('input');
  roiDwell.name = 'roi-dwell';
  const roiCooldown = el('input');
  roiCooldown.name = 'roi-cooldown';
  const putBtn = tid(el('button', undefined, 'ROI 저장'), 'video-roi-put');
  const baseBtn = tid(el('button', undefined, '현재 frame을 기준으로'), 'video-roi-baseline');
  for (const b of [putBtn, baseBtn]) b.type = 'button';
  const roiErr = tid(el('div', 'vd-roi-err'), 'video-roi-err');
  roiEditor.append(
    lab('이름', roiName), lab('영역', roiRect), lab('측정', roiMetric), lab('진입', roiEnter),
    lab('해제', roiExit), lab('대기(ms)', roiDwell), lab('재전환(ms)', roiCooldown), putBtn, baseBtn, roiErr,
  );
  const roiHist = tid(el('div', 'vd-roi-history'), 'video-roi-history');
  const evidBox = tid(el('div', 'vd-evidence'), 'video-evidence');
  const evidNav = tid(el('div', 'vd-evid-nav'), 'video-evid-nav');
  evidNav.hidden = true;
  const evidPrev = tid(el('button', undefined, '◀'), 'video-evid-prev');
  const evidLabel = tid(el('span', 'vd-evid-label'), 'video-evid-label');
  const evidNext = tid(el('button', undefined, '▶'), 'video-evid-next');
  for (const b of [evidPrev, evidNext]) b.type = 'button';
  evidNav.append(evidPrev, evidLabel, evidNext);
  roiBox.append(roiMeta, roiList, roiEditor, roiHist, evidBox, evidNav);

  root.append(status, latchEl, errEl, stage, ident, tools, healthEl, recEl, snapsEl, roiBox, audioEl, cfgBox);

  const syncForm = () => {
    urlIn.value = form.url;
    nameIn.value = form.name;
    devIn.value = form.device;
    srcSel.value = form.source;
    sizeSel.value = `${form.width}x${form.height}`;
    fpsSel.value = String(form.fps);
  };
  syncForm();
  const readForm = () => {
    const [w, h] = sizeSel.value.split('x').map(Number);
    form = { source: srcSel.value as SourceKind, url: urlIn.value.trim(), name: nameIn.value.trim(), device: devIn.value, width: w ?? 1280, height: h ?? 720, fps: Number(fpsSel.value) };
    ctx.scheduler.invalidate();
  };
  for (const i of [srcSel, urlIn, nameIn, devIn, sizeSel, fpsSel]) i.addEventListener('input', readForm);

  // ---- data ----
  const current = () => {
    const id = pinned ?? book.newestId;
    return id ? (book.captures.get(id) ?? null) : null;
  };
  const handle = (ev: LoomEvent) => {
    if ((ROI_KINDS as readonly string[]).includes(ev.kind)) {
      const rid = obj(ev.payload).capture_id;
      if (pinned && typeof rid === 'string' && rid !== pinned) return;
      if (roiBook.apply(ev.kind, ev.payload, seqOf(String(ev.seq)))) ctx.scheduler.invalidate();
      return;
    }
    const id = obj(ev.payload).capture_id;
    if (pinned && id !== pinned) return;
    if (book.apply(ev.kind, ev.payload, seqOf(String(ev.seq)), ev.ts?.wall_ns ?? null)) ctx.scheduler.invalidate();
  };
  const applySnap = () => {
    const snap = mirror.snapshot;
    if (!snap || snap.version === snapshotVersion) return;
    snapshotVersion = snap.version;
    for (const item of snap.items) {
      book.applySnapshotItem(item, snap.throughSeq);
      if (obj(item).domain === 'video') roiBook.applySnapshotState(obj(item).state, snap.throughSeq);
    }
  };
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const offControl = ctx.onControl(handle);
  const offLatch = ctx.latch.subscribe(() => ctx.scheduler.invalidate());

  // ---- frame pull (bounded: one request in flight, paced to the capture fps) ----
  const schedule = (ms: number) => {
    if (disposed) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void pull(), Math.max(0, ms));
  };
  const pull = async () => {
    timer = null;
    const c = current();
    const live = !!c && LIVE_STATES.has(c.state);
    const period = 1000 / Math.max(1, Math.min(60, c?.fps ?? 30));
    // after the capture ends, fetch its last (frozen) frame once so the stop frame is visible
    const finalFetch = !!c && !live && finalFetchDone !== c.id;
    if (!c || frozen || viewing || pendingView || inflight || (!live && !finalFetch)) {
      if (frozen && pin?.status === 'pinned') ctx.scheduler.invalidate();
      schedule(250);
      return;
    }
    inflight = true;
    const t0 = performance.now();
    const ac = new AbortController();
    const to = setTimeout(() => ac.abort(), FETCH_TIMEOUT_MS);
    try {
      // read-only media plane through the SDK (declared prefix, same-origin GET, no-store, redirect error)
      const r = await ctx.media(frameUrl(c.id), ac.signal);
      if (!r.ok) {
        const body = obj(await r.json().catch(() => ({})));
        fetchErr = `프레임 ${r.status} ${String(body.error ?? '')}`;
        if (finalFetch) finalFetchDone = c.id;
        return;
      }
      const id = Number(r.headers.get('x-loom-frame-id') ?? NaN);
      if (id === lastFetchedId && c.id === lastFetchedCapture) {
        sameSkipped++;
        await r.arrayBuffer().catch(() => undefined);
        if (finalFetch) finalFetchDone = c.id;
        return;
      }
      const buf = await r.arrayBuffer();
      const sha = hex(await crypto.subtle.digest('SHA-256', buf));
      const bmp = await createImageBitmap(new Blob([buf], { type: 'image/jpeg' }));
      pending?.bmp.close();
      pending = {
        bmp,
        meta: {
          captureId: c.id,
          id,
          epoch: wide(r.headers.get('x-loom-capture-epoch')) ?? '',
          sha,
          headerSha: r.headers.get('x-loom-frame-sha256') ?? '',
          recvUtcNs: wide(r.headers.get('x-loom-recv-utc-ns')) ?? '',
          drawnAtMs: 0,
          bytes: buf.byteLength,
        },
      };
      lastFetchedId = id;
      lastFetchedCapture = c.id;
      fetched++;
      fetchErr = null;
      if (finalFetch) finalFetchDone = c.id;
      ctx.scheduler.invalidate();
    } catch (e) {
      fetchErr = `프레임 가져오기 실패: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      clearTimeout(to);
      inflight = false;
      schedule(period - (performance.now() - t0));
    }
  };
  schedule(0);

  // ---- reuse / sources / artifact viewer ----
  const reuseSettings = () => {
    const c = current();
    if (!c || LIVE_STATES.has(c.state)) return;
    const r = formFromCapture(c, form);
    if (!r) return;
    form = r.form;
    reuseNoteText = r.note;
    ensureOption(sizeSel, `${form.width}x${form.height}`, `${form.width}×${form.height}`);
    ensureOption(fpsSel, String(form.fps), `${form.fps} fps`);
    syncForm();
    cfgBox.open = true;
    ctx.scheduler.invalidate();
  };
  reuseBtn.addEventListener('click', reuseSettings);

  const loadSources = async (local: boolean): Promise<void> => {
    if (sourcesPending) return;
    sourcesPending = true;
    devLoadBtn.disabled = true;
    try {
      const r = await ctx.query<unknown>('video.sources', { local });
      if (disposed) return;
      const s = readSources(r);
      sources = { replay: s.replay, local: s.local.enumerated ? s.local : (sources?.local ?? s.local) };
      sourcesErr = null;
      replayList.replaceChildren(
        ...sources.replay.map((n) => {
          const o = el('option');
          o.value = n;
          return o;
        }),
      );
      if (s.local.enumerated) {
        deviceList.replaceChildren(
          ...sources.local.devices.map((n) => {
            const o = el('option');
            o.value = n;
            return o;
          }),
        );
      }
    } catch (e) {
      sourcesErr = e instanceof Error ? e.message : String(e);
    } finally {
      sourcesPending = false;
      if (!disposed) {
        devLoadBtn.disabled = false;
        ctx.scheduler.invalidate();
      }
    }
  };
  const onSourceChange = () => {
    if (srcSel.value === 'replay' && !replayRequested) {
      replayRequested = true;
      void loadSources(false);
    }
  };
  srcSel.addEventListener('change', onSourceChange);
  devLoadBtn.addEventListener('click', () => void loadSources(true));

  const leaveArtifactView = () => {
    viewing = null;
    evid = null;
    pendingView?.bmp.close();
    pendingView = null;
    if (!frozen) schedule(0);
    ctx.scheduler.invalidate();
  };
  const viewArtifact = async (s: SnapshotView): Promise<void> => {
    if (viewPending || disposed) return;
    viewPending = true;
    const ac = new AbortController();
    const to = setTimeout(() => ac.abort(), FETCH_TIMEOUT_MS);
    try {
      const r = await ctx.media(artifactUrl(s.artifactId), ac.signal);
      if (!r.ok) {
        const body = obj(await r.json().catch(() => ({})));
        ctx.ui.toast(`아티팩트 보기 실패: ${r.status} ${String(body.error ?? '')}`.trim());
        return;
      }
      const ct = r.headers.get('content-type') ?? '';
      if (!ct.startsWith('image/jpeg')) {
        ctx.ui.toast(`아티팩트 보기 실패: 형식 ${ct || '없음'}`);
        return;
      }
      const buf = await r.arrayBuffer();
      const sha = hex(await crypto.subtle.digest('SHA-256', buf));
      const bmp = await createImageBitmap(new Blob([buf], { type: 'image/jpeg' }));
      if (disposed) {
        bmp.close();
        return;
      }
      pendingView?.bmp.close();
      pendingView = { bmp, view: { artifactId: s.artifactId, frameId: s.frameId, expectedSha: s.sha256, sha, ok: sha === s.sha256 } };
    } catch (e) {
      ctx.ui.toast(`아티팩트 보기 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      clearTimeout(to);
      viewPending = false;
      if (!disposed) ctx.scheduler.invalidate();
    }
  };

  const showEvid = async (i: number): Promise<void> => {
    const e = evid;
    const fr = e?.m.frames[i];
    if (!e || !fr) return;
    const bytes = b64Bytes(fr.b64);
    if (!bytes) {
      ctx.ui.toast('증거 보기 실패: base64 오류');
      return;
    }
    e.i = i;
    try {
      const bmp = await createImageBitmap(new Blob([bytes as BlobPart], { type: 'image/jpeg' }));
      if (disposed || evid !== e || e.i !== i) {
        bmp.close();
        return;
      }
      pendingView?.bmp.close();
      pendingView = {
        bmp,
        view: { artifactId: e.m.artifactId, frameId: fr.frameId, expectedSha: fr.sha256, sha: e.shas[i] ?? '', ok: e.oks[i] === true },
      };
    } catch (err) {
      ctx.ui.toast(`증거 보기 실패: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      if (!disposed) ctx.scheduler.invalidate();
    }
  };
  const viewEvidence = async (aid: string): Promise<void> => {
    if (viewPending || disposed) return;
    viewPending = true;
    const ac = new AbortController();
    const to = setTimeout(() => ac.abort(), FETCH_TIMEOUT_MS);
    let opened: { m: EvidenceManifest; i: number; shas: string[]; oks: boolean[] } | null = null;
    try {
      const r = await ctx.media(artifactUrl(aid), ac.signal);
      if (!r.ok) {
        const body = obj(await r.json().catch(() => ({})));
        ctx.ui.toast(`증거 보기 실패: ${r.status} ${String(body.error ?? '')}`.trim());
        return;
      }
      const m = readEvidenceManifest(await r.json().catch(() => null));
      if (!m) {
        ctx.ui.toast('증거 보기 실패: 형식 오류');
        return;
      }
      const shas: string[] = [];
      const oks: boolean[] = [];
      for (const f of m.frames) {
        const bytes = b64Bytes(f.b64);
        const sha = bytes ? hex(await crypto.subtle.digest('SHA-256', bytes as BufferSource)) : '';
        shas.push(sha);
        oks.push(!!bytes && sha === f.sha256 && bytes.length === f.size);
      }
      const t = m.frames.findIndex((f) => f.role === 'trigger');
      opened = { m, i: t < 0 ? 0 : t, shas, oks };
    } catch (e) {
      ctx.ui.toast(`증거 보기 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      clearTimeout(to);
      viewPending = false;
      if (!disposed) ctx.scheduler.invalidate();
    }
    if (opened && !disposed) {
      evid = opened;
      await showEvid(opened.i);
    }
  };
  evidPrev.addEventListener('click', () => {
    if (evid && evid.i > 0) void showEvid(evid.i - 1);
  });
  evidNext.addEventListener('click', () => {
    if (evid && evid.i < evid.m.frames.length - 1) void showEvid(evid.i + 1);
  });

  // ---- actions ----
  const act = (id: string, params: object, what: string) =>
    ctx
      .act(id, params)
      .then(() => ctx.ui.toast(`${what} 요청됨`))
      .catch((e: unknown) => ctx.ui.toast(`${what} 실패: ${e instanceof Error ? e.message : String(e)}`))
      .finally(() => ctx.scheduler.invalidate());
  closeBtn.addEventListener('click', () => {
    const c = current();
    if (c) void act('close', { capture_id: c.id }, '캡처 닫기');
  });
  const unpinFrame = (captureId: string, frameId: number): void => {
    ctx
      .act('unpin', { capture_id: captureId, frame_id: frameId })
      .catch((e: unknown) => ctx.ui.toast(`보존 해제 실패: ${e instanceof Error ? e.message : String(e)}`));
  };
  const pinFrame = (f: Drawn): void => {
    const mine: PinView = { captureId: f.captureId, frameId: f.id, status: 'pending', expiresAtMs: null, ttlMs: null, reason: null, sha256: null };
    pin = mine;
    ctx.scheduler.invalidate();
    ctx
      .act('pin', { capture_id: f.captureId, frame_id: f.id, ttl_ms: PIN_TTL_MS })
      .then((handle) => {
        if (pin === mine) pin = readPinResult(handle.result, f.captureId, f.id, Date.now());
        else unpinFrame(f.captureId, f.id);
      })
      .catch((e: unknown) => {
        if (pin === mine) pin = readPinError(e, f.captureId, f.id);
      })
      .finally(() => {
        if (!disposed) ctx.scheduler.invalidate();
      });
  };
  const releasePin = (): void => {
    if (pin?.status === 'pinned') unpinFrame(pin.captureId, pin.frameId);
    pin = null;
  };
  freezeBtn.addEventListener('click', () => {
    if (viewing || pendingView) {
      leaveArtifactView();
      return;
    }
    if (frozen) {
      releasePin();
      frozen = null;
      pendingNav?.bmp.close();
      pendingNav = null;
      schedule(0);
    } else {
      frozen = drawn;
      if (drawn) pinFrame(drawn);
    }
    ctx.scheduler.invalidate();
  });
  const navTo = async (dir: -1 | 1): Promise<void> => {
    const c = current();
    if (!frozen || navBusy || viewing || !c || frozen.captureId !== c.id) return;
    const target = ringStep(frozen.id, dir, book.health.get(c.id)?.lastFrameId ?? null);
    if (target === null) return;
    navBusy = true;
    const ac = new AbortController();
    const to = setTimeout(() => ac.abort(), FETCH_TIMEOUT_MS);
    try {
      const r = await ctx.media(frameUrl(c.id, target), ac.signal);
      if (!r.ok) {
        const body = obj(await r.json().catch(() => ({})));
        fetchErr = `frame ${target} 없음 (${r.status} ${String(body.error ?? '')})`;
        return;
      }
      const id = Number(r.headers.get('x-loom-frame-id') ?? NaN);
      if (id !== target) {
        fetchErr = `frame ${target} 요청에 ${id} 수신`;
        return;
      }
      const buf = await r.arrayBuffer();
      const sha = hex(await crypto.subtle.digest('SHA-256', buf));
      const bmp = await createImageBitmap(new Blob([buf], { type: 'image/jpeg' }));
      if (disposed) {
        bmp.close();
        return;
      }
      const meta: Drawn = {
        captureId: c.id,
        id,
        epoch: wide(r.headers.get('x-loom-capture-epoch')) ?? '',
        sha,
        headerSha: r.headers.get('x-loom-frame-sha256') ?? '',
        recvUtcNs: wide(r.headers.get('x-loom-recv-utc-ns')) ?? '',
        drawnAtMs: Date.now(),
        bytes: buf.byteLength,
      };
      pendingNav?.bmp.close();
      pendingNav = { bmp };
      const old = pin;
      frozen = meta;
      fetchErr = null;
      pinFrame(meta);
      if (old?.status === 'pinned') unpinFrame(old.captureId, old.frameId);
    } catch (e) {
      fetchErr = `frame ${target} 가져오기 실패: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      clearTimeout(to);
      navBusy = false;
      if (!disposed) ctx.scheduler.invalidate();
    }
  };
  prevBtn.addEventListener('click', () => void navTo(-1));
  nextBtn.addEventListener('click', () => void navTo(1));
  snapBtn.addEventListener('click', () => {
    const c = current();
    const f = frozen ?? drawn;
    if (c) void act('snapshot', f && f.captureId === c.id ? { capture_id: c.id, frame_id: f.id } : { capture_id: c.id }, '스냅샷');
  });
  putBtn.addEventListener('click', () => {
    const f: RoiForm = {
      name: roiName.value,
      rect: roiRect.value,
      metric: roiMetric.value,
      enter: roiEnter.value,
      exit: roiExit.value,
      dwell: roiDwell.value,
      cooldown: roiCooldown.value,
    };
    const r = roiFormSpec(f);
    if (typeof r === 'string') {
      roiErr.textContent = r;
      return;
    }
    roiErr.textContent = '';
    const cfg = roiBook.config;
    void act('roi_put', { ...r, ...(cfg ? { expect_rev: cfg.rev } : {}) }, 'ROI 저장');
  });
  baseBtn.addEventListener('click', () => {
    const c = current();
    const f = frozen ?? drawn;
    if (c && f) void act('roi_baseline', { capture_id: c.id, frame_id: f.id }, 'ROI 기준');
  });
  recBtn.addEventListener('click', () => {
    const c = current();
    if (!c) return;
    const rec = book.records.get(c.id);
    if (rec?.state === 'recording') void act('record_stop', { capture_id: c.id }, '녹화 정지');
    else void act('record_start', { capture_id: c.id }, '녹화 시작');
  });
  openBtn.addEventListener('click', () => {
    readForm();
    const err = validateForm(form);
    if (err) {
      formErr.textContent = err;
      return;
    }
    frozen = null;
    pin = null;
    openErr = null;
    reuseNoteText = null;
    ctx
      .act('open', openParams(form))
      .then(() => ctx.ui.toast('캡처 열기 요청됨'))
      .catch((e: unknown) => {
        const code = (e as { dataCode?: string }).dataCode ?? (e as { code?: string }).code;
        openErr = `열기 거부${code ? ` (${code})` : ''}: ${e instanceof Error ? e.message : String(e)}`;
        ctx.ui.toast(`캡처 열기 실패: ${e instanceof Error ? e.message : String(e)}`);
      })
      .finally(() => ctx.scheduler.invalidate());
  });
  // explicit engine check (never on mount / tab open)
  let capPending = false;
  capBtn.addEventListener('click', () => {
    if (capPending) return;
    capPending = true;
    capBtn.disabled = true;
    capEl.textContent = '확인 중…';
    ctx
      .query<unknown>('video.capabilities', {})
      .then((r) => {
        if (!disposed) capEl.textContent = capabilityText(r);
      })
      .catch((e: unknown) => {
        if (!disposed) capEl.textContent = `video.capabilities 실패: ${e instanceof Error ? e.message : String(e)}`;
      })
      .finally(() => {
        capPending = false;
        capBtn.disabled = false;
      });
  });

  // ---- draw ----
  const draw = (): bigint => {
    const t0 = performance.now();
    applySnap();
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = seqOf(String(ev.seq));
      handle(ev);
    }
    if (pendingView) {
      const g = canvas.getContext('2d');
      if (g) {
        if (canvas.width !== pendingView.bmp.width || canvas.height !== pendingView.bmp.height) {
          canvas.width = pendingView.bmp.width;
          canvas.height = pendingView.bmp.height;
        }
        g.drawImage(pendingView.bmp, 0, 0);
      }
      viewing = pendingView.view;
      pendingView.bmp.close();
      pendingView = null;
    }
    if (pendingNav && !viewing) {
      const g = canvas.getContext('2d');
      if (g) {
        if (canvas.width !== pendingNav.bmp.width || canvas.height !== pendingNav.bmp.height) {
          canvas.width = pendingNav.bmp.width;
          canvas.height = pendingNav.bmp.height;
        }
        g.drawImage(pendingNav.bmp, 0, 0);
      }
      pendingNav.bmp.close();
      pendingNav = null;
    }
    if (pin) pin = pinTick(pin, Date.now());
    if (pending && !frozen && !viewing) {
      const g = canvas.getContext('2d');
      if (g) {
        if (canvas.width !== pending.bmp.width || canvas.height !== pending.bmp.height) {
          canvas.width = pending.bmp.width;
          canvas.height = pending.bmp.height;
        }
        g.drawImage(pending.bmp, 0, 0);
        drawn = { ...pending.meta, drawnAtMs: Date.now() };
        render.tick(performance.now());
      }
      pending.bmp.close();
      pending = null;
    }
    const c = current();
    const st = c?.state ?? 'unknown';
    const h = c ? book.health.get(c.id) : undefined;
    const rec = c ? book.records.get(c.id) : undefined;
    const latched = ctx.latch.on;

    status.dataset.state = c ? st : 'none';
    stateChip.dataset.state = st;
    stateChip.textContent = c ? `● ${STATE_LABEL[st] ?? st}${c.reason ? ` · ${REASON_LABEL[c.reason] ?? c.reason}` : ''}` : '캡처 없음';
    srcEl.textContent = c ? `${c.id} · epoch ${c.epoch ?? '?'} · ${c.source} ${c.sourceLabel} · ${c.width ?? '?'}×${c.height ?? '?'}@${c.fps ?? '?'}` : '';
    closeBtn.disabled = !c || !LIVE_STATES.has(st);
    latchEl.hidden = !latched;
    latchEl.textContent = latched ? 'STOP 래치 — 새 프레임 게시 중지, 열기/녹화/스냅샷 거부' : '';
    const errs = [fetchErr, st === 'failed' ? `실패: ${REASON_LABEL[c?.reason ?? ''] ?? c?.reason ?? ''}` : null, c?.stderrTail && st === 'failed' ? c.stderrTail.split('\n').slice(-2).join(' / ') : null].filter(Boolean);
    errEl.textContent = errs.join(' · ');
    errEl.hidden = errs.length === 0;

    const shown = frozen ?? drawn;
    const mismatch = shown && shown.headerSha && shown.headerSha !== shown.sha;
    if (viewing) {
      const v = viewing;
      frameIdEl.textContent = `frame_id ${v.frameId} · 아티팩트 ${v.artifactId}`;
      frameIdEl.dataset.epoch = '';
      frameIdEl.title = '';
      shaEl.textContent = `sha256 ${v.sha}`;
      shaEl.title = `아티팩트 파일 바이트로 계산 (스냅샷 이벤트 sha256 ${v.ok ? '일치' : '불일치!'})`;
      shaEl.dataset.mismatch = v.ok ? 'false' : 'true';
    } else {
      frameIdEl.textContent = shown ? `frame_id ${shown.id}` : 'frame_id —';
      frameIdEl.dataset.epoch = shown?.epoch ?? '';
      frameIdEl.title = shown ? `capture_epoch ${shown.epoch} · recv_utc_ns ${shown.recvUtcNs}` : '';
      shaEl.textContent = shown ? `sha256 ${shown.sha}` : 'sha256 —';
      shaEl.title = shown ? `브라우저가 받은 바이트로 계산 (서버 헤더 ${mismatch ? '불일치!' : '일치'})` : '';
      shaEl.dataset.mismatch = mismatch ? 'true' : 'false';
    }
    stage.dataset.view = viewing ? 'artifact' : 'live';
    frozenEl.textContent = frozen ? `⏸ 고정됨 (capture ${frozen.captureId}) · 수신 ${fmtWallNs(frozen.recvUtcNs)}` : c && !LIVE_STATES.has(st) && c.stoppedAtFrame !== null ? `■ 정지 시점 frame ${c.stoppedAtFrame}` : '';
    stage.dataset.frozen = frozen ? 'true' : 'false';
    overlay.textContent = viewing
      ? `아티팩트 ${viewing.artifactId} · frame ${viewing.frameId} · sha256 ${viewing.ok ? '일치' : '불일치!'}`
      : frozen ? `고정 frame ${frozen.id}` : h?.stale && LIVE_STATES.has(st) ? `정체 — 마지막 수신 ${h.ageMs ?? '?'} ms 전` : !c ? '캡처 없음' : '';
    overlay.hidden = overlay.textContent === '';

    const rfps = render.perSec(performance.now());
    healthEl.textContent = healthLine(h, c, { renderFps: rfps, fetched, sameSkipped }, Date.now());
    healthEl.dataset.renderFps = String(rfps);

    recEl.textContent = rec
      ? `녹화 ${rec.id} · ${rec.state} · frames ${rec.framesWritten} · gap ${rec.gapFrames}${rec.partial ? ` · partial(${rec.partialReasons.join(',')})` : ''}${rec.size !== null ? ` · ${fmtBytes(rec.size)}` : ''} · MJPEG copy MKV (H.264 아님)`
      : '녹화 없음';
    recEl.replaceChildren(recEl.textContent);
    if (rec?.artifactId && rec.state === 'finalized') {
      const a = tid(el('a', undefined, '다운로드'), 'video-record-download');
      a.href = artifactUrl(rec.artifactId);
      a.download = artifactFileName(rec.artifactId) ?? '';
      a.rel = 'noopener';
      recEl.append(' ', a);
    }
    const snaps = book.snapshots.filter((s) => s.captureId === c?.id).slice(-4);
    const key = `${c?.id ?? ''}|${snaps.map((x) => x.artifactId).join(',')}`;
    if (key !== snapsKey) {
      snapsKey = key;
      snapsEl.replaceChildren(
        ...snaps.map((sn) => {
          const row = tid(el('div', 'vd-snap'), 'video-snap-row');
          row.setAttribute('data-artifact-id', sn.artifactId);
          const label = el('span', 'vd-mono', `📷 ${sn.artifactId} · frame ${sn.frameId} · ${sn.sha256.slice(0, 16)}… · ${fmtBytes(sn.size)}`);
          label.title = sn.sha256;
          const view = tid(el('button', undefined, '보기'), 'video-snap-view');
          view.type = 'button';
          view.addEventListener('click', () => void viewArtifact(sn));
          const save = tid(el('a', undefined, '저장'), 'video-snap-save');
          save.href = artifactUrl(sn.artifactId);
          save.download = artifactFileName(sn.artifactId) ?? '';
          save.rel = 'noopener';
          row.append(label, ' ', view, ' ', save);
          return row;
        }),
      );
    }

    const prefill = c && !LIVE_STATES.has(st) ? formFromCapture(c, form) : null;
    reuseBtn.hidden = !prefill;
    reuseNote.textContent = reuseNoteText ?? '';
    reuseNote.hidden = reuseNoteText === null;
    devLoadBtn.hidden = form.source !== 'avfoundation';
    srcListEl.textContent = sourceListText(sources, sourcesErr);
    srcListEl.hidden = srcListEl.textContent === '';

    // ROI inspection: everything below is the daemon's payload rendered verbatim
    const roiRows = roiBook.rows(c?.id ?? null);
    const rk = JSON.stringify(roiRows);
    if (rk !== roiRowsKey) {
      roiRowsKey = rk;
      roiList.replaceChildren(
        ...roiRows.map((r) => {
          const row = tid(el('div', 'vd-roi-row'), 'video-roi-row');
          row.dataset.roi = r.name;
          row.dataset.state = r.state;
          const chip = tid(el('span', 'vd-roi-chip', r.chip), 'video-roi-chip');
          chip.dataset.state = r.state;
          const val = tid(el('span', 'vd-mono', r.value), 'video-roi-value');
          const base = tid(el('span', 'vd-note', r.baseline), 'video-roi-baseline-frame');
          const rm = tid(el('button', undefined, '✕ 삭제'), 'video-roi-remove');
          rm.type = 'button';
          rm.addEventListener('click', () => {
            const cfg = roiBook.config;
            void act('roi_remove', { roi: r.name, ...(cfg ? { expect_rev: cfg.rev } : {}) }, 'ROI 삭제');
          });
          row.append(el('span', 'vd-mono', r.name), ' ', chip, ' ', val, ' ', el('span', 'vd-note', r.threshold), ' ', base, ' ', rm);
          return row;
        }),
      );
    }
    const specs = roiBook.config?.rois ?? [];
    const ok = JSON.stringify([specs.map((x) => [x.name, x.x, x.y, x.w, x.h]), roiRows.map((r) => r.state)]);
    if (ok !== roiOverlayKey) {
      roiOverlayKey = ok;
      roiSvg.replaceChildren(
        ...specs.map((x, k) => {
          const rect = document.createElementNS(SVGNS, 'rect');
          rect.setAttribute('class', 'vd-roi-rect');
          rect.setAttribute('x', String(x.x));
          rect.setAttribute('y', String(x.y));
          rect.setAttribute('width', String(x.w));
          rect.setAttribute('height', String(x.h));
          rect.setAttribute('data-roi', x.name);
          rect.setAttribute('data-state', roiRows[k]?.state ?? 'unknown');
          const title = document.createElementNS(SVGNS, 'title');
          title.textContent = x.name;
          rect.append(title);
          return rect;
        }),
      );
    }
    roiSvg.style.left = `${canvas.offsetLeft}px`;
    roiSvg.style.top = `${canvas.offsetTop}px`;
    roiSvg.style.width = `${canvas.offsetWidth}px`;
    roiSvg.style.height = `${canvas.offsetHeight}px`;
    roiSvg.style.display = viewing ? 'none' : '';
    roiMeta.textContent = inspectLine(c ? roiBook.metrics.get(c.id) : undefined, roiBook.config?.rev ?? null);
    const hist = roiBook.historyFor(c?.id ?? null).slice(0, ROI_HISTORY_MAX);
    const hk = hist.map((x) => `${x.roi}:${x.seq}`).join(',');
    if (hk !== roiHistKey) {
      roiHistKey = hk;
      roiHist.replaceChildren(
        ...hist.map((x) => {
          const row = tid(el('div', 'vd-roi-hist-row', historyText(x)), 'video-roi-hist-row');
          row.dataset.state = x.state;
          return row;
        }),
      );
    }
    const evs = roiBook.evidenceFor(c?.id ?? null);
    const ek = JSON.stringify(evs);
    if (ek !== evidKey) {
      evidKey = ek;
      evidBox.replaceChildren(
        ...evs.map((e) => {
          const row = tid(el('div', 'vd-evid-row'), 'video-evidence-row');
          row.dataset.state = e.state;
          row.append(el('span', undefined, evidenceText(e)));
          if (e.state === 'written' && e.artifactId !== null) {
            const aid = e.artifactId;
            const v = tid(el('button', undefined, '보기'), 'video-evidence-view');
            v.type = 'button';
            v.addEventListener('click', () => void viewEvidence(aid));
            row.append(' ', v);
          }
          return row;
        }),
      );
    }
    const showNav = !!evid && !!viewing && viewing.artifactId === evid.m.artifactId;
    evidNav.hidden = !showNav;
    if (evid && showNav) {
      const fr = evid.m.frames[evid.i];
      const good = evid.oks[evid.i] === true;
      evidLabel.textContent = fr
        ? `증거 ${evid.m.roi} · ${evid.i + 1}/${evid.m.frames.length} ${fr.role} · frame ${fr.frameId} · sha256 ${good ? '일치' : '불일치!'}`
        : '';
      evidLabel.dataset.ok = String(good);
      evidPrev.disabled = evid.i <= 0;
      evidNext.disabled = evid.i >= evid.m.frames.length - 1;
    }
    {
      const opRoi = canOperate();
      const f = frozen ?? drawn;
      putBtn.disabled = !opRoi;
      baseBtn.disabled = !opRoi || !c || !f || f.captureId !== c.id || !roiBook.config?.rois.length;
      baseBtn.title = f ? `기준 = 화면의 frame ${f.id}` : '프레임 없음';
    }

    const op = canOperate() && !latched;
    const live = !!c && LIVE_STATES.has(st);
    const block = viewing ? null : snapshotBlock(frozen, pin, c?.id ?? null);
    snapBtn.disabled = !op || !c || !(frozen ?? drawn) || !!viewing || block !== null;
    snapBtn.title = block ?? '';
    pinEl.textContent = frozen ? pinText(pin, Date.now()) : '';
    pinEl.dataset.status = frozen && pin ? pin.status : 'none';
    pinEl.hidden = pinEl.textContent === '';
    const lastId = c ? (book.health.get(c.id)?.lastFrameId ?? null) : null;
    prevBtn.hidden = nextBtn.hidden = !frozen || !!viewing;
    prevBtn.disabled = navBusy || !frozen || ringStep(frozen.id, -1, lastId) === null;
    nextBtn.disabled = navBusy || !frozen || ringStep(frozen.id, 1, lastId) === null;
    recBtn.disabled = rec?.state === 'recording' ? !c : !op || !live;
    recBtn.textContent = rec?.state === 'recording' ? '■ 녹화 정지' : '● 녹화 시작';
    freezeBtn.textContent = viewing || frozen ? '▶ 실시간' : '⏸ 고정';
    freezeBtn.disabled = !viewing && !frozen && !drawn;
    openBtn.disabled = !op || live;
    for (const [l, on] of [[urlL, form.source === 'http-mjpeg'], [nameL, form.source === 'replay'], [devL, form.source === 'avfoundation']] as const) l.hidden = !on;
    formErr.textContent = validateForm(form) ?? '';
    openErrEl.textContent = openErr ?? '';
    openErrEl.hidden = openErr === null;
    cli.textContent = cliLine(form);
    ctx.scheduler.reportWork(performance.now() - t0);
    return lastSeq < 0n ? 0n : lastSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);

  return {
    mount(host: HTMLElement) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      disposed = true;
      if (timer) clearTimeout(timer);
      pending?.bmp.close();
      pendingNav?.bmp.close();
      pendingView?.bmp.close();
      offAppend();
      offControl();
      offLatch();
      offFrame();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'video.workbench',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'video',
    title: '비디오 캡처',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'video',
        filter: (p) => ({
          domains: ['video'],
          ...(typeof p.channel === 'string' ? { channels: [p.channel] } : {}),
          kinds: [...VIDEO_KINDS, ...ROI_KINDS],
        }),
        snapshot: 'required',
        maxBatchMs: 100,
        mirrorRows: 2_000,
      },
    ],
    queries: ['video.status', 'video.capabilities', 'video.sources'],
    media: { prefixes: ['/api/v1/video/'] },
    actions: [
      { id: 'open', rpc: 'video.open', class: 'request', right: 'operate', confirm: 'none', label: '비디오 캡처 열기' },
      { id: 'snapshot', rpc: 'video.snapshot', class: 'request', right: 'operate', confirm: 'none', label: '비디오 스냅샷' },
      { id: 'record_start', rpc: 'video.record.start', class: 'request', right: 'operate', confirm: 'none', label: '녹화 시작' },
      { id: 'record_stop', rpc: 'video.record.stop', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '녹화 정지' },
      { id: 'pin', rpc: 'video.pin', class: 'request', right: 'watch', confirm: 'none', label: '고정 프레임 보존 (메모리)' },
      { id: 'unpin', rpc: 'video.unpin', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '고정 프레임 보존 해제' },
      { id: 'roi_put', rpc: 'video.roi.put', class: 'request', right: 'operate', confirm: 'none', label: 'ROI 추가/수정' },
      { id: 'roi_remove', rpc: 'video.roi.remove', class: 'request', right: 'operate', confirm: 'none', label: 'ROI 삭제' },
      { id: 'roi_baseline', rpc: 'video.roi.baseline', class: 'request', right: 'operate', confirm: 'none', label: 'ROI 기준 프레임 지정' },
      { id: 'close', rpc: 'video.close', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '비디오 캡처 닫기' },
    ],
    memory: { maxBytes: 64 * 1024 * 1024 },
    render: { maxHz: 60, budgetMs: 8, lagTargetMs: 250 },
  },
  createVideoWorkbench,
);
