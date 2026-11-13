const path = require('path'), fs = require('fs');
const { JSDOM } = require('/Users/ctmctm/workspace/Loom/ui/node_modules/jsdom');
const DIR = process.argv[2];
const html = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8').replace('<script src="app.js"></script>', '');
const js = fs.readFileSync(path.join(DIR, 'app.js'), 'utf8');
const xtjs = fs.readFileSync(path.join(DIR, 'vendor/xterm/xterm.js'), 'utf8');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const results = []; const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined ? '  ' + JSON.stringify(info) : '')); };
function boot(storage) {
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:4180/uart-workbench/opus/index.html', runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window;
  if (storage) for (const [k, v] of Object.entries(storage)) w.localStorage.setItem(k, v);
  w.localStorage.setItem('unrelated.key', 'keep-me');
  w.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} });
  w.HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  w.HTMLDialogElement.prototype.close = function () { this.open = false; };
  w.Element.prototype.getBoundingClientRect = function () { return { left: 0, top: 0, right: 1200, bottom: 800, width: 1200, height: 800, x: 0, y: 0 }; };
  w.Element.prototype.scrollIntoView = function () {};
  w.CSS = { escape: s => String(s).replace(/["\\]/g, '\\$&') };
  w.structuredClone = globalThis.structuredClone;
  w.__loomRecTestBacklog = 16384; // isolate sink backpressure without huge bursts
  try { w.eval(xtjs); } catch (e) { console.log('NOTE xterm eval in jsdom failed:', e.message); }
  w.eval(js);
  return w;
}
const snap = w => w.loomUartOpus.snapshot();
const click = (w, sel) => { const el = w.document.querySelector(sel); if (!el) throw new Error('missing ' + sel); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); return el; };
const macMenu = (w, mid, label) => { click(w, `.mac[data-mid="${mid}"] [data-act="mac-more"]`); const it = [...w.document.querySelectorAll('.menu [data-mi]')].find(b => b.textContent.includes(label)); if (!it) throw new Error('menu item ' + label); it.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); };
(async () => {
  let w = boot();
  await sleep(900);
  let s = snap(w);
  ok('initial 2 panes mcu|aux', s.panes === 2 && s.layout.dir === 'row' && s.layout.a.on === 'mcu' && s.layout.b.on === 'aux', s.layout);
  ok('mcu boot log present', s.tabs.find(t => t.alias === 'mcu').lines > 20);
  ok('terminals default dark', s.tabs.every(t => t.theme === 'dark'));
  // nested split: split pane 2 (aux) down -> 3 panes; then split new pane right -> 4
  click(w, '.pane[data-pane-no="2"] [data-act="split-down"]');
  s = snap(w); ok('split down on pane2 -> 3 panes nested', s.panes === 3 && s.layout.b.dir === 'col', s.layout.b);
  click(w, `.pane[data-pane-no="3"] [data-act="pick"][data-sid="s-console"]`);
  s = snap(w); ok('open console into empty pane3', s.layout.b.b.on === 'console');
  click(w, '.pane[data-pane-no="3"] [data-act="split-right"]');
  s = snap(w); ok('split right pane3 -> 4 panes depth 3', s.panes === 4 && s.layout.b.b.dir === 'row');
  // session exclusivity: picking mcu in empty pane moves it
  click(w, `.pane[data-pane-no="4"] [data-act="pick"][data-sid="s-mcu"]`);
  s = snap(w); ok('opening already-open mcu moves tab (no duplicate port)', s.tabs.filter(t => t.alias === 'mcu').length === 1 && s.panes === 3, { panes: s.panes, tabs: s.tabs.map(t => [t.alias, t.pane]) });
  // per-terminal theme independence
  const auxTab = s.tabs.find(t => t.alias === 'aux');
  click(w, `[data-act="theme"][data-tab="${auxTab.id}"]`);
  s = snap(w); ok('theme toggle affects only aux', s.tabs.find(t => t.alias === 'aux').theme === 'light' && s.tabs.filter(t => t.alias !== 'aux').every(t => t.theme === 'dark'), s.tabs.map(t => [t.alias, t.theme]));
  { const xm = snap(w).tabs.find(t => t.alias === 'mcu').xterm;
    ok('snapshot exposes bounded xterm buffer facts (selection, cursorY, visible<=20, tail<=20)', xm && typeof xm.selection === 'string' && Number.isInteger(xm.cursorY) && Array.isArray(xm.visible) && xm.visible.length <= 20 && Array.isArray(xm.tail) && xm.tail.length <= 20 && xm.tail.some(l => /login|TLM|LoomBoot/.test(l)), { cursorY: xm && xm.cursorY, tail: xm && xm.tail.slice(-3) }); }
  ok('aux xterm theme background light, mcu dark', snap(w).tabs.find(t => t.alias === 'aux').xterm?.bg === '#ffffff' && snap(w).tabs.find(t => t.alias === 'mcu').xterm?.bg === '#0f1216');
  ok('aux term-wrap has t-light class, others dark', w.document.querySelector(`.term-wrap[data-tab="${auxTab.id}"]`).classList.contains('t-light') && [...w.document.querySelectorAll('.term-wrap')].filter(e => e.classList.contains('t-light')).length === 1);
  // maximize / restore
  click(w, '.pane[data-pane-no="1"] [data-act="max"]');
  s = snap(w); ok('maximize pane1', s.maxPane === 1 && w.document.querySelector('#treeRoot').classList.contains('has-max'));
  { const css = fs.readFileSync(path.join(DIR, 'styles.css'), 'utf8'); const r0 = snap(w).layout.r;
    ok('maximize CSS hides non-max branches + dividers, max chain flex 1 (static; geometry=QA)', /\.tree-root\.has-max \.divider \{ display: none; \}/.test(css) && /\.tree-root\.has-max \.child:not\(:has\(\.pane\.maxed\)\) \{ display: none; \}/.test(css) && /has-max \.child:has\(\.pane\.maxed\) \{ flex: 1 1 0px !important; \}/.test(css));
    click(w, '.pane[data-pane-no="1"] [data-act="max"]');
    ok('restore keeps split ratio', snap(w).layout.r === r0, { r0, r: snap(w).layout.r }); }
  s = snap(w); ok('restore', s.maxPane === null);
  // REV3: real terminal (vendored xterm) — no Send button/row, one xterm slot per visible tab, input = xterm onData -> transmit -> device line editor
  ok('no Send button / send row / inline prompt', !w.document.querySelector('[data-act="send"], .inrow, .btn-l, .iprompt, .ip-in'));
  ok('vendored xterm loaded (globalThis.Terminal)', typeof w.Terminal === 'function');
  ok('each visible tab has xterm slot with its persistent host', [...w.document.querySelectorAll('.pane .term-wrap')].every(tw => tw.querySelector('.term.xslot[data-view="xterm"] > .xhost')));
  const mcu = s.tabs.find(t => t.alias === 'mcu');
  const hostBefore = w.document.querySelector(`.xhost[data-tab="${mcu.id}"]`);
  click(w, `.pane .tab[data-tab="${s.tabs.find(t => t.alias === 'aux').id}"]`); // re-render
  ok('xterm host element survives re-render (same instance re-attached)', w.document.querySelector(`.xhost[data-tab="${mcu.id}"]`) === hostBefore && hostBefore.isConnected);
  const logText = tid => { click(w, `[data-act="search"][data-tab="${tid}"]`); const x = w.document.querySelector(`.term[data-tab="${tid}"] .tlines`).textContent; click(w, `[data-act="search"][data-tab="${tid}"]`); return x; };
  const importRun = async (name, steps, tid, wait = 2500) => {
    click(w, '[data-cmd="import"]');
    w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ macros: [{ name, steps }] });
    w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
    click(w, `[data-act="macros-menu"][data-tab="${tid}"]`);
    [...w.document.querySelectorAll('.menu [data-mi]')].find(b => b.textContent.includes(name)).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
    await sleep(wait); return snap(w).tabs.find(t => t.id === tid).run;
  };
  // Login via default macro (same transmit path as keyboard); password must not be echoed by device
  let rr = await importRun('L1', [{ type: 'send', text: '' }, { type: 'wait', text: 'login:', ms: 2000 }, { type: 'send', text: 'root' }, { type: 'wait', text: 'Password:', ms: 2000 }, { type: 'send', text: 'loom' }, { type: 'wait', text: 'Welcome', ms: 3000 }], mcu.id);
  ok('login flow over terminal path', rr && rr.status === 'done', rr);
  let lt = logText(mcu.id);
  ok('device echo assembled into log ("login: root"), password not echoed', /login: root/.test(lt) && !/Password: ?loom/.test(lt), lt.slice(-300));
  // Tab completion: "sta<TAB>" -> "status " then Enter
  rr = await importRun('T1', [{ type: 'send', text: 'sta\t' }, { type: 'wait', text: 'OK', ms: 2000 }], mcu.id);
  lt = logText(mcu.id);
  ok('Tab completion: sta<Tab> -> status executed', rr.status === 'done' && /mcu> status/.test(lt), lt.slice(-200));
  // history: ESC[A recalls previous line
  rr = await importRun('H1', [{ type: 'send', text: '\u001b[A' }, { type: 'wait', text: 'OK', ms: 2000 }], mcu.id);
  ok('history ↑ (ESC[A) recalls + runs status', rr.status === 'done', rr);
  // cursor editing: "tmp" ESC[D ESC[D 'e' -> "temp"; Ctrl+U clears "junk"
  rr = await importRun('E1', [{ type: 'send', text: 'junk\u0015tmp\u001b[D\u001b[De' }, { type: 'wait', text: 'tmp117', ms: 2000 }], mcu.id);
  lt = logText(mcu.id);
  ok('Ctrl+U + ←← insert edits line to "temp"', rr.status === 'done' && /mcu> temp(?![a-z])/.test(lt), { rr, hit: (lt.match(/mcu> [a-z]{2,6}/g) || []).slice(-4) });
  // multiline paste: "status\rtemp\r" in ONE transmit must execute in arrival order (ordered device queue)
  for (let k = 0; k < 3; k++) {
    rr = await importRun('P' + k, [{ type: 'send', text: 'status\rtemp' }, { type: 'wait', text: 'tmp117', ms: 3000 }], mcu.id, 900);
    lt = logText(mcu.id); const seg = lt.slice(lt.lastIndexOf('매크로 ▶ P' + k));
    const iS = seg.indexOf('mcu> status'), iF = seg.indexOf('fw      ecu-app'), iT = seg.indexOf('mcu> temp'), iTmp = seg.indexOf('tmp117');
    ok(`paste run ${k + 1}: status→its output→temp→its output (no reversal)`, rr.status === 'done' && iS >= 0 && iS < iF && iF < iT && iT < iTmp, { iS, iF, iT, iTmp });
  }
  // Ctrl+C
  const txB = snap(w).tabs.find(t => t.id === mcu.id).tx;
  rr = await importRun('C1', [{ type: 'send', text: 'abc\u0003', eol: 'none' }, { type: 'wait', text: 'mcu> ', ms: 1500 }], mcu.id, 800);
  lt = logText(mcu.id);
  ok('Ctrl+C: ^C echoed, fresh prompt, TX counted', rr.status === 'done' && /\^C/.test(lt) && snap(w).tabs.find(t => t.id === mcu.id).tx === txB + 4, lt.slice(-120));
  // disconnect / reconnect
  click(w, `[data-act="disconnect"][data-tab="${mcu.id}"]`);
  s = snap(w); ok('disconnect state + overlay', s.tabs.find(t => t.alias === 'mcu').conn === 'disconnected' && !!w.document.querySelector(`.term-wrap[data-tab="${mcu.id}"] .conn-overlay`));
  click(w, `.conn-overlay [data-act="connect"][data-tab="${mcu.id}"]`);
  s = snap(w); ok('reconnect', s.tabs.find(t => t.alias === 'mcu').conn === 'connected');
  // HEX mode
  click(w, `[data-act="mode"][data-mode="hex"][data-tab="${mcu.id}"]`);
  ok('hex view renders bytes', /\b0d 0a\b/.test(w.document.querySelector(`.term[data-tab="${mcu.id}"]`).textContent));
  click(w, `[data-act="mode"][data-mode="text"][data-tab="${mcu.id}"]`);
  ok('back to TERM re-attaches same xterm host', w.document.querySelector(`.term.xslot[data-tab="${mcu.id}"] > .xhost`) === hostBefore);
  click(w, `[data-act="macros-menu"][data-tab="${mcu.id}"]`);
  [...w.document.querySelectorAll('.menu [data-mi]')].find(b => b.textContent.includes('Read status')).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
  ok('▶ runs immediately: no confirm/target popup, no editor', !w.document.querySelector('#dlgRun').open && !w.document.querySelector('#dlgMacro').open && snap(w).tabs.find(t => t.alias === 'mcu').run?.macro === 'Read status');
  await sleep(1800);
  s = snap(w); let r = s.tabs.find(t => t.alias === 'mcu').run;
  ok('Read status completes on mcu', r && r.status === 'done' && r.steps.every(x => x === 'ok'), r);
  // macro CRUD: create via editor
  click(w, '[data-cmd="new-macro"]');
  const f = w.document.querySelector('#dlgMacro form');
  f.querySelector('[name=name]').value = 'Slow probe';
  const t0 = f.querySelector('[data-f="text"][data-i="0"]'); t0.value = 'version'; t0.dispatchEvent(new w.Event('input', { bubbles: true }));
  click(w, '#dlgMacro [data-add="delay"]');
  const d1 = f.querySelector('[data-f="ms"][data-i="1"]'); d1.value = '1500'; d1.dispatchEvent(new w.Event('input', { bubbles: true }));
  click(w, '#dlgMacro [data-add="send"]');
  const t2 = f.querySelector('[data-f="text"][data-i="2"]'); t2.value = 'status'; t2.dispatchEvent(new w.Event('input', { bubbles: true }));
  f.dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  s = snap(w); const mk = s.macros.find(m => m.name === 'Slow probe');
  ok('macro created with 3 steps', mk && mk.steps === 3, mk);
  // edit: rename
  macMenu(w, mk.id, '편집');
  w.document.querySelector('#dlgMacro [name=name]').value = 'Slow probe v2';
  w.document.querySelector('#dlgMacro form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  s = snap(w); ok('macro renamed', s.macros.some(m => m.name === 'Slow probe v2') && !s.macros.some(m => m.name === 'Slow probe'));
  // duplicate + delete
  macMenu(w, mk.id, '복제');
  s = snap(w); const dup = s.macros.find(m => m.name.startsWith('Slow probe v2 (복사'));
  ok('macro duplicated', !!dup);
  macMenu(w, dup.id, '삭제');
  s = snap(w); ok('macro deleted', !s.macros.some(m => m.id === dup.id));
  // run + stop during delay -> step 3 never sent
  const before = s.tabs.find(t => t.alias === 'mcu').macroSends.length;
  click(w, `[data-act="run-macro"][data-macro="${mk.id}"]`);
  await sleep(400);
  s = snap(w); r = s.tabs.find(t => t.alias === 'mcu').run;
  ok('custom macro running at delay step', r && r.status === 'running' && r.step === 2, r);
  click(w, `.runstrip [data-act="stop-macro"][data-tab="${mcu.id}"]`);
  await sleep(2000);
  s = snap(w); const mt = s.tabs.find(t => t.alias === 'mcu'); r = mt.run;
  const sends = mt.macroSends.slice(before).map(x => x.text);
  ok('stopped: status stopped, step3 skipped', r.status === 'stopped' && r.steps[2] === 'skip', r);
  ok('stopped: no "status" sent after stop (only "version")', JSON.stringify(sends) === '["version"]', sends);
  // QA repro: disconnect mid-delay aborts run; reconnect must NOT resume (no "must-not-send")
  { const aux = snap(w).tabs.find(t => t.alias === 'aux');
    const before = aux.macroSends.length;
    const p = importRun('DR1', [{ type: 'send', text: 'version' }, { type: 'delay', ms: 3000 }, { type: 'send', text: 'must-not-send' }], aux.id, 0);
    await p; await sleep(1100);
    click(w, `[data-act="disconnect"][data-tab="${aux.id}"]`);
    await sleep(900);
    click(w, `.conn-overlay [data-act="connect"][data-tab="${aux.id}"]`);
    await sleep(2600);
    const a2 = snap(w).tabs.find(t => t.id === aux.id);
    const sends = a2.macroSends.slice(before).map(x => x.text);
    ok('disconnect aborts macro; reconnect never resumes it', a2.run.status === 'stopped' && !sends.includes('must-not-send') && JSON.stringify(sends) === '["version"]', { run: a2.run, sends }); }
  // port discovery dropdown (sim registry, uart.ports shape)
  click(w, '[data-cmd="new-session"]');
  { const dlg = w.document.querySelector('#dlgSession'); const sel = dlg.querySelector('[data-role="portsel"]');
    const opts = [...sel.options].map(o => o.textContent);
    ok('port select lists detected ports with description + in-use badge', sel.tagName === 'SELECT' && opts.some(t => /\/dev\/ttyUSB0 — FTDI FT232R.*사용 중: mcu/.test(t)) && opts.some(t => /수동 입력/.test(t)), opts);
    ok('new session defaults to a free detected port', /사용 가능/.test(sel.selectedOptions[0].textContent), sel.value);
    sel.value = '/dev/ttyUSB0'; sel.dispatchEvent(new w.Event('change', { bubbles: true }));
    ok('selecting in-use port offers focus/move', !!dlg.querySelector('[data-act="port-focus"]') && !!dlg.querySelector('[data-act="port-move"]'));
    dlg.querySelector('[name=alias]').value = 'dup0';
    const openBtn = dlg.querySelector('button[value="open"]');
    dlg.querySelector('form').dispatchEvent(Object.assign(new w.Event('submit', { bubbles: true, cancelable: true }), { submitter: openBtn }));
    ok('save+open on in-use port refused (no duplicate physical port)', dlg.open && /사용 중/.test(dlg.querySelector('[data-role="err"]').textContent) && snap(w).tabs.filter(t => t.alias === 'dup0').length === 0);
    click(w, '#dlgSession [data-act="rescan"]');
    sel.value = '/dev/ttyUSB3'; sel.dispatchEvent(new w.Event('change', { bubbles: true }));
    dlg.querySelector('form').dispatchEvent(Object.assign(new w.Event('submit', { bubbles: true, cancelable: true }), { submitter: openBtn }));
    ok('free port saved + opened as new tab', !dlg.open && snap(w).tabs.some(t => t.alias === 'dup0') && snap(w).sessions.find(x => x.alias === 'dup0').port === '/dev/ttyUSB3');
    const dp = snap(w).detectedPorts.find(p => p.path === '/dev/ttyUSB3');
    ok('discovery reflects newly opened port', dp && dp.openBy === 'dup0', dp); }
  // cross-session exclusivity: second saved session on mcu's port focuses mcu instead of opening
  click(w, '[data-cmd="new-session"]');
  { const dlg = w.document.querySelector('#dlgSession'); const sel = dlg.querySelector('[data-role="portsel"]');
    dlg.querySelector('[name=alias]').value = 'mcu_alt'; sel.value = '/dev/ttyUSB0'; sel.dispatchEvent(new w.Event('change', { bubbles: true }));
    dlg.querySelector('form').dispatchEvent(Object.assign(new w.Event('submit', { bubbles: true, cancelable: true }), { submitter: dlg.querySelector('button[value="save"]') }));
    const sid = snap(w).sessions.find(x => x.alias === 'mcu_alt').id;
    click(w, '.pane[data-pane-no="1"] [data-act="add-tab"]');
    [...w.document.querySelectorAll('.menu [data-mi]')].find(x => x.textContent.startsWith('mcu_alt')).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
    ok('opening 2nd profile on same physical port does not duplicate (focuses mcu)', !snap(w).tabs.some(t => t.alias === 'mcu_alt') && snap(w).tabs.filter(t => t.alias === 'mcu').length === 1); }
  // ===== REV4: receive-time gutter + UART recording =====
  { const mt = snap(w).tabs.find(t => t.alias === 'mcu');
    ok('gutter on by default, rows show HH:mm:ss.SSS host times', mt.tsGutter && Array.isArray(mt.gutter) && mt.gutter.some(x => /^\d\d:\d\d:\d\d\.\d{3}$/.test(x)), mt.gutter && mt.gutter.slice(0, 4));
    ok('VT buffer has no injected timestamps (tail lines raw)', mt.xterm.tail.every(l => !/^\d\d:\d\d:\d\d\.\d{3}/.test(l)), mt.xterm.tail.slice(-3));
    click(w, `[data-act="ts-toggle"][data-tab="${mt.id}"]`);
    ok('ts toggle hides gutter (data-ts=off), same xterm host kept', w.document.querySelector(`.term.xslot[data-tab="${mt.id}"]`).dataset.ts === 'off' && w.document.querySelector(`.term.xslot[data-tab="${mt.id}"] > .xhost`) === hostBefore);
    click(w, `[data-act="ts-toggle"][data-tab="${mt.id}"]`); }
  const recOpen = tid => click(w, `[data-act="rec"][data-tab="${tid}"]`);
  const recDlg = () => w.document.querySelector('#dlgRec');
  const setFmt = f => { recDlg().querySelector(`input[name=format][value=${f}]`).checked = true; };
  // (a) cancel: picker AbortError -> nothing recorded, honest message
  w.showSaveFilePicker = () => Promise.reject(Object.assign(new Error('The user aborted a request.'), { name: 'AbortError' }));
  recOpen(mcu.id); click(w, '#dlgRec [data-act="rec-file"]'); await sleep(50);
  ok('picker cancel -> idle, no recorder, message shown', snap(w).tabs.find(t => t.id === mcu.id).recState === 'idle' && /취소/.test(recDlg().querySelector('[data-role=err]').textContent));
  // (b) mock FSA with random async latency: serialized order, checkpoint commit, close on stop
  const files = []; let pickArg = null;
  const mkHandle = (name, failAt) => { const f = { name, committed: '', closes: 0, writes: 0 }; files.push(f);
    return { name, createWritable: async (o) => { let buf = o && o.keepExistingData ? f.committed : ''; let pos = buf.length;
      return { write: async c => { await sleep(Math.random() * 30); f.writes++; if (failAt && f.writes >= failAt) throw Object.assign(new Error('disk full (mock)'), { name: 'QuotaExceededError' }); buf = buf.slice(0, pos) + c; pos += c.length; },
        seek: async n => { pos = n; }, close: async () => { await sleep(5); f.committed = buf; f.closes++; }, abort: async () => {} }; } }; };
  w.showSaveFilePicker = async (o) => { pickArg = o; return mkHandle(o.suggestedName); };
  recOpen(mcu.id); setFmt('log'); click(w, '#dlgRec [data-act="rec-file"]'); await sleep(80);
  ok('file record starts after explicit pick; suggested name alias+time.log', snap(w).tabs.find(t => t.id === mcu.id).recState === 'recording' && /^mcu-\d{8}-\d{6}\.log$/.test(pickArg.suggestedName), pickArg && pickArg.suggestedName);
  ok('tab shows rec dot + pill state recording', !!w.document.querySelector(`.tab[data-tab="${mcu.id}"] .rec-dot`) && w.document.querySelector(`[data-act="rec"][data-tab="${mcu.id}"]`).dataset.recState === 'recording');
  const f1 = files[files.length - 1];
  rr = await importRun('RECA', [{ type: 'send', text: 'status' }, { type: 'wait', text: 'OK', ms: 2000 }, { type: 'send', text: 'temp' }, { type: 'wait', text: 'tmp117', ms: 2000 }], mcu.id, 1300);
  const len1 = f1.committed.length, c1 = f1.closes; await sleep(1300); const len2 = f1.committed.length;
  { const ri = snap(w).tabs.find(t => t.id === mcu.id).rec;
    ok('commit-on-close fake: file grows on ~1s checkpoints BEFORE stop', c1 >= 1 && len1 > 0 && len2 > len1 && ri.state === 'recording', { c1, len1, len2, closes: f1.closes });
    ok('honest counters: committed == file bytes, pending >= 0, lastSavedAt set', ri.committed === new TextEncoder().encode(f1.committed).length && ri.pending >= 0 && !!ri.lastSavedAt, { committed: ri.committed, file: f1.committed.length, pending: ri.pending, last: ri.lastSavedAt }); }
  recOpen(mcu.id); click(w, '#dlgRec [data-act="rec-stop"]'); await sleep(400);
  { const ri = snap(w).tabs.find(t => t.id === mcu.id).rec; const txt = f1.committed; const ls = txt.split('\n').filter(Boolean);
    ok('stop flushes+closes: state closed, written==committed==file bytes', ri.state === 'closed' && ri.written === ri.committed && ri.committed === new TextEncoder().encode(txt).length, { ri: { state: ri.state, w: ri.written, c: ri.committed }, file: txt.length });
    ok('.log format: header, timestamped RX/TX lines in order, footer', /^# Loom UART log — mcu \/dev\/ttyUSB0/.test(txt) && ls.some(l => /^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3} TX  status$/.test(l)) && ls.some(l => /^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3} RX  mcu> status$/.test(l)) && /# stopped .* 사용자 중지/.test(ls[ls.length - 1]), ls.filter(l => /status|temp|stopped/.test(l)).slice(0, 6));
    const iS = txt.indexOf('TX  status'), iT = txt.indexOf('TX  temp'), iO = txt.indexOf('RX  tmp117');
    ok('serialized writes keep stream order despite random write latency', iS > 0 && iS < iT && iT < iO, { iS, iT, iO }); }
  // (c) write error -> state error, no fake "recording"
  w.showSaveFilePicker = async (o) => mkHandle(o.suggestedName, 2);
  recOpen(mcu.id); click(w, '#dlgRec [data-act="rec-file"]'); await sleep(80);
  await importRun('RECB', [{ type: 'send', text: 'version' }, { type: 'wait', text: 'OK', ms: 2000 }], mcu.id, 800);
  { const ti = snap(w).tabs.find(t => t.id === mcu.id);
    ok('write failure -> recState error with message, pill rec-error', ti.recState === 'error' && /QuotaExceededError: disk full/.test(ti.rec.errors.join()) && w.document.querySelector(`[data-act="rec"][data-tab="${mcu.id}"]`).dataset.recState === 'error', ti.rec && ti.rec.errors); }
  // (c2) stalled writes: backlog bounded -> explicit error, never silent growth (exercise with a small burst against a never-resolving fake)
  { w.showSaveFilePicker = async (o) => ({ name: o.suggestedName, createWritable: async () => ({ write: () => new Promise(() => {}), seek: async () => {}, close: async () => {}, abort: async () => {} }) });
    click(w, `.tab[data-tab="${mcu.id}"]`); recOpen(mcu.id); setFmt('log'); click(w, '#dlgRec [data-act="rec-file"]'); await sleep(60); recDlg().close();
    for (let k = 0; k < 40 && snap(w).tabs.find(t => t.id === mcu.id).recState === 'recording'; k++) await importRun('BK' + k, [{ type: 'send', text: 'status' }, { type: 'wait', text: 'OK', ms: 1500 }], mcu.id, 260);
    const ti = snap(w).tabs.find(t => t.id === mcu.id);
    ok('stalled file writes: backlog capped (test limit 16 KiB) -> explicit error state', ti.recState === 'error' && /대기 .* > 16\.0 kB/.test(ti.rec.errors.join()), ti.rec && ti.rec.errors);
  }
  // macroStop regression: manual input already sent (queued at device) is NOT dropped; no further macro step sent
  { const typeInto = (tid, text) => { const ta = w.document.querySelector(`.xhost[data-tab="${tid}"] .xterm-helper-textarea`); ta.dispatchEvent(new w.InputEvent('input', { data: text, inputType: 'insertText', bubbles: true })); ta.dispatchEvent(new w.KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true })); };
    click(w, `.tab[data-tab="${mcu.id}"]`);
    const m0 = snap(w).tabs.find(t => t.id === mcu.id).macroSends.length;
    importRun('MS1', [{ type: 'send', text: 'status' }, { type: 'delay', ms: 1500 }, { type: 'send', text: 'temp' }], mcu.id, 0);
    await sleep(20); typeInto(mcu.id, 'version'); // arrives while device is busy with "status" -> queued
    click(w, `.runstrip [data-act="stop-macro"][data-tab="${mcu.id}"]`);
    await sleep(2000);
    const mt2 = snap(w).tabs.find(t => t.id === mcu.id); const lt2 = logText(mcu.id).slice(-1500);
    ok('macroStop keeps already-sent manual input queued (version answered)', /mcu> version/.test(lt2) && /ecu-app 4\.12\.0\+g7c1e2d9 \(2026-10-28\)/.test(lt2), lt2.slice(-300));
    ok('macroStop: no further macro step sent (no "temp")', JSON.stringify(mt2.macroSends.slice(m0).map(x => x.text)) === '["status"]' && mt2.run.status === 'stopped', mt2.macroSends.slice(m0)); }
  // (d) jsonl buffer on aux: independent of pause/clear/line cap; exact RX byte accounting
  const auxT = snap(w).tabs.find(t => t.alias === 'aux');
  click(w, `.tab[data-tab="${auxT.id}"]`);
  recOpen(auxT.id); setFmt('jsonl'); recDlg().querySelector('input[name=format][value=jsonl]').dispatchEvent(new w.Event('change', { bubbles: true }));
  ok('fileTs checkbox disabled for JSONL (applies to .log only)', recDlg().querySelector('input[name=fileTs]').disabled === true);
  click(w, '#dlgRec [data-act="rec-buffer"]'); await sleep(30);
  const rx0 = snap(w).tabs.find(t => t.id === auxT.id).rx;
  click(w, `[data-act="pause"][data-tab="${auxT.id}"]`); click(w, `[data-act="clear"][data-tab="${auxT.id}"]`);
  await sleep(3400);
  recOpen(auxT.id); const rxStop = snap(w).tabs.find(t => t.id === auxT.id).rx; click(w, '#dlgRec [data-act="rec-stop"]'); await sleep(50);
  { const at = snap(w).tabs.find(t => t.id === auxT.id);
    ok('jsonl buffer: recorded RX bytes == bytes received while paused+cleared', at.rec.state === 'closed' && at.rec.sink === 'buffer' && at.rec.rxBytes === rxStop - rx0 && at.rec.rxBytes > 0, { rec: at.rec.rxBytes, delta: rxStop - rx0 });
    const whole = at.rec.tail.filter(l => !l.endsWith('…')); // preview lines >200 chars are truncated
    ok('jsonl records parse with type/t/ms/dir/data', whole.length > 0 && whole.every(l => { const o = JSON.parse(l); return o.type && o.t && o.ms; }) && whole.some(l => JSON.parse(l).dir === 'rx'), whole.slice(-2));
    ok('buffer sink is labelled as not-a-file (committed stays 0) + export offered', at.rec.committed === 0 && /파일에 저장되지 않음/.test(recDlg().textContent) && !!recDlg().querySelector('[data-act="rec-export"]')); }
  click(w, `[data-act="pause"][data-tab="${auxT.id}"]`);
  recDlg().close();
  // (e) per-target isolation + disconnect stops recording explicitly
  click(w, `.tab[data-tab="${mcu.id}"]`);
  recOpen(mcu.id); setFmt('log'); click(w, '#dlgRec [data-act="rec-buffer"]'); recDlg().close();
  ok('recording isolated per target (aux not recording)', snap(w).tabs.find(t => t.id === auxT.id).recState === 'closed' && snap(w).tabs.find(t => t.id === mcu.id).recState === 'recording');
  click(w, `[data-act="disconnect"][data-tab="${mcu.id}"]`); await sleep(30);
  { const ri = snap(w).tabs.find(t => t.id === mcu.id).rec; ok('disconnect stops+closes recording with explicit reason', ri.state === 'closed' && /포트 닫힘으로 기록 종료/.test(ri.reason), ri.reason); }
  click(w, `.conn-overlay [data-act="connect"][data-tab="${mcu.id}"]`);
  ok('reconnect does not resume recording', snap(w).tabs.find(t => t.id === mcu.id).recState === 'closed');
  // ===== REV5: immediate ▶ vs separate ✎, missing vars, Current/All-connected target =====
  { const typeIn = (tid, text) => { const ta = w.document.querySelector(`.xhost[data-tab="${tid}"] .xterm-helper-textarea`); ta.dispatchEvent(new w.InputEvent('input', { data: text, inputType: 'insertText', bubbles: true })); ta.dispatchEvent(new w.KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true })); };
    const tab = a => snap(w).tabs.find(t => t.alias === a);
    click(w, `.tab[data-tab="${mcu.id}"]`); click(w, '#railMacros');
    if (w.document.querySelector('#modUart').classList.contains('side-collapsed')) click(w, '#railMacros');
    const n0 = tab('mcu').macroSends.length;
    click(w, '.mac[data-mid="m-status"] [data-act="run-macro"]'); await sleep(30);
    ok('sidebar ▶ runs immediately on current session (no popup)', !w.document.querySelector('#dlgRun').open && !w.document.querySelector('#dlgMacro').open && tab('mcu').run.macro === 'Read status' && tab('mcu').run.status === 'running');
    await sleep(1500);
    const n1 = tab('mcu').macroSends.length;
    ok('row has only ▶ + separate ⋯ (no adjacent edit/dup/delete); target label = current alias; no retired confirm wording', !w.document.querySelector('.mac [data-act="edit-macro"], .mac [data-act="dup-macro"], .mac [data-act="del-macro"]') && !!w.document.querySelector('.mac[data-mid="m-status"] [data-act="mac-more"]') && /대상: 활성 → mcu/.test(w.document.querySelector('.mac[data-mid="m-status"] [data-role="mac-target"]').textContent) && !/대상 확인|확인 생략|실행 전 대상/.test(w.document.querySelector('#side').textContent));
    click(w, '.mac[data-mid="m-status"] .mac-name'); await sleep(50);
    ok('row body click opens nothing and runs nothing', !w.document.querySelector('#dlgMacro').open && tab('mcu').macroSends.length === n1);
    macMenu(w, 'm-status', '편집'); await sleep(300);
    ok('⋯ → 편집 opens editor only, never runs', w.document.querySelector('#dlgMacro').open && tab('mcu').macroSends.length === n1 && n1 > n0);
    w.document.querySelector('#dlgMacro').close();
    // missing variable -> vars-only dialog, no run; filling runs
    click(w, '[data-cmd="import"]'); w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ macros: [{ name: 'NeedVar', vars: [{ name: 'who', def: '' }], steps: [{ type: 'send', text: 'echo ${who}' }] }] });
    w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
    const nv = snap(w).macros.find(m => m.name === 'NeedVar'); const n2 = tab('mcu').macroSends.length;
    click(w, `.mac[data-mid="${nv.id}"] [data-act="run-macro"]`); await sleep(50);
    ok('missing var: inline vars dialog (not editor), nothing sent', w.document.querySelector('#dlgRun').open && !w.document.querySelector('#dlgMacro').open && !!w.document.querySelector('#dlgRun input[data-missing="true"]') && tab('mcu').macroSends.length === n2);
    w.document.querySelector('#dlgRun input[name="var:who"]').value = 'loom';
    w.document.querySelector('#dlgRun form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true })); await sleep(80);
    ok('after filling var it runs with saved value', tab('mcu').macroSends.slice(n2).some(x => x.text === 'echo loom'));
    // open debug (read-only) somewhere and close dup0 so it can be reopened after All is enabled
    const dup = tab('dup0'); if (dup) click(w, `.tab .x[data-tab="${dup.id}"]`);
    if (!tab('debug')) { click(w, '.pane[data-pane-no="1"] [data-act="add-tab"]'); [...w.document.querySelectorAll('.menu [data-mi]')].find(x => x.textContent.startsWith('debug')).dispatchEvent(new w.MouseEvent('click', { bubbles: true })); }
    await sleep(100);
    ok('default send target = current session', snap(w).broadcast.on === false && w.document.querySelector('#bcCur').getAttribute('aria-pressed') === 'true');
    click(w, '[data-cmd="bc-all"]');
    const bc = snap(w).broadcast; const conn = snap(w).tabs.filter(t => t.conn === 'connected' && t.alias !== 'debug').map(t => t.alias).sort();
    ok('All: recipients = connected writable, read-only debug excluded + shown', bc.on && JSON.stringify(bc.recipients.slice().sort()) === JSON.stringify(conn) && bc.excluded.some(x => /^debug:읽기 전용/.test(x)) && /ALL →/.test(w.document.querySelector('#bcBadge').textContent) && /debug\(읽기 전용\)/.test(w.document.querySelector('#bcBadge').textContent), bc);
    ok('pane headers mark ALL recipient / excluded', !!w.document.querySelector(`.phead[data-tab="${mcu.id}"] .chip.bc-in`));
    // a port opened after enabling does NOT join
    click(w, '.pane[data-pane-no="1"] [data-act="add-tab"]'); [...w.document.querySelectorAll('.menu [data-mi]')].find(x => x.textContent.startsWith('dup0')).dispatchEvent(new w.MouseEvent('click', { bubbles: true })); await sleep(100);
    ok('newly opened port does not silently join recipients', !snap(w).broadcast.recipients.includes('dup0') && tab('dup0') && tab('dup0').conn === 'connected');
    // typing in one terminal fans out to the frozen set only
    const tx0 = Object.fromEntries(snap(w).tabs.map(t => [t.alias, t.tx]));
    click(w, `.tab[data-tab="${mcu.id}"]`); typeIn(mcu.id, 'version'); await sleep(200);
    const tx1 = Object.fromEntries(snap(w).tabs.map(t => [t.alias, t.tx]));
    ok('typed bytes routed to every recipient, not to excluded/new ports', bc.recipients.every(a => tx1[a] > tx0[a]) && tx1.debug === tx0.debug && tx1.dup0 === tx0.dup0, { tx0, tx1 });
    // ===== REV6: macro execution scope (panel) — independent of keyboard input target =====
    click(w, '[data-cmd="import"]'); w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ macros: [{ name: 'GRP', steps: [{ type: 'send', text: 'version' }, { type: 'delay', ms: 1200 }, { type: 'send', text: 'status' }] }] });
    w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
    const g = snap(w).macros.find(m => m.name === 'GRP');
    const sendsSince = (base, a) => snap(w).tabs.find(t => t.alias === a).macroSends.slice(base[a] || 0).map(x => x.text);
    const base = () => Object.fromEntries(snap(w).tabs.map(t => [t.alias, t.macroSends.length]));
    click(w, `.tab[data-tab="${mcu.id}"]`);
    ok('scope control present near ▶, default 활성 with live preview = active alias', snap(w).macroScope.mode === 'active' && w.document.querySelector('.mscope [data-act="ms-mode"][data-mode="active"]').getAttribute('aria-checked') === 'true' && /활성 → mcu/.test(w.document.querySelector('[data-role="ms-recipients"]').textContent));
    // keyboard All (BC on) must NOT turn ▶ into broadcast
    let b0 = base(); click(w, `.mac[data-mid="${g.id}"] [data-act="run-macro"]`); await sleep(1500);
    ok('independence: keyboard target = All, macro scope = 활성 -> ▶ runs on active session only', snap(w).broadcast.on && JSON.stringify(sendsSince(b0, 'mcu')) === '["version","status"]' && ['aux', 'console'].every(a => sendsSince(b0, a).length === 0) && /대상: 활성 → mcu/.test(w.document.querySelector(`.mac[data-mid="m-status"] [data-role="mac-target"]`).textContent));
    // 연결된 모두: recipients resolved at launch, read-only excluded and shown
    click(w, '.mscope [data-act="ms-mode"][data-mode="all"]');
    const ms = snap(w).macroScope; const connW = snap(w).tabs.filter(t => t.conn === 'connected' && t.alias !== 'debug').map(t => t.alias).sort();
    ok('scope 연결된 모두: preview lists connected writable + count, debug excluded (읽기 전용)', ms.mode === 'all' && JSON.stringify(ms.recipients.slice().sort()) === JSON.stringify(connW) && ms.excluded.some(x => /^debug:읽기 전용/.test(x)) && /연결된 모두 → .*\(\d+\)/.test(w.document.querySelector('[data-role="ms-recipients"]').textContent), ms);
    const conT = tab('console'); click(w, `.tab .x[data-tab="${conT.id}"]`); await sleep(50); // console closed before launch
    b0 = base(); click(w, `.mac[data-mid="${g.id}"] [data-act="run-macro"]`); await sleep(100);
    const running = snap(w).tabs.filter(t => t.run && t.run.macro === 'GRP' && t.run.status === 'running');
    ok('All scope ▶: one run per recipient, shared group id', running.length >= 2 && new Set(running.map(t => t.run.gid)).size === 1, running.map(t => t.alias));
    click(w, '.pane[data-pane-no="1"] [data-act="add-tab"]'); [...w.document.querySelectorAll('.menu [data-mi]')].find(x => x.textContent.startsWith('console')).dispatchEvent(new w.MouseEvent('click', { bubbles: true })); await sleep(60);
    const auxId = tab('aux').id; click(w, `.tab[data-tab="${auxId}"]`); click(w, `[data-act="disconnect"][data-tab="${auxId}"]`);
    await sleep(1500);
    const af = snap(w).tabs;
    ok('snapshot: session opened mid-run does not join (console no run/sends)', !(af.find(t => t.alias === 'console').run) && sendsSince(b0, 'console').length === 0);
    ok('disconnect cancels that target only; others complete', af.find(t => t.alias === 'aux').run.status === 'stopped' && JSON.stringify(sendsSince(b0, 'aux')) === '["version"]' && running.filter(t => t.alias !== 'aux').every(t => af.find(x => x.alias === t.alias).run.status === 'done' && JSON.stringify(sendsSince(b0, t.alias)) === '["version","status"]'), running.map(t => [t.alias, sendsSince(b0, t.alias)]));
    click(w, `.conn-overlay [data-act="connect"][data-tab="${auxId}"]`);
    // global stop (toolbar badge) cancels the whole group
    b0 = base(); click(w, `.mac[data-mid="${g.id}"] [data-act="run-macro"]`); await sleep(150);
    const grpNow = snap(w).tabs.filter(t => t.run && t.run.macro === 'GRP' && t.run.status === 'running').map(t => t.alias);
    click(w, '#bcBadge [data-cmd="stop-all"]'); await sleep(1500);
    ok('global Stop: every target stopped, no further step sent', grpNow.length >= 2 && grpNow.every(a => snap(w).tabs.find(t => t.alias === a).run.status === 'stopped' && JSON.stringify(sendsSince(b0, a)) === '["version"]'), grpNow);
    // sidebar ■ 전체 중지 stops every running instance of that macro
    b0 = base(); click(w, `.mac[data-mid="${g.id}"] [data-act="run-macro"]`); await sleep(150);
    const grp2 = snap(w).tabs.filter(t => t.run && t.run.macro === 'GRP' && t.run.status === 'running').map(t => t.alias);
    const sb = w.document.querySelector(`.mac[data-mid="${g.id}"] [data-act="stop-macro-all"]`);
    ok('sidebar shows ■ 전체 중지 while group runs', grp2.length >= 2 && !!sb && /전체 중지/.test(sb.textContent));
    click(w, `.mac[data-mid="${g.id}"] [data-act="stop-macro-all"]`); await sleep(1500);
    ok('sidebar 전체 중지 stops all targets of that macro, no further step', grp2.every(a => snap(w).tabs.find(t => t.alias === a).run.status === 'stopped' && JSON.stringify(sendsSince(b0, a)) === '["version"]'));
    // 선택: empty -> no run + picker focus; subset runs exactly; disconnected selection excluded
    click(w, '.mscope [data-act="ms-mode"][data-mode="selected"]');
    ok('선택 shows checkbox picker of open sessions; read-only disabled', !!w.document.querySelector('.mscope-pick') && w.document.querySelector(`.mscope-pick input[value="s-debug"]`).disabled === true);
    b0 = base(); click(w, '.mac[data-mid="m-status"] [data-act="run-macro"]'); await sleep(50);
    ok('선택 empty: nothing runs, picker focused', Object.keys(b0).every(a => sendsSince(b0, a).length === 0) && w.document.activeElement && w.document.activeElement.dataset.act === 'ms-sel');
    for (const sid of ['s-mcu', 's-console']) { const cb = w.document.querySelector(`.mscope-pick input[value="${sid}"]`); cb.checked = true; cb.dispatchEvent(new w.Event('change', { bubbles: true })); }
    ok('선택 preview shows exact chosen aliases + count', /선택 → mcu, console \(2\)/.test(w.document.querySelector('[data-role="ms-recipients"]').textContent), w.document.querySelector('[data-role="ms-recipients"]').textContent);
    const conId = tab('console').id; click(w, `.tab[data-tab="${conId}"]`); click(w, `[data-act="disconnect"][data-tab="${conId}"]`); await sleep(50);
    ok('선택 with a disconnected session: preview excludes it with reason', /선택 → mcu · 제외 console\(닫힘\)/.test(w.document.querySelector('[data-role="ms-recipients"]').textContent), w.document.querySelector('[data-role="ms-recipients"]').textContent);
    b0 = base(); click(w, `.mac[data-mid="${g.id}"] [data-act="run-macro"]`); await sleep(1500);
    ok('선택 ▶ runs exactly the eligible chosen subset', JSON.stringify(sendsSince(b0, 'mcu')) === '["version","status"]' && ['aux', 'console', 'dup0'].every(a => sendsSince(b0, a).length === 0));
    click(w, `.conn-overlay [data-act="connect"][data-tab="${conId}"]`);
    // legacy fixed target -> visible override that wins over the panel scope
    click(w, '[data-cmd="import"]'); w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ macros: [{ name: 'FixAux', target: 's-aux', steps: [{ type: 'send', text: 'AT' }] }] });
    w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
    const fx = snap(w).macros.find(m => m.name === 'FixAux');
    ok('legacy target migrated to fixed override, row shows 고정 chip + label', JSON.stringify(fx.fixed) === '["aux"]' && !!w.document.querySelector(`.mac[data-mid="${fx.id}"] .chip.fixed`) && /대상: 고정 → aux/.test(w.document.querySelector(`.mac[data-mid="${fx.id}"] [data-role="mac-target"]`).textContent));
    click(w, '.mscope [data-act="ms-mode"][data-mode="all"]');
    b0 = base(); click(w, `.mac[data-mid="${fx.id}"] [data-act="run-macro"]`); await sleep(300);
    ok('precedence: fixed override wins over panel 연결된 모두 (only aux)', JSON.stringify(sendsSince(b0, 'aux')) === '["AT"]' && ['mcu', 'console'].every(a => sendsSince(b0, a).length === 0));
    // REV6.1 F5: import of fixed targets referring to custom sessions in the same payload / unresolved ids (scope = 연결된 모두 here)
    { const imp = obj => { click(w, '[data-cmd="import"]'); w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify(obj); w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true })); };
      const toastTxt = () => [...w.document.querySelectorAll('#toasts .toast')].map(x => x.textContent).join(' | ');
      imp({ format: 'loom-uart-bench', version: 1, sessions: [{ id: 'profile-qa1', alias: 'qa-bridge', group: 'QA', port: '/dev/ttyUSB9', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', profile: 'mcu' }, { id: 'profile-auxdup', alias: 'aux', port: '/dev/ttyUSB1', baud: 9600, profile: 'aux' }],
        macros: [{ name: 'FixBridge', fixed: ['profile-qa1'], steps: [{ type: 'send', text: 'version' }] }, { name: 'FixAuxAlias', fixed: ['profile-auxdup'], steps: [{ type: 'send', text: 'ATI' }] }] });
      const fb = snap(w).macros.find(m => m.name === 'FixBridge'), fa = snap(w).macros.find(m => m.name === 'FixAuxAlias');
      ok('import remaps fixed -> custom session imported in same payload (new id), chip 고정 not widened', JSON.stringify(fb.fixed) === '["qa-bridge"]' && fb.unresolvedFixed === 0 && /고정 → 없음 · 제외 qa-bridge\(열려 있지 않음\)/.test(fb.targetLabel) && /새 세션에 연결/.test(toastTxt()), { fb, toast: toastTxt() });
      ok('import remaps fixed -> existing session with same alias (aux)', JSON.stringify(fa.fixed) === '["aux"]' && /^고정 → aux/.test(fa.targetLabel), fa);
      let b1 = base(); click(w, `.mac[data-mid="${fb.id}"] [data-act="run-macro"]`); await sleep(200);
      ok('fixed to an unopened imported session: ▶ blocked, NOT widened to panel 연결된 모두', snap(w).macroScope.mode === 'all' && Object.keys(b1).every(a => sendsSince(b1, a).length === 0));
      imp({ macros: [{ name: 'FixGhost', fixed: ['profile-missing'], steps: [{ type: 'send', text: 'status' }] }] });
      const fg = snap(w).macros.find(m => m.name === 'FixGhost');
      ok('unresolved fixed id kept + reported (chip 고정 · 미해결, toast)', fg.unresolvedFixed === 1 && /미해결 profile-missing\(저장된 세션 없음\)/.test(fg.targetLabel) && /고정 · 미해결/.test(w.document.querySelector(`.mac[data-mid="${fg.id}"] .chip.fixed.unresolved`).textContent) && /1개 미해결/.test(toastTxt()), { fg, toast: toastTxt() });
      b1 = base(); click(w, `.mac[data-mid="${fg.id}"] [data-act="run-macro"]`); await sleep(200);
      ok('unresolved fixed: ▶ sends nothing anywhere (never falls back to panel scope)', Object.keys(b1).every(a => sendsSince(b1, a).length === 0)); }
    // one scope rule for every entry point: pane macro menu obeys 연결된 모두 / 선택 (never collapses to that pane)
    { const paneRun = (tid, name) => { click(w, `.tab[data-tab="${tid}"]`); click(w, `[data-act="macros-menu"][data-tab="${tid}"]`); const hdr = w.document.querySelector('.menu .mh').textContent; [...w.document.querySelectorAll('.menu [data-mi]')].find(x => x.textContent.includes(name)).dispatchEvent(new w.MouseEvent('click', { bubbles: true })); return hdr; };
      const expAll = snap(w).macroScope.recipients.slice().sort();
      b0 = base(); const h1 = paneRun(mcu.id, 'GRP'); await sleep(1500);
      ok('pane menu in 연결된 모두: header shows scope, runs on every scope recipient', /실행 대상: 연결된 모두 →/.test(h1) && expAll.length >= 2 && expAll.every(a => JSON.stringify(sendsSince(b0, a)) === '["version","status"]'), { h1, expAll });
      click(w, '.mscope [data-act="ms-mode"][data-mode="selected"]'); // selection still mcu + console
      const expSel = snap(w).macroScope.recipients.slice().sort();
      b0 = base(); const h2 = paneRun(tab('aux').id, 'GRP'); await sleep(1500);
      ok('pane menu in 선택 (opened from aux pane): runs only the selected sessions, not aux', /실행 대상: 선택 →/.test(h2) && JSON.stringify(expSel) === '["console","mcu"]' && expSel.every(a => JSON.stringify(sendsSince(b0, a)) === '["version","status"]') && sendsSince(b0, 'aux').length === 0, { h2, expSel, aux: sendsSince(b0, 'aux') });
      click(w, '.mscope [data-act="ms-mode"][data-mode="all"]'); }
    click(w, '[data-cmd="bc-current"]');
    const tx2 = Object.fromEntries(snap(w).tabs.map(t => [t.alias, t.tx]));
    click(w, `.tab[data-tab="${mcu.id}"]`); typeIn(mcu.id, 'help'); await sleep(150);
    const tx3 = Object.fromEntries(snap(w).tabs.map(t => [t.alias, t.tx]));
    ok('macro scope 연결된 모두 does not make typing broadcast (keyboard Current -> only focused)', snap(w).macroScope.mode === 'all' && tx3.mcu > tx2.mcu && Object.keys(tx2).filter(a => a !== 'mcu').every(a => tx3[a] === tx2[a]), { tx2, tx3 });
    click(w, '[data-cmd="bc-all"]'); // leave All ON to prove reload resets it
  }
  // sidebar explicit toggle + group collapse persist
  click(w, '[data-cmd="toggle-side"]');
  ok('toolbar toggle collapses sidebar (aria-expanded=false)', w.document.querySelector('#modUart').classList.contains('side-collapsed') && w.document.querySelector('#tbSide').getAttribute('aria-expanded') === 'false');
  click(w, '[data-cmd="toggle-side"]');
  ok('toolbar toggle reopens sidebar', !w.document.querySelector('#modUart').classList.contains('side-collapsed'));
  click(w, '#railSessions'); // panel was on Macros view (after macro save) -> switches to Sessions, stays open
  ok('rail: Sessions view shown open', !w.document.querySelector('#modUart').classList.contains('side-collapsed') && w.document.querySelector('#railSessions').getAttribute('aria-expanded') === 'true');
  click(w, '.group-h[data-group="Gateway SoM"]');
  ok('group collapses independently', w.document.querySelector('.group-h[data-group="Gateway SoM"]').getAttribute('aria-expanded') === 'false' && w.document.querySelector('.group-h[data-group="ECU bench A"]').getAttribute('aria-expanded') === 'true');
  click(w, '#railSessions'); // same rail button again -> collapses the same view
  ok('rail: pressing active Sessions again collapses', w.document.querySelector('#modUart').classList.contains('side-collapsed'));
  // persistence: reload with same storage
  await sleep(300);
  const keys = {}; for (let i = 0; i < w.localStorage.length; i++) { const k = w.localStorage.key(i); keys[k] = w.localStorage.getItem(k); }
  ok('only own prefix keys written (+unrelated preserved)', Object.keys(keys).filter(k => k !== 'unrelated.key').every(k => k.startsWith('loomUartOpus.v1.')) && keys['unrelated.key'] === 'keep-me', Object.keys(keys));
  const layoutBefore = JSON.stringify(snap(w).layout, (k, v) => k === 'rect' || k === 'leaf' || k === 'split' ? undefined : v);
  w.close();
  { const ms0 = JSON.parse(keys['loomUartOpus.v1.macros']); ms0.push({ id: 'm-legacy', name: 'LegacyFixed', target: 's-console', confirm: true, hotkey: '', pinned: false, vars: [], steps: [{ type: 'send', text: 'uptime', eol: 'session' }] }); keys['loomUartOpus.v1.macros'] = JSON.stringify(ms0); } // pre-REV6 stored shape
  w = boot(keys); await sleep(500);
  s = snap(w);
  ok('reload: macro scope resets to 활성 (not persisted)', s.macroScope.mode === 'active');
  ok('reload: stored legacy target migrated to visible fixed override', JSON.stringify(s.macros.find(m => m.name === 'LegacyFixed').fixed) === '["console"]' && /^고정 → /.test(s.macros.find(m => m.name === 'LegacyFixed').targetLabel), s.macros.find(m => m.name === 'LegacyFixed'));
  ok('reload: sidebar collapsed + group state persisted', w.document.querySelector('#modUart').classList.contains('side-collapsed') && JSON.parse(w.localStorage.getItem('loomUartOpus.v1.ui')).closedGroups.includes('Gateway SoM'));
  ok('reload: recording never auto-resumes (prefs only persisted)', snap(w).tabs.every(t => t.recState === 'idle') && !!w.localStorage.getItem('loomUartOpus.v1.recPrefs') && !/data|chunks/.test(w.localStorage.getItem('loomUartOpus.v1.recPrefs')));
  ok('reload: send target resets to Current (no implicit All)', snap(w).broadcast.on === false);
  ok('reload: macro persisted', s.macros.some(m => m.name === 'Slow probe v2' && m.steps === 3));
  ok('reload: layout + tabs restored', JSON.stringify(s.layout, (k, v) => k === 'rect' || k === 'leaf' || k === 'split' ? undefined : v) === layoutBefore, s.layout);
  ok('reload: aux theme light restored, others dark', s.tabs.find(t => t.alias === 'aux').theme === 'light' && s.tabs.filter(t => t.alias !== 'aux').every(t => t.theme === 'dark'));
  // close panes down to last
  for (let i = 0; i < 6 && snap(w).panes > 1; i++) click(w, '.pane[data-pane-no="1"] [data-act="close-pane"]');
  click(w, '.pane[data-pane-no="1"] [data-act="close-pane"]');
  s = snap(w); ok('last pane close keeps 1 empty pane with picker', s.panes === 1 && s.tabs.length === 0 && !!w.document.querySelector('.picker'));
  // import JSON
  click(w, '[data-cmd="import"]');
  w.document.querySelector('#dlgConfirm [name=json]').value = JSON.stringify({ format: 'loom-uart-bench', version: 1, macros: [{ name: 'Imported AT', steps: [{ type: 'send', text: 'AT' }, { type: 'wait', text: 'OK', ms: 1000 }], hotkey: 'F2' }] });
  w.document.querySelector('#dlgConfirm form').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  s = snap(w); const im = s.macros.find(m => m.name === 'Imported AT');
  ok('import adds macro, conflicting hotkey cleared', im && im.hotkey === '' , im);
  w.close();
  const fails = results.filter(r => !r.pass).length;
  console.log(`\n${results.length - fails}/${results.length} PASS`);
  fs.writeFileSync(path.join(process.argv[3], 'selfcheck-result.json'), JSON.stringify({ when: new Date().toISOString(), env: 'jsdom (ui/node_modules), not a browser; geometry & pointer drag NOT covered', results }, null, 2));
  process.exit(fails ? 1 : 0);
})().catch(e => { console.error('CRASH', e); process.exit(2); });
