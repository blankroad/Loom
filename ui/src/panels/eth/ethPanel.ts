// eth.packets (F2, UI-M1) — Wireshark-style packet panel. Content spec: _xteam/eth-ui.md.
//
//   status bar   capture state · pps/bps · kernel drop total · record · ts source/precision
//                [■ 캡처 정지] (safety direction, watch OK) [캡처 시작…] [⇩ pcapng]
//   filter bar   server-checked display filter → pushdown via filter.ext{eth_filter}
//   mode bar     LIVE "표시 1/N 샘플링 · 기록 100 %"  |  COMPLETE "완전(저널 기준)"
//   list         seq · time(rel/abs/delta/ref) · src · dst · proto · len · info · ↔UDS · origin
//   TX rows      eth.tx via RowTable (stages + "NIC 관찰 ✓")
//   detail       tree ↔ hex (two-way range sync) + row actions
//
// Honesty (eth-ui §실시간성): the live list may be sampled; pausing (Space), scrolling
// back or changing the filter switches to `eth.query` over the journal so a complete
// range is shown and labelled as such. Kernel drops (source.gap) become red gap rows and
// the badge total equals the sum of their `count`.

import {
  CHANNEL_PROP,
  createRowTable,
  formatRequestRef,
  isFinal,
  registerPanel,
  TreeHex,
  VirtualTable,
  decodePayloadBytes,
  type Column,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type RowLike,
  type TreeNode,
} from '../../sdk';
import { openCaptureStart, openEditSend, openSocketSend } from './dialogs';
import {
  fmtTime,
  fromEvent,
  gapRow,
  gapRowText,
  PacketStore,
  readBatch,
  readCaptureState,
  readStats,
  statsCoverage,
  EMPTY_STATS,
  fmtCount,
  fmtRate,
  u64,
  type CaptureStats,
  readLinkEvent,
  readPkt,
  samplingText,
  type BatchCoverage,
  type CaptureState,
  type PktLink,
  type PktRow,
  type TimeMode,
} from './ethModel';
import { FilterBar } from './filterBar';

export const QUERY_MAX = 2_000;
const MAX_PENDING_LINKS = 2_000; // links for packets not seen yet (C2)
const MAX_NIC_OBSERVED = 4_096; // tx request_ids with a captured copy (C2)
const ORG: Record<string, [string, string]> = {
  agent: ['AGT', 'org org-agt'],
  user: ['USR', 'org org-usr'],
  device: ['DEV', 'org org-dev'],
  script: ['SCR', 'org org-scr'],
};

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const big = (v: unknown): bigint | null => (typeof v === 'string' && /^\d{1,20}$/.test(v) ? BigInt(v) : null);

export function createEthPackets(ctx: PanelContext): PanelInstance {
  const ch = typeof ctx.props.channel === 'string' ? ctx.props.channel : 'lan1';
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  let mirror = ctx.store('pkts');
  let lastSeq = -1n;
  let gapsSeen = 0;
  const live = new PacketStore();
  const pendingLinks = new Map<string, PktLink>();
  const nicObserved = new Set<string>();
  let capture: CaptureState | null = null;
  let stats: CaptureStats = EMPTY_STATS;
  let statsSeen = false;
  let coverage: BatchCoverage | null = null;
  // drop totals since the panel opened: from source.gap counts and from per-second stats.
  // Both describe the same driver counters; the larger one is shown (a lower bound, never a false 0).
  let gapDrop = 0n;
  let statsDrop = 0n;
  /** some drop amount was not reported (unknown count / stats unavailable) → total is only a lower bound */
  let dropLowerBound = false;
  let lastTsSource = '?';

  let mode: 'live' | 'complete' = 'live';
  let complete: { rows: PktRow[]; from: bigint | null; to: bigint | null; truncated: boolean; reason: string } | null = null;
  let completeState: 'idle' | 'loading' | 'error' = 'idle';
  let completeErr = '';
  let completeAc: AbortController | null = null;
  let timeMode: TimeMode = 'rel';
  let refMono: bigint | null = null;
  let selected: PktRow | null = null;
  let detailHex: string | null = null;
  let detailAc: AbortController | null = null;
  let detailMs: number | null = null;
  let itemsDirty = true;
  let prevMono = new Map<string, bigint>();

  // ---- DOM ----
  const root = el('div', 'eth-panel');
  root.tabIndex = 0;
  const status = el('div', 'eth-status');
  status.setAttribute('data-testid', 'eth-status');
  const statusText = el('span');
  const stopBtn = el('button', 'danger', '■ 캡처 정지');
  stopBtn.type = 'button';
  stopBtn.title = '캡처 정지 — 안전 방향(보기 자격도 가능)';
  const startBtn = el('button', undefined, '캡처 시작…');
  startBtn.type = 'button';
  const exportBtn = el('button', undefined, '⇩ pcapng');
  exportBtn.type = 'button';
  status.append(statusText, stopBtn, startBtn, exportBtn);
  const permEl = el('div', 'eth-perm');
  permEl.hidden = true;

  const filterHost = el('div');
  const modeBar = el('div', 'eth-mode');
  const modeBadge = el('span', 'eth-badge');
  modeBadge.setAttribute('data-testid', 'eth-mode');
  const pauseBtn = el('button', undefined, '⏸ 일시정지');
  pauseBtn.type = 'button';
  const timeSel = el('select');
  timeSel.setAttribute('aria-label', '시간 기준');
  for (const [v, t] of [['rel', '캡처 시작 기준'], ['abs', '절대 시각'], ['delta', '이전 행 대비'], ['ref', '기준 행 대비']] as const) {
    timeSel.append(Object.assign(el('option', undefined, t), { value: v }));
  }
  const keepEl = el('span', 'muted');
  modeBar.append(modeBadge, pauseBtn, timeSel, keepEl);

  const listHost = el('div', 'eth-list');
  const txWrap = el('details', 'eth-tx');
  const txSum = el('summary', undefined, '송신');
  const txHost = el('div', 'eth-tx-host');
  txWrap.append(txSum, txHost);
  const detail = el('div', 'eth-detail');
  const detailBar = el('div', 'eth-detail-bar');
  const resendBtn = el('button', undefined, '재전송');
  const editBtn = el('button', undefined, '편집 후 송신…');
  const sockBtn = el('button', undefined, 'UDP/TCP 송신…');
  const refBtn = el('button', undefined, '이 행을 시간 기준으로');
  const copyBtn = el('button', undefined, '증거 REF 복사');
  const detailInfo = el('span', 'muted');
  detailInfo.setAttribute('data-testid', 'eth-detail-info');
  for (const b of [resendBtn, editBtn, sockBtn, refBtn, copyBtn]) b.type = 'button';
  detailBar.append(resendBtn, editBtn, sockBtn, refBtn, copyBtn, detailInfo);
  const treeHost = el('div', 'eth-treehex');
  detail.append(detailBar, treeHost);
  const dialogHost = el('div');
  root.append(status, permEl, filterHost, modeBar, listHost, txWrap, detail, dialogHost);

  const treeHex = new TreeHex(treeHost);
  treeHex.clear('행을 고르면 상세 트리와 바이트가 보입니다');

  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- list ----
  const items = (): PktRow[] => (mode === 'complete' ? (complete?.rows ?? []) : live.rows);
  const cols: Column<PktRow>[] = [
    { key: 'seq', header: 'seq', width: '6em', cell: (r) => (r.kind === 'gap' && r.gap ? { text: gapRowText(r.gap), cls: 'gap-text' } : String(r.seq)) },
    {
      key: 'time',
      header: '시간',
      width: '9em',
      cell: (r) => fmtTime(r, timeMode, { startMonoNs: capture?.startMonoNs ?? live.rows.find((x) => x.kind === 'pkt')?.monoNs ?? null, prevMonoNs: prevMono.get(r.key) ?? null, refMonoNs: refMono }),
    },
    { key: 'src', header: '출발지', width: '10em', cell: (r) => r.src },
    { key: 'dst', header: '목적지', width: '10em', cell: (r) => r.dst },
    { key: 'proto', header: '프로토콜', width: '6.5em', cell: (r) => r.proto },
    { key: 'len', header: '길이', width: '3.5em', cell: (r) => (r.kind === 'pkt' ? String(r.len) : '') },
    { key: 'info', header: '정보', width: '1fr', cell: (r) => r.info },
    {
      key: 'link',
      header: 'UDS',
      width: '4em',
      cell: (r) =>
        r.link
          ? { text: r.link.confidence === 'exact' ? '↔UDS' : '↔UDS?', cls: `chip chip-${r.link.confidence}`, title: `UDS ${r.link.requestId} ${r.link.role} (${r.link.confidence}) — 클릭: UDS 콘솔 행으로` }
          : '',
    },
    {
      key: 'org',
      header: '출처',
      width: '3.5em',
      cell: (r) => {
        if (r.kind === 'gap') return '';
        const [t, c] = ORG[r.badge] ?? ['?', 'org'];
        return { text: t, cls: c, title: r.attributed ? `송신 주체(attributed_origin) — 기록 origin은 device` : r.badge };
      },
    },
  ];
  const table = new VirtualTable<PktRow>(listHost, {
    columns: cols,
    keyOf: (r) => r.key,
    rowAttrs: (r): Record<string, string> => {
      const a: Record<string, string> = { 'data-origin': r.badge };
      if (r.kind === 'gap' && r.gap) a['data-gap'] = r.gap.kind;
      else a['data-seq'] = String(r.seq);
      const rid = r.link?.requestId ?? r.txRequestId;
      if (rid) a['data-request-id'] = rid;
      return a;
    },
    rowClass: (r) => r.color,
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: (key) => onSelect(key),
    onCellClick: (r, col) => {
      if (col === 'link' && r.link) {
        ctx.ui.reveal(formatRequestRef('uds', r.link.target, r.link.requestId));
        return true;
      }
      return false;
    },
    onScrollAway: () => {
      if (mode === 'live') goComplete('scrollback');
    },
    emptyText: '패킷 없음',
    ariaLabel: '패킷 목록',
  });

  // ---- TX rows (eth.tx, shared RowStore) ----
  const txRows = ctx.rows({ domain: 'eth', kinds: ['eth.tx'] });
  const nicCell = (r: RowLike): string => {
    if (nicObserved.has(r.requestId) || r.payload.nic_observed === true) return 'NIC 관찰 ✓';
    return isFinal(r.state) || r.state === 'sent' ? 'NIC 관찰 —' : '';
  };
  const txTable = createRowTable(txHost, {
    columns: [
      { key: 'kind', header: '종류', width: '4em', cell: (r) => String(r.payload.kind ?? 'raw') },
      { key: 'sum', header: '요약', width: '1fr', cell: (r) => String(r.payload.summary ?? r.payload.to ?? '') },
      { key: 'nic', header: 'NIC', width: '7em', cell: (r) => ({ text: nicCell(r), cls: nicCell(r).includes('✓') ? 'res-ok' : 'muted' }) },
    ],
    onNeedRender: () => ctx.scheduler.invalidate(),
    ariaLabel: '이더넷 송신',
    emptyText: '송신 없음',
  });

  // ---- filter ----
  const filter = new FilterBar(filterHost, {
    check: (expr, signal) => ctx.query('eth.filter.check', { expr }, signal),
    apply: (expr) => {
      // live pushdown: new server subscription with filter.ext{eth_filter}
      mirror = ctx.resubscribe('pkts', expr ? { eth_filter: expr } : null);
      lastSeq = -1n;
      gapsSeen = 0;
      offAppend();
      offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
      goComplete('filter'); // honesty: show the complete filtered range from the journal
    },
  });

  // ---- live / complete ----
  const headSeq = (): bigint => {
    for (let i = live.rows.length - 1; i >= 0; i--) if (live.rows[i]!.kind === 'pkt') return live.rows[i]!.seq;
    return mirror.headSeq;
  };

  const goComplete = (reason: string) => {
    mode = 'complete';
    completeState = 'loading';
    completeAc?.abort();
    const ac = new AbortController();
    completeAc = ac;
    const to = headSeq();
    const expr = filter.applied;
    itemsDirty = true;
    ctx
      .query<{ rows?: unknown[]; range?: { from?: string; to?: string }; truncated?: boolean }>(
        'eth.query',
        { ch, range: { to: String(to) }, ...(expr ? { filter: expr } : {}), max: QUERY_MAX, detail: false },
        ac.signal,
      )
      .then((res) => {
        if (ac.signal.aborted) return;
        const rows = (res?.rows ?? []).map((r) => readPkt(obj(r), ch)).filter((r): r is PktRow => r !== null);
        complete = {
          rows,
          from: big(res?.range?.from) ?? rows[0]?.seq ?? null,
          to: big(res?.range?.to) ?? rows[rows.length - 1]?.seq ?? to,
          truncated: res?.truncated === true || rows.length >= QUERY_MAX,
          reason,
        };
        completeState = 'idle';
        itemsDirty = true;
        ctx.scheduler.invalidate();
      })
      .catch((e: unknown) => {
        if (ac.signal.aborted) return;
        completeState = 'error';
        completeErr = e instanceof Error ? e.message : String(e);
        ctx.scheduler.invalidate();
      });
    ctx.scheduler.invalidate();
  };

  const goLive = () => {
    mode = 'live';
    complete = null;
    completeState = 'idle';
    completeAc?.abort();
    itemsDirty = true;
    table.followTail();
  };

  const togglePause = () => (mode === 'live' ? goComplete('pause') : goLive());

  // ---- detail ----
  const onSelect = (key: string | null) => {
    detailAc?.abort();
    detailHex = null;
    selected = key ? (items().find((r) => r.key === key) ?? null) : null;
    if (!selected || selected.kind !== 'pkt') {
      treeHex.clear(selected ? '손실 구간 — 상세 없음' : '행을 고르면 상세 트리와 바이트가 보입니다');
      ctx.scheduler.invalidate();
      return;
    }
    const ac = new AbortController();
    detailAc = ac;
    const t0 = performance.now();
    treeHex.clear('불러오는 중…');
    ctx
      .query<Record<string, unknown>>('eth.query', { ref: selected.ref, detail: true }, ac.signal)
      .then((res) => {
        if (ac.signal.aborted) return;
        detailMs = Math.round(performance.now() - t0);
        const bytes = decodePayloadBytes(res?.bytes) ?? (typeof res?.hex === 'string' ? decodePayloadBytes({ encoding: 'hex', data: res.hex }) : null);
        detailHex = bytes && res?.truncated !== true ? [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('') : null;
        const raw = typeof res?.raw_url === 'string' ? res.raw_url : null;
        treeHex.set(
          Array.isArray(res?.tree) ? (res.tree as TreeNode[]) : [],
          bytes,
          raw ? { text: '큰 프레임 — 원본 받기 (/api/v1/raw)', href: raw } : res?.truncated === true ? { text: '잘린 프레임 (snaplen)' } : null,
        );
        ctx.scheduler.invalidate();
      })
      .catch((e: unknown) => {
        if (!ac.signal.aborted) treeHex.clear(`상세 실패: ${e instanceof Error ? e.message : String(e)}`);
      });
    ctx.scheduler.invalidate();
  };

  // ---- buttons & keys ----
  stopBtn.addEventListener('click', () => void act('captureStop', { ch }));
  startBtn.addEventListener('click', () => openCaptureStart(dialogHost, ctx, ch));
  exportBtn.addEventListener('click', () => {
    const range = mode === 'complete' && complete ? { from: String(complete.from ?? ''), to: String(complete.to ?? '') } : undefined;
    void act('export', { ch, format: 'pcapng', ...(filter.applied ? { filter: filter.applied } : {}), ...(range ? { range } : {}) }).then((h) => {
      if (h) ctx.ui.toast(`pcapng 내보내기 시작${h.requestId ? ` (${h.requestId})` : ''} — 진행률은 Activity`);
    });
  });
  pauseBtn.addEventListener('click', togglePause);
  timeSel.addEventListener('change', () => {
    timeMode = timeSel.value as TimeMode;
    itemsDirty = true;
    ctx.scheduler.invalidate();
  });
  resendBtn.addEventListener('click', () => {
    if (detailHex) void act('send', { kind: 'raw', ch, hex: detailHex });
  });
  editBtn.addEventListener('click', () => {
    if (detailHex) openEditSend(dialogHost, ctx, ch, detailHex, null);
  });
  sockBtn.addEventListener('click', () => openSocketSend(dialogHost, ctx, ch));
  refBtn.addEventListener('click', () => {
    if (selected?.kind === 'pkt') {
      refMono = selected.monoNs;
      timeMode = 'ref';
      timeSel.value = 'ref';
      itemsDirty = true;
      ctx.scheduler.invalidate();
    }
  });
  copyBtn.addEventListener('click', () => {
    if (!selected?.ref) return;
    navigator.clipboard
      ?.writeText(selected.ref)
      .then(() => ctx.ui.toast(`복사: ${selected?.ref}`))
      .catch(() => ctx.ui.toast(selected?.ref ?? ''));
  });
  root.addEventListener('keydown', (e) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
    if (e.key === ' ' && !e.repeat) {
      e.preventDefault();
      togglePause();
    }
  });

  // ---- consume live stream ----
  const handle = (ev: LoomEvent) => {
    const p = obj(ev.payload);
    switch (ev.kind) {
      case 'eth.pkt': {
        const seq = BigInt(ev.seq);
        if (live.get(seq)) return; // re-snapshot after resubscribe
        const r = fromEvent(ev);
        if (!r) return;
        const pend = pendingLinks.get(String(seq));
        if (pend && !r.link) {
          r.link = pend;
          pendingLinks.delete(String(seq));
        }
        if (r.txRequestId) noteNic(r.txRequestId);
        lastTsSource = r.tsSource;
        live.push(r);
        itemsDirty = true;
        return;
      }
      case 'eth.batch':
        coverage = readBatch(p) ?? coverage;
        return;
      case 'eth.link': {
        const { seq, link, ethTx } = readLinkEvent(p);
        if (ethTx) noteNic(ethTx);
        if (link && seq !== null) {
          const row = live.get(seq);
          if (row) row.link = link;
          else if (pendingLinks.size < MAX_PENDING_LINKS) pendingLinks.set(String(seq), link);
          itemsDirty = true;
        }
        return;
      }
      case 'eth.capture_state':
        capture = readCaptureState(p, capture);
        return;
      case 'eth.stats':
        stats = readStats(p, stats);
        statsSeen = true;
        if (stats.kernelDrop !== null) statsDrop += stats.kernelDrop;
        if (stats.ifDrop !== null) statsDrop += stats.ifDrop;
        // Any unreported interval stays unknown even after later counters recover.
        if (stats.kernelDrop === null || stats.ifDrop === null) dropLowerBound = true;
        coverage = statsCoverage(stats) ?? coverage;
        return;
      case 'source.gap':
      case 'record.gap':
      case 'display.gap': {
        // gap payload: seq_range{from,to} (provisional) — {first,last} accepted too (CORE d2 naming)
        const sr = obj(p.seq_range);
        const first = big(sr.from) ?? big(sr.first);
        const last = big(sr.to) ?? big(sr.last);
        const exact = u64(p.count);
        const count: bigint | 'unknown' = exact ?? 'unknown';
        const cause = typeof p.cause === 'string' ? p.cause : '?';
        if (ev.kind === 'source.gap' && (cause === 'kernel_drop' || cause === 'if_drop')) {
          if (exact === null) dropLowerBound = true;
          else gapDrop += exact;
        }
        if (ev.kind === 'source.gap' && cause === 'stats_unavailable') dropLowerBound = true;
        live.push(gapRow(ev.kind, count, cause, first, last, BigInt(ev.seq)));
        itemsDirty = true;
        return;
      }
      default:
        return; // eth.tx → RowStore (context), others ignored
    }
  };

  const noteNic = (rid: string) => {
    if (nicObserved.size >= MAX_NIC_OBSERVED) {
      const first = nicObserved.values().next().value;
      if (first !== undefined) nicObserved.delete(first);
    }
    nicObserved.add(rid);
  };

  let offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const offTx = txRows.onChange(() => ctx.scheduler.invalidate());

  const draw = (): bigint => {
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      handle(ev);
    }
    while (gapsSeen < mirror.gaps.length) {
      const g = mirror.gaps[gapsSeen++];
      if (g?.fromBatch) {
        live.push(gapRow('display.gap', g.count, g.cause, g.firstSeq, g.lastSeq, lastSeq));
        itemsDirty = true;
      }
    }

    if (itemsDirty) {
      const list = items();
      if (timeMode === 'delta') {
        prevMono = new Map();
        let prev: bigint | null = null;
        for (const r of list) {
          if (r.kind !== 'pkt') continue;
          if (prev !== null) prevMono.set(r.key, prev);
          prev = r.monoNs;
        }
      }
      table.setItems(list);
      itemsDirty = false;
    }
    table.render();
    txTable.setItems(txRows.list().map((row) => ({ t: 'row' as const, row })));
    txTable.render();
    txSum.textContent = `송신 (${txRows.list().length})`;

    // status bar
    const st = capture?.state ?? '?';
    const perm = st === 'permission_denied';
    const dropTotal = gapDrop > statsDrop ? gapDrop : statsDrop;
    // unknown now: no capture state yet, driver reports no drop counters, or the latest second had none
    const dropUnknownNow = !capture || capture.dropStats === 'unavailable' || (statsSeen && stats.kernelDrop === null && stats.ifDrop === null);
    const dropPartial = dropUnknownNow || dropLowerBound || capture?.dropStats === 'partial';
    const drop =
      dropTotal === 0n && (dropUnknownNow || dropLowerBound)
        ? '커널 드롭: 알 수 없음'
        : `커널 드롭 ${dropPartial ? '≥' : ''}${fmtCount(dropTotal)}`;
    status.dataset.state = perm ? 'PERMISSION' : st;
    statusText.textContent = [
      `eth.packets ─ ${ch} ● ${st.toUpperCase()}${capture?.sim ? ' [SIM tap]' : ''}`,
      ...(st === 'error' && capture?.error ? [`오류: ${capture.error}`] : []),
      `pps ${stats.pps === null ? '?' : fmtCount(stats.pps)}`,
      `bps ${fmtRate(stats.bps)}`,
      drop,
      `기록 ${ctx.recordHealth}`,
      `ts ${lastTsSource}${capture?.tsPrecision ? ` (${capture.tsPrecision})` : ''}`,
      ...(capture?.bpf ? [`캡처 필터: ${capture.bpf}`] : []),
    ].join('  ');
    status.classList.toggle('has-drop', dropTotal > 0n);
    permEl.hidden = !perm;
    permEl.textContent = '캡처 불가(권한) — 벤치 터미널에서 `loom doctor`. DoIP 진단 연결은 영향 없음.';

    // mode bar
    if (mode === 'live') {
      modeBadge.dataset.mode = 'live';
      modeBadge.textContent = `라이브 · ${samplingText(coverage, ctx.recordHealth)}`;
      pauseBtn.textContent = '⏸ 일시정지 (Space)';
    } else {
      modeBadge.dataset.mode = 'complete';
      pauseBtn.textContent = '▶ 라이브로 (Space)';
      if (completeState === 'loading') modeBadge.textContent = '완전 범위 조회 중… (저널)';
      else if (completeState === 'error') modeBadge.textContent = `완전 범위 조회 실패: ${completeErr}`;
      else if (complete) {
        const r = complete.from !== null && complete.to !== null ? ` seq ${complete.from}–${complete.to}` : '';
        modeBadge.textContent = `완전(저널 기준) · ${complete.rows.length.toLocaleString('ko-KR')}행${r}${complete.truncated ? ` · 최대 ${QUERY_MAX.toLocaleString('ko-KR')}행 — 범위를 좁히세요` : ''}`;
      }
    }
    keepEl.textContent = `보관 ${live.rows.length.toLocaleString('ko-KR')}행${live.evicted ? ` · 이전 ${live.evicted.toLocaleString('ko-KR')}행 보관 범위 밖 → 일시정지하면 저널에서 조회` : ''}`;

    // actions
    const op = canOperate();
    resendBtn.disabled = !op || !detailHex;
    editBtn.disabled = !op || !detailHex;
    sockBtn.disabled = !op;
    startBtn.disabled = !op;
    stopBtn.disabled = false; // safety direction
    refBtn.disabled = selected?.kind !== 'pkt';
    copyBtn.disabled = selected?.kind !== 'pkt';
    detailInfo.textContent = selected?.kind === 'pkt' ? `${selected.ref}${detailMs !== null ? ` · 상세 ${detailMs} ms` : ''}` : '';
    return lastSeq;
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
      completeAc?.abort();
      detailAc?.abort();
      offAppend();
      offTx();
      offFrame();
      filter.dispose();
      table.dispose();
      txTable.dispose();
      treeHex.dispose();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'eth.packets',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'eth',
    title: '이더넷 패킷',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'pkts',
        filter: (p) => ({
          domains: ['eth'],
          channels: [typeof p.channel === 'string' ? p.channel : 'lan1'],
          kinds: ['eth.pkt', 'eth.batch', 'eth.tx', 'eth.link', 'eth.capture_state', 'eth.stats', 'source.gap', 'record.gap', 'display.gap'],
        }),
        snapshot: 'required',
        maxBatchMs: 40,
        mirrorRows: 4_000,
      },
    ],
    // eth.send is also an action: the context allows it as a query only with dry_run:true
    queries: ['eth.query', 'eth.filter.check', 'eth.ifaces', 'eth.send'],
    actions: [
      { id: 'send', rpc: 'eth.send', class: 'request', right: 'operate', confirm: 'none', label: '이더넷 송신' },
      { id: 'captureStart', rpc: 'eth.capture.start', class: 'job', right: 'operate', confirm: 'none', label: '캡처 시작' },
      { id: 'captureStop', rpc: 'eth.capture.stop', class: 'job', right: 'watch', confirm: 'none', safetyDirection: 'reduce', label: '캡처 정지' },
      { id: 'export', rpc: 'eth.export', class: 'job', right: 'watch', confirm: 'none', label: 'pcapng 내보내기' },
    ],
    memory: { maxBytes: 8 * 1024 * 1024 }, // 4 MiB rows + mirror + detail
    render: { maxHz: 20, budgetMs: 4, lagTargetMs: 200 },
    shellSlots: ['channel_bar'],
  },
  createEthPackets,
);
