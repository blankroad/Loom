// Read-time "Decode As" rules for one channel (operator-upgrade B4b, finding ETH-06).
// A per-viewer interpretation preference (like Wireshark's Decode As): stored in this browser per channel,
// sent as the optional `decode_as` parameter of eth.query / eth.show / eth.export. It never changes the
// recorded packets or the live (worker-decoded) list. Contract: work/reports/operator-upgrade/eth/FUNCTIONS.md B4b.
import { loadJson, saveJson } from '../../workbench/eth/bench';

export const DECODE_AS_KEY = 'loom.eth.decodeas.v1';
/** rules per channel = daemon MAX_DECODE_AS_RULES */
export const MAX_DECODE_AS = 8;
/** channels remembered (oldest dropped) */
export const MAX_DECODE_AS_CHANNELS = 32;

export interface DecodeAsRule {
  transport: 'tcp' | 'udp';
  port: number;
  proto: 'doip' | 'someip';
}

/** F6.1 — valid, de-duplicated, conflict-free (first wins), ≤ MAX_DECODE_AS rules */
export function sanitizeRules(v: unknown): DecodeAsRule[] {
  if (!Array.isArray(v)) return [];
  const out: DecodeAsRule[] = [];
  for (const e of v as unknown[]) {
    if (out.length >= MAX_DECODE_AS) break;
    if (typeof e !== 'object' || e === null) continue;
    const o = e as Record<string, unknown>;
    if (o.transport !== 'tcp' && o.transport !== 'udp') continue;
    if (typeof o.port !== 'number' || !Number.isInteger(o.port) || o.port < 1 || o.port > 65535) continue;
    if (o.proto !== 'doip' && o.proto !== 'someip') continue;
    const same = out.find((r) => r.transport === o.transport && r.port === o.port);
    if (same) continue; // exact duplicate, or a conflict (first wins)
    out.push({ transport: o.transport, port: o.port, proto: o.proto });
  }
  return out;
}
/** F6.2 — one editor row → rule or a user-facing error */
export function parseRuleRow(transport: string, portText: string, proto: string): DecodeAsRule | { error: string } {
  const t = transport.trim();
  if (t !== 'tcp' && t !== 'udp') return { error: '전송 계층은 tcp 또는 udp' };
  const pt = portText.trim();
  const port = /^\d{1,5}$/.test(pt) ? Number(pt) : 0;
  if (port < 1 || port > 65535) return { error: '포트는 1–65535' };
  const p = proto.trim();
  if (p !== 'doip' && p !== 'someip') return { error: '프로토콜은 doip 또는 someip' };
  return { transport: t, port, proto: p };
}
/** F6.3 — editor rows → rules or the first error (with its 1-based row) */
export function validateRules(rows: readonly { transport: string; port: string; proto: string }[]): DecodeAsRule[] | { error: string } {
  if (rows.length > MAX_DECODE_AS) return { error: `규칙은 최대 ${MAX_DECODE_AS}개입니다` };
  const out: DecodeAsRule[] = [];
  for (const [i, row] of rows.entries()) {
    const n = i + 1;
    const r = parseRuleRow(row.transport, row.port, row.proto);
    if ('error' in r) return { error: `${n}번째 규칙: ${r.error}` };
    const same = out.find((x) => x.transport === r.transport && x.port === r.port);
    if (same) {
      if (same.proto === r.proto) continue;
      return { error: `${n}번째 규칙: 같은 포트(${r.transport} ${r.port})가 다른 프로토콜로 이미 있습니다` };
    }
    out.push(r);
  }
  return out;
}
/** F6.4 — rules stored for `ch` (sanitised; storage failures → []) */
export function loadRules(ch: string): DecodeAsRule[] {
  const map = loadJson<Record<string, unknown>>(DECODE_AS_KEY, (v): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v), {});
  return sanitizeRules(map[ch]);
}
/** F6.5 — store rules for `ch` (empty removes the channel); bounded to MAX_DECODE_AS_CHANNELS */
export function saveRules(ch: string, rules: readonly DecodeAsRule[]): void {
  const map = loadJson<Record<string, unknown>>(DECODE_AS_KEY, (v): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v), {});
  delete map[ch];
  const clean = sanitizeRules(rules);
  if (clean.length > 0) map[ch] = clean;
  for (let keys = Object.keys(map); keys.length > MAX_DECODE_AS_CHANNELS; keys = Object.keys(map)) delete map[keys[0] as string];
  saveJson(DECODE_AS_KEY, map);
}
/** F6.6 — `udp:64681=doip, tcp:30509=someip` */
export function rulesText(rules: readonly DecodeAsRule[]): string {
  return rules.map((r) => `${r.transport}:${r.port}=${r.proto}`).join(', ');
}
/** F6.7 — `detail` texts of `decode_as` warnings in an eth.show result (`warnings[]` of {code, layer, detail}) */
export function decodeAsNotes(show: unknown): string[] {
  const w = typeof show === 'object' && show !== null ? (show as Record<string, unknown>).warnings : null;
  if (!Array.isArray(w)) return [];
  const out: string[] = [];
  for (const e of w as unknown[]) {
    if (typeof e !== 'object' || e === null) continue;
    const o = e as Record<string, unknown>;
    if (o.code === 'decode_as' && typeof o.detail === 'string') out.push(o.detail);
  }
  return out;
}
