import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Dev server proxies the daemon (loomd or tools/mock) so the SPA stays same-origin.
// LOOM_DAEMON defaults to the mock server port.
const daemon = process.env.LOOM_DAEMON ?? 'http://localhost:7421';

export default defineConfig({
  base: '/ui/',
  plugins: [react()],
  server: {
    host: 'localhost',
    port: 7420,
    strictPort: true,
    // Permissions/Referrer policy as loomd sends them (contract 01 §1.2). The strict CSP is
    // not set in dev: Vite's React refresh preamble is an inline script. CSP is checked against
    // loomd itself (e2e replay project).
    headers: {
      'Permissions-Policy': 'serial=(), usb=(), hid=(), bluetooth=()',
      'Referrer-Policy': 'no-referrer',
    },
    proxy: {
      '/rpc': { target: daemon.replace(/^http/, 'ws'), ws: true, changeOrigin: true },
      '/api': { target: daemon, changeOrigin: true },
    },
  },
  build: { target: 'es2023', sourcemap: true },
  worker: { format: 'es' },
});
