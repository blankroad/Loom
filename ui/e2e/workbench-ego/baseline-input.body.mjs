/* eslint-disable */
// Ego scenario body "baseline-input" — run via: ui/e2e/workbench-ego/run.sh <baseline|final> baseline-input
const tabs = () => page.evaluate(() => [...document.querySelectorAll(".dv-tab")].map((t) => `${t.textContent.trim()}${t.classList.contains("dv-active-tab") ? "*" : ""}`).join(" | "));
const expectOn = async (ch, pat, ms) => { const r = await call("uart.expect", { channel: ch, patterns: [{ kind: "literal", pattern: pat }], timeout_ms: ms }); return r?.ok === true ? "match" : (r?.error?.code ?? JSON.stringify(r).slice(0, 120)); };
const typeInto = async (chip, text) => {
  await page.click(`.channel-bar [data-chan="${chip}"]`, { label: `open ${chip}` });
  await page.waitForTimeout(300);
  const n = await page.evaluate(() => document.querySelectorAll(".xterm-helper-textarea").length);
  step(`textareas after ${chip}`, String(n));
  await page.focus('[data-panel="uart.terminal"] .xterm-helper-textarea');
  await page.keyboard.type(text); await page.keyboard.press("Enter");
};
await run("baseline-input", async () => {
  await viewport(1440, 900);
  await login("/l/m1-default");
  ok("uart dut", await call("uart.open", { target: "sim://bootshell?split=0", alias: "dut", profile: "linux-shell" }));
  ok("uart aux", await call("uart.open", { target: "sim://bootshell?split=0", alias: "aux", profile: "linux-shell" }));
  step("dut login", await expectOn("dut", "login:", 8000));
  step("aux login", await expectOn("aux", "login:", 8000));
  await page.waitForSelector('.channel-bar [data-chan="uart/aux"]', { state: "visible" });
  await typeInto("uart/aux", "root");
  step("aux Password (expect match)", await expectOn("aux", "Password:", 3000));
  step("dut Password (expect no match)", await expectOn("dut", "Password:", 1500));
  step("tabs", await tabs());
  await shot("input-aux-typed");
  await typeInto("uart/dut", "qadut");
  step("dut Password (expect match)", await expectOn("dut", "Password:", 3000));
  step("chip after dut input", await page.evaluate(() => document.querySelector('[data-testid="input-chip"]')?.textContent ?? "none"));
  await shot("input-dut-typed");
  await page.click('.channel-bar [data-chan="uart/aux"]', { label: "reopen aux" }); await page.waitForTimeout(300);
  step("tabs after reopen aux", await tabs());
  await shot("reopen-aux");
  await page.reload(); await visible('[data-testid="loom-shell"]', 10000); await page.waitForTimeout(800);
  step("tabs after reload", await tabs());
  await shot("after-reload");
  await login("/p/uart/terminal?channel=aux"); await page.waitForTimeout(800);
  step("deep link aux tabs", await tabs());
  step("hash", await page.evaluate(() => location.hash));
  await shot("deeplink-aux");
});
