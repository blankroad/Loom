/* Loom UDS Read DID — local simulation engine (no DOM, no network).
   Everything here is a LOCAL MOCK: fixture ECUs, fixture DIDs, mock routes. The raw bytes are UDS PDUs only;
   no CAN-TP segmentation, DoIP headers or vendor EthDiag framing is modelled or claimed. */
(function (root) {
  'use strict';

  const hex2 = n => (n & 0xff).toString(16).toUpperCase().padStart(2, '0');
  const hex4 = n => (n & 0xffff).toString(16).toUpperCase().padStart(4, '0');
  const hexs = b => Array.from(b || [], hex2).join(' ');

  /* ---------- fixtures (clearly synthetic) ---------- */
  const ECUS = [
    { id: 'BCM', name: 'Body Control Module', doipLA: 0x1001, canTx: 0x740, canRx: 0x748 },
    { id: 'GW', name: 'Central Gateway', doipLA: 0x1010, canTx: 0x7E0, canRx: 0x7E8 },
  ];
  const TESTER_LA = 0x0E00;
  const TRANSPORTS = [
    { id: 'doip', label: 'DoIP' },
    { id: 'cantp', label: 'CAN-TP' },
    { id: 'ethdiag', label: 'EthDiag' },
  ];
  // Fixture DID catalogue. Decoders are fixture-defined (not ODX).
  const DIDS = [
    { did: 0xF190, name: 'VIN', long: 'Vehicle Identification Number', enc: 'ascii', len: 17 },
    { did: 0xF18C, name: 'ECU Serial Number', long: 'ECU serial number (fixture)', enc: 'ascii' },
    { did: 0x0101, name: 'Supply Voltage', long: 'Supply voltage — synthetic fixture DID', enc: 'u16', scale: 0.01, unit: 'V', len: 2, synthetic: true },
  ];
  const ECU_DATA = {
    BCM: { 0xF190: 'LOOMSIM0000000001', 0xF18C: 'SIM-BCM-000417', 0x0101: 1255 },
    GW: { 0xF190: 'LOOMSIM0000000001', 0xF18C: 'SIM-GW-000233', 0x0101: 1302 },
  };
  const NRC = {
    0x10: 'generalReject', 0x11: 'serviceNotSupported', 0x13: 'incorrectMessageLengthOrInvalidFormat',
    0x14: 'responseTooLong', 0x22: 'conditionsNotCorrect', 0x31: 'requestOutOfRange',
    0x33: 'securityAccessDenied', 0x78: 'requestCorrectlyReceived-ResponsePending',
  };
  const SIM_NRCS = [0x22, 0x31, 0x33];
  const TIMING = { p2: 1000, p2star: 5000 }; // SIM-scaled client timeouts (ms)
  const SCENARIOS = ['positive', 'nrc', 'pending', 'timeout', 'disconnect'];
  const LATENCIES = [80, 400, 900]; // all < P2
  const PENDING_INTERVAL = 1200;    // < P2*
  const LATE_AFTER = 1200;          // late reply arrives P2 + this

  const ecu = id => ECUS.find(e => e.id === id);
  const didInfo = d => DIDS.find(x => x.did === d) || null;

  function route(ecuId, tr) {
    const e = ecu(ecuId);
    if (!e) return null;
    if (tr === 'doip') return {
      transport: 'doip', label: 'DoIP', link: 'eth', channel: 'ETH1 (SIM)', host: '192.168.10.20:13400',
      testerLA: TESTER_LA, ecuLA: e.doipLA,
      summary: `ETH1 · 0x${hex4(TESTER_LA)}→0x${hex4(e.doipLA)}`,
      fields: [['Ethernet 채널', 'ETH1 (SIM)'], ['DoIP 엔티티', '192.168.10.20:13400 (mock)'], ['Tester 논리 주소', '0x' + hex4(TESTER_LA)], ['ECU 논리 주소', '0x' + hex4(e.doipLA)]],
    };
    if (tr === 'cantp') return {
      transport: 'cantp', label: 'CAN-TP', link: 'can', channel: 'CAN1 500 kbit/s (SIM)', txId: e.canTx, rxId: e.canRx,
      summary: `CAN1 · 0x${e.canTx.toString(16).toUpperCase()}→0x${e.canRx.toString(16).toUpperCase()}`,
      fields: [['CAN 버스', 'CAN1 · 500 kbit/s (SIM)'], ['요청 ID (Tx)', '0x' + e.canTx.toString(16).toUpperCase()], ['응답 ID (Rx)', '0x' + e.canRx.toString(16).toUpperCase()], ['주소 지정', 'normal 11-bit · physical (mock)']],
    };
    if (tr === 'ethdiag') return {
      transport: 'ethdiag', label: 'EthDiag', link: 'eth', channel: 'ETH1 (SIM)', adapter: '벤더 Ethernet 진단 어댑터 (mock)',
      summary: `ETH1 · 벤더 어댑터 (mock) · ${e.id}`,
      fields: [['Ethernet 채널', 'ETH1 (SIM)'], ['어댑터', '벤더 Ethernet 진단 어댑터 (mock)'], ['대상', `${e.id} (어댑터 내부 주소 지정 — 모델링 안 함)`], ['프레이밍', '미정의 · ISO 13400 DoIP 아님']],
    };
    return null;
  }

  /* ---------- validation / PDU ---------- */
  function validateDid(text) {
    const raw = String(text == null ? '' : text);
    let s = raw.replace(/[\s_]/g, '');
    if (/^0x/i.test(s)) s = s.slice(2);
    if (!s) return { ok: false, error: 'DID를 입력하세요 (16비트 hex, 예: F190)' };
    if (!/^[0-9a-fA-F]+$/.test(s)) return { ok: false, error: '16진수 문자만 사용할 수 있습니다 (0-9, A-F)' };
    if (s.length > 4) return { ok: false, error: `${s.length}자리 — DID는 16비트(4자리)입니다. SID 22는 자동으로 붙습니다` };
    if (s.length % 2) return { ok: false, error: `홀수 자릿수(${s.length}) — 4자리 hex가 필요합니다 (예: F190)` };
    if (s.length < 4) return { ok: false, error: '4자리 hex가 필요합니다 (0000–FFFF)' };
    const did = parseInt(s, 16);
    return { ok: true, did, norm: hex4(did) };
  }
  const buildRequest = did => [0x22, (did >> 8) & 0xff, did & 0xff];

  // Decode one UDS PDU in the context of a Read DID transaction. Returns fields with byte offsets.
  function decodePdu(bytes, did) {
    const b = Array.from(bytes || []);
    const f = [];
    if (!b.length) return { kind: 'empty', fields: f };
    const sid = b[0];
    if (sid === 0x22) {
      f.push({ name: 'SID', off: 0, len: 1, value: '0x22', note: 'ReadDataByIdentifier 요청' });
      if (b.length >= 3) f.push({ name: 'DID', off: 1, len: 2, value: '0x' + hex4((b[1] << 8) | b[2]), note: (didInfo((b[1] << 8) | b[2]) || {}).name || '카탈로그에 없음' });
      return { kind: 'request', fields: f };
    }
    if (sid === 0x7F) {
      f.push({ name: 'SID-NR', off: 0, len: 1, value: '0x7F', note: '부정 응답' });
      if (b.length > 1) f.push({ name: 'SIDRQ-NR', off: 1, len: 1, value: '0x' + hex2(b[1]), note: b[1] === 0x22 ? 'ReadDataByIdentifier' : '요청 SID 불일치' });
      if (b.length > 2) f.push({ name: 'NRC', off: 2, len: 1, value: '0x' + hex2(b[2]), note: NRC[b[2]] || 'unknown' });
      return { kind: b[2] === 0x78 ? 'pending' : 'nrc', nrc: b[2], fields: f };
    }
    if (sid === 0x62) {
      f.push({ name: 'SID-PR', off: 0, len: 1, value: '0x62', note: '긍정 응답 (0x22 + 0x40)' });
      const echo = b.length >= 3 ? ((b[1] << 8) | b[2]) : -1;
      const echoOk = echo === did;
      f.push({ name: 'DID', off: 1, len: 2, value: echo >= 0 ? '0x' + hex4(echo) : '—', note: echoOk ? '요청 DID와 일치' : '요청 DID와 불일치', bad: !echoOk });
      const data = b.slice(3);
      const info = didInfo(echo);
      const dv = decodeValue(info, data);
      f.push({ name: info ? info.name : 'dataRecord', off: 3, len: data.length, value: dv.text, unit: dv.unit, note: dv.note, bad: !dv.ok, decoded: true });
      return { kind: 'positive', echoOk, value: dv, fields: f };
    }
    f.push({ name: 'SID', off: 0, len: 1, value: '0x' + hex2(sid), note: '예상하지 않은 SID', bad: true });
    return { kind: 'unexpected', fields: f };
  }
  function decodeValue(info, data) {
    if (!info) return { ok: true, text: hexs(data), unit: '', note: `원시 데이터 ${data.length} B (카탈로그 디코더 없음)` };
    if (info.enc === 'ascii') {
      const printable = data.every(x => x >= 0x20 && x < 0x7f);
      const lenOk = !info.len || data.length === info.len;
      return { ok: printable && lenOk, text: String.fromCharCode.apply(null, data), unit: '', note: `ASCII ${data.length} B · fixture 디코더` + (lenOk ? '' : ` (기대 ${info.len} B)`) };
    }
    if (info.enc === 'u16') {
      if (data.length !== 2) return { ok: false, text: hexs(data), unit: '', note: `길이 ${data.length} B (기대 2 B)` };
      const raw = (data[0] << 8) | data[1];
      return { ok: true, text: (raw * info.scale).toFixed(2), unit: info.unit, note: `u16 0x${hex4(raw)} × ${info.scale} · fixture 디코더 (ODX 아님)` };
    }
    return { ok: true, text: hexs(data), unit: '', note: '' };
  }

  /* ---------- engine ---------- */
  function createEngine(opts) {
    opts = opts || {};
    const clock = opts.clock || { now: () => Date.now(), set: (fn, ms) => setTimeout(fn, ms), clear: h => clearTimeout(h) };
    const LIMIT = opts.historyLimit || 200;
    const listeners = [];
    const ctxs = new Map();       // key ecu|transport -> { link, gen, active }
    const txns = [];              // history (oldest first)
    const confirmed = new Map();  // key ecu|did -> entry
    const counters = { BCM: 0, GW: 0 };
    let seq = 0;

    const ckey = (e, t) => e + '|' + t;
    function ctx(e, t) {
      const k = ckey(e, t);
      if (!ctxs.has(k)) ctxs.set(k, { key: k, ecu: e, transport: t, link: 'offline', gen: 0, active: null });
      return ctxs.get(k);
    }
    const emit = (type, data) => listeners.forEach(fn => { try { fn(type, data); } catch (e) { /* listener isolation */ } });
    const byId = id => txns.find(x => x.id === id) || null;
    const terminal = s => s !== 'waiting' && s !== 'pending';

    function connect(e, t) {
      const c = ctx(e, t);
      if (!route(e, t)) return { ok: false, error: 'unknown route' };
      if (c.link === 'online') return { ok: true, ctx: c };
      c.link = 'online'; c.gen += 1; c.since = clock.now();
      emit('link', { ctx: c });
      return { ok: true, ctx: c };
    }
    function disconnect(e, t, cause) {
      const c = ctx(e, t);
      if (c.link !== 'online') return { ok: false, error: 'offline' };
      const tx = c.active ? byId(c.active) : null;
      c.link = 'offline'; c.gen += 1; c.cause = cause || 'user';
      if (tx) finish(tx, 'link-lost', { kind: 'link', label: cause === 'sim' ? '링크 끊김 (SIM 시나리오)' : '링크 끊김 (사용자 연결 해제)', note: '응답 없음 — 이후 이 연결 세대의 응답은 도착할 수 없음' });
      emit('link', { ctx: c });
      return { ok: true };
    }

    function read(req) {
      const v = validateDid(req.didText);
      if (!v.ok) return { ok: false, error: v.error, code: 'invalid' };
      const e = ecu(req.ecu), r = route(req.ecu, req.transport);
      if (!e || !r) return { ok: false, error: '알 수 없는 ECU/경로', code: 'route' };
      const c = ctx(req.ecu, req.transport);
      if (c.link !== 'online') return { ok: false, error: '경로가 연결되어 있지 않습니다', code: 'offline' };
      if (c.active) return { ok: false, error: `이 대상·경로에서 요청 #${byId(c.active).no} 진행 중`, code: 'busy' };
      const s = req.sim || {};
      const sim = {
        scenario: SCENARIOS.includes(s.scenario) ? s.scenario : 'positive',
        latency: LATENCIES.includes(+s.latency) ? +s.latency : 400,
        nrc: SIM_NRCS.includes(+s.nrc) ? +s.nrc : 0x22,
        pending: Math.max(1, Math.min(5, +s.pending || 2)),
        pendingInterval: PENDING_INTERVAL,
        late: !!s.late,
        p2: TIMING.p2, p2star: TIMING.p2star,
      };
      const t0 = clock.now();
      const info = didInfo(v.did);
      const tx = {
        id: ++seq, no: seq, ecu: e.id, ecuName: e.name, transport: r.transport,
        route: JSON.parse(JSON.stringify(r)), gen: c.gen, ctxKey: c.key,
        did: v.did, didHex: v.norm, didName: info ? info.name : null,
        request: buildRequest(v.did), sim, t0, tEnd: null,
        status: 'waiting', pendingCount: 0, final: null, events: [],
        applied: null, appliedNote: '', timers: [], deadline: null,
      };
      tx.events.push({ t: t0, kind: 'tx', bytes: tx.request.slice(), label: 'Tx 요청' });
      c.active = tx.id;
      txns.push(tx);
      evict();
      arm(tx, sim.p2);
      plan(tx);
      emit('txn', { txn: tx, created: true });
      return { ok: true, txn: tx };
    }

    function ecuFinal(tx) {
      // Simulated ECU behaviour for a "normal" final reply.
      const data = ECU_DATA[tx.ecu];
      if (!(tx.did in data)) return [0x7F, 0x22, 0x31];
      const info = didInfo(tx.did);
      let payload;
      if (info.enc === 'ascii') payload = Array.from(String(data[tx.did]), ch => ch.charCodeAt(0));
      else { const raw = data[tx.did] + (counters[tx.ecu]++ % 7); payload = [(raw >> 8) & 0xff, raw & 0xff]; }
      return [0x62, tx.request[1], tx.request[2]].concat(payload);
    }

    function plan(tx) {
      const s = tx.sim, gen = tx.gen, at = (ms, fn) => tx.timers.push(clock.set(() => {
        const c = ctxs.get(tx.ctxKey);
        if (!c || c.gen !== gen) return; // connection generation gone: nothing can arrive
        fn();
      }, ms));
      if (s.scenario === 'positive') at(s.latency, () => deliver(tx, ecuFinal(tx)));
      else if (s.scenario === 'nrc') at(s.latency, () => deliver(tx, [0x7F, 0x22, s.nrc]));
      else if (s.scenario === 'pending') {
        for (let i = 0; i < s.pending; i++) at(s.latency + i * s.pendingInterval, () => deliver(tx, [0x7F, 0x22, 0x78]));
        at(s.latency + s.pending * s.pendingInterval, () => deliver(tx, ecuFinal(tx)));
      } else if (s.scenario === 'timeout') {
        if (s.late) at(s.p2 + LATE_AFTER, () => deliver(tx, ecuFinal(tx)));
      } else if (s.scenario === 'disconnect') at(s.latency, () => disconnect(tx.ecu, tx.transport, 'sim'));
    }

    function arm(tx, ms) {
      if (tx.deadline) clock.clear(tx.deadline.h);
      const due = clock.now() + ms;
      tx.deadline = { due, ms, h: clock.set(() => { if (!terminal(tx.status)) onTimeout(tx); }, ms) };
    }
    function onTimeout(tx) {
      const which = tx.pendingCount ? `P2* ${tx.sim.p2star} ms` : `P2 ${tx.sim.p2} ms`;
      finish(tx, 'timeout', { kind: 'timeout', label: '타임아웃', note: `${which} 안에 최종 응답 없음 — 응답 바이트 없음, 자동 재시도 없음` });
    }

    function deliver(tx, bytes) {
      const t = clock.now();
      const d = decodePdu(bytes, tx.did);
      if (terminal(tx.status)) {
        tx.events.push({ t, kind: 'late', bytes, label: 'Rx 늦은 응답', note: `${statusText(tx)} 이후 수신 — 무시됨 (상태·확정 값 변경 없음)`, ignored: true, dk: d.kind });
        emit('txn', { txn: tx, late: true });
        return;
      }
      if (d.kind === 'pending') {
        tx.pendingCount += 1; tx.status = 'pending';
        tx.events.push({ t, kind: 'pending', bytes, label: `Rx 응답 대기 (0x78) #${tx.pendingCount}`, note: `P2* ${tx.sim.p2star} ms 타이머 재시작` });
        arm(tx, tx.sim.p2star);
        emit('txn', { txn: tx });
        return;
      }
      if (d.kind === 'positive') {
        tx.final = bytes.slice();
        finish(tx, 'positive', { kind: 'positive', bytes, label: 'Rx 긍정 응답' }, true);
        applyConfirmed(tx, d);
        emit('txn', { txn: tx });
        return;
      }
      tx.final = bytes.slice();
      tx.nrc = d.nrc;
      finish(tx, 'nrc', { kind: 'nrc', bytes, label: `Rx 부정 응답 0x${hex2(d.nrc)}`, note: NRC[d.nrc] || '' });
    }

    function finish(tx, status, ev, silent) {
      if (terminal(tx.status)) return;
      const t = clock.now();
      ev.t = t;
      tx.events.push(ev);
      tx.status = status; tx.tEnd = t;
      if (tx.deadline) { clock.clear(tx.deadline.h); tx.deadline = null; }
      const c = ctxs.get(tx.ctxKey);
      if (c && c.active === tx.id) c.active = null;
      if (status !== 'positive') tx.applied = false;
      if (!silent) emit('txn', { txn: tx });
    }

    function applyConfirmed(tx, d) {
      const key = tx.ecu + '|' + tx.did;
      const cur = confirmed.get(key);
      if (!d.echoOk || !d.value.ok) { tx.applied = false; tx.appliedNote = '응답 검증 실패 — 확정 값에 반영하지 않음'; return; }
      if (cur && cur.no > tx.no) { tx.applied = false; tx.appliedNote = `더 최근 요청 #${cur.no}의 확정 값이 이미 있어 반영하지 않음`; return; }
      confirmed.set(key, {
        key, ecu: tx.ecu, did: tx.did, didHex: tx.didHex, didName: tx.didName,
        value: d.value.text, unit: d.value.unit, data: tx.final.slice(3), txnId: tx.id, no: tx.no,
        t: tx.tEnd, transport: tx.transport, routeLabel: tx.route.label, routeSummary: tx.route.summary,
      });
      tx.applied = true; tx.appliedNote = '확정 값으로 반영됨';
    }

    function cancel(e, t) {
      const c = ctx(e, t);
      const tx = c.active ? byId(c.active) : null;
      if (!tx) return { ok: false, error: '진행 중 요청 없음' };
      finish(tx, 'cancelled', { kind: 'cancel', label: '사용자 취소', note: '테스터 측 대기 중단 — 이후 도착하는 응답은 무시됨' });
      return { ok: true, txn: tx };
    }

    function evict() {
      while (txns.length > LIMIT) {
        const i = txns.findIndex(x => terminal(x.status));
        if (i < 0) break;
        txns.splice(i, 1);
      }
    }
    function clearHistory() {
      for (let i = txns.length - 1; i >= 0; i--) if (terminal(txns[i].status)) txns.splice(i, 1);
      emit('clear', {});
    }

    function statusText(tx) {
      return ({ waiting: '응답 대기', pending: '응답 대기(0x78)', positive: '긍정', nrc: 'NRC', timeout: '타임아웃', cancelled: '취소', 'link-lost': '링크 끊김' })[tx.status] || tx.status;
    }

    function exportRows() {
      return txns.map(tx => ({
        no: tx.no, t0: new Date(tx.t0).toISOString(), ecu: tx.ecu, transport: tx.route.label, route: tx.route.summary,
        did: tx.didHex, didName: tx.didName || '', request: hexs(tx.request), finalResponse: tx.final ? hexs(tx.final) : '',
        status: tx.status, nrc: tx.nrc != null ? '0x' + hex2(tx.nrc) : '', pendingCount: tx.pendingCount,
        elapsedMs: tx.tEnd != null ? tx.tEnd - tx.t0 : null, appliedToConfirmed: !!tx.applied, simScenario: tx.sim.scenario,
        events: tx.events.map(ev => ({ dtMs: ev.t - tx.t0, kind: ev.kind, bytes: ev.bytes ? hexs(ev.bytes) : '', ignored: !!ev.ignored, note: ev.note || '' })),
      }));
    }

    return {
      connect, disconnect, read, cancel, clearHistory, exportRows,
      ctx, txns, confirmed, byId, statusText, terminal,
      on: fn => listeners.push(fn),
      activeCount: () => txns.filter(x => !terminal(x.status)).length,
      limit: LIMIT,
    };
  }

  root.LoomUds = {
    createEngine, validateDid, buildRequest, decodePdu, route,
    ECUS, DIDS, TRANSPORTS, NRC, SIM_NRCS, TIMING, SCENARIOS, LATENCIES, PENDING_INTERVAL, LATE_AFTER,
    hex2, hex4, hexs, didInfo,
  };
})(typeof window !== 'undefined' ? window : globalThis);
