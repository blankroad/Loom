/* eslint-disable */
// Service separation acceptance on a real loomd with sim devices. Run via run.sh.
const SAME = (a, b) => [...a].sort().join('|') === [...b].sort().join('|') && a.length === b.length;
const fence = async () => {
  const result = await Promise.race([page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))), new Promise(r => setTimeout(() => r(false), 6000))]);
  check('render callback fence', result === true);
};
const state = async (pg = page) => pg.evaluate(() => {
  const stop = document.querySelector('[data-testid="stop-button"]')?.getBoundingClientRect();
  return {
    route: location.hash,
    heading: document.querySelector('.workspace-heading h1')?.textContent,
    services: [...document.querySelectorAll('.service-nav > a[href^="#/s/"]')].map(a => a.getAttribute('href').slice(4)),
    current: [...document.querySelectorAll('a[aria-current="page"]')].map(a => a.getAttribute('href')),
    channels: [...document.querySelectorAll('.channel-explorer [data-chan]')].map(c => c.dataset.chan),
    tabs: [...document.querySelectorAll('.dv-tab')].map(t => t.textContent.trim()),
    groups: document.querySelectorAll('[data-testid="layout"] .dv-groupview').length,
    picker: !!document.querySelector('select[aria-label="레이아웃"]'),
    relations: [...document.querySelectorAll('.service-relations a')].map(a=>a.getAttribute('href')),
    navGroups: [...document.querySelectorAll('.service-nav-label')].map(n=>n.textContent),
    text: document.querySelector('.workbench-main')?.textContent ?? '',
    empty: !!document.querySelector('[data-testid="dock-empty"]'),
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
const domains = ['uart', 'can', 'eth', 'uds', 'psu'];
const aliases = {uart:['mcu','aux'], can:['pt'], eth:['mirror'], uds:['gw'], psu:['rail']};
const titles = {uart:'UART', can:'CAN', eth:'이더넷', uds:'UDS', psu:'PSU'};
const serviceDomains = d => d === 'uds' ? ['uds','can','eth'] : [d];
const serviceChannels = d => serviceDomains(d).flatMap(domain => aliases[domain].map(a => `${domain}/${a}`));
function expectService(d, s) {
  const want = serviceChannels(d);
  check(`${d} scoped explorer`, SAME(s.channels, want), s.channels);
  check(`${d} exact view count`, s.tabs.length === want.length, s.tabs);
  check(`${d} no foreign view`, s.tabs.every(t => serviceDomains(d).some(domain => t.includes(titles[domain]))), s.tabs);
  check(`${d} active aliases`, want.every(key => s.tabs.some(t => t.includes(` · ${key.split('/')[1]}`))), s.tabs);
  check(`${d} single readable group`, s.groups === 1, s.groups);
  check(`${d} no integration layout picker`, !s.picker);
  check(`${d} shared STOP visible`, s.stop);
  check(`${d} no page overflow`, !s.overflow);
}
await run('services', async () => {
  await viewport(1440,900); await login('/s/uart');
  let s = await wait(s => s.route === '#/s/uart' && s.empty);
  check('empty service URL retained', s.route === '#/s/uart', s.route);
  check('no inactive service navigation', s.services.length === 0, s.services);
  check('empty UART scoped', s.channels.length === 0 && s.empty);
  await capture('uart-empty');
  ok('uart mcu', await call('uart.open',{target:'sim://bootshell?split=0',alias:'mcu'}));
  ok('uart aux', await call('uart.open',{target:'sim://bootshell?split=0',alias:'aux'}));
  s = await wait(s => s.channels.length === 2 && s.tabs.length === 2);
  check('UART-only navigation', SAME(s.services,['uart']),s.services);expectService('uart',s);
  await capture('uart-service');
  ok('can pt',await call('can.open',{channel:'pt'}));
  ok('eth mirror',await call('eth.capture.start',{iface:'sim0',ch:'mirror',backend:'sim'}));
  ok('uds gw',await call('uds.connect',{name:'gw',sim:'bench'}));
  ok('psu rail',await call('psu.connect',{ch:'rail'}));
  s=await wait(s=>s.services.length===5);
  check('five active services available',SAME(s.services,domains),s.services);
  expectService('uart',s);check('unrelated events keep UART route',s.route==='#/s/uart');
  for(const d of domains){
    await page.click(`.service-nav > a[href="#/s/${d}"]`,{label:`${d} 서비스 열기`});
    s=await wait(s=>s.route===`#/s/${d}`&&s.tabs.length===serviceChannels(d).length&&SAME(s.channels,serviceChannels(d)));
    step(`${d} service state`,s);expectService(d,s);
    check(`${d} menu retains shared live state`,SAME(s.services,domains),s.services);
    if(d==='can'||d==='eth')check(`${d} links to common diagnostics`,s.relations.includes('#/s/uds'),s.relations);
    if(d==='uds'){check('UDS diagnostic workspace combines actual CAN and Ethernet observers',SAME(s.channels,['uds/gw','can/pt','eth/mirror']),s.channels);check('diagnostics classified separately',s.navGroups.includes('진단'));check('UDS links both transport observations',SAME(s.relations,['#/s/can','#/s/eth']),s.relations);}
    await capture(`${d}-all-connected`);
  }
  await page.reload();s=await wait(s=>s.route==='#/s/psu'&&s.tabs.length===1&&s.channels.includes('psu/rail'));
  expectService('psu',s);check('reload keeps connected OFF PSU visible',s.channels.includes('psu/rail'));
  await page.click('a[href="#/l/m1-default"]',{label:'통합 작업 공간 열기'});
  const all=domains.flatMap(d=>aliases[d].map(a=>`${d}/${a}`));
  s=await wait(s=>s.route==='#/l/m1-default'&&s.tabs.length===all.length);
  check('integration all real channels',SAME(s.channels,all),s.channels);
  check('integration six views',s.tabs.length===6,s.tabs);
  check('integration layout tools available',s.picker);
  await capture('integration');
  await go('#/s/uart');await wait(s=>s.tabs.length===2);
  ok('close mcu',await call('uart.close',{channel:'mcu'}));ok('close aux',await call('uart.close',{channel:'aux'}));
  s=await wait(s=>!s.services.includes('uart')&&s.channels.length===0&&s.empty);
  check('closed service leaves primary navigation',SAME(s.services,['can','eth','uds','psu']),s.services);
  check('closed UART stays honest empty service',s.route==='#/s/uart'&&s.channels.length===0&&s.tabs.length===0);
  await capture('uart-closed');
  ok('reopen mcu',await call('uart.open',{target:'sim://bootshell?split=0',alias:'mcu'}));
  s=await wait(s=>s.channels.includes('uart/mcu')&&s.tabs.length===1);check('reopen existing route restores only UART',SAME(s.channels,['uart/mcu'])&&s.tabs.length===1);
  const late=await task.newPage();
  try {
    await late.goto((await ctl('/login',{route:'/s/can'})).url);
    await late.waitForSelector('[data-testid="loom-shell"]',{state:'visible',timeout:10000});
    const ls=await wait(s=>s.route==='#/s/can'&&s.channels.includes('can/pt')&&s.tabs.length===1,late);
    check('late join domain scope',SAME(ls.channels,['can/pt'])&&ls.tabs.length===1,ls);
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
