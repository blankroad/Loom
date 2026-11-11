import concurrent.futures,datetime,hashlib,json,pathlib,urllib.request
ROOT=pathlib.Path(__file__).parent
URLS={
'downloads':'https://sigrok.org/wiki/Downloads',
'demo-wiki':'https://sigrok.org/wiki/Demo_device',
'homebrew':'https://formulae.brew.sh/api/formula/sigrok-cli.json',
'libsigrok-brew':'https://formulae.brew.sh/api/formula/libsigrok.json',
'cli-man':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/doc/sigrok-cli.1',
'cli-input':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/input.c',
'cli-output':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/output.c',
'cli-decode':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/decode.c',
'cli-session':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/session.c',
'cli-main':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/main.c',
'lib-make-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/Makefile.am',
'demo-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/src/hardware/demo/api.c',
'demo-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/demo/api.c',
'hantek-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/src/hardware/hantek-6xxx/api.c',
'hantek-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/hantek-6xxx/api.c',
'kingst-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/kingst-la2016/api.c',
'kingst-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/src/hardware/kingst-la2016/api.c',
'dslogic-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/dreamsourcelab-dslogic/api.c',
'rigol-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/rigol-ds/api.c',
'siglent-master':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/siglent-sds/api.c',
'kingst-wiki':'https://sigrok.org/wiki/Kingst_LA2016',
'dslogic-wiki':'https://sigrok.org/wiki/DreamSourceLab_DSLogic',
'hantek-bl':'https://sigrok.org/wiki/Hantek_6022BL',
'fx2':'https://sigrok.org/wiki/Fx2lafw',
'dwfpy-pypi':'https://pypi.org/pypi/dwfpy/json',
'ad3':'https://digilent.com/shop/analog-discovery-3/',
'waveforms-downloads':'https://digilent.com/reference/software/waveforms/waveforms-3/start',
'waveforms-notes':'https://digilent.com/reference/software/waveforms/waveforms-3/reference-manual',
'rigol-price':'https://www.rigolna.com/products/digital-oscilloscopes/dho800/',
'siglent-price':'https://www.siglentna.com/digital-oscilloscopes/sds800x-hd/',
'rigol-guide':'https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf',
'siglent-guide':'https://www.siglent.com/wp-content/uploads/2024/09/SDS800XHD_Series_ProgrammingGuide_EN11G.pdf',
'hantek-download':'https://www.hantek.com/download?key=yhsc&pid=0&sid=0&word=2000',
'pyvisa-py':'https://pyvisa-py.readthedocs.io/en/latest/installation.html'
}
def one(kv):
 k,u=kv
 try:
  with urllib.request.urlopen(urllib.request.Request(u,headers={'User-Agent':'Mozilla/5.0 Loom R2c research'}),timeout=25) as r:b=r.read();status=r.status
  name=k+('.pdf' if u.endswith('.pdf') else '.txt');(ROOT/name).write_bytes(b)
  return {'id':k,'url':u,'status':status,'file':name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
 except Exception as e:return {'id':k,'url':u,'error':str(e)}
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as p:rows=list(p.map(one,URLS.items()))
(ROOT/'sources.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sources':rows},indent=2))
for r in rows:print(r['id'],r.get('status',r.get('error')),r.get('bytes'))
