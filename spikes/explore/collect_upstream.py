import concurrent.futures, datetime, html, json, pathlib, re, urllib.request,xml.etree.ElementTree as ET
from collect_sources import REPOS
ROOT=pathlib.Path(__file__).parent; RAW=ROOT/'upstream-raw'; RAW.mkdir(exist_ok=True)
def fetch(url):
 with urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'Mozilla/5.0 Loom planning research'}),timeout=20) as r:return r.read().decode()
def repo(name):
 row={'repo':name};base='https://github.com/'+name; slug=name.replace('/','__')
 try:
  page=fetch(base);(RAW/(slug+'.html')).write_text(page)
  m=re.search(r'"defaultBranch":"([^"]+)"',page);branch=m[1] if m else None;row['default_branch']=branch
  if branch:
   u=base+'/commits/'+branch+'.atom';atom=fetch(u);(RAW/(slug+'.atom')).write_text(atom);tree=ET.fromstring(atom);ns={'a':'http://www.w3.org/2005/Atom'};e=tree.find('a:entry',ns)
   row['last_commit']={'date':e.findtext('a:updated',namespaces=ns),'title':e.findtext('a:title',namespaces=ns).strip(),'url':e.find('a:link',ns).attrib['href'],'source':u}
  issues=fetch(base+'/issues?q=is%3Aissue+sort%3Aupdated-desc');(RAW/(slug+'__issues.html')).write_text(issues)
  dates=re.findall(r'(?:datetime="|"updatedAt":")([0-9TZ:.-]+)',issues)
  row['issue_dates_observed']=sorted(set(dates),reverse=True)[:5];row['issue_source']=base+'/issues?q=is%3Aissue+sort%3Aupdated-desc'
 except Exception as e:row['error']=str(e)
 return row
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as p:rows=list(p.map(repo,REPOS))
(ROOT/'upstream-evidence.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'note':'Atom latest branch commit; issue dates HTML observed, not response-time metric','repos':rows},ensure_ascii=False,indent=2))
for r in rows:print(r['repo'],r.get('last_commit',{}).get('date'),r.get('issue_dates_observed'),r.get('error',''))
