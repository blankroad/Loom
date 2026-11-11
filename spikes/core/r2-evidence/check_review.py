"""R2 리뷰의 형식·링크·입력 근거를 검사한다. 입력 문서는 변경하지 않는다."""
import hashlib,json,os,pathlib,re
ROOT=pathlib.Path('/Users/ctmctm/Desktop/01_PROJECT/Loom')
OUT=ROOT/'spikes/core/r2-evidence'
review=ROOT/'docs/plan/core/r2-sol-review.md'
s=review.read_text()
assert len(s.splitlines())<=400
assert re.findall(r'^## ([1-7])\.',s,re.M)==list('1234567')
print('리뷰 형식 확인',flush=True)
broken=[]
for target in re.findall(r'\]\(([^\s)]+)\)',s):
    if '://' in target or target.startswith('#'):continue
    normalized=pathlib.Path(os.path.normpath(str(review.parent/target.split('#')[0])))
    if not normalized.exists():broken.append(target)
assert not broken,broken
print('문서 링크 확인',flush=True)
inputs=['docs/plan/core/r1-opus.md','docs/plan/core/r1-sol.md','docs/plan/02-core-principle.md','docs/plan/00-master-plan.md']
inputs += ['docs/plan/_xteam/'+p+'.md'for p in ['can-core','uart-core','uds-core','core-test','core-ui','test-domains','ui-test','ui-panel-template']]
assert not re.search(r'[\u3040-\u30ff\u4e00-\u9fff]',s)
result={'ok':True,'review_lines':len(s.splitlines()),'required_sections':7,'broken_links':broken,'read_inputs':inputs,'review_sha256':hashlib.sha256(s.encode()).hexdigest(),'scope':'리뷰 형식·로컬 링크·리뷰 hash. 입력 문서 전체 hash 수집은 제외. 제품 동작·Rust 보조probe 통과를 뜻하지 않음.'}
(OUT/'review-validation.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
print(json.dumps({'ok':True,'review_lines':result['review_lines'],'inputs':len(inputs),'broken_links':broken},ensure_ascii=False))
