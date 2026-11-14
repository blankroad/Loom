"""REPORT-ONLY validator for the PROPOSED dbg contracts (not a product test).
Run: python3 -I work/reports/ti-debugger-implementation/contracts/validate.py   (needs jsonschema>=4.18)
Checks: metaschemas; every example line valid; u64 runtime range; negative cases rejected as expected; journal payloads vs DTO defs.
Exit code 1 if any check fails."""
import json,sys
from jsonschema import Draft202012Validator as V
import os
base=sys.argv[1] if len(sys.argv)>1 else os.path.dirname(os.path.abspath(__file__))
b=json.load(open(base+'/bridge-v1.schema.json')); V.check_schema(b); vb=V(b)
d=json.load(open(base+'/dbg-dto.schema.json')); V.check_schema(d)
U64MAX=2**64-1
def u64_ok(o):
    bad=[]
    def walk(x,p):
        if isinstance(x,dict):
            for k,v in x.items(): walk(v,p+'/'+k)
        elif isinstance(x,list):
            for i,v in enumerate(x): walk(v,p+f'/{i}')
        elif isinstance(x,str) and x.isdigit() and len(x)>=19 and int(x)>U64MAX: bad.append(p)
    walk(o,''); return bad
fails=0
for f in ['bridge-session.jsonl','bridge-deadline.jsonl']:
    for i,l in enumerate(open(base+'/examples/'+f)):
        o=json.loads(l)
        if '_comment' in o: continue
        e=list(vb.iter_errors(o))
        if e or u64_ok(o): fails+=1; print('FAIL',f,i+1,[x.message[:90] for x in e][:2])
    print('checked',f)
for i,l in enumerate(open(base+'/examples/bridge-negative.jsonl')):
    o=json.loads(l); exp=o.pop('_expect'); why=o.pop('_why')
    valid=not list(vb.iter_errors(o)); rt=not u64_ok(o)
    got = 'invalid' if not valid else ('schema_valid_runtime_reject' if not rt else 'valid')
    ok= got==exp; fails+= not ok
    print('NEG', 'ok ' if ok else 'BAD', exp, '-', why)
# journal payloads vs dto defs
j=json.load(open(base+'/examples/dbg-journal.json'))
m={'dbg.run_state':'RunStatePayload','dbg.sample':'SampleBatch','dbg.mem_block':'MemBlockRef'}
for ev in j['events']:
    k=m.get(ev['kind'])
    if not k: continue
    sch={'$schema':d['$schema'],'$defs':d['$defs'],'$ref':'#/$defs/'+k}
    e=list(V(sch).iter_errors(ev['payload']))
    if e: fails+=1; print('FAIL journal',ev['seq'],[x.message[:100] for x in e][:2])
sch={'$schema':d['$schema'],'$defs':d['$defs'],'$ref':'#/$defs/WatchResult'}
e=list(V(sch).iter_errors(j['agent_watch_result_example'])); fails+=bool(e); print('WatchResult', 'ok' if not e else e[0].message)
print('TOTAL_FAILS',fails)

sys.exit(1 if fails else 0)
