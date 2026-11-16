// @vitest-environment jsdom
// operator-upgrade logic — batch 1 specification tests (Opus-authored; Sonnet must not edit).
// Findings: L2 decoder options · L5 honest partial (held vs engine) · L8a/b/c live/layout · L1 pre-start load warning.
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md (B1-*).
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import {
  annCoverageText,
  CaptureData,
  cliLine,
  coverageNote,
  decodeCoverage,
  decoderSpec,
  DEFAULT_CONFIG,
  estimatePublishLoad,
  followHeadSample,
  rawCoverage,
  readAnnotations,
  readCaptureState,
  readEdges,
  sanitizeConfig,
  startParams,
  validateConfig,
  type DecodeCoverage,
  type RawCoverage,
  type StartConfig,
} from './logicModel';
import { AXIS_HEADER, GUTTER, paint } from './waveform';

const CAP = 'lc-b1';
const RATE = 1_000_000;
const edges = (start: number, end: number, initial: number, list: unknown[], extra: Record<string, unknown> = {}) =>
  readEdges({ capture_id: CAP, samplerate: RATE, sample_start: start, sample_end: end, initial, edges: list, dropped: 0, ...extra }, CAP, 1n, null)!;
const annRow = (start: number, end: number, text: string, decoder = 'uart-1') => ({ decoder, class: null, start_sample: start, end_sample: end, text });
const withState = (c: CaptureData, state: string, counters?: Record<string, number>) => {
  c.info = readCaptureState({ capture_id: CAP, state, samplerate: RATE, ...(counters ? { counters } : {}) }, CAP, 5n, null);
  return c;
};
const BACKEND_KEYS = new Set(['source', 'driver', 'conn', 'samplerate', 'channels', 'decoders', 'duration_ms', 'max_samples', 'pattern', 'text', 'frame_period_ms', 'clock_hz']);

interface SpecCase {
  name: string;
  config: Partial<StartConfig>;
  spec: string;
  canonical: string;
  synthetic_ok: boolean;
}
let specCases: SpecCase[] = [];
beforeAll(async () => {
  const fs = (await import(/* @vite-ignore */ 'node:fs' as string)) as { readFileSync(p: string, enc: 'utf8'): string };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  specCases = (JSON.parse(fs.readFileSync(`${cwd}/../crates/loom-logic/tests/fixtures/ui-decoder-specs.json`, 'utf8')) as { cases: SpecCase[] }).cases;
});

// ---------------------------------------------------------------------------------------------
describe('B1 L2 — decoder options reach the backend wire', () => {
  it('decoderSpec emits exactly the backend-verified spec for every shared fixture case', () => {
    expect(specCases.length).toBeGreaterThanOrEqual(8);
    for (const k of specCases) expect(decoderSpec({ ...DEFAULT_CONFIG, ...k.config }), k.name).toBe(k.spec);
  });

  it('a device capture with 8E1 sends the option; keys stay backend-only; the CLI line carries the same spec', () => {
    const c: StartConfig = { ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', uartParity: 'even' };
    expect(validateConfig(c)).toEqual([]);
    const p = startParams(c, ['D0']);
    for (const k of Object.keys(p)) expect(BACKEND_KEYS.has(k)).toBe(true);
    expect(p.decoders).toEqual(['uart:baudrate=115200:rx=D0:parity=even']);
    expect(cliLine(c, ['D0'])).toContain('--decode uart:baudrate=115200:rx=D0:parity=even');
  });

  it('synthetic refuses what the generator cannot shape (mirrors backend NotImplemented), device accepts it', () => {
    for (const k of specCases) {
      const synth = validateConfig({ ...DEFAULT_CONFIG, samplerate: 2_000_000, ...k.config });
      if (k.synthetic_ok) expect(synth, k.name).toEqual([]);
      else expect(synth.join('\n'), k.name).toMatch(/합성 (UART는 8N1 LSB-first만|SPI는 8-bit 워드만) 생성합니다/);
      expect(validateConfig({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', ...k.config }), k.name).toEqual([]);
    }
  });

  it('refuses incomplete or out-of-range decoder options before the RPC', () => {
    const dev = { ...DEFAULT_CONFIG, source: 'sigrok-device' as const, driver: 'fx2lafw' };
    expect(validateConfig({ ...dev, uartDataBits: 4 }).join()).toContain('UART 데이터 비트는 5–9');
    expect(validateConfig({ ...dev, uartLine: 'tx', uartTx: 8 }).join()).toContain('UART TX 채널이 캡처 채널 밖입니다');
    const spi = { ...dev, decoder: 'spi' as const };
    expect(validateConfig({ ...spi, spiMosi: null, spiMiso: null }).join()).toContain('SPI는 MOSI 또는 MISO가 필요합니다');
    expect(validateConfig({ ...spi, spiAnnotate: 'miso' }).join()).toContain('MISO 표시를 고르려면 MISO 핀이 필요합니다');
    expect(validateConfig({ ...spi, spiMosi: null, spiMiso: 3, spiAnnotate: 'mosi' }).join()).toContain('MOSI 표시를 고르려면 MOSI 핀이 필요합니다');
    expect(validateConfig({ ...spi, spiWordsize: 33 }).join()).toContain('SPI 워드 크기는 1–32');
    expect(validateConfig({ ...spi, spiMosi: 0 }).join()).toContain('SPI 핀이 겹칩니다');
  });

  it('sanitizeConfig: a pre-upgrade saved config keeps its values and gains defaults; junk falls back per key', () => {
    const old = { source: 'sigrok-device', driver: 'fx2lafw', uartBaud: 9600, uartRx: 4, spiMosi: 1, decoder: 'uart' }; // no new keys
    const c = sanitizeConfig(old);
    expect(c).toMatchObject({ source: 'sigrok-device', driver: 'fx2lafw', uartBaud: 9600, uartRx: 4, uartLine: 'rx', uartParity: 'none', uartDataBits: 8, spiCpol: 0, spiAnnotate: 'mosi' });
    const junk = sanitizeConfig({ uartParity: 'mark', uartStopBits: 2, spiCpol: 3, spiMosi: null, spiMiso: 'x', samplerate: Number.NaN, source: 'saleae', decoder: 'i2c', extra: 1 });
    expect(junk.uartParity).toBe('none');
    expect(junk.uartStopBits).toBe('1');
    expect(junk.spiCpol).toBe(0);
    expect(junk.spiMosi).toBeNull(); // null is a valid pin choice ("없음")
    expect(junk.spiMiso).toBeNull(); // DEFAULT
    expect(junk.samplerate).toBe(DEFAULT_CONFIG.samplerate);
    expect(junk.source).toBe('synthetic');
    expect(junk.decoder).toBe('uart');
    expect('extra' in junk).toBe(false);
    for (const bad of [null, 7, 'x', []]) expect(sanitizeConfig(bad)).toEqual(DEFAULT_CONFIG);
    const fresh = sanitizeConfig(null);
    fresh.uartBaud = 1;
    expect(DEFAULT_CONFIG.uartBaud).toBe(115_200); // never hands out the shared default object
  });
});

// ---------------------------------------------------------------------------------------------
describe('B1 L1 — pre-start publish-load estimate (warning, never a blocker)', () => {
  it('default synthetic UART stays well under both budgets, no warnings', () => {
    const e = estimatePublishLoad(DEFAULT_CONFIG, ['D0']);
    // "LOOM 0000\r\n" = 11 B per 100 ms frame
    expect(e.annotationBasis).toBe('synthetic');
    expect(e.annotationsPerS).toBeCloseTo(110, 6);
    expect(e.edgesPerS).toBeCloseTo(1100, 6);
    expect(e.warnings).toEqual([]);
  });

  it('ROOT check: 256 B text every 25 ms at 115200 exceeds both live budgets', () => {
    const c = { ...DEFAULT_CONFIG, text: 'A'.repeat(256), framePeriodMs: 25 };
    expect(validateConfig(c)).toEqual([]); // still startable
    const e = estimatePublishLoad(c, ['D0']);
    expect(e.annotationsPerS).toBeCloseTo(10_240, 6); // frame (2+2560)/115200 s < 25 ms period
    expect(e.edgesPerS).toBeCloseTo(102_400, 6);
    expect(e.warnings).toHaveLength(2);
    expect(e.warnings[0]).toBe("예상 디코드 10,240행/s > live 게시 상한 2,000행/s — 초과분은 live 미리보기·저널에 실리지 않고 '디코드 손실'로 셉니다");
    expect(e.warnings[1]).toBe("예상 전이 102,400개/s > live 게시 상한 10,240개/s — 초과 구간은 파형에 '데이터 없음'(빗금)으로 표시됩니다");
  });

  it('a long synthetic frame is limited by its own duration, not the period', () => {
    // 256 B at 9600 baud: frame = (2 + 2560)/9600 s ≈ 266.9 ms > 25 ms period
    const e = estimatePublishLoad({ ...DEFAULT_CONFIG, text: 'A'.repeat(256), framePeriodMs: 25, uartBaud: 9600 }, ['D0']);
    expect(e.annotationsPerS).toBeCloseTo(256 / (2562 / 9600), 6);
  });

  it('device UART: worst case continuous traffic from the frame format, labelled as such', () => {
    const e = estimatePublishLoad({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw' }, ['D0']);
    expect(e.annotationBasis).toBe('worst_case');
    expect(e.annotationsPerS).toBeCloseTo(11_520, 6); // 115200 / (1+8+0+1)
    expect(e.edgesPerS).toBeCloseTo(115_200, 6);
    expect(e.warnings[0]).toBe("예상 디코드 11,520행/s (연속 송신 시 최대) > live 게시 상한 2,000행/s — 초과분은 live 미리보기·저널에 실리지 않고 '디코드 손실'로 셉니다");
    const e2 = estimatePublishLoad({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', uartBaud: 9600, uartParity: 'even', uartStopBits: '1.5' }, ['D0']);
    expect(e2.annotationsPerS).toBeCloseTo(9600 / 11.5, 6);
    expect(e2.warnings).toEqual([]);
  });

  it('hidden decoder lane publishes no edges for it; device SPI clock is unknown and said so', () => {
    expect(estimatePublishLoad(DEFAULT_CONFIG, ['D5']).edgesPerS).toBe(0);
    const spi = estimatePublishLoad({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', decoder: 'spi' });
    expect(spi.annotationsPerS).toBeNull();
    expect(spi.edgesPerS).toBeNull();
    expect(spi.edgeBasis).toBe('unknown');
    expect(spi.warnings).toEqual(['SPI 클록을 알 수 없어 게시량을 추정하지 못했습니다 — 1 MHz SPI는 전이 약 2,000,000개/s로 live 게시 상한을 크게 넘습니다']);
    const raw = estimatePublishLoad({ ...DEFAULT_CONFIG, source: 'sigrok-device', driver: 'fx2lafw', decoder: 'none' });
    expect(raw).toMatchObject({ annotationsPerS: 0, annotationBasis: 'none', edgesPerS: null, edgeBasis: 'unknown' });
    expect(raw.warnings).toEqual(['실제 신호 밀도를 알 수 없어 전이 게시량을 추정하지 못했습니다 — 상한 10,240개/s 초과분은 빗금으로 표시됩니다']);
    expect(estimatePublishLoad({ ...DEFAULT_CONFIG, source: 'sigrok-demo', decoder: 'none' }).warnings).toEqual([]);
  });

  it('synthetic SPI uses the backend default clock (samplerate/10)', () => {
    // 1 MS/s → clk 100 kHz, half = 5 samples; frame = 8h + 16h·B samples; "LOOM 0000\r\n" B=11 → 920 samples < 100 ms
    const e = estimatePublishLoad({ ...DEFAULT_CONFIG, decoder: 'spi' }, ['D0', 'D1', 'D2']);
    expect(e.annotationsPerS).toBeCloseTo(110, 6);
    expect(e.edgesPerS).toBeCloseTo(110 * 16 + 110 * 8 + 10 * 2, 6); // CLK + MOSI + CS per frame
  });
});

// ---------------------------------------------------------------------------------------------
describe('B1 L5 — held vs engine coverage (model)', () => {
  const withAnns = (n: number) => {
    const c = new CaptureData(CAP);
    const rows = Array.from({ length: n }, (_, i) => annRow(i * 10, i * 10 + 9, `4${i}`));
    c.addAnnotations(readAnnotations('logic.annotations', { capture_id: CAP, annotations: rows }, CAP, 1n, RATE).rows, 0);
    return c;
  };

  it('stopped capture holding fewer rows than the engine published is partial with the exact missing count', () => {
    const d = decodeCoverage(withState(withAnns(3), 'stopped', { annotations: 5, samples: 100 }));
    expect(d).toEqual({ published: 5, engineDropped: 0, held: 3, evicted: 0, notReceived: 2, state: 'partial' });
  });

  it('complete only when nothing is missing, dropped or evicted; raw unparsed lines count as held', () => {
    expect(decodeCoverage(withState(withAnns(3), 'stopped', { annotations: 3 })).state).toBe('complete');
    const c = withAnns(2);
    c.addRawLines(['uart-1: garbled'], 0);
    expect(decodeCoverage(withState(c, 'stopped', { annotations: 3 }))).toMatchObject({ held: 3, notReceived: 0, state: 'complete' });
    expect(decodeCoverage(withState(withAnns(3), 'stopped', { annotations: 3, dropped_annotations: 4 }))).toMatchObject({ engineDropped: 4, state: 'partial' });
  });

  it('running → live (counters are stale until the final state); no counters → unknown', () => {
    expect(decodeCoverage(withState(withAnns(3), 'running', { annotations: 0 }))).toMatchObject({ state: 'live', notReceived: 0 });
    expect(decodeCoverage(withState(withAnns(3), 'stopped')).state).toBe('unknown');
    expect(decodeCoverage(new CaptureData(CAP)).state).toBe('unknown');
  });

  it('raw: a front part never received is partial (not UI eviction); union of unknown ranges is not double counted', () => {
    const c = new CaptureData(CAP);
    c.addChunk(edges(1000, 2000, 0, []));
    c.addChunk(edges(3000, 4000, 0, [[3100, 1]], { dropped: 5 })); // edges_capped [3101, 4000)
    withState(c, 'stopped', { samples: 4000 });
    expect(rawCoverage(c)).toEqual({
      engineSamples: 4000,
      heldStart: 1000,
      heldEnd: 4000,
      evicted: 0,
      notReceivedBefore: 1000,
      notReceivedAfter: 0,
      unknownInside: 1000 + 899, // chunk gap [2000,3000) + capped [3101,4000)
      state: 'partial',
    });
  });

  it('raw complete / tail missing / nothing received', () => {
    const full = new CaptureData(CAP);
    full.addChunk(edges(0, 500, 1, [[100, 0]]));
    expect(rawCoverage(withState(full, 'stopped', { samples: 500 })).state).toBe('complete');
    expect(rawCoverage(withState(full, 'stopped', { samples: 800 }))).toMatchObject({ notReceivedAfter: 300, state: 'partial' });
    expect(rawCoverage(withState(full, 'running', { samples: 800 }))).toMatchObject({ notReceivedAfter: 0, state: 'live' });
    const none = withState(new CaptureData(CAP), 'stopped', { samples: 700 });
    expect(rawCoverage(none)).toMatchObject({ heldStart: null, heldEnd: null, notReceivedAfter: 700, state: 'partial' });
    expect(rawCoverage(withState(new CaptureData(CAP), 'stopped', { samples: 0 })).state).toBe('complete');
  });

  it('coverageNote / annCoverageText: exact texts, fixed part order, nothing while live', () => {
    const d = (o: Partial<DecodeCoverage>): DecodeCoverage => ({ published: 5, engineDropped: 0, held: 5, evicted: 0, notReceived: 0, state: 'complete', ...o });
    const r = (o: Partial<RawCoverage>): RawCoverage => ({ engineSamples: 10, heldStart: 0, heldEnd: 10, evicted: 0, notReceivedBefore: 0, notReceivedAfter: 0, unknownInside: 0, state: 'complete', ...o });
    expect(coverageNote(d({}), r({}))).toBe(' — 이 패널이 엔진 게시분 전체를 보유합니다');
    expect(coverageNote(d({ state: 'live' }), r({ state: 'live' }))).toBe('');
    expect(coverageNote(d({ state: 'unknown', published: null }), r({}))).toBe(' — 엔진 카운터 미수신: 이 패널 보유분이 전체인지 알 수 없습니다');
    expect(coverageNote(d({ state: 'complete' }), r({ state: 'unknown' }))).toBe(' — 엔진 카운터 미수신: 이 패널 보유분이 전체인지 알 수 없습니다');
    expect(
      coverageNote(
        d({ published: 1515, held: 1365, notReceived: 150, engineDropped: 2, evicted: 3, state: 'partial' }),
        r({ notReceivedBefore: 4000, notReceivedAfter: 7, unknownInside: 12_345, evicted: 9, state: 'partial' }),
      ),
    ).toBe(' — 이 패널은 일부만 보유합니다: 디코드 1,365/1,515행 (미수신 150) · 게시 손실 2행 · 디코드 보관 밖 3행 · 원시 앞부분 sample 4,000 미수신 · 원시 뒷부분 sample 7 미수신 · 원시 데이터 없음 sample 12,345 · 원시 보관 밖 sample 9');
    expect(coverageNote(d({}), r({ unknownInside: 5, state: 'partial' }))).toBe(' — 이 패널은 일부만 보유합니다: 원시 데이터 없음 sample 5');
    expect(annCoverageText(d({ published: 1515, held: 1365, notReceived: 150, state: 'partial' }))).toBe(' · 패널 보유 1,365 / 엔진 게시 1,515 — 150행 미수신');
    expect(annCoverageText(d({ engineDropped: 9, state: 'partial' }))).toBe('');
    expect(annCoverageText(d({ published: null, state: 'unknown' }))).toBe('');
  });

  it('L8a follow anchor is the received raw end, not a decode end beyond it', () => {
    const c = new CaptureData(CAP);
    expect(followHeadSample(c)).toBe(0);
    c.addAnnotations(readAnnotations('logic.annotations', { capture_id: CAP, annotations: [annRow(60_000, 61_000, '41')] }, CAP, 1n, RATE).rows, 0);
    expect(followHeadSample(c)).toBe(61_000); // no raw yet → previous head rule
    c.addChunk(edges(0, 50_000, 1, []));
    expect(followHeadSample(c)).toBe(50_000);
  });
});

// ---------------------------------------------------------------------------------------------
describe('B1 panel', () => {
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
    f.value = value;
    f.dispatchEvent(new Event('change', { bubbles: true }));
    h!.frame();
  };
  const state = (seq: number, st: string, extra: Record<string, unknown> = {}) =>
    ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: String(seq), origin: 'device', payload: { capture_id: CAP, state: st, source: 'synthetic', samplerate: RATE, channels: ['D0'], decoders: ['uart:rx=D0:baudrate=115200'], ...extra } });
  const edgeEv = (seq: number, start: number, end: number) =>
    ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: String(seq), payload: { capture_id: CAP, samplerate: RATE, sample_start: start, sample_end: end, initial: 1, edges: [[start + 5, 0]], dropped: 0 } });
  const annEv = (seq: number, rows: unknown[]) => ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: String(seq), payload: { capture_id: CAP, samplerate: RATE, annotations: rows, dropped: 0 } });
  const toS = (t: string) => {
    const m = /^([\d.]+) (s|ms|µs|ns)$/.exec(t.trim())!;
    return Number(m[1]) * { s: 1, ms: 1e-3, µs: 1e-6, ns: 1e-9 }[m[2] as 's' | 'ms' | 'µs' | 'ns'];
  };
  const winTo = () => toS(q('logic-window').textContent!.split(' · ')[0]!.split(' – ')[1]!);

  it('L5: a stopped capture missing published rows says so in the table info and the stop notice', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.batch([edgeEv(11, 0, 100), annEv(12, [annRow(10, 20, '41'), annRow(30, 40, '42'), annRow(50, 60, '43')])]);
    h.net.emitControl(state(20, 'stopped', { reason: 'user_stop', counters: { samples: 100, annotations: 5 } }));
    h.frame();
    expect(q('logic-ann-info').textContent).toContain('패널 보유 3 / 엔진 게시 5 — 2행 미수신');
    const notice = q('logic-notice').textContent!;
    expect(notice).toContain('이 패널은 일부만 보유합니다: 디코드 3/5행 (미수신 2)');
    expect(notice).not.toContain('기록은 이 패널에 남아 있습니다');
  });

  it('L5: complete capture is stated as complete; missing counters are stated as unknown', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.batch([edgeEv(11, 0, 100), annEv(12, [annRow(10, 20, '41')])]);
    h.net.emitControl(state(20, 'stopped', { reason: 'duration', counters: { samples: 100, annotations: 1 } }));
    h.frame();
    expect(q('logic-notice').textContent).toContain('이 패널이 엔진 게시분 전체를 보유합니다');
    expect(q('logic-ann-info').textContent).not.toContain('미수신');
    h.dispose();
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(30, 'stopped', { reason: 'duration' }));
    h.frame();
    expect(q('logic-notice').textContent).toContain('엔진 카운터 미수신: 이 패널 보유분이 전체인지 알 수 없습니다');
  });

  it('L5: raw front never received is named with its sample count', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.batch([edgeEv(11, 4000, 5000)]);
    h.net.emitControl(state(20, 'stopped', { reason: 'duration', counters: { samples: 5000, annotations: 0 } }));
    h.frame();
    expect(q('logic-notice').textContent).toContain('원시 앞부분 sample 4,000 미수신');
  });

  it('L8a: live follow ends at the received raw end even when a decode box is already further', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.batch([edgeEv(11, 0, 50_000), annEv(12, [annRow(60_000, 61_000, '41')])]);
    expect(winTo()).toBeCloseTo(0.05, 4);
  });

  it('L8c: follow turns off when the shown capture stops (view stays at its end)', () => {
    h = mountPanel('logic.analyzer');
    h.net.emitControl(state(10, 'running'));
    h.batch([edgeEv(11, 0, 50_000)]);
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('true');
    h.net.emitControl(state(20, 'stopped', { reason: 'duration', counters: { samples: 50_000 } }));
    h.frame();
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('false');
    expect(winTo()).toBeCloseTo(0.05, 4);
    h.frame();
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('false');
  });

  it('L8c: reopening onto an existing capture collapses the config once; a user-opened config is never re-collapsed', () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    expect(q<HTMLDetailsElement>('logic-config').open).toBe(true); // nothing captured yet
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, events: [], items: [{ domain: 'logic', kind: 'logic.snapshot', state: { captures: [{ capture_id: CAP, state: 'stopped', source: 'synthetic' }], annotations: [], edges: [] } } as never] } as never);
    h.snapshotEnd('5');
    h.frame();
    const cfg = q<HTMLDetailsElement>('logic-config');
    expect(cfg.open).toBe(false);
    cfg.querySelector('summary')!.click();
    cfg.open = true; // jsdom does not toggle <details> on summary click
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: 'lc-b2', seq: '50', origin: 'device', payload: { capture_id: 'lc-b2', state: 'running' } }));
    h.frame();
    expect(cfg.open).toBe(true);
  });

  it('L2: advanced options exist per decoder, change the CLI line, and synthetic-only limits block start', () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    for (const n of ['uart_line', 'uart_tx', 'uart_data_bits', 'uart_parity', 'uart_stop_bits', 'uart_bit_order']) expect(field(n), n).not.toBeNull();
    setField('uart_parity', 'even');
    expect(q('logic-cli').textContent).toContain('--decode uart:baudrate=115200:rx=D0:parity=even');
    expect(q('logic-config-problems').textContent).toContain('합성 UART는 8N1 LSB-first만 생성합니다');
    expect(q<HTMLButtonElement>('logic-start').disabled).toBe(true);
    setField('uart_line', 'tx');
    expect(field('uart_tx').closest('label')!.hidden).toBe(false);
    expect(field('rx').closest('label')!.hidden).toBe(true);
    setField('uart_tx', '3');
    expect(q('logic-cli').textContent).toContain('uart:baudrate=115200:tx=D3:parity=even');
    setField('decoder', 'spi');
    for (const n of ['spi_mode', 'spi_wordsize', 'spi_bit_order', 'spi_cs_polarity', 'spi_annotate']) expect(field(n).closest('label')!.hidden, n).toBe(false);
    expect(field('uart_parity').closest('label')!.hidden).toBe(true);
    setField('spi_mode', '3');
    setField('miso', '4');
    setField('spi_annotate', 'miso');
    expect(q('logic-cli').textContent).toContain('spi:clk=D0:mosi=D1:miso=D4:cs=D2:cpol=1:cpha=1:annotate=miso');
    // persisted for the next panel (sanitizeConfig round-trip)
    h.dispose();
    h = mountPanel('logic.analyzer');
    h.frame();
    expect(field('spi_mode').value).toBe('3');
    expect(field('decoder').value).toBe('spi');
  });

  it('L2: a device start sends the advanced spec verbatim', async () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    setField('source', 'sigrok-device');
    setField('driver', 'fx2lafw');
    setField('uart_data_bits', '7');
    setField('uart_parity', 'odd');
    setField('uart_stop_bits', '2');
    expect(q('logic-config-problems').textContent).toBe('');
    q<HTMLButtonElement>('logic-start').click();
    for (let i = 0; i < 4; i++) await Promise.resolve();
    const call = h.net.calls.find((c) => c.method === 'logic.start')!;
    expect((call.params as { decoders: string[] }).decoders).toEqual(['uart:baudrate=115200:rx=D0:data_bits=7:parity=odd:stop_bits=2']);
  });

  it('L1: load warnings are shown separately and never disable start', () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    expect(q('logic-config-warnings').textContent).toBe('');
    setField('text', 'A'.repeat(256));
    setField('frame_period', '25');
    const w = q('logic-config-warnings');
    expect(w.querySelectorAll('li')).toHaveLength(2);
    expect(w.textContent).toContain('> live 게시 상한 2,000행/s');
    expect(q('logic-config-problems').textContent).toBe('');
    expect(q<HTMLButtonElement>('logic-start').disabled).toBe(false);
  });
});

// ---------------------------------------------------------------------------------------------
describe('B1 L8b — axis header stays inside the gutter', () => {
  const record = () => {
    const calls: [string, unknown[]][] = [];
    const g = new Proxy({} as Record<string, unknown>, {
      get: (_t, k) => (k === 'canvas' ? null : (...a: unknown[]) => void calls.push([String(k), a])),
      set: () => true,
    }) as unknown as CanvasRenderingContext2D;
    return { g, calls };
  };
  const pal = { bg: '#fff', grid: '#eee', fg: '#000', muted: '#666', hi: '#070', lo: '#070', dense: '#8c8', unknown: '#c80', ann: '#36c', annText: '#126', cursor: '#d40', sel: '#d40' };

  for (const [rate, text] of [[RATE, AXIS_HEADER.time], [null, AXIS_HEADER.sample]] as const) {
    it(`header "${text}" is drawn under a gutter clip (rate ${rate ?? 'none'})`, () => {
      const { g, calls } = record();
      const c = new CaptureData(CAP);
      c.addChunk(edges(0, 100, 0, [[10, 1]]));
      paint(g, { cap: c, lanes: [{ bit: 0, name: 'D0' }], decoders: [], view: { s0: 0, spp: 1, width: 400 }, rate, palette: pal, hoverX: null, selected: null }, 80);
      const at = calls.findIndex(([k, a]) => k === 'fillText' && a[0] === text);
      expect(at).toBeGreaterThan(0);
      const save = calls.slice(0, at).map(([k]) => k).lastIndexOf('save');
      expect(save).toBeGreaterThanOrEqual(0);
      const between = calls.slice(save, at);
      const rect = between.find(([k]) => k === 'rect')!;
      expect(rect[1]).toEqual([0, 0, GUTTER - 4, 22]);
      expect(between.some(([k]) => k === 'clip')).toBe(true);
      expect(calls.slice(at + 1).some(([k]) => k === 'restore')).toBe(true);
      expect(calls.some(([k, a]) => k === 'fillText' && a[0] === '시간(sample clock)')).toBe(false);
    });
  }
});
