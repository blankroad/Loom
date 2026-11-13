// Project coordinator (item 10, owner A) source-local checks — jsdom + fake engine clock. NOT browser acceptance.
const fs = require('fs'), path = require('path');
const { boot, DIR } = require('./harness.cjs');
const CORE_KEY = 'loom.udsReadDid.v1', WF_KEY = 'loom.udsReadDid.workflow.v1';
const results = []; let cur = '';
function ok(cond, msg) { results.push({ group: cur, ok: !!cond, msg }); if (!cond) console.log('FAIL', cur, '—', msg); }
function group(n, fn) { cur = n; try { fn(); } catch (e) { ok(false, 'threw: ' + (e && e.stack || e)); } }

function start(storage) {
  const b = boot(storage), { w } = b, H = w.LoomWorkbench, d = w.document;
  const P = w.LoomUdsProject && w.LoomUdsProject.instance, ui = w.LoomUdsProject && w.LoomUdsProject.ui;
  const click = el => { if (!el) throw new Error('missing element'); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); };
  return Object.assign(b, { H, E: H.engine, P, ui, d, click, $: s => d.querySelector(s), wf: w.LoomUdsWorkflow.instance });
}
// Full observable definition state: core + every extension export + every persisted key.
function state(t) {
  const ext = {}; for (const [k, impl] of t.H.project.sections()) ext[k] = impl.export();
  const ls = {}; for (let i = 0; i < t.w.localStorage.length; i++) { const k = t.w.localStorage.key(i); ls[k] = t.w.localStorage.getItem(k); }
  return JSON.stringify({ core: t.H.coreProject.export(), ext, ls, ecus: t.H.constants.ECUS.map(e => e.id), dids: t.H.constants.DIDS.map(x => x.did) });
}
const memState = t => { const s = JSON.parse(state(t)); delete s.ls; return JSON.stringify(s); };
// Selective storage failure (quota) for chosen keys; times = how many writes fail (Infinity = permanent).
function quota(t, keys, times) {
  const proto = t.w.Storage.prototype, orig = proto.setItem; let left = times;
  proto.setItem = function (k, v) { if (keys.includes(k) && left > 0) { left--; const e = new Error('QuotaExceededError'); e.name = 'QuotaExceededError'; throw e; } return orig.call(this, k, v); };
  return () => { proto.setItem = orig; };
}
// Build a rich custom state: custom ECU + DID + fixture + profile, workflow defs bound to custom ECU, trace/layout prefs.
function customize(t) {
  const { E, H, wf } = t, C = E.catalog;
  const a = C.addEcu({ id: 'ABS', name: 'ABS Module (SIM)', doipLA: '1020', canTx: '730', canRx: '738' });
  const dd = C.addDid({ did: 'D001', name: 'Wheel speed', long: 'SIM wheel speed', type: 'uint', recLen: 2, byteOffset: 0, length: 2, endian: 'big', scale: '0.1', offset: '0', decimals: '1', unit: 'km/h' }, [['ABS', [0x01, 0x2C]]]);
  const p = E.saveProfile('ABS', 'doip', { targetId: 'new', name: 'ABS bench', values: { channel: 'ETH2', host: '10.0.0.5', port: '13400', testerLA: '0E00', ecuLA: '1020', p2: '1500', p2star: '6000' } });
  const c = wf.saveCommand({ id: 'new', name: 'ABS speed', folder: 'abs', mode: 'did', didText: 'D001', target: { ecu: 'ABS', transport: 'doip' } });
  const wa = wf.addWatch({ did: 'D001', target: { ecu: 'ABS', transport: 'doip' }, intervalMs: 1000 });
  const tr = H.project.sections().get('trace'), tv = tr.validate(Object.assign(tr.export(), { timeMode: 'rel' }));
  if (tv.ok) tr.import(tv.value);
  if (t.w.LoomUdsLayout && t.w.LoomUdsLayout.saveNamed) t.w.LoomUdsLayout.saveNamed('QA wide');
  return { a, dd, p, c, wa, tv };
}

/* ================= boot ================= */
group('boot', () => {
  const t = start();
  ok(t.P && t.ui && t.$('#projectMount #pj'), 'coordinator + UI mounted in #projectMount');
  ok(t.$('#pjSum table') && t.E.txns.length === 0, 'summary rendered, no TX');
  ok([...t.H.project.sections().keys()].sort().join() === 'layout,trace,workflow', 'extension sections registered: workflow/trace/layout');
  t.w.close();
});

/* ================= round trip ================= */
let fileText = null, origMem = null;
group('export + round trip', () => {
  const t = start(), { E, H, P } = t;
  const c = customize(t);
  ok(c.a.ok && c.dd.ok && c.p.ok && c.c.ok && c.wa.ok && c.tv.ok, 'custom ECU/DID/fixture/profile/workflow/trace set up');
  E.connect('BCM', 'doip'); t.o.doRead(); t.clock.advance(1000); E.disconnect('BCM', 'doip');
  t.click(t.$('[data-pj="export"]'));
  ok((t.w.__dl || []).some(n => /^loom-uds-project-.*\.json$/.test(n)) && /내보냄/.test(t.$('#pjMsg').textContent), 'export downloads JSON via real download');
  const j = P.build();
  ok(j.format === 'loom-uds-project/1' && j.version === 1 && j.sections.core && j.sections.workflow && j.sections.trace && j.sections.layout, 'file format/version/sections');
  const txt = JSON.stringify(j);
  ok(!/"txns"|"events"|"history"|"confirmed"|"link"\s*:\s*"online"/.test(txt) && !txt.includes('LOOMSIM0000000001\"') , 'no history/events/confirmed/links exported');
  ok(j.sections.core.catalog.ecus.length === 1 && !j.sections.core.catalog.ecus.some(e => e.id === 'BCM'), 'presets never exported');
  fileText = txt; origMem = memState(t);
  t.w.close();
  // fresh page: import via UI preview → explicit apply
  const t2 = start();
  ok(t2.H.constants.ECUS.every(e => e.id !== 'ABS'), 'fresh page has no custom ECU');
  t2.ui.loadText(fileText, 'qa.json');
  ok(t2.$('#pjPrev .pj-ok') && t2.$('[data-pj="apply"]') && !t2.$('[data-pj="apply"]').disabled, 'validated preview with enabled apply');
  ok(t2.H.constants.ECUS.every(e => e.id !== 'ABS'), 'preview alone mutates nothing');
  t2.click(t2.$('[data-pj="apply"]'));
  ok(/적용됨/.test(t2.$('#pjMsg').textContent), 'apply succeeded: ' + t2.$('#pjMsg').textContent);
  ok(memState(t2) === origMem, 'round trip: core + workflow + trace + layout identical');
  ok(t2.E.txns.length === 0 && t2.H.activity.list().length === 0 && t2.H.coreProject.guard().length === 0, 'import starts nothing (no TX, no activity, no link)');
  ok(!t2.wf.wrt('w1').running, 'imported watch definition not running');
  // the imported custom ECU/DID/fixture are functional through the normal engine path
  t2.E.connect('ABS', 'doip');
  const r = t2.H.send({ ecu: 'ABS', transport: 'doip', mode: 'did', didText: 'D001' }); t2.clock.advance(1000);
  ok(r.ok && r.txn.status === 'positive' && t2.w.LoomUds.hexs(r.txn.final) === '62 D0 01 01 2C' && r.txn.profile.name === 'ABS bench', 'custom ECU+profile+DID fixture answer via engine after import');
  ok(JSON.parse(t2.w.localStorage.getItem(WF_KEY)).commands[0].name === 'ABS speed', 'imported workflow persisted');
  t2.E.disconnect('ABS', 'doip');
  // reload: definitions persisted, still nothing running
  const ls = {}; for (let i = 0; i < t2.w.localStorage.length; i++) { const k = t2.w.localStorage.key(i); ls[k] = t2.w.localStorage.getItem(k); }
  t2.w.close();
  const t3 = start(ls);
  t3.clock.advance(5000);
  ok(t3.H.constants.ECUS.some(e => e.id === 'ABS') && t3.wf.doc.commands.length === 1 && t3.E.txns.length === 0, 'reload after import: definitions persist, no TX');
  t3.w.close();
});

/* ================= invalid file → zero mutation ================= */
group('invalid → zero mutation', () => {
  const t = start(); customize(t);
  const base = JSON.parse(fileText);
  const before = state(t);
  const cases = [
    ['oversize', 'x'.repeat(1024 * 1024 + 1)],
    ['bad json', '{"format":'],
    ['wrong format', JSON.stringify(Object.assign({}, base, { format: 'loom-uds-project/2' }))],
    ['wrong version', JSON.stringify(Object.assign({}, base, { version: 2 }))],
    ['no core', JSON.stringify(Object.assign({}, base, { sections: { workflow: base.sections.workflow } }))],
    ['unknown section', JSON.stringify(Object.assign({}, base, { sections: Object.assign({}, base.sections, { macros: {} }) }))],
    ['bad catalog', JSON.stringify(Object.assign({}, base, { sections: Object.assign({}, base.sections, { core: Object.assign({}, base.sections.core, { catalog: { format: 'x' } }) }) }))],
    ['profile → unknown ECU', (() => { const x = JSON.parse(fileText); x.sections.core.catalog.ecus = []; x.sections.core.catalog.dids = []; x.sections.core.catalog.fixtures = {}; return JSON.stringify(x); })()],
    ['bad workflow', (() => { const x = JSON.parse(fileText); x.sections.workflow.commands[0].didText = 'ZZ'; return JSON.stringify(x); })()],
    ['bad trace', (() => { const x = JSON.parse(fileText); x.sections.trace = { v: 9 }; return JSON.stringify(x); })()],
    ['bad layout', (() => { const x = JSON.parse(fileText); x.sections.layout = { v: 9 }; return JSON.stringify(x); })()],
  ];
  cases.forEach(([n, txt]) => {
    const r = t.ui.loadText(txt, n + '.json');
    ok(!r.ok && /거부/.test(t.$('#pjMsg').textContent) && !t.$('[data-pj="apply"]') && state(t) === before, `${n}: rejected (${t.$('#pjMsg').textContent.slice(0, 70)}), zero mutation`);
  });
  ok(t.P.parse('x'.repeat(1024 * 1024 + 1)).error.includes('해석하지 않음'), 'size checked before parse');
  t.w.close();
});

/* ================= guard (no auto stop/disconnect) ================= */
group('guard', () => {
  const t = start(), { E, H, P } = t;
  E.connect('BCM', 'doip');
  let stopped = 0, running = true;
  H.activity.register('qa', { list: () => (running ? [{ kind: 'test', label: 'QA 활동 (오프라인 실행 중)' }] : []), stop: () => { stopped++; running = false; } });
  const before = state(t);
  t.ui.loadText(fileText, 'qa.json');
  ok(t.$('.pj-guard') && t.$('[data-pj="apply"]').disabled && /QA 활동/.test(t.$('.pj-guard').textContent) && /BCM/.test(t.$('.pj-guard').textContent), 'guard reasons shown, apply disabled');
  const r = P.apply(t.ui.pending().plan);
  ok(!r.ok && r.code === 'guard' && state(t) === before && stopped === 0 && E.ctx('BCM', 'doip').link === 'online', 'apply refused, zero mutation, nothing auto-stopped/disconnected');
  t.click(t.$('[data-pj="stop"][data-key="qa"]'));
  ok(stopped === 1 && t.$('[data-pj="apply"]').disabled, 'explicit stop button stops only that activity; link still blocks');
  t.click(t.$('[data-pj="disconnect"]'));
  ok(E.ctx('BCM', 'doip').link === 'offline' && !t.$('[data-pj="apply"]').disabled && !t.$('.pj-guard'), 'explicit disconnect-all clears guard');
  // running workflow watch blocks too
  E.connect('BCM', 'doip'); t.wf.addWatch({ did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 }); t.wf.startWatch(t.wf.doc.watches[0].id);
  ok(P.guard().some(g => g.kind === 'activity' && g.key === 'workflow'), 'running watch listed by guard');
  t.wf.stopAllWatches(); t.clock.advance(1000); E.disconnect('BCM', 'doip');
  t.click(t.$('[data-pj="apply"]'));
  ok(/적용됨/.test(t.$('#pjMsg').textContent), 'apply after explicit cleanup');
  t.w.close();
});

/* ================= history / confirmed / missing sections ================= */
group('preserve live + missing sections', () => {
  const t = start(), { E } = t;
  E.connect('BCM', 'doip'); t.o.doRead(); t.clock.advance(1000); E.disconnect('BCM', 'doip');
  const hist = E.txns.length, conf = JSON.stringify([...E.confirmed.values()].map(x => [x.key, x.value, x.no]));
  t.wf.saveCommand({ id: 'new', name: 'keep me?', mode: 'did', didText: 'F190' });
  const trSec = t.H.project.sections().get('trace'), lySec = t.H.project.sections().get('layout');
  trSec.import(trSec.validate(Object.assign(trSec.export(), { timeMode: 'rel' })).value);
  if (t.w.LoomUdsLayout && t.w.LoomUdsLayout.saveNamed) t.w.LoomUdsLayout.saveNamed('keep?');
  // test-only extension without defaults(): absent → kept unchanged, and shown as such
  let qaVal = { v: 1, n: 7 }, qaImports = 0;
  const unreg = t.H.project.register('qa', { export: () => JSON.parse(JSON.stringify(qaVal)), validate: v => (v && v.v === 1 ? { ok: true, value: v } : { ok: false, error: 'qa' }), import: v => { qaImports++; qaVal = v; } });
  const x = JSON.parse(fileText); delete x.sections.workflow; delete x.sections.trace; delete x.sections.layout;
  t.ui.loadText(JSON.stringify(x), 'partial.json');
  const prev = t.$('#pjPrev').textContent;
  ok((prev.match(/파일에 없음 → 기본값/g) || []).length === 3 && /현재 유지/.test(prev), 'preview: workflow/trace/layout → defaults, defaults-less section → kept');
  t.click(t.$('[data-pj="apply"]'));
  ok(/적용됨/.test(t.$('#pjMsg').textContent) && t.wf.doc.commands.length === 0, 'missing workflow section → validated default (empty)');
  ok(JSON.stringify(trSec.export()) === JSON.stringify(trSec.validate(trSec.defaults()).value) && JSON.stringify(lySec.export()) === JSON.stringify(lySec.validate(lySec.defaults()).value), 'missing trace/layout → their validated defaults()');
  ok(qaImports === 0 && qaVal.n === 7, 'section without defaults() kept unchanged (never guessed)');
  const withQa = t.P.build(); unreg();
  ok(!t.P.plan(withQa).ok && /알 수 없는 섹션/.test(t.P.plan(withQa).error), 'section not registered on this page → file rejected');
  ok(E.txns.length === hist && JSON.stringify([...E.confirmed.values()].map(x => [x.key, x.value, x.no])) === conf, 'history + confirmed values preserved');
  t.w.close();
});

/* ================= rollback ================= */
group('rollback: core commit quota', () => {
  const t = start(); customize(t);
  const x = JSON.parse(fileText); x.sections.workflow.commands[0].name = 'FROM FILE';
  x.sections.core.catalog.ecus[0].name = 'FROM FILE ECU';
  const beforeMem = memState(t), beforeLs = JSON.parse(state(t)).ls;
  t.ui.loadText(JSON.stringify(x), 'q.json');
  const undo = quota(t, [CORE_KEY], Infinity);
  t.click(t.$('[data-pj="apply"]'));
  undo();
  ok(memState(t) === beforeMem, 'memory of every section restored');
  ok(JSON.stringify(JSON.parse(state(t)).ls[WF_KEY]) === JSON.stringify(beforeLs[WF_KEY]) && JSON.parse(state(t)).ls[CORE_KEY] === beforeLs[CORE_KEY], 'persisted workflow + core keys equal pre-import');
  ok(/저장 실패/.test(t.$('#pjMsg').textContent) && /복원 불완전/.test(t.$('#pjMsg').textContent) && t.$('#pjMsg').classList.contains('severe'), 'permanent core quota: failure + incomplete rollback (core re-commit) reported distinctly');
  t.w.close();
});
group('rollback: selective extension quota', () => {
  const t = start(); customize(t);
  const x = JSON.parse(fileText); x.sections.workflow.commands[0].name = 'FROM FILE'; x.sections.core.catalog.ecus[0].name = 'FROM FILE ECU';
  const beforeMem = memState(t), beforeLs = JSON.parse(state(t)).ls;
  // transient: only the first workflow write fails → workflow import throws → full clean rollback
  t.ui.loadText(JSON.stringify(x), 'q1.json');
  let undo = quota(t, [WF_KEY], 1);
  t.click(t.$('[data-pj="apply"]'));
  undo();
  ok(memState(t) === beforeMem && t.H.constants.ECUS.find(e => e.id === 'ABS').name === 'ABS Module (SIM)', 'workflow storage failure → core + all sections restored in memory');
  ok(JSON.parse(state(t)).ls[WF_KEY] === beforeLs[WF_KEY], 'workflow persisted value unchanged');
  ok(/workflow 실패/.test(t.$('#pjMsg').textContent) && /복원됨/.test(t.$('#pjMsg').textContent) && !/불완전/.test(t.$('#pjMsg').textContent), 'clean rollback reported as such: ' + t.$('#pjMsg').textContent.slice(0, 90));
  // permanent: workflow snapshot re-import also cannot persist → distinct incomplete-rollback report
  t.ui.loadText(JSON.stringify(x), 'q2.json');
  undo = quota(t, [WF_KEY], Infinity);
  const r = t.P.apply(t.ui.pending().plan);
  undo();
  ok(!r.ok && r.code === 'rollback-incomplete' && /workflow/.test(r.rollbackError), 'permanent extension quota → rollback-incomplete names the section');
  ok(memState(t) === beforeMem && JSON.parse(state(t)).ls[WF_KEY] === beforeLs[WF_KEY], 'memory still restored, persisted workflow untouched');
  t.w.close();
});
group('rollback: extension import throws', () => {
  const t = start(); customize(t);
  const beforeMem = memState(t);
  const x = JSON.parse(fileText); x.sections.core.catalog.ecus[0].name = 'FROM FILE ECU';
  const impl = t.H.project.sections().get('layout'), orig = impl.import; let n = 0;
  impl.import = v => { if (n++ === 0) throw new Error('simulated layout failure'); return orig(v); };
  t.ui.loadText(JSON.stringify(x), 't.json');
  t.click(t.$('[data-pj="apply"]'));
  impl.import = orig;
  ok(memState(t) === beforeMem && /layout 실패/.test(t.$('#pjMsg').textContent) && /복원됨/.test(t.$('#pjMsg').textContent), 'thrown extension import → full rollback, visible error');
  t.w.close();
});
group('workflow section stop-first', () => {
  const t = start(), sec = t.H.project.sections().get('workflow');
  t.E.connect('BCM', 'doip'); t.wf.addWatch({ did: '0101', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 500 }); t.wf.startWatch('w1');
  let threw = false; try { sec.import(t.w.LoomUdsWorkflow.model.emptyDoc()); } catch (e) { threw = true; }
  ok(threw && t.wf.wrt('w1').running && t.wf.doc.watches.length === 1, 'workflow import throws while running, never stops jobs');
  t.wf.stopAllWatches(); t.clock.advance(1000);
  const undo = quota(t, [WF_KEY], 1); threw = false;
  try { sec.import(t.w.LoomUdsWorkflow.model.emptyDoc()); } catch (e) { threw = /저장소/.test(e.message); }
  undo();
  ok(threw && t.wf.doc.watches.length === 1, 'workflow import throws on storage failure and keeps previous definitions');
  t.w.close();
});

const pass = results.filter(r => r.ok).length;
fs.writeFileSync(path.join(DIR, '..', 'qa', 'team', 'workflow-project-check.log'), JSON.stringify({ check: 'project-check', pass, total: results.length, results }, null, 1));
console.log(`project-check: ${pass}/${results.length} PASS`);
process.exitCode = pass === results.length ? 0 : 1;
