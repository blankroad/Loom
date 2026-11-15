// Page probes shared by the native WK driver (wk.mjs) and the Ego/Chromium side of the parity check,
// so both browsers run byte-identical JS. Each is a function BODY string (WebDriver execute/async
// convention: the last argument is the completion callback). Credential hygiene: never returns the
// URL fragment, cookies, storage or page source; only asset paths, sizes and digests.

// SHA-256 of every same-origin /ui/ asset the page actually loaded (index.html + performance entries).
// Equal results in Ego and WK prove both render the same embedded UI build.
export const ASSET_DIGESTS = `
const done = arguments[arguments.length - 1];
(async () => {
  const paths = new Set(['/ui/index.html']);
  for (const e of performance.getEntriesByType('resource')) {
    try { const u = new URL(e.name); if (u.origin === location.origin && u.pathname.startsWith('/ui/')) paths.add(u.pathname); } catch {}
  }
  const out = {};
  for (const p of [...paths].sort()) {
    const r = await fetch(p, { cache: 'no-store', credentials: 'omit' });
    const b = new Uint8Array(await r.arrayBuffer());
    const h = [...new Uint8Array(await crypto.subtle.digest('SHA-256', b))].map((x) => x.toString(16).padStart(2, '0')).join('');
    out[p] = { status: r.status, bytes: b.length, sha256: h };
  }
  done({ ok: true, inner: [innerWidth, innerHeight], dpr: devicePixelRatio, assets: out });
})().catch((e) => done({ ok: false, error: String(e && e.message || e) }));
`;
