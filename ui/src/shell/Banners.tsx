// Connection / STALLED / REC failed / worker crash / epoch banners (plan §5.4),
// toasts, and the single aria-live region (approvals · STOP · errors only, §5.9).

import { useEffect, useRef, useState } from 'react';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

export function Banners() {
  const { shell, rows, reporter, scheduler } = useServices();
  useStoreVersion(shell);
  useStoreVersion(rows);
  useStoreVersion(reporter);
  const out: { key: string; cls: string; text: string }[] = [];
  const c = shell.conn;
  if (c.status === 'reconnecting') {
    out.push({
      key: 'conn',
      cls: 'banner-warn',
      text: `연결 끊김 — 재연결 중 (${Math.ceil(c.retryInMs / 1000)}s, ${c.reason}). 쓰기 버튼 잠금, STOP은 HTTP로 동작`,
    });
  } else if (c.status === 'closed') {
    out.push({ key: 'conn', cls: 'banner-err', text: `연결 닫힘: ${c.reason}` });
  } else if (c.status === 'connecting') {
    out.push({ key: 'conn', cls: 'banner-info', text: '데몬에 연결 중…' });
  }
  if (c.status === 'open' && shell.status.badge === 'STALLED') {
    out.push({ key: 'stalled', cls: 'banner-err', text: 'STALLED — 데몬 heartbeat 없음. 쓰기 잠금 (STOP은 가능)' });
  }
  if (shell.recordHealth === 'failed') {
    out.push({ key: 'rec', cls: 'banner-err', text: '기록 불능 — 에이전트 쓰기 중단됨, 사용자 입력은 가능 (REC FAILED)' });
  } else if (shell.recordHealth === 'degraded') {
    out.push({ key: 'rec', cls: 'banner-warn', text: '기록 지연 — REC degraded' });
  }
  for (const w of shell.workers.values()) {
    if (w.state === 'crashed' || w.state === 'restarting') {
      out.push({ key: `w-${w.worker}`, cls: 'banner-err', text: `worker ${w.worker} ${w.state} (재기동 ${w.restarts}회)` });
    }
  }
  if (shell.epochChange) {
    out.push({ key: 'epoch', cls: 'banner-warn', text: '데몬 재시작 (epoch 변경) — 다시 snapshot 받는 중' });
  }
  if (shell.automation) {
    out.push(
      shell.automationCapped
        ? { key: 'automation', cls: 'banner-warn', text: '자동화 브라우저 — 보기 전용 (쓰기 버튼 비활성, STOP·비상 OFF는 가능)' }
        : { key: 'automation', cls: 'banner-warn', text: shell.mockServer ? '자동화 브라우저 — mock 서버라 쓰기 허용' : '자동화 브라우저 — 자동 실행 정책에서 쓰기 허용' },
    );
  }
  if (rows.rejected > 0) {
    out.push({
      key: 'rows',
      cls: 'banner-err',
      text: `진행 중 쓰기 행 상한 초과 — ${rows.rejected}건 화면에 못 올림 (기록은 데몬 저널)`,
    });
  }
  const back = reporter.returned;
  if (out.length === 0 && !back) return null;
  return (
    <div className="banners">
      {out.map((b) => (
        <div key={b.key} className={`banner ${b.cls}`} data-banner={b.key}>
          {b.text}
        </div>
      ))}
      {back && (
        <div className="banner banner-info" data-banner="hidden-return">
          숨김 중 {(back.hiddenMs / 1000).toFixed(1)} s ({back.events.toLocaleString('ko-KR')} 이벤트){' '}
          <button
            type="button"
            onClick={() => {
              reporter.dismissReturn();
              scheduler.invalidateAll();
            }}
          >
            — 라이브로 이동
          </button>
        </div>
      )}
    </div>
  );
}

export function Toasts() {
  const { toasts } = useServices();
  useStoreVersion(toasts);
  // one timer per toast id, started when it first appears (re-renders do not extend it)
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());
  useEffect(() => {
    const live = new Set(toasts.items.map((t) => t.id));
    for (const t of toasts.items) {
      if (!timers.current.has(t.id)) timers.current.set(t.id, setTimeout(() => toasts.remove(t.id), t.ttlMs));
    }
    for (const [id, h] of timers.current) {
      if (!live.has(id)) {
        clearTimeout(h);
        timers.current.delete(id);
      }
    }
  }, [toasts, toasts.version]);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  return (
    <div className="toasts">
      {toasts.items.map((t) => (
        <div key={t.id} className="toast" data-toast-key={t.key ?? ''}>
          {t.text}
          {t.actions.map((a) => (
            <button
              key={a.label}
              type="button"
              onClick={() => {
                a.run();
                toasts.remove(t.id);
              }}
            >
              {a.label}
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

/** One assertive live region. Announces only approvals, STOP and errors. */
export function AriaAnnouncer() {
  const { shell, stop } = useServices();
  useStoreVersion(shell);
  useStoreVersion(stop);
  const [msg, setMsg] = useState('');
  const prev = useRef({ approvals: 0, phase: stop.phase, errors: 0 });
  const approvals = shell.approvals.size;
  const phase = stop.phase;
  const errors = shell.errors.length;
  useEffect(() => {
    const p = prev.current;
    if (approvals > p.approvals) setMsg(`승인 요청 ${approvals}건`);
    else if (phase !== p.phase && phase !== 'idle') setMsg(phase === 'locked' ? 'STOP 잠김' : phase === 'unconfirmed' ? 'STOP 확인 안 됨' : 'STOP 요청 중');
    else if (errors > p.errors) setMsg(`오류: ${shell.errors[errors - 1]?.message ?? ''}`);
    prev.current = { approvals, phase, errors };
  }, [approvals, phase, errors, shell]);
  return (
    <div className="sr-only" aria-live="assertive" role="status" data-testid="aria-live">
      {msg}
    </div>
  );
}
