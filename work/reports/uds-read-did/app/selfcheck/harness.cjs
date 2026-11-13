// Shared jsdom boot for source-local checks (NOT browser acceptance).
const path = require('path'), fs = require('fs');
const DIR = path.resolve(__dirname, '..');
const { JSDOM } = require(path.resolve(DIR, '../../../../ui/node_modules/jsdom'));
const html = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8').replace(/<script src="[^"]+"><\/script>/g, '');
// Scripts in index.html order (core host + feature modules); files not yet delivered by their owner are skipped.
const ALL = [...fs.readFileSync(path.join(DIR, 'index.html'), 'utf8').matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
const FILES = ALL.filter(f => fs.existsSync(path.join(DIR, f)));
const SRC = FILES.map(f => fs.readFileSync(path.join(DIR, f), 'utf8'));
function fakeClock(start) {
  let t = start || Date.UTC(2026, 10, 14, 3, 0, 0), id = 0; const q = [];
  return {
    now: () => t,
    set(fn, ms) { const h = ++id; q.push({ h, at: t + Math.max(0, ms), fn }); return h; },
    clear(h) { const i = q.findIndex(x => x.h === h); if (i >= 0) q.splice(i, 1); },
    advance(ms) { const end = t + ms; for (;;) { q.sort((a, b) => a.at - b.at || a.h - b.h); const n = q[0]; if (!n || n.at > end) break; q.shift(); t = n.at; n.fn(); } t = end; },
    pending: () => q.length,
  };
}
function boot(storage) {
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:4180/uds-read-did/app/index.html', runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window; const clock = fakeClock(); w.__loomUdsClock = clock;
  if (storage) for (const [k, v] of Object.entries(storage)) w.localStorage.setItem(k, v);
  w.localStorage.setItem('unrelated.key', 'keep');
  w.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  w.Element.prototype.scrollIntoView = function () {};
  w.URL.createObjectURL = () => 'blob:x'; w.URL.revokeObjectURL = () => {};
  w.HTMLAnchorElement.prototype.click = function () { w.__dl = (w.__dl || []).concat([this.download]); };
  w.requestAnimationFrame = fn => setTimeout(fn, 0);
  SRC.forEach(s => w.eval(s));
  return { w, clock, d: w.document, o: w.loomUdsOpus, U: w.LoomUds };
}
module.exports = { boot, fakeClock, DIR, FILES, ALL };
