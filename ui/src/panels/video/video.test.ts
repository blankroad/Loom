import { describe, expect, it } from 'vitest';
import { artifactUrl, cliLine, DEFAULT_FORM, frameUrl, hex, openParams, RateMeter, validateForm, VideoBook, wide } from './videoModel';

const st = (id: string, state: string, extra: Record<string, unknown> = {}) => ({
  capture_id: id,
  state,
  config: { source: 'http-mjpeg', source_label: 'http://127.0.0.1:8090/stream', width: 640, height: 360, fps: 30 },
  ...extra,
});

describe('video model', () => {
  it('newest state wins by seq; snapshot items apply at the high-water', () => {
    const b = new VideoBook();
    expect(b.apply('video.capture_state', st('v1', 'live'), 10n)).toBe(true);
    expect(b.apply('video.capture_state', st('v1', 'opening'), 5n)).toBe(false);
    expect(b.captures.get('v1')?.state).toBe('live');
    b.apply('video.capture_state', st('v2', 'opening'), 11n);
    expect(b.newestId).toBe('v2');
    b.applySnapshotItem({ domain: 'video', state: { captures: [st('v2', 'closed', { reason: 'stop_latch', stopped_at_frame_id: 42 })], health: [], records: [], snapshots: [] } }, 20n);
    expect(b.captures.get('v2')).toMatchObject({ state: 'closed', reason: 'stop_latch', stoppedAtFrame: 42 });
    b.applySnapshotItem({ domain: 'logic', state: { captures: [st('v3', 'live')] } }, 30n);
    expect(b.captures.has('v3')).toBe(false);
  });

  it('health, record and snapshot readers keep truth fields', () => {
    const b = new VideoBook();
    b.apply('video.health', { capture_id: 'v1', frames_total: 90, measured_fps: 30, requested_fps: 30, last_frame_id: 90, age_ms: 12, stale: false, ffmpeg: { dup_frames: 1, drop_frames: 2 }, media: { skipped: 3 }, ring: { frames: 90 }, oversize: 0, truncated: 1, junk_bytes: 5 }, 1n);
    expect(b.health.get('v1')).toMatchObject({ measuredFps: 30, dup: 1, drop: 2, mediaSkipped: 3, truncated: 1, junkBytes: 5, stale: false });
    b.apply('video.record_state', { capture_id: 'v1', recording_id: 'rec-v1-1', state: 'finalized', frames_written: 88, gap_frames: 2, partial: true, partial_reasons: ['queue_gap'], size: 1000, artifact_id: 'rec-v1-1', limit_reason: null }, 2n);
    expect(b.records.get('v1')).toMatchObject({ partial: true, partialReasons: ['queue_gap'], gapFrames: 2 });
    expect(b.apply('video.snapshot', { capture_id: 'v1', artifact_id: 'snap-v1-2', frame_id: 7, sha256: 'ab', size: 9 }, 3n)).toBe(true);
    expect(b.apply('video.snapshot', { capture_id: 'v1', artifact_id: 'snap-v1-2', frame_id: 7, sha256: 'ab', size: 9 }, 4n)).toBe(false);
    expect(b.apply('video.snapshot', { capture_id: 'v1', artifact_id: 'x' }, 5n)).toBe(false);
  });

  it('open form mirrors backend selection rules', () => {
    expect(validateForm(DEFAULT_FORM)).toBeNull();
    expect(validateForm({ ...DEFAULT_FORM, url: 'http://user:pw@127.0.0.1:1/' })).toMatch(/자격증명/);
    expect(validateForm({ ...DEFAULT_FORM, url: 'http://10.0.0.2/' })).toMatch(/루프백/);
    expect(validateForm({ ...DEFAULT_FORM, url: 'https://127.0.0.1/' })).toMatch(/http/);
    for (const device of ['', 'default', 'None', '0', ' Cam', 'a:b']) {
      expect(validateForm({ ...DEFAULT_FORM, source: 'avfoundation', device })).not.toBeNull();
    }
    expect(validateForm({ ...DEFAULT_FORM, source: 'avfoundation', device: 'USB Capture HDMI' })).toBeNull();
    expect(validateForm({ ...DEFAULT_FORM, source: 'replay', name: '../x.mp4' })).not.toBeNull();
    expect(validateForm({ ...DEFAULT_FORM, fps: 61 })).not.toBeNull();
    expect(validateForm({ ...DEFAULT_FORM, width: 641 })).not.toBeNull();
    expect(openParams({ ...DEFAULT_FORM, source: 'replay', name: 'a.mp4' })).toEqual({ source: 'replay', name: 'a.mp4', width: 1280, height: 720, fps: 30 });
    expect(cliLine(DEFAULT_FORM)).toBe('loom video open --source http-mjpeg --url http://127.0.0.1:8090/stream --size 1280x720 --fps 30');
  });

  it('urls carry opaque ids only; hex and rate meter', () => {
    expect(frameUrl('v0123456789ab')).toBe('/api/v1/video/v0123456789ab/frame');
    expect(frameUrl('v0123456789ab', 5)).toBe('/api/v1/video/v0123456789ab/frame?id=5');
    expect(artifactUrl('snap-v0123456789ab-1')).toBe('/api/v1/video/artifact/snap-v0123456789ab-1');
    expect(frameUrl('../x')).not.toContain('../');
    expect(hex(new Uint8Array([0, 15, 255]).buffer)).toBe('000fff');
    const m = new RateMeter();
    for (let t = 0; t < 2000; t += 33) m.tick(t);
    expect(m.perSec(2000)).toBeGreaterThanOrEqual(29);
    expect(m.perSec(2000)).toBeLessThanOrEqual(31);
  });

  it('wide epoch/ns values above 2^53 round-trip exactly through JSON events; unsafe numbers are refused', () => {
    const text = '{"capture_id":"v1","state":"live","capture_epoch":"1794645164583021002","config":{}}';
    const b = new VideoBook();
    b.apply('video.capture_state', JSON.parse(text), 1n);
    expect(b.captures.get('v1')?.epoch).toBe('1794645164583021002');
    expect(BigInt(b.captures.get('v1')!.epoch!)).toBe(1794645164583021002n);
    // a bare JSON number has already lost precision → refused instead of silently rounded
    const rounded = JSON.parse('{"capture_id":"v2","state":"live","capture_epoch":1794645164583021002,"config":{}}');
    b.apply('video.capture_state', rounded, 2n);
    expect(b.captures.get('v2')?.epoch).toBeNull();
    b.apply('video.health', JSON.parse('{"capture_id":"v1","frames_total":1,"last_recv_mono_ns":"9007199254740993"}'), 3n);
    expect(b.health.get('v1')?.lastRecvMonoNs).toBe('9007199254740993');
    b.apply('video.snapshot', JSON.parse('{"capture_id":"v1","artifact_id":"snap-v1-1","frame_id":5,"sha256":"ab","size":3,"capture_epoch":"1794645164583021002","recv_utc_ns":"18446744073709551615"}'), 4n);
    expect(b.snapshots[0]).toMatchObject({ epoch: '1794645164583021002', recvUtcNs: '18446744073709551615' });
    expect(wide('42')).toBe('42');
    expect(wide(42)).toBe('42');
    for (const bad of ['', '012', '1e3', '-1', '123456789012345678901', 1.5, -1, 2 ** 60, null]) expect(wide(bad)).toBeNull();
  });
});
