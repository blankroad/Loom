"""Bounded review probes; no physical device, no Loom product implementation."""
import asyncio,datetime,json,pathlib,statistics
ROOT=pathlib.Path(__file__).parent
async def ssh_channels():
 import asyncssh
 class Server(asyncssh.SSHServer):
  def begin_auth(self,username):return False
 async def handler(process):
  process.stdout.write(process.command+':begin\n')
  await asyncio.sleep(.05)
  process.stderr.write(process.command+':stderr\n');process.exit(0)
 key=asyncssh.generate_private_key('ssh-ed25519')
 server=await asyncssh.create_server(Server,'127.0.0.1',0,server_host_keys=[key],process_factory=handler)
 port=server.get_port();known=asyncssh.import_known_hosts(f'[127.0.0.1]:{port} '+key.export_public_key().decode())
 events=[]
 try:
  async with asyncssh.connect('127.0.0.1',port=port,username='sim',known_hosts=known) as conn:
   async def run(label):
    events.append({'channel':label,'kind':'request','origin':'agent'})
    p=await conn.create_process(label,request_pty=False)
    first=await p.stdout.readline();events.append({'channel':label,'kind':'stdout','origin':'device','data':first,'exit_status_at_read':p.exit_status})
    error=await p.stderr.read();await p.wait_closed();events.append({'channel':label,'kind':'completed','stderr':error,'exit_status':p.exit_status})
   await asyncio.wait_for(asyncio.gather(run('job1'),run('job2')),2)
 finally:server.close();await server.wait_closed()
 assert len(events)==6 and all(e['exit_status_at_read'] is None for e in events if e['kind']=='stdout')
 return {'status':'pass','connections':1,'exec_channels':2,'events':events,'scope':'API streaming before exit only; event list is local evidence, not a Loom viewer'}
def phase_calc():
 waits=[1000-i for i in range(1000)]
 return {'kind':'analytical model, NOT benchmark','poll_period_ms':1000,'uniform_event_phase_samples':1000,'detection_ms_max':max(waits),'detection_ms_p95':sorted(waits)[949],'fraction_within_500ms':sum(x<=500 for x in waits)/len(waits),'transport_and_render_added':False}
from pydlt import DltMessage,ArgumentString,MessageType,MessageLogInfo,StorageHeader
m=DltMessage.create_verbose_message([ArgumentString('test')],MessageType.DLT_TYPE_LOG,MessageLogInfo.DLT_LOG_INFO,'CAMS','CAM',ecu_id='ECU1');wire=m.to_bytes()
try:DltMessage.create_from_bytes(wire,with_storage_header=True);parse='unexpectedly accepted'
except Exception as e:parse=type(e).__name__+': '+str(e)
checks={'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'ssh':asyncio.run(ssh_channels()),'status_poll_math':phase_calc(),'dlt_wire':{'length':len(wire),'prefix_hex':wire[:4].hex(),'storage_magic_hex':'444c5401','parsed_as_storage':parse}}
(ROOT/'behavior-results.json').write_text(json.dumps(checks,ensure_ascii=False,indent=2));print(json.dumps(checks,ensure_ascii=False,indent=2))
