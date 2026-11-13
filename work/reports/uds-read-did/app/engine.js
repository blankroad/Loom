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
  // Service names: labels only (no behaviour). The mock ECU implements 0x22 alone.
  const SIDS = {
    0x10: 'DiagnosticSessionControl', 0x11: 'ECUReset', 0x14: 'ClearDiagnosticInformation', 0x19: 'ReadDTCInformation',
    0x22: 'ReadDataByIdentifier', 0x23: 'ReadMemoryByAddress', 0x24: 'ReadScalingDataByIdentifier', 0x27: 'SecurityAccess',
    0x28: 'CommunicationControl', 0x29: 'Authentication', 0x2A: 'ReadDataByPeriodicIdentifier', 0x2C: 'DynamicallyDefineDataIdentifier',
    0x2E: 'WriteDataByIdentifier', 0x2F: 'InputOutputControlByIdentifier', 0x31: 'RoutineControl', 0x34: 'RequestDownload',
    0x35: 'RequestUpload', 0x36: 'TransferData', 0x37: 'RequestTransferExit', 0x38: 'RequestFileTransfer', 0x3D: 'WriteMemoryByAddress',
    0x3E: 'TesterPresent', 0x83: 'AccessTimingParameter', 0x84: 'SecuredDataTransmission', 0x85: 'ControlDTCSetting',
    0x86: 'ResponseOnEvent', 0x87: 'LinkControl',
  };
  const SUBFN_SIDS = [0x10, 0x11, 0x19, 0x27, 0x28, 0x29, 0x2C, 0x31, 0x3E, 0x83, 0x85, 0x86, 0x87]; // byte 1 = sub-function (bit 7 = SPRMIB)
  const sidName = s => SIDS[s] || null;
  // Raw PDU limits: prototype UI/engine cap, NOT a transport limit (no CAN-TP/DoIP layer is modelled).
  const RAW_MAX_BYTES = 4095;
  const RAW_TEXT_MAX = 24576;
  const TIMING = { p2: 1000, p2star: 5000 }; // SIM-scaled client timeouts (ms)
  const SCENARIOS = ['positive', 'nrc', 'pending', 'timeout', 'disconnect'];
  const LATENCIES = [80, 400, 900]; // all < P2
  const PENDING_INTERVAL = 1200;    // < P2*
  const LATE_AFTER = 1200;          // late reply arrives (configured) P2 + this
  const CONFIG_FORMAT = 'loom-uds-read-did-config/1';
  const MAX_PROFILES = 60;

  const ecu = id => ECUS.find(e => e.id === id);
  const didInfo = d => DIDS.find(x => x.did === d) || null;

  /* ---------- catalogue (items 4/5): presets immutable; custom ECUs/DIDs + SIM fixture payloads ----------
     ECUS / DIDS stay the SAME arrays (U.ECUS / U.DIDS); custom entries are spliced in/out by the engine catalog API.
     Custom DID decoders are user-defined SIM definitions, NOT ODX. Fixture payloads are SIM ECU data, not vehicle data. */
  const PRESET_ECUS = ECUS.map(e => e.id), PRESET_DIDS = DIDS.map(x => x.did);
  ECUS.forEach(e => Object.freeze(e)); DIDS.forEach(x => Object.freeze(x));
  const CAT_FORMAT = 'loom-uds-catalog/1';
  const CAT_LIMITS = { ecus: 32, dids: 64, fixtureBytes: 255, ecuName: 40, didName: 40, long: 80, unit: 12, intBytes: 6 };
  const DID_TYPES = ['ascii', 'bytes', 'uint', 'sint'];
  const ECU_ID_RE = /^[A-Z][A-Z0-9_]{1,7}$/;
  function defaultEcuData(id) { return { 0xF190: 'LOOMSIM0000000001', 0xF18C: `SIM-${id}-000001`.slice(0, 20), 0x0101: 1200 }; }
  function num(v) { if (typeof v === 'number') return v; const t = String(v == null ? '' : v).trim(); return t === '' || !/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(t) ? NaN : Number(t); }
  // Validate one custom ECU against a registry view {ids:Set, ecus:[...]} (pure).
  function checkEcu(x, others) {
    const errors = {};
    if (!x || typeof x !== 'object') return { ok: false, errors: { _: '객체 아님' } };
    const id = String(x.id == null ? '' : x.id).trim().toUpperCase();
    if (!ECU_ID_RE.test(id)) errors.id = 'ID: 영문 대문자로 시작, A–Z·0–9·_ 2–8자';
    else if (PRESET_ECUS.includes(id)) errors.id = '프리셋 ECU ID — 사용할 수 없음';
    else if (others.some(e => e.id === id)) errors.id = '이미 있는 ECU ID';
    const name = String(x.name == null ? '' : x.name).trim();
    const ne = textError(name, CAT_LIMITS.ecuName, true); if (ne) errors.name = ne;
    const out = { id, name, custom: true };
    const hx = (k, digits, max, label) => { const r = parseHexStrict(x[k], digits); if (!r.ok) errors[k] = r.error; else if (r.n < 1 || r.n > max) errors[k] = `${label} 0x1–0x${max.toString(16).toUpperCase()}`; else out[k] = r.n; };
    hx('doipLA', 4, 0xFFFF, 'DoIP 논리 주소'); hx('canTx', 3, 0x7FF, '11-bit CAN ID'); hx('canRx', 3, 0x7FF, '11-bit CAN ID');
    if (!errors.doipLA && out.doipLA === TESTER_LA) errors.doipLA = 'Tester 논리 주소(0x0E00)와 같을 수 없음';
    if (!errors.canTx && !errors.canRx && out.canTx === out.canRx) errors.canRx = '요청 ID와 같을 수 없음';
    const rest = others.filter(e => e.id !== id);
    if (!errors.doipLA && rest.some(e => e.doipLA === out.doipLA)) errors.doipLA = `ECU ${rest.find(e => e.doipLA === out.doipLA).id}와 DoIP 주소 중복`;
    ['canTx', 'canRx'].forEach(k => { if (!errors[k]) { const o = rest.find(e => e.canTx === out[k] || e.canRx === out[k]); if (o) errors[k] = `ECU ${o.id}와 CAN ID 중복`; } });
    const ok = !Object.keys(errors).length;
    return { ok, errors, value: ok ? out : null };
  }
  // Validate one custom DID definition (pure). others = other DIDs (presets + customs except itself).
  function checkDidDef(x, others) {
    const errors = {};
    if (!x || typeof x !== 'object') return { ok: false, errors: { _: '객체 아님' } };
    const dv = validateDid(typeof x.did === 'number' ? hex4(x.did) : x.did);
    let did = null;
    if (!dv.ok) errors.did = dv.error;
    else if (PRESET_DIDS.includes(dv.did)) errors.did = '프리셋 DID — 사용자 정의로 덮어쓸 수 없음';
    else if (others.some(o => o.did === dv.did)) errors.did = '이미 있는 DID';
    else did = dv.did;
    const name = String(x.name == null ? '' : x.name).trim();
    const ne = textError(name, CAT_LIMITS.didName, true); if (ne) errors.name = ne;
    const long = String(x.long == null ? '' : x.long).trim();
    const le = textError(long, CAT_LIMITS.long, false); if (le) errors.long = le;
    const type = String(x.type == null ? '' : x.type);
    if (!DID_TYPES.includes(type)) errors.type = 'ascii / bytes / uint / sint';
    const I = (k, lo, hi) => { const r = parseIntStrict(x[k]); if (!r.ok) { errors[k] = r.error; return null; } if (r.n < lo || r.n > hi) { errors[k] = `${lo}–${hi}`; return null; } return r.n; };
    const recLen = I('recLen', 1, CAT_LIMITS.fixtureBytes);
    const byteOffset = I('byteOffset', 0, CAT_LIMITS.fixtureBytes - 1);
    const isInt = type === 'uint' || type === 'sint';
    const length = I('length', 1, isInt ? CAT_LIMITS.intBytes : CAT_LIMITS.fixtureBytes);
    if (recLen != null && byteOffset != null && length != null && byteOffset + length > recLen) errors.length = `오프셋 ${byteOffset} + 길이 ${length} > 레코드 ${recLen} B`;
    const endian = x.endian === 'little' ? 'little' : x.endian === 'big' || x.endian == null || !isInt ? 'big' : null;
    if (endian == null) errors.endian = 'big / little';
    let scale = 1, offset = 0, decimals = 0, unit = '';
    if (isInt) {
      scale = num(x.scale); offset = num(x.offset == null ? 0 : x.offset);
      if (!Number.isFinite(scale) || scale === 0 || Math.abs(scale) > 1e9) errors.scale = '0이 아닌 유한한 수 (|x| ≤ 1e9)';
      if (!Number.isFinite(offset) || Math.abs(offset) > 1e12) errors.offset = '유한한 수 (|x| ≤ 1e12)';
      const dr = parseIntStrict(x.decimals == null ? 0 : x.decimals); if (!dr.ok || dr.n > 6) errors.decimals = '0–6'; else decimals = dr.n;
      unit = String(x.unit == null ? '' : x.unit).trim();
      const ue = textError(unit, CAT_LIMITS.unit, false); if (ue) errors.unit = ue;
    }
    const ok = !Object.keys(errors).length;
    return { ok, errors, value: ok ? { did, name, long, type, recLen, byteOffset, length, endian, scale, offset, decimals, unit, custom: true, synthetic: true, enc: 'custom' } : null };
  }
  function parseFixture(v) {
    if (Array.isArray(v)) return v.length <= CAT_LIMITS.fixtureBytes && v.every(n => Number.isInteger(n) && n >= 0 && n <= 255) ? { ok: true, bytes: v.slice() } : { ok: false, error: '0–255 바이트 배열, 최대 255 B' };
    if (typeof v !== 'string') return { ok: false, error: 'hex 문자열' };
    if (!v.trim()) return { ok: true, bytes: [] };
    const p = parseRawHex(v);
    if (!p.ok) return { ok: false, error: p.error };
    if (p.len > CAT_LIMITS.fixtureBytes) return { ok: false, error: `최대 ${CAT_LIMITS.fixtureBytes} B` };
    return { ok: true, bytes: p.bytes };
  }
  // Pure whole-catalogue validation (project preflight / storage). Never mutates ECUS/DIDS.
  // strict: any invalid entry fails; otherwise invalid entries are dropped + counted.
  function validateCatalog(obj, strict) {
    if (obj == null) return { ok: true, value: { ecus: [], dids: [], fixtures: {} }, dropped: 0 };
    if (typeof obj !== 'object' || obj.format !== CAT_FORMAT) return { ok: false, error: `카탈로그 형식이 ${CAT_FORMAT} 이 아님` };
    const inE = Array.isArray(obj.ecus) ? obj.ecus : [], inD = Array.isArray(obj.dids) ? obj.dids : [];
    if (strict && (inE.length > CAT_LIMITS.ecus || inD.length > CAT_LIMITS.dids)) return { ok: false, error: `사용자 ECU ≤ ${CAT_LIMITS.ecus}, DID ≤ ${CAT_LIMITS.dids}` };
    let dropped = Math.max(0, inE.length - CAT_LIMITS.ecus) + Math.max(0, inD.length - CAT_LIMITS.dids);
    const ecus = [], dids = [], fixtures = {};
    const presetE = ECUS.filter(e => !e.custom);
    for (let i = 0; i < Math.min(inE.length, CAT_LIMITS.ecus); i++) {
      const r = checkEcu(inE[i], presetE.concat(ecus));
      if (!r.ok) { if (strict) { const k = Object.keys(r.errors)[0]; return { ok: false, error: `ECU #${i + 1} ${k}: ${r.errors[k]}` }; } dropped++; continue; }
      ecus.push(r.value);
    }
    const presetD = DIDS.filter(x => !x.custom);
    for (let i = 0; i < Math.min(inD.length, CAT_LIMITS.dids); i++) {
      const r = checkDidDef(inD[i], presetD.concat(dids));
      if (!r.ok) { if (strict) { const k = Object.keys(r.errors)[0]; return { ok: false, error: `DID #${i + 1} ${k}: ${r.errors[k]}` }; } dropped++; continue; }
      dids.push(r.value);
    }
    const fx = obj.fixtures && typeof obj.fixtures === 'object' && !Array.isArray(obj.fixtures) ? obj.fixtures : {};
    const ecuIds = PRESET_ECUS.concat(ecus.map(e => e.id));
    for (const eid of Object.keys(fx)) {
      if (!ecuIds.includes(eid) || !fx[eid] || typeof fx[eid] !== 'object') { if (strict) return { ok: false, error: `fixture: 알 수 없는 ECU ${String(eid).slice(0, 12)}` }; dropped++; continue; }
      for (const dh of Object.keys(fx[eid])) {
        const dv = validateDid(dh), def = dv.ok && dids.find(x => x.did === dv.did);
        const pf = def ? parseFixture(fx[eid][dh]) : { ok: false, error: '사용자 DID 아님' };
        if (!pf.ok) { if (strict) return { ok: false, error: `fixture ${eid}/${String(dh).slice(0, 8)}: ${pf.error}` }; dropped++; continue; }
        (fixtures[eid] = fixtures[eid] || {})[hex4(dv.did)] = pf.bytes;
      }
    }
    return { ok: true, value: { ecus, dids, fixtures }, dropped };
  }
  // Decode a custom DID data record (exact bytes kept by the caller).
  function decodeCustom(info, data) {
    const tag = '사용자 정의 디코더 (SIM, ODX 아님)';
    if (data.length !== info.recLen) return { ok: false, text: hexs(data), unit: '', note: `길이 ${data.length} B (기대 ${info.recLen} B) · ${tag}` };
    const v = data.slice(info.byteOffset, info.byteOffset + info.length);
    const where = `@${info.byteOffset}+${info.length} B`;
    if (info.type === 'ascii') { const pr = v.every(x => x >= 0x20 && x < 0x7f); return { ok: pr, text: String.fromCharCode.apply(null, v), unit: '', note: `ASCII ${where}` + (pr ? '' : ' (비인쇄 문자 포함)') + ` · ${tag}` }; }
    if (info.type === 'bytes') return { ok: true, text: hexs(v), unit: '', note: `bytes ${where} · ${tag}` };
    const be = info.endian !== 'little' ? v : v.slice().reverse();
    let raw = 0; be.forEach(x => { raw = raw * 256 + x; });
    if (info.type === 'sint' && v.length && be[0] & 0x80) raw -= Math.pow(256, v.length);
    const phys = raw * info.scale + info.offset;
    return { ok: true, text: phys.toFixed(info.decimals), unit: info.unit, raw, note: `${info.type} ${where} ${info.endian === 'little' ? 'LE' : 'BE'} raw ${raw} × ${info.scale} + ${info.offset} · ${tag}` };
  }

  /* ---------- SIM ECU services (items 9/11) ----------
     DiagnosticSessionControl 0x10 (01/02/03), TesterPresent 0x3E (00, SPRMIB 80), ReadDTCInformation 0x19 (01/02),
     ClearDiagnosticInformation 0x14 (FFFFFF). SIM fixture behaviour only — no SecurityAccess / flashing modelled. */
  const SESSIONS = { 1: 'default', 2: 'programming', 3: 'extendedDiagnostic' };
  const S3_MS = 5000;                       // S3server (SIM, engine clock)
  const P2_SRV = { p2: 50, p2star: 5000 };  // ECU-reported in 0x50 response: METADATA ONLY, never applied to client profile
  const DTC_AVAIL = 0xFF;
  const DTC_BITS = ['testFailed', 'testFailedThisOperationCycle', 'pendingDTC', 'confirmedDTC', 'testNotCompletedSinceLastClear', 'testFailedSinceLastClear', 'testNotCompletedThisOperationCycle', 'warningIndicatorRequested'];
  const DTC_FIX = {
    BCM: [{ code: 0x9A2311, status: 0x2F }, { code: 0x914200, status: 0x28 }, { code: 0x925516, status: 0x24 }],
    GW: [{ code: 0xC10000, status: 0x09 }, { code: 0xC15500, status: 0x08 }],
  };
  const DTC_DESC = {
    0x9A2311: 'Driver door ajar switch — circuit short to ground (SIM fixture)',
    0x914200: 'Interior lamp supply — open circuit (SIM fixture, historic)',
    0x925516: 'Rear wiper motor — circuit voltage below threshold (SIM fixture)',
    0xC10000: 'Lost communication with ECM/PCM (SIM fixture)',
    0xC15500: 'Lost communication with instrument cluster (SIM fixture, historic)',
  };
  function dtcText(code) {
    const b0 = (code >> 16) & 0xff;
    return 'PCBU'[b0 >> 6] + ((b0 >> 4) & 3) + (b0 & 0xf).toString(16).toUpperCase() + hex2((code >> 8) & 0xff) + '-' + hex2(code & 0xff);
  }
  const statusBits = st => DTC_BITS.filter((_, i) => st & (1 << i));

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

  /* Raw UDS PDU parser — one grammar, all-or-nothing (no partial parse):
     tokens are separated by whitespace (space/tab/newline). Each token is EITHER
       0xHH  — "0x"/"0X" + exactly 2 hex digits = one byte, OR
       a compact even-length hex run (e.g. 22F190) = len/2 bytes.
     Everything else rejects the whole input: empty, odd run, non-hex (incl. commas), bare/long 0x, > RAW_MAX_BYTES. */
  function parseRawHex(text) {
    const s = String(text == null ? '' : text);
    if (s.length > RAW_TEXT_MAX) return { ok: false, error: `입력 ${s.length}자 — 최대 ${RAW_TEXT_MAX}자 (잘라내지 않음)` };
    const toks = s.split(/\s+/).filter(Boolean);
    if (!toks.length) return { ok: false, error: 'hex 바이트를 입력하세요 (SID 포함, 예: 22 F1 90)' };
    const out = [];
    const at = (i, t) => `${i + 1}번째 토큰 "${t.length > 12 ? t.slice(0, 12) + '…' : t}"`;
    for (let i = 0; i < toks.length; i++) {
      const t = toks[i];
      if (/^0x/i.test(t)) {
        const h = t.slice(2);
        if (!/^[0-9a-fA-F]*$/.test(h)) return { ok: false, error: `${at(i, t)}: 16진수 문자만 (0-9, A-F)` };
        if (h.length !== 2) return { ok: false, error: `${at(i, t)}: 0x 표기는 바이트당 정확히 2자리 (예: 0x22 0xF1)` };
        out.push(parseInt(h, 16));
      } else {
        if (!/^[0-9a-fA-F]+$/.test(t)) return { ok: false, error: `${at(i, t)}: 16진수 문자만 (0-9, A-F) — 구분자는 공백·탭·줄바꿈` };
        if (t.length % 2) return { ok: false, error: `${at(i, t)}: 홀수 자릿수(${t.length}) — 바이트 경계가 모호함` };
        for (let j = 0; j < t.length; j += 2) out.push(parseInt(t.substr(j, 2), 16));
      }
      if (out.length > RAW_MAX_BYTES) return { ok: false, error: `${RAW_MAX_BYTES} B 초과 — 프로토타입 상한 (전송 계층 한계 아님, 잘라내지 않음)` };
    }
    return { ok: true, bytes: out, sid: out[0], norm: hexs(out), len: out.length };
  }
  // DID of an exact-shape raw ReadDataByIdentifier (22 hi lo, single DID). Anything else -> null.
  const rawDidOf = b => (b.length === 3 && b[0] === 0x22 ? (b[1] << 8) | b[2] : null);
  const shortHex = (b, n) => (b.length > n ? hexs(b.slice(0, n)) + ` … (+${b.length - n} B)` : hexs(b));

  // Raw request: decode only what is known (SID name, exact 22 shape, sub-function bit); rest shown as raw bytes.
  function decodeRawRequest(b) {
    const f = [], sid = b[0], nm = sidName(sid);
    f.push({ name: 'SID', off: 0, len: 1, value: '0x' + hex2(sid), note: (nm ? nm + ' 요청' : '이 목업 카탈로그에 없는 SID') + ' · Raw 입력 그대로' });
    if (sid === 0x22) {
      if (b.length === 3) {
        const did = (b[1] << 8) | b[2];
        f.push({ name: 'DID', off: 1, len: 2, value: '0x' + hex4(did), note: (didInfo(did) || {}).name || '카탈로그에 없음' });
      } else {
        f.push({ name: '형식', off: 1, len: b.length - 1, value: b.length > 1 ? shortHex(b.slice(1), 48) : '—', note: `0x22 요청 ${b.length} B — 이 목업은 SID + DID 2 B (3 B) 단일 DID만 해석 (mock ECU 0x13 응답)`, bad: true });
      }
      return { kind: 'request', raw: true, fields: f };
    }
    let o = 1;
    if (SUBFN_SIDS.includes(sid) && b.length >= 2) {
      const sp = b[1] >> 7;
      f.push({ name: 'subFunction', off: 1, len: 1, value: '0x' + hex2(b[1] & 0x7f), note: `SPRMIB=${sp}` + (sp ? ' (긍정 응답 억제 요청 — 이 목업의 확정 값에 영향 없음)' : '') });
      o = 2;
    }
    if (b.length > o) f.push({ name: 'parameters', off: o, len: b.length - o, value: shortHex(b.slice(o), 48), note: `원시 ${b.length - o} B — 해석 안 함` });
    return { kind: 'request', raw: true, fields: f };
  }
  // Decode one UDS PDU. ctx = DID number (legacy Read DID call) or { did, sid, kind:'did'|'raw', dir:'tx'|'rx' }.
  function decodePdu(bytes, ctx) {
    const c = ctx != null && typeof ctx === 'object' ? ctx : { did: ctx, sid: 0x22, kind: 'did' };
    const did = c.did, rsid = c.sid == null ? 0x22 : c.sid;
    const b = Array.from(bytes || []);
    const f = [];
    if (!b.length) return { kind: 'empty', fields: f };
    const sid = b[0];
    if (c.kind === 'raw' && c.dir === 'tx') return decodeRawRequest(b);
    if (sid === 0x22 && c.dir !== 'rx') {
      f.push({ name: 'SID', off: 0, len: 1, value: '0x22', note: 'ReadDataByIdentifier 요청' });
      if (b.length >= 3) f.push({ name: 'DID', off: 1, len: 2, value: '0x' + hex4((b[1] << 8) | b[2]), note: (didInfo((b[1] << 8) | b[2]) || {}).name || '카탈로그에 없음' });
      return { kind: 'request', fields: f };
    }
    if (sid === 0x7F) {
      f.push({ name: 'SID-NR', off: 0, len: 1, value: '0x7F', note: '부정 응답' });
      if (b.length > 1) f.push({ name: 'SIDRQ-NR', off: 1, len: 1, value: '0x' + hex2(b[1]), note: b[1] === rsid ? (sidName(b[1]) || '요청 SID 0x' + hex2(b[1]) + ' 일치') : '요청 SID 불일치', bad: b[1] !== rsid });
      if (b.length > 2) f.push({ name: 'NRC', off: 2, len: 1, value: '0x' + hex2(b[2]), note: NRC[b[2]] || 'unknown' });
      return { kind: b[2] === 0x78 ? 'pending' : 'nrc', nrc: b[2], fields: f };
    }
    if (sid === 0x62 && rsid === 0x22) {
      f.push({ name: 'SID-PR', off: 0, len: 1, value: '0x62', note: '긍정 응답 (0x22 + 0x40)' });
      const echo = b.length >= 3 ? ((b[1] << 8) | b[2]) : -1;
      const echoOk = echo === did;
      f.push({ name: 'DID', off: 1, len: 2, value: echo >= 0 ? '0x' + hex4(echo) : '—', note: echoOk ? '요청 DID와 일치' : '요청 DID와 불일치', bad: !echoOk });
      const data = b.slice(3);
      const info = c.def !== undefined && echo === did ? c.def : didInfo(echo); // txn-time DID definition snapshot
      const dv = decodeValue(info, data);
      f.push({ name: info ? info.name : 'dataRecord', off: 3, len: data.length, value: dv.text, unit: dv.unit, note: dv.note, bad: !dv.ok, decoded: true });
      return { kind: 'positive', echoOk, value: dv, fields: f };
    }
    if (sid === ((rsid + 0x40) & 0xff) && rsid !== 0x22 && SVC_DEC[sid]) {
      f.push({ name: 'SID-PR', off: 0, len: 1, value: '0x' + hex2(sid), note: `긍정 응답 (0x${hex2(rsid)} + 0x40) ${sidName(rsid)}` });
      SVC_DEC[sid](b, f);
      return { kind: 'positive', generic: true, service: rsid, echoOk: false, value: { ok: false, text: '', unit: '' }, fields: f };
    }
    if (sid === ((rsid + 0x40) & 0xff) && rsid !== 0x22) {
      f.push({ name: 'SID-PR', off: 0, len: 1, value: '0x' + hex2(sid), note: `긍정 응답 (0x${hex2(rsid)} + 0x40)` + (sidName(rsid) ? ' ' + sidName(rsid) : '') });
      if (b.length > 1) f.push({ name: 'responseData', off: 1, len: b.length - 1, value: shortHex(b.slice(1), 48), note: `원시 ${b.length - 1} B — 해석 안 함` });
      return { kind: 'positive', generic: true, echoOk: false, value: { ok: false, text: '', unit: '' }, fields: f };
    }
    f.push({ name: 'SID', off: 0, len: 1, value: '0x' + hex2(sid), note: '예상하지 않은 SID', bad: true });
    return { kind: 'unexpected', fields: f };
  }
  const SVC_DEC = {
    0x50(b, f) {
      if (b.length > 1) f.push({ name: 'diagnosticSessionType', off: 1, len: 1, value: '0x' + hex2(b[1]), note: SESSIONS[b[1]] || '알 수 없는 세션' });
      if (b.length >= 4) f.push({ name: 'P2server_max', off: 2, len: 2, value: ((b[2] << 8) | b[3]) + ' ms', note: 'ECU 보고 메타데이터 — 클라이언트 P2 프로필은 바꾸지 않음 (SIM)' });
      if (b.length >= 6) f.push({ name: 'P2*server_max', off: 4, len: 2, value: ((b[4] << 8) | b[5]) * 10 + ' ms', note: `0x${hex4((b[4] << 8) | b[5])} × 10 ms · ECU 보고 메타데이터 (클라이언트 P2* 유지)` });
      if (b.length !== 6) f.push({ name: '형식', off: 1, len: Math.max(0, b.length - 1), value: `${b.length} B`, note: '기대 6 B', bad: true });
    },
    0x7E(b, f) { if (b.length > 1) f.push({ name: 'zeroSubFunction', off: 1, len: 1, value: '0x' + hex2(b[1]), note: 'TesterPresent 긍정 응답 — S3 타이머 재시작 (SIM)' }); },
    0x54(b, f) { f.push({ name: 'clear', off: 0, len: 1, value: '완료', note: 'SIM ECU DTC 메모리 지움 (로컬 SIM 동작, 실제 차량 아님)' }); },
    0x59(b, f) {
      if (b.length < 2) return;
      const sub = b[1];
      f.push({ name: 'reportType', off: 1, len: 1, value: '0x' + hex2(sub), note: sub === 1 ? 'reportNumberOfDTCByStatusMask' : sub === 2 ? 'reportDTCByStatusMask' : '해석 안 함' });
      if (b.length > 2) f.push({ name: 'DTCStatusAvailabilityMask', off: 2, len: 1, value: '0x' + hex2(b[2]), note: '' });
      if (sub === 1 && b.length >= 6) {
        f.push({ name: 'DTCFormatIdentifier', off: 3, len: 1, value: '0x' + hex2(b[3]), note: b[3] === 1 ? 'ISO 14229-1 DTC format' : '' });
        f.push({ name: 'DTCCount', off: 4, len: 2, value: String((b[4] << 8) | b[5]), note: '' });
      } else if (sub === 2) {
        const recs = parseDtcRecords(b);
        recs.list.forEach((r, i) => {
          f.push({ name: `DTC[${i}]`, off: 3 + i * 4, len: 3, value: dtcText(r.code), note: DTC_DESC[r.code] || '설명 없음 (fixture 외)' });
          f.push({ name: `status[${i}]`, off: 6 + i * 4, len: 1, value: '0x' + hex2(r.status), note: statusBits(r.status).join(', ') || '없음' });
        });
        if (recs.rest) f.push({ name: '형식', off: 3 + recs.list.length * 4, len: recs.rest, value: `${recs.rest} B`, note: '4 B 레코드 경계 불일치', bad: true });
        if (!recs.list.length && !recs.rest) f.push({ name: 'records', off: 3, len: 0, value: '0건', note: '마스크와 일치하는 DTC 없음' });
      }
    },
  };
  function parseDtcRecords(b) {
    const list = [], body = b.slice(3);
    for (let i = 0; i + 4 <= body.length; i += 4) list.push({ code: (body[i] << 16) | (body[i + 1] << 8) | body[i + 2], status: body[i + 3] });
    return { list, rest: body.length % 4, avail: b.length > 2 ? b[2] : null };
  }
  function decodeValue(info, data) {
    if (info && info.custom) return decodeCustom(info, data);
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
    const counters = {};
    let seq = 0;
    const ecuData = {};           // SIM ECU fixture data: ecu -> { did: value | bytes[] }
    const dtcMem = {};            // SIM ECU DTC memory: ecu -> [{code,status}]
    const dataOf = id => (ecuData[id] = ecuData[id] || (ECU_DATA[id] ? Object.assign({}, ECU_DATA[id]) : defaultEcuData(id)));
    const dtcOf = id => (dtcMem[id] = dtcMem[id] || (DTC_FIX[id] || []).map(x => Object.assign({}, x)));
    /* connection profile store: presets are built-in & immutable; customs scoped to one ECU×route */
    const customs = new Map();    // id -> custom profile
    const active = new Map();     // ctx key -> custom id (absent = preset)
    let nextPid = 1;

    const ckey = (e, t) => e + '|' + t;
    function ctx(e, t) {
      const k = ckey(e, t);
      if (!ctxs.has(k)) ctxs.set(k, { key: k, ecu: e, transport: t, link: 'offline', gen: 0, active: null, srv: { session: 1, s3: null, s3Due: null }, sv: { session: 1, status: 'offline' } });
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
    function checkEntry(x, i, hasEcu) {
      if (!x || typeof x !== 'object') return { error: `#${i + 1}: 객체 아님` };
      if (!(hasEcu ? hasEcu(x.ecu) : ecu(x.ecu)) || !FIELDS[x.transport]) return { error: `#${i + 1}: 알 수 없는 ECU/경로` };
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

    // Pure strict validation of a profile store against a CANDIDATE ECU id set (project preflight; no mutation).
    function validateConfigFor(obj, ecuIds) {
      if (obj == null) return { ok: true, value: { format: CONFIG_FORMAT, profiles: [], active: {} } };
      if (typeof obj !== 'object' || obj.format !== CONFIG_FORMAT) return { ok: false, error: `프로필 형식이 ${CONFIG_FORMAT} 이 아님` };
      const list = Array.isArray(obj.profiles) ? obj.profiles : [];
      if (list.length > MAX_PROFILES) return { ok: false, error: `프로필 최대 ${MAX_PROFILES}개 초과` };
      const has = id => ecuIds.includes(id), ids = new Set(), names = new Set(), out = [];
      for (let i = 0; i < list.length; i++) {
        const x = list[i], r = checkEntry(x, i, has);
        if (r.error) return { ok: false, error: '프로필 ' + r.error };
        const id = x && typeof x.id === 'string' && /^u\d{1,6}$/.test(x.id) ? x.id : null;
        if (!id || ids.has(id)) return { ok: false, error: `프로필 #${i + 1}: id 형식(u<n>)/중복` };
        const nk = r.p.ecu + '|' + r.p.transport + '|' + r.p.name;
        if (names.has(nk) || r.p.name === `프리셋 ${r.p.ecu} ${trLabel(r.p.transport)}`) return { ok: false, error: `프로필 #${i + 1}: 같은 ECU·경로에 이름 중복` };
        ids.add(id); names.add(nk); out.push(Object.assign({ id }, exportProfile(Object.assign({ id }, r.p))));
      }
      const act = {};
      if (obj.active != null) {
        if (typeof obj.active !== 'object') return { ok: false, error: 'active 형식 오류' };
        for (const k of Object.keys(obj.active)) {
          const p = out.find(q => q.id === obj.active[k]);
          if (!p || ckey(p.ecu, p.transport) !== k) return { ok: false, error: `active ${String(k).slice(0, 24)}: 프로필 불일치` };
          act[k] = p.id;
        }
      }
      return { ok: true, value: { format: CONFIG_FORMAT, profiles: out, active: act } };
    }

    /* ---------- catalogue store (custom ECUs / DIDs / SIM fixtures) ---------- */
    const ecuCtxs = id => TRANSPORTS.map(t => ctx(id, t.id));
    function ecuGuard(id) {
      const busy = ecuCtxs(id).filter(c => c.link === 'online' || c.active);
      return busy.length ? `${id}: ${busy.map(c => trLabel(c.transport) + (c.active ? ' 요청 진행 중' : ' 연결됨')).join(', ')} — 먼저 연결 해제/완료 (자동 해제 없음)` : null;
    }
    const plainEcu = e => ({ id: e.id, name: e.name, doipLA: e.doipLA, canTx: e.canTx, canRx: e.canRx });
    const plainDid = x => ({ did: hex4(x.did), name: x.name, long: x.long, type: x.type, recLen: x.recLen, byteOffset: x.byteOffset, length: x.length, endian: x.endian, scale: x.scale, offset: x.offset, decimals: x.decimals, unit: x.unit });
    function exportStore() {
      const fixtures = {};
      Object.keys(ecuData).forEach(eid => {
        if (!ecu(eid)) return;
        DIDS.filter(x => x.custom && Array.isArray(ecuData[eid][x.did])).forEach(x => { (fixtures[eid] = fixtures[eid] || {})[hex4(x.did)] = hexs(ecuData[eid][x.did]); });
      });
      return { format: CAT_FORMAT, ecus: ECUS.filter(e => e.custom).map(plainEcu), dids: DIDS.filter(x => x.custom).map(plainDid), fixtures };
    }
    // Apply an already-validated catalogue value (replace all custom entries). Caller guarantees guards.
    function applyCatalog(v) {
      const oldE = new Map(ECUS.filter(e => e.custom).map(e => [e.id, e]));
      for (let i = ECUS.length - 1; i >= 0; i--) if (ECUS[i].custom) ECUS.splice(i, 1);
      for (let i = DIDS.length - 1; i >= 0; i--) if (DIDS[i].custom) DIDS.splice(i, 1);
      v.ecus.forEach(e => ECUS.push(Object.freeze(Object.assign({}, e, { custom: true }))));
      v.dids.forEach(x => DIDS.push(Object.freeze(Object.assign({}, x))));
      oldE.forEach((e, id) => {
        const n = ecu(id);
        if (!n || n.doipLA !== e.doipLA || n.canTx !== e.canTx || n.canRx !== e.canRx) TRANSPORTS.forEach(t => PRESETS.delete(presetId(id, t.id)));
        if (!n) { [...customs.values()].filter(p => p.ecu === id).forEach(p => customs.delete(p.id)); TRANSPORTS.forEach(t => active.delete(ckey(id, t.id))); delete ecuData[id]; delete dtcMem[id]; }
      });
      Object.keys(ecuData).forEach(eid => Object.keys(ecuData[eid]).forEach(k => { if (!PRESET_DIDS.includes(+k)) delete ecuData[eid][k]; }));
      Object.keys(v.fixtures || {}).forEach(eid => Object.keys(v.fixtures[eid]).forEach(dh => { dataOf(eid)[parseInt(dh, 16)] = v.fixtures[eid][dh].slice(); }));
      emit('catalog', { action: 'replace' });
    }
    const curValue = () => validateCatalog(exportStore(), true).value;
    function mutate(fn) {
      const v = curValue(), r = fn(v);
      if (!r || !r.ok) return r;
      const chk = validateCatalog(Object.assign(exportStoreOf(v), {}), true); // re-validate the whole candidate
      if (!chk.ok) return { ok: false, code: 'invalid', error: chk.error };
      applyCatalog(chk.value);
      return r;
    }
    const exportStoreOf = v => ({ format: CAT_FORMAT, ecus: v.ecus.map(plainEcu), dids: v.dids.map(plainDid), fixtures: Object.fromEntries(Object.entries(v.fixtures).map(([e, m]) => [e, Object.fromEntries(Object.entries(m).map(([d, b]) => [d, hexs(b)]))])) });
    const catalog = {
      format: CAT_FORMAT, limits: CAT_LIMITS, types: DID_TYPES,
      validate: obj => validateCatalog(obj, true),
      checkEcu: (x, exceptId) => checkEcu(x, ECUS.filter(e => e.id !== exceptId)),
      checkDid: (x, exceptDid) => checkDidDef(x, DIDS.filter(o => o.did !== exceptDid)),
      parseFixture, exportStore, guard: ecuGuard,
      load(obj) { // boot restore from own storage: invalid entries dropped & counted, never applied
        if ([...ctxs.values()].some(c => c.link === 'online')) return { ok: false, error: 'online' };
        const r = validateCatalog(obj, false);
        if (!r.ok) { applyCatalog({ ecus: [], dids: [], fixtures: {} }); return { ok: true, dropped: 1 }; }
        applyCatalog(r.value);
        return { ok: true, dropped: r.dropped };
      },
      replace(v) { // project commit: v from validate(); all ECU contexts must be idle/offline
        const g = ECUS.map(e => ecuGuard(e.id)).filter(Boolean);
        if (g.length) return { ok: false, code: 'online', error: g.join(' · ') };
        applyCatalog(v); return { ok: true };
      },
      addEcu(x) {
        if (ECUS.filter(e => e.custom).length >= CAT_LIMITS.ecus) return { ok: false, code: 'limit', error: `사용자 ECU 최대 ${CAT_LIMITS.ecus}개` };
        const c = checkEcu(x, ECUS); if (!c.ok) return { ok: false, code: 'invalid', errors: c.errors, error: '입력 오류 — 저장하지 않음' };
        return mutate(v => { v.ecus.push(c.value); return { ok: true, ecu: c.value.id }; });
      },
      updateEcu(id, x) {
        const cur = ecu(id);
        if (!cur || !cur.custom) return { ok: false, code: 'preset', error: '프리셋 ECU는 수정할 수 없음' };
        const c = checkEcu(Object.assign({}, x, { id }), ECUS.filter(e => e.id !== id)); if (!c.ok) return { ok: false, code: 'invalid', errors: c.errors, error: '입력 오류 — 저장하지 않음' };
        const addr = c.value.doipLA !== cur.doipLA || c.value.canTx !== cur.canTx || c.value.canRx !== cur.canRx;
        if (addr) { const g = ecuGuard(id); if (g) return { ok: false, code: 'online', error: '주소 변경: ' + g }; }
        return mutate(v => { v.ecus = v.ecus.map(e => (e.id === id ? c.value : e)); return { ok: true, ecu: id, addressChanged: addr }; });
      },
      deleteEcu(id) {
        const cur = ecu(id);
        if (!cur || !cur.custom) return { ok: false, code: 'preset', error: '프리셋 ECU는 삭제할 수 없음' };
        const g = ecuGuard(id); if (g) return { ok: false, code: 'online', error: g };
        const nProf = [...customs.values()].filter(p => p.ecu === id).length;
        return mutate(v => { v.ecus = v.ecus.filter(e => e.id !== id); delete v.fixtures[id]; return { ok: true, ecu: id, profilesRemoved: nProf }; });
      },
      addDid(x, fixtureFor) {
        if (DIDS.filter(d => d.custom).length >= CAT_LIMITS.dids) return { ok: false, code: 'limit', error: `사용자 DID 최대 ${CAT_LIMITS.dids}개` };
        const c = checkDidDef(x, DIDS); if (!c.ok) return { ok: false, code: 'invalid', errors: c.errors, error: '입력 오류 — 저장하지 않음' };
        return mutate(v => {
          v.dids.push(c.value);
          (fixtureFor || []).forEach(([eid, bytes]) => { if (ecu(eid)) (v.fixtures[eid] = v.fixtures[eid] || {})[hex4(c.value.did)] = bytes.slice(); });
          return { ok: true, did: c.value.did };
        });
      },
      updateDid(did, x) {
        const cur = didInfo(did);
        if (!cur || !cur.custom) return { ok: false, code: 'preset', error: '프리셋 DID는 수정할 수 없음' };
        const c = checkDidDef(Object.assign({}, x, { did: hex4(did) }), DIDS.filter(o => o.did !== did)); if (!c.ok) return { ok: false, code: 'invalid', errors: c.errors, error: '입력 오류 — 저장하지 않음' };
        return mutate(v => { v.dids = v.dids.map(o => (o.did === did ? c.value : o)); return { ok: true, did }; });
      },
      deleteDid(did) {
        const cur = didInfo(did);
        if (!cur || !cur.custom) return { ok: false, code: 'preset', error: '프리셋 DID는 삭제할 수 없음' };
        return mutate(v => { v.dids = v.dids.filter(o => o.did !== did); Object.values(v.fixtures).forEach(m => delete m[hex4(did)]); return { ok: true, did }; });
      },
      setFixture(eid, did, val) {
        const info = didInfo(did);
        if (!ecu(eid) || !info || !info.custom) return { ok: false, code: 'target', error: '사용자 DID와 ECU만 SIM fixture 편집 가능' };
        let bytes = null;
        if (val != null) { const p = parseFixture(val); if (!p.ok) return { ok: false, code: 'invalid', error: p.error }; bytes = p.bytes; }
        return mutate(v => { const m = (v.fixtures[eid] = v.fixtures[eid] || {}); if (bytes) m[hex4(did)] = bytes; else delete m[hex4(did)]; return { ok: true }; });
      },
      fixture: (eid, did) => { const b = ecu(eid) && dataOf(eid)[did]; return Array.isArray(b) ? b.slice() : null; },
    };

    function connect(e, t) {
      const c = ctx(e, t);
      if (!ecu(e) || !FIELDS[t]) return { ok: false, error: 'unknown route' };
      if (c.link === 'online') return { ok: true, ctx: c };
      // Freeze the active profile for this connection generation; reads use only this snapshot.
      c.conn = { route: JSON.parse(JSON.stringify(routeFromProfile(e, activeProfile(e, t)))) };
      c.link = 'online'; c.gen += 1; c.since = clock.now();
      clearS3(c); c.srv.session = 1;
      c.sv = { session: 1, status: 'assumed', t: clock.now(), note: '새 연결 — 서버 기본 세션으로 가정 (확인 응답 아님)' };
      emit('link', { ctx: c });
      return { ok: true, ctx: c };
    }
    function disconnect(e, t, cause) {
      const c = ctx(e, t);
      if (c.link !== 'online') return { ok: false, error: 'offline' };
      const tx = c.active ? byId(c.active) : null;
      c.link = 'offline'; c.gen += 1; c.cause = cause || 'user'; c.conn = null;
      clearS3(c); c.srv.session = 1;
      c.sv = { session: 1, status: 'reset', t: clock.now(), note: '연결 해제 — 다음 연결은 기본 세션에서 시작 (SIM)' };
      if (tx) finish(tx, 'link-lost', { kind: 'link', label: cause === 'sim' ? '링크 끊김 (SIM 시나리오)' : '링크 끊김 (사용자 연결 해제)', note: '응답 없음 — 이후 이 연결 세대의 응답은 도착할 수 없음' });
      emit('link', { ctx: c });
      return { ok: true };
    }

    function read(req) {
      const v = validateDid(req.didText);
      if (!v.ok) return { ok: false, error: v.error, code: 'invalid' };
      const info = didInfo(v.did);
      return begin(req, { kind: 'did', sid: 0x22, did: v.did, didHex: v.norm, didName: info ? info.name : null, didDef: info ? JSON.parse(JSON.stringify(info)) : null, request: buildRequest(v.did) });
    }
    // Raw UDS PDU: exact user bytes (incl. SID). No 22 prepended, no DID length forced, no padding/alteration.
    function sendRaw(req) {
      const p = parseRawHex(req.rawText);
      if (!p.ok) return { ok: false, error: p.error, code: 'invalid' };
      const did = rawDidOf(p.bytes), info = did != null ? didInfo(did) : null;
      return begin(req, { kind: 'raw', sid: p.sid, did, didHex: did != null ? hex4(did) : null, didName: info ? info.name : null, didDef: info ? JSON.parse(JSON.stringify(info)) : null, request: p.bytes.slice() });
    }
    function begin(req, q) {
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
      const tx = {
        id: ++seq, no: seq, ecu: e.id, ecuName: e.name, transport: r.transport,
        route: JSON.parse(JSON.stringify(r)), gen: c.gen, ctxKey: c.key,
        timing: { p2: r.timing.p2, p2star: r.timing.p2star }, profile: Object.assign({}, r.profile),
        kind: q.kind, sid: q.sid, sidName: sidName(q.sid),
        did: q.did, didHex: q.didHex, didName: q.didName, didDef: q.didDef,
        request: q.request, sim, t0, tEnd: null,
        source: typeof req.source === 'string' && /^[a-z][a-z0-9:_-]{0,39}$/.test(req.source) ? req.source : 'manual',
        status: 'waiting', pendingCount: 0, final: null, events: [],
        applied: null, appliedNote: '', timers: [], deadline: null,
      };
      tx.events.push({ t: t0, kind: 'tx', bytes: tx.request.slice(), label: tx.kind === 'raw' ? `Tx Raw 요청 (SID 0x${hex2(tx.sid)})` : 'Tx 요청' });
      c.active = tx.id;
      txns.push(tx);
      evict();
      arm(tx, tx.timing.p2);
      plan(tx);
      emit('txn', { txn: tx, created: true });
      return { ok: true, txn: tx };
    }

    // Simulated ECU "normal" final reply, evaluated when the SIM ECU processes the request (null = suppressed).
    function ecuFinal(tx) {
      const c = ctxs.get(tx.ctxKey);
      if (c && c.gen === tx.gen) touchS3(c); // any processed request restarts S3 (non-default session)
      if (tx.sid !== 0x22) return serviceReply(tx, c);
      if (tx.request.length !== 3) return [0x7F, 0x22, 0x13]; // mock: malformed / multi-DID 0x22 not modelled
      const data = dataOf(tx.ecu);
      const info = didInfo(tx.did);
      if (!info || !(tx.did in data)) return [0x7F, 0x22, 0x31];
      let payload;
      if (info.custom) payload = data[tx.did].slice();
      else if (info.enc === 'ascii') payload = Array.from(String(data[tx.did]), ch => ch.charCodeAt(0));
      else { counters[tx.ecu] = counters[tx.ecu] || 0; const raw = data[tx.did] + (counters[tx.ecu]++ % 7); payload = [(raw >> 8) & 0xff, raw & 0xff]; }
      return [0x62, tx.request[1], tx.request[2]].concat(payload);
    }

    function plan(tx) {
      const s = tx.sim, gen = tx.gen, at = (ms, fn) => tx.timers.push(clock.set(() => {
        const c = ctxs.get(tx.ctxKey);
        if (!c || c.gen !== gen) return; // connection generation gone: nothing can arrive
        fn();
      }, ms));
      if (s.scenario === 'positive') at(s.latency, () => deliver(tx, ecuFinal(tx)));
      else if (s.scenario === 'nrc') at(s.latency, () => { const c = ctxs.get(tx.ctxKey); if (c) touchS3(c); deliver(tx, [0x7F, tx.sid, s.nrc]); });
      else if (s.scenario === 'pending') {
        for (let i = 0; i < s.pending; i++) at(s.latency + i * s.pendingInterval, () => deliver(tx, [0x7F, tx.sid, 0x78]));
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
      if (bytes == null) { // SPRMIB: server sends no positive response (request processed)
        if (terminal(tx.status)) return;
        finish(tx, 'suppressed', { kind: 'suppressed', label: '응답 억제 (SPRMIB=1)', note: 'SIM ECU가 요청을 처리했고 긍정 응답을 보내지 않음 — 테스터 측 확인 아님' });
        afterService(tx, null);
        return;
      }
      const d = decodePdu(bytes, { did: tx.did, sid: tx.sid, kind: tx.kind, dir: 'rx', def: tx.didDef });
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
        afterService(tx, bytes);
        emit('txn', { txn: tx });
        return;
      }
      tx.final = bytes.slice();
      tx.nrc = d.nrc;
      finish(tx, 'nrc', { kind: 'nrc', bytes, label: `Rx 부정 응답 0x${hex2(d.nrc)}`, note: NRC[d.nrc] || '' });
    }

    /* ---------- SIM ECU service state ---------- */
    function clearS3(c) { if (c.srv.s3) clock.clear(c.srv.s3); c.srv.s3 = null; c.srv.s3Due = null; }
    // Server received/processed a request on this context: restart S3 while in a non-default session.
    function touchS3(c) {
      clearS3(c);
      if (c.srv.session === 1 || c.link !== 'online') return;
      const gen = c.gen;
      c.srv.s3Due = clock.now() + S3_MS;
      c.srv.s3 = clock.set(() => {
        c.srv.s3 = null; c.srv.s3Due = null;
        if (c.gen !== gen || c.link !== 'online') return;
        const prev = c.srv.session;
        c.srv.session = 1;
        c.sv = { session: 1, status: 'expired', t: clock.now(), prev, note: `S3 ${S3_MS} ms 동안 요청 없음 — SIM 서버가 ${SESSIONS[prev]} → default 복귀` };
        emit('session', { ctxKey: c.key, ecu: c.ecu, session: 1 });
      }, S3_MS);
    }
    // Tester-side session knowledge from a terminal 0x10 transaction (DID/manual/raw alike: same context state).
    function afterService(tx, bytes) {
      const c = ctxs.get(tx.ctxKey);
      if (!c || tx.sid !== 0x10 || tx.request.length !== 2 || c.gen !== tx.gen) return;
      const req = tx.request[1] & 0x7f;
      if (bytes && bytes[0] === 0x50 && bytes[1] === req) c.sv = { session: req, status: 'confirmed', t: clock.now(), txnId: tx.id, no: tx.no, p2srv: bytes.length >= 4 ? (bytes[2] << 8) | bytes[3] : null, p2starsrv: bytes.length >= 6 ? ((bytes[4] << 8) | bytes[5]) * 10 : null };
      else if (tx.status === 'nrc') return;
      else c.sv = { session: c.sv.session, status: 'unknown', requested: req, t: clock.now(), txnId: tx.id, no: tx.no, note: tx.status === 'suppressed' ? `0x${hex2(req)} 요청 (응답 억제) — 확인 응답 없음` : `0x${hex2(req)} 요청 ${statusText(tx)} — 서버 세션 알 수 없음` };
      emit('session', { ctxKey: c.key, ecu: c.ecu, session: c.sv.session });
    }
    function serviceReply(tx, c) {
      const b = tx.request, sid = b[0], sub = b.length > 1 ? b[1] & 0x7f : null, spr = b.length > 1 && !!(b[1] & 0x80);
      const nr = n => [0x7F, sid, n];
      if (sid === 0x10) {
        if (b.length !== 2) return nr(0x13);
        if (!SESSIONS[sub]) return nr(0x12);
        if (sub === 2 && c.srv.session === 1) return nr(0x22); // SIM fixture: programming only from a non-default session
        c.srv.session = sub;
        touchS3(c);
        return spr ? null : [0x50, sub, (P2_SRV.p2 >> 8) & 0xff, P2_SRV.p2 & 0xff, ((P2_SRV.p2star / 10) >> 8) & 0xff, (P2_SRV.p2star / 10) & 0xff];
      }
      if (sid === 0x3E) {
        if (b.length !== 2) return nr(0x13);
        if (sub !== 0) return nr(0x12);
        return spr ? null : [0x7E, 0x00];
      }
      if (sid === 0x19) {
        if (b.length < 2) return nr(0x13);
        if (sub !== 1 && sub !== 2) return nr(0x12);
        if (b.length !== 3) return nr(0x13);
        const mask = b[2], hits = dtcOf(tx.ecu).filter(x => x.status & mask & DTC_AVAIL);
        if (spr) return null;
        if (sub === 1) return [0x59, 0x01, DTC_AVAIL, 0x01, (hits.length >> 8) & 0xff, hits.length & 0xff];
        return [0x59, 0x02, DTC_AVAIL].concat(...hits.map(x => [(x.code >> 16) & 0xff, (x.code >> 8) & 0xff, x.code & 0xff, x.status & DTC_AVAIL]));
      }
      if (sid === 0x14) {
        if (b.length !== 4) return nr(0x13);
        if (((b[1] << 16) | (b[2] << 8) | b[3]) !== 0xFFFFFF) return nr(0x31); // only "all groups" modelled
        if (c.srv.session === 1) return nr(0x22);                               // SIM fixture: clear needs a non-default session
        dtcMem[tx.ecu] = [];
        emit('dtc', { ecu: tx.ecu });
        return [0x54];
      }
      return nr(0x11);
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
      if (status === 'timeout' || status === 'cancelled') afterService(tx, null);
      if (!silent) emit('txn', { txn: tx });
    }

    function applyConfirmed(tx, d) {
      const key = tx.ecu + '|' + tx.did;
      const cur = confirmed.get(key);
      // Only a single-DID 0x22 request (DID mode, or raw of exact shape 22 hi lo) may update confirmed DID values.
      if (tx.sid !== 0x22 || tx.did == null || tx.request.length !== 3 || d.generic) { tx.applied = false; tx.appliedNote = `Raw 요청 SID 0x${hex2(tx.sid)} — DID 확정 값 대상 아님 (반영 안 함)`; return; }
      if (!d.echoOk || !d.value.ok) { tx.applied = false; tx.appliedNote = '응답 검증 실패 — 확정 값에 반영하지 않음'; return; }
      if (cur && cur.no > tx.no) { tx.applied = false; tx.appliedNote = `더 최근 요청 #${cur.no}의 확정 값이 이미 있어 반영하지 않음`; return; }
      confirmed.set(key, {
        key, ecu: tx.ecu, did: tx.did, didHex: tx.didHex, didName: tx.didName,
        value: d.value.text, unit: d.value.unit, data: tx.final.slice(3), txnId: tx.id, no: tx.no,
        t: tx.tEnd, transport: tx.transport, routeLabel: tx.route.label, routeSummary: tx.route.summary,
        profile: Object.assign({}, tx.profile), ptag: tx.route.ptag, kind: tx.kind,
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
      return ({ waiting: '응답 대기', pending: '응답 대기(0x78)', positive: '긍정', suppressed: '응답 억제', nrc: 'NRC', timeout: '타임아웃', cancelled: '취소', 'link-lost': '링크 끊김' })[tx.status] || tx.status;
    }

    function exportRows() {
      return txns.map(tx => ({
        no: tx.no, t0: new Date(tx.t0).toISOString(), ecu: tx.ecu, transport: tx.route.label, route: tx.route.summary,
        kind: tx.kind, sid: '0x' + hex2(tx.sid), sidName: tx.sidName || '', requestLen: tx.request.length,
        did: tx.didHex || '', didName: tx.didName || '', request: hexs(tx.request), finalResponse: tx.final ? hexs(tx.final) : '',
        source: tx.source, status: tx.status, nrc: tx.nrc != null ? '0x' + hex2(tx.nrc) : '', pendingCount: tx.pendingCount,
        elapsedMs: tx.tEnd != null ? tx.tEnd - tx.t0 : null, appliedToConfirmed: !!tx.applied, simScenario: tx.sim.scenario,
        profileId: tx.profile.id, profileName: tx.profile.name, profileSource: tx.profile.source, profileBase: tx.profile.base, profileRev: tx.profile.rev,
        p2Ms: tx.timing.p2, p2starMs: tx.timing.p2star, connectionGen: tx.gen,
        config: Object.assign({}, tx.route.values),
        events: tx.events.map(ev => ({ dtMs: ev.t - tx.t0, kind: ev.kind, bytes: ev.bytes ? hexs(ev.bytes) : '', ignored: !!ev.ignored, note: ev.note || '' })),
      }));
    }

    return {
      connect, disconnect, read, sendRaw, cancel, clearHistory, exportRows,
      profile, profilesFor, activeProfile, routeFor, checkName: nameError, customCount: () => customs.size, saveProfile, deleteProfile, exportConfig, loadConfig, importConfig,
      ctx, txns, confirmed, byId, statusText, terminal, catalog, validateConfigFor,
      session(e, t) { const c = ctx(e, t); return { server: { session: c.srv.session, name: SESSIONS[c.srv.session], s3Due: c.srv.s3Due }, tester: Object.assign({ name: SESSIONS[c.sv.session] }, c.sv), link: c.link, gen: c.gen }; },
      dtcMemory: id => dtcOf(id).map(x => Object.assign({}, x)),
      simRestoreDtc(id) { if (!ecu(id)) return { ok: false }; dtcMem[id] = (DTC_FIX[id] || []).map(x => Object.assign({}, x)); emit('dtc', { ecu: id }); return { ok: true, count: dtcMem[id].length }; },
      on: fn => { listeners.push(fn); return () => { const i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1); }; },
      clock,
      activeCount: () => txns.filter(x => !terminal(x.status)).length,
      limit: LIMIT,
    };
  }

  root.LoomUds = {
    createEngine, validateDid, buildRequest, decodePdu, route, parseRawHex, rawDidOf, sidName, SIDS, SUBFN_SIDS, RAW_MAX_BYTES, RAW_TEXT_MAX,
    ECUS, DIDS, TRANSPORTS, NRC, SIM_NRCS, TIMING, SCENARIOS, LATENCIES, PENDING_INTERVAL, LATE_AFTER,
    hex2, hex4, hexs, didInfo, validateCatalog, decodeCustom, parseFixture, CAT_FORMAT, CAT_LIMITS, DID_TYPES, PRESET_ECUS, PRESET_DIDS,
    SESSIONS, S3_MS, P2_SRV, DTC_BITS, DTC_DESC, DTC_AVAIL, dtcText, statusBits, parseDtcRecords,
    validateProfile, routeFromProfile, presetProfile, presetValues, hostError, hexId, kbit,
    CHANNELS, BITRATES, LIMITS, FIELDS, CONFIG_FORMAT, MAX_PROFILES,
  };
})(typeof window !== 'undefined' ? window : globalThis);
