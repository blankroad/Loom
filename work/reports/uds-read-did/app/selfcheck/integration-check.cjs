// Shared host contract + integration checks (jsdom + fake clock; NOT browser acceptance).
// Run: node work/reports/uds-read-did/app/selfcheck/integration-check.cjs
const { boot, FILES, ALL } = require('./harness.cjs');
const results = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const wins = [];
const start = s => { const b = boot(s); wins.push(b.w); return b; };
const hexs = a => a.map(x => x.toString(16).toUpperCase().padStart(2, '0')).join(' ');

try {
  console.log('scripts loaded: ' + FILES.join(', ') + (ALL.length !== FILES.length ? '  (not yet delivered: ' + ALL.filter(f => !FILES.includes(f)).join(', ') + ')' : ''));
  /* ---- H1 host shape ---- */
  let b = start(), { w, d, o, clock } = b, H = w.LoomWorkbench;
  ok('H1 window.LoomWorkbench exists, frozen, version 1, engine/constants identical to core', !!H && Object.isFrozen(H) && H.version === 1 && H.engine === o.E && H.constants === w.LoomUds);
  ok('H1 mount slots present exactly once', ['workflowMount', 'watchMount', 'traceToolsMount', 'layoutToolsMount', 'catalogMount', 'servicesMount', 'projectMount'].every(id => d.querySelectorAll('#' + id).length === 1));
  const order = ALL.indexOf('host.js');
  ok('H1 script order: engine,app,tools,config,host before every feature module', ALL.slice(0, 5).join() === 'engine.js,app.js,tools.js,config.js,host.js' && ['catalog.js', 'services.js', 'project.js', 'workflow.js', 'trace-tools.js', 'layout.js'].every(f => ALL.indexOf(f) > order));
  ok('H1 clock is the engine clock (fake clock in checks)', H.clock.now() === clock.now() && (() => { let hit = 0; const hd = H.clock.set(() => hit++, 50); clock.advance(49); const a = hit; clock.advance(1); H.clock.clear(H.clock.set(() => hit++, 10)); clock.advance(20); return a === 0 && hit === 1 && hd; })());
  /* ---- H2 context / editor / compose ---- */
  let cx = H.context();
  ok('H2 context snapshot {ecu,transport,profile} preset, offline', cx.ecu === 'BCM' && cx.transport === 'doip' && cx.profile.source === 'preset' && cx.link === 'offline');
  cx.ecu = 'GW';
  ok('H2 context is a copy (mutation does not retarget)', H.context().ecu === 'BCM');
  const ed0 = H.editor();
  ok('H2 editor() read-only snapshot', ed0.mode === 'did' && ed0.didText === 'F190' && typeof ed0.rawText === 'string');
  ok('H2 compose fills editor, raw mode, zero TX', H.compose({ mode: 'raw', rawText: '22 F1 8C' }).ok && d.getElementById('rawInput').value === '22 F1 8C' && o.st.mode === 'raw' && o.E.txns.length === 0);
  ok('H2 compose invalid mode refused', !H.compose({ mode: 'x' }).ok && o.st.mode === 'raw');
  H.compose({ mode: 'did', didText: 'F190' });
  /* ---- H3 send ---- */
  let r = H.send({ mode: 'did', didText: 'F190', source: 'watch' });
  ok('H3 send offline refused (no auto-connect), zero TX', !r.ok && r.code === 'offline' && o.E.txns.length === 0 && o.curCtx().link === 'offline');
  o.E.connect('BCM', 'doip'); o.E.connect('GW', 'cantp'); o.flush();
  const evs = []; const un = H.subscribe(ev => evs.push(ev.type + (ev.created ? '+' : '')));
  r = H.send({ ecu: 'GW', transport: 'cantp', mode: 'did', didText: 'F18C', source: 'watch' });
  ok('H3 explicit-context send: GW/CAN-TP txn while current is BCM/DoIP; source stamped; selection not stolen', r.ok && r.txn.ecu === 'GW' && r.txn.transport === 'cantp' && r.txn.source === 'watch' && o.st.selTxn !== r.txn.id && o.st.ecu === 'BCM');
  const r2 = H.send({ ecu: 'GW', transport: 'cantp', mode: 'raw', rawText: '22 F1 90', source: 'watch' });
  ok('H3 busy context refused (serial guard), no queue', !r2.ok && r2.code === 'busy' && o.E.txns.length === 1);
  ok('H3 invalid input refused', H.send({ mode: 'raw', rawText: '2' }).code === 'invalid' && H.send({ mode: 'did', didText: 'XYZ' }).code === 'invalid');
  const m = H.send({ mode: 'did', didText: 'F190' });
  ok('H3 manual-source send selects its row', m.ok && m.txn.source === 'manual' && o.st.selTxn === m.txn.id);
  ok('H3 subscribe got one created event per TX (no duplicates)', evs.filter(x => x === 'txn+').length === 2, evs);
  /* ---- H4 owned cancel ---- */
  ok('H4 cancelOwned refuses wrong owner (manual txn not cancellable as watch)', H.cancelOwned(m.txn.id, 'watch').code === 'not-owner' && !o.E.terminal(m.txn.status));
  clock.advance(500); o.flush();
  ok('H4 completed txns: cancelOwned refuses not-active', H.cancelOwned(r.txn.id, 'watch').code === 'not-active' && r.txn.status === 'positive');
  const r3 = H.send({ ecu: 'GW', transport: 'cantp', mode: 'did', didText: 'F190', source: 'watch' });
  ok('H4 cancelOwned cancels own active txn only', H.cancelOwned(r3.txn.id, 'watch').ok && r3.txn.status === 'cancelled');
  /* ---- H5 render hooks / re-entrancy ---- */
  let n = 0; const offR = H.onRender(() => { n++; if (n < 50) H.refresh(); });
  H.refresh(); clock.advance(0); o.flush();
  ok('H5 onRender re-entrant refresh does not loop synchronously', n >= 1 && n < 50);
  offR(); const n0 = n; H.refresh();
  ok('H5 onRender unsubscribe works', n === n0);
  /* ---- H6 trace view ---- */
  const frozen = o.E.txns.slice(0, 1);
  H.setTraceView({ rows: () => frozen, byId: id => frozen.find(x => x.id === id) || null });
  ok('H6 trace view rows drive history', d.querySelectorAll('#txBody .tx-row').length === 1);
  H.setTraceView({ rows: () => [], byId: () => null });
  ok('H6 empty filtered view clears selection + detail (no hidden-row detail)', o.st.selTxn === null && d.querySelectorAll('#txBody .tx-row').length === 0 && /트랜잭션을 선택/.test(d.getElementById('seq').textContent));
  H.setTraceView(null);
  ok('H6 null restores base rows', d.querySelectorAll('#txBody .tx-row').length === o.E.txns.length);
  ok('H6 invalid view rejected', !H.setTraceView({}).ok);
  /* ---- H7 registries ---- */
  let threw = 0; try { H.project.register('catalog', { export() {}, validate() {}, import() {} }); } catch (e) { threw++; }
  try { H.project.register('workflow2', {}); } catch (e) { threw++; }
  ok('H7 project.register rejects reserved key / bad impl', threw === 2);
  const unA = H.activity.register('qa', { list: () => [{ ecu: 'GW', transport: 'cantp', kind: 'watch' }], stop() {} });
  ok('H7 activity.list aggregates with key; forEcu', H.activity.list().some(x => x.key === 'qa' && x.kind === 'watch') && H.activity.forEcu('GW').length >= 1 && H.activity.forEcu('BCM').every(x => x.key !== 'qa'));
  unA();
  ok('H7 activity unregister', !H.activity.list().some(x => x.key === 'qa'));
  /* ---- H8 drawer ---- */
  H.tools.open('services');
  ok('H8 tool drawer: one panel visible, tab selected', !d.getElementById('toolDrawer').hidden && !d.getElementById('tp-services').hidden && d.getElementById('tp-catalog').hidden && d.getElementById('tt-services').getAttribute('aria-selected') === 'true');
  d.getElementById('toolClose').dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
  ok('H8 close hides drawer', d.getElementById('toolDrawer').hidden && H.tools.current() === null);
  un();

  /* ==== I: cross-module integration (core + A workflow/project + B trace/layout), real modules ==== */
  b = start(); ({ w, d, o, clock } = b); H = w.LoomWorkbench;
  const E = o.E, WF = w.LoomUdsWorkflow && w.LoomUdsWorkflow.instance, TR = w.LoomUdsTrace, SV = w.LoomUdsServices;
  const mounts = ['workflowMount', 'watchMount', 'traceToolsMount', 'layoutToolsMount', 'catalogMount', 'servicesMount', 'projectMount'];
  ok('I1 every mount populated by its owner module (no empty slot)', mounts.every(id => d.getElementById(id).children.length > 0), mounts.filter(id => !d.getElementById(id).children.length));
  ok('I1 owner globals present: workflow, trace, layout, project, services, catalog', !!WF && !!TR && !!w.LoomUdsLayout && !!w.LoomUdsProject && !!SV && !!w.LoomUdsCatalog);
  ok('I1 boot: zero TX, nothing running, all offline', E.txns.length === 0 && H.activity.list().length === 0 && H.coreProject.guard().length === 0);
  o.st.sim.latency = 80;
  E.connect('BCM', 'doip'); o.flush();
  /* I2 TP + watch + manual on one context: one in-flight request at a time, skips not queues */
  SV.pref.tpInterval = 1000; SV.pref.tpMode = '80';
  const tp = SV.tpStart('BCM', 'doip');
  const wa = WF.addWatch({ did: 'F190', target: { ecu: 'BCM', transport: 'doip' }, intervalMs: 300 });
  const wid = wa && (wa.id || (wa.watch && wa.watch.id));
  const ws = WF.startWatch(wid);
  ok('I2 TP + watch started explicitly on BCM/DoIP', tp.ok && ws && ws.ok !== false);
  ok('I2 activity aggregates tp + workflow (watch)', H.activity.list().some(x => x.key === 'tp') && H.activity.list().some(x => x.key === 'workflow' && x.kind === 'watch'));
  clock.advance(6000); o.flush();
  const ctxTx = E.txns.filter(x => x.ctxKey === 'BCM|doip');
  const overlap = ctxTx.some((x, i) => i > 0 && x.t0 < ctxTx[i - 1].tEnd);
  ok('I2 never two overlapping requests on the context (serial guard honoured by TP + watch)', ctxTx.length > 10 && !overlap, ctxTx.length);
  ok('I2 sources stamped tester-present + watch; TP skipped busy slots', ctxTx.some(x => x.source === 'tester-present') && ctxTx.some(x => x.source === 'watch') && SV.tps.get('BCM|doip').skipped + SV.tps.get('BCM|doip').sent >= 6);
  const sel0 = o.st.selTxn;
  ok('I2 automation never stole selection', sel0 == null || !ctxTx.filter(x => x.source !== 'manual').some(x => x.id === sel0));
  /* I3 trace pause with live automation */
  TR.pause(); o.flush();
  const frozenN = d.querySelectorAll('#txBody .tx-row').length, liveN = E.txns.length;
  clock.advance(3000); o.flush();
  ok('I3 trace display paused: rows frozen while engine keeps receiving', d.querySelectorAll('#txBody .tx-row').length === frozenN && E.txns.length > liveN);
  TR.resume(); o.flush();
  ok('I3 resume shows live rows', d.querySelectorAll('#txBody .tx-row').length === Math.min(E.txns.length, E.limit));
  /* I4 guards with running automation */
  const g = H.coreProject.guard();
  ok('I4 project guard lists running TP, watch and online link (import must be refused)', g.some(r => r.key === 'tp') && g.some(r => r.key === 'workflow') && g.some(r => r.kind === 'online' || r.kind === 'busy'));
  E.catalog.addEcu({ id: 'ABS', name: 'ABS', doipLA: '1020', canTx: '760', canRx: '768' });
  /* I5 stop: explicit; in-flight requests finish normally (never cancelled) */
  WF.stopAllWatches(); SV.tpStop('BCM|doip', 'QA');
  clock.advance(2000); o.flush();
  const nStop = E.txns.length; clock.advance(5000);
  ok('I5 after explicit stops: no further TX, nothing cancelled, activity empty', E.txns.length === nStop && !E.txns.some(x => x.status === 'cancelled') && H.activity.list().length === 0);
  /* I6 manual read then reload restores definitions only */
  H.send({ mode: 'did', didText: 'F190' }); clock.advance(100); o.flush();
  const store = {}; for (let i = 0; i < w.localStorage.length; i++) { const k = w.localStorage.key(i); store[k] = w.localStorage.getItem(k); }
  const b2 = start(store);
  b2.clock.advance(20000);
  ok('I6 reload: watch definition + custom ECU restored, but nothing running / sent / connected / timers', b2.U.ECUS.some(e => e.id === 'ABS') && b2.w.LoomUdsWorkflow.instance.doc.watches.length === 1 && b2.w.LoomWorkbench.activity.list().length === 0 && b2.o.E.txns.length === 0 && b2.o.curCtx().link === 'offline' && b2.clock.pending() === 0);
  /* I7 suppressed status flows through trace filters */
  TR.filters.set({ status: 'suppressed' }); o.flush();
  const vis = o.visibleTxns();
  ok('I7 trace status filter "suppressed" shows only 3E 80 rows', vis.length > 0 && vis.every(x => x.status === 'suppressed'));
  TR.filters.clear(); o.flush();
} catch (e) { ok('no exception', false, String(e && e.stack || e)); }
finally { wins.forEach(w => { try { clearInterval(w.loomUdsOpus && w.loomUdsOpus.tickH); clearInterval(w.LoomUdsServices && w.LoomUdsServices.tickH); w.close(); } catch (e) { /* ignore */ } }); }
const fail = results.filter(r => !r.pass).length;
console.log(`\n${results.length - fail}/${results.length} PASS`);
require('fs').writeFileSync(require('path').join(__dirname, 'integration-check-result.json'), JSON.stringify({ when: new Date().toISOString(), note: 'jsdom source-local, not browser acceptance', scripts: FILES, results }, null, 2));
process.exitCode = fail ? 1 : 0;
