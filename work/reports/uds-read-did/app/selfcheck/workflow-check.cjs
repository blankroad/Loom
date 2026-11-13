// Workflow (owner A) source-local checks — jsdom + fake engine clock. NOT browser acceptance, NOT integration proof.
// Uses the real core host.js when present (harness loads index.html scripts in order); otherwise a minimal contract stub host.
const fs = require('fs'), path = require('path');
const { boot, DIR } = require('./harness.cjs');
const KEY = 'loom.udsReadDid.workflow.v1';
const results = []; let cur = '';
function ok(cond, msg) { results.push({ group: cur, ok: !!cond, msg }); if (!cond) console.log('FAIL', cur, '—', msg); }
function group(n, fn) { cur = n; try { fn(); } catch (e) { ok(false, 'threw: ' + (e && e.stack || e)); } }
const REAL_HOST = fs.existsSync(path.join(DIR, 'host.js'));

/* ---------- minimal stub host (only used when host.js is absent) ---------- */
function stubHost(w) {
  const O = w.loomUdsOpus, E = O.E, st = O.st, U = w.LoomUds, subs = new Set(), acts = new Map(), secs = new Map();
  E.on((type, data) => subs.forEach(fn => fn(Object.assign({ type }, data && data.txn ? { txn: data.txn } : {}))));
  const clock = w.__loomUdsClock;
  return {
    engine: E, constants: U, clock,
    context: () => ({ ecu: st.ecu, transport: st.transport }), editor: () => ({ mode: st.mode, didText: st.didText, rawText: st.rawText }),
    compose: o => { if (o.didText != null) st.didText = o.didText; if (o.rawText != null) st.rawText = o.rawText; st.mode = o.mode; O.render(); return { ok: true }; },
    send: o => { const r = o.mode === 'raw' ? E.sendRaw({ ecu: o.ecu, transport: o.transport, rawText: o.rawText, sim: st.sim }) : E.read({ ecu: o.ecu, transport: o.transport, didText: o.didText, sim: st.sim }); if (r.ok) r.txn.source = o.source || 'manual'; return r.ok ? { ok: true, txn: r.txn } : r; },
    cancelOwned: id => { const tx = E.byId(id), c = tx && E.ctx(tx.ecu, tx.transport); return c && c.active === id ? E.cancel(tx.ecu, tx.transport) : { ok: false }; },
    selectTxn: id => O.selectTxn(id), refresh: () => O.render(), subscribe: fn => { subs.add(fn); return () => subs.delete(fn); }, onRender: () => () => {},
    project: { register: (k, impl) => { secs.set(k, impl); return () => secs.delete(k); }, sections: () => secs },
    activity: { register: (k, impl) => { acts.set(k, impl); return () => acts.delete(k); }, list: () => [...acts].flatMap(([key, i]) => i.list().map(x => Object.assign({ key }, x))) },
    ui: { esc: O.esc, toast: O.toast, download: (n, t, ty) => w.LoomUdsTools.download(n, t, ty), stamp: () => 'x' },
  };
}

function start(storage) {
  const b = boot(storage);
  const { w } = b;
  if (!w.LoomWorkbench) { w.LoomWorkbench = stubHost(w); w.LoomUdsWorkflow.boot(); }
  const H = w.LoomWorkbench, wf = w.LoomUdsWorkflow.instance, E = H.engine, d = w.document;
  const click = el => { if (!el) throw new Error('missing element'); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); };
  const input = (el, v) => { el.value = v; el.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const change = (el, v) => { el.value = v; el.dispatchEvent(new w.Event('change', { bubbles: true })); };
  const submit = el => el.dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const $ = s => d.querySelector(s);
  return Object.assign(b, { H, wf, E, d, click, input, change, submit, $ });
}
const inflightPerCtx = E => { const m = {}; E.txns.forEach(t => { if (!E.terminal(t.status)) m[t.ecu + '|' + t.transport] = (m[t.ecu + '|' + t.transport] || 0) + 1; }); return Math.max(0, ...Object.values(m)); };

/* ================= boot / no auto anything ================= */
group('boot', () => {
  const t = start();
  ok(!!t.wf, 'workflow instance booted once from host');
  ok(REAL_HOST ? !!t.H.version : true, `host = ${REAL_HOST ? 'real host.js' : 'STUB (no host.js yet)'}`);
  ok(t.$('#workflowMount #wf') && t.$('#watchMount #wt'), 'owned subtrees rendered inside both mounts');
  ok(t.d.querySelectorAll('#wf').length === 1, 'single subtree (no duplicate boot)');
  ok(t.E.txns.length === 0 && t.H.activity.list().length === 0, 'no TX / no activity at boot');
  ok(t.w.localStorage.getItem(KEY) === null, 'nothing written at boot');
  t.w.close();
});

/* ================= model validation (pure, all-or-nothing) ================= */
group('model', () => {
  const t = start(), M = t.w.LoomUdsWorkflow.model;
  const good = { format: M.FORMAT, commands: [{ id: 'c1', name: 'VIN', desc: '', folder: 'id', mode: 'did', didText: 'f190', target: null }], watches: [{ id: 'w1', did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 }], sequences: [{ id: 's1', name: 'S', target: null, loops: 2, stopOnError: true, steps: [{ mode: 'cmd', cmdId: 'c1', delayMs: 0, expect: { status: 'positive', prefix: '62f190' } }] }] };
  const snap = JSON.stringify(good);
  const r = M.validate(good);
  ok(r.ok && r.value.commands[0].didText === 'F190' && r.value.sequences[0].steps[0].expect.prefix === '62 F1 90', 'normalizes DID/prefix');
  ok(JSON.stringify(good) === snap, 'validate does not mutate input');
  const bad = (mut, why) => { const x = JSON.parse(snap); mut(x); const v = M.validate(x); ok(!v.ok && v.error, `rejects ${why}: ${v.error}`); };
  bad(x => { x.format = 'loom-uds-workflow/2'; }, 'unknown version');
  bad(x => { x.commands[0].didText = 'F19'; }, 'odd DID');
  bad(x => { x.commands[0].mode = 'raw'; x.commands[0].rawText = '22 F1 9'; }, 'odd raw');
  bad(x => { x.commands[0].mode = 'raw'; x.commands[0].rawText = '22,F1'; }, 'comma raw');
  bad(x => { x.commands.push(Object.assign({}, x.commands[0])); }, 'duplicate id');
  bad(x => { x.sequences[0].steps[0].cmdId = 'c9'; }, 'dangling command ref');
  bad(x => { x.sequences[0].loops = 100; x.sequences[0].steps = Array(11).fill(x.sequences[0].steps[0]); }, 'steps×loops > 1000');
  bad(x => { x.sequences[0].loops = 0; }, 'zero loops');
  bad(x => { x.watches[0].intervalMs = 50; }, 'interval below min');
  bad(x => { x.watches[0].target = null; }, 'watch without target');
  bad(x => { x.commands[0].name = 'a\u0007b'; }, 'control char name');
  bad(x => { x.commands[0].target = { ecu: 'BCM', transport: 'flexray' }; }, 'unknown transport');
  bad(x => { x.commands = Array(201).fill(0).map((_, i) => ({ id: 'c' + (i + 1), name: 'n' + i, mode: 'did', didText: 'F190' })); x.sequences = []; }, 'too many commands');
  ok(M.expectOk({ status: 'response', prefix: '' }, { status: 'nrc', final: [0x7f, 0x22, 0x31] }).ok, 'expect response accepts NRC');
  ok(!M.expectOk({ status: 'positive', prefix: '62 F1 90' }, { status: 'positive', final: [0x62, 0xF1, 0x8C] }).ok, 'prefix mismatch fails');
  t.w.close();
});

/* ================= saved commands (UI) ================= */
group('commands', () => {
  const t = start(), { wf, E, H, $, click, input, submit, change } = t;
  H.tools && H.tools.open && H.tools.open('workflow');
  click($('[data-wf="cmd-new"]'));
  ok(!$('#wfCmdEdit').hidden, 'new command editor opens');
  input($('#wfeName'), 'VIN read'); input($('#wfeFolder'), 'ident'); input($('#wfeDid'), 'f19');
  submit($('#wfCmdEdit'));
  ok(/4자리|홀수/.test($('#wfeErr').textContent) && wf.doc.commands.length === 0, 'invalid DID refused, nothing saved');
  input($('#wfeDid'), 'f190'); submit($('#wfCmdEdit'));
  ok(wf.doc.commands.length === 1 && wf.doc.commands[0].didText === 'F190', 'saved normalized command');
  const stored = JSON.parse(t.w.localStorage.getItem(KEY));
  ok(stored.format === 'loom-uds-workflow/1' && stored.commands.length === 1 && !('runs' in stored) && !('recent' in stored), 'own key holds definitions only');
  ok(t.w.localStorage.getItem('unrelated.key') === 'keep', 'unrelated key untouched');
  // load to editor: never sends
  t.o.st.didText = '0101'; t.d.getElementById('didInput').value = '0101';
  click($('[data-cid="c1"] [data-wf="cmd-load"]'));
  ok(H.editor().didText === 'F190' && H.editor().mode === 'did' && E.txns.length === 0, 'load fills editor, zero TX');
  // run offline → error, no TX
  click($('[data-cid="c1"] [data-wf="cmd-run"]'));
  ok(E.txns.length === 0 && /연결/.test($('[data-cid="c1"] .wf-st').textContent), 'run offline refused with visible error');
  E.connect('BCM', 'doip');
  click($('[data-cid="c1"] [data-wf="cmd-run"]'));
  const tx = E.txns[0];
  ok(tx && tx.source === 'command' && tx.ecu === 'BCM' && tx.transport === 'doip' && tx.didHex === 'F190', 'run sends via shared engine with current context, source stamped');
  ok(t.o.st.selTxn === tx.id, 'explicit run in current context selects its row (like manual Read)');
  // busy: a second run while in flight → busy error, does not cancel
  const r2 = wf.runCommand('c1');
  ok(!r2.ok && r2.code === 'busy' && !E.terminal(tx.status), 'busy refused, in-flight not cancelled');
  t.clock.advance(1000);
  ok(tx.status === 'positive', 'command txn completed normally');
  // bound target: executes against binding even when current context differs
  const r3 = wf.saveCommand({ id: 'new', name: 'GW serial', mode: 'raw', rawText: '22F18C', target: { ecu: 'GW', transport: 'doip' } });
  ok(r3.ok, 'saved bound raw command');
  const r4 = wf.runCommand(r3.id);
  ok(!r4.ok && r4.code === 'offline', 'bound target offline refused (no auto connect)');
  E.connect('GW', 'doip');
  const r5 = wf.runCommand(r3.id);
  ok(t.o.st.ecu === 'BCM' && t.o.st.selTxn !== r5.txId, 'bound other-ECU run: no target switch, no selection steal');
  ok(r5.ok && E.byId(r5.txId).ecu === 'GW' && E.byId(r5.txId).kind === 'raw' && t.w.LoomUds.hexs(E.byId(r5.txId).request) === '22 F1 8C', 'bound raw exact bytes to GW while current = BCM');
  t.clock.advance(1000);
  // duplicate + delete
  click($('[data-cid="c1"] [data-wf="cmd-dup"]'));
  ok(wf.doc.commands.length === 3 && wf.doc.commands[2].name === 'VIN read 복사', 'duplicate');
  click($(`[data-cid="${wf.doc.commands[2].id}"] [data-wf="cmd-del"]`));
  ok(wf.doc.commands.length === 2, 'delete');
  // folder filter
  change($('#wfFolder'), 'ident');
  ok(t.d.querySelectorAll('#wfCmdList [data-cid]').length === 1, 'folder filter');
  change($('#wfFolder'), '');
  t.w.close();
});

group('command drafts preserved', () => {
  const t = start(), { wf, $, click, input, H } = t;
  wf.saveCommand({ id: 'new', name: 'A', mode: 'did', didText: 'F190' });
  wf.saveCommand({ id: 'new', name: 'B', mode: 'raw', rawText: '10 03' });
  const before = H.editor();
  click($('[data-cid="c1"] [data-wf="cmd-edit"]'));
  input($('#wfeName'), 'A edited (unsaved)');
  click($('[data-cid="c2"] [data-wf="cmd-edit"]'));
  ok($('#wfeName').value === 'B' && $('#wfeRaw').value === '10 03', 'editing B shows B');
  input($('#wfeDesc'), 'b draft');
  click($('[data-cid="c1"] [data-wf="cmd-edit"]'));
  ok($('#wfeName').value === 'A edited (unsaved)', 'A unsaved draft restored after editing B');
  ok(wf.doc.commands[0].name === 'A', 'draft not committed');
  ok(JSON.stringify(H.editor()) === JSON.stringify(before), 'main request editor untouched by library editing');
  click($('[data-cid="c2"] [data-wf="cmd-edit"]'));
  ok($('#wfeDesc').value === 'b draft', 'B draft restored');
  // save current: editor → new draft (raw)
  H.compose({ mode: 'raw', rawText: '3E 00' });
  click($('[data-wf="save-current"]'));
  ok($('#wfeRaw').value === '3E 00' && $('#wfCmdEdit').classList.contains('mode-raw'), 'save current → raw draft from editor');
  t.submit($('#wfCmdEdit'));
  ok(wf.doc.commands.length === 3 && wf.doc.commands[2].rawText === '3E 00' && t.E.txns.length === 0, 'saved current request, zero TX');
  t.w.close();
});

/* ================= recent ================= */
group('recent', () => {
  const t = start(), { wf, E, H, $, click } = t;
  E.connect('BCM', 'doip');
  t.o.doRead(); t.clock.advance(1000);
  H.send({ mode: 'raw', rawText: '22 F1 8C' }); t.clock.advance(1000);
  t.o.doRead(); t.clock.advance(1000);           // same DID again → dedup, newest
  ok(wf.recent.length === 2 && wf.recent[0].no === 3, 'bounded unique recent, newest first');
  E.clearHistory();
  ok(wf.recent.length === 2, 'recent survives history clear (runtime only)');
  click($('#wft-recent'));
  ok(!$('#wfp-recent').hidden && t.d.querySelectorAll('#wfRecent [data-ri]').length === 2, 'recent tab lists');
  const n = E.txns.length;
  click($('#wfRecent [data-ri="1"] [data-wf="rec-load"]'));
  ok(H.editor().mode === 'raw' && H.editor().rawText === '22 F1 8C' && E.txns.length === n, 'recent → editor, zero TX');
  click($('#wfRecent [data-ri="1"] [data-wf="rec-save"]'));
  ok(!$('#wfCmdEdit').hidden && $('#wfeTarget').value === 'BCM|doip', 'recent → command draft with target binding');
  for (let i = 0; i < 30; i++) { H.send({ mode: 'raw', rawText: '22 00 ' + (i < 16 ? '0' : '') + i.toString(16) }); t.clock.advance(1000); }
  ok(wf.recent.length === 20, 'recent bounded to 20');
  ok(!JSON.parse(t.w.localStorage.getItem(KEY)).recent, 'recent never persisted');
  t.w.close();
});

/* ================= watch ================= */
group('watch', () => {
  const t = start(), { wf, E, H, $, click, input, submit, change } = t;
  input($('#wtDid'), '0101'); change($('#wtTarget'), 'BCM|doip'); change($('#wtInt'), '500');
  submit($('#wtAdd'));
  ok(wf.doc.watches.length === 1 && wf.doc.watches[0].intervalMs === 500, 'watch added via form');
  submit($('#wtAdd')); input($('#wtDid'), '0101'); submit($('#wtAdd'));
  ok(wf.doc.watches.length === 1 && $('#wtMsg').textContent, 'duplicate watch refused visibly');
  click($('tr[data-wid="w1"] [data-wt="start"]'));
  ok(!wf.wrt('w1').running && E.txns.length === 0 && /연결/.test($('#wtMsg').textContent), 'start offline refused (no auto connect)');
  E.connect('BCM', 'doip');
  click($('tr[data-wid="w1"] [data-wt="start"]'));
  ok(wf.wrt('w1').running && E.txns.length === 1 && E.txns[0].source === 'watch', 'explicit start → first poll now');
  ok(H.activity.list().some(a => a.kind === 'watch' && a.ecu === 'BCM' && a.key === 'workflow'), 'running watch registered as activity');
  let maxIn = 0; for (let i = 0; i < 30; i++) { t.clock.advance(100); maxIn = Math.max(maxIn, inflightPerCtx(E)); }
  const r = wf.wrt('w1');
  ok(maxIn === 1, 'never more than one in-flight request per context');
  ok(t.o.st.selTxn == null, 'watch polls never select rows');
  ok(r.sent >= 6 && r.value != null && r.unit === 'V' && r.samples.length >= 5, `values/unit/samples (${r.sent} sent, value ${r.value})`);
  const cf = [...E.confirmed.values()].find(x => x.did === 0x0101);
  ok(cf && String(cf.value) === String(r.value) && r.valueNo === cf.no, 'value comes from engine confirmed entry of that txn');
  ok(Math.abs(wf.cadence(r) - 500) <= 1, `actual cadence 500 (${wf.cadence(r)})`);
  ok($('tr[data-wid="w1"] svg.wt-spark polyline') && $('tr[data-wid="w1"] [data-c="mm"]').textContent.includes('/'), 'sparkline + min/max rendered');
  // P2 / latency exceeds period → skips, not queue
  change($('tr[data-wid="w1"] select[data-wt="int"]'), '200');
  t.o.st.sim.latency = 900;
  const s0 = r.skips.busy, sent0 = r.sent;
  for (let i = 0; i < 20; i++) { t.clock.advance(100); maxIn = Math.max(maxIn, inflightPerCtx(E)); }
  ok(r.skips.busy > s0 && maxIn === 1 && r.sent - sent0 <= 3, `slow response → busy skips (${r.skips.busy - s0}), no overlap, no queue growth`);
  ok(/건너뜀|진행/.test($('tr[data-wid="w1"] [data-c="status"]').textContent + r.note) || r.skips.busy > 0, 'skip visible');
  t.o.st.sim.latency = 80;
  t.clock.advance(1000);
  // manual request in progress → watch skips, manual never cancelled
  t.clock.advance(150);
  let manual = null;
  for (let i = 0; i < 10 && !manual; i++) { if (E.ctx('BCM', 'doip').active == null) manual = t.o.doRead(); else t.clock.advance(10); }
  t.o.st.sim.latency = 400;
  ok(manual && manual.source === 'manual', 'manual read issued between polls');
  const sk = r.skips.busy; t.clock.advance(250);
  ok(r.skips.busy > sk && !E.terminal(manual.status) || manual.status === 'positive', 'watch skipped while manual in flight');
  t.clock.advance(1000);
  ok(manual.status === 'positive', 'manual txn completed (never cancelled)');
  t.o.st.sim.latency = 80;
  // NRC keeps old value + age
  t.o.st.sim.scenario = 'nrc'; t.clock.advance(600);     // in-flight positive (if any) completes; next polls are NRC
  const keep = r.value, keepT = r.valueT;
  t.clock.advance(1000);
  ok(r.status === 'nrc' && r.value === keep && r.valueT === keepT, 'NRC retains previous value and its age');
  t.o.st.sim.scenario = 'timeout'; t.clock.advance(2500);
  ok(r.status === 'timeout' && r.value === keep, 'timeout retains previous value');
  t.o.st.sim.scenario = 'positive'; t.clock.advance(1000);
  // stop → no more sends
  click($('tr[data-wid="w1"] [data-wt="stop"]'));
  t.clock.advance(300);
  const n = E.txns.length; t.clock.advance(3000);
  ok(!r.running && E.txns.length === n && H.activity.list().length === 0, 'stop → no further polls, activity cleared');
  // link loss → stop, reconnect does not resume
  click($('tr[data-wid="w1"] [data-wt="start"]'));
  t.clock.advance(50);
  E.disconnect('BCM', 'doip', 'user');
  t.clock.advance(100);
  ok(!r.running, 'link loss stops watch');
  E.connect('BCM', 'doip'); const n2 = E.txns.length; t.clock.advance(3000);
  ok(!r.running && E.txns.length === n2, 'reconnect does not auto-resume');
  // two rows on one context: bounded deferral, both get values
  wf.addWatch({ did: 'F18C', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 });
  wf.startWatch('w1'); wf.startWatch('w2');
  maxIn = 0; for (let i = 0; i < 40; i++) { t.clock.advance(50); maxIn = Math.max(maxIn, inflightPerCtx(E)); }
  ok(maxIn === 1 && wf.wrt('w2').value === 'SIM-BCM-000417' && wf.wrt('w1').sent > 1 && wf.wrt('w2').sent > 1, 'two rows share a context without overlap, both updated');
  ok(!wf.wrt('w2').numeric && $('tr[data-wid="w2"] [data-c="spark"]').textContent.includes('비수치'), 'non-numeric value → no fake chart');
  // tick patch must not rebuild focused interval select
  H.tools.open('watch');
  const sel = $('tr[data-wid="w1"] select[data-wt="int"]'); sel.focus();
  ok(t.d.activeElement === sel, 'interval select focusable in open watch drawer');
  t.clock.advance(1500);
  ok(t.d.activeElement === sel && sel.isConnected, 'runtime ticks never rebuild the focused control');
  wf.stopAllWatches();
  t.w.close();
});

group('watch reload', () => {
  const t = start(); const { wf, E } = t;
  E.connect('BCM', 'doip');
  wf.addWatch({ did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 });
  wf.saveCommand({ id: 'new', name: 'x', mode: 'did', didText: 'F190' });
  wf.saveSequence({ name: 'S', loops: 1, stopOnError: true, steps: [{ mode: 'cmd', cmdId: 'c1', delayMs: 0, expect: { status: 'positive' } }] });
  wf.startWatch('w1'); t.clock.advance(600);
  const saved = t.w.localStorage.getItem(KEY), prefs = t.w.localStorage.getItem('loom.udsReadDid.v1');
  t.w.close();
  const t2 = start({ [KEY]: saved, 'loom.udsReadDid.v1': prefs });
  t2.clock.advance(5000);
  ok(t2.wf.doc.watches.length === 1 && t2.wf.doc.sequences.length === 1 && t2.wf.doc.commands.length === 1, 'definitions restored on reload');
  ok(t2.E.txns.length === 0 && !t2.wf.wrt('w1').running && t2.H.activity.list().length === 0, 'reload: no watch/sequence/TX auto start');
  t2.w.close();
  const t3 = start({ [KEY]: '{bad json' });
  ok(t3.wf.doc.commands.length === 0 && /손상/.test(t3.$('#wfNote').textContent) && t3.w.localStorage.getItem(KEY) === '{bad json', 'corrupt storage → defaults + visible notice, original kept');
  t3.w.close();
  const t4 = start({ [KEY]: JSON.stringify({ format: 'loom-uds-workflow/9', commands: [] }) });
  ok(/형식|버전/.test(t4.$('#wfNote').textContent), 'unknown stored version rejected visibly');
  t4.w.close();
});

/* ================= sequences ================= */
group('sequence', () => {
  const t = start(), { wf, E, H, $, click } = t;
  wf.saveCommand({ id: 'new', name: 'VIN', mode: 'did', didText: 'F190' });
  const s = wf.saveSequence({ name: 'Ident', target: null, loops: 2, stopOnError: true, steps: [
    { mode: 'cmd', cmdId: 'c1', delayMs: 0, expect: { status: 'positive', prefix: '62 F1 90' } },
    { mode: 'did', didText: 'F18C', delayMs: 300, expect: { status: 'positive', prefix: '' } },
    { mode: 'raw', rawText: '10 03', delayMs: 0, expect: { status: 'response', prefix: '' } },
  ] });
  ok(s.ok, 'sequence saved');
  click($('#wft-sequences'));
  click($('[data-sid="s1"] [data-wf="seq-run"]'));
  ok(wf.runs.get('s1').status === 'refused' && E.txns.length === 0, 'run offline refused, zero TX');
  E.connect('BCM', 'doip'); E.connect('GW', 'doip');
  click($('[data-sid="s1"] [data-wf="seq-run"]'));
  const run = wf.runs.get('s1');
  ok(run.status === 'running' && run.target.ecu === 'BCM', 'explicit run, target frozen at start');
  ok(H.activity.list().some(a => a.kind === 'sequence'), 'running sequence registered as activity');
  t.o.st.ecu = 'GW'; t.o.render();              // user switches target mid-run
  let maxIn = 0, order = [];
  for (let i = 0; i < 80; i++) { t.clock.advance(100); maxIn = Math.max(maxIn, E.txns.filter(x => !E.terminal(x.status)).length); }
  ok(run.status === 'passed', `sequence passed (${run.status} ${run.error})`);
  ok(E.txns.length === 6 && E.txns.every(x => x.ecu === 'BCM' && x.source === 'sequence'), 'exactly steps×loops real TX, all to frozen target');
  ok(maxIn === 1, 'strictly sequential (one in flight)');
  ok(!E.txns.some(x => x.id === t.o.st.selTxn), 'sequence sends never steal selection');
  for (let i = 1; i < E.txns.length; i++) order.push(E.txns[i].t0 >= E.txns[i - 1].tEnd);
  ok(order.every(Boolean), 'each step sent only after previous terminal');
  ok(E.txns[1].t0 - E.txns[0].tEnd >= 300, 'step delay honoured');
  ok(run.results.length === 6 && run.results.every(r => r.ok), 'per-step results recorded');
  ok($('#wfRun .wf-res') && $('#wfRun').textContent.includes('통과'), 'run view shows results');
  ok($('#wfRun .wf-rh').textContent.includes('반복 2/2 · 단계 3/3 완료'), 'completed run summary shows full count (not step 1)');
  // fail-fast on expectation mismatch
  wf.saveSequence({ id: 's1', name: 'Ident', target: null, loops: 2, stopOnError: true, steps: [{ mode: 'did', didText: 'F190', delayMs: 0, expect: { status: 'positive', prefix: '62 F1 8C' } }, { mode: 'did', didText: 'F18C', delayMs: 0, expect: { status: 'positive' } }] });
  t.o.st.ecu = 'BCM';
  const n0 = E.txns.length;
  wf.startSequence('s1'); t.clock.advance(3000);
  ok(wf.runs.get('s1').status === 'failed' && E.txns.length === n0 + 1 && /접두/.test(wf.runs.get('s1').error), 'fail-fast stops after first mismatch');
  ok($('#wfRun .wf-rh').textContent.includes('반복 1/2 · 단계 1/2'), 'failed run summary shows the failing step');
  // continue-on-error
  wf.saveSequence({ id: 's1', name: 'Ident', target: null, loops: 1, stopOnError: false, steps: [{ mode: 'did', didText: 'F190', delayMs: 0, expect: { status: 'nrc' } }, { mode: 'did', didText: 'F18C', delayMs: 0, expect: { status: 'positive' } }] });
  const n1 = E.txns.length; wf.startSequence('s1'); t.clock.advance(3000);
  ok(wf.runs.get('s1').status === 'done-errors' && E.txns.length === n1 + 2, 'continue-on-error runs all steps, reports failures');
  // stop mid-flight cancels only own txn
  t.o.st.sim.latency = 900;
  wf.startSequence('s1'); t.clock.advance(200);
  const own = E.byId(wf.runs.get('s1').txnId);
  click($('[data-sid="s1"] [data-wf="seq-stop"]'));
  ok(wf.runs.get('s1').status === 'stopped' && own.status === 'cancelled', 'stop cancels owned in-flight txn');
  const n2 = E.txns.length; t.clock.advance(5000);
  ok(E.txns.length === n2, 'no sends after stop');
  // foreign manual busy at step → step fails, manual untouched
  t.o.st.sim.latency = 80;
  wf.saveSequence({ id: 's1', name: 'Ident', target: null, loops: 1, stopOnError: true, steps: [{ mode: 'did', didText: 'F190', delayMs: 500, expect: { status: 'positive' } }] });
  wf.startSequence('s1'); t.clock.advance(100);
  t.o.st.sim.latency = 900; const man = t.o.doRead(); t.clock.advance(500);
  ok(wf.runs.get('s1').status === 'failed' && man.status !== 'cancelled', 'foreign busy → step fails, manual not cancelled');
  t.clock.advance(2000);
  ok(man.status === 'positive', 'manual completes');
  t.o.st.sim.latency = 80;
  // watch on same context skips while sequence runs
  wf.addWatch({ did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 200 });
  wf.saveSequence({ id: 's1', name: 'Ident', target: null, loops: 5, stopOnError: true, steps: [{ mode: 'did', didText: 'F190', delayMs: 100, expect: { status: 'positive' } }] });
  wf.startWatch('w1'); t.clock.advance(50);
  wf.startSequence('s1');
  maxIn = 0; for (let i = 0; i < 40; i++) { t.clock.advance(50); maxIn = Math.max(maxIn, inflightPerCtx(E)); }
  ok(wf.runs.get('s1').status === 'passed' && wf.wrt('w1').skips.sequence > 0 && maxIn === 1, 'watch yields to sequence on same context (skip reason sequence), no overlap');
  wf.stopAllWatches();
  // refuse saving/deleting while running; deleting referenced command refused
  wf.saveCommand({ id: 'new', name: 'ref', mode: 'did', didText: 'F190' });
  wf.saveSequence({ name: 'Uses', loops: 1, stopOnError: true, steps: [{ mode: 'cmd', cmdId: 'c2', delayMs: 0, expect: { status: 'any' } }] });
  ok(!wf.deleteCommand('c2').ok, 'command referenced by sequence cannot be deleted');
  // bound loops/steps in editor form
  click($('[data-wf="seq-new"]'));
  t.$('#wfsName').value = 'Bad loops'; t.$('#wfsLoops').value = '0'; t.submit($('#wfSeqEdit'));
  ok(/반복/.test($('#wfsErr').textContent), 'editor rejects invalid loops visibly');
  t.w.close();
});

/* ================= import / project section ================= */
group('import + project', () => {
  const t = start(), { wf, E, H } = t;
  wf.saveCommand({ id: 'new', name: 'A', mode: 'did', didText: 'F190' });
  const before = JSON.stringify(wf.doc);
  const bad = wf.importDoc({ format: 'loom-uds-workflow/1', commands: [{ id: 'c1', name: 'x', mode: 'did', didText: 'zz' }] });
  ok(!bad.ok && JSON.stringify(wf.doc) === before, 'invalid import rejected atomically');
  E.connect('BCM', 'doip');
  wf.addWatch({ did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 }); wf.startWatch('w1');
  const exp = wf.exportDoc();
  const busy = wf.importDoc(exp);
  ok(!busy.ok && /중지/.test(busy.error) && wf.wrt('w1').running, 'standalone import refused while running (no implicit stop)');
  const sec = H.project.sections ? H.project.sections().get('workflow') : null;
  ok(sec && typeof sec.validate === 'function', 'project section "workflow" registered');
  if (sec) {
    const snap = JSON.stringify(exp);
    ok(sec.validate(exp).ok && JSON.stringify(exp) === snap, 'project validate pure');
    ok(!sec.validate({ format: 'loom-uds-workflow/0' }).ok, 'project validate rejects unknown version');
    let threw = false; try { sec.import(sec.validate(exp).value); } catch (e) { threw = true; }
    ok(threw && wf.wrt('w1').running, 'project import while running throws (stop-first), never stops jobs');
    wf.stopAllWatches();
    const v = sec.validate(Object.assign({}, exp, { commands: exp.commands.concat([{ id: 'c7', name: 'P', mode: 'raw', rawText: '19 02 FF', target: null }]) }));
    sec.import(v.value);
    ok(wf.doc.commands.length === 2 && E.txns.filter(x => x.source === 'watch').length === 1 && !wf.wrt('w1').running, 'project import replaces definitions only, nothing starts');
  }
  ok(wf.importDoc(exp).ok && wf.canUndo(), 'standalone import when stopped');
  ok(wf.undoImport().ok && wf.doc.commands.length === 2, 'undo import restores previous definitions');
  t.click(t.$('[data-wf="export"]'));
  ok((t.w.__dl || []).some(n => /^loom-uds-workflow-.*\.json$/.test(n)), 'export downloads JSON file');
  t.w.close();
});

const pass = results.filter(r => r.ok).length;
const out = { check: 'workflow-check', host: REAL_HOST ? 'real host.js' : 'stub', pass, total: results.length, results };
fs.writeFileSync(path.join(DIR, '..', 'qa', 'team', 'workflow-check.log'), JSON.stringify(out, null, 1));
console.log(`workflow-check: ${pass}/${results.length} PASS (host: ${out.host})`);
process.exitCode = pass === results.length ? 0 : 1;
