// U1-B (operator-upgrade/uart B5) — export a PAST range of one UART channel from the daemon journal.
// Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B5.1 (Opus owns types/signatures/constants; bodies are Sonnet B5).
//
// Source: read-only `core.events.query` with `peek: true` (never moves the client's @last cursor), current daemon run
// (stream epoch) only. Every exported line carries the canonical stream seq. The header states provenance, requested and
// actual range, completeness (`complete` | `partial: <reasons>`), gaps and caps — "unknown/partial is not zero or PASS"
// (COMMON). Bounded: ≤ EXPORT_MAX_BYTES output, ≤ EXPORT_MAX_PAGES pages, ≤ EXPORT_MAX_PROBES locate probes, cancel by
// AbortSignal. Device text is data: it is only ever written into the file, never interpreted.

import type { LoomEvent } from '../../proto/provisional';
import { decodePayloadBytes } from '../../sdk/terminalView';

export const EXPORT_QUERY = 'core.events.query';
export const EXPORT_MAX_BYTES = 32 * 1024 * 1024;
export const EXPORT_MAX_PAGES = 2_000;
export const EXPORT_PAGE_ROWS = 1_000;
export const EXPORT_MAX_PROBES = 64;
/** gaps listed individually in the header / summary (the count is always exact) */
export const EXPORT_MAX_GAPS = 1_000;
export const EXPORT_KINDS: readonly string[] = ['uart.data', 'uart.state', 'uart.config', 'record.gap'];
export const EXPORT_RECENT_MINUTES: readonly number[] = [5, 15, 60, 240];
/** an unterminated .log line is written in pieces of this many characters (same rule as REC) */
export const EXPORT_LINE_MAX = 64 * 1024;

export type ExportFormat = 'log' | 'jsonl';
export type ExportRange = { kind: 'retained' } | { kind: 'recent'; minutes: number } | { kind: 'generation'; generation: number };
export type GapKind = 'rx_offset' | 'record_gap' | 'unreadable' | 'retention';
export interface ExportGap {
  kind: GapKind;
  /** stream seq where it was noticed (decimal string), null = before the first exported record */
  atSeq: string | null;
  detail: string;
  /** missing RX bytes when known (rx_offset jumps) */
  bytes?: number;
}
/** why an export is not complete; [] = complete */
export type Incomplete = 'gaps' | 'cap_bytes' | 'cap_pages' | 'cancelled' | 'retention_moved' | 'query_failed' | 'probe_limit';
export interface ExportLimits {
  maxBytes: number;
  maxPages: number;
  maxProbes: number;
  pageRows: number;
}
export const DEFAULT_EXPORT_LIMITS: ExportLimits = { maxBytes: EXPORT_MAX_BYTES, maxPages: EXPORT_MAX_PAGES, maxProbes: EXPORT_MAX_PROBES, pageRows: EXPORT_PAGE_ROWS };

export interface ExportSummary {
  channel: string;
  format: ExportFormat;
  range: ExportRange;
  /** stream epoch of this daemon run (decimal string); null when the daemon returned none */
  epoch: string | null;
  /** stream high-water seq when the export started = fixed upper bound of the request */
  hwSeq: string | null;
  /** first seq requested after locating the range start */
  startSeq: string | null;
  /** oldest seq still in the recording (found by probing); null = not probed / nothing retained */
  oldestRetainedSeq: string | null;
  first: { seq: string; wallMs: number } | null;
  last: { seq: string; wallMs: number } | null;
  generations: number[];
  events: number;
  rxBytes: number;
  txBytes: number;
  pages: number;
  probes: number;
  /** body bytes (UTF-8) */
  outBytes: number;
  gaps: ExportGap[];
  /** gaps beyond EXPORT_MAX_GAPS (counted, not listed) */
  gapsDropped: number;
  incomplete: Incomplete[];
  /** message of the failure behind `query_failed` */
  error: string | null;
  startedMs: number;
  /** the limits this export ran with (stated in the header) */
  limits: ExportLimits;
}
export interface ExportResult extends ExportSummary {
  /** header + body */
  text: string;
  fileName: string;
}
export interface ExportProgress {
  phase: 'locate' | 'read' | 'done';
  pages: number;
  events: number;
  outBytes: number;
}
/** `ctx.query` shape: resolves with the unwrapped result, rejects with a NetError (`dataCode`, `data`) */
export type QueryFn = (rpc: string, params: object, signal?: AbortSignal) => Promise<unknown>;
export interface ExportOptions {
  query: QueryFn;
  channel: string;
  range: ExportRange;
  format: ExportFormat;
  signal: AbortSignal;
  /** wall clock ms at start (recent-minutes threshold, header, file name) */
  nowMs: number;
  limits?: Partial<ExportLimits>;
  onProgress?: (p: ExportProgress) => void;
}

/** ANSI/OSC escapes removed from .log text (same expression as REC, recorder.ts) */
// eslint-disable-next-line no-control-regex -- ANSI/OSC escape sequences are control characters by definition
export const ANSI_RE = /\x1b\[[0-9;?]*[A-Za-z~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][A-Z0-9]|\x1b[A-Za-z]/g;

/** ts.wall_ns (decimal string) → ms; 0 when missing/invalid. §B5.1 */
export function wallMsOf(ev: LoomEvent): number {
  const ns = (ev as { ts?: { wall_ns?: unknown } }).ts?.wall_ns;
  if (typeof ns !== 'string' && typeof ns !== 'number') return 0;
  try {
    return Number(BigInt(ns) / 1_000_000n);
  } catch {
    return 0;
  }
}

/** `e<epoch>:seq:<seq>` (the daemon's Cursor::Seq display form). §B5.1 */
export function cursorAt(epoch: string, seq: bigint): string {
  return `e${epoch}:seq:${seq}`;
}

/** local `YYYY-MM-DD HH:mm:ss.SSS` (same as REC). §B5.1 */
export function fmtWall(ms: number): string {
  const d = new Date(ms);
  const p = (n: number, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

/** `<channel>-journal-YYYYMMDD-HHmmss.<format>` (local time). §B5.1 */
export function exportFileName(channel: string, format: ExportFormat, nowMs: number): string {
  const d = new Date(nowMs);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${channel}-journal-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.${format}`;
}

/** human text of a range: '보존 범위 전체' | '최근 N분' | '연결 세대 N'. §B5.1 */
export function rangeText(r: ExportRange): string {
  if (r.kind === 'retained') return '보존 범위 전체';
  if (r.kind === 'recent') return `최근 ${r.minutes}분`;
  return `연결 세대 ${r.generation}`;
}

/** NetError-like → `dataCode` (e.g. 'E_CURSOR_GAP') or null. §B5.1 */
export function errCode(e: unknown): string | null {
  const c = (e as { dataCode?: unknown } | null)?.dataCode;
  return typeof c === 'string' ? c : null;
}

/** NetError-like → `data.detail` when it is an object, else `data`, else null. §B5.1 */
export function errDetail(e: unknown): Record<string, unknown> | null {
  const data = (e as { data?: unknown } | null)?.data;
  if (typeof data !== 'object' || data === null) return null;
  const detail = (data as { detail?: unknown }).detail;
  if (typeof detail === 'object' && detail !== null) return detail as Record<string, unknown>;
  return data as Record<string, unknown>;
}

/** Smallest seq in [lo, hi] for which the monotone `test` is true (false…false true…true); null when none.
 * At most `maxProbes` calls; `exhausted` = the cap stopped the search (then `seq` is the best upper bound found). §B5.1 */
export async function findFirstSeq(
  lo: bigint,
  hi: bigint,
  test: (seq: bigint) => Promise<boolean>,
  maxProbes: number,
): Promise<{ seq: bigint | null; probes: number; exhausted: boolean }> {
  let l = lo;
  let h = hi;
  let best: bigint | null = null;
  let probes = 0;
  while (l <= h) {
    if (probes >= maxProbes) return { seq: best, probes, exhausted: true };
    const mid = l + (h - l) / 2n;
    probes++;
    if (await test(mid)) {
      best = mid;
      h = mid - 1n;
    } else {
      l = mid + 1n;
    }
  }
  return { seq: best, probes, exhausted: false };
}

/** RX byte continuity: an `rx_offset` jump forward = bytes the journal does not have; `record.gap` events are gaps too. §B5.1 */
export class GapTracker {
  private nextRx: bigint | null = null;
  observe(ev: LoomEvent): ExportGap | null {
    const payload = ((ev as { payload?: unknown }).payload ?? {}) as Record<string, unknown>;
    if (ev.kind === 'record.gap') {
      const lost = payload.lost_bytes;
      const gap: ExportGap = { kind: 'record_gap', atSeq: String(ev.seq), detail: 'record.gap' + (typeof lost === 'number' ? ` ${lost} B` : '') };
      if (typeof lost === 'number') gap.bytes = lost;
      return gap;
    }
    if (ev.kind !== 'uart.data' || (ev as { dir?: unknown }).dir !== 'rx') return null;
    const raw = payload.rx_offset;
    let off: bigint;
    try {
      if (typeof raw === 'number' && Number.isInteger(raw)) off = BigInt(raw);
      else if (typeof raw === 'string' && /^\d+$/.test(raw)) off = BigInt(raw);
      else return null;
    } catch {
      return null;
    }
    const len = typeof payload.len === 'number' ? payload.len : (decodePayloadBytes(payload.bytes)?.length ?? 0);
    let gap: ExportGap | null = null;
    if (this.nextRx !== null && off > this.nextRx) {
      const n = off - this.nextRx;
      gap = { kind: 'rx_offset', atSeq: String(ev.seq), bytes: Number(n), detail: `RX ${n} B 누락 (rx_offset ${this.nextRx}→${off})` };
    }
    const end = off + BigInt(len);
    this.nextRx = this.nextRx === null || end > this.nextRx ? end : this.nextRx;
    return gap;
  }
}

/** Formats published records into the export body (.log lines like REC + `#seq`, or .jsonl objects). §B5.1 */
export class ExportWriter {
  bytes = 0;
  capped = false;
  private parts: string[] = [];
  private pending: Record<'rx' | 'tx', { text: string; wallMs: number; seq: string } | null> = { rx: null, tx: null };
  private dec: Record<'rx' | 'tx', TextDecoder> = { rx: new TextDecoder('utf-8', { fatal: false }), tx: new TextDecoder('utf-8', { fatal: false }) };
  constructor(readonly format: ExportFormat, readonly maxBytes: number = EXPORT_MAX_BYTES) {}

  /** append `s` unless the output cap would be exceeded (then it and everything after is dropped) */
  private put(s: string): void {
    if (this.capped) return;
    let n = 0;
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c < 0x80) n += 1;
      else if (c < 0x800) n += 2;
      else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length && (s.charCodeAt(i + 1) & 0xfc00) === 0xdc00) {
        n += 4;
        i++;
      } else n += 3;
    }
    if (this.bytes + n > this.maxBytes) {
      this.capped = true;
      return;
    }
    this.bytes += n;
    this.parts.push(s);
  }

  /** one finished `.log` data line */
  private line(dir: 'rx' | 'tx', wallMs: number, seq: string, text: string, tail = ''): void {
    this.put(`${fmtWall(wallMs)} #${seq} ${dir.toUpperCase()}  ${text}${tail}\n`);
  }

  /** one published record of EXPORT_KINDS */
  record(ev: LoomEvent): void {
    if (this.capped) return;
    const e = ev as unknown as Record<string, unknown>;
    const payload = (e.payload ?? {}) as Record<string, unknown>;
    const seq = String(ev.seq);
    const wallMs = wallMsOf(ev);
    const isData = ev.kind === 'uart.data';
    const dir: 'rx' | 'tx' = e.dir === 'tx' ? 'tx' : 'rx';
    if (this.format === 'jsonl') {
      const base = { epoch: String(e.epoch ?? ''), seq, t: new Date(wallMs).toISOString(), ms: wallMs };
      if (!isData) {
        this.put(`${JSON.stringify({ type: 'event', epoch: base.epoch, seq, kind: ev.kind, t: base.t, ms: wallMs, generation: e.generation ?? null, payload })}\n`);
        return;
      }
      const bytes = decodePayloadBytes(payload.bytes);
      const off = payload.rx_offset;
      const row: Record<string, unknown> = {
        type: 'data',
        ...base,
        dir: e.dir,
        origin: e.origin,
        generation: e.generation ?? null,
        rx_offset: typeof off === 'number' || typeof off === 'string' ? String(off) : null,
        len: typeof payload.len === 'number' ? payload.len : (bytes?.length ?? 0),
        hex: bytes ? Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('') : null,
        text: bytes ? this.dec[dir].decode(bytes, { stream: true }) : null,
      };
      if (payload.secret === true) row.secret = true;
      Object.assign(row, {
        producer_id: e.producer_id,
        producer_epoch: e.producer_epoch,
        source_seq: e.source_seq,
        request_id: e.request_id ?? null,
        client_id: e.client_id ?? null,
      });
      this.put(`${JSON.stringify(row)}\n`);
      return;
    }
    if (!isData) {
      this.put(`-- ${fmtWall(wallMs)} #${seq} ${ev.kind} ${JSON.stringify(payload)}\n`);
      return;
    }
    const bytes = decodePayloadBytes(payload.bytes);
    if (!bytes) {
      const cur = this.pending[dir];
      if (cur) {
        this.line(dir, cur.wallMs, cur.seq, cur.text, ' ⟨줄바꿈 없음⟩');
        this.pending[dir] = null;
      }
      this.put(`${fmtWall(wallMs)} #${seq} ${dir.toUpperCase()}  ⟨바이트 없음 (${payload.secret ? 'secret' : 'raw_ref'})⟩\n`);
      return;
    }
    let cur = this.pending[dir];
    const text = this.dec[dir].decode(bytes, { stream: true }).replace(ANSI_RE, '');
    for (const ch of text) {
      if (ch === '\n') {
        this.line(dir, cur?.wallMs ?? wallMs, cur?.seq ?? seq, cur?.text ?? '');
        cur = null;
      } else if (ch === '\r') continue;
      else if (ch === '\b' || ch === '\x7f') {
        if (cur) cur.text = cur.text.slice(0, -1);
      } else if (ch < ' ' && ch !== '\t') continue;
      else {
        if (!cur) cur = { text: '', wallMs, seq };
        cur.text += ch;
        if (cur.text.length >= EXPORT_LINE_MAX) {
          this.line(dir, cur.wallMs, cur.seq, cur.text, ` ⟨줄바꿈 없이 ${EXPORT_LINE_MAX / 1024} KiB — 다음 줄에 이어짐⟩`);
          cur = null;
        }
      }
    }
    this.pending[dir] = cur;
  }

  /** a `--`/`{"type":"note"}` line (e.g. a gap noticed by the tracker) */
  note(text: string, wallMs: number, seq: string | null): void {
    if (this.format === 'jsonl') this.put(`${JSON.stringify({ type: 'note', seq, t: new Date(wallMs).toISOString(), ms: wallMs, text })}\n`);
    else this.put(`-- ${fmtWall(wallMs)} #${seq ?? '-'} ${text}\n`);
  }

  /** flush unterminated lines (`⟨줄바꿈 없음⟩`) */
  finish(): void {
    for (const dir of ['rx', 'tx'] as const) {
      const rest = this.dec[dir].decode();
      if (rest && this.format === 'log') {
        const cur = this.pending[dir];
        if (cur) cur.text += rest.replace(ANSI_RE, '');
      }
      const cur = this.pending[dir];
      if (cur) this.line(dir, cur.wallMs, cur.seq, cur.text, ' ⟨줄바꿈 없음⟩');
      this.pending[dir] = null;
    }
  }

  body(): string {
    return this.parts.join('');
  }
}

/** Header text (ends with '\n'): `# …` lines for .log, one `{"type":"header",…}` line for .jsonl. §B5.1 */
export function exportHeader(s: ExportSummary): string {
  const complete = s.incomplete.length === 0;
  if (s.format === 'jsonl') {
    return `${JSON.stringify({
      type: 'header',
      provenance: 'daemon-journal-export',
      channel: s.channel,
      format: s.format,
      range: s.range,
      epoch: s.epoch,
      hwSeq: s.hwSeq,
      startSeq: s.startSeq,
      oldestRetainedSeq: s.oldestRetainedSeq,
      first: s.first,
      last: s.last,
      generations: s.generations,
      events: s.events,
      rxBytes: s.rxBytes,
      txBytes: s.txBytes,
      pages: s.pages,
      probes: s.probes,
      outBytes: s.outBytes,
      gaps: s.gaps,
      gapsDropped: s.gapsDropped,
      incomplete: s.incomplete,
      error: s.error,
      startedMs: s.startedMs,
      limits: s.limits,
      complete,
    })}\n`;
  }
  const out: string[] = [
    `# Loom UART 저널 내보내기 · ${s.channel} · ${fmtWall(s.startedMs)}`,
    `# provenance: daemon-journal-export · core.events.query (peek) · stream epoch ${s.epoch ?? '?'} · 이전 데몬 실행(epoch)의 기록은 포함하지 않음`,
    `# 요청 범위: ${rangeText(s.range)} · 상한 seq ${s.hwSeq ?? '?'} · 시작 seq ${s.startSeq ?? '-'}`,
    `# 보존 시작 seq: ${s.oldestRetainedSeq ?? '?'}`,
    s.first && s.last
      ? `# 실제 범위: seq ${s.first.seq}–${s.last.seq} · ${fmtWall(s.first.wallMs)} – ${fmtWall(s.last.wallMs)}`
      : '# 실제 범위: 없음 (해당 기록 0건)',
    `# 세대(generation): ${s.generations.length ? s.generations.join(', ') : '없음'} · 기록 ${s.events}건 · RX ${s.rxBytes} B · TX ${s.txBytes} B · 페이지 ${s.pages} · 탐색 ${s.probes}`,
    complete ? '# 완전성: complete' : `# 완전성: partial: ${s.incomplete.join(', ')}${s.error ? ` — ${s.error}` : ''}`,
  ];
  if (s.gaps.length || s.gapsDropped) {
    out.push(`# 공백 ${s.gaps.length + s.gapsDropped}건: ${s.gaps.slice(0, 20).map((g) => `${g.kind}@${g.atSeq ?? '-'} ${g.detail}`).join('; ')}`);
  }
  out.push(
    `# 한도: 출력 ${s.limits.maxBytes / (1024 * 1024)} MiB · ${s.limits.maxPages} 페이지 · 페이지당 ${s.limits.pageRows}건 · 탐색 ${s.limits.maxProbes}`,
    '# 열: 시각 #seq 방향  텍스트 (ANSI 제거·줄 단위; 원문 바이트는 .jsonl)',
  );
  return `${out.join('\n')}\n`;
}

/** One-line UI summary of a finished export (status line / toast). §B5.1 */
export function summaryText(s: ExportSummary): string {
  const n = s.gaps.length + s.gapsDropped;
  return `${s.incomplete.length === 0 ? 'complete' : `partial: ${s.incomplete.join(', ')}`} · ${s.events}건 · ${s.outBytes} B${n ? ` · 공백 ${n}건` : ''}`;
}

/** Locate the range, page the journal, build the file. Never throws for daemon errors: they end up in
 * `incomplete`/`error`. §B5.1 */
export async function runExport(o: ExportOptions): Promise<ExportResult> {
  const limits: ExportLimits = { ...DEFAULT_EXPORT_LIMITS, ...o.limits };
  const s: ExportSummary = {
    channel: o.channel,
    format: o.format,
    range: o.range,
    epoch: null,
    hwSeq: null,
    startSeq: null,
    oldestRetainedSeq: null,
    first: null,
    last: null,
    generations: [],
    events: 0,
    rxBytes: 0,
    txBytes: 0,
    pages: 0,
    probes: 0,
    outBytes: 0,
    gaps: [],
    gapsDropped: 0,
    incomplete: [],
    error: null,
    startedMs: o.nowMs,
    limits,
  };
  const writer = new ExportWriter(o.format, limits.maxBytes);
  const tracker = new GapTracker();
  const gens = new Set<number>();
  const inc = (x: Incomplete): void => {
    if (!s.incomplete.includes(x)) s.incomplete.push(x);
  };
  const progress = (phase: ExportProgress['phase']): void => o.onProgress?.({ phase, pages: s.pages, events: s.events, outBytes: writer.bytes });
  const addGap = (g: ExportGap): void => {
    if (s.gaps.length < EXPORT_MAX_GAPS) s.gaps.push(g);
    else s.gapsDropped++;
  };
  const q = (p: object): Promise<unknown> =>
    o.query(EXPORT_QUERY, { ...p, filter: { domains: ['uart'], channels: [o.channel], kinds: [...EXPORT_KINDS] }, peek: true }, o.signal);
  type Page = {
    events?: LoomEvent[];
    truncated?: boolean;
    next_cursor?: string | null;
    unreadable?: { from: unknown; to: unknown; cause?: unknown }[];
    stream_epoch?: unknown;
    stream_seq_hw?: unknown;
  };
  const isAbort = (e: unknown): boolean => o.signal.aborted || (e as { name?: unknown } | null)?.name === 'AbortError';

  const work = async (): Promise<void> => {
    progress('locate');
    // 1. head: stream epoch + high-water seq = the fixed upper bound
    const head = (await q({ max: 1 })) as Page;
    if (head.stream_epoch === undefined || head.stream_epoch === null || head.stream_seq_hw === undefined || head.stream_seq_hw === null) {
      inc('query_failed');
      s.error = 'daemon returned no stream epoch / high-water seq';
      return;
    }
    const epoch = String(head.stream_epoch);
    const hw = BigInt(head.stream_seq_hw as string | number);
    s.epoch = epoch;
    s.hwSeq = String(hw);
    if (hw === 0n) return;
    const until = cursorAt(epoch, hw);
    // a locate probe: the first event at/after `seq` (or the daemon's cursor error)
    const probe = async (seq: bigint): Promise<{ gap: boolean; first: LoomEvent | null; scanLimit: boolean }> => {
      try {
        const r = (await q({ since: cursorAt(epoch, seq), until, max: 1 })) as Page;
        return { gap: false, first: r.events?.[0] ?? null, scanLimit: false };
      } catch (e) {
        const code = errCode(e);
        if (code === 'E_CURSOR_GAP') return { gap: true, first: null, scanLimit: false };
        if (code === 'E_SCAN_LIMIT') {
          const partial = errDetail(e)?.partial;
          return { gap: false, first: Array.isArray(partial) ? ((partial[0] as LoomEvent | undefined) ?? null) : null, scanLimit: true };
        }
        throw e;
      }
    };
    // 2. oldest retained seq
    const oldestR = await findFirstSeq(1n, hw, async (seq) => !(await probe(seq)).gap, limits.maxProbes);
    s.probes = oldestR.probes;
    if (oldestR.exhausted) {
      inc('probe_limit');
      return;
    }
    if (oldestR.seq === null) {
      addGap({ kind: 'retention', atSeq: null, detail: '요청 범위가 모두 보존 범위 밖' });
      return;
    }
    const oldest = oldestR.seq;
    s.oldestRetainedSeq = String(oldest);
    // 3. start of the requested range
    let start: bigint | null = oldest;
    if (o.range.kind !== 'retained') {
      const range = o.range;
      const threshold = o.nowMs - (range.kind === 'recent' ? range.minutes : 0) * 60_000;
      const startR = await findFirstSeq(
        oldest,
        hw,
        async (seq) => {
          const pr = await probe(seq);
          if (pr.gap) return false;
          if (!pr.first) return true;
          return range.kind === 'recent' ? wallMsOf(pr.first) >= threshold : ((pr.first as { generation?: number }).generation ?? 0) >= range.generation;
        },
        Math.max(0, limits.maxProbes - s.probes),
      );
      s.probes += startR.probes;
      if (startR.exhausted) {
        inc('probe_limit');
        return;
      }
      start = startR.seq;
      if (start !== null && start === oldest && oldest > 1n) {
        addGap({ kind: 'retention', atSeq: null, detail: `요청 시작이 보존 범위보다 오래됨 (보존 시작 seq ${oldest})` });
      }
    }
    if (start === null) return;
    s.startSeq = String(start);
    // 4. page the journal
    let since = cursorAt(epoch, start);
    for (;;) {
      if (o.signal.aborted) {
        inc('cancelled');
        return;
      }
      if (s.pages >= limits.maxPages) {
        inc('cap_pages');
        return;
      }
      let page: Page;
      try {
        page = (await q({ since, until, max: limits.pageRows })) as Page;
      } catch (e) {
        const code = errCode(e);
        if (code === 'E_SCAN_LIMIT') {
          const d = errDetail(e);
          const partial = d?.partial;
          const resume = d?.resume_from;
          if (!Array.isArray(partial) || typeof resume !== 'string') {
            inc('query_failed');
            s.error = e instanceof Error ? e.message : String(e);
            return;
          }
          page = { events: partial as LoomEvent[], truncated: true, next_cursor: resume };
        } else if (code === 'E_CURSOR_GAP') {
          inc('retention_moved');
          return;
        } else if (isAbort(e)) {
          inc('cancelled');
          return;
        } else {
          inc('query_failed');
          s.error = e instanceof Error ? e.message : String(e);
          return;
        }
      }
      s.pages++;
      for (const u of page.unreadable ?? []) {
        const from = String(u.from);
        addGap({ kind: 'unreadable', atSeq: from, detail: `seq ${from}–${String(u.to)} ${String(u.cause ?? '')}`.trimEnd() });
      }
      for (const ev of page.events ?? []) {
        const gen = (ev as { generation?: number | null }).generation ?? 0;
        if (o.range.kind === 'generation' && gen > o.range.generation) {
          progress('read');
          return;
        }
        const gap = tracker.observe(ev);
        if (gap) {
          addGap(gap);
          writer.note(`공백: ${gap.detail}`, wallMsOf(ev), String(ev.seq));
        }
        writer.record(ev);
        if (writer.capped) {
          inc('cap_bytes');
          progress('read');
          return;
        }
        s.events++;
        const wallMs = wallMsOf(ev);
        const seq = String(ev.seq);
        s.first ??= { seq, wallMs };
        s.last = { seq, wallMs };
        const g = (ev as { generation?: unknown }).generation;
        if (typeof g === 'number') gens.add(g);
        if (ev.kind === 'uart.data') {
          const p = ((ev as { payload?: unknown }).payload ?? {}) as Record<string, unknown>;
          const n = typeof p.len === 'number' ? p.len : (decodePayloadBytes(p.bytes)?.length ?? 0);
          if ((ev as { dir?: unknown }).dir === 'tx') s.txBytes += n;
          else s.rxBytes += n;
        }
      }
      progress('read');
      if (page.truncated && typeof page.next_cursor === 'string') since = page.next_cursor;
      else return;
    }
  };

  try {
    await work();
  } catch (e) {
    if (isAbort(e)) inc('cancelled');
    else {
      inc('query_failed');
      s.error = e instanceof Error ? e.message : String(e);
    }
  }
  writer.finish();
  s.generations = [...gens].sort((a, b) => a - b);
  if (s.gaps.length || s.gapsDropped) s.incomplete.unshift('gaps');
  s.incomplete = [...new Set(s.incomplete)];
  s.outBytes = writer.bytes;
  const text = exportHeader(s) + writer.body();
  progress('done');
  return { ...s, text, fileName: exportFileName(o.channel, o.format, o.nowMs) };
}

/** Blob download (`a[download]`), like Recorder.exportBuffer: `requested` only means the browser/app was asked. §B5.1 */
export function downloadText(name: string, text: string, format: ExportFormat): { requested: boolean; bytes: number } {
  const blob = new Blob([text], { type: format === 'jsonl' ? 'application/x-ndjson' : 'text/plain;charset=utf-8' });
  try {
    const a = document.createElement('a');
    if (!('download' in a)) return { requested: false, bytes: blob.size };
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
    return { requested: true, bytes: blob.size };
  } catch {
    return { requested: false, bytes: blob.size };
  }
}
