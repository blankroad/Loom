# CAN R2 교차 리뷰 — can-sol → can-opus

대상: `r1-opus.md` 전체 1,083줄. 작성일: 2026-11-09.
기준: 최우선 핵심 원칙, 공통 브리프 §3·§4, CAN 브리프, 현재 `_xteam` 계약.
주의: CAN–UDS 계약은 지시문에 적힌 v0.1에서 **v0.2로 갱신**되어 §9·§10을 우선 적용했다.
자기 R1과 상대 R1은 수정하지 않았다. 추가 사실 확인·측정만 `spikes/can/`에 기록했다.

## 1. 요약 판정

**수정 후 채택 권고.** 핵심 루프와 M1 방향은 맞지만 타이밍·기록·승인 보장은 그대로 확정하면 안 된다.
강점 3개: §2.0의 구체적 양방향 시나리오, §3.11의 공유 가시성 불변식, §11·§12의 raw/TUI 우선 M1 분해.
약점 3개: 단일 프로세스/타이머 휠의 성능 근거 부족, BLF·HW 시각·TX 확인 의미 혼동, origin 기반 승인/송신 예외의 허점.
우선 수정: 측정 경계와 실패 상태, 원자적 backlog→live, UDS v0.2의 rx_start·예약·keepalive 소유권을 계약으로 고정한다.

## 2. 핵심 원칙 체크리스트 점검

| 항목 | 판정 | 근거와 보완 |
|---|---|---|
| 맨 앞 핵심 루프 4요소 표 | 통과 | §0이 명령·데몬·사용자 화면·마일스톤을 모두 연결한다. |
| 구체적 종단 간 시나리오 | 통과 | §2.0의 agent0x300→device0x301→사용자 :send→agent의 user 조회까지 완결된다. |
| 실시간 목표·고부하 정직 표시 | **미흡** | §3.12 목표/생략/LAG는 좋다. Fixed 항상 정확·MinMaxLTTB 스파이크 보존·recording intact의 무조건 표현은 수정해야 한다. |
| M1 walking skeleton | 통과 | §11 M1에 virtual·send/trace·live TUI·사용자 송신이 있고 DBC/웹/실HW를 뺐다. |
| 같은 세션·늦은 참가·숨은 연결 금지 | 통과, 계약 보강 | §3.11 I1–I6 충족. §3.5·I3에는 snapshot high-water와 live 연결의 원자성이 추가로 필요하다. |
| 부수 기능 후순위/비범위 | 통과 | §1 우선순위·§11 M5에 CANoe/ARXML/Replay/미려한 UI가 분리되어 있다. |

이는 계획의 충족도 판정이며 실제 구현·성능 시험 통과 판정이 아니다.
M1에 웹이나 DBC를 다시 넣을 이유는 없다. 핵심 루프 수용 시험을 먼저 유지한다.

## 3. 틀린 사실 및 미확인 항목 판정

### 3.1 상단 출처 표·§7의 재확인

2026-11-09에 PyPI JSON API 19개 패키지를 다시 조회했다.
정확한 metadata·wheel 목록은 `spikes/can/r2-package-evidence.json`에 보관했다.
아래 “확인”은 배포 metadata/소스 사실이며 Windows·실HW 정상 동작 인증은 아니다.

| 항목 | Opus 주장 | 판정 | 실제·근거 |
|---|---|---|---|
| python-can | 4.6.1,2025-08-12,LGPL-3.0-only,Py≥3.9 | 확인 | [해당 버전](https://pypi.org/project/python-can/4.6.1/). 개발 활동 미확인은 해소: R1 조사에서2026-11-08 commit 확인. |
| PCAN Darwin loader | PCBUSB를 로드 | 확인 | [basic.py:673–680](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/pcan/basic.py#L673). PCBUSB와 MacCAN을 완전히 별개의 python-can 경로로 쓰면 오해다. |
| Vector init 요청 | bitrate/fd/timing/listen_only면 요청 | 확인 | [canlib.py:234–255](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/vector/canlib.py#L234). 요청과 실제 부여는 다르다. |
| periodic override | SocketCAN/IXXAT만,vector/pcan은 thread | 확인,버전 한정 | 설치4.6.1 interfaces 전체 검색 결과 일치. [BusABC](https://github.com/hardbyte/python-can/blob/v4.6.1/can/bus.py), [IXXAT](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/ixxat/canlib.py). |
| virtual FD | protocol=CAN_FD 가능 | 확인,의미 제한 | [virtual.py:84](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/virtual.py#L84): protocol 값은 동작에 영향 없는 시험용 선언이다. FD 물리·DLC 검증 인증은 아니다. |
| CAN file IO | ASC/BLF/MF4/TRC 등 | 확인,범위 제한 | [File IO](https://python-can.readthedocs.io/en/stable/file_io.html). MF4는 optional, CAN bus log reader와 일반 측정 channel reader를 구분한다. |
| can-isotp | 2.0.7 MIT,NotifierBasedCanStack | 확인 | [metadata](https://pypi.org/project/can-isotp/2.0.7/), [v2.0.7 source](https://github.com/pylessard/python-can-isotp/blob/v2.0.7/isotp/protocol.py). python-can4.6.1과 virtual 왕복도 실제 성공했다. |
| ISO-TP 주소 모드 | Normal/NormalFixed/Extended/Mixed×11/29 | **표현 수정** | [address.py](https://github.com/pylessard/python-can-isotp/blob/v2.0.7/isotp/address.py): 실제 enum7개,NormalFixed는29bit만이다. 본문 §15의7개 열거는 맞다. |
| cantools | 45.1.2,2026-10-29,MIT | 확인 | [metadata](https://pypi.org/project/cantools/45.1.2/). Py≥3.10,python-can≥4.6.0 요구도 lock에 반영한다. |
| canmatrix | 1.3.3,2026-10-21,BSD-2-Clause | 확인 | [metadata](https://pypi.org/project/canmatrix/1.3.3/). 단,아래 asammdf와 버전 동시 선택 충돌. |
| asammdf | 8.8.32,2026-11-03,LGPLv3+ | 확인 | [metadata](https://pypi.org/project/asammdf/8.8.32/). GPL 전용으로 정정할 사안이 아니다. |
| Textual | 8.2.8,2026-06-30,MIT | 확인 | [metadata](https://pypi.org/project/textual/8.2.8/). |
| pyqtgraph/PySide6 | 0.14.0 MIT/6.12.0 LGPL-3.0 | 대체로 확인,라이선스 축약 | [pyqtgraph](https://pypi.org/project/pyqtgraph/0.14.0/), [PySide6](https://pypi.org/project/PySide6/6.12.0/): 후자는 LGPL3-only OR GPL2-only OR GPL3-only 선언이다. |
| tsdownsample | 0.1.5.1,2026-06-01,MIT | 확인 | [배포 파일](https://pypi.org/project/tsdownsample/0.1.5.1/#files). CPython3.11–3.14의macOS arm64/Linux x86_64/Windows amd64 wheel 존재도 확인했다. |
| websockets/FastAPI | 17.4 BSD/0.162.2 MIT | 확인 | [websockets](https://pypi.org/project/websockets/17.4/), [FastAPI](https://pypi.org/project/fastapi/0.162.2/); websockets는 BSD-3-Clause. |
| uvicorn/msgspec | 0.54.0 BSD/0.22.0 BSD | 확인 | [uvicorn](https://pypi.org/project/uvicorn/0.54.0/), [msgspec](https://pypi.org/project/msgspec/0.22.0/); 모두 BSD-3-Clause. |
| py_canoe | 26.4.1,2026-08-30,MIT,COM | 확인,패키지명 구분 | [배포명 py-canoe](https://pypi.org/project/py-canoe/26.4.1/). auto-py-utils 프로젝트로 Vector 공식 SDK라고 부르면 안 된다. |
| ldfparser | 0.26.0 MIT | 확인 | [metadata](https://pypi.org/project/ldfparser/0.26.0/). LIN runtime을 범위 밖에 둔 판단에 동의한다. |
| autosar-data | 0.17.0,라이선스 미확인 | **미확인 해소** | [원본 Cargo.toml](https://github.com/DanielT/autosar-data-py/blob/main/Cargo.toml)에 version0.17.0과 `MIT OR Apache-2.0` 선언. 최종 artifact의 notice는 배포 시 확인. |
| python-can-cvector | 1.6.0 LGPLv3,이점 미확인 | **기능 확인** | [프로젝트](https://github.com/zariiii9003/python-can-cvector): VectorBus subclass,Cython send/recv 재구현,LGPL-3.0-or-later. XL Windows 경로의 개선 후보이며 timing 보증은 없다. |
| polars/pyarrow | 2.1.0 MIT?/26.0.1 Apache2 | **MIT 미확인 해소** | [polars metadata](https://pypi.org/project/polars/2.1.0/), [원본 LICENSE](https://github.com/pola-rs/polars/blob/main/LICENSE), [pyarrow](https://pypi.org/project/pyarrow/26.0.1/). |
| uPlot | 1.6.x 기억,버전 미확인 | **1.6.32 MIT 확인** | [npm latest](https://registry.npmjs.org/uplot/latest). 실측 번들 크기·성능은 계속 미확인이다. |

PyPI 라이선스 필드가 비어 있다는 것과 upstream 라이선스가 없다는 것은 다르다.
반대로 wheel이 존재한다는 것과 모든 대상 OS/CPU/Qt 조합에서 동작한다는 것도 다르다.

### 3.2 사실 오류·과도한 보장 — 리드 수정 우선순위

**F1 · P1 · SocketCAN HW timestamp(§8).** “HW TS ✓” → stock python-can4.6.1은 `SO_TIMESTAMPNS` 경로다.
[socketcan.py:792](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/socketcan/socketcan.py#L792)와 [Linux timestamping](https://docs.kernel.org/networking/timestamping.html) 참조.
일반 kernel software timestamp와 HW `SO_TIMESTAMPING`/driver 설정을 구분하고 capability 기본값을 software로 고친다.

**F2 · P1 · Windows sleep(§3.4).** “Python≥3.11 고해상도 timer 사용”은 사실, “따라서10ms±1ms”는 미검증 추론이다.
[Python3.11 time.sleep](https://docs.python.org/3.11/library/time.html#time.sleep)은 waitable timer 변경과 scheduler로 인한 추가 지연을 함께 설명한다.
[Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw)는 고해상도 flag를 설명할 뿐 Python thread 실행 deadline을 보장하지 않는다.
100ns timer resolution,thread wake-up latency,GIL 대기,driver 접수,wire 전송 시각을 각각 분리해야 한다.
또한 python-can 자체도 [broadcastmanager.py:305–346](https://github.com/hardbyte/python-can/blob/v4.6.1/can/broadcastmanager.py#L305)의 pywin32 timer 경로가 있어 비교 환경을 고정해야 한다.

**F3 · P1 · Vector 공존(§8.1).** “네 인자를 모두 생략하면 공존”은 CC의 후보 경로이지 FD 공존 일반해가 아니다.
FD는 V4 interface와 `fd=True`가 필요하고 init 요청 mask도 만든다.
반환 permission mask로 [설정 write 범위](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/vector/canlib.py#L532)를 제한하므로 init 요청 실패가 항상 open 실패인 것도 아니다.
소스 경로 존재는 확인,현장 CANoe/driver/HW 조합의 공유 RX/TX·재설정 무발생은 **미확인**으로 유지한다.
“HW listen-only 불가”도 절대 사실 대신 “init 없으면 Loom이 채널 silent 설정을 바꿀 수 없음”으로 고친다.

**F4 · P1 · HW 시각 모델(§6.1).** “python-can timestamp=raw driver clock” → Vector는 이미 XL tick를 초로 바꾸고 host offset을 더한다.
[canlib.py:761–765](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/vector/canlib.py#L761).
`backend_timestamp`,host ingest monotonic,선택 raw device tick/clock domain을 구분한다.
자동1분 drift 보정은 원 tick·동기 근거 없이 정확도를 만들지 못한다. wall jump·다른 adapter clock의 오차를 표시한다.

**F5 · P1 · TX echo/결과(§3.4·I2).** “own-message echo에서 seq 매칭” → CAN/driver frame에 Loom seq가 자동 포함되지 않는다.
같은 ID/payload 반복은 모호하므로 backend receipt ID 없이는 정확히 join할 수 없다.
`requested/accepted/confirmed/failed`를 따로 기록하고 원 이벤트 timestamp를 나중에 덮어쓰지 않는다.
`bus.send` 성공을 wire 송신/ECU 수신 성공이라고 표시하면 안 된다.
ISO-TP ECU 응답은 `origin=device`,연결 정보만 `request_origin/corr`로 둔다(UDS v0.2 §9-6).

**F6 · P1 · BLF flush/손실(§3.6·§6.2·§3.12-6).** “10초/4MB flush”는 BLFWriter 기본 API 동작이 아니다.
[blf.py:560–610](https://github.com/hardbyte/python-can/blob/v4.6.1/can/io/blf.py#L560): 기본 container131,072B,private `_flush()`,stop 시 header 갱신이다.
Python file flush·OS cache·fsync·chunk seal을 분리하고 의도한 내구성 창을 정해야 한다.
driver overrun만 “유일한 진짜 손실”이 아니다. recorder queue overflow/disk full/process crash도 원본을 잃는다.
새 spike의 정상 close100,000/100,000 대 abrupt81,920/100,000은 이 위험을 실제로 확인했다(§3.4).
BLF에는 Loom seq가 없으므로 `(ch,ts,id,seq)`만으로 sidecar의 정확한 frame 위치를 찾을 수 없다. chunk+ordinal 또는 canonical journal이 필요하다.

**F7 · P1 · dependency 조합(§7).** asammdf8.8.32+canmatrix1.3.3 동시 pin은 충돌한다.
[asammdf metadata](https://pypi.org/pypi/asammdf/8.8.32/json)의 요구는 `canmatrix[arxml,dbc]==1.2`다.
MF4 extra의 호환 lock을 따로 만들거나 변환기 환경을 분리해야 한다. 두 라이브러리 자체 버전은 맞다.
MF4Reader는 CAN bus logging 대상,MF4Writer append 미지원이며 일반 MF4 신호는 asammdf adapter로 읽는다.
근거: [mf4.py:84–124,318](https://github.com/hardbyte/python-can/blob/v4.6.1/can/io/mf4.py#L84).

**F8 · P1 · 승인 보장(§3.10·§10.1·CR5).** “user origin 뷰어만 승인→agent 자기승인 불가”는 성립하지 않는다.
공유 discovery token과 self-declared client/origin만 검사하면 agent도 같은 RPC를 구성할 수 있다.
`--as-user`와 user 무제한 TX 예외까지 있어 origin이 안전 경계가 되어서는 안 된다.
서버 발급 role/capability·승인 범위/hash/만료를 정의하고 agent 경로에 승인 자격을 주지 않아야 한다.
동일 OS 계정의 agent가 모든 사용자 자격을 읽을 수 있는 위협 모델에서는 UI 분리만으로 사람임을 증명하지 못한다.
그 한계를 명시하거나 별도의 사용자 승인 채널/OS 보안 경계를 써야 한다. 이는 취향이 아니라 주장과 집행 방식의 불일치다.

**F9 · P2 · 다운샘플/Fixed 정확도(§3.12).** “MinMaxLTTB=픽셀 min/max=스파이크 보존” → 서로 다른 보장이다.
[tsdownsample](https://github.com/predict-idlab/tsdownsample)은 min/max 후보를 만든 뒤 LTTB로 더 줄인다. 모든 극값·짧은 상태 전이를 보존하지 않는다.
Fixed도 드라이버/집계 입력 손실이 없을 때 최신값·count가 정확하다. stale/불완전 quality가 필요하다.
알람·wait-for·summary 통계는 원본에 대해 계산하고 plot 근사와 분리한다.

**F10 · P2 · Qt 성능/라이선스(§5.1·§7·R10).** pyqtgraph “최상(OpenGL)” 순위는 동일 데이터 비교가 없어 **의심(미확인)**이다.
웹 선택 근거는 공유/배포/범용성으로 충분하다. 근거 없는 성능 서열은 삭제한다.
“LGPL 수정 없이 의존→문제 없음/수정 시 공개만”도 배포 조건 전체를 설명하지 못한다.
license notice·배포 library source/교체 가능성 등 실제 적용 의무를 release 체크에 넣되 이번 계획에 법적 적합 판정을 쓰지 않는다.
원문 확인 경로: [asammdf의 LGPL LICENSE](https://github.com/danielhrisca/asammdf/blob/master/LICENSE).

### 3.3 남겨 둔 ‘미확인’의 처리

| 항목·위치 | 최종 판정 | 리드 조치/근거 |
|---|---|---|
| PCBUSB 현 상태·Apple silicon·FD·라이선스,§8 | 공개 지원 범위는 확인,현장 작동은 미확인 | [README](https://github.com/mac-can/PCBUSB-Library), [v0.13/2024-07-28](https://github.com/mac-can/PCBUSB-Library/releases/tag/v0.13), [EULA](https://github.com/mac-can/PCBUSB-Library/blob/main/LICENSE). macOS12+ Intel/Apple silicon·PCAN-USB FD 명시,binary freeware 자체 EULA. 현재macOS27/보유 장치는 미인증. |
| Vector driver 최신·공유 TX,§8 | 26.30.16 Windows 공개 배포 확인;공유 TX 실HW 미확인 | [Vector 공식](https://www.vector.com/de/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/). open 순서·CC/FD·각 app 종료까지 인증한다. |
| CANoe 공식 Python/XIL,§1 | 일반 공식 PyPI SDK는 확인 못 함;공식 MCP 존재 | [Vector CANoe AI](https://www.vector.com/en/product/canoe/canoe-ai-features/):20 SP2부터 MCP Server/AI Package. 공식 MCP→COM fallback 순서로 후속 평가;XIL 설치 API/entitlement는 미확인. |
| tsdownsample wheel,§7 | 대상3 OS×CPython3.11–3.14 주요 wheel 존재 | r2-package-evidence.json의 파일명. wheel 설치/성능·free-threaded 지원은 별개. |
| cantools decode 비용,§3.3 | 단순3신호451,855frames/s 실측 | R1 r1-results.json,200k회/0.443s. 복잡한 mux/저장/UI 부하를 포함하지 않는다. |
| BLF 처리량·crash,§6 | 단독 처리량/abrupt tail 손실 확인 | §3.4;전체 RX→기록+UI5kfps 보증은 여전히 미확인. |
| Windows10ms±1ms·CANoe 공유·HW cyclic | 미확인 유지 | macOS 결과를 Windows로 일반화하지 않는다. HW/driver/pywin32 유무를 고정한 wire 시험 필요. |
| PNG 서버 렌더·JS bundle·Kvaser mac | 미확인 유지 | 이번 연구 범위 밖;M1의 차단 조건으로 만들지 않는다. |
| Codex skill 호환,§15.5 | 공식 문서로 확인 가능 | [Build skills](https://learn.chatgpt.com/docs/build-skills):name/description frontmatter와 SKILL.md·references 지원,repo `.agents/skills`,symlink 지원. Claude 경로의 원본을 symlink/동일 소스로 제공하면 된다. |

### 3.4 측정으로 본 §3.12·§11의 현실성

R1 결과는 `spikes/can/r1-results.json`,새 결과는 `spikes/can/r2-results.json`이다.
R2 재현: `spikes/can/.venv/bin/python spikes/can/r2_probe.py`.
환경은 macOS27.0.1 arm64/CPython3.14.1/python-can4.6.1,단일 실행이다.
R1 FC 표본은 조건별100개·PDU왕복50회,R2 주기 표본은 task별149–152간격으로 작다.

| 실험 | idle | GIL decode load | 해석 |
|---|---|---|---|
| R1 FF-send→FC-send p99/max | 1.486/2.557ms | 51.863/60.503ms | 같은 프로세스의 ISO-TP도 Python CPU 경쟁 영향을 받는다. |
| R1 256B PDU RTT p99 | 9.566ms | 253.774ms | STmin0,BS0,오류0이나 tight deadline 보증은 아니다. |
| R2 10ms send-hook 간격오차 p99,task0x100/101 | 4.556/6.777ms | 46.833/59.753ms | observer RX 지연을 뺀 host send 호출 측정도 ±1ms를 입증하지 못했다. |
| R2 위 오차 max,task0x100/101 | 5.030/6.964ms | 100.344/103.033ms | wire jitter·Windows·타이머 휠 결과는 아니다. |
| R2 BLF 정상 close | 100,000프레임,0.988s,약101,199fps | 해당 없음 | 단독 writer+close 처리량은 충분해 보이나 전체 pipeline 검증 아님. |
| R2 BLF abrupt exit | 81,920프레임 읽힘,18,080누락 | 해당 없음 | reader 오류 없이 끝남. no stop/no explicit flush인 child process 시험. |

BLF 시험은 작성 payload·timestamp를 고정하고 ordinary file/zlib을 썼다. fsync/전원 장애 시험은 아니다.
abrupt 결과를 “항상18,080프레임 유실” 또는 “10초 flush를 구현해도 같은 유실”로 해석하지 않는다.
R2 send hook은 virtual bus에 들어가기 직전 두 Python periodic task를 관측한다. 타이머 휠 비교는 실행하지 않았다.
R1 FC 측정 역시 wire latency가 아닌 host send hook 간격이며 USB·중재·Windows가 빠져 있다.
load는 별도 thread의 DBC decode 경쟁이며 물리100% bus load 등가 조건이 아니다.

| 초안 목표 | 현실성 판정·수용 기준 수정 |
|---|---|
| M1 TUI p95<200ms·late join<1s | **현실적인 목표**,아직 달성 증거 없음.10Hz redraw+20–50ms batch라 가능성은 있으나 refresh 완료 계측·원자적 join·bounded backlog가 필요. |
| 웹 p95<150/p99<200ms,Graphics10–20Hz | 정상 정의된5kfps/2viewer profile에서 검증할 목표로 수용.고부하·OS 전체의 절대 상한으로 부르지 않는다. |
| send→버스<10ms,STOP→마지막 frame<50ms | 시작/끝 정의 수정.먼저 gateway accept→driver submit와Stop→신규 submit 차단을 측정;wire 완료는 지원 backend/큐/중재별 별도 인증. |
| trace 조회 가능<10ms·wait-for<20ms | ring ingest/조건 trigger와 CLI 기동·RPC 응답·decode 대기를 구분한다. 기준 머신·CPU 부하·percentile을 명시한다. |
| fake ECU가5ms 안에 응답 | 정상 부하 목표로만 유지.일반 CI OS scheduler에서 hard deadline으로 고정하면 flaky하다. |
| 5kfps60초 BLF 무손실 | 합리적 정상 수용 기준.송신·응답·echo 수를 정의한 generator 기대 집합과대조하고 close 후 재독출;record health 실패·gap 시험도 추가. |

타이머 휠은 task 관리/스레드 수를 줄이는 **설계 후보**다.정확도 개선의 증명은 아니다.
tick 크기·여러 deadline 동시 도착·한 `bus.send`의 blocking이 전체 태스크를 늦추는 head-of-line 문제를 명세해야 한다.
M2 첫 기본안은 monotonic absolute deadline+깨어날 수 있는 wait+bounded send timeout의 단일 min-heap scheduler를 추천한다.
missed cycle은 기본 skip/count,지연분을 burst catch-up하지 않는 정책을 정하고 timer wheel은 task 수 측정 후 비교한다.
SocketCAN은 BCM 유지,`modifier_callback`을 주면 [thread fallback](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/socketcan/socketcan.py#L939)이므로 IG는 가능한 `modify_data` 경로를 쓴다.
ISO-TP FC는20/50ms 주기 wheel에 넣지 않고 stack의 즉시 처리 경로에 유지한다.

## 4. 빠진 것

15개 절은 모두 존재한다.다음은 제목 누락이 아니라 통합 구현 전에 필요한 계약의 빈칸이다.

1. **§3·§4 snapshot 원자성:** snapshot high-water S를 고정한 뒤 `after_seq=S`를 연결하고 retention gap·재시작 epoch를 반환해야 한다.
   서버측 `@last` 자동 전진도 client/topic뿐 아니라 filter/query key·동시 CLI 호출·응답 유실 처리 규칙이 필요하다.
2. **§6·CR3 이벤트 호환:** 공통 `ts/domain/channel/dir/origin/payload` envelope와 flat frame을 하나로 매핑하고 seq/wall_ns는 JSON 문자열/64bit binary로 정한다.
   `origin=replay`는 공통 enum을 늘린다.v0.2에 맞춰 `origin=script,purpose=replay,original_origin=...`을 권장한다.
3. **§3.12 과부하 계수:** display gap의seq범위/count와capture/record gap을 분리한다.정확히 셀 수 없는 driver 누락은 `unknown`이다.
   Fixed 최신값·count,recording intact도 건강 상태에 의존한다.대기열512MB가 실제 Python object RSS 상한인지 검증해야 한다.
4. **§4·§5 IG 상태:** 전체 초기값·비활성 mux branch·CAS revision·사용자 takeover·E2E/rolling counter 기본 정책이 없다.
   `--base last`는 stale·잘못된 mux·device counter를 복사할 수 있다.미지정 필수값은 거절하고 `새 태스크`로 같은 ID producer를 중복 생성하지 않는다.
5. **§8·§9 HW capability:** channel별 FD/data bitrate/HW timestamp/TX receipt/bus load/error frame 지원표가 필요하다.
   CC500k8B의3.7–4.5kfps는11bit·약111–135bit/frame의 산술 추정으로 타당하지만 FD2–5M만으로20kfps를 보장하지 못한다.
   FD payload·nominal bitrate·channel 수를 profile에 고정하고 error bits/ACK/버스 재전송·추정 load를 표시한다.
6. **§10 실패/취소:** bus-off·owner 종료·daemon 재시작·진행 중 ISO-TP 취소의 잔여 송신과 자동 재개 금지를 명시한다.
   user IG에도 finite rate·기간 한도가 필요하고 승인받은 bytes/ID/rate/기간/hash를 재검증한다.
   Stop 구현에서 [Vector flush_tx_buffer](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/vector/canlib.py#L934)를 범용 queue clear로 쓰면 추가 frame 발생 경고가 있어 검증 전 금지한다.
7. **§11·§12 gate 의존:** Q0에 K4a(policy)·K6a(recorder)를 명시 의존으로 추가한다.M0의 기록 포맷 결정을 미룬 채 M1의 BLF 선택을 확정하지 않는다.
   TUI 이벤트 수신 테스트와 실제 Textual refresh 완료 지연 테스트를 구분하고 disk full/abrupt close/느린 client를 포함한다.
8. **§15 타팀 최신 요구:** UDS v0.2 `rx_start/rx_complete`,epoch,req_id/deadline,4095B cap,예약 raw 차단,UDS 소유 TesterPresent를 반영해야 한다.
   viz V-UDS-0의 점진 transaction 갱신·p95≤100ms/p99≤200ms·terminal event 비집계·take-control을 M3 수용 기준에 넣는다.
9. **§6·§10 비밀/비신뢰 데이터:** UDS 승인 토큰/seed-key 원문 노출 금지,XML entity 차단,regex/파일크기 한도,XSS escape를 추가한다.
   공통 승인/token endpoint의 Host/Origin 검증·단기 bootstrap·읽기 role/승인 role 분리도 필요하다.

## 5. 더 나은 선택 (상대 vs 나)

| 쟁점 | 상대 안 | 내 R1 안 | 추천 | 이유 |
|---|---|---|---|---|
| daemon 배치 | 단일 process, 문제 시 CAN 자식 분리 | supervisor+CAN/기록/viz worker | M1 단순 배치 허용, M2부터 decode/export 격리 기본 | R1 GIL 실측으로 FC 위험 확인. 사용자 URL/권한 통합과 process 통합은 별개다. |
| IPC | JSON-RPC/WS+msgpack 열 블록 조기 도입 | JSON batch 먼저, 필요 시 binary | M1 JSON batch, M3 측정 후 column binary | 양안 모두 25Hz 안팎 batching 동의. 초기 binary는 핵심 루프 gate에 불필요. |
| viewer | 웹 주 UI+Textual TUI, M1 TUI | 동일 | 상대 안 채택 | 화면 구성·키·Activity·user NOTE가 구체적이다. |
| periodic scheduler | 즉시 자체 timer wheel | send_periodic/BCM capability+실측 | BCM 또는 단일 absolute-deadline heap로 시작, wheel 비교 후 | wheel로 ±1ms 보장 불가, blocking·catch-up·cancel 정책이 우선. |
| 기록 원본 | BLF+TX sidecar | canonical journal+표준 export | journal/manifest를 원본, BLF를 교환 포맷으로 | abrupt loss/seq join 근거. 단독 BLF throughput은 충분하므로 BLF 전체를 폐기할 필요는 없다. |
| M1 범위 | raw/TUI/사용자 송신/자동 기록 | 동일+기록 손실 분리 | 상대 핵심 루프 유지, 원자적 join/record gate 보강 | 웹/DB를 빼는 순서가 옳다. |
| ISO-TP API | 본문 sync와 아래 async 철회표 공존 | accepted+transfer/event | UDS v0.2 async 기준으로 하나만 남김 | deadline/req_id/rx_start가 P2 오판과 late event 처리에 필요. |
| raw 예약 충돌 | agent R4 승인, user 경고 | 기본 차단 | 기본 차단, 정지/해제 후 takeover | 위험 승인만으로 두 상태기의 wire 간섭이 해결되지 않는다. |
| 패딩/maxPDU | 0xCC/FD 확장 | null/확장 64KiB 후보 | null+ECU profile, 초기 4095B | 내 R1의 64KiB 확대 후보도 v0.2 초기 범위에서는 철회한다. |
| 위험 등급 | R0–R4 | R0–R3 | 숫자는 core 통일, scope/rate는 user도 집행 | 등급 수보다 실효 승인과 동일 producer 중재가 중요하다. |
| 스킬 | 공통+CAN+viz, references 상세 | CAN/viz CLI 공유 | 상대 구조 채택, Codex `.agents/skills` 배포 추가 | 이미 공식 지원이 확인돼 별도 호환 추측이 불필요하다. |
| CANoe 제어 | py-canoe COM 4동사 M5 | 공식 MCP 우선 조사 | M5 공식 MCP→COM fallback | 현재 Vector 공식 경로가 존재한다. 어느 쪽도 raw CAN/FC 기본 경로는 아니다. |

표의 구현 선택은 리드 제안이다. 이번 라운드에서 제품 구현·타팀 계약 수정은 수행하지 않았다.
단일 process를 불가능하다고 판정하지 않는다. M1 저부하 조건과 M2 이후 CPU 경쟁 조건을 분리해 판정한다.
free-threaded Python은 native dependency 호환과 실측 없이는 우선 완화책으로 삼지 않는다.

## 6. 합의 가능한 부분

- §0·§2.0·§3.11을 통합 계획의 핵심으로 채택한다. 장치 owner는 하나이고 agent/user/device가 같은 stream에 보인다.
- §3.9의 단일 Notifier+NotifierBasedCanStack와 PDU 경계 IPC를 채택한다. 다른 recv 소비자를 추가하지 않는다.
- §3.3의 지연 decode·관심 신호 cache·구독별 batch·느린 viewer 격리를 채택한다. callback은 queue 전달만 한다.
- §4의 bounded CLI·필터·커서·출력 잘림 표시·summary 중심 viz 사용을 채택한다.
- §5의 웹 주 UI/TUI 보조·Trace Fixed/Chrono·Graphics/State·IG·STOP·출처 문자 배지를 채택한다.
- §6의 DB hash/설정/버전 metadata·표준 CAN export·long/wide 형식·Parquet 시간 범위 query를 채택한다.
- §9의 virtual fake ECU·3 OS CI·실HW 공존 별도 gate와 §11의 M1 raw/TUI 우선 순서를 채택한다.
- §10의 daemon 정책 집행·dry-run·lease·감사와 §15.5의 짧은 SKILL.md+references 구조를 채택한다.
- late viewer backlog·Activity·사용자 개입 NOTE는 상대 초안의 좋은 구체화다. 단위/field 스키마를 고정해 재사용한다.

## 7. `_xteam/can-uds-isotp.md` 미결 8항목에 대한 최종 의견

§8의 5질문에 v0.2에서 결정해야 할 3항목을 더해 아래 8개로 답한다. 문서 직접 수정은 리드에게 맡긴다.

| # | 쟁점 | 최종 의견 |
|---|---|---|
| 1 | 엔진(§8-1) | can-isotp 2.0.7 NotifierBasedCanStack, 단일 Notifier 공유. 자체 ISO-TP 재구현 불필요. |
| 2 | 패딩(§8-2) | 기본 null, ECU profile로 0xCC/0x55 명시. FD DLC 맞춤 padding과 ECU 고정 padding을 구분. |
| 3 | 소유권/raw 충돌(§8-3,§9-7) | 전체 addressing/address 필드를 owner 키로, tx_id 예약 raw는 user/agent 모두 기본 차단. 명시 override는 정지/권한 이전·감사 포함. |
| 4 | 주기 PDU(§8-4,§9-5) | **TesterPresent 의미·timer는 UDS 상주 session manager 소유**, CAN 기능은 선택. idle_only·거래 serialization·owner lease·disconnect/Stop 종료 필수. |
| 5 | IPC(§8-5) | local HTTP/WS+JSON-RPC 배치 권고. 필드/상태 의미를 먼저 합의, 동기 helper는 동일 async job을 기다리는 얇은 wrapper. |
| 6 | 시작/완료·timing(§9-2/3/4/10) | epoch/req_id/deadline, rx_start/rx_complete 필수. TX submit/confirmed와 ts_source/uncertainty 구분. P2는 UDS 소유이며 late event/watermark 처리 합의. |
| 7 | functional/group(§4,§9-8) | normal CC7/FD62, extended·mixed CC6/FD61 상한을 주소별 검증. 한 번 SF 송신+allowlist 멤버별 FC 상태기. unknown 응답에 자동 FC 금지. |
| 8 | maxPDU·자원(§2,§9-9) | **초기 4095B 상한**, queue/PDU 수/총 bytes도 제한. FD의 32bit 길이 표현을 무제한 allocation 허용으로 해석하지 않음. |

UDS v0.2의 PDU 완성→수신 p99≤5ms는 독립된 목표로 계측하고 “동일 process면 사실상0”이라고 확정하지 않는다.
동일 process라도 다른 thread/queue/GIL·callback 구독자 경로에 따라 지연이 생긴다. R1 RTT 수치는 이 구간의 측정값이 아니다.
CAN M2 최소판에 normal11/virtual+위 epoch/rx_start/예약 차단을 포함하고 UDS M1은 DoIP로 독립 진행하는 일정에 동의한다.
M4에 FD/주소 전체/group을 추가하되 추가된 모드도 같은 사용자 Trace와 자동 기록을 통과해야 한다.
리드 우선 결정은 ① shared stream/원자적 join ② 기록 내구성/손실 ③ 승인 role ④ UDS v0.2 경계 ⑤ scheduler 실측 순서다.

<!-- END R2 -->
