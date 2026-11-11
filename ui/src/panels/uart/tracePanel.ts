// uart.trace (M1 minimal) — uart-ui §1, plan §5.1.2: rows = RX lines / TX events.
// Columns: time · origin · content (invisible characters symbolized).
// C2: ring of MAX_LINES (20,000). Eviction is chunked (EVICT_CHUNK rows at once)
// so a fast flood does not shift the array per line; evicted rows are counted and
// shown as "이전 N행 보관 범위 밖". A partial RX line is capped at MAX_PARTIAL
// characters (then emitted with "…").

import {
  CHANNEL_PROP,
  createLatchLine,
  decodePayloadBytes,
  fmtWall,
  gapText,
  originCell,
  registerPanel,
  VirtualTable,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
} from '../../sdk';
import { symbolize } from './uartState';

export const MAX_LINES = 20_000;
const EVICT_CHUNK = 1_000;
const MAX_PARTIAL = 64 * 1024;

export interface TraceLine {
  key: string;
  wallMs: number;
  origin: string;
  dir: 'rx' | 'tx' | 'gap';
  text: string;
}

/** Splits RX bytes into lines across chunk boundaries (streaming UTF-8). */
export class LineAssembler {
  private dec = new TextDecoder('utf-8', { fatal: false });
  private partial = '';

  push(bytes: Uint8Array): string[] {
    const s = this.partial + this.dec.decode(bytes, { stream: true });
    const parts = s.split('\n');
    this.partial = parts.pop() ?? '';
    const out = parts.map((l) => l.replace(/\r$/, ''));
    if (this.partial.length > MAX_PARTIAL) {
      out.push(`${this.partial.slice(0, MAX_PARTIAL)}…`);
      this.partial = '';
    }
    return out;
  }
  /** after a gap: drop the half line (it would splice unrelated bytes) */
  reset(): void {
    this.dec = new TextDecoder('utf-8', { fatal: false });
    this.partial = '';
  }
}

export function createUartTrace(ctx: PanelContext): PanelInstance {
  const mirror = ctx.store('trace');
  const lines: TraceLine[] = [];
  let evicted = 0;
  let lastSeq = -1n;
  let gapsSeen = 0;
  let n = 0;
  const asm = new LineAssembler();

  const push = (l: Omit<TraceLine, 'key'>) => {
    lines.push({ ...l, key: String(++n) });
    if (lines.length > MAX_LINES + EVICT_CHUNK) {
      lines.splice(0, EVICT_CHUNK);
      evicted += EVICT_CHUNK;
    }
  };

  const root = document.createElement('div');
  root.className = 'uart-trace';
  const info = document.createElement('div');
  info.className = 'muted';
  const tableHost = document.createElement('div');
  tableHost.className = 'trace-host';
  root.append(createLatchLine(ctx), info, tableHost);

  const table = new VirtualTable<TraceLine>(tableHost, {
    columns: [
      { key: 't', header: '시각', width: '8.5em', cell: (l) => fmtWall(l.wallMs) },
      {
        key: 'o',
        header: '출처',
        width: '4em',
        cell: (l) => (l.dir === 'gap' ? '' : l.dir === 'tx' ? { ...originCell(l.origin), text: `${originCell(l.origin).text}▶` } : originCell(l.origin)),
      },
      { key: 'x', header: '내용', width: '1fr', cell: (l) => (l.dir === 'gap' ? { text: l.text, cls: 'gap-text' } : { text: l.text, cls: 'mono' }) },
    ],
    keyOf: (l) => l.key,
    rowAttrs: (l) => ({ 'data-origin': l.origin, 'data-dir': l.dir }),
    onNeedRender: () => ctx.scheduler.invalidate(),
    emptyText: '수신 없음',
    ariaLabel: 'UART 트레이스',
  });

  const wall = (ev: LoomEvent) => Number(BigInt(ev.ts.wall_ns) / 1_000_000n);

  const consume = () => {
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (ev.kind !== 'uart.data') continue;
      const p = ev.payload as Record<string, unknown>;
      const bytes = decodePayloadBytes(p.bytes ?? p);
      if (!bytes) continue;
      if (ev.dir === 'tx') push({ wallMs: wall(ev), origin: ev.origin, dir: 'tx', text: symbolize(bytes) });
      else for (const l of asm.push(bytes)) push({ wallMs: wall(ev), origin: 'device', dir: 'rx', text: symbolize(l) });
    }
    while (gapsSeen < mirror.gaps.length) {
      const g = mirror.gaps[gapsSeen++];
      if (!g) continue;
      asm.reset();
      push({ wallMs: 0, origin: 'device', dir: 'gap', text: gapText(g) });
    }
  };

  const off = ctx.scheduler.onFrame(() => {
    consume();
    table.setItems(lines);
    table.render();
    info.textContent = `${lines.length.toLocaleString('ko-KR')}행${evicted ? ` · 이전 ${evicted.toLocaleString('ko-KR')}행 보관 범위 밖` : ''}`;
    return mirror.headSeq;
  });
  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());

  return {
    mount(host) {
      host.append(root);
      ctx.scheduler.invalidate();
    },
    resize() {
      ctx.scheduler.invalidate();
    },
    dispose() {
      off();
      offAppend();
      table.dispose();
      root.remove();
    },
  };
}

registerPanel(
  {
    id: 'uart.trace',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'uart',
    title: 'UART 트레이스',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'trace',
        // identical (filter, options) to uart.terminal → net shares ONE server subscription
        // (contract v1: ≤ 8 subscriptions per viewer connection)
        filter: (p) => ({ domains: ['uart'], channels: [typeof p.channel === 'string' ? p.channel : 'dut'] }),
        snapshot: 'required',
        maxBatchMs: 10,
        mirrorRows: 4_000,
      },
    ],
    queries: [],
    actions: [],
    memory: { maxBytes: 8 * 1024 * 1024 },
    render: { maxHz: 30, budgetMs: 4, lagTargetMs: 150 },
  },
  createUartTrace,
);
