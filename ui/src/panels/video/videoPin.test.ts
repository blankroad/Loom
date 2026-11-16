// @vitest-environment jsdom
// Operator upgrade B3 (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 3) — Opus-authored spec tests.
// V-03 UI: freeze = daemon pin (video.pin/unpin), visible retention countdown, snapshot disabled with a reason
// when the frozen frame is not retained · V-04: ring ◀▶ while frozen, recv_utc shown. Sonnet must not edit this file.
import { createHash, webcrypto } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getPanel } from '../../sdk/registry';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import {
  fmtRemain,
  PIN_RETRY_HINT,
  PIN_TTL_MS,
  pinText,
  pinTick,
  readPinError,
  readPinResult,
  ringStep,
  snapshotBlock,
  type PinView,
} from './videoModel';

const CAP = 'v0123456789ab';
const SHA = 'ab'.repeat(32);
const pad = (n: number, w = 2) => String(n).padStart(w, '0');
const localHms = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
};
const pv = (o: Partial<PinView> = {}): PinView => ({
  captureId: CAP,
  frameId: 42,
  status: 'pinned',
  expiresAtMs: 1_120_000,
  ttlMs: 120_000,
  reason: null,
  sha256: SHA,
  ...o,
});

// ------------------------------------------------------------------------------------------------
describe('B3-M pure model', () => {
  it('M1 pin result: envelope or bare; expiry from the reply time; daemon frame id wins; malformed → bad_reply', () => {
    const data = { capture_id: CAP, frame_id: 42, sha256: SHA, size: 7, owner: 'user', ttl_ms: 120_000, expires_in_ms: 119_999, refreshed: false, retained_by_before: 'ring', pins: {} };
    expect(readPinResult({ data }, CAP, 42, 1_000)).toEqual({ captureId: CAP, frameId: 42, status: 'pinned', expiresAtMs: 120_999, ttlMs: 120_000, reason: null, sha256: SHA });
    expect(readPinResult(data, CAP, 42, 0).expiresAtMs).toBe(119_999);
    expect(readPinResult({ ...data, frame_id: 43 }, CAP, 42, 0).frameId).toBe(43);
    expect(readPinResult({ ...data, frame_id: '43' }, CAP, 42, 0).frameId).toBe(42);
    expect(readPinResult({ expires_in_ms: 5 }, CAP, 42, 10)).toEqual({ captureId: CAP, frameId: 42, status: 'pinned', expiresAtMs: 15, ttlMs: null, reason: null, sha256: null });
    expect(readPinResult({ expires_in_ms: 0 }, CAP, 42, 10).status).toBe('pinned');
    for (const bad of [null, 'x', {}, { data: { expires_in_ms: '5' } }, { expires_in_ms: -1 }, { expires_in_ms: Number.NaN }, { expires_in_ms: Number.POSITIVE_INFINITY }]) {
      expect(readPinResult(bad, CAP, 42, 0)).toEqual({ captureId: CAP, frameId: 42, status: 'error', expiresAtMs: null, ttlMs: null, reason: 'bad_reply', sha256: null });
    }
  });

  it('M2 rejection: E_LIMIT → refused with the daemon reason; E_NOT_FOUND; other codes normalized; no code → internal', () => {
    const err = (code: unknown, detail: unknown) => Object.assign(new Error('x'), { code, detail });
    const none = { captureId: CAP, frameId: 42, expiresAtMs: null, ttlMs: null, sha256: null };
    expect(readPinError(err('E_LIMIT', { reason: 'pin_user_limit', frame_id: 42, pins: {} }), CAP, 42)).toEqual({ ...none, status: 'refused', reason: 'pin_user_limit' });
    expect(readPinError(err('limit', { reason: 'pin_bytes', need: 9, free: 1 }), CAP, 42)).toEqual({ ...none, status: 'refused', reason: 'pin_bytes' });
    expect(readPinError(err('E_LIMIT', null), CAP, 42)).toEqual({ ...none, status: 'refused', reason: 'limit' });
    expect(readPinError(err('E_NOT_FOUND', { expired: true }), CAP, 42)).toEqual({ ...none, status: 'error', reason: 'not_found' });
    expect(readPinError(err('E_RIGHTS', null), CAP, 42)).toEqual({ ...none, status: 'error', reason: 'rights' });
    expect(readPinError(err('stop_latched', null), CAP, 42).reason).toBe('stop_latched');
    expect(readPinError(new Error('boom'), CAP, 42)).toEqual({ ...none, status: 'error', reason: 'internal' });
    expect(readPinError('str', CAP, 42).reason).toBe('internal');
    expect(readPinError(err('', null), CAP, 42).reason).toBe('internal');
  });

  it('M3 expiry: inclusive, pinned only, never mutates, unchanged views keep identity', () => {
    const p = pv();
    expect(pinTick(p, 1_119_999)).toBe(p);
    expect(pinTick(p, 1_120_000)).toEqual({ ...p, status: 'expired' });
    expect(p.status).toBe('pinned');
    for (const other of [pv({ status: 'refused', expiresAtMs: null }), pv({ status: 'pending', expiresAtMs: null }), pv({ expiresAtMs: null }), pv({ status: 'expired' })]) {
      expect(pinTick(other, 9e15)).toBe(other);
    }
  });

  it('M4 remaining time table', () => {
    const cases: [number, string][] = [
      [120_000, '2:00'],
      [119_001, '2:00'],
      [119_000, '1:59'],
      [61_000, '1:01'],
      [600_000, '10:00'],
      [1, '0:01'],
      [0, '0:00'],
      [-5, '0:00'],
      [Number.NaN, '0:00'],
    ];
    for (const [ms, s] of cases) expect(fmtRemain(ms)).toBe(s);
  });

  it('M5 status line per pin state (labels for known codes, raw text for unknown)', () => {
    const now = 1_000_000;
    expect(pinText(null, now)).toBe('');
    expect(pinText(pv({ status: 'pending', expiresAtMs: null }), now)).toBe('📌 보존 요청 중 · frame 42');
    expect(pinText(pv(), now)).toBe('📌 데몬 보존 · frame 42 · 남은 시간 2:00');
    expect(pinText(pv(), now + 61_000)).toBe('📌 데몬 보존 · frame 42 · 남은 시간 0:59');
    expect(pinText(pv({ expiresAtMs: null }), now)).toBe('📌 데몬 보존 · frame 42 · 남은 시간 ?');
    expect(pinText(pv({ status: 'refused', reason: 'pin_user_limit' }), now)).toBe('⚠ 보존 거부 · frame 42 · 고정 한도 4개');
    expect(pinText(pv({ status: 'refused', reason: 'pin_bytes' }), now)).toBe('⚠ 보존 거부 · frame 42 · 보존 메모리 한도 16 MiB');
    expect(pinText(pv({ status: 'refused', reason: '<b>x</b>' }), now)).toBe('⚠ 보존 거부 · frame 42 · <b>x</b>');
    expect(pinText(pv({ status: 'refused', reason: null }), now)).toBe('⚠ 보존 거부 · frame 42 · ?');
    expect(pinText(pv({ status: 'expired' }), now)).toBe('⌛ 보존 만료 · frame 42');
    expect(pinText(pv({ status: 'error', reason: 'not_found' }), now)).toBe('⚠ 보존 실패 · frame 42 · 프레임이 이미 링에 없음');
    expect(pinText(pv({ status: 'error', reason: 'rights' }), now)).toBe('⚠ 보존 실패 · frame 42 · rights');
  });

  it('M6 snapshot block reasons: only a pinned frozen frame of the current capture may be snapshotted', () => {
    const f = { captureId: CAP, id: 42 };
    expect(snapshotBlock(null, null, CAP)).toBeNull();
    expect(snapshotBlock(null, pv({ status: 'expired' }), null)).toBeNull();
    expect(snapshotBlock(f, pv(), CAP)).toBeNull();
    expect(snapshotBlock(f, pv(), null)).toBe('다른 캡처의 고정 프레임');
    expect(snapshotBlock(f, pv(), 'v999999999999')).toBe('다른 캡처의 고정 프레임');
    expect(snapshotBlock(f, null, CAP)).toBe(`고정 프레임 보존 안 됨 — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ frameId: 41 }), CAP)).toBe(`고정 프레임 보존 안 됨 — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ captureId: 'v999999999999' }), CAP)).toBe(`고정 프레임 보존 안 됨 — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ status: 'pending', expiresAtMs: null }), CAP)).toBe('보존 확인 중');
    expect(snapshotBlock(f, pv({ status: 'expired' }), CAP)).toBe(`고정 프레임 보존 만료 — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ status: 'refused', reason: 'pin_user_limit' }), CAP)).toBe(`고정 프레임 보존 안 됨 (고정 한도 4개) — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ status: 'error', reason: 'not_found' }), CAP)).toBe(`고정 프레임 보존 안 됨 (프레임이 이미 링에 없음) — ${PIN_RETRY_HINT}`);
    expect(snapshotBlock(f, pv({ status: 'error', reason: null }), CAP)).toBe(`고정 프레임 보존 안 됨 (?) — ${PIN_RETRY_HINT}`);
  });

  it('M7 ring step bounds', () => {
    const cases: [number, -1 | 1, number | null, number | null][] = [
      [42, -1, 50, 41],
      [1, -1, 50, null],
      [42, 1, 50, 43],
      [50, 1, 50, null],
      [60, 1, 50, null],
      [42, 1, null, null],
      [42, -1, null, 41],
    ];
    for (const [id, dir, last, want] of cases) expect(ringStep(id, dir, last)).toBe(want);
  });
});

// ------------------------------------------------------------------------------------------------
// Panel (jsdom). Frame route serves ids from `have` (latest = `latest`); Date.now is controlled.

let h: Harness | null = null;
const JPEG = new Uint8Array([0xff, 0xd8, 1, 2, 3, 0xff, 0xd9]);
const JPEG_SHA = createHash('sha256').update(JPEG).digest('hex');
const RECV_UTC_MS = 1_762_770_000_123;
let latest = 42;
let have = new Set<number>();
let now = 0;
const fetchedUrls: string[] = [];

function setup() {
  latest = 42;
  have = new Set([39, 40, 41, 42]);
  now = 1_000_000;
  fetchedUrls.length = 0;
  vi.stubGlobal('crypto', webcrypto);
  vi.stubGlobal(
    'fetch',
    vi.fn(async (u: string) => {
      const url = new URL(String(u));
      fetchedUrls.push(url.pathname + url.search);
      if (url.pathname !== `/api/v1/video/${CAP}/frame`) return new Response('{"error":"no_media"}', { status: 404 });
      const id = url.searchParams.has('id') ? Number(url.searchParams.get('id')) : latest;
      if (!have.has(id)) return new Response('{"error":"expired"}', { status: 404, headers: { 'content-type': 'application/json' } });
      return new Response(JPEG, {
        status: 200,
        headers: {
          'content-type': 'image/jpeg',
          'x-loom-frame-id': String(id),
          'x-loom-capture-epoch': '1794645164583021002',
          'x-loom-frame-sha256': JPEG_SHA,
          'x-loom-recv-utc-ns': String(BigInt(RECV_UTC_MS) * 1_000_000n + BigInt(id)),
        },
      });
    }),
  );
  vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ width: 2, height: 2, close: vi.fn() })));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage: vi.fn() } as never);
  vi.spyOn(Date, 'now').mockImplementation(() => now);
}
afterEach(() => {
  h?.dispose();
  h = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const cfg = { source: 'http-mjpeg', source_label: 'http://127.0.0.1:64684/stream', width: 640, height: 360, fps: 15, quality: 5, audio: false };
const liveEv = () => ev({ kind: 'video.capture_state', channel: CAP, payload: { capture_id: CAP, capture_epoch: '1794645164583021002', state: 'live', reason: null, config: cfg } });
const healthEv = (last: number) =>
  ev({
    kind: 'video.health',
    channel: CAP,
    payload: { capture_id: CAP, state: 'live', frames_total: last, measured_fps: 15, requested_fps: 15, last_frame_id: last, age_ms: 25, stale: false, ffmpeg: { dup_frames: 0, drop_frames: 0 }, media: { skipped: 0 }, ring: { frames: 4 } },
  });
const pinOk = (p: unknown) => {
  const o = p as { frame_id: number; ttl_ms: number };
  return { data: { capture_id: CAP, frame_id: o.frame_id, sha256: JPEG_SHA, size: JPEG.length, owner: 'user', ttl_ms: o.ttl_ms, expires_in_ms: o.ttl_ms, refreshed: false, retained_by_before: 'ring', pins: { count: 1 } } };
};
const failEnv = (cmd: string, code: string, detail: unknown) => ({ schema_version: 1, ok: false, cmd, meta: {}, others_since_last: {}, error: { code, message: `${cmd} refused`, detail } });
const callsOf = (method: string) => h!.net.calls.filter((c) => c.method === method).map((c) => c.params);

async function liveWithFrame() {
  await import('./videoPanel');
  h = mountPanel('video.workbench', {});
  h.net.responders.set('video.pin', pinOk);
  h.net.responders.set('video.unpin', (p) => ({ data: { ...(p as object), released: 1 } }));
  h.net.responders.set('video.snapshot', () => ({ data: { artifact_id: `snap-${CAP}-1` } }));
  h.batch([liveEv(), healthEv(42)]);
  await vi.waitFor(() => {
    h!.frame();
    expect(q('video-frame-id').textContent).toBe('frame_id 42');
  });
  return h;
}
const pinShows = async (status: string, text?: string) =>
  vi.waitFor(() => {
    h!.frame();
    expect(q('video-pin').dataset.status).toBe(status);
    if (text !== undefined) expect(q('video-pin').textContent).toBe(text);
  });

describe('B3-P V-03 freeze = daemon pin', () => {
  it('manifest: pin is a watch request; unpin is a watch safety (reduce) action', async () => {
    await import('./videoPanel');
    const a = Object.fromEntries(getPanel('video.workbench')!.manifest.actions.map((x) => [x.id, x]));
    expect(a.pin).toMatchObject({ rpc: 'video.pin', class: 'request', right: 'watch', confirm: 'none' });
    expect(a.pin!.safetyDirection).toBeUndefined();
    expect(a.unpin).toMatchObject({ rpc: 'video.unpin', class: 'request', right: 'watch', confirm: 'none', safetyDirection: 'reduce' });
  });

  it('freeze pins the drawn frame, counts down, snapshot targets it, ▶ 실시간 releases it', async () => {
    setup();
    await liveWithFrame();
    expect(q('video-pin').hidden).toBe(true);
    q('video-freeze').click();
    await vi.waitFor(() => expect(callsOf('video.pin')).toEqual([{ capture_id: CAP, frame_id: 42, ttl_ms: PIN_TTL_MS }]));
    await pinShows('pinned', '📌 데몬 보존 · frame 42 · 남은 시간 2:00');
    expect(q('video-pin').hidden).toBe(false);
    expect(q('video-canvas').parentElement!.dataset.frozen).toBe('true');
    expect(q('video-frozen').textContent).toBe(`⏸ 고정됨 (capture ${CAP}) · 수신 ${localHms(RECV_UTC_MS)}`);
    // countdown advances without any user action (existing pull timer invalidates while pinned)
    now += 61_000;
    await pinShows('pinned', '📌 데몬 보존 · frame 42 · 남은 시간 0:59');
    const snap = q<HTMLButtonElement>('video-snapshot-btn');
    expect(snap.disabled).toBe(false);
    expect(snap.title).toBe('');
    snap.click();
    await vi.waitFor(() => expect(callsOf('video.snapshot')).toEqual([{ capture_id: CAP, frame_id: 42 }]));
    q('video-freeze').click();
    await vi.waitFor(() => expect(callsOf('video.unpin')).toEqual([{ capture_id: CAP, frame_id: 42 }]));
    h!.frame();
    expect(q('video-pin').textContent).toBe('');
    expect(q('video-pin').dataset.status).toBe('none');
    expect(q('video-canvas').parentElement!.dataset.frozen).toBe('false');
    expect(callsOf('video.pin')).toHaveLength(1);
  });

  it('expiry: countdown runs out → expired, snapshot disabled with the reason, no unpin for a lapsed pin', async () => {
    setup();
    await liveWithFrame();
    q('video-freeze').click();
    await pinShows('pinned');
    now += PIN_TTL_MS;
    await pinShows('expired', '⌛ 보존 만료 · frame 42');
    const snap = q<HTMLButtonElement>('video-snapshot-btn');
    expect(snap.disabled).toBe(true);
    expect(snap.title).toBe(`고정 프레임 보존 만료 — ${PIN_RETRY_HINT}`);
    q('video-freeze').click();
    h!.frame();
    expect(q('video-pin').dataset.status).toBe('none');
    expect(callsOf('video.unpin')).toEqual([]);
  });

  it('a refused pin keeps the visual freeze but blocks snapshot with the daemon reason', async () => {
    setup();
    await liveWithFrame();
    h!.net.responders.set('video.pin', () => failEnv('video.pin', 'E_LIMIT', { reason: 'pin_user_limit', frame_id: 42, pins: { user_count: 4 } }));
    q('video-freeze').click();
    await pinShows('refused', '⚠ 보존 거부 · frame 42 · 고정 한도 4개');
    expect(q('video-canvas').parentElement!.dataset.frozen).toBe('true');
    const snap = q<HTMLButtonElement>('video-snapshot-btn');
    expect(snap.disabled).toBe(true);
    expect(snap.title).toBe(`고정 프레임 보존 안 됨 (고정 한도 4개) — ${PIN_RETRY_HINT}`);
  });

  it('a pin reply that arrives after the operator went live is released, not adopted', async () => {
    setup();
    await liveWithFrame();
    q('video-freeze').click();
    q('video-freeze').click(); // back to live before the reply resolves
    await vi.waitFor(() => expect(callsOf('video.unpin')).toEqual([{ capture_id: CAP, frame_id: 42 }]));
    h!.frame();
    expect(q('video-pin').dataset.status).toBe('none');
    expect(q('video-canvas').parentElement!.dataset.frozen).toBe('false');
  });
});

describe('B3-P V-04 ring ◀▶ while frozen', () => {
  it('hidden while live; ◀ fetches the exact id, verifies, moves the pin; ▶ bounded by the last frame; a miss keeps the frame', async () => {
    setup();
    await liveWithFrame();
    expect(q('video-ring-prev').hidden).toBe(true);
    expect(q('video-ring-next').hidden).toBe(true);
    q('video-freeze').click();
    await pinShows('pinned');
    expect(q('video-ring-prev').hidden).toBe(false);
    expect(q<HTMLButtonElement>('video-ring-prev').disabled).toBe(false);
    expect(q<HTMLButtonElement>('video-ring-next').disabled).toBe(true); // 42 is the last frame
    latest = 43; // the live stream moves on; navigation must stay on exact ids
    have.add(43);
    q('video-ring-prev').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-frame-id').textContent).toBe('frame_id 41');
    });
    expect(fetchedUrls).toContain(`/api/v1/video/${CAP}/frame?id=41`);
    expect(q('video-frame-sha256').textContent).toBe(`sha256 ${JPEG_SHA}`);
    await vi.waitFor(() => expect(callsOf('video.unpin')).toEqual([{ capture_id: CAP, frame_id: 42 }]));
    expect(callsOf('video.pin')).toEqual([
      { capture_id: CAP, frame_id: 42, ttl_ms: PIN_TTL_MS },
      { capture_id: CAP, frame_id: 41, ttl_ms: PIN_TTL_MS },
    ]);
    const order = h!.net.calls.map((c) => `${c.method}:${(c.params as { frame_id?: number }).frame_id}`);
    expect(order.indexOf('video.pin:41')).toBeLessThan(order.indexOf('video.unpin:42'));
    await pinShows('pinned', '📌 데몬 보존 · frame 41 · 남은 시간 2:00');
    expect(q('video-frozen').textContent).toBe(`⏸ 고정됨 (capture ${CAP}) · 수신 ${localHms(RECV_UTC_MS)}`);
    expect(q<HTMLButtonElement>('video-ring-next').disabled).toBe(false);
    // miss: frame 40 already left the ring → error shown, still on 41, no pin attempt for 40
    have.delete(40);
    q('video-ring-prev').click();
    await vi.waitFor(() => {
      h!.frame();
      expect(q('video-error').textContent).toContain('frame 40 없음');
    });
    expect(q('video-frame-id').textContent).toBe('frame_id 41');
    expect(callsOf('video.pin').map((p) => (p as { frame_id: number }).frame_id)).toEqual([42, 41]);
    // back to live: navigation hidden, pin 41 released
    q('video-freeze').click();
    await vi.waitFor(() => expect(callsOf('video.unpin')).toContainEqual({ capture_id: CAP, frame_id: 41 }));
    h!.frame();
    expect(q('video-ring-prev').hidden).toBe(true);
  });
});
