"""Run a bounded observer against an already-connected, isolated DoIP test target.
No sockets or ECU libraries: both sender and observer use the supplied Loom CLI.
"""
import argparse,hashlib,json,os,queue,subprocess,threading,time
from pathlib import Path

p=argparse.ArgumentParser();p.add_argument('--cli',required=True);p.add_argument('--home',required=True);p.add_argument('--target',default='live');p.add_argument('--output',required=True);a=p.parse_args()
env={**os.environ,'LOOM_HOME':a.home,'LOOM_CLIENT':'uds-live-observer-after','LOOM_NO_AUTOSTART':'1'}
args=[a.cli,'--json','events','--domain','uds','--channel',a.target,'--since','now','--max','2','--follow','--timeout','8','--peek']
binary_sha256=hashlib.sha256(Path(a.cli).read_bytes()).hexdigest()
started=time.monotonic_ns();proc=subprocess.Popen(args,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1);frames=[];q=queue.Queue()
def consume():
 for line in proc.stdout:
  try:r={'received_ns':time.monotonic_ns(),'while_follow_running':proc.poll() is None,'envelope':json.loads(line)}
  except ValueError:r={'parse_error':line}
  frames.append(r);q.put(r)
thread=threading.Thread(target=consume);thread.start();initial=q.get(timeout=5)
if initial.get('envelope',{}).get('data',{}).get('count')!=0:
 proc.terminate();proc.wait();thread.join();raise RuntimeError('observer did not begin with an empty snapshot')
sender={**env,'LOOM_CLIENT':'uds-live-sender-after'};requests=[]
for request in [['uds','read-did','F190','-t',a.target],['uds','raw','22FFFF','-t',a.target]]+[['uds','read-did','F190','-t',a.target] for _ in range(8)]:
 t0=time.monotonic_ns();r=subprocess.run([a.cli,'--json',*request],env=sender,capture_output=True,text=True,timeout=10);requests.append({'args':request,'start_ns':t0,'end_ns':time.monotonic_ns(),'exit':r.returncode,'envelope':json.loads(r.stdout)})
proc.wait(timeout=12);thread.join(timeout=2);stderr=proc.stderr.read();events=[]
for f in frames:
 d=f.get('envelope',{}).get('data',{})
 for e in d.get('events',[]):events.append({'received_ns':f['received_ns'],'epoch':d.get('stream_epoch'),'event':e})
ids=[r['envelope'].get('data',{}).get('request_id') or r['envelope'].get('error',{}).get('request_id') for r in requests]
checks={'initial_empty':True,'observer_exit0':proc.returncode==0,'no_parse_errors':all('parse_error' not in f for f in frames),'streamed_before_exit':any(f.get('while_follow_running') and f.get('envelope',{}).get('data',{}).get('count',0)>0 for f in frames),'unique_epoch_seq':len(events)==len({(e['epoch'],e['event']['seq']) for e in events}),'all_request_ids':all(ids),'each_request_tx_rx_result':all(all(any(e['event'].get('request_id')==i and e['event']['kind']==k for e in events) for k in ['uds.tx','uds.rx','uds.result']) for i in ids),'nrc_truthful':requests[1]['envelope'].get('error',{}).get('code')=='E_NRC','bounded_batch_max2':all(f.get('envelope',{}).get('data',{}).get('count',0)<=2 for f in frames)}
first=next((e for e in events if e['event'].get('request_id')==ids[0]),None);latency_ms=(first['received_ns']-requests[0]['start_ns'])/1e6 if first else None;checks['first_event_under_1000ms']=latency_ms is not None and 0<=latency_ms<1000
result={'cli_sha256':binary_sha256,'command':args,'checks':checks,'first_event_delay_ms':latency_ms,'requests':requests,'frames':frames,'events':events,'observer_stderr':stderr,'elapsed_ms':(time.monotonic_ns()-started)/1e6}
Path(a.output).write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps({'checks':checks,'first_event_delay_ms':latency_ms,'observed_events':len(events),'request_ids':ids,'frame_count':len(frames)}));raise SystemExit(0 if all(checks.values()) else 1)
