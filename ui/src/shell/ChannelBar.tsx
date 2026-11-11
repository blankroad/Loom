import { useEffect, useState } from 'react';
import { defaultPanelFor } from '../store/channels';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

const NEW_MARK_MS = 10_000;
const GEN_MARK_MS = 10_000;

/**
 * Channel bar (plan §5.1.1 row 2, §5.7 UD-31): every channel the daemon has shown us —
 * always, whatever auto_open_panels says (no hidden channel). Closed channels stay with
 * "닫힘"; a generation change (device reconnect) is marked for 10 s. Click = open its panel.
 */
export function ChannelBar() {
  const { shell, showSlot, now } = useServices();
  useStoreVersion(shell);
  const chans = [...shell.channels.values()];
  const t = now();
  // re-render once a second while a "new" / generation mark is showing, so it fades
  const marked = chans.some(
    (c) => (c.addedLiveMs !== null && t - c.addedLiveMs < NEW_MARK_MS) || (c.genChangedMs !== null && t - c.genChangedMs < GEN_MARK_MS),
  );
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!marked) return;
    const h = setInterval(() => setTick((x) => x + 1), 1_000);
    return () => clearInterval(h);
  }, [marked]);
  return (
    <nav className="channel-bar" aria-label="채널">
      {chans.length === 0 ? (
        <span className="muted">채널 없음</span>
      ) : (
        chans.map((c) => {
          const dp = defaultPanelFor(c.domain, c.channel);
          const isNew = c.addedLiveMs !== null && t - c.addedLiveMs < NEW_MARK_MS;
          const genNew = c.genChangedMs !== null && t - c.genChangedMs < GEN_MARK_MS;
          const cls = `chan chan-${c.state}${isNew ? ' chan-new' : ''}${genNew ? ' chan-gen' : ''}`;
          const label = (
            <>
              {c.domain.toUpperCase()} {c.channel}
              {c.generation !== null ? ` gen${c.generation}` : ''}
              {genNew ? ' ↻' : ''}
              {c.state === 'closed' ? ' (닫힘)' : ''}
            </>
          );
          return dp ? (
            <button
              key={`${c.domain}/${c.channel}`}
              type="button"
              className={cls}
              data-chan={`${c.domain}/${c.channel}`}
              data-state={c.state}
              title={`${dp.panel} 열기`}
              onClick={() => showSlot(dp)}
            >
              {label}
            </button>
          ) : (
            <span key={`${c.domain}/${c.channel}`} className={cls} data-chan={`${c.domain}/${c.channel}`} data-state={c.state}>
              {label}
            </span>
          );
        })
      )}
      {shell.overflow.channels > 0 && <span className="muted">+{shell.overflow.channels} 표시 안 됨</span>}
    </nav>
  );
}

/** 1 Hz clock + heartbeat dot: updates even with no data, so a frozen page is visible. */
export function HeartbeatClock() {
  const { shell } = useServices();
  useStoreVersion(shell);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const h = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(h);
  }, []);
  const s = shell.status;
  const cls = s.badge === 'STALLED' ? 'hb-stalled' : s.warnDot ? 'hb-warn' : 'hb-ok';
  return (
    <span className={`hb-clock ${cls}`} data-testid="hb-clock">
      <span aria-hidden="true">●</span> {now.toLocaleTimeString('ko-KR', { hour12: false })}
    </span>
  );
}
