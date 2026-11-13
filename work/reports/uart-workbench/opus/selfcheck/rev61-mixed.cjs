// REV6.1 focused regression (jsdom, not a browser): a fixed macro with one known + one unresolved session id is blocked entirely (zero TX).
const path = require('path'), fs = require('fs');
const { JSDOM } = require('/Users/ctmctm/workspace/Loom/ui/node_modules/jsdom');
const DIR = process.argv[2];
const html = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8').replace(/<script src="[^"]+"><\/script>/g, '');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const results = []; const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined ? '  ' + JSON.stringify(info) : '')); };
(async () => {
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:4180/uart-workbench/opus/index.html', runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window;
  w.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} });
  w.HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  w.HTMLDialogElement.prototype.close = function () { this.open = false; };
  w.Element.prototype.getBoundingClientRect = function () { return { left: 0, top: 0, right: 1200, bottom: 800, width: 1200, height: 800, x: 0, y: 0 }; };
  w.Element.prototype.scrollIntoView = function () {};
  w.CSS = { escape: s => String(s).replace(/["\\]/g, '\\$&') };
  w.structuredClone = globalThis.structuredClone;
  w.eval(fs.readFileSync(path.join(DIR, 'vendor/xterm/xterm.js'), 'utf8'));
  w.eval(fs.readFileSync(path.join(DIR, 'app.js'), 'utf8'));
  await sleep(900);
  const snap = () => w.loomUartOpus.snapshot();
  const click = sel => w.document.querySelector(sel).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
  click('[data-cmd="import"]');
  w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ macros: [{ name: 'MixFix', fixed: ['s-aux', 'profile-missing2'], steps: [{ type: 'send', text: 'ATI' }] }] });
  w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const mm = snap().macros.find(m => m.name === 'MixFix');
  ok('mixed fixed [aux, unresolved]: label says 실행 차단(미해결), aux is connected', /^고정 · 실행 차단\(미해결\) → aux/.test(mm.targetLabel) && snap().tabs.find(t => t.alias === 'aux').conn === 'connected', mm.targetLabel);
  click('#railMacros'); if (w.document.querySelector('#modUart').classList.contains('side-collapsed')) click('#railMacros');
  for (const scope of ['active', 'all']) {
    click(`.mscope [data-act="ms-mode"][data-mode="${scope}"]`);
    const tx0 = Object.fromEntries(snap().tabs.map(t => [t.alias, t.tx]));
    click(`.mac[data-mid="${mm.id}"] [data-act="run-macro"]`); await sleep(300);
    const tx1 = Object.fromEntries(snap().tabs.map(t => [t.alias, t.tx]));
    const toast = [...w.document.querySelectorAll('#toasts .toast')].map(x => x.textContent).pop() || '';
    ok(`mixed fixed blocked entirely in scope ${scope}: zero TX on every session (incl. known aux), block toast`, Object.keys(tx0).every(a => tx1[a] === tx0[a]) && !snap().tabs.some(t => t.run && t.run.macro === 'MixFix') && /실행 차단/.test(toast), { tx0, tx1, toast });
  }
  const fails = results.filter(r => !r.pass).length;
  console.log(`\n${results.length - fails}/${results.length} PASS`);
  fs.writeFileSync(path.join(DIR, 'selfcheck/rev61-mixed-result.json'), JSON.stringify({ when: new Date().toISOString(), env: 'jsdom focused regression', results }, null, 2));
  process.exit(fails ? 1 : 0);
})().catch(e => { console.error('CRASH', e); process.exit(2); });
