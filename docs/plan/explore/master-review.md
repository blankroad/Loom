ISSUES
# 탐색 리드의 00-master-plan.md 검토 (2026-11-10) — 대부분 수용한다(D1 worker 선언, D2 role 부여, D5 종료 코드, D8, D10, D13, D14). 아래는 수정이 필요한 것만.
## (a) 탐색 설계를 깨는 결정 — 이유와 수정 제안
1. **D12 `finally` + D6 인터록 충돌 (안전, 가장 중요)**: `safe_state: true` 동작(PSU 출력 OFF)을 abort 시에도 실행하면, UDS 플래시나 debug flash 중에 사용자가 STOP하는 순간 러너가 전원을 끊는다. 벽돌 위험이 가장 큰 조합이며, D6 예시 "`uds.flashing` 중 PSU 전원 사이클 금지"와도 정면으로 충돌한다.
   → 제안: "`finally`의 safe_state 동작도 **인터록·deny list를 반드시 통과**하고, 막히면 skip + `run` 결과에 `finally_blocked` 기록". 승인은 run 시작 시 일괄 승인에 포함한다(abort 시점에 새 승인을 기다리지 않음).
2. **D6 R2에 정책 `allow` 경로가 없다**: R2가 매 호출 확인(토큰 또는 뷰어)이면 PSU `set`(전압 설정), debug 허용 RAM 쓰기, AWG 설정 같은 반복 벤치 동작마다 확인이 뜬다. watch→조정 루프가 사실상 불가능해진다. UART(`R2="allow"`)와 CAN(`tx=allow`)은 이미 allow 경로를 갖고 있다.
   → 제안: D6에 "env=sim은 R2 자동, bench는 사용자 정책에서 **동작 단위 `allow` 완화 가능(완화는 뷰어 승인 후 적용 — D6 2층 규칙 그대로)**, vehicle은 불가" 한 줄 추가.
3. **D3 gap 이름에 '장비/전송 쪽 손실'이 없다**: `display.gap`(화면)과 `record.gap`(기록)만 있다. 그래서 DLT MCNT 불연속·`MESSAGE_BUFFER_OVERFLOW`, RTT 타깃 드롭, sigrok 샘플 오버런, SCPI 응답 누락처럼 **Loom 수신 전에 생긴 손실**을 표현할 곳이 없다. 이를 record.gap에 섞으면 "Loom이 못 썼다"와 "장비가 안 보냈다"가 구분되지 않는다(탐색 plan §5.2 c, §6.4).
   → 제안: `record.gap`에 `source: device|transport|daemon|recorder` 필드를 필수로 넣거나, 이벤트 `source.gap`을 추가한다.
4. **D4 단계에서 `confirmed`와 `effective_verified`가 양자택일 종단으로 적혀 있다**: DLT 레벨 변경은 `confirmed`(데몬 ACK) **다음에** `effective_verified`(앱 로그로 반영 관측, 최대 2 s)가 오는 순차 관계다.
   → 제안: `confirmed → effective_verified | effect_unverified(관측창 만료)`의 전이를 허용한다. `sent`는 "드라이버에 넘김"이며 wire 완료가 아님을 명시한다(탐색 R2 지적: enqueue ≠ wire 완료).
5. **D9 STOP과 '출력 유지 장치'의 의미가 정의되지 않았다**: `loom stop-all`/`Ctrl-] x`는 TX latch다. 하지만 PSU 출력, 릴레이 KL30/KL15, AWG, halt된 코어는 latch를 걸어도 **현재 상태가 유지된다**. 사용자가 STOP을 "전원 차단"으로 오해할 수 있다.
   → 제안: STOP = 새 쓰기 차단만(자동 OFF 없음; 전원 차단은 플래시 중 벽돌 위험이 있음). 출력 유지 장치는 뷰어에 "STOP됨 — 출력 ON 유지" 배지를 띄우고, 별도의 **사용자 비상 OFF 키**(도메인 패널, 확인 없음, 인터록은 경고만)를 둔다.
6. **§3 일정: 스코프를 P2에 두었지만 웹 viz는 P3(CAN M3)**이다. 탐색 M1-scope는 웹 파형 패널을 수용 기준으로 썼다.
   → 제안(탐색 쪽에서 해결): M1-scope는 TUI(캡처 배지·측정치·스파크라인)로 수용하고, 웹 `waveform` 패널은 P3 이후 M2로 옮긴다. 메인 쪽 변경은 필요 없고, §3 표에 "스코프 M1=TUI" 주석만 부탁.
7. **D10 콘솔 엔진 분리(CR-X5)의 담당·시점이 §3에 없다**: SSH(P3)의 전제 조건인데 CORE·UART 어느 쪽 작업에도 들어 있지 않다.
   → 제안: CORE P2에 "UART 콘솔 엔진·위젯을 core 컴포넌트로 이관(UART 협업)"을 추가한다.
8. (경미) **D6 하드 차단이 "loom.toml + 뷰어 승인"으로 풀린다**: debug 옵션 바이트·RDP·보안 잠금·mass erase는 벽돌 위험이라, 탐색 plan은 플러그인 고유의 해제 불가 목록으로 두었다. 위협 모델상 보안 경계는 아니므로 결정은 존중한다.
   → 제안: 도메인이 `immutable_deny`(설정으로도 해제 불가, 사용자는 벤더 도구 사용)를 선언할 수 있게 해 달라.
## (b) 마스터가 탐색 계획을 잘못 옮긴 곳
1. **D4 "탐색 CR-X3 채택"**: 탐색 안은 `requested→approved→queued→dispatching→driver_accepted→response_received→effective_verified|failed|outcome_unknown`이다. 그중 승인 대기와 "드라이버 수락 ≠ wire 완료" 구분이 빠졌다. 축약 자체는 수용하며, (a)-4만 반영해 달라.
2. **§5 리스크 "탐색 도메인은 세 팀 M1 이후 착수"와 §3 "PSU M1 = P1(세 팀 M1과 동시)"가 서로 모순**이다. 탐색은 §3을 따르겠다(PSU가 첫 플러그인으로 계약을 검증하므로 P1이 더 낫다). §5 문구 수정을 요청한다.
3. **§3 P2 CORE "러너 core" vs EXPLORE P3 "교차 러너 M1"의 경계가 불명확**하다. 탐색 plan은 러너 전체(R-a)를 자기 작업으로 적었다.
   → 제안 분담: CORE = 잡 소유·스키마 파서·승인/인터록 연결·run 이벤트. EXPLORE = 교차 도메인 단계 어댑터(psu/dlt/io…), SimWorld 1차, run 패널·개입 UX, 교차 시나리오 수용 시험.
4. §1 표 ④행의 지연 목표 "도메인별"은 사실이지만 값이 비어 있다. 탐색 목표는 PSU/SSH 수신→화면 p95 ≤ 100 ms, DLT ≤ 150 ms, debug watch ≤ 250 ms, LA 디코드 ≤ 300 ms, 스코프 트리거→화면 ≤ 1 s이다(모두 미실측 SLO).
## (c) plan.md를 결정에 맞추려면 고칠 곳 (아직 수정 안 함)
1. 위험 등급(§5 공통 전제, §6.3 게이트, §6.5, D7):
   - R2 = 토큰 또는 뷰어. 모든 탐색 CLI에 `loom <domain> confirm <tok>` 동사를 추가한다.
   - allow 자동은 (a)-2 결정 뒤 반영한다.
   - R4 = 매번 뷰어 승인 + `default_blocked`(debug flash·주변장치 쓰기, DLT 영속 설정, 셸 efuse)로 바꾼다.
   - "R4 기본 거부"와 "deny 해제 불가" 표현을 고친다.
   - 환경 축 `sim|bench|vehicle`을 추가한다(vehicle에서 PSU·IO·debug 쓰기 기본 deny).
2. 정책 2층(§6.5): 사용자 파일을 `~/.loom/config/policy.toml`로 바꾼다. "완화는 사용자 파일에서만" 대신 "완화는 뷰어 승인 후 적용, 강화 즉시".
3. 쓰기 단계(§0 공통, §6.3, D6): 단계 이름을 D4로 바꾼다(`requested/accepted/sent/confirmed/effective_verified/failed/outcome_unknown`). DLT의 `acknowledged`는 `confirmed`로 바꾼다.
4. 이벤트(§6.4):
   - `metrics`를 맵에서 `[{name, value, unit}]` 리스트로 바꾼다.
   - gap 표기를 `display.gap`/`record.gap`/`record.health`로 바꾼다(`la.gap`, `gap{…}` 제거, 출처 필드는 (a)-3 결정에 따름).
   - 필수 필드 목록(`client_id, generation, purpose, risk` 등)을 참조한다.
   - **64 KiB 메시지 상한**에 맞춰 스코프 캡처 이벤트는 데시메이션 ≤ 2k점 + 원본은 artifact(`raw_ref`)로 한다(현재 "≤10k점 이벤트"는 상한 초과 위험).
5. CLI(§5.x d, §6.7, §0.1):
   - 종료 코드를 D5 표로 바꾼다: 사용자 중단 6 `E_INTERRUPTED_BY_USER`, 한계·deny 4, 승인 5 `E_NO_APPROVER`, outcome_unknown 7.
   - `--json` 봉투와 `others_since_last`를 모든 결과에 붙인다. 시나리오에서 `events --origin user` 호출 대신 `others_since_last`를 보여 준다.
   - 커서 `@c41`/`@p118`를 `seq:N`/`@last`/`mark:X`로 바꾼다.
   - 출력 상한을 "하드 1,000행/64 KiB, 도메인 기본값 이하"로 정리한다.
   - RPC 이름은 snake 2단·3단 허용으로 완화한다(§6.2 주석, §7.1).
6. 뷰어(§5.x e, §6.6):
   - M1은 도메인 TUI(Textual), 접두키 `Ctrl-]`, `Ctrl-] x` STOP으로 한다.
   - PSU 비상 OFF 키를 STOP과 구분해 정의한다((a)-5).
   - heartbeat·`LIVE/LAG/STALLED` 배지와 `no_viewer_attached` 경고를 반영한다. 결정 D14 "뷰어 실행 제안 → 실패"는 `E_NO_APPROVER`(승인 필요 시)와 경고(그 외)로 바꾼다.
7. 시나리오(§7.1): `finally` = R0–R1 + `safe_state:true`, abort 시에도 실행(D12)으로 바꾼다. Registry에 `safe_state` 선언 대상(psu 출력 OFF, io 지정 채널, scope AWG OFF, la 패턴 정지)과 인터록 통과 조건((a)-1)을 명시한다. CORE/EXPLORE 분담은 (b)-3에 따른다.
8. 기록·경로(§5.7, §7.3): `~/.loom/{run,records,config}`, 발견 파일 `daemon.json`, 보존 30일·20 GiB, `--no-record`여도 감사는 기록.
9. 스킬(§6.7, D13 반영): `skills/` 원본 → `loom skills sync`로 **복사**(기본). symlink 서술을 제거하고, 공통 `loom` 스킬 규칙(others_since_last 대응)을 참조한다.
10. 일정(§8, §8.1, §8.2): P0–P4 표기로 바꾼다.
    - P0: X0 + DLT M0 gate + SimPsu
    - P1: PSU M1
    - P2: DLT M1 + 스코프(TUI) + IO
    - P3: 교차 러너(분담 반영) + SSH
    - P4: LA + Debug
    - 스코프 웹 파형은 P3 이후 M2로 둔다.
11. 결정 기록 §12의 D7·D14·D15·D16·D19를 마스터 D6·D9·D12 기준으로 갱신하고, "메인 결정 대기" 문구를 지운다(데몬 배치·종료 코드·R4 의미·스킬 경로·자동 기동은 결정됨).
