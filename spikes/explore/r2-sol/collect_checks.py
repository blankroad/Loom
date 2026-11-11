"""R2 review evidence: upstream sources and public package metadata, read-only."""
import concurrent.futures,datetime,hashlib,json,pathlib,urllib.request
ROOT=pathlib.Path(__file__).parent
SOURCES={
'dlt-common-v3.0.1':'https://raw.githubusercontent.com/COVESA/dlt-daemon/v3.0.1/include/dlt/dlt_common.h',
'dlt-client-v3.0.1':'https://raw.githubusercontent.com/COVESA/dlt-daemon/v3.0.1/src/lib/dlt_client.c',
'dlt-daemon-client-v3.0.1':'https://raw.githubusercontent.com/COVESA/dlt-daemon/v3.0.1/src/daemon/dlt_daemon_client.c',
'dlt-daemon-connection-v3.0.1':'https://raw.githubusercontent.com/COVESA/dlt-daemon/v3.0.1/src/daemon/dlt_daemon_connection.c',
'dlt-protocol-v3.0.1':'https://raw.githubusercontent.com/COVESA/dlt-daemon/v3.0.1/include/dlt/dlt_protocol.h',
'pyocd-options':'https://pyocd.io/docs/options.html',
'pyocd-rtt-source':'https://raw.githubusercontent.com/pyocd/pyOCD/v0.46.1/pyocd/subcommands/rtt_cmd.py',
'someipy-license':'https://raw.githubusercontent.com/chrizog/someipy/master/LICENSE.md',
'saleae-license':'https://raw.githubusercontent.com/saleae/logic2-automation/master/LICENSE',
'pyxcp-readme':'https://raw.githubusercontent.com/christoph2/pyxcp/master/README.rst',
'plin':'https://www.peak-system.com/products/software/development-packages/plin-api/?L=1',
'asyncssh-api':'https://asyncssh.readthedocs.io/en/latest/api.html',
'numato-manual':'https://numato.com/docs/8-channel-usb-relay-module/',
}
PKGS='pyocd pylink-square pyhid-usb-relay numato-gpio telnetlib3 lauterbach-trace32-rcl pyelftools pylibftdi opencv-python logic2-automation someipy pysigrok'.split()
def fetch(item):
 name,url=item
 try:
  with urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'Mozilla/5.0 Loom-R2-review'}),timeout=25) as r:raw=r.read();status=r.status
  (ROOT/(name+'.txt')).write_bytes(raw)
  row={'name':name,'url':url,'status':status,'sha256':hashlib.sha256(raw).hexdigest()}
  if name.startswith('pypi-'):
   d=json.loads(raw);i=d['info'];f=d['releases'].get(i['version'],[])
   row['info']={k:i.get(k) for k in ['version','requires_python','license','license_expression','classifiers','project_urls','requires_dist']};row['release_date']=min([x['upload_time_iso_8601'] for x in f],default=None)
  return row
 except Exception as e:return {'name':name,'url':url,'error':str(e)}
items=list(SOURCES.items())+[(f'pypi-{p}',f'https://pypi.org/pypi/{p}/json') for p in PKGS]
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:rows=list(pool.map(fetch,items))
(ROOT/'source-evidence.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sources':rows},ensure_ascii=False,indent=2))
for x in rows:print(x['name'],x.get('status',x.get('error')),x.get('info',{}).get('version'),x.get('release_date'),str(x.get('info',{}).get('license_expression') or x.get('info',{}).get('license'))[:45])
