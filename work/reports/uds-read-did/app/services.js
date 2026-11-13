/* Loom UDS workbench — services panel (items 9 + 11), core-owned. Uses ONLY the shared host H.
   Session control (0x10), TesterPresent runner (0x3E), DTC read/clear (0x19/0x14) — every action is a real engine
   transaction on the CURRENT explicit context (same route snapshot, timing, serial guard, SIM options as a manual read).
   SIM ECU behaviour only; no SecurityAccess / flashing; nothing auto-starts after reload/import/reconnect. */
(function () {
  'use strict';
  const H = window.LoomWorkbench; if (!H) return;
  const U = H.constants, E = H.engine, d = document, esc = H.ui.esc;
  const O = H._core.O, hook = O.hook, saved = O.saved.services || {};
  const mount = d.getElementById('servicesMount'), chip = d.getElementById('sessChip');
  const TP_INTERVALS = [1000, 2000, 3000, 4000];
  const MAX_DTC_HIST = 6;

  /* ---------- prefs (definitions only; never running state) ---------- */
  const pref = {
    tpInterval: TP_INTERVALS.includes(saved.tpInterval) ? saved.tpInterval : 2000,
    tpMode: saved.tpMode === '00' ? '00' : '80',
    dtcMask: typeof saved.dtcMask === 'string' && /^[0-9A-F]{2}$/.test(saved.dtcMask) ? saved.dtcMask : 'FF',
    sprmib: !!saved.sprmib,
  };
  hook.prefs.set('services', () => Object.assign({}, pref));
  function validatePrefs(x) {
    if (x == null) return { ok: true, value: { tpInterval: 2000, tpMode: '80', dtcMask: 'FF', sprmib: false } };
    if (typeof x !== 'object' || Array.isArray(x)) return { ok: false, error: 'services 섹션 객체 아님' };
    if (!TP_INTERVALS.includes(x.tpInterval)) return { ok: false, error: `services.tpInterval: ${TP_INTERVALS.join('/')} ms` };
    if (x.tpMode !== '00' && x.tpMode !== '80') return { ok: false, error: 'services.tpMode: 00 / 80' };
    if (typeof x.dtcMask !== 'string' || !/^[0-9A-F]{2}$/.test(x.dtcMask)) return { ok: false, error: 'services.dtcMask: 2자리 대문자 hex' };
    return { ok: true, value: { tpInterval: x.tpInterval, tpMode: x.tpMode, dtcMask: x.dtcMask, sprmib: !!x.sprmib } };
  }

  /* ---------- TesterPresent runner (explicit start/stop, per context) ---------- */
  const tps = new Map(); // ctxKey -> job
  const ck = (e, t) => e + '|' + t;
  function tpStart(ecu, tr) {
    const k = ck(ecu, tr);
    if (tps.has(k) && tps.get(k).running) return { ok: false, error: '이미 실행 중' };
    const cx = H.contextOf(ecu, tr);
    if (!cx || cx.link !== 'online') return { ok: false, error: '경로가 연결되어 있지 않음 — [연결] 후 시작 (자동 연결 없음)' };
    const job = { key: k, ecu, transport: tr, interval: pref.tpInterval, mode: pref.tpMode, running: true, gen: cx.gen, h: null, startT: H.clock.now(), sent: 0, skipped: 0, nrc: 0, other: 0, sends: [], last: null, stopReason: '' };
    tps.set(k, job);
    fire(job);
    O.savePrefs(); H.schedule();
    return { ok: true, job };
  }
  function tpStop(k, reason) {
    const job = tps.get(k); if (!job || !job.running) return false;
    job.running = false; if (job.h != null) H.clock.clear(job.h); job.h = null;
    job.stopReason = reason || '사용자 중지'; job.stopT = H.clock.now();
    H.schedule();
    return true; // in-flight TP request (if any) completes normally — never cancelled
  }
  function fire(job) {
    if (!job.running) return;
    job.h = H.clock.set(() => fire(job), job.interval);
    const cx = H.contextOf(job.ecu, job.transport);
    if (!cx || cx.link !== 'online' || cx.gen !== job.gen) { tpStop(job.key, '링크 끊김/재연결 — 중지됨 (자동 재개 없음)'); return; }
    if (cx.activeTxnId) { job.skipped++; job.lastSkip = H.clock.now(); patch(); return; } // busy: skip, never queue / cancel
    const r = H.send({ ecu: job.ecu, transport: job.transport, mode: 'raw', rawText: job.mode === '80' ? '3E 80' : '3E 00', source: 'tester-present' });
    if (!r.ok) { job.skipped++; job.lastErr = r.error; patch(); return; }
    job.sent++; job.sends.push(H.clock.now()); if (job.sends.length > 20) job.sends.shift();
    job.last = { id: r.txn.id, no: r.txn.no };
  }
  const cadence = job => (job.sends.length < 2 ? null : Math.round((job.sends[job.sends.length - 1] - job.sends[0]) / (job.sends.length - 1)));
  H.activity.register('tp', {
    list: () => [...tps.values()].filter(j => j.running).map(j => ({ ecu: j.ecu, transport: j.transport, kind: 'tester-present', label: `TesterPresent 3E ${j.mode} / ${j.interval} ms` })),
    stop: () => { [...tps.keys()].forEach(k => tpStop(k, '사용자 중지 (전체)')); },
  });

  /* ---------- DTC results (parsed from real 0x59 responses in engine txns; bounded per ECU) ---------- */
  const dtcHist = new Map(); // ecu -> [{no, txnId, t, mask, avail, list, transport, source}] newest first
  function capture(tx) {
    if (tx.sid !== 0x19 || tx.status !== 'positive' || !tx.final || tx.final[0] !== 0x59 || tx.final[1] !== 0x02 || tx.request.length !== 3) return;
    const p = U.parseDtcRecords(tx.final);
    const arr = dtcHist.get(tx.ecu) || [];
    if (arr.some(x => x.txnId === tx.id)) return;
    arr.unshift({ no: tx.no, txnId: tx.id, t: tx.tEnd, mask: tx.request[2], avail: p.avail, list: p.list, rest: p.rest, transport: tx.transport, source: tx.source });
    dtcHist.set(tx.ecu, arr.slice(0, MAX_DTC_HIST));
  }
  function diff(cur, prev) {
    if (!prev) return null;
    const pm = new Map(prev.list.map(x => [x.code, x.status])), cm = new Map(cur.list.map(x => [x.code, x.status]));
    return {
      added: cur.list.filter(x => !pm.has(x.code)).map(x => x.code),
      gone: prev.list.filter(x => !cm.has(x.code)).map(x => x.code),
      changed: cur.list.filter(x => pm.has(x.code) && pm.get(x.code) !== x.status).map(x => x.code),
      sameMask: cur.mask === prev.mask,
    };
  }

  /* ---------- actions (current context, explicit) ---------- */
  let msg = '', msgCls = '', confirmClear = false;
  function say(t, cls) { msg = t; msgCls = cls || ''; patch(); }
  function sendSvc(rawText, label) {
    const cx = H.context();
    const r = H.send({ ecu: cx.ecu, transport: cx.transport, mode: 'raw', rawText, source: 'service' });
    if (!r.ok) { say(`${label}: ${r.error} — 전송하지 않음`, 'err'); return r; }
    say(`${label} → #${r.txn.no} 전송 (${cx.ecu} · ${U.TRANSPORTS.find(t => t.id === cx.transport).label})`, '');
    return r;
  }
  function session(sub) { return sendSvc(`10 ${U.hex2(sub | (pref.sprmib ? 0x80 : 0))}`, `세션 ${U.SESSIONS[sub]}`); }
  function dtcRead(sub) { return sendSvc(`19 ${U.hex2(sub)} ${pref.dtcMask}`, sub === 1 ? 'DTC 개수' : 'DTC 읽기'); }
  function dtcClear() {
    if (!confirmClear) { confirmClear = true; patch(); return null; }
    confirmClear = false;
    return sendSvc('14 FF FF FF', 'DTC 지우기 (SIM ECU)');
  }

  /* ---------- render ---------- */
  const SB = { confirmed: ['ok', '확인됨'], assumed: ['', '가정 (새 연결)'], unknown: ['warn', '알 수 없음'], expired: ['warn', 'S3 만료'], reset: ['', '초기화 (연결 해제)'], offline: ['', '연결 안 됨'] };
  function build() {
    mount.innerHTML =
      `<p class="sv-target" id="svTarget"></p>` +
      `<section class="sv-sec" aria-labelledby="svSessH"><h3 id="svSessH">세션 · DiagnosticSessionControl 0x10</h3>` +
      `<div class="sv-row"><span class="seg sm" role="group" aria-label="세션 요청"><button type="button" data-sv="s1" title="10 01 전송">Default 01</button><button type="button" data-sv="s3" title="10 03 전송">Extended 03</button><button type="button" data-sv="s2" title="10 02 전송 (SIM: non-default 세션에서만)">Programming 02</button></span>` +
      `<label class="sv-ck"><input type="checkbox" id="svSpr"> SPRMIB (응답 억제)</label></div>` +
      `<dl class="sv-kv" id="svSess"></dl>` +
      `<p class="sv-note">세션 상태는 이 ECU·경로 컨텍스트별. ECU 보고 P2server/P2*server는 메타데이터로만 표시하며 클라이언트 프로필 P2/P2*는 바꾸지 않음 (SIM 축소값 유지). S3 ${U.S3_MS} ms 동안 처리된 요청이 없으면 SIM 서버가 default로 복귀. Raw 10/3E도 같은 상태를 사용.</p></section>` +
      `<section class="sv-sec" aria-labelledby="svTpH"><h3 id="svTpH">TesterPresent · 0x3E</h3>` +
      `<div class="sv-row"><label>주기 <select class="fld" id="svTpInt">${TP_INTERVALS.map(v => `<option value="${v}">${v} ms</option>`).join('')}</select></label>` +
      `<label>형식 <select class="fld" id="svTpMode"><option value="80">3E 80 (응답 억제)</option><option value="00">3E 00 (응답)</option></select></label>` +
      `<button type="button" class="btn" id="svTpBtn" data-sv="tp"></button></div>` +
      `<dl class="sv-kv" id="svTp"></dl><div class="sv-others" id="svTpOthers"></div>` +
      `<p class="sv-note">명시적 시작/중지만. 같은 컨텍스트에 진행 중 요청이 있으면 그 주기는 건너뜀 (대기열·취소 없음). 링크 끊김·재연결 시 중지되며 재로드·가져오기 후 자동 재개 없음. 현재 SIM 옵션이 적용됨.</p></section>` +
      `<section class="sv-sec" aria-labelledby="svDtcH"><h3 id="svDtcH">DTC · ReadDTCInformation 0x19 / Clear 0x14</h3>` +
      `<div class="sv-row"><label for="svMask">상태 마스크 0x</label><input class="fld sv-mask" id="svMask" maxlength="2" spellcheck="false" autocomplete="off" aria-describedby="svMaskErr">` +
      `<select class="fld" id="svMaskPre" aria-label="마스크 프리셋"><option value="">프리셋…</option><option value="FF">FF 전체</option><option value="09">09 testFailed+confirmed</option><option value="08">08 confirmed</option><option value="04">04 pending</option><option value="01">01 testFailed</option></select>` +
      `<button type="button" class="btn" data-sv="d1" title="19 01 mask">개수 19 01</button><button type="button" class="btn primary" data-sv="d2" title="19 02 mask">읽기 19 02</button>` +
      `<span class="grow"></span><button type="button" class="btn danger" data-sv="clr" id="svClr"></button></div>` +
      `<small class="cf-err" id="svMaskErr" role="status"></small>` +
      `<div class="sv-dtc" id="svDtc"></div>` +
      `<div class="sv-row"><button type="button" class="btn ghost" data-sv="restore" title="SIM ECU 고장 fixture 재주입 — 로컬 SIM 동작, 요청 전송 없음">SIM 고장 fixture 재주입</button></div>` +
      `<p class="sv-note">DTC는 SIM fixture (실제 차량 아님). 표는 실제 0x59 02 응답 바이트를 파싱 (Raw 19 02도 포함). 지우기(14 FF FF FF)는 SIM ECU 메모리만 지우며 SIM에서는 non-default 세션 필요 (아니면 7F 14 22).</p></section>` +
      `<p class="sv-msg" id="svMsg" aria-live="polite"></p>`;
    mount.querySelector('#svTpInt').value = String(pref.tpInterval);
    mount.querySelector('#svTpMode').value = pref.tpMode;
    mount.querySelector('#svMask').value = pref.dtcMask;
    mount.querySelector('#svSpr').checked = pref.sprmib;
  }
  const $m = id => mount.querySelector('#' + id);
  const ago = t => (t == null ? '—' : ((H.clock.now() - t) / 1000).toFixed(1) + ' s 전');
  function sessHtml(cx, s) {
    const [cls, lab] = SB[s.tester.status] || ['', s.tester.status];
    const rows = [
      ['테스터 기준', `<b>${esc(s.tester.name)}</b> <span class="sv-b ${cls}">${esc(lab)}</span>` + (s.tester.no ? ` · 출처 <a data-goto="${s.tester.txnId}">#${s.tester.no}</a>` : '') + ` · ${ago(s.tester.t)}`],
    ];
    if (s.tester.note) rows.push(['설명', esc(s.tester.note)]);
    if (s.tester.status === 'confirmed' && s.tester.p2srv != null) rows.push(['ECU 보고 타이밍', `P2server ${s.tester.p2srv} ms · P2*server ${s.tester.p2starsrv} ms <small>(메타데이터 — 클라이언트 프로필 P2 ${E.routeFor(cx.ecu, cx.transport).timing.p2} ms 유지)</small>`]);
    rows.push(['SIM 서버 내부', `${esc(s.server.name)}` + (s.server.s3Due != null ? ` · S3 남은 <b id="svS3">${Math.max(0, s.server.s3Due - H.clock.now())} ms</b>` : ' · S3 없음 (default)') + ' <small>(SIM 진실 값 — 실제 장치라면 테스터가 알 수 없음)</small>']);
    return rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${v}</dd>`).join('');
  }
  function tpHtml(job) {
    if (!job) return '<dt>상태</dt><dd>중지됨 · 이 컨텍스트에서 실행한 적 없음</dd>';
    const cad = cadence(job);
    return [['상태', job.running ? `<b class="ok">실행 중</b> 3E ${job.mode} · ${job.interval} ms` : `중지됨 — ${esc(job.stopReason)}`],
      ['전송 / 건너뜀', `${job.sent} / ${job.skipped}` + (job.lastSkip ? ` <small>(마지막 건너뜀 ${ago(job.lastSkip)} — 진행 중 요청)</small>` : '')],
      ['실제 주기', cad != null ? `${cad} ms (최근 ${job.sends.length}회 평균)` : '—'],
      ['마지막', job.last ? `<a data-goto="${job.last.id}">#${job.last.no}</a> ${esc(E.byId(job.last.id) ? E.statusText(E.byId(job.last.id)) : '기록에서 제거됨')}` : '—']]
      .map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('');
  }
  function dtcHtml(ecu) {
    const arr = dtcHist.get(ecu) || [], cur = arr[0];
    const prev = cur && (arr.slice(1).find(x => x.mask === cur.mask) || arr[1]); // prefer the last read with the same mask
    if (!cur) return `<p class="sv-empty">${esc(ecu)}: 아직 0x59 02 응답 없음 — [읽기 19 02]</p>`;
    const df = diff(cur, prev), mark = c => (df && df.added.includes(c) ? '<span class="sv-b warn">신규</span>' : df && df.changed.includes(c) ? '<span class="sv-b">상태 변경</span>' : '');
    let h = `<div class="sv-dtc-h">출처 <a data-goto="${cur.txnId}">#${cur.no}</a> · ${H.ui.fmtT(cur.t)} · 마스크 0x${U.hex2(cur.mask)} · 가용 0x${cur.avail != null ? U.hex2(cur.avail) : '—'} · <b>${cur.list.length}건</b>` + (cur.source !== 'service' ? ` · ${esc(cur.source)}` : '') + '</div>';
    if (cur.list.length) h += `<div class="ct-scroll" role="region" aria-label="DTC 목록" tabindex="0"><table class="ftab sv-dtab"><thead><tr><th>DTC</th><th>상태</th><th>플래그</th><th>설명 (SIM fixture)</th></tr></thead><tbody>${cur.list.map(r => `<tr><td class="v">${U.dtcText(r.code)} ${mark(r.code)}</td><td class="v">0x${U.hex2(r.status)}</td><td class="n">${U.statusBits(r.status).map(b => `<span class="sv-flag" title="${b}">${b.replace(/([A-Z])/g, ' $1').trim()}</span>`).join('')}</td><td class="n">${esc(U.DTC_DESC[r.code] || '설명 없음')}</td></tr>`).join('')}</tbody></table></div>`;
    if (cur.rest) h += `<p class="sv-empty bad">응답 끝 ${cur.rest} B가 4 B 레코드 경계와 맞지 않음 (해석 안 함)</p>`;
    if (df) h += `<p class="sv-diff">이전 #${prev.no} (마스크 0x${U.hex2(prev.mask)}) 대비: 신규 ${df.added.length} · 사라짐 ${df.gone.length}${df.gone.length ? ' (' + df.gone.map(U.dtcText).join(', ') + ')' : ''} · 상태 변경 ${df.changed.length}${df.sameMask ? '' : ' · 마스크가 달라 직접 비교 주의'}</p>`;
    return h;
  }
  function patch() {
    if (!mount.firstChild) build();
    const cx = H.context(), s = E.session(cx.ecu, cx.transport), job = tps.get(ck(cx.ecu, cx.transport));
    const trl = U.TRANSPORTS.find(t => t.id === cx.transport).label;
    $m('svTarget').innerHTML = `대상 <b>${esc(cx.ecu)}</b> · <b>${esc(trl)}</b> · ${cx.link === 'online' ? `연결됨 gen ${cx.gen}` : '<span class="off">연결 안 됨 — 전송 불가</span>'}` + (cx.activeTxnId ? ` · 진행 중 #${E.byId(cx.activeTxnId).no}` : '') + ' <small>(현재 선택 컨텍스트에 전송)</small>';
    $m('svSess').innerHTML = sessHtml(cx, s);
    const run = job && job.running;
    const tb = $m('svTpBtn'); tb.textContent = run ? 'TesterPresent 중지' : 'TesterPresent 시작'; tb.className = 'btn' + (run ? ' danger' : ' primary');
    $m('svTpInt').disabled = !!run; $m('svTpMode').disabled = !!run;
    $m('svTp').innerHTML = tpHtml(job);
    const others = [...tps.values()].filter(j => j.running && j.key !== ck(cx.ecu, cx.transport));
    $m('svTpOthers').innerHTML = others.length ? '다른 컨텍스트 실행 중: ' + others.map(j => `<button type="button" class="btn ghost sv-ostop" data-stop="${esc(j.key)}" title="이 TesterPresent 중지">${esc(j.ecu)}·${esc(U.TRANSPORTS.find(t => t.id === j.transport).label)} ${j.interval} ms ■</button>`).join('') : '';
    const clr = $m('svClr'); clr.textContent = confirmClear ? 'SIM ECU DTC 지우기 확인 (14 FF FF FF)' : 'DTC 지우기…';
    $m('svDtc').innerHTML = dtcHtml(cx.ecu);
    const mv = $m('svMask').value.trim().toUpperCase();
    $m('svMaskErr').textContent = /^[0-9A-F]{2}$/.test(mv) ? '' : '2자리 hex (00–FF)';
    const m = $m('svMsg'); m.textContent = msg; m.className = 'sv-msg' + (msgCls ? ' ' + msgCls : '');
    renderChip(cx, s, job);
  }
  function renderChip(cx, s, job) {
    if (!chip) return;
    if (cx.link !== 'online') { chip.innerHTML = ''; chip.className = 'sess-chip'; return; }
    const [cls, lab] = SB[s.tester.status] || ['', ''];
    chip.className = 'sess-chip ' + cls;
    chip.innerHTML = `<span class="flab">세션</span><b>${esc(s.tester.name)}</b><small>${esc(lab)}</small>` + (s.server.s3Due != null ? `<small title="SIM 서버 S3 남은 시간">S3 ${(Math.max(0, s.server.s3Due - H.clock.now()) / 1000).toFixed(1)} s</small>` : '') + (job && job.running ? `<small class="ok" title="TesterPresent 실행 중">TP ${job.interval} ms</small>` : '');
    chip.title = '세션 상태 (테스터 기준) — [서비스] 도구에서 변경';
  }

  /* ---------- events ---------- */
  mount.addEventListener('click', e => {
    const a = e.target.closest('a[data-goto]'); if (a) { const id = +a.dataset.goto; if (E.byId(id)) H.selectTxn(id); else H.ui.toast('출처 트랜잭션이 기록에서 제거됨'); return; }
    const st = e.target.closest('[data-stop]'); if (st) { tpStop(st.dataset.stop, '사용자 중지'); patch(); return; }
    const b = e.target.closest('[data-sv]'); if (!b || b.disabled) return;
    const k = b.dataset.sv;
    if (k !== 'clr') confirmClear = false;
    if (k === 's1') session(1); else if (k === 's2') session(2); else if (k === 's3') session(3);
    else if (k === 'tp') {
      const cx = H.context(), job = tps.get(ck(cx.ecu, cx.transport));
      if (job && job.running) { tpStop(job.key, '사용자 중지'); say('TesterPresent 중지 — 진행 중 요청은 정상 종료', ''); }
      else { const r = tpStart(cx.ecu, cx.transport); say(r.ok ? `TesterPresent 시작 3E ${pref.tpMode} / ${pref.tpInterval} ms` : 'TesterPresent: ' + r.error, r.ok ? '' : 'err'); }
    } else if (k === 'd1' || k === 'd2') {
      if (!/^[0-9A-F]{2}$/.test(pref.dtcMask)) { say('상태 마스크가 유효하지 않음 — 전송하지 않음', 'err'); $m('svMask').focus(); return; }
      dtcRead(k === 'd1' ? 1 : 2);
    } else if (k === 'clr') dtcClear();
    else if (k === 'restore') { const cx = H.context(); const r = E.simRestoreDtc(cx.ecu); say(`SIM: ${cx.ecu} 고장 fixture ${r.count}건 재주입 (요청 전송 없음 — 다시 읽어 확인)`, ''); }
    patch();
  });
  mount.addEventListener('change', e => {
    const t = e.target;
    if (t.id === 'svTpInt') pref.tpInterval = +t.value;
    else if (t.id === 'svTpMode') pref.tpMode = t.value === '00' ? '00' : '80';
    else if (t.id === 'svSpr') pref.sprmib = t.checked;
    else if (t.id === 'svMaskPre' && t.value) { pref.dtcMask = t.value; $m('svMask').value = t.value; t.value = ''; }
    else return;
    O.savePrefs(); patch();
  });
  mount.addEventListener('input', e => {
    if (e.target.id !== 'svMask') return;
    const v = e.target.value.trim().toUpperCase();
    if (/^[0-9A-F]{2}$/.test(v)) { pref.dtcMask = v; O.savePrefs(); }
    else pref.dtcMask = v; // invalid draft blocks send; not persisted
    $m('svMaskErr').textContent = /^[0-9A-F]{2}$/.test(v) ? '' : '2자리 hex (00–FF)';
  });
  chip && chip.addEventListener('click', () => H.tools.open('services'));
  H.subscribe(ev => {
    if (ev.type === 'txn' && ev.txn && !ev.late) capture(ev.txn);
    if (ev.type === 'link' && ev.context && ev.context.link !== 'online') { const j = tps.get(ck(ev.context.ecu, ev.context.transport)); if (j && j.running) tpStop(j.key, '링크 끊김 — 중지됨 (자동 재개 없음)'); }
    if (ev.type === 'context') { confirmClear = false; msg = ''; }
  });
  H.onRender(patch);
  // Light ticker: S3 countdown text + chip only (no innerHTML rebuild of panels → focus/links stay put).
  const tickH = setInterval(() => {
    const cx = H.context(); if (cx.link !== 'online') return;
    const s = E.session(cx.ecu, cx.transport), el = $m('svS3');
    if (el && s.server.s3Due != null) el.textContent = Math.max(0, s.server.s3Due - H.clock.now()) + ' ms';
    renderChip(cx, s, tps.get(ck(cx.ecu, cx.transport)));
  }, 500);

  /* ---------- project section (core): TP/DTC preferences only, never running jobs ---------- */
  H._core.registerCore('services', {
    export: () => Object.assign({}, pref),
    validate: validatePrefs,
    import: v => { Object.assign(pref, v); if (mount.firstChild) { $m('svTpInt').value = String(pref.tpInterval); $m('svTpMode').value = pref.tpMode; $m('svMask').value = pref.dtcMask; $m('svSpr').checked = pref.sprmib; } },
  });

  patch();
  window.LoomUdsServices = { tps, tpStart, tpStop, session, dtcRead, dtcClear, dtcHist, pref, patch, tickH, cadence };
})();
