// Socket test drawer (selected web: ethernet-workbench/opus sock.js, right-hand `.sock` drawer).
// Real path only: every send is one explicit user action through ctx.act('send') → eth.send
// (ActionGate/policy unchanged). The preview is the daemon's dry-run (eth.send{dry_run:true} as a
// query). Templates and the composer draft are per-viewer local preferences; nothing is sent on
// load, there is no repeat timer and no implicit retransmit. Loom has no TCP session RPC: TCP sends
// use Loom's own connection per eth.send (eth/plan §4), so no connect/disconnect state is shown.
import { bytesToHex, type PanelContext } from '../../sdk';
import { btn, el, icon, loadJson, saveJson, seg } from '../../workbench/eth/bench';

export const SOCK_DRAFT_KEY = 'loom.eth.sock.draft.v1';
export const SOCK_TPL_KEY = 'loom.eth.sock.templates.v1';
const PREVIEW_MS = 200;
const MAX_TPL = 50;

type Proto = 'udp' | 'tcp';
type Fmt = 'hex' | 'text';
export interface SockDraft {
  proto: Proto;
  host: string;
  port: string;
  fromPort: string;
  fmt: Fmt;
  data: string;
}
export interface SockTemplate extends SockDraft {
  name: string;
}
const isStr = (v: unknown): v is string => typeof v === 'string';
function isDraft(v: unknown): v is SockDraft {
  const o = v as Record<string, unknown> | null;
  return !!o && (o.proto === 'udp' || o.proto === 'tcp') && (o.fmt === 'hex' || o.fmt === 'text') && isStr(o.host) && isStr(o.port) && isStr(o.data) && isStr(o.fromPort);
}
const isTplList = (v: unknown): v is SockTemplate[] => Array.isArray(v) && v.length <= MAX_TPL && v.every((t) => isDraft(t) && isStr((t as SockTemplate).name));
const DEFAULT_DRAFT: SockDraft = { proto: 'udp', host: '', port: '13400', fromPort: '', fmt: 'hex', data: '' };

/** `"02 fd 00 05"` / `"02FD0005"` / `0x02,` … → lowercase hex, or null when not whole bytes. */
export function parseHexPayload(s: string): string | null {
  const h = s.replace(/0x/gi, '').replace(/[\s:,_-]/g, '').toLowerCase();
  return h.length > 0 && h.length % 2 === 0 && /^[0-9a-f]+$/.test(h) ? h : null;
}

/** Text payload with the reference escapes (\r \n \t \\ \xHH); other characters as UTF-8. */
export function parseTextPayload(s: string): string | null {
  const out: number[] = [];
  const enc = new TextEncoder();
  let lit = '';
  const flush = () => {
    if (lit) out.push(...enc.encode(lit));
    lit = '';
  };
  for (let i = 0; i < s.length; i++) {
    const c = s[i]!;
    if (c !== '\\') {
      lit += c;
      continue;
    }
    const n = s[i + 1];
    if (n === 'r' || n === 'n' || n === 't' || n === '\\') {
      flush();
      out.push(n === 'r' ? 13 : n === 'n' ? 10 : n === 't' ? 9 : 92);
      i++;
    } else if (n === 'x' && /^[0-9a-fA-F]{2}$/.test(s.slice(i + 2, i + 4))) {
      flush();
      out.push(parseInt(s.slice(i + 2, i + 4), 16));
      i += 3;
    } else return null;
  }
  flush();
  return out.length ? bytesToHex(new Uint8Array(out)) : null;
}

/** Whole-string port check: 1..65535 decimal digits only (no sign, spaces, exponent or partial parse). */
export function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const n = Number(text);
  return n >= 1 && n <= 65535 ? n : null;
}

export interface SocketDrawer {
  root: HTMLElement;
  txHost: HTMLElement;
  txSum: HTMLElement;
  setOpen(open: boolean): void;
  readonly open: boolean;
  setWritable(w: boolean): void;
  dispose(): void;
}

export function createSocketDrawer(ctx: PanelContext, ch: string, onClose: () => void): SocketDrawer {
  let draft: SockDraft = { ...DEFAULT_DRAFT, ...loadJson(SOCK_DRAFT_KEY, isDraft, DEFAULT_DRAFT) };
  const tpls: SockTemplate[] = loadJson(SOCK_TPL_KEY, isTplList, []);
  let editing: number | null = null;
  let writable = false;
  let open = false;
  /** single in-flight guard: at most one eth.send from this drawer until the previous one settles */
  let inFlight = false;
  let pvTimer: ReturnType<typeof setTimeout> | null = null;
  let pvAc: AbortController | null = null;

  const root = el('aside', 'eth-sock');
  root.setAttribute('aria-label', 'TCP/UDP 소켓 테스트');
  root.setAttribute('data-testid', 'eth-sock');
  root.hidden = true;
  const head = el('div', 'sk-h');
  const h2 = el('h2', undefined, '소켓 테스트');
  const close = btn('', 'wb-ibtn', '닫기 (Alt+S)');
  close.append(icon('x'));
  close.setAttribute('aria-label', '소켓 테스트 닫기');
  close.addEventListener('click', onClose);
  head.append(h2, close);
  const body = el('div', 'sk-body');

  // endpoint
  const secEp = el('section', 'sk-sec');
  secEp.append(el('h3', undefined, '대상 · 엔드포인트'));
  const chRow = el('div', 'sk-row');
  chRow.append(el('label', undefined, '채널'), el('span', 'mono', ch));
  const protoSeg = seg<Proto>(
    [
      { value: 'udp', label: 'UDP' },
      { value: 'tcp', label: 'TCP', title: 'Loom 자체 연결로만 송신 (eth.send kind=tcp)' },
    ],
    draft.proto,
    (v) => {
      draft.proto = v;
      changed();
    },
    '프로토콜',
  );
  const protoRow = el('div', 'sk-row');
  protoRow.append(el('label', undefined, '프로토콜'), protoSeg.root);
  const host = Object.assign(el('input', 'wb-fld'), { placeholder: '192.168.0.10', value: draft.host, spellcheck: false });
  host.setAttribute('aria-label', '피어 IP/호스트');
  host.style.flex = '1';
  const port = Object.assign(el('input', 'wb-fld'), { placeholder: '13400', value: draft.port, inputMode: 'numeric' });
  port.setAttribute('aria-label', '피어 포트');
  port.style.width = '80px';
  const fromPort = Object.assign(el('input', 'wb-fld'), { placeholder: '(자동)', value: draft.fromPort, inputMode: 'numeric' });
  fromPort.setAttribute('aria-label', '출발 포트');
  fromPort.style.width = '80px';
  const peerRow = el('div', 'sk-row');
  peerRow.append(el('label', undefined, '피어'), host);
  const portRow = el('div', 'sk-row');
  portRow.append(el('label', undefined, '포트'), port, el('label', undefined, '출발'), fromPort);
  const epNote = el('div', 'sk-note', `실제 daemon eth.send · ${ch} 캡처에 TX가 보이면 목록에 TX 출처로 표시됩니다. TCP 세션 열기/닫기 상태는 백엔드가 제공하지 않아 표시하지 않습니다.`);
  secEp.append(chRow, protoRow, peerRow, portRow, epNote);

  // send
  const secSend = el('section', 'sk-sec');
  const sendH = el('h3', undefined, '보내기');
  const editTag = el('span', 'wb-chip pend');
  editTag.hidden = true;
  sendH.append(editTag);
  const fmtSeg = seg<Fmt>(
    [
      { value: 'hex', label: 'HEX' },
      { value: 'text', label: 'Text' },
    ],
    draft.fmt,
    (v) => {
      draft.fmt = v;
      changed();
    },
    '형식',
  );
  const fmtRow = el('div', 'sk-row');
  fmtRow.append(fmtSeg.root, el('span', 'sk-note', '공백/쉼표/콜론/0x 허용 · Text는 \\r \\n \\t \\xHH'));
  const data = Object.assign(el('textarea', 'wb-fld'), { value: draft.data, spellcheck: false });
  data.setAttribute('aria-label', '페이로드');
  data.setAttribute('data-testid', 'eth-sock-data');
  const err = el('div', 'sk-err');
  const prev = el('div', 'sk-prev');
  prev.setAttribute('data-testid', 'eth-sock-preview');
  const sendBtn = btn('', 'wb-btn primary', '보내기 (Ctrl/⌘+Enter) — eth.send 1회');
  sendBtn.append(icon('send'), '보내기');
  sendBtn.setAttribute('data-testid', 'eth-sock-send');
  const sendRow = el('div', 'sk-row');
  const tplName = Object.assign(el('input', 'wb-fld'), { placeholder: '템플릿 이름' });
  tplName.setAttribute('aria-label', '템플릿 이름');
  tplName.style.flex = '1';
  const saveBtn = btn('새로 저장', 'wb-btn', '현재 내용을 템플릿으로 저장 (전송 없음)');
  const updBtn = btn('템플릿 업데이트', 'wb-btn');
  const cancelEdit = btn('편집 취소', 'wb-btn ghost');
  sendRow.append(sendBtn);
  const tplRow = el('div', 'sk-row');
  tplRow.append(tplName, saveBtn, updBtn, cancelEdit);
  secSend.append(sendH, fmtRow, data, err, prev, sendRow, tplRow);

  // templates
  const secTpl = el('section', 'sk-sec');
  secTpl.append(el('h3', undefined, '템플릿'));
  const tplList = el('div', 'sk-tpls');
  secTpl.append(tplList);

  // TX log (eth.tx rows, real request lifecycle)
  const tx = el('details', 'eth-tx');
  tx.setAttribute('data-testid', 'eth-tx');
  tx.open = true;
  const txSum = el('summary', undefined, '송신');
  const txHost = el('div', 'eth-tx-host');
  tx.append(txSum, txHost);

  body.append(secEp, secSend, secTpl, tx);
  root.append(head, body);

  const payloadHex = (): string | null => (draft.fmt === 'hex' ? parseHexPayload(draft.data) : parseTextPayload(draft.data));
  const target = (): string | null => {
    const p = parsePort(draft.port);
    if (!draft.host.trim() || p === null) return null;
    return `${draft.host.trim()}:${p}`;
  };
  /** empty = daemon picks the source port; anything else must be a valid port or nothing is sent */
  const fromPortValue = (): number | null | 'invalid' => (draft.fromPort === '' ? null : (parsePort(draft.fromPort) ?? 'invalid'));
  const params = (hex: string, to: string, fp: number | null) => ({ kind: draft.proto, ch, to, ...(fp !== null ? { from_port: fp } : {}), hex });

  const validate = (): { hex: string; to: string; fp: number | null } | null => {
    const hex = payloadHex();
    const to = target();
    const fp = fromPortValue();
    const msgs: string[] = [];
    host.classList.toggle('invalid', !!draft.host && !draft.host.trim());
    const portBad = !!draft.port && parsePort(draft.port) === null;
    port.classList.toggle('invalid', portBad);
    if (portBad) msgs.push('포트: 1–65535 정수');
    fromPort.classList.toggle('invalid', fp === 'invalid');
    if (fp === 'invalid') msgs.push('출발 포트: 비우거나(자동) 1–65535 정수');
    data.classList.toggle('invalid', !!draft.data && hex === null);
    if (draft.data && hex === null) msgs.push(draft.fmt === 'hex' ? 'hex 형식 오류 — 바이트 단위(짝수 자리) 0-9a-f' : '알 수 없는 escape (\\r \\n \\t \\\\ \\xHH)');
    err.textContent = msgs.join(' · ');
    if (!hex || !to || fp === 'invalid') return null;
    return { hex, to, fp };
  };

  const byteLine = () => {
    const hex = payloadHex();
    prev.textContent = hex ? `${hex.length / 2} B · ${hex.replace(/(..)/g, '$1 ').trim().slice(0, 96)}${hex.length > 64 ? '…' : ''}` : '';
  };
  const runPreview = () => {
    pvTimer = null;
    const v = validate();
    byteLine();
    if (!v) return;
    pvAc?.abort();
    const ac = new AbortController();
    pvAc = ac;
    ctx
      .query<Record<string, unknown>>('eth.send', { ...params(v.hex, v.to, v.fp), dry_run: true }, ac.signal)
      .then((r) => {
        if (ac.signal.aborted) return;
        const bits = [r?.risk, r?.proto, r?.info].filter((x): x is string => typeof x === 'string');
        if (bits.length) prev.textContent = `${prev.textContent} · ${bits.join(' · ')}`;
      })
      .catch(() => {
        /* preview is optional; the byte line above stays */
      });
  };

  const syncButtons = () => {
    const v = validate();
    sendBtn.disabled = !writable || !v || inFlight;
    sendBtn.title = !writable
      ? '권한 또는 쓰기 잠금으로 송신 불가'
      : inFlight
        ? '이전 송신 처리 중 — 완료 후 다시'
        : !v
          ? '피어/포트/페이로드를 확인하세요'
          : '보내기 (Ctrl/⌘+Enter) — eth.send 1회';
    saveBtn.disabled = !draft.data.trim();
    updBtn.hidden = cancelEdit.hidden = editing === null;
    editTag.hidden = editing === null;
    editTag.textContent = editing !== null ? `편집 중: ${tpls[editing]?.name ?? ''}` : '';
  };

  const changed = () => {
    saveJson(SOCK_DRAFT_KEY, draft);
    syncButtons();
    if (pvTimer) clearTimeout(pvTimer);
    pvTimer = setTimeout(runPreview, PREVIEW_MS);
  };
  const loadDraft = (d: SockDraft) => {
    draft = { proto: d.proto, host: d.host, port: d.port, fromPort: d.fromPort, fmt: d.fmt, data: d.data };
    host.value = draft.host;
    port.value = draft.port;
    fromPort.value = draft.fromPort;
    data.value = draft.data;
    protoSeg.set(draft.proto);
    fmtSeg.set(draft.fmt);
    changed();
  };
  host.addEventListener('input', () => {
    draft.host = host.value;
    changed();
  });
  port.addEventListener('input', () => {
    draft.port = port.value;
    changed();
  });
  fromPort.addEventListener('input', () => {
    draft.fromPort = fromPort.value;
    changed();
  });
  data.addEventListener('input', () => {
    draft.data = data.value;
    changed();
  });

  const setInFlight = (v: boolean) => {
    inFlight = v;
    renderTpls(); // template ▶ buttons follow the guard; also re-syncs Send
  };
  /** every send path (button, Ctrl/⌘+Enter, template ▶) goes through here: one request at a time */
  const send = (): boolean => {
    if (inFlight || !writable) return false;
    const v = validate();
    if (!v) return false;
    const proto = draft.proto;
    setInFlight(true);
    ctx
      .act('send', params(v.hex, v.to, v.fp))
      .then((hnd) => ctx.ui.toast(`${proto.toUpperCase()} ${v.to} 송신 요청${hnd?.requestId ? ` (${hnd.requestId})` : ''}`))
      .catch((e: unknown) => (err.textContent = `송신 실패: ${e instanceof Error ? e.message : String(e)}`))
      .finally(() => setInFlight(false));
    return true;
  };
  sendBtn.addEventListener('click', () => void send());
  data.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (!e.repeat) send();
    }
  });

  const persistTpls = () => saveJson(SOCK_TPL_KEY, tpls);
  const renderTpls = () => {
    const rows = tpls.map((t, i) => {
      const r = el('div', `sk-tpl${editing === i ? ' editing' : ''}`);
      r.dataset.tpl = String(i);
      const acts = el('span', 'acts');
      const run = btn('', 'wb-ibtn', '불러와서 1회 보내기');
      run.append(icon('play'));
      run.setAttribute('aria-label', `${t.name} 보내기`);
      run.disabled = !writable || inFlight;
      run.addEventListener('click', () => {
        if (inFlight || !writable) return; // never swap the composer under an in-flight send
        loadDraft(t);
        send();
      });
      const ed = btn('', 'wb-ibtn', '편집 (전송 없음)');
      ed.append(icon('edit'));
      ed.addEventListener('click', () => {
        editing = i;
        tplName.value = t.name;
        loadDraft(t);
        renderTpls();
      });
      const del = btn('', 'wb-ibtn', '삭제');
      del.append(icon('trash'));
      del.addEventListener('click', () => {
        tpls.splice(i, 1);
        if (editing === i) editing = null;
        persistTpls();
        renderTpls();
      });
      acts.append(run, ed, del);
      r.append(el('span', 'tn-name', t.name), acts, el('span', 'tn-sub', `${t.proto.toUpperCase()} ${t.host || '?'}:${t.port} · ${t.fmt} ${t.data.slice(0, 40)}`));
      return r;
    });
    tplList.replaceChildren(...(rows.length ? rows : [el('div', 'sk-note', '저장된 템플릿 없음 — 보내기 영역에서 [새로 저장]')]));
    syncButtons();
  };
  saveBtn.addEventListener('click', () => {
    if (tpls.length >= MAX_TPL) return ctx.ui.toast(`템플릿은 최대 ${MAX_TPL}개`);
    tpls.push({ ...draft, name: tplName.value.trim() || `${draft.proto.toUpperCase()} ${draft.host}:${draft.port}` });
    editing = null;
    tplName.value = '';
    persistTpls();
    renderTpls();
  });
  updBtn.addEventListener('click', () => {
    if (editing === null || !tpls[editing]) return;
    tpls[editing] = { ...draft, name: tplName.value.trim() || tpls[editing]!.name };
    editing = null;
    tplName.value = '';
    persistTpls();
    renderTpls();
  });
  cancelEdit.addEventListener('click', () => {
    editing = null;
    tplName.value = '';
    renderTpls();
  });
  renderTpls();
  byteLine(); // local only — restoring a draft never queries or sends

  return {
    root,
    txHost,
    txSum,
    get open() {
      return open;
    },
    setOpen(v: boolean) {
      open = v;
      root.hidden = !v;
      if (v) host.focus();
    },
    setWritable(w: boolean) {
      if (w === writable) return;
      writable = w;
      for (const f of [host, port, fromPort, data]) f.disabled = !w;
      renderTpls();
    },
    dispose() {
      if (pvTimer) clearTimeout(pvTimer);
      pvAc?.abort();
      root.remove();
    },
  };
}
