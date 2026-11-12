// Receive / Transmit view (research/fidelity/can.md §2): the reference's split layout — a
// per-ID Receive list on top, a Transmit list with its message editor below. DOM only; the
// data lives in RxSummary / TxList and every send goes through the panel's gated send path.

import { CLASSIC_LENGTHS, draftToParams, emptyDraft, FD_LENGTH_LIST, paramsToDraft, type TxDraft, type TxList } from './txList';
import { fmtData, fmtRxId, typeFlags, type RxRow, type RxSummary } from './rxSummary';
import type { CanSendParams } from './sendLine';

export interface RxTxHooks {
  channel: string;
  /** current trace id filter (null = all) — the Receive list honours it for display only */
  idFilter(): ReadonlySet<number> | null;
  /** send the selected transmit entry through the gated path */
  sendEntry(uid: number): void;
  /** open the chronological trace filtered to this id */
  openTrace(row: Readonly<RxRow>): void;
  /** editor content changed: params for the dry-run risk preview (null = invalid/closed) */
  draftChanged(params: CanSendParams | null): void;
  /** panel clock (ms) */
  now(): number;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
const setText = (e: HTMLElement, t: string) => {
  if (e.textContent !== t) e.textContent = t;
};
const tid = <T extends HTMLElement>(e: T, id: string): T => {
  e.dataset.testid = id;
  return e;
};
const fmtN = (n: number) => n.toLocaleString('ko-KR');

const RX_COLS = ['CAN-ID', '유형', 'Len', 'Data', '주기 ms', '횟수', '마지막 s'] as const;
const TX_COLS = ['CAN-ID', '유형', 'Len', 'Data', '주기', '횟수', '트리거', '메모'] as const;

export class RxTxView {
  readonly root: HTMLDivElement;
  private rxBody: HTMLDivElement;
  private rxWindow: HTMLDivElement;
  private rxEmpty: HTMLDivElement;
  private rxEls = new Map<string, HTMLDivElement>();
  private rxOrder: string[] = [];
  private rxVersion = -1;
  private rxFilterKey = '';
  private rxSelected: string | null = null;

  private txBody: HTMLDivElement;
  private txEmpty: HTMLDivElement;
  private txVersion = -1;
  private sendBtn: HTMLButtonElement;
  private editBtn: HTMLButtonElement;
  private delBtn: HTMLButtonElement;
  private newBtn: HTMLButtonElement;
  private txMsg: HTMLSpanElement;

  private editor: HTMLFormElement;
  private edId: HTMLInputElement;
  private edLen: HTMLSelectElement;
  private edBytes: HTMLDivElement;
  private edExt: HTMLInputElement;
  private edFd: HTMLInputElement;
  private edBrs: HTMLInputElement;
  private edComment: HTMLInputElement;
  private edErr: HTMLSpanElement;
  private edOk: HTMLButtonElement;
  private editing: number | null = null; // uid being edited, null = new
  private draft: TxDraft = emptyDraft();
  private writable = true;

  constructor(
    private readonly rx: RxSummary,
    private readonly tx: TxList,
    private readonly hooks: RxTxHooks,
  ) {
    this.root = tid(el('div', 'crx'), 'can-rxtx');

    // ---------------- Receive ----------------
    const rxSec = tid(el('section', 'crx-sec crx-rx'), 'can-rx');
    rxSec.setAttribute('aria-label', '수신 (ID별 최신)');
    const rxSide = el('div', 'crx-side', '수신');
    const rxMain = el('div', 'crx-main');
    const rxTools = el('div', 'crx-tools');
    this.rxWindow = tid(el('div', 'crx-window muted'), 'can-rx-window');
    const reset = tid(el('button', undefined, '집계 초기화'), 'can-rx-reset');
    reset.type = 'button';
    reset.title = '수신 요약만 새 집계 창으로 시작 (트레이스 링·기록은 그대로)';
    reset.addEventListener('click', () => {
      this.rx.reset(this.hooks.now());
      this.rxSelected = null;
      this.render();
    });
    rxTools.append(this.rxWindow, reset);
    const rxHead = el('div', 'crx-grid crx-rxgrid crx-head');
    rxHead.setAttribute('role', 'row');
    for (const c of RX_COLS) {
      const h = el('span', undefined, c);
      h.setAttribute('role', 'columnheader');
      rxHead.append(h);
    }
    this.rxBody = el('div', 'crx-body');
    this.rxBody.setAttribute('role', 'grid');
    this.rxBody.setAttribute('aria-label', 'CAN 수신 요약');
    this.rxBody.tabIndex = 0;
    this.rxEmpty = el('div', 'crx-empty muted', '수신 프레임 없음 (이 집계 창)');
    this.rxBody.append(this.rxEmpty);
    this.rxBody.addEventListener('click', (e) => {
      const row = (e.target as HTMLElement).closest<HTMLElement>('.crx-rxrow');
      if (row?.dataset.key) this.selectRx(row.dataset.key);
    });
    this.rxBody.addEventListener('dblclick', (e) => {
      const row = (e.target as HTMLElement).closest<HTMLElement>('.crx-rxrow');
      const r = row?.dataset.key ? this.rx.get(row.dataset.key) : undefined;
      if (r) this.hooks.openTrace(r);
    });
    this.rxBody.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const i = this.rxSelected ? this.rxOrder.indexOf(this.rxSelected) : -1;
        const j = Math.max(0, Math.min(this.rxOrder.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)));
        if (this.rxOrder[j]) this.selectRx(this.rxOrder[j]!);
      } else if (e.key === 'Enter' && this.rxSelected) {
        e.preventDefault();
        const r = this.rx.get(this.rxSelected);
        if (r) this.hooks.openTrace(r);
      }
    });
    rxMain.append(rxTools, rxHead, this.rxBody);
    rxSec.append(rxSide, rxMain);

    // ---------------- Transmit ----------------
    const txSec = tid(el('section', 'crx-sec crx-tx'), 'can-tx');
    txSec.setAttribute('aria-label', '송신 목록');
    const txSide = el('div', 'crx-side', '송신');
    const txMain = el('div', 'crx-main');
    const txTools = el('div', 'crx-tools');
    this.newBtn = tid(el('button', undefined, '+ 새 메시지'), 'can-tx-new');
    this.editBtn = tid(el('button', undefined, '편집'), 'can-tx-edit');
    this.delBtn = tid(el('button', undefined, '삭제'), 'can-tx-delete');
    this.sendBtn = tid(el('button', undefined, '보내기 (Space)'), 'can-tx-send');
    for (const b of [this.newBtn, this.editBtn, this.delBtn, this.sendBtn]) b.type = 'button';
    this.sendBtn.title = '선택한 메시지를 1회 송신 — 송신 줄과 같은 ActionGate·권한·dry-run 경로';
    this.txMsg = tid(el('span', 'crx-txmsg muted'), 'can-tx-msg');
    this.txMsg.setAttribute('aria-live', 'polite');
    const defer = tid(el('span', 'crx-defer muted', '주기 송신: 이 패널에서 미지원'), 'can-tx-periodic-deferred');
    defer.title = '주기 작업은 daemon can.periodic.* 소유 — 이 패널은 아직 연결하지 않았고 로컬 타이머로 흉내 내지 않습니다';
    txTools.append(this.newBtn, this.editBtn, this.delBtn, this.sendBtn, defer, this.txMsg);
    this.newBtn.addEventListener('click', () => this.openEditor(null));
    this.editBtn.addEventListener('click', () => {
      if (this.tx.selected !== null) this.openEditor(this.tx.selected);
    });
    this.delBtn.addEventListener('click', () => {
      if (this.tx.selected !== null) this.tx.remove(this.tx.selected);
      this.render();
    });
    this.sendBtn.addEventListener('click', () => this.sendSelected());

    const txHead = el('div', 'crx-grid crx-txgrid crx-head');
    txHead.setAttribute('role', 'row');
    for (const c of TX_COLS) {
      const h = el('span', undefined, c);
      h.setAttribute('role', 'columnheader');
      txHead.append(h);
    }
    this.txBody = el('div', 'crx-body');
    this.txBody.setAttribute('role', 'grid');
    this.txBody.setAttribute('aria-label', 'CAN 송신 목록');
    this.txBody.tabIndex = 0;
    this.txEmpty = el('div', 'crx-empty muted', '송신 메시지 없음 — “+ 새 메시지”');
    this.txBody.append(this.txEmpty);
    this.txBody.addEventListener('click', (e) => {
      const row = (e.target as HTMLElement).closest<HTMLElement>('.crx-txrow');
      if (row?.dataset.uid) this.tx.select(Number(row.dataset.uid));
      this.render();
    });
    this.txBody.addEventListener('dblclick', (e) => {
      const row = (e.target as HTMLElement).closest<HTMLElement>('.crx-txrow');
      if (row?.dataset.uid) this.openEditor(Number(row.dataset.uid));
    });
    this.txBody.addEventListener('keydown', (e) => {
      // reference: Space = Transmit > Send for the selected message. Handled here so the
      // panel-level Space (trace render pause) does not also fire.
      if (e.key === ' ') {
        e.preventDefault();
        e.stopPropagation();
        this.sendSelected();
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        this.tx.move(e.key === 'ArrowDown' ? 1 : -1);
        this.render();
      } else if (e.key === 'Enter' && this.tx.selected !== null) {
        e.preventDefault();
        this.openEditor(this.tx.selected);
      } else if (e.key === 'Delete' && this.tx.selected !== null) {
        e.preventDefault();
        this.tx.remove(this.tx.selected);
        this.render();
      }
    });

    // ---------------- editor ----------------
    this.editor = tid(el('form', 'crx-editor'), 'can-tx-editor');
    this.editor.hidden = true;
    this.editor.setAttribute('aria-label', '송신 메시지 편집');
    const row1 = el('div', 'crx-edrow');
    this.edId = tid(el('input', 'crx-edid'), 'can-tx-id');
    this.edId.setAttribute('aria-label', 'ID (hex)');
    this.edId.spellcheck = false;
    this.edId.autocomplete = 'off';
    this.edLen = tid(el('select'), 'can-tx-len');
    this.edLen.setAttribute('aria-label', '길이 (바이트)');
    this.edBytes = tid(el('div', 'crx-bytes'), 'can-tx-bytes');
    row1.append(this.labeled('ID (hex)', this.edId), this.labeled('Length', this.edLen), this.labeled('Data (hex)', this.edBytes));
    const row2 = el('div', 'crx-edrow');
    const cycle = tid(el('input', 'crx-cycle'), 'can-tx-cycle');
    cycle.value = '0';
    cycle.disabled = true;
    cycle.title = '주기 송신 미지원 (daemon can.periodic 미연결) — 0 = 수동 송신만';
    this.edExt = this.check('Extended (29bit)', 'can-tx-ext');
    this.edFd = this.check('CAN FD', 'can-tx-fd');
    this.edBrs = this.check('Bit Rate Switch', 'can-tx-brs');
    const rtr = this.check('Remote Request', 'can-tx-rtr');
    rtr.disabled = true;
    rtr.parentElement!.title = 'RTR 송신은 can.send 구조형에 없음 — 미지원';
    const types = el('fieldset', 'crx-types');
    types.append(el('legend', undefined, 'Message Type'), this.edExt.parentElement!, this.edFd.parentElement!, rtr.parentElement!, this.edBrs.parentElement!);
    row2.append(this.labeled('Cycle Time ms', cycle), types);
    const row3 = el('div', 'crx-edrow');
    this.edComment = tid(el('input', 'crx-comment'), 'can-tx-comment');
    this.edComment.maxLength = 80;
    this.edErr = tid(el('span', 'crx-ederr'), 'can-tx-error');
    this.edErr.setAttribute('aria-live', 'polite');
    this.edOk = tid(el('button', undefined, 'OK'), 'can-tx-ok');
    this.edOk.type = 'submit';
    const cancel = tid(el('button', undefined, '취소'), 'can-tx-cancel');
    cancel.type = 'button';
    row3.append(this.labeled('Comment', this.edComment), this.edErr, this.edOk, cancel);
    this.editor.append(row1, row2, row3);

    this.editor.addEventListener('submit', (e) => {
      e.preventDefault();
      this.commitEditor();
    });
    cancel.addEventListener('click', () => this.closeEditor());
    this.editor.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        this.closeEditor();
      }
      e.stopPropagation(); // typing a space in the editor never toggles pause / sends
    });
    this.edId.addEventListener('input', () => this.readEditor());
    this.edComment.addEventListener('input', () => this.readEditor());
    this.edLen.addEventListener('change', () => this.readEditor(true));
    for (const c of [this.edExt, this.edBrs]) c.addEventListener('change', () => this.readEditor());
    this.edFd.addEventListener('change', () => {
      if (!this.edFd.checked) this.edBrs.checked = false;
      this.readEditor(true);
    });

    txMain.append(txTools, txHead, this.txBody, this.editor);
    txSec.append(txSide, txMain);
    this.root.append(rxSec, txSec);
  }

  private labeled(text: string, ctl: HTMLElement): HTMLLabelElement {
    const l = el('label', 'crx-lab');
    l.append(el('span', 'muted', text), ctl);
    return l;
  }
  private check(text: string, id: string): HTMLInputElement {
    const l = el('label');
    const c = tid(el('input'), id);
    c.type = 'checkbox';
    l.append(c, ` ${text}`);
    return c;
  }

  // ---------------- receive ----------------
  private selectRx(key: string): void {
    this.rxSelected = key;
    for (const [k, e] of this.rxEls) e.setAttribute('aria-selected', String(k === key));
  }

  private renderRx(): void {
    const f = this.hooks.idFilter();
    const fk = f ? [...f].join(',') : '';
    if (this.rx.version === this.rxVersion && fk === this.rxFilterKey) return;
    this.rxVersion = this.rx.version;
    this.rxFilterKey = fk;
    const w = this.rx.window;
    const parts = [`집계 창 ${fmtN(w.frames)} 프레임 · ID ${fmtN(w.ids)}/${fmtN(w.maxIds)}`];
    if (w.untrackedFrames) parts.push(`상한 초과 미집계 ${fmtN(w.untrackedFrames)}`);
    parts.push(w.gaps ? `gap ${fmtN(w.gaps)}건 (알려진 유실 ${fmtN(w.gapFramesKnown)}${w.gapsUnknown ? ` + 미상 ${fmtN(w.gapsUnknown)}건` : ''}) → 횟수는 하한` : 'gap 없음');
    setText(this.rxWindow, parts.join(' · '));
    this.rxWindow.dataset.frames = String(w.frames);
    this.rxWindow.dataset.gaps = String(w.gaps);
    this.rxWindow.dataset.untracked = String(w.untrackedFrames);
    this.rxWindow.title = '횟수 = 이 패널이 이 집계 창에서 받은 rx 프레임 (버스 총계 아님). 자기 송신 echo 제외. 링 퇴출·일시정지와 무관';

    const list = this.rx.list().filter((r) => !f || f.has(r.id));
    const keys = list.map((r) => r.key);
    const live = new Set(keys);
    for (const [k, e] of this.rxEls) {
      if (!live.has(k)) {
        e.remove();
        this.rxEls.delete(k);
      }
    }
    const orderChanged = keys.length !== this.rxOrder.length || keys.some((k, i) => k !== this.rxOrder[i]);
    for (const r of list) {
      let e = this.rxEls.get(r.key);
      if (!e) {
        e = el('div', 'crx-grid crx-rxgrid crx-row crx-rxrow');
        e.setAttribute('role', 'row');
        e.dataset.testid = 'can-rx-row';
        e.dataset.key = r.key;
        e.dataset.id = fmtRxId(r);
        e.dataset.ext = String(r.ext);
        e.setAttribute('aria-selected', String(r.key === this.rxSelected));
        for (let i = 0; i < RX_COLS.length; i++) e.append(el('span', 'crx-td'));
        this.rxEls.set(r.key, e);
      }
      const c = e.children as HTMLCollectionOf<HTMLElement>;
      const flags = typeFlags(r);
      setText(c[0]!, fmtRxId(r));
      setText(c[1]!, flags.join(' '));
      setText(c[2]!, String(r.len));
      c[2]!.title = r.dlc !== null ? `DLC ${r.dlc}` : 'DLC 없음';
      setText(c[3]!, fmtData(r.data, r.rtr));
      c[3]!.classList.toggle('crx-changed', r.changed);
      setText(c[4]!, r.cycleMs === null ? '—' : r.cycleMs.toFixed(1));
      c[4]!.title = r.cycleMs === null ? (r.count < 2 ? '프레임 1개 — 간격 없음' : '직전 프레임과의 사이에 gap — 간격 신뢰 불가') : '이 ID 직전 수신과의 간격 (hub 단조 시각)';
      const lower = r.gapsSince > 0;
      setText(c[5]!, `${lower ? '≥' : ''}${fmtN(r.count)}`);
      c[5]!.title = lower ? `이 ID 집계 중 gap ${r.gapsSince}건 — 실제 수는 이보다 많을 수 있음` : '이 집계 창에서 받은 수';
      const rel = w.startMonoMs === null ? 0 : (r.lastMonoMs - w.startMonoMs) / 1000;
      setText(c[6]!, rel.toFixed(4));
      c[6]!.title = `마지막 수신: 집계 창 시작 기준 · seq ${r.lastSeq}`;
      e.dataset.count = String(r.count);
      e.dataset.flags = flags.join(',');
      e.dataset.data = r.data ?? '';
      e.dataset.lowerBound = String(lower);
      e.dataset.cycle = r.cycleMs === null ? '' : r.cycleMs.toFixed(3);
    }
    if (orderChanged) {
      for (const k of keys) this.rxBody.append(this.rxEls.get(k)!);
      this.rxOrder = keys;
    }
    this.rxEmpty.hidden = list.length > 0;
  }

  // ---------------- transmit ----------------
  private renderTx(): void {
    const sel = this.tx.selected;
    this.editBtn.disabled = sel === null;
    this.delBtn.disabled = sel === null;
    this.sendBtn.disabled = sel === null || !this.writable;
    this.newBtn.disabled = false;
    if (this.tx.version === this.txVersion) return;
    this.txVersion = this.tx.version;
    for (const n of [...this.txBody.querySelectorAll('.crx-txrow')]) n.remove();
    for (const e of this.tx.entries) {
      const p = e.params;
      const row = el('div', 'crx-grid crx-txgrid crx-row crx-txrow');
      row.setAttribute('role', 'row');
      row.dataset.testid = 'can-tx-row';
      row.dataset.uid = String(e.uid);
      row.dataset.id = fmtRxId(p);
      row.dataset.count = String(e.count);
      row.setAttribute('aria-selected', String(e.uid === sel));
      const flags = typeFlags({ ext: p.ext, fd: p.fd, brs: p.brs === true, esi: false, rtr: false });
      const cells = [fmtRxId(p), flags.join(' '), String(p.data.length / 2), fmtData(p.data, false), '수동', fmtN(e.count), '수동', e.comment];
      for (const t of cells) row.append(el('span', 'crx-td', t));
      if (e.lastError) row.title = `마지막 실패: ${e.lastError}`;
      else if (e.lastRequestId) row.title = `마지막 request_id ${e.lastRequestId}`;
      row.classList.toggle('crx-failed', e.lastError !== null);
      this.txBody.append(row);
    }
    this.txEmpty.hidden = this.tx.entries.length > 0;
  }

  private sendSelected(): void {
    if (this.tx.selected === null) {
      this.say('선택한 송신 메시지 없음', true);
      return;
    }
    if (!this.writable) return;
    this.hooks.sendEntry(this.tx.selected);
  }

  say(t: string, err = false): void {
    setText(this.txMsg, t);
    this.txMsg.classList.toggle('err', err);
  }

  // ---------------- editor ----------------
  private openEditor(uid: number | null): void {
    const e = uid === null ? undefined : this.tx.get(uid);
    this.editing = e ? e.uid : null;
    this.draft = e ? paramsToDraft(e.params, e.comment) : emptyDraft();
    this.edId.value = this.draft.id;
    this.edExt.checked = this.draft.ext;
    this.edFd.checked = this.draft.fd;
    this.edBrs.checked = this.draft.brs;
    this.edComment.value = this.draft.comment;
    this.fillLengths();
    this.edLen.value = String(this.draft.len);
    this.buildByteCells();
    this.editor.hidden = false;
    setText(this.edOk, e ? 'OK (수정)' : 'OK (추가)');
    this.validate();
    this.edId.focus();
  }

  private closeEditor(): void {
    this.editor.hidden = true;
    this.editing = null;
    setText(this.edErr, '');
    this.hooks.draftChanged(null);
    this.txBody.focus();
  }

  private fillLengths(): void {
    const lens = this.edFd.checked ? FD_LENGTH_LIST : CLASSIC_LENGTHS;
    const cur = Number(this.edLen.value || this.draft.len);
    this.edLen.replaceChildren(...lens.map((n) => Object.assign(el('option', undefined, String(n)), { value: String(n) })));
    // keep the length when valid in the new mode, else clamp to the largest allowed ≤ it
    const keep = lens.includes(cur) ? cur : [...lens].reverse().find((n) => n <= cur) ?? 8;
    this.edLen.value = String(keep);
  }

  private buildByteCells(): void {
    const n = Number(this.edLen.value);
    const cells: HTMLElement[] = [];
    for (let i = 0; i < n; i++) {
      const wrap = el('label', 'crx-byte');
      const inp = el('input');
      inp.dataset.testid = 'can-tx-byte';
      inp.dataset.index = String(i);
      inp.maxLength = 2;
      inp.spellcheck = false;
      inp.autocomplete = 'off';
      inp.value = this.draft.bytes[i] ?? '00';
      inp.setAttribute('aria-label', `Data ${i}`);
      inp.addEventListener('input', () => {
        this.readEditor();
        if (inp.value.length === 2) (wrap.nextElementSibling?.querySelector('input') as HTMLInputElement | null)?.focus();
      });
      wrap.append(inp, el('span', 'crx-byteidx muted', String(i)));
      cells.push(wrap);
    }
    this.edBytes.replaceChildren(...cells);
  }

  private readEditor(rebuild = false): void {
    const bytes = [...this.edBytes.querySelectorAll<HTMLInputElement>('input')].map((i) => i.value);
    // keep bytes typed beyond the current length (shrinking then growing restores them)
    for (let i = 0; i < bytes.length; i++) this.draft.bytes[i] = bytes[i]!;
    if (rebuild) this.fillLengths();
    this.draft = {
      id: this.edId.value,
      ext: this.edExt.checked,
      fd: this.edFd.checked,
      brs: this.edBrs.checked,
      len: Number(this.edLen.value),
      bytes: this.draft.bytes,
      comment: this.edComment.value,
    };
    if (rebuild) this.buildByteCells();
    this.validate();
  }

  private validate(): void {
    this.edBrs.disabled = !this.edFd.checked;
    const r = draftToParams(this.draft, this.hooks.channel);
    if (r.ok) {
      setText(this.edErr, `→ ${r.canonical}`);
      this.edErr.classList.remove('err');
      this.edOk.disabled = false;
      this.hooks.draftChanged(r.params);
    } else {
      setText(this.edErr, r.error);
      this.edErr.classList.add('err');
      this.edOk.disabled = true;
      this.hooks.draftChanged(null);
    }
  }

  private commitEditor(): void {
    this.readEditor();
    const r = draftToParams(this.draft, this.hooks.channel);
    if (!r.ok) return;
    const res = this.tx.upsert(this.editing, r.params, r.canonical, this.draft.comment);
    if (!res.ok) {
      setText(this.edErr, res.error);
      this.edErr.classList.add('err');
      return;
    }
    this.tx.select(res.uid);
    this.closeEditor();
    this.render();
  }

  /** test/integration hook: the editor's current draft */
  get currentDraft(): Readonly<TxDraft> {
    return this.draft;
  }

  setWritable(w: boolean): void {
    this.writable = w;
    this.sendBtn.disabled = this.tx.selected === null || !w;
    this.sendBtn.title = w ? '선택한 메시지를 1회 송신 — 송신 줄과 같은 ActionGate·권한·dry-run 경로' : '송신 불가 (operate 권한 없음 또는 쓰기 잠김)';
  }

  render(): void {
    this.renderRx();
    this.renderTx();
  }
}
