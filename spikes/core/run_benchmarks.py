"""빌드 완료 후 벤치마크를 순차 실행한다. 실행마다 독립 결과 디렉터리를 만든다."""
import datetime,hashlib,json,pathlib,platform,subprocess,sys
ROOT=pathlib.Path(__file__).resolve().parent
stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
OUT=ROOT/'runs'/stamp
OUT.mkdir(parents=True)
env={'started_at':stamp,'os':platform.platform(),'arch':platform.machine(),'cpu':'Apple M1 (4 performance + 4 efficiency cores)','ram_bytes':16*1024**3,'build':'cargo release, default allocator, default profile, no LTO override','rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'cargo':subprocess.check_output(['cargo','--version'],text=True).strip(),'warning':'macOS 한 머신의 합성 부하. Windows/Linux·브라우저 렌더·24 h soak 결과가 아니다. 동시 사용자/다른 에이전트 부하는 통제하지 못함.'}
sources={}
for project in ['worker-ipc','ws-batch','journal']:
    for rel in ['Cargo.toml','Cargo.lock','src/main.rs']:
        source=ROOT/project/rel
        dest=OUT/'sources'/project/rel
        dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(source.read_bytes())
        sources[f'{project}/{rel}']=hashlib.sha256(source.read_bytes()).hexdigest()
env['source_sha256']=sources
(OUT/'environment.json').write_text(json.dumps(env,ensure_ascii=False,indent=2))
def run(name,args,cwd):
    p=subprocess.run(args,cwd=cwd,text=True,capture_output=True,timeout=180)
    (OUT/(name+'.stderr')).write_text(p.stderr)
    if p.returncode:raise RuntimeError((name,p.returncode,p.stderr))
    d=json.loads(p.stdout);(OUT/(name+'.json')).write_text(json.dumps(d,ensure_ascii=False,indent=2));print(name,flush=True)
for rep in range(1,4):
    run(f'ipc-{rep}',[str(ROOT/'worker-ipc/target/release/core-worker-ipc-spike')],ROOT/'worker-ipc')
for codec in ['json','binary','column']:
    for n,slow in [(1,False),(4,False),(4,True)]:
        for rep in range(1,3):
            run(f'ws-{codec}-{n}-'+('slow-' if slow else '')+str(rep),[str(ROOT/'ws-batch/target/release/core-ws-batch-spike'),codec,str(n)]+(['slow']if slow else []),ROOT/'ws-batch')
jcwd=OUT/'journal-work';jcwd.mkdir()
run('journal',[str(ROOT/'journal/target/release/core-journal-spike')],jcwd)
(ROOT/'latest-run.txt').write_text(str(OUT.relative_to(ROOT))+'\n')
print(str(OUT),flush=True)
