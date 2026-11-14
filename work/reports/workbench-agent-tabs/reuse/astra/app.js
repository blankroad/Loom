'use strict';
const $ = s => document.querySelector(s);
const services = {
  uart: {name:'UART',path:'../../../uart-workbench/opus/index.html',selector:'#modUart',controls:['#railSessions','#railMacros','#tbSplitRight','#dlgRec'],health:'원본 터미널 표시 · 외부 구독 없음'},
  uds: {name:'UDS',path:'../../../uds-read-did/app/index.html',selector:'#modUds',controls:['#selEcu','#segRoute','#rawInput','#txBody','#hex','#toolTabs'],health:'이벤트 구독 준비'},
  eth: {name:'Ethernet',path:'../../../ethernet-workbench/opus/index.html',selector:'#modEth',controls:['#chbar','#dfInput','#tree','#hex','#tbSocket'],health:'원본 캡처 표시 · 외부 구독 없음'}
};
let serviceMode=null;
let maximized=null, observing=false, seen=0, evicted=0, received=null, mode='stack';
function logEvent(ev){
  received=Date.now(); seen++;
  $('#events .empty')?.remove();
  const row=document.createElement('div'); row.className='event';
  const time=document.createElement('time'); time.textContent=new Date(received).toLocaleTimeString('ko-KR',{hour12:false});
  const action=document.createElement('button'); action.textContent=ev.txn ? `UDS #${ev.txn.no ?? ev.txn.id}` : `UDS ${ev.type}`;
  action.onclick=()=>{const cb=$('[data-service="uds"]');cb.checked=true;maximized='uds';layout();if(ev.txn)services.uds.frame.contentWindow.LoomWorkbench.selectTxn(ev.txn.id);};
  const pre=document.createElement('pre'); pre.textContent=JSON.stringify(ev,null,2);
  row.append(time,action,pre); $('#events').append(row);
  if($('#events').children.length>250){$('#events').firstElementChild.remove();evicted++;}
  if($('#follow').checked)$('#events').scrollTop=$('#events').scrollHeight;
  if(observing && ev.type===$('#waitFor').value){observing=false;$('#taskState').textContent=`조건 충족 · ${time.textContent} · 로그에서 원본 근거 열기`;$('#step2').classList.remove('active');$('#step3').classList.add('active');$('#cancel').disabled=true;}
  health();
}
function health(){
  const ctx=services.uds.frame?.contentWindow?.LoomWorkbench?.context?.();
  $('#agentTarget').textContent=ctx ? `${ctx.ecu.toUpperCase()} · ${ctx.transport} · ${ctx.link}` : 'UDS 대상 준비 중';
  if(ctx)services.uds.panel.querySelector('header small').textContent=`${ctx.ecu.toUpperCase()} · ${ctx.transport} · SIM`;
  $('#agentStep').textContent=$('#taskState').textContent;
  $('#agentEvidence').textContent=received ? `SIM 이벤트 ${seen} · ${new Date(received).toLocaleTimeString('ko-KR')}` : '근거 대기 · 자동 실행 없음';
  $('#health').replaceChildren();
  for(const [id,s] of Object.entries(services)){
    const item=document.createElement('span');
    item.textContent=`${s.name} · ${s.health}`;
    if(id==='uds')item.textContent+=` | 마지막 이벤트 ${received?new Date(received).toLocaleTimeString('ko-KR'):'—'} | 경과 ${received?Math.floor((Date.now()-received)/1000)+'s':'—'} | 전송 lag/drop 미계측 | 로그 제외 ${evicted} | 오류 ${s.error||'없음'}`;
    item.title=id==='uds'?`수신 ${seen}건. 경과는 마지막 SIM 이벤트 이후 시간이며 장치 지연이 아닙니다.`:'상태·RX/TX·drop은 원본 패널에서 확인. 외부 수집 상태로 추정하지 않습니다.';
    $('#health').append(item);
  }
}
function layout(){
  $('#workspace').className=mode;
  for(const [id,s] of Object.entries(services)){
    s.panel.hidden=serviceMode ? serviceMode!==id : !$(`[data-service="${id}"]`).checked || !!(maximized&&maximized!==id);
    s.panel.classList.toggle('max',maximized===id || serviceMode===id);
    s.frame.contentDocument?.documentElement.toggleAttribute('data-astra-core',!serviceMode);
    s.panel.querySelector('[data-jump]').textContent=serviceMode===id?'관찰 패널로 돌아가기':'서비스 열기';
    s.panel.querySelector('[data-max]').textContent=maximized===id?'복원':'최대화';
  }
  $('#stack').setAttribute('aria-pressed',mode==='stack');$('#columns').setAttribute('aria-pressed',mode==='columns');
}
for(const [id,s] of Object.entries(services)){
  const panel=document.createElement('section');panel.className='module';panel.dataset.module=id;
  panel.innerHTML=`<header><b>${s.name}</b><small>원본 renderer · SIM</small><span class="grow"></span><button data-jump>서비스 열기</button><button data-max>최대화</button><button data-hide aria-label="${s.name} 숨기기">숨기기</button></header><div class="viewport"></div><div class="handle" role="separator" tabindex="0" aria-label="${s.name} 패널 크기" aria-orientation="horizontal"></div>`;
  const frame=document.createElement('iframe');frame.title=`${s.name} 원본 서비스`;s.frame=frame;s.panel=panel;s.loads=0;
  frame.addEventListener('load',()=>{
    s.loads++;
    try{
      const doc=frame.contentDocument;
      if(!doc.querySelector(s.selector))throw new Error('원본 모듈 없음 / HTTP 경로 확인');
      const adapter=doc.createElement('link');adapter.rel='stylesheet';adapter.href=new URL('core-panels.css',location.href).href;doc.head.append(adapter);
      doc.documentElement.toggleAttribute('data-astra-core',!serviceMode);
      // Source documents and their renderer instances stay mounted, including in full-service mode.
      s.missing=s.controls.filter(c=>!doc.querySelector(c));
      if(s.missing.length)throw new Error('원본 control 누락: '+s.missing.join(','));
      if(id==='uds'){
        const host=frame.contentWindow.LoomWorkbench;if(!host)throw new Error('UDS host 없음');
        s.unsubscribe?.();s.unsubscribe=host.subscribe(logEvent);s.health='SIM 이벤트 구독 중';
      }
      s.ready=true;$('#step1').textContent=Object.values(services).every(x=>x.ready)?'원본 준비 완료':'원본 준비';
    }catch(e){s.health='로드 오류';s.error=e.message;}
    health();
  });
  frame.src=s.path;panel.querySelector('.viewport').append(frame);$('#workspace').append(panel);
  if(id==='eth'){const pause=document.createElement('button');pause.textContent='표시 일시정지';pause.onclick=()=>{const original=frame.contentDocument.querySelector('#tbPause');original?.click();pause.textContent=frame.contentDocument.querySelector('#tbPauseLabel')?.textContent||'표시 일시정지';};panel.querySelector('header').insertBefore(pause,panel.querySelector('[data-jump]'));}
  panel.querySelector('[data-jump]').onclick=()=>{serviceMode=serviceMode===id?null:id;layout();};
  panel.querySelector('[data-max]').onclick=()=>{maximized=maximized===id?null:id;layout();};
  panel.querySelector('[data-hide]').onclick=()=>{$(`[data-service="${id}"]`).checked=false;if(maximized===id)maximized=null;serviceMode=null;layout();};
  const handle=panel.querySelector('.handle');
  function resize(delta){
    const panels=Object.values(services).map(x=>x.panel).filter(x=>!x.hidden);
    const other=panels[panels.indexOf(panel)+1];if(!other)return;
    const axis=mode==='stack'?'height':'width';
    // Opus w26 QA fix: read every size before writing any flex value. Writing the first panel's
    // flex before measuring the others re-laid them out and collapsed a non-adjacent panel (~9 px).
    const sizes=panels.map(el=>el.getBoundingClientRect()[axis]);
    const i=panels.indexOf(panel),a=sizes[i],b=sizes[i+1];
    const d=Math.max(Math.min(0,100-a),Math.min(Math.max(0,b-100),delta));
    sizes[i]=a+d;sizes[i+1]=b-d;
    panels.forEach((el,k)=>{el.style.flex=`${sizes[k]} 1 0px`;});
  }
  handle.onkeydown=e=>{if(['ArrowUp','ArrowDown','ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();resize(['ArrowUp','ArrowLeft'].includes(e.key)?-40:40);}};
  handle.onpointerdown=e=>{let pos=mode==='stack'?e.clientY:e.clientX;handle.setPointerCapture(e.pointerId);document.body.classList.add('dragging');handle.onpointermove=ev=>{let next=mode==='stack'?ev.clientY:ev.clientX;resize(next-pos);pos=next;};};
  const end=()=>{handle.onpointermove=null;document.body.classList.remove('dragging');};handle.onpointerup=end;handle.onpointercancel=end;handle.onlostpointercapture=end;
}
for(const b of document.querySelectorAll('[data-view]'))b.onclick=()=>{
  const task=b.dataset.view==='task';$('#tasks').hidden=!task;$('#journal').hidden=!task;
  for(const tab of document.querySelectorAll('[data-view]'))tab.setAttribute('aria-pressed',tab===b);
  // No frame move, replace, navigation, or src assignment here.
};
for(const cb of document.querySelectorAll('[data-service]'))cb.onchange=()=>{maximized=null;serviceMode=null;layout();};
for(const name of ['stack','columns'])$('#'+name).onclick=()=>{mode=name;for(const s of Object.values(services)){s.panel.style.flex='';s.panel.style.height='';s.panel.querySelector('.handle').setAttribute('aria-orientation',name==='stack'?'horizontal':'vertical');}layout();};
$('#restore').onclick=()=>{maximized=null;serviceMode=null;for(const s of Object.values(services)){s.panel.style.flex='';s.panel.style.height='';}layout();};
$('#observe').onclick=()=>{observing=true;$('#taskState').textContent='대기 중 · '+$('#waitFor').selectedOptions[0].textContent;$('#step2').classList.add('active');$('#step3').classList.remove('active');$('#cancel').disabled=false;};
$('#cancel').onclick=()=>{observing=false;$('#taskState').textContent='관측 작업 취소 · 원본 세션 유지';$('#step2').classList.remove('active');$('#cancel').disabled=true;};
$('#waitFor').onchange=()=>{if(observing)$('#taskState').textContent='대기 중 · '+$('#waitFor').selectedOptions[0].textContent;};
$('#clear').onclick=()=>{$('#events').replaceChildren();evicted=0;health();};
window.astraReuse={inspect:()=>Object.fromEntries(Object.entries(services).map(([k,s])=>[k,{loads:s.loads,ready:!!s.ready,missing:s.missing,error:s.error,document:s.frame.contentDocument,url:s.frame.src}]))};
layout();health();setInterval(health,1000);
