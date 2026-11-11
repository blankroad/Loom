// shell.test — test execution screen, M1b minimal (_xteam/ui-test.md, test/plan §5,
// contract 07). Shell-owned (not a panel): it reads TestSessionStore directly and
// calls test.* RPCs through the ActionGate (rights + STALLED lock).
//
// Honesty rules: the UI shows the server's verdict records only — proposed / effective /
// override (with "by") and rejected PASS attempts (code + message) side by side. Without
// any test.evidence_checked (CORE verifier is P2) the evidence column says so instead of
// pretending. User events in the live evidence pane are marked "판정 제외".

import { useEffect, useState } from 'react';
import type { LoomEvent } from '../../proto/provisional';
import type { ActionSpec } from '../../sdk/manifest';
import { useStoreVersion } from '../../store/hooks';
import {
  EVIDENCE_ICON,
  VERDICT_ICON,
  type AskView,
  type EvidenceStatus,
  type StepView,
  type Verdict,
  type VerdictView,
} from '../../store/testSession';
import { useServices } from '../ShellContext';

export const LIVE_EVIDENCE_ROWS = 2_000;
const SHOWN_ROWS = 60;

const spec = (id: string, rpc: string, right: ActionSpec['right'], confirm: ActionSpec['confirm'] = 'none', label?: string): ActionSpec => ({
  id,
  rpc,
  class: 'request',
  right,
  confirm,
  label: label ?? id,
});
export const TEST_ACTIONS = {
  pause: spec('pause', 'test.pause', 'operate'),
  resume: spec('resume', 'test.resume', 'operate'),
  skip: spec('skip', 'test.skip', 'operate'),
  retry: spec('retry', 'test.retry', 'operate'),
  answer: spec('answer', 'test.answer', 'operate'),
  check: spec('check', 'test.check', 'operate'),
  // contract 07 §7.4: override = viewer approve rights only
  override: spec('override', 'test.verdict', 'approve', 'none', '판정 수정'),
  end: spec('end', 'test.end', 'operate', 'card', '테스트 중단 (finally 실행 여부는 서버가 묻는다)'),
} as const;

function stepIcon(st: StepView): string {
  const v = st.accepted?.effective;
  if (v) return VERDICT_ICON[v];
  if (st.status === 'running') return '▶';
  return '·';
}

function verdictText(v: VerdictView | null): string {
  if (!v) return '—';
  if (v.rejected) return `${v.proposed?.toUpperCase() ?? '?'} 거부: ${v.rejected.code}`;
  const eff = v.effective ?? '—';
  if (v.override) return `${eff} (수정 by ${v.by}, 이전 ${v.previous?.effective ?? '—'})`;
  return v.proposed && v.proposed !== v.effective ? `제안 ${v.proposed} → 확정 ${eff}` : `${eff} (by ${v.by})`;
}

function evidenceCell(st: StepView, verifierSeen: boolean): string {
  if (st.evidence.length === 0) return verifierSeen ? '—' : '검증기 P2';
  const icons = st.evidence.map((e) => EVIDENCE_ICON[e.status as EvidenceStatus] ?? e.status).join(' · ');
  return st.evidence.some((e) => e.gapInWindow) ? `${icons} · 창 안 gap ⚠` : icons;
}

function elapsedText(st: StepView, nowWall: number): string {
  if (!st.startedWallMs) return '';
  const end = st.endedWallMs ?? nowWall;
  const e = Math.max(0, (end - st.startedWallMs) / 1000).toFixed(1);
  return st.timeoutMs ? `${e}/${(st.timeoutMs / 1000).toFixed(0)}s` : `${e}s`;
}

function useNowTick(active: boolean, ms = 500): number {
  const [t, setT] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const h = setInterval(() => setT(Date.now()), ms);
    return () => clearInterval(h);
  }, [active, ms]);
  return t;
}

/** live evidence for the current step: its domains, ring of LIVE_EVIDENCE_ROWS (C2).
 * State is keyed by step so switching steps never shows the previous step's rows. */
function useLiveEvidence(step: StepView | null): { rows: LoomEvent[]; dropped: number } {
  const { net } = useServices();
  const key = step ? `${step.id}|${step.domains.map((d) => `${d.domain}/${d.channel ?? ''}`).join(',')}` : '';
  const [st, setSt] = useState<{ key: string; rows: LoomEvent[]; dropped: number }>({ key: '', rows: [], dropped: 0 });
  const domainsKey = step ? step.domains.map((d) => `${d.domain}/${d.channel ?? ''}`).join(',') : '';
  useEffect(() => {
    if (!key || !domainsKey) return;
    const parts = domainsKey.split(',').map((x) => x.split('/'));
    const domains = [...new Set(parts.map((p) => p[0] as string))];
    const channels = parts.map((p) => p[1]).filter((c): c is string => !!c);
    const sub = net.subscribe(
      { domains, ...(channels.length ? { channels } : {}) },
      { snapshot: false },
      {
        onBatch: (bt) =>
          setSt((prev) => {
            const base = prev.key === key ? prev : { key, rows: [], dropped: 0 };
            const rows = [...base.rows, ...bt.events];
            const over = rows.length - LIVE_EVIDENCE_ROWS;
            // oldest leave the screen (the journal keeps them)
            return over > 0 ? { key, rows: rows.slice(over), dropped: base.dropped + over } : { key, rows, dropped: base.dropped };
          }),
      },
    );
    return () => sub.close();
  }, [key, domainsKey, net]);
  return st.key === key ? { rows: st.rows, dropped: st.dropped } : { rows: [], dropped: 0 };
}

function AskDock({ asks, onAnswer }: { asks: AskView[]; onAnswer(a: AskView, value: string): void }) {
  const now = useNowTick(asks.length > 0, 1000);
  const [text, setText] = useState<Record<string, string>>({});
  if (asks.length === 0) return null;
  return (
    // docked bottom-left with a teal frame: never mistaken for the centred amber approval card
    <aside className="ask-dock" aria-label="에이전트 질문" data-testid="ask-dock">
      {asks.map((a) => {
        // atMs is wall-clock ms (TestSessionStore uses Date.now for asks)
        const left = a.timeoutMs ? Math.max(0, Math.round((a.timeoutMs - (now - a.atMs)) / 1000)) : null;
        return (
          <div key={a.id} className="ask-item" data-ask-id={a.id}>
            <header>
              에이전트 질문{left !== null ? ` · 남은 ${left}s` : ''}
            </header>
            <p>{a.question}</p>
            <div className="ask-actions">
              {a.options.map((o) => (
                <button key={o} type="button" onClick={() => onAnswer(a, o)}>
                  {o}
                </button>
              ))}
              <input
                aria-label="직접 입력"
                value={text[a.id] ?? ''}
                onChange={(e) => setText({ ...text, [a.id]: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (text[a.id] ?? '').trim()) onAnswer(a, (text[a.id] ?? '').trim());
                }}
              />
            </div>
          </div>
        );
      })}
    </aside>
  );
}

interface ReasonForm {
  kind: 'override' | 'skip';
  stepId: string;
}

function PlanModal({ onClose }: { onClose(): void }) {
  const { net, testSession, shell, toasts } = useServices();
  const [plan, setPlan] = useState<Record<string, unknown> | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    net
      .call<Record<string, unknown>>('test.plan.show', { session: testSession.sessionId })
      .then(setPlan)
      .catch((e: unknown) => setErr(e instanceof Error ? e.message : String(e)));
  }, [net, testSession]);
  const steps = Array.isArray(plan?.steps) ? (plan.steps as Record<string, unknown>[]) : [];
  const approval = plan?.approval as { approval_id?: string } | undefined;
  return (
    <div className="confirm-overlay" onKeyDown={(e) => e.key === 'Escape' && onClose()}>
      <div className="confirm-card plan-modal" role="dialog" aria-label="계획 검토" data-testid="plan-modal">
        <header>계획 검토 {typeof plan?.plan_hash === 'string' ? `· ${plan.plan_hash.slice(0, 12)}` : ''}</header>
        {err && <p className="warn">계획을 불러오지 못함: {err}</p>}
        <table className="plan-table">
          <thead>
            <tr>
              <th>원문</th>
              <th>해석 단계</th>
              <th>술어</th>
              <th>등급</th>
            </tr>
          </thead>
          <tbody>
            {steps.map((s, i) => (
              <tr key={i}>
                <td>{String(s.source_text ?? '')}</td>
                <td className="mono">{String(s.interpreted ?? s.command ?? '')}</td>
                <td>{String(s.predicate_text ?? '')}</td>
                <td>
                  <span className={`risk risk-${String(s.risk ?? '')}`}>{String(s.risk ?? 'R?')}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {/* F1: approval exists only when policy turned it on */}
        <div className="actions">
          <button type="button" onClick={onClose}>
            닫기 (Esc)
          </button>
          {approval?.approval_id && (
            <button
              type="button"
              className="approve"
              disabled={!shell.can('approve')}
              onClick={() =>
                net
                  .call('core.approval.respond', { approval_id: approval.approval_id, decision: 'approve' })
                  .then(onClose)
                  .catch((e: unknown) => toasts.push(`계획 승인 실패: ${e instanceof Error ? e.message : String(e)}`))
              }
            >
              계획 승인
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export function TestScreen() {
  const { testSession: ts, shell, gate, toasts, uiHooks } = useServices();
  useStoreVersion(ts);
  useStoreVersion(shell);
  const [selId, setSelId] = useState<string | null>(null);
  const [form, setForm] = useState<ReasonForm | null>(null);
  const [reason, setReason] = useState('');
  const [ovVerdict, setOvVerdict] = useState<Verdict>('fail');
  const [planOpen, setPlanOpen] = useState(false);
  const running = ts.state === 'RUNNING';
  const nowWall = useNowTick(running);
  const current = ts.currentStep;
  const selected = (selId ? ts.step(selId) : null) ?? current ?? ts.steps[0] ?? null;
  const live = useLiveEvidence(current);
  const session = ts.sessionId;

  const run = (a: ActionSpec, params: object) =>
    gate.run(a, { session, ...params }).catch((e: unknown) => toasts.push(`${a.label ?? a.id} 실패: ${e instanceof Error ? e.message : String(e)}`));

  const asks = [...ts.asks.values()];

  if (!session) {
    return (
      <section className="test-screen empty" data-testid="test-screen">
        <p className="muted">진행 중인 테스트 세션 없음 — 에이전트가 `loom test start`로 시작하면 여기에 나타납니다.</p>
      </section>
    );
  }

  const shownLive = live.rows.slice(-SHOWN_ROWS);
  return (
    <section className="test-screen" data-testid="test-screen" aria-label="테스트 실행">
      <header className="test-head">
        <strong>TEST {ts.title || ts.tcId || ''}</strong> · 세션 {session}
        {ts.actor ? ` · 실행 ${ts.actor}` : ''} · <span className={`env-badge env-${ts.env ?? shell.env ?? 'sim'}`}>[{(ts.env ?? shell.env ?? '?').toUpperCase()}]</span>{' '}
        · <span data-testid="test-state">{ts.state}</span>
        {ts.result && (
          <span className={`test-result res-${ts.result}`} data-testid="test-result">
            {' '}
            결과 {ts.result}
          </span>
        )}
        {current && ` · S${current.n}/${ts.steps.length}`}
        <span className="spacer" />
        <button type="button" onClick={() => setPlanOpen(true)}>
          계획 보기
        </button>
        {ts.state === 'PAUSED' ? (
          <button type="button" disabled={!shell.can('operate')} onClick={() => void run(TEST_ACTIONS.resume, {})}>
            ▶ 계속
          </button>
        ) : (
          <button type="button" disabled={!shell.can('operate') || !ts.active} onClick={() => void run(TEST_ACTIONS.pause, {})}>
            ⏸ 일시정지
          </button>
        )}
        <button type="button" className="danger" disabled={!shell.can('operate') || !ts.active} onClick={() => void run(TEST_ACTIONS.end, { abort: true })}>
          ■ 중단
        </button>
      </header>
      {ts.state === 'PAUSED' && <div className="banner banner-warn">일시정지 — 스케줄링만 멈춤. 장비 시간·진행 중 요청은 계속 흐릅니다.</div>}
      {shell.recordHealth === 'failed' && (
        <div className="banner banner-err" data-testid="test-rec-failed">
          REC FAILED — PASS 판정 불가 (기록·감사 불능)
        </div>
      )}
      {shell.latch.on && (
        <div className="banner banner-err" data-testid="test-stop-banner">
          STOP — finally는 자동 실행하지 않습니다(A15). 안전 상태 복구는 위 STOP 패널의{' '}
          <button
            type="button"
            onClick={() => (document.querySelector('.recover-btn') as HTMLButtonElement | null)?.focus()}
          >
            복구 송신으로 →
          </button>
        </div>
      )}

      <div className="test-body">
        <table className="test-steps" data-testid="test-steps">
          <thead>
            <tr>
              <th />
              <th>#</th>
              <th>단계</th>
              <th>경과/한도</th>
              <th>증거 (Loom 검증)</th>
              <th>판정</th>
            </tr>
          </thead>
          <tbody>
            {ts.steps.map((st) => (
              <tr
                key={st.id}
                data-step-id={st.id}
                data-status={st.status}
                data-verdict={st.accepted?.effective ?? ''}
                className={`${st.id === current?.id ? 'current' : ''} ${st.id === selected?.id ? 'selected' : ''}`}
                onClick={() => setSelId(st.id)}
              >
                <td className="icon">{stepIcon(st)}</td>
                <td>{st.n}</td>
                <td>{st.title}</td>
                <td className="mono">{elapsedText(st, nowWall)}</td>
                <td>{evidenceCell(st, ts.verifierSeen)}</td>
                <td className={st.verdict?.rejected ? 'res-nrc' : ''}>{verdictText(st.verdict)}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {selected && (
          <div className="test-detail" data-testid="test-detail">
            <h3>
              S{selected.n} {selected.title}
            </h3>
            {selected.expectText && <p>기대: {selected.expectText}</p>}
            <div className="test-evidence">
              {selected.evidence.length === 0 && (
                <p className="muted">{ts.verifierSeen ? '인용된 증거 없음' : '증거 검증기는 CORE P2 — 아직 검증 결과 이벤트가 없습니다'}</p>
              )}
              {selected.evidence.map((e) => (
                <div key={e.id} className={`ev-row ev-${e.status}`} data-evidence-status={e.status}>
                  <span>{EVIDENCE_ICON[e.status as EvidenceStatus] ?? e.status}</span> <span className="mono">{e.ref}</span>{' '}
                  {e.observed && <span>관측 {e.observed}</span>} {e.cause && <span className="warn">({e.cause})</span>}
                  {e.gapInWindow && <span className="warn"> 창 안 gap</span>}{' '}
                  <button type="button" onClick={() => uiHooks.reveal(e.ref)}>
                    증거 열기
                  </button>
                </div>
              ))}
            </div>
            <div className="test-verdicts">
              {selected.history.map((v, i) => (
                <div key={i} className={v.rejected ? 'verdict-rejected' : 'verdict'} data-verdict-row={v.rejected ? 'rejected' : v.override ? 'override' : 'accepted'}>
                  {v.rejected ? (
                    <>
                      ⛔ {v.proposed?.toUpperCase()} 거부 — {v.rejected.code}: {v.rejected.message}
                    </>
                  ) : (
                    <>
                      원판정 {v.proposed ?? '—'} / 확정 {v.effective ?? '—'}
                      {v.override && ` / 수정 by ${v.by} (이전 ${v.previous?.effective ?? '—'} by ${v.previous?.by ?? '?'})`}
                      {!v.override && ` · by ${v.by}`}
                      {v.reason && ` · 사유: ${v.reason}`}
                    </>
                  )}
                  {v.refs.map((r) => (
                    <button key={r} type="button" className="ref-btn" onClick={() => uiHooks.reveal(r)}>
                      증거 열기 {r}
                    </button>
                  ))}
                </div>
              ))}
            </div>
            <div className="test-actions">
              <button
                type="button"
                disabled={!shell.can('approve')}
                title={shell.can('approve') ? '' : '판정 수정은 approve 자격 필요 (계약 07 §7.4)'}
                onClick={() => {
                  setForm({ kind: 'override', stepId: selected.id });
                  setReason('');
                }}
              >
                판정 수정…
              </button>
              <button type="button" disabled={!shell.can('operate') || selected.status === 'ended'} onClick={() => setForm({ kind: 'skip', stepId: selected.id })}>
                건너뛰기…
              </button>
              <button type="button" disabled={!shell.can('operate')} onClick={() => void run(TEST_ACTIONS.retry, { step: selected.id })}>
                재시도
              </button>
            </div>
            {form && form.stepId === selected.id && (
              <form
                className="reason-form"
                data-testid="reason-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (!reason.trim()) return;
                  if (form.kind === 'override') void run(TEST_ACTIONS.override, { step: form.stepId, verdict: ovVerdict, override: true, reason: reason.trim() });
                  else void run(TEST_ACTIONS.skip, { step: form.stepId, reason: reason.trim() });
                  setForm(null);
                }}
              >
                {form.kind === 'override' && (
                  <select aria-label="수정 판정" value={ovVerdict} onChange={(e) => setOvVerdict(e.target.value as Verdict)}>
                    {(['pass', 'fail', 'inconclusive', 'blocked', 'skipped'] as const).map((v) => (
                      <option key={v} value={v}>
                        {v}
                      </option>
                    ))}
                  </select>
                )}
                <input aria-label="사유 (필수)" placeholder="사유 (필수)" value={reason} onChange={(e) => setReason(e.target.value)} />
                <button type="submit" disabled={!reason.trim()}>
                  {form.kind === 'override' ? '판정 수정' : '건너뛰기'}
                </button>
                <button type="button" onClick={() => setForm(null)}>
                  취소
                </button>
              </form>
            )}
          </div>
        )}

        <div className="test-live" data-testid="test-live">
          <header>
            실시간 증거 {current ? `(S${current.n})` : ''}
            {live.dropped > 0 && <span className="muted"> · ⋯ {live.dropped}행 화면 생략 (기록은 저널)</span>}
          </header>
          {!current && <p className="muted">진행 중인 단계 없음</p>}
          {current && current.domains.length === 0 && <p className="muted">이 단계의 도메인 정보가 없어 증거 스트림을 고를 수 없음</p>}
          <ul>
            {shownLive.map((ev) => (
              <li key={`${ev.epoch}:${ev.seq}`} data-origin={ev.origin} className={ev.origin === 'user' ? 'user-ev' : ''}>
                <span className="mono">{ev.seq}</span> <span className={`org org-${ev.origin === 'agent' ? 'agt' : ev.origin === 'user' ? 'usr' : ev.origin === 'script' ? 'scr' : 'dev'}`}>
                  {ev.origin === 'agent' ? 'AGT' : ev.origin === 'user' ? 'USR' : ev.origin === 'script' ? 'SCR' : 'DEV'}
                </span>{' '}
                {ev.domain}
                {ev.channel ? ` ${ev.channel}` : ''} {ev.kind}
                {ev.origin === 'user' && <span className="warn"> [판정 제외]</span>}
              </li>
            ))}
          </ul>
        </div>
      </div>

      <AskDock asks={asks} onAnswer={(a, value) => void run(TEST_ACTIONS.answer, { ask_id: a.id, value })} />
      {planOpen && <PlanModal onClose={() => setPlanOpen(false)} />}
    </section>
  );
}

/** Footer step band (M1b simple timeline): one segment per step, current highlighted. */
export function StepBand() {
  const { testSession: ts } = useServices();
  useStoreVersion(ts);
  if (!ts.sessionId || ts.steps.length === 0) return null;
  return (
    <div className="step-band" data-testid="step-band" aria-label="단계 띠">
      {ts.steps.map((st) => (
        <span
          key={st.id}
          className={`band-seg band-${st.status}${st.id === ts.currentStepId ? ' band-current' : ''} band-v-${st.accepted?.effective ?? 'none'}`}
          title={`S${st.n} ${st.title} — ${st.accepted?.effective ?? st.status}`}
        >
          {stepIcon(st)}
          {st.n}
        </span>
      ))}
    </div>
  );
}

/** Top-bar session badge → opens the test layout. */
export function TestBadge() {
  const { testSession: ts } = useServices();
  useStoreVersion(ts);
  if (!ts.sessionId) return null;
  const cur = ts.currentStep;
  return (
    <button type="button" className="test-badge" data-testid="test-badge" data-prio="3" onClick={() => (location.hash = '#/l/test')}>
      TEST {ts.title || ts.sessionId} {ts.result ?? ts.state}
      {cur ? ` S${cur.n}/${ts.steps.length}` : ''}
    </button>
  );
}
