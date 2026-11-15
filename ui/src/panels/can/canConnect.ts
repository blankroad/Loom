// CAN module application — connect / interface section shown while can.trace has NO channel prop
// (module tab before any interface is bound; work/reports/module-apps/CONTRACT.md).
//
// Truthfulness (crates/loom-can): the daemon's CAN worker is `can_sim` (backend "sim") only.
// `can.open{channel, bus?, nodes?}` with nodes "default" (BCM sim nodes) | "none" (empty bus).
// SocketCAN / PCAN / Vector / Kvaser adapters do not exist in the engine — listed disabled, never mapped.
// Nothing here runs on mount: opening is the [연결] button (ctx.act → ActionGate, operate right);
// listing open channels is the [열린 채널 조회] button (`can.status{}` read-only query).

import type { PanelContext } from '../../sdk';

export const CAN_NAME_RE = /^[a-z][a-z0-9_-]{0,31}$/;

export type CanNodes = 'default' | 'none';
export interface CanOpenDraft {
  channel: string;
  /** empty = same as channel (daemon default) */
  bus: string;
  nodes: CanNodes;
}

export const CAN_BACKENDS: readonly { value: string; label: string; implemented: boolean }[] = [
  { value: 'sim', label: 'can_sim · 가상 버스', implemented: true },
  { value: 'socketcan', label: 'SocketCAN — 엔진 미구현', implemented: false },
  { value: 'pcan', label: 'PCAN / Vector / Kvaser — 엔진 미구현', implemented: false },
];

/** draft → exact `can.open` params (worker: channel required, bus defaults to channel, nodes default|none) */
export function canOpenParams(d: CanOpenDraft): { ok: true; params: { channel: string; bus?: string; nodes: CanNodes } } | { ok: false; error: string } {
  const channel = d.channel.trim();
  const bus = d.bus.trim();
  if (!CAN_NAME_RE.test(channel)) return { ok: false, error: '채널 이름: 영문 소문자로 시작하는 1–32자 (a-z 0-9 _ -)' };
  if (bus && !CAN_NAME_RE.test(bus)) return { ok: false, error: '버스 이름: 영문 소문자로 시작하는 1–32자 (비우면 채널 이름)' };
  if (d.nodes !== 'default' && d.nodes !== 'none') return { ok: false, error: '노드 구성을 선택하세요' };
  return { ok: true, params: { channel, ...(bus && bus !== channel ? { bus } : {}), nodes: d.nodes } };
}

export function canOpenCli(d: CanOpenDraft): string {
  const r = canOpenParams(d);
  if (!r.ok) return 'loom can open --channel <이름>';
  return `loom can open --channel ${r.params.channel}${r.params.bus ? ` --bus ${r.params.bus}` : ''} --nodes ${r.params.nodes}`;
}

export interface OpenChannelInfo {
  channel: string;
  bus: string | null;
  backend: string | null;
  state: string | null;
}

/** `can.status{}` result → open channel rows (unknown fields stay null, never guessed) */
export function readCanStatus(r: unknown): OpenChannelInfo[] {
  const o = typeof r === 'object' && r !== null ? (r as Record<string, unknown>) : {};
  const d = typeof o.data === 'object' && o.data !== null ? (o.data as Record<string, unknown>) : o;
  const list = Array.isArray(d.channels) ? d.channels : [];
  const top = typeof d.backend === 'string' ? d.backend : null;
  const str = (v: unknown) => (typeof v === 'string' && v ? v : null);
  return list.slice(0, 64).flatMap((c) => {
    const x = typeof c === 'object' && c !== null ? (c as Record<string, unknown>) : {};
    const channel = str(x.channel) ?? str(x.name);
    return channel ? [{ channel, bus: str(x.bus), backend: str(x.backend) ?? top, state: str(x.state) }] : [];
  });
}

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const tid = <E extends HTMLElement>(e: E, id: string): E => {
  e.dataset.testid = id;
  return e;
};

export interface CanConnectSection {
  root: HTMLElement;
  /** called each frame: rights / write lock */
  update(writable: boolean, hasRight: boolean): void;
  dispose(): void;
}

/** The unbound module's interface section. `view` = the view the bound panel should open with. */
export function createCanConnect(ctx: PanelContext, view: () => string): CanConnectSection {
  const draft: CanOpenDraft = { channel: '', bus: '', nodes: 'default' };
  let pending = false;
  let listing = false;
  let disposed = false;
  const listAbort = new AbortController();

  const root = tid(el('section', 'ct-connect'), 'can-connect');
  root.setAttribute('aria-label', 'CAN 인터페이스 연결');
  const head = el('div', 'ct-connect-head');
  head.append(
    el('strong', undefined, 'CAN 인터페이스'),
    tid(el('span', 'ct-unbound', '채널 미선택 — 아무 장치도 열지 않았습니다. 아래에서 명시적으로 연결하거나 열린 채널을 고르세요.'), 'can-unbound'),
  );
  const grid = el('div', 'ct-connect-grid');
  const field = (label: string, input: HTMLElement) => {
    const l = el('label', 'ct-field');
    l.append(el('span', 'muted', label), input);
    return l;
  };
  const chIn = tid(el('input'), 'can-connect-channel');
  chIn.placeholder = '예: ch0, body-a';
  chIn.maxLength = 32;
  chIn.autocomplete = 'off';
  chIn.spellcheck = false;
  const busIn = tid(el('input'), 'can-connect-bus');
  busIn.placeholder = '= 채널 이름 (같은 버스 공유 시 입력)';
  busIn.maxLength = 32;
  busIn.autocomplete = 'off';
  busIn.spellcheck = false;
  const backSel = tid(el('select'), 'can-connect-backend');
  for (const b of CAN_BACKENDS) {
    const o = el('option', undefined, b.label);
    o.value = b.value;
    o.disabled = !b.implemented;
    backSel.append(o);
  }
  backSel.value = 'sim';
  const nodeSel = tid(el('select'), 'can-connect-nodes');
  for (const [v, t] of [['default', '기본 시뮬 노드 (BCM)'], ['none', '노드 없음 (빈 버스)']] as const) {
    const o = el('option', undefined, t);
    o.value = v;
    nodeSel.append(o);
  }
  const openBtn = tid(el('button', 'ct-connect-btn', '연결'), 'can-connect-open');
  openBtn.type = 'button';
  grid.append(field('채널 이름', chIn), field('버스', busIn), field('백엔드', backSel), field('노드', nodeSel), openBtn);
  const err = tid(el('div', 'ct-connect-err'), 'can-connect-error');
  err.setAttribute('role', 'status');
  const cli = tid(el('code', 'ct-connect-cli'), 'can-connect-cli');
  const note = el('div', 'muted ct-connect-note', '백엔드: 데몬 CAN 엔진은 현재 can_sim(가상 버스)만 구현되어 있습니다. 실제 CAN 어댑터는 연결할 수 없습니다.');

  const listRow = el('div', 'ct-open-list');
  const listBtn = tid(el('button', undefined, '열린 채널 조회'), 'can-list-open');
  listBtn.type = 'button';
  listBtn.title = 'can.status (읽기 전용) — 이미 열린 채널을 보기만 합니다. 아무것도 열거나 보내지 않습니다.';
  const listHost = tid(el('span', 'ct-open-items'), 'can-open-channels');
  listRow.append(listBtn, listHost);
  root.append(head, grid, err, cli, note, listRow);

  const readDraft = () => {
    draft.channel = chIn.value;
    draft.bus = busIn.value;
    draft.nodes = nodeSel.value === 'none' ? 'none' : 'default';
    const r = canOpenParams(draft);
    const typed = draft.channel.trim() !== '' || draft.bus.trim() !== '';
    err.textContent = r.ok || !typed ? '' : r.error;
    chIn.setAttribute('aria-invalid', String(typed && !r.ok && !CAN_NAME_RE.test(draft.channel.trim())));
    cli.textContent = canOpenCli(draft);
    ctx.scheduler.invalidate(); // button enablement follows the draft on the next frame
  };
  for (const i of [chIn, busIn, nodeSel]) i.addEventListener('input', readDraft);
  nodeSel.addEventListener('change', readDraft);
  readDraft();

  const observe = (channel: string) => ctx.ui.openPanel('can.trace', { channel, view: view() });

  openBtn.addEventListener('click', () => {
    readDraft();
    const r = canOpenParams(draft);
    if (!r.ok) {
      err.textContent = r.error;
      return;
    }
    if (pending) return;
    pending = true;
    openBtn.disabled = true;
    err.textContent = ''; // button stays disabled while pending; errors only in this line
    ctx.act('open', r.params).then(
      () => {
        if (disposed) return;
        err.textContent = '';
        ctx.ui.toast(`CAN ${r.params.channel} 열기 요청됨 (can_sim) — 실제 상태는 채널 패널에서 확인`);
        observe(r.params.channel);
      },
      (e: unknown) => {
        if (disposed) return;
        const code = (e as { code?: string; dataCode?: string }).dataCode ?? (e as { code?: string }).code;
        err.textContent = `연결 실패${code ? ` (${code})` : ''}: ${e instanceof Error ? e.message : String(e)}`;
      },
    ).finally(() => {
      pending = false;
      ctx.scheduler.invalidate();
    });
  });

  listBtn.addEventListener('click', () => {
    if (listing) return;
    listing = true;
    listHost.textContent = '조회 중…';
    ctx.query('can.status', {}, listAbort.signal).then(
      (res) => {
        if (disposed) return;
        const rows = readCanStatus(res);
        if (!rows.length) {
          listHost.textContent = '열린 CAN 채널 없음';
          return;
        }
        listHost.replaceChildren(
          ...rows.map((c) => {
            const b = tid(el('button', 'ct-open-item', `${c.channel}${c.bus && c.bus !== c.channel ? ` (bus ${c.bus})` : ''} · ${c.backend ?? 'backend ?'} · ${c.state ?? '상태 ?'}`), 'can-open-channel');
            b.type = 'button';
            b.dataset.channel = c.channel;
            b.title = `${c.channel} 트레이스 보기 (관측만 — 열기/송신 없음)`;
            b.addEventListener('click', () => observe(c.channel));
            return b;
          }),
        );
      },
      (e: unknown) => {
        if (disposed || listAbort.signal.aborted) return;
        listHost.textContent = `조회 실패: ${e instanceof Error ? e.message : String(e)}`;
      },
    ).finally(() => {
      listing = false;
    });
  });

  return {
    root,
    update(writable, hasRight) {
      const r = canOpenParams(draft);
      openBtn.disabled = pending || !writable || !r.ok;
      openBtn.title = !hasRight ? '연결은 operate 권한 필요' : !writable ? '연결 STALLED — 쓰기 잠김' : !r.ok ? '채널 이름을 입력하세요' : 'can.open (명시적 연결, operate 권한)';
      listBtn.disabled = listing;
    },
    dispose() {
      disposed = true;
      listAbort.abort();
    },
  };
}
