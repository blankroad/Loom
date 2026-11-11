"""Read-only public metadata collection for the Explore planning spike."""
import concurrent.futures, datetime, json, pathlib, urllib.request
ROOT=pathlib.Path(__file__).parent
PACKAGES='pyocd pylink-square pygdbmi pyvisa pyvisa-py pyvisa-sim python-can ldfparser someipy scapy dpkt pyshark python-dlt pydlt pyxcp pya2ldb pya2l pyftdi ch341 pyBusPirateLite saleae-automation pymodbus hidapi asyncssh paramiko adbutils python-lin pyplin python-flexray'.split()
def get(url):
    req=urllib.request.Request(url, headers={'User-Agent':'Loom-Explore-Planning/1.0','Accept':'application/json'})
    with urllib.request.urlopen(req,timeout=25) as r: return json.load(r)
def one(name):
    url=f'https://pypi.org/pypi/{name}/json'
    try:
        d=get(url); i=d['info']; files=d['releases'].get(i['version'],[])
        row={'package':name,'url':url,'version':i['version'],'release_date':min((x['upload_time_iso_8601'] for x in files),default=None),'requires_python':i['requires_python'],'license':i.get('license'),'license_expression':i.get('license_expression'),'classifiers':i.get('classifiers'),'project_urls':i.get('project_urls'),'requires_dist':i.get('requires_dist')}
        (ROOT/'pypi-raw').mkdir(exist_ok=True)
        (ROOT/'pypi-raw'/f'{name}.json').write_text(json.dumps(d,ensure_ascii=False,indent=2))
        return row
    except Exception as e: return {'package':name,'url':url,'error':str(e)}
if __name__=='__main__':
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool: rows=list(pool.map(one,PACKAGES))
    out={'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'method':'PyPI JSON info.version (index advertised latest; prerelease status not separately inferred)','packages':rows}
    (ROOT/'pypi-evidence.json').write_text(json.dumps(out,ensure_ascii=False,indent=2))
    for r in rows: print(r['package'],r.get('version'),r.get('release_date'),r.get('requires_python'),str(r.get('license_expression') or r.get('license'))[:80],r.get('error',''),r.get('project_urls'))
