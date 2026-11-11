docs/plan/02-core-principle.md 준수 (먼저 읽고 r1-addendum-core.md 도 따를 것)

# R1 지시 — uds-opus (Claude Opus 5.5)

당신은 Loom 프로젝트 UDS 컴포넌트의 워커다. 리드는 uds-lead. **계획 단계이며 구현 코드는 쓰지 않는다.** 한국어로 쓴다.

## 먼저 읽을 것
1. `docs/plan/00-brief.md` (공통 브리프, 특히 §3 공통 계약, §4 산출물 15개 절, §5 규칙)
2. `docs/plan/uds/brief.md` (UDS 브리프)
3. `docs/plan/can/brief.md` (CAN 팀 브리프 — CAN-TP 호스팅, loom.viz 소유 팀)
4. 있으면 참고: `docs/plan/_xteam/` 안의 파일, `docs/plan/can/` 진행 중 초안 (읽기만)

## 할 일
`docs/plan/uds/r1-opus.md`에 `00-brief.md` §4의 **15개 절 전체** 초안을 쓴다. 다른 워커(uds-sol)가 같은 주제로 독립 초안을 쓰고 있으니, 당신은 아래 관점에 **무게**를 둔다(다른 절도 빠짐없이 쓴다):

- **아키텍처**: 프로세스 구성(UDS 데몬/세션 관리자가 어디 사는지 — 독립 데몬 vs core 데몬 내 모듈, DoIP 소켓 소유, CAN-TP는 CAN 데몬 위), 세션 상태(활성 세션, 보안 레벨, TesterPresent 타이머)를 누가 들고 있는지, 다중 ECU/다중 클라이언트(에이전트+사용자) 동시 접근 시 직렬화/락 규칙.
- **에이전트/사용자 UX**: `loom uds ...` CLI 명령 체계 전체(connect/disconnect/status, session, security, read-did, write-did, dtc list/clear, routine, io, reset, raw, run scenario.yaml, flash 여부, discover), 사람이 읽는 출력 예시와 `--json` 예시, 출력 상한. 사용자 트레이스 뷰(터미널 TUI, herdr 패인) 화면 목업(ASCII), 출처(agent/user) 표시, NRC 해석, P2 타이밍 표시.
- **스킬 설계**: `.claude/skills/loom-uds/SKILL.md` 초안 개요(짧게) + references 파일 구성, 에이전트가 지켜야 할 절차(연결→상태 확인→읽기 우선→위험 동작 전 확인). Codex 호환(AGENTS.md 등) 방안.
- **안전 규칙**: 서비스·서브기능별 위험 등급표(0x10/0x11/0x14/0x19/0x22/0x23/0x27/0x28/0x2E/0x2F/0x31/0x34-0x37/0x3D/0x3E/0x85 등), 등급별 확인 방식(자동 허용/사용자 확인/명시적 플래그+확인 토큰/금지), dry-run, ECU 화이트리스트, 레이트 제한, 감사 로그, `raw` 명령이 가드레일을 우회하지 못하게 하는 방법(raw 바이트를 서비스 ID로 분류), 시나리오 YAML 실행 시 사전 위험 요약.
- 진단 DB(사용자 YAML/JSON DID 정의 스키마 초안 포함, ODX/CDD는 범위 판단), seed-key 플러그인 인터페이스 초안.
- CAN-TP 경계: UDS가 CAN 데몬의 ISO-TP 채널에 바라는 API(open/close/send PDU/recv PDU, 기능 주소, 파라미터, 에러 보고) 요구사항 목록.
- viz 요구사항(DID 폴링 플롯, 요청/응답 트레이스 표) 목록.

## 규칙
- 파일 쓰기는 `docs/plan/uds/`만. 다른 곳(특히 `_xteam/`) 쓰지 말 것 — 리드가 한다.
- 사실(라이브러리 버전·라이선스 등)을 적을 땐 확인했으면 출처를, 추정이면 "추정"이라고 표시.
- 결정이 필요한 곳은 근거와 함께 하나를 골라 제안하고, 대안을 짧게 남긴다.
- 완료되면 응답으로 "DONE r1-opus" 한 줄만 남긴다.
