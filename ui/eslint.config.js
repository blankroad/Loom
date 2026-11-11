import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';

// Invariant I-UI1 (plan.md §4.3): panels reach the daemon only through PanelContext.
const panelBans = {
  'no-restricted-globals': ['error',
    { name: 'WebSocket', message: 'Panels must use PanelContext (I-UI1).' },
    { name: 'fetch', message: 'Panels must use ctx.query (I-UI1).' },
    { name: 'XMLHttpRequest', message: 'I-UI1' },
    { name: 'EventSource', message: 'I-UI1' },
    { name: 'WebTransport', message: 'I-UI1' },
  ],
  'no-restricted-properties': ['error',
    { object: 'navigator', property: 'serial', message: 'Browsers never open devices (I-UI1).' },
    { object: 'navigator', property: 'usb', message: 'I-UI1' },
    { object: 'navigator', property: 'hid', message: 'I-UI1' },
    { object: 'navigator', property: 'bluetooth', message: 'I-UI1' },
  ],
  'no-restricted-imports': ['error', { patterns: [
    { group: ['**/net/**', '**/net'], message: 'Panels import only ../sdk (I-UI1).' },
    { group: ['**/store/**', '**/store'], message: 'Panels import only ../sdk (I-UI1).' },
  ] }],
};

export default tseslint.config(
  { ignores: ['dist', 'node_modules', 'src/proto/gen'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: { globals: { ...globals.browser, ...globals.worker } },
    plugins: { 'react-hooks': reactHooks },
    rules: { ...reactHooks.configs.recommended.rules },
  },
  { files: ['src/panels/**/*.{ts,tsx}'], rules: panelBans },
  { files: ['tools/**/*.ts', 'e2e/**/*.ts', '*.config.{ts,js}'], languageOptions: { globals: globals.node } },
);
