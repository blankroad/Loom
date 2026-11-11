// Evidence / reveal references (contract 07 §7.2, loom-proto `RefString`).
//   canonical:        `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]`
//   epoch-less input: `<domain>:<channel>@seq:<a>[..<b>][#<json-pointer>]`
//   Loom-local:       `<domain>:<channel>@req:<request_id>` — link chips that know a
//                     request_id but no seq (eth ↔UDS). `channel` may be `*`.
// Channel charset follows loom-proto: [A-Za-z0-9_.-]{1,64} (plus `*` for @req).
// Length ≤ 512 like the Rust parser. JSON Pointer: "" or "/…" with ~0/~1 escapes only.

export interface EvidenceRef {
  domain: string;
  channel: string;
  /** stream epoch when given (canonical form); null = bind from the session */
  epoch: bigint | null;
  seqA: bigint | null;
  seqB: bigint | null;
  requestId: string | null;
  pointer: string | null;
}

const MAX_LEN = 512;
const DOMAIN = '([a-z][a-z0-9_]*)';
const CHANNEL = '([A-Za-z0-9_.-]{1,64})';
const SEQ_RE = new RegExp(`^${DOMAIN}:${CHANNEL}@(?:e(\\d{1,20}):)?seq:(\\d{1,20})(?:\\.\\.(\\d{1,20}))?$`);
const REQ_RE = new RegExp(`^${DOMAIN}:([A-Za-z0-9_.-]{1,64}|\\*)@req:([A-Za-z0-9_.:-]{1,128})$`);

function validPointer(p: string): boolean {
  if (p === '') return true;
  if (!p.startsWith('/')) return false;
  return !/~(?![01])/.test(p);
}

export function parseEvidenceRef(ref: string): EvidenceRef | null {
  if (ref.length > MAX_LEN) return null;
  const hash = ref.indexOf('#');
  const body = hash >= 0 ? ref.slice(0, hash) : ref;
  const pointer = hash >= 0 ? ref.slice(hash + 1) : null;
  if (pointer !== null && !validPointer(pointer)) return null;

  const s = SEQ_RE.exec(body);
  if (s) {
    const a = BigInt(s[4] as string);
    const b = s[5] ? BigInt(s[5]) : null;
    if (b !== null && b < a) return null; // inverted range
    return { domain: s[1] as string, channel: s[2] as string, epoch: s[3] ? BigInt(s[3]) : null, seqA: a, seqB: b, requestId: null, pointer };
  }
  if (pointer === null) {
    const r = REQ_RE.exec(body);
    if (r) return { domain: r[1] as string, channel: r[2] as string, epoch: null, seqA: null, seqB: null, requestId: r[3] as string, pointer: null };
  }
  return null;
}

export function formatRequestRef(domain: string, channel: string | null, requestId: string): string {
  return `${domain}:${channel ?? '*'}@req:${requestId}`;
}

export function formatSeqRef(domain: string, channel: string, epoch: bigint | null, a: bigint, b: bigint | null = null): string {
  return `${domain}:${channel}@${epoch !== null ? `e${epoch}:` : ''}seq:${a}${b !== null && b !== a ? `..${b}` : ''}`;
}
