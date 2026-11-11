// UI-M1 acceptance suite (plan §11 UI-T01–T10 + eth/test/psu) on two backends.
//   mock   = tools/mock (TS)                     → Vite :7440 → mock :7441
//   replay = CORE `loomd --fixture-replay`        → Vite :7450 → loomd :4790 (scenario all, echo writes)
//            + :4791 `--rights watch` and :4792 `--scenario saturate` (opened per test with the
//            DEV-only page query `?loom_daemon=ws://localhost:479x/rpc`, src/net/index.ts)
// Dedicated ports so this suite never reuses a dev server a person (or ui-opus) left running on
// 7420/7421 against another backend. Tests that can run on replay carry the `@replay` tag; the
// replay project runs only those. Run (lead adds the scripts):
//   pnpm exec playwright test -c e2e/m1.config.ts --project=mock
//   pnpm exec playwright test -c e2e/m1.config.ts --project=replay
// Numeric latency gates (UI-T03) only with LOOM_PERF=1 on the fixed runner.
// loomd is built by `cargo build -p loom-gateway` (workspace root); `cargo run` reuses that build.

import { defineConfig, devices } from '@playwright/test';
import os from 'node:os';
import path from 'node:path';

process.env.PLAYWRIGHT_BROWSERS_PATH ??= path.join(os.homedir(), '.cache', 'ms-playwright');

const MOCK_PORT = 7441;
const VITE_MOCK = 7440;
const VITE_REPLAY = 7450;
const UI = path.resolve(import.meta.dirname, '..'); // webServer cwd defaults to this file's dir (e2e/)
const ROOT = path.resolve(UI, '..'); // workspace root (Cargo workspace)
const loomd = (port: number, extra: string) =>
  `cargo run -q -p loom-gateway --bin loomd -- --fixture-replay builtin --port ${port} --echo-writes --allow-origin http://localhost:${VITE_REPLAY} ${extra}`;

process.env.LOOM_MOCK_URL ??= `ws://localhost:${MOCK_PORT}/rpc`;

export default defineConfig({
  testDir: 'm1',
  timeout: 60_000,
  fullyParallel: false,
  workers: 1, // one shared daemon per backend: tests inject faults and STOP
  reporter: [['list']],
  use: { trace: 'retain-on-failure', ...devices['Desktop Chrome'] },
  projects: [
    { name: 'mock', use: { baseURL: `http://localhost:${VITE_MOCK}/ui/` }, metadata: { backend: 'mock' } },
    { name: 'replay', grep: /@replay/, use: { baseURL: `http://localhost:${VITE_REPLAY}/ui/` }, metadata: { backend: 'replay' } },
  ],
  webServer: [
    { command: 'pnpm mock', cwd: UI, env: { LOOM_MOCK_PORT: String(MOCK_PORT) }, url: `http://localhost:${MOCK_PORT}/healthz`, reuseExistingServer: true, timeout: 20_000 },
    { command: `pnpm exec vite --port ${VITE_MOCK} --strictPort`, cwd: UI, env: { LOOM_DAEMON: `http://localhost:${MOCK_PORT}` }, url: `http://localhost:${VITE_MOCK}/ui/`, reuseExistingServer: true, timeout: 30_000 },
    { command: loomd(4790, '--scenario all'), cwd: ROOT, url: 'http://localhost:4790/', reuseExistingServer: true, timeout: 300_000 },
    { command: loomd(4791, '--scenario normal --rights watch'), cwd: ROOT, url: 'http://localhost:4791/', reuseExistingServer: true, timeout: 300_000 },
    { command: loomd(4792, '--scenario saturate'), cwd: ROOT, url: 'http://localhost:4792/', reuseExistingServer: true, timeout: 300_000 },
    { command: `pnpm exec vite --port ${VITE_REPLAY} --strictPort`, cwd: UI, env: { LOOM_DAEMON: 'http://localhost:4790' }, url: `http://localhost:${VITE_REPLAY}/ui/`, reuseExistingServer: true, timeout: 30_000 },
  ],
});
