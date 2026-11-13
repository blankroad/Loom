// ECU / DID catalogue source-local checks — jsdom + fake clock, NOT browser acceptance.
// Run: node work/reports/uds-read-did/app/selfcheck/catalog-check.cjs
const { boot } = require('./harness.cjs');
const results = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const wins = [];
const start = s => { const b = boot(s); wins.push(b.w); return b; };
const hexs = a => (a || []).map(x => x.toString(16).toUpperCase().padStart(2, '0')).join(' ');
const PK = 'loom.udsReadDid.v1';

try {
  let b = start(), { w, d, o, clock, U } = b, H = w.LoomWorkbench, E = o.E, C = E.catalog;
  const $ = id => d.getElementById(id);
  const click = el => (typeof el === 'string' ? d.querySelector(el) : el).dispatchEvent(new w.MouseEvent('click', { bubbles: true }));
  const inp = (id, v) => { const el = $(id); el.value = v; el.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const chg = (id, v) => { const el = $(id); el.value = v; el.dispatchEvent(new w.Event('change', { bubbles: true })); };
  const submit = id => $(id).dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const row = (attr, v) => d.querySelector(`#catalogMount tr[data-${attr}="${v}"]`);
  o.st.sim.latency = 80;
  /* ---- C1 presets ---- */
  ok('C1 presets: BCM/GW + 3 DIDs listed read-only (no edit/delete buttons)', row('ecu', 'BCM') && row('ecu', 'GW') && !row('ecu', 'BCM').querySelector('[data-ct="ecu-del"]') && ['F190', 'F18C', '0101'].every(h => row('did', h) && !row('did', h).querySelector('[data-ct="did-del"]')));
  ok('C1 engine refuses preset edit/delete', C.deleteEcu('BCM').code === 'preset' && C.updateEcu('GW', { name: 'x' }).code === 'preset' && C.deleteDid(0xF190).code === 'preset' && C.updateDid(0x0101, {}).code === 'preset' && U.ECUS.length === 2 && U.DIDS.length === 3);
  ok('C1 preset entries frozen', Object.isFrozen(U.ECUS[0]) && Object.isFrozen(U.DIDS[0]));
  /* ---- C2 ECU create via UI with strict validation ---- */
  click('[data-ct="ecu-new"]');
  inp('ce-id', 'bcm'); inp('ce-name', ''); inp('ce-doipLA', '0E00'); inp('ce-canTx', '740'); inp('ce-canRx', '740');
  submit('ctEcuForm');
  ok('C2 invalid ECU: preset id / empty name / tester LA / duplicate CAN id → errors, nothing added', U.ECUS.length === 2 && /프리셋/.test($('ce-id-e').textContent) && $('ce-name-e').textContent && /Tester/.test($('ce-doipLA-e').textContent) && /중복|같을/.test($('ce-canTx-e').textContent + $('ce-canRx-e').textContent));
  inp('ce-id', 'ABS'); inp('ce-name', 'ABS SIM'); inp('ce-doipLA', '1020'); inp('ce-canTx', '760'); inp('ce-canRx', '768');
  submit('ctEcuForm'); o.flush();
  ok('C2 ECU ABS added: selector option, presets kept, no connect/TX', U.ECUS.map(e => e.id).join() === 'BCM,GW,ABS' && [...$('selEcu').options].some(x => x.value === 'ABS') && E.txns.length === 0 && E.ctx('ABS', 'doip').link === 'offline');
  ok('C2 preset profiles derived from custom addresses', E.routeFor('ABS', 'doip').ecuLA === 0x1020 && E.routeFor('ABS', 'cantp').txId === 0x760);
  ok('C2 duplicate address refused (engine)', !C.addEcu({ id: 'X2', name: 'x', doipLA: '1020', canTx: '761', canRx: '769' }).ok);
  /* ---- C3 context + profile on custom ECU ---- */
  chg('selEcu', 'ABS'); o.flush();
  ok('C3 select ABS: ctx/route/name show custom ECU', o.st.ecu === 'ABS' && /사용자 SIM ECU/.test($('ecuName').textContent));
  const sv = E.saveProfile('ABS', 'doip', { targetId: 'preset:ABS|doip', name: 'ABS lab', values: Object.assign({}, U.presetValues('ABS', 'doip'), { host: '192.0.2.44' }) });
  ok('C3 custom profile on custom ECU validates against registry', sv.ok && E.activeProfile('ABS', 'doip').name === 'ABS lab');
  E.connect('ABS', 'doip'); o.flush();
  H.send({ mode: 'did', didText: 'F18C' }); clock.advance(80); o.flush();
  let t = E.txns[E.txns.length - 1];
  ok('C3 read on custom ECU: default SIM fixture F18C, route snapshot custom profile', t.status === 'positive' && /SIM-ABS-000001/.test(String.fromCharCode(...t.final.slice(3))) && t.route.host === '192.0.2.44:13400' && t.ecu === 'ABS');
  /* ---- C4 custom DID with preview + fixture ---- */
  click('[data-ct="did-new"]');
  inp('cd-did', 'F190'); inp('cd-name', 'Temp'); chg('cd-type', 'sint'); inp('cd-recLen', '3'); inp('cd-byteOffset', '2'); inp('cd-length', '2');
  submit('ctDidForm');
  ok('C4 invalid DID: preset DID refused, offset+len > recLen refused, nothing added', U.DIDS.length === 3 && /프리셋/.test($('cd-did-e').textContent) && /레코드/.test($('cd-length-e').textContent));
  inp('cd-did', 'f1a0'); inp('cd-byteOffset', '1'); inp('cd-scale', '0.1'); inp('cd-offset', '-40'); inp('cd-decimals', '1'); inp('cd-unit', '°C'); chg('cd-endian', 'big');
  inp('cd-fx', '00 01 2C');
  ok('C4 live preview: expected PDU 62 F1 A0 00 01 2C, decoded -10.0 °C, lengths', /62 F1 A0 00 01 2C/.test($('cdPrev').textContent) && /-10\.0 °C/.test($('cdPrev').textContent) && /기대 3 B/.test($('cdPrev').textContent));
  inp('cd-fx', '00 01'); ok('C4 preview flags length mismatch (fixture 2 B vs record 3 B)', /길이 2 B \(기대 3 B\)/.test($('cdPrev').textContent));
  inp('cd-fx', '00 0 1'); ok('C4 invalid fixture hex shown, blocks save', !!$('cd-fx-e').textContent);
  submit('ctDidForm'); ok('C4 invalid fixture not saved', U.DIDS.length === 3);
  inp('cd-fx', '00 01 2C'); submit('ctDidForm'); o.flush();
  const info = U.didInfo(0xF1A0);
  ok('C4 DID F1A0 added (custom, bounded fields), fixture on all ECUs', info && info.custom && info.type === 'sint' && info.recLen === 3 && ['BCM', 'GW', 'ABS'].every(e => hexs(C.fixture(e, 0xF1A0)) === '00 01 2C'));
  ok('C4 DID appears in DID list + quick select + values pane', !!d.querySelector('#didList li[data-did="F1A0"]') && [...$('didQuick').options].some(x => x.value === 'F1A0') && !!d.querySelector('#valsBody .cv[data-did="F1A0"]'));
  H.send({ mode: 'did', didText: 'F1A0' }); clock.advance(80); o.flush();
  t = E.txns[E.txns.length - 1];
  const cv = E.confirmed.get('ABS|' + 0xF1A0);
  ok('C4 custom DID read → configured fixture response (not 7F 31), exact raw bytes, decoded value confirmed', t.status === 'positive' && hexs(t.final) === '62 F1 A0 00 01 2C' && cv && cv.value === '-10.0' && cv.unit === '°C' && hexs(cv.data) === '00 01 2C');
  ok('C4 detail shows user-defined decoder note (not ODX)', /사용자 정의 디코더 \(SIM, ODX 아님\)/.test($('fields').textContent));
  /* ---- C5 definition change: old txn keeps snapshot ---- */
  const r5 = C.updateDid(0xF1A0, { name: 'Temp2', type: 'uint', recLen: '3', byteOffset: '0', length: '3', scale: '1', offset: '0', decimals: '0', unit: 'cnt' });
  o.selectTxn(t.id); o.flush();
  ok('C5 DID redefined; earlier txn detail still decodes with request-time definition', r5.ok && U.didInfo(0xF1A0).name === 'Temp2' && t.didDef.type === 'sint' && /-10\.0/.test($('fields').textContent));
  H.send({ mode: 'did', didText: 'F1A0' }); clock.advance(80);
  ok('C5 new read uses new definition (uint 3 B = 300 cnt)', E.confirmed.get('ABS|' + 0xF1A0).value === '300' && E.confirmed.get('ABS|' + 0xF1A0).unit === 'cnt');
  ok('C5 per-ECU fixture edit (setFixture GW only)', C.setFixture('GW', 0xF1A0, '01 02 03').ok && hexs(C.fixture('GW', 0xF1A0)) === '01 02 03' && hexs(C.fixture('ABS', 0xF1A0)) === '00 01 2C');
  ok('C5 fixture removed → SIM ECU 7F 22 31', C.setFixture('ABS', 0xF1A0, null).ok && (H.send({ mode: 'did', didText: 'F1A0' }), clock.advance(80), hexs(E.txns[E.txns.length - 1].final) === '7F 22 31'));
  /* ---- C6 rename keeps existing labels; address change guarded ---- */
  const tAbs = E.txns.find(x => x.ecu === 'ABS');
  ok('C6 rename while online allowed (name only); txn labels immutable', C.updateEcu('ABS', { name: 'ABS renamed', doipLA: '1020', canTx: '760', canRx: '768' }).ok && tAbs.ecuName === 'ABS SIM' && tAbs.ecu === 'ABS');
  ok('C6 address change while online refused (no silent disconnect)', C.updateEcu('ABS', { name: 'ABS renamed', doipLA: '1021', canTx: '760', canRx: '768' }).code === 'online' && E.ctx('ABS', 'doip').link === 'online');
  /* ---- C7 delete guards ---- */
  H.tools.open('catalog');
  click(row('ecu', 'ABS').querySelector('[data-ct="ecu-del"]')); o.flush();
  ok('C7 delete step 1 shows blocking reasons (online), explicit disconnect button, nothing deleted', /삭제 불가/.test($('ctEcuDel').textContent) && /DoIP 연결됨/.test($('ctEcuDel').textContent) && !!d.querySelector('[data-ct="ecu-disc"]') && U.ECUS.some(e => e.id === 'ABS'));
  click(row('ecu', 'ABS').querySelector('[data-ct="ecu-del"]'));
  ok('C7 second click still refused while online', U.ECUS.some(e => e.id === 'ABS') && E.ctx('ABS', 'doip').link === 'online');
  click('[data-ct="ecu-disc"]'); o.flush();
  ok('C7 explicit [disconnect] → offline', E.ctx('ABS', 'doip').link === 'offline');
  let stopped = 0; const unA = H.activity.register('qa-watch', { list: () => (stopped ? [] : [{ ecu: 'ABS', transport: 'doip', kind: 'watch', label: 'QA watch' }]), stop() { stopped++; } });
  ok('C7 offline-but-running automation still blocks delete', /QA watch/.test((o.render(), $('ctEcuDel').textContent)) && /자동 작업 중지/.test($('ctEcuDel').textContent));
  click(row('ecu', 'ABS').querySelector('[data-ct="ecu-del"]'));
  ok('C7 delete refused while activity running', U.ECUS.some(e => e.id === 'ABS') && !stopped);
  click('[data-ct="ecu-stopact"]'); o.flush();
  ok('C7 explicit [stop automation] calls owner stop', stopped === 1);
  ok('C7 blockers cleared → box switches to explicit confirm text naming profiles removed; still not deleted', /프로필 1개/.test($('ctEcuDel').textContent) && U.ECUS.some(e => e.id === 'ABS'));
  click(row('ecu', 'ABS').querySelector('[data-ct="ecu-del"]')); o.flush();
  ok('C7 deleted: ECU gone, its profiles gone, current ECU falls back to preset, history rows keep ABS', !U.ECUS.some(e => e.id === 'ABS') && !E.exportConfig().profiles.some(p => p.ecu === 'ABS') && o.st.ecu === 'BCM' && E.txns.some(x => x.ecu === 'ABS') && !E.txns.some(x => x.status === 'cancelled'));
  unA();
  /* ---- C8 persistence + reload ---- */
  C.addEcu({ id: 'EPS', name: 'EPS SIM', doipLA: '1030', canTx: '770', canRx: '778' });
  E.saveProfile('EPS', 'cantp', { targetId: 'preset:EPS|cantp', name: 'EPS bench', values: Object.assign({}, U.presetValues('EPS', 'cantp'), { bitrate: 250000 }) });
  o.setContext('EPS', 'cantp');
  const raw = w.localStorage.getItem(PK), sp = JSON.parse(raw);
  ok('C8 stored: schema 2, additive catalog key with custom ECU/DID/fixtures, no history/link', sp.schema === 2 && sp.catalog.format === 'loom-uds-catalog/1' && sp.catalog.ecus.length === 1 && sp.catalog.dids.length === 1 && sp.catalog.fixtures.GW && !raw.includes('"txns"') && !raw.includes('online'));
  const b2 = start({ [PK]: raw });
  ok('C8 reload: custom ECU before saved.ecu validation → EPS restored as current, CAN-TP', b2.o.st.ecu === 'EPS' && b2.o.st.transport === 'cantp');
  ok('C8 reload: custom-ECU profile not dropped, active; DID + fixture restored; no notice', b2.o.E.activeProfile('EPS', 'cantp').name === 'EPS bench' && b2.U.didInfo(0xF1A0).name === 'Temp2' && hexs(b2.o.E.catalog.fixture('GW', 0xF1A0)) === '01 02 03' && !b2.o.st.cfgNotice);
  ok('C8 reload: nothing connected/sent, no timers', b2.o.E.txns.length === 0 && b2.U.ECUS.every(e => b2.U.TRANSPORTS.every(t => b2.o.E.ctx(e.id, t.id).link === 'offline')) && b2.clock.pending() === 0);
  /* ---- C9 migration + corrupt entries ---- */
  const old = JSON.parse(raw); delete old.catalog;
  const b3 = start({ [PK]: JSON.stringify(old) });
  ok('C9 old schema-2 prefs without catalog: presets only; EPS profile dropped with notice; ecu falls back', b3.U.ECUS.length === 2 && b3.o.st.ecu === 'BCM' && /검증에 실패/.test(b3.o.st.cfgNotice));
  const bad = JSON.parse(raw); bad.catalog.ecus.push({ id: 'BCM', name: 'dup' }, { id: 'ZZ', name: 'x', doipLA: 'GG', canTx: '1', canRx: '2' }); bad.catalog.dids.push({ did: 'F190', name: 'x', type: 'ascii', recLen: 1, byteOffset: 0, length: 1 });
  const b4 = start({ [PK]: JSON.stringify(bad) });
  ok('C9 corrupt catalog entries dropped individually (valid EPS kept), visible notice', b4.U.ECUS.map(e => e.id).join() === 'BCM,GW,EPS' && b4.U.DIDS.length === 4 && /ECU\/DID 3개/.test(b4.o.st.cfgNotice));
  const b5 = start({ [PK]: JSON.stringify(Object.assign({}, old, { catalog: { format: 'nope' } })) });
  ok('C9 wrong catalog format → presets only', b5.U.ECUS.length === 2);
  /* ---- C10 bounds ---- */
  const E6 = b5.o.E;
  let n = 0; for (let i = 0; i < 40; i++) if (E6.catalog.addEcu({ id: 'E' + i, name: 'n', doipLA: (0x2000 + i).toString(16), canTx: (0x100 + i).toString(16), canRx: (0x300 + i).toString(16) }).ok) n++;
  ok('C10 custom ECU bound 32', n === 32 && E6.catalog.addEcu({ id: 'EX', name: 'n', doipLA: '3000', canTx: '500', canRx: '501' }).code === 'limit');
  ok('C10 fixture bound 255 B', !E6.catalog.parseFixture(Array(256).fill('00').join(' ')).ok && E6.catalog.parseFixture(Array(255).fill('00').join(' ')).ok);
  ok('C10 sint/uint length ≤ 6, scale ≠ 0', !E6.catalog.checkDid({ did: 'A000', name: 'x', type: 'uint', recLen: 8, byteOffset: 0, length: 7, scale: 1 }).ok && !E6.catalog.checkDid({ did: 'A000', name: 'x', type: 'uint', recLen: 2, byteOffset: 0, length: 2, scale: 0 }).ok);
  const le = U.decodeCustom({ type: 'uint', recLen: 2, byteOffset: 0, length: 2, endian: 'little', scale: 1, offset: 0, decimals: 0, unit: '' }, [0x34, 0x12]);
  const ss = U.decodeCustom({ type: 'sint', recLen: 1, byteOffset: 0, length: 1, endian: 'big', scale: 1, offset: 0, decimals: 0, unit: '' }, [0xFF]);
  ok('C10 decoders: LE uint 0x1234=4660, sint 0xFF=-1, ascii/bytes', le.text === '4660' && ss.text === '-1' && U.decodeCustom({ type: 'ascii', recLen: 2, byteOffset: 0, length: 2 }, [0x41, 0x42]).text === 'AB' && U.decodeCustom({ type: 'bytes', recLen: 3, byteOffset: 1, length: 2 }, [1, 2, 3]).text === '02 03');
} catch (e) { ok('no exception', false, String(e && e.stack || e)); }
finally { wins.forEach(w => { try { clearInterval(w.loomUdsOpus.tickH); clearInterval(w.LoomUdsServices && w.LoomUdsServices.tickH); w.close(); } catch (e) { /* ignore */ } }); }
const fail = results.filter(r => !r.pass).length;
console.log(`\n${results.length - fail}/${results.length} PASS`);
require('fs').writeFileSync(require('path').join(__dirname, 'catalog-check-result.json'), JSON.stringify({ when: new Date().toISOString(), note: 'jsdom source-local, not browser acceptance', results }, null, 2));
process.exitCode = fail ? 1 : 0;
