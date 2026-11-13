/* Loom UDS workbench — workflow extension (owner A): saved commands + recent reuse (1), watch + sparklines (7), sequences (8).
   LOCAL SIM only. Every request goes through the shared host H.send (same engine read/sendRaw, same context/profile/timing
   snapshots, same busy/offline guards). No synthetic history rows, no auto start on reload/import/reconnect.
   Definitions/prefs persist under own key only; runtime (runs, values, recent) is never persisted or imported. */
(function (root) {
  'use strict';
  const KEY = 'loom.udsReadDid.workflow.v1';
  const FORMAT = 'loom-uds-workflow/1';
  const LIM = {
    commands: 200, watches: 32, sequences: 50, steps: 50, loops: [1, 100], delay: [0, 60000], interval: [200, 60000],
    name: 40, desc: 200, folder: 32, recent: 20, samples: 60, results: 200, sendsPerRun: 1000, prefix: 64, fileBytes: 512 * 1024,
  };
  const GAP_MS = 100;            // owned automation yields this long after its own completion before the next owned send
  const INTERVALS = [200, 500, 1000, 2000, 5000, 10000];
  const EXPECT = ['positive', 'nrc', 'response', 'any'];
  const TABS = ['commands', 'recent', 'sequences'];
  const CTRL = /[\u0000-\u001f\u007f]/;
  const ID_RE = /^[A-Za-z0-9_.-]{1,24}$/;          // ECU id shape (existence checked at run time: registry is dynamic)
  const TR_IDS = ['doip', 'cantp', 'ethdiag'];

  /* =================== pure model =================== */
  function U() { return root.LoomUds; }
  const isObj = x => !!x && typeof x === 'object' && !Array.isArray(x);
  function text(v, max, label, required) {
    if (v == null) v = '';
    if (typeof v !== 'string') return { error: `${label}: 문자열 아님` };
    const s = v.trim();
    if (required && !s) return { error: `${label}: 값을 입력하세요` };
    if (s.length > max) return { error: `${label}: ${max}자 이하` };
    if (CTRL.test(s)) return { error: `${label}: 제어 문자 불가` };
    return { v: s };
  }
  function target(t, label) {
    if (t == null) return { v: null };
    if (!isObj(t)) return { error: `${label}: 대상 형식 오류` };
    if (typeof t.ecu !== 'string' || !ID_RE.test(t.ecu)) return { error: `${label}: ECU id 형식 오류` };
    if (!TR_IDS.includes(t.transport)) return { error: `${label}: 알 수 없는 경로` };
    return { v: { ecu: t.ecu, transport: t.transport } };
  }
  // Request payload: DID text (normalized hex4) or exact raw bytes (normalized "22 F1 90").
  function request(mode, x, label) {
    if (mode === 'did') {
      const r = U().validateDid(x.didText);
      return r.ok ? { v: { mode, didText: r.norm } } : { error: `${label}: ${r.error}` };
    }
    if (mode === 'raw') {
      const r = U().parseRawHex(x.rawText);
      return r.ok ? { v: { mode, rawText: r.norm } } : { error: `${label}: ${r.error}` };
    }
    return { error: `${label}: 모드는 DID 또는 Raw` };
  }
  const intIn = (v, lo, hi) => Number.isSafeInteger(v) && v >= lo && v <= hi;
  function idOk(id, p) { return typeof id === 'string' && new RegExp(`^${p}\\d{1,6}$`).test(id); }

  function vCommand(x, i) {
    const L = `명령 #${i + 1}`;
    if (!isObj(x)) return { error: `${L}: 객체 아님` };
    if (!idOk(x.id, 'c')) return { error: `${L}: id 형식 오류` };
    const n = text(x.name, LIM.name, `${L} 이름`, true); if (n.error) return n;
    const ds = text(x.desc, LIM.desc, `${L} 설명`); if (ds.error) return ds;
    const f = text(x.folder, LIM.folder, `${L} 폴더`); if (f.error) return f;
    const rq = request(x.mode, x, L); if (rq.error) return rq;
    const t = target(x.target, L); if (t.error) return t;
    return { v: Object.assign({ id: x.id, name: n.v, desc: ds.v, folder: f.v }, rq.v, { target: t.v }) };
  }
  function vWatch(x, i) {
    const L = `감시 #${i + 1}`;
    if (!isObj(x)) return { error: `${L}: 객체 아님` };
    if (!idOk(x.id, 'w')) return { error: `${L}: id 형식 오류` };
    const r = U().validateDid(x.did); if (!r.ok) return { error: `${L}: ${r.error}` };
    const t = target(x.target, L); if (t.error) return t;
    if (!t.v) return { error: `${L}: 대상 필요` };
    if (!intIn(x.intervalMs, LIM.interval[0], LIM.interval[1])) return { error: `${L}: 주기 ${LIM.interval[0]}–${LIM.interval[1]} ms 정수` };
    return { v: { id: x.id, did: r.norm, target: t.v, intervalMs: x.intervalMs } };
  }
  function vStep(x, i, j, cmdIds) {
    const L = `시퀀스 #${i + 1} 단계 ${j + 1}`;
    if (!isObj(x)) return { error: `${L}: 객체 아님` };
    let base;
    if (x.mode === 'cmd') {
      if (!idOk(x.cmdId, 'c')) return { error: `${L}: 명령 참조 형식 오류` };
      if (cmdIds && !cmdIds.has(x.cmdId)) return { error: `${L}: 존재하지 않는 명령 ${x.cmdId}` };
      base = { mode: 'cmd', cmdId: x.cmdId };
    } else { const rq = request(x.mode, x, L); if (rq.error) return rq; base = rq.v; }
    if (!intIn(x.delayMs, LIM.delay[0], LIM.delay[1])) return { error: `${L}: 지연 ${LIM.delay[0]}–${LIM.delay[1]} ms 정수` };
    const ex = isObj(x.expect) ? x.expect : {};
    const st = ex.status == null ? 'positive' : ex.status;
    if (!EXPECT.includes(st)) return { error: `${L}: 기대 상태 오류` };
    let prefix = '';
    if (ex.prefix != null && String(ex.prefix).trim() !== '') {
      const p = U().parseRawHex(ex.prefix);
      if (!p.ok) return { error: `${L} 응답 접두: ${p.error}` };
      if (p.len > LIM.prefix) return { error: `${L} 응답 접두: 최대 ${LIM.prefix} B` };
      prefix = p.norm;
    }
    return { v: Object.assign(base, { delayMs: x.delayMs, expect: { status: st, prefix } }) };
  }
  function vSequence(x, i, cmdIds) {
    const L = `시퀀스 #${i + 1}`;
    if (!isObj(x)) return { error: `${L}: 객체 아님` };
    if (!idOk(x.id, 's')) return { error: `${L}: id 형식 오류` };
    const n = text(x.name, LIM.name, `${L} 이름`, true); if (n.error) return n;
    const t = target(x.target, L); if (t.error) return t;
    if (!intIn(x.loops, LIM.loops[0], LIM.loops[1])) return { error: `${L}: 반복 ${LIM.loops[0]}–${LIM.loops[1]}` };
    if (typeof x.stopOnError !== 'boolean') return { error: `${L}: 오류 시 중지 값은 true/false` };
    if (!Array.isArray(x.steps) || !x.steps.length) return { error: `${L}: 단계가 비어 있음` };
    if (x.steps.length > LIM.steps) return { error: `${L}: 단계 최대 ${LIM.steps}` };
    if (x.steps.length * x.loops > LIM.sendsPerRun) return { error: `${L}: 단계×반복 최대 ${LIM.sendsPerRun}회 전송` };
    const steps = [];
    for (let j = 0; j < x.steps.length; j++) { const s = vStep(x.steps[j], i, j, cmdIds); if (s.error) return s; steps.push(s.v); }
    return { v: { id: x.id, name: n.v, target: t.v, loops: x.loops, stopOnError: x.stopOnError, steps } };
  }
  function vPrefs(p) {
    p = isObj(p) ? p : {};
    return { tab: TABS.includes(p.tab) ? p.tab : 'commands', folder: typeof p.folder === 'string' && p.folder.length <= LIM.folder && !CTRL.test(p.folder) ? p.folder : '' };
  }
  // Whole-document validation: pure, all-or-nothing. Returns normalized copy.
  function validate(json) {
    if (!isObj(json)) return { ok: false, error: '객체가 아님' };
    if (json.format !== FORMAT) return { ok: false, error: `알 수 없는 형식/버전 ${JSON.stringify(String(json.format)).slice(0, 40)} — ${FORMAT}만 지원` };
    const arr = (k, max, label) => {
      const a = json[k] == null ? [] : json[k];
      if (!Array.isArray(a)) return { error: `${label}: 배열 아님` };
      if (a.length > max) return { error: `${label}: 최대 ${max}개` };
      return { v: a };
    };
    const ca = arr('commands', LIM.commands, '명령'); if (ca.error) return { ok: false, error: ca.error };
    const wa = arr('watches', LIM.watches, '감시'); if (wa.error) return { ok: false, error: wa.error };
    const sa = arr('sequences', LIM.sequences, '시퀀스'); if (sa.error) return { ok: false, error: sa.error };
    const out = { format: FORMAT, commands: [], watches: [], sequences: [], prefs: vPrefs(json.prefs) };
    const seen = new Set();
    const uniq = id => { if (seen.has(id)) return false; seen.add(id); return true; };
    for (let i = 0; i < ca.v.length; i++) { const r = vCommand(ca.v[i], i); if (r.error) return { ok: false, error: r.error }; if (!uniq(r.v.id)) return { ok: false, error: `중복 id ${r.v.id}` }; out.commands.push(r.v); }
    const cmdIds = new Set(out.commands.map(c => c.id));
    for (let i = 0; i < wa.v.length; i++) { const r = vWatch(wa.v[i], i); if (r.error) return { ok: false, error: r.error }; if (!uniq(r.v.id)) return { ok: false, error: `중복 id ${r.v.id}` }; out.watches.push(r.v); }
    for (let i = 0; i < sa.v.length; i++) { const r = vSequence(sa.v[i], i, cmdIds); if (r.error) return { ok: false, error: r.error }; if (!uniq(r.v.id)) return { ok: false, error: `중복 id ${r.v.id}` }; out.sequences.push(r.v); }
    return { ok: true, value: out };
  }
  const emptyDoc = () => ({ format: FORMAT, commands: [], watches: [], sequences: [], prefs: vPrefs(null) });
  const clone = x => JSON.parse(JSON.stringify(x));
  function nextId(doc, p) {
    const all = [].concat(doc.commands, doc.watches, doc.sequences).map(x => x.id).filter(id => id[0] === p).map(id => +id.slice(1));
    return p + (all.length ? Math.max.apply(null, all) + 1 : 1);
  }
  // Expected-result check for a sequence step (pure).
  function expectOk(expect, tx) {
    const s = tx.status;
    const stOk = expect.status === 'any' ? true : expect.status === 'response' ? (s === 'positive' || s === 'nrc') : s === expect.status;
    if (!stOk) return { ok: false, note: `상태 ${s} — 기대 ${expect.status}` };
    if (expect.prefix) {
      const want = U().parseRawHex(expect.prefix).bytes;
      const got = tx.final || [];
      if (got.length < want.length || want.some((b, i) => got[i] !== b)) return { ok: false, note: `응답 접두 불일치 — 기대 ${expect.prefix}` };
    }
    return { ok: true, note: '' };
  }

  const model = { validate, vCommand, vWatch, vSequence, vStep, expectOk, emptyDoc, nextId, LIM, FORMAT, KEY, INTERVALS, EXPECT, GAP_MS };

  /* =================== runtime =================== */
  function create(H, opts) {
    opts = opts || {};
    const E = H.engine, clock = H.clock, d = root.document;
    const storage = opts.storage !== undefined ? opts.storage : (() => { try { return root.localStorage; } catch (e) { return null; } })();
    const listeners = [];
    const owned = new Map();        // txnId -> { kind:'watch'|'seq'|'command', ref, tx }
    const wrt = new Map();          // watch id -> runtime
    const runs = new Map();         // sequence id -> last run (runtime)
    const cmdLast = new Map();      // command id -> { ok, no?, txId?, error? }
    const recent = [];              // bounded, runtime only
    const deferred = new Map();     // ctxKey -> [watch ids] (bounded by watch count)
    let notice = '', storeErr = '';
    let doc = loadStored();
    let runSeq = 0, ageTimer = null, disposed = false;

    function loadStored() {
      let raw = null;
      try { raw = storage ? storage.getItem(KEY) : null; } catch (e) { notice = '저장소를 읽을 수 없음 — 기본값 사용'; return emptyDoc(); }
      if (raw == null) return emptyDoc();
      let j; try { j = JSON.parse(raw); } catch (e) { notice = '저장된 워크플로 정의가 손상되어 무시됨 (기본값 사용, 원본 키는 덮어쓸 때까지 유지)'; return emptyDoc(); }
      const r = validate(j);
      if (!r.ok) { notice = `저장된 워크플로 정의 무시: ${r.error}`; return emptyDoc(); }
      return r.value;
    }
    function persist() {
      if (!storage) return false;
      try { storage.setItem(KEY, JSON.stringify(doc)); storeErr = ''; return true; } catch (e) { storeErr = '저장 실패 (용량/권한) — 변경은 이 세션에만 유지됨'; return false; }
    }
    const emit = what => listeners.forEach(fn => { try { fn(what); } catch (e) { /* isolate */ } });
    const now = () => clock.now();
    const ckey = t => t.ecu + '|' + t.transport;
    const ecuExists = id => (H.constants.ECUS || []).some(e => e.id === id);
    const curCtx = () => { const c = H.context() || {}; return { ecu: c.ecu, transport: c.transport }; };
    const ctxOf = t => E.ctx(t.ecu, t.transport);
    function routeTag(t) {
      try { const r = E.routeFor(t.ecu, t.transport); return r ? `${r.label} · ${r.ptag}` : '대상 없음'; } catch (e) { return '대상 없음'; }
    }
    function trackOwned(tx, kind, ref) { owned.set(tx.id, { kind, ref, tx }); }
    const seqOnCtx = k => [...runs.values()].some(r => r.status === 'running' && ckey(r.target) === k);

    /* ---------- definitions CRUD ---------- */
    function commit(next, what) {
      const r = validate(next);
      if (!r.ok) return { ok: false, error: r.error };
      doc = r.value; persist(); emit(what || 'defs');
      return { ok: true };
    }
    function saveCommand(draft) {
      const next = clone(doc), isNew = !draft.id || draft.id === 'new';
      const id = isNew ? nextId(doc, 'c') : draft.id;
      if (isNew && doc.commands.length >= LIM.commands) return { ok: false, error: `명령 최대 ${LIM.commands}개` };
      const c = { id, name: draft.name, desc: draft.desc || '', folder: draft.folder || '', mode: draft.mode, didText: draft.mode === 'did' ? draft.didText : undefined, rawText: draft.mode === 'raw' ? draft.rawText : undefined, target: draft.target || null };
      const one = vCommand(c, 0);
      if (one.error) return { ok: false, error: one.error.replace(/^명령 #1:? ?/, '') };
      if (next.commands.some(x => x.id !== id && x.name === one.v.name && x.folder === one.v.folder)) return { ok: false, error: '같은 폴더에 같은 이름의 명령이 있음' };
      const i = next.commands.findIndex(x => x.id === id);
      if (!isNew && i < 0) return { ok: false, error: '편집 대상 명령이 없음 (삭제됨)' };
      if (i >= 0) next.commands[i] = one.v; else next.commands.push(one.v);
      const r = commit(next);
      return r.ok ? { ok: true, id } : r;
    }
    function uniqueName(list, name, folder) {
      let n = name;
      for (let j = 2; list.some(x => x.name === n && (folder === undefined || x.folder === folder)); j++) n = name.slice(0, LIM.name - 5) + ` (${j})`;
      return n;
    }
    function duplicateCommand(id) {
      const c = doc.commands.find(x => x.id === id); if (!c) return { ok: false, error: '명령 없음' };
      const copy = Object.assign(clone(c), { id: 'new', name: uniqueName(doc.commands, c.name + ' 복사', c.folder) });
      return saveCommand(copy);
    }
    function deleteCommand(id) {
      const users = doc.sequences.filter(s => s.steps.some(st => st.mode === 'cmd' && st.cmdId === id));
      if (users.length) return { ok: false, error: `시퀀스에서 사용 중: ${users.map(s => s.name).join(', ')} — 먼저 단계를 바꾸세요` };
      const next = clone(doc); next.commands = next.commands.filter(x => x.id !== id);
      cmdLast.delete(id);
      return commit(next);
    }
    function addWatch(w) {
      if (doc.watches.length >= LIM.watches) return { ok: false, error: `감시 최대 ${LIM.watches}개` };
      const x = { id: nextId(doc, 'w'), did: w.did, target: w.target, intervalMs: +w.intervalMs };
      const one = vWatch(x, 0); if (one.error) return { ok: false, error: one.error.replace(/^감시 #1:? ?/, '') };
      if (doc.watches.some(y => y.did === one.v.did && ckey(y.target) === ckey(one.v.target))) return { ok: false, error: '같은 대상·DID 감시가 이미 있음' };
      const next = clone(doc); next.watches.push(one.v);
      const r = commit(next, 'watch-defs');
      return r.ok ? { ok: true, id: x.id } : r;
    }
    function setWatchInterval(id, ms) {
      const next = clone(doc), w = next.watches.find(x => x.id === id); if (!w) return { ok: false, error: '감시 없음' };
      w.intervalMs = +ms;
      const r = commit(next, 'watch-defs');
      if (r.ok && wrt.get(id) && wrt.get(id).running) { const rt = wrt.get(id); if (rt.timer) clock.clear(rt.timer); rt.timer = clock.set(() => tick(id), w.intervalMs); }
      return r;
    }
    function deleteWatch(id) {
      stopWatch(id, '삭제');
      const next = clone(doc); next.watches = next.watches.filter(x => x.id !== id);
      wrt.delete(id);
      return commit(next, 'watch-defs');
    }
    function saveSequence(draft) {
      const next = clone(doc), isNew = !draft.id || draft.id === 'new';
      const id = isNew ? nextId(doc, 's') : draft.id;
      if (isNew && doc.sequences.length >= LIM.sequences) return { ok: false, error: `시퀀스 최대 ${LIM.sequences}개` };
      if (!isNew && runs.get(id) && runs.get(id).status === 'running') return { ok: false, error: '실행 중인 시퀀스는 저장할 수 없음 — 먼저 중지' };
      const s = { id, name: draft.name, target: draft.target || null, loops: +draft.loops, stopOnError: !!draft.stopOnError, steps: (draft.steps || []).map(st => Object.assign({}, st, { delayMs: +st.delayMs })) };
      const one = vSequence(s, 0, new Set(doc.commands.map(c => c.id)));
      if (one.error) return { ok: false, error: one.error.replace(/^시퀀스 #1:? ?/, '') };
      if (next.sequences.some(x => x.id !== id && x.name === one.v.name)) return { ok: false, error: '같은 이름의 시퀀스가 있음' };
      const i = next.sequences.findIndex(x => x.id === id);
      if (!isNew && i < 0) return { ok: false, error: '편집 대상 시퀀스가 없음 (삭제됨)' };
      if (i >= 0) next.sequences[i] = one.v; else next.sequences.push(one.v);
      const r = commit(next);
      return r.ok ? { ok: true, id } : r;
    }
    function duplicateSequence(id) {
      const s = doc.sequences.find(x => x.id === id); if (!s) return { ok: false, error: '시퀀스 없음' };
      return saveSequence(Object.assign(clone(s), { id: 'new', name: uniqueName(doc.sequences, s.name + ' 복사') }));
    }
    function deleteSequence(id) {
      if (runs.get(id) && runs.get(id).status === 'running') return { ok: false, error: '실행 중 — 먼저 중지' };
      const next = clone(doc); next.sequences = next.sequences.filter(x => x.id !== id); runs.delete(id);
      return commit(next);
    }
    function setPrefs(p) { doc.prefs = vPrefs(Object.assign({}, doc.prefs, p)); persist(); }

    /* ---------- commands: compose / run ---------- */
    const reqOf = c => (c.mode === 'did' ? { mode: 'did', didText: c.didText } : { mode: 'raw', rawText: c.rawText });
    function loadToEditor(c) { H.compose(reqOf(c)); return { ok: true }; }   // never sends
    function runCommand(id) {
      const c = doc.commands.find(x => x.id === id); if (!c) return { ok: false, error: '명령 없음' };
      const t = c.target || curCtx();
      if (!ecuExists(t.ecu)) { const r = { ok: false, error: `대상 ECU ${t.ecu} 없음 (삭제됨)` }; cmdLast.set(id, r); emit('cmd'); return r; }
      // Explicit human run: select the new row only when it runs in the CURRENT context (never switches target for a bound other ECU).
      const here = H.context() || {}, same = here.ecu === t.ecu && here.transport === t.transport;
      const res = H.send(Object.assign({ ecu: t.ecu, transport: t.transport, source: 'command', select: same }, reqOf(c)));
      const r = res.ok ? { ok: true, txId: res.txn.id, no: res.txn.no, target: t } : { ok: false, error: res.error, code: res.code };
      if (res.ok) trackOwned(res.txn, 'command', id);
      cmdLast.set(id, r); emit('cmd');
      return r;
    }
    // Editor state for "save current": host accessor if provided (R1), else read-only legacy state.
    function editorState() {
      if (typeof H.editor === 'function') return H.editor();
      const st = root.loomUdsOpus && root.loomUdsOpus.st;
      return st ? { mode: st.mode, didText: st.didText, rawText: st.rawText } : null;
    }

    /* ---------- recent (runtime only, from engine history via subscribe) ---------- */
    function noteRecent(tx) {
      if (!tx || !tx.request) return;
      const textOf = tx.kind === 'did' ? tx.didHex : U().hexs(tx.request);
      const sig = [tx.ecu, tx.transport, tx.kind, textOf].join('|');
      const i = recent.findIndex(r => r.sig === sig);
      if (i >= 0 && recent[i].txId > tx.id) return;          // older txn event never displaces newer
      if (i >= 0) recent.splice(i, 1);
      recent.unshift({ sig, txId: tx.id, no: tx.no, ecu: tx.ecu, transport: tx.transport, mode: tx.kind, text: textOf, status: tx.status, t: tx.t0, source: tx.source || '' });
      if (recent.length > LIM.recent) recent.length = LIM.recent;
    }
    function seedRecent() { (E.txns || []).forEach(noteRecent); }

    /* ---------- watch ---------- */
    function rt(id) {
      if (!wrt.has(id)) wrt.set(id, { running: false, timer: null, inflight: null, deferred: false, value: null, unit: '', valueT: null, src: null, status: 'idle', note: '', min: null, max: null, samples: [], sends: [], sent: 0, skips: { busy: 0, sequence: 0 }, numeric: true });
      return wrt.get(id);
    }
    function startWatch(id) {
      const w = doc.watches.find(x => x.id === id); if (!w) return { ok: false, error: '감시 없음' };
      const r = rt(id); if (r.running) return { ok: true };
      if (!ecuExists(w.target.ecu)) { r.status = 'error'; r.note = `ECU ${w.target.ecu} 없음`; emit('watch'); return { ok: false, error: r.note }; }
      if (ctxOf(w.target).link !== 'online') { r.status = 'offline'; r.note = '경로 연결 후 시작 (자동 연결 없음)'; emit('watch'); return { ok: false, error: r.note, code: 'offline' }; }
      Object.assign(r, { running: true, sends: [], sent: 0, skips: { busy: 0, sequence: 0 }, status: 'running', note: '', deferred: false });
      attempt(id);                                            // explicit start = first poll now
      if (r.running) r.timer = clock.set(() => tick(id), w.intervalMs);
      armAge(); emit('watch');
      return { ok: true };
    }
    function stopWatch(id, why) {
      const r = wrt.get(id); if (!r || !r.running) return { ok: false };
      r.running = false; r.deferred = false;
      if (r.timer) clock.clear(r.timer); r.timer = null;
      if (why) r.note = why;
      if (r.status === 'running' || r.status === 'ok' || r.status === 'skip') r.status = 'stopped';
      // in-flight owned request (if any) completes normally and still updates this row; never cancels anything
      emit('watch');
      return { ok: true };
    }
    const stopAllWatches = why => doc.watches.forEach(w => stopWatch(w.id, why));
    function tick(id) {
      const w = doc.watches.find(x => x.id === id), r = wrt.get(id);
      if (!w || !r || !r.running) return;
      r.timer = clock.set(() => tick(id), w.intervalMs);      // fixed-rate slot; a busy slot is skipped, never queued
      attempt(id);
      emit('watch-rt');
    }
    function attempt(id) {
      const w = doc.watches.find(x => x.id === id), r = wrt.get(id);
      const k = ckey(w.target), c = ctxOf(w.target);
      if (c.link !== 'online') { r.status = 'offline'; stopWatch(id, '링크 오프라인 — 중지됨 (재연결 시 자동 재개 없음)'); r.status = 'offline'; return; }
      if (seqOnCtx(k)) { r.skips.sequence++; r.note = '시퀀스 실행 중 — 건너뜀'; return; }
      if (r.inflight != null) { r.skips.busy++; r.note = '이전 감시 요청 진행 중 (주기 < 응답 시간) — 건너뜀'; return; }
      if (c.active != null) {
        const o = owned.get(c.active);
        if (o && o.kind === 'watch' && !r.deferred) {           // another watch row on this context: one bounded deferral
          r.deferred = true;
          const q = deferred.get(k) || []; if (!q.includes(id)) q.push(id); deferred.set(k, q);
          r.note = '같은 경로의 다른 감시 대기';
          return;
        }
        r.skips.busy++; r.note = o ? '경로 사용 중 — 건너뜀' : '수동/다른 요청 진행 중 — 건너뜀 (취소하지 않음)';
        return;
      }
      send(id);
    }
    function send(id) {
      const w = doc.watches.find(x => x.id === id), r = wrt.get(id);
      r.deferred = false;
      const res = H.send({ ecu: w.target.ecu, transport: w.target.transport, mode: 'did', didText: w.did, source: 'watch' });
      if (!res.ok) {
        if (res.code === 'offline') { stopWatch(id, '링크 오프라인 — 중지됨'); r.status = 'offline'; return; }
        if (res.code === 'busy') { r.skips.busy++; r.note = '경로 사용 중 — 건너뜀'; return; }
        r.status = 'error'; r.note = res.error || '전송 거부'; stopWatch(id, r.note); r.status = 'error'; return;
      }
      trackOwned(res.txn, 'watch', id);
      r.inflight = res.txn.id; r.sent++; r.sends.push(now()); if (r.sends.length > 12) r.sends.shift();
      r.note = '';
    }
    function confirmedFor(tx) {
      for (const v of E.confirmed.values()) if (v.txnId === tx.id) return v;
      return null;
    }
    function watchDone(id, tx) {
      const r = wrt.get(id); if (!r) return;
      if (r.inflight === tx.id) r.inflight = null;
      r.src = { no: tx.no, id: tx.id, status: tx.status, t: tx.tEnd };
      if (tx.status === 'positive') {
        const cf = tx.applied ? confirmedFor(tx) : null;
        if (cf) {
          r.value = cf.value; r.unit = cf.unit || ''; r.valueT = cf.t; r.valueNo = tx.no;
          const n = Number(cf.value);
          if (cf.value !== '' && Number.isFinite(n)) {
            r.samples.push(n); if (r.samples.length > LIM.samples) r.samples.shift();
            r.min = r.min == null ? n : Math.min(r.min, n); r.max = r.max == null ? n : Math.max(r.max, n);
          } else r.numeric = false;
          r.status = r.running ? 'ok' : 'stopped'; r.note = '';
        } else { r.status = 'stale'; r.note = tx.appliedNote || '확정 값 반영 안 됨 — 이전 값 유지'; }
      } else if (tx.status === 'nrc') { r.status = 'nrc'; r.note = `NRC 0x${U().hex2(tx.nrc)} — 이전 값 유지`; }
      else if (tx.status === 'timeout') { r.status = 'timeout'; r.note = '타임아웃 — 이전 값 유지'; }
      else if (tx.status === 'cancelled') { r.status = 'cancelled'; r.note = '취소됨 — 이전 값 유지'; }
      else if (tx.status === 'link-lost') { stopWatch(id, '링크 끊김 — 중지됨 (자동 재개 없음)'); r.status = 'link-lost'; }
      if (!r.running && r.status === 'ok') r.status = 'stopped';
      runDeferred(ckey(tx.route ? { ecu: tx.ecu, transport: tx.transport } : { ecu: tx.ecu, transport: tx.transport }));
    }
    function runDeferred(k) {
      const q = deferred.get(k); if (!q || !q.length) return;
      const id = q.shift(); if (!q.length) deferred.delete(k);
      clock.set(() => {
        const r = wrt.get(id), w = doc.watches.find(x => x.id === id);
        if (!r || !w || !r.running || !r.deferred) return;
        const c = ctxOf(w.target);
        if (c.link === 'online' && c.active == null && r.inflight == null && !seqOnCtx(k)) send(id);
        else { r.deferred = false; r.skips.busy++; }
        emit('watch-rt');
      }, GAP_MS);
    }
    function cadence(r) {
      if (r.sends.length < 2) return null;
      return Math.round((r.sends[r.sends.length - 1] - r.sends[0]) / (r.sends.length - 1));
    }
    function armAge() {
      if (ageTimer || disposed) return;
      const any = () => [...wrt.values()].some(r => r.running);
      if (!any()) return;
      ageTimer = clock.set(function ageTick() { ageTimer = null; emit('age'); if (any() && !disposed) ageTimer = clock.set(ageTick, 1000); }, 1000);
    }

    /* ---------- sequences ---------- */
    function startSequence(id) {
      const s = doc.sequences.find(x => x.id === id); if (!s) return { ok: false, error: '시퀀스 없음' };
      const prev = runs.get(id); if (prev && prev.status === 'running') return { ok: false, error: '이미 실행 중' };
      const t = s.target ? clone(s.target) : curCtx();
      const fail = msg => { runs.set(id, { id, runId: ++runSeq, name: s.name, target: t, status: 'refused', results: [], steps: [], loops: s.loops, loop: 0, idx: 0, error: msg, startedAt: now() }); emit('seq'); return { ok: false, error: msg }; };
      if (!ecuExists(t.ecu)) return fail(`대상 ECU ${t.ecu} 없음`);
      if (ctxOf(t).link !== 'online') return fail('경로 연결 후 실행 (자동 연결 없음)');
      if (seqOnCtx(ckey(t))) return fail('이 경로에서 다른 시퀀스 실행 중');
      const steps = [];
      for (const st of s.steps) {
        if (st.mode === 'cmd') {
          const c = doc.commands.find(x => x.id === st.cmdId);
          if (!c) return fail(`명령 ${st.cmdId} 없음`);
          steps.push(Object.assign({ label: `명령 "${c.name}"` }, reqOf(c), { delayMs: st.delayMs, expect: clone(st.expect) }));
        } else steps.push(Object.assign({ label: st.mode === 'did' ? `DID ${st.didText}` : `Raw ${st.rawText}` }, clone(st)));
      }
      const run = { id, runId: ++runSeq, name: s.name, target: t, steps, loops: s.loops, stopOnError: s.stopOnError, loop: 0, idx: 0, status: 'running', phase: 'delay', results: [], failures: 0, txnId: null, timer: null, startedAt: now(), endedAt: null, error: '' };
      runs.set(id, run);
      schedStep(run, steps[0].delayMs);
      emit('seq');
      return { ok: true, runId: run.runId, target: t };
    }
    function alive(run) { const cur = runs.get(run.id); return cur === run && run.status === 'running'; }
    function schedStep(run, ms) {
      run.phase = 'delay'; run.dueAt = now() + ms;
      run.timer = clock.set(() => { run.timer = null; if (alive(run)) stepSend(run); }, ms);
    }
    function stepSend(run) {
      const st = run.steps[run.idx], c = ctxOf(run.target);
      if (c.link !== 'online') return endRun(run, 'failed', '링크 오프라인');
      if (c.active != null) {
        const o = owned.get(c.active);
        if (o && o.kind === 'watch') { run.phase = 'wait-watch'; run.waitTx = c.active; emit('seq'); return; } // resumes on that txn's completion
        return stepResult(run, null, { ok: false, note: '경로 사용 중 (수동/다른 요청) — 취소하지 않음' }, 'busy');
      }
      const res = H.send(Object.assign({ ecu: run.target.ecu, transport: run.target.transport, source: 'sequence' }, st.mode === 'did' ? { mode: 'did', didText: st.didText } : { mode: 'raw', rawText: st.rawText }));
      if (!res.ok) return stepResult(run, null, { ok: false, note: res.error || '전송 거부' }, res.code || 'error');
      trackOwned(res.txn, 'seq', run.id);
      run.txnId = res.txn.id; run.phase = 'await'; run.sentAt = now();
      emit('seq');
    }
    function stepResult(run, tx, ev, code) {
      const st = run.steps[run.idx];
      run.results.push({ loop: run.loop, idx: run.idx, label: st.label, txId: tx ? tx.id : null, no: tx ? tx.no : null, status: tx ? tx.status : code, final: tx && tx.final ? U().hexs(tx.final.slice(0, 16)) + (tx.final.length > 16 ? ' …' : '') : '', ok: ev.ok, note: ev.note, t: now() });
      if (run.results.length > LIM.results) run.results.shift();
      run.txnId = null;
      if (!ev.ok) { run.failures++; if (run.stopOnError) return endRun(run, 'failed', `단계 ${run.idx + 1} 실패: ${ev.note}`); }
      run.idx++;
      if (run.idx >= run.steps.length) { run.idx = 0; run.loop++; }
      if (run.loop >= run.loops) return endRun(run, run.failures ? 'done-errors' : 'passed', run.failures ? `실패 ${run.failures}건 (계속 실행 설정)` : '');
      schedStep(run, Math.max(GAP_MS, run.steps[run.idx].delayMs));
      emit('seq');
    }
    function endRun(run, status, msg) {
      run.status = status; run.error = msg || ''; run.endedAt = now(); run.phase = 'end';
      if (run.timer) clock.clear(run.timer); run.timer = null;
      emit('seq');
    }
    function stopSequence(id) {
      const run = runs.get(id); if (!run || run.status !== 'running') return { ok: false, error: '실행 중 아님' };
      let cancelled = false;
      if (run.txnId != null) {
        const c = ctxOf(run.target), o = owned.get(run.txnId);
        if (c.active === run.txnId && o && o.kind === 'seq' && o.ref === id) {
          const r = typeof H.cancelOwned === 'function' ? H.cancelOwned(run.txnId, 'sequence') : (E.cancel(run.target.ecu, run.target.transport));
          cancelled = !!(r && r.ok);
        }
      }
      endRun(run, 'stopped', cancelled ? '사용자 중지 — 진행 중 시퀀스 요청 취소' : '사용자 중지');
      return { ok: true, cancelled };
    }
    function seqTxnDone(id, tx) {
      const run = runs.get(id);
      if (!run || run.status !== 'running' || run.txnId !== tx.id) return;  // stopped/old run: result ignored
      stepResult(run, tx, expectOk(run.steps[run.idx].expect, tx));
    }
    function resumeWaiting(txId) {
      runs.forEach(run => { if (alive(run) && run.phase === 'wait-watch' && run.waitTx === txId) { run.waitTx = null; schedStep(run, GAP_MS); } });
    }
    const stopAllSequences = () => runs.forEach(r => { if (r.status === 'running') stopSequence(r.id); });
    const anyRunning = () => [...wrt.values()].some(r => r.running) || [...runs.values()].some(r => r.status === 'running');

    /* ---------- engine events ---------- */
    function onHost(ev) {
      if (disposed) return;
      if (ev && ev.txn) noteRecent(ev.txn);
      let changed = !!(ev && ev.txn);
      for (const [id, o] of [...owned]) {
        if (!E.terminal(o.tx.status)) continue;
        owned.delete(id);
        if (o.kind === 'watch') watchDone(o.ref, o.tx);
        else if (o.kind === 'seq') seqTxnDone(o.ref, o.tx);
        resumeWaiting(id);
        changed = true;
      }
      // link loss while idle: running watches stop (no auto-resume on reconnect)
      doc.watches.forEach(w => { const r = wrt.get(w.id); if (r && r.running && r.inflight == null && ctxOf(w.target).link !== 'online') { stopWatch(w.id, '링크 오프라인 — 중지됨 (재연결 시 자동 재개 없음)'); r.status = 'offline'; changed = true; } });
      if (changed) emit('host');
    }
    const unsub = H.subscribe(onHost);
    seedRecent();

    /* ---------- import / export (definitions only) ---------- */
    let undoDoc = null;
    function exportDoc() { return clone(doc); }
    function importDoc(json, how) {
      if (anyRunning()) return { ok: false, error: '실행 중인 감시/시퀀스가 있음 — 먼저 중지 (가져오기는 실행을 시작하지 않음)' };
      const r = validate(json); if (!r.ok) return r;
      undoDoc = clone(doc);
      doc = r.value; wrt.clear(); runs.clear(); cmdLast.clear(); persist(); emit('defs');
      return { ok: true, counts: { commands: doc.commands.length, watches: doc.watches.length, sequences: doc.sequences.length }, how };
    }
    function undoImport() { if (!undoDoc || anyRunning()) return { ok: false }; doc = undoDoc; undoDoc = null; wrt.clear(); runs.clear(); persist(); emit('defs'); return { ok: true }; }
    // Project section: pure validate, import replaces definitions after stopping owned automations; nothing restarts.
    if (H.project && typeof H.project.register === 'function') {
      H.project.register('workflow', {
        export: () => { const x = clone(doc); return x; },
        validate: json => validate(json),
        defaults: () => emptyDoc(),          // project coordinator: fresh default for a section absent from a file
        // Stop-first: the coordinator guarantees activities are already stopped. Never implicitly stops jobs; throws if any run (→ coordinator rollback).
        import: value => {
          if (anyRunning()) throw new Error('workflow: 실행 중인 감시/시퀀스 — 먼저 중지 필요');
          const r = validate(value); if (!r.ok) throw new Error('workflow: ' + r.error);
          const prev = doc;
          doc = r.value;
          // storage failure must THROW (coordinator rolls back); memory reverts first so this section stays self-consistent
          if (storage && !persist()) { doc = prev; emit('defs'); throw new Error('workflow: 저장소 기록 실패 (용량/권한) — 정의 변경 안 됨'); }
          wrt.clear(); runs.clear(); cmdLast.clear(); emit('defs');
        },
      });
    }

    // Activity registry: every RUNNING watch/sequence (offline-but-running included) so core can guard ECU delete / project import.
    let unAct = null;
    if (H.activity && typeof H.activity.register === 'function') {
      unAct = H.activity.register('workflow', {
        list: () => {
          const out = [];
          doc.watches.forEach(w => { const r = wrt.get(w.id); if (r && r.running) out.push({ ecu: w.target.ecu, transport: w.target.transport, kind: 'watch', label: `감시 DID ${w.did}` }); });
          runs.forEach(r => { if (r.status === 'running') out.push({ ecu: r.target.ecu, transport: r.target.transport, kind: 'sequence', label: `시퀀스 "${r.name}"` }); });
          return out;
        },
        stop: () => { stopAllWatches('사용자 중지 (활동 목록)'); stopAllSequences(); },
      });
    }

    function dispose() {
      if (typeof unAct === 'function') unAct();
      disposed = true; stopAllWatches(); stopAllSequences();
      if (ageTimer) clock.clear(ageTimer); ageTimer = null;
      if (typeof unsub === 'function') unsub();
      listeners.length = 0;
    }

    return {
      get doc() { return doc; }, get notice() { return notice; }, set notice(v) { notice = v; }, get storeErr() { return storeErr; },
      on: fn => { listeners.push(fn); return () => { const i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1); }; },
      saveCommand, duplicateCommand, deleteCommand, loadToEditor, runCommand, editorState, cmdLast,
      recent, addWatch, setWatchInterval, deleteWatch, startWatch, stopWatch, stopAllWatches, wrt: id => wrt.get(id) || rt(id), cadence,
      saveSequence, duplicateSequence, deleteSequence, startSequence, stopSequence, stopAllSequences, runs,
      setPrefs, exportDoc, importDoc, undoImport, canUndo: () => !!undoDoc, anyRunning, routeTag, ecuExists, curCtx, owned,
      dispose, H,
    };
  }


  /* =================== UI (owned subtrees inside #workflowMount / #watchMount only) =================== */
  function mountUI(wf, H) {
    const d = root.document, E = H.engine, UU = H.constants;
    const esc = (H.ui && H.ui.esc) || (s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]));
    const toast = (m, k) => { if (H.ui && H.ui.toast) H.ui.toast(m, k); };
    const mW = d.getElementById('workflowMount'), mT = d.getElementById('watchMount');
    if (!mW && !mT) return null;
    const q = (sel, el) => (el || d).querySelector(sel);
    const trLabel = id => (UU.TRANSPORTS.find(t => t.id === id) || {}).label || id;
    const tgtText = t => (t ? `${t.ecu}·${trLabel(t.transport)}` : '현재 대상');
    const ST = { waiting: '대기', pending: '대기 0x78', positive: '긍정', nrc: 'NRC', timeout: '타임아웃', cancelled: '취소', 'link-lost': '링크 끊김', suppressed: '응답 억제' };
    const stTxt = s => ST[s] || s;
    const pduOf = c => (c.mode === 'did' ? `22 ${c.didText.slice(0, 2)} ${c.didText.slice(2)}` : c.rawText);
    // repaint a list region; keeps focus on the element with the same data-k (lists only — editors are never repainted by events)
    function paint(el, html) {
      if (!el) return;
      const a = d.activeElement, k = a && el.contains(a) ? a.getAttribute('data-k') : null;
      el.innerHTML = html;
      if (k) { const n = el.querySelector(`[data-k="${k}"]`); if (n) n.focus({ preventScroll: true }); }
    }
    function targetOptions(sel, withCurrent) {
      const opts = (withCurrent ? [`<option value="">${withCurrent}</option>`] : [])
        .concat((UU.ECUS || []).map(e => UU.TRANSPORTS.map(t => `<option value="${esc(e.id)}|${t.id}">${esc(e.id)} · ${esc(t.label)}</option>`).join('')));
      return opts.join('');
    }
    const parseTarget = v => (v ? { ecu: v.split('|')[0], transport: v.split('|')[1] } : null);
    const tval = t => (t ? `${t.ecu}|${t.transport}` : '');
    const ecuSig = () => (UU.ECUS || []).map(e => e.id).join(',');
    let lastEcuSig = ecuSig();

    /* ---------------- library (#workflowMount) ---------------- */
    const drafts = new Map();              // command id|'new' -> unsaved draft (kept while editing another)
    let editing = null;                    // command id being edited, or null
    let seqDraft = null, seqSel = null;    // sequence editor draft / selected sequence for run view
    let fileMsg = '';
    if (mW) {
      mW.innerHTML =
        `<div class="wf" id="wf">` +
        `<div class="wf-bar"><span class="seg sm" role="tablist" aria-label="워크플로 라이브러리" id="wfTabs">` +
        `<button type="button" role="tab" id="wft-commands" data-wtab="commands" aria-controls="wfp-commands">명령 <b class="ct" id="wfcCmd"></b></button>` +
        `<button type="button" role="tab" id="wft-recent" data-wtab="recent" aria-controls="wfp-recent">최근 요청 <b class="ct" id="wfcRecent"></b></button>` +
        `<button type="button" role="tab" id="wft-sequences" data-wtab="sequences" aria-controls="wfp-sequences">시퀀스 <b class="ct" id="wfcSeq"></b></button></span>` +
        `<span class="grow"></span>` +
        `<button type="button" class="btn" data-wf="save-current" title="요청 편집기의 현재 DID/Raw를 새 명령 초안으로 (전송 없음)">현재 요청 저장</button>` +
        `<button type="button" class="btn ghost" data-wf="export" title="명령·감시·시퀀스 정의 JSON 다운로드 (실행 상태 제외)">내보내기</button>` +
        `<button type="button" class="btn ghost" data-wf="import" title="정의 JSON 가져오기 — 전체 검증 후 교체, 실행 시작 안 함">가져오기</button>` +
        `<input type="file" id="wfFile" accept=".json,application/json" hidden></div>` +
        `<div class="wf-note" id="wfNote" role="status" aria-live="polite"></div>` +
        `<section class="wf-panel" role="tabpanel" id="wfp-commands" aria-labelledby="wft-commands">` +
        `<div class="wf-tools"><label class="flab" for="wfFolder">폴더</label><select class="fld" id="wfFolder"></select>` +
        `<input class="fld wf-search" id="wfSearch" type="search" placeholder="이름·설명·hex 검색" aria-label="명령 검색" autocomplete="off" spellcheck="false">` +
        `<span class="grow"></span><button type="button" class="btn" data-wf="cmd-new">새 명령</button></div>` +
        `<ul class="wf-list" id="wfCmdList" aria-label="저장 명령"></ul>` +
        `<form class="wf-edit" id="wfCmdEdit" novalidate autocomplete="off" hidden></form></section>` +
        `<section class="wf-panel" role="tabpanel" id="wfp-recent" aria-labelledby="wft-recent" hidden>` +
        `<p class="wf-hint">이번 세션의 최근 고유 요청 ${LIM.recent}개 (기록 200개 제거와 무관, 저장·가져오기 안 됨). [편집기로]는 입력만 채우며 전송하지 않습니다 — 대상·경로는 현재 선택 유지.</p>` +
        `<ul class="wf-list" id="wfRecent" aria-label="최근 요청"></ul></section>` +
        `<section class="wf-panel" role="tabpanel" id="wfp-sequences" aria-labelledby="wft-sequences" hidden>` +
        `<div class="wf-tools"><span class="wf-hint">단계는 하나씩 응답을 기다린 뒤 진행 · 실행 대상은 시작 시 고정</span><span class="grow"></span><button type="button" class="btn" data-wf="seq-new">새 시퀀스</button></div>` +
        `<ul class="wf-list" id="wfSeqList" aria-label="시퀀스"></ul>` +
        `<form class="wf-edit" id="wfSeqEdit" novalidate autocomplete="off" hidden></form>` +
        `<div class="wf-run" id="wfRun" aria-live="polite"></div></section></div>`;
    }
    function setTab(t, focus) {
      if (wf.doc.prefs.tab !== t) wf.setPrefs({ tab: t });
      d.querySelectorAll('#wfTabs [data-wtab]').forEach(b => { const on = b.dataset.wtab === t; b.setAttribute('aria-selected', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1; });
      TABS.forEach(k => { const p = d.getElementById('wfp-' + k); if (p) p.hidden = k !== t; });
      if (focus) d.getElementById('wft-' + t).focus();
      renderLib();
    }
    function note(msg, kind, undo) {
      const n = d.getElementById('wfNote'); if (!n) return;
      n.className = 'wf-note' + (kind ? ' ' + kind : '');
      n.innerHTML = msg ? esc(msg) + (undo ? ' <button type="button" class="btn sm" data-wf="undo-import">되돌리기</button>' : '') : '';
    }
    function folders() { return [...new Set(wf.doc.commands.map(c => c.folder).filter(Boolean))].sort(); }
    function cmdStatus(c) {
      const l = wf.cmdLast.get(c.id); if (!l) return '';
      if (!l.ok) return `<span class="wf-st bad">${esc(l.error)}</span>`;
      const tx = E.byId(l.txId);
      return `<button type="button" class="wf-st lnk s-${tx ? tx.status : 'gone'}" data-wf="goto" data-tx="${l.txId}" title="기록에서 보기">#${l.no} ${tx ? stTxt(tx.status) : '기록에서 제거됨'} · ${esc(tgtText(l.target))}</button>`;
    }
    function renderCmdList() {
      const sel = d.getElementById('wfFolder'); if (!sel) return;
      const fs = folders(), f = wf.doc.prefs.folder;
      const want = `<option value="">전체 (${wf.doc.commands.length})</option>` + fs.map(x => `<option value="${esc(x)}">${esc(x)}</option>`).join('');
      if (sel.dataset.sig !== want) { sel.innerHTML = want; sel.dataset.sig = want; }
      sel.value = fs.includes(f) ? f : '';
      const s = (d.getElementById('wfSearch').value || '').trim().toLowerCase().replace(/\s/g, '');
      const list = wf.doc.commands.filter(c => (!sel.value || c.folder === sel.value) && (!s || (c.name + c.desc + c.folder + pduOf(c)).toLowerCase().replace(/\s/g, '').includes(s)));
      paint(d.getElementById('wfCmdList'), list.length ? list.map(c => {
        const live = c.target ? wf.routeTag(c.target) : '';
        const missing = c.target && !wf.ecuExists(c.target.ecu);
        return `<li class="wf-item${editing === c.id ? ' editing' : ''}" data-cid="${c.id}">` +
          `<div class="wf-main"><b class="wf-name">${esc(c.name)}</b>${c.folder ? `<span class="wf-chip">${esc(c.folder)}</span>` : ''}` +
          `<span class="wf-kind k-${c.mode}">${c.mode === 'did' ? 'DID' : 'RAW'}</span><code class="wf-pdu" title="${esc(pduOf(c))}">${esc(pduOf(c))}</code>` +
          `<span class="wf-tgt${missing ? ' bad' : ''}" title="${c.target ? esc('고정 대상 — 실행 시 ' + live) : '실행 시점의 현재 대상·경로·프로필 사용'}">${esc(tgtText(c.target))}${missing ? ' (없음)' : ''}</span>` +
          (c.desc ? `<span class="wf-desc">${esc(c.desc)}</span>` : '') + cmdStatus(c) + `</div>` +
          `<div class="wf-acts"><button type="button" class="btn sm" data-wf="cmd-load" data-k="load-${c.id}" title="요청 편집기에 채우기 (전송 없음)">편집기로</button>` +
          `<button type="button" class="btn sm primary" data-wf="cmd-run" data-k="run-${c.id}" title="${esc(c.target ? '고정 대상 ' + tgtText(c.target) : '현재 대상')}으로 1회 전송">실행</button>` +
          `<button type="button" class="ibtn" data-wf="cmd-edit" data-k="edit-${c.id}" title="편집" aria-label="${esc(c.name)} 편집">✎</button>` +
          `<button type="button" class="ibtn" data-wf="cmd-dup" data-k="dup-${c.id}" title="복제" aria-label="${esc(c.name)} 복제">⧉</button>` +
          `<button type="button" class="ibtn danger" data-wf="cmd-del" data-k="del-${c.id}" title="삭제" aria-label="${esc(c.name)} 삭제">✕</button></div></li>`;
      }).join('') : `<li class="wf-empty">${wf.doc.commands.length ? '조건에 맞는 명령 없음' : '저장된 명령 없음 — [현재 요청 저장] 또는 [새 명령]'}</li>`);
    }
    /* command editor (rebuilt only when switching the edited item) */
    function cmdDraftFrom(c) { return { id: c.id, name: c.name, desc: c.desc, folder: c.folder, mode: c.mode, didText: c.didText || '', rawText: c.rawText || '', target: c.target ? Object.assign({}, c.target) : null }; }
    function readCmdForm() {
      const f = d.getElementById('wfCmdEdit'); if (!f || f.hidden || editing == null) return null;
      const dr = drafts.get(editing) || {};
      dr.name = q('#wfeName', f).value; dr.folder = q('#wfeFolder', f).value; dr.desc = q('#wfeDesc', f).value;
      dr.didText = q('#wfeDid', f).value; dr.rawText = q('#wfeRaw', f).value; dr.target = parseTarget(q('#wfeTarget', f).value);
      drafts.set(editing, dr); return dr;
    }
    function openCmdEditor(id, draft) {
      readCmdForm();                                   // keep the previous item's unsaved input
      editing = id;
      if (draft) drafts.set(id, draft);
      if (!drafts.has(id)) { const c = wf.doc.commands.find(x => x.id === id); if (!c) { editing = null; return; } drafts.set(id, cmdDraftFrom(c)); }
      const dr = drafts.get(id), f = d.getElementById('wfCmdEdit');
      f.hidden = false;
      f.innerHTML =
        `<div class="wf-eh"><b>${id === 'new' ? '새 명령' : '명령 편집'}</b><span class="wf-hint">저장 전 초안은 다른 명령을 편집해도 유지 · 요청 편집기 입력은 바꾸지 않음</span></div>` +
        `<div class="wf-grid"><label>이름<input class="fld" id="wfeName" maxlength="${LIM.name}" value="${esc(dr.name)}" required></label>` +
        `<label>폴더<input class="fld" id="wfeFolder" maxlength="${LIM.folder}" list="wfeFolders" value="${esc(dr.folder)}" placeholder="(없음)"></label><datalist id="wfeFolders">${folders().map(x => `<option value="${esc(x)}">`).join('')}</datalist>` +
        `<label class="wide">설명<input class="fld" id="wfeDesc" maxlength="${LIM.desc}" value="${esc(dr.desc)}" placeholder="선택"></label>` +
        `<span class="wf-lab">요청<span class="seg sm" id="wfeMode" role="radiogroup" aria-label="명령 요청 모드"><button type="button" role="radio" data-m="did">DID</button><button type="button" role="radio" data-m="raw">Raw</button></span></span>` +
        `<label class="m-did">DID (hex 4자리)<input class="fld" id="wfeDid" maxlength="12" value="${esc(dr.didText)}" placeholder="F190" spellcheck="false"></label>` +
        `<label class="m-raw wide">Raw PDU (SID 포함 그대로)<textarea class="fld" id="wfeRaw" rows="2" spellcheck="false" placeholder="22 F1 90">${esc(dr.rawText)}</textarea></label>` +
        `<label class="wide">대상 바인딩<select class="fld" id="wfeTarget">${targetOptions(null, '현재 대상 (실행 시점 선택 사용)')}</select></label></div>` +
        `<div class="wf-prev" id="wfePrev" aria-live="polite"></div>` +
        `<div class="wf-err" id="wfeErr" role="alert"></div>` +
        `<div class="wf-eb"><button type="submit" class="btn primary">저장</button><button type="button" class="btn" data-wf="cmd-cancel">취소</button></div>`;
      q('#wfeTarget', f).value = tval(dr.target);
      if (q('#wfeTarget', f).value !== tval(dr.target)) { const o = d.createElement('option'); o.value = tval(dr.target); o.textContent = `${tgtText(dr.target)} (없음)`; q('#wfeTarget', f).appendChild(o); q('#wfeTarget', f).value = tval(dr.target); }
      cmdMode(dr.mode || 'did');
      renderCmdList();
      q('#wfeName', f).focus({ preventScroll: true });
    }
    function cmdMode(m) {
      const f = d.getElementById('wfCmdEdit'), dr = drafts.get(editing); if (!dr) return;
      dr.mode = m; f.classList.toggle('mode-raw', m === 'raw'); f.classList.toggle('mode-did', m === 'did');
      f.querySelectorAll('#wfeMode [data-m]').forEach(b => { const on = b.dataset.m === m; b.setAttribute('aria-checked', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1; });
      cmdPreview();
    }
    function cmdPreview() {
      const f = d.getElementById('wfCmdEdit'), dr = readCmdForm(); if (!dr) return;
      const p = q('#wfePrev', f);
      let r;
      if (dr.mode === 'did') { const v = UU.validateDid(dr.didText); r = v.ok ? { ok: true, t: `22 ${v.norm.slice(0, 2)} ${v.norm.slice(2)} · 3 B` } : { ok: false, t: v.error }; }
      else { const v = UU.parseRawHex(dr.rawText); r = v.ok ? { ok: true, t: `${v.len > 24 ? UU.hexs(v.bytes.slice(0, 24)) + ' …' : v.norm} · ${v.len} B · SID 0x${UU.hex2(v.sid)} ${UU.sidName(v.sid) || ''}` } : { ok: false, t: v.error }; }
      const t = dr.target || wf.curCtx();
      p.className = 'wf-prev' + (r.ok ? '' : ' bad');
      p.innerHTML = `<span class="k">전송될 PDU</span> <code>${esc(r.t)}</code> <span class="k">실행 대상</span> ${esc(tgtText(t))} · ${esc(wf.routeTag(t))}${dr.target ? '' : ' (지금 기준, 실행 시점에 다시 결정)'}`;
    }
    function closeCmdEditor(discard) {
      if (discard && editing != null) drafts.delete(editing);
      editing = null; const f = d.getElementById('wfCmdEdit'); f.hidden = true; f.innerHTML = ''; renderCmdList();
    }
    function saveCmd() {
      const dr = readCmdForm(); if (!dr) return;
      const r = wf.saveCommand(Object.assign({}, dr, { id: editing }));
      if (!r.ok) { q('#wfeErr').textContent = r.error; return; }
      drafts.delete(editing); closeCmdEditor(); note(`명령 저장됨`, 'ok');
      const li = d.querySelector(`[data-cid="${r.id}"] [data-wf="cmd-edit"]`); if (li) li.focus({ preventScroll: true });
    }
    function newCmd(fromEditor, base) {
      setTab('commands');
      let dr;
      if (base) dr = base;
      else if (fromEditor) {
        const es = wf.editorState();
        if (!es) { note('편집기 상태를 읽을 수 없음', 'bad'); return; }
        const m = es.mode === 'raw' ? 'raw' : 'did';
        const v = m === 'did' ? UU.validateDid(es.didText) : UU.parseRawHex(es.rawText);
        dr = { name: m === 'did' ? (v.ok ? `DID ${v.norm}` : 'DID') : (v.ok ? `Raw ${v.norm}`.slice(0, LIM.name) : 'Raw'), desc: '', folder: wf.doc.prefs.folder || '', mode: m, didText: es.didText || '', rawText: es.rawText || '', target: null };
      } else dr = { name: '', desc: '', folder: wf.doc.prefs.folder || '', mode: 'did', didText: '', rawText: '', target: null };
      drafts.delete('new');
      openCmdEditor('new', Object.assign({ id: 'new' }, dr));
    }

    /* recent */
    function renderRecent() {
      const el = d.getElementById('wfRecent'); if (!el || d.getElementById('wfp-recent').hidden) return;
      paint(el, wf.recent.length ? wf.recent.map((r, i) => {
        const tx = E.byId(r.txId), s = tx ? tx.status : r.status;
        return `<li class="wf-item" data-ri="${i}"><div class="wf-main"><button type="button" class="wf-no lnk" data-wf="goto" data-tx="${r.txId}" ${tx ? '' : 'disabled'} title="${tx ? '기록에서 보기' : '기록에서 제거됨'}">#${r.no}</button>` +
          `<span class="wf-tgt">${esc(r.ecu)}·${esc(trLabel(r.transport))}</span><span class="wf-kind k-${r.mode}">${r.mode === 'did' ? 'DID' : 'RAW'}</span>` +
          `<code class="wf-pdu">${esc(r.mode === 'did' ? `22 ${r.text.slice(0, 2)} ${r.text.slice(2)}` : r.text.length > 72 ? r.text.slice(0, 72) + ' …' : r.text)}</code>` +
          `<span class="wf-st s-${s}">${esc(stTxt(s))}</span>${r.source && r.source !== 'manual' ? `<span class="wf-chip">${esc(r.source)}</span>` : ''}</div>` +
          `<div class="wf-acts"><button type="button" class="btn sm" data-wf="rec-load" data-k="rl-${r.sig}" title="요청 편집기에 채우기 (전송 없음, 대상 유지)">편집기로</button>` +
          `<button type="button" class="btn sm" data-wf="rec-save" data-k="rs-${r.sig}" title="이 요청을 대상 고정 명령 초안으로">명령 저장</button></div></li>`;
      }).join('') : '<li class="wf-empty">이번 세션에 보낸 요청 없음</li>');
    }

    /* sequences */
    const RS = { running: '실행 중', passed: '통과', 'done-errors': '완료 (실패 포함)', failed: '실패', stopped: '중지', refused: '시작 거부' };
    function renderSeqList() {
      const el = d.getElementById('wfSeqList'); if (!el) return;
      paint(el, wf.doc.sequences.length ? wf.doc.sequences.map(s => {
        const run = wf.runs.get(s.id), on = run && run.status === 'running';
        return `<li class="wf-item${seqSel === s.id ? ' sel' : ''}${seqDraft && seqDraft.id === s.id ? ' editing' : ''}" data-sid="${s.id}"><div class="wf-main">` +
          `<button type="button" class="wf-name lnk" data-wf="seq-sel" data-k="ss-${s.id}" title="실행 결과 보기">${esc(s.name)}</button>` +
          `<span class="wf-tgt">${esc(tgtText(s.target))}</span><span class="wf-desc">${s.steps.length}단계 × ${s.loops}회${s.stopOnError ? ' · 오류 시 중지' : ' · 오류 후 계속'}</span>` +
          (run ? `<span class="wf-st r-${run.status}">${RS[run.status] || run.status}${on ? ` ${run.loop + 1}/${run.loops} · ${run.idx + 1}/${run.steps.length}` : ''}</span>` : '') + `</div>` +
          `<div class="wf-acts">${on ? `<button type="button" class="btn sm danger" data-wf="seq-stop" data-k="sx-${s.id}">중지</button>` : `<button type="button" class="btn sm primary" data-wf="seq-run" data-k="sr-${s.id}" title="${esc(s.target ? tgtText(s.target) : '현재 대상 (시작 시 고정)')}">실행</button>`}` +
          `<button type="button" class="ibtn" data-wf="seq-edit" data-k="se-${s.id}" title="편집" aria-label="${esc(s.name)} 편집" ${on ? 'disabled' : ''}>✎</button>` +
          `<button type="button" class="ibtn" data-wf="seq-dup" data-k="sd-${s.id}" title="복제" aria-label="${esc(s.name)} 복제">⧉</button>` +
          `<button type="button" class="ibtn danger" data-wf="seq-del" data-k="sk-${s.id}" title="삭제" aria-label="${esc(s.name)} 삭제" ${on ? 'disabled' : ''}>✕</button></div></li>`;
      }).join('') : '<li class="wf-empty">시퀀스 없음 — [새 시퀀스]</li>');
    }
    function stepOpts(st) {
      const cmds = wf.doc.commands;
      return `<select class="fld sm" data-sf="mode" aria-label="단계 종류"><option value="did"${st.mode === 'did' ? ' selected' : ''}>DID</option><option value="raw"${st.mode === 'raw' ? ' selected' : ''}>Raw</option><option value="cmd"${st.mode === 'cmd' ? ' selected' : ''}${cmds.length ? '' : ' disabled'}>저장 명령</option></select>`;
    }
    function renderSeqSteps() {
      const f = d.getElementById('wfSeqEdit'), tb = q('#wfsSteps', f); if (!tb || !seqDraft) return;
      const cmds = wf.doc.commands;
      paint(tb, seqDraft.steps.map((st, j) => `<tr data-j="${j}"><td class="n">${j + 1}</td><td>${stepOpts(st).replace('data-sf="mode"', `data-sf="mode" data-k="m${j}"`)}</td><td>` +
        (st.mode === 'cmd' ? `<select class="fld sm" data-sf="cmdId" data-k="c${j}" aria-label="명령">${cmds.map(c => `<option value="${c.id}"${c.id === st.cmdId ? ' selected' : ''}>${esc(c.name)} — ${esc(pduOf(c))}</option>`).join('')}</select>`
          : st.mode === 'did' ? `<input class="fld sm" data-sf="didText" data-k="v${j}" value="${esc(st.didText || '')}" placeholder="F190" maxlength="12" aria-label="DID" spellcheck="false">`
          : `<input class="fld sm" data-sf="rawText" data-k="v${j}" value="${esc(st.rawText || '')}" placeholder="10 03" aria-label="Raw PDU" spellcheck="false">`) +
        `</td><td><input class="fld sm num" data-sf="delayMs" data-k="d${j}" type="number" min="0" max="${LIM.delay[1]}" step="100" value="${esc(st.delayMs)}" aria-label="지연 ms"></td>` +
        `<td><select class="fld sm" data-sf="status" data-k="s${j}" aria-label="기대 상태">${[['positive', '긍정'], ['nrc', 'NRC'], ['response', '응답(긍정/NRC)'], ['any', '무관']].map(([v, l]) => `<option value="${v}"${st.expect.status === v ? ' selected' : ''}>${l}</option>`).join('')}</select></td>` +
        `<td><input class="fld sm" data-sf="prefix" data-k="p${j}" value="${esc(st.expect.prefix || '')}" placeholder="예: 50 03" aria-label="응답 접두 hex" spellcheck="false"></td>` +
        `<td class="acts"><button type="button" class="ibtn" data-sa="up" data-k="u${j}" aria-label="단계 ${j + 1} 위로" ${j ? '' : 'disabled'}>↑</button><button type="button" class="ibtn" data-sa="down" data-k="w${j}" aria-label="단계 ${j + 1} 아래로" ${j < seqDraft.steps.length - 1 ? '' : 'disabled'}>↓</button><button type="button" class="ibtn danger" data-sa="rm" data-k="x${j}" aria-label="단계 ${j + 1} 삭제">✕</button></td></tr>`).join(''));
      q('#wfsCount', f).textContent = `${seqDraft.steps.length}/${LIM.steps}단계 · 최대 전송 ${seqDraft.steps.length * (+q('#wfsLoops', f).value || 0)}회`;
    }
    function openSeqEditor(s) {
      seqDraft = s ? JSON.parse(JSON.stringify(s)) : { id: 'new', name: '', target: null, loops: 1, stopOnError: true, steps: [{ mode: 'did', didText: 'F190', delayMs: 0, expect: { status: 'positive', prefix: '' } }] };
      const f = d.getElementById('wfSeqEdit'); f.hidden = false;
      f.innerHTML = `<div class="wf-eh"><b>${seqDraft.id === 'new' ? '새 시퀀스' : '시퀀스 편집'}</b><span class="wf-hint">지연 = 이전 단계 응답 후 대기 (최소 ${GAP_MS} ms 양보) · 단계×반복 ≤ ${LIM.sendsPerRun}</span></div>` +
        `<div class="wf-grid"><label>이름<input class="fld" id="wfsName" maxlength="${LIM.name}" value="${esc(seqDraft.name)}"></label>` +
        `<label>대상<select class="fld" id="wfsTarget">${targetOptions(null, '현재 대상 (시작 시 고정)')}</select></label>` +
        `<label>반복<input class="fld num" id="wfsLoops" type="number" min="1" max="${LIM.loops[1]}" value="${seqDraft.loops}"></label>` +
        `<label class="chk"><input type="checkbox" id="wfsStop" ${seqDraft.stopOnError ? 'checked' : ''}> 오류 시 중지 (fail-fast)</label></div>` +
        `<div class="wf-steps"><table class="wf-st-tbl"><thead><tr><th>#</th><th>종류</th><th>요청</th><th>지연 ms</th><th>기대</th><th>응답 접두</th><th></th></tr></thead><tbody id="wfsSteps"></tbody></table></div>` +
        `<div class="wf-eb"><button type="button" class="btn sm" data-wf="seq-add-step">단계 추가</button><span class="wf-hint" id="wfsCount"></span><span class="grow"></span>` +
        `<button type="submit" class="btn primary">저장</button><button type="button" class="btn" data-wf="seq-cancel">취소</button></div><div class="wf-err" id="wfsErr" role="alert"></div>`;
      q('#wfsTarget', f).value = tval(seqDraft.target);
      renderSeqSteps(); renderSeqList();
      q('#wfsName', f).focus({ preventScroll: true });
    }
    function readSeqForm() {
      const f = d.getElementById('wfSeqEdit'); if (!seqDraft || f.hidden) return null;
      seqDraft.name = q('#wfsName', f).value; seqDraft.target = parseTarget(q('#wfsTarget', f).value);
      seqDraft.loops = +q('#wfsLoops', f).value; seqDraft.stopOnError = q('#wfsStop', f).checked;
      f.querySelectorAll('#wfsSteps tr[data-j]').forEach(tr => {
        const st = seqDraft.steps[+tr.dataset.j]; if (!st) return;
        tr.querySelectorAll('[data-sf]').forEach(el => {
          const k = el.dataset.sf;
          if (k === 'status' || k === 'prefix') st.expect[k] = el.value;
          else if (k === 'delayMs') st.delayMs = el.value === '' ? NaN : +el.value;
          else if (k !== 'mode') st[k] = el.value;
        });
      });
      return seqDraft;
    }
    function closeSeqEditor() { seqDraft = null; const f = d.getElementById('wfSeqEdit'); f.hidden = true; f.innerHTML = ''; renderSeqList(); }
    function renderRun() {
      const el = d.getElementById('wfRun'); if (!el) return;
      const id = seqSel, s = wf.doc.sequences.find(x => x.id === id), run = id ? wf.runs.get(id) : null;
      if (!s || !run) { el.innerHTML = s ? `<p class="wf-hint">"${esc(s.name)}" 실행 기록 없음 (이번 세션)</p>` : ''; return; }
      const cur = run.status === 'running';
      const ph = { delay: '지연 대기', 'wait-watch': '같은 경로 감시 요청 완료 대기', await: '응답 대기', end: '' }[run.phase] || '';
      el.innerHTML = `<div class="wf-rh"><b>${esc(run.name)}</b><span class="wf-st r-${run.status}">${RS[run.status] || run.status}</span>` +
        `<span class="wf-tgt" title="시작 시 고정된 대상">${esc(tgtText(run.target))} 고정</span>` +
        // completed runs have loop === loops (idx wrapped to 0): show the full count, not "step 1"; failed/stopped show the step reached
        (run.steps.length ? `<span class="wf-desc">${run.loop >= run.loops ? `반복 ${run.loops}/${run.loops} · 단계 ${run.steps.length}/${run.steps.length} 완료` : `반복 ${run.loop + 1}/${run.loops} · 단계 ${run.idx + 1}/${run.steps.length}`}${cur && ph ? ' · ' + ph : ''}</span>` : '') +
        (run.error ? `<span class="wf-desc${run.status === 'failed' || run.status === 'refused' ? ' bad' : ''}">${esc(run.error)}</span>` : '') + `</div>` +
        (run.steps.length ? `<ol class="wf-steplist">${run.steps.map((st, j) => `<li class="${cur && j === run.idx ? 'cur' : ''}">${esc(st.label)}<span class="wf-desc">${st.delayMs ? ` +${st.delayMs} ms` : ''} · 기대 ${esc(st.expect.status)}${st.expect.prefix ? ' ' + esc(st.expect.prefix) : ''}</span></li>`).join('')}</ol>` : '') +
        (run.results.length ? `<table class="wf-res"><thead><tr><th>반복</th><th>단계</th><th>요청</th><th>상태</th><th>응답</th><th>판정</th></tr></thead><tbody>${run.results.slice(-30).reverse().map(r =>
          `<tr class="${r.ok ? 'ok' : 'bad'}"><td>${r.loop + 1}</td><td>${r.idx + 1}</td><td>${r.txId ? `<button type="button" class="lnk" data-wf="goto" data-tx="${r.txId}">#${r.no}</button>` : '—'}</td><td>${esc(stTxt(r.status))}</td><td><code>${esc(r.final)}</code></td><td>${r.ok ? '통과' : esc(r.note)}</td></tr>`).join('')}</tbody></table>` : '');
    }
    function renderCounts() {
      const set = (id, n) => { const e = d.getElementById(id); if (e) e.textContent = n; };
      set('wfcCmd', wf.doc.commands.length || ''); set('wfcRecent', wf.recent.length || ''); set('wfcSeq', wf.doc.sequences.length || '');
    }
    function renderLib() {
      if (!mW) return;
      renderCounts();
      const t = wf.doc.prefs.tab;
      if (t === 'commands') renderCmdList(); else if (t === 'recent') renderRecent(); else { renderSeqList(); renderRun(); }
    }
    function download(json) {
      const txt = JSON.stringify(json, null, 2), name = `loom-uds-workflow-${(H.ui && H.ui.stamp ? H.ui.stamp() : 'export')}.json`;
      if (H.ui && H.ui.download) H.ui.download(name, txt, 'application/json');
      return name;
    }
    if (mW) {
      mW.addEventListener('click', e => {
        const tb = e.target.closest('[data-wtab]'); if (tb) { setTab(tb.dataset.wtab); return; }
        const b = e.target.closest('[data-wf]'); if (!b) return;
        const a = b.dataset.wf, li = b.closest('[data-cid],[data-sid],[data-ri]');
        const cid = li && li.dataset.cid, sid = li && li.dataset.sid;
        if (a === 'save-current') newCmd(true);
        else if (a === 'cmd-new') newCmd(false);
        else if (a === 'cmd-load') { const c = wf.doc.commands.find(x => x.id === cid); if (c) { wf.loadToEditor(c); note(`"${c.name}" 편집기에 채움 — 전송 안 함`, 'ok'); } }
        else if (a === 'cmd-run') { const r = wf.runCommand(cid); if (!r.ok) toast(r.error, 'err'); renderCmdList(); }
        else if (a === 'cmd-edit') openCmdEditor(cid);
        else if (a === 'cmd-dup') { const r = wf.duplicateCommand(cid); note(r.ok ? '복제됨' : r.error, r.ok ? 'ok' : 'bad'); }
        else if (a === 'cmd-del') { const c = wf.doc.commands.find(x => x.id === cid); const r = wf.deleteCommand(cid); if (r.ok) { drafts.delete(cid); if (editing === cid) closeCmdEditor(); } note(r.ok ? `"${c.name}" 삭제됨` : r.error, r.ok ? 'ok' : 'bad'); }
        else if (a === 'cmd-cancel') closeCmdEditor(true);
        else if (a === 'goto') { const id = +b.dataset.tx; if (E.byId(id)) H.selectTxn(id); else toast('기록에서 제거된 트랜잭션'); }
        else if (a === 'rec-load') { const r = wf.recent[+li.dataset.ri]; if (r) { H.compose(r.mode === 'did' ? { mode: 'did', didText: r.text } : { mode: 'raw', rawText: r.text }); note(`#${r.no} 요청을 편집기에 채움 — 전송 안 함, 대상은 현재 선택 유지`, 'ok'); } }
        else if (a === 'rec-save') { const r = wf.recent[+li.dataset.ri]; if (r) newCmd(false, { name: (r.mode === 'did' ? `DID ${r.text}` : `Raw ${r.text}`).slice(0, LIM.name), desc: `#${r.no}에서 저장`, folder: '', mode: r.mode, didText: r.mode === 'did' ? r.text : '', rawText: r.mode === 'raw' ? r.text : '', target: { ecu: r.ecu, transport: r.transport } }); }
        else if (a === 'seq-new') openSeqEditor(null);
        else if (a === 'seq-edit') openSeqEditor(wf.doc.sequences.find(x => x.id === sid));
        else if (a === 'seq-dup') { const r = wf.duplicateSequence(sid); note(r.ok ? '복제됨' : r.error, r.ok ? 'ok' : 'bad'); }
        else if (a === 'seq-del') { const r = wf.deleteSequence(sid); if (r.ok && seqSel === sid) seqSel = null; if (r.ok && seqDraft && seqDraft.id === sid) closeSeqEditor(); note(r.ok ? '삭제됨' : r.error, r.ok ? 'ok' : 'bad'); }
        else if (a === 'seq-sel') { seqSel = sid; renderSeqList(); renderRun(); }
        else if (a === 'seq-run') { seqSel = sid; const r = wf.startSequence(sid); if (!r.ok) toast(r.error, 'err'); renderSeqList(); renderRun(); }
        else if (a === 'seq-stop') { wf.stopSequence(sid); renderSeqList(); renderRun(); }
        else if (a === 'seq-cancel') closeSeqEditor();
        else if (a === 'seq-add-step') { readSeqForm(); if (seqDraft.steps.length >= LIM.steps) { q('#wfsErr').textContent = `단계 최대 ${LIM.steps}`; return; } seqDraft.steps.push({ mode: 'did', didText: '', delayMs: 0, expect: { status: 'positive', prefix: '' } }); renderSeqSteps(); const n = d.querySelector(`#wfsSteps [data-k="v${seqDraft.steps.length - 1}"]`); if (n) n.focus(); }
        else if (a === 'export') { const n = download(wf.exportDoc()); note(`정의 내보냄: ${n} (실행 상태·최근 요청 제외)`, 'ok'); }
        else if (a === 'import') { if (wf.anyRunning()) { note('실행 중인 감시/시퀀스가 있음 — 먼저 중지', 'bad'); return; } d.getElementById('wfFile').click(); }
        else if (a === 'undo-import') { const r = wf.undoImport(); note(r.ok ? '가져오기 이전 정의로 되돌림' : '되돌릴 수 없음 (실행 중이거나 기록 없음)', r.ok ? 'ok' : 'bad'); }
      });
      d.getElementById('wfSeqEdit').addEventListener('click', e => {
        const b = e.target.closest('[data-sa]'); if (!b || !seqDraft) return;
        readSeqForm();
        const j = +b.closest('tr').dataset.j, s = seqDraft.steps;
        if (b.dataset.sa === 'rm') { if (s.length > 1) s.splice(j, 1); else { q('#wfsErr').textContent = '최소 1단계'; return; } }
        else if (b.dataset.sa === 'up' && j > 0) { [s[j - 1], s[j]] = [s[j], s[j - 1]]; b.dataset.k = 'u' + (j - 1); }
        else if (b.dataset.sa === 'down' && j < s.length - 1) { [s[j + 1], s[j]] = [s[j], s[j + 1]]; b.dataset.k = 'w' + (j + 1); }
        renderSeqSteps();
      });
      d.getElementById('wfSeqEdit').addEventListener('change', e => {
        const el = e.target;
        if (el.dataset.sf === 'mode') { readSeqForm(); const st = seqDraft.steps[+el.closest('tr').dataset.j]; st.mode = el.value; if (st.mode === 'cmd' && !st.cmdId && wf.doc.commands[0]) st.cmdId = wf.doc.commands[0].id; renderSeqSteps(); }
        else if (el.id === 'wfsLoops') { readSeqForm(); renderSeqSteps(); }
      });
      d.getElementById('wfSeqEdit').addEventListener('submit', e => {
        e.preventDefault(); const dr = readSeqForm(); if (!dr) return;
        const r = wf.saveSequence(dr);
        if (!r.ok) { q('#wfsErr').textContent = r.error; return; }
        seqSel = r.id; closeSeqEditor(); renderRun(); note('시퀀스 저장됨', 'ok');
      });
      const cf = d.getElementById('wfCmdEdit');
      cf.addEventListener('submit', e => { e.preventDefault(); saveCmd(); });
      cf.addEventListener('input', () => cmdPreview());
      cf.addEventListener('change', () => cmdPreview());
      cf.addEventListener('click', e => { const b = e.target.closest('#wfeMode [data-m]'); if (b) { readCmdForm(); cmdMode(b.dataset.m); } });
      cf.addEventListener('keydown', e => {
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeCmdEditor(false); return; }
        if (e.target.closest('#wfeMode') && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) { e.preventDefault(); readCmdForm(); const m = drafts.get(editing).mode === 'did' ? 'raw' : 'did'; cmdMode(m); q(`#wfeMode [data-m="${m}"]`).focus(); }
      });
      d.getElementById('wfTabs').addEventListener('keydown', e => {
        if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
        e.preventDefault(); const i = TABS.indexOf(wf.doc.prefs.tab);
        setTab(TABS[(i + (e.key === 'ArrowRight' ? 1 : TABS.length - 1)) % TABS.length], true);
      });
      d.getElementById('wfFolder').addEventListener('change', e => { wf.setPrefs({ folder: e.target.value }); renderCmdList(); });
      d.getElementById('wfSearch').addEventListener('input', () => renderCmdList());
      d.getElementById('wfFile').addEventListener('change', e => {
        const file = e.target.files && e.target.files[0]; e.target.value = '';
        if (!file) return;
        if (file.size > LIM.fileBytes) { note(`파일 ${file.size} B — 최대 ${LIM.fileBytes} B (가져오지 않음)`, 'bad'); return; }
        const rd = new root.FileReader();
        rd.onload = () => {
          let j; try { j = JSON.parse(String(rd.result)); } catch (err) { note('JSON 구문 오류 — 가져오지 않음 (기존 정의 유지)', 'bad'); return; }
          const r = wf.importDoc(j);
          if (!r.ok) { note(`가져오기 거부: ${r.error} — 기존 정의 유지`, 'bad'); return; }
          drafts.clear(); editing = null; seqDraft = null; seqSel = null;
          ['wfCmdEdit', 'wfSeqEdit'].forEach(id => { const f = d.getElementById(id); f.hidden = true; f.innerHTML = ''; });
          note(`가져옴 (교체): 명령 ${r.counts.commands} · 감시 ${r.counts.watches} · 시퀀스 ${r.counts.sequences} — 아무것도 자동 시작하지 않음`, 'ok', true);
        };
        rd.onerror = () => note('파일 읽기 실패', 'bad');
        rd.readAsText(file);
      });
    }

    /* ---------------- watch (#watchMount) ---------------- */
    if (mT) {
      mT.innerHTML = `<div class="wt" id="wt">` +
        `<form class="wt-add" id="wtAdd" novalidate autocomplete="off"><label>DID<input class="fld" id="wtDid" list="wtDids" maxlength="12" placeholder="0101" spellcheck="false"></label><datalist id="wtDids"></datalist>` +
        `<label>대상<select class="fld" id="wtTarget"></select></label>` +
        `<label>주기<select class="fld" id="wtInt">${INTERVALS.map(v => `<option value="${v}"${v === 1000 ? ' selected' : ''}>${v >= 1000 ? v / 1000 + ' s' : v + ' ms'}</option>`).join('')}</select></label>` +
        `<button type="submit" class="btn">감시 추가</button><span class="grow"></span>` +
        `<button type="button" class="btn danger" data-wt="stop-all" title="소유한 감시 폴링만 중지 (수동 요청 취소 안 함)">모두 중지</button></form>` +
        `<div class="wf-err" id="wtMsg" role="alert"></div>` +
        `<div class="wt-scroll"><table class="wt-tbl" aria-label="DID 감시"><thead><tr><th>DID</th><th>대상</th><th class="r">값</th><th>경과</th><th>상태</th><th>출처</th><th class="r">최소 / 최대</th><th>추이</th><th>주기 · 실제 · 건너뜀</th><th></th></tr></thead><tbody id="wtBody"></tbody></table></div>` +
        `<p class="wf-hint">명시적 [시작]으로만 폴링 · 새로고침/가져오기/재연결 시 자동 시작 없음 · 경로당 진행 중 요청 1개 (사용 중이면 그 주기는 건너뜀, 대기열 없음) · 값은 엔진 확정 값(해당 요청이 반영한 경우만) · NRC/타임아웃은 이전 값과 경과 유지 · 링크 끊김 시 중지.</p></div>`;
    }
    function didName(h) { const x = (UU.DIDS || []).find(z => UU.hex4(z.did) === h); return x ? x.name : ''; }
    function fillWatchSelects() {
      const tg = d.getElementById('wtTarget'); if (!tg) return;
      const v = tg.value; tg.innerHTML = targetOptions(null, null);
      const cur = wf.curCtx(); tg.value = v && [...tg.options].some(o => o.value === v) ? v : tval(cur);
      d.getElementById('wtDids').innerHTML = (UU.DIDS || []).map(x => `<option value="${UU.hex4(x.did)}">${esc(x.name)}</option>`).join('');
    }
    function spark(r) {
      const s = r.samples; if (s.length < 2) return '<span class="wt-na">—</span>';
      const W = 96, Hh = 20, lo = Math.min.apply(null, s), hi = Math.max.apply(null, s), span = hi - lo || 1;
      const pts = s.map((v, i) => `${(i * (W - 2) / (s.length - 1) + 1).toFixed(1)},${(Hh - 2 - (v - lo) * (Hh - 4) / span).toFixed(1)}`).join(' ');
      const last = pts.split(' ').pop().split(',');
      return `<svg class="wt-spark" viewBox="0 0 ${W} ${Hh}" width="${W}" height="${Hh}" role="img" aria-label="최근 ${s.length}개 값 ${lo}–${hi}"><polyline points="${pts}" fill="none"/><circle cx="${last[0]}" cy="${last[1]}" r="1.8"/></svg>`;
    }
    const WS = { idle: '대기', running: '시작', ok: '정상', stale: '반영 안 됨', nrc: 'NRC', timeout: '타임아웃', cancelled: '취소', 'link-lost': '링크 끊김', offline: '오프라인', stopped: '중지', error: '오류', skip: '건너뜀' };
    function age(r) { if (r.valueT == null) return '—'; const s = Math.max(0, (H.clock.now() - r.valueT) / 1000); return s < 10 ? s.toFixed(1) + ' s' : Math.round(s) + ' s'; }
    function cellsFor(w) {
      const r = wf.wrt(w.id), cad = wf.cadence(r), sk = r.skips.busy + r.skips.sequence;
      const inflight = r.inflight != null ? E.byId(r.inflight) : null;
      return {
        value: r.value == null ? '<span class="wt-na">—</span>' : `<b>${esc(r.value)}</b> <span class="u">${esc(r.unit)}</span>`,
        age: age(r),
        status: `<span class="wt-st w-${r.status}${r.running ? ' on' : ''}" title="${esc(r.note)}">${r.running ? '● ' : ''}${esc(WS[r.status] || r.status)}${inflight ? ' · 진행 #' + inflight.no : ''}</span>${r.note ? `<span class="wt-note">${esc(r.note)}</span>` : ''}`,
        src: r.src ? `<button type="button" class="lnk" data-wt="goto" data-tx="${r.src.id}" title="${esc(stTxt(r.src.status))}">#${r.src.no}</button>${r.valueNo && r.valueNo !== r.src.no ? ` <span class="wt-na">값 #${r.valueNo}</span>` : ''}` : '—',
        mm: r.min == null ? '—' : `${r.min} / ${r.max}`,
        spark: r.numeric ? spark(r) : '<span class="wt-na">비수치</span>',
        cad: `<span title="실제 평균 송신 간격 (최근 ${r.sends.length}회)">${cad == null ? '—' : cad + ' ms'}</span> · <span class="${sk ? 'wt-skip' : ''}" title="사용 중 ${r.skips.busy} · 시퀀스 ${r.skips.sequence}">${sk}</span> · 송신 ${r.sent}`,
        btn: r.running ? `<button type="button" class="btn sm danger" data-wt="stop" data-k="t-${w.id}">중지</button>` : `<button type="button" class="btn sm primary" data-wt="start" data-k="t-${w.id}">시작</button>`,
      };
    }
    function renderWatchRows() {
      const tb = d.getElementById('wtBody'); if (!tb) return;
      paint(tb, wf.doc.watches.length ? wf.doc.watches.map(w => {
        const c = cellsFor(w), missing = !wf.ecuExists(w.target.ecu);
        return `<tr data-wid="${w.id}"><td><code>${w.did}</code> <span class="wt-dn">${esc(didName(w.did))}</span></td>` +
          `<td class="${missing ? 'bad' : ''}" title="${esc(wf.routeTag(w.target))}">${esc(tgtText(w.target))}${missing ? ' (없음)' : ''}</td>` +
          `<td class="r" data-c="value">${c.value}</td><td data-c="age">${c.age}</td><td data-c="status">${c.status}</td><td data-c="src">${c.src}</td><td class="r" data-c="mm">${c.mm}</td><td data-c="spark">${c.spark}</td>` +
          `<td><select class="fld sm" data-wt="int" data-k="i-${w.id}" aria-label="DID ${w.did} 주기">${INTERVALS.concat(INTERVALS.includes(w.intervalMs) ? [] : [w.intervalMs]).map(v => `<option value="${v}"${v === w.intervalMs ? ' selected' : ''}>${v >= 1000 ? v / 1000 + ' s' : v + ' ms'}</option>`).join('')}</select> <span data-c="cad">${c.cad}</span></td>` +
          `<td class="acts"><span data-c="btn">${c.btn}</span><button type="button" class="ibtn danger" data-wt="del" data-k="x-${w.id}" aria-label="DID ${w.did} 감시 삭제">✕</button></td></tr>`;
      }).join('') : '<tr><td colspan="10" class="wf-empty">감시 없음 — DID·대상·주기를 고르고 [감시 추가]</td></tr>');
    }
    // runtime patch: only the changed cells, never the interval select / add form
    function patchWatch() {
      const tb = d.getElementById('wtBody'); if (!tb) return;
      wf.doc.watches.forEach(w => {
        const tr = tb.querySelector(`tr[data-wid="${w.id}"]`); if (!tr) return;
        const c = cellsFor(w);
        Object.keys(c).forEach(k => { const el = tr.querySelector(`[data-c="${k}"]`); if (el && el.innerHTML !== c[k]) paint(el, c[k]); });
      });
    }
    if (mT) {
      d.getElementById('wtAdd').addEventListener('submit', e => {
        e.preventDefault();
        const r = wf.addWatch({ did: d.getElementById('wtDid').value, target: parseTarget(d.getElementById('wtTarget').value), intervalMs: +d.getElementById('wtInt').value });
        d.getElementById('wtMsg').textContent = r.ok ? '' : r.error;
        if (r.ok) d.getElementById('wtDid').value = '';
      });
      mT.addEventListener('click', e => {
        const b = e.target.closest('[data-wt]'); if (!b || b.tagName === 'SELECT') return;
        const tr = b.closest('tr[data-wid]'), id = tr && tr.dataset.wid, a = b.dataset.wt;
        if (a === 'start') { const r = wf.startWatch(id); d.getElementById('wtMsg').textContent = r.ok ? '' : r.error; }
        else if (a === 'stop') wf.stopWatch(id, '사용자 중지');
        else if (a === 'del') wf.deleteWatch(id);
        else if (a === 'stop-all') wf.stopAllWatches('사용자 중지 (모두)');
        else if (a === 'goto') { const t = +b.dataset.tx; if (E.byId(t)) H.selectTxn(t); else toast('기록에서 제거된 트랜잭션'); }
      });
      mT.addEventListener('change', e => { const s = e.target.closest('select[data-wt="int"]'); if (!s) return; const r = wf.setWatchInterval(s.closest('tr').dataset.wid, +s.value); d.getElementById('wtMsg').textContent = r.ok ? '' : r.error; });
    }

    /* ---------------- wiring ---------------- */
    wf.on(what => {
      if (what === 'defs') { renderLib(); renderWatchRows(); return; }
      if (what === 'watch-defs' || what === 'watch') { renderWatchRows(); return; }
      if (what === 'age' || what === 'watch-rt') { patchWatch(); return; }
      if (what === 'seq') { if (mW && wf.doc.prefs.tab === 'sequences') { renderSeqList(); renderRun(); } return; }
      // host/cmd: engine progress → statuses only
      patchWatch(); renderCounts();
      if (!mW) return;
      const t = wf.doc.prefs.tab;
      if (t === 'commands') renderCmdList(); else if (t === 'recent') renderRecent(); else { renderSeqList(); renderRun(); }
    });
    // ECU registry is dynamic (core catalog): refresh target pickers when it changes, nothing else.
    if (typeof H.onRender === 'function') H.onRender(() => { const s = ecuSig(); if (s !== lastEcuSig) { lastEcuSig = s; fillWatchSelects(); renderLib(); renderWatchRows(); } });
    if (wf.notice || wf.storeErr) note(wf.notice || wf.storeErr, 'bad');
    fillWatchSelects();
    if (mW) setTab(wf.doc.prefs.tab);
    renderWatchRows();
    return { renderLib, renderWatchRows, patchWatch, openCmdEditor, setTab, drafts };
  }

  /* =================== bootstrap (once; no polling for core) =================== */
  let booted = null;
  function boot() {
    if (booted || !root.LoomWorkbench) return booted;
    const H = root.LoomWorkbench;
    const wf = create(H);
    let ui = null;
    try { ui = mountUI(wf, H); } catch (e) { if (root.console) root.console.error('workflow UI', e); }
    booted = { wf, ui };
    root.LoomUdsWorkflow.instance = wf; root.LoomUdsWorkflow.ui = ui;
    return booted;
  }

  root.LoomUdsWorkflow = Object.assign(root.LoomUdsWorkflow || {}, { model, create, mountUI, boot });
  if (root.LoomWorkbench) boot();
  else if (root.document) root.document.addEventListener('uds:host-ready', boot, { once: true });
})(typeof window !== 'undefined' ? window : globalThis);
