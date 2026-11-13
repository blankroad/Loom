/* Loom UDS workbench — ECU / DID catalogue (items 4 + 5), core-owned. Uses the shared host H + engine catalog API.
   Presets (BCM/GW, F190/F18C/0101) are read-only. Custom ECUs/DIDs and per-ECU SIM fixture payloads are local SIM
   definitions (never ODX, never real vehicle data). Forms keep their drafts across renders; lists patch only on change. */
(function () {
  'use strict';
  const H = window.LoomWorkbench; if (!H) return;
  const U = H.constants, E = H.engine, C = E.catalog, d = document, esc = H.ui.esc, O = H._core.O;
  const mount = d.getElementById('catalogMount');
  const $m = id => mount.querySelector('#' + id);
  const TRL = id => U.TRANSPORTS.find(t => t.id === id).label;

  /* ---------- state ---------- */
  let ecuForm = null;   // { mode:'new'|'edit', id, vals:{...}, tried }
  let didForm = null;   // { mode:'new'|'edit', did, vals:{...}, fx:{ecu, text}, allEcus, tried }
  let delEcu = null, delDid = null, msg = '', msgCls = '';
  const say = (t, c) => { msg = t; msgCls = c || ''; const m = $m('ctMsg'); if (m) { m.textContent = t; m.className = 'ct-msg' + (c ? ' ' + c : ''); } };
  const hx = (n, w) => n.toString(16).toUpperCase().padStart(w, '0');

  /* ---------- ECU ---------- */
  function ecuReasons(id) {
    const out = [];
    U.TRANSPORTS.forEach(t => { const c = E.ctx(id, t.id); if (c.link === 'online' || c.active) out.push({ kind: c.active ? 'busy' : 'online', tr: t.id, label: `${TRL(t.id)} ${c.active ? '요청 #' + E.byId(c.active).no + ' 진행 중' : '연결됨'}` }); });
    H.activity.forEcu(id).forEach(a => out.push({ kind: 'activity', key: a.key, label: `${a.label || a.kind} (${a.transport ? TRL(a.transport) : '전체'})` }));
    return out;
  }
  function ecuList() {
    return `<div class="ct-scroll" role="region" aria-label="ECU 목록" tabindex="0"><table class="ftab ct-tab"><thead><tr><th>ECU</th><th>이름</th><th>DoIP LA</th><th>CAN Tx/Rx</th><th>연결</th><th></th></tr></thead><tbody>` +
      U.ECUS.map(e => {
        const on = U.TRANSPORTS.filter(t => E.ctx(e.id, t.id).link === 'online').map(t => TRL(t.id));
        const acts = H.activity.forEcu(e.id).length;
        return `<tr data-ecu="${esc(e.id)}"${e.id === H.context().ecu ? ' class="cur"' : ''}><td class="v">${esc(e.id)} ${e.custom ? '<span class="ct-b">사용자</span>' : '<span class="ct-b pre">프리셋</span>'}</td><td>${esc(e.name)}</td><td class="v">0x${hx(e.doipLA, 4)}</td><td class="v">0x${hx(e.canTx, 3)}/0x${hx(e.canRx, 3)}</td>` +
          `<td class="n">${on.length ? '● ' + esc(on.join(', ')) : '—'}${acts ? ` · 자동 ${acts}` : ''}</td><td class="ct-act">` +
          `<button type="button" class="btn ghost" data-ct="ecu-sel" title="이 ECU를 대상으로 선택 (전송 없음)">선택</button>` +
          (e.custom ? `<button type="button" class="btn ghost" data-ct="ecu-edit">편집</button><button type="button" class="btn ghost danger" data-ct="ecu-del">${delEcu === e.id ? '삭제 확인' : '삭제'}</button>` : '') + '</td></tr>';
      }).join('') + '</tbody></table></div>';
  }
  function delEcuBox() {
    if (!delEcu) return '';
    const rs = ecuReasons(delEcu), nProf = E.exportConfig().profiles.filter(p => p.ecu === delEcu).length;
    if (!rs.length) return `<div class="ct-warn">ECU <b>${esc(delEcu)}</b> 삭제: 사용자 연결 프로필 ${nProf}개도 함께 삭제됩니다. 기록·확정 값의 ECU 표기는 그대로 유지됩니다. [삭제 확인]을 다시 누르세요. <button type="button" class="btn ghost" data-ct="ecu-del-cancel">취소</button></div>`;
    return `<div class="ct-warn err">ECU <b>${esc(delEcu)}</b> 삭제 불가 — 먼저 정리하세요 (자동 해제·중지 없음):<ul>${rs.map(r => `<li>${esc(r.label)}</li>`).join('')}</ul>` +
      (rs.some(r => r.kind !== 'activity') ? `<button type="button" class="btn" data-ct="ecu-disc" title="이 ECU의 모든 경로 연결 해제 (진행 중 요청은 링크 끊김으로 종료)">이 ECU 연결 모두 해제</button>` : '') +
      (rs.some(r => r.kind === 'activity') ? `<button type="button" class="btn" data-ct="ecu-stopact" title="이 ECU 관련 자동 작업 중지 (소유 모듈의 stop)">자동 작업 중지</button>` : '') +
      ` <button type="button" class="btn ghost" data-ct="ecu-del-cancel">취소</button></div>`;
  }
  const ECU_F = [['id', 'ECU ID', '영문 대문자 시작 2–8자 (A–Z 0–9 _) · 생성 후 변경 불가'], ['name', '이름', `1–${U.CAT_LIMITS.ecuName}자`], ['doipLA', 'DoIP 논리 주소 0x', '16-bit hex · Tester 0x0E00 제외 · ECU 간 고유'], ['canTx', 'CAN 요청 ID 0x', '11-bit hex · 고유'], ['canRx', 'CAN 응답 ID 0x', '11-bit hex · 고유']];
  function ecuFormHtml() {
    if (!ecuForm) return `<button type="button" class="btn" data-ct="ecu-new">+ 사용자 ECU</button>`;
    const f = ecuForm;
    return `<form class="ct-form" id="ctEcuForm" novalidate autocomplete="off"><b>${f.mode === 'new' ? '새 사용자 ECU' : 'ECU 편집 · ' + esc(f.id)}</b><div class="ct-grid">` +
      ECU_F.map(([k, l, h]) => `<div class="cf"><label class="cf-l" for="ce-${k}">${l}</label><input class="fld" id="ce-${k}" data-ek="${k}" value="${esc(f.vals[k])}" spellcheck="false"${k === 'id' && f.mode === 'edit' ? ' disabled' : ''} aria-describedby="ce-${k}-e"><small class="cf-hint">${esc(h)}</small><small class="cf-err" id="ce-${k}-e" role="status"></small></div>`).join('') +
      `</div><p class="ct-note">경로별 프리셋 프로필은 이 주소에서 생성됩니다 (DoIP 192.168.10.20:13400 · CAN1 500k). 주소 변경은 이 ECU의 모든 경로가 연결 해제·유휴일 때만. SIM ECU는 F190/F18C/0101 기본 fixture로 응답합니다.</p>` +
      `<div class="ct-foot"><span class="grow"></span><button type="button" class="btn" data-ct="ecu-cancel">취소</button><button type="submit" class="btn primary">${f.mode === 'new' ? '추가' : '저장'}</button></div></form>`;
  }
  function ecuErrors() {
    if (!ecuForm) return;
    const f = ecuForm, r = f.mode === 'new' ? C.checkEcu(f.vals) : C.checkEcu(Object.assign({}, f.vals, { id: f.id }), f.id);
    ECU_F.forEach(([k]) => { const e = (f.tried || f.touched[k]) ? r.errors[k] || '' : '', el = $m('ce-' + k); if (el) { el.classList.toggle('invalid', !!e); el.setAttribute('aria-invalid', e ? 'true' : 'false'); $m('ce-' + k + '-e').textContent = e; } });
    return r;
  }
  function saveEcu() {
    const f = ecuForm; f.tried = true; ecuErrors();
    const r = f.mode === 'new' ? C.addEcu(f.vals) : C.updateEcu(f.id, f.vals);
    if (!r.ok) { say(r.error, 'err'); if (r.errors) { const k = Object.keys(r.errors)[0], el = $m('ce-' + k); if (el) el.focus(); } return r; }
    say(f.mode === 'new' ? `ECU ${r.ecu} 추가 — 대상 목록에 추가됨 (연결 안 함)` : `ECU ${r.ecu} 저장${r.addressChanged ? ' · 프리셋 주소 갱신 (다음 연결부터)' : ''}`);
    ecuForm = null; full();
    return r;
  }

  /* ---------- DID ---------- */
  const DID_DEF = { did: '', name: '', long: '', type: 'uint', recLen: '2', byteOffset: '0', length: '2', endian: 'big', scale: '1', offset: '0', decimals: '0', unit: '' };
  function didList() {
    return `<div class="ct-scroll" role="region" aria-label="DID 목록" tabindex="0"><table class="ftab ct-tab"><thead><tr><th>DID</th><th>이름</th><th>정의</th><th>SIM fixture (${esc(H.context().ecu)})</th><th></th></tr></thead><tbody>` +
      U.DIDS.map(x => {
        const fx = x.custom ? C.fixture(H.context().ecu, x.did) : null;
        const def = x.custom ? `${x.type} @${x.byteOffset}+${x.length} / ${x.recLen} B` + (x.type === 'uint' || x.type === 'sint' ? ` ${x.endian === 'little' ? 'LE' : 'BE'} ×${x.scale}${x.offset ? (x.offset > 0 ? '+' : '') + x.offset : ''}${x.unit ? ' ' + x.unit : ''}` : '') : x.enc === 'u16' ? `u16 ×${x.scale} ${x.unit} (fixture)` : `ASCII${x.len ? ' ' + x.len + ' B' : ''} (fixture)`;
        return `<tr data-did="${U.hex4(x.did)}"><td class="v">${U.hex4(x.did)} ${x.custom ? '<span class="ct-b">사용자</span>' : '<span class="ct-b pre">프리셋</span>'}</td><td>${esc(x.name)}</td><td class="n">${esc(def)}</td>` +
          `<td class="v fx">${x.custom ? (fx ? esc(U.hexs(fx)) + (fx.length !== x.recLen ? ' <span class="bad">길이≠</span>' : '') : '<span class="na">없음 → 7F 22 31</span>') : '<span class="na">내장</span>'}</td><td class="ct-act">` +
          `<button type="button" class="btn ghost" data-ct="did-use" title="요청 편집기에 채우기 (전송 없음)">편집기로</button>` +
          (x.custom ? `<button type="button" class="btn ghost" data-ct="did-edit">편집</button><button type="button" class="btn ghost danger" data-ct="did-del">${delDid === x.did ? '삭제 확인' : '삭제'}</button>` : '') + '</td></tr>';
      }).join('') + '</tbody></table></div>';
  }
  const DF = [['did', 'DID 0x', '4자리 hex · 프리셋/기존과 중복 불가'], ['name', '이름', `1–${U.CAT_LIMITS.didName}자`], ['long', '설명', '선택'], ['type', '형식'], ['recLen', '레코드 길이 B', `응답 dataRecord 기대 길이 1–${U.CAT_LIMITS.fixtureBytes}`], ['byteOffset', '바이트 오프셋', '레코드 안 값 시작'], ['length', '값 길이 B', `uint/sint ≤ ${U.CAT_LIMITS.intBytes}`], ['endian', '엔디언'], ['scale', '배율', 'phys = raw × 배율 + 오프셋'], ['offset', '오프셋 (값)', ''], ['decimals', '소수 자릿수', '0–6'], ['unit', '단위', `0–${U.CAT_LIMITS.unit}자`]];
  const INT_ONLY = ['endian', 'scale', 'offset', 'decimals', 'unit'];
  function didFormHtml() {
    if (!didForm) return `<button type="button" class="btn" data-ct="did-new">+ 사용자 DID</button>`;
    const f = didForm, sel = (k, opts) => `<select class="fld" id="cd-${k}" data-dk="${k}">${opts.map(([v, l]) => `<option value="${v}"${f.vals[k] === v ? ' selected' : ''}>${l}</option>`).join('')}</select>`;
    const ctl = k => (k === 'type' ? sel('type', U.DID_TYPES.map(t => [t, t])) : k === 'endian' ? sel('endian', [['big', 'big (BE)'], ['little', 'little (LE)']]) : `<input class="fld" id="cd-${k}" data-dk="${k}" value="${esc(f.vals[k])}" spellcheck="false"${k === 'did' && f.mode === 'edit' ? ' disabled' : ''} aria-describedby="cd-${k}-e">`);
    return `<form class="ct-form" id="ctDidForm" novalidate autocomplete="off"><b>${f.mode === 'new' ? '새 사용자 DID' : 'DID 편집 · ' + esc(f.vals.did)}</b><div class="ct-grid">` +
      DF.map(([k, l, h]) => `<div class="cf${INT_ONLY.includes(k) ? ' int-only' : ''}" data-cdf="${k}"><label class="cf-l" for="cd-${k}">${l}</label>${ctl(k)}${h ? `<small class="cf-hint">${esc(h)}</small>` : ''}<small class="cf-err" id="cd-${k}-e" role="status"></small></div>`).join('') +
      `</div><fieldset class="ct-fx"><legend>SIM fixture 응답 데이터 (ODX 아님)</legend>` +
      `<div class="ct-row"><label for="cd-fxEcu">ECU</label><select class="fld" id="cd-fxEcu">${U.ECUS.map(e => `<option value="${esc(e.id)}"${e.id === f.fx.ecu ? ' selected' : ''}>${esc(e.id)}</option>`).join('')}</select>` +
      `<button type="button" class="btn ghost" data-ct="fx-zero" title="레코드 길이만큼 00으로 채우기">0 채우기</button>` +
      (f.mode === 'new' ? `<label class="ct-ck"><input type="checkbox" id="cd-fxAll"${f.allEcus ? ' checked' : ''}> 모든 ECU에 같은 fixture</label>` : `<button type="button" class="btn ghost" data-ct="fx-clear" title="이 ECU에서 fixture 제거 → 7F 22 31">fixture 제거</button>`) + `</div>` +
      `<textarea class="fld ct-fxin" id="cd-fx" rows="2" spellcheck="false" aria-describedby="cd-fx-e cdPrev">${esc(f.fx.text)}</textarea><small class="cf-err" id="cd-fx-e" role="status"></small>` +
      `<div class="ct-prev" id="cdPrev" aria-live="polite"></div></fieldset>` +
      `<div class="ct-foot"><span class="grow"></span><button type="button" class="btn" data-ct="did-cancel">취소</button><button type="submit" class="btn primary">${f.mode === 'new' ? '추가' : '저장'}</button></div></form>`;
  }
  function didCheck() {
    const f = didForm;
    return f.mode === 'new' ? C.checkDid(f.vals) : C.checkDid(Object.assign({}, f.vals), f.did);
  }
  function didErrors() {
    if (!didForm) return null;
    const f = didForm, r = didCheck(), isInt = f.vals.type === 'uint' || f.vals.type === 'sint';
    mount.querySelectorAll('#ctDidForm .int-only').forEach(el => { el.hidden = !isInt; });
    DF.forEach(([k]) => { const e = (f.tried || f.touched[k]) ? r.errors[k] || '' : '', el = $m('cd-' + k); if (el) { el.classList.toggle('invalid', !!e); el.setAttribute('aria-invalid', e ? 'true' : 'false'); $m('cd-' + k + '-e').textContent = e; } });
    // fixture + preview: exact bytes → decoded with THIS draft definition
    const p = C.parseFixture(f.fx.text), fxe = $m('cd-fx-e');
    fxe.textContent = p.ok ? '' : p.error;
    $m('cd-fx').classList.toggle('invalid', !p.ok);
    let pv = '';
    if (!r.ok) pv = `<span class="na">정의 오류 ${Object.keys(r.errors).length}개 — 미리보기 없음</span>`;
    else if (!p.ok) pv = '<span class="na">fixture hex 오류</span>';
    else if (!p.bytes.length) pv = `<span class="na">fixture 없음 → SIM ECU ${esc(f.fx.ecu)}는 7F 22 31 (requestOutOfRange)</span>`;
    else {
      const dv = U.decodeCustom(r.value, p.bytes), pdu = [0x62, r.value.did >> 8, r.value.did & 0xff].concat(p.bytes);
      pv = `<div><span class="k">기대 응답 PDU</span> <code>${U.hexs(pdu)}</code> <small>${pdu.length} B (dataRecord ${p.bytes.length} / 기대 ${r.value.recLen} B)</small></div>` +
        `<div><span class="k">해석</span> <b class="${dv.ok ? '' : 'bad'}">${esc(dv.text)}${dv.unit ? ' ' + esc(dv.unit) : ''}</b> <small>${esc(dv.note)}</small></div>`;
    }
    $m('cdPrev').innerHTML = pv;
    return { r, p };
  }
  function saveDid() {
    const f = didForm; f.tried = true;
    const { r, p } = didErrors();
    if (!r.ok) { say('DID 정의 입력 오류 — 저장하지 않음', 'err'); const k = Object.keys(r.errors)[0], el = $m('cd-' + k); if (el) el.focus(); return null; }
    if (!p.ok) { say('fixture hex 오류 — 저장하지 않음', 'err'); $m('cd-fx').focus(); return null; }
    let res;
    if (f.mode === 'new') {
      const targets = p.bytes.length ? (f.allEcus ? U.ECUS.map(e => e.id) : [f.fx.ecu]) : [];
      res = C.addDid(f.vals, targets.map(id => [id, p.bytes]));
    } else {
      res = C.updateDid(f.did, f.vals);
      if (res.ok) res = Object.assign(C.setFixture(f.fx.ecu, f.did, p.bytes.length ? p.bytes : null), { did: f.did });
    }
    if (!res.ok) { say(res.error, 'err'); return res; }
    say(`DID ${U.hex4(res.did)} ${f.mode === 'new' ? '추가' : '저장'} — 요청 편집기/목록에 반영 (전송 없음). 기존 기록은 요청 시점 정의 유지.`);
    didForm = null; full();
    return res;
  }
  function openDid(x) {
    const ecu = H.context().ecu;
    if (!x) { didForm = { mode: 'new', did: null, vals: Object.assign({}, DID_DEF), fx: { ecu, text: '00 00' }, allEcus: true, tried: false, touched: {} }; }
    else {
      const fx = C.fixture(ecu, x.did);
      didForm = { mode: 'edit', did: x.did, vals: { did: U.hex4(x.did), name: x.name, long: x.long || '', type: x.type, recLen: String(x.recLen), byteOffset: String(x.byteOffset), length: String(x.length), endian: x.endian, scale: String(x.scale), offset: String(x.offset), decimals: String(x.decimals), unit: x.unit || '' }, fx: { ecu, text: fx ? U.hexs(fx) : '' }, allEcus: false, tried: false, touched: {} };
    }
    full(); const el = $m(x ? 'cd-name' : 'cd-did'); if (el) el.focus();
  }

  /* ---------- render ---------- */
  let listSig = '';
  function sigOf() { const cx = H.context(); return [cx.ecu, U.ECUS.map(e => e.id + e.name + e.doipLA + e.canTx + e.canRx).join(), U.DIDS.map(x => x.did + x.name + (x.custom ? x.recLen + x.type + x.byteOffset + x.length + x.scale + x.offset + x.unit + x.endian : '')).join(), JSON.stringify(C.exportStore().fixtures), U.ECUS.map(e => U.TRANSPORTS.map(t => E.ctx(e.id, t.id).link[1] + (E.ctx(e.id, t.id).active ? 1 : 0)).join('')).join(), H.activity.list().length, delEcu, delDid].join('|'); }
  function lists() {
    const s = sigOf(); if (s === listSig) return; listSig = s;
    const f = d.activeElement && mount.contains(d.activeElement) && d.activeElement.closest('tr[data-ecu],tr[data-did]') ? [d.activeElement.closest('tr').dataset.ecu, d.activeElement.closest('tr').dataset.did, d.activeElement.dataset.ct] : null;
    $m('ctEcuList').innerHTML = ecuList(); $m('ctEcuDel').innerHTML = delEcuBox(); $m('ctDidList').innerHTML = didList();
    if (f) { const tr = mount.querySelector(f[0] ? `tr[data-ecu="${f[0]}"]` : `tr[data-did="${f[1]}"]`), b = tr && tr.querySelector(`[data-ct="${f[2]}"]`); if (b) b.focus(); }
    $m('ctCount').textContent = `사용자 ECU ${U.ECUS.filter(e => e.custom).length}/${U.CAT_LIMITS.ecus} · 사용자 DID ${U.DIDS.filter(x => x.custom).length}/${U.CAT_LIMITS.dids}`;
  }
  function full() {
    mount.innerHTML =
      `<p class="ct-top"><span id="ctCount"></span> · 프리셋은 변경·삭제 불가. 정의·fixture는 로컬 SIM 전용 (ODX·실제 차량 데이터 아님).</p>` +
      `<section class="sv-sec"><h3>ECU</h3><div id="ctEcuList"></div><div id="ctEcuDel"></div><div id="ctEcuForm">${ecuFormHtml()}</div></section>` +
      `<section class="sv-sec"><h3>DID 카탈로그</h3><div id="ctDidList"></div><div id="ctDidForm">${didFormHtml()}</div></section>` +
      `<p class="ct-msg${msgCls ? ' ' + msgCls : ''}" id="ctMsg" aria-live="polite">${esc(msg)}</p>`;
    listSig = ''; lists(); ecuErrors(); if (didForm) didErrors();
  }

  /* ---------- events ---------- */
  mount.addEventListener('click', e => {
    const b = e.target.closest('[data-ct]'); if (!b || b.disabled) return;
    const k = b.dataset.ct, tr = b.closest('tr'), eid = tr && tr.dataset.ecu, did = tr && tr.dataset.did ? parseInt(tr.dataset.did, 16) : null;
    if (k !== 'ecu-del' && k !== 'ecu-disc' && k !== 'ecu-stopact') delEcu = null;
    if (k !== 'did-del') delDid = null;
    switch (k) {
      case 'ecu-sel': O.setContext(eid, null); say(`대상 ECU ${eid} 선택 (전송·연결 없음)`); break;
      case 'ecu-new': ecuForm = { mode: 'new', vals: { id: '', name: '', doipLA: '', canTx: '', canRx: '' }, tried: false, touched: {} }; full(); $m('ce-id').focus(); return;
      case 'ecu-edit': { const x = U.ECUS.find(q => q.id === eid); ecuForm = { mode: 'edit', id: eid, vals: { id: eid, name: x.name, doipLA: hx(x.doipLA, 4), canTx: hx(x.canTx, 3), canRx: hx(x.canRx, 3) }, tried: false, touched: {} }; full(); $m('ce-name').focus(); return; }
      case 'ecu-cancel': ecuForm = null; full(); return;
      case 'ecu-del': {
        if (delEcu !== eid) { delEcu = eid; break; }
        const rs = ecuReasons(eid);
        if (rs.length) { say(`${eid} 삭제 불가 — 연결/자동 작업 정리 필요`, 'err'); break; }
        const r = C.deleteEcu(eid); delEcu = null;
        say(r.ok ? `ECU ${eid} 삭제 · 사용자 프로필 ${r.profilesRemoved}개 삭제 · 기존 기록은 변경 없음` : r.error, r.ok ? '' : 'err');
        if (r.ok && ecuForm && ecuForm.id === eid) ecuForm = null;
        full(); return;
      }
      case 'ecu-del-cancel': delEcu = null; break;
      case 'ecu-disc': { const id = delEcu; U.TRANSPORTS.forEach(t => { if (E.ctx(id, t.id).link === 'online') E.disconnect(id, t.id, 'user'); }); delEcu = id; say(`${id}: 모든 경로 연결 해제 (사용자)`); break; }
      case 'ecu-stopact': { const id = delEcu; [...new Set(H.activity.forEcu(id).map(a => a.key))].forEach(key => H.activity.stop(key)); delEcu = id; say(`${id}: 관련 자동 작업 중지 요청 (모듈별 stop — 해당 모듈의 다른 대상도 중지될 수 있음)`); break; }
      case 'did-new': openDid(null); return;
      case 'did-edit': openDid(U.didInfo(did)); return;
      case 'did-cancel': didForm = null; full(); return;
      case 'did-use': H.compose({ mode: 'did', didText: U.hex4(did) }); say(`DID ${U.hex4(did)} 편집기에 채움 (전송 없음)`); break;
      case 'did-del': {
        if (delDid !== did) { delDid = did; break; }
        const r = C.deleteDid(did); delDid = null;
        say(r.ok ? `DID ${U.hex4(did)} 삭제 · 기존 기록·확정 값은 요청 시점 정의/값 유지` : r.error, r.ok ? '' : 'err');
        if (r.ok && didForm && didForm.did === did) didForm = null;
        full(); return;
      }
      case 'fx-zero': { const n = parseInt(didForm.vals.recLen, 10); if (n >= 1 && n <= U.CAT_LIMITS.fixtureBytes) { didForm.fx.text = Array(n).fill('00').join(' '); $m('cd-fx').value = didForm.fx.text; didErrors(); } return; }
      case 'fx-clear': didForm.fx.text = ''; $m('cd-fx').value = ''; didErrors(); return;
    }
    H.refresh(); lists();
  });
  mount.addEventListener('submit', e => { e.preventDefault(); if (e.target.id === 'ctEcuForm') saveEcu(); else if (e.target.id === 'ctDidForm') saveDid(); });
  mount.addEventListener('input', e => {
    const t = e.target;
    if (t.dataset.ek && ecuForm) { ecuForm.vals[t.dataset.ek] = t.value; ecuForm.touched[t.dataset.ek] = true; ecuErrors(); }
    else if (t.dataset.dk && didForm) { didForm.vals[t.dataset.dk] = t.value; didForm.touched[t.dataset.dk] = true; didErrors(); }
    else if (t.id === 'cd-fx' && didForm) { didForm.fx.text = t.value; didErrors(); }
  });
  mount.addEventListener('change', e => {
    const t = e.target;
    if (t.dataset.dk && didForm) { didForm.vals[t.dataset.dk] = t.value; didErrors(); }
    else if (t.id === 'cd-fxEcu' && didForm) { didForm.fx.ecu = t.value; if (didForm.mode === 'edit') { const fx = C.fixture(t.value, didForm.did); didForm.fx.text = fx ? U.hexs(fx) : ''; $m('cd-fx').value = didForm.fx.text; } didErrors(); }
    else if (t.id === 'cd-fxAll' && didForm) didForm.allEcus = t.checked;
  });
  mount.addEventListener('keydown', e => { if (e.key === 'Escape' && (ecuForm || didForm) && e.target.closest('form')) { e.preventDefault(); if (e.target.closest('#ctEcuForm')) ecuForm = null; else didForm = null; full(); } });
  H.onRender(() => { if (mount.firstChild) lists(); });
  H.subscribe(ev => { if (ev.type === 'catalog' && mount.firstChild) lists(); });
  full();
  window.LoomUdsCatalog = { full, lists, saveEcu, saveDid, openDid, state: () => ({ ecuForm, didForm, delEcu, delDid }) };
})();
