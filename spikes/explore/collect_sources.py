"""Public upstream evidence only; no device access."""
import concurrent.futures,datetime,json,pathlib,urllib.request
from collect_metadata import get,one
ROOT=pathlib.Path(__file__).parent
EXTRA=['logic2-automation','ch341a','pych341','pyBusPirate','python-periphery','pyserial']
REPOS=['pyocd/pyOCD','square/pylink','cs01/pygdbmi','pyvisa/pyvisa','pyvisa/pyvisa-py','pyvisa/pyvisa-sim','hardbyte/python-can','c4deszes/ldfparser','chrizog/someipy','secdev/scapy','kbandla/dpkt','KimiNewt/pyshark','bmwcarit/python-dlt','COVESA/dlt-daemon','mikiepure/pydlt','christoph2/pyxcp','christoph2/pyA2L','eblot/pyftdi','saleae/logic2-automation','pymodbus-dev/pymodbus','trezor/cython-hidapi','ronf/asyncssh','paramiko/paramiko','openatx/adbutils','willhyper/pyplin','richClubb/python-lin']
def repo(r):
 out={'repo':r,'url':'https://github.com/'+r}
 for label,path in [('commit','commits?per_page=1'),('issue_activity','issues?state=all&sort=updated&direction=desc&per_page=1')]:
  try: out[label]=get('https://api.github.com/repos/'+r+'/'+path)
  except Exception as e: out[label]={'error':str(e)}
 return out
if __name__=='__main__':
 with concurrent.futures.ThreadPoolExecutor(max_workers=6) as p: rows=list(p.map(repo,REPOS)); extras=list(p.map(one,EXTRA))
 (ROOT/'github-evidence.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'note':'issues API includes pull requests; latest updated item is activity, not maintainer responsiveness','repos':rows},ensure_ascii=False,indent=2))
 (ROOT/'pypi-extra-evidence.json').write_text(json.dumps(extras,ensure_ascii=False,indent=2))
 for r in rows:
  c=r['commit'];i=r['issue_activity'];print(r['repo'],c[0]['commit']['committer']['date'] if isinstance(c,list) and c else c,i[0]['updated_at'] if isinstance(i,list) and i else i)
 for r in extras: print(r['package'],r.get('version'),r.get('release_date'),str(r.get('license'))[:45],r.get('error',''))
