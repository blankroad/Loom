/* eslint-disable */
// Fidelity acceptance on the native simulator; see per-domain selectors below.
const tick=()=>page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
const route=async domain=>{
  await page.evaluate(d=>{location.hash=`#/s/${d}`},domain);
  await page.waitForFunction(d=>location.hash===`#/s/${d}`&&document.querySelector(`[data-panel^="${d}."]`),domain,{timeout:10000});
  await tick();
};
const layout=async()=>page.evaluate(()=>({overflow:document.documentElement.scrollWidth>innerWidth,stop:(()=>{const r=document.querySelector('[data-testid="stop-button"]')?.getBoundingClientRect();return !!r&&r.left>=0&&r.right<=innerWidth&&r.bottom<=innerHeight})(),panels:[...document.querySelectorAll('[data-panel]')].map(n=>n.dataset.panel)}));
let fidelityDone=false;
await run('fidelity',async()=>{
  await viewport(1440,1000);await login('/s/uart');
  ok('open uart',await call('uart.open',{target:'sim://bootshell?split=0',alias:'mcu'}));
  ok('open can',await call('can.open',{channel:'pt'}));
  ok('open eth',await call('eth.capture.start',{iface:'sim0',ch:'mirror',backend:'sim'}));
  ok('open uds',await call('uds.connect',{name:'gw',sim:'bench'}));
  ok('open psu',await call('psu.connect',{ch:'rail'}));
  ok('agent CAN sample',await call('can.send',{channel:'pt',frame:'300#11223344'}));
  ok('agent UDS sample',await call('uds.request',{target:'gw',bytes_hex:'22F190'}));
  ok('PSU measured sample',await call('psu.meas',{ch:'rail'}));
  for(const domain of ['uart','can','eth','uds','psu']){
    await route(domain);
    const s=await layout();check(`${domain} desktop no page overflow`,!s.overflow,s);
    check(`${domain} STOP reachable`,s.stop);
    await shot(`${domain}-fidelity`);
  }
  await page.waitForFunction(()=>!document.querySelector('.toast'),undefined,{timeout:15000});
  await attempt('uart interactions',async()=>{
  await route('uart');
  await page.click('[data-testid="uart-send-string-toggle"]',{label:'UART 작성기 열기'});
  await page.fill('[data-testid="uart-ss-input"]','version');
  const take=await page.evaluate(()=>[...document.querySelectorAll('.uart-lease button')].find(n=>n.textContent==='가져오기')?.disabled===false);
  if(take)await page.click('.uart-lease button:text-is("가져오기")',{label:'UART 입력권 가져오기'});
  await page.waitForFunction(()=>!document.querySelector('[data-testid="uart-ss-send"]').disabled,undefined,{timeout:8000});
  check('UART composer enabled under current input lease',await page.evaluate(()=>!document.querySelector('[data-testid="uart-ss-send"]').disabled));
  check('UART text preview includes CR',await page.evaluate(()=>document.querySelector('[data-testid="uart-ss-preview"]').textContent.includes('0D')));
  await page.click('[data-testid="uart-ss-send"]',{label:'UART 문자열 전송'});
  await page.click('[data-testid="uart-ss-mode-hex"]',{label:'HEX 작성 모드 선택'});
  await page.fill('[data-testid="uart-ss-input"]','GZ');
  check('UART invalid HEX blocked',await page.evaluate(()=>document.querySelector('[data-testid="uart-ss-send"]').disabled&&document.querySelector('[data-testid="uart-ss-input"]').getAttribute('aria-invalid')==='true'));
  await page.fill('[data-testid="uart-ss-input"]','76 65 72 73 69 6f 6e');
  await page.selectOption('[data-testid="uart-ss-ending"]','cr');
  await page.click('[data-testid="uart-ss-send"]',{label:'UART HEX 원문 전송'});
  await page.click('[data-testid="uart-view-hex"]',{label:'UART 원문 보기'});
  await visible('[data-testid="uart-bytes"]');
  await waitText('[data-testid="uart-bytes"]','76 65 72 73 69 6F 6E');
  check('UART byte rows contain original version bytes',await page.evaluate(()=>document.querySelector('[data-testid="uart-bytes"]').textContent.includes('76 65 72 73 69 6F 6E')));
  await shot('uart-fidelity-bytes');
  await page.click('[data-testid="uart-view-terminal"]',{label:'터미널로 돌아가기'});
  await page.click('[data-testid="uart-send-string-toggle"]',{label:'UART 작성기 접기'});
  await shot('uart-fidelity');

  });
  await attempt('can interactions',async()=>{
  await route('can');
  check('CAN service defaults Receive Transmit',await page.evaluate(()=>document.querySelector('[data-testid="can-view-rxtx"]').getAttribute('aria-selected')==='true'));
  await visible('[data-testid="can-rx-row"]');
  check('CAN summary has observation window',await page.evaluate(()=>document.querySelector('[data-testid="can-rx-window"]').textContent.length>0));
  await page.click('[data-testid="can-tx-new"]',{label:'CAN 메시지 작성'});
  await page.fill('[data-testid="can-tx-id"]','300');
  await page.selectOption('[data-testid="can-tx-len"]','2');
  await page.fill('[data-testid="can-tx-byte"] >> nth=0','12');
  await page.fill('[data-testid="can-tx-byte"] >> nth=1','34');
  await page.click('[data-testid="can-tx-ok"]',{label:'CAN 송신 목록 추가'});
  check('CAN draft in manual list',await page.evaluate(()=>document.querySelector('[data-testid="can-tx-row"]').textContent.includes('12 34')));
  await page.click('[data-testid="can-tx-send"]',{label:'CAN 선택 메시지 전송'});
  await page.waitForFunction(()=>document.querySelector('[data-testid="can-tx-row"]').dataset.count==='1',undefined,{timeout:8000});
  await shot('can-fidelity');
  await page.click('[data-testid="can-view-trace"]',{label:'CAN 시간순 Trace 확인'});
  await visible('[data-testid="can-send-input"]');
  check('CAN Trace retained',await page.evaluate(()=>document.querySelector('[data-testid="can-view-trace"]').getAttribute('aria-selected')==='true'));
  await shot('can-fidelity-trace');

  });
  await attempt('eth interactions',async()=>{
  await route('eth');
  ok('ETH visible capture sample',await call('eth.send',{ch:'mirror',kind:'raw',hex:'ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002'}));
  await page.waitForFunction(()=>[...document.querySelectorAll('.eth-list .vt-row[data-seq]')].some(n=>!n.hidden),undefined,{timeout:10000});
  await page.press('.eth-panel','F8');
  await visible('.th-node');
  await page.click('.th-node[data-off][data-len] >> nth=0',{label:'프로토콜 범위 선택'});
  check('ETH HEX ASCII selected ranges synchronized',await page.evaluate(()=>{const a=[...document.querySelectorAll('.th-b.hl')].map(n=>n.dataset.off),b=[...document.querySelectorAll('.th-char.hl')].map(n=>n.dataset.off);return a.length>0&&JSON.stringify(a)===JSON.stringify(b)}));
  await waitText('[data-testid="eth-sb-field"]','bytes');
  check('ETH selected field status',await page.evaluate(()=>document.querySelector('[data-testid="eth-sb-field"]').textContent.includes('bytes')));
  const before=await page.evaluate(()=>document.querySelector('[data-testid="eth-split-list"]').getAttribute('aria-valuenow'));
  await page.press('[data-testid="eth-split-list"]','ArrowDown');
  check('ETH pane resize keyboard',before!==await page.evaluate(()=>document.querySelector('[data-testid="eth-split-list"]').getAttribute('aria-valuenow')));
  await page.fill('[data-testid="eth-filter-input"]','doip.type ==');
  await page.waitForFunction(()=>document.querySelector('[data-testid="eth-filter-frame"]').dataset.validity==='err',undefined,{timeout:8000});
  check('ETH invalid filter visible',await page.evaluate(()=>document.querySelector('[data-testid="eth-filter-input"]').getAttribute('aria-invalid')==='true'));
  await page.click('[data-testid="eth-filter-clear"]',{label:'표시 필터 초기화'});
  await shot('eth-fidelity');

  });
  await attempt('uds interactions',async()=>{
  await route('uds');
  await page.click('.uds-ds-svc[data-sid="22"]',{label:'UDS 읽기 서비스 선택'});
  check('UDS picker fills raw PDU',await page.evaluate(()=>document.querySelector('input[aria-label="UDS 요청 (hex)"]').value.replaceAll(' ','').toUpperCase()==='22F190'));
  await page.click('.uds-ds-transmit',{label:'UDS 명시적 전송'});
  await page.waitForFunction(()=>document.querySelector('.uds-table').textContent.includes('ReadData'),undefined,{timeout:8000});
  check('UDS trace shows actual transaction',await page.evaluate(()=>document.querySelector('.uds-table').textContent.includes('ReadData')));
  await page.fill('.uds-ds-search','22');
  await page.waitForFunction(()=>document.querySelectorAll('.uds-ds-svc').length===1&&document.querySelector('.uds-ds-svc').dataset.sid==='22',undefined,{timeout:8000});
  check('UDS catalog search scoped',await page.evaluate(()=>[...document.querySelectorAll('.uds-ds-svc')].filter(n=>!n.hidden).every(n=>n.dataset.sid==='22')));
  await page.fill('.uds-ds-search','');
  await shot('uds-fidelity');

  });
  await attempt('psu interactions',async()=>{
  await route('psu');
  const prior=await call('psu.status',{ch:'rail'});
  step('PSU status shape',Object.keys(prior.data??prior));
  await page.fill('[data-testid="psu-set-v"]','12');
  check('PSU edits marked unapplied',await page.evaluate(()=>document.querySelector('[data-testid="psu-set-pending"]').textContent.includes('미적용')));
  const typed=await call('psu.status',{ch:'rail'});
  check('PSU typing does not change setpoint',JSON.stringify((prior.data??prior).set_v)===JSON.stringify((typed.data??typed).set_v));
  await page.click('[data-testid="psu-apply"]',{label:'PSU 목표값 적용'});
  await waitText('[data-testid="psu-setpoint-v"]','12.00');
  check('PSU applied target separate from measured',await page.evaluate(()=>document.querySelector('[data-testid="psu-setpoint-v"]').textContent.includes('12.00')&&!!document.querySelector('[data-testid="psu-meter-v"] .psu-num')));
  await page.click('[data-testid="psu-output-toggle"]',{label:'시뮬레이터 출력 켜기'});
  await waitText('[data-testid="psu-out"]','ON');
  await call('psu.meas',{ch:'rail'});
  await page.click('[data-testid="psu-lane-p"]',{label:'전력 관측 레인 표시'});
  await page.waitForFunction(()=>!document.querySelector('[data-testid="psu-lane-row-p"]').hidden,undefined,{timeout:8000});
  check('PSU third lane visible',await page.evaluate(()=>!document.querySelector('[data-testid="psu-lane-row-p"]').hidden));
  check('PSU actual canvas plots',await page.evaluate(()=>document.querySelectorAll('.psu-lane-plot canvas').length>=3&&!document.querySelector('.psu-plot-msg:not([hidden])')));
  await shot('psu-fidelity');
  await page.click('[data-testid="psu-emergency"]',{label:'PSU 채널 비상 OFF'});
  await waitText('[data-testid="psu-out"]','OFF');
  check('PSU emergency leaves channel connected',await page.evaluate(()=>document.querySelector('.channel-explorer [data-chan="psu/rail"]')!==null));

  });
  for(const domain of ['uart','can','eth','uds','psu']){
    await route(domain);
    for(const width of [768,390]){
      await viewport(width,1000);await tick();const s=await layout();
      check(`${domain} ${width} no page overflow`,!s.overflow,s);
      check(`${domain} ${width} STOP visible`,s.stop);
      await shot(`${domain}-fidelity-${width}`);
    }
    await viewport(1440,1000);
  }
  fidelityDone=true;
});
if(fidelityDone&&failures.length===0)console.log('finish',JSON.stringify(await task.finish({keep:[]})));
else console.log('TaskSpace retained for diagnosis',task.spaceId);
