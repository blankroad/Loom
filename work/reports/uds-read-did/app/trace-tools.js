/* Loom UDS Read DID — trace tools (worker B): display filters + display pause (item 2), continuous log with
   IndexedDB segments (item 3) and a read-only offline log viewer. LOCAL SIM only.
   Nothing in this file connects, sends, retries or replays: the viewer never touches the engine. */
(function () {
  'use strict';
  const d = document, PKEY = 'loom.udsReadDid.trace.v1';
  const LOG_FORMAT = 'loom-uds-trace-log/1', TRACE1 = 'loom-uds-read-did-trace/1';
  const LIMITS = {
    chunkRecords: 200, flushMs: 1000,                 // buffered records are written as one chunk
    segRecords: 20000, segBytes: 4 * 1024 * 1024,     // rotation
    maxSegments: 24, maxTotalBytes: 32 * 1024 * 1024, // retention (oldest closed segments deleted first)
    memTotalBytes: 8 * 1024 * 1024,                   // memory fallback budget
    importBytes: 16 * 1024 * 1024, importRecords: 200000, str: 400, hexBytes: 4100, seen: 4000,
  };
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const pad = (n, w) => String(n).padStart(w || 2, '0');
  const fmtT = t => { const x = new Date(t); return `${pad(x.getHours())}:${pad(x.getMinutes())}:${pad(x.getSeconds())}.${pad(x.getMilliseconds(), 3)}`; };
  const fmtRel = ms => (ms < 0 ? '-' : '+') + (Math.abs(ms) / 1000).toFixed(3) + ' s';
  const fmtB = n => (n < 1024 ? n + ' B' : n < 1048576 ? (n / 1024).toFixed(1) + ' KB' : (n / 1048576).toFixed(2) + ' MB');
  const u8len = s => { let n = 0; for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); n += c < 0x80 ? 1 : c < 0x800 ? 2 : (c >= 0xD800 && c < 0xDC00) ? (i++, 4) : 3; } return n; };
  const hex2 = n => (n & 0xff).toString(16).toUpperCase().padStart(2, '0');
  const hex4 = n => (n & 0xffff).toString(16).toUpperCase().padStart(4, '0');
  const hexs = b => Array.from(b || [], hex2).join(' ');
  const unhex = s => (s ? s.split(' ').map(x => parseInt(x, 16)) : []);

  /* ================= filters (pure; shared by live view and offline viewer) ================= */
  const STATUS_OPTS = [['', '상태: 전체'], ['active', '진행 중'], ['positive', '긍정'], ['nrc', 'NRC'], ['timeout', '타임아웃'], ['cancelled', '취소'], ['link-lost', '링크 끊김'], ['suppressed', '응답 억제'], ['late', 'late 응답 있음'], ['unapplied', '값 미반영'], ['raw', 'RAW 요청'], ['did', 'DID 요청']];
  const EMPTY_F = () => ({ q: '', sid: '', did: '', nrc: '', status: '', route: '', profile: '' });
  function parseByteF(v, lab) {
    v = String(v || '').trim(); if (!v) return { v: null };
    if (!/^(?:0x)?[0-9a-f]{1,2}$/i.test(v)) return { err: `${lab}: 1–2자리 hex (예: 22, 0x7F)` };
    return { v: parseInt(v.replace(/^0x/i, ''), 16) };
  }
  function parseDidF(v) {
    v = String(v || '').trim(); if (!v) return { v: null };
    if (!/^(?:0x)?[0-9a-f]{4}$/i.test(v)) return { err: 'DID: 4자리 hex (예: F190)' };
    return { v: parseInt(v.replace(/^0x/i, ''), 16) };
  }
  function qHex(q) {
    const s = q.replace(/0x/gi, '').replace(/\s+/g, '');
    if (!s || !/^[0-9a-f]+$/i.test(s) || s.length % 2) return null;
    return s.toUpperCase().match(/../g).join(' ');
  }
  const hayCache = new WeakMap();
  function hay(tx) {
    const k = tx.events.length + '|' + tx.status;
    const c = hayCache.get(tx); if (c && c.k === k) return c;
    const r = tx.route || {}, p = tx.profile || {};
    const txt = ['#' + tx.no, tx.ecu, tx.ecuName, r.label, r.summary, r.ptag, p.name, p.id, tx.didHex, tx.didName, tx.sidName,
      tx.sid != null ? '0x' + hex2(tx.sid) : '', tx.status, tx.kind].concat(tx.events.map(ev => (ev.label || '') + ' ' + (ev.note || ''))).join('\n').toLowerCase();
    const hx = [tx.request, tx.final].concat(tx.events.map(ev => ev.bytes)).filter(Boolean).map(b => ' ' + hexs(b) + ' ').join('|');
    const v = { k, txt, hx }; hayCache.set(tx, v); return v;
  }
  const hasNrc = (tx, n) => tx.nrc === n || tx.events.some(ev => ev.kind !== 'tx' && ev.bytes && ev.bytes.length >= 3 && ev.bytes[0] === 0x7F && ev.bytes[2] === n);
  function statusOk(tx, s) {
    if (s === 'active') return tx.status === 'waiting' || tx.status === 'pending';
    if (s === 'late') return tx.events.some(ev => ev.kind === 'late');
    if (s === 'unapplied') return tx.status === 'positive' && tx.applied === false;
    if (s === 'raw' || s === 'did') return tx.kind === s;
    return tx.status === s;
  }
  function compile(F) {
    F = Object.assign(EMPTY_F(), F || {});
    const errors = {};
    const sid = parseByteF(F.sid, 'SID'), did = parseDidF(F.did), nrc = parseByteF(F.nrc, 'NRC');
    if (sid.err) errors.sid = sid.err; if (did.err) errors.did = did.err; if (nrc.err) errors.nrc = nrc.err;
    const q = String(F.q || '').trim().slice(0, 200), ql = q.toLowerCase(), qh = q ? qHex(q) : null;
    const status = STATUS_OPTS.some(o => o[0] === F.status) ? F.status : '';
    const route = typeof F.route === 'string' ? F.route : '', profile = typeof F.profile === 'string' ? F.profile : '';
    const active = !!(q || sid.v != null || did.v != null || nrc.v != null || status || route || profile);
    const pred = tx => {
      if (sid.v != null && tx.sid !== sid.v) return false;
      if (did.v != null && tx.did !== did.v) return false;
      if (nrc.v != null && !hasNrc(tx, nrc.v)) return false;
      if (route && tx.transport !== route) return false;
      if (profile && (tx.profile || {}).id !== profile) return false;
      if (status && !statusOk(tx, status)) return false;
      if (q) { const h = hay(tx); if (!h.txt.includes(ql) && !(qh && h.hx.includes(' ' + qh + ' '))) return false; }
      return true;
    };
    return { active, errors, invalid: Object.keys(errors).length > 0, pred, qHex: qh };
  }

  /* deep snapshot of a live txn (display freeze; engine object never mutated) */
  function cloneTxn(tx, at) {
    const c = Object.assign({}, tx, {
      route: JSON.parse(JSON.stringify(tx.route || {})), profile: Object.assign({}, tx.profile), timing: Object.assign({}, tx.timing), sim: Object.assign({}, tx.sim),
      request: (tx.request || []).slice(), final: tx.final ? tx.final.slice() : null,
      events: tx.events.map(ev => Object.assign({}, ev, { bytes: ev.bytes ? ev.bytes.slice() : ev.bytes })),
      timers: [], deadline: null, frozenAt: at,
    });
    if (c.tEnd == null) c.tEnd = at; // elapsed of an in-flight row is frozen at the pause instant
    return Object.freeze(c);
  }

  /* ================= log validation / reconstruction (pure) ================= */
  const HEXRE = /^(?:[0-9A-F]{2}(?: [0-9A-F]{2})*)?$/;
  function V(line) {
    const fail = m => { throw new Error(`${line}행: ${m}`); };
    return {
      fail,
      str(o, k, opt) { const v = o[k]; if (v == null || v === '') { if (opt) return ''; fail(`${k} 누락`); } if (typeof v !== 'string') fail(`${k} 문자열 아님`); if (v.length > LIMITS.str) fail(`${k} 길이 ${v.length} > ${LIMITS.str}`); return v; },
      num(o, k, opt) { const v = o[k]; if (v == null) { if (opt) return null; fail(`${k} 누락`); } if (typeof v !== 'number' || !isFinite(v)) fail(`${k} 숫자 아님`); return v; },
      int(o, k, lo, hi, opt) { const v = this.num(o, k, opt); if (v == null) return null; if (!Number.isInteger(v) || v < lo || v > hi) fail(`${k} 범위 밖 (${lo}–${hi})`); return v; },
      hex(o, k, opt) { const v = o[k]; if (v == null || v === '') { if (opt) return null; fail(`${k} 누락`); } if (typeof v !== 'string' || !HEXRE.test(v)) fail(`${k}: 대문자 hex 바이트 형식 아님`); if ((v.length + 1) / 3 > LIMITS.hexBytes) fail(`${k} ${LIMITS.hexBytes} B 초과`); return unhex(v); },
      config(o, k) {
        const c = o[k]; if (c == null) return {};
        if (typeof c !== 'object' || Array.isArray(c)) fail(`${k} 객체 아님`);
        const ks = Object.keys(c); if (ks.length > 24) fail(`${k} 항목 24개 초과`);
        const out = {};
        ks.forEach(x => {
          if (!/^[A-Za-z][A-Za-z0-9_]{0,31}$/.test(x)) fail(`${k}.${esc(x.slice(0, 16))} 키 형식 아님`);
          const val = c[x];
          if (typeof val === 'string') { if (val.length > LIMITS.str) fail(`${k}.${x} 길이 초과`); out[x] = val; }
          else if ((typeof val === 'number' && isFinite(val)) || typeof val === 'boolean' || val === null) out[x] = val;
          else fail(`${k}.${x} 값 형식 아님`);
        });
        return Object.freeze(out);
      },
      fields(o, k) {
        const f = o[k]; if (f == null) return [];
        if (!Array.isArray(f) || f.length > 24) fail(`${k} 배열(≤24) 아님`);
        return Object.freeze(f.map((p, i) => { if (!Array.isArray(p) || p.length !== 2 || p.some(x => typeof x !== 'string' || x.length > LIMITS.str)) fail(`${k}[${i}] [이름, 값] 문자열 쌍 아님`); return Object.freeze([p[0], p[1]]); }));
      },
      // status / event kinds: known values plus any future lower-case token (e.g. 'suppressed'); rendered escaped, as-is.
      token(o, k) { const x = o[k]; if (typeof x !== 'string' || !/^[a-z][a-z0-9-]{0,23}$/.test(x)) fail(`${k} 형식 아님 (소문자 토큰)`); return x; },
      oneOf(o, k, list, opt) { const v = o[k]; if (v == null && opt) return null; if (!list.includes(v)) fail(`${k} 알 수 없는 값`); return v; },
    };
  }
  function checkRecord(o, line) {
    const v = V(line);
    if (!o || typeof o !== 'object' || Array.isArray(o)) v.fail('객체 아님');
    const r = o.r, t = v.num(o, 't');
    switch (r) {
      case 'hdr': if (o.format !== LOG_FORMAT) v.fail(`format이 ${LOG_FORMAT} 아님`); return { r, t, format: LOG_FORMAT, seg: v.str(o, 'seg'), part: v.int(o, 'part', 1, 1e6), session: v.str(o, 'session', true), startedAt: v.str(o, 'startedAt'), note: v.str(o, 'note', true) };
      case 'mark': return { r, t, text: v.str(o, 'text') };
      case 'clear': return { r, t };
      case 'ctx': return { r, t, ecu: v.str(o, 'ecu'), transport: v.str(o, 'transport'), link: v.oneOf(o, 'link', ['online', 'offline']), gen: v.int(o, 'gen', 0, 1e9) };
      case 'tx': {
        const p = o.profile && typeof o.profile === 'object' ? o.profile : v.fail('profile 누락');
        return { r, t, id: v.int(o, 'id', 1, 1e12), no: v.int(o, 'no', 1, 1e12), ecu: v.str(o, 'ecu'), ecuName: v.str(o, 'ecuName', true), transport: v.str(o, 'transport'),
          routeLabel: v.str(o, 'routeLabel'), routeSummary: v.str(o, 'routeSummary', true), ptag: v.str(o, 'ptag', true),
          profile: { id: v.str(p, 'id', true), name: v.str(p, 'name', true), source: v.str(p, 'source', true), base: v.str(p, 'base', true), rev: v.int(p, 'rev', 0, 1e9, true) },
          p2: v.int(o, 'p2', 0, 1e7, true), p2star: v.int(o, 'p2star', 0, 1e7, true), gen: v.int(o, 'gen', 0, 1e9, true),
          kind: v.oneOf(o, 'kind', ['did', 'raw']), sid: v.int(o, 'sid', 0, 255), did: v.int(o, 'did', 0, 0xffff, true), didName: v.str(o, 'didName', true),
          request: v.hex(o, 'request'), sim: v.str(o, 'sim', true), source: v.str(o, 'source', true), config: v.config(o, 'config'), fields: v.fields(o, 'fields'), snap: !!o.snap };
      }
      case 'ev': return { r, t, id: v.int(o, 'id', 1, 1e12), i: v.int(o, 'i', 0, 1e6), kind: v.token(o, 'kind'), bytes: v.hex(o, 'bytes', true), label: v.str(o, 'label', true), note: v.str(o, 'note', true), ignored: !!o.ignored, snap: !!o.snap };
      case 'st': return { r, t, id: v.int(o, 'id', 1, 1e12), status: v.token(o, 'status'), nrc: v.int(o, 'nrc', 0, 255, true), pendingCount: v.int(o, 'pendingCount', 0, 1e6, true) || 0, applied: o.applied === true ? true : o.applied === false ? false : null, appliedNote: v.str(o, 'appliedNote', true), tEnd: v.num(o, 'tEnd', true), final: v.hex(o, 'final', true) };
      default: v.fail(`알 수 없는 레코드 r=${esc(String(r).slice(0, 12))}`);
    }
  }
  function blankTxn(rec, U) {
    const sn = U && U.sidName ? U.sidName(rec.sid) : null;
    return { id: rec.id, no: rec.no, ecu: rec.ecu, ecuName: rec.ecuName, transport: rec.transport,
      route: { label: rec.routeLabel, summary: rec.routeSummary, ptag: rec.ptag, transport: rec.transport, values: rec.config, fields: rec.fields }, profile: rec.profile,
      timing: { p2: rec.p2, p2star: rec.p2star }, gen: rec.gen, kind: rec.kind, sid: rec.sid, sidName: sn,
      did: rec.did, didHex: rec.did != null ? hex4(rec.did) : '', didName: rec.didName, request: rec.request, sim: { scenario: rec.sim }, source: rec.source,
      t0: rec.t, tEnd: null, status: 'waiting', pendingCount: 0, final: null, nrc: null, applied: null, appliedNote: '', events: [], snap: rec.snap };
  }
  // records -> { meta, txns, ctx, marks }. Event index dedupes snapshot re-headers (rotation / seen-map purge).
  function rebuild(recs, U, meta) {
    const map = new Map(), ctx = [], marks = [];
    recs.forEach((x, n) => {
      if (x.r === 'tx') { if (!map.has(x.id)) map.set(x.id, Object.assign(blankTxn(x, U), { _ev: [] })); }
      else if (x.r === 'ev') {
        const tx = map.get(x.id); if (!tx) throw new Error(`${n + 1}번째 레코드: 헤더 없는 트랜잭션 id ${x.id}`);
        if (!tx._ev[x.i]) tx._ev[x.i] = { t: x.t, kind: x.kind, bytes: x.bytes, label: x.label, note: x.note, ignored: x.ignored, snap: x.snap };
      } else if (x.r === 'st') {
        const tx = map.get(x.id); if (!tx) throw new Error(`${n + 1}번째 레코드: 헤더 없는 트랜잭션 id ${x.id}`);
        Object.assign(tx, { status: x.status, nrc: x.nrc, pendingCount: x.pendingCount, applied: x.applied, appliedNote: x.appliedNote, tEnd: x.tEnd, final: x.final });
      } else if (x.r === 'ctx') ctx.push(x);
      else if (x.r === 'mark' || x.r === 'clear') marks.push(x);
    });
    const txns = [...map.values()].sort((a, b) => a.t0 - b.t0 || a.no - b.no);
    txns.forEach(tx => { tx.events = tx._ev.filter(Boolean); delete tx._ev; Object.freeze(tx); });
    const t0 = recs.length ? recs[0].t : (txns[0] ? txns[0].t0 : 0);
    return Object.freeze({ meta, txns, ctx, marks, t0, records: recs.length });
  }
  function fromTrace1(obj, U) {
    if (!Array.isArray(obj.transactions)) throw new Error('transactions 배열 없음');
    if (obj.transactions.length > LIMITS.importRecords) throw new Error('트랜잭션 수 상한 초과');
    const recs = [];
    obj.transactions.forEach((x, i) => {
      const v = V(`transactions[${i}]`);
      if (!x || typeof x !== 'object') v.fail('객체 아님');
      const t0 = Date.parse(v.str(x, 't0')); if (!isFinite(t0)) v.fail('t0 시각 형식 아님');
      const no = v.int(x, 'no', 1, 1e12), tl = v.str(x, 'transport'), tr = ((U && U.TRANSPORTS) || []).find(t => t.label === tl || t.id === tl);
      const sid = /^0x[0-9A-F]{2}$/.test(x.sid) ? parseInt(x.sid.slice(2), 16) : (x.sid == null ? 0x22 : v.fail('sid 형식 아님'));
      const didS = v.str(x, 'did', true); if (didS && !/^[0-9A-F]{4}$/.test(didS)) v.fail('did 형식 아님');
      recs.push(checkRecord({ r: 'tx', t: t0, id: no, no, ecu: x.ecu, transport: tr ? tr.id : tl, routeLabel: tl, routeSummary: x.route || '',
        profile: { id: x.profileId || '', name: x.profileName || '', source: x.profileSource || '', base: x.profileBase || '', rev: typeof x.profileRev === 'number' ? x.profileRev : null },
        p2: x.p2Ms, p2star: x.p2starMs, gen: x.connectionGen, kind: x.kind || 'did', sid, did: didS ? parseInt(didS, 16) : null, didName: x.didName || '', request: x.request, sim: x.simScenario || '', config: x.config }, `transactions[${i}]`));
      if (!Array.isArray(x.events)) v.fail('events 배열 아님');
      x.events.forEach((ev, j) => {
        const vv = V(`transactions[${i}].events[${j}]`);
        recs.push(checkRecord({ r: 'ev', t: t0 + vv.num(ev, 'dtMs'), id: no, i: j, kind: ev.kind, bytes: ev.bytes || null, note: ev.note || '', label: '', ignored: !!ev.ignored }, `transactions[${i}].events[${j}]`));
      });
      const nrc = /^0x[0-9A-F]{2}$/.test(x.nrc || '') ? parseInt(x.nrc.slice(2), 16) : null;
      recs.push(checkRecord({ r: 'st', t: t0 + (x.elapsedMs || 0), id: no, status: x.status, nrc, pendingCount: x.pendingCount || 0, applied: !!x.appliedToConfirmed, tEnd: x.elapsedMs != null ? t0 + x.elapsedMs : null, final: x.finalResponse || null }, `transactions[${i}]`));
    });
    return recs;
  }
  // Atomic: whole text validated before anything is shown; first error rejects the file.
  function parseLog(text, U, name) {
    try {
      if (typeof text !== 'string') throw new Error('텍스트 아님');
      if (u8len(text) > LIMITS.importBytes) throw new Error(`파일 크기 상한 ${fmtB(LIMITS.importBytes)} 초과`);
      const s = text.replace(/^﻿/, '');
      let whole = null;
      if (/^\s*\{/.test(s)) { try { whole = JSON.parse(s); } catch (e) { whole = null; } }
      let recs, kind;
      if (whole && whole.format === TRACE1) { recs = fromTrace1(whole, U); kind = 'trace/1 JSON 내보내기'; }
      else if (whole && whole.format === LOG_FORMAT) {
        if (!Array.isArray(whole.records)) throw new Error('records 배열 없음');
        if (whole.records.length > LIMITS.importRecords) throw new Error(`레코드 상한 ${LIMITS.importRecords} 초과`);
        recs = whole.records.map((o, i) => checkRecord(o, i + 1)); kind = 'log JSON';
      } else if (whole && typeof whole === 'object' && !Array.isArray(whole) && whole.r == null) throw new Error(`알 수 없는 형식 ${esc(String(whole.format || '(없음)').slice(0, 40))} — ${LOG_FORMAT} 또는 ${TRACE1}만 지원`);
      else {
        const lines = s.split('\n'); if (lines.length && lines[lines.length - 1] === '') lines.pop();
        if (lines.length > LIMITS.importRecords) throw new Error(`레코드 상한 ${LIMITS.importRecords} 초과`);
        recs = lines.map((ln, i) => { if (!ln.trim()) throw new Error(`${i + 1}행: 빈 줄`); let o; try { o = JSON.parse(ln); } catch (e) { throw new Error(`${i + 1}행: JSON 아님`); } return checkRecord(o, i + 1); });
        kind = 'log JSONL';
      }
      if (!recs.length) throw new Error('레코드 없음');
      if (kind !== 'trace/1 JSON 내보내기' && recs[0].r !== 'hdr') throw new Error('1행: 헤더(hdr) 레코드 필요');
      const hdr = recs[0].r === 'hdr' ? recs[0] : null;
      const meta = { name: String(name || '').slice(0, 120), kind, seg: hdr ? hdr.seg : '', part: hdr ? hdr.part : null, startedAt: hdr ? hdr.startedAt : '', bytes: u8len(text) };
      return { ok: true, value: rebuild(recs, U, meta) };
    } catch (e) { return { ok: false, error: String(e && e.message || e) }; }
  }

  /* ================= stores (async; IndexedDB or honest memory fallback) ================= */
  function memStore() {
    const segs = new Map(), chunks = new Map();
    const P = v => Promise.resolve(v);
    return { kind: 'memory', budget: LIMITS.memTotalBytes,
      init: () => P({ kind: 'memory' }),
      listSegs: () => P([...segs.values()].map(x => Object.assign({}, x))),
      putSeg: m => { segs.set(m.id, Object.assign({}, m)); return P(); },
      addChunk: (id, n, text) => { if (!chunks.has(id)) chunks.set(id, []); chunks.get(id)[n] = text; return P(); },
      readSeg: id => P((chunks.get(id) || []).join('')),
      delSeg: id => { segs.delete(id); chunks.delete(id); return P(); },
    };
  }
  function idbStore(idb) {
    let db = null;
    const req2p = r => new Promise((res, rej) => { r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error || new Error('IndexedDB 오류')); });
    const tx2p = t => new Promise((res, rej) => { t.oncomplete = () => res(); t.onerror = () => rej(t.error || new Error('IndexedDB 트랜잭션 오류')); t.onabort = () => rej(t.error || new Error('IndexedDB 트랜잭션 중단')); });
    const ck = (id, n) => id + ':' + pad(n, 7);
    return { kind: 'idb', budget: LIMITS.maxTotalBytes,
      init() {
        return new Promise((res, rej) => {
          let r; try { r = idb.open('loom.udsReadDid.traceLog', 1); } catch (e) { rej(e); return; }
          r.onupgradeneeded = () => { const x = r.result; if (!x.objectStoreNames.contains('segs')) x.createObjectStore('segs', { keyPath: 'id' }); if (!x.objectStoreNames.contains('chunks')) x.createObjectStore('chunks', { keyPath: 'k' }); };
          r.onsuccess = () => { db = r.result; res({ kind: 'idb' }); };
          r.onerror = () => rej(r.error || new Error('IndexedDB 열기 실패'));
          r.onblocked = () => rej(new Error('IndexedDB 차단됨'));
        });
      },
      listSegs() { return req2p(db.transaction('segs', 'readonly').objectStore('segs').getAll()); },
      putSeg(m) { const t = db.transaction('segs', 'readwrite'); t.objectStore('segs').put(Object.assign({}, m)); return tx2p(t); },
      addChunk(id, n, text) { const t = db.transaction('chunks', 'readwrite'); t.objectStore('chunks').put({ k: ck(id, n), seg: id, n, text }); return tx2p(t); },
      readSeg(id) { return req2p(db.transaction('chunks', 'readonly').objectStore('chunks').getAll(IDBKeyRangeOf(idb).bound(id + ':', id + ':￿'))).then(rows => rows.sort((a, b) => a.n - b.n).map(x => x.text).join('')); },
      delSeg(id) { const t = db.transaction(['segs', 'chunks'], 'readwrite'); t.objectStore('chunks').delete(IDBKeyRangeOf(idb).bound(id + ':', id + ':￿')); t.objectStore('segs').delete(id); return tx2p(t); },
    };
  }
  const IDBKeyRangeOf = idb => idb.__KeyRange || window.IDBKeyRange;

  /* ================= boot ================= */
  let booted = false;
  function boot() {
    const H = window.LoomWorkbench;
    if (booted || !H || !H.engine) return;
    booted = true;
    const E = H.engine, U = H.constants || window.LoomUds, clock = H.clock;
    const $ = id => d.getElementById(id);
    const mount = $('traceToolsMount');
    if (!mount) { booted = false; return; }

    /* ---------- prefs (own key; never filters/pause/recording state) ---------- */
    let prefs = { v: 1, timeMode: 'abs', viewerTime: 'rel' };
    function validatePrefs(o) {
      if (!o || typeof o !== 'object' || Array.isArray(o)) return { ok: false, error: 'trace: 객체 아님' };
      if (o.v !== 1) return { ok: false, error: `trace: 알 수 없는 버전 ${esc(String(o.v))}` };
      const tm = o.timeMode == null ? 'abs' : o.timeMode, vt = o.viewerTime == null ? 'rel' : o.viewerTime;
      if (!['abs', 'rel'].includes(tm) || !['abs', 'rel'].includes(vt)) return { ok: false, error: 'trace: timeMode는 abs|rel' };
      return { ok: true, value: { v: 1, timeMode: tm, viewerTime: vt } };
    }
    try { const raw = localStorage.getItem(PKEY); if (raw) { const r = validatePrefs(JSON.parse(raw)); if (r.ok) prefs = r.value; } } catch (e) { /* defaults */ }
    const savePrefs = () => { try { localStorage.setItem(PKEY, JSON.stringify(prefs)); } catch (e) { if (H.ui && H.ui.toast) H.ui.toast('추적 설정 저장 실패 (' + (e && e.name || e) + ') — 이 세션에만 적용', 'err'); } };

    /* ---------- DOM (own subtree only) ---------- */
    const routeOpts = () => '<option value="">경로: 전체</option>' + (U.TRANSPORTS || []).map(t => `<option value="${esc(t.id)}">${esc(t.label)}</option>`).join('');
    mount.innerHTML =
      '<div class="tt" id="ttRoot">' +
      '<div class="tt-row" role="search" aria-label="트랜잭션 표시 필터 (표시만 거름 — 수신·기록·내보내기는 전체)">' +
      '<input id="ttQ" class="fld tt-q" type="search" placeholder="검색: hex (62 F1 90) 또는 텍스트 (VIN, BCM)" aria-label="hex 또는 텍스트 검색" spellcheck="false" autocomplete="off" maxlength="200">' +
      '<input id="ttSid" class="fld tt-s" placeholder="SID" aria-label="SID 필터 (hex)" maxlength="4" spellcheck="false" autocomplete="off">' +
      '<input id="ttDid" class="fld tt-d" placeholder="DID" aria-label="DID 필터 (4자리 hex)" maxlength="6" spellcheck="false" autocomplete="off">' +
      '<input id="ttNrc" class="fld tt-s" placeholder="NRC" aria-label="NRC 필터 (hex, 응답 7F xx NN)" maxlength="4" spellcheck="false" autocomplete="off">' +
      `<select id="ttStatus" class="fld" aria-label="상태 필터">${STATUS_OPTS.map(o => `<option value="${o[0]}">${esc(o[1])}</option>`).join('')}</select>` +
      `<select id="ttRoute" class="fld" aria-label="경로 필터">${routeOpts()}</select>` +
      '<select id="ttProfile" class="fld tt-p" aria-label="프로필 필터"><option value="">프로필: 전체</option></select>' +
      '<button type="button" class="btn ghost" id="ttClear">필터 해제</button>' +
      '<span class="tt-count" id="ttCount" aria-live="polite"></span>' +
      '<span class="grow"></span>' +
      '<span class="seg sm" id="ttTime" role="radiogroup" aria-label="시각 표시"><button type="button" role="radio" data-tm="abs">절대</button><button type="button" role="radio" data-tm="rel">상대</button></span>' +
      '<button type="button" class="btn" id="ttPause" aria-pressed="false" title="표시만 고정 — 수신·응답 처리·연속 기록은 계속">표시 일시정지</button>' +
      '</div>' +
      '<div class="tt-msg" id="ttMsg" aria-live="polite"></div>' +
      '<div class="tt-row tt-log" aria-label="연속 기록">' +
      '<button type="button" class="btn" id="tlRec" aria-pressed="false">기록 시작</button>' +
      '<span class="tl-st" id="tlSt" role="status" aria-live="polite"></span>' +
      '<span class="grow"></span>' +
      '<button type="button" class="btn ghost disc" id="tlSegBtn" aria-expanded="false" aria-controls="tlSegs">저장된 세그먼트 <b id="tlSegN">0</b></button>' +
      '<button type="button" class="btn ghost" id="tlView" title="읽기 전용 — 전송·재생·연결 없음">오프라인 보기…</button>' +
      '</div>' +
      '<div class="tl-segs" id="tlSegs" hidden></div>' +
      '</div>';
    const viewer = d.createElement('div');
    viewer.className = 'tl-viewer'; viewer.id = 'tlViewer'; viewer.hidden = true;
    viewer.setAttribute('role', 'dialog'); viewer.setAttribute('aria-modal', 'true'); viewer.setAttribute('aria-labelledby', 'tvTitle');
    viewer.innerHTML =
      '<div class="tv-box">' +
      '<div class="tv-h"><b id="tvTitle">오프라인 로그 보기</b><span class="tv-ro">읽기 전용 · 전송·재생·연결 없음 · 라이브 기록과 분리</span><span class="grow"></span>' +
      '<label class="btn ghost tv-file">파일 열기…<input type="file" id="tvFile" accept=".jsonl,.json,application/json,text/plain"></label>' +
      '<button type="button" class="btn ghost" id="tvClose">닫기</button></div>' +
      '<div class="tv-src" id="tvSrc"></div>' +
      '<div class="tt-row tv-f" role="search" aria-label="오프라인 로그 필터">' +
      '<input id="tvQ" class="fld tt-q" type="search" placeholder="검색: hex 또는 텍스트" aria-label="오프라인 hex/텍스트 검색" maxlength="200" spellcheck="false" autocomplete="off">' +
      '<input id="tvSid" class="fld tt-s" placeholder="SID" aria-label="오프라인 SID 필터" maxlength="4"><input id="tvDid" class="fld tt-d" placeholder="DID" aria-label="오프라인 DID 필터" maxlength="6"><input id="tvNrc" class="fld tt-s" placeholder="NRC" aria-label="오프라인 NRC 필터" maxlength="4">' +
      `<select id="tvStatus" class="fld" aria-label="오프라인 상태 필터">${STATUS_OPTS.map(o => `<option value="${o[0]}">${esc(o[1])}</option>`).join('')}</select>` +
      `<select id="tvRoute" class="fld" aria-label="오프라인 경로 필터">${routeOpts()}</select>` +
      '<span class="tt-count" id="tvCount" aria-live="polite"></span><span class="grow"></span>' +
      '<span class="seg sm" id="tvTime" role="radiogroup" aria-label="오프라인 시각 표시"><button type="button" role="radio" data-tm="abs">절대</button><button type="button" role="radio" data-tm="rel">상대</button></span>' +
      '</div>' +
      '<div class="tt-msg" id="tvMsg" aria-live="polite"></div>' +
      '<div class="tv-main"><div class="tv-list" id="tvList" role="grid" tabindex="0" aria-label="오프라인 트랜잭션 (↑↓)"></div>' +
      '<div class="tv-det" id="tvDet" aria-label="선택 트랜잭션 이벤트"></div></div>' +
      '</div>';
    mount.appendChild(viewer);

    /* ---------- live display filter + pause ---------- */
    let F = EMPTY_F(), C = compile(F);
    let paused = false, frozenAll = null, frozenMap = null, pausedAt = null, pausedNew = 0;
    let last = { base: 0, src: 0, out: 0, first: null };
    function scopeOk(base) {
      if (typeof H.traceScope === 'function') {
        let sc = null; try { sc = H.traceScope(); } catch (e) { sc = null; }
        if (sc && sc.filter === 'all') return () => true;
        if (sc && sc.filter === 'ecu') return t => t.ecu === sc.ecu;
      }
      const ids = new Set(base.map(t => t.id)), live = E.txns;  // fallback inference (host without traceScope)
      if (!live.some(t => !ids.has(t.id))) return () => true;  // base scope = all
      const cur = (H.context() || {}).ecu, liveIds = new Set(live.map(t => t.id));
      return t => ids.has(t.id) || (!liveIds.has(t.id) && t.ecu === cur); // evicted/cleared since pause: same ECU scope
    }
    const view = {
      rows(base) {
        let src = base;
        if (paused) { if (!frozenAll) freezeNow(); src = frozenAll.filter(scopeOk(base)); }
        const out = C.active ? src.filter(C.pred) : src;
        last = { base: base.length, src: src.length, out: out.length, first: out.length ? out[0].t0 : null };
        return out;
      },
      byId(id) { return paused && frozenMap ? frozenMap.get(id) || null : null; },
      // optional for core: relative/absolute row time text (contract extension request, see TRACE-HANDOFF)
      timeMode: () => prefs.timeMode,
      timeText(tx) { return prefs.timeMode === 'rel' && last.first != null ? fmtRel(tx.t0 - last.first) : fmtT(tx.t0); },
    };
    function freezeNow() {
      const at = clock.now();
      frozenAll = E.txns.map(t => cloneTxn(t, at)); frozenMap = new Map(frozenAll.map(t => [t.id, t]));
    }
    function setFilters(patch) {
      F = Object.assign({}, F, patch); C = compile(F);
      syncFilterInputs(); H.refresh(); renderToolbar();
    }
    function clearFilters() { F = EMPTY_F(); C = compile(F); syncFilterInputs(true); H.refresh(); renderToolbar(); }
    function pause() {
      if (paused) return; paused = true; pausedAt = clock.now(); pausedNew = 0; freezeNow();
      H.refresh(); renderToolbar();
    }
    function resume() {
      if (!paused) return; paused = false; frozenAll = frozenMap = null; pausedAt = null; pausedNew = 0;
      H.refresh(); renderToolbar();
    }
    const fIds = { q: 'ttQ', sid: 'ttSid', did: 'ttDid', nrc: 'ttNrc', status: 'ttStatus', route: 'ttRoute', profile: 'ttProfile' };
    function syncFilterInputs(force) {
      Object.entries(fIds).forEach(([k, id]) => {
        const el = $(id); if (!el) return;
        if (force && el.value !== F[k]) el.value = F[k];
        const bad = !!C.errors[k]; el.setAttribute('aria-invalid', bad ? 'true' : 'false'); el.classList.toggle('invalid', bad);
      });
    }
    function profileOptions() {
      const sel = $('ttProfile'); if (!sel || d.activeElement === sel) return;
      const seen = new Map();
      (frozenAll || E.txns).forEach(t => { const p = t.profile || {}; if (p.id && !seen.has(p.id)) seen.set(p.id, p); });
      if (F.profile && !seen.has(F.profile)) seen.set(F.profile, { id: F.profile, name: F.profile + ' (현재 기록 없음)' });
      const key = [...seen.keys()].join('|');
      if (sel.dataset.k === key) return;
      sel.dataset.k = key;
      sel.innerHTML = '<option value="">프로필: 전체</option>' + [...seen.values()].map(p => `<option value="${esc(p.id)}">${esc((p.name || p.id) + (p.source ? ' · ' + p.source : ''))}</option>`).join('');
      sel.value = F.profile;
    }
    function renderToolbar() {
      const cnt = $('ttCount'), msg = $('ttMsg'), pb = $('ttPause');
      const errs = Object.values(C.errors);
      cnt.textContent = (C.active || paused) ? `표시 ${last.out} / ${paused ? '고정 ' + last.src : last.base}` : `전체 ${last.base}건 표시`;
      cnt.classList.toggle('on', C.active);
      pb.textContent = paused ? '표시 재개' : '표시 일시정지';
      pb.setAttribute('aria-pressed', paused ? 'true' : 'false');
      pb.classList.toggle('on', paused);
      $('ttRoot').classList.toggle('paused', paused);
      const parts = [];
      if (paused) parts.push(`<span class="tt-pz">일시정지 ${esc(fmtT(pausedAt))} 시점 화면 고정 · 수신·기록은 계속 (새 이벤트 ${pausedNew}건) · 재개하면 최신 상태 표시</span>`);
      if (errs.length) parts.push(`<span class="tt-err">${errs.map(esc).join(' · ')} — 해당 조건은 적용하지 않음</span>`);
      if (C.active && !paused && last.out === 0 && last.base > 0) parts.push('<span>조건에 맞는 기록 없음 — 선택·상세 해제됨</span>');
      msg.innerHTML = parts.join(' ');
      $('ttTime').querySelectorAll('[data-tm]').forEach(b => b.setAttribute('aria-checked', b.dataset.tm === prefs.timeMode ? 'true' : 'false'));
      profileOptions();
    }
    Object.entries(fIds).forEach(([k, id]) => {
      const el = $(id);
      el.addEventListener(el.tagName === 'SELECT' ? 'change' : 'input', () => setFilters({ [k]: el.value }));
    });
    $('ttClear').addEventListener('click', clearFilters);
    $('ttPause').addEventListener('click', () => (paused ? resume() : pause()));
    $('ttTime').addEventListener('click', e => { const b = e.target.closest('[data-tm]'); if (!b) return; prefs.timeMode = b.dataset.tm; savePrefs(); H.refresh(); renderToolbar(); });
    $('ttTime').addEventListener('keydown', e => { if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return; e.preventDefault(); prefs.timeMode = prefs.timeMode === 'abs' ? 'rel' : 'abs'; savePrefs(); H.refresh(); renderToolbar(); $('ttTime').querySelector(`[data-tm="${prefs.timeMode}"]`).focus(); });

    /* ---------- continuous recorder ---------- */
    let store = window.__loomUdsTraceStore || (window.indexedDB ? idbStore(window.indexedDB) : memStore());
    const R = { state: 'init', storeKind: null, storeNote: '', seg: null, buf: [], bufBytes: 0, flushH: null, q: Promise.resolve(), seen: new Map(), ctxSeen: new Map(), err: '', notice: '', segs: [], sel: null, delArm: null, startT: null, session: 'R' + Math.random().toString(36).slice(2, 8) };
    const ready = store.init().then(r => { R.storeKind = r.kind; }, err => {
      R.storeNote = `IndexedDB 열기 실패 (${String(err && (err.name || err.message) || err)})`; store = memStore(); R.storeKind = 'memory';
    }).then(() => refreshSegs(true)).then(() => { R.state = 'idle'; renderRec(); });
    const budget = () => (R.storeKind === 'idb' ? LIMITS.maxTotalBytes : LIMITS.memTotalBytes);
    function refreshSegs(boot) {
      return store.listSegs().then(list => {
        const fix = [];
        list.forEach(m => { if (m.state === 'recording' && (!R.seg || R.seg.id !== m.id)) { m.state = 'interrupted'; if (boot) fix.push(store.putSeg(m)); } });
        R.segs = list.sort((a, b) => b.startedT - a.startedT || (b.part || 0) - (a.part || 0));
        if (R.sel && !R.segs.some(s => s.id === R.sel)) R.sel = null;
        return Promise.all(fix).catch(() => null);
      }).then(() => renderSegs(), err => { R.err = '세그먼트 목록 읽기 실패: ' + errText(err); renderRec(); });
    }
    const errText = err => { const n = err && err.name; return n === 'QuotaExceededError' ? '저장 공간 부족 (QuotaExceededError)' : String(n ? n + (err.message ? ': ' + err.message : '') : (err && err.message) || err); };
    function newSeg(part) {
      const t = clock.now();
      R.seg = { id: `${R.session}-${pad(Math.floor(t / 1000) % 1e8, 8)}-${pad(part, 3)}${Math.random().toString(36).slice(2, 5)}`, part, session: R.session, startedT: t, startedAt: new Date(t).toISOString(), endedT: null, records: 0, bytes: 0, chunks: 0, state: 'recording', store: R.storeKind };
      R.seen = new Map(); R.ctxSeen = new Map(); R.s = 0;
      push({ r: 'hdr', format: LOG_FORMAT, seg: R.seg.id, part, session: R.session, startedAt: R.seg.startedAt, note: 'Loom UDS Read DID — local SIM log. Mock routes; bytes are UDS PDUs only (no CAN-TP/DoIP/EthDiag frames). Engine clock.' });
    }
    function push(rec) {
      if (rec.t == null) rec.t = clock.now();
      rec.s = R.s = (R.s || 0) + 1;
      const line = JSON.stringify(rec) + '\n';
      R.buf.push(line); R.bufBytes += u8len(line);
      if (R.buf.length >= LIMITS.chunkRecords) flush();
      else if (!R.flushH) R.flushH = clock.set(() => { R.flushH = null; flush(); }, LIMITS.flushMs);
    }
    // Serial write queue. enqueue() appends ONE op and returns that op's promise (resolves true/false, never rejects).
    // Queued ops never call flush()/enqueue() themselves, so no op can wait on its own chain.
    function enqueue(fn, onErr) {
      const p = R.q.then(fn).then(() => true, err => { (onErr || fail)(err); return false; });
      R.q = p; return p;
    }
    function flush() {
      if (R.flushH) { clock.clear(R.flushH); R.flushH = null; }
      if (!R.buf.length || !R.seg) return R.q;
      const seg = R.seg, text = R.buf.join(''), n = seg.chunks, k = R.buf.length, b = R.bufBytes;
      R.buf = []; R.bufBytes = 0;
      seg.chunks += 1; seg.records += k; seg.bytes += b;
      const meta = Object.assign({}, seg);
      const p = enqueue(() => (seg.dead ? null : store.addChunk(seg.id, n, text).then(() => store.putSeg(meta)).then(() => { seg.saved = meta.records; renderRec(); })));
      if (R.state === 'recording' && R.seg === seg && (seg.bytes >= LIMITS.segBytes || seg.records >= LIMITS.segRecords)) rotate();
      return p;
    }
    function fail(err) {
      if (R.state === 'error') return;
      const seg = R.seg;
      if (seg) seg.dead = true;
      R.state = 'error'; R.err = `기록 중지됨 — 저장 실패: ${errText(err)}. 저장된 레코드 ${seg ? seg.saved || 0 : 0}건까지만 보존`;
      if (R.flushH) { clock.clear(R.flushH); R.flushH = null; }
      R.buf = []; R.bufBytes = 0;
      if (seg) { const m = Object.assign({}, seg, { state: 'failed', records: seg.saved || 0, endedT: clock.now() }); R.seg = null; store.putSeg(m).catch(() => null).then(() => refreshSegs()); }
      renderRec();
    }
    function snapshotLive(reason) {
      // Context + in-flight state at segment start (flagged snap: observed from live state, not as it happened).
      ctxScan();
      E.txns.forEach(tx => { if (!E.terminal(tx.status)) diffTxn(tx, true); });
      if (reason) push({ r: 'mark', text: reason });
    }
    function rotate() {
      const old = R.seg;
      push({ r: 'mark', text: 'rotate → part ' + (old.part + 1) });
      flushTo(old, 'closed');
      newSeg(old.part + 1);
      snapshotLive('rotated from ' + old.id);
      R.notice = `세그먼트 회전: ${old.id} 닫음 (레코드 ${old.records} 상한 ${LIMITS.segRecords} / ${fmtB(LIMITS.segBytes)})`;
      enqueue(() => retention());
    }
    function flushTo(seg, state) {
      const text = R.buf.join(''), k = R.buf.length, b = R.bufBytes, n = seg.chunks;
      R.buf = []; R.bufBytes = 0; if (R.flushH) { clock.clear(R.flushH); R.flushH = null; }
      if (k) { seg.chunks += 1; seg.records += k; seg.bytes += b; }
      seg.state = state; seg.endedT = clock.now();
      const meta = Object.assign({}, seg);
      return enqueue(() => (seg.dead ? null : Promise.resolve(k ? store.addChunk(seg.id, n, text) : null).then(() => store.putSeg(meta)).then(() => { seg.saved = meta.records; })));
    }
    function retention() {
      return store.listSegs().then(list => {
        const closed = list.filter(m => !R.seg || m.id !== R.seg.id).sort((a, b) => a.startedT - b.startedT);
        let total = list.reduce((s, m) => s + (m.bytes || 0), 0), count = list.length, del = [];
        while (closed.length && (count > LIMITS.maxSegments || total > budget())) { const m = closed.shift(); del.push(m); count--; total -= m.bytes || 0; }
        if (!del.length) return null;
        return Promise.all(del.map(m => store.delSeg(m.id))).then(() => { R.notice = `보존 한도 (${LIMITS.maxSegments}개 / ${fmtB(budget())})로 오래된 세그먼트 ${del.length}개 삭제: ${del.map(m => m.id).join(', ')}`; });
      }).then(() => refreshSegs(), err => { R.err = '보존 정리 실패: ' + errText(err); renderRec(); });
    }
    function txRec(tx, snap) {
      const r = tx.route || {}, p = tx.profile || {};
      return { r: 'tx', t: tx.t0, id: tx.id, no: tx.no, ecu: tx.ecu, ecuName: tx.ecuName || '', transport: tx.transport, routeLabel: r.label || '', routeSummary: r.summary || '', ptag: r.ptag || '',
        profile: { id: p.id || '', name: p.name || '', source: p.source || '', base: p.base || '', rev: typeof p.rev === 'number' ? p.rev : null },
        p2: tx.timing ? tx.timing.p2 : null, p2star: tx.timing ? tx.timing.p2star : null, gen: tx.gen, kind: tx.kind, sid: tx.sid, did: tx.did != null ? tx.did : null, didName: tx.didName || '',
        request: hexs(tx.request), sim: tx.sim ? tx.sim.scenario : '', source: tx.source || '',
        config: Object.assign({}, r.values || {}), fields: Array.isArray(r.fields) ? r.fields.map(f => [String(f[0]), String(f[1])]) : [], snap: snap ? 1 : undefined };
    }
    function diffTxn(tx, snap) {
      let s = R.seen.get(tx.id);
      if (!s) {
        if (R.seen.size >= LIMITS.seen) { for (const [k, v] of R.seen) { if (v.term) { R.seen.delete(k); break; } } }
        s = { n: 0, st: null, term: false }; R.seen.set(tx.id, s);
        push(txRec(tx, snap || tx.t0 < R.startT || tx.events.length > 1));
      }
      for (; s.n < tx.events.length; s.n++) {
        const ev = tx.events[s.n];
        push({ r: 'ev', t: ev.t, id: tx.id, i: s.n, kind: ev.kind, bytes: ev.bytes ? hexs(ev.bytes) : undefined, label: ev.label || '', note: ev.note || '', ignored: ev.ignored ? 1 : undefined, snap: ev.t < R.startT ? 1 : undefined });
      }
      const sk = tx.status + '|' + tx.pendingCount + '|' + tx.applied;
      if (s.st !== sk) {
        s.st = sk; s.term = E.terminal(tx.status);
        push({ r: 'st', t: tx.tEnd != null ? tx.tEnd : clock.now(), id: tx.id, status: tx.status, nrc: tx.nrc != null ? tx.nrc : undefined, pendingCount: tx.pendingCount, applied: tx.applied, appliedNote: tx.appliedNote || '', tEnd: tx.tEnd, final: tx.final ? hexs(tx.final) : undefined });
      }
    }
    function ctxScan() {
      (U.ECUS || []).forEach(e => (U.TRANSPORTS || []).forEach(t => {
        const c = E.ctx(e.id, t.id), k = e.id + '|' + t.id, v = c.link + '|' + c.gen;
        if (R.ctxSeen.get(k) === v) return;
        const first = !R.ctxSeen.has(k); R.ctxSeen.set(k, v);
        if (first && c.link !== 'online' && !c.gen) return; // never-used offline contexts are not noise-logged
        push({ r: 'ctx', ecu: e.id, transport: t.id, link: c.link, gen: c.gen });
      }));
    }
    function onEngine(ev) {
      if (paused && ev && ev.txn) pausedNew++;
      if (R.state === 'recording' || R.state === 'starting') {
        if (ev && ev.type === 'clear') push({ r: 'clear' });
        if (ev && ev.txn) diffTxn(ev.txn);
        E.txns.forEach(tx => { const s = R.seen.get(tx.id); if (!s ? !E.terminal(tx.status) || tx.t0 >= R.startT : (s.n !== tx.events.length || s.st !== tx.status + '|' + tx.pendingCount + '|' + tx.applied)) diffTxn(tx); });
        ctxScan();
        renderRec();
      }
      if (paused) renderToolbar();
    }
    function start() {
      if (R.state !== 'idle' && R.state !== 'error') return Promise.resolve(false);
      R.err = ''; R.notice = ''; R.state = 'starting'; R.startT = clock.now();
      newSeg(1);
      snapshotLive('start');
      const seg = R.seg;
      renderRec();
      // 'recording' is claimed only after the segment meta AND the first chunk (hdr/snapshot) are durably written.
      enqueue(() => store.putSeg(Object.assign({}, seg)), err => {
        seg.dead = true; if (R.seg === seg) R.seg = null;
        R.state = 'error'; R.buf = []; R.bufBytes = 0; if (R.flushH) { clock.clear(R.flushH); R.flushH = null; }
        R.err = '기록 시작 실패: ' + errText(err) + ' — 기록되지 않음'; renderRec();
      });
      return flush().then(ok => {
        if (ok && !seg.dead && R.state === 'starting' && R.seg === seg) R.state = 'recording';
        renderRec(); refreshSegs();
        return R.state === 'recording' || (R.state === 'stopping' && !seg.dead);
      });
    }
    function stop() {
      if (R.state !== 'recording' && R.state !== 'starting') return Promise.resolve(false);
      const seg = R.seg;
      push({ r: 'mark', text: 'stop' });
      R.state = 'stopping'; renderRec();
      return flushTo(seg, 'closed').then(ok => { if (R.state === 'stopping') R.state = 'idle'; if (R.seg === seg) R.seg = null; return retention().then(() => { renderRec(); return ok; }); });
    }
    function selSeg() { return R.segs.find(s => s.id === R.sel) || null; }
    function segText(id) { if (R.seg && R.seg.id === id) return flush().then(() => store.readSeg(id)); return store.readSeg(id); }
    function download(name, text, type) {
      const a = d.createElement('a'); a.href = URL.createObjectURL(new Blob([text], { type })); a.download = name;
      d.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
    }
    function exportSeg(fmt) {
      const m = selSeg(); if (!m) return Promise.resolve(null);
      return segText(m.id).then(text => {
        if (fmt === 'jsonl') { download(`loom-uds-log-${m.id}.jsonl`, text, 'application/x-ndjson'); return text; }
        const recs = text.split('\n').filter(Boolean).map(l => JSON.parse(l));
        const out = JSON.stringify({ format: LOG_FORMAT, exportedAt: new Date().toISOString(), segment: m, note: 'Local SIM log segment. Bytes are UDS PDUs only.', records: recs }, null, 1);
        download(`loom-uds-log-${m.id}.json`, out, 'application/json'); return out;
      }, err => { R.err = '세그먼트 읽기 실패: ' + errText(err); renderRec(); return null; });
    }
    function deleteSeg() {
      const m = selSeg(); if (!m) return Promise.resolve(false);
      if (R.seg && R.seg.id === m.id) { R.err = '기록 중인 세그먼트는 삭제할 수 없음 — 먼저 기록 중지'; renderRec(); return Promise.resolve(false); }
      if (R.delArm !== m.id) { R.delArm = m.id; renderSegs(); return Promise.resolve(false); }
      R.delArm = null;
      return store.delSeg(m.id).then(() => { R.notice = `세그먼트 ${m.id} 삭제됨 (다른 세그먼트 유지)`; R.sel = null; return refreshSegs(); }).then(() => true, err => { R.err = '삭제 실패: ' + errText(err); renderRec(); return false; });
    }
    let actWas = false;
    function renderRec() {
      const b = $('tlRec'), stEl = $('tlSt'); if (!b) return;
      const on = R.state === 'recording' || R.state === 'starting';
      // Activity membership changed (start/stop/error) → one scheduled core render so the header activity badge updates.
      // renderRec is never called from onRender, so this cannot loop.
      const act = on || R.state === 'stopping';
      if (act !== actWas) { actWas = act; if (H.schedule) H.schedule(); }
      b.textContent = on ? '기록 중지' : R.state === 'stopping' ? '저장 중…' : '기록 시작';
      b.disabled = R.state === 'init' || R.state === 'stopping';
      b.setAttribute('aria-pressed', on ? 'true' : 'false'); b.classList.toggle('rec', on);
      const where = R.storeKind === 'idb' ? '브라우저 로컬 저장소(IndexedDB)' : '메모리 전용 — 새로고침 시 소실';
      let h = '';
      if (R.state === 'init') h = '저장소 확인 중…';
      else if (R.state === 'starting') h = '<span class="tl-dot"></span>기록 시작 중 (저장 확인 전)…';
      else if (R.state === 'recording') { const s = R.seg; h = `<span class="tl-dot on"></span><b>기록 중</b> · ${esc(s.id)} · 레코드 ${s.records + R.buf.length} (저장 ${s.saved || 0}) · ${fmtB(s.bytes + R.bufBytes)} · ${esc(where)}`; }
      else if (R.state === 'stopping') h = '마지막 청크 저장 중…';
      else h = `기록 꺼짐 · ${esc(where)} · 세그먼트 상한 ${LIMITS.segRecords} 레코드/${fmtB(LIMITS.segBytes)}, 보존 ${LIMITS.maxSegments}개/${fmtB(budget())}`;
      if (R.storeKind === 'memory') h += ` <span class="tt-warn">${esc((R.storeNote ? R.storeNote + ' — ' : 'IndexedDB 사용 불가 — ') + '메모리에만 기록됨, 브라우저/OS 파일이 아님')}</span>`;
      if (R.err) h += ` <span class="tt-err">${esc(R.err)}</span>`;
      if (R.notice) h += ` <span class="tl-note">${esc(R.notice)}</span>`;
      stEl.innerHTML = h;
      $('tlSegN').textContent = String(R.segs.length);
    }
    const SEGST = { recording: '기록 중', closed: '완료', interrupted: '중단됨 (새로고침/종료)', failed: '저장 실패로 중단' };
    function renderSegs() {
      renderRec();
      const box = $('tlSegs'); if (!box || box.hidden) return;
      if (!R.segs.length) { box.innerHTML = '<p class="tl-empty">저장된 세그먼트 없음 — [기록 시작]으로만 기록합니다 (새로고침 후 자동 재개 없음).</p>'; return; }
      const m = selSeg();
      box.innerHTML = '<div class="tl-tbl" role="radiogroup" aria-label="저장된 세그먼트 (하나 선택)">' + R.segs.map(s =>
        `<label class="tl-seg${s.id === R.sel ? ' sel' : ''}"><input type="radio" name="tlseg" value="${esc(s.id)}"${s.id === R.sel ? ' checked' : ''}>` +
        `<span class="mono">${esc(s.id)}</span><span>${esc(fmtT(s.startedT))}${s.endedT ? '–' + esc(fmtT(s.endedT)) : ''}</span><span>${s.records} 레코드</span><span>${fmtB(s.bytes || 0)}</span><span class="tl-s s-${esc(s.state)}">${esc(SEGST[s.state] || s.state)}</span></label>`).join('') + '</div>' +
        `<div class="tl-act"><span>${m ? '선택: <b class="mono">' + esc(m.id) + '</b>' : '세그먼트를 선택하세요'}</span><span class="grow"></span>` +
        `<button type="button" class="btn ghost" data-tl="open"${m ? '' : ' disabled'}>오프라인 보기로 열기</button>` +
        `<button type="button" class="btn ghost" data-tl="jsonl"${m ? '' : ' disabled'}>JSONL 다운로드</button>` +
        `<button type="button" class="btn ghost" data-tl="json"${m ? '' : ' disabled'}>JSON 다운로드</button>` +
        `<button type="button" class="btn ghost danger" data-tl="del"${m ? '' : ' disabled'}>${m && R.delArm === m.id ? '삭제 확인 (이 세그먼트만)' : '삭제'}</button></div>`;
    }
    $('tlRec').addEventListener('click', () => { if (R.state === 'recording' || R.state === 'starting') stop(); else start(); });
    $('tlSegBtn').addEventListener('click', () => { const box = $('tlSegs'); box.hidden = !box.hidden; $('tlSegBtn').setAttribute('aria-expanded', String(!box.hidden)); if (!box.hidden) refreshSegs(); });
    $('tlSegs').addEventListener('change', e => { if (e.target.name === 'tlseg') { R.sel = e.target.value; R.delArm = null; renderSegs(); } });
    $('tlSegs').addEventListener('click', e => {
      const b = e.target.closest('[data-tl]'); if (!b) return;
      const a = b.dataset.tl;
      if (a === 'open') openSegment(R.sel); else if (a === 'jsonl' || a === 'json') exportSeg(a); else if (a === 'del') deleteSeg();
    });

    /* ---------- offline viewer (read-only; no engine access below this line except none) ---------- */
    const VW = { model: null, F: EMPTY_F(), C: compile(EMPTY_F()), sel: null, src: '', err: '', opener: null };
    function openViewer(opener) { VW.opener = opener || d.activeElement; viewer.hidden = false; renderViewer(); $('tvClose').focus(); }
    function closeViewer() { viewer.hidden = true; const o = VW.opener; VW.opener = null; if (o && o.focus) o.focus(); }
    function loadText(text, name, src) {
      const r = parseLog(text, U, name);
      if (!r.ok) { VW.err = `열기 실패 (${esc(name || '파일')}): ${r.error} — 아무것도 불러오지 않음`; renderViewer(); return r; }
      VW.model = r.value; VW.src = src; VW.err = ''; VW.sel = r.value.txns.length ? r.value.txns[0].id : null;
      renderViewer(); return r;
    }
    function openSegment(id) {
      if (!id) return Promise.resolve(null);
      openViewer($('tlView'));
      return segText(id).then(text => loadText(text, id, '저장된 세그먼트 ' + id), err => { VW.err = '세그먼트 읽기 실패: ' + errText(err); renderViewer(); });
    }
    function openFile(file) {
      if (!file) return;
      if (file.size > LIMITS.importBytes) { VW.err = `파일 ${fmtB(file.size)} — 상한 ${fmtB(LIMITS.importBytes)} 초과, 읽지 않음`; renderViewer(); return; }
      const rd = new FileReader();
      rd.onload = () => loadText(String(rd.result), file.name, '파일 ' + file.name);
      rd.onerror = () => { VW.err = '파일 읽기 실패'; renderViewer(); };
      rd.readAsText(file);
    }
    function vis() { const m = VW.model; if (!m) return []; return VW.C.active ? m.txns.filter(VW.C.pred) : m.txns; }
    function tv(t) { return prefs.viewerTime === 'rel' && VW.model ? fmtRel(t - VW.model.t0) : fmtT(t); }
    const ST = { waiting: '응답 대기', pending: '대기 0x78', positive: '✓ 긍정', nrc: '✗ NRC', timeout: '⌛ 타임아웃', cancelled: '⛔ 취소', 'link-lost': '⚡ 링크 끊김', suppressed: '응답 억제 (SPRMIB, 응답 없음 정상)' };
    function renderViewer() {
      if (viewer.hidden) return;
      const m = VW.model, rows = vis();
      if (VW.sel != null && !rows.some(t => t.id === VW.sel)) VW.sel = rows.length ? rows[0].id : null;
      $('tvSrc').innerHTML = m
        ? `<b>${esc(VW.src)}</b> · ${esc(m.meta.kind)} · ${m.meta.seg ? '세그먼트 ' + esc(m.meta.seg) + (m.meta.part ? ' part ' + m.meta.part : '') + ' · ' : ''}${m.meta.startedAt ? '시작 ' + esc(m.meta.startedAt) + ' · ' : ''}레코드 ${m.records} · 트랜잭션 ${m.txns.length} · 연결 이벤트 ${m.ctx.length} · ${fmtB(m.meta.bytes)}`
        : '<span>열린 로그 없음 — [파일 열기…] 또는 저장된 세그먼트의 [오프라인 보기로 열기]. 전송·재생 기능은 없습니다.</span>';
      const errs = Object.values(VW.C.errors);
      $('tvMsg').innerHTML = (VW.err ? `<span class="tt-err">${esc(VW.err)}</span> ` : '') + (errs.length ? `<span class="tt-err">${errs.map(esc).join(' · ')} — 해당 조건은 적용하지 않음</span>` : '');
      $('tvCount').textContent = m ? `표시 ${rows.length} / ${m.txns.length}` : '';
      $('tvTime').querySelectorAll('[data-tm]').forEach(b => b.setAttribute('aria-checked', b.dataset.tm === prefs.viewerTime ? 'true' : 'false'));
      const list = $('tvList');
      list.innerHTML = !m ? '' : '<div class="tv-r tv-hd" role="row"><span>#</span><span>시각</span><span>ECU</span><span>경로</span><span>DID · SID</span><span>요청</span><span>최종 응답</span><span>상태</span><span>프로필</span></div>' +
        (rows.length ? rows.map(t => `<div class="tv-r${t.id === VW.sel ? ' sel' : ''}" role="row" data-id="${t.id}" id="tv-${t.id}" aria-selected="${t.id === VW.sel}">` +
          `<span>${t.no}</span><span>${esc(tv(t.t0))}</span><span>${esc(t.ecu)}</span><span>${esc(t.route.label)}</span>` +
          `<span>${t.kind === 'raw' ? '<i class="rawtag">RAW</i> ' : ''}${t.did != null ? esc(t.didHex + ' ' + (t.didName || '')) : '0x' + hex2(t.sid) + ' ' + esc(t.sidName || '')}</span>` +
          `<span class="mono" title="${t.request.length} B">${esc(hexs(t.request.slice(0, 48)))}${t.request.length > 48 ? ' …' : ''}</span>` +
          `<span class="mono">${t.final ? esc(hexs(t.final.slice(0, 48))) + (t.final.length > 48 ? ' …' : '') : '—'}</span>` +
          `<span class="s-${esc(t.status)}">${esc(ST[t.status] || t.status)}${t.nrc != null && t.status === 'nrc' ? ' 0x' + hex2(t.nrc) : ''}${t.events.some(e => e.kind === 'late') ? ' · late' : ''}</span>` +
          `<span>${esc(t.profile.name || t.profile.id || '')}</span></div>`).join('') : '<div class="tl-empty">조건에 맞는 트랜잭션 없음</div>');
      if (VW.sel != null) list.setAttribute('aria-activedescendant', 'tv-' + VW.sel); else list.removeAttribute('aria-activedescendant');
      const t = m && VW.sel != null ? m.txns.find(x => x.id === VW.sel) : null;
      $('tvDet').innerHTML = !t ? '<div class="tl-empty">트랜잭션을 선택하면 기록된 이벤트·바이트가 표시됩니다.</div>' :
        `<div class="tv-dh">#${t.no} · ${esc(t.ecu)} · ${esc(t.route.label)} ${esc(t.route.summary || '')} · 프로필 ${esc(t.profile.name || t.profile.id || '-')}${t.profile.rev != null ? ' r' + t.profile.rev : ''} · P2 ${t.timing.p2 != null ? t.timing.p2 : '-'} / P2* ${t.timing.p2star != null ? t.timing.p2star : '-'} ms${t.snap ? ' · <span class="tt-warn">기록 시작/회전 시점 스냅샷 포함</span>' : ''}</div>` +
        (t.route.fields && t.route.fields.length ? `<div class="tv-cfg"><span class="k">경로 설정 (기록 시점 고정)</span> ${t.route.fields.map(f => esc(f[0]) + ': <b>' + esc(f[1]) + '</b>').join(' · ')}</div>`
          : t.route.values && Object.keys(t.route.values).length ? `<div class="tv-cfg"><span class="k">경로 설정 (기록 시점 고정)</span> ${Object.entries(t.route.values).map(([k2, v2]) => esc(k2) + '=<b>' + esc(v2) + '</b>').join(' · ')}</div>` : '') +
        (t.source ? `<div class="tv-cfg"><span class="k">출처</span> ${esc(t.source)}</div>` : '') +
        `<div class="tv-req"><span class="k">요청 ${t.request.length} B</span> <span class="mono">${esc(hexs(t.request))}</span></div>` +
        '<ol class="tv-ev">' + t.events.map(ev => `<li class="k-${esc(ev.kind)}${ev.ignored ? ' ign' : ''}"><span class="dt">+${ev.t - t.t0} ms</span><span class="kd">${esc(ev.kind)}</span><span>${esc(ev.label || '')}${ev.snap ? ' <i class="tt-warn">snap</i>' : ''}</span>${ev.bytes ? `<span class="mono by">${esc(hexs(ev.bytes))}</span>` : ''}${ev.note ? `<span class="nt">${esc(ev.note)}</span>` : ''}</li>`).join('') + '</ol>' +
        (t.appliedNote ? `<div class="tv-ap">${esc(t.appliedNote)}</div>` : '');
    }
    const vIds = { q: 'tvQ', sid: 'tvSid', did: 'tvDid', nrc: 'tvNrc', status: 'tvStatus', route: 'tvRoute' };
    Object.entries(vIds).forEach(([k, id]) => {
      const el = $(id);
      el.addEventListener(el.tagName === 'SELECT' ? 'change' : 'input', () => {
        VW.F = Object.assign({}, VW.F, { [k]: el.value }); VW.C = compile(VW.F);
        Object.entries(vIds).forEach(([kk, ii]) => { const bad = !!VW.C.errors[kk]; $(ii).setAttribute('aria-invalid', String(bad)); $(ii).classList.toggle('invalid', bad); });
        renderViewer();
      });
    });
    $('tvTime').addEventListener('click', e => { const b = e.target.closest('[data-tm]'); if (!b) return; prefs.viewerTime = b.dataset.tm; savePrefs(); renderViewer(); });
    $('tvList').addEventListener('click', e => { const r = e.target.closest('.tv-r[data-id]'); if (!r) return; VW.sel = +r.dataset.id; renderViewer(); });
    $('tvList').addEventListener('keydown', e => {
      const rows = vis(); if (!rows.length || (e.key !== 'ArrowDown' && e.key !== 'ArrowUp')) return;
      e.preventDefault();
      let i = rows.findIndex(t => t.id === VW.sel); i = Math.max(0, Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)));
      VW.sel = rows[i].id; renderViewer(); const el = $('tv-' + VW.sel); if (el && el.scrollIntoView) el.scrollIntoView({ block: 'nearest' });
    });
    $('tvFile').addEventListener('change', e => { openFile(e.target.files && e.target.files[0]); e.target.value = ''; });
    $('tvClose').addEventListener('click', closeViewer);
    viewer.addEventListener('keydown', e => { if (e.key === 'Escape') { e.preventDefault(); closeViewer(); } });
    $('tlView').addEventListener('click', () => openViewer($('tlView')));

    /* ---------- host wiring ---------- */
    H.setTraceView(view);
    const unsub = H.subscribe(onEngine);
    const unrender = H.onRender(renderToolbar);
    // Global recording activity: project import / ECU delete guards see it; stop() is the user's explicit [기록 중지] path.
    const unact = H.activity && H.activity.register ? H.activity.register('trace-log', {
      list: () => (R.state === 'recording' || R.state === 'starting' || R.state === 'stopping') ? [{ kind: 'recording', label: '연속 기록 ' + (R.seg ? R.seg.id : '') }] : [],
      stop: () => { stop(); },
    }) : null;
    if (H.project && H.project.register) {
      H.project.register('trace', {
        export: () => JSON.parse(JSON.stringify(prefs)),
        defaults: () => ({ v: 1, timeMode: 'abs', viewerTime: 'rel' }), // section absent → this (validated) default
        validate: validatePrefs,
        // Strict: persist FIRST; a storage failure THROWS to the coordinator before prefs change (rollback-safe).
        import: v => { const next = Object.assign({}, v); localStorage.setItem(PKEY, JSON.stringify(next)); prefs = next; H.refresh(); renderToolbar(); renderViewer(); },
      });
    }
    renderToolbar(); renderRec();

    window.LoomUdsTrace = {
      LIMITS, LOG_FORMAT, compile, parseLog, cloneTxn, ready,
      filters: { get: () => Object.assign({}, F), set: patch => setFilters(patch || {}), clear: clearFilters, compiled: () => C },
      pause, resume, isPaused: () => paused, view, last: () => Object.assign({}, last),
      rec: { start, stop, flush: () => flush(), state: () => R.state, seg: () => (R.seg ? Object.assign({}, R.seg) : null), segs: () => R.segs.slice(), refresh: refreshSegs, select: id => { R.sel = id; renderSegs(); }, exportSeg, deleteSeg, text: segText, err: () => R.err, notice: () => R.notice, storeKind: () => R.storeKind, queue: () => R.q, buffered: () => R.buf.length },
      viewer: { open: openViewer, close: closeViewer, loadText, openSegment, model: () => VW.model, rows: vis, setFilters: f => { VW.F = Object.assign({}, VW.F, f); VW.C = compile(VW.F); renderViewer(); }, select: id => { VW.sel = id; renderViewer(); }, sel: () => VW.sel, err: () => VW.err },
      prefs: () => Object.assign({}, prefs),
      destroy() { if (R.flushH) clock.clear(R.flushH); try { unsub && unsub(); unrender && unrender(); unact && unact(); } catch (e) { /* ignore */ } H.setTraceView(null); },
    };
  }
  if (window.LoomWorkbench) boot();
  else { d.addEventListener('uds:host-ready', boot); window.addEventListener('uds:host-ready', boot); }
})();
