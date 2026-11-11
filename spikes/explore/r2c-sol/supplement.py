import concurrent.futures,json,pathlib,urllib.request,hashlib
R=pathlib.Path(__file__).parent
urls={
'cli-decode-072':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/sigrok-cli-0.7.2/decode.c',
'cli-session-072':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/sigrok-cli-0.7.2/session.c',
'cli-man-072':'https://raw.githubusercontent.com/sigrokproject/sigrok-cli/sigrok-cli-0.7.2/doc/sigrok-cli.1',
'demo-protocol-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/src/hardware/demo/protocol.c',
'demo-header-052':'https://raw.githubusercontent.com/sigrokproject/libsigrok/libsigrok-0.5.2/src/hardware/demo/protocol.h',
'rigol-protocol':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/rigol-ds/protocol.c',
'siglent-protocol':'https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/siglent-sds/protocol.c',
'dwf-latest-manual':'https://files.digilent.com/manuals/WaveForms/3.25.1/main.html',
'dwf-3243-manual':'https://files.digilent.com/manuals/WaveForms/3.24.3/main.html',
'dwf-release':'https://files.digilent.com/Software/WaveForms3/ReleaseNotes.txt',
'ad3-reference':'https://digilent.com/reference/test-and-measurement/analog-discovery-3/reference-manual',
'ad3-facts':'https://digilent.com/blog/analog-discovery-3-just-the-facts/',
'hantek-user':'https://www.hantek.com/uploadpic/hantek/files/20250326/DSO2000%20Series%20Digital%20Storage%20Oscilloscope%20User%20Manual20250326154940.pdf',
'hantek-catalogue':'https://www.hantek.com/uploadpic/hantek/files/20230915/Hantek%20Catalogue%202023.pdf',
}
def one(kv):
 k,u=kv
 try:
  with urllib.request.urlopen(urllib.request.Request(u,headers={'User-Agent':'Mozilla/5.0'}),timeout=20) as f:b=f.read()
  n=k+('.pdf' if u.endswith('.pdf') else '.txt');(R/n).write_bytes(b);return {'id':k,'url':u,'status':200,'file':n,'sha256':hashlib.sha256(b).hexdigest()}
 except Exception as e:return {'id':k,'url':u,'error':str(e)}
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as p:rows=list(p.map(one,urls.items()))
a=R/'sources.json';d=json.loads(a.read_text());d['sources'].extend(rows);a.write_text(json.dumps(d,indent=2));print('\n'.join(x['id']+' '+str(x.get('status',x.get('error'))) for x in rows))
