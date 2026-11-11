docs/plan/02-core-principle.md 준수 (최우선 — 이 문서보다 우선)

# CORE R1 공통 지시 (core-opus, core-sol 둘 다 읽을 것)

당신은 Loom **CORE 팀**(Rust 코어) 워커다. 리드는 core-lead. 이번 라운드는 **독립 초안**: 상대 워커 초안을 보지 말고 혼자 쓴다. 한국어로 쓴다.

## 먼저 읽기 (순서대로)
1. `docs/plan/02-core-principle.md` — 최우선 원칙과 체크리스트
2. `docs/plan/core/brief.md` — CORE 팀 범위 10개 항목과 산출물
3. `docs/plan/00-master-plan.md` — 특히 §2 D1–D14, §2.1 A1–A16, §2.2 B1–B7, **§2.3 C1–C8 (최우선, C > B > A > D)**, §3·§5
4. `docs/plan/00-brief.md` — §4의 15절 구성, §5 규칙 (§3은 C1–C8로 대체됨)
5. 참고(필요 부분만 grep으로 찾아 읽기 — 파일이 크다):
   - `docs/plan/uart/plan.md` §15 "남은 요청 (CORE 계약 v1에서 확정 필요)"(1200행 근처), 데몬·worker·기록 절
   - `docs/plan/can/plan.md` §3(worker 분리 실측), §6(기록), §12.1, §15
   - `docs/plan/uds/plan.md` §0, §3.1, §6.2, §15
   - `docs/plan/explore/plan.md` §6(core에 요구할 것), §7.1(러너), §10
   - `docs/plan/_xteam/*.md` 전부 (특히 `can-uds-isotp.md`의 RPC 경계)
   - `docs/plan/ui/brief.md`, `docs/plan/test/brief.md` (UI·TEST 팀이 지금 동시에 계획 중. `docs/plan/ui/`, `docs/plan/test/`에 진행 중 초안이 생기면 읽어도 됨)
   - 다른 팀이 CORE에 보낸 요청 파일 `docs/plan/_xteam/*core*.md` 가 생겼으면 반드시 읽고 반영

## 배경 한 줄
사용자는 Python 코어가 **느리고 메모리 문제로 자꾸 죽는** 경험을 했다. 1순위 품질 목표 = **오래 켜 두어도 죽지 않고, 메모리가 늘지 않고, 장치→화면 지연이 낮은 코어**. 코어는 "장치 worker → 이벤트 허브 → (에이전트 CLI, 사용자 UI) 동시 전달, 쓰기는 정책·승인·감사를 거쳐 worker로" 의 배관이다.

## 산출물 형식 (둘 다 동일)
`00-brief.md` §4의 **15절 전체**를 갖춘 계획 초안. 맨 앞에 반드시:
- **§0 핵심 루프 표**: 코어 관점에서 4요소(직접 접근·읽기·쓰기·사용자 실시간 표시+개입)가 각각 어떤 코어 부품·경로로 구현되는지.
- **종단 간 시나리오 1개 이상**: 구체적 명령(`loom uart send dut "reboot\n"` 등) → worker → 허브 → UI 화면에 몇 ms 안에 → 사용자 개입(UI에서 입력/STOP/승인)까지, 각 홉의 지연 예산(ms)과 그 홉의 큐 상한 포함.
- **실시간성·고부하 목표치**: 홉별 지연 예산, CAN 100% 부하·UART 폭주 시 화면이 멈추거나 거짓말하지 않는 방법(드롭 표시·다운샘플·우선 큐).
- **M1 = 걷는 뼈대**(시뮬레이션 백엔드 위 4요소 최소 동작)와 **P0 첫 산출물 = 스키마 + 인터페이스 stub crate**(도메인 팀이 첫 주에 쓸 수 있게: 어떤 crate, 어떤 trait·타입, 어떤 fixture, 어떤 sim worker 견본).
- 원칙에 기여하지 않는 기능은 "후순위/비범위"로 명시.
- 결정마다 **근거**를 쓰고, 실제 확인한 사실(문서 URL, crate 버전·라이선스·최근 릴리스, 스파이크 측정값)과 추정을 구분 표시 (`[확인]`, `[추정]`).

## 반드시 다룰 결정 (brief 범위 10항목 — 각각 선택안 비교표 + 추천)
1. 프로세스 구조: `loomd` gateway + 장치 worker 프로세스. Rust 도메인을 gateway 안 crate로 둘지 worker 프로세스로 둘지의 **판정 기준**(벤더 DLL 크래시 격리, 타이밍 전용 스레드, 메모리 예산). 마스터 D1의 "UDS는 gateway 안"이 Rust에서도 맞는지 재검토.
2. **worker 프로토콜**: Rust worker와 Python 어댑터 worker(CANoe COM)가 같은 프로토콜. 전송(파이프·UDS 소켓·named pipe·공유 메모리 링), 직렬화(바이너리 권장 근거 — 후보 비교), 프레이밍, 버전 협상, 흐름 제어·배압, 감독·재기동(지수 백오프, 크래시 루프 차단), 하트비트, 메모리 예산 집행(Windows Job Object, Linux cgroup/rlimit, macOS), 쓰기 요청 경로(A1 쓰기 클래스 3종)와 publish-before-reply(D2).
3. IPC(D2: JSON-RPC 2.0 over WS, role·origin·token), 이벤트 허브(seq 두 층 D3·snapshot·우선 제어 큐·gap·적응형 배치 A5), 고속 데이터의 UI 전송(JSON vs 바이너리 WS 프레임·열 단위 배치 — UI 팀과 `_xteam/core-ui.md`에서 합의할 초안 내용).
4. 저널·기록(D7): 무손실 디스크 저널, 인덱스, 조회, 재생, 보존·quota, fsync 정책. 형식 비교(자체 세그먼트 vs SQLite vs Arrow/Parquet vs 혼합)와 근거. 메모리 링 = 최근 화면용, 원본 = 디스크(C2).
5. CLI `loom`(D5 봉투·종료 코드·커서), 데몬 자동 기동(D8), **기동 시간 목표**(에이전트가 자주 부름 — 예: `loom uart send` 전체 왕복 p95).
6. 정책·승인·감사(D6, A8–A12, B4), STOP(A3), 쓰기 파이프라인과 도메인 훅(A1, A2, A14). 감사 해시 체인 구현, 정책 파일 2층, 완화=뷰어 승인.
7. 스키마 정본 = Rust 타입(serde) → JSON Schema → TS/Python 생성(C3). 도구 선택, 버전 규칙, CI에서 drift 검사.
8. 러너 코어(D12, A15)와 **테스트 세션 코어**(C7): 세션·단계·마커·증거 인용 검증의 코어 부분. TEST 기획과 `_xteam/core-test.md`로 합의할 초안 내용.
9. C2 성능·메모리: **큐 상한 표**(모든 큐: 이름·위치·상한·넘칠 때 정책 drop/block/spill·관측 지표), 프로세스별 메모리 예산, 24 h soak 관문(RSS 증가 상한 수치), 크래시 격리 시험, 측정 하네스.
10. 빌드·배포: Windows x64·Linux x64 실HW, macOS 개발. 단일 바이너리 배포 의미(loomd+loom+웹 정적 파일 임베드? worker는 별도 실행 파일?), CI 3 OS, cargo workspace crate 구조, 도메인 팀과의 경계(`crates/loom-core` 등 CORE만 수정, 도메인 crate가 의존하는 안정 API 표면).

## 마일스톤·작업 분해
- §11 마일스톤 M0(P0 stub), M1(걷는 뼈대), M2…: 각각 수용 기준을 측정 가능하게.
- §12 작업 분해: 병렬 가능한 단위, 각 작업에 추천 담당(Opus 워커 / Sol 워커), 의존성, 그 작업을 기다리는 타팀 표시.

## 규칙
- 파일 쓰기는 `docs/plan/core/`, `docs/plan/_xteam/`(지시된 파일만), `spikes/core/` 만.
- 스파이크는 `spikes/core/` 안에서 짧은 검증 코드만. 실제 구현 시작 금지. Rust 스파이크는 `spikes/core/<이름>/` 별도 cargo 프로젝트, `target/`도 그 안에. `cargo install`·rustup 컴포넌트 추가·brew·전역 pip 금지. crates.io 의존성 받는 것은 허용.
- 사용자 설정 파일(~/.claude, ~/.codex 등) 수정 금지. 다른 팀 폴더는 읽기만.
- 다른 에이전트에게 herdr로 메시지 보내지 말 것.
- 끝나면 화면에 산출 파일 경로와 3줄 요약만 출력.
