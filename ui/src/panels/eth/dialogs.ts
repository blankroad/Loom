// In-panel dialogs: edit-and-send (hex + live decode preview), UDP/TCP payload send,
// capture start. All writes go through ctx.act (ActionGate, origin=user by the server).
// The preview uses `eth.send{dry_run:true}` as a QUERY — the context refuses that RPC
// unless dry_run is true.

import { bytesToHex, type PanelContext } from '../../sdk';

export const PREVIEW_MS = 200;

interface DialogHandle {
  el: HTMLDivElement;
  body: HTMLDivElement;
  close(): void;
}

function openDialog(host: HTMLElement, title: string, testId: string): DialogHandle {
  const overlay = document.createElement('div');
  overlay.className = 'eth-dialog-overlay';
  const el = document.createElement('div');
  el.className = 'eth-dialog';
  el.setAttribute('role', 'dialog');
  el.setAttribute('aria-label', title);
  el.setAttribute('data-testid', testId);
  const h = document.createElement('header');
  h.textContent = title;
  const body = document.createElement('div');
  overlay.append(el);
  el.append(h, body);
  host.append(overlay);
  const close = () => overlay.remove();
  overlay.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  });
  return { el, body, close };
}

function field(labelText: string, input: HTMLElement): HTMLLabelElement {
  const l = document.createElement('label');
  l.className = 'eth-field';
  const span = document.createElement('span');
  span.textContent = labelText;
  l.append(span, input);
  return l;
}

function btn(text: string, on: () => void, cls = ''): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = text;
  if (cls) b.className = cls;
  b.addEventListener('click', on);
  return b;
}

const cleanHex = (s: string) => s.replace(/[\s:]/g, '').toLowerCase();
const validHex = (h: string) => h.length > 0 && h.length % 2 === 0 && /^[0-9a-f]+$/.test(h);

/** Edit-and-send: hex editor + live decode preview from the server's dry-run. */
export function openEditSend(host: HTMLElement, ctx: PanelContext, ch: string, hex: string, risk: string | null): DialogHandle {
  const d = openDialog(host, '편집 후 송신 (raw)', 'eth-edit-dialog');
  const ta = document.createElement('textarea');
  ta.className = 'mono';
  ta.rows = 6;
  ta.value = hex.replace(/(..)/g, '$1 ').trim();
  ta.setAttribute('aria-label', '송신할 프레임 hex');
  const preview = document.createElement('div');
  preview.className = 'muted';
  preview.setAttribute('data-testid', 'eth-edit-preview');
  const riskEl = document.createElement('span');
  riskEl.className = 'risk';
  riskEl.textContent = risk ?? 'R?';
  let t: ReturnType<typeof setTimeout> | null = null;
  let ac: AbortController | null = null;
  const runPreview = () => {
    const h = cleanHex(ta.value);
    if (!validHex(h)) {
      preview.textContent = 'hex 형식 오류';
      return;
    }
    ac?.abort();
    ac = new AbortController();
    const mine = ac;
    ctx
      .query<Record<string, unknown>>('eth.send', { kind: 'raw', ch, hex: h, dry_run: true }, mine.signal)
      .then((r) => {
        if (mine.signal.aborted) return;
        if (typeof r?.risk === 'string') riskEl.textContent = r.risk;
        preview.textContent = [r?.proto, r?.info].filter((x) => typeof x === 'string').join(' · ') || '미리보기 정보 없음';
      })
      .catch(() => {
        if (!mine.signal.aborted) preview.textContent = '미리보기 없음 (서버 dry-run 필요)';
      });
  };
  ta.addEventListener('input', () => {
    if (t) clearTimeout(t);
    t = setTimeout(runPreview, PREVIEW_MS);
  });
  const send = btn('보내기', () => {
    const h = cleanHex(ta.value);
    if (!validHex(h)) {
      preview.textContent = 'hex 형식 오류';
      return;
    }
    ctx
      .act('send', { kind: 'raw', ch, hex: h })
      .then(() => d.close())
      .catch((e: unknown) => (preview.textContent = `송신 실패: ${e instanceof Error ? e.message : String(e)}`));
  }, 'primary');
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append('등급 ', riskEl, ' ', btn('취소 (Esc)', d.close), send);
  d.body.append(ta, preview, row);
  runPreview();
  ta.focus();
  return d;
}

/** UDP/TCP payload (TCP: Loom's own connection only — eth/plan §4). */
export function openSocketSend(host: HTMLElement, ctx: PanelContext, ch: string): DialogHandle {
  const d = openDialog(host, 'UDP/TCP 페이로드 송신', 'eth-socket-dialog');
  const proto = document.createElement('select');
  for (const p of ['udp', 'tcp']) proto.append(Object.assign(document.createElement('option'), { value: p, textContent: p.toUpperCase() }));
  const to = Object.assign(document.createElement('input'), { placeholder: '192.168.0.10:13400' });
  const fromPort = Object.assign(document.createElement('input'), { placeholder: '(선택)', inputMode: 'numeric' });
  const mode = document.createElement('select');
  for (const m of ['hex', 'text']) mode.append(Object.assign(document.createElement('option'), { value: m, textContent: m }));
  const payload = Object.assign(document.createElement('textarea'), { rows: 4 });
  payload.className = 'mono';
  const msg = document.createElement('div');
  msg.className = 'muted';
  const send = btn('보내기', () => {
    const hex = mode.value === 'hex' ? cleanHex(payload.value) : bytesToHex(new TextEncoder().encode(payload.value));
    if (!/^[^:\s]+:\d{1,5}$/.test(to.value.trim()) || !validHex(hex)) {
      msg.textContent = '대상(IP:PORT)과 페이로드를 확인하세요';
      return;
    }
    const fp = Number(fromPort.value);
    ctx
      .act('send', { kind: proto.value, ch, to: to.value.trim(), ...(fromPort.value && fp > 0 ? { from_port: fp } : {}), hex })
      .then(() => d.close())
      .catch((e: unknown) => (msg.textContent = `송신 실패: ${e instanceof Error ? e.message : String(e)}`));
  }, 'primary');
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append(btn('취소 (Esc)', d.close), send);
  d.body.append(field('프로토콜', proto), field('대상', to), field('출발 포트', fromPort), field('형식', mode), field('페이로드', payload), msg, row);
  to.focus();
  return d;
}

/** Capture start: NIC · BPF (kernel filter — not the display filter) · snaplen · promisc · buffer. */
export function openCaptureStart(host: HTMLElement, ctx: PanelContext, ch: string): DialogHandle {
  const d = openDialog(host, '캡처 시작', 'eth-capture-dialog');
  const iface = Object.assign(document.createElement('input'), { placeholder: 'eth0 / lo / \\Device\\NPF_…' });
  const list = document.createElement('datalist');
  list.id = `eth-ifaces-${Math.random().toString(36).slice(2)}`;
  iface.setAttribute('list', list.id);
  const bpf = Object.assign(document.createElement('input'), { placeholder: 'port 13400 or arp (BPF)' });
  bpf.className = 'mono bpf';
  const snaplen = Object.assign(document.createElement('input'), { value: '262144', inputMode: 'numeric' });
  const promisc = Object.assign(document.createElement('input'), { type: 'checkbox' });
  const buffer = Object.assign(document.createElement('input'), { value: '64', inputMode: 'numeric' });
  const msg = document.createElement('div');
  msg.className = 'muted';
  msg.textContent = 'BPF는 커널에서 버린다(기록에도 없음). 보기만 줄이려면 표시 필터를 쓰세요.';
  ctx
    .query<{ ifaces?: { name?: string; alias?: string; can_capture?: boolean }[] }>('eth.ifaces', {})
    .then((r) => {
      for (const i of r?.ifaces ?? []) {
        if (!i.name) continue;
        list.append(Object.assign(document.createElement('option'), { value: i.name, textContent: `${i.alias ?? ''}${i.can_capture === false ? ' (권한 없음)' : ''}` }));
      }
    })
    .catch(() => {});
  const start = btn('시작', () => {
    if (!iface.value.trim()) {
      msg.textContent = 'NIC를 고르세요';
      return;
    }
    ctx
      .act('captureStart', {
        iface: iface.value.trim(),
        ch,
        ...(bpf.value.trim() ? { bpf: bpf.value.trim() } : {}),
        snaplen: Number(snaplen.value) || 262144,
        promisc: promisc.checked,
        buffer_mib: Number(buffer.value) || 64,
      })
      .then(() => d.close())
      .catch((e: unknown) => (msg.textContent = `시작 실패: ${e instanceof Error ? e.message : String(e)}`));
  }, 'primary');
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append(btn('취소 (Esc)', d.close), start);
  d.body.append(field('NIC', iface), list, field('캡처 필터 (BPF)', bpf), field('snaplen', snaplen), field('promiscuous', promisc), field('버퍼 MiB', buffer), msg, row);
  iface.focus();
  return d;
}
