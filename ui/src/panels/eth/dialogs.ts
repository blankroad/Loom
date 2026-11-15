// In-panel dialogs: edit-and-send (hex + live decode preview), UDP/TCP payload send,
// capture start. All writes go through ctx.act (ActionGate, origin=user by the server).
// The preview uses `eth.send{dry_run:true}` as a QUERY — the context refuses that RPC
// unless dry_run is true.

import { bytesToHex, type PanelContext } from '../../sdk';
import type { IfaceConfig } from './ifaceConfigs';

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

export const CHANNEL_RE = /^[A-Za-z0-9_.-]{1,64}$/;

interface CaptureFormOpts {
  /** null = start this panel's channel (fixed); otherwise the add-channel form with an editable new alias */
  add: null | {
    existing: () => Promise<string[]>;
    onStarted: (ch: string, cfg: IfaceConfig) => void;
    /** prefill from a saved interface configuration (unbound module application) */
    initial?: IfaceConfig | null;
    /** offer [구성 저장]: store the form locally WITHOUT any RPC (previous alias when editing) */
    onSave?: (cfg: IfaceConfig, previousAlias: string | null) => void;
    title?: string;
    testId?: string;
  };
}

/** Shared capture form. `eth.capture.start` needs iface + backend (crates/loom-eth/src/model.rs
 *  CaptureStartParams: backend has no default); BPF is live-only (worker validate_start). Replay and
 *  stream need extra sources and stay CLI-only (`loom eth capture start … --replay`). */
function captureForm(host: HTMLElement, ctx: PanelContext, ch: string, o: CaptureFormOpts): DialogHandle {
  const adding = o.add !== null;
  const d = openDialog(host, o.add?.title ?? (adding ? '채널 추가 — 새 인터페이스 캡처' : `캡처 시작 — ${ch}`), o.add?.testId ?? (adding ? 'eth-add-channel-dialog' : 'eth-capture-dialog'));
  const alias = Object.assign(document.createElement('input'), { placeholder: 'lan2', spellcheck: false });
  alias.setAttribute('aria-label', '새 채널 이름');
  const iface = Object.assign(document.createElement('input'), { placeholder: 'en5 / eth0 / sim', spellcheck: false });
  iface.setAttribute('aria-label', 'NIC');
  const list = document.createElement('datalist');
  list.id = `eth-ifaces-${Math.random().toString(36).slice(2)}`;
  iface.setAttribute('list', list.id);
  const backend = document.createElement('select');
  backend.setAttribute('aria-label', '백엔드');
  for (const [v, t] of [['live', 'live — 실제 NIC'], ['sim', 'sim — daemon 합성 탭 (SIM)']] as const) backend.append(Object.assign(document.createElement('option'), { value: v, textContent: t }));
  const bpf = Object.assign(document.createElement('input'), { placeholder: 'port 13400 or arp (BPF, live 전용)' });
  bpf.className = 'mono bpf';
  const snaplen = Object.assign(document.createElement('input'), { value: '262144', inputMode: 'numeric' });
  const promisc = Object.assign(document.createElement('input'), { type: 'checkbox' });
  const buffer = Object.assign(document.createElement('input'), { value: '64', inputMode: 'numeric' });
  const msg = document.createElement('div');
  msg.className = 'muted';
  msg.setAttribute('data-testid', 'eth-capture-msg');
  const note = 'BPF는 커널에서 버린다(기록에도 없음). 보기만 줄이려면 표시 필터를 쓰세요.';
  msg.textContent = note;
  const syncBackend = () => {
    if (iface.value.trim() === 'sim') backend.value = 'sim';
    bpf.disabled = backend.value !== 'live';
  };
  iface.addEventListener('input', syncBackend);
  backend.addEventListener('change', syncBackend);
  ctx
    .query<{ ifaces?: { name?: string; desc?: string; alias?: string; can_capture?: boolean }[] }>('eth.ifaces', {})
    .then((r) => {
      for (const i of r?.ifaces ?? []) {
        if (!i.name || i.name === 'replay' || i.name === 'stream') continue; // need a source file / producer: CLI only
        list.append(Object.assign(document.createElement('option'), { value: i.name, textContent: `${i.alias ?? i.desc ?? ''}${i.can_capture === false ? ' (권한 없음)' : ''}` }));
      }
    })
    .catch(() => {});
  const init = o.add?.initial ?? null;
  if (init) {
    alias.value = init.alias;
    iface.value = init.iface;
    backend.value = init.backend;
    bpf.value = init.bpf;
    snaplen.value = String(init.snaplen);
    promisc.checked = init.promisc;
    buffer.value = String(init.bufferMib);
  }
  /** the form as a config, or null (message set) when alias / NIC are invalid */
  const formConfig = (): IfaceConfig | null => {
    const a = adding ? alias.value.trim() : ch;
    if (adding && !CHANNEL_RE.test(a)) {
      msg.textContent = '채널 이름: 영문·숫자·_.- 1–64자';
      return null;
    }
    if (!iface.value.trim()) {
      msg.textContent = 'NIC를 고르세요';
      return null;
    }
    const posInt = (v: string, def: number) => {
      const n = Number(v);
      return Number.isInteger(n) && n > 0 ? n : def;
    };
    const be = backend.value === 'sim' ? 'sim' : 'live';
    return { alias: a, iface: iface.value.trim(), backend: be, bpf: be === 'live' ? bpf.value.trim() : '', snaplen: posInt(snaplen.value, 262144), promisc: promisc.checked, bufferMib: posInt(buffer.value, 64) };
  };
  let busy = false;
  const start = btn(adding ? (init || o.add?.onSave ? '연결 (캡처 시작)' : '추가하고 시작') : '시작', () => {
    if (busy) return;
    const cfg = formConfig();
    if (!cfg) return;
    const newCh = cfg.alias;
    const params = {
      iface: iface.value.trim(),
      ch: newCh,
      backend: backend.value,
      ...(backend.value === 'live' && bpf.value.trim() ? { bpf: bpf.value.trim() } : {}),
      snaplen: Number(snaplen.value) || 262144,
      promisc: promisc.checked,
      buffer_mib: Number(buffer.value) || 64,
    };
    busy = true;
    start.disabled = true;
    const go = async () => {
      if (o.add) {
        // never restart / overwrite a channel that already exists (this one or another panel's)
        const names = await o.add.existing().catch(() => null);
        if (names === null) throw new Error('채널 목록 확인 실패 — 추가하지 않음');
        if (newCh === ch || names.includes(newCh)) throw new Error(`이미 있는 채널 "${newCh}" — 다른 이름을 쓰세요 (기존 채널은 그 패널의 [캡처 시작…])`);
      }
      await ctx.act('captureStart', params);
    };
    go()
      .then(() => {
        d.close();
        if (o.add) o.add.onStarted(newCh, cfg);
      })
      .catch((e: unknown) => {
        msg.textContent = `${adding ? '추가' : '시작'} 실패: ${e instanceof Error ? e.message : String(e)}`;
        msg.classList.add('err');
        busy = false;
        start.disabled = false;
      });
  }, 'primary');
  start.setAttribute('data-testid', adding ? 'eth-add-channel-start' : 'eth-capture-start');
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append(btn('취소 (Esc)', d.close));
  const onSave = o.add?.onSave;
  if (onSave) {
    const save = btn('구성 저장', () => {
      const cfg = formConfig();
      if (!cfg) return;
      onSave(cfg, init?.alias ?? null); // local only: no eth.capture.start, no device opened
      d.close();
    });
    save.title = '이 브라우저에 구성만 저장 — 캡처를 시작하지 않음';
    save.setAttribute('data-testid', 'eth-iface-save');
    row.append(save);
  }
  row.append(start);
  if (adding) d.body.append(field('새 채널 이름', alias));
  else d.body.append(field('채널', Object.assign(document.createElement('b'), { textContent: ch })));
  d.body.append(field('NIC', iface), list, field('백엔드', backend), field('캡처 필터 (BPF)', bpf), field('snaplen', snaplen), field('promiscuous', promisc), field('버퍼 MiB', buffer), msg, row);
  syncBackend();
  (adding ? alias : iface).focus();
  return d;
}

/** Capture start for THIS panel's channel (fixed name). */
export function openCaptureStart(host: HTMLElement, ctx: PanelContext, ch: string): DialogHandle {
  return captureForm(host, ctx, ch, { add: null });
}

/** Add channel: a NEW editable alias + a real NIC → eth.capture.start → open that channel's panel. */
export function openAddChannel(host: HTMLElement, ctx: PanelContext, currentCh: string): DialogHandle {
  return captureForm(host, ctx, currentCh, {
    add: {
      existing: () =>
        ctx.query<{ channels?: { ch?: string }[] }>('eth.capture.status', {}).then((r) => (r?.channels ?? []).map((c) => c.ch).filter((x): x is string => typeof x === 'string')),
      onStarted: (ch) => {
        ctx.ui.openPanel('eth.packets', { channel: ch });
        ctx.ui.toast(`채널 ${ch} 캡처 시작 — 새 패널로 열었습니다`);
      },
    },
  });
}

/** Unbound Ethernet module application: explicit [연결] for a saved or new interface configuration.
 *  Same capture form (never restarts an existing daemon channel); [구성 저장] stores it locally only. */
export function openConnectInterface(
  host: HTMLElement,
  ctx: PanelContext,
  initial: IfaceConfig | null,
  on: { onSave: (cfg: IfaceConfig, previousAlias: string | null) => void; onStarted: (ch: string, cfg: IfaceConfig) => void },
): DialogHandle {
  return captureForm(host, ctx, '', {
    add: {
      existing: () =>
        ctx.query<{ channels?: { ch?: string }[] }>('eth.capture.status', {}).then((r) => (r?.channels ?? []).map((c) => c.ch).filter((x): x is string => typeof x === 'string')),
      onStarted: on.onStarted,
      onSave: on.onSave,
      initial,
      title: initial ? `인터페이스 연결 — ${initial.alias}` : '인터페이스 연결 — 새 구성',
      testId: 'eth-connect-dialog',
    },
  });
}

/** Follow stream (selected web dlgFollow): the daemon's `eth.follow` reassembly — client red, server blue. */
export function openFollow(
  host: HTMLElement,
  ctx: PanelContext,
  ch: string,
  proto: 'tcp' | 'udp',
  stream: number,
  onOnly: (expr: string) => void,
): DialogHandle {
  const d = openDialog(host, `스트림 따라가기 — ${proto.toUpperCase()} stream ${stream}`, 'eth-follow-dialog');
  d.el.classList.add('eth-dialog-wide');
  const bar = document.createElement('div');
  bar.className = 'eth-fs-bar';
  const view = document.createElement('select');
  for (const [v, t] of [['ascii', 'ASCII'], ['hex', 'Hex Dump'], ['raw', 'Raw']] as const) view.append(Object.assign(document.createElement('option'), { value: v, textContent: t }));
  view.setAttribute('aria-label', '표시 형식');
  const sum = document.createElement('span');
  sum.className = 'eth-fs-sum';
  const out = document.createElement('div');
  out.className = 'eth-fs-body';
  out.setAttribute('data-testid', 'eth-follow-body');
  let segs: { dir: string; bytes: Uint8Array; seq: string }[] = [];
  const toBytes = (h: string) => new Uint8Array((h.match(/../g) ?? []).map((x) => parseInt(x, 16)));
  const render = () => {
    const parts = segs.map((s) => {
      const div = document.createElement('div');
      div.className = s.dir === 'c2s' ? 'c' : 's';
      if (view.value === 'ascii') div.textContent = [...s.bytes].map((b) => (b === 10 || b === 13 || b === 9 || (b >= 32 && b < 127) ? String.fromCharCode(b) : '.')).join('');
      else if (view.value === 'raw') div.textContent = bytesToHex(s.bytes);
      else {
        const lines: string[] = [];
        for (let o = 0; o < s.bytes.length; o += 16) {
          const sl = [...s.bytes.subarray(o, o + 16)];
          lines.push(`${o.toString(16).padStart(8, '0')}  ${sl.map((b) => b.toString(16).padStart(2, '0')).join(' ').padEnd(47)}  ${sl.map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('')}`);
        }
        div.textContent = lines.join('\n');
      }
      return div;
    });
    out.replaceChildren(...(parts.length ? parts : [Object.assign(document.createElement('div'), { className: 'gap', textContent: '페이로드 없음' })]));
  };
  view.addEventListener('change', render);
  const load = () => {
    out.textContent = '불러오는 중…';
    ctx
      .query<{ segments?: { dir?: string; data?: string; seq?: string }[]; bytes?: unknown; truncated?: boolean; gaps?: unknown }>('eth.follow', { ch, proto, stream })
      .then((r) => {
        segs = (r?.segments ?? []).map((s) => ({ dir: s.dir === 'c2s' ? 'c2s' : 's2c', bytes: toBytes(typeof s.data === 'string' ? s.data : ''), seq: String(s.seq ?? '') }));
        const c = segs.filter((s) => s.dir === 'c2s').reduce((a, s) => a + s.bytes.length, 0);
        const sv = segs.filter((s) => s.dir !== 'c2s').reduce((a, s) => a + s.bytes.length, 0);
        sum.textContent = `클라이언트 ${c} B · 서버 ${sv} B${r?.truncated ? ' · 잘림(최대 크기)' : ''} — daemon eth.follow`;
        render();
      })
      .catch((e: unknown) => (out.textContent = `스트림 조회 실패: ${e instanceof Error ? e.message : String(e)}`));
  };
  const expr = `${proto}.stream == ${stream}`;
  bar.append(view, btn('새로 고침', load), btn('이 스트림만 표시', () => {
    onOnly(expr);
    d.close();
  }), sum);
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append(btn('닫기 (Esc)', d.close));
  d.body.append(bar, out, row);
  load();
  view.focus();
  return d;
}

/** Shortcut list (selected web dlgKeys) — only keys this panel actually handles. */
export function openKeys(host: HTMLElement): DialogHandle {
  const d = openDialog(host, '단축키', 'eth-keys-dialog');
  const kv = document.createElement('div');
  kv.className = 'eth-kv';
  for (const [k, t] of [
    ['Space', '표시 일시정지 / 라이브로 (목록 포커스)'],
    ['Ctrl+/', '표시 필터로 이동'],
    ['Enter / Esc', '필터 적용 / 편집 되돌리기'],
    ['F8 · Ctrl+↓', '다음 패킷'],
    ['F7 · Ctrl+↑', '이전 패킷'],
    ['Alt+S', '소켓 테스트 열기/닫기'],
    ['Ctrl/⌘+Enter', '소켓 테스트 보내기'],
    ['↑ ↓ (경계)', '패널 경계 조절 · 더블클릭 기본값'],
  ] as const) {
    const kb = document.createElement('kbd');
    kb.textContent = k;
    kv.append(kb, Object.assign(document.createElement('span'), { textContent: t }));
  }
  const row = document.createElement('div');
  row.className = 'eth-dialog-actions';
  row.append(btn('닫기 (Esc)', d.close));
  d.body.append(kv, row);
  (row.firstElementChild as HTMLElement).focus();
  return d;
}
