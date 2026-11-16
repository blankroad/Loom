// logic.analyzer tools — pure text / search / copy / measurement functions (operator-upgrade logic B3a;
// findings L3 radix·search·line view·copy, L4 cursors·pulse measurement). Contracts: FUNCTIONS.md §B3a.
// Device text is untrusted data: everything here returns plain strings for textContent / clipboard.

import { type Annotation, type CaptureData, fmtSeconds } from './logicModel';

// ---------------------------------------------------------------------------------------------
// Radix / ASCII

export type Radix = 'hex' | 'ascii' | 'dec' | 'bin';
export const RADIXES: readonly Radix[] = ['hex', 'ascii', 'dec', 'bin'];

/** B3-T1. Decoded word value of an annotation text: `^[0-9A-Fa-f]{1,8}$` → hex value (sigrok uart/spi
 * `format=hex`), anything else (e.g. "Frame error") → null (not a data word). */
export function wordValue(text: string): number | null {
  return /^[0-9A-Fa-f]{1,8}$/.test(text) ? parseInt(text, 16) : null;
}

/** B3-T2. One value as ASCII: 0x20..0x7E as the char (backslash itself → `\\`), 0x0D → `\r`, 0x0A → `\n`,
 * 0x09 → `\t` (two visible chars each), other ≤ 0xFF → `\xHH` (upper-case), > 0xFF → `\x{H…}`. */
export function asciiOf(v: number): string {
  if (v === 0x5c) return '\\\\';
  if (v >= 0x20 && v <= 0x7e) return String.fromCharCode(v);
  if (v === 0x0d) return '\\r';
  if (v === 0x0a) return '\\n';
  if (v === 0x09) return '\\t';
  if (v <= 0xff) return `\\x${v.toString(16).toUpperCase().padStart(2, '0')}`;
  return `\\x{${v.toString(16).toUpperCase()}}`;
}

/** B3-T3. Annotation text in the chosen radix. Non-data text (wordValue null) is returned unchanged.
 * hex: upper-case, zero-padded to the original text length; dec: base 10; bin: padded to 8 / 16 / 32 bits
 * (v ≤ 0xFF / ≤ 0xFFFF / else); ascii: asciiOf. */
export function formatWord(text: string, radix: Radix): string {
  const v = wordValue(text);
  if (v === null) return text;
  switch (radix) {
    case 'hex':
      return v.toString(16).toUpperCase().padStart(text.length, '0');
    case 'dec':
      return String(v);
    case 'bin':
      return v.toString(2).padStart(v <= 0xff ? 8 : v <= 0xffff ? 16 : 32, '0');
    default:
      return asciiOf(v);
  }
}

// ---------------------------------------------------------------------------------------------
// Line view (CR / LF / CRLF / lone CR)

export type LineEnd = 'lf' | 'cr' | 'crlf' | 'none' | 'split' | 'note';
export interface TextLine {
  /** `${decoder}:${startSample}:${firstRowKey}` (stable across re-splits of the same rows) */
  key: string;
  decoder: string;
  /** ASCII rendering (asciiOf per byte) without the terminator; note lines: `⟦<row text>⟧` */
  text: string;
  startSample: number;
  endSample: number;
  /** keys of the rows that make this line, terminator rows included */
  rowKeys: string[];
  /** data bytes in the line, terminators included */
  bytes: number;
  end: LineEnd;
}
export const MAX_LINE_CHARS = 4096;

/** B3-T4. Split decoded rows into lines, per decoder (rows of different decoders never mix), FUNCTIONS §B3a. */
export function splitLines(rows: readonly Annotation[], maxChars = MAX_LINE_CHARS): TextLine[] {
  interface Open {
    line: TextLine;
  }
  interface DecState {
    open: Open | null;
    /** the line a CR just ended (a following LF belongs to it); null when the previous byte was not a CR */
    crLine: TextLine | null;
  }
  const out: TextLine[] = [];
  const st = new Map<string, DecState>();
  const stateOf = (d: string): DecState => {
    let s = st.get(d);
    if (!s) {
      s = { open: null, crLine: null };
      st.set(d, s);
    }
    return s;
  };
  const begin = (a: Annotation, text: string): TextLine => {
    const line: TextLine = {
      key: `${a.decoder}:${a.start}:${a.key}`,
      decoder: a.decoder,
      text,
      startSample: a.start,
      endSample: a.end,
      rowKeys: [],
      bytes: 0,
      end: 'none',
    };
    out.push(line);
    return line;
  };
  for (const a of rows) {
    const s = stateOf(a.decoder);
    const v = wordValue(a.text);
    if (v === null || v > 0xff) {
      if (s.open) {
        s.open.line.end = 'none';
        s.open = null;
      }
      const note = begin(a, `⟦${a.text}⟧`);
      note.rowKeys.push(a.key);
      note.end = 'note';
      s.crLine = null;
      continue;
    }
    if (v === 0x0a) {
      if (s.crLine) {
        s.crLine.rowKeys.push(a.key);
        s.crLine.endSample = a.end;
        s.crLine.bytes += 1;
        s.crLine.end = 'crlf';
        s.crLine = null;
        continue;
      }
      const line = s.open ? s.open.line : begin(a, '');
      line.rowKeys.push(a.key);
      line.endSample = a.end;
      line.bytes += 1;
      line.end = 'lf';
      s.open = null;
      continue;
    }
    if (v === 0x0d) {
      const line = s.open ? s.open.line : begin(a, '');
      line.rowKeys.push(a.key);
      line.endSample = a.end;
      line.bytes += 1;
      line.end = 'cr';
      s.open = null;
      s.crLine = line;
      continue;
    }
    s.crLine = null;
    const r = asciiOf(v);
    if (s.open && s.open.line.text.length + r.length > maxChars) {
      s.open.line.end = 'split';
      s.open = null;
    }
    if (!s.open) s.open = { line: begin(a, '') };
    const line = s.open.line;
    line.text += r;
    line.rowKeys.push(a.key);
    line.endSample = a.end;
    line.bytes += 1;
  }
  for (const s of st.values()) if (s.open) s.open.line.end = 'none';
  return out.sort((x, y) => x.startSample - y.startSample);
}

// ---------------------------------------------------------------------------------------------
// Search

export type SearchMode = 'text' | 'bytes';
export interface SearchQuery {
  mode: SearchMode;
  pattern: string;
  /** text mode only; bytes mode is always exact */
  caseSensitive: boolean;
}
export interface SearchHit {
  firstKey: string;
  lastKey: string;
  decoder: string;
  startSample: number;
  endSample: number;
}
export interface SearchResult {
  hits: SearchHit[];
  /** more than `limit` matches exist */
  truncated: boolean;
  /** user-visible reason the pattern is unusable (then hits = []) */
  error: string | null;
}
export const MAX_SEARCH_BYTES = 256;
export const MAX_SEARCH_HITS = 1000;

/** B61-T1 (ROOT follow-up to the live marker run). Index in `hits` of the hit identical to `prev` by stable identity
 * (same decoder, startSample and endSample — row keys may change across re-search); −1 when `prev` is null or
 * no such hit exists (the hit was lost: evicted, window moved, or no longer matches). First match wins. */
export function relocateHit(prev: SearchHit | null, hits: readonly SearchHit[]): number {
  if (!prev) return -1;
  return hits.findIndex((h) => h.decoder === prev.decoder && h.startSample === prev.startSample && h.endSample === prev.endSample);
}

/** B3-T5. "4C 4F" / "4c4f" / "0x4C,0x4F" → bytes; null if empty, malformed or > MAX_SEARCH_BYTES. */
export function parseBytesPattern(s: string): Uint8Array | null {
  const toks = s
    .trim()
    .split(/[\s,]+/)
    .filter((t) => t !== '')
    .map((t) => t.replace(/^0[xX]/, ''));
  if (toks.length === 0) return null;
  const out: number[] = [];
  if (toks.length > 1) {
    for (const t of toks) {
      if (!/^[0-9a-fA-F]{1,2}$/.test(t)) return null;
      out.push(parseInt(t, 16));
    }
  } else {
    const t = toks[0]!;
    if (!/^[0-9a-fA-F]+$/.test(t)) return null;
    if (t.length <= 2) out.push(parseInt(t, 16));
    else if (t.length % 2 === 0) for (let i = 0; i < t.length; i += 2) out.push(parseInt(t.slice(i, i + 2), 16));
    else return null;
  }
  if (out.length === 0 || out.length > MAX_SEARCH_BYTES) return null;
  return Uint8Array.from(out);
}

/** B3-T6. Byte-sequence search over consecutive data words of one decoder (FUNCTIONS §B3a). */
export function searchRows(rows: readonly Annotation[], q: SearchQuery, limit = MAX_SEARCH_HITS): SearchResult {
  const fail = (error: string): SearchResult => ({ hits: [], truncated: false, error });
  let pat: Uint8Array;
  if (q.mode === 'bytes') {
    if (q.pattern.trim() === '') return fail('검색어가 비어 있습니다');
    const p = parseBytesPattern(q.pattern);
    if (!p) return fail('바이트 패턴: 16진수 바이트 (예: 4C 4F 4F 4D)');
    pat = p;
  } else {
    if (q.pattern === '') return fail('검색어가 비어 있습니다');
    pat = new TextEncoder().encode(q.pattern);
    if (pat.length > MAX_SEARCH_BYTES) return fail('검색어가 256바이트를 넘습니다');
  }
  const fold = q.mode === 'text' && !q.caseSensitive;
  const f = (b: number) => (fold && b >= 0x41 && b <= 0x5a ? b + 32 : b);
  const want = Array.from(pat, f);
  const hits: SearchHit[] = [];
  interface Seg {
    vals: number[];
    rows: Annotation[];
  }
  const segs = new Map<string, Seg>();
  const run = (seg: Seg) => {
    const n = want.length;
    let i = 0;
    while (i + n <= seg.vals.length) {
      let ok = true;
      for (let j = 0; j < n; j++) {
        if (seg.vals[i + j] !== want[j]) {
          ok = false;
          break;
        }
      }
      if (ok) {
        const first = seg.rows[i]!;
        const last = seg.rows[i + n - 1]!;
        hits.push({ firstKey: first.key, lastKey: last.key, decoder: first.decoder, startSample: first.start, endSample: last.end });
        i += n;
      } else i++;
    }
    seg.vals = [];
    seg.rows = [];
  };
  for (const a of rows) {
    let seg = segs.get(a.decoder);
    if (!seg) {
      seg = { vals: [], rows: [] };
      segs.set(a.decoder, seg);
    }
    const v = wordValue(a.text);
    if (v === null || v > 0xff) {
      run(seg);
      continue;
    }
    seg.vals.push(f(v));
    seg.rows.push(a);
  }
  for (const seg of segs.values()) run(seg);
  hits.sort((x, y) => x.startSample - y.startSample);
  return { hits: hits.slice(0, limit), truncated: hits.length > limit, error: null };
}

// ---------------------------------------------------------------------------------------------
// Copy / export text (clipboard). Same metadata idea and time format as the daemon `logic.export`.

export type CopyScope = 'all' | 'cursors' | 'selection';
export interface CopyMeta {
  captureId: string;
  samplerate: number | null;
  /** 'panel' = rows this panel holds (live/snapshot); B3b adds 'archive' */
  source: 'panel' | 'archive';
  scope: CopyScope;
  fromSample: number;
  toSample: number;
  /** coverage state of the held data (decodeCoverage state) */
  coverage: 'complete' | 'partial' | 'live' | 'unknown';
  /** B6: archive view only — the held archive window (0-based archive row numbers, inclusive; total null = unknown) */
  archiveRows?: { first: number; last: number; total: number | null };
}

/** B3-T7. Exact mirror of the daemon's H4: sample / rate seconds via integer ns, 9 decimals, trailing zeros
 * and a trailing '.' trimmed ("0.00001", "0", "1.5"). rate null → `#<sample>`. */
export function secondsExact(sample: number, rate: number | null): string {
  const r = rate === null ? 0 : Math.round(rate);
  if (rate === null || !(r > 0)) return `#${sample}`;
  const ns = (BigInt(Math.round(sample)) * 1_000_000_000n) / BigInt(r);
  const secs = ns / 1_000_000_000n;
  const frac = (ns % 1_000_000_000n).toString().padStart(9, '0');
  return `${secs}.${frac}`.replace(/0+$/, '').replace(/\.$/, '');
}

/** B3-T8. Metadata lines (each ends with \n): `# loom logic copy`, `# capture_id=…`, `# source=…`,
 * `# scope=…`, `# from_sample=…`, `# to_sample=…`, `# rows=<rows>`, `# coverage=…`, then (B6)
 * `# archive_rows=<first>-<last>/<total|?>` iff meta.archiveRows is given, then `# radix=…` iff radix is given. */
export function copyHeader(meta: CopyMeta, rows: number, radix?: Radix): string {
  return (
    '# loom logic copy\n' +
    `# capture_id=${meta.captureId}\n` +
    `# source=${meta.source}\n` +
    `# scope=${meta.scope}\n` +
    `# from_sample=${meta.fromSample}\n` +
    `# to_sample=${meta.toSample}\n` +
    `# rows=${rows}\n` +
    `# coverage=${meta.coverage}\n` +
    (meta.archiveRows ? `# archive_rows=${meta.archiveRows.first}-${meta.archiveRows.last}/${meta.archiveRows.total ?? '?'}\n` : '') +
    (radix ? `# radix=${radix}\n` : '')
  );
}

/** B3-T9. header(radix) + one line per row: `<secondsExact(start)> <decoder>: <formatWord(text, radix)>`. */
export function rowsToText(rows: readonly Annotation[], radix: Radix, meta: CopyMeta): string {
  let out = copyHeader(meta, rows.length, radix);
  for (const a of rows) out += `${secondsExact(a.start, meta.samplerate)} ${a.decoder}: ${formatWord(a.text, radix)}\n`;
  return out;
}

/** B3-T10. header (no radix) + `start_sample,end_sample,t_start_s,t_end_s,decoder,text` + RFC 4180 rows
 * (raw text, never radix-converted). */
export function rowsToCsv(rows: readonly Annotation[], meta: CopyMeta): string {
  const f = (s: string) => (/[,"\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
  let out = `${copyHeader(meta, rows.length)}start_sample,end_sample,t_start_s,t_end_s,decoder,text\n`;
  for (const a of rows) {
    out += `${a.start},${a.end},${secondsExact(a.start, meta.samplerate)},${secondsExact(a.end, meta.samplerate)},${f(a.decoder)},${f(a.text)}\n`;
  }
  return out;
}

/** B3-T11. header(radix 'ascii') + one line per TextLine: `<secondsExact(start)> <decoder>: <text>`. */
export function linesToText(lines: readonly TextLine[], meta: CopyMeta): string {
  let out = copyHeader(meta, lines.length, 'ascii');
  for (const l of lines) out += `${secondsExact(l.startSample, meta.samplerate)} ${l.decoder}: ${l.text}\n`;
  return out;
}

// ---------------------------------------------------------------------------------------------
// Cursors and pulse measurement (L4)

export interface Cursors {
  /** absolute samples; null = not placed */
  a: number | null;
  b: number | null;
}
export interface CursorMeasure {
  dSamples: number;
  dtS: number | null;
  freqHz: number | null;
  text: string;
}

/** B3-T12. Hz with 3 decimals trimmed: ≥ 1e6 MHz, ≥ 1e3 kHz, else Hz ("115.2 kHz", "0.5 Hz"). */
export function fmtHz(f: number): string {
  const units: [number, string][] = [
    [1e6, 'MHz'],
    [1e3, 'kHz'],
    [1, 'Hz'],
  ];
  for (const [u, name] of units) {
    const v = +(f / u).toFixed(3);
    if (v >= 1 || u === 1) return `${v} ${name}`;
  }
  return `${f} Hz`;
}

/** B3-T13. Both cursors set → Δ; text `A <t> · B <t> · Δ <n> smp · Δt <s> · 1/Δt <Hz>` (time parts only with a
 * rate; 1/Δt only when Δ > 0); otherwise null. */
export function measureCursors(c: Cursors, rate: number | null): CursorMeasure | null {
  if (c.a === null || c.b === null) return null;
  const dSamples = Math.abs(c.b - c.a);
  const dtS = rate ? dSamples / rate : null;
  const freqHz = rate && dSamples > 0 ? rate / dSamples : null;
  // same format as fmtSampleTime (not imported here)
  const at = (x: number) => (rate ? fmtSeconds(x / rate, 4) : `#${Math.round(x).toLocaleString('ko-KR')}`);
  const parts = [`A ${at(c.a)}`, `B ${at(c.b)}`, `Δ ${Math.round(dSamples).toLocaleString('ko-KR')} smp`];
  if (dtS !== null) parts.push(`Δt ${fmtSeconds(dtS, 4)}`);
  if (freqHz !== null) parts.push(`1/Δt ${fmtHz(freqHz)}`);
  return { dSamples, dtS, freqHz, text: parts.join(' · ') };
}

export interface PulseMeasure {
  level: 0 | 1;
  /** sample where this level began (a real transition) */
  start: number;
  /** sample of the transition that ended it */
  end: number;
  /** end of the following opposite level, when known */
  nextEnd: number | null;
  widthS: number | null;
  periodS: number | null;
  /** HIGH share of the period in percent, when the period is known */
  dutyPct: number | null;
  text: string;
}
/** transitions scanned per direction before giving up (hover stays cheap) */
export const PULSE_SCAN_EDGES = 100_000;

/** B3-T14. The level run of `bit` containing `sample`, bounded by two REAL transitions of that bit inside
 * known, contiguous raw data; null whenever either bound is unknown (FUNCTIONS §B3a — never invented). */
export function pulseAt(cap: CaptureData, bit: number, sample: number, rate: number | null): PulseMeasure | null {
  const fmtN = (n: number) => Math.round(n).toLocaleString('ko-KR');
  const st = cap.stateAt(sample);
  if (st === null) return null;
  const bitOf = (m: number) => (m >> bit) & 1;
  const level = bitOf(st) as 0 | 1;
  const chunks = cap.chunks;
  /** last chunk whose start ≤ s (−1 when none) */
  const idxAt = (s: number): number => {
    let lo = 0;
    let hi = chunks.length - 1;
    let r = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (chunks[mid]!.start <= s) {
        r = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return r;
  };
  /** number of edges of `c` with sample ≤ s */
  const upper = (c: { samples: Float64Array }, s: number): number => {
    let lo = 0;
    let hi = c.samples.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (c.samples[mid]! <= s) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  };
  const finalOf = (c: { samples: Float64Array; masks: Uint16Array; initial: number }) => (c.samples.length ? c.masks[c.samples.length - 1]! : c.initial);
  const back = (): number | null => {
    let k = idxAt(sample);
    if (k < 0) return null;
    let c = chunks[k]!;
    let i = upper(c, sample) - 1;
    let scanned = 0;
    for (;;) {
      for (; i >= 0; i--) {
        if (++scanned > PULSE_SCAN_EDGES) return null;
        const before = i === 0 ? c.initial : c.masks[i - 1]!;
        if (bitOf(c.masks[i]!) !== bitOf(before)) return c.samples[i]!;
      }
      const prev = chunks[k - 1];
      if (!prev || prev.end !== c.start) return null;
      if (bitOf(c.initial) !== bitOf(finalOf(prev))) return c.start;
      k--;
      c = prev;
      i = c.samples.length - 1;
    }
  };
  /** first real transition of `bit` strictly after `from` (null = none within known contiguous data) */
  const fwd = (from: number): number | null => {
    let k = idxAt(from);
    if (k < 0) return null;
    let c = chunks[k]!;
    let i = upper(c, from);
    let scanned = 0;
    for (;;) {
      for (; i < c.samples.length; i++) {
        if (++scanned > PULSE_SCAN_EDGES) return null;
        const before = i === 0 ? c.initial : c.masks[i - 1]!;
        if (bitOf(c.masks[i]!) !== bitOf(before)) return c.samples[i]!;
      }
      const next = chunks[k + 1];
      if (!next || next.start !== c.end) return null;
      if (bitOf(next.initial) !== bitOf(finalOf(c))) return next.start;
      k++;
      c = next;
      i = 0;
    }
  };
  const p = back();
  const n = fwd(sample);
  if (p === null || n === null) return null;
  if (p < cap.evictedTo || cap.unknown.some((u) => u.start < n && u.end > p)) return null;
  let n2: number | null = fwd(n);
  if (n2 !== null && cap.unknown.some((u) => u.start < n2! && u.end > n)) n2 = null;
  const w = n - p;
  const widthS = rate ? w / rate : null;
  const per = n2 !== null ? n2 - p : null;
  const periodS = rate && per !== null ? per / rate : null;
  const dutyPct = per !== null ? (level ? (w / per) * 100 : ((per - w) / per) * 100) : null;
  let text = `${level ? 'HIGH' : 'LOW'} ${rate ? `${fmtSeconds(widthS!, 4)} (${fmtN(w)} smp)` : `${fmtN(w)} smp`}`;
  if (per !== null && dutyPct !== null) {
    text += ` · 주기 ${rate ? fmtSeconds(periodS!, 4) : `${fmtN(per)} smp`}${rate ? ` · ${fmtHz(1 / periodS!)}` : ''} · duty ${dutyPct.toFixed(1)}%`;
  }
  return { level, start: p, end: n, nextEnd: n2, widthS, periodS, dutyPct, text };
}
