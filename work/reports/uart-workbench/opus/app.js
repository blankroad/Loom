/* Loom UART Bench — Opus proposal. Native JS, no dependencies, no eval, no network, no real ports.
   Everything below the "SIM" section is a local simulator. Persistence: localStorage keys prefixed
   `loomUartOpus.v1.` only (never clears other keys). */
(() => {
'use strict';

/* ============================== utils ============================== */
const LS = 'loomUartOpus.v1.';
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const ic = (id, cls = 'i') => `<svg class="${cls}" aria-hidden="true"><use href="#i-${id}"/></svg>`;
let seq = 0;
const nid = p => p + Date.now().toString(36).slice(-4) + (++seq).toString(36);
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const enc = new TextEncoder();
const store = {
  get(k, d) { try { const v = localStorage.getItem(LS + k); return v == null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem(LS + k, JSON.stringify(v)); } catch { /* storage unavailable: run in-memory */ } },
  del(k) { try { localStorage.removeItem(LS + k); } catch { /* ignore */ } },
};
const fmtBytes = n => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} kB` : `${(n / 1048576).toFixed(2)} MB`;
const pad = (n, w = 2) => String(n).padStart(w, '0');
const fmtTs = t => { const d = new Date(t); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`; };
const EOLS = { CR: '\r', LF: '\n', CRLF: '\r\n', none: '' };
const MINW = 240, MINH = 150, DIV = 6;

/* ============================== defaults ============================== */
const DEF_SESSIONS = [
  { id: 's-mcu', alias: 'mcu', group: 'ECU bench A', port: '/dev/ttyUSB0', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', echo: false, ts: true, autoReconnect: true, profile: 'mcu', color: '#0969da' },
  { id: 's-aux', alias: 'aux', group: 'ECU bench A', port: '/dev/ttyUSB1', baud: 9600, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CRLF', echo: false, ts: true, autoReconnect: true, profile: 'aux', color: '#bf8700' },
  { id: 's-console', alias: 'console', group: 'Gateway SoM', port: '/dev/ttyUSB2', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', echo: false, ts: true, autoReconnect: true, profile: 'linux', color: '#8250df' },
  { id: 's-debug', alias: 'debug', group: 'Gateway SoM', port: '/dev/ttyACM0', baud: 921600, dataBits: 8, parity: 'N', stopBits: 1, flow: 'RTS/CTS', eol: 'LF', echo: false, ts: true, autoReconnect: true, profile: 'log', color: '#1b7c83' },
];
const DEF_MACROS = [
  { id: 'm-login', name: 'Login', target: 'active', hotkey: 'F2', confirm: true, pinned: true,
    vars: [{ name: 'user', def: 'root' }, { name: 'pass', def: 'loom' }],
    steps: [
      { type: 'send', text: '', eol: 'session' },
      { type: 'wait', text: 'login:', ms: 2000 },
      { type: 'send', text: '${user}', eol: 'session' },
      { type: 'wait', text: 'Password:', ms: 2000 },
      { type: 'send', text: '${pass}', eol: 'session' },
      { type: 'wait', text: 'Welcome', ms: 3000 },
    ] },
  { id: 'm-status', name: 'Read status', target: 'active', hotkey: 'F3', confirm: true, pinned: true, vars: [],
    steps: [
      { type: 'send', text: 'status', eol: 'session' },
      { type: 'wait', text: 'OK', ms: 2000 },
      { type: 'delay', ms: 300 },
      { type: 'send', text: 'temp', eol: 'session' },
      { type: 'wait', text: 'OK', ms: 2000 },
    ] },
];
const PROFILES = { mcu: { label: 'MCU shell (login, status, temp)', devBaud: 115200 }, aux: { label: 'AUX power controller (AT)', devBaud: 9600 }, linux: { label: 'Linux console (U-Boot, kernel, sh)', devBaud: 115200 }, log: { label: '로그 스트림 (읽기 전용)', devBaud: 921600 } };
/* SIM port discovery: same shape as the existing worker method `uart.ports` -> { ports:[{path,vid,pid,serial,manufacturer,product,interface,location}], truncated }.
   Future adapter: replace uartPortsSim() with net.call('uart.ports', {}). No real enumeration here. */
const PORT_REGISTRY = [
  { path: '/dev/ttyUSB0', vid: 0x0403, pid: 0x6001, serial: 'A10K2B3X', manufacturer: 'FTDI', product: 'FT232R USB UART', interface: 0, location: '1-1.2' },
  { path: '/dev/ttyUSB1', vid: 0x10c4, pid: 0xea60, serial: '0001', manufacturer: 'Silicon Labs', product: 'CP2102N USB to UART Bridge', interface: 0, location: '1-1.3' },
  { path: '/dev/ttyUSB2', vid: 0x067b, pid: 0x23a3, serial: 'GW0C2241', manufacturer: 'Prolific', product: 'PL2303GT USB Serial', interface: 0, location: '1-1.4' },
  { path: '/dev/ttyACM0', vid: 0x0483, pid: 0x374b, serial: '066DFF55', manufacturer: 'STMicroelectronics', product: 'STLINK-V3 VCP', interface: 2, location: '1-2' },
  { path: '/dev/ttyUSB3', vid: 0x1a86, pid: 0x7523, serial: null, manufacturer: 'QinHeng', product: 'CH340 serial', interface: 0, location: '1-1.1' },
];
function uartPortsSim() {
  const gone = new Set(Object.values(T).filter(t => t.conn === 'lost' || t.conn === 'waiting').map(t => sess(t.sid)?.port)); // unplugged in sim
  return { ports: PORT_REGISTRY.filter(p => !gone.has(p.path)).map(p => ({ ...p })), truncated: false };
}
const hex4 = n => n == null ? '????' : n.toString(16).padStart(4, '0');
function portAnno(path, selfSid) {
  const t = Object.values(T).find(x => sess(x.sid)?.port === path);
  return { openBy: t ? { tab: t.id, sid: t.sid, alias: sess(t.sid).alias, pane: paneNo(leafOfTab(t.id)?.id), conn: t.conn } : null,
    savedAs: S.sessions.filter(x => x.port === path && x.id !== selfSid).map(x => ({ id: x.id, alias: x.alias })) };
}
const COLORS = ['#0969da', '#bf8700', '#8250df', '#1b7c83', '#cf222e', '#1a7f37', '#57606a'];
const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1000000];
const RESERVED_KEYS = new Set(['Alt+Shift+=', 'Alt+Shift+-', 'Alt+Shift+Enter', 'Alt+Shift+W', 'Alt+1', 'Alt+2', 'Alt+0', 'Alt+F',
  'Alt+ArrowLeft', 'Alt+ArrowRight', 'Alt+ArrowUp', 'Alt+ArrowDown', 'Alt+Shift+ArrowLeft', 'Alt+Shift+ArrowRight', 'Alt+Shift+ArrowUp', 'Alt+Shift+ArrowDown']);

/* ============================== state ============================== */
const S = {
  sessions: store.get('sessions', null) || structuredClone(DEF_SESSIONS),
  macros: store.get('macros', null) || structuredClone(DEF_MACROS),
  ui: Object.assign({ side: 'sessions', collapsed: false, closedGroups: [] }, store.get('ui', {})),
  runVars: store.get('runVars', {}),
  recPrefs: store.get('recPrefs', {}), // recording preferences only (never payload, never auto-resume)
  layout: null, // { root, active, max }
};
if (!store.get('mig2', false)) { S.sessions.forEach(x => { if (x.profile !== 'log') x.echo = false; }); store.set('mig2', true); } // rev3: sim devices echo; avoid double echo
if (!store.get('mig3', false)) { S.sessions.forEach(x => { x.ts = true; }); store.set('mig3', true); } // rev4: receive-time gutter default on
/* rev6: legacy per-macro `target: <sessionId>` becomes an explicit, visibly labelled override `fixed: [sessionId…]` (wins over the panel scope) */
function migrateMacro(m) { if (!Array.isArray(m.fixed)) m.fixed = m.target && m.target !== 'active' ? [m.target] : []; delete m.target; delete m.confirm; return m; }
S.macros.forEach(migrateMacro);
/* macro execution scope (macro panel) — independent of the keyboard input target (toolbar BC). Not persisted: reload = 'active'. */
const MS = { mode: 'active', sel: [] }; // sel = session ids for 'selected'
const T = {}; // tabId -> runtime
let tabSeq = 0;

const sess = id => S.sessions.find(s => s.id === id);
const tabOfSession = sid => Object.values(T).find(t => t.sid === sid);
const persist = { t: 0, all() { clearTimeout(this.t); this.t = setTimeout(saveNow, 150); } };
function saveNow() {
  store.set('sessions', S.sessions); store.set('macros', S.macros); store.set('ui', S.ui); store.set('runVars', S.runVars); store.set('recPrefs', S.recPrefs);
  if (S.layout) {
    const tabs = {}; for (const t of Object.values(T)) tabs[t.id] = { sid: t.sid, mode: t.mode, theme: t.theme, eol: t.eol };
    store.set('layout', { root: S.layout.root, active: S.layout.active, max: S.layout.max, tabs });
  }
}

/* ============================== layout tree ============================== */
const leaf = (tabs = []) => ({ t: 'leaf', id: nid('p'), tabs, on: tabs[0] || null });
function walk(n, f, parent = null) { f(n, parent); if (n.t === 'split') { walk(n.a, f, n); walk(n.b, f, n); } }
function leaves() { const r = []; walk(S.layout.root, n => { if (n.t === 'leaf') r.push(n); }); return r; }
function findNode(id) { let r = null; walk(S.layout.root, n => { if (n.id === id) r = n; }); return r; }
function parentOf(id) { let r = null; walk(S.layout.root, (n, p) => { if (n.id === id) r = p; }); return r; }
function leafOfTab(tid) { return leaves().find(l => l.tabs.includes(tid)) || null; }
function activeLeaf() { return findNode(S.layout.active) || leaves()[0]; }
function activeTab() { const l = activeLeaf(); return l && l.on ? T[l.on] : null; }
function paneNo(id) { return leaves().findIndex(l => l.id === id) + 1; }
function replaceNode(oldId, nn) {
  const p = parentOf(oldId);
  if (!p) S.layout.root = nn; else if (p.a.id === oldId) p.a = nn; else p.b = nn;
}
function minSize(n, dir) {
  if (n.t === 'leaf') return dir === 'row' ? MINW : MINH;
  return n.dir === dir ? minSize(n.a, dir) + minSize(n.b, dir) + DIV : Math.max(minSize(n.a, dir), minSize(n.b, dir));
}
/* split leaf `lid` toward `zone` (left/right/top/bottom); new leaf gets `tabs`. Returns new leaf or null if too small. */
function splitLeaf(lid, zone, tabs = [], force = false) {
  const L = findNode(lid); if (!L) return null;
  const dir = zone === 'left' || zone === 'right' ? 'row' : 'col';
  const el = paneEl(lid);
  if (el && !force && !isNarrow()) {
    const r = el.getBoundingClientRect(); const need = 2 * (dir === 'row' ? MINW : MINH) + DIV;
    if ((dir === 'row' ? r.width : r.height) < need) { toast(`pane ${paneNo(lid)}이(가) 너무 작아 ${dir === 'row' ? '가로' : '세로'}로 나눌 수 없습니다 (최소 ${dir === 'row' ? MINW : MINH}px). 경계를 끌어 넓히거나 최대화하세요.`); return null; }
  }
  const nl = leaf(tabs);
  const first = zone === 'left' || zone === 'top';
  const sp = { t: 'split', id: nid('s'), dir, r: 0.5, a: first ? nl : L, b: first ? L : nl };
  replaceNode(lid, sp);
  S.layout.active = nl.id;
  if (S.layout.max) S.layout.max = null;
  return nl;
}
function removeLeaf(lid) {
  const p = parentOf(lid);
  if (!p) return false; // root leaf stays
  const sib = p.a.id === lid ? p.b : p.a;
  replaceNode(p.id, sib);
  if (S.layout.max === lid) S.layout.max = null;
  if (S.layout.active === lid) { let f = sib; while (f.t === 'split') f = f.a; S.layout.active = f.id; }
  return true;
}
function defaultLayout() {
  const a = leaf(), b = leaf();
  return { root: { t: 'split', id: nid('s'), dir: 'row', r: 0.56, a, b }, active: a.id, max: null, _init: [['s-mcu', a.id], ['s-aux', b.id]] };
}

/* ============================== tabs runtime ============================== */
function newTab(sid, meta = {}) {
  const id = meta.id || ('t' + (++tabSeq).toString(36) + Date.now().toString(36).slice(-3));
  const s = sess(sid);
  T[id] = { id, sid, lines: [], rx: 0, tx: 0, conn: 'disconnected', mode: meta.mode || 'text', theme: meta.theme || 'dark', eol: meta.eol || null,
    paused: false, pauseMark: 0, clearIdx: 0, search: { open: false, q: '', filter: false, cur: -1 }, hist: [], hi: -1, draft: '',
    stick: true, scrollTop: 0, waiters: [], run: null, sim: null, retry: 0, rxAll: '', rxBase: 0, la: { arr: [], col: 0, t0: 0 }, txLine: '', xpend: [], xpendLen: 0, tsMarks: [], lastMark: null, rec: null, recLast: null, recPicking: false };
  createXterm(T[id]);
  if (s) connect(id, { first: true, quiet: true });
  return T[id];
}
function pushLine(tab, d, s, e) {
  tab.lines.push({ t: Date.now(), d, s, e });
  if (tab.lines.length > 3000) { const cut = 500; tab.lines.splice(0, cut); tab.clearIdx = Math.max(0, tab.clearIdx - cut); tab.pauseMark = Math.max(0, tab.pauseMark - cut); }
  dirty(tab.id);
}
function sysLine(tab, s) {
  pushLine(tab, 'sys', s);
  const mid = tab.la.arr.length > 0;
  tab.rec && tab.rec.note(s, Date.now());
  xwrite(tab, `${mid ? '\r\n' : ''}\x1b[2;3m${/^E:/.test(s) ? '\x1b[31m' : ''}· ${s}\x1b[0m\r\n`);
  if (mid) tab.la = { arr: [], col: 0, t0: 0 };
  if (tab.sim && tab.sim.pActive && tab.conn === 'connected') tab.sim.redraw();
}

/* ============================== xterm surface (vendored @xterm/xterm 6.0.0, MIT) ============================== */
const XT = globalThis.Terminal;
const XTHEME = {
  dark: { background: '#0f1216', foreground: '#d0d7de', cursor: '#d0d7de', cursorAccent: '#0f1216', selectionBackground: 'rgba(56,139,253,0.45)', black: '#484f58', red: '#ff7b72', green: '#56d364', yellow: '#e3b341', blue: '#79c0ff', magenta: '#d2a8ff', cyan: '#76e3ea', white: '#d0d7de', brightBlack: '#6e7781', brightRed: '#ffa198', brightGreen: '#7ee787', brightYellow: '#f2cc60', brightBlue: '#a5d6ff', brightMagenta: '#e2c5ff', brightCyan: '#b3f0ff', brightWhite: '#ffffff' },
  light: { background: '#ffffff', foreground: '#1f2328', cursor: '#1f2328', cursorAccent: '#ffffff', selectionBackground: '#b6e3ff', black: '#1f2328', red: '#cf222e', green: '#1a7f37', yellow: '#9a6700', blue: '#0969da', magenta: '#8250df', cyan: '#1b7c83', white: '#6e7781', brightBlack: '#8c959f', brightRed: '#a40e26', brightGreen: '#116329', brightYellow: '#7d4e00', brightBlue: '#0550ae', brightMagenta: '#6639ba', brightCyan: '#1b7c83', brightWhite: '#424a53' },
};
const stripAnsi = x => x.replace(/\x1b\[[0-9;?]*[A-Za-z~]|\x1b[()][A-Z0-9]|\x1b[A-Za-z]/g, '');
function createXterm(tab) {
  if (!XT) return;
  try {
    const xt = new XT({ fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace', fontSize: 12.5, lineHeight: 1.2, cursorBlink: true, scrollback: 3000, theme: XTHEME[tab.theme] || XTHEME.dark });
    const host = document.createElement('div'); host.className = 'xhost'; host.dataset.tab = tab.id;
    tab.xt = xt; tab.host = host; tab.xopen = false;
    xt.onData(d => keyInput(tab.id, d));
    xt.attachCustomKeyEventHandler(ev => xtKeyFilter(ev, tab));
  } catch (err) { console.warn('xterm unavailable', err); tab.xt = null; }
}
function disposeTab(tab) {
  if (tab.rec) tab.rec.stop('탭/pane 닫힘으로 기록 종료'); // close continues after the tab is gone; result is toasted
  try { tab.ro && tab.ro.disconnect(); tab.xt && tab.xt.dispose(); } catch { /* already gone */ }
}
/* t = host receive time of these bytes; kept through pause so the gutter shows arrival, not render time */
function xwrite(tab, x, t = Date.now()) {
  if (!tab.xt) return;
  if (tab.paused) { tab.xpend.push([t, x]); tab.xpendLen += x.length; while (tab.xpendLen > 300000 && tab.xpend.length > 1) { const d = tab.xpend.shift(); tab.xpendLen -= d[1].length; tab.dispGap = (tab.dispGap || 0) + enc.encode(d[1]).length; tab.gapShown = false; } return; } // long pause: oldest display chunks evicted -> counted as display gap (recording already has them)
  const clr = x.includes('\x1b[2J');
  try {
    if (tab.dispGap && !tab.gapShown) { tab.gapShown = true; tab.xt.write(`\r\n\x1b[7;33m · 표시 누락 ${tab.dispGap} B (터미널 화면만, 기록·로그에는 포함) \x1b[0m\r\n`); }
    tab.xt.write(x, () => markRows(tab, t, x, clr));
  } catch (e) { tab.dispGap = (tab.dispGap || 0) + enc.encode(x).length; tab.gapShown = false; console.warn('xterm write dropped (display only)', e); } // recording tapped the raw bytes before this
}
function flushPaused(tab) { const p = tab.xpend; tab.xpend = []; tab.xpendLen = 0; for (const [t, x] of p) xwrite(tab, x, t); }
/* receive-time gutter: one xterm marker per buffer row = time its first byte arrived (markers follow scrollback trimming) */
function markRows(tab, t, x, clr) {
  const xt = tab.xt; if (!xt || !T[tab.id]) return;
  const b = xt.buffer.active; const cur = b.baseY + b.cursorY;
  if (clr) { for (const m of tab.tsMarks) if (!m.mk.isDisposed && m.mk.line >= b.baseY) m.mk.dispose(); tab.lastMark = null; }
  const start = tab.lastMark && !tab.lastMark.isDisposed ? tab.lastMark.line + 1 : Math.max(clr ? b.baseY : 0, cur - (x.match(/\n/g) || []).length);
  const end = b.cursorX > 0 ? cur : cur - 1; // an empty cursor row is stamped when its first byte arrives
  for (let y = start; y <= end; y++) { let mk; try { mk = xt.registerMarker(y - cur); } catch { mk = null; } if (!mk) continue; tab.tsMarks.push({ mk, t }); tab.lastMark = mk; }
  if (tab.tsMarks.length > 3600) tab.tsMarks = tab.tsMarks.filter(m => !m.mk.isDisposed);
  gutterSoon(tab);
}
function gutterSoon(tab) { if (tab.gRaf) return; tab.gRaf = requestAnimationFrame(() => { tab.gRaf = 0; renderGutter(tab); }); }
function renderGutter(tab) {
  const s = sess(tab.sid); if (!tab.xopen || !s || !s.ts || !T[tab.id]) return;
  const g = $(`.tsgutter[data-tab="${tab.id}"]`); if (!g) return;
  const xt = tab.xt, b = xt.buffer.active, ch = cellSize(tab).height;
  const map = new Map(); for (const m of tab.tsMarks) if (!m.mk.isDisposed) map.set(m.mk.line, m.t);
  let h = '';
  for (let i = 0; i < xt.rows; i++) { const y = b.viewportY + i; const l = b.getLine(y); const tt = l && !l.isWrapped ? map.get(y) : undefined; h += `<div class="tsr" style="height:${ch}px;line-height:${ch}px">${tt ? fmtTs(tt) : (l && l.isWrapped ? '↳' : '')}</div>`; }
  g.innerHTML = h; g.dataset.rows = String(xt.rows);
}
function attachXterm(tid) {
  const tab = T[tid]; if (!tab || !tab.xt) return;
  const slot = $(`.term[data-tab="${tid}"][data-view="xterm"]`); if (!slot) return;
  if (tab.host.parentElement === slot) return;
  slot.appendChild(tab.host);
  if (!tab.xopen) {
    try { tab.xt.open(tab.host); tab.xopen = true; } catch (err) { console.warn('xterm open failed', err); return; }
    if (window.ResizeObserver) { tab.ro = new ResizeObserver(() => fitSoon(tid)); tab.ro.observe(tab.host); }
    tab.xt.onRender(() => gutterSoon(tab)); tab.xt.onScroll(() => gutterSoon(tab)); tab.xt.onResize(() => gutterSoon(tab));
  }
  fitSoon(tid); renderGutter(tab); // fill new gutter immediately (no blank frame after re-render)
}
function fitSoon(tid) { const t = T[tid]; if (!t || t.fitRaf) return; t.fitRaf = requestAnimationFrame(() => { t.fitRaf = 0; fitXterm(t); }); }
function cellSize(tab) {
  try { const c = tab.xt._core._renderService.dimensions.css.cell; if (c && c.width > 0 && c.height > 0) return c; } catch { /* private API changed: measure */ }
  const m = document.createElement('span'); const o = tab.xt.options;
  m.style.cssText = `position:absolute;visibility:hidden;white-space:pre;font-family:${o.fontFamily};font-size:${o.fontSize}px`; m.textContent = 'W'.repeat(40);
  document.body.appendChild(m); const r = m.getBoundingClientRect(); m.remove();
  return { width: r.width / 40 || 7.5, height: Math.ceil(o.fontSize * o.lineHeight * 1.18) || 18 };
}
function fitXterm(tab) {
  if (!tab.xt || !tab.xopen || !tab.host.isConnected) return;
  const w = tab.host.clientWidth, h = tab.host.clientHeight; if (w < 30 || h < 20) return;
  const c = cellSize(tab);
  const cols = Math.max(20, Math.floor((w - 12) / c.width)), rows = Math.max(3, Math.floor(h / c.height));
  if (cols !== tab.xt.cols || rows !== tab.xt.rows) tab.xt.resize(cols, rows);
}
function copyText(x) { try { navigator.clipboard && navigator.clipboard.writeText(x).catch(() => {}); } catch { /* clipboard blocked */ } }
/* keys the workbench owns while a terminal is focused; everything else is VT input handled by xterm */
function xtKeyFilter(ev, tab) {
  if (ev.type !== 'keydown') return true;
  const k = comboOf(ev);
  if (RESERVED_KEYS.has(k) || S.macros.some(m => m.hotkey && m.hotkey === k)) return false; // bubbles to workbench handler
  if (ev.ctrlKey && !ev.altKey && !ev.metaKey && ev.code === 'KeyC' && tab.xt.hasSelection()) { copyText(tab.xt.getSelection()); return false; }
  if (ev.ctrlKey && ev.shiftKey && !ev.altKey && ev.code === 'KeyC') { if (tab.xt.hasSelection()) copyText(tab.xt.getSelection()); return false; }
  if (ev.ctrlKey && !ev.altKey && !ev.metaKey && ev.code === 'KeyV') return false; // let the browser paste -> xterm paste -> onData
  return true;
}
/* device -> terminal: raw bytes (with ANSI/CRLF) to xterm, plain text to log lines + macro wait stream */
function feed(tab, raw) {
  const now = Date.now();
  tab.rx += enc.encode(raw).length;
  tab.rec && tab.rec.tap('rx', raw, now); // recording taps the raw stream before pause/filter/clear/line cap
  xwrite(tab, raw, now);
  tab.rxAll += stripAnsi(raw);
  if (tab.rxAll.length > 20000) { const cut = tab.rxAll.length - 10000; tab.rxAll = tab.rxAll.slice(cut); tab.rxBase += cut; }
  for (const w of tab.waiters.slice()) if (tab.rxAll.indexOf(w.text, Math.max(0, w.pos - tab.rxBase)) >= 0) { tab.waiters.splice(tab.waiters.indexOf(w), 1); w.ok(); }
  logAssemble(tab, raw);
  dirty(tab.id);
}
/* single-line cursor emulation so echoed edits (BS, CSI D/C/K, CR redraw) become the final visible line in the log */
function lineEmu(L, raw, t, onLine) {
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === '\x1b') {
      const m = /^\x1b\[([0-9;?]*)([A-Za-z~])/.exec(raw.slice(i, i + 16));
      if (m) { const n = parseInt(m[1], 10) || 1; if (m[2] === 'D') L.col = Math.max(0, L.col - n); else if (m[2] === 'C') L.col += n; else if (m[2] === 'K') L.arr.length = Math.min(L.arr.length, L.col); else if (m[2] === 'J' && m[1] === '2') { L.arr = []; L.col = 0; } i += m[0].length - 1; }
      else i += 1;
      continue;
    }
    if (ch === '\n') { onLine(Array.from(L.arr, c => c ?? ' ').join(''), L.t0 || t); L.arr = []; L.col = 0; L.t0 = 0; continue; }
    if (ch === '\r') { L.col = 0; continue; }
    if (ch === '\b') { L.col = Math.max(0, L.col - 1); continue; }
    if (ch < ' ' && ch !== '\t') continue;
    if (!L.t0) L.t0 = t;
    L.arr[L.col++] = ch;
  }
}
function logAssemble(tab, raw) {
  lineEmu(tab.la, raw, Date.now(), line => { pushLine(tab, 'rx', line); if (tab.sim && tab.sim.garbled()) tab.lines[tab.lines.length - 1].g = 1; });
}
/* keyboard (xterm onData) and macros share this transmit path */
/* send-target mode: 'current' (default, never persisted) or 'all' = recipient set frozen when enabled */
const BC = { on: false, ids: [], excluded: [], at: 0 };
const GROUPS = {}; // gid -> { name, ids }
function bcWhy(t) { const s = t && sess(t.sid); if (!s) return '닫힘'; if (t.conn !== 'connected') return connLabel(t.conn); if (s.profile === 'log') return '읽기 전용'; return ''; }
function bcSnapshot() { BC.ids = []; BC.excluded = []; for (const t of Object.values(T)) { const why = bcWhy(t); if (why) BC.excluded.push({ alias: sess(t.sid)?.alias || '?', why }); else BC.ids.push(t.id); } BC.at = Date.now(); }
function setBroadcast(on) {
  if (on) { bcSnapshot(); if (!BC.ids.length) { BC.on = false; toast('연결된 쓰기 가능 세션이 없어 “연결된 모두”를 켤 수 없습니다'); renderAll(); return; } BC.on = true; toast(`키 입력 대상: 연결된 모두 → ${BC.ids.map(id => sess(T[id].sid).alias).join(', ')} (매크로는 매크로 패널의 실행 대상을 따름)`); }
  else { BC.on = false; BC.ids = []; BC.excluded = []; }
  renderAll();
}
function bcDrop(tid, why) {
  if (!BC.on || !BC.ids.includes(tid)) return;
  BC.ids.splice(BC.ids.indexOf(tid), 1); BC.excluded.push({ alias: sess(T[tid]?.sid)?.alias || '?', why });
  if (!BC.ids.length) { BC.on = false; BC.excluded = []; toast('수신자가 모두 빠져 키 입력 대상을 “현재 세션”으로 되돌렸습니다'); }
}
function bcTargets() { return BC.ids.map(id => T[id]).filter(t => t && !bcWhy(t)); }
function keyInput(tid, d) {
  const tab = T[tid]; if (!tab) return;
  if (BC.on) { // typed bytes go to every frozen recipient (each with its own EOL); routed through the normal transmit path
    const rs = bcTargets(); if (!rs.length) return;
    for (const r of rs) transmit(r, d.replace(/\r/g, EOLS[tabEol(r)]));
    if (!BC.ids.includes(tid) && !tab.bcWarned) { tab.bcWarned = true; toast(`${sess(tab.sid).alias}는 “연결된 모두” 수신자가 아닙니다 — 입력은 ${rs.map(r => sess(r.sid).alias).join(', ')}로만 갑니다`); }
    return;
  }
  if (tab.conn !== 'connected') return;
  transmit(tab, d.replace(/\r/g, EOLS[tabEol(tab)]));
}
function transmit(tab, data) {
  const s = sess(tab.sid);
  tab.tx += enc.encode(data).length;
  tab.rec && tab.rec.tap('tx', data, Date.now());
  if (s && s.echo) xwrite(tab, data.replace(/\x7f/g, '\b \b').replace(/\r\n|\r|\n/g, '\r\n'));
  const plain = stripAnsi(data); let prev = '';
  for (const ch of plain) {
    if (ch === '\r' || (ch === '\n' && prev !== '\r')) { pushLine(tab, 'tx', tab.txLine, tabEol(tab)); tab.txLine = ''; }
    else if (ch === '\x7f' || ch === '\b') tab.txLine = tab.txLine.slice(0, -1);
    else if (ch === '\x03') { pushLine(tab, 'tx', tab.txLine + '^C', 'none'); tab.txLine = ''; }
    else if (ch === '\x15') tab.txLine = '';
    else if (ch >= ' ' || ch === '\t') tab.txLine += ch;
    prev = ch;
  }
  tab.sim && tab.sim.input(data);
}
/* ============================== UART log recording (per target, real-time file sink) ============================== */
const REC_BUF_LIMIT = 5 * 1024 * 1024, REC_FLUSH_MS = 200;
const REC_FILE_BACKLOG = (globalThis.__loomRecTestBacklog | 0) || 8 * 1024 * 1024; // max accepted-but-unwritten bytes (local test may lower it before load)
/* Chrome FSA: data reaches the chosen file only on writable.close(); reopening with keepExistingData copies the file into a new swap file,
   so the checkpoint interval is 1 s and grows with file size (bounded copy cost): 1 s < 4 MB, then +1 s per 4 MB, max 10 s. */
const recCommitMs = bytes => Math.min(10000, 1000 * Math.max(1, Math.ceil(bytes / (4 * 1024 * 1024))));
function fmtFull(t) { const d = new Date(t); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${fmtTs(t)}`; }
function isoLocal(t) { const d = new Date(t); const o = -d.getTimezoneOffset(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${fmtTs(t)}${o >= 0 ? '+' : '-'}${pad(Math.floor(Math.abs(o) / 60))}:${pad(Math.abs(o) % 60)}`; }
function recFileName(alias, fmt, t = Date.now()) { const d = new Date(t); return `${alias}-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.${fmt}`; }
function recPrefs(sid) { return Object.assign({ format: 'log', rx: true, tx: true, fileTs: true }, S.recPrefs[sid] || {}); }
class Recorder {
  constructor(tab, o) {
    const s = sess(tab.sid);
    Object.assign(this, { tabId: tab.id, alias: s.alias, port: s.port, frameS: `${frame(s)} flow ${s.flow}`, sink: o.sink, format: o.format, dirs: { rx: !!o.rx, tx: !!o.tx }, fileTs: !!o.fileTs,
      handle: o.handle || null, writable: o.writable || null, name: o.name, state: 'recording', started: Date.now(), seen: { rx: 0, tx: 0 }, queued: 0, written: 0, committed: 0,
      errors: [], reason: '', pend: '', q: Promise.resolve(), lastSavedAt: 0, chunks: [], rxEmu: { arr: [], col: 0, t0: 0 }, tx: '', txT0: 0, prevTx: '', tail: [], dead: false, closing: null });
    this.fT = setInterval(() => this.flush(), REC_FLUSH_MS);
    if (this.sink === 'file') this.scheduleCommit();
    this.put(this.header());
  }
  get active() { return this.state === 'recording'; }
  header() {
    const t = this.started, dirs = ['rx', 'tx'].filter(k => this.dirs[k]);
    if (this.format === 'jsonl') return JSON.stringify({ type: 'start', t: isoLocal(t), ms: t, alias: this.alias, port: this.port, frame: this.frameS, dirs, clock: 'host-receive', peer: 'simulated (prototype)' }) + '\n';
    return `# Loom UART log — ${this.alias} ${this.port} ${this.frameS}\n# started ${fmtFull(t)} (host clock, UTC${isoLocal(t).slice(-6)}) · ${dirs.join('+').toUpperCase()} · peer: simulated (prototype)\n`;
  }
  line(D, t, text) { return `${this.fileTs ? fmtFull(t) + ' ' : ''}${D}  ${text}\n`; }
  tap(dir, raw, t) {
    if (!this.active || !this.dirs[dir]) return;
    const n = enc.encode(raw).length; this.seen[dir] += n;
    if (this.format === 'jsonl') { this.put(JSON.stringify({ type: 'data', t: isoLocal(t), ms: t, dir, len: n, data: raw }) + '\n'); return; }
    if (dir === 'rx') { lineEmu(this.rxEmu, raw, t, (text, t0) => this.put(this.line('RX', t0, text))); return; }
    for (const ch of stripAnsi(raw)) { // TX: keystroke bytes -> logical lines
      if (ch === '\r' || (ch === '\n' && this.prevTx !== '\r')) { this.put(this.line('TX', this.txT0 || t, this.tx)); this.tx = ''; this.txT0 = 0; }
      else if (ch !== '\n') { if (!this.txT0) this.txT0 = t; if (ch === '\x7f' || ch === '\b') this.tx = this.tx.slice(0, -1); else if (ch < ' ' && ch !== '\t') this.tx += `<${ch.charCodeAt(0).toString(16).padStart(2, '0')}>`; else this.tx += ch; }
      this.prevTx = ch;
    }
  }
  note(text, t) { if (this.active) this.put(this.format === 'jsonl' ? JSON.stringify({ type: 'note', t: isoLocal(t), ms: t, text }) + '\n' : `${this.fileTs ? fmtFull(t) + ' ' : ''}--  ${text}\n`); }
  put(text) {
    this.pend += text;
    for (const l of text.split('\n')) if (l) this.tail.push(l.length > 200 ? l.slice(0, 200) + '…' : l);
    if (this.tail.length > 20) this.tail.splice(0, this.tail.length - 20);
    if (this.pend.length > 65536) this.flush();
  }
  flush() {
    if (!this.pend || this.dead) return;
    const chunk = this.pend; this.pend = ''; const n = enc.encode(chunk).length; this.queued += n;
    if (this.sink === 'buffer') {
      if (this.written + n > REC_BUF_LIMIT) { this.fail(new Error(`브라우저 버퍼 한도 ${fmtBytes(REC_BUF_LIMIT)} 초과 — 기록 중지 (그때까지 내용은 내보내기 가능)`)); return; }
      this.chunks.push(chunk); this.written += n; return; // memory only: never counted as saved to a file
    }
    if (this.queued - this.written > REC_FILE_BACKLOG) { this.fail(new Error(`파일 쓰기가 밀려 대기 ${fmtBytes(this.queued - this.written)} > ${fmtBytes(REC_FILE_BACKLOG)} — 기록 중지 (이미 확정된 ${fmtBytes(this.committed)}는 파일에 있음)`)); return; }
    this.q = this.q.then(async () => { if (this.dead) return; await this.writable.write(chunk); this.written += n; }).catch(e => this.fail(e)); // serialized, bounded backlog
  }
  /* FSA writables commit on close(): checkpoint = close + reopen(keepExistingData) + seek, so data reaches the file every few seconds */
  scheduleCommit() { clearTimeout(this.cT); this.cT = setTimeout(() => { this.flush(); this.commit(); if (this.state === 'recording' && !this.dead) this.scheduleCommit(); }, recCommitMs(this.committed)); }
  commit() {
    if (this.sink !== 'file' || this.dead) return;
    this.q = this.q.then(async () => {
      if (this.dead || this.written === this.committed) return;
      await this.writable.close(); this.committed = this.written; this.lastSavedAt = Date.now(); // durable only after close
      this.writable = await this.handle.createWritable({ keepExistingData: true }); await this.writable.seek(this.written);
    }).catch(e => this.fail(e));
  }
  get pending() { return this.queued + enc.encode(this.pend).length - this.committed; } // accepted but not yet in the file
  fail(e) {
    if (this.dead) return; this.dead = true; this.state = 'error';
    this.errors.push(`${e && e.name && e.name !== 'Error' ? e.name + ': ' : ''}${(e && e.message) || e}`);
    clearInterval(this.fT); clearTimeout(this.cT);
    try { const w = this.writable; if (w && w.abort) Promise.resolve(w.abort()).catch(() => {}); } catch { /* ignore */ }
    recChanged(this);
  }
  stop(reason) {
    if (this.closing) return this.closing;
    if (this.state !== 'recording') return (this.closing = Promise.resolve(this));
    const t = Date.now(); this.reason = reason || '사용자 중지';
    if (this.format === 'log') {
      if (this.rxEmu.arr.length) { this.put(this.line('RX', this.rxEmu.t0 || t, Array.from(this.rxEmu.arr, c => c ?? ' ').join('') + '  ⟨줄바꿈 없음⟩')); this.rxEmu = { arr: [], col: 0, t0: 0 }; }
      if (this.tx) { this.put(this.line('TX', this.txT0 || t, this.tx + '  ⟨줄바꿈 없음⟩')); this.tx = ''; }
    }
    this.put(this.format === 'jsonl' ? JSON.stringify({ type: 'stop', t: isoLocal(t), ms: t, reason: this.reason, rxBytes: this.seen.rx, txBytes: this.seen.tx }) + '\n'
      : `# stopped ${fmtFull(t)} · ${this.reason} · RX ${this.seen.rx} B · TX ${this.seen.tx} B\n`);
    this.state = 'stopping'; clearInterval(this.fT); clearTimeout(this.cT); this.flush(); recChanged(this);
    if (this.sink === 'buffer') { if (!this.dead) this.state = 'closed'; recChanged(this); return (this.closing = Promise.resolve(this)); }
    this.closing = this.q.then(async () => { if (this.dead) return; await this.writable.close(); this.committed = this.written; this.lastSavedAt = Date.now(); this.state = 'closed'; })
      .catch(e => this.fail(e)).then(() => { recChanged(this); return this; });
    return this.closing;
  }
  exportBuffer() {
    const a = document.createElement('a'); a.href = URL.createObjectURL(new Blob(this.chunks, { type: this.format === 'jsonl' ? 'application/x-ndjson' : 'text/plain' }));
    a.download = this.name.replace(/ \(버퍼\)$/, ''); document.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }
  info() { return { state: this.state, sink: this.sink, format: this.format, name: this.name, dirs: this.dirs, fileTs: this.fileTs, rxBytes: this.seen.rx, txBytes: this.seen.tx, queued: this.queued, written: this.written, committed: this.committed, pending: this.pending, lastSavedAt: this.lastSavedAt ? fmtFull(this.lastSavedAt) : null, errors: this.errors.slice(), reason: this.reason, tail: this.tail.slice() }; }
}
function recChanged(rec) {
  const t = T[rec.tabId];
  if ((rec.state === 'closed' || rec.state === 'error') && !rec.toasted) {
    rec.toasted = true;
    toast(rec.state === 'closed' ? `${rec.alias} 기록 ${rec.sink === 'file' ? '파일 저장 완료' : '버퍼 종료 (내보내기 필요)'}: ${rec.name} · ${fmtBytes(rec.written)}` : `${rec.alias} 기록 오류: ${rec.errors[rec.errors.length - 1]}`);
    if (t && t.rec === rec) { t.recLast = rec; t.rec = null; }
  }
  if (t) { const L = leafOfTab(t.id); if (L) structural(L.id); refreshRecDialog(t.id); }
}
function recBytesText(x) {
  return x.sink === 'file'
    ? `RX ${fmtBytes(x.seen.rx)} · TX ${fmtBytes(x.seen.tx)} · 파일 확정 ${fmtBytes(x.committed)} · 쓰는 중 ${fmtBytes(x.written - x.committed)} · 미확정 합계 ${fmtBytes(Math.max(0, x.pending))} · 마지막 저장 ${x.lastSavedAt ? fmtTs(x.lastSavedAt) : '아직 없음'}`
    : `RX ${fmtBytes(x.seen.rx)} · TX ${fmtBytes(x.seen.tx)} · 메모리 버퍼 ${fmtBytes(x.written)} / ${fmtBytes(REC_BUF_LIMIT)} (파일에 저장되지 않음 — 내보내기 필요)`;
}
function recState(t) { return t.recPicking ? 'picking' : t.rec ? t.rec.state : t.recLast ? t.recLast.state : 'idle'; }
function recPillHTML(t) {
  const st = recState(t); const r = t.rec;
  const label = { idle: 'REC', picking: '파일 선택…', recording: r && r.sink === 'file' ? `● 저장 ${fmtBytes(r.committed)}` : `● 버퍼 ${fmtBytes(r ? r.written : 0)}`, stopping: '저장 중…', closed: 'REC', error: 'REC 오류' }[st];
  const title = st === 'recording' ? `기록 중 → ${r.sink === 'file' ? '파일' : '브라우저 버퍼'} ${r.name} · 눌러서 상태/중지` : st === 'error' ? '기록 오류 — 눌러서 확인' : 'UART 로그 기록 (파일 위치 직접 선택)';
  return `<button type="button" class="pill rec rec-${st}" data-act="rec" data-tab="${t.id}" data-rec-state="${st}" title="${esc(title)}">${esc(label)}</button>`;
}
function recStartFile(tid, prefs) {
  const t = T[tid]; const s = sess(t.sid);
  if (typeof window.showSaveFilePicker !== 'function') return Promise.reject(Object.assign(new Error('이 브라우저는 파일 직접 저장(File System Access API)을 지원하지 않습니다'), { name: 'NotSupportedError' }));
  const name = recFileName(s.alias, prefs.format);
  let p;
  try { p = window.showSaveFilePicker({ suggestedName: name, types: [prefs.format === 'jsonl' ? { description: 'UART JSON Lines', accept: { 'application/x-ndjson': ['.jsonl'] } } : { description: 'UART text log', accept: { 'text/plain': ['.log'] } }] }); } // must run inside the click (user activation)
  catch (e) { p = Promise.reject(e); }
  t.recPicking = true; structural(leafOfTab(tid)?.id);
  return p.then(async h => {
    const w = await h.createWritable({ keepExistingData: false });
    const tt = T[tid];
    if (!tt || tt.conn !== 'connected') { try { await w.abort(); } catch { /* ignore */ } throw new Error('그 사이 포트가 닫혀 기록을 시작하지 않았습니다'); }
    tt.rec = new Recorder(tt, { sink: 'file', ...prefs, handle: h, writable: w, name: h.name || name });
    return tt.rec;
  }).finally(() => { const tt = T[tid]; if (tt) { tt.recPicking = false; structural(leafOfTab(tid)?.id); } });
}
function recStartBuffer(tid, prefs) { const t = T[tid]; t.rec = new Recorder(t, { sink: 'buffer', ...prefs, name: recFileName(sess(t.sid).alias, prefs.format) + ' (버퍼)' }); return t.rec; }
function recDialogHTML(tid) {
  const t = T[tid]; const s = sess(t.sid); const r = t.rec; const last = t.recLast; const p = recPrefs(t.sid);
  const fsa = typeof window.showSaveFilePicker === 'function'; const conn = t.conn === 'connected';
  const stat = x => `<div class="target-card${x.state === 'error' ? ' bad' : ''}" data-role="recstat" data-rec-state="${x.state}" style="--tag:${s.color}"><span class="dot"></span><b>${{ recording: '기록 중', stopping: '저장 중…', closed: '저장 완료', error: '오류' }[x.state] || x.state}</b><span class="chip">${x.sink === 'file' ? '파일' : '브라우저 버퍼 (파일 아님)'}</span>
    <small>${esc(x.name)} · .${x.format} · ${['rx', 'tx'].filter(k => x.dirs[k]).join('+').toUpperCase()}${x.fileTs ? ' · 줄 시각' : ''}<br><span data-role="recbytes">${recBytesText(x)}</span>${x.reason ? `<br>종료: ${esc(x.reason)}` : ''}${x.errors.length ? `<br><b style="color:var(--err)">${x.errors.map(esc).join('<br>')}</b>` : ''}</small></div>`;
  return `<div class="dlg-h">${ic('file')}<h2>UART 기록 — ${esc(s.alias)}</h2><span class="chip">${esc(s.port)}</span></div>
    <div class="dlg-b">
      ${r ? stat(r) : `
      <fieldset><legend>형식 · 방향</legend><div class="chk-row">
        <label class="chk"><input type="radio" name="format" value="log" ${p.format === 'log' ? 'checked' : ''}> .log 읽기 쉬운 텍스트 · 줄 단위 (줄바꿈 전까지의 미완성 줄은 다음 줄바꿈/중지 때 기록)</label>
        <label class="chk"><input type="radio" name="format" value="jsonl" ${p.format === 'jsonl' ? 'checked' : ''}> .jsonl 원문 · 수신/송신 조각 그대로 즉시 (프롬프트·바이너리 포함)</label></div>
        <div class="chk-row" style="margin-top:8px"><label class="chk"><input type="checkbox" name="rx" ${p.rx ? 'checked' : ''}> RX 수신</label><label class="chk"><input type="checkbox" name="tx" ${p.tx ? 'checked' : ''}> TX 송신</label>
        <label class="chk" data-role="filets"><input type="checkbox" name="fileTs" ${p.fileTs ? 'checked' : ''} ${p.format === 'jsonl' ? 'disabled' : ''}> .log 전용: 줄 앞 시각 (YYYY-MM-DD HH:mm:ss.SSS)</label></div>
        <p class="hint" style="margin:6px 0 0">.jsonl 은 각 레코드에 t(ISO, 로컬 오프셋)·ms(epoch)를 항상 넣습니다 — 위 시각 옵션은 .jsonl 에 적용되지 않습니다.</p>
        <p class="hint" style="margin:8px 0 0">화면 시각 표시와 별개입니다. 일시정지·필터·지우기·화면 줄 제한과 무관하게 원시 송수신을 기록합니다. 시작은 매번 직접 눌러야 하며 새로고침 후 자동 재개하지 않습니다.</p></fieldset>
      ${fsa ? '<p class="hint" style="margin:0">“파일 선택”에서 저장 위치를 직접 고릅니다. 이미 있는 파일을 고르면 브라우저가 덮어쓰기를 확인합니다. 브라우저는 close() 때만 파일에 반영하므로 약 1초마다(큰 파일은 최대 10초) 닫았다 다시 열어 확정하고, 중지 시 마지막으로 저장·닫습니다.</p>'
        : '<p class="hint" style="margin:0;color:var(--warn)">이 브라우저는 파일 직접 저장(File System Access)을 지원하지 않습니다. 브라우저 버퍼(최대 5 MB, 메모리에만, 끝나면 내보내기 필요)만 쓸 수 있습니다.</p>'}
      ${conn ? '' : '<p class="hint" style="margin:0;color:var(--err)">포트가 닫혀 있어 기록을 시작할 수 없습니다.</p>'}`}
      ${!r && last ? `<div><div class="hint" style="margin-bottom:4px">지난 기록</div>${stat(last)}</div>` : ''}
      ${(r || last) ? `<pre class="preview" data-role="rectail">${esc((r || last).tail.join('\n'))}</pre>` : ''}
      <div class="err-text" data-role="err"></div>
    </div>
    <div class="dlg-f">
      ${!r && last && last.sink === 'buffer' && last.chunks.length ? `<button type="button" class="btn" data-act="rec-export">${ic('down')}버퍼 내보내기</button>` : ''}
      <span class="grow"></span><button type="button" class="btn" data-close>닫기</button>
      ${r ? (r.state === 'recording' ? `<button type="button" class="btn danger" data-act="rec-stop">${ic('stop')}기록 중지 (저장·닫기)</button>` : '')
        : `<button type="button" class="btn" data-act="rec-buffer" ${conn ? '' : 'disabled'}>브라우저 버퍼로 기록 (파일 아님)</button><button type="button" class="btn primary" data-act="rec-file" ${conn && fsa && !t.recPicking ? '' : 'disabled'}>${ic('file')}파일 선택 후 기록 시작</button>`}
    </div>`;
}
function openRecDialog(tid) {
  const dlg = $('#dlgRec'); dlg.dataset.tab = tid;
  dlg.innerHTML = `<form method="dialog" novalidate>${recDialogHTML(tid)}</form>`;
  if (!dlg.open) dlg.showModal();
}
function refreshRecDialog(tid) { const dlg = $('#dlgRec'); if (dlg && dlg.open && dlg.dataset.tab === tid && T[tid]) dlg.innerHTML = `<form method="dialog" novalidate>${recDialogHTML(tid)}</form>`; }
function recFormPrefs(form) { const fd = new FormData(form); const fts = form.querySelector('input[name=fileTs]'); return { format: fd.get('format') === 'jsonl' ? 'jsonl' : 'log', rx: fd.has('rx'), tx: fd.has('tx'), fileTs: fts ? fts.checked : true }; }
$('#dlgRec').addEventListener('change', e => { if (e.target.name === 'format') { const c = $('#dlgRec input[name=fileTs]'); if (c) c.disabled = e.target.value === 'jsonl'; } });
$('#dlgRec').addEventListener('click', e => {
  const dlg = $('#dlgRec'); const tid = dlg.dataset.tab; const t = T[tid]; const form = $('form', dlg);
  if (e.target.closest('[data-close]')) { e.preventDefault(); dlg.close(); return; }
  const b = e.target.closest('[data-act]'); if (!b || !t) return;
  const err = $('[data-role="err"]', form);
  if (b.dataset.act === 'rec-file' || b.dataset.act === 'rec-buffer') {
    const prefs = recFormPrefs(form);
    if (!prefs.rx && !prefs.tx) { err.textContent = 'RX 또는 TX 중 하나는 골라야 합니다'; return; }
    S.recPrefs[t.sid] = prefs; persist.all();
    if (b.dataset.act === 'rec-buffer') { recStartBuffer(tid, prefs); toast(`${sess(t.sid).alias}: 브라우저 버퍼 기록 시작 (파일 아님, 최대 ${fmtBytes(REC_BUF_LIMIT)})`); refreshRecDialog(tid); structural(leafOfTab(tid).id); return; }
    recStartFile(tid, prefs).then(rec => { toast(`${rec.alias}: 기록 시작 → ${rec.name}`); refreshRecDialog(tid); })
      .catch(x => { const e2 = $('#dlgRec [data-role="err"]'); const msg = x && x.name === 'AbortError' ? '파일 선택이 취소되었습니다 — 기록하지 않았습니다.' : `파일을 열 수 없습니다 (${x && x.name ? x.name + ': ' : ''}${x && x.message}). 기록하지 않았습니다 — 필요하면 브라우저 버퍼를 쓰세요.`; if (e2) e2.textContent = msg; toast(msg); });
    return;
  }
  if (b.dataset.act === 'rec-stop' && t.rec) { t.rec.stop('사용자 중지'); refreshRecDialog(tid); return; }
  if (b.dataset.act === 'rec-export' && t.recLast) { t.recLast.exportBuffer(); toast('버퍼 내보내기를 시작했습니다'); }
});
window.addEventListener('beforeunload', e => {
  const act = Object.values(T).filter(t => t.rec && (t.rec.state === 'recording' || t.rec.state === 'stopping'));
  if (!act.length) return;
  for (const t of act) t.rec.flush();
  e.preventDefault(); e.returnValue = ''; // browser asks; data after the last checkpoint (adaptive 1–10 s, see recCommitMs) may not be committed if you leave
});

function connect(tid, { first = false, replug = false, quiet = false } = {}) {
  const tab = T[tid]; const s = sess(tab.sid); if (!s) return;
  tab.conn = 'connected'; tab.retry = 0;
  sysLine(tab, `${first ? '포트 열림' : replug ? '장치 재연결됨 · 포트 다시 열림' : '포트 다시 열림'}: ${s.port} @ ${frame(s)} · flow ${s.flow}${s.baud !== PROFILES[s.profile]?.devBaud ? `  (주의: 장치는 ${PROFILES[s.profile]?.devBaud} baud)` : ''}`);
  if (!tab.sim) tab.sim = new Sim(tab, s.profile);
  tab.sim.start(first || replug);
  if (!quiet) structural(leafOfTab(tid)?.id);
}
function disconnect(tid, why = 'user') {
  const tab = T[tid]; if (!tab) return;
  tab.sim && tab.sim.stop();
  if (tab.run && tab.run.status === 'running') { tab.run.stopped = true; tab.run.reason = why === 'lost' ? '장치 분리로 중단' : '포트 닫힘으로 중단'; tab.run.abort && tab.run.abort(); } // never resumes after reconnect
  if (tab.rec) tab.rec.stop(why === 'lost' ? '장치 분리로 기록 종료' : '포트 닫힘으로 기록 종료'); // explicit stop + close, never silently continues
  bcDrop(tid, why === 'lost' ? '장치 분리' : '닫힘');
  for (const w of tab.waiters.splice(0)) w.fail(new Error('포트 닫힘'));
  const s = sess(tab.sid);
  if (why === 'user') { tab.conn = 'disconnected'; sysLine(tab, `포트 닫힘 (사용자): ${s.port}`); }
  else if (why === 'lost') {
    tab.conn = 'lost'; pushLine(tab, 'sys', `E: 장치 분리됨 — ${s.port} 사라짐 (USB disconnect, 시뮬레이션)`);
    if (s.autoReconnect) {
      tab.conn = 'waiting'; sysLine(tab, `자동 재연결 대기: ${s.port} 를 2초마다 확인`);
      clearTimeout(tab.retryT); tab.retryT = setTimeout(() => { if (T[tid] && tab.conn === 'waiting') connect(tid, { replug: true }); }, 4200);
    }
  }
  structural(leafOfTab(tid)?.id);
}
function frame(s) { return `${s.baud} ${s.dataBits}${s.parity}${s.stopBits}`; }
function tabEol(tab) { return tab.eol || sess(tab.sid)?.eol || 'CR'; }
function sendText(tid, text, eolKey, via = 'user') {
  const tab = T[tid]; const s = sess(tab?.sid);
  if (!tab || !s || tab.conn !== 'connected') return false;
  const ek = !eolKey || eolKey === 'session' ? tabEol(tab) : eolKey;
  tab.sentLog = (tab.sentLog || []); tab.sentLog.push({ t: Date.now(), via, text }); if (tab.sentLog.length > 200) tab.sentLog.shift();
  transmit(tab, text + EOLS[ek]);
  return true;
}
function closeTab(tid) {
  const tab = T[tid]; if (!tab) return;
  if (tab.run && tab.run.status === 'running') stopMacro(tid, '탭 닫힘');
  bcDrop(tid, '탭 닫힘');
  tab.sim && tab.sim.stop(); clearTimeout(tab.retryT); disposeTab(tab);
  const L = leafOfTab(tid);
  delete T[tid];
  if (L) { const i = L.tabs.indexOf(tid); L.tabs.splice(i, 1); if (L.on === tid) L.on = L.tabs[Math.min(i, L.tabs.length - 1)] || null; }
  renderAll();
}

/* ============================== session placement ============================== */
/* where: 'tab' (into leaf) | 'left'|'right'|'top'|'bottom' (split leaf) ; index for tab insert */
function placeSession(sid, leafId, where = 'tab', index = null) {
  const s = sess(sid); if (!s) return;
  let tab = tabOfSession(sid);
  if (tab && where === 'tab' && leafOfTab(tab.id)?.id === leafId && index == null) { focusTab(tab.id); return; }
  if (!tab) {
    const busy = portAnno(s.port, sid).openBy;
    if (busy) { focusTab(busy.tab); toast(`${s.port}는 ${busy.alias} (pane ${busy.pane})가 이미 열고 있습니다 — 같은 물리 포트는 한 탭만. 그 탭으로 이동했습니다.`); return; }
    tab = newTab(sid);
  }
  placeTab(tab.id, leafId, where, index);
}
function placeTab(tid, leafId, where = 'tab', index = null) {
  const src = leafOfTab(tid);
  const dst = findNode(leafId); if (!dst) return;
  if (where !== 'tab') {
    if (src && src.id === leafId && src.tabs.length === 1) { toast('탭이 하나뿐인 pane은 자기 자신에게 분할할 수 없습니다'); return; }
    // check size before mutating
    const el = paneEl(leafId);
    if (el && !isNarrow()) {
      const r = el.getBoundingClientRect(); const row = where === 'left' || where === 'right';
      if ((row ? r.width : r.height) < 2 * (row ? MINW : MINH) + DIV) { splitLeaf(leafId, where, []); return; } // emits toast, returns null
    }
    if (src) detachTab(tid, src);
    const nl = splitLeaf(leafId, where, [tid], true);
    if (src && src.id !== leafId && src.tabs.length === 0 && findNode(src.id)) removeLeaf(src.id);
    if (nl) S.layout.active = nl.id;
  } else {
    if (src) {
      const oi = src.tabs.indexOf(tid);
      detachTab(tid, src);
      if (src.id === leafId && index != null && oi < index) index--;
    }
    if (index == null || index > dst.tabs.length) index = dst.tabs.length;
    dst.tabs.splice(index, 0, tid); dst.on = tid; S.layout.active = dst.id;
    if (src && src.id !== dst.id && src.tabs.length === 0) removeLeaf(src.id);
  }
  if (S.layout.max && S.layout.max !== S.layout.active) S.layout.max = null;
  renderAll();
}
function detachTab(tid, L) { const i = L.tabs.indexOf(tid); L.tabs.splice(i, 1); if (L.on === tid) L.on = L.tabs[Math.min(i, L.tabs.length - 1)] || null; }
function focusTab(tid, focusInput = false) {
  const L = leafOfTab(tid); if (!L) return;
  L.on = tid; S.layout.active = L.id;
  if (S.layout.max && S.layout.max !== L.id) S.layout.max = null;
  renderAll();
  if (focusInput) $(`[data-fkey="send:${tid}"]`)?.focus();
}
function setActiveLeaf(id) {
  if (S.layout.active === id) return;
  const prev = S.layout.active; S.layout.active = id;
  $$('.pane').forEach(p => p.classList.toggle('active', p.dataset.leaf === id));
  if (S.ui.side === 'macros' && !S.ui.collapsed) renderSide(); // ▶ target label follows the current session
  renderChrome(); persist.all(); void prev;
}

/* ============================== rendering ============================== */
const treeRoot = $('#treeRoot');
const paneEl = id => $(`.pane[data-leaf="${id}"]`);
const isNarrow = () => window.matchMedia('(max-width: 640px)').matches;
let drag = null, resizing = null, deferred = false;
function renderAll() {
  if ((drag && drag.active) || resizing) { deferred = true; return; }
  renderSide(); renderTree(); renderChrome(); persist.all();
}
function structural(leafId) {
  if ((drag && drag.active) || resizing) { deferred = true; return; }
  if (leafId && paneEl(leafId)) renderPane(leafId); else renderTree();
  renderSide(); renderChrome(); persist.all();
}
function withFocus(fn) {
  const a = document.activeElement; let k = null;
  const xh = a && a.closest && a.closest('.xhost'); const xtid = xh && xh.dataset.tab;
  if (a && a.dataset && a.dataset.fkey) k = { k: a.dataset.fkey, s: a.selectionStart, e: a.selectionEnd };
  fn();
  if (xtid && T[xtid] && T[xtid].xt) { const host = T[xtid].host; if (host.isConnected) T[xtid].xt.focus(); }
  if (k) { const n = $(`[data-fkey="${CSS.escape(k.k)}"]`); if (n && n !== document.activeElement) { n.focus({ preventScroll: true }); try { if (k.s != null) n.setSelectionRange(k.s, k.e); } catch { /* not a text field */ } } }
}
function renderTree() {
  withFocus(() => {
    treeRoot.classList.toggle('has-max', !!S.layout.max);
    treeRoot.innerHTML = nodeHTML(S.layout.root);
    for (const L of leaves()) if (L.on && T[L.on]) renderTerm(L.on, true);
  });
}
function nodeHTML(n) {
  if (n.t === 'leaf') return paneHTML(n);
  const min = `data-mina="${minSize(n.a, n.dir)}" data-minb="${minSize(n.b, n.dir)}"`;
  return `<div class="split ${n.dir}" data-split="${n.id}">
    <div class="child" style="flex:${n.r} 1 0px">${nodeHTML(n.a)}</div>
    <div class="divider" role="separator" tabindex="0" data-split="${n.id}" data-fkey="div:${n.id}" ${min}
      aria-orientation="${n.dir === 'row' ? 'vertical' : 'horizontal'}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(n.r * 100)}"
      aria-label="pane 경계 (${n.dir === 'row' ? '좌우' : '상하'}) — 끌기 또는 화살표로 크기 조절, 더블클릭 50:50"
      title="끌어서 크기 조절 · 더블클릭 50:50"></div>
    <div class="child" style="flex:${1 - n.r} 1 0px">${nodeHTML(n.b)}</div></div>`;
}
function paneHTML(L) {
  const act = S.layout.active === L.id, mx = S.layout.max === L.id, no = paneNo(L.id);
  const tabsH = L.tabs.map(tid => {
    const t = T[tid]; const s = sess(t?.sid); if (!t || !s) return '';
    const dc = t.conn === 'connected' ? '' : t.conn === 'disconnected' ? 'off' : t.conn;
    return `<div class="tab${L.on === tid ? ' on' : ''}" role="tab" tabindex="0" aria-selected="${L.on === tid}" data-tab="${tid}" data-fkey="tab:${tid}" style="--tag:${s.color}"
      title="${esc(s.alias)} — ${esc(s.port)} ${frame(s)} (${connLabel(t.conn)})&#10;끌어서 다른 pane/가장자리로 이동 · 우클릭: 이동 메뉴 · Alt+Shift+방향키">
      <span class="dot ${dc}"></span><span class="t-label">${esc(s.alias)}</span><span class="t-port">${esc(s.port.replace('/dev/', ''))}</span>${t.run && t.run.status === 'running' ? '<span class="run-badge" title="매크로 실행 중"></span>' : ''}${t.rec && t.rec.state === 'recording' ? '<span class="rec-dot" title="기록 중"></span>' : ''}
      <button type="button" class="x" data-act="close-tab" data-tab="${tid}" aria-label="${esc(s.alias)} 탭 닫기" title="탭 닫기 (포트 닫힘)">${ic('x')}</button></div>`;
  }).join('');
  const t = L.on ? T[L.on] : null;
  return `<div class="pane${act ? ' active' : ''}${mx ? ' maxed' : ''}" data-leaf="${L.id}" data-pane-no="${no}" aria-label="pane ${no}">
    <div class="tabstrip" data-leaf="${L.id}">
      <span class="pane-no" title="pane ${no}">${no}</span>
      <div class="tabs" role="tablist" data-leaf="${L.id}">${tabsH}</div>
      <button type="button" class="tab-add" data-act="add-tab" data-leaf="${L.id}" title="이 pane에 세션 열기" aria-label="pane ${no}에 세션 열기">${ic('plus')}</button>
      <div class="pane-acts">
        <button type="button" class="ibtn" data-act="split-right" data-leaf="${L.id}" title="오른쪽으로 분할 (Alt+Shift+=)" aria-label="pane ${no} 오른쪽 분할">${ic('split-r')}</button>
        <button type="button" class="ibtn" data-act="split-down" data-leaf="${L.id}" title="아래로 분할 (Alt+Shift+-)" aria-label="pane ${no} 아래 분할">${ic('split-d')}</button>
        <button type="button" class="ibtn" data-act="max" data-leaf="${L.id}" aria-pressed="${mx}" title="${mx ? '복원' : '최대화'} (Alt+Shift+Enter)" aria-label="pane ${no} ${mx ? '복원' : '최대화'}">${ic(mx ? 'restore' : 'max')}</button>
        <button type="button" class="ibtn danger" data-act="close-pane" data-leaf="${L.id}" title="pane 닫기 (Alt+Shift+W)" aria-label="pane ${no} 닫기">${ic('pane-x')}</button>
      </div>
    </div>
    ${t ? termPaneHTML(t) : pickerHTML(L)}
  </div>`;
}
function connLabel(c) { return { connected: '연결됨', disconnected: '닫힘', lost: '장치 없음', waiting: '재연결 대기' }[c] || c; }
function termPaneHTML(t) {
  const s = sess(t.sid); const id = t.id;
  const newN = t.paused ? t.lines.length - t.pauseMark : 0;
  const ro = s.profile === 'log';
  const overlay = t.conn === 'connected' ? '' : t.conn === 'disconnected'
    ? `<div class="conn-overlay" data-conn="disconnected"><span class="msg"><b>${esc(s.port)} 닫힘.</b> 로그는 남아 있습니다.</span><button type="button" class="btn sm blue" data-act="connect" data-tab="${id}">${ic('plug')}다시 연결</button></div>`
    : `<div class="conn-overlay" data-conn="${t.conn}"><span class="msg"><b>${t.conn === 'waiting' ? '재연결 대기 중…' : '장치 없음.'}</b> ${esc(s.port)} 가 사라졌습니다${t.conn === 'waiting' ? ' · 자동 재연결 켜짐' : ''}.</span>
       <button type="button" class="btn sm blue" data-act="connect" data-tab="${id}">지금 다시 열기</button>${t.conn === 'waiting' ? `<button type="button" class="btn sm" data-act="cancel-retry" data-tab="${id}">대기 취소</button>` : ''}</div>`;
  const r = t.run;
  return `<div class="phead" data-tab="${id}">
      <div class="ph-target" title="이 pane의 실제 대상">
        <span class="ph-dot" style="--tag:${s.color}"></span><span class="ph-alias">${esc(s.alias)}</span><span class="ph-port">${esc(s.port)}</span>
        <span class="ph-frame">${frame(s)} · ${esc(s.flow)} · ${tabEol(t)}${s.echo ? ' · echo' : ''}</span>
        ${BC.on ? (BC.ids.includes(id) ? '<span class="chip bc-in" title="연결된 모두 송신 수신자">ALL</span>' : '<span class="chip bc-out" title="연결된 모두 송신에서 제외됨">ALL 제외</span>') : ''}<span class="ph-state ${t.conn}" data-role="state">${connLabel(t.conn)}</span>
        <span class="ph-ctr" data-role="ctr">RX <b>${fmtBytes(t.rx)}</b> · TX <b>${fmtBytes(t.tx)}</b></span>
      </div>
      <div class="ph-tools">
        <span class="seg" role="group" aria-label="보기 형식"><button type="button" data-act="mode" data-mode="text" data-tab="${id}" aria-pressed="${t.mode === 'text'}" title="실제 터미널 (xterm)">TERM</button><button type="button" data-act="mode" data-mode="hex" data-tab="${id}" aria-pressed="${t.mode === 'hex'}" title="송수신 바이트 HEX 보기">HEX</button></span>
        <button type="button" class="pill" data-act="pause" data-tab="${id}" aria-pressed="${t.paused}" title="화면 갱신 일시정지 (수신은 계속 기록)">${t.paused ? `재개 +${newN}` : '일시정지'}</button>
        <button type="button" class="pill" data-act="clear" data-tab="${id}" title="화면만 지우기 (로그·카운터 보존)">지우기</button>
        <button type="button" class="pill" data-act="ts-toggle" data-tab="${id}" aria-pressed="${!!s.ts}" title="행 앞 수신 시각 ${s.ts ? '숨기기' : '표시'} — 호스트 시계 HH:mm:ss.SSS · 파일 기록 옵션과 별개">시각</button>
        ${recPillHTML(t)}
        <select class="eol-sel" data-act="eol" data-tab="${id}" title="Enter 시 보낼 줄 끝 (이 탭)" aria-label="줄 끝">${Object.keys(EOLS).map(k => `<option value="${k}" ${tabEol(t) === k ? 'selected' : ''}>${k}</option>`).join('')}</select>
        <button type="button" class="ibtn" data-act="macros-menu" data-tab="${id}" title="매크로 실행 → ${esc(s.alias)}" aria-label="${esc(s.alias)} 대상 매크로 메뉴">${ic('macro')}</button>
        <button type="button" class="ibtn" data-act="search" data-tab="${id}" aria-pressed="${t.search.open}" title="검색 (Alt+F)" aria-label="검색">${ic('search')}</button>
        <button type="button" class="ibtn" data-act="theme" data-tab="${id}" title="이 터미널만 ${t.theme === 'dark' ? '밝게' : '어둡게'}" aria-label="터미널 테마 전환 (현재 ${t.theme === 'dark' ? '어두움' : '밝음'})">${ic(t.theme === 'dark' ? 'sun' : 'moon')}</button>
        <button type="button" class="ibtn" data-act="${t.conn === 'connected' ? 'disconnect' : 'connect'}" data-tab="${id}" title="${t.conn === 'connected' ? '포트 닫기' : '포트 열기'}" aria-label="${t.conn === 'connected' ? '포트 닫기' : '포트 열기'}">${ic(t.conn === 'connected' ? 'unplug' : 'plug')}</button>
        <button type="button" class="ibtn" data-act="settings" data-tab="${id}" title="세션 설정" aria-label="${esc(s.alias)} 세션 설정">${ic('gear')}</button>
        <button type="button" class="ibtn" data-act="more" data-tab="${id}" title="더보기: 탭 이동, 로그 저장, 타임스탬프, 분리 시뮬레이션" aria-label="더보기">${ic('more')}</button>
      </div>
    </div>
    <div class="search${t.search.open ? ' open' : ''}" data-tab="${id}">
      <input type="search" data-act="search-q" data-tab="${id}" data-fkey="search:${id}" placeholder="로그 검색 (Enter 다음, Shift+Enter 이전)" value="${esc(t.search.q)}" aria-label="로그 검색">
      <span class="count" data-role="count"></span>
      <label><input type="checkbox" data-act="search-filter" data-tab="${id}" ${t.search.filter ? 'checked' : ''}> 일치 줄만</label>
      <button type="button" class="ibtn" data-act="search" data-tab="${id}" title="검색 닫기" aria-label="검색 닫기">${ic('x')}</button>
    </div>
    <div class="term-wrap t-${t.theme}" data-theme="${t.theme}" data-tab="${id}">
      ${logView(t) ? `<div class="term" data-tab="${id}" data-mode="${t.mode}" data-view="log"><div class="tlines" role="log" aria-label="${esc(s.alias)} ${t.mode === 'hex' ? 'HEX' : '로그 검색'} 보기" data-tab="${id}"></div></div>`
        : `<div class="term xslot" data-tab="${id}" data-mode="${t.mode}" data-view="xterm" data-ts="${s.ts ? 'on' : 'off'}" title="" aria-label="${esc(s.alias)} 터미널 (${esc(s.port)})"><div class="tsgutter" data-tab="${id}" aria-hidden="true" title="호스트 수신 시각 HH:mm:ss.SSS (이 PC 시계, 장치 출력 아님)"></div></div>`}
      <div class="term-note" data-role="note"><span>아래에 새 줄</span><button type="button" data-act="to-bottom" data-tab="${id}">맨 아래로</button></div>
      ${overlay}
    </div>
    ${r ? runStripHTML(t) : ''}
    `;
}
function pickerHTML(L) {
  const no = paneNo(L.id);
  return `<div class="picker" data-leaf="${L.id}">
    <h3>pane ${no} — 비어 있음</h3>
    <p>저장된 UART 세션을 이 pane에 엽니다. 다른 pane에서 이미 열린 포트는 이 pane으로 옮겨집니다 (포트는 한 탭만 점유).</p>
    <div class="plist">${S.sessions.map(s => {
      const ot = tabOfSession(s.id); const where = ot ? leafOfTab(ot.id) : null;
      return `<button type="button" class="pitem" data-act="pick" data-sid="${s.id}" data-leaf="${L.id}" style="--tag:${s.color}"><span class="dot"></span><b>${esc(s.alias)}</b><small>${esc(s.port)} · ${frame(s)}${where ? ` · pane ${paneNo(where.id)}에서 옮김` : ''}</small></button>`;
    }).join('')}</div>
    <p class="hint">또는 왼쪽 세션 목록에서 끌어다 놓거나, 다른 pane의 탭을 여기로 끌어오세요.</p></div>`;
}
function runStripHTML(t) {
  const r = t.run; const s = sess(t.sid);
  const stTxt = { running: `실행 중 · ${r.i + 1}/${r.steps.length}`, done: '완료', failed: `실패 · ${r.err || ''}`, stopped: `${r.reason || '중지됨'} · step ${r.i + 1}에서` }[r.status];
  return `<div class="runstrip st-${r.status}" data-run="${r.id}" data-status="${r.status}" data-tab="${t.id}">
    <div class="run-top"><span class="rname">${ic('macro')} ${esc(r.name)}</span><span class="rtarget">→ ${esc(s.alias)} · ${esc(s.port)}</span>
      <span class="rstat ${r.status}" data-role="rstat">${esc(stTxt)}</span>
      ${r.status === 'running' ? `<button type="button" class="btn-stop" data-act="stop-macro" data-tab="${t.id}" title="이 대상의 매크로 중지: 이후 step은 송신하지 않음 (터미널 밖에서 Esc도 가능 — 터미널 포커스 중 Esc는 장치로 전송)">${ic('stop')}중지</button>`
        : `<button type="button" class="ibtn" data-act="dismiss-run" data-tab="${t.id}" title="닫기" aria-label="실행 결과 닫기">${ic('x')}</button>`}</div>
    <div class="steps">${r.steps.map((st, i) => `<span class="step ${st.st === 'pending' ? '' : st.st}" data-step="${i}" title="${esc(stepText(st, r.vars))}${st.note ? ' — ' + esc(st.note) : ''}"><span>${i + 1}. ${esc(stepText(st, r.vars))}</span>${st.note ? `<span>${esc(st.note)}</span>` : ''}</span>`).join('')}</div>
  </div>`;
}
function stepText(st, vars) {
  const v = x => vars ? subst(x, vars) : x;
  if (st.type === 'send') return `send ${st.text === '' ? '⏎' : JSON.stringify(v(st.text))}${st.eol && st.eol !== 'session' ? ' +' + st.eol : ''}`;
  if (st.type === 'delay') return `delay ${st.ms}ms`;
  return `wait ${JSON.stringify(v(st.text))} ≤${st.ms}ms`;
}
function renderPane(lid) {
  const el = paneEl(lid); const L = findNode(lid); if (!el || !L) return;
  withFocus(() => {
    el.outerHTML = paneHTML(L);
    if (L.on && T[L.on]) renderTerm(L.on, true);
  });
}

/* ---- terminal body ---- */
const dirtySet = new Set(); let raf = 0;
function dirty(tid) { dirtySet.add(tid); if (!raf) raf = requestAnimationFrame(flush); }
function flush() {
  raf = 0;
  for (const tid of dirtySet) {
    const t = T[tid]; if (!t) continue;
    const L = leafOfTab(tid); if (!L || L.on !== tid) continue;
    const pe = paneEl(L.id); if (!pe) continue;
    const ctr = $('[data-role="ctr"]', pe); if (ctr) ctr.innerHTML = `RX <b>${fmtBytes(t.rx)}</b> · TX <b>${fmtBytes(t.tx)}</b>`;
    if (t.rec && t.rec.state === 'recording') { const rp = $('[data-act="rec"]', pe); if (rp) rp.textContent = t.rec.sink === 'file' ? `● 저장 ${fmtBytes(t.rec.committed)}` : `● 버퍼 ${fmtBytes(t.rec.written)}`; const rb = $('#dlgRec[open] [data-role="recbytes"]'); if (rb && $('#dlgRec').dataset.tab === tid) rb.textContent = recBytesText(t.rec); }
    if (t.paused) { const pb = $('[data-act="pause"]', pe); if (pb) pb.textContent = `재개 +${t.lines.length - t.pauseMark}`; continue; }
    renderTerm(tid);
  }
  dirtySet.clear();
  renderChromeLight();
}
function hl(text, q) {
  if (!q) return esc(text);
  const lo = text.toLowerCase(), ql = q.toLowerCase(); let i = 0, out = '', j;
  while ((j = lo.indexOf(ql, i)) !== -1) { out += esc(text.slice(i, j)) + '<mark>' + esc(text.slice(j, j + q.length)) + '</mark>'; i = j + q.length; }
  return out + esc(text.slice(i));
}
function cls(l) {
  if (l.d === 'tx') return 'tx'; if (l.d === 'sys') return /^E:/.test(l.s) ? 'sys e' : 'sys';
  if (l.g) return 'garb';
  const s = l.s;
  if (/(^|\s)(E:|E\/|ERR\b|ERROR\b|FAIL|panic|Login incorrect|overrange)/.test(s)) return 'e';
  if (/(^|\s)(W:|W\/|WARN|warning|timeout|VIN_SAG)/i.test(s)) return 'w';
  if (/^(OK|\+READY|ready\.)$/.test(s.trim())) return 'ok';
  if (/(login: ?$|Password: ?$|mcu> ?$|# ?$)/.test(s)) return 'prompt';
  return '';
}
function logView(t) { return t.mode === 'hex' || t.search.open || !t.xt; }
function renderTerm(tid, restore = false) {
  const t = T[tid]; if (!logView(t)) { attachXterm(tid); return; }
  const s = sess(t.sid); const el = $(`.term[data-tab="${tid}"]`); if (!el) return;
  const end = t.paused ? t.pauseMark : t.lines.length;
  let vis = t.lines.slice(t.clearIdx, end);
  const q = t.search.open ? t.search.q : '';
  let matches = 0;
  if (q) { const ql = q.toLowerCase(); vis = vis.filter(l => { const m = l.s.toLowerCase().includes(ql); if (m) matches += l.s.toLowerCase().split(ql).length - 1; return m || !t.search.filter; }); }
  if (vis.length > 1500) vis = vis.slice(-1500);
  const ts = s.ts;
  let h = '';
  if (t.mode === 'hex') {
    for (const l of vis) {
      const tsH = ts ? `<span class="ts">${fmtTs(l.t)}</span>` : '';
      if (l.d === 'sys') { h += `<span class="ln sys">${tsH}${hl(l.s, q)}</span>`; continue; }
      const bytes = enc.encode(l.s + (l.d === 'rx' ? '\r\n' : EOLS[l.e || 'CR']));
      for (let o = 0; o < bytes.length; o += 16) {
        const ch = bytes.slice(o, o + 16);
        const hx = Array.from(ch, b => b.toString(16).padStart(2, '0')).join(' ');
        const asc = Array.from(ch, b => b >= 32 && b < 127 ? String.fromCharCode(b) : '.').join('');
        h += `<span class="ln hx ${l.d}"><span class="dir">${o === 0 ? `${tsH}${l.d.toUpperCase()}` : ''}</span><span class="bytes">${hl(hx, q)}</span><span class="asc">${esc(asc)}</span></span>`;
      }
    }
  } else {
    for (const l of vis) h += `<span class="ln ${cls(l)}">${ts ? `<span class="ts">${fmtTs(l.t)}</span>` : ''}${hl(l.s, q)}</span>`;
  }
  ($('.tlines', el) || el).innerHTML = h;
  el.dataset.lines = String(vis.length);
  const pe = el.closest('.pane');
  if (t.search.open && pe) {
    const c = $('[data-role="count"]', pe);
    const marks = $$('mark', el);
    if (t.search.cur >= marks.length) t.search.cur = marks.length ? 0 : -1;
    if (t.search.cur >= 0 && marks[t.search.cur]) marks[t.search.cur].classList.add('cur');
    if (c) c.textContent = q ? `${t.search.cur >= 0 ? t.search.cur + 1 : 0}/${matches}` : '';
  }
  if (t.stick) el.scrollTop = el.scrollHeight;
  else if (restore) el.scrollTop = t.scrollTop;
  const note = pe && $('[data-role="note"]', pe); if (note) note.classList.toggle('show', !t.stick && !t.paused);
}

/* ---- chrome (shell header chip, status bar, toolbar context) ---- */
function renderChrome() {
  const L = activeLeaf(); const t = activeTab(); const s = t && sess(t.sid);
  $('#tbMaxLabel').textContent = S.layout.max ? '복원' : '최대화';
  $('#tbSide').setAttribute('aria-expanded', String(!S.ui.collapsed)); $('#tbSide').setAttribute('aria-pressed', String(!S.ui.collapsed));
  $('#tbSideLabel').textContent = S.ui.collapsed ? '세션 목록 열기' : '세션 목록 접기';
  $('#railSessions').setAttribute('aria-expanded', String(!S.ui.collapsed && S.ui.side === 'sessions')); $('#railMacros').setAttribute('aria-expanded', String(!S.ui.collapsed && S.ui.side === 'macros'));
  $('#tbMax').setAttribute('aria-pressed', String(!!S.layout.max));
  $('#tbCtx').innerHTML = s ? `대상 <b>${esc(s.alias)}</b> ${esc(s.port)} · pane ${paneNo(L.id)}` : `pane ${L ? paneNo(L.id) : '-'} 비어 있음`;
  $('#modUart').classList.toggle('side-collapsed', !!S.ui.collapsed);
  $('#railSessions').setAttribute('aria-pressed', String(!S.ui.collapsed && S.ui.side === 'sessions'));
  $('#railMacros').setAttribute('aria-pressed', String(!S.ui.collapsed && S.ui.side === 'macros'));
  renderChromeLight();
}
function renderChromeLight() {
  const all = Object.values(T); const con = all.filter(t => t.conn === 'connected').length;
  const running = all.filter(t => t.run && t.run.status === 'running').length;
  $('#shellConn').innerHTML = `<span class="dot" style="background:${con ? 'var(--ok)' : 'var(--faint)'}"></span><span>UART <b>${all.length}</b> 열림<span class="cc-x"> · <b>${con}</b> 연결</span></span>`;
  const L = activeLeaf(); const t = activeTab(); const s = t && sess(t.sid);
  $('#stTarget').innerHTML = s ? `활성 pane ${paneNo(L.id)} › <b>${esc(s.alias)}</b>  ${esc(s.port)}  ${frame(s)}  ${connLabel(t.conn)}  RX ${fmtBytes(t.rx)} TX ${fmtBytes(t.tx)}` : `활성 pane ${L ? paneNo(L.id) : '-'} › 비어 있음`;
  $('#stExtra').textContent = `pane ${leaves().length} · 탭 ${all.length} · 매크로 실행 ${running} · 기록 ${all.filter(x => x.rec && x.rec.state === 'recording').length}`;
  $('#bcCur').setAttribute('aria-pressed', String(!BC.on)); $('#bcAll').setAttribute('aria-pressed', String(BC.on));
  const bb = $('#bcBadge'); bb.classList.toggle('on', BC.on);
  bb.innerHTML = BC.on ? `<b>ALL →</b> <span data-role="bc-recipients">${BC.ids.map(id => esc(sess(T[id]?.sid)?.alias || '?')).join(', ')}</span>${BC.excluded.length ? ` <span class="bc-ex" data-role="bc-excluded">제외: ${BC.excluded.map(x => `${esc(x.alias)}(${esc(x.why)})`).join(', ')}</span>` : ''}
      <button type="button" class="btn sm" data-cmd="bc-all" title="지금 연결된 세션으로 수신자 다시 고르기">↻ 다시 고르기</button>${running ? `<button type="button" class="btn-stop" data-cmd="stop-all" title="모든 대상의 남은 step 취소">${ic('stop')}전체 중지</button>` : ''}`
    : `<span class="bc-cur" data-role="bc-current">${s ? `→ ${esc(s.alias)} 만` : '→ 활성 pane 없음'}</span>${running ? `<button type="button" class="btn-stop" data-cmd="stop-all" title="모든 실행 중 매크로의 남은 step 취소">${ic('stop')}전체 중지</button>` : ''}`;
}

/* ---- sidebar ---- */
const side = $('#side');
/* macro execution scope control (Active / All connected / Selected) with live recipient preview */
function msScopeHTML() {
  const open = Object.values(T);
  const modeBtn = k => `<button type="button" role="radio" aria-checked="${MS.mode === k}" aria-pressed="${MS.mode === k}" data-act="ms-mode" data-mode="${k}" data-fkey="msmode:${k}">${MS_LABEL[k]}</button>`;
  const pick = MS.mode !== 'selected' ? '' : `<div class="mscope-pick" role="group" aria-label="매크로를 보낼 세션 선택">${open.length ? open.map(t => {
      const s = sess(t.sid); const why = bcWhy(t); const on = MS.sel.includes(t.sid);
      return `<label class="chk${why ? ' dis' : ''}"><input type="checkbox" data-act="ms-sel" value="${t.sid}" data-fkey="mssel:${t.sid}" ${on ? 'checked' : ''} ${why ? 'disabled' : ''}> <b>${esc(s.alias)}</b> <small>${esc(s.port)}${why ? ` · ${esc(why)} — 제외` : ''}</small></label>`;
    }).join('') : '<span class="hint">열린 세션 없음</span>'}${MS.sel.filter(sid => !tabOfSession(sid)).map(sid => `<span class="hint">${esc(sess(sid)?.alias || '?')} · 열려 있지 않음 — 제외</span>`).join('')}</div>`;
  return `<div class="mscope" role="group" aria-label="매크로 실행 대상">
    <div class="mscope-row"><span class="mscope-l">실행 대상</span><span class="seg" role="radiogroup" aria-label="매크로 실행 대상">${['active', 'all', 'selected'].map(modeBtn).join('')}</span></div>
    <div class="mscope-to${MS.mode !== 'active' ? ' multi' : ''}" data-role="ms-recipients" aria-live="polite">${esc(macroTargetLabel({ fixed: [] }))}</div>
    ${pick}
    <div class="mscope-note">▶·단축키 매크로에만 적용 — 키 입력 대상(툴바)과 별개. 실행할 때 수신자를 고정하므로 새로 연 세션은 실행 중에 끼지 않습니다. “고정” 매크로는 이 설정 대신 자기 대상으로 갑니다.</div></div>`;
}
function renderSide() {
  const a = document.activeElement; const keepFilter = a && a.id === 'sessFilter' ? [a.selectionStart, a.selectionEnd] : null;
  const close = `<button type="button" class="ibtn" data-act="collapse-side" title="패널 접기 (Alt+0)" aria-label="패널 접기">${ic('x')}</button>`;
  if (S.ui.side === 'macros') {
    side.innerHTML = `<div class="side-head"><h2>매크로</h2><span class="cnt">${S.macros.length}</span>
        <button type="button" class="ibtn" data-act="new-macro" title="새 매크로" aria-label="새 매크로">${ic('plus')}</button>${close}</div>
      ${msScopeHTML()}
      <div class="side-body" role="list">${S.macros.map(m => {
        const runs = Object.values(T).filter(t => t.run && t.run.status === 'running' && t.run.macroId === m.id);
        const tgt = macroTargetLabel(m);
        return `<div class="mac${runs.length ? ' running' : ''}" role="listitem" data-mid="${m.id}">
          ${runs.length ? `<button type="button" class="btn-stop mac-stop" data-act="stop-macro-all" data-macro="${m.id}" title="이 매크로의 실행 대상 ${runs.length}곳 모두 중지 (남은 step 송신 안 함)" aria-label="${esc(m.name)} 전체 중지">${ic('stop')}전체 중지</button>`
            : `<button type="button" class="ibtn mac-run" data-act="run-macro" data-macro="${m.id}" title="바로 실행 → ${esc(tgt)}" aria-label="${esc(m.name)} 바로 실행 (${esc(tgt)})">${ic('play')}</button>`}
          <span class="mac-name">${esc(m.name)}${m.hotkey ? `<span class="chip kbd">${esc(m.hotkey)}</span>` : ''}${m.fixed.length ? `<span class="chip fixed${m.fixed.some(sid => !sess(sid)) ? ' unresolved' : ''}" title="고정 대상: 패널 실행 대상(${MS_LABEL[MS.mode]}) 대신 항상 ${esc(m.fixed.map(sid => sess(sid)?.alias || `미해결 ${sid}`).join(', '))} — ⋯ 편집에서 변경">${m.fixed.some(sid => !sess(sid)) ? '고정 · 미해결' : '고정'}</span>` : ''}</span>
          <button type="button" class="ibtn mac-more" data-act="mac-more" data-macro="${m.id}" title="편집 · 복제 · 삭제" aria-label="${esc(m.name)} 메뉴 (편집·복제·삭제)">${ic('more')}</button>
          <span class="mac-sub">${esc(m.steps.map(st => stepText(st)).join(' → '))}</span>
          <span class="mac-sub" data-role="mac-target">대상: ${esc(tgt)}${m.vars.length ? ` · 변수 ${m.vars.map(v => v.name).join(', ')}` : ''}${m.pinned ? ' · ★' : ''}</span>
        </div>`;
      }).join('') || '<p class="side-note">매크로가 없습니다. “새 매크로”로 만드세요.</p>'}</div>
      <div class="side-foot"><button type="button" class="btn sm" data-act="new-macro">${ic('plus')}새 매크로</button><button type="button" class="btn sm" data-act="import">${ic('up')}가져오기</button><button type="button" class="btn sm" data-act="export">${ic('down')}내보내기</button></div>
      <div class="side-note">▶는 누르면 바로 실행합니다 — 대상은 위 “실행 대상”(활성 / 연결된 모두 / 선택)이며, “고정” 표시 매크로만 자기 대상으로 갑니다. 편집·복제·삭제는 ⋯. 실행 중 ■ 전체 중지는 이 매크로의 모든 대상을, pane의 중지는 그 대상만 멈춥니다. 단축키는 터미널 입력 중에도 동작합니다.</div>`;
  } else {
    const f = (S.ui.filter || '').toLowerCase();
    const groups = [...new Set(S.sessions.map(s => s.group || '기타'))];
    side.innerHTML = `<div class="side-head"><h2>저장된 UART 세션</h2><span class="cnt">${S.sessions.length}</span>
        <button type="button" class="ibtn" data-act="new-session" title="새 세션" aria-label="새 세션">${ic('plus')}</button>${close}</div>
      <div class="side-tools"><input type="search" class="field-sm" id="sessFilter" placeholder="별칭, 포트, baud 검색" value="${esc(S.ui.filter || '')}" aria-label="세션 검색"></div>
      <div class="side-body" role="tree">${groups.map(g => {
        const items = S.sessions.filter(s => (s.group || '기타') === g && (!f || `${s.alias} ${s.port} ${s.baud} ${s.group}`.toLowerCase().includes(f)));
        if (!items.length) return '';
        const open = !S.ui.closedGroups.includes(g);
        return `<button type="button" class="group-h" data-act="toggle-group" data-group="${esc(g)}" aria-expanded="${open}">${ic('chev')}${esc(g)} <span style="font-weight:400">(${items.length})</span></button>
          ${open ? items.map(s => {
            const t = tabOfSession(s.id); const L = t && leafOfTab(t.id);
            const isAct = t && activeTab() === t;
            return `<div class="sess${isAct ? ' is-active' : ''}" role="treeitem" tabindex="0" data-sid="${s.id}" style="--tag:${s.color}"
                title="더블클릭/Enter: 활성 pane에 열기 · 끌어서 pane 가장자리에 놓으면 분할">
              <span class="dot"></span>
              <span class="sess-name">${esc(s.alias)}${t && L ? `<span class="open-tag" title="${connLabel(t.conn)}">pane ${paneNo(L.id)}</span>` : ''}</span>
              <span class="sess-acts">
                <button type="button" class="ibtn" data-act="open-here" data-sid="${s.id}" title="활성 pane에 탭으로 열기" aria-label="${esc(s.alias)} 활성 pane에 열기">${ic('play')}</button>
                <button type="button" class="ibtn" data-act="open-right" data-sid="${s.id}" title="활성 pane 오른쪽 새 분할에 열기" aria-label="${esc(s.alias)} 오른쪽 분할에 열기">${ic('split-r')}</button>
                <button type="button" class="ibtn" data-act="open-down" data-sid="${s.id}" title="활성 pane 아래 새 분할에 열기" aria-label="${esc(s.alias)} 아래 분할에 열기">${ic('split-d')}</button>
                <button type="button" class="ibtn" data-act="edit-session" data-sid="${s.id}" title="세션 설정" aria-label="${esc(s.alias)} 설정">${ic('gear')}</button>
              </span>
              <span class="sess-meta">${esc(s.port)} · ${frame(s)} · ${esc(PROFILES[s.profile]?.label.split(' (')[0] || s.profile)}</span>
            </div>`;
          }).join('') : ''}`;
      }).join('')}</div>
      <div class="side-foot"><button type="button" class="btn sm" data-act="new-session">${ic('plus')}새 세션</button></div>
      <div class="side-note">포트는 한 탭만 점유합니다. 이미 열린 세션을 다른 pane으로 끌면 그 탭이 옮겨집니다.</div>`;
    if (keepFilter) { const fi = $('#sessFilter'); fi.focus(); fi.setSelectionRange(...keepFilter); }
  }
}

/* ============================== pointer: divider resize ============================== */
function startResize(e, dv) {
  if (e.button !== 0) return;
  const node = findNode(dv.dataset.split); if (!node) return;
  e.preventDefault();
  const sp = dv.parentElement; const rect = sp.getBoundingClientRect();
  const row = node.dir === 'row';
  const total = (row ? rect.width : rect.height) - DIV;
  const minA = +dv.dataset.mina, minB = +dv.dataset.minb;
  const [ca, cb] = [dv.previousElementSibling, dv.nextElementSibling];
  resizing = { node, dv, pid: e.pointerId };
  dv.setPointerCapture(e.pointerId);
  dv.classList.add('dragging'); document.body.classList.add('resizing', row ? 'col-resize' : 'row-resize');
  const move = ev => {
    const pos = (row ? ev.clientX - rect.left : ev.clientY - rect.top) - DIV / 2;
    const a = total >= minA + minB ? clamp(pos, minA, total - minB) : clamp(pos, 0, total);
    node.r = total > 0 ? a / total : 0.5;
    ca.style.flex = `${node.r} 1 0px`; cb.style.flex = `${1 - node.r} 1 0px`;
    dv.setAttribute('aria-valuenow', String(Math.round(node.r * 100)));
  };
  const up = () => {
    dv.removeEventListener('pointermove', move); dv.removeEventListener('pointerup', up); dv.removeEventListener('pointercancel', up);
    dv.classList.remove('dragging'); document.body.classList.remove('resizing', 'col-resize', 'row-resize');
    resizing = null; persist.all();
    if (deferred) { deferred = false; renderAll(); }
  };
  dv.addEventListener('pointermove', move); dv.addEventListener('pointerup', up); dv.addEventListener('pointercancel', up);
}
function nudgeDivider(dv, delta, abs) {
  const node = findNode(dv.dataset.split); if (!node) return;
  const sp = dv.parentElement.getBoundingClientRect(); const total = (node.dir === 'row' ? sp.width : sp.height) - DIV;
  const lo = total > 0 ? +dv.dataset.mina / total : 0, hi = total > 0 ? 1 - (+dv.dataset.minb / total) : 1;
  node.r = clamp(abs != null ? abs : node.r + delta, Math.min(lo, 0.5), Math.max(hi, 0.5));
  dv.previousElementSibling.style.flex = `${node.r} 1 0px`; dv.nextElementSibling.style.flex = `${1 - node.r} 1 0px`;
  dv.setAttribute('aria-valuenow', String(Math.round(node.r * 100)));
  persist.all();
}

/* ============================== pointer: tab / session drag ============================== */
const ghostEl = () => drag.ghost;
const dropOv = $('#dropOv'), dropLabel = $('#dropLabel'), insertMark = $('#insertMark');
let suppressClick = false;
function startDrag(e, payload, el) {
  if (e.button !== 0 || e.target.closest('button')) return;
  drag = { payload, el, x0: e.clientX, y0: e.clientY, active: false, pid: e.pointerId, target: null };
  try { el.setPointerCapture(e.pointerId); } catch { /* pointer already gone */ }
  el.addEventListener('pointermove', onDragMove);
  el.addEventListener('pointerup', onDragUp);
  el.addEventListener('pointercancel', onDragCancel);
}
function onDragMove(e) {
  if (!drag) return;
  if (!drag.active) {
    if (Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) < 6) return;
    drag.active = true;
    const s = sess(drag.payload.kind === 'tab' ? T[drag.payload.tid]?.sid : drag.payload.sid);
    const g = document.createElement('div'); g.className = 'drag-ghost'; g.style.setProperty('--tag', s?.color || 'var(--accent)');
    g.innerHTML = `<span class="dot"></span>${esc(s?.alias || '')} <span style="font:11px var(--mono);color:var(--muted)">${esc(s?.port || '')}</span>`;
    document.body.appendChild(g); drag.ghost = g;
    drag.el.classList.add('drag-src'); document.body.classList.add('dragging');
  }
  const g = ghostEl(); g.style.left = e.clientX + 'px'; g.style.top = e.clientY + 'px';
  drag.target = hitTest(e.clientX, e.clientY);
  showDropTarget(drag.target);
}
function hitTest(x, y) {
  const el = document.elementFromPoint(x, y); if (!el) return null;
  const strip = el.closest('.tabstrip');
  if (strip) {
    const lid = strip.dataset.leaf; const tabs = $$('.tab', strip);
    let idx = tabs.length; for (let i = 0; i < tabs.length; i++) { const r = tabs[i].getBoundingClientRect(); if (x < r.left + r.width / 2) { idx = i; break; } }
    const ref = tabs[idx] ? tabs[idx].getBoundingClientRect() : (tabs.length ? tabs[tabs.length - 1].getBoundingClientRect() : $('.tabs', strip).getBoundingClientRect());
    const mx = tabs[idx] ? ref.left : (tabs.length ? ref.right : ref.left + 2);
    return { kind: 'tabs', leaf: lid, index: idx, mark: { x: mx, top: ref.top + 3, h: ref.height - 6 } };
  }
  const pane = el.closest('.pane');
  if (pane) {
    const pr = pane.getBoundingClientRect(); const ts = $('.tabstrip', pane).getBoundingClientRect();
    const r = { left: pr.left, top: ts.bottom, width: pr.width, height: pr.bottom - ts.bottom };
    const fx = (x - r.left) / r.width, fy = (y - r.top) / r.height;
    const d = { left: fx, right: 1 - fx, top: fy, bottom: 1 - fy };
    let zone = 'center', m = 0.25;
    for (const k of Object.keys(d)) if (d[k] < m) { m = d[k]; zone = k; }
    const box = { ...r };
    if (zone === 'left') box.width = r.width / 2; else if (zone === 'right') { box.left = r.left + r.width / 2; box.width = r.width / 2; }
    else if (zone === 'top') box.height = r.height / 2; else if (zone === 'bottom') { box.top = r.top + r.height / 2; box.height = r.height / 2; }
    return { kind: 'zone', leaf: pane.dataset.leaf, zone, box };
  }
  return null;
}
const ZONE_TXT = { center: '이 pane에 탭으로', left: '왼쪽에 분할', right: '오른쪽에 분할', top: '위에 분할', bottom: '아래에 분할' };
function showDropTarget(tg) {
  dropOv.classList.remove('show'); insertMark.style.display = 'none';
  if (!tg) return;
  if (tg.kind === 'tabs') { Object.assign(insertMark.style, { display: 'block', left: tg.mark.x - 1 + 'px', top: tg.mark.top + 'px', height: tg.mark.h + 'px' }); return; }
  Object.assign(dropOv.style, { left: tg.box.left + 'px', top: tg.box.top + 'px', width: tg.box.width + 'px', height: tg.box.height + 'px' });
  dropLabel.textContent = `pane ${paneNo(tg.leaf)}: ${ZONE_TXT[tg.zone]}`;
  dropOv.classList.add('show');
}
function endDrag() {
  if (!drag) return;
  const el = drag.el;
  el.removeEventListener('pointermove', onDragMove); el.removeEventListener('pointerup', onDragUp); el.removeEventListener('pointercancel', onDragCancel);
  el.classList.remove('drag-src'); document.body.classList.remove('dragging');
  drag.ghost && drag.ghost.remove();
  dropOv.classList.remove('show'); insertMark.style.display = 'none';
  const was = drag; drag = null; return was;
}
function onDragUp(e) {
  const d = endDrag(); if (!d) return;
  if (!d.active) return; // a click: handled by click event
  suppressClick = true; setTimeout(() => { suppressClick = false; }, 0);
  const tg = hitTest(e.clientX, e.clientY);
  deferred = false;
  if (!tg) { renderAll(); return; }
  const where = tg.kind === 'tabs' ? 'tab' : tg.zone === 'center' ? 'tab' : tg.zone;
  const index = tg.kind === 'tabs' ? tg.index : null;
  if (d.payload.kind === 'tab') placeTab(d.payload.tid, tg.leaf, where, index);
  else placeSession(d.payload.sid, tg.leaf, where, index);
}
function onDragCancel() { const d = endDrag(); if (d && d.active && deferred) { deferred = false; renderAll(); } }

/* ============================== keyboard pane navigation ============================== */
function neighbor(lid, dir) {
  const a = paneEl(lid)?.getBoundingClientRect(); if (!a) return null;
  let best = null, bd = Infinity;
  for (const L of leaves()) {
    if (L.id === lid) continue; const r = paneEl(L.id)?.getBoundingClientRect(); if (!r || !r.width) continue;
    let gap, ov;
    if (dir === 'left') { gap = a.left - r.right; ov = Math.min(a.bottom, r.bottom) - Math.max(a.top, r.top); }
    else if (dir === 'right') { gap = r.left - a.right; ov = Math.min(a.bottom, r.bottom) - Math.max(a.top, r.top); }
    else if (dir === 'up') { gap = a.top - r.bottom; ov = Math.min(a.right, r.right) - Math.max(a.left, r.left); }
    else { gap = r.top - a.bottom; ov = Math.min(a.right, r.right) - Math.max(a.left, r.left); }
    if (gap < -4 || ov <= 0) continue;
    const dd = gap * 1000 - ov; if (dd < bd) { bd = dd; best = L; }
  }
  return best;
}
const DIRZONE = { left: 'left', right: 'right', up: 'top', down: 'bottom' };
function moveActiveTab(dir) {
  const L = activeLeaf(); if (!L || !L.on) return;
  const n = neighbor(L.id, dir);
  if (n) placeTab(L.on, n.id, 'tab');
  else if (L.tabs.length > 1) placeTab(L.on, L.id, DIRZONE[dir]);
  else toast('그 방향에 pane이 없습니다. 탭이 2개 이상이면 새 분할로 옮깁니다.');
}

/* ============================== macros: run engine ============================== */
function subst(text, vars) { return String(text).replace(/\$\{(\w+)\}/g, (m, k) => (k in vars ? vars[k] : m)); }
/* Recipient resolution — ONE rule for every entry point (sidebar ▶, pane macro menu, hotkey):
   1) macro has a fixed override (m.fixed, labelled “고정”) -> those sessions;
   2) otherwise the macro panel scope MS: active tab at launch | all connected writable | selected sessions.
   A pane-menu launch only makes that pane active (like any click on it); it never narrows All/Selected.
   Keyboard input target (toolbar BC) never affects macros. The set is resolved once per execution (snapshot). */
const MS_LABEL = { active: '활성', all: '연결된 모두', selected: '선택' };
function macroRecipients(m) {
  let source, cand;
  if (m.fixed && m.fixed.length) { source = 'fixed'; cand = m.fixed.map(sid => ({ sid, t: tabOfSession(sid) })); }
  else if (MS.mode === 'all') { source = 'all'; cand = Object.values(T).map(t => ({ sid: t.sid, t })); }
  else if (MS.mode === 'selected') { source = 'selected'; cand = MS.sel.map(sid => ({ sid, t: tabOfSession(sid) })); }
  else { source = 'active'; const t = activeTab(); cand = t ? [{ sid: t.sid, t }] : []; }
  const ids = [], skipped = [];
  for (const c of cand) {
    const known = !!sess(c.sid);
    const al = known ? sess(c.sid).alias : `미해결 ${c.sid}`;
    const why = !known ? '저장된 세션 없음' : !c.t ? '열려 있지 않음' : bcWhy(c.t) || (c.t.run && c.t.run.status === 'running' ? '다른 매크로 실행 중' : '');
    if (why) skipped.push({ alias: al, why }); else ids.push(c.t.id);
  }
  return { source, ids, skipped, aliases: ids.map(id => sess(T[id].sid).alias) };
}
function macroTargetLabel(m) {
  const R = macroRecipients(m);
  const head = R.source === 'fixed' ? (m.fixed.some(sid => !sess(sid)) ? '고정 · 실행 차단(미해결)' : '고정') : MS_LABEL[R.source];
  return `${head} → ${R.ids.length ? `${R.aliases.join(', ')}${R.ids.length > 1 ? ` (${R.ids.length})` : ''}` : '없음'}${R.skipped.length ? ` · 제외 ${R.skipped.map(x => `${x.alias}(${x.why})`).join(', ')}` : ''}`;
}
/* ▶ / hotkey / menu: run immediately on the resolved recipients with saved/default vars. No confirm popup; never opens the editor. */
function requestRun(mid, hintTab) {
  const m = S.macros.find(x => x.id === mid); if (!m) return;
  const vars = {}; for (const v of m.vars) vars[v.name] = S.runVars[m.id]?.[v.name] ?? v.def;
  const missing = m.vars.filter(v => !String(vars[v.name] ?? '').length);
  if (missing.length) { openVarsDialog(m, hintTab, vars, missing); return; }
  if (hintTab && T[hintTab]) { const L = leafOfTab(hintTab); if (L) { L.on = hintTab; setActiveLeaf(L.id); } } // launched from a pane: that pane is the active one (matters only for 활성)
  const R = macroRecipients(m);
  const ghost = R.source === 'fixed' ? m.fixed.filter(sid => !sess(sid)) : [];
  if (ghost.length) { toast(`“${m.name}” 실행 차단: 고정 대상 ${ghost.length}개 미해결 (${ghost.join(', ')}) — 일부만 보내지 않습니다. ⋯ 편집에서 다시 지정하세요`); return; } // whole fixed macro blocked, nothing sent
  if (!R.ids.length) {
    const why = R.source === 'selected' && !MS.sel.length ? '선택된 세션 없음 — 매크로 패널의 “선택”에서 고르세요' : R.source === 'active' && !R.skipped.length ? '활성 pane에 열린 세션 없음' : R.skipped.map(x => `${x.alias}(${x.why})`).join(', ');
    toast(`“${m.name}” 실행 안 함: ${why}`);
    if (R.source === 'selected' && !MS.sel.length) { S.ui.side = 'macros'; S.ui.collapsed = false; renderSide(); renderChrome(); $('#side [data-act="ms-sel"]:not([disabled])')?.focus(); }
    return;
  }
  if (R.ids.length === 1 && R.source === 'active') { runMacro(m, R.ids[0], vars); if (R.skipped.length) toast(`“${m.name}” 제외: ${R.skipped.map(x => `${x.alias}(${x.why})`).join(', ')}`); return; }
  runGroup(m, vars, R);
}
function runGroup(m, vars, R) {
  const gid = nid('g'); const ids = R.ids.slice(); // recipient set locked for this execution
  GROUPS[gid] = { name: m.name, ids: ids.slice(), source: R.source };
  toast(`“${m.name}” → ${R.source === 'fixed' ? '고정' : MS_LABEL[R.source] || R.source} ${ids.length}곳: ${R.aliases.join(', ')}${R.skipped.length ? ' · 제외 ' + R.skipped.map(x => `${x.alias}(${x.why})`).join(', ') : ''}`);
  for (const id of ids) runMacro(m, id, vars, gid);
  renderChrome();
}
function stopAllRuns() { let n = 0; for (const t of Object.values(T)) if (t.run && t.run.status === 'running') { stopMacro(t.id); n++; } if (n) toast(`실행 중 매크로 ${n}개 중지 (남은 step 송신 안 함)`); }
function openVarsDialog(m, hintTab, vars, missing) {
  openDlg($('#dlgRun'), `
    <div class="dlg-h">${ic('macro')}<h2>변수 값 필요 — ${esc(m.name)}</h2></div>
    <div class="dlg-b"><p class="hint" style="margin:0">이 매크로는 다음 변수에 저장된 값이나 기본값이 없어 실행하지 않았습니다: <b>${missing.map(v => '${' + esc(v.name) + '}').join(', ')}</b>. 값을 넣으면 저장하고 바로 실행합니다 (매크로 편집기는 열지 않음).</p>
      <div class="grid2">${m.vars.map(v => `<label class="fld"><span>\${${esc(v.name)}}${missing.includes(v) ? ' · 필요' : ''}</span><input name="var:${esc(v.name)}" value="${esc(vars[v.name] ?? '')}" class="mono" data-missing="${missing.includes(v)}"></label>`).join('')}</div>
      <div class="err-text" data-role="err"></div></div>
    <div class="dlg-f"><span class="grow"></span><button type="button" class="btn" data-close>취소</button><button type="submit" class="btn primary" data-role="go">${ic('play')}저장 후 실행</button></div>`,
  form => {
    const fd = new FormData(form); const vv = {}; for (const v of m.vars) vv[v.name] = String(fd.get('var:' + v.name) ?? '');
    const still = m.vars.filter(v => !vv[v.name].length); if (still.length) { $('[data-role="err"]', form).textContent = `아직 비어 있음: ${still.map(v => v.name).join(', ')}`; return false; }
    S.runVars[m.id] = vv; persist.all(); setTimeout(() => requestRun(m.id, hintTab), 0);
  },
  form => setTimeout(() => form.querySelector('input[data-missing="true"]')?.focus(), 0));
}
function sleepR(run, ms) {
  return new Promise((ok, fail) => { const h = setTimeout(() => { run.abort = null; ok(); }, ms); run.abort = () => { clearTimeout(h); run.abort = null; fail(new Error('중지됨')); }; });
}
function waitR(run, tab, text, ms) {
  return new Promise((ok, fail) => {
    const w = { text, pos: tab.rxBase + tab.rxAll.length, ok: () => { clearTimeout(h); run.abort = null; ok(); }, fail: err => { clearTimeout(h); run.abort = null; fail(err); } };
    const h = setTimeout(() => { const i = tab.waiters.indexOf(w); if (i >= 0) tab.waiters.splice(i, 1); run.abort = null; fail(new Error(`시간 초과 ${ms}ms`)); }, ms);
    tab.waiters.push(w);
    run.abort = () => { const i = tab.waiters.indexOf(w); if (i >= 0) tab.waiters.splice(i, 1); w.fail(new Error('중지됨')); };
  });
}
async function runMacro(m, tid, vars, gid = null) {
  const tab = T[tid]; if (!tab) return;
  if (tab.run && tab.run.status === 'running') { toast(`${sess(tab.sid).alias}에서 이미 매크로가 실행 중입니다`); return; }
  const run = { id: nid('r'), gid, macroId: m.id, name: m.name, vars, steps: m.steps.map(st => ({ ...st, st: 'pending', note: '' })), i: 0, status: 'running', stopped: false, abort: null, err: '', t0: Date.now() };
  tab.run = run;
  S.runVars[m.id] = { ...vars }; persist.all();
  const s = sess(tab.sid);
  sysLine(tab, `매크로 ▶ ${m.name} 시작 → ${s.alias} (${s.port})`);
  const upd = () => updateRunStrip(tid);
  structural(leafOfTab(tid)?.id); renderSide();
  for (let i = 0; i < run.steps.length; i++) {
    if (run.stopped || !T[tid]) break;
    const st = run.steps[i]; run.i = i; st.st = 'run'; upd();
    try {
      if (st.type === 'send') {
        if (!sendText(tid, subst(st.text, vars), st.eol || 'session', 'macro:' + m.id)) throw new Error('포트가 연결되어 있지 않음');
      } else if (st.type === 'delay') await sleepR(run, Math.max(0, +st.ms || 0));
      else await waitR(run, tab, subst(st.text, vars), Math.max(50, +st.ms || 1000));
      if (run.stopped) { st.st = 'skip'; st.note = '중지'; break; }
      st.st = 'ok';
    } catch (err) {
      st.st = 'fail'; st.note = err.message;
      if (!run.stopped) { run.status = 'failed'; run.err = `step ${i + 1}: ${err.message}`; }
      break;
    }
  }
  if (!T[tid]) return;
  if (run.stopped) run.status = 'stopped';
  else if (run.status === 'running') run.status = 'done';
  for (const st of run.steps) if (st.st === 'pending') st.st = 'skip';
  sysLine(tab, `매크로 ■ ${m.name} ${{ done: '완료', failed: '실패 — ' + run.err, stopped: `${run.reason || '사용자 중지'} (step ${run.i + 1}), 이후 step 송신 안 함` }[run.status]} · ${((Date.now() - run.t0) / 1000).toFixed(1)}s`);
  structural(leafOfTab(tid)?.id); renderSide();
  if (run.status === 'done') setTimeout(() => { if (T[tid] && T[tid].run === run) { T[tid].run = null; structural(leafOfTab(tid)?.id); } }, 6000);
}
function stopMacro(tid) {
  const t = T[tid]; const r = t && t.run; if (!r || r.status !== 'running') return;
  r.stopped = true; r.abort && r.abort(); // cancels only unsent steps/delay/wait; bytes already sent stay queued at the device
  updateRunStrip(tid);
}
function updateRunStrip(tid) {
  const t = T[tid]; const L = leafOfTab(tid); if (!t || !L || L.on !== tid) return;
  const pe = paneEl(L.id); const el = pe && $(`.runstrip[data-tab="${tid}"]`, pe);
  if (el && t.run) el.outerHTML = runStripHTML(t); else if (pe) renderPane(L.id);
}

/* ============================== dialogs ============================== */
function openDlg(dlg, html, onSubmit, onReady) {
  dlg.innerHTML = `<form method="dialog" novalidate>${html}</form>`;
  const form = $('form', dlg);
  form.addEventListener('submit', e => { e.preventDefault(); if (onSubmit && onSubmit(form, e.submitter) === false) return; dlg.close(); });
  dlg.addEventListener('click', e => { if (e.target.closest('[data-close]')) { e.preventDefault(); dlg.close(); } });
  onReady && onReady(form);
  dlg.showModal();
}
const anyDialogOpen = () => $$('dialog').some(d => d.open);

/* ---- session settings ---- */
function openSessionDialog(sid, isNew = false) {
  const freeP = uartPortsSim().ports.find(p => { const a = portAnno(p.path); return !a.openBy && !a.savedAs.length; }) || uartPortsSim().ports.find(p => !portAnno(p.path).openBy);
  const base = isNew ? { id: nid('s-'), alias: `uart${S.sessions.length + 1}`, group: 'ECU bench A', port: freeP ? freeP.path : '', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', echo: false, ts: true, autoReconnect: true, profile: 'mcu', color: COLORS[S.sessions.length % COLORS.length] } : sess(sid);
  if (!base) return;
  const s = { ...base }; const open = !isNew && tabOfSession(s.id);
  const opt = (arr, v) => arr.map(x => `<option value="${esc(Array.isArray(x) ? x[0] : x)}" ${String(Array.isArray(x) ? x[0] : x) === String(v) ? 'selected' : ''}>${esc(Array.isArray(x) ? x[1] : x)}</option>`).join('');
  openDlg($('#dlgSession'), `
    <div class="dlg-h"><span class="ph-dot" style="--tag:${s.color}"></span><h2>${isNew ? '새 UART 세션' : `세션 설정 — ${esc(s.alias)}`}</h2>${open ? `<span class="ph-state ${open.conn}">${connLabel(open.conn)} · pane ${paneNo(leafOfTab(open.id).id)}</span>` : ''}</div>
    <div class="dlg-b">
      <div class="grid2">
        <label class="fld"><span>별칭 (탭·대상 표시명, 중복 불가)</span><input name="alias" value="${esc(s.alias)}" required maxlength="24" class="mono"></label>
        <label class="fld"><span>그룹</span><input name="group" value="${esc(s.group || '')}" list="dlGroups"><datalist id="dlGroups">${[...new Set(S.sessions.map(x => x.group))].map(g => `<option value="${esc(g)}">`).join('')}</datalist></label>
        <div class="fld" style="grid-column:1/-1"><span>포트 — 감지된 UART 포트 (시뮬레이션 레지스트리, uart.ports 형식)</span>
          <div style="display:flex;gap:6px"><select name="portSel" class="mono" data-role="portsel" style="flex:1;min-width:0" aria-label="감지된 포트"></select><button type="button" class="btn" data-act="rescan" title="포트 다시 검색">${ic('reset')}새로고침</button></div>
          <input name="portManual" class="inp mono" data-role="portman" placeholder="수동 경로 (감지되지 않는 포트만, 예: /dev/ttyS1)" value="" hidden aria-label="수동 포트 경로">
          <div class="hint" data-role="portinfo" aria-live="polite"></div></div>
        <label class="fld"><span>시뮬레이션 장치 (실장비 없음)</span><select name="profile">${opt(Object.entries(PROFILES).map(([k, v]) => [k, `${v.label} · ${v.devBaud}`]), s.profile)}</select></label>
      </div>
      <fieldset><legend>직렬 프레임</legend><div class="grid4">
        <label class="fld"><span>Baud</span><select name="baud" class="mono">${opt(BAUDS, s.baud)}</select></label>
        <label class="fld"><span>Data bits</span><select name="dataBits" class="mono">${opt([5, 6, 7, 8], s.dataBits)}</select></label>
        <label class="fld"><span>Parity</span><select name="parity" class="mono">${opt([['N', 'None'], ['E', 'Even'], ['O', 'Odd'], ['M', 'Mark'], ['S', 'Space']], s.parity)}</select></label>
        <label class="fld"><span>Stop bits</span><select name="stopBits" class="mono">${opt([1, 1.5, 2], s.stopBits)}</select></label>
        <label class="fld"><span>Flow control</span><select name="flow" class="mono">${opt(['none', 'RTS/CTS', 'XON/XOFF'], s.flow)}</select></label>
        <label class="fld"><span>전송 줄 끝</span><select name="eol" class="mono">${opt(Object.keys(EOLS), s.eol)}</select></label>
        <label class="fld"><span>태그 색</span><select name="color">${opt(COLORS.map(c => [c, c]), s.color)}</select></label>
      </div><p class="hint" style="margin:8px 0 0">장치 baud와 다르게 설정하면 수신이 깨진 문자로 보이고 명령이 통하지 않습니다 (시뮬레이션).</p></fieldset>
      <div class="chk-row">
        <label class="chk"><input type="checkbox" name="echo" ${s.echo ? 'checked' : ''}> 로컬 에코 (장치가 에코하지 않을 때만)</label>
        <label class="chk"><input type="checkbox" name="ts" ${s.ts ? 'checked' : ''}> 타임스탬프</label>
        <label class="chk"><input type="checkbox" name="autoReconnect" ${s.autoReconnect ? 'checked' : ''}> 장치 분리 시 자동 재연결</label>
      </div>
      <div class="err-text" data-role="err"></div>
    </div>
    <div class="dlg-f">
      ${isNew ? '' : `<button type="button" class="btn danger" data-act="dlg-del-session">${ic('trash')}삭제</button><button type="button" class="btn" data-act="dlg-dup-session">${ic('copy')}복제</button>`}
      <span class="grow"></span>
      <button type="button" class="btn" data-close>취소</button>
      ${isNew ? `<button type="submit" class="btn" value="save">저장</button><button type="submit" class="btn primary" value="open">저장 후 활성 pane에 열기</button>` : `<button type="submit" class="btn primary" value="save">${open ? '적용 (열린 탭에 즉시)' : '저장'}</button>`}
    </div>`,
  (form, submitter) => {
    const fd = new FormData(form); const err = $('[data-role="err"]', form);
    const alias = String(fd.get('alias')).trim();
    if (!/^[\w.-]{1,24}$/.test(alias)) { err.textContent = '별칭은 영문/숫자/._- 1–24자'; return false; }
    if (S.sessions.some(x => x.alias.toLowerCase() === alias.toLowerCase() && x.id !== s.id)) { err.textContent = `별칭 “${alias}”은 이미 있습니다 — 대상 혼동 방지를 위해 고유해야 합니다`; return false; }
    const sel = String(fd.get('portSel') || '');
    const port = (sel === '__manual' ? String(fd.get('portManual') || '') : sel).trim(); if (!port) { err.textContent = '포트를 고르세요 (감지된 포트가 없으면 새로고침 또는 수동 입력)'; return false; }
    const busy = portAnno(port, s.id).openBy;
    const wantsOpen = (submitter && submitter.value === 'open') || (!isNew && tabOfSession(s.id) && port !== s.port);
    if (busy && busy.sid !== s.id && wantsOpen) { err.textContent = `${port}는 ${busy.alias} (pane ${busy.pane})가 사용 중입니다 — 같은 물리 포트는 한 탭만 열 수 있습니다. 위 버튼으로 그 세션으로 이동하세요.`; return false; }
    const clash = S.sessions.find(x => x.port === port && x.id !== s.id);
    const next = { ...s, alias, group: String(fd.get('group')).trim() || '기타', port, baud: +fd.get('baud'), dataBits: +fd.get('dataBits'), parity: fd.get('parity'), stopBits: +fd.get('stopBits'), flow: fd.get('flow'), eol: fd.get('eol'), color: fd.get('color'), profile: fd.get('profile'), echo: fd.has('echo'), ts: fd.has('ts'), autoReconnect: fd.has('autoReconnect') };
    const before = isNew ? null : sess(s.id);
    if (isNew) S.sessions.push(next); else Object.assign(before, next);
    if (clash) toast(`주의: ${port}는 ${clash.alias}와 같은 포트입니다. 동시에 열 수 없습니다.`);
    const t = tabOfSession(next.id);
    if (t && before) {
      sysLine(t, `설정 적용: ${next.port} @ ${frame(next)} · flow ${next.flow} · EOL ${next.eol} · echo ${next.echo ? 'on' : 'off'}`);
      if (t.sim && t.sim.profile !== next.profile) { t.sim.stop(); t.sim = new Sim(t, next.profile); if (t.conn === 'connected') t.sim.start(true); }
    }
    persist.all();
    if (submitter && submitter.value === 'open') placeSession(next.id, activeLeaf().id, 'tab'); else renderAll();
    toast(isNew ? `세션 ${alias} 저장됨` : `세션 ${alias} 설정 저장됨`);
  },
  form => {
    const selEl = $('[data-role="portsel"]', form), man = $('[data-role="portman"]', form), info = $('[data-role="portinfo"]', form);
    const fill = (keep, announce) => {
      const ports = uartPortsSim().ports; const cur = keep ?? s.port;
      const o = ports.map(p => { const a = portAnno(p.path, s.id);
        const st = a.openBy ? (a.openBy.sid === s.id ? ` · 이 세션이 열고 있음 (pane ${a.openBy.pane})` : ` · 사용 중: ${a.openBy.alias} (pane ${a.openBy.pane})`) : a.savedAs.length ? ` · 저장됨: ${a.savedAs.map(x => x.alias).join(', ')}` : ' · 사용 가능';
        return `<option value="${esc(p.path)}" ${p.path === cur ? 'selected' : ''}>${esc(p.path)} — ${esc(p.manufacturer)} ${esc(p.product)} (${hex4(p.vid)}:${hex4(p.pid)})${esc(st)}</option>`; });
      if (!ports.length) o.push('<option value="" disabled selected>감지된 UART 포트 없음 — 장치 연결 후 새로고침</option>');
      if (cur && cur !== '__manual' && !ports.some(p => p.path === cur)) o.push(`<option value="${esc(cur)}" selected>${esc(cur)} — 현재 감지 안 됨 (저장된 값)</option>`);
      o.push(`<option value="__manual" ${cur === '__manual' ? 'selected' : ''}>수동 입력…</option>`);
      selEl.innerHTML = o.join(''); upd();
      if (announce) toast(`포트 ${ports.length}개 감지됨 (시뮬레이션)`);
    };
    const upd = () => {
      const v = selEl.value; man.hidden = v !== '__manual';
      if (v === '__manual') { info.innerHTML = '감지 목록에 없는 경로만 직접 입력하세요. 열 때 같은 포트를 쓰는 탭이 있으면 거절됩니다.'; return; }
      const p = PORT_REGISTRY.find(x => x.path === v); const a = portAnno(v, s.id);
      let h = p ? `${esc(p.manufacturer)} ${esc(p.product)} · VID:PID ${hex4(p.vid)}:${hex4(p.pid)}${p.serial ? ` · SN ${esc(p.serial)}` : ''} · USB ${esc(p.location || '-')}` : '';
      if (a.openBy && a.openBy.sid !== s.id) h += `<div style="margin-top:6px;display:flex;gap:6px;flex-wrap:wrap;align-items:center"><b style="color:var(--warn)">사용 중: ${esc(a.openBy.alias)} (pane ${a.openBy.pane}, ${connLabel(a.openBy.conn)})</b>
        <button type="button" class="btn sm" data-act="port-focus" data-tab="${a.openBy.tab}">그 탭으로 이동</button><button type="button" class="btn sm" data-act="port-move" data-tab="${a.openBy.tab}">활성 pane으로 옮기기</button></div>`;
      else if (a.savedAs.length) h += `<div style="margin-top:6px;display:flex;gap:6px;flex-wrap:wrap;align-items:center"><span>저장된 세션 ${a.savedAs.map(x => `<b>${esc(x.alias)}</b>`).join(', ')}이(가) 같은 포트를 씁니다 (동시에 열 수 없음)</span>${a.savedAs.map(x => `<button type="button" class="btn sm" data-act="port-open-saved" data-sid="${x.id}">${esc(x.alias)} 열기</button>`).join('')}</div>`;
      info.innerHTML = h;
    };
    selEl.addEventListener('change', upd);
    fill(s.port || null, false);
    form.addEventListener('click', e => {
      const b = e.target.closest('[data-act]'); if (!b) return;
      if (b.dataset.act === 'rescan') { fill(selEl.value, true); return; }
      if (b.dataset.act === 'port-focus') { $('#dlgSession').close(); focusTab(b.dataset.tab); return; }
      if (b.dataset.act === 'port-move') { $('#dlgSession').close(); placeTab(b.dataset.tab, activeLeaf().id, 'tab'); return; }
      if (b.dataset.act === 'port-open-saved') { $('#dlgSession').close(); placeSession(b.dataset.sid, activeLeaf().id, 'tab'); return; }
      if (b.dataset.act === 'dlg-dup-session') { $('#dlgSession').close(); duplicateSession(s.id); }
      if (b.dataset.act === 'dlg-del-session') { $('#dlgSession').close(); deleteSession(s.id); }
    });
  });
}
function duplicateSession(sid) {
  const s = sess(sid); let n = 2, alias = `${s.alias}-${n}`; while (S.sessions.some(x => x.alias === alias)) alias = `${s.alias}-${++n}`;
  const c = { ...s, id: nid('s-'), alias }; S.sessions.splice(S.sessions.indexOf(s) + 1, 0, c);
  persist.all(); renderAll(); openSessionDialog(c.id); toast(`${s.alias} 복제 → ${alias} (포트를 바꾸세요)`);
}
function deleteSession(sid) {
  const s = sess(sid);
  confirmDlg(`세션 “${s.alias}” 삭제`, `${s.port} · ${frame(s)}${tabOfSession(sid) ? '<br>열린 탭도 닫힙니다.' : ''}`, '삭제', () => {
    const t = tabOfSession(sid); if (t) closeTab(t.id);
    S.sessions = S.sessions.filter(x => x.id !== sid);
    const hit = S.macros.filter(m => m.fixed.includes(sid)).length; // kept as unresolved (visible, blocked) — never silently widened to the panel scope
    if (hit) toast(`고정 매크로 ${hit}개가 삭제된 ${s.alias}를 가리킵니다 — “미해결”로 표시되고 실행되지 않습니다 (⋯ 편집에서 다시 지정)`);
    MS.sel = MS.sel.filter(x => x !== sid);
    persist.all(); renderAll(); toast(`세션 ${s.alias} 삭제됨`);
  });
}
function confirmDlg(title, body, okLabel, onOk) {
  openDlg($('#dlgConfirm'), `<div class="dlg-h"><h2>${esc(title)}</h2></div><div class="dlg-b"><p style="margin:0">${body}</p></div>
    <div class="dlg-f"><button type="button" class="btn" data-close>취소</button><button type="submit" class="btn danger" autofocus>${esc(okLabel)}</button></div>`, () => { onOk(); });
}

/* ---- macro editor ---- */
function openMacroDialog(mid, isNew = false) {
  const src = isNew ? { id: nid('m-'), name: '', fixed: [], hotkey: '', pinned: true, vars: [], steps: [{ type: 'send', text: '', eol: 'session' }] } : S.macros.find(m => m.id === mid);
  if (!src) return;
  const D = structuredClone(src);
  const dlg = $('#dlgMacro');
  const stepsHTML = () => D.steps.map((st, i) => `<div class="step-row" data-i="${i}">
      <span class="n">${i + 1}</span>
      <select data-f="type" data-i="${i}" aria-label="step ${i + 1} 종류"><option value="send" ${st.type === 'send' ? 'selected' : ''}>send</option><option value="wait" ${st.type === 'wait' ? 'selected' : ''}>wait</option><option value="delay" ${st.type === 'delay' ? 'selected' : ''}>delay</option></select>
      ${st.type === 'delay' ? `<input class="val" data-f="ms" data-i="${i}" type="number" min="0" max="60000" step="50" value="${st.ms ?? 500}" aria-label="step ${i + 1} 지연 ms">`
        : `<input class="val" data-f="text" data-i="${i}" value="${esc(st.text ?? '')}" placeholder="${st.type === 'send' ? '보낼 텍스트 (비우면 줄끝만) · ${변수}' : '기다릴 수신 문자열'}" aria-label="step ${i + 1} ${st.type === 'send' ? '보낼 텍스트' : '기다릴 문자열'}">`}
      <span class="ms">${st.type === 'send' ? `<select data-f="eol" data-i="${i}" aria-label="step ${i + 1} 줄끝"><option value="session" ${!st.eol || st.eol === 'session' ? 'selected' : ''}>세션 EOL</option>${Object.keys(EOLS).map(k => `<option value="${k}" ${st.eol === k ? 'selected' : ''}>${k}</option>`).join('')}</select>`
        : st.type === 'wait' ? `<input data-f="ms" data-i="${i}" type="number" min="50" max="60000" step="100" value="${st.ms ?? 2000}" title="시간 초과 ms" aria-label="step ${i + 1} 시간 초과 ms">` : '<span class="hint">ms</span>'}</span>
      <span class="sacts"><button type="button" class="ibtn" data-sa="up" data-i="${i}" ${i === 0 ? 'disabled' : ''} aria-label="위로">↑</button><button type="button" class="ibtn" data-sa="down" data-i="${i}" ${i === D.steps.length - 1 ? 'disabled' : ''} aria-label="아래로">↓</button><button type="button" class="ibtn danger" data-sa="del" data-i="${i}" ${D.steps.length === 1 ? 'disabled' : ''} aria-label="step ${i + 1} 삭제">${ic('trash')}</button></span>
    </div>`).join('');
  const varsHTML = () => D.vars.map((v, i) => `<div class="var-row" data-vi="${i}"><input data-vf="name" data-vi="${i}" value="${esc(v.name)}" placeholder="이름 (예: user)" aria-label="변수 ${i + 1} 이름"><input data-vf="def" data-vi="${i}" value="${esc(v.def)}" placeholder="기본값" aria-label="변수 ${i + 1} 기본값"><button type="button" class="ibtn danger" data-va="del" data-vi="${i}" aria-label="변수 삭제">${ic('trash')}</button></div>`).join('');
  const previewTxt = () => { const vv = Object.fromEntries(D.vars.map(v => [v.name, v.def])); return D.steps.map((st, i) => `${i + 1}. ${stepText(st, vv)}`).join('\n'); };
  openDlg(dlg, `
    <div class="dlg-h">${ic('macro')}<h2>${isNew ? '새 매크로' : `매크로 수정 — ${esc(src.name)}`}</h2></div>
    <div class="dlg-b">
      <div class="grid2">
        <label class="fld"><span>이름</span><input name="name" value="${esc(D.name)}" placeholder="예: Bootloader enter" maxlength="40" autofocus></label>
        <div class="fld"><span>단축키 (F1–F12 또는 Ctrl/Alt+키)</span><div style="display:flex;gap:6px"><button type="button" class="btn hotkey-btn" data-act="cap-hotkey">${esc(D.hotkey || '없음 — 눌러서 지정')}</button><button type="button" class="btn" data-act="clr-hotkey" title="단축키 지우기">지우기</button></div></div>
        <div class="fld" style="grid-column:1/-1"><span>실행 대상</span>
          <div class="chk-row"><label class="chk"><input type="radio" name="tmode" value="panel" ${D.fixed.length ? '' : 'checked'}> 매크로 패널의 실행 대상 따름 (활성 / 연결된 모두 / 선택) — 기본</label>
            <label class="chk"><input type="radio" name="tmode" value="fixed" ${D.fixed.length ? 'checked' : ''}> 고정: 항상 아래 세션으로 — 패널 설정보다 우선, 목록에 “고정” 표시</label></div>
          <div class="chk-row" data-role="fixed-list" style="margin-top:6px">${S.sessions.map(s => `<label class="chk"><input type="checkbox" name="fixed" value="${s.id}" ${D.fixed.includes(s.id) ? 'checked' : ''} ${D.fixed.length ? '' : 'disabled'}> ${esc(s.alias)} <small class="hint">${esc(s.port)}</small></label>`).join('')}${D.fixed.filter(sid => !sess(sid)).map(sid => `<span class="hint" data-role="fixed-unresolved" style="color:var(--err)">미해결 고정 대상 ${esc(sid)} (저장된 세션 없음) — 저장하면 제거됩니다</span>`).join('')}</div></div>
        <div class="fld"><span>옵션</span><div class="chk-row" style="padding-top:5px"><label class="chk"><input type="checkbox" name="pinned" ${D.pinned ? 'checked' : ''}> pane 매크로 메뉴 맨 위에 표시 (★)</label></div></div>
      </div>
      <fieldset><legend>Step</legend><div data-role="steps">${stepsHTML()}</div>
        <div style="display:flex;gap:6px;margin-top:8px;flex-wrap:wrap"><button type="button" class="btn sm" data-add="send">+ send</button><button type="button" class="btn sm" data-add="wait">+ wait</button><button type="button" class="btn sm" data-add="delay">+ delay</button>
        <span class="hint" style="align-self:center">wait는 그 step 시작 이후 수신 줄에서 문자열을 찾습니다. 시간 초과면 매크로가 멈춥니다.</span></div></fieldset>
      <fieldset><legend>변수 <span class="hint" style="font-weight:400">— send 텍스트에서 \${이름}, 실행 시 값 확인</span></legend><div data-role="vars">${varsHTML()}</div>
        <button type="button" class="btn sm" data-va="add" style="margin-top:8px">+ 변수</button></fieldset>
      <div><div class="hint" style="margin-bottom:4px">미리보기 (기본값 대입)</div><pre class="preview" data-role="preview">${esc(previewTxt())}</pre></div>
      <div class="err-text" data-role="err"></div>
    </div>
    <div class="dlg-f"><span class="hint">로컬 시뮬레이션에만 송신 · 코드 실행 없음</span><span class="grow"></span><button type="button" class="btn" data-close>취소</button><button type="submit" class="btn primary">저장</button></div>`,
  form => {
    const fd = new FormData(form); const err = $('[data-role="err"]', form);
    D.name = String(fd.get('name')).trim(); D.pinned = fd.has('pinned');
    D.fixed = fd.get('tmode') === 'fixed' ? fd.getAll('fixed').map(String) : [];
    if (fd.get('tmode') === 'fixed' && !D.fixed.length) { err.textContent = '“고정”을 고르면 세션을 하나 이상 선택하세요 (또는 패널 실행 대상 따름)'; return false; }
    if (!D.name) { err.textContent = '이름을 입력하세요'; return false; }
    if (S.macros.some(m => m.id !== D.id && m.name.toLowerCase() === D.name.toLowerCase())) { err.textContent = `“${D.name}” 이름이 이미 있습니다`; return false; }
    for (const [i, st] of D.steps.entries()) {
      if (st.type === 'wait' && !String(st.text || '').length) { err.textContent = `step ${i + 1}: wait 문자열이 비었습니다`; return false; }
      if (st.type !== 'send' && !(+st.ms >= 0 && +st.ms <= 60000)) { err.textContent = `step ${i + 1}: ms는 0–60000`; return false; }
    }
    const names = D.vars.map(v => v.name.trim());
    if (names.some(n => !/^\w+$/.test(n))) { err.textContent = '변수 이름은 영문/숫자/_'; return false; }
    if (new Set(names).size !== names.length) { err.textContent = '변수 이름 중복'; return false; }
    const used = new Set(); D.steps.forEach(st => String(st.text || '').replace(/\$\{(\w+)\}/g, (m, k) => used.add(k)));
    const undef = [...used].filter(k => !names.includes(k)); if (undef.length) { err.textContent = `정의되지 않은 변수: ${undef.join(', ')}`; return false; }
    D.vars.forEach(v => { v.name = v.name.trim(); });
    D.steps = D.steps.map(st => st.type === 'delay' ? { type: 'delay', ms: +st.ms } : st.type === 'wait' ? { type: 'wait', text: st.text, ms: +st.ms || 2000 } : { type: 'send', text: st.text ?? '', eol: st.eol || 'session' });
    const i = S.macros.findIndex(m => m.id === D.id);
    if (i >= 0) S.macros[i] = D; else S.macros.push(D);
    S.ui.side = 'macros'; S.ui.collapsed = false;
    persist.all(); renderAll(); toast(`매크로 “${D.name}” 저장됨`);
  },
  form => {
    const reSteps = () => { $('[data-role="steps"]', form).innerHTML = stepsHTML(); prev(); };
    const reVars = () => { $('[data-role="vars"]', form).innerHTML = varsHTML(); prev(); };
    const prev = () => { $('[data-role="preview"]', form).textContent = previewTxt(); };
    form.addEventListener('input', e => {
      const t = e.target;
      if (t.dataset.f) { const st = D.steps[+t.dataset.i]; if (t.dataset.f === 'type') { const ty = t.value; D.steps[+t.dataset.i] = ty === 'delay' ? { type: 'delay', ms: 500 } : ty === 'wait' ? { type: 'wait', text: st.text || '', ms: 2000 } : { type: 'send', text: st.text || '', eol: 'session' }; reSteps(); return; } st[t.dataset.f] = t.dataset.f === 'ms' ? +t.value : t.value; prev(); }
      if (t.dataset.vf) { D.vars[+t.dataset.vi][t.dataset.vf] = t.value; prev(); }
    });
    form.addEventListener('change', e => {
      if (e.target.name === 'tmode') { $$('input[name=fixed]', form).forEach(c => { c.disabled = e.target.value !== 'fixed'; }); return; }
      if (e.target.dataset.f === 'type' || e.target.dataset.f === 'eol') e.target.dispatchEvent(new Event('input', { bubbles: true }));
    });
    form.addEventListener('click', e => {
      const b = e.target.closest('button'); if (!b) return;
      if (b.dataset.add) { D.steps.push(b.dataset.add === 'delay' ? { type: 'delay', ms: 500 } : b.dataset.add === 'wait' ? { type: 'wait', text: '', ms: 2000 } : { type: 'send', text: '', eol: 'session' }); reSteps(); $$('.step-row', form).pop()?.querySelector('input')?.focus(); }
      else if (b.dataset.sa) { const i = +b.dataset.i; if (b.dataset.sa === 'del') D.steps.splice(i, 1); else { const j = b.dataset.sa === 'up' ? i - 1 : i + 1; [D.steps[i], D.steps[j]] = [D.steps[j], D.steps[i]]; } reSteps(); }
      else if (b.dataset.va === 'add') { D.vars.push({ name: `v${D.vars.length + 1}`, def: '' }); reVars(); }
      else if (b.dataset.va === 'del') { D.vars.splice(+b.dataset.vi, 1); reVars(); }
      else if (b.dataset.act === 'clr-hotkey') { D.hotkey = ''; $('[data-act="cap-hotkey"]', form).textContent = '없음 — 눌러서 지정'; }
      else if (b.dataset.act === 'cap-hotkey') {
        b.classList.add('capturing'); b.textContent = '키를 누르세요… (Esc 취소)';
        const onKey = ev => {
          if (['Shift', 'Control', 'Alt', 'Meta'].includes(ev.key)) return;
          ev.preventDefault(); ev.stopPropagation(); document.removeEventListener('keydown', onKey, true); b.classList.remove('capturing');
          if (ev.key === 'Escape') { b.textContent = D.hotkey || '없음 — 눌러서 지정'; return; }
          const k = comboOf(ev); const errEl = $('[data-role="err"]', form);
          if (!/^F([1-9]|1[0-2])$/.test(k.split('+').pop()) && !ev.ctrlKey && !ev.altKey) { errEl.textContent = 'F1–F12 또는 Ctrl/Alt 조합만 지정할 수 있습니다'; b.textContent = D.hotkey || '없음 — 눌러서 지정'; return; }
          if (RESERVED_KEYS.has(k)) { errEl.textContent = `${k}는 작업대 단축키로 예약되어 있습니다`; b.textContent = D.hotkey || '없음 — 눌러서 지정'; return; }
          const other = S.macros.find(m => m.id !== D.id && m.hotkey === k);
          if (other) { errEl.textContent = `${k}는 “${other.name}”이 사용 중입니다`; b.textContent = D.hotkey || '없음 — 눌러서 지정'; return; }
          errEl.textContent = ['F5', 'F11', 'F12'].includes(k) ? `참고: ${k}는 브라우저 기능을 덮어씁니다 (작업대 안에서만)` : '';
          D.hotkey = k; b.textContent = k;
        };
        document.addEventListener('keydown', onKey, true);
      }
    });
  });
}
function comboOf(e) {
  let k = e.code.startsWith('Key') ? e.code.slice(3) : e.code.startsWith('Digit') ? e.code.slice(5) : e.code === 'Equal' ? '=' : e.code === 'Minus' ? '-' : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Meta', k].filter(Boolean).join('+');
}
function duplicateMacro(mid) {
  const m = S.macros.find(x => x.id === mid); let n = 2, name = `${m.name} (복사)`; while (S.macros.some(x => x.name === name)) name = `${m.name} (복사 ${n++})`;
  const c = { ...structuredClone(m), id: nid('m-'), name, hotkey: '' }; S.macros.splice(S.macros.indexOf(m) + 1, 0, c);
  persist.all(); renderAll(); toast(`“${m.name}” 복제됨 → “${name}” (단축키는 비움)`);
}
function deleteMacro(mid) {
  const i = S.macros.findIndex(x => x.id === mid); if (i < 0) return; const m = S.macros[i];
  if (Object.values(T).some(t => t.run && t.run.status === 'running' && t.run.macroId === mid)) { toast('실행 중인 매크로는 먼저 중지하세요'); return; }
  S.macros.splice(i, 1); persist.all(); renderAll();
  toast(`매크로 “${m.name}” 삭제됨`, '되돌리기', () => { S.macros.splice(Math.min(i, S.macros.length), 0, m); persist.all(); renderAll(); });
}

/* ---- run confirm ---- */
/* ---- import / export ---- */
function exportJSON() {
  const data = { format: 'loom-uart-bench', version: 1, exportedAt: new Date().toISOString(), sessions: S.sessions, macros: S.macros };
  const txt = JSON.stringify(data, null, 2);
  openDlg($('#dlgConfirm'), `<div class="dlg-h">${ic('down')}<h2>세션·매크로 내보내기</h2></div>
    <div class="dlg-b"><p class="hint" style="margin:0">세션 ${S.sessions.length}개, 매크로 ${S.macros.length}개. 파일로 저장하거나 복사해 다른 작업대에 가져오세요.</p>
    <textarea class="inp mono" data-role="json" readonly style="height:240px;padding:8px;resize:vertical">${esc(txt)}</textarea></div>
    <div class="dlg-f"><button type="button" class="btn" data-act="copy">복사</button><span class="grow"></span><button type="button" class="btn" data-close>닫기</button><button type="button" class="btn primary" data-act="file">${ic('down')}파일로 저장</button></div>`, null, form => {
    form.addEventListener('click', e => {
      const b = e.target.closest('[data-act]'); if (!b) return;
      if (b.dataset.act === 'copy') { const ta = $('[data-role="json"]', form); ta.select(); (navigator.clipboard?.writeText(txt) || Promise.reject()).then(() => toast('JSON 복사됨'), () => { document.execCommand?.('copy'); toast('JSON 선택됨 — 복사하세요'); }); }
      if (b.dataset.act === 'file') { const a = document.createElement('a'); a.href = URL.createObjectURL(new Blob([txt], { type: 'application/json' })); a.download = `loom-uart-bench-${new Date().toISOString().slice(0, 10)}.json`; document.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 1000); toast('파일 저장을 시작했습니다'); }
    });
  });
}
function importDialog() {
  openDlg($('#dlgConfirm'), `<div class="dlg-h">${ic('up')}<h2>세션·매크로 가져오기</h2></div>
    <div class="dlg-b"><p class="hint" style="margin:0">내보낸 JSON을 붙여넣거나 파일을 고르세요. 같은 이름의 매크로는 이름 뒤에 “(가져옴)”을 붙이고, 같은 별칭의 세션은 건너뜁니다. 단축키 충돌은 비웁니다.</p>
    <textarea class="inp mono" name="json" data-role="json" placeholder='{"format":"loom-uart-bench","version":1,"macros":[...]}' style="height:200px;padding:8px;resize:vertical"></textarea>
    <div class="err-text" data-role="err"></div></div>
    <div class="dlg-f"><button type="button" class="btn" data-act="pick-file">${ic('file')}파일 선택…</button><span class="grow"></span><button type="button" class="btn" data-close>취소</button><button type="submit" class="btn primary">가져오기</button></div>`,
  form => {
    const err = $('[data-role="err"]', form); const r = importData(String(new FormData(form).get('json') || ''));
    if (r.error) { err.textContent = r.error; return false; }
    toast(`가져옴: 매크로 ${r.m}개, 세션 ${r.s}개${r.skipped ? ` · 건너뜀 ${r.skipped}` : ''}${r.remapped ? ` · 고정 대상 ${r.remapped}개 새 세션에 연결` : ''}${r.unresolved ? ` · 고정 대상 ${r.unresolved}개 미해결 — 해당 매크로는 실행되지 않음 (⋯ 편집에서 다시 지정)` : ''}`);
  },
  form => {
    form.addEventListener('click', e => { if (e.target.closest('[data-act="pick-file"]')) { const fi = $('#importFile'); fi.value = ''; fi.onchange = () => { const f = fi.files[0]; if (!f) return; f.text().then(t => { $('[data-role="json"]', form).value = t; }); }; fi.click(); } });
  });
}
function importData(txt) {
  let d; try { d = JSON.parse(txt); } catch (e) { return { error: 'JSON 형식이 아닙니다: ' + e.message }; }
  if (!d || typeof d !== 'object') return { error: '객체가 아닙니다' };
  const ms = Array.isArray(d) ? d : Array.isArray(d.macros) ? d.macros : [];
  const ss = Array.isArray(d.sessions) ? d.sessions : [];
  if (!ms.length && !ss.length) return { error: 'macros 또는 sessions 배열이 없습니다' };
  let m = 0, s = 0, skipped = 0, unresolved = 0, remapped = 0;
  const idMap = {}; // payload session id -> local session id (new, or existing session with the same alias)
  for (const x of ss) {
    if (!x || typeof x.alias !== 'string' || typeof x.port !== 'string') { skipped++; continue; }
    const same = S.sessions.find(y => y.alias.toLowerCase() === x.alias.toLowerCase());
    if (same) { if (typeof x.id === 'string') idMap[x.id] = same.id; skipped++; continue; }
    const nidS = nid('s-'); if (typeof x.id === 'string') idMap[x.id] = nidS;
    S.sessions.push({ ...DEF_SESSIONS[0], ...x, id: nidS, profile: PROFILES[x.profile] ? x.profile : 'mcu', baud: +x.baud || 115200 }); s++;
  }
  for (const x of ms) {
    if (!x || typeof x.name !== 'string' || !Array.isArray(x.steps) || !x.steps.length) { skipped++; continue; }
    const steps = x.steps.filter(st => st && ['send', 'wait', 'delay'].includes(st.type)).map(st => st.type === 'delay' ? { type: 'delay', ms: clamp(+st.ms || 0, 0, 60000) } : st.type === 'wait' ? { type: 'wait', text: String(st.text ?? ''), ms: clamp(+st.ms || 2000, 50, 60000) } : { type: 'send', text: String(st.text ?? ''), eol: EOLS[st.eol] !== undefined ? st.eol : 'session' });
    if (!steps.length) { skipped++; continue; }
    let name = x.name.slice(0, 40); if (S.macros.some(y => y.name.toLowerCase() === name.toLowerCase())) name = `${name} (가져옴)`;
    const hk = typeof x.hotkey === 'string' && !S.macros.some(y => y.hotkey === x.hotkey) && !RESERVED_KEYS.has(x.hotkey) ? x.hotkey : '';
    // legacy target -> fixed override; payload ids remapped to local ids; unresolved ids are KEPT (visible “미해결”, macro blocked) — never widened to the panel scope
    const fixed = (Array.isArray(x.fixed) ? x.fixed : x.target && x.target !== 'active' ? [x.target] : []).filter(sid => typeof sid === 'string').map(sid => {
      if (idMap[sid]) { if (idMap[sid] !== sid) remapped++; return idMap[sid]; }
      if (!sess(sid)) unresolved++;
      return sid;
    });
    S.macros.push({ id: nid('m-'), name, fixed, hotkey: hk, pinned: !!x.pinned, vars: Array.isArray(x.vars) ? x.vars.filter(v => v && /^\w+$/.test(v.name)).map(v => ({ name: v.name, def: String(v.def ?? '') })) : [], steps }); m++;
  }
  persist.all(); renderAll();
  return { m, s, skipped, unresolved, remapped };
}

/* ---- keys ---- */
function keysDialog() {
  const rows = [
    ['Alt+Shift+=', '활성 pane 오른쪽 분할'], ['Alt+Shift+-', '활성 pane 아래 분할'], ['Alt+Shift+Enter', '활성 pane 최대화 / 복원'], ['Alt+Shift+W', '활성 pane 닫기 (마지막 pane은 비움)'],
    ['Alt+←↑→↓', '인접 pane으로 포커스'], ['Alt+Shift+←↑→↓', '활성 탭을 인접 pane으로 이동 (없으면 새 분할)'],
    ['경계에서 ←→ / ↑↓', '경계 2% 이동 (Shift 10%) · Enter 50:50'], ['Alt+F', '활성 터미널 검색'], ['Alt+1 / Alt+2 / Alt+0', '세션 / 매크로 패널 / 패널 접기'],
    ['Esc (터미널 밖에서)', '활성 pane의 실행 중 매크로 중지 · 검색·메뉴 닫기 — 터미널 포커스 중 Esc는 장치로 전송되므로 pane의 중지 버튼을 쓰세요'], ...S.macros.filter(m => m.hotkey).map(m => [m.hotkey, `매크로 “${m.name}”`]),
  ];
  openDlg($('#dlgKeys'), `<div class="dlg-h">${ic('kbd')}<h2>단축키</h2></div><div class="dlg-b"><div class="keys-tbl">${rows.map(([k, v]) => `<kbd>${esc(k)}</kbd><span>${esc(v)}</span>`).join('')}</div>
    <p class="hint" style="margin:0">탭은 포인터로 다른 pane의 탭 줄(순서 지정) 또는 pane 가장자리(새 분할)로 끌 수 있습니다. 탭 우클릭이나 ⋯ 메뉴에서 “pane N으로 이동”도 가능합니다.</p></div>
    <div class="dlg-f"><button type="submit" class="btn primary">닫기</button></div>`);
}

/* ---- context menu ---- */
let menuEl = null;
function closeMenu() { menuEl && menuEl.remove(); menuEl = null; }
function openMenu(x, y, items) {
  closeMenu();
  const m = document.createElement('div'); m.className = 'menu'; m.setAttribute('role', 'menu');
  m.innerHTML = items.map((it, i) => it === '-' ? '<hr>' : it.h ? `<div class="mh">${esc(it.h)}</div>` : `<button type="button" role="menuitem" data-mi="${i}" ${it.disabled ? 'disabled' : ''}>${it.icon ? ic(it.icon) : ''}${esc(it.label)}${it.k ? `<span class="k">${esc(it.k)}</span>` : ''}</button>`).join('');
  document.body.appendChild(m); menuEl = m;
  const r = m.getBoundingClientRect();
  m.style.left = clamp(x, 6, window.innerWidth - r.width - 6) + 'px'; m.style.top = clamp(y, 6, window.innerHeight - r.height - 6) + 'px';
  m.addEventListener('click', e => { const b = e.target.closest('[data-mi]'); if (!b) return; const it = items[+b.dataset.mi]; closeMenu(); it.run(); });
  m.querySelector('button:not([disabled])')?.focus();
}
function tabMenu(tid, x, y) {
  const t = T[tid]; const s = sess(t.sid); const L = leafOfTab(tid);
  const others = leaves().filter(l => l.id !== L.id);
  openMenu(x, y, [
    { h: `${s.alias} · ${s.port}` },
    ...others.map(l => ({ label: `pane ${paneNo(l.id)}로 이동${l.on ? ` (${sess(T[l.on]?.sid)?.alias})` : ' (비어 있음)'}`, icon: 'send', run: () => placeTab(tid, l.id, 'tab') })),
    { label: '새 오른쪽 분할로 이동', icon: 'split-r', disabled: L.tabs.length < 2, run: () => placeTab(tid, L.id, 'right') },
    { label: '새 아래 분할로 이동', icon: 'split-d', disabled: L.tabs.length < 2, run: () => placeTab(tid, L.id, 'bottom') },
    '-',
    { label: s.ts ? '타임스탬프 숨기기' : '타임스탬프 표시', run: () => { s.ts = !s.ts; persist.all(); renderTerm(tid); } },
    { label: '로그 저장 (.log)', icon: 'down', run: () => saveLog(tid) },
    { label: t.conn === 'connected' ? '장치 분리 시뮬레이션' : '장치 분리 시뮬레이션 (연결 필요)', icon: 'unplug', disabled: t.conn !== 'connected', run: () => disconnect(tid, 'lost') },
    { label: 'DTR 펄스 (대상 리셋)', disabled: t.conn !== 'connected', run: () => { sysLine(t, 'DTR 펄스 100ms → 대상 리셋'); t.sim && t.sim.reset(); } },
    '-',
    { label: '탭 닫기 (포트 닫힘)', icon: 'x', run: () => closeTab(tid) },
  ]);
}
function saveLog(tid) {
  const t = T[tid]; const s = sess(t.sid);
  const txt = t.lines.map(l => `${fmtTs(l.t)} ${l.d.toUpperCase().padEnd(3)} ${l.s}`).join('\n');
  const a = document.createElement('a'); a.href = URL.createObjectURL(new Blob([txt], { type: 'text/plain' })); a.download = `${s.alias}-${new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)}.log`;
  document.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  toast(`${s.alias} 로그 ${t.lines.length}줄 저장`);
}

/* ---- toast ---- */
function toast(msg, actLabel, act) {
  const box = $('#toasts'); const el = document.createElement('div'); el.className = 'toast'; el.setAttribute('role', 'status');
  el.innerHTML = `<span>${esc(msg)}</span>${actLabel ? `<button type="button">${esc(actLabel)}</button>` : ''}`;
  if (actLabel) el.querySelector('button').addEventListener('click', () => { el.remove(); act(); });
  box.appendChild(el); while (box.children.length > 4) box.firstChild.remove();
  setTimeout(() => el.remove(), actLabel ? 7000 : 3800);
}

/* ============================== commands & events ============================== */
function cmd(name, lid) {
  lid = lid || S.layout.active;
  const L = findNode(lid);
  switch (name) {
    case 'split-right': case 'split-down': {
      if (!L) return; if (S.layout.max) S.layout.max = null;
      if (splitLeaf(lid, name === 'split-right' ? 'right' : 'bottom', [])) renderAll(); break;
    }
    case 'maximize': S.layout.max = S.layout.max ? null : (L && leaves().length > 1 ? lid : null); if (L) S.layout.active = lid; renderAll(); break;
    case 'close-pane': {
      if (!L) return;
      for (const tid of L.tabs.slice()) { const t = T[tid]; if (t) { if (t.run && t.run.status === 'running') stopMacro(tid); bcDrop(tid, 'pane 닫힘'); t.sim && t.sim.stop(); clearTimeout(t.retryT); disposeTab(t); delete T[tid]; } }
      if (!removeLeaf(lid)) { L.tabs = []; L.on = null; S.layout.max = null; toast('마지막 pane은 닫지 않고 비워 둡니다 — 세션을 열거나 분할하세요'); }
      renderAll(); break;
    }
    case 'toggle-side': S.ui.collapsed = !S.ui.collapsed; if (!S.ui.collapsed && !S.ui.side) S.ui.side = 'sessions'; renderSide(); renderChrome(); persist.all(); break;
    case 'bc-current': setBroadcast(false); break;
    case 'bc-all': setBroadcast(true); break;
    case 'stop-all': stopAllRuns(); break;
    case 'new-session': openSessionDialog(null, true); break;
    case 'new-macro': openMacroDialog(null, true); break;
    case 'export': exportJSON(); break;
    case 'import': importDialog(); break;
    case 'keys': keysDialog(); break;
    case 'reset-layout': confirmDlg('레이아웃 초기화', '열린 탭을 모두 닫고 mcu | aux 두 pane으로 되돌립니다. 저장된 세션·매크로는 그대로입니다.', '초기화', () => {
      for (const t of Object.values(T)) { t.sim && t.sim.stop(); clearTimeout(t.retryT); disposeTab(t); delete T[t.id]; }
      initLayout(null); renderAll(); }); break;
  }
}
const work = $('#work');
document.addEventListener('click', e => {
  if (suppressClick) { suppressClick = false; e.preventDefault(); e.stopPropagation(); return; }
  if (menuEl && !e.target.closest('.menu')) closeMenu();
  const c = e.target.closest('[data-cmd]'); if (c) { cmd(c.dataset.cmd); return; }
  const railB = e.target.closest('.rail [data-side]');
  if (railB) { const p = railB.dataset.side; if (!S.ui.collapsed && S.ui.side === p) S.ui.collapsed = true; else { S.ui.side = p; S.ui.collapsed = false; } renderSide(); renderChrome(); persist.all(); return; }
  if (e.target.id === 'sideBackdrop') { S.ui.collapsed = true; renderChrome(); persist.all(); return; }
  const b = e.target.closest('[data-act]');
  const pane = e.target.closest('.pane');
  if (pane && !b?.dataset.leaf) setActiveLeaf(pane.dataset.leaf);
  const tabEl = e.target.closest('.tab');
  if (tabEl && (!b || b.dataset.act !== 'close-tab')) { focusTab(tabEl.dataset.tab); return; }
  if (!b) return;
  const tid = b.dataset.tab, t = tid && T[tid], lid = b.dataset.leaf;
  switch (b.dataset.act) {
    case 'close-tab': closeTab(tid); break;
    case 'split-right': case 'split-down': cmd(b.dataset.act, lid); break;
    case 'max': S.layout.active = lid; cmd('maximize', lid); break;
    case 'close-pane': cmd('close-pane', lid); break;
    case 'add-tab': {
      const r = b.getBoundingClientRect();
      openMenu(r.left, r.bottom + 4, [{ h: `pane ${paneNo(lid)}에 열기` }, ...S.sessions.map(s => { const ot = tabOfSession(s.id); return { label: `${s.alias} · ${s.port}${ot ? ` (pane ${paneNo(leafOfTab(ot.id).id)}에서 옮김)` : ''}`, run: () => placeSession(s.id, lid, 'tab') }; }), '-', { label: '새 세션…', icon: 'plus', run: () => openSessionDialog(null, true) }]);
      break;
    }
    case 'pick': placeSession(b.dataset.sid, lid, 'tab'); break;
    case 'mode': t.mode = b.dataset.mode; renderPane(leafOfTab(tid).id); persist.all(); break;
    case 'ts-toggle': { const ss = sess(t.sid); ss.ts = !ss.ts; persist.all(); renderPane(leafOfTab(tid).id); break; }
    case 'rec': openRecDialog(tid); break;
    case 'pause': t.paused = !t.paused; if (t.paused) t.pauseMark = t.lines.length; else { t.stick = true; if (t.xt && t.xpend.length) flushPaused(t); } renderPane(leafOfTab(tid).id); break;
    case 'clear': t.clearIdx = t.lines.length; t.pauseMark = Math.max(t.pauseMark, t.clearIdx); if (t.xt) t.xt.clear(); renderTerm(tid); toast(`${sess(t.sid).alias} 화면 지움 (로그 ${t.lines.length}줄 보존)`); break;
    case 'search': t.search.open = !t.search.open; if (!t.search.open) t.search.cur = -1; renderPane(leafOfTab(tid).id); if (t.search.open) $(`[data-fkey="search:${tid}"]`)?.focus(); else t.xt && t.xt.focus(); break;
    case 'theme': t.theme = t.theme === 'dark' ? 'light' : 'dark'; if (t.xt) t.xt.options.theme = XTHEME[t.theme]; renderPane(leafOfTab(tid).id); persist.all(); break;
    case 'connect': clearTimeout(t.retryT); connect(tid); break;
    case 'disconnect': disconnect(tid, 'user'); break;
    case 'cancel-retry': clearTimeout(t.retryT); t.conn = 'disconnected'; sysLine(t, '자동 재연결 취소'); structural(leafOfTab(tid).id); break;
    case 'settings': openSessionDialog(t.sid); break;
    case 'more': { const r = b.getBoundingClientRect(); tabMenu(tid, r.right - 220, r.bottom + 4); break; }
    case 'to-bottom': t.stick = true; renderTerm(tid); break;
    case 'macros-menu': {
      const r = b.getBoundingClientRect(); const al = sess(t.sid).alias;
      void al; // the pane click already made this pane active; targets follow the one macro scope rule
      openMenu(r.left, r.bottom + 4, [{ h: `실행 대상: ${macroTargetLabel({ fixed: [] })}` }, ...[...S.macros].sort((x, y) => (y.pinned ? 1 : 0) - (x.pinned ? 1 : 0)).map(m => ({ label: `${m.pinned ? '★ ' : ''}${m.name}${m.fixed.length ? ` (고정 → ${m.fixed.map(sid => sess(sid)?.alias || '?').join(', ')})` : ''}`, k: m.hotkey || '', icon: 'play', run: () => requestRun(m.id, tid) })), '-', { label: '새 매크로…', icon: 'plus', run: () => openMacroDialog(null, true) }]);
      break;
    }
    case 'run-macro': requestRun(b.dataset.macro, tid); break;
    case 'stop-macro': stopMacro(tid); break;
    case 'ms-mode': MS.mode = b.dataset.mode; withFocus(() => renderSide()); break;
    case 'stop-macro-all': { let n = 0; for (const x of Object.values(T)) if (x.run && x.run.status === 'running' && x.run.macroId === b.dataset.macro) { stopMacro(x.id); n++; } toast(`매크로 ${n}곳 중지 (남은 step 송신 안 함)`); renderSide(); break; }
    case 'mac-more': { const r = b.getBoundingClientRect(); const mid = b.dataset.macro; const mm = S.macros.find(x => x.id === mid);
      openMenu(r.right - 200, r.bottom + 4, [{ h: mm ? mm.name : '' }, { label: '편집…', icon: 'edit', run: () => openMacroDialog(mid) }, { label: '복제', icon: 'copy', run: () => duplicateMacro(mid) }, '-', { label: '삭제', icon: 'trash', run: () => deleteMacro(mid) }]); break; }
    case 'dismiss-run': t.run = null; renderPane(leafOfTab(tid).id); break;
    case 'collapse-side': S.ui.collapsed = true; renderChrome(); persist.all(); break;
    case 'toggle-group': { const g = b.dataset.group; S.ui.closedGroups = S.ui.closedGroups.includes(g) ? S.ui.closedGroups.filter(x => x !== g) : [...S.ui.closedGroups, g]; renderSide(); persist.all(); break; }
    case 'open-here': placeSession(b.dataset.sid, S.layout.active, 'tab'); break;
    case 'open-right': placeSession(b.dataset.sid, S.layout.active, 'right'); break;
    case 'open-down': placeSession(b.dataset.sid, S.layout.active, 'bottom'); break;
    case 'edit-session': openSessionDialog(b.dataset.sid); break;
    case 'new-session': openSessionDialog(null, true); break;
    case 'new-macro': openMacroDialog(null, true); break;
    case 'edit-macro': openMacroDialog(b.dataset.macro); break;
    case 'dup-macro': duplicateMacro(b.dataset.macro); break;
    case 'del-macro': deleteMacro(b.dataset.macro); break;
    case 'import': importDialog(); break;
    case 'export': exportJSON(); break;
  }
});
document.addEventListener('dblclick', e => {
  const s = e.target.closest('.sess'); if (s && !e.target.closest('button')) { placeSession(s.dataset.sid, S.layout.active, 'tab'); return; }
  const dv = e.target.closest('.divider'); if (dv) nudgeDivider(dv, 0, 0.5);
});
document.addEventListener('contextmenu', e => { const tb = e.target.closest('.tab'); if (tb) { e.preventDefault(); tabMenu(tb.dataset.tab, e.clientX, e.clientY); } });
document.addEventListener('pointerdown', e => {
  if (menuEl && !e.target.closest('.menu')) closeMenu();
  const dv = e.target.closest('.divider'); if (dv) { startResize(e, dv); return; }
  const tb = e.target.closest('.tab'); if (tb) { startDrag(e, { kind: 'tab', tid: tb.dataset.tab }, tb); return; }
  const se = e.target.closest('.sess'); if (se) { startDrag(e, { kind: 'session', sid: se.dataset.sid }, se); return; }
  const pane = e.target.closest('.pane'); if (pane) setActiveLeaf(pane.dataset.leaf);
});
work.addEventListener('scroll', e => {
  const el = e.target; if (!el.classList || !el.classList.contains('term')) return;
  const t = T[el.dataset.tab]; if (!t) return;
  t.stick = el.scrollHeight - el.scrollTop - el.clientHeight < 24; t.scrollTop = el.scrollTop;
  const note = $('[data-role="note"]', el.parentElement); note && note.classList.toggle('show', !t.stick && !t.paused);
}, true);
document.addEventListener('input', e => {
  const el = e.target;
  if (el.id === 'sessFilter') { S.ui.filter = el.value; renderSide(); return; }
  const tid = el.dataset.tab; const t = tid && T[tid]; if (!t) return;
  if (el.dataset.act === 'search-q') { t.search.q = el.value; t.search.cur = el.value ? 0 : -1; t.stick = false; renderTerm(tid); scrollToMark(tid); }
});
document.addEventListener('change', e => {
  const el = e.target;
  if (el.dataset && el.dataset.act === 'ms-sel') { const sid = el.value; MS.sel = el.checked ? [...new Set([...MS.sel, sid])] : MS.sel.filter(x => x !== sid); withFocus(() => renderSide()); return; }
  const tid = el.dataset?.tab; const t = tid && T[tid]; if (!t) return;
  if (el.dataset.act === 'eol') { t.eol = el.value; renderPane(leafOfTab(tid).id); persist.all(); }
  if (el.dataset.act === 'search-filter') { t.search.filter = el.checked; renderTerm(tid); }
});
function scrollToMark(tid) { const el = $(`.term[data-tab="${tid}"] mark.cur`); if (el) el.scrollIntoView({ block: 'center' }); }

document.addEventListener('keydown', e => {
  if (anyDialogOpen()) return;
  const k = comboOf(e);
  const el = e.target;
  if (el.dataset && el.dataset.act === 'search-q' && e.key === 'Enter') {
    e.preventDefault(); const t = T[el.dataset.tab]; const n = $$(`.term[data-tab="${t.id}"] mark`).length; if (!n) return;
    t.search.cur = ((t.search.cur < 0 ? 0 : t.search.cur) + (e.shiftKey ? -1 : 1) + n) % n; t.stick = false; renderTerm(t.id); scrollToMark(t.id); return;
  }
  if (el.classList && el.classList.contains('divider')) {
    const row = findNode(el.dataset.split)?.dir === 'row'; const st = e.shiftKey ? 0.1 : 0.02;
    const m = { ArrowLeft: row ? -st : 0, ArrowRight: row ? st : 0, ArrowUp: row ? 0 : -st, ArrowDown: row ? 0 : st };
    if (e.key in m && m[e.key] && !e.altKey) { e.preventDefault(); nudgeDivider(el, m[e.key]); return; }
    if (e.key === 'Home') { e.preventDefault(); nudgeDivider(el, 0, 0); return; }
    if (e.key === 'End') { e.preventDefault(); nudgeDivider(el, 0, 1); return; }
    if (e.key === 'Enter') { e.preventDefault(); nudgeDivider(el, 0, 0.5); return; }
  }
  if (el.classList && el.classList.contains('tab') && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); focusTab(el.dataset.tab, true); return; }
  if (el.classList && el.classList.contains('sess') && e.key === 'Enter') { e.preventDefault(); placeSession(el.dataset.sid, S.layout.active, 'tab'); return; }
  // macro hotkeys
  const mac = S.macros.find(m => m.hotkey && m.hotkey === k);
  if (mac) { e.preventDefault(); requestRun(mac.id, null); return; }
  if (e.key === 'Escape') {
    if (menuEl) { closeMenu(); return; }
    if (drag) { const d = endDrag(); if (d && d.active) { suppressClick = false; if (deferred) { deferred = false; renderAll(); } } return; }
    const t = activeTab(); if (t && t.run && t.run.status === 'running') { e.preventDefault(); stopMacro(t.id); toast(`매크로 “${t.run.name}” 중지됨`); return; }
    if (t && t.search.open) { t.search.open = false; renderPane(leafOfTab(t.id).id); return; }
    return;
  }
  const arrows = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
  if (e.altKey && !e.ctrlKey && !e.metaKey && arrows[e.key]) {
    e.preventDefault();
    if (e.shiftKey) moveActiveTab(arrows[e.key]);
    else { const n = neighbor(S.layout.active, arrows[e.key]); if (n) { S.layout.active = n.id; renderAll(); if (n.on) $(`[data-fkey="send:${n.on}"]`)?.focus(); } }
    return;
  }
  const map = { 'Alt+Shift+=': 'split-right', 'Alt+Shift+-': 'split-down', 'Alt+Shift+Enter': 'maximize', 'Alt+Shift+W': 'close-pane' };
  if (map[k]) { e.preventDefault(); cmd(map[k]); return; }
  if (k === 'Alt+1' || k === 'Alt+2') { e.preventDefault(); S.ui.side = k === 'Alt+1' ? 'sessions' : 'macros'; S.ui.collapsed = false; renderSide(); renderChrome(); persist.all(); return; }
  if (k === 'Alt+0') { e.preventDefault(); S.ui.collapsed = !S.ui.collapsed; renderChrome(); persist.all(); return; }
  if (k === 'Alt+F') { e.preventDefault(); const t = activeTab(); if (t) { t.search.open = true; renderPane(leafOfTab(t.id).id); $(`[data-fkey="search:${t.id}"]`)?.focus(); } return; }
  if (e.key === '?' && !/INPUT|TEXTAREA|SELECT/.test(el.tagName)) { keysDialog(); }
}, false);
window.addEventListener('resize', () => { if (isNarrow()) return; });
window.addEventListener('beforeunload', saveNow);

/* ============================== SIM (local only) ============================== */
const MCU_BOOT = ['', 'LoomBoot 2.3.1 (Oct 28 2026 - 14:02:11 +0900)', 'CPU:   STM32H743 rev V @ 480 MHz', 'Reset: PIN (DTR)', 'Flash: 2048 KiB, ECC ok', 'RAM:   1024 KiB, test passed',
  'Boot:  slot A  fw=ecu-app 4.12.0+g7c1e2d9  crc=0x9E41A7C2 ok', 'Jumping to application @ 0x08020000', '[    0.000] ecu-app 4.12.0 starting', '[    0.002] clk: HSE 25 MHz, PLL1 480 MHz, APB1 120 MHz',
  '[    0.011] gpio: 6 banks configured', '[    0.019] adc1: calibrated offset=+3 lsb', '[    0.020] adc2: calibrated offset=-1 lsb', '[    0.034] can0: 500 kbit/s, sample point 87.5%',
  '[    0.035] can1: 500 kbit/s, sample point 87.5% (listen-only)', '[    0.041] i2c2: probe 0x48 tmp117 ok', '[    0.043] i2c2: probe 0x50 eeprom ok, 32 KiB', '[    0.057] spi1: probe w25q64 ok, 8 MiB',
  '[    0.061] W: rtc: lost power, time reset to 2026-01-01 00:00:00', '[    0.078] nvm: config v7 loaded (412 bytes)', '[    0.090] wdt: enabled, timeout 2000 ms', '[    0.102] shell: uart1 115200 8N1', '[    0.103] ready.', ''];
const AUX_BOOT = ['', 'AUX-PWR v1.4.2 (bootloader 0.9.1)', 'rails: 12V_SW=on 5V0=on 3V3_AUX=on', 'relays: 0b0101', '+READY'];
const LNX_BOOT = ['', 'U-Boot 2024.04-loom (Sep 02 2026 - 09:12:40 +0000)', '', 'CPU:   i.MX8MM rev1.0 1800 MHz (running at 1200 MHz)', 'Model: Loom GW-SoM rev C', 'DRAM:  2 GiB', 'MMC:   FSL_SDHC: 1, FSL_SDHC: 2',
  'Loading Environment from MMC... OK', 'Net:   eth0: ethernet@30be0000', 'Hit any key to stop autoboot:  0', 'switch to partitions #0, OK', '29360640 bytes read in 312 ms (89.7 MiB/s)', 'Booting from mmc ...',
  'Starting kernel ...', '', '[    0.000000] Booting Linux on physical CPU 0x0000000000 [0x410fd034]', '[    0.000000] Linux version 6.6.23-loom (oe-user@oe-host) #1 SMP PREEMPT',
  '[    0.000000] Machine model: Loom GW-SoM rev C', '[    0.000000] earlycon: ec_imx6q0 at MMIO 0x0000000030890000 (options \'115200n8\')', '[    0.412318] 30890000.serial: ttymxc1 at MMIO 0x30890000 (irq = 31) is a IMX',
  '[    0.981402] flexcan 308c0000.can: registered (rx-fifo)', '[    1.204557] imx_thermal: Industrial CPU temperature grade - max:105C', '[    1.877120] EXT4-fs (mmcblk2p2): mounted filesystem with ordered data mode',
  '[    2.301933] W: imx-sdma 302c0000.dma-controller: external firmware not found, using ROM firmware', '[    3.115620] fec 30be0000.ethernet eth0: Link is Up - 100Mbps/Full', '[    3.900112] systemd[1]: Reached target Multi-User System.',
  '', 'Loom GW-SoM (Yocto 5.1 scarthgap) gw-som ttymxc1', ''];
class Sim {
  constructor(tab, profile) { this.tab = tab; this.profile = profile; this.timers = new Set(); this.t0 = performance.now(); this.auth = 0; this.user = ''; this.n = 0; this.tlm = true; this.hb = true;
    this.lb = ''; this.lc = 0; this.hist = []; this.hi = -1; this.esc = ''; this.pStr = ''; this.pActive = false; this.noEcho = false; this.lastCR = false; this.inq = ''; this.busy = false; this.cmdT = 0; }
  get s() { return sess(this.tab.sid); }
  later(ms, f) { const h = setTimeout(() => { this.timers.delete(h); f(); }, ms); this.timers.add(h); }
  stop() { for (const h of this.timers) clearTimeout(h); this.timers.clear(); clearInterval(this.iv); this.iv = 0; this.running = false; this.inq = ''; this.busy = false; this.cmdT = 0; }
  up() { return (performance.now() - this.t0) / 1000; }
  garbled() { const s = this.s; return s && PROFILES[this.profile] && s.baud !== PROFILES[this.profile].devBaud; }
  /* raw device -> host bytes */
  w(x) {
    if (!T[this.tab.id] || this.tab.conn !== 'connected' || !x) return;
    if (this.garbled()) { const g = '\u00ff\u00fe\u00bf~\u00c6\u00d8\u00a4x\u00e0'; x = stripAnsi(x).replace(/[ -~]/g, (c, i) => g[(c.charCodeAt(0) + i) % 9]); }
    feed(this.tab, x);
  }
  color(l) {
    if (/(^|[\s\]])(E:|E\/|ERR\b|ERROR\b)|Login incorrect|overrange/.test(l)) return `\x1b[31m${l}\x1b[0m`;
    if (/(^|[\s\]])(W:|W\/)|VIN_SAG|bus-off/.test(l)) return `\x1b[33m${l}\x1b[0m`;
    if (/^(OK|\+READY|ready\.)$/.test(l.trim())) return `\x1b[32m${l}\x1b[0m`;
    if (/^(D\/|\[ +\d+\.\d+\] TLM)/.test(l)) return `\x1b[2m${l}\x1b[0m`;
    return l;
  }
  /* one line of device output; prompts (login:, Password:, mcu>, #) are printed without newline */
  out(line) {
    if (/(login: |Password: |> |# )$/.test(line)) return this.prompt(line);
    const c = this.color(line);
    if (this.pActive) { this.w('\r\x1b[K' + c + '\r\n'); this.redraw(); } // async log over an active prompt: reprint prompt + typed buffer
    else this.w(c + '\r\n');
  }
  prompt(p) { this.pStr = p; this.pActive = true; this.lb = ''; this.lc = 0; this.hi = -1; this.noEcho = /Password: $/.test(p); this.w(p); }
  redraw() { const b = this.noEcho ? '' : this.lb; const back = this.noEcho ? 0 : this.lb.length - this.lc; this.w('\r\x1b[K' + this.pStr + b + (back > 0 ? `\x1b[${back}D` : '')); }
  move(n) { if (!n) return; this.lc += n; if (!this.noEcho) this.w(n < 0 ? `\x1b[${-n}D` : `\x1b[${n}C`); }
  cmds() {
    if (this.profile === 'mcu') return this.auth === 2 ? ['help', 'status', 'temp', 'version', 'log on', 'log off', 'reset', 'logout'] : [];
    if (this.profile === 'linux') return this.auth === 2 ? ['help', 'uname -a', 'uptime', 'ip -br a', 'ls', 'dmesg | tail -n 3', 'exit'] : [];
    if (this.profile === 'aux') return ['AT', 'ATI', 'AT+STAT?', 'AT+RLY=', 'AT+HB='];
    return [];
  }
  complete() {
    if (this.noEcho) return;
    const pre = this.lb.slice(0, this.lc).toLowerCase();
    const c = this.cmds().filter(x => x.toLowerCase().startsWith(pre));
    if (!c.length) { this.w('\x07'); return; }
    let cp = c[0]; for (const x of c) while (!x.toLowerCase().startsWith(cp.toLowerCase())) cp = cp.slice(0, -1);
    if (c.length === 1) { this.lb = c[0] + (/[=?]$/.test(c[0]) ? '' : ' '); this.lc = this.lb.length; this.redraw(); return; }
    if (cp.length > pre.length) { this.lb = cp; this.lc = cp.length; this.redraw(); return; }
    this.w('\r\n' + c.join('    ') + '\r\n'); this.redraw();
  }
  escKey(e) {
    const len = this.lb.length;
    if (e === '\x1b[A') { if (!this.hist.length) return; this.hi = this.hi < 0 ? this.hist.length - 1 : Math.max(0, this.hi - 1); this.lb = this.hist[this.hi]; this.lc = this.lb.length; this.redraw(); }
    else if (e === '\x1b[B') { if (this.hi < 0) return; this.hi++; if (this.hi >= this.hist.length) { this.hi = -1; this.lb = ''; } else this.lb = this.hist[this.hi]; this.lc = this.lb.length; this.redraw(); }
    else if (e === '\x1b[C') { if (this.lc < len) this.move(1); }
    else if (e === '\x1b[D') { if (this.lc > 0) this.move(-1); }
    else if (['\x1b[H', '\x1b[1~', '\x1bOH'].includes(e)) this.move(-this.lc);
    else if (['\x1b[F', '\x1b[4~', '\x1bOF'].includes(e)) this.move(len - this.lc);
    else if (e === '\x1b[3~' && this.lc < len) { this.lb = this.lb.slice(0, this.lc) + this.lb.slice(this.lc + 1); if (!this.noEcho) { const tail = this.lb.slice(this.lc); this.w(tail + ' ' + `\x1b[${tail.length + 1}D`); } }
  }
  /* device-side line discipline (echo, edit, history, Tab completion, ^C ^U ^L) */
  key(ch) {
    if (this.esc) {
      this.esc += ch;
      if (/^\x1b\[[0-9;]*[A-Za-z~]$/.test(this.esc) || /^\x1bO[A-Za-z]$/.test(this.esc)) { const e = this.esc; this.esc = ''; this.escKey(e); }
      else if (this.esc.length > 8 || (this.esc.length === 2 && ch !== '[' && ch !== 'O')) this.esc = '';
      return;
    }
    if (ch === '\x1b') { this.esc = ch; return; }
    if (ch === '\n' && this.lastCR) { this.lastCR = false; return; }
    this.lastCR = ch === '\r';
    if (ch === '\r' || ch === '\n') {
      const line = this.lb; this.w('\r\n');
      if (line.trim() && !this.noEcho && this.hist[this.hist.length - 1] !== line) { this.hist.push(line); if (this.hist.length > 50) this.hist.shift(); }
      this.lb = ''; this.lc = 0; this.hi = -1; this.pActive = false;
      // one command at a time, fixed latency; later input waits in this.inq (arrival order)
      this.busy = true;
      const h = setTimeout(() => { this.timers.delete(h); this.cmdT = 0; this.handle(line); if (this.profile === 'aux') this.prompt(''); this.busy = false; this.drain(); }, 60);
      this.timers.add(h); this.cmdT = h;
      return;
    }
    if (ch === '\x03') { this.w('^C\r\n'); this.lb = ''; this.lc = 0; this.pActive = false; this.busy = true; const h = setTimeout(() => { this.timers.delete(h); this.cmdT = 0; this.interrupt(); this.busy = false; this.drain(); }, 30); this.timers.add(h); this.cmdT = h; return; }
    if (ch === '\x15') { this.lb = ''; this.lc = 0; this.redraw(); return; }
    if (ch === '\x0c') { this.w('\x1b[2J\x1b[H'); this.redraw(); return; }
    if (ch === '\x01') { this.move(-this.lc); return; }
    if (ch === '\x05') { this.move(this.lb.length - this.lc); return; }
    if (ch === '\x7f' || ch === '\b') {
      if (this.lc > 0) { this.lb = this.lb.slice(0, this.lc - 1) + this.lb.slice(this.lc); this.lc--; if (!this.noEcho) { const tail = this.lb.slice(this.lc); this.w('\b' + tail + ' ' + `\x1b[${tail.length + 1}D`); } }
      return;
    }
    if (ch === '\t') { this.complete(); return; }
    if (ch < ' ') return;
    this.lb = this.lb.slice(0, this.lc) + ch + this.lb.slice(this.lc); this.lc++;
    if (this.noEcho) return;
    const tail = this.lb.slice(this.lc); this.w(ch + tail + (tail.length ? `\x1b[${tail.length}D` : ''));
  }
  burst(lines, gap = 35, then) { lines.forEach((l, i) => this.later(i * gap, () => this.out(l))); if (then) this.later(lines.length * gap + 10, then); }
  start(fresh) {
    this.stop(); this.running = true;
    const p = this.profile;
    if (fresh) { this.t0 = performance.now(); this.auth = 0; this.pActive = false; this.pStr = ''; this.lb = ''; this.lc = 0; this.noEcho = false; }
    else if (this.pActive) this.redraw();
    if (p === 'mcu') { if (fresh) this.burst(MCU_BOOT, 28, () => this.out('loom-mcu login: ')); this.iv = setInterval(() => this.tickMcu(), 1000); }
    else if (p === 'aux') { if (fresh) this.burst(AUX_BOOT, 90, () => this.prompt('')); this.iv = setInterval(() => this.tickAux(), 3000); }
    else if (p === 'linux') { if (fresh) this.burst(LNX_BOOT, 45, () => this.out('gw-som login: ')); this.iv = setInterval(() => this.tickLnx(), 9000); }
    else { if (fresh) this.out('=== SWO/RTT bridge ch0 · ecu-app trace ==='); this.iv = setInterval(() => this.tickLog(), 260); }
  }
  interrupt() {
    if (this.profile === 'mcu') { if (this.auth === 1) this.auth = 0; this.out(this.auth === 2 ? 'mcu> ' : 'loom-mcu login: '); }
    else if (this.profile === 'linux') { if (this.auth === 1) this.auth = 0; this.out(this.auth === 2 ? `${this.user}@gw-som:~# ` : 'gw-som login: '); }
    else if (this.profile === 'aux') this.prompt('');
  }
  reset() { this.stop(); this.pActive = false; this.w('\r\n'); this.out('Resetting...'); this.later(400, () => this.start(true)); this.running = true; }
  tickMcu() {
    if (!this.tlm) return; this.n++; const t = this.up().toFixed(3).padStart(9);
    const vb = (12.4 + Math.sin(this.n / 7) * 0.05).toFixed(2), ib = (0.82 + Math.sin(this.n / 3) * 0.04).toFixed(2), tp = (41.2 + Math.sin(this.n / 11) * 0.6).toFixed(1);
    this.out(`[${t}] TLM vbat=${vb}V ibat=${ib}A temp=${tp}C can0=${1500 + this.n * 13}/${200 + this.n * 2} err=0`);
    if (this.n % 12 === 0) this.out(`[${t}] W: can0: tx mailbox timeout, retry 1`);
    if (this.n % 37 === 0) this.out(`[${t}] E: adc2 ch3 overrange (4095)`);
  }
  tickAux() { if (!this.hb) return; this.n++; this.out(`+HB ${String(this.n).padStart(4, '0')} vin=${(12.38 + Math.sin(this.n) * 0.03).toFixed(2)} iout=0.42 rly=0101`); if (this.n % 6 === 0) this.out('+EVT: VIN_SAG 11.82V 3ms'); }
  tickLnx() { this.n++; const t = this.up().toFixed(6).padStart(12); const m = [`[${t}] usb 1-1: new high-speed USB device number ${this.n + 2} using ci_hdrc`, `[${t}] W: can0: bus-off, restarting in 100 ms`, `[${t}] fec 30be0000.ethernet eth0: Link is Up - 100Mbps/Full`]; this.out(m[this.n % 3]); }
  tickLog() {
    this.n++; const id = (0x18FF50E5 + (this.n % 4)).toString(16).toUpperCase();
    const m = [`D/can   rx id=0x${id} dlc=8 7A 01 00 3C ${pad((this.n % 256).toString(16).toUpperCase())} 00 FF FF`, `I/sched cpu=${20 + this.n % 9}% idle=${80 - this.n % 9}% tasks=14`, `D/adc   ch0=2048 ch1=${1000 + this.n % 40} ch2=3310 ch3=${this.n % 23 ? 2711 : 4095}`, `D/can   tx id=0x0CF00400 dlc=8 F0 FF 7D 00 00 00 FF FF`];
    this.out(m[this.n % 4]); if (this.n % 29 === 0) this.out('W/nvm   write queue depth 6 (>4)'); if (this.n % 61 === 0) this.out('I/uds   session 0x01 -> 0x03 (extended)');
  }
  input(payload) {
    if (this.garbled()) return; // device can't decode at wrong baud
    if (this.profile === 'log') return; // read-only trace stream
    const ci = payload.lastIndexOf('\x03');
    if (ci >= 0) { // ^C is out-of-band: cancel running command + drop pending input, then continue with bytes after it
      this.inq += payload.slice(0, ci); this.drain(); // bytes before ^C first (in order)
      if (this.cmdT) { clearTimeout(this.cmdT); this.timers.delete(this.cmdT); this.cmdT = 0; }
      this.inq = ''; this.busy = false; this.key('\x03'); payload = payload.slice(ci + 1);
    }
    this.inq += payload; this.drain();
  }
  drain() { while (this.inq && !this.busy) { const ch = this.inq[0]; this.inq = this.inq.slice(1); this.key(ch); } }
  handle(raw) {
    const c = raw.trim(); const p = this.profile;
    if (p === 'mcu') {
      if (this.auth === 0) { if (!c) return this.out('loom-mcu login: '); this.user = c; this.auth = 1; return this.out('Password: '); }
      if (this.auth === 1) { if (!c) { this.auth = 0; this.out('Login incorrect'); return this.out('loom-mcu login: '); } this.auth = 2; this.out(`Welcome, ${this.user}. Type 'help' for commands.`); return this.out('mcu> '); }
      const [cmd0, ...a] = c.split(/\s+/);
      const R = {
        '': [], help: ['commands: status temp version log on|off reset logout help', 'OK'],
        status: [`fw      ecu-app 4.12.0+g7c1e2d9`, `uptime  ${this.up().toFixed(1)} s`, `vbat    ${(12.4 + Math.random() * 0.04).toFixed(2)} V`, 'state   RUN', `can0    rx ${1500 + this.n * 13} tx ${200 + this.n * 2} err 0`, 'OK'],
        temp: [`tmp117  ${(41 + Math.random()).toFixed(1)} C`, `mcu     ${(47.5 + Math.random()).toFixed(1)} C`, 'OK'],
        version: ['ecu-app 4.12.0+g7c1e2d9 (2026-10-28) bootloader 2.3.1', 'OK'],
      };
      if (cmd0 === 'log') { this.tlm = a[0] !== 'off'; this.out(`telemetry ${this.tlm ? 'on' : 'off'}`); this.out('OK'); return this.out('mcu> '); }
      if (cmd0 === 'reset') { this.out('Resetting...'); this.later(500, () => this.start(true)); return; }
      if (cmd0 === 'clear') { this.w('\x1b[2J\x1b[H'); return this.out('mcu> '); }
      if (cmd0 === 'logout') { this.auth = 0; return this.out('loom-mcu login: '); }
      (R[cmd0] || [`ERR unknown command '${cmd0}' (try 'help')`]).forEach(l => this.out(l)); return this.out('mcu> ');
    }
    if (p === 'aux') {
      const u = c.toUpperCase(); if (!u) return;
      if (u === 'AT') return this.out('OK');
      if (u === 'ATI') { this.out('AUX-PWR v1.4.2'); return this.out('OK'); }
      if (u === 'AT+STAT?') { this.out(`+STAT: VIN=${(12.38 + Math.random() * 0.02).toFixed(2)},IOUT=0.42,RLY=0b0101,T=33.1`); return this.out('OK'); }
      let m = u.match(/^AT\+RLY=(\d),([01])$/); if (m) { this.out(`+RLY: ${m[1]},${m[2]}`); return this.out('OK'); }
      m = u.match(/^AT\+HB=([01])$/); if (m) { this.hb = m[1] === '1'; return this.out('OK'); }
      return this.out('ERROR');
    }
    if (p === 'linux') {
      if (this.auth === 0) { if (!c) return this.out('gw-som login: '); this.user = c; this.auth = 1; return this.out('Password: '); }
      if (this.auth === 1) { if (!c) { this.auth = 0; this.out('Login incorrect'); return this.out('gw-som login: '); } this.auth = 2; this.out('Welcome to Loom GW-SoM (Yocto 5.1 scarthgap)'); return this.out(`${this.user}@gw-som:~# `); }
      const R = { '': [], 'uname -a': ['Linux gw-som 6.6.23-loom #1 SMP PREEMPT aarch64 GNU/Linux'], uptime: [` 10:42:07 up ${Math.floor(this.up() / 60)} min,  1 user,  load average: 0.08, 0.05, 0.01`],
        'ip -br a': ['lo               UNKNOWN        127.0.0.1/8', 'eth0             UP             192.168.10.20/24', 'can0             UP'], ls: ['\x1b[1;34mbin\x1b[0m  ecu-update.swu  \x1b[1;34mlogs\x1b[0m'], help: ['uname -a  uptime  ip -br a  ls  dmesg | tail -n 3  exit   (Tab: 완성)'], 'dmesg | tail -n 3': LNX_BOOT.slice(24, 27), exit: [] };
      if (c === 'exit') { this.auth = 0; return this.out('gw-som login: '); }
      (R[c] || [`-sh: ${c.split(' ')[0]}: not found`]).forEach(l => this.out(l)); return this.out(`${this.user}@gw-som:~# `);
    }
    // log stream ignores input
  }
}

/* ============================== init ============================== */
function initLayout(saved) {
  if (saved && saved.root && saved.tabs) {
    try {
      S.layout = { root: saved.root, active: saved.active, max: saved.max || null };
      // rebuild tabs; drop unknown sessions / ids
      for (const L of leaves()) {
        L.tabs = L.tabs.filter(tid => saved.tabs[tid] && sess(saved.tabs[tid].sid) && !Object.values(T).some(t => t.sid === saved.tabs[tid].sid));
        for (const tid of L.tabs) { const m = saved.tabs[tid]; newTab(m.sid, { id: tid, mode: m.mode, theme: m.theme, eol: m.eol }); sysLine(T[tid], '작업대 다시 불러옴: 레이아웃·탭·테마 복원 (이전 화면 로그는 저장하지 않음)'); }
        if (!L.tabs.includes(L.on)) L.on = L.tabs[0] || null;
      }
      if (!findNode(S.layout.active)) S.layout.active = leaves()[0].id;
      if (S.layout.max && !findNode(S.layout.max)) S.layout.max = null;
      return;
    } catch (err) { console.warn('layout restore failed, using default', err); for (const t of Object.values(T)) { t.sim && t.sim.stop(); delete T[t.id]; } }
  }
  const d = defaultLayout(); S.layout = { root: d.root, active: d.active, max: null };
  for (const [sid, lid] of d._init) { if (!sess(sid)) continue; const t = newTab(sid); const L = findNode(lid); L.tabs.push(t.id); L.on = t.id; }
}
initLayout(store.get('layout', null));
renderAll();

/* bounded public-API buffer view (<= 20 lines each) for QA assertions */
function xtLines(xt) {
  const b = xt.buffer.active; const line = y => { const l = b.getLine(y); return l ? l.translateToString(true) : ''; };
  const vis = []; for (let y = b.viewportY; y < b.viewportY + Math.min(xt.rows, 20); y++) vis.push(line(y));
  const cy = b.baseY + b.cursorY; const tail = []; for (let y = Math.max(0, cy - 19); y <= cy; y++) tail.push(line(y));
  return { viewportY: b.viewportY, baseY: b.baseY, visible: vis, tail };
}
/* Read-only inspection hook for QA (no state mutation). */
window.loomUartOpus = Object.freeze({
  snapshot() {
    const rect = el => { if (!el) return null; const r = el.getBoundingClientRect(); return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) }; };
    const tree = n => n.t === 'leaf' ? { leaf: n.id, no: paneNo(n.id), tabs: n.tabs.map(t => sess(T[t]?.sid)?.alias), on: sess(T[n.on]?.sid)?.alias || null, rect: rect(paneEl(n.id)) } : { split: n.id, dir: n.dir, r: +n.r.toFixed(4), a: tree(n.a), b: tree(n.b) };
    return JSON.parse(JSON.stringify({
      layout: tree(S.layout.root), activePane: paneNo(S.layout.active), maxPane: S.layout.max ? paneNo(S.layout.max) : null, panes: leaves().length,
      tabs: Object.values(T).map(t => ({ id: t.id, alias: sess(t.sid)?.alias, pane: paneNo(leafOfTab(t.id)?.id), conn: t.conn, mode: t.mode, theme: t.theme, rx: t.rx, tx: t.tx, lines: t.lines.length, paused: t.paused,
        xterm: t.xt ? { cols: t.xt.cols, rows: t.xt.rows, attached: !!(t.host && t.host.isConnected), opened: !!t.xopen, focused: !!(t.host && t.host.contains(document.activeElement)), bg: t.xt.options.theme && t.xt.options.theme.background, cursorX: t.xt.buffer.active.cursorX, cursorY: t.xt.buffer.active.cursorY,
          selection: t.xt.getSelection().slice(0, 500), ...xtLines(t.xt) } : null,
        displayGapBytes: t.dispGap || 0,
        rec: (t.rec || t.recLast) ? { ...(t.rec || t.recLast).info(), current: !!t.rec } : null, recState: recState(t), tsGutter: !!sess(t.sid)?.ts,
        gutter: (() => { const g = $(`.tsgutter[data-tab="${t.id}"]`); return g ? Array.from(g.children).slice(0, 20).map(d => d.textContent) : null; })(),
        deviceLine: t.sim ? { prompt: t.sim.pStr || '', buffer: t.sim.noEcho ? '***' : (t.sim.lb || ''), cursor: t.sim.lc || 0 } : null,
        run: t.run && { gid: t.run.gid, macro: t.run.name, status: t.run.status, step: t.run.i + 1, steps: t.run.steps.map(s => s.st) }, macroSends: (t.sentLog || []).filter(x => x.via.startsWith('macro')).map(x => ({ t: x.t, text: x.text })) })),
      macros: S.macros.map(m => ({ id: m.id, name: m.name, hotkey: m.hotkey, fixed: m.fixed.map(sid => sess(sid)?.alias || `미해결:${sid}`), unresolvedFixed: m.fixed.filter(sid => !sess(sid)).length, steps: m.steps.length, pinned: m.pinned, targetLabel: macroTargetLabel(m) })),
      macroScope: (() => { const R = macroRecipients({ fixed: [] }); return { mode: MS.mode, selected: MS.sel.map(sid => sess(sid)?.alias || sid), recipients: R.aliases, excluded: R.skipped.map(x => `${x.alias}:${x.why}`) }; })(),
      sessions: S.sessions.map(s => ({ id: s.id, alias: s.alias, port: s.port, baud: s.baud, frame: frame(s) })),
      broadcast: { on: BC.on, recipients: BC.ids.map(id => sess(T[id]?.sid)?.alias), excluded: BC.excluded.map(x => `${x.alias}:${x.why}`) },
      detectedPorts: uartPortsSim().ports.map(p => { const a = portAnno(p.path); return { path: p.path, product: p.product, openBy: a.openBy && a.openBy.alias, savedAs: a.savedAs.map(x => x.alias) }; }),
      storageKeys: (() => { try { return Object.keys(localStorage).filter(k => k.startsWith(LS)); } catch { return []; } })(),
    }));
  },
});
})();
