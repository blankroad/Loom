// @vitest-environment jsdom
// video.workbench as the Video module tab (work/reports/module-apps/CONTRACT.md): the explicit source
// form is visible before anything is opened; opening the tab never opens a device or fetches media;
// engine check and open are explicit; a daemon refusal (E_UNSUPPORTED on Windows) stays visible.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import { capabilityText } from './videoPanel';
import { DEFAULT_FORM, openParams } from './videoModel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});
const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const flush = async () => {
  for (let k = 0; k < 8; k++) await Promise.resolve();
};

const CAPS = {
  ffmpeg: { path: null, version: null, error: 'ffmpeg not found on PATH' },
  sources: {
    'http-mjpeg': { implemented: true, loopback_only: true },
    replay: { implemented: true },
    avfoundation: { implemented: true, device_verified: false },
    rtsp: { implemented: false, reason: 'not validated against an RTSP endpoint' },
  },
  audio: { available: false },
};

describe('video module tab (unpinned workbench)', () => {
  it('source form open before any capture; no RPC and no media fetch on mount', async () => {
    vi.useFakeTimers();
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    await import('./videoPanel');
    h = mountPanel('video.workbench', {});
    await vi.advanceTimersByTimeAsync(3_000);
    h.frame();
    expect(q<HTMLDetailsElement>('video-config').open).toBe(true);
    expect(q('video-config').querySelector('select[name="source"]')).not.toBeNull();
    expect(q('video-state').textContent).toBe('캡처 없음');
    expect(q<HTMLButtonElement>('video-open').disabled).toBe(false);
    expect(q<HTMLButtonElement>('video-close').disabled).toBe(true);
    expect(h.net.calls).toEqual([]);
    expect(fetchSpy).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it('[엔진 확인] = one read-only video.capabilities query; the line is truthful', async () => {
    await import('./videoPanel');
    h = mountPanel('video.workbench', {});
    h.net.responders.set('video.capabilities', () => ({ data: CAPS }));
    q('video-engine-check').click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'video.capabilities', params: {} }]);
    const t = q('video-capability').textContent!;
    expect(t).toContain('FFmpeg 없음 (ffmpeg not found on PATH)');
    expect(t).toContain('avfoundation 구현·실물 미검증');
    expect(t).toContain('rtsp 미지원');
  });

  it('open is explicit with the form params; a platform refusal stays visible', async () => {
    await import('./videoPanel');
    h = mountPanel('video.workbench', {});
    h.net.responders.set('video.open', () => ({ schema_version: 1, ok: false, cmd: 'video.open', meta: {}, others_since_last: {}, error: { code: 'unsupported', message: 'video capture is not available on this platform yet' } }));
    h.frame();
    expect(h.net.calls).toEqual([]);
    q('video-open').click();
    await flush();
    h.frame();
    expect(h.net.calls).toEqual([{ method: 'video.open', params: openParams(DEFAULT_FORM) }]);
    const e = q('video-open-error');
    expect(e.hidden).toBe(false);
    expect(e.textContent).toContain('unsupported');
    expect(e.textContent).toContain('not available on this platform');
  });

  it('watch right: open disabled', async () => {
    await import('./videoPanel');
    h = mountPanel('video.workbench', {}, { hello: { rights: 'watch' } });
    h.frame();
    expect(q<HTMLButtonElement>('video-open').disabled).toBe(true);
  });
});

describe('capabilityText', () => {
  it('reports a present engine and unimplemented sources without upgrading them', () => {
    expect(capabilityText({ ffmpeg: { path: '/opt/ffmpeg', version: 'ffmpeg version 7.1' }, sources: { decklink: { implemented: false } } })).toBe(
      'FFmpeg /opt/ffmpeg (ffmpeg version 7.1) · decklink 미지원 · 오디오 미지원',
    );
    expect(capabilityText(null)).toBe('FFmpeg 없음 — 캡처 불가 · 소스 정보 없음 · 오디오 미지원');
  });
});
