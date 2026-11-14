// logic.analyzer model — pure parsing / retention for the `logic` domain (CONTRACT.md §3, §4, §8).
//
// Two layers that are never mixed:
//   RAW      logic.edges  — sample-exact transitions from the byte stream (no interpretation)
//   DECODED  logic.annotation / logic.annotations — libsigrok PD output, an interpretation over [start,end]
//
// Sample axis is the only timing used for waveform/decode relations: t = sample / samplerate.
// Host `ts` is receive time (lag only). Masks are JSON numbers (u16), sample_end is exclusive,
// edge samples are absolute (§8). Anything not covered by a received chunk is UNKNOWN and is drawn
// as a gap, never bridged with the previous level.

export type CaptureStateName = 'starting' | 'running' | 'stopping' | 'stopped' | 'failed' | 'unknown';
export type LogicSource = 'sigrok-demo' | 'sigrok-device' | 'synthetic' | string;

export interface Counters {
  samples: number;
  edges: number;
  annotations: number;
  droppedEdges: number;
  droppedAnnotations: number;
  truncatedLines: number;
  unparsedLines: number;
  emitErrors: number;
}

export interface CaptureInfo {
  id: string;
  state: CaptureStateName;
  source: LogicSource | null;
  driver: string | null;
  samplerate: number | null;
  channels: string[];
  decoders: string[];
  maxDurationMs: number | null;
  maxSamples: number | null;
  reason: string | null;
  /** "acq exit 0 (sigint) · dec exit 0" — §9 exit:{acquisition,decoder} */
  exitText: string | null;
  stderrTail: string | null;
  error: string | null;
  counters: Counters | null;
  quality: string[];
  /** who started the capture: payload.requested_by (agent|user|script). The envelope origin is the
   * state REPORTER (device) and is never used as the starter. null = not reported → not shown. */
  requestedBy: string | null;
  /** seq of the capture_state event this came from (stale filter) */
  seq: bigint;
}

export interface EdgeChunk {
  captureId: string;
  seq: bigint;
  samplerate: number | null;
  start: number; // inclusive
  end: number; // exclusive
  initial: number;
  /** absolute sample of each transition */
  samples: Float64Array;
  /** state AFTER each transition */
  masks: Uint16Array;
  /** backend prefix-retained cap: edges after the last kept one were not sent */
  dropped: number;
  /** first malformed edge: from the last valid edge (exclusive) to chunk end the state is unknown */
  malformedFrom: number | null;
  malformed: number;
  hostMonoNs: bigint | null;
}

export interface Annotation {
  key: string;
  captureId: string;
  decoder: string;
  /** null = PD/sigrok-cli 0.7.2 gave none; never invented (§8) */
  cls: string | null;
  start: number;
  end: number;
  tStartNs: number | null;
  tEndNs: number | null;
  text: string;
  seq: bigint;
}

/** Unknown sample range: no raw data for it (edge cap, chunk discontinuity, stream loss, eviction). */
export interface UnknownRange {
  start: number;
  end: number;
  cause: 'edges_capped' | 'malformed' | 'chunk_gap' | 'stream_gap' | 'evicted';
  detail: string;
}

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : {});
const str = (v: unknown): string | null => (typeof v === 'string' && v !== '' ? v : null);
/** JSON number or decimal string → non-negative SAFE integer (sample index, count) */
export const num = (v: unknown): number | null => {
  const n = typeof v === 'number' ? v : typeof v === 'string' && /^\d{1,16}$/.test(v) ? Number(v) : NaN;
  return Number.isSafeInteger(n) && n >= 0 ? n : null;
};
/** samplerate: finite > 0 */
export const rateNum = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : num(v) || null);
/** mask: JSON number (contract §8). A "0b…" string from an early backend draft is tolerated. */
export const mask = (v: unknown): number | null => {
  if (typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 0xffff) return v;
  if (typeof v === 'string') {
    const m = /^0b([01]{1,16})$/.exec(v);
    if (m) return parseInt(m[1]!, 2);
  }
  return null;
};
const strList = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

const STATES: ReadonlySet<string> = new Set(['starting', 'running', 'stopping', 'stopped', 'failed']);

export function readCounters(v: unknown): Counters | null {
  const c = obj(v);
  if (!Object.keys(c).length) return null;
  const n = (k: string) => num(c[k]) ?? 0;
  return {
    samples: n('samples'),
    edges: n('edges'),
    annotations: n('annotations'),
    droppedEdges: n('dropped_edges'),
    droppedAnnotations: n('dropped_annotations'),
    truncatedLines: n('truncated_lines'),
    unparsedLines: n('unparsed_lines'),
    emitErrors: n('emit_errors'),
  };
}

function exitText(exit: Record<string, unknown>): string | null {
  const one = (label: string, v: unknown): string | null => {
    const e = obj(v);
    if (!Object.keys(e).length) return null;
    const code = typeof e.code === 'number' ? String(e.code) : '—';
    const sig = typeof e.signal === 'number' || typeof e.signal === 'string' ? ` signal ${e.signal}` : '';
    const how = str(e.stop_style) ? ` (${e.stop_style})` : '';
    return `${label} exit ${code}${sig}${how}`;
  };
  const parts = [one('수집', exit.acquisition), one('디코더', exit.decoder)].filter(Boolean);
  if (!parts.length && ('code' in exit || 'signal' in exit)) parts.push(one('프로세스', exit));
  return parts.length ? parts.join(' · ') : null;
}

/** logic.capture_state payload → CaptureInfo; fields missing in a later state keep the previous value. */
export function readCaptureState(p: Record<string, unknown>, channel: string | null, seq: bigint, prev: CaptureInfo | null): CaptureInfo | null {
  const id = str(p.capture_id) ?? channel ?? prev?.id ?? null;
  if (!id) return null;
  const raw = str(p.state);
  const state: CaptureStateName = raw && STATES.has(raw) ? (raw as CaptureStateName) : (prev?.state ?? 'unknown');
  const limits = obj(p.limits);
  const exit = obj(p.exit);
  const has = (k: string) => Object.prototype.hasOwnProperty.call(p, k);
  return {
    id,
    state,
    source: str(p.source) ?? prev?.source ?? null,
    driver: str(p.driver) ?? prev?.driver ?? null,
    samplerate: rateNum(p.samplerate) ?? prev?.samplerate ?? null,
    channels: has('channels') ? strList(p.channels) : (prev?.channels ?? []),
    decoders: has('decoders') ? strList(p.decoders) : (prev?.decoders ?? []),
    maxDurationMs: has('limits') ? num(limits.max_duration_ms) : (prev?.maxDurationMs ?? null),
    maxSamples: has('limits') ? num(limits.max_samples) : (prev?.maxSamples ?? null),
    reason: str(p.reason) ?? (state === 'stopped' || state === 'failed' ? (prev?.reason ?? null) : null),
    exitText: has('exit') ? exitText(exit) : (prev?.exitText ?? null),
    stderrTail: str(p.stderr_tail) ?? prev?.stderrTail ?? null,
    error: str(p.error) ?? str(p.message) ?? (state === 'failed' ? (prev?.error ?? null) : null),
    counters: readCounters(p.counters) ?? prev?.counters ?? null,
    quality: has('quality') ? strList(p.quality) : (prev?.quality ?? []),
    requestedBy: str(p.requested_by) ?? prev?.requestedBy ?? null,
    seq,
  };
}

/** logic.edges payload → chunk; null if structurally unusable (then nothing is drawn for it). */
export function readEdges(p: Record<string, unknown>, channel: string | null, seq: bigint, hostMonoNs: bigint | null): EdgeChunk | null {
  const captureId = str(p.capture_id) ?? channel;
  const start = num(p.sample_start);
  const end = num(p.sample_end);
  const initial = mask(p.initial);
  if (!captureId || start === null || end === null || end < start || initial === null) return null;
  const list = Array.isArray(p.edges) ? p.edges : [];
  const samples: number[] = [];
  const masks: number[] = [];
  let last = start - 1;
  let malformedFrom: number | null = null;
  let malformed = 0;
  for (const e of list) {
    const s = Array.isArray(e) ? num(e[0]) : null;
    const m = Array.isArray(e) ? mask(e[1]) : null;
    // sample_end is exclusive; edges must be ordered and inside the chunk. A bad edge makes everything
    // after the last valid edge unknown (its position/state are unknown) — never skip it and keep drawing.
    if (s === null || m === null || s < start || s >= end || s < last) {
      malformed = list.length - samples.length;
      malformedFrom = last + 1;
      break;
    }
    samples.push(s);
    masks.push(m);
    last = s;
  }
  return {
    captureId,
    seq,
    samplerate: rateNum(p.samplerate),
    start,
    end,
    initial,
    samples: Float64Array.from(samples),
    masks: Uint16Array.from(masks),
    dropped: num(p.dropped) ?? 0,
    malformedFrom,
    malformed,
    hostMonoNs,
  };
}

function readOneAnnotation(a: Record<string, unknown>, captureId: string, seq: bigint, samplerate: number | null): Annotation | null {
  const start = num(a.start_sample);
  const end = num(a.end_sample);
  const text = typeof a.text === 'string' ? a.text : null;
  if (start === null || end === null || text === null || end < start) return null;
  // derive both from the sample clock when a rate is known, so tEnd ≥ tStart always holds
  const tns = (k: string, s: number) => (samplerate ? (s * 1e9) / samplerate : num(a[k]));
  return {
    // identity = decoder + sample span + text: the same annotation from snapshot, backlog and live is listed once
    key: `${str(a.decoder) ?? '?'}:${start}:${end}:${text}`,
    captureId,
    decoder: str(a.decoder) ?? '?',
    cls: str(a.class),
    start,
    end,
    tStartNs: tns('t_start_ns', start),
    tEndNs: tns('t_end_ns', end),
    text,
    seq,
  };
}

/** logic.annotation (one) and logic.annotations (bounded batch, §8) → rows + reported drop count. */
export interface AnnotationRead {
  captureId: string | null;
  rows: Annotation[];
  /** decoder lines sigrok-cli printed but backend could not parse (class "_raw", no samples) — kept as text */
  rawLines: string[];
  dropped: number;
  /** rows with invalid samples (negative / unsafe / end < start) — not drawn, counted */
  rejected: number;
}

export function readAnnotations(kind: string, p: Record<string, unknown>, channel: string | null, seq: bigint, samplerate: number | null): AnnotationRead {
  const captureId = str(p.capture_id) ?? channel;
  const out: AnnotationRead = { captureId, rows: [], rawLines: [], dropped: num(p.dropped) ?? 0, rejected: 0 };
  if (!captureId) return out;
  const rate = rateNum(p.samplerate) ?? samplerate;
  const list = kind === 'logic.annotation' ? [p] : Array.isArray(p.annotations) ? p.annotations : [];
  for (const x of list) {
    const r = obj(x);
    if (r.class === '_raw' && (r.start_sample === null || r.start_sample === undefined)) {
      if (typeof r.text === 'string') out.rawLines.push(`${str(r.decoder) ?? '?'}: ${r.text}`);
      continue;
    }
    const a = readOneAnnotation(r, captureId, seq, rate);
    if (a) out.rows.push(a);
    else out.rejected++;
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Per-capture retention

export const MAX_EDGES_PER_CAPTURE = 400_000; // ≈ 4.8 MB (Float64 + Uint16)
export const MAX_ANNOTATIONS_PER_CAPTURE = 20_000;
export const MAX_CAPTURES = 6;
export const MAX_CHUNKS_PER_CAPTURE = 4_096;

export class CaptureData {
  info: CaptureInfo | null = null;
  chunks: EdgeChunk[] = [];
  annotations: Annotation[] = [];
  unknown: UnknownRange[] = [];
  edgeCount = 0;
  /** first sample still retained (older evicted → shown as 보관 범위 밖) */
  evictedTo = 0;
  evictedAnnotations = 0;
  /** loss reported by the stream: edges dropped by the worker cap (sum of chunk.dropped) */
  droppedEdges = 0;
  droppedAnnotations = 0;
  rejectedAnnotations = 0;
  rawLines: string[] = [];
  rawLinesTotal = 0;
  lastHostMonoNs: bigint | null = null;
  lastChunkSeq = -1n;
  private seenChunk = new Set<string>();
  private seenAnn = new Set<string>();
  /** bumps on every change (renderer cache key) */
  version = 0;
  /** samplerate seen on edges (for captures whose state was not received yet) */
  edgeRate: number | null = null;

  constructor(readonly id: string) {}

  get samplerate(): number | null {
    return this.info?.samplerate ?? this.edgeRate;
  }
  /** first and last sample covered by retained raw data */
  get range(): { start: number; end: number } | null {
    if (!this.chunks.length) return null;
    return { start: this.chunks[0]!.start, end: this.chunks[this.chunks.length - 1]!.end };
  }
  /** highest sample known (raw or decoded) */
  get headSample(): number {
    const r = this.range?.end ?? 0;
    const a = this.annotations.length ? this.annotations[this.annotations.length - 1]!.end : 0;
    return Math.max(r, a, this.info?.counters?.samples ?? 0);
  }
  /** highest channel bit observed in masks (for lanes when capture_state lacks channel names) */
  maxBit = -1;

  /** chunks refused: partial overlap with retained coverage, or already-evicted range */
  rejectedChunks = 0;
  malformedEdges = 0;
  private streamGapPending = false;

  addChunk(c: EdgeChunk): boolean {
    // identity = sample span (snapshot copy, backlog and live of the same chunk carry the same span)
    const k = `${c.start}-${c.end}`;
    if (this.seenChunk.has(k)) return false;
    // already-evicted coverage (e.g. snapshot re-applied after eviction) is not resurrected
    if (c.end <= this.evictedTo && this.evictedTo > 0) return false;
    let i = this.chunks.length;
    while (i > 0 && this.chunks[i - 1]!.start > c.start) i--;
    const prev = this.chunks[i - 1];
    const next = this.chunks[i];
    // overlapping coverage would give two "truths" for the same samples and break the ordered-end lookup
    if ((prev && prev.end > c.start) || (next && c.end > next.start)) {
      this.rejectedChunks++;
      this.version++;
      return false;
    }
    this.seenChunk.add(k);
    if (c.samplerate) this.edgeRate = c.samplerate;
    if (c.hostMonoNs !== null) this.lastHostMonoNs = c.hostMonoNs;
    if (c.seq > this.lastChunkSeq) this.lastChunkSeq = c.seq;
    let bits = c.initial;
    for (const m of c.masks) bits |= m;
    if (bits) this.maxBit = Math.max(this.maxBit, 31 - Math.clz32(bits));
    this.chunks.splice(i, 0, c);
    this.edgeCount += c.samples.length;
    const lastKept = c.samples.length ? c.samples[c.samples.length - 1]! + 1 : c.start;
    if (c.malformedFrom !== null) {
      this.malformedEdges += c.malformed;
      this.addUnknown({ start: Math.min(c.malformedFrom, c.end), end: c.end, cause: 'malformed', detail: `형식 오류 전이 ${c.malformed}개 — 이후 상태 알 수 없음` });
    } else if (c.dropped > 0) {
      this.droppedEdges += c.dropped;
      // prefix-retained cap: after the last kept edge the true state is unknown up to the chunk end
      this.addUnknown({ start: Math.min(lastKept, c.end), end: c.end, cause: 'edges_capped', detail: `전이 ${c.dropped.toLocaleString('ko-KR')}개 생략(이벤트당 상한)` });
    }
    // incremental gap bookkeeping (no full rescan): the new chunk fills or creates gaps only next to itself
    this.unknown = this.unknown.filter((u) => !((u.cause === 'chunk_gap' || u.cause === 'stream_gap') && u.start < c.end && u.end > c.start));
    const gap = (a0: number, b0: number) => {
      if (b0 <= a0) return;
      const cause = this.streamGapPending ? 'stream_gap' : 'chunk_gap';
      this.addUnknown({ start: a0, end: b0, cause, detail: cause === 'stream_gap' ? '스트림 손실 — 이 구간 이벤트 미수신' : `원시 데이터 없음 (${(b0 - a0).toLocaleString('ko-KR')} 샘플)` });
    };
    if (prev) gap(prev.end, c.start);
    if (next) gap(c.end, next.start);
    if (prev) this.streamGapPending = false;
    this.evict();
    this.version++;
    return true;
  }

  private addUnknown(u: UnknownRange): void {
    if (u.end <= u.start) return;
    let i = this.unknown.length;
    while (i > 0 && this.unknown[i - 1]!.start > u.start) i--;
    this.unknown.splice(i, 0, u);
  }

  /** a stream loss (seq hole) whose content is unknown: the following data gap stays unknown */
  noteStreamGap(): void {
    this.streamGapPending = true;
  }

  private evict(): void {
    // bounded by edges AND chunk count (idle pins send edge-free chunks forever)
    while ((this.edgeCount > MAX_EDGES_PER_CAPTURE || this.chunks.length > MAX_CHUNKS_PER_CAPTURE) && this.chunks.length > 1) {
      const c = this.chunks.shift()!;
      this.edgeCount -= c.samples.length;
      this.evictedTo = c.end;
      this.seenChunk.delete(`${c.start}-${c.end}`);
    }
    if (this.evictedTo > 0 && this.unknown.length && this.unknown[0]!.end <= this.evictedTo) {
      this.unknown = this.unknown.filter((u) => u.end > this.evictedTo);
    }
  }

  addRawLines(lines: string[], rejected: number): void {
    if (!lines.length && !rejected) return;
    this.rawLinesTotal += lines.length;
    this.rejectedAnnotations += rejected;
    this.rawLines.push(...lines);
    if (this.rawLines.length > 200) this.rawLines.splice(0, this.rawLines.length - 200);
    this.version++;
  }

  addAnnotations(rows: Annotation[], dropped: number): number {
    let added = 0;
    for (const a of rows) {
      if (this.seenAnn.has(a.key)) continue;
      this.seenAnn.add(a.key);
      // ordered by start sample (stable for equal starts)
      let i = this.annotations.length;
      while (i > 0 && this.annotations[i - 1]!.start > a.start) i--;
      this.annotations.splice(i, 0, a);
      added++;
    }
    this.droppedAnnotations += dropped;
    while (this.annotations.length > MAX_ANNOTATIONS_PER_CAPTURE) {
      const a = this.annotations.shift()!;
      this.seenAnn.delete(a.key);
      this.evictedAnnotations++;
    }
    if (added || dropped) {
      this.version++;
      this.maxEndVersion = -1;
    }
    return added;
  }

  /** index of first chunk whose end > sample */
  chunkAt(sample: number): number {
    let lo = 0;
    let hi = this.chunks.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.chunks[mid]!.end <= sample) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** state bitmask at `sample`, or null when that sample is unknown (no chunk / capped / evicted) */
  stateAt(sample: number): number | null {
    if (sample < this.evictedTo) return null;
    if (this.unknown.some((u) => sample >= u.start && sample < u.end)) return null;
    const c = this.chunks[this.chunkAt(sample)];
    if (!c || sample < c.start) return null;
    let st = c.initial;
    for (let i = 0; i < c.samples.length && c.samples[i]! <= sample; i++) st = c.masks[i]!;
    return st;
  }

  /** prefix max of effective end (annotations are start-ordered; nested/long frames break end order) */
  private maxEnd: Float64Array = new Float64Array(0);
  private maxEndVersion = -1;

  /** first index whose prefix-max end > sample: every annotation overlapping `sample` or later is at ≥ this index */
  annotationAt(sample: number): number {
    if (this.maxEndVersion !== this.version) {
      this.maxEnd = new Float64Array(this.annotations.length);
      let m = -1;
      this.annotations.forEach((a, i) => {
        m = Math.max(m, annEnd(a));
        this.maxEnd[i] = m;
      });
      this.maxEndVersion = this.version;
    }
    let lo = 0;
    let hi = this.maxEnd.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.maxEnd[mid]! <= sample) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }
}

/** zero-length annotations still occupy their start sample */
export const annEnd = (a: Annotation): number => Math.max(a.end, a.start + 1);

/** Stale-seq guard: a capture_state never replaces a newer one, whatever path it arrived on. */
export class CaptureBook {
  readonly captures = new Map<string, CaptureData>();
  /** id of the capture with the newest capture_state seq (auto-follow target) */
  newestId: string | null = null;
  private newestSeq = -1n;

  get(id: string): CaptureData {
    let c = this.captures.get(id);
    if (!c) {
      c = new CaptureData(id);
      this.captures.set(id, c);
      this.trim();
    }
    return c;
  }

  /** returns true if applied (not stale) */
  applyState(p: Record<string, unknown>, channel: string | null, seq: bigint): boolean {
    const id = str(p.capture_id) ?? channel;
    if (!id) return false;
    const c = this.get(id);
    if (c.info && seq <= c.info.seq) return false;
    const next = readCaptureState(p, channel, seq, c.info);
    if (!next) return false;
    c.info = next;
    c.version++;
    // `>=`: snapshot items share one seq (the high-water H) and arrive oldest first, so the later
    // one is the newer capture. Live records have unique seqs, so this changes nothing there.
    if (seq >= this.newestSeq) {
      this.newestSeq = seq;
      this.newestId = id;
    }
    return true;
  }

  /** order for the capture picker: newest state first */
  list(): CaptureData[] {
    // Equal seqs (one snapshot) keep snapshot order reversed: later inserted = newer, first.
    return [...this.captures.values()]
      .map((c, i) => ({ c, i }))
      .sort((a, b) => {
        const sa = a.c.info?.seq ?? a.c.lastChunkSeq;
        const sb = b.c.info?.seq ?? b.c.lastChunkSeq;
        return sa === sb ? b.i - a.i : sa > sb ? -1 : 1;
      })
      .map((x) => x.c);
  }

  private trim(): void {
    if (this.captures.size <= MAX_CAPTURES) return;
    // evict the oldest NON-running capture (running ones are never dropped)
    const victims = this.list().reverse().filter((c) => c.info?.state !== 'running' && c.info?.state !== 'starting' && c.id !== this.newestId);
    const v = victims[0];
    if (v) this.captures.delete(v.id);
  }
}

// ---------------------------------------------------------------------------------------------
// Snapshot (late join). Gateway item = {domain:"logic", channel?, kind:"logic.snapshot", state:<reducer value>}.
// Proposed reducer value (UI-STATUS.md §계약 제안 1); every layer is optional and tolerant:
//   {channel: capture_id, capture: <capture_state payload>, capture_seq: "<u64>",
//    annotations: [<annotation>…≤200], edges: [<logic.edges payload + seq>…≤64 KiB]}
// The flat form (state = the capture_state payload itself, with `seq`) is accepted too.
// core.channels entries ({domain, channel, state:"running", seq, kind}) give only the state word.

export interface SnapshotCapture {
  id: string;
  state: Record<string, unknown> | null;
  annotations: Record<string, unknown>[];
  edges: Record<string, unknown>[];
}

/** CONTRACT §9 reducer snapshot: state = {captures:[capture_state payload…], annotations:[row+capture_id…], edges:[edges payload…]}.
 * Snapshot payloads carry no record seq — the caller applies them at the snapshot high-water (throughSeq), so any later
 * control event (seq > H) wins and any older one (seq ≤ H) is stale. */
export function readSnapshotItem(item: unknown): SnapshotCapture[] {
  const it = obj(item);
  if (it.domain !== 'logic') return [];
  const s = obj(it.state);
  const byId = new Map<string, SnapshotCapture>();
  const at = (id: string) => {
    let c = byId.get(id);
    if (!c) byId.set(id, (c = { id, state: null, annotations: [], edges: [] }));
    return c;
  };
  for (const x of Array.isArray(s.captures) ? s.captures : []) {
    const p = obj(x);
    const id = str(p.capture_id);
    if (id) at(id).state = p;
  }
  for (const x of Array.isArray(s.annotations) ? s.annotations : []) {
    const p = obj(x);
    const id = str(p.capture_id);
    if (id) at(id).annotations.push(p);
  }
  for (const x of Array.isArray(s.edges) ? s.edges : []) {
    const p = obj(x);
    const id = str(p.capture_id);
    if (id) at(id).edges.push(p);
  }
  return [...byId.values()];
}

// ---------------------------------------------------------------------------------------------
// formatting

/** sample → time string on the device sample clock */
export function fmtSampleTime(sample: number, rate: number | null, digits = 3): string {
  if (!rate) return `#${Math.round(sample).toLocaleString('ko-KR')}`;
  return fmtSeconds(sample / rate, digits);
}

export function fmtSeconds(s: number, digits = 3): string {
  const a = Math.abs(s);
  const fix = (v: number) => {
    const t = v.toFixed(digits);
    return t.includes('.') ? t.replace(/0+$/, '').replace(/\.$/, '') : t;
  };
  if (a === 0) return '0 s';
  if (a >= 1) return `${fix(s)} s`;
  if (a >= 1e-3) return `${fix(s * 1e3)} ms`;
  if (a >= 1e-6) return `${fix(s * 1e6)} µs`;
  return `${fix(s * 1e9)} ns`;
}

export function fmtRate(hz: number | null): string {
  if (!hz) return '? Hz';
  if (hz >= 1e6) return `${+(hz / 1e6).toFixed(3)} MHz`;
  if (hz >= 1e3) return `${+(hz / 1e3).toFixed(3)} kHz`;
  return `${hz} Hz`;
}

/** "1M" "250k" "1000000" → Hz */
export function parseRate(s: string): number | null {
  const m = /^\s*(\d+(?:\.\d+)?)\s*([kKmM]?)(?:hz|Hz)?\s*$/.exec(s);
  if (!m) return null;
  const v = Number(m[1]) * (m[2] === 'k' || m[2] === 'K' ? 1e3 : m[2] === 'm' || m[2] === 'M' ? 1e6 : 1);
  return Number.isFinite(v) && v > 0 ? Math.round(v) : null;
}

/** nice tick step (in samples) for ~`target` px spacing */
export function tickStep(samplesPerPx: number, rate: number | null, targetPx = 90): number {
  const raw = samplesPerPx * targetPx;
  const unit = rate ? rate * 1e-9 : 1; // samples per ns (time ticks) or 1 (sample ticks)
  const v = raw / unit;
  const p = Math.pow(10, Math.floor(Math.log10(Math.max(v, 1e-12))));
  const f = v / p;
  const nice = f < 1.5 ? 1 : f < 3.5 ? 2 : f < 7.5 ? 5 : 10;
  return Math.max(nice * p * unit, rate ? 1e-9 : 1);
}

export const SOURCE_BADGE: Record<string, { text: string; cls: string; title: string }> = {
  'sigrok-demo': { text: 'sigrok demo 드라이버', cls: 'lg-src-demo', title: 'libsigrok demo 드라이버의 실제 acquisition — 패턴 파형이며 디코드 내용은 의미 없음' },
  'sigrok-device': { text: '실제 장비', cls: 'lg-src-device', title: 'sigrok-cli가 연 실제 logic analyzer 장비' },
  synthetic: { text: 'SYNTHETIC 생성 파형', cls: 'lg-src-synth', title: 'Loom이 생성한 이상 파형. 디코딩은 실제 libsigrok PD' },
};

export const STATE_LABEL: Record<CaptureStateName, string> = {
  starting: '시작 중',
  running: '캡처 중',
  stopping: '정지 중',
  stopped: '정지됨',
  failed: '실패',
  unknown: '상태 미수신',
};

export const REASON_LABEL: Record<string, string> = {
  user_stop: '사용자 정지',
  duration: '지정 시간 도달',
  samples: '샘플 상한 도달',
  eof: '입력 종료(EOF)',
  stop_latch: 'STOP 래치',
  error: '오류',
};

// ---------------------------------------------------------------------------------------------
// start config (form ⇄ RPC params). Param names follow the CLI flags (CONTRACT §5); see
// UI-STATUS.md §계약 제안 2 — backend may rename; the mapping lives only here.

export type DecoderKind = 'none' | 'uart' | 'spi';
export interface StartConfig {
  source: 'sigrok-demo' | 'synthetic' | 'sigrok-device';
  driver: string;
  conn: string;
  samplerate: number;
  pattern: string;
  /** synthetic text with \\r \\n escapes as typed in the form ({n} = frame counter) */
  text: string;
  framePeriodMs: number;
  durationS: number;
  decoder: DecoderKind;
  uartBaud: number;
  uartRx: number;
  spiClk: number;
  spiMosi: number;
  spiMiso: number | null;
  spiCs: number | null;
}

export const DEFAULT_CONFIG: StartConfig = {
  source: 'synthetic',
  driver: '',
  conn: '',
  samplerate: 1_000_000,
  pattern: 'graycode',
  text: 'LOOM {n}\\r\\n',
  framePeriodMs: 100,
  durationS: 10,
  decoder: 'uart',
  uartBaud: 115_200,
  uartRx: 0,
  spiClk: 0,
  spiMosi: 1,
  spiMiso: null,
  spiCs: 2,
};

/** acquisition is always D0..D7, 1 byte/sample (CONTRACT §8 packing). Lane visibility is a UI choice only. */
export const MAX_CHANNELS = 8;
export const CAPTURE_CHANNELS: readonly string[] = Array.from({ length: MAX_CHANNELS }, (_, i) => `D${i}`);
/** first slice: only fx2lafw-class 8-bit devices. Others are listed as unsupported / not verified on hardware. */
export const SUPPORTED_DEVICE_DRIVERS: readonly string[] = ['fx2lafw'];
export const UNSUPPORTED_DEVICE_NOTES: readonly { driver: string; note: string }[] = [
  { driver: 'saleae-logic16', note: '16ch — 첫 slice 미지원 (8-bit 고정), 실물 미검증' },
  { driver: 'saleae-logic-pro', note: 'Logic Pro 16 — 첫 slice 미지원, 실물 미검증' },
];
export const MAX_DURATION_S = 3600;
/** backend bounds (CONTRACT §9) */
export const MIN_SAMPLERATE = 1_000;
export const MAX_SAMPLERATE = 10_000_000;
export const DEMO_PATTERN_SAMPLERATE = 200_000;

/** "\\r\\n" typed in the form → CR LF */
export function unescapeText(t: string): string {
  return t.replace(/\\(r|n|t|\\)/g, (_, c: string) => (c === 'r' ? '\r' : c === 'n' ? '\n' : c === 't' ? '\t' : '\\'));
}

export function decoderSpec(c: StartConfig): string | null {
  if (c.decoder === 'uart') return `uart:baudrate=${c.uartBaud}:rx=D${c.uartRx}`;
  if (c.decoder === 'spi') {
    const parts = [`spi:clk=D${c.spiClk}`, `mosi=D${c.spiMosi}`];
    if (c.spiMiso !== null) parts.push(`miso=D${c.spiMiso}`);
    if (c.spiCs !== null) parts.push(`cs=D${c.spiCs}`);
    return parts.join(':');
  }
  return null;
}

/** validation: problems shown next to the Start button; [] = startable */
export function validateConfig(c: StartConfig): string[] {
  const p: string[] = [];
  if (c.source === 'sigrok-device') {
    if (!c.driver) p.push('실제 장비는 드라이버를 명시해야 합니다 (fx2lafw)');
    else if (!SUPPORTED_DEVICE_DRIVERS.includes(c.driver)) p.push(`드라이버 ${c.driver}: 첫 slice 미지원 (fx2lafw 8-bit 계열만, 실물 미검증)`);
  }
  const rate = effectiveRate(c);
  if (!(rate >= MIN_SAMPLERATE && rate <= MAX_SAMPLERATE)) p.push('샘플레이트 1 kHz – 10 MHz');
  if (!(c.durationS >= 0.1 && c.durationS <= MAX_DURATION_S)) p.push(`캡처 시간 0.1–${MAX_DURATION_S} s`);
  if (c.source === 'synthetic') {
    if (new TextEncoder().encode(unescapeText(c.text)).length > 256) p.push('합성 텍스트 ≤ 256 B');
    if (!(Number.isInteger(c.framePeriodMs) && c.framePeriodMs >= 1 && c.framePeriodMs <= 10_000)) p.push('프레임 주기 1–10000 ms');
    if (c.decoder === 'none') p.push('합성 소스는 UART 또는 SPI 디코더를 선택해야 파형이 생성됩니다');
  }
  const inRange = (ch: number | null) => ch === null || (Number.isInteger(ch) && ch >= 0 && ch < MAX_CHANNELS);
  if (c.decoder === 'uart') {
    if (!inRange(c.uartRx)) p.push('UART RX 채널이 캡처 채널 밖입니다');
    if (!(c.uartBaud > 0)) p.push('baudrate가 올바르지 않습니다');
    else if (rate < c.uartBaud * 4) p.push('샘플레이트가 baudrate의 4배 미만 — 디코드 불가');
  }
  if (c.decoder === 'spi') {
    const pins = [c.spiClk, c.spiMosi, c.spiMiso, c.spiCs].filter((x): x is number => x !== null);
    if (!pins.every(inRange)) p.push('SPI 핀이 캡처 채널 밖입니다');
    if (new Set(pins).size !== pins.length) p.push('SPI 핀이 겹칩니다');
  }
  return p;
}

/** demo patterns run at the libsigrok demo default only (backend rejects other rates with a pattern) */
export function effectiveRate(c: StartConfig): number {
  return c.source === 'sigrok-demo' && c.pattern ? DEMO_PATTERN_SAMPLERATE : c.samplerate;
}

/** `shown` = display channel names (acquisition is always D0..D7; this only selects which bits backend emits as edges) */
export function startParams(c: StartConfig, shown: readonly string[] = CAPTURE_CHANNELS): Record<string, unknown> {
  const dec = decoderSpec(c);
  const p: Record<string, unknown> = {
    source: c.source,
    samplerate: effectiveRate(c),
    channels: [...shown],
    decoders: dec ? [dec] : [],
    duration_ms: Math.round(c.durationS * 1000),
  };
  if (c.source === 'sigrok-device') {
    p.driver = c.driver;
    if (c.conn.trim()) p.conn = c.conn.trim();
  }
  if (c.source === 'sigrok-demo' && c.pattern) p.pattern = c.pattern;
  if (c.source === 'synthetic') {
    if (c.text) p.text = unescapeText(c.text);
    p.frame_period_ms = c.framePeriodMs;
  }
  return p;
}

/** equivalent `loom logic start` CLI line (shown so a user can hand the same capture to an agent) */
export function cliLine(c: StartConfig, shown: readonly string[] = CAPTURE_CHANNELS): string {
  const dec = decoderSpec(c);
  const r0 = effectiveRate(c);
  const rate = r0 % 1_000_000 === 0 ? `${r0 / 1_000_000}M` : r0 % 1000 === 0 ? `${r0 / 1000}k` : String(r0);
  const q = (s: string) => (/^[A-Za-z0-9_.:=,-]+$/.test(s) ? s : `"${s.replace(/(["\\$`])/g, '\\$1')}"`);
  const parts = ['loom logic start', `--source ${c.source}`];
  if (c.source === 'sigrok-device') parts.push(`--driver ${q(c.driver || '<driver>')}`, ...(c.conn.trim() ? [`--conn ${q(c.conn.trim())}`] : []));
  parts.push(`--rate ${rate}`, `--channels ${shown.join(',')}`);
  if (dec) parts.push(`--decode ${dec}`);
  parts.push(`--duration ${c.durationS}s`);
  if (c.source === 'sigrok-demo' && c.pattern) parts.push(`--pattern ${q(c.pattern)}`);
  if (c.source === 'synthetic' && c.text) parts.push(`--text ${q(c.text)}`, `--frame-period ${c.framePeriodMs}ms`);
  return parts.join(' ');
}

export interface Capability {
  enginePath: string | null;
  engineVersion: string | null;
  engineError: string | null;
  compiledDrivers: string[];
  compiledDecoders: string[];
  sources: string[];
  decoders: string[];
  deviceDrivers: string[];
  deviceVerified: boolean;
}

/** logic.status (CONTRACT §9). `compiled` = what the installed sigrok-cli contains, NOT what Loom supports. */
export function readCapability(v: unknown): Capability {
  const r = obj(obj(v).data ?? v);
  const engine = obj(r.engine);
  const compiled = obj(r.compiled);
  const impl = obj(r.implemented);
  return {
    enginePath: str(engine.path),
    engineVersion: str(engine.version),
    engineError: str(engine.error),
    compiledDrivers: strList(compiled.drivers),
    compiledDecoders: strList(compiled.decoders),
    sources: strList(impl.sources),
    decoders: strList(impl.decoders),
    deviceDrivers: strList(impl.device_drivers),
    deviceVerified: impl.device_verified === true,
  };
}
