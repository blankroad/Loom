// Source-local Raw UDS PDU checks (jsdom + fake clock; NOT browser acceptance).
// Run from repo root:  node work/reports/uds-read-did/app/selfcheck/raw-check.cjs
const fs = require('fs'), path = require('path');
const { boot, fakeClock } = require('./harness.cjs');
const results = [], wins = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined && !cond ? '  ' + JSON.stringify(info) : '')); };
const start = storage => { const b = boot(storage); wins.push(b.w); return b; };
const PK = 'loom.udsReadDid.v1';
const hexs = a => Array.from(a, x => x.toString(16).toUpperCase().padStart(2, '0')).join(' ');
function ui(b) {
  const { w, d, o } = b;
  const click = s => { const el = typeof s === 'string' ? d.querySelector(s) : s; if (!el) throw new Error('missing ' + s); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); };
  const raw = v => { const e = d.getElementById('rawInput'); e.value = v; e.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const did = v => { const e = d.getElementById('didInput'); e.value = v; e.dispatchEvent(new w.Event('input', { bubbles: true })); };
  const submit = () => d.getElementById('req').dispatchEvent(new w.Event('submit', { bubbles: true, cancelable: true }));
  const key = (s, k, x) => (typeof s === 'string' ? d.querySelector(s) : s).dispatchEvent(new w.KeyboardEvent('keydown', Object.assign({ key: k, bubbles: true, cancelable: true }, x || {})));
  const mode = m => click(`#segMode [data-mode="${m}"]`);
  const route = tr => click(`#segRoute [data-tr="${tr}"]`);
  const sim = p => { Object.assign(o.st.sim, p); o.savePrefs(); };
  return { click, raw, did, submit, key, mode, route, sim, flush: () => o.flush(), txt: id => d.getElementById(id).textContent, $: id => d.getElementById(id) };
}

try {
  const U = start().U, P = U.parseRawHex;
  /* ---- A. parser: one grammar, all-or-nothing ---- */
  const eq = ['22 F1 90', '22F190', '0x22 0xF1 0x90', '0X22\t0xf1\n0x90', '22 F190', '  22\n\nf1   90  ', '22f1\t90'];
  ok('A1 spaced/compact/0x/tab/newline/mixed all -> exactly 22 F1 90 (3 B)', eq.every(s => P(s).ok && P(s).norm === '22 F1 90' && P(s).len === 3), eq.filter(s => !(P(s).ok && P(s).norm === '22 F1 90')));
  const bad = ['', '   \n\t', '2', '22F19', '22 F1 9', 'G0', '22,F1,90', '22-F1', '0x', '0x2', '0x220', '0xF190', '0x2G', '22 0x', 'F1 90 zz', '２２', '22;'];
  ok('A2 empty/odd/non-hex/comma/bare-or-long 0x rejected with error, no bytes', bad.every(s => { const r = P(s); return !r.ok && r.error && !('bytes' in r); }), bad.filter(s => P(s).ok));
  ok('A3 errors name the offending token', /2번째 토큰 "F19"/.test(P('22 F19').error) && /0x 표기는 바이트당 정확히 2자리/.test(P('0x220').error) && /공백·탭·줄바꿈/.test(P('22,F1').error));
  ok('A4 arbitrary payload kept: zeros/FF/one-byte/non-22 SID', P('00').norm === '00' && P('FF').norm === 'FF' && P('3E').len === 1 && P('31 01 FF 00 00').norm === '31 01 FF 00 00' && P('00 00 00').len === 3);
  const big = Array.from({ length: U.RAW_MAX_BYTES }, (_, i) => (i * 7) & 0xff);
  ok('A5 4095 B accepted exactly; 4096 B rejected (cap, not truncation)', P(hexs(big)).ok && P(hexs(big)).len === 4095 && hexs(P(hexs(big)).bytes) === hexs(big) && !P(hexs(big) + ' 00').ok && /4095 B 초과/.test(P(hexs(big) + ' 00').error));
  ok('A6 text length > RAW_TEXT_MAX rejected explicitly', !P(' '.repeat(U.RAW_TEXT_MAX) + '22').ok && /잘라내지 않음/.test(P(' '.repeat(U.RAW_TEXT_MAX) + '22').error));
  ok('A7 rawDidOf only for exact 22 hi lo', U.rawDidOf([0x22, 0xF1, 0x90]) === 0xF190 && U.rawDidOf([0x22, 0xF1]) === null && U.rawDidOf([0x22, 0xF1, 0x90, 0xF1, 0x8C]) === null && U.rawDidOf([0x2E, 0xF1, 0x90]) === null);

  /* ---- B. engine: exact bytes, SIM honesty ---- */
  const c = fakeClock(), E = U.createEngine({ clock: c });
  const S = (txt, sim) => { const r = E.sendRaw({ ecu: 'BCM', transport: 'doip', rawText: txt, sim: Object.assign({ scenario: 'positive', latency: 80 }, sim) }); return r; };
  const inv = S('22 F1 9');
  ok('B1 offline / invalid raw -> refused, zero TX', !S('22F190').ok && S('22F190').code === 'offline' && !inv.ok && E.txns.length === 0);
  E.connect('BCM', 'doip');
  ok('B2 invalid raw while online -> code invalid, zero TX, no timers', !S('').ok && S('').code === 'invalid' && !S('ZZ').ok && E.txns.length === 0 && c.pending() === 0);
  let r = S('22F190'); c.advance(80);
  const t1 = r.txn;
  ok('B3 raw 22F190: kind raw, SID 0x22, exact request, positive VIN applied (provenance kind raw)', t1.kind === 'raw' && t1.sid === 0x22 && hexs(t1.request) === '22 F1 90' && t1.status === 'positive' && t1.applied === true && E.confirmed.get('BCM|' + 0xF190).kind === 'raw' && E.confirmed.get('BCM|' + 0xF190).value === 'LOOMSIM0000000001');
  const conf0 = () => JSON.stringify([...E.confirmed.values()].map(v => [v.key, v.no]));
  const before = conf0();
  const cases = [['31 01 FF 00', '7F 31 11'], ['3E 80', null], ['3E', '7F 3E 13'], ['00', '7F 00 11'], ['FF', '7F FF 11'], ['62 F1 90', '7F 62 11'], ['2E F1 90 41 42', '7F 2E 11'], ['22', '7F 22 13'], ['22 F1', '7F 22 13'], ['22 F1 90 00', '7F 22 13'], ['22 F1 90 F1 8C', '7F 22 13'], ['22 12 34', '7F 22 31']];
  const got = cases.map(([q]) => { const x = S(q); c.advance(80); return x.txn; });
  ok('B4 request bytes are exactly the input (no 22 prepended, no padding)', got.every((t, i) => hexs(t.request) === hexs(P(cases[i][0]).bytes)), got.map(t => hexs(t.request)));
  // Batch 3 (intentional, services item 9): SIM ECU now implements TesterPresent 0x3E. '3E 80' (SPRMIB) -> status suppressed, no
  // response bytes; '3E' (1 B) -> 7F 3E 13 incorrectMessageLength. Every other unmodelled SID keeps 7F SID 11.
  ok('B5 mock replies: unmodelled SID -> 7F SID 11 (SID echoed); 3E 80 suppressed; 3E -> 7F 3E 13; malformed/multi-DID 22 -> 7F 22 13; unknown DID -> 7F 22 31', got.every((t, i) => (cases[i][1] === null ? t.status === 'suppressed' && t.final === null : t.status === 'nrc' && hexs(t.final) === cases[i][1])), got.map(t => t.final ? hexs(t.final) : t.status));
  ok('B6 none of the non-DID / malformed raw requests touch confirmed values', conf0() === before && got.every(t => t.applied === false));
  /* ---- C. NRC / pending echo the actual SID ---- */
  const tn = S('31 01 02 03', { scenario: 'nrc', nrc: 0x22 }).txn; c.advance(80);
  const tp = S('2E F1 90 41', { scenario: 'pending', pending: 2, latency: 80 }).txn; c.advance(80 + 2 * 1200);
  ok('C1 NRC scenario echoes SID: 7F 31 22', hexs(tn.final) === '7F 31 22' && tn.nrc === 0x22);
  ok('C2 pending scenario: 7F 2E 78 ×2 then mock 7F 2E 11; P2* armed; no DID apply', tp.events.filter(e => e.kind === 'pending').every(e => hexs(e.bytes) === '7F 2E 78') && tp.pendingCount === 2 && hexs(tp.final) === '7F 2E 11' && conf0() === before);
  const tdid = E.read({ ecu: 'BCM', transport: 'doip', didText: 'F18C', sim: { scenario: 'pending', pending: 1, latency: 80 } }).txn; c.advance(80 + 1200);
  ok('C3 DID path unchanged: kind did, 7F 22 78 then 62 F1 8C, applied', tdid.kind === 'did' && hexs(tdid.events[1].bytes) === '7F 22 78' && tdid.final[0] === 0x62 && tdid.applied === true);
  const tl = S('22 F1 90', { scenario: 'timeout', late: true }).txn; c.advance(1000 + 1200);
  ok('C4 raw timeout + late 62: status timeout, late ignored, confirmed still #1', tl.status === 'timeout' && tl.events.some(e => e.kind === 'late' && e.ignored) && E.confirmed.get('BCM|' + 0xF190).no === t1.no);
  const decG = U.decodePdu([0x71, 0x01, 0xFF, 0x00], { sid: 0x31, kind: 'raw', did: null, dir: 'rx' });
  ok('C5 generic positive (SID+0x40, non-22) decodes as generic, never DID echo', decG.kind === 'positive' && decG.generic && decG.echoOk === false && decG.fields[0].name === 'SID-PR');
  ok('C6 decode: 7F with mismatched SIDRQ flagged; raw request fields; one-byte safe', U.decodePdu([0x7F, 0x22, 0x11], { sid: 0x31, kind: 'raw', dir: 'rx' }).fields[1].bad === true &&
    U.decodePdu([0x3E, 0x80], { sid: 0x3E, kind: 'raw', dir: 'tx' }).fields.map(f => f.name).join() === 'SID,subFunction' && /SPRMIB=1/.test(U.decodePdu([0x3E, 0x80], { sid: 0x3E, kind: 'raw', dir: 'tx' }).fields[1].note) &&
    U.decodePdu([0x3E], { sid: 0x3E, kind: 'raw', dir: 'tx' }).fields.length === 1 && U.decodePdu([0x22, 0xF1], { sid: 0x22, kind: 'raw', dir: 'tx' }).fields[1].bad === true);
  ok('C7 legacy decodePdu(bytes, did) signature unchanged for DID path', U.decodePdu([0x22, 0xF1, 0x90], 0xF190).kind === 'request' && U.decodePdu(t1.final, 0xF190).echoOk === true);

  /* ---- D. snapshot / context isolation / cancel ---- */
  const E2 = U.createEngine({ clock: c });
  const sv = E2.saveProfile('BCM', 'doip', { targetId: 'preset:BCM|doip', name: 'RawQA', values: Object.assign({}, U.presetValues('BCM', 'doip'), { host: '192.0.2.77', p2: 1500, p2star: 6000 }) });
  E2.connect('BCM', 'doip');
  const tr = E2.sendRaw({ ecu: 'BCM', transport: 'doip', rawText: '31 01 AA BB', sim: { scenario: 'pending', pending: 3, latency: 80 } }).txn;
  ok('D1 raw txn snapshots custom profile/route/timing', sv.ok && tr.profile.name === 'RawQA' && tr.route.host === '192.0.2.77:13400' && tr.timing.p2 === 1500);
  const busy = E2.read({ ecu: 'BCM', transport: 'doip', didText: 'F190', sim: {} }), busy2 = E2.sendRaw({ ecu: 'BCM', transport: 'doip', rawText: '22F190', sim: {} });
  ok('D2 one active per context across modes (DID and raw both busy)', !busy.ok && busy.code === 'busy' && !busy2.ok && busy2.code === 'busy' && E2.txns.length === 1);
  ok('D3 online config guard still holds for raw context', E2.saveProfile('BCM', 'doip', { targetId: sv.profile.id, name: 'RawQA', values: U.presetValues('BCM', 'doip') }).code === 'online');
  E2.connect('BCM', 'cantp');
  const tc = E2.sendRaw({ ecu: 'BCM', transport: 'cantp', rawText: '22 01 01', sim: { scenario: 'positive', latency: 80 } }).txn; c.advance(80);
  ok('D4 other route independent: CAN-TP raw 22 01 01 positive while DoIP raw pending', tc.status === 'positive' && tc.route.label === 'CAN-TP' && tr.status === 'pending');
  E2.cancel('BCM', 'doip'); c.advance(10000);
  ok('D5 cancel raw: later 7F 31 78/11 recorded as ignored late, status stays cancelled', tr.status === 'cancelled' && tr.events.filter(e => e.kind === 'late').every(e => e.ignored && e.bytes[1] === 0x31) && tr.events.some(e => e.kind === 'late'));

  /* ---- E. UI ---- */
  const b = start(), { w, d, o } = b, u = ui(b);
  u.click('#btnLink'); u.flush();
  ok('E1 default mode DID; mode radios labelled; Read label 읽기', d.getElementById('req').classList.contains('m-did') && u.$('segMode').getAttribute('role') === 'radiogroup' && d.querySelectorAll('#segMode [role=radio]').length === 2 && u.txt('btnReadLabel') === '읽기');
  u.mode('raw'); u.flush();
  ok('E2 Raw mode: textarea labelled, default 22 F1 90 preview 3 B, button 전송, hint mentions grammar+limit+Ctrl/⌘+Enter', d.getElementById('req').classList.contains('m-raw') && d.querySelector('label[for="rawInput"]') && /22 F1 90/.test(u.txt('rawPdu')) && /3 B/.test(u.txt('rawSum')) && u.txt('btnReadLabel') === '전송' && /4095 B/.test(u.txt('rawHint')) && /Ctrl\/⌘\+Enter/.test(u.txt('rawHint')) && /0x/.test(u.txt('rawHint')));
  u.raw(''); u.submit(); u.flush();
  ok('E3 empty raw submit -> zero TX, error, aria-invalid, focus textarea', o.E.txns.length === 0 && u.$('rawInput').getAttribute('aria-invalid') === 'true' && /전송하지 않음/.test(u.txt('didMsg')) && d.activeElement === u.$('rawInput'));
  ['ZZ', '22 F1 9', '0x220', '22,F1'].forEach(v => { u.raw(v); u.submit(); });
  u.raw('22 F1 9'); u.flush();
  ok('E4 invalid raw (non-hex/odd/0x/comma) never sends; preview shows no partial bytes', o.E.txns.length === 0 && !/22 F1/.test(u.txt('rawPdu')) && /부분 해석·전송 없음/.test(u.txt('rawPdu')));
  u.raw('22F190'); u.key('#rawInput', 'Enter'); u.flush();
  ok('E5 plain Enter in textarea does not send', o.E.txns.length === 0);
  u.key('#rawInput', 'Enter', { ctrlKey: true }); u.flush();
  const x1 = o.E.txns[0];
  ok('E6 Ctrl+Enter sends exactly 22 F1 90 as raw', o.E.txns.length === 1 && x1.kind === 'raw' && hexs(x1.request) === '22 F1 90');
  b.clock.advance(400); u.flush();
  ok('E7 raw exact-shape row: RAW tag + F190 VIN; detail 요청 row raw; confirmed RAW tag', !!d.querySelector(`#tx-${x1.id}.k-raw .c-did .rawtag`) && /F190 VIN/.test(d.querySelector(`#tx-${x1.id} .c-did`).textContent) && /Raw UDS PDU · SID 0x22/.test(u.txt('fields')) && !!d.querySelector('#valsBody .cv-p .rawtag'));
  u.raw('31 01 FF 00'); u.key('#rawInput', 'Enter', { metaKey: true }); b.clock.advance(400); u.flush();
  const x2 = o.E.txns[1];
  ok('E8 ⌘+Enter non-22 raw: row RAW 0x31 RoutineControl, no VIN/DID label, NRC 7F 31 11', x2 && hexs(x2.final) === '7F 31 11' && /RAW\s*0x31 RoutineControl/.test(d.querySelector(`#tx-${x2.id} .c-did`).textContent) && !/VIN|F190/.test(d.querySelector(`#tx-${x2.id} .c-did`).textContent));
  ok('E9 non-22 detail: SIDRQ-NR matches 0x31, serviceNotSupported, applied box says unrelated to DID values', /RoutineControl/.test(u.txt('fields')) && /serviceNotSupported/.test(u.txt('fields')) && /DID 확정 값과 무관/.test(u.txt('fields')) && !d.querySelector('#fields .applied.yes') && !/LOOMSIM/.test(u.txt('fields')));
  u.click(d.querySelector('#seq li[data-i="0"]')); u.flush();
  ok('E10 raw Tx event decodes SID/subFunction/parameters with offsets', /REQUEST_PDU/.test(u.txt('fields')) && /subFunction/.test(u.txt('fields')) && /parameters/.test(u.txt('fields')) && d.querySelectorAll('#hex .hx-h b').length === 4);
  // switching mode / editing while pending never relabels old TX
  u.raw('2E F1 90 41'); u.sim({ scenario: 'pending', pending: 2, latency: 80 }); u.submit(); u.flush();
  const x3 = o.E.txns[2];
  const rowBefore = d.querySelector(`#tx-${x3.id}`).innerHTML.replace(/data-el="\d+">\d+ ms/, '');
  u.raw('10 03'); u.mode('did'); u.did('F18C'); u.route('cantp'); u.flush();
  ok('E11 edit raw + switch mode/route while #3 pending: txn and row unchanged', x3.kind === 'raw' && hexs(x3.request) === '2E F1 90 41' && x3.route.label === 'DoIP' && d.querySelector(`#tx-${x3.id}`).innerHTML.replace(/data-el="\d+">\d+ ms/, '') === rowBefore);
  u.route('doip'); u.mode('raw'); u.flush();
  ok('E12 per-mode inputs kept across switches (raw 10 03, DID F18C)', u.$('rawInput').value === '10 03' && u.$('didInput').value === 'F18C' && o.st.rawText === '10 03');
  ok('E13 busy context: 전송 disabled, message names #3', u.$('btnRead').disabled && /#3/.test(u.txt('didMsg')));
  b.clock.advance(80 + 2 * 1200); u.flush();
  ok('E14 pending raw finishes with echoed SID 7F 2E 11; VIN confirmed still from #1', hexs(x3.final) === '7F 2E 11' && o.E.confirmed.get('BCM|' + 0xF190).no === 1);
  u.click('#didList li[data-did="0101"]'); u.flush();
  ok('E15 DID list pick switches to DID mode, raw text kept, no TX', o.st.mode === 'did' && u.$('didInput').value === '0101' && o.st.rawText === '10 03' && o.E.txns.length === 3);
  u.key('#segMode [data-mode="did"]', 'ArrowRight'); u.flush();
  ok('E16 mode radiogroup ←→ toggles and moves focus', o.st.mode === 'raw' && d.activeElement === d.querySelector('#segMode [data-mode="raw"]'));
  // one-byte + long raw render safely
  u.sim({ scenario: 'positive', latency: 80 });
  u.raw('3E'); u.submit(); b.clock.advance(80); u.flush();
  const x4 = o.E.txns[3];
  // Batch 3 (intentional): 0x3E is modelled; a 1-byte 3E is a length error (0x13), previously serviceNotSupported (0x11).
  ok('E17 one-byte raw 3E: sent, 7F 3E 13 (incorrectMessageLength), detail renders', hexs(x4.request) === '3E' && hexs(x4.final) === '7F 3E 13' && /TesterPresent/.test(u.txt('fields')));
  u.raw(hexs(big)); u.flush();
  ok('E18 4095 B raw preview: exact normalized text + 4095 B count', u.txt('rawPdu').includes(hexs(big)) && /4095 B/.test(u.txt('rawSum')));
  u.submit(); b.clock.advance(80); u.flush();
  const x5 = o.E.txns[4];
  ok('E19 4095 B raw sent exactly; hex pane 256 rows; row/fields truncated text with length', x5.request.length === 4095 && hexs(x5.request) === hexs(big) && d.querySelectorAll('#hex .hx-row').length === 1 && (u.click(d.querySelector('#seq li[data-i="0"]')), u.flush(), d.querySelectorAll('#hex .hx-row').length === 256) && /…/.test(d.querySelector(`#tx-${x5.id} .c-req`).textContent) && /4095 B/.test(d.querySelector(`#tx-${x5.id} .c-req`).title) && /바이트 패널 참조/.test(u.txt('fields')));
  // export
  const pj = JSON.parse(w.LoomUdsTools.exportJson()), csv = w.LoomUdsTools.exportCsv();
  const j2 = pj.transactions.find(t => t.no === x2.no), j1 = pj.transactions.find(t => t.no === x1.no), jd = pj.transactions.find(t => t.no === x5.no);
  ok('E20 export JSON: kind/sid/sidName/requestLen; non-22 raw has empty did/didName; exact request', j2.kind === 'raw' && j2.sid === '0x31' && j2.sidName === 'RoutineControl' && j2.requestLen === 4 && j2.did === '' && j2.didName === '' && j2.request === '31 01 FF 00' && j1.did === 'F190' && jd.requestLen === 4095);
  const hdr = csv.split('\n')[0].split(',');
  ok('E21 export CSV has kind,sid,requestLen columns; raw row values', ['kind', 'sid', 'requestLen'].every(k => hdr.includes(k)) && csv.split('\n').some(l => l.startsWith(x2.no + ',') && l.includes(',raw,0x31,4,')));
  // persistence
  u.raw('19 02 FF'); u.flush();
  const sp = JSON.parse(w.localStorage.getItem(PK));
  ok('E22 prefs: reqMode raw + rawText persisted, schema 2, no history/link', sp.schema === 2 && sp.reqMode === 'raw' && sp.rawText === '19 02 FF' && !('txns' in sp) && !JSON.stringify(sp).includes('7F 31'));
  const store = {}; Object.keys(w.localStorage).forEach(k => { store[k] = w.localStorage.getItem(k); });
  const b2 = start(store), u2 = ui(b2); b2.clock.advance(10000); u2.flush();
  ok('E23 reload: raw mode + text restored, no link, no TX, no timers', b2.o.st.mode === 'raw' && b2.d.getElementById('rawInput').value === '19 02 FF' && b2.o.E.txns.length === 0 && b2.o.curCtx().link === 'offline' && b2.clock.pending() === 0);
  u.raw('22 '.repeat(9000)); u.flush();
  const sp2 = JSON.parse(w.localStorage.getItem(PK));
  ok('E24 oversize raw text: rejected, never truncated in editor, not persisted (key omitted)', o.st.rawText.length === 27000 && u.$('rawInput').value.length === 27000 && !('rawText' in sp2) && /잘라내지 않음/.test(u.txt('didMsg')));
  /* ---- F. old schema-2 prefs migrate with all profiles exactly ---- */
  const bc = start(); const Ec = bc.o.E;
  Ec.saveProfile('BCM', 'doip', { targetId: 'preset:BCM|doip', name: 'Keep A', values: Object.assign({}, bc.U.presetValues('BCM', 'doip'), { host: '10.9.8.7' }) });
  Ec.saveProfile('GW', 'cantp', { targetId: 'preset:GW|cantp', name: 'Keep B', values: Object.assign({}, bc.U.presetValues('GW', 'cantp'), { txId: 0x7E1 }) });
  const oldCfg = Ec.exportConfig();
  const oldPrefs = { schema: 2, ecu: 'GW', transport: 'cantp', didText: 'F18C', sim: { scenario: 'positive', latency: 400, nrc: 34, pending: 2, late: false }, filter: 'all', histH: null, didCollapsed: false, valsCollapsed: false, routeInfoOpen: false, simOpen: false, config: oldCfg };
  const b3 = start({ [PK]: JSON.stringify(oldPrefs) }); b3.o.flush();
  ok('F1 config-era schema-2 prefs (no reqMode/rawText): DID mode, default raw text, profiles+active identical', b3.o.st.mode === 'did' && b3.o.st.rawText === '22 F1 90' && JSON.stringify(b3.o.E.exportConfig()) === JSON.stringify(oldCfg) && b3.o.E.activeProfile('GW', 'cantp').name === 'Keep B' && !b3.o.st.cfgNotice);
  b3.o.setMode('raw');
  const sp3 = JSON.parse(b3.w.localStorage.getItem(PK));
  ok('F2 after mode change save: config block unchanged byte-for-byte, other prefs kept', JSON.stringify(sp3.config) === JSON.stringify(oldCfg) && sp3.didText === 'F18C' && sp3.ecu === 'GW' && sp3.reqMode === 'raw');
  const b4 = start({ [PK]: JSON.stringify(Object.assign({}, oldPrefs, { reqMode: 'bogus', rawText: 42 })) });
  ok('F3 bad reqMode/rawText types fall back to DID / default, profiles still loaded', b4.o.st.mode === 'did' && b4.o.st.rawText === '22 F1 90' && b4.o.E.customCount() === 2);
  /* ---- G. static ---- */
  const css = fs.readFileSync(path.resolve(__dirname, '../styles.css'), 'utf8');
  ok('G1 CSS: raw preview wraps (overflow-wrap anywhere) + scroll bound; narrow raw grid single column', /\.raw-prev \{[^}]*overflow: auto;[^}]*overflow-wrap: anywhere/.test(css) && /@media \(max-width: 760px\) \{\s*\.raw-row \{ grid-template-columns: minmax\(0, 1fr\); \}/.test(css));
  ok('G1b history header relabelled DID · SID (raw rows may show SID)', d.querySelector('.tx-head .c-did').textContent === 'DID · SID');
  ok('G2 no transport framing invented: hex caption still says UDS PDU only', /UDS PDU \(CAN-TP\/DoIP\/EthDiag 전송 프레임 아님\)/.test(u.txt('hex')));
} catch (e) {
  ok('no exception', false, String(e && e.stack || e));
} finally {
  wins.forEach(w => { try { clearInterval(w.loomUdsOpus && w.loomUdsOpus.tickH); w.close(); } catch (e) { /* ignore */ } });
}
const pass = results.filter(r => r.pass).length;
console.log(`\n${pass}/${results.length} PASS`);
fs.writeFileSync(path.join(__dirname, 'raw-check-result.json'), JSON.stringify({ when: new Date().toISOString(), pass, total: results.length, results }, null, 1));
process.exit(pass === results.length ? 0 : 1);
