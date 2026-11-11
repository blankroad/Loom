// STOP state panel (plan §5.1.7). Shown while the latch is on.
// - what is locked, what could not be cancelled, outputs still held
// - [비상 OFF] per held output: any right, no confirm, interlock only warns (A3)
// - recovery list from core.safe_state.list; [복구 송신 ⏎] runs the checked
//   items via core.safe_state.run{ids} and shows per-item results (A15)
// - [해제…]: M1 = operate + confirm card (approve right arrives in M2)

import { useEffect, useRef, useState } from 'react';
import {
  readSafeStateList,
  readSafeStateRun,
  type SafeStateItem,
  type SafeStateResult,
} from '../store/adapter';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

export function StopPanel() {
  const { shell, net, stop, confirm, toasts } = useServices();
  useStoreVersion(shell);
  const latch = shell.latch;
  const [items, setItems] = useState<SafeStateItem[]>([]);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<Map<string, SafeStateResult>>(new Map());
  const [busy, setBusy] = useState(false);
  const recoverBtn = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!latch.on) return;
    let alive = true;
    net
      .call('core.safe_state.list', {})
      .then((res) => {
        if (!alive) return;
        const list = readSafeStateList(res);
        setItems(list);
        setChecked(new Set(list.filter((i) => i.kind === 'recover' && !i.blocked).map((i) => i.id)));
      })
      .catch(() => alive && setItems([]));
    return () => {
      alive = false;
    };
  }, [latch.on, net]);

  // A15 "1 key": focus [복구 송신 ⏎] unless the user is typing in a terminal
  useEffect(() => {
    if (!latch.on) return;
    const active = document.activeElement;
    if (active && active.closest('[data-terminal]')) return;
    recoverBtn.current?.focus();
  }, [latch.on, items.length]);

  if (!latch.on) return null;

  const run = async (ids: string[]) => {
    if (ids.length === 0) return;
    setBusy(true);
    try {
      const res = readSafeStateRun(await net.call('core.safe_state.run', { ids }));
      setResults(new Map(res.map((r) => [r.id, r])));
    } catch (e) {
      toasts.push(`복구 송신 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const release = async () => {
    if (!shell.can('operate')) return;
    const ok = await confirm.confirm({
      mode: 'card',
      actionId: 'stop.release',
      rpc: 'core.stop.release',
      label: 'STOP 해제 — agent·script 쓰기를 다시 허용',
      params: {},
    });
    if (ok) await stop.release();
  };

  const recover = items.filter((i) => i.kind === 'recover');
  const emergency = (domain: string, channel: string) =>
    items.find((i) => i.kind === 'emergency_off' && i.domain === domain && (i.channel ?? '') === channel);

  return (
    <section className="stop-panel" data-testid="stop-panel" aria-label="STOP 상태">
      <header>
        ■ STOP — agent·script 쓰기 잠김{latch.by ? ` (${latch.by})` : ''}
        <button type="button" className="release-btn" disabled={!shell.can('operate')} onClick={() => void release()}>
          해제…
        </button>
      </header>
      <p>
        잠긴 것: 새 agent·script 쓰기
        {latch.jobsStopped.length > 0 && ` · 잡 ${latch.jobsStopped.length}개 정지 (${latch.jobsStopped.join(', ')})`}
        {latch.approvalsCancelled > 0 && ` · 대기 승인 ${latch.approvalsCancelled}건 취소`}
      </p>
      {latch.uncancellable.length > 0 && (
        <p className="warn">이미 드라이버에 넘어가 취소 못 했을 수 있음: {latch.uncancellable.join(' · ')}</p>
      )}
      {latch.outputsHeld.length > 0 && (
        <div className="outputs-held">
          ⚠ STOP — 출력 유지 중:
          {latch.outputsHeld.map((o) => {
            const em = emergency(o.domain, o.channel);
            return (
              <span key={`${o.domain}/${o.channel}`} className="held">
                {o.domain} {o.channel} {o.label} {o.state}{' '}
                <button
                  type="button"
                  className="emergency-off"
                  disabled={!em || busy}
                  title={em ? `확인 없이 즉시 (인터록은 경고만)${em.method ? ` · ${em.method}` : ''}` : '비상 OFF 경로 없음 — 데몬 safe_state 목록에 이 장치의 비상 OFF 항목이 없습니다'}
                  onClick={() => em && void run([em.id])}
                >
                  비상 OFF !
                </button>
                {!em && <span className="muted"> (경로 없음)</span>}
              </span>
            );
          })}
        </div>
      )}
      <div className="recover">
        복구 송신 (safe_state · 사용자 출처 · 인터록 검사):
        <ul>
          {recover.map((i) => {
            const r = results.get(i.id);
            return (
              <li key={i.id}>
                <label>
                  <input
                    type="checkbox"
                    disabled={!!i.blocked}
                    checked={checked.has(i.id)}
                    onChange={(e) => {
                      const n = new Set(checked);
                      if (e.target.checked) n.add(i.id);
                      else n.delete(i.id);
                      setChecked(n);
                    }}
                  />{' '}
                  {i.label}
                </label>
                {i.blocked && <span className="blocked"> ⛔ 막힘: {i.blocked}</span>}
                {r && (
                  <span className={r.ok ? 'ok' : 'fail'} data-outcome={r.outcome} data-request-id={r.requestId ?? undefined}>
                    {' '}
                    {r.ok ? '✓ 보냄' : r.outcome === 'skipped' ? `– 건너뜀: ${r.blocked ?? ''}` : `✗ ${r.blocked ? `막힘: ${r.blocked}` : (r.error ?? '실패')}`}
                    {r.finallyBlocked && ' · finally 막힘'}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
        <button
          ref={recoverBtn}
          type="button"
          className="recover-btn"
          disabled={busy || checked.size === 0 || !shell.can('operate')}
          onClick={() => void run([...checked])}
        >
          복구 송신 ⏎
        </button>{' '}
        <span className="muted">finally는 자동 실행하지 않습니다 (A15). 사용자 입력은 계속 가능합니다.</span>
      </div>
    </section>
  );
}
