import { defineConfig, devices } from '@playwright/test';
import os from 'node:os';
import path from 'node:path';

process.env.PLAYWRIGHT_BROWSERS_PATH ??= path.join(os.homedir(), '.cache', 'ms-playwright');

export default defineConfig({
  testDir: '.',
  testMatch: 'real-smoke.spec.ts',
  timeout: 45_000,
  workers: 1,
  reporter: [['list']],
  use: { ...devices['Desktop Chrome'], trace: 'off' },
});
