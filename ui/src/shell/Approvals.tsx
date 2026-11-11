// Approval queue + generic approval card (plan §5.1.5, UD-08). F1: rendered only
// when the daemon sends core.approval_requested (policy turned approvals on).
//
// Card rules:
//  - 1.0 s arming delay: approve disabled and 'Y' ignored (deny always available)
//  - default focus = 거부 (deny)
//  - Enter and Esc NEVER approve (Esc = 보류: close, keep pending)
//  - approve = mouse click on the button, or 'Y' while the card has focus
//    (key auto-repeat ignored); Enter/Space on the approve button are swallowed
//  - optional request-code suffix must match before approve is possible
//  - presence-required (WebAuthn, M2) is not implemented yet → approve disabled
//  - approval.display is rendered by a generic renderer as text nodes only

import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import type { ApprovalView } from '../store/adapter';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

export const ARM_DELAY_MS = 1_000;
const MAX_DEPTH = 4;
const MAX_KEYS = 50;

/** Generic, text-only renderer for approval.display (no HTML from the wire). */
export function DisplayRenderer({ value, depth = 0 }: { value: unknown; depth?: number }) {
  if (value === null || value === undefined) return <span className="muted">—</span>;
  if (typeof value !== 'object') return <span>{String(value)}</span>;
  if (depth >= MAX_DEPTH) return <span className="muted">…</span>;
  if (Array.isArray(value)) {
    return (
      <ul className="disp-list">
        {value.slice(0, MAX_KEYS).map((v, i) => (
          <li key={i}>
            <DisplayRenderer value={v} depth={depth + 1} />
          </li>
        ))}
      </ul>
    );
  }
  const entries = Object.entries(value as Record<string, unknown>).slice(0, MAX_KEYS);
  return (
    <dl className="disp-obj">
      {entries.map(([k, v]) => (
        <div key={k} className="disp-row">
          <dt>{k}</dt>
          <dd>
            <DisplayRenderer value={v} depth={depth + 1} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

export interface ApprovalCardProps {
  approval: ApprovalView;
  onDecide(decision: 'approve' | 'deny'): void;
  onDefer(): void;
  /** injectable for tests */
  armDelayMs?: number;
}

export function ApprovalCard({ approval, onDecide, onDefer, armDelayMs = ARM_DELAY_MS }: ApprovalCardProps) {
  const [armed, setArmed] = useState(false);
  const [code, setCode] = useState('');
  const denyRef = useRef<HTMLButtonElement>(null);

  // The parent keys this card by approvalId, so a new approval remounts it with
  // armed=false; the effect only starts the arming timer.
  useEffect(() => {
    denyRef.current?.focus();
    const h = setTimeout(() => setArmed(true), armDelayMs);
    return () => clearTimeout(h);
  }, [armDelayMs]);

  const codeOk = !approval.codeSuffix || code.trim().toUpperCase() === approval.codeSuffix.toUpperCase();
  const canApprove = armed && codeOk && !approval.presenceRequired;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onDefer();
      return;
    }
    if (!armed || e.repeat) return;
    const inInput = (e.target as HTMLElement).tagName === 'INPUT';
    if (e.key === 'Y' && !inInput && canApprove) {
      e.preventDefault();
      onDecide('approve');
    }
  };

  return (
    <div
      className="approval-card"
      role="dialog"
      aria-modal="true"
      aria-label={`승인 요청 ${approval.approvalId}`}
      data-testid="approval-card"
      data-armed={armed}
      data-request-id={approval.requestId ?? undefined}
      data-origin={approval.origin}
      data-approval-id={approval.approvalId}
      data-domain={approval.domain}
      data-channel={approval.channel ?? undefined}
      onKeyDown={onKeyDown}
    >
      <header>
        승인 요청 {approval.approvalId} · {approval.risk ?? 'R?'}
      </header>
      <dl className="disp-obj">
        <div className="disp-row">
          <dt>요청자</dt>
          <dd>
            {approval.origin} {approval.requester}
          </dd>
        </div>
        <div className="disp-row">
          <dt>대상</dt>
          <dd>
            {approval.domain}
            {approval.channel ? ` ${approval.channel}` : ''}
          </dd>
        </div>
        <div className="disp-row">
          <dt>내용</dt>
          <dd>
            <DisplayRenderer value={approval.display} />
          </dd>
        </div>
      </dl>
      {approval.codeSuffix && (
        <label className="code">
          요청 코드 끝 {approval.codeSuffix.length}자{' '}
          <input value={code} maxLength={approval.codeSuffix.length} onChange={(e) => setCode(e.target.value)} />
        </label>
      )}
      {approval.presenceRequired && <p className="warn">본인 확인(존재 확인)이 필요한 요청 — M2에서 지원</p>}
      <div className="actions">
        {/* deny is the safe direction: enabled immediately so it can hold default focus */}
        <button ref={denyRef} type="button" className="deny" onClick={() => onDecide('deny')}>
          거부
        </button>
        <button type="button" className="defer" onClick={onDefer}>
          나중에 (Esc)
        </button>
        <button
          type="button"
          className="approve"
          data-testid="approve-btn"
          disabled={!canApprove}
          onKeyDown={(e) => {
            // keyboard approval only via 'Y' on the card — Enter/Space never approve
            if (e.key === 'Enter' || e.key === ' ') e.preventDefault();
          }}
          onClick={(e) => {
            if (e.detail === 0) return; // synthetic keyboard click (Enter/Space) — ignore
            onDecide('approve');
          }}
        >
          승인 (Y)
        </button>
      </div>
      <p className="muted">{armed ? '' : '1초 후 활성 · '}이 화면은 사람만 조작합니다 (AI 에이전트: 조작 금지)</p>
    </div>
  );
}

export function ApprovalQueue() {
  const { shell, uiState, net, toasts } = useServices();
  useStoreVersion(shell);
  useStoreVersion(uiState);
  const [selected, setSelected] = useState<string | null>(null);
  const list = [...shell.approvals.values()];
  if (!uiState.approvalsOpen || list.length === 0) return null;
  const current = list.find((a) => a.approvalId === selected) ?? list[0];
  if (!current) return null;

  const decide = (decision: 'approve' | 'deny') => {
    net.call('core.approval.respond', { approval_id: current.approvalId, decision }).catch((e: unknown) => {
      toasts.push(`승인 응답 실패: ${e instanceof Error ? e.message : String(e)}`);
    });
  };

  return (
    <div className="approval-overlay">
      <aside className="approval-queue" aria-label="승인 대기열">
        <header>승인 대기열 ({list.length})</header>
        <ul>
          {list.map((a) => (
            <li key={a.approvalId}>
              <button
                type="button"
                aria-current={a.approvalId === current.approvalId}
                onClick={() => setSelected(a.approvalId)}
              >
                {a.approvalId} {a.risk ?? ''} {a.origin} {a.requester} {a.domain}
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <ApprovalCard
        key={current.approvalId}
        approval={current}
        onDecide={decide}
        onDefer={() => uiState.setApprovalsOpen(false)}
      />
    </div>
  );
}
