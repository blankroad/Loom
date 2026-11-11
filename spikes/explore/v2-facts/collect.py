"""Read-only upstream metadata/source collection; no package installation."""
import concurrent.futures, datetime, hashlib, json, pathlib, urllib.request

ROOT = pathlib.Path(__file__).resolve().parent
CRATES = 'calamine csv gherkin cucumber quick-junit minijinja askama cel-interpreter cel serde_json_path jaq jaq-core jaq-std jaq-json tonic prost tonic-prost tonic-prost-build tokio dlt-core russh ssh2 openssh probe-rs visa-rs hidapi tokio-modbus'.split()

def fetch(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'Loom-planning-facts/1.0', 'Accept': 'application/json'})
    with urllib.request.urlopen(req, timeout=40) as r:
        return r.read()

def one(name):
    url = f'https://crates.io/api/v1/crates/{name}'
    try:
        raw = fetch(url)
        d = json.loads(raw)
        (ROOT / 'raw').mkdir(exist_ok=True)
        (ROOT / 'raw' / f'{name}.json').write_bytes(raw)
        stable = [v for v in d['versions'] if not v['yanked'] and '-' not in v['num']]
        v = stable[0]
        return {'crate': name, 'version': v['num'], 'released': v['created_at'], 'license': v['license'], 'rust_version': v.get('rust_version'), 'repository': d['crate']['repository'], 'url': url}
    except Exception as e:
        return {'crate': name, 'url': url, 'error': str(e)}

if __name__ == '__main__':
    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
        rows = list(pool.map(one, CRATES))
    result = {'retrieved_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'selection': 'newest non-yanked stable version by crates.io versions ordering', 'crates': rows}
    (ROOT / 'crates.json').write_text(json.dumps(result, ensure_ascii=False, indent=2))
    for row in rows:
        print(json.dumps(row, ensure_ascii=False))
