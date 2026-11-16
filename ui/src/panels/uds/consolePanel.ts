// uds.console — Bench Light UDS workbench. Exact port of the selected web reference
// work/reports/uds-read-did/app (index.html / styles.css / app.js / config.js / services.js /
// catalog.js / workflow.js / project.js) onto the real daemon data path (uds.* rows + uds.state).
//
//  .ctxbar   ctx-row  : ECU · 경로 (DoIP; CAN-TP/EthDiag disabled — backend unsupported) · route-sum ·
//                       preset chip · link state (uds.state only) · [연결] (uds.connect) · ›연결 설정 · ›주소 상세
//            ctx-tools: connection overview · session chip · 서비스 / ECU·DID / 명령·시퀀스 / 감시 / 프로젝트
//  .wsp      DID 목록 | request composer (DID / Raw) + transaction history + 응답 순서 | 해석 | 바이트 | 확정 값
//
// Nothing here is simulated: rows, link/session state and decoded values come from the daemon.
// Presets and local edits never imply a live connection. [연결 해제] (uds.disconnect) closes a link on
// purpose and [목록에서 제거] (uds.forget) removes a closed target; there is no polling RPC. Selection / template / catalogue clicks never send; sending is [읽기]/[전송],
// Enter in the DID field or Ctrl/⌘+Enter in Raw, and R2+ previews need a second press (F1: the daemon
// policy decides). Single-letter keys (T, !) only when the panel has focus and no input field does.

import './uds.css';
import {
  createLatchLine,
  fmtWall,
  isFinal,
  mergeGaps,
  registerPanel,
  stageCell,
  VirtualTable,
  type Cell,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type RowItem,
  type RowLike,
} from '../../sdk';
import { divider, el, icon } from '../../workbench/eth/bench';
import {
  buildPdu,
  defaultValues,
  detailPairs,
  didSidText,
  dtcStatusFlags,
  dtcStatusTitle,
  emptyTarget,
  filterCatalog,
  filterDtcs,
  findTemplate,
  hexRowWidth,
  isDenied,
  isForgotten,
  isTesterPresent,
  keyTemplate,
  latestDtcs,
  leaseText,
  linkDownMessage,
  needsSecondEnter,
  normalizeHex,
  paramIssues,
  parsePdu,
  pduFields,
  previewText,
  reduceTarget,
  reduceTargetState,
  reqHexOf,
  respHexOf,
  resultSymbol,
  rowDid,
  rowRoute,
  seedFromRow,
  seqEvents,
  SERVICE_GROUPS,
  spacedHex,
  statusClass,
  targetCells,
  targetLine,
  templateLabel,
  timingText,
  tpCellText,
  updateLastValues,
  type DtcFilter,
  type LastValue,
  type ParamValues,
  type SeqEvent,
  type ServiceTemplate,
  type TargetView,
} from './udsModel';
import {
  checkDbText,
  connectParams,
  DEFAULT_TP_INTERVAL_MS,
  loadPrefs,
  MAX_DB_TEXT_BYTES,
  MAX_SAVED_DBS,
  normalizeDid,
  normalizeRaw,
  presetIssues,
  profileDraftForTarget,
  profileNeedsReconnect,
  savePrefs,
  tpIntervalOf,
  type ConnPreset,
  type SavedDb,
  type UdsPrefs,
} from './udsPrefs';
import { createTools, type ToolsApi } from './udsTools';

export const PREVIEW_DEBOUNCE_MS = 150;
const SECOND_ENTER_MS = 3_000;
const MAX_TARGETS = 32; // C2: target entries (over → ignored + counted)
let instanceSeq = 0;

function button(text: string, cls = 'btn', title?: string): HTMLButtonElement {
  const b = el('button', cls, text);
  b.type = 'button';
  if (title) b.title = title;
  return b;
}
function ibtn(name: string, title: string, small?: string, cls = 'ibtn'): HTMLButtonElement {
  const b = button('', cls, title);
  b.setAttribute('aria-label', title);
  b.append(icon(name));
  if (small) b.append(el('small', undefined, small));
  return b;
}
/** reference `.seg` radio group */
function segGroup<T extends string>(cls: string, aria: string, items: { v: T; label: string; title?: string; disabled?: boolean; note?: string }[], onPick: (v: T) => void) {
  const root = el('span', `seg ${cls}`.trim());
  root.setAttribute('role', 'radiogroup');
  root.setAttribute('aria-label', aria);
  const btns = new Map<T, HTMLButtonElement>();
  for (const it of items) {
    const b = button(it.label, '', it.title);
    b.setAttribute('role', 'radio');
    b.dataset.v = it.v;
    b.disabled = it.disabled === true;
    if (it.note) b.append(el('small', undefined, it.note));
    b.addEventListener('click', () => !b.disabled && onPick(it.v));
    btns.set(it.v, b);
    root.append(b);
  }
  root.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const en = items.filter((i) => !btns.get(i.v)!.disabled);
    const cur = en.findIndex((i) => btns.get(i.v)!.getAttribute('aria-checked') === 'true');
    const nx = en[(cur + (e.key === 'ArrowRight' ? 1 : en.length - 1)) % en.length];
    if (!nx) return;
    e.preventDefault();
    onPick(nx.v);
    btns.get(nx.v)!.focus();
  });
  const set = (v: T) => {
    for (const [k, b] of btns) {
      b.setAttribute('aria-checked', String(k === v));
      b.tabIndex = k === v ? 0 : -1;
    }
  };
  return { root, set, btns };
}
function colHead(text: string): HTMLSpanElement {
  const c = el('span', undefined, text);
  c.setAttribute('role', 'columnheader');
  return c;
}
const str = (v: unknown): string => (typeof v === 'string' ? v : v === undefined || v === null ? '' : String(v));
const rowTarget = (r: RowLike): string => str(r.payload.target) || (r.channel ?? '');
const fmtClock = (ms: number) => fmtWall(ms);

function download(name: string, text: string, type: string): boolean {
  try {
    if (typeof URL.createObjectURL !== 'function') return false;
    const url = URL.createObjectURL(new Blob([text], { type }));
    const a = el('a');
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1_000);
    return true;
  } catch {
    return false;
  }
}

export function createUdsConsole(ctx: PanelContext): PanelInstance {
  const uid = `uds${++instanceSeq}`;
  const mirror = ctx.store('uds');
  // dry-run previews are not transactions: never shown as rows (uds-ui U3)
  const rows = ctx.rows({ domain: 'uds', where: (r) => !isTesterPresent(r) && r.payload.dry_run !== true });
  const tpRows = ctx.rows({ domain: 'uds', where: isTesterPresent });
  const targets = new Map<string, TargetView>();
  /** seq of the newest uds.state applied per target (live control, backlog or snapshot high-water) */
  const targetSeq = new Map<string, bigint>();
  let targetsOverflow = 0;
  const lastValues = new Map<string, LastValue>();
  let lastSeq = -1n;
  let snapshotVersion = 0;
  /** one-shot bound-panel hydration from uds.status (see hydrateFromStatus) */
  let statusHydration: 'idle' | 'asked' | 'done' = 'idle';
  let disposed = false;
  let rowsVersionSeen = -1;
  let rowsVersion = 0;
  const targetScope = typeof ctx.props.targets === 'string' ? new Set(ctx.props.targets.split(',')) : null;
  const initialTarget = typeof ctx.props.targets === 'string' ? ctx.props.targets.split(',')[0] : undefined;
  let selectedTarget: string | null = initialTarget ?? null;

  let prefs: UdsPrefs = loadPrefs();
  const setPrefs = (next: UdsPrefs) => {
    prefs = next;
    savePrefs(prefs);
    uiKey = '';
    ctx.scheduler.invalidate();
  };

  // ---- per-request projections (first sight; rows never carry the route) ----
  const rowNo = new Map<string, number>();
  let nextNo = 1;
  const hidden = new Set<string>(); // 기록 지우기: local display only, daemon history stays
  let histScope: 'all' | 'ecu' = prefs.histScope;

  const target = () => selectedTarget ?? [...targets.keys()][0] ?? prefs.presets[0]?.ecu ?? null;
  const linkOf = (t: string | null) => (t ? (targets.get(t)?.link ?? '?') : '?');
  /** link of the selected target: reported and not up (down, connecting, lost…) → read-only. Unreported → gate/daemon decide. */
  const linkAllowsWrite = () => {
    const l = linkOf(target());
    return l === '?' || l.toLowerCase() === 'up';
  };
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  /** Unbound module application (no `targets` prop — the UDS service tab before any connection): a preset-only
   *  alias the daemon has never reported in uds.state is not a target yet. Composing/editing stays available;
   *  sending waits for an explicit [연결]. Scoped panels (`targets` set) keep the gate/daemon-decides rule. */
  const needsConnect = () => {
    const t = target();
    return !targetScope && t !== null && !targets.has(t);
  };
  /** request composer / templates / params editable (no send) */
  const editable = () => canOperate() && linkAllowsWrite();
  const writable = () => editable() && !needsConnect();
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });
  const presetOf = (ecu: string | null): ConnPreset | null => (ecu ? (prefs.presets.find((p) => p.ecu === ecu) ?? null) : null);

  // =============================== DOM ===============================
  const root = el('div', 'uds-panel uds-wb');
  root.tabIndex = -1;
  const mod = el('main', 'mod-uds');
  mod.dataset.module = 'uds';

  // ---- context strip ----
  const ctxbar = el('section', 'ctxbar');
  ctxbar.setAttribute('aria-label', '진단 대상과 경로');
  const ctxRow = el('div', 'ctx-row');
  const ecuLab = el('label', 'flab', 'ECU');
  const ecuSel = el('select', 'fld ecu-sel');
  ecuSel.id = `${uid}-ecu`;
  ecuLab.htmlFor = ecuSel.id;
  ecuSel.setAttribute('aria-label', '대상');
  const ecuName = el('span', 'ecu-name');
  const routeLab = el('span', 'flab', '경로');
  routeLab.id = `${uid}-rlab`;
  const routeSeg = segGroup('route-seg', '경로', [
    { v: 'doip', label: 'DoIP', title: 'DoIP (ISO 13400) — daemon 지원 (실제 엔티티 또는 내장 SIM bench ECU)' },
    { v: 'cantp', label: 'CAN-TP', disabled: true, note: '미지원', title: 'CAN-TP (ISO 15765-2) — 백엔드 미지원: Loom UDS daemon에 CAN-TP 전송이 없습니다' },
    { v: 'ethdiag', label: 'EthDiag', disabled: true, note: '미지원', title: '벤더 Ethernet 진단 어댑터 — 백엔드 미지원' },
  ], () => {});
  routeSeg.root.setAttribute('aria-labelledby', routeLab.id);
  routeSeg.set('doip');
  const routeSum = el('span', 'route-sum');
  routeSum.title = '경로 주소 요약 — 상세는 ‘주소 상세’';
  const cfgProv = el('span', 'cfg-prov');
  const linkSt = el('span', 'link-st');
  linkSt.setAttribute('aria-live', 'polite');
  const connectBtn = button('', 'btn');
  connectBtn.dataset.cmd = 'connect';
  const connectLabel = el('span', undefined, '연결');
  connectBtn.append(icon('link'), connectLabel);
  const cfgBtn = button('', 'btn ghost disc');
  cfgBtn.append(icon('chev'), '연결 설정');
  const cfgDirty = el('span', 'dirty-dot');
  cfgDirty.title = '저장 안 된 초안';
  cfgDirty.hidden = true;
  cfgBtn.append(cfgDirty);
  const riBtn = button('', 'btn ghost disc');
  riBtn.append(icon('chev'), '주소 상세');
  // U6: close on purpose / remove a closed target / profile differs from the live link
  const discBtn = button('', 'btn ghost');
  discBtn.dataset.cmd = 'disconnect';
  const discLabel = el('span', undefined, '연결 해제');
  discBtn.append(icon('x'), discLabel);
  const forgetBtn = button('목록에서 제거', 'btn sm ghost', '닫힌 대상을 daemon 목록에서 제거 (uds.forget) — 기록은 남음');
  forgetBtn.dataset.cmd = 'forget';
  forgetBtn.hidden = true;
  const reconnNote = el('span', 'reconn-note', '재연결 필요');
  reconnNote.setAttribute('data-testid', 'uds-reconnect-needed');
  reconnNote.hidden = true;
  ctxRow.append(ecuLab, ecuSel, ecuName, el('span', 'vdiv'), routeLab, routeSeg.root, routeSum, cfgProv, linkSt, connectBtn, discBtn, forgetBtn, reconnNote, el('span', 'grow'), cfgBtn, riBtn);

  const ctxTools = el('div', 'ctx-tools');
  const connOv = el('span', 'conn-ov');
  connOv.setAttribute('aria-label', '활성 연결 개요');
  const sessChip = el('span', 'sess-chip');
  sessChip.setAttribute('aria-live', 'polite');
  const toolTabs = el('div', 'tooltabs');
  toolTabs.setAttribute('role', 'tablist');
  toolTabs.setAttribute('aria-label', '도구 서랍');
  type ToolId = 'services' | 'catalog' | 'workflow' | 'watch' | 'project';
  const TOOLS: [ToolId, string][] = [['services', '서비스'], ['catalog', 'ECU·DID'], ['workflow', '명령·시퀀스'], ['watch', '감시'], ['project', '프로젝트']];
  const tabBtns = new Map<ToolId, HTMLButtonElement>();
  for (const [id, label] of TOOLS) {
    const b = button(label, '');
    b.setAttribute('role', 'tab');
    b.dataset.tool = id;
    b.id = `${uid}-tt-${id}`;
    b.setAttribute('aria-controls', `${uid}-tp-${id}`);
    b.addEventListener('click', () => openTool(tool === id ? null : id));
    tabBtns.set(id, b);
    toolTabs.append(b);
  }
  ctxTools.append(connOv, sessChip, el('span', 'grow'), toolTabs);

  // 주소 상세 (route info) + logical-link grid (testid uds-targets)
  const routeInfo = el('div', 'route-info');
  routeInfo.id = `${uid}-ri`;
  routeInfo.hidden = true;
  riBtn.setAttribute('aria-controls', routeInfo.id);
  riBtn.setAttribute('aria-expanded', 'false');
  const riTop = el('div', 'ri-top');
  const riKv = el('dl', 'ri-kv');
  const riNote = el('p', 'ri-note');
  riNote.innerHTML =
    '<b>Loom UDS daemon 경로.</b> 현재 지원 전송은 DoIP (ISO 13400 — 실제 엔티티 또는 daemon 내장 SIM bench ECU)뿐입니다. CAN-TP·EthDiag는 백엔드 미지원이라 선택할 수 없고 연결된 것으로 표시하지 않습니다. 바이트 패널은 UDS PDU만 보여 주며 DoIP 헤더·TCP 프레임은 Ethernet 모듈에서 별도로 봅니다 (자동 연동 없음).';
  riTop.append(riKv, riNote);
  const bar = el('section', 'uds-targets');
  bar.setAttribute('data-testid', 'uds-targets');
  bar.setAttribute('aria-label', '논리 링크');
  const linkGrid = el('div', 'uds-ds-grid uds-ds-linkgrid');
  linkGrid.setAttribute('role', 'table');
  linkGrid.setAttribute('aria-label', '논리 링크');
  bar.append(linkGrid);
  routeInfo.append(riTop, bar);

  // 연결 설정 (preset editor)
  const cfgPanel = el('form', 'cfg-panel');
  cfgPanel.id = `${uid}-cfg`;
  cfgPanel.noValidate = true;
  cfgPanel.autocomplete = 'off';
  cfgPanel.hidden = true;
  cfgPanel.setAttribute('role', 'region');
  cfgBtn.setAttribute('aria-controls', cfgPanel.id);
  cfgBtn.setAttribute('aria-expanded', 'false');
  ctxbar.append(ctxRow, ctxTools, routeInfo, cfgPanel);

  // ---- tool drawer ----
  const drawer = el('aside', 'tooldrawer');
  drawer.setAttribute('aria-label', '도구 서랍');
  drawer.hidden = true;
  const tdH = el('div', 'td-h');
  const tdTitle = el('b');
  const tdClose = ibtn('x', '도구 서랍 닫기 (Esc)');
  tdH.append(tdTitle, el('span', 'grow'), tdClose);
  drawer.append(tdH);
  const tdPanels = new Map<ToolId, HTMLElement>();
  for (const [id] of TOOLS) {
    const s = el('section', 'td-panel');
    s.setAttribute('role', 'tabpanel');
    s.id = `${uid}-tp-${id}`;
    s.setAttribute('aria-labelledby', `${uid}-tt-${id}`);
    s.hidden = true;
    tdPanels.set(id, s);
    drawer.append(s);
  }

  // ---- workspace ----
  const wsp = el('div', 'wsp');
  // DID list
  const didPane = el('aside', 'didpane pane');
  didPane.setAttribute('aria-label', 'DID 목록');
  const didH = el('div', 'pane-h');
  const didCollapse = ibtn('panel', 'DID 목록 접기');
  didH.append(el('span', undefined, 'DID 목록'), el('span', 'grow'), didCollapse);
  const didSearchWrap = el('div', 'did-search');
  const didSearch = el('input', 'fld');
  didSearch.type = 'search';
  didSearch.placeholder = '이름 또는 hex 검색';
  didSearch.setAttribute('aria-label', 'DID 검색');
  didSearch.spellcheck = false;
  didSearchWrap.append(didSearch);
  const didList = el('ul', 'didlist');
  didList.setAttribute('role', 'listbox');
  didList.setAttribute('aria-label', 'Read DID 빠른 선택 (선택만으로 전송하지 않음)');
  didList.tabIndex = 0;
  const didNote = el('p', 'pane-note');
  didNote.innerHTML = '선택은 요청 편집기에만 채웁니다. 전송은 <b>읽기</b>로만.';
  didPane.append(didH, didSearchWrap, didList, didNote);
  const didRail = button('', 'rail did-rail', 'DID 목록 펼치기');
  didRail.append(icon('panel'), el('span', undefined, 'DID'));
  didRail.hidden = true;

  // center: request composer
  const center = el('section', 'center');
  const req = el('form', 'req m-did');
  req.noValidate = true;
  req.autocomplete = 'off';
  req.setAttribute('aria-label', 'UDS 요청 (DID 읽기 / Raw PDU)');
  const reqTarget = el('div', 'req-target');
  const reqRow = el('div', 'req-row');
  reqRow.setAttribute('data-testid', 'uds-current');
  let mode: 'did' | 'raw' = prefs.mode;
  const modeSeg = segGroup('sm mode-seg', '요청 모드 (←→)', [
    { v: 'did', label: 'DID', title: 'DID 읽기 — SID 0x22 + DID 자동 구성' },
    { v: 'raw', label: 'Raw', title: 'Raw UDS PDU — 입력한 바이트(SID 포함) 그대로 전송' },
  ], (v) => setMode(v));
  const sidKv = el('span', 'kv-c did-only');
  sidKv.title = '서비스 고정: ReadDataByIdentifier';
  sidKv.append(el('span', 'k', 'SID'), el('b', undefined, '0x22'), el('small', undefined, 'ReadDataByIdentifier'));
  const didLabel = el('label', 'kv-c did-c did-only');
  const didInput = el('input', 'fld did-in');
  didInput.type = 'text';
  didInput.maxLength = 12;
  didInput.spellcheck = false;
  didInput.placeholder = 'F190';
  didInput.setAttribute('aria-label', 'DID');
  didInput.value = prefs.did;
  didLabel.append(el('span', 'k', 'DID'), didInput);
  const didQuick = el('select', 'fld did-quick did-only');
  didQuick.setAttribute('aria-label', '빠른 DID 선택');
  const didName = el('span', 'did-name did-only');
  const reqPdu = el('span', 'pdu did-only');
  reqPdu.setAttribute('aria-live', 'polite');
  const rawSum = el('span', 'pdu raw-only');
  const SEND_TITLE = '요청 전송 (DID 입력란에서 Enter · Raw에서 Ctrl/⌘+Enter) — daemon dry-run·정책·ActionGate 경유';
  const sendBtn = button('', 'btn primary uds-ds-transmit', SEND_TITLE);
  sendBtn.type = 'submit';
  sendBtn.dataset.cmd = 'read';
  const sendLabel = el('span', undefined, '읽기');
  sendBtn.append(icon('read'), sendLabel);
  const abortBtn = button('', 'btn danger', '이 대상의 진행 중 요청 취소 (uds.abort — 이미 보낸 요청은 ⌛? 불확정이 될 수 있음)');
  abortBtn.dataset.cmd = 'cancel';
  const abortLabel = el('span', undefined, '취소');
  abortBtn.append(icon('x'), abortLabel);
  reqRow.append(modeSeg.root, sidKv, didLabel, didQuick, didName, reqPdu, rawSum, el('span', 'grow'), sendBtn, abortBtn);
  const rawRow = el('div', 'raw-row raw-only');
  const rawEd = el('div', 'raw-ed');
  const rawLab = el('label', 'k');
  rawLab.append('Raw UDS PDU ', el('small', undefined, '(SID 포함 hex)'));
  const rawInput = el('textarea', 'fld raw-in');
  rawInput.id = `${uid}-raw`;
  rawLab.htmlFor = rawInput.id;
  rawInput.rows = 2;
  rawInput.spellcheck = false;
  rawInput.placeholder = '22 F1 90';
  rawInput.setAttribute('aria-label', 'UDS 요청 (hex)');
  rawInput.setAttribute('aria-keyshortcuts', 'Control+Enter Meta+Enter');
  rawInput.value = prefs.raw;
  rawEd.append(rawLab, rawInput);
  const rawPv = el('div', 'raw-pv');
  const rawPduLab = el('span', 'k', '전송될 PDU (정규화)');
  const rawPrev = el('div', 'raw-prev');
  rawPrev.setAttribute('role', 'region');
  rawPrev.setAttribute('aria-label', '전송될 PDU');
  rawPrev.tabIndex = 0;
  rawPv.append(rawPduLab, rawPrev);
  const rawHint = el('p', 'raw-hint');
  rawHint.innerHTML =
    '<b>형식</b> 공백·탭·줄바꿈 구분 또는 연속 hex (<code>22 F1 90</code> = <code>22F190</code>), <code>0x</code>는 바이트마다 2자리 (<code>0x22 0xF1 0x90</code>). 홀수 자릿수·비hex·쉼표는 전체 거부 (부분 전송 없음). 최대 4095 B. SID 자동 추가·패딩 없음. <b>Ctrl/⌘+Enter</b> 또는 [전송]으로만 전송, Enter는 줄바꿈.';
  rawRow.append(rawEd, rawPv, rawHint);
  const reqMsg = el('div', 'req-msg');
  reqMsg.setAttribute('aria-live', 'polite');
  const msgText = el('span');
  const preview = el('span', 'uds-preview req-pv');
  preview.setAttribute('data-testid', 'uds-preview');
  // U7 (B1-P3a): profile-less daemon target → explicit local draft (never sends, never saves)
  const mkProfileBtn = button('이 대상으로 프로필 만들기', 'btn sm', 'daemon이 보고한 대상 이름·논리 주소로 새 로컬 프로필 초안 — 저장·연결하지 않음');
  mkProfileBtn.dataset.cmd = 'make-profile';
  mkProfileBtn.hidden = true;
  mkProfileBtn.addEventListener('click', onMakeProfile);
  reqMsg.append(msgText, mkProfileBtn, preview);
  req.append(reqTarget, reqRow, rawRow, reqMsg);

  // history
  const hist = el('div', 'hist pane');
  const histH = el('div', 'pane-h');
  const histCount = el('span', 'hcount');
  const scopeSeg = segGroup('sm', '기록 범위', [{ v: 'all', label: '전체' }, { v: 'ecu', label: '현재 ECU' }], (v) => {
    histScope = v;
    setPrefs({ ...prefs, histScope: v });
  });
  const expJson = ibtn('down', '기록 JSON 내보내기', 'JSON', 'ibtn exp');
  const expCsv = ibtn('down', '기록 CSV 내보내기', 'CSV', 'ibtn exp');
  const clearBtn = ibtn('trash', '완료된 기록 지우기 (표시에서만 — daemon 기록·진행 중 요청은 유지)', undefined, 'ibtn danger');
  histH.append(el('span', undefined, '트랜잭션'), histCount, el('span', 'grow'), scopeSeg.root, expJson, expCsv, clearBtn);
  const histHost = el('div', 'hist-host');
  hist.append(histH, histHost);
  const divHist = el('div', 'divider h');
  // detail
  const detail = el('div', 'detail');
  const seqPane = el('div', 'dt-seq pane');
  seqPane.setAttribute('aria-label', '응답 순서');
  const seqHead = el('span', 'bsel');
  const seqH = el('div', 'pane-h');
  seqH.append(el('span', undefined, '응답 순서'), el('span', 'grow'), seqHead);
  const seqList = el('ol', 'seq');
  seqList.setAttribute('role', 'listbox');
  seqList.tabIndex = 0;
  seqList.setAttribute('aria-label', '선택 트랜잭션 이벤트 (↑↓, 바이트/해석 대상 선택)');
  seqPane.append(seqH, seqList);
  const fieldsPane = el('div', 'dt-fields pane');
  fieldsPane.setAttribute('aria-label', '해석');
  const fieldsHead = el('span', 'bsel');
  const fieldsH = el('div', 'pane-h');
  fieldsH.append(el('span', undefined, '해석'), el('span', 'grow'), fieldsHead);
  const fields = el('div', 'fields');
  fields.setAttribute('data-testid', 'uds-fields');
  fieldsPane.append(fieldsH, fields);
  // U2 minimum: seed of a selected positive 67 xx row → copy / key template (never sends; renderDetail places it)
  const seedBar = el('div', 'seed-bar');
  seedBar.setAttribute('data-testid', 'uds-seed');
  const seedCode = el('code', 'seed-hex');
  const seedCopy = button('', 'btn sm', 'seed hex 복사');
  seedCopy.dataset.cmd = 'seed-copy';
  seedCopy.append(icon('copy'), '복사');
  const seedTpl = button('키 요청 템플릿 → Raw', 'btn sm', 'sendKey 템플릿을 Raw 편집기에 채움 — <key>를 직접 입력하기 전에는 전송 불가');
  seedTpl.dataset.cmd = 'seed-template';
  seedBar.append(el('span', 'k', 'seed'), seedCode, seedCopy, seedTpl);
  const bytesPane = el('div', 'dt-bytes pane');
  bytesPane.setAttribute('aria-label', 'UDS PDU 바이트');
  const copyHex = ibtn('copy', '선택 PDU hex 복사');
  const bytesH = el('div', 'pane-h');
  bytesH.append(el('span', undefined, '바이트 · UDS PDU'), el('span', 'grow'), copyHex);
  const hexEl = el('div', 'hex');
  hexEl.tabIndex = 0;
  bytesPane.append(bytesH, hexEl);
  detail.append(seqPane, fieldsPane, bytesPane);
  center.append(req, hist, divHist, detail);

  // confirmed values
  const vals = el('aside', 'vals pane');
  vals.setAttribute('aria-label', '확정 값');
  vals.setAttribute('data-testid', 'uds-last-values');
  const valsEcu = el('b', 'vals-ecu');
  const valsCollapse = ibtn('panel', '확정 값 접기');
  const valsH = el('div', 'pane-h');
  valsH.append(el('span', undefined, '확정 값'), valsEcu, el('span', 'grow'), valsCollapse);
  const valsBody = el('div', 'vals-body');
  const valsNote = el('p', 'pane-note', 'daemon이 해석한 확정 응답(decoded)으로만 갱신. 각 값은 출처 트랜잭션·시각·경로를 유지합니다.');
  vals.append(valsH, valsBody, valsNote);
  const valsRail = button('', 'rail right vals-rail', '확정 값 펼치기');
  valsRail.append(icon('panel'), el('span', undefined, '확정 값'));
  valsRail.hidden = true;
  wsp.append(didPane, didRail, center, vals, valsRail);
  mod.append(ctxbar, drawer, wsp);
  root.append(createLatchLine(ctx), mod);

  // =============================== history table ===============================
  const didNameOf = (did: string) => prefs.dids.find((d) => d.did === did)?.name ?? null;
  const cell = (text: string, cls: string, title?: string): Cell => ({ text, cls, title: title ?? (text.length > 12 ? text : undefined) });
  const statusText = (r: RowLike, now: number) => {
    const st = stageCell(r, now).text;
    const res = resultSymbol(r).text;
    return `${st}${res ? ` ${res}` : ''}`;
  };
  const elapsedText = (r: RowLike, now: number) => timingText(r, now) || (isFinal(r.state) ? '—' : '');
  const table = new VirtualTable<RowItem>(histHost, {
    rowHeight: 22,
    columns: [
      { key: 'no', header: '#', width: '44px', cell: (it) => (it.t === 'row' ? cell(String(rowNo.get(it.row.requestId) ?? ''), 'c-no') : cell('손실', 'c-no')) },
      { key: 't', header: '시각', width: '108px', cell: (it) => (it.t === 'row' ? cell(fmtClock(it.row.firstWallMs), 'c-t', it.row.firstWallMs ? new Date(it.row.firstWallMs).toISOString() : undefined) : cell(`${it.gap.kind}`, 'c-t')) },
      { key: 'ecu', header: 'ECU', width: '72px', cell: (it) => (it.t === 'row' ? cell(rowTarget(it.row), 'c-ecu', rowTarget(it.row) || undefined) : cell('', 'c-ecu')) },
      { key: 'rt', header: '경로', width: '132px', cell: (it) => {
          if (it.t !== 'row') return cell('', 'c-rt');
          const rr = rowRoute(it.row.payload);
          return cell(rr.text, rr.known ? 'c-rt' : 'c-rt na', rr.title);
        },
      },
      { key: 'did', header: 'DID · SID', width: '112px', cell: (it) => (it.t === 'row' ? cell(didSidText(it.row, didNameOf), 'c-did', str(it.row.payload.service) || undefined) : cell('', 'c-did')) },
      {
        key: 'req', header: '요청', width: '84px',
        cell: (it) => {
          if (it.t !== 'row') return cell('', 'c-req');
          const h = reqHexOf(it.row);
          return h ? cell(spacedHex(h.length > 128 ? h.slice(0, 128) : h) + (h.length > 128 ? ' …' : ''), 'c-req', `${spacedHex(h)} (${h.length / 2} B)`) : cell('—', 'c-req na');
        },
      },
      { key: 'st', header: '상태', width: '136px', cell: (it, now) => (it.t === 'row' ? cell(statusText(it.row, now), 'c-st') : cell('', 'c-st')) },
      { key: 'p', header: '78', width: '30px', cell: (it) => (it.t === 'row' ? cell(String(Number(it.row.payload.pending_count) || '') || '', 'c-p', 'responsePending (0x78) 횟수') : cell('', 'c-p')) },
      { key: 'el', header: '경과', width: '64px', cell: (it, now) => (it.t === 'row' ? cell(elapsedText(it.row, now), 'c-el') : cell('', 'c-el')) },
      // U4: the only flexible track is last, so a long response never shifts status / 78 / elapsed
      {
        key: 'rsp', header: '최종 응답', width: 'minmax(170px, 1fr)',
        cell: (it) => {
          if (it.t !== 'row') return cell('', 'c-rsp');
          const h = isFinal(it.row.state) ? respHexOf(it.row) : null;
          return h ? cell(spacedHex(h), 'c-rsp') : cell('—', 'c-rsp na');
        },
      },
    ],
    keyOf: (it) => (it.t === 'row' ? it.row.requestId : it.key),
    rowAttrs: (it): Record<string, string> =>
      it.t === 'row'
        ? { 'data-request-id': it.row.requestId, 'data-origin': it.row.origin, 'data-state': it.row.state }
        : { 'data-gap': it.gap.kind },
    rowClass: (it) => (it.t === 'row' ? `st-row-${it.row.state} ${statusClass(it.row)}` : 'gap-row'),
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: () => {
      selEv = -1;
      selField = -1;
      detailKey = '';
      ctx.scheduler.invalidate();
    },
    ariaLabel: '트랜잭션 기록 (↑↓ 선택)',
    emptyText: '아직 요청 없음 (자동 전송·재전송 없음). ① ECU 선택 후 [연결] ② DID 선택 또는 입력 — 또는 [Raw]에서 hex PDU ③ [읽기] / [전송]',
  });

  // cross-panel reveal (e.g. eth `↔UDS` chip): select + scroll to the request_id row
  const offReveal = ctx.ui.onReveal((ref) => {
    if (ref.domain !== 'uds' || ref.requestId === null) return false;
    return revealRow(ref.requestId);
  });
  function revealRow(id: string): boolean {
    hidden.delete(id);
    if (histScope === 'ecu') {
      histScope = 'all';
      scopeSeg.set('all');
    }
    table.setItems(visibleItems());
    table.render();
    return table.reveal(id);
  }
  const visibleRows = (): RowLike[] => {
    const cur = target();
    return rows.list().filter((r) => !hidden.has(r.requestId) && (histScope === 'all' || rowTarget(r) === cur));
  };
  const visibleItems = (): RowItem[] => mergeGaps(visibleRows(), mirror.gaps);
  const rowById = (id: string | null) => (id ? (rows.list().find((r) => r.requestId === id) ?? null) : null);

  // =============================== composer ===============================
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let previewAbort: AbortController | null = null;
  let previewRisk: unknown = null;
  let previewFor = '';
  let armedAt: number | null = null;
  let submitted = false;

  /** normalized request of the composer (whole-input validation; never partial) */
  const composed = (): { hex: string | null; err: string | null } => {
    if (mode === 'did') {
      const v = normalizeDid(didInput.value);
      return v.did ? { hex: `22${v.did}`, err: null } : { hex: null, err: v.err };
    }
    return normalizeRaw(rawInput.value);
  };

  const runPreview = () => {
    previewTimer = null;
    const { hex } = composed();
    const t = target();
    if (!hex || !t || needsConnect()) {
      // unbound + not connected: no dry-run against an alias the daemon does not know (re-run once it is reported)
      previewRisk = null;
      previewFor = '';
      preview.textContent = hex && t ? 'dry-run 미리보기는 연결 후' : '';
      return;
    }
    previewAbort?.abort();
    const ac = new AbortController();
    previewAbort = ac;
    ctx
      .query<Record<string, unknown>>('uds.request', { target: t, bytes_hex: hex, dry_run: true }, ac.signal)
      .then((res) => {
        if (ac.signal.aborted) return;
        const pv = previewText(res);
        previewRisk = pv.risk;
        previewFor = `${t}|${hex}`;
        preview.textContent = pv.text ? `dry-run: ${pv.text}` : '';
      })
      .catch(() => {
        if (!ac.signal.aborted) preview.textContent = '미리보기 실패';
      });
  };
  /** any request change: disarm the second press and re-run the (debounced) dry-run only */
  const reqChanged = () => {
    armedAt = null;
    previewRisk = null;
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(runPreview, PREVIEW_DEBOUNCE_MS);
    uiKey = '';
    ctx.scheduler.invalidate();
  };
  const persistComposer = () => {
    prefs = { ...prefs, mode, did: didInput.value.slice(0, 12), raw: rawInput.value.slice(0, 12_000) };
    savePrefs(prefs);
  };

  function setMode(m: 'did' | 'raw') {
    if (mode === m) return;
    mode = m;
    persistComposer();
    submitted = false;
    reqChanged();
  }

  /** UI-side duplicate-entry guard per target: set synchronously before ctx.act('send') and cleared when the RPC
   *  settles (uds.request answers after the final result). Button + Enter + tool [실행] repeats inside that
   *  window are dropped — never queued, never retried. */
  const sending = new Set<string>();
  const sendOnce = async (t: string, hex: string): Promise<{ ok: boolean; requestId: string | null; msg: string }> => {
    if (sending.has(t)) return { ok: false, requestId: null, msg: '같은 대상 요청이 진행 중 — 전송 안 함 (중복 방지)' };
    sending.add(t);
    uiKey = '';
    ctx.scheduler.invalidate();
    try {
      const h = await ctx.act('send', { target: t, bytes_hex: hex });
      const st = h.state();
      const ok = st === null || (st !== 'failed' && st !== 'outcome_unknown');
      return { ok, requestId: h.requestId, msg: ok ? '✓' : (st ?? '') };
    } catch (e) {
      return { ok: false, requestId: null, msg: e instanceof Error ? e.message : String(e) };
    } finally {
      sending.delete(t);
      uiKey = '';
      ctx.scheduler.invalidate();
    }
  };
  const inflightFor = (t: string | null) => (t ? rows.list().filter((r) => rowTarget(r) === t && !isFinal(r.state)) : []);

  const submit = () => {
    submitted = true;
    const { hex, err } = composed();
    const t = target();
    if (!writable()) {
      uiKey = '';
      ctx.scheduler.invalidate();
      return;
    }
    if (!hex || !t) {
      msgText.textContent = !t ? '대상 없음 — ECU를 선택하세요' : `${err ?? '유효하지 않음'} — 전송하지 않음`;
      reqMsg.className = 'req-msg err';
      return;
    }
    const key = `${t}|${hex}`;
    if (needsSecondEnter(previewRisk) && previewFor === key && (armedAt === null || performance.now() - armedAt > SECOND_ENTER_MS)) {
      armedAt = performance.now();
      preview.textContent = `${preview.textContent ?? ''} — ${str(previewRisk)}: Enter 한 번 더`;
      return;
    }
    if (sending.has(t)) return; // previous send of this target not acknowledged yet: drop, no queue/retry
    armedAt = null;
    submitted = false;
    void sendOnce(t, hex).then((r) => {
      if (!r.ok && r.requestId === null) ctx.ui.toast(`send 실패: ${r.msg}`);
    });
  };

  didInput.addEventListener('input', () => {
    persistComposer();
    reqChanged();
  });
  didInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.repeat && !e.isComposing) {
      e.preventDefault();
      submit();
    }
  });
  rawInput.addEventListener('input', () => {
    persistComposer();
    syncFromHex();
    reqChanged();
  });
  rawInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.repeat) {
      e.preventDefault();
      submit();
    }
  });
  req.addEventListener('submit', (e) => {
    e.preventDefault();
    submit();
  });
  sendBtn.addEventListener('click', (e) => {
    e.preventDefault();
    submit();
  });
  abortBtn.addEventListener('click', () => {
    const t = target();
    if (t) void act('abort', { target: t });
  });
  didQuick.addEventListener('change', () => {
    if (!didQuick.value) return;
    didInput.value = didQuick.value;
    persistComposer();
    reqChanged();
  });

  // =============================== 서비스 tool (catalogue → params → Raw composer) ===============================
  const svcRoot = el('div', 'sv');
  const svcTarget = el('p', 'sv-target');
  const catSec = el('section', 'sv-sec');
  catSec.setAttribute('data-testid', 'uds-catalog');
  catSec.setAttribute('aria-label', '진단 서비스');
  const search = el('input', 'fld sv-search');
  search.type = 'search';
  search.placeholder = 'SID 또는 이름 (예: 22, Read)';
  search.setAttribute('aria-label', '서비스 검색');
  search.spellcheck = false;
  const tree = el('div', 'uds-ds-tree');
  tree.setAttribute('role', 'list');
  catSec.append(el('h3', undefined, '진단 서비스 (ISO 14229-1)'), search, tree, el('p', 'uds-ds-note', '선택·파라미터 편집은 요청 편집기 Raw만 채웁니다 (전송 없음).'));
  const setSec = el('section', 'sv-sec');
  setSec.setAttribute('data-testid', 'uds-settings');
  const kv = el('div', 'uds-ds-kv');
  const serviceCell = el('span', 'uds-ds-kv-v mono');
  serviceCell.setAttribute('data-testid', 'uds-service');
  kv.append(el('span', 'uds-ds-kv-k', '진단 서비스'), serviceCell);
  const params = el('div', 'uds-ds-grid uds-ds-params');
  params.setAttribute('role', 'table');
  params.setAttribute('aria-label', '파라미터');
  params.setAttribute('data-testid', 'uds-params');
  const issuesEl = el('div', 'uds-ds-issues');
  issuesEl.setAttribute('data-testid', 'uds-param-issues');
  setSec.append(el('h3', undefined, '파라미터'), kv, params, issuesEl);
  const dtcSec = el('section', 'sv-sec');
  dtcSec.setAttribute('data-testid', 'uds-dtc');
  const dtcActions = el('div', 'sv-row');
  const dtcReadBtn = button('읽기 템플릿 19 02 FF', 'btn', 'ReadDTCInformation reportDTCByStatusMask를 요청 편집기에 채움 (전송 없음)');
  const dtcClearBtn = button('지우기 템플릿 14 FF FF FF', 'btn', 'ClearDiagnosticInformation 전체 그룹을 요청 편집기에 채움 (전송 없음, dry-run 등급 확인 필요)');
  dtcActions.append(dtcReadBtn, dtcClearBtn);
  // U3 (B1-P2): panel-local status-bit filter (not persisted); response order is kept
  let dtcFilter: DtcFilter = 'all';
  const DTC_FILTERS: { v: DtcFilter; label: string; title: string }[] = [
    { v: 'all', label: '전체', title: '응답 순서 그대로 모든 DTC' },
    { v: 'confirmed', label: 'confirmed', title: 'bit3 confirmedDTC (CDTC)' },
    { v: 'testFailed', label: 'testFailed', title: 'bit0 testFailed (TF)' },
    { v: 'pending', label: 'pending', title: 'bit2 pendingDTC (PDTC)' },
  ];
  const dtcFilterSeg = segGroup('sm dtc-filter', 'DTC 상태 필터', DTC_FILTERS, onDtcFilter);
  const dtcCount = el('span', 'hcount dtc-count');
  const dtcFbar = el('div', 'dtc-fbar');
  dtcFbar.append(dtcFilterSeg.root, dtcCount);
  const dtcBody = el('div', 'sv-dtc');
  dtcSec.append(el('h3', undefined, 'DTC'), dtcActions, dtcFbar, dtcBody);
  const tpSec = el('section', 'sv-sec');
  tpSec.setAttribute('data-testid', 'uds-session');
  const tpKv = el('dl', 'sv-kv');
  // U1: daemon TesterPresent job (default off; interval from the profile or the visible default)
  const tpRow = el('div', 'sv-row tp-row');
  const tpBtn = button('', 'btn sm');
  tpBtn.dataset.cmd = 'tp';
  const tpIv = el('input', 'fld tp-iv');
  tpIv.type = 'number';
  tpIv.min = '500';
  tpIv.max = '30000';
  tpIv.step = '100';
  tpIv.setAttribute('aria-label', 'TesterPresent 주기 (ms)');
  const tpInfo = el('span', 'sv-note tp-info');
  tpRow.append(tpBtn, tpIv, el('span', 'unit', 'ms'), tpInfo);
  // U2: security unlock with a loom.toml key provider (R3 sendKey, second click)
  const secRow = el('div', 'sv-row sec-row');
  const secLevel = el('input', 'fld sec-level');
  secLevel.setAttribute('aria-label', 'requestSeed 레벨 (홀수 hex 01–7D)');
  secLevel.placeholder = '01';
  secLevel.maxLength = 4;
  secLevel.spellcheck = false;
  const secProv = el('input', 'fld sec-prov');
  secProv.setAttribute('aria-label', '키 제공자 이름 (loom.toml [[uds.key_providers]])');
  secProv.placeholder = 'provider 이름';
  secProv.maxLength = 64;
  secProv.spellcheck = false;
  const unlockBtn = button('', 'btn sm');
  unlockBtn.dataset.cmd = 'unlock';
  const unlockLabel = el('span', undefined, '자동 해제');
  unlockBtn.append(unlockLabel);
  secRow.append(el('span', 'k', '보안'), secLevel, secProv, unlockBtn);
  const secMsg = el('p', 'sv-note sec-msg');
  secMsg.setAttribute('role', 'status');
  tpSec.append(
    el('h3', undefined, '세션 · 보안 · TesterPresent'),
    tpKv,
    tpRow,
    secRow,
    secMsg,
    el('p', 'sv-note', '세션 전환은 [10] DiagnosticSessionControl 템플릿 → [전송]. TesterPresent는 daemon 작업 (유휴 시에만 3E 80, 링크·STOP·제어권 변경 시 자동 중지, 자동 재시작 없음). 자동 해제는 loom.toml에 사용자가 등록한 키 제공자만 이름으로 사용하며 실패 시 재시도하지 않습니다.'),
  );
  // U5: diagnostic DB (decode only; never risk)
  const dbSec = el('section', 'sv-sec');
  dbSec.setAttribute('data-testid', 'uds-db');
  const dbInfo = el('p', 'sv-note db-info');
  const dbText = el('textarea', 'fld db-text');
  dbText.rows = 5;
  dbText.spellcheck = false;
  dbText.placeholder = '{"loom_diag": 1, "ecu": "…", "dids": [ … ]}';
  dbText.setAttribute('aria-label', 'loom_diag DB JSON');
  const dbFile = el('input');
  dbFile.type = 'file';
  dbFile.accept = '.json,application/json';
  dbFile.hidden = true;
  const dbPick = button('파일…', 'btn sm', 'JSON 파일을 읽어 아래 편집기에 채움 (전송 없음)');
  dbPick.dataset.cmd = 'db-file';
  const dbSaved = el('select', 'fld db-saved');
  dbSaved.setAttribute('aria-label', '저장된 DB');
  const dbApply = button('대상에 적용', 'btn sm', 'uds.db.set — 선택한 대상의 DID 이름·단위 해석만 바뀜 (위험 등급 불변)');
  dbApply.dataset.cmd = 'db-set';
  const dbClearBtn = button('해제', 'btn sm ghost', 'uds.db.clear — ISO DID 표만 사용');
  dbClearBtn.dataset.cmd = 'db-clear';
  const dbRow = el('div', 'sv-row');
  dbRow.append(dbPick, dbSaved, el('span', 'grow'), dbApply, dbClearBtn);
  const dbMsg = el('p', 'sv-note db-msg');
  dbMsg.setAttribute('role', 'status');
  dbSec.append(el('h3', undefined, '진단 DB (loom_diag)'), dbInfo, dbText, dbFile, dbRow, dbMsg);
  svcRoot.append(svcTarget, catSec, setSec, dtcSec, tpSec, dbSec);
  tdPanels.get('services')!.append(svcRoot);

  let tpl: ServiceTemplate | null = null;
  let values: ParamValues = {};
  let catalogQuery = '';
  let catalogKey = '';
  const paramInputs = new Map<string, HTMLInputElement | HTMLSelectElement>();

  const renderIssues = () => {
    const list = tpl ? paramIssues(tpl, values) : [];
    issuesEl.textContent = list.length ? `⚠ ${list.join(' · ')} — 그대로 보내면 ECU가 거절할 수 있음` : '';
  };
  const renderService = () => {
    const sid = normalizeHex(rawInput.value)?.slice(0, 2) ?? null;
    serviceCell.textContent = tpl ? templateLabel(tpl) : sid ? `SID 0x${sid} — 카탈로그 밖 (원시 hex 그대로)` : '—';
  };
  const renderParams = () => {
    paramInputs.clear();
    const head = el('div', 'uds-ds-row uds-ds-head');
    head.setAttribute('role', 'row');
    for (const h of ['파라미터', '값 (hex)', '의미']) head.append(colHead(h));
    const out: HTMLElement[] = [head];
    if (!tpl) {
      out.push(el('div', 'uds-ds-empty', '서비스를 고르면 ISO 14229-1 파라미터가 나타납니다 · Raw에 hex를 직접 써도 됩니다'));
      params.replaceChildren(...out);
      return;
    }
    const sidRow = el('div', 'uds-ds-row uds-ds-fixed');
    sidRow.setAttribute('role', 'row');
    sidRow.append(el('span', undefined, 'SID'), el('span', 'mono', `0x${tpl.sid}`), el('span', 'muted', '고정'));
    out.push(sidRow);
    const w = editable();
    for (const p of tpl.params) {
      const row = el('div', 'uds-ds-row');
      row.setAttribute('role', 'row');
      row.dataset.param = p.key;
      const meaning = el('span', 'muted');
      const setMeaning = () => {
        const c = p.choices?.find((x) => x.value === (values[p.key] ?? '').toUpperCase());
        meaning.textContent = c ? c.label : p.bytes === null ? `가변 길이${p.optional ? ' · 선택' : ''}` : `${p.bytes}바이트${p.optional ? ' · 선택' : ''}`;
      };
      let field: HTMLInputElement | HTMLSelectElement;
      if (p.choices) {
        const sel = el('select', 'fld');
        for (const c of p.choices) sel.append(Object.assign(el('option', undefined, `${c.value} ${c.label}`), { value: c.value }));
        sel.value = values[p.key] ?? p.def;
        sel.addEventListener('change', () => onParam(p.key, sel.value, setMeaning));
        field = sel;
      } else {
        const inp = el('input', 'fld');
        inp.value = values[p.key] ?? '';
        inp.placeholder = p.bytes === null ? 'hex…' : '00'.repeat(p.bytes);
        inp.spellcheck = false;
        inp.addEventListener('input', () => onParam(p.key, inp.value, setMeaning));
        field = inp;
      }
      field.setAttribute('aria-label', p.name);
      field.disabled = !w;
      paramInputs.set(p.key, field);
      setMeaning();
      row.append(el('span', undefined, p.name), field, meaning);
      out.push(row);
    }
    params.replaceChildren(...out);
  };
  const onParam = (key: string, v: string, after: () => void) => {
    if (!tpl) return;
    values = { ...values, [key]: v };
    rawInput.value = buildPdu(tpl, values);
    after();
    renderIssues();
    renderService();
    persistComposer();
    reqChanged();
  };
  const applyTemplate = (t: ServiceTemplate, preset?: ParamValues) => {
    if (!editable()) return; // fills the composer only — never sends
    tpl = t;
    values = { ...defaultValues(t), ...preset };
    rawInput.value = buildPdu(t, values);
    modeSeg.set('raw');
    mode = 'raw';
    renderParams();
    renderIssues();
    renderService();
    catalogKey = '';
    persistComposer();
    reqChanged();
  };
  /** typing the Raw line selects the matching template (if any) and reflects its fields */
  function syncFromHex() {
    const sid = rawInput.value.replace(/\s+/g, '').replace(/^0x/i, '').slice(0, 2).toUpperCase();
    const t = sid.length === 2 ? findTemplate(sid) : null;
    if (t !== tpl) {
      tpl = t;
      values = t ? parsePdu(t, rawInput.value) : {};
      renderParams();
      catalogKey = '';
    } else if (t) {
      values = parsePdu(t, rawInput.value);
      for (const [k, f] of paramInputs) if (f.value !== values[k]) f.value = values[k] ?? '';
    }
    renderIssues();
    renderService();
  }
  search.addEventListener('input', () => {
    catalogQuery = search.value;
    ctx.scheduler.invalidate();
  });
  const dtcTpl = findTemplate('19');
  const clrTpl = findTemplate('14');
  dtcReadBtn.addEventListener('click', () => dtcTpl && applyTemplate(dtcTpl, { type: '02', mask: 'FF' }));
  dtcClearBtn.addEventListener('click', () => clrTpl && applyTemplate(clrTpl, { group: 'FFFFFF' }));

  const renderCatalog = () => {
    const w = editable();
    const key = `${catalogQuery}|${tpl?.sid ?? ''}|${w}`;
    if (key === catalogKey) return;
    catalogKey = key;
    const list = filterCatalog(catalogQuery);
    const out: HTMLElement[] = [];
    for (const g of SERVICE_GROUPS) {
      const items = list.filter((t) => t.group === g);
      if (!items.length) continue;
      out.push(el('div', 'uds-ds-group', g));
      for (const t of items) {
        const b = button(templateLabel(t), 'uds-ds-svc');
        b.setAttribute('role', 'listitem');
        b.dataset.sid = t.sid;
        b.setAttribute('aria-pressed', String(tpl === t));
        b.disabled = !w;
        b.addEventListener('click', () => applyTemplate(t));
        out.push(b);
      }
    }
    if (!out.length) out.push(el('div', 'uds-ds-empty', '일치하는 표준 서비스 없음 — Raw에 hex를 직접 입력'));
    tree.replaceChildren(...out);
  };

  let dtcKey = '';
  const renderDtc = () => {
    const cur = target();
    const key = `${rowsVersion}|${cur}|${editable()}|${dtcFilter}`;
    if (key === dtcKey) return;
    dtcKey = key;
    dtcReadBtn.disabled = dtcClearBtn.disabled = !editable(); // templates fill the composer (no send)
    dtcFilterSeg.set(dtcFilter);
    const dv = latestDtcs(rows.list(), cur);
    const grid = el('div', 'uds-ds-grid uds-ds-dtcgrid');
    grid.setAttribute('role', 'table');
    grid.setAttribute('aria-label', 'DTC');
    const head = el('div', 'uds-ds-row uds-ds-head');
    head.setAttribute('role', 'row');
    for (const h of ['DTC (hex)', '상태', '상태 비트 (ISO 14229-1)']) head.append(colHead(h));
    grid.append(head);
    if (!dv) {
      dtcCount.textContent = '';
      grid.append(el('div', 'uds-ds-empty', `${cur ?? '대상'}: 확정된 0x19 응답 없음`));
    } else if (dv.entries.length === 0) {
      dtcCount.textContent = '표시 0/0';
      grid.append(el('div', 'uds-ds-empty', '없음 (응답에 DTC 0개)'));
    } else {
      const shown = filterDtcs(dv.entries, dtcFilter);
      dtcCount.textContent = `표시 ${shown.length}/${dv.entries.length}`;
      if (shown.length === 0) {
        const label = DTC_FILTERS.find((f) => f.v === dtcFilter)?.label ?? dtcFilter;
        grid.append(el('div', 'uds-ds-empty', `필터 ${label}: 해당 DTC 없음 (전체 ${dv.entries.length})`));
      }
      for (const d of shown) {
        const row = el('div', 'uds-ds-row');
        row.setAttribute('role', 'row');
        row.dataset.dtc = d.dtc;
        const st = el('span', 'mono', d.status);
        st.dataset.cell = 'status';
        st.title = dtcStatusTitle(d.statusByte, dv.availabilityMask);
        const bits = el('span', 'dtc-bits');
        if (d.statusByte === null) bits.textContent = '?';
        else {
          const set = dtcStatusFlags(d.statusByte, dv.availabilityMask).filter((f) => f.set);
          if (set.length === 0) bits.textContent = '—';
          for (const f of set) {
            const ab = el('abbr', `dtc-bit b${f.bit}${f.supported === false ? ' na' : ''}`, f.abbr);
            ab.title = f.name + (f.supported === false ? ' — availability mask에 없음' : '');
            bits.append(ab);
          }
        }
        if (d.statusByte === null) bits.classList.add('na');
        row.append(el('span', 'mono', d.dtc), st, bits);
        grid.append(row);
      }
    }
    const src = el('div', 'uds-ds-note');
    if (dv) {
      src.append('출처 ');
      const a = button(dv.requestId, 'lnk');
      a.addEventListener('click', () => revealRow(dv.requestId));
      const mk = dv.availabilityMask === null ? 'availability mask 미상' : `availability mask 0x${dv.availabilityMask.toString(16).toUpperCase().padStart(2, '0')}`;
      src.append(a, ` · DTC 코드 원시값 (재매핑 없음) · 상태 비트 ISO 14229-1 · ${mk}`);
    } else src.textContent = 'DTC 코드·상태 원시값 + ISO 14229-1 상태 비트';
    dtcBody.replaceChildren(grid, src);
  };

  // =============================== connect / presets ===============================
  const connecting = new Set<string>();
  connectBtn.addEventListener('click', () => {
    const t = target();
    const p = presetOf(t);
    if (!t || !p || connecting.has(t) || presetIssues(p).length || !canOperate()) return; // re-entry guard: one connect per target in flight
    connecting.add(t);
    uiKey = '';
    ctx.scheduler.invalidate();
    void ctx
      .act('connect', connectParams(p))
      .catch((e: unknown) => ctx.ui.toast(`uds.connect 실패: ${e instanceof Error ? e.message : String(e)}`))
      .finally(() => {
        connecting.delete(t);
        uiKey = '';
        ctx.scheduler.invalidate();
      });
  });
  ecuSel.addEventListener('change', () => selectTarget(ecuSel.value || null));

  // =============================== U6 disconnect / forget · U1 TP · U2 unlock · U5 DB (B4-D*) ===============================
  /** operations in flight per target (`${op}:${target}`): one of each at a time, cleared when the RPC settles */
  const pendingOps = new Set<string>();
  /** U6: E_BUSY answer armed a second click that aborts and closes (same target, within SECOND_ENTER_MS) */
  let discArm: { target: string; until: number } | null = null;
  /** U2: first click armed the R3 unlock for exactly this target/level/provider (within SECOND_ENTER_MS) */
  let unlockArm: { key: string; until: number } | null = null;
  /** U5: local pre-check of the DB editor text (checkDbText), refreshed on every edit */
  let dbCheck: ReturnType<typeof checkDbText> | null = null;
  /** B4-D1 (Sonnet body): [연결 해제] → uds.disconnect; E_BUSY arms a second click with abort:true. */
  function onDisconnect(): void {
    const t = target();
    if (!t || !targets.has(t) || !canOperate()) return;
    const key = `disconnect:${t}`;
    if (pendingOps.has(key)) return;
    const abort = discArm?.target === t && performance.now() < discArm.until;
    discArm = null;
    pendingOps.add(key);
    void (async () => {
      try {
        await ctx.act('disconnect', abort ? { target: t, abort: true } : { target: t });
      } catch (e) {
        if ((e as { code?: unknown } | null)?.code === 'E_BUSY' && !abort) {
          discArm = { target: t, until: performance.now() + SECOND_ENTER_MS };
          ctx.ui.toast(`${t}: 진행 중 요청 있음 — [연결 해제]를 ${SECOND_ENTER_MS / 1000}초 안에 다시 누르면 요청을 중단하고 해제 (쓰기는 결과 불확정이 될 수 있음)`);
        } else ctx.ui.toast(`uds.disconnect 실패: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        pendingOps.delete(key);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D2 (Sonnet body): [목록에서 제거] → uds.forget. */
  function onForget(): void {
    const t = target();
    if (!t || !canOperate()) return;
    const key = `forget:${t}`;
    if (pendingOps.has(key)) return;
    pendingOps.add(key);
    void (async () => {
      try {
        await ctx.act('forget', { target: t });
      } catch (e) {
        ctx.ui.toast(`uds.forget 실패: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        pendingOps.delete(key);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D3 (Sonnet body): TP toggle → uds.tester_present on (interval) / off. */
  function onTpToggle(): void {
    const t = target();
    const tv = t ? targets.get(t) : undefined;
    if (!t || !tv) return;
    const key = `tp:${t}`;
    if (pendingOps.has(key)) return;
    let iv = 0;
    if (!tv.tpOn) {
      if (!(canOperate() && tv.link === 'up')) return;
      iv = Number(tpIv.value);
      if (!Number.isInteger(iv) || iv < 500 || iv > 30000) {
        ctx.ui.toast('TesterPresent 주기: 500–30000 ms 정수 (ECU S3보다 짧게)');
        return;
      }
    }
    pendingOps.add(key);
    void (async () => {
      try {
        if (tv.tpOn) await ctx.act('tp_off', { target: t, on: false });
        else {
          await ctx.act('tp_on', { target: t, on: true, interval_ms: iv });
          const p = presetOf(t);
          if (p && p.tpIntervalMs !== iv) setPrefs({ ...prefs, presets: prefs.presets.map((x) => (x.id === p.id ? { ...p, tpIntervalMs: iv } : x)) });
        }
      } catch (e) {
        ctx.ui.toast(`uds.tester_present 실패: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        pendingOps.delete(key);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D4 (Sonnet body): [자동 해제] → second click → uds.security_unlock. */
  function onUnlock(): void {
    const t = target();
    const tv = t ? targets.get(t) : undefined;
    if (!t || !tv || !canOperate() || tv.link !== 'up') return;
    const opKey = `unlock:${t}`;
    if (pendingOps.has(opKey)) return;
    const base = 'sv-note sec-msg';
    const hh = (n: number) => n.toString(16).toUpperCase().padStart(2, '0');
    const lv = secLevel.value.trim().replace(/^0x/i, '');
    const level = /^[0-9A-Fa-f]{1,2}$/.test(lv) ? parseInt(lv, 16) : -1;
    if (level < 1 || level > 0x7d || level % 2 === 0) {
      secMsg.className = `${base} err`;
      secMsg.textContent = '레벨: 홀수 hex 01–7D (requestSeed)';
      unlockArm = null;
      return;
    }
    const prov = secProv.value.trim();
    if (!/^[A-Za-z0-9_.-]{1,64}$/.test(prov)) {
      secMsg.className = `${base} err`;
      secMsg.textContent = '키 제공자 이름: 영문·숫자·_.- 1–64자 (loom.toml [[uds.key_providers]])';
      unlockArm = null;
      return;
    }
    const key = `${t}|${level}|${prov}`;
    if (!(unlockArm?.key === key && performance.now() < unlockArm.until)) {
      unlockArm = { key, until: performance.now() + SECOND_ENTER_MS };
      secMsg.className = `${base} warn`;
      secMsg.textContent = `R3 보안 해제: ${t} 레벨 ${hh(level)} · 제공자 ${prov} — ${SECOND_ENTER_MS / 1000}초 안에 다시 누르면 실행 (실패 시 재시도 없음)`;
      uiKey = '';
      ctx.scheduler.invalidate();
      return;
    }
    unlockArm = null;
    secMsg.className = base;
    secMsg.textContent = '';
    pendingOps.add(opKey);
    void (async () => {
      try {
        const h = await ctx.act('unlock', { target: t, level, provider: prov });
        const d = (typeof h.result === 'object' && h.result !== null ? h.result : {}) as Record<string, unknown>;
        secMsg.className = base;
        secMsg.textContent = d.already_unlocked === true ? `레벨 ${hh(level)} 이미 해제됨 (seed 0 — 키 전송 없음)` : `레벨 ${hh(level)} 해제됨`;
        const p = presetOf(t);
        if (p && (p.keyProvider !== prov || p.unlockLevel !== level)) {
          setPrefs({ ...prefs, presets: prefs.presets.map((x) => (x.id === p.id ? { ...p, keyProvider: prov, unlockLevel: level } : x)) });
        }
      } catch (e) {
        secMsg.className = `${base} err`;
        secMsg.textContent = `해제 실패: ${e instanceof Error ? e.message : String(e)} — 자동 재시도 없음`;
      } finally {
        pendingOps.delete(opKey);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D5a (Sonnet body): DB editor text changed → dbCheck + message. */
  function onDbInput(): void {
    dbCheck = dbText.value.trim() ? checkDbText(dbText.value) : null;
    const base = 'sv-note db-msg';
    if (!dbCheck) {
      dbMsg.className = base;
      dbMsg.textContent = '';
    } else if (dbCheck.err !== null) {
      dbMsg.className = `${base} err`;
      dbMsg.textContent = dbCheck.err;
    } else {
      const dids = dbCheck.db?.dids;
      dbMsg.className = base;
      dbMsg.textContent = `${dbCheck.bytes} B · DID ${Array.isArray(dids) ? dids.length : 0}개 — [대상에 적용]하면 daemon이 검증`;
    }
    uiKey = '';
    ctx.scheduler.invalidate();
  }
  /** B4-D5b (Sonnet body): file picked → read text (≤ limit) into the editor. */
  function onDbFile(): void {
    const f = dbFile.files?.[0];
    if (!f) return;
    const fail = (text: string) => {
      dbMsg.className = 'sv-note db-msg err';
      dbMsg.textContent = text;
    };
    if (f.size > MAX_DB_TEXT_BYTES) {
      fail(`파일이 너무 큼 (${f.size} B, 최대 ${MAX_DB_TEXT_BYTES} B)`);
      dbFile.value = '';
      return;
    }
    void f
      .text()
      .then((text) => {
        dbText.value = text;
        onDbInput();
      })
      .catch(() => fail('파일을 읽을 수 없음'))
      .finally(() => {
        dbFile.value = '';
      });
  }
  /** B4-D5c (Sonnet body): saved DB picked → editor. */
  function onDbSaved(): void {
    const s0 = prefs.diagDbs?.find((x) => x.id === dbSaved.value);
    if (s0) {
      dbText.value = s0.json;
      onDbInput();
    }
    dbSaved.value = '';
  }
  /** B4-D5d (Sonnet body): [대상에 적용] → uds.db.set; remember the DB in prefs.diagDbs. */
  function onDbApply(): void {
    const t = target();
    const tv = t ? targets.get(t) : undefined;
    if (!t || !tv || !canOperate() || tv.link === 'closed') return;
    const key = `db_set:${t}`;
    if (pendingOps.has(key)) return;
    const base = 'sv-note db-msg';
    const db = dbCheck?.db;
    if (!db) {
      dbMsg.className = `${base} err`;
      dbMsg.textContent = dbCheck?.err ?? 'DB JSON을 입력하세요';
      return;
    }
    pendingOps.add(key);
    void (async () => {
      try {
        const h = await ctx.act('db_set', { target: t, db });
        const d = (typeof h.result === 'object' && h.result !== null ? h.result : {}) as Record<string, unknown>;
        const rdb = (typeof d.db === 'object' && d.db !== null ? d.db : {}) as Record<string, unknown>;
        const hash = String(rdb.hash);
        const ecu = typeof rdb.ecu === 'string' ? rdb.ecu : null;
        const warnings = Array.isArray(d.warnings) ? d.warnings.filter((w): w is string => typeof w === 'string') : [];
        dbMsg.className = base;
        dbMsg.textContent = `${t}: DB ${hash} 적용 (${ecu ?? '-'}, DID ${String(rdb.dids)}개)${warnings.length ? ` · ${warnings.join(' · ')}` : ''}`;
        const entry: SavedDb = { id: `db-${hash}`, name: ecu ?? hash, json: dbText.value };
        const list = [entry, ...(prefs.diagDbs ?? []).filter((x) => x.id !== entry.id)].slice(0, MAX_SAVED_DBS);
        setPrefs({ ...prefs, diagDbs: list });
      } catch (e) {
        dbMsg.className = `${base} err`;
        dbMsg.textContent = `적용 실패: ${e instanceof Error ? e.message : String(e)}`;
      } finally {
        pendingOps.delete(key);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D5e (Sonnet body): [해제] → uds.db.clear. */
  function onDbClear(): void {
    const t = target();
    if (!t || !canOperate()) return;
    const key = `db_clear:${t}`;
    if (pendingOps.has(key)) return;
    const base = 'sv-note db-msg';
    pendingOps.add(key);
    void (async () => {
      try {
        const h = await ctx.act('db_clear', { target: t });
        const d = (typeof h.result === 'object' && h.result !== null ? h.result : {}) as Record<string, unknown>;
        dbMsg.className = base;
        dbMsg.textContent = `${t}: ${d.cleared === true ? 'DB 해제 — ISO DID 표만 사용' : '적용된 DB 없음'}`;
      } catch (e) {
        dbMsg.className = `${base} err`;
        dbMsg.textContent = `해제 실패: ${e instanceof Error ? e.message : String(e)}`;
      } finally {
        pendingOps.delete(key);
        uiKey = '';
        ctx.scheduler.invalidate();
      }
    })();
  }
  /** B4-D6a (Sonnet body): copy the selected row's seed hex. */
  function onSeedCopy(): void {
    const r = rowById(table.selected);
    const sd = r ? seedFromRow(r) : null;
    if (!sd) return;
    const cb = navigator.clipboard as Clipboard | undefined;
    if (!cb?.writeText) {
      ctx.ui.toast('클립보드 사용 불가');
      return;
    }
    cb.writeText(sd.seedHex).then(
      () => ctx.ui.toast('seed 복사됨'),
      () => ctx.ui.toast('클립보드 사용 불가'),
    );
  }
  /** B4-D6b (Sonnet body): key template of the selected row's level → Raw composer (no send). */
  function onSeedTemplate(): void {
    const r = rowById(table.selected);
    const sd = r ? seedFromRow(r) : null;
    if (!sd) return;
    setMode('raw');
    modeSeg.set('raw');
    rawInput.value = keyTemplate(sd.level);
    persistComposer();
    reqChanged();
    rawInput.focus();
  }
  discBtn.addEventListener('click', onDisconnect);
  forgetBtn.addEventListener('click', onForget);
  tpBtn.addEventListener('click', onTpToggle);
  unlockBtn.addEventListener('click', onUnlock);
  dbText.addEventListener('input', onDbInput);
  dbPick.addEventListener('click', () => dbFile.click());
  dbFile.addEventListener('change', onDbFile);
  dbSaved.addEventListener('change', onDbSaved);
  dbApply.addEventListener('click', onDbApply);
  dbClearBtn.addEventListener('click', onDbClear);
  seedCopy.addEventListener('click', onSeedCopy);
  seedTpl.addEventListener('click', onSeedTemplate);

  function selectTarget(t: string | null) {
    if (selectedTarget === t) return;
    selectedTarget = t;
    reqChanged(); // the preview belongs to a target
  }

  // 연결 설정 editor (local presets only — saving never connects)
  let cfgOpen = false;
  let draft: ConnPreset | null = null;
  let draftBase = '';
  const draftDirty = () => draft !== null && JSON.stringify(draft) !== draftBase;
  const newId = (pfx: string) => `${pfx}-${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`;
  function openCfg(presetId?: string) {
    // re-selecting the unsaved draft keeps it (no silent loss of edits)
    if (presetId && draft && presetId === draft.id && !prefs.presets.some((p) => p.id === presetId)) {
      cfgOpen = true;
      buildCfg();
      uiKey = '';
      ctx.scheduler.invalidate();
      return;
    }
    const t = target();
    const explicit = presetId ? (prefs.presets.find((x) => x.id === presetId) ?? null) : null;
    const p = explicit ?? presetOf(t) ?? (t === null ? (prefs.presets[0] ?? null) : null); // never presets[0] when a target is selected
    if (p) {
      draft = { ...p };
      draftBase = JSON.stringify(p);
    } else if (t !== null) {
      const tv = targets.get(t);
      draft = profileDraftForTarget(t, tv ? { backend: tv.backend, addr: tv.addr } : undefined, newId('p'));
      draftBase = '';
    } else {
      draft = { id: newId('p'), ecu: 'ecu', label: '새 프로필', sim: false, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 };
      draftBase = '';
    }
    cfgOpen = true;
    buildCfg();
    uiKey = '';
    ctx.scheduler.invalidate();
  }
  /** B1-P3d (Sonnet body): [이 대상으로 프로필 만들기] click. */
  function onMakeProfile(): void {
    openCfg();
  }
  /** B1-P2b (Sonnet body): DTC status filter pick. */
  function onDtcFilter(v: DtcFilter): void {
    dtcFilter = v;
    dtcKey = '';
    ctx.scheduler.invalidate();
  }
  function closeCfg(discard: boolean) {
    if (discard) draft = null;
    cfgOpen = false;
    uiKey = '';
    ctx.scheduler.invalidate();
  }
  cfgBtn.addEventListener('click', () => (cfgOpen ? closeCfg(false) : draft ? ((cfgOpen = true), buildCfg(), ctx.scheduler.invalidate()) : openCfg()));
  riBtn.addEventListener('click', () => {
    routeInfo.hidden = !routeInfo.hidden;
    riBtn.setAttribute('aria-expanded', String(!routeInfo.hidden));
    uiKey = '';
    ctx.scheduler.invalidate();
  });

  const cfgIssues = (d: ConnPreset): string[] => {
    const out = presetIssues(d);
    if (!d.label.trim()) out.push('프로필 이름 필요');
    if (prefs.presets.some((p) => p.id !== d.id && p.ecu === d.ecu)) out.push(`ECU 이름 ${d.ecu}: 이미 다른 프로필이 사용 중`);
    return out;
  };
  function buildCfg() {
    if (!draft) return;
    const d = draft;
    const head = el('div', 'cfg-head');
    const title = el('b', 'cfg-title');
    title.id = `${uid}-cfgt`;
    cfgPanel.setAttribute('aria-labelledby', title.id);
    title.append('연결 설정 · ', el('span', undefined, `${d.ecu} · DoIP`));
    const selLab = el('label', 'cf-l', '편집 대상');
    const sel = el('select', 'fld cfg-sel');
    sel.id = `${uid}-cfgsel`;
    selLab.htmlFor = sel.id;
    for (const p of prefs.presets) sel.append(Object.assign(el('option', undefined, `${p.sim ? 'SIM' : 'DoIP'} · ${p.label} (${p.ecu})`), { value: p.id }));
    if (!prefs.presets.some((p) => p.id === d.id)) sel.append(Object.assign(el('option', undefined, '새 프로필 (저장 안 됨)'), { value: d.id }));
    sel.value = d.id;
    sel.addEventListener('change', () => openCfg(sel.value));
    const src = el('span', 'cfg-src', prefs.presets.some((p) => p.id === d.id) ? `id ${d.id}` : '저장 안 됨');
    const dup = button('복제', 'btn', '편집 대상을 새 프로필 초안으로 복제');
    dup.addEventListener('click', () => {
      draft = { ...d, id: newId('p'), ecu: `${d.ecu}_2`.slice(0, 64), label: `${d.label} 복사` };
      draftBase = '';
      buildCfg();
      ctx.scheduler.invalidate();
    });
    const add = button('새 프로필', 'btn', '빈 DoIP 프로필 초안');
    add.addEventListener('click', () => {
      draft = { id: newId('p'), ecu: 'ecu_new', label: '새 프로필', sim: false, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 };
      draftBase = '';
      buildCfg();
      ctx.scheduler.invalidate();
    });
    const del = button('삭제', 'btn danger', '이 프로필 삭제 (연결된 대상에는 영향 없음)');
    del.disabled = !prefs.presets.some((p) => p.id === d.id) || prefs.presets.length <= 1;
    del.addEventListener('click', () => {
      setPrefs({ ...prefs, presets: prefs.presets.filter((p) => p.id !== d.id) });
      draft = null;
      openCfg();
    });
    head.append(title, selLab, sel, src, el('span', 'grow'), dup, add, del);

    const errs = new Map<string, HTMLElement>();
    const field = (k: keyof ConnPreset, label: string, hint: string, o: { wide?: boolean; pfx?: string; num?: boolean } = {}) => {
      const w = el('div', `cf${o.wide ? ' wide' : ''}`);
      w.dataset.f = k;
      const id = `${uid}-cf-${k}`;
      const l = el('label', 'cf-l', label);
      l.htmlFor = id;
      const wrap = el('span', 'cf-in');
      if (o.pfx) wrap.append(el('span', 'pfx', o.pfx));
      const inp = el('input', 'fld');
      inp.id = id;
      inp.dataset.k = k;
      inp.spellcheck = false;
      inp.autocomplete = 'off';
      if (o.num) inp.inputMode = 'numeric';
      const raw = d[k];
      inp.value = o.pfx && typeof raw === 'string' ? raw.replace(/^0x/i, '') : String(raw);
      inp.disabled = d.sim && (k === 'host' || k === 'port' || k === 'la' || k === 'testerLa' || k === 'activation');
      inp.addEventListener('input', () => {
        const v = inp.value.trim();
        const next = { ...draft! } as Record<string, unknown>;
        next[k] = o.num ? (/^\d+$/.test(v) ? Number(v) : NaN) : o.pfx ? `0x${v}` : inp.value;
        draft = next as unknown as ConnPreset;
        syncCfgStatus();
      });
      wrap.append(inp);
      const err = el('small', 'cf-err');
      err.setAttribute('role', 'status');
      errs.set(k, err);
      w.append(l, wrap, el('small', 'cf-hint', hint), err);
      return w;
    };
    const body = el('div', 'cfg-body');
    const fsProf = el('fieldset', 'cfg-fs');
    fsProf.append(el('legend', undefined, '프로필'), field('label', '이름', '1–80자', { wide: true }), field('ecu', 'ECU 이름 (uds.connect name)', '영문·숫자·_.- 1–64자'));
    const fsRoute = el('fieldset', 'cfg-fs');
    const kindW = el('div', 'cf');
    const kindL = el('label', 'cf-l', '엔티티');
    const kind = el('select', 'fld');
    kind.id = `${uid}-cf-kind`;
    kindL.htmlFor = kind.id;
    kind.append(Object.assign(el('option', undefined, 'DoIP 엔티티 (실제 ECU)'), { value: 'doip' }), Object.assign(el('option', undefined, 'SIM bench (daemon 내장 가상 ECU)'), { value: 'sim' }));
    kind.value = d.sim ? 'sim' : 'doip';
    kind.addEventListener('change', () => {
      draft = { ...draft!, sim: kind.value === 'sim' };
      buildCfg();
    });
    kindW.append(kindL, kind, el('small', 'cf-hint', d.sim ? 'SIM: 주소 무시 · 실제 소켓 없음' : '실제 DoIP TCP 연결'));
    fsRoute.append(
      el('legend', undefined, 'DoIP 경로'),
      kindW,
      field('host', 'DoIP 엔티티 호스트', 'IPv4 또는 호스트 이름', { wide: true }),
      field('port', 'TCP 포트', '1–65535 · 기본 13400', { num: true }),
      field('testerLa', 'Tester 논리 주소', '16-bit hex', { pfx: '0x' }),
      field('la', 'ECU 논리 주소', '16-bit hex', { pfx: '0x' }),
      field('activation', '활성화 타입', '0–255 (routing activation)', { num: true }),
    );
    const fsTime = el('fieldset', 'cfg-fs');
    fsTime.append(el('legend', undefined, '연결 타이밍'), field('timeoutMs', '연결 타임아웃 (ms)', '100–60000', { num: true }), el('p', 'cf-note', 'P2/P2*는 daemon이 보고한 값(uds.state)을 표시합니다.'));
    body.append(fsProf, fsRoute, fsTime);

    const foot = el('div', 'cfg-foot');
    const st = el('span', 'cfg-st');
    st.setAttribute('aria-live', 'polite');
    const revert = button('되돌리기', 'btn');
    revert.addEventListener('click', () => openCfg(d.id));
    const cancel = button('취소', 'btn', '초안 폐기 후 닫기');
    cancel.addEventListener('click', () => closeCfg(true));
    const save = button('저장', 'btn primary', '로컬 프로필 저장 — 연결·전송하지 않음');
    save.type = 'submit';
    foot.append(st, el('span', 'grow'), revert, cancel, save);
    const note = el('p', 'cfg-note', '프로필은 이 브라우저의 로컬 설정이며 다음 [연결]부터 사용됩니다. 저장은 연결하지 않고, 이미 연결된 대상의 실제 주소는 daemon 상태(주소 상세)가 기준입니다.');
    cfgPanel.replaceChildren(head, body, foot, note);

    function syncCfgStatus() {
      const dd = draft!;
      const list = cfgIssues(dd);
      for (const [k, e] of errs) {
        const map: Record<string, RegExp> = { ecu: /ECU 이름/, host: /호스트/, port: /포트/, la: /^ECU 논리/, testerLa: /Tester/, activation: /활성화/, timeoutMs: /타임아웃/, label: /프로필 이름/ };
        e.textContent = list.filter((m) => map[k]?.test(m)).join(' · ');
      }
      st.replaceChildren();
      if (list.length) {
        st.append(el('span', 'err', `저장 불가 — ${list.length}개 문제`));
        const ul = el('ul');
        for (const m of list) ul.append(el('li', undefined, m));
        st.append(ul);
      } else st.append(el('span', draftDirty() ? 'dirty' : 'ok', draftDirty() ? '저장 안 된 변경 있음' : '저장됨 · 유효'));
      save.disabled = list.length > 0 || !draftDirty();
      revert.disabled = !draftDirty() || !prefs.presets.some((p) => p.id === dd.id);
      cfgDirty.hidden = !draftDirty();
    }
    syncCfgStatus();
  }
  cfgPanel.addEventListener('submit', (e) => {
    e.preventDefault();
    if (!draft || cfgIssues(draft).length) return;
    const d = { ...draft, label: draft.label.trim() };
    const exists = prefs.presets.some((p) => p.id === d.id);
    setPrefs({ ...prefs, presets: exists ? prefs.presets.map((p) => (p.id === d.id ? d : p)) : [...prefs.presets, d] });
    draft = { ...d };
    draftBase = JSON.stringify(d);
    if (!targets.has(d.ecu)) selectTarget(d.ecu);
    buildCfg();
  });
  cfgPanel.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeCfg(false);
      cfgBtn.focus();
    }
  });

  // =============================== tools drawer ===============================
  let tool: ToolId | null = null;
  function openTool(id: ToolId | null) {
    tool = id;
    for (const [k, b] of tabBtns) b.setAttribute('aria-selected', String(k === id));
    for (const [k, s] of tdPanels) s.hidden = k !== id;
    drawer.hidden = id === null;
    tdTitle.textContent = id ? (TOOLS.find((t) => t[0] === id)?.[1] ?? '') : '';
    if (id) mod.style.setProperty('--td-top', `${ctxbar.offsetHeight || 70}px`);
    tools.render(true);
    ctx.scheduler.invalidate();
  }
  tdClose.addEventListener('click', () => openTool(null));
  drawer.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      const back = tool ? tabBtns.get(tool) : null;
      openTool(null);
      back?.focus();
    }
  });

  const api: ToolsApi = {
    ctx,
    prefs: () => prefs,
    setPrefs,
    target,
    writable,
    canOperate,
    link: (t) => linkOf(t),
    rows: () => rows.list(),
    rowNo: (id) => rowNo.get(id) ?? null,
    reveal: (id) => {
      revealRow(id);
    },
    fillDid: (did) => {
      setMode('did');
      modeSeg.set('did');
      didInput.value = did;
      persistComposer();
      reqChanged();
      didInput.focus();
    },
    fillRaw: (hex) => {
      setMode('raw');
      modeSeg.set('raw');
      rawInput.value = spacedHex(hex);
      persistComposer();
      syncFromHex();
      reqChanged();
      rawInput.focus();
    },
    current: () => composed().hex,
    send: sendOnce,
    openCfg: (id) => {
      openTool(null);
      openCfg(id);
    },
    selectTarget,
    targets: () => [...targets.values()],
    invalidate: () => ctx.scheduler.invalidate(),
  };
  const tools = createTools(api, {
    catalog: tdPanels.get('catalog')!,
    workflow: tdPanels.get('workflow')!,
    watch: tdPanels.get('watch')!,
    project: tdPanels.get('project')!,
    isOpen: (id) => tool === id,
  });

  // =============================== layout prefs ===============================
  const applyLayout = () => {
    wsp.classList.toggle('did-collapsed', prefs.didCollapsed);
    wsp.classList.toggle('vals-collapsed', prefs.valsCollapsed);
    didPane.hidden = prefs.didCollapsed;
    didRail.hidden = !prefs.didCollapsed;
    vals.hidden = prefs.valsCollapsed;
    valsRail.hidden = !prefs.valsCollapsed;
    center.style.setProperty('--hist-h', `${prefs.histPct}%`);
    divHist.setAttribute('aria-valuenow', String(Math.round(prefs.histPct)));
  };
  didCollapse.addEventListener('click', () => setPrefs({ ...prefs, didCollapsed: true }));
  didRail.addEventListener('click', () => setPrefs({ ...prefs, didCollapsed: false }));
  valsCollapse.addEventListener('click', () => setPrefs({ ...prefs, valsCollapsed: true }));
  valsRail.addEventListener('click', () => setPrefs({ ...prefs, valsCollapsed: false }));
  divider(divHist, {
    orientation: 'horizontal',
    label: '기록/상세 경계 크기 조절 (↑↓)',
    container: () => center,
    get: () => prefs.histPct,
    set: (pct, persist) => {
      prefs = { ...prefs, histPct: pct };
      if (persist) savePrefs(prefs);
      applyLayout();
    },
    def: 42,
    min: 15,
    max: 85,
  });

  // =============================== history header actions ===============================
  const exportRows = () =>
    visibleRows().map((r) => {
      const rr = rowRoute(r.payload);
      const sn = (v: unknown): string | null => (typeof v === 'string' ? v : null);
      return {
      no: rowNo.get(r.requestId) ?? null,
      request_id: r.requestId,
      time: new Date(r.firstWallMs || 0).toISOString(),
      target: rowTarget(r),
      generation: rr.generation,
      backend: rr.known ? sn(r.payload.backend) : null,
      addr: rr.known ? sn(r.payload.addr) : null,
      db_hash: sn(r.payload.db_hash),
      purpose: sn(r.payload.purpose),
      origin: r.origin,
      state: r.state,
      service: str(r.payload.service) || null,
      request: reqHexOf(r),
      response: respHexOf(r),
      nrc: r.payload.nrc ?? null,
      pending_count: Number(r.payload.pending_count) || 0,
      rtt_ms: typeof r.payload.rtt_ms === 'number' ? r.payload.rtt_ms : null,
      result: resultSymbol(r).text,
      };
    });
  const stamp = () => new Date().toISOString().replace(/[:.]/g, '-');
  expJson.addEventListener('click', () => {
    const ok = download(`uds-history-${stamp()}.json`, JSON.stringify({ format: 'loom.uds.history', version: 2, rows: exportRows() }, null, 2), 'application/json');
    if (!ok) ctx.ui.toast('내보내기 실패: 이 환경에서 다운로드를 만들 수 없음');
  });
  expCsv.addEventListener('click', () => {
    const rs = exportRows();
    const cols = Object.keys(rs[0] ?? { no: 0, request_id: '', time: '', target: '', generation: 0, backend: '', addr: '', db_hash: '', purpose: '', origin: '', state: '', service: '', request: '', response: '', nrc: '', pending_count: 0, rtt_ms: 0, result: '' });
    const q = (v: unknown) => {
      const s0 = v === null || v === undefined ? '' : String(v);
      return /[",\n]/.test(s0) ? `"${s0.replace(/"/g, '""')}"` : s0;
    };
    const text = [cols.join(','), ...rs.map((r) => cols.map((c) => q((r as Record<string, unknown>)[c])).join(','))].join('\n');
    if (!download(`uds-history-${stamp()}.csv`, text, 'text/csv')) ctx.ui.toast('내보내기 실패: 이 환경에서 다운로드를 만들 수 없음');
  });
  clearBtn.addEventListener('click', () => {
    for (const r of rows.list()) if (isFinal(r.state)) hidden.add(r.requestId);
    if (table.selected && hidden.has(table.selected)) table.select(null);
    ctx.scheduler.invalidate();
  });

  // =============================== detail panes ===============================
  let selEv = -1;
  let selField = -1;
  let detailKey = '';
  const DIR_CLS: Record<SeqEvent['kind'], string> = { tx: 'k-tx', pending: 'k-pending', positive: 'k-positive', nrc: 'k-nrc', timeout: 'k-timeout', link: 'k-link', cancel: 'k-cancel', late: 'k-late', wait: 'k-pending' };
  const renderDetail = (now: number) => {
    const cols = hexRowWidth(hexEl.clientWidth);
    const r = rowById(table.selected);
    const evs = r ? seqEvents(r, now) : [];
    const key = r ? `${r.requestId}|${r.updates}|${r.state}|${selEv}|${selField}|${isFinal(r.state) ? '' : Math.floor(now / 500)}|${cols}` : `none|${cols}`;
    if (key === detailKey) return;
    detailKey = key;
    if (!r) {
      seqHead.textContent = fieldsHead.textContent = '';
      seqList.replaceChildren(el('li', 'empty', '트랜잭션을 선택하면 Tx·0x78·최종 응답·타임아웃 순서가 표시됩니다.'));
      fields.replaceChildren(el('div', 'empty', '선택한 트랜잭션의 요청·응답 해석이 여기에 표시됩니다.'));
      hexEl.replaceChildren(el('div', 'empty', 'UDS PDU 바이트'));
      return;
    }
    const no = rowNo.get(r.requestId);
    seqHead.textContent = `#${no ?? '?'} · ${rowTarget(r)} · ${rowRoute(r.payload).text}`;
    // the detail follows the latest PDU unless the user picked an event
    const withBytes = evs.map((e, i) => (e.hex ? i : -1)).filter((i) => i >= 0);
    const pick = selEv >= 0 && evs[selEv]?.hex ? selEv : (withBytes[withBytes.length - 1] ?? 0);
    seqList.replaceChildren(
      ...evs.map((e, i) => {
        const li = el('li', `${DIR_CLS[e.kind]}${e.hex ? '' : ' nobytes'}`);
        li.setAttribute('role', 'option');
        li.dataset.i = String(i);
        li.setAttribute('aria-selected', String(i === pick));
        li.append(el('span', 'dt', e.dt), el('span', 'dir', e.dir), el('span', 'lb', e.label));
        if (e.hex) li.append(el('span', 'by', spacedHex(e.hex)));
        if (e.note) li.append(el('span', 'nt', e.note));
        li.addEventListener('click', () => {
          if (!e.hex) return;
          selEv = i;
          selField = -1;
          ctx.scheduler.invalidate();
        });
        return li;
      }),
    );
    const ev = evs[pick]!;
    fieldsHead.textContent = ev.label;
    const t = targets.get(rowTarget(r));
    const snap = el('dl', 'fsnap');
    for (const [k, v] of detailPairs(r, t)) {
      const d = el('div', 'disp-row');
      d.style.display = 'contents';
      d.append(el('dt', undefined, k), el('dd', undefined, v));
      snap.append(d);
    }
    const out: HTMLElement[] = [snap];
    if (ev.hex) {
      const fl = pduFields(ev.hex, ev.kind === 'tx' ? 'tx' : 'rx', r.payload);
      const tab = el('table', 'ftab');
      const th = el('tr');
      for (const h of ['이름', '오프셋', '값', '설명']) th.append(el('th', undefined, h));
      const thead = el('thead');
      thead.append(th);
      const tb = el('tbody');
      const pr = el('tr', 'pdu-row');
      pr.append(el('td', undefined, ev.kind === 'tx' ? 'REQUEST_PDU' : 'RESPONSE_PDU'), el('td'), el('td', 'v', ev.hex.length > 128 ? `${spacedHex(ev.hex.slice(0, 128))} … (바이트 패널 참조)` : spacedHex(ev.hex)), el('td', undefined, `${ev.hex.length / 2} B`));
      tb.append(pr);
      fl.forEach((f, i) => {
        const tr = el('tr', `fd${f.decoded ? ' dec' : ''}${selField === i ? ' on' : ''}`);
        tr.dataset.f = String(i);
        tr.append(el('td', undefined, f.name), el('td', 'o', f.len ? `${f.off}${f.len > 1 ? `–${f.off + f.len - 1}` : ''}` : '—'), el('td', `v${f.bad ? ' bad' : ''}`, f.value), el('td', `n${f.bad ? ' bad' : ''}`, f.note));
        tr.addEventListener('mouseenter', () => markBytes(f.len ? f.off : -1, f.len));
        tr.addEventListener('mouseleave', () => markBytes(selField >= 0 ? (fl[selField]?.off ?? -1) : -1, selField >= 0 ? (fl[selField]?.len ?? 0) : 0));
        tr.addEventListener('click', () => {
          selField = selField === i ? -1 : i;
          ctx.scheduler.invalidate();
        });
        tb.append(tr);
      });
      tab.append(thead, tb);
      out.push(tab);
    }
    out.push(appliedNote(r));
    const sd = seedFromRow(r);
    if (sd) {
      seedCode.textContent = spacedHex(sd.seedHex) + (sd.zero ? ' (0 — 이미 해제됨)' : '');
      seedTpl.disabled = sd.zero;
      out.push(seedBar);
    }
    fields.replaceChildren(...out);
    // bytes
    if (!ev.hex) {
      hexEl.replaceChildren(el('div', 'empty', '이 이벤트에는 PDU 바이트가 없습니다 (전송·수신 바이트 없음).'));
      return;
    }
    const fl = pduFields(ev.hex, ev.kind === 'tx' ? 'tx' : 'rx', r.payload);
    const sf = selField >= 0 ? fl[selField] : undefined;
    const on = (o: number) => !!sf && sf.len > 0 && o >= sf.off && o < sf.off + sf.len;
    const b = ev.hex.match(/../g) ?? [];
    const kids: HTMLElement[] = [el('div', 'hx-cap', `${ev.label} · ${b.length} B · UDS PDU (DoIP/TCP 프레임 아님)`)];
    for (let o = 0; o < b.length; o += cols) {
      const rowEl = el('div', 'hx-row');
      const hh = el('span', 'hx-h');
      const aa = el('span', 'hx-a');
      b.slice(o, o + cols).forEach((x, j) => {
        const n = parseInt(x, 16);
        const hb = el('b', on(o + j) ? 'on' : undefined, x);
        hb.dataset.o = String(o + j);
        const ab = el('b', `${n >= 0x20 && n < 0x7f ? '' : 'np'}${on(o + j) ? ' on' : ''}`.trim() || undefined, n >= 0x20 && n < 0x7f ? String.fromCharCode(n) : '.');
        ab.dataset.o = String(o + j);
        hh.append(hb);
        aa.append(ab);
      });
      rowEl.append(el('span', 'hx-off', o.toString(16).padStart(4, '0')), hh, aa);
      kids.push(rowEl);
    }
    hexEl.replaceChildren(...kids);
  };
  function markBytes(off: number, len: number) {
    for (const b of hexEl.querySelectorAll<HTMLElement>('b[data-o]')) {
      const o = Number(b.dataset.o);
      b.classList.toggle('on', off >= 0 && o >= off && o < off + len);
    }
  }
  const appliedNote = (r: RowLike): HTMLElement => {
    const p = r.payload;
    if (isDenied(p)) return el('div', 'applied none', '거부됨 — 전송 안 됨, 확정 값과 무관');
    if (!isFinal(r.state)) return el('div', 'applied none', '진행 중 — 최종 응답 전에는 확정 값을 바꾸지 않음');
    if (p.ok_suppressed === true) return el('div', 'applied none', '응답 억제 (SPRMIB) — 긍정 응답 바이트 없음, 확정 값과 무관');
    if (r.state === 'failed' || r.state === 'outcome_unknown') return el('div', 'applied none', `${resultSymbol(r).text} — 확정 값 갱신 없음 (이전 값이 있으면 이전 출처 그대로 유지)`);
    const dec = Array.isArray(p.decoded) ? p.decoded.length : 0;
    if (!dec) return el('div', 'applied none', 'daemon 해석(decoded) 없음 — 확정 값 갱신 없음');
    const t = rowTarget(r);
    const applied = (p.decoded as unknown[]).some((d) => {
      const did = str((d as Record<string, unknown>)?.did);
      return lastValues.get(`${t}:${did}`)?.requestId === r.requestId;
    });
    return el('div', `applied ${applied ? 'yes' : 'no'}`, applied ? `확정 값에 반영됨 (${dec}개 DID)` : '값 미반영 — 더 최근 요청의 값이 확정 값으로 남아 있음');
  };
  seqList.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const items = [...seqList.querySelectorAll<HTMLElement>('li[data-i]:not(.nobytes)')].map((li) => Number(li.dataset.i));
    if (!items.length) return;
    e.preventDefault();
    const cur = items.indexOf(selEv);
    const nx = e.key === 'ArrowDown' ? items[Math.min(items.length - 1, cur + 1)] : items[Math.max(0, cur < 0 ? items.length - 1 : cur - 1)];
    selEv = nx ?? -1;
    selField = -1;
    ctx.scheduler.invalidate();
  });
  copyHex.addEventListener('click', () => {
    const r = rowById(table.selected);
    if (!r) return;
    const evs = seqEvents(r, performance.now());
    const ev = (selEv >= 0 && evs[selEv]?.hex ? evs[selEv] : [...evs].reverse().find((x) => x.hex)) ?? null;
    if (!ev?.hex) return;
    const text = spacedHex(ev.hex);
    const done = () => ctx.ui.toast(`복사됨: ${text.length > 40 ? `${text.slice(0, 40)}…` : text}`);
    try {
      void navigator.clipboard.writeText(text).then(done, () => ctx.ui.toast('복사 실패: 클립보드 권한 없음'));
    } catch {
      ctx.ui.toast('복사 실패: 클립보드 없음');
    }
  });

  // =============================== confirmed values ===============================
  let valsKey = '';
  const renderVals = () => {
    const cur = target();
    const key = `${rowsVersion}|${cur}|${prefs.dids.length}|${uiKey}`;
    if (key === valsKey) return;
    valsKey = key;
    valsEcu.textContent = cur ?? '';
    if (!cur) {
      valsBody.replaceChildren(el('div', 'cv', 'ECU를 선택하세요'));
      return;
    }
    const dids = prefs.dids.map((d) => d.did);
    for (const v of lastValues.values()) if (v.target === cur && !dids.includes(v.did)) dids.push(v.did);
    const list = rows.list();
    const latestFor = (did: string) => {
      for (let i = list.length - 1; i >= 0; i--) {
        const r = list[i]!;
        if (rowTarget(r) === cur && rowDid(r) === did) return r;
      }
      return null;
    };
    valsBody.replaceChildren(
      ...dids.map((did) => {
        const v = lastValues.get(`${cur}:${did}`);
        const name = v?.name || didNameOf(did) || `DID ${did}`;
        const c = el('div', 'cv');
        c.dataset.did = `${cur}:${did}`;
        const h = el('div', 'cv-h');
        h.append(el('b', undefined, did), el('span', undefined, name));
        c.append(h);
        if (v) {
          c.append(el('div', 'cv-v', `${v.value}${v.unit ? ` ${v.unit}` : ''}`));
          const p = el('div', 'cv-p');
          const a = el('a', undefined, `#${rowNo.get(v.requestId) ?? '?'}`);
          a.dataset.goto = v.requestId;
          a.title = '출처 트랜잭션 선택';
          a.addEventListener('click', () => revealRow(v.requestId));
          p.append('출처 ', a, ` · ${fmtClock(v.wallMs)} · ${rowRoute(rowById(v.requestId)?.payload ?? {}).text} · ${v.origin}`);
          c.append(p);
        } else c.append(el('div', 'cv-v none', '확정 값 없음'));
        const latest = latestFor(did);
        if (latest && !isFinal(latest.state)) c.append(el('div', 'cv-i', `읽는 중 #${rowNo.get(latest.requestId) ?? '?'}`));
        else if (latest && v?.requestId !== latest.requestId && (latest.state === 'failed' || latest.state === 'outcome_unknown' || resultSymbol(latest).cls === 'res-nrc'))
          c.append(el('div', 'cv-w', `최근 #${rowNo.get(latest.requestId) ?? '?'} ${resultSymbol(latest).text} — ${v ? '위는 이전 값' : '확정 값 없음'}`));
        return c;
      }),
    );
  };

  // =============================== DID list ===============================
  let didKey = '';
  let didKb = -1;
  const renderDidList = () => {
    const v = normalizeDid(didInput.value).did;
    const q = didSearch.value.trim().toLowerCase().replace(/^0x/, '');
    const key = `${q}|${v}|${didKb}|${JSON.stringify(prefs.dids)}`;
    if (key === didKey) return;
    didKey = key;
    const list = prefs.dids.filter((d) => !q || d.did.toLowerCase().includes(q) || d.name.toLowerCase().includes(q) || d.note.toLowerCase().includes(q));
    didList.replaceChildren(
      ...(list.length
        ? list.map((d, i) => {
            const li = el('li');
            li.setAttribute('role', 'option');
            li.dataset.did = d.did;
            li.setAttribute('aria-selected', String(d.did === v));
            if (i === didKb) li.classList.add('kb');
            const raw = el('span', 'raw');
            raw.append('22 ', el('b', undefined, `${d.did.slice(0, 2)} ${d.did.slice(2)}`));
            li.append(raw, el('span', 'nm', d.name), el('span', 'sub', d.note));
            li.addEventListener('click', () => pickDid(d.did));
            return li;
          })
        : [Object.assign(el('li', 'none', '일치하는 DID 없음 — 요청 편집기에 hex 직접 입력'), { role: 'presentation' })]),
    );
    const qv = didQuick.value;
    didQuick.replaceChildren(Object.assign(el('option', undefined, '빠른 선택…'), { value: '' }), ...prefs.dids.map((d) => Object.assign(el('option', undefined, `${d.did} · ${d.name}`), { value: d.did })));
    didQuick.value = v && prefs.dids.some((d) => d.did === v) ? v : qv && prefs.dids.some((d) => d.did === qv) ? '' : '';
  };
  function pickDid(did: string) {
    if (mode !== 'did') {
      setMode('did');
      modeSeg.set('did');
    }
    didInput.value = did;
    persistComposer();
    reqChanged();
  }
  didSearch.addEventListener('input', () => {
    didKb = -1;
    ctx.scheduler.invalidate();
  });
  didList.addEventListener('keydown', (e) => {
    const items = [...didList.querySelectorAll<HTMLElement>('li[data-did]')];
    if (!items.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      didKb = Math.max(0, Math.min(items.length - 1, didKb + (e.key === 'ArrowDown' ? 1 : -1)));
      pickDid(items[didKb]!.dataset.did!);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      didInput.focus();
    }
  });

  // =============================== keyboard ===============================
  root.addEventListener('keydown', (e) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA' || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = target();
    if (!t) return;
    if (e.key === 'T') {
      e.preventDefault();
      void act('takeover', { target: t });
    } else if (e.key === '!') {
      e.preventDefault();
      void act('abort', { target: t });
    }
  });

  // =============================== data ===============================
  const offRows = rows.onChange(() => {
    rowsVersion++;
    ctx.scheduler.invalidate();
  });
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const tick = setInterval(() => {
    if (rows.list().some((r) => !isFinal(r.state))) ctx.scheduler.invalidate();
  }, 250); // elapsed counters of in-flight rows keep moving

  /** uds.state for one target with a per-target seq guard (older backlog/control rows never overwrite) */
  function applyState(ev: LoomEvent): boolean {
    if (ev.kind !== 'uds.state') return false;
    const name = str((ev.payload as Record<string, unknown>).target) || ev.channel;
    if (!name || (targetScope && !targetScope.has(name))) return false;
    let seq: bigint;
    try {
      seq = BigInt(ev.seq);
    } catch {
      return false;
    }
    if (seq <= (targetSeq.get(name) ?? -1n)) return false;
    if (isForgotten(ev.payload)) {
      targets.delete(name);
      targetSeq.set(name, seq);
      if (selectedTarget === name) selectedTarget = null;
      uiKey = '';
      linksKey = '';
      return true;
    }
    noteTarget(name);
    const next = reduceTarget(targets.get(name), ev);
    if (!next || !targets.has(name)) return false;
    targets.set(name, next);
    targetSeq.set(name, seq);
    uiKey = '';
    return true;
  }
  /**
   * Bound panel (`targets` prop) mounted after its target connected: the shell opens the bound panel before it removes
   * the seeded unbound one, so the SDK mux joins the still-open `{domains:['uds']}` entry and replays THAT entry's
   * snapshot, taken before the connect. The live `uds.state` (a priority kind) went out on the control path before this
   * panel existed, so neither the mirror nor onControl will ever carry it. After the first native snapshot has been
   * applied, ask the daemon once (read-only `uds.status`, same StatePayload as `uds.state`) and fill only targets in
   * scope whose state did not change meanwhile. No polling, never for an unbound panel, never a connect.
   * Seq guard: everything this panel had seen when it asked (snapshot high water, mirror head, applied states) is no
   * newer than the reply, so the reply is stamped with that maximum — a later fresh snapshot (h ≥ stamp) still
   * replaces it, a later live uds.state (seq > stamp) still wins, and a state applied while the call was in flight
   * (live link down, newer snapshot) makes the reply for that target stale and it is dropped.
   */
  function hydrateFromStatus(): void {
    if (!targetScope || statusHydration !== 'idle') return;
    statusHydration = 'asked';
    const askedVersion = snapshotVersion;
    const askedSeq = new Map(targetSeq);
    let stamp = mirror.snapshot?.throughSeq ?? -1n;
    if (mirror.headSeq > stamp) stamp = mirror.headSeq;
    for (const s of targetSeq.values()) if (s > stamp) stamp = s;
    let call: Promise<unknown>;
    try {
      call = ctx.query<unknown>('uds.status', {});
    } catch {
      statusHydration = 'done';
      return;
    }
    call.then(
      (r) => {
        statusHydration = 'done';
        if (disposed || askedVersion !== snapshotVersion) return; // a newer native snapshot replaced the view meanwhile
        const list = (r as { targets?: unknown } | null)?.targets;
        if (!Array.isArray(list)) return;
        let changed = false;
        for (const payload of list) {
          const next = reduceTargetState(undefined, payload);
          if (!next || !targetScope.has(next.target)) continue;
          if (targetSeq.get(next.target) !== askedSeq.get(next.target)) continue; // newer live/snapshot state wins
          noteTarget(next.target);
          if (!targets.has(next.target)) continue;
          targets.set(next.target, next);
          targetSeq.set(next.target, stamp);
          changed = true;
        }
        if (changed) {
          uiKey = '';
          ctx.scheduler.invalidate();
        }
      },
      () => {
        statusHydration = 'done'; // no inference: the link stays unreported
      },
    );
  }
  const noteTarget = (name: string) => {
    if (targets.has(name)) return;
    if (targets.size >= MAX_TARGETS) {
      targetsOverflow++;
      return;
    }
    targets.set(name, emptyTarget(name));
  };

  const LINK_HEAD = ['링크', '대상', '전송', '주소', '세션', '보안', 'TesterPresent', 'P2/P2*', '환경', '제어', ''];
  let linksKey = '';
  const tpCount = () => {
    const by = new Map<string, number>();
    for (const r of tpRows.list()) {
      const t = rowTarget(r) || '?';
      by.set(t, (by.get(t) ?? 0) + 1);
    }
    return by;
  };
  const renderTargets = (tpBy: Map<string, number>) => {
    const env = ctx.env ?? '?';
    const w = canOperate();
    const cur = target();
    const views = [...targets.values()].map((t) => ({ t, c: targetCells(t, tpBy.get(t.target) ?? 0, env), line: targetLine(t, tpBy.get(t.target) ?? 0, env), lease: leaseText(t, ctx.connId) }));
    const key = JSON.stringify([w, cur, targetsOverflow, views.map((v) => [v.line, v.lease, v.c.link.text])]);
    if (key === linksKey) return;
    linksKey = key;
    const head = el('div', 'uds-ds-row uds-ds-head');
    head.setAttribute('role', 'row');
    for (const h of LINK_HEAD) {
      const c = colHead(h);
      if (h === 'TesterPresent') c.title = 'TesterPresent (0x3E) 유지 신호 — ISO-TP/CAN-TP 전송 상태가 아님';
      head.append(c);
    }
    const children: HTMLElement[] = [head];
    for (const v of views) {
      const row = el('div', 'uds-ds-row uds-ds-link-row');
      row.setAttribute('role', 'row');
      row.dataset.target = v.t.target;
      row.title = v.line;
      row.setAttribute('aria-selected', String(v.t.target === cur));
      const c = (text: string, col: string, cls = '') => {
        const s0 = el('span', `mono ${cls}`.trim(), text);
        s0.setAttribute('role', 'cell');
        s0.dataset.col = col;
        return s0;
      };
      const link = c(v.c.link.text, 'link', v.c.link.cls);
      if (v.c.link.title) link.title = v.c.link.title;
      const tp = c(v.c.testerPresent, 'tester-present');
      tp.title = 'TesterPresent (0x3E) — ISO-TP/CAN-TP 아님';
      const take = button('제어 가져오기 T', 'btn');
      take.disabled = !w;
      take.addEventListener('click', (e) => {
        e.stopPropagation();
        void act('takeover', { target: v.t.target });
      });
      const takeCell = el('span');
      takeCell.setAttribute('role', 'cell');
      takeCell.append(take);
      row.append(link, c(v.t.target, 'target', 'uds-ds-strong'), c(v.c.backend, 'backend'), c(v.c.addr, 'addr'), c(v.c.session, 'session'), c(v.c.security, 'security'), tp, c(v.c.p2, 'p2'), c(v.c.env, 'env'), c(v.lease, 'lease', 'lease'), takeCell);
      row.addEventListener('click', () => selectTarget(v.t.target));
      children.push(row);
    }
    if (views.length === 0) children.push(el('div', 'uds-ds-empty', '연결된 대상 없음 — [연결] 또는 에이전트의 loom uds connect로 연결하면 나타납니다'));
    if (targetsOverflow) children.push(el('div', 'uds-ds-empty', `+${targetsOverflow} 대상 표시 안 됨`));
    linkGrid.replaceChildren(...children);
  };

  // ---- context strip / composer state (cheap; keyed) ----
  let uiKey = '';
  let lastNeedsConnect: boolean | null = null;
  const renderUi = (tpBy: Map<string, number>, now: number) => {
    const nc = needsConnect();
    if (nc !== lastNeedsConnect) {
      const was = lastNeedsConnect;
      lastNeedsConnect = nc;
      if (was === true && !nc) reqChanged(); // target now reported by the daemon → fresh dry-run before any send
    }
    const cur = target();
    const tv = cur ? targets.get(cur) : undefined;
    const pr = presetOf(cur);
    const inflight = inflightFor(cur);
    const comp = composed();
    const key = JSON.stringify([cur, tv, pr, [...targets.keys()], prefs.presets.map((p) => p.ecu), inflight.map((r) => r.requestId), [...sending], mode, comp, submitted, writable(), editable(), canOperate(), [...connecting], [...pendingOps], discArm, unlockArm, prefs.diagDbs?.length ?? 0, cfgOpen, draftDirty(), routeInfo.hidden, ctx.latch.on, tpBy.get(cur ?? '') ?? 0, Math.floor(now / 1000)]);
    if (key === uiKey) return;
    uiKey = key;
    // ECU select: live targets first, then preset-only and scoped names
    const names: string[] = [...targets.keys()];
    for (const p of prefs.presets) if (!names.includes(p.ecu) && (!targetScope || targetScope.has(p.ecu))) names.push(p.ecu);
    if (targetScope) for (const n of targetScope) if (!names.includes(n)) names.push(n);
    if (cur && !names.includes(cur)) names.push(cur);
    const optKey = names.map((n) => `${n}${targets.has(n) ? '' : '*'}`).join('\u0000');
    if (ecuSel.dataset.k !== optKey) {
      ecuSel.dataset.k = optKey;
      ecuSel.replaceChildren(...names.map((n) => Object.assign(el('option', undefined, targets.has(n) ? n : `${n} (프리셋)`), { value: n })));
    }
    if (cur && ecuSel.value !== cur) ecuSel.value = cur;
    ecuName.textContent = pr ? `${pr.label}${pr.sim ? ' · SIM bench' : ''}` : tv ? '프리셋 없음 (외부에서 연결된 대상)' : '';

    // route summary: daemon state first, preset (not connected) second
    const live = tv && tv.link !== '?';
    routeSum.textContent = live
      ? `${tv!.backend}${tv!.addr ? ` · ${tv!.addr}` : ''}`
      : pr
        ? pr.sim
          ? `SIM bench · ${pr.testerLa}→${pr.la}`
          : `${pr.host || '(호스트 없음)'}:${pr.port} · ${pr.testerLa}→${pr.la}`
        : '—';
    routeSum.title = live ? 'daemon이 보고한 경로 (uds.state backend·addr)' : '프리셋 값 — 연결 전 (daemon 상태 아님)';
    cfgProv.textContent = pr ? `${pr.sim ? 'SIM' : '프리셋'} · ${pr.label}` : '프리셋 없음';
    cfgProv.className = `cfg-prov${pr && !['sim-bench', 'doip-1'].includes(pr.id) ? ' custom' : ''}`;
    cfgProv.title = pr ? `로컬 프로필 ${pr.id} — 연결 상태가 아님` : '이 대상의 로컬 프로필 없음';
    // link state: only uds.state
    const link = tv?.link ?? '?';
    if (cur && connecting.has(cur)) {
      linkSt.className = 'link-st wait';
      linkSt.textContent = '연결 중…';
    } else if (link === 'up') {
      linkSt.className = 'link-st on';
      linkSt.textContent = '연결됨';
    } else if (link === 'connecting') {
      linkSt.className = 'link-st wait';
      linkSt.textContent = '연결 중';
    } else if (link === '?') {
      linkSt.className = 'link-st off';
      linkSt.textContent = tv ? '링크 미보고' : '연결 안 됨';
    } else if (link === 'closed') {
      linkSt.className = 'link-st closed';
      linkSt.textContent = `해제됨${tv?.linkCause ? ` (${tv.linkCause})` : ''}`;
    } else {
      linkSt.className = 'link-st lost';
      linkSt.textContent = `끊김${tv?.linkCause ? ` (${tv.linkCause})` : ''}`;
    }
    // [연결]: uds.connect with the preset (a live link is closed with [연결 해제] first)
    const issues = pr ? presetIssues(pr) : [];
    connectBtn.disabled = !canOperate() || !cur || !pr || issues.length > 0 || link === 'up' || connecting.has(cur);
    connectLabel.textContent = link === 'up' ? '연결됨' : '연결';
    mkProfileBtn.hidden = !(cur !== null && presetOf(cur) === null && targets.has(cur));
    connectBtn.title = link === 'up'
      ? '연결됨 — 새 설정을 적용하려면 [연결 해제] 후 [연결]'
      : !pr
        ? '이 대상의 프로필이 없음 — [이 대상으로 프로필 만들기] 후 [연결]'
        : issues.length
          ? `프로필 오류: ${issues.join(' · ')}`
          : !canOperate()
            ? '쓰기 잠금 또는 권한 없음'
            : `uds.connect ${pr.sim ? 'sim:bench' : `${pr.host}:${pr.port}`} — 요청은 보내지 않음`;
    // U6 disconnect / forget / reconnect note
    discBtn.hidden = !(tv && ['up', 'down', 'connecting'].includes(link));
    discBtn.disabled = !canOperate() || pendingOps.has(`disconnect:${cur}`);
    const discArmed = discArm?.target === cur && performance.now() < discArm.until;
    discBtn.classList.toggle('armed', discArmed);
    discLabel.textContent = discArmed ? '중단 후 해제' : '연결 해제';
    discBtn.title = discArmed
      ? '다시 누르면 진행 중 요청을 중단하고 연결 해제 (uds.disconnect abort)'
      : 'uds.disconnect — 의도적으로 링크를 닫음 (자동 재연결 없음, UDS 바이트 전송 없음)';
    forgetBtn.hidden = !(tv && (link === 'closed' || link === 'down'));
    forgetBtn.disabled = !canOperate() || pendingOps.has(`forget:${cur}`);
    reconnNote.hidden = !(pr && tv && profileNeedsReconnect(pr, { backend: tv.backend, addr: tv.addr, link: tv.link }));
    reconnNote.title = '프로필이 현재 연결과 다름 — [연결 해제] 후 [연결]로 새 설정 적용';
    // U1 TesterPresent job
    tpBtn.textContent = tv?.tpOn ? 'TP 중지' : 'TP 시작';
    tpBtn.disabled = !tv || pendingOps.has(`tp:${cur}`) || (!tv.tpOn && !(canOperate() && link === 'up'));
    if (document.activeElement !== tpIv) tpIv.value = String(tv?.tpOn && tv.tpIntervalMs !== null ? tv.tpIntervalMs : tpIntervalOf(pr));
    tpIv.disabled = !!tv?.tpOn || !canOperate();
    tpInfo.textContent = tv
      ? tpCellText(tv, tpBy.get(tv.target) ?? 0) +
        (tv.tpOn && tv.tpOwner ? ` · ${tv.tpOwner}` : '') +
        (!tv.tpOn && !(pr && pr.tpIntervalMs) ? ` · 기본 주기 ${DEFAULT_TP_INTERVAL_MS} ms (프로필 미설정)` : '')
      : '';
    // U2 security unlock
    if (!secLevel.value && document.activeElement !== secLevel && pr?.unlockLevel !== undefined) secLevel.value = pr.unlockLevel.toString(16).toUpperCase().padStart(2, '0');
    if (!secProv.value && document.activeElement !== secProv && pr?.keyProvider) secProv.value = pr.keyProvider;
    secLevel.disabled = secProv.disabled = !canOperate();
    unlockBtn.disabled = !canOperate() || !tv || link !== 'up' || pendingOps.has(`unlock:${cur}`);
    const unlockArmed = unlockArm !== null && cur !== null && unlockArm.key.startsWith(`${cur}|`) && performance.now() < unlockArm.until;
    unlockBtn.classList.toggle('armed', unlockArmed);
    unlockLabel.textContent = unlockArmed ? 'R3 실행 확인' : '자동 해제';
    // U5 diagnostic DB
    if (tv?.dbHash) dbInfo.replaceChildren('적용됨 ', el('b', undefined, tv.dbHash), ` · ${tv.dbEcu ?? '-'} · DID ${tv.dbDids ?? '?'}개`);
    else dbInfo.textContent = '적용된 DB 없음 — ISO DID 표만 사용';
    const dbOptKey = (prefs.diagDbs ?? []).map((d) => `${d.id}\u0000${d.name}`).join('\u0001');
    if (dbSaved.dataset.k !== dbOptKey) {
      dbSaved.dataset.k = dbOptKey;
      dbSaved.replaceChildren(
        Object.assign(el('option', undefined, '저장된 DB…'), { value: '' }),
        ...(prefs.diagDbs ?? []).map((d) => Object.assign(el('option', undefined, `${d.name}`), { value: d.id })),
      );
    }
    dbApply.disabled = !canOperate() || !tv || link === 'closed' || !dbCheck?.db || pendingOps.has(`db_set:${cur}`);
    dbClearBtn.disabled = !canOperate() || !tv?.dbHash || pendingOps.has(`db_clear:${cur}`);
    cfgBtn.setAttribute('aria-expanded', String(cfgOpen));
    cfgPanel.hidden = !cfgOpen;
    cfgDirty.hidden = !draftDirty();

    // route info
    if (!routeInfo.hidden) {
      const kv: [string, string][] = [
        ['대상', cur ?? '—'],
        ['백엔드 (daemon)', tv && tv.backend !== '?' ? tv.backend : '미보고'],
        ['주소 (daemon)', tv?.addr ?? '미보고'],
        ['링크', link],
        ['프로필', pr ? `${pr.label} · ${pr.sim ? 'SIM bench' : `${pr.host || '—'}:${pr.port}`}` : '없음'],
        ['Tester → ECU LA', pr ? `${pr.testerLa} → ${pr.la}` : '—'],
        ['활성화 · 타임아웃', pr ? `${pr.activation} · ${pr.timeoutMs} ms` : '—'],
      ];
      riKv.replaceChildren(...kv.flatMap(([k, v]) => [el('dt', undefined, k), el('dd', undefined, v)]));
    }

    // tools row: connection overview + session chip
    const ov: HTMLElement[] = [];
    for (const t of targets.values()) {
      if (t.link !== 'up') continue;
      const i = el('span', `ov-i${t.target === cur ? ' cur' : ''}${inflightFor(t.target).length ? ' busy' : ''}`);
      i.append(el('span', 'dot'), `${t.target} · ${t.backend}`);
      i.title = `${t.target} 연결됨 (uds.state)`;
      ov.push(i);
    }
    if (!ov.length) ov.push(el('span', 'ov-none', '활성 연결 없음'));
    const act0 = rows.list().filter((r) => !isFinal(r.state)).length;
    if (act0) ov.push(el('span', 'ov-act', `진행 중 ${act0}`));
    connOv.replaceChildren(...ov);
    sessChip.replaceChildren();
    if (tv) {
      const c = targetCells(tv, tpBy.get(tv.target) ?? 0, ctx.env ?? '?');
      sessChip.className = `sess-chip${tv.sessionConfidence === 'confirmed' ? ' ok' : ''}`;
      sessChip.title = `${targetLine(tv, tpBy.get(tv.target) ?? 0, ctx.env ?? '?')} · ${leaseText(tv, ctx.connId)}`;
      const take = button('T', 'take', '제어 가져오기 (uds.lease.takeover)');
      take.disabled = !canOperate();
      take.addEventListener('click', () => void act('takeover', { target: tv.target }));
      sessChip.append('세션 ', el('b', undefined, c.session), el('small', undefined, `보안 ${c.security}`), el('small', undefined, `TP ${c.testerPresent}`), el('small', undefined, `P2 ${c.p2}`), el('small', undefined, leaseText(tv, ctx.connId)), take);
    }
    svcTarget.replaceChildren('대상 ', el('b', undefined, cur ?? '—'), ` · ${tv && tv.backend !== '?' ? tv.backend : '미보고'} · `, link === 'up' ? '연결됨' : Object.assign(el('span', 'off', '연결 안 됨')));
    tpKv.replaceChildren(
      ...(tv
        ? (() => {
            const c = targetCells(tv, tpBy.get(tv.target) ?? 0, ctx.env ?? '?');
            return [['세션', c.session], ['보안', c.security], ['TesterPresent', c.testerPresent], ['P2/P2*', c.p2], ['제어', leaseText(tv, ctx.connId)]].flatMap(([k, v]) => [el('dt', undefined, k), el('dd', 'mono', v)]);
          })()
        : [el('dt', undefined, '상태'), el('dd', undefined, 'uds.state 미보고')]),
    );

    // composer
    req.className = `req m-${mode}`;
    modeSeg.set(mode);
    const p2 = tv && tv.p2 !== null ? ` · P2 ${tv.p2}/${tv.p2Star ?? '?'} ms` : '';
    reqTarget.replaceChildren('요청 대상 ', el('b', undefined, cur ?? '—'), ' · ', el('b', undefined, 'DoIP'), ` ${live ? `${tv!.backend}${tv!.addr ? ` ${tv!.addr}` : ''}` : ''}${p2} · `, link === 'up' ? '연결됨' : Object.assign(el('span', 'off', link === '?' ? (tv ? '링크 미보고 — daemon이 판단' : '연결 안 됨 — 위의 [연결]') : `링크 ${link} — 읽기 전용`)));
    const dv = normalizeDid(didInput.value);
    const showErr = mode === 'did' && !dv.did && (submitted || didInput.value.length > 0);
    didInput.classList.toggle('invalid', showErr);
    didInput.setAttribute('aria-invalid', String(showErr));
    didName.textContent = dv.did ? (didNameOf(dv.did) ?? '카탈로그에 없는 DID — 응답은 daemon 해석 또는 원시 데이터') : '';
    reqPdu.replaceChildren(el('span', 'k', '요청 PDU'), dv.did ? el('code', undefined, spacedHex(`22${dv.did}`)) : el('code', 'none', '—'), el('small', undefined, dv.did ? '3 B' : ''));
    const rv = normalizeRaw(rawInput.value);
    const rawErr = mode === 'raw' && !rv.hex && (submitted || rawInput.value.trim().length > 0);
    rawInput.classList.toggle('invalid', rawErr);
    rawInput.setAttribute('aria-invalid', String(rawErr));
    const sid = rv.hex?.slice(0, 2) ?? null;
    rawSum.replaceChildren(el('span', 'k', 'SID'), sid ? el('code', undefined, `0x${sid}`) : el('code', 'none', '—'), el('small', undefined, sid ? `${findTemplate(sid)?.name ?? '카탈로그에 없는 SID'} · ${rv.hex!.length / 2} B` : '0 B'));
    rawPrev.replaceChildren(...(rv.hex ? [el('code', undefined, spacedHex(rv.hex)), el('small', undefined, `${rv.hex.length / 2} B · 입력 그대로 (SID 포함)`)] : [el('span', 'none', rawErr ? '유효하지 않음 — 부분 해석·전송 없음' : '—')]));
    sendLabel.textContent = mode === 'did' ? '읽기' : '전송';
    const w = writable();
    const ed = editable();
    didInput.disabled = rawInput.disabled = !ed;
    sendBtn.disabled = !w || !cur || sending.has(cur);
    sendBtn.title = needsConnect() ? `${cur} 연결 안 됨 — 먼저 [연결] (uds.connect). 편집은 가능, 전송은 연결 후` : SEND_TITLE;
    abortBtn.disabled = false; // safety direction: always available
    abortLabel.textContent = inflight.length ? `취소 #${rowNo.get(inflight[inflight.length - 1]!.requestId) ?? ''}` : '취소';
    for (const f of paramInputs.values()) f.disabled = !ed;
    let cls = '';
    let text = '';
    const err = mode === 'did' ? dv.err : rv.err;
    if (!canOperate()) {
      cls = 'warn';
      text = ctx.writeLocked ? '쓰기 잠금 (STALLED·연결 끊김) — 취소만 가능' : '권한 없음 (operate 필요)';
    } else if (!linkAllowsWrite()) {
      cls = 'warn';
      text = linkDownMessage(link, presetOf(cur) !== null);
    } else if (needsConnect()) {
      cls = 'warn';
      text = `${cur}: daemon에 연결된 대상 아님 — 위의 [연결]로 연결한 뒤 전송. 요청 편집·DID 선택은 전송하지 않습니다.`;
    } else if ((mode === 'did' ? showErr : rawErr) && err) {
      cls = 'err';
      text = `${err} — 전송하지 않음`;
    } else if (inflight.length) {
      cls = 'warn';
      text = `요청 #${rowNo.get(inflight[inflight.length - 1]!.requestId) ?? '?'} 진행 중 — 결과를 기다리거나 [취소]. 추가 요청은 daemon 대기열 정책을 따릅니다.`;
    } else if (comp.hex) text = mode === 'did' ? 'Enter 또는 [읽기]로 전송. 선택만으로는 전송하지 않습니다.' : 'Ctrl/⌘+Enter 또는 [전송]으로 전송. 입력 바이트 그대로 (SID 포함).';
    msgText.textContent = text;
    reqMsg.className = `req-msg${cls ? ` ${cls}` : ''}`;
    for (const [k, b] of tabBtns) b.setAttribute('aria-selected', String(k === tool));
    applyLayout();
  };

  const draw = (): bigint => {
    const now = performance.now();
    const snapshot = mirror.snapshot;
    if (snapshot && snapshot.version !== snapshotVersion) {
      snapshotVersion = snapshot.version;
      const before = new Map(targets);
      targets.clear();
      targetsOverflow = 0;
      uiKey = '';
      for (const item of snapshot.items) {
        if (item.domain !== 'uds' || item.kind !== 'uds.snapshot') continue;
        const state = item.state as { kind?: unknown; payload?: unknown } | null;
        if (!state || state.kind !== 'uds.state') continue;
        const next = reduceTargetState(undefined, state.payload);
        if (!next || (targetScope && !targetScope.has(next.target))) continue;
        noteTarget(next.target);
        if (!targets.has(next.target)) continue;
        // a newer live state (e.g. link down) already applied wins over this snapshot (as of throughSeq)
        const applied = targetSeq.get(next.target) ?? -1n;
        if (applied > snapshot.throughSeq) targets.set(next.target, before.get(next.target) ?? next);
        else {
          targets.set(next.target, next);
          targetSeq.set(next.target, snapshot.throughSeq);
        }
      }
      // targets known only from newer live control events survive a snapshot that predates them
      for (const [name, t] of before) {
        if (!targets.has(name) && (targetSeq.get(name) ?? -1n) > snapshot.throughSeq && targets.size < MAX_TARGETS) targets.set(name, t);
      }
    }
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (!snapshot || BigInt(ev.seq) > snapshot.throughSeq) applyState(ev);
    }
    if (snapshot) hydrateFromStatus();
    const list = rows.list();
    if (rowsVersion !== rowsVersionSeen) {
      rowsVersionSeen = rowsVersion;
      for (const r of list) {
        const t = rowTarget(r);
        if (t && (!targetScope || targetScope.has(t))) noteTarget(t);
        if (!rowNo.has(r.requestId)) {
          rowNo.set(r.requestId, nextNo++);
        }
        updateLastValues(lastValues, r, now);
      }
      uiKey = '';
    }
    table.setItems(visibleItems());
    table.render();
    const vis = visibleRows();
    histCount.textContent = `${vis.length}${histScope === 'ecu' ? `/${list.length - hidden.size}` : ''} · 진행 ${list.filter((r) => !isFinal(r.state)).length}`;
    const tpBy = tpCount();
    renderTargets(tpBy);
    renderUi(tpBy, now);
    renderCatalog();
    renderDtc();
    renderDidList();
    renderVals();
    renderDetail(now);
    tools.render(false);
    scopeSeg.set(histScope);
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  // uds.state is a priority kind: live link changes arrive on the control path (data subs get them only via
  // snapshot/backlog). Same per-target seq guard as the mirror path.
  const offControl = ctx.onControl((ev) => {
    if (applyState(ev)) ctx.scheduler.invalidate();
  });
  if (mode === 'raw') syncFromHex();
  renderParams();
  renderService();
  applyLayout();

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      if (tool) mod.style.setProperty('--td-top', `${ctxbar.offsetHeight || 70}px`);
      ctx.scheduler.invalidate();
    },
    dispose() {
      disposed = true;
      if (previewTimer) clearTimeout(previewTimer);
      previewAbort?.abort();
      clearInterval(tick);
      tools.dispose();
      offRows();
      offAppend();
      offReveal();
      offFrame();
      offControl();
      table.dispose();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'uds.console',
    propsSchema: { type: 'object', properties: { targets: { type: 'string', pattern: '^[A-Za-z0-9_.-]{1,64}(,[A-Za-z0-9_.-]{1,64}){0,15}$' } } },
    domain: 'uds',
    title: 'UDS 콘솔',
    version: 1,
    milestone: 'M1',
    topics: [{ key: 'uds', filter: () => ({ domains: ['uds'] }), snapshot: 'required', mirrorRows: 2_000 }],
    // dry-run preview only: the context refuses this query unless dry_run:true.
    // uds.status: read-only one-shot hydration of a bound panel (hydrateFromStatus)
    queries: ['uds.request', 'uds.status'],
    actions: [
      { id: 'send', rpc: 'uds.request', class: 'request', right: 'operate', confirm: 'none', label: 'UDS 요청' },
      { id: 'connect', rpc: 'uds.connect', class: 'request', right: 'operate', confirm: 'none', label: 'UDS 연결' },
      { id: 'takeover', rpc: 'uds.lease.takeover', class: 'request', right: 'operate', confirm: 'none', label: '제어 가져오기' },
      { id: 'abort', rpc: 'uds.abort', class: 'request', right: 'operate', confirm: 'none', safetyDirection: 'reduce', label: '중단' },
      // U6/U1/U2/U5 (operator upgrade B4)
      { id: 'disconnect', rpc: 'uds.disconnect', class: 'request', right: 'operate', confirm: 'none', label: '연결 해제' },
      { id: 'forget', rpc: 'uds.forget', class: 'request', right: 'operate', confirm: 'none', label: '대상 제거' },
      { id: 'tp_on', rpc: 'uds.tester_present', class: 'request', right: 'operate', confirm: 'none', label: 'TesterPresent 시작' },
      { id: 'tp_off', rpc: 'uds.tester_present', class: 'request', right: 'operate', confirm: 'none', safetyDirection: 'reduce', label: 'TesterPresent 중지' },
      { id: 'unlock', rpc: 'uds.security_unlock', class: 'request', right: 'operate', confirm: 'none', label: '보안 해제 (R3)' },
      { id: 'db_set', rpc: 'uds.db.set', class: 'request', right: 'operate', confirm: 'none', label: '진단 DB 적용' },
      { id: 'db_clear', rpc: 'uds.db.clear', class: 'request', right: 'operate', confirm: 'none', label: '진단 DB 해제' },
    ],
    memory: { maxBytes: 32 * 1024 * 1024 },
    render: { maxHz: 20, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createUdsConsole,
);
