// shell.test acceptance (_xteam/ui-test.md "수용 시험" 5개, UI-M1b) + psu.gauge emergency OFF
// (_xteam/explore-ui.md §psu.gauge). Mock TC-1 session (tools/mock/testRun.ts) and SimPsu
// (tools/mock/psu.ts); layout `#/l/test` = shell.test + psu.gauge(bench).

import { Daemon, expect, mockOnly, openApp, pageQuery, RpcError, sleep, test, type G } from './lib';

async function freshSession(ctl: Daemon): Promise<string> {
  await ctl.inject('reset');
  await ctl.call('core.stop.release').catch(() => undefined);
  const r = await ctl.agent<{ session: string }>('test.run_tc1', { gap_ms: 5 });
  return r.session;
}

test.describe('shell.test acceptance (TC-1, sim)', () => {
  test.beforeEach(() => mockOnly('fixture replay has no test sessions'));

  test('1: agent `test step --begin` → ▶ in the step list and the step band within 200 ms', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await freshSession(ctl);
      await openApp(page, '/l/test');
      await expect(page.locator('[data-testid=test-steps] tr[data-step-id=S5]')).toHaveClass(/current/);
      await ctl.agent('test.step', { step: 'S5', end: true });
      await expect(page.locator('[data-testid=test-steps] tr.current')).toHaveCount(0);
      // timing on the page clock: RPC sent → DOM marks S5 current (both in the browser? no — measure from Node)
      const t0 = Date.now();
      await ctl.agent('test.step', { step: 'S5', begin: true });
      await expect(page.locator('[data-testid=test-steps] tr[data-step-id=S5]')).toHaveClass(/current/, { timeout: 2_000 });
      await expect(page.locator('[data-testid=step-band] .band-current')).toHaveCount(1);
      const ms = Date.now() - t0;
      test.info().annotations.push({ type: 'test 1: step begin → ▶ + band', description: `${ms} ms incl. RPC + Playwright polling` });
      if (process.env.LOOM_PERF === '1') expect(ms).toBeLessThanOrEqual(200);
    } finally {
      ctl.close();
    }
  });

  test('2: a PASS citing a fabricated seq is rejected on screen; the verdict does not change', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await freshSession(ctl);
      await openApp(page, '/l/test');
      const s5 = page.locator('[data-testid=test-steps] tr[data-step-id=S5]');
      await expect(s5).toHaveAttribute('data-verdict', '');
      const err = await ctl.agent('test.verdict', { step: 'S5', verdict: 'pass', evidence_refs: ['uart:dut@e1:seq:1'] }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('E_EVIDENCE_OUT_OF_WINDOW');
      await s5.click();
      await expect(page.getByTestId('test-detail')).toContainText('E_OUT_OF_WINDOW');
      await expect(page.locator('[data-testid=test-detail] [data-verdict-row=rejected]')).toHaveCount(1);
      await expect(s5).toHaveAttribute('data-verdict', ''); // not pass
      // S3 (rejected then accepted with real evidence) shows both
      await page.locator('[data-testid=test-steps] tr[data-step-id=S3]').click();
      await expect(page.locator('[data-testid=test-detail] [data-verdict-row=rejected]')).toHaveCount(1);
      await expect(page.locator('[data-testid=test-steps] tr[data-step-id=S3]')).toHaveAttribute('data-verdict', 'pass');
    } finally {
      ctl.close();
    }
  });

  test('3: a second browser joining late sees the same steps and verdicts', async ({ page, browser }) => {
    const ctl = await Daemon.open();
    try {
      await freshSession(ctl);
      await openApp(page, '/l/test');
      await expect(page.locator('[data-testid=test-steps] tr[data-step-id=S5]')).toHaveClass(/current/);
      const ctx2 = await browser.newContext({ baseURL: test.info().project.use.baseURL });
      const late = await ctx2.newPage();
      try {
        await openApp(late, '/l/test');
        const sig = (p: typeof page) =>
          p.locator('[data-testid=test-steps] tr[data-step-id]').evaluateAll((rows) =>
            rows.map((r) => `${(r as unknown as { getAttribute(n: string): string }).getAttribute('data-step-id')}:${(r as unknown as { getAttribute(n: string): string }).getAttribute('data-status')}:${(r as unknown as { getAttribute(n: string): string }).getAttribute('data-verdict')}`).join(' '),
          );
        await expect.poll(() => sig(late)).toBe(await sig(page));
        await expect(late.getByTestId('test-state')).toHaveText((await page.getByTestId('test-state').textContent()) ?? '');
        await expect(late.getByTestId('ask-dock')).toBeVisible(); // the open question too
      } finally {
        await ctx2.close();
      }
    } finally {
      ctl.close();
    }
  });

  test('4: STOP → finally is not run automatically; the recovery key is shown', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await freshSession(ctl);
      await openApp(page, '/l/test');
      const testEvents = () => ctl.control.filter((e) => e.domain === 'test').length;
      await page.getByTestId('stop-button').click();
      await expect(page.getByTestId('test-stop-banner')).toContainText('finally는 자동 실행하지 않습니다');
      await expect(page.getByTestId('stop-panel').getByRole('button', { name: /복구 송신/ })).toBeVisible();
      const n0 = testEvents();
      await sleep(1_000);
      expect(testEvents()).toBe(n0); // nothing ran on its own
      const connId = await page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.hello?.conn_id ?? '');
      const calls = (await ctl.inject<{ conns: { id: string; calls: Record<string, number> }[] }>('conn_stats')).conns.find((c) => c.id === connId)?.calls ?? {};
      expect(Object.keys(calls).filter((m) => m.startsWith('test.'))).toEqual([]);
      await page.getByTestId('test-stop-banner').getByRole('button', { name: /복구 송신으로/ }).click();
      await expect(page.getByTestId('stop-panel').getByRole('button', { name: /복구 송신/ })).toBeFocused();
    } finally {
      await ctl.call('core.stop.release').catch(() => undefined);
      ctl.close();
    }
  });

  test('5: verdict override from the agent client is refused; an approve viewer can override with a reason', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await freshSession(ctl);
      const err = await ctl.agent('test.verdict', { step: 'S4', verdict: 'pass', override: true, reason: 'agent' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('E_DENIED');
      await openApp(page, '/l/test', pageQuery({ loom_rights: 'approve' }));
      await page.locator('[data-testid=test-steps] tr[data-step-id=S4]').click();
      await page.getByTestId('test-detail').getByRole('button', { name: '판정 수정…' }).click();
      const form = page.getByTestId('reason-form');
      await form.getByLabel('수정 판정').selectOption('inconclusive');
      await expect(form.getByRole('button', { name: '판정 수정' })).toBeDisabled(); // reason required
      await form.getByLabel('사유 (필수)').fill('이전 세션 잔여 DTC');
      await form.getByRole('button', { name: '판정 수정' }).click();
      await expect(page.locator('[data-testid=test-steps] tr[data-step-id=S4]')).toHaveAttribute('data-verdict', 'inconclusive');
      await expect(page.locator('[data-testid=test-detail] [data-verdict-row=override]')).toHaveCount(1);
    } finally {
      ctl.close();
    }
  });
});

test.describe('psu.gauge (SimPsu bench)', () => {
  test.beforeEach(() => mockOnly('fixture replay has no psu domain'));

  test('emergency OFF from a watch tab: no confirm, during STOP, output goes OFF; agent psu set shows at once', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset');
      await ctl.call('core.stop.release').catch(() => undefined);
      await ctl.call('psu.out', { channel: 'bench', state: 'on' });
      await openApp(page, '/l/test', pageQuery({ loom_rights: 'watch' }));
      const head = page.getByTestId('psu-head');
      await expect(head).toContainText('ON');
      // agent sets a value → the gauge shows it (explore-ui: ≤ 100 ms; logic here)
      await ctl.agent('psu.set', { channel: 'bench', v: 12.0 });
      await expect(page.getByTestId('psu-vals')).toContainText('12.00');
      // STOP keeps outputs: "출력 유지 중"
      await page.getByTestId('stop-button').click();
      await expect(head).toContainText('출력 유지');
      // emergency OFF: watch right, no confirm card, works during STOP
      await head.getByRole('button', { name: /비상 OFF/ }).click();
      await expect(page.getByTestId('confirm-card')).toHaveCount(0);
      await expect(head).toContainText('OFF');
      await expect.poll(() => ctl.control.some((e) => e.kind === 'psu.cmd' && e.origin === 'user' && e.payload.state === 'off' && e.payload.write_state === 'confirmed')).toBe(true);
      // the agent is still latched out
      const err = await ctl.agent('psu.out', { channel: 'bench', state: 'on' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('stop_latched');
    } finally {
      await ctl.call('core.stop.release').catch(() => undefined);
      await ctl.call('psu.out', { channel: 'bench', state: 'on' }).catch(() => undefined);
      ctl.close();
    }
  });
});
