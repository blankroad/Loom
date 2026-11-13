// Source-local configuration checks (jsdom + fake clock; NOT browser acceptance).
// Run from repo root:  node work/reports/uds-read-did/app/selfcheck/config-check.cjs
const fs = require('fs'), path = require('path');
const { boot, fakeClock } = require('./harness.cjs');
const results = [], wins = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const start = storage => { const b = boot(storage); wins.push(b.w); return b; };
const PK = 'loom.udsReadDid.v1';
function ui(b) {
  const { w, d, o } = b;
  const click = s => { const el = typeof s === 'string' ? d.querySelector(s) : s; if (!el) throw new Error('missing ' + s); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); };
  const inp = (id, v) => { const e = d.getElementById(id); e.value = v; e.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const sel = (id, v) => { const e = d.getElementById(id); e.value = v; e.dispatchEvent(new w.Event('change', { bubbles: true })); };
  const save = () => d.getElementById('cfgPanel').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const key = (el, k) => el.dispatchEvent(new w.KeyboardEvent('keydown', { key: k, bubbles: true }));
  const route = tr => click(`#segRoute [data-tr="${tr}"]`);
  const ecu = id => sel('selEcu', id);
  const err = k => (d.getElementById(`cf-${k}-err`) || {}).textContent || '';
  return { click, inp, sel, save, key, route, ecu, err, flush: () => o.flush(), txt: id => d.getElementById(id).textContent, $: id => d.getElementById(id) };
}

try {
  const U0 = start().U, V = U0.validateProfile;
  /* ---- A. validation (pure) ---- */
  const doip = p => V('doip', Object.assign({ channel: 'ETH1', host: '192.168.10.20', port: '13400', testerLA: '0E00', ecuLA: '1001', p2: '1000', p2star: '5000' }, p));
  const cantp = p => V('cantp', Object.assign({ channel: 'CAN1', bitrate: '500000', idFormat: '11', txId: '740', rxId: '748', p2: '1000', p2star: '5000' }, p));
  const eth = p => V('ethdiag', Object.assign({ channel: 'ETH1', adapter: 'Vendor X', endpoint: '', p2: '1000', p2star: '5000' }, p));
  ok('host: IPv4 / hostname / trailing-dot FQDN accepted', ['10.0.0.1', '255.255.255.255', 'ecu-test.local', 'gw.lab.example.', 'a'].every(h => doip({ host: h }).ok));
  const badHosts = { '': 1, '256.1.1.1': 1, '10.01.0.1': 1, '10.0.0': 1, 'fe80::1': 1, '[::1]': 1, 'host:13400': 1, 'bad_host': 1, '-a.b': 1, 'a..b': 1, 'h st': 1, '1.2.3.4.5': 1 };
  ok('host: invalid IPv4/IPv6/host:port/underscore/empty labels rejected with field error', Object.keys(badHosts).every(h => !doip({ host: h }).ok && doip({ host: h }).errors.host), Object.keys(badHosts).filter(h => doip({ host: h }).ok));
  ok('host: IPv6 restriction explicit (not silent)', /IPv6/.test(doip({ host: 'fe80::1' }).errors.host));
  ok('port: 1 and 65535 ok; 0, 65536, 13400.5, 1e3, -1, empty rejected', doip({ port: '1' }).ok && doip({ port: '65535' }).ok && ['0', '65536', '13400.5', '1e3', '-1', '', ' '].every(p => doip({ port: p }).errors.port));
  ok('logical address: 0x prefix ok, 5 hex digits rejected (no truncation), non-hex rejected', doip({ testerLA: '0x0E80' }).values.testerLA === 0x0E80 && /잘라내지 않음/.test(doip({ ecuLA: '10001' }).errors.ecuLA) && doip({ ecuLA: 'G001' }).errors.ecuLA);
  ok('logical address: tester == ECU rejected', !!doip({ ecuLA: '0E00' }).errors.ecuLA);
  ok('CAN 11-bit: 7FF ok, 800 rejected; 29-bit: 1FFFFFFF ok, 20000000 rejected', cantp({ txId: '7FF' }).ok && cantp({ txId: '800' }).errors.txId && cantp({ idFormat: '29', txId: '1FFFFFFF', rxId: '18DAF101' }).ok && cantp({ idFormat: '29', txId: '20000000' }).errors.txId);
  ok('CAN: format switch 29->11 with 18DA00F1 flags field, never truncates', /11-bit/.test(cantp({ txId: '18DA00F1' }).errors.txId) && cantp({ txId: '18DA00F1' }).values === null);
  ok('CAN: Tx == Rx rejected, bitrate must be listed metadata', cantp({ rxId: '740' }).errors.rxId && cantp({ bitrate: '333000' }).errors.bitrate);
  ok('timing: integers in range; decimal/exponent/out-of-range/P2*<P2 rejected', doip({ p2: '50', p2star: '100' }).ok && ['1000.5', '1e3', '49', '30001', 'abc'].every(v => doip({ p2: v }).errors.p2) && doip({ p2: '3000', p2star: '2000' }).errors.p2star && doip({ p2star: '60001' }).errors.p2star);
  ok('EthDiag: adapter required, control chars/oversize endpoint rejected; no DoIP fields', !eth({ adapter: '' }).ok && eth({ endpoint: 'a\u0001' }).errors.endpoint && eth({ endpoint: 'x'.repeat(65) }).errors.endpoint && !('host' in eth({}).values) && !('ecuLA' in eth({}).values));
  ok('channel: only SIM channels', doip({ channel: 'ETH3' }).errors.channel && cantp({ channel: 'ETH1' }).errors.channel);

  /* ---- B. presets preserved + reset path; engine ---- */
  const c = fakeClock(), U = U0, E = U.createEngine({ clock: c });
  const pv = JSON.stringify(U.presetValues('BCM', 'doip'));
  const D = (over) => Object.assign({}, U.presetValues('BCM', 'doip'), over);
  const r1 = E.saveProfile('BCM', 'doip', { targetId: 'preset:BCM|doip', name: 'Lab', values: D({ host: '10.0.0.5', ecuLA: 0x1F01 }) });
  ok('save preset-target with changes creates custom u1 r1 (base preset) and applies it', r1.ok && r1.action === 'created' && r1.profile.id === 'u1' && r1.profile.rev === 1 && r1.profile.base === 'preset:BCM|doip' && E.activeProfile('BCM', 'doip').id === 'u1');
  ok('preset object immutable and values unchanged', Object.isFrozen(U.presetProfile('BCM', 'doip').values) && JSON.stringify(U.presetProfile('BCM', 'doip').values) === pv && U.route('BCM', 'doip').summary === 'ETH1 · 0x0E00→0x1001');
  /* C. invalid save: no mutation, id counter untouched */
  const snap = () => JSON.stringify(E.exportConfig());
  let before = snap();
  const bad = E.saveProfile('BCM', 'doip', { targetId: 'new', name: 'X', values: D({ host: 'fe80::1' }) });
  const badName = E.saveProfile('BCM', 'doip', { targetId: 'new', name: 'Lab', values: D({}) });
  ok('invalid values / duplicate name save refused, store unchanged', !bad.ok && bad.code === 'invalid' && bad.errors.host && !badName.ok && badName.errors.name && snap() === before);
  const r2 = E.saveProfile('BCM', 'doip', { targetId: 'u1', name: 'Lab', values: D({ host: '10.0.0.6', ecuLA: 0x1F01 }) });
  ok('update custom: same id, rev 2', r2.ok && r2.action === 'updated' && r2.profile.id === 'u1' && r2.profile.rev === 2);
  const r3 = E.saveProfile('BCM', 'doip', { targetId: 'new', name: 'Lab2', values: D({ port: 14000 }) });
  ok('id counter not consumed by refused saves (next is u2)', r3.ok && r3.profile.id === 'u2');
  const rp = E.saveProfile('BCM', 'doip', { targetId: 'preset:BCM|doip', name: '', values: U.presetValues('BCM', 'doip') });
  ok('reset path: apply unchanged preset -> active preset, customs kept', rp.ok && rp.action === 'preset' && E.activeProfile('BCM', 'doip').source === 'preset' && E.customCount() === 2);

  /* ---- D. online guard (engine) + snapshot ---- */
  E.saveProfile('BCM', 'doip', { targetId: 'u1', name: 'Lab', values: D({ host: '10.0.0.6', ecuLA: 0x1F01 }) });
  E.connect('BCM', 'doip');
  const tx1 = E.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'pending', pending: 2, latency: 400 } }).txn;
  before = snap();
  const on1 = E.saveProfile('BCM', 'doip', { targetId: 'u1', name: 'Lab', values: D({ host: '10.9.9.9', ecuLA: 0x1F01 }) });
  const on2 = E.saveProfile('BCM', 'doip', { targetId: 'preset:BCM|doip', values: U.presetValues('BCM', 'doip') });
  const on3 = E.deleteProfile('BCM', 'doip', 'u1');
  ok('online: save/apply-preset/delete-active refused in engine; no mutation, no disconnect/abort', [on1, on2, on3].every(r => !r.ok && r.code === 'online') && snap() === before && E.ctx('BCM', 'doip').link === 'online' && tx1.status === 'waiting');
  ok('txn snapshot: profile u1 r3, host 10.0.0.6, timing 1000/5000', tx1.profile.id === 'u1' && tx1.profile.rev === 3 && tx1.route.host === '10.0.0.6:13400' && tx1.timing.p2 === 1000 && tx1.timing.p2star === 5000);
  const delOther = E.deleteProfile('BCM', 'doip', 'u2');
  ok('online: deleting a non-active custom allowed, does not touch connection', delOther.ok && !delOther.wasActive && E.ctx('BCM', 'doip').conn.route.profile.id === 'u1');
  const cOther = E.saveProfile('BCM', 'cantp', { targetId: 'new', name: 'CANx', values: Object.assign({}, U.presetValues('BCM', 'cantp'), { idFormat: '29', txId: 0x18DA01F1, rxId: 0x18DAF101 }) });
  ok('other context (BCM CAN-TP, offline) saves independently while DoIP in flight', cOther.ok && tx1.status === 'waiting' && E.routeFor('BCM', 'cantp').summary === 'CAN1 · 0x18DA01F1→0x18DAF101');
  c.advance(400 + 2400);
  ok('in-flight txn completes positive with original snapshot', tx1.status === 'positive' && tx1.route.host === '10.0.0.6:13400' && E.confirmed.get('BCM|' + 0xF190).profile.rev === 3);
  E.disconnect('BCM', 'doip');
  E.saveProfile('BCM', 'doip', { targetId: 'u1', name: 'Lab', values: D({ host: 'ecu-test.local', port: 15000, ecuLA: 0x1F01, p2: 2000 }) });
  E.connect('BCM', 'doip');
  const tx2 = E.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'positive', latency: 80 } }).txn; c.advance(80);
  ok('after disconnect/save/reconnect: #2 uses r4 ecu-test.local:15000 P2 2000; #1 keeps r3 10.0.0.6', tx2.profile.rev === 4 && tx2.route.host === 'ecu-test.local:15000' && tx2.timing.p2 === 2000 && tx1.profile.rev === 3 && tx1.route.host === '10.0.0.6:13400');
  const cvp = E.confirmed.get('BCM|' + 0xF190);
  ok('confirmed value provenance = #2 r4 (monotonic rule intact)', cvp.no === tx2.no && cvp.profile.rev === 4 && /ecu-test/.test(JSON.stringify(E.byId(cvp.txnId).route)));
  const rows = E.exportRows();
  ok('trace export rows carry profile id/rev/source, timing and config values per request', rows[0].profileId === 'u1' && rows[0].profileRev === 3 && rows[0].config.host === '10.0.0.6' && rows[1].profileRev === 4 && rows[1].p2Ms === 2000 && rows[1].p2starMs === 5000 && rows[1].config.port === 15000 && rows[0].connectionGen === 1 && rows[1].connectionGen === 3);
  E.disconnect('BCM', 'doip');
  E.deleteProfile('BCM', 'doip', 'u1');
  ok('deleting the active profile (offline) falls back to preset; old txns keep provenance', E.activeProfile('BCM', 'doip').source === 'preset' && tx1.profile.name === 'Lab' && tx2.route.host === 'ecu-test.local:15000' && cvp.profile.id === 'u1');
  ok('ECU/route isolation: BCM customs not listed for GW DoIP or BCM EthDiag', E.profilesFor('GW', 'doip').length === 1 && E.profilesFor('BCM', 'ethdiag').length === 1 && E.routeFor('GW', 'doip').profile.source === 'preset');
  ok('cross-context target rejected (cannot save BCM CAN profile into GW CAN-TP)', E.saveProfile('GW', 'cantp', { targetId: cOther.profile.id, name: 'CANx', values: cOther.profile.values }).code === 'target');

  /* ---- E. creation cap ---- */
  const c3 = fakeClock(), E3 = U.createEngine({ clock: c3 }), MAX = U.MAX_PROFILES;
  for (let i = 0; i < MAX; i++) E3.saveProfile('GW', 'doip', { targetId: 'new', name: 'P' + i, values: Object.assign({}, U.presetValues('GW', 'doip'), { port: 20000 + i }) });
  before = JSON.stringify(E3.exportConfig());
  const over = E3.saveProfile('GW', 'doip', { targetId: 'new', name: 'Pover', values: U.presetValues('GW', 'doip') });
  const overPreset = E3.saveProfile('GW', 'doip', { targetId: 'preset:GW|doip', name: 'Pover2', values: Object.assign({}, U.presetValues('GW', 'doip'), { port: 1 }) });
  ok(`cap: ${MAX} customs, create (new or from changed preset) refused code limit, store/active unchanged`, E3.customCount() === MAX && !over.ok && over.code === 'limit' && overPreset.code === 'limit' && JSON.stringify(E3.exportConfig()) === before);
  const upd = E3.saveProfile('GW', 'doip', { targetId: 'u5', name: 'P4', values: Object.assign({}, U.presetValues('GW', 'doip'), { port: 30000 }) });
  const presetApply = E3.saveProfile('GW', 'doip', { targetId: 'preset:GW|doip', values: U.presetValues('GW', 'doip') });
  ok('cap: update existing and unchanged preset apply still allowed', upd.ok && upd.action === 'updated' && presetApply.ok && presetApply.action === 'preset');
  E3.deleteProfile('GW', 'doip', 'u1');
  const after = E3.saveProfile('GW', 'doip', { targetId: 'new', name: 'Pnew', values: U.presetValues('GW', 'doip') });
  ok(`cap: refused creates consumed no id (next after delete is u${MAX + 1})`, after.ok && after.profile.id === 'u' + (MAX + 1));
  const E4 = U.createEngine({ clock: fakeClock() }), ld = E4.loadConfig(JSON.parse(JSON.stringify(E3.exportConfig())));
  ok('cap: a full saved set reloads without drops', ld.dropped === 0 && E4.customCount() === MAX);

  /* ---- F. timing behaviour ---- */
  const c5 = fakeClock(), E5 = U.createEngine({ clock: c5 });
  const D5 = o => Object.assign({}, U.presetValues('BCM', 'doip'), o);
  E5.connect('BCM', 'doip');
  const tdef = E5.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'timeout' } }).txn;
  c5.advance(999); const w999 = tdef.status; c5.advance(1);
  ok('preset defaults preserved: timeout exactly at P2 1000 ms', w999 === 'waiting' && tdef.status === 'timeout' && tdef.tEnd - tdef.t0 === 1000);
  E5.disconnect('BCM', 'doip');
  E5.saveProfile('BCM', 'doip', { targetId: 'new', name: 'fast', values: D5({ p2: 300, p2star: 500 }) });
  E5.connect('BCM', 'doip');
  const tf = E5.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'positive', latency: 400 } }).txn;
  c5.advance(300); const s300 = tf.status; c5.advance(100);
  ok('P2 300 < SIM latency 400: timeout at 300, reply at 400 recorded late/ignored, nothing confirmed', s300 === 'timeout' && tf.tEnd - tf.t0 === 300 && tf.events.some(e => e.kind === 'late' && e.ignored) && !E5.confirmed.size && tf.sim.latency === 400);
  const tp = E5.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'pending', pending: 2, latency: 80 } }).txn;
  c5.advance(80 + 500);
  ok('P2* 500 < SIM 0x78 interval 1200: timeout 500 ms after first 0x78 (SIM interval independent)', tp.status === 'timeout' && tp.tEnd - tp.t0 === 580 && tp.pendingCount === 1 && /P2\* 500/.test(tp.events[tp.events.length - 1].note) && tp.sim.pendingInterval === 1200);
  c5.advance(5000);
  ok('later 0x78/62 after P2* timeout are late & ignored', tp.status === 'timeout' && tp.events.filter(e => e.kind === 'late').length === 2);
  const tl = E5.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: { scenario: 'timeout', late: true } }).txn;
  c5.advance(300 + 1200);
  ok('late-reply SIM offset follows configured P2 (+1200)', tl.events.some(e => e.kind === 'late' && e.t - tl.t0 === 1500));

  /* ---- G. persistence / migration ---- */
  const v1 = { ecu: 'GW', transport: 'cantp', didText: 'F18C', sim: { scenario: 'nrc', nrc: 0x31 }, filter: 'ecu' };
  const bM = start({ [PK]: JSON.stringify(v1) }), uM = ui(bM);
  bM.clock.advance(10000); uM.flush();
  ok('v1 prefs migrate: ECU/route/DID/SIM restored, presets active, no link/txn/timers', bM.o.st.ecu === 'GW' && bM.o.st.transport === 'cantp' && bM.o.st.didText === 'F18C' && bM.o.st.sim.nrc === 0x31 && bM.o.E.activeProfile('GW', 'cantp').source === 'preset' && !bM.o.E.txns.length && bM.o.curCtx().link === 'offline' && bM.clock.pending() === 0);
  uM.click('#btnCfg'); uM.inp('cf-txId', '7E1'); uM.save(); uM.flush();
  const sM = JSON.parse(bM.w.localStorage.getItem(PK));
  ok('after save: schema 2 written to same key, v1 top-level prefs kept, config holds 1 profile + active', sM.schema === 2 && sM.ecu === 'GW' && sM.didText === 'F18C' && sM.config.profiles.length === 1 && sM.config.active['GW|cantp'] === sM.config.profiles[0].id && Object.keys(bM.w.localStorage).sort().join() === [PK, 'unrelated.key'].sort().join());
  ok('storage: no history/link/draft persisted', !/LOOMSIM|"link"|drafts|txns/.test(bM.w.localStorage.getItem(PK)));
  const bR = start({ [PK]: JSON.stringify(sM) }); bR.clock.advance(10000); bR.o.flush();
  ok('reload: custom profile restored & active; still offline, no txn/timers', bR.o.E.activeProfile('GW', 'cantp').values.txId === 0x7E1 && bR.o.curCtx().link === 'offline' && !bR.o.E.txns.length && bR.clock.pending() === 0);
  const corrupt = JSON.parse(JSON.stringify(sM));
  corrupt.config.profiles.push({ id: 'u7', ecu: 'BCM', transport: 'doip', name: 'bad', rev: 1, values: Object.assign({}, U.presetValues('BCM', 'doip'), { host: '999.1.1.1' }) });
  corrupt.config.profiles.push({ id: sM.config.profiles[0].id, ecu: 'BCM', transport: 'doip', name: 'dupid', rev: 1, values: U.presetValues('BCM', 'doip') });
  corrupt.config.active['BCM|doip'] = 'u7';
  const bC = start({ [PK]: JSON.stringify(corrupt) }); bC.o.flush();
  ok('corrupt saved entries dropped (3), valid kept, active falls back to preset, notice shown', bC.o.E.customCount() === 1 && bC.o.E.activeProfile('BCM', 'doip').source === 'preset' && /3개/.test(bC.o.st.cfgNotice) && bC.o.E.activeProfile('GW', 'cantp').values.txId === 0x7E1);
  const bJ = start({ [PK]: '{bad json' }), bS = start({ [PK]: JSON.stringify({ schema: 3, ecu: 'GW', config: { profiles: [] } }) });
  ok('unparseable prefs -> defaults + notice; unknown schema -> profiles ignored + notice', bJ.o.st.ecu === 'BCM' && /손상/.test(bJ.o.st.cfgNotice) && /schema 3/.test(bS.o.st.cfgNotice) && bS.o.E.customCount() === 0);

  /* ---- H. import / export (atomic) ---- */
  const bI = start(), uI = ui(bI), EI = bI.o.E, CI = bI.w.LoomUdsConfig;
  uI.click('#btnCfg'); uI.inp('cf-host', '10.2.2.2'); uI.save(); uI.flush();
  const text = CI.exportCfg(), exp = JSON.parse(text);
  ok('config export: own format, customs only, separate file name', exp.format === 'loom-uds-read-did-config/1' && exp.profiles.length === 1 && /config-.*\.json$/.test(bI.w.__dl[bI.w.__dl.length - 1]));
  before = JSON.stringify(EI.exportConfig());
  const bads = ['{nope', JSON.stringify({ format: 'x', profiles: [] }), JSON.stringify({ format: exp.format, profiles: [] }),
    JSON.stringify({ format: exp.format, profiles: [exp.profiles[0], Object.assign({}, exp.profiles[0], { values: Object.assign({}, exp.profiles[0].values, { port: 0 }) })] }),
    JSON.stringify({ format: exp.format, profiles: [Object.assign({}, exp.profiles[0], { ecu: 'XX' })] })];
  const rs = bads.map(t => CI.importText(t));
  ok('import: malformed/wrong format/empty/one-bad-entry/unknown ECU all rejected, store unchanged', rs.every(r => !r.ok) && JSON.stringify(EI.exportConfig()) === before && /port/.test(rs[3].error));
  EI.connect('BCM', 'doip');
  const ri = CI.importText(text);
  ok('import valid: added as new id, name collision renamed, active/link unchanged, no txn', ri.ok && ri.added === 1 && ri.renamed === 1 && EI.customCount() === 2 && EI.activeProfile('BCM', 'doip').id === exp.profiles[0].id && EI.ctx('BCM', 'doip').link === 'online' && !EI.txns.length);
  EI.disconnect('BCM', 'doip');

  /* ---- I. UI lifecycle ---- */
  const b = start(), u = ui(b), o = b.o, d = b.d, C = b.w.LoomUdsConfig;
  u.click('#btnCfg'); u.flush();
  ok('panel opens scoped (BCM · DoIP), preset target, Save disabled (already applied), focus in panel', !u.$('cfgPanel').hidden && /BCM · DoIP/.test(u.txt('cfgTitle')) && u.$('cfgSave').disabled && d.activeElement.id === 'cfgProfile');
  u.inp('cf-host', '999.1.1.1'); u.inp('cf-port', '0'); u.inp('cf-ecuLA', '0E00');
  ok('field-local errors live (host/port/ecuLA) with aria-invalid', /0–255/.test(u.err('host')) && /1–65535/.test(u.err('port')) && /Tester/.test(u.err('ecuLA')) && u.$('cf-host').getAttribute('aria-invalid') === 'true');
  const storeB = b.w.localStorage.getItem(PK); u.save(); u.flush();
  ok('invalid Save: no mutation (customs 0, storage same), focus first invalid field', o.E.customCount() === 0 && b.w.localStorage.getItem(PK) === storeB && d.activeElement.id === 'cf-host');
  ok('profile select locked while dirty', u.$('cfgProfile').disabled);
  u.key(u.$('cf-host'), 'Escape'); u.flush();
  ok('Escape closes, keeps draft (dirty dot), focus returns to button', u.$('cfgPanel').hidden && !u.$('cfgDirty').hidden && d.activeElement.id === 'btnCfg');
  u.route('cantp'); u.click('#btnCfg'); u.flush();
  ok('switch route: CAN-TP panel shows its own clean preset draft (no leak)', /CAN-TP/.test(u.txt('cfgTitle')) && u.$('cf-txId').value === '740' && u.$('cfgDirty').hidden);
  u.sel('cf-idFormat', '29'); u.inp('cf-txId', '18DA01F1'); u.sel('cf-idFormat', '11');
  ok('CAN format change flags ID instead of truncating; hint follows format', /11-bit/.test(u.err('txId')) && u.$('cf-txId').value === '18DA01F1' && /11-bit/.test(u.txt('cf-txId-hint')));
  u.click('[data-cfg="cancel"]'); u.flush();
  ok('Cancel discards CAN-TP draft and closes', u.$('cfgPanel').hidden && !C.drafts.has('BCM|cantp'));
  u.route('doip'); u.click('#btnCfg'); u.flush();
  ok('switch back: DoIP dirty draft retained (host 999.1.1.1)', u.$('cf-host').value === '999.1.1.1' && C.isDirty(C.draft()));
  u.click('[data-cfg="revert"]'); u.flush();
  ok('Revert restores saved values', u.$('cf-host').value === '192.168.10.20' && !C.isDirty(C.draft()));
  u.inp('cf-host', '192.0.2.10'); u.inp('cf-port', '14500'); u.inp('cf-name', 'QA'); u.inp('cf-p2', '2000');
  ok('Save label explains new custom from preset', /새 프로필/.test(u.txt('cfgSave')) && !u.$('cfgSave').disabled);
  u.save(); u.flush();
  ok('Save applies: provenance chip, route summary/req target show custom, presets intact', /사용자 "QA" r1/.test(u.txt('cfgProv')) && /사용자 "QA" r1/.test(u.txt('reqTarget')) && /P2 2000/.test(u.txt('reqTarget')) && U.presetProfile('BCM', 'doip').values.host === '192.168.10.20');
  u.click('#btnLink'); u.flush();
  u.inp('cf-host', 'ecu-test.local');
  ok('online: draft editable, Save disabled, status + explicit disconnect action shown', u.$('cf-host').value === 'ecu-test.local' && u.$('cfgSave').disabled && !u.$('cfgDisc').hidden && /연결 해제 후/.test(u.txt('cfgStatus')));
  o.st.sim = Object.assign(o.st.sim, { scenario: 'pending', pending: 2, latency: 400 }); o.doRead(); u.flush();
  const ta = o.E.txns[0];
  ok('in-flight request: disconnect button names the request it will end', /#1/.test(u.txt('cfgDisc')));
  u.save(); u.flush();
  ok('submit while online refused (engine), request still in flight, link still online', ta.status === 'waiting' && o.curCtx().link === 'online' && o.E.activeProfile('BCM', 'doip').rev === 1);
  ok('detail shows request-time profile + timing snapshot', /사용자 "QA" r1/.test(u.txt('fields')) && /P2 2000 ms/.test(u.txt('fields')));
  u.click('#cfgDisc'); u.flush();
  ok('explicit disconnect from panel ends #1 as link-lost (user action)', ta.status === 'link-lost' && o.curCtx().link === 'offline');
  u.save(); u.flush();
  ok('offline Save -> r2 applied; #1 keeps r1 192.0.2.10', o.E.activeProfile('BCM', 'doip').rev === 2 && ta.profile.rev === 1 && ta.route.host === '192.0.2.10:14500');
  u.click('[data-cfg="delete"]'); u.flush();
  ok('Delete needs confirmation (first click arms)', o.E.customCount() === 1 && u.txt('cfgDel') === '삭제 확인');
  u.click('[data-cfg="delete"]'); u.flush();
  ok('Delete confirmed: active falls back to preset with message; history row unchanged', o.E.customCount() === 0 && o.E.activeProfile('BCM', 'doip').source === 'preset' && /프리셋으로 복귀/.test(u.txt('cfgStatus')) && /192\.0\.2\.10/.test(d.querySelector(`#tx-${ta.id} .c-rt`).title + ta.route.host));
  u.click('[data-cfg="dup"]'); u.flush();
  ok('Duplicate creates unsaved new draft named …사본', u.$('cfgProfile').value === 'new' && /사본/.test(u.$('cf-name').value) && C.isDirty(C.draft()));
  u.click('[data-cfg="cancel"]'); u.flush();
  // C2 (root native): Escape keeps draft; Cancel discards; reopen must show SAVED values in the DOM, not stale inputs
  u.click('#btnCfg'); u.inp('cf-host', 'unsaved-qa.local'); u.key(u.$('cf-host'), 'Escape'); u.flush();
  u.click('#btnCfg'); u.flush();
  const escKept = u.$('cf-host').value === 'unsaved-qa.local';
  u.click('[data-cfg="cancel"]'); u.flush(); u.click('#btnCfg'); u.flush();
  ok('C2: after Escape-reopen draft shown; after Cancel+reopen #cf-host DOM = saved host', escKept && u.$('cf-host').value === o.E.activeProfile('BCM', 'doip').values.host && u.$('cfgDirty').hidden);
  u.inp('cf-port', '1234'); u.click('[data-cfg="cancel"]'); u.flush(); u.click('#btnCfg'); u.flush();
  ok('C2: Cancel twice in a row (same target) still rebuilds DOM from saved (port)', u.$('cf-port').value === String(o.E.activeProfile('BCM', 'doip').values.port));
  u.click('[data-cfg="cancel"]'); u.flush();
  u.click('#btnSim'); u.flush();
  ok('SIM panel separately labelled; client timing shown from profile', /SIM 전용/.test(u.txt('simPanel')) && /현재 P2 1000 ms/.test(u.txt('simTiming')));
  u.click('#btnCfg'); u.flush();
  ok('accessibility: every config input/select has a label[for]', [...d.querySelectorAll('#cfgPanel input[data-k], #cfgPanel select[data-k], #cfgProfile')].every(el => d.querySelector(`label[for="${el.id}"]`)));
} catch (e) {
  ok('no exception', false, String(e && e.stack || e));
} finally {
  wins.forEach(w => { try { clearInterval(w.loomUdsOpus && w.loomUdsOpus.tickH); w.close(); } catch (e) { /* ignore */ } });
}
const pass = results.filter(r => r.pass).length;
console.log(`\n${pass}/${results.length} PASS`);
fs.writeFileSync(path.join(__dirname, 'config-check-result.json'), JSON.stringify({ when: new Date().toISOString(), pass, total: results.length, results }, null, 1));
process.exit(pass === results.length ? 0 : 1);
