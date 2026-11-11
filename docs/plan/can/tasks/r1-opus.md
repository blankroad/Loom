docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선함)

# R1 지시 — can-opus

1. 먼저 `docs/plan/can/tasks/r1-common.md`를 읽고 따른다.
2. 출력: `docs/plan/can/r1-opus.md`

## 당신의 관점 (무게를 더 둘 것)
- **아키텍처**: 데몬 프로세스 구성, 채널/버스 추상화, 이벤트 버스, 기록 파이프라인, 클라이언트(CLI·TUI·웹) 연결 방식, ISO-TP 호스팅 위치. 데이터 흐름을 다이어그램으로.
- **사용자/에이전트 UX**: CANoe 사용자가 익숙한 Trace/Graphics/IG/Statistics 경험을 어떻게 재현할지 화면 레이아웃 수준으로. 에이전트 CLI의 출력 형태(사람용/`--json`), 출력 상한, 커서("마지막 본 이후") 개념.
- **스킬 설계**: `.claude/skills/loom-can/`, `loom-viz/` SKILL.md 구성(짧게) + references 파일 목록과 각 내용 요약, 에이전트가 따를 워크플로(연결 확인 → DB 로드 → 관찰 → 송신 등). Codex 호환 방안.
- **안전 규칙**: 송신 위험 등급, 사용자 확인, dry-run, 레이트 제한, 채널별 tx 허용/금지(listen-only), 실차/벤치 구분, 감사 로그.
- 라이브러리·타이밍 등도 15개 절 모두 빠짐없이 쓰되, 깊은 사실 조사는 Sol 쪽이 맡으니 확신 없는 건 "미확인"으로 남겨도 된다.
