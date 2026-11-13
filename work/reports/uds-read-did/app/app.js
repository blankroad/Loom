/* Loom UDS Read DID — DOM adapter (Opus). Local mock only; see engine.js. */
(function () {
  'use strict';
  const U = window.LoomUds, d = document, $ = id => d.getElementById(id);
  const PKEY = 'loom.udsReadDid.v1';
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const pad = (n, w) => String(n).padStart(w || 2, '0');
  const fmtT = t => { const x = new Date(t); return `${pad(x.getHours())}:${pad(x.getMinutes())}:${pad(x.getSeconds())}.${pad(x.getMilliseconds(), 3)}`; };

  /* ---------- persisted prefs (own key only; never history, never link state) ----------
     schema 2 = v1 top-level prefs (unchanged, baseline-readable) + `config` (custom connection profiles).
     Unversioned v1 objects migrate with presets active. */
  const SCHEMA = 2;
  let prefsCorrupt = false;
  function loadPrefs() {
    let raw = null;
    try { raw = localStorage.getItem(PKEY); } catch (e) { return {}; }
    if (raw == null) return {};
    try { const o = JSON.parse(raw); if (o && typeof o === 'object' && !Array.isArray(o)) return o; } catch (e) { /* fallthrough */ }
    prefsCorrupt = true; return {};
  }
  const saved = loadPrefs();
  const S0 = saved.sim || {};
  const E = U.createEngine({ clock: window.__loomUdsClock });
  // Shared-host hooks (host.js builds window.LoomWorkbench on top). Core-owned; extensions use H only.
  const hook = { traceView: null, renderCbs: [], ctxCbs: [], prefs: new Map(), rendering: false, again: false };
  // Catalog (custom ECUs/DIDs + SIM fixture payloads) restores BEFORE profiles and saved.ecu validation. Additive key.
  const catLoad = E.catalog && (saved.schema == null || saved.schema === SCHEMA) ? E.catalog.load(saved.catalog) : { ok: true, dropped: 0 };
  const st = {
    ecu: U.ECUS.some(e => e.id === saved.ecu) ? saved.ecu : 'BCM',
    transport: U.TRANSPORTS.some(t => t.id === saved.transport) ? saved.transport : 'doip',
    didText: typeof saved.didText === 'string' ? saved.didText.slice(0, 12) : 'F190',
    // Request mode + raw text: own prefs only (additive top-level keys; config/profiles untouched). Never auto-sent.
    // Oversize raw text (> RAW_TEXT_MAX, already rejected by the parser) is never truncated: it is simply not persisted.
    mode: saved.reqMode === 'raw' ? 'raw' : 'did',
    rawText: typeof saved.rawText === 'string' && saved.rawText.length <= U.RAW_TEXT_MAX ? saved.rawText : '22 F1 90',
    rawSubmitted: false,
    sim: {
      scenario: U.SCENARIOS.includes(S0.scenario) ? S0.scenario : 'positive',
      latency: U.LATENCIES.includes(+S0.latency) ? +S0.latency : 400,
      nrc: U.SIM_NRCS.includes(+S0.nrc) ? +S0.nrc : 0x22,
      pending: Math.max(1, Math.min(5, +S0.pending || 2)),
      late: !!S0.late,
    },
    filter: saved.filter === 'ecu' ? 'ecu' : 'all',
    histH: typeof saved.histH === 'number' && saved.histH >= 15 && saved.histH <= 80 ? saved.histH : null,
    didCollapsed: !!saved.didCollapsed, valsCollapsed: !!saved.valsCollapsed,
    routeInfoOpen: !!saved.routeInfoOpen, simOpen: !!saved.simOpen,
    selTxn: null, selEv: null, selEvAuto: true, selField: null, didSearch: '', submitted: false,
  };
  function savePrefs() {
    const extra = {};
    hook.prefs.forEach((fn, k) => { try { extra[k] = fn(); } catch (e) { /* section isolation */ } });
    try {
      localStorage.setItem(PKEY, JSON.stringify({ ...extra, schema: SCHEMA, ecu: st.ecu, transport: st.transport, didText: st.didText, reqMode: st.mode, rawText: st.rawText.length <= U.RAW_TEXT_MAX ? st.rawText : undefined, sim: st.sim, filter: st.filter, histH: st.histH, didCollapsed: st.didCollapsed, valsCollapsed: st.valsCollapsed, routeInfoOpen: st.routeInfoOpen, simOpen: st.simOpen, config: E.exportConfig(), catalog: E.catalog ? E.catalog.exportStore() : undefined }));
      return true;
    } catch (e) { return false; /* storage unavailable/quota: prefs are a convenience; project commit checks this */ }
  }

  // Restore custom profiles (boot only; everything offline). Invalid stored entries are dropped, never applied.
  const schemaIn = saved.schema == null ? 1 : saved.schema;
  const cfgLoad = schemaIn === SCHEMA ? E.loadConfig(saved.config) : { ok: true, dropped: 0, loaded: 0 };
  st.cfgNotice = prefsCorrupt ? '저장된 설정이 손상되어 기본값 사용 (프리셋)' :
    schemaIn !== 1 && schemaIn !== SCHEMA ? `알 수 없는 설정 schema ${esc(String(schemaIn))} — 사용자 프로필 무시, 프리셋 사용` :
    cfgLoad.dropped ? `저장된 사용자 프로필/선택 ${cfgLoad.dropped}개가 검증에 실패해 무시됨 — 해당 경로는 프리셋 사용` :
    catLoad.dropped ? `저장된 사용자 ECU/DID ${catLoad.dropped}개가 검증에 실패해 무시됨 — 프리셋 유지` : '';
  st.cfgMigrated = schemaIn === 1 && Object.keys(saved).length > 0;
  const now = () => (window.__loomUdsClock ? window.__loomUdsClock.now() : Date.now());
  const curCtx = () => E.ctx(st.ecu, st.transport);
  const curRoute = () => E.routeFor(st.ecu, st.transport);
  const trLabel = id => (U.TRANSPORTS.find(t => t.id === id) || {}).label || id;

  function toast(msg, kind) {
    const t = d.createElement('div'); t.className = 'toast' + (kind ? ' ' + kind : ''); t.textContent = msg;
    $('toasts').appendChild(t); setTimeout(() => t.remove(), 3200);
  }

  /* ---------- context strip ---------- */
  const SC_LABEL = { positive: '정상', nrc: 'NRC', pending: 'Pending→정상', timeout: '타임아웃', disconnect: '연결 끊김' };
  function simSummary() {
    const s = st.sim;
    if (s.scenario === 'nrc') return `NRC 0x${U.hex2(s.nrc)}`;
    if (s.scenario === 'pending') return `0x78×${s.pending}→정상`;
    if (s.scenario === 'timeout') return '타임아웃' + (s.late ? '+늦은응답' : '');
    if (s.scenario === 'disconnect') return '연결 끊김';
    return `정상 ${s.latency} ms`;
  }
  let catSig = '';
  function buildCatalog() {
    const sig = U.ECUS.map(e => e.id + ':' + e.name).join(',') + '/' + U.DIDS.map(x => x.did + ':' + x.name).join(',');
    if (sig === catSig) return;
    catSig = sig;
    $('selEcu').innerHTML = U.ECUS.map(e => `<option value="${esc(e.id)}">${esc(e.id)}${e.custom ? ' *' : ''}</option>`).join('');
    $('selEcu').value = st.ecu;
    $('didQuick').innerHTML = '<option value="">빠른 선택…</option>' + U.DIDS.map(x => `<option value="${U.hex4(x.did)}">${U.hex4(x.did)} · ${esc(x.name)}</option>`).join('');
  }
  function buildStatic() {
    $('segRoute').innerHTML = U.TRANSPORTS.map(t => `<button type="button" role="radio" data-tr="${t.id}" aria-checked="false">${t.label}</button>`).join('');
    buildCatalog();
    $('didInput').value = st.didText;
    $('rawInput').value = st.rawText;
  }
  function renderCtx() {
    if (!U.ECUS.some(x => x.id === st.ecu)) { st.ecu = U.ECUS[0].id; notifyCtx(); }
    buildCatalog();
    const e = U.ECUS.find(x => x.id === st.ecu), r = curRoute(), c = curCtx();
    $('selEcu').value = st.ecu;
    $('ecuName').textContent = e.name + (e.custom ? ' (사용자 SIM ECU)' : ' (fixture)');
    $('segRoute').querySelectorAll('button').forEach(b => {
      const on = b.dataset.tr === st.transport, busy = !!E.ctx(st.ecu, b.dataset.tr).active;
      b.setAttribute('aria-checked', on ? 'true' : 'false');
      b.tabIndex = on ? 0 : -1;
      b.innerHTML = esc(trLabel(b.dataset.tr)) + (busy ? '<span class="busy-dot" title="진행 중 요청 있음"></span>' : '');
      const rb = E.routeFor(st.ecu, b.dataset.tr);
      b.title = `${trLabel(b.dataset.tr)} — ${rb.summary} · ${rb.ptag} (mock 경로)`;
    });
    $('routeSum').textContent = r.summary;
    const pv = $('cfgProv');
    pv.textContent = r.ptag;
    pv.className = 'cfg-prov' + (r.profile.source === 'custom' ? ' custom' : '');
    pv.title = r.profile.source === 'custom' ? `사용자 프로필 ${r.profile.name} · rev ${r.profile.rev} · 기반 ${r.profile.base}` + (c.link === 'online' ? ' · 연결 시점 스냅샷' : '') : `내장 프리셋 ${r.profile.id} (변경 불가)`;
    const ls = $('linkSt');
    if (c.link === 'online') { ls.className = 'link-st on'; ls.textContent = `연결됨 · gen ${c.gen}`; }
    else if (c.cause === 'sim') { ls.className = 'link-st lost'; ls.textContent = '끊김 (SIM)'; }
    else { ls.className = 'link-st off'; ls.textContent = '연결 안 됨'; }
    const bl = $('btnLink');
    bl.innerHTML = c.link === 'online' ? '<svg class="i"><use href="#i-unplug"/></svg>연결 해제' : '<svg class="i"><use href="#i-plug"/></svg>연결';
    bl.title = c.link === 'online' ? '이 ECU·경로의 SIM 링크 해제 (진행 중 요청은 링크 끊김으로 종료)' : '이 ECU·경로의 SIM 링크 연결 — 요청을 보내지 않음';
    $('simSummary').textContent = simSummary();
    $('btnRouteInfo').setAttribute('aria-expanded', st.routeInfoOpen ? 'true' : 'false');
    $('routeInfo').hidden = !st.routeInfoOpen;
    if (st.routeInfoOpen) renderRouteInfo(r);
    $('btnSim').setAttribute('aria-expanded', st.simOpen ? 'true' : 'false');
    $('simPanel').hidden = !st.simOpen;
    const chip = $('shellConn');
    chip.className = 'conn-chip' + (c.link === 'online' ? ' live' : '') + (E.activeCount() ? ' busy' : '');
    chip.innerHTML = `<span class="dot"></span><span><b>${esc(st.ecu)}</b> · ${esc(r.label)} · ${c.link === 'online' ? '연결됨' : '연결 안 됨'}</span>`;
    $('stTarget').innerHTML = `대상 <b>${esc(st.ecu)}</b> · ${esc(r.label)} ${esc(r.summary)}`;
    $('stExtra').textContent = `진행 중 ${E.activeCount()} · 기록 ${E.txns.length}/${E.limit} · SIM ${simSummary()}`;
  }
  const ROUTE_NOTE = {
    doip: '<b>Mock DoIP 경로.</b> 실제 TCP 소켓·routing activation 없음. (현재 native Loom UDS가 지원하는 전송은 DoIP뿐이며, 이 목업은 그 백엔드에 연결되지 않습니다.) ETH1은 별도 <a href="../../ethernet-workbench/opus/index.html">Ethernet 모듈</a> 목업에서 볼 수 있으나 캡처가 이 UDS 목업과 연동되지 않습니다.',
    cantp: '<b>Mock CAN-TP 경로.</b> UDS 수준 응답만 시뮬레이션합니다. ISO-TP 분할·흐름 제어 프레임이나 CAN 버스 캡처를 생성하지 않습니다. native Loom UDS는 CAN-TP 미지원.',
    ethdiag: '<b>벤더 Ethernet 진단 어댑터 (mock).</b> ISO 13400 DoIP가 아니며 프레이밍·주소 체계를 정의하지 않습니다. 바이트 패널은 UDS PDU만 표시합니다. native 미지원. ETH1: <a href="../../ethernet-workbench/opus/index.html">Ethernet 모듈</a> (연동 없음).',
  };
  function renderRouteInfo(r) {
    const c = curCtx();
    const rows = r.fields.concat([['프로필', `${r.ptag}${c.link === 'online' ? ' · 연결 gen ' + c.gen + ' 스냅샷' : ''}`], ['클라이언트 P2 / P2*', `${r.timing.p2} / ${r.timing.p2star} ms`]]);
    $('routeInfo').innerHTML = `<dl class="ri-kv">${rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl><p class="ri-note">${ROUTE_NOTE[r.transport]} SIM ECU는 설정한 주소와 관계없이 이 컨텍스트의 fixture ECU로 응답합니다 (주소 라우팅 모델링 안 함).</p>`;
  }

  /* ---------- request editor ---------- */
  const sidLabel = s => `0x${U.hex2(s)}` + (U.sidName(s) ? ' ' + U.sidName(s) : '');
  function renderMode() {
    const raw = st.mode === 'raw', f = $('req');
    f.classList.toggle('m-raw', raw); f.classList.toggle('m-did', !raw);
    $('segMode').querySelectorAll('button').forEach(b => { const on = b.dataset.mode === st.mode; b.setAttribute('aria-checked', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1; });
    $('btnReadLabel').textContent = raw ? '전송' : '읽기';
    $('btnRead').title = raw ? 'Raw PDU 전송 (입력란에서 Ctrl/⌘+Enter)' : '요청 전송 (DID 입력란에서 Enter)';
  }
  function renderRaw(c, active) {
    const p = U.parseRawHex(st.rawText), inp = $('rawInput');
    const showErr = !p.ok && (st.rawSubmitted || st.rawText.trim().length > 0);
    inp.classList.toggle('invalid', showErr);
    inp.setAttribute('aria-invalid', showErr ? 'true' : 'false');
    $('rawSum').innerHTML = p.ok ? `<span class="k">SID</span><code>0x${U.hex2(p.sid)}</code><small>${esc(U.sidName(p.sid) || '카탈로그에 없는 SID')} · ${p.len} B</small>` : '<span class="k">SID</span><code class="none">—</code><small>0 B</small>';
    $('rawPdu').innerHTML = p.ok ? `<code>${p.norm}</code><small>${p.len} B · 입력 그대로 (SID 포함)</small>` : `<span class="none">${showErr ? '유효하지 않음 — 부분 해석·전송 없음' : '—'}</span>`;
    let cls = '', text = '';
    if (showErr) { cls = 'err'; text = p.error + ' — 전송하지 않음'; }
    else if (c.link !== 'online') { cls = 'warn'; text = '이 ECU·경로가 연결되어 있지 않습니다. [연결] 후 전송.'; }
    else if (active) { cls = 'warn'; text = `요청 #${active.no} 진행 중 (${E.statusText(active)}) — 완료/취소 후 같은 대상·경로에 다시 전송. 다른 경로는 독립적으로 사용 가능.`; }
    else if (p.ok) text = 'Ctrl/⌘+Enter 또는 [전송]으로 이 바이트 그대로 전송. Enter는 줄바꿈.' + (p.sid !== 0x22 ? ' mock ECU는 0x22 외 서비스에 NRC 0x11 (serviceNotSupported, mock)로 응답 — 실제 ECU 동작 아님.' : U.rawDidOf(p.bytes) == null ? ' mock ECU는 단일 DID 3 B 형식이 아닌 0x22에 NRC 0x13 (mock)로 응답.' : '');
    return { cls, text };
  }
  function renderReq() {
    renderMode();
    const r = curRoute(), c = curCtx(), v = U.validateDid(st.didText);
    const active = c.active ? E.byId(c.active) : null;
    $('reqTarget').innerHTML = `요청 대상 <b>${esc(st.ecu)}</b> · <b>${esc(r.label)}</b> ${esc(r.summary)} · <span class="ptag">${esc(r.ptag)}</span> · P2 ${r.timing.p2}/${r.timing.p2star} ms · ` + (c.link === 'online' ? `연결됨 (gen ${c.gen})` : '<span class="off">연결 안 됨 — 위의 [연결]</span>');
    const inp = $('didInput');
    const showErr = !v.ok && (st.submitted || st.didText.length > 0);
    inp.classList.toggle('invalid', showErr);
    inp.setAttribute('aria-invalid', showErr ? 'true' : 'false');
    const info = v.ok ? U.didInfo(v.did) : null;
    $('didName').textContent = v.ok ? (info ? info.name + (info.custom ? ` (사용자 SIM 정의 · ${info.recLen} B)` : info.synthetic ? ' (synthetic)' : '') : '카탈로그에 없는 DID — 원시 데이터로 표시') : '';
    $('reqPdu').innerHTML = v.ok ? `<span class="k">요청 PDU</span><code>${U.hexs(U.buildRequest(v.did))}</code><small>3 B</small>` : '<span class="k">요청 PDU</span><code class="none">—</code>';
    const msg = $('didMsg');
    let cls = '', text = '';
    if (st.mode === 'raw') ({ cls, text } = renderRaw(c, active));
    else if (showErr) { cls = 'err'; text = v.error + ' — 전송하지 않음'; }
    else if (c.link !== 'online') { cls = 'warn'; text = '이 ECU·경로가 연결되어 있지 않습니다. [연결] 후 읽기.'; }
    else if (active) { cls = 'warn'; text = `요청 #${active.no} 진행 중 (${E.statusText(active)}) — 완료/취소 후 같은 대상·경로에 다시 읽기. 다른 경로는 독립적으로 사용 가능.`; }
    else if (v.ok) text = 'Enter 또는 [읽기]로 전송. 선택만으로는 전송하지 않습니다.';
    msg.className = 'req-msg' + (cls ? ' ' + cls : ''); msg.textContent = text;
    $('btnRead').disabled = c.link !== 'online' || !!active;
    const bc = $('btnCancel');
    bc.disabled = !active;
    $('btnCancelLabel').textContent = active ? `취소 #${active.no}` : '취소';
    $('didQuick').value = v.ok && U.didInfo(v.did) ? U.hex4(v.did) : '';
  }
  function setMode(m, opts) {
    if (m !== 'did' && m !== 'raw') return;
    st.mode = m; savePrefs(); renderReq();
    if (opts && opts.focusSeg) $('segMode').querySelector(`[data-mode="${m}"]`).focus();
  }
  function setDid(text, opts) {
    if (st.mode !== 'did') st.mode = 'did'; // explicit DID pick → DID mode; raw text is kept untouched
    st.didText = text; $('didInput').value = text; st.submitted = false;
    savePrefs(); renderReq(); renderDidList();
    if (opts && opts.focus) $('didInput').focus();
  }
  function doRead() {
    st.submitted = true;
    const res = E.read({ ecu: st.ecu, transport: st.transport, didText: st.didText, sim: st.sim });
    if (!res.ok) {
      renderReq();
      if (res.code === 'invalid') $('didInput').focus();
      else toast(res.error, 'err');
      return null;
    }
    st.submitted = false;
    selectTxn(res.txn.id, { scroll: true });
    render();
    return res.txn;
  }

  // Raw send: exact parsed bytes; invalid input → error + focus, zero TX.
  function doSend() {
    st.rawSubmitted = true;
    const res = E.sendRaw({ ecu: st.ecu, transport: st.transport, rawText: st.rawText, sim: st.sim });
    if (!res.ok) {
      renderReq();
      if (res.code === 'invalid') $('rawInput').focus();
      else toast(res.error, 'err');
      return null;
    }
    st.rawSubmitted = false;
    selectTxn(res.txn.id, { scroll: true });
    render();
    return res.txn;
  }

  /* ---------- DID list ---------- */
  function renderDidList() {
    const q = st.didSearch.trim().toLowerCase().replace(/^0x/, '').replace(/\s/g, '');
    const v = U.validateDid(st.didText);
    const list = U.DIDS.filter(x => !q || x.name.toLowerCase().includes(q) || x.long.toLowerCase().includes(q) || U.hex4(x.did).toLowerCase().includes(q) || ('22' + U.hex4(x.did)).toLowerCase().includes(q));
    $('didList').innerHTML = list.length ? list.map(x => {
      const h = U.hex4(x.did), sel = v.ok && v.did === x.did;
      return `<li role="option" data-did="${h}" aria-selected="${sel}" id="did-${h}"><span class="raw">22 <b>${h.slice(0, 2)} ${h.slice(2)}</b></span><span class="nm">${esc(x.name)}</span><span class="sub">${esc(x.long)}</span></li>`;
    }).join('') : '<li class="none" role="presentation">일치하는 DID 없음 — 요청 편집기에 hex 직접 입력</li>';
  }

  /* ---------- history ---------- */
  function baseTxns() { return E.txns.filter(t => st.filter === 'all' || t.ecu === st.ecu); }
  function visibleTxns() {
    const base = baseTxns(), tv = hook.traceView;
    if (!tv || typeof tv.rows !== 'function') return base;
    try { const r = tv.rows(base); return Array.isArray(r) ? r.filter(x => x && x.id != null) : base; } catch (e) { return base; }
  }
  // Selected-detail resolution: trace view (frozen/paused rows) first, then live engine.
  function txById(id) {
    if (id == null) return null;
    const tv = hook.traceView;
    if (tv && typeof tv.byId === 'function') { try { const x = tv.byId(id); if (x) return x; } catch (e) { /* fall back */ } }
    return E.byId(id);
  }
  const ST_TEXT = tx => {
    if (tx.status === 'waiting') return '⏳ 응답 대기';
    if (tx.status === 'pending') return `⏳ 대기 0x78×${tx.pendingCount}`;
    if (tx.status === 'positive') return '✓ 긍정';
    if (tx.status === 'suppressed') return '✓ 응답 억제';
    if (tx.status === 'nrc') return `✗ NRC 0x${U.hex2(tx.nrc)}`;
    if (tx.status === 'timeout') return '⌛ 타임아웃';
    if (tx.status === 'cancelled') return '⛔ 취소';
    if (tx.status === 'link-lost') return '⚡ 링크 끊김';
    return tx.status;
  };
  const elapsed = tx => (tx.tEnd != null ? tx.tEnd : now()) - tx.t0;
  const reqTitle = tx => (tx.request.length > 64 ? U.hexs(tx.request.slice(0, 64)) + ` … (${tx.request.length} B)` : U.hexs(tx.request) + ` (${tx.request.length} B)`);
  function didCell(tx) {
    if (tx.kind !== 'raw') return `<span class="c-did" title="${esc(tx.didName || '')}">${tx.didHex} ${esc(tx.didName || '')}</span>`;
    const lab = tx.did != null ? `${tx.didHex} ${tx.didName || ''}` : sidLabel(tx.sid) + (tx.sid === 0x22 ? ' (형식 불일치)' : '');
    return `<span class="c-did" title="${esc('Raw UDS PDU · SID ' + sidLabel(tx.sid) + ' · ' + tx.request.length + ' B')}"><span class="rawtag">RAW</span> ${esc(lab.trim())}</span>`;
  }
  // Trace display may supply relative/absolute timestamps (B). Fallback: absolute wall-clock.
  function timeText(tx) {
    const tv = hook.traceView;
    if (tv && typeof tv.timeText === 'function') { try { const t = tv.timeText(tx); if (typeof t === 'string' && t) return t; } catch (e) { /* fallback */ } }
    return fmtT(tx.t0);
  }
  function rowHtml(tx) {
    const late = tx.events.some(ev => ev.kind === 'late');
    const unapplied = tx.status === 'positive' && tx.applied === false;
    const rsp = tx.final ? U.hexs(tx.final) : '';
    return `<div class="tx-row s-${tx.status}${tx.kind === 'raw' ? ' k-raw' : ''}${tx.id === st.selTxn ? ' sel' : ''}" role="row" id="tx-${tx.id}" data-id="${tx.id}" aria-selected="${tx.id === st.selTxn}">` +
      `<span class="c-no">${tx.no}</span><span class="c-t">${esc(timeText(tx))}</span><span class="c-ecu">${esc(tx.ecu)}</span>` +
      `<span class="c-rt" title="${esc(tx.route.label + ' ' + tx.route.summary + ' · ' + tx.route.ptag)}">${esc(tx.route.label)} <small>${esc(tx.route.summary.replace(/^\S+ · /, ''))}</small></span>` +
      didCell(tx) +
      `<span class="c-req" title="${esc(reqTitle(tx))}">${esc(tx.request.length > 64 ? U.hexs(tx.request.slice(0, 64)) + ' …' : U.hexs(tx.request))}</span>` +
      `<span class="c-rsp" title="${esc(rsp)}">${rsp ? esc(rsp) : '<span class="na">—</span>'}</span>` +
      `<span class="c-st">${ST_TEXT(tx)}${unapplied ? '<span class="unapplied" title="' + esc(tx.appliedNote) + '">값 미반영</span>' : ''}${late ? '<span class="late" title="종료 후 도착한 응답 — 무시됨">late</span>' : ''}</span>` +
      `<span class="c-p">${tx.pendingCount || ''}</span><span class="c-el" data-el="${tx.id}">${elapsed(tx)} ms</span></div>`;
  }
  function renderHistory() {
    const vis = visibleTxns();
    if (st.selTxn != null && !vis.some(t => t.id === st.selTxn)) { st.selTxn = vis.length ? vis[vis.length - 1].id : null; st.selEv = null; st.selField = null; }
    const body = $('txBody');
    body.innerHTML = vis.map(rowHtml).join('');
    body.setAttribute('aria-rowcount', String(vis.length));
    if (st.selTxn != null) body.setAttribute('aria-activedescendant', 'tx-' + st.selTxn); else body.removeAttribute('aria-activedescendant');
    const empty = $('txEmpty');
    empty.hidden = vis.length > 0;
    if (!vis.length) empty.innerHTML = E.txns.length ? `현재 ECU(${esc(st.ecu)})의 기록 없음 — 범위 [전체]에서 다른 ECU 기록 확인` : '아직 요청 없음 (자동 전송·재전송 없음).<ol><li>ECU와 경로 선택 후 [연결]</li><li>DID 선택 또는 입력 (요청 PDU 확인) — 또는 [Raw]에서 hex PDU 입력</li><li>[읽기] / [전송]</li></ol>';
    $('segFilter').querySelectorAll('button').forEach(b => b.setAttribute('aria-checked', b.dataset.filter === st.filter ? 'true' : 'false'));
    $('histCount').textContent = `${vis.length}${st.filter === 'ecu' ? '/' + E.txns.length : ''} · 진행 ${E.activeCount()}`;
  }
  function scrollRowIntoView(id) {
    const row = $('tx-' + id), sc = $('txScroll');
    if (!row || !sc) return;
    const top = row.offsetTop, h = row.offsetHeight, head = 24;
    if (top - head < sc.scrollTop) sc.scrollTop = top - head;
    else if (top + h > sc.scrollTop + sc.clientHeight) sc.scrollTop = top + h - sc.clientHeight;
  }
  function defaultEv(tx) {
    for (let i = tx.events.length - 1; i >= 0; i--) if (tx.events[i].bytes && tx.events[i].kind !== 'late') return i;
    return 0;
  }
  function selectTxn(id, opts) {
    st.selTxn = id; const tx = id != null ? txById(id) : null;
    st.selEv = tx ? defaultEv(tx) : null; st.selEvAuto = true; st.selField = null;
    renderHistory(); renderDetail();
    if (opts && opts.scroll && id != null) scrollRowIntoView(id);
  }

  /* ---------- detail ---------- */
  const DIR = { tx: 'Tx', pending: 'Rx', positive: 'Rx', nrc: 'Rx', late: 'Rx', timeout: '—', cancel: '—', link: '—' };
  function renderDetail() {
    const tx = st.selTxn != null ? txById(st.selTxn) : null;
    if (!tx) {
      $('seqHead').textContent = ''; $('fieldsHead').textContent = '';
      $('seq').innerHTML = '<li class="empty">트랜잭션을 선택하면 Tx·0x78·최종 응답·타임아웃/취소 순서가 표시됩니다.</li>';
      $('fields').innerHTML = '<div class="empty">선택한 트랜잭션의 요청·응답 해석이 여기에 표시됩니다.</div>';
      $('hex').innerHTML = '<div class="empty">UDS PDU 바이트</div>';
      return;
    }
    if (st.selEvAuto || st.selEv == null || !tx.events[st.selEv]) { const di = defaultEv(tx); if (di !== st.selEv) st.selField = null; st.selEv = di; }
    $('seqHead').textContent = `#${tx.no} · ${tx.ecu} · ${tx.route.label}` + (tx.kind === 'raw' ? ` · RAW 0x${U.hex2(tx.sid)}` : '');
    $('seq').innerHTML = tx.events.map((ev, i) => {
      const dt = ev.t - tx.t0;
      return `<li class="k-${ev.kind}${ev.bytes ? '' : ' nobytes'}" role="option" data-i="${i}" id="ev-${i}" aria-selected="${i === st.selEv}">` +
        `<span class="dt">+${dt}</span><span class="dir">${DIR[ev.kind] || ''}</span><span class="lb">${esc(ev.label)}</span>` +
        (ev.bytes ? `<span class="by">${U.hexs(ev.bytes)}</span>` : '') + (ev.note ? `<span class="nt">${esc(ev.note)}</span>` : '') + '</li>';
    }).join('');
    $('seq').setAttribute('aria-activedescendant', 'ev-' + st.selEv);
    renderFields(tx); renderHex(tx);
  }
  const decOf = (tx, ev) => U.decodePdu(ev.bytes, { did: tx.did, sid: tx.sid, kind: tx.kind, dir: ev.kind === 'tx' ? 'tx' : 'rx', def: tx.didDef });
  const reqText = tx => (tx.kind === 'raw'
    ? `Raw UDS PDU · SID ${sidLabel(tx.sid)} · ${tx.request.length} B · 입력 그대로 (SID 자동 추가·패딩 없음)` + (tx.did != null ? ` · 단일 DID ${tx.didHex}${tx.didName ? ' ' + tx.didName : ''} 형식` : '')
    : `Read DID (0x22) · DID ${tx.didHex}${tx.didName ? ' ' + tx.didName : ''}`);
  function selPdu(tx) { const ev = tx.events[st.selEv]; return ev && ev.bytes ? ev : tx.events[0]; }
  function renderFields(tx) {
    const ev = selPdu(tx), dec = decOf(tx, ev);
    $('fieldsHead').textContent = ev.label + (ev.ignored ? ' (무시됨)' : '');
    const snap = [['트랜잭션', `#${tx.no} · ${fmtT(tx.t0)} · ${E.statusText(tx)}`], ['요청', reqText(tx)], ['ECU', `${tx.ecu} — ${tx.ecuName}`], ['경로 (요청 시점 고정)', `${tx.route.label} · ${tx.route.fields.map(f => f[1]).join(' · ')}`], ['연결 프로필', `${tx.route.ptag} · ${tx.profile.id}${tx.profile.source === 'custom' ? ' (기반 ' + tx.profile.base + ')' : ''}`], ['클라이언트 타이밍', `P2 ${tx.timing.p2} ms · P2* ${tx.timing.p2star} ms`], ['연결 세대', 'gen ' + tx.gen], ['SIM 시나리오', simText(tx.sim)]];
    let html = `<dl class="fsnap">${snap.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl>`;
    html += '<table class="ftab"><thead><tr><th>이름</th><th>오프셋</th><th>값</th><th>설명</th></tr></thead><tbody>';
    html += `<tr class="pdu-row"><td>${ev.kind === 'tx' ? 'REQUEST_PDU' : 'RESPONSE_PDU'}</td><td></td><td class="v">${ev.bytes.length > 64 ? U.hexs(ev.bytes.slice(0, 64)) + ' … (바이트 패널 참조)' : U.hexs(ev.bytes)}</td><td>${ev.bytes.length} B</td></tr>`;
    dec.fields.forEach((f, i) => {
      html += `<tr class="fd${f.decoded ? ' dec' : ''}${st.selField === i ? ' on' : ''}" data-f="${i}"><td>${esc(f.name)}</td><td class="o">${f.len ? f.off + (f.len > 1 ? '–' + (f.off + f.len - 1) : '') : '—'}</td><td class="v${f.bad ? ' bad' : ''}">${esc(f.value)}${f.unit ? ' ' + esc(f.unit) : ''}</td><td class="n${f.bad ? ' bad' : ''}">${esc(f.note || '')}</td></tr>`;
    });
    html += '</tbody></table>';
    html += appliedBox(tx, ev);
    $('fields').innerHTML = html;
  }
  function simText(s) {
    if (s.scenario === 'positive') return `정상 · 지연 ${s.latency} ms`;
    if (s.scenario === 'nrc') return `NRC 0x${U.hex2(s.nrc)} · 지연 ${s.latency} ms`;
    if (s.scenario === 'pending') return `0x78 ×${s.pending} (지연 ${s.latency} ms, 간격 ${s.pendingInterval} ms) → 최종`;
    if (s.scenario === 'timeout') return '응답 없음' + (s.late ? ` · 늦은 응답 P2+${s.lateAfter} ms` : '');
    return `링크 끊김 @ ${s.latency} ms`;
  }
  function appliedBox(tx, ev) {
    if (ev.ignored) return '<div class="applied no">무시된 늦은 응답 — 이 트랜잭션 상태와 확정 값에 반영되지 않음</div>';
    if (tx.status === 'suppressed') return '<div class="applied none">응답 억제 (SPRMIB) — 긍정 응답 바이트 없음, 확정 값과 무관</div>';
    if (tx.kind === 'raw' && tx.did == null) return `<div class="applied none">Raw 요청 SID ${esc(sidLabel(tx.sid))} — DID 확정 값과 무관, 갱신 없음</div>`;
    if (tx.status === 'positive') return `<div class="applied ${tx.applied ? 'yes' : 'no'}">${esc(tx.appliedNote)}</div>`;
    if (tx.status === 'waiting' || tx.status === 'pending') return '<div class="applied none">진행 중 — 최종 긍정 응답 전에는 확정 값을 바꾸지 않음</div>';
    return `<div class="applied none">${esc(E.statusText(tx))} — 확정 값 갱신 없음 (이전 값이 있으면 이전 출처 그대로 유지)</div>`;
  }
  function renderHex(tx) {
    const ev = selPdu(tx), b = ev.bytes;
    const dec = decOf(tx, ev), f = st.selField != null ? dec.fields[st.selField] : null;
    const on = i => f && i >= f.off && i < f.off + f.len;
    let html = `<div class="hx-cap">${esc(ev.label)} · ${b.length} B · UDS PDU (CAN-TP/DoIP/EthDiag 전송 프레임 아님)</div>`;
    for (let o = 0; o < b.length; o += 16) {
      const row = b.slice(o, o + 16);
      html += `<div class="hx-row"><span class="hx-off">${o.toString(16).padStart(4, '0')}</span><span class="hx-h">${row.map((x, j) => `<b data-o="${o + j}"${on(o + j) ? ' class="on"' : ''}>${U.hex2(x)}</b>`).join('')}</span><span class="hx-a">${row.map((x, j) => `<b data-o="${o + j}" class="${x >= 0x20 && x < 0x7f ? '' : 'np'}${on(o + j) ? ' on' : ''}">${x >= 0x20 && x < 0x7f ? esc(String.fromCharCode(x)) : '.'}</b>`).join('')}</span></div>`;
    }
    $('hex').innerHTML = html;
  }

  /* ---------- confirmed values ---------- */
  const lastShown = new Map();
  function renderVals() {
    $('valsEcu').textContent = st.ecu;
    const keys = U.DIDS.map(x => x.did);
    E.confirmed.forEach(cv => { if (cv.ecu === st.ecu && !keys.includes(cv.did)) keys.push(cv.did); });
    $('valsBody').innerHTML = keys.map(did => {
      const info = U.didInfo(did), cv = E.confirmed.get(st.ecu + '|' + did);
      const latest = E.txns.filter(t => t.ecu === st.ecu && t.did === did).reduce((a, t) => (!a || t.no > a.no ? t : a), null);
      let warn = '', inflight = '';
      if (latest && !E.terminal(latest.status)) inflight = `<div class="cv-i">읽는 중 #${latest.no} (${esc(latest.route.label)})</div>`;
      else if (latest && latest.status !== 'positive' && (!cv || latest.no > cv.no)) warn = `<div class="cv-w">최근 #${latest.no} ${esc(ST_TEXT(latest))} — ${cv ? '아래는 이전 값' : '확정 값 없음'}</div>`;
      const flash = cv && lastShown.has(did) && lastShown.get(did) !== cv.no;
      if (cv) lastShown.set(did, cv.no);
      const name = info ? info.name : 'DID ' + U.hex4(did);
      return `<div class="cv${flash ? ' flash' : ''}" data-did="${U.hex4(did)}"><div class="cv-h"><b>${U.hex4(did)}</b><span>${esc(name)}</span></div>` +
        (cv ? `<div class="cv-v">${esc(cv.value)}${cv.unit ? ' ' + esc(cv.unit) : ''}</div><div class="cv-p">출처 <a data-goto="${cv.txnId}" title="출처 트랜잭션 선택">#${cv.no}</a> · ${fmtT(cv.t)} · ${esc(cv.routeLabel)} ${esc(cv.routeSummary)}${cv.profile && cv.profile.source === 'custom' ? ' · ' + esc(cv.ptag) : ''}${cv.kind === 'raw' ? ' · <span class="rawtag">RAW</span>' : ''}</div>` : '<div class="cv-v none">확정 값 없음</div>') +
        inflight + warn + '</div>';
    }).join('');
  }

  /* ---------- render loop ---------- */
  let pending = false;
  function render() {
    if (hook.rendering) { hook.again = true; return; } // re-entrant refresh from an onRender callback → one follow-up pass, never a loop
    pending = false; hook.rendering = true;
    try {
      renderCtx(); renderReq(); renderDidList(); renderHistory(); renderDetail(); renderVals(); if (window.LoomUdsTools) window.LoomUdsTools.render(); if (window.LoomUdsConfig) window.LoomUdsConfig.render();
      hook.renderCbs.slice().forEach(fn => { try { fn(); } catch (e) { /* extension isolation */ } });
    } finally { hook.rendering = false; }
    if (hook.again) { hook.again = false; schedule(); }
  }
  function notifyCtx() { const c = { ecu: st.ecu, transport: st.transport }; hook.ctxCbs.slice().forEach(fn => { try { fn(c); } catch (e) { /* isolation */ } }); }
  function schedule() { if (pending) return; pending = true; (window.requestAnimationFrame || setTimeout)(() => { if (pending) render(); }); }
  E.on(type => { if (type === 'config' || type === 'catalog') savePrefs(); schedule(); });
  const tickH = setInterval(() => {
    if (!E.activeCount()) return;
    // Elapsed via the displayed row (paused trace clones keep their frozen tEnd) while live requests continue.
    E.txns.forEach(tx => { if (!E.terminal(tx.status)) { const el = d.querySelector(`[data-el="${tx.id}"]`); if (el) el.textContent = elapsed(txById(tx.id) || tx) + ' ms'; } });
  }, 200);

  /* ---------- events ---------- */
  function setContext(ecuId, tr) {
    const okE = U.ECUS.some(x => x.id === ecuId), okT = U.TRANSPORTS.some(t => t.id === tr);
    if ((ecuId != null && !okE) || (tr != null && !okT)) return false;
    const before = st.ecu + '|' + st.transport;
    if (ecuId != null) st.ecu = ecuId;
    if (tr != null) st.transport = tr;
    savePrefs(); render();
    if (before !== st.ecu + '|' + st.transport) notifyCtx();
    return true;
  }
  $('selEcu').addEventListener('change', e => { setContext(e.target.value, null); });
  $('segRoute').addEventListener('click', e => { const b = e.target.closest('button[data-tr]'); if (!b) return; setContext(null, b.dataset.tr); });
  $('segRoute').addEventListener('keydown', e => {
    if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    e.preventDefault();
    const ids = U.TRANSPORTS.map(t => t.id), i = ids.indexOf(st.transport);
    setContext(null, ids[(i + (e.key === 'ArrowRight' ? 1 : ids.length - 1)) % ids.length]); $('segRoute').querySelector(`[data-tr="${st.transport}"]`).focus();
  });
  $('btnLink').addEventListener('click', () => {
    const c = curCtx();
    if (c.link === 'online') E.disconnect(st.ecu, st.transport, 'user'); else E.connect(st.ecu, st.transport);
    render();
  });
  $('btnRouteInfo').addEventListener('click', () => { st.routeInfoOpen = !st.routeInfoOpen; savePrefs(); renderCtx(); });
  $('btnSim').addEventListener('click', () => { st.simOpen = !st.simOpen; savePrefs(); renderCtx(); if (window.LoomUdsTools) window.LoomUdsTools.render(); });
  $('didInput').addEventListener('input', e => { st.didText = e.target.value; st.submitted = false; savePrefs(); renderReq(); renderDidList(); });
  $('req').addEventListener('submit', e => { e.preventDefault(); if (st.mode === 'raw') doSend(); else doRead(); });
  $('rawInput').addEventListener('input', e => { st.rawText = e.target.value; st.rawSubmitted = false; savePrefs(); renderReq(); });
  $('rawInput').addEventListener('keydown', e => {
    // Only Ctrl/⌘+Enter sends. Plain Enter = newline (textarea default). IME composition ignored.
    if (e.key !== 'Enter' || !(e.ctrlKey || e.metaKey) || e.isComposing) return;
    e.preventDefault(); doSend();
  });
  $('segMode').addEventListener('click', e => { const b = e.target.closest('button[data-mode]'); if (b) setMode(b.dataset.mode); });
  $('segMode').addEventListener('keydown', e => {
    if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    e.preventDefault(); setMode(st.mode === 'raw' ? 'did' : 'raw', { focusSeg: true });
  });
  $('btnCancel').addEventListener('click', () => { const r = E.cancel(st.ecu, st.transport); if (!r.ok) toast(r.error, 'err'); render(); });
  $('didQuick').addEventListener('change', e => { if (e.target.value) setDid(e.target.value, { focus: true }); });
  $('didSearch').addEventListener('input', e => { st.didSearch = e.target.value; renderDidList(); });
  $('didList').addEventListener('click', e => { const li = e.target.closest('li[data-did]'); if (li) setDid(li.dataset.did); });
  $('didList').addEventListener('keydown', e => {
    const items = [...$('didList').querySelectorAll('li[data-did]')];
    if (!items.length) return;
    let i = items.findIndex(li => li.getAttribute('aria-selected') === 'true');
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); i = e.key === 'ArrowDown' ? Math.min(items.length - 1, i + 1) : Math.max(0, i - 1); setDid(items[i].dataset.did); }
    else if (e.key === 'Enter') { e.preventDefault(); if (i >= 0) setDid(items[i].dataset.did, { focus: true }); }
  });
  $('segFilter').addEventListener('click', e => { const b = e.target.closest('button[data-filter]'); if (!b) return; st.filter = b.dataset.filter; savePrefs(); renderHistory(); renderDetail(); });
  $('txBody').addEventListener('click', e => { const r = e.target.closest('.tx-row'); if (r) selectTxn(+r.dataset.id); });
  $('txBody').addEventListener('keydown', e => {
    const vis = visibleTxns(); if (!vis.length) return;
    let i = vis.findIndex(t => t.id === st.selTxn);
    const map = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: vis.length - 1, PageDown: i + 10, PageUp: i - 10 };
    if (!(e.key in map)) return;
    e.preventDefault();
    i = Math.max(0, Math.min(vis.length - 1, i < 0 && e.key === 'ArrowUp' ? vis.length - 1 : map[e.key]));
    selectTxn(vis[i].id, { scroll: true });
  });
  $('seq').addEventListener('click', e => { const li = e.target.closest('li[data-i]'); if (!li) return; const tx = txById(st.selTxn); if (!tx || !tx.events[+li.dataset.i].bytes) return; st.selEv = +li.dataset.i; st.selEvAuto = false; st.selField = null; renderDetail(); });
  $('seq').addEventListener('keydown', e => {
    const tx = txById(st.selTxn); if (!tx) return;
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    const step = e.key === 'ArrowDown' ? 1 : -1;
    for (let i = st.selEv + step; i >= 0 && i < tx.events.length; i += step) if (tx.events[i].bytes) { st.selEv = i; st.selEvAuto = false; st.selField = null; renderDetail(); break; }
  });
  $('fields').addEventListener('click', e => { const tr = e.target.closest('tr.fd'); if (!tr) return; const i = +tr.dataset.f; st.selField = st.selField === i ? null : i; const tx = txById(st.selTxn); if (!tx) return; renderFields(tx); renderHex(tx); });
  $('hex').addEventListener('click', e => {
    const b = e.target.closest('b[data-o]'); const tx = txById(st.selTxn); if (!b || !tx) return;
    const o = +b.dataset.o, dec = decOf(tx, selPdu(tx));
    const i = dec.fields.findIndex(f => o >= f.off && o < f.off + f.len);
    st.selField = i >= 0 ? i : null; renderFields(tx); renderHex(tx);
  });
  $('valsBody').addEventListener('click', e => {
    const a = e.target.closest('a[data-goto]'); if (!a) return;
    const id = +a.dataset.goto;
    if (!E.byId(id)) { toast('출처 트랜잭션이 기록에서 제거됨 (값의 출처 정보는 유지)'); return; }
    if (!visibleTxns().some(t => t.id === id)) { st.filter = 'all'; savePrefs(); }
    selectTxn(id, { scroll: true });
  });

  buildStatic();
  render();

  window.loomUdsOpus = { hook, saved, txById, baseTxns, setContext, buildCatalog, tickH, esc, trLabel, curRoute, renderReq, renderCtx, renderHistory, renderDetail, renderVals, E, st, render, flush: () => render(), selectTxn, doRead, doSend, setMode, setDid, savePrefs, toast, curCtx, visibleTxns, elapsed, fmtT, ST_TEXT, simSummary, schedule };
})();
