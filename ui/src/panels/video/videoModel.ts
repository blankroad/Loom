// video.workbench model — pure helpers (CONTRACT: work/reports/video-capture/CONTRACT.md).
// Events carry metadata only; frame bytes come from GET /api/v1/video/{capture_id}/frame.

export const VIDEO_KINDS = ['video.capture_state', 'video.health', 'video.snapshot', 'video.record_state'] as const;
export const SOURCES = ['http-mjpeg', 'replay', 'avfoundation'] as const;
export type SourceKind = (typeof SOURCES)[number];
export const LIVE_STATES = new Set(['opening', 'live', 'stalled', 'stopping']);

export const STATE_LABEL: Record<string, string> = {
  opening: '여는 중',
  live: '수신 중',
  stalled: '정체(프레임 없음)',
  stopping: '정지 중',
  closed: '닫힘',
  failed: '실패',
  unknown: '알 수 없음',
};

export const REASON_LABEL: Record<string, string> = {
  stop_latch: 'STOP',
  close: '사용자 닫기',
  eof: '소스 종료(EOF)',
  disconnect: '연결 끊김',
  connection_refused: '연결 거부',
  source_not_found: '소스 없음',
  open_timeout: '첫 프레임 시간 초과',
  stall_timeout: '정체 시간 초과',
  open_failed: '열기 실패',
  spawn_error: 'FFmpeg 실행 실패',
  no_frames: '프레임 없음 2초+',
  frames_resumed: '수신 재개',
  worker_shutdown: '워커 종료',
};

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
/** Wide u64 (epoch, ns) as an exact decimal string. The daemon sends strings (U64 contract); a JSON number is
 * accepted only while it is a safe integer — an unsafe one was already rounded by JSON.parse, so it is refused. */
export const wide = (v: unknown): string | null =>
  typeof v === 'string' && /^(0|[1-9]\d{0,19})$/.test(v) ? v : typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? String(v) : null;

export interface CaptureView {
  id: string;
  seq: bigint;
  /** capture_epoch, exact decimal string */
  epoch: string | null;
  state: string;
  reason: string | null;
  sourceLabel: string;
  source: string;
  width: number | null;
  height: number | null;
  fps: number | null;
  stoppedAtFrame: number | null;
  error: string | null;
  stderrTail: string;
  cleanupMs: number | null;
  /** ts.wall_ns of the newest applied capture_state event (exact decimal); null for reducer snapshot items */
  stateWallNs: string | null;
}

export interface HealthView {
  framesTotal: number;
  measuredFps: number;
  requestedFps: number | null;
  lastFrameId: number | null;
  /** exact decimal string */
  lastRecvMonoNs: string | null;
  ageMs: number | null;
  stale: boolean;
  oversize: number;
  truncated: number;
  junkBytes: number;
  dup: number;
  drop: number;
  mediaSkipped: number;
  ringFrames: number;
  /** `last_recv_utc_ns` (exact decimal) — optional, null from daemons that do not send it */
  lastRecvUtcNs: string | null;
  /** health payload `state` (null when absent) */
  state: string | null;
}

export interface RecordView {
  id: string;
  state: string;
  framesWritten: number;
  gapFrames: number;
  partial: boolean;
  partialReasons: string[];
  size: number | null;
  sha256: string | null;
  artifactId: string | null;
  limitReason: string | null;
}

export interface SnapshotView {
  artifactId: string;
  frameId: number;
  sha256: string;
  size: number;
  /** exact decimal strings */
  epoch: string | null;
  recvUtcNs: string | null;
}

export function readCapture(p: unknown, seq: bigint, wallNs: unknown = null): CaptureView | null {
  const o = obj(p);
  const id = str(o.capture_id);
  if (!id) return null;
  const cfg = obj(o.config);
  return {
    id,
    seq,
    epoch: wide(o.capture_epoch),
    state: str(o.state) ?? 'unknown',
    reason: str(o.reason),
    sourceLabel: str(cfg.source_label) ?? '',
    source: str(cfg.source) ?? '',
    width: num(cfg.width),
    height: num(cfg.height),
    fps: num(cfg.fps),
    stoppedAtFrame: num(o.stopped_at_frame_id),
    error: str(o.error),
    stderrTail: str(o.stderr_tail) ?? '',
    cleanupMs: num(o.cleanup_ms),
    stateWallNs: wide(wallNs),
  };
}

export function readHealth(p: unknown): HealthView | null {
  const o = obj(p);
  if (!str(o.capture_id)) return null;
  const ff = obj(o.ffmpeg);
  const media = obj(o.media);
  const ring = obj(o.ring);
  return {
    framesTotal: num(o.frames_total) ?? 0,
    measuredFps: num(o.measured_fps) ?? 0,
    requestedFps: num(o.requested_fps),
    lastFrameId: num(o.last_frame_id),
    lastRecvMonoNs: wide(o.last_recv_mono_ns),
    ageMs: num(o.age_ms),
    stale: o.stale === true,
    oversize: num(o.oversize) ?? 0,
    truncated: num(o.truncated) ?? 0,
    junkBytes: num(o.junk_bytes) ?? 0,
    dup: num(ff.dup_frames) ?? 0,
    drop: num(ff.drop_frames) ?? 0,
    mediaSkipped: num(media.skipped) ?? 0,
    ringFrames: num(ring.frames) ?? 0,
    lastRecvUtcNs: wide(o.last_recv_utc_ns),
    state: str(o.state),
  };
}

export function readRecord(p: unknown): RecordView | null {
  const o = obj(p);
  const id = str(o.recording_id);
  if (!id) return null;
  return {
    id,
    state: str(o.state) ?? 'unknown',
    framesWritten: num(o.frames_written) ?? 0,
    gapFrames: num(o.gap_frames) ?? 0,
    partial: o.partial === true,
    partialReasons: Array.isArray(o.partial_reasons) ? o.partial_reasons.filter((x): x is string => typeof x === 'string') : [],
    size: num(o.size),
    sha256: str(o.sha256),
    artifactId: str(o.artifact_id),
    limitReason: str(o.limit_reason),
  };
}

export function readSnapshot(p: unknown): SnapshotView | null {
  const o = obj(p);
  const artifactId = str(o.artifact_id);
  const sha256 = str(o.sha256);
  const frameId = num(o.frame_id);
  if (!artifactId || !sha256 || frameId === null) return null;
  return { artifactId, frameId, sha256, size: num(o.size) ?? 0, epoch: wide(o.capture_epoch), recvUtcNs: wide(o.recv_utc_ns) };
}

/** Per-capture book: newest state wins by seq (snapshot items use the snapshot high-water). */
export class VideoBook {
  readonly captures = new Map<string, CaptureView>();
  readonly health = new Map<string, HealthView>();
  readonly records = new Map<string, RecordView>();
  readonly snapshots: (SnapshotView & { captureId: string })[] = [];
  newestId: string | null = null;
  private newestSeq = -1n;

  applyState(p: unknown, seq: bigint, wallNs: unknown = null): boolean {
    const c = readCapture(p, seq, wallNs);
    if (!c) return false;
    const old = this.captures.get(c.id);
    if (old && old.seq > seq) return false;
    this.captures.set(c.id, c);
    if (seq >= this.newestSeq) {
      this.newestSeq = seq;
      this.newestId = c.id;
    }
    return true;
  }
  applyHealth(p: unknown): boolean {
    const id = str(obj(p).capture_id);
    const h = readHealth(p);
    if (!id || !h) return false;
    this.health.set(id, h);
    return true;
  }
  applyRecord(p: unknown): boolean {
    const id = str(obj(p).capture_id);
    const r = readRecord(p);
    if (!id || !r) return false;
    this.records.set(id, r);
    return true;
  }
  applySnapshot(p: unknown): boolean {
    const id = str(obj(p).capture_id);
    const s = readSnapshot(p);
    if (!id || !s || this.snapshots.some((x) => x.artifactId === s.artifactId)) return false;
    this.snapshots.push({ ...s, captureId: id });
    while (this.snapshots.length > 16) this.snapshots.shift();
    return true;
  }
  apply(kind: string, p: unknown, seq: bigint, wallNs: unknown = null): boolean {
    switch (kind) {
      case 'video.capture_state':
        return this.applyState(p, seq, wallNs);
      case 'video.health':
        return this.applyHealth(p);
      case 'video.record_state':
        return this.applyRecord(p);
      case 'video.snapshot':
        return this.applySnapshot(p);
      default:
        return false;
    }
  }
  /** Reducer snapshot item `{domain:'video', state:{captures, health, records, snapshots}}`. */
  applySnapshotItem(item: unknown, throughSeq: bigint): void {
    const it = obj(item);
    if (it.domain !== 'video') return;
    const s = obj(it.state);
    const arr = (v: unknown) => (Array.isArray(v) ? v : []);
    for (const c of arr(s.captures)) this.applyState(c, throughSeq, null);
    for (const h of arr(s.health)) this.applyHealth(h);
    for (const r of arr(s.records)) this.applyRecord(r);
    for (const x of arr(s.snapshots)) this.applySnapshot(x);
  }
}

export interface OpenForm {
  source: SourceKind;
  url: string;
  name: string;
  device: string;
  width: number;
  height: number;
  fps: number;
}

export const DEFAULT_FORM: OpenForm = { source: 'http-mjpeg', url: 'http://127.0.0.1:8090/stream', name: '', device: '', width: 1280, height: 720, fps: 30 };

/** Mirror of the backend's strict checks (the backend stays authoritative). */
export function validateForm(f: OpenForm): string | null {
  if (f.source === 'http-mjpeg') {
    let u: URL;
    try {
      u = new URL(f.url);
    } catch {
      return 'URL 형식 오류';
    }
    if (u.protocol !== 'http:') return 'http:// 루프백 URL만 지원';
    if (u.username || u.password) return 'URL 자격증명(user@)은 허용되지 않음';
    if (!['127.0.0.1', 'localhost', '[::1]'].includes(u.hostname)) return '루프백 호스트만 허용 (127.0.0.1, localhost, [::1])';
  } else if (f.source === 'replay') {
    if (!/^[A-Za-z0-9_-][A-Za-z0-9._-]{0,63}$/.test(f.name)) return 'replay: $LOOM_HOME/video/replay 안의 파일 이름만';
  } else {
    const d = f.device;
    if (!d || d.trim() !== d || /^(default|none)$/i.test(d) || /^\d+$/.test(d) || d.includes(':')) {
      return 'avfoundation: 정확한 장치 이름 필요 (default/none/번호 금지)';
    }
  }
  if (f.width % 2 || f.height % 2 || f.width < 160 || f.width > 3840 || f.height < 120 || f.height > 2160) return '해상도 범위/짝수 오류';
  if (!Number.isInteger(f.fps) || f.fps < 1 || f.fps > 60) return 'fps 1..60';
  return null;
}

export function openParams(f: OpenForm): Record<string, unknown> {
  const p: Record<string, unknown> = { source: f.source, width: f.width, height: f.height, fps: f.fps };
  if (f.source === 'http-mjpeg') p.url = f.url;
  else if (f.source === 'replay') p.name = f.name;
  else p.device = f.device;
  return p;
}

export function cliLine(f: OpenForm): string {
  const sel = f.source === 'http-mjpeg' ? `--url ${f.url}` : f.source === 'replay' ? `--name ${f.name}` : `--device '${f.device}'`;
  return `loom video open --source ${f.source} ${sel} --size ${f.width}x${f.height} --fps ${f.fps}`;
}

export function hex(buf: ArrayBuffer): string {
  return Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');
}

export const frameUrl = (captureId: string, frameId?: number): string =>
  `/api/v1/video/${encodeURIComponent(captureId)}/frame${frameId ? `?id=${frameId}` : ''}`;
export const artifactUrl = (artifactId: string): string => `/api/v1/video/artifact/${encodeURIComponent(artifactId)}`;

/** Render-rate meter over the last second. */
export class RateMeter {
  private t: number[] = [];
  tick(now: number): void {
    this.t.push(now);
    if (this.t.length > 256) this.t.shift();
  }
  perSec(now: number): number {
    while (this.t.length && now - (this.t[0] ?? now) > 1000) this.t.shift();
    return this.t.length;
  }
}

export const fmtBytes = (n: number | null): string =>
  n === null ? '—' : n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GiB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MiB` : `${Math.round(n / 1024)} KiB`;

// ---- operator upgrade B1 (contracts: work/reports/operator-upgrade/video/FUNCTIONS.md §B1-M) ----

export interface HealthExtras {
  renderFps: number;
  fetched: number;
  sameSkipped: number;
}

export interface Prefill {
  form: OpenForm;
  note: string | null;
}

export interface SourceList {
  replay: string[];
  local: { enumerated: boolean; devices: string[]; warning: string | null };
}

export const REUSE_HTTP_NOTE = 'URL은 쿼리 문자열 없이 기록됩니다 — 토큰이 필요하면 다시 입력하세요';
export const MAX_SOURCE_ITEMS = 100;

/** M4: exact wall ns → local `HH:MM:SS.mmm`, `—` when absent/invalid. */
export function fmtWallNs(ns: string | null | undefined): string {
  const w = wide(ns);
  if (w === null) return '—';
  const ms = Number(BigInt(w) / 1_000_000n);
  if (!Number.isSafeInteger(ms)) return '—';
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return '—';
  const p2 = (n: number) => String(n).padStart(2, '0');
  return `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, '0')}`;
}

/** M5: elapsed ms → `N초` / `N분` / `N시간 M분`. */
export function fmtAgo(ms: number): string {
  const t = Number.isFinite(ms) && ms > 0 ? ms : 0;
  if (t < 60_000) return `${Math.floor(t / 1000)}초`;
  if (t < 3_600_000) return `${Math.floor(t / 60_000)}분`;
  return `${Math.floor(t / 3_600_000)}시간 ${Math.floor((t % 3_600_000) / 60_000)}분`;
}

/** M6 (V-05): live line unchanged; ended captures get a summary without fps/fresh/STALE. */
export function healthLine(h: HealthView | undefined, c: CaptureView | null, x: HealthExtras, nowWallMs: number): string {
  if (c === null) return `표시 ${x.renderFps} fps`;
  if (LIVE_STATES.has(c.state)) {
    if (!h) return `표시 ${x.renderFps} fps`;
    return [
      `frames ${h.framesTotal}`,
      `수신 ${h.measuredFps}/${h.requestedFps ?? '?'} fps`,
      `표시 ${x.renderFps} fps`,
      `마지막 수신 ${h.ageMs ?? '—'} ms`,
      h.stale ? 'STALE' : 'fresh',
      `dup ${h.dup} / drop ${h.drop}`,
      `oversize ${h.oversize} · truncated ${h.truncated} · junk ${h.junkBytes}B`,
      `media skip ${h.mediaSkipped}`,
      `ring ${h.ringFrames}`,
      `fetched ${x.fetched} · same ${x.sameSkipped}`,
    ].join(' · ');
  }
  const ended = fmtWallNs(c.stateWallNs);
  const ago = ended !== '—' && c.stateWallNs !== null ? ` (${fmtAgo(nowWallMs - Number(BigInt(c.stateWallNs) / 1_000_000n))} 전)` : '';
  return `종료 · frames ${h?.framesTotal ?? '—'} · 마지막 frame ${c.stoppedAtFrame ?? h?.lastFrameId ?? '—'} · 마지막 수신 ${fmtWallNs(h?.lastRecvUtcNs)} · 종료 시각 ${ended}${ago}`;
}

/** M7 (V-07): prefill the open form from an ended capture (no I/O, no validation). */
export function formFromCapture(c: CaptureView, base: OpenForm): Prefill | null {
  const posInt = (v: number | null): v is number => v !== null && Number.isInteger(v) && v > 0;
  const dims = { width: posInt(c.width) ? c.width : base.width, height: posInt(c.height) ? c.height : base.height, fps: posInt(c.fps) ? c.fps : base.fps };
  const l = c.sourceLabel;
  if (c.source === 'http-mjpeg' && l.startsWith('http://')) return { form: { ...base, ...dims, source: 'http-mjpeg', url: l }, note: REUSE_HTTP_NOTE };
  if (c.source === 'replay' && l.startsWith('replay:')) return { form: { ...base, ...dims, source: 'replay', name: l.slice(7) }, note: null };
  if (c.source === 'avfoundation' && l.startsWith('avfoundation:')) return { form: { ...base, ...dims, source: 'avfoundation', device: l.slice(13) }, note: null };
  return null;
}

/** M8 (V-08): `video.sources` result (envelope or bare) → filtered lists. */
export function readSources(r: unknown): SourceList {
  const d = obj(obj(r).data ?? r);
  const replay: string[] = [];
  if (Array.isArray(d.replay)) {
    for (const v of d.replay) {
      if (replay.length >= MAX_SOURCE_ITEMS) break;
      if (typeof v === 'string' && /^[A-Za-z0-9_-][A-Za-z0-9._-]{0,63}$/.test(v) && !replay.includes(v)) replay.push(v);
    }
  }
  const lo = obj(d.local);
  const enumerated = lo.enumerated === true;
  const devices: string[] = [];
  if (enumerated && Array.isArray(lo.avfoundation_video)) {
    for (const v of lo.avfoundation_video) {
      if (devices.length >= MAX_SOURCE_ITEMS) break;
      const n = str(obj(v).name);
      if (n !== null && n.length >= 1 && n.length <= 128 && ![...n].some((ch) => ch.charCodeAt(0) <= 0x1f || ch.charCodeAt(0) === 0x7f) && !devices.includes(n)) devices.push(n);
    }
  }
  return { replay, local: { enumerated, devices, warning: str(lo.warning) } };
}

/** M9: one truthful line for the source lists (or the query failure). */
export function sourceListText(s: SourceList | null, error: string | null): string {
  if (error !== null) return `video.sources 실패: ${error}`;
  if (s === null) return '';
  const parts = [s.replay.length > 0 ? `replay ${s.replay.length}개` : 'replay 없음 ($LOOM_HOME/video/replay)'];
  if (s.local.enumerated) parts.push(`장치 ${s.local.devices.length}개 (실물·TCC 미검증)`);
  return parts.join(' · ');
}

/** M10 (V-06): download file name for a snapshot/recording artifact id; null otherwise. */
export function artifactFileName(artifactId: string): string | null {
  if (/^snap-v[0-9a-f]{12}-\d{1,12}$/.test(artifactId)) return `${artifactId}.jpg`;
  if (/^rec-v[0-9a-f]{12}-\d{1,12}$/.test(artifactId)) return `${artifactId}.mkv`;
  return null;
}

// ------------------------------------------------------------------------------------------------
// Operator upgrade B3 (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 3): freeze = daemon pin
// (video.pin / video.unpin), visible retention countdown, snapshot disabled with a reason when the
// frozen frame is not retained, ring ◀▶ while frozen. Types/constants by Opus; bodies by Sonnet.

/** TTL the panel requests for a freeze pin (= daemon default `PIN_TTL_DEFAULT_MS`). */
export const PIN_TTL_MS = 120_000;
/** Pin lifecycle as the panel knows it (no `released`: a released pin is `null`). */
export type PinStatus = 'pending' | 'pinned' | 'refused' | 'expired' | 'error';
/** The panel's view of its own freeze pin. */
export interface PinView {
  captureId: string;
  frameId: number;
  status: PinStatus;
  /** local wall ms when the daemon pin lapses (`Date.now()` at reply + `expires_in_ms`); null unless pinned/expired */
  expiresAtMs: number | null;
  /** daemon `ttl_ms` of the reply */
  ttlMs: number | null;
  /** refusal/error reason code (`pin_user_limit`, `pin_bytes`, `not_found`, `bad_reply`, or a normalized error code) */
  reason: string | null;
  /** daemon sha256 of the pinned bytes */
  sha256: string | null;
}
/** Operator wording for pin reason codes; unknown codes are shown raw (textContent). */
export const PIN_REASON_LABEL: Record<string, string> = {
  pin_user_limit: '고정 한도 4개',
  pin_bytes: '보존 메모리 한도 16 MiB',
  not_found: '프레임이 이미 링에 없음',
  bad_reply: '응답 형식 오류',
};
/** Snapshot-disabled suffix telling the operator how to recover. */
export const PIN_RETRY_HINT = '▶ 실시간 후 다시 고정';

/** B3-M1 `video.pin` success (envelope `{data}` or bare) → pinned view; malformed → `error/bad_reply`. */
export function readPinResult(r: unknown, captureId: string, frameId: number, nowMs: number): PinView {
  const top = typeof r === 'object' && r !== null ? (r as Record<string, unknown>) : null;
  const inner = top !== null && typeof top.data === 'object' && top.data !== null ? (top.data as Record<string, unknown>) : top;
  const e = inner?.expires_in_ms;
  if (inner === null || typeof e !== 'number' || !Number.isFinite(e) || e < 0) {
    return { captureId, frameId, status: 'error', expiresAtMs: null, ttlMs: null, reason: 'bad_reply', sha256: null };
  }
  const fid = inner.frame_id;
  const ttl = inner.ttl_ms;
  return {
    captureId,
    frameId: typeof fid === 'number' && Number.isSafeInteger(fid) && fid >= 1 ? fid : frameId,
    status: 'pinned',
    expiresAtMs: nowMs + e,
    ttlMs: typeof ttl === 'number' && Number.isFinite(ttl) ? ttl : null,
    reason: null,
    sha256: typeof inner.sha256 === 'string' ? inner.sha256 : null,
  };
}

/** B3-M2 `video.pin` rejection (DomainRpcError-like `{code, detail}`) → refused / error view. */
export function readPinError(e: unknown, captureId: string, frameId: number): PinView {
  const o = typeof e === 'object' && e !== null ? (e as Record<string, unknown>) : null;
  const raw = o !== null && typeof o.code === 'string' && o.code !== '' ? o.code : null;
  const code = raw === null ? 'internal' : raw.replace(/^E_/, '').toLowerCase();
  const detail = o !== null && typeof o.detail === 'object' && o.detail !== null ? (o.detail as Record<string, unknown>) : null;
  const base = { captureId, frameId, expiresAtMs: null, ttlMs: null, sha256: null };
  if (code === 'limit') return { ...base, status: 'refused', reason: typeof detail?.reason === 'string' ? detail.reason : 'limit' };
  if (code === 'not_found') return { ...base, status: 'error', reason: 'not_found' };
  return { ...base, status: 'error', reason: code };
}

/** B3-M3 pinned and lapsed at `nowMs` → a new `expired` view; otherwise the same object. */
export function pinTick(p: PinView, nowMs: number): PinView {
  return p.status === 'pinned' && p.expiresAtMs !== null && nowMs >= p.expiresAtMs ? { ...p, status: 'expired' } : p;
}

/** B3-M4 remaining ms → `m:ss` (ceil to whole seconds; ≤ 0 / NaN → `0:00`). */
export function fmtRemain(ms: number): string {
  const s = Math.ceil(Math.max(0, Number.isFinite(ms) ? ms : 0) / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** B3-M5 one status line for the frozen frame's pin ('' when none). */
export function pinText(p: PinView | null, nowMs: number): string {
  if (p === null) return '';
  const label = (r: string | null) => (r === null ? '?' : (PIN_REASON_LABEL[r] ?? r));
  const n = p.frameId;
  switch (p.status) {
    case 'pending':
      return `📌 보존 요청 중 · frame ${n}`;
    case 'pinned':
      return `📌 데몬 보존 · frame ${n} · 남은 시간 ${p.expiresAtMs === null ? '?' : fmtRemain(p.expiresAtMs - nowMs)}`;
    case 'refused':
      return `⚠ 보존 거부 · frame ${n} · ${label(p.reason)}`;
    case 'expired':
      return `⌛ 보존 만료 · frame ${n}`;
    case 'error':
      return `⚠ 보존 실패 · frame ${n} · ${label(p.reason)}`;
  }
}

/** B3-M6 why the snapshot button is disabled for the frozen frame, or null when allowed. */
export function snapshotBlock(frozen: { captureId: string; id: number } | null, pin: PinView | null, captureId: string | null): string | null {
  if (frozen === null) return null;
  if (captureId === null || frozen.captureId !== captureId) return '다른 캡처의 고정 프레임';
  if (pin === null || pin.captureId !== frozen.captureId || pin.frameId !== frozen.id) return `고정 프레임 보존 안 됨 — ${PIN_RETRY_HINT}`;
  switch (pin.status) {
    case 'pinned':
      return null;
    case 'pending':
      return '보존 확인 중';
    case 'expired':
      return `고정 프레임 보존 만료 — ${PIN_RETRY_HINT}`;
    case 'refused':
    case 'error': {
      const l = pin.reason === null ? '?' : (PIN_REASON_LABEL[pin.reason] ?? pin.reason);
      return `고정 프레임 보존 안 됨 (${l}) — ${PIN_RETRY_HINT}`;
    }
  }
}

/** B3-M7 ring ◀▶ target id, or null when there is nowhere verifiable to go. */
export function ringStep(frameId: number, dir: -1 | 1, lastFrameId: number | null): number | null {
  const n = frameId + dir;
  if (n < 1) return null;
  if (dir === 1 && (lastFrameId === null || n > lastFrameId)) return null;
  return n;
}
