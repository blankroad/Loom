# Loom 계약 v1

상태: **v1.0 (P0, 태그 `core-p0-d2`와 동기)** · 소유: CORE 팀 · 변경: `CHANGELOG.md` + 도메인 팀 리뷰
근거: 마스터 계획 `docs/plan/00-master-plan.md` — 우선순위 **F > E > C > B > A > D**(§2.5 F1, §2.4 E1–E12, §2.3 C1–C8, §2.2 B, §2.1 A, §2 D). 상세 설계: `docs/plan/core/plan.md` v2.0.

> **최우선 원칙**(`docs/plan/02-core-principle.md`): 에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간에 UI에서 보고 개입한다. 이 계약의 모든 항목은 이 루프를 위한 것이다.

## 정본 관계
- **타입 정본 = Rust crate `crates/loom-proto`**(C3). JSON Schema·TS·Python 타입은 여기서 생성한다. 문서와 타입이 다르면 타입이 맞고, 문서를 고친다(CHANGELOG에 기록).
- 이 폴더의 문서는 타입이 표현하지 못하는 **의미·순서·기본값·상한**을 정한다.
- 도메인별 세부(이벤트 kind의 payload 스키마, 위험 등급 표, 도메인 명령)는 각 도메인 crate가 `Registry`에 등록하는 것이 정본이고, 도메인 팀 `plan.md`가 설명한다.

## 문서
| 파일 | 내용 |
|---|---|
| `01-processes-ipc.md` | `loomd`·gateway·worker, WS JSON-RPC, hello·role·rights, 구독·배치·우선 큐, publish-before-reply, HTTP, 이름 규칙 |
| `02-events.md` | 이벤트 레코드, 두 층 순번, 시간, origin·dir, payload, 쓰기 상태, 손실 3층, 코어 이벤트 kind |
| `03-cli.md` | `loom` CLI 봉투, 종료 코드·오류 코드, 출력 상한, 커서, `others_since_last`, 자동 기동 |
| `04-safety.md` | 위험 등급, **F1 기본 모두 자동**, 설정으로 켜는 확인·승인·차단, STOP·비상 OFF·복구, 감사, 기록 불능 |
| `05-worker-protocol.md` | worker 프로토콜 `loom-worker/1`, lane·프레임·크레딧, 감독·재기동, 메모리 예산 |
| `06-journal.md` | 저널 세그먼트 형식, 내구성, stream mapping, 보존 |
| `07-test-session.md` | 테스트 세션, 단계 마커, 증거 REF, 검증 규칙, 종료 코드 |
| `08-domain-integration.md` | 도메인 등록(`DomainModule`), 훅, crate 경계, P0 태그와 가져오는 방법 |
| `CHANGELOG.md` | 변경 이력 |

## 용어
- **gateway**: `loomd` 본 프로세스(IPC·허브·정책·감사·저널 조정·도메인 gateway 모듈).
- **worker**: 장치를 단독 소유하는 자식 프로세스(`loomd worker <kind>`).
- **허브**: gateway 안 단일 시퀀서. stream seq를 붙이고 구독자에게 팬아웃한다.
- **viewer**: 사용자 UI 연결(웹). **agent**: `loom` CLI 연결. **script**: 러너·테스트 세션 내부 actor.
- MUST / SHOULD / MAY는 RFC 2119 의미로 쓴다(한국어 문장 안에서 "반드시 / 권장 / 허용").
