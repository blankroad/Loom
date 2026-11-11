// Activity strip (plan §5.3): last 2 entries, expandable to 50.
import { useState } from 'react';
import type { ActivityEntry } from '../store/adapter';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

const ORG: Record<string, string> = { agent: 'AGT', user: 'USR', device: 'DEV', script: 'SCR' };

function fmtTime(ms: number): string {
  if (!ms) return '--:--:--';
  const d = new Date(ms);
  return `${d.toLocaleTimeString('ko-KR', { hour12: false })}.${String(d.getMilliseconds()).padStart(3, '0')}`;
}

function Line({ e, delivered }: { e: ActivityEntry; delivered: boolean }) {
  return (
    <li className="act-line" data-origin={e.origin}>
      <span className="t">{fmtTime(e.wallMs)}</span>{' '}
      <span className={`org org-${(ORG[e.origin] ?? '?').toLowerCase()}`}>{ORG[e.origin] ?? '?'}</span> {e.actor}{' '}
      {e.domain}
      {e.channel ? ` ${e.channel}` : ''} {e.verb} {e.summary}
      {e.range && ` seq ${e.range.seqA}–${e.range.seqB}`}
      {e.waiting && <span className="waiting"> ⏳ {e.waiting}</span>}
      {e.origin === 'user' && (
        <span className="delivery">{delivered ? ' ⇢ 에이전트에게 전달됨' : ' ⇢ 전달 대기'}</span>
      )}
    </li>
  );
}

export function ActivityStrip() {
  const { activity } = useServices();
  useStoreVersion(activity);
  const [open, setOpen] = useState(false);
  const entries = activity.recent(open ? 50 : 2);
  return (
    <section className={`activity${open ? ' open' : ''}`} aria-label="Activity">
      <button type="button" className="act-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        Activity {open ? '▾' : '▸'}
      </button>
      {activity.dropped > 0 && <span className="muted"> (오래된 {activity.dropped}건 보관 범위 밖)</span>}
      <ul>
        {entries.length === 0 ? (
          <li className="muted">활동 없음</li>
        ) : (
          entries.map((e, i) => <Line key={`${e.seq}-${i}`} e={e} delivered={activity.isDelivered(e)} />)
        )}
      </ul>
    </section>
  );
}
