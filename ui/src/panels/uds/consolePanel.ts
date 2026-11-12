// uds.console (M1) — uds-ui U1–U5, plan §5.1.4; benchmark-fidelity layout after the
// Softing DTS.monaco "Diagnostic Services" control unit (docs/impl/main/research/fidelity/uds.md).
//
//  논리 링크 (Logical Link list)  : one grid row per target — link · transport(backend) · addr ·
//                                   session · security · TesterPresent · P2/P2* · env · owner
//                                   + [제어 가져오기 T]. TesterPresent is the 0x3E keep-alive,
//                                   never an ISO-TP/CAN-TP transport state.
//  진단 서비스 (service catalog)  : static ISO 14229-1 request templates (MAIN D1). Picking or
//                                   editing one only rewrites the hex line — it never sends.
//  설정 (Settings)                : logical link, service, parameter grid, canonical PDU hex line
//                                   → 150 ms idle → daemon dry-run preview.
//  현재 선택 (Current selection)  : 보내기 (Transmit) through the existing uds.request action
//                                   (R2+ previews need a second Enter — UI mistake prevention
//                                   only, the daemon policy decides, F1) · 중단 · 폴링 정지.
//  트랜잭션 (trace)               : RowTable, one row per request_id, progressive stages; the
//                                   expanded detail pairs follow the reference block order
//                                   (MAIN D4). TesterPresent rows fold into the link grid.
//  DTC / DID 값                   : raw payload.dtcs (MAIN D2) and daemon-decoded DID values.
// Single-letter keys (T, !) only when the panel has focus and no input field does.

import './uds.css';
import {
  createLatchLine,
  createRowTable,
  mergeGaps,
  registerPanel,
  type PanelContext,
  type PanelInstance,
} from '../../sdk';
import {
  buildPdu,
  defaultValues,
  detailPairs,
  emptyTarget,
  filterCatalog,
  findTemplate,
  hexByte,
  isTesterPresent,
  latestDtcs,
  leaseText,
  needsSecondEnter,
  normalizeHex,
  paramIssues,
  parsePdu,
  previewText,
  reduceTarget,
  reduceTargetState,
  resultSymbol,
  SERVICE_GROUPS,
  targetCells,
  targetLine,
  templateLabel,
  timingText,
  updateLastValues,
  type LastValue,
  type ParamValues,
  type ServiceTemplate,
  type TargetView,
} from './udsModel';

export const PREVIEW_DEBOUNCE_MS = 150;
const SECOND_ENTER_MS = 3_000;
const MAX_TARGETS = 32; // C2: target bar entries (over → ignored + counted)
const MAX_DID_ROWS = 64;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function button(text: string, cls?: string, title?: string): HTMLButtonElement {
  const b = el('button', cls, text);
  b.type = 'button';
  if (title) b.title = title;
  return b;
}

function colHead(text: string): HTMLSpanElement {
  const c = el('span', undefined, text);
  c.setAttribute('role', 'columnheader');
  return c;
}

/** group box with a caption (the reference's framed control units) */
function box(cls: string, caption: string, testid?: string): { root: HTMLElement; head: HTMLElement } {
  const root = el('section', `uds-ds-box ${cls}`);
  const head = el('div', 'uds-ds-caption');
  head.append(el('span', 'uds-ds-title', caption));
  root.append(head);
  root.setAttribute('aria-label', caption);
  if (testid) root.setAttribute('data-testid', testid);
  return { root, head };
}

const str = (v: unknown): string => (typeof v === 'string' ? v : v === undefined || v === null ? '' : String(v));
/** service name, else the SID in hex (canonical `sid` is a number: 34 → "SID 0x22") */
const serviceText = (p: Readonly<Record<string, unknown>>): string => {
  const svc = str(p.service);
  if (svc) return svc;
  const sid = p.sid === undefined || p.sid === null ? null : hexByte(p.sid);
  return sid ? `SID 0x${sid}` : '';
};

export function createUdsConsole(ctx: PanelContext): PanelInstance {
  const mirror = ctx.store('uds');
  // dry-run previews are not transactions: never shown as rows (uds-ui U3 — the tools/mock
  // currently publishes them; reported)
  const rows = ctx.rows({ domain: 'uds', where: (r) => !isTesterPresent(r) && r.payload.dry_run !== true });
  const tpRows = ctx.rows({ domain: 'uds', where: isTesterPresent });
  const targets = new Map<string, TargetView>();
  let targetsOverflow = 0;
  const lastValues = new Map<string, LastValue>();
  let lastSeq = -1n;
  let snapshotVersion = 0;
  let rowsVersionSeen = -1;
  let rowsVersion = 0;
  const targetScope = typeof ctx.props.targets === 'string' ? new Set(ctx.props.targets.split(',')) : null;
  const initialTarget = typeof ctx.props.targets === 'string' ? ctx.props.targets.split(',')[0] : undefined;
  let selectedTarget: string | null = initialTarget ?? null;

  const writable = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- DOM: logical links ----
  const root = el('div', 'uds-panel uds-ds');
  root.tabIndex = 0;
  const links = box('uds-ds-links uds-targets', '논리 링크');
  const bar = links.root;
  bar.setAttribute('data-testid', 'uds-targets');
  links.head.append(el('span', 'muted', 'manager가 연결한 대상만 · 연결은 상단 인터페이스 연결'));
  const linkGrid = el('div', 'uds-ds-grid uds-ds-linkgrid');
  linkGrid.setAttribute('role', 'table');
  linkGrid.setAttribute('aria-label', '논리 링크');
  bar.append(linkGrid);

  // ---- DOM: service catalog ----
  const catalog = box('uds-ds-catalog', '진단 서비스', 'uds-catalog');
  const search = el('input', 'uds-ds-search');
  search.type = 'search';
  search.placeholder = 'SID 또는 이름 (예: 22, Read)';
  search.setAttribute('aria-label', '서비스 검색');
  search.spellcheck = false;
  const tree = el('div', 'uds-ds-tree');
  tree.setAttribute('role', 'list');
  catalog.root.append(search, tree, el('div', 'uds-ds-note muted', 'ISO 14229-1 요청 형식 · 선택은 PDU만 채움 (전송 없음)'));

  // ---- DOM: settings (logical link / service / params / PDU) ----
  const settings = box('uds-ds-settings', '설정', 'uds-settings');
  const kv = el('div', 'uds-ds-kv');
  const targetSel = el('select');
  targetSel.setAttribute('aria-label', '대상');
  const linkInfo = el('span', 'mono muted');
  const linkCell = el('span', 'uds-ds-kv-v');
  linkCell.append(targetSel, ' ', linkInfo);
  const serviceCell = el('span', 'uds-ds-kv-v mono');
  serviceCell.setAttribute('data-testid', 'uds-service');
  kv.append(el('span', 'uds-ds-kv-k', '논리 링크'), linkCell, el('span', 'uds-ds-kv-k', '진단 서비스'), serviceCell);
  const params = el('div', 'uds-ds-grid uds-ds-params');
  params.setAttribute('role', 'table');
  params.setAttribute('aria-label', '파라미터');
  params.setAttribute('data-testid', 'uds-params');
  const pduRow = el('div', 'uds-ds-pdu');
  const input = el('input', 'mono');
  input.placeholder = '22 F190';
  input.setAttribute('aria-label', 'UDS 요청 (hex)');
  input.spellcheck = false;
  const clearBtn = button('✖', 'uds-ds-icon', 'PDU 지우기 (전송 없음)');
  clearBtn.setAttribute('aria-label', 'PDU 지우기');
  pduRow.append(el('span', 'uds-ds-kv-k', 'PDU'), input, clearBtn);
  const preview = el('div', 'uds-preview uds-ds-preview muted');
  preview.setAttribute('data-testid', 'uds-preview');
  preview.setAttribute('aria-live', 'polite');
  const issuesEl = el('div', 'uds-ds-issues');
  issuesEl.setAttribute('data-testid', 'uds-param-issues');
  settings.root.append(kv, params, pduRow, preview, issuesEl);

  // ---- DOM: current selection ----
  const current = el('div', 'uds-ds-current');
  current.setAttribute('data-testid', 'uds-current');
  const sendBtn = button('보내기 ⏎', 'uds-ds-transmit', 'Transmit — 현재 PDU를 uds.request로 보냄 (daemon dry-run·정책·ActionGate 경유)');
  const abortBtn = button('중단 !', 'danger', '선택 대상의 진행 중 요청 중단 (이미 보낸 요청은 ⌛? 불확정이 될 수 있음)');
  const pollBtn = button('폴링 정지', undefined, '선택 대상의 주기 요청(폴링) 정지 — 순환 시작은 UI에 없음');
  current.append(el('span', 'uds-ds-title', '현재 선택'), sendBtn, abortBtn, pollBtn, el('span', 'muted', '서비스 선택·파라미터 편집은 보내지 않음'));

  // ---- DOM: trace + side tabs ----
  const trace = box('uds-ds-trace', '트랜잭션');
  const cursorEl = el('span', 'uds-cursor muted');
  trace.head.append(cursorEl);
  const tableHost = el('div', 'uds-table');
  trace.root.append(tableHost);

  const side = el('section', 'uds-ds-box uds-ds-side');
  const tabs = el('div', 'uds-ds-tabs');
  tabs.setAttribute('role', 'tablist');
  const dtcPane = el('div', 'uds-ds-pane');
  dtcPane.setAttribute('data-testid', 'uds-dtc');
  dtcPane.setAttribute('role', 'tabpanel');
  const lastEl = el('div', 'uds-ds-pane');
  lastEl.setAttribute('data-testid', 'uds-last-values');
  lastEl.setAttribute('role', 'tabpanel');
  const panes = { dtc: dtcPane, did: lastEl } as const;
  type Tab = keyof typeof panes;
  let tab: Tab = 'dtc';
  const tabBtns = (Object.keys(panes) as Tab[]).map((k) => {
    const b = button(k === 'dtc' ? 'DTC' : 'DID 값', 'uds-ds-tab');
    b.setAttribute('role', 'tab');
    b.dataset.tab = k;
    b.addEventListener('click', () => {
      tab = k;
      syncTabs();
    });
    return b;
  });
  tabs.append(...tabBtns);
  const dtcActions = el('div', 'uds-ds-actions');
  const dtcReadBtn = button('읽기 템플릿 19 02 FF', undefined, 'ReadDTCInformation reportDTCByStatusMask를 PDU에 채움 (전송 없음)');
  const dtcClearBtn = button('지우기 템플릿 14 FF FF FF', undefined, 'ClearDiagnosticInformation 전체 그룹을 PDU에 채움 (전송 없음, dry-run 등급 확인 필요)');
  dtcActions.append(dtcReadBtn, dtcClearBtn);
  const dtcBody = el('div');
  dtcPane.append(dtcActions, dtcBody);
  side.append(tabs, dtcPane, lastEl);
  const syncTabs = () => {
    for (const b of tabBtns) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
    for (const k of Object.keys(panes) as Tab[]) panes[k].hidden = k !== tab;
  };
  syncTabs();

  const top = el('div', 'uds-ds-top');
  top.append(catalog.root, settings.root);
  const bottom = el('div', 'uds-ds-bottom');
  bottom.append(trace.root, side);
  root.append(createLatchLine(ctx), bar, top, current, bottom);

  const table = createRowTable(tableHost, {
    columns: [
      { key: 'target', header: '대상', width: '5em', cell: (r) => str(r.payload.target) || (r.channel ?? '') },
      { key: 'service', header: '서비스', width: '13em', cell: (r) => serviceText(r.payload) },
      { key: 'summary', header: '요약', width: '1fr', cell: (r) => str(r.payload.summary) },
      { key: 'result', header: '결과', width: '10em', cell: (r) => resultSymbol(r) },
      { key: 'timing', header: 'p2/rtt', width: '7em', cell: (r, now) => timingText(r, now) },
    ],
    detail: (r) => detailPairs(r, targets.get(str(r.payload.target) || (r.channel ?? ''))),
    onNeedRender: () => ctx.scheduler.invalidate(),
    ariaLabel: 'UDS 트랜잭션',
    emptyText: '요청 없음',
  });

  // cross-panel reveal (e.g. eth `↔UDS` chip): select + scroll to the request_id row
  const offReveal = ctx.ui.onReveal((ref) => {
    if (ref.domain !== 'uds' || ref.requestId === null) return false;
    table.setItems(mergeGaps(rows.list(), mirror.gaps));
    table.render();
    return table.reveal(ref.requestId);
  });

  // ---- composer: template ⇄ canonical hex line ----
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let previewAbort: AbortController | null = null;
  let previewRisk: unknown = null;
  let armedAt: number | null = null;
  let tpl: ServiceTemplate | null = null;
  let values: ParamValues = {};
  let catalogQuery = '';
  let catalogKey = '';
  const paramInputs = new Map<string, HTMLInputElement | HTMLSelectElement>();

  const target = () => selectedTarget ?? [...targets.keys()][0] ?? null;

  const runPreview = () => {
    previewTimer = null;
    const hex = normalizeHex(input.value);
    const t = target();
    if (!hex || !t) {
      previewRisk = null;
      preview.textContent = input.value.trim() ? 'hex만 지원 (예: 22 F190)' : '';
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
        preview.textContent = pv.text;
      })
      .catch(() => {
        if (!ac.signal.aborted) preview.textContent = '미리보기 실패';
      });
  };

  /** any PDU change: disarm the second Enter and re-run the (debounced) dry-run only */
  const pduChanged = () => {
    armedAt = null;
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(runPreview, PREVIEW_DEBOUNCE_MS);
  };

  const renderIssues = () => {
    const list = tpl ? paramIssues(tpl, values) : [];
    issuesEl.textContent = list.length ? `⚠ ${list.join(' · ')} — 그대로 보내면 ECU가 거절할 수 있음` : '';
  };

  const renderService = () => {
    const sid = normalizeHex(input.value)?.slice(0, 2) ?? null;
    serviceCell.textContent = tpl ? templateLabel(tpl) : sid ? `SID 0x${sid} — 카탈로그 밖 (원시 hex 그대로)` : '—';
  };

  const renderParams = () => {
    paramInputs.clear();
    const head = el('div', 'uds-ds-row uds-ds-head');
    head.setAttribute('role', 'row');
    for (const h of ['파라미터', '값 (hex)', '의미']) {
      const c = el('span', undefined, h);
      c.setAttribute('role', 'columnheader');
      head.append(c);
    }
    const out: HTMLElement[] = [head];
    if (!tpl) {
      out.push(el('div', 'uds-ds-empty muted', '서비스를 고르면 ISO 14229-1 파라미터가 나타납니다 · PDU에 hex를 직접 써도 됩니다'));
      params.replaceChildren(...out);
      return;
    }
    const sidRow = el('div', 'uds-ds-row uds-ds-fixed');
    sidRow.setAttribute('role', 'row');
    sidRow.append(el('span', undefined, 'SID'), el('span', 'mono', `0x${tpl.sid}`), el('span', 'muted', '고정'));
    out.push(sidRow);
    const w = writable();
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
        const sel = el('select', 'mono');
        for (const c of p.choices) sel.append(Object.assign(el('option', undefined, `${c.value} ${c.label}`), { value: c.value }));
        sel.value = values[p.key] ?? p.def;
        sel.addEventListener('change', () => onParam(p.key, sel.value, setMeaning));
        field = sel;
      } else {
        const inp = el('input', 'mono');
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
    input.value = buildPdu(tpl, values);
    after();
    renderIssues();
    renderService();
    pduChanged();
  };

  const applyTemplate = (t: ServiceTemplate, preset?: ParamValues) => {
    if (!writable()) return;
    tpl = t;
    values = { ...defaultValues(t), ...preset };
    input.value = buildPdu(t, values);
    renderParams();
    renderIssues();
    renderService();
    catalogKey = '';
    pduChanged();
  };

  /** typing the hex line selects the matching template (if any) and reflects its fields */
  const syncFromHex = () => {
    const sid = input.value.replace(/\s+/g, '').slice(0, 2).toUpperCase();
    const t = sid.length === 2 ? findTemplate(sid) : null;
    if (t !== tpl) {
      tpl = t;
      values = t ? parsePdu(t, input.value) : {};
      renderParams();
      catalogKey = '';
    } else if (t) {
      values = parsePdu(t, input.value);
      for (const [k, f] of paramInputs) if (f.value !== values[k]) f.value = values[k] ?? '';
    }
    renderIssues();
    renderService();
  };

  input.addEventListener('input', () => {
    syncFromHex();
    pduChanged();
  });

  const submit = () => {
    const hex = normalizeHex(input.value);
    const t = target();
    if (!hex || !t) {
      preview.textContent = !t ? '대상 없음' : 'hex만 지원 (예: 22 F190)';
      return;
    }
    if (needsSecondEnter(previewRisk) && (armedAt === null || performance.now() - armedAt > SECOND_ENTER_MS)) {
      armedAt = performance.now();
      preview.textContent = `${preview.textContent ?? ''} — ${str(previewRisk)}: Enter 한 번 더`;
      return;
    }
    armedAt = null;
    void act('send', { target: t, bytes_hex: hex });
    input.value = '';
    preview.textContent = '';
    previewRisk = null;
    syncFromHex();
  };

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.repeat) {
      e.preventDefault();
      submit();
    }
  });
  sendBtn.addEventListener('click', submit);
  clearBtn.addEventListener('click', () => {
    input.value = '';
    if (previewTimer) clearTimeout(previewTimer);
    previewAbort?.abort();
    previewRisk = null;
    armedAt = null;
    preview.textContent = '';
    syncFromHex();
  });
  abortBtn.addEventListener('click', () => {
    const t = target();
    if (t) void act('abort', { target: t });
  });
  pollBtn.addEventListener('click', () => {
    const t = target();
    if (t) void act('pollStop', { target: t });
  });
  targetSel.addEventListener('change', () => {
    selectedTarget = targetSel.value || null;
    pduChanged(); // the preview belongs to a target
    ctx.scheduler.invalidate();
  });
  search.addEventListener('input', () => {
    catalogQuery = search.value;
    ctx.scheduler.invalidate();
  });
  const dtcTpl = findTemplate('19');
  const clrTpl = findTemplate('14');
  dtcReadBtn.addEventListener('click', () => dtcTpl && applyTemplate(dtcTpl, { type: '02', mask: 'FF' }));
  dtcClearBtn.addEventListener('click', () => clrTpl && applyTemplate(clrTpl, { group: 'FFFFFF' }));
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

  // ---- data ----
  const offRows = rows.onChange(() => {
    rowsVersion++;
    ctx.scheduler.invalidate();
  });
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const offCursor = ctx.agentCursor.on(() => ctx.scheduler.invalidate());
  const tick = setInterval(() => {
    if (rows.list().some((r) => r.state === 'sent' || r.state === 'accepted' || r.state === 'requested')) ctx.scheduler.invalidate();
  }, 250); // elapsed counters of in-flight rows keep moving

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
  const renderTargets = () => {
    const env = ctx.env ?? '?';
    const tpBy = new Map<string, number>();
    for (const r of tpRows.list()) {
      const t = str(r.payload.target) || r.channel || '?';
      tpBy.set(t, (tpBy.get(t) ?? 0) + 1);
    }
    const w = writable();
    const cur = target();
    const views = [...targets.values()].map((t) => ({ t, c: targetCells(t, tpBy.get(t.target) ?? 0, env), line: targetLine(t, tpBy.get(t.target) ?? 0, env), lease: leaseText(t, ctx.connId) }));
    const key = JSON.stringify([w, cur, targetsOverflow, views.map((v) => [v.line, v.lease, v.c.link.text])]);
    if (key !== linksKey) {
      linksKey = key;
      const head = el('div', 'uds-ds-row uds-ds-head');
      head.setAttribute('role', 'row');
      for (const h of LINK_HEAD) {
        const c = el('span', undefined, h);
        c.setAttribute('role', 'columnheader');
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
        const cell = (text: string, col: string, cls = '') => {
          const c = el('span', `mono ${cls}`.trim(), text);
          c.setAttribute('role', 'cell');
          c.dataset.col = col;
          return c;
        };
        const link = cell(v.c.link.text, 'link', v.c.link.cls);
        if (v.c.link.title) link.title = v.c.link.title;
        const tp = cell(v.c.testerPresent, 'tester-present');
        tp.title = 'TesterPresent (0x3E) — ISO-TP/CAN-TP 아님';
        const take = button('제어 가져오기 T');
        take.disabled = !w;
        take.addEventListener('click', (e) => {
          e.stopPropagation();
          void act('takeover', { target: v.t.target });
        });
        const takeCell = el('span');
        takeCell.setAttribute('role', 'cell');
        takeCell.append(take);
        row.append(
          link,
          cell(v.t.target, 'target', 'uds-ds-strong'),
          cell(v.c.backend, 'backend'),
          cell(v.c.addr, 'addr'),
          cell(v.c.session, 'session'),
          cell(v.c.security, 'security'),
          tp,
          cell(v.c.p2, 'p2'),
          cell(v.c.env, 'env'),
          cell(v.lease, 'lease', 'lease'),
          takeCell,
        );
        row.addEventListener('click', () => {
          if (selectedTarget === v.t.target) return;
          selectedTarget = v.t.target;
          pduChanged();
          ctx.scheduler.invalidate();
        });
        children.push(row);
      }
      if (views.length === 0) children.push(el('div', 'uds-ds-empty muted', '대상 없음 — 인터페이스 연결(UDS DoIP) 또는 에이전트의 loom uds connect로 연결하면 나타납니다'));
      if (targetsOverflow) children.push(el('div', 'uds-ds-empty muted', `+${targetsOverflow} 대상 표시 안 됨`));
      linkGrid.replaceChildren(...children);
    }

    const opts = [...targets.keys()];
    if (opts.join('\u0000') !== [...targetSel.options].map((o) => o.value).join('\u0000')) {
      targetSel.replaceChildren(...opts.map((o) => Object.assign(el('option', undefined, o), { value: o })));
    }
    if (cur && targetSel.value !== cur) targetSel.value = cur;
    const tv = cur ? targets.get(cur) : undefined;
    linkInfo.textContent = tv ? `${tv.backend}${tv.addr ? ` ${tv.addr}` : ''}` : '';
  };

  const renderCatalog = () => {
    const w = writable();
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
        const b = button(templateLabel(t), 'uds-ds-svc mono');
        b.setAttribute('role', 'listitem');
        b.dataset.sid = t.sid;
        b.setAttribute('aria-pressed', String(tpl === t));
        b.disabled = !w;
        b.addEventListener('click', () => applyTemplate(t));
        out.push(b);
      }
    }
    if (!out.length) out.push(el('div', 'uds-ds-empty muted', '일치하는 표준 서비스 없음 — PDU에 hex를 직접 입력'));
    tree.replaceChildren(...out);
  };

  let sideKey = '';
  const renderSide = () => {
    const cur = target();
    const sec = Math.floor(performance.now() / 1000);
    const key = `${rowsVersion}|${cur}|${tab === 'did' ? sec : ''}|${writable()}`;
    if (key === sideKey) return;
    sideKey = key;
    dtcReadBtn.disabled = dtcClearBtn.disabled = !writable();

    const dv = latestDtcs(rows.list(), cur);
    const grid = el('div', 'uds-ds-grid uds-ds-dtcgrid');
    grid.setAttribute('role', 'table');
    grid.setAttribute('aria-label', 'DTC');
    const head = el('div', 'uds-ds-row uds-ds-head');
    head.setAttribute('role', 'row');
    for (const h of ['DTC (hex)', '상태 바이트']) head.append(colHead(h));
    grid.append(head);
    if (!dv) grid.append(el('div', 'uds-ds-empty muted', `${cur ?? '대상'}: 확정된 0x19 응답 없음`));
    else if (dv.entries.length === 0) grid.append(el('div', 'uds-ds-empty muted', '없음 (응답에 DTC 0개)'));
    else
      for (const d of dv.entries) {
        const row = el('div', 'uds-ds-row');
        row.setAttribute('role', 'row');
        row.dataset.dtc = d.dtc;
        row.append(el('span', 'mono', d.dtc), el('span', 'mono', d.status));
        grid.append(row);
      }
    const src = el('div', 'uds-ds-note muted', dv ? `출처 ${dv.requestId} · payload.dtcs 원시값 (재매핑·상태 비트 해석 없음)` : 'payload.dtcs 원시값만 표시');
    dtcBody.replaceChildren(grid, src);

    const didGrid = el('div', 'uds-ds-grid uds-ds-didgrid');
    didGrid.setAttribute('role', 'table');
    didGrid.setAttribute('aria-label', 'DID 값');
    const dh = el('div', 'uds-ds-row uds-ds-head');
    dh.setAttribute('role', 'row');
    for (const h of ['대상', 'DID', '이름', '값', '단위', '경과']) dh.append(colHead(h));
    didGrid.append(dh);
    const now = performance.now();
    const vals = [...lastValues.values()].reverse().slice(0, MAX_DID_ROWS);
    if (!vals.length) didGrid.append(el('div', 'uds-ds-empty muted', '마지막 값: — (daemon이 해석한 확정 응답만)'));
    for (const v of vals) {
      const row = el('div', 'uds-ds-row');
      row.setAttribute('role', 'row');
      row.dataset.did = `${v.target}:${v.did}`;
      row.append(
        el('span', 'mono', v.target),
        el('span', 'mono', v.did),
        el('span', undefined, v.name),
        el('span', 'mono', v.value),
        el('span', undefined, v.unit),
        el('span', 'muted', `${Math.round((now - v.atMs) / 1000)}s`),
      );
      didGrid.append(row);
    }
    lastEl.replaceChildren(didGrid);
  };

  const draw = (): bigint => {
    const snapshot = mirror.snapshot;
    if (snapshot && snapshot.version !== snapshotVersion) {
      snapshotVersion = snapshot.version;
      targets.clear();
      targetsOverflow = 0;
      for (const item of snapshot.items) {
        if (item.domain !== 'uds' || item.kind !== 'uds.snapshot') continue;
        const state = item.state as { kind?: unknown; payload?: unknown } | null;
        if (!state || state.kind !== 'uds.state') continue;
        const next = reduceTargetState(undefined, state.payload);
        if (!next || (targetScope && !targetScope.has(next.target))) continue;
        noteTarget(next.target);
        if (targets.has(next.target)) targets.set(next.target, next);
      }
    }
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      const name = ev.kind === 'uds.state' ? (str((ev.payload as Record<string, unknown>).target) || ev.channel) : null;
      if (name && (!targetScope || targetScope.has(name)) && (!snapshot || BigInt(ev.seq) > snapshot.throughSeq)) {
        noteTarget(name);
        const next = reduceTarget(targets.get(name), ev);
        if (next && targets.has(name)) targets.set(name, next);
      }
    }
    const list = rows.list();
    if (rowsVersion !== rowsVersionSeen) {
      rowsVersionSeen = rowsVersion;
      const now = performance.now();
      for (const r of list) {
        const t = str(r.payload.target) || r.channel;
        if (t && (!targetScope || targetScope.has(t))) noteTarget(t);
        updateLastValues(lastValues, r, now);
      }
    }
    table.setItems(mergeGaps(list, mirror.gaps));
    table.render();
    renderTargets();
    renderCatalog();
    renderSide();
    const c = ctx.agentCursor.seq();
    cursorEl.textContent = c === null ? '' : `에이전트 확인 위치 seq:${c}`;
    const w = writable();
    input.disabled = !w;
    sendBtn.disabled = !w;
    clearBtn.disabled = !w;
    pollBtn.disabled = !w;
    search.disabled = false; // browsing the catalog is read-only
    for (const f of paramInputs.values()) f.disabled = !w;
    abortBtn.disabled = false; // safety direction: always available
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  renderParams();
  renderService();

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      if (previewTimer) clearTimeout(previewTimer);
      previewAbort?.abort();
      clearInterval(tick);
      offRows();
      offAppend();
      offCursor();
      offReveal();
      offFrame();
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
    // dry-run preview only: the context refuses this query unless dry_run:true
    queries: ['uds.request'],
    actions: [
      { id: 'send', rpc: 'uds.request', class: 'request', right: 'operate', confirm: 'none', label: 'UDS 요청' },
      { id: 'takeover', rpc: 'uds.lease.takeover', class: 'request', right: 'operate', confirm: 'none', label: '제어 가져오기' },
      { id: 'abort', rpc: 'uds.abort', class: 'request', right: 'operate', confirm: 'none', safetyDirection: 'reduce', label: '중단' },
      { id: 'pollStop', rpc: 'uds.poll.stop', class: 'job', right: 'operate', confirm: 'none', label: '폴링 정지' },
    ],
    memory: { maxBytes: 32 * 1024 * 1024 },
    render: { maxHz: 20, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createUdsConsole,
);
