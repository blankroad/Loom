/* Loom UDS Read DID — flexible layout (worker B, item 6): column width handles (DID|center, center|values,
   detail seq|fields|bytes), maximize/restore history or detail, reset, named layouts. Pure presentation:
   only CSS custom properties / classes on #wsp and #detail + own handle elements; never sends or connects.
   The core vertical history/detail divider (#divHist, core prefs histH) is left untouched. */
(function () {
  'use strict';
  const d = document, PKEY = 'loom.udsReadDid.layout.v1';
  const LIM = { did: [160, 420], vals: [220, 480], seq: [15, 45], bytes: [18, 45], midPct: 25, centerMin: 480, named: 12, name: 32 };
  const BP = { vals: 1181, detail: 1181, did: 861, mobile: 561 }; // min widths at which each control applies (matches core media queries)
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const EMPTY = () => ({ didW: null, valsW: null, seqPct: null, bytesPct: null, max: null });

  /* ---------- pure validation (project section + stored prefs) ---------- */
  function checkLayout(o, where) {
    if (!o || typeof o !== 'object' || Array.isArray(o)) return { error: `${where}: 객체 아님` };
    const out = EMPTY();
    const num = (k, r) => {
      const v = o[k]; if (v == null) return null;
      if (typeof v !== 'number' || !isFinite(v) || v < r[0] || v > r[1]) throw new Error(`${where}.${k}: ${r[0]}–${r[1]} 범위 숫자 또는 null`);
      return Math.round(v);
    };
    try {
      out.didW = num('didW', LIM.did); out.valsW = num('valsW', LIM.vals); out.seqPct = num('seqPct', LIM.seq); out.bytesPct = num('bytesPct', LIM.bytes);
      if (out.seqPct != null && out.bytesPct != null && out.seqPct + out.bytesPct > 100 - LIM.midPct) throw new Error(`${where}: 상세 좌/우 합 ${100 - LIM.midPct}% 초과`);
      if (o.max != null && o.max !== 'hist' && o.max !== 'detail') throw new Error(`${where}.max: hist|detail|null`);
      out.max = o.max || null;
    } catch (e) { return { error: e.message }; }
    return { value: out };
  }
  function validate(o) {
    if (!o || typeof o !== 'object' || Array.isArray(o)) return { ok: false, error: 'layout: 객체 아님' };
    if (o.v !== 1) return { ok: false, error: `layout: 알 수 없는 버전 ${esc(String(o.v))}` };
    const c = checkLayout(o.cur == null ? {} : o.cur, 'layout.cur'); if (c.error) return { ok: false, error: c.error };
    if (o.named != null && !Array.isArray(o.named)) return { ok: false, error: 'layout.named: 배열 아님' };
    const named = [], seen = new Set();
    for (const [i, x] of (o.named || []).entries()) {
      if (named.length >= LIM.named) return { ok: false, error: `layout.named: 최대 ${LIM.named}개` };
      if (!x || typeof x.name !== 'string') return { ok: false, error: `layout.named[${i}].name 문자열 아님` };
      const name = x.name.trim();
      if (!name || name.length > LIM.name) return { ok: false, error: `layout.named[${i}].name 1–${LIM.name}자` };
      if (seen.has(name)) return { ok: false, error: `layout.named: 이름 중복 "${esc(name)}"` };
      const l = checkLayout(x.l, `layout.named[${i}].l`); if (l.error) return { ok: false, error: l.error };
      seen.add(name); named.push({ name, l: l.value });
    }
    return { ok: true, value: { v: 1, cur: c.value, named } };
  }

  let booted = false;
  function boot() {
    const H = window.LoomWorkbench;
    if (booted || !H) return;
    const $ = id => d.getElementById(id);
    const mount = $('layoutToolsMount'), wsp = $('wsp'), det = $('detail');
    if (!mount || !wsp || !det) return;
    booted = true;

    let P = { v: 1, cur: EMPTY(), named: [] }, notice = '';
    try { const raw = localStorage.getItem(PKEY); if (raw) { const r = validate(JSON.parse(raw)); if (r.ok) P = r.value; else notice = '저장된 레이아웃 무시: ' + r.error; } } catch (e) { notice = '저장된 레이아웃 손상 — 기본값'; }
    const L = () => P.cur;
    const save = () => { try { localStorage.setItem(PKEY, JSON.stringify(P)); notice = notice.startsWith('레이아웃 저장 실패') ? '' : notice; } catch (e) { notice = '레이아웃 저장 실패 (' + (e && e.name || e) + ') — 이 세션에만 적용'; const mb = d.getElementById('loMenuBtn'); if (mb) mb.title = notice; renderPop(); } };
    const vw = () => window.innerWidth || d.documentElement.clientWidth || 1440;

    /* ---------- controls in #layoutToolsMount ---------- */
    mount.innerHTML =
      '<span class="lo-tools" id="loTools">' +
      '<button type="button" class="btn ghost lo-b" id="loMaxHist" aria-pressed="false" title="트랜잭션 기록 최대화 (Esc: 복원)">기록 ⤢</button>' +
      '<button type="button" class="btn ghost lo-b" id="loMaxDet" aria-pressed="false" title="응답 상세 최대화 (Esc: 복원)">상세 ⤢</button>' +
      '<button type="button" class="btn ghost lo-b disc" id="loMenuBtn" aria-expanded="false" aria-controls="loPop" title="레이아웃 초기화 · 이름 저장/불러오기">레이아웃</button>' +
      '<div class="lo-pop" id="loPop" role="dialog" aria-label="레이아웃" hidden></div>' +
      '</span>';
    const mk = (id, lab, host) => {
      const h = d.createElement('div');
      h.className = 'lo-h'; h.id = id; h.tabIndex = 0; h.dataset.owner = 'layout';
      h.setAttribute('role', 'separator'); h.setAttribute('aria-orientation', 'vertical'); h.setAttribute('aria-label', lab + ' (←→, Home=기본)');
      host.appendChild(h); return h;
    };
    // Absolutely positioned → out of grid flow (no implicit grid track / no mobile overflow).
    const hDid = mk('loHDid', 'DID 목록 너비', wsp), hVals = mk('loHVals', '확정 값 너비', wsp);
    const hSeq = mk('loHSeq', '응답 순서 열 너비', det), hBytes = mk('loHBytes', '바이트 열 너비', det);

    /* ---------- apply ---------- */
    function apply() {
      const l = L(), w = vw();
      wsp.classList.toggle('lo-did', l.didW != null); wsp.style.setProperty('--lo-did', (l.didW || 216) + 'px');
      wsp.classList.toggle('lo-vals', l.valsW != null); wsp.style.setProperty('--lo-vals', (l.valsW || 288) + 'px');
      det.classList.add('lo-cols');
      det.style.setProperty('--lo-seq', (l.seqPct || 30) + '%'); det.style.setProperty('--lo-bytes', (l.bytesPct || 30) + '%');
      const mobile = w < BP.mobile, max = mobile ? null : l.max;
      wsp.classList.toggle('lo-max-hist', max === 'hist'); wsp.classList.toggle('lo-max-detail', max === 'detail');
      $('loMaxHist').setAttribute('aria-pressed', String(max === 'hist')); $('loMaxDet').setAttribute('aria-pressed', String(max === 'detail'));
      $('loMaxHist').textContent = max === 'hist' ? '기록 복원' : '기록 ⤢'; $('loMaxDet').textContent = max === 'detail' ? '상세 복원' : '상세 ⤢';
      $('loMaxHist').disabled = $('loMaxDet').disabled = mobile;
      if (mobile) $('loMaxHist').title = $('loMaxDet').title = '모바일 폭: 단일 세로 배치 (최대화 없음)';
      place();
    }
    function vis(h, on) { h.hidden = !on; }
    function place() {
      const w = vw(), l = L(), max = w >= BP.mobile ? l.max : null;
      const didOn = w >= BP.did && !wsp.classList.contains('did-collapsed') && !max && !$('didPane').hidden;
      const valsOn = w >= BP.vals && !wsp.classList.contains('vals-collapsed') && !max && !$('vals').hidden;
      const detOn = w >= BP.detail && max !== 'hist';
      vis(hDid, didOn); vis(hVals, valsOn); vis(hSeq, detOn); vis(hBytes, detOn);
      const wr = wsp.getBoundingClientRect(), dr = det.getBoundingClientRect();
      if (didOn) { const r = $('didPane').getBoundingClientRect(); if (r.width) hDid.style.left = (r.right - wr.left) + 'px'; }
      if (valsOn) { const r = $('vals').getBoundingClientRect(); if (r.width) hVals.style.left = (r.left - wr.left - 6) + 'px'; }
      if (detOn) {
        const s = det.querySelector('.dt-seq'), b = det.querySelector('.dt-bytes');
        const rs = s && s.getBoundingClientRect(), rb = b && b.getBoundingClientRect();
        if (rs && rs.width) hSeq.style.left = (rs.right - dr.left) + 'px';
        if (rb && rb.width) hBytes.style.left = (rb.left - dr.left - 6) + 'px';
      }
      aria();
    }
    function aria() {
      const l = L(), b = bounds();
      const set = (h, now, lo, hi, unit) => { h.setAttribute('aria-valuemin', String(lo)); h.setAttribute('aria-valuemax', String(hi)); h.setAttribute('aria-valuenow', String(now)); h.setAttribute('aria-valuetext', now + unit); };
      set(hDid, l.didW != null ? l.didW : 216, LIM.did[0], b.didMax, 'px'); set(hVals, l.valsW != null ? l.valsW : 288, LIM.vals[0], b.valsMax, 'px');
      set(hSeq, l.seqPct != null ? l.seqPct : 30, LIM.seq[0], b.seqMax, '%'); set(hBytes, l.bytesPct != null ? l.bytesPct : 30, LIM.bytes[0], b.bytesMax, '%');
    }
    // Desktop clamps: centre column keeps >= centerMin px; detail middle keeps >= midPct %.
    function bounds() {
      const l = L(), W = wsp.clientWidth || vw();
      const curDid = l.didW != null ? l.didW : 216, curVals = l.valsW != null ? l.valsW : 288;
      const room = W - 12 - 12 - LIM.centerMin;
      return {
        didMax: Math.max(LIM.did[0], Math.min(LIM.did[1], room - (W >= BP.vals ? curVals : 0))),
        valsMax: Math.max(LIM.vals[0], Math.min(LIM.vals[1], room - curDid)),
        seqMax: Math.min(LIM.seq[1], 100 - LIM.midPct - (l.bytesPct != null ? l.bytesPct : 30)),
        bytesMax: Math.min(LIM.bytes[1], 100 - LIM.midPct - (l.seqPct != null ? l.seqPct : 30)),
      };
    }
    function setDim(k, v) {
      const b = bounds(), l = L();
      if (v == null) l[k] = null;
      else if (k === 'didW') l.didW = Math.round(clamp(v, LIM.did[0], b.didMax));
      else if (k === 'valsW') l.valsW = Math.round(clamp(v, LIM.vals[0], b.valsMax));
      else if (k === 'seqPct') l.seqPct = Math.round(clamp(v, LIM.seq[0], b.seqMax));
      else if (k === 'bytesPct') l.bytesPct = Math.round(clamp(v, LIM.bytes[0], b.bytesMax));
      apply();
    }
    const DIM = new Map([[hDid, 'didW'], [hVals, 'valsW'], [hSeq, 'seqPct'], [hBytes, 'bytesPct']]);
    const DEF = { didW: 216, valsW: 288, seqPct: 30, bytesPct: 30 };
    DIM.forEach((k, h) => {
      h.addEventListener('pointerdown', e => {
        if (e.button !== 0) return;
        e.preventDefault();
        h.classList.add('dragging'); d.body.classList.add('resizing');
        try { h.setPointerCapture(e.pointerId); } catch (x) { /* unsupported */ }
        const wr = wsp.getBoundingClientRect(), dr = det.getBoundingClientRect();
        const move = ev => {
          const x = ev.clientX;
          if (k === 'didW') setDim(k, x - wr.left - 6);
          else if (k === 'valsW') setDim(k, wr.right - 6 - x);
          else if (k === 'seqPct') setDim(k, ((x - dr.left) / Math.max(1, dr.width)) * 100);
          else setDim(k, ((dr.right - x) / Math.max(1, dr.width)) * 100);
        };
        const up = () => { h.classList.remove('dragging'); d.body.classList.remove('resizing'); h.removeEventListener('pointermove', move); h.removeEventListener('pointerup', up); h.removeEventListener('pointercancel', up); save(); };
        h.addEventListener('pointermove', move); h.addEventListener('pointerup', up); h.addEventListener('pointercancel', up);
      });
      h.addEventListener('keydown', e => {
        const px = k === 'didW' || k === 'valsW', step = (px ? 16 : 2) * (e.shiftKey ? 4 : 1);
        const cur = L()[k] != null ? L()[k] : DEF[k];
        // value grows toward the centre for left handles, away for right handles: ArrowRight widens DID/seq, narrows vals/bytes
        const dir = k === 'didW' || k === 'seqPct' ? 1 : -1;
        if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { e.preventDefault(); setDim(k, cur + (e.key === 'ArrowRight' ? step : -step) * dir); save(); }
        else if (e.key === 'Home') { e.preventDefault(); setDim(k, null); save(); }
      });
      h.addEventListener('dblclick', () => { setDim(k, null); save(); });
    });

    /* ---------- maximize / restore ---------- */
    function setMax(m) { L().max = L().max === m ? null : m; apply(); save(); }
    $('loMaxHist').addEventListener('click', () => setMax('hist'));
    $('loMaxDet').addEventListener('click', () => setMax('detail'));
    d.addEventListener('keydown', e => {
      if (e.key !== 'Escape' || e.defaultPrevented || !L().max) return;
      if (e.target && e.target.closest && e.target.closest('#toolDrawer, #tlViewer, #loPop, dialog, [role="dialog"]')) return;
      const m = L().max; L().max = null; apply(); save();
      const b = $(m === 'hist' ? 'loMaxHist' : 'loMaxDet'); if (b && !d.activeElement) b.focus();
    });

    /* ---------- reset + named layouts popover ---------- */
    let popMsg = '';
    function renderPop() {
      const pop = $('loPop'); if (pop.hidden) return;
      pop.innerHTML =
        '<div class="lo-row"><button type="button" class="btn" data-lo="reset">기본 레이아웃으로 초기화</button><small>열 너비·최대화 (기록/상세 세로 경계는 경계 더블클릭)</small></div>' +
        '<div class="lo-row"><input class="fld" id="loName" maxlength="' + LIM.name + '" placeholder="이름 (예: 넓은 기록)" aria-label="레이아웃 이름" autocomplete="off"><button type="button" class="btn" data-lo="save">현재 저장</button></div>' +
        (P.named.length ? '<ul class="lo-list" aria-label="저장된 레이아웃">' + P.named.map((x, i) => `<li><span>${esc(x.name)}</span><button type="button" class="btn ghost" data-lo="load" data-i="${i}">불러오기</button><button type="button" class="btn ghost danger" data-lo="del" data-i="${i}" aria-label="${esc(x.name)} 삭제">삭제</button></li>`).join('') + '</ul>' : '<p class="lo-none">저장된 레이아웃 없음</p>') +
        `<p class="lo-msg" aria-live="polite">${esc(popMsg || notice)}</p>`;
    }
    function togglePop(open) {
      const pop = $('loPop'), on = open != null ? open : pop.hidden;
      pop.hidden = !on; $('loMenuBtn').setAttribute('aria-expanded', String(on)); popMsg = '';
      if (on) { renderPop(); const f = $('loName'); if (f) f.focus(); } else $('loMenuBtn').focus();
    }
    function saveNamed(name) {
      name = String(name || '').trim();
      if (!name || name.length > LIM.name) { popMsg = `이름 1–${LIM.name}자`; renderPop(); return false; }
      const i = P.named.findIndex(x => x.name === name);
      if (i < 0 && P.named.length >= LIM.named) { popMsg = `최대 ${LIM.named}개 — 기존 항목을 삭제하세요`; renderPop(); return false; }
      const l = Object.assign({}, L());
      if (i >= 0) P.named[i] = { name, l }; else P.named.push({ name, l });
      save(); popMsg = (i >= 0 ? '덮어씀: ' : '저장됨: ') + name; renderPop(); return true;
    }
    function loadNamed(i) { const x = P.named[i]; if (!x) return false; P.cur = Object.assign(EMPTY(), x.l); apply(); save(); popMsg = '불러옴: ' + x.name; renderPop(); return true; }
    function reset() { P.cur = EMPTY(); apply(); save(); popMsg = '기본 레이아웃'; renderPop(); }
    $('loMenuBtn').addEventListener('click', () => togglePop());
    $('loPop').addEventListener('click', e => {
      const b = e.target.closest('[data-lo]'); if (!b) return;
      const a = b.dataset.lo, i = +b.dataset.i;
      if (a === 'reset') reset();
      else if (a === 'save') saveNamed($('loName').value);
      else if (a === 'load') loadNamed(i);
      else if (a === 'del') { const x = P.named[i]; if (x) { P.named.splice(i, 1); save(); popMsg = '삭제됨: ' + x.name; renderPop(); } }
    });
    $('loPop').addEventListener('keydown', e => {
      if (e.key === 'Escape') { e.preventDefault(); togglePop(false); }
      else if (e.key === 'Enter' && e.target.id === 'loName') { e.preventDefault(); saveNamed(e.target.value); }
    });
    d.addEventListener('pointerdown', e => { const pop = $('loPop'); if (!pop.hidden && !e.target.closest('#loTools')) { pop.hidden = true; $('loMenuBtn').setAttribute('aria-expanded', 'false'); } });

    /* ---------- host wiring ---------- */
    let raf = 0;
    const later = () => { if (raf) return; raf = 1; (window.requestAnimationFrame || setTimeout)(() => { raf = 0; place(); }); };
    window.addEventListener('resize', () => apply());
    const unrender = H.onRender(later); // reposition own handles only (core collapse/expand changes geometry)
    if (H.project && H.project.register) {
      H.project.register('layout', {
        export: () => JSON.parse(JSON.stringify(P)),
        defaults: () => ({ v: 1, cur: EMPTY(), named: [] }), // section absent from a project file → this (validated) default
        validate,
        // Strict: persist FIRST and let a storage failure (quota/blocked) THROW to the project coordinator before any
        // state is applied, so the coordinator's rollback sees an untouched layout. Normal UI writes stay best-effort.
        import: v => {
          const next = { v: 1, cur: Object.assign(EMPTY(), v.cur), named: v.named.map(x => ({ name: x.name, l: Object.assign(EMPTY(), x.l) })) };
          localStorage.setItem(PKEY, JSON.stringify(next));
          P = next; apply(); renderPop();
        },
      });
    }
    apply();

    window.LoomUdsLayout = {
      LIM, validate, get: () => JSON.parse(JSON.stringify(P)), cur: () => Object.assign({}, L()),
      setDim, setMax: m => { L().max = m === 'hist' || m === 'detail' ? m : null; apply(); save(); }, reset, saveNamed, loadNamed,
      deleteNamed: i => { P.named.splice(i, 1); save(); }, apply, place, bounds, handles: { hDid, hVals, hSeq, hBytes }, togglePop,
      destroy() { try { unrender && unrender(); } catch (e) { /* ignore */ } },
    };
  }
  if (window.LoomWorkbench) boot();
  else { d.addEventListener('uds:host-ready', boot); window.addEventListener('uds:host-ready', boot); }
})();
