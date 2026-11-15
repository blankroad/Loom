// PSU module application — connect section shown while psu.gauge has NO channel prop
// (work/reports/module-apps/CONTRACT.md). Safety: an unbound gauge has no target, so it never
// issues psu.out / psu.set / psu.status and never falls back to an implicit "bench" alias.
//
// Engine truth (crates/loom-psu gateway/handlers.rs `Connect`):
//   psu.connect{ch, backend:"sim", vmax?, imax?}             — SimPsu
//   psu.connect{ch, backend:"scpi-tcp", address, vmax?, imax?} — external SCPI endpoint on LOOPBACK TCP only
//   "visa" / anything else → E_NOT_IMPLEMENTED (listed disabled here, never mapped to another backend).
// vmax/imax may only LOWER the profile limits (model.rs Limits::from_value: min(default)). Connect only
// queries the supply — it never switches the output (help notes); output ON stays a separate R3 action.
// One supply per PSU worker: a second connect with another alias is refused by the daemon (E_PORT_BUSY).

import type { PanelContext } from '../../sdk';

export const PSU_NAME_RE = /^[a-z][a-z0-9_-]{0,31}$/;
export type PsuBackend = 'sim' | 'scpi-tcp';
export const PSU_BACKENDS: readonly { value: string; label: string; implemented: boolean }[] = [
  { value: 'sim', label: 'sim · SimPsu (가상 전원)', implemented: true },
  { value: 'scpi-tcp', label: 'scpi-tcp · 루프백 SCPI 엔드포인트', implemented: true },
  { value: 'visa', label: 'VISA / USB / GPIB / LAN 계측기 — 엔진 미구현', implemented: false },
];

export interface PsuConnectDraft {
  ch: string;
  backend: PsuBackend;
  address: string;
  vmax: string;
  imax: string;
}
export interface PsuConnectParams {
  ch: string;
  backend: PsuBackend;
  address?: string;
  vmax?: number;
  imax?: number;
}

/** loopback `<ip>:<port>` (Rust SocketAddr: no host names) — accepted here: 127.x.x.x:p or [::1]:p */
export function isLoopbackAddr(a: string): boolean {
  const v4 = /^127\.(\d{1,3})\.(\d{1,3})\.(\d{1,3}):(\d{1,5})$/.exec(a);
  const v6 = /^\[::1\]:(\d{1,5})$/.exec(a);
  let port: number;
  if (v4) {
    if ([v4[1], v4[2], v4[3]].some((o) => Number(o) > 255 || (o!.length > 1 && o!.startsWith('0')))) return false;
    port = Number(v4[4]);
  } else if (v6) port = Number(v6[1]);
  else return false;
  return port >= 1 && port <= 65_535;
}

const limit = (s: string): { ok: true; v: number | undefined } | { ok: false } => {
  const t = s.trim();
  if (!t) return { ok: true, v: undefined };
  if (!/^\d+(\.\d+)?$/.test(t)) return { ok: false };
  const v = Number(t);
  return Number.isFinite(v) && v > 0 ? { ok: true, v } : { ok: false };
};

export function psuConnectParams(d: PsuConnectDraft): { ok: true; params: PsuConnectParams } | { ok: false; error: string } {
  const ch = d.ch.trim();
  if (!PSU_NAME_RE.test(ch)) return { ok: false, error: '별칭: 영문 소문자로 시작하는 1–32자 (a-z 0-9 _ -)' };
  if (d.backend !== 'sim' && d.backend !== 'scpi-tcp') return { ok: false, error: '구현된 백엔드를 선택하세요' };
  const address = d.address.trim();
  if (d.backend === 'scpi-tcp' && !isLoopbackAddr(address)) return { ok: false, error: 'scpi-tcp 주소: 루프백 <ip>:<port> (예: 127.0.0.1:5025) — LAN 계측기는 미지원' };
  const vmax = limit(d.vmax);
  const imax = limit(d.imax);
  if (!vmax.ok || !imax.ok) return { ok: false, error: '한도는 0보다 큰 숫자 (비우면 프로필 기본값)' };
  return {
    ok: true,
    params: {
      ch,
      backend: d.backend,
      ...(d.backend === 'scpi-tcp' ? { address } : {}),
      ...(vmax.v !== undefined ? { vmax: vmax.v } : {}),
      ...(imax.v !== undefined ? { imax: imax.v } : {}),
    },
  };
}

export function psuConnectCli(d: PsuConnectDraft): string {
  const r = psuConnectParams(d);
  if (!r.ok) return 'loom psu connect <별칭>';
  return `loom psu connect ${r.params.ch}${r.params.address ? ` --scpi-tcp ${r.params.address}` : ''}`;
}

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const tid = <E extends HTMLElement>(e: E, id: string): E => {
  e.setAttribute('data-testid', id);
  return e;
};

export interface PsuConnectSection {
  root: HTMLElement;
  update(writable: boolean, hasRight: boolean): void;
  dispose(): void;
}

export function createPsuConnect(ctx: PanelContext): PsuConnectSection {
  const draft: PsuConnectDraft = { ch: '', backend: 'sim', address: '', vmax: '', imax: '' };
  let pending = false;
  let disposed = false;

  const root = tid(el('section', 'psu-connect'), 'psu-connect');
  root.setAttribute('aria-label', 'PSU 연결');
  root.append(
    el('strong', undefined, 'PSU 연결'),
    tid(el('span', 'psu-unbound muted', ' 별칭 미선택 — 아무 전원 장치에도 연결·명령하지 않았습니다. 연결해도 출력은 바뀌지 않습니다.'), 'psu-unbound'),
  );
  const grid = el('div', 'psu-connect-grid');
  const field = (label: string, input: HTMLElement) => {
    const l = el('label', 'psu-field');
    l.append(el('span', 'psu-setlabel', label), input);
    return l;
  };
  const text = (id: string, ph: string) => {
    const i = tid(el('input'), id);
    i.placeholder = ph;
    i.autocomplete = 'off';
    i.spellcheck = false;
    return i;
  };
  const chIn = text('psu-connect-ch', '예: bench-a');
  chIn.maxLength = 32;
  const backSel = tid(el('select'), 'psu-connect-backend');
  for (const b of PSU_BACKENDS) {
    const o = el('option', undefined, b.label);
    o.value = b.value;
    o.disabled = !b.implemented;
    backSel.append(o);
  }
  backSel.value = 'sim';
  const addrIn = text('psu-connect-address', '127.0.0.1:5025');
  const vmaxIn = text('psu-connect-vmax', '프로필 기본');
  vmaxIn.inputMode = 'decimal';
  const imaxIn = text('psu-connect-imax', '프로필 기본');
  imaxIn.inputMode = 'decimal';
  const addrL = field('SCPI 주소 (루프백)', addrIn);
  const btn = tid(el('button', 'psu-apply', '연결'), 'psu-connect-open');
  btn.type = 'button';
  grid.append(field('별칭', chIn), field('백엔드', backSel), addrL, field('전압 한도 V (낮추기만)', vmaxIn), field('전류 한도 A (낮추기만)', imaxIn), btn);
  const err = tid(el('div', 'psu-pending err'), 'psu-connect-error');
  err.setAttribute('role', 'status');
  const cli = tid(el('code', 'psu-connect-cli muted'), 'psu-connect-cli');
  const note = el(
    'div',
    'psu-connect-note muted',
    '지원: sim, scpi-tcp(루프백 소프트웨어 엔드포인트, VOLT/CURR/OUTP/MEAS만). VISA·USB·GPIB·LAN 계측기는 미구현. 데몬의 PSU 작업자는 한 번에 전원 1대만 구동합니다.',
  );

  const viewRow = el('div', 'psu-connect-grid');
  const viewIn = text('psu-view-ch', '이미 연결된 별칭');
  viewIn.maxLength = 32;
  const viewBtn = tid(el('button', undefined, '보기'), 'psu-view-open');
  viewBtn.type = 'button';
  viewBtn.title = '이미 연결된 별칭의 게이지를 엽니다 (관측만 — 연결/출력 명령 없음)';
  viewRow.append(field('열린 전원 보기', viewIn), viewBtn);
  root.append(grid, err, cli, note, viewRow);

  const read = () => {
    draft.ch = chIn.value;
    draft.backend = backSel.value === 'scpi-tcp' ? 'scpi-tcp' : 'sim';
    draft.address = addrIn.value;
    draft.vmax = vmaxIn.value;
    draft.imax = imaxIn.value;
    addrL.hidden = draft.backend !== 'scpi-tcp';
    const r = psuConnectParams(draft);
    const typed = [draft.ch, draft.address, draft.vmax, draft.imax].some((s) => s.trim() !== '');
    err.textContent = r.ok || !typed ? '' : r.error;
    cli.textContent = psuConnectCli(draft);
    viewBtn.disabled = !PSU_NAME_RE.test(viewIn.value.trim());
    ctx.scheduler.invalidate();
  };
  for (const i of [chIn, addrIn, vmaxIn, imaxIn, viewIn]) i.addEventListener('input', read);
  backSel.addEventListener('change', read);
  read();

  btn.addEventListener('click', () => {
    read();
    const r = psuConnectParams(draft);
    if (!r.ok) {
      err.textContent = r.error;
      return;
    }
    if (pending) return;
    pending = true;
    btn.disabled = true;
    ctx.act('connect', r.params).then(
      () => {
        if (disposed) return;
        ctx.ui.toast(`PSU ${r.params.ch} 연결 요청됨 (${r.params.backend}) — 출력은 변경하지 않음`);
        ctx.ui.openPanel('psu.gauge', { channel: r.params.ch });
      },
      (e: unknown) => {
        if (disposed) return;
        const code = (e as { dataCode?: string }).dataCode ?? (e as { code?: string }).code;
        err.textContent = `연결 실패${code ? ` (${code})` : ''}: ${e instanceof Error ? e.message : String(e)}`;
      },
    ).finally(() => {
      pending = false;
      ctx.scheduler.invalidate();
    });
  });
  viewBtn.addEventListener('click', () => {
    const ch = viewIn.value.trim();
    if (PSU_NAME_RE.test(ch)) ctx.ui.openPanel('psu.gauge', { channel: ch });
  });

  return {
    root,
    update(writable, hasRight) {
      const r = psuConnectParams(draft);
      btn.disabled = pending || !writable || !r.ok;
      btn.title = !hasRight ? '연결은 operate 권한 필요' : !writable ? '쓰기 잠김 (연결 끊김/STALLED)' : !r.ok ? '별칭과 백엔드 설정을 확인하세요' : 'psu.connect — 장치를 열고 조회만 합니다 (출력 변경 없음)';
    },
    dispose() {
      disposed = true;
    },
  };
}
