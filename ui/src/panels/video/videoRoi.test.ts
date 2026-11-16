// @vitest-environment jsdom
// Operator upgrade B6 (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 6) — Opus-authored spec tests.
// V-01/V-02 UI: ROI list/overlay/history render the daemon's judgments verbatim (the UI never computes a
// metric or a state), UNKNOWN reasons visible, inspection rate shown apart from render fps;
// V-04: evidence lifecycle + in-panel evidence viewer with per-frame sha256 verification and ◀▶;
// editor/baseline go through declared actions with expect_rev. Sonnet must not edit this file.
import { createHash, webcrypto } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getPanel } from '../../sdk/registry';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import {
  b64Bytes,
  EVIDENCE_MAX,
  evidenceText,
  fmtMetrics,
  historyText,
  inspectLine,
  PIXEL_NOTE_KO,
  readEvidence,
  readEvidenceManifest,
  readRoiConfig,
  readRoiMetric,
  readRoiState,
  ROI_HISTORY_MAX,
  ROI_KINDS,
  RoiBook,
  roiChip,
  roiFormSpec,
  type RoiStateView,
} from './roiView';

const CAP = 'v0123456789ab';
const SHA = 'ab'.repeat(32);
const pad = (n: number, w = 2) => String(n).padStart(w, '0');
const localHms = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
};
const UTC_MS = 1_762_770_000_123;
const UTC = String(BigInt(UTC_MS) * 1_000_000n);

const spec = (name: string, o: Record<string, unknown> = {}) => ({
  name, x: 0.25, y: 0.25, w: 0.5, h: 0.5, metric: 'mean_abs_diff', enter: 30, exit: 10, dwell_ms: 0, cooldown_ms: 0,
  pixel_threshold: 24, evidence: true, evidence_pre: 2, evidence_post: 2, ...o,
});
const cfgP = (rev: number, rois: unknown[], o: Record<string, unknown> = {}) => ({ capture_id: CAP, capture_epoch: '1', config_rev: rev, rois, change: { op: 'put', roi: 'lamp', expect_rev: null }, note: 'x', ...o });
const metricsP = (mad: number) => ({ mean_luma: 200.04, luma_delta: -12.5, mean_abs_diff: mad, changed_px: 88.25 });
const stateP = (roi: string, seq: number, state: string, reason: string | null, o: Record<string, unknown> = {}) => ({
  capture_id: CAP, capture_epoch: '1', roi, config_rev: 1, state, reason, transition_seq: seq,
  transition: { from: 'unknown', from_reason: null, to: state, reason, transition_seq: seq, mono_ns: 0, value: null },
  frame_id: 40 + seq, sha256: SHA, recv_utc_ns: UTC, recv_mono_ns: '0', metrics: null, baseline: null, evidence: false, note: 'x', ...o,
});
const metricP = (rois: unknown[], o: Record<string, unknown> = {}) => ({
  capture_id: CAP, capture_epoch: '1', config_rev: 1, analysis: { analysed: 120, skipped: 7, decode_errors: 1, last_frame_id: 130 }, rois,
  inspection_hz_max: 10, note: 'x', ...o,
});
const evidP = (n: number, state: string, o: Record<string, unknown> = {}) => ({ capture_id: CAP, capture_epoch: '1', evidence_id: n, state, roi: 'lamp', transition_seq: 4, ...o });

// ------------------------------------------------------------------------------------------------
describe('B6-M readers', () => {
  it('M1 roi_config: specs read with defaults, invalid entries skipped, malformed payload refused', () => {
    const c = readRoiConfig(cfgP(3, [spec('lamp'), { name: 'bad name', x: 0, y: 0, w: 1, h: 1, enter: 1 }, { name: 'min', x: 0, y: 0, w: 1, h: 1, enter: 8 }]), 9n);
    expect(c).toEqual({
      rev: 3,
      captureId: CAP,
      seq: 9n,
      rois: [
        { name: 'lamp', x: 0.25, y: 0.25, w: 0.5, h: 0.5, metric: 'mean_abs_diff', enter: 30, exit: 10, dwellMs: 0, cooldownMs: 0, evidence: true },
        { name: 'min', x: 0, y: 0, w: 1, h: 1, metric: 'mean_abs_diff', enter: 8, exit: 4, dwellMs: 300, cooldownMs: 1000, evidence: true },
      ],
    });
    expect(readRoiConfig(cfgP(0, [], { capture_id: null }), 1n)?.captureId).toBeNull();
    for (const bad of [null, {}, { config_rev: -1, rois: [] }, { config_rev: 1, rois: {} }, { config_rev: '1', rois: [] }]) expect(readRoiConfig(bad, 1n)).toBeNull();
  });

  it('M2 roi_state: identity, judgment and evidence flag; malformed → null', () => {
    const s = readRoiState(stateP('lamp', 4, 'changed', null, { metrics: metricsP(99.5), baseline: { frame_id: 7, sha256: SHA }, evidence: true }));
    expect(s).toEqual({
      captureId: CAP, roi: 'lamp', state: 'changed', reason: null, seq: 4, frameId: 44, sha256: SHA, recvUtcNs: UTC,
      metrics: { meanLuma: 200.04, lumaDelta: -12.5, meanAbsDiff: 99.5, changedPx: 88.25 }, baselineFrameId: 7, configRev: 1, evidence: true,
    });
    const u = readRoiState(stateP('lamp', 2, 'unknown', 'stale', { frame_id: null, sha256: null, recv_utc_ns: null }));
    expect(u).toMatchObject({ state: 'unknown', reason: 'stale', frameId: null, sha256: null, recvUtcNs: null, metrics: null, baselineFrameId: null, evidence: false });
    expect(readRoiState(stateP('lamp', 1, 'normal', null, { metrics: { mean_luma: 1 }, sha256: 'xyz' }))).toMatchObject({ metrics: null, sha256: null });
    for (const bad of [null, { ...stateP('lamp', 1, 'on', null) }, { ...stateP('lamp', 1, 'normal', null), capture_id: 5 }, { ...stateP('lamp', 1, 'normal', null), transition_seq: -1 }, { ...stateP('lamp', 1, 'normal', null), roi: 3 }]) {
      expect(readRoiState(bad)).toBeNull();
    }
  });

  it('M3 roi_metric: counters, inspection rate, rows; bad rows skipped', () => {
    const m = readRoiMetric(metricP([{ roi: 'lamp', state: 'normal', reason: null, transition_seq: 3, metrics: metricsP(2.5), metrics_frame_id: 130 }, { roi: 'x', state: 'weird', transition_seq: 1 }]));
    expect(m).toEqual({
      captureId: CAP, configRev: 1, analysed: 120, skipped: 7, decodeErrors: 1, lastFrameId: 130, hzMax: 10,
      rois: [{ roi: 'lamp', state: 'normal', reason: null, seq: 3, metrics: { meanLuma: 200.04, lumaDelta: -12.5, meanAbsDiff: 2.5, changedPx: 88.25 }, metricsFrameId: 130 }],
    });
    expect(readRoiMetric({ capture_id: CAP, rois: [] })).toEqual({ captureId: CAP, configRev: null, analysed: 0, skipped: 0, decodeErrors: 0, lastFrameId: null, hzMax: null, rois: [] });
    expect(readRoiMetric({ rois: [] })).toBeNull();
  });

  it('M4 evidence lifecycle rows; strict artifact ids', () => {
    const w = readEvidence(evidP(1, 'written', { artifact_id: `evid-${CAP}-1`, frames: [{ role: 'pre', frame_id: 8, sha256: SHA, size: 10 }, { role: 'x', frame_id: 9 }], partial: ['post_deadline', 3], size: 999 }));
    expect(w).toEqual({
      captureId: CAP, n: 1, state: 'written', roi: 'lamp', seq: 4, artifactId: `evid-${CAP}-1`, triggerFrameId: null,
      frames: [{ role: 'pre', frameId: 8, sha256: SHA, size: 10 }], partial: ['post_deadline'], reason: null, size: 999,
    });
    expect(readEvidence(evidP(2, 'collecting', { trigger_frame_id: 50 }))).toMatchObject({ state: 'collecting', triggerFrameId: 50, artifactId: null, frames: [], partial: [] });
    expect(readEvidence(evidP(3, 'written', { artifact_id: '../etc/passwd' }))?.artifactId).toBeNull();
    expect(readEvidence(evidP(4, 'skipped', { reason: 'evidence_busy' }))?.reason).toBe('evidence_busy');
    for (const bad of [null, evidP(0, 'written'), evidP(1, 'done'), { ...evidP(1, 'written'), roi: null }]) expect(readEvidence(bad)).toBeNull();
  });

  it('M5 manifest: strict schema and frames (viewer input)', () => {
    const f = { role: 'trigger', frame_id: 9, sha256: SHA, size: 3, recv_utc_ns: UTC, recv_mono_ns: '0', jpeg_b64: 'AAEC' };
    const m = { schema: 'loom.video.evidence/1', artifact_id: `evid-${CAP}-1`, capture_id: CAP, roi: 'lamp', transition_seq: 4, partial: [], frames: [f] };
    expect(readEvidenceManifest(m)).toEqual({ artifactId: `evid-${CAP}-1`, captureId: CAP, roi: 'lamp', seq: 4, partial: [], frames: [{ role: 'trigger', frameId: 9, sha256: SHA, size: 3, b64: 'AAEC', recvUtcNs: UTC }] });
    expect(readEvidenceManifest({ ...m, schema: 'other/1' })).toBeNull();
    expect(readEvidenceManifest({ ...m, frames: [{ ...f, jpeg_b64: 5 }] })).toBeNull();
    expect(readEvidenceManifest({ ...m, frames: [{ ...f, sha256: 'nothex' }] })).toBeNull();
    expect(readEvidenceManifest(null)).toBeNull();
  });
});

describe('B6-M book and text', () => {
  it('M6 book: config by seq, states deduped by transition_seq, bounded history, baselines, metrics, bounded evidence', () => {
    const b = new RoiBook();
    expect(b.apply('video.roi_config', cfgP(1, [spec('lamp')]), 5n)).toBe(true);
    expect(b.apply('video.roi_config', cfgP(0, []), 4n)).toBe(false);
    expect(b.config?.rev).toBe(1);
    expect(b.apply('video.roi_judgment', stateP('lamp', 1, 'unknown', 'no_baseline'), 6n)).toBe(true);
    expect(b.apply('video.roi_judgment', stateP('lamp', 1, 'unknown', 'no_baseline'), 7n)).toBe(false);
    expect(b.history).toHaveLength(1);
    // baseline announced by roi_config change.op = baseline (no transition needed)
    b.apply('video.roi_config', cfgP(1, [spec('lamp')], { change: { op: 'baseline', roi: null, frame_id: 33, sha256: SHA, baselines: [{ roi: 'lamp', baseline: { frame_id: 33 }, reason: null }, { roi: 'tiny', baseline: null, reason: 'roi_too_small' }] } }), 8n);
    expect(b.baselines.get(`${CAP}\u0000lamp`)).toBe(33);
    expect(b.baselines.has(`${CAP}\u0000tiny`)).toBe(false);
    b.apply('video.roi_judgment', stateP('lamp', 2, 'normal', null, { baseline: { frame_id: 35, sha256: SHA } }), 9n);
    expect(b.baselines.get(`${CAP}\u0000lamp`)).toBe(35);
    for (let i = 3; i < 3 + ROI_HISTORY_MAX + 5; i++) b.apply('video.roi_judgment', stateP('lamp', i, i % 2 ? 'changed' : 'normal', null), BigInt(10 + i));
    expect(b.history).toHaveLength(ROI_HISTORY_MAX);
    expect(b.history.at(-1)?.seq).toBe(3 + ROI_HISTORY_MAX + 4);
    expect(b.apply('video.roi_metric', metricP([]), 200n)).toBe(true);
    expect(b.metrics.get(CAP)?.analysed).toBe(120);
    for (let n = 1; n <= EVIDENCE_MAX + 2; n++) b.apply('video.evidence', evidP(n, 'collecting'), BigInt(300 + n));
    expect(b.evidence.size).toBe(EVIDENCE_MAX);
    expect(b.evidence.has(`${CAP}\u00001`)).toBe(false);
    b.apply('video.evidence', evidP(EVIDENCE_MAX + 2, 'written', { artifact_id: `evid-${CAP}-${EVIDENCE_MAX + 2}` }), 400n);
    expect(b.evidence.get(`${CAP}\u0000${EVIDENCE_MAX + 2}`)?.state).toBe('written');
    expect(b.apply('video.health', {}, 1n)).toBe(false);
    // late joiner: reducer snapshot state
    const j = new RoiBook();
    j.applySnapshotState({ roi_config: cfgP(2, [spec('lamp')]), roi_states: [stateP('lamp', 5, 'changed', null)], roi_metrics: [metricP([])], evidence: [evidP(1, 'written', { artifact_id: `evid-${CAP}-1` })] }, 50n);
    expect(j.config?.rev).toBe(2);
    expect(j.states.get(`${CAP}\u0000lamp`)?.state).toBe('changed');
    expect(j.history).toHaveLength(0);
    expect(j.metrics.has(CAP)).toBe(true);
    expect(j.evidenceFor(CAP)).toHaveLength(1);
    j.applySnapshotState({ roi_config: null, roi_states: 'x' }, 60n);
    expect(j.config?.rev).toBe(2);
  });

  it('M7 rows: config order, daemon state verbatim, metrics from the 1 s summary, no_state before any event', () => {
    const b = new RoiBook();
    expect(b.rows(CAP)).toEqual([]);
    b.apply('video.roi_config', cfgP(1, [spec('lamp'), spec('door', { metric: 'luma_delta', enter: 50, exit: 20 })]), 1n);
    expect(b.rows(null).map((r) => [r.name, r.state, r.reason])).toEqual([['lamp', 'unknown', 'no_state'], ['door', 'unknown', 'no_state']]);
    b.apply('video.roi_judgment', stateP('lamp', 3, 'changed', null, { metrics: metricsP(95), baseline: { frame_id: 7, sha256: SHA } }), 2n);
    b.apply('video.roi_judgment', stateP('door', 1, 'unknown', 'baseline_mismatch'), 3n);
    b.apply('video.roi_metric', metricP([{ roi: 'lamp', state: 'changed', reason: null, transition_seq: 3, metrics: metricsP(97.25), metrics_frame_id: 130 }]), 4n);
    expect(b.rows(CAP)).toEqual([
      { name: 'lamp', state: 'changed', reason: null, chip: '변화', value: '평균 차 % 97.3 · 평균 밝기 200.0', threshold: '진입 ≥ 30 · 해제 ≤ 10', baseline: '기준 frame 7', seq: 3 },
      { name: 'door', state: 'unknown', reason: 'baseline_mismatch', chip: '판정 불가 · 기준 불일치', value: '—', threshold: '진입 ≥ 50 · 해제 ≤ 20', baseline: '기준 없음', seq: 1 },
    ]);
    expect(b.rows('v999999999999').map((r) => r.reason)).toEqual(['no_state', 'no_state']);
    expect(b.historyFor(CAP).map((s) => s.roi)).toEqual(['door', 'lamp']);
    expect(b.historyFor(null)).toEqual([]);
    b.apply('video.evidence', evidP(1, 'written'), 5n);
    b.apply('video.evidence', evidP(2, 'collecting'), 6n);
    expect(b.evidenceFor(CAP).map((e) => e.n)).toEqual([2, 1]);
  });

  it('M8 text tables', () => {
    expect(roiChip(undefined)).toBe('판정 불가 · 상태 없음');
    const s = (state: string, reason: string | null) => readRoiState(stateP('lamp', 1, state, reason)) as RoiStateView;
    expect(roiChip(s('normal', null))).toBe('정상');
    expect(roiChip(s('changed', null))).toBe('변화');
    expect(roiChip(s('unknown', 'analysis_gap'))).toBe('판정 불가 · 분석 공백');
    expect(roiChip(s('unknown', 'new_reason'))).toBe('판정 불가 · new_reason');
    expect(roiChip(s('unknown', null))).toBe('판정 불가 · ?');
    const m = { meanLuma: 10, lumaDelta: -12.34, meanAbsDiff: 5.55, changedPx: 40 };
    expect(fmtMetrics(null, 'mean_abs_diff')).toBe('—');
    expect(fmtMetrics(m, 'luma_delta')).toBe('밝기 차 12.3 · 평균 밝기 10.0');
    expect(fmtMetrics(m, 'changed_px')).toBe('변화 픽셀 % 40.0 · 평균 밝기 10.0');
    expect(fmtMetrics(m, 'mean_abs_diff')).toBe('평균 차 % 5.5 · 평균 밝기 10.0');
    expect(inspectLine(undefined, null)).toBe(`검사 대기 · 설정 rev — · ${PIXEL_NOTE_KO}`);
    expect(inspectLine(undefined, 4)).toBe(`검사 대기 · 설정 rev 4 · ${PIXEL_NOTE_KO}`);
    expect(inspectLine(readRoiMetric(metricP([]))!, 9)).toBe(`검사 ≤10 Hz · 분석 120 · 건너뜀 7 · 디코드 실패 1 · 설정 rev 1 · ${PIXEL_NOTE_KO}`);
    expect(inspectLine(readRoiMetric({ capture_id: CAP, rois: [] })!, 9)).toBe(`검사 ≤? Hz · 분석 0 · 건너뜀 0 · 디코드 실패 0 · 설정 rev 9 · ${PIXEL_NOTE_KO}`);
    expect(historyText(readRoiState(stateP('lamp', 4, 'changed', null))!)).toBe(`#4 lamp → 변화 · frame 44 · ${localHms(UTC_MS)}`);
    expect(historyText(readRoiState(stateP('lamp', 5, 'unknown', 'not_live', { frame_id: null, recv_utc_ns: null }))!)).toBe('#5 lamp → 판정 불가 · 캡처 비활성 · frame — · —');
    expect(evidenceText(readEvidence(evidP(2, 'collecting', { trigger_frame_id: 50 }))!)).toBe('증거 #2 lamp 수집 중 · trigger frame 50');
    const fr = { role: 'pre', frame_id: 8, sha256: SHA, size: 1 };
    expect(evidenceText(readEvidence(evidP(1, 'written', { frames: [fr, fr] }))!)).toBe('증거 #1 lamp 저장됨 · 2 frames');
    expect(evidenceText(readEvidence(evidP(1, 'written', { frames: [fr], partial: ['post_deadline', 'bytes_cap'] }))!)).toBe('증거 #1 lamp 저장됨 · 1 frames · 부분: post_deadline, bytes_cap');
    expect(evidenceText(readEvidence(evidP(3, 'skipped', { reason: 'evidence_busy' }))!)).toBe('증거 #3 lamp 건너뜀 · evidence_busy');
    expect(evidenceText(readEvidence(evidP(4, 'aborted'))!)).toBe('증거 #4 lamp 중단 · ?');
  });

  it('M9 editor form → put params (daemon validates again); M10 base64', () => {
    const f = { name: ' lamp ', rect: '0.1, 0.2,0.3,0.4', metric: 'changed_px', enter: '12.5', exit: '', dwell: '', cooldown: '' };
    expect(roiFormSpec(f)).toEqual({ roi: { name: 'lamp', x: 0.1, y: 0.2, w: 0.3, h: 0.4, metric: 'changed_px', enter: 12.5 } });
    expect(roiFormSpec({ ...f, exit: '3', dwell: '500', cooldown: '0' })).toEqual({ roi: { name: 'lamp', x: 0.1, y: 0.2, w: 0.3, h: 0.4, metric: 'changed_px', enter: 12.5, exit: 3, dwell_ms: 500, cooldown_ms: 0 } });
    expect(roiFormSpec({ ...f, name: 'a b' })).toBe('ROI 이름: 영문·숫자·_·- 1~32자');
    expect(roiFormSpec({ ...f, rect: '0.1,0.2,0.3' })).toBe('ROI 영역: x,y,w,h (0~1 비율)');
    expect(roiFormSpec({ ...f, rect: '0.1,,0.3,0.4' })).toBe('ROI 영역: x,y,w,h (0~1 비율)');
    expect(roiFormSpec({ ...f, metric: 'ocr' })).toBe('측정 방식을 고르세요');
    expect(roiFormSpec({ ...f, enter: '0' })).toBe('진입 임계값: 0보다 큰 수');
    expect(roiFormSpec({ ...f, exit: '-1' })).toBe('해제 임계값: 0 이상 수');
    expect(roiFormSpec({ ...f, dwell: '1.5' })).toBe('대기 시간: ms 정수');
    expect(roiFormSpec({ ...f, cooldown: 'x' })).toBe('재전환 간격: ms 정수');
    expect(Array.from(b64Bytes('AAEC')!)).toEqual([0, 1, 2]);
    expect(b64Bytes('')).toEqual(new Uint8Array(0));
    expect(b64Bytes('*not base64*')).toBeNull();
  });
});

// ------------------------------------------------------------------------------------------------
// Panel (jsdom)

let h: Harness | null = null;
const JPEG = new Uint8Array([0xff, 0xd8, 1, 2, 3, 0xff, 0xd9]);
const JPEG_SHA = createHash('sha256').update(JPEG).digest('hex');
const B64 = Buffer.from(JPEG).toString('base64');
let artifact: () => Response = () => new Response('{"error":"no_media"}', { status: 404 });
const fetched: string[] = [];

function setup() {
  fetched.length = 0;
  vi.stubGlobal('crypto', webcrypto);
  vi.stubGlobal(
    'fetch',
    vi.fn(async (u: string) => {
      const url = new URL(String(u));
      fetched.push(url.pathname);
      if (url.pathname === `/api/v1/video/${CAP}/frame`)
        return new Response(JPEG, { status: 200, headers: { 'content-type': 'image/jpeg', 'x-loom-frame-id': '42', 'x-loom-frame-sha256': JPEG_SHA, 'x-loom-recv-utc-ns': UTC } });
      if (url.pathname.startsWith('/api/v1/video/artifact/')) return artifact();
      return new Response('{"error":"no_media"}', { status: 404 });
    }),
  );
  vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ width: 2, height: 2, close: vi.fn() })));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage: vi.fn() } as never);
}
afterEach(() => {
  h?.dispose();
  h = null;
  artifact = () => new Response('{"error":"no_media"}', { status: 404 });
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const q = <E extends Element = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const qa = (id: string) => [...h!.host.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];
const cfg = { source: 'http-mjpeg', source_label: 'http://127.0.0.1:64684/stream', width: 640, height: 360, fps: 15, quality: 5, audio: false };
const liveEv = () => ev({ kind: 'video.capture_state', channel: CAP, payload: { capture_id: CAP, capture_epoch: '1', state: 'live', reason: null, config: cfg } });
const roiEv = (kind: string, payload: Record<string, unknown>, channel: string = CAP) => ev({ kind, channel, payload });
const callsOf = (method: string) => h!.net.calls.filter((c) => c.method === method).map((c) => c.params);
const field = <E extends HTMLElement>(name: string) => q('video-roi-editor').querySelector<E>(`[name="${name}"]`)!;
const type = (name: string, v: string) => {
  const i = field<HTMLInputElement | HTMLSelectElement>(name);
  i.value = v;
  i.dispatchEvent(new Event('input'));
  i.dispatchEvent(new Event('change'));
};

async function mountLive() {
  await import('./videoPanel');
  h = mountPanel('video.workbench', {});
  for (const m of ['video.roi.put', 'video.roi.remove', 'video.roi.baseline']) h.net.responders.set(m, () => ({ data: {} }));
  h.batch([liveEv()]);
  await vi.waitFor(() => {
    h!.frame();
    expect(q('video-frame-id').textContent).toBe('frame_id 42');
  });
  return h;
}

describe('B6-P manifest', () => {
  it('ROI actions need operate; the topic subscribes the ROI kinds', async () => {
    await import('./videoPanel');
    const m = getPanel('video.workbench')!.manifest;
    const a = Object.fromEntries(m.actions.map((x) => [x.id, x]));
    expect(a.roi_put).toMatchObject({ rpc: 'video.roi.put', class: 'request', right: 'operate', confirm: 'none' });
    expect(a.roi_remove).toMatchObject({ rpc: 'video.roi.remove', right: 'operate' });
    expect(a.roi_baseline).toMatchObject({ rpc: 'video.roi.baseline', right: 'operate' });
    const kinds = m.topics[0]!.filter({}).kinds ?? [];
    for (const k of ROI_KINDS) expect(kinds).toContain(k);
    // Shared channel-status rule (ui/src/store/channels.ts isStateKind and loom-gateway hub is_channel_state):
    // `<domain>.state` / `<domain>.*_state` on a channel is connectivity. Panels may not import store/ (I-UI1),
    // so the predicate is mirrored here verbatim; ROI kinds must never match it (amendment A6).
    const isStateKind = (kind: string) => {
      const dot = kind.indexOf('.');
      if (dot <= 0) return false;
      const e = kind.slice(dot + 1);
      return e === 'state' || e.endsWith('_state');
    };
    expect(isStateKind('video.capture_state')).toBe(true);
    for (const k of ROI_KINDS) expect(isStateKind(k), k).toBe(false);
    expect(ROI_KINDS).toEqual(['video.roi_config', 'video.roi_judgment', 'video.roi_metric', 'video.evidence']);
  });
});

describe('B6-P V-01/V-02 judgments rendered verbatim', () => {
  it('list, chips, overlay, history and inspection line come only from daemon events', async () => {
    setup();
    await mountLive();
    expect(q('video-roi-list').children).toHaveLength(0);
    expect(q('video-roi-meta').textContent).toBe(`검사 대기 · 설정 rev — · ${PIXEL_NOTE_KO}`);
    h!.batch([
      roiEv('video.roi_config', cfgP(1, [spec('lamp'), spec('door', { x: 0, y: 0, w: 0.25, h: 0.25 })])),
      roiEv('video.roi_judgment', stateP('lamp', 3, 'changed', null, { metrics: metricsP(95), baseline: { frame_id: 7, sha256: SHA } })),
      roiEv('video.roi_judgment', stateP('door', 2, 'unknown', 'stale')),
      roiEv('video.roi_metric', metricP([{ roi: 'lamp', state: 'changed', reason: null, transition_seq: 3, metrics: metricsP(97.25), metrics_frame_id: 130 }])),
    ]);
    const rows = qa('video-roi-row');
    expect(rows.map((r) => [r.dataset.roi, r.dataset.state])).toEqual([['lamp', 'changed'], ['door', 'unknown']]);
    expect(qa('video-roi-chip').map((e) => e.textContent)).toEqual(['변화', '판정 불가 · 수신 정체']);
    expect(qa('video-roi-value')[0]!.textContent).toBe('평균 차 % 97.3 · 평균 밝기 200.0');
    expect(qa('video-roi-baseline-frame').map((e) => e.textContent)).toEqual(['기준 frame 7', '기준 없음']);
    expect(q('video-roi-meta').textContent).toBe(`검사 ≤10 Hz · 분석 120 · 건너뜀 7 · 디코드 실패 1 · 설정 rev 1 · ${PIXEL_NOTE_KO}`);
    expect(q('video-health').textContent).toContain('표시'); // render fps stays in the health line, separate from inspection
    const svg = q<SVGSVGElement>('video-roi-overlay');
    expect(svg.namespaceURI).toBe('http://www.w3.org/2000/svg');
    expect(svg.getAttribute('viewBox')).toBe('0 0 1 1');
    const rects = [...svg.querySelectorAll('rect')];
    expect(rects.map((r) => [r.dataset.roi, r.dataset.state, r.getAttribute('x'), r.getAttribute('y'), r.getAttribute('width'), r.getAttribute('height')])).toEqual([
      ['lamp', 'changed', '0.25', '0.25', '0.5', '0.5'],
      ['door', 'unknown', '0', '0', '0.25', '0.25'],
    ]);
    expect(rects[0]!.querySelector('title')?.textContent).toBe('lamp');
    expect(qa('video-roi-hist-row').map((r) => r.textContent)).toEqual([
      `#2 door → 판정 불가 · 수신 정체 · frame 42 · ${localHms(UTC_MS)}`,
      `#3 lamp → 변화 · frame 43 · ${localHms(UTC_MS)}`,
    ]);
    // untrusted text never becomes markup
    h!.batch([roiEv('video.roi_judgment', stateP('lamp', 4, 'unknown', '<img src=x onerror=alert(1)>'))]);
    expect(h!.host.querySelector('img')).toBeNull();
    expect(qa('video-roi-chip')[0]!.textContent).toBe('판정 불가 · <img src=x onerror=alert(1)>');
    expect(h!.net.calls.filter((c) => c.method.startsWith('video.roi'))).toEqual([]);
  });

  it('a config edited without a capture (idle channel, capture_id null) is still shown', async () => {
    setup();
    await mountLive();
    h!.batch([roiEv('video.roi_config', cfgP(2, [spec('lamp')], { capture_id: null }), 'inspect')]);
    expect(qa('video-roi-row').map((r) => r.dataset.roi)).toEqual(['lamp']);
    expect(qa('video-roi-chip')[0]!.textContent).toBe('판정 불가 · 상태 없음');
  });
});

describe('B6-P editor and baseline actions', () => {
  it('put validates locally and sends expect_rev; remove per row; baseline = the exact frame on screen', async () => {
    setup();
    await mountLive();
    q<HTMLButtonElement>('video-roi-put').click();
    expect(q('video-roi-err').textContent).toBe('ROI 이름: 영문·숫자·_·- 1~32자');
    expect(callsOf('video.roi.put')).toEqual([]);
    expect(q<HTMLButtonElement>('video-roi-baseline').disabled).toBe(true); // no ROI configured yet
    type('roi-name', 'lamp');
    type('roi-rect', '0.25,0.25,0.5,0.5');
    type('roi-metric', 'mean_abs_diff');
    type('roi-enter', '30');
    q<HTMLButtonElement>('video-roi-put').click();
    await vi.waitFor(() => expect(callsOf('video.roi.put')).toEqual([{ roi: { name: 'lamp', x: 0.25, y: 0.25, w: 0.5, h: 0.5, metric: 'mean_abs_diff', enter: 30 } }]));
    expect(q('video-roi-err').textContent).toBe('');
    h!.batch([roiEv('video.roi_config', cfgP(5, [spec('lamp')]))]);
    q<HTMLButtonElement>('video-roi-put').click();
    await vi.waitFor(() => expect(callsOf('video.roi.put')).toHaveLength(2));
    expect(callsOf('video.roi.put')[1]).toMatchObject({ expect_rev: 5 });
    q<HTMLButtonElement>('video-roi-remove').click();
    await vi.waitFor(() => expect(callsOf('video.roi.remove')).toEqual([{ roi: 'lamp', expect_rev: 5 }]));
    const base = q<HTMLButtonElement>('video-roi-baseline');
    expect(base.disabled).toBe(false);
    expect(base.title).toBe('기준 = 화면의 frame 42');
    base.click();
    await vi.waitFor(() => expect(callsOf('video.roi.baseline')).toEqual([{ capture_id: CAP, frame_id: 42 }]));
  });
});

describe('B6-P V-04 evidence', () => {
  const manifest = (sha: string) => ({
    schema: 'loom.video.evidence/1', artifact_id: `evid-${CAP}-1`, capture_id: CAP, roi: 'lamp', transition_seq: 4, partial: [],
    frames: [
      { role: 'pre', frame_id: 40, sha256: JPEG_SHA, size: JPEG.length, recv_utc_ns: UTC, recv_mono_ns: '0', jpeg_b64: B64 },
      { role: 'trigger', frame_id: 41, sha256: sha, size: JPEG.length, recv_utc_ns: UTC, recv_mono_ns: '0', jpeg_b64: B64 },
      { role: 'post', frame_id: 42, sha256: JPEG_SHA, size: JPEG.length, recv_utc_ns: UTC, recv_mono_ns: '0', jpeg_b64: B64 },
    ],
  });

  it('lifecycle rows; view opens the trigger frame in-panel, verifies every sha256, ◀▶ walks the bundle', async () => {
    setup();
    artifact = () => new Response(JSON.stringify(manifest(JPEG_SHA)), { status: 200, headers: { 'content-type': 'application/json' } });
    await mountLive();
    h!.batch([roiEv('video.evidence', evidP(1, 'collecting', { trigger_frame_id: 41 }))]);
    expect(qa('video-evidence-row').map((r) => r.textContent)).toEqual(['증거 #1 lamp 수집 중 · trigger frame 41']);
    expect(qa('video-evidence-view')).toHaveLength(0);
    h!.batch([roiEv('video.evidence', evidP(1, 'written', { artifact_id: `evid-${CAP}-1`, frames: [{ role: 'trigger', frame_id: 41, sha256: JPEG_SHA, size: 7 }], partial: [] }))]);
    expect(qa('video-evidence-row')).toHaveLength(1);
    expect(q('video-evid-nav').hidden).toBe(true);
    q('video-evidence-view').click();
    const stage = q('video-canvas').parentElement!;
    await vi.waitFor(() => {
      h!.frame();
      expect(stage.dataset.view).toBe('artifact');
    });
    expect(fetched).toContain(`/api/v1/video/artifact/evid-${CAP}-1`);
    expect(q('video-evid-nav').hidden).toBe(false);
    expect(q('video-evid-label').textContent).toBe('증거 lamp · 2/3 trigger · frame 41 · sha256 일치');
    expect(q('video-frame-id').textContent).toBe(`frame_id 41 · 아티팩트 evid-${CAP}-1`);
    q('video-evid-prev').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-evid-label').textContent).toBe('증거 lamp · 1/3 pre · frame 40 · sha256 일치');
    });
    expect(q<HTMLButtonElement>('video-evid-prev').disabled).toBe(true);
    q('video-evid-next').click();
    q('video-evid-next').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-evid-label').textContent).toBe('증거 lamp · 3/3 post · frame 42 · sha256 일치');
    });
    expect(q<HTMLButtonElement>('video-evid-next').disabled).toBe(true);
    q('video-freeze').click(); // ▶ 실시간 leaves the viewer
    h!.frame();
    expect(stage.dataset.view).toBe('live');
    expect(q('video-evid-nav').hidden).toBe(true);
  });

  it('a frame whose bytes do not match its sha256 is flagged; a malformed manifest is refused', async () => {
    setup();
    artifact = () => new Response(JSON.stringify(manifest(SHA)), { status: 200, headers: { 'content-type': 'application/json' } });
    await mountLive();
    h!.batch([roiEv('video.evidence', evidP(1, 'written', { artifact_id: `evid-${CAP}-1` }))]);
    q('video-evidence-view').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-evid-label').textContent).toBe('증거 lamp · 2/3 trigger · frame 41 · sha256 불일치!');
    });
    expect(q('video-evid-label').dataset.ok).toBe('false');
    expect(q('video-frame-sha256').dataset.mismatch).toBe('true');
    q('video-freeze').click();
    artifact = () => new Response('{"schema":"nope"}', { status: 200, headers: { 'content-type': 'application/json' } });
    q('video-evidence-view').click();
    await vi.waitFor(() => expect(h!.toasts).toContain('증거 보기 실패: 형식 오류'));
    artifact = () => new Response('{"error":"not_found"}', { status: 404, headers: { 'content-type': 'application/json' } });
    q('video-evidence-view').click();
    await vi.waitFor(() => expect(h!.toasts).toContain('증거 보기 실패: 404 not_found'));
  });
});
