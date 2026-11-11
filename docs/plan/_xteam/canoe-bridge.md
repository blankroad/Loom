# CANoe 브리지 ↔ UDS `canoe` 백엔드 — 합의 문서 (초안 v0.1, can-lead 작성, 2026-11-10)

근거: 마스터 §2.2 **B3**(브리지 P2 승격, 소유: 브리지 코어 = CAN, `canoe` 백엔드 = UDS), `03-v1.2-tasks.md`, CAN `plan.md` §8.2(브리지 설계)·D48·D52.
COM 이름은 `py-canoe` 26.4.1 wheel 소스(2026-11-10)에서 확인했다. **설치된 CANoe와 대조는 아직 안 함**(사용자 CANoe 버전 미확인 — B7).

## 1. 경계
| 항목 | CAN 팀 (브리지 코어 `loom/canoe/`) | UDS 팀 (`canoe` 백엔드) |
|---|---|---|
| 프로세스 | `canoe-bridge` worker(Windows), COM 전용 STA 스레드 하나 + 호출 큐, gateway가 감독 | gateway 안 UDS 세션 관리자에서 브리지 클라이언트 프록시 호출 |
| 책임 | 부착·측정 상태·재연결, COM 객체 무효화 처리, 호출 직렬화·timeout, STOP latch, 브리지 상태 이벤트 | SID 위험 분류·승인·감사(Loom 파이프라인), 세션·보안접근 흐름, 트랜잭션 이벤트 발행, 기본 경로 선택 |
| 이벤트 | `canoe.state`(detached/attaching/attached, 측정 running/stopped, 버전, cfg), `canoe.value`(신호·sysvar) | `uds.*` 트랜잭션 이벤트(`backend="canoe"` 필드) — **브리지는 uds 이벤트를 내지 않는다**(중복 방지) |
| 사용자 화면 | 상단 `CANoe <버전> MEAS RUNNING` / `CANoe DETACHED` 배지, Activity | UDS 트랜잭션 패널에 요청·응답 점진 갱신. 버스가 관찰되면 CAN Trace에 `EXT canoe` 프레임(시간 창으로 느슨히 연결, 추정 표시) |

## 2. 브리지 내부 API 제안 (UDS가 호출, 공개 CLI 없음)
```python
class CanoeBridgeClient:                       # gateway 쪽 프록시, 모두 async, 브리지 worker로 IPC
    async def status(self) -> BridgeStatus     # {state, canoe_version, config_path, measurement, since_ns}
    async def diag_list_ecus(self) -> list[EcuInfo]   # {qualifier, network, bus_kind: "can"|"ethernet"|"unknown"}
    async def diag_request(self, ecu: str, pdu: bytes, *, request_id: str, decision_id: str,
                           timeout_s: float, origin: str) -> DiagResult
    async def diag_tester_present(self, ecu: str, on: bool, *, decision_id: str, origin: str) -> None
    def subscribe(self, cb) -> Unsubscribe      # canoe.state, diag_progress(0x78 중간 응답)
# DiagResult = {request_id, ecu, state: "sent"|"confirmed"|"outcome_unknown"|"failed",
#               t_submit_ns, t_sent_ns|None, responses: [{sender, data: bytes, t_rx_ns, response_pending: bool}],
#               final: "positive"|"negative"|"none", error: {code, reason}|None, ts_quality: "com_event"|"polled"}
```
- COM 경로: `Networks…Devices(ecu).Diagnostic.CreateRequestFromStream(pdu)` → `Send()` → `OnConfirmation`(→ `confirmed`)·`OnResponse`·`OnTimeout`·`OnCompletion` 이벤트, 없으면 `Pending`/`Responses` 폴링(10 ms). 응답마다 `Sender`·`Stream`, 0x78은 `response_pending=true`로 따로 표시.
- **`decision_id` 필수**: Loom 파이프라인(UDS 정책)의 판정 id가 없는 호출은 브리지가 거절한다(exit 4 `E_DENIED`, `detail.reason=no_policy_decision`). 브리지를 우회한 진단 송신 경로를 만들지 않기 위함.
- 직렬화: COM 호출은 한 스레드에서 순서대로. **ECU별 진행 중 요청 1개**(같은 ECU 두 번째는 exit 6 `E_LOCKED`), 다른 ECU 요청은 겹칠 수 있다.
- 시각: `t_sent_ns`/`t_rx_ns`는 **브리지가 COM 이벤트·폴링에서 관측한 host 시각**이다. 버스 시각이 아니다(`ts_quality`). P2/P2* 타이밍은 CANoe 진단 설정이 처리하고, `timeout_s`는 Loom 쪽 바깥 상한.
- 실패: COM timeout·CANoe 재시작·측정 정지 중 진행 요청 → `outcome_unknown`(자동 재시도 금지). COM 바쁨(`RPC_E_CALL_REJECTED`) 상한 초과 → exit 7 `E_NO_RESPONSE`(`detail.reason=canoe_busy`). 측정이 멈춰 있으면 → exit 7 `E_NOT_OPEN`(`detail.reason=measurement_stopped`).

## 3. STOP · 잡 · 상태
- STOP(A3): 브리지는 agent·script origin의 **새** 진단 요청을 latch한다. 사용자 origin 요청은 허용. 진행 중 요청은 COM에서 취소할 수 없으므로 끝까지 관측하고, timeout이면 `outcome_unknown`.
- Loom이 켠 tester present(`diag_tester_present on`)는 **잡**(A1)으로 본다 → STOP이면 브리지가 `DiagStopTesterPresent`를 호출한다. 이 때문에 ECU 세션이 기본 세션으로 돌아갈 수 있음을 UDS 화면에 표시해 달라(UDS 판단 요청 Q3).
- **CANoe 측정 자체는 STOP으로 멈추지 않는다**(Loom 소유 아님). 뷰어에 "CANoe 측정 정지" 키를 따로 둔다(CAN 담당).
- 쓰기 상태(A14): `requested`·`accepted`는 UDS 파이프라인, `sent` = 브리지가 `Send()` 반환, `confirmed` = `OnConfirmation` 수신, 응답 수신 후 판정은 UDS.

## 4. 경로 선택과 충돌 (UDS 결정 사항, CAN 의견)
- 기본 경로(B3/B7): CANoe 측정이 켜져 있으면 `canoe`, 아니면 자체 스택. Linux는 항상 자체 스택. 브리지는 `status().measurement`와 `canoe.state` 이벤트로 판단 근거를 준다.
- **같은 ECU에 두 테스터 금지(CAN 의견):** UDS 세션이 `canoe` 경로를 쓰는 동안 같은 ECU 주소로 Loom 자체 CAN-TP 채널(`can.isotp.open`)을 열지 않는다. 세션 도중 백엔드 전환 금지(세션 종료 후 전환). CAN은 `can.isotp.open`에서 같은 tx_id를 `canoe` 세션이 쓰고 있으면 exit 6 `E_LOCKED`(`detail.reason=canoe_session`)로 막는다.
- 측정 정지·CANoe 종료 시 `canoe` 세션은 끊긴 것으로 처리(자동으로 자체 스택에 넘기지 않음 — 사용자에게 보이고 다시 시작).

## 5. 미확인 · 확인 계획
| # | 항목 | 확인 방법 |
|---|---|---|
| U1 | 사용자 CANoe 버전에서 위 COM 이름·이벤트가 그대로인지 | 브리지 v0 첫 부착 시 `Version` 기록 + 수동 체크리스트(CAN `plan.md` §8.3) |
| U2 | `diag_list_ecus`에 쓸 COM 열거 경로(Networks→Devices)와 ECU별 버스 종류(CAN/이더넷) 판별 | v0 실기 |
| U3 | **seed-key**: CANoe에 설정된 seed-key DLL을 COM으로 쓰는 방법. 후보 ① 사용자 설정의 CAPL 함수(`CAPL.GetFunction`은 측정 OnInit 때만 획득 가능 — py-canoe 구현), ② UDS가 같은 DLL을 직접 로드, ③ COM 진단 객체의 보안접근 지원 여부 | UDS 판단 + 실기 |
| U4 | CANoe 진단 DB의 DID·루틴 이름(qualifier)을 Loom 표시용으로 가져올 수 있는지 (`CreateRequest(qualifier)`는 이름으로 요청 생성만 확인) | UDS 검토 |
| U5 | COM 호출 지연 p50/p99(요청 1건 왕복에 브리지가 더하는 시간) | v0 실측, UDS 목표(100 ms 화면 갱신)와 비교 |
| U6 | CANoe가 Vector 이더넷 인터페이스를 쓸 때 자체 DoIP(PC NIC)와 충돌 | **UDS 리스크로 기록 요청**(B3) |

## 6. UDS에 묻는 것
- Q1. §2 API 형태(특히 `decision_id` 필수, ECU별 진행 1개, `DiagResult` 필드)로 `canoe` 백엔드를 만들 수 있나? 빠진 필드가 있나?
- Q2. 0x78 중간 응답을 `diag_progress` 이벤트로 받으면 충분한가, 아니면 `DiagResult.responses` 누적만으로 충분한가?
- Q3. STOP 시 tester present 정지(§3)를 기본으로 해도 되나?
- Q4. seed-key 경로(U3) 선호안.
- Q5. 기능 주소(functional) 요청을 `canoe` 백엔드에서도 지원해야 하나(v0 범위 여부)?

## uds 의견
(uds-lead, 2026-11-10) 기준: UDS `plan.md` v1.2 §3.7(`canoe` 백엔드)·§15.3a·M2c·U-27/U-28·리스크 16–18. 근거 사실: `docs/plan/uds/v12-canoe-facts.md`(공개 자료, 실기 미검증). §1 경계·§4 경로 선택·`can.isotp.open`의 `canoe_session` 잠금에 **동의**한다. UDS도 같은 ECU를 `canoe`와 자체 DoIP로 동시에 열지 않는다(exit 6 `E_LOCKED`, `target_on_other_backend`). U6(Vector 이더넷 ↔ 자체 DoIP 충돌)은 UDS 리스크 16으로 기록했다.

**정정 요청 1건 — 쓰기 상태 이름(§2 `DiagResult.state`, §3 마지막 줄)**
- `confirmed` = `OnConfirmation`은 can-uds-isotp "메인 확정"(마스터 A14)과 충돌한다. UDS 요청의 `write_state`는 UDS 행이 소유하고, **`confirmed` = ECU 긍정 응답**이다. 송신 확인을 `confirmed`로 보이면 ECU가 응답하기 전에 쓰기가 확인된 것처럼 보인다.
- 제안: `DiagResult.state`를 전송 계층 필드 `tx_state: submitted|sent|bus_confirmed|failed|unknown`으로 바꾼다(`sent` = `Send()` 반환, `bus_confirmed` = `OnConfirmation`). `write_state`(confirmed·failed·outcome_unknown)는 UDS가 `responses`·`final`·timeout으로 정한다. ISO-TP의 `tx_state` 용법과 같다.

**Q1 — API 형태**: 이 형태로 만들 수 있다. `decision_id` 필수(core 파이프라인 판정 id, `presend_check` 뒤 호출), ECU별 진행 중 1개(UDS 대상당 인플라이트 1과 같음)에 동의한다. 추가 요청:
1. `EcuInfo`에 주소 정보를 가능한 만큼: CAN이면 tx/rx id, DoIP면 테스터 SA·대상 LA·게이트웨이 IP. UDS가 "같은 물리 ECU" 중복을 판정하는 데 쓴다. 모르면 `null`로 두고, 그러면 UDS는 qualifier가 같은지만 본다.
2. `BridgeStatus`와 `canoe.state`에 **세대 번호**(`generation`: 부착·측정 시작마다 증가). UDS는 세대가 바뀌면 epoch를 올리고 대상을 link down으로 둔다.
3. 응답 항목에 `positive`·`response_code`, 결과에 `t_confirm_ns`(OnConfirmation 시각, 없으면 null). `ts_quality: com_event|polled`는 UDS 행 `timing.ts_quality`에 그대로 쓴다(plan 반영).
4. `diag_cancel(request_id)`: COM에서 취소할 수 없어도 브리지 대기를 끝내고 ECU 잠금을 `outcome_unknown`으로 넘긴다. 늦은 응답이 오면 `diag_progress`로 계속 알려 달라(UDS 늦은 응답 격리 규칙에 필요).
5. `call_capl(name, args)`(Q4용)와 TesterPresent 상태 읽기(`TesterPresentStatus`).
6. `FakeCanoeBridge`: 같은 API를 CI 3 OS에서 쓸 수 있게 하고, 측정 정지·0x78·COM 예외·지연을 주입할 수 있게.
- `origin` 인자는 gateway 내부 프록시만 채운다(연결 role 기준). 브리지는 외부에서 온 origin을 받지 않는다.

**Q2 — 0x78**: 둘 다 필요하다. `diag_progress`(request_id, 0x78 수신, t_rx_ns)를 **실시간으로** 받아야 사용자 화면의 `pending ×N` 카운터가 바로 늘어난다(02 원칙). `DiagResult.responses` 누적은 최종 판정과 기록용이다. 0x78은 최종 결과가 아니며, 바깥 timeout에서 마지막 0x78을 성공으로 쓰지 않는다(UDS 러너 규칙).

**Q3 — STOP과 TesterPresent**: 기본으로 하는 데 동의한다. 정지 대상은 **Loom이 켠 TP만**이다. Loom 부착 전부터 CANoe 설정으로 돌던 TP는 건드리지 않고 `tp_owner: canoe`로 표시한다. UDS 화면에는 이미 "STOP — TP 정지, S3 뒤 기본 세션 복귀 가능" 배지와 `active_overrides` 복구 송신 1키가 있다(plan §10.7).

**Q4 — seed-key**: ① **사용자 CAPL 함수 우선**. 함수는 key만 계산해 돌려준다(`DiagGenerateKeyFromSeed`가 CANoe에 설정된 DLL을 씀). UDS가 `27 xx key` 바이트를 만들고 R3 뷰어 승인을 받은 뒤 `diag_request`로 보낸다(바이트 관문 유지, key는 기록에서 마스킹). 함수 이름은 `loom.toml [uds]` 대상 설정에 둔다. `GetFunction`이 측정 OnInit 때만 가능하면 브리지가 부착·측정 시작 때 미리 얻어 두기를 요청한다. ② DLL 직접 로드(UDS M4 DLL 브리지)는 대안으로 둔다. ③ unlock 전체를 CANoe에 맡기는 방식은 쓰지 않는다(Loom이 key 바이트를 볼 수 없음).

**Q5 — 기능 주소**: v0 범위 밖이다. `canoe` 대상의 기능 주소 요청은 UDS가 exit 4 `E_UNSUPPORTED`(`reason=functional_on_canoe`)로 거부한다. 필요하면 UDS M3b(기능 그룹)와 같은 시기에 다시 논의한다.

**U4 — 이름 가져오기**: COM 열거 API가 없으면 UDS가 CDD 파일(XML)을 읽기 전용으로 읽어 DID·루틴 이름을 **표시용**으로만 쓴다(1순위, 등급은 낮추지 않음). 브리지에는 `status().config_path`와, 가능하면 진단 기술 파일 경로만 요청한다.

**U5 — 지연**: UDS 화면 목표(응답→뷰어 p95 ≤ 100 ms)는 자체 스택 기준이다. `canoe` 경로는 브리지 실측(p50/p99) 뒤 별도 목표로 게시한다. 그때까지 화면에는 `ts_source=canoe`와 `ts_quality`를 함께 표시한다.

## 메인 확정 (2026-11-10)
- **UDS 정정 요청 채택.** 브리지 `DiagResult.state` → 전송 계층 필드 `tx_state: submitted|sent|bus_confirmed|failed|unknown`. 요청의 `write_state`(`confirmed` = ECU 긍정 응답, `failed`, `outcome_unknown`)는 UDS 행이 소유한다. `can-uds-isotp.md` 메인 확정(마스터 A14)과 같은 원칙.
- UDS 추가 요청 1–6(`EcuInfo` 주소, `generation`, `t_confirm_ns`·`ts_quality`, `diag_cancel` 후 늦은 응답 `diag_progress`, `call_capl`·TP 상태, `FakeCanoeBridge`)과 Q2–Q5 답은 **브리지 v0 범위로 채택**. 정확한 시그니처는 구현 단계(P2 K16)에서 CAN이 이 문서를 갱신하며 확정한다. 별도 회신 라운드 없음.

## v2.0 변경 (can-lead, 2026-11-10 — 마스터 C1·C4)
- 브리지는 **진단 필수 경로에서 내려 선택 기능(P4)**. 진단 기본 경로는 자체 스택(DoIP over PC NIC + 미디어 컨버터 — C4.1, CAN-TP over VN5640·PCAN FD). 위 합의(§1–§6, uds 의견, 메인 확정)는 **P4 구현 때 그대로 쓴다.**
- 언어: C1은 "대안이 없을 때만 Python"인데, Rust로 COM이 불가능하다는 근거는 없다(`windows` 0.62.2 `IDispatch::GetIDsOfNames/Invoke`, `IConnectionPoint` 이벤트 싱크 — CAN `v2-sol.md` §9). 그래서 P4 첫 작업은 **설치된 CANoe에서 Rust late-binding 3일 스파이크**(신호 읽기·진단 이벤트 싱크·재시작 재부착). 재현 가능한 blocker가 나올 때만 Python 어댑터 worker 예외를 결정 기록에 남긴다(CAN plan D71). 어느 언어든 같은 worker 프로토콜·메모리 예산·`decision_id`·`generation`·늦은 응답 규칙.
- UDS 쪽 `FakeCanoeBridge`는 CI 3 OS 계약 시험용으로 유지 요청 그대로.
