# feature-team-coding — 모듈 workspace 수명 규칙 보완

상태: **수정본 작성·검증·현재 작업 적용 완료 / 설치본 반영 미완료**. 메인이 직접 최종 편집·검토했다. Opus 최초 수정 후 외부 working-directory Read 승인 요청을 Esc로 취소했고, 이미 허용된 메인 파일 접근으로 인수·중복 정리·검증했다. 전체 외부 읽기 허용이나 권한 설정 변경은 하지 않았다. 독립 forward-test를 받았다고 주장하지 않는다.

## 최종 규칙

- 새 독립 코딩 모듈의 구현·조사·리뷰·검증마다 전용 Herdr workspace를 생성한다. MAIN split으로 대체하지 않는다. 같은 모듈의 수정·재검증은 같은 workspace를 재사용한다.
- 생성 주체, 작업 파일, cwd와 실제 workspace/tab/pane/session ID를 기록한다. `--no-focus`를 유지한다. UI workspace와 Git worktree/보안 sandbox는 별개다.
- 결과·검사·미커밋·재개 로그 인수 → 자신이 시작한 서버/에이전트 정상 종료 → 해당 module workspace close → 목록에서 제거 확인까지 한다. 사용자의 이번 종료 지시 범위에서 재승인을 반복 요청하지 않는다.
- MAIN·타 사용자 공간·다른 모듈의 진행 작업은 보호한다. 소스/worktree/세션 로그 삭제는 정리가 아니다.

## 실제 적용

| 모듈 | 생성·사용 | 정상 종료·제거 | 보존 |
|---|---|---|---|
| 샌드박스 조사 | w9 / w9:t1 / w9:p1 / d99812ab-bebc-4f1c-bca4-c280d82e9b32 | /exit 후 workspace close w9 성공 | sandbox-root-cause.md, source-manifest.json, Claude 세션 로그 |
| 스킬 수명 규칙 | wA / wA:t1 / wA:p1 / 5e98aae7-046a-4a2a-ab5c-1988651eadd9 | /exit 후 workspace close wA 성공 | 이 보고서, cache 후보·patch, Claude 세션 로그 |

최종 `herdr workspace list`: **w8 MAIN만, pane_count=1**. 기존 완료된 MAIN 내부의 직접 생성 worker p6/p8도 결과 보존·/exit 후 pane close했다. 다른 사용자 공간·코드·세션 로그는 삭제하지 않았다.

## 검증과 세 상황 검토

- `quick_validate.py` 최종 후보 PASS.
- 설치본 파일 2개를 임시 디렉터리에 복제 → 패치 dry-run PASS → 임시본 실제 apply PASS → 후보와 바이트 일치. 설치 위치에는 쓰지 않았다.
- 새 ETH 검증 모듈: 새 workspace 생성, 응답 ID 기록, 그 안에서 지정 모델 foreground 시작.
- 같은 ETH 수정 후 재검증: 기존 모듈 workspace 재사용, 완료 전 새 공간을 불필요하게 만들지 않음.
- MAIN+다른 사용자 공간이 있을 때 모듈 완료: 본인이 만든 대상 ID만 인수 후 닫고 MAIN/다른 사용자 공간이 남았는지 확인.

## 수정본과 설치 상태

캐시 후보: `~/.cache/feature-team-coding-foreground/feature-team-coding/{SKILL.md,references/herdr.md}`. 기존 foreground 규칙을 유지하며 workspace 수명 절은 한 곳으로 정리했다. `foreground.patch`도 최신 전체 차이로 갱신했다.

저장소 패치: [feature-team-coding-workspace.patch](../skill-updates/feature-team-coding-workspace.patch). 설치 경로 `~/.codex/skills/feature-team-coding`는 현재 메인 세션의 쓰기 허용 밖이라 설치하지 않았다. 다른 실행기로 이 쓰기 제한을 우회하지 않았다. 현재 작업에는 사용자 지시와 수정본 규칙을 직접 적용했다.

사용자가 일반 터미널에서 설치하려면(검토된 설치본 기준 패치):

```sh
patch -d "$HOME/.codex/skills/feature-team-coding" -p1 < "$HOME/workspace/Loom/docs/impl/main/skill-updates/feature-team-coding-workspace.patch"
```

설치본이 그 사이 바뀌었으면 patch 실패를 강제로 무시하지 말고 차이를 재검토한다. 설치 후 이후 대화에서 스킬을 다시 로드한다.

## 최종 SHA256

- `SKILL.md`: `a689cd68e60d3fdaacbf6bf95d749e8be568643f338ed546f31374914732679a`
- `references/herdr.md`: `b239b79ec809c7a8996fa41669b2795a442abe1b146c76366aae0cdda7e7e1a7`
- patch: `3bb45b4883cc9c71f3fd9ee5fba32991b204a04a02b862587b1a81aa6029d16c`

브라우저 실행 환경/승인/복구 지침도 같은 최신 패치에 포함했다. 추가 파일 references/browser-execution.md SHA256 `b9302330dec2d11a0a32aebd1bc0ce422de76d767f8f63c87a8522bbbee640cd`. [추가 규칙과 Codex 재확인](skill-browser-execution.md). 설치 명령은 동일하며 세 파일을 반영한다.
