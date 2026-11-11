import http from 'node:http';
import fs from 'node:fs/promises';
import { WebSocketServer } from 'ws';
import { encode } from '@msgpack/msgpack';
const now=()=>performance.timeOrigin+performance.now();
export async function startServer() {
  const server=http.createServer(async(req,res)=>{
    if(req.url==='/clock'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({now:now()}));return;}
    const path=req.url?.split('?')[0];
    const file=path==='/client.js'?'dist/client.js':path==='/xterm.css'?'dist/xterm.css':'index.html';
    res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html');
    res.setHeader('Cross-Origin-Opener-Policy','same-origin');
    res.setHeader('Cross-Origin-Embedder-Policy','require-corp');
    try{res.end(await fs.readFile(new URL(file,import.meta.url)));}catch{res.statusCode=404;res.end();}
  });
  const wss=new WebSocketServer({server,maxPayload:65536,perMessageDeflate:false});
  wss.on('connection',(ws,req)=>{
    const q=new URL(req.url,'http://localhost').searchParams;
    const format=q.get('format')||'json',rate=Number(q.get('rate')||10000);
    let seq=0,generated=0,bytes=0,maxMessage=0,dropped=0;
    const begin=now();
    const timer=setInterval(()=>{
      if(ws.readyState!==1)return;
      let due=Math.floor((now()-begin)*rate/1000)-generated;
      // Bound catch-up work; count the omitted source generation slots explicitly.
      if(due>rate){dropped+=due-rate;generated+=due-rate;due=rate;}
      while(due>0){
        const count=Math.min(due,200),stamp=now();
        if(ws.bufferedAmount>2*1024*1024){seq+=count;generated+=count;dropped+=count;due-=count;continue;}
        let data;
        if(format==='column'){
          const b=new ArrayBuffer(16+count*24),v=new DataView(b);
          v.setUint32(0,0x4c554931,true);v.setUint32(4,count,true);v.setFloat64(8,stamp,true);
          const base=seq;
          for(let i=0;i<count;i++){
            const n=base+i+1;
            v.setUint32(16+i*4,n,true);
            v.setUint32(16+count*4+i*4,n%2048,true);
            v.setFloat64(16+count*8+i*8,stamp,true);
            v.setUint32(16+count*16+i*4,n^0x12345678,true);
            v.setUint32(16+count*20+i*4,n^0x76543210,true);
          }
          seq+=count;data=Buffer.from(b);
        }else{
          const rows=Array.from({length:count},()=>{const n=++seq;return [n,n%2048,stamp,n^0x12345678,n^0x76543210];});
          const msg={version:1,rows};
          data=format==='msgpack'?encode(msg):JSON.stringify(msg);
        }
        const length=typeof data==='string'?Buffer.byteLength(data):data.byteLength;
        if(length>65536)throw Error('batch exceeds 64 KiB');
        ws.send(data);bytes+=length;maxMessage=Math.max(maxMessage,length);generated+=count;due-=count;
      }
    },20);
    ws.on('message',message=>{if(String(message)==='stats')ws.send(JSON.stringify({stats:{seq,generated,bytes,maxMessage,dropped,elapsed_ms:now()-begin}}));});
    ws.on('close',()=>clearInterval(timer));
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {url:`http://127.0.0.1:${server.address().port}`,close:async()=>{for(const ws of wss.clients)ws.terminate();await new Promise(r=>wss.close(r));await new Promise(r=>server.close(r));}};
}
