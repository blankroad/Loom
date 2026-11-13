// Source-local focused checks (jsdom; NOT browser acceptance).
// Run from repo root:  node work/reports/ethernet-workbench/opus/selfcheck/check.cjs
const path = require('path'), fs = require('fs');
const DIR = path.resolve(__dirname, '..');
const { JSDOM } = require(path.resolve(DIR, '../../../../ui/node_modules/jsdom'));
const html = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8').replace(/<script src="[^"]+"><\/script>/g, '');
const SRC = ['pkt.js', 'app.js', 'sock.js', 'tools.js'].map(f => fs.readFileSync(path.join(DIR, f), 'utf8'));
const results = [];
const ok = (name, cond, info) => { results.push({ name, pass: !!cond, info }); console.log((cond ? 'PASS ' : 'FAIL ') + name + (info !== undefined ? '  ' + JSON.stringify(info) : '')); };
function boot(storage, opts) {
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:4180/ethernet-workbench/opus/index.html', runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window; w.__loomEthManualClock = true; if (opts && opts.retain) w.__loomEthRetain = opts.retain;
  if (storage) for (const [k, v] of Object.entries(storage)) w.localStorage.setItem(k, v);
  w.localStorage.setItem('unrelated.key', 'keep');
  w.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  w.HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  w.HTMLDialogElement.prototype.close = function () { this.open = false; };
  w.Element.prototype.scrollIntoView = function () {};
  w.URL.createObjectURL = () => 'blob:x'; w.URL.revokeObjectURL = () => {};
  w.HTMLAnchorElement.prototype.click = function () { w.__dl = (w.__dl || 0) + 1; };
  SRC.forEach(s => w.eval(s));
  return w;
}
const dump = w => { const o = {}; for (let i = 0; i < w.localStorage.length; i++) { const k = w.localStorage.key(i); o[k] = w.localStorage.getItem(k); } return o; };
const click = (w, sel) => { const el = typeof sel === 'string' ? w.document.querySelector(sel) : sel; if (!el) throw new Error('missing ' + sel); el.dispatchEvent(new w.MouseEvent('click', { bubbles: true })); return el; };
const key = (w, el, k, extra) => el.dispatchEvent(new w.KeyboardEvent('keydown', Object.assign({ key: k, bubbles: true }, extra || {})));

let w = boot(); let L = w.LoomEth, S = L.S, P = w.LoomPkt, o = w.loomEthOpus, d = w.document;
o.tick(160); // 8 s

/* 1. multi-channel initial */
let s = o.snapshot();
ok('two SIM channels open & live initially', s.channels.length === 2 && s.channels.every(c => c.live) && s.channels.every(c => c.captured > 0), s.channels.map(c => [c.alias, c.captured]));
ok('channel cards only for open channels + SIM badge', d.querySelectorAll('.chcard').length === 2 && d.querySelectorAll('.chcard .st.sim').length === 2);
const protos = new Set(S.view.map(p => p.sum.proto));
ok('fixture covers ARP/UDP/TCP/DoIP/SOME/IP/SOME/IP-SD', ['ARP', 'TCP', 'DoIP', 'SOME/IP', 'SOME/IP-SD'].every(x => protos.has(x)), [...protos]);

/* 2. coherence: list length == bytes, tree ranges point at matching bytes */
let coh = true, bad = null;
S.view.forEach(p => {
  const dd = P.decode(p.bytes, { no: p.no, tcpIsn: S.isn, stream: L.streamIdx });
  if (dd.sum.info !== p.sum.info) { coh = false; bad = ['info', p.no]; }
  if (JSON.stringify(dd.tree).includes('incorrect')) { coh = false; bad = ['cksum', p.no]; }
  if (p.f.ip) { const src = p.f['ip.src'][0].split('.').map(Number); if (src.join() !== Array.from(p.bytes.slice(26, 30)).join()) { coh = false; bad = ['ipsrc', p.no]; } }
  if (p.f['frame.len'][0] !== p.bytes.length) { coh = false; bad = ['len', p.no]; }
});
ok('every frame: info re-decodes, checksums correct, ip.src bytes, length coherent', coh, bad);

/* 3. selection + keyboard + field<->byte */
const doipP = S.view.find(p => p.sum.proto === 'DoIP' && p.sum.info.includes('Diagnostic message'));
L.selectPacket(doipP, { scroll: true });
ok('select -> list row .sel + details tree', d.querySelector('.prow.sel') && d.querySelector('#tree').textContent.includes(`Frame ${doipP.no}:`) && d.querySelector('#tree').textContent.includes('DoIP (ISO13400)'));
ok('bytes pane has exactly len hex cells', d.querySelectorAll('#hex .hx-h b').length === doipP.bytes.length, d.querySelectorAll('#hex .hx-h b').length);
const ipSrcNode = [...d.querySelectorAll('#tree .tn')].find(t => t.textContent.startsWith('Source Address:'));
click(w, ipSrcNode);
const on = [...d.querySelectorAll('#hex .hx-h b.on')].map(b => +b.dataset.o);
ok('field->bytes: IPv4 Source Address highlights offsets 26..29', JSON.stringify(on) === '[26,27,28,29]', on);
click(w, d.querySelector('#hex .hx-h b[data-o="13"]'));
ok('byte->field: offset 13 selects Ethernet Type node', (d.querySelector('#tree .tn.on') || {}).textContent.startsWith('Type: IPv4'), (d.querySelector('#tree .tn.on') || {}).textContent);
const i0 = S.view.indexOf(S.sel);
key(w, d.querySelector('#plBody'), 'ArrowDown');
ok('keyboard ArrowDown selects next row', S.view.indexOf(S.sel) === i0 + 1);
key(w, d, 'F7');
ok('F7 previous packet (global)', S.view.indexOf(S.sel) === i0);

/* 4. display filter valid / invalid / count / scope */
const inp = d.querySelector('#dfInput');
inp.value = 'doip'; click(w, '#dfApply');
s = o.snapshot();
ok('display filter doip -> only DoIP rows, shown<captured', S.view.every(p => p.f.doip) && s.shown < s.capturedInScope && s.shown > 0, [s.shown, s.capturedInScope]);
ok('count text shows displayed/captured', d.querySelector('#dfCount').textContent.includes(`${s.shown}`) && d.querySelector('#dfCount').textContent.includes(`${s.capturedInScope}`));
const before = S.view.length;
inp.value = 'doip.type =='; inp.dispatchEvent(new w.Event('input', { bubbles: true }));
ok('live validation marks invalid', inp.classList.contains('invalid'));
click(w, '#dfApply');
ok('invalid apply keeps prior filter + results', o.snapshot().df === 'doip' && S.view.length === before && inp.getAttribute('aria-invalid') === 'true' && d.querySelector('#dfMsg').classList.contains('err'));
inp.value = 'ip.addr == 10.0.3.0/24 && udp.port == 30490'; click(w, '#dfApply');
ok('cidr + && filter -> SOME/IP-SD rows only', S.view.length > 0 && S.view.every(p => p.sum.proto === 'SOME/IP-SD'), S.view.length);
inp.value = 'loom.ch == "ecu-doip"'; click(w, '#dfApply');
ok('loom.ch filter -> only ecu-doip channel', S.view.length > 0 && S.view.every(p => p.ch.alias === 'ecu-doip'));
click(w, '#dfClear');
const ch1 = S.chans[1];
click(w, `[data-scope="${ch1.id}"]`);
ok('scope=channel -> only that channel; captured count per scope', S.view.every(p => p.ch === ch1) && o.snapshot().capturedInScope === ch1.pkts.length);
click(w, '[data-scope="all"]');

/* 5. O2: selection excluded by filter */
const sd = S.view.find(p => p.sum.proto === 'SOME/IP-SD'); L.selectPacket(sd);
L.applyDF('doip');
ok('O2 filter excludes selection -> first match selected, details coherent', S.sel && S.sel.f.doip && S.sel === S.view[0] && d.querySelector('#tree').textContent.includes(`Frame ${S.sel.no}:`));
L.applyDF('udp && tcp');
ok('O2 empty filter result -> selection & panes cleared', S.sel === null && !!d.querySelector('#tree .empty') && !!d.querySelector('#hex .empty'));
L.applyDF('');

/* 6. independent capture stop/start */
const [A, B] = S.chans;
click(w, `.chcard[data-ch="${A.id}"] [data-act="ch-stop"]`);
const aCount = A.captured, bCount = B.captured, aRet = A.pkts.length;
o.tick(60);
ok('stop A: A frozen + history retained, B keeps capturing', A.captured === aCount && A.pkts.length === aRet && B.captured > bCount && !A.live && B.live, [A.captured, B.captured - bCount]);
ok('stopped card shows STOPPED + start button', d.querySelector(`.chcard[data-ch="${A.id}"] [data-role=cap-state]`).textContent === 'STOPPED' && !!d.querySelector(`.chcard[data-ch="${A.id}"] [data-act=ch-start]`));
click(w, `.chcard[data-ch="${A.id}"] [data-act="ch-start"]`); o.tick(40);
ok('restart A resumes capture', A.captured > aCount && A.live);

/* 7. pause view vs capture */
click(w, '#tbPause');
const shownP = S.view.length, capP = B.captured;
o.tick(40);
ok('pause view: list frozen while capture continues', S.view.length === shownP && B.captured > capP && !!d.querySelector('#plNotice.paused'));
ok('paused notice counts pending', +d.querySelector('[data-role=pending]').textContent.replace(/,/g, '') > 0);
const resumeBtn = d.querySelector('#plNotice [data-cmd=pause-view]'); o.tick(10);
ok('paused banner resume button node stable across ticks', d.querySelector('#plNotice [data-cmd=pause-view]') === resumeBtn);
click(w, resumeBtn);
ok('resume catches up', S.view.length > shownP);
ok('O4 resumed: paused notice hidden (no stale paused text) + CSS [hidden] rule', (d.querySelector('#plNotice').hidden || !/일시정지/.test(d.querySelector('#plNotice').textContent)) && /\.pl-notice\[hidden\][^{]*\{\s*display:\s*none/.test(fs.readFileSync(path.join(DIR, 'styles.css'), 'utf8')));

/* 8. add channel + capture filter + max */
click(w, '[data-cmd="add-channel"]');
const f = d.querySelector('#dlgChannel form');
f.elements.alias.value = 'lab-gen'; f.elements.cf.value = 'udp port 99999'; f.dispatchEvent(new w.Event('submit', { cancelable: true }));
ok('invalid capture filter blocks add', S.chans.length === 2 && d.querySelector('[data-err=cf]').textContent.includes('99999'));
f.elements.cf.value = 'not arp'; f.dispatchEvent(new w.Event('submit', { cancelable: true }));
const C = S.chans[2]; o.tick(60);
ok('third channel added live with capture filter applied at capture', C && C.live && C.pkts.length > 0 && C.pkts.every(p => !p.f.arp) && C.cfExcluded >= 1, C && [C.pkts.length, C.cfExcluded]);
ok('scope seg lists 3 channels + all', d.querySelectorAll('#scopeSeg [data-scope]').length === 4);

/* 9. retention bound + drop notice */
let w2 = boot(null, { retain: 50 }); let o2 = w2.loomEthOpus, S2 = w2.LoomEth.S;
o2.tick(400);
const ch2 = S2.chans[1];
ok('retention bounded per channel, drop counted + notice', ch2.pkts.length === 50 && ch2.dropped === ch2.captured - 50 && !w2.document.querySelector('#plNotice').hidden && w2.document.querySelector('[data-role=dropped]'), [ch2.pkts.length, ch2.dropped]);
ok('view never holds evicted packets', S2.view.every(p => !p.gone) && S2.view.length <= 100);

/* 10. socket test: disconnected blocks, connect, send, template, per-channel independence */
click(w, '#tbSocket');
ok('socket drawer opens', !d.querySelector('#sock').hidden && d.querySelector('[data-role=sk-state]').textContent === '닫힘');
d.querySelector('#skCh').value = A.id; d.querySelector('#skCh').dispatchEvent(new w.Event('change', { bubbles: true }));
ok('send disabled while closed', d.querySelector('[data-act=sk-send]').disabled);
click(w, '[data-tpl="t1"] [data-act="tpl-run"]');
ok('template run while closed -> blocked with reason, nothing sent', d.querySelector('[data-role=sk-why]').textContent.includes('실행 차단') && !A.pkts.some(p => p.own));
click(w, '[data-act="sk-connect"]'); o.tick(1);
ok('TCP connect -> established, SYN/SYN-ACK/ACK captured on A', d.querySelector('[data-role=sk-state]').textContent === '연결됨' && A.pkts.filter(p => p.own).length === 2);
click(w, '[data-act="sk-fmt"][data-v="hex"]');
const ta = d.querySelector('#skData');
ta.value = '02 FD 00 05 zz'; ta.dispatchEvent(new w.Event('input', { bubbles: true }));
ok('hex validation error shown', d.querySelector('#skErr').textContent.includes('zz'));
click(w, '[data-act="sk-send"]');
ok('invalid hex not sent', A.pkts.filter(p => p.own && p.sum.proto === 'DoIP').length === 0);
click(w, '[data-tpl="t1"] [data-act="tpl-run"]'); o.tick(1);
click(w, '[data-tpl="t3"] [data-act="tpl-run"]'); o.tick(1);
const tx = A.pkts.filter(p => p.own && p.sum.proto === 'DoIP'), rx = A.pkts.filter(p => !p.own && /Routing activation response|62 f1 90/.test(p.sum.info) && p.ch === A && p.sum.dst === A.me.ip);
ok('template TX traceable on channel A + deterministic RX', tx.length === 2 && rx.length >= 2, [tx.length, rx.length]);
const logT = d.querySelector('#skLog').textContent;
ok('transcript has timestamped TX/RX with frame refs', /TX02 fd 00 05/.test(logT) && /RX02 fd 00 06/.test(logT) && /#\d+/.test(logT));
ok('nothing leaked to channel B', !B.pkts.some(p => p.own));
click(w, '[data-tpl="t6"] [data-act="tpl-run"]');
ok('template port mismatch blocked', d.querySelector('[data-role=sk-why]').textContent.includes('≠'));
// B independent session: still closed
d.querySelector('#skCh').value = B.id; d.querySelector('#skCh').dispatchEvent(new w.Event('change', { bubbles: true }));
ok('B session independent (closed) while A connected', d.querySelector('[data-role=sk-state]').textContent === '닫힘' && w.LoomEthSock._sess.get(A.id).state === 'est');
// UDP SOME/IP on B
click(w, '[data-act="sk-connect"]');
click(w, '[data-tpl="t5"] [data-act="tpl-run"]'); o.tick(1);
ok('UDP SOME/IP request -> Response 0x80 on B', B.pkts.some(p => !p.own && p.f.someip && p.f['someip.messagetype'][0] === 0x80));
// text + unicode
d.querySelector('#skCh').value = A.id; d.querySelector('#skCh').dispatchEvent(new w.Event('change', { bubbles: true }));
// save + edit template
click(w, '[data-act="sk-fmt"][data-v="hex"]');
d.querySelector('#skData').value = '02 FD 80 01 00 00 00 06 0E 00 10 01 3E 00'; d.querySelector('#skData').dispatchEvent(new w.Event('input', { bubbles: true }));
d.querySelector('#skTplName').value = 'my TP'; click(w, '[data-act="tpl-save"]');
const tpls = w.LoomEthSock._tpls(); const mine = tpls.find(t => t.name === 'my TP');
ok('save template persists to own prefix', mine && JSON.parse(w.localStorage.getItem('loom.eth.opus.v1.templates')).some(t => t.name === 'my TP'));
click(w, `[data-tpl="${mine.id}"] [data-act="tpl-edit"]`);
d.querySelector('#skTplName').value = 'my TP2'; click(w, '[data-act="tpl-update"]');
ok('edit/update template', w.LoomEthSock._tpls().find(t => t.id === mine.id).name === 'my TP2');
// repeat + disconnect stops it
for (const [id, v] of [['#skRepN', '50'], ['#skRepMs', '100']]) { const el = d.querySelector(id); el.value = v; el.dispatchEvent(new w.Event('input', { bubbles: true })); }
click(w, '[data-act="sk-rep"]');
const stopBtn = d.querySelector('[data-act="sk-rep-stop"]'), repIn = d.querySelector('#skRepN'), logEl = d.querySelector('#skLog');
o.tick(4);
ok('O5 repeat traffic keeps Stop/inputs/log DOM nodes stable + entered values', stopBtn && stopBtn.isConnected && d.querySelector('[data-act="sk-rep-stop"]') === stopBtn && d.querySelector('#skRepN') === repIn && repIn.value === '50' && d.querySelector('#skLog') === logEl && /반복 \d+\/50/.test(logEl.textContent));
click(w, stopBtn); const sentStop = A.pkts.filter(p => p.own && p.f.uds).length; o.tick(20);
ok('O5 Stop click on the captured node stops repeat (no further TX)', A.pkts.filter(p => p.own && p.f.uds).length === sentStop && !w.LoomEthSock._sess.get(A.id).repeat && d.querySelector('#skRepN').value === '50');
click(w, '[data-act="sk-rep"]'); o.tick(4);
const sent1 = A.pkts.filter(p => p.own && p.f.uds).length;
click(w, '[data-act="sk-disconnect"]'); o.tick(20);
const sent2 = A.pkts.filter(p => p.own && p.f.uds).length;
ok('disconnect stops repeat + blocks send', sent2 === sent1 && d.querySelector('[data-act=sk-send]').disabled && A.pkts.some(p => p.own && p.f['tcp.flags.fin'] && p.f['tcp.flags.fin'][0]), [sent1, sent2]);
// refused port
d.querySelector('#skPort').value = '1234'; d.querySelector('#skPort').dispatchEvent(new w.Event('input', { bubbles: true }));
click(w, '[data-act="sk-connect"]'); o.tick(1);
ok('connect to non-listening port -> RST, refused', d.querySelector('[data-role=sk-state]').textContent.includes('거부') && A.pkts.some(p => p.sum.cls === 'p-rst'));

/* 11. follow stream + export */
L.applyDF('tcp.port == 7000'); L.selectPacket(S.view[S.view.length - 1]);
click(w, '#tbFollow');
const fb = d.querySelector('#fsBody');
ok('follow stream shows directional ASCII (STAT? client red / STAT seq server blue)', d.querySelector('#dlgFollow').open && fb.querySelector('.c') && fb.querySelector('.s') && /STAT\?/.test(fb.querySelector('.c').textContent) && /STAT seq=/.test(fb.querySelector('.s').textContent));
click(w, '#dlgFollow [data-fs="only"]');
ok('follow "only this stream" applies tcp.stream filter', /^tcp\.stream == \d+ && loom\.ch == "body-someip"$/.test(o.snapshot().df) && S.view.length > 0 && S.view.every(p => p.f.tcp && p.f['tcp.port'].includes(7000)));
const shown = S.view.length;
const buf = w.LoomEthTools._exportPcap();
const dv = new DataView(buf.buffer); let off = 24, n = 0, okLen = true;
while (off < buf.length) { const incl = dv.getUint32(off + 8, true); if (incl !== S.view[n].bytes.length) okLen = false; off += 16 + incl; n++; }
ok('pcap: magic a1b2c3d4 LE, linktype 1, packet count == displayed rows, lengths match', dv.getUint32(0, true) === 0xa1b2c3d4 && dv.getUint32(20, true) === 1 && n === shown && okLen && off === buf.length, [n, shown]);
const csv = w.LoomEthTools._exportCsv();
ok('csv rows == displayed + header', csv.trim().split('\r\n').length === shown + 1);

/* 12. persistence: reload restores config + templates, no auto connect/send */
const st = dump(w);
ok('storage only under own prefix (plus unrelated untouched)', Object.keys(st).every(k => k.startsWith('loom.eth.opus.v1.') || k === 'unrelated.key'));
const w3 = boot(st); const S3 = w3.LoomEth.S, o3 = w3.loomEthOpus;
o3.tick(40);
ok('reload: 3 channels restored, templates restored', S3.chans.length === 3 && w3.LoomEthSock._tpls().some(t => t.name === 'my TP2'));
ok('reload: no socket session open, no TX sent', [...w3.LoomEthSock._sess.values()].every(x => x.state === 'closed' && !x.repeat) && S3.chans.every(c => !c.pkts.some(p => p.own)));
ok('reload: display filter restored', o3.snapshot().df === 'tcp.port == 7000' || o3.snapshot().df.startsWith('tcp.stream'));

/* 13. O1 unicode text */
const u = P.parseText('A😀한\\r\\n\\x41').bytes;
ok('O1 parseText encodes full code points + escapes', JSON.stringify(u) === JSON.stringify([...new TextEncoder().encode('A😀한'), 13, 10, 0x41]), u);

/* 14. close channel */
const C3 = S3.chans[2];
w3.LoomEth.confirmDlg = () => Promise.resolve(true);
(async () => {
  click(w3, `.chcard[data-ch="${C3.id}"] [data-act="ch-menu"]`);
  const item = [...w3.document.querySelectorAll('#menu [data-mi]')].find(b => b.textContent.includes('채널 닫기'));
  click(w3, item);
  await new Promise(r => setTimeout(r, 10));
  w3.document.querySelector('#dlgConfirm [data-r="1"]').dispatchEvent(new w3.MouseEvent('click', { bubbles: true }));
  await new Promise(r => setTimeout(r, 10));
  ok('close channel removes card, others keep live', S3.chans.length === 2 && w3.document.querySelectorAll('.chcard').length === 2 && S3.chans.every(c => c.live));
  const pass = results.filter(r => r.pass).length;
  console.log(`\n${pass}/${results.length} PASS`);
  fs.writeFileSync(path.join(__dirname, 'selfcheck-result.json'), JSON.stringify({ when: new Date().toISOString(), pass, total: results.length, results }, null, 1));
  process.exit(pass === results.length ? 0 : 1);
})();
