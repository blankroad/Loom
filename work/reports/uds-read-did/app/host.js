/* Loom UDS workbench — shared host contract (window.LoomWorkbench, "H"). Core-owned (uds-core-opus).
   Loads after engine/app/tools/config, before catalog/services/project/workflow/trace-tools/layout.
   Extensions use ONLY this object (not loomUdsOpus internals). Local SIM only: every request goes through the
   same engine E.read / E.sendRaw path, context, serial guard and timing as a manual read. */
(function () {
  'use strict';
  const U = window.LoomUds, O = window.loomUdsOpus, E = O.E, st = O.st, hook = O.hook, d = document, $ = id => d.getElementById(id);
  const VERSION = 1;
  const KEY_RE = /^[a-z][a-z0-9_-]{0,31}$/;

  /* ---------- clock (same instance the engine uses; fake clock in checks) ---------- */
  const clock = Object.freeze({ now: () => E.clock.now(), set: (fn, ms) => E.clock.set(fn, ms), clear: h => E.clock.clear(h) });

  /* ---------- context / editor ---------- */
  function ctxSnap(ecu, tr) {
    if (!U.ECUS.some(x => x.id === ecu) || !U.FIELDS[tr]) return null;
    const c = E.ctx(ecu, tr), r = E.routeFor(ecu, tr);
    return { ecu, transport: tr, profile: Object.assign({}, r.profile), link: c.link, gen: c.gen, activeTxnId: c.active || null };
  }
  const context = () => ctxSnap(st.ecu, st.transport) || ctxSnap(U.ECUS[0].id, st.transport);
  const editor = () => ({ mode: st.mode, didText: st.didText, rawText: st.rawText });
  // History scope the trace view is applied on top of (base rows can be empty/evicted, so B cannot infer it).
  const traceScope = () => ({ filter: st.filter, ecu: st.ecu });

  // Fill the request editor only. Never sends, never connects.
  function compose(o) {
    o = o || {};
    const mode = o.mode === 'raw' ? 'raw' : o.mode === 'did' ? 'did' : null;
    if (!mode) return { ok: false, code: 'invalid', error: 'mode는 did 또는 raw' };
    if (o.didText != null && (typeof o.didText !== 'string' || o.didText.length > 64)) return { ok: false, code: 'invalid', error: 'didText 문자열 64자 이하' };
    if (o.rawText != null && (typeof o.rawText !== 'string' || o.rawText.length > U.RAW_TEXT_MAX)) return { ok: false, code: 'invalid', error: `rawText 문자열 ${U.RAW_TEXT_MAX}자 이하` };
    if (o.didText != null) { st.didText = o.didText; $('didInput').value = o.didText; st.submitted = false; }
    if (o.rawText != null) { st.rawText = o.rawText; $('rawInput').value = o.rawText; st.rawSubmitted = false; }
    st.mode = mode;
    O.savePrefs(); O.render();
    return { ok: true };
  }

  // Explicit-context send through the shared engine. No auto-connect; offline / busy / invalid refused by the engine.
  // opts.sim (optional) overrides the SIM option for this request only; default = current SIM options (same as manual).
  function send(o) {
    o = o || {};
    const ecu = o.ecu != null ? o.ecu : st.ecu, transport = o.transport != null ? o.transport : st.transport;
    const source = typeof o.source === 'string' && o.source ? o.source : 'manual';
    const sim = o.sim && typeof o.sim === 'object' ? Object.assign({}, st.sim, o.sim) : st.sim;
    let res;
    if (o.mode === 'raw') res = E.sendRaw({ ecu, transport, rawText: o.rawText, sim, source });
    else if (o.mode === 'did') res = E.read({ ecu, transport, didText: o.didText, sim, source });
    else return { ok: false, code: 'invalid', error: 'mode는 did 또는 raw' };
    if (!res.ok) return { ok: false, code: res.code, error: res.error };
    // Manual-source sends select the new row (same as [읽기]); automation sources never steal the user's selection.
    if (o.select === true || (o.select !== false && source === 'manual')) O.selectTxn(res.txn.id, { scroll: true });
    O.schedule();
    return { ok: true, txn: res.txn };
  }

  // Owned cancellation: only if this exact txn is still its context's active request (and, if given, its source matches).
  function cancelOwned(txnId, source) {
    const tx = E.byId(txnId);
    if (!tx) return { ok: false, code: 'gone', error: '트랜잭션 없음' };
    if (source != null && tx.source !== source) return { ok: false, code: 'not-owner', error: `소유자 불일치 (${tx.source})` };
    const c = E.ctx(tx.ecu, tx.transport);
    if (c.active !== tx.id) return { ok: false, code: 'not-active', error: '이미 종료되었거나 다른 요청이 진행 중' };
    const r = E.cancel(tx.ecu, tx.transport);
    O.schedule();
    return r.ok ? { ok: true, txn: r.txn } : { ok: false, code: 'engine', error: r.error };
  }

  /* ---------- events ---------- */
  const subs = new Set();
  const fire = ev => subs.forEach(fn => { try { fn(ev); } catch (e) { /* subscriber isolation */ } });
  E.on((type, data) => {
    data = data || {};
    // Current ECU removed by a catalog change → re-target to the first ECU BEFORE subscribers run (no TX, no connect).
    if (type === 'catalog' && !U.ECUS.some(x => x.id === st.ecu)) O.setContext(U.ECUS[0].id, null);
    const ev = { type };
    if (data.txn) { ev.txn = data.txn; if (data.created) ev.created = true; if (data.late) ev.late = true; }
    if (data.ctx) ev.context = { ecu: data.ctx.ecu, transport: data.ctx.transport, link: data.ctx.link, gen: data.ctx.gen, cause: data.ctx.cause || null };
    ['ctxKey', 'action', 'ecu', 'did', 'session'].forEach(k => { if (data[k] != null) ev[k] = data[k]; });
    fire(ev);
  });
  hook.ctxCbs.push(() => fire({ type: 'context', context: context() }));
  function subscribe(fn) { if (typeof fn !== 'function') return () => {}; subs.add(fn); return () => subs.delete(fn); }
  function onRender(fn) {
    if (typeof fn !== 'function') return () => {};
    hook.renderCbs.push(fn);
    return () => { const i = hook.renderCbs.indexOf(fn); if (i >= 0) hook.renderCbs.splice(i, 1); };
  }

  /* ---------- trace view ---------- */
  function setTraceView(v) {
    if (v != null && (typeof v !== 'object' || typeof v.rows !== 'function')) return { ok: false, error: '{rows(baseRows), byId(id)} 또는 null' };
    hook.traceView = v || null;
    O.render();
    return { ok: true };
  }

  /* ---------- project section registry (coordinator = project.js) ---------- */
  const CORE_KEYS = ['catalog', 'profiles', 'services', 'core'];
  const sections = new Map();
  const project = {
    register(key, impl) {
      if (!KEY_RE.test(String(key)) || CORE_KEYS.includes(key)) throw new Error('project key invalid/reserved: ' + key);
      if (!impl || ['export', 'validate', 'import'].some(f => typeof impl[f] !== 'function')) throw new Error('project section needs export/validate/import');
      sections.set(key, impl);
      return () => { if (sections.get(key) === impl) sections.delete(key); };
    },
    sections: () => new Map(sections),
  };
  const coreSections = new Map(); // core-owned project sections (catalog.js / config / services.js), same shape
  function registerCore(key, impl) { coreSections.set(key, impl); }

  /* ---------- core project API (consumed by A's project coordinator; see team/PROJECT-CONTRACT.md) ----------
     Core definitions = catalog (custom ECUs/DIDs/SIM fixtures) + profiles (custom connection profiles) + services prefs.
     validate() is PURE: catalog and profiles are validated TOGETHER against the candidate ECU set; live U.ECUS/DIDS untouched. */
  const coreProject = {
    keys: ['catalog', 'profiles', 'services'],
    export() {
      const sv = coreSections.get('services');
      return { catalog: E.catalog.exportStore(), profiles: E.exportConfig(), services: sv ? sv.export() : null };
    },
    validate(core) {
      if (!core || typeof core !== 'object' || Array.isArray(core)) return { ok: false, error: 'core 섹션 객체 아님' };
      const cat = E.catalog.validate(core.catalog == null ? null : core.catalog);
      if (!cat.ok) return { ok: false, section: 'catalog', error: cat.error };
      const ids = U.PRESET_ECUS.concat(cat.value.ecus.map(e => e.id));
      const prof = E.validateConfigFor(core.profiles == null ? null : core.profiles, ids);
      if (!prof.ok) return { ok: false, section: 'profiles', error: prof.error };
      const sv = coreSections.get('services');
      const svc = sv ? sv.validate(core.services == null ? null : core.services) : { ok: true, value: null };
      if (!svc.ok) return { ok: false, section: 'services', error: svc.error };
      return { ok: true, value: { catalog: cat.value, profiles: prof.value, services: svc.value } };
    },
    // Blocking reasons BEFORE any mutation: running activities (all keys) + any online/busy context. Never auto-stops.
    guard() {
      const reasons = [];
      activity.list().forEach(a => reasons.push({ kind: 'activity', key: a.key, ecu: a.ecu || null, transport: a.transport || null, label: a.label || a.kind }));
      U.ECUS.forEach(e => U.TRANSPORTS.forEach(t => { const c = E.ctx(e.id, t.id); if (c.link === 'online' || c.active) reasons.push({ kind: c.active ? 'busy' : 'online', ecu: e.id, transport: t.id, label: `${e.id} · ${t.label} ${c.active ? '요청 진행 중' : '연결됨'}` }); }));
      return reasons;
    },
    // value MUST come from validate(). Throws/returns !ok on failure → caller restores export() snapshot via apply().
    apply(value) {
      const g = coreProject.guard().filter(r => r.kind !== 'activity');
      if (g.length) return { ok: false, error: '연결/진행 중 컨텍스트: ' + g.map(r => r.label).join(', ') };
      const rc = E.catalog.replace(value.catalog); if (!rc.ok) return rc;
      const rp = E.loadConfig(value.profiles); if (!rp.ok) return { ok: false, error: '프로필 적용 실패: ' + rp.error };
      const sv = coreSections.get('services'); if (sv && value.services) sv.import(value.services);
      O.render();
      return { ok: true };
    },
    commit: () => O.savePrefs(), // true = written to localStorage (false = quota/unavailable → rollback)
    // Explicit user action only (e.g. a [모두 연결 해제] button in the guard notice).
    disconnectAll() { let n = 0; U.ECUS.forEach(e => U.TRANSPORTS.forEach(t => { if (E.ctx(e.id, t.id).link === 'online') { E.disconnect(e.id, t.id, 'user'); n++; } })); O.render(); return n; },
  };

  /* ---------- activity registry (running automation / recording / TP) ---------- */
  const acts = new Map();
  const activity = {
    register(key, impl) {
      if (!KEY_RE.test(String(key))) throw new Error('activity key invalid: ' + key);
      if (!impl || typeof impl.list !== 'function' || typeof impl.stop !== 'function') throw new Error('activity needs list()/stop()');
      acts.set(key, impl);
      return () => { if (acts.get(key) === impl) acts.delete(key); };
    },
    // [{key, ecu?, transport?, kind, label?}] — every RUNNING item (offline-but-running included).
    list() {
      const out = [];
      acts.forEach((impl, key) => {
        let l = [];
        try { l = impl.list() || []; } catch (e) { l = [{ kind: 'error', label: '활동 목록 오류 — 실행 중으로 간주' }]; }
        l.forEach(x => out.push(Object.assign({ key }, x)));
      });
      return out;
    },
    // Explicit user action only (a button); never called implicitly by imports or deletes.
    stop(key) { const impl = acts.get(key); if (!impl) return false; try { impl.stop(); } catch (e) { return false; } O.schedule(); return true; },
    forEcu(ecu) { return activity.list().filter(x => x.ecu === ecu); },
  };

  /* ---------- tool drawer (compact tabs; one panel open at a time) ---------- */
  const TOOLS = { services: '서비스 · 세션 / TesterPresent / DTC', catalog: 'ECU · DID 카탈로그', workflow: '저장 명령 · 시퀀스', watch: '감시 (DID 주기 읽기)', project: '프로젝트 파일' };
  let tool = typeof O.saved.toolTab === 'string' && TOOLS[O.saved.toolTab] ? O.saved.toolTab : null;
  hook.prefs.set('toolTab', () => tool);
  const drawer = $('toolDrawer');
  function placeDrawer() { const cb = $('ctxbar'); if (cb && drawer) drawer.style.setProperty('--td-top', cb.offsetHeight + 'px'); }
  function renderDrawer() {
    d.querySelectorAll('#toolTabs [data-tool]').forEach(b => { const on = b.dataset.tool === tool; b.setAttribute('aria-selected', on ? 'true' : 'false'); b.tabIndex = on || (!tool && b.dataset.tool === 'services') ? 0 : -1; });
    drawer.hidden = !tool;
    Object.keys(TOOLS).forEach(k => { $('tp-' + k).hidden = k !== tool; });
    $('toolTitle').textContent = tool ? TOOLS[tool] : '';
    d.body.classList.toggle('tool-open', !!tool);
    if (tool) placeDrawer();
  }
  function openTool(k, opts) {
    const prev = tool;
    tool = k && TOOLS[k] ? (k === tool && !(opts && opts.keep) ? null : k) : null;
    O.savePrefs(); renderDrawer();
    if (tool && tool !== prev) { const p = $('tp-' + tool); const f = p.querySelector('input,select,textarea,button'); (f || $('toolClose')).focus({ preventScroll: true }); }
    if (!tool && prev) { const b = $('tt-' + prev); if (b) b.focus(); }
    fire({ type: 'tool', tool });
    return tool;
  }
  $('toolTabs').addEventListener('click', e => { const b = e.target.closest('[data-tool]'); if (b) openTool(b.dataset.tool); });
  $('toolTabs').addEventListener('keydown', e => {
    if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    e.preventDefault();
    const ks = Object.keys(TOOLS), cur = d.activeElement && d.activeElement.dataset ? d.activeElement.dataset.tool : tool, i = Math.max(0, ks.indexOf(cur));
    $('tt-' + ks[(i + (e.key === 'ArrowRight' ? 1 : ks.length - 1)) % ks.length]).focus();
  });
  $('toolClose').addEventListener('click', () => openTool(null));
  drawer.addEventListener('keydown', e => { if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); openTool(null); } });
  if (typeof window.ResizeObserver === 'function') { try { new window.ResizeObserver(placeDrawer).observe($('ctxbar')); } catch (e) { /* optional */ } }
  window.addEventListener('resize', placeDrawer);

  /* ---------- connection overview (all ECU × route contexts that are online or busy) ---------- */
  function renderOverview() {
    const items = [];
    U.ECUS.forEach(e => U.TRANSPORTS.forEach(t => {
      const c = E.ctx(e.id, t.id); if (c.link !== 'online' && !c.active) return;
      const cur = e.id === st.ecu && t.id === st.transport;
      items.push(`<button type="button" class="ov-i${cur ? ' cur' : ''}${c.active ? ' busy' : ''}" data-ov="${O.esc(e.id)}|${t.id}" title="${O.esc(e.id + ' · ' + t.label + (c.link === 'online' ? ' 연결됨 gen ' + c.gen : ' 연결 안 됨') + (c.active ? ' · 진행 #' + E.byId(c.active).no : '') + ' — 클릭: 이 대상으로 전환 (전송 없음)')}"><span class="dot"></span>${O.esc(e.id)}·${O.esc(t.label)}</button>`);
    }));
    const acts = activity.list().length;
    $('connOverview').innerHTML = `<span class="flab">연결</span>${items.length ? items.join('') : '<span class="ov-none">없음</span>'}` + (acts ? `<span class="ov-act" title="실행 중 자동 작업 (감시/시퀀스/TesterPresent/기록)">자동 ${acts}</span>` : '');
  }
  $('connOverview').addEventListener('click', e => { const b = e.target.closest('[data-ov]'); if (!b) return; const [ecu, tr] = b.dataset.ov.split('|'); O.setContext(ecu, tr); });
  onRender(renderOverview);

  const H = Object.freeze({
    version: VERSION, engine: E, constants: U, clock,
    context, contextOf: ctxSnap, editor, traceScope, compose, send, cancelOwned,
    selectTxn: id => O.selectTxn(id, { scroll: true }), refresh: () => O.render(), schedule: () => O.schedule(),
    subscribe, onRender, setTraceView, project, activity, coreProject,
    tools: { open: k => openTool(k, { keep: true }), close: () => openTool(null), current: () => tool, names: Object.keys(TOOLS) },
    ui: { esc: O.esc, toast: O.toast, download: (n, t, ty) => window.LoomUdsTools.download(n, t, ty), stamp: () => window.LoomUdsTools.stamp(), fmtT: O.fmtT },
    _core: { registerCore, coreSections, fire, O },
  });
  window.LoomWorkbench = H;
  renderDrawer(); renderOverview();
  try { d.dispatchEvent(new window.CustomEvent('uds:host-ready', { detail: { version: VERSION } })); } catch (e) { /* old env */ }
})();
