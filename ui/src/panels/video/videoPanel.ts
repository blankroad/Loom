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
  artifactUrl,
  cliLine,
  DEFAULT_FORM,
  fmtBytes,
  frameUrl,
  hex,
  LIVE_STATES,
  openParams,
  RateMeter,
  REASON_LABEL,
  SOURCES,
  STATE_LABEL,
  validateForm,
  VIDEO_KINDS,
  VideoBook,
  wide,
  type OpenForm,
  type SourceKind,
} from './videoModel';
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
  const render = new RateMeter();

  // ---- DOM ----
  const root = el('div', 'vd-panel');
  const status = tid(el('div', 'vd-status'), 'video-status');
  const stateChip = tid(el('span', 'vd-chip'), 'video-state');
  const srcEl = tid(el('span', 'vd-src'), 'video-source');
  const closeBtn = tid(el('button', 'vd-stop', '■ 캡처 닫기'), 'video-close');
  closeBtn.type = 'button';
  closeBtn.title = '수집 정지 + FFmpeg 정리 (안전 방향, 보기 권한 가능)';
  status.append(stateChip, srcEl, closeBtn);
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
  ident.append(frameIdEl, shaEl, frozenEl);

  const healthEl = tid(el('div', 'vd-health'), 'video-health');
  const tools = el('div', 'vd-tools');
  const freezeBtn = tid(el('button', undefined, '⏸ 고정'), 'video-freeze');
  const snapBtn = tid(el('button', undefined, '📷 스냅샷'), 'video-snapshot-btn');
  const recBtn = tid(el('button', undefined, '● 녹화 시작'), 'video-record-btn');
  for (const b of [freezeBtn, snapBtn, recBtn]) b.type = 'button';
  tools.append(freezeBtn, snapBtn, recBtn);
  const recEl = tid(el('div', 'vd-record'), 'video-record');
  const snapsEl = tid(el('div', 'vd-snaps'), 'video-snapshots');
  const audioEl = tid(el('div', 'vd-note', '오디오: 미지원 (video-only) · RTSP/DeckLink: 미지원 · 캡처보드/TCC: 미검증'), 'video-limits');

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
  const openBtn = tid(el('button', 'vd-open', '열기'), 'video-open');
  openBtn.type = 'button';
  const formErr = tid(el('div', 'vd-form-err'), 'video-form-error');
  const cli = tid(el('code', 'vd-cli'), 'video-cli');
  grid.append(lab('소스', srcSel), urlL, nameL, devL, lab('크기', sizeSel), lab('fps', fpsSel), openBtn);
  cfgBox.append(grid, formErr, cli);

  root.append(status, latchEl, errEl, stage, ident, tools, healthEl, recEl, snapsEl, audioEl, cfgBox);

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
    const id = obj(ev.payload).capture_id;
    if (pinned && id !== pinned) return;
    if (book.apply(ev.kind, ev.payload, seqOf(String(ev.seq)))) ctx.scheduler.invalidate();
  };
  const applySnap = () => {
    const snap = mirror.snapshot;
    if (!snap || snap.version === snapshotVersion) return;
    snapshotVersion = snap.version;
    for (const item of snap.items) book.applySnapshotItem(item, snap.throughSeq);
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
    if (!c || frozen || inflight || (!live && !finalFetch)) {
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
  freezeBtn.addEventListener('click', () => {
    frozen = frozen ? null : drawn;
    if (!frozen) schedule(0);
    ctx.scheduler.invalidate();
  });
  snapBtn.addEventListener('click', () => {
    const c = current();
    const f = frozen ?? drawn;
    if (c) void act('snapshot', f && f.captureId === c.id ? { capture_id: c.id, frame_id: f.id } : { capture_id: c.id }, '스냅샷');
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
    void act('open', openParams(form), '캡처 열기');
  });

  // ---- draw ----
  const draw = (): bigint => {
    const t0 = performance.now();
    applySnap();
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = seqOf(String(ev.seq));
      handle(ev);
    }
    if (pending && !frozen) {
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
    frameIdEl.textContent = shown ? `frame_id ${shown.id}` : 'frame_id —';
    frameIdEl.dataset.epoch = shown?.epoch ?? '';
    frameIdEl.title = shown ? `capture_epoch ${shown.epoch} · recv_utc_ns ${shown.recvUtcNs}` : '';
    shaEl.textContent = shown ? `sha256 ${shown.sha}` : 'sha256 —';
    shaEl.title = shown ? `브라우저가 받은 바이트로 계산 (서버 헤더 ${mismatch ? '불일치!' : '일치'})` : '';
    shaEl.dataset.mismatch = mismatch ? 'true' : 'false';
    frozenEl.textContent = frozen ? `⏸ 고정됨 (capture ${frozen.captureId})` : c && !LIVE_STATES.has(st) && c.stoppedAtFrame !== null ? `■ 정지 시점 frame ${c.stoppedAtFrame}` : '';
    stage.dataset.frozen = frozen ? 'true' : 'false';
    overlay.textContent = frozen ? `고정 frame ${frozen.id}` : h?.stale && LIVE_STATES.has(st) ? `정체 — 마지막 수신 ${h.ageMs ?? '?'} ms 전` : !c ? '캡처 없음' : '';
    overlay.hidden = overlay.textContent === '';

    const rfps = render.perSec(performance.now());
    healthEl.textContent = h
      ? [
          `frames ${h.framesTotal}`,
          `수신 ${h.measuredFps}/${h.requestedFps ?? '?'} fps`,
          `표시 ${rfps} fps`,
          `마지막 수신 ${h.ageMs ?? '—'} ms`,
          h.stale ? 'STALE' : 'fresh',
          `dup ${h.dup} / drop ${h.drop}`,
          `oversize ${h.oversize} · truncated ${h.truncated} · junk ${h.junkBytes}B`,
          `media skip ${h.mediaSkipped}`,
          `ring ${h.ringFrames}`,
          `fetched ${fetched} · same ${sameSkipped}`,
        ].join(' · ')
      : `표시 ${rfps} fps`;
    healthEl.dataset.renderFps = String(rfps);

    recEl.textContent = rec
      ? `녹화 ${rec.id} · ${rec.state} · frames ${rec.framesWritten} · gap ${rec.gapFrames}${rec.partial ? ` · partial(${rec.partialReasons.join(',')})` : ''}${rec.size !== null ? ` · ${fmtBytes(rec.size)}` : ''} · MJPEG copy MKV (H.264 아님)`
      : '녹화 없음';
    recEl.replaceChildren(recEl.textContent);
    if (rec?.artifactId && rec.state === 'finalized') {
      const a = el('a', undefined, ' 다운로드');
      a.href = artifactUrl(rec.artifactId);
      recEl.append(a);
    }
    const snaps = book.snapshots.filter((s) => s.captureId === c?.id).slice(-4);
    snapsEl.replaceChildren(
      ...snaps.map((s) => {
        const a = el('a', 'vd-snap', `📷 ${s.artifactId} · frame ${s.frameId} · ${s.sha256.slice(0, 16)}… · ${fmtBytes(s.size)}`);
        a.href = artifactUrl(s.artifactId);
        a.title = s.sha256;
        return a;
      }),
    );

    const op = canOperate() && !latched;
    const live = !!c && LIVE_STATES.has(st);
    snapBtn.disabled = !op || !c || !(frozen ?? drawn);
    recBtn.disabled = rec?.state === 'recording' ? !c : !op || !live;
    recBtn.textContent = rec?.state === 'recording' ? '■ 녹화 정지' : '● 녹화 시작';
    freezeBtn.textContent = frozen ? '▶ 실시간' : '⏸ 고정';
    freezeBtn.disabled = !frozen && !drawn;
    openBtn.disabled = !op || live;
    for (const [l, on] of [[urlL, form.source === 'http-mjpeg'], [nameL, form.source === 'replay'], [devL, form.source === 'avfoundation']] as const) l.hidden = !on;
    formErr.textContent = validateForm(form) ?? '';
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
          kinds: [...VIDEO_KINDS],
        }),
        snapshot: 'required',
        maxBatchMs: 100,
        mirrorRows: 2_000,
      },
    ],
    queries: ['video.status', 'video.capabilities'],
    media: { prefixes: ['/api/v1/video/'] },
    actions: [
      { id: 'open', rpc: 'video.open', class: 'request', right: 'operate', confirm: 'none', label: '비디오 캡처 열기' },
      { id: 'snapshot', rpc: 'video.snapshot', class: 'request', right: 'operate', confirm: 'none', label: '비디오 스냅샷' },
      { id: 'record_start', rpc: 'video.record.start', class: 'request', right: 'operate', confirm: 'none', label: '녹화 시작' },
      { id: 'record_stop', rpc: 'video.record.stop', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '녹화 정지' },
      { id: 'close', rpc: 'video.close', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '비디오 캡처 닫기' },
    ],
    memory: { maxBytes: 64 * 1024 * 1024 },
    render: { maxHz: 60, budgetMs: 8, lagTargetMs: 250 },
  },
  createVideoWorkbench,
);
