// Test harness: mount a registered panel against FakeNet with real stores and a
// manually driven RenderScheduler. Lives in sdk/ so panel tests stay inside the
// I-UI1 import rules (panels may not import store/ or net/).

import type { HelloResult, LoomEvent } from '../../proto/provisional';
import { ActivityStore } from '../../store/activityStore';
import { RenderScheduler } from '../../store/renderScheduler';
import { RowStore } from '../../store/rowStore';
import { ShellControlStore } from '../../store/shellControl';
import { ev, FakeNet, TEST_HELLO, writeEv } from '../../store/testing/fakeNet';
import { ActionGate } from '../actionGate';
import { createPanelContext, SimpleCursorBus } from '../context';
import type { Props } from '../manifest';
import { getPanel, type PanelInstance } from '../registry';

export { ev, writeEv };

export interface Harness {
  net: FakeNet;
  shell: ShellControlStore;
  rows: RowStore;
  activity: ActivityStore;
  host: HTMLDivElement;
  instance: PanelInstance;
  toasts: string[];
  reveals: string[];
  /** run pending animation frames (draws) */
  frame(): void;
  /** deliver a batch to the panel's n-th subscription (default 0) and draw */
  batch(events: LoomEvent[], sub?: number): void;
  snapshotEnd(h?: string, sub?: number): void;
  dispose(): void;
}

export function mountPanel(
  id: string,
  props: Props = {},
  opts: { hello?: Partial<HelloResult>; now?: () => number } = {},
): Harness {
  const reg = getPanel(id);
  if (!reg) throw new Error(`panel ${id} not registered (import its module first)`);
  // harness clock: advances 100 ms per frame() so maxHz-limited panels always draw
  let clock = 0;
  const now = opts.now ?? (() => clock);
  const net = new FakeNet();
  net.open({ ...TEST_HELLO, conn_id: 'c-me', ...opts.hello });
  const shell = new ShellControlStore();
  shell.setConn(net.state, 0);
  const rows = new RowStore({ now });
  const activity = new ActivityStore();
  let pending: (() => void) | null = null;
  const scheduler = new RenderScheduler({
    raf: (cb) => {
      pending = () => cb(now());
      return 1;
    },
    cancelRaf: () => {
      pending = null;
    },
    now,
  });
  const toasts: string[] = [];
  const reveals: string[] = [];
  const gate = new ActionGate({
    net,
    rights: () => shell.rights,
    writeLocked: () => false,
    confirmUi: { confirm: async () => true },
  });
  const h = createPanelContext(reg.manifest, props, {
    net,
    gate,
    scheduler,
    rows,
    shell,
    cursor: new SimpleCursorBus(),
    ui: { reveal: (r) => void reveals.push(r), openPanel: () => {}, toast: (m) => void toasts.push(m) },
    agentCursor: activity,
    now,
    absNowMs: now,
  });
  const instance = reg.factory(h.ctx);
  const host = document.createElement('div');
  document.body.append(host);
  instance.mount(host);
  const frame = () => {
    clock += 100;
    for (let i = 0; i < 4 && pending; i++) {
      const p = pending;
      pending = null;
      p();
    }
  };
  frame();
  return {
    net,
    shell,
    rows,
    activity,
    host,
    instance,
    toasts,
    reveals,
    frame,
    batch(events, sub = 0) {
      net.emitBatch(sub, events);
      frame();
    },
    snapshotEnd(hs = '0', sub = 0) {
      net.subs[sub]?.h.onSnapshotEnd?.(hs);
      frame();
    },
    dispose() {
      instance.dispose();
      h.dispose();
      host.remove();
    },
  };
}
