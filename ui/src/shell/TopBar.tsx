// Top bar (plan §5.1.1). Drop order when narrow is handled by CSS (`data-prio`):
// env > STOP > connection/LIVE > approvals > REC > viewers > agent activity.
// The env badge is never hidden (B4).

import type { Env } from '../proto/provisional';
import { useStoreVersion } from '../store/hooks';
import type { StopPhase } from '../store/stopController';
import { useServices } from './ShellContext';
import { TestBadge } from './test/TestScreen';
import { ServiceNav } from './ServiceNav';
import type { Route } from './routes';

const ENV_LABEL: Record<Env, string> = { sim: 'SIM', bench: 'BENCH', vehicle: 'VEHICLE' };

export function EnvBadge({ env }: { env: Env | null }) {
  return (
    <span className={`env-badge env-${env ?? 'unknown'}`} data-env={env ?? 'unknown'} data-prio="0">
      [{env ? ENV_LABEL[env] : 'ENV ?'}]
    </span>
  );
}

export function LiveBadge() {
  const { shell } = useServices();
  useStoreVersion(shell);
  const s = shell.status;
  // browser-side lag (oldest received-but-undrawn event); nothing pending → idle time instead
  const lagPart = s.idleMs !== null && s.lagMs === 0 ? `idle ${(s.idleMs / 1000).toFixed(1)}s` : `lag ${Math.round(s.lagMs)}ms`;
  const text =
    s.badge === 'STALLED'
      ? `STALLED${s.sinceHeartbeatMs !== null ? ` ${(s.sinceHeartbeatMs / 1000).toFixed(1)}s` : ''}`
      : s.badge === 'LAG'
        ? `LAG ${Math.round(s.lagMs)}ms`
        : `${s.badge} ${lagPart}`;
  const tip = [
    `브라우저 lag ${Math.round(s.lagMs)} ms${s.lagFromReceipt ? ' (수신 시점 기준 — 시계 사용 불가)' : ''}`,
    `서버 배치 lag ${Math.round(s.serverLagMs)} ms`,
    s.clockErrMs !== null ? `시계 ±${s.clockErrMs.toFixed(1)} ms` : '시계 미동기',
  ].join(' · ');
  return (
    <span
      className={`live-badge live-${s.badge.toLowerCase()}${s.lagSevere ? ' lag-severe' : ''}`}
      data-testid="live-badge"
      data-badge={s.badge}
      data-prio="2"
      title={tip}
    >
      <span className={`hb-dot${s.warnDot ? ' hb-warn' : ''}`} aria-hidden="true">
        ●
      </span>{' '}
      {text}
      {s.clockUncertain && s.badge !== 'STALLED' && (
        <span className="clock-uncertain" data-testid="clock-uncertain">
          {' '}
          · 동기 불확실
        </span>
      )}
    </span>
  );
}

const STOP_TEXT: Record<StopPhase, string> = {
  idle: '■ STOP 전체',
  requesting: 'STOP 요청 중…',
  locked: 'STOP — 잠김',
  unconfirmed: 'STOP 확인 안 됨 — 재시도',
};

/**
 * Always visible, never confirmed, enabled for EVERY right including watch
 * (contract v1.1: STOP and emergency OFF are safety-direction actions; only the
 * release needs operate).
 */
export function StopButton() {
  const { stop } = useServices();
  useStoreVersion(stop);
  const phase = stop.phase;
  return (
    <button
      type="button"
      className={`stop-btn stop-${phase}`}
      data-testid="stop-button"
      data-phase={phase}
      data-prio="1"
      title={stop.lastVia === 'http' ? 'WS 끊김 — HTTP 대체 경로로 보냄' : 'agent·script 쓰기와 잡을 모두 잠근다 (Ctrl-] x)'}
      onClick={() => {
        if (phase !== 'locked') void stop.requestStop();
      }}
    >
      {STOP_TEXT[phase]}
    </button>
  );
}

function agoText(wallMs: number): string {
  if (!wallMs) return '';
  const s = Math.max(0, Math.round((Date.now() - wallMs) / 1000));
  return `${s}s 전`;
}

/**
 * Bench Light header (44px): brand · service tabs · env chip · connection chip · compact live/REC/viewers/agent ·
 * approvals · rights · STOP. STOP stays in the same place on every route.
 */
export function TopBar({ route }: { route?: Route }) {
  const { shell, activity, uiState } = useServices();
  useStoreVersion(shell);
  useStoreVersion(activity);
  const latestAgent = activity.recent(50).find((e) => e.origin === 'agent' || e.origin === 'script');
  const approvals = shell.approvals.size;
  const open = [...shell.channels.values()].filter((c) => c.state === 'open');
  const uartOpen = open.filter((c) => c.domain === 'uart').length;
  return (
    <header className="shell-hdr top-bar" role="banner" data-shell="header" data-env={shell.env ?? 'unknown'}>
      <a className="brand" href="#/l/m1-default" title="Loom 통합 작업 공간"><span className="brand-mark" aria-hidden="true">L</span><span className="brand-label">Loom</span></a>
      {route && <ServiceNav route={route} />}
      <span className="grow" />
      <EnvBadge env={shell.env} />
      <span className="conn-chip" title={`열린 인터페이스 ${open.length}개 (UART ${uartOpen})`}>
        <span className={`connection connection-${shell.conn.status}`} data-testid="connection-status" title={shell.conn.status === 'closed' ? shell.conn.reason : 'Loom 서버 연결'}>
          <span className="connection-dot" aria-hidden="true" />
          {shell.conn.status === 'open' ? '연결됨' : shell.conn.status === 'closed' ? '연결 끊김' : '연결 중'}
        </span>
        <span className="conn-count"><span aria-hidden="true">· </span>인터페이스 <b>{open.length}</b></span>
      </span>
      <LiveBadge />
      <span className={`rec rec-${shell.recordHealth}`} data-prio="4">
        REC {shell.recordHealth}
      </span>
      <span className="viewers" data-prio="5">
        보는 중 {shell.viewers ? `${shell.viewers.visible}/${shell.viewers.subscribed}` : '?'}
      </span>
      {latestAgent && (
        <span className="agent-activity" data-prio="6" data-origin={latestAgent.origin} title={`에이전트 ${latestAgent.actor} ${latestAgent.verb}`}>
          AGT {latestAgent.actor} {latestAgent.verb} {agoText(latestAgent.wallMs)}
        </span>
      )}
      {/* F1: approvals exist only when the policy enables them */}
      {approvals > 0 && (
        <button type="button" className="approvals-btn" data-prio="3" onClick={() => uiState.setApprovalsOpen(true)}>
          ⚠ 승인 {approvals}
        </button>
      )}
      <TestBadge />
      <span className="hdr-sep" aria-hidden="true" />
      <span className="rights" data-testid="rights" title="이 브라우저의 자격">자격: {shell.hello?.rights ?? '—'}{shell.automationCapped ? ' → watch (자동화 브라우저)' : ''}</span>
      <StopButton />
    </header>
  );
}
