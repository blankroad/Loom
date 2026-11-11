import { defineConfig, devices } from '@playwright/test';
import os from 'node:os';
import path from 'node:path';

// Browsers live outside the project (impl playbook §4).
process.env.PLAYWRIGHT_BROWSERS_PATH ??= path.join(os.homedir(), '.cache', 'ms-playwright');

export default defineConfig({
  testDir: 'e2e',
  testIgnore: ['m1/**'], // M1 acceptance suite runs via e2e/m1.config.ts (pnpm e2e:m1)
  timeout: 30_000,
  fullyParallel: false,
  reporter: [['list']],
  use: { baseURL: 'http://localhost:7420/ui/', trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    { command: 'pnpm mock', url: 'http://localhost:7421/healthz', reuseExistingServer: true, timeout: 20_000 },
    { command: 'pnpm dev', url: 'http://localhost:7420/ui/', reuseExistingServer: true, timeout: 30_000 },
  ],
});
