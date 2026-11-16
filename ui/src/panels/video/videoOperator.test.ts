// @vitest-environment jsdom
// Operator upgrade B1 (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 1) — Opus-authored spec tests.
// V-05 truthful ended health · V-06 in-panel artifact viewing, no bare artifact anchors · V-07 reuse settings ·
// V-08 replay/device lists only on explicit selection. Sonnet must not edit this file.
import { createHash, webcrypto } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import {
  artifactFileName,
  DEFAULT_FORM,
  fmtAgo,
  fmtWallNs,
  formFromCapture,
  healthLine,
  MAX_SOURCE_ITEMS,
  readCapture,
  readHealth,
  readSources,
  REUSE_HTTP_NOTE,
  sourceListText,
  VideoBook,
  type CaptureView,
  type HealthView,
} from './videoModel';

const CAP = 'v0123456789ab';
const pad = (n: number, w = 2) => String(n).padStart(w, '0');
const localHms = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
};
const nsOf = (ms: number) => String(BigInt(ms) * 1_000_000n);

const capture = (o: Partial<CaptureView> = {}): CaptureView => ({
  id: CAP,
  seq: 1n,
  epoch: '1794645164583021002',
  state: 'closed',
  reason: 'eof',
  sourceLabel: 'http://127.0.0.1:64684/stream',
  source: 'http-mjpeg',
  width: 640,
  height: 360,
  fps: 15,
  stoppedAtFrame: 1011,
  error: null,
  stderrTail: '',
  cleanupMs: 4,
  stateWallNs: null,
  ...o,
});
const health = (o: Partial<HealthView> = {}): HealthView => ({
  framesTotal: 1011,
  measuredFps: 3,
  requestedFps: 15,
  lastFrameId: 1011,
  lastRecvMonoNs: '9007199254740993',
  ageMs: 25,
  stale: false,
  oversize: 0,
  truncated: 1,
  junkBytes: 5,
  dup: 1,
  drop: 2,
  mediaSkipped: 3,
  ringFrames: 90,
  lastRecvUtcNs: null,
  state: 'live',
  ...o,
});
const X = { renderFps: 12, fetched: 4, sameSkipped: 1 };

// ------------------------------------------------------------------------------------------------
describe('B1-M pure model', () => {
  it('M6 V-05 regression: an ended capture never shows fresh/fps/age from its last live health', () => {
    const now = 1_762_770_000_000 + 16 * 60_000;
    const ended = nsOf(1_762_770_000_000);
    const line = healthLine(health(), capture({ stateWallNs: ended }), X, now);
    expect(line).toBe(`종료 · frames 1011 · 마지막 frame 1011 · 마지막 수신 — · 종료 시각 ${localHms(1_762_770_000_000)} (16분 전)`);
    for (const bad of ['fresh', 'STALE', '수신 3/', '25 ms']) expect(line).not.toContain(bad);
    // failed captures are ended too; a known last receive time is shown; unknown end time is not invented
    const f = healthLine(health({ lastRecvUtcNs: nsOf(1_762_769_999_123) }), capture({ state: 'failed', stoppedAtFrame: null }), X, now);
    expect(f).toBe(`종료 · frames 1011 · 마지막 frame 1011 · 마지막 수신 ${localHms(1_762_769_999_123)} · 종료 시각 —`);
    expect(healthLine(undefined, capture({ stoppedAtFrame: null }), X, now)).toBe('종료 · frames — · 마지막 frame — · 마지막 수신 — · 종료 시각 —');
  });

  it('M6 live line is unchanged byte for byte; no capture / no health → render fps only', () => {
    expect(healthLine(health(), capture({ state: 'live' }), X, 0)).toBe(
      'frames 1011 · 수신 3/15 fps · 표시 12 fps · 마지막 수신 25 ms · fresh · dup 1 / drop 2 · oversize 0 · truncated 1 · junk 5B · media skip 3 · ring 90 · fetched 4 · same 1',
    );
    expect(healthLine(health({ stale: true, ageMs: null, requestedFps: null }), capture({ state: 'stalled' }), X, 0)).toContain('수신 3/? fps · 표시 12 fps · 마지막 수신 — ms · STALE');
    expect(healthLine(undefined, capture({ state: 'opening' }), X, 0)).toBe('표시 12 fps');
    expect(healthLine(health(), null, { ...X, renderFps: 0 }, 0)).toBe('표시 0 fps');
  });

  it('M4/M5 time formatting tables', () => {
    expect(fmtWallNs('1762770000123000000')).toBe(localHms(1762770000123));
    for (const bad of [null, undefined, '', '12a', '-1', '012']) expect(fmtWallNs(bad)).toBe('—');
    const cases: [number, string][] = [
      [0, '0초'],
      [59_999, '59초'],
      [60_000, '1분'],
      [960_000, '16분'],
      [3_599_999, '59분'],
      [3_660_000, '1시간 1분'],
      [-5, '0초'],
      [Number.NaN, '0초'],
    ];
    for (const [ms, s] of cases) expect(fmtAgo(ms)).toBe(s);
  });

  it('M1/M2 capture_state wall time: plumbed from the event, ordered by seq, never invented for snapshots', () => {
    expect(readCapture({ capture_id: 'v1', state: 'closed', config: {} }, 3n, '1762770000000000000')?.stateWallNs).toBe('1762770000000000000');
    expect(readCapture({ capture_id: 'v1', state: 'closed', config: {} }, 3n, 'abc')?.stateWallNs).toBeNull();
    expect(readCapture({ capture_id: 'v1', state: 'closed', config: {} }, 3n)?.stateWallNs).toBeNull();
    const b = new VideoBook();
    expect(b.apply('video.capture_state', { capture_id: 'v1', state: 'closed', config: {} }, 10n, '2000')).toBe(true);
    expect(b.apply('video.capture_state', { capture_id: 'v1', state: 'live', config: {} }, 5n, '1000')).toBe(false);
    expect(b.captures.get('v1')).toMatchObject({ state: 'closed', stateWallNs: '2000' });
    b.applySnapshotItem({ domain: 'video', state: { captures: [{ capture_id: 'v2', state: 'closed', config: {} }] } }, 20n);
    expect(b.captures.get('v2')?.stateWallNs).toBeNull();
    // other kinds ignore the wall time argument
    expect(b.apply('video.health', { capture_id: 'v1', frames_total: 1 }, 30n, '3000')).toBe(true);
  });

  it('M3 new optional health fields default to null (old daemons) and are read when present', () => {
    expect(readHealth({ capture_id: 'v1', frames_total: 1 })).toMatchObject({ lastRecvUtcNs: null, state: null });
    expect(readHealth({ capture_id: 'v1', frames_total: 1, state: 'closed', last_recv_utc_ns: '18446744073709551615' })).toMatchObject({
      lastRecvUtcNs: '18446744073709551615',
      state: 'closed',
    });
    expect(readHealth({ capture_id: 'v1', last_recv_utc_ns: 2 ** 60 })?.lastRecvUtcNs).toBeNull();
  });

  it('M7 V-07 prefill per source kind; invalid labels refuse; size/fps fall back to the base form', () => {
    expect(formFromCapture(capture({ width: 800, height: 600, fps: 25 }), DEFAULT_FORM)).toEqual({
      form: { ...DEFAULT_FORM, source: 'http-mjpeg', url: 'http://127.0.0.1:64684/stream', width: 800, height: 600, fps: 25 },
      note: REUSE_HTTP_NOTE,
    });
    expect(formFromCapture(capture({ source: 'replay', sourceLabel: 'replay:clip.mp4' }), DEFAULT_FORM)).toEqual({
      form: { ...DEFAULT_FORM, source: 'replay', name: 'clip.mp4', width: 640, height: 360, fps: 15 },
      note: null,
    });
    const avf = formFromCapture(capture({ source: 'avfoundation', sourceLabel: 'avfoundation:USB Capture HDMI', width: null, fps: 0 }), DEFAULT_FORM);
    expect(avf?.form).toEqual({ ...DEFAULT_FORM, source: 'avfoundation', device: 'USB Capture HDMI', width: 1280, height: 360, fps: 30 });
    for (const bad of [
      capture({ source: 'rtsp', sourceLabel: 'rtsp://x' }),
      capture({ source: 'replay', sourceLabel: 'clip.mp4' }),
      capture({ source: 'http-mjpeg', sourceLabel: '' }),
      capture({ source: '', sourceLabel: 'replay:x' }),
    ]) {
      expect(formFromCapture(bad, DEFAULT_FORM)).toBeNull();
    }
  });

  it('M8/M9 V-08 source lists: envelope or bare, untrusted names filtered, bounded, local only when enumerated', () => {
    const bare = {
      kinds: ['http-mjpeg', 'replay', 'avfoundation'],
      replay: ['a.mp4', '../x', '.hidden', 'a.mp4', 'b.mkv', 7],
      local: { enumerated: true, avfoundation_video: [{ index: 0, name: 'USB Capture HDMI' }, { index: 1, name: 'bad\u0007name' }, { index: 2, name: '' }, { name: 'USB Capture HDMI' }], warning: 'may trigger TCC' },
    };
    const want = { replay: ['a.mp4', 'b.mkv'], local: { enumerated: true, devices: ['USB Capture HDMI'], warning: 'may trigger TCC' } };
    expect(readSources(bare)).toEqual(want);
    expect(readSources({ data: bare })).toEqual(want);
    expect(readSources({ replay: ['a.mp4'], local: { enumerated: false, avfoundation_video: [{ name: 'Cam' }] } }).local).toEqual({ enumerated: false, devices: [], warning: null });
    expect(readSources(null)).toEqual({ replay: [], local: { enumerated: false, devices: [], warning: null } });
    const many = readSources({ replay: Array.from({ length: 300 }, (_, i) => `c${i}.mp4`) });
    expect(many.replay).toHaveLength(MAX_SOURCE_ITEMS);
    expect(sourceListText(readSources(bare), null)).toBe('replay 2개 · 장치 1개 (실물·TCC 미검증)');
    expect(sourceListText(readSources({ replay: [] }), null)).toBe('replay 없음 ($LOOM_HOME/video/replay)');
    expect(sourceListText(null, null)).toBe('');
    expect(sourceListText(readSources(bare), 'ffmpeg helper timed out')).toBe('video.sources 실패: ffmpeg helper timed out');
  });

  it('M10 download names only for strict artifact ids', () => {
    expect(artifactFileName(`snap-${CAP}-12`)).toBe(`snap-${CAP}-12.jpg`);
    expect(artifactFileName(`rec-${CAP}-3`)).toBe(`rec-${CAP}-3.mkv`);
    for (const bad of ['snap-../x-1', `SNAP-${CAP}-1`, `snap-${CAP}-`, `x-${CAP}-1`, `snap-${CAP}-1-2`, '']) expect(artifactFileName(bad)).toBeNull();
  });
});

// ------------------------------------------------------------------------------------------------
// Panel (jsdom). fetch (ctx.media transport), createImageBitmap, canvas 2D and WebCrypto are injected.

let h: Harness | null = null;
let routes: (path: string) => Response = () => new Response('{"error":"no_media"}', { status: 404 });
const fetched: string[] = [];
const JPEG = new Uint8Array([0xff, 0xd8, 1, 2, 3, 0xff, 0xd9]);
const JPEG_SHA = createHash('sha256').update(JPEG).digest('hex');

function setup() {
  fetched.length = 0;
  vi.stubGlobal('crypto', webcrypto);
  vi.stubGlobal(
    'fetch',
    vi.fn(async (u: string) => {
      const p = new URL(String(u)).pathname;
      fetched.push(p);
      return routes(p);
    }),
  );
  vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ width: 2, height: 2, close: vi.fn() })));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage: vi.fn() } as never);
}
afterEach(() => {
  h?.dispose();
  h = null;
  routes = () => new Response('{"error":"no_media"}', { status: 404 });
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const field = <E extends HTMLElement>(name: string) => q('video-config').querySelector<E>(`[name="${name}"]`)!;
const wallTs = (ms: number) => ({ mono_ns: '1', wall_ns: nsOf(ms), clock_id: 't', source: 'gateway' as const });
const cfg = (o: Record<string, unknown> = {}) => ({ source: 'http-mjpeg', source_label: 'http://127.0.0.1:64684/stream', width: 640, height: 360, fps: 15, quality: 5, audio: false, ...o });
const stateEv = (state: string, atMs: number, extra: Record<string, unknown> = {}, config: Record<string, unknown> = {}) =>
  ev({ kind: 'video.capture_state', channel: CAP, ts: wallTs(atMs), payload: { capture_id: CAP, capture_epoch: '1794645164583021002', state, reason: null, config: cfg(config), ...extra } });
const healthEv = () =>
  ev({
    kind: 'video.health',
    channel: CAP,
    payload: { capture_id: CAP, state: 'live', frames_total: 1011, measured_fps: 3, requested_fps: 15, last_frame_id: 1011, age_ms: 25, stale: false, ffmpeg: { dup_frames: 0, drop_frames: 0 }, media: { skipped: 0 }, ring: { frames: 90 } },
  });
const snapEv = (sha: string, n = 1) =>
  ev({ kind: 'video.snapshot', channel: CAP, payload: { capture_id: CAP, artifact_id: `snap-${CAP}-${n}`, frame_id: 7, sha256: sha, size: JPEG.length, capture_epoch: '1794645164583021002', recv_utc_ns: '1762770000000000000' } });

async function mount() {
  await import('./videoPanel');
  h = mountPanel('video.workbench', {});
  return h;
}

describe('B1-P V-05 ended health in the panel', () => {
  it('after EOF the health line is an ended summary, not "fresh" from the last live health', async () => {
    setup();
    await mount();
    const t = Date.now();
    h!.batch([stateEv('live', t - 20 * 60_000), healthEv()]);
    expect(q('video-health').textContent).toContain('fresh');
    expect(q('video-health').textContent).toContain('수신 3/15 fps');
    h!.batch([stateEv('closed', t - 16 * 60_000 - 500, { reason: 'eof', stopped_at_frame_id: 1011 })]);
    expect(q('video-health').textContent).toMatch(/^종료 · frames 1011 · 마지막 frame 1011 · 마지막 수신 — · 종료 시각 \d\d:\d\d:\d\d\.\d{3} \(16분 전\)$/);
    expect(q('video-health').textContent).not.toMatch(/fresh|STALE|수신 \d+\/\d+ fps/);
  });
});

describe('B1-P V-07 reuse settings', () => {
  it('hidden while live; after EOF prefills the form (incl. non-preset size/fps) without any RPC; open stays explicit', async () => {
    setup();
    await mount();
    const t = Date.now();
    h!.batch([stateEv('live', t, {}, { width: 800, height: 600, fps: 25 })]);
    expect(q('video-reuse').hidden).toBe(true);
    h!.batch([stateEv('closed', t + 1, { reason: 'eof' }, { width: 800, height: 600, fps: 25 })]);
    expect(q('video-reuse').hidden).toBe(false);
    q('video-reuse').click();
    h!.frame();
    expect(field<HTMLSelectElement>('source').value).toBe('http-mjpeg');
    expect(field<HTMLInputElement>('url').value).toBe('http://127.0.0.1:64684/stream');
    expect(field<HTMLSelectElement>('size').value).toBe('800x600');
    expect(field<HTMLSelectElement>('fps').value).toBe('25');
    expect(q<HTMLDetailsElement>('video-config').open).toBe(true);
    expect(q('video-reuse-note').hidden).toBe(false);
    expect(q('video-reuse-note').textContent).toBe(REUSE_HTTP_NOTE);
    expect(h!.net.calls).toEqual([]);
    expect(q('video-cli').textContent).toBe('loom video open --source http-mjpeg --url http://127.0.0.1:64684/stream --size 800x600 --fps 25');
  });

  it('replay capture: name restored, no URL note', async () => {
    setup();
    await mount();
    h!.batch([stateEv('closed', Date.now(), { reason: 'eof' }, { source: 'replay', source_label: 'replay:clip.mp4' })]);
    q('video-reuse').click();
    h!.frame();
    expect(field<HTMLSelectElement>('source').value).toBe('replay');
    expect(field<HTMLInputElement>('name').value).toBe('clip.mp4');
    expect(q('video-reuse-note').hidden).toBe(true);
    expect(h!.net.calls).toEqual([]);
  });
});

describe('B1-P V-08 source lists', () => {
  const RESULT = (local: boolean) => ({
    data: {
      kinds: ['http-mjpeg', 'replay', 'avfoundation'],
      replay: ['a.mp4', '../x', 'b.mkv'],
      local: local ? { enumerated: true, avfoundation_video: [{ index: 0, name: 'USB Capture HDMI' }], warning: 'TCC' } : { enumerated: false, note: 'only with local=true' },
    },
  });
  const choose = (v: string) => {
    const s = field<HTMLSelectElement>('source');
    s.value = v;
    s.dispatchEvent(new Event('input'));
    s.dispatchEvent(new Event('change'));
    h!.frame();
  };
  const opts = (id: string) => [...q(id).querySelectorAll('option')].map((o) => o.value);

  it('replay list is read once when replay is chosen (never on mount); devices only on the explicit button', async () => {
    setup();
    await mount();
    h!.net.responders.set('video.sources', (p) => RESULT((p as { local: boolean }).local));
    h!.frame();
    expect(h!.net.calls).toEqual([]);
    expect(q('video-device-load').hidden).toBe(true);
    choose('replay');
    await vi.waitFor(() => expect(opts('video-replay-list')).toEqual(['a.mp4', 'b.mkv']));
    expect(h!.net.calls).toEqual([{ method: 'video.sources', params: { local: false } }]);
    expect(field<HTMLInputElement>('name').getAttribute('list')).toBe(q('video-replay-list').id);
    h!.frame();
    expect(q('video-source-list').textContent).toBe('replay 2개');
    choose('http-mjpeg');
    choose('replay');
    expect(h!.net.calls).toHaveLength(1);

    choose('avfoundation');
    expect(q('video-device-load').hidden).toBe(false);
    expect(h!.net.calls).toHaveLength(1);
    q('video-device-load').click();
    await vi.waitFor(() => expect(opts('video-device-list')).toEqual(['USB Capture HDMI']));
    expect(h!.net.calls[1]).toEqual({ method: 'video.sources', params: { local: true } });
    expect(field<HTMLInputElement>('device').getAttribute('list')).toBe(q('video-device-list').id);
    h!.frame();
    expect(q('video-source-list').textContent).toBe('replay 2개 · 장치 1개 (실물·TCC 미검증)');
  });

  it('a failed listing is shown, not swallowed', async () => {
    setup();
    await mount();
    h!.net.responders.set('video.sources', () => ({ schema_version: 1, ok: false, cmd: 'video.sources', meta: {}, others_since_last: {}, error: { code: 'timeout', message: 'ffmpeg helper timed out' } }));
    choose('avfoundation');
    q('video-device-load').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-source-list').textContent).toContain('video.sources 실패');
    });
    expect(q('video-source-list').textContent).toContain('ffmpeg helper timed out');
  });
});

describe('B1-P V-06 artifacts never navigate the app', () => {
  it('snapshot rows: view button + save anchor with download; recordings download; no bare anchors', async () => {
    setup();
    await mount();
    h!.batch([
      stateEv('closed', Date.now(), { reason: 'eof' }),
      snapEv(JPEG_SHA),
      ev({ kind: 'video.record_state', channel: CAP, payload: { capture_id: CAP, recording_id: `rec-${CAP}-2`, artifact_id: `rec-${CAP}-2`, state: 'finalized', frames_written: 9, gap_frames: 0, partial: false, partial_reasons: [], size: 1000 } }),
    ]);
    const row = q('video-snap-row');
    expect(row.dataset.artifactId).toBe(`snap-${CAP}-1`);
    const save = q<HTMLAnchorElement>('video-snap-save');
    expect(save.getAttribute('href')).toBe(`/api/v1/video/artifact/snap-${CAP}-1`);
    expect(save.getAttribute('download')).toBe(`snap-${CAP}-1.jpg`);
    expect(q<HTMLAnchorElement>('video-record-download').getAttribute('download')).toBe(`rec-${CAP}-2.mkv`);
    const anchors = [...h!.host.querySelectorAll('a')];
    expect(anchors.length).toBeGreaterThanOrEqual(2);
    for (const a of anchors) expect(a.getAttribute('download')).toBeTruthy();
    expect(q('video-snap-view').tagName).toBe('BUTTON');
  });

  it('view = in-panel, sha256 verified in the browser; ▶ 실시간 leaves the viewer', async () => {
    setup();
    routes = (p) => (p.startsWith('/api/v1/video/artifact/') ? new Response(JPEG, { status: 200, headers: { 'content-type': 'image/jpeg' } }) : new Response('{"error":"no_media"}', { status: 404 }));
    await mount();
    h!.batch([stateEv('closed', Date.now(), { reason: 'eof' }), snapEv(JPEG_SHA)]);
    const stage = q('video-canvas').parentElement!;
    q('video-snap-view').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(stage.dataset.view).toBe('artifact');
    });
    expect(fetched).toContain(`/api/v1/video/artifact/snap-${CAP}-1`);
    expect(q('video-overlay').textContent).toBe(`아티팩트 snap-${CAP}-1 · frame 7 · sha256 일치`);
    expect(q('video-frame-sha256').textContent).toBe(`sha256 ${JPEG_SHA}`);
    expect(q('video-frame-sha256').dataset.mismatch).toBe('false');
    expect(q('video-freeze').textContent).toBe('▶ 실시간');
    expect(q<HTMLButtonElement>('video-snapshot-btn').disabled).toBe(true);
    q('video-freeze').click();
    h!.frame();
    expect(stage.dataset.view).toBe('live');
    expect(q('video-overlay').textContent ?? '').not.toContain('아티팩트');
  });

  it('a hash mismatch is flagged; a non-JPEG reply is refused with a toast', async () => {
    setup();
    let ct = 'image/jpeg';
    routes = (p) => (p.startsWith('/api/v1/video/artifact/') ? new Response(JPEG, { status: 200, headers: { 'content-type': ct } }) : new Response('{"error":"no_media"}', { status: 404 }));
    await mount();
    h!.batch([stateEv('closed', Date.now(), { reason: 'eof' }), snapEv('ab'.repeat(32))]);
    const stage = q('video-canvas').parentElement!;
    q('video-snap-view').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(stage.dataset.view).toBe('artifact');
    });
    expect(q('video-overlay').textContent).toContain('sha256 불일치!');
    expect(q('video-frame-sha256').dataset.mismatch).toBe('true');
    q('video-freeze').click();
    h!.frame();
    ct = 'application/json';
    q('video-snap-view').click();
    await vi.waitFor(() => expect(h!.toasts.some((t) => t.includes('아티팩트 보기 실패: 형식 application/json'))).toBe(true));
    h!.frame();
    expect(stage.dataset.view).toBe('live');
  });
});
