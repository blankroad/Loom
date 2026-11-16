// CAN DBC in the panel (CAN-5): DB chip, message names for the Receive view, decoded-signal text. Pure.
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B5-DB.
//
// · `can.db.info{channel}` → chip (`DB: 없음` | `DB: <file> · <sha8>`); unknown (read failed) is "DB: ?", never "없음".
// · `can.db.catalog` pages → id → message name, bounded (≤ DB_NAMES_MAX names, ≤ DB_PAGES_MAX pages); a cut is
//   reported as incomplete, never as "no name".
// · Decoding is on demand (one `can.trace.query{since=until=seq, max:1, decode:true}`), never per frame.
// · DBC text (file, message, signal, unit, label) is untrusted → the view uses textContent only.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-5 work.

/** message names kept per DB */
export const DB_NAMES_MAX = 4_096;
/** catalog pages read per DB (each ≤ DB_PAGE_SIZE messages) */
export const DB_PAGES_MAX = 20;
/** = crates/loom-can signals.rs CATALOG_MAX_MESSAGES */
export const DB_PAGE_SIZE = 1_000;
/** DBC path length bound for the load box */
export const DB_PATH_MAX = 4_096;
export const DB_ENCODINGS = ['auto', 'utf8', 'windows1252'] as const;
export type DbEncoding = (typeof DB_ENCODINGS)[number];

export interface DbInfo {
  loaded: boolean;
  path: string | null;
  sha: string | null;
  messages: number | null;
  encoding: string | null;
  encodingGuessed: boolean;
  warningsTotal: number;
  blockedTotal: number;
}

export interface DbChip {
  text: string;
  title: string;
  /** 'ct-db-none' | 'ct-db-ok' | 'ct-db-warn' | 'ct-db-unknown' */
  cls: string;
}

/** `can.db.info{channel}` result → that channel's info; null when the shape is unexpected */
export function parseDbInfo(v: unknown, channel: string): DbInfo | null {
  const count = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
  if (typeof v !== 'object' || v === null) return null;
  const arr = (v as { channels?: unknown }).channels;
  if (!Array.isArray(arr)) return null;
  const e = (arr as unknown[]).find((x): x is Record<string, unknown> => typeof x === 'object' && x !== null && (x as Record<string, unknown>).channel === channel);
  if (!e || typeof e.loaded !== 'boolean') return null;
  if (!e.loaded) {
    return { loaded: false, path: null, sha: null, messages: null, encoding: null, encodingGuessed: false, warningsTotal: 0, blockedTotal: 0 };
  }
  return {
    loaded: true,
    path: typeof e.path === 'string' ? e.path : null,
    sha: typeof e.db_sha256 === 'string' ? e.db_sha256 : null,
    messages: count(e.messages) ? e.messages : null,
    encoding: typeof e.encoding === 'string' ? e.encoding : null,
    encodingGuessed: e.encoding_guessed === true,
    warningsTotal: count(e.warnings_total) ? e.warnings_total : 0,
    blockedTotal: count(e.blocked_total) ? e.blocked_total : 0,
  };
}

/** chip for the top bar; `info === null` = unknown (read failed / not read yet) */
export function dbChip(info: DbInfo | null): DbChip {
  const basename = (p: string): string => p.split(/[\\/]/).filter(Boolean).pop() ?? '?';
  if (info === null) return { text: 'DB: ?', title: 'DB 상태를 읽지 못함 — 눌러서 다시 읽기·불러오기', cls: 'ct-db-unknown' };
  if (!info.loaded) return { text: 'DB: 없음', title: '이 채널에 DBC 없음 — 눌러서 데몬 컴퓨터 경로의 DBC 불러오기', cls: 'ct-db-none' };
  const { path, sha, messages, encoding, encodingGuessed, warningsTotal, blockedTotal } = info;
  let title = `${path ?? '?'}\nsha256 ${sha ?? '?'} · 메시지 ${messages ?? '?'}개 · 인코딩 ${encoding ?? '?'}${encodingGuessed ? ' (추정)' : ''}`;
  if (warningsTotal > 0) title += `\n경고 ${warningsTotal}`;
  if (blockedTotal > 0) title += `\n디코드/인코드 막힘 ${blockedTotal}개`;
  return {
    text: `DB: ${path ? basename(path) : '?'} · ${sha ? sha.slice(0, 8) : '?'}`,
    title,
    cls: warningsTotal > 0 || blockedTotal > 0 || encodingGuessed ? 'ct-db-warn' : 'ct-db-ok',
  };
}

/** load box input → `can.db.load` params, or a user-facing error */
export function dbLoadParams(path: string, encoding: DbEncoding, channel: string): { ok: true; params: Record<string, unknown> } | { ok: false; error: string } {
  const t = path.trim();
  if (t === '') return { ok: false, error: '데몬 컴퓨터의 DBC 절대 경로를 입력하세요' };
  if (t.length > DB_PATH_MAX) return { ok: false, error: '경로가 너무 깁니다 (≤ 4096자)' };
  if (!(t.startsWith('/') || /^[A-Za-z]:[\\/]/.test(t))) return { ok: false, error: '경로는 데몬 컴퓨터의 절대 경로여야 합니다' };
  return { ok: true, params: { channel, path: t, ...(encoding !== 'auto' ? { encoding } : {}) } };
}

/** id → message name for one DB (sha); bounded */
export class DbNames {
  /** sha of the DB these names belong to (null = none) */
  sha: string | null = null;
  /** pages read so far */
  pages = 0;
  /** every catalog page was read (no next_cursor) within the bounds */
  complete = false;
  /** a bound (names/pages) cut the catalog short */
  cut = false;
  /** bumps when names change (view redraw key) */
  version = 0;
  /** key `${ext ? 'x' : 's'}${id}` → name */
  private readonly names = new Map<string, string>();

  /** forget everything and bind to `sha` (null = no DB) */
  reset(sha: string | null): void {
    this.sha = sha;
    this.names.clear();
    this.pages = 0;
    this.complete = false;
    this.cut = false;
    this.version++;
  }

  /** one `can.db.catalog` page → names; returns the next cursor to read, or null when done/cut/mismatch */
  addPage(v: unknown): number | null {
    const count = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
    if (typeof v !== 'object' || v === null) return null;
    const o = v as Record<string, unknown>;
    if (typeof o.db_sha256 !== 'string' || o.db_sha256 !== this.sha || !Array.isArray(o.messages)) return null;
    for (const m of o.messages as unknown[]) {
      if (typeof m !== 'object' || m === null) continue;
      const r = m as Record<string, unknown>;
      if (typeof r.name !== 'string' || !count(r.id_raw) || typeof r.ext !== 'boolean') continue;
      if (this.names.size >= DB_NAMES_MAX) {
        this.cut = true;
        break;
      }
      this.names.set(`${r.ext ? 'x' : 's'}${r.id_raw}`, r.name);
    }
    this.pages++;
    this.version++;
    if (this.cut) return null;
    const next = count(o.next_cursor) ? o.next_cursor : null;
    if (next === null) {
      this.complete = true;
      return null;
    }
    if (this.pages >= DB_PAGES_MAX) {
      this.cut = true;
      return null;
    }
    return next;
  }

  /** message name for a frame id; null = not in the DB (or not read) */
  nameOf(id: number, ext: boolean): string | null {
    return this.names.get(`${ext ? 'x' : 's'}${id}`) ?? null;
  }
}

/** a trace/last row's `decoded` value → display lines */
export function decodedLines(decoded: unknown): string[] {
  if (decoded === undefined || decoded === null) return ['DB에 없는 id — 디코드할 메시지 없음'];
  const unknown = ['디코드 결과 형식을 알 수 없음'];
  if (typeof decoded !== 'object') return unknown;
  const d = decoded as Record<string, unknown>;
  if (typeof d.error === 'string') return [`디코드 실패${typeof d.message === 'string' ? ` (${d.message})` : ''}: ${d.error}`];
  if (typeof d.message === 'string' && Array.isArray(d.signals)) {
    let head = d.message;
    const lm = d.length_mismatch;
    if (typeof lm === 'object' && lm !== null) {
      const l = lm as Record<string, unknown>;
      if (typeof l.expected === 'number' && typeof l.actual === 'number') head += ` · 길이 불일치 (DBC ${l.expected} B, 프레임 ${l.actual} B)`;
    }
    if (d.fd_mismatch === true) head += ' · FD 불일치';
    const lines = [head];
    for (const sg of d.signals as unknown[]) {
      if (typeof sg !== 'object' || sg === null) continue;
      const g = sg as Record<string, unknown>;
      if (typeof g.name !== 'string') continue;
      if (g.unavailable === 'short_frame') {
        lines.push(`  ${g.name}: 값 없음 (짧은 프레임 — 필요 ${String(g.needed)} B, 실제 ${String(g.actual)} B)`);
      } else if (typeof g.unavailable === 'string') {
        lines.push(`  ${g.name}: 값 없음 (${g.unavailable})`);
      } else {
        const unit = typeof g.unit === 'string' ? g.unit : '';
        lines.push(`  ${g.name} = ${String(g.value)}${unit ? ` ${unit}` : ''}${typeof g.label === 'string' ? ` (${g.label})` : ''} · raw ${String(g.raw)}`);
      }
    }
    return lines;
  }
  return unknown;
}
