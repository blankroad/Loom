// Shared test setup. uPlot reads window.matchMedia at module load, and jsdom does
// not implement it — provide a static stub for DOM tests.
const w = (globalThis as { window?: { matchMedia?: unknown } }).window;
if (w && typeof w.matchMedia !== 'function') {
  Object.defineProperty(w, 'matchMedia', {
    configurable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener() {},
      removeListener() {},
      addEventListener() {},
      removeEventListener() {},
      dispatchEvent: () => false,
    }),
  });
}
