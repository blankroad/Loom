// Icon set of the agreed REV6.1 web workbench (work/reports/uart-workbench/opus/index.html <symbol>s), verbatim.
// Inline SVG (no sprite dependency), so vanilla panels, Dockview headers and React chrome render the same glyphs.

export const ICONS: Record<string, string> = {
  "plus": "<path d=\"M8 3v10M3 8h10\"/>",
  "split-r": "<rect x=\"2\" y=\"2.5\" width=\"12\" height=\"11\" rx=\"1.5\"/><path d=\"M8 2.5v11\"/>",
  "split-d": "<rect x=\"2\" y=\"2.5\" width=\"12\" height=\"11\" rx=\"1.5\"/><path d=\"M2 8h12\"/>",
  "max": "<path d=\"M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9\"/>",
  "restore": "<path d=\"M13.5 6.5h-4v-4M2.5 9.5h4v4M9.5 6.5 14 2M6.5 9.5 2 14\"/>",
  "x": "<path d=\"M4 4l8 8M12 4l-8 8\"/>",
  "pane-x": "<rect x=\"2\" y=\"2.5\" width=\"12\" height=\"11\" rx=\"1.5\"/><path d=\"M6 6.5l4 4M10 6.5l-4 4\"/>",
  "gear": "<circle cx=\"8\" cy=\"8\" r=\"2.2\"/><path d=\"M8 1.8v1.6M8 12.6v1.6M14.2 8h-1.6M3.4 8H1.8M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1M12.4 12.4l-1.1-1.1M4.7 4.7 3.6 3.6\"/>",
  "plug": "<path d=\"M5.5 2v3M10.5 2v3M4 5h8v2.5a4 4 0 0 1-8 0zM8 11.5V14\"/>",
  "unplug": "<path d=\"M5.5 2v3M10.5 2v3M4 5h8v2.5a4 4 0 0 1-8 0zM8 11.5V14M2 2l12 12\"/>",
  "search": "<circle cx=\"7\" cy=\"7\" r=\"4.2\"/><path d=\"M10.2 10.2 14 14\"/>",
  "pause": "<path d=\"M5.5 3v10M10.5 3v10\"/>",
  "play": "<path d=\"M4.5 2.8v10.4L13 8z\"/>",
  "stop": "<rect x=\"3.5\" y=\"3.5\" width=\"9\" height=\"9\" rx=\"1\"/>",
  "trash": "<path d=\"M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4M6.8 6.5v4.5M9.2 6.5v4.5\"/>",
  "edit": "<path d=\"M10.5 2.5l3 3L6 13H3v-3z\"/>",
  "copy": "<rect x=\"5\" y=\"5\" width=\"8.5\" height=\"8.5\" rx=\"1.2\"/><path d=\"M3 10.5V3.8A1.3 1.3 0 0 1 4.3 2.5H10.5\"/>",
  "chev": "<path d=\"M4 6l4 4 4-4\"/>",
  "sessions": "<rect x=\"2\" y=\"2.5\" width=\"12\" height=\"11\" rx=\"1.5\"/><path d=\"M4.5 6l2 2-2 2M8 10.5h3.5\"/>",
  "macro": "<path d=\"M9 1.8 3.5 9h4L7 14.2 12.5 7h-4z\"/>",
  "down": "<path d=\"M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10\"/>",
  "up": "<path d=\"M8 10.5v-8M4.5 6 8 2.5 11.5 6M3 13.5h10\"/>",
  "kbd": "<rect x=\"1.5\" y=\"4\" width=\"13\" height=\"8.5\" rx=\"1.5\"/><path d=\"M4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6\"/>",
  "sun": "<circle cx=\"8\" cy=\"8\" r=\"2.8\"/><path d=\"M8 1.5v1.5M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1 1M11.6 11.6l1 1M3.4 12.6l1-1M11.6 4.4l1-1\"/>",
  "moon": "<path d=\"M13.2 9.8A5.6 5.6 0 0 1 6.2 2.8a5.6 5.6 0 1 0 7 7z\"/>",
  "reset": "<path d=\"M2.8 6.2A5.3 5.3 0 1 1 3 10M2.5 2.8v3.6h3.6\"/>",
  "more": "<path d=\"M3.5 8h.01M8 8h.01M12.5 8h.01\" stroke-width=\"2.6\"/>",
  "send": "<path d=\"M2.5 8h9M8 4.5 11.5 8 8 11.5M13.5 3v10\"/>",
  "file": "<path d=\"M4 1.8h5.5L12.5 5v9.2H4zM9.5 1.8V5h3\"/>",
  "can": "<path d=\"M2 5h12M2 11h12M5 5v6M11 5v6\"/>",
  "eth": "<rect x=\"6\" y=\"1.8\" width=\"4\" height=\"3.4\" rx=\".6\"/><rect x=\"1.8\" y=\"10.8\" width=\"4\" height=\"3.4\" rx=\".6\"/><rect x=\"10.2\" y=\"10.8\" width=\"4\" height=\"3.4\" rx=\".6\"/><path d=\"M8 5.2v2.8M3.8 10.8V8h8.4v2.8\"/>",
  "uds": "<circle cx=\"8\" cy=\"8\" r=\"5.6\"/><path d=\"M8 5v3.2l2 1.4\"/>",
  "psu": "<path d=\"M9 1.8 4 9h4l-1 5.2L12 7H8z\"/>"
};

/** `<svg class="i">…</svg>` markup for innerHTML (trusted constant content only). */
export function icon(id: string, cls = 'i'): string {
  return `<svg class="${cls}" viewBox="0 0 16 16" aria-hidden="true">${ICONS[id] ?? ''}</svg>`;
}
