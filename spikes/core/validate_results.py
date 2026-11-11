"""스파이크 산출물의 내부 일관성 확인. 제품 수용 시험을 대신하지 않는다."""
import hashlib,json,pathlib,re,sqlite3,struct
ROOT=pathlib.Path(__file__).resolve().parent
REPO=ROOT.parent.parent
RUN=ROOT/(ROOT/'latest-run.txt').read_text().strip()
checks=[]
for p in RUN.glob('ipc-*.json'):
    rows=json.loads(p.read_text());assert len(rows)==3
    for x in rows:
        assert x['rtt_n']==10000 and x['stream_n']==300000
        assert 99000<x['achieved_events_s']<101000
    checks.append(p.name)
for p in RUN.glob('ws-*.json'):
    d=json.loads(p.read_text());assert d['server']['sent_events']==300000
    assert d['server']['max_frame_bytes']<=65536
    for c in d['clients']:
        if not c['slow']:
            assert c['received_events']==300000,(p,c)
            assert c['duplicates']==0 and c['missing_inside_received_range']==0
    if any(c['slow']for c in d['clients']):assert d['server']['send_timeouts']>=1
    else:assert d['server']['queue_dropped_batches']==0
    checks.append(p.name)
for p in (RUN/'journal-work/data').glob('*.db'):
    with sqlite3.connect(f'file:{p}?mode=ro&immutable=1',uri=True) as c:
        assert c.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
        count,lo,hi=c.execute('SELECT count(*),min(seq),max(seq) FROM events').fetchone()
        assert (count,lo,hi)==(100000,0,99999)
    checks.append(p.name)
for p in (RUN/'journal-work/data').glob('seg-*-false.bin'):
    seq=0
    with p.open('rb') as f:
        while head:=f.read(4):
            assert len(head)==4
            size=struct.unpack('<I',head)[0];body=f.read(size);assert len(body)==size
            for line in body.splitlines():
                d=json.loads(line);assert int(d['source_seq'])==seq;seq+=1
    assert seq==100000
    checks.append(p.name)
env=json.loads((RUN/'environment.json').read_text())
for name,digest in env['source_sha256'].items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest,name
    assert hashlib.sha256((RUN/'sources'/name).read_bytes()).hexdigest()==digest,name
docs=[REPO/'docs/plan/core/r1-sol.md',ROOT/'README.md',ROOT/'library-evidence.md',REPO/'docs/plan/_xteam/core-test.md',REPO/'docs/plan/_xteam/core-ui.md']
for p in docs:
    s=p.read_text()
    assert s.count('```')%2==0,p
    for link in re.findall(r'\]\(([^\s)]+)\)',s):
        if '://' in link or link.startswith('#'):continue
        path=link.split('#')[0]
        assert (p.parent/path).exists(),(p,path)
s=docs[0].read_text()
assert [int(x)for x in re.findall(r'^## (\d+)\.',s,re.M)]==list(range(16))
assert not re.search(r'[\u3040-\u30ff]',s)
checks.extend(['스냅샷 source hash','문서 링크','0–15절','코드 fence'])
result={'run':str(RUN.relative_to(ROOT)),'ok':True,'checks':checks,'limitations':['zstd 청크 내용 readback 검증 미실시','실제 브라우저·Windows/Linux·24 h soak 미실행','최초 실행 정상 구독자 손실은 실패 원본으로 별도 보존']}
(RUN/'validation.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
print(json.dumps({'ok':True,'checks':len(checks),'report':str((RUN/'validation.json').relative_to(REPO))},ensure_ascii=False))
