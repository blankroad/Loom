/* eslint-disable */
// Ego scenario body "final" — run via: ui/e2e/workbench-ego/run.sh <build-label> final
// Integrated build. UI-driven connect (2 UART + CAN/UDS/ETH), explorer, distinct panels, reopen focus,
// channel-scoped input/data, explicit saved-layout reload, late join, widths, STOP. Every acceptance
// expectation is a check(); any failed check => RESULT FAIL.
const state = (pg = page) => pg.evaluate(() => ({
  tabs: [...document.querySelectorAll(".dv-tab")].map((t) => `${t.textContent.trim()}${t.classList.contains("dv-active-tab") ? "*" : ""}`).join(" | "),
  chans: [...document.querySelectorAll("[data-chan]")].map((c) => `${c.dataset.chan}:${c.dataset.state}`).join(" "),
  welcome: !!document.querySelector('[data-testid="workbench-welcome"]'),
  hscroll: document.documentElement.scrollWidth > innerWidth,
  stopInView: (() => { const r = document.querySelector('[data-testid="stop-button"]')?.getBoundingClientRect(); return !!r && r.width > 0 && r.right <= innerWidth && r.bottom <= innerHeight && r.x >= 0 && r.y >= 0; })(),
  w: innerWidth,
}));
const tabCount = (tabs, title) => tabs.split(" | ").filter((t) => t.replace(/\*$/, "") === title).length;
const expectOn = async (ch, pat, ms) => { const r = await call("uart.expect", { channel: ch, patterns: [{ kind: "literal", pattern: pat }], timeout_ms: ms }); return r?.ok === true ? "match" : (r?.error?.code ?? JSON.stringify(r).slice(0, 160)); };
const connect = async (domain, name) => {
  await page.click("button.interface-add >> nth=0", { label: `add ${domain} ${name}` });
  await visible("dialog.interface-dialog");
  await page.selectOption("dialog.interface-dialog select >> nth=0", domain);
  await page.fill("dialog.interface-dialog input >> nth=0", name);
  await page.click('dialog.interface-dialog button[type="submit"]', { label: `connect ${name}` });
  const closed = await page.waitForFunction(() => !document.querySelector("dialog.interface-dialog[open]"), undefined, { timeout: 8000 }).then(() => true, () => false);
  const err = closed ? "" : await page.evaluate(() => document.querySelector(".interface-error")?.textContent ?? "dialog still open");
  await page.waitForTimeout(500);
  const st = await state(); step(`after connect ${domain}/${name}`, st);
  check(`ui-connect ${domain}/${name}`, closed && st.chans.includes(`${domain}/${name}:open`), err || st.chans);
};
const typeInto = async (pg, chan, text) => {
  await pg.click(`[data-chan="${chan}"]`, { label: `open ${chan}` }); await pg.waitForTimeout(300);
  await pg.focus('[data-panel="uart.terminal"] .xterm-helper-textarea');
  await pg.keyboard.type(text); await pg.keyboard.press("Enter");
};
const widths = async (prefix) => { for (const w of [1024, 768, 390]) { await viewport(w, 900); await page.waitForTimeout(400); const st = await state(); step(`${prefix}-${w}`, st);
  check(`${prefix}-${w} no-hscroll`, !st.hscroll); check(`${prefix}-${w} stop-in-view`, st.stopInView); await shot(`${prefix}-${w}`); } await viewport(1440, 900); await page.waitForTimeout(300); };

await run("final", async () => {
  await viewport(1440, 900);
  await login("/l/m1-default");
  await page.waitForTimeout(800);
  { const st = await state(); step("empty-1440", st); await shot("empty-1440");
    check("empty-no-fake-panels", !/· (dut|ch0|bcm)\b/.test(st.tabs), st.tabs || "(no tabs)");
    check("empty-welcome", st.welcome); check("empty-1440 stop-in-view", st.stopInView); }
  await widths("empty");
  await page.click("button.interface-add >> nth=0", { label: "open connect dialog" }); await visible("dialog.interface-dialog");
  await shot("connect-dialog-1440"); await page.keyboard.press("Escape"); await page.waitForTimeout(200);
  check("dialog-escape-closes", await page.evaluate(() => !document.querySelector("dialog.interface-dialog[open]")));

  // two UART aliases via UI
  await connect("uart", "dut"); await connect("uart", "aux");
  check("dut boots", (await expectOn("dut", "login:", 8000)) === "match");
  check("aux boots", (await expectOn("aux", "login:", 8000)) === "match");
  await page.waitForTimeout(300);
  step("term metrics two-uart", await termMetrics());
  await shot("two-uart-1440");
  // channel-scoped input
  await typeInto(page, "uart/aux", "root");
  check("input aux -> aux", (await expectOn("aux", "Password:", 3000)) === "match");
  check("input aux not -> dut", (await expectOn("dut", "Password:", 1500)) === "E_TIMEOUT");
  await typeInto(page, "uart/dut", "root");
  check("input dut -> dut", (await expectOn("dut", "Password:", 3000)) === "match");
  await typeInto(page, "uart/aux", "secret"); check("aux shell", (await expectOn("aux", "root@sim:~# ", 3000)) === "match");
  await typeInto(page, "uart/dut", "secret"); check("dut shell", (await expectOn("dut", "root@sim:~# ", 3000)) === "match");
  // channel-scoped data
  ok("agent dut marker", await call("uart.send", { channel: "dut", data: Buffer.from("echo qa-dut-only").toString("hex"), eol: "cr" }));
  check("data dut marker on dut", (await expectOn("dut", "qa-dut-only", 3000)) === "match");
  check("data dut marker not on aux", (await expectOn("aux", "qa-dut-only", 1500)) === "E_TIMEOUT");
  // repeated open focuses existing slot (no duplicates)
  for (const c of ["uart/aux", "uart/dut", "uart/aux", "uart/aux"]) {
    await page.click(`[data-chan="${c}"]`, { label: `reopen ${c}` }); await page.waitForTimeout(250);
    const tabs = (await state()).tabs; const title = `UART 터미널 · ${c.split("/")[1]}`;
    check(`reopen ${c} focuses single tab`, tabCount(tabs, title) === 1 && tabs.includes(`${title}*`), tabs);
  }
  await shot("reopen-aux-1440");
  const views = await page.evaluate(() => [...document.querySelectorAll("[data-view]")].map((b) => b.getAttribute("aria-label")).join(" | "));
  check("per-channel views listed", /aux 터미널/.test(views) && /aux 트레이스/.test(views) && /dut 트레이스/.test(views), views);

  await connect("can", "ch0"); await connect("uds", "ecu"); await connect("eth", "cap");
  await page.waitForTimeout(1000);
  for (const c of ["can/ch0", "uds/ecu", "eth/cap"]) await attempt(`open ${c}`, async () => { await page.click(`[data-chan="${c}"]`, { label: `open ${c}` }); await page.waitForTimeout(500); });
  { const st = await state(); step("all", st); await shot("multi-domains-1440");
    for (const t of ["UART 터미널 · dut", "UART 터미널 · aux", "CAN 트레이스 · ch0", "UDS 콘솔 · ecu", "이더넷 패킷 · cap"]) check(`distinct panel "${t}"`, tabCount(st.tabs, t) === 1, st.tabs); }
  await attempt("open uds/ecu tab", () => page.click('.dv-tab:has-text("UDS 콘솔 · ecu")', { label: "show ecu tab" }));
  await page.waitForTimeout(400);
  { const t = await page.evaluate(() => (document.querySelector('[data-panel="uds.console"]')?.textContent ?? "none").slice(0, 160)); step("uds ecu panel", t);
    check("uds ecu panel shows connected target", !t.includes("대상 없음"), t); }
  await page.fill('input[aria-label="채널 검색"]', "aux"); await page.waitForTimeout(200);
  { const r = await page.evaluate(() => [...document.querySelectorAll("[data-chan]")].map((c) => c.dataset.chan).join(" ")); check("search aux filters", r === "uart/aux", r); }
  await shot("search-aux-1440"); await page.fill('input[aria-label="채널 검색"]', "");
  await widths("multi");
  // keyboard focus ring
  await page.focus('input[aria-label="채널 검색"]'); await page.keyboard.press("Tab"); await page.keyboard.press("Tab");
  { const f = await page.evaluate(() => { const a = document.activeElement, s = getComputedStyle(a); return { el: `${a.tagName}.${a.className}`, label: (a.getAttribute("aria-label") || a.textContent || "").slice(0, 40), outline: `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}`, shadow: s.boxShadow }; });
    step("focused", f); check("visible focus ring", f.outline.startsWith("solid") || (f.shadow && f.shadow !== "none"), f.outline); }
  await shot("focus-1440");
  // explicit persistence: save -> select saved layout -> reload
  step("tabs before save", (await state()).tabs);
  await page.fill('input[aria-label="저장할 레이아웃 이름"]', "qa-bench");
  await page.press('input[aria-label="저장할 레이아웃 이름"]', "Enter"); await page.waitForTimeout(400);
  await page.selectOption('select[aria-label="레이아웃"]', "qa-bench"); await page.waitForTimeout(600);
  check("saved layout selected", (await page.evaluate(() => location.hash)) === "#/l/qa-bench");
  await page.reload(); await visible('[data-testid="loom-shell"]', 10000); await page.waitForTimeout(1200);
  { const after = (await state()).tabs; check("saved-layout-reload keeps alias tabs", ["UART 터미널 · dut", "UART 터미널 · aux"].every((t) => tabCount(after, t) === 1), after); }
  await shot("saved-layout-reload-1440");

  // late join: second viewer tab after all interfaces exist
  const late = await task.newPage(); step("late page", late.label);
  try {
    await late.cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
    await late.goto((await ctl("/login", { route: "/l/m1-default" })).url);
    await late.waitForSelector('[data-testid="loom-shell"]', { state: "visible", timeout: 10000 }); await late.waitForTimeout(1500);
    const ex = (await state(late)).chans; step("late explorer", ex);
    for (const c of ["uart/dut", "uart/aux", "can/ch0", "uds/ecu", "eth/cap"]) check(`late-join ${c} listed open`, ex.includes(`${c}:open`), ex);
    for (const c of ["uart/aux", "uart/dut", "uds/ecu", "eth/cap", "can/ch0"]) await attempt(`late open ${c}`, async () => { await late.click(`[data-chan="${c}"]`, { label: `late open ${c}` }); await late.waitForTimeout(500); });
    const lt = (await state(late)).tabs; step("late tabs", lt);
    for (const t of ["UART 터미널 · dut", "UART 터미널 · aux"]) check(`late distinct "${t}"`, tabCount(lt, t) === 1, lt);
    if (await attempt("late focus aux", async () => { await late.click('[data-chan="uart/aux"]', { label: "late focus aux" }); await late.waitForTimeout(300); await late.focus('[data-panel="uart.terminal"] .xterm-helper-textarea'); })) {
      const hdr = await late.evaluate(() => (document.querySelector('[data-panel="uart.terminal"]')?.innerText ?? "").split("\n").slice(0, 3).join(" ").slice(0, 160));
      step("late aux header", hdr);
      check("late aux header hydrated (alias + 115200)", /aux/.test(hdr) && /115200/.test(hdr), hdr);
      await late.keyboard.type("echo late-aux"); await late.keyboard.press("Enter");
      check("late input aux -> aux", (await expectOn("aux", "late-aux", 3000)) === "match");
      check("late input aux not -> dut", (await expectOn("dut", "late-aux", 1500)) === "E_TIMEOUT");
    }
    await late.screenshot({ path: `${S}/late-join-1440.png` }); step("shot", `${S}/late-join-1440.png`);
  } finally { await late.close(); }

  // STOP at 390 + enforcement
  await viewport(390, 900); await page.waitForTimeout(300);
  check("pre-stop-390 stop-in-view", (await state()).stopInView);
  await page.click('[data-testid="stop-button"]', { label: "press STOP at 390" });
  await visible('[data-testid="stop-panel"]');
  { const r = await call("uart.send", { channel: "aux", data: "78", eol: "cr" }); step("agent uart after STOP", r); check("STOP blocks agent uart", r?.error?.code === "E_STOPPED"); }
  { const r = await call("can.send", { channel: "ch0", frame: "300#77" }); step("agent can after STOP", r); check("STOP blocks agent can", r?.error?.code === "E_STOPPED"); }
  { const st = await state(); step("stop-390", st); check("stop-390 no-hscroll", !st.hscroll); }
  await shot("stop-390");
  await viewport(1440, 900); await page.waitForTimeout(300); await shot("stop-1440");
});
