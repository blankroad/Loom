"""Bounded backend feasibility checks, NOT a Loom implementation or viewer."""
import asyncio,datetime,importlib.metadata,inspect,json,pathlib,platform,socket,statistics,subprocess,time,traceback
ROOT=pathlib.Path(__file__).parent.resolve();results=[]
def check(name,fn):
 try: results.append({'name':name,'status':'pass','result':fn()})
 except Exception: results.append({'name':name,'status':'fail','error':traceback.format_exc()})
def visa():
 import pyvisa
 rm=pyvisa.ResourceManager(str(ROOT/'psu.yaml')+'@sim');i=rm.open_resource('TCPIP::localhost::INSTR');i.timeout=500;i.read_termination='\n';i.write_termination='\n'
 ident=i.query('*IDN?');i.write('VOLT 12.000');i.write('CURR 0.200');i.write('OUTP 1');assert float(i.query('VOLT?'))==12;assert i.query('OUTP?')=='1'
 times=[]
 for _ in range(100):t=time.perf_counter_ns();i.query('VOLT?');times.append((time.perf_counter_ns()-t)/1e6)
 i.write('OUTP 0');assert i.query('OUTP?')=='0';i.close();rm.close()
 return {'idn':ident,'write_read':'VOLT=12,CURR=.2,OUTP=1 then OUTP=0','n':100,'query_ms_median':statistics.median(times),'query_ms_p95':sorted(times)[94],'scope':'in-process YAML simulator; no USB/TCP physical timing'}
def modbus():
 async def run():
  from pymodbus.client import AsyncModbusTcpClient
  from pymodbus.server import StartAsyncTcpServer,ServerAsyncStop
  from pymodbus.datastore import ModbusDeviceContext,ModbusSequentialDataBlock,ModbusServerContext
  probe=socket.socket();probe.bind(('127.0.0.1',0));port=probe.getsockname()[1];probe.close()
  ctx=ModbusServerContext(devices=ModbusDeviceContext(hr=ModbusSequentialDataBlock(1,[0]*20)),single=True)
  task=asyncio.create_task(StartAsyncTcpServer(ctx,address=('127.0.0.1',port)))
  client=AsyncModbusTcpClient('127.0.0.1',port=port,timeout=.5,retries=0)
  try:
   for _ in range(40):
    if await client.connect():break
    await asyncio.sleep(.025)
   assert client.connected
   a=await client.write_register(0,1234,device_id=1);b=await client.read_holding_registers(0,count=1,device_id=1);assert not a.isError() and b.registers==[1234]
   return {'address':'loopback ephemeral','write':1234,'read':b.registers}
  finally:
   client.close();await ServerAsyncStop();task.cancel();await asyncio.gather(task,return_exceptions=True)
 return asyncio.run(run())
def virtual():
 import can
 a=can.Bus(interface='virtual',channel='explore-factcheck');b=can.Bus(interface='virtual',channel='explore-factcheck')
 try:
  a.send(can.Message(arbitration_id=0x123,is_extended_id=False,data=[1,2]),timeout=.2);r=b.recv(.2);assert r and bytes(r.data)==b'\x01\x02'
  return {'frame':'123#0102','scope':'single-process python-can virtual; not vcan or XCP'}
 finally:a.shutdown();b.shutdown()
def dlt():
 from pydlt import DltMessage,ArgumentString,MessageType,MessageLogInfo
 m=DltMessage.create_verbose_message([ArgumentString('frame 42 lat=1.25ms')],MessageType.DLT_TYPE_LOG,MessageLogInfo.DLT_LOG_INFO,'CAMS','CAM',ecu_id='ECU1')
 raw=m.to_bytes();parsed=DltMessage.create_from_bytes(raw,with_storage_header=False);assert parsed.to_bytes()==raw
 return {'bytes':len(raw),'hex':raw.hex(),'message':str(parsed),'scope':'V1 log codec only; no daemon/control roundtrip'}
def gdb():
 s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close()
 p=subprocess.Popen(['/opt/homebrew/bin/qemu-system-arm','-M','lm3s6965evb','-S','-gdb',f'tcp:127.0.0.1:{port}','-nographic','-monitor','none','-serial','none'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 c=None
 try:
  for _ in range(60):
   try:c=socket.create_connection(('127.0.0.1',port),.2);break
   except OSError:time.sleep(.02)
  assert c;c.settimeout(1)
  def rsp(payload):
   data=payload.encode();c.sendall(b'$'+data+b'#'+f'{sum(data)%256:02x}'.encode());buf=b''
   while b'$' not in buf:buf+=c.recv(1)
   out=b''
   while True:
    x=c.recv(1)
    if x==b'#':break
    out+=x
   checksum=c.recv(2);assert int(checksum,16)==sum(out)%256;c.sendall(b'+');return out.decode()
  assert rsp('M20000000,4:78563412')=='OK';read=rsp('m20000000,4');assert read=='78563412'
  return {'machine':'lm3s6965evb','write':'M20000000,4:78563412','read':read,'scope':'QEMU GDB RSP halted SRAM only; no target firmware/RTT/SWD throughput'}
 finally:
  if c:c.close()
  p.terminate()
  try:p.communicate(timeout=2)
  except subprocess.TimeoutExpired:p.kill();p.communicate()
def ssh():
 async def run():
  import asyncssh
  class Server(asyncssh.SSHServer):
   def begin_auth(self,username):return False
  async def handler(process):process.stdout.write('sim-target:'+process.command+'\n');process.exit(0)
  key=asyncssh.generate_private_key('ssh-ed25519');server=await asyncssh.create_server(Server,'127.0.0.1',0,server_host_keys=[key],process_factory=handler)
  port=server.get_port();known=asyncssh.import_known_hosts(f'[127.0.0.1]:{port} '+key.export_public_key().decode())
  try:
   async with asyncssh.connect('127.0.0.1',port=port,username='sim',known_hosts=known) as conn:
    r=await conn.run('status',timeout=1);assert r.stdout=='sim-target:status\n';return {'stdout':r.stdout,'exit_status':r.exit_status,'scope':'ephemeral local SSH server with verified generated host key; no real OS command'}
  finally:server.close();await server.wait_closed()
 return asyncio.run(run())
for name,fn in [('pyvisa-sim-psu',visa),('pymodbus-tcp',modbus),('python-can-virtual',virtual),('pydlt-codec',dlt),('qemu-gdb-memory',gdb),('asyncssh-loopback',ssh)]:check(name,fn)
versions={d.metadata['Name']:d.version for d in importlib.metadata.distributions()}
(ROOT/'backend-results.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'python':platform.python_version(),'platform':platform.platform(),'versions':versions,'checks':results},ensure_ascii=False,indent=2))
for r in results:print(json.dumps(r,ensure_ascii=False))
