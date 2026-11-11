// CAN Trace send line parser/validator (can-ui U-CAN-6, plan §5.1.3).
//
// Accepted forms (cansend-like):
//   0x300#00            11-bit id (≤ 3 hex digits and ≤ 0x7FF)
//   300#01.02.03        bytes may be separated by '.', ' ' or nothing
//   18DAF110#02 10 03   29-bit: 8 hex digits, or any value > 0x7FF (≤ 0x1FFFFFFF)
//   x123#00             explicit 29-bit ('x' suffix/prefix as in the Trace id column)
//   fd 0x123#00 11 …    CAN FD (prefix tokens: fd, brs)
//   123##1 00 11 …      cansend FD form: '##' + flags nibble (bit0 = BRS)
// Validation: classic CAN length 0–8; FD length ∈ {0–8, 12, 16, 20, 24, 32, 48, 64};
// RTR ('R') is not sent from the UI in M1 (the structured can.send form has no rtr).
// Nothing here talks to the daemon — the result is the `can.send` params (CAN schema
// v1 §3, structured form `{channel, id, ext, fd, brs?, data}`; `frame` string form is not
// used so ext/brs are explicit), sent through ctx.act (ActionGate).
//   brs: FD → always explicit (daemon default is true; "fd 0x123#…" means BRS off);
//        Classic → omitted (giving it is E_USAGE).

export interface CanSendParams {
  channel: string;
  id: number;
  ext: boolean;
  fd: boolean;
  /** FD only (explicit true/false); absent for Classic */
  brs?: boolean;
  /** hex, lower case, no separators */
  data: string;
}

export type ParseResult = { ok: true; params: CanSendParams; canonical: string } | { ok: false; error: string };

export const FD_LENGTHS: ReadonlySet<number> = new Set([0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64]);
export const MAX_SFF_ID = 0x7ff;
export const MAX_EFF_ID = 0x1fffffff;
/** send-line input cap (characters) — longer input is rejected, never truncated */
export const MAX_LINE_CHARS = 256;

export function parseSendLine(line: string, channel: string): ParseResult {
  let s = line.trim();
  if (!s) return { ok: false, error: '입력 없음 — 예: 0x300#00' };
  if (s.length > MAX_LINE_CHARS) return { ok: false, error: `입력이 너무 깁니다 (≤ ${MAX_LINE_CHARS}자)` };

  let fd = false;
  let brs = false;
  // prefix tokens: fd, brs (any order)
  for (;;) {
    const m = /^(fd|brs)\s+/i.exec(s);
    if (!m) break;
    if (m[1]!.toLowerCase() === 'fd') fd = true;
    else brs = true;
    s = s.slice(m[0].length);
  }

  const hash = s.indexOf('#');
  if (hash < 0) return { ok: false, error: "'#'이 없습니다 — 형식 <id>#<data>, 예: 0x300#00" };
  let idPart = s.slice(0, hash).trim();
  let dataPart = s.slice(hash + 1);
  if (dataPart.startsWith('#')) {
    // cansend FD: <id>##<flags><data>
    fd = true;
    const flags = dataPart.slice(1, 2);
    if (!/^[0-9a-fA-F]$/.test(flags)) return { ok: false, error: "'##' 뒤에 FD 플래그 1자리(0–F)가 필요합니다" };
    if (parseInt(flags, 16) & 1) brs = true;
    dataPart = dataPart.slice(2);
  }
  if (/^\s*r/i.test(dataPart)) return { ok: false, error: 'RTR(원격 프레임)은 M1에서 지원하지 않습니다' };
  if (brs && !fd) return { ok: false, error: 'BRS는 FD 프레임에만 쓸 수 있습니다 (fd brs …)' };

  // id
  let forceExt = false;
  if (/^x/i.test(idPart)) {
    forceExt = true;
    idPart = idPart.slice(1);
  } else if (/x$/i.test(idPart) && !/^0x$/i.test(idPart)) {
    forceExt = true;
    idPart = idPart.slice(0, -1);
  }
  const digits = idPart.replace(/^0x/i, '');
  if (!/^[0-9a-fA-F]{1,8}$/.test(digits)) return { ok: false, error: `ID "${idPart}"가 16진수가 아닙니다 (1–8자리)` };
  const id = parseInt(digits, 16);
  const ext = forceExt || digits.length === 8 || id > MAX_SFF_ID;
  if (ext && id > MAX_EFF_ID) return { ok: false, error: `29bit ID 범위 초과 (≤ 0x1FFFFFFF): 0x${id.toString(16).toUpperCase()}` };

  // data
  const raw = dataPart.replace(/[\s.]/g, '');
  if (!/^[0-9a-fA-F]*$/.test(raw)) return { ok: false, error: '데이터에 16진수가 아닌 문자가 있습니다' };
  if (raw.length % 2 !== 0) return { ok: false, error: '데이터 자릿수가 홀수입니다 (바이트는 2자리)' };
  const len = raw.length / 2;
  if (!fd && len > 8) return { ok: false, error: `CAN(클래식) 데이터는 0–8바이트 (입력 ${len}) — FD면 앞에 fd` };
  if (fd && !FD_LENGTHS.has(len)) return { ok: false, error: `FD 길이 ${len}는 허용되지 않음 (0–8, 12, 16, 20, 24, 32, 48, 64)` };

  const data = raw.toLowerCase();
  const params: CanSendParams = { channel, id, ext, fd, data };
  if (fd) params.brs = brs;
  return { ok: true, params, canonical: formatFrame(id, ext, fd, brs, data) };
}

/** `0x300#00` / `18DAF110x#02 10 03` / `fd brs 0x123#…` — what the row echo shows */
export function formatFrame(id: number, ext: boolean, fd: boolean, brs: boolean, dataHex: string): string {
  const pre = `${fd ? 'fd ' : ''}${brs ? 'brs ' : ''}`;
  const bytes = dataHex.match(/../g)?.join(' ') ?? '';
  return `${pre}${fmtId(id, ext)}#${bytes}`;
}

/** Trace id column: 11-bit `0x300`, 29-bit `18DAF110x` (CAN plan: 29bit marked with x) */
export function fmtId(id: number, ext: boolean): string {
  return ext ? `${id.toString(16).toUpperCase().padStart(8, '0')}x` : `0x${id.toString(16).toUpperCase().padStart(3, '0')}`;
}
