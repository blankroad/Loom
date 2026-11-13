// Layout source-local checks (worker B, item 6): jsdom (no real geometry — clamps/aria/classes/persistence only).
// NOT browser acceptance. Run: node work/reports/uds-read-did/app/selfcheck/layout-check.cjs
const path = require('path'), fs = require('fs');
const { boot, DIR } = require('./harness.cjs');
const results = []; let pass = 0, fail = 0;
function ok(id, cond, note) { results.push({ id, pass: !!cond, note: note || '' }); if (cond) pass++; else { fail++; console.log('FAIL', id, note || ''); } }
const KEY = 'loom.udsReadDid.layout.v1';
const wins = [];
const setW = (w, px) => Object.defineProperty(w, 'innerWidth', { configurable: true, value: px });
const key = (w, el, k, o) => el.dispatchEvent(new w.KeyboardEvent('keydown', Object.assign({ key: k, bubbles: true }, o || {})));

try {
  /* A — module, handles, mount */
  const b = boot(); wins.push(b.w); const { w, d } = b; const Lo = w.LoomUdsLayout;
  setW(w, 1440); Lo.apply();
  ok('A1 module boots, 4 separators in #wsp/#detail', Lo && d.querySelectorAll('#wsp > .lo-h').length === 2 && d.querySelectorAll('#detail > .lo-h').length === 2);
  ok('A2 separators have role/orientation/aria/tabindex', [...d.querySelectorAll('.lo-h')].every(h => h.getAttribute('role') === 'separator' && h.getAttribute('aria-orientation') === 'vertical' && h.tabIndex === 0 && h.getAttribute('aria-valuenow') && h.getAttribute('aria-label')));
  ok('A3 controls in #layoutToolsMount only', d.querySelectorAll('#layoutToolsMount #loMaxHist, #layoutToolsMount #loMaxDet, #layoutToolsMount #loMenuBtn').length === 3);
  ok('A4 core divider untouched', d.getElementById('divHist') && d.getElementById('divHist').getAttribute('aria-orientation') === 'horizontal');

  /* B — widths, clamps, keyboard */
  Lo.setDim('didW', 300);
  ok('B1 DID width → own class + var', d.getElementById('wsp').classList.contains('lo-did') && d.getElementById('wsp').style.getPropertyValue('--lo-did') === '300px' && Lo.cur().didW === 300);
  Lo.setDim('didW', 10); ok('B2 min clamp', Lo.cur().didW === 160);
  Lo.setDim('didW', 5000); ok('B3 max clamp', Lo.cur().didW === 420);
  Lo.setDim('valsW', 5000); const bd = Lo.bounds();
  ok('B4 centre min kept (vals clamped by room)', Lo.cur().valsW <= bd.valsMax && Lo.cur().valsW <= 480 && (1440 - 24 - 12 - Lo.cur().didW - Lo.cur().valsW) >= 480 - 12);
  Lo.setDim('seqPct', 45); Lo.setDim('bytesPct', 45);
  ok('B5 detail middle ≥ 25%', Lo.cur().seqPct + Lo.cur().bytesPct <= 75 && d.getElementById('detail').style.getPropertyValue('--lo-bytes') === Lo.cur().bytesPct + '%');
  Lo.setDim('didW', 216);
  key(w, Lo.handles.hDid, 'ArrowRight'); ok('B6 ArrowRight widens DID by 16', Lo.cur().didW === 232 && Lo.handles.hDid.getAttribute('aria-valuenow') === '232');
  key(w, Lo.handles.hDid, 'ArrowLeft', { shiftKey: true }); ok('B7 Shift+ArrowLeft −64', Lo.cur().didW === 168);
  key(w, Lo.handles.hVals, 'ArrowRight'); const v1 = Lo.cur().valsW; key(w, Lo.handles.hVals, 'ArrowLeft');
  ok('B8 vals handle direction (Left widens)', Lo.cur().valsW === v1 + 16);
  key(w, Lo.handles.hDid, 'Home'); ok('B9 Home resets that dimension only', Lo.cur().didW == null && !d.getElementById('wsp').classList.contains('lo-did') && Lo.cur().valsW != null);
  ok('B10 persisted', JSON.parse(w.localStorage.getItem(KEY)).cur.valsW === Lo.cur().valsW);

  /* C — maximize / restore */
  Lo.setMax('hist');
  ok('C1 maximize history class + pressed', d.getElementById('wsp').classList.contains('lo-max-hist') && d.getElementById('loMaxHist').getAttribute('aria-pressed') === 'true' && d.getElementById('loMaxHist').textContent === '기록 복원');
  ok('C2 handles hidden while maximized', Lo.handles.hDid.hidden && Lo.handles.hVals.hidden && Lo.handles.hSeq.hidden);
  d.getElementById('loMaxDet').click();
  ok('C3 switch to detail max', d.getElementById('wsp').classList.contains('lo-max-detail') && !d.getElementById('wsp').classList.contains('lo-max-hist'));
  key(w, d.getElementById('toolDrawer'), 'Escape');
  ok('C4 Esc inside tool drawer does not restore layout', Lo.cur().max === 'detail');
  key(w, d.body, 'Escape');
  ok('C5 Esc restores', Lo.cur().max == null && !d.getElementById('wsp').className.includes('lo-max'));
  d.getElementById('loMaxHist').click(); d.getElementById('loMaxHist').click();
  ok('C6 button toggles restore', Lo.cur().max == null);

  /* D — collapse / breakpoints */
  d.querySelector('[data-cmd="collapse-did"]').click(); Lo.place();
  ok('D1 DID collapsed → DID handle hidden; collapsed rail width not overridden', Lo.handles.hDid.hidden && d.getElementById('wsp').classList.contains('did-collapsed'));
  d.querySelector('[data-cmd="expand-did"]').click(); Lo.place();
  ok('D2 expand → DID handle visible', !Lo.handles.hDid.hidden);
  setW(w, 1000); Lo.apply();
  ok('D3 ≤1180: vals/detail handles hidden, DID kept', Lo.handles.hVals.hidden && Lo.handles.hSeq.hidden && !Lo.handles.hDid.hidden);
  setW(w, 768); Lo.apply();
  ok('D4 768: DID handle hidden', Lo.handles.hDid.hidden);
  setW(w, 390); Lo.setMax('hist');
  ok('D5 390: maximize ignored (single stack), buttons disabled', !d.getElementById('wsp').className.includes('lo-max') && d.getElementById('loMaxHist').disabled && [...d.querySelectorAll('.lo-h')].every(h => h.hidden));
  Lo.setMax(null); setW(w, 1440); Lo.apply();

  /* E — reset / named layouts */
  Lo.setDim('didW', 300); Lo.setDim('seqPct', 20);
  ok('E1 save named', Lo.saveNamed('넓은 DID') && Lo.get().named.length === 1);
  Lo.reset();
  ok('E2 reset → defaults, named kept', JSON.stringify(Lo.cur()) === JSON.stringify({ didW: null, valsW: null, seqPct: null, bytesPct: null, max: null }) && Lo.get().named.length === 1);
  ok('E3 load named', Lo.loadNamed(0) && Lo.cur().didW === 300 && Lo.cur().seqPct === 20);
  ok('E4 overwrite same name', Lo.saveNamed('넓은 DID') && Lo.get().named.length === 1);
  ok('E5 name validation', !Lo.saveNamed('') && !Lo.saveNamed('x'.repeat(33)));
  for (let i = 0; i < 11; i++) Lo.saveNamed('L' + i);
  ok('E6 cap 12', Lo.get().named.length === 12 && !Lo.saveNamed('over'));
  Lo.togglePop(true);
  ok('E7 popover lists names escaped', d.querySelectorAll('#loPop .lo-list li').length === 12 && d.getElementById('loMenuBtn').getAttribute('aria-expanded') === 'true');
  key(w, d.getElementById('loPop'), 'Escape');
  ok('E8 Esc closes popover', d.getElementById('loPop').hidden);

  /* F — validation / project section / reload */
  const V = Lo.validate;
  ok('F1 validate pure: version, ranges, max, dup names', !V({ v: 2 }).ok && !V({ v: 1, cur: { didW: 9999 } }).ok && !V({ v: 1, cur: { max: 'x' } }).ok && !V({ v: 1, named: [{ name: 'a', l: {} }, { name: 'a', l: {} }] }).ok && !V({ v: 1, cur: { seqPct: 45, bytesPct: 45 } }).ok && V({ v: 1 }).ok);
  const sec = w.LoomWorkbench.project.sections().get('layout');
  const before = JSON.stringify(Lo.get());
  ok('F2 project section registered; validate does not mutate', sec && !sec.validate({ v: 1, cur: { didW: 1 } }).ok && sec.validate({ v: 1, cur: { didW: 200 } }).ok && JSON.stringify(Lo.get()) === before);
  sec.import(sec.validate({ v: 1, cur: { didW: 250, max: 'detail' }, named: [{ name: 'P', l: { valsW: 300 } }] }).value);
  ok('F3 section import applies + persists', Lo.cur().didW === 250 && Lo.get().named.length === 1 && JSON.parse(w.localStorage.getItem(KEY)).cur.didW === 250);
  ok('F4 section export round-trips', sec.validate(sec.export()).ok);
  { const a1 = sec.defaults(), a2 = sec.defaults(); a1.named.push(1);
    ok('F10 defaults(): fresh object each call, valid, = factory layout', a2.named.length === 0 && sec.validate(a2).ok && JSON.stringify(sec.validate(a2).value.cur) === JSON.stringify({ didW: null, valsW: null, seqPct: null, bytesPct: null, max: null })); }
  {
    const snap = JSON.stringify(Lo.get()), proto = w.Storage.prototype, orig = proto.setItem;
    proto.setItem = function () { const e = new Error('quota'); e.name = 'QuotaExceededError'; throw e; };
    let threw = null; try { sec.import(sec.validate({ v: 1, cur: { didW: 333 }, named: [] }).value); } catch (e) { threw = e; }
    const unchanged = JSON.stringify(Lo.get()) === snap && !d.getElementById('wsp').style.getPropertyValue('--lo-did').includes('333');
    let kthrew = false; try { key(w, Lo.handles.hDid, 'ArrowRight'); } catch (e) { kthrew = true; } // convenience write failure: visible notice, no throw
    proto.setItem = orig;
    ok('F8 strict import THROWS on storage failure, state untouched (rollback-safe)', threw && threw.name === 'QuotaExceededError' && unchanged);
    ok('F9 convenience save failure visible, no throw', !kthrew && /레이아웃 저장 실패/.test(d.getElementById('loMenuBtn').title));
    Lo.setDim('didW', 250); Lo.setMax('detail');
  }
  const stored = w.localStorage.getItem(KEY);
  const b2 = boot({ [KEY]: stored }); wins.push(b2.w); setW(b2.w, 1440); b2.w.LoomUdsLayout.apply();
  ok('F5 reload restores layout', b2.w.LoomUdsLayout.cur().didW === 250 && b2.d.getElementById('wsp').classList.contains('lo-max-detail'));
  const b3 = boot({ [KEY]: '{"v":9}' }); wins.push(b3.w);
  ok('F6 unknown stored version ignored → defaults', b3.w.LoomUdsLayout.cur().didW == null);
  const b4 = boot({ [KEY]: '{bad' }); wins.push(b4.w);
  ok('F7 corrupt stored → defaults', b4.w.LoomUdsLayout.cur().didW == null);

  /* G — static CSS contract */
  const css = fs.readFileSync(path.join(DIR, 'layout.css'), 'utf8'), js = fs.readFileSync(path.join(DIR, 'layout.js'), 'utf8');
  ok('G1 handles absolute (no implicit grid track)', /\.lo-h \{ position: absolute/.test(css));
  ok('G2 widths via own vars only, collapsed rail respected', /#wsp\.lo-did:not\(\.did-collapsed\) \{ --did-w: var\(--lo-did\); \}/.test(css) && /#wsp\.lo-vals:not\(\.vals-collapsed\)/.test(css));
  ok('G3 ≤1180 DID cap + handle hides; ≤560 no handles; maximize only ≥561', /max-width: 1180px[^}]*min\(var\(--lo-did\), 26vw\)/.test(css) && /@media \(max-width: 560px\) \{ \.lo-h \{ display: none !important; \} \}/.test(css) && /@media \(min-width: 561px\)[^]*lo-max-hist/.test(css));
  ok('G4 detail columns only ≥1181', /@media \(min-width: 1181px\) \{\s*#detail\.lo-cols/.test(css));
  ok('G5 no send/connect/network in layout.js', !/\bfetch\(|XMLHttpRequest|H\.send|sendRaw|connect\(/.test(js));
  ok('G6 onRender only repositions (no refresh)', /H\.onRender\(later\)/.test(js) && !/H\.refresh\(/.test(js));
} catch (e) { console.error('ERROR', e); fail++; }
wins.forEach(w => { try { w.close(); } catch (e) { /* */ } });
fs.writeFileSync(path.join(DIR, '../qa/team/trace-layout-check-result.log'), JSON.stringify({ check: 'layout-check', pass, fail, total: pass + fail, results }, null, 1));
console.log(`layout-check ${pass}/${pass + fail} ${fail ? 'FAIL' : 'PASS'}`);
process.exit(fail ? 1 : 0);
