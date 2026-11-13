/* Loom Ethernet mockup (Opus) — TCP/UDP socket test (SIM only).
   Per-channel independent session; TX/RX are built as real frames and fed to that channel's capture.
   Never opens an OS socket and never performs a network request. */
(function () {
  'use strict';
  const L = window.LoomEth, P = L.P, S = L.S, esc = L.esc;
  const $ = s => document.querySelector(s);
  const SEED = [
    { name: 'DoIP 라우팅 활성화 (0x0005)', proto: 'tcp', port: 13400, fmt: 'hex', data: '02 FD 00 05 00 00 00 07 0E 00 00 00 00 00 00' },
    { name: 'DoIP TesterPresent 3E 00', proto: 'tcp', port: 13400, fmt: 'hex', data: '02 FD 80 01 00 00 00 06 0E 00 10 01 3E 00' },
    { name: 'DoIP ReadDID F190 (VIN)', proto: 'tcp', port: 13400, fmt: 'hex', data: '02 FD 80 01 00 00 00 07 0E 00 10 01 22 F1 90' },
    { name: 'DoIP 차량 식별 요청 (UDP)', proto: 'udp', port: 13400, fmt: 'hex', data: '02 FD 00 01 00 00 00 00' },
    { name: 'SOME/IP GetVersion 0x1234/0x0001', proto: 'udp', port: 30501, fmt: 'hex', data: '12 34 00 01 00 00 00 08 00 0E 00 01 01 01 00 00' },
    { name: '상태 질의 STAT?', proto: 'tcp', port: 7000, fmt: 'text', data: 'STAT?\\r\\n' },
    { name: 'Echo hello', proto: 'tcp', port: 7, fmt: 'text', data: 'hello loom\\r\\n' }
  ];
  let tpls = L.store.get('templates', null);
  if (!Array.isArray(tpls)) { tpls = SEED.map((t, i) => ({ id: 't' + (i + 1), ...t })); L.store.set('templates', tpls); }
  const draft = Object.assign({ fmt: 'hex', data: '', target: null, repN: '10', repMs: '200' }, L.store.get('draft', {}));
  const sess = new Map(); // chId -> session (never persisted: reload starts closed)
  let open = !!(S.layout && S.layout.sock), editing = null, logVer = -1, built = null;

  function chOf(id) { return S.chans.find(c => c.id === id); }
  function target() {
    let ch = chOf(draft.target) || chOf(S.activeCh) || S.chans[0];
    if (ch) draft.target = ch.id;
    return ch;
  }
  function getSess(ch) {
    if (!sess.has(ch.id)) {
      const pr = L.PROFILES[ch.profile], d = pr.def;
      sess.set(ch.id, { proto: d.proto, host: d.host, port: d.port, state: 'closed', lport: 51000 + ch.slot * 100, seq: 0, ack: 0, log: [], ver: 0, repeat: null, routing: false, why: '' });
    }
    return sess.get(ch.id);
  }
  function peerOf(ch, s) { return L.hostOf(ch, s.host); }
  function peerDef(ch, s) { return L.PROFILES[ch.profile].peers.find(p => p.host === s.host); }
  function listening(ch, s) { const pd = peerDef(ch, s); return !!pd && (pd[s.proto] || []).includes(+s.port); }
  function log(s, dir, bytes, note, no) { s.log.push({ t: S.now, dir, bytes, note, no }); if (s.log.length > 500) s.log.splice(0, s.log.length - 500); s.ver++; }
  const isOpen = s => s.state === 'est' || s.state === 'udp';

  /* ---- frame emit helpers (own = TX from bench) ---- */
  function emit(ch, bytes, reg, own) { const pk = L.capture(ch, bytes, S.now, reg, own); return pk ? pk.no : null; }
  function tcpOut(ch, s, flags, payload) {
    const me = ch.me, pe = peerOf(ch, s);
    const no = emit(ch, P.tcpFrame(me, pe, s.lport, +s.port, s.seq, s.ack, flags, payload, ch.ipid()), null, true);
    s.seq = (s.seq + payload.length + (flags & (P.F.SYN | P.F.FIN) ? 1 : 0)) >>> 0; return no;
  }
  function tcpIn(ch, s, flags, payload) {
    const me = ch.me, pe = peerOf(ch, s);
    const no = emit(ch, P.tcpFrame(pe, me, +s.port, s.lport, s.peerSeq, s.seq, flags, payload, ch.ipid()), null, false);
    s.peerSeq = (s.peerSeq + payload.length + (flags & (P.F.SYN | P.F.FIN) ? 1 : 0)) >>> 0; s.ack = s.peerSeq; return no;
  }

  /* ---- connect / disconnect ---- */
  function connect(ch) {
    const s = getSess(ch); s.why = '';
    if (isOpen(s) || s.state === 'pend') return;
    const vErr = validateEndpoint(s); if (vErr) { s.why = vErr; render(); return; }
    s.lport = s.lport + 1 > 51000 + ch.slot * 100 + 99 ? 51000 + ch.slot * 100 : s.lport + 1;
    if (s.proto === 'udp') {
      s.state = 'udp'; log(s, 'sys', null, `UDP 소켓 열림 (SIM) ${ch.me.ip}:${s.lport} → ${peerOf(ch, s).ip}:${s.port}`); render(); return;
    }
    const pe = peerOf(ch, s);
    s.state = 'pend'; s.seq = (0x10000 * (ch.slot + 1) + s.lport * 13) >>> 0; s.peerSeq = (0x7000000 + s.lport * 7919 + ch.slot) >>> 0; s.ack = 0; s.routing = false;
    const k = `${ch.me.ip}:${s.lport}>${pe.ip}:${s.port}`, kr = `${pe.ip}:${s.port}>${ch.me.ip}:${s.lport}`;
    Object.assign(S.isn, { [k]: s.seq, [kr]: s.peerSeq });
    const synNo = tcpOut(ch, s, P.F.SYN, []);
    log(s, 'sys', null, `SYN → ${pe.ip}:${s.port}`, synNo);
    render();
    L.schedule(1.2, () => {
      if (s.state !== 'pend' || !chOf(ch.id)) return;
      if (!listening(ch, s)) {
        const no = emit(ch, P.tcpFrame(pe, ch.me, +s.port, s.lport, 0, s.seq, P.F.RST | P.F.ACK, [], ch.ipid()), null, false);
        s.state = 'ref'; log(s, 'sys', null, `RST 수신 — ${pe.ip}:${s.port} 에서 수신 대기 중인 sim 서비스 없음 (연결 거부)`, no); render(); return;
      }
      s.ack = s.peerSeq; tcpIn(ch, s, P.F.SYN | P.F.ACK, []);
      L.schedule(0.3, () => {
        if (s.state !== 'pend') return;
        tcpOut(ch, s, P.F.ACK, []); s.state = 'est';
        log(s, 'sys', null, `연결됨 (SIM) ${ch.me.ip}:${s.lport} ⇄ ${pe.ip}:${s.port}`); render();
      });
    });
  }
  function disconnect(ch, why) {
    const s = getSess(ch); stopRepeat(s, null);
    if (s.state === 'est') {
      const no = tcpOut(ch, s, P.F.FIN | P.F.ACK, []);
      L.schedule(1, () => { if (!chOf(ch.id)) return; tcpIn(ch, s, P.F.FIN | P.F.ACK, []); L.schedule(0.3, () => { if (chOf(ch.id)) tcpOut(ch, s, P.F.ACK, []); }); });
      log(s, 'sys', null, `연결 종료 (FIN)${why ? ' — ' + why : ''}`, no);
    } else if (s.state === 'udp') log(s, 'sys', null, 'UDP 소켓 닫힘');
    else if (s.state === 'pend' || s.state === 'ref') log(s, 'sys', null, '연결 시도 취소');
    s.state = 'closed'; s.routing = false; render();
  }
  function validateEndpoint(s) {
    const port = +s.port;
    if (!Number.isInteger(port) || port < 1 || port > 65535) return '포트는 1–65535 정수';
    return '';
  }

  /* ---- deterministic SIM peers ---- */
  function respond(ch, s, data) {
    const port = +s.port, pd = peerDef(ch, s);
    if (!pd || !(pd[s.proto] || []).includes(port)) return { out: [], note: `응답 없음 — sim 피어 ${peerOf(ch, s).ip}가 ${s.proto.toUpperCase()} ${port}을 수신하지 않음` };
    if (port === 7) return { out: [data.slice()] };
    if (port === 7000) {
      const line = String.fromCharCode(...data).trim();
      return { out: [P.txt(line === 'STAT?' ? `STAT seq=manual vbat=12.4V temp=33C\r\n` : `ERR unknown command "${line.slice(0, 20)}"\r\n`)] };
    }
    if (port === 13400) return doipPeer(ch, s, data);
    if (port === 30501) return someipPeer(data);
    return { out: [] };
  }
  function doipPeer(ch, s, d) {
    const la = 0x1001, out = []; let o = 0;
    const vin = P.txt('WLOOMSIM00000' + String(1000 + ch.slot).slice(-4));
    while (o < d.length) {
      if (d.length - o < 8 || d[o] !== 0x02 || d[o + 1] !== 0xfd) { out.push(P.doip(0x0000, [0x00])); break; }
      const ty = P.u16(d, o + 2), ln = P.u32(d, o + 4), b = d.slice(o + 8, o + 8 + ln);
      if (b.length < ln) { out.push(P.doip(0x0000, [0x04])); break; }
      if (ty === 0x0001 && s.proto === 'udp') out.push(P.doip(0x0004, [...vin, ...P.p16(la), ...P.macB(L.hostOf(ch, 'ecu').mac), ...P.macB(L.hostOf(ch, 'ecu').mac), 0, 0]));
      else if (ty === 0x0005 && b.length >= 7) { s.routing = true; out.push(P.doip(0x0006, [b[0], b[1], ...P.p16(la), 0x10, 0, 0, 0, 0])); }
      else if (ty === 0x8001 && b.length >= 5) {
        const sa = [b[0], b[1]], ta = [b[2], b[3]], u = b.slice(4);
        if (!s.routing) out.push(P.doip(0x8003, [...ta, ...sa, 0x02]));
        else {
          out.push(P.doip(0x8002, [...ta, ...sa, 0x00]));
          const sid = u[0]; let r = null;
          if (sid === 0x3e) r = u[1] & 0x80 ? null : [0x7e, u[1] & 0x7f];
          else if (sid === 0x10 && u.length >= 2) r = [0x50, u[1], 0x00, 0x32, 0x01, 0xf4];
          else if (sid === 0x22 && u.length >= 3) r = P.u16(u, 1) === 0xf190 ? [0x62, 0xf1, 0x90, ...vin] : [0x7f, 0x22, 0x31];
          else r = [0x7f, sid, 0x11];
          if (r) out.push(P.doip(0x8001, [...ta, ...sa, ...r]));
        }
      } else out.push(P.doip(0x0000, [0x01]));
      o += 8 + ln;
    }
    return { out: out.length ? [out.flat()] : [] };
  }
  function someipPeer(d) {
    if (d.length < 16 || P.u32(d, 4) !== d.length - 8) return { out: [], note: '응답 없음 — SOME/IP 헤더 길이 불일치 (sim 피어가 폐기)' };
    const svc = P.u16(d, 0), mid = P.u16(d, 2), cl = P.u16(d, 8), se = P.u16(d, 10), mt = d[14];
    if (mt !== 0x00) return { out: [], note: '응답 없음 — Request(0x00)가 아닌 메시지' };
    if (svc === 0x1234 && mid === 0x0001) return { out: [P.someip(svc, mid, cl, se, d[13], 0x80, 0x00, [0x01, 0x02, 0x00])] };
    return { out: [P.someip(svc, mid, cl, se, d[13], 0x81, svc === 0x1234 ? 0x03 : 0x02, [])] };
  }

  /* ---- send ---- */
  function parsePayload() {
    const r = draft.fmt === 'hex' ? P.parseHex(draft.data) : P.parseText(draft.data);
    if (r.err) return r;
    if (r.bytes.length > 1400) return { err: `최대 1400바이트 (현재 ${r.bytes.length})` };
    return r;
  }
  function send(ch, bytesOverride, label) {
    const s = getSess(ch);
    if (!isOpen(s)) { s.why = s.state === 'pend' ? '연결 중 — 완료 후 보내기 가능' : '연결되지 않음 — 보내기 차단 (먼저 연결)'; render(); return false; }
    let bytes = bytesOverride;
    if (!bytes) { const r = parsePayload(); if (r.err) { s.why = ''; setErr(r.err); return false; } bytes = r.bytes; }
    setErr(''); s.why = '';
    const pe = peerOf(ch, s);
    let no;
    if (s.proto === 'udp') no = emit(ch, P.udpFrame(ch.me, pe, s.lport, +s.port, bytes, ch.ipid()), null, true);
    else no = tcpOut(ch, s, P.F.PSH | P.F.ACK, bytes);
    log(s, 'tx', bytes, label || '', no);
    const rsp = respond(ch, s, bytes);
    L.schedule(2 + ch.slot * 0.1, () => {
      if (!chOf(ch.id) || !isOpen(s)) return;
      if (!rsp.out.length) { if (s.proto === 'tcp') tcpIn(ch, s, P.F.ACK, []); log(s, 'sys', null, rsp.note || '응답 없음'); render(); return; }
      rsp.out.forEach(r => {
        let rno;
        if (s.proto === 'udp') rno = emit(ch, P.udpFrame(pe, ch.me, +s.port, s.lport, r, ch.ipid()), null, false);
        else rno = tcpIn(ch, s, P.F.PSH | P.F.ACK, r);
        log(s, 'rx', r, '', rno);
      });
      if (s.proto === 'tcp') L.schedule(0.3, () => { if (s.state === 'est') tcpOut(ch, s, P.F.ACK, []); });
      render();
    });
    render();
    return true;
  }
  function startRepeat(ch, count, every) {
    const s = getSess(ch);
    if (!isOpen(s)) { s.why = '연결되지 않음 — 반복 전송 불가'; render(); return; }
    const r = parsePayload(); if (r.err) { setErr(r.err); return; }
    stopRepeat(s, null);
    const rep = s.repeat = { left: count, every, sent: 0 };
    const step = () => {
      if (s.repeat !== rep || !isOpen(s) || !chOf(ch.id)) return;
      send(ch, r.bytes, `반복 ${rep.sent + 1}/${count}`); rep.sent++; rep.left--;
      if (rep.left > 0) L.schedule(every, step); else { s.repeat = null; log(s, 'sys', null, `반복 완료 (${count}회)`); render(); }
    };
    step();
  }
  function stopRepeat(s, why) { if (s.repeat) { const r = s.repeat; s.repeat = null; log(s, 'sys', null, `반복 중지 (${r.sent}회 전송)${why ? ' — ' + why : ''}`); } }

  /* ---- templates ---- */
  function saveTpls() { L.store.set('templates', tpls); }
  function runTpl(t) {
    const ch = target(); if (!ch) return;
    const s = getSess(ch);
    const r = t.fmt === 'hex' ? P.parseHex(t.data) : P.parseText(t.data);
    if (r.err) { L.toast(`템플릿 "${t.name}" 데이터 오류: ${r.err}`, 'err'); return; }
    if (!isOpen(s)) { s.why = `"${t.name}" 실행 차단 — ${ch.alias} 세션이 연결되지 않음 (자동 연결하지 않음)`; render(); return; }
    if (s.proto !== t.proto || +s.port !== +t.port) { s.why = `"${t.name}" 실행 차단 — 템플릿 대상 ${t.proto.toUpperCase()} ${t.port} ≠ 현재 세션 ${s.proto.toUpperCase()} ${s.port}. ✎로 불러와 다른 대상에 보낼 수 있음`; render(); return; }
    send(ch, r.bytes, '템플릿: ' + t.name);
  }

  /* ---- render ---- */
  function setErr(m) { const e = $('#skErr'); if (e) e.textContent = m; const ta = $('#skData'); if (ta) ta.classList.toggle('invalid', !!m); }
  const STATE_L = { closed: ['닫힘', ''], pend: ['연결 중…', 'pend'], est: ['연결됨', 'est'], ref: ['거부됨 (RST)', 'ref'], udp: ['열림 (UDP)', 'est'] };
  function fmtPayload(b, fmt) {
    if (!b) return '';
    if (fmt === 'text') return esc(String.fromCharCode(...b.slice(0, 200)).replace(/\r/g, '\\r').replace(/\n/g, '\\n').replace(/[^\x20-\x7e\\]/g, '·')) + (b.length > 200 ? '…' : '');
    return Array.from(b.slice(0, 96)).map(P.h2).join(' ') + (b.length > 96 ? ' …' : '');
  }
  function preview() {
    const r = parsePayload(), el = $('#skPrev'); if (!el) return;
    el.textContent = !draft.data ? '' : r.err ? '' : `${r.bytes.length} B · ${Array.from(r.bytes.slice(0, 24)).map(P.h2).join(' ')}${r.bytes.length > 24 ? ' …' : ''}`;
    setErr(draft.data && r.err ? r.err : '');
  }
  function render(force) {
    const el = $('#sock'); if (!el) return;
    el.hidden = !open;
    const tb = $('#tbSocket'); if (tb) tb.setAttribute('aria-pressed', open);
    if (!open) { built = null; return; }
    const ch = target();
    if (!ch) { el.innerHTML = `<div class="sk-h"><h2>소켓 테스트 (SIM)</h2><button type="button" class="ibtn" data-cmd="toggle-socket" title="닫기"><svg class="i"><use href="#i-x"/></svg></button></div><div class="sk-sec"><div class="sk-note">열린 채널이 없습니다. 채널을 추가하세요.</div></div>`; built = null; return; }
    const s = getSess(ch);
    const sig = [ch.id, s.state, s.proto, s.host, s.port, !!s.repeat, s.why, editing, draft.fmt, S.chans.map(c => c.id + c.alias + c.live).join(','), JSON.stringify(tpls)].join('|');
    if (force || built !== sig) { build(ch, s); built = sig; logVer = -1; }
    if (logVer !== s.ver) { renderLog(s); logVer = s.ver; }
  }
  function build(ch, s) {
    const el = $('#sock'), pr = L.PROFILES[ch.profile], st = STATE_L[s.state];
    const locked = s.state !== 'closed' && s.state !== 'ref';
    const pe = peerOf(ch, s);
    const tplRows = tpls.map(t => `<div class="tpl${editing === t.id ? ' editing' : ''}" data-tpl="${t.id}">
        <span class="tn-name" title="${esc(t.name)}">${esc(t.name)}</span>
        <span class="tn-sub">${t.proto.toUpperCase()} ${t.port} · ${t.fmt.toUpperCase()} · ${esc(t.data.slice(0, 40))}</span>
        <span class="acts"><button type="button" class="ibtn" data-act="tpl-run" title="현재 세션으로 실행 (연결 필요, 자동 연결 없음)" style="color:var(--ok)"><svg class="i"><use href="#i-play"/></svg></button><button type="button" class="ibtn" data-act="tpl-edit" title="편집 — 보내기 칸으로 불러오기"><svg class="i"><use href="#i-edit"/></svg></button><button type="button" class="ibtn danger" data-act="tpl-del" title="삭제"><svg class="i"><use href="#i-trash"/></svg></button></span></div>`).join('');
    const ed = editing && tpls.find(t => t.id === editing);
    el.innerHTML = `
      <div class="sk-h"><h2>소켓 테스트 <span class="st sim">SIM</span></h2><button type="button" class="ibtn" data-cmd="toggle-socket" title="패널 닫기 (Alt+S)"><svg class="i"><use href="#i-x"/></svg></button></div>
      <div class="sk-body">
        <div class="sk-sec">
          <h3>대상 채널 · 엔드포인트</h3>
          <div class="sk-row"><label for="skCh">채널</label><select class="fld" id="skCh" data-act="sk-ch" style="flex:1">${S.chans.map(c => { const cs = getSess(c); return `<option value="${c.id}" ${c.id === ch.id ? 'selected' : ''}>${esc(c.alias)} (${c.iface}) — ${STATE_L[cs.state][0]}</option>`; }).join('')}</select>
            <span class="sk-state ${st[1]}" data-role="sk-state">${st[0]}</span></div>
          <div class="sk-row"><label>프로토콜</label><span class="seg" role="radiogroup">${['tcp', 'udp'].map(p => `<button type="button" role="radio" data-act="sk-proto" data-v="${p}" aria-checked="${s.proto === p}" ${locked ? 'disabled' : ''}>${p.toUpperCase()}${p === 'tcp' ? ' client' : ''}</button>`).join('')}</span></div>
          <div class="sk-row"><label for="skHost">피어</label><select class="fld" id="skHost" data-act="sk-host" ${locked ? 'disabled' : ''} style="flex:1">${pr.peers.map(p => `<option value="${p.host}" ${p.host === s.host ? 'selected' : ''}>${esc(p.name)} ${L.hostOf(ch, p.host).ip} (sim: TCP ${(p.tcp || []).join(',') || '–'} / UDP ${(p.udp || []).join(',') || '–'})</option>`).join('')}</select></div>
          <div class="sk-row"><label for="skPort">포트</label><input class="fld" id="skPort" data-act="sk-port" inputmode="numeric" value="${esc(s.port)}" ${locked ? 'disabled' : ''} style="width:84px">
            <span class="sk-note" style="flex:1">로컬 ${ch.me.ip}:${s.state === 'closed' ? '자동' : s.lport}</span>
            ${s.state === 'closed' || s.state === 'ref'
              ? `<button type="button" class="btn primary" data-act="sk-connect">${s.proto === 'tcp' ? '연결' : '열기'}</button>`
              : `<button type="button" class="btn danger" data-act="sk-disconnect">${s.proto === 'tcp' ? '연결 끊기' : '닫기'}</button>`}</div>
          <div class="sk-why" data-role="sk-why">${esc(s.why)}</div>
          <div class="sk-note">로컬 시뮬레이터 전용 — OS 소켓·실제 네트워크 송신 없음. TX/RX는 <b>${esc(ch.alias)}</b> 캡처에 실제 프레임으로 기록 (캡처 LIVE일 때). 다른 채널 세션과 독립.</div>
        </div>
        <div class="sk-sec">
          <h3>보내기 ${ed ? `<span class="chip" style="background:var(--warn-bg);color:var(--warn)">편집 중: ${esc(ed.name)}</span>` : ''}</h3>
          <div class="sk-row"><span class="seg" role="radiogroup" aria-label="데이터 형식">${[['hex', 'HEX'], ['text', 'Text']].map(([v, l]) => `<button type="button" role="radio" data-act="sk-fmt" data-v="${v}" aria-checked="${draft.fmt === v}">${l}</button>`).join('')}</span>
            <span class="sk-note" style="flex:1">${draft.fmt === 'hex' ? '공백/쉼표/콜론/0x 허용' : '이스케이프 \\r \\n \\t \\\\ \\xHH'}</span></div>
          <textarea class="fld" id="skData" data-act="sk-data" spellcheck="false" placeholder="${draft.fmt === 'hex' ? '02 FD 80 01 …' : 'STAT?\\r\\n'}" aria-describedby="skErr">${esc(draft.data)}</textarea>
          <div class="sk-err" id="skErr" role="alert"></div>
          <div class="sk-note" id="skPrev" style="font-family:var(--mono)"></div>
          <div class="sk-row">
            <button type="button" class="btn primary" data-act="sk-send" ${isOpen(s) ? '' : 'disabled'} title="${isOpen(s) ? 'SIM 전송' : '연결되지 않음 — 보내기 차단'}"><svg class="i"><use href="#i-send"/></svg>보내기</button>
            <span class="sk-note">반복</span><input class="fld" id="skRepN" data-act="sk-repn" value="${esc(draft.repN)}" style="width:46px" inputmode="numeric" aria-label="반복 횟수"><span class="sk-note">회 ×</span><input class="fld" id="skRepMs" data-act="sk-repms" value="${esc(draft.repMs)}" style="width:58px" inputmode="numeric" aria-label="간격 ms"><span class="sk-note">ms</span>
            ${s.repeat ? `<button type="button" class="btn danger" data-act="sk-rep-stop"><svg class="i"><use href="#i-stop"/></svg>반복 중지</button>` : `<button type="button" class="btn" data-act="sk-rep" ${isOpen(s) ? '' : 'disabled'}>반복 시작</button>`}
          </div>
          <div class="sk-row"><input class="fld" id="skTplName" placeholder="템플릿 이름" value="${esc(ed ? ed.name : '')}" style="flex:1" maxlength="48">
            ${ed ? `<button type="button" class="btn" data-act="tpl-update">템플릿 업데이트</button><button type="button" class="btn ghost" data-act="tpl-cancel">편집 취소</button>` : ''}
            <button type="button" class="btn" data-act="tpl-save">${ed ? '새로 저장' : '템플릿으로 저장'}</button></div>
        </div>
        <div class="sk-sec" style="border-bottom:0;padding-bottom:4px"><h3>송수신 기록 — ${esc(ch.alias)} <span class="grow" style="flex:1"></span><button type="button" class="btn ghost" data-act="sk-log-clear" style="height:20px;font-size:11px">기록 지우기</button></h3></div>
        <div class="tx-log" id="skLog" aria-live="polite"></div>
        <div class="sk-sec"><h3>저장된 템플릿 <span class="chip">${tpls.length}</span></h3><div class="tpls">${tplRows || '<div class="sk-note">템플릿 없음</div>'}</div></div>
      </div>`;
    preview();
  }
  function renderLog(s) {
    const el = $('#skLog'); if (!el) return;
    const fmt = draft.fmt;
    el.innerHTML = s.log.length ? s.log.map(e => `<div class="e ${e.dir}"><span class="ts">${(e.t / 1000).toFixed(4)}</span><span class="d">${e.dir === 'tx' ? 'TX' : e.dir === 'rx' ? 'RX' : '·'}</span><span class="pl">${e.bytes ? fmtPayload(e.bytes, fmt) + ` <small>${e.bytes.length} B</small>` : esc(e.note)}${e.bytes && e.note ? ` <small>${esc(e.note)}</small>` : ''}${e.no ? ` <small><a href="#" data-frame="${e.no}" style="color:#8ab4ff">#${e.no}</a></small>` : (e.dir !== 'sys' ? ' <small>(캡처 정지/필터 — 목록 미기록)</small>' : '')}</span></div>`).join('')
      : '<div class="empty">아직 송수신 없음. 연결 후 보내면 TX와 결정적 sim 응답(RX)이 여기와 패킷 목록에 함께 기록됩니다.</div>';
    el.scrollTop = el.scrollHeight;
  }

  /* ---- events ---- */
  document.addEventListener('click', e => {
    const fr = e.target.closest('#skLog [data-frame]');
    if (fr) {
      e.preventDefault(); const no = +fr.dataset.frame;
      const p = S.view.find(x => x.no === no);
      if (p) L.selectPacket(p, { scroll: true }); else L.toast(`#${no}은 현재 표시 범위/필터/일시정지로 목록에 없음`, 'warn');
      return;
    }
    const a = e.target.closest('#sock [data-act]'); if (!a || a.disabled) return;
    const ch = target(); if (!ch) return; const s = getSess(ch), act = a.dataset.act;
    if (act === 'sk-proto') { s.proto = a.dataset.v; const pd = L.PROFILES[ch.profile].peers.find(p => (p[s.proto] || []).length); if (pd && !(peerDef(ch, s) || {})[s.proto]?.includes(+s.port)) { s.host = pd.host; s.port = pd[s.proto][0]; } s.state = 'closed'; render(true); }
    else if (act === 'sk-connect') connect(ch);
    else if (act === 'sk-disconnect') disconnect(ch);
    else if (act === 'sk-fmt') { draft.fmt = a.dataset.v; L.store.set('draft', draft); render(true); }
    else if (act === 'sk-send') send(ch);
    else if (act === 'sk-rep') {
      const n = +$('#skRepN').value, ms = +$('#skRepMs').value;
      if (!Number.isInteger(n) || n < 1 || n > 1000 || !Number.isInteger(ms) || ms < 50 || ms > 60000) { setErr('반복: 1–1000회, 간격 50–60000 ms 정수'); return; }
      startRepeat(ch, n, ms); render(true);
    }
    else if (act === 'sk-rep-stop') { stopRepeat(s, '사용자 중지'); render(true); }
    else if (act === 'sk-log-clear') { s.log = []; s.ver++; render(); }
    else if (act === 'tpl-save' || act === 'tpl-update') {
      const name = $('#skTplName').value.trim();
      const r = parsePayload();
      if (!name) { setErr('템플릿 이름을 입력하세요'); $('#skTplName').focus(); return; }
      if (r.err) { setErr('템플릿 저장 불가: ' + r.err); return; }
      const rec = { name, proto: s.proto, port: +s.port, fmt: draft.fmt, data: draft.data };
      if (act === 'tpl-update' && editing) { const t = tpls.find(x => x.id === editing); Object.assign(t, rec); L.toast(`템플릿 "${name}" 업데이트`); }
      else { tpls.push({ id: 't' + Date.now().toString(36), ...rec }); L.toast(`템플릿 "${name}" 저장 (${rec.proto.toUpperCase()} ${rec.port})`); }
      editing = null; saveTpls(); render(true);
    }
    else if (act === 'tpl-cancel') { editing = null; render(true); }
    else {
      const row = a.closest('[data-tpl]'); const t = row && tpls.find(x => x.id === row.dataset.tpl); if (!t) return;
      if (act === 'tpl-run') runTpl(t);
      else if (act === 'tpl-edit') {
        editing = t.id; draft.fmt = t.fmt; draft.data = t.data; L.store.set('draft', draft);
        if (s.state === 'closed' || s.state === 'ref') { s.proto = t.proto; s.port = t.port; const pd = L.PROFILES[ch.profile].peers.find(p => (p[t.proto] || []).includes(+t.port)); if (pd) s.host = pd.host; }
        render(true);
      }
      else if (act === 'tpl-del') L.confirmDlg('템플릿 삭제', `"${esc(t.name)}" 템플릿을 삭제합니다.`, '삭제').then(ok => { if (!ok) return; tpls = tpls.filter(x => x !== t); if (editing === t.id) editing = null; saveTpls(); render(true); });
    }
  });
  document.addEventListener('change', e => {
    const a = e.target.closest('#sock [data-act]'); if (!a) return;
    const ch = target(); if (!ch) return; const s = getSess(ch);
    if (a.dataset.act === 'sk-ch') { draft.target = a.value; L.store.set('draft', draft); editing = null; render(true); }
    else if (a.dataset.act === 'sk-host') { s.host = a.value; render(true); }
  });
  document.addEventListener('input', e => {
    const a = e.target.closest('#sock [data-act]'); if (!a) return;
    const ch = target(); if (!ch) return; const s = getSess(ch);
    if (a.dataset.act === 'sk-data') { draft.data = a.value; L.store.set('draft', draft); preview(); }
    else if (a.dataset.act === 'sk-repn' || a.dataset.act === 'sk-repms') { draft[a.dataset.act === 'sk-repn' ? 'repN' : 'repMs'] = a.value; L.store.set('draft', draft); }
    else if (a.dataset.act === 'sk-port') { s.port = a.value.trim(); const bad = !!validateEndpoint(s); a.classList.toggle('invalid', bad); s.why = bad ? '포트는 1–65535 정수' : ''; const w = document.querySelector('[data-role=sk-why]'); if (w) w.textContent = s.why; }
  });
  document.addEventListener('keydown', e => {
    if (e.target && e.target.id === 'skData' && e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); const ch = target(); if (ch) send(ch); }
  });

  window.LoomEthSock = {
    render,
    cmd(c, el) {
      if (c === 'toggle-socket') { open = !open; S.layout.sock = open; L.saveLayout(); render(true); if (open) { const f = $('#skCh'); if (f) f.focus(); } return; }
      if (window.LoomEthTools && window.LoomEthTools.cmd) window.LoomEthTools.cmd(c, el);
    },
    onActive(ch) { if (open && draft.target !== ch.id && !sess.has(draft.target)) { draft.target = ch.id; render(true); } },
    onChannelClosed(ch) { const s = sess.get(ch.id); if (s) { s.repeat = null; s.state = 'closed'; } sess.delete(ch.id); if (draft.target === ch.id) draft.target = null; editing = null; render(true); },
    _sess: sess, _connect: connect, _disconnect: disconnect, _send: send, _runTpl: runTpl, _tpls: () => tpls, _draft: draft, _target: target
  };
  render(true);
})();
