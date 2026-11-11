# R3 사실 확인 — uart-opus (스킬 배치)

- 작성: uart-opus, 2026-11-09. docs/plan/02-core-principle.md 준수.
- 확인 방법: 공식 문서 WebFetch(2026-11-09), 로컬 `codex-cli 0.166.0` 바이너리 문자열 검사(읽기 전용), `~/.codex`·`~/.agents`·`~/.claude` 목록 조회만 함(수정 없음).

## 1. Codex CLI — 스킬 발견 경로와 frontmatter

| 항목 | 결과 | 출처 | 확신도 |
|---|---|---|---|
| 저장소(프로젝트) 경로 | `$CWD/.agents/skills`, 그리고 **CWD에서 저장소 루트까지 모든 상위 디렉터리의 `.agents/skills`**(`$REPO_ROOT/.agents/skills`는 모든 하위 폴더에 적용) | https://learn.chatgpt.com/docs/build-skills (developers.openai.com/codex/skills에서 308 리다이렉트) | 높음 |
| 사용자 경로(문서) | `$HOME/.agents/skills` | 같은 문서 | 높음 |
| 사용자 경로(레거시, 문서에는 없음) | `$CODEX_HOME/skills`(기본 `~/.codex/skills`)도 여전히 발견 대상. 내장 `skill-installer`도 여기에 설치한다 | 로컬 바이너리 문자열: 발견 범주 `codex-home-skills`, `.agents/skills`, `user-agent-skills`, `system-skills`, 그리고 "Installs into `$CODEX_HOME/skills/<skill-name>` (defaults to `~/.codex/skills`)". 로컬에 `~/.codex/skills/*/SKILL.md` 실제 존재 | 중간 (비문서화 → 향후 제거 가능) |
| 관리자/시스템 | `/etc/codex/skills`, Codex 내장(skill-creator, skill-installer) | 공식 문서 | 높음 |
| `.codex/skills`(프로젝트) | **문서에 없음**. 프로젝트의 `.codex/`는 설정 파일(config.toml 등)용 | 공식 문서 | 높음 |
| 이름 충돌 | 병합하지 않음. 같은 `name`이 둘 다 선택지에 나타날 수 있음 | 공식 문서 | 높음 |
| 심링크 | 지원: "follows the symlink target when scanning these locations" | 공식 문서 | 높음 |
| frontmatter | **`name`, `description` 필수**. 그 외 필드·길이 제한은 Codex 문서에 명시 없음 → Agent Skills 표준을 따른다: `name` 1–64자, 소문자·숫자·하이픈, **디렉터리 이름과 같아야 함**, `--` 금지, `description` 1–1024자, 선택 필드 `license`, `compatibility`, `metadata`, `allowed-tools`(실험적) | Codex 문서 + https://agentskills.io/specification | 높음 |
| Codex 전용 부가 파일 | 선택 `agents/openai.yaml`(UI 표시명, `policy.allow_implicit_invocation` 기본 true, `dependencies.tools`). 없어도 동작 | Codex 문서 | 높음 |
| 하위 폴더 | `scripts/`, `references/`, `assets/` 선택. SKILL.md에서 링크하는 방식은 Codex 문서에 명시 없음 → 표준: **스킬 루트 기준 상대 경로, 한 단계 깊이**(`[cli](references/cli.md)`) | Codex 문서, agentskills.io §File references | 높음 |

## 2. Claude Code — 프로젝트 스킬 경로와 references

| 항목 | 결과 | 출처 | 확신도 |
|---|---|---|---|
| 프로젝트 경로 | `.claude/skills/<skill-name>/SKILL.md`. 시작 디렉터리부터 **저장소 루트까지 상위 디렉터리**에서 로드. 중첩 `<subdir>/.claude/skills/`는 해당 폴더 파일을 처음 읽을 때 로드 | https://code.claude.com/docs/en/skills | 높음 |
| 개인/기업/플러그인 | `~/.claude/skills/<name>/SKILL.md`, 관리 설정 디렉터리, `<plugin>/skills/<name>/SKILL.md`. 충돌 시 enterprise > personal > project | 같은 문서 | 높음 |
| `.agents/skills` | Claude Code 문서에 **언급 없음** → Claude Code는 `.agents/skills`를 읽지 않는 것으로 간주 | 같은 문서 | 높음 |
| 심링크 | 지원: personal/project/enterprise 위치의 `<skill-name>` 항목은 다른 곳 디렉터리를 가리키는 심링크여도 되고, 같은 대상은 한 번만 로드 | 같은 문서 | 높음 |
| frontmatter | 모든 필드 선택(`name`은 디렉터리 이름이 기본값, `description` 권장). `description`+`when_to_use`는 목록에서 1,536자로 잘림. 모르는 필드는 무시. Claude 확장 필드(`allowed-tools`, `context`, `agent`, `hooks`, `paths` 등)는 claude.ai/Skills API 업로드 시 오류 → **이식성을 위해 `name`+`description`만 쓴다** | 같은 문서 | 높음 |
| references 링크 | SKILL.md에서 **상대 Markdown 링크**(`[reference.md](reference.md)`)로 연결하면 필요할 때 로드. 문서 예시는 파일을 스킬 루트에 두지만 하위 폴더 상대 링크도 같은 방식(표준의 `references/` 관례와 호환). 스크립트 경로는 `${CLAUDE_SKILL_DIR}` 권장(Claude 전용 치환 — Codex 호환 본문에서는 피함). SKILL.md는 500줄 이하 권장 | 같은 문서 | 높음 (하위 폴더 링크는 표준 근거, Claude 문서 예시는 루트만 — 중간) |

### 계획서에 반영할 정정
- r1-opus §4.8의 "Codex: `.agents/skills/` 또는 `~/.codex/skills/` [추정]" → **프로젝트 = `.agents/skills/`(상위 탐색), 사용자 = `~/.agents/skills/`(문서) / `~/.codex/skills/`(레거시, 비문서)**로 확정.
- 두 에이전트 공통 frontmatter: `name: loom-uart`(디렉터리 이름과 동일), `description` ≤ 1024자(Claude 1,536자 한도보다 엄격한 쪽). 다른 필드는 쓰지 않는다.
- 본문은 `references/*.md` 상대 링크(한 단계)만 쓰고 `${CLAUDE_SKILL_DIR}` 같은 에이전트 전용 치환은 쓰지 않는다.

## 3. 결론 — 단일 원본 배치 추천 (3줄)

1. 원본은 `skills/loom-uart/`(SKILL.md + `references/`) 하나만 두고, 저장소에 **상대 심링크 2개를 커밋**한다: `.claude/skills/loom-uart → ../../skills/loom-uart`, `.agents/skills/loom-uart → ../../skills/loom-uart`. 두 에이전트 모두 공식적으로 심링크를 따라가고 저장소 루트까지 상위 탐색하므로, 저장소 안 어디서 시작해도 설치 단계 없이 발견된다.
2. frontmatter는 표준 공통분모(`name`=디렉터리 이름, `description` ≤ 1024자)만 쓰고, 링크는 `references/…` 상대 경로로 한정한다. 이러면 같은 파일을 Claude Code와 Codex가 그대로 읽는다(`agents/openai.yaml`은 필요할 때만 추가).
3. Windows 체크아웃에서 git 심링크가 텍스트 파일로 풀리는 경우(`core.symlinks=false`, 개발자 모드 꺼짐 [추정])에 대비해 `loom skills install --copy`(두 경로로 복사, 사용자 전역 위치에는 쓰지 않음)를 대체 경로로 둔다. CI에서는 두 경로의 SKILL.md가 원본과 동일한지 검사한다.
