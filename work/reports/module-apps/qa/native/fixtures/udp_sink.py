import socket,sys,os,json,time
d=sys.argv[1]; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(("127.0.0.1",0))
open(f"{d}/ready.json","w").write(json.dumps({"pid":os.getpid(),"port":s.getsockname()[1]}))
f=open(f"{d}/rx.ndjson","a",buffering=1); s.settimeout(1.0); end=time.time()+10800
while time.time()<end and not os.path.exists(f"{d}/stop"):
  try: b,a=s.recvfrom(65535)
  except socket.timeout: continue
  f.write(json.dumps({"wall_ns":time.time_ns(),"from":f"{a[0]}:{a[1]}","hex":b.hex(),"len":len(b)})+"\n")
