// Operator upgrade B6 (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 6): ROI inspection view.
// The daemon judges (video.roi_judgment / video.roi_metric / video.evidence); this module only reads those
// journal payloads, keeps a bounded book of them and formats text. It never computes a metric or a state.
// Types/constants by Opus; function bodies by Sonnet. Untrusted strings are rendered via textContent only.

import { fmtWallNs } from './videoModel';

/** Journal kinds this view reads (also subscribed by the panel topic). */
export const ROI_KINDS = ['video.roi_config', 'video.roi_judgment', 'video.roi_metric', 'video.evidence'] as const;
/** ROI judgment states (daemon). */
export type RoiStateName = 'unknown' | 'normal' | 'changed';
export const ROI_STATE_LABEL: Record<RoiStateName, string> = { unknown: '판정 불가', normal: '정상', changed: '변화' };
/** UNKNOWN reasons (daemon `UnknownReason`). Unknown codes are shown raw. */
export const ROI_REASON_LABEL: Record<string, string> = {
  not_live: '캡처 비활성',
  stale: '수신 정체',
  no_baseline: '기준 없음',
  baseline_mismatch: '기준 불일치',
  roi_too_small: 'ROI 너무 작음',
  decode_error: '디코드 실패',
  analysis_gap: '분석 공백',
  config_changed: '설정 변경',
};
export const METRIC_LABEL: Record<string, string> = { luma_delta: '밝기 차', mean_abs_diff: '평균 차 %', changed_px: '변화 픽셀 %' };
export const METRICS = ['mean_abs_diff', 'changed_px', 'luma_delta'] as const;
/** Shown wherever a judgment is shown: pixel change ≠ scene understanding. */
export const PIXEL_NOTE_KO = '픽셀 변화 측정만 — 장면 이해 아님';
/** Bounded panel memory. */
export const ROI_HISTORY_MAX = 64;
export const EVIDENCE_MAX = 16;
export const EVIDENCE_SCHEMA = 'loom.video.evidence/1';
export const EVIDENCE_STATE_LABEL: Record<string, string> = { collecting: '수집 중', written: '저장됨', skipped: '건너뜀', aborted: '중단', failed: '실패' };

export interface RoiSpecView {
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  metric: string;
  enter: number;
  exit: number;
  dwellMs: number;
  cooldownMs: number;
  evidence: boolean;
}
export interface RoiConfigView {
  rev: number;
  rois: RoiSpecView[];
  /** capture channel the event came on (null = edited without a capture) */
  captureId: string | null;
  seq: bigint;
}
export interface RoiMetricsView {
  meanLuma: number;
  lumaDelta: number;
  meanAbsDiff: number;
  changedPx: number;
}
export interface RoiStateView {
  captureId: string;
  roi: string;
  state: RoiStateName;
  reason: string | null;
  /** daemon transition_seq (per ROI) */
  seq: number;
  frameId: number | null;
  sha256: string | null;
  recvUtcNs: string | null;
  metrics: RoiMetricsView | null;
  baselineFrameId: number | null;
  configRev: number | null;
  evidence: boolean;
}
export interface RoiMetricRow {
  roi: string;
  state: RoiStateName;
  reason: string | null;
  seq: number;
  metrics: RoiMetricsView | null;
  metricsFrameId: number | null;
}
export interface RoiMetricView {
  captureId: string;
  configRev: number | null;
  analysed: number;
  skipped: number;
  decodeErrors: number;
  lastFrameId: number | null;
  hzMax: number | null;
  rois: RoiMetricRow[];
}
export type EvidenceRole = 'pre' | 'trigger' | 'post';
export interface EvidenceFrameView {
  role: EvidenceRole;
  frameId: number;
  sha256: string;
  size: number;
}
export type EvidenceState = 'collecting' | 'written' | 'skipped' | 'aborted' | 'failed';
export interface EvidenceView {
  captureId: string;
  n: number;
  state: EvidenceState;
  roi: string;
  seq: number | null;
  artifactId: string | null;
  triggerFrameId: number | null;
  frames: EvidenceFrameView[];
  partial: string[];
  reason: string | null;
  size: number | null;
}
export interface ManifestFrame extends EvidenceFrameView {
  b64: string;
  recvUtcNs: string | null;
}
export interface EvidenceManifest {
  artifactId: string;
  captureId: string;
  roi: string;
  seq: number | null;
  partial: string[];
  frames: ManifestFrame[];
}
/** One list row (pure, from the book). */
export interface RoiRow {
  name: string;
  state: RoiStateName;
  /** UNKNOWN reason, or `no_state` when the daemon has published nothing for this capture yet */
  reason: string | null;
  chip: string;
  value: string;
  threshold: string;
  baseline: string;
  seq: number;
}
/** Editor form (raw input strings). */
export interface RoiForm {
  name: string;
  rect: string;
  metric: string;
  enter: string;
  exit: string;
  dwell: string;
  cooldown: string;
}

// ---- readers (M1–M5) ----
export function readRoiConfig(p: unknown, seq: bigint): RoiConfigView | null {
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
  const fin = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
  if (!isObj(p) || !isInt(p.config_rev) || !Array.isArray(p.rois)) return null;
  const rois: RoiSpecView[] = [];
  for (const r of p.rois as unknown[]) {
    if (!isObj(r) || typeof r.name !== 'string' || !/^[A-Za-z0-9_-]{1,32}$/.test(r.name)) continue;
    if (!fin(r.x) || !fin(r.y) || !fin(r.w) || !fin(r.h) || !fin(r.enter)) continue;
    rois.push({
      name: r.name,
      x: r.x,
      y: r.y,
      w: r.w,
      h: r.h,
      metric: typeof r.metric === 'string' ? r.metric : 'mean_abs_diff',
      enter: r.enter,
      exit: fin(r.exit) ? r.exit : r.enter / 2,
      dwellMs: isInt(r.dwell_ms) ? r.dwell_ms : 300,
      cooldownMs: isInt(r.cooldown_ms) ? r.cooldown_ms : 1000,
      evidence: typeof r.evidence === 'boolean' ? r.evidence : true,
    });
  }
  return { rev: p.config_rev, rois, captureId: typeof p.capture_id === 'string' ? p.capture_id : null, seq };
}
export function readRoiState(p: unknown): RoiStateView | null {
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
  const fin = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
  if (!isObj(p) || typeof p.capture_id !== 'string' || typeof p.roi !== 'string') return null;
  if (typeof p.state !== 'string' || !Object.hasOwn(ROI_STATE_LABEL, p.state) || !isInt(p.transition_seq)) return null;
  const m = p.metrics;
  const metrics =
    isObj(m) && fin(m.mean_luma) && fin(m.luma_delta) && fin(m.mean_abs_diff) && fin(m.changed_px)
      ? { meanLuma: m.mean_luma, lumaDelta: m.luma_delta, meanAbsDiff: m.mean_abs_diff, changedPx: m.changed_px }
      : null;
  const bf = isObj(p.baseline) ? p.baseline.frame_id : null;
  return {
    captureId: p.capture_id,
    roi: p.roi,
    state: p.state as RoiStateName,
    reason: typeof p.reason === 'string' ? p.reason : null,
    seq: p.transition_seq,
    frameId: isInt(p.frame_id) ? p.frame_id : null,
    sha256: typeof p.sha256 === 'string' && /^[0-9a-f]{64}$/.test(p.sha256) ? p.sha256 : null,
    recvUtcNs: typeof p.recv_utc_ns === 'string' && /^\d{1,20}$/.test(p.recv_utc_ns) ? p.recv_utc_ns : null,
    metrics,
    baselineFrameId: isInt(bf) ? bf : null,
    configRev: isInt(p.config_rev) ? p.config_rev : null,
    evidence: p.evidence === true,
  };
}
export function readRoiMetric(p: unknown): RoiMetricView | null {
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
  const fin = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
  if (!isObj(p) || typeof p.capture_id !== 'string') return null;
  const a = isObj(p.analysis) ? p.analysis : {};
  const rois: RoiMetricRow[] = [];
  for (const r of Array.isArray(p.rois) ? (p.rois as unknown[]) : []) {
    if (!isObj(r) || typeof r.roi !== 'string' || typeof r.state !== 'string' || !Object.hasOwn(ROI_STATE_LABEL, r.state)) continue;
    if (!isInt(r.transition_seq)) continue;
    const m = r.metrics;
    rois.push({
      roi: r.roi,
      state: r.state as RoiStateName,
      reason: typeof r.reason === 'string' ? r.reason : null,
      seq: r.transition_seq,
      metrics:
        isObj(m) && fin(m.mean_luma) && fin(m.luma_delta) && fin(m.mean_abs_diff) && fin(m.changed_px)
          ? { meanLuma: m.mean_luma, lumaDelta: m.luma_delta, meanAbsDiff: m.mean_abs_diff, changedPx: m.changed_px }
          : null,
      metricsFrameId: isInt(r.metrics_frame_id) ? r.metrics_frame_id : null,
    });
  }
  return {
    captureId: p.capture_id,
    configRev: isInt(p.config_rev) ? p.config_rev : null,
    analysed: isInt(a.analysed) ? a.analysed : 0,
    skipped: isInt(a.skipped) ? a.skipped : 0,
    decodeErrors: isInt(a.decode_errors) ? a.decode_errors : 0,
    lastFrameId: isInt(a.last_frame_id) ? a.last_frame_id : null,
    hzMax: isInt(p.inspection_hz_max) ? p.inspection_hz_max : null,
    rois,
  };
}
export function readEvidence(p: unknown): EvidenceView | null {
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
  if (!isObj(p) || typeof p.capture_id !== 'string' || typeof p.roi !== 'string') return null;
  if (!isInt(p.evidence_id) || p.evidence_id < 1) return null;
  if (typeof p.state !== 'string' || !Object.hasOwn(EVIDENCE_STATE_LABEL, p.state)) return null;
  const frames: EvidenceFrameView[] = [];
  for (const f of Array.isArray(p.frames) ? (p.frames as unknown[]) : []) {
    if (!isObj(f) || (f.role !== 'pre' && f.role !== 'trigger' && f.role !== 'post') || !isInt(f.frame_id)) continue;
    if (typeof f.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(f.sha256) || !isInt(f.size)) continue;
    frames.push({ role: f.role, frameId: f.frame_id, sha256: f.sha256, size: f.size });
  }
  return {
    captureId: p.capture_id,
    n: p.evidence_id,
    state: p.state as EvidenceState,
    roi: p.roi,
    seq: isInt(p.transition_seq) ? p.transition_seq : null,
    artifactId: typeof p.artifact_id === 'string' && /^evid-v[0-9a-f]{12}-\d{1,12}$/.test(p.artifact_id) ? p.artifact_id : null,
    triggerFrameId: isInt(p.trigger_frame_id) ? p.trigger_frame_id : null,
    frames,
    partial: Array.isArray(p.partial) ? (p.partial as unknown[]).filter((x): x is string => typeof x === 'string') : [],
    reason: typeof p.reason === 'string' ? p.reason : null,
    size: isInt(p.size) ? p.size : null,
  };
}
export function readEvidenceManifest(j: unknown): EvidenceManifest | null {
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
  if (!isObj(j) || j.schema !== EVIDENCE_SCHEMA) return null;
  if (typeof j.artifact_id !== 'string' || typeof j.capture_id !== 'string' || typeof j.roi !== 'string' || !Array.isArray(j.frames)) return null;
  const frames: ManifestFrame[] = [];
  for (const f of j.frames as unknown[]) {
    if (!isObj(f) || (f.role !== 'pre' && f.role !== 'trigger' && f.role !== 'post') || !isInt(f.frame_id)) return null;
    if (typeof f.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(f.sha256) || !isInt(f.size) || typeof f.jpeg_b64 !== 'string') return null;
    frames.push({
      role: f.role,
      frameId: f.frame_id,
      sha256: f.sha256,
      size: f.size,
      b64: f.jpeg_b64,
      recvUtcNs: typeof f.recv_utc_ns === 'string' && /^\d{1,20}$/.test(f.recv_utc_ns) ? f.recv_utc_ns : null,
    });
  }
  return {
    artifactId: j.artifact_id,
    captureId: j.capture_id,
    roi: j.roi,
    seq: isInt(j.transition_seq) ? j.transition_seq : null,
    partial: Array.isArray(j.partial) ? (j.partial as unknown[]).filter((x): x is string => typeof x === 'string') : [],
    frames,
  };
}

// ---- book (M6) ----
export class RoiBook {
  config: RoiConfigView | null = null;
  /** latest state per `${captureId}\u0000${roi}` */
  readonly states = new Map<string, RoiStateView>();
  /** every received transition, oldest first, bounded ROI_HISTORY_MAX */
  readonly history: RoiStateView[] = [];
  /** baseline frame per `${captureId}\u0000${roi}` (from roi_state.baseline and roi_config change.op = baseline) */
  readonly baselines = new Map<string, number>();
  /** latest metric summary per capture */
  readonly metrics = new Map<string, RoiMetricView>();
  /** latest lifecycle per `${captureId}\u0000${n}`, bounded EVIDENCE_MAX (oldest inserted dropped) */
  readonly evidence = new Map<string, EvidenceView>();

  apply(kind: string, p: unknown, seq: bigint): boolean {
    switch (kind) {
      case 'video.roi_config': {
        const c = readRoiConfig(p, seq);
        if (!c) return false;
        if (this.config && this.config.seq > seq) return false;
        this.config = c;
        const o = p as Record<string, unknown>;
        const change = typeof o.change === 'object' && o.change !== null ? (o.change as Record<string, unknown>) : {};
        if (change.op === 'baseline' && typeof o.capture_id === 'string' && Array.isArray(change.baselines)) {
          for (const it of change.baselines as unknown[]) {
            const r = typeof it === 'object' && it !== null ? (it as Record<string, unknown>) : {};
            const b = typeof r.baseline === 'object' && r.baseline !== null ? (r.baseline as Record<string, unknown>) : {};
            if (typeof r.roi === 'string' && typeof b.frame_id === 'number' && Number.isSafeInteger(b.frame_id) && b.frame_id >= 0) {
              this.baselines.set(`${o.capture_id}\u0000${r.roi}`, b.frame_id);
            }
          }
        }
        return true;
      }
      case 'video.roi_judgment': {
        const s = readRoiState(p);
        if (!s) return false;
        const key = `${s.captureId}\u0000${s.roi}`;
        const old = this.states.get(key);
        if (old && old.seq >= s.seq) return false;
        this.states.set(key, s);
        this.history.push(s);
        while (this.history.length > ROI_HISTORY_MAX) this.history.shift();
        if (s.baselineFrameId !== null) this.baselines.set(key, s.baselineFrameId);
        return true;
      }
      case 'video.roi_metric': {
        const m = readRoiMetric(p);
        if (!m) return false;
        this.metrics.set(m.captureId, m);
        return true;
      }
      case 'video.evidence': {
        const e = readEvidence(p);
        if (!e) return false;
        const key = `${e.captureId}\u0000${e.n}`;
        if (!this.evidence.has(key) && this.evidence.size >= EVIDENCE_MAX) {
          const oldest = this.evidence.keys().next();
          if (!oldest.done) this.evidence.delete(oldest.value);
        }
        this.evidence.set(key, e);
        return true;
      }
      default:
        return false;
    }
  }
  /** Reducer snapshot `state` object: `{roi_config, roi_states, roi_metrics, evidence}`. */
  applySnapshotState(state: unknown, throughSeq: bigint): void {
    const s = typeof state === 'object' && state !== null ? (state as Record<string, unknown>) : {};
    if (typeof s.roi_config === 'object' && s.roi_config !== null) this.apply('video.roi_config', s.roi_config, throughSeq);
    if (Array.isArray(s.roi_states)) {
      for (const item of s.roi_states as unknown[]) {
        const st = readRoiState(item);
        if (!st) continue;
        const key = `${st.captureId}\u0000${st.roi}`;
        const old = this.states.get(key);
        if (old && old.seq >= st.seq) continue;
        this.states.set(key, st);
        if (st.baselineFrameId !== null) this.baselines.set(key, st.baselineFrameId);
      }
    }
    if (Array.isArray(s.roi_metrics)) for (const m of s.roi_metrics as unknown[]) this.apply('video.roi_metric', m, throughSeq);
    if (Array.isArray(s.evidence)) for (const e of s.evidence as unknown[]) this.apply('video.evidence', e, throughSeq);
  }
  rows(captureId: string | null): RoiRow[] {
    if (!this.config) return [];
    return this.config.rois.map((spec) => {
      const key = captureId === null ? '' : `${captureId}\u0000${spec.name}`;
      const s = captureId === null ? undefined : this.states.get(key);
      const m = captureId === null ? undefined : this.metrics.get(captureId)?.rois.find((r) => r.roi === spec.name);
      const b = captureId === null ? undefined : this.baselines.get(key);
      return {
        name: spec.name,
        state: s?.state ?? 'unknown',
        reason: s ? s.reason : 'no_state',
        chip: roiChip(s),
        value: fmtMetrics(m?.metrics ?? s?.metrics ?? null, spec.metric),
        threshold: `진입 ≥ ${spec.enter} · 해제 ≤ ${spec.exit}`,
        baseline: b !== undefined ? `기준 frame ${b}` : '기준 없음',
        seq: s?.seq ?? 0,
      };
    });
  }
  historyFor(captureId: string | null): RoiStateView[] {
    if (captureId === null) return [];
    return this.history.filter((h) => h.captureId === captureId).reverse();
  }
  evidenceFor(captureId: string | null): EvidenceView[] {
    if (captureId === null) return [];
    return [...this.evidence.values()].filter((e) => e.captureId === captureId).sort((a, b) => b.n - a.n);
  }
}

// ---- text (M8) ----
export function roiChip(s: RoiStateView | undefined): string {
  if (s === undefined) return '판정 불가 · 상태 없음';
  if (s.state === 'normal') return '정상';
  if (s.state === 'changed') return '변화';
  return `판정 불가 · ${s.reason === null ? '?' : (ROI_REASON_LABEL[s.reason] ?? s.reason)}`;
}
export function fmtMetrics(m: RoiMetricsView | null, metric: string): string {
  if (m === null) return '—';
  const v = metric === 'luma_delta' ? Math.abs(m.lumaDelta) : metric === 'changed_px' ? m.changedPx : m.meanAbsDiff;
  return `${METRIC_LABEL[metric] ?? metric} ${v.toFixed(1)} · 평균 밝기 ${m.meanLuma.toFixed(1)}`;
}
export function inspectLine(m: RoiMetricView | undefined, configRev: number | null): string {
  if (m === undefined) return `검사 대기 · 설정 rev ${configRev ?? '—'} · ${PIXEL_NOTE_KO}`;
  return `검사 ≤${m.hzMax ?? '?'} Hz · 분석 ${m.analysed} · 건너뜀 ${m.skipped} · 디코드 실패 ${m.decodeErrors} · 설정 rev ${m.configRev ?? configRev ?? '—'} · ${PIXEL_NOTE_KO}`;
}
export function historyText(s: RoiStateView): string {
  return `#${s.seq} ${s.roi} → ${roiChip(s)} · frame ${s.frameId ?? '—'} · ${fmtWallNs(s.recvUtcNs)}`;
}
export function evidenceText(e: EvidenceView): string {
  const L = EVIDENCE_STATE_LABEL[e.state] ?? e.state;
  if (e.state === 'collecting') return `증거 #${e.n} ${e.roi} ${L} · trigger frame ${e.triggerFrameId ?? '—'}`;
  if (e.state === 'written') {
    return `증거 #${e.n} ${e.roi} ${L} · ${e.frames.length} frames${e.partial.length > 0 ? ` · 부분: ${e.partial.join(', ')}` : ''}`;
  }
  return `증거 #${e.n} ${e.roi} ${L} · ${e.reason ?? '?'}`;
}

// ---- editor / bytes (M9–M10) ----
export function roiFormSpec(f: RoiForm): { roi: Record<string, unknown> } | string {
  const name = f.name.trim();
  if (!/^[A-Za-z0-9_-]{1,32}$/.test(name)) return 'ROI 이름: 영문·숫자·_·- 1~32자';
  const parts = f.rect.split(',').map((x) => x.trim());
  const nums = parts.map((x) => (x === '' ? NaN : Number(x)));
  if (parts.length !== 4 || nums.some((n) => !Number.isFinite(n))) return 'ROI 영역: x,y,w,h (0~1 비율)';
  const metric = f.metric.trim();
  if (!(METRICS as readonly string[]).includes(metric)) return '측정 방식을 고르세요';
  const enterT = f.enter.trim();
  const enter = enterT === '' ? NaN : Number(enterT);
  if (!Number.isFinite(enter) || enter <= 0) return '진입 임계값: 0보다 큰 수';
  const exitT = f.exit.trim();
  const exit = exitT === '' ? undefined : Number(exitT);
  if (exit !== undefined && (!Number.isFinite(exit) || exit < 0)) return '해제 임계값: 0 이상 수';
  const dwellT = f.dwell.trim();
  if (dwellT !== '' && !/^\d+$/.test(dwellT)) return '대기 시간: ms 정수';
  const coolT = f.cooldown.trim();
  if (coolT !== '' && !/^\d+$/.test(coolT)) return '재전환 간격: ms 정수';
  const roi: Record<string, unknown> = { name, x: nums[0], y: nums[1], w: nums[2], h: nums[3], metric, enter };
  if (exit !== undefined) roi.exit = exit;
  if (dwellT !== '') roi.dwell_ms = Number(dwellT);
  if (coolT !== '') roi.cooldown_ms = Number(coolT);
  return { roi };
}
export function b64Bytes(b64: string): Uint8Array | null {
  try {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}
