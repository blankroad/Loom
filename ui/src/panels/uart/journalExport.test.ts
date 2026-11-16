// U1-B batch 5 — journal export (FUNCTIONS.md §B5.1). Opus-owned specification; Sonnet must not edit.
// A fake `core.events.query` with the daemon's paging/retention/error semantics (core_rpc.rs events_query).
import { describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import {
  cursorAt,
  errCode,
  errDetail,
  exportFileName,
  exportHeader,
  ExportWriter,
  EXPORT_KINDS,
  EXPORT_QUERY,
  findFirstSeq,
  fmtWall,
  GapTracker,
  rangeText,
  runExport,
  summaryText,
  wallMsOf,
  type ExportOptions,
} from './journalExport';

const T0 = new Date(2026, 10, 16, 9, 0, 0, 0).getTime();
const MIN = 60_000;
const hex = (s: string) => Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, '0')).join('');

function rec(seq: number, kind: string, o: Partial<LoomEvent> & { wallMs?: number; payload?: Record<string, unknown> } = {}): LoomEvent {
  const wall = o.wallMs ?? T0 + seq * 1000;
  return {
    schema_version: 1,
    session_id: 's',
    epoch: '7',
    seq: String(seq),
    domain: 'uart',
    channel: 'dut',
    kind,
    origin: 'device',
    producer_id: 'p-uart',
    producer_epoch: '3',
    source_seq: String(seq * 10),
    generation: 1,
    ts: { mono_ns: String(seq * 1_000_000), wall_ns: `${wall}000000`, clock_id: 'host', source: 'host_read_complete' },
    ...o,
    payload: o.payload ?? {},
  } as unknown as LoomEvent;
}
const rx = (seq: number, text: string, rxOffset: number, o: Partial<LoomEvent> & { wallMs?: number } = {}) =>
  rec(seq, 'uart.data', { dir: 'rx', ...o, payload: { bytes: { encoding: 'hex', data: hex(text) }, len: new TextEncoder().encode(text).length, rx_offset: rxOffset } });
const tx = (seq: number, text: string, o: Partial<LoomEvent> = {}) =>
  rec(seq, 'uart.data', { dir: 'tx', origin: 'agent', request_id: 'r-1', client_id: 'agent#1', ...o, payload: { bytes: { encoding: 'hex', data: hex(text) }, len: text.length } });
const state = (seq: number, st: string, generation = 1) => rec(seq, 'uart.state', { generation, payload: { state: st, path: '/dev/ttyUSB0' } });

class NetErr extends Error {
  constructor(readonly dataCode: string, message: string, readonly data: unknown) {
    super(message);
  }
}

interface FakeOpts {
  oldest?: number;
  /** throw E_SCAN_LIMIT once when a page would start at this seq (partial = first 1 event) */
  scanLimitAt?: number;
  /** add `unreadable` to the page that starts at this seq */
  unreadableAt?: { at: number; from: number; to: number };
  /** retention moves: pages starting at/after this seq answer E_CURSOR_GAP */
  cursorGapFrom?: number;
  /** every call fails with this */
  failWith?: Error;
}

function fakeJournal(records: LoomEvent[], f: FakeOpts = {}) {
  const calls: Record<string, unknown>[] = [];
  const hw = records.length ? Number(records[records.length - 1]!.seq) : 0;
  const oldest = f.oldest ?? 1;
  let scanned = false;
  const query = async (rpc: string, params: object, signal?: AbortSignal): Promise<unknown> => {
    expect(rpc).toBe(EXPORT_QUERY);
    const p = params as { since?: string; until?: string; max?: number; peek?: boolean; filter?: unknown };
    calls.push(p as Record<string, unknown>);
    expect(p.peek).toBe(true);
    expect(p.filter).toEqual({ domains: ['uart'], channels: ['dut'], kinds: [...EXPORT_KINDS] });
    if (signal?.aborted) throw new DOMException('aborted', 'AbortError');
    if (f.failWith) throw f.failWith;
    const max = p.max ?? 100;
    const base = { stream_epoch: '7', stream_seq_hw: String(hw) };
    if (!p.since) {
      const ev = records.slice(-max);
      return { ...base, events: ev, count: ev.length, truncated: false, next_cursor: null };
    }
    const m = /^e7:seq:(\d+)$/.exec(p.since);
    expect(m).not.toBeNull();
    const from = Number(m![1]);
    const until = p.until ? Number(/seq:(\d+)$/.exec(p.until)![1]) : hw;
    if (from < oldest) throw new NetErr('E_CURSOR_GAP', 'this range is not in the recording', { code: 'E_CURSOR_GAP', detail: { stream_epoch: '7', from: String(from) } });
    // the cases below hit page reads only (max > 1), never the 1-row locate probes
    if (f.cursorGapFrom !== undefined && from >= f.cursorGapFrom && max > 1) throw new NetErr('E_CURSOR_GAP', 'retention moved', { code: 'E_CURSOR_GAP' });
    const inRange = records.filter((r) => Number(r.seq) >= from && Number(r.seq) <= until);
    if (f.scanLimitAt === from && !scanned && max > 1) {
      scanned = true;
      throw new NetErr('E_SCAN_LIMIT', 'scan budget reached', { code: 'E_SCAN_LIMIT', detail: { resume_from: `e7:seq:${Number(inRange[0]!.seq) + 1}`, partial: inRange.slice(0, 1), scanned_bytes: '9' } });
    }
    const page = inRange.slice(0, max);
    const truncated = inRange.length > max;
    const out: Record<string, unknown> = {
      ...base,
      events: page,
      count: page.length,
      truncated,
      next_cursor: truncated ? `e7:seq:${Number(page[page.length - 1]!.seq) + 1}` : null,
      source: 'journal',
    };
    if (f.unreadableAt && f.unreadableAt.at === from) out.unreadable = [{ from: String(f.unreadableAt.from), to: String(f.unreadableAt.to), cause: 'producer_not_recorded' }];
    return out;
  };
  return { query, calls };
}

const opts = (q: ExportOptions['query'], o: Partial<ExportOptions> = {}): ExportOptions => ({
  query: q,
  channel: 'dut',
  range: { kind: 'retained' },
  format: 'log',
  signal: new AbortController().signal,
  nowMs: T0 + 3600_000,
  ...o,
});

// ---------------------------------------------------------------- small pure helpers

describe('helpers', () => {
  it('wallMsOf / cursorAt / fmtWall / exportFileName / rangeText', () => {
    expect(wallMsOf(rec(1, 'uart.data', { wallMs: 1_700_000_000_123 }))).toBe(1_700_000_000_123);
    expect(wallMsOf({ ...rec(1, 'x'), ts: { ...rec(1, 'x').ts, wall_ns: 'garbage' } } as LoomEvent)).toBe(0);
    expect(cursorAt('7', 12n)).toBe('e7:seq:12');
    expect(fmtWall(new Date(2026, 0, 2, 3, 4, 5, 6).getTime())).toBe('2026-01-02 03:04:05.006');
    expect(exportFileName('dut', 'log', new Date(2026, 10, 16, 9, 5, 7).getTime())).toBe('dut-journal-20261116-090507.log');
    expect(exportFileName('dut', 'jsonl', new Date(2026, 10, 16, 9, 5, 7).getTime())).toBe('dut-journal-20261116-090507.jsonl');
    expect(rangeText({ kind: 'retained' })).toBe('보존 범위 전체');
    expect(rangeText({ kind: 'recent', minutes: 15 })).toBe('최근 15분');
    expect(rangeText({ kind: 'generation', generation: 3 })).toBe('연결 세대 3');
  });

  it('errCode / errDetail read NetError fields', () => {
    const e = new NetErr('E_SCAN_LIMIT', 'm', { code: 'E_SCAN_LIMIT', detail: { resume_from: 'e7:seq:5' } });
    expect(errCode(e)).toBe('E_SCAN_LIMIT');
    expect(errDetail(e)).toEqual({ resume_from: 'e7:seq:5' });
    expect(errCode(new Error('x'))).toBeNull();
    expect(errDetail(new NetErr('E_X', 'm', { code: 'E_X', a: 1 }))).toEqual({ code: 'E_X', a: 1 });
    expect(errDetail(new Error('x'))).toBeNull();
  });
});

describe('findFirstSeq', () => {
  it('binary-searches a monotone predicate within the probe cap', async () => {
    let n = 0;
    const r = await findFirstSeq(1n, 100n, async (s) => { n++; return s >= 37n; }, 64);
    expect(r).toEqual({ seq: 37n, probes: n, exhausted: false });
    expect(n).toBeLessThanOrEqual(8);
  });
  it('null when never true; lo when always true', async () => {
    expect((await findFirstSeq(1n, 50n, async () => false, 64)).seq).toBeNull();
    expect((await findFirstSeq(5n, 50n, async () => true, 64)).seq).toBe(5n);
  });
  it('reports exhaustion at the cap', async () => {
    const r = await findFirstSeq(1n, 1_000_000n, async (s) => s >= 777_777n, 3);
    expect(r.exhausted).toBe(true);
    expect(r.probes).toBe(3);
  });
});

// ---------------------------------------------------------------- gap tracker & writer

describe('GapTracker', () => {
  it('reports rx_offset jumps and record.gap; ignores TX and the first RX', () => {
    const g = new GapTracker();
    expect(g.observe(rx(1, 'hello', 100))).toBeNull();
    expect(g.observe(tx(2, 'ls\r'))).toBeNull();
    expect(g.observe(rx(3, 'abc', 105))).toBeNull();
    const jump = g.observe(rx(4, 'zz', 120));
    expect(jump).toMatchObject({ kind: 'rx_offset', atSeq: '4', bytes: 12 });
    const rg = g.observe(rec(5, 'record.gap', { payload: { lost_bytes: 64 } }));
    expect(rg).toMatchObject({ kind: 'record_gap', atSeq: '5' });
  });
  it('accepts rx_offset as a decimal string', () => {
    const g = new GapTracker();
    g.observe(rx(1, 'ab', 0));
    expect(g.observe(rec(2, 'uart.data', { dir: 'rx', payload: { bytes: { encoding: 'hex', data: hex('c') }, len: 1, rx_offset: '7' } }))).toMatchObject({ bytes: 5 });
  });
});

describe('ExportWriter (.log)', () => {
  it('assembles lines across chunks with the seq and time of the chunk that started them', () => {
    const w = new ExportWriter('log');
    w.record(rx(3, 'hel', 0));
    w.record(rx(4, 'lo\r\nwor', 3));
    w.record(rx(5, 'ld\n', 10));
    w.finish();
    expect(w.body().split('\n').filter(Boolean)).toEqual([
      `${fmtWall(T0 + 3000)} #3 RX  hello`,
      `${fmtWall(T0 + 4000)} #4 RX  world`,
    ]);
  });
  it('strips ANSI, applies backspace, drops other controls, keeps RX/TX lines apart', () => {
    const w = new ExportWriter('log');
    w.record(rx(1, '\x1b[31mred\x1b[0m ab\bc\x07\n', 0));
    w.record(tx(2, 'ls\r'));
    w.record(tx(3, '\n'));
    w.finish();
    const lines = w.body().split('\n').filter(Boolean);
    expect(lines[0]).toBe(`${fmtWall(T0 + 1000)} #1 RX  red ac`);
    expect(lines[1]).toBe(`${fmtWall(T0 + 2000)} #2 TX  ls`);
  });
  it('writes state/config/gap records as -- lines with their payload and flushes an unterminated tail', () => {
    const w = new ExportWriter('log');
    w.record(state(1, 'open'));
    w.record(rx(2, 'tail', 0));
    w.note('공백: RX 12 B 누락', T0 + 2500, '2');
    w.finish();
    const lines = w.body().split('\n').filter(Boolean);
    expect(lines[0]).toBe(`-- ${fmtWall(T0 + 1000)} #1 uart.state {"state":"open","path":"/dev/ttyUSB0"}`);
    expect(lines).toContain(`-- ${fmtWall(T0 + 2500)} #2 공백: RX 12 B 누락`);
    expect(lines[lines.length - 1]).toBe(`${fmtWall(T0 + 2000)} #2 RX  tail ⟨줄바꿈 없음⟩`);
  });
  it('stops at maxBytes and says so', () => {
    const w = new ExportWriter('log', 100);
    for (let i = 1; i <= 50; i++) w.record(rx(i, `line ${i}\n`, i * 10));
    expect(w.capped).toBe(true);
    expect(w.bytes).toBeLessThanOrEqual(100);
    expect(new TextEncoder().encode(w.body()).length).toBe(w.bytes);
  });
});

describe('ExportWriter (.jsonl)', () => {
  it('one object per record with canonical seq, epoch, source identity and exact bytes', () => {
    const w = new ExportWriter('jsonl');
    w.record(rx(9, 'hi\n', 40, { generation: 2 }));
    w.record(tx(10, 'ls\r'));
    w.record(state(11, 'closed', 2));
    w.finish();
    const rows = w.body().trim().split('\n').map((l) => JSON.parse(l) as Record<string, unknown>);
    expect(rows[0]).toMatchObject({
      type: 'data', epoch: '7', seq: '9', ms: T0 + 9000, dir: 'rx', origin: 'device', generation: 2, rx_offset: '40', len: 3,
      hex: hex('hi\n'), text: 'hi\n', producer_id: 'p-uart', producer_epoch: '3', source_seq: '90', request_id: null, client_id: null,
    });
    expect(rows[0]!.t).toBe(new Date(T0 + 9000).toISOString());
    expect(rows[1]).toMatchObject({ type: 'data', seq: '10', dir: 'tx', origin: 'agent', request_id: 'r-1', client_id: 'agent#1', rx_offset: null });
    expect(rows[2]).toMatchObject({ type: 'event', seq: '11', kind: 'uart.state', generation: 2, payload: { state: 'closed' } });
  });
});

// ---------------------------------------------------------------- runExport

const lines = (text: string) => text.split('\n');
const header = (text: string) => lines(text).filter((l) => l.startsWith('# '));

describe('runExport', () => {
  it('pages the retained range with peek and a fixed upper bound; complete header', async () => {
    const recs = [state(1, 'open'), rx(2, 'boot\n', 0), rx(3, 'ok\n', 5), tx(4, 'ls\r'), rx(5, 'a b\n', 8)];
    const j = fakeJournal(recs);
    const r = await runExport({ ...opts(j.query), limits: { pageRows: 2 } });
    expect(r.incomplete).toEqual([]);
    expect(r.epoch).toBe('7');
    expect(r.hwSeq).toBe('5');
    expect(r.startSeq).toBe('1');
    expect(r.oldestRetainedSeq).toBe('1');
    expect(r.first).toEqual({ seq: '1', wallMs: T0 + 1000 });
    expect(r.last).toEqual({ seq: '5', wallMs: T0 + 5000 });
    expect(r.events).toBe(5);
    expect(r.rxBytes).toBe(5 + 3 + 4);
    expect(r.txBytes).toBe(3);
    expect(r.pages).toBe(3);
    expect(r.generations).toEqual([1]);
    const reads = j.calls.filter((c) => c.max === 2);
    expect(reads.map((c) => c.since)).toEqual(['e7:seq:1', 'e7:seq:3', 'e7:seq:5']);
    expect(reads.every((c) => c.until === 'e7:seq:5')).toBe(true);
    const h = header(r.text);
    expect(h[0]).toContain('Loom UART 저널 내보내기 · dut');
    expect(h.some((l) => l.includes('provenance: daemon-journal-export'))).toBe(true);
    expect(h.some((l) => l.includes('이전 데몬 실행(epoch)의 기록은 포함하지 않음'))).toBe(true);
    expect(h.some((l) => l.includes('실제 범위: seq 1–5'))).toBe(true);
    expect(h.some((l) => l === '# 완전성: complete')).toBe(true);
    expect(r.text).toContain('#2 RX  boot');
    expect(r.text).toContain('#4 TX  ls');
    expect(r.fileName).toMatch(/^dut-journal-\d{8}-\d{6}\.log$/);
    expect(r.outBytes).toBe(new TextEncoder().encode(r.text.slice(r.text.indexOf('\n', r.text.lastIndexOf('# ')) + 1)).length);
  });

  it('starts at the oldest retained seq (found by probing) without calling that a gap', async () => {
    const recs = Array.from({ length: 10 }, (_, i) => rx(i + 1, `l${i + 1}\n`, i * 3));
    const j = fakeJournal(recs, { oldest: 4 });
    const r = await runExport(opts(j.query));
    expect(r.oldestRetainedSeq).toBe('4');
    expect(r.startSeq).toBe('4');
    expect(r.first?.seq).toBe('4');
    expect(r.incomplete).toEqual([]);
    expect(r.probes).toBeGreaterThan(0);
    expect(r.probes).toBeLessThanOrEqual(64);
  });

  it('recent N minutes starts at the first record at/after now − N min', async () => {
    const recs = Array.from({ length: 30 }, (_, i) => rx(i + 1, `m${i + 1}\n`, i * 4, { wallMs: T0 + i * MIN }));
    const j = fakeJournal(recs);
    const r = await runExport(opts(j.query, { range: { kind: 'recent', minutes: 10 }, nowMs: T0 + 29 * MIN }));
    expect(r.startSeq).toBe('20');
    expect(r.first?.seq).toBe('20');
    expect(r.incomplete).toEqual([]);
  });

  it('a recent range older than retention is partial with a retention gap', async () => {
    const recs = Array.from({ length: 30 }, (_, i) => rx(i + 1, `m${i + 1}\n`, i * 4, { wallMs: T0 + i * MIN }));
    const j = fakeJournal(recs, { oldest: 25 });
    const r = await runExport(opts(j.query, { range: { kind: 'recent', minutes: 10 }, nowMs: T0 + 29 * MIN }));
    expect(r.startSeq).toBe('25');
    expect(r.incomplete).toContain('gaps');
    expect(r.gaps[0]).toMatchObject({ kind: 'retention', atSeq: null });
    expect(header(r.text).some((l) => l.startsWith('# 완전성: partial: gaps'))).toBe(true);
  });

  it('a generation range exports that generation only', async () => {
    const recs = [state(1, 'open', 1), rx(2, 'g1\n', 0, { generation: 1 }), state(3, 'open', 2), rx(4, 'g2\n', 3, { generation: 2 }), rx(5, 'g2b\n', 6, { generation: 2 }), state(6, 'open', 3), rx(7, 'g3\n', 10, { generation: 3 })];
    const j = fakeJournal(recs);
    const r = await runExport(opts(j.query, { range: { kind: 'generation', generation: 2 } }));
    expect(r.first?.seq).toBe('3');
    expect(r.last?.seq).toBe('5');
    expect(r.generations).toEqual([2]);
    expect(r.text).not.toContain('g3');
    expect(r.incomplete).toEqual([]);
  });

  it('an rx_offset jump is a gap: partial, listed in the header and as a note line', async () => {
    const j = fakeJournal([rx(1, 'abc\n', 0), rx(2, 'def\n', 100)]);
    const r = await runExport(opts(j.query));
    expect(r.incomplete).toEqual(['gaps']);
    expect(r.gaps).toEqual([expect.objectContaining({ kind: 'rx_offset', atSeq: '2', bytes: 96 })]);
    expect(header(r.text).some((l) => l.startsWith('# 공백 1건'))).toBe(true);
    expect(r.text).toMatch(/^-- .* #2 .*96 B/m);
  });

  it('unreadable journal ranges are gaps', async () => {
    const j = fakeJournal([rx(1, 'a\n', 0), rx(2, 'b\n', 2)], { unreadableAt: { at: 1, from: 7, to: 9 } });
    const r = await runExport(opts(j.query));
    expect(r.gaps).toContainEqual(expect.objectContaining({ kind: 'unreadable' }));
    expect(r.incomplete).toContain('gaps');
  });

  it('E_SCAN_LIMIT keeps the partial events and resumes; nothing lost, still complete', async () => {
    const recs = [rx(1, 'a\n', 0), rx(2, 'b\n', 2), rx(3, 'c\n', 4)];
    const j = fakeJournal(recs, { scanLimitAt: 1 });
    const r = await runExport(opts(j.query));
    expect(r.events).toBe(3);
    expect(r.incomplete).toEqual([]);
    expect(r.text.match(/ RX {2}[abc]$/gm)).toHaveLength(3);
  });

  it('retention moving during the export is partial (retention_moved)', async () => {
    const recs = Array.from({ length: 8 }, (_, i) => rx(i + 1, `x${i}\n`, i * 3));
    const j = fakeJournal(recs, { cursorGapFrom: 3 });
    const r = await runExport({ ...opts(j.query), limits: { pageRows: 2 } });
    expect(r.incomplete).toContain('retention_moved');
    expect(r.events).toBeGreaterThan(0);
  });

  it('page and byte caps end the export truthfully', async () => {
    const recs = Array.from({ length: 10 }, (_, i) => rx(i + 1, `y${i}\n`, i * 3));
    const pages = await runExport({ ...opts(fakeJournal(recs).query), limits: { pageRows: 2, maxPages: 2 } });
    expect(pages.incomplete).toContain('cap_pages');
    expect(pages.events).toBe(4);
    const bytes = await runExport({ ...opts(fakeJournal(recs).query), limits: { maxBytes: 60 } });
    expect(bytes.incomplete).toContain('cap_bytes');
    expect(bytes.outBytes).toBeLessThanOrEqual(60);
  });

  it('cancel → cancelled', async () => {
    const ac = new AbortController();
    const recs = Array.from({ length: 10 }, (_, i) => rx(i + 1, `z${i}\n`, i * 3));
    const j = fakeJournal(recs);
    const r = await runExport({
      ...opts(j.query, { signal: ac.signal }),
      limits: { pageRows: 2 },
      onProgress: (p) => { if (p.phase === 'read' && p.pages >= 1) ac.abort(); },
    });
    expect(r.incomplete).toContain('cancelled');
  });

  it('a failing daemon gives query_failed with the message, never a silent empty file', async () => {
    const j = fakeJournal([rx(1, 'a\n', 0)], { failWith: new NetErr('E_DAEMON', 'hub stopped', { code: 'E_DAEMON' }) });
    const r = await runExport(opts(j.query));
    expect(r.incomplete).toEqual(['query_failed']);
    expect(r.error).toContain('hub stopped');
    expect(header(r.text).some((l) => l.startsWith('# 완전성: partial: query_failed'))).toBe(true);
  });

  it('nothing recorded → complete and says so', async () => {
    const r = await runExport(opts(fakeJournal([]).query));
    expect(r.incomplete).toEqual([]);
    expect(r.events).toBe(0);
    expect(header(r.text).some((l) => l.includes('실제 범위: 없음'))).toBe(true);
  });

  it('.jsonl starts with a header object', async () => {
    const r = await runExport(opts(fakeJournal([rx(1, 'a\n', 0)]).query, { format: 'jsonl' }));
    const first = JSON.parse(lines(r.text)[0]!) as Record<string, unknown>;
    expect(first).toMatchObject({ type: 'header', provenance: 'daemon-journal-export', channel: 'dut', epoch: '7', complete: true });
    expect(JSON.parse(lines(r.text)[1]!)).toMatchObject({ type: 'data', seq: '1' });
  });

  it('exportHeader / summaryText state completeness', async () => {
    const ok = await runExport(opts(fakeJournal([rx(1, 'a\n', 0)]).query));
    expect(exportHeader(ok)).toContain('# 완전성: complete');
    expect(summaryText(ok)).toContain('complete');
    const bad = await runExport(opts(fakeJournal([rx(1, 'a\n', 0), rx(2, 'b\n', 50)]).query));
    expect(summaryText(bad)).toContain('partial');
  });
});
