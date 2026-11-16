// CAN periodic jobs table (CAN-2) — daemon jobs of this channel, all owners.
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B3-VIEW.
//
// · Read-only for jobs this panel did not start/take over: the only action there is "넘겨받기"
//   (explicit takeover → confirm card in the panel). "정지" for own jobs is a safety-reducing action.
// · STOP latch: banner; the panel disables new starts (stopping stays possible).
// · textContent only; no timers (render is driven by the panel frame, gated by JobTable.version).
//
// SCAFFOLD (Opus): fields/types/signatures. Function bodies are Sonnet batch-3 work.

import { fmtId } from './sendLine';
import { jobStateText, type JobRow, type JobTable } from './periodicJobs';

export interface PeriodicHooks {
  /** this panel started or took over the job (session) */
  isMine(job: string): boolean;
  stop(job: string): void;
  takeover(job: string): void;
  /** one can.periodic.list read */
  refresh(): void;
}

/** table columns (header text) */
export const JOB_COLS = ['작업', 'CAN-ID', '주기 ms', '소유자', '상태', '보냄/놓침', '리스 남음', ''] as const;

export class PeriodicView {
  readonly root: HTMLDivElement;
  private readonly body: HTMLDivElement;
  private readonly empty: HTMLDivElement;
  private readonly status: HTMLSpanElement;
  private readonly banner: HTMLDivElement;
  private readonly refreshBtn: HTMLButtonElement;
  /** job → row element */
  private readonly els = new Map<string, HTMLDivElement>();
  private renderedVersion = -1;
  private writable = true;
  private latched = false;

  constructor(
    private readonly table: JobTable,
    private readonly hooks: PeriodicHooks,
  ) {
    const mk = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
      const e = document.createElement(tag);
      if (cls) e.className = cls;
      if (text !== undefined) e.textContent = text;
      return e;
    };
    this.root = mk('div', 'crx-periodic');
    this.root.dataset.testid = 'can-periodic';
    this.root.setAttribute('role', 'region');
    this.root.setAttribute('aria-label', '주기 송신 작업');
    const head = mk('div', 'crx-periodic-head');
    const title = mk('span', 'crx-periodic-title', '주기 송신 작업 (데몬 · 모든 소유자)');
    this.refreshBtn = mk('button', undefined, '목록 새로고침');
    this.refreshBtn.type = 'button';
    this.refreshBtn.dataset.testid = 'can-periodic-refresh';
    this.refreshBtn.addEventListener('click', () => this.hooks.refresh());
    this.status = mk('span', 'crx-periodic-status');
    this.status.dataset.testid = 'can-periodic-status';
    this.status.setAttribute('aria-live', 'polite');
    head.append(title, this.refreshBtn, this.status);
    this.banner = mk('div', 'crx-periodic-stop', 'STOP — 주기 송신 정지됨 (새 시작 불가 · 정지는 가능)');
    this.banner.dataset.testid = 'can-periodic-stop';
    this.banner.hidden = true;
    const header = mk('div', 'crx-job-head');
    for (const c of JOB_COLS) header.append(mk('span', undefined, c));
    this.body = mk('div', 'crx-job-body');
    this.body.dataset.testid = 'can-periodic-body';
    this.empty = mk('div', 'crx-job-empty muted');
    this.empty.dataset.testid = 'can-periodic-empty';
    this.root.append(head, this.banner, header, this.body, this.empty);
  }

  /** status line next to the refresh button (read result / action errors) */
  setStatus(text: string, err: boolean): void {
    if (this.status.textContent !== text) this.status.textContent = text;
    this.status.classList.toggle('err', err);
  }

  /** STOP latch banner */
  setLatched(latched: boolean): void {
    this.latched = latched;
    this.banner.hidden = !latched;
  }

  /** operate right and not STALLED: enables takeover (stop is never disabled) */
  setWritable(w: boolean): void {
    if (w !== this.writable) {
      this.writable = w;
      this.renderedVersion = -1;
    }
  }

  /** redraw when the table version (or writable/mine state) changed; `force` redraws regardless */
  render(force?: boolean): void {
    if (!force && this.renderedVersion === this.table.version) return;
    const rows = this.table.list();
    this.empty.hidden = rows.length > 0;
    const text = this.table.loaded ? '이 채널에 데몬 주기 송신 작업 없음' : '목록을 아직 읽지 않음 — 새로고침';
    if (this.empty.textContent !== text) this.empty.textContent = text;
    this.body.replaceChildren();
    this.els.clear();
    for (const r of rows) {
      const e = document.createElement('div');
      e.className = 'crx-job';
      this.fillRow(e, r);
      this.body.append(e);
      this.els.set(r.job, e);
    }
    this.renderedVersion = this.table.version;
  }

  /** one row's cells/attributes/action button (textContent only) */
  private fillRow(e: HTMLDivElement, r: Readonly<JobRow>): void {
    const mine = this.hooks.isMine(r.job);
    e.dataset.job = r.job;
    e.dataset.state = r.state;
    e.dataset.mine = String(mine);
    e.title = mine ? '이 보기가 시작했거나 넘겨받은 작업' : '다른 소유자의 작업 — 읽기 전용 (넘겨받기 = 확인 카드)';
    const texts = [
      r.job,
      fmtId(r.id, r.ext),
      r.periodMs === null ? '?' : String(r.periodMs),
      `${r.origin} ${r.owner}`,
      jobStateText(r),
      `${r.sent ?? '?'} / ${r.missed ?? '?'}`,
      r.leaseLeftMs === null ? '—' : `${(r.leaseLeftMs / 1000).toFixed(1)} s`,
    ];
    for (const t of texts) {
      const c = document.createElement('span');
      c.textContent = t;
      e.append(c);
    }
    const act = document.createElement('span');
    if (r.state !== 'stopped') {
      const b = document.createElement('button');
      b.type = 'button';
      if (mine) {
        b.textContent = '정지';
        b.dataset.testid = 'can-job-stop';
        b.title = '이 작업 정지 (안전 방향 — 확인 없이)';
        b.addEventListener('click', () => this.hooks.stop(r.job));
      } else {
        b.textContent = '넘겨받기';
        b.dataset.testid = 'can-job-takeover';
        b.disabled = !this.writable;
        b.title = '넘겨받으면 이 보기가 이 작업을 조작합니다 (확인 카드)';
        b.addEventListener('click', () => this.hooks.takeover(r.job));
      }
      act.append(b);
    }
    e.append(act);
  }
}
