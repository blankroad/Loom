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
  const LATE_AFTER = 1200;          // late reply arrives (configured) P2 + this
  const CONFIG_FORMAT = 'loom-uds-read-did-config/1';
  const MAX_PROFILES = 60;

  const ecu = id => ECUS.find(e => e.id === id);
  const didInfo = d => DIDS.find(x => x.did === d) || null;

  /* ---------- connection profiles (per ECU × route; presets read-only) ---------- */
  const CHANNELS = { eth: ['ETH1', 'ETH2'], can: ['CAN1', 'CAN2'] }; // SIM channels only
  const BITRATES = [125000, 250000, 500000, 1000000];                // CAN-TP metadata (no bit timing simulated)
  const LIMITS = { p2: [50, 30000], p2star: [100, 60000], port: [1, 65535], name: 32, adapter: 40, endpoint: 64 };
  const FIELDS = {
    doip: ['channel', 'host', 'port', 'testerLA', 'ecuLA', 'p2', 'p2star'],
    cantp: ['channel', 'bitrate', 'idFormat', 'txId', 'rxId', 'p2', 'p2star'],
    ethdiag: ['channel', 'adapter', 'endpoint', 'p2', 'p2star'],
  };
  const linkOf = tr => (tr === 'cantp' ? 'can' : 'eth');
  const trLabel = tr => (TRANSPORTS.find(t => t.id === tr) || {}).label || tr;
  const presetId = (e, t) => `preset:${e}|${t}`;
  function presetValues(ecuId, tr) {
    const e = ecu(ecuId);
    if (!e) return null;
    const tm = { p2: TIMING.p2, p2star: TIMING.p2star };
    if (tr === 'doip') return Object.assign({ channel: 'ETH1', host: '192.168.10.20', port: 13400, testerLA: TESTER_LA, ecuLA: e.doipLA }, tm);
    if (tr === 'cantp') return Object.assign({ channel: 'CAN1', bitrate: 500000, idFormat: '11', txId: e.canTx, rxId: e.canRx }, tm);
    if (tr === 'ethdiag') return Object.assign({ channel: 'ETH1', adapter: '벤더 Ethernet 진단 어댑터', endpoint: '' }, tm);
    return null;
  }
  const PRESETS = new Map();
  function presetProfile(ecuId, tr) {
    const id = presetId(ecuId, tr);
    if (!PRESETS.has(id)) {
      const v = presetValues(ecuId, tr);
      if (!v) return null;
      PRESETS.set(id, Object.freeze({ id, ecu: ecuId, transport: tr, name: `프리셋 ${ecuId} ${trLabel(tr)}`, source: 'preset', base: id, rev: 0, values: Object.freeze(v) }));
    }
    return PRESETS.get(id);
  }

  const CTRL = /[\u0000-\u001f\u007f]/;
  function hostError(h) {
    if (!h) return '호스트(IPv4 또는 이름)를 입력하세요';
    if (/\s/.test(h)) return '공백 불가';
    if (h.includes(':') || h.includes('[')) return 'IPv6·포트 포함 형식 미지원 — IPv4 또는 호스트 이름만 (포트는 별도 입력)';
    if (/^[0-9.]+$/.test(h)) {
      const p = h.split('.');
      if (p.length !== 4 || p.some(x => !/^\d{1,3}$/.test(x))) return 'IPv4는 a.b.c.d 네 부분이어야 합니다';
      if (p.some(x => x.length > 1 && x[0] === '0')) return 'IPv4 각 부분에 선행 0 불가 (모호함)';
      if (p.some(x => +x > 255)) return 'IPv4 각 부분은 0–255';
      return null;
    }
    if (h.length > 253) return '호스트 이름 253자 초과';
    const labels = h.replace(/\.$/, '').split('.');
    for (const l of labels) {
      if (!l) return '빈 레이블(연속된 점) 불가';
      if (l.length > 63) return '레이블 63자 초과';
      if (!/^[A-Za-z0-9-]+$/.test(l)) return '호스트 이름은 영문·숫자·하이픈만 (IDN·밑줄 불가)';
      if (l[0] === '-' || l[l.length - 1] === '-') return '레이블은 하이픈으로 시작/끝날 수 없음';
    }
    if (/^\d+$/.test(labels[labels.length - 1])) return '마지막 레이블이 숫자뿐 — IPv4 형식을 확인하세요';
    return null;
  }
  // Strict integer: number that is a safe integer, or a string of decimal digits. No sign, decimals, exponent, hex.
  function parseIntStrict(v) {
    if (typeof v === 'number') return Number.isSafeInteger(v) && v >= 0 ? { ok: true, n: v } : { ok: false, error: '정수만 (소수·음수 불가)' };
    const s = String(v == null ? '' : v).trim();
    if (!s) return { ok: false, error: '값을 입력하세요' };
    if (!/^\d+$/.test(s)) return { ok: false, error: /^-?\d*\.\d+$|^-?\d+\.$/.test(s) ? '정수만 (소수 불가)' : '10진 정수만' };
    if (s.length > 9) return { ok: false, error: '범위 초과' };
    return { ok: true, n: parseInt(s, 10) };
  }
  // Hex: number (integer) or string with optional 0x; never truncates.
  function parseHexStrict(v, maxDigits) {
    if (typeof v === 'number') return Number.isSafeInteger(v) && v >= 0 ? { ok: true, n: v } : { ok: false, error: '정수 hex 값이 아님' };
    let s = String(v == null ? '' : v).trim();
    if (/^0x/i.test(s)) s = s.slice(2);
    if (!s) return { ok: false, error: 'hex 값을 입력하세요' };
    if (!/^[0-9a-fA-F]+$/.test(s)) return { ok: false, error: '16진수 문자만 (0-9, A-F)' };
    if (s.length > maxDigits) return { ok: false, error: `${s.length}자리 — 최대 ${maxDigits}자리 (잘라내지 않음)` };
    return { ok: true, n: parseInt(s, 16) };
  }
  const hexId = (n, fmt) => '0x' + (fmt === '29' ? n.toString(16).toUpperCase().padStart(8, '0') : n.toString(16).toUpperCase().padStart(3, '0'));
  function textError(s, max, required) {
    if (required && !s) return '값을 입력하세요';
    if (s.length > max) return `${max}자 이하`;
    if (CTRL.test(s)) return '제어 문자 불가';
    return null;
  }

  // Validate a profile draft for transport tr. Input values may be strings (form) or numbers (stored/imported).
  function validateProfile(tr, inp) {
    const f = FIELDS[tr];
    if (!f) return { ok: false, errors: { _: '알 수 없는 경로' }, values: null };
    inp = inp && typeof inp === 'object' ? inp : {};
    const errors = {}, out = {};
    const str = k => String(inp[k] == null ? '' : inp[k]).trim();
    const chans = CHANNELS[linkOf(tr)];
    out.channel = str('channel');
    if (!chans.includes(out.channel)) errors.channel = `SIM 채널 ${chans.join('/')} 중 선택`;
    const intIn = (k, lo, hi) => {
      const r = parseIntStrict(inp[k]);
      if (!r.ok) errors[k] = r.error;
      else if (r.n < lo || r.n > hi) errors[k] = `${lo}–${hi} 범위`;
      else out[k] = r.n;
    };
    if (tr === 'doip') {
      out.host = str('host');
      const he = hostError(out.host); if (he) errors.host = he;
      intIn('port', LIMITS.port[0], LIMITS.port[1]);
      ['testerLA', 'ecuLA'].forEach(k => { const r = parseHexStrict(inp[k], 4); if (!r.ok) errors[k] = r.error; else if (r.n > 0xffff) errors[k] = '16비트 초과'; else out[k] = r.n; });
      if (!errors.testerLA && !errors.ecuLA && out.testerLA === out.ecuLA) errors.ecuLA = 'Tester 논리 주소와 같을 수 없음';
    } else if (tr === 'cantp') {
      const b = parseIntStrict(inp.bitrate);
      if (!b.ok || !BITRATES.includes(b.n)) errors.bitrate = '125k/250k/500k/1M 중 선택'; else out.bitrate = b.n;
      out.idFormat = str('idFormat');
      if (out.idFormat !== '11' && out.idFormat !== '29') errors.idFormat = '11-bit 또는 29-bit';
      const max = out.idFormat === '29' ? 0x1FFFFFFF : 0x7FF;
      ['txId', 'rxId'].forEach(k => {
        const r = parseHexStrict(inp[k], 8);
        if (!r.ok) errors[k] = r.error;
        else if (errors.idFormat) out[k] = r.n;
        else if (r.n > max) errors[k] = out.idFormat === '29' ? '29-bit 최대 0x1FFFFFFF' : '11-bit 최대 0x7FF (29-bit 형식 선택 필요)';
        else out[k] = r.n;
      });
      if (!errors.txId && !errors.rxId && out.txId === out.rxId) errors.rxId = '요청 ID와 같을 수 없음';
    } else if (tr === 'ethdiag') {
      out.adapter = str('adapter'); out.endpoint = str('endpoint');
      const ae = textError(out.adapter, LIMITS.adapter, true); if (ae) errors.adapter = ae;
      const ee = textError(out.endpoint, LIMITS.endpoint, false); if (ee) errors.endpoint = ee;
    }
    intIn('p2', LIMITS.p2[0], LIMITS.p2[1]);
    intIn('p2star', LIMITS.p2star[0], LIMITS.p2star[1]);
    if (!errors.p2 && !errors.p2star && out.p2star < out.p2) errors.p2star = 'P2* ≥ P2 이어야 함';
    const ok = !Object.keys(errors).length;
    return { ok, errors, values: ok ? out : null };
  }
  const sameValues = (tr, a, b) => FIELDS[tr].every(k => a[k] === b[k]);
  const kbit = b => (b >= 1000000 ? b / 1000000 + ' Mbit/s' : b / 1000 + ' kbit/s');

  // Build the route view for an ECU from a profile. This object is what gets snapshotted per connection / request.
  function routeFromProfile(ecuId, p) {
    const e = ecu(ecuId);
    if (!e || !p) return null;
    const v = p.values, tr = p.transport;
    const base = {
      transport: tr, label: trLabel(tr), link: linkOf(tr),
      profile: { id: p.id, name: p.name, source: p.source, base: p.base, rev: p.rev },
      ptag: p.source === 'preset' ? '프리셋' : `사용자 "${p.name}" r${p.rev}`,
      timing: { p2: v.p2, p2star: v.p2star },
      values: Object.assign({}, v),
    };
    if (tr === 'doip') return Object.assign(base, {
      channel: `${v.channel} (SIM)`, host: `${v.host}:${v.port}`, testerLA: v.testerLA, ecuLA: v.ecuLA,
      summary: `${v.channel} · 0x${hex4(v.testerLA)}→0x${hex4(v.ecuLA)}`,
      fields: [['Ethernet 채널', `${v.channel} (SIM)`], ['DoIP 엔티티', `${v.host}:${v.port} (mock)`], ['Tester 논리 주소', '0x' + hex4(v.testerLA)], ['ECU 논리 주소', '0x' + hex4(v.ecuLA)]],
    });
    if (tr === 'cantp') return Object.assign(base, {
      channel: `${v.channel} ${kbit(v.bitrate)} (SIM)`, txId: v.txId, rxId: v.rxId, idFormat: v.idFormat,
      summary: `${v.channel} · ${hexId(v.txId, v.idFormat)}→${hexId(v.rxId, v.idFormat)}`,
      fields: [['CAN 버스', `${v.channel} · ${kbit(v.bitrate)} (SIM)`], ['요청 ID (Tx)', hexId(v.txId, v.idFormat)], ['응답 ID (Rx)', hexId(v.rxId, v.idFormat)], ['주소 지정', `normal ${v.idFormat}-bit · physical (mock)`]],
    });
    if (tr === 'ethdiag') return Object.assign(base, {
      channel: `${v.channel} (SIM)`, adapter: `${v.adapter} (mock)`, endpoint: v.endpoint,
      summary: `${v.channel} · 벤더 어댑터 (mock) · ${e.id}`,
      fields: [['Ethernet 채널', `${v.channel} (SIM)`], ['어댑터', `${v.adapter} (mock)`]].concat(v.endpoint ? [['엔드포인트 메모', `${v.endpoint} (해석 안 함)`]] : [])
        .concat([['대상', `${e.id} (어댑터 내부 주소 지정 — 모델링 안 함)`], ['프레이밍', '미정의 · ISO 13400 DoIP 아님']]),
    });
    return null;
  }
  // Preset route (compatibility helper: what each context starts with).
  const route = (ecuId, tr) => routeFromProfile(ecuId, presetProfile(ecuId, tr));

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
    /* connection profile store: presets are built-in & immutable; customs scoped to one ECU×route */
    const customs = new Map();    // id -> custom profile
    const active = new Map();     // ctx key -> custom id (absent = preset)
    let nextPid = 1;

    const ckey = (e, t) => e + '|' + t;
    function ctx(e, t) {
      const k = ckey(e, t);
      if (!ctxs.has(k)) ctxs.set(k, { key: k, ecu: e, transport: t, link: 'offline', gen: 0, active: null });
      return ctxs.get(k);
    }
    const emit = (type, data) => listeners.forEach(fn => { try { fn(type, data); } catch (e) { /* listener isolation */ } });
    const byId = id => txns.find(x => x.id === id) || null;
    const terminal = s => s !== 'waiting' && s !== 'pending';

    /* ---------- profiles ---------- */
    function profile(id) {
      const m = /^preset:([^|]+)\|(\w+)$/.exec(String(id));
      return m ? presetProfile(m[1], m[2]) : customs.get(id) || null;
    }
    const profilesFor = (e, t) => [presetProfile(e, t)].concat([...customs.values()].filter(p => p.ecu === e && p.transport === t));
    function activeProfile(e, t) { const id = active.get(ckey(e, t)); return (id && customs.get(id)) || presetProfile(e, t); }
    // Route the UI should show: the frozen connection snapshot while online, else the active profile.
    function routeFor(e, t) { const c = ctx(e, t); return c.link === 'online' && c.conn ? c.conn.route : routeFromProfile(e, activeProfile(e, t)); }
    const ONLINE_ERR = '연결 중에는 설정을 적용할 수 없습니다 — 먼저 연결 해제 (자동 해제·요청 중단 없음)';
    function nameError(e, t, name, exceptId) {
      const te = textError(name, LIMITS.name, true);
      if (te) return te;
      if (profilesFor(e, t).some(p => p.id !== exceptId && p.name === name)) return '이 ECU·경로에 같은 이름의 프로필이 있음';
      return null;
    }
    function freezeCustom(p) { p.values = Object.freeze(Object.assign({}, p.values)); return Object.freeze(p); }
    // Save draft & apply as the context's active profile. Refused (no mutation) while online or invalid.
    function saveProfile(e, t, draft) {
      draft = draft || {};
      if (!ecu(e) || !FIELDS[t]) return { ok: false, code: 'route', error: '알 수 없는 ECU/경로' };
      const c = ctx(e, t), k = ckey(e, t);
      if (c.link === 'online') return { ok: false, code: 'online', error: ONLINE_ERR };
      const v = validateProfile(t, draft.values);
      const tid = draft.targetId, tgt = tid === 'new' ? null : profile(tid);
      if (tid !== 'new' && (!tgt || tgt.ecu !== e || tgt.transport !== t)) return { ok: false, code: 'target', error: '이 ECU·경로의 프로필이 아님' };
      const name = String(draft.name == null ? '' : draft.name).trim();
      if (!v.ok) return { ok: false, code: 'invalid', errors: v.errors, error: '입력 오류 — 저장하지 않음' };
      if (tgt && tgt.source === 'preset' && sameValues(t, v.values, tgt.values)) {
        active.delete(k); emit('config', { ctxKey: k });
        return { ok: true, profile: tgt, action: 'preset' };
      }
      const ne = nameError(e, t, name, tgt && tgt.source === 'custom' ? tgt.id : null);
      if (ne) return { ok: false, code: 'invalid', errors: { name: ne }, error: '입력 오류 — 저장하지 않음' };
      let p, action;
      if (tgt && tgt.source === 'custom') {
        p = freezeCustom({ id: tgt.id, ecu: e, transport: t, name, source: 'custom', base: tgt.base, rev: tgt.rev + 1, values: v.values, savedAt: clock.now() });
        action = 'updated';
      } else {
        // Creation cap (same limit as load/import, so nothing saved can be dropped on reload). Checked before any id/store change.
        if (customs.size >= MAX_PROFILES) return { ok: false, code: 'limit', error: `사용자 프로필 최대 ${MAX_PROFILES}개 — 기존 프로필을 삭제하거나 갱신하세요` };
        const base = tgt ? tgt.base : presetId(e, t);
        p = freezeCustom({ id: 'u' + nextPid++, ecu: e, transport: t, name, source: 'custom', base, rev: 1, values: v.values, savedAt: clock.now() });
        action = 'created';
      }
      customs.set(p.id, p); active.set(k, p.id);
      emit('config', { ctxKey: k });
      return { ok: true, profile: p, action };
    }
    function deleteProfile(e, t, id) {
      const p = customs.get(id), k = ckey(e, t);
      if (!p || p.ecu !== e || p.transport !== t) return { ok: false, code: 'target', error: '삭제할 수 있는 사용자 프로필이 아님 (프리셋은 삭제 불가)' };
      const wasActive = active.get(k) === id;
      if (wasActive && ctx(e, t).link === 'online') return { ok: false, code: 'online', error: '적용 중인 프로필은 연결 해제 후 삭제할 수 있습니다' };
      customs.delete(id);
      if (wasActive) active.delete(k);
      emit('config', { ctxKey: k });
      return { ok: true, wasActive };
    }
    const exportProfile = p => ({ id: p.id, ecu: p.ecu, transport: p.transport, name: p.name, base: p.base, rev: p.rev, savedAt: p.savedAt || null, values: Object.assign({}, p.values) });
    function exportConfig() {
      const act = {}; active.forEach((id, k) => { act[k] = id; });
      return { format: CONFIG_FORMAT, profiles: [...customs.values()].map(exportProfile), active: act };
    }
    // Validate one stored/imported profile entry. Returns normalized custom (without id) or error text.
    function checkEntry(x, i) {
      if (!x || typeof x !== 'object') return { error: `#${i + 1}: 객체 아님` };
      if (!ecu(x.ecu) || !FIELDS[x.transport]) return { error: `#${i + 1}: 알 수 없는 ECU/경로` };
      const name = typeof x.name === 'string' ? x.name.trim() : '';
      const ne = textError(name, LIMITS.name, true);
      if (ne) return { error: `#${i + 1} 이름: ${ne}` };
      const v = validateProfile(x.transport, x.values);
      if (!v.ok) { const f = Object.keys(v.errors)[0]; return { error: `#${i + 1} ${f}: ${v.errors[f]}` }; }
      const rev = Number.isSafeInteger(x.rev) && x.rev >= 1 ? x.rev : 1;
      const base = presetId(x.ecu, x.transport);
      return { p: { ecu: x.ecu, transport: x.transport, name, source: 'custom', base, rev, values: v.values, savedAt: typeof x.savedAt === 'number' ? x.savedAt : null } };
    }
    // Boot-time restore from own storage. Partial: invalid entries are dropped and counted (never applied).
    function loadConfig(obj) {
      if ([...ctxs.values()].some(c => c.link === 'online')) return { ok: false, error: 'online' };
      customs.clear(); active.clear();
      let dropped = 0, maxN = 0;
      if (!obj || typeof obj !== 'object' || !Array.isArray(obj.profiles)) return { ok: true, dropped: obj == null ? 0 : 1, loaded: 0 };
      obj.profiles.slice(0, MAX_PROFILES).forEach((x, i) => {
        const r = checkEntry(x, i), id = x && typeof x.id === 'string' && /^u\d{1,6}$/.test(x.id) ? x.id : null;
        if (r.error || !id || customs.has(id) || profilesFor(r.p.ecu, r.p.transport).some(q => q.name === r.p.name)) { dropped++; return; }
        customs.set(id, freezeCustom(Object.assign({ id }, r.p)));
        maxN = Math.max(maxN, +id.slice(1));
      });
      dropped += Math.max(0, obj.profiles.length - MAX_PROFILES);
      nextPid = Math.max(nextPid, maxN + 1);
      if (obj.active && typeof obj.active === 'object') Object.keys(obj.active).forEach(k => {
        const p = customs.get(obj.active[k]);
        if (p && ckey(p.ecu, p.transport) === k) active.set(k, p.id); else dropped++;
      });
      emit('config', {});
      return { ok: true, dropped, loaded: customs.size };
    }
    // User import: whole file validated first, then ALL entries added as new custom profiles (atomic).
    // Never changes active profiles, links or requests.
    function importConfig(obj) {
      if (!obj || typeof obj !== 'object' || obj.format !== CONFIG_FORMAT) return { ok: false, error: `형식이 ${CONFIG_FORMAT} 이 아님` };
      if (!Array.isArray(obj.profiles) || !obj.profiles.length) return { ok: false, error: '프로필 목록이 비어 있음' };
      if (obj.profiles.length + customs.size > MAX_PROFILES) return { ok: false, error: `프로필 최대 ${MAX_PROFILES}개 초과` };
      const add = [];
      for (let i = 0; i < obj.profiles.length; i++) {
        const r = checkEntry(obj.profiles[i], i);
        if (r.error) return { ok: false, error: r.error };
        add.push(r.p);
      }
      let renamed = 0;
      const taken = new Map();
      add.forEach(p => {
        const k = ckey(p.ecu, p.transport);
        if (!taken.has(k)) taken.set(k, new Set(profilesFor(p.ecu, p.transport).map(q => q.name)));
        const set = taken.get(k);
        let n = p.name;
        for (let j = 2; set.has(n); j++) n = (p.name.slice(0, LIMITS.name - 5) + ` (${j})`);
        if (n !== p.name) renamed++;
        p.name = n; set.add(n);
      });
      const ids = add.map(p => { const id = 'u' + nextPid++; customs.set(id, freezeCustom(Object.assign({ id }, p))); return id; });
      emit('config', {});
      return { ok: true, added: ids.length, renamed, ids };
    }

    function connect(e, t) {
      const c = ctx(e, t);
      if (!ecu(e) || !FIELDS[t]) return { ok: false, error: 'unknown route' };
      if (c.link === 'online') return { ok: true, ctx: c };
      // Freeze the active profile for this connection generation; reads use only this snapshot.
      c.conn = { route: JSON.parse(JSON.stringify(routeFromProfile(e, activeProfile(e, t)))) };
      c.link = 'online'; c.gen += 1; c.since = clock.now();
      emit('link', { ctx: c });
      return { ok: true, ctx: c };
    }
    function disconnect(e, t, cause) {
      const c = ctx(e, t);
      if (c.link !== 'online') return { ok: false, error: 'offline' };
      const tx = c.active ? byId(c.active) : null;
      c.link = 'offline'; c.gen += 1; c.cause = cause || 'user'; c.conn = null;
      if (tx) finish(tx, 'link-lost', { kind: 'link', label: cause === 'sim' ? '링크 끊김 (SIM 시나리오)' : '링크 끊김 (사용자 연결 해제)', note: '응답 없음 — 이후 이 연결 세대의 응답은 도착할 수 없음' });
      emit('link', { ctx: c });
      return { ok: true };
    }

    function read(req) {
      const v = validateDid(req.didText);
      if (!v.ok) return { ok: false, error: v.error, code: 'invalid' };
      const e = ecu(req.ecu);
      if (!e || !FIELDS[req.transport]) return { ok: false, error: '알 수 없는 ECU/경로', code: 'route' };
      const c = ctx(req.ecu, req.transport);
      if (c.link !== 'online' || !c.conn) return { ok: false, error: '경로가 연결되어 있지 않습니다', code: 'offline' };
      const r = c.conn.route;
      if (c.active) return { ok: false, error: `이 대상·경로에서 요청 #${byId(c.active).no} 진행 중`, code: 'busy' };
      const s = req.sim || {};
      const sim = {
        scenario: SCENARIOS.includes(s.scenario) ? s.scenario : 'positive',
        latency: LATENCIES.includes(+s.latency) ? +s.latency : 400,
        nrc: SIM_NRCS.includes(+s.nrc) ? +s.nrc : 0x22,
        pending: Math.max(1, Math.min(5, +s.pending || 2)),
        pendingInterval: PENDING_INTERVAL,
        late: !!s.late, lateAfter: LATE_AFTER,
      };
      const t0 = clock.now();
      const info = didInfo(v.did);
      const tx = {
        id: ++seq, no: seq, ecu: e.id, ecuName: e.name, transport: r.transport,
        route: JSON.parse(JSON.stringify(r)), gen: c.gen, ctxKey: c.key,
        timing: { p2: r.timing.p2, p2star: r.timing.p2star }, profile: Object.assign({}, r.profile),
        did: v.did, didHex: v.norm, didName: info ? info.name : null,
        request: buildRequest(v.did), sim, t0, tEnd: null,
        status: 'waiting', pendingCount: 0, final: null, events: [],
        applied: null, appliedNote: '', timers: [], deadline: null,
      };
      tx.events.push({ t: t0, kind: 'tx', bytes: tx.request.slice(), label: 'Tx 요청' });
      c.active = tx.id;
      txns.push(tx);
      evict();
      arm(tx, tx.timing.p2);
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
        if (s.late) at(tx.timing.p2 + s.lateAfter, () => deliver(tx, ecuFinal(tx)));
      } else if (s.scenario === 'disconnect') at(s.latency, () => disconnect(tx.ecu, tx.transport, 'sim'));
    }

    function arm(tx, ms) {
      if (tx.deadline) clock.clear(tx.deadline.h);
      const due = clock.now() + ms;
      tx.deadline = { due, ms, h: clock.set(() => { if (!terminal(tx.status)) onTimeout(tx); }, ms) };
    }
    function onTimeout(tx) {
      const which = tx.pendingCount ? `P2* ${tx.timing.p2star} ms` : `P2 ${tx.timing.p2} ms`;
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
        tx.events.push({ t, kind: 'pending', bytes, label: `Rx 응답 대기 (0x78) #${tx.pendingCount}`, note: `P2* ${tx.timing.p2star} ms 타이머 재시작` });
        arm(tx, tx.timing.p2star);
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
        profile: Object.assign({}, tx.profile), ptag: tx.route.ptag,
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
        profileId: tx.profile.id, profileName: tx.profile.name, profileSource: tx.profile.source, profileBase: tx.profile.base, profileRev: tx.profile.rev,
        p2Ms: tx.timing.p2, p2starMs: tx.timing.p2star, connectionGen: tx.gen,
        config: Object.assign({}, tx.route.values),
        events: tx.events.map(ev => ({ dtMs: ev.t - tx.t0, kind: ev.kind, bytes: ev.bytes ? hexs(ev.bytes) : '', ignored: !!ev.ignored, note: ev.note || '' })),
      }));
    }

    return {
      connect, disconnect, read, cancel, clearHistory, exportRows,
      profile, profilesFor, activeProfile, routeFor, checkName: nameError, customCount: () => customs.size, saveProfile, deleteProfile, exportConfig, loadConfig, importConfig,
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
    validateProfile, routeFromProfile, presetProfile, presetValues, hostError, hexId, kbit,
    CHANNELS, BITRATES, LIMITS, FIELDS, CONFIG_FORMAT, MAX_PROFILES,
  };
})(typeof window !== 'undefined' ? window : globalThis);
