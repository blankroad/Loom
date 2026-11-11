# CANoe 진단 COM 공개 자료 사실 확인

2026-11-10. **확인**은 공개 문서/소스 확인이며 실기 합격이 아니다. 설치·CANoe/래퍼 실행은 하지 않았고 plan.md는 수정하지 않았다. py_canoe는 열람 당시 auto-py-utils/py-canoe `main` 기준이다.
Vector COM Technical Reference 전체 본문은 확보하지 못했다. Vector 공개 AN-IND-1-001 v2.0(2026-02-24)·AN-IND-1-026 v1.4(2024-10-07)를 우선 사용했고, 지원 문서의 검색 공개 본문만 확인한 경우 별도 표시했다.

## 1. 임의 요청 바이트와 응답 객체

- **확인 — 경로:** py_canoe는 `Application.Networks.Item(i)`를 1부터 순회하고 각 network의 devices를 순회해 device의 `Diagnostic` COM 객체를 수집한다(ECU qualifier/device name으로 선택). 통상 객체 경로는 `Application.Networks.Item(i).Devices.Item(j).Diagnostic`; 설정된 진단 대상이 전제이며 DB 없는 장치 자동 발견을 뜻하지 않는다. [networks.py L43–76](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/networks.py#L43)
- **확인 — 바이트 송신:** 해당 Diagnostic의 `CreateRequestFromStream(bytearray)` → 반환 request의 `Send()`. 이름 기반 `CreateRequest(primitive_path)`와 별도 경로여서 Loom이 승인한 UDS 바이트를 넘길 수 있다. [Diagnostic 래퍼](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic.py#L21), [Request 래퍼](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic_request.py#L28)
- **확인 — 완료/수신:** `request.Pending`, `request.Responses.Count`, `Responses.Item(i)`; 응답은 `Stream` 바이트, `Positive`, `ResponseCode`, `Sender`. 래퍼의 pending 응답 판정은 `not Positive`이고 ResponseCode가 0x78인 경우다. [Response 래퍼](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic_response.py), [응답 컬렉션](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic_responses.py)
- **확인 — 대기 방식:** Send 뒤 완료를 별도 조회하며, Python `send_diag_request`는 Pending 해제와 응답 존재를 10 ms 기본 간격으로 폴링하는 동기 헬퍼다. 상위 CANoe 래퍼의 timeout 기본은 30 s다. Request 이벤트 클래스는 있지만 해당 래퍼의 WithEvents 연결은 주석 처리돼 있다. [폴링 구현](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/networks.py#L79), [상위 timeout](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/canoe.py#L1325), [이벤트 연결](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic_request.py#L9)

## 2. 0x78·P2/P2*·타임스탬프

- **확인 — 타이머 구분:** Vector는 호출자의 응답 대기 제한과 진단층의 P2/P2 extended timeout을 구분한다. py_canoe의 외부 timeout/poll_s는 총 대기 제한이며 P2/P2* 재설정 구현이 아니다. [Vector SN-IND-1-040 §12 검색 공개 본문](https://support.vector.com/sys_attachment.do?sys_id=4e25b90f1b386d90afd085d2604bcbc7&sysparm_viewer_id=42497ed987fce910cd36fd99cebb35a7&sysparm_viewer_table=kb_knowledge), [대기 코드](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/networks.py#L79)
- **추정 — 책임 배치:** 기본 진단 채널에서는 CANoe의 진단 기술/진단층 설정이 프로토콜 타이밍·0x78 대기를 맡고 Loom은 별도 총 deadline을 둔다. COM으로 P2/P2*를 직접 읽고 바꾸는 정확한 API, 모든 0x78을 실시간 콜백으로 받는 보장은 **미확인**이다. [Vector 기본 진단 채널 설명 §2.4.2](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-026_DoIP_in_CANoe.pdf#page=5)
- **확인 — 헬퍼 주의:** 외부 timeout 때에도 응답이 있으면 아직 Pending인 상태의 마지막 0x78을 반환할 수 있다. 반환값 존재를 최종 성공으로 취급하면 안 된다. [timeout 이후 처리 L84–109](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/networks.py#L84)
- **미확인 — COM 응답 시각:** Response의 Time/TimeStamp 같은 속성은 확인 못 했다(래퍼에 없음이 COM 불가의 증거는 아님). Vector Trace의 계층별 타임스탬프는 확인됐지만 COM에서 얻는 경로는 별개다. 호스트에서 폴링 수신 시각을 찍는 방안은 **추정**이며 wire 시각/P2 실측으로 표시하면 안 된다. Vector도 COM의 타이밍 부정확성을 명시한다. [Trace 시각](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-026_DoIP_in_CANoe.pdf#page=5), [COM 제약 §2.6](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-001_CANoe_CANalyzer_as_Diagnostic_Tools.pdf#page=18)

## 3. 설정된 seed-key DLL

- **확인:** DLL은 진단 기술별 Diagnostic Layer → Security Access → Seed & Key DLL에 설정한다. CAPL `DiagGenerateKeyFromSeed` 또는 비동기 `DiagStartGenerateKeyFromSeed` + `_Diag_GenerateKeyResult`가 그 DLL을 사용한다. [Vector 설정 §2.8](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-001_CANoe_CANalyzer_as_Diagnostic_Tools.pdf#page=18), [SN-IND-1-040 §11 검색 공개 본문](https://support.vector.com/sys_attachment.do?sys_id=4e25b90f1b386d90afd085d2604bcbc7&sysparm_viewer_id=42497ed987fce910cd36fd99cebb35a7&sysparm_viewer_table=kb_knowledge)
- **확인 / 추정:** COM의 `Application.CAPL.GetFunction(name)`로 사용자 CAPL 함수에 접근하고 래퍼가 call하는 경로는 확인됐다. 따라서 사용자 CAPL 래퍼 안에서 위 키 계산 함수를 호출하는 조합은 가능성이 있는 **추정**이다(인자·배열 전달·키 회수는 실기 미검증). 내장 CAPL 함수를 이름만으로 직접 COM 호출하거나 COM 전용 Unlock/SecurityAccess 함수로 해제하는 경로는 **미확인**이다. [측정 초기화 L32–35](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/measurement.py#L32), [사용자 함수 전용·호출 구현](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/capl.py)
- **추정 — 승인 경계:** CAPL 래퍼는 키 계산만 수행하고, 완성된 0x27 key 요청 바이트는 Loom 분류·승인을 거쳐 다시 COM raw 송신해야 B3의 바이트 기준 관문이 유지된다. 자동 unlock 전체를 CAPL에 위임하면 이 확인이 별도로 필요하다.

## 4. 자동 TesterPresent

- **확인:** 동일 Diagnostic 객체에 `DiagStartTesterPresent()`, `DiagStopTesterPresent()`, 읽기 속성 `TesterPresentStatus`가 있다. py_canoe의 control_tester_present는 앞의 두 메서드를 호출한다. 주기·조건의 COM 변경 API와 설정된 TP 프레임 전부를 Loom으로 받는 경로는 **미확인**이다. [Diagnostic 원소스](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic.py#L9), [제어 래퍼](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/networks.py#L120)
- **추정:** canoe 백엔드에서는 CANoe 자동 TP와 Loom TP를 동시에 돌리지 않도록 소유권을 정해야 한다. 상태 조회 가능성과 실제 TP 원문 이벤트 수집 가능성은 구별한다.

## 5. DID·루틴 이름 열거 / CDD XML

- **미확인:** 공개 COM/py_canoe 자료에서 DID·RID 이름 전체를 열거하는 컬렉션 API는 찾지 못했다. 알려진 qualifier로 CreateRequest가 가능한 사실은 전체 목록 열거 가능성을 입증하지 않는다. [Diagnostic 공개 래퍼](https://github.com/auto-py-utils/py-canoe/blob/main/src/py_canoe/core/child_elements/diagnostic.py)
- **확인:** 공개 CDD 샘플은 XML 선언·CANDELA 루트·candela.dtd 참조를 가지며 cantools는 ElementTree로 읽고 DIAGCLASS/DIAGINST의 STATICVALUE·QUAL에서 식별자·이름을 추출한다. 파일 직접 읽기의 근거는 있으나 이 파서가 모든 루틴/변형·모든 CDD 버전을 지원한다는 보장은 **미확인**이다. [CDD 실물](https://github.com/cantools/cantools/blob/master/tests/files/cdd/example.cdd), [파서 L194–243](https://github.com/cantools/cantools/blob/master/src/cantools/database/diagnostics/formats/cdd.py#L194)
- **확인:** CANdelaStudio의 ODX/CSV/XML 등 export도 공개 기능이다. CDD 직접 해석과 CANoe가 현재 활성화한 진단 variant의 이름 목록 일치는 별도 검증 대상이다. [Vector CANdelaStudio](https://www.vector.com/en/product/candelastudio/)

## 6. DoIP 인터페이스와 다른 테스터의 라우팅 활성화

- **확인:** CANoe는 OS TCP/IP stack/Windows NIC와 자체 shared·노드별 TCP/IP stack을 구별한다. VN56xx 경로는 정밀 시각·다른 Vector HW와 동기화를 제공하고, OS 경로는 방화벽·NetIsolator 영향을 받는다. VN 하드웨어가 있다는 사실만으로 Windows 소켓에서 같은 인터페이스를 쓸 수 있다고 단정할 수 없다. [Vector AN-IND-1-026 §2.3, p3–4](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-026_DoIP_in_CANoe.pdf#page=3)
- **확인:** CANoe의 진단 채널은 adapter/IP·tester SA·gateway/ECU LA 등에 묶인다. 여러 채널이 같은 TCP 연결을 공유할 수 있고 단일 TCP 연결만 허용하는 ECU도 있다. 실제 장비의 다중 테스터 지원은 **미확인**이다. [같은 문서 §3.2, p14–17](https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-026_DoIP_in_CANoe.pdf#page=14)
- **확인 — 동일 SA/다른 TCP:** 같은 DoIPInterface의 기존 연결에 SA가 등록돼 있으면 기존 연결에 alive check; 기한 안에 응답하면 새 활성화는 0x03으로 거부·새 소켓 종료. 응답이 없으면 후속 연결 수/활성화 검사를 진행한다(SWS_DoIP_00105/00107). [AUTOSAR R24-11 p41](https://www.autosar.org/fileadmin/standards/R24-11/CP/AUTOSAR_CP_SWS_DiagnosticOverIP.pdf#page=41)
- **확인 — 0x02 구별:** 이미 다른 SA로 등록된 **그 TCP 소켓**에서 SA를 바꿔 활성화를 요청하면 0x02·소켓 종료(00106). 별도 소켓의 다른 SA가 무조건 0x02인 것은 아니다. 다른 허용 SA의 별도 연결은 자리가 있으면 후속 인증/활성화 검사로 진행하며, 자리가 없고 기존 tester가 모두 alive면 0x01(00107); 미등록 SA는 0x00(00104). [AUTOSAR p40–42](https://www.autosar.org/fileadmin/standards/R24-11/CP/AUTOSAR_CP_SWS_DiagnosticOverIP.pdf#page=40)
- **추정:** CANoe와 Loom 자체 DoIP가 다른 SA로 둘 다 활성화돼도 ECU 세션·보안·쓰기 상태가 독립한다는 보장은 없다. 해당 ECU 프로필의 동시성은 **미확인**이므로 COM 백엔드 사용 중 자체 DoIP의 같은 ECU 접근을 독립 세션이라고 간주하지 않는다.
