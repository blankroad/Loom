// uart.terminal (M1) — uart-ui §1–4, plan §5.1.2.
// Header: port status · baud · gen · env · input-lease chip · actions.
// Body:   TerminalView (device bytes as-is through the 3-layer defense).
// Footer: last agent send + stage · agent expect wait · rx rate · blocked counts · REC.

import {
  CHANNEL_PROP,
  createLatchLine,
  bytesToHex,
  decodePayloadBytes,
  gapText,
  registerPanel,
  STAGE_CHIP,
  TerminalView,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type WriteState,
} from '../../sdk';
import {
  expectText,
  INITIAL_INPUT,
  inputChip,
  reduceInput,
  reducePort,
  restoreUartState,
  symbolize,
  type UartInputState,
  type UartPortState,
} from './uartState';

const PALETTE: { label: string; byte?: number; signal?: string; title: string }[] = [
  { label: 'Ctrl-W', byte: 0x17, title: '단어 지우기 (브라우저가 가로채는 키)' },
  { label: 'Ctrl-T', byte: 0x14, title: '0x14' },
  { label: 'Ctrl-N', byte: 0x0e, title: '0x0E' },
  { label: 'Ctrl-]', byte: 0x1d, title: '0x1D 리터럴' },
  { label: 'Esc', byte: 0x1b, title: '0x1B' },
  { label: 'Tab', byte: 0x09, title: '0x09' },
  { label: 'Ctrl-C', byte: 0x03, title: '0x03 (즉시)' },
  { label: 'Break', signal: 'break', title: 'UART break 신호 (uart.signal)' },
];

const RX_WINDOW_MS = 1_000;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

export function createUartTerminal(ctx: PanelContext): PanelInstance {
  const channel = typeof ctx.props.channel === 'string' ? ctx.props.channel : 'dut';
  const now = () => performance.now();
  const mirror = ctx.store('term');
  const hasWriteRight = () => ctx.rights.has('operate') || ctx.rights.has('approve');
  /** U-3: STALLED / disconnected → read-only like the shell's write lock */
  const canWrite = () => hasWriteRight() && !ctx.writeLocked;

  let input: UartInputState = INITIAL_INPUT;
  let port: UartPortState = { status: '?', baud: null, generation: null };
  let lastSeq = -1n;
  let lastOverwritten = 0;
  let gapsSeen = 0;
  let separatorDone = false;
  let snapshotVersion = 0;
  let paused = false;
  let lastSend: { origin: string; text: string; requestId: string | null; state: WriteState | null; atMs: number } | null = null;
  const rx: { atMs: number; n: number }[] = []; // bounded by RX_WINDOW_MS pruning
  let inputSeq = 0;
  let lastErrToast = 0;

  const send = (bytes: Uint8Array) => {
    ctx
      .act('input', { channel, data_hex: bytesToHex(bytes), input_seq: ++inputSeq })
      .catch((e: unknown) => {
        if (now() - lastErrToast > 5_000) {
          lastErrToast = now();
          ctx.ui.toast(`UART 입력 실패: ${e instanceof Error ? e.message : String(e)}`);
        }
      });
  };

  const term = new TerminalView({
    readonly: !canWrite(),
    onInput: send,
    onLargePaste: (text) => {
      // > 4 KiB paste goes through a confirm card (ActionGate 'card')
      ctx
        .act('inputLarge', { channel, data_hex: bytesToHex(new TextEncoder().encode(text)), input_seq: ++inputSeq })
        .catch((e: unknown) => ctx.ui.toast(`붙여넣기 취소: ${e instanceof Error ? e.message : String(e)}`));
    },
  });

  // ---- DOM ----
  const root = el('div', 'uart-panel');
  const head = el('div', 'uart-head');
  const portEl = el('span', 'uart-port');
  const chipEl = el('span', 'uart-chip');
  chipEl.setAttribute('data-testid', 'input-chip');
  const btn = (label: string, title: string, on: () => void) => {
    const b = el('button', 'uart-btn', label);
    b.type = 'button';
    b.title = title;
    b.addEventListener('click', on);
    return b;
  };
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`));
  const yieldBtn = btn('반환', '입력권을 에이전트에게 반환 (Ctrl-] y)', () => void act('yield', { channel }));
  const clearBtn = btn('지우기', '미완결 줄 지우기 (Ctrl-] c)', () => void act('clear', { channel }));
  const takeBtn = btn('가져오기', '입력권 가져오기 (Ctrl-] o)', () => void act('takeover', { channel }));
  const holdBtn = btn('HOLD', '에이전트 쓰기 보류 (Ctrl-] h)', () => void act(input.hold ? 'unhold' : 'hold', { channel }));
  const pauseBtn = btn('⏸', '화면 일시정지 (수신·기록은 계속)', () => {
    paused = !paused;
    term.setPaused(paused);
    ctx.scheduler.invalidate();
  });
  const palette = el('details', 'uart-palette');
  const sum = el('summary', undefined, '키 ▾');
  palette.append(sum);
  for (const k of PALETTE) {
    palette.append(
      btn(k.label, k.title, () => {
        if (k.byte !== undefined) term.sendBytes(new Uint8Array([k.byte]));
        else if (k.signal) void act('signal', { channel, kind: k.signal });
        term.focus();
      }),
    );
  }
  head.append(portEl, chipEl, yieldBtn, clearBtn, takeBtn, holdBtn, pauseBtn, palette);
  const body = el('div', 'uart-body');
  const foot = el('div', 'uart-foot');
  foot.setAttribute('data-testid', 'uart-status');
  root.append(createLatchLine(ctx), head, body, foot);

  const offAppend = mirror.onAppend(() => ctx.scheduler.invalidate());
  const tick = setInterval(() => ctx.scheduler.invalidate(), 1_000); // counters keep moving

  const consume = (): void => {
    const t = now();
    const snapshot = mirror.snapshot;
    if (snapshot && snapshot.version !== snapshotVersion) {
      snapshotVersion = snapshot.version;
      for (const item of snapshot.items) {
        if (item.domain !== 'uart' || item.kind !== 'uart.snapshot') continue;
        const state = item.state as Record<string, unknown> | null;
        if (!state || (item.channel ?? state.channel) !== channel) continue;
        ({ port, input } = restoreUartState(state, t));
      }
    }
    if (mirror.overwritten !== lastOverwritten) {
      term.markGap(`⋯ 화면 생략 ${mirror.overwritten - lastOverwritten}건 (패널 버퍼) — 기록은 데몬 저널`);
      lastOverwritten = mirror.overwritten;
    }
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (snapshot && lastSeq <= snapshot.throughSeq && ev.kind !== 'uart.data' && ev.kind !== 'uart.write') continue;
      handle(ev, t);
    }
    while (gapsSeen < mirror.gaps.length) {
      const g = mirror.gaps[gapsSeen++];
      if (g) term.markGap(gapText(g));
    }
    if (mirror.snapshotDone && !separatorDone) {
      separatorDone = true;
      term.markSeparator('── 백로그 / 라이브 ──');
    }
  };

  const handle = (ev: LoomEvent, t: number): void => {
    const p = ev.payload as Record<string, unknown>;
    if (ev.kind === 'uart.data') {
      const bytes = decodePayloadBytes(p.bytes ?? p);
      if (!bytes) return;
      if (ev.dir === 'tx') {
        if (ev.origin !== 'device') {
          // the tx bytes of the send already shown keep its stage (control and data paths
          // may deliver the write row and the tx bytes in either order)
          const same = lastSend !== null && ev.request_id && lastSend.requestId === ev.request_id;
          // an echo without request_id (e.g. raw user keys) right after a write row of the same origin
          // keeps that row's request_id instead of blanking the status line's data-request-id
          const keep = !ev.request_id && lastSend !== null && lastSend.requestId !== null && lastSend.origin === ev.origin && t - lastSend.atMs < 1000;
          lastSend = {
            origin: ev.origin,
            text: symbolize(bytes),
            requestId: ev.request_id ?? (keep ? lastSend!.requestId : null),
            state: same || keep ? (lastSend?.state ?? null) : null,
            atMs: same || keep ? (lastSend?.atMs ?? t) : t,
          };
        }
        return; // the device echo arrives as rx — never draw tx bytes ourselves
      }
      term.write(bytes);
      rx.push({ atMs: t, n: bytes.length });
      return;
    }
    if (ev.kind === 'uart.write') {
      const ws = p.write_state as WriteState | undefined;
      if (lastSend && ev.request_id && lastSend.requestId === ev.request_id) lastSend.state = ws ?? lastSend.state;
      else if (ev.origin === 'agent' || ev.origin === 'script') {
        const b = decodePayloadBytes(p.bytes);
        lastSend = {
          origin: ev.origin,
          text: b ? symbolize(b) : typeof p.text === 'string' ? symbolize(p.text) : '…',
          requestId: ev.request_id ?? null,
          state: ws ?? null,
          atMs: t,
        };
      }
      return;
    }
    port = reducePort(port, ev);
    input = reduceInput(input, ev, t);
  };

  const draw = (): bigint => {
    consume();
    const t = now();
    const writable = canWrite();
    if (term.readonly === writable) term.setReadonly(!writable);
    portEl.textContent = `UART ${channel} ● ${port.status}${port.baud ? ` ${port.baud}` : ''}${
      port.generation !== null ? ` gen${port.generation}` : ''
    } [${(ctx.env ?? '?').toUpperCase()}]`;
    const chip = inputChip(input, ctx.connId, paused, t);
    chipEl.textContent = chip.text;
    chipEl.dataset.kind = chip.kind;
    yieldBtn.disabled = !writable || !chip.canYield;
    clearBtn.disabled = !writable || !chip.canClear;
    takeBtn.disabled = !writable || !chip.canTakeover;
    holdBtn.disabled = !writable;
    holdBtn.textContent = input.hold ? 'HOLD 해제' : 'HOLD';
    holdBtn.setAttribute('aria-pressed', String(input.hold));
    pauseBtn.setAttribute('aria-pressed', String(paused));

    while (rx.length && t - (rx[0] as { atMs: number }).atMs > RX_WINDOW_MS) rx.shift();
    const rate = rx.reduce((a, x) => a + x.n, 0);
    const st = term.stats;
    const parts: string[] = [];
    if (lastSend) {
      const tag = lastSend.origin === 'agent' ? 'AGT' : lastSend.origin === 'script' ? 'SCR' : 'USR';
      const stage = lastSend.state ? STAGE_CHIP[lastSend.state] : '';
      parts.push(`${tag} ▶ "${lastSend.text}" ${stage} ${Math.round((t - lastSend.atMs) / 1000)}s 전`);
    }
    const ex = expectText(input, t);
    if (ex) parts.push(ex);
    parts.push(`rx ${rate < 1024 ? `${rate} B/s` : `${(rate / 1024).toFixed(1)} KB/s`}`);
    parts.push(`차단 질의×${st.blockedQueries} 자동응답×${st.blockedAutoInput}`);
    if (st.droppedBytes) parts.push(`화면 생략 ${st.droppedBytes} B`);
    parts.push(`REC ${ctx.recordHealth}`);
    if (!hasWriteRight()) parts.push('보기 전용(watch)');
    else if (!writable) parts.push('쓰기 잠김(STALLED)');
    foot.textContent = parts.join(' │ ');
    // U-2: the status line names the last send (UI-T02 compares it with the RPC request_id)
    if (lastSend?.requestId) foot.dataset.requestId = lastSend.requestId;
    else delete foot.dataset.requestId;
    if (lastSend) {
      foot.dataset.origin = lastSend.origin;
      if (lastSend.state) foot.dataset.state = lastSend.state;
      else delete foot.dataset.state;
    } else {
      delete foot.dataset.origin;
      delete foot.dataset.state;
    }
    return mirror.headSeq;
  };
  const offFrame = ctx.scheduler.onFrame(draw);

  return {
    mount(host) {
      host.append(root);
      term.open(body);
      ctx.scheduler.invalidate();
    },
    resize(w, h) {
      // no fit addon installed (P1): estimate from a 13 px monospace cell
      term.fit(Math.floor((w - 8) / 7.8), Math.floor((h - 56) / 17));
    },
    dispose() {
      clearInterval(tick);
      offAppend();
      offFrame();
      term.dispose();
      root.remove();
    },
  };
}

const uartActions = (
  [
    ['input', 'uart.input', 'stream', 'none'],
    ['inputLarge', 'uart.input', 'request', 'card'],
    ['yield', 'uart.input.yield', 'request', 'none'],
    ['clear', 'uart.input.clear', 'request', 'none'],
    ['takeover', 'uart.input.takeover', 'request', 'none'],
    ['hold', 'uart.hold', 'request', 'none'],
    ['unhold', 'uart.unhold', 'request', 'none'],
    ['signal', 'uart.signal', 'request', 'none'],
  ] as const
).map(([id, rpc, cls, confirm]) => ({ id, rpc, class: cls, right: 'operate' as const, confirm }));

registerPanel(
  {
    id: 'uart.terminal',
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    domain: 'uart',
    title: 'UART 터미널',
    version: 1,
    milestone: 'M1',
    topics: [
      {
        key: 'term',
        filter: (p) => ({ domains: ['uart'], channels: [typeof p.channel === 'string' ? p.channel : 'dut'] }),
        snapshot: 'required',
        maxBatchMs: 10,
        mirrorRows: 4_000,
      },
    ],
    queries: [],
    actions: uartActions,
    memory: { maxBytes: 8 * 1024 * 1024 }, // 2 MiB draw queue + scrollback + mirror
    render: { maxHz: 60, budgetMs: 4, lagTargetMs: 100 },
    shellSlots: ['channel_bar'],
  },
  createUartTerminal,
);
