// uart.terminal (M1) — uart-ui §1–4, plan §5.1.2; CoolTerm-fidelity layout
// (docs/impl/main/research/fidelity/uart.md).
// Toolbar: View [터미널|HEX] · Send String · 화면 지우기 · 일시정지 ⟷ input-lease chip + lease actions · 키 ▾.
// Body:   TerminalView (device bytes as-is through the 3-layer defense) OR the byte (HEX) view of
//         the original uart.data bytes. Keys are sent only from the terminal view.
// Drawer: Send String (Text/HEX + explicit line ending) → existing uart.input / inputLarge path.
// Status bar line 1: UART alias ● status baud gen [ENV] · path / 8-N-1 · flow · open time · TX/RX observed · activity.
// Status bar line 2: last agent send + stage · agent expect wait · rx rate · blocked counts · REC.

import {
  CHANNEL_PROP,
  createLatchLine,
  bytesToHex,
  decodePayloadBytes,
  fmtWall,
  gapText,
  originCell,
  registerPanel,
  STAGE_CHIP,
  TerminalView,
  VirtualTable,
  type LoomEvent,
  type PanelContext,
  type PanelInstance,
  type WriteState,
} from '../../sdk';
import './uart.css';
import { ByteLog, rowAscii, rowHex, rowsToHex, type ByteRow } from './byteLog';
import { createSendString, routeFor } from './sendString';
import {
  expectText,
  frameText,
  INITIAL_INPUT,
  inputChip,
  isOpenStatus,
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
const LED_MS = 180;

const hms = (ms: number): string => {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (x: number) => String(x).padStart(2, '0');
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
};

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
  /** U-3: STALLED / disconnected → read-only like the shell's write lock. A port that is not OPEN (closed,
   * disconnected, unknown — e.g. a retained history view after STOP/disconnect) is read-only too. */
  const canWrite = () => hasWriteRight() && !ctx.writeLocked && isOpenStatus(port.status);

  let input: UartInputState = INITIAL_INPUT;
  let port: UartPortState = { status: '?', baud: null, generation: null };
  let lastSeq = -1n;
  /** seq of the newest port state applied (live control, backlog or snapshot high-water): older never overwrite */
  let portSeq = -1n;
  let lastOverwritten = 0;
  let gapsSeen = 0;
  let separatorDone = false;
  let snapshotVersion = 0;
  let paused = false;
  let lastSend: { origin: string; text: string; requestId: string | null; state: WriteState | null; atMs: number } | null = null;
  const rx: { atMs: number; n: number }[] = []; // bounded by RX_WINDOW_MS pruning
  let inputSeq = 0;
  let lastErrToast = 0;
  let view: 'terminal' | 'hex' = 'terminal';
  let size: { w: number; h: number } | null = null;
  // observed in this panel's local window only (not device/daemon totals)
  let txObserved = 0;
  let rxObserved = 0;
  let countsPartial = false;
  let txLedAt = Number.NEGATIVE_INFINITY;
  let rxLedAt = Number.NEGATIVE_INFINITY;
  let ledTimer: ReturnType<typeof setTimeout> | null = null;
  let openSinceWallMs: number | null = null;
  const bytes = new ByteLog();
  const wallOf = (ev: LoomEvent): number => {
    try {
      return Number(BigInt(ev.ts.wall_ns) / 1_000_000n);
    } catch {
      return 0;
    }
  };

  const send = (bytes: Uint8Array) => {
    if (!canWrite()) return; // never emit input for a port that is not open (panel-level, before the gate)
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
    onLargePaste: (text) => sendLarge(new TextEncoder().encode(text), '붙여넣기 취소'),
  });
  /** > 4 KiB goes through a confirm card (ActionGate 'card') — paste and Send String alike */
  function sendLarge(b: Uint8Array, failText: string): void {
    ctx
      .act('inputLarge', { channel, data_hex: bytesToHex(b), input_seq: ++inputSeq })
      .catch((e: unknown) => ctx.ui.toast(`${failText}: ${e instanceof Error ? e.message : String(e)}`));
  }

  // ---- DOM ----
  const root = el('div', 'uart-panel');
  const head = el('div', 'uart-head uart-tools');
  head.setAttribute('role', 'toolbar');
  head.setAttribute('aria-label', `UART ${channel} 도구 막대`);
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
  /** CoolTerm-style tool button: icon above a short label (icon only on narrow panels) */
  const tool = (icon: string, label: string, title: string, testid: string, on: () => void) => {
    const b = el('button', 'uart-btn uart-tool');
    b.type = 'button';
    b.title = title;
    b.setAttribute('aria-label', label);
    b.setAttribute('data-testid', testid);
    b.append(el('span', 'uart-tool-ico', icon), el('span', 'uart-tool-lbl', label));
    b.addEventListener('click', on);
    return b;
  };
  const act = (id: string, params: object) =>
    ctx.act(id, params).catch((e: unknown) => ctx.ui.toast(`${id} 실패: ${e instanceof Error ? e.message : String(e)}`));
  const yieldBtn = btn('반환', '입력권을 에이전트에게 반환 (Ctrl-] y)', () => void act('yield', { channel }));
  // server-side dirty input line (uart.input.clear) — NOT the local screen clear below
  const clearBtn = btn('미완결 줄 지우기', '장치로 보낸 미완결 입력 줄을 서버에서 지움 (uart.input.clear, Ctrl-] c) — 화면 지우기 아님', () => void act('clear', { channel }));
  clearBtn.setAttribute('data-testid', 'uart-input-clear');
  const takeBtn = btn('가져오기', '입력권 가져오기 (Ctrl-] o)', () => void act('takeover', { channel }));
  const holdBtn = btn('HOLD', '에이전트 쓰기 보류 (Ctrl-] h)', () => void act(input.hold ? 'unhold' : 'hold', { channel }));

  // View: terminal (xterm VT) | HEX (original bytes) — CoolTerm View Plain/Hex
  const viewGroup = el('span', 'uart-viewsw');
  viewGroup.setAttribute('role', 'group');
  viewGroup.setAttribute('aria-label', '보기');
  const termViewBtn = tool('▤', '터미널', '터미널 보기 — xterm VT, 키 입력이 장치로 즉시 전송됨', 'uart-view-terminal', () => setView('terminal'));
  const hexViewBtn = tool('0x', 'HEX', 'HEX 보기 — 수신/송신 원본 바이트 (키 입력 안 보냄)', 'uart-view-hex', () => setView('hex'));
  viewGroup.append(termViewBtn, hexViewBtn);
  const sendStrBtn = tool('⇪', 'Send String', 'Text/HEX 문자열과 줄 끝을 지정해 전송 (입력권 경로 uart.input)', 'uart-send-string-toggle', () =>
    setDrawer(!drawer.isOpen),
  );
  const clearViewBtn = tool('✕', '화면 지우기', '이 패널의 화면과 HEX 보기만 비움 — 장치·데몬 저널·서버 입력 줄에는 영향 없음', 'uart-clear-view', () => {
    try {
      term.term.clear();
    } catch {
      /* not opened yet */
    }
    bytes.clear();
    ctx.scheduler.invalidate();
  });
  const pauseBtn = tool('⏸', '일시정지', '화면 일시정지 (수신·기록은 계속)', 'uart-pause', () => {
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
        if (view === 'terminal') term.focus();
      }),
    );
  }
  const leaseGroup = el('span', 'uart-lease');
  leaseGroup.setAttribute('role', 'group');
  leaseGroup.setAttribute('aria-label', '입력권');
  leaseGroup.append(chipEl, takeBtn, yieldBtn, clearBtn, holdBtn, palette);
  head.append(viewGroup, sendStrBtn, clearViewBtn, pauseBtn, el('span', 'uart-flex'), leaseGroup);

  const body = el('div', 'uart-body');
  const bytesHost = el('div', 'uart-bytes');
  bytesHost.hidden = true;
  bytesHost.setAttribute('data-testid', 'uart-bytes');
  const bytesInfo = el('div', 'uart-bytes-info');
  const bytesCopy = btn('선택 행 HEX 복사', '선택한 행의 원본 바이트를 "48 65 6C" 형식으로 복사', () => {
    const key = table.selected;
    const row = key ? bytes.rows.find((r) => r.key === key) : undefined;
    if (!row) return;
    const text = rowsToHex([row]);
    void navigator.clipboard?.writeText(text).then(
      () => ctx.ui.toast(`HEX 복사: ${text}`),
      () => ctx.ui.toast('클립보드 쓰기 실패'),
    );
  });
  bytesCopy.disabled = true;
  const bytesMeta = el('span', 'uart-bytes-meta');
  bytesInfo.append(bytesMeta, bytesCopy);
  const tableHost = el('div', 'uart-bytes-table');
  bytesHost.append(bytesInfo, tableHost);
  const table = new VirtualTable<ByteRow>(tableHost, {
    columns: [
      { key: 't', header: '시각', width: '8.5em', cell: (r) => (r.wallMs ? fmtWall(r.wallMs) : '') },
      {
        key: 'd',
        header: '방향',
        width: '5.5em',
        cell: (r) => (r.kind === 'gap' ? '' : { ...originCell(r.origin), text: `${r.dir === 'tx' ? 'TX' : 'RX'} ${originCell(r.origin).text}` }),
      },
      { key: 'h', header: 'HEX', width: '50ch', cell: (r) => (r.kind === 'gap' ? { text: r.text ?? '', cls: 'gap-text' } : { text: rowHex(r), cls: 'mono uart-hex' }) },
      { key: 'a', header: 'ASCII', width: 'minmax(17ch, 1fr)', cell: (r) => (r.kind === 'gap' ? '' : { text: rowAscii(r), cls: 'mono uart-ascii' }) },
    ],
    keyOf: (r) => r.key,
    rowAttrs: (r) => ({ 'data-dir': r.dir ?? 'gap', 'data-origin': r.origin }),
    rowClass: (r) => (Number(r.key) % 2 === 0 ? 'uart-zebra' : ''),
    onNeedRender: () => ctx.scheduler.invalidate(),
    onSelect: (key) => {
      bytesCopy.disabled = key === null;
    },
    emptyText: '바이트 없음',
    ariaLabel: `UART ${channel} HEX 보기`,
  });

  const drawer = createSendString({
    channel,
    gate: () => {
      if (!hasWriteRight()) return { ok: false, reason: '보기 전용(watch)' };
      if (ctx.writeLocked) return { ok: false, reason: '쓰기 잠김(STALLED)' };
      if (!isOpenStatus(port.status)) return { ok: false, reason: `포트 ${port.status}` };
      if (paused) return { ok: false, reason: '화면 일시정지 중' };
      return { ok: true };
    },
    submit: (b) => {
      if (routeFor(b.length) === 'inputLarge') sendLarge(b, 'Send String 취소');
      else send(b);
    },
    onClose: () => setDrawer(false),
  });

  const status = el('div', 'uart-statusbar');
  const line1 = el('div', 'uart-portline');
  const cfgEl = el('span', 'uart-cfg');
  const connEl = el('span', 'uart-conn');
  const countEl = el('span', 'uart-counts');
  countEl.setAttribute('data-testid', 'uart-counts');
  const leds = el('span', 'uart-leds');
  leds.title = '이 패널이 받은 uart.data 이벤트 기준 활동 표시 — 모뎀 신호선 상태 아님';
  const txLed = el('span', 'uart-led');
  const rxLed = el('span', 'uart-led');
  leds.append(txLed, el('span', 'uart-led-lbl', 'TX'), rxLed, el('span', 'uart-led-lbl', 'RX'));
  line1.append(portEl, cfgEl, connEl, countEl, el('span', 'uart-flex'), leds);
  const foot = el('div', 'uart-foot');
  foot.setAttribute('data-testid', 'uart-status');
  status.append(line1, foot);
  root.append(createLatchLine(ctx), head, body, bytesHost, drawer.root, status);

  function setView(v: 'terminal' | 'hex'): void {
    if (view === v) return;
    view = v;
    const toHex = v === 'hex';
    if (toHex && document.activeElement instanceof HTMLElement && body.contains(document.activeElement)) document.activeElement.blur();
    body.hidden = toHex;
    bytesHost.hidden = !toHex;
    if (!toHex) {
      fitBody();
      term.focus();
    } else table.followTail();
    ctx.scheduler.invalidate();
  }
  function setDrawer(open: boolean): void {
    drawer.setOpen(open);
    fitBody();
    sendStrBtn.setAttribute('aria-expanded', String(open));
    if (!open && view === 'terminal') term.focus();
    ctx.scheduler.invalidate();
  }
  // The toolbar, composer and footer can wrap independently of the dock dimensions.
  // Fit the actual remaining body so xterm's canvas cannot cover sibling controls.
  const fitBody = () => {
    if (view !== 'terminal') return;
    const w = body.clientWidth || size?.w || 0;
    const h = body.clientHeight;
    if (w > 0 && h > 0) term.fit(Math.floor((w - 8) / 7.8), Math.floor(h / 17));
  };
  const bodyObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(fitBody);
  const pulse = () => {
    if (ledTimer !== null) return;
    ledTimer = setTimeout(() => {
      ledTimer = null;
      ctx.scheduler.invalidate();
    }, LED_MS + 20);
  };

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
        const restored = restoreUartState(state, t);
        input = restored.input;
        // the snapshot is as of throughSeq: a newer live port state (e.g. disconnected) already applied wins
        if (snapshot.throughSeq >= portSeq) {
          port = restored.port;
          portSeq = snapshot.throughSeq;
        }
      }
    }
    if (mirror.overwritten !== lastOverwritten) {
      const text = `⋯ 화면 생략 ${mirror.overwritten - lastOverwritten}건 (패널 버퍼) — 기록은 데몬 저널`;
      term.markGap(text);
      bytes.gap(text);
      countsPartial = true;
      lastOverwritten = mirror.overwritten;
    }
    for (const ev of mirror.after(lastSeq)) {
      lastSeq = BigInt(ev.seq);
      if (snapshot && lastSeq <= snapshot.throughSeq && ev.kind !== 'uart.data' && ev.kind !== 'uart.write') continue;
      handle(ev, t);
    }
    while (gapsSeen < mirror.gaps.length) {
      const g = mirror.gaps[gapsSeen++];
      if (g) {
        term.markGap(gapText(g));
        bytes.gap(gapText(g));
        countsPartial = true;
      }
    }
    if (mirror.snapshotDone && !separatorDone) {
      separatorDone = true;
      term.markSeparator('── 백로그 / 라이브 ──');
    }
  };

  const handle = (ev: LoomEvent, t: number): void => {
    const p = ev.payload as Record<string, unknown>;
    if (ev.kind === 'uart.data') {
      const data = decodePayloadBytes(p.bytes ?? p);
      if (!data) return;
      // byte view + observed counters use the ORIGINAL payload bytes (never the symbolized text)
      const dir = ev.dir === 'tx' ? 'tx' : 'rx';
      bytes.append(dir, ev.origin, wallOf(ev), data);
      if (dir === 'tx') {
        txObserved += data.length;
        txLedAt = t;
      } else {
        rxObserved += data.length;
        rxLedAt = t;
      }
      pulse();
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
            text: symbolize(data),
            requestId: ev.request_id ?? (keep ? lastSend!.requestId : null),
            state: same || keep ? (lastSend?.state ?? null) : null,
            atMs: same || keep ? (lastSend?.atMs ?? t) : t,
          };
        }
        return; // the device echo arrives as rx — never draw tx bytes ourselves
      }
      term.write(data);
      rx.push({ atMs: t, n: data.length });
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
    applyPort(ev);
    input = reduceInput(input, ev, t);
  };
  /** uart.state / uart.config with a per-port seq guard (control path and data/backlog path share it) */
  function applyPort(ev: LoomEvent): boolean {
    if (ev.kind !== 'uart.state' && ev.kind !== 'uart.config') return false;
    let seq: bigint;
    try {
      seq = BigInt(ev.seq);
    } catch {
      return false;
    }
    if (seq <= portSeq) return false;
    portSeq = seq;
    const before = port.status;
    port = reducePort(port, ev);
    if (ev.kind === 'uart.state' && port.status !== before) openSinceWallMs = isOpenStatus(port.status) ? wallOf(ev) || null : null;
    return true;
  }
  // Live port state is a priority kind: it reaches data subscriptions only via snapshot/backlog, so apply it from
  // the control path too (same seq guard; an older backlog row or snapshot never overrides it).
  const offControl = ctx.onControl((ev) => {
    if ((ev.channel ?? (ev.payload as { channel?: unknown } | null)?.channel) !== channel) return;
    if (applyPort(ev)) ctx.scheduler.invalidate();
  });

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
    termViewBtn.setAttribute('aria-pressed', String(view === 'terminal'));
    hexViewBtn.setAttribute('aria-pressed', String(view === 'hex'));
    sendStrBtn.setAttribute('aria-expanded', String(drawer.isOpen));
    drawer.update();

    // status bar line 1 (CoolTerm: "port / 115200 8-N-1", "Connected hh:mm:ss, n / n bytes")
    const frame = frameText(port);
    const cfg = [port.path, frame].filter(Boolean).join(' / ');
    cfgEl.textContent = [cfg, port.flow ? `흐름 ${port.flow}` : ''].filter(Boolean).join(' · ');
    cfgEl.hidden = cfgEl.textContent === '';
    if (openSinceWallMs !== null && isOpenStatus(port.status)) {
      connEl.textContent = `연결 ${hms(Date.now() - openSinceWallMs)}`;
      connEl.hidden = false;
    } else connEl.hidden = true;
    countEl.textContent = `관측 TX ${txObserved.toLocaleString('ko-KR')} B / RX ${rxObserved.toLocaleString('ko-KR')} B${countsPartial ? ' (누락 구간 있음)' : ''}`;
    countEl.title = '이 패널이 받은 이벤트(백로그 포함) 기준 — 장치·데몬 누계 아님';
    txLed.dataset.on = String(t - txLedAt < LED_MS);
    rxLed.dataset.on = String(t - rxLedAt < LED_MS);

    if (view === 'hex' && !paused) {
      table.setItems(bytes.rows);
      table.render();
    }
    bytesMeta.textContent = `원본 바이트 ${bytes.bytes.toLocaleString('ko-KR')} B · ${bytes.rows.length.toLocaleString('ko-KR')}행${
      bytes.evictedRows ? ` · 보기 범위 밖 ${bytes.evictedBytes.toLocaleString('ko-KR')} B/${bytes.evictedRows.toLocaleString('ko-KR')}행` : ''
    } · 키 입력은 터미널 보기에서만 전송${paused ? ' · 일시정지' : ''}`;

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
      bodyObserver?.observe(body);
      ctx.scheduler.invalidate();
    },
    resize(w, h) {
      size = { w, h };
      // no fit addon installed (P1): estimate from a 13 px monospace cell
      fitBody();
      ctx.scheduler.invalidate();
    },
    dispose() {
      clearInterval(tick);
      bodyObserver?.disconnect();
      if (ledTimer !== null) clearTimeout(ledTimer);
      table.dispose();
      offAppend();
      offControl();
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
