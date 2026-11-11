# Explore R2 교차 리뷰 — Sol → Opus R1

작성: 2026-11-09 KST. 대상: [r1-opus.md](r1-opus.md). 비교: [r1-sol.md](r1-sol.md).
최우선 기준은 [02-core-principle.md](../02-core-principle.md)다. 두 R1은 수정하지 않았다.
판정은 계획의 정확성과 수용 기준에 대한 것이며 구현 완료·물리 HW 시험 판정이 아니다.
P0=장비 안전/핵심 루프를 깨므로 통합 전 수정, P1=설계·수용 기준 확정 전 수정, P2=근거·표기 보완이다.

**조건부 채택 권고.** 사용자 개입 시나리오·선언형 패널·벤치 프로파일은 좋다.
다만 pyOCD 연결 안전 기본값, 송신 전 방송의 의미, IO 읽기 gate, 기록/지연 보장은 수정해야 한다.
최종 기본 추천은 **PSU → DLT → SSH → Debug의 4개**다. 다섯째는 보유 장비에 따라 IO 또는 XCP를 조건부로 정한다.

R2 재확인 자료는 `spikes/explore/r2-sol/`에 보존했다.
`source-evidence.json`은 12개 패키지 재조회와 공식 문서/소스, `local-checks.json`은 pyOCD 옵션·TOML 파싱 결과다.
`dependency-conflict.txt`, `behavior-results.json`, `score-audit.json`, `local-source-manifest.json`이 추가 근거다.
존재하지 않는 COVESA `v3.0.1` raw URL 조회 실패도 남겼다. 이를 3.0.1 프로토콜 부재로 해석하지 않았다.
DLT 확인은 사용자 로컬 orig 헤더/빌드 원본 소스와 upstream master를 구분했고 로컬 파일 hash를 저장했다.
R1의 백엔드 6개·import 14개 결과도 재사용하되 해당 범위를 넘는 성능 주장은 하지 않았다.

## 1. 틀린 사실

### F1. “attach만, 코어 영향 없음”은 pyOCD 기본 동작으로 보장되지 않는다 — P0

- 대상: Opus §4.4 a/b/i, 특히 469행의 코어 영향 없음 표현.
- **확인:** 설치된 pyOCD 0.46.1의 API 옵션 기본값은 `connect_mode='halt'`, `auto_unlock=True`, `resume_on_disconnect=True`다.
- 자동 unlock은 보호된 타깃을 erase하여 접근하는 경로를 허용한다. 단순 연결을 R0으로 분류하면서 이 옵션을 통제하지 않으면 R5 금지 정책을 우회할 수 있다.
- 또한 `cache.read_code_from_elf=True`는 code read를 실제 타깃 대신 ELF에서 만족시킬 수 있다. 장비 읽기와 파일/캐시 읽기의 출처를 구분해야 한다.
- **수정:** Loom의 제안 기본을 `attach`로 두는 것은 가능하다. backend 계약에 `connect_mode=attach`, `auto_unlock=False`, disconnect 상태 정책, 설정 파일/target hook 적용 정책을 명시한다.
- read 결과에는 `source=device|cache|elf`를 구분하고 live read는 해당 경로를 검증한다. 실제 probe/MCU에서 연결·해제 시 halt/reset/erase가 없음을 시험한다.
- **근거:** [pyOCD 공식 옵션](https://pyocd.io/docs/options.html), 설치본 `core/options.py`, `r2-sol/local-checks.json`.
- **확신도: 높음.** 이는 제안한 Loom attach가 불가능하다는 뜻이 아니라 라이브러리 기본값만으로 안전 보장을 얻을 수 없다는 뜻이다.

### F2. 최신 pyOCD와 최신 PyLink를 같은 extra에 나열한 조합은 동시 고정 설치가 안 된다 — P1

- 대상: §3 후보 4, §4.4 f, §5.11의 `loom[dbg]`.
- pyOCD 0.46.1의 의존성은 `pylink-square>=1.0,<2.0`이다. pylink-square 2.1.0과 함께 고정하면 resolver가 `ResolutionImpossible`을 반환한다.
- R2에서는 `pip install --dry-run pyocd==0.46.1 pylink-square==2.1.0`으로 재현했다. 실제 환경 변경은 없었다.
- 버전 두 개가 각각 최신이라는 사실은 맞다. 그 조합을 검증된 공존 baseline으로 채택할 수 없다는 것이 오류다.
- **수정:** 공존 baseline은 R1에서 설치/import한 pyOCD 0.46.1+PyLink 1.7.0으로 고정하거나, J-Link backend 환경을 분리한다.
- **근거:** [pyOCD 0.46.1 metadata](https://pypi.org/pypi/pyocd/0.46.1/json), `r2-sol/dependency-conflict.txt`.
- **확신도: 높음.**

### F3. DLT TCP 수신 바이트를 그대로 저장해도 표준 `.dlt` storage 형식이 되지는 않는다 — P1

- 대상: §4.1 c, 199행의 수신 바이트 그대로/storage header 포함 표현.
- DLT V1 wire message와 파일의 storage header는 다르다. V1 `.dlt`는 메시지 앞에 `DLT\x01`, 수신 시각·ECU ID를 담는 storage header를 둔다.
- 로컬 COVESA `dlt_client.c`는 storage header를 제외하여 송신하며, `DltStandardHeader.len`도 storage header 제외 길이다.
- pydlt 0.3.5에서 만든 29-byte wire message는 prefix `2500001d`이며 storage 형식으로 parse하면 `DLT\x01` 부재 오류가 난다.
- **수정:** 원시 TCP chunk 기록은 `wire.bin+chunk index`, 표준 export는 framing 후 storage header를 생성한 `.dlt`로 구분한다.
- 완전 payload decode 전에 framing/export는 가능하지만 TCP chunk=DLT message로 가정하면 안 된다. partial frame·잘못된 길이도 보존/표시한다.
- **근거:** [COVESA dlt_common.h](https://github.com/COVESA/dlt-daemon/blob/master/include/dlt/dlt_common.h), [dlt_client.c](https://github.com/COVESA/dlt-daemon/blob/master/src/lib/dlt_client.c), 로컬 원본 발췌 및 `r2-sol/behavior-results.json`.
- **확신도: 높음.**

### F4. `MARKER(0xF04)`에 임의 label이 wire로 전달된다고 가정할 근거가 없다 — P2

- 대상: §4.1 d `marker --label TEXT`, §5.12의 `label: "run end"`.
- 로컬 `DltServiceMarker`는 `service_id`와 `status`만 가진다. daemon marker 생성 코드도 이 구조를 보낸다.
- CLI의 label 기능 자체는 가능하지만 COVESA marker 응답이 문자열을 보존해 준다는 뜻은 아니다.
- **수정:** label은 core annotation/sidecar로 기록하고 marker 응답과 연결할 정책을 명시한다. remote marker와 local annotation을 구분한다.
- **근거:** [COVESA marker 구조](https://github.com/COVESA/dlt-daemon/blob/master/include/dlt/dlt_common.h), [daemon marker 구현](https://github.com/COVESA/dlt-daemon/blob/master/src/daemon/dlt_daemon_client.c), `local-dlt-common-excerpts.txt` 및 `local-dlt-daemon-client-excerpts.txt`.
- **확신도: 높음.**

### F5. PSU 자발 상태 변경을 1 Hz 폴링으로 500 ms 안에 항상 표시할 수는 없다 — P1

- 대상: §4.2 a의 상태 폴링 1 Hz와 §4.2 c의 보호 트립/출력 상태→화면 ≤500 ms.
- 폴 직후 보호 트립이나 front-panel 변경이 생기면 다음 폴까지 거의 1 s다. 쓰기 직후 즉시 폴은 agent/user가 명령하지 않은 사건을 해결하지 못한다.
- 균일한 사건 위상이라는 계산 모델에서 감지 대기 p95는 약 950 ms, 500 ms 이내 비율은 50%다. 통신·화면 시간도 아직 더하지 않은 값이다.
- **수정:** unsolicited SRQ/event 지원이 검증된 기종을 쓰거나 상태 폴 간격을 줄인다. 아니면 목표를 `poll period + query RTT + render budget`으로 수정한다.
- 10 Hz V/I도 장비의 측정 갱신률·두 query의 RTT를 먼저 재야 한다. 100 ms 고정 해상도 라벨 대신 모델별 실제 갱신/평균 조건과 age를 표시한다.
- **근거:** Opus의 주기 정의에 대한 산술, `r2-sol/behavior-results.json`의 `status_poll_math`.
- **확신도: 높음.** 계산 모델이며 실 장비 benchmark가 아니다.

### F6. 릴레이 controller readback만으로 기계 접점 동작 시간을 확정할 수는 없다 — P1

- 대상: §4.5 c, 기계식 릴레이 동작 시간을 되읽기로만 확정한다는 표현.
- 장치의 relay-state 응답이 command latch/coil drive 상태인지, 실제 접점·부하 전압 feedback인지 먼저 구분해야 한다.
- device에서 실제 조회한 상태는 핵심 루프의 읽기로 인정할 수 있다. 다만 그것이 물리 KL30 차단 완료의 증거인지는 별도다.
- **수정:** `commanded`, `device_reported`, `contact_verified`, `unknown`을 나눈다. 접점 완료 시간은 독립 feedback이나 외부 계측이 있을 때만 기록한다.
- **근거:** [Numato relay manual](https://numato.com/docs/8-channel-usb-relay-module/)의 relay read 명령 범위, [pyhid-usb-relay state API](https://pypi.org/project/pyhid-usb-relay/1.4.0/). 두 API 이름만으로 물리 feedback을 추론할 수 없다.
- **확신도: 높음**(readback의 증명 범위), **미확인**(사용자 보드에 실제 feedback이 있는지).

### F7. “별도 프로세스이므로 라이선스 영향 없음”은 과도한 단정이다 — P1

- 대상: §4.1 f의 dlt-daemon MPL-2.0 설명, §1.3의 GPL 대안 설명.
- MPL 소프트웨어의 사용과 외부 배포 조건은 구분해야 한다. subprocess라는 이유만으로 해당 프로그램의 고지·소스 제공 의무가 사라지지 않는다.
- **수정:** dlt-daemon은 MPL-2.0으로 확인하고, 동봉 여부·수정 여부·외부 배포 여부에 따른 패키징 검토로 남긴다.
- 별도 프로세스는 장애/의존성 격리 대안이며 라이선스 면제 보장으로 제시하지 않는다.
- **근거:** 저장한 COVESA LICENSE, [Mozilla MPL 2.0 FAQ Q5–Q10](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).
- **확신도: 높음.** 구체 배포물의 법적 결론까지 내린 것은 아니다.

### F8. 일부 “미확인 라이선스”는 실제 원문으로 확인 가능하다 — P2

| 대상 | Opus 표기 | R2 확인 | 수정 |
|---|---|---|---|
| pyhid-usb-relay 1.4.0 | 비표준 표기라 미확인 | PyPI license에 MIT 허가문 전체, classifier도 MIT | MIT로 정정 |
| someipy 2.1.2 | PyPI 미표기 | upstream LICENSE.md GPL v3 | 소스 GPL-3.0 명시, 채택 artifact와 다시 대조 |
| logic2-automation 1.0.11 | 라이선스 미표기 | upstream LICENSE Apache-2.0 | 소스 Apache-2.0 명시, 채택 artifact와 다시 대조 |

근거: [pyhid metadata](https://pypi.org/pypi/pyhid-usb-relay/1.4.0/json), [someipy LICENSE](https://github.com/chrizog/someipy/blob/master/LICENSE.md), [Saleae LICENSE](https://github.com/saleae/logic2-automation/blob/master/LICENSE).
확신도는 pyhid metadata **높음**, upstream 두 파일 **높음**, 해당 과거 wheel/sdist와의 완전 일치 여부는 **미확인**이다.
미확인이라고 적은 것 자체를 허위 사실로 보지는 않는다. 확인 가능한 근거를 추가하는 정정이다.

### F9. 별도 SSH exec 채널은 본질적으로 숨은 연결이 아니다 — P1

- 대상: §4.3 a의 별도 exec면 사용자에게 안 보이므로 공유 PTY를 기본으로 해야 한다는 논리.
- 같은 daemon 소유 SSH connection에서 여러 채널을 열고 요청·stdout/stderr·exit를 동일 event stream에 올릴 수 있다.
- R2 local SSH 시험은 한 connection의 두 exec 채널에서 종료 전에 stdout을 받았고 stderr·exit를 각각 회수했다.
- **수정:** 비대화 exec는 named channel+동일 stream, 대화형 입력은 shared PTY로 나눈다. PTY만이 가시성을 만족하는 것은 아니다.
- Opus가 제안한 side-channel 자동 탭은 이 원칙을 이미 만족하는 좋은 대안이다. 예외 대신 정식 경로로 승격하면 된다.
- **근거:** [AsyncSSH 공식 API](https://asyncssh.readthedocs.io/en/latest/api.html), `r2-sol/behavior-results.json`.
- **확신도: 높음.** local 이벤트 목록은 backend 가능성 근거이며 Loom 뷰어 구현/검증은 아니다.

### F10. 벤치 프로파일 TOML 예시는 현재 구문으로 파싱되지 않는다 — P2

- 대상: §5.7. `vmax = 16.0 imax = 5.0 ...`, probe/device/elf, ssh/policy를 한 줄에 여러 대입으로 쓴 부분.
- Python 3.14 `tomllib.loads`가 line 8 column 15에서 `Expected newline or end of document`를 반환했다.
- **수정:** 각 key/value를 별도 줄로 나눈다. 예제도 프로파일 한계 계약의 일부이므로 복사 가능한 형식이어야 한다.
- **근거:** `r2-sol/local-checks.json`의 `toml_parse`. entry-point TOML 블록은 파싱에 성공했다.
- **확신도: 높음.**

### 사실 오류로 몰면 안 되는 항목

- 재조회한 pyOCD/PyLink/telnetlib3/TRACE32/pyelftools/pylibftdi/OpenCV/Numato/HID 패키지의 버전·날짜는 Opus 표와 일치했다.
- numato-gpio 0.14.0의 Python 범위는 `>=3.9,<3.14`로, Opus의 제한 지적이 맞다. 이는 프로젝트의 Python≥3.11 계약 전체와 충돌하는 것은 아니다.
- pyOCD 0.46.1에는 실제 `rtt` subcommand가 있다. Renode/QEMU의 GDB 경로와 OpenOCD Tcl 기본 6666도 R1 자료로 확인했다.
- DLT service ID 0x01/0x03/0x11/0x13/0xF04/0xF08은 원본 헤더와 일치한다. 번호가 틀렸다는 지적은 하지 않는다.
- 후보 18행 점수를 재계산했다. 정수 반올림을 허용하면 모두 맞다. 평가 근거에는 이견이 있지만 산술 오류는 아니다.
- SimDltServer/SimPsu/SimScpiServer/SimTarget/SimRelay/SimWorld는 **앞으로 만들자는 계획명**이다. 현재 없다는 이유만으로 허위 라이브러리라고 평가하지 않는다.
- CH341/Bus Pirate의 특정 PyPI 이름 404는 조사 사실일 수 있으나 관련 모든 Python 프로젝트의 부재를 증명하지 않는다. FlexRay도 “이번 조사에서 검증한 wrapper 없음”으로 표현을 좁힌다.

## 2. 평가 이견

기호는 접근/읽기/쓰기/실시간 순서다. 도메인의 원리상 능력과 이번에 채택할 backend의 검증 상태를 분리한다.
추가로 “현재 검증 상태” 열을 두면 도메인 가능성 ○를 HW 검증 완료처럼 읽는 일을 줄일 수 있다.

| 후보/기준 | 상대 판정 | 내 판정 | 근거·순위 영향 |
|---|---|---|---|
| DLT | ○○○○ / 94 / 1위 | 원리상 동일, 실제 control gate 전 조건부 | 로그 codec은 확인했으나 3.0.1 set-level의 ACK·앱 효력은 양쪽 모두 실측 안 함 |
| PSU | ○○○○ / 94 / 2위 | 동일 / 기본 1위 | 작은 상태ful sim으로 loop를 먼저 검증하기 쉽고 명령·응답·실측 구분이 명료함 |
| Shell | SSH/ADB/telnet/TCP 모두 한 묶음 / 92 | SSH는 ○○○○, 나머지는 후속 backend | 공통 UI 재사용과 인증·PTY·signal·adb 의미가 동일한 것은 아님 |
| Debug | ○○○○ / 88 | 조건부 동일, 안전 초기화 gate 필요 | Cortex-M 가정·SDK·ELF·attach 옵션·라이브 read source를 검증해야 함 |
| IO | ○△○○ / 83인데 상위 5개 | 현재는 상위 추천 보류 | 실제 device readback 있는 모델로 좁히면 읽기 ○ 승격 가능. 명령 캐시만 읽으면 루프 완성 아님 |
| XCP/CCP | 묶어서 ○○○○ / 79 | XCP만 조건부 후보, CCP 별도 미확인 | pyXCP의 XCP 지원은 CCP 지원 근거가 아님. 사용자 slave/A2L 없으면 순위 올리지 않음 |
| CAN XL | ○○○○ / 65, CAN 흡수 | 검증 backend 기준 △△△△, 귀속 요청 | python-can 4.6.1에는 CAN/FD 모델만 확인. XL_API라는 이름은 CAN XL API 증거가 아님 |
| LIN | △○○○, Windows라 접근 △ | Windows의 확인된 PLIN 경로는 ○ 가능, macOS는 미확인 | OS 지원 제한은 O 점수에서 평가. ctypes 작성 비용은 E/M에서 평가해야 중복 감점이 줄어듦 |
| Ethernet/pcap/SOME-IP | 한 행 ○○○△ | passive capture ○○✗○ / active SOME-IP는 조건부 ○○○○ | BPF/Npcap 권한과 SOME/IP 서비스 소켓은 다른 경로다. 단일 점수로 섞지 않음 |
| Scope | ○○○△, 느린 파형이라 실시간 △ | 파형 완료 후 live 표시와 acquisition 지연을 분리 | 작은 bounded snapshot을 즉시 공유할 수 있음. PSU M1 뒤로 두는 우선순위는 동의 |
| Saleae/sigrok | 한 행 ○○△△ | backend별 분리, 상위 제외에는 동의 | Saleae GUI 의존·capture export와 sigrok CLI streaming 특성은 다름 |
| 직접 접근 정의 | 한 OS만 되거나 새 ctypes면 △ | 접근 능력과 OS/구현비 분리 | Windows-only라도 사람 없이 daemon이 장비를 열면 그 환경의 직접 접근은 성립 |

읽기 △인 IO를 추천한 것은 Opus의 “△ 한 개 허용” 자체 규칙에는 맞는다.
그러나 최우선 원칙은 네 요소 모두의 실체를 요구한다. 상위는 선택한 backend에서 네 요소 ○를 입증하도록 gate를 강화해야 한다.
내 R1의 “릴레이 feedback 없으면 읽기 △”도 더 정밀하게 해석한다.
물리 접점 센서가 반드시 있어야만 장비 읽기인 것은 아니다. **실제 device state query는 읽기이고, host command cache는 읽기 대체물이 아니다.**

CAN XL·LIN runtime은 최신 CAN 초안에서 현재 비범위다([CAN Sol §1.4](../can/r1-sol.md), [CAN Opus §1](../can/r1-opus.md)).
따라서 이미 CAN 팀이 구현하기로 한 것처럼 “흡수”라고 확정하지 말고 해당 팀의 범위 변경 요청/후속 귀속으로 남긴다.

## 3. 빠진 것

### 3.1 브리프 산출물과 a~j의 형식은 갖췄지만 검증 깊이가 부족하다

후보는 18개라 최소 15개 조건을 만족한다. 루프 기준·추천·플러그인·착수/질문의 다섯 산출물도 모두 있다.
상위 5개에 a~j 소절이 전부 있으므로 “절이 없다”는 지적은 하지 않는다.
아래는 절 안에 필요한 실행 계약·근거가 빠진 부분이다.

| 영역 | 빠진 부분 | R3 보완 |
|---|---|---|
| 라이브러리 활동 | 최신 릴리스 중심, GitHub commit/issue 활동·실제 지원 Python이 일부 후보에만 있음 | R1 Sol upstream 표+R2 재조회 병합, 활동 날짜와 성능 보증 분리 |
| HW/OS | 별도 HW 가격/조달 표 없음, 모델별 driver·vendor runtime 경로가 얕음 | 보유/미보유와 공식 가격·견적 미확인 구분, native OS smoke gate |
| 시뮬레이터 실재성 | 제안한 fake들과 기존 실물 도구가 같은 나열 수준 | 구현 예정/설치·import 확인/왕복 확인/HW 확인의 4단계로 표기 |
| DLT a/f/g | V1/V2 경계, control fragmentation·timeout·앱 반영, source/wheel 선택 기준 | V1 TCP부터; native daemon control gate; injection은 후속 |
| PSU a/c/h | 실제 측정 refresh·SCPI 직렬화·timeout 상태, on/off readback과 물리 출력 구분 | query 하나만 in-flight, 단위·age·status budget, 실제 장비 한 모델 |
| SSH b/h/i | host key mismatch·named channel 수명·재연결 epoch·PTY 사용자 개입 중 exec 규칙 | host identity 고정, 별도 exec channel, PTY writer lease |
| Debug a/f/h | 연결 안전 옵션·cache/ELF source·probe lifecycle·다중 word coherence | F1/F2와 한 probe/MCU acceptance |
| IO c/h/i | device report vs contact feedback, pulse 실패/daemon crash·타이머 한계 | 모델별 capability, edge/operation limit 분리, 실제 readback gate |
| 공통 M1 | 늦은 viewer 접속 snapshot+cursor, 세션 재연결 epoch·실제 render 계측 | 아래 C1~C5 공통 수용에 추가 |

### 3.2 송신 전 방송은 화면 표시 완료와 실제 TX의 증거가 아니다 — P0

Opus §4 공통 전제·§5.3의 “기록→방송→송신”은 의도 가시성에 유용하다.
그러나 이벤트를 queue에 넣은 시각만으로 사용자가 화면에서 봤다고 말할 수 없다.
그 행을 이미 송신된 TX로 표시하면 driver 실패·부분 송신 때 잠시 거짓 기록이 된다.
다음 상태를 한 `request_id`로 구분해야 한다.

```text
requested/approved → queued → dispatching → driver_accepted
→ response_received → effective_verified | failed | outcome_unknown
```

wire 완료를 측정할 수 없는 backend는 `driver_accepted`를 wire 완료로 승격하지 않는다.
UI render acknowledgement는 지연 계측에 쓰되 모든 송신을 느린 viewer의 ACK에 무조건 묶지는 않는다.
실제 연결이 있는 수신·송신과 아직 계획인 동작이 화면에서 구분되면 핵심 원칙을 지킬 수 있다.

### 3.3 읽기/쓰기의 경계와 사용자 개입 중재가 부족하다 — P0/P1

- `DeviceBackend.open/poll/write`에 deadline/cancel/부분 결과 의미가 없다. 모든 작업에 전체 deadline·전송 취소 가능 여부·실행 결과 미확인을 정의한다.
- Shell read/expect 외에도 DLT contexts, memory read, IO events의 `--timeout/--limit/--max-bytes` 기본값을 구체화한다.
- `--since`만 있는 events는 전체 세션을 반환할 수 있다. cursor·tail·byte cap을 동시에 적용한다.
- 사용자 변경 뒤 agent의 대기 중 쓰기·poll 조건·scenario sweep을 어떻게 중단할지 resource/symbol/channel 단위 lease로 정한다.
- DLT 같은 control은 장비가 request ID를 echo하지 않을 수 있다. 서비스/대상별 직렬화와 늦은 응답 처리 규칙이 필요하다.
- readback 실패는 쓰기 실패 확정이 아니다. 실제 변경됐을 수 있으므로 자동 재시도하지 않고 outcome_unknown으로 남긴다.

### 3.4 무손실·드롭·late attach 계약이 부족하다 — P1

원본 기록을 우선한다는 방향은 동의한다. 다음은 별도로 명시해야 한다.

- 메모리 queue byte cap, 디스크 full/stall, durability watermark, frame parser backlog에 대한 정지/저하 정책.
- target가 이미 버린 데이터, transport loss, daemon loss, recorder loss, viewer 생략을 분리한 gap source.
- DLT MCNT는 순환·재연결·발행자 경계 때문에 손실량을 항상 정확히 증명하지 못한다. 검출 불가 범위는 unknown이다.
- 구독 필터가 일부 seq를 건너뛰어도 그것은 drop이 아니다. 모든 seq 불연속을 생략으로 표시하면 정상 필터도 loss처럼 보인다.
- late attach는 `snapshot at N + events after N`의 원자적 경계와 backlog cap·gap을 가져야 한다.
- 장비 timestamp가 없는 PSU와 unsynchronized DLT에 device→render를 실측했다고 주장하지 않는다. host-rx→render와 사건 감지 지연을 따로 낸다.

### 3.5 안전 승인·인터록을 실제 권한/작업 범위에 묶는 계약이 부족하다 — P0

Opus §5.6의 클라이언트 종류 자기선언과 CLI에 answer 함수가 없다는 사실은 viewer 권한의 강제 수단이 아니다.
현재 문서도 “최선”이라고 한계를 인정하지만, 승인 RPC가 보호된다는 보장은 더 좁혀야 한다.
인증된 role별 권한과 action/resource/profile hash·epoch·byte/value 한계·횟수·expiry에 묶인 grant를 정의한다.
`grant dbg.halt 10m`가 이름만 같은 다른 probe/target에도 적용돼서는 안 된다.
승인 후 장치 재연결·프로파일 변경·인자 변경은 grant를 무효화한다. 이미 허용된 동일 범위는 불필요하게 재질문하지 않는다.

플래시 중 OFF를 R4로 올리는 것만으로는 interlock 의미가 모호하다.
일반 OFF 금지/작업 취소 협상/사용자의 emergency override를 구분하고, override도 이미 전송된 flash를 되돌리지는 못함을 표시한다.
`ctx.interlocks.active()` 조회 뒤 실제 송신 전 상태가 바뀔 수 있어 같은 daemon의 operation lease로 검사와 실행을 연결해야 한다.
profile 한계는 clamp/거부 중 하나로 확정한다. 기본은 **거부**이며 clamp가 필요하면 requested/effective를 나눠 사용자가 명확히 선택하게 한다.

### 3.6 시나리오·origin·replay의 최신 계약 정합성이 부족하다 — P1

[CAN–UDS v0.2 §9.6](../_xteam/can-uds-isotp.md)는 origin enum을 agent/user/device/script로 유지하도록 제안한다.
Opus의 CR-3는 변경 요청임이 명시돼 있지만 `script:<run-id>`와 `core`를 이미 공통 사례의 기본값으로 사용한다.
통합 전에는 `origin=script`, `actor_id/run_id/purpose`를 별도 필드로 두는 안을 추천한다.
replay는 `original_origin`을 보존하되 재생 주체·simulated/replayed·별도 namespace를 명시한다.
과거 agent TX를 현재 agent가 물리 장치에 송신한 것처럼 표시하면 안 된다.

`pause`가 `wait_for` 타이머를 멈춰도 물리 DUT·ECU S3·SSH 명령·릴레이 pulse는 계속 진행한다.
UI pause와 scenario scheduling pause, 실제 장비 시간·안전 deadline을 구분한다.
`finally`도 추가 장비 write이므로 abort 상태의 lease·승인·인터록을 다시 통과해야 한다.
자동 재시도는 단계별 idempotency·현재 epoch·user 개입 확인 후에만 허용한다.

### 3.7 후보 누락보다 후보 분할이 중요하다 — P2

18개가 이미 충분하다. HMI 후보 추가는 내 R1보다 넓은 좋은 탐색이다.
새 후보를 억지로 늘리기보다 XCP/CCP, passive pcap/active SOME-IP, Saleae/sigrok, SSH/ADB를 분리해야 한다.
CH341·Bus Pirate·Raspberry Pi 계열은 구현 경로와 OS·전압 제약이 달라 FT232H의 성숙도로 함께 평가하지 않는다.
TRACE32는 pyOCD와 대상 MCU·벤더 프로세스·비용이 달라 Debug 도메인 안의 별도 capability/비용 경로로 둔다.

## 4. 더 나은 선택

### 4.1 최종 추천과 착수 순서

**기본 4개: PSU → DLT → SSH → Debug.**
IO는 실제 device-state 조회가 검증된 보드가 있고 점화/버튼 작업이 빈번하면 5순위로 승격한다.
XCP는 slave·symbol map/A2L·RAM write 권한이 있으면 별도의 5순위 후보다.
둘 다 미확인이라면 네 개로 끝낸다. 다섯 개를 채우기 위해 부분 루프를 상위에 넣지 않는다.
이는 내 R1의 “조건부 XCP를 5번째 목록에 포함”보다 착수 gate를 분명히 한 판단이다.

| 안 | 핵심 루프 가치 | 단순성 | 구현 비용 | HW 확보 | Windows/macOS | 기존 3팀과 경계 | 추천 |
|---|---|---|---|---|---|---|---|
| Opus: DLT 1위+자체 codec | 현재 DLT 업무와 직접 연결 | parser/control을 함께 새로 개발 | 숨은 비용 중간 이상 | native daemon 있어 유리 | pure TCP는 양 OS 유리 | 독립 도메인 | native control gate 통과 후 채택, 1위 승격 가능 |
| PSU 단일 프로파일부터 | 설정→실측→user OFF 명확 | sim property와 bounded query로 시작 | 낮음~중간 | sim 가능, 실제 모델 질문 필요 | LAN 우선 양 OS | UART/UDS와 장비 소유 분리 | 기본 첫 확장 |
| Opus: Shell 통합+공유 PTY exec | live 개입은 좋음 | transport는 닮지만 shell 의미는 복잡 | sentinel/프롬프트/RC 파싱 비용 | SSH 타깃 미확인 | 인증·PTY·adb driver별 | UART 전체 engine 변경 의존 | SSH named exec+PTY로 축소 |
| Debug 4 backend 동시 계획 | 관찰·쓰기 가치 큼 | 가장 복잡 | SDK·MCU·ELF·RTT 조합 비용 큼 | 실제 probe/MCU 미확인 | vendor 지원별 | 독립, console만 재사용 | QEMU+한 probe부터 |
| IO top 5 기본 채택 | 사람 손 없는 점화/버튼에 가치 | 한 모델이면 단순 | 다수 driver면 증가 | 모델/feedback 미확인 | HID/serial/Modbus별 | PSU/UDS와 interlock 접점 | readback 모델 확인 후 승격 |
| XCP 조건부 5위 | 측정과 calibration 연결이 강함 | polling 두 symbol부터 가능 | DAQ/A2L/CAN까지 넓히면 큼 | slave·권한이 주요 gate | Ethernet 먼저 | CAN owner 우회 금지 | 사용자 작업 근거 있을 때 IO와 비교 |

DLT와 PSU 동점에서 사용자의 현재 DLT 작업을 우선한 Opus의 이유는 충분히 타당하다.
내 추천이 PSU를 먼저 두는 이유는 순수 codec 성공과 달리 작은 PSU backend set/query가 이미 검증됐기 때문이다.
DLT native set-level+효력 관측이 먼저 통과하면 DLT를 1위로 바꿔도 전체 아키텍처에 손해가 없다.

### 4.2 플러그인 계약은 작게 고정하고 UI는 선언형을 채택한다

| 항목 | Opus 안 | 더 나은 통합안 | 효과 |
|---|---|---|---|
| 등록 | backend/rpc/event/cli/panel/sim_actor/profile/interlock/skill 전부 Registry | 필수는 domain/backend/command/event/schema-version, 나머지는 선택 capability | 첫 plugin 도입 비용 감소 |
| Backend | open/rx/poll/write/health | deadline/cancel/close state·partial outcome 추가, push/poll은 선택 protocol | shell/blocking SDK/SCPI 모두 수용 |
| UI | PanelSpec+actions | 그대로 채택하되 모든 action을 인증·lease·정책으로 연결 | 매번 UI를 새로 안 만듦 |
| 데이터 | metrics 평면 숫자 map | metrics+unit+quality+source_seq+clock, 원시 payload 유지 | 간편 플롯과 근거 추적 동시 확보 |
| 큰 데이터 | 64 KB 이상 blob_ref | encoded envelope 전체 상한, immutable artifact ID/hash/length/형식 | base64 팽창·경로 오해 방지 |
| source identity | channel 이름 중심 | backend resource identity+serial+epoch+profile hash | 재연결 후 오래된 write 차단 |
| 안전 registry | R0~R5 신규 공통화 | 의미 기반 action policy부터 합의, 기존 등급은 매핑 | UART/CAN/UDS 등급 재작성 최소화 |
| 배포 | domain extra 최신 라이브러리 나열 | 실제 resolver lock과 플랫폼별 extra, vendor SDK 선택 설치 | F2 같은 조합 실패 방지 |

신뢰하는 Python plugin이 backend I/O를 마음대로 실행하면 registry만으로 우회를 물리적으로 막지는 못한다.
플러그인 API 규약·trusted package 목록·기록 경로 검증으로 보장 범위를 설명한다.
외부 seed-key/SDK worker 격리도 같은 resource lease와 stream을 지키는 경우만 허용한다.

### 4.3 SimWorld와 교차 기능은 두 번째 단계로 둔다

Opus의 공유 가상 DUT는 내 R1보다 명확한 장점이다. 독립 simulator만 있으면 전원 OFF 뒤에도 CAN/DLT가 계속 흐르는 가짜 데모가 될 수 있다.
다만 모든 도메인 M1이 거대한 SimWorld와 scenario runner를 선행 의존하면 걷는 뼈대가 커진다.

1. domain M1: 작은 stateful sim + 동일 daemon stream + user intervention + bounded read.
2. 실제 backend gate: DLT native control, SSH 채널, QEMU GDB/한 probe, SCPI 모델별 transport 확인.
3. 두세 domain 이후: 가상 시간·DUT state·fault seed를 공유하는 작은 SimWorld adapter를 연결한다.
4. runner M1: 유한 call/wait/assert/sleep/manual과 user abort만 먼저. parallel·재시도·복구 정책은 후속이다.
5. 통합 timeline은 clock source/uncertainty·reconnect epoch가 표시되는 상태에서 domain lane을 합친다.

core에 runner를 두자는 방향은 동의하지만 “독립 runner 모듈이면 교차 시나리오 불가”라는 인과는 약하다.
모듈 이름보다 상주 job ownership·동일 RPC/policy·동일 event stream이 중요하다.
UDS 전용 시나리오를 강제로 폐기하지 말고 core runner의 `uds.*` adapter를 제공하는 방식으로 요청한다.

### 4.4 HW/OS에 대한 현실적인 기본값

사용자 답이 없을 때 Windows bench PC·J-Link·8ch HID relay의 보유를 가정하지 않는다.
기본은 macOS local simulation이며 실제 모델 확인 후 물리 backend를 고른다.
LAN SCPI·TCP DLT·SSH는 USB driver 영향을 줄일 수 있지만 실제 타깃 접근권한까지 자동 해결하지는 않는다.
Windows device host는 localhost daemon+tunnel이라는 방향에 동의한다. 양 host 인증·시간·권한·재연결은 별도 M2 계약이다.
PLIN/Vector XL의 Windows-only 제한은 명확히 남기며 macOS ctypes로 해결된다고 표현하지 않는다.
PyFtdi Windows는 단순 Zadig 절차 완료만으로 공식 지원이 되지 않는다. [공식 설치 문서](https://eblot.github.io/pyftdi/installation.html)는 Windows를 공식 미지원으로 설명한다.

## 5. 합의 가능한 부분

1. **도메인 5개에 실제 사용자 개입을 구체적으로 넣은 점**은 그대로 채택한다. 특히 PSU user OFF가 agent wait를 interrupted로 반환하는 시나리오는 내 초안보다 명확하다.
2. **DLT를 현재 사용자 맥락의 강한 후보로 둔 점**에 동의한다. APID/CTID 단위 레벨 변경, 표준 `.dlt` export, 비verbose 후순위는 좋은 경계다.
3. **벤치 프로파일의 사용자 소유·해시·한계값·채널 별칭**을 채택한다. 다만 적용된 프로파일 변경 승인과 OS 파일 접근 강제는 구분한다.
4. **선언형 PanelSpec+actions**는 내 R1의 panel 요구보다 구체적이다. trace/gauge/state/timeseries와 사용자 RPC action을 연결하는 안을 우선 채택한다.
5. **metrics 자동 플롯**을 채택한다. 단위·source seq·stale/gap quality를 덧붙이면 다른 도메인에도 쓰기 좋다.
6. **SimWorld**는 내 R1보다 교차 도메인의 인과관계를 잘 설명한다. domain M1 이후 작은 공유 fixture로 도입한다.
7. **PSU set vs meas, 되읽기 속도의 한계, timeout gap**을 사용자에게 보여주는 방향에 동의한다. 100 ms라는 고정 수치만 모델별 근거로 바꾼다.
8. **debug MMIO read도 부작용이 있을 수 있음, ELF hash, non-atomic read, HALTED 구간**을 다룬 점이 좋다.
9. **AURIX/RH850 질문과 TRACE32 대안**은 Cortex-M 중심인 내 R1보다 사용자 MCU 다양성을 잘 반영했다. M1의 backend 수만 좁히면 된다.
10. **원격 bench를 SSH tunnel로 연결하며 기본 IPC를 local로 유지**하는 제안은 재사용 가치가 있다.
11. **replay가 장비 쓰기를 자동 재현하지 않음**, 파일 분석을 별도 상위 도메인으로 추천하지 않음에 동의한다.
12. **IO safe_state 기본 keep**은 무조건 OFF보다 신중하다. user emergency OFF와 자동 정리 정책을 분리한다.
13. **HMI 영상/터치 후보 추가**, DMM의 암전류 분해능 구분은 내 R1보다 넓은 탐색이므로 후보표에 남긴다.
14. **라이브러리 버전 1.x 이상 여부만으로 채택을 결정하지는 말되**, metadata 날짜·license를 표에 직접 적은 방식은 유지한다.

## 6. 핵심 원칙 점검

아래 충족은 **계획 내용의 충족**이며 실제 제품 시험 통과가 아니다.
R1 지시는 요약→기준표 구조이므로 R1에 a 표가 있다는 점을 인정한다. 최종 plan.md에는 맨 앞의 별도 핵심 루프 표를 여전히 넣어야 한다.

| 02-core-principle 6개 항목 | DLT §4.1 | PSU §4.2 | Shell §4.3 | Debug §4.4 | IO §4.5 |
|---|---|---|---|---|---|
| 1. 네 요소의 구체 구현 표 | 충족 | 충족 | 충족 | 충족, F1 전제 | **부분**: readback 미지원까지 top 범위 |
| 2. agent→device→화면→user 개입 E2E | 충족, ACK/효력 구분 필요 | 충족, user OFF 시나리오 좋음 | 충족, exec/PTY 중재 필요 | 충족, halt 승인만이 아닌 실제 user resume 있음 | 부분: 램프 변경과 실제 device 읽기 응답을 분리해야 함 |
| 3. 사건→화면 지연+고부하 정직성 | 부분: TX 의도 지연·기록 무손실 보장 수정 | **부분**: 1 Hz/500 ms 모순 | 부분: UART 정책 인용 외 SSH별 backpressure 부족 | 부분: sample→render와 device→render 구분, RTT high-load 부족 | 부분: 명령→램프 기준, flood/pulse 큐 한계 없음 |
| 4. M1=sim 네 요소 걷는 뼈대 | 충족하는 계획, native gate 추가 | 부분: SimWorld+플롯 의존 축소 | 충족하는 계획, 실제 SSH path 포함 필요 | 충족하는 계획, fake만으로 probe 검증 주장 금지 | 부분: command-cache-only 모드는 M1 통과로 치지 않음 |
| 5. 같은 세션/late viewer/사용자 개입 | 부분: late attach snapshot 경계 없음 | 부분: shared queue는 있지만 late attach 없음 | 부분: PTY 공유는 있으나 재연결 epoch/탭 snapshot 없음 | 부분: 등록 stream 외 late attach 계약 없음 | 부분: 같은 stream이나 snapshot/writer lease 부족 |
| 6. 루프 외 기능 후순위/비범위 | 충족, injection도 후속 축소 권장 | 충족, DMM/scope 후속 | 충족, push/pull/ADB 후속 | 부분: M1의 RTT/write/halt/복수 backend 폭이 큼 | 부분: driver 3~4종을 한 M1에 넣지 않기 |

### 공통 수용 기준 C1~C5

- **C1 동일 데이터:** 단일 owner의 agent TX·device RX에 session/epoch/seq를 붙이고 viewer와 bounded CLI가 같은 원시 참조를 읽는다.
- **C2 late attach:** agent 작업 중 두 번째 viewer가 붙어 snapshot watermark 뒤 live를 받는다. 중복/누락과 user 개입을 시험한다.
- **C3 user override:** user 변경 뒤 agent pending write가 취소되거나 명시적 충돌을 반환한다. agent가 그 이벤트를 다음 read에서 확인한다.
- **C4 지연 계측:** host-rx→실제 renderer 반영 시각을 측정한다. device clock이 없으면 device→render는 추정/미확인으로 남긴다.
- **C5 부하/실패:** acquisition/record/view 각각의 queue 상한, gap source, disk stall/full, disconnect·reconnect epoch를 주입한다.

### 도메인별 현실적인 지연·부하 수용안

| 도메인 | 정상 profile의 제안 목표 | 부하·실패에서 확인할 것 | 현재 증거의 범위 |
|---|---|---|---|
| DLT | host-rx→render p95≤150 ms, 1k msg/s | 10k msg/s에서는 원시 수신 건수/bytes·recorder·UI 생략 분리, user level 요청이 큐에 묻히지 않음 | V1 codec 왕복만 확인; native control 처리량 미확인 |
| PSU | scalar 5 Hz부터, 수신→render p95≤100 ms; 사건 지연은 poll+RTT+render | 느린 query·timeout·spontaneous trip·user OFF 중재, 실제 sample age | YAML get/set 확인. 동적 물리 부하/실 HW RTT 미확인 |
| SSH | 수신→render p95≤100 ms, 짧은 named exec | stdout/stderr 폭주 중 user PTY 입력, 키 불일치, partial UTF-8, 서버 재시작 | local SSH 1connection/2exec live output 확인 |
| Debug | 4 scalar×20 Hz부터, sample 수신→render p95≤250 ms | watch 지연·RTT overflow unknown·target HALT·stale·user write, cache source 표시 | QEMU halted SRAM 왕복 및 pyOCD import/옵션 확인; SWD 실측 없음 |
| IO | 실제 device report 수신→render p95≤100 ms | readback 없는 상태는 unknown/추정; pulse cancel·daemon 종료·reconnect 후 출력 상태 | Modbus TCP 왕복만 있음; 실제 HID/RTU/접점 미검증 |

이 표의 지연값도 M1 제안이다. R1의 VISA simulator query 약 0.01 ms를 physical SCPI RTT에 대입하지 않는다.
SWD clock·SEGGER vendor peak throughput을 변수 watch Hz나 화면 지연 보장으로 환산하지 않는다.
IO의 토글 ≤2회/s와 최소 20 ms pulse는 동작 횟수 제한인지 전기 edge 제한인지 정의해야 공존할 수 있다.
짧은 pulse가 GUI에 100 ms 뒤 보이는 것과 실제 pulse 폭을 장비가 정확히 구현하는 것도 다른 수용 항목이다.

### M1 범위 조정

- DLT: V1 TCP·get-info/set-level·trace·user level만. injection/serial/persist/all-level은 후속이다.
- PSU: stateful simulated PSU 한 모델, 작은 숫자/trace+user OFF부터. 동적 교차 장비 부하와 고급 플롯은 후속이다.
- SSH: local SSH server+named exec+PTY 한 경로. ADB/telnet/TCP 전부를 완료 조건으로 묶지 않는다.
- Debug: sim RAM read/write와 실제 GDB adapter 최소 경로를 선택하고 한 probe 지원을 후속 gate로 둔다. RTT full control-block 구현은 별도다.
- IO: 지원할 한 보드 capability와 실제 device-state 응답을 먼저 고른다. readback 없는 모드는 제어 부가기능이지 loop 완료 판정 경로가 아니다.

## 7. 쟁점 목록

| 쟁점 | Opus 안 | Sol R1 안 | 내 R2 추천 | 근거 |
|---|---|---|---|---|
| **최종 상위 3~5개와 순서** | DLT→PSU→Shell→Debug→IO | PSU→DLT→SSH→Debug→조건부 XCP | **기본 PSU→DLT→SSH→Debug 4개. 다섯째 IO/XCP는 소유 장비·권한 gate 후 선택** | partial loop를 숫자 채우기 위해 상위에 넣지 않음 |
| DLT 1위 여부 | 사용자 기존 업무로 즉시 1위 | PSU 먼저, DLT native control gate | native set-level/효력 gate가 먼저 통과하면 DLT 1위 허용 | 업무 근거는 Opus가 강함, 구현 확실성은 아직 미확인 |
| DLT codec | 자체 구현, pydlt는 dev 교차검증 | pydlt codec/기존 wrapper 검토 | pydlt codec 재사용+작은 live/control adapter부터 비교, 자체 구현은 필요한 차이만 | 0.x라는 이유만으로 parser 전체 재작성은 비용 근거 부족 |
| IO top gate | 읽기 △여도 추천 | feedback/readback 제약으로 후보 | 실제 device query 있는 모델은 ○ 가능, cache-only는 제외 | 장비 읽기와 접점 물리 확인을 구분 |
| XCP/CCP | 하나의 후보, 낮은 순위 | 조건부 XCP 5위 | XCP·CCP 분리; XCP는 slave+map+권한 확보 시만 | pyXCP는 CCP 지원 근거가 아님 |
| SSH exec | 공유 PTY+sentinel 기본 | named exec+PTY 분리 | Sol 경로+Opus 자동 탭 | 한 connection의 다채널 live output 시험 통과 |
| Debug 연결 | attach 무영향 기본, 옵션 미상 | safe memory map 중심 | API attach/auto_unlock/cache/disconnect 옵션 고정, probe별 smoke | 실제 옵션은 halt/auto-unlock 기본 |
| Debug dependency | 최신 pyOCD+PyLink 나열 | 충돌·1.7.0 baseline 명시 | lock 공존 조합 또는 환경 분리 | resolver 재현 |
| Simulator 구조 | 각 domain M1부터 SimWorld 연동 | 독립 sim 우선 | 독립 M1→작은 공유 SimWorld adapter | Opus의 인과 모델은 채택, 초기 범위는 축소 |
| Timeline/runner | core/viz 기능, E3 조기 | 두 domain M1 이후 | 상주 job+동일 RPC, pause/abort 의미 확정 뒤 core로 | module 귀속보다 세션·시간·정책 계약 중요 |
| TX visibility | 방송 먼저면 송신 전 사용자가 봄 | queued/sent/ACK/효력 분리 | intent 먼저 표시, 실제 송신 상태는 별도 | enqueue≠render≠wire completion |
| origin/replay | script:<run-id>, core, 과거 origin 유지 | enum 유지+별도 ID | 최신 xteam v0.2처럼 enum 유지, original_origin 분리 | 타팀 이벤트 호환 |
| 한계 초과 | clamp/거부 표현 혼재 | 거부 | 기본 거부, clamp는 명시 기능+effective value | 요청과 다른 출력의 조용한 실행 방지 |
| 승인/grant | viewer 자기선언, method 10m | authenticated role+digest 범위 | 자원/epoch/내용/한계/횟수/TTL 바인딩 | 이미 허용된 범위는 재사용, 무관 타깃 승인 전이 금지 |
| 확인할 viewer 없음 | R3+ 실패, 모든 skill도 먼저 열어달라 요청 | late attach 가능·승인 정책 | 실행 가능한 local viewer는 자동 attach 제공, 신규 승인이 필요한 경우에만 구체 action으로 요청 | core live 원칙과 불필요한 사용자 중단을 함께 줄임 |
| HW/OS 기본값 | Windows bench+J-Link+8ch HID 가정 | macOS sim 우선 | 미보유 가정, 모델 확인 뒤 physical profile 선택 | 사용자 실제 보유 정보 미확인 |
| CAN XL/LIN 귀속 | CAN 범위/확장 흡수 | 귀속 변경 요청 | 현재 CAN 비범위 유지, explicit 후속 요청만 | 다른 팀 범위 임의 확대 금지 |
| E0 착수 | 기존 M1과 병행 문서+spec | 기존 M1 이후 통합 | 문서/사실 확인은 지금, product 통합은 core/M1 gate 뒤 | 기존 세 팀 critical path 보호 |

R3 통합 우선순위는 **F1 안전 기본값 → TX/권한/IO 읽기 gate → F2 의존성·F3 형식·F5 지연 → M1 축소/동일 stream 수용 → 나머지 근거 정리**다.
최종 plan.md는 각 도메인의 simulation 네 요소와 user intervention을 실제 수용하기 전 구현 완료로 표시하지 않는다.

<!-- END R2 -->
