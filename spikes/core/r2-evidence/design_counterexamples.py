"""계획의 명시 조건을 적용한 산술·정보 손실 반례. 제품 실행 결과가 아니다."""
import hashlib,itertools,json,pathlib,zlib

OUT=pathlib.Path(__file__).resolve().parent
MIB=1024**2
rows={}
rows['subscriber_budget']={
    'connections':64,'subscriptions_per_connection':32,
    'total_subscriptions':64*32,
    'logical_queue_capacity_mib':64*32*(2+1),
    'document_gateway_hard_mib':512,
    'document_own_subtotal_with_64_queues_mib':64+64*(2+1)+32+64,
    'note':'논리 용량 합이다. Arc 공유·overlap으로 실제 RSS가 반드시6144 MiB인 것은 아니지만 aggregate byte budget의 증명도 아니다.'}
source=['a1','a2','b1','b2']
interleavings=[p for p in itertools.permutations(source)
    if p.index('a1')<p.index('a2') and p.index('b1')<p.index('b2')]
rows['lost_stream_mapping']={'same_producer_journals':{'a':['a1','a2'],'b':['b1','b2']},'valid_global_orders':interleavings,'order_count':len(interleavings),'note':'시각도 같다고 두면 원본만으로 어느 전역순번을 사용했는지 결정할 수 없다.'}
event={'origin':'device','kind':'can.frame','rpm':1000}
plan_expect=lambda e:e['rpm']>=2900
caller_pred=lambda e:e['origin']=='device'
rows['unbound_predicate']={'plan_expected_match':plan_expect(event),'citation_supplied_match':caller_pred(event),'device_witness':True,'note':'caller pred를 그대로 검증하면 계획 기대는 false여도 O4/O5의 명시 조건은 만족 가능. 구현 버그 발견이 아니라 누락된 plan-binding 조건의 반례.'}
rows['publish_timeout']={'covers':42,'published':41,'wait_ms':50,'warning':'publish_lag','reply_returned':True,'invariant_published_before_reply':False}
remaining=1
approved=[remaining>0,remaining>0]
sends=sum(approved)
rows['approval_check_then_consume']={'initial_uses':remaining,'both_check_before_send':approved,'send_count':sends,'note':'consume이 backend 이후이고 in-flight reservation이 없으면1회 승인으로2개 송신 가능. 원자적 reserve/rollback/unknown 상태가 필요.'}
rows['storage_arithmetic']={'mean_record_bytes_from_sol_spike':580.77776,'can_average_events_s':15000,'hours':24,'can_only_gib':15000*580.77776*86400/1024**3,'twenty_gib_seconds':20*1024**3/(15000*580.77776),'note':'Opus의5k상시+20k 절반시간 =15k 평균. 정본 형식·압축비가 다른 workload의 계획용 산술이며 실측 저장량이 아니다.'}
rows['uart_budget']={'ports':8,'ring_mib_per_port':16,'ring_only_mib':8*16,'worker_soft_mib':64,'worker_hard_mib':128,'journal_queue_cap_mib':64}
def crc32c(data):
    crc=0xffffffff
    for b in data:
        crc^=b
        for _ in range(8):crc=(crc>>1)^(0x82f63b78 if crc&1 else 0)
    return crc^0xffffffff
rows['crc_polynomial']={'input':'123456789','ieee_crc32':f'{zlib.crc32(b"123456789"):08x}','castagnoli_crc32c':f'{crc32c(b"123456789"):08x}','equal':False}
assert len(interleavings)==6 and sends==2
assert rows['subscriber_budget']['logical_queue_capacity_mib']==6144
(OUT/'design-counterexamples.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2))
print(json.dumps(rows,ensure_ascii=False,indent=2))
