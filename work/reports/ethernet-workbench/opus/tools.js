/* Loom Ethernet mockup (Opus) — Follow Stream (selected TCP/UDP conversation) and export of displayed rows
   (CSV of list columns, classic libpcap .pcap of the same packets). */
(function () {
  'use strict';
  const L = window.LoomEth, P = L.P, S = L.S, esc = L.esc;
  const $ = s => document.querySelector(s);
  const fs = { dir: 'both', as: 'ascii', conv: null };

  function convPackets(sel) {
    const c = sel.sum.conv;
    return sel.ch.pkts.filter(p => p.sum.conv && p.sum.conv.p === c.p && p.sum.conv.key === c.key);
  }
  function payloadOf(p) {
    // payload range = last node "TCP payload"/"UDP payload" in decoded tree; derive from header lengths
    const b = p.bytes, ihl = (b[14] & 15) * 4, tot = (b[16] << 8) | b[17], l4 = 14 + ihl, end = 14 + tot;
    if (b[23] === 17) return b.slice(l4 + 8, end);
    const doff = (b[l4 + 12] >> 4) * 4; return b.slice(l4 + doff, end);
  }
  function dumpHex(bytes, base) {
    let out = '';
    for (let r = 0; r < bytes.length; r += 16) {
      const row = Array.from(bytes.slice(r, r + 16));
      out += (base + r).toString(16).padStart(8, '0') + '  ' + row.map(P.h2).join(' ').padEnd(47) + '  ' + row.map(c => c >= 32 && c < 127 ? String.fromCharCode(c) : '.').join('') + '\n';
    }
    return out;
  }
  function openFollow() {
    if (!S.sel) { L.toast('먼저 TCP/UDP 패킷을 선택하세요', 'warn'); return; }
    if (!S.sel.sum.conv) { L.toast(`#${S.sel.no} (${S.sel.sum.proto})는 TCP/UDP 대화가 아님 — 따라갈 스트림 없음`, 'warn'); return; }
    const pk = convPackets(S.sel);
    const first = pk.find(p => p.f['tcp.flags.syn'] && p.f['tcp.flags.syn'][0] && !p.f['tcp.flags.ack'][0]) || pk[0];
    fs.conv = { ...S.sel.sum.conv, ch: S.sel.ch, cli: `${first.sum.src}:${(first.f['tcp.srcport'] || first.f['udp.srcport'])[0]}` };
    fs.dir = 'both';
    renderFollow();
    $('#dlgFollow').showModal();
  }
  function renderFollow() {
    const d = $('#dlgFollow'), c = fs.conv; if (!c) return;
    const all = c.ch.pkts.filter(p => p.sum.conv && p.sum.conv.p === c.p && p.sum.conv.key === c.key);
    const srv = c.a === c.cli ? c.b : c.a;
    let cliB = 0, srvB = 0, segs = 0, txt = '', off = { c: 0, s: 0 };
    all.forEach(p => {
      const pl = payloadOf(p); if (!pl.length) return;
      const fromCli = `${p.sum.src}:${(p.f['tcp.srcport'] || p.f['udp.srcport'])[0]}` === c.cli;
      if (fromCli) cliB += pl.length; else srvB += pl.length;
      if (fs.dir === 'cli' && !fromCli) return; if (fs.dir === 'srv' && fromCli) return;
      segs++;
      const k = fromCli ? 'c' : 's';
      let body;
      if (fs.as === 'ascii') body = esc(Array.from(pl).map(x => x === 10 ? '\n' : x === 13 ? '' : x >= 32 && x < 127 ? String.fromCharCode(x) : '.').join(''));
      else if (fs.as === 'hexdump') body = esc(dumpHex(pl, off[k]));
      else body = Array.from(pl).map(P.h2).join('');
      off[k] += pl.length;
      txt += `<span class="${k}" title="#${p.no} · ${(p.t / 1000).toFixed(6)} s · ${pl.length} B">${body}</span>`;
    });
    const filt = `${c.p}.stream == ${c.idx} && loom.ch == "${c.ch.alias}"`;
    d.innerHTML = `<div class="dlg-h"><h2>Follow ${c.p.toUpperCase()} Stream (${c.p}.stream eq ${c.idx}) — ${esc(c.ch.alias)}</h2><button type="button" class="ibtn" data-fs="close" title="닫기"><svg class="i"><use href="#i-x"/></svg></button></div>
      <div class="dlg-b">
        <div class="fs-sum"><span class="c">${esc(c.cli)}</span> → <span class="s">${esc(srv)}</span> : ${cliB} bytes · <span class="s">${esc(srv)}</span> → <span class="c">${esc(c.cli)}</span> : ${srvB} bytes · 보존된 ${all.length} 패킷 (열 때의 스냅샷 — 라이브 갱신 안 됨, 새로고침 버튼)</div>
        <div class="fs-bar">
          <label>방향 <select class="fld" data-fs="dir"><option value="both" ${fs.dir === 'both' ? 'selected' : ''}>전체 대화 (${cliB + srvB} B)</option><option value="cli" ${fs.dir === 'cli' ? 'selected' : ''}>${esc(c.cli)} → ${esc(srv)} (${cliB} B)</option><option value="srv" ${fs.dir === 'srv' ? 'selected' : ''}>${esc(srv)} → ${esc(c.cli)} (${srvB} B)</option></select></label>
          <label>표시 형식 <select class="fld" data-fs="as"><option value="ascii" ${fs.as === 'ascii' ? 'selected' : ''}>ASCII</option><option value="hexdump" ${fs.as === 'hexdump' ? 'selected' : ''}>Hex Dump</option><option value="raw" ${fs.as === 'raw' ? 'selected' : ''}>Raw</option></select></label>
          <button type="button" class="btn" data-fs="refresh">새로고침</button>
        </div>
        <div class="fs-body" id="fsBody" tabindex="0">${txt || '<span class="gap">이 방향/스트림에 페이로드 없음 (핸드셰이크·ACK만)</span>'}</div>
        <div class="sk-note">클라이언트(첫 SYN/첫 패킷 송신자) <b style="color:var(--fs-cli)">빨강</b>, 서버 <b style="color:var(--fs-srv)">파랑</b> — Wireshark Follow Stream 관례. 세그먼트에 마우스를 올리면 프레임 번호.</div>
      </div>
      <div class="dlg-f"><button type="button" class="btn" data-fs="exclude" title="이 스트림을 제외하는 표시 필터 적용">이 스트림 제외</button><button type="button" class="btn primary" data-fs="only" title="${esc(filt)}">이 스트림만 표시</button><button type="button" class="btn" data-fs="close">닫기</button></div>`;
  }
  document.addEventListener('click', e => {
    const b = e.target.closest('#dlgFollow [data-fs]'); if (!b || b.tagName === 'SELECT') return;
    const c = fs.conv, d = $('#dlgFollow'), act = b.dataset.fs;
    const filt = c ? `${c.p}.stream == ${c.idx} && loom.ch == "${c.ch.alias}"` : '';
    if (act === 'close') d.close();
    else if (act === 'refresh') renderFollow();
    else if (act === 'only' || act === 'exclude') {
      const f = act === 'only' ? filt : `!(${filt})`;
      $('#dfInput').value = f; d.close(); L.applyDF(f);
    }
  });
  document.addEventListener('change', e => {
    const s = e.target.closest('#dlgFollow select[data-fs]'); if (!s) return;
    fs[s.dataset.fs] = s.value; renderFollow();
  });

  /* ---------------- export ---------------- */
  const live = () => S.view.filter(p => !p.gone);
  function download(name, mime, data) {
    const blob = new Blob([data], { type: mime });
    window.__loomEthLastExport = { name, mime, size: blob.size, data };
    try {
      const url = URL.createObjectURL(blob), a = document.createElement('a');
      a.href = url; a.download = name; document.body.appendChild(a); a.click(); a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 4000);
    } catch (err) { L.toast('브라우저가 다운로드를 차단: ' + err.message, 'err'); return false; }
    return true;
  }
  function stamp() { const d = new Date(); const z = n => String(n).padStart(2, '0'); return `${d.getFullYear()}${z(d.getMonth() + 1)}${z(d.getDate())}-${z(d.getHours())}${z(d.getMinutes())}${z(d.getSeconds())}`; }
  function exportCsv() {
    const rows = live();
    const q = v => `"${String(v).replace(/"/g, '""')}"`;
    const csv = ['No.,Time,Channel,Interface,Source,Destination,Protocol,Length,Info'].concat(rows.map(p => [p.no, (p.t / 1000).toFixed(6), q(p.ch.alias), p.ch.iface, q(p.sum.src), q(p.sum.dst), q(p.sum.proto), p.bytes.length, q(p.sum.info)].join(','))).join('\r\n') + '\r\n';
    const name = `loom-eth-sim-${stamp()}.csv`;
    if (download(name, 'text/csv', csv)) L.toast(`CSV ${rows.length}행 (${name}) — 브라우저 다운로드로 전달`);
    return csv;
  }
  function exportPcap() {
    const rows = live();
    const buf = P.toPcap(rows, S.epoch);
    const name = `loom-eth-sim-${stamp()}.pcap`;
    if (download(name, 'application/vnd.tcpdump.pcap', buf)) L.toast(`pcap ${rows.length} 패킷, ${buf.length} bytes (${name}) — 브라우저 다운로드로 전달`);
    return buf;
  }
  function openExport() {
    const d = $('#dlgExport'), rows = live();
    const chs = [...new Set(rows.map(p => p.ch.alias))];
    const bytes = rows.reduce((a, p) => a + 16 + p.bytes.length, 24);
    d.innerHTML = `<div class="dlg-h"><h2>표시된 패킷 내보내기</h2><button type="button" class="ibtn" data-ex="close"><svg class="i"><use href="#i-x"/></svg></button></div>
      <div class="dlg-b">
        <div class="kv"><span>대상</span><span><b>${rows.length}</b>개 — 현재 목록에 표시된 행 그대로 (범위 ${esc(S.scope === 'all' ? '전체' : (S.chans.find(c => c.id === S.scope) || {}).alias || '')}${S.df.applied ? `, 필터 <code>${esc(S.df.applied)}</code>` : ''}${S.paused ? ', 일시정지 스냅샷' : ''})</span>
          <span>채널</span><span>${esc(chs.join(', ') || '—')}</span></div>
        <div class="sk-note"><b>CSV</b>: 목록 열(No., Time, Channel, Interface, Source, Destination, Protocol, Length, Info).<br>
        <b>pcap (libpcap classic, LINKTYPE_ETHERNET, µs)</b>: 같은 패킷의 실제 프레임 바이트 — Wireshark/tcpdump로 열 수 있는 형식. 채널 별칭은 pcap에 담기지 않음(pcapng 인터페이스 블록 미구현). 예상 크기 ${bytes} bytes. 모두 SIM 생성 트래픽.</div>
      </div>
      <div class="dlg-f"><button type="button" class="btn" data-ex="close">취소</button><button type="button" class="btn" data-ex="csv" ${rows.length ? '' : 'disabled'}>CSV 내보내기</button><button type="button" class="btn primary" data-ex="pcap" ${rows.length ? '' : 'disabled'}>pcap 내보내기</button></div>`;
    d.showModal();
  }
  document.addEventListener('click', e => {
    const b = e.target.closest('#dlgExport [data-ex]'); if (!b || b.disabled) return;
    const d = $('#dlgExport');
    if (b.dataset.ex === 'csv') exportCsv(); else if (b.dataset.ex === 'pcap') exportPcap();
    d.close();
  });

  window.LoomEthTools = {
    cmd(c) { if (c === 'follow') openFollow(); else if (c === 'export') openExport(); },
    _exportCsv: exportCsv, _exportPcap: exportPcap, _openFollow: openFollow, _fs: fs
  };
})();
