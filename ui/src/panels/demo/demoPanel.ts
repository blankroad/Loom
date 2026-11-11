// Demo panel — proves a panel can be built inside the I-UI1 lint rules using only
// the SDK: one declared topic (rows table), one declared action (marker).
// Open with `#/p/demo/echo?domain=uart` (any domain to watch).
// Real UART/CAN/UDS panels arrive in P1.

import { ORIGIN_BADGE, WRITE_STATE_CHIP, registerPanel, type LoomEvent, type PanelContext } from '../../sdk';

const VISIBLE_ROWS = 50;

function text(el: HTMLElement, s: string): HTMLElement {
  el.textContent = s; // device data is untrusted: text only, never innerHTML
  return el;
}

function rowFor(ev: LoomEvent): HTMLTableRowElement {
  const tr = document.createElement('tr');
  if (ev.request_id) tr.dataset.requestId = ev.request_id;
  tr.dataset.origin = ev.origin;
  const badge = ORIGIN_BADGE[ev.origin];
  const ws = (ev.payload as { write_state?: keyof typeof WRITE_STATE_CHIP }).write_state;
  const cells = [ev.seq, ev.kind, ev.channel ?? '', '', ws ? WRITE_STATE_CHIP[ws] : '', ev.risk ?? ''];
  cells.forEach((c, i) => {
    const td = document.createElement('td');
    if (i === 3) {
      const b = text(document.createElement('span'), badge.text);
      b.className = `org ${badge.cls}`;
      td.append(b);
    } else {
      text(td, c);
    }
    tr.append(td);
  });
  return tr;
}

function createDemo(ctx: PanelContext) {
  let root: HTMLElement | null = null;
  let tbody: HTMLTableSectionElement | null = null;
  let status: HTMLElement | null = null;
  const mirror = ctx.store('events');
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const offFrame = ctx.scheduler.onFrame(() => {
    if (!tbody || !status) return;
    const rows = mirror.tail(VISIBLE_ROWS);
    tbody.replaceChildren(...rows.map(rowFor));
    const g = mirror.gaps[mirror.gaps.length - 1];
    text(
      status,
      `${mirror.snapshotDone ? 'live' : 'snapshot…'} · head seq ${mirror.headSeq}` +
        (g ? ` · ⋯ ${g.count} 화면 생략 (${g.kind}, ${g.cause})` : ''),
    );
    return mirror.headSeq;
  });

  return {
    mount(el: HTMLElement) {
      root = document.createElement('div');
      const bar = document.createElement('div');
      status = text(document.createElement('span'), 'snapshot…');
      const btn = text(document.createElement('button'), '마커');
      (btn as HTMLButtonElement).type = 'button';
      btn.addEventListener('click', () => {
        ctx
          .act('mark', { label: `demo ${new Date().toISOString()}` })
          .then(() => ctx.ui.toast('마커를 남겼습니다'))
          .catch((e: unknown) => ctx.ui.toast(`마커 실패: ${e instanceof Error ? e.message : String(e)}`));
      });
      bar.append(status, ' ', btn);
      const table = document.createElement('table');
      table.className = 'demo-table';
      tbody = document.createElement('tbody');
      table.append(tbody);
      root.append(bar, table);
      el.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {},
    dispose() {
      offAppend();
      offFrame();
      root?.remove();
      root = tbody = status = null;
    },
  };
}

registerPanel(
  {
    id: 'demo.echo',
    domain: 'demo',
    title: 'Demo — 이벤트 행',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'events',
        filter: (p) => (typeof p.domain === 'string' ? { domains: [p.domain] } : {}),
        snapshot: 'required',
        mirrorRows: 500,
      },
    ],
    queries: [],
    actions: [{ id: 'mark', rpc: 'core.mark', class: 'request', right: 'operate', confirm: 'none', label: '마커' }],
    memory: { maxBytes: 4 * 1024 * 1024 },
    render: { maxHz: 20, budgetMs: 4 },
  },
  createDemo,
);
