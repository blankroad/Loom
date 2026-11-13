/* Loom Ethernet mockup (Opus) — packet engine.
   Pure functions: build real frame bytes, decode bytes -> {summary, fields, tree with [offset,len]},
   display-filter subset, capture-filter (BPF subset), classic libpcap writer.
   The list/detail/bytes panes are all derived from the same Uint8Array, so they cannot disagree. */
(function (root) {
  'use strict';
  const h2 = n => (n & 255).toString(16).padStart(2, '0');
  const h4 = n => (n & 0xffff).toString(16).padStart(4, '0');
  const hx = (n, w) => '0x' + n.toString(16).padStart(w, '0');
  const macB = s => s.split(':').map(x => parseInt(x, 16));
  const ipB = s => s.split('.').map(Number);
  const macS = (b, o) => [0, 1, 2, 3, 4, 5].map(i => h2(b[o + i])).join(':');
  const ipS = (b, o) => `${b[o]}.${b[o + 1]}.${b[o + 2]}.${b[o + 3]}`;
  const u16 = (b, o) => (b[o] << 8) | b[o + 1];
  const u32 = (b, o) => ((b[o] << 24) >>> 0) + (b[o + 1] << 16) + (b[o + 2] << 8) + b[o + 3];
  const p16 = n => [(n >> 8) & 255, n & 255];
  const p32 = n => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
  const txt = s => Array.from(new TextEncoder().encode(s));

  function sum16(bytes, init) {
    let s = init || 0;
    for (let i = 0; i < bytes.length; i += 2) s += (bytes[i] << 8) + (i + 1 < bytes.length ? bytes[i + 1] : 0);
    while (s >>> 16) s = (s & 0xffff) + (s >>> 16);
    return s;
  }
  const csum = (bytes, init) => (~sum16(bytes, init)) & 0xffff;
  function pseudo(src, dst, proto, len) { return sum16([...ipB(src), ...ipB(dst), 0, proto, ...p16(len)]); }

  /* ---------------- builders ---------------- */
  function eth(dst, src, type, payload) {
    const f = [...macB(dst), ...macB(src), ...p16(type), ...payload];
    while (f.length < 60) f.push(0); // Ethernet minimum frame (without FCS) -> trailing padding
    return Uint8Array.from(f);
  }
  function ipv4(src, dst, proto, id, payload, ttl) {
    const hdr = [0x45, 0x00, ...p16(20 + payload.length), ...p16(id & 0xffff), 0x40, 0x00, ttl || 64, proto, 0, 0, ...ipB(src), ...ipB(dst)];
    const c = csum(hdr); hdr[10] = c >> 8; hdr[11] = c & 255;
    return [...hdr, ...payload];
  }
  function udp(src, dst, sp, dp, payload) {
    const len = 8 + payload.length;
    const seg = [...p16(sp), ...p16(dp), ...p16(len), 0, 0, ...payload];
    let c = csum(seg, pseudo(src, dst, 17, len)); if (c === 0) c = 0xffff;
    seg[6] = c >> 8; seg[7] = c & 255;
    return seg;
  }
  const F = { FIN: 1, SYN: 2, RST: 4, PSH: 8, ACK: 16 };
  function tcp(src, dst, sp, dp, seq, ack, flags, win, payload) {
    const seg = [...p16(sp), ...p16(dp), ...p32(seq), ...p32(ack), 0x50, flags, ...p16(win), 0, 0, 0, 0, ...payload];
    const c = csum(seg, pseudo(src, dst, 6, seg.length)); seg[16] = c >> 8; seg[17] = c & 255;
    return seg;
  }
  function arp(op, sha, spa, tha, tpa) { return [0, 1, 8, 0, 6, 4, 0, op, ...macB(sha), ...ipB(spa), ...macB(tha), ...ipB(tpa)]; }
  function doip(type, payload) { return [0x02, 0xfd, ...p16(type), ...p32(payload.length), ...payload]; }
  function someip(svc, method, client, session, ifv, mtype, rc, payload) {
    return [...p16(svc), ...p16(method), ...p32(8 + payload.length), ...p16(client), ...p16(session), 0x01, ifv, mtype, rc, ...payload];
  }
  function someipSdOffer(session, svc, inst, major, ttl, minor, epIp, epPort) {
    const entry = [0x01, 0x00, 0x00, 0x10, ...p16(svc), ...p16(inst), major, (ttl >> 16) & 255, (ttl >> 8) & 255, ttl & 255, ...p32(minor)];
    const opt = [0x00, 0x09, 0x04, 0x00, ...ipB(epIp), 0x00, 0x11, ...p16(epPort)];
    const sd = [0xc0, 0, 0, 0, ...p32(entry.length), ...entry, ...p32(opt.length), ...opt];
    return someip(0xffff, 0x8100, 0x0000, session, 0x01, 0x02, 0x00, sd);
  }
  // convenience: full frames
  function udpFrame(a, b, sp, dp, payload, id) {
    return eth(b.mac, a.mac, 0x0800, ipv4(a.ip, b.ip, 17, id, udp(a.ip, b.ip, sp, dp, payload)));
  }
  function tcpFrame(a, b, sp, dp, seq, ack, flags, payload, id, win) {
    return eth(b.mac, a.mac, 0x0800, ipv4(a.ip, b.ip, 6, id, tcp(a.ip, b.ip, sp, dp, seq, ack, flags, win || 64240, payload)));
  }
  function arpFrame(op, a, b) {
    return op === 1 ? eth('ff:ff:ff:ff:ff:ff', a.mac, 0x0806, arp(1, a.mac, a.ip, '00:00:00:00:00:00', b.ip))
      : eth(b.mac, a.mac, 0x0806, arp(2, a.mac, a.ip, b.mac, b.ip));
  }

  /* ---------------- decoder ---------------- */
  const DOIP_T = {
    0x0000: 'Generic DoIP header NACK', 0x0001: 'Vehicle identification request', 0x0004: 'Vehicle announcement message/vehicle identification response',
    0x0005: 'Routing activation request', 0x0006: 'Routing activation response', 0x0007: 'Alive check request', 0x0008: 'Alive check response',
    0x8001: 'Diagnostic message', 0x8002: 'Diagnostic message ACK', 0x8003: 'Diagnostic message NACK'
  };
  const UDS_SID = { 0x10: 'DiagnosticSessionControl', 0x11: 'ECUReset', 0x22: 'ReadDataByIdentifier', 0x27: 'SecurityAccess', 0x2e: 'WriteDataByIdentifier', 0x31: 'RoutineControl', 0x3e: 'TesterPresent', 0x19: 'ReadDTCInformation', 0x7f: 'Negative Response' };
  const SOMEIP_MT = { 0x00: 'Request', 0x01: 'Request no Return', 0x02: 'Notification', 0x80: 'Response', 0x81: 'Error' };
  const isSomeipPort = p => p === 30490 || p === 30501 || p === 30509;

  function node(l, o, n, c, cls) { return { l, o, n, c: c || [], cls }; }
  function bits(v, mask, width, label) {
    let s = ''; for (let i = width - 1; i >= 0; i--) { s += (mask >> i) & 1 ? ((v >> i) & 1 ? '1' : '0') : '.'; if (i % 4 === 0 && i) s += ' '; }
    return s + ' = ' + label;
  }
  function add(f, k, v) { (f[k] || (f[k] = [])).push(v); }

  // ctx: {no, t, chAlias, iface, tcpIsn: Map key->isn, stream: fn(proto,key)->idx}
  function decode(b, ctx) {
    ctx = ctx || {};
    const L = b.length, f = {}, tree = [];
    const sum = { src: '', dst: '', proto: 'Ethernet', info: '', cls: '' };
    add(f, 'frame', 1); add(f, 'frame.len', L); if (ctx.no) add(f, 'frame.number', ctx.no);
    if (ctx.iface) add(f, 'frame.interface_name', ctx.iface); if (ctx.chAlias) add(f, 'loom.ch', ctx.chAlias);
    const fr = node(`Frame ${ctx.no || '?'}: ${L} bytes on wire (${L * 8} bits), ${L} bytes captured (${L * 8} bits) on interface ${ctx.iface || '?'}`, 0, L, [
      node(`Interface: ${ctx.iface || '?'} (Loom channel "${ctx.chAlias || '?'}", SIM)`, -1, 0),
      node(`Time since start of capture: ${(ctx.t != null ? (ctx.t / 1000).toFixed(6) : '?')} seconds`, -1, 0),
      node(`Frame Number: ${ctx.no || '?'}`, -1, 0),
      node(`Frame Length: ${L} bytes (${L * 8} bits)`, -1, 0)
    ], 'proto meta');
    tree.push(fr);
    if (L < 14) { sum.info = 'Malformed (short frame)'; return { sum, f, tree }; }
    const dst = macS(b, 0), src = macS(b, 6), et = u16(b, 12);
    add(f, 'eth', 1); add(f, 'eth.dst', dst); add(f, 'eth.src', src); add(f, 'eth.addr', dst); add(f, 'eth.addr', src); add(f, 'eth.type', et);
    const ethN = node(`Ethernet II, Src: ${src}, Dst: ${dst}`, 0, 14, [
      node(`Destination: ${dst}${dst === 'ff:ff:ff:ff:ff:ff' ? ' (Broadcast)' : ''}`, 0, 6),
      node(`Source: ${src}`, 6, 6),
      node(`Type: ${et === 0x0800 ? 'IPv4' : et === 0x0806 ? 'ARP' : 'Unknown'} (${hx(et, 4)})`, 12, 2)
    ], 'proto');
    tree.push(ethN);
    sum.src = src; sum.dst = dst;
    let end = L; // end of L3 payload (excludes padding)
    if (et === 0x0806 && L >= 42) {
      const op = u16(b, 20), sha = macS(b, 22), spa = ipS(b, 28), tha = macS(b, 32), tpa = ipS(b, 38);
      add(f, 'arp', 1); add(f, 'arp.opcode', op); add(f, 'arp.src.proto_ipv4', spa); add(f, 'arp.dst.proto_ipv4', tpa);
      tree.push(node(`Address Resolution Protocol (${op === 1 ? 'request' : 'reply'})`, 14, 28, [
        node('Hardware type: Ethernet (1)', 14, 2), node('Protocol type: IPv4 (0x0800)', 16, 2),
        node('Hardware size: 6', 18, 1), node('Protocol size: 4', 19, 1),
        node(`Opcode: ${op === 1 ? 'request' : 'reply'} (${op})`, 20, 2),
        node(`Sender MAC address: ${sha}`, 22, 6), node(`Sender IP address: ${spa}`, 28, 4),
        node(`Target MAC address: ${tha}`, 32, 6), node(`Target IP address: ${tpa}`, 38, 4)
      ], 'proto'));
      sum.proto = 'ARP'; sum.cls = 'p-arp';
      sum.info = op === 1 ? `Who has ${tpa}? Tell ${spa}` : `${spa} is at ${sha}`;
      end = 42;
    } else if (et === 0x0800 && L >= 34) {
      const ihl = (b[14] & 15) * 4, tot = u16(b, 16), id = u16(b, 18), ttl = b[22], pr = b[23], ck = u16(b, 24);
      const s = ipS(b, 26), d = ipS(b, 30);
      const hdr = Array.from(b.slice(14, 14 + ihl)); hdr[10] = 0; hdr[11] = 0;
      const ckOk = csum(hdr) === ck;
      add(f, 'ip', 1); add(f, 'ip.src', s); add(f, 'ip.dst', d); add(f, 'ip.addr', s); add(f, 'ip.addr', d);
      add(f, 'ip.proto', pr); add(f, 'ip.ttl', ttl); add(f, 'ip.id', id); add(f, 'ip.len', tot);
      const prn = pr === 6 ? 'TCP' : pr === 17 ? 'UDP' : 'Unknown';
      tree.push(node(`Internet Protocol Version 4, Src: ${s}, Dst: ${d}`, 14, ihl, [
        node(bits(b[14], 0xf0, 8, 'Version: 4'), 14, 1),
        node(bits(b[14], 0x0f, 8, `Header Length: ${ihl} bytes (${ihl / 4})`), 14, 1),
        node(`Differentiated Services Field: ${hx(b[15], 2)} (DSCP: CS0, ECN: Not-ECT)`, 15, 1),
        node(`Total Length: ${tot}`, 16, 2),
        node(`Identification: ${hx(id, 4)} (${id})`, 18, 2),
        node(bits(b[20], 0xe0, 8, `Flags: ${hx(b[20] >> 5, 1)}${b[20] & 0x40 ? ", Don't fragment" : ''}`), 20, 1),
        node(`...0 0000 0000 0000 = Fragment Offset: 0`, 20, 2),
        node(`Time to Live: ${ttl}`, 22, 1),
        node(`Protocol: ${prn} (${pr})`, 23, 1),
        node(`Header Checksum: ${hx(ck, 4)} [${ckOk ? 'correct' : 'incorrect'}]`, 24, 2, null, ckOk ? 'good' : 'bad'),
        node(`Source Address: ${s}`, 26, 4), node(`Destination Address: ${d}`, 30, 4)
      ], 'proto'));
      sum.src = s; sum.dst = d;
      const l4 = 14 + ihl; end = Math.min(L, 14 + tot);
      const segLen = end - l4;
      if (pr === 17 && segLen >= 8) {
        const sp = u16(b, l4), dp = u16(b, l4 + 2), ul = u16(b, l4 + 4), uc = u16(b, l4 + 6);
        const seg = Array.from(b.slice(l4, l4 + ul)); seg[6] = 0; seg[7] = 0;
        let calc = csum(seg, pseudo(s, d, 17, ul)); if (calc === 0) calc = 0xffff;
        const key = convKey(s, sp, d, dp), si = ctx.stream ? ctx.stream('udp', key) : 0;
        add(f, 'udp', 1); add(f, 'udp.srcport', sp); add(f, 'udp.dstport', dp); add(f, 'udp.port', sp); add(f, 'udp.port', dp);
        add(f, 'udp.length', ul); add(f, 'udp.stream', si);
        const pl = ul - 8, po = l4 + 8;
        tree.push(node(`User Datagram Protocol, Src Port: ${sp}, Dst Port: ${dp}`, l4, 8, [
          node(`Source Port: ${sp}`, l4, 2), node(`Destination Port: ${dp}`, l4 + 2, 2), node(`Length: ${ul}`, l4 + 4, 2),
          node(`Checksum: ${hx(uc, 4)} [${calc === uc ? 'correct' : 'incorrect'}]`, l4 + 6, 2, null, calc === uc ? 'good' : 'bad'),
          node(`[Stream index: ${si}]`, -1, 0, null, 'meta'),
          node(`UDP payload (${pl} bytes)`, po, pl)
        ], 'proto'));
        sum.proto = 'UDP'; sum.cls = 'p-udp';
        sum.info = `${sp} → ${dp} Len=${pl}`;
        sum.conv = { p: 'udp', idx: si, key, a: `${s}:${sp}`, b: `${d}:${dp}` };
        if (pl > 0) app(b, po, pl, sp, dp, f, tree, sum);
      } else if (pr === 6 && segLen >= 20) {
        const sp = u16(b, l4), dp = u16(b, l4 + 2), seq = u32(b, l4 + 4), ack = u32(b, l4 + 8);
        const doff = (b[l4 + 12] >> 4) * 4, fl = b[l4 + 13], win = u16(b, l4 + 14), tc = u16(b, l4 + 16);
        const seg = Array.from(b.slice(l4, end)); seg[16] = 0; seg[17] = 0;
        const calc = csum(seg, pseudo(s, d, 6, seg.length));
        const key = convKey(s, sp, d, dp), si = ctx.stream ? ctx.stream('tcp', key) : 0;
        const isn = ctx.tcpIsn || {};
        const myIsn = isn[`${s}:${sp}>${d}:${dp}`], peerIsn = isn[`${d}:${dp}>${s}:${sp}`];
        const rseq = myIsn != null ? (seq - myIsn) >>> 0 : seq, rack = (fl & F.ACK) && peerIsn != null ? (ack - peerIsn) >>> 0 : ack;
        const pl = end - l4 - doff, po = l4 + doff;
        const fn = []; if (fl & F.FIN) fn.push('FIN'); if (fl & F.SYN) fn.push('SYN'); if (fl & F.RST) fn.push('RST'); if (fl & F.PSH) fn.push('PSH'); if (fl & F.ACK) fn.push('ACK');
        add(f, 'tcp', 1); add(f, 'tcp.srcport', sp); add(f, 'tcp.dstport', dp); add(f, 'tcp.port', sp); add(f, 'tcp.port', dp);
        add(f, 'tcp.stream', si); add(f, 'tcp.len', pl); add(f, 'tcp.seq', rseq); add(f, 'tcp.ack', rack);
        ['fin', 'syn', 'reset', 'push', 'ack'].forEach((k, i) => add(f, 'tcp.flags.' + k, (fl >> i) & 1));
        const flagKids = [
          node(bits(fl, 0x10, 8, `Acknowledgment: ${fl & F.ACK ? 'Set' : 'Not set'}`), l4 + 13, 1),
          node(bits(fl, 0x08, 8, `Push: ${fl & F.PSH ? 'Set' : 'Not set'}`), l4 + 13, 1),
          node(bits(fl, 0x04, 8, `Reset: ${fl & F.RST ? 'Set' : 'Not set'}`), l4 + 13, 1),
          node(bits(fl, 0x02, 8, `Syn: ${fl & F.SYN ? 'Set' : 'Not set'}`), l4 + 13, 1),
          node(bits(fl, 0x01, 8, `Fin: ${fl & F.FIN ? 'Set' : 'Not set'}`), l4 + 13, 1)
        ];
        tree.push(node(`Transmission Control Protocol, Src Port: ${sp}, Dst Port: ${dp}, Seq: ${rseq}${fl & F.ACK ? ', Ack: ' + rack : ''}, Len: ${pl}`, l4, doff, [
          node(`Source Port: ${sp}`, l4, 2), node(`Destination Port: ${dp}`, l4 + 2, 2),
          node(`[Stream index: ${si}]`, -1, 0, null, 'meta'), node(`[TCP Segment Len: ${pl}]`, -1, 0, null, 'meta'),
          node(`Sequence Number: ${rseq}    ${myIsn != null ? '(relative sequence number)' : ''}`, l4 + 4, 4),
          node(`Sequence Number (raw): ${seq}`, l4 + 4, 4),
          node(`Acknowledgment Number: ${fl & F.ACK ? rack + (peerIsn != null ? '    (relative ack number)' : '') : 0}`, l4 + 8, 4),
          node(`Acknowledgment number (raw): ${ack}`, l4 + 8, 4),
          node(bits(b[l4 + 12], 0xf0, 8, `Header Length: ${doff} bytes (${doff / 4})`), l4 + 12, 1),
          node(`Flags: ${hx(fl, 3)} (${fn.join(', ')})`, l4 + 12, 2, flagKids),
          node(`Window: ${win}`, l4 + 14, 2),
          node(`Checksum: ${hx(tc, 4)} [${calc === tc ? 'correct' : 'incorrect'}]`, l4 + 16, 2, null, calc === tc ? 'good' : 'bad'),
          node(`Urgent Pointer: 0`, l4 + 18, 2),
          ...(pl > 0 ? [node(`TCP payload (${pl} bytes)`, po, pl)] : [])
        ], 'proto'));
        sum.proto = 'TCP'; sum.cls = fl & F.RST ? 'p-rst' : (fl & (F.SYN | F.FIN)) ? 'p-synfin' : 'p-tcp';
        sum.info = `${sp} → ${dp} [${fn.join(', ')}] Seq=${rseq}${fl & F.ACK ? ' Ack=' + rack : ''} Win=${win} Len=${pl}`;
        sum.conv = { p: 'tcp', idx: si, key, a: `${s}:${sp}`, b: `${d}:${dp}` };
        if (pl > 0) app(b, po, pl, sp, dp, f, tree, sum);
      }
    }
    if (end < L) {
      const pad = L - end;
      ethN.c.push(node(`Padding: ${Array.from(b.slice(end, L)).map(h2).join('')}`, end, pad));
      ethN.n = 14; // header only; padding child points at trailer bytes
    }
    return { sum, f, tree };
  }
  function convKey(s, sp, d, dp) { const a = `${s}:${sp}`, c = `${d}:${dp}`; return a < c ? `${a}|${c}` : `${c}|${a}`; }

  function app(b, o, n, sp, dp, f, tree, sum) {
    if ((sp === 13400 || dp === 13400) && n >= 8 && b[o] === 0x02 && b[o + 1] === 0xfd) return decDoip(b, o, n, f, tree, sum);
    if ((isSomeipPort(sp) || isSomeipPort(dp)) && n >= 16) return decSomeip(b, o, n, f, tree, sum);
    add(f, 'data', 1); add(f, 'data.len', n);
    const raw = Array.from(b.slice(o, o + n));
    tree.push(node(`Data (${n} bytes)`, o, n, [node(`Data: ${raw.slice(0, 48).map(h2).join('')}${n > 48 ? '…' : ''}`, o, n), node(`[Length: ${n}]`, -1, 0, null, 'meta')], 'proto'));
    const printable = raw.every(c => (c >= 32 && c < 127) || c === 10 || c === 13 || c === 9);
    if (printable) sum.info += `  "${String.fromCharCode(...raw).replace(/\r/g, '\\r').replace(/\n/g, '\\n').slice(0, 60)}"`;
  }
  function decDoip(b, o, n, f, tree, sum) {
    let off = o; const msgs = [];
    while (off + 8 <= o + n) {
      const ty = u16(b, off + 2), ln = u32(b, off + 4), body = off + 8, bl = Math.min(ln, o + n - body);
      const tn = DOIP_T[ty] || 'Unknown';
      add(f, 'doip', 1); add(f, 'doip.type', ty); add(f, 'doip.length', ln);
      const kids = [
        node(`Version: DoIP ISO 13400-2:2012 (${hx(b[off], 2)})`, off, 1), node(`Inverse version: ${hx(b[off + 1], 2)}`, off + 1, 1),
        node(`Type: ${tn} (${hx(ty, 4)})`, off + 2, 2), node(`Length: ${ln}`, off + 4, 4)
      ];
      let info = tn;
      if (ty === 0x0004 && bl >= 32) {
        const vin = String.fromCharCode(...b.slice(body, body + 17)), la = u16(b, body + 17);
        add(f, 'doip.logical_address', la);
        kids.push(node(`VIN: ${vin}`, body, 17), node(`Logical Address: ${hx(la, 4)}`, body + 17, 2),
          node(`EID: ${macS(b, body + 19)}`, body + 19, 6), node(`GID: ${macS(b, body + 25)}`, body + 25, 6), node(`Further action required: No further action required (${hx(b[body + 31], 2)})`, body + 31, 1));
        if (bl >= 33) kids.push(node(`VIN/GID sync. status: VIN and/or GID are synchronized (${hx(b[body + 32], 2)})`, body + 32, 1));
        info += `, VIN ${vin}, LA ${hx(la, 4)}`;
      } else if (ty === 0x0005 && bl >= 7) {
        const sa = u16(b, body); add(f, 'doip.source_address', sa);
        kids.push(node(`Source address: ${hx(sa, 4)}`, body, 2), node(`Activation type: Default (${hx(b[body + 2], 2)})`, body + 2, 1), node(`Reserved by ISO: ${Array.from(b.slice(body + 3, body + 7)).map(h2).join('')}`, body + 3, 4));
        info += ` SA=${hx(sa, 4)}`;
      } else if (ty === 0x0006 && bl >= 9) {
        const lt = u16(b, body), sa = u16(b, body + 2), rc = b[body + 4]; add(f, 'doip.source_address', sa); add(f, 'doip.response_code', rc);
        kids.push(node(`Logical address tester: ${hx(lt, 4)}`, body, 2), node(`Source address: ${hx(sa, 4)}`, body + 2, 2),
          node(`Routing activation response code: ${rc === 0x10 ? 'Routing successfully activated' : 'Denied'} (${hx(rc, 2)})`, body + 4, 1), node(`Reserved by ISO: ${Array.from(b.slice(body + 5, body + 9)).map(h2).join('')}`, body + 5, 4));
        info += rc === 0x10 ? ' (success)' : ` (code ${hx(rc, 2)})`;
      } else if (ty === 0x0000 && bl >= 1) {
        kids.push(node(`Header NACK code: ${b[body] === 0 ? 'Incorrect pattern format' : 'Unknown payload type'} (${hx(b[body], 2)})`, body, 1));
      } else if ((ty === 0x8001 || ty === 0x8002 || ty === 0x8003) && bl >= 4) {
        const sa = u16(b, body), ta = u16(b, body + 2);
        add(f, 'doip.source_address', sa); add(f, 'doip.target_address', ta);
        kids.push(node(`Source address: ${hx(sa, 4)}`, body, 2), node(`Target address: ${hx(ta, 4)}`, body + 2, 2));
        info += ` ${hx(sa, 4)} → ${hx(ta, 4)}`;
        if (ty === 0x8001 && bl > 4) {
          const uo = body + 4, ul = bl - 4, sid = b[uo];
          const base = sid === 0x7f ? 0x7f : (sid & 0x40 && UDS_SID[sid - 0x40] ? sid - 0x40 : sid);
          const sname = UDS_SID[base] || 'Unknown';
          const resp = sid !== 0x7f && base !== sid;
          add(f, 'uds', 1); add(f, 'uds.sid', sid);
          const uk = [node(`Service Identifier: ${sname}${sid === 0x7f ? '' : resp ? ' Response' : ' Request'} (${hx(sid, 2)})`, uo, 1)];
          if (sid === 0x7f && ul >= 3) uk.push(node(`Request service: ${UDS_SID[b[uo + 1]] || '?'} (${hx(b[uo + 1], 2)})`, uo + 1, 1), node(`NRC: ${hx(b[uo + 2], 2)}`, uo + 2, 1));
          else if ((base === 0x22 || base === 0x2e) && ul >= 3) { uk.push(node(`Data Identifier: ${hx(u16(b, uo + 1), 4)}`, uo + 1, 2)); if (ul > 3) uk.push(node(`Data: ${Array.from(b.slice(uo + 3, uo + ul)).map(h2).join('')}${resp ? '  "' + String.fromCharCode(...b.slice(uo + 3, uo + ul)).replace(/[^\x20-\x7e]/g, '.') + '"' : ''}`, uo + 3, ul - 3)); }
          else if (ul > 1) uk.push(node(`Data: ${Array.from(b.slice(uo + 1, uo + ul)).map(h2).join('')}`, uo + 1, ul - 1));
          kids.push(node(`Unified Diagnostic Services (${ul} bytes)`, uo, ul, uk, 'proto'));
          info += ` UDS ${sname}${sid === 0x7f ? '' : resp ? ' Rsp' : ' Req'} ${Array.from(b.slice(uo, uo + Math.min(ul, 8))).map(h2).join(' ')}${ul > 8 ? '…' : ''}`;
        } else if (ty === 0x8002 && bl >= 5) {
          kids.push(node(`ACK code: Routing confirmation ACK (${hx(b[body + 4], 2)})`, body + 4, 1));
          if (bl > 5) kids.push(node(`Previous diagnostic message data: ${Array.from(b.slice(body + 5, body + bl)).map(h2).join('')}`, body + 5, bl - 5));
        }
      }
      tree.push(node(`DoIP (ISO13400) Protocol, ${tn}`, off, 8 + bl, kids, 'proto'));
      msgs.push(info);
      off = body + bl;
      if (bl < ln) break;
    }
    sum.proto = 'DoIP'; sum.cls = 'p-doip'; sum.info = msgs.join('; ');
  }
  function decSomeip(b, o, n, f, tree, sum) {
    const svc = u16(b, o), mid = u16(b, o + 2), ln = u32(b, o + 4), cl = u16(b, o + 8), se = u16(b, o + 10);
    const pv = b[o + 12], iv = b[o + 13], mt = b[o + 14], rc = b[o + 15];
    add(f, 'someip', 1); add(f, 'someip.serviceid', svc); add(f, 'someip.methodid', mid); add(f, 'someip.messagetype', mt);
    add(f, 'someip.clientid', cl); add(f, 'someip.sessionid', se);
    const pl = Math.min(n - 16, ln - 8), po = o + 16;
    const kids = [
      node(`Message ID: ${hx((svc << 16 >>> 0) + mid, 8)}`, o, 4, [node(`Service ID: ${hx(svc, 4)}`, o, 2), node(`Method ID: ${hx(mid, 4)}`, o + 2, 2)]),
      node(`Length: ${ln}`, o + 4, 4),
      node(`Request ID: ${hx((cl << 16 >>> 0) + se, 8)}`, o + 8, 4, [node(`Client ID: ${hx(cl, 4)}`, o + 8, 2), node(`Session ID: ${hx(se, 4)}`, o + 10, 2)]),
      node(`Protocol Version: ${pv}`, o + 12, 1), node(`Interface Version: ${iv}`, o + 13, 1),
      node(`Message Type: ${SOMEIP_MT[mt] || 'Unknown'} (${hx(mt, 2)})`, o + 14, 1), node(`Return Code: ${rc === 0 ? 'Ok' : 'Not Ok'} (${hx(rc, 2)})`, o + 15, 1)
    ];
    if (svc === 0xffff && mid === 0x8100 && pl >= 12) {
      add(f, 'someipsd', 1);
      const sd = po, el = u32(b, sd + 4), eo = sd + 8;
      const sk = [node(`Flags: ${hx(b[sd], 2)} (Reboot, Unicast)`, sd, 1), node('Reserved: 0x000000', sd + 1, 3), node(`Length of Entries Array: ${el}`, sd + 4, 4)];
      let svcList = [];
      for (let e = eo; e + 16 <= eo + el; e += 16) {
        const es = u16(b, e + 4), ei = u16(b, e + 6), maj = b[e + 8], ttl = (b[e + 9] << 16) | (b[e + 10] << 8) | b[e + 11], min = u32(b, e + 12);
        add(f, 'someipsd.entry.serviceid', es); svcList.push(`${hx(es, 4)}.${hx(ei, 4)} v${maj}.${min}`);
        sk.push(node(`Service Entry: type ${b[e] === 1 ? 'Offer Service' : 'Find Service'}, Service ${hx(es, 4)}, Instance ${hx(ei, 4)}, Version ${maj}.${min}, TTL ${ttl}`, e, 16, [
          node(`Type: ${b[e] === 1 ? 'Offer Service' : 'Find Service'} (${hx(b[e], 2)})`, e, 1), node(`Index 1: ${b[e + 1]}`, e + 1, 1), node(`Index 2: ${b[e + 2]}`, e + 2, 1),
          node(`Number of options: ${b[e + 3] >> 4}/${b[e + 3] & 15}`, e + 3, 1), node(`Service ID: ${hx(es, 4)}`, e + 4, 2), node(`Instance ID: ${hx(ei, 4)}`, e + 6, 2),
          node(`Major Version: ${maj}`, e + 8, 1), node(`TTL: ${ttl}`, e + 9, 3), node(`Minor Version: ${min}`, e + 12, 4)
        ]));
      }
      const ol = eo + el; const olen = u32(b, ol);
      sk.push(node(`Length of Options Array: ${olen}`, ol, 4));
      if (olen >= 12) {
        const op = ol + 4, ip = ipS(b, op + 4), port = u16(b, op + 10);
        sk.push(node(`IPv4 Endpoint Option: ${ip}:${port} (UDP)`, op, 12, [node(`Length: ${u16(b, op)}`, op, 2), node('Type: IPv4 Endpoint (0x04)', op + 2, 1), node(`IPv4 Address: ${ip}`, op + 4, 4), node('L4 Protocol: UDP (0x11)', op + 9, 1), node(`Port: ${port}`, op + 10, 2)]));
      }
      kids.push(node(`SOME/IP Service Discovery Protocol`, sd, pl, sk, 'proto'));
      sum.proto = 'SOME/IP-SD'; sum.cls = 'p-someip'; sum.info = `SOME/IP-SD Offer ${svcList.join(', ')}`;
    } else {
      if (pl > 0) kids.push(node(`Payload: ${Array.from(b.slice(po, po + pl)).map(h2).join('')}`, po, pl));
      sum.proto = 'SOME/IP'; sum.cls = 'p-someip';
      sum.info = `${SOMEIP_MT[mt] || 'Unknown'} svc ${hx(svc, 4)} method ${hx(mid, 4)} session ${hx(se, 4)}${pl ? ' len ' + pl : ''}${rc ? ' rc ' + hx(rc, 2) : ''}`;
    }
    tree.push(node(`SOME/IP Protocol (Service ID: ${hx(svc, 4)}, Method ID: ${hx(mid, 4)}, Length: ${ln})`, o, 16 + Math.max(0, pl), kids, 'proto'));
  }

  /* ---------------- display filter (documented subset) ---------------- */
  const FIELDS = {
    frame: 'p', 'frame.len': 'n', 'frame.number': 'n', 'frame.interface_name': 's', 'loom.ch': 's',
    eth: 'p', 'eth.addr': 'm', 'eth.src': 'm', 'eth.dst': 'm', 'eth.type': 'n',
    arp: 'p', 'arp.opcode': 'n', 'arp.src.proto_ipv4': 'i', 'arp.dst.proto_ipv4': 'i',
    ip: 'p', 'ip.addr': 'i', 'ip.src': 'i', 'ip.dst': 'i', 'ip.proto': 'n', 'ip.ttl': 'n', 'ip.id': 'n', 'ip.len': 'n',
    udp: 'p', 'udp.port': 'n', 'udp.srcport': 'n', 'udp.dstport': 'n', 'udp.length': 'n', 'udp.stream': 'n',
    tcp: 'p', 'tcp.port': 'n', 'tcp.srcport': 'n', 'tcp.dstport': 'n', 'tcp.stream': 'n', 'tcp.len': 'n', 'tcp.seq': 'n', 'tcp.ack': 'n',
    'tcp.flags.syn': 'n', 'tcp.flags.ack': 'n', 'tcp.flags.fin': 'n', 'tcp.flags.reset': 'n', 'tcp.flags.push': 'n',
    doip: 'p', 'doip.type': 'n', 'doip.length': 'n', 'doip.source_address': 'n', 'doip.target_address': 'n', 'doip.logical_address': 'n', 'doip.response_code': 'n',
    uds: 'p', 'uds.sid': 'n',
    someip: 'p', 'someip.serviceid': 'n', 'someip.methodid': 'n', 'someip.messagetype': 'n', 'someip.clientid': 'n', 'someip.sessionid': 'n',
    someipsd: 'p', 'someipsd.entry.serviceid': 'n',
    data: 'p', 'data.len': 'n'
  };
  const OPS = { '==': '==', eq: '==', '!=': '!=', ne: '!=', '>': '>', gt: '>', '<': '<', lt: '<', '>=': '>=', ge: '>=', '<=': '<=', le: '<=', contains: 'contains' };
  function lex(s) {
    const out = []; let i = 0;
    const re = [
      ['ws', /^\s+/], ['str', /^"((?:[^"\\]|\\.)*)"/], ['op', /^(==|!=|>=|<=|&&|\|\||[<>!()])/],
      ['mac', /^[0-9a-fA-F]{2}(?:[:-][0-9a-fA-F]{2}){5}(?![\w:.-])/], ['ip', /^\d{1,3}(?:\.\d{1,3}){3}(?:\/\d{1,2})?(?![\w.])/],
      ['num', /^(?:0x[0-9a-fA-F]+|\d+)(?![\w.:])/], ['id', /^[A-Za-z_][\w.-]*/]
    ];
    while (i < s.length) {
      const rest = s.slice(i); let m = null, k;
      for (const [kk, r] of re) { m = rest.match(r); if (m) { k = kk; break; } }
      if (!m) return { err: `Unexpected character "${rest[0]}" at position ${i + 1}.`, at: i };
      if (k !== 'ws') out.push({ k, v: k === 'str' ? m[1].replace(/\\(.)/g, '$1') : m[0], at: i });
      i += m[0].length;
    }
    return { toks: out };
  }
  function parseDF(src) {
    const s = (src || '').trim();
    if (!s) return { ok: true, empty: true, fn: () => true };
    const lx = lex(s); if (lx.err) return { ok: false, err: lx.err, at: lx.at };
    const t = lx.toks; let p = 0;
    const peek = () => t[p], eat = () => t[p++];
    const isKw = (tk, ...w) => tk && (tk.k === 'op' || tk.k === 'id') && w.includes(tk.v.toLowerCase ? tk.v.toLowerCase() : tk.v);
    function err(m, tk) { const e = new Error(m); e.at = tk ? tk.at : s.length; throw e; }
    function orE() { let l = andE(); while (isKw(peek(), '||', 'or')) { eat(); const r = andE(), a = l; l = x => a(x) || r(x); } return l; }
    function andE() { let l = notE(); while (isKw(peek(), '&&', 'and')) { eat(); const r = notE(), a = l; l = x => a(x) && r(x); } return l; }
    function notE() { if (isKw(peek(), '!', 'not')) { eat(); const e = notE(); return x => !e(x); } return prim(); }
    function prim() {
      const tk = eat();
      if (!tk) err('Unexpected end of filter string.');
      if (tk.k === 'op' && tk.v === '(') { const e = orE(); const c = eat(); if (!c || c.v !== ')') err('Missing ")".', c); return e; }
      if (tk.k !== 'id') err(`"${tk.v}" was unexpected in this context.`, tk);
      const name = tk.v.toLowerCase(), ty = FIELDS[name];
      if (!ty) err(`"${tk.v}" is not a valid protocol or protocol field.`, tk);
      const nx = peek();
      const opk = nx && (nx.k === 'op' || nx.k === 'id') ? OPS[nx.v.toLowerCase()] : null;
      if (!opk) return x => !!x[name];
      eat();
      if (ty === 'p') err(`"${tk.v}" is a protocol; it cannot be compared with "${nx.v}".`, nx);
      const vt = eat(); if (!vt) err(`Expected a value after "${nx.v}".`);
      let val, cmp;
      if (ty === 'n') {
        if (vt.k !== 'num') err(`"${vt.v}" is not a valid number for ${name}.`, vt);
        if (opk === 'contains') err(`"contains" cannot be used with numeric field ${name}.`, nx);
        val = parseInt(vt.v, vt.v.startsWith('0x') ? 16 : 10);
        cmp = { '==': a => a === val, '!=': a => a !== val, '>': a => a > val, '<': a => a < val, '>=': a => a >= val, '<=': a => a <= val }[opk];
      } else if (ty === 'i') {
        if (vt.k !== 'ip') err(`"${vt.v}" is not a valid IPv4 address.`, vt);
        const [addr, bitsS] = vt.v.split('/'); const parts = addr.split('.').map(Number);
        if (parts.some(x => x > 255)) err(`"${vt.v}" is not a valid IPv4 address.`, vt);
        const pb = bitsS == null ? 32 : +bitsS; if (pb > 32) err(`"${vt.v}" has an invalid prefix length.`, vt);
        if (!['==', '!='].includes(opk)) err(`Only == and != are supported for address field ${name} in this mockup.`, nx);
        const toN = a => a.split('.').reduce((acc, x) => acc * 256 + (+x), 0);
        const mask = pb === 0 ? 0 : (0xffffffff << (32 - pb)) >>> 0; const base = (toN(addr) & mask) >>> 0;
        const eq = a => ((toN(a) & mask) >>> 0) === base;
        cmp = opk === '==' ? eq : a => !eq(a);
      } else if (ty === 'm') {
        if (vt.k !== 'mac') err(`"${vt.v}" is not a valid Ethernet address.`, vt);
        if (!['==', '!='].includes(opk)) err(`Only == and != are supported for ${name}.`, nx);
        val = vt.v.toLowerCase().replace(/-/g, ':'); cmp = opk === '==' ? a => a === val : a => a !== val;
      } else {
        if (vt.k !== 'str') err(`String value for ${name} must be quoted, e.g. ${name} == "ecu-doip".`, vt);
        val = vt.v;
        if (!['==', '!=', 'contains'].includes(opk)) err(`Only ==, != and contains are supported for ${name}.`, nx);
        cmp = opk === '==' ? a => a === val : opk === '!=' ? a => a !== val : a => String(a).includes(val);
      }
      // Wireshark semantics: "==" any value matches; "!=" all values differ (field must be present)
      if (opk === '!=') return x => !!x[name] && x[name].every(cmp);
      return x => !!x[name] && x[name].some(cmp);
    }
    try {
      const fn = orE();
      if (p < t.length) err(`"${t[p].v}" was unexpected in this context.`, t[p]);
      return { ok: true, fn };
    } catch (e) { return { ok: false, err: e.message, at: e.at }; }
  }

  /* ---------------- capture filter (libpcap/BPF subset) ---------------- */
  function parseCF(src) {
    const s = (src || '').trim();
    if (!s) return { ok: true, empty: true, fn: () => true };
    const t = s.match(/\(|\)|[^\s()]+/g) || []; let p = 0;
    const peek = () => t[p] && t[p].toLowerCase(), eat = () => t[p++];
    function err(m) { throw new Error(m); }
    const isIp = v => /^\d{1,3}(\.\d{1,3}){3}$/.test(v || '') && v.split('.').every(x => +x <= 255);
    const isMac = v => /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/i.test(v || '');
    function orE() { let l = andE(); while (peek() === 'or' || peek() === '||') { eat(); const r = andE(), a = l; l = x => a(x) || r(x); } return l; }
    function andE() { let l = notE(); while (peek() === 'and' || peek() === '&&') { eat(); const r = notE(), a = l; l = x => a(x) && r(x); } return l; }
    function notE() { if (peek() === 'not' || peek() === '!') { eat(); const e = notE(); return x => !e(x); } return prim(); }
    function num(v, what) { if (!/^\d+$/.test(v || '')) err(`expected ${what} number after "${t[p - 2] || ''}"`); return +v; }
    function prim() {
      let w = peek(); if (w == null) err('syntax error: unexpected end of expression');
      if (w === '(') { eat(); const e = orE(); if (peek() !== ')') err('syntax error: missing ")"'); eat(); return e; }
      eat();
      if (w === 'arp') return x => !!x.arp;
      if (w === 'ip') return x => !!x.ip;
      if (w === 'tcp' || w === 'udp') {
        const pr = w;
        if (['port', 'src', 'dst'].includes(peek())) return portE(pr);
        return x => !!x[pr];
      }
      if (w === 'port' || w === 'src' || w === 'dst') { p--; return portE(null); }
      if (w === 'host') { const v = eat(); if (!isIp(v)) err(`illegal host address "${v || ''}"`); return x => !!x['ip.addr'] && x['ip.addr'].includes(v); }
      if (w === 'ether') {
        let dir = null; if (peek() === 'src' || peek() === 'dst') dir = eat().toLowerCase();
        if (peek() !== 'host') err('syntax error: expected "host" after "ether"'); eat();
        const v = (eat() || '').toLowerCase(); if (!isMac(v)) err(`illegal Ethernet address "${v}"`);
        const k = dir ? 'eth.' + dir : 'eth.addr'; return x => !!x[k] && x[k].includes(v);
      }
      if (w === 'less') { const n = num(eat(), 'length'); return x => x['frame.len'][0] <= n; }
      if (w === 'greater') { const n = num(eat(), 'length'); return x => x['frame.len'][0] >= n; }
      err(`syntax error near "${w}" (supported: arp ip tcp udp [tcp|udp] [src|dst] port N, [src|dst] host A, ether [src|dst] host M, less N, greater N, and/or/not, ())`);
    }
    function portE(pr) {
      let dir = null; if (peek() === 'src' || peek() === 'dst') dir = eat().toLowerCase();
      if (dir && peek() === 'host') { eat(); const v = eat(); if (!isIp(v)) err(`illegal host address "${v || ''}"`); return x => !!x['ip.' + dir] && x['ip.' + dir].includes(v); }
      if (peek() !== 'port') err(`syntax error: expected "port"`); eat();
      const n = num(eat(), 'port'); if (n > 65535) err(`illegal port number ${n} > 65535`);
      const prs = pr ? [pr] : ['tcp', 'udp'];
      return x => prs.some(q => { const k = dir ? `${q}.${dir}port` : `${q}.port`; return !!x[k] && x[k].includes(n); });
    }
    try { const fn = orE(); if (p < t.length) err(`syntax error near "${t[p]}"`); return { ok: true, fn }; }
    catch (e) { return { ok: false, err: e.message }; }
  }

  /* ---------------- libpcap (classic .pcap) writer ---------------- */
  function toPcap(pkts, epochMs) {
    let total = 24; pkts.forEach(p => { total += 16 + p.bytes.length; });
    const buf = new Uint8Array(total), dv = new DataView(buf.buffer);
    dv.setUint32(0, 0xa1b2c3d4, true); dv.setUint16(4, 2, true); dv.setUint16(6, 4, true);
    dv.setInt32(8, 0, true); dv.setUint32(12, 0, true); dv.setUint32(16, 65535, true); dv.setUint32(20, 1, true); // LINKTYPE_ETHERNET
    let o = 24;
    pkts.forEach(p => {
      const us = Math.round((epochMs + p.t) * 1000);
      dv.setUint32(o, Math.floor(us / 1e6), true); dv.setUint32(o + 4, us % 1e6, true);
      dv.setUint32(o + 8, p.bytes.length, true); dv.setUint32(o + 12, p.bytes.length, true);
      buf.set(p.bytes, o + 16); o += 16 + p.bytes.length;
    });
    return buf;
  }

  /* ---------------- payload input parsing (Packet Sender style) ---------------- */
  function parseHex(s) {
    const clean = (s || '').replace(/0x/gi, ' ').replace(/[,:;\s]+/g, ' ').trim();
    if (!clean) return { err: '보낼 데이터가 비어 있습니다.' };
    const toks = clean.split(' '); const out = [];
    for (const tk of toks) {
      if (!/^[0-9a-fA-F]+$/.test(tk)) return { err: `HEX가 아닌 문자: "${tk}"` };
      const v = tk.length % 2 ? '0' + tk : tk;
      for (let i = 0; i < v.length; i += 2) out.push(parseInt(v.slice(i, i + 2), 16));
    }
    return { bytes: out };
  }
  function parseText(s) {
    if (!s) return { err: '보낼 데이터가 비어 있습니다.' };
    const out = []; let i = 0, lit = '';
    const flush = () => { if (lit) { out.push(...txt(lit)); lit = ''; } }; // encode whole code points (non-BMP safe)
    while (i < s.length) {
      const c = s[i];
      if (c === '\\') {
        flush();
        const n = s[i + 1];
        if (n === 'r') { out.push(13); i += 2; continue; }
        if (n === 'n') { out.push(10); i += 2; continue; }
        if (n === 't') { out.push(9); i += 2; continue; }
        if (n === '\\') { out.push(92); i += 2; continue; }
        if (n === 'x' && /^[0-9a-fA-F]{2}$/.test(s.slice(i + 2, i + 4))) { out.push(parseInt(s.slice(i + 2, i + 4), 16)); i += 4; continue; }
        return { err: `알 수 없는 이스케이프 "\\${n || ''}" (지원: \\r \\n \\t \\\\ \\xHH)` };
      }
      lit += c; i++;
    }
    flush();
    return { bytes: out };
  }

  root.LoomPkt = {
    h2, h4, hx, macB, ipB, u16, u32, p16, p32, txt, csum, F,
    eth, ipv4, udp, tcp, arp, doip, someip, someipSdOffer, udpFrame, tcpFrame, arpFrame,
    decode, convKey, parseDF, parseCF, toPcap, parseHex, parseText, FIELDS
  };
})(typeof window !== 'undefined' ? window : globalThis);
