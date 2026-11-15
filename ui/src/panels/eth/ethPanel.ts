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
// Fidelity (docs/impl/main/research/fidelity/eth.md, Wireshark main window Fig 3.1):
//   capture strip / filter / mode → [ list ═ details(tree) ═ bytes ] as ONE vertical stack with
//   two draggable+keyboard separators (role=separator) → bottom status bar (selection feedback,
//   shown/kept counts) → send drawer. Related-packet glyphs come only from eth.link facts.
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
  type StoreMirror,
  type TreeNode,
} from '../../sdk';
import { openAddChannel, openCaptureStart, openConnectInterface, openEditSend, openFollow, openKeys } from './dialogs';
import { loadConfigs, saveConfigs, upsertConfig, type IfaceConfig } from './ifaceConfigs';
import { createSocketDrawer } from './socketDrawer';
import { btn, divider, grow, icon, loadJson, saveJson, tbtn, vdiv } from '../../workbench/eth/bench';
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
  seqRange,
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
import '../../workbench/eth/bench.css';
import './eth.css';

export const QUERY_MAX = 2_000;
/** other-channel card refresh (read-only eth.capture.status) */
export const CH_REFRESH_MS = 5_000;
const MAX_PENDING_LINKS = 2_000; // links for packets not seen yet (C2)
const MAX_NIC_OBSERVED = 4_096; // tx request_ids with a captured copy (C2)
const ORG: Record<string, [string, string]> = {
  agent: ['AGT', 'org org-agt'],
  user: ['USR', 'org org-usr'],
  device: ['DEV', 'org org-dev'],
  script: ['SCR', 'org org-scr'],
};

/** Per-viewer layout prefs (selected web: list height % and detail width %, colour rules). Storage optional. */
export const PANE_KEY = 'loom.eth.layout.v2';
/** [list height %, details width %] — reference defaults `--list-h: 52%`, `--dtl-w: 55%` */
export const PANE_DEFAULT: readonly [number, number] = [52, 55];
interface EthPrefs {
  panes: [number, number];
  colorize: boolean;
}
const isPrefs = (v: unknown): v is EthPrefs => {
  const o = v as Partial<EthPrefs> | null;
  return !!o && Array.isArray(o.panes) && o.panes.length === 2 && o.panes.every((x) => typeof x === 'number' && x >= 12 && x <= 88) && typeof o.colorize === 'boolean';
};
const loadPrefs = (): EthPrefs => loadJson(PANE_KEY, isPrefs, { panes: [...PANE_DEFAULT], colorize: true });

/** Related-packet glyph (Wireshark Table 3.16 subset): only rows sharing a UDS request_id via eth.link. */
export function relGlyph(role: string, confidence: string): string {
  const exact = confidence === 'exact';
  if (role === 'request') return exact ? '→' : '⇢';
  if (role === 'response') return exact ? '←' : '⇠';
  return exact ? '•' : '◦';
}

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const big = (v: unknown): bigint | null => (typeof v === 'string' && /^\d{1,20}$/.test(v) ? BigInt(v) : null);

/** `eth.export` result as the daemon returns it (handlers.rs `Export`); unknown fields → null/0, never invented. */
export interface ExportResult {
  path: string | null;
  packets: number | null;
  bytes: number | null;
  truncated: boolean;
  /** loss entries listed for the exported range */
  gaps: number;
  commentsDropped: number;
}
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : null);
/** null when the action reported no result object at all (e.g. a source-compatible fake handle) */
export function readExportResult(v: unknown): ExportResult | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  return {
    path: typeof o.path === 'string' && o.path ? o.path : null,
    packets: num(o.packets),
    bytes: num(o.bytes),
    truncated: o.truncated === true,
    gaps: Array.isArray(o.gaps) ? o.gaps.length : 0,
    commentsDropped: num(o.comments_dropped) ?? 0,
  };
}
/** daemon write-row `error` ({code, message} or a string) → "E_LOCKED: message"; null when absent */
export function txReason(e: unknown): string | null {
  if (typeof e === 'string') return e || null;
  if (typeof e !== 'object' || e === null) return null;
  const o = e as Record<string, unknown>;
  const code = typeof o.code === 'string' ? o.code : '';
  const msg = typeof o.message === 'string' ? o.message : '';
  return code && msg ? `${code}: ${msg}` : code || msg || null;
}

/** Mirror of an UNBOUND panel: no server subscription exists (never an implicit lan1). */
const UNBOUND_MIRROR: StoreMirror<LoomEvent> = {
  snapshot: null,
  snapshotDone: false,
  headSeq: 0n,
  gaps: [],
  overwritten: 0,
  tail: () => [],
  after: () => [],
  project: () => Promise.reject(new Error('eth.packets: no channel bound')),
  onAppend: () => () => {},
};

/** props.channel when set; null = the unbound Ethernet module application (service tab before any capture) */
export function boundChannel(props: Readonly<Record<string, unknown>>): string | null {
  return typeof props.channel === 'string' && props.channel !== '' ? props.channel : null;
}

export function createEthPackets(ctx: PanelContext): PanelInstance {
  // Unbound (no props.channel): the full workbench mounts with real empty data and its own interface
  // configuration / [연결]. It never subscribes, captures, queries a journal, exports or sends for any alias;
  // [연결] → eth.capture.start of the alias the user chose → that channel's bound panel (ctx.ui.openPanel).
  const bound = boundChannel(ctx.props);
  const ch = bound ?? '';
  const canOperate = () => (ctx.rights.has('operate') || ctx.rights.has('approve')) && !ctx.writeLocked;
  let mirror = bound ? ctx.store('pkts') : UNBOUND_MIRROR;
  let lastSeq = -1n;
  let gapsSeen = 0;
  const live = new PacketStore();
  const pendingLinks = new Map<string, PktLink>();
  const nicObserved = new Set<string>();
  let capture: CaptureState | null = null;
  let captureGen: number | null = null;
  let captureSeq = -1n; // newest capture_state applied (control, backlog or snapshot high water)
  let snapshotVersion = -1;
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
  let relRid: string | null = null; // request_id of the selected packet's eth.link
  const prefs0 = loadPrefs();
  let panes: [number, number] = [prefs0.panes[0], prefs0.panes[1]];

  // ---- DOM (Bench Light layout of the selected web workbench) ----
  //   46px toolbar → channel card → filter bar (scope/mode + display filter)
  //   → [ list ═ (details | bytes) ] + socket drawer → status line
  const root = el('div', prefs0.colorize ? 'eth-panel colorize' : 'eth-panel');
  root.tabIndex = 0;
  root.dataset.module = 'eth';

  // toolbar
  const toolbar = el('div', 'wb-mtbar');
  toolbar.setAttribute('role', 'toolbar');
  toolbar.setAttribute('aria-label', 'Ethernet 작업 공간');
  toolbar.setAttribute('data-testid', 'eth-toolbar');
  // order and labels = selected web toolbar: 채널 추가 · 모두 시작 · 모두 정지 | 표시 일시정지 · 자동 스크롤 · 색상 |
  // 스트림 따라가기 · 내보내기 · 기록 지우기 | 소켓 테스트 · 단축키. This panel is one daemon channel (props.channel);
  // more channels are more eth.packets panels in the Dockview, so "모두" acts on this panel's channel.
  const addBtn = tbtn('plus', '채널 추가', '새 채널 이름 + NIC로 캡처 시작 (eth.capture.start) → 그 채널 패널 열기', 'add-channel');
  const startBtn = tbtn('play', '캡처 시작…', '정지된 채널 캡처 시작 (eth.capture.start)', 'start-all');
  const stopBtn = tbtn('stop', '캡처 정지', '캡처 정지 — 안전 방향(보기 자격도 가능), 기록 유지 (eth.capture.stop)', 'stop-all');
  const pauseBtn = tbtn('pause', '표시 일시정지', '화면만 고정하고 저널에서 완전 범위 조회 — 캡처는 계속 (Space)', 'pause-view');
  const autoBtn = tbtn('scroll', '자동 스크롤', '새 패킷으로 자동 스크롤', 'autoscroll');
  autoBtn.setAttribute('aria-pressed', 'true');
  const colorBtn = tbtn('color', '색상', '프로토콜 색상 규칙', 'colorize');
  colorBtn.setAttribute('aria-pressed', String(prefs0.colorize));
  const followBtn = tbtn('follow', '스트림 따라가기', '선택 패킷의 TCP/UDP 대화 따라가기 (daemon eth.follow)', 'follow');
  const EXPORT_TITLE = '표시 범위를 pcapng로 내보내기 (eth.export) — 파일은 daemon(벤치) 머신에 생성, 완료 시 경로 표시';
  const exportBtn = tbtn('down', '내보내기', EXPORT_TITLE, 'export');
  const clearBtn = tbtn('trash', '기록 지우기', '이 패널의 보관 목록만 비움 — 저널·녹화 기록은 유지', 'clear-view');
  const sockBtn = tbtn('send', '소켓 테스트', 'UDP/TCP 소켓 테스트 패널 (Alt+S)', 'toggle-socket');
  sockBtn.setAttribute('aria-pressed', 'false');
  const keysBtn = tbtn('kbd', '단축키', '단축키 (?)', 'keys');
  const tbCtx = el('span', 'wb-ctx');
  tbCtx.setAttribute('data-testid', 'eth-ctx');
  toolbar.append(addBtn, startBtn, stopBtn, vdiv(), pauseBtn, autoBtn, colorBtn, vdiv(), followBtn, exportBtn, clearBtn, vdiv(), sockBtn, keysBtn, grow(), tbCtx);
  // time reference (not a toolbar item in the selected web): scope row + detail pane header
  const timeSel = el('select', 'wb-tsel');
  timeSel.setAttribute('aria-label', '시간 기준');
  timeSel.title = 'Time 열 기준';
  for (const [v, t] of [['rel', '캡처 시작 기준'], ['abs', '절대 시각'], ['delta', '이전 행 대비'], ['ref', '기준 행 대비']] as const) {
    timeSel.append(Object.assign(el('option', undefined, t), { value: v }));
  }
  const refBtn = btn('기준 행', 'wb-btn', '선택 행을 시간 기준으로');
  const copyBtn = btn('REF 복사', 'wb-btn', '선택 패킷 증거 REF 복사');

  // channel card (real capture_state / stats)
  const chbar = el('section', 'eth-chbar');
  chbar.setAttribute('aria-label', '캡처 채널');
  const status = el('div', 'eth-chcard');
  status.setAttribute('data-testid', 'eth-status');
  const chDot = el('span', 'ch-dot');
  const chName = el('span', 'ch-name');
  const chAlias = el('span', 'alias', bound ?? '채널 미선택');
  const chState = el('span', 'wb-chip stopped', '?');
  const chSim = el('span', 'wb-chip sim', 'SIM tap');
  chSim.title = 'daemon이 보고한 시뮬레이션 탭 (실제 NIC 아님)';
  chName.append(chAlias, chState, chSim);
  const statusText = el('span', 'ch-meta');
  const chActs = el('span', 'ch-acts');
  const cardStart = btn('', 'wb-btn', '캡처 시작… (eth.capture.start)');
  cardStart.append(icon('play'), '시작');
  const cardStop = btn('', 'wb-btn', '캡처 정지 (eth.capture.stop)');
  cardStop.append(icon('stop'), '정지');
  chActs.append(cardStart, cardStop);
  status.append(chDot, chName, chActs, statusText);
  const permEl = el('div', 'eth-perm');
  permEl.hidden = true;
  // other daemon channels (selected web: multi-card strip). Real eth.capture.status only (the control
  // path is topic-filtered to this channel, sdk/context.ts onControl), refreshed every CH_REFRESH_MS
  // while mounted; their packet statistics live in their own panel and are not shown here.
  status.classList.add('active');
  status.setAttribute('aria-current', 'true');
  status.title = bound ? `${ch} — 이 패널이 보여 주는 채널` : '연결된 채널 없음 — 인터페이스 구성을 고르고 [연결]';
  // ---- unbound: saved / new interface configuration + explicit [연결] ----
  let cfgs: IfaceConfig[] = bound ? [] : loadConfigs();
  const NEW_CFG = '\u0000new';
  const cfgSel = el('select', 'wb-tsel eth-cfg-sel');
  cfgSel.setAttribute('aria-label', '인터페이스 구성');
  cfgSel.setAttribute('data-testid', 'eth-iface-config');
  cfgSel.title = '이 브라우저에 저장된 캡처 인터페이스 구성 (선택만으로 캡처하지 않음)';
  const cfgConnect = btn('', 'wb-btn primary', '선택한 구성으로 캡처 시작 (eth.capture.start) — 확인 대화상자에서 [연결]을 눌러야 시작');
  cfgConnect.append(icon('play'), '연결…');
  cfgConnect.setAttribute('data-cmd', 'iface-connect');
  const cfgEdit = btn('구성 편집…', 'wb-btn', '구성 편집 — 저장만 하면 캡처하지 않음');
  cfgEdit.setAttribute('data-cmd', 'iface-edit');
  const cfgDel = btn('삭제', 'wb-btn', '저장된 구성 삭제 (로컬만 — daemon 채널·기록은 그대로)');
  cfgDel.setAttribute('data-cmd', 'iface-delete');
  const selectedCfg = (): IfaceConfig | null => cfgs.find((c) => c.alias === cfgSel.value) ?? null;
  let cfgKey = '';
  const renderCfgs = () => {
    const key = JSON.stringify(cfgs.map((c) => [c.alias, c.iface, c.backend]));
    if (key === cfgKey) return;
    cfgKey = key;
    const prev = cfgSel.value;
    cfgSel.replaceChildren(
      ...cfgs.map((c) => Object.assign(el('option', undefined, `${c.alias} · ${c.iface}${c.backend === 'sim' ? ' (SIM)' : ''}`), { value: c.alias })),
      Object.assign(el('option', undefined, '새 구성…'), { value: NEW_CFG }),
    );
    cfgSel.value = cfgs.some((c) => c.alias === prev) ? prev : (cfgs[0]?.alias ?? NEW_CFG);
  };
  const storeCfg = (c: IfaceConfig, previousAlias: string | null) => {
    cfgs = upsertConfig(cfgs, c, previousAlias ?? undefined);
    saveConfigs(cfgs);
    renderCfgs();
    cfgSel.value = c.alias;
    ctx.scheduler.invalidate();
  };
  const connectCfg = (initial: IfaceConfig | null) =>
    openConnectInterface(dialogHost, ctx, initial, {
      onSave: (c, prevAlias) => {
        storeCfg(c, prevAlias);
        ctx.ui.toast(`구성 ${c.alias} 저장 — 캡처는 시작하지 않았습니다`);
      },
      onStarted: (newCh, c) => {
        storeCfg(c, initial?.alias ?? null);
        void refreshChannels();
        ctx.ui.openPanel('eth.packets', { channel: newCh });
        ctx.ui.toast(`채널 ${newCh} 캡처 시작 — 그 채널 패널을 열었습니다`);
      },
    });
  if (!bound) {
    renderCfgs();
    cfgConnect.addEventListener('click', () => connectCfg(selectedCfg()));
    cfgEdit.addEventListener('click', () => connectCfg(selectedCfg()));
    cfgDel.addEventListener('click', () => {
      const c = selectedCfg();
      if (!c) return;
      cfgs = cfgs.filter((x) => x.alias !== c.alias);
      saveConfigs(cfgs);
      renderCfgs();
      ctx.scheduler.invalidate();
    });
    cfgSel.addEventListener('change', () => ctx.scheduler.invalidate());
    status.dataset.unbound = 'true';
    chActs.replaceChildren(cfgSel, cfgConnect, cfgEdit, cfgDel); // the channel card's start/stop need a channel
  }
  const otherHost = el('span', 'eth-chothers');
  otherHost.setAttribute('data-testid', 'eth-other-channels');
  const addTile = btn('', 'eth-ch-add', '채널 추가 — 새 채널 이름 + NIC로 캡처 시작');
  addTile.append(icon('plus'), '채널');
  addTile.setAttribute('data-cmd', 'add-channel-tile');
  chbar.append(status, otherHost, addTile, permEl);
  interface OtherCh {
    state: string;
    backend: string | null;
    iface: string | null;
  }
  const others = new Map<string, OtherCh>();
  let othersKey = '';
  let channelsErr = false;
  const MAX_OTHER_CH = 32;
  const strOrNull = (v: unknown): string | null => (typeof v === 'string' && v ? v : null);
  const noteOther = (name: unknown, p: Record<string, unknown>, replace = false) => {
    if (typeof name !== 'string' || !name || name === ch) return;
    if (!others.has(name) && others.size >= MAX_OTHER_CH) return;
    const prev = replace ? undefined : others.get(name);
    others.set(name, {
      state: typeof p.state === 'string' ? p.state.toLowerCase() : (prev?.state ?? '?'),
      backend: strOrNull(p.backend) ?? prev?.backend ?? null,
      iface: strOrNull(p.iface) ?? prev?.iface ?? null,
    });
  };
  const refreshChannels = () =>
    ctx
      .query<{ channels?: Record<string, unknown>[] }>('eth.capture.status', {})
      .then((r) => {
        channelsErr = false;
        others.clear();
        for (const c of r?.channels ?? []) noteOther(c.ch, c, true);
        ctx.scheduler.invalidate();
      })
      .catch(() => {
        channelsErr = true;
        ctx.scheduler.invalidate();
      });
  const renderOthers = () => {
    const list = [...others.entries()].sort(([a], [b]) => a.localeCompare(b));
    const key = JSON.stringify([list, channelsErr]);
    if (key === othersKey) return;
    othersKey = key;
    const cards: HTMLElement[] = list.map(([name, o]) => {
      const live = o.state === 'live' || o.state === 'running' || o.state === 'capturing' || o.state === 'replaying';
      const card = el('div', 'eth-chcard other');
      card.dataset.state = o.state;
      card.dataset.channel = name;
      card.tabIndex = 0;
      card.setAttribute('role', 'button');
      card.title = `${name} 채널 패널 열기 (이 패널의 목록에는 섞지 않음)`;
      const nm = el('span', 'ch-name');
      nm.append(el('span', 'alias', name), el('span', `wb-chip ${live ? 'live' : o.state === 'error' || o.state === 'permission_denied' ? 'err' : o.state === '?' ? 'pend' : 'stopped'}`, o.state === '?' ? '상태 미상' : o.state.toUpperCase()));
      if (o.backend === 'sim') nm.append(el('span', 'wb-chip sim', 'SIM'));
      const open = btn('열기', 'wb-btn', `${name} 패널 열기`);
      const acts = el('span', 'ch-acts');
      acts.append(open);
      const meta = el('span', 'ch-meta', [o.iface ?? 'iface ?', o.backend ?? 'backend ?', '통계는 그 채널 패널에서'].join(' · '));
      card.append(el('span', 'ch-dot'), nm, acts, meta);
      const go = () => ctx.ui.openPanel('eth.packets', { channel: name });
      card.addEventListener('click', go);
      card.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          go();
        }
      });
      return card;
    });
    if (channelsErr) cards.push(Object.assign(el('span', 'eth-ch-note', '다른 채널 목록 조회 실패'), { title: 'eth.capture.status' }));
    otherHost.replaceChildren(...cards);
  };
  void refreshChannels();
  const chTimer = setInterval(() => void refreshChannels(), CH_REFRESH_MS);

  // filter bar: scope row (capture filter, live/complete mode) + display filter row
  const fbar = el('section', 'eth-fbar');
  fbar.setAttribute('aria-label', '필터');
  const scopeRow = el('div', 'eth-frow');
  const scopeLab = el('span', 'eth-flab', '범위');
  scopeLab.title = '이 패널의 채널 · 캡처 필터는 캡처 시점에 적용';
  const capf = el('span', 'eth-capf');
  const chipCh = el('span', 'wb-seg');
  const chipBtn = btn(bound ?? '채널 미선택', '');
  chipBtn.setAttribute('aria-checked', 'true');
  chipBtn.setAttribute('role', 'radio');
  chipCh.setAttribute('role', 'radiogroup');
  chipCh.setAttribute('aria-label', '표시 채널 범위');
  chipCh.append(chipBtn);
  const modeBar = el('div', 'eth-frow eth-mode-row');
  modeBar.setAttribute('data-testid', 'eth-mode-row');
  const modeBadge = el('span', 'eth-mode-badge');
  modeBadge.setAttribute('data-testid', 'eth-mode');
  scopeRow.append(scopeLab, chipCh, capf, grow(), timeSel, modeBadge);
  const dfCount = el('span', 'eth-dfcount');
  dfCount.setAttribute('data-testid', 'eth-dfcount');
  dfCount.setAttribute('aria-live', 'polite');
  const filterHost = el('div', 'eth-filter-host');
  filterHost.setAttribute('data-testid', 'eth-filter-host');
  fbar.append(scopeRow, filterHost);

  // capture workspace
  const wsp = el('div', 'eth-wsp');
  const listHost = el('div', 'eth-list');
  const panesEl = el('section', 'eth-panes');
  panesEl.setAttribute('data-testid', 'eth-panes');
  panesEl.setAttribute('aria-label', '패킷 캡처 작업 공간');
  const splitA = el('div', 'eth-split eth-split-a');
  splitA.setAttribute('data-testid', 'eth-split-list');
  const lower = el('div', 'eth-lower');
  panesEl.append(listHost, splitA, lower);
  const detailBar = el('div', 'eth-detail-bar');
  detailBar.setAttribute('data-testid', 'eth-detail-bar');
  const resendBtn = btn('재전송', 'wb-btn', '선택 프레임을 그대로 raw 송신 (eth.send)');
  const editBtn = btn('편집 후 송신…', 'wb-btn', 'hex 편집 + daemon dry-run 미리보기 후 송신');
  const detailInfo = el('span', 'eth-info');
  detailInfo.setAttribute('data-testid', 'eth-detail-info');
  const expandBtn = btn('＋', 'wb-ibtn', '모두 펼치기');
  const collapseBtn = btn('－', 'wb-ibtn', '모두 접기');
  detailBar.append(el('span', undefined, '패킷 상세'), detailInfo, grow(), resendBtn, editBtn, refBtn, copyBtn, expandBtn, collapseBtn);
  lower.append(detailBar);
  // Wireshark details pane: protocol layers start collapsed; node selection feeds the status bar.
  const treeHex = new TreeHex(lower, { collapseTopLayers: true, onSelectNode: () => ctx.scheduler.invalidate() });
  treeHex.root.classList.add('eth-treehex');
  const splitB = el('div', 'eth-split eth-split-b');
  splitB.setAttribute('data-testid', 'eth-split-bytes');
  const bytesHead = el('div', 'eth-bytes-h');
  const bytesSel = el('span', 'eth-info');
  bytesHead.append(el('span', undefined, '바이트'), grow(), bytesSel);
  // DOM order = visual order (tab / screen reader): tree, separator, bytes
  const thRight = treeHex.root.querySelector('.th-right');
  treeHex.root.insertBefore(splitB, thRight);
  treeHex.root.insertBefore(bytesHead, thRight);
  treeHex.clear('행을 고르면 상세 트리와 바이트가 보입니다');

  const statusBar = el('div', 'eth-statusbar');
  statusBar.setAttribute('data-testid', 'eth-statusbar');
  statusBar.setAttribute('role', 'status');
  const sbField = el('span', 'eth-sb-field');
  sbField.setAttribute('data-testid', 'eth-sb-field');
  const sbCount = el('span', 'eth-sb-count');
  sbCount.setAttribute('data-testid', 'eth-sb-count');
  const keepEl = el('span', 'eth-sb-keep');
  keepEl.setAttribute('data-testid', 'eth-sb-keep');
  statusBar.append(sbField, sbCount, keepEl);
  const dialogHost = el('div');

  const sock = createSocketDrawer(ctx, ch, () => toggleSock(false));
  const txHost = sock.txHost;
  const txSum = sock.txSum;
  wsp.append(panesEl, sock.root);
  root.append(toolbar, chbar, fbar, wsp, statusBar, dialogHost);

  const toggleSock = (v = !sock.open) => {
    if (!bound && v) return; // socket test sends on a channel: needs a connected alias
    sock.setOpen(v);
    sockBtn.setAttribute('aria-pressed', String(v));
  };

  // ---- resizable panes (pointer drag + keyboard), per-viewer ----
  const applyPanes = () => {
    panesEl.style.setProperty('--eth-list-h', `${panes[0]}%`);
    lower.style.setProperty('--eth-dtl-w', `${panes[1]}%`);
    splitA.setAttribute('aria-valuenow', String(Math.round(panes[0])));
    splitB.setAttribute('aria-valuenow', String(Math.round(panes[1])));
  };
  const savePrefs = () => saveJson(PANE_KEY, { panes: panes.map((x) => Math.round(x * 10) / 10), colorize: root.classList.contains('colorize') });
  const setPane = (i: 0 | 1, v: number, persist: boolean) => {
    panes = i === 0 ? [v, panes[1]] : [panes[0], v];
    applyPanes();
    if (persist) savePrefs();
  };
  divider(splitA, { orientation: 'horizontal', label: '패킷 목록 | 상세 경계', container: () => panesEl, get: () => panes[0], set: (v, p) => setPane(0, v, p), def: PANE_DEFAULT[0] });
  divider(splitB, { orientation: 'vertical', label: '상세 | 바이트 경계', container: () => lower, get: () => panes[1], set: (v, p) => setPane(1, v, p), def: PANE_DEFAULT[1] });
  applyPanes();
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => {
      ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    });

  // ---- list ----
  const items = (): PktRow[] => (mode === 'complete' ? (complete?.rows ?? []) : live.rows);
  const cols: Column<PktRow>[] = [
    {
      key: 'rel',
      header: '',
      width: '1.6em',
      cell: (r) => {
        if (!relRid || r.kind !== 'pkt' || r.link?.requestId !== relRid) return '';
        return { text: relGlyph(r.link.role, r.link.confidence), cls: `eth-rel eth-rel-${r.link.confidence === 'exact' ? 'exact' : 'probable'}`, title: `선택 행과 같은 UDS 요청 ${relRid} · ${r.link.role} (${r.link.confidence})` };
      },
    },
    { key: 'seq', header: 'No.', width: '64px', cell: (r) => (r.kind === 'gap' && r.gap ? { text: gapRowText(r.gap), cls: 'gap-text' } : String(r.seq)) },
    {
      key: 'time',
      header: 'Time',
      width: '92px',
      cell: (r) => fmtTime(r, timeMode, { startMonoNs: capture?.startMonoNs ?? null, prevMonoNs: prevMono.get(r.key) ?? null, refMonoNs: refMono }),
    },
    { key: 'src', header: 'Source', width: '136px', cell: (r) => r.src },
    { key: 'dst', header: 'Destination', width: '136px', cell: (r) => r.dst },
    { key: 'proto', header: 'Protocol', width: '84px', cell: (r) => r.proto },
    { key: 'len', header: 'Length', width: '58px', cell: (r) => (r.kind === 'pkt' ? { text: String(r.len), cls: 'eth-num' } : '') },
    { key: 'info', header: 'Info', width: 'minmax(240px, 1fr)', cell: (r) => r.info },
    {
      key: 'link',
      header: 'UDS',
      width: '52px',
      cell: (r) =>
        r.link
          ? { text: r.link.confidence === 'exact' ? '↔UDS' : '↔UDS?', cls: `chip chip-${r.link.confidence}`, title: `UDS ${r.link.requestId} ${r.link.role} (${r.link.confidence}) — 클릭: UDS 콘솔 행으로` }
          : '',
    },
    {
      key: 'org',
      header: '출처',
      width: '44px',
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
      {
        key: 'sum',
        header: '요약',
        width: '1fr',
        cell: (r) => {
          const base = String(r.payload.summary ?? r.payload.to ?? '');
          // daemon's final failed row carries error{code,message} (e.g. E_LOCKED) — show it, never a bare ✗
          const why = r.state === 'failed' || r.state === 'outcome_unknown' ? txReason(r.payload.error) : null;
          if (!why) return base;
          return { text: base ? `${base} — ${why}` : why, cls: 'res-fail eth-tx-why', title: `송신 실패 사유 (daemon): ${why}` };
        },
      },
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
      if (!bound) {
        // unbound: no subscription to push down to (never an implicit channel)
        if (expr) ctx.ui.toast('표시 필터는 채널 연결 후 그 채널 패널에서 적용됩니다');
        return;
      }
      // live pushdown: new server subscription with filter.ext{eth_filter}
      mirror = ctx.resubscribe('pkts', expr ? { eth_filter: expr } : null);
      lastSeq = -1n;
      gapsSeen = 0;
      offAppend();
      offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
      goComplete('filter'); // honesty: show the complete filtered range from the journal
    },
  });
  filterHost.querySelector('.eth-filter-row')?.append(dfCount);

  // ---- live / complete ----
  const headSeq = (): bigint => {
    for (let i = live.rows.length - 1; i >= 0; i--) if (live.rows[i]!.kind === 'pkt') return live.rows[i]!.seq;
    return mirror.headSeq;
  };

  const goComplete = (reason: string) => {
    if (!bound) return; // no journal range without a channel: the empty live view stays as it is
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
        { ch, range: seqRange(null, to > 0n ? to : null), ...(expr ? { filter: expr } : {}), max: QUERY_MAX, detail: false },
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
    relRid = selected?.kind === 'pkt' ? (selected.link?.requestId ?? null) : null;
    itemsDirty = true; // re-render related-packet glyphs
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
      .query<Record<string, unknown>>('eth.show', { ref: selected.ref }, ac.signal)
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
  const stopCapture = () => {
    if (bound) void act('captureStop', { ch });
  };
  const startCapture = () => (bound ? openCaptureStart(dialogHost, ctx, ch) : connectCfg(selectedCfg()));
  stopBtn.addEventListener('click', stopCapture);
  cardStop.addEventListener('click', stopCapture);
  startBtn.addEventListener('click', startCapture);
  cardStart.addEventListener('click', startCapture);
  const addChannel = () => (bound ? openAddChannel(dialogHost, ctx, ch) : connectCfg(null));
  addBtn.addEventListener('click', addChannel);
  addTile.addEventListener('click', addChannel);
  // Auto-scroll = the list follows the live tail. Leaving the tail keeps the view still by switching to the
  // complete (journal) range — the same honesty rule as scrolling back (sampled live list never shown as complete).
  autoBtn.addEventListener('click', () => {
    if (mode === 'live' && table.following) goComplete('autoscroll');
    else {
      goLive();
      table.followTail();
    }
    ctx.scheduler.invalidate();
  });
  colorBtn.addEventListener('click', () => {
    const on = !root.classList.contains('colorize');
    root.classList.toggle('colorize', on);
    colorBtn.setAttribute('aria-pressed', String(on));
    savePrefs();
  });
  followBtn.addEventListener('click', () => {
    if (selected?.kind !== 'pkt' || selected.stream === null) return;
    const proto = /udp/i.test(selected.proto) || /(^|\.)udp/.test(selected.proto.toLowerCase()) ? 'udp' : 'tcp';
    openFollow(dialogHost, ctx, ch, proto, selected.stream, (expr) => filter.setExpr(expr));
  });
  clearBtn.addEventListener('click', () => {
    live.clear();
    pendingLinks.clear();
    if (selected) onSelect(null);
    if (mode === 'complete') goLive();
    itemsDirty = true;
    ctx.ui.toast('보관 목록을 비웠습니다 — 저널·녹화 기록은 그대로입니다');
    ctx.scheduler.invalidate();
  });
  keysBtn.addEventListener('click', () => openKeys(dialogHost));
  const foldAll = (open: boolean) => {
    for (const node of root.querySelectorAll<HTMLElement>('.th-tree .th-node[aria-expanded]')) {
      const kids = node.nextElementSibling;
      if (!(kids instanceof HTMLElement) || !kids.classList.contains('th-kids')) continue;
      kids.hidden = !open;
      node.setAttribute('aria-expanded', String(open));
      const t = node.querySelector('.th-tog');
      if (t) t.textContent = open ? '▾' : '▸';
    }
  };
  expandBtn.addEventListener('click', () => foldAll(true));
  collapseBtn.addEventListener('click', () => foldAll(false));
  // eth.export is synchronous and bounded (no progress feed): busy while the RPC runs, then the daemon's
  // actual result {path, packets, bytes, truncated, gaps, comments_dropped}. Errors stay errors (act() toast).
  let exportDlg: { close(): void } | null = null;
  let disposed = false;
  exportBtn.addEventListener('click', () => {
    if (exportBtn.disabled || !bound) return;
    const range = mode === 'complete' && complete ? seqRange(complete.from, complete.to) : undefined;
    exportBtn.disabled = true;
    exportBtn.setAttribute('aria-busy', 'true');
    exportBtn.title = '내보내는 중… (daemon이 파일을 쓰는 중)';
    void act('export', { ch, format: 'pcapng', ...(filter.applied ? { filter: filter.applied } : {}), ...(range ? { range } : {}) })
      .then((h) => {
        if (!h || disposed) return;
        const r = readExportResult(h.result);
        if (!r) {
          ctx.ui.toast('pcapng 내보내기 완료 — daemon이 결과 정보를 주지 않았습니다');
          return;
        }
        exportDlg?.close();
        exportDlg = openExportResult(r);
        ctx.ui.toast(r.path ? `pcapng 내보내기 완료: ${r.path}` : 'pcapng 내보내기 완료 — 경로 정보 없음');
      })
      .finally(() => {
        exportBtn.disabled = false;
        exportBtn.removeAttribute('aria-busy');
        exportBtn.title = EXPORT_TITLE;
      });
  });
  const openExportResult = (r: ExportResult): { close(): void } => {
    const overlay = el('div', 'eth-dialog-overlay');
    const box = el('div', 'eth-dialog');
    box.setAttribute('role', 'dialog');
    box.setAttribute('aria-label', 'pcapng 내보내기 결과');
    box.setAttribute('data-testid', 'eth-export-result');
    const body = el('div');
    const close = () => {
      overlay.remove();
      if (exportDlg?.close === close) exportDlg = null;
    };
    const kv = el('div', 'eth-kv');
    const pathEl = el('code', 'eth-export-path', r.path ?? '응답에 경로 없음');
    pathEl.setAttribute('data-testid', 'eth-export-path');
    const add = (k: string, v: HTMLElement | string, cls?: string) => {
      const vEl = typeof v === 'string' ? el('span', cls, v) : v;
      kv.append(el('span', 'muted', k), vEl);
    };
    add('파일 (daemon 머신)', pathEl);
    add('패킷', r.packets === null ? '?' : String(r.packets));
    add('바이트', r.bytes === null ? '?' : `${r.bytes} B`);
    if (r.truncated) add('잘림', '예 — 한도/스캔 예산에 도달해 일부만 저장됨', 'res-fail');
    if (r.gaps > 0) add('손실 구간', `${r.gaps}건 — 이 범위에 캡처 손실이 있었음`, 'res-fail');
    if (r.commentsDropped > 0) add('주석 생략', `${r.commentsDropped}개 (pcapng 한도)`);
    const note = el('p', 'muted', '파일은 Loom daemon이 실행 중인 머신(벤치)에 저장됩니다 — 브라우저 다운로드가 아닙니다.');
    const actions = el('div', 'eth-dialog-actions');
    if (r.path) {
      const copy = btn('경로 복사', 'wb-btn', '파일 경로를 클립보드로 복사');
      copy.setAttribute('data-testid', 'eth-export-copy');
      const p = r.path;
      copy.addEventListener('click', () => {
        const done = navigator.clipboard?.writeText(p);
        if (!done) ctx.ui.toast(p);
        else done.then(() => ctx.ui.toast(`복사: ${p}`)).catch(() => ctx.ui.toast(p));
      });
      actions.append(copy);
    }
    const closeBtn = btn('닫기 (Esc)', 'wb-btn');
    closeBtn.addEventListener('click', close);
    actions.append(closeBtn);
    overlay.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        close();
      }
    });
    body.append(kv, note, actions);
    box.append(el('header', undefined, 'pcapng 내보내기 완료'), body);
    overlay.append(box);
    dialogHost.append(overlay);
    closeBtn.focus();
    return { close };
  };
  pauseBtn.addEventListener('click', togglePause);
  timeSel.addEventListener('change', () => {
    timeMode = timeSel.value as TimeMode;
    itemsDirty = true;
    ctx.scheduler.invalidate();
  });
  resendBtn.addEventListener('click', () => {
    if (detailHex && bound) void act('send', { kind: 'raw', ch, hex: detailHex });
  });
  editBtn.addEventListener('click', () => {
    if (detailHex && bound) openEditSend(dialogHost, ctx, ch, detailHex, null);
  });
  sockBtn.addEventListener('click', () => toggleSock());
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
  const stepPacket = (dir: 1 | -1) => {
    const list = items().filter((r) => r.kind === 'pkt');
    if (!list.length) return;
    const i = selected ? list.findIndex((r) => r.key === selected!.key) : -1;
    const j = i < 0 ? (dir > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + dir));
    const key = list[j]!.key;
    table.select(key);
    table.reveal(key);
  };
  root.addEventListener('keydown', (e) => {
    // Wireshark Table 3.1 subset: Ctrl+/ filter focus, F8/Ctrl+↓ next, F7/Ctrl+↑ previous packet
    if ((e.ctrlKey || e.metaKey) && e.key === '/') {
      e.preventDefault();
      filter.focus();
      return;
    }
    if (e.key === 'F8' || e.key === 'F7' || ((e.ctrlKey || e.metaKey) && (e.key === 'ArrowDown' || e.key === 'ArrowUp'))) {
      e.preventDefault();
      stepPacket(e.key === 'F8' || e.key === 'ArrowDown' ? 1 : -1);
      return;
    }
    if (e.altKey && (e.code === 'KeyS' || e.key === 's' || e.key === 'S' || e.key === 'ß')) {
      e.preventDefault();
      toggleSock();
      return;
    }
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
    if ((e.target as HTMLElement).closest?.('.th-tree, .eth-split, .eth-sock')) return; // tree keys (fold/toggle) stay in the tree
    if (e.key === '?' && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      openKeys(dialogHost);
      return;
    }
    if (e.key === ' ' && !e.repeat) {
      e.preventDefault();
      togglePause();
    }
  });

  // ---- consume live stream ----
  /** A new capture generation never inherits the previous start anchor (worker clock, kept as reported). */
  const applyCapture = (p: Record<string, unknown>, generation: number | null, seq: bigint | null = null) => {
    // control path and backlog/snapshot can both carry a state: never let an older one override a newer one
    if (seq !== null) {
      if (seq <= captureSeq) return;
      captureSeq = seq;
    }
    const prev = generation !== null && captureGen !== null && generation !== captureGen && capture ? { ...capture, startMonoNs: null } : capture;
    if (generation !== null) captureGen = generation;
    capture = readCaptureState(p, prev);
  };
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
        applyCapture(p, typeof ev.generation === 'number' ? ev.generation : null, BigInt(ev.seq));
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
    const snap = mirror.snapshot;
    if (snap && snap.version !== snapshotVersion) {
      snapshotVersion = snap.version;
      for (const item of snap.items) {
        const st = obj(item.state);
        if (item.domain !== 'eth' || st.kind !== 'eth.capture_state' || (st.ch ?? item.channel) !== ch) continue;
        applyCapture(st, typeof st.generation === 'number' ? st.generation : null, snap.throughSeq);
      }
    }
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
    status.dataset.state = !bound ? 'UNBOUND' : perm ? 'PERMISSION' : st;
    const live_ = st === 'running' || st === 'live' || st === 'capturing' || st === 'replaying';
    chState.textContent = !bound ? '미연결' : st === '?' ? '상태 미수신' : st.toUpperCase();
    chState.className = `wb-chip ${!bound ? 'stopped' : live_ ? 'live' : st === 'error' || perm ? 'err' : st === '?' ? 'pend' : 'stopped'}`;
    chSim.hidden = !capture?.sim;
    chSim.textContent = capture?.sim ? 'SIM tap' : '';
    if (!bound) renderCfgs();
    statusText.textContent = !bound
      ? `인터페이스 구성을 고르거나 새로 만든 뒤 [연결…] — 이 탭을 여는 것만으로는 캡처·송신하지 않습니다${others.size ? ' · 이미 있는 daemon 채널은 오른쪽 카드 [열기]' : ''}`
      : [
      ...(capture?.iface ? [capture.iface] : []),
      ...(st === 'error' && capture?.error ? [`오류: ${capture.error}`] : []),
      `pps ${stats.pps === null ? '?' : fmtCount(stats.pps)}`,
      `bps ${fmtRate(stats.bps)}`,
      drop,
      `기록 ${ctx.recordHealth}`,
      `ts ${lastTsSource}${capture?.tsPrecision ? ` (${capture.tsPrecision})` : ''}`,
      ].join(' · ');
    capf.replaceChildren(el('span', 'k', '캡처 필터 '), el('b', undefined, capture?.bpf || '(없음)'), capture?.bpf ? ' (캡처 정지 후 변경)' : '');
    status.classList.toggle('has-drop', dropTotal > 0n);
    permEl.hidden = !perm;
    permEl.textContent = '캡처 불가(권한) — 벤치 터미널에서 `loom doctor`. DoIP 진단 연결은 영향 없음.';

    renderOthers();
    addTile.disabled = !canOperate();
    // mode bar
    if (mode === 'live') {
      modeBadge.dataset.mode = 'live';
      modeBadge.textContent = `라이브 · ${samplingText(coverage, ctx.recordHealth)}`;
      pauseBtn.label.textContent = '표시 일시정지';
      pauseBtn.classList.remove('paused');
      pauseBtn.setAttribute('aria-pressed', 'false');
    } else {
      modeBadge.dataset.mode = 'complete';
      pauseBtn.label.textContent = '라이브로';
      pauseBtn.classList.add('paused');
      pauseBtn.setAttribute('aria-pressed', 'true');
      if (completeState === 'loading') modeBadge.textContent = '완전 범위 조회 중… (저널)';
      else if (completeState === 'error') modeBadge.textContent = `완전 범위 조회 실패: ${completeErr}`;
      else if (complete) {
        const r = complete.from !== null && complete.to !== null ? ` seq ${complete.from}–${complete.to}` : '';
        modeBadge.textContent = `완전(저널 기준) · ${complete.rows.length.toLocaleString('ko-KR')}행${r}${complete.truncated ? ` · 최대 ${QUERY_MAX.toLocaleString('ko-KR')}행 — 범위를 좁히세요` : ''}`;
      }
    }
    // bottom status bar (Wireshark Fig 3.22/3.24): selected field · shown/kept · selection
    const sel = selected;
    const fieldLabel = treeHex.selectedLabel;
    const hl = treeHex.highlighted;
    statusBar.dataset.selected = sel?.kind === 'pkt' ? 'packet' : sel ? 'gap' : 'none';
    sbField.textContent =
      sel?.kind === 'pkt' && fieldLabel
        ? `${fieldLabel}${hl ? ` · ${hl.len} byte${hl.len === 1 ? '' : 's'} @0x${hl.off.toString(16).padStart(4, '0')}` : ''}`
        : sel?.kind === 'pkt'
          ? `선택 seq ${sel.seq} · ${sel.proto} · ${sel.len} B${relRid ? ` · UDS ${relRid}` : ''}`
          : sel
            ? '손실 구간 선택 — 상세 없음'
            : '패킷 선택 없음 — ↑/↓ 또는 F7/F8';
    const shown = items();
    let nPkt = 0;
    let nGap = 0;
    for (const r of shown) {
      if (r.kind === 'pkt') nPkt++;
      else nGap++;
    }
    const fx = filter.applied ? `표시 필터 적용(서버)` : '표시 필터 없음';
    sbCount.textContent =
      mode === 'complete'
        ? `완전 범위 패킷 ${nPkt.toLocaleString('ko-KR')}${nGap ? ` · 손실 행 ${nGap}` : ''} · ${fx}`
        : `라이브 표시 패킷 ${nPkt.toLocaleString('ko-KR')}${nGap ? ` · 손실 행 ${nGap}` : ''} · ${fx}`;
    dfCount.replaceChildren('표시 ', el('b', undefined, nPkt.toLocaleString('ko-KR')), ' / 보관 ', el('b', undefined, live.rows.length.toLocaleString('ko-KR')));
    keepEl.textContent = `보관 ${live.rows.length.toLocaleString('ko-KR')}행${live.evicted ? ` · 이전 ${live.evicted.toLocaleString('ko-KR')}행 보관 범위 밖 → 일시정지하면 저널에서 조회` : ''}`;

    // actions
    const op = canOperate();
    resendBtn.disabled = !op || !detailHex || !bound;
    editBtn.disabled = !op || !detailHex || !bound;
    sockBtn.disabled = !op || !bound;
    sockBtn.title = bound ? 'UDP/TCP 소켓 테스트 패널 (Alt+S)' : '소켓 테스트는 연결된 채널에서 — 먼저 [연결…]';
    startBtn.disabled = addBtn.disabled = cardStart.disabled = !op;
    cfgConnect.disabled = !op;
    // editing a new/saved config is local (no RPC); only [삭제] needs a saved one
    cfgDel.disabled = !selectedCfg();
    sock.setWritable(op && bound !== null);
    exportBtn.disabled = !bound || exportBtn.getAttribute('aria-busy') === 'true';
    // safety direction: enabled unless the capture is known to have ended (stopping it again does nothing)
    stopBtn.disabled = cardStop.disabled = !bound || st === 'stopped' || st === 'error' || st === 'permission_denied' || st === 'replay_done';
    cardStart.hidden = live_;
    cardStop.hidden = !live_ && st !== '?';
    autoBtn.setAttribute('aria-pressed', String(mode === 'live' && table.following));
    followBtn.disabled = selected?.kind !== 'pkt' || selected.stream === null;
    followBtn.title = followBtn.disabled ? '스트림 번호가 있는 TCP/UDP 패킷을 선택하세요 (daemon eth.follow)' : `선택 패킷의 대화 따라가기 — stream ${selected?.kind === 'pkt' ? selected.stream : ''}`;
    tbCtx.replaceChildren(
      ...(selected?.kind === 'pkt'
        ? [el('b', undefined, `#${selected.seq}`), ` ${selected.proto} · ${selected.len} B · ${ch}`]
        : [bound ? `${ch} · 표시 ${nPkt.toLocaleString('ko-KR')}` : '채널 미선택 · 연결 전']),
    );
    bytesSel.textContent = hl ? `offset 0x${hl.off.toString(16).padStart(4, '0')} · ${hl.len} byte${hl.len === 1 ? '' : 's'}` : '';
    refBtn.disabled = selected?.kind !== 'pkt';
    copyBtn.disabled = selected?.kind !== 'pkt';
    detailInfo.textContent = selected?.kind === 'pkt' ? `${selected.ref}${detailMs !== null ? ` · 상세 ${detailMs} ms` : ''}` : '';
    return lastSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);
  // live capture state arrives on the control path only (priority kind); apply it for this channel
  const offControl = ctx.onControl((ev) => {
    if (ev.kind !== 'eth.capture_state') return;
    const p = obj(ev.payload);
    if (!bound) {
      void refreshChannels(); // unbound: another channel started/stopped → its card (read-only status)
      return;
    }
    if ((ev.channel ?? p.ch) !== ch) return;
    void refreshChannels(); // a state change here (e.g. after 채널 추가) → refresh the other cards too
    applyCapture(p, typeof ev.generation === 'number' ? ev.generation : null, BigInt(ev.seq));
    ctx.scheduler.invalidate();
  });

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      disposed = true;
      exportDlg?.close();
      completeAc?.abort();
      detailAc?.abort();
      offAppend();
      offTx();
      offFrame();
      offControl();
      clearInterval(chTimer);
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
        // no props.channel = unbound module application: the panel never opens this topic (no subscription);
        // onControl then only refreshes the read-only channel cards. Never an implicit lan1.
        filter: (p) => ({
          domains: ['eth'],
          ...(boundChannel(p) ? { channels: [boundChannel(p)!] } : {}),
          kinds: ['eth.pkt', 'eth.batch', 'eth.tx', 'eth.link', 'eth.capture_state', 'eth.stats', 'source.gap', 'record.gap', 'display.gap'],
        }),
        snapshot: 'required',
        maxBatchMs: 40,
        mirrorRows: 4_000,
      },
    ],
    // eth.send is also an action: the context allows it as a query only with dry_run:true
    queries: ['eth.query', 'eth.show', 'eth.filter.check', 'eth.ifaces', 'eth.send', 'eth.follow', 'eth.capture.status'],
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
