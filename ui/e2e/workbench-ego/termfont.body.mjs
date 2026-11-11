/* eslint-disable */
// Ego scenario body "termfont" — run via: ui/e2e/workbench-ego/run.sh <build-label> termfont
// Separates viewport-emulation artifacts from the xterm WebGL atlas font: native (no emulation),
// DPR1 and DPR2 overrides, each set BEFORE login; plus a Canvas ctx.font acceptance probe.
const fontProbe = () => page.evaluate(() => {
  const c = document.createElement("canvas").getContext("2d");
  const fam = "ui-monospace, Menlo, Consolas, monospace";
  const out = {};
  for (const f of [` normal 26px ${fam}`, `normal 26px ${fam}`, ` bold 26px ${fam}`, `italic normal 26px ${fam}`, " normal 13px monospace", "normal 13px monospace"]) {
    c.font = "10px sans-serif"; c.font = f;
    out[JSON.stringify(f)] = { accepted: c.font !== "10px sans-serif", applied: c.font, wPx: Math.round(c.measureText("W").width * 10) / 10 };
  }
  return { ua: navigator.userAgent, dpr: devicePixelRatio, fonts: out };
});
const mode = async (name, setup) => {
  await setup();
  await login("/p/uart/terminal?channel=" + name);
  await page.waitForTimeout(300);
  ok(`uart ${name}`, await call("uart.open", { target: "sim://bootshell?split=0", alias: name, profile: "linux-shell" }));
  check(`${name} boots`, (await call("uart.expect", { channel: name, patterns: [{ kind: "literal", pattern: "login:" }], timeout_ms: 8000 }))?.ok === true);
  await page.waitForTimeout(3000);
  const tm = await termMetrics(); const t0 = tm[tm.length - 1];
  const cssW = t0?.canvases?.[0]?.css?.[0] ?? 0, boxW = t0?.xterm?.[0] ?? 1;
  step(`${name} screen/container width ratio`, String(Math.round((cssW / boxW) * 100) / 100));
  step(`${name} probe`, await fontProbe());
  step(`${name} term`, await termMetrics());
  await shot(`termfont-${name}`);
};
await run("termfont", async () => {
  await mode("native", () => page.cdp("Emulation.clearDeviceMetricsOverride"));
  // native, then a later resize via panel maximize (fit recalculation path)
  await attempt("native maximize", async () => { await page.click('button:has-text("최대화") >> nth=0', { label: "maximize terminal" }); await page.waitForTimeout(2500); });
  step("native maximized term", await termMetrics()); await shot("termfont-native-maximized");
  await mode("dpr1", () => viewport(1440, 900));
  await mode("dpr2", () => page.cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false }));
  await page.cdp("Emulation.clearDeviceMetricsOverride");
});
