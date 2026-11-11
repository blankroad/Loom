import fs from 'node:fs/promises';
import os from 'node:os';
import { chromium } from 'playwright';
import { startServer } from './server.mjs';
const seconds=Number(process.env.BENCH_SECONDS||60),soak=Number(process.env.SOAK_SECONDS||0);
const server=await startServer();
const browser=await chromium.launch({headless:true,channel:'chromium',args:['--enable-precise-memory-info','--disable-background-timer-throttling','--disable-renderer-backgrounding']});
const out={started_at:new Date().toISOString(),environment:{os:os.platform(),release:os.release(),arch:os.arch(),cpus:os.cpus().map(c=>c.model),ram_bytes:os.totalmem(),node:process.version,browser:browser.version(),viewport:{width:1280,height:900}},method:{seconds,soak,rate:10000,capacity:2000000,compression:false,renderProxy:'next requestAnimationFrame after draw; not compositor presentation',latencySampling:'one descriptor per received WS batch, bounded 100k rolling samples; not all frame presentation'},runs:[]};
await fs.mkdir('results',{recursive:true});
const save=()=>fs.writeFile('results/benchmark.json',JSON.stringify(out,null,2));
try{
  for(const [format,renderer,fixed] of [['json','dom',false],['json','canvas',false],['column','dom',false],['column','canvas',false],['msgpack','canvas',false],['column','canvas',true]]){
    const page=await browser.newPage({viewport:out.environment.viewport});
    page.on('pageerror',error=>console.error('PAGE ERROR',error));
    await page.goto(`${server.url}/?format=${format}&renderer=${renderer}&fixed=${+fixed}`);await page.evaluate(()=>window.ready);
    await page.waitForTimeout(3000);
    if(!(await page.evaluate(()=>window.metrics().draws)))throw Error('No animation frames: invalid rendering benchmark');
    await page.evaluate(()=>window.resetMetrics());
    for(let s=0;s<seconds;s+=10){await page.waitForTimeout(Math.min(10,seconds-s)*1000);await page.evaluate(()=>window.collect());}
    await page.waitForTimeout(30);const metrics=await page.evaluate(()=>window.metrics());
    const latest=await page.evaluate(()=>window.inspectLatest());
    if(latest.id!==latest.seq%2048||latest.lo!==((latest.seq^0x12345678)>>>0)||latest.hi!==((latest.seq^0x76543210)>>>0))throw Error('wire/ring payload mismatch');
    if(metrics.gaps||metrics.measurementDropped)throw Error('unexpected data/measurement loss');
    out.runs.push(metrics);await save();
    console.log(JSON.stringify({format,renderer,fixed,received:metrics.received,latency:metrics.generated_to_next_raf_ms,decode:metrics.decode_ms,draw:metrics.draw_ms}));
    if(format==='column'&&renderer==='canvas'&&!fixed)await page.screenshot({path:'results/canvas.png'});
    await page.close();
  }
  const page=await browser.newPage({viewport:out.environment.viewport});await page.goto(`${server.url}/?format=column&renderer=canvas`);await page.evaluate(()=>window.ready);
  out.terminal=[];for(const useWebgl of [false,true])out.terminal.push(await page.evaluate(v=>window.terminalBench(v),useWebgl));await save();console.log(JSON.stringify({terminal:out.terminal}));
  // Viewer credit exhaustion is injected explicitly; no claim about journal integrity.
  await page.evaluate(()=>window.setDisplayPause(true));await page.waitForTimeout(500);await page.evaluate(()=>window.setDisplayPause(false));await page.waitForTimeout(100);
  out.gapTest=await page.evaluate(()=>({metrics:window.metrics(),badge:document.querySelector('#status').textContent}));
  if(!out.gapTest.metrics.gaps||!out.gapTest.badge.includes('LIVE-DEGRADED')||out.gapTest.metrics.gapRanges.reduce((n,g)=>n+g.last_seq-g.first_seq+1,0)!==out.gapTest.metrics.gaps)throw Error('gap honesty failed');
  await page.locator('#stop').click();if(!(await page.locator('#stopstate').textContent()).includes('acknowledged'))throw Error('STOP UI click failed');
  out.stopTest={clicked:true,scope:'UI event dispatch only; no daemon or device'};await save();
  if(soak){
    await page.close();const p=await browser.newPage({viewport:out.environment.viewport});await p.goto(`${server.url}/?format=column&renderer=canvas`);await p.evaluate(()=>window.ready);
    out.soak={seconds,samples:[]};const pidInfo=await browser.newBrowserCDPSession();
    for(let s=0;s<soak;s+=30){
      await p.waitForTimeout(Math.min(30,soak-s)*1000);
      const client=await p.context().newCDPSession(p);await client.send('HeapProfiler.collectGarbage');const heap=await client.send('Runtime.getHeapUsage');await client.detach();
      const m=await p.evaluate(()=>window.collect());out.soak.samples.push({seconds:s+Math.min(30,soak-s),heap,metrics:m});await save();
      console.log(JSON.stringify({soak_seconds:s+30,received:m.received,usedHeap:heap.usedSize,arrayBuffers:m.arrayBufferBytes,gaps:m.gaps}));
    }
    out.soak.final=await p.evaluate(()=>window.metrics());out.soak.seconds=soak;await pidInfo.detach();
  }
}finally{out.finished_at=new Date().toISOString();await save();await browser.close();await server.close();}
