// uds.console tool drawer — ECU·DID / 명령·시퀀스 / 감시 / 프로젝트 (selected web reference:
// work/reports/uds-read-did/app/{catalog,workflow,project}.js). The 서비스 tool lives in consolePanel
// (it edits the composer directly).
//
// Rules carried over from the reference and the real backend:
//  - catalogue / command / project edits are local settings only — they never connect or send;
//  - [실행] of a saved command is one explicit uds.request through the ActionGate;
//  - a sequence runs only on an explicit [실행]: one step at a time, each awaits its final daemon
//    result, the run stops on the first failure, on [중지], on the write lock / STOP latch / missing
//    right, or when the link is reported down — no retries, no implicit re-send;
//  - 감시 shows confirmed values from real responses; there is no polling RPC, so nothing polls.
import { isFinal, type PanelContext, type RowLike } from '../../sdk';
import { btn, el } from '../../workbench/eth/bench';
import { resultSymbol, spacedHex, type TargetView } from './udsModel';
import { normalizeDid, normalizeRaw, sanitizePrefs, STANDARD_DIDS, type SavedCmd, type SavedSeq, type UdsPrefs } from './udsPrefs';

export interface SendResult {
  ok: boolean;
  requestId: string | null;
  msg: string;
}

export interface ToolsApi {
  ctx: PanelContext;
  prefs(): UdsPrefs;
  setPrefs(p: UdsPrefs): void;
  target(): string | null;
  writable(): boolean;
  canOperate(): boolean;
  link(t: string | null): string;
  rows(): readonly RowLike[];
  rowNo(id: string): number | null;
  reveal(id: string): void;
  fillDid(did: string): void;
  fillRaw(hex: string): void;
  /** composer request as canonical hex, null if invalid */
  current(): string | null;
  /** one explicit uds.request; resolves after the daemon's final result */
  send(target: string, hex: string): Promise<SendResult>;
  openCfg(presetId?: string): void;
  selectTarget(t: string | null): void;
  targets(): TargetView[];
  invalidate(): void;
}

type ToolKey = 'catalog' | 'workflow' | 'watch' | 'project';
export interface ToolMounts {
  catalog: HTMLElement;
  workflow: HTMLElement;
  watch: HTMLElement;
  project: HTMLElement;
  isOpen(id: ToolKey): boolean;
}

export const PROJECT_FORMAT = 'loom.uds.project';
export const MAX_PROJECT_BYTES = 512 * 1024;

export interface ProjectParse {
  prefs: UdsPrefs | null;
  error: string | null;
  /** per list: [kept, dropped] */
  report: Record<'presets' | 'dids' | 'cmds' | 'seqs', [number, number]>;
}

/** Project file → validated settings. Whole file checked first; nothing is applied, connected or sent here. */
export function parseProjectText(text: string): ProjectParse {
  const empty: ProjectParse['report'] = { presets: [0, 0], dids: [0, 0], cmds: [0, 0], seqs: [0, 0] };
  if (text.length > MAX_PROJECT_BYTES) return { prefs: null, error: `파일이 너무 큼 (최대 ${MAX_PROJECT_BYTES / 1024} KB)`, report: empty };
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    return { prefs: null, error: `JSON 구문 오류: ${e instanceof Error ? e.message : String(e)}`, report: empty };
  }
  const o = (typeof raw === 'object' && raw !== null ? raw : null) as Record<string, unknown> | null;
  if (!o || o.format !== PROJECT_FORMAT) return { prefs: null, error: `형식이 다름 — format "${PROJECT_FORMAT}" 필요`, report: empty };
  if (o.version !== 1) return { prefs: null, error: `지원하지 않는 버전: ${String(o.version)} (1만 지원)`, report: empty };
  const body = (typeof o.prefs === 'object' && o.prefs !== null ? o.prefs : null) as Record<string, unknown> | null;
  if (!body) return { prefs: null, error: 'prefs 객체 없음', report: empty };
  for (const k of ['presets', 'dids', 'cmds', 'seqs'] as const) {
    if (body[k] !== undefined && !Array.isArray(body[k])) return { prefs: null, error: `${k}: 배열이 아님`, report: empty };
  }
  if (!Array.isArray(body.presets) || body.presets.length === 0) return { prefs: null, error: 'presets: 최소 1개 필요', report: empty };
  const p = sanitizePrefs(body);
  const n = (k: 'presets' | 'dids' | 'cmds' | 'seqs') => (Array.isArray(body[k]) ? (body[k] as unknown[]).length : 0);
  const report: ProjectParse['report'] = {
    presets: [p.presets.length, n('presets') - p.presets.length],
    dids: [p.dids.length, Math.max(0, n('dids') - p.dids.length)],
    cmds: [p.cmds.length, n('cmds') - p.cmds.length],
    seqs: [p.seqs.length, n('seqs') - p.seqs.length],
  };
  if (report.presets[1] > 0 && report.presets[0] === 0) return { prefs: null, error: 'presets: 유효한 프로필 없음', report };
  return { prefs: p, error: null, report };
}

export function projectJson(p: UdsPrefs): string {
  return JSON.stringify({ format: PROJECT_FORMAT, version: 1, savedAt: new Date().toISOString(), prefs: { presets: p.presets, dids: p.dids, cmds: p.cmds, seqs: p.seqs } }, null, 2);
}

const newId = (pfx: string) => `${pfx}-${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
const cmdKind = (hex: string) => (/^22[0-9A-F]{4}$/.test(hex) ? 'DID' : 'RAW');
const sbtn = (text: string, title?: string, cls = 'btn sm') => btn(text, cls, title);
const lnk = (text: string, title?: string) => btn(text, 'lnk', title);

interface StepRun {
  cmdId: string;
  name: string;
  hex: string;
  status: 'wait' | 'run' | 'ok' | 'fail' | 'skip';
  requestId: string | null;
  msg: string;
}
interface SeqRun {
  seqId: string;
  name: string;
  target: string;
  steps: StepRun[];
  stop: boolean;
  done: boolean;
  result: string;
}

export function createTools(api: ToolsApi, m: ToolMounts): { render(force: boolean): void; dispose(): void; runSeq(id: string): Promise<void>; stopSeq(): void } {
  let disposed = false;
  const keys: Record<ToolKey, string> = { catalog: '', workflow: '', watch: '', project: '' };

  // ---------------- ECU·DID ----------------
  const ct = el('div', 'ct');
  m.catalog.append(ct);
  let didDraft = { did: '', name: '', note: '' };
  let didMsg = '';
  const renderCatalog = () => {
    const p = api.prefs();
    const tgts = api.targets();
    const key = JSON.stringify([p.presets, p.dids, tgts.map((t) => [t.target, t.link, t.backend, t.addr]), api.target(), didMsg]);
    if (key === keys.catalog) return;
    keys.catalog = key;
    const top = el('p', 'ct-top', '로컬 정의 — 연결 상태와 값은 daemon(uds.state·응답)에서만 옵니다. 편집은 연결·전송하지 않습니다.');
    // ECU table: presets + targets connected from elsewhere
    const ecuTab = el('table', 'ct-tab');
    const eh = el('tr');
    for (const h of ['ECU', '프로필', '엔티티', '주소', '링크 (daemon)', '']) eh.append(el('th', undefined, h));
    const eb = el('tbody');
    const names = new Set<string>();
    const ecuRow = (ecu: string, label: string, kind: string, addr: string, presetId: string | null) => {
      names.add(ecu);
      const tv = tgts.find((t) => t.target === ecu);
      const tr = el('tr', ecu === api.target() ? 'cur' : undefined);
      tr.dataset.ecu = ecu;
      const acts = el('td', 'ct-act');
      const sel = sbtn('선택', '이 ECU를 대상으로 선택 (연결하지 않음)');
      sel.addEventListener('click', () => api.selectTarget(ecu));
      acts.append(sel);
      if (presetId) {
        const ed = sbtn('편집', '연결 설정에서 편집');
        ed.addEventListener('click', () => api.openCfg(presetId));
        acts.append(ed);
      }
      tr.append(el('td', 'v', ecu), el('td', undefined, label), el('td', 'v', kind), el('td', 'v', addr), el('td', `v${tv ? '' : ' na'}`, tv ? tv.link : '미보고'), acts);
      eb.append(tr);
    };
    for (const pr of p.presets) ecuRow(pr.ecu, pr.label, pr.sim ? 'SIM bench' : 'DoIP', pr.sim ? `${pr.testerLa}→${pr.la}` : `${pr.host || '—'}:${pr.port} ${pr.testerLa}→${pr.la}`, pr.id);
    for (const t of tgts) if (!names.has(t.target)) ecuRow(t.target, '— (외부 연결)', t.backend, t.addr ?? '—', null);
    const eth = el('thead');
    eth.append(eh);
    ecuTab.append(eth, eb);
    const ecuScroll = el('div', 'ct-scroll');
    ecuScroll.append(ecuTab);
    const addEcu = sbtn('새 프로필…', '연결 설정에서 새 DoIP 프로필 작성');
    addEcu.addEventListener('click', () => api.openCfg());

    // DID catalogue
    const didTab = el('table', 'ct-tab');
    const dh = el('tr');
    for (const h of ['DID', '이름', '설명', '']) dh.append(el('th', undefined, h));
    const db = el('tbody');
    for (const d of p.dids) {
      const tr = el('tr');
      tr.dataset.did = d.did;
      const std = STANDARD_DIDS.some((s) => s.did === d.did);
      const nm = el('td');
      nm.append(d.name, ' ', el('span', `ct-b${std ? ' pre' : ''}`, std ? '표준' : '사용자'));
      const acts = el('td', 'ct-act');
      const load = sbtn('편집기로', 'DID 읽기 편집기에 채우기 (전송 없음)');
      load.addEventListener('click', () => api.fillDid(d.did));
      const del = sbtn('✕', `${d.did} 삭제`);
      del.setAttribute('aria-label', `${d.did} 삭제`);
      del.addEventListener('click', () => api.setPrefs({ ...api.prefs(), dids: api.prefs().dids.filter((x) => x.did !== d.did) }));
      acts.append(load, del);
      tr.append(el('td', 'v', d.did), nm, el('td', undefined, d.note), acts);
      db.append(tr);
    }
    const dth = el('thead');
    dth.append(dh);
    didTab.append(dth, db);
    const didScroll = el('div', 'ct-scroll');
    didScroll.append(didTab);
    const form = el('div', 'ct-form');
    const grid = el('div', 'ct-grid');
    const fld = (k: 'did' | 'name' | 'note', label: string, ph: string) => {
      const w = el('label', 'cf');
      const i = el('input', 'fld');
      i.value = didDraft[k];
      i.placeholder = ph;
      i.spellcheck = false;
      i.dataset.k = k;
      i.addEventListener('input', () => {
        didDraft = { ...didDraft, [k]: i.value };
      });
      w.append(el('span', 'cf-l', label), i);
      return w;
    };
    grid.append(fld('did', 'DID (4자리 hex)', 'F190'), fld('name', '이름', 'VIN'), fld('note', '설명', '선택'));
    const msg = el('p', `ct-msg${didMsg.startsWith('✓') ? '' : ' err'}`, didMsg);
    const add = sbtn('추가', 'DID 카탈로그에 추가 (이름만 — 값은 ECU 응답에서)', 'btn sm primary');
    add.addEventListener('click', () => {
      const v = normalizeDid(didDraft.did);
      const name = didDraft.name.trim();
      if (!v.did) didMsg = v.err ?? 'DID 오류';
      else if (!name || name.length > 80) didMsg = '이름 1–80자 필요';
      else if (didDraft.note.length > 200) didMsg = '설명 최대 200자';
      else if (api.prefs().dids.some((d) => d.did === v.did)) didMsg = `${v.did}: 이미 있음`;
      else {
        api.setPrefs({ ...api.prefs(), dids: [...api.prefs().dids, { did: v.did, name, note: didDraft.note.trim() }] });
        didDraft = { did: '', name: '', note: '' };
        didMsg = `✓ ${v.did} 추가됨`;
      }
      keys.catalog = '';
      renderCatalog();
    });
    const reset = sbtn('표준 목록 복원', 'ISO 14229-1 표준 DID 이름 목록으로 되돌리기 (사용자 DID 삭제)');
    reset.addEventListener('click', () => {
      api.setPrefs({ ...api.prefs(), dids: STANDARD_DIDS.map((d) => ({ ...d })) });
      didMsg = '✓ 표준 목록 복원';
    });
    const foot = el('div', 'ct-foot');
    foot.append(add, reset);
    form.append(el('b', undefined, 'DID 추가'), grid, msg, foot);
    const ecuH = el('h3', undefined, 'ECU');
    const didH = el('h3', undefined, 'DID 카탈로그');
    ct.replaceChildren(top, ecuH, ecuScroll, addEcu, didH, didScroll, form, el('p', 'ct-note', 'DID 이름은 ISO 14229-1 Annex C 표준 식별자 또는 사용자 정의입니다. 응답 값 해석은 daemon(decoded)만 사용합니다.'));
  };

  // ---------------- 명령·시퀀스 ----------------
  const wf = el('div', 'wf');
  m.workflow.append(wf);
  let cmdEdit: SavedCmd | null = null;
  let seqEdit: SavedSeq | null = null;
  let wfNote = '';
  let run: SeqRun | null = null;
  let lastCmdRun: { id: string; res: SendResult | null } | null = null;
  const refreshWf = () => {
    keys.workflow = '';
    if (m.isOpen('workflow')) renderWorkflow();
    api.invalidate();
  };
  const running = () => run !== null && !run.done;

  async function runCmd(c: SavedCmd) {
    const t = api.target();
    if (!t || !api.writable() || running() || (lastCmdRun !== null && lastCmdRun.res === null)) return; // one command send at a time
    lastCmdRun = { id: c.id, res: null };
    refreshWf();
    const res = await api.send(t, c.hex);
    if (disposed) return;
    lastCmdRun = { id: c.id, res };
    refreshWf();
  }

  async function runSeq(id: string) {
    const s = api.prefs().seqs.find((x) => x.id === id);
    const t = api.target();
    if (!s || running()) return;
    if (!t) {
      wfNote = '대상 없음 — ECU를 선택하세요';
      return refreshWf();
    }
    const cmds = api.prefs().cmds;
    const steps = s.steps.map((cid) => cmds.find((c) => c.id === cid));
    if (!steps.length || steps.some((c) => !c)) {
      wfNote = `"${s.name}": 단계가 없거나 삭제된 명령을 참조 — 실행하지 않음`;
      return refreshWf();
    }
    const r: SeqRun = {
      seqId: s.id, name: s.name, target: t, stop: false, done: false, result: '실행 중',
      steps: steps.map((c) => ({ cmdId: c!.id, name: c!.name, hex: c!.hex, status: 'wait', requestId: null, msg: '' })),
    };
    run = r;
    wfNote = '';
    refreshWf();
    for (const step of r.steps) {
      const block = r.stop
        ? '중지됨 (사용자)'
        : !api.canOperate()
          ? '쓰기 잠금 또는 권한 없음'
          : api.ctx.stopLatched
            ? 'STOP 래치'
            : !['up', '?'].includes(api.link(r.target))
              ? `링크 ${api.link(r.target)}`
              : null;
      if (block) {
        r.result = `중지 — ${block}`;
        break;
      }
      step.status = 'run';
      refreshWf();
      const res = await api.send(r.target, step.hex);
      if (disposed) return;
      step.requestId = res.requestId;
      step.msg = res.msg;
      if (!res.ok) {
        step.status = 'fail';
        r.result = `실패로 중지 — ${step.name}: ${res.msg}`;
        break;
      }
      step.status = 'ok';
      refreshWf();
    }
    for (const st of r.steps) if (st.status === 'wait') st.status = 'skip';
    if (r.steps.every((st) => st.status === 'ok')) r.result = '완료 — 모든 단계 성공';
    r.done = true;
    refreshWf();
  }
  const stopSeq = () => {
    if (run && !run.done) {
      run.stop = true;
      refreshWf();
    }
  };

  const renderWorkflow = () => {
    const p = api.prefs();
    const w = api.writable();
    const cur = api.current();
    const key = JSON.stringify([p.cmds, p.seqs, w, cur, cmdEdit, seqEdit, wfNote, run, lastCmdRun, api.target()]);
    if (key === keys.workflow) return;
    keys.workflow = key;
    const bar = el('div', 'wf-bar');
    const saveCur = sbtn('현재 요청 저장', '요청 편집기의 현재 DID/Raw를 새 명령 초안으로 (전송 없음)');
    saveCur.disabled = !cur;
    saveCur.addEventListener('click', () => {
      if (!cur) return;
      cmdEdit = { id: newId('c'), name: cmdKind(cur) === 'DID' ? `Read ${cur.slice(2)}` : `Raw ${spacedHex(cur.slice(0, 8))}`, hex: cur };
      refreshWf();
    });
    const addCmd = sbtn('새 명령');
    addCmd.addEventListener('click', () => {
      cmdEdit = { id: newId('c'), name: '', hex: '' };
      refreshWf();
    });
    bar.append(saveCur, addCmd);
    const out: HTMLElement[] = [bar, el('p', 'wf-hint', '[편집기로]는 입력만 채우며 전송하지 않습니다. [실행]은 현재 대상으로 1회 전송 (ActionGate·daemon 정책 경유, 재시도 없음).')];
    if (wfNote) out.push(el('div', 'wf-note bad', wfNote));

    // command editor
    if (cmdEdit) {
      const ed = el('div', 'wf-edit');
      const g = el('div', 'wf-grid');
      const nl = el('label');
      const ni = el('input', 'fld');
      ni.value = cmdEdit.name;
      ni.setAttribute('aria-label', '명령 이름');
      nl.append('이름', ni);
      const hl = el('label', 'wide');
      const hi = el('textarea', 'fld');
      hi.value = cmdEdit.hex ? spacedHex(cmdEdit.hex) : '';
      hi.placeholder = '22 F1 90';
      hi.setAttribute('aria-label', '명령 PDU (hex)');
      hl.append('UDS PDU (SID 포함 hex)', hi);
      g.append(nl, hl);
      const pv = el('div', 'wf-prev');
      const err = el('div', 'wf-err');
      const sync = () => {
        const v = normalizeRaw(hi.value);
        pv.replaceChildren(el('span', 'k', '전송될 PDU '), el('code', undefined, v.hex ? spacedHex(v.hex) : '—'));
        pv.className = `wf-prev${v.hex ? '' : ' bad'}`;
        err.textContent = hi.value.trim() && !v.hex ? (v.err ?? '') : '';
      };
      hi.addEventListener('input', sync);
      sync();
      const eb = el('div', 'wf-eb');
      const save = sbtn('저장', undefined, 'btn sm primary');
      save.addEventListener('click', () => {
        const v = normalizeRaw(hi.value);
        const name = ni.value.trim();
        if (!name || name.length > 80) {
          err.textContent = '이름 1–80자 필요';
          return;
        }
        if (!v.hex) {
          err.textContent = v.err ?? 'PDU 오류';
          return;
        }
        const c: SavedCmd = { id: cmdEdit!.id, name, hex: v.hex };
        const pp = api.prefs();
        const exists = pp.cmds.some((x) => x.id === c.id);
        if (!exists && pp.cmds.length >= 200) {
          err.textContent = '명령은 최대 200개';
          return;
        }
        cmdEdit = null;
        api.setPrefs({ ...pp, cmds: exists ? pp.cmds.map((x) => (x.id === c.id ? c : x)) : [...pp.cmds, c] });
        refreshWf();
      });
      const cancel = sbtn('취소');
      cancel.addEventListener('click', () => {
        cmdEdit = null;
        refreshWf();
      });
      eb.append(save, cancel);
      ed.append(el('div', 'wf-eh', ''), g, pv, err, eb);
      (ed.firstChild as HTMLElement).append(el('b', undefined, p.cmds.some((x) => x.id === cmdEdit!.id) ? '명령 편집' : '새 명령'));
      out.push(ed);
    }

    // command list
    out.push(el('h3', undefined, `저장 명령 (${p.cmds.length})`));
    const ul = el('ul', 'wf-list');
    if (!p.cmds.length) ul.append(el('li', 'wf-empty', '저장된 명령 없음 — [현재 요청 저장] 또는 [새 명령]'));
    for (const c of p.cmds) {
      const li = el('li', `wf-item${cmdEdit?.id === c.id ? ' editing' : ''}`);
      li.dataset.cmd = c.id;
      const main = el('div', 'wf-main');
      const k = cmdKind(c.hex);
      main.append(el('span', `wf-kind${k === 'RAW' ? ' k-raw' : ''}`, k), el('span', 'wf-name', c.name), el('span', 'wf-pdu', spacedHex(c.hex)));
      if (lastCmdRun?.id === c.id) {
        const r = lastCmdRun.res;
        const st = el('span', `wf-st${r ? (r.ok ? ' r-passed' : ' r-failed') : ' r-running'}`, r ? (r.ok ? '✓' : `✗ ${r.msg}`) : '전송 중…');
        if (r?.requestId) {
          const a = lnk(`#${api.rowNo(r.requestId) ?? '?'}`, '기록에서 보기');
          a.addEventListener('click', () => api.reveal(r.requestId!));
          main.append(a);
        }
        main.append(st);
      }
      const acts = el('div', 'wf-acts');
      const load = sbtn('편집기로', '요청 편집기에 채우기 (전송 없음)');
      load.addEventListener('click', () => (k === 'DID' ? api.fillDid(c.hex.slice(2)) : api.fillRaw(c.hex)));
      const go = sbtn('실행', `현재 대상(${api.target() ?? '없음'})으로 1회 전송`, 'btn sm go');
      go.dataset.run = c.id;
      go.disabled = !w || !api.target() || running() || (lastCmdRun?.id === c.id && lastCmdRun.res === null);
      go.addEventListener('click', () => void runCmd(c));
      const ed = sbtn('✎', '편집');
      ed.setAttribute('aria-label', `${c.name} 편집`);
      ed.addEventListener('click', () => {
        cmdEdit = { ...c };
        refreshWf();
      });
      const used = p.seqs.filter((s) => s.steps.includes(c.id)).map((s) => s.name);
      const del = sbtn('✕', used.length ? `시퀀스에서 사용 중: ${used.join(', ')}` : '삭제');
      del.setAttribute('aria-label', `${c.name} 삭제`);
      del.disabled = used.length > 0;
      del.addEventListener('click', () => api.setPrefs({ ...api.prefs(), cmds: api.prefs().cmds.filter((x) => x.id !== c.id) }));
      acts.append(load, go, ed, del);
      li.append(main, acts);
      ul.append(li);
    }
    out.push(ul);

    // sequences
    const sh = el('div', 'wf-bar');
    const addSeq = sbtn('새 시퀀스');
    addSeq.disabled = !p.cmds.length;
    addSeq.addEventListener('click', () => {
      seqEdit = { id: newId('s'), name: '', steps: [] };
      refreshWf();
    });
    sh.append(el('h3', undefined, `시퀀스 (${p.seqs.length})`), el('span', 'grow'), addSeq);
    out.push(sh, el('p', 'wf-hint', '단계는 하나씩 최종 응답을 기다린 뒤 진행 · 실패·[중지]·쓰기 잠금·STOP·링크 끊김이면 즉시 중지 · 재시도 없음 · 실행 대상은 시작 시 고정'));
    if (seqEdit) {
      const se = seqEdit;
      const ed = el('div', 'wf-edit');
      const g = el('div', 'wf-grid');
      const nl = el('label', 'wide');
      const ni = el('input', 'fld');
      ni.value = se.name;
      ni.setAttribute('aria-label', '시퀀스 이름');
      ni.addEventListener('input', () => {
        se.name = ni.value;
      });
      nl.append('이름', ni);
      g.append(nl);
      const tbl = el('table', 'wf-st-tbl');
      const th = el('tr');
      for (const h of ['#', '명령', '']) th.append(el('th', undefined, h));
      tbl.append(th);
      se.steps.forEach((cid, i) => {
        const tr = el('tr');
        const sel = el('select', 'fld sm');
        for (const c of p.cmds) sel.append(Object.assign(el('option', undefined, `${c.name} · ${spacedHex(c.hex)}`), { value: c.id }));
        sel.value = cid;
        sel.addEventListener('change', () => {
          se.steps[i] = sel.value;
        });
        const acts = el('td', 'acts');
        const up = sbtn('↑', '위로');
        up.disabled = i === 0;
        up.addEventListener('click', () => {
          [se.steps[i - 1], se.steps[i]] = [se.steps[i]!, se.steps[i - 1]!];
          refreshWf();
        });
        const rm = sbtn('✕', '단계 삭제');
        rm.addEventListener('click', () => {
          se.steps.splice(i, 1);
          refreshWf();
        });
        acts.append(up, rm);
        const tdSel = el('td');
        tdSel.append(sel);
        tr.append(el('td', 'n', String(i + 1)), tdSel, acts);
        tbl.append(tr);
      });
      const steps = el('div', 'wf-steps');
      steps.append(tbl);
      const err = el('div', 'wf-err');
      const eb = el('div', 'wf-eb');
      const addStep = sbtn('단계 추가');
      addStep.disabled = se.steps.length >= 64;
      addStep.addEventListener('click', () => {
        se.steps.push(p.cmds[0]!.id);
        refreshWf();
      });
      const save = sbtn('저장', undefined, 'btn sm primary');
      save.addEventListener('click', () => {
        const name = se.name.trim();
        if (!name || name.length > 80) {
          err.textContent = '이름 1–80자 필요';
          return;
        }
        if (!se.steps.length) {
          err.textContent = '단계 1개 이상 필요';
          return;
        }
        const pp = api.prefs();
        const s: SavedSeq = { id: se.id, name, steps: se.steps.filter((cid) => pp.cmds.some((c) => c.id === cid)) };
        const exists = pp.seqs.some((x) => x.id === s.id);
        seqEdit = null;
        api.setPrefs({ ...pp, seqs: exists ? pp.seqs.map((x) => (x.id === s.id ? s : x)) : [...pp.seqs, s] });
        refreshWf();
      });
      const cancel = sbtn('취소');
      cancel.addEventListener('click', () => {
        seqEdit = null;
        refreshWf();
      });
      eb.append(addStep, el('span', 'grow'), save, cancel);
      ed.append(g, steps, err, eb);
      out.push(ed);
    }
    const sl = el('ul', 'wf-list');
    if (!p.seqs.length) sl.append(el('li', 'wf-empty', '시퀀스 없음'));
    for (const s of p.seqs) {
      const on = run?.seqId === s.id && !run.done;
      const li = el('li', `wf-item${run?.seqId === s.id ? ' sel' : ''}`);
      li.dataset.seq = s.id;
      const main = el('div', 'wf-main');
      main.append(el('span', 'wf-name', s.name), el('span', 'wf-chip', `${s.steps.length}단계`));
      const acts = el('div', 'wf-acts');
      const go = on ? sbtn('중지', '현재 단계 결과 후 중지 (진행 중 요청 취소는 [취소])', 'btn sm danger') : sbtn('실행', `현재 대상(${api.target() ?? '없음'})에 고정해 실행`, 'btn sm go');
      go.dataset.run = s.id;
      go.disabled = on ? false : !w || !api.target() || running();
      go.addEventListener('click', () => (on ? stopSeq() : void runSeq(s.id)));
      const ed = sbtn('✎', '편집');
      ed.setAttribute('aria-label', `${s.name} 편집`);
      ed.disabled = on;
      ed.addEventListener('click', () => {
        seqEdit = { ...s, steps: [...s.steps] };
        refreshWf();
      });
      const del = sbtn('✕', '삭제');
      del.setAttribute('aria-label', `${s.name} 삭제`);
      del.disabled = on;
      del.addEventListener('click', () => api.setPrefs({ ...api.prefs(), seqs: api.prefs().seqs.filter((x) => x.id !== s.id) }));
      acts.append(go, ed, del);
      li.append(main, acts);
      sl.append(li);
    }
    out.push(sl);
    if (run) {
      const rv = el('div', 'wf-run');
      rv.setAttribute('data-testid', 'uds-seq-run');
      const rh = el('div', 'wf-rh');
      rh.append(el('b', undefined, run.name), el('span', 'wf-tgt', `대상 ${run.target} (시작 시 고정)`), el('span', `wf-st ${run.done ? (run.steps.every((x) => x.status === 'ok') ? 'r-passed' : 'r-failed') : 'r-running'}`, run.result));
      const ol = el('ol', 'wf-steplist');
      for (const st of run.steps) {
        const li = el('li', st.status === 'run' ? 'cur' : undefined);
        li.dataset.status = st.status;
        const mark = { wait: '·', run: '⏳', ok: '✓', fail: '✗', skip: '—' }[st.status];
        li.append(`${mark} ${st.name} · ${spacedHex(st.hex)}`);
        if (st.requestId) {
          const a = lnk(` #${api.rowNo(st.requestId) ?? '?'}`, '기록에서 보기');
          a.addEventListener('click', () => api.reveal(st.requestId!));
          li.append(a);
        }
        if (st.status === 'fail' && st.msg) li.append(el('span', 'r-bad', ` ${st.msg}`));
        ol.append(li);
      }
      rv.append(rh, ol);
      out.push(rv);
    }
    wf.replaceChildren(...out);
  };

  // ---------------- 감시 ----------------
  const wt = el('div', 'wt');
  m.watch.append(wt);
  const renderWatch = () => {
    const t = api.target();
    const rows = api.rows();
    const key = `${t}|${rows.length}|${rows.map((r) => r.updates).reduce((a, b) => a + b, 0)}`;
    if (key === keys.watch) return;
    keys.watch = key;
    type H = { wall: number; did: string; name: string; value: string; unit: string; rid: string };
    const hist: H[] = [];
    const last = new Map<string, H & { n: number }>();
    for (const r of rows) {
      if ((r.payload.target ?? r.channel) !== t || !isFinal(r.state) || r.state === 'failed' || r.state === 'outcome_unknown') continue;
      if (resultSymbol(r).cls === 'res-nrc' || !Array.isArray(r.payload.decoded)) continue;
      for (const d of r.payload.decoded as Record<string, unknown>[]) {
        const did = typeof d?.did === 'string' ? d.did : null;
        if (!did) continue;
        const value = typeof d.value === 'string' ? d.value : JSON.stringify(d.value ?? '');
        const h: H = { wall: r.firstWallMs, did, name: typeof d.name === 'string' ? d.name : '', value, unit: typeof d.unit === 'string' ? d.unit : '', rid: r.requestId };
        const prev = last.get(did);
        if (!prev || prev.value !== value) hist.push(h);
        last.set(did, { ...h, n: (prev?.n ?? 0) + 1 });
      }
    }
    const note = el('p', 'wf-hint', '확정 값 감시 — 실제 응답(daemon decoded)에서만. daemon에 주기 폴링 RPC가 없어 자동 읽기·폴링은 하지 않습니다. 값을 갱신하려면 [읽기]·명령·시퀀스로 명시 전송.');
    const sc = el('div', 'wt-scroll');
    const tb = el('table', 'wt-tbl');
    const th = el('tr');
    for (const h of ['DID', '이름', '값', '읽기', '마지막']) th.append(el('th', undefined, h));
    tb.append(th);
    for (const [did, v] of last) {
      const tr = el('tr');
      tr.dataset.did = did;
      const val = el('td');
      val.dataset.c = 'value';
      val.append(el('b', undefined, v.value), v.unit ? el('span', 'u', ` ${v.unit}`) : '');
      const src = el('td');
      src.dataset.c = 'src';
      const a = lnk(`#${api.rowNo(v.rid) ?? '?'}`, '출처 트랜잭션');
      a.addEventListener('click', () => api.reveal(v.rid));
      src.append(a);
      tr.append(el('td', 'r', did), el('td', undefined, v.name), val, el('td', 'r', String(v.n)), src);
      tb.append(tr);
    }
    if (!last.size) {
      const tr = el('tr');
      const td = el('td', 'wt-na', `${t ?? '대상'}: 확정 값 없음`);
      td.colSpan = 5;
      tr.append(td);
      tb.append(tr);
    }
    sc.append(tb);
    const ch = el('table', 'wt-tbl');
    const chh = el('tr');
    for (const h of ['시각', 'DID', '값', '출처']) chh.append(el('th', undefined, h));
    ch.append(chh);
    for (const h of hist.slice(-200).reverse()) {
      const tr = el('tr');
      const a = lnk(`#${api.rowNo(h.rid) ?? '?'}`);
      a.addEventListener('click', () => api.reveal(h.rid));
      const td = el('td');
      td.append(a);
      tr.append(el('td', 'r', new Date(h.wall).toLocaleTimeString()), el('td', 'r', h.did), el('td', undefined, `${h.value}${h.unit ? ` ${h.unit}` : ''}`), td);
      ch.append(tr);
    }
    const sc2 = el('div', 'wt-scroll');
    sc2.append(ch);
    wt.replaceChildren(note, el('h3', undefined, `현재 값 · ${t ?? '—'}`), sc, el('h3', undefined, '변경 기록 (최근 200)'), sc2);
  };

  // ---------------- 프로젝트 ----------------
  const pj = el('div', 'pj');
  m.project.append(pj);
  let pending: ProjectParse | null = null;
  let pjMsg: { text: string; cls: string } = { text: '', cls: '' };
  const fileIn = el('input');
  fileIn.type = 'file';
  fileIn.accept = 'application/json,.json';
  fileIn.hidden = true;
  fileIn.setAttribute('aria-label', '프로젝트 파일');
  const importText = (text: string, name: string) => {
    pending = parseProjectText(text);
    pjMsg = pending.error ? { text: `${name}: ${pending.error} — 적용하지 않음`, cls: 'bad' } : { text: `${name}: 검증됨 — [적용]해야 교체됩니다`, cls: 'ok' };
    if (pending.error) pending = null;
    keys.project = '';
    renderProject();
  };
  fileIn.addEventListener('change', () => {
    const f = fileIn.files?.[0];
    fileIn.value = '';
    if (!f) return;
    if (f.size > MAX_PROJECT_BYTES) {
      pending = null;
      pjMsg = { text: `${f.name}: 파일이 너무 큼 (최대 ${MAX_PROJECT_BYTES / 1024} KB)`, cls: 'bad' };
      keys.project = '';
      return renderProject();
    }
    void f.text().then((t) => !disposed && importText(t, f.name), () => {
      pjMsg = { text: `${f.name}: 읽기 실패`, cls: 'bad' };
      keys.project = '';
      renderProject();
    });
  });
  const renderProject = () => {
    const key = JSON.stringify([pending?.report, pjMsg, running(), api.prefs().presets.length, api.prefs().cmds.length]);
    if (key === keys.project) return;
    keys.project = key;
    const exp = el('section', 'pj-sec');
    const eb = sbtn('프로젝트 JSON 내보내기', 'ECU 프로필·DID 카탈로그·저장 명령·시퀀스를 한 파일로');
    eb.addEventListener('click', () => {
      try {
        if (typeof URL.createObjectURL !== 'function') throw new Error('download unavailable');
        const url = URL.createObjectURL(new Blob([projectJson(api.prefs())], { type: 'application/json' }));
        const a = el('a');
        a.href = url;
        a.download = `uds-project-${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1_000);
        pjMsg = { text: '내보냄', cls: 'ok' };
      } catch {
        pjMsg = { text: '내보내기 실패: 이 환경에서 다운로드를 만들 수 없음', cls: 'bad' };
      }
      keys.project = '';
      renderProject();
    });
    const er = el('div', 'pj-row');
    er.append(eb);
    exp.append(el('span', 'pj-h', '내보내기'), el('p', 'pj-hint', 'ECU 프로필, DID 카탈로그, 저장 명령·시퀀스 정의를 한 파일로 저장합니다. 트랜잭션 기록·확정 값·연결 상태는 포함하지 않습니다.'), er);
    const imp = el('section', 'pj-sec');
    const pick = sbtn('파일 선택…', '프로젝트 JSON 가져오기 (검증 후 미리보기, 자동 적용 안 함)');
    pick.addEventListener('click', () => fileIn.click());
    const ir = el('div', 'pj-row');
    ir.append(pick, fileIn);
    const prev = el('div', 'pj-prev');
    if (pending?.prefs) {
      const tb = el('table', 'pj-tbl');
      const hr = el('tr');
      for (const h of ['항목', '가져옴', '버림 (유효하지 않음)']) hr.append(el('th', undefined, h));
      const thead = el('thead');
      thead.append(hr);
      const tbody = el('tbody');
      const labels: Record<string, string> = { presets: 'ECU 프로필', dids: 'DID', cmds: '명령', seqs: '시퀀스' };
      for (const [k, [kept, dropped]] of Object.entries(pending.report)) {
        const tr = el('tr', dropped ? 'keep' : undefined);
        tr.append(el('th', undefined, labels[k] ?? k), el('td', undefined, String(kept)), el('td', undefined, String(dropped)));
        tbody.append(tr);
      }
      tb.append(thead, tbody);
      const row = el('div', 'pj-row');
      const apply = sbtn('적용', '현재 정의를 교체 — 연결·전송·시퀀스 시작 없음', 'btn sm primary');
      apply.disabled = running();
      apply.title = running() ? '시퀀스 실행 중 — 적용 거부 (자동 중지 없음)' : apply.title;
      apply.addEventListener('click', () => {
        if (!pending?.prefs || running()) return;
        const cur = api.prefs();
        api.setPrefs({ ...cur, presets: pending.prefs.presets, dids: pending.prefs.dids, cmds: pending.prefs.cmds, seqs: pending.prefs.seqs });
        pending = null;
        pjMsg = { text: '적용됨 — 연결·전송은 시작하지 않았습니다', cls: 'ok' };
        keys.project = '';
        renderProject();
      });
      const cancel = sbtn('취소');
      cancel.addEventListener('click', () => {
        pending = null;
        pjMsg = { text: '', cls: '' };
        keys.project = '';
        renderProject();
      });
      row.append(apply, cancel);
      prev.append(tb, row);
    }
    const msg = el('div', `pj-msg${pjMsg.cls ? ` ${pjMsg.cls}` : ''}`, pjMsg.text);
    msg.setAttribute('role', 'status');
    imp.append(el('span', 'pj-h', '가져오기'), el('p', 'pj-hint', `파일 전체를 먼저 검증하고 미리보기를 보여줍니다. [적용]해야 교체됩니다. 시퀀스 실행 중에는 거부합니다. 적용해도 연결·전송·시퀀스는 시작되지 않고, 기록과 확정 값은 유지됩니다. 최대 ${MAX_PROJECT_BYTES / 1024} KB.`), ir, prev, msg);
    pj.replaceChildren(exp, imp);
  };

  return {
    render(force) {
      if (force) for (const k of Object.keys(keys) as ToolKey[]) keys[k] = '';
      if (m.isOpen('catalog')) renderCatalog();
      if (m.isOpen('workflow')) renderWorkflow();
      if (m.isOpen('watch')) renderWatch();
      if (m.isOpen('project')) renderProject();
    },
    dispose() {
      disposed = true;
      if (run && !run.done) run.stop = true;
    },
    runSeq,
    stopSeq,
  };
}
