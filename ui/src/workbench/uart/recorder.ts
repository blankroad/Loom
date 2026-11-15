// REC — the REV4/REV6.1 live-save of one UART tab, fed with the ORIGINAL uart.data bytes from the daemon journal
// (before display, pause, clear or scrollback limits). The daemon journal itself is always recorded independently
// (REC health in the header); this is the user's own readable/exact export.
//
// Sinks (portable — nothing depends on one browser API):
//   'file'   Chromium File System Access (showSaveFilePicker): serialized writes, checkpoint (close → reopen
//            keepExistingData → seek) every 1 s (+1 s per 4 MiB, ≤ 10 s); Stop flushes, writes footer, closes.
//   'buffer' in-memory (≤ 5 MiB) for WebKit/WKWebView or any browser without FSA; exported with a Blob download
//            (`a[download]`). If a host blocks downloads, the export reports that it did not save — never success.
// Formats: '.log' readable lines (ANSI stripped, `YYYY-MM-DD HH:mm:ss.SSS RX|TX  line`, unterminated tails held
// until newline or Stop → `⟨줄바꿈 없음⟩`), '.jsonl' exact chunks {t, ms, dir, len, hex, text}.
// Timestamps are the journal's host receive time (ts.wall_ns), never written into the device stream.

export type RecFormat = 'log' | 'jsonl';
export type RecState = 'idle' | 'picking' | 'recording' | 'stopping' | 'closed' | 'error';
export interface RecPrefs { format: RecFormat; rx: boolean; tx: boolean; fileTs: boolean }
export const DEFAULT_REC_PREFS: RecPrefs = { format: 'log', rx: true, tx: true, fileTs: true };

const BUF_LIMIT = 5 * 1024 * 1024;
/** an unterminated .log line is written out in pieces of this size (bounded memory, no byte loss) */
export const PENDING_LINE_MAX = 64 * 1024;
const FILE_BACKLOG = 8 * 1024 * 1024;
const commitMs = (bytes: number) => Math.min(10_000, 1000 * Math.max(1, Math.ceil(bytes / (4 * 1024 * 1024))));
const pad = (n: number, w = 2) => String(n).padStart(w, '0');
export const fmtTs = (t: number) => { const d = new Date(t); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`; };
const fmtFull = (t: number) => { const d = new Date(t); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${fmtTs(t)}`; };
export function recFileName(alias: string, fmt: RecFormat, t = Date.now()): string {
  const d = new Date(t);
  return `${alias}-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.${fmt}`;
}
// eslint-disable-next-line no-control-regex -- ANSI/OSC escape sequences are control characters by definition
const stripAnsi = (x: string) => x.replace(/\x1b\[[0-9;?]*[A-Za-z~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][A-Z0-9]|\x1b[A-Za-z]/g, '');
const enc = new TextEncoder();
const toHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');

interface WritableLike { write(d: string | BufferSource | Blob): Promise<void>; seek?(n: number): Promise<void>; close(): Promise<void> }
interface FileHandleLike { name: string; createWritable(o?: { keepExistingData?: boolean }): Promise<WritableLike> }
type SavePicker = (o: { suggestedName: string; types?: { description: string; accept: Record<string, string[]> }[] }) => Promise<FileHandleLike>;
export const hasFilePicker = (): boolean => typeof (globalThis as { showSaveFilePicker?: unknown }).showSaveFilePicker === 'function';

export class Recorder {
  state: RecState = 'recording';
  readonly sink: 'file' | 'buffer';
  readonly prefs: RecPrefs;
  name: string;
  rxBytes = 0;
  txBytes = 0;
  /** bytes accepted for the sink (formatted) */
  queued = 0;
  /** bytes handed to the writer */
  written = 0;
  /** bytes durable in the file (after a checkpoint/close) or held in the buffer */
  committed = 0;
  lastSavedAt: number | null = null;
  error: string | null = null;
  reason: string | null = null;
  tail: string[] = [];
  private chunks: string[] = []; // buffer sink
  private bufBytes = 0;
  private pendingLine: Record<'rx' | 'tx', { text: string; t: number } | null> = { rx: null, tx: null };
  /** per-direction streaming decoders: a multi-byte character split across two chunks is never broken */
  private dec: Record<'rx' | 'tx', TextDecoder> = { rx: new TextDecoder('utf-8', { fatal: false }), tx: new TextDecoder('utf-8', { fatal: false }) };
  /** separate decoders for the .jsonl `text` convenience field (the exact bytes are `hex`) */
  private jdec: Record<'rx' | 'tx', TextDecoder> = { rx: new TextDecoder('utf-8', { fatal: false }), tx: new TextDecoder('utf-8', { fatal: false }) };
  private q: string[] = [];
  private qBytes = 0;
  private writing: Promise<void> = Promise.resolve();
  private w: WritableLike | null = null;
  private handle: FileHandleLike | null = null;
  private pos = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;

  private constructor(sink: 'file' | 'buffer', prefs: RecPrefs, name: string, private readonly alias: string, private readonly onChange: () => void) {
    this.sink = sink;
    this.prefs = { ...prefs, fileTs: prefs.format === 'jsonl' ? true : prefs.fileTs };
    this.name = name;
  }

  static buffer(alias: string, prefs: RecPrefs, onChange: () => void): Recorder {
    const r = new Recorder('buffer', prefs, `${recFileName(alias, prefs.format)} (버퍼)`, alias, onChange);
    r.header();
    return r;
  }
  /** null = the user cancelled the picker (nothing recorded) */
  static async file(alias: string, prefs: RecPrefs, onChange: () => void): Promise<Recorder | null> {
    const pick = (globalThis as { showSaveFilePicker?: SavePicker }).showSaveFilePicker;
    if (!pick) throw new Error('이 브라우저/앱에는 파일 저장 선택기(File System Access)가 없습니다 — 버퍼 기록 후 내보내기를 쓰세요');
    let handle: FileHandleLike;
    try {
      handle = await pick({ suggestedName: recFileName(alias, prefs.format), types: [{ description: prefs.format === 'jsonl' ? 'JSON Lines' : 'Log', accept: { [prefs.format === 'jsonl' ? 'application/x-ndjson' : 'text/plain']: [`.${prefs.format}`] } }] });
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return null;
      throw e;
    }
    const r = new Recorder('file', prefs, handle.name, alias, onChange);
    r.handle = handle;
    r.w = await handle.createWritable({ keepExistingData: false });
    r.header();
    r.schedule();
    return r;
  }

  private header(): void {
    const t = Date.now();
    if (this.prefs.format === 'jsonl') this.put(`${JSON.stringify({ type: 'start', t: new Date(t).toISOString(), ms: t, alias: this.alias, dirs: this.dirs() })}\n`);
    else this.put(`# Loom UART 기록 · ${this.alias} · 시작 ${fmtFull(t)} · ${this.dirs()} · 원본은 데몬 저널 (uart.data)\n`);
  }
  private dirs(): string {
    return [this.prefs.rx ? 'RX' : '', this.prefs.tx ? 'TX' : ''].filter(Boolean).join('+') || '없음';
  }

  /** raw journal chunk (wallMs = daemon host receive time) */
  feed(dir: 'rx' | 'tx', bytes: Uint8Array, wallMs: number): void {
    if (this.state !== 'recording') return;
    if ((dir === 'rx' && !this.prefs.rx) || (dir === 'tx' && !this.prefs.tx)) return;
    if (dir === 'rx') this.rxBytes += bytes.length;
    else this.txBytes += bytes.length;
    if (this.prefs.format === 'jsonl') {
      this.put(`${JSON.stringify({ t: new Date(wallMs).toISOString(), ms: wallMs, dir, len: bytes.length, hex: toHex(bytes), text: this.jdec[dir].decode(bytes, { stream: true }) })}\n`);
      return;
    }
    this.consume(dir, this.dec[dir].decode(bytes, { stream: true }), wallMs);
    this.onChange();
  }
  private consume(dir: 'rx' | 'tx', raw: string, wallMs: number): void {
    let cur = this.pendingLine[dir];
    const text = stripAnsi(raw);
    for (const ch of text) {
      if (this.state !== 'recording' && this.state !== 'stopping') break;
      if (ch === '\n') {
        this.line(dir, cur?.t ?? wallMs, cur?.text ?? '');
        cur = null;
      } else if (ch === '\r') continue;
      else if (ch === '\b' || ch === '\x7f') { if (cur) cur.text = cur.text.slice(0, -1); }
      else if (ch < ' ' && ch !== '\t') continue;
      else {
        if (!cur) cur = { text: '', t: wallMs };
        cur.text += ch;
        if (cur.text.length >= PENDING_LINE_MAX) {
          // a long stream without newline: write it out now, visibly marked — bounded memory, nothing dropped
          this.line(dir, cur.t, cur.text, ` ⟨줄바꿈 없이 ${PENDING_LINE_MAX / 1024} KiB — 다음 줄에 이어짐⟩`);
          cur = null;
        }
      }
    }
    this.pendingLine[dir] = cur;
  }
  private line(dir: 'rx' | 'tx', t: number, text: string, note = ''): void {
    const s = `${this.prefs.fileTs ? `${fmtFull(t)} ` : ''}${dir.toUpperCase()}  ${text}${note}`;
    this.tail.push(s);
    if (this.tail.length > 20) this.tail.shift();
    this.put(`${s}\n`);
  }
  note(text: string): void {
    if (this.state !== 'recording') return;
    const t = Date.now();
    this.put(this.prefs.format === 'jsonl' ? `${JSON.stringify({ type: 'note', t: new Date(t).toISOString(), ms: t, text })}\n` : `-- ${fmtFull(t)} ${text}\n`);
  }

  private put(s: string): void {
    const n = enc.encode(s).length;
    this.queued += n;
    if (this.sink === 'buffer') {
      if (this.bufBytes + n > BUF_LIMIT) return this.fail(`버퍼 한도 ${BUF_LIMIT / 1048576} MiB 초과 — 기록 중단 (지금까지 내용은 내보낼 수 있음)`);
      this.chunks.push(s);
      this.bufBytes += n;
      this.committed = this.bufBytes;
      this.written = this.bufBytes;
      return;
    }
    if (this.qBytes + n > FILE_BACKLOG) return this.fail('파일 쓰기가 밀려 8 MiB를 넘었습니다 — 기록 중단 (조용히 버리지 않음)');
    this.q.push(s);
    this.qBytes += n;
    this.drain();
  }
  private drain(): void {
    this.writing = this.writing.then(async () => {
      if (!this.w || !this.q.length) return;
      const data = this.q.join('');
      const n = this.qBytes;
      this.q = [];
      this.qBytes = 0;
      await this.w.write(data);
      this.pos += n;
      this.written += n;
    }).catch((e: unknown) => this.fail(`파일 쓰기 실패: ${e instanceof Error ? e.message : String(e)}`));
  }
  private schedule(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.checkpoint(), commitMs(this.pos));
  }
  /** make what was written durable: FSA commits on close() */
  private async checkpoint(): Promise<void> {
    if (this.sink !== 'file' || this.state !== 'recording' || !this.handle) return;
    this.writing = this.writing.then(async () => {
      if (!this.w || !this.handle) return;
      await this.w.close();
      this.committed = this.written;
      this.lastSavedAt = Date.now();
      this.w = await this.handle.createWritable({ keepExistingData: true });
      if (this.w.seek) await this.w.seek(this.pos);
    }).catch((e: unknown) => this.fail(`파일 저장(체크포인트) 실패: ${e instanceof Error ? e.message : String(e)}`));
    await this.writing;
    this.onChange();
    if (this.state === 'recording') this.schedule();
  }
  private fail(msg: string): void {
    if (this.state === 'error') return;
    this.error = msg;
    this.state = 'error';
    if (this.timer) clearTimeout(this.timer);
    this.onChange();
  }

  get pending(): number {
    return Math.max(0, this.queued - this.committed);
  }

  async stop(reason: string | null = null): Promise<void> {
    if (this.state !== 'recording' && this.state !== 'error') return;
    const wasError = this.state === 'error';
    this.reason = reason;
    if (!wasError) {
      for (const dir of ['rx', 'tx'] as const) {
        // flush a trailing partial multi-byte sequence (shown as U+FFFD, never silently dropped)
        if (this.prefs.format === 'log') {
          const tail = this.dec[dir].decode();
          if (tail) this.consume(dir, tail, Date.now());
        }
        const p = this.pendingLine[dir];
        if (p && this.prefs.format === 'log') this.line(dir, p.t, p.text, ' ⟨줄바꿈 없음⟩');
        this.pendingLine[dir] = null;
      }
      const t = Date.now();
      this.put(this.prefs.format === 'jsonl' ? `${JSON.stringify({ type: 'stop', t: new Date(t).toISOString(), ms: t, reason, rx: this.rxBytes, tx: this.txBytes })}\n` : `# 기록 종료 ${fmtFull(t)}${reason ? ` · ${reason}` : ''} · RX ${this.rxBytes} B · TX ${this.txBytes} B\n`);
    }
    this.state = 'stopping';
    this.onChange();
    if (this.timer) clearTimeout(this.timer);
    if (this.sink === 'file') {
      this.drain();
      this.writing = this.writing.then(async () => {
        if (this.w) await this.w.close();
        this.w = null;
        this.committed = this.written;
        this.lastSavedAt = Date.now();
      }).catch((e: unknown) => { this.error = `파일 닫기 실패: ${e instanceof Error ? e.message : String(e)}`; });
      await this.writing;
    }
    this.state = this.error ? 'error' : 'closed';
    this.onChange();
  }

  /** buffer sink: requests a Blob download. `requested` only means the browser/app was asked to download —
   * completion is the host's (WKWebView may need the desktop download handler); the UI says so. */
  exportBuffer(): { requested: boolean; name: string; bytes: number } {
    const name = recFileName(this.alias, this.prefs.format);
    const blob = new Blob(this.chunks, { type: this.prefs.format === 'jsonl' ? 'application/x-ndjson' : 'text/plain;charset=utf-8' });
    try {
      const a = document.createElement('a');
      if (!('download' in a)) return { requested: false, name, bytes: blob.size };
      a.href = URL.createObjectURL(blob);
      a.download = name;
      document.body.append(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
      return { requested: true, name, bytes: blob.size };
    } catch {
      return { requested: false, name, bytes: blob.size };
    }
  }
  bufferText(): string {
    return this.chunks.join('');
  }
}
