/* Loom UDS Read DID — connection profile editor (Opus). Per ECU × route draft → Save & apply (offline only, engine-enforced).
   Presets are immutable; custom profiles are local mock configuration. Applies on the next explicit [연결]. */
(function () {
  'use strict';
  const U = window.LoomUds, O = window.loomUdsOpus, E = O.E, st = O.st, d = document, $ = id => d.getElementById(id), esc = O.esc;
  const panel = $('cfgPanel'), btn = $('btnCfg');
  const drafts = new Map(); // ctx key -> draft (memory only, survives switching away/back)
  let draftGen = 0;         // module-wide: every (re)sealed or programmatically replaced draft gets a unique build signature
  st.cfgOpen = false;

  /* ---------- field model (UI) ---------- */
  const TIMING_F = [
    { k: 'p2', label: 'P2 (ms)', hint: `${U.LIMITS.p2[0]}–${U.LIMITS.p2[1]} · 기본 ${U.TIMING.p2}`, num: true },
    { k: 'p2star', label: 'P2* (ms)', hint: `${U.LIMITS.p2star[0]}–${U.LIMITS.p2star[1]} · ≥ P2 · 기본 ${U.TIMING.p2star}`, num: true },
  ];
  const chanOpts = link => U.CHANNELS[link].map(c => [c, c + ' (SIM)']);
  const ROUTE_F = {
    doip: [
      { k: 'channel', label: 'Ethernet 채널', opts: chanOpts('eth') },
      { k: 'host', label: 'DoIP 엔티티 호스트', hint: 'IPv4 또는 호스트 이름 · IPv6 미지원 · DNS 조회 안 함', wide: true },
      { k: 'port', label: 'TCP 포트', hint: '1–65535 · 기본 13400', num: true },
      { k: 'testerLA', label: 'Tester 논리 주소', pfx: '0x', hint: '16-bit hex' },
      { k: 'ecuLA', label: 'ECU 논리 주소', pfx: '0x', hint: '16-bit hex' },
    ],
    cantp: [
      { k: 'channel', label: 'CAN 채널', opts: chanOpts('can') },
      { k: 'bitrate', label: '비트레이트 (메타데이터)', opts: U.BITRATES.map(b => [String(b), U.kbit(b)]), hint: 'SIM은 비트 타이밍을 사용하지 않음' },
      { k: 'idFormat', label: 'CAN ID 형식', opts: [['11', '11-bit 표준'], ['29', '29-bit 확장']] },
      { k: 'txId', label: '요청 ID (Tx)', pfx: '0x', hint: 'id' },
      { k: 'rxId', label: '응답 ID (Rx)', pfx: '0x', hint: 'id' },
      { k: '_addr', label: '주소 지정', fixed: 'normal · physical (고정 — 다른 모드 미모델링)' },
    ],
    ethdiag: [
      { k: 'channel', label: 'Ethernet 채널', opts: chanOpts('eth') },
      { k: 'adapter', label: '어댑터 이름 (벤더 mock)', hint: `1–${U.LIMITS.adapter}자 · 표시용`, wide: true },
      { k: 'endpoint', label: '엔드포인트 메모', hint: `0–${U.LIMITS.endpoint}자 · 해석·연결하지 않는 메타데이터`, wide: true },
      { k: '_frame', label: '프레이밍', fixed: '미정의 벤더 프레이밍 · ISO 13400 DoIP 아님' },
    ],
  };
  const idHint = fmt => (fmt === '29' ? '29-bit hex · 최대 1FFFFFFF' : '11-bit hex · 최대 7FF');

  /* ---------- draft helpers ---------- */
  const ckey = () => st.ecu + '|' + st.transport;
  function fmtValues(p) {
    const v = p.values, o = {};
    U.FIELDS[p.transport].forEach(k => {
      const x = v[k];
      if (k === 'testerLA' || k === 'ecuLA') o[k] = U.hex4(x);
      else if (k === 'txId' || k === 'rxId') o[k] = U.hexId(x, v.idFormat).slice(2);
      else o[k] = String(x);
    });
    return o;
  }
  function suggestName(base) {
    const names = new Set(E.profilesFor(st.ecu, st.transport).map(p => p.name));
    for (let i = 1; ; i++) { const n = `${base} ${i}`.slice(0, U.LIMITS.name); if (!names.has(n)) return n; }
  }
  function fromProfile(p) {
    return { targetId: p.id, name: p.source === 'custom' ? p.name : suggestName(`${st.ecu} ${O.trLabel(st.transport)} 사용자`), vals: fmtValues(p), init: null, v: 0, tried: false, confirmDel: false };
  }
  function seal(dr) { dr.init = JSON.stringify([dr.name, dr.vals]); dr.v = ++draftGen; return dr; }
  function draft() {
    const k = ckey();
    let dr = drafts.get(k);
    if (!dr || (dr.targetId !== 'new' && !E.profile(dr.targetId))) { dr = seal(fromProfile(E.activeProfile(st.ecu, st.transport))); drafts.set(k, dr); }
    return dr;
  }
  const isDirty = dr => dr.targetId === 'new' || JSON.stringify([dr.name, dr.vals]) !== dr.init;
  const target = dr => (dr.targetId === 'new' ? null : E.profile(dr.targetId));
  function check(dr) {
    const v = U.validateProfile(st.transport, dr.vals), errors = Object.assign({}, v.errors);
    const tg = target(dr), presetSame = tg && tg.source === 'preset' && !isDirty(dr);
    if (!presetSame) { const ne = E.checkName(st.ecu, st.transport, dr.name.trim(), tg && tg.source === 'custom' ? tg.id : null); if (ne) errors.name = ne; }
    return { ok: !Object.keys(errors).length, errors };
  }

  /* ---------- render ---------- */
  function fieldHtml(f, dr) {
    if (f.fixed) return `<div class="cf fixed"><span class="cf-l">${esc(f.label)}</span><span class="cf-fix">${esc(f.fixed)}</span></div>`;
    const id = 'cf-' + f.k, desc = `${id}-hint ${id}-err`, val = dr.vals[f.k] == null ? '' : dr.vals[f.k];
    const ctl = f.opts
      ? `<select class="fld" id="${id}" data-k="${f.k}" aria-describedby="${desc}">${f.opts.map(([v, l]) => `<option value="${esc(v)}"${v === val ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`
      : `<span class="cf-in">${f.pfx ? `<span class="pfx">${f.pfx}</span>` : ''}<input class="fld" id="${id}" data-k="${f.k}" value="${esc(val)}" spellcheck="false" autocomplete="off"${f.num ? ' inputmode="numeric"' : ''} aria-describedby="${desc}"></span>`;
    return `<div class="cf${f.wide ? ' wide' : ''}" data-f="${f.k}"><label class="cf-l" for="${id}">${esc(f.label)}</label>${ctl}<small class="cf-hint" id="${id}-hint">${esc(f.hint || '')}</small><small class="cf-err" id="${id}-err" role="status"></small></div>`;
  }
  function build(dr) {
    const tr = st.transport, list = E.profilesFor(st.ecu, tr), act = E.activeProfile(st.ecu, tr);
    const opt = p => `<option value="${esc(p.id)}"${p.id === dr.targetId ? ' selected' : ''}>${p.source === 'preset' ? '프리셋' : '사용자'} · ${esc(p.name)}${p.source === 'custom' ? ' r' + p.rev : ''}${p.id === act.id ? ' ● 적용' : ''}</option>`;
    panel.innerHTML =
      `<div class="cfg-head"><b class="cfg-title" id="cfgTitle">연결 설정 · <span>${esc(st.ecu)} · ${esc(O.trLabel(tr))}</span></b>` +
      `<label class="cf-l" for="cfgProfile">편집 대상</label><select class="fld cfg-sel" id="cfgProfile">${list.map(opt).join('')}${dr.targetId === 'new' ? '<option value="new" selected>새 프로필 (저장 안 됨)</option>' : ''}</select>` +
      `<span class="cfg-src" id="cfgSrc"></span><span class="grow"></span>` +
      `<button type="button" class="btn" data-cfg="dup" title="편집 대상을 새 사용자 프로필 초안으로 복제">복제</button>` +
      `<button type="button" class="btn" data-cfg="preset" title="이 ECU·경로 프리셋 값을 초안에 불러오기 (저장 전에는 적용 안 됨)">프리셋 값</button>` +
      `<button type="button" class="btn danger" data-cfg="delete" id="cfgDel">삭제</button>` +
      `<span class="vdiv"></span>` +
      `<button type="button" class="btn ghost" data-cfg="export" title="사용자 프로필 JSON 내보내기 (기록 내보내기와 별개)">설정 내보내기</button>` +
      `<button type="button" class="btn ghost" data-cfg="import" title="설정 JSON 가져오기 — 검증 후 새 사용자 프로필로 추가, 적용·연결 안 함">가져오기</button>` +
      `<input type="file" id="cfgFile" accept="application/json,.json" hidden></div>` +
      `<div class="cfg-body">` +
      `<fieldset class="cfg-fs"><legend>프로필</legend>${fieldHtml({ k: 'name', label: '이름', hint: `1–${U.LIMITS.name}자 · ECU·경로 안에서 고유`, wide: true }, { vals: { name: dr.name } })}</fieldset>` +
      `<fieldset class="cfg-fs"><legend>${esc(O.trLabel(tr))} 경로</legend>${ROUTE_F[tr].map(f => fieldHtml(f, dr)).join('')}</fieldset>` +
      `<fieldset class="cfg-fs"><legend>클라이언트 타이밍</legend>${TIMING_F.map(f => fieldHtml(f, dr)).join('')}<p class="cf-note">타임아웃 판정에 실제 사용. SIM 응답 지연·0x78 간격은 SIM 옵션 (별도).</p></fieldset>` +
      `</div>` +
      `<div class="cfg-foot"><span class="cfg-st" id="cfgStatus" aria-live="polite"></span>` +
      `<button type="button" class="btn" data-cfg="disconnect" id="cfgDisc" hidden></button><span class="grow"></span>` +
      `<button type="button" class="btn" data-cfg="revert" id="cfgRevert">되돌리기</button>` +
      `<button type="button" class="btn" data-cfg="cancel" title="초안 폐기 후 닫기 (Esc는 초안 유지하고 닫기)">취소</button>` +
      `<button type="submit" class="btn primary" id="cfgSave">저장·적용</button></div>` +
      `<p class="cfg-note">설정은 이 ECU·경로에만 적용되며 다음 [연결]부터 사용됩니다. 진행 중·과거 요청은 요청 시점 설정을 유지합니다. SIM ECU는 설정 주소와 관계없이 이 컨텍스트의 fixture ECU로 응답 (주소 라우팅 모델링 안 함).<span id="cfgNotice"></span></p>`;
    panel.dataset.sk = `${ckey()}|${dr.targetId}|${dr.v}|${list.map(p => p.id + ':' + p.rev).join(',')}|${act.id}`;
  }
  function update(dr) {
    const c = O.curCtx(), online = c.link === 'online', tg = target(dr), act = E.activeProfile(st.ecu, st.transport), dirty = isDirty(dr);
    const chk = check(dr);
    const mk = `${ckey()}|${c.gen}|${c.link}`;
    if (st.cfgMsgKey !== mk) { if (st.cfgMsgKey != null) st.cfgMsg = ''; st.cfgMsgKey = mk; }
    // field errors (live, field-local)
    panel.querySelectorAll('[data-k]').forEach(el => {
      const k = el.dataset.k, e = chk.errors[k] || '', err = $(`cf-${k}-err`);
      el.classList.toggle('invalid', !!e); el.setAttribute('aria-invalid', e ? 'true' : 'false');
      if (err) err.textContent = e;
    });
    if (st.transport === 'cantp') ['txId', 'rxId'].forEach(k => { const h = $(`cf-${k}-hint`); if (h) h.textContent = idHint(dr.vals.idFormat); });
    const nameIn = $('cf-name'), presetClean = tg && tg.source === 'preset' && !dirty;
    if (nameIn) { nameIn.disabled = !!presetClean; $('cf-name-hint').textContent = tg && tg.source === 'preset' ? '프리셋을 수정해 저장하면 이 이름의 새 사용자 프로필 생성' : `1–${U.LIMITS.name}자 · ECU·경로 안에서 고유`; }
    $('cfgProfile').disabled = dirty;
    $('cfgProfile').title = dirty ? '저장하지 않은 변경 있음 — 저장 또는 되돌리기 후 전환' : '편집할 프로필 선택 (저장·적용 전에는 연결 설정 변경 없음)';
    $('cfgSrc').textContent = tg ? (tg.source === 'preset' ? `내장 프리셋 ${tg.id} · 변경 불가` : `사용자 · rev ${tg.rev} · 기반 ${tg.base}`) : `새 프로필 · 기반 ${'preset:' + ckey()}`;
    const del = $('cfgDel');
    del.disabled = !tg || tg.source !== 'custom';
    del.textContent = dr.confirmDel ? '삭제 확인' : '삭제';
    del.title = !tg || tg.source !== 'custom' ? '프리셋·저장 안 된 초안은 삭제 대상 아님' : (tg.id === act.id && online ? '적용 중 프로필 — 연결 해제 후 삭제 가능' : '사용자 프로필 삭제 (두 번 눌러 확인)');
    $('cfgRevert').disabled = !dirty;
    // save state
    const save = $('cfgSave');
    let label = '저장·적용', why = '';
    if (presetClean) label = tg.id === act.id ? '적용 중' : '프리셋 적용';
    else if (!tg || tg.source === 'preset') label = '새 프로필로 저장·적용';
    const nothing = presetClean ? tg.id === act.id : tg && tg.source === 'custom' && !dirty && tg.id === act.id;
    if (nothing) { why = '변경 없음 · 이미 적용 중'; label = '적용 중'; }
    save.textContent = label;
    save.disabled = online || nothing;
    const disc = $('cfgDisc');
    disc.hidden = !online;
    const a = c.active ? E.byId(c.active) : null;
    disc.textContent = a ? `연결 해제 (요청 #${a.no} 링크 끊김 처리)` : '연결 해제';
    let msg;
    if (online) msg = `<b class="warn">연결됨 gen ${c.gen}</b> — 연결은 ${esc(c.conn.route.ptag)} 스냅샷 사용 중. 저장·적용은 연결 해제 후 가능 (자동 해제·요청 중단 없음).`;
    else msg = `연결 안 됨 · 적용 중 <b>${esc(act.source === 'preset' ? '프리셋' : act.name + ' r' + act.rev)}</b>${why ? ' · ' + why : ' · 저장하면 다음 [연결]부터 사용'}`;
    if (dirty) msg += ' · <b class="dirty">저장 안 된 변경</b>';
    if (dr.tried && !chk.ok) msg += ` · <b class="err">입력 오류 ${Object.keys(chk.errors).length}개 — 저장하지 않음</b>`;
    if (st.cfgMsg) msg += ` · ${st.cfgMsg}`;
    $('cfgStatus').innerHTML = msg;
    $('cfgNotice').textContent = st.cfgNotice ? ' ⚠ ' + st.cfgNotice : '';
  }
  function render() {
    const k = ckey(), dr = drafts.get(k);
    $('cfgDirty').hidden = !(dr && isDirty(dr));
    btn.setAttribute('aria-expanded', st.cfgOpen ? 'true' : 'false');
    panel.hidden = !st.cfgOpen;
    if (!st.cfgOpen) { delete panel.dataset.sk; return; } // closed: next open always rebuilds from the current draft
    const cur = draft(), list = E.profilesFor(st.ecu, st.transport), act = E.activeProfile(st.ecu, st.transport);
    const sk = `${k}|${cur.targetId}|${cur.v}|${list.map(p => p.id + ':' + p.rev).join(',')}|${act.id}`;
    if (panel.dataset.sk !== sk) {
      const f = d.activeElement && panel.contains(d.activeElement) ? d.activeElement.id : null;
      build(cur);
      if (f && $(f)) $(f).focus();
    }
    update(cur);
    $('cfgDirty').hidden = !isDirty(cur);
  }

  /* ---------- actions ---------- */
  function open(v) {
    st.cfgOpen = v; st.cfgMsg = '';
    render();
    if (v) { const f = $('cfgProfile'); if (f) f.focus(); } else btn.focus();
  }
  function reload(dr) { drafts.set(ckey(), seal(dr)); st.cfgMsg = ''; render(); }
  function save() {
    const dr = draft(), tg = target(dr);
    dr.tried = true;
    const presetClean = tg && tg.source === 'preset' && !isDirty(dr);
    const res = E.saveProfile(st.ecu, st.transport, { targetId: dr.targetId, name: dr.name, values: presetClean ? tg.values : dr.vals });
    if (!res.ok) {
      st.cfgMsg = res.code === 'invalid' ? '' : `<b class="err">${esc(res.error)}</b>`;
      render();
      if (res.code === 'invalid') { const first = Object.keys(res.errors)[0], el = $('cf-' + first); if (el) el.focus(); }
      else O.toast(res.error, 'err');
      return res;
    }
    drafts.set(ckey(), seal(fromProfile(res.profile)));
    const p = res.profile, what = p.source === 'preset' ? '프리셋 적용' : `${res.action === 'created' ? '새 프로필 저장' : '저장'}: ${p.name} r${p.rev}`;
    st.cfgMsg = `<b class="ok">${esc(what)}</b> — 다음 [연결]부터 사용`;
    O.render();
    O.toast(`${st.ecu} · ${O.trLabel(st.transport)} ${what} · 다음 [연결]부터 사용`);
    return res;
  }
  function act(cmd) {
    const dr = draft(), tg = target(dr);
    if (cmd !== 'delete') dr.confirmDel = false;
    switch (cmd) {
      case 'dup': {
        const src = tg || E.activeProfile(st.ecu, st.transport);
        reload({ targetId: 'new', name: suggestName(`${src.name} 사본`.slice(0, U.LIMITS.name - 2)), vals: Object.assign({}, dr.vals), v: dr.v, tried: false });
        const n = $('cf-name'); if (n) { n.focus(); n.select && n.select(); }
        break;
      }
      case 'preset': { const p = U.presetProfile(st.ecu, st.transport); dr.vals = fmtValues(p); dr.v = ++draftGen; st.cfgMsg = '프리셋 값을 초안에 불러옴 (저장 전 미적용)'; render(); break; }
      case 'revert': reload(dr.targetId === 'new' ? fromProfile(E.activeProfile(st.ecu, st.transport)) : Object.assign(fromProfile(tg), { v: dr.v })); break;
      case 'cancel': drafts.delete(ckey()); open(false); break;
      case 'delete': {
        if (!tg || tg.source !== 'custom') return;
        if (!dr.confirmDel) { dr.confirmDel = true; render(); return; }
        const r = E.deleteProfile(st.ecu, st.transport, tg.id);
        dr.confirmDel = false;
        if (!r.ok) { st.cfgMsg = `<b class="err">${esc(r.error)}</b>`; O.toast(r.error, 'err'); render(); return; }
        drafts.delete(ckey());
        O.render();
        st.cfgMsg = `<b>${esc(tg.name)} 삭제</b>${r.wasActive ? ' — 적용 중이던 프로필이어서 프리셋으로 복귀' : ''}`;
        render(); $('cfgProfile').focus();
        break;
      }
      case 'disconnect': E.disconnect(st.ecu, st.transport, 'user'); O.render(); break;
      case 'export': exportCfg(); break;
      case 'import': $('cfgFile').click(); break;
    }
  }
  function exportCfg() {
    const cfg = E.exportConfig();
    if (!cfg.profiles.length) { O.toast('내보낼 사용자 프로필 없음 (프리셋은 내장)'); return null; }
    const text = JSON.stringify(Object.assign({ exportedAt: new Date().toISOString(), note: 'Loom UDS Read DID local mock connection profiles. Presets are built-in and not included.' }, cfg), null, 2);
    window.LoomUdsTools.download(`loom-uds-read-did-config-${window.LoomUdsTools.stamp()}.json`, text, 'application/json');
    return text;
  }
  const MAX_IMPORT = 256 * 1024;
  function importText(text) {
    let obj, res;
    if (typeof text !== 'string' || text.length > MAX_IMPORT) res = { ok: false, error: '파일이 비었거나 256 KB 초과' };
    else { try { obj = JSON.parse(text); } catch (e) { res = { ok: false, error: 'JSON 구문 오류' }; } }
    if (!res) res = E.importConfig(obj);
    st.cfgMsg = res.ok ? `<b class="ok">가져옴 ${res.added}개</b>${res.renamed ? ` (이름 충돌 ${res.renamed}개에 (n) 접미사)` : ''} — 적용·연결하지 않음` : `<b class="err">가져오기 거부: ${esc(res.error)}</b> — 변경 없음`;
    O.toast(res.ok ? `설정 ${res.added}개 추가 (적용 안 함)` : `가져오기 거부: ${res.error}`, res.ok ? '' : 'err');
    O.render();
    return res;
  }

  /* ---------- events ---------- */
  btn.addEventListener('click', () => open(!st.cfgOpen));
  panel.addEventListener('submit', e => { e.preventDefault(); save(); });
  panel.addEventListener('click', e => { const b = e.target.closest('button[data-cfg]'); if (b && !b.disabled) act(b.dataset.cfg); });
  panel.addEventListener('input', e => {
    const k = e.target.dataset && e.target.dataset.k; if (!k) return;
    const dr = draft(); dr.confirmDel = false; st.cfgMsg = '';
    if (k === 'name') dr.name = e.target.value; else dr.vals[k] = e.target.value;
    update(dr); $('cfgDirty').hidden = !isDirty(dr);
  });
  panel.addEventListener('change', e => {
    if (e.target.id === 'cfgProfile') { const p = E.profile(e.target.value); if (p && !isDirty(draft())) reload(fromProfile(p)); return; }
    if (e.target.id === 'cfgFile') {
      const f = e.target.files && e.target.files[0]; e.target.value = '';
      if (!f) return;
      if (f.size > MAX_IMPORT) { importText(null); return; }
      f.text().then(importText, () => importText(null));
      return;
    }
    if (e.target.tagName === 'SELECT' && e.target.dataset.k) { const dr = draft(); dr.vals[e.target.dataset.k] = e.target.value; st.cfgMsg = ''; update(dr); $('cfgDirty').hidden = !isDirty(dr); }
  });
  panel.addEventListener('keydown', e => { if (e.key === 'Escape') { e.preventDefault(); open(false); } });

  window.LoomUdsConfig = { render, open, save, act, importText, exportCfg, drafts, draft, isDirty };
  render();
  if (st.cfgNotice) O.toast(st.cfgNotice, 'err');
})();
