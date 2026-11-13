// Services (session / TesterPresent / DTC) source-local checks — jsdom + fake clock, NOT browser acceptance.
// Run: node work/reports/uds-read-did/app/selfcheck/services-check.cjs
const { boot } = require('./harness.cjs');
const results = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const wins = [];
const start = s => { const b = boot(s); wins.push(b.w); return b; };
const hexs = a => (a || []).map(x => x.toString(16).toUpperCase().padStart(2, '0')).join(' ');

try {
  let b = start(), { w, d, o, clock } = b, H = w.LoomWorkbench, S = w.LoomUdsServices, E = o.E, U = b.U;
  const click = sel => d.querySelector(sel).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
  const last = () => E.txns[E.txns.length - 1];
  o.st.sim.latency = 80;
  /* ---- S1 session ---- */
  ok('S1 services panel mounted in #servicesMount; no TX/timers from boot', !!d.querySelector('#servicesMount #svSess') && E.txns.length === 0);
  click('[data-sv="s3"]'); o.flush();
  ok('S1 session request offline refused, zero TX, message shown', E.txns.length === 0 && /연결되어 있지 않/.test(d.getElementById('svMsg').textContent));
  E.connect('BCM', 'doip'); o.flush();
  let ss = E.session('BCM', 'doip');
  ok('S1 connect: tester view "assumed default", server default, no S3', ss.tester.status === 'assumed' && ss.tester.session === 1 && ss.server.session === 1 && ss.server.s3Due == null);
  click('[data-sv="s3"]'); clock.advance(80); o.flush();
  let t = last(); ss = E.session('BCM', 'doip');
  ok('S1 10 03 → 50 03 00 32 01 F4, source service, txn through engine/profile snapshot', hexs(t.request) === '10 03' && hexs(t.final) === '50 03 00 32 01 F4' && t.source === 'service' && t.profile.source === 'preset');
  ok('S1 extended confirmed (tester), server extended, S3 armed 5000 ms', ss.tester.status === 'confirmed' && ss.tester.session === 3 && ss.server.session === 3 && ss.server.s3Due === clock.now() + U.S3_MS);
  ok('S1 ECU-reported P2server/P2* shown as metadata; client profile P2/P2* unchanged (1000/5000)', ss.tester.p2srv === 50 && ss.tester.p2starsrv === 5000 && E.routeFor('BCM', 'doip').timing.p2 === 1000 && t.timing.p2 === 1000 && /메타데이터/.test(d.getElementById('svSess').textContent));
  ok('S1 non-DID positive never touches confirmed DID values', E.confirmed.size === 0 && t.applied === false);
  ok('S1 session chip shows extended + S3', /extendedDiagnostic/.test(d.getElementById('sessChip').textContent) && /S3/.test(d.getElementById('sessChip').textContent));
  const dec = U.decodePdu(t.final, { sid: 0x10, kind: 'raw', dir: 'rx' });
  ok('S1 0x50 decoder: session/P2server/P2*server fields', dec.fields.some(f => f.name === 'P2server_max' && f.value === '50 ms') && dec.fields.some(f => f.name === 'P2*server_max' && f.value === '5000 ms'));
  /* ---- S2 S3 expiry (fake clock) ---- */
  clock.advance(4999); o.flush();
  ok('S2 S3 not expired at 4999 ms', E.session('BCM', 'doip').server.session === 3);
  clock.advance(1); o.flush(); ss = E.session('BCM', 'doip');
  ok('S2 S3 expiry at 5000 ms → server default, tester "expired" (not just requested label)', ss.server.session === 1 && ss.tester.status === 'expired' && ss.tester.session === 1 && ss.tester.prev === 3);
  /* ---- S3 programming guard, raw parity ---- */
  click('[data-sv="s2"]'); clock.advance(80); o.flush();
  ok('S3 10 02 from default → 7F 10 22 (SIM fixture), tester state unchanged', hexs(last().final) === '7F 10 22' && E.session('BCM', 'doip').tester.status === 'expired');
  H.send({ mode: 'raw', rawText: '10 03' }); clock.advance(80);
  ok('S3 Raw 10 03 updates the SAME context session state', E.session('BCM', 'doip').tester.status === 'confirmed' && E.session('BCM', 'doip').server.session === 3 && last().kind === 'raw');
  H.send({ mode: 'raw', rawText: '10 02' }); clock.advance(80);
  ok('S3 10 02 from extended → 50 02', hexs(last().final).startsWith('50 02') && E.session('BCM', 'doip').server.session === 2);
  ok('S3 other context (BCM CAN-TP) independent', E.session('BCM', 'cantp').server.session === 1);
  ['10', '10 05', '10 03 00'].forEach(q => { H.send({ mode: 'raw', rawText: q }); clock.advance(80); });
  ok('S3 10 malformed: 1 B → 13, unknown sub 05 → 12, 3 B → 13', E.txns.slice(-3).map(x => hexs(x.final)).join() === '7F 10 13,7F 10 12,7F 10 13');
  H.send({ mode: 'raw', rawText: '10 83' }); clock.advance(80); ss = E.session('BCM', 'doip');
  ok('S3 10 83 (SPRMIB): status suppressed, server extended, tester "unknown" (not confirmed)', last().status === 'suppressed' && last().final === null && ss.server.session === 3 && ss.tester.status === 'unknown');
  /* ---- S4 SIM timeout / late ---- */
  o.st.sim.scenario = 'timeout'; o.st.sim.late = false;
  H.send({ mode: 'raw', rawText: '10 01' }); clock.advance(1000);
  ok('S4 timeout on 10 01: tester unknown, server unchanged (request never processed)', last().status === 'timeout' && E.session('BCM', 'doip').tester.status === 'unknown' && E.session('BCM', 'doip').server.session === 3);
  o.st.sim.scenario = 'positive';
  /* ---- S5 TesterPresent runner ---- */
  clock.advance(10000); // S3 expires again
  H.send({ mode: 'raw', rawText: '10 03' }); clock.advance(80);
  const n0 = E.txns.length;
  d.getElementById('svTpInt').value = '2000'; d.getElementById('svTpInt').dispatchEvent(new w.Event('change', { bubbles: true }));
  d.getElementById('svTpMode').value = '80'; d.getElementById('svTpMode').dispatchEvent(new w.Event('change', { bubbles: true }));
  click('#svTpBtn'); o.flush();
  const job = S.tps.get('BCM|doip');
  ok('S5 TP explicit start: immediate 3E 80 txn source tester-present, activity listed', job.running && last().source === 'tester-present' && hexs(last().request) === '3E 80' && H.activity.list().some(a => a.key === 'tp' && a.ecu === 'BCM'));
  clock.advance(80);
  ok('S5 3E 80 suppressed; selection not stolen', last().status === 'suppressed' && o.st.selTxn !== last().id);
  clock.advance(12000); o.flush();
  ok('S5 TP keeps extended alive past S3 (12 s later still extended)', E.session('BCM', 'doip').server.session === 3 && job.sent === 7);
  ok('S5 actual cadence 2000 ms shown', S.cadence(job) === 2000 && /2000 ms/.test(d.getElementById('svTp').textContent));
  // busy skip: manual long pending occupies context at a tick
  o.st.sim.scenario = 'pending'; o.st.sim.pending = 3;
  const man = H.send({ mode: 'did', didText: 'F190' });
  const sk0 = job.skipped;
  clock.advance(2000);
  ok('S5 TP skips while manual request busy (no queue), never cancels manual', job.skipped === sk0 + 1 && !E.terminal(man.txn.status) && man.txn.status === 'pending');
  o.st.sim.scenario = 'positive';
  clock.advance(5000);
  ok('S5 manual completes normally after TP skip', man.txn.status === 'positive');
  const nBefore = E.txns.length;
  click('#svTpBtn'); clock.advance(10000); o.flush();
  ok('S5 TP stop: no further TX, activity cleared', !job.running && E.txns.length === nBefore && !H.activity.list().some(a => a.key === 'tp'));
  ok('S5 after stop S3 expires (no fake background success)', E.session('BCM', 'doip').server.session === 1 && E.session('BCM', 'doip').tester.status === 'expired');
  click('#svTpBtn'); clock.advance(100);
  E.disconnect('BCM', 'doip', 'user'); o.flush(); clock.advance(10000);
  const job2 = S.tps.get('BCM|doip');
  ok('S5 link down stops TP with reason', !job2.running && /링크/.test(job2.stopReason));
  const nL = E.txns.length;
  E.connect('BCM', 'doip'); clock.advance(10000); o.flush();
  ok('S5 reconnect does NOT auto-resume TP (no TX in 10 s)', !S.tps.get('BCM|doip').running && E.txns.length === nL);
  ok('S5 reconnect resets session to assumed default', E.session('BCM', 'doip').tester.status === 'assumed' && E.session('BCM', 'doip').server.session === 1);
  H.send({ mode: 'raw', rawText: '3E 00' }); clock.advance(80);
  ok('S5 raw 3E 00 → 7E 00', hexs(last().final) === '7E 00');
  ['3E 01', '3E 00 00'].forEach(q => { H.send({ mode: 'raw', rawText: q }); clock.advance(80); });
  ok('S5 3E 01 → 12, 3E 00 00 → 13', E.txns.slice(-2).map(x => hexs(x.final)).join() === '7F 3E 12,7F 3E 13');
  /* ---- S6 DTC ---- */
  click('[data-sv="d2"]'); clock.advance(80); o.flush();
  t = last();
  ok('S6 19 02 FF → 59 02 FF + 3 BCM fixture records (4 B each)', hexs(t.request) === '19 02 FF' && t.final[0] === 0x59 && t.final.length === 3 + 12);
  ok('S6 DTC table: codes/status/flags/descriptions from parsed bytes', /B1A23-11/.test(d.getElementById('svDtc').textContent) && /0x2F/.test(d.getElementById('svDtc').textContent) && /confirmed/i.test(d.getElementById('svDtc').textContent) && d.querySelectorAll('#svDtc tbody tr').length === 3);
  const mask = d.getElementById('svMask'); mask.value = '01'; mask.dispatchEvent(new w.Event('input', { bubbles: true }));
  click('[data-sv="d2"]'); clock.advance(80); o.flush();
  ok('S6 mask 01 (testFailed) → only 1 record; diff notes mask differs', last().final.length === 3 + 4 && /마스크가 달라/.test(d.getElementById('svDtc').textContent));
  mask.value = 'ZZ'; mask.dispatchEvent(new w.Event('input', { bubbles: true }));
  const nz = E.txns.length; click('[data-sv="d2"]');
  ok('S6 invalid mask → no TX', E.txns.length === nz && /유효하지/.test(d.getElementById('svMsg').textContent));
  mask.value = 'FF'; mask.dispatchEvent(new w.Event('input', { bubbles: true }));
  click('[data-sv="d1"]'); clock.advance(80);
  ok('S6 19 01 FF → 59 01 FF 01 00 03', hexs(last().final) === '59 01 FF 01 00 03');
  click('#svClr'); const nc = E.txns.length;
  ok('S6 clear needs explicit second click (no TX on first)', E.txns.length === nc && /확인/.test(d.getElementById('svClr').textContent));
  click('#svClr'); clock.advance(80);
  ok('S6 14 FF FF FF in default session → 7F 14 22; DTCs kept', hexs(last().final) === '7F 14 22' && E.dtcMemory('BCM').length === 3);
  H.send({ mode: 'raw', rawText: '10 03' }); clock.advance(80);
  click('#svClr'); click('#svClr'); clock.advance(80);
  ok('S6 extended + clear → 54; SIM DTC memory empty', hexs(last().final) === '54' && E.dtcMemory('BCM').length === 0);
  H.send({ mode: 'raw', rawText: '19 02 FF' }); clock.advance(80); o.flush();
  ok('S6 raw 19 02 FF after clear → 59 02 FF (0 records); diff shows 3 gone', hexs(last().final) === '59 02 FF' && /사라짐 3/.test(d.getElementById('svDtc').textContent));
  click('[data-sv="restore"]');
  ok('S6 SIM restore is local (no TX) and labelled SIM', E.dtcMemory('BCM').length === 3 && last().request[0] === 0x19 && /SIM/.test(d.getElementById('svMsg').textContent));
  ['19', '19 02', '19 05 FF', '14 00 00 01'].forEach(q => { H.send({ mode: 'raw', rawText: q }); clock.advance(80); });
  ok('S6 malformed: 19 → 13, 19 02 → 13, 19 05 → 12, 14 group≠FFFFFF → 31', E.txns.slice(-4).map(x => hexs(x.final)).join() === '7F 19 13,7F 19 13,7F 19 12,7F 14 31');
  ok('S6 GW fixture separate', E.dtcMemory('GW').length === 2 && E.dtcMemory('GW')[0].code === 0xC10000 && U.dtcText(0xC10000) === 'U0100-00');
  ok('S6 non-DID services never wrote confirmed DID values', [...E.confirmed.values()].every(v => v.did === 0xF190));
  /* ---- S7 reload: prefs only, nothing running ---- */
  click('#svTpBtn'); clock.advance(100);
  const store = { 'loom.udsReadDid.v1': w.localStorage.getItem('loom.udsReadDid.v1') };
  const sp = JSON.parse(store['loom.udsReadDid.v1']);
  ok('S7 services prefs persisted (interval/mode/mask), no running state/history', sp.services && sp.services.tpInterval === 2000 && sp.services.tpMode === '80' && !('running' in sp.services) && !('tps' in sp.services));
  const b2 = start(store);
  b2.clock.advance(20000);
  ok('S7 reload: TP not running, zero TX, offline, no timers', b2.w.LoomUdsServices.tps.size === 0 && b2.o.E.txns.length === 0 && b2.o.curCtx().link === 'offline' && b2.clock.pending() === 0 && b2.d.getElementById('svTpInt').value === '2000');
} catch (e) { ok('no exception', false, String(e && e.stack || e)); }
finally { wins.forEach(w => { try { clearInterval(w.loomUdsOpus.tickH); clearInterval(w.LoomUdsServices && w.LoomUdsServices.tickH); w.close(); } catch (e) { /* ignore */ } }); }
const fail = results.filter(r => !r.pass).length;
console.log(`\n${results.length - fail}/${results.length} PASS`);
require('fs').writeFileSync(require('path').join(__dirname, 'services-check-result.json'), JSON.stringify({ when: new Date().toISOString(), note: 'jsdom source-local, not browser acceptance', results }, null, 2));
process.exitCode = fail ? 1 : 0;
