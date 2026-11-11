// uds.console (M1) — uds-ui U1–U5, plan §5.1.4.
//  target bar   : per target link/session/security/TP/P2 + control owner + [제어 가져오기 T]
//  transactions : RowTable (one row per request_id, progressive stages, result symbol,
//                 pending ×N, elapsed/limit). TesterPresent folded into the bar counter.
//  last values  : decoded DID values of confirmed responses
//  command line : hex → 150 ms idle → dry-run preview; Enter sends (R2+ previews need a
//                 second Enter — UI mistake prevention only, the daemon policy decides, F1)
// Single-letter keys (T, !) only when the panel has focus and no input field does.

import {
  createLatchLine,
  createRowTable,
  mergeGaps,
  registerPanel,
  type PanelContext,
  type PanelInstance,
  type RowLike,
} from '../../sdk';
import {
  bytesText,
  dtcText,
  emptyTarget,
  hexByte,
  isTesterPresent,
  leaseText,
  needsSecondEnter,
  normalizeHex,
  nrcText,
  previewText,
  reduceTarget,
  reduceTargetState,
  resultSymbol,
  targetLine,
  timingText,
  updateLastValues,
  type LastValue,
  type TargetView,
} from './udsModel';

export const PREVIEW_DEBOUNCE_MS = 150;
const SECOND_ENTER_MS = 3_000;
const MAX_TARGETS = 32; // C2: target bar entries (over → ignored + counted)

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
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

  // ---- DOM ----
  const root = el('div', 'uds-panel');
  root.tabIndex = 0;
  const bar = el('div', 'uds-targets');
  bar.setAttribute('data-testid', 'uds-targets');
  const cursorEl = el('span', 'uds-cursor muted');
  const tableHost = el('div', 'uds-table');
  const lastEl = el('div', 'uds-last');
  lastEl.setAttribute('data-testid', 'uds-last-values');
  const cmd = el('div', 'uds-cmd');
  const targetSel = el('select');
  targetSel.setAttribute('aria-label', '대상');
  const input = el('input');
  input.placeholder = '22 F190';
  input.setAttribute('aria-label', 'UDS 요청 (hex)');
  input.spellcheck = false;
  const preview = el('span', 'uds-preview muted');
  preview.setAttribute('data-testid', 'uds-preview');
  const sendBtn = el('button', undefined, '보내기 ⏎');
  sendBtn.type = 'button';
  const abortBtn = el('button', 'danger', '중단 !');
  abortBtn.type = 'button';
  abortBtn.title = '선택 대상의 진행 중 요청 중단 (이미 보낸 요청은 ⌛? 불확정이 될 수 있음)';
  const pollBtn = el('button', undefined, '폴링 정지');
  pollBtn.type = 'button';
  cmd.append('대상 ', targetSel, ' > ', input, ' ', preview, ' ', sendBtn, ' ', abortBtn, ' ', pollBtn);
  root.append(createLatchLine(ctx), bar, cursorEl, tableHost, lastEl, cmd);

  const table = createRowTable(tableHost, {
    columns: [
      { key: 'target', header: '대상', width: '5em', cell: (r) => str(r.payload.target) || (r.channel ?? '') },
      { key: 'service', header: '서비스', width: '13em', cell: (r) => serviceText(r.payload) },
      { key: 'summary', header: '요약', width: '1fr', cell: (r) => str(r.payload.summary) },
      { key: 'result', header: '결과', width: '10em', cell: (r) => resultSymbol(r) },
      { key: 'timing', header: 'p2/rtt', width: '7em', cell: (r, now) => timingText(r, now) },
    ],
    detail: (r: RowLike) => {
      const p = r.payload;
      const lines: [string, string][] = [
        ['request_id', r.requestId],
        // canonical bytes{encoding,data} / resp_bytes first, legacy req_hex / resp_hex second
        ['요청', bytesText(p.bytes) ?? bytesText(p.req_hex) ?? '—'],
        ['응답', bytesText(p.resp_bytes) ?? bytesText(p.resp_hex) ?? '—'],
        ['NRC', nrcText(p) ?? '—'],
      ];
      const dtcs = dtcText(p);
      if (dtcs !== null) lines.push(['DTC', dtcs]);
      const block = p.block && typeof p.block === 'object' ? (p.block as Record<string, unknown>) : null;
      if (block) lines.push(['차단', `${str(block.class)} ${str(block.reason)}`.trim()]);
      lines.push(
        ['출처', `${r.origin}${r.clientId ? ` ${r.clientId}` : ''}`],
        ['등급', r.risk ?? (typeof p.risk === 'string' ? p.risk : 'R?')],
        ['갱신', `${r.updates}회 · ${r.state}`],
      );
      return lines;
    },
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

  // ---- command line ----
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let previewAbort: AbortController | null = null;
  let previewRisk: unknown = null;
  let armedAt: number | null = null;

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

  input.addEventListener('input', () => {
    armedAt = null;
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(runPreview, PREVIEW_DEBOUNCE_MS);
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
  };

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.repeat) {
      e.preventDefault();
      submit();
    }
  });
  sendBtn.addEventListener('click', submit);
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
    ctx.scheduler.invalidate();
  });
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

  const renderTargets = () => {
    const env = ctx.env ?? '?';
    const tpBy = new Map<string, number>();
    for (const r of tpRows.list()) {
      const t = str(r.payload.target) || r.channel || '?';
      tpBy.set(t, (tpBy.get(t) ?? 0) + 1);
    }
    const children: HTMLElement[] = [];
    for (const t of targets.values()) {
      const line = el('div', 'uds-target');
      line.dataset.target = t.target;
      line.append(el('span', 'mono', targetLine(t, tpBy.get(t.target) ?? 0, env)), ' ─ ', el('span', 'lease', leaseText(t, ctx.connId)), ' ');
      const take = el('button', undefined, '제어 가져오기 T');
      take.type = 'button';
      take.disabled = !writable();
      take.addEventListener('click', () => void act('takeover', { target: t.target }));
      line.append(take);
      children.push(line);
    }
    if (children.length === 0) children.push(el('div', 'muted', '대상 없음 — 에이전트가 loom uds connect로 연결하면 나타납니다'));
    if (targetsOverflow) children.push(el('div', 'muted', `+${targetsOverflow} 대상 표시 안 됨`));
    bar.replaceChildren(...children);

    const opts = [...targets.keys()];
    if (opts.join('\u0000') !== [...targetSel.options].map((o) => o.value).join('\u0000')) {
      targetSel.replaceChildren(...opts.map((o) => Object.assign(el('option', undefined, o), { value: o })));
    }
    const cur = target();
    if (cur && targetSel.value !== cur) targetSel.value = cur;
  };

  const renderLastValues = () => {
    if (lastValues.size === 0) {
      lastEl.textContent = '마지막 값: —';
      return;
    }
    const now = performance.now();
    const parts = [...lastValues.values()]
      .slice(-12)
      .reverse()
      .map((v) => `${v.target}:${v.did} ${v.name ? `${v.name}=` : ''}${v.value}${v.unit ? ` ${v.unit}` : ''} (${Math.round((now - v.atMs) / 1000)}s)`);
    lastEl.textContent = `마지막 값: ${parts.join(' · ')}`;
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
    renderLastValues();
    const c = ctx.agentCursor.seq();
    cursorEl.textContent = c === null ? '' : `에이전트 확인 위치 seq:${c}`;
    const w = writable();
    input.disabled = !w;
    sendBtn.disabled = !w;
    pollBtn.disabled = !w;
    abortBtn.disabled = false; // safety direction: always available
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);

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
