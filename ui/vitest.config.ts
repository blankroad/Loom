import { defineConfig } from 'vitest/config';

// Default env is node. DOM tests opt in with a first-line docblock:
//   // @vitest-environment jsdom
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'tools/**/*.test.ts'],
    environment: 'node',
    setupFiles: ['./vitest.setup.ts'],
  },
});
