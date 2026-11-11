"""CORE R1용 공개 1차 자료 수집. 전역 패키지 설치 없이 실행한다."""
import concurrent.futures, datetime, html, json, pathlib, re, time, urllib.request

ROOT = pathlib.Path(__file__).resolve().parent
OUT = ROOT / 'evidence'
OUT.mkdir(exist_ok=True)
CRATES = '''tokio axum tokio-tungstenite fastwebsockets rust-embed serde serde_json simd-json postcard bincode rmp-serde rkyv prost capnp flatbuffers interprocess shared_memory iceoryx2 jsonrpsee schemars ts-rs specta typify rusqlite redb arrow parquet zstd lz4_flex windows win32job process-wrap clap tracing tracing-subscriber tikv-jemallocator mimalloc dhat sysinfo cargo-dist'''.split()
SOURCES = {
 'tokio-pipe': 'https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html',
 'interprocess': 'https://docs.rs/interprocess/latest/interprocess/local_socket/index.html',
 'bincode': 'https://github.com/bincode-org/bincode',
 'postcard': 'https://postcard.jamesmunns.com/wire-format',
 'rmp-serde': 'https://docs.rs/rmp-serde/latest/rmp_serde/',
 'jsonrpsee': 'https://docs.rs/jsonrpsee/latest/jsonrpsee/',
 'schemars': 'https://graham.cool/schemars/',
 'ts-rs': 'https://docs.rs/ts-rs/latest/ts_rs/',
 'typify': 'https://docs.rs/typify/latest/typify/',
 'specta': 'https://docs.rs/specta/latest/specta/',
 'datamodel-code-generator': 'https://github.com/koxudaxi/datamodel-code-generator',
 'quicktype': 'https://github.com/glideapps/quicktype',
 'sqlite-wal': 'https://sqlite.org/wal.html',
 'sqlite-sync': 'https://sqlite.org/pragma.html#pragma_synchronous',
 'redb': 'https://docs.rs/redb/latest/redb/',
 'job-object': 'https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects',
 'job-limits': 'https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information',
 'windows-memory': 'https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex',
 'cgroup': 'https://docs.kernel.org/admin-guide/cgroup-v2.html',
 'linux-status': 'https://man7.org/linux/man-pages/man5/proc_pid_status.5.html',
 'prlimit': 'https://man7.org/linux/man-pages/man2/prlimit.2.html',
 'iceoryx2': 'https://docs.rs/iceoryx2/latest/iceoryx2/',
 'cargo-dist': 'https://github.com/axodotdev/cargo-dist',
 'iceoryx-platform': 'https://github.com/eclipse-iceoryx/iceoryx2',
 'bincode-current': 'https://docs.rs/bincode/latest/bincode/',
 'bincode2-source': 'https://docs.rs/crate/bincode/2.0.1/source/src/lib.rs',
 'axum-source': 'https://docs.rs/axum/latest/src/axum/extract/ws.rs.html',
 'shared-memory': 'https://docs.rs/shared_memory/latest/shared_memory/',
 'simd-json': 'https://docs.rs/simd-json/latest/simd_json/',
 'jsonrpc': 'https://www.jsonrpc.org/specification',
 'canfd': 'https://www.can-cia.org/can-knowledge/can-fd-the-basic-idea',
}
def get(url):
    req = urllib.request.Request(url, headers={'User-Agent':'Loom-core-planning/0.1 (local research)'})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=35) as r: return r.read().decode(), r.status
        except Exception:
            if attempt == 2: raise
            time.sleep(attempt + 1)
def crate(name):
    url = f'https://crates.io/api/v1/crates/{name}'
    try:
        raw, status = get(url)
        (OUT / f'crate-{name}.json').write_text(raw)
        d = json.loads(raw); c = d['crate']
        versions = [v for v in d['versions'] if not v['yanked'] and not re.search(r'[a-zA-Z]', v['num'])]
        v = versions[0]
        return {'name':name,'version':v['num'],'license':v['license'],'released_at':v['created_at'],'downloads_total':c['downloads'],'recent_downloads':c.get('recent_downloads'),'repository':c['repository'],'documentation':c.get('documentation'),'rust_version':v.get('rust_version'),'api':url,'status':status}
    except Exception as e: return {'name':name,'error':str(e),'api':url}
def source(item):
    key,url = item
    try:
        raw,status=get(url)
        (OUT / f'{key}.html').write_text(raw)
        clean=re.sub(r'<(script|style)\b[^>]*>.*?</\1>', '', raw, flags=re.S|re.I)
        clean=html.unescape(re.sub('<[^>]+>', ' ', clean))
        clean=re.sub(r'\s+', ' ', clean)
        (OUT / f'{key}.txt').write_text(clean)
        return {'key':key,'url':url,'status':status,'chars':len(clean)}
    except Exception as e:return {'key':key,'url':url,'error':str(e)}
if __name__=='__main__':
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        crates=list(pool.map(crate, CRATES)); sources=list(pool.map(source,SOURCES.items()))
    stamp=datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUT/'metadata.json').write_text(json.dumps({'fetched_at':stamp,'crates':crates,'sources':sources},ensure_ascii=False,indent=2))
    print(json.dumps({'fetched_at':stamp,'crates':len(crates),'sources':len(sources),'errors':[x for x in crates+sources if 'error' in x]},ensure_ascii=False,indent=2))
