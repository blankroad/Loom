/* eslint-disable */
// Service module-tab acceptance on a real loomd with sim devices. Run via run.sh.
// Rule (module-apps, supersedes active-only service navigation): seven PERMANENT module tabs (UART, CAN, Ethernet,
// UDS, PSU, logic, video) + Integrated, whatever is connected. data-count = OPEN channels of that service (UDS: real
// UDS connections only). A service without an own open channel shows its UNBOUND module application (one view, no
// alias, no implicit lan1/bench binding); UART's unbound application is the saved-session picker (dock-empty).
// Navigation never creates channels. Integrated holds actual channel views only, never the unbound module apps.
// DOM notes: every module keeps a retained hidden dock, so views are counted ONLY in the front
// [data-testid="layout"] dock. The shell side explorer (.channel-explorer) exists for CAN/PSU/logic/video/Integrated;
// UART's side is its session list (.sess[data-channel], unsaved open channels); ETH/UDS own their toolbar and hide
// the side (no shell explorer) — their actual alias binding is asserted from the front-dock view titles.
const SAME = (a, b) => [...a].sort().join('|') === [...b].sort().join('|') && a.length === b.length;
const fence = async () => {
  const result = await Promise.race([page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))), new Promise(r => setTimeout(() => r(false), 6000))]);
  check('render callback fence', result === true);
};
const state = async (pg = page) => pg.evaluate(() => {
  const stop = document.querySelector('[data-testid="stop-button"]')?.getBoundingClientRect();
  const front = document.querySelector('[data-testid="layout"]');
  const side = document.querySelector('#side');
  const module = document.querySelector('[data-testid="loom-shell"]')?.dataset.module ?? null;
  const shellExplorer = [...document.querySelectorAll('#side .channel-explorer [data-chan]')].map(c => ({key: c.dataset.chan, state: c.dataset.state}));
  const uartSide = [...document.querySelectorAll('#side .sess[data-channel]')].map(c => ({key: `uart/${c.dataset.channel}`, state: 'open'}));
  const explorer = module === 'uart' ? uartSide : shellExplorer;
  return {
    route: location.hash,
    module,
    status: document.querySelector('.shell-status .st-target')?.textContent ?? null,
    services: [...document.querySelectorAll('.service-nav > a[href^="#/s/"]')].map(a => a.getAttribute('href').slice(4)),
    counts: Object.fromEntries([...document.querySelectorAll('.service-nav a[data-service]')].map(a => [a.dataset.service, Number(a.dataset.count)])),
    current: [...document.querySelectorAll('a[aria-current="page"]')].map(a => a.getAttribute('href')),
    sideHidden: !side || side.hidden,
    explorerAll: explorer.map(e => e.key),
    channels: explorer.filter(e => e.state === 'open').map(e => e.key),
    tabs: front ? [...front.querySelectorAll('.dv-tab')].map(t => t.textContent.trim()) : [],
    allTabs: document.querySelectorAll('.dv-tab').length,
    fronts: document.querySelectorAll('[data-testid="layout"]').length,
    groups: front ? front.querySelectorAll('.dv-groupview').length : 0,
    picker: !!document.querySelector('select[aria-label="레이아웃"]'),
    // historical Bench-Light-removed selectors, recorded only to prove they are gone (not applicable, see na())
    relations: [...document.querySelectorAll('.service-relations a')].map(a=>a.getAttribute('href')),
    navGroups: [...document.querySelectorAll('.service-nav-label')].map(n=>n.textContent),
    text: document.querySelector('#modUart .work')?.textContent ?? '',
    empty: !!document.querySelector('[data-testid="dock-empty"]'),
    uartPicker: !!document.querySelector('[data-testid="dock-empty"] [data-testid="uart-empty-pane"]'),
    stop: !!stop && stop.x >= 0 && stop.y >= 0 && stop.right <= innerWidth && stop.bottom <= innerHeight,
    overflow: document.documentElement.scrollWidth > innerWidth,
  };
});
const wait = async (pred, pg = page) => {
  let s; const end = Date.now() + 12000;
  do { s = await state(pg); if (pred(s)) return s; await pg.waitForTimeout(150); } while (Date.now() < end);
  return s;
};
const go = async route => { await page.evaluate(r => { location.hash = r; }, route); return wait(s => s.route === route); };
const capture = async name => { await fence(); await shot(name); };
const na = (name, reason) => step(`N/A ${name}`, reason);
const MODULES = ['uart', 'can', 'eth', 'uds', 'psu', 'logic', 'video'];
const domains = ['uart', 'can', 'eth', 'uds', 'psu'];   // connected via sim in this run
const unconnected = ['logic', 'video'];                 // never connected here: unbound apps throughout
const aliases = {uart:['mcu','aux'], can:['pt'], eth:['mirror'], uds:['gw'], psu:['rail']};
const titles = {uart:'UART', can:'CAN', eth:'이더넷', uds:'UDS', psu:'PSU', logic:'로직', video:'비디오'};
const ownsToolbar = d => d === 'eth' || d === 'uds';
const serviceDomains = d => d === 'uds' ? ['uds','can','eth'] : [d];
const serviceChannels = d => serviceDomains(d).flatMap(domain => aliases[domain].map(a => `${domain}/${a}`));
const tabAlias = t => (t.match(/ · ([^·]+)$/)?.[1] ?? '').trim();
const countsAre = (s, want) => MODULES.every(m => s.counts[m] === (want[m] ?? 0));
function expectPermanentTabs(label, s) {
  check(`${label} seven permanent module tabs`, SAME(s.services, MODULES), s.services);
  check(`${label} exactly one front dock`, s.fronts === 1, s.fronts);
}
function expectFrame(d, s) {
  check(`${d} single readable group`, s.groups === 1, s.groups);
  check(`${d} no integration layout picker`, !s.picker);
  check(`${d} shared STOP visible`, s.stop);
  check(`${d} no page overflow`, !s.overflow);
}
function expectService(d, s) {
  const want = serviceChannels(d);
  expectPermanentTabs(d, s);
  if (ownsToolbar(d)) {
    check(`${d} module-owned toolbar hides shell explorer`, s.sideHidden && s.explorerAll.length === 0, s.explorerAll);
    check(`${d} views bind exactly the actual aliases`, SAME(s.tabs.map(tabAlias), want.map(k => k.split('/')[1])), s.tabs);
  } else {
    check(`${d} scoped explorer`, SAME(s.channels, want), s.channels);
  }
  check(`${d} exact view count`, s.tabs.length === want.length, s.tabs);
  check(`${d} no foreign view`, s.tabs.every(t => serviceDomains(d).some(domain => t.includes(titles[domain]))), s.tabs);
  check(`${d} active aliases`, want.every(key => s.tabs.some(t => t.includes(` · ${key.split('/')[1]}`))), s.tabs);
  check(`${d} no unbound app beside own channel`, s.tabs.every(t => tabAlias(t) !== ''), s.tabs);
  expectFrame(d, s);
}
function expectUnbound(d, s, label) {
  expectPermanentTabs(label, s);
  check(`${label} unbound app is the only view`, s.tabs.length === 1 && s.tabs[0].includes(titles[d]), s.tabs);
  check(`${label} no alias / no implicit binding`, tabAlias(s.tabs[0] ?? '') === '' && !s.tabs.some(t => /lan1|bench/.test(t)), s.tabs);
  check(`${label} not the closed-workbench card`, !s.empty);
  check(`${label} no own open channel`, (s.counts[d] ?? -1) === 0 && !s.channels.some(k => k.startsWith(`${d}/`)), {counts: s.counts, channels: s.channels});
  if (ownsToolbar(d)) check(`${label} module-owned toolbar hides shell explorer`, s.sideHidden);
  expectFrame(label, s);
}
await run('services', async () => {
  await viewport(1440,900); await login('/s/uart');
  let s = await wait(s => s.route === '#/s/uart' && s.empty);
  check('empty service URL retained', s.route === '#/s/uart', s.route);
  expectPermanentTabs('zero channels', s);
  check('zero channels: every count 0', countsAre(s, {}), s.counts);
  check('empty UART is the session picker application', s.uartPicker && s.channels.length === 0 && s.tabs.length === 0, s);
  await capture('uart-empty');
  // every module tab before any connection: full unbound application, navigation only
  for (const d of MODULES.filter(m => m !== 'uart')) {
    await page.click(`.service-nav > a[href="#/s/${d}"]`, {label: `${d} 모듈 열기 (연결 전)`});
    s = await wait(s => s.route === `#/s/${d}` && s.tabs.length === 1);
    step(`${d} unbound state`, s); expectUnbound(d, s, `${d} before connect`);
    await capture(`${d}-unbound`);
  }
  await page.click('a[href="#/l/m1-default"]', {label: '통합 작업 공간 (연결 전)'});
  s = await wait(s => s.route === '#/l/m1-default');
  check('zero-channel Integrated has no module apps stuffed in', s.tabs.length === 0 && s.channels.length === 0, s.tabs);
  check('navigation-only created no channel', countsAre(s, {}) && s.explorerAll.length === 0, {counts: s.counts, explorer: s.explorerAll});
  await go('#/s/uart'); s = await wait(s => s.empty && s.uartPicker);
  check('UART picker retained after tour', s.uartPicker);
  ok('uart mcu', await call('uart.open',{target:'sim://bootshell?split=0',alias:'mcu'}));
  ok('uart aux', await call('uart.open',{target:'sim://bootshell?split=0',alias:'aux'}));
  s = await wait(s => s.channels.length === 2 && s.tabs.length === 2);
  check('UART counts only UART', countsAre(s, {uart: 2}), s.counts);expectService('uart',s);
  await capture('uart-service');
  ok('can pt',await call('can.open',{channel:'pt'}));
  ok('eth mirror',await call('eth.capture.start',{iface:'sim0',ch:'mirror',backend:'sim'}));
  ok('uds gw',await call('uds.connect',{name:'gw',sim:'bench'}));
  ok('psu rail',await call('psu.connect',{ch:'rail'}));
  const live = {uart: 2, can: 1, eth: 1, uds: 1, psu: 1};
  s=await wait(s=>countsAre(s, live));
  check('connected counts truthful (UDS = real UDS only)',countsAre(s, live),s.counts);
  expectService('uart',s);check('unrelated events keep UART route',s.route==='#/s/uart');
  for(const d of domains){
    await page.click(`.service-nav > a[href="#/s/${d}"]`,{label:`${d} 서비스 열기`});
    s=await wait(s=>s.route===`#/s/${d}`&&s.tabs.length===serviceChannels(d).length&&(ownsToolbar(d)||SAME(s.channels,serviceChannels(d))));
    step(`${d} service state`,s);expectService(d,s);
    check(`${d} menu retains shared live state`,countsAre(s, live),s.counts);
    if(d==='can'||d==='eth'){na(`${d} links to common diagnostics`,'Bench Light removed .service-relations; UDS reached via permanent tab');check(`${d} historical relation links absent`,s.relations.length===0,s.relations);}
    if(d==='uds'){
      check('UDS diagnostic workspace combines actual CAN and Ethernet observers',SAME(s.tabs.map(tabAlias),['gw','pt','mirror']),s.tabs);
      na('diagnostics classified separately / UDS relation links','Bench Light removed .service-nav-label and .service-relations');
      check('historical diagnostics nav group absent',s.navGroups.length===0&&s.relations.length===0,{navGroups:s.navGroups,relations:s.relations});
    }
    await capture(`${d}-all-connected`);
  }
  for(const d of unconnected){
    await page.click(`.service-nav > a[href="#/s/${d}"]`,{label:`${d} 모듈 열기 (다른 서비스 연결됨)`});
    s=await wait(s=>s.route===`#/s/${d}`&&s.tabs.length===1);
    step(`${d} unbound while others live`,s);expectUnbound(d,s,`${d} while others connected`);
    check(`${d} navigation kept live counts`,countsAre(s, live),s.counts);
  }
  await go('#/s/psu');await wait(s=>s.tabs.length===1&&s.channels.includes('psu/rail'));
  await page.reload();s=await wait(s=>s.route==='#/s/psu'&&s.tabs.length===1&&s.channels.includes('psu/rail'));
  expectService('psu',s);check('reload keeps connected OFF PSU visible',s.channels.includes('psu/rail'));
  await page.click('a[href="#/l/m1-default"]',{label:'통합 작업 공간 열기'});
  const all=domains.flatMap(d=>aliases[d].map(a=>`${d}/${a}`));
  s=await wait(s=>s.route==='#/l/m1-default'&&s.tabs.length===all.length);
  check('integration all real channels',SAME(s.channels,all),s.channels);
  check('integration six views',s.tabs.length===6,s.tabs);
  check('integration views are actual channels only (no unbound module app)',SAME(s.tabs.map(tabAlias),all.map(k=>k.split('/')[1])),s.tabs);
  check('integration layout tools available',s.picker);
  expectPermanentTabs('integration',s);
  await capture('integration');
  await go('#/s/uart');await wait(s=>s.tabs.length===2);
  ok('close mcu',await call('uart.close',{channel:'mcu'}));ok('close aux',await call('uart.close',{channel:'aux'}));
  s=await wait(s=>s.counts.uart===0&&!s.channels.some(k=>k.startsWith('uart/')));
  check('closed UART: count 0, tab stays permanent',s.counts.uart===0&&SAME(s.services,MODULES),{counts:s.counts,services:s.services});
  check('closing UART leaves other live counts',countsAre(s,{...live,uart:0}),s.counts);
  check('closed UART stays honest (no open UART channel; only UART history or the picker)',s.route==='#/s/uart'&&s.channels.length===0&&(s.tabs.length===0?s.uartPicker:s.tabs.every(t=>t.includes('UART'))),s);
  await capture('uart-closed');
  ok('reopen mcu',await call('uart.open',{target:'sim://bootshell?split=0',alias:'mcu'}));
  s=await wait(s=>s.counts.uart===1&&s.channels.includes('uart/mcu'));
  check('reopen existing route restores only UART mcu',SAME(s.channels,['uart/mcu'])&&s.tabs.some(t=>t.includes(' · mcu'))&&s.tabs.every(t=>t.includes('UART')),s);
  const late=await task.newPage();
  try {
    await late.goto((await ctl('/login',{route:'/s/can'})).url);
    await late.waitForSelector('[data-testid="loom-shell"]',{state:'visible',timeout:10000});
    const ls=await wait(s=>s.route==='#/s/can'&&s.channels.includes('can/pt')&&s.tabs.length===1,late);
    check('late join domain scope',SAME(ls.channels,['can/pt'])&&ls.tabs.length===1,ls);
    check('late join seven permanent tabs',SAME(ls.services,MODULES),ls.services);
  } finally {await late.close();}
  for(const width of [768,390]){
    await viewport(width,900);await fence();s=await state();check(`${width} STOP visible`,s.stop);check(`${width} no horizontal page overflow`,!s.overflow);await shot(`uart-${width}`);
  }
  await viewport(1440,900);await go('#/s/can');await wait(s=>s.tabs.length===1);
  await page.click('[data-testid="stop-button"]',{label:'공통 STOP 동작 확인'});
  await page.waitForSelector('.stop-panel',{state:'visible',timeout:8000});
  const blocked=await call('can.send',{channel:'pt',frame:'300#77'});
  check('service STOP enforces shared core latch',blocked?.error?.code==='E_STOPPED'||blocked?.code==='E_STOPPED',blocked);
  await capture('service-stop');
});
console.log('finish',JSON.stringify(await task.finish({keep:[]})));
