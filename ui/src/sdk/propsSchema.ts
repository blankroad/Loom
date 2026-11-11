// Panel props schema (plan §5.7: saved layouts' props are validated against the panel
// manifest's JSON Schema). A small JSON-Schema subset — bounded, no runtime dependency:
//   {type:'object', properties:{k:{type, enum?, pattern?, maxLength?, minimum?, maximum?}},
//    required?: string[]}
// Props arrive from URLs as strings, so `number`/`integer`/`boolean` also accept their string
// spelling. Unknown props are dropped (reported) — except `at` (deep-link cursor).
// Invalid values are dropped too (the panel then uses its default) and reported; a missing
// required prop makes the whole props set invalid.

import type { Props } from './manifest';

export interface PropSchema {
  type: 'string' | 'number' | 'integer' | 'boolean';
  enum?: readonly (string | number | boolean)[];
  pattern?: string;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
}

export interface PropsSchema {
  type: 'object';
  properties: Readonly<Record<string, PropSchema>>;
  required?: readonly string[];
}

export interface PropsCheck {
  ok: boolean;
  props: Props;
  problems: string[];
}

const MAX_KEYS = 32;
const MAX_STR = 256;
/** channel / target names (loom-proto RefString channel charset) */
export const CHANNEL_PROP: PropSchema = { type: 'string', pattern: '^[A-Za-z0-9_.-]{1,64}$' };
/** deep-link cursor, allowed on every panel */
const CURSOR_PROP: PropSchema = { type: 'string', pattern: '^(seq:\\d{1,20}|mark:[^\\s#]{1,64}|-\\d{1,6}(\\.\\d{1,3})?s)$' };

function coerce(v: unknown, s: PropSchema): { ok: true; value: string | number | boolean } | { ok: false; why: string } {
  switch (s.type) {
    case 'string': {
      if (typeof v !== 'string') return { ok: false, why: '문자열이 아님' };
      if (v.length > (s.maxLength ?? MAX_STR)) return { ok: false, why: `길이 > ${s.maxLength ?? MAX_STR}` };
      if (s.pattern && !new RegExp(s.pattern).test(v)) return { ok: false, why: '형식 불일치' };
      return { ok: true, value: v };
    }
    case 'number':
    case 'integer': {
      const n = typeof v === 'number' ? v : typeof v === 'string' && /^-?\d+(\.\d+)?$/.test(v) ? Number(v) : NaN;
      if (!Number.isFinite(n)) return { ok: false, why: '숫자가 아님' };
      if (s.type === 'integer' && !Number.isInteger(n)) return { ok: false, why: '정수가 아님' };
      if (s.minimum !== undefined && n < s.minimum) return { ok: false, why: `< ${s.minimum}` };
      if (s.maximum !== undefined && n > s.maximum) return { ok: false, why: `> ${s.maximum}` };
      return { ok: true, value: n };
    }
    case 'boolean': {
      if (typeof v === 'boolean') return { ok: true, value: v };
      if (v === 'true' || v === 'false') return { ok: true, value: v === 'true' };
      return { ok: false, why: 'true/false가 아님' };
    }
  }
}

/** schema absent → primitive values only (≤ 32 keys, strings ≤ 256 chars) */
export function validateProps(schema: PropsSchema | undefined, raw: unknown): PropsCheck {
  const problems: string[] = [];
  const out: Props = {};
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return { ok: schema?.required?.length ? false : true, props: {}, problems: raw === undefined ? [] : ['props가 객체가 아님'] };
  }
  const entries = Object.entries(raw as Record<string, unknown>);
  if (entries.length > MAX_KEYS) problems.push(`props ${entries.length}개 > ${MAX_KEYS} (뒤는 버림)`);
  for (const [k, v] of entries.slice(0, MAX_KEYS)) {
    const s = schema?.properties[k] ?? (k === 'at' ? CURSOR_PROP : undefined);
    if (!s) {
      if (schema) {
        problems.push(`모르는 prop "${k}" 버림`);
        continue;
      }
      if (typeof v === 'string' ? v.length <= MAX_STR : typeof v === 'number' || typeof v === 'boolean') out[k] = v;
      else problems.push(`prop "${k}" 값 형식이 원시값이 아님 — 버림`);
      continue;
    }
    if (s.enum && !s.enum.includes(v as string | number | boolean)) {
      problems.push(`prop "${k}" 허용 값이 아님 — 버림`);
      continue;
    }
    const c = coerce(v, s);
    if (c.ok) out[k] = c.value;
    else problems.push(`prop "${k}" ${c.why} — 버림`);
  }
  const missing = (schema?.required ?? []).filter((k) => !(k in out));
  for (const k of missing) problems.push(`필수 prop "${k}" 없음`);
  return { ok: missing.length === 0, props: out, problems };
}
