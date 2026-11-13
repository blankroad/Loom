/* Loom Ethernet mockup (Opus) — app core: channels, deterministic SIM traffic, capture/retention,
   packet list (virtual), details tree, bytes pane, display/capture filters, pause view, dividers.
   Socket test lives in sock.js (loaded after this file) and talks to window.LoomEth. */
(function () {
  'use strict';
  const P = window.LoomPkt;
  const $ = s => document.querySelector(s);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const LS = 'loom.eth.opus.v1.';
  const RETAIN = window.__loomEthRetain || 2000; // per channel
  const MAX_CH = 4, TICK = 50, ROW = 20;
  const store = {
    get(k, d) { try { const v = localStorage.getItem(LS + k); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
    set(k, v) { try { localStorage.setItem(LS + k, JSON.stringify(v)); } catch (e) { /* storage blocked: keep in memory */ } }
  };

  /* ---------------- SIM profiles (deterministic) ---------------- */
  const PROFILES = {
    doip: {
      label: 'DoIP 게이트웨이 (UDP/TCP 13400)', net: '192.168.10',
      hosts: { me: 10, ecu: 20, tester2: 31 },
      peers: [{ host: 'ecu', name: 'ECU-GW', tcp: [13400], udp: [13400], svc: 'doip', la: 0x1001 }],
      def: { proto: 'tcp', host: 'ecu', port: 13400, fmt: 'hex' }
    },
    someip: {
      label: 'SOME/IP 바디 도메인 (UDP 30490/30501) + 상태 TCP', net: '10.0.3',
      hosts: { me: 10, ecu: 20, bench: 5 },
      peers: [{ host: 'ecu', name: 'BODY-ECU', udp: [30501], tcp: [], svc: 'someip' }, { host: 'bench', name: 'bench-host', tcp: [7, 7000], udp: [7], svc: 'echo' }],
      def: { proto: 'udp', host: 'ecu', port: 30501, fmt: 'hex' }
    },
    generic: {
      label: '일반 TCP/UDP (echo 7, syslog 514)', net: '172.16.0',
      hosts: { me: 10, peer: 20 },
      peers: [{ host: 'peer', name: 'echo-peer', tcp: [7], udp: [7], svc: 'echo' }],
      def: { proto: 'tcp', host: 'peer', port: 7, fmt: 'text' }
    }
  };
  function hostOf(ch, k) {
    const pr = PROFILES[ch.profile], o = pr.hosts[k];
    return { ip: `${pr.net}.${o}`, mac: `02:00:${P.h2(ch.slot + 0x0a)}:${P.h2(o)}:00:${P.h2(o)}` };
  }
  const BCAST = { mac: 'ff:ff:ff:ff:ff:ff', ip: '255.255.255.255' };
  const SD_MC = { mac: '01:00:5e:60:e0:f5', ip: '224.224.224.245' };

  /* every profile returns [{every, at, fn(n, emit)}]; emit(bytes, dtMs, isnRegs) */
  function scriptFor(ch) {
    const me = hostOf(ch, 'me');
    if (ch.profile === 'doip') {
      const ecu = hostOf(ch, 'ecu'), t2 = hostOf(ch, 'tester2');
      const vin = P.txt('WLOOMSIM00000' + String(1000 + ch.slot).slice(-4));
      const eid = P.macB(ecu.mac);
      const ann = () => P.doip(0x0004, [...vin, 0x10, 0x01, ...eid, ...eid, 0x00, 0x00]);
      const st = { seq: 0x1000 + ch.slot * 7919, ack: 0x5000 + ch.slot * 104729, sp: 49200 + ch.slot, up: false };
      return [
        { at: 120, every: 100, max: 3, fn: (n, e) => e(P.udpFrame(ecu, BCAST, 13400, 13400, ann(), ch.ipid())) },
        { at: 600, every: 0, fn: (n, e) => { // tester2 TCP handshake + routing activation
          const k = `${t2.ip}:${st.sp}>${ecu.ip}:13400`, kr = `${ecu.ip}:13400>${t2.ip}:${st.sp}`;
          e(P.arpFrame(1, t2, ecu)); e(P.arpFrame(2, ecu, t2), 0.4);
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, 0, P.F.SYN, [], ch.ipid()), 1, { [k]: st.seq });
          e(P.tcpFrame(ecu, t2, 13400, st.sp, st.ack, st.seq + 1, P.F.SYN | P.F.ACK, [], ch.ipid()), 1.5, { [kr]: st.ack });
          st.seq++; st.ack++;
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 1.8);
          const ra = P.doip(0x0005, [0x0e, 0x80, 0x00, 0, 0, 0, 0]);
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.PSH | P.F.ACK, ra, ch.ipid()), 3); st.seq += ra.length;
          const rr = P.doip(0x0006, [0x0e, 0x80, 0x10, 0x01, 0x10, 0, 0, 0, 0]);
          e(P.tcpFrame(ecu, t2, 13400, st.sp, st.ack, st.seq, P.F.PSH | P.F.ACK, rr, ch.ipid()), 6); st.ack += rr.length;
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 6.4);
          st.up = true;
        } },
        { at: 2000, every: 2000, fn: (n, e) => { if (!st.up) return; // TesterPresent cycle (suppress positive response)
          const dm = P.doip(0x8001, [0x0e, 0x80, 0x10, 0x01, 0x3e, 0x80]);
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.PSH | P.F.ACK, dm, ch.ipid())); st.seq += dm.length;
          const ak = P.doip(0x8002, [0x10, 0x01, 0x0e, 0x80, 0x00, 0x3e, 0x80]);
          e(P.tcpFrame(ecu, t2, 13400, st.sp, st.ack, st.seq, P.F.PSH | P.F.ACK, ak, ch.ipid()), 2.1); st.ack += ak.length;
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 2.5);
        } },
        { at: 3300, every: 5000, fn: (n, e) => { if (!st.up) return; // periodic ReadDID F190 (VIN)
          const rq = P.doip(0x8001, [0x0e, 0x80, 0x10, 0x01, 0x22, 0xf1, 0x90]);
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.PSH | P.F.ACK, rq, ch.ipid())); st.seq += rq.length;
          const ak = P.doip(0x8002, [0x10, 0x01, 0x0e, 0x80, 0x00]);
          const rs = P.doip(0x8001, [0x10, 0x01, 0x0e, 0x80, 0x62, 0xf1, 0x90, ...vin]);
          e(P.tcpFrame(ecu, t2, 13400, st.sp, st.ack, st.seq, P.F.PSH | P.F.ACK, [...ak, ...rs], ch.ipid()), 3.2); st.ack += ak.length + rs.length;
          e(P.tcpFrame(t2, ecu, st.sp, 13400, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 3.6);
        } },
        { at: 9000, every: 15000, fn: (n, e) => { e(P.arpFrame(1, ecu, me)); e(P.arpFrame(2, me, ecu), 0.3); } }
      ];
    }
    if (ch.profile === 'someip') {
      const ecu = hostOf(ch, 'ecu'), bench = hostOf(ch, 'bench');
      const st = { sess: 0, ev: 0, seq: 0x2000 + ch.slot * 31, ack: 0x9000 + ch.slot * 17, up: false, line: 0 };
      return [
        { at: 80, every: 1000, fn: (n, e) => e(P.udpFrame(ecu, SD_MC, 30490, 30490, P.someipSdOffer(++st.sess & 0xffff, 0x1234, 0x0001, 1, 3, 0, ecu.ip, 30501), ch.ipid())) },
        { at: 260, every: 500, fn: (n, e) => { // event notification: vehicle speed (u16 0.01 km/h) + door bitmap
          st.ev++; const spd = (3000 + ((st.ev * 137) % 2500)) & 0xffff;
          e(P.udpFrame(ecu, me, 30501, 30501, P.someip(0x1234, 0x8001, 0x0000, st.ev & 0xffff, 1, 0x02, 0x00, [...P.p16(spd), st.ev % 4 === 0 ? 0x01 : 0x00, 0x00]), ch.ipid()));
        } },
        { at: 700, every: 0, fn: (n, e) => {
          const sp = 50001, k = `${me.ip}:${sp}>${bench.ip}:7000`, kr = `${bench.ip}:7000>${me.ip}:${sp}`;
          e(P.tcpFrame(me, bench, sp, 7000, st.seq, 0, P.F.SYN, [], ch.ipid()), 0, { [k]: st.seq });
          e(P.tcpFrame(bench, me, 7000, sp, st.ack, st.seq + 1, P.F.SYN | P.F.ACK, [], ch.ipid()), 0.6, { [kr]: st.ack });
          st.seq++; st.ack++;
          e(P.tcpFrame(me, bench, sp, 7000, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 0.9); st.up = true;
        } },
        { at: 1500, every: 2000, fn: (n, e) => { if (!st.up) return;
          const sp = 50001, q = P.txt('STAT?\r\n');
          e(P.tcpFrame(me, bench, sp, 7000, st.seq, st.ack, P.F.PSH | P.F.ACK, q, ch.ipid())); st.seq += q.length;
          st.line++; const r = P.txt(`STAT seq=${st.line} vbat=${(12.1 + (st.line % 7) / 10).toFixed(1)}V temp=${30 + (st.line * 3) % 12}C\r\n`);
          e(P.tcpFrame(bench, me, 7000, sp, st.ack, st.seq, P.F.PSH | P.F.ACK, r, ch.ipid()), 1.4); st.ack += r.length;
          e(P.tcpFrame(me, bench, sp, 7000, st.seq, st.ack, P.F.ACK, [], ch.ipid()), 1.8);
        } }
      ];
    }
    const peer = hostOf(ch, 'peer');
    return [
      { at: 200, every: 0, fn: (n, e) => { e(P.arpFrame(1, me, peer)); e(P.arpFrame(2, peer, me), 0.5); } },
      { at: 500, every: 1500, fn: (n, e) => e(P.udpFrame(peer, me, 514, 514, P.txt(`<134>sim-app[${200 + ch.slot}]: heartbeat n=${n + 1} load=${(n * 7) % 100}%`), ch.ipid())) }
    ];
  }

  /* ---------------- state ---------------- */
  const S = {
    now: 0, nextNo: 1, chans: [], activeCh: null, scope: 'all',
    view: [], sel: null, selNode: null, paused: false, pausedAt: 0, autoscroll: true, colorize: true,
    df: { text: '', applied: '', fn: () => true, ok: true },
    streams: { tcp: new Map(), udp: new Map() }, isn: {},
    layout: Object.assign({ listFrac: 0.52, dtlFrac: 0.55, dtlHFrac: 0.55, sock: false }, store.get('layout', {})),
    recent: store.get('recent', []), dirty: true, epoch: Date.now(), sched: []
  };
  let slotSeq = 0;
  function streamIdx(proto, key) { const m = S.streams[proto]; if (!m.has(key)) m.set(key, m.size); return m.get(key); }

  function mkChannel(cfg) {
    const used = new Set(S.chans.map(c => c.slot));
    let slot = 0; while (used.has(slot)) slot++;
    const ch = {
      id: cfg.id || 'ch' + (++slotSeq) + '-' + Date.now().toString(36).slice(-4), alias: cfg.alias, iface: cfg.iface || 'sim' + slot,
      profile: cfg.profile || 'generic', slot, capFilter: cfg.capFilter || '', cf: P.parseCF(cfg.capFilter || '').fn,
      live: false, startedAt: null, pkts: [], captured: 0, dropped: 0, cfExcluded: 0, ipidN: 1 + slot * 1000, gen: null
    };
    ch.ipid = () => (ch.ipidN = (ch.ipidN + 1) & 0xffff);
    ch.me = hostOf(ch, 'me');
    S.chans.push(ch);
    return ch;
  }
  function startCh(ch) {
    if (ch.live) return;
    ch.live = true; ch.startedAt = S.now;
    if (!ch.gen) ch.gen = scriptFor(ch).map(ev => ({ ...ev, n: 0, next: S.now + ev.at }));
    else ch.gen.forEach(ev => { if (ev.next < S.now) { const k = ev.every ? Math.ceil((S.now - ev.next) / ev.every) : 0; ev.next += k * ev.every; if (!ev.every && ev.n) ev.next = Infinity; } });
    persist(); S.dirty = true;
  }
  function stopCh(ch) { if (!ch.live) return; ch.live = false; persist(); S.dirty = true; }

  /* capture one frame on ch at sim time t. Returns packet or null (stopped / capture-filtered) */
  function capture(ch, bytes, t, isnRegs, own) {
    if (isnRegs) Object.assign(S.isn, isnRegs);
    if (!ch.live) return null;
    const ctx = { no: S.nextNo, t, iface: ch.iface, chAlias: ch.alias, tcpIsn: S.isn, stream: streamIdx };
    const d = P.decode(bytes, ctx);
    if (!ch.cf(d.f)) { ch.cfExcluded++; return null; }
    const pk = { no: S.nextNo++, t, ch, bytes, sum: d.sum, f: d.f, own: !!own };
    ch.pkts.push(pk); ch.captured++;
    if (ch.pkts.length > RETAIN) { const gone = ch.pkts.splice(0, ch.pkts.length - RETAIN); ch.dropped += gone.length; gone.forEach(g => { g.gone = true; }); }
    if (!S.paused && inScope(pk) && S.df.fn(pk.f)) { S.view.push(pk); }
    S.dirty = true;
    return pk;
  }
  function inScope(pk) { return S.scope === 'all' || pk.ch.id === S.scope; }

  function tick(ms) {
    const target = S.now + (ms || TICK);
    // run generators & scheduled callbacks in time order
    for (;;) {
      let best = null, bt = Infinity;
      for (const ch of S.chans) if (ch.live && ch.gen) for (const ev of ch.gen) if (ev.next < bt && ev.next <= target) { bt = ev.next; best = { ch, ev }; }
      const sj = S.sched.length ? S.sched[0] : null;
      if (sj && sj.at <= target && sj.at <= bt) { S.sched.shift(); S.now = Math.max(S.now, sj.at); sj.fn(); continue; }
      if (!best) break;
      S.now = Math.max(S.now, bt);
      const { ch, ev } = best;
      const base = ev.next, emitted = [];
      ev.fn(ev.n, (bytes, dt, reg) => emitted.push([bytes, base + (dt || 0) + ((S.nextNo * 37) % 100) / 1000, reg]));
      emitted.sort((a, b) => a[1] - b[1]).forEach(([bytes, t, reg]) => capture(ch, bytes, t, reg));
      ev.n++;
      ev.next = ev.every && (!ev.max || ev.n < ev.max) ? ev.next + ev.every : Infinity;
    }
    S.now = target;
    // compact view of evicted packets (not while paused: frozen view)
    if (!S.paused && S.view.length && S.view[0].gone) { S.view = S.view.filter(p => !p.gone); if (S.sel && S.sel.gone) { S.sel = null; S.dirtyDetail = true; } }
  }
  function schedule(dtMs, fn) { const at = S.now + dtMs; let i = S.sched.length; while (i > 0 && S.sched[i - 1].at > at) i--; S.sched.splice(i, 0, { at, fn }); }

  /* ---------------- view / filter ---------------- */
  function allPkts() {
    const lists = S.chans.filter(c => S.scope === 'all' || c.id === S.scope).map(c => c.pkts);
    const out = []; lists.forEach(l => out.push(...l)); out.sort((a, b) => a.no - b.no); return out;
  }
  function rebuildView() {
    S.view = allPkts().filter(p => S.df.fn(p.f));
    S.dirty = true;
    // O2: never keep showing details/bytes of a packet that is no longer in the list
    if (S.sel && (S.sel.gone || !S.view.includes(S.sel))) {
      const prev = S.sel.no;
      if (S.view.length) {
        S.userScrolled = S.view.length > 1;
        selectPacket(S.view[0], { scroll: true });
        toast(`선택 #${prev}은 현재 필터/범위 밖 → 첫 일치 패킷 #${S.view[0].no} 선택`);
      } else {
        S.sel = null; S.selNode = null; curTree = null; renderTree(); renderHex();
        toast(`선택 #${prev}은 현재 필터/범위 밖 — 일치 패킷 없음, 상세/바이트 비움`);
      }
    }
  }
  function capturedInScope() { return S.chans.filter(c => S.scope === 'all' || c.id === S.scope).reduce((a, c) => a + c.pkts.length, 0); }

  function applyDF(text) {
    const r = P.parseDF(text);
    const inp = $('#dfInput');
    if (!r.ok) {
      inp.classList.remove('valid'); inp.classList.add('invalid'); inp.setAttribute('aria-invalid', 'true');
      setDfMsg(`<b>필터 오류</b> — ${esc(r.err)} · 이전 결과 유지 (적용됨: <code>${esc(S.df.applied || '없음')}</code>)`, true);
      toast('표시 필터 오류 — 적용하지 않음, 이전 결과 유지', 'err');
      return false;
    }
    S.df = { text, applied: text.trim(), fn: r.fn, ok: true };
    inp.removeAttribute('aria-invalid');
    if (S.df.applied) { S.recent = [S.df.applied, ...S.recent.filter(x => x !== S.df.applied)].slice(0, 8); store.set('recent', S.recent); }
    store.set('df', S.df.applied);
    setDfMsg('', false);
    rebuildView(); render(true); validateLive();
    return true;
  }
  function setDfMsg(html, isErr) { const m = $('#dfMsg'); m.innerHTML = html; m.classList.toggle('err', !!isErr); }
  function validateLive() {
    const inp = $('#dfInput'), v = inp.value; const r = P.parseDF(v);
    inp.classList.toggle('valid', !!v.trim() && r.ok); inp.classList.toggle('invalid', !!v.trim() && !r.ok);
    inp.classList.toggle('applied-mismatch', v.trim() !== S.df.applied);
    if (v.trim() && !r.ok) setDfMsg(`${esc(r.err)} <span style="color:var(--muted)">· Enter 해도 적용되지 않음, 현재 결과 유지</span>`, true);
    else if (v.trim() !== S.df.applied) setDfMsg(v.trim() ? '유효한 식 — Enter 또는 적용을 눌러 반영' : '빈 필터 — 적용하면 모든 패킷 표시', false);
    else setDfMsg('', false);
  }
  const SUGG = [
    ['doip', 'DoIP 전체'], ['doip.type == 0x8001', 'DoIP 진단 메시지'], ['someip', 'SOME/IP (SD 포함)'], ['someipsd', 'SOME/IP-SD만'],
    ['arp', 'ARP'], ['tcp.port == 7000', '상태 TCP'], ['tcp.flags.syn == 1', 'SYN 포함'], ['udp && !someip', 'SOME/IP 아닌 UDP'],
    ['ip.addr == 192.168.10.0/24', '서브넷'], ['frame.len > 100', '긴 프레임'], ['loom.ch == "ecu-doip"', '채널 별칭 (Loom 확장)'], ['data', '디코드 안 된 페이로드']
  ];
  function showSug() {
    const v = $('#dfInput').value.trim().toLowerCase(), ul = $('#dfSug');
    const items = [];
    if (S.recent.length) { items.push(['__h', '최근 적용']); S.recent.filter(x => !v || x.toLowerCase().includes(v)).slice(0, 5).forEach(x => items.push([x, ''])); }
    const ex = SUGG.filter(([x]) => !v || x.includes(v));
    if (ex.length) { items.push(['__h', '예시']); ex.forEach(x => items.push(x)); }
    const last = v.split(/[\s()!&|]+/).pop();
    if (last && last.length >= 2) {
      const fl = Object.keys(P.FIELDS).filter(k => k.startsWith(last) && k !== last).slice(0, 8);
      if (fl.length) { items.push(['__h', '필드']); fl.forEach(k => items.push([v.slice(0, v.length - last.length) + k, P.FIELDS[k] === 'p' ? 'protocol' : P.FIELDS[k] === 'n' ? 'number' : P.FIELDS[k] === 'i' ? 'IPv4' : P.FIELDS[k] === 'm' ? 'MAC' : 'string'])); }
    }
    if (!items.length) { hideSug(); return; }
    ul.innerHTML = items.map(([x, d], i) => x === '__h' ? `<li class="hd" role="presentation">${esc(d)}</li>` : `<li role="option" data-v="${esc(x)}" id="sg${i}"><span>${esc(x)}</span><small>${esc(d)}</small></li>`).join('');
    ul.hidden = false; $('#dfInput').setAttribute('aria-expanded', 'true');
  }
  function hideSug() { $('#dfSug').hidden = true; $('#dfInput').setAttribute('aria-expanded', 'false'); }

  /* ---------------- rendering: channel strip / scope ---------------- */
  const fmtN = n => n.toLocaleString('en-US');
  function chColor(ch) { return `var(--c${ch.slot % 4})`; }
  function renderChannels() {
    const bar = $('#chbar');
    const cards = S.chans.map(ch => {
      const pr = PROFILES[ch.profile];
      return `<div class="chcard${S.activeCh === ch.id ? ' active' : ''}" data-ch="${ch.id}" style="--tag:${chColor(ch)}">
        <button type="button" class="ch-sel" data-act="ch-select" title="이 채널을 활성(소켓 테스트 기본 대상)으로" aria-pressed="${S.activeCh === ch.id}"><span class="chdot"></span></button>
        <div class="ch-name"><span class="alias">${esc(ch.alias)}</span><span class="st sim">SIM</span><span class="st ${ch.live ? 'live' : 'stopped'}" data-role="cap-state">${ch.live ? 'LIVE' : 'STOPPED'}</span></div>
        <div class="ch-meta" title="${esc(pr.label)}">${esc(ch.iface)} · ${ch.me.ip}/24 · <span class="link-up">link up</span> · <span class="cnt"><b data-role="cap-count">${fmtN(ch.captured)}</b> 캡처</span>${ch.dropped ? ` · <span class="drop">${fmtN(ch.dropped)} 삭제</span>` : ''}${ch.cfExcluded ? ` · ${fmtN(ch.cfExcluded)} 필터 제외` : ''}</div>
        <div class="ch-acts">
          ${ch.live ? `<button type="button" class="btn" data-act="ch-stop" title="이 채널만 캡처 정지 — 기록 유지, 다른 채널 계속"><svg class="i"><use href="#i-stop"/></svg>정지</button>`
                    : `<button type="button" class="btn" data-act="ch-start" title="이 채널 캡처 시작"><svg class="i"><use href="#i-play"/></svg>시작</button>`}
          <button type="button" class="ibtn" data-act="ch-menu" aria-haspopup="menu" title="채널 메뉴"><svg class="i"><use href="#i-more"/></svg></button>
        </div></div>`;
    }).join('');
    bar.innerHTML = (cards || '<span class="ch-empty">열린 채널 없음 — 채널 추가로 SIM 채널을 여세요.</span>') +
      `<button type="button" class="ch-add" data-cmd="add-channel" ${S.chans.length >= MAX_CH ? 'disabled title="최대 4채널"' : ''}><svg class="i"><use href="#i-plus"/></svg>채널</button>`;
    // scope seg
    const opts = [['all', '전체']].concat(S.chans.map(c => [c.id, c.alias]));
    $('#scopeSeg').innerHTML = opts.map(([id, l]) => {
      const c = S.chans.find(x => x.id === id);
      return `<button type="button" role="radio" data-scope="${id}" aria-checked="${S.scope === id}">${c ? `<span class="chdot" style="--tag:${chColor(c)}"></span>` : ''}${esc(l)}</button>`;
    }).join('');
    const cf = S.chans.filter(c => S.scope === 'all' || c.id === S.scope)
      .map(c => `<b>${esc(c.alias)}</b> ${c.capFilter ? esc(c.capFilter) : '<i>(없음)</i>'}`).join(' · ');
    $('#capfSummary').innerHTML = `<span class="k">캡처 필터</span> ${cf || '—'} <span class="k">(채널 정지 후 변경)</span>`;
    const live = S.chans.filter(c => c.live).length;
    const sc = $('#shellConn'); sc.classList.toggle('live', live > 0);
    sc.innerHTML = `<span class="dot"></span><span>ETH <b>${S.chans.length}</b>ch · <b>${live}</b> LIVE</span>`;
  }
  function updateChannelCounts() {
    S.chans.forEach(ch => {
      const card = document.querySelector(`.chcard[data-ch="${ch.id}"]`); if (!card) return;
      const meta = card.querySelector('.ch-meta');
      const html = `${esc(ch.iface)} · ${ch.me.ip}/24 · <span class="link-up">link up</span> · <span class="cnt"><b data-role="cap-count">${fmtN(ch.captured)}</b> 캡처</span>${ch.dropped ? ` · <span class="drop">${fmtN(ch.dropped)} 삭제</span>` : ''}${ch.cfExcluded ? ` · ${fmtN(ch.cfExcluded)} 필터 제외` : ''}`;
      if (meta.innerHTML !== html) meta.innerHTML = html;
    });
  }

  /* ---------------- rendering: packet list (virtual) ---------------- */
  const body = () => $('#plBody');
  function renderList(force) {
    const b = body(), rows = $('#plRows'), n = S.view.length;
    $('#plSpacer').style.height = (n * ROW) + 'px';
    b.setAttribute('aria-rowcount', n);
    if (S.autoscroll && !S.paused && !S.userScrolled) b.scrollTop = Math.max(0, n * ROW - b.clientHeight);
    const top = b.scrollTop, h = b.clientHeight || 400;
    const first = Math.max(0, Math.floor(top / ROW) - 4), last = Math.min(n, Math.ceil((top + h) / ROW) + 4);
    const key = `${first}:${last}:${n}:${S.sel ? S.sel.no : 0}:${S.view[first] ? S.view[first].no : 0}:${S.colorize}`;
    if (!force && rows.dataset.key === key) return;
    rows.dataset.key = key;
    rows.style.transform = `translateY(${first * ROW}px)`;
    const conv = S.sel && S.sel.sum.conv ? S.sel.sum.conv.p + S.sel.sum.conv.idx : null;
    let html = '';
    for (let i = first; i < last; i++) {
      const p = S.view[i], s = p.sum;
      const c = p.sum.conv ? p.sum.conv.p + p.sum.conv.idx : null;
      html += `<div class="prow ${s.cls}${p === S.sel ? ' sel' : ''}${p.own ? ' own' : ''}${conv && c === conv ? ' conv' : ''}" role="row" aria-selected="${p === S.sel}" data-i="${i}" data-no="${p.no}">` +
        `<span class="c-no">${p.no}</span><span class="c-t">${(p.t / 1000).toFixed(6)}</span><span class="c-ch"><span class="chdot" style="--tag:${chColor(p.ch)}"></span>${esc(p.ch.alias)}</span>` +
        `<span class="c-src">${esc(s.src)}</span><span class="c-dst">${esc(s.dst)}</span><span class="c-pr">${esc(s.proto)}</span><span class="c-len">${p.bytes.length}</span><span class="c-info" title="${esc(s.info)}">${esc(s.info)}</span></div>`;
    }
    rows.innerHTML = html;
    const empty = $('#plEmpty');
    if (!n) {
      empty.hidden = false;
      const cap = capturedInScope();
      empty.textContent = !S.chans.length ? '열린 채널이 없습니다.' : cap ? `표시 필터와 일치하는 패킷 없음 (0 / ${fmtN(cap)})` : S.chans.some(c => c.live) ? '패킷 대기 중…' : '캡처된 패킷 없음 — 채널을 시작하세요.';
    } else empty.hidden = true;
  }
  function renderCounts() {
    const cap = capturedInScope();
    const pend = S.paused ? allPkts().filter(p => p.no > S.pausedAt && S.df.fn(p.f)).length : 0;
    $('#dfCount').innerHTML = `표시 <b data-role="shown">${fmtN(S.view.length)}</b> / 캡처 <b data-role="captured">${fmtN(cap)}</b>`;
    const drop = S.chans.filter(c => S.scope === 'all' || c.id === S.scope).reduce((a, c) => a + c.dropped, 0);
    const nt = $('#plNotice');
    if (S.paused) {
      nt.hidden = false; nt.className = 'pl-notice paused';
      // keep the resume button node stable: only the pending count text changes per tick
      if (!nt.querySelector('[data-role=pending]')) nt.innerHTML = `표시 일시정지 — 캡처는 계속 중, 새 일치 패킷 <b data-role="pending"></b>개 대기 <button type="button" class="btn" data-cmd="pause-view">재개하고 따라잡기</button>`;
      nt.querySelector('[data-role=pending]').textContent = fmtN(pend);
    } else if (drop) {
      nt.hidden = false; nt.className = 'pl-notice';
      if (nt.querySelector('[data-role=pending]')) nt.innerHTML = '';
      nt.innerHTML = `보존 한도 ${fmtN(RETAIN)}패킷/채널 — 오래된 <b data-role="dropped">${fmtN(drop)}</b>개 삭제됨 (목록·내보내기에서 제외)`;
    } else { nt.hidden = true; if (nt.innerHTML) nt.innerHTML = ''; }
    const scopeName = S.scope === 'all' ? '전체 채널' : (S.chans.find(c => c.id === S.scope) || {}).alias;
    $('#stTarget').innerHTML = `범위 <b>${esc(scopeName || '-')}</b> · 표시 <b>${fmtN(S.view.length)}</b>/${fmtN(cap)}${S.df.applied ? ` · 필터 <b>${esc(S.df.applied)}</b>` : ''}`;
    $('#stExtra').innerHTML = `${S.paused ? '<b>표시 일시정지</b> · ' : ''}sim ${(S.now / 1000).toFixed(1)}s${drop ? ` · <span class="drop">삭제 ${fmtN(drop)}</span>` : ''}${S.sel ? ` · 선택 #${S.sel.no}` : ''}`;
    $('#tbCtx').innerHTML = S.sel ? `#${S.sel.no} <b>${esc(S.sel.sum.proto)}</b> ${S.sel.bytes.length} B · ${esc(S.sel.ch.alias)}` : '';
  }

  /* ---------------- details tree + bytes ---------------- */
  let curTree = null, expanded = new Set(['1', '2', '3', '4', '5', '6']);
  function selectPacket(p, opts) {
    S.sel = p; S.selNode = null;
    const d = P.decode(p.bytes, { no: p.no, t: p.t, iface: p.ch.iface, chAlias: p.ch.alias, tcpIsn: S.isn, stream: streamIdx });
    curTree = d.tree;
    renderTree(); renderHex();
    renderList(true); renderCounts();
    if (opts && opts.scroll) scrollToSel();
    if (window.LoomEthSock && window.LoomEthSock.onSelect) window.LoomEthSock.onSelect(p);
  }
  function scrollToSel() {
    const i = S.view.indexOf(S.sel); if (i < 0) return;
    S.userScrolled = i < S.view.length - 1; // keep a non-latest selection in view (autoscroll pauses)
    const b = body(), y = i * ROW;
    if (y < b.scrollTop) b.scrollTop = y; else if (y + ROW > b.scrollTop + b.clientHeight) b.scrollTop = y + ROW - b.clientHeight;
    renderList(true);
  }
  function flatTree(nodes, path, depth, out) {
    nodes.forEach((n, i) => { const id = path ? path + '.' + (i + 1) : String(i + 1); out.push({ n, id, depth }); if (n.c.length && expanded.has(id)) flatTree(n.c, id, depth + 1, out); });
    return out;
  }
  function renderTree() {
    const el = $('#tree');
    if (!S.sel || !curTree) { el.innerHTML = '<div class="empty">패킷을 선택하면 디코드 트리가 표시됩니다.</div>'; return; }
    const rows = flatTree(curTree, '', 0, []);
    el.innerHTML = rows.map(({ n, id, depth }) => {
      const has = n.c.length > 0, on = S.selNode === id;
      const cls = ['tn', n.cls && n.cls.includes('proto') ? 'proto' : '', n.cls && n.cls.includes('meta') ? 'meta' : '', on ? 'on' : ''].join(' ');
      let label = esc(n.l).replace(/\[correct\]/, '<span class="good">[correct]</span>').replace(/\[incorrect\]/, '<span class="bad">[incorrect]</span>');
      return `<div class="${cls}" role="treeitem" data-id="${id}" data-o="${n.o}" data-n="${n.n}" aria-level="${depth + 1}" ${has ? `aria-expanded="${expanded.has(id)}"` : ''} aria-selected="${on}" style="padding-left:${4 + depth * 16}px">` +
        `<span class="tw">${has ? '<svg class="i"><use href="#i-chev"/></svg>' : ''}</span><span class="tl">${label}</span></div>`;
    }).join('');
  }
  function nodeById(id) { let arr = curTree, n = null; for (const k of id.split('.')) { n = arr[+k - 1]; if (!n) return null; arr = n.c; } return n; }
  function renderHex() {
    const el = $('#hex');
    if (!S.sel) { el.innerHTML = '<div class="empty">선택된 패킷 없음</div>'; $('#bSel').textContent = ''; return; }
    const b = S.sel.bytes; let html = '';
    for (let r = 0; r < b.length; r += 16) {
      let hx = '', as = '';
      for (let i = r; i < Math.min(r + 16, b.length); i++) {
        const c = b[i], pr = c >= 32 && c < 127;
        hx += `<b data-o="${i}">${P.h2(c)}</b>`;
        as += `<b data-o="${i}"${pr ? '' : ' class="np"'}>${pr ? esc(String.fromCharCode(c)) : '.'}</b>`;
      }
      html += `<div class="hx-row"><span class="hx-off">${r.toString(16).padStart(4, '0')}</span><span class="hx-h">${hx}</span><span class="hx-a">${as}</span></div>`;
    }
    el.innerHTML = html;
    paintHex();
  }
  function paintHex() {
    const el = $('#hex'); el.querySelectorAll('b.on, b.par').forEach(x => x.classList.remove('on', 'par'));
    const n = S.selNode ? nodeById(S.selNode) : null;
    if (!n || n.o < 0) { $('#bSel').textContent = S.sel ? `${S.sel.bytes.length} bytes · 필드를 선택하면 바이트 강조` : ''; return; }
    // parent protocol range lightly
    const top = nodeById(S.selNode.split('.')[0] + (S.selNode.includes('.') ? '' : ''));
    if (top && top !== n && top.o >= 0) for (let i = top.o; i < top.o + top.n; i++) el.querySelectorAll(`b[data-o="${i}"]`).forEach(x => x.classList.add('par'));
    for (let i = n.o; i < n.o + n.n; i++) el.querySelectorAll(`b[data-o="${i}"]`).forEach(x => { x.classList.remove('par'); x.classList.add('on'); });
    const hexs = Array.from(S.sel.bytes.slice(n.o, n.o + Math.min(n.n, 12))).map(P.h2).join(' ');
    $('#bSel').textContent = `offset 0x${n.o.toString(16).padStart(4, '0')} · ${n.n} byte${n.n > 1 ? 's' : ''} · ${hexs}${n.n > 12 ? ' …' : ''}`;
    const first = el.querySelector('b.on'); if (first && first.scrollIntoView) first.scrollIntoView({ block: 'nearest' });
  }
  function selectNode(id, fromBytes) {
    S.selNode = id;
    // ensure ancestors expanded
    const parts = id.split('.'); for (let i = 1; i < parts.length; i++) expanded.add(parts.slice(0, i).join('.'));
    renderTree(); paintHex();
    const tn = document.querySelector(`#tree .tn[data-id="${id}"]`); if (tn && tn.scrollIntoView) tn.scrollIntoView({ block: 'nearest' });
    if (fromBytes) $('#hex').focus({ preventScroll: true });
  }
  function deepestAt(off) {
    let best = null;
    (function walk(nodes, path) {
      nodes.forEach((n, i) => {
        const id = path ? path + '.' + (i + 1) : String(i + 1);
        if (n.o >= 0 && off >= n.o && off < n.o + n.n) { if (!best || n.n <= best.n.n) best = { n, id }; }
        if (n.c.length) walk(n.c, id);
      });
    })(curTree || [], '');
    // prefer non-frame node
    return best;
  }

  /* ---------------- master render ---------------- */
  let lastFull = 0;
  function render(force) {
    if (force || S.dirtyChannels) { renderChannels(); S.dirtyChannels = false; }
    else updateChannelCounts();
    if (S.dirtyDetail) { S.dirtyDetail = false; if (!S.sel) { curTree = null; renderTree(); renderHex(); } }
    renderList(force); renderCounts();
    if (window.LoomEthSock && window.LoomEthSock.render) window.LoomEthSock.render();
  }

  /* ---------------- persistence ---------------- */
  function persist() {
    store.set('channels', S.chans.map(c => ({ id: c.id, alias: c.alias, iface: c.iface, profile: c.profile, capFilter: c.capFilter, live: c.live })));
    store.set('scope', S.scope); store.set('active', S.activeCh);
  }
  function saveLayout() { store.set('layout', S.layout); }
  function applyLayout() {
    $('#cap').style.setProperty('--list-h', (S.layout.listFrac * 100).toFixed(2) + '%');
    $('#lower').style.setProperty('--dtl-w', (S.layout.dtlFrac * 100).toFixed(2) + '%');
    $('#lower').style.setProperty('--dtl-h', (S.layout.dtlHFrac * 100).toFixed(2) + '%');
  }

  /* ---------------- dividers (pointer drag + keyboard) ---------------- */
  function bindDivider(el, kind) {
    const narrow = () => window.matchMedia && window.matchMedia('(max-width: 860px)').matches;
    function frac(e) {
      if (kind === 'list') { const r = $('#cap').getBoundingClientRect(); return { k: 'listFrac', v: (e.clientY - r.top - 6) / (r.height - 12) }; }
      const r = $('#lower').getBoundingClientRect();
      return narrow() ? { k: 'dtlHFrac', v: (e.clientY - r.top) / r.height } : { k: 'dtlFrac', v: (e.clientX - r.left) / r.width };
    }
    el.addEventListener('pointerdown', e => {
      e.preventDefault(); el.setPointerCapture && el.setPointerCapture(e.pointerId);
      el.classList.add('dragging'); document.body.classList.add('resizing', kind === 'list' || narrow() ? 'row' : 'col');
      const move = ev => { const { k, v } = frac(ev); S.layout[k] = Math.min(0.85, Math.max(0.12, v)); applyLayout(); renderList(true); };
      const up = () => { el.classList.remove('dragging'); document.body.classList.remove('resizing', 'row', 'col'); el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up); el.removeEventListener('pointercancel', up); saveLayout(); };
      el.addEventListener('pointermove', move); el.addEventListener('pointerup', up); el.addEventListener('pointercancel', up);
    });
    el.addEventListener('keydown', e => {
      const k = kind === 'list' ? 'listFrac' : narrow() ? 'dtlHFrac' : 'dtlFrac';
      const dec = kind === 'list' || narrow() ? 'ArrowUp' : 'ArrowLeft', inc = kind === 'list' || narrow() ? 'ArrowDown' : 'ArrowRight';
      if (e.key !== dec && e.key !== inc) return;
      e.preventDefault(); S.layout[k] = Math.min(0.85, Math.max(0.12, S.layout[k] + (e.key === inc ? 0.03 : -0.03))); applyLayout(); saveLayout(); renderList(true);
    });
  }

  /* ---------------- toasts / dialogs / menus ---------------- */
  function toast(msg, kind) {
    const t = document.createElement('div'); t.className = 'toast' + (kind ? ' ' + kind : ''); t.textContent = msg;
    $('#toasts').appendChild(t); setTimeout(() => t.remove(), kind === 'err' ? 5200 : 3200);
  }
  function openMenu(anchor, items) {
    const m = $('#menu');
    m.innerHTML = items.map((it, i) => it === '-' ? '<hr>' : `<button type="button" role="menuitem" data-mi="${i}" ${it.disabled ? 'disabled' : ''} class="${it.danger ? 'danger' : ''}" title="${esc(it.title || '')}">${esc(it.label)}</button>`).join('');
    m.hidden = false;
    const r = anchor.getBoundingClientRect();
    m.style.left = Math.max(8, Math.min(window.innerWidth - 220, r.right - 200)) + 'px'; m.style.top = (r.bottom + 4) + 'px';
    m.onclick = e => { const b = e.target.closest('[data-mi]'); if (!b || b.disabled) return; closeMenu(); items[+b.dataset.mi].run(); };
    const f = m.querySelector('button:not(:disabled)'); if (f) f.focus();
  }
  function closeMenu() { $('#menu').hidden = true; }
  function confirmDlg(title, msg, okLabel) {
    return new Promise(res => {
      const d = $('#dlgConfirm');
      d.innerHTML = `<div class="dlg-h"><h2>${esc(title)}</h2></div><div class="dlg-b"><div>${msg}</div></div><div class="dlg-f"><button type="button" class="btn" data-r="0">취소</button><button type="button" class="btn danger" data-r="1">${esc(okLabel)}</button></div>`;
      d.onclick = e => { const b = e.target.closest('[data-r]'); if (b) { d.close(); res(b.dataset.r === '1'); } };
      d.oncancel = () => res(false);
      d.showModal(); d.querySelector('[data-r="0"]').focus();
    });
  }

  /* ---------------- channel dialog ---------------- */
  function channelDialog(ch) {
    const d = $('#dlgChannel'), edit = !!ch;
    const usedIf = new Set(S.chans.filter(c => c !== ch).map(c => c.iface));
    const ifs = ['sim0', 'sim1', 'sim2', 'sim3'].filter(x => !usedIf.has(x));
    const defP = !edit ? (['doip', 'someip', 'generic'].find(p => !S.chans.some(c => c.profile === p)) || 'generic') : ch.profile;
    d.innerHTML = `<form method="dialog" novalidate>
      <div class="dlg-h"><h2>${edit ? '채널 설정 — ' + esc(ch.alias) : 'SIM 채널 추가'}</h2><button type="button" class="ibtn" data-x title="닫기"><svg class="i"><use href="#i-x"/></svg></button></div>
      <div class="dlg-b">
        <div class="sk-note">가상(SIM) 인터페이스입니다. 실제 NIC 탐색·캡처를 하지 않으며 결정적 시뮬레이션 트래픽만 생성합니다.</div>
        <div class="fg"><label for="cdAlias">별칭</label><input class="fld" id="cdAlias" name="alias" maxlength="24" value="${esc(edit ? ch.alias : '')}" placeholder="예: lab-gen">
          <div class="err" data-err="alias"></div>
          <label for="cdIf">인터페이스</label><select class="fld" id="cdIf" name="iface" ${edit ? 'disabled' : ''}>${(edit ? [ch.iface] : ifs).map(x => `<option>${x}</option>`).join('')}</select>
          <div class="hint">SIM 이름 — 같은 이름 중복 불가</div>
          <label for="cdProf">트래픽 프로필</label><select class="fld" id="cdProf" name="profile" ${edit ? 'disabled' : ''}>${Object.entries(PROFILES).map(([k, v]) => `<option value="${k}" ${k === defP ? 'selected' : ''}>${esc(v.label)}</option>`).join('')}</select>
          <label for="cdCf">캡처 필터</label><input class="fld" id="cdCf" name="cf" value="${esc(edit ? ch.capFilter : '')}" placeholder="libpcap 문법 일부, 예: not arp, udp port 30490" ${edit && ch.live ? 'disabled' : ''}>
          <div class="hint">${edit && ch.live ? '캡처 중에는 변경 불가 — 먼저 이 채널을 정지하세요.' : '캡처 시점 적용: 일치하지 않는 패킷은 저장되지 않음 (표시 필터와 다름). 비우면 전체.'}</div>
          <div class="err" data-err="cf"></div>
        </div>
        ${edit ? '' : '<label class="sk-row" style="font-size:12px"><input type="checkbox" name="start" checked> 추가 후 바로 캡처 시작</label>'}
      </div>
      <div class="dlg-f"><button type="button" class="btn" data-x>취소</button><button type="submit" class="btn primary">${edit ? '저장' : '추가'}</button></div></form>`;
    const form = d.querySelector('form');
    d.querySelectorAll('[data-x]').forEach(b => b.onclick = () => d.close());
    form.onsubmit = e => {
      e.preventDefault();
      const alias = form.elements.alias.value.trim(), cfv = form.elements.cf.value.trim();
      let bad = false;
      const ae = form.querySelector('[data-err=alias]'), ce = form.querySelector('[data-err=cf]');
      ae.textContent = ''; ce.textContent = '';
      if (!/^[\w.-]{1,24}$/.test(alias)) { ae.textContent = '영문/숫자/._- 1–24자'; bad = true; }
      else if (S.chans.some(c => c !== ch && c.alias === alias)) { ae.textContent = '이미 사용 중인 별칭'; bad = true; }
      const cfr = P.parseCF(cfv); if (!cfr.ok) { ce.textContent = '캡처 필터 오류: ' + cfr.err; bad = true; }
      if (bad) return;
      if (edit) {
        const renamed = ch.alias !== alias; ch.alias = alias;
        if (!ch.live) { ch.capFilter = cfv; ch.cf = cfr.fn; }
        if (renamed) toast('별칭 변경 — 기존 패킷의 loom.ch 값도 새 별칭으로 표시됩니다');
        S.chans.forEach(c => c.pkts.forEach(p => { p.f['loom.ch'] = [p.ch.alias]; }));
      } else {
        const nc = mkChannel({ alias, iface: form.elements.iface.value, profile: form.elements.profile.value, capFilter: cfv });
        S.activeCh = nc.id;
        if (form.elements.start.checked) startCh(nc);
        toast(`채널 ${alias} (${nc.iface}, SIM) 추가`);
      }
      d.close(); persist(); rebuildView(); render(true);
    };
    d.showModal(); form.elements.alias.focus();
  }
  async function closeChannel(ch) {
    const ok = await confirmDlg('채널 닫기', `<b>${esc(ch.alias)}</b> (${esc(ch.iface)}) 채널을 닫습니다. 보존된 <b>${fmtN(ch.pkts.length)}</b>개 패킷과 이 채널의 소켓 세션 기록이 삭제됩니다. 다른 채널은 영향 없음.`, '채널 닫기');
    if (!ok) return;
    ch.live = false; ch.pkts.forEach(p => { p.gone = true; });
    S.chans = S.chans.filter(c => c !== ch);
    if (S.scope === ch.id) S.scope = 'all';
    if (S.activeCh === ch.id) S.activeCh = S.chans[0] ? S.chans[0].id : null;
    if (S.sel && S.sel.ch === ch) { S.sel = null; S.dirtyDetail = true; }
    if (window.LoomEthSock && window.LoomEthSock.onChannelClosed) window.LoomEthSock.onChannelClosed(ch);
    persist(); rebuildView(); render(true);
    toast(`채널 ${ch.alias} 닫음`);
  }
  function burst(ch) {
    if (!ch.live) { toast('캡처 정지 상태 — 버스트는 LIVE 채널에서만', 'warn'); return; }
    // deterministic burst: 500 syslog-like UDP frames (exercise retention / drop notice)
    const me = ch.me, peer = { ip: PROFILES[ch.profile].net + '.99', mac: '02:00:00:00:00:99' };
    for (let i = 0; i < 500; i++) capture(ch, P.udpFrame(peer, me, 40000 + (i % 8), 9999, P.txt(`burst ${i + 1}/500 seq=${ch.captured}`), ch.ipid()), S.now + i * 0.2);
    toast(`${ch.alias}: 버스트 500 프레임 주입 (보존 한도 ${fmtN(RETAIN)}/채널)`);
    render(false);
  }

  /* ---------------- events ---------------- */
  function cmd(c, el) {
    switch (c) {
      case 'add-channel': if (S.chans.length >= MAX_CH) toast('최대 4채널', 'warn'); else channelDialog(null); break;
      case 'start-all': S.chans.forEach(startCh); render(true); break;
      case 'stop-all': S.chans.forEach(stopCh); render(true); toast('모든 채널 캡처 정지 — 기록 유지'); break;
      case 'pause-view': togglePause(); break;
      case 'autoscroll': S.autoscroll = !S.autoscroll; S.userScrolled = false; $('#tbAuto').setAttribute('aria-pressed', S.autoscroll); render(true); break;
      case 'colorize': S.colorize = !S.colorize; $('#tbColor').setAttribute('aria-pressed', S.colorize); $('#plist').classList.toggle('colorize', S.colorize); store.set('colorize', S.colorize); render(true); break;
      case 'df-apply': hideSug(); applyDF($('#dfInput').value); break;
      case 'df-clear': $('#dfInput').value = ''; hideSug(); applyDF(''); break;
      case 'clear-view': clearScope(); break;
      case 'tree-expand': if (curTree) { (function w(ns, p) { ns.forEach((n, i) => { const id = p ? p + '.' + (i + 1) : String(i + 1); if (n.c.length) { expanded.add(id); w(n.c, id); } }); })(curTree, ''); renderTree(); } break;
      case 'tree-collapse': expanded.clear(); if (S.selNode) S.selNode = S.selNode.split('.')[0]; renderTree(); paintHex(); break;
      case 'keys': keysDialog(); break;
      default: if (window.LoomEthSock && window.LoomEthSock.cmd) window.LoomEthSock.cmd(c, el);
    }
  }
  function togglePause() {
    S.paused = !S.paused;
    if (S.paused) { S.pausedAt = S.nextNo - 1; }
    else { rebuildView(); S.userScrolled = false; toast('표시 재개 — 일시정지 동안 캡처된 패킷 반영'); }
    const b = $('#tbPause'); b.setAttribute('aria-pressed', S.paused); b.classList.toggle('paused', S.paused);
    $('#tbPauseLabel').textContent = S.paused ? '표시 재개' : '표시 일시정지';
    render(true);
  }
  async function clearScope() {
    const chs = S.chans.filter(c => S.scope === 'all' || c.id === S.scope);
    const n = chs.reduce((a, c) => a + c.pkts.length, 0);
    if (!n) { toast('지울 기록 없음'); return; }
    const ok = await confirmDlg('기록 지우기', `${chs.map(c => '<b>' + esc(c.alias) + '</b>').join(', ')}의 보존 패킷 ${fmtN(n)}개를 지웁니다. 캡처 상태(LIVE/STOPPED)는 바뀌지 않습니다.`, '지우기');
    if (!ok) return;
    chs.forEach(c => { c.pkts.forEach(p => { p.gone = true; }); c.pkts = []; c.dropped = 0; });
    if (S.sel && S.sel.gone) { S.sel = null; S.dirtyDetail = true; }
    rebuildView(); render(true);
  }
  function keysDialog() {
    const d = $('#dlgKeys');
    const rows = [['↑ / ↓', '패킷 목록 이전/다음 (목록 포커스)'], ['Home / End, PgUp / PgDn', '처음/끝, 페이지 이동'], ['F7 / F8 · Ctrl+↑ / Ctrl+↓', '어디서든 이전/다음 패킷 (Wireshark 관례)'],
      ['Space', '표시 일시정지/재개 (목록 포커스) — 캡처는 계속'], ['Ctrl+/', '표시 필터로 포커스'], ['Enter / Esc', '필터 적용 / 편집 되돌리기'],
      ['← / →', '트리 접기/펼치기 (트리 포커스)'], ['경계 포커스 + 방향키', '패널 경계 크기 조절'], ['Alt+S', '소켓 테스트 패널'], ['?', '이 도움말']];
    d.innerHTML = `<div class="dlg-h"><h2>단축키</h2><button type="button" class="ibtn" data-x><svg class="i"><use href="#i-x"/></svg></button></div>
      <div class="dlg-b"><div class="kv">${rows.map(([k, v]) => `<span><kbd>${esc(k)}</kbd></span><span>${esc(v)}</span>`).join('')}</div>
      <div class="sk-note">표시 필터 지원 부분집합: 프로토콜 이름 (eth arp ip tcp udp doip uds someip someipsd data), 필드 비교 <code>== != &gt; &lt; &gt;= &lt;=</code> (eq ne gt lt ge le), 문자열 필드 <code>contains</code>, <code>&amp;&amp; || !</code> (and or not), 괄호, <code>ip.addr == a.b.c.d/nn</code>. 지원 필드는 입력 중 제안 목록에 표시. <code>loom.ch</code>는 Loom 확장(채널 별칭).</div></div>`;
    d.querySelector('[data-x]').onclick = () => d.close();
    d.showModal();
  }

  function moveSel(delta, abs) {
    if (!S.view.length) return;
    let i = S.sel ? S.view.indexOf(S.sel) : -1;
    if (abs === 'home') i = 0; else if (abs === 'end') i = S.view.length - 1;
    else i = i < 0 ? (delta > 0 ? 0 : S.view.length - 1) : Math.min(S.view.length - 1, Math.max(0, i + delta));
    if (i !== S.view.length - 1) S.userScrolled = true; else S.userScrolled = false;
    selectPacket(S.view[i], { scroll: true });
  }

  function bind() {
    document.addEventListener('click', e => {
      if (!e.target.closest('#menu') && !e.target.closest('[data-act="ch-menu"]')) closeMenu();
      if (!e.target.closest('.dfwrap')) hideSug();
      const c = e.target.closest('[data-cmd]'); if (c && !c.disabled) { cmd(c.dataset.cmd, c); return; }
      const sc = e.target.closest('[data-scope]'); if (sc) { S.scope = sc.dataset.scope; persist(); rebuildView(); render(true); return; }
      const card = e.target.closest('.chcard');
      if (card) {
        const ch = S.chans.find(x => x.id === card.dataset.ch); if (!ch) return;
        const a = e.target.closest('[data-act]'); const act = a && a.dataset.act;
        if (act === 'ch-start') { startCh(ch); render(true); }
        else if (act === 'ch-stop') { stopCh(ch); render(true); toast(`${ch.alias} 캡처 정지 — 기록 ${fmtN(ch.pkts.length)}개 유지, 다른 채널 계속`); }
        else if (act === 'ch-menu') openMenu(a, [
          { label: ch.live ? '캡처 정지' : '캡처 시작', run: () => { ch.live ? stopCh(ch) : startCh(ch); render(true); } },
          { label: '이 채널만 보기', run: () => { S.scope = ch.id; persist(); rebuildView(); render(true); } },
          { label: '설정 (별칭·캡처 필터)…', run: () => channelDialog(ch) },
          { label: '버스트 500 프레임 (보존 한도 확인)', disabled: !ch.live, run: () => burst(ch) },
          '-',
          { label: '채널 닫기…', danger: true, run: () => closeChannel(ch) }
        ]);
        S.activeCh = ch.id; persist();
        document.querySelectorAll('.chcard').forEach(x => x.classList.toggle('active', x.dataset.ch === ch.id));
        if (window.LoomEthSock && window.LoomEthSock.onActive) window.LoomEthSock.onActive(ch);
        return;
      }
      const row = e.target.closest('.prow');
      if (row) { const p = S.view[+row.dataset.i]; if (p) { S.userScrolled = p !== S.view[S.view.length - 1]; selectPacket(p); body().focus({ preventScroll: true }); } return; }
      const tn = e.target.closest('#tree .tn');
      if (tn) {
        const id = tn.dataset.id;
        if (e.target.closest('.tw') && tn.hasAttribute('aria-expanded')) { expanded.has(id) ? expanded.delete(id) : expanded.add(id); renderTree(); return; }
        selectNode(id); return;
      }
      const hb = e.target.closest('#hex b[data-o]');
      if (hb) { const d = deepestAt(+hb.dataset.o); if (d) selectNode(d.id, true); }
    });
    document.addEventListener('dblclick', e => {
      const tn = e.target.closest('#tree .tn[aria-expanded]'); if (tn) { const id = tn.dataset.id; expanded.has(id) ? expanded.delete(id) : expanded.add(id); renderTree(); }
    });
    body().addEventListener('scroll', () => {
      const b = body(); const atEnd = b.scrollTop + b.clientHeight >= b.scrollHeight - ROW * 1.5;
      S.userScrolled = !atEnd; renderList(false);
      const hd = document.querySelector('.pl-head'); if (hd && hd.scrollLeft !== b.scrollLeft) hd.scrollLeft = b.scrollLeft;
    });
    body().addEventListener('keydown', e => {
      const page = Math.max(1, Math.floor(body().clientHeight / ROW) - 1);
      const map = { ArrowDown: [1], ArrowUp: [-1], PageDown: [page], PageUp: [-page], Home: [0, 'home'], End: [0, 'end'] };
      if (map[e.key]) { e.preventDefault(); moveSel(...map[e.key]); }
      else if (e.key === ' ') { e.preventDefault(); togglePause(); }
    });
    $('#tree').addEventListener('keydown', e => {
      if (!curTree) return;
      const rows = flatTree(curTree, '', 0, []); let i = rows.findIndex(r => r.id === S.selNode);
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); i = Math.min(rows.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1))); selectNode(rows[i].id); }
      else if ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && i >= 0) {
        e.preventDefault(); const r = rows[i];
        if (e.key === 'ArrowRight' && r.n.c.length) expanded.add(r.id);
        else if (e.key === 'ArrowLeft') { if (expanded.has(r.id)) expanded.delete(r.id); else if (r.id.includes('.')) { selectNode(r.id.split('.').slice(0, -1).join('.')); return; } }
        renderTree();
      }
    });
    const inp = $('#dfInput');
    inp.addEventListener('input', () => { validateLive(); showSug(); });
    inp.addEventListener('focus', showSug);
    inp.addEventListener('keydown', e => {
      const ul = $('#dfSug'), opts = [...ul.querySelectorAll('li[role=option]')], cur = ul.querySelector('[aria-selected=true]');
      if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !ul.hidden && opts.length) {
        e.preventDefault(); let i = opts.indexOf(cur); i = e.key === 'ArrowDown' ? (i + 1) % opts.length : (i <= 0 ? opts.length - 1 : i - 1);
        opts.forEach(o => o.setAttribute('aria-selected', 'false')); opts[i].setAttribute('aria-selected', 'true'); inp.setAttribute('aria-activedescendant', opts[i].id); return;
      }
      if (e.key === 'Enter') { e.preventDefault(); if (cur && !ul.hidden) { inp.value = cur.dataset.v; hideSug(); validateLive(); return; } hideSug(); applyDF(inp.value); }
      else if (e.key === 'Escape') { if (!ul.hidden) { hideSug(); return; } inp.value = S.df.applied; validateLive(); }
    });
    $('#dfSug').addEventListener('mousedown', e => { const li = e.target.closest('li[data-v]'); if (li) { e.preventDefault(); inp.value = li.dataset.v; hideSug(); validateLive(); inp.focus(); } });
    document.addEventListener('keydown', e => {
      const typing = /INPUT|TEXTAREA|SELECT/.test((e.target && e.target.tagName) || '');
      if ((e.ctrlKey || e.metaKey) && e.key === '/') { e.preventDefault(); inp.focus(); inp.select(); return; }
      if (e.key === 'F8' || (e.ctrlKey && e.key === 'ArrowDown')) { e.preventDefault(); moveSel(1); return; }
      if (e.key === 'F7' || (e.ctrlKey && e.key === 'ArrowUp')) { e.preventDefault(); moveSel(-1); return; }
      if (e.altKey && (e.key === 's' || e.key === 'S' || e.code === 'KeyS')) { e.preventDefault(); cmd('toggle-socket'); return; }
      if (!typing && e.key === '?') { keysDialog(); return; }
      if (e.key === 'Escape') closeMenu();
    });
    bindDivider($('#divList'), 'list');
    bindDivider($('#divLower'), 'lower');
    window.addEventListener('resize', () => renderList(true));
  }

  /* ---------------- boot ---------------- */
  function boot() {
    S.colorize = store.get('colorize', true);
    $('#plist').classList.toggle('colorize', S.colorize); $('#tbColor').setAttribute('aria-pressed', S.colorize);
    const saved = store.get('channels', null);
    const cfgs = saved && saved.length ? saved : [
      { alias: 'ecu-doip', iface: 'sim0', profile: 'doip', capFilter: '', live: true },
      { alias: 'body-someip', iface: 'sim1', profile: 'someip', capFilter: '', live: true }
    ];
    cfgs.slice(0, MAX_CH).forEach(c => { const ch = mkChannel(c); if (c.live) startCh(ch); });
    S.scope = store.get('scope', 'all'); if (S.scope !== 'all' && !S.chans.some(c => c.id === S.scope)) S.scope = 'all';
    S.activeCh = store.get('active', null); if (!S.chans.some(c => c.id === S.activeCh)) S.activeCh = S.chans[0] ? S.chans[0].id : null;
    const df = store.get('df', '');
    $('#dfInput').value = df; if (df) { const r = P.parseDF(df); if (r.ok) S.df = { text: df, applied: df, fn: r.fn, ok: true }; }
    applyLayout(); bind(); persist();
    if (saved && saved.length) setDfMsg('재로드: 채널 구성·표시 필터·레이아웃 복원 (캡처 기록·소켓 연결·반복 전송은 복원하지 않음).', false);
    render(true); renderTree(); renderHex(); validateLive();
    if (!window.__loomEthManualClock) setInterval(() => { tick(TICK); render(false); }, TICK);
  }

  window.LoomEth = { S, P, PROFILES, hostOf, capture, schedule, tick, render, toast, esc, store, chColor, fmtN, selectPacket, applyDF, openMenu, confirmDlg, persist, saveLayout, applyLayout, rebuildView, streamIdx };
  window.loomEthOpus = {
    snapshot() {
      return {
        now: S.now, channels: S.chans.map(c => ({ id: c.id, alias: c.alias, iface: c.iface, live: c.live, captured: c.captured, retained: c.pkts.length, dropped: c.dropped, cfExcluded: c.cfExcluded, capFilter: c.capFilter })),
        scope: S.scope, shown: S.view.length, capturedInScope: capturedInScope(), paused: S.paused, df: S.df.applied,
        sel: S.sel ? { no: S.sel.no, len: S.sel.bytes.length, proto: S.sel.sum.proto } : null, selNode: S.selNode
      };
    },
    tick(n, ms) { for (let i = 0; i < (n || 1); i++) tick(ms || TICK); render(false); }
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot); else boot();
})();
