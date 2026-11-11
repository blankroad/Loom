/* eslint-disable */
// Ego scenario body "baseline" — run via: ui/e2e/workbench-ego/run.sh <baseline|final> baseline
// Baseline UX: empty states, multi-interface chips, open/focus behavior, input targeting, widths, STOP.
const dock = () => page.evaluate(() => ({
  tabs: [...document.querySelectorAll(".dv-tab")].map((t) => ({ t: t.textContent.trim(), active: t.classList.contains("dv-active-tab") })),
  panels: [...document.querySelectorAll("[data-panel]")].map((p) => p.dataset.panel),
  chips: [...document.querySelectorAll(".channel-bar [data-chan]")].map((c) => `${c.dataset.chan}:${c.dataset.state}:${c.tagName}`),
  hscroll: document.documentElement.scrollWidth > innerWidth,
  w: innerWidth,
}));
await run("baseline", async () => {
  await viewport(1440, 900);
  await login("/l/m1-default");
  step("empty-default", await dock());
  for (const w of [1024, 768, 390]) { await viewport(w, 900); await page.waitForTimeout(300); step(`empty-${w}`, await dock()); await shot(`empty-${w}`); }
  await viewport(1440, 900);
  // Agent opens actual sim interfaces (baseline has no UI connect).
  ok("uart dut", await call("uart.open", { target: "sim://bootshell?split=0", alias: "dut", profile: "linux-shell" }));
  ok("uart aux", await call("uart.open", { target: "sim://bootshell?split=0", alias: "aux", profile: "linux-shell" }));
  ok("can ch0", await call("can.open", { channel: "ch0" }));
  step("uds ecu", await call("uds.connect", { name: "ecu", sim: "bench" }));
  step("eth cap", await call("eth.capture.start", { iface: "sim0", ch: "cap", backend: "sim" }));
  await page.waitForTimeout(1500);
  step("after-open", await dock());
  await shot("multi-1440-after-agent-open");
  // Click aux chip, then dut chip, then aux again: distinct panels? focus existing?
  for (const c of ["uart/aux", "uart/dut", "uart/aux", "can/ch0", "eth/cap", "uds/ecu"]) {
    const sel = `.channel-bar [data-chan="${c}"]`;
    if (await page.evaluate((s) => !!document.querySelector(s) && document.querySelector(s).tagName === "BUTTON", sel)) {
      await page.click(sel, { label: `open ${c}` }); await page.waitForTimeout(400);
      step(`click ${c}`, await dock());
    } else step(`click ${c}`, "no clickable chip");
  }
  await shot("multi-1440-after-clicks");
  // Input targeting: focus aux terminal, log in on aux only.
  await waitText(".xterm-rows", "login:", 8000).catch(() => step("login-prompt", "not seen"));
  const terms = await page.evaluate(() => [...document.querySelectorAll('[data-panel="uart.terminal"]')].map((p) => ({ visible: p.offsetParent !== null })));
  step("terminals", terms);
  for (const w of [1024, 768, 390]) { await viewport(w, 900); await page.waitForTimeout(300); step(`multi-${w}`, await dock()); await shot(`multi-${w}`); }
  await viewport(1440, 900);
  // keyboard focus visibility on top chrome
  await page.click('[data-testid="rights"]', { label: "focus chrome start" });
  for (let i = 0; i < 4; i++) await page.keyboard.press("Tab");
  step("focused", await page.evaluate(() => { const a = document.activeElement; return { tag: a.tagName, cls: a.className, txt: (a.textContent || a.getAttribute("aria-label") || "").slice(0, 40), outline: getComputedStyle(a).outlineStyle + " " + getComputedStyle(a).outlineWidth }; }));
  await shot("focus-ring-1440");
  await page.click('[data-testid="stop-button"]', { label: "press STOP" });
  await visible('[data-testid="stop-panel"]');
  step("agent after STOP", await call("uart.send", { channel: "aux", data: "78", eol: "cr" }));
  await shot("stop-1440");
  await viewport(390, 900); await page.waitForTimeout(300); step("stop-390", await dock()); await shot("stop-390");
});
