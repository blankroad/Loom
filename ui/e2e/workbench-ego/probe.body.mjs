/* eslint-disable */
// Ego scenario body "probe" — run via: ui/e2e/workbench-ego/run.sh <baseline|final> probe
await run("probe", async () => {
  await viewport(1440, 900);
  await login("/l/m1-default");
  step("title", await page.title());
  await shot("probe-1440");
});
