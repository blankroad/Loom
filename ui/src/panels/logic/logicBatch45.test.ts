// @vitest-environment jsdom
// operator-upgrade logic — batch 4/5 UI specification tests (Opus-authored; Sonnet must not edit).
// Findings: L6 UART TX+RX in one capture, decoder error rows; L7 software trigger (form, state, marker).
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md (B45u-*).
import { afterEach, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import {
  CaptureBook,
  CaptureData,
  cliLine,
  DEFAULT_CONFIG,
  decoderSpec,
  estimatePublishLoad,
  readAnnotations,
  readDecoderTracks,
  readTriggerInfo,
  startParams,
  trackLabel,
  triggerText,
  validateConfig,
  type StartConfig,
} from './logicModel';
import { paint } from './waveform';

const CAP = 'lc-45';
const RATE = 1_000_000;
const DUPLEX: StartConfig = { ...DEFAULT_CONFIG, uartLine: 'both', uartRx: 0, uartTx: 1, uartBaud: 100_000 };

describe('B45u model — duplex UART + glitch', () => {
  it('spec, validation, params and CLI', () => {
    expect(decoderSpec(DUPLEX)).toBe('uart:baudrate=100000:rx=D0:tx=D1');
    expect(decoderSpec({ ...DUPLEX, source: 'sigrok-device', driver: 'fx2lafw', uartParity: 'even' })).toBe('uart:baudrate=100000:rx=D0:tx=D1:parity=even');
    expect(validateConfig(DUPLEX)).toEqual([]); // the synthetic generator drives both lines
    expect(validateConfig({ ...DUPLEX, uartTx: 0 }).join()).toContain('UART RX와 TX가 같은 채널입니다');
    expect(validateConfig({ ...DUPLEX, uartTx: 9 }).join()).toContain('UART TX 채널이 캡처 채널 밖입니다');
    expect(validateConfig({ ...DUPLEX, uartRx: -1 }).join()).toContain('UART RX 채널이 캡처 채널 밖입니다');
    const g = { ...DUPLEX, glitchEvery: 5 };
    expect(validateConfig(g)).toEqual([]);
    expect(startParams(g).glitch_every).toBe(5);
    expect(startParams(DUPLEX).glitch_every).toBeUndefined();
    expect(cliLine(g)).toContain('--glitch-every 5');
    expect(cliLine(DUPLEX)).not.toContain('--glitch-every');
    expect(validateConfig({ ...g, glitchEvery: 1001 }).join()).toContain('글리치 주기는 1–1000');
    expect(validateConfig({ ...g, source: 'sigrok-device', driver: 'fx2lafw' }).join()).toContain('글리치는 합성 UART에서만 쓸 수 있습니다');
    expect(validateConfig({ ...DEFAULT_CONFIG, decoder: 'spi', glitchEvery: 3 }).join()).toContain('글리치는 합성 UART에서만 쓸 수 있습니다');
    // a device capture never sends glitch_every even if a stale value is in the form
    expect(startParams({ ...DUPLEX, source: 'sigrok-device', driver: 'fx2lafw', glitchEvery: 0 }).glitch_every).toBeUndefined();
  });

  it('load estimate counts both lines', () => {
    const one = estimatePublishLoad({ ...DUPLEX, uartLine: 'rx' }, ['D0', 'D1']);
    const two = estimatePublishLoad(DUPLEX, ['D0', 'D1']);
    expect(two.annotationsPerS).toBeCloseTo(one.annotationsPerS! * 2, 6);
    expect(two.edgesPerS).toBeCloseTo(one.edgesPerS! * 2, 6);
    expect(estimatePublishLoad(DUPLEX, ['D0']).edgesPerS).toBeCloseTo(one.edgesPerS!, 6); // TX lane hidden
    const dev = estimatePublishLoad({ ...DUPLEX, source: 'sigrok-device', driver: 'fx2lafw', uartBaud: 115_200 }, ['D0', 'D1']);
    expect(dev.annotationsPerS).toBeCloseTo(23_040, 6);
  });

  it('tracks, labels and row roles come from the backend, never guessed', () => {
    const tracks = readDecoderTracks([{ id: 'uart-1', line: 'rx', channel: 'D0' }, { id: 'uart-2', line: 'tx', channel: 'D1' }, { id: 3 }])!;
    expect(tracks).toEqual([
      { id: 'uart-1', line: 'rx', channel: 'D0' },
      { id: 'uart-2', line: 'tx', channel: 'D1' },
    ]);
    expect(readDecoderTracks('x')).toBeNull();
    expect(trackLabel('uart-2', tracks)).toBe('uart-2 TX D1');
    expect(trackLabel('spi-1', tracks)).toBe('spi-1');
    expect(trackLabel('uart-1', null)).toBe('uart-1');
    const r = readAnnotations('logic.annotations', { capture_id: CAP, annotations: [
      { decoder: 'uart-1', class: null, start_sample: 1, end_sample: 2, text: 'Frame error', role: 'error' },
      { decoder: 'uart-1', class: null, start_sample: 3, end_sample: 4, text: '41', role: 'data' },
      { decoder: 'uart-1', class: null, start_sample: 5, end_sample: 6, text: 'Oops' },
      { decoder: 'uart-1', class: null, start_sample: 7, end_sample: 8, text: '42', role: 'bogus' },
    ] }, CAP, 1n, RATE);
    expect(r.rows.map((a) => a.role)).toEqual(['error', 'data', null, null]);
    const b = new CaptureBook();
    b.applyState({ capture_id: CAP, state: 'running', decoder_tracks: [{ id: 'uart-1', line: 'rx', channel: 'D0' }], counters: { error_rows: 4 } }, CAP, 1n);
    b.applyState({ capture_id: CAP, state: 'stopped' }, CAP, 2n);
    expect(b.captures.get(CAP)!.info!.decoderTracks).toEqual([{ id: 'uart-1', line: 'rx', channel: 'D0' }]);
    expect(b.captures.get(CAP)!.info!.counters!.errorRows).toBe(4);
  });
});

describe('B45u model — trigger', () => {
  const T: StartConfig = { ...DEFAULT_CONFIG, triggerOn: true, triggerCh: 3, triggerEdge: 'rising', triggerPreMs: 10, triggerPostMs: 500 };
  it('params, validation, CLI', () => {
    expect(validateConfig(T)).toEqual([]);
    expect(startParams(T).trigger).toEqual({ channel: 'D3', edge: 'rising', pre_ms: 10, post_ms: 500 });
    expect(startParams(DEFAULT_CONFIG).trigger).toBeUndefined();
    expect(cliLine(T)).toContain('--trigger D3:rising --pre 10ms --post 500ms');
    expect(cliLine(DEFAULT_CONFIG)).not.toContain('--trigger');
    expect(validateConfig({ ...T, triggerPreMs: 1001 }).join()).toContain('트리거 앞 구간 0–1000 ms');
    expect(validateConfig({ ...T, triggerPreMs: 1.5 }).join()).toContain('트리거 앞 구간 0–1000 ms');
    expect(validateConfig({ ...T, triggerPostMs: 0 }).join()).toContain('트리거 뒤 구간 1 ms – 캡처 시간');
    expect(validateConfig({ ...T, durationS: 0.4 }).join()).toContain('트리거 뒤 구간 1 ms – 캡처 시간');
    expect(validateConfig({ ...T, triggerCh: 8 }).join()).toContain('트리거 채널이 캡처 채널 밖입니다');
    expect(validateConfig({ ...T, triggerOn: false, triggerPreMs: 5000 })).toEqual([]); // off → not validated
  });

  it('trigger state from capture_state', () => {
    const t = readTriggerInfo({ state: 'fired', sample: 12_345, channel: 'D3', edge: 'rising', pre_ms: 10, post_ms: 500 })!;
    expect(t).toEqual({ state: 'fired', sample: 12_345, channel: 'D3', edge: 'rising', preMs: 10, postMs: 500 });
    expect(triggerText(t)).toBe('트리거 sample 12,345');
    expect(triggerText({ ...t, state: 'armed', sample: null })).toBe('트리거 대기 (D3 rising)');
    expect(triggerText({ ...t, state: 'not_fired', sample: null })).toBe('트리거 미발생 (D3 rising)');
    expect(triggerText(readTriggerInfo({ state: 'x' }))).toBe('트리거 상태 미확인');
    expect(triggerText(null)).toBe('');
    expect(readTriggerInfo(7)).toBeNull();
    const b = new CaptureBook();
    b.applyState({ capture_id: CAP, state: 'running', trigger: { state: 'armed', channel: 'D3', edge: 'rising' } }, CAP, 1n);
    b.applyState({ capture_id: CAP, state: 'running', trigger: { state: 'fired', sample: 20, channel: 'D3', edge: 'rising' } }, CAP, 2n);
    b.applyState({ capture_id: CAP, state: 'stopped', reason: 'trigger_post' }, CAP, 3n);
    expect(b.captures.get(CAP)!.info!.trigger).toMatchObject({ state: 'fired', sample: 20 });
  });
});

describe('B45u waveform — trigger marker and track labels', () => {
  it('draws T at the trigger sample and labelled tracks', () => {
    const calls: [string, unknown[]][] = [];
    const g = new Proxy({} as Record<string, unknown>, {
      get: (_t, k) => (k === 'canvas' ? null : (...a: unknown[]) => void calls.push([String(k), a])),
      set: () => true,
    }) as unknown as CanvasRenderingContext2D;
    const pal = { bg: '#fff', grid: '#eee', fg: '#000', muted: '#666', hi: '#070', lo: '#070', dense: '#8c8', unknown: '#c80', ann: '#36c', annText: '#126', cursor: '#d40', sel: '#d40' };
    paint(g, { cap: new CaptureData(CAP), lanes: [{ bit: 0, name: 'D0' }], decoders: ['uart-1'], view: { s0: 0, spp: 1, width: 400 }, rate: RATE, palette: pal, hoverX: null, selected: null, decoderLabels: { 'uart-1': 'uart-1 RX D0' }, triggerSample: 40 }, 80);
    expect(calls.some(([k, a]) => k === 'fillText' && a[0] === 'T')).toBe(true);
    expect(calls.some(([k, a]) => k === 'moveTo' && a[0] === 96 + 40 + 0.5)).toBe(true);
    expect(calls.some(([k, a]) => k === 'fillText' && a[0] === '⟦uart-1 RX D0⟧')).toBe(true);
  });
});

// ---------------------------------------------------------------------------------------------
describe('B45u panel', () => {
  let h: Harness | null = null;
  afterEach(() => {
    h?.dispose();
    h = null;
    document.body.replaceChildren();
    try {
      localStorage.clear();
    } catch {
      /* ignore */
    }
  });
  const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
  const field = <E extends HTMLElement = HTMLSelectElement>(name: string) => q('logic-config').querySelector<E>(`[name="${name}"]`)!;
  const setField = (name: string, value: string) => {
    const f = field<HTMLSelectElement | HTMLInputElement>(name);
    if (f instanceof HTMLInputElement && f.type === 'checkbox') f.checked = value === 'on';
    else f.value = value;
    f.dispatchEvent(new Event('change', { bubbles: true }));
    h!.frame();
  };
  const st = (seq: number, state: string, extra: Record<string, unknown> = {}) =>
    ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: String(seq), origin: 'device', payload: { capture_id: CAP, state, source: 'synthetic', samplerate: RATE, channels: ['D0', 'D1'], ...extra } });

  it('form: RX+TX shows both pins; glitch only for synthetic UART; trigger fields reach the CLI line', () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    setField('uart_line', 'both');
    expect(field('rx').closest('label')!.hidden).toBe(false);
    expect(field('uart_tx').closest('label')!.hidden).toBe(false);
    setField('uart_tx', '1');
    expect(q('logic-cli').textContent).toContain('--decode uart:baudrate=115200:rx=D0:tx=D1');
    expect(field<HTMLInputElement>('glitch_every').closest('label')!.hidden).toBe(false);
    setField('glitch_every', '5');
    expect(q('logic-cli').textContent).toContain('--glitch-every 5');
    setField('source', 'sigrok-device');
    expect(field<HTMLInputElement>('glitch_every').closest('label')!.hidden).toBe(true);
    for (const n of ['trigger_ch', 'trigger_edge', 'trigger_pre_ms', 'trigger_post_ms']) expect(field(n).closest('label')!.hidden, n).toBe(true);
    setField('trigger_on', 'on');
    for (const n of ['trigger_ch', 'trigger_edge', 'trigger_pre_ms', 'trigger_post_ms']) expect(field(n).closest('label')!.hidden, n).toBe(false);
    setField('trigger_ch', '3');
    setField('trigger_edge', 'rising');
    expect(q('logic-cli').textContent).toContain('--trigger D3:rising --pre 10ms --post 1000ms');
  });

  it('duplex tracks are labelled by line; error rows are highlighted and counted', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(st(10, 'running', { decoder_tracks: [{ id: 'uart-1', line: 'rx', channel: 'D0' }, { id: 'uart-2', line: 'tx', channel: 'D1' }], counters: { samples: 1000, error_rows: 1 } }));
    h.batch([
      ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: '11', payload: { capture_id: CAP, samplerate: RATE, dropped: 0, annotations: [
        { decoder: 'uart-1', class: null, start_sample: 10, end_sample: 90, text: '4C', role: 'data' },
        { decoder: 'uart-1', class: null, start_sample: 90, end_sample: 100, text: 'Frame error', role: 'error' },
        { decoder: 'uart-2', class: null, start_sample: 200, end_sample: 280, text: '6F', role: 'data' },
      ] } }),
    ]);
    expect(q('logic-wave').getAttribute('aria-label')).toContain('디코드 트랙 uart-1 RX D0, uart-2 TX D1');
    expect(h.host.querySelector('.lg-table .vt-row[data-start="90"]')!.classList.contains('lg-annerr')).toBe(true);
    expect(h.host.querySelector('.lg-table .vt-row[data-start="10"]')!.classList.contains('lg-annerr')).toBe(false);
    expect(h.host.querySelector('.lg-table .vt-row[data-start="200"]')!.textContent).toContain('uart-2 TX D1');
    const ctr = q('logic-counters').querySelector('[data-counter="error_rows"]')!;
    expect(ctr.textContent).toBe('디코드 오류 1');
    expect(ctr.className).toContain('lg-bad');
  });

  it('trigger: armed is explicit while nothing is published; fired shows the sample', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(st(10, 'running', { trigger: { state: 'armed', channel: 'D3', edge: 'rising', pre_ms: 10, post_ms: 500 }, counters: { samples: 50_000 } }));
    h.frame();
    expect(q('logic-trigger').textContent).toBe('트리거 대기 (D3 rising)');
    expect(q('logic-trigger').dataset.state).toBe('armed');
    expect(q('logic-empty').textContent).toBe('트리거 대기 중 — 트리거 이후의 원시 파형과 디코드만 게시·보관됩니다');
    h.net.emitControl(st(20, 'running', { trigger: { state: 'fired', sample: 52_000, channel: 'D3', edge: 'rising', pre_ms: 10, post_ms: 500 } }));
    h.batch([ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: '21', payload: { capture_id: CAP, samplerate: RATE, sample_start: 42_000, sample_end: 60_000, initial: 0, edges: [[52_000, 8]], dropped: 0 } })]);
    expect(q('logic-trigger').textContent).toBe('트리거 sample 52,000');
    expect(q('logic-trigger').dataset.state).toBe('fired');
    h.net.emitControl(st(30, 'stopped', { reason: 'trigger_post' }));
    h.frame();
    expect(q('logic-notice').textContent).toContain('트리거 후 지정 길이 도달');
    h.dispose();
    h = mountPanel('logic.analyzer');
    h.net.emitControl(st(10, 'running'));
    h.frame();
    expect(q('logic-trigger').hidden).toBe(true);
  });
});
