// Source-local focused checks (jsdom + fake clock; NOT browser acceptance).
// Run from repo root:  node work/reports/uds-read-did/app/selfcheck/check.cjs
const fs = require('fs'), path = require('path');
const { boot, fakeClock } = require('./harness.cjs');
const results = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const wins = [];
function start(storage) { const b = boot(storage); wins.push(b.w); return b; }
function helpers(b) {
  const { w, d } = b;
  const click = el => { el = typeof el === 'string' ? d.querySelector(el) : el; if (!el) throw new Error('missing'); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); return el; };
  const key = (el, k) => (typeof el === 'string' ? d.querySelector(el) : el).dispatchEvent(new w.KeyboardEvent('keydown', { key: k, bubbles: true }));
  const type = v => { const i = d.getElementById('didInput'); i.value = v; i.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const submit = () => d.getElementById('req').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const route = tr => click(`#segRoute [data-tr="${tr}"]`);
  const ecu = id => { const s = d.getElementById('selEcu'); s.value = id; s.dispatchEvent(new w.Event('change', { bubbles: true })); };
  const sim = patch => { Object.assign(b.o.st.sim, patch); b.o.savePrefs(); };
  const flush = () => b.o.flush();
  const txt = id => d.getElementById(id).textContent;
  return { click, key, type, submit, route, ecu, sim, flush, txt };
}
const hexs = a => a.map(x => x.toString(16).toUpperCase().padStart(2, '0')).join(' ');

try {
  /* ---- 1. initial state ---- */
  let b = start(), { w, d, o, U, clock } = b, h = helpers(b);
  ok('initial: DID list populated with 3 fixture DIDs incl. raw prefix (U1)', d.querySelectorAll('#didList li[data-did]').length === 3 && d.querySelector('#didList li[data-did="F190"]').textContent.includes('22 F1 90'));
  ok('initial: link offline, Read disabled, no auto send', o.curCtx().link === 'offline' && d.getElementById('btnRead').disabled && o.E.txns.length === 0);
  clock.advance(10000); h.flush();
  ok('initial: no transaction / timer after 10 s idle', o.E.txns.length === 0 && clock.pending() === 0);
  ok('initial: empty history honestly labelled', !d.getElementById('txEmpty').hidden && h.txt('txEmpty').includes('아직 요청 없음'));
  ok('shell: nav UART/Ethernet/UDS with real relative links, UDS current', [...d.querySelectorAll('.svcnav a')].map(a => a.getAttribute('href')).join() === '../../uart-workbench/opus/index.html,../../ethernet-workbench/opus/index.html,./index.html' && d.querySelector('.svcnav a[aria-current="page"]').dataset.svc === 'uds');
  ['../../uart-workbench/opus/index.html', '../../ethernet-workbench/opus/index.html'].forEach(p => ok('nav target exists: ' + p, fs.existsSync(path.resolve(__dirname, '..', p))));

  /* ---- 2. validation (no TX) ---- */
  h.click('#btnLink'); h.flush();
  ok('connect: link online gen 1, no TX', o.curCtx().link === 'online' && o.curCtx().gen === 1 && o.E.txns.length === 0);
  const bad = ['', 'F19', 'F1900', 'G190', '22 F1 90', '1', 'ZZZZ', '0x'];
  bad.forEach(v => { h.type(v); h.submit(); h.flush(); });
  ok('invalid DIDs (empty/odd/short/long/non-hex/with SID) never send', o.E.txns.length === 0);
  h.type('F19'); h.flush();
  ok('invalid DID shows error + aria-invalid + no PDU', d.getElementById('didInput').getAttribute('aria-invalid') === 'true' && h.txt('didMsg').includes('홀수') && h.txt('reqPdu').includes('—'));
  h.type('22 F1 90'); h.flush();
  ok('6-digit input explains SID is automatic', h.txt('didMsg').includes('SID 22는 자동'));
  ['0xf190', 'f1 90', 'F190'].forEach(v => ok('normalizes ' + JSON.stringify(v) + ' -> F190', U.validateDid(v).ok && U.validateDid(v).norm === 'F190'));
  ok('range: 0000 and FFFF valid, 10000 rejected', U.validateDid('0000').ok && U.validateDid('FFFF').ok && !U.validateDid('10000').ok);
  h.type('F190'); h.flush();
  ok('PDU preview exact 22 F1 90 (3 B)', h.txt('reqPdu').includes('22 F1 90') && h.txt('reqPdu').includes('3 B'));

  /* ---- 3. DID list selection fills editor without sending ---- */
  h.click('#didList li[data-did="F18C"]'); h.flush();
  ok('DID list click fills editor, no TX', d.getElementById('didInput').value === 'F18C' && o.E.txns.length === 0 && h.txt('reqPdu').includes('22 F1 8C'));
  h.key('#didList', 'ArrowDown'); h.flush();
  ok('DID list ArrowDown moves selection (0101), no TX', d.getElementById('didInput').value === '0101' && o.E.txns.length === 0);
  h.click('#didList li[data-did="F190"]');

  /* ---- 4. positive path ---- */
  h.submit(); h.flush();
  let t1 = o.E.txns[0];
  ok('Read: one txn, Tx 22 F1 90 recorded, status waiting', o.E.txns.length === 1 && hexs(t1.request) === '22 F1 90' && t1.status === 'waiting' && t1.events[0].kind === 'tx');
  ok('Read: new txn selected, Read disabled while active, Cancel enabled', o.st.selTxn === t1.id && d.getElementById('btnRead').disabled && !d.getElementById('btnCancel').disabled);
  clock.advance(400); h.flush();
  const vin = 'LOOMSIM0000000001';
  ok('positive: final = 62 F1 90 + 17 ASCII VIN', t1.status === 'positive' && hexs(t1.final) === '62 F1 90 ' + hexs([...vin].map(c => c.charCodeAt(0))));
  const cv1 = o.E.confirmed.get('BCM|' + 0xF190);
  ok('positive: confirmed VIN with provenance #1 / DoIP route', cv1 && cv1.value === vin && cv1.no === 1 && cv1.routeLabel === 'DoIP' && h.txt('valsBody').includes('#1'));
  ok('positive: detail auto-follows final response (RESPONSE_PDU, decoded VIN)', h.txt('fields').includes('RESPONSE_PDU') && h.txt('fields').includes(vin) && h.txt('fields').includes('확정 값으로 반영됨'));
  ok('positive: hex pane has exactly 20 byte cells', d.querySelectorAll('#hex .hx-h b').length === 20);
  // field <-> byte
  const didRow = [...d.querySelectorAll('#fields tr.fd')].find(r => r.textContent.startsWith('DID'));
  h.click(didRow); h.flush();
  ok('field->bytes: DID highlights offsets 1,2', JSON.stringify([...d.querySelectorAll('#hex .hx-h b.on')].map(x => +x.dataset.o)) === '[1,2]');
  h.click(d.querySelector('#hex .hx-h b[data-o="7"]'));
  ok('byte->field: offset 7 selects VIN data field', (d.querySelector('#fields tr.fd.on') || {}).textContent.startsWith('VIN'));
  // select Tx event shows request decode
  h.click(d.querySelector('#seq li[data-i="0"]'));
  ok('timeline: selecting Tx shows REQUEST_PDU 22 F1 90', h.txt('fields').includes('REQUEST_PDU') && d.querySelectorAll('#hex .hx-h b').length === 3);

  /* ---- 5. snapshot: route/ECU change never relabels ---- */
  h.sim({ scenario: 'pending', pending: 2 });
  h.submit(); h.flush();
  const t2 = o.E.txns[1];
  h.route('cantp'); h.flush();
  ok('route switch while #2 in flight: row still DoIP, detail route frozen', d.querySelector(`#tx-${t2.id} .c-rt`).textContent.startsWith('DoIP') && h.txt('fields').includes('192.168.10.20') && t2.route.label === 'DoIP');
  ok('other context (CAN-TP) offline and independent; Cancel disabled there', o.curCtx().link === 'offline' && d.getElementById('btnCancel').disabled);
  h.ecu('GW'); h.flush();
  ok('ECU switch: history rows keep BCM; vals panel scoped to GW', d.querySelector(`#tx-${t1.id} .c-ecu`).textContent === 'BCM' && h.txt('valsEcu') === 'GW' && !h.txt('valsBody').includes(vin));
  h.ecu('BCM'); h.route('doip'); h.flush();

  /* ---- 6. pending sequence ---- */
  clock.advance(400); h.flush();
  ok('pending: 7F 22 78 recorded, status pending, confirmed unchanged', t2.status === 'pending' && t2.pendingCount === 1 && hexs(t2.events[1].bytes) === '7F 22 78' && o.E.confirmed.get('BCM|' + 0xF190).no === 1);
  ok('pending: row shows ⏳ 0x78×1 and vals shows reading #2', d.querySelector(`#tx-${t2.id} .c-st`).textContent.includes('0x78×1') && h.txt('valsBody').includes('읽는 중 #2'));
  clock.advance(1200); h.flush();
  ok('pending: second 0x78', t2.pendingCount === 2 && t2.status === 'pending');
  clock.advance(1200); h.flush();
  ok('pending→positive: final after 2×78, timeline tx,78,78,62, confirmed #2', t2.status === 'positive' && t2.events.map(e => e.kind).join() === 'tx,pending,pending,positive' && o.E.confirmed.get('BCM|' + 0xF190).no === 2);
  ok('pending: elapsed = 400+2*1200 ms', t2.tEnd - t2.t0 === 2800);

  /* ---- 7. NRC keeps old confirmed value with old provenance ---- */
  h.sim({ scenario: 'nrc', nrc: 0x22, latency: 80 });
  h.submit(); clock.advance(80); h.flush();
  const t3 = o.E.txns[2];
  ok('NRC: 7F 22 22 final, status nrc, decode conditionsNotCorrect', t3.status === 'nrc' && hexs(t3.final) === '7F 22 22' && h.txt('fields').includes('conditionsNotCorrect'));
  ok('NRC: confirmed VIN still from #2, vals warns latest #3 NRC', o.E.confirmed.get('BCM|' + 0xF190).no === 2 && h.txt('valsBody').includes('최근 #3') && h.txt('valsBody').includes('이전 값'));
  ok('NRC: no green applied box, explicit no-update text', !d.querySelector('#fields .applied.yes') && h.txt('fields').includes('확정 값 갱신 없음'));

  /* ---- 8. timeout + late response ignored ---- */
  h.sim({ scenario: 'timeout', late: true });
  h.submit(); h.flush();
  const t4 = o.E.txns[3];
  clock.advance(999); h.flush();
  ok('timeout: still waiting at 999 ms', t4.status === 'waiting');
  clock.advance(1); h.flush();
  ok('timeout at P2 1000 ms, no response bytes', t4.status === 'timeout' && !t4.final && t4.events.filter(e => e.bytes).length === 1 && t4.tEnd - t4.t0 === 1000);
  clock.advance(1200); h.flush();
  ok('late positive after timeout: recorded ignored, status stays timeout, confirmed unchanged', t4.status === 'timeout' && t4.events.some(e => e.kind === 'late' && e.ignored && e.bytes[0] === 0x62) && o.E.confirmed.get('BCM|' + 0xF190).no === 2);
  ok('late badge on row', !!d.querySelector(`#tx-${t4.id} .late`));

  /* ---- 9. cancel isolation ---- */
  h.sim({ scenario: 'pending', pending: 2, latency: 400 });
  h.submit(); clock.advance(500); h.flush();
  const t5 = o.E.txns[4];
  h.click('#btnCancel'); h.flush();
  ok('cancel: status cancelled, Read re-enabled for context', t5.status === 'cancelled' && !d.getElementById('btnRead').disabled && !o.curCtx().active);
  clock.advance(5000); h.flush();
  ok('cancel: later 78/62 arrive as ignored late, no status/confirmed change', t5.status === 'cancelled' && t5.events.filter(e => e.kind === 'late').length === 2 && o.E.confirmed.get('BCM|' + 0xF190).no === 2);

  /* ---- 10. disconnect scenario ---- */
  h.sim({ scenario: 'disconnect', latency: 400 });
  h.submit(); clock.advance(400); h.flush();
  const t6 = o.E.txns[5];
  ok('disconnect: txn link-lost, ctx offline gen bumped, Read disabled', t6.status === 'link-lost' && o.curCtx().link === 'offline' && o.curCtx().gen === 2 && d.getElementById('btnRead').disabled);
  clock.advance(10000); h.flush();
  ok('disconnect: no later events, no auto reconnect/retry', t6.events.length === 2 && o.curCtx().link === 'offline' && o.E.txns.length === 6);

  /* ---- 11. one active per context; other contexts independent; older positive guard ---- */
  h.click('#btnLink'); h.route('cantp'); h.click('#btnLink'); h.flush();
  h.type('0101'); h.sim({ scenario: 'pending', pending: 3, latency: 400 });
  h.submit(); h.flush();
  const tA = o.E.txns[6];
  h.submit(); h.flush();
  ok('busy: second Read on same context rejected, no new txn', o.E.txns.length === 7);
  h.route('doip'); h.sim({ scenario: 'positive', latency: 80 });
  h.submit(); clock.advance(80); h.flush();
  const tB = o.E.txns[7];
  ok('parallel other route: #B (DoIP) positive while #A (CAN-TP) still in flight', tB.status === 'positive' && !o.E.terminal(tA.status) && o.E.confirmed.get('BCM|' + 0x0101).no === tB.no);
  const vB = o.E.confirmed.get('BCM|' + 0x0101).value;
  clock.advance(5000); h.flush();
  ok('older #A completes later: positive but NOT applied; confirmed stays #B', tA.status === 'positive' && tA.applied === false && /더 최근 요청/.test(tA.appliedNote) && o.E.confirmed.get('BCM|' + 0x0101).no === tB.no && o.E.confirmed.get('BCM|' + 0x0101).value === vB);
  ok('older #A row shows 값 미반영 badge', !!d.querySelector(`#tx-${tA.id} .unapplied`));
  ok('voltage decode consistent with raw bytes (u16 x 0.01)', (() => { const dd = tB.final.slice(3); return ((dd[0] << 8 | dd[1]) * 0.01).toFixed(2) === vB; })());

  /* ---- 12. unknown DID -> NRC 0x31 ---- */
  h.type('1234'); h.submit(); clock.advance(80); h.flush();
  const t9 = o.E.txns[8];
  ok('uncatalogued DID 1234: SIM ECU answers 7F 22 31', hexs(t9.final) === '7F 22 31' && t9.status === 'nrc');

  /* ---- 13. raw/decoded consistency over all positives ---- */
  let cons = true;
  o.E.txns.filter(t => t.status === 'positive').forEach(t => { const dd = U.decodePdu(t.final, t.did); if (!dd.echoOk || dd.fields[0].value !== '0x62' || dd.fields[1].off !== 1 || dd.fields[2].off !== 3 || dd.fields[2].len !== t.final.length - 3) cons = false; });
  o.E.confirmed.forEach(cv => { const t = o.E.byId(cv.txnId); if (!t || U.decodePdu(t.final, t.did).value.text !== cv.value || hexs(cv.data) !== hexs(t.final.slice(3))) cons = false; });
  ok('every positive: field offsets cover PDU; every confirmed value = decode(source txn bytes)', cons);

  /* ---- 14. filter + selection consistency + keyboard ---- */
  h.ecu('GW'); h.flush();
  h.click('#segFilter [data-filter="ecu"]'); h.flush();
  ok('filter current ECU (GW, none): selection cleared, detail empty, honest empty text', o.st.selTxn === null && h.txt('txEmpty').includes('현재 ECU(GW)') && h.txt('fields').includes('선택한 트랜잭션'));
  h.ecu('BCM'); h.click('#segFilter [data-filter="all"]'); h.flush();
  o.selectTxn(t1.id); h.key('#txBody', 'ArrowDown'); h.flush();
  ok('keyboard ArrowDown selects next txn', o.st.selTxn === t2.id);
  h.key('#txBody', 'End'); h.flush();
  ok('keyboard End selects last txn and detail follows', o.st.selTxn === t9.id && h.txt('seqHead').startsWith('#9'));
  h.click(d.querySelector(`#valsBody a[data-goto="${tB.id}"]`)); h.flush();
  ok('confirmed provenance link selects source txn', o.st.selTxn === tB.id);

  /* ---- 15. export ---- */
  const json = w.LoomUdsTools.exportJson(), csv = w.LoomUdsTools.exportCsv();
  const pj = JSON.parse(json);
  ok('export JSON: all txns, exact bytes, statuses, late flagged', pj.transactions.length === 9 && pj.transactions[0].finalResponse === hexs(t1.final) && pj.transactions[3].status === 'timeout' && pj.transactions[3].events.some(e => e.ignored));
  ok('export CSV: header + 9 rows, downloads triggered', csv.trim().split('\n').length === 10 && w.__dl.length === 2 && /\.json$/.test(w.__dl[0]) && /\.csv$/.test(w.__dl[1]));

  /* ---- 16. SIM panel + persistence ---- */
  h.click('#btnSim'); h.flush();
  ok('SIM panel renders scenario radios + note', d.querySelectorAll('#simSeg button').length === 5 && h.txt('simPanel').includes('P2 1000 ms'));
  h.click('#simSeg [data-sc="nrc"]'); h.flush();
  ok('SIM scenario click updates summary + enables NRC select only', o.st.sim.scenario === 'nrc' && h.txt('simSummary').startsWith('NRC') && !d.getElementById('simNrc').disabled && d.getElementById('simPend').disabled);
  const keys = Object.keys(w.localStorage).filter(k => k !== 'unrelated.key');
  ok('storage: only own prefix key, unrelated key kept', keys.length === 1 && keys[0] === 'loom.udsReadDid.v1' && w.localStorage.getItem('unrelated.key') === 'keep');
  const savedPrefs = JSON.parse(w.localStorage.getItem('loom.udsReadDid.v1'));
  ok('storage: no history / link state persisted', !('txns' in savedPrefs) && !JSON.stringify(savedPrefs).includes('LOOMSIM') && !('link' in savedPrefs));

  /* ---- 17. history clear keeps confirmed + in-flight ---- */
  h.route('cantp'); h.sim({ scenario: 'pending', pending: 2 }); h.type('F190'); h.submit(); h.flush();
  const tLive = o.E.txns[o.E.txns.length - 1];
  w.LoomUdsTools.clearHistory(); h.flush();
  ok('clear: only in-flight txn remains, confirmed values kept', o.E.txns.length === 1 && o.E.txns[0] === tLive && o.E.confirmed.size === 2);
  ok('confirmed provenance survives eviction text (#2)', h.txt('valsBody').includes('#2'));

  /* ---- 18. reload: prefs restored, no auto connect/send ---- */
  const store = {}; Object.keys(w.localStorage).forEach(k => { store[k] = w.localStorage.getItem(k); });
  const b2 = start(store), h2 = helpers(b2);
  b2.clock.advance(10000); h2.flush();
  ok('reload: ECU/route/DID/SIM restored', b2.o.st.ecu === 'BCM' && b2.o.st.transport === 'cantp' && b2.d.getElementById('didInput').value === 'F190' && b2.o.st.sim.scenario === 'pending');
  ok('reload: no txn, no link, no timers', b2.o.E.txns.length === 0 && b2.o.curCtx().link === 'offline' && b2.clock.pending() === 0);
  const b3 = start({ 'loom.udsReadDid.v1': '{bad json' });
  ok('corrupt prefs fall back to defaults', b3.o.st.ecu === 'BCM' && b3.o.st.didText === 'F190');

  /* ---- 19. history bound (engine) ---- */
  const c2 = fakeClock(); const E2 = U.createEngine({ clock: c2, historyLimit: 5 });
  E2.connect('BCM', 'doip'); E2.connect('GW', 'doip');
  E2.read({ ecu: 'GW', transport: 'doip', didText: 'F190', sim: { scenario: 'pending', pending: 5 } });
  for (let i = 0; i < 8; i++) { E2.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'positive', latency: 80 } }); c2.advance(80); }
  ok('history bounded to limit, in-flight txn never evicted', E2.txns.length === 5 && E2.txns.some(t => t.ecu === 'GW' && !E2.terminal(t.status)) && E2.confirmed.get('BCM|' + 0xF190).no === 9);
  /* ---- 20. U2 static guard (source only; native geometry is root's proof) ---- */
  const css = fs.readFileSync(path.resolve(__dirname, '../styles.css'), 'utf8');
  const i560 = css.indexOf('@media (max-width: 560px)'), m560 = css.slice(i560, css.indexOf('\n}\n', i560) + 2); // the original (first) 560 block only
  const has = re => re.test(m560);
  ok('U2: <=560 uses block/flex flow (mod block, wsp+detail flex column, center block), no grid row tracks, divider hidden',
    has(/\.mod-uds \{ display: block; overflow-y: auto;/) && has(/\.wsp, \.wsp\.did-collapsed, \.wsp\.vals-collapsed \{ display: flex; flex-direction: column;/) &&
    has(/\.center \{ display: block; min-height: auto; \}/) && has(/\.detail \{ display: flex; flex-direction: column;/) && has(/\.wsp > \* \{ flex: none; min-height: auto; \}/) &&
    !/grid-template-rows/.test(m560) && has(/\.divider\.h \{ display: none; \}/));
} catch (e) {
  ok('no exception', false, String(e && e.stack || e));
} finally {
  wins.forEach(w => { try { clearInterval(w.loomUdsOpus && w.loomUdsOpus.tickH); w.close(); } catch (e) { /* ignore */ } });
}
const pass = results.filter(r => r.pass).length;
console.log(`\n${pass}/${results.length} PASS`);
fs.writeFileSync(path.join(__dirname, 'selfcheck-result.json'), JSON.stringify({ when: new Date().toISOString(), pass, total: results.length, results }, null, 1));
process.exit(pass === results.length ? 0 : 1);
