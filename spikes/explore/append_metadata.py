import json,pathlib
r=pathlib.Path('spikes/explore');doc=pathlib.Path('docs/plan/explore/r1-sol.md')
rows=json.loads((r/'pypi-evidence.json').read_text())['packages']+json.loads((r/'pypi-extra-evidence.json').read_text())
licenses={'pyvisa':'MIT','pyvisa-py':'MIT','pyvisa-sim':'MIT','pyocd':'Apache-2.0','pylink-square':'Apache-2.0','pygdbmi':'MIT','someipy':'메타데이터 공란; upstream GPL-3.0','hidapi':'메타데이터 공란; upstream 복수 라이선스','logic2-automation':'메타데이터 공란; upstream Apache-2.0','python-dlt':'공란/다른 프로젝트','dpkt':'BSD','pya2ldb':'GPLv2 metadata; source GPLv2, SPDX 범위 재확인','pya2l':'BSD metadata'}
s='''
### PyPI 버전·지원 Python 상세

조회 URL은 각 패키지 링크의 `/json` API다. `requires_python` 공란은 무제한 호환 보장이 아니다.
BSD/GPL 계열의 정확한 배포 의무는 채택할 wheel/sdist의 LICENSE까지 고정해야 한다.
pya2ldb README badge와 license metadata가 일관되지 않아 허용 라이선스로 완화 해석하지 않았다.

| 패키지 | 최신 버전 | 마지막 릴리스 UTC | Requires-Python | 라이선스/주의 |
|---|---|---|---|---|
'''
for x in rows:
 if 'version' not in x:continue
 name=x['package'];lic=licenses.get(name,x.get('license_expression') or x.get('license') or '미확인');lic=lic.replace('\n',' ')[:100]
 s+=f"| [{name}](https://pypi.org/project/{name}/) | {x['version']} | {(x.get('release_date') or '미확인')[:10]} | `{x.get('requires_python') or '공란'}` | {lic} |\n"
s+='''
`python-dlt` 0.2.1은 이름 변경 안내용이며 automotive 후보로 사용하지 않는다.
현재 PyPI `dlt` 1.32.0의 summary도 data loading library다. BMW 저장소와 혼동 금지다.
공식 automotive wrapper는 검증한 commit hash를 source URL로 고정하는 방식부터 검토한다.
PyPI `sigrok`는 stefanhoelzl/python-sigrok 별도 프로젝트이며 libsigrok 공식 SWIG binding 그 자체가 아니다.
404 후보는 `ch341`, `ch341a`, `pych341`, `pyBusPirateLite`, `pyBusPirate`, `saleae-automation`, `python-flexray`다.
Saleae의 실제 패키지명은 `logic2-automation`이며 import 경로는 `saleae.automation`이다.

### GitHub 최근 활동 확인

REST API는 rate limit로 대부분 403을 반환했다. 이 실패를 숨기지 않고 별도 JSON에 보존했다.
대신 공개 기본 브랜치 commit Atom과 이슈 목록 HTML을 읽어 아래 날짜를 얻었다.
커밋은 Atom 첫 entry 시각이고, 이슈는 정렬된 목록 HTML에서 관측한 최신 날짜다.
이슈 날짜는 사용자 댓글·생성·업데이트 표시에 대한 근사 활동 지표이며 maintainer 응답 SLA가 아니다.
라이브러리 릴리스가 오래됐어도 최근 소스 활동이 있을 수 있으므로 두 값을 함께 판단한다.

| 저장소 | 기본 브랜치 최신 commit UTC | 이슈 목록 관측 최신 날짜 | 해석 |
|---|---|---|---|
'''
for x in json.loads((r/'upstream-evidence.json').read_text())['repos']:
 name=x['repo'];c=x.get('last_commit',{});dates=x.get('issue_dates_observed',[])
 s+=f"| [{name}](https://github.com/{name}) | [{c.get('date','미확인')[:10]}]({c.get('url','https://github.com/'+name)}) | {dates[0][:10] if dates else '날짜 추출 없음/미확인'} | {'2026년 커밋 관측' if c.get('date','').startswith('2026') else '활동 간격 주의'} |\n"
with doc.open('a') as f:f.write(s)
