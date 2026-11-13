/* Loom UDS Read DID — secondary controls: SIM options, export, clear, copy, pane collapse, divider. */
(function () {
  'use strict';
  const U = window.LoomUds, O = window.loomUdsOpus, E = O.E, st = O.st, d = document, $ = id => d.getElementById(id);
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

  /* ---------- SIM options (applied to the NEXT Read only; frozen into each transaction) ---------- */
  const SC = [['positive', '정상'], ['nrc', 'NRC'], ['pending', 'Pending→정상'], ['timeout', '타임아웃'], ['disconnect', '연결 끊김']];
  function renderSim() {
    const p = $('simPanel');
    if (p.hidden) return;
    const s = st.sim, has = p.querySelector('#simSeg');
    if (!has) {
      p.innerHTML =
        `<div class="sp-row"><span class="sp-tag">SIM</span><span class="flab" id="simLab">다음 읽기 응답</span>` +
        `<span class="seg" id="simSeg" role="radiogroup" aria-labelledby="simLab">${SC.map(([k, l]) => `<button type="button" role="radio" data-sc="${k}" aria-checked="false">${l}</button>`).join('')}</span></div>` +
        `<div class="sp-row">` +
        `<label>지연 <select class="fld" id="simLat">${U.LATENCIES.map(v => `<option value="${v}">${v} ms</option>`).join('')}</select></label>` +
        `<label>NRC <select class="fld" id="simNrc">${U.SIM_NRCS.map(v => `<option value="${v}">0x${U.hex2(v)} ${U.NRC[v]}</option>`).join('')}</select></label>` +
        `<label>0x78 횟수 <select class="fld" id="simPend">${[1, 2, 3, 4, 5].map(v => `<option value="${v}">${v}</option>`).join('')}</select></label>` +
        `<label><input type="checkbox" id="simLate"> 타임아웃 후 늦은 응답 (P2+${U.LATE_AFTER} ms)</label></div>` +
        `<p class="sp-note">요청 시점에 트랜잭션에 고정되어 진행 중·과거 요청에는 영향 없음. SIM 응답 지연·0x78 간격 ${U.PENDING_INTERVAL} ms·늦은 응답 오프셋은 SIM 전용이며 클라이언트 타이밍과 독립. 클라이언트 타이밍은 [연결 설정] 프로필 값 (<span id="simTiming"></span>, 기본 ${U.TIMING.p2}/${U.TIMING.p2star} SIM 축소값). 자동 재시도·주기 전송·재접속 없음. 카탈로그에 없는 DID는 SIM ECU가 NRC 0x31로 응답. <b>Raw</b>: [정상]은 mock ECU 기본 동작 — 22 + 단일 DID(3 B)만 fixture 응답, 그 외 0x22 형식은 7F 22 13, 0x22 외 서비스는 7F &lt;SID&gt; 11 (mock, 실제 ECU 효과 없음). NRC·0x78 시나리오는 실제 요청 SID를 echo.</p>`;
    }
    const tm = O.curRoute().timing; $('simTiming').textContent = `현재 P2 ${tm.p2} ms · P2* ${tm.p2star} ms`;
    p.querySelectorAll('#simSeg button').forEach(b => { const on = b.dataset.sc === s.scenario; b.setAttribute('aria-checked', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1; });
    $('simLat').value = String(s.latency); $('simNrc').value = String(s.nrc); $('simPend').value = String(s.pending); $('simLate').checked = s.late;
    $('simLat').disabled = !['positive', 'nrc', 'pending', 'disconnect'].includes(s.scenario);
    $('simNrc').disabled = s.scenario !== 'nrc';
    $('simPend').disabled = s.scenario !== 'pending';
    $('simLate').disabled = s.scenario !== 'timeout';
  }
  function setSim(patch) { Object.assign(st.sim, patch); O.savePrefs(); O.renderCtx(); renderSim(); }
  $('simPanel').addEventListener('click', e => { const b = e.target.closest('#simSeg button[data-sc]'); if (b) setSim({ scenario: b.dataset.sc }); });
  $('simPanel').addEventListener('keydown', e => {
    if (!e.target.closest('#simSeg') || !['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    e.preventDefault();
    const ids = SC.map(x => x[0]), i = ids.indexOf(st.sim.scenario);
    setSim({ scenario: ids[(i + (e.key === 'ArrowRight' ? 1 : ids.length - 1)) % ids.length] });
    $('simPanel').querySelector(`[data-sc="${st.sim.scenario}"]`).focus();
  });
  $('simPanel').addEventListener('change', e => {
    const t = e.target;
    if (t.id === 'simLat') setSim({ latency: +t.value });
    else if (t.id === 'simNrc') setSim({ nrc: +t.value });
    else if (t.id === 'simPend') setSim({ pending: +t.value });
    else if (t.id === 'simLate') setSim({ late: t.checked });
  });

  /* ---------- export / clear / copy ---------- */
  function download(name, text, type) {
    const a = d.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type })); a.download = name;
    d.body.appendChild(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }
  const stamp = () => new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  function exportJson() {
    const rows = E.exportRows();
    if (!rows.length) { O.toast('내보낼 기록 없음'); return null; }
    const text = JSON.stringify({ format: 'loom-uds-read-did-trace/1', exportedAt: new Date().toISOString(), note: 'Local SIM trace. Mock routes; bytes are UDS PDUs only (no CAN-TP/DoIP/EthDiag frames).', transactions: rows }, null, 2);
    download(`loom-uds-read-did-${stamp()}.json`, text, 'application/json');
    return text;
  }
  const csvq = v => { const s = v == null ? '' : String(v); return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s; };
  function exportCsv() {
    const rows = E.exportRows();
    if (!rows.length) { O.toast('내보낼 기록 없음'); return null; }
    const cols = ['no', 't0', 'ecu', 'transport', 'route', 'kind', 'sid', 'requestLen', 'did', 'didName', 'request', 'finalResponse', 'status', 'nrc', 'pendingCount', 'elapsedMs', 'appliedToConfirmed', 'simScenario'];
    const lines = [cols.concat('events').join(',')].concat(rows.map(r => cols.map(c => csvq(r[c])).concat(csvq(r.events.map(ev => `+${ev.dtMs}ms ${ev.kind}${ev.ignored ? '(ignored)' : ''} ${ev.bytes}`.trim()).join(' | '))).join(',')));
    const text = lines.join('\n') + '\n';
    download(`loom-uds-read-did-${stamp()}.csv`, text, 'text/csv');
    return text;
  }
  function clearHistory() {
    const done = E.txns.filter(t => E.terminal(t.status)).length;
    if (!done) { O.toast('지울 완료 기록 없음'); return; }
    E.clearHistory();
    if (st.selTxn != null && !E.byId(st.selTxn)) { st.selTxn = null; st.selEv = null; st.selField = null; }
    O.render();
    O.toast(`완료 기록 ${done}건 지움 · 확정 값과 진행 중 요청은 유지`);
  }
  function copyHex() {
    const tx = st.selTxn != null ? E.byId(st.selTxn) : null;
    if (!tx) { O.toast('선택한 트랜잭션 없음'); return; }
    const ev = tx.events[st.selEv] && tx.events[st.selEv].bytes ? tx.events[st.selEv] : tx.events[0];
    const text = U.hexs(ev.bytes);
    const ok = () => O.toast(`복사됨: ${text}`), fail = () => O.toast('클립보드 사용 불가 — 바이트 패널에서 직접 선택하세요', 'err');
    try { navigator.clipboard.writeText(text).then(ok, fail); } catch (e) { fail(); }
  }

  /* ---------- pane collapse ---------- */
  function applyPanes() {
    const w = $('wsp');
    w.classList.toggle('did-collapsed', st.didCollapsed);
    w.classList.toggle('vals-collapsed', st.valsCollapsed);
    $('didPane').hidden = st.didCollapsed; $('didRail').hidden = !st.didCollapsed;
    $('vals').hidden = st.valsCollapsed; $('valsRail').hidden = !st.valsCollapsed;
    $('center').style.setProperty('--hist-h', st.histH != null ? st.histH + '%' : '');
    if (st.histH == null) $('center').style.removeProperty('--hist-h');
    const dv = $('divHist'); dv.setAttribute('aria-valuemin', '15'); dv.setAttribute('aria-valuemax', '80'); dv.setAttribute('aria-valuenow', String(st.histH != null ? st.histH : 42));
  }
  function togglePane(which, collapsed, focusId) {
    st[which] = collapsed; O.savePrefs(); applyPanes();
    const f = $(focusId); if (f) f.focus();
  }

  d.addEventListener('click', e => {
    const b = e.target.closest('[data-cmd]'); if (!b) return;
    switch (b.dataset.cmd) {
      case 'export-json': exportJson(); break;
      case 'export-csv': exportCsv(); break;
      case 'clear-history': clearHistory(); break;
      case 'copy-hex': copyHex(); break;
      case 'collapse-did': togglePane('didCollapsed', true, 'didRail'); break;
      case 'expand-did': togglePane('didCollapsed', false, 'didList'); break;
      case 'collapse-vals': togglePane('valsCollapsed', true, 'valsRail'); break;
      case 'expand-vals': togglePane('valsCollapsed', false, 'valsBody'); break;
    }
  });

  /* ---------- history/detail divider (drag + keyboard, persisted) ---------- */
  const div = $('divHist');
  function histBounds() {
    const c = $('center'), req = $('req');
    const avail = c.clientHeight - req.offsetHeight - 6 - 6;
    return { c, avail: Math.max(1, avail), top: c.getBoundingClientRect().top + req.offsetHeight + 6 };
  }
  function setHist(pct) { st.histH = Math.round(Math.max(15, Math.min(80, pct))); applyPanes(); }
  div.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    e.preventDefault();
    const b = histBounds();
    div.classList.add('dragging'); d.body.classList.add('resizing');
    try { div.setPointerCapture(e.pointerId); } catch (x) { /* not supported */ }
    const move = ev => setHist(((ev.clientY - b.top) / b.avail) * 100);
    const up = () => { div.classList.remove('dragging'); d.body.classList.remove('resizing'); div.removeEventListener('pointermove', move); div.removeEventListener('pointerup', up); div.removeEventListener('pointercancel', up); O.savePrefs(); };
    div.addEventListener('pointermove', move); div.addEventListener('pointerup', up); div.addEventListener('pointercancel', up);
  });
  div.addEventListener('keydown', e => {
    const cur = st.histH != null ? st.histH : 42;
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') { e.preventDefault(); setHist(cur + (e.key === 'ArrowDown' ? 4 : -4)); O.savePrefs(); }
    else if (e.key === 'Home') { e.preventDefault(); st.histH = null; applyPanes(); O.savePrefs(); }
  });
  div.addEventListener('dblclick', () => { st.histH = null; applyPanes(); O.savePrefs(); });

  applyPanes();
  window.LoomUdsTools = { download, stamp, render: renderSim, exportJson, exportCsv, clearHistory, copyHex, applyPanes, setHist };
  renderSim();
})();
