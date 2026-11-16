// Per-viewer UDS workbench preferences (selected web: uds-read-did/app config.js, catalog.js,
// workflow.js, project.js, layout.js). Only settings live here — connection presets, the DID
// catalogue, saved commands and layout. No history, no link state, no values: a reload never
// connects, sends, polls or retries. Every list is validated on load; corrupt data → defaults.
import { loadJson, saveJson } from '../../workbench/eth/bench';
import { laFromAddr } from './udsModel';

export const PREFS_KEY = 'loom.uds.workbench.v1';

/** Real backend: DoIP (`uds.connect{host,port,la,tester_la,activation}`) or the daemon's built-in
 *  simulated ECU (`uds.connect{sim:'bench'}`). CAN-TP / EthDiag are not connectable yet. */
export interface ConnPreset {
  id: string;
  /** UDS target alias used by uds.connect `name` */
  ecu: string;
  label: string;
  sim: boolean;
  host: string;
  port: number;
  /** ECU logical address, hex text `0x1001` */
  la: string;
  /** tester logical address, hex text `0x0E80` */
  testerLa: string;
  activation: number;
  timeoutMs: number;
  /** U1: TesterPresent interval for this ECU (500–30000 ms; absent → DEFAULT_TP_INTERVAL_MS, shown) */
  tpIntervalMs?: number;
  /** U2: loom.toml [[uds.key_providers]] name used by [자동 해제] (the provider itself is never in the UI) */
  keyProvider?: string;
  /** U2: requestSeed sub-function for [자동 해제] (odd 0x01–0x7D) */
  unlockLevel?: number;
}

/** U5: a diagnostic DB kept in the project file (attached per target with uds.db.set). */
export interface SavedDb {
  id: string;
  name: string;
  /** the loom_diag JSON text exactly as imported (≤ MAX_DB_TEXT_BYTES) */
  json: string;
}
export const MAX_SAVED_DBS = 8;
/** same limit as the daemon (crates/loom-uds/src/diagdb.rs MAX_INLINE_DB_BYTES) */
export const MAX_DB_TEXT_BYTES = 48 * 1024;
export const DEFAULT_TP_INTERVAL_MS = 2000;

export interface DidDef {
  did: string;
  name: string;
  note: string;
}

export interface SavedCmd {
  id: string;
  name: string;
  /** canonical hex without spaces */
  hex: string;
}

/** User sequence: ordered saved-command ids. Runs only on an explicit [실행]; one request at a time,
 *  each step waits for its final result; stops on the first failure / abort / lock / link loss; no retry. */
export interface SavedSeq {
  id: string;
  name: string;
  steps: string[];
}

export interface UdsPrefs {
  presets: ConnPreset[];
  dids: DidDef[];
  cmds: SavedCmd[];
  seqs: SavedSeq[];
  mode: 'did' | 'raw';
  did: string;
  raw: string;
  histPct: number;
  didCollapsed: boolean;
  valsCollapsed: boolean;
  histScope: 'all' | 'ecu';
  /** U5 (optional so older stored prefs stay valid) */
  diagDbs?: SavedDb[];
}

/** ISO 14229-1 Annex C standard identifiers (names only — values always come from the ECU). */
export const STANDARD_DIDS: readonly DidDef[] = [
  { did: 'F190', name: 'VIN', note: 'Vehicle Identification Number' },
  { did: 'F18C', name: 'ECU Serial Number', note: 'ECUSerialNumberDataIdentifier' },
  { did: 'F187', name: 'Spare Part Number', note: 'vehicleManufacturerSparePartNumber' },
  { did: 'F191', name: 'ECU Hardware Number', note: 'vehicleManufacturerECUHardwareNumber' },
  { did: 'F195', name: 'SW Version', note: 'systemSupplierECUSoftwareVersionNumber' },
  { did: 'F186', name: 'Active Session', note: 'ActiveDiagnosticSessionDataIdentifier' },
];

/** Defaults: the daemon's built-in simulated ECU (labelled SIM) and an empty DoIP entry for a real bench. */
export const DEFAULT_PRESETS: readonly ConnPreset[] = [
  { id: 'sim-bench', ecu: 'ecu1', label: 'SIM bench (가상 ECU)', sim: true, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 },
  { id: 'doip-1', ecu: 'ecu', label: 'DoIP ECU', sim: false, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 },
];

export const DEFAULT_PREFS: UdsPrefs = {
  presets: DEFAULT_PRESETS.map((p) => ({ ...p })),
  dids: STANDARD_DIDS.map((d) => ({ ...d })),
  cmds: [],
  seqs: [],
  mode: 'did',
  did: 'F190',
  raw: '',
  histPct: 42,
  didCollapsed: false,
  valsCollapsed: false,
  histScope: 'all',
};

const NAME = /^[A-Za-z0-9_.-]{1,64}$/;
const isStr = (v: unknown): v is string => typeof v === 'string';
export const parseLa = (v: string): number | null => {
  const t = v.trim();
  const n = /^0x[0-9a-f]{1,4}$/i.test(t) ? parseInt(t.slice(2), 16) : /^\d{1,5}$/.test(t) ? Number(t) : NaN;
  return Number.isInteger(n) && n >= 0 && n <= 0xffff ? n : null;
};

/** Validation messages for a preset (empty = valid). */
export function presetIssues(p: ConnPreset): string[] {
  const out: string[] = [];
  if (!NAME.test(p.ecu)) out.push('ECU 이름: 영문·숫자·_.- 1–64자');
  if (!p.sim && !p.host.trim()) out.push('DoIP 호스트(IP) 필요');
  if (!p.sim && !/^[A-Za-z0-9.:-]{1,253}$/.test(p.host.trim())) out.push('호스트 형식 오류');
  if (!Number.isInteger(p.port) || p.port < 1 || p.port > 65535) out.push('포트 1–65535');
  if (parseLa(p.la) === null) out.push('ECU 논리 주소 0x0000–0xFFFF');
  if (parseLa(p.testerLa) === null) out.push('Tester 논리 주소 0x0000–0xFFFF');
  if (!Number.isInteger(p.activation) || p.activation < 0 || p.activation > 255) out.push('활성화 타입 0–255');
  if (!Number.isInteger(p.timeoutMs) || p.timeoutMs < 100 || p.timeoutMs > 60000) out.push('연결 타임아웃 100–60000 ms');
  return out;
}

/** What the daemon reported for a target (uds.state backend/addr) — the only inputs a new profile may be prefilled from. */
export interface ReportedRoute {
  backend: string;
  addr: string | null;
}

/** FUNCTIONS.md B1-F9 (U7): new, unsaved local profile for a target that has none. Never copies another profile. */
export function profileDraftForTarget(ecu: string, reported: ReportedRoute | undefined, id: string): ConnPreset {
  return {
    id,
    ecu,
    label: `${ecu} (daemon 보고에서)`,
    sim: reported?.backend === 'sim',
    host: '',
    port: 13400,
    la: laFromAddr(reported?.addr ?? null) ?? '',
    testerLa: '0x0E80',
    activation: 0,
    timeoutMs: 5000,
  };
}

/** FUNCTIONS.md B4-P1: the preset's TesterPresent interval, or the default when absent/invalid. */
export function tpIntervalOf(p: ConnPreset | null): number {
  const v = p?.tpIntervalMs;
  return typeof v === 'number' && Number.isInteger(v) && v >= 500 && v <= 30000 ? v : DEFAULT_TP_INTERVAL_MS;
}

/** FUNCTIONS.md B4-P2 (U6 "재연결 필요"): the profile no longer matches what the up link was opened with. */
export function profileNeedsReconnect(p: ConnPreset, reported: ReportedRoute & { link: string }): boolean {
  if (reported.link !== 'up') return false;
  if (p.sim !== (reported.backend === 'sim')) return true;
  if (p.sim) return false;
  const la = laFromAddr(reported.addr);
  if (la === null) return false;
  return parseLa(p.la) !== parseInt(la.slice(2), 16);
}

/** FUNCTIONS.md B4-P3 (U5): local pre-check of a DB text before uds.db.set (full validation is the daemon's). */
export function checkDbText(text: string): { db: Record<string, unknown> | null; err: string | null; bytes: number } {
  const bytes = new TextEncoder().encode(text).length;
  if (bytes > MAX_DB_TEXT_BYTES) return { db: null, err: `DB가 너무 큼 (${bytes} B, 최대 ${MAX_DB_TEXT_BYTES} B)`, bytes };
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return { db: null, err: `JSON 구문 오류: ${e instanceof Error ? e.message : String(e)}`, bytes };
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return { db: null, err: 'loom_diag DB 객체가 아님', bytes };
  const db = parsed as Record<string, unknown>;
  if (db.loom_diag !== 1) return { db: null, err: 'loom_diag 1 형식이 아님', bytes };
  if ('dids' in db && db.dids !== undefined && !Array.isArray(db.dids)) return { db: null, err: 'dids: 배열이 아님', bytes };
  return { db, err: null, bytes };
}

/** uds.connect params (crates/loom-uds/src/handlers.rs `connect`). */
export function connectParams(p: ConnPreset): Record<string, unknown> {
  if (p.sim) return { name: p.ecu, sim: 'bench', timeout_ms: p.timeoutMs };
  return {
    name: p.ecu,
    host: p.host.trim(),
    port: p.port,
    la: parseLa(p.la),
    tester_la: parseLa(p.testerLa),
    activation: p.activation,
    timeout_ms: p.timeoutMs,
  };
}

const ID = /^[A-Za-z0-9_.-]{1,64}$/;
const int = (v: unknown, lo: number, hi: number): boolean => typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi;
/** Structural check only: a DoIP draft may still lack a host (presetIssues blocks its [연결]). */
function isPreset(v: unknown): v is ConnPreset {
  const o = v as Record<string, unknown> | null;
  return (
    !!o && isStr(o.id) && ID.test(o.id) && isStr(o.ecu) && NAME.test(o.ecu) && isStr(o.label) && o.label.length <= 80 &&
    typeof o.sim === 'boolean' && isStr(o.host) && o.host.length <= 253 && int(o.port, 1, 65535) &&
    isStr(o.la) && parseLa(o.la) !== null && isStr(o.testerLa) && parseLa(o.testerLa) !== null &&
    int(o.activation, 0, 255) && int(o.timeoutMs, 100, 60000)
  );
}
const isSeq = (v: unknown): v is SavedSeq => {
  const o = v as Record<string, unknown> | null;
  return !!o && isStr(o.id) && ID.test(o.id) && isStr(o.name) && o.name.trim().length > 0 && o.name.length <= 80 &&
    Array.isArray(o.steps) && o.steps.length <= 64 && o.steps.every((x) => isStr(x) && ID.test(x));
};
/** first entry per id wins */
const uniq = <T extends { id: string }>(xs: T[]): T[] => {
  const seen = new Set<string>();
  return xs.filter((x) => (seen.has(x.id) ? false : (seen.add(x.id), true)));
};
const isDid = (v: unknown): v is DidDef => {
  const o = v as Record<string, unknown> | null;
  return !!o && isStr(o.did) && /^[0-9A-F]{4}$/.test(o.did) && isStr(o.name) && o.name.length <= 80 && isStr(o.note) && o.note.length <= 200;
};
const isCmd = (v: unknown): v is SavedCmd => {
  const o = v as Record<string, unknown> | null;
  return !!o && isStr(o.id) && ID.test(o.id) && isStr(o.name) && o.name.trim().length > 0 && o.name.length <= 80 && isStr(o.hex) && /^([0-9A-F]{2}){1,4095}$/.test(o.hex);
};

/** Accepts a stored or imported object; drops invalid entries, falls back per field. */
export function sanitizePrefs(v: unknown): UdsPrefs {
  const o = (typeof v === 'object' && v !== null ? v : {}) as Record<string, unknown>;
  const presets = Array.isArray(o.presets) ? uniq(o.presets.filter(isPreset)).slice(0, 64) : null;
  const didSeen = new Set<string>();
  const dids = Array.isArray(o.dids) ? o.dids.filter(isDid).filter((d) => (didSeen.has(d.did) ? false : (didSeen.add(d.did), true))).slice(0, 512) : null;
  const cmds = Array.isArray(o.cmds) ? uniq(o.cmds.filter(isCmd)).slice(0, 200) : null;
  const cmdIds = new Set((cmds ?? []).map((c) => c.id));
  // a sequence naming a missing command is dropped whole (never run a partial plan)
  const seqs = Array.isArray(o.seqs) ? uniq(o.seqs.filter(isSeq)).filter((q) => q.steps.every((id) => cmdIds.has(id))).slice(0, 100) : null;
  const num = (x: unknown, d: number, lo: number, hi: number) => (typeof x === 'number' && x >= lo && x <= hi ? x : d);
  const cleanPreset = (p: ConnPreset): ConnPreset => {
    const c: ConnPreset = { ...p };
    if (!int(c.tpIntervalMs, 500, 30000)) delete c.tpIntervalMs;
    if (!(isStr(c.keyProvider) && NAME.test(c.keyProvider))) delete c.keyProvider;
    if (!(int(c.unlockLevel, 1, 0x7d) && (c.unlockLevel as number) % 2 === 1)) delete c.unlockLevel;
    return c;
  };
  const enc = new TextEncoder();
  const diagDbs = Array.isArray(o.diagDbs)
    ? uniq(
        (o.diagDbs as unknown[]).filter((x): x is SavedDb => {
          const d = x as Record<string, unknown> | null;
          return !!d && isStr(d.id) && ID.test(d.id) && isStr(d.name) && d.name.trim().length > 0 && d.name.length <= 80 &&
            isStr(d.json) && enc.encode(d.json).length <= MAX_DB_TEXT_BYTES;
        }),
      ).slice(0, MAX_SAVED_DBS)
    : [];
  return {
    presets: presets && presets.length ? presets.map(cleanPreset) : DEFAULT_PREFS.presets.map((p) => ({ ...p })),
    dids: dids ?? DEFAULT_PREFS.dids.map((d) => ({ ...d })),
    cmds: cmds ?? [],
    seqs: seqs ?? [],
    mode: o.mode === 'raw' ? 'raw' : 'did',
    did: isStr(o.did) ? o.did.slice(0, 12) : DEFAULT_PREFS.did,
    raw: isStr(o.raw) ? o.raw.slice(0, 12_000) : '',
    histPct: num(o.histPct, DEFAULT_PREFS.histPct, 15, 85),
    didCollapsed: o.didCollapsed === true,
    valsCollapsed: o.valsCollapsed === true,
    histScope: o.histScope === 'ecu' ? 'ecu' : 'all',
    ...(diagDbs.length ? { diagDbs: diagDbs.map((d) => ({ id: d.id, name: d.name, json: d.json })) } : {}),
  };
}

const anyValue = (v: unknown): v is unknown => v !== undefined;
export const loadPrefs = (): UdsPrefs => sanitizePrefs(loadJson<unknown>(PREFS_KEY, anyValue, null));
export const savePrefs = (p: UdsPrefs): void => saveJson(PREFS_KEY, p);

/** DID editor text → 4 hex digits (`0x`, spaces, `_` allowed). */
export function normalizeDid(text: string): { did: string | null; err: string | null } {
  const t = text.replace(/^0x/i, '').replace(/[\s_]/g, '').toUpperCase();
  if (!t) return { did: null, err: 'DID를 입력하세요 (4자리 hex, 예: F190)' };
  if (/^22[0-9A-F]{4}$/.test(t)) return { did: null, err: 'SID 22는 자동으로 붙습니다 — DID 4자리만 입력 (예: F190)' };
  if (!/^[0-9A-F]+$/.test(t)) return { did: null, err: 'hex 숫자만 (0-9, A-F)' };
  if (t.length !== 4) return { did: null, err: `DID는 정확히 4자리 hex (${t.length}자리 입력됨)` };
  return { did: t, err: null };
}

/** Raw editor text → canonical hex; whole-input reject (no partial send). */
export function normalizeRaw(text: string): { hex: string | null; err: string | null } {
  const toks = text.trim().split(/\s+/).filter(Boolean);
  if (!toks.length) return { hex: null, err: 'SID를 포함한 hex 바이트를 입력하세요 (예: 22 F1 90)' };
  if (/,/.test(text)) return { hex: null, err: '쉼표는 구분자로 쓰지 않습니다 — 공백으로 구분' };
  let hex = '';
  for (const t of toks) {
    if (/^0x/i.test(t)) {
      if (!/^0x[0-9a-f]{2}$/i.test(t)) return { hex: null, err: `0x 표기는 바이트마다 2자리: ${t}` };
      hex += t.slice(2);
    } else if (/^[0-9a-f]+$/i.test(t)) {
      if (t.length % 2) return { hex: null, err: `홀수 자릿수: ${t}` };
      hex += t;
    } else return { hex: null, err: `hex 아님: ${t}` };
  }
  if (hex.length / 2 > 4095) return { hex: null, err: '최대 4095 B' };
  return { hex: hex.toUpperCase(), err: null };
}

export const spaced = (hex: string): string => hex.match(/../g)?.join(' ') ?? hex;
