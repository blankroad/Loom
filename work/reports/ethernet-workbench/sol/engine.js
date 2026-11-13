/* Pure deterministic packet/state model. No sockets, requests, or browser dependency. */
(function(root){
'use strict';
const LIMIT=240, PREFIX='loom.eth.sol.v1';
const enc=s=>Array.from(new TextEncoder().encode(s));
const hex=a=>a.map(x=>x.toString(16).padStart(2,'0')).join(' ');
const ascii=a=>a.map(x=>x>=32&&x<127?String.fromCharCode(x):'.').join('');
const ipBytes=s=>s.split('.').map(Number), validIP=s=>/^\d+\.\d+\.\d+\.\d+$/.test(s)&&ipBytes(s).every(n=>n>=0&&n<=255);
const u16=n=>[n>>>8&255,n&255], u32=n=>[n>>>24&255,n>>>16&255,n>>>8&255,n&255];
function checksum(a){let sum=0;for(let i=0;i<a.length;i+=2)sum+=(a[i]<<8)+(a[i+1]||0);while(sum>>>16)sum=(sum&65535)+(sum>>>16);return (~sum)&65535;}
const field=(name,start,length)=>({name,start,length});
function packet(ch,id,spec={}){
 const kind=spec.kind||['ARP','DoIP','SOMEIP','TCP','UDP','DoIP'][id%6];
 const reverse=spec.reverse??(Math.floor(id/6)%2===1), local=ch.ip, remote=spec.remote||local.replace(/\.\d+$/,'.20');
 let src=reverse?remote:local,dst=reverse?local:remote,sp=spec.sp||49152,dp=spec.dp||(kind==='SOMEIP'?30490:kind==='DoIP'?13400:5000);
 if(reverse)[sp,dp]=[dp,sp];
 const sm=reverse?[2,0,0,0,0,32]:[2,0,0,0,0,16],dm=reverse?[2,0,0,0,0,16]:[2,0,0,0,0,32];
 let raw=[],groups=[],payload=[],offset=0,transport=spec.transport||(kind==='TCP'?'TCP':'UDP'),info='',protocol=kind;
 if(kind==='ARP'){
  dst='Broadcast';src=local;transport='ARP';payload=[];
  const arpMAC=[2,0,0,0,0,16];
  raw=[...Array(6).fill(255),...arpMAC,8,6,0,1,8,0,6,4,0,1,...arpMAC,...ipBytes(local),...Array(6).fill(0),...ipBytes(remote)];
  groups.push({name:'Ethernet II · broadcast',start:0,length:14,fields:[field('Destination: ff:ff:ff:ff:ff:ff',0,6),field('Source: '+hex(arpMAC).replaceAll(' ',':'),6,6),field('Type: ARP (0x0806)',12,2)]});
  groups.push({name:'Address Resolution Protocol · request',start:14,length:28,fields:[field('Hardware type: Ethernet (1)',14,2),field('Protocol type: IPv4 (0x0800)',16,2),field('Address sizes: MAC 6 / IP 4',18,2),field('Opcode: request (1)',20,2),field('Sender MAC: '+hex(arpMAC).replaceAll(' ',':'),22,6),field('Sender IP: '+local,28,4),field('Target MAC: 00:00:00:00:00:00',32,6),field('Target IP: '+remote,38,4)]});
  info='Who has '+remote+'? Tell '+local;
 }else{
  if(spec.payload) {payload=[...spec.payload];protocol=transport;info=spec.info||`${spec.direction||'fixture'} ${sp} → ${dp} · ${payload.length} payload bytes`;}
  else if(kind==='DoIP'){
   const body=reverse?[...enc('LOOMSIM0000000001'),0x0e,0x80,...sm,...dm,0]:[];
   payload=[2,253,...u16(reverse?4:1),...u32(body.length),...body];info=reverse?'Vehicle identification response · LOOMSIM0000000001':'Vehicle identification request';
  }else if(kind==='SOMEIP'){
   payload=[0x12,0x34,0x80,0x01,...u32(12),0,1,...u16(id&65535),1,1,2,0,0,0,0,42];info='Service 0x1234 · event 0x8001 · notification · value 42';
  }else {payload=enc(kind==='TCP'?(reverse?'STATUS OK\r\n':'GET STATUS\r\n'):'bench-heartbeat');info=kind==='TCP'?`${sp} → ${dp} [PSH, ACK] Seq=${id*16} · ${ascii(payload).trim()}`:`${sp} → ${dp} · heartbeat`;}
  const tcp=transport==='TCP',headerLen=tcp?20:8,ipLen=20+headerLen+payload.length;
  let ip=[0x45,0,...u16(ipLen),...u16(id&65535),0x40,0,64,tcp?6:17,0,0,...ipBytes(src),...ipBytes(dst)];const ipcs=checksum(ip);ip[10]=ipcs>>>8;ip[11]=ipcs&255;
  let th=tcp?[...u16(sp),...u16(dp),...u32(id*16),...u32(id*16+1),0x50,0x18,0x20,0,0,0,0,0]:[...u16(sp),...u16(dp),...u16(headerLen+payload.length),0,0];
  const pseudo=[...ipBytes(src),...ipBytes(dst),0,tcp?6:17,...u16(th.length+payload.length),...th,...payload];const tcs=checksum(pseudo)||65535;th[tcp?16:6]=tcs>>>8;th[tcp?17:7]=tcs&255;
  raw=[...dm,...sm,8,0,...ip,...th,...payload];offset=34+headerLen;
  groups.push({name:'Ethernet II · '+hex(sm).replaceAll(' ',':'),start:0,length:14,fields:[field('Destination: '+hex(dm).replaceAll(' ',':'),0,6),field('Source: '+hex(sm).replaceAll(' ',':'),6,6),field('Type: IPv4 (0x0800)',12,2)]});
  groups.push({name:`Internet Protocol v4 · ${src} → ${dst}`,start:14,length:20,fields:[field('Version: 4 · Header length: 20 bytes',14,1),field('DSCP / ECN: 0',15,1),field('Total length: '+ipLen,16,2),field('Identification: '+(id&65535),18,2),field('Flags: Don’t fragment',20,2),field('Time to live: 64',22,1),field('Protocol: '+transport+' ('+(tcp?6:17)+')',23,1),field('Header checksum: 0x'+ipcs.toString(16).padStart(4,'0'),24,2),field('Source: '+src,26,4),field('Destination: '+dst,30,4)]});
  groups.push({name:`${transport} · ${sp} → ${dp}`,start:34,length:headerLen,fields:[field('Source port: '+sp,34,2),field('Destination port: '+dp,36,2),...(tcp?[field('Sequence number: '+id*16,38,4),field('Acknowledgment number: '+(id*16+1),42,4),field('Header length: 20 · Flags: PSH, ACK',46,2),field('Window: 8192',48,2),field('Checksum: 0x'+tcs.toString(16).padStart(4,'0'),50,2),field('Urgent pointer: 0',52,2)]:[field('Length: '+(headerLen+payload.length),38,2),field('Checksum: 0x'+tcs.toString(16).padStart(4,'0'),40,2)])]});
  if(kind==='DoIP'&&!spec.payload)groups.push({name:'DoIP · '+info,start:offset,length:payload.length,fields:[field('Protocol version: 0x02',offset,1),field('Inverse version: 0xfd',offset+1,1),field('Payload type: '+(reverse?'0x0004 (identification response)':'0x0001 (identification request)'),offset+2,2),field('Payload length: '+(payload.length-8),offset+4,4),...(reverse?[field('VIN: LOOMSIM0000000001',offset+8,17),field('Logical address: 0x0e80',offset+25,2),field('EID: '+hex(sm),offset+27,6),field('GID: '+hex(dm),offset+33,6),field('Further action: 0',offset+39,1)]:[])]});
  else if(kind==='SOMEIP'&&!spec.payload)groups.push({name:'SOME/IP · notification',start:offset,length:payload.length,fields:[field('Service ID: 0x1234',offset,2),field('Method / Event ID: 0x8001',offset+2,2),field('Length: 12 (after length field)',offset+4,4),field('Client ID: 1',offset+8,2),field('Session ID: '+(id&65535),offset+10,2),field('Protocol version: 1',offset+12,1),field('Interface version: 1',offset+13,1),field('Message type: notification (0x02)',offset+14,1),field('Return code: E_OK',offset+15,1),field('Event value: 42',offset+16,4)]});
  else groups.push({name:'Application data · '+payload.length+' bytes',start:offset,length:payload.length,fields:[field('Payload: '+ascii(payload),offset,payload.length)]});
 }
 if(raw.length<60){const start=raw.length;raw.push(...Array(60-start).fill(0));groups.push({name:'Ethernet padding · '+(60-start)+' bytes',start,length:60-start,fields:[field('Padding: zero bytes',start,60-start)]});}
 groups.unshift({name:'Frame '+id+' · '+raw.length+' bytes on wire (FCS omitted)',start:0,length:raw.length,fields:[field('Captured length: '+raw.length+' bytes',0,raw.length)]});
 return {id,ch:ch.id,alias:ch.alias,time:(id*.137).toFixed(6),src,dst,sp,dp,protocol,transport,info,raw,payload,offset,groups,direction:spec.direction||'RX',conversation:[src,dst].sort().join('|')+'|'+[sp,dp].sort((a,b)=>a-b).join('|')+'|'+transport};
}
function compileFilter(value){
 const s=value.trim();if(!s)return ()=>true;
 const rules=s.split(/\s*&&\s*/).map(token=>{
  token=token.trim();if(/^(ip|tcp|udp|arp|doip|someip)$/.test(token))return p=>token==='ip'?p.transport!=='ARP':token==='someip'?p.protocol==='SOMEIP':token==='doip'?p.protocol==='DoIP':p.transport===token.toUpperCase();
  let m=token.match(/^ip\.addr\s*==\s*(\d+\.\d+\.\d+\.\d+)$/);if(m&&validIP(m[1]))return p=>p.src===m[1]||p.dst===m[1];
  m=token.match(/^(tcp|udp)\.port\s*==\s*(\d+)$/);if(m&&+m[2]>0&&+m[2]<=65535)return p=>p.transport===m[1].toUpperCase()&&(p.sp===+m[2]||p.dp===+m[2]);
  m=token.match(/^frame\.len\s*(==|>|<)\s*(\d+)$/);if(m)return p=>m[1]==='>'?p.raw.length>+m[2]:m[1]==='<'?p.raw.length<+m[2]:p.raw.length===+m[2];
  m=token.match(/^channel\s*==\s*"([A-Za-z0-9_-]+)"$/);if(m)return p=>p.alias===m[1];
  throw Error('Unsupported expression: '+token+'. Use a listed field or protocol; combine with &&.');
 });return p=>rules.every(rule=>rule(p));
}
function parsePayload(text,encoding){if(encoding==='TEXT'){const a=enc(text);if(!a.length||a.length>4096)throw Error('Enter 1–4096 UTF-8 bytes.');return a;}const s=text.replace(/\s/g,'');if(!s.length||s.length%2||!/^[a-f\d]+$/i.test(s))throw Error('HEX needs complete byte pairs (00–ff), separated by optional whitespace.');if(s.length>8192)throw Error('Maximum payload is 4096 bytes.');return s.match(/../g).map(x=>parseInt(x,16));}
class Bench{
 constructor(saved){this.channels=[];this.seq=0;this.dropped=0;this.predicate=()=>true;this.filter='';this.scope='all';this.paused=null;this.transcript=[];this.templates=saved?.templates||[{name:'DoIP alive check',transport:'UDP',endpoint:'192.168.10.20',port:13400,encoding:'HEX',payload:'02 fd 00 07 00 00 00 00'},{name:'Read bench status',transport:'TCP',endpoint:'192.168.10.20',port:5000,encoding:'TEXT',payload:'GET STATUS\r\n'}];
 const configs=saved?.channels?.length?saved.channels:[{alias:'bench',ip:'192.168.10.10'},{alias:'vehicle',ip:'192.168.20.10'}];configs.slice(0,4).forEach(c=>this.addChannel(c.alias,c.ip,c.captureFilter));this.active=this.channels[0].id;
 for(let i=0;i<30;i++){const c=this.channels[i%this.channels.length];this.append(c,packet(c,++this.seq));}
 }
 addChannel(alias,ip,captureFilter='all'){if(this.channels.length>=4)throw Error('Maximum four simulated channels.');if(!/^[A-Za-z0-9_-]{1,24}$/.test(alias)||this.channels.some(c=>c.alias===alias))throw Error('Use a unique 1–24 character alias (letters, numbers, _ or -).');if(!validIP(ip))throw Error('Enter a valid IPv4 address.');const c={id:'sim'+(this.channels.length+1),alias,ip,iface:'sim'+this.channels.length,live:false,link:'UP',captureFilter:['all','udp','tcp','arp'].includes(captureFilter)?captureFilter:'all',count:0,dropped:0,packets:[],connection:null};this.channels.push(c);return c;}
 append(c,p){if(c.captureFilter!=='all'&&p.transport.toLowerCase()!==c.captureFilter)return false;c.packets.push(p);c.count++;if(c.packets.length>LIMIT){c.packets.shift();c.dropped++;this.dropped++;}return true;}
 tick(){this.channels.filter(c=>c.live).forEach(c=>this.append(c,packet(c,++this.seq)));}
 setFilter(s){const p=compileFilter(s);this.predicate=p;this.filter=s.trim();}
 source(){return this.paused===null?this.channels.flatMap(c=>c.packets):this.paused;}
 rows(){return this.source().filter(p=>(this.scope==='all'||p.ch===this.scope)&&this.predicate(p)).sort((a,b)=>a.id-b.id);}
 pause(){if(this.paused===null)this.paused=this.channels.flatMap(c=>c.packets);else this.paused=null;}
 connect(c,transport,remote,port){if(!['TCP','UDP'].includes(transport)||!validIP(remote)||!Number.isInteger(+port)||+port<1||+port>65535)throw Error('Use a valid IPv4 endpoint and port 1–65535.');c.connection={transport,remote,port:+port};}
 send(c,text,encoding){if(!c.connection)throw Error('Connect this channel before sending.');const payload=parsePayload(text,encoding);const con=c.connection;const tx=packet(c,++this.seq,{kind:con.transport,transport:con.transport,remote:con.remote,dp:con.port,payload,reverse:false,direction:'TX',info:'Socket test TX · '+payload.length+' bytes'});const rx=packet(c,++this.seq,{kind:con.transport,transport:con.transport,remote:con.remote,dp:con.port,payload,reverse:true,direction:'RX',info:'Socket test RX · deterministic echo · '+payload.length+' bytes'});[tx,rx].forEach(p=>{const captured=c.live&&this.append(c,p);this.transcript.push({...p,captured:!!captured});});this.transcript=this.transcript.slice(-80);return [tx,rx];}
 saveTemplate(t){if(!t.name.trim())throw Error('Give the packet a name.');parsePayload(t.payload,t.encoding);if(!validIP(t.endpoint)||!Number.isInteger(+t.port)||+t.port<1||+t.port>65535)throw Error('Template endpoint is invalid.');const i=this.templates.findIndex(x=>x.name===t.name.trim());t={...t,name:t.name.trim()};if(i<0)this.templates.push(t);else this.templates[i]=t;this.templates=this.templates.slice(-30);}
 settings(){return {channels:this.channels.map(({alias,ip,captureFilter})=>({alias,ip,captureFilter})),templates:this.templates};}
}
const api={Bench,packet,compileFilter,parsePayload,validIP,checksum,hex,ascii,LIMIT,PREFIX};if(typeof module!=='undefined')module.exports=api;root.EtherBench=api;
})(typeof globalThis!=='undefined'?globalThis:this);
