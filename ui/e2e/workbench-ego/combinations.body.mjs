/* eslint-disable */
// Ego scenario body "combinations" (ui-combo-qa, ownership 3 of ui-active-interfaces) — run via:
//   ui/e2e/workbench-ego/run.sh <frozen-label> combinations
// Active-interface workbench acceptance against the ACTUAL sim daemon (own harness loomd, agent RPC
// opens real sim devices; no mocks). Each subset case gets a fresh daemon (fresh origin => clean
// localStorage). Native DPR 2 viewport; every measurement/screenshot follows a 2×rAF callback fence.
// Aliases are deliberately NOT the legacy preset names (dut/ch0/bcm/lan1/bench), so any invented
// fallback target is visible as a tab/alias outside the active set. Every expectation is a check().
const TITLE = { "UART 터미널": "uart", "UART 트레이스": "uart", "CAN 트레이스": "can", "이더넷 패킷": "eth", "UDS 콘솔": "uds", "벤치 전원 (PSU)": "psu", "테스트 실행": "test" };
const OPEN = {
  uart: (a) => call("uart.open", { target: "sim://bootshell?split=0", alias: a, profile: "linux-shell" }),
  can: (a) => call("can.open", { channel: a }),
  eth: (a) => call("eth.capture.start", { iface: "sim0", ch: a, backend: "sim" }),
  uds: (a) => call("uds.connect", { name: a, sim: "bench" }),
  psu: (a) => call("psu.connect", { ch: a }),
};
const CLOSE = { uart: (a) => call("uart.close", { channel: a }), can: (a) => call("can.close", { channel: a }), eth: (a) => call("eth.capture.stop", { ch: a }) };
const open = async (d, a) => ok(`open ${d}/${a}`, await OPEN[d](a));
const close = async (d, a) => ok(`close ${d}/${a}`, await CLOSE[d](a));
const key = ([d, a]) => `${d}/${a}`;
const eqset = (a, b) => a.length === b.length && [...a].sort().join("|") === [...b].sort().join("|");
const parseTab = (t) => { const i = t.indexOf(" · "); const base = i < 0 ? t : t.slice(0, i); return { base, domain: TITLE[base] ?? `?${base}`, alias: i < 0 ? null : t.slice(i + 3) }; };
const tabKey = (t) => { const p = parseTab(t); return `${p.domain}/${p.alias}`; };

// render fence: two nested requestAnimationFrame callbacks completed in the page (a 2×rAF callback
// fence — evidence that the renderer ran frame callbacks, not proof of physical paint). Ego rAF
// cadence can be ~1 s. A fence that does not complete is a FAIL check, never a silent pass.
const fence = async (pg = page, label = "") => {
  const t0 = Date.now();
  const r = await Promise.race([
    pg.evaluate(() => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(() => res("raf2"))))),
    new Promise((res) => setTimeout(() => res("timeout"), 6000)),
  ]).catch((e) => `err ${String(e?.message ?? e).slice(0, 80)}`);
  if (r !== "raf2") check(`2xrAF fence ${label}`, false, `${r} after ${Date.now() - t0} ms`);
  return r === "raf2";
};
const snap = async (pg = page) => { await fence(pg, "snap"); return pg.evaluate(() => {
  const tog = document.querySelector(".explorer-inactive-toggle");
  const stop = document.querySelector('[data-testid="stop-button"]')?.getBoundingClientRect();
  const dock = document.querySelector('[data-testid="layout"]');
  const sel = document.querySelector('select[aria-label="레이아웃"]');
  const act = document.activeElement;
  return {
    dpr: devicePixelRatio, w: innerWidth,
    tabs: [...document.querySelectorAll(".dv-tab")].map((t) => t.textContent.trim()),
    focusedTab: (document.querySelector(".dv-groupview.dv-active-group .dv-tab.dv-active-tab") ?? document.querySelector(".dv-tab.dv-active-tab"))?.textContent.trim() ?? null,
    panels: [...document.querySelectorAll("[data-testid=layout] [data-panel]")].map((p) => p.dataset.panel),
    dvGroups: document.querySelectorAll("[data-testid=layout] .dv-groupview").length,
    chans: [...document.querySelectorAll(".channel-explorer [data-chan]")].map((c) => `${c.dataset.chan}:${c.dataset.state}`),
    groups: [...document.querySelectorAll(".channel-explorer .explorer-group")].map((g) => g.getAttribute("aria-label")),
    heading: document.querySelector(".explorer-heading span")?.textContent ?? null,
    inactive: tog ? { text: tog.textContent.trim(), expanded: tog.getAttribute("aria-expanded") } : null,
    welcome: !!document.querySelector('[data-testid="workbench-welcome"]'),
    dockEmpty: !!document.querySelector('[data-testid="dock-empty"]'),
    placeholders: [...document.querySelectorAll("[data-testid=layout] .panel-empty")].map((e) => e.textContent.trim().slice(0, 80)),
    noTarget: (dock?.innerText ?? "").includes("대상 없음"),
    layout: sel ? { value: sel.value, opts: [...sel.querySelectorAll("option")].map((o) => ({ v: o.value, l: o.textContent.trim(), g: o.parentElement?.label ?? "", d: o.disabled })) } : null,
    route: location.hash.startsWith("#/") ? location.hash : "(boot)",
    focus: act ? { cls: String(act.className).slice(0, 40), panel: act.closest?.("[data-panel]")?.dataset.panel ?? null } : null,
    stopInView: !!stop && stop.width > 0 && stop.right <= innerWidth && stop.bottom <= innerHeight && stop.x >= 0 && stop.y >= 0,
    hscroll: document.documentElement.scrollWidth > innerWidth,
  };
}); };
const until = async (pred, ms = 10000, pg = page) => { const end = Date.now() + ms; let st; do { st = await snap(pg); if (pred(st)) return st; await pg.waitForTimeout(400); } while (Date.now() < end); return st; };
const fshot = async (name, pg = page) => { await fence(pg, name); const p = `${S}/${name}.png`; await pg.screenshot({ path: p }); step("shot", p); };
const fresh = async () => { await ctl("/close"); await ctl("/start"); };

/** strict: default workspace / explorer show EXACTLY the active set (inactive disclosure collapsed) */
const expectActive = (name, st, active, { views = true } = {}) => {
  const want = active.map(key);
  check(`${name} dpr2`, st.dpr === 2, String(st.dpr));
  check(`${name} explorer primary exact`, st.inactive?.expanded !== "true" && eqset(st.chans.map((c) => c.replace(/:[a-z]+$/, "")), want) && st.chans.every((c) => c.endsWith(":open")), { chans: st.chans, want });
  check(`${name} explorer groups exact`, eqset(st.groups, [...new Set(active.map(([d]) => `${d.toUpperCase()} 채널`))]), st.groups);
  check(`${name} heading = active count`, st.heading === String(want.length), st.heading);
  const foreign = st.tabs.filter((t) => !want.includes(tabKey(t)));
  check(`${name} no tab outside active set`, foreign.length === 0, foreign);
  if (views) { const missing = want.filter((w) => !st.tabs.some((t) => tabKey(t) === w)); check(`${name} every active device has a view`, missing.length === 0, missing); }
  check(`${name} no placeholders/대상 없음`, !st.placeholders.length && !st.noTarget, { placeholders: st.placeholders, noTarget: st.noTarget });
  check(`${name} welcome ${want.length ? "hidden" : "shown"}`, st.welcome === !want.length);
  // default workspace = one tab group (root tabs); background arrivals must not split into per-domain columns
  if (st.route === "#/l/m1-default" || st.route === "(boot)") check(`${name} default workspace max 1 dock group`, st.dvGroups <= 1, { dvGroups: st.dvGroups, route: st.route });
  check(`${name} stop-in-view`, st.stopInView); check(`${name} no-hscroll`, !st.hscroll);
};

/** every offered layout choice is enabled and renders only active devices (none empty / foreign) */
const exerciseLayouts = async (name, active) => {
  const want = active.map(key), domains = new Set(active.map(([d]) => d));
  const st0 = await snap();
  step(`${name} layout options`, st0.layout);
  if (!check(`${name} layout picker present`, !!st0.layout)) return;
  check(`${name} no disabled layout choice`, st0.layout.opts.every((o) => !o.d), st0.layout.opts.filter((o) => o.d));
  check(`${name} no test choice without session`, !st0.layout.opts.some((o) => o.v === "test"), st0.layout.opts.map((o) => o.v));
  for (const o of st0.layout.opts.filter((x) => x.v && x.v !== "m1-default" && x.g !== "저장됨")) {
    await page.selectOption('select[aria-label="레이아웃"]', o.v);
    // bounded hydration wait (8 s): new panels must show their actual target, else FAIL below
    const st = await until((s) => s.route === `#/l/${o.v}` && s.tabs.length > 0 && !s.noTarget && !s.placeholders.length, 8000);
    const foreign = st.tabs.filter((t) => !want.includes(tabKey(t)));
    step(`${name} layout ${o.v}`, { tabs: st.tabs, route: st.route });
    check(`${name} layout "${o.v}" only active devices, non-empty`, st.tabs.length > 0 && !foreign.length && !st.placeholders.length && !st.noTarget, { foreign, placeholders: st.placeholders, noTarget: st.noTarget, n: st.tabs.length });
    check(`${name} layout "${o.v}" domains eligible`, st.tabs.every((t) => domains.has(parseTab(t).domain)), st.tabs);
  }
  await page.evaluate(() => { location.hash = "#/l/m1-default"; }); await page.waitForTimeout(500);
  expectActive(`${name} back-to-default`, await until((s) => s.tabs.length >= want.length && !s.noTarget, 8000), active);
};

const COMBOS = [
  ["uart", [["uart", "mcu"], ["uart", "lnx"]]],
  ["can", [["can", "pt"]]],
  ["eth", [["eth", "mirror"]]],
  ["uart-eth", [["uart", "mcu"], ["eth", "mirror"]]],
  ["eth-can", [["eth", "mirror"], ["can", "pt"]]],
  ["uart-can", [["uart", "mcu"], ["can", "pt"]]],
  ["all3", [["uart", "mcu"], ["uart", "lnx"], ["can", "pt"], ["eth", "mirror"]]],
  ["all5", [["uart", "mcu"], ["can", "pt"], ["eth", "mirror"], ["uds", "gw"], ["psu", "rail"]]],
];

await run("combinations", async () => {
  // ---------------------------------------------------------------- subset matrix (snapshot path)
  let first = true;
  for (const [name, active] of COMBOS) {
    if (!first) await fresh(); first = false;
    for (const [d, a] of active) await open(d, a);
    await viewport(1440, 900);
    await login("/l/m1-default");
    const st = await until((s) => active.every((x) => s.chans.includes(`${key(x)}:open`)) && s.tabs.length >= active.length && !s.noTarget, 10000);
    step(`${name} state`, st);
    expectActive(name, st, active);
    await fshot(`${name}-1440`);
    await exerciseLayouts(name, active);
    if (name === "all3" || name === "uart") for (const w of [768, 390]) {
      await viewport(w, 900); await page.waitForTimeout(300);
      const s = await snap(); check(`${name}-${w} stop-in-view`, s.stopInView); check(`${name}-${w} no-hscroll`, !s.hscroll); await fshot(`${name}-${w}`);
    }
    if (name === "all5") check("all5 psu output off still connected (not closed)", st.chans.includes("psu/rail:open"), st.chans);
  }

  // ---------------------------------------------------------------- live transitions (one daemon)
  await fresh(); await viewport(1440, 900);
  await login("/l/m1-default");
  { const st = await until((s) => s.welcome, 6000); step("live empty", st); expectActive("live empty", st, []); await exerciseLayouts("live empty", []); await fshot("live-empty-1440"); }

  await open("uart", "mcu");
  let st = await until((s) => s.tabs.some((t) => tabKey(t) === "uart/mcu"), 10000);
  expectActive("live +uart/mcu", st, [["uart", "mcu"]]);
  // focus the mcu terminal; a later auto-added device must not steal tab or keyboard focus
  await page.click('[data-chan="uart/mcu"]', { label: "focus mcu" }); await page.waitForTimeout(300);
  await page.focus('[data-panel="uart.terminal"] .xterm-helper-textarea');
  await page.evaluate(() => { window.__qa = { uart: document.querySelector('[data-testid=layout] [data-panel="uart.terminal"]') }; });
  await open("can", "pt");
  st = await until((s) => s.tabs.some((t) => tabKey(t) === "can/pt"), 10000);
  expectActive("live +can/pt", st, [["uart", "mcu"], ["can", "pt"]]);
  check("autoopen keeps focused tab", st.focusedTab === "UART 터미널 · mcu", st.focusedTab);
  check("autoopen keeps terminal input focus", st.focus?.panel === "uart.terminal" && /xterm-helper-textarea/.test(st.focus?.cls ?? ""), st.focus);
  await fshot("live-uart-can-1440");

  // unsent CAN draft (never sent: no Enter) must survive unrelated open/close/heartbeat
  const DRAFT = "0x123#11 22";
  await page.click('[data-chan="can/pt"]', { label: "show can pt" }); await page.waitForTimeout(400);
  await page.fill('[data-testid=layout] [data-panel="can.trace"] [data-testid="can-send-input"]', DRAFT);
  await page.evaluate(() => { window.__qa.can = document.querySelector('[data-testid=layout] [data-panel="can.trace"]'); });
  const canKept = (pg = page) => pg.evaluate((dr) => { const el = document.querySelector('[data-testid=layout] [data-panel="can.trace"]'); return { same: !!el && el === window.__qa?.can, draft: el?.querySelector('[data-testid="can-send-input"]')?.value ?? null, want: dr }; }, DRAFT);

  await open("eth", "mirror");
  st = await until((s) => s.tabs.some((t) => tabKey(t) === "eth/mirror"), 10000);
  expectActive("live +eth/mirror", st, [["uart", "mcu"], ["can", "pt"], ["eth", "mirror"]]);
  check("eth open keeps focused CAN tab", st.focusedTab === "CAN 트레이스 · pt", st.focusedTab);
  { const k = await canKept(); check("eth open keeps CAN instance + draft", k.same && k.draft === DRAFT, k); }

  await close("uart", "mcu");
  st = await until((s) => !s.tabs.some((t) => tabKey(t) === "uart/mcu") && !s.chans.some((c) => c.startsWith("uart/mcu")), 10000);
  expectActive("live -uart/mcu", st, [["can", "pt"], ["eth", "mirror"]]);
  check("closed uart disclosed as 1 inactive (collapsed)", st.inactive?.expanded === "false" && /1/.test(st.inactive?.text ?? ""), st.inactive);
  check("uart close keeps focused CAN tab", st.focusedTab === "CAN 트레이스 · pt", st.focusedTab);
  { const k = await canKept(); check("uart close keeps CAN instance + draft", k.same && k.draft === DRAFT, k); }
  await fshot("live-uart-closed-1440");
  if (await attempt("open inactive disclosure", () => page.click(".explorer-inactive-toggle", { label: "show inactive" }))) {
    await page.waitForTimeout(300); const s = await snap();
    check("disclosure shows closed uart/mcu history", s.inactive?.expanded === "true" && s.chans.includes("uart/mcu:closed"), { chans: s.chans, inactive: s.inactive });
    check("disclosure does not add tabs", eqset(s.tabs, st.tabs), s.tabs);
    await fshot("live-inactive-disclosed-1440");
    await page.click(".explorer-inactive-toggle", { label: "hide inactive" }); await page.waitForTimeout(300);
    check("disclosure collapses back", (await snap()).inactive?.expanded === "false");
  }
  await page.waitForTimeout(4000); // several heartbeats / unrelated store ticks
  { const s = await snap(); check("heartbeat: tabs unchanged", eqset(s.tabs, st.tabs), s.tabs); const k = await canKept(); check("heartbeat: CAN instance + draft kept", k.same && k.draft === DRAFT, k); }

  await open("uart", "mcu");
  st = await until((s) => s.tabs.some((t) => tabKey(t) === "uart/mcu"), 10000);
  expectActive("live reopen uart/mcu", st, [["uart", "mcu"], ["can", "pt"], ["eth", "mirror"]]);
  check("reopen: no inactive disclosure left", st.inactive === null, st.inactive);
  check("reopen keeps focused CAN tab", st.focusedTab === "CAN 트레이스 · pt", st.focusedTab);
  { const k = await canKept(); check("reopen keeps CAN instance + draft", k.same && k.draft === DRAFT, k); }
  { const n = st.tabs.filter((t) => t === "UART 터미널 · mcu").length; check("reopen: single mcu terminal tab", n === 1, st.tabs); }
  // second UART alias live: both distinct
  await open("uart", "lnx");
  st = await until((s) => s.tabs.some((t) => tabKey(t) === "uart/lnx"), 10000);
  expectActive("live +uart/lnx", st, [["uart", "mcu"], ["uart", "lnx"], ["can", "pt"], ["eth", "mirror"]]);
  await fshot("live-reopen-1440");

  // saved layout: stored data intact; only available panes applied; restored on reopen
  await page.fill('input[aria-label="저장할 레이아웃 이름"]', "qa-active");
  await page.press('input[aria-label="저장할 레이아웃 이름"]', "Enter"); await page.waitForTimeout(400);
  const stored = () => page.evaluate(() => localStorage.getItem("loom.layout.qa-active"));
  const saved0 = await stored();
  check("saved layout stored with CAN pane", !!saved0 && saved0.includes('"can.trace"') && saved0.includes('"pt"'), (saved0 ?? "null").slice(0, 120));
  await close("can", "pt");
  await page.selectOption('select[aria-label="레이아웃"]', "qa-active"); await page.waitForTimeout(600);
  st = await until((s) => !s.tabs.some((t) => tabKey(t) === "can/pt"), 8000);
  step("saved w/o can", st);
  check("saved layout route selected", st.route === "#/l/qa-active", st.route);
  expectActive("saved w/o can", st, [["uart", "mcu"], ["uart", "lnx"], ["eth", "mirror"]]);
  check("saved layout data intact after filtered apply", (await stored()) === saved0);
  await fshot("saved-filtered-1440");
  await open("can", "pt");
  st = await until((s) => s.tabs.some((t) => tabKey(t) === "can/pt"), 10000);
  step("saved after can reopen", st.tabs);
  check("saved layout: reopened CAN pane available again", st.tabs.some((t) => tabKey(t) === "can/pt"), st.tabs);
  check("saved layout data still intact", (await stored()) === saved0);
  await page.evaluate(() => { location.hash = "#/l/m1-default"; }); await page.waitForTimeout(500);

  // close eth, then reload: retained history not in primary
  await close("eth", "mirror");
  const LIVE = [["uart", "mcu"], ["uart", "lnx"], ["can", "pt"]];
  st = await until((s) => !s.tabs.some((t) => tabKey(t) === "eth/mirror"), 10000);
  expectActive("live -eth/mirror", st, LIVE);
  await page.reload(); await visible('[data-testid="loom-shell"]', 10000);
  st = await until((s) => LIVE.every((x) => s.chans.includes(`${key(x)}:open`)) && s.tabs.length >= LIVE.length, 10000);
  step("reload", st); expectActive("reload", st, LIVE); await fshot("reload-1440");

  // second viewer (late join) + explicit deep link stays single-target
  const late = await task.newPage(); step("late page", late.label);
  const lshot = async (name) => { await fence(late, name); await late.screenshot({ path: `${S}/${name}.png` }); step("shot", `${S}/${name}.png`); };
  try {
    await late.cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
    await late.goto((await ctl("/login", { route: "/l/m1-default" })).url);
    await late.waitForSelector('[data-testid="loom-shell"]', { state: "visible", timeout: 10000 });
    let ls = await until((s) => LIVE.every((x) => s.chans.includes(`${key(x)}:open`)) && s.tabs.length >= LIVE.length, 10000, late);
    step("late", ls); expectActive("late-join", ls, LIVE);
    check("late-join eth history only behind disclosure", ls.inactive?.expanded === "false", ls.inactive);
    await lshot("late-join-1440");
    // live change observed by both viewers
    await close("uart", "lnx");
    const LIVE2 = [["uart", "mcu"], ["can", "pt"]];
    ls = await until((s) => !s.tabs.some((t) => tabKey(t) === "uart/lnx"), 10000, late);
    expectActive("late-join -uart/lnx", ls, LIVE2);
    expectActive("main -uart/lnx", await until((s) => !s.tabs.some((t) => tabKey(t) === "uart/lnx"), 6000), LIVE2);
    await late.evaluate(() => { location.hash = "#/p/uart/terminal?channel=mcu"; }); await late.waitForTimeout(800);
    ls = await snap(late);
    check("deep link: single requested target", eqset(ls.tabs, ["UART 터미널 · mcu"]), ls.tabs);
    await open("eth", "mirror");
    await late.waitForTimeout(2500);
    ls = await snap(late);
    check("deep link: device open does not auto-add tab", eqset(ls.tabs, ["UART 터미널 · mcu"]), ls.tabs);
    await lshot("late-deeplink-1440");
    st = await until((s) => s.tabs.some((t) => tabKey(t) === "eth/mirror"), 10000);
    expectActive("main default adds reopened eth", st, [...LIVE2, ["eth", "mirror"]]);
  } finally { await late.close(); }

  // STOP remains visible/enforced
  await viewport(390, 900); await page.waitForTimeout(300);
  check("pre-stop-390 stop-in-view", (await snap()).stopInView);
  await page.click('[data-testid="stop-button"]', { label: "press STOP" });
  await visible('[data-testid="stop-panel"]');
  { const r = await call("uart.send", { channel: "mcu", data: "78", eol: "cr" }); step("agent uart after STOP", r); check("STOP blocks agent uart", r?.error?.code === "E_STOPPED"); }
  { const r = await call("can.send", { channel: "pt", frame: "300#77" }); step("agent can after STOP", r); check("STOP blocks agent can", r?.error?.code === "E_STOPPED"); }
  { const s = await snap(); check("stop-390 no-hscroll", !s.hscroll); }
  await fshot("stop-390");
  await viewport(1440, 900); await fshot("stop-1440");
});
