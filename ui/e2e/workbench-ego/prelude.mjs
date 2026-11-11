/* eslint-disable */
// Ego scenario prelude (ui-qa). Concatenated by run.sh after `const BUILD, CTL, S` and before a
// <scenario>.body.mjs, then piped to `ego-browser nodejs`. Reads the harness port/secret from the
// per-run 0600 control file; never prints the secret, tokens or bootstrap URLs.
const fs = await import("node:fs/promises");
const { port, secret } = JSON.parse(await fs.readFile(CTL, "utf8"));
const ctl = async (route, body = {}) => {
  const r = await fetch(`http://127.0.0.1:${port}${route}`, { method: "POST", headers: { "x-ctl": secret }, body: JSON.stringify(body) });
  const j = await r.json(); if (!r.ok) throw new Error(`${route}: ${j.error}`); return j;
};
const call = async (method, params) => (await ctl("/call", { method, params })).result;
const step = (k, v) => console.log(k, typeof v === "string" ? v : JSON.stringify(v).slice(0, 500));
const task = await taskSpace("loom ui-workbench qa");
console.log("spaceId", task.spaceId);
const page = task.page("p1");
// DPR 2 = this host's native ratio; a DPR-1 override leaves the xterm WebGL canvas at 2x backing (tiny-glyph artifact, see report).
const viewport = (width, height) => page.cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: false });
const waitText = (sel, text, timeout = 8000) => page.waitForFunction(([s, t]) => [...document.querySelectorAll(s)].some((n) => n.textContent.includes(t)), [sel, text], { timeout });
const visible = (sel, timeout = 8000) => page.waitForSelector(sel, { state: "visible", timeout });
const login = async (route) => {
  await page.goto((await ctl("/login", { route })).url);
  await visible('[data-testid="loom-shell"]', 10000);
  await waitText('[data-testid="rights"]', "operate");
};
const shot = async (name) => { const p = `${S}/${name}.png`; await page.screenshot({ path: p }); step("shot", p); return p; };
const ok = (label, r) => { step(label, r); if (r?.ok !== true && r?.data?.ok !== true) throw new Error(`${label} not ok`); return r; };
// Acceptance checks aggregate: every failed check makes the scenario RESULT FAIL (runner exit 1).
const failures = [];
const check = (name, cond, detail = "") => {
  step(`CHECK ${name}`, `${cond ? "PASS" : "FAIL"}${detail ? " — " + (typeof detail === "string" ? detail : JSON.stringify(detail)) : ""}`);
  if (!cond) failures.push(name);
  return cond;
};
// non-fatal action: record a failed check and continue the scenario
const attempt = async (name, fn) => { try { await fn(); return true; } catch (e) { check(name, false, String(e?.message ?? e).slice(0, 200)); return false; } };
const run = async (name, fn) => {
  try {
    await ctl("/start"); await fn();
    if (failures.length) throw new Error(`${failures.length} check(s) failed: ${failures.join(", ")}`);
    step("RESULT", `${name} PASS`);
  } catch (e) { step("RESULT", `${name} FAIL: ${String(e?.message ?? e).replace(/#boot=[^"\s]*/g, "#boot=<redacted>")}`); try { await shot(`${name}-fail-${Date.now()}`); } catch {} }
  finally { step("close", await ctl("/close").catch((e) => String(e))); }
};
// Terminal render metrics (xterm WebGL): DPR, canvas backing vs CSS box, measured cell, font.
const termMetrics = (pg = page) => pg.evaluate(() => [...document.querySelectorAll('[data-panel="uart.terminal"]')].map((p) => {
  const r = (el) => { if (!el) return null; const b = el.getBoundingClientRect(); return [Math.round(b.width * 10) / 10, Math.round(b.height * 10) / 10]; };
  const m = p.querySelector(".xterm-char-measure-element"), ta = p.querySelector(".xterm-helper-textarea"), x = p.querySelector(".xterm");
  return {
    dpr: devicePixelRatio, inner: [innerWidth, innerHeight],
    xterm: r(x), screen: r(p.querySelector(".xterm-screen")),
    canvases: [...p.querySelectorAll("canvas")].map((c) => ({ cls: c.className, css: r(c), backing: [c.width, c.height], ratio: c.getBoundingClientRect().width ? Math.round((c.width / c.getBoundingClientRect().width) * 100) / 100 : null })),
    measure: m ? { rect: r(m), font: getComputedStyle(m).fontSize, chars: m.textContent.length } : null,
    xtermFont: x ? getComputedStyle(x).fontSize : null, textareaFont: ta ? getComputedStyle(ta).fontSize : null,
    renderer: p.querySelector(".xterm-rows") ? "dom" : "webgl/canvas",
  };
}));
