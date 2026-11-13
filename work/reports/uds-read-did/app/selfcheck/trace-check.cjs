// Trace tools source-local checks (worker B, items 2/3): jsdom + fake clock + in-test fake IndexedDB.
// NOT browser acceptance. Run: node work/reports/uds-read-did/app/selfcheck/trace-check.cjs
const path = require('path'), fs = require('fs');
const { fakeClock, DIR } = require('./harness.cjs');
const { JSDOM } = require(path.resolve(DIR, '../../../../ui/node_modules/jsdom'));
const RAWHTML = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8');
const html = RAWHTML.replace(/<script src="[^"]+"><\/script>/g, '');
const ALL = [...RAWHTML.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
const SRC = ALL.filter(f => fs.existsSync(path.join(DIR, f))).map(f => [f, fs.readFileSync(path.join(DIR, f), 'utf8')]);

/* ---------- minimal fake IndexedDB (subset used by trace-tools.js), persistent across "reloads" ---------- */
function fakeIDB(data) {
  data = data || { stores: {} };
  const ctl = { failNext: null, failOpen: null, data };
  const later = fn => Promise.resolve().then(fn);
  const KeyRange = { bound: (lo, hi) => ({ lo, hi, has: k => k >= lo && k <= hi }) };
  function db() {
    const names = () => Object.keys(data.stores);
    return {
      objectStoreNames: { contains: n => n in data.stores },
      createObjectStore(n, o) { data.stores[n] = { keyPath: o.keyPath, rows: new Map() }; },
      transaction(list, mode) {
        list = [].concat(list); const t = { oncomplete: null, onerror: null, onabort: null, error: null }, ops = [];
        const fail = ctl.failNext; if (mode === 'readwrite' && fail) ctl.failNext = null;
        t.objectStore = n => {
          const s = data.stores[n];
          const req = fn => { const r = { onsuccess: null, onerror: null, result: undefined }; ops.push(() => { r.result = fn(); if (r.onsuccess) r.onsuccess(); }); return r; };
          return {
            put: v => req(() => { if (!fail) s.rows.set(v[s.keyPath], JSON.parse(JSON.stringify(v))); }),
            delete: k => req(() => { if (fail) return; if (k && k.has) [...s.rows.keys()].filter(k.has).forEach(x => s.rows.delete(x)); else s.rows.delete(k); }),
            getAll: k => req(() => [...s.rows.entries()].filter(([x]) => !k || k.has(x)).sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(e => JSON.parse(JSON.stringify(e[1])))),
          };
        };
        later(() => { ops.forEach(f => f()); if (fail) { t.error = { name: fail, message: 'fake ' + fail }; t.onerror && t.onerror(); } else t.oncomplete && t.oncomplete(); });
        return t;
      },
      names,
    };
  }
  const idb = {
    __KeyRange: KeyRange,
    open() {
      const r = { result: null, onsuccess: null, onerror: null, onupgradeneeded: null, onblocked: null, error: null };
      later(() => {
        if (ctl.failOpen) { r.error = { name: ctl.failOpen }; r.onerror && r.onerror(); return; }
        r.result = db(); if (!('segs' in data.stores)) r.onupgradeneeded && r.onupgradeneeded(); r.onsuccess && r.onsuccess();
      });
      return r;
    },
  };
  return { idb, ctl };
}

function boot(o) {
  o = o || {};
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:4180/uds-read-did/app/index.html', runScripts: 'outside-only', pretendToBeVisual: true });
  const w = dom.window, clock = fakeClock(o.t0); w.__loomUdsClock = clock;
  if (o.storage) for (const [k, v] of Object.entries(o.storage)) w.localStorage.setItem(k, v);
  if (o.idb) w.indexedDB = o.idb;
  w.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  w.Element.prototype.scrollIntoView = function () {};
  w.URL.createObjectURL = () => 'blob:x'; w.URL.revokeObjectURL = () => {};
  w.__dl = [];
  const RealBlob = w.Blob;
  w.HTMLAnchorElement.prototype.click = function () { w.__dl.push(this.download); };
  w.Blob = function (parts, opt) { w.__lastBlob = parts.join(''); return new RealBlob(parts, opt); };
  w.requestAnimationFrame = fn => setTimeout(fn, 0);
  SRC.forEach(([f, s]) => { try { w.eval(s); } catch (e) { throw new Error(f + ': ' + e.message); } });
  return { w, clock, d: w.document, o: w.loomUdsOpus, U: w.LoomUds, H: w.LoomWorkbench, T: w.LoomUdsTrace, E: w.loomUdsOpus.E };
}
const tick = () => new Promise(r => setImmediate(r));
async function settle(T) { for (let i = 0; i < 20; i++) { await T.rec.queue(); await tick(); } }

const results = []; let pass = 0, fail = 0;
function ok(id, cond, note) { results.push({ id, pass: !!cond, note: note || '' }); if (cond) pass++; else { fail++; console.log('FAIL', id, note || ''); } }
const rows = d => [...d.querySelectorAll('#txBody .tx-row')].map(r => +r.dataset.id);
const wins = [];

(async () => {
  /* ===== A — pure filter compile ===== */
  {
    const b = boot(); wins.push(b.w); const { T, E, o, clock, d } = b;
    const C = T.compile;
    const tx = { no: 1, ecu: 'BCM', ecuName: 'Body', route: { label: 'DoIP', summary: 'ETH1 · 0x1001', ptag: 'preset' }, profile: { id: 'preset:BCM|doip', name: 'BCM DoIP' }, transport: 'doip', kind: 'did', sid: 0x22, did: 0xF190, didHex: 'F190', didName: 'VIN', status: 'nrc', nrc: 0x31, request: [0x22, 0xF1, 0x90], final: [0x7F, 0x22, 0x31], events: [{ kind: 'tx', bytes: [0x22, 0xF1, 0x90], label: 'Tx' }, { kind: 'pending', bytes: [0x7F, 0x22, 0x78], label: 'p' }, { kind: 'nrc', bytes: [0x7F, 0x22, 0x31], label: 'n' }] };
    ok('A1 sid', C({ sid: '22' }).pred(tx) && !C({ sid: '0x2E' }).pred(tx));
    ok('A2 did', C({ did: 'f190' }).pred(tx) && !C({ did: 'F18C' }).pred(tx));
    ok('A3 nrc final + pending 78', C({ nrc: '31' }).pred(tx) && C({ nrc: '78' }).pred(tx) && !C({ nrc: '22' }).pred(tx));
    ok('A4 hex byte-aligned', C({ q: '22 f1 90' }).pred(tx) && C({ q: '7F2231' }).pred(tx) && C({ q: '0x7F 0x22' }).pred(tx) && !C({ q: '2F 19' }).pred(tx) && !C({ q: '2231 7F' }).pred(tx));
    ok('A5 text', C({ q: 'vin' }).pred(tx) && C({ q: 'bcm doip' }).pred(tx) && !C({ q: 'gateway' }).pred(tx));
    ok('A6 status/route/profile', C({ status: 'nrc' }).pred(tx) && !C({ status: 'positive' }).pred(tx) && C({ route: 'doip' }).pred(tx) && !C({ route: 'cantp' }).pred(tx) && C({ profile: 'preset:BCM|doip' }).pred(tx) && !C({ profile: 'x' }).pred(tx));
    const bad = C({ sid: 'zz', did: 'F1', nrc: '123' });
    ok('A7 invalid fields reported, not applied', bad.invalid && bad.errors.sid && bad.errors.did && bad.errors.nrc && bad.pred(tx) && !bad.active);
    ok('A8 empty filter inactive', !C({}).active && C({ status: 'did' }).active && C({ status: 'did' }).pred(tx));

    /* ===== B — live DOM filter / selection reconcile ===== */
    E.connect('BCM', 'doip');
    o.st.didText = 'F190'; o.doRead(); clock.advance(500);
    o.st.didText = 'F18C'; o.doRead(); clock.advance(500);
    o.st.sim.scenario = 'nrc'; o.st.didText = 'F190'; o.doRead(); clock.advance(500); o.st.sim.scenario = 'positive';
    ok('B1 three rows', rows(d).length === 3);
    T.filters.set({ did: 'F18C' });
    ok('B2 filter DID → 1 row + count', rows(d).length === 1 && /표시 1 \/ 3/.test(d.getElementById('ttCount').textContent));
    T.filters.set({ did: '', status: 'nrc' });
    ok('B3 status nrc → selected row is the nrc row', rows(d).length === 1 && o.st.selTxn === rows(d)[0] && E.byId(rows(d)[0]).status === 'nrc');
    T.filters.set({ status: '', sid: '19' });
    ok('B4 empty result clears selection + detail', rows(d).length === 0 && o.st.selTxn == null && d.getElementById('seqHead').textContent === '' && /맞는 기록 없음/.test(d.getElementById('ttMsg').textContent));
    T.filters.set({ sid: 'q' });
    ok('B5 invalid filter visible error, aria-invalid', d.getElementById('ttSid').getAttribute('aria-invalid') === 'true' && /적용하지 않음/.test(d.getElementById('ttMsg').textContent) && rows(d).length === 3);
    T.filters.clear();
    ok('B6 clear restores, inputs emptied', rows(d).length === 3 && d.getElementById('ttSid').value === '' && /전체 3건/.test(d.getElementById('ttCount').textContent));
    T.filters.set({ q: '62 F1 8C' });
    ok('B7 hex search on response bytes', rows(d).length === 1 && E.byId(rows(d)[0]).didHex === 'F18C');
    T.filters.clear();
    // export handlers untouched + unfiltered
    const json = b.w.LoomUdsTools.exportJson();
    T.filters.set({ did: 'F18C' });
    const json2 = b.w.LoomUdsTools.exportJson();
    ok('B8 existing JSON export unchanged by display filter', JSON.parse(json).transactions.length === 3 && JSON.parse(json2).transactions.length === 3);
    T.filters.clear();

    /* ===== C — display pause ===== */
    o.st.sim.scenario = 'pending'; o.st.didText = 'F190'; o.doRead(); clock.advance(100); o.st.sim.scenario = 'positive';
    const pendId = E.txns[E.txns.length - 1].id;
    o.selectTxn(pendId);
    T.pause();
    const frozenIds = rows(d);
    clock.advance(6000); // pending → final while paused
    o.doRead(); clock.advance(500);
    ok('C1 engine continues while paused', E.byId(pendId).status === 'positive' && E.txns.length === 5);
    ok('C2 display frozen', JSON.stringify(rows(d)) === JSON.stringify(frozenIds) && rows(d).length === 4);
    ok('C3 selected detail = frozen snapshot', d.getElementById('tx-' + pendId).className.includes('s-waiting') || d.getElementById('tx-' + pendId).className.includes('s-pending'));
    const fz = T.view.byId(pendId);
    ok('C4 frozen clone immutable + elapsed frozen', Object.isFrozen(fz) && fz.status !== 'positive' && fz.tEnd === fz.frozenAt && fz !== E.byId(pendId));
    ok('C5 paused notice counts new events', /새 이벤트 [1-9]\d*건/.test(d.getElementById('ttMsg').textContent) && d.getElementById('ttPause').getAttribute('aria-pressed') === 'true');
    T.filters.set({ status: 'active' });
    ok('C6 filter applies to frozen rows', rows(d).length === 1 && rows(d)[0] === pendId);
    T.filters.clear();
    // scope: core filter 'ecu' while paused, other ECU frozen rows excluded even after clear
    E.connect('GW', 'doip'); b.H.send({ ecu: 'GW', transport: 'doip', mode: 'did', didText: 'F190', source: 'test' }); clock.advance(500);
    T.resume(); T.pause();
    o.st.filter = 'ecu'; o.render();
    ok('C7 explicit traceScope excludes other ECU frozen rows', rows(d).every(id => T.view.byId(id).ecu === 'BCM') && rows(d).length === 5);
    o.st.filter = 'all'; o.render();
    ok('C8 scope all includes GW frozen', rows(d).length === 6);
    T.resume();
    ok('C9 resume shows live, detail live', rows(d).length === 6 && T.view.byId(pendId) == null && !d.getElementById('ttRoot').classList.contains('paused'));
    ok('C10 timeText rel/abs', T.view.timeText(E.txns[0]).includes(':') && (b.d.querySelector('#ttTime [data-tm="rel"]').click(), /^\+0\.000 s$/.test(T.view.timeText(E.txns[0]))));
    b.d.querySelector('#ttTime [data-tm="abs"]').click();
  }

  /* ===== D — recorder with fake IDB ===== */
  const F = fakeIDB();
  {
    const b = boot({ idb: F.idb }); wins.push(b.w); const { T, E, o, clock, d, H } = b;
    await T.ready; await settle(T);
    ok('D1 idb store, idle, no auto start', T.rec.storeKind() === 'idb' && T.rec.state() === 'idle' && H.activity.list().length === 0);
    E.connect('BCM', 'doip');
    const p = T.rec.start();
    ok('D2 starting is not claimed as recording', T.rec.state() === 'starting' && /저장 확인 전/.test(d.getElementById('tlSt').textContent));
    ok('D3 activity registered while starting', H.activity.list().some(x => x.kind === 'recording' && x.key === 'trace-log'));
    ok('D4 start promise resolves recording', (await p) === true && T.rec.state() === 'recording' && /기록 중/.test(d.getElementById('tlSt').textContent));
    // traffic: pending, nrc, timeout+late, raw, cancel; display paused + filtered meanwhile
    T.pause(); T.filters.set({ did: 'FFFF' });
    o.st.sim.scenario = 'pending'; o.doRead(); clock.advance(4000);
    o.st.sim.scenario = 'nrc'; o.doRead(); clock.advance(500);
    o.st.sim.scenario = 'timeout'; o.st.sim.late = true; o.doRead(); clock.advance(3000); o.st.sim.late = false;
    o.st.sim.scenario = 'positive'; o.setMode('raw'); o.st.rawText = '3E 00'; o.doSend(); clock.advance(500); o.setMode('did');
    o.doRead(); E.cancel('BCM', 'doip'); clock.advance(1000);
    T.resume(); T.filters.clear();
    // > history limit: 200 more reads → engine evicts the earliest
    for (let i = 0; i < 200; i++) { o.doRead(); clock.advance(450); }
    ok('D5 engine evicted early rows', E.txns.length === 200 && !E.byId(1));
    E.disconnect('BCM', 'doip', 'user');
    await settle(T);
    const st = await T.rec.stop(); await settle(T);
    ok('D6 stop → idle, closed segment, activity cleared', st === true && T.rec.state() === 'idle' && T.rec.segs()[0].state === 'closed' && H.activity.list().length === 0);
    const seg = T.rec.segs()[0];
    const text = await T.rec.text(seg.id);
    const recs = text.trim().split('\n').map(l => JSON.parse(l));
    const txRecs = recs.filter(r => r.r === 'tx');
    ok('D7 every txn captured despite eviction/pause/filter', txRecs.length === 205 && recs.some(r => r.r === 'tx' && r.id === 1));
    const k = kind => recs.filter(r => r.r === 'ev' && r.kind === kind).length;
    ok('D8 tx/pending/nrc/timeout/late(timeout+after-cancel)/cancel/positive events', k('tx') === 205 && k('pending') >= 2 && k('nrc') === 1 && k('timeout') === 1 && k('late') === 2 && k('cancel') === 1 && k('positive') >= 200);
    ok('D9 ctx + marks + header', recs[0].r === 'hdr' && recs[0].format === 'loom-uds-trace-log/1' && recs.some(r => r.r === 'ctx' && r.link === 'offline') && recs[recs.length - 1].text === 'stop');
    ok('D10 meta records == stored lines, bytes == utf8', seg.records === recs.length && seg.bytes === Buffer.byteLength(text, 'utf8'));
    const t1 = txRecs[0];
    ok('D11 tx record carries profile + immutable route config/fields + timing', t1.profile.id && t1.config && t1.config.host && t1.config.port && Array.isArray(t1.fields) && t1.fields.length >= 3 && t1.p2 === 1000);
    ok('D12 raw request bytes exact', txRecs.some(r => r.kind === 'raw' && r.request === '3E 00' && r.sid === 0x3E));
    // export exact + JSON
    T.rec.select(seg.id);
    const jl = await T.rec.exportSeg('jsonl');
    ok('D13 JSONL download = exact stored bytes', jl === text && b.w.__lastBlob === text && b.w.__dl.some(n => /\.jsonl$/.test(n)));
    const js = JSON.parse(await T.rec.exportSeg('json'));
    ok('D14 JSON download records', js.format === 'loom-uds-trace-log/1' && js.records.length === recs.length && js.segment.id === seg.id);
    // offline viewer from segment
    const before = E.txns.length, timers = clock.pending();
    await (async () => { T.viewer.open(); await T.viewer.openSegment(seg.id); })();
    const m = T.viewer.model();
    ok('D15 viewer reconstructs all txns, no engine effect', m && m.txns.length === 205 && E.txns.length === before && clock.pending() === timers);
    const late = m.txns.find(t => t.events.some(e => e.kind === 'late'));
    ok('D16 viewer txn exact: late + status + profile/config', late && late.status === 'timeout' && late.route.values.host && d.getElementById('tlViewer').hidden === false);
    T.viewer.setFilters({ status: 'nrc' });
    ok('D17 viewer filter', T.viewer.rows().length === 1 && /표시 1 \/ 205/.test(d.getElementById('tvCount').textContent));
    T.viewer.select(late.id); T.viewer.setFilters({ status: '' }); T.viewer.select(late.id);
    ok('D18 viewer detail shows config + late', /경로 설정/.test(d.getElementById('tvDet').textContent) && /late/.test(d.getElementById('tvDet').textContent));
    ok('D19 viewer has no send controls', !d.querySelector('#tlViewer [data-cmd="read"], #tlViewer #btnRead') && /전송·재생·연결 없음/.test(d.getElementById('tlViewer').textContent));
    d.getElementById('tlViewer').dispatchEvent(new b.w.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    ok('D20 Esc closes viewer', d.getElementById('tlViewer').hidden === true);

    // second segment, then delete only selected
    await T.rec.start(); clock.advance(100); E.connect('BCM', 'doip'); o.doRead(); clock.advance(500); await T.rec.stop(); await settle(T);
    ok('D21 two segments', T.rec.segs().length === 2);
    const keep = T.rec.segs().find(s => s.id !== seg.id);
    T.rec.select(seg.id);
    d.getElementById('tlSegBtn').click(); await settle(T);
    const armed = await T.rec.deleteSeg();
    ok('D22 delete arms first (confirm button, nothing deleted)', armed === false && T.rec.segs().length === 2 && /삭제 확인 \(이 세그먼트만\)/.test(d.getElementById('tlSegs').textContent));
    ok('D23 delete confirm removes only selected', (await T.rec.deleteSeg()) === true && T.rec.segs().length === 1 && T.rec.segs()[0].id === keep.id);
    // start, then "reload" while recording
    await T.rec.start(); o.doRead(); clock.advance(500); await T.rec.flush(); await settle(T);
    b.w.close();
  }
  {
    const b = boot({ idb: F.idb }); wins.push(b.w); const { T, H } = b;
    await T.ready; await settle(T);
    const segs = T.rec.segs();
    ok('D24 reload: recording NOT resumed, segment listed interrupted', T.rec.state() === 'idle' && H.activity.list().length === 0 && segs.length === 2 && segs.some(s => s.state === 'interrupted'));
    // quota failure mid-recording
    await T.rec.start(); b.E.connect('BCM', 'doip');
    F.ctl.failNext = 'QuotaExceededError';
    b.o.doRead(); b.clock.advance(1500); await settle(T);
    ok('D25 quota error → error state, visible, no false recording', T.rec.state() === 'error' && /QuotaExceededError/.test(b.d.getElementById('tlSt').textContent) && !/기록 중 ·/.test(b.d.getElementById('tlSt').textContent) && H.activity.list().length === 0 && b.d.getElementById('tlRec').textContent === '기록 시작');
    // start failure
    F.ctl.failNext = 'QuotaExceededError';
    const r = await T.rec.start(); await settle(T);
    ok('D26 start failure → not recording', r === false && T.rec.state() === 'error' && /기록 시작 실패/.test(T.rec.err()));
    // rotation + retention (limits lowered for the check)
    F.ctl.failNext = null;
    const L = T.LIMITS, keepL = Object.assign({}, L);
    L.segRecords = 40; L.maxSegments = 5;
    await T.rec.start();
    for (let i = 0; i < 30; i++) { b.o.doRead(); b.clock.advance(500); }
    await T.rec.stop(); await settle(T);
    const after = T.rec.segs();
    ok('D27 rotation creates parts; retention keeps ≤ max', after.length === 5 && after.some(s => s.part > 1) && /보존 한도|세그먼트 회전/.test(T.rec.notice()));
    const part = after.filter(s => s.part > 1 && s.records > 10)[0];
    const partText = part ? await T.rec.text(part.id) : '';
    const pr = T.parseLog(partText, b.U, 'part');
    ok('D28 rotated part self-contained (header + parse ok)', pr.ok && JSON.parse(partText.split('\n')[0]).r === 'hdr' && pr.value.txns.length > 0, pr.error);
    Object.assign(L, keepL);
  }
  {
    const F2 = fakeIDB(); F2.ctl.failOpen = 'SecurityError';
    const b = boot({ idb: F2.idb }); wins.push(b.w); const { T } = b;
    await T.ready; await settle(T);
    ok('D29 IDB open failure → honest memory fallback notice', T.rec.storeKind() === 'memory' && /IndexedDB 열기 실패/.test(b.d.getElementById('tlSt').textContent) && /메모리/.test(b.d.getElementById('tlSt').textContent));
  }

  /* ===== E — import validation / sanitization ===== */
  {
    const b = boot(); wins.push(b.w); const { T, U, d, E, clock } = b;
    const hdr = JSON.stringify({ r: 'hdr', t: 1, format: 'loom-uds-trace-log/1', seg: 's', part: 1, startedAt: 'x' });
    const txl = JSON.stringify({ r: 'tx', t: 2, id: 1, no: 1, ecu: 'BCM', transport: 'doip', routeLabel: 'DoIP', profile: { id: 'p' }, kind: 'did', sid: 34, did: 61840, request: '22 F1 90' });
    const ev = lab => JSON.stringify({ r: 'ev', t: 3, id: 1, i: 0, kind: 'tx', bytes: '22 F1 90', label: lab });
    T.viewer.open();
    T.viewer.loadText([hdr, txl, ev('<img src=x onerror=alert(1)>')].join('\n') + '\n', 'ok.jsonl', 'f');
    ok('E1 valid JSONL opens; label escaped (no injected element)', T.viewer.model().txns.length === 1 && !d.querySelector('#tlViewer img') && /&lt;img|<img/.test(d.getElementById('tvDet').textContent));
    const prev = T.viewer.model();
    const bad = T.viewer.loadText([hdr, txl, JSON.stringify({ r: 'ev', t: 3, id: 1, i: 0, kind: 'tx', bytes: '22 G1' })].join('\n'), 'bad.jsonl', 'f');
    ok('E2 bad hex rejects whole file with line number, previous model kept', !bad.ok && /3행/.test(bad.error) && T.viewer.model() === prev && /아무것도 불러오지 않음/.test(d.getElementById('tvMsg').textContent));
    ok('E3 missing header rejected', !T.parseLog(txl + '\n', U).ok);
    ok('E4 unknown format rejected', /알 수 없는 형식/.test(T.parseLog('{"format":"other/9"}', U).error));
    ok('E5 orphan ev rejected', !T.parseLog([hdr, ev('a')].join('\n'), U).ok);
    ok('E6 non-object / wrong types rejected', !T.parseLog([hdr, '[1,2]'].join('\n'), U).ok && !T.parseLog([hdr, JSON.stringify({ r: 'tx', t: 2, id: 'x' })].join('\n'), U).ok);
    ok('E7 overlong string rejected', !T.parseLog([hdr, txl, ev('x'.repeat(500))].join('\n'), U).ok);
    const big = 'x'.repeat(T.LIMITS.importBytes + 1);
    ok('E8 size cap', /상한/.test(T.parseLog(big, U).error));
    ok('E9 bad config value rejected', !T.parseLog([hdr, JSON.stringify(Object.assign(JSON.parse(txl), { config: { host: { a: 1 } } }))].join('\n'), U).ok);
    ok('E10 future status token accepted (suppressed)', T.parseLog([hdr, txl, JSON.stringify({ r: 'st', t: 4, id: 1, status: 'suppressed' })].join('\n'), U).value.txns[0].status === 'suppressed');
    ok('E11 bad status token rejected', !T.parseLog([hdr, txl, JSON.stringify({ r: 'st', t: 4, id: 1, status: '<b>' })].join('\n'), U).ok);
    // existing trace/1 export opens read-only
    E.connect('BCM', 'doip'); b.o.doRead(); clock.advance(500); b.o.st.sim.scenario = 'nrc'; b.o.doRead(); clock.advance(500);
    const tj = b.w.LoomUdsTools.exportJson();
    const n0 = E.txns.length;
    const r1 = T.viewer.loadText(tj, 'trace.json', 'f');
    ok('E12 trace/1 export opens (exact bytes/profile/config/events)', r1.ok && r1.value.txns.length === 2 && r1.value.txns[1].status === 'nrc' && hexEq(r1.value.txns[0].final, E.txns[0].final) && r1.value.txns[0].route.values.host === E.txns[0].route.values.host && E.txns.length === n0, r1.error);
    ok('E13 viewer import never touches engine', E.txns.length === n0 && E.activeCount() === 0);
    // project section trace
    const sec = b.H.project.sections().get('trace');
    ok('E14 project section trace registered, pure validate', sec && sec.validate({ v: 1, timeMode: 'rel' }).ok && !sec.validate({ v: 2 }).ok && !sec.validate({ v: 1, timeMode: 'x' }).ok && T.prefs().timeMode === 'abs');
    sec.import(sec.validate({ v: 1, timeMode: 'rel', viewerTime: 'abs' }).value);
    ok('E15 import applies prefs only + persisted', T.prefs().timeMode === 'rel' && JSON.parse(b.w.localStorage.getItem('loom.udsReadDid.trace.v1')).timeMode === 'rel' && T.rec.state() !== 'recording');
    {
      const proto = b.w.Storage.prototype, orig = proto.setItem, before = JSON.stringify(T.prefs());
      proto.setItem = function () { const e = new Error('quota'); e.name = 'QuotaExceededError'; throw e; };
      let threw = null; try { sec.import(sec.validate({ v: 1, timeMode: 'abs', viewerTime: 'rel' }).value); } catch (e) { threw = e; }
      proto.setItem = orig;
      ok('E17 strict import THROWS on storage failure, prefs untouched', threw && threw.name === 'QuotaExceededError' && JSON.stringify(T.prefs()) === before);
    }
    { const a1 = sec.defaults(), a2 = sec.defaults(); a1.timeMode = 'rel';
      ok('E18 defaults(): fresh, valid, factory prefs', a2.timeMode === 'abs' && sec.validate(a2).ok && a1 !== a2); }
    ok('E16 export of section has no log data', JSON.stringify(sec.export()).length < 120);
  }
  /* ===== F — static ===== */
  {
    const js = fs.readFileSync(path.join(DIR, 'trace-tools.js'), 'utf8'), css = fs.readFileSync(path.join(DIR, 'trace-tools.css'), 'utf8');
    ok('F1 no network/send APIs in trace-tools', !/\bfetch\(|XMLHttpRequest|WebSocket|\.sendRaw\(|\.read\(\{|H\.send\(|E\.connect|showSaveFilePicker/.test(js));
    ok('F2 onRender callback does not refresh', /H\.onRender\(renderToolbar\)/.test(js) && !/function renderToolbar\(\)[^]*?H\.refresh\(\)[^]*?function profileOptions/.test(js.slice(js.indexOf('function renderToolbar'), js.indexOf('Object.entries(fIds)'))));
    ok('F3 viewer overlay fixed + mobile rules', /\.tl-viewer \{ position: fixed/.test(css) && /@media \(max-width: 560px\)/.test(css));
  }

  wins.forEach(w => { try { w.close(); } catch (e) { /* closed */ } });
  const out = { check: 'trace-check', pass, fail, total: pass + fail, results };
  fs.writeFileSync(path.join(DIR, '../qa/team/trace-check-result.log'), JSON.stringify(out, null, 1));
  console.log(`trace-check ${pass}/${pass + fail} ${fail ? 'FAIL' : 'PASS'}`);
  process.exit(fail ? 1 : 0);
})().catch(e => { console.error('ERROR', e); wins.forEach(w => { try { w.close(); } catch (x) { /* */ } }); process.exit(2); });
function hexEq(a, b) { return JSON.stringify(Array.from(a || [])) === JSON.stringify(Array.from(b || [])); }
