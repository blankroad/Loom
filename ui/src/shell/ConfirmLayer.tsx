// Renders ConfirmHost requests ('card' and 'enter_twice'). Same safety rules as
// the approval card: 1 s arming, default focus on 취소, Esc cancels.
// 'enter_twice': the first Enter arms, the second Enter (≤ 3 s later) confirms.
// Each request mounts a fresh card (keyed), so arming state never leaks between requests.

import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import type { ConfirmRequest } from '../sdk/actionGate';
import { useStoreVersion } from '../store/hooks';
import { ARM_DELAY_MS, DisplayRenderer } from './Approvals';
import { useServices } from './ShellContext';

const ENTER_WINDOW_MS = 3_000;

let reqSerial = 0;
const reqKeys = new WeakMap<ConfirmRequest, number>();
function keyOf(r: ConfirmRequest): number {
  let k = reqKeys.get(r);
  if (k === undefined) reqKeys.set(r, (k = ++reqSerial));
  return k;
}

export function ConfirmLayer() {
  const { confirm } = useServices();
  useStoreVersion(confirm);
  const req = confirm.current;
  if (!req) return null;
  return <ConfirmCard key={keyOf(req)} req={req} answer={(ok) => confirm.answer(ok)} />;
}

export function ConfirmCard({ req, answer }: { req: ConfirmRequest; answer: (ok: boolean) => void }) {
  const [armed, setArmed] = useState(req.mode === 'enter_twice');
  const firstEnter = useRef<number | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
    if (req.mode === 'enter_twice') return;
    const h = setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => clearTimeout(h);
  }, [req.mode]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      answer(false);
      return;
    }
    if (req.mode === 'enter_twice' && e.key === 'Enter' && !e.repeat) {
      e.preventDefault();
      const t = performance.now();
      if (firstEnter.current !== null && t - firstEnter.current <= ENTER_WINDOW_MS) answer(true);
      else firstEnter.current = t;
    }
  };

  return (
    <div className="confirm-overlay">
      <div
        className="confirm-card"
        role="dialog"
        aria-modal="true"
        aria-label={req.label}
        onKeyDown={onKeyDown}
        data-testid="confirm-card"
      >
        <header>{req.label}</header>
        <p className="mono">{req.rpc}</p>
        <DisplayRenderer value={req.params} />
        {req.mode === 'enter_twice' && <p className="muted">Enter를 한 번 더 누르면 실행합니다 (3초 안)</p>}
        <div className="actions">
          <button ref={cancelRef} type="button" onClick={() => answer(false)}>
            취소
          </button>
          {req.mode === 'card' && (
            <button type="button" className="approve" disabled={!armed} onClick={() => answer(true)}>
              확인
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
