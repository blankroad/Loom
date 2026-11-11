import { decode } from '@msgpack/msgpack';
import { Terminal } from '@xterm/xterm';
import { WebglAddon } from '@xterm/addon-webgl';
const q=new URLSearchParams(location.search),format=q.get('format')||'json',renderer=q.get('renderer')||'dom';
const fixed=q.get('fixed')==='1',capacity=fixed?2048:Number(q.get('capacity')||2000000);
const seqs=new Uint32Array(capacity),ids=new Uint32Array(capacity),times=new Float64Array(capacity),lo=new Uint32Array(capacity),hi=new Uint32Array(capacity);
// Full ring from the first sample: history allocation cannot hide behind a short run.
let written=capacity,received=0,lastSeq=0,gaps=0,maxPending=0,pending=[],stopRequested=0,stopLatency=null,serverStats=null,measurementDropped=0,displayPaused=false,gapRanges=[];
for(let i=0;i<capacity;i++){seqs[i]=i+1;ids[i]=i%2048;lo[i]=(i+1)^0x12345678;hi[i]=(i+1)^0x76543210;}
const fixedSlots=new Int32Array(2048).fill(-1);
let decodeTimes=[],ingestTimes=[],drawTimes=[],frameIntervals=[],latencies=[],lastRaf=0,offset=0,samples=[],draws=0,socket;
const maxSamples=100000;let sampledFrames=0;
function sample(a,value){sampledFrames++;if(a.length<maxSamples)a.push(value);else a[sampledFrames%maxSamples]=value;}
function stats(a){if(!a.length)return {n:0};const b=a.slice().sort((a,b)=>a-b);const p=x=>b[Math.min(b.length-1,Math.floor((b.length-1)*x))];return {n:a.length,p50:p(.5),p95:p(.95),p99:p(.99),max:b.at(-1)};}
function put(n,id,t,l,h){
  if(lastSeq&&n!==lastSeq+1)gaps+=Math.max(0,n-lastSeq-1);lastSeq=n;
  const p=fixed?id:(written%capacity);seqs[p]=n;ids[p]=id;times[p]=t;lo[p]=l;hi[p]=h;written++;received++;
  if(fixed)fixedSlots[id]=p;
}
const view=document.querySelector('#view'),status=document.querySelector('#status');
let canvas,ctx,rows=[];
if(renderer==='canvas'){canvas=document.createElement('canvas');canvas.width=1050;canvas.height=616;view.append(canvas);ctx=canvas.getContext('2d');ctx.font='14px monospace';}
else{for(let i=0;i<28;i++){const d=document.createElement('div');view.append(d);rows.push(d);}}
function text(p){return `${String(seqs[p]).padStart(10)} | ${ids[p].toString(16).padStart(3,'0')} | device RX | ${lo[p].toString(16).padStart(8,'0')} ${hi[p].toString(16).padStart(8,'0')}`;}
function draw(){
  const start=performance.now();
  if(ctx){ctx.fillStyle='#131a23';ctx.fillRect(0,0,1050,616);ctx.fillStyle='#d8e4ee';}
  for(let i=0;i<28;i++){
    const p=fixed?i:((written-28+i)%capacity+capacity)%capacity;
    const s=text(p);if(ctx)ctx.fillText(s,8,i*22+17);else rows[i].textContent=s;
  }
  // Force style/layout completion in DOM; this is still not a compositor presentation ACK.
  if(!ctx)void view.offsetHeight;
  sample(drawTimes,performance.now()-start);draws++;
  const batches=pending;pending=[];
  requestAnimationFrame(()=>{const done=performance.timeOrigin+performance.now();for(const b of batches)sample(latencies,done-b.stamp-offset);});
}
function raf(t){if(lastRaf)sample(frameIntervals,t-lastRaf);lastRaf=t;if(pending.length)draw();requestAnimationFrame(raf);}
document.querySelector('#stop').onclick=()=>{stopLatency=performance.now()-stopRequested;document.querySelector('#stopstate').textContent='STOP acknowledged (UI simulation only)';};
window.injectStall=ms=>{const end=performance.now()+ms;while(performance.now()<end){};};
window.testStop=()=>{stopRequested=performance.now();document.querySelector('#stop').click();return stopLatency;};
window.setDisplayPause=value=>displayPaused=value;
window.inspectLatest=()=>{const p=fixed?lastSeq%2048:(written-1)%capacity;return {seq:seqs[p],id:ids[p],lo:lo[p],hi:hi[p]};};
window.metrics=()=>({format,renderer,fixed,capacity,received,lastSeq,gaps,gapRanges,maxPending,measurementDropped,draws,decode_ms:stats(decodeTimes),ingest_ms:stats(ingestTimes),draw_ms:stats(drawTimes),raf_interval_ms:stats(frameIntervals),generated_to_next_raf_ms:stats(latencies),heap:performance.memory?{used:performance.memory.usedJSHeapSize,total:performance.memory.totalJSHeapSize}:null,arrayBufferBytes:seqs.byteLength+ids.byteLength+times.byteLength+lo.byteLength+hi.byteLength,domNodes:document.querySelectorAll('*').length,samples,serverStats,crossOriginIsolated,userAgent:navigator.userAgent,clockOffsetMs:offset,stopLatency});
window.collect=()=>{socket?.send('stats');samples.push({elapsed_ms:performance.now(),received,heap:performance.memory?.usedJSHeapSize,nodes:document.querySelectorAll('*').length});return window.metrics();};
window.resetMetrics=()=>{decodeTimes=[];ingestTimes=[];drawTimes=[];latencies=[];frameIntervals=[];};
// Independent S2: write completion measures parser throughput, separately from render.
window.terminalBench=async(useWebgl=false)=>{
  const term=new Terminal({cols:120,rows:30,scrollback:1000});term.open(document.querySelector('#terminal'));
  let webgl=false,error=null;
  if(useWebgl){try{term.loadAddon(new WebglAddon());webgl=true;}catch(e){error=String(e);}}
  const chunk=('seq=0123456789 temperature=42.1 rpm=1200\r\n').repeat(1000),bytes=chunk.length*100;
  const start=performance.now();
  for(let i=0;i<100;i++)await new Promise(resolve=>term.write(chunk,resolve));
  const parse_ms=performance.now()-start;
  await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
  const result={requestedWebgl:useWebgl,webgl,error,bytes,parse_ms,throughput_MiB_s:bytes/1048576/(parse_ms/1000),completion_to_2raf_ms:performance.now()-start-parse_ms};term.dispose();return result;
};
window.ready=(async()=>{
  const clocks=[];
  for(let i=0;i<8;i++){const a=performance.timeOrigin+performance.now();const c=await(await fetch('/clock')).json();const b=performance.timeOrigin+performance.now();clocks.push({rtt:b-a,offset:(a+b)/2-c.now});}
  clocks.sort((a,b)=>a.rtt-b.rtt);offset=clocks[0].offset;
  socket=new WebSocket(`${location.origin.replace('http:','ws:')}/trace?format=${format}&rate=${q.get('rate')||10000}`);socket.binaryType='arraybuffer';
  socket.onmessage=event=>{
    let msg,start=performance.now(),v,count;
    if(typeof event.data==='string'){msg=JSON.parse(event.data);if(msg.stats){serverStats=msg.stats;return;}}
    else if(format==='msgpack')msg=decode(new Uint8Array(event.data));
    else{v=new DataView(event.data);if(v.getUint32(0,true)!==0x4c554931)throw Error('magic');count=v.getUint32(4,true);if(event.data.byteLength!==16+count*24)throw Error('length');}
    sample(decodeTimes,performance.now()-start);start=performance.now();
    if(displayPaused){
      const first=v?v.getUint32(16,true):msg.rows[0][0],last=v?v.getUint32(16+(count-1)*4,true):msg.rows.at(-1)[0],n=v?count:msg.rows.length;
      gaps+=n;lastSeq=last;
      if(gapRanges.length&&gapRanges.at(-1).last_seq+1===first){gapRanges.at(-1).last_seq=last;gapRanges.at(-1).count+=n;}
      else if(gapRanges.length<32)gapRanges.push({first_seq:first,last_seq:last,count:n,cause:'injected_viewer_credit_exhaustion'});
      status.textContent=`LIVE-DEGRADED display.gap=${gaps} (viewer-only injection; recording untested)`;return;
    }
    let stamp;
    if(v){for(let i=0;i<count;i++)put(v.getUint32(16+i*4,true),v.getUint32(16+count*4+i*4,true),v.getFloat64(16+count*8+i*8,true),v.getUint32(16+count*16+i*4,true),v.getUint32(16+count*20+i*4,true));stamp=v.getFloat64(8,true);}
    else{for(const r of msg.rows)put(...r);stamp=msg.rows[0][2];}
    sample(ingestTimes,performance.now()-start);
    // Bound pending measurement descriptors; losing these is visible in the diagnostic.
    if(pending.length>=512){measurementDropped++;pending.shift();}pending.push({stamp});maxPending=Math.max(maxPending,pending.length);
    status.textContent=`${gaps?'LIVE-DEGRADED display.gap='+gaps:'LIVE'} | ${format}/${renderer}${fixed?' Fixed':''} | RX ${received.toLocaleString()} | ring ${capacity.toLocaleString()} | seq ${lastSeq}`;
  };
  requestAnimationFrame(raf);
  await new Promise(r=>socket.addEventListener('open',r,{once:true}));return true;
})();
