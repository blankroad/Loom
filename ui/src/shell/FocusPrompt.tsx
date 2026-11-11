// Agent "look here" prompt (plan §5.7) + per-tab settings controls.
// The prompt only routes/reveals on the user's [열기]; Enter/Esc work when the user is
// not typing somewhere (keys inside inputs and the terminal stay theirs).

import { useEffect } from 'react';
import { describeFocus } from '../store/focus';
import { useStoreVersion } from '../store/hooks';
import type { AutoOpen, FollowAgent } from '../store/tabSettings';
import { useServices } from './ShellContext';

const typingTarget = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);

export function FocusPrompt() {
  const { focus } = useServices();
  useStoreVersion(focus);
  const p = focus.pending;
  useEffect(() => {
    if (!p) return;
    const onKey = (e: KeyboardEvent) => {
      if (typingTarget(e.target)) return;
      if (e.key === 'Enter') {
        e.preventDefault();
        focus.accept();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        focus.dismiss();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [focus, p]);

  const info = focus.lastAuto;
  if (!p && !info) return null;
  if (!p && info) {
    const d = describeFocus(info);
    return (
      <div className="focus-prompt focus-auto" role="status" data-testid="focus-auto">
        에이전트 {info.actor}을(를) 따라 이동함 · {d.where}
        {d.at ? ` · ${d.at}` : ''}
        <button type="button" onClick={() => focus.dismissAutoInfo()}>
          닫기
        </button>
      </div>
    );
  }
  if (!p) return null;
  const d = describeFocus(p);
  return (
    <div className="focus-prompt" role="dialog" aria-label="에이전트 가리키기" data-testid="focus-prompt">
      <p>
        에이전트 {p.actor}이 보여 주려 함 · {d.where}
        {d.at ? ` · ${d.at}` : ''}
        {p.reason ? ` · 이유 ${p.reason}` : ''}
        {p.count > 1 ? ` (×${p.count})` : ''}
      </p>
      {p.held && <p className="muted">입력 중이라 자동 따라가기를 보류함</p>}
      <div className="actions">
        <button type="button" className="primary" onClick={() => focus.accept()}>
          열기 ⏎
        </button>
        <label>
          <input type="checkbox" onChange={(e) => e.target.checked && focus.followAndOpen()} /> 이 탭은 자동으로 따라가기
        </label>
        <button type="button" onClick={() => focus.dismiss()}>
          무시 Esc
        </button>
      </div>
    </div>
  );
}

export function TabSettingsControls() {
  const { settings } = useServices();
  useStoreVersion(settings);
  return (
    <details className="tab-settings">
      <summary>패널 동작 <span aria-hidden="true">⌄</span></summary>
      <div className="tab-settings-popover">
      <strong>이 브라우저 탭의 동작</strong>
      <p className="muted">장치 연결과 송신 권한에는 영향을 주지 않습니다.</p>
      <label>
        새 채널 패널{' '}
        <select aria-label="auto_open_panels" value={settings.autoOpenPanels} onChange={(e) => settings.setAutoOpen(e.target.value as AutoOpen)}>
          <option value="on">자동 열기</option>
          <option value="off">열지 않음</option>
        </select>
      </label>{' '}
      <label>
        에이전트 가리키기{' '}
        <select aria-label="follow_agent" value={settings.followAgent} onChange={(e) => settings.setFollow(e.target.value as FollowAgent)}>
          <option value="ask">묻기</option>
          <option value="auto">자동 따라가기</option>
          <option value="off">끄기</option>
        </select>
      </label>
      </div>
    </details>
  );
}
