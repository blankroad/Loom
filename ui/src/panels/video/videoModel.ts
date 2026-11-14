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

export function readCapture(p: unknown, seq: bigint): CaptureView | null {
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

  applyState(p: unknown, seq: bigint): boolean {
    const c = readCapture(p, seq);
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
  apply(kind: string, p: unknown, seq: bigint): boolean {
    switch (kind) {
      case 'video.capture_state':
        return this.applyState(p, seq);
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
    for (const c of arr(s.captures)) this.applyState(c, throughSeq);
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
