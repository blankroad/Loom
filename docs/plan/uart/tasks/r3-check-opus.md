docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R3 사실 확인 — uart-opus (짧게, 15분 이내)

리드가 plan.md를 통합 중이다. 아래만 확인해서 docs/plan/uart/r3-facts-opus.md 에 결과(항목/결과/출처 URL/확신도)로 적어라. 파일 쓰기는 docs/plan/uart/ 만. 사용자 설정(~/.claude, ~/.codex)은 읽기만 하고 수정 금지.

1. Codex CLI(OpenAI)가 SKILL.md 형식 스킬을 어디서 발견하는가(프로젝트 경로/사용자 경로, 예: .agents/skills, .codex/skills, ~/.codex/skills 등), frontmatter 요구사항이 Claude Code와 같은가. 공식 문서/저장소로 확인. 로컬에 codex가 있으면 --help 등 읽기 전용으로 확인 가능.
2. Claude Code 프로젝트 스킬 경로 .claude/skills/<name>/SKILL.md 와 references 상대 링크 방식 확인(공식 문서).
3. 결론: 저장소 단일 원본 skills/loom-uart/ 를 두 에이전트가 같이 쓰려면 어떤 배치(심링크/복사/설치 명령)가 가장 단순한지 3줄 추천.

완료 시 화면에 "R3 CHECK DONE: docs/plan/uart/r3-facts-opus.md" 한 줄만 출력하고 멈춘다.
