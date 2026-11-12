// Bootstrap code → viewer cookie (contract 01 §1.3, plan §3.7).
//
// `loom ui` (TTY) opens `http://localhost:7420/ui/#boot=<code>[&next=<route>]`. The code
// is a one-time secret (60 s, single use) in the FRAGMENT so it never reaches server logs
// or a Referer. This module, before anything else runs:
//   1. reads it and IMMEDIATELY strips the fragment with history.replaceState (the address
//      bar and the history entry keep no secret — also on failure),
//   2. POSTs it to BOOTSTRAP_PATH (same origin, no-referrer, no-store) — the daemon answers
//      with Set-Cookie (HttpOnly, SameSite=Strict),
//   3. ok → the original deep link (`next`, or the one remembered before #/need-auth);
//      failure → `#/need-auth?reason=<expired|network|malformed>` (the reason is a fixed
//      word, never the code).
// The code is never logged, toasted, put in the DOM, or kept in a variable after use.
//
// Path: contract text says "페이지가 POST로 쿠키와 교환"; plan §3.7 draws `POST /bootstrap`.
// Lead's task uses `/api/v1/bootstrap` (same prefix as the STOP fallback) — asked CORE.

export const BOOTSTRAP_PATH = '/api/v1/bootstrap';
export const RETURN_KEY = 'loom.returnTo';
const CODE_RE = /^[A-Za-z0-9_-]{16,256}$/;
const ROUTE_RE = /^\/(p|l|s)\/[A-Za-z0-9_./?=&%:-]{1,512}$/;

export type BootReason = 'expired' | 'network' | 'malformed';
export type BootResult = { kind: 'none' } | { kind: 'ok'; next: string } | { kind: 'failed'; reason: BootReason };

export interface BootDeps {
  location: { hash: string; pathname: string; search: string };
  history: { replaceState(data: unknown, unused: string, url?: string): void };
  fetch(url: string, init: RequestInit): Promise<{ ok: boolean; status: number }>;
  /** sessionStorage-like; remembered deep link from before #/need-auth */
  session?: { getItem(k: string): string | null; removeItem(k: string): void } | null;
}

/** `#boot=...&next=...` → parts (null when the fragment is not a bootstrap) */
export function readBootFragment(hash: string): { code: string; next: string | null } | null {
  const h = hash.replace(/^#/, '');
  if (!h.startsWith('boot=')) return null;
  const q = new URLSearchParams(h);
  const code = q.get('boot') ?? '';
  const next = q.get('next');
  return { code, next: next && ROUTE_RE.test(next) ? next : null };
}

function takeReturnTo(d: BootDeps): string | null {
  try {
    const r = d.session?.getItem(RETURN_KEY) ?? null;
    d.session?.removeItem(RETURN_KEY);
    return r && ROUTE_RE.test(r) ? r : null;
  } catch {
    return null;
  }
}

export async function runBootstrap(d: BootDeps): Promise<BootResult> {
  const frag = readBootFragment(d.location.hash);
  if (!frag) return { kind: 'none' };
  // 1. strip the secret from the address bar / history entry before any await
  d.history.replaceState(null, '', d.location.pathname + d.location.search);
  if (!CODE_RE.test(frag.code)) return { kind: 'failed', reason: 'malformed' };
  let res: { ok: boolean; status: number };
  try {
    res = await d.fetch(BOOTSTRAP_PATH, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ code: frag.code }),
      credentials: 'same-origin',
      cache: 'no-store',
      referrerPolicy: 'no-referrer',
      redirect: 'error',
    });
  } catch {
    return { kind: 'failed', reason: 'network' };
  }
  if (!res.ok) return { kind: 'failed', reason: res.status >= 500 ? 'network' : 'expired' };
  return { kind: 'ok', next: frag.next ?? takeReturnTo(d) ?? '/l/m1-default' };
}

/** apply the result to the hash route (the caller then starts the connection) */
export function routeAfterBoot(r: BootResult, setHash: (h: string) => void): void {
  if (r.kind === 'ok') setHash(`#${r.next}`);
  else if (r.kind === 'failed') setHash(`#/need-auth?reason=${r.reason}`);
}

export const NEED_AUTH_TEXT: Record<BootReason | 'none', string> = {
  expired: '코드가 만료됐거나 이미 쓰였습니다 — 터미널에서 `loom ui`를 다시 실행하세요.',
  network: '데몬에 연결하지 못해 자격을 받지 못했습니다 — 데몬이 실행 중인지 확인한 뒤 터미널에서 `loom ui`를 다시 실행하세요.',
  malformed: '주소의 자격 코드 형식이 올바르지 않습니다 — 터미널에서 `loom ui`를 다시 실행하세요.',
  none: '',
};
