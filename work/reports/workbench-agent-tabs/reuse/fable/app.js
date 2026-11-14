/* Fable 핵심 패널 목업(개정 2) — 원본 모듈을 same-origin iframe으로 한 번만 띄우고,
   adapter CSS로 "핵심 관측면만" 남긴다(카탈로그·네비·연결 폼·매크로·전역 툴바 숨김, 원본 DOM/상태 보존).
   원칙: iframe은 frameHost에 한 번 만들고 절대 재부모화하지 않는다(재부모화=reload). 레이아웃은 slot 자리표시자, 좌표만 rAF 동기화.
   식별은 서비스가 아니라 세션/채널 단위(UART·mcu, UART·aux, UDS·BCM): 원본 frame DOM에서 읽는다. 원본 파일은 변경하지 않는다. */
(() => {
'use strict';
const $ = (s, r = document) => r.querySelector(s);
const el = (t, a = {}, ...kids) => { const e = document.createElement(t); for (const [k, v] of Object.entries(a)) { if (k === 'class') e.className = v; else if (k.startsWith('on')) e.addEventListener(k.slice(2), v); else if (v !== null && v !== undefined) e.setAttribute(k, v); } for (const k of kids) e.append(k); return e; };
const txt = (d, sel) => { const n = d && d.querySelector(sel); return n ? n.textContent.trim() : ''; };
const SHELL_OFF = `[data-shell="header"],[data-shell="status"]{display:none!important}.shell{grid-template-rows:minmax(0,1fr)!important;height:100%!important;min-height:0!important}`;

/* 재사용 원본. core: 핵심면만 남기는 adapter CSS / full: 서비스 툴바 포함(outer shell만 숨김) */
const SERVICES = [
  { id: 'uart', name: 'UART', src: '../../../uart-workbench/opus/index.html', verified: 'UART sim: 이전 동시대조 확인',
    core: SHELL_OFF + `.rail,.mtbar,.side,.backdrop,.tab-add,[data-act="macros-menu"],[data-act="settings"]{display:none!important}
.mod-uart{grid-template-columns:minmax(0,1fr)!important;grid-template-rows:minmax(0,1fr)!important;grid-template-areas:"work"!important}`,
    full: SHELL_OFF + `@media (max-width:900px){.mod-uart{grid-template-columns:44px auto minmax(0,1fr)!important;grid-template-areas:"rail tbar tbar" "rail side work"!important}.mod-uart .side{position:static!important;width:220px!important;box-shadow:none!important}.mod-uart .backdrop{display:none!important}}`,
    // 핵심면(보여야 함) / 숨긴 chrome(DOM엔 있고 display:none)
    core_visible: { '세션 탭(식별)': '.tabstrip .tab', '세션 헤더(alias·port·baud·연결·RX/TX)': '.phead', 'xterm 터미널': '.xterm', '일시정지': '[data-act="pause"]', '시각 표시': '[data-act="ts-toggle"]', '검색/필터': '[data-act="search"]', 'HEX 보기': '[data-act="mode"]', 'pane 분할/최대화': '.pane-acts' },
    core_hidden: { '모듈 툴바': '.mtbar', '세션·매크로 서랍': '#side', '레일': '.rail', '세션 추가(카탈로그)': '.tab-add', '매크로 메뉴': '[data-act="macros-menu"]', '세션 설정 폼': '[data-act="settings"]' },
    freshness: d => txt(d, '#stTarget'),
    // 세션 단위 식별: 원본 세션 탭 + 활성 pane 헤더
    sessions(d) {
      const out = [];
      for (const tab of d.querySelectorAll('.tabstrip .tab[data-tab]')) {
        const id = tab.dataset.tab, ph = d.querySelector(`.phead[data-tab="${id}"]`);
        const dot = tab.querySelector('.dot');
        const st = ph ? txt(ph, '.ph-state') : dot ? (dot.classList.contains('off') ? '닫힘' : dot.classList.contains('lost') ? '장치 없음' : dot.classList.contains('waiting') ? '재연결 대기' : '연결됨') : '';
        out.push({ key: 'uart:' + id, svc: 'uart', alias: txt(tab, '.t-label'), label: 'UART·' + txt(tab, '.t-label'), sub: (ph ? txt(ph, '.ph-port') + ' ' + txt(ph, '.ph-frame') : txt(tab, '.t-port')).trim(),
          state: st, on: st === '연결됨', ctr: ph ? txt(ph, '.ph-ctr') : '', active: tab.classList.contains('on'), tabId: id });
      }
      return out;
    },
    focus(d, s) { d.querySelector(`.tabstrip .tab[data-tab="${s.tabId}"]`)?.click(); },
    hideCss: s => `.child:has(> .pane:has(.tab[data-tab="${s.tabId}"].on)),.tree-root > .pane:has(.tab[data-tab="${s.tabId}"].on){display:none!important}`,
    badgeCss: (s, text) => `.phead[data-tab="${s.tabId}"] .ph-target::after{content:"${text.replace(/"/g, '\\"')}";font:600 10.5px ui-monospace,Menlo,monospace;color:#2f6fed;background:#e8efff;border:1px solid #b9ccff;border-radius:9px;padding:1px 7px;white-space:nowrap}`,
    freeze(d) { for (const b of d.querySelectorAll('.phead [data-act="pause"]')) b.click(); return true; },
  },
  { id: 'uds', name: 'UDS', src: '../../../uds-read-did/app/index.html', verified: 'UDS: external DoIP 이전 동시대조 확인(frame 자체는 mock route SIM)',
    core: SHELL_OFF + `.ctx-tools,#btnCfg,#btnRouteInfo,#btnSim,#cfgProv,#cfgPanel,#routeInfo,#simPanel,.tooldrawer,.didpane,#didRail,#req,[data-cmd="clear-history"]{display:none!important}
.wsp{grid-template-columns:minmax(0,1fr) var(--vals-w,250px)!important;grid-template-rows:minmax(0,1fr)!important;grid-template-areas:"center vals"!important}.wsp.vals-collapsed{--vals-w:30px}
.center{grid-template-rows:minmax(70px,var(--hist-h,46%)) 6px minmax(120px,1fr)!important}.vals{max-height:none!important}.ctx-row{padding:4px 10px!important}`,
    full: SHELL_OFF,
    core_visible: { 'ECU·경로·연결 상태(compact 헤더)': '.ctx-row', '트랜잭션 트레이스': '#txBody', '기록 범위 필터': '#segFilter', 'trace 도구': '#traceToolsMount', '응답 순서': '#seq', '해석(디코드)': '#fields', 'UDS PDU 바이트': '#hex', '확정 값': '#vals', 'hex 복사': '[data-cmd="copy-hex"]', 'JSON/CSV 내보내기': '[data-cmd="export-json"]' },
    core_hidden: { '도구 탭(서비스·ECU·DID·시퀀스·감시·프로젝트)': '#toolTabs', 'DID 카탈로그': '#didPane', '요청 편집기(DID/Raw)': '#req', '연결 설정 폼': '#btnCfg', 'SIM 옵션': '#btnSim', '주소 상세': '#btnRouteInfo' },
    freshness: d => txt(d, '#histCount') + '|' + txt(d, '#seqHead'),
    sessions(d) {
      const sel = d.querySelector('#selEcu'); if (!sel) return [];
      const ecu = sel.value || txt(sel, 'option:checked'); const ls = d.querySelector('#linkSt');
      const route = txt(d, '#segRoute [aria-checked="true"]') || txt(d, '#segRoute [aria-pressed="true"]');
      const st = ls ? (ls.classList.contains('on') ? '연결됨' : ls.classList.contains('lost') ? '연결 끊김' : '연결 안 됨') : '';
      return [{ key: 'uds:' + ecu, svc: 'uds', alias: ecu, label: 'UDS·' + ecu, sub: (route + ' ' + txt(d, '#routeSum')).trim(), state: st, on: st === '연결됨', ctr: txt(d, '#histCount'), active: true, nohide: true }];
    },
    focus(d) { d.querySelector('#txBody')?.focus(); },
    freeze: null, // 원본에 표시 일시정지 없음 — 정직하게 비활성
  },
  { id: 'eth', name: 'Ethernet', src: '../../../ethernet-workbench/opus/index.html', verified: 'ETH sim: 이전 동시대조 확인', optional: true,
    core: SHELL_OFF + `.mtbar,.chbar,#sock,[data-cmd="clear-view"]{display:none!important}.mod-eth{grid-template-rows:auto minmax(0,1fr)!important}`,
    full: SHELL_OFF,
    core_visible: { '표시 필터': '#dfInput', '범위(채널)': '#scopeSeg', '패킷 목록': '#plBody', '패킷 상세': '#tree', '바이트': '#hex' },
    core_hidden: { '모듈 툴바': '.mtbar', '채널 카드 바': '#chbar', '소켓 테스트': '#sock' },
    freshness: d => txt(d, '#dfCount'),
    sessions(d) {
      return [...d.querySelectorAll('#chbar .chcard')].map((c, i) => ({ key: 'eth:' + i, svc: 'eth', alias: txt(c, 'b') || `ch${i + 1}`, label: 'ETH·' + (txt(c, 'b') || `ch${i + 1}`), sub: '', state: /캡처|실행|run/i.test(c.textContent) ? '캡처 중' : '정지', on: /캡처 중|실행/.test(c.textContent), ctr: '', active: true, nohide: true }));
    },
    focus(d) { d.querySelector('#plBody')?.focus(); },
    freeze(d) { d.querySelector('#tbPause')?.click(); return true; },
  },
  { id: 'can', name: 'CAN', src: null, na: true,
    reason: 'CAN 프레임 트레이스의 독립 HTML 원본이 없다. 기준 UI는 ui/src/panels/can/canTrace.ts(TypeScript, ../../sdk·can.css import)로 번들/빌드 없이는 iframe으로 재사용할 수 없고, 이 작업은 패키지 설치·Cargo·원본 변경 금지라 범위에서 제외한다. 요약 표로 위장하지 않는다. CAN/PSU는 실물 미확인.' },
];
const byId = id => SERVICES.find(s => s.id === id);

const state = {
  tab: 'combined', rawShell: false,
  cols: [['uart', 'uds']],                 // 기본: UART 위 / UDS 아래 (UDS 트레이스+해석+확정 값이 전체 폭을 쓴다)
  colW: {}, slotH: {},
  shown: { uart: true, uds: true, eth: false, can: false },
  mode: { uart: 'core', uds: 'core', eth: 'core' },   // core | full (frame 단위 서비스 점프, reload 없음)
  hiddenSess: new Set(), max: null, curTask: 't1', paused: false,
  hw: { stopAt: null },
};
const frames = {};   // id → { iframe, loads, marker, lastFresh, lastRx, fpBefore, sessions:[], sessRx:{} }
const sessCache = {};

/* ---------- frames ---------- */
function ensureFrame(svc) {
  if (frames[svc.id] || svc.na) return;
  const f = el('iframe', { title: `${svc.name} 원본 모듈`, src: svc.src, 'data-svc': svc.id, 'data-hidden': '1' });
  const rec = frames[svc.id] = { iframe: f, loads: 0, marker: null, lastFresh: null, lastRx: null, sessions: [], sessRx: {} };
  f.addEventListener('load', () => {
    rec.loads++;
    try { const w = f.contentWindow; if (!w.__fableMarker) w.__fableMarker = `${svc.id}-${Date.now()}`; rec.marker = w.__fableMarker; applyAdapter(svc.id); } catch (e) { rec.err = String(e); }
    pollFrames(); renderAll();
  });
  $('#frameHost').append(f);
}
function adapterCss(svc) {
  if (state.rawShell) return '';
  let css = state.mode[svc.id] === 'full' ? svc.full : svc.core;
  if (state.mode[svc.id] === 'core') {
    for (const s of (frames[svc.id]?.sessions || [])) if (state.hiddenSess.has(s.key) && svc.hideCss) css += svc.hideCss(s);
    const t = curTask(); const tg = t && t.target;
    if (tg && tg.svc === svc.id && svc.badgeCss) { const s = (frames[svc.id]?.sessions || []).find(x => x.alias === tg.alias); if (s) css += svc.badgeCss(s, `에이전트 대상 · ${t.step + 1}/${t.steps.length} ${t.steps[Math.min(t.step, t.steps.length - 1)].text}`); }
  }
  return css;
}
function applyAdapter(id) {
  const rec = frames[id], svc = byId(id); if (!rec) return;
  try {
    const d = rec.iframe.contentDocument; if (!d || !d.head) return;
    let s = d.querySelector('style[data-fable-adapter]');
    const css = adapterCss(svc);
    if (!css) { s && s.remove(); return; }
    if (!s) { s = d.createElement('style'); s.setAttribute('data-fable-adapter', '1'); d.head.append(s); }
    if (s.textContent !== css) { s.textContent = css; rec.iframe.contentWindow.dispatchEvent(new Event('resize')); }
  } catch {}
}
function unloadFrame(id) { const r = frames[id]; if (!r) return; r.iframe.remove(); delete frames[id]; state.shown[id] = false; removeFromLayout(id); renderAll(); }

/* ---------- layout model ---------- */
function removeFromLayout(id) { state.cols = state.cols.map(c => c.filter(s => s !== id)).filter(c => c.length); if (state.max === id) state.max = null; }
function addToLayout(id, where = 'right') { removeFromLayout(id); if (where === 'right' || !state.cols.length) state.cols.push([id]); else state.cols[state.cols.length - 1].push(id); }
function visibleCols() { return state.cols.map(c => c.filter(s => state.shown[s])).filter(c => c.length); }

function renderLayout() {
  const L = $('#layout'); L.textContent = '';
  const cols = visibleCols(); const t = curTask();
  L.classList.toggle('maxed', !!state.max && cols.flat().includes(state.max));
  cols.forEach((col, ci) => {
    if (ci) L.append(mkDiv('v', ci));
    const c = el('div', { class: 'col' }); c.style.flexGrow = state.colW[ci] || 1;
    col.forEach((id, si) => {
      if (si) c.append(mkDiv('h', ci, si));
      const svc = byId(id), rec = frames[id];
      const isTarget = t && t.target && t.target.svc === id && t.state !== 'done' && t.state !== 'cancel';
      const slot = el('section', { class: 'slot' + (svc.na ? ' na' : '') + (state.max === id ? ' max' : '') + (isTarget ? ' target' : ''), 'data-slot': id, 'aria-label': `${svc.name} 패널` });
      slot.style.flexGrow = state.slotH[id] || 1;
      const sess = rec?.sessions || [];
      const idText = svc.na ? '' : sess.length ? sess.filter(s => !state.hiddenSess.has(s.key)).map(s => s.label).join(' · ') : (rec?.loads ? '세션 없음' : '문서 로딩…');
      const h = el('div', { class: 'slot-h' },
        el('span', { class: 'nm' }, svc.name),
        el('span', { class: 'ident', title: '이 패널에 보이는 세션/채널(원본 frame에서 읽음)' }, idText),
        ...(isTarget ? [el('span', { class: 'chip agent', title: '현재 에이전트 작업의 대상 패널' }, `에이전트 대상 ${t.target.alias ? '· ' + t.target.alias : ''} · 단계 ${t.step + 1}/${t.steps.length}: ${t.steps[Math.min(t.step, t.steps.length - 1)].text}`)] : []),
        el('span', { class: 'grow' }),
        ...(svc.na ? [el('span', { class: 'chip tiny' }, '범위 외')] : [
          el('button', { class: 'ibtn', title: svc.freeze ? '표시 일시정지/재개 — 원본 패널의 자체 일시정지를 누른다(수신·기록은 계속)' : '원본 UDS 화면에는 표시 일시정지가 없다(기록 범위 필터만). 정직하게 비활성', disabled: svc.freeze ? null : '', onclick: () => { const d = rec?.iframe.contentDocument; if (d && svc.freeze) { svc.freeze(d); toast(`${svc.name}: 원본 일시정지 토글`); } } }, '⏸'),
          el('button', { class: 'ibtn', title: '검사: 이 패널의 핵심면/숨김/상태 보존', onclick: () => runCheck(id) }, '🔍'),
          el('button', { class: 'btn xs' + (state.mode[id] === 'full' ? ' on' : ''), title: '서비스 점프: 같은 frame에서 원본 서비스 전체(툴바·세션·카탈로그·요청 편집기)를 펼친다. reload 없음, 상태 그대로. 다시 누르면 핵심면만', onclick: () => { state.mode[id] = state.mode[id] === 'full' ? 'core' : 'full'; applyAdapter(id); renderAll(); } }, state.mode[id] === 'full' ? '핵심면만' : '전체 서비스'),
          el('a', { class: 'ibtn', href: svc.src, target: '_blank', rel: 'noopener', title: '원본 서비스를 새 탭에서 열기(독립 문서, 이 frame과 별개)' }, '↗'),
        ]),
        el('button', { class: 'ibtn', title: '왼쪽 열로', onclick: () => moveSlot(id, -1) }, '◂'),
        el('button', { class: 'ibtn', title: '오른쪽 열로 (새 열)', onclick: () => moveSlot(id, +1) }, '▸'),
        el('button', { class: 'ibtn', title: state.max === id ? '복원' : '최대화', onclick: () => { state.max = state.max === id ? null : id; renderAll(); } }, state.max === id ? '⤡' : '⤢'),
        el('button', { class: 'ibtn', title: '숨김(상태 유지)', onclick: () => { state.shown[id] = false; if (state.max === id) state.max = null; renderAll(); } }, '×'),
      );
      const body = el('div', { class: 'slot-body' });
      if (svc.na) body.append(el('p', {}, svc.reason), el('p', {}, '기준 원본: ', el('code', {}, 'ui/src/panels/can/{canTrace.ts, traceView.ts, rxtxView.ts, txList.ts, can.css}'), ' — 제품 번들 안에서만 렌더됨. 다음 단계: CAN 트레이스 컴포넌트를 독립 mount로 노출한 뒤 같은 slot 규약(세션 칩 CAN·can0, 핵심면=프레임 트레이스)으로 삽입. 여기에는 미연결 미리보기도 두지 않는다.'));
      slot.append(h, body); c.append(slot);
    });
    L.append(c);
  });
  if (!cols.length) L.append(el('div', { class: 'slot na', style: 'border:0' }, el('div', { class: 'slot-body' }, '표시할 관측면을 상단 "패널 ▾"에서 선택하세요. 숨긴 frame은 상태를 유지한 채 대기 중입니다.')));
}
function mkDiv(kind, ci, si) {
  const d = el('div', { class: 'div ' + kind, role: 'separator', 'aria-orientation': kind === 'v' ? 'vertical' : 'horizontal', tabindex: '0' });
  d.addEventListener('pointerdown', e => {
    e.preventDefault(); d.setPointerCapture(e.pointerId);
    const prev = d.previousElementSibling, next = d.nextElementSibling;
    const r1 = prev.getBoundingClientRect(), r2 = next.getBoundingClientRect();
    const total = kind === 'v' ? r1.width + r2.width : r1.height + r2.height;
    const g1 = parseFloat(prev.style.flexGrow) || 1, g2 = parseFloat(next.style.flexGrow) || 1, gsum = g1 + g2;
    const start = kind === 'v' ? e.clientX : e.clientY, s1 = kind === 'v' ? r1.width : r1.height;
    const move = ev => {
      const cur = kind === 'v' ? ev.clientX : ev.clientY;
      const n1 = Math.min(total - 120, Math.max(120, s1 + cur - start));
      prev.style.flexGrow = gsum * n1 / total; next.style.flexGrow = gsum * (total - n1) / total;
      if (kind === 'v') { state.colW[ci - 1] = +prev.style.flexGrow; state.colW[ci] = +next.style.flexGrow; }
      else { state.slotH[prev.dataset.slot] = +prev.style.flexGrow; state.slotH[next.dataset.slot] = +next.style.flexGrow; }
    };
    const up = () => { d.removeEventListener('pointermove', move); d.removeEventListener('pointerup', up); };
    d.addEventListener('pointermove', move); d.addEventListener('pointerup', up);
  });
  return d;
}
function moveSlot(id, dir) {
  const ci = state.cols.findIndex(c => c.includes(id)); if (ci < 0) return;
  removeFromLayout(id);
  const target = ci + dir;
  if (target < 0) state.cols.unshift([id]); else if (target >= state.cols.length) state.cols.push([id]); else state.cols[target].push(id);
  renderAll();
}

/* ---------- iframe 좌표 동기화 (재부모화 없음) ---------- */
function syncFrames() {
  const host = $('#frameHost');
  for (const [id, rec] of Object.entries(frames)) {
    const body = document.querySelector(`.slot[data-slot="${id}"] .slot-body`);
    const vis = body && state.shown[id] && body.getClientRects().length;
    if (!vis) { rec.iframe.dataset.hidden = '1'; continue; }
    const r = body.getBoundingClientRect(); const hr = host.getBoundingClientRect();
    Object.assign(rec.iframe.style, { left: (r.left - hr.left) + 'px', top: (r.top - hr.top) + 'px', width: r.width + 'px', height: r.height + 'px' });
    rec.iframe.dataset.hidden = '0';
  }
  requestAnimationFrame(syncFrames);
}

/* ---------- 패널(관측면) 메뉴 ---------- */
function renderSvcList() {
  const ul = $('#svcList'); ul.textContent = '';
  for (const s of SERVICES) {
    const rec = frames[s.id];
    const li = el('li', { class: 'svc' + (s.na ? ' na' : '') });
    const cb = el('input', { type: 'checkbox', id: 'cb-' + s.id, 'aria-label': `${s.name} 표시` }); cb.checked = !!state.shown[s.id]; if (s.na) cb.disabled = true;
    cb.addEventListener('change', () => { state.shown[s.id] = cb.checked; if (cb.checked) { ensureFrame(s); if (!state.cols.flat().includes(s.id)) addToLayout(s.id, 'right'); } renderAll(); });
    li.append(cb, el('label', { class: 'nm', for: 'cb-' + s.id }, s.name + (s.optional ? ' (선택)' : '')),
      el('span', { class: 'acts' }, ...(rec ? [el('button', { class: 'ibtn', title: 'frame unload(상태 소멸)', onclick: () => unloadFrame(s.id) }, '⏏')] : [])),
      el('span', { class: 'st' }, s.na ? '원본 없음 · 범위 외 · 실물 미확인' : rec ? (rec.loads ? `로드 ${rec.loads}회 · ${state.shown[s.id] ? '표시' : '숨김(상태 유지)'} · ${state.mode[s.id] === 'core' ? '핵심면' : '전체 서비스'}` : '문서 로딩…') : '미생성'));
    ul.append(li);
  }
}

/* ---------- 세션/채널 칩 (세션 단위 식별·표시·포커스) ---------- */
function renderSessions() {
  const o = $('#sessStrip'); o.textContent = '';
  const t = curTask();
  let any = false;
  for (const s of SERVICES) {
    const rec = frames[s.id]; if (!rec || !rec.loads) continue;
    for (const se of rec.sessions) {
      any = true;
      const hidden = state.hiddenSess.has(se.key);
      const isTarget = t && t.target && t.target.svc === s.id && (!t.target.alias || t.target.alias === se.alias) && t.state !== 'done' && t.state !== 'cancel';
      const rx = rec.sessRx[se.key]; const age = rx ? (Date.now() - rx) / 1000 : null;
      const chip = el('button', { class: 'sess' + (se.on ? ' on' : '') + (hidden ? ' hid' : '') + (isTarget ? ' target' : '') + (state.shown[s.id] ? '' : ' off'), title: `${se.label} — ${se.sub}\n${se.state} ${se.ctr}\n클릭: 해당 세션/패널로 포커스 · ×/👁: 표시/숨김(상태 유지)`,
        onclick: () => focusSession(s, se) },
        el('span', { class: 'dot' }), el('b', {}, se.label), el('span', { class: 'sub' }, se.sub), el('span', { class: 'st' }, se.state),
        el('span', { class: 'fresh', title: '마지막 화면 변화(원본 DOM 카운터, 1초 폴링) — 엔진 텔레메트리 아님' }, age === null ? '' : age < 1 ? '지금' : age < 60 ? `${age.toFixed(0)}s` : `${(age / 60).toFixed(0)}m`),
        ...(isTarget ? [el('span', { class: 'chip agent tiny' }, '에이전트 대상')] : []));
      const wrap = el('span', { class: 'sess-wrap' }, chip);
      if (!se.nohide) wrap.append(el('button', { class: 'ibtn tiny', title: hidden ? '이 세션 패널 다시 표시' : '이 세션 패널 숨김(원본 세션·로그 상태 유지)', onclick: () => { hidden ? state.hiddenSess.delete(se.key) : state.hiddenSess.add(se.key); applyAdapter(s.id); renderAll(); } }, hidden ? '👁' : '×'));
      o.append(wrap);
    }
  }
  if (!any) o.append(el('span', { class: 'note' }, '세션/채널을 원본 frame에서 읽는 중…'));
  o.append(el('span', { class: 'grow' }), el('span', { class: 'note' }, '세션 칩 = 원본 frame의 세션 탭/ECU/채널에서 1초마다 읽음 · 클릭=포커스 · 세션 추가/설정은 "전체 서비스" 또는 개별 서비스 탭에서'));
}
function focusSession(svc, se) {
  if (!state.shown[svc.id]) { state.shown[svc.id] = true; if (!state.cols.flat().includes(svc.id)) addToLayout(svc.id, 'right'); }
  if (state.hiddenSess.has(se.key)) { state.hiddenSess.delete(se.key); applyAdapter(svc.id); }
  renderAll();
  const d = frames[svc.id]?.iframe.contentDocument; try { svc.focus(d, se); } catch {}
  document.querySelector(`.slot[data-slot="${svc.id}"]`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

/* ---------- 에이전트 작업 (목업 데이터 + 로컬 SIM 시연) ---------- */
const TASKS = [
  { id: 't1', title: 'BCM VIN(F190) 읽고 UART mcu 부팅 로그와 대조', state: 'run', step: 1, target: { svc: 'uds', alias: 'BCM' },
    steps: [
      { text: 'UDS·BCM 경로 DoIP 연결', run: 'uds-link' },
      { text: 'UDS·BCM 0x22 F190 읽기 → 트랜잭션/해석 확인', run: 'uds-read', did: 'F190', target: { svc: 'uds', alias: 'BCM' } },
      { text: 'UART·mcu 포트 열기, 로그인 프롬프트 대기', run: 'uart-connect', alias: 'mcu', target: { svc: 'uart', alias: 'mcu' }, wait: 'UART·mcu 로그인 프롬프트 수신 대기(사용자가 직접 입력해도 됨)' },
      { text: 'UART·mcu cat /etc/vin 결과와 UDS 값 비교', target: { svc: 'uart', alias: 'mcu' } },
      { text: '결과 기록' }],
    evidence: [['UDS·BCM 트랜잭션/해석', 'uds', 'BCM'], ['UART·mcu 터미널', 'uart', 'mcu']], err: null },
  { id: 't2', title: 'DoIP 7000 포트 패킷 캡처 필터 검증', state: 'wait', step: 0, target: { svc: 'eth' },
    steps: [{ text: 'ETH 채널(SIM) 캡처 시작 — Ethernet 패널 표시 필요', wait: 'Ethernet 패널이 숨김 상태 — 패널 ▾에서 표시' }, { text: '표시 필터 doip || tcp.port == 7000 적용 → 건수 확인' }, { text: '선택 패킷 상세·hex 캡처' }],
    evidence: [['Ethernet 패킷 목록/필터', 'eth']], err: null },
  { id: 't3', title: 'GW DID F187 읽기', state: 'fail', step: 1, target: { svc: 'uds', alias: 'GW' },
    steps: [{ text: 'UDS·GW 경로 CAN-TP 연결' }, { text: 'UDS·GW 0x22 F187 읽기' }], evidence: [['UDS 트랜잭션 (NRC)', 'uds', 'GW']], err: 'NRC 0x31 requestOutOfRange — 재시도 안 함, 사용자 확인 필요' },
  { id: 't4', title: 'UART aux 포트 상태 확인', state: 'done', step: 2, target: { svc: 'uart', alias: 'aux' },
    steps: [{ text: 'UART·aux 포트 열기' }, { text: 'status 명령 → 응답 확인' }, { text: '결과 기록' }], evidence: [['UART·aux 터미널', 'uart', 'aux']], err: null },
];
const TIMELINE = [{ t: Date.now() - 95e3, text: '[목업] t4 완료: UART·aux status 응답 확인' }, { t: Date.now() - 61e3, text: '[목업] t3 실패: UDS·GW F187 NRC 0x31 — 사용자 확인 대기' }, { t: Date.now() - 20e3, text: '[목업] t1 단계 1 완료: UDS·BCM DoIP 연결(목업 기록, 실제 frame 상태와 다를 수 있음)' }];
const STATE_LABEL = { run: '진행', wait: '대기', done: '완료', fail: '실패', cancel: '취소', pause: '일시정지' };
function curTask() { return TASKS.find(x => x.id === state.curTask); }
function curStep(t) { return t.steps[Math.min(t.step, t.steps.length - 1)]; }
function log(text) { TIMELINE.push({ t: Date.now(), text }); renderTimeline(); }

/* 시연: 현재 단계를 원본 SIM 패널의 자체 입력으로 수행(프로토타입 고정 절차, 실제 에이전트/엔진 아님) */
function demoStep() {
  const t = curTask(); if (!t || t.state === 'done' || t.state === 'cancel' || t.state === 'fail') return toast('이 작업은 시연 대상이 아님');
  if (state.paused) return toast('작업 일시정지 중 — 재개 후 시연');
  const st = curStep(t); const svc = st.run ? byId(st.run.split('-')[0]) : null;
  const d = svc && frames[svc.id]?.iframe.contentDocument;
  if (st.run && !d) { t.state = 'wait'; log(`[시연] t1 단계 ${t.step + 1} 대기: ${svc.name} 패널 미표시`); renderAll(); return; }
  try {
    if (st.run === 'uds-link') { const ls = d.querySelector('#linkSt'); if (ls && !ls.classList.contains('on')) d.querySelector('#btnLink')?.click(); log(`[시연] UDS·${d.querySelector('#selEcu')?.value} [연결] 눌림 (원본 SIM)`); }
    else if (st.run === 'uds-read') {
      const ls = d.querySelector('#linkSt'); if (ls && !ls.classList.contains('on')) { d.querySelector('#btnLink')?.click(); }
      const inp = d.querySelector('#didInput'); if (inp) { inp.value = st.did; inp.dispatchEvent(new Event('input', { bubbles: true })); }
      d.querySelector('#req')?.requestSubmit(); log(`[시연] UDS 0x22 ${st.did} 요청을 원본 요청 편집기로 전송 (SIM 응답은 트랜잭션 패널에)`);
    }
    else if (st.run === 'uart-connect') {
      const se = (frames.uart?.sessions || []).find(x => x.alias === st.alias);
      if (se) { d.querySelector(`.tabstrip .tab[data-tab="${se.tabId}"]`)?.click(); const b = d.querySelector(`.phead[data-tab="${se.tabId}"] [data-act="connect"]`); if (b) b.click(); log(`[시연] UART·${st.alias} ${b ? '포트 열기 눌림' : '이미 연결됨'} (원본 SIM)`); }
      else log(`[시연] UART·${st.alias} 세션 탭을 frame에서 찾지 못함`);
    } else log(`[시연] 단계 "${st.text}"는 자동 절차 없음 — 사용자/에이전트 판단 단계로 표기`);
  } catch (e) { log('[시연] 실패: ' + e); }
  t.step = Math.min(t.step + 1, t.steps.length - 1);
  const ns = curStep(t); t.state = ns.wait ? 'wait' : (t.step >= t.steps.length - 1 ? 'done' : 'run');
  if (t.state === 'done') log(`[시연] ${t.id} 완료 표기(목업)`);
  if (ns.target) t.target = ns.target;
  for (const id of Object.keys(frames)) applyAdapter(id);
  renderAll();
}

function renderAgentStrip() {
  const o = $('#agentStrip'); o.textContent = '';
  const t = curTask(); if (!t) return;
  const st = curStep(t); const stLabel = state.paused ? 'pause' : t.state;
  const lastEv = TIMELINE[TIMELINE.length - 1];
  const tgtLabel = t.target ? `${byId(t.target.svc).name}${t.target.alias ? '·' + t.target.alias : ''}` : '—';
  o.append(
    el('span', { class: 'k' }, '대상'), el('button', { class: 'val tgt', title: '대상 패널로 포커스', onclick: () => { const r = frames[t.target.svc]; const se = r?.sessions.find(x => !t.target.alias || x.alias === t.target.alias); if (se) focusSession(byId(t.target.svc), se); else focusSvc(t.target.svc); } }, tgtLabel),
    el('span', { class: 'k' }, '작업'), el('span', { class: 'val' }, t.title),
    el('span', { class: 'k' }, `단계 ${t.step + 1}/${t.steps.length}`), el('span', { class: 'val step' }, st.text),
    el('span', { class: 'chip st ' + stLabel }, STATE_LABEL[stLabel] + (t.state === 'wait' && st.wait ? ' · ' + st.wait : '') + (t.err ? ' · ' + t.err : '')),
    el('span', { class: 'k' }, '근거'), ...t.evidence.flatMap(([lab, id, alias], i) => [el('a', { class: 'ev', onclick: () => { const r = frames[id]; const se = r?.sessions.find(x => x.alias === alias); se ? focusSession(byId(id), se) : focusSvc(id); } }, lab)]),
    el('span', { class: 'k' }, '최근'), el('span', { class: 'val time', title: lastEv ? lastEv.text : '' }, lastEv ? `${new Date(lastEv.t).toLocaleTimeString()} ${lastEv.text.slice(0, 60)}` : '—'),
    el('span', { class: 'grow' }),
    el('button', { class: 'btn xs', title: '에이전트 작업 일시정지/재개 — 장치·패널에는 영향 없음', 'aria-pressed': String(state.paused), onclick: () => { state.paused = !state.paused; log(state.paused ? '[사용자] 작업 일시정지' : '[사용자] 작업 재개'); renderAll(); } }, state.paused ? '▶ 재개' : '⏸ 작업 일시정지'),
    el('button', { class: 'btn xs', title: '작업 취소: 에이전트 단계만 중단. 장치 출력은 하드웨어 STOP', disabled: (t.state === 'run' || t.state === 'wait') ? null : '', onclick: () => { t.state = 'cancel'; log(`[사용자] ${t.id} 작업 취소 — 장치/frame 그대로`); renderAll(); } }, '✕ 작업 취소'),
    el('button', { class: 'btn xs demo', title: '프로토타입 시연: 현재 단계를 원본 SIM 패널의 자체 입력으로 수행해 본다(실제 에이전트/엔진 아님)', onclick: demoStep }, '▶ 시연 1단계(로컬 SIM)'),
  );
}
function focusSvc(id) { const svc = byId(id); if (!svc || svc.na) return; if (!state.shown[id]) { state.shown[id] = true; ensureFrame(svc); if (!state.cols.flat().includes(id)) addToLayout(id, 'right'); } renderAll(); document.querySelector(`.slot[data-slot="${id}"]`)?.scrollIntoView({ block: 'nearest' }); }

function renderTasks() {
  const ul = $('#taskList'); ul.textContent = '';
  for (const t of TASKS) {
    ul.append(el('li', { class: 'task', 'aria-current': String(state.curTask === t.id), tabindex: '0', onclick: () => { state.curTask = t.id; for (const id of Object.keys(frames)) applyAdapter(id); renderAll(); } },
      el('span', { class: 't' }, t.title), el('span', { class: 's' }, `${t.target ? byId(t.target.svc).name + (t.target.alias ? '·' + t.target.alias : '') : ''} · ${t.step + 1}/${t.steps.length} ${curStep(t).text}`), el('span', { class: 'st ' + t.state }, STATE_LABEL[t.state])));
  }
  const t = curTask(); const cur = $('#taskCur'); cur.textContent = '';
  if (!t) return;
  cur.append(el('b', {}, '단계'), el('ol', {}, ...t.steps.map((s, i) => el('li', { class: i < t.step ? 'done' : i === t.step ? 'cur' : '' }, s.text + (s.wait && i === t.step ? ` — 대기: ${s.wait}` : '')))));
  if (t.err) cur.append(el('div', { class: 'ev err' }, '오류: ' + t.err));
  cur.append(el('div', { class: 'ev' }, '근거: ', ...t.evidence.flatMap(([lab, id, alias], i) => [i ? ' · ' : '', el('a', { onclick: () => { const se = frames[id]?.sessions.find(x => x.alias === alias); se ? focusSession(byId(id), se) : focusSvc(id); } }, lab)])));
  renderTimeline();
}
function renderTimeline() {
  const ol = $('#timeline'); if (!ol) return; ol.textContent = '';
  for (const e of TIMELINE.slice(-30).reverse()) ol.append(el('li', {}, el('time', {}, new Date(e.t).toLocaleTimeString()), ' ', e.text));
}

/* ---------- 폴링: 세션 식별 + 자료 신선도 (작업 단계와 분리) ---------- */
function pollFrames() {
  let changed = false;
  for (const s of SERVICES) {
    const rec = frames[s.id]; if (!rec || !rec.loads) continue;
    try {
      const d = rec.iframe.contentDocument;
      const f = String(s.freshness(d)); if (f !== rec.lastFresh) { rec.lastFresh = f; rec.lastRx = Date.now(); }
      const sess = s.sessions(d);
      for (const se of sess) { const k = se.key + '|' + se.ctr + '|' + se.state; if (sessCache[se.key] !== k) { sessCache[se.key] = k; rec.sessRx[se.key] = Date.now(); } }
      const sig = JSON.stringify(sess.map(x => [x.key, x.label, x.active])); if (sig !== rec.sessSig) { rec.sessSig = sig; changed = true; }
      rec.sessions = sess;
    } catch {}
    applyAdapter(s.id);
  }
  renderSessions(); renderAgentStrip(); if (changed) renderLayout();
}

/* ---------- 검사 ---------- */
function runCheck(only) {
  const b = $('#checkBody'); b.textContent = '';
  for (const s of SERVICES) {
    if (only && s.id !== only) continue;
    const card = el('div', { class: 'ck' }); card.append(el('b', {}, s.name));
    if (s.na) { card.append(el('span', { class: 'na' }, '원본 HTML 없음 — 검사 대상 아님(정직 표기)')); b.append(card); continue; }
    const rec = frames[s.id];
    if (!rec || !rec.loads) { card.append(el('span', { class: 'na' }, 'frame 미생성/미로드')); b.append(card); continue; }
    const ul = el('ul');
    let d; try { d = rec.iframe.contentDocument; } catch (e) { ul.append(el('li', { class: 'bad' }, 'same-origin 접근 실패: ' + e)); }
    if (d) {
      const vis = n => n && getComputedStyle(n).display !== 'none' && n.getClientRects().length > 0;
      const core = state.mode[s.id] === 'core' && !state.rawShell;
      ul.append(el('li', {}, `모드: ${state.rawShell ? '원본 shell(adapter 없음)' : state.mode[s.id] === 'core' ? '핵심면' : '전체 서비스'} · 세션 ${rec.sessions.map(x => x.label).join(', ') || '없음'}`));
      for (const [lab, sel] of Object.entries(s.core_visible)) { const n = d.querySelector(sel); const ok = n && (vis(n) || !state.shown[s.id]); ul.append(el('li', { class: ok ? 'ok' : 'bad' }, `핵심면 ${lab} ${n ? (ok ? '표시 ✓' : '있으나 숨김 ✗') : '없음 ✗'} ${sel}`)); }
      for (const [lab, sel] of Object.entries(s.core_hidden)) { const n = d.querySelector(sel); const hid = n && !vis(n); ul.append(el('li', { class: !n ? 'bad' : core ? (hid ? 'ok' : 'bad') : (hid ? 'bad' : 'ok') }, `비핵심 ${lab} ${!n ? 'DOM 없음 ✗' : hid ? 'DOM 보존·숨김' : '표시'} ${sel}`)); }
      const markerOk = rec.iframe.contentWindow.__fableMarker === rec.marker;
      ul.append(el('li', { class: rec.loads === 1 && markerOk ? 'ok' : 'bad' }, `로드 ${rec.loads}회 · window 마커 ${markerOk ? '보존' : '소실'} (탭 전환·분할·최대화·핵심/전체 전환 후에도 1회여야 함)`));
      const fp = s.freshness(d); if (rec.fpBefore !== undefined) ul.append(el('li', { class: 'ok' }, `상태 지문(탭 전환 전): "${rec.fpBefore}" → 지금 "${fp}" (같거나 SIM 수신으로 증가해야 함)`));
      rec.fpBefore = fp;
    }
    card.append(ul); b.append(card);
  }
  $('#checkPanel').hidden = false; $('#btnCheck').setAttribute('aria-expanded', 'true');
}

/* ---------- 탭 / 공통 ---------- */
function setTab(tab) {
  for (const s of SERVICES) { const r = frames[s.id]; if (r && r.loads) try { r.fpBefore = s.freshness(r.iframe.contentDocument); } catch {} }
  state.tab = tab; $('#app').dataset.tab = tab;
  for (const b of document.querySelectorAll('.tabs [role=tab]')) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
  $('#sideAgent').hidden = tab !== 'agent';
  renderStatus();
}
function toast(msg) { const t = el('div', { class: 'chip warn toast' }, msg); document.body.append(t); setTimeout(() => t.remove(), 3200); }
function renderStatus() {
  const n = Object.values(frames).filter(r => r.loads).length, shown = SERVICES.filter(s => state.shown[s.id] && !s.na).length;
  $('#stLeft').textContent = `${state.tab === 'combined' ? '통합 관측' : '에이전트 작업'} · frame ${n}개 로드 / ${shown}개 표시 · 숨긴 frame/세션 상태 유지 · adapter ${state.rawShell ? 'off' : 'on(핵심면)'}` + (state.hw.stopAt ? ` · 하드웨어 STOP 시뮬 ${new Date(state.hw.stopAt).toLocaleTimeString()}` : '');
}
function renderAll() { renderLayout(); renderSvcList(); renderSessions(); renderAgentStrip(); renderTasks(); renderStatus(); }

document.addEventListener('click', e => {
  const b = e.target.closest('[data-cmd]'); if (!b) return;
  const cmd = b.dataset.cmd;
  if (cmd === 'preset') {
    const ids = SERVICES.filter(s => state.shown[s.id] && !s.na).map(s => s.id);
    if (b.dataset.preset === 'row') state.cols = ids.map(i => [i]);
    else if (b.dataset.preset === 'col') state.cols = ids.length ? [ids] : [];
    else { state.shown.uart = state.shown.uds = state.shown.eth = true; for (const id of ['uart', 'uds', 'eth']) ensureFrame(byId(id)); state.cols = [['uart'], ['uds', 'eth']]; }
    state.max = null; renderAll();
  }
  if (cmd === 'run-check') runCheck();
  if (cmd === 'close-check') { $('#checkPanel').hidden = true; $('#btnCheck').setAttribute('aria-expanded', 'false'); }
});
$('#tabCombined').onclick = () => setTab('combined'); $('#tabAgent').onclick = () => setTab('agent');
$('#btnCheck').onclick = () => $('#checkPanel').hidden ? runCheck() : ($('#checkPanel').hidden = true, $('#btnCheck').setAttribute('aria-expanded', 'false'));
$('#btnShell').onclick = () => { state.rawShell = !state.rawShell; $('#btnShell').setAttribute('aria-pressed', String(state.rawShell)); for (const id of Object.keys(frames)) applyAdapter(id); renderStatus(); };
$('#btnHwStop').onclick = () => { state.hw.stopAt = Date.now(); log('[사용자] 하드웨어 STOP — 엔진 미연결, 명령 전송 시뮬레이션만'); toast('하드웨어 STOP: 엔진 미연결 — 명령 전송은 시뮬레이션만. 작업 취소와는 별개.'); renderAll(); };
document.addEventListener('keydown', e => { if (e.key === '?' && !e.target.closest('input,textarea')) runCheck(); });
document.addEventListener('click', e => { const m = $('#panelMenu'); if (m.open && !m.contains(e.target)) m.open = false; });

/* init */
for (const s of SERVICES) if (state.shown[s.id]) ensureFrame(s);
renderAll(); setTab('combined'); requestAnimationFrame(syncFrames); setInterval(pollFrames, 1000);
})();
