# UDS · DoIP · EthDiag 벤치마크 (UDS 담당)

상태: **READY_FOR_REVIEW — FROZEN** (TP=TesterPresent, 함께 열린 패널의 현재 송신 기능 정정 반영) · 기준 `dbca34d` · 출처 열람일 2026-11-12 · 문서만 작성(소스 변경 없음)
상위 통합본: [서비스별 요약과 구현 결정](service-tool-benchmarks.md). 이 문서는 UDS 전용 상세 검토다.
표기: **[열람]** 본문을 직접 확인한 출처 / **[검색]** 검색 요약으로만 확인한 출처 / **[사용자]** 사용자가 제공한 맥락.

## 1. 계층·전송·대상 식별 (검증)
| 층 | 항목 | 성격 | Loom 서비스 위치 |
|---|---|---|---|
| 응용 진단 | UDS (ISO 14229-1) | 세션, 보안, 서비스, DTC | **진단 그룹** |
| 전송 | CAN-TP (ISO 15765-2) | CAN 위의 분할·흐름 제어 | 독립 서비스 아님 → UDS 대상의 전송 속성, CAN Trace의 보조 보기 |
| 전송 | DoIP (ISO 13400) | Ethernet/IP 위의 진단 전송 | 독립 서비스 아님 → UDS 대상의 전송 속성, ETH 패킷의 프로토콜 |
| 전송 | EthDiag | [사용자] 현대·기아 Ethernet 진단, DoIP와 비슷하지만 별개 | 독립 서비스 아님. **사양 미확보** |
| 물리 | CAN / Ethernet | 인터페이스 | **인터페이스 그룹** (CAN, Ethernet 서비스) |

**대상(target) 식별, Loom 코드 기준**
- `ConnectSpec`(`crates/loom-uds/src/manager.rs:577-586`)은 `name`, `sim`, `host`, `port`(기본 DoIP 포트), `la`(ECU 논리 주소), `tester_la`, `activation`, `auto_reconnect`, `timeout`으로 이루어진다.
- 대상의 전송은 `set_endpoint(backend, addr)`로 기록된다(`session.rs:172`, 예: `("doip", "DoIP 0x1001")`).
- **이 식별에는 CAN 채널이나 ETH 캡처 인터페이스를 가리키는 필드가 없다.** 따라서 UI가 "이 ECU는 저 CAN 채널이나 ETH 인터페이스를 통해 연결되어 있다"고 표시할 근거가 코드에 없다. 그런 연결은 표시하지 않는다.

**현재 지원 범위**
- **UDS manager**: `sim`(내장 sim DoIP entity)과 `doip`(`host:port`, Env::Bench)만 연결한다(`manager.rs:691-700`). capability도 `doip`, `sim`이다(`module.rs:527-528`).
- **`cantp`, `canoe`**: `payload.rs:92`의 transport 값 목록에만 있다. **실제 backend가 있다는 근거가 아니다.**
- **CAN ISO-TP**: CAN 도메인에 실제로 있다(`crates/loom-can-isotp`, `loom can isotp` = "ISO-TP endpoints for transport debugging (UDS diagnostics: use `loom uds`)", `crates/loom-can/src/cli.rs:291-300`). 다만 **UDS manager와 연결되어 있지 않다.** "CAN ISO-TP가 있다"는 것과 "UDS-over-CAN-TP가 된다"는 것은 다른 말이다.
- **EthDiag**: 미지원이며 지원을 주장하지 않는다.
- **"TP" 표시 혼동 주의**: 현재 `uds.console` 대상 막대의 "TP"는 **TesterPresent**(payload `tester_present`, on/ok_count, `payload.rs:75,99`)이며 **CAN-TP 전송 상태가 아니다.** 전송 계층 상태를 표시하는 곳은 현재 없다. 전송 이름(doip/sim)은 endpoint 표시로만 나온다.

## 2. 참조 도구 (근거 범주: V 공급사 주장, B API·동봉, O OSS)
- **Softing DTS.monaco (V)** — "used in numerous engineering departments all over the world". ODX 2.0.1/2.2, OTX(ISO 13209), 편집 가능한 **워크스페이스와 레이아웃**, DTC 컨트롤(결함 기억 읽기·지우기), 응용 계층을 심볼로 보는 **Symbolic Trace**, **Ethernet(DoIP) 통신 표시와 기록**, ECU 플래시. [열람] <https://automotive.softing.com/products/softing-dts/softing-dtsmonaco.html>
- **Vector CANoe (V)** — "highly valued by users worldwide", ECU 진단의 시험과 시뮬레이션, DoIP 진단 영상. 진단 창 구성의 세부는 이 페이지에 없다. [열람] <https://www.vector.com/int/en/products/products-a-z/software/canoe/>. **CANalyzer**는 버스 분석 도구에 진단(결함 기억, variant coding, OBD-II)을 함께 둔다. [열람] <https://www.vector.com/int/en/products/products-a-z/software/canalyzer/>
- **PEAK PCAN-UDS API (B, GUI 아님)** — ISO 14229-1 DLL. 전송은 PCAN-ISO-TP(ISO 15765-2:2016), 하드웨어 접근은 PCAN-Basic 위에 있다. **화면 벤치마크 대상이 아니며**, UDS / 전송 / 하드웨어 계층 분리의 근거로만 쓴다. [열람] <https://www.peak-system.com/products/software/development-packages/pcan-uds-api/>
- **DoIP 관찰 근거** — Wireshark 표시 필터 `doip`, `doip.type`, `doip.source_address`. [열람] <https://www.wireshark.org/docs/dfref/d/doip.html>
- 순위는 없다. 공개된 비교 사용량 자료를 찾지 못했다.

**EthDiag 조사**
- 정확한 용어 "EthDiag"로 검색했을 때 OEM 사양이나 정의 문서가 **없었다**.
- 검색에 나온 것은 GDS Smart / KDS 2.0의 "Ethernet이 필요한 ECU 업그레이드" 장애 대응 문서(장비 공급사 GIT 지원 페이지)뿐이며, 용어도 사양도 담고 있지 않다. [검색] <https://support.gitauto.com/hc/en-gb/articles/54857177298201-HYUNDAI-GDS-Smart-How-to-Resolve-ECU-Upgrade-Failures-Requiring-Ethernet>
- 결론: [사용자] 맥락으로만 기록한다. DoIP의 별칭이나 변형으로 취급하지 않는다. 포트, 헤더, 활성화 절차를 추정하지 않는다. 디코더, 필터, 선택지도 만들지 않는다.

## 3. 참조 도구에서 가져올 것 / 따라 하지 말 것
- **가져올 것**: 대상별 세션·보안·전송 상태 막대, 트랜잭션 행에 원시 바이트와 심볼(서비스·NRC 이름)을 병기하는 것(Symbolic Trace의 축소판, ISO 14229 표준 표만 사용), DTC 읽기(0x19)와 지우기(0x14)의 화면 분리, 진단 화면 옆에 원시 통신 관찰을 두는 것(DTS.monaco의 DoIP 표시, CANalyzer의 버스+진단).
- **따라 하지 말 것**: ODX/OTX 데이터베이스와 시퀀스 편집기, 플래시 작업 흐름, 사용자가 자유롭게 편집하는 워크스페이스(Loom 도킹 레이아웃으로 충분), 탐색용 UDS 스캐너(SavvyCAN UDS Scan 같은 대량 쓰기).
- **주의**: 상용 도구는 ECU와 전송 경로를 데이터베이스로 **명시적으로 설정**한다. 화면에 함께 있다는 이유로 연결하지 않는다. Loom도 manager가 연결한 정보만 표시해야 한다.

## 4. 진단 그룹 작업 흐름 레이아웃 후보
공통 전제:
- 서비스 이동은 연결·해제·캡처 시작·송신을 일으키지 않는다.
- CAN/ETH 패널은 **실제로 열린** 채널만 보여 준다. 열린 것이 없으면 해당 서비스로 이동하는 링크만 둔다.
- **현재 배포 상태**: 함께 열리는 CAN/ETH 패널은 기존 송신 기능(CAN 송신 줄, ETH 캡처 제어와 `eth.tx`)을 ActionGate와 권한 통제 아래 그대로 가진다. **읽기 전용으로 강제되지 않는다.** 아래 레이아웃의 "관찰 전용"은 **제안**이다.
- 쓰기는 기존 ActionGate, dry-run, 위험 배지, STOP을 그대로 따른다.

**A. 콘솔 중심 + 관찰 보조 (권장)**
```
┌ 대상 막대: ECU별 1줄 — name · 전송(DoIP|sim) · addr · session · security · TesterPresent · P2 · 소유자 ┐
├ 트랜잭션 표 (raw + 서비스/NRC 이름, pending×N) ─────────────┬ DTC / DID 최근값 ┤
├ 명령 줄 (hex → dry-run 미리보기 → Enter) ─────────────────────┴────────────────┤
└ 하단 탭: [ETH(열림 시)] [CAN(열림 시)] — 제안: 관찰 전용 모드 + "ECU와 연결 미확정" 표시 ┘
```
- 장점: 현재 `uds.console`을 그대로 쓰므로 변경이 가장 작다. 관찰 패널을 진단과 섞지 않는다.
- 단점: 원시 프레임과 진단 행을 연결해 보려면 탭을 오가야 한다.

**B. 상하 분할 병렬 보기 (DTS.monaco/CANalyzer형)**
- UDS 콘솔이 위, ETH 또는 CAN Trace가 아래에 동시에 보인다. 시간 커서를 공유한다.
- 장점: 진단 행과 프레임을 시간으로 대조할 수 있다.
- 단점: 시간이 겹친다는 이유로 "같은 흐름"이라는 오해를 부르기 쉽다. 화면 밀도가 높다. 연결 표시는 주소·request_id가 일치할 때(ETH ↔UDS 열)만 허용해야 한다.

**C. 대상 카드 그리드**
- ECU마다 카드 하나(상태, 최근 트랜잭션, DTC 수)를 두고, 클릭하면 상세 콘솔로 들어간다.
- 장점: 대상이 여러 개일 때 한눈에 본다.
- 단점: 현재 대상 수와 사용 패턴에 비해 과하다. 새 컴포넌트가 필요하다.

**권장: A.** 이유:
1. 기존 콘솔과 수용 시험을 유지한다.
2. 관찰 패널이 ECU와 연결되었다고 오해할 여지가 가장 적다.
3. 현재 실제 전송이 DoIP/sim 하나뿐이라, 병렬 대조(B)의 이득이 제한적이다.

B는 ↔UDS 연결 검증이 끝난 뒤 선택 보기로 둔다. C는 대상이 많아진 뒤에 다시 판단한다.

## 5. 다음 단계 (제안, 구현 아님)
1. **진단 그룹 레이아웃 A의 수용 조건 정리**: 열린 도메인만 표시, 자동 캡처 없음, "연결 미확정" 문구, 양방향 이동 시 RPC 부작용 없음. 함께 열린 CAN/ETH 패널의 송신 기능을 진단 그룹 안에서 숨길지(관찰 전용 모드) 결정한다. 현재는 숨기지 않는다.
2. **콘솔 의미 표시**: 서비스·NRC 이름, DTC 읽기/지우기 분리(지우기는 R3 확인), 전송 열은 실제 값(doip/sim)만.
3. **UDS-over-CAN-TP (별도 설계)**: 기존 CAN ISO-TP를 manager backend로 연결할지 CAN·UDS 소유자가 합의한다. 그 전에는 UI에 `cantp` 선택지를 두지 않는다.
4. **EthDiag**: OEM 사양 문서를 확보한 뒤 별도 backend로 검토한다. 그 전에는 표시하지 않는다.

## 6. 한계
- 상용 도구의 실제 화면은 확인하지 않았다(공개 제품 페이지만 근거로 썼다).
- 코드 판단은 작업 트리를 읽은 결과다. 실행 검증은 하지 않았다. `dbca34d` 실검증 통과는 main의 검증 결과를 옮긴 것이다.
