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
  /** B4: published rows with role 'error' (counters.error_rows); undefined = older engine (not reported) */
  errorRows?: number;
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
  /** B3b: capture_state `archive` (whole-capture evidence). undefined/null = not reported (older engine). */
  archive?: ArchiveInfo | null;
  /** B4: capture_state `decoder_tracks` (which stack decodes which line). undefined/null = not reported. */
  decoderTracks?: DecoderTrack[] | null;
  /** B5: capture_state `trigger`. undefined/null = no trigger configured / not reported. */
  trigger?: TriggerInfo | null;
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
  /** B4: Loom-derived row role from the backend (`data` | `error`); null = not reported (older engine / raw).
   * Never inferred in the UI. */
  role?: 'data' | 'error' | null;
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
  cause: 'edges_capped' | 'malformed' | 'chunk_gap' | 'stream_gap' | 'evicted' | 'not_loaded';
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
    errorRows: Object.prototype.hasOwnProperty.call(c, 'error_rows') ? (num(c.error_rows) ?? 0) : undefined,
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
    archive: has('archive') ? readArchiveInfo(p.archive) : (prev?.archive ?? null),
    decoderTracks: has('decoder_tracks') ? readDecoderTracks(p.decoder_tracks) : (prev?.decoderTracks ?? null),
    trigger: has('trigger') ? readTriggerInfo(p.trigger) : (prev?.trigger ?? null),
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
    role: a.role === 'data' || a.role === 'error' ? a.role : null,
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

  /** B3b-M5. Drop all raw chunks and raw unknown ranges (annotations untouched) — archive view re-windowing.
   * After it: chunks [], edgeCount 0, evictedTo 0, unknown [], seenChunk cleared, droppedEdges/malformedEdges/
   * rejectedChunks 0, streamGapPending false; maxBit and edgeRate kept; version++. */
  clearChunks(): void {
    this.chunks = [];
    this.edgeCount = 0;
    this.evictedTo = 0;
    this.unknown = [];
    this.seenChunk.clear();
    this.droppedEdges = 0;
    this.malformedEdges = 0;
    this.rejectedChunks = 0;
    this.streamGapPending = false;
    this.version++;
  }

  /** B3b-M6. Archive view: `[start, end)` exists in the archive but was not loaded (page limit) → unknown with
   * cause 'not_loaded' (hatched, never drawn as a level). Ignored when end ≤ start. version++. */
  markNotLoaded(start: number, end: number, detail: string): void {
    if (end <= start) return;
    this.addUnknown({ start, end, cause: 'not_loaded', detail });
    this.version++;
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
  trigger_post: '트리거 후 지정 길이 도달',
};

// ---------------------------------------------------------------------------------------------
// start config (form ⇄ RPC params). Param names follow the CLI flags (CONTRACT §5); see
// UI-STATUS.md §계약 제안 2 — backend may rename; the mapping lives only here.

export type DecoderKind = 'none' | 'uart' | 'spi';
/** backend `uart` option allow-lists (crates/loom-logic/src/model.rs parse_uart) */
export type UartParity = 'none' | 'odd' | 'even';
export type UartStopBits = '1' | '1.5' | '2';
export type BitOrder = 'lsb-first' | 'msb-first';
/** annotated UART line of the single decoder stack (batch 1: one line per capture) */
/** B4: 'both' = RX and TX in one capture (two decoder stacks: uart-1 RX, uart-2 TX) */
export type UartLine = 'rx' | 'tx' | 'both';
export type CsPolarity = 'active-low' | 'active-high';
export type SpiAnnotate = 'mosi' | 'miso';
export const UART_PARITIES: readonly UartParity[] = ['none', 'odd', 'even'];
export const UART_STOP_BITS: readonly UartStopBits[] = ['1', '1.5', '2'];
export const BIT_ORDERS: readonly BitOrder[] = ['lsb-first', 'msb-first'];
export const UART_LINES: readonly UartLine[] = ['rx', 'tx', 'both'];
export type TriggerEdgeName = 'rising' | 'falling' | 'either';
export const TRIGGER_EDGES: readonly TriggerEdgeName[] = ['rising', 'falling', 'either'];
/** backend Trigger bounds (crates/loom-logic/src/model.rs) */
export const MAX_TRIGGER_PRE_MS = 1000;
export const GLITCH_EVERY = { min: 1, max: 1000 } as const;
export const CS_POLARITIES: readonly CsPolarity[] = ['active-low', 'active-high'];
export const SPI_ANNOTATES: readonly SpiAnnotate[] = ['mosi', 'miso'];
/** inclusive integer bounds (backend `num(k, v, lo, hi)`) */
export const UART_DATA_BITS = { min: 5, max: 9 } as const;
export const SPI_WORDSIZE = { min: 1, max: 32 } as const;

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
  /** RX data channel (bit index) — used when uartLine = 'rx' */
  uartRx: number;
  /** TX data channel (bit index) — used when uartLine = 'tx' */
  uartTx: number;
  uartLine: UartLine;
  uartDataBits: number;
  uartParity: UartParity;
  uartStopBits: UartStopBits;
  uartBitOrder: BitOrder;
  spiClk: number;
  /** null = no MOSI line (then MISO is required) */
  spiMosi: number | null;
  spiMiso: number | null;
  spiCs: number | null;
  spiCpol: 0 | 1;
  spiCpha: 0 | 1;
  spiWordsize: number;
  spiBitOrder: BitOrder;
  spiCsPolarity: CsPolarity;
  /** which data line becomes the decoded row stream */
  spiAnnotate: SpiAnnotate;
  /** B4: synthetic UART only — every Nth byte gets a framing error (0 = off) */
  glitchEvery: number;
  /** B5: software trigger */
  triggerOn: boolean;
  triggerCh: number;
  triggerEdge: TriggerEdgeName;
  triggerPreMs: number;
  triggerPostMs: number;
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
  uartTx: 1,
  uartLine: 'rx',
  uartDataBits: 8,
  uartParity: 'none',
  uartStopBits: '1',
  uartBitOrder: 'lsb-first',
  spiClk: 0,
  spiMosi: 1,
  spiMiso: null,
  spiCs: 2,
  spiCpol: 0,
  spiCpha: 0,
  spiWordsize: 8,
  spiBitOrder: 'msb-first',
  spiCsPolarity: 'active-low',
  spiAnnotate: 'mosi',
  glitchEvery: 0,
  triggerOn: false,
  triggerCh: 0,
  triggerEdge: 'falling',
  triggerPreMs: 10,
  triggerPostMs: 1000,
};

/** sanitizeConfig allow-lists: a saved value outside its list falls back to DEFAULT_CONFIG */
export const CONFIG_ENUMS: { readonly [K in keyof StartConfig]?: readonly unknown[] } = {
  source: ['synthetic', 'sigrok-demo', 'sigrok-device'],
  decoder: ['none', 'uart', 'spi'],
  uartLine: UART_LINES,
  triggerEdge: TRIGGER_EDGES,
  uartParity: UART_PARITIES,
  uartStopBits: UART_STOP_BITS,
  uartBitOrder: BIT_ORDERS,
  spiCpol: [0, 1],
  spiCpha: [0, 1],
  spiBitOrder: BIT_ORDERS,
  spiCsPolarity: CS_POLARITIES,
  spiAnnotate: SPI_ANNOTATES,
};
/** pin fields that may be null ("없음") */
export const NULLABLE_CONFIG_KEYS: readonly (keyof StartConfig)[] = ['spiMosi', 'spiMiso', 'spiCs'];

/** FUNCTIONS.md B1-M1: localStorage value (any shape, any age) → a valid StartConfig. Never returns DEFAULT_CONFIG itself. */
export function sanitizeConfig(saved: unknown): StartConfig {
  const out: Record<string, unknown> = { ...DEFAULT_CONFIG };
  if (typeof saved !== 'object' || saved === null || Array.isArray(saved)) return out as unknown as StartConfig;
  const src = saved as Record<string, unknown>;
  const defaults = DEFAULT_CONFIG as unknown as Record<string, unknown>;
  for (const k of Object.keys(DEFAULT_CONFIG) as (keyof StartConfig)[]) {
    const v = src[k];
    const d = defaults[k];
    if (v === undefined) continue;
    if (NULLABLE_CONFIG_KEYS.includes(k) && v === null) {
      out[k] = null;
      continue;
    }
    const allowed = CONFIG_ENUMS[k];
    if (allowed) {
      if (allowed.includes(v)) out[k] = v;
      continue;
    }
    if (d === null) {
      if (typeof v === 'number' && Number.isFinite(v)) out[k] = v;
      continue;
    }
    if (typeof v === typeof d && (typeof v !== 'number' || Number.isFinite(v))) out[k] = v;
  }
  return out as unknown as StartConfig;
}

// ---------------------------------------------------------------------------------------------
// Pre-start publish-load estimate (L1 recommendation 1). Budgets mirror crates/loom-logic/src/worker.rs.

/** worker ANNOT_RATE (rows/s); ANNOT_BURST 4000 */
export const PUBLISH_ANNOTATIONS_PER_S = 2000;
/** worker MAX_EDGES_PER_EVENT (1024) per EDGE_FLUSH (100 ms) */
export const PUBLISH_EDGES_PER_S = 10_240;
/** synthetic: what the number is based on; worst_case: continuous traffic upper bound; unknown: not estimable; none: no decoder */
export type LoadBasis = 'synthetic' | 'worst_case' | 'unknown' | 'none';
export interface LoadEstimate {
  /** decoded rows/s; null = cannot be estimated */
  annotationsPerS: number | null;
  annotationBasis: LoadBasis;
  /** upper bound of raw transitions/s over the displayed channels; null = cannot be estimated */
  edgesPerS: number | null;
  edgeBasis: LoadBasis;
  /** user-visible warnings (Korean), fixed order: annotations, edges, unknown-density note */
  warnings: string[];
}

/** FUNCTIONS.md B1-M3 */
export function estimatePublishLoad(c: StartConfig, shown: readonly string[] = CAPTURE_CHANNELS): LoadEstimate {
  const fmtN = (n: number) => Math.round(n).toLocaleString('ko-KR');
  const isShown = (bit: number | null) => bit !== null && shown.includes('D' + bit);
  const synthetic = c.source === 'synthetic';
  const B = new TextEncoder().encode(unescapeText(c.text).replaceAll('{n}', '0000')).length;
  const periodS = c.framePeriodMs / 1000;
  const rate = effectiveRate(c);
  const uartFrameBits = 1 + c.uartDataBits + (c.uartParity !== 'none' ? 1 : 0) + Number(c.uartStopBits);
  const uartBit = c.uartLine === 'rx' ? c.uartRx : c.uartTx;

  let annotationsPerS: number | null;
  let annotationBasis: LoadBasis;
  let edgesPerS: number | null;
  let edgeBasis: LoadBasis;

  if (c.decoder === 'none') {
    annotationsPerS = 0;
    annotationBasis = 'none';
    if (synthetic) {
      edgesPerS = 0;
      edgeBasis = 'none';
    } else {
      edgesPerS = null;
      edgeBasis = 'unknown';
    }
  } else if (c.decoder === 'uart') {
    const both = c.uartLine === 'both';
    const shownLines = both ? Number(isShown(c.uartRx)) + Number(isShown(c.uartTx)) : Number(isShown(uartBit));
    if (synthetic) {
      let perLine: number;
      if (both) {
        // the TX frame starts half a period after the RX frame (and after it ended)
        const f = (2 + 10 * B) / c.uartBaud;
        perLine = B / Math.max(periodS, Math.max(periodS / 2, f) + f);
      } else {
        perLine = B / Math.max(periodS, (2 + 10 * B) / c.uartBaud);
      }
      annotationsPerS = both ? 2 * perLine : perLine;
      annotationBasis = 'synthetic';
      edgesPerS = perLine * 10 * shownLines;
      edgeBasis = 'synthetic';
    } else {
      const perLine = c.uartBaud / uartFrameBits;
      annotationsPerS = both ? 2 * perLine : perLine;
      annotationBasis = 'worst_case';
      edgesPerS = perLine * uartFrameBits * shownLines;
      edgeBasis = 'worst_case';
    }
  } else if (synthetic) {
    const clk = Math.max(1, Math.floor(rate / 10));
    const h = Math.floor(rate / clk / 2);
    const frameS = (8 * h + 16 * h * B) / rate;
    const rows = B / Math.max(periodS, frameS);
    const framesPerS = 1 / Math.max(periodS, frameS);
    annotationsPerS = rows;
    annotationBasis = 'synthetic';
    edgesPerS =
      (isShown(c.spiClk) ? 16 * rows : 0) + (isShown(c.spiMosi) ? 8 * rows : 0) + (isShown(c.spiMiso) ? 8 * rows : 0) + (isShown(c.spiCs) ? 2 * framesPerS : 0);
    edgeBasis = 'synthetic';
  } else {
    annotationsPerS = null;
    annotationBasis = 'unknown';
    edgesPerS = null;
    edgeBasis = 'unknown';
  }

  const warnings: string[] = [];
  if (annotationsPerS !== null && annotationsPerS > PUBLISH_ANNOTATIONS_PER_S) {
    const worst = annotationBasis === 'worst_case';
    warnings.push(
      `예상 디코드 ${fmtN(annotationsPerS)}행/s${worst ? ' (연속 송신 시 최대)' : ''} > live 게시 상한 ${fmtN(PUBLISH_ANNOTATIONS_PER_S)}행/s — 초과분은 live 미리보기·저널에 실리지 않고 '디코드 손실'로 셉니다`,
    );
  }
  if (edgesPerS !== null && edgesPerS > PUBLISH_EDGES_PER_S) {
    const worst = edgeBasis === 'worst_case';
    warnings.push(
      `예상 전이 ${fmtN(edgesPerS)}개/s${worst ? ' (최대)' : ''} > live 게시 상한 ${fmtN(PUBLISH_EDGES_PER_S)}개/s — 초과 구간은 파형에 '데이터 없음'(빗금)으로 표시됩니다`,
    );
  }
  if (edgeBasis === 'unknown' && c.decoder === 'spi') {
    warnings.push('SPI 클록을 알 수 없어 게시량을 추정하지 못했습니다 — 1 MHz SPI는 전이 약 2,000,000개/s로 live 게시 상한을 크게 넘습니다');
  } else if (edgeBasis === 'unknown' && c.source === 'sigrok-device') {
    warnings.push(`실제 신호 밀도를 알 수 없어 전이 게시량을 추정하지 못했습니다 — 상한 ${fmtN(PUBLISH_EDGES_PER_S)}개/s 초과분은 빗금으로 표시됩니다`);
  }
  return { annotationsPerS, annotationBasis, edgesPerS, edgeBasis, warnings };
}

// ---------------------------------------------------------------------------------------------
// Held-vs-engine coverage (L5 recommendation 1): is what this panel holds the whole published record?

/** live = capture not terminal (engine counters are stale until the final state) */
export type CoverageState = 'live' | 'complete' | 'partial' | 'unknown';
export interface DecodeCoverage {
  /** rows the engine published (latest capture_state counters.annotations); null = not reported */
  published: number | null;
  /** rows the engine could not publish: max(counters.dropped_annotations, panel-seen batch drops) */
  engineDropped: number;
  /** rows this panel holds: annotations + unparsed raw lines + rejected rows */
  held: number;
  /** rows this panel evicted (MAX_ANNOTATIONS_PER_CAPTURE) */
  evicted: number;
  /** published rows this panel never received = max(0, published − held − evicted); 0 when published is null */
  notReceived: number;
  state: CoverageState;
}
export interface RawCoverage {
  /** samples the engine acquired (counters.samples); null = not reported */
  engineSamples: number | null;
  /** first sample of the first retained chunk; null = no chunk */
  heldStart: number | null;
  /** exclusive end of the last retained chunk; null = no chunk */
  heldEnd: number | null;
  /** UI-evicted samples [0, evictedTo) */
  evicted: number;
  /** samples before heldStart that were never received (not UI eviction) = max(0, heldStart − evictedTo) */
  notReceivedBefore: number;
  /** terminal only: max(0, engineSamples − (heldEnd ?? 0)); 0 while live or when engineSamples is null */
  notReceivedAfter: number;
  /** union length of all unknown ranges clipped to [heldStart, heldEnd) (no double counting) */
  unknownInside: number;
  state: CoverageState;
}

/** FUNCTIONS.md B1-M4 */
export function decodeCoverage(c: CaptureData): DecodeCoverage {
  const k = c.info?.counters ?? null;
  const st = c.info?.state ?? 'unknown';
  const published = k ? k.annotations : null;
  const engineDropped = Math.max(k?.droppedAnnotations ?? 0, c.droppedAnnotations);
  const held = c.annotations.length + c.rawLinesTotal + c.rejectedAnnotations;
  const evicted = c.evictedAnnotations;
  const notReceived = published === null ? 0 : Math.max(0, published - held - evicted);
  let state: CoverageState;
  if (!k || st === 'unknown') state = 'unknown';
  else if (st === 'starting' || st === 'running' || st === 'stopping') state = 'live';
  else state = notReceived === 0 && engineDropped === 0 && evicted === 0 ? 'complete' : 'partial';
  return { published, engineDropped, held, evicted, notReceived, state };
}

/** FUNCTIONS.md B1-M5 */
export function rawCoverage(c: CaptureData): RawCoverage {
  const k = c.info?.counters ?? null;
  const st = c.info?.state ?? 'unknown';
  const engineSamples = k ? k.samples : null;
  const r = c.range;
  const heldStart = r ? r.start : null;
  const heldEnd = r ? r.end : null;
  const evicted = c.evictedTo;
  const notReceivedBefore = heldStart === null ? 0 : Math.max(0, heldStart - c.evictedTo);
  const terminal = st === 'stopped' || st === 'failed';
  const notReceivedAfter = terminal && engineSamples !== null ? Math.max(0, engineSamples - (heldEnd ?? 0)) : 0;
  let unknownInside = 0;
  if (heldStart !== null && heldEnd !== null) {
    const clipped: [number, number][] = [];
    for (const u of c.unknown) {
      const s = Math.max(u.start, heldStart);
      const e = Math.min(u.end, heldEnd);
      if (e > s) clipped.push([s, e]);
    }
    clipped.sort((a, b) => a[0] - b[0]);
    let curS = 0;
    let curE = 0;
    let open = false;
    for (const [s, e] of clipped) {
      if (open && s <= curE) curE = Math.max(curE, e);
      else {
        if (open) unknownInside += curE - curS;
        curS = s;
        curE = e;
        open = true;
      }
    }
    if (open) unknownInside += curE - curS;
  }
  let state: CoverageState;
  if (!k || st === 'unknown') state = 'unknown';
  else if (!terminal) state = 'live';
  else state = notReceivedBefore === 0 && notReceivedAfter === 0 && unknownInside === 0 && evicted === 0 ? 'complete' : 'partial';
  return { engineSamples, heldStart, heldEnd, evicted, notReceivedBefore, notReceivedAfter, unknownInside, state };
}

/** FUNCTIONS.md B1-M7: suffix of the stopped/failed notice (replaces "— 기록은 이 패널에 남아 있습니다"); '' while live */
export function coverageNote(d: DecodeCoverage, r: RawCoverage): string {
  const fmtN = (n: number) => Math.round(n).toLocaleString('ko-KR');
  if (d.state === 'unknown' || r.state === 'unknown') return ' — 엔진 카운터 미수신: 이 패널 보유분이 전체인지 알 수 없습니다';
  if (d.state === 'live' || r.state === 'live') return '';
  if (d.state === 'complete' && r.state === 'complete') return ' — 이 패널이 엔진 게시분 전체를 보유합니다';
  const parts: string[] = [];
  if (d.notReceived > 0) parts.push(`디코드 ${fmtN(d.held)}/${fmtN(d.published ?? 0)}행 (미수신 ${fmtN(d.notReceived)})`);
  if (d.engineDropped > 0) parts.push(`게시 손실 ${fmtN(d.engineDropped)}행`);
  if (d.evicted > 0) parts.push(`디코드 보관 밖 ${fmtN(d.evicted)}행`);
  if (r.notReceivedBefore > 0) parts.push(`원시 앞부분 sample ${fmtN(r.notReceivedBefore)} 미수신`);
  if (r.notReceivedAfter > 0) parts.push(`원시 뒷부분 sample ${fmtN(r.notReceivedAfter)} 미수신`);
  if (r.unknownInside > 0) parts.push(`원시 데이터 없음 sample ${fmtN(r.unknownInside)}`);
  if (r.evicted > 0) parts.push(`원시 보관 밖 sample ${fmtN(r.evicted)}`);
  return ' — 이 패널은 일부만 보유합니다: ' + parts.join(' · ');
}

/** FUNCTIONS.md B1-M8: insertion after "N행" in the decode-table info line; '' unless published rows were not received */
export function annCoverageText(d: DecodeCoverage): string {
  const fmtN = (n: number) => Math.round(n).toLocaleString('ko-KR');
  if (d.published !== null && d.notReceived > 0) return ` · 패널 보유 ${fmtN(d.held)} / 엔진 게시 ${fmtN(d.published)} — ${fmtN(d.notReceived)}행 미수신`;
  return '';
}

/** FUNCTIONS.md B1-M6 (L8a): live-follow anchor = end of RECEIVED raw data, not the decode/engine head */
export function followHeadSample(c: CaptureData): number {
  return c.chunks.length ? c.range!.end : c.headSample;
}

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
  if (c.decoder === 'uart') {
    let spec =
      `uart:baudrate=${c.uartBaud}:` +
      (c.uartLine === 'rx' ? `rx=D${c.uartRx}` : c.uartLine === 'tx' ? `tx=D${c.uartTx}` : `rx=D${c.uartRx}:tx=D${c.uartTx}`);
    if (c.uartDataBits !== 8) spec += `:data_bits=${c.uartDataBits}`;
    if (c.uartParity !== 'none') spec += `:parity=${c.uartParity}`;
    if (c.uartStopBits !== '1') spec += `:stop_bits=${c.uartStopBits}`;
    if (c.uartBitOrder === 'msb-first') spec += ':bit_order=msb-first';
    return spec;
  }
  if (c.decoder === 'spi') {
    const parts = [`spi:clk=D${c.spiClk}`];
    if (c.spiMosi !== null) parts.push(`mosi=D${c.spiMosi}`);
    if (c.spiMiso !== null) parts.push(`miso=D${c.spiMiso}`);
    if (c.spiCs !== null) parts.push(`cs=D${c.spiCs}`);
    if (c.spiCpol === 1) parts.push('cpol=1');
    if (c.spiCpha === 1) parts.push('cpha=1');
    if (c.spiWordsize !== 8) parts.push(`wordsize=${c.spiWordsize}`);
    if (c.spiBitOrder === 'lsb-first') parts.push('bitorder=lsb-first');
    if (c.spiCsPolarity === 'active-high') parts.push('cs_polarity=active-high');
    if (c.spiAnnotate === 'miso' && c.spiMosi !== null) parts.push('annotate=miso');
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
    if (c.uartLine !== 'tx' && !inRange(c.uartRx)) p.push('UART RX 채널이 캡처 채널 밖입니다');
    if (c.uartLine !== 'rx' && !inRange(c.uartTx)) p.push('UART TX 채널이 캡처 채널 밖입니다');
    if (c.uartLine === 'both' && c.uartRx === c.uartTx) p.push('UART RX와 TX가 같은 채널입니다');
    if (!(c.uartBaud > 0)) p.push('baudrate가 올바르지 않습니다');
    else if (rate < c.uartBaud * 4) p.push('샘플레이트가 baudrate의 4배 미만 — 디코드 불가');
    if (!(Number.isInteger(c.uartDataBits) && c.uartDataBits >= 5 && c.uartDataBits <= 9)) p.push('UART 데이터 비트는 5–9 (정수)');
    if (c.source === 'synthetic' && (c.uartDataBits !== 8 || c.uartParity !== 'none' || c.uartStopBits !== '1' || c.uartBitOrder !== 'lsb-first')) {
      p.push('합성 UART는 8N1 LSB-first만 생성합니다 (실제 장비/데모에서는 사용 가능)');
    }
  }
  if (c.decoder === 'spi') {
    const pins = [c.spiClk, c.spiMosi, c.spiMiso, c.spiCs].filter((x): x is number => x !== null);
    if (!pins.every(inRange)) p.push('SPI 핀이 캡처 채널 밖입니다');
    if (new Set(pins).size !== pins.length) p.push('SPI 핀이 겹칩니다');
    if (c.spiMosi === null && c.spiMiso === null) p.push('SPI는 MOSI 또는 MISO가 필요합니다');
    if (c.spiAnnotate === 'miso' && c.spiMiso === null) p.push('MISO 표시를 고르려면 MISO 핀이 필요합니다');
    if (c.spiAnnotate === 'mosi' && c.spiMosi === null && c.spiMiso !== null) p.push('MOSI 표시를 고르려면 MOSI 핀이 필요합니다');
    if (!(Number.isInteger(c.spiWordsize) && c.spiWordsize >= 1 && c.spiWordsize <= 32)) p.push('SPI 워드 크기는 1–32 (정수)');
    if (c.source === 'synthetic' && c.spiWordsize !== 8) p.push('합성 SPI는 8-bit 워드만 생성합니다 (실제 장비/데모에서는 사용 가능)');
  }
  if (c.glitchEvery !== 0) {
    if (!(c.source === 'synthetic' && c.decoder === 'uart')) p.push('글리치는 합성 UART에서만 쓸 수 있습니다');
    else if (!(Number.isInteger(c.glitchEvery) && c.glitchEvery >= 1 && c.glitchEvery <= 1000)) p.push('글리치 주기는 1–1000 (정수, 0=끔)');
  }
  if (c.triggerOn) {
    if (!(Number.isInteger(c.triggerCh) && c.triggerCh >= 0 && c.triggerCh < MAX_CHANNELS)) p.push('트리거 채널이 캡처 채널 밖입니다');
    if (!(Number.isInteger(c.triggerPreMs) && c.triggerPreMs >= 0 && c.triggerPreMs <= 1000)) p.push('트리거 앞 구간 0–1000 ms (정수)');
    if (!(Number.isInteger(c.triggerPostMs) && c.triggerPostMs >= 1 && c.triggerPostMs <= Math.round(c.durationS * 1000))) {
      p.push('트리거 뒤 구간 1 ms – 캡처 시간 (정수 ms)');
    }
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
  if (c.source === 'synthetic' && c.decoder === 'uart' && c.glitchEvery > 0) p.glitch_every = c.glitchEvery;
  if (c.triggerOn) p.trigger = { channel: `D${c.triggerCh}`, edge: c.triggerEdge, pre_ms: c.triggerPreMs, post_ms: c.triggerPostMs };
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
  if (c.source === 'synthetic' && c.decoder === 'uart' && c.glitchEvery > 0) parts.push(`--glitch-every ${c.glitchEvery}`);
  if (c.triggerOn) parts.push(`--trigger D${c.triggerCh}:${c.triggerEdge} --pre ${c.triggerPreMs}ms --post ${c.triggerPostMs}ms`);
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

// ---------------------------------------------------------------------------------------------
// B3b — capture archive (whole-capture evidence kept by the daemon; FUNCTIONS.md §B3b)

export type ArchiveStateName = 'disabled' | 'pending' | 'recording' | 'complete' | 'partial' | 'failed' | 'unknown';
export interface ArchiveInfo {
  state: ArchiveStateName;
  /** daemon-side directory (absolute path string as reported) */
  dir: string | null;
  /** why disabled */
  reason: string | null;
  error: string | null;
  rawEndSample: number | null;
  rawBytes: number | null;
  annotationRows: number | null;
  rawTruncatedAt: number | null;
  annotationTruncatedAt: number | null;
  /** archives removed by rotation when this one was opened */
  pruned: string[];
}
export interface ArchiveLabel {
  text: string;
  tone: 'ok' | 'warn' | 'bad' | 'muted';
  title: string;
}
/** archive row pages per load (the daemon returns ≤1000 rows / ≤64 KiB per page; B6 keeps the window ≤ ARCHIVE_WINDOW_ROWS) */
export const ARCHIVE_ROW_PAGES = 20;
/** archive edge pages per view window (2048 edges each) */
export const ARCHIVE_EDGE_PAGES = 10;
export const ARCHIVE_NOTE = '아카이브가 켜져 있으면 전체 기록(모든 디코드 행·원시 D0–D7)은 캡처 아카이브에 남습니다 — 시작 후 상태 줄에서 확인하세요';
export const NOT_LOADED_DETAIL = '아카이브: 이 배율에서 전이가 많아 일부만 불러옴 — 확대하세요';
/** archive states whose files can be read */
export const ARCHIVE_READABLE: readonly ArchiveStateName[] = ['recording', 'complete', 'partial'];

/** B3b-M1. capture_state `archive` → ArchiveInfo; non-object → null; unknown/missing state word → 'unknown'.
 * Numbers via `num` (null when absent/invalid); strings via `str`; pruned via strList. */
export function readArchiveInfo(v: unknown): ArchiveInfo | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const names: readonly string[] = ['disabled', 'pending', 'recording', 'complete', 'partial', 'failed'];
  const w = str(o.state);
  return {
    state: (w !== null && names.includes(w) ? w : 'unknown') as ArchiveStateName,
    dir: str(o.dir),
    reason: str(o.reason),
    error: str(o.error),
    rawEndSample: num(o.raw_end_sample),
    rawBytes: num(o.raw_bytes),
    annotationRows: num(o.annotation_rows),
    rawTruncatedAt: num(o.raw_truncated_at_sample),
    annotationTruncatedAt: num(o.annotation_truncated_at_row),
    pruned: strList(o.pruned),
  };
}

/** B3b-M2. Status-line chip text (FUNCTIONS.md §B3b exact strings). */
export function archiveLabel(a: ArchiveInfo | null | undefined): ArchiveLabel {
  const fmtN = (n: number) => Math.round(n).toLocaleString('ko-KR');
  if (!a) {
    return { text: '아카이브 정보 없음', tone: 'muted', title: '이 캡처의 capture_state에 archive가 없습니다 (이전 엔진이거나 아직 수신 전)' };
  }
  const title = a.dir ?? '';
  switch (a.state) {
    case 'disabled':
      return { text: `아카이브 꺼짐 — ${a.reason ?? '사유 미보고'}`, tone: 'muted', title };
    case 'pending':
      return { text: '아카이브 준비 중', tone: 'muted', title };
    case 'recording':
      return { text: `아카이브 기록 중 · 디코드 ${fmtN(a.annotationRows ?? 0)}행`, tone: 'ok', title };
    case 'complete':
      return { text: `아카이브 완전 · 디코드 ${fmtN(a.annotationRows ?? 0)}행 · 원시 sample ${fmtN(a.rawEndSample ?? 0)}`, tone: 'ok', title };
    case 'partial':
      return {
        text:
          `아카이브 일부 — ${a.error ?? '잘림'}` +
          (a.rawTruncatedAt !== null ? ` · 원시 sample ${fmtN(a.rawTruncatedAt)}에서 잘림` : '') +
          (a.annotationTruncatedAt !== null ? ` · 디코드 ${fmtN(a.annotationTruncatedAt)}행에서 잘림` : ''),
        tone: 'warn',
        title,
      };
    case 'failed':
      return { text: `아카이브 실패 — ${a.error ?? '사유 미보고'}`, tone: 'bad', title };
    default:
      return { text: '아카이브 상태 미확인', tone: 'muted', title };
  }
}

export interface ArchiveEdgesPage {
  chunks: EdgeChunk[];
  nextFrom: number | null;
  complete: boolean;
  truncatedAt: number | null;
}
export interface ArchiveRowsPage {
  rows: Annotation[];
  rawLines: string[];
  rejected: number;
  nextRow: number | null;
  complete: boolean;
  truncatedAt: number | null;
  /** archive rows reported in this page (incl. unparsed) */
  count: number;
}

/** B3b-M7. `logic.archive.read` kind=edges result → chunks (readEdges with seq -1n, hostMonoNs null). */
export function readArchiveEdges(result: unknown, captureId: string): ArchiveEdgesPage {
  const o = obj(result);
  const chunks: EdgeChunk[] = [];
  if (Array.isArray(o.chunks)) {
    for (const c of o.chunks) {
      const e = readEdges(obj(c), captureId, -1n, null);
      if (e) chunks.push(e);
    }
  }
  return { chunks, nextFrom: num(o.next_from), complete: o.complete === true, truncatedAt: num(o.truncated_at) };
}

/** B3b-M8. `logic.archive.read` kind=annotations result → rows (readAnnotations 'logic.annotations', seq -1n). */
export function readArchiveRows(result: unknown, captureId: string, samplerate: number | null): ArchiveRowsPage {
  const o = obj(result);
  const list = Array.isArray(o.rows) ? o.rows : [];
  const r = readAnnotations('logic.annotations', { capture_id: captureId, samplerate, annotations: list }, captureId, -1n, samplerate);
  return {
    rows: r.rows,
    rawLines: r.rawLines,
    rejected: r.rejected,
    nextRow: num(o.next_row),
    complete: o.complete === true,
    truncatedAt: num(o.truncated_at),
    count: list.length,
  };
}

/** B3b-M9. Copy/coverage state of the archive view: no info → unknown; recording → live; complete with every
 * row loaded → complete; otherwise partial. */
export function archiveViewCoverage(a: ArchiveInfo | null | undefined, rowsAllLoaded: boolean): CoverageState {
  if (!a) return 'unknown';
  if (a.state === 'recording') return 'live';
  if (a.state === 'complete' && rowsAllLoaded) return 'complete';
  return 'partial';
}

// ---------------------------------------------------------------------------------------------
// B6 — bounded archive row window (ROOT browser defect D-L5W, FUNCTIONS.md §B6). The archive view holds at most
// ARCHIVE_WINDOW_ROWS consecutive archive rows, never evicts inside the window, names the held range, and moves
// with previous / next / seek. Archive row numbers are the daemon's 0-based `row`; the UI shows them 1-based.

/** rows the archive view holds at most; equal to the per-capture cap so the archive CaptureData never evicts */
export const ARCHIVE_WINDOW_ROWS = MAX_ANNOTATIONS_PER_CAPTURE;
/** rows asked per archive page (the daemon may return fewer: ≤64 KiB per page) */
export const ARCHIVE_PAGE_ROWS = 1000;

export interface ArchiveRowWindow {
  /** after_row the window was opened with (null = from archive row 0) */
  startAfter: number | null;
  /** 0-based archive row number of the first row held; null = nothing held yet */
  first: number | null;
  /** 0-based archive row number of the last row held; null = nothing held yet */
  last: number | null;
  /** archive rows held = sum of page.count (parsed rows + unparsed + rejected) */
  held: number;
  /** the daemon said nothing follows `last` (a page with next_row null) */
  eof: boolean;
}
/** where a new window starts (`startAfter` = after_row of its first request); null result = no such window */
export interface ArchiveWindowStart {
  startAfter: number | null;
}

/** B6-M1. A new, empty window opened after `startAfter`: `{startAfter, first: null, last: null, held: 0, eof: false}`. */
export function archiveWindowStart(startAfter: number | null): ArchiveRowWindow {
  return { startAfter, first: null, last: null, held: 0, eof: false };
}

/** B6-M2. Fold one archive page into the window; returns a NEW object (never mutates `w`).
 * `base` = (w.last ?? w.startAfter ?? −1) + 1 = archive row number of the page's first row.
 * count 0 → `{...w, eof: page.nextRow === null}`.
 * otherwise → first = w.first ?? base; last = page.nextRow ?? (base + page.count − 1); held = w.held + page.count;
 * eof = page.nextRow === null. */
export function archiveWindowAdd(w: ArchiveRowWindow, page: Pick<ArchiveRowsPage, 'count' | 'nextRow'>): ArchiveRowWindow {
  if (page.count === 0) return { ...w, eof: page.nextRow === null };
  const base = (w.last ?? w.startAfter ?? -1) + 1;
  return {
    startAfter: w.startAfter,
    first: w.first ?? base,
    last: page.nextRow ?? base + page.count - 1,
    held: w.held + page.count,
    eof: page.nextRow === null,
  };
}

/** B6-M3. after_row for the window's next page: `w.last ?? w.startAfter` (null = send no after_row). */
export function archiveWindowAfterRow(w: ArchiveRowWindow): number | null {
  return w.last ?? w.startAfter;
}

/** B6-M4. `limit` for the window's next page: max(0, min(ARCHIVE_PAGE_ROWS, ARCHIVE_WINDOW_ROWS − w.held)). */
export function archiveWindowLimit(w: ArchiveRowWindow): number {
  return Math.max(0, Math.min(ARCHIVE_PAGE_ROWS, ARCHIVE_WINDOW_ROWS - w.held));
}

/** B6-M5. `w.held >= ARCHIVE_WINDOW_ROWS`. */
export function archiveWindowFull(w: ArchiveRowWindow): boolean {
  return w.held >= ARCHIVE_WINDOW_ROWS;
}

/** B6-M6. The window just before `w`: null when `w.first` is null or 0. Otherwise
 * start = max(0, w.first − ARCHIVE_WINDOW_ROWS) → `{startAfter: start === 0 ? null : start − 1}`. */
export function archivePrevStart(w: ArchiveRowWindow): ArchiveWindowStart | null {
  if (w.first === null || w.first === 0) return null;
  const start = Math.max(0, w.first - ARCHIVE_WINDOW_ROWS);
  return { startAfter: start === 0 ? null : start - 1 };
}

/** B6-M7. The window right after `w`: null when `w.eof` or `w.last` is null; otherwise `{startAfter: w.last}`. */
export function archiveNextStart(w: ArchiveRowWindow): ArchiveWindowStart | null {
  if (w.eof || w.last === null) return null;
  return { startAfter: w.last };
}

/** B6-M8. Seek to the 1-based row the user typed. null when `row1` is not a finite integer, < 1, or
 * (total !== null and row1 > total). Otherwise start0 = row1 − 1 → `{startAfter: start0 === 0 ? null : start0 − 1}`. */
export function archiveSeekStart(row1: number, total: number | null): ArchiveWindowStart | null {
  if (!Number.isInteger(row1) || row1 < 1) return null;
  if (total !== null && row1 > total) return null;
  const start0 = row1 - 1;
  return { startAfter: start0 === 0 ? null : start0 - 1 };
}

/** B6-M9. Coverage of what the archive view holds (replaces archiveViewCoverage in the panel).
 * !a → 'unknown'; a.state 'recording' → 'live'; 'complete' only when ALL hold:
 *   a.state === 'complete'; w.startAfter === null; (w.first === null || w.first === 0); w.eof; evicted === 0;
 *   a.annotationRows === null || (w.last === null ? a.annotationRows === 0 : w.last + 1 === a.annotationRows).
 * Anything else → 'partial'. */
export function archiveWindowCoverage(a: ArchiveInfo | null | undefined, w: ArchiveRowWindow, evicted: number): CoverageState {
  if (!a) return 'unknown';
  if (a.state === 'recording') return 'live';
  const rowsAgree = a.annotationRows === null || (w.last === null ? a.annotationRows === 0 : w.last + 1 === a.annotationRows);
  if (a.state === 'complete' && w.startAfter === null && (w.first === null || w.first === 0) && w.eof && evicted === 0 && rowsAgree) {
    return 'complete';
  }
  return 'partial';
}

/** B6-M10. Visible text naming the held range, the total and the window cap (numbers `toLocaleString('ko-KR')`,
 * row numbers 1-based, `t` = total === null ? '?' : fmt(total), cap = fmt(ARCHIVE_WINDOW_ROWS)):
 *   nothing held (first or last null): eof → `행 없음 (전체 ${t}행)`; busy → '아카이브 불러오는 중…'; else '아카이브 창 비어 있음'.
 *   held: `행 ${fmt(first+1)}–${fmt(last+1)} / 전체 ${t}행` + suffix where head = first > 0 || startAfter !== null,
 *   tail = !eof:
 *     !head && !tail → ' (전체 표시)'
 *     head && tail   → ` · 앞뒤 미표시 (창 최대 ${cap}행)`
 *     head           → ` · 앞부분 미표시 (창 최대 ${cap}행)`
 *     tail           → ` · 뒷부분 미표시 (창 최대 ${cap}행)` */
export function archiveWindowText(w: ArchiveRowWindow, total: number | null, busy: boolean): string {
  const fmt = (n: number) => n.toLocaleString('ko-KR');
  const t = total === null ? '?' : fmt(total);
  if (w.first === null || w.last === null) {
    if (w.eof) return `행 없음 (전체 ${t}행)`;
    return busy ? '아카이브 불러오는 중…' : '아카이브 창 비어 있음';
  }
  const head = w.first > 0 || w.startAfter !== null;
  const tail = !w.eof;
  const cap = fmt(ARCHIVE_WINDOW_ROWS);
  let suffix = ' (전체 표시)';
  if (head && tail) suffix = ` · 앞뒤 미표시 (창 최대 ${cap}행)`;
  else if (head) suffix = ` · 앞부분 미표시 (창 최대 ${cap}행)`;
  else if (tail) suffix = ` · 뒷부분 미표시 (창 최대 ${cap}행)`;
  return `행 ${fmt(w.first + 1)}–${fmt(w.last + 1)} / 전체 ${t}행${suffix}`;
}

// ---------------------------------------------------------------------------------------------
// B4 — decoder tracks and error rows; B5 — software trigger (FUNCTIONS.md §B45u)

export interface DecoderTrack {
  /** decoder instance id as printed (uart-1) */
  id: string;
  /** rx | tx | mosi | miso */
  line: string;
  /** Dn */
  channel: string;
}
export type TriggerStateName = 'armed' | 'fired' | 'not_fired' | 'unknown';
export interface TriggerInfo {
  state: TriggerStateName;
  /** absolute trigger sample once fired */
  sample: number | null;
  channel: string | null;
  edge: string | null;
  preMs: number | null;
  postMs: number | null;
}

/** B45-M1. capture_state `decoder_tracks` → tracks (entries without string id/line/channel dropped); non-array → null. */
export function readDecoderTracks(v: unknown): DecoderTrack[] | null {
  if (!Array.isArray(v)) return null;
  const out: DecoderTrack[] = [];
  for (const x of v) {
    const o = obj(x);
    const id = str(o.id);
    const line = str(o.line);
    const channel = str(o.channel);
    if (id !== null && line !== null && channel !== null) out.push({ id, line, channel });
  }
  return out;
}

/** B45-M2. Track label for a decoder id: `uart-1 RX D0` (line upper-cased) when a track matches, else the id. */
export function trackLabel(id: string, tracks: readonly DecoderTrack[] | null | undefined): string {
  const t = tracks?.find((x) => x.id === id);
  return t ? `${id} ${t.line.toUpperCase()} ${t.channel}` : id;
}

/** B45-M3. capture_state `trigger` → TriggerInfo (state word outside the 3 names → 'unknown'); non-object → null. */
export function readTriggerInfo(v: unknown): TriggerInfo | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const w = str(o.state);
  return {
    state: w === 'armed' || w === 'fired' || w === 'not_fired' ? w : 'unknown',
    sample: num(o.sample),
    channel: str(o.channel),
    edge: str(o.edge),
    preMs: num(o.pre_ms),
    postMs: num(o.post_ms),
  };
}

/** B45-M4. Status/counter text: armed `트리거 대기 (D3 rising)`; fired `트리거 sample 12,345`; not_fired
 * `트리거 미발생 (D3 rising)`; unknown `트리거 상태 미확인`; null → ''. */
export function triggerText(t: TriggerInfo | null | undefined): string {
  if (!t) return '';
  const what = `${t.channel ?? '?'} ${t.edge ?? '?'}`;
  switch (t.state) {
    case 'armed':
      return `트리거 대기 (${what})`;
    case 'fired':
      return `트리거 sample ${t.sample === null ? '?' : Math.round(t.sample).toLocaleString('ko-KR')}`;
    case 'not_fired':
      return `트리거 미발생 (${what})`;
    default:
      return '트리거 상태 미확인';
  }
}
