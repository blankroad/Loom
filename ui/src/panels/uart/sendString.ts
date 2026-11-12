// UART "Send String" drawer (CoolTerm Connection > Send String… analogue).
// Text or HEX entry + an EXPLICIT line ending, with the exact final bytes previewed before
// sending. It never sends by itself: the owner panel routes the bytes through the existing
// gated uart.input path (≤ LARGE_PASTE_BYTES) or the inputLarge confirm card (larger).
// The xterm terminal keeps sending keystrokes directly; this drawer is a separate, visible path.

import { LARGE_PASTE_BYTES } from '../../sdk';

export type SendMode = 'text' | 'hex';
export type LineEnding = 'none' | 'cr' | 'lf' | 'crlf';

export const LINE_ENDINGS: { id: LineEnding; label: string; bytes: number[] }[] = [
  { id: 'none', label: '없음', bytes: [] },
  { id: 'cr', label: 'CR (0D)', bytes: [0x0d] },
  { id: 'lf', label: 'LF (0A)', bytes: [0x0a] },
  { id: 'crlf', label: 'CR+LF (0D 0A)', bytes: [0x0d, 0x0a] },
];
const endingBytes = (le: LineEnding): number[] => LINE_ENDINGS.find((x) => x.id === le)?.bytes ?? [];

/** default ending per mode: text behaves like Enter in the terminal (CR); hex sends exactly what is typed */
export const DEFAULT_ENDING: Record<SendMode, LineEnding> = { text: 'cr', hex: 'none' };

export type Encoded = { ok: true; bytes: Uint8Array } | { ok: false; error: string };

/**
 * HEX entry: two hex digits per byte; whitespace, ',', ':' and '-' separate freely;
 * an optional 0x prefix per token. "54 68", "5468", "54-68", "0x54,0x68" → 54 68.
 */
export function parseHex(src: string): Encoded {
  let digits = '';
  for (const tok of src.split(/[\s,:-]+/)) {
    if (!tok) continue;
    const t = /^0x/i.test(tok) ? tok.slice(2) : tok;
    const bad = t.search(/[^0-9a-fA-F]/);
    if (bad >= 0) return { ok: false, error: `HEX가 아닌 문자 "${t[bad]}"` };
    digits += t;
  }
  if (digits.length % 2 !== 0) return { ok: false, error: `HEX 자릿수가 홀수 (${digits.length}자리) — 바이트당 2자리` };
  const out = new Uint8Array(digits.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(digits.slice(i * 2, i * 2 + 2), 16);
  return { ok: true, bytes: out };
}

/**
 * Final bytes for one Send. Text: UTF-8; every line break in the box AND the end use the
 * selected ending (CoolTerm Enter emulation; "없음" removes breaks). HEX: parsed bytes + ending.
 */
export function encodeSendString(mode: SendMode, src: string, le: LineEnding): Encoded {
  const end = endingBytes(le);
  let body: Uint8Array;
  if (mode === 'hex') {
    const r = parseHex(src);
    if (!r.ok) return r;
    body = r.bytes;
  } else {
    const enc = new TextEncoder();
    const parts = src.split(/\r\n|\r|\n/).map((l) => enc.encode(l));
    const total = parts.reduce((a, p) => a + p.length, 0) + end.length * parts.length;
    const out = new Uint8Array(total - end.length); // the trailing ending is added below
    let o = 0;
    parts.forEach((p, i) => {
      out.set(p, o);
      o += p.length;
      if (i < parts.length - 1) {
        out.set(end, o);
        o += end.length;
      }
    });
    body = out;
  }
  if (body.length === 0) return { ok: false, error: '보낼 데이터 없음' };
  const all = new Uint8Array(body.length + end.length);
  all.set(body);
  all.set(end, body.length);
  return { ok: true, bytes: all };
}

/** which existing action carries the bytes: plain gated input, or the inputLarge confirm card */
export const routeFor = (n: number): 'input' | 'inputLarge' => (n > LARGE_PASTE_BYTES ? 'inputLarge' : 'input');

export function previewText(bytes: Uint8Array, max = 24): string {
  const shown = [...bytes.subarray(0, max)].map((b) => b.toString(16).toUpperCase().padStart(2, '0')).join(' ');
  return `${bytes.length.toLocaleString('ko-KR')} B · ${shown}${bytes.length > max ? ` … +${bytes.length - max} B` : ''}`;
}

export interface SendGate {
  ok: boolean;
  reason?: string;
}

export interface SendStringOptions {
  channel: string;
  /** current gate (rights, write lock, port open, pause) — evaluated on every update */
  gate(): SendGate;
  submit(bytes: Uint8Array): void;
  onClose(): void;
}

export interface SendStringDrawer {
  readonly root: HTMLElement;
  readonly isOpen: boolean;
  setOpen(open: boolean): void;
  update(): void;
}

let uid = 0;

export function createSendString(o: SendStringOptions): SendStringDrawer {
  const id = `uart-ss-${++uid}`;
  let mode: SendMode = 'text';
  const ending: Record<SendMode, LineEnding> = { ...DEFAULT_ENDING };
  let open = false;

  const root = document.createElement('section');
  root.className = 'uart-ss';
  root.hidden = true;
  root.setAttribute('data-testid', 'uart-send-string');
  root.setAttribute('aria-label', `Send String (${o.channel})`);

  const bar = document.createElement('div');
  bar.className = 'uart-ss-bar';
  const title = document.createElement('strong');
  title.className = 'uart-ss-title';
  title.textContent = `Send String (${o.channel})`;

  const modes = document.createElement('span');
  modes.className = 'uart-ss-modes';
  modes.setAttribute('role', 'radiogroup');
  modes.setAttribute('aria-label', '입력 형식');
  const radio = (m: SendMode, label: string) => {
    const l = document.createElement('label');
    const r = document.createElement('input');
    r.type = 'radio';
    r.name = `${id}-mode`;
    r.value = m;
    r.checked = m === mode;
    r.setAttribute('data-testid', `uart-ss-mode-${m}`);
    r.addEventListener('change', () => {
      if (!r.checked) return;
      mode = m;
      endSel.value = ending[mode];
      area.placeholder = PLACEHOLDER[mode];
      update();
    });
    l.append(r, ` ${label}`);
    return l;
  };
  modes.append(radio('text', 'Text'), radio('hex', 'HEX'));

  const endLabel = document.createElement('label');
  endLabel.className = 'uart-ss-end';
  endLabel.append('줄 끝 ');
  const endSel = document.createElement('select');
  endSel.setAttribute('data-testid', 'uart-ss-ending');
  for (const le of LINE_ENDINGS) {
    const opt = document.createElement('option');
    opt.value = le.id;
    opt.textContent = le.label;
    endSel.append(opt);
  }
  endSel.value = ending[mode];
  endSel.addEventListener('change', () => {
    ending[mode] = endSel.value as LineEnding;
    update();
  });
  endLabel.append(endSel);

  const sendBtn = document.createElement('button');
  sendBtn.type = 'button';
  sendBtn.className = 'uart-btn uart-ss-send';
  sendBtn.textContent = 'Send';
  sendBtn.setAttribute('data-testid', 'uart-ss-send');
  const closeBtn = document.createElement('button');
  closeBtn.type = 'button';
  closeBtn.className = 'uart-btn uart-ss-close';
  closeBtn.textContent = '✕';
  closeBtn.title = 'Send String 닫기';
  closeBtn.setAttribute('aria-label', 'Send String 닫기');
  closeBtn.addEventListener('click', () => o.onClose());
  bar.append(title, modes, endLabel, sendBtn, closeBtn);

  const PLACEHOLDER: Record<SendMode, string> = {
    text: '보낼 문자열 — Shift+Enter 또는 ⌘/Ctrl+Enter로 전송',
    hex: '예: 54 68 69 73 0D — 바이트당 HEX 2자리, 공백 선택',
  };
  const area = document.createElement('textarea');
  area.className = 'uart-ss-area';
  area.rows = 3;
  area.spellcheck = false;
  area.placeholder = PLACEHOLDER[mode];
  area.setAttribute('data-testid', 'uart-ss-input');
  area.setAttribute('aria-label', `Send String 입력 (${o.channel})`);

  const preview = document.createElement('div');
  preview.className = 'uart-ss-preview';
  preview.setAttribute('data-testid', 'uart-ss-preview');
  preview.setAttribute('aria-live', 'polite');
  root.append(bar, area, preview);

  let current: Encoded = { ok: false, error: '보낼 데이터 없음' };
  const update = () => {
    current = encodeSendString(mode, area.value, ending[mode]);
    const g = o.gate();
    area.setAttribute('aria-invalid', String(!current.ok && area.value !== ''));
    if (current.ok) {
      const route = routeFor(current.bytes.length) === 'inputLarge' ? ' · 큰 입력 — 확인 카드 후 전송' : '';
      preview.textContent = `${o.channel}로 보낼 바이트: ${previewText(current.bytes)}${route}`;
      preview.dataset.state = 'ok';
    } else {
      preview.textContent = area.value === '' ? `보낼 데이터 없음 · 줄 끝 ${LINE_ENDINGS.find((x) => x.id === ending[mode])?.label}` : current.error;
      preview.dataset.state = area.value === '' ? 'empty' : 'error';
    }
    if (!g.ok) preview.textContent += ` · 전송 불가: ${g.reason ?? ''}`;
    sendBtn.disabled = !current.ok || !g.ok;
    sendBtn.title = !g.ok ? `전송 불가: ${g.reason ?? ''}` : current.ok ? `${o.channel}에 ${current.bytes.length} B 전송 (uart.input)` : '';
  };
  const doSend = () => {
    update();
    if (!current.ok || !o.gate().ok) return;
    o.submit(current.bytes);
  };
  sendBtn.addEventListener('click', doSend);
  area.addEventListener('input', update);
  area.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.shiftKey || e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      doSend();
    }
  });

  return {
    root,
    get isOpen() {
      return open;
    },
    setOpen(v: boolean) {
      open = v;
      root.hidden = !v;
      if (v) {
        update();
        area.focus();
      }
    },
    update() {
      if (open) update();
    },
  };
}
