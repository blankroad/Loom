# CAN v2.0 — Rust 드라이버·프로토콜·상한 설계 조사

작성: can-sol, 2026-11-10. 대상: `plan.md` v1.2의 바뀌는 절. **조사·설계 제안**이며 제품 구현 완료가 아니다. `02-core-principle.md`와 마스터 C1–C8을 적용한다. 수치는 **실측 / 소스 확인 / 추정 / 목표 / 미확인**으로 구분한다. 기준 문서: [마스터 §2.3](../00-master-plan.md), [v2 작업](../04-v2-tasks.md), [현재 계획](plan.md), [ISO-TP 합의](../_xteam/can-uds-isotp.md), CORE/UI/TEST brief.

## 1. 리드가 반영할 변경

| v1.2 절 | v2 변경 | 유지할 계약 |
|---|---|---|
| §3·§7·§8 장치 경로 | Python Bus/Notifier 제거. Rust 장치 owner + bounded dispatcher + 전용 OS 스레드. Windows vendor별 worker 분리 | 장치 1 owner/수신 reader 1개, gateway/worker 분리, 원본 ID와 전달 seq 분리 |
| §7·§8.1 Vector | CAN 소유 `loom-vector-xl-sys` + 안전 wrapper. 공개 `vxlapi-sys`가 **존재하고 FD도 구현**하므로 C4의 부재 전제는 사실 정정. 자체 바인딩 소유 결정은 유지 | VN5640 CAN FD·원시 Ethernet 1급, CANoe 공존 시험 |
| §8.1 PCAN | 공식 PCAN-Basic 5.1 Rust FFI 실재 확인. Windows x64 우선, Linux `socketcan` FD | M2 첫 PCAN-USB FD 시험부터 FD64/BRS/ESI·타임스탬프 |
| §8.4 Linux | `socketcan::nl` 읽기 전용 façade | **사람이 netdev 사전 설정 + Loom 검증**. root helper/자동 bitrate 변경 없음 |
| §15.2 ISO-TP | CAN 소유 Rust 상태기계·전용 실행 스레드로 대체. 기존 crate는 비교·독립 검증용 | 공개 `can.isotp.*`, PDU IPC·예약 ID·abort/barrier·원시 FF/CF/FC 공개 |
| §6·§7 포맷 | 정본 저널은 CORE Rust 기록기. BLF/ASC는 Rust streaming exporter 후보가 실제 존재 | D7 원본 저널 정본, 표준 포맷은 export, 손실·origin sidecar |
| §4·§5·§6.4·§12 | 웹 셸·패널 프레임워크·viz를 UI 팀에 이관. CAN은 패널 명세와 gateway 질의 모듈 | 에이전트 읽기/쓰기와 사람의 같은 스트림 관찰·개입 |
| §8.2 CANoe | 기본 진단 경로에서 제외, P4 선택 worker. Rust COM 대안이 있어 Python 예외는 근거 재검토 | 진단 기본은 자체 CAN-TP/DoIP. 자동 CANoe 우선 선택 폐기 |
| §9·§11 | M1 = Rust 두 프로세스 + virtual + **웹 UI**. C7 증거 인용, C8 짧은 스킬 | D2/D3/D4/D5/D6/D7, A1–A16, B1/B4/B6 |

안전 의미를 언어 변경에 끼워 바꾸지 않는다. **vehicle=bench 규칙**, vehicle 상시 배지만 다르다. STOP은 agent/script 새 쓰기와 **모든 잡**을 latch하고 사용자 1회 개입은 허용한다. 예약 ISO-TP tx_id는 사용자 raw 1회도 takeover 전 차단한다. 기록/감사 장애(A12)는 sim 포함 agent/script R1+를 막고 사용자 쓰기는 degraded 표시와 함께 허용한다. 빈 allowlist를 기본 거부로 바꾸지 않는다.

## 2. crate 선택표

확인일 2026-11-10. 버전·게시일은 공식 sparse registry의 non-yanked stable 최신값(예외 별도 표기), 라이선스는 해당 배포 archive의 `Cargo.toml`을 확인했다. [원자료](../../../spikes/can/rust/crates-evidence.json)에 checksum을 남겼다. “최근 게시”는 유지보수 품질 보증이 아니다. OS 표의 W/L은 Windows/Linux이며 실제 장비 검증과 구분한다. 표의 채택은 **계획상 선택**이고 probe가 실제 빌드한 외부 crate는 crossbeam 두 개뿐이다.

### 2.1 드라이버·런타임

| crate/SDK | 버전 / 최근 게시 | 라이선스·유지 신호 | W / L, FD | 판단 |
|---|---|---|---|---|
| [socketcan](https://docs.rs/crate/socketcan/4.0.0) | 4.0.0 / 2026-09-05 | MIT, 최근 게시, MSRV1.89 | — / ✓, FD✓ | **채택**. Linux raw/FD socket·timestamp·netlink 검증. 기본 async wrapper 대신 RX 전용 thread |
| [rtnetlink](https://docs.rs/crate/rtnetlink/0.24.0) | 0.24.0 / 2026-11-04 | MIT, 최근 게시 | — / ✓, CAN 속성 해석은 별도 | 보류. `socketcan::nl`로 부족할 때만 read-only 확장; tokio/netlink 중복 의존 회피 |
| [netlink-packet-route](https://docs.rs/crate/netlink-packet-route/0.34.0) | 0.34.0 / 2026-11-03 | MIT, 최근 게시 | — / ✓, CAN link attrs | rtnetlink 하부 대안. 최초 구현은 socketcan 내장 getter로 충분한지 실기 확인 |
| [PCAN-Basic 공식 Rust FFI](https://www.peak-system.com/support/software-information/development-kits/pcan-basic/) | SDK5.1.0.1194 / 공식5.1.0 이력2026-07-01 | PEAK EULA, vendor 관리 | ✓ / 이 Rust sample의 Linux 검증 없음, FD✓ | **채택**. crates.io 공식 crate라는 뜻은 아님. 패키지 내 `.rs` 모듈을 안전 wrapper로 감쌈 |
| [peak-can-sys](https://docs.rs/crate/peak-can-sys/0.2.0) | 0.2.0 / 2025-12-19 | MIT OR Apache-2.0, 커뮤니티 | W/L FFI, FD 심화검증 안 함 | 공식 PEAK FFI가 있으므로 보류. 자체 이름 변경·헤더 배포 관련 README 주장도 vendor 권리 검증을 대신하지 않음 |
| [vxlapi-sys](https://docs.rs/crate/vxlapi-sys/2.1.0) | 2.1.0 / 2026-08-12 | MIT OR Apache-2.0, 최근 게시 | W DLL / L 장치 지원 아님, FD✓ | **참고·비교**. 자체 CAN 소유 바인딩 유지; 오류/event·공존·Ethernet 부족은 §3 |
| [libloading](https://docs.rs/crate/libloading/0.9.0) | 0.9.0 / 2025-11-05 | ISC, MSRV1.88 | ✓ / ✓, FD 해당 없음 | **채택** Vector runtime load. PCAN 공식 import-lib 모델 변경이 필요할 때만 보조 |
| [bindgen](https://docs.rs/crate/bindgen/0.73.2) | 0.73.2 / 2026-09-08 | BSD-3-Clause, 최근 게시 | 생성 host/target 설정 필요 | SDK 보유 CI에서 생성·ABI 검증용. 사용자 빌드마다 libclang 요구하지 않음 |
| [tokio](https://docs.rs/crate/tokio/1.54.1) | 1.54.1 / 2026-10-21 | MIT, 최근 게시 | ✓ / ✓, FD 해당 없음 | gateway WS/RPC/IPC 대기·감독에 채택(CORE 소유). FC/주기 deadline을 일반 async task와 공유하지 않음 |
| [crossbeam-channel](https://docs.rs/crate/crossbeam-channel/0.5.17) | 0.5.17 / 2026-09-05 | MIT OR Apache-2.0, 최근 게시 | ✓ / ✓, macOS✓ | **채택**, 모든 생성은 `bounded`. probe 빌드·실행 성공 |
| [thread-priority](https://docs.rs/crate/thread-priority/3.1.1) | 3.1.1 / 2026-06-20 | MIT, MSRV1.85 | W/L/mac API, 권한별 실패 가능 | 선택 채택. best-effort 상승 결과를 capability에 표시; 실패를 성공으로 숨기지 않음 |
| [windows](https://docs.rs/crate/windows/0.62.2) | 0.62.2 / 2025-10-06 | MIT OR Apache-2.0, Microsoft | W / — | Windows notification/timer/COM. 필요한 Win32 feature만 |
| [smoltcp](https://docs.rs/crate/smoltcp/0.14.0) | 0.14.0 / 2026-08-17 | 0BSD, MSRV1.91 | OS 독립, raw Device 구현 필요 | VN5640 사용자 공간 TCP 경로 **실현성 후보**, §7. CAN FD crate 아님 |

toolchain 제안: workspace Rust **1.93.1** 고정(로컬 실측), 최소치로 낮추려면 smoltcp1.91·socketcan1.89 등의 MSRV와 CORE 선택을 함께 확인한다. 제품 dependency lock·checksum·전이 라이선스 검사는 CORE 배포 관문에 포함한다.

### 2.2 ISO-TP·DBC

| crate | 버전 / 최근 게시 | 라이선스·유지 | W / L, FD | 판단 |
|---|---|---|---|---|
| [automotive](https://docs.rs/crate/automotive/0.6.0) | 0.6.0 / 2026-11-06 | MIT, 최근 게시 | adapter에 따라 W/L, ISO-TP CC/FD 구현 | 기능이 없어서 탈락하는 것이 아님. async send/recv와 Loom owner·barrier·fault 관측 의미 차이 때문에 **참조/oracle**, 전체 stack 채택 보류 |
| [can-iso-tp](https://docs.rs/crate/can-iso-tp/0.2.0) | 0.2.0 / 2026-03-17 | MIT OR Apache-2.0, 0.x | OS 독립 traits, FD backend 조건 | 고정 RX slice·polling 상태기계가 가장 가까운 대안. 그래도 Loom 최종 send 관문·bus ACK·rx_start/barrier 어댑터 필요. 비교 시험 후 코드 재사용 가능하나 제품 엔진 선택은 자체 상태기계 |
| [ecu_diagnostics](https://docs.rs/crate/ecu_diagnostics/0.107.5) | 0.107.5 / 2026-08-21 | **GPL-3.0-only**, 최근 게시 | W/L backend, 전체 FD 미확인 | UDS/session까지 포괄해 CAN/UDS 소유 경계와 중복. core 연결 채택 보류; 라이선스 적합성 미결. `IsoTPSettings`만으로 FD·abort 계약 충족이라 판단하지 않음 |
| [isotp-rs](https://docs.rs/crate/isotp-rs/0.2.1) | 0.2.1 / 2024-10-25 | GPL-3.0, **조회된 모든 버전 yanked** | OS 추상화, can-fd feature 이력 | 신규 dependency로 채택 안 함. 단순 “최신0.2.1 안정판 사용” 추천 금지 |
| [socketcan-isotp](https://docs.rs/crate/socketcan-isotp/1.0.2) | 1.0.2 / 2023-06-28 | BSD-3-Clause, 게시 간격 김 | — / ✓, kernel·옵션 조건 FD | Linux kernel PDU 경로 비교용. Windows 공통 stack과 FF 시점 노출·fault hook 요구를 대신하지 못함 |
| [can-dbc](https://docs.rs/crate/can-dbc/10.0.0) | 10.0.0 / 2026-07-15 | MIT OR Apache-2.0, MSRV1.83 | OS 독립, DBC FD payload 기술 가능 | **파서 채택**. runtime encode/decode는 별도. 기본/확장 mux·value table·attrs AST 확인 |
| [dbc-codegen](https://docs.rs/crate/dbc-codegen/0.4.0) | 0.4.0 / 2026-09-22 | MIT OR Apache-2.0, MSRV1.88 | OS 독립, 생성된 message별 검증 | DBC를 사용 중 교체하는 IG/CLI의 주 엔진으로 부적합. fixture의 독립 비교·고정 DB 최적화 후보 |
| [can_decode](https://docs.rs/crate/can_decode/0.7.2) | 0.7.2 / 2026-05-27 | MIT, 최근 게시 | OS 독립, 모든 FD/mux 조합 미검증 | runtime codec 대안. endian/signed/float/enum 지원 소스 확인. 아래 전체 보장 행렬을 통과하기 전 채택 보류 |
| [canparse](https://docs.rs/crate/canparse/0.1.4) | 0.1.4 / 2019-07-29 | MIT/Apache-2.0, 게시 오래됨 | OS 독립, FD 완성도 미확인 | 기본 파서 대안 조사까지. can-dbc 대비 신규 도입 이유 부족 |

DBC 결정: `can-dbc` AST → CAN 소유 immutable compiled database → **runtime codec 직접 구현**. 모토롤라 sawtooth/Intel, signed 1..64bit, IEEE32/64, scale/offset, min/max·choice, DLC0..64, mux selector+범위/중첩, attrs를 한 IR에 담고 encode 전 fresh/base-last/mux/revision·원자 patch를 집행한다. DB별 compiled IR32 MiB·동시 활성 DB4개를 초기 상한으로 제안하고 gateway 예산은 CORE와 합의한다. 단순 bitshift 엔진만 만들고 검증을 생략하지 않는다. 잘못된 mux/범위/누락 필수 신호는 쓰기 전에 명시 오류다.

근거: `can-dbc10.0.0`의 `src/ast/extended_multiplex.rs:46–92`에 `SG_MUL_VAL_` 단일/범위/복수 범위 fixture가 있고 README에 value_tables·attribute_definitions/defaults/values·value_descriptions가 명시된다. 반면 `sigtype_attr_list`·일부 deprecated 항목은 미구현이다. 공백 변형·`VAL_` 세미콜론 내성은 확인했지만 “임의 vendor 비표준 전부 지원”은 아니다. 미지원 문법은 위치·원문 요약으로 경고하고 쓰기에 영향을 주면 fail closed. 파일 인코딩은 명시 옵션, 자동 추측 결과는 표시한다. 파서 크기 목표16 MiB·message50k·signal500k·문자열64 KiB·mux 깊이16을 제한하고 필요 시 importer 별도 프로세스로 분리한다(수치는 목표). cantools/실차 공개 fixture와 read-only differential 비교는 개발 검증용이며 제품 Python 의존은 제거한다.

### 2.3 로그 포맷

| Rust 후보 | 버전 / 최근 게시 | 라이선스·유지 | W/L·FD·R/W 확인 범위 | v2 결정 |
|---|---|---|---|---|
| [autors-blf](https://docs.rs/crate/autors-blf/0.1.1) | 0.1.1 / 2026-08-03 | Apache-2.0, 신규0.x | portable 소스, CAN/FD object·streaming R/W | **M2 export 우선 후보 채택**, 외부 fixture/빌드 관문 전 제품 확정 아님. 없다는 전제 폐기 |
| [autors-asc](https://docs.rs/crate/autors-asc/0.1.1) | 0.1.1 / 2026-08-03 | Apache-2.0, 신규0.x | CAN/FD/RTR/error, absolute/relative·hex/dec R/W | M2 adapter 후보. CANoe 변형·FD 열·날짜/시간대 fixture 필수 |
| TRC 독립 crate | 적합한 maintained release **이번 조사에서 미확인** | 해당 없음 | [Rust cantraceviewer](https://github.com/tomrford/cantraceviewer) parser 사례 존재; 부재 증명 아님 | M3 자체 작은 line reader/writer, [PEAK 공식 사양 진입점](https://www.peak-system.fr/en/support/documentation) 기준 CC1.3·FD2.1부터. 3.0 XL은 제외 |
| [asammdf](https://docs.rs/crate/asammdf/0.1.0) | 0.1.0 / 2024-07-01 | Apache-2.0 | MDF3/4.1 R/W 표방, README **production 미준비** | M4 보류, MDF signal 지원과 CAN bus logging schema를 혼동하지 않음 |
| [mdfr / mdfr-core](https://github.com/ratal/mdfr) | main Cargo0.8.3, 2026-11-10 조회; crates.io 배포는 미확인(`publish=false`) | GPL-3.0, 최근 repo 활동 | Rust MDF3/4 R/W·4.3 streaming CAN/FD logger 표방 | “Rust MF4 writer 없음”은 틀림. 라이선스·커밋 고정·실제 도구 호환·빌드 심사 전 보류. Python feature 기본 켜짐도 제외 필요 |
| [mdf4](https://docs.rs/crate/mdf4/0.0.1) | 0.0.1 / 2022-12-09 | MIT, 오래된 소규모 구현 | 헤더 식별 수준 소스; 완성 bus logger 아님 | 미채택. `mdf0.2.0`은 MD5 CLI이므로 ASAM 후보가 아님 |
| [parquet / arrow](https://docs.rs/crate/parquet/60.1.0) | 60.1.0 / 2026-10-28 | Apache-2.0, MSRV1.88 | OS 독립; FD는 column schema로 보존 | seal 후 변환 worker에만. live CAN DLL worker에 Arrow/DataFrame 전체를 넣지 않음 |
| [zstd](https://docs.rs/crate/zstd/0.14.3) | 0.14.3 / 2026-10-16 | BSD-3-Clause, 최근 게시 | OS 독립 wrapper, native dependency 검토 | CORE seal 후 압축 선택과 정렬. RX thread 압축 금지 |

BLF 소스 확인: [`src/stream.rs`](https://docs.rs/crate/autors-blf/0.1.1/source/src/stream.rs)에는 `BlfReader/BlfWriter`, reader limits, writer container 크기 설정이 있다. `BlfFile::parse_reader`는 **전체 object 메모리 수집**, `write()`도 Vec를 만드므로 live/대파일 경로에서 쓰지 않는다. 기본 reader container512 MiB/object256 MiB·writer4 MiB를 그대로 쓰면 C2 예산을 침범한다. 변환 worker에서 header64 KiB/container8 MiB/object1 MiB 상한과 writer256 KiB를 **명시 설정**하고, 큰 합법 객체는 unsupported-limit 오류로 보고한다(값은 제품 목표). exporter는 seal된 저널을 iterator로 읽고 seek 가능한 임시 파일에 finish/헤더 갱신 후 원자 rename한다. 비정상 종료는 `.partial`과 source 범위를 남겨 재생성한다. 정본은 BLF가 아니므로 exporter 장애로 캡처를 막지 않는다.

M2 호환 관문: CC11/29·RTR·error·FD64/BRS/ESI·방향·복수 채널·정확한 시간 단위·자정/시간대·container 경계·truncate를 CANoe/PCAN-View와 독립 reader로 비교한다. 자기 writer-reader 왕복만으로 통과하지 않는다. origin/승인/원본 seq/불확실 시각은 sidecar로 보존하며 reader가 건너뛴 unknown object 수도 보고한다. MF4는 위 후보 검증 전 optional 미지원으로 명시하고 Python asammdf를 숨은 필수 의존으로 남기지 않는다.

## 3. Windows FFI와 Linux netdev의 구현 경계

### 3.1 PEAK 공식 모듈 — 확인됨

[공식 버전 이력](https://www.peak-system.com/support/software-information/development-kits/pcan-basic/)의 5.1.0 추가 사항과 [제품 다운로드](https://www.peak-system.com/products/software/development-packages/pcan-basic/)를 대조했다. 실제 `PCAN-Basic.zip` ReadMe는 **5.1.0.1194**, 내부 `Samples/Samples.zip/Console/Rust/01_LookUpChannel/src/PCANBasic.rs`에 `unsafe extern "system"`(416행), `InitializeFD`(782), `ReadFD`(921), `WriteFD`(989)가 있다. Rust 예제 9종에는 event receive·thread receive·trace가 포함된다. 정확한 URL·ZIP SHA256·내부 경로는 [source-checks.json](../../../spikes/can/rust/source-checks.json)에 기록했다. Windows x64/ARM64 지원 문구를 Rust sample ARM64 빌드 통과로 확대하지 않는다.

채택 범위: 공식 FFI의 ABI/상수/FD 구조체 → `PcanDevice` RAII 안전 wrapper. 채널 열기·FD bitrate 문자열·event handle·drain read·status·error text·GetValue timestamp/장치 식별·WriteFD·종료를 구현한다. `ReadFD`의 vendor 시각과 host ingest를 별도 기록하며 단위·wrap·clock mapping을 시험한다. `WriteFD` 성공은 기본 `driver_accepted`; 공식 API와 실기에서 상관 가능한 TX completion을 확인하기 전 `bus_confirmed`를 만들지 않는다.

공식 예제는 import library/build.rs 방식이므로 **PCAN worker만 링크/로드**, gateway 실행을 DLL 존재에 묶지 않는다. 배포 DLL 누락은 worker capability unavailable로 처리한다. 선택 runtime loading이 필요하면 동일 공식 ABI 선언을 `libloading`으로 감싸되 새로운 비공식 함수·레이아웃을 추측하지 않는다. vendor sample/헤더는 EULA이며 ReadMe는 원래 PEAK/OEM HW 연결에 사용을 제한한다. OSS 라이선스로 재분류하거나 SDK 전체를 repo에 복사하지 않는다. wrapper 배포 전 필요한 `.rs`/import lib 재배포 권한을 확인한다. 이번 조사본에는 ZIP/DLL을 남기지 않는다.

### 3.2 Vector — 공개 구현을 확인한 뒤 자체 crate를 소유

공식 [XL Driver Library 제품 문서](https://www.vector.com/en/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/)는 `vxlapi.h`, x86/x64 DLL, manual, C/C++ samples가 SDK 구성물이라고 명시한다. 최신 페이지 표시는26.30.16(2026-07-23), 확인한 [공식 manual](https://www.vector.com/dam/jcr:37a98de5-a2ca-4333-b6c7-ead8b088c937/XL_Driver_Library_Manual_EN.2026-09-12-16-40-28.pdf)은26.30이다. SDK header 재배포/생성 선언의 배포 허용 범위는 **미확인**. 사용자의 라이선스 보유가 공개 저장소 재배포 허가를 자동 의미하지 않는다. 로컬/CI 설치 SDK 경로와 버전·해시를 기록하고 vendor header/DLL은 commit하지 않는 구조를 제안한다.

`vxlapi-sys2.1.0`는 단순 classic-only가 아니다. [`src/api.rs`](https://docs.rs/crate/vxlapi-sys/2.1.0/source/src/api.rs)의46–77·126–128행에 세 FD 함수,741–775에 FD TX/RX,844–863에 TX_OK·`timeStampSync` 해석이 있다. 604–615행은 permission=0 shared open을 구현한다. 다만 **FD 설정이면 `wants_init_access()`가 true**(579–582), receive는 chip state/error/timer를 건너뛰고(758–774), activate는 `XL_ACTIVATE_RESET_CLOCK`(695–699)을 전달한다. `xlEth*`/`xlNetEth*`는 없다. 그러므로 “공개 crate 없음” 정정과 별개로 Loom 전용 wrapper가 필요하다.

| 자체 crate 범위 | 반드시 할 일 / 검증 |
|---|---|
| 일반 lifecycle | `xlOpenDriver/CloseDriver`, 장치·serial·channel mask 열거, `xlOpenPort/ClosePort`, notification handle, activate/deactivate, status 코드 보존. 한 owner만 receive |
| CAN FD | `xlCanFdSetConfiguration`, `xlCanTransmitEx`의 accepted count, `xlCanReceive`, V4 interface·packed event union·DLC ladder·11/29bit·FD/BRS/ESI·RX/error/chipstate/TX_OK/TX_ERROR/overrun 처리 |
| shared/configure | **shared는 permissionMask 입력0**, FD port를 열어도 init 요청0을 유지. 실제 timing 조회와 expected 비교; 공유 모드에서 설정·타임스탬프 reset·다른 앱 flush 없음. configure만 init grant 확인 후 timing 설정. 권한을 못 받았다고 자동 강탈/재설정하지 않음 |
| 시각 | raw timestamp·단위·clock_id·quality·host_ingest 저장, sync mapping은 별도. TX accepted와 wire/TX_OK 분리. 다른 앱 TXACK는 자기 요청 완료로 매칭하지 않음 |
| channel Ethernet | manual §7.3의 **`xlEthSetConfig`, `xlEthGetConfig`, `xlEthSetBypass`, `xlEthTransmit`, `xlEthReceive`, `xlEthRequestChannelStatus`**. transmit은 raw packet 제출, receive는 동기 event dequeue. status/error/TXACK/lostevent 보존 |
| network Ethernet | §7.2의 **`xlNetEthOpenNetwork`, `xlNetOpenVirtualPort`/`xlNetAddVirtualPort`, `xlNetConnectMeasurementPoint`, `xlNetSetNotification`, `xlNetActivateNetwork`, `xlNetEthSend`, `xlNetEthReceive`, `xlNetDeactivateNetwork`, `xlNetCloseNetwork`**. 사전 구성된 network/port를 식별 |
| ABI | `repr(C)`/packed/union·u64 XLaccess·Windows LLP64·calling convention 검증. SDK C harness와 Rust의 size/alignment/offset·event tag를 x64 CI에서 비교. `libloading::Library`는 모든 handle보다 오래 생존 |

**중요한 API 정정:** VN5640은 manual §7.1.3상 channel/network 둘 다 지원한다. network-based에서는 `xlNet*`, channel-based에서 `xlEth*`를 쓰며 **두 API handle을 섞지 않는다**(§7.2.3). task의 `xlEth*`만 구현하면 network-based 장비를 지원했다고 할 수 없다. CAN FD와 Ethernet을 별도 capability로 노출하고 Ethernet mode를 open 결과에 명시한다.

bindgen vs handwritten: 초기 **생성한 raw 선언을 SDK C ABI와 비교**하고, vendor 권리 검토 뒤 재현 가능한 generated 산출물 또는 최소 수기 선언을 선택한다. build-time allowlist로 CAN/FD와 선택 Ethernet만 포함한다. 손 선언은 범위가 작아도 ABI drift 위험이 있어 C layout probe를 생략할 이유가 아니다. public crate의 MIT/Apache 선언을 참고할 수 있지만 vendor 자료의 재배포 권리는 별도다. CAN 소유 wrapper/테스트와 CORE 공통 IPC의 경계는 유지한다.

### 3.3 pyvxl 교차 검증

[cmcerove/pyvxl](https://github.com/cmcerove/pyvxl) commit `d45cca4353f61bd867782b7242bfe1b1053265e9` 확인: `src/make.bat:27–28`은 Python32bit 검사, `vector.py:41–44`는 `vxlapi.dll`,385행은32bit mask 처리이며 `xlCanFd*`/`xlEth*`는 이 공개 구현에서 찾지 못했다. Python2.7·win32 설치 의존이 있어 **이 공개판은32bit CAN 경로라는 판단을 지지**한다. 사용자 private fork까지 같은 기능이라고 단정하지 않는다.

비교 항목은 classic11/29·DLC·RX count·channel mask·open/init permission·드라이버 시각 단위·공존 시 오류다. FD64/ESI/TX ACK·Ethernet의 oracle은 이 공개 pyvxl이 아니라 **공식 C SDK sample + CANoe 관측**으로 삼는다. reference 도구는 별도 개발/실험 환경에서 사용하며 Rust 제품에 Python adapter로 포함하지 않는다.

### 3.4 Linux 읽기 전용 netlink

[`socketcan4.0.0/src/nl/mod.rs`](https://docs.rs/crate/socketcan/4.0.0/source/src/nl/mod.rs)의 `CanInterface::details`(897), `can_params`(1105), `bit_timing`(1198)·data timing·ctrl mode·restart getter로 실효값을 읽는다. wrapper에는 **getter만** 노출한다. bitrate/dbitrate/FD·MTU/state/restart-ms를 사람 설정값과 비교하고 mismatch는 `E_USAGE` exit2, DOWN은 `E_NOT_OPEN` exit7, 미노출 속성은 unknown으로 구분한다. desired configuration이 필수 검증값이면 unknown을 일치로 간주하지 않는다. 안내에는 사람이 실행할 `ip` 예를 표시하되 실행하지 않는다. open 직전/재연결 때 재검증한다.

timestamp는 Python baseline의 제한을 그대로 옮기지 않는다. [`src/socket.rs:239–261,612`](https://docs.rs/crate/socketcan/4.0.0/source/src/socket.rs)의 timestamp API와 `timestamp.rs`에서 SW/RAW_HARDWARE 지원을 확인했다. **crate 지원 ≠ 해당 PCAN/커널의 HW timestamp 제공**이다. 없는 HW timestamp는 null, SW timestamp·host ingest를 구분하며 HW 품질을 추정으로 올리지 않는다.

## 4. ISO-TP 엔진 결정과 스레드

**결론: 제품 엔진은 CAN 소유 Rust 상태기계로 직접 구현한다.** “Rust crate가 없다”가 근거가 아니다. wire codec/BS/STmin은 기존 구현에도 있으나, Loom에서 중요한 최종 전송 관문·단일 reader·두 종류 TX 완료·abort 후 잔여·drain barrier·원시 프레임 증거를 일관되게 소유해야 한다. `automotive`/`can-iso-tp`와 differential fixture를 공유하고 검증된 작은 부품을 도입할 수 있다. 새 stack 비용을 일정에 넣으며 단순 rewrite만으로 표준 적합성 통과를 주장하지 않는다.

| 요구 | 기존 소스 확인 / 부족한 증거 | 자체 엔진의 관문 |
|---|---|---|
| normal11/29·CC/FD·padding·BS/STmin·FC WAIT/OVFLW | automotive0.6 `transports/isotp/mod.rs:57–104`에FD64·STmin encoding·bounded batch, can-iso-tp0.2 `pdu.rs`에 FD SF와 polling states | SF/FF/CF/FC·SN wrap·길이/DLC·BS0/8·STmin0/1..127ms/F1..F9·WFTmax·N_BS/N_CR·padding0xCC/null·maxPDU4095 fixture |
| 요청/소유/예약 ID | 라이브러리 transport API에 Loom gateway actor/lease/정책 위임 계약은 없음 | `open/send/recv/cancel/abort/barrier/close/list/open_group/set_params` 이름·응답 필드 유지. worker가 owner/epoch/idempotency와 모든 raw/IG/periodic/replay 예약 ID를 집행 |
| tx_state·에러 | generic adapter send 완료를 버스 ACK로 취급할 위험. backend별 검증 필요 | 모든 프레임 accepted만이면 `driver_accepted`, 자기 transfer의 필요한 모든 TX completion을 확인해야 `bus_confirmed`. ACK 없는 backend는 승격 금지. ECU 긍정 응답에 의한 UDS `write_state=confirmed`는 UDS가 소유 |
| rx_start·barrier | full-PDU callback만으로 P2 판정 이전 FF 관측을 보장 못 함 | FF ingest 직후 priority `rx_start`; deadline까지의 transport 처리 **및 event 발행 완료** covered seq/time를 확인하는 유한 barrier. raw queue drain만으로 complete 반환 금지 |
| abort/cancel | future drop만으로 이미 driver에 제출된 CF 취소를 증명 못 함 | epoch/generation 확인→최종 send latch→미제출 CF·FC 제거→rx/tx state clear. `residual_tx_possible`·aborted 수 보고, 늦은 completion을 이전 transfer에 격리 |
| 결함 훅 | can-iso-tp의 timing/clock 추상화는 비교에 유용. 모든 요구 주입 API는 미확인 | virtual only: wrong SN/길이·dropCF·FC WAIT/overflow/지연·bus-off·late PDU. 실HW fault는 승인·지원 capability 아래만 |

`can.isotp.open`의 default/필드·functional SF 범위·server reverse ID·unknown responder 자동 FC 금지·TesterPresent UDS 소유·UDS 위임 시 이중 승인 금지를 유지한다. full addressing/group/set_params는 기존 M4 범위를 유지하되 **M2 PCAN raw FD를 뒤로 미루지 않는다**. FD TP의 SF/FF와 timing fixture는 M0부터 만들고 full 계약 관문은 UDS와 동기화한다. Rust API 명칭이 바뀌어도 wire method/enum/64bit JSON 문자열은 그대로다.

실행 모델: vendor RX thread는 normalize/sequence/고정 ring fanout만 하고 decode·serialize·압축하지 않는다. transport+최종 TX arbiter 전용 thread는 ISO-TP state와 FD/CC TX·주기 heap을 함께 소유한다. FC/만료·STOP/사용자 개입이 먼저이고 주기 job은 bounded 시간만 처리한다. vendor send는 nonblocking enqueue 또는 짧은 timeout을 쓰며 DLL call이 멎으면 worker watchdog이 종료한다. 최소 길이 단위 작업만 진행하므로 큰 replay가 FC를 독점하지 않는다. 별도 journal writer·IPC pump가 bounded queue를 소비한다. 소수 control RPC는 tokio에서 이 스레드로 메시지를 전달한다. CPU decode는 gateway bounded pool, 파일 변환은 별도 worker다.

Windows는 notification HANDLE과 wakeable wait, deadline timer를 함께 사용한다. [Microsoft high-resolution waitable timer](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw)의 `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`은 Win10 1803+ 지원이며 스케줄러 실행 시각 보증이 아니다. thread priority는 above-normal 등의 best effort부터 시험하고 TIME_CRITICAL/realtime process를 기본으로 쓰지 않는다. Linux elevated policy는 권한 실패 가능, macOS 결과로 대체 금지. 지원 priority/timer와 fallback을 공개한다. 전역 timer resolution 변경/무한 busy-spin은 기본값 아님. 100µs STmin은 **최소 간격을 지켜야** 하지만 최대 지각·ECU timeout 적합성은 실기 검증 전 미확인이다.

## 5. C2 — 모든 큐·링·메모리 상한

다음은 **제품 목표/초기 추정**, 장비별 negotiation 후 조정한다. 단위는 MiB=2²⁰B. worker admission profile은 최대 CAN4채널·ISO endpoint32개·주기 job128개·Ethernet 활성 network1개다. 상한 초과 open/job은 accepted 전에 resource-exhausted 원인으로 거절하며 CORE 오류 코드에 매핑한다. `FrameSlot128B`는 이번 probe의 실측 layout일 뿐 D3 JSON 직렬화 layout이 아니다.

| 경로/저장 | 상한 목표 | 가득 찼을 때 정책(block/drop/spill) | 증거·카운터 |
|---|---|---|---|
| vendor/kernel→reader | Vector CAN RX queue8,192 요청(지원 크기 SDK 확인); PCAN vendor 고정값 조회 가능 시 게시, Linux socket SO_RCVBUF1 MiB 요청/실효값 조회 | HW RX에서 backpressure 보장 불가. 연속 drain, driver overrun 보고. 알 수 없는 개수는 unknown | `source.gap`, `source_overrun_total`, `source_loss_unknown_total`. 미관측 wire 손실0을 선언하지 않음 |
| normalize→공유 raw ring | **65,536×128B=8 MiB**, worker 합계 | 고정 overwrite. journal/transport에 필요한 항목이 밀리면 손실/해당 TP 오류; 단순 보존 창 만료 자체는 gap 아님. RX reader block 금지 | journal 미처리 원본 범위는 `record.gap`; 수신 후 유실은 driver `source.gap`으로 둔갑하지 않음. `ring_overrun_total`·consumer lag |
| ring→journal writer | 32,768 frame slots, payload4 MiB + metadata | `try_send`, 평상시 writer가 디스크로 spill. 큐 full 시 새 항목 drop 범위 계수·record unhealthy; **무한 RAM/무한 재시도 없음** | `record.gap`, `record_queue_full_total`, source seq 범위·count·last durable seq. A12 즉시 적용 |
| journal encode/write buffer | 고정 scratch2 MiB, BufWriter1 MiB, single in-flight chunk; chunk64 MiB/60s는 **디스크 파일** | disk error/full/flush deadline 초과→health degraded, 메모리 보관 확대 금지. rotation·압축은 seal 뒤 | `record.health`, `journal_write_error_total`, flush lag·durable watermark. quota 만료 파일 삭제와 미기록 loss 구분 |
| ring→gateway raw IPC | 8,192 frame slots≈1 MiB + **wire byte budget2 MiB**, 둘 중 먼저 | RX block 안 함. IPC lag 시 raw live drop, 저널이 정상일 때 source range 재조회 가능. 재연결 후 bounded catch-up 또는 live jump 명시 | `display.gap` + 원본 ID 범위(`scope=worker_live`), `ipc_live_drop_total`. 아직 할당 안 된 stream_seq 범위 조작 금지 |
| TP/error/state→gateway priority IPC | 고정256 entries, entry≤8 KiB, **총2 MiB** | raw와 분리. silent drop 금지. 잠깐 try/유한 wait 뒤 full이면 transport/event delivery unhealthy→관련 session 실패·barrier incomplete; supervisor에 fatal 상태 전달 | `priority_queue_full_total`, bounded reserved status latch에 최초 실패/누계·watermark. 알 수 없는 완료는 outcome_unknown |
| TX 일반 요청 | 1,024×256B=256 KiB, PDU는 별도 pool handle | enqueue 전 정책·epoch 확인, full은 **accepted 전에 거절**. 이미 accepted 요청은 deadline으로 terminal 실패; drop 성공 처리 금지 | `tx_rejected_total`, `tx_deadline_total`. 거절 자체는 source/record/display gap 아님 |
| 최종 FC/사용자/제어 예약 lane | FC64×128B=8 KiB, 사용자64×256B=16 KiB, STOP은 atomic latch+독립 wake | 새 agent 일반 요청이 예약 lane을 소비 못 함. FC full은 endpoint overflow/error; STOP latch는 queue 공간 불필요. 사용자 flood도 bounded 거절 | `fc_queue_full_total`, `user_tx_rejected_total`, STOP ack·residual count. 승인/기록 실패는 따로 처리 |
| periodic heap/patch | job128개 + 변경256개, 총256 KiB 예약 | append된 tick 큐 없음. missed tick skip/count, patch는 최신 revision CAS. job full 거절, patch full이면 rejected | `periodic_missed_total`, `periodic_lateness_ns`, patch revision. 잡 생략을 raw capture gap으로 세지 않음 |
| RX→ISO dispatcher | 4,096×128B=512 KiB, endpoint32 | `try_send`. full이면 해당 endpoint abort/error, 방치한 stale CF를 계속 소비하며 성공하지 않음 | `isotp_ingress_overflow_total`, `can.isotp_rx_error`; raw 저널 보존 시 record.gap 추가하지 않음 |
| ISO PDU pool·명령·완료 | PDU64slots×4 KiB=256 KiB(실효 max4095), TX/RX endpoint당 각1개 in-flight, command256×256B=64 KiB; 완료 pool도 총256 KiB | allocation admission, full 전 거절/FC overflow. ACK 없는 송신 성공 불가. 완료 소비가 느리면 bounded retention 뒤 session 오류/조회 불확실 표시 | `isotp_pool_exhausted_total`, `isotp_completion_overflow_total`, transfer/epoch. complete/barrier publish 보장과 연결 |
| Ethernet XL↔UDS raw IPC | RX/TX 각각4,096×2,048B=8 MiB, **합계16 MiB**. jumbo 미지원 | raw queue full은 affected TCP link degraded, 제한된 packet drop·재전송 관측. raw capture recording을 켰으면 별도 record gap. CAN priority lane과 자원 분리 | `eth_rx_drop_total`, `eth_tx_rejected_total`, driver lostevent는 source.gap. TCP retry를 CAN frame 손실과 혼합하지 않음 |
| gateway mirror/query→WS (CORE/UI 경계) | CORE 공통 mirror 고정 크기 협의, 구독별2 MiB·동시16개 초기 제안. query 응답≤1 MiB/page | decode pool·snapshot·재조회도 상한. 느린 viewer만 drop/disconnect, 캡처 reader block 금지 | 할당된 stream 범위의 `display.gap`, `subscriber_lag`, `query_truncated`; CORE/UI가 화면 생략 표시 |

링은 무기한 저널 대체가 아니다. 5k fps에서65,536개는 **약13.1초**,20k fps에서3.28초, journal queue32,768은 각각6.55/1.64초 완충이라는 산술 추정이다. ring에 오래된 event reference를 붙잡아두는 subscriber를 만들지 않는다. 고정 slot 복사 또는 bounded slab handle만 사용한다. crossbeam slot header/allocator 비용 때문에 위 payload 합계를 RSS라고 주장하지 않는다.

worker 메모리 산정(추정): CAN ring8 + journal payload/slot overhead8 + raw/control IPC8 + TX/scheduler/TP4 + serialization4 + thread stack reserve16 + vendor/user buffers24 + runtime/allocator/code pages32 = **약104 MiB**, Ethernet packet16+scratch/소켓 여유16을 더해 약136 MiB. vendor DLL의 실제 메모리는 미측정이므로 **정상 RSS 목표≤192 MiB, hard budget256 MiB/worker**로 여유를 둔다. max4 channels 초과는 새 worker/profile으로 분할하고 budget도 함께 늘려야 한다. Ethernet TCP stack을 UDS worker에 두면 그쪽 socket budget은 UDS가 별도로 책임진다.

감독자가1초 간격 RSS/queue high-water를 읽는다(목표).192 MiB 지속10초면 새 channel/job·추가 recording format·변환 admission을 중단하고 degraded 알림; **필수 정본 캡처를 메모리 확보용으로 조용히 끄지 않는다**.256 MiB 도달/계속 증가면 최종 TX latch→진행 요청 terminal/unknown 처리→가능한 flush와 상태 보고 후 worker 종료, 불응 시 supervisor terminate. OS 제한(Job Object/cgroup 사용 가능 시)은 마지막 방어이며 native driver kernel memory까지 통제한다고 주장하지 않는다. 순환 RSS 샘플·histogram bucket·ID stats map도 고정 상한; ID 캐시는 최대8,192 entry+other 집계, 요청 dedupe4,096·유한 TTL, trace 문자열/diagnostic text 길이 상한. exporter·DBC heap은 이 worker에 유입하지 않는다.

손실 이벤트 자체의 발행 큐가 가득 차는 문제는 reserved atomic counter·최초/최종 source seq latch로 보전하고 다음 health snapshot에 실어 보낸다. gateway가 죽어도 worker 저널은 지속한다. 저널 실패로 근거를 남길 수 없으면 `record.health=unhealthy`와 마지막 확정 watermark를 supervisor가 관측하고 **기록 정상으로 재표시하지 않는다**. gap marker도 복구 못 한 hard crash 구간은 unknown 범위다.

### 5.1 24h soak 관문 — 목표, 아직 실행 안 함

| 항목 | 부하·측정 방법 | 통과 목표 / 판정 한계 |
|---|---|---|
| 공통 C2 혼합 | CORE/TEST harness에서 CAN·UART flood·UDS polling 동시, Windows x64와 Linux x64 각24h. 최초30분 warmup 뒤 rolling RSS·CPU·queue depth·lag 수집 | 호스트 RAM·디스크·OS/driver/firmware·모든 부하 seed를 기록. macOS 단기 probe로 대체 불가 |
| CAN 기본 | virtual aggregate5k fps, CC/FD64 혼합 각50%,32개 TP endpoint 중8개 왕복256B/4095B 반복,128 periodic jobs는100ms 이상·aggregate≤1,280fps부터, raw+priority events·journal on | 실제 HW는 채널별 산정 bus utilization≤70%로 부하를 맞춤. 500k CC 한 버스에 임의5k FD64·128개10ms를 우겨 넣지 않음. virtual overload와 wire profile 분리 |
| burst/상호작용 | 매시간10분20k fps virtual burst, viewer4개·늦은 구독·query·확대·replay/export 별도 process, 사용자1회 송신·STOP 반복 | capture→journal 정상 구간 source.gap=0·record.gap=0, accepted 요청 terminal 상태 누락0. display.gap은 예산상 허용하되 정확히 표시 |
| 메모리 | warmup 후1분 RSS 중앙값·선형 slope·고수위, epoch별 분리; vendor 포함 private bytes도 W에서 병기 | CAN worker RSS≤192 MiB 정상, 절대256 MiB 초과0, **24h drift≤16 MiB 및 slope≤0.5 MiB/h** 모두. 고정 ring plateau·동적 map 해제 확인. 이는 추정 예산의 검증 관문 |
| 기록 | source seq를 deterministic 생성기/독립 수신기와 대조, manifest·chunk·seal·late gateway replay | 정상 구간 원본 record 손실0/중복0/오순서0. driver가 loss counter를 안 주면 source 무손실 판정은 independent oracle/알려진 생성 seq 없이는 미확인 |
| 지연 | host ingest→FC driver submit p99≤3ms, PDU complete→UDS process 수신 p99≤5ms,10ms periodic 절대 간격오차 p99<3ms **탐색 목표**, web≤150ms·UDS화면≤100ms p95는 기존 계획 계승 | raw·journal·UI 부하 on, p50/p99/max·sample count·missed 함께 보고. macOS 이번 periodic p99는 목표 **미달**. wire FC/CF는 별도 외부 timestamp로 검증 |
| 고의 장애 | bus-off 매2h, receiver disconnect·gateway restart·worker crash, disk full/250ms·2s·10s writer pause, 느린 viewer·priority saturation | 장애 창은 loss0 기준의 **별도 구간**. 기대 gap/ERROR/unknown·A12 latch·복구 명시·메모리 상한을 판정하며 손실을 정상으로 숨기지 않음 |
| 재시작/내구성 | SIGKILL/TerminateProcess, fsync 지연·chunk 중간 절단·전원 시험 분리 | v1.2 process RPO1s·seal/fsync 최대60s는 목표로 유지하고 CORE 구현 실측. journal tail 검증·이전 승인/잡 자동 재개0 |

“무손실0”은 정상 지원 부하+정상 저장장치+관측 가능한 입력 범위의 관문이다. 중단 불가능한 실제 버스와 유한 큐에서 무한 disk stall을 무손실로 처리할 수 없다. fault segment를 정상 통계에서 빼더라도 발생 시각/범위·손실/unknown은 보고서에 모두 남긴다.

## 6. 벤더 DLL 프로세스 격리

**`can-pcan-worker`와 `can-vector-worker` 분리**, Linux는 `can-socketcan-worker`, macOS는 `can-sim-worker`. vxlapi64.dll과 PCANBasic.dll은 각각 해당 worker만 로드한다. gateway/UI/CLI·UDS session manager에서 vendor DLL을 로드하지 않는다. 추가 프로세스 RSS/IPC 비용보다 한 vendor fault가 다른 장비·진단·저널까지 전파되지 않는 것이 C2에 부합한다. 기본은 물리 장치별1 worker(최대4 CAN channel profile), 동일 Vector 장치의 CAN/Ethernet은 driver lifecycle을 공유하되 둘의 crash domain이 같다는 것을 게시한다. 독립 fault가 필요하면 장치/worker를 더 분할하며 같은 물리 채널을 중복 owner로 열지 않는다.

| 상황 | 감독·재열기 규칙 |
|---|---|
| DLL load/version/symbol 실패 | 해당 backend unavailable, 누락 함수·DLL bitness/version 표시. 지원하지 않는 Ethernet symbol만으로 가능한 CAN까지 거짓 지원하지 않음. 전체 gateway 시작 실패로 만들지 않음 |
| heartbeat 정지/native call hang/crash | CORE supervisor가 해당 worker만 종료. producer_epoch/channel generation 증가, IPC stale reply 폐기. 마지막 durable source seq 이후는 unknown gap, 진행 TX/TP는 failed 또는 outcome_unknown(접수 전인지 여부로 구분) |
| 재기동 | 목표 backoff1/2/4/8/30초,5분 내3회 crash면 자동 재시작을 멈추고 failed 상태. 수치 CORE supervisor 정책과 합의. 프로세스 재시작과 채널 TX 재개는 다름 |
| 자동 reopen | policy가 허용한 read/listen/shared 채널만 사전설정·장치 identity 재검증 후 reopen 가능. 승인/lease/job/TP transfer·replay는 복원하지 않음. configure/timing 쓰기는 기존 policy를 다시 통과 |
| 공존/상태 | shared open 실패를 configure fallback으로 바꾸지 않음. CANoe에 init access 요청/clock reset을 강요하지 않음. PCAN custom app 공존 한도도 official API·실기로 별도 확인 |
| 저널 | writer는 worker native crash에 같이 중단될 수 있다. 이전 seal chunk는 보존·tail 복구, 다른 vendor worker journal은 계속. gateway 감독이 죽은 worker의 마지막 manifest/durable watermark를 수집 |

격리는 vendor kernel driver/USB stack/호스트 OS 장애까지 차단하지 않는다. worker마다 seq/manifest와 CORE supervision protocol을 공유하고 exit cause를 같은 UI 연결 상태에 표시한다.

## 7. VN5640 Ethernet 위 DoIP — UDS 합의 초안 재료

공식 XL26.30 §7.1.3–7.2.3, [Vector KB0015597](https://support.vector.com/kb/?id=kb_article_view&sysparm_article=KB0015597), [VN5000 공식 manual6.1 §2.1.3 p22](https://www.vector.com/dam/jcr:5c1b5484-bfbc-4cd3-8c3e-e4c7aafb44fd/VN5000_Manual_EN.2026-10-12-11-27-49.pdf)을 확인했다. 아래 지연은 경로 비교 **추정**, ms 수치는 실측 전 약속하지 않는다.

| 경로 | 사실 확인 / 남은 미확인 | 구현량·지연 예상 | 라이선스·실측 관문 |
|---|---|---|---|
| **(a) VN5640→Windows NIC→OS TCP/IP** | `Vector Hardware Config → Ethernet access mode configuration → Network-based mode`는 실제 기능(공식 XL p134–135, 화면 확인). Hardware Manager는 network-based 장비 구성에 사용. 하지만 이는 Vector network/virtual port 모델이며 **VN5640 vehicle port를 NDIS NIC로 노출한다는 공식 기능은 확인 못 함** | 가능하면 OS TCP/UDP 재사용으로 구현량 작음. OS route/adapter 존재부터 미확인이라 일정의 확정 전제로 삼지 않음 | 설치 driver/firmware에서 `GetAdaptersAddresses`/Windows 장치·route 확인, vendor 문서의 정확한 기능명과 CANoe 없이 socket 연결 증거 필요. host Ethernet uplink와 vehicle-facing NIC를 혼동하지 않음 |
| **(b) XL raw Ethernet→smoltcp→DoIP** | XL channel의 `xlEthTransmit/Receive`, network의 `xlNetEthSend/Receive` 확인. smoltcp0.14.0은 TCP client/server·UDP·IPv4/6·ARP 제공 | 세 경로 중 구현량/시험량 최대. custom `Device`·poll wake/deadline·MAC/IP/route·socket lifecycle·raw IPC·retransmission. OS socket 직접 경로보다 hop/스케줄링 지연 증가 가능, 수치는 미확인 | smoltcp0BSD; Vector SDK/runtime 사용 권리는 별도. XL lane·checksum/FCS·source MAC·MTU·TXACK/lostevent·network shared 권한·CANoe 공존·DoIP routing activation와 UDP discovery를 실기 |
| **(c) PC NIC→100/1000BASE-T1 media converter→ECU** | 일반 OS NIC TCP/UDP 사용 가능. VN5000 manual p22는 **VN5640 자체4개 독립 media converter**를 명시하므로 별도 converter 외에 VN5640 물리 변환 모드도 후보 | OS stack 재사용, 구현량 가장 작음. 변환 PHY·링크 지연은 존재하나 TCP 처리까지 자체 구현할 이유가 적음. 별도 HW/케이블/전원·T1 master/slave 설정 필요 | 실제100BASE-T1/1000BASE-T1 옵션·port pair·link/wakeup·DoIP activation·VLAN·ECU IP/서브넷 시험. 보유 VN5640 converter 설정 지속성·다른 앱과 충돌 확인. 새로운 제품 구매를 이 문서가 결정하지 않음 |

smoltcp 소스 범위: [`0.14.0 README`](https://docs.rs/crate/smoltcp/0.14.0/source/README.md)는 TCP window scaling·out-of-order reassembly·keepalive·재전송·delayed ACK·Nagle·선택 Reno/CUBIC을 설명한다. SACK/TCP timestamp/PLPMTU, Ethernet802.1Q/jumbo는 지원하지 않는다고 명시한다. 차량 VLAN이 필요하면 XL 경계에서 검증한 VLAN strip/tag metadata와 egress 삽입을 **명시 구현**해야 하고 자동 지원으로 표시하지 않는다. 이것은 최소 DoIP TCP를 만들 수 있다는 근거이며 완전한 OS TCP 동등성이나 DoIP conformance 증거는 아니다. TLS가 필요하면 별도 Rust TLS/UDS 범위와 bounded buffer 계약이 추가된다.

제안: **VN5640 raw Ethernet (b)의 L2 driver/queue 계층은 M2부터 검증하여 1급 유지**하고, DoIP 첫 실기 성공 경로는 입증된 PC NIC+(c)로 병행한다. (a)는 driver 확인 관문 전 확정하지 않는다. (c)를 쓴다고 VN5640 CAN/FD·raw Ethernet 작업을 optional로 내리지 않는다. (b) TCP/UDP socket abstraction/DoIP는 **UDS 소유**, XL raw API와 port/timestamp/packet metadata는 **CAN 소유**다. user-space TCP를 CAN device thread에 끼우지 않고 UDS transport actor와 bounded raw IPC로 연결하는 안을 우선 비교한다.

리드가 `_xteam/vn5640-eth.md`에 전달할 합의 항목: device serial+network/port/mode 식별, shared/init 권한, MAC/IP/VLAN 소유자, RX/TX packet 최대2,048B·queue budget, checksum/FCS 처리와 error packet 정책, UDP discovery/TCP stream API, link generation/late packet 격리, frame accepted와 TXACK·TCP ACK·DoIP ACK·ECU response 각각의 의미, TS quality, STOP/승인/DoIP activation line 쓰기 경계, (a) NIC 실재 확인 결과, (b)/(c) RTT·p99·loss·CPU·RSS 비교. 본 작업은 해당 `_xteam` 파일을 수정하지 않는다.

## 8. M0/M1 Rust 실현성과 관문

### 8.1 프로세스 간 virtual CAN

`can-sim-worker` 안에 bus owner를 두고 endpoint별 **로컬 stream socket**으로 Rust simulator/client를 연결한다(Unix domain socket:macOS/Linux, Windows named pipe; CORE 공통 IPC 재사용 가능하면 그것을 우선). same-process in-memory channel만으로 python-can virtual을 대체했다고 하지 않는다. 공유 메모리는 M0 기본에서 제외한다: crash recovery·원자 ordering·ABI·권한 비용이 소켓 serialization 절감보다 큰지 아직 근거가 없다.

message는 length-prefix+version+bounded frame payload, max record8 KiB, endpoint16개·각 outgoing8,192 frame/2 MiB 중 먼저, attach nonce·local peer 검증. owner가 `(channel, is_ext, id, dlc, bytes, fd/brs/esi, direction, origin, host_ingest, producer_epoch, source_seq)`를 정상화하고 producer origin을 외부 입력대로 믿지 않는다. 가상 ECU는 device origin을 서버가 부여한다. bus rate/연결 해제/loopback·필터·slow endpoint·fault seed·arbitration 모델 유무를 capability에 기록한다. 초기 immediate delivery는 wire timing simulator가 아니다. 사용자 개입·STOP에도 device SimNode는 계속 동작한다.

macOS는 gateway/CLI/WS+웹/저널·codec·ISO state machine·별도 프로세스 sim·fake CANoe·문서/계약 시험이 가능하다. Windows DLL 및 Linux AF_CAN/netlink 경로는 target 전용 cfg와 fake backend로 분리하며 macOS build에 SDK/DLL을 요구하지 않는다. `vcan`은 Linux netdev이므로 macOS virtual 대체라고 쓰지 않는다.

### 8.2 이번 실측과 남은 spike

[RESULTS](../../../spikes/can/rust/RESULTS.md)·[Rust source](../../../spikes/can/rust/probe/src/main.rs)·[원시 JSON](../../../spikes/can/rust/probe-result.json). rustc/cargo1.93.1, macOS aarch64, release, default priority. 외부 드라이버 없음.

| 실측 | 결과 | 해석 |
|---|---|---|
| 전용 thread10ms, idle1,000회 | 절대 간격오차 p99 **4.527333ms**, max 4.987417 ms, skip0; deadline 지각 p99 4.839209 ms | 잠정 p99<3ms 목표 미달. std sleep 기본으로 ±1ms 약속 금지 |
| 동일, CPU 경쟁1thread1,000회 | 간격오차 p99 **4.581000ms**, max 12.535250 ms, skip1; deadline 지각 max 16.452708 ms | 짧은 시험에서 tail/skip 존재. Windows·wire·FC 결과가 아님 |
| crossbeam bounded8,192×128B,100만개×5회 | **11.600–11.917 Mframe/s**, seq 손실0·재정렬0, 표본 latency p99 0.032833–0.067416 ms | blocking synthetic producer+single consumer, 디스크/직렬화/IPC 없음. 실제 RX 무손실 성능으로 확대 금지 |
| saturation/layout | 128B size assert, capacity8을 채운9번째 try_send Full, 기존8개 보존 | overflow 감지 확인. 실제 gap 이벤트 pipeline은 아직 구현 안 함 |

v1.2 Python FC p99≈52ms·주기 오차 부하 p99≈47–60ms와는 **다른 부하와 다른 계측점**이다. “Rust라서 GIL 병목 해결/FC3ms 달성”이라는 결론을 내리지 않는다. 다음 M0 측정은 동일 journal-on/20k fps·decode load를 별도 process로 둔 조건, FF ingest→FC submit·외부 wire, CC/FD BS0/8·STmin0/1ms/100µs, abort latency·STOP-before-FF·priority full·barrier timeout을 포함한다. IPC는 두 프로세스 PDU complete→UDS recv를 별도로 잰다.

### 8.3 수정된 마일스톤·팀 handoff

| 관문 | CAN 산출물 | 타팀 연결 / 실패 조건 |
|---|---|---|
| M0 | Rust trait·고정 queue/ring·sim socket 두 프로세스·codec fixtures·timer/FC/ABI spikes. Windows SDK 파일/권리 및 VN mode 확인 | CORE P0 schema/IPC stub에 연결. stub 문서만 있고 executable 없이 M1 착수 완료 선언 금지 |
| **M1** | Rust worker+gateway+CLI `send/trace/last/wait`·정본 journal·원자 snapshot+live·가짜 ECU. 에이전트가 보내고 사용자 웹에서 같은 raw/갱신값 관찰·직접1회 송신·STOP | **UI팀 최소 웹 Trace/상태/개입/STOP/기록 상태**가 필수. TUI만 있으면 미통과. echo 반환만 있고 journal/UI가 별도 데이터면 미통과 |
| M2 | Windows PCAN-USB **FD부터**, Linux SocketCAN FD; VN5640 CAN FD·공존·raw Ethernet 두 mode 확인; 최소 ISO-TP normal11/29·abort/barrier; BLF export 후보 외부 호환 | 별개 vendor crash+24h 혼합 soak 관문. UDS M1b용 최소 TP 일정 동기화. 실HW·FD 없이 M2완료 선언 금지 |
| M3/M4 | DoIP(b)/(c) 비교, full TP addressing/groups/fault·DBC/표준포맷 확장·장기 로그 질의 | UDS가 socket/DoIP, CAN이 XL L2·raw evidence. MF4 지원은 후보 관문 뒤 선택 |
| P4 | CANoe optional bridge·fake adapter | 진단 기본 경로의 선행조건 아님. §9 예외 기록/실기 후 지원 표시 |

CAN→UI 패널 명세에는 Trace frame/error/gap·timestamp quality·record health·fixed/chrono, Graphics signal quality/gap, IG revision/lease/latch/예약 ID, 신호 raw/physical/mux/DBC revision, 사용자1회송신·takeover·승인 상태를 포함한다. gateway CAN 질의 모듈은 seq/time query·last/snapshot·wait/assert·min/max buckets·paging을 제공한다. 사용자에게 렌더러 내부 crate/IPC 방식을 노출하지 않는다. UI/viz 구현 소유를 CAN 일정에 다시 배정하지 않는다.

C7/TEST 전달: 기존 `can wait`·`sig wait`/assert 계열 명령을 유한 timeout·expected/observed와 함께 registry에 등록하고, 결과에 원본 ID 범위·stream cursor·DBC revision·조건·판정시각·gap/ts quality를 붙인다. 정확한 CLI 동사는 CORE registry 확정명으로 통일하며 도메인 독자 TC runner를 만들지 않는다. TEST는 자연어 TC를 명령으로 실행하고 session·step·HTML/JUnit을 소유, CORE가 증거 존재·조건·seq 범위를 검증한다. 기록 gap을 가로지른 “미발생” 판정은 inconclusive여야 한다. session 사전승인은 승인된 ID/byte/rate/lease digest 안에서만 재사용하고 dry-run·최종 presend 검사·STOP을 우회하지 않는다. 자연어 TC마다 새 Python 테스트 코드를 만들지 않는다.

C8: CAN 스킬은 연결→`loom can help --json`→상태/짧은 조회→dry-run·쓰기→wait/assert 증거 인용으로 짧게 유지한다. 공통 승인/STOP/저널은 공통 loom 스킬, TC·보고는 loom-test 스킬에 둔다. static 긴 명령 목록·Python SDK 예제는 정본에서 제거하고 registry help가 변경 기준이다.

## 9. CANoe COM Python 예외에 대한 판단

**Rust로 COM이 불가능하다는 근거는 없다.** [`windows0.62.2`](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Com/struct.IDispatch.html)는 `GetIDsOfNames/Invoke`를 제공하고, 실제 배포 소스 `Win32/System/Com/mod.rs:4872–4876`에서 확인했다. STA `CoInitializeEx`, VARIANT/SAFEARRAY·BSTR·HRESULT 처리, late-binding DISPID cache, message pump, `IDispatch` sink 구현과 [IConnectionPoint Advise/Unadvise](https://learn.microsoft.com/en-us/windows/win32/com/using-iconnectionpoint)로 이벤트 연결이 가능하다. 문제는 CANoe 버전별 type library·outgoing IID/event signature·진단 객체 수명·busy retry·세대 전환의 구현/실기 비용이다. 그러므로 C1 “대안이 없을 때만 Python”에 대해 **현재 증거는 예외 조건 미충족**이다. C4의 P4 Python adapter 방향은 optional 위치로 유지하되 “불가능”이라는 허위 근거로 자동 승인하지 않는다. 결정 기록은 `Rust COM 기술적 대안 존재 / CANoe 설치 환경 spike 미실행 / Python 예외 미확정`으로 적는다. P4에서 Rust STA late-binding+signal read+진단 event sink+CANoe 재시작을3일 timebox(추정)로 시험하고 재현 가능한 blocker/vendor Python-only 제약이 입증되면 Python 별도 bounded worker 예외 사유를 남긴다. 비용/일정만 이유라면 C1 완화 결정이 필요하며 CAN 단독으로 정하지 않는다.

Python adapter를 최종 선택해도 동일 IPC·memory budget·supervision·policy decision_id·generation·late response·STOP 계약을 적용한다. 기존 [CANoe 합의](../_xteam/canoe-bridge.md)의 정정대로 `tx_state`와 UDS `write_state`를 구분한다. 자동 CANoe 선택·측정 시작·세션 중 backend 전환은 없다. bridge 부재가 VN5640/PCAN 자체 진단을 막지 않는다.

## 10. 미확인·리드 결정에 남길 항목

| 항목 | 현재 상태 | 닫는 증거 |
|---|---|---|
| 공식 PEAK Rust 지원 | **ZIP 내부 FD 함수까지 확인**, Windows build/실기는 미실행 | x64 sample+safe wrapper 빌드, FD loopback·독립 수신·event drain·timestamp |
| Vector 공개 crate 없음 | **사실 정정 필요**,2.1.0에 FD 구현 있음 | 자체 wrapper 선택 근거는 §3의 event/shared/Ethernet 요구로 대체 |
| Vector/PEAK SDK 재배포 | vendor EULA/SDK 권한의 정확한 소스·생성물 배포 범위 미확인 | SDK terms/사용자 계약에서 배포 범위 확인; 전체 SDK는 repo 제외 |
| VN5640 Windows NIC(a) | 실제 기능 미확인, network-based와 동의어 아님 | 공식 기능 문서+실제 adapter/route/socket 증거 |
| VN5640(b) TCP | 가능성은 있음, VLAN·IPC·공존·DoIP·tail latency 미실행 | 두 access mode raw packet 시험→UDS socket→DoIP fixture, CPU/RSS/RTT |
| Rust periodic/FC 목표 | periodic 단기 p99<3ms **미달**, FC 미측정 | Windows timer·priority·wire/journal-on 동조건 측정. 성능 SLA 확정 전 결과 공개 |
| BLF/ASC/MF4 | Rust 후보 존재, 생산 호환·license/transitive 심사·빌드 미통과 | 독립 tool fixture·stream bounds·fault recovery·bus logging metadata |
| 24h/C2 | 수치 목표와 완충 산술만 제안 | 2 OS 실HW+혼합 soak, loss 기준·fault 창·RSS slope 별도 보고 |
| CANoe Python | optional P4, C1 예외 근거 미충족 | Rust installed-CANoe spike 또는 lead의 명시 C1 예외 결정 |

출력 범위는 이 문서와 `spikes/can/rust/`로 한정했다. `plan.md`·타팀 문서·`_xteam`은 변경하지 않았다.
