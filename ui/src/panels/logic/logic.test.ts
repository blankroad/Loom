// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import {
  CaptureBook,
  CaptureData,
  DEFAULT_CONFIG,
  MAX_CHUNKS_PER_CAPTURE,
  readAnnotations,
  readEdges,
  readSnapshotItem,
  startParams,
  validateConfig,
  type StartConfig,
} from './logicModel';
import { AXIS_CHAR_PX, axisTicks, laneRuns, visibleAnnotations } from './waveform';


const CAP = 'lc-1';
const edges = (start: number, end: number, initial: number, list: unknown[], extra: Record<string, unknown> = {}) =>
  readEdges({ capture_id: CAP, samplerate: 1_000_000, sample_start: start, sample_end: end, initial, edges: list, dropped: 0, ...extra }, CAP, 1n, null);
const cap = (...chunks: (ReturnType<typeof readEdges>)[]) => {
  const c = new CaptureData(CAP);
  for (const ch of chunks) c.addChunk(ch!);
  return c;
};
const ann = (start: number, end: number, text: string, decoder = 'uart-1') => ({ decoder, class: null, start_sample: start, end_sample: end, text });

// backend StartConfig::from_params START_KEYS (crates/loom-logic/src/model.rs, CONTRACT §9)
const BACKEND_KEYS = new Set(['source', 'driver', 'conn', 'samplerate', 'channels', 'decoders', 'duration_ms', 'max_samples', 'pattern', 'text', 'frame_period_ms', 'clock_hz']);

describe('logic model — raw edges', () => {
  it('chunk gaps are unknown, never bridged with the previous level', () => {
    const c = cap(edges(0, 10, 1, []), edges(20, 30, 1, []));
    expect(c.stateAt(5)).toBe(1);
    expect(c.stateAt(15)).toBeNull();
    const runs = laneRuns(c, 0, { s0: 0, spp: 1, width: 30 });
    expect(runs.map((r) => [r.x0, r.x1, r.kind])).toEqual([[0, 10, 1], [10, 20, 'unknown'], [20, 30, 1]]);
  });

  it('edge cap (dropped>0): after the last kept edge the state is unknown up to sample_end', () => {
    const c = cap(edges(0, 100, 0, [[10, 1]], { dropped: 5 }));
    expect(c.stateAt(10)).toBe(1);
    expect(c.stateAt(11)).toBeNull();
    expect(c.droppedEdges).toBe(5);
  });

  it('R1: edge-free (idle pin) chunks are bounded by chunk count', () => {
    const c = new CaptureData(CAP);
    for (let i = 0; i < MAX_CHUNKS_PER_CAPTURE + 50; i++) c.addChunk(edges(i * 100, (i + 1) * 100, 0, [])!);
    expect(c.chunks.length).toBe(MAX_CHUNKS_PER_CAPTURE);
    expect(c.evictedTo).toBe(50 * 100);
    expect(c.stateAt(10)).toBeNull(); // evicted → unknown, not "low"
    expect(c.stateAt(50 * 100 + 1)).toBe(0);
  });

  it('R2: invalid sample indices / rate are rejected, never turned into geometry', () => {
    expect(edges(-1, 10, 0, [])).toBeNull();
    expect(edges(0, 2.5, 0, [])).toBeNull();
    expect(readEdges({ capture_id: CAP, sample_start: '99999999999999999', sample_end: 10, initial: 0, edges: [] }, CAP, 1n, null)).toBeNull();
    expect(edges(0, 10, 0, [], { samplerate: 0 })!.samplerate).toBeNull();
    expect(edges(0, 10, 0, [], { samplerate: -5 })!.samplerate).toBeNull();
    const r = readAnnotations('logic.annotations', { capture_id: CAP, samplerate: 1000, annotations: [ann(100, 10, 'rev'), ann(-1, 5, 'neg'), ann(1, 2, 'ok')] }, CAP, 1n, null);
    expect(r.rows.map((a) => a.text)).toEqual(['ok']);
    expect(r.rejected).toBe(2);
    expect(r.rows[0]!.tEndNs).toBeGreaterThanOrEqual(r.rows[0]!.tStartNs!);
  });

  it('R3: a malformed edge mid-chunk makes the rest unknown (not a fake HIGH), counted apart from the cap', () => {
    const c = cap(edges(0, 100, 0, [[10, 1], [20, 'invalid'], [90, 0]]));
    expect(c.stateAt(10)).toBe(1);
    expect(c.stateAt(11)).toBeNull();
    expect(c.stateAt(95)).toBeNull();
    expect(c.malformedEdges).toBe(2);
    expect(c.droppedEdges).toBe(0);
    const runs = laneRuns(c, 0, { s0: 0, spp: 1, width: 100 });
    expect(runs.filter((r) => r.kind === 1).every((r) => r.x1 <= 11)).toBe(true);
  });

  it('R4: a dense pixel is clipped to the chunk end and never claims the unknown region after it', () => {
    const c = cap(edges(0, 5, 0, [[1, 1], [2, 0]]), edges(20, 30, 0, []));
    const runs = laneRuns(c, 0, { s0: 0, spp: 10, width: 3 });
    const dense = runs.find((r) => r.kind === 'dense')!;
    expect(dense.x1).toBeLessThanOrEqual(0.5);
    expect(runs.find((r) => r.kind === 'unknown')).toMatchObject({ x0: 0.5, x1: 2 });
  });

  it('R6: partially overlapping chunks are refused (ordered lookup stays valid)', () => {
    const c = cap(edges(0, 100, 1, []));
    expect(c.addChunk(edges(20, 30, 0, [])!)).toBe(false);
    expect(c.rejectedChunks).toBe(1);
    expect(c.stateAt(50)).toBe(1);
    // the same span again (snapshot copy + backlog) is a silent duplicate, not a rejection
    expect(c.addChunk(edges(0, 100, 1, [])!)).toBe(false);
    expect(c.rejectedChunks).toBe(1);
  });

  it('accepts the transitional "0b…" mask only as a string fallback; numbers are canonical (§8)', () => {
    expect(edges(0, 10, 0, [[3, 0b101]])!.masks[0]).toBe(5);
    expect(readEdges({ capture_id: CAP, sample_start: 0, sample_end: 10, initial: '0b1', edges: [] }, CAP, 1n, null)!.initial).toBe(1);
  });
});

describe('logic model — annotations', () => {
  it('R5: a long annotation starting before the view is still found after zoom/pan', () => {
    const c = new CaptureData(CAP);
    const r = readAnnotations('logic.annotations', { capture_id: CAP, annotations: [ann(0, 1000, 'frame'), ann(10, 20, 'byte')] }, CAP, 1n, 1000);
    c.addAnnotations(r.rows, 0);
    const vis = visibleAnnotations(c, { s0: 50, spp: 1, width: 100 });
    expect(vis.byDecoder.get('uart-1')!.map((a) => a.text)).toEqual(['frame']);
    expect(vis.omitted).toBe(0);
  });

  it('class null and decoder id are preserved; single and batch kinds both read; _raw lines kept as text', () => {
    const one = readAnnotations('logic.annotation', { capture_id: CAP, ...ann(5, 9, '4C') }, CAP, 2n, 1_000_000);
    expect(one.rows[0]).toMatchObject({ decoder: 'uart-1', cls: null, text: '4C', tStartNs: 5000 });
    const raw = readAnnotations('logic.annotations', { capture_id: CAP, annotations: [{ decoder: 'uart-1', class: '_raw', start_sample: null, end_sample: null, text: 'garbled' }] }, CAP, 3n, null);
    expect(raw.rows).toEqual([]);
    expect(raw.rawLines).toEqual(['uart-1: garbled']);
    expect(raw.rejected).toBe(0);
  });

  it('visible cap reports omitted rows instead of dropping them silently', () => {
    const c = new CaptureData(CAP);
    c.addAnnotations(readAnnotations('logic.annotations', { capture_id: CAP, annotations: [ann(0, 1, 'a'), ann(1, 2, 'b'), ann(2, 3, 'c')] }, CAP, 1n, null).rows, 0);
    expect(visibleAnnotations(c, { s0: 0, spp: 1, width: 10 }, 2).omitted).toBe(1);
  });
});

describe('logic model — lifecycle / snapshot / start wire', () => {
  it('stale capture_state (seq ≤ applied) never overrides a newer one', () => {
    const b = new CaptureBook();
    expect(b.applyState({ capture_id: CAP, state: 'stopped' }, CAP, 20n)).toBe(true);
    expect(b.applyState({ capture_id: CAP, state: 'running' }, CAP, 15n)).toBe(false);
    expect(b.captures.get(CAP)!.info!.state).toBe('stopped');
  });

  it('snapshot captures sharing the high-water seq: the last (newest) one is followed and listed first', () => {
    const items = readSnapshotItem({
      domain: 'logic',
      kind: 'logic.snapshot',
      state: {
        captures: [
          { capture_id: 'lc-uart1', state: 'stopped', source: 'synthetic' },
          { capture_id: 'lc-uart2', state: 'stopped', source: 'synthetic' },
          { capture_id: 'lc-demo3', state: 'stopped', source: 'sigrok-demo' },
        ],
      },
    });
    const b = new CaptureBook();
    const H = 812n;
    for (const sc of items) if (sc.state) b.applyState(sc.state, sc.id, H);
    expect(b.newestId).toBe('lc-demo3');
    expect(b.list().map((c) => c.id)).toEqual(['lc-demo3', 'lc-uart2', 'lc-uart1']);
    // a later live record still wins with a unique higher seq; an older one never does
    expect(b.applyState({ capture_id: 'lc-live4', state: 'running' }, 'lc-live4', H + 1n)).toBe(true);
    expect(b.newestId).toBe('lc-live4');
    expect(b.applyState({ capture_id: 'lc-old', state: 'stopped' }, 'lc-old', H - 5n)).toBe(true);
    expect(b.newestId).toBe('lc-live4');
    expect(b.list()[0]!.id).toBe('lc-live4');
  });

  it('reads the §9 reducer snapshot {captures, annotations, edges}', () => {
    const items = readSnapshotItem({
      domain: 'logic',
      kind: 'logic.snapshot',
      state: {
        captures: [{ capture_id: 'lc-a', state: 'stopped' }, { capture_id: 'lc-b', state: 'running', source: 'synthetic' }],
        annotations: [{ capture_id: 'lc-b', ...ann(1, 2, '4C') }],
        edges: [{ capture_id: 'lc-b', sample_start: 0, sample_end: 10, initial: 0, edges: [[3, 1]], dropped: 0 }],
      },
    });
    expect(items.map((i) => i.id).sort()).toEqual(['lc-a', 'lc-b']);
    const b = items.find((i) => i.id === 'lc-b')!;
    expect(b.state!.source).toBe('synthetic');
    expect(b.annotations).toHaveLength(1);
    expect(b.edges).toHaveLength(1);
  });

  it('start params use only backend keys; demo pattern pins 200 kHz; channels are the display selection', () => {
    const cfgs: StartConfig[] = [
      { ...DEFAULT_CONFIG },
      { ...DEFAULT_CONFIG, source: 'sigrok-demo', decoder: 'none', samplerate: 1_000_000, pattern: 'graycode' },
      { ...DEFAULT_CONFIG, source: 'sigrok-demo', decoder: 'none', samplerate: 1_000_000, pattern: '' },
      { ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', conn: '1.5', decoder: 'spi' },
    ];
    for (const c of cfgs) {
      expect(validateConfig(c)).toEqual([]);
      const p = startParams(c, ['D0', 'D2']);
      for (const k of Object.keys(p)) expect(BACKEND_KEYS.has(k)).toBe(true);
      expect((p.decoders as string[]).length).toBeLessThanOrEqual(1);
      expect(p.channels).toEqual(['D0', 'D2']);
    }
    expect(startParams(cfgs[1]!).samplerate).toBe(200_000);
    expect(startParams(cfgs[1]!).driver).toBeUndefined();
    expect(startParams(cfgs[2]!).samplerate).toBe(1_000_000);
    expect(startParams(cfgs[0]!).text).toBe('LOOM {n}\r\n');
    expect(startParams(cfgs[3]!)).toMatchObject({ driver: 'fx2lafw', conn: '1.5', decoders: ['spi:clk=D0:mosi=D1:cs=D2'] });
  });

  it('refuses unsupported drivers and out-of-range rates before the RPC', () => {
    expect(validateConfig({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'saleae-logic16' }).join()).toContain('미지원');
    expect(validateConfig({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: '' }).length).toBeGreaterThan(0);
    expect(validateConfig({ ...DEFAULT_CONFIG, samplerate: 24_000_000 }).join()).toContain('10 MHz');
  });
});

describe('logic.analyzer panel', () => {
  let h: Harness | null = null;
  afterEach(() => {
    h?.dispose();
    h = null;
    document.body.replaceChildren();
  });
  const q = (id: string) => h!.host.querySelector<HTMLElement>(`[data-testid="${id}"]`)!;
  const state = (seq: number, st: string, extra: Record<string, unknown> = {}) =>
    ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: String(seq), origin: 'agent', payload: { capture_id: CAP, state: st, source: 'synthetic', samplerate: 1_000_000, channels: ['D0'], decoders: ['uart:baudrate=115200:rx=D0'], quality: ['synthetic'], ...extra } });

  it('lifecycle via control path: running → stopped, stale ignored, STOP sends capture_id, records retained', async () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.frame();
    expect(q('logic-state').textContent).toContain('캡처 중');
    expect(q('logic-source').textContent).toContain('SYNTHETIC');
    h.batch([
      ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: '11', payload: { capture_id: CAP, samplerate: 1_000_000, sample_start: 0, sample_end: 100, initial: 1, edges: [[10, 0], [20, 1]], dropped: 0 } }),
      ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: '12', payload: { capture_id: CAP, samplerate: 1_000_000, annotations: [ann(10, 95, '4C')], dropped: 0 } }),
    ]);
    const stop = q('logic-stop') as HTMLButtonElement;
    expect(stop.disabled).toBe(false);
    stop.click();
    await Promise.resolve();
    expect(h.net.calls.find((c) => c.method === 'logic.stop')!.params).toEqual({ capture_id: CAP });
    h.net.emitControl(state(30, 'stopped', { reason: 'user_stop', exit: { acquisition: null, decoder: { code: 0, signal: null } } }));
    h.net.emitControl(state(25, 'running'));
    h.frame();
    expect(q('logic-state').textContent).toContain('정지됨');
    expect(q('logic-notice').textContent).toContain('사용자 정지');
    expect(q('logic-notice').textContent).toContain('디코더 exit 0');
    expect((q('logic-stop') as HTMLButtonElement).disabled).toBe(true);
    // stopped capture keeps its decoded rows (class null shown as —, decoder id verbatim)
    const row = h.host.querySelector('.lg-table .vt-row[data-start="10"]')!;
    expect(row.textContent).toContain('4C');
    expect(row.textContent).toContain('uart-1');
    expect(row.textContent).toContain('—');
  });

  it('late join: §9 snapshot alone shows state, decode rows and counters', () => {
    h = mountPanel('logic.analyzer');
    h.net.subs[0]!.h.onSnapshotPart?.({
      sub: 's', n: 1, total: 1, events: [],
      items: [{ domain: 'logic', kind: 'logic.snapshot', state: {
        captures: [{ capture_id: CAP, state: 'running', source: 'sigrok-demo', driver: 'demo', samplerate: 200_000, channels: ['D0', 'D1'], counters: { samples: 5000, edges: 3, annotations: 1, dropped_edges: 7 } }],
        annotations: [{ capture_id: CAP, ...ann(1, 2, 'x') }],
        edges: [{ capture_id: CAP, sample_start: 0, sample_end: 10, initial: 0, edges: [[3, 1]], dropped: 0 }],
      } } as never],
    } as never);
    h.snapshotEnd('50');
    expect(q('logic-state').textContent).toContain('캡처 중');
    expect(q('logic-source').dataset.source).toBe('sigrok-demo');
    expect(q('logic-counters').querySelector('[data-counter="dropped_edges"]')!.className).toContain('lg-bad');
    expect(h.host.querySelectorAll('.lg-table .vt-row[data-start]').length).toBe(1);
    // a control event older than the snapshot high-water is stale
    h.net.emitControl(state(40, 'stopped'));
    h.frame();
    expect(q('logic-state').textContent).toContain('캡처 중');
  });

  it('watch right: start disabled, stop (safety direction) still enabled; start is request-class with backend params', async () => {
    h = mountPanel('logic.analyzer', {}, { hello: { rights: 'watch' } });
    h.net.emitControl(state(10, 'running'));
    h.frame();
    expect((q('logic-start') as HTMLButtonElement).disabled).toBe(true);
    expect((q('logic-stop') as HTMLButtonElement).disabled).toBe(false);
    h.dispose();
    h = mountPanel('logic.analyzer');
    h.frame();
    const start = q('logic-start') as HTMLButtonElement;
    expect(start.disabled).toBe(false);
    start.click();
    for (let i = 0; i < 4; i++) await Promise.resolve();
    const call = h.net.calls.find((c) => c.method === 'logic.start')!;
    for (const k of Object.keys(call.params as object)) expect(BACKEND_KEYS.has(k)).toBe(true);
    expect(q('logic-cli').textContent).toContain('loom logic start --source synthetic');
  });

  it('starter comes from payload.requested_by (live and snapshot), never from the device-origin envelope', () => {
    h = mountPanel('logic.analyzer');
    const meta = () => q('logic-meta').textContent!;
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: '10', origin: 'device', payload: { capture_id: CAP, state: 'running' } }));
    h.frame();
    expect(meta()).not.toContain('시작'); // unknown starter → not shown (device is the reporter)
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: '11', origin: 'device', payload: { capture_id: CAP, state: 'running', requested_by: 'agent' } }));
    h.frame();
    expect(meta()).toContain('시작 AGT');
    h.dispose();
    h = mountPanel('logic.analyzer');
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, events: [], items: [{ domain: 'logic', kind: 'logic.snapshot', state: { captures: [{ capture_id: CAP, state: 'stopped', requested_by: 'user' }], annotations: [], edges: [] } } as never] } as never);
    h.snapshotEnd('5');
    expect(meta()).toContain('시작 USR');
  });

  // window readout "a – b · x smp/px" → seconds on the sample clock
  const toS = (t: string) => {
    const m = /^([\d.]+) (s|ms|µs|ns)$/.exec(t.trim())!;
    return Number(m[1]) * { s: 1, ms: 1e-3, µs: 1e-6, ns: 1e-9 }[m[2] as 's' | 'ms' | 'µs' | 'ns'];
  };
  const win = () => {
    const t = q('logic-window').textContent!;
    if (!t) return null;
    const [range] = t.split(' · ');
    const [a, b] = range!.split(' – ');
    return { from: toS(a!), to: toS(b!), span: toS(b!) - toS(a!) };
  };
  const running = (id: string, seq: number, extra: Record<string, unknown> = {}) =>
    ev({ kind: 'logic.capture_state', domain: 'logic', channel: id, seq: String(seq), origin: 'device', payload: { capture_id: id, state: 'running', source: 'synthetic', channels: ['D0'], ...extra } });
  const edgeEv = (id: string, seq: number, start: number, end: number) =>
    ev({ kind: 'logic.edges', domain: 'logic', channel: id, seq: String(seq), payload: { capture_id: id, samplerate: 1_000_000, sample_start: start, sample_end: end, initial: 1, edges: [[start + 5, 0]], dropped: 0 } });

  it('view: empty draws keep the unsized sentinel; first rate/data gives a ~10 ms window that follows the head', () => {
    h = mountPanel('logic.analyzer');
    for (let i = 0; i < 5; i++) h.frame(); // no capture: nothing to size by
    expect(win()).toBeNull();
    h.net.emitControl(running(CAP, 10)); // no samplerate, no data yet
    h.frame();
    h.frame();
    expect(win()).toBeNull();
    h.batch([edgeEv(CAP, 11, 0, 21_601_000)]); // agent capture already 21.6 s in when the UI first sees data
    const w = win()!;
    expect(w.span).toBeGreaterThan(0.009);
    expect(w.span).toBeLessThan(0.011);
    expect(w.to).toBeCloseTo(21.601, 3);
    expect(q('logic-window').textContent).toContain('smp/px'); // not px/smp (the 64 px/sample bug)
  });

  it('view: user zoom stays on the same capture; a new latest capture resets to the default window + live follow', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(running(CAP, 10, { samplerate: 1_000_000 }));
    h.frame();
    expect(win()!.span).toBeCloseTo(0.01, 3);
    (q('logic-zoom-in') as HTMLButtonElement).click();
    (q('logic-zoom-in') as HTMLButtonElement).click();
    h.frame();
    expect(win()!.span).toBeCloseTo(0.0025, 4);
    h.batch([edgeEv(CAP, 11, 0, 50_000)]);
    expect(win()!.span).toBeCloseTo(0.0025, 4); // same capture: zoom kept
    h.net.emitControl(running('lc-2', 20, { samplerate: 1_000_000 }));
    h.batch([edgeEv('lc-2', 21, 0, 30_000)]);
    expect(win()!.span).toBeCloseTo(0.01, 3);
    expect(win()!.to).toBeCloseTo(0.03, 3);
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('true');
  });

  it('pinned capture ignores other captures', () => {
    h = mountPanel('logic.analyzer', { channel: CAP });
    expect(h.net.subs[0]!.filter.channels).toEqual([CAP]);
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: 'lc-other', seq: '5', payload: { capture_id: 'lc-other', state: 'running' } }));
    h.frame();
    expect(q('logic-state').textContent).toBe('캡처 없음');
  });
});

describe('waveform painter', () => {
  it('paints raw lanes, hatched unknown and decode boxes without throwing (stub 2D context)', async () => {
    const { paint } = await import('./waveform');
    const calls: string[] = [];
    const g = new Proxy({} as Record<string, unknown>, {
      get: (_t, k) => (k === 'canvas' ? null : (...a: unknown[]) => void calls.push(`${String(k)}:${a.length}`)),
      set: () => true,
    }) as unknown as CanvasRenderingContext2D;
    const c = cap(edges(0, 50, 0, [[10, 1], [20, 0]], { dropped: 2 }), edges(80, 100, 1, []));
    c.addAnnotations(readAnnotations('logic.annotations', { capture_id: CAP, annotations: [ann(10, 30, '4C')] }, CAP, 1n, 1e6).rows, 0);
    const pal = { bg: '#fff', grid: '#eee', fg: '#000', muted: '#666', hi: '#070', lo: '#070', dense: '#8c8', unknown: '#c80', ann: '#36c', annText: '#126', cursor: '#d40', sel: '#d40' };
    paint(g, { cap: c, lanes: [{ bit: 0, name: 'D0' }], decoders: ['uart-1'], view: { s0: 0, spp: 0.5, width: 200 }, rate: 1e6, palette: pal, hoverX: 12, selected: null }, 100);
    expect(calls.some((x) => x.startsWith('fillText'))).toBe(true);
    expect(calls.filter((x) => x.startsWith('clip')).length).toBeGreaterThan(0); // hatch + annotation text clip
  });
});

describe('axis ticks (zoomed far from t=0)', () => {
  it('labels stay distinct at ~300 µs span around 2.7 s @ 1 MHz', () => {
    const view = { s0: 2_700_900, spp: 300 / 800, width: 800 };
    const ticks = axisTicks(view, 1e6);
    const labels = ticks.map((t) => t.label).filter((l) => l !== '');
    expect(labels.length).toBeGreaterThanOrEqual(3);
    expect(new Set(labels).size).toBe(labels.length);
    expect(labels.every((l) => l.startsWith('2.70') && l.endsWith(' s'))).toBe(true);
    // labelled ticks are far enough apart for their text
    const xs = ticks.filter((t) => t.label !== '').map((t) => t.x);
    const longest = Math.max(...labels.map((l) => l.length));
    for (let i = 1; i < xs.length; i++) expect(xs[i]! - xs[i - 1]!).toBeGreaterThanOrEqual(longest * AXIS_CHAR_PX);
  });

  it('resolves ns steps and sample-index axes', () => {
    const ns = axisTicks({ s0: 5_000_000_000, spp: 0.05, width: 600 }, 1e9).map((t) => t.label).filter(Boolean);
    expect(new Set(ns).size).toBe(ns.length);
    expect(ns.length).toBeGreaterThan(1);
    const idx = axisTicks({ s0: 123_456, spp: 0.5, width: 600 }, null).map((t) => t.label).filter(Boolean);
    expect(new Set(idx).size).toBe(idx.length);
  });
});

describe('selected annotation row contrast', () => {
  // vitest blanks CSS modules (even `?raw`) — read the file itself (same as shell/hiddenCss.test.ts)
  let logicCss = '';
  beforeAll(async () => {
    // jsdom gives import.meta.url a non-file scheme here; resolve from the ui/ working directory.
    const fs = (await import(/* @vite-ignore */ 'node:fs' as string)) as { readFileSync(p: string, enc: 'utf8'): string };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    logicCss = fs.readFileSync(`${cwd}/src/panels/logic/logic.css`, 'utf8');
  });
  it('is scoped to the logic panel and light with dark text', () => {
    expect(logicCss).toMatch(/\.lg-panel \.vt-row\[aria-selected='true'\] \{ background: #d6e6ff; color: #10213a; \}/);
    expect(logicCss).not.toMatch(/^\.vt-row/m);
  });
});
