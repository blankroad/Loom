# UDS v2 — Rust 이식·전송 조사 (2026-11-10)

권장: **Rust 자체 트랜잭션 러너 + 작은 서비스 코덱 + bounded tokio DoIP**. CAN-TP는 CAN worker의 `IsoTpPort`를 유지한다. 기본 경로에 Python·CANoe를 요구하지 않는다. 이 문서는 조사 보고서이며 `plan.md`는 수정하지 않았다.
02 핵심 루프와 C1–C8 > B > A > D 적용: Rust 장치 읽기·쓰기 → CORE 동일 이벤트 스트림 → M1 웹 UI 실시간 표시·사용자 개입까지가 완료 기준이다. echo 벤치마크만으로 통과하지 않는다. WS JSON-RPC·역할·승인·쓰기 상태·CLI exit·감사 계약은 유지, serde 정본에서 TS/Python 타입을 생성한다(C3). TC는 공통 `loom test`, 도움말/스킬은 C7·C8에 맞춘다.
조회 기준: 아래 버전·라이선스는 crates.io API, 최근 커밋은 GitHub API 실제 조회(UTC 일자, HEAD가 배포 버전과 같다는 뜻은 아님). 미확인은 추정으로 채우지 않았다.

## 1. UDS 코덱·러너

- **권장**: D2의 자체 러너는 유지하되 `udsoncan` 의존은 제거한다. `automotive_diag`는 SID/NRC·enum·상수 보조로만 사용하고, M1의 0x10/0x22/0x2E/0x19·raw 요청 조립과 길이·SID·DID/서브기능 에코 검증은 작은 순수 Rust 코덱으로 구현한다. raw는 승인된 바이트를 그대로 전송한다.
- **근거**: [automotive_diag](https://github.com/oxibus/automotive_diag/tree/6bed2f35f8833fb499493b89043bb136b2799899/src)는 정의 중심이지 udsoncan 서비스 코덱 전체 대체품은 아니다. [automotive UDS](https://github.com/I-CAN-hack/automotive/blob/96657f1f805ba4ab1ca413d5b3d308a5a7d52a16/src/protocols/uds/mod.rs)는 P2/P2*·0x78·총 타임아웃·응답 검증·SPRMIB를 갖춘 유용한 참조이나, 서비스 헬퍼와 상태를 소유하는 client가 결합되어 코덱만 떼는 이득이 작다.
- 자체 러너가 RPC 수신부터 큐 포함 총 deadline, 단일 in-flight/물리 ECU, epoch·abort·publish-before-reply를 소유한다. P2 첫 수신과 PDU 완료를 구분하고 CAN은 `rx_start` + `can.isotp.barrier`로 IPC 지연을 분리한다. 0x78은 재송신 없이 P2*만 연장하고 횟수·총 deadline을 제한한다.
- D19는 **SID 중첩(R0 포함)** 기준 U 격리, 별칭·SA 우회 금지, 0x78/미완성 PDU·정숙·프로브 성공으로 U 해제 금지를 유지한다. 완전한 최종 응답/기존 소켓 원천 차단/확인된 리셋/검증 OEM 상한만 회복 증거다. `late`를 새 요청의 `confirmed`로 만들지 않는다.
- SPRMIB 무응답은 실행 확인이 아닌 `ok_suppressed`; **0x78을 받았다면 최종 응답이 필요**하다(위 automotive 소스도 구현). `tx_state`와 ECU 긍정 응답 기반 `write_state`를 분리한다. `ecu_diagnostics`는 GPL·세션/재접속 러너까지 포함하므로 런타임 채택하지 않는다. `uds-rs`는 Linux ISO-TP 결합으로 공통 전송 코어에 부적합하다.
- **미확인**: Rust 후보의 D19·CORE 승인/저널 통합은 입증되지 않았다. 자체 러너의 NRC·타이머 경계·에코 전수 시험이 관문이며, 일정 실패 시 Python Client로 회귀하는 v1 D2 조건은 C1에 맞춰 Rust 범위 축소/일정 조정으로 바꿀 것을 제안한다.

## 2. DoIP

- **권장**: 엔티티+SA당 TCP 연결 하나, LA별 분배, 단일 수신 task, alive-check 우선 처리, ACK/NACK와 UDS 결과 구분, 요청별 남은 deadline을 자체 구현한다. UDP discovery/announcement(13400), 재활성화·세대 교체도 UDS 소유다. `tokio-rustls`는 인증서 체인·서버 이름 검증을 유지하는 선택 TLS 계층으로 쓴다.
- **근거**: [automotive_diag ActivationType](https://github.com/oxibus/automotive_diag/blob/6bed2f35f8833fb499493b89043bb136b2799899/src/doip/activation_type.rs)는 central security **0xE0**, [doip-definitions](https://github.com/samp-reston/doip-definitions/blob/36ab2310a1e93107b0c3bdf01576c213fe2a33ac/src/doip_payload/activation_type.rs)는 **0x02**다. 로컬 빌드에서 `doip-codec`가 0xE0 RA 요청을 거부했다. 그대로 채택하지 않는다.
- `doip-codec` STD 경로의 분할/연속 프레임 처리는 통과했지만 u32::MAX 길이 헤더를 오류 없이 대기했다. 헤더 검증·길이 상한을 **누적/할당 전** 적용하는 작은 자체 코덱을 권장한다. [doip-sockets](https://github.com/samp-reston/doip-sockets/tree/e28b8a944812a090b834ee691905635849e142a5)는 소켓 래퍼여서 Loom의 공유 연결·LA 분배·D19·alive/deadline 상태기를 대신하지 못한다.
- **미확인**: OEM central-security 추가 데이터·인증, 실제 TLS ECU/인증서, alive-check 폭주·잘린 헤더·재연결 중 늦은 응답은 HIL/가짜 엔티티 시험 필요. TLS 실패를 평문으로 자동 전환하지 않는다.

## 3. ISO-TP — CAN 팀에 줄 의견

- **권장**: UDS는 PDU RPC만 사용한다(D11). Windows Vector XL·공식 PCAN-Basic FD FFI 위 사용자 공간 TP는 CAN 팀 소유; Linux PCAN은 SocketCAN FD, 커널 `CAN_ISOTP` 우선 검토. FD가 M2 HIL 필수이며 CAN classic 통과로 대체하지 않는다(C5).
- **근거**: [Linux ISO-TP 문서](https://docs.kernel.org/networking/iso15765-2.html)는 CAN/CAN FD·LL 옵션·PDU 소켓 모델을 제공한다. mainline 5.10+ ISO-TP를 전제로 배포 커널 모듈/설정을 검사한다. `socketcan-isotp`는 Linux 커널 래퍼이고 완성 PDU read만으로 첫 FF 수신 시각을 얻었다고 볼 수 없어, CAN raw 관측·시퀀스/barrier 계약은 별도로 필요하다.
- `isotp-rs`는 모든 배포 버전 yanked·연결 저장소 404라 제외. `tokio-socketcan-isotp`는 MIT 파일 확인; [README](https://github.com/japawBlob/tokio-socketcan-isotp/tree/029dc98a6f9e9d6100bc8ef6ddcf4b1ff3c7d4ac)는 구형 커널의 WouldBlock busy-wait 위험과 6.6.5 확인을 명시한다. 최신 `can-hal-isotp`는 사용자 공간 후보로 CAN 팀에 전달하되 지금 채택 확정하지 않는다.
- **미확인**: 각 후보의 FD 길이·주소 확장·STmin/BS·취소·타임스탬프·최대 PDU·oversize/FC 폭주·Windows 동작. 사용자 공간 TP는 PCAN/XL 공통 CAN 추상화 뒤에 두고 UDS에 중복 구현하지 않는다.

## 4. VN5640 Ethernet 위 DoIP

근거는 Vector [VN5000 Hardware Manual v6.1](https://www.vector.com/dam/jcr:5c1b5484-bfbc-4cd3-8c3e-e4c7aafb44fd/VN5000_Manual_EN.2026-10-12-11-27-49.pdf) §2.1.3(p22), §14.2–14.4와 [OS 지원표(2026-05-26)](https://support.vector.com/sys_attachment.do?sys_id=6d90f0968349cb10e9c7d6126daad373)다. VN5640은 CAN/CAN FD 2채널, Ethernet, 독립형 미디어 컨버터 4개를 제공하며 1000BASE-T1은 HW 옵션이다.

| 경로 | 전제·위험·실측 항목 | 판단 |
|---|---|---|
| (a) VN → OS NIC → OS TCP | 공개 매뉴얼에서 VN5640의 NDIS/일반 NIC 노출 설정명·조건을 확인하지 못했다. `ETH16 Uplink`의 VHM **Ethernet Host**는 PC↔VN 측정 호스트 링크이며 OS NIC 노출 증거가 아니다. | 미확인, 기본 경로로 약속하지 않음 |
| (b) XL raw Ethernet → smoltcp → DoIP | Windows XL FFI raw-frame 소유는 CAN, TCP/IP·DoIP는 UDS. bounded frame 인터페이스·단일 poll 소유자·시계/링크 상태 계약 필요. USB/FFI/IPC 지터, poll 지연, RX 유실, MTU/VLAN·필터·체크섬·재조립 상한을 HIL 검증. | C4 직접 XL 경로 후보, 구현량 큼 |
| (c) PC NIC → 미디어 컨버터 → T1 ECU | **VN5640 자체 standalone converter**를 우선 후보로 사용: VHM 설정 후 **Deploy**, 외부 전원, 대응 포트/PHY master-slave·속도 설정. PC NIC의 OS TCP 사용. VN HW 옵션/포트 짝이 불충분하면 별도 컨버터. | 가장 짧은 기본 DoIP 경로로 조건부 권장 |

- (c)의 VN 자체 사용은 매뉴얼 기능에 근거한 **설계 추론**이며 미실측이다. 초기 VHM 설정·Deploy가 필요한 벤치 준비와 에이전트의 반복적 직접 open/read/write를 구분한다. 매뉴얼은 설정이 PC 연결 없이 유지됨을 명시한다. USB 측정 uplink와 ECU 데이터 포트를 혼동하지 않는다.
- [Vector 설치 안내 §4.2.25](https://support.vector.com/sys_attachment.do?sys_id=1552ea3a4751791069f16240f26d43fe&sysparm_viewer_id=9d52eaf64751791069f16240f26d43ee&sysparm_viewer_table=kb_knowledge)의 `NetworkDriver.msi`는 **PC NIC를 CANoe에 쓰는 드라이버**이며 VN→OS NIC 증거가 아니다. OS 표는 VN5640 Windows 10/11 x64 지원, Linux 칸 공란이다. Linux에서 XL 지원을 가정하지 않는다; (c)는 미리 설정된 독립형 변환기를 통과하는 PC NIC 경로라 Linux VN 드라이버 없이 가능할 것으로 보이나 HIL 필수다.
- [smoltcp 0.14 README](https://github.com/smoltcp-rs/smoltcp/blob/v0.14.0/README.md)는 ARP·Ethernet multicast/broadcast·UDP·TCP 재전송/backoff·윈도우 확장·재정렬·keepalive를 제공한다. DoIP 13400 발견/공지는 별도 구현이다. 기본 delayed ACK 10 ms·Nagle·poll 주기가 짧은 진단 RTT에 미치는 영향을 측정한다. README의 loopback 처리량은 VN/USB 성능 증거가 아니다.
- **미확인/관문**: Windows/Linux 실기 포트별 VIN·쓰기/되읽기→웹 UI, UDP 발견/멀티 ECU, link flap·ARP·TCP 재전송/윈도우, 100/1000T1 옵션, CAN FD 동시 부하, CANoe 공존·채널 소유, activation line 전압/제어. [VN5640 DAIO XL 안내](https://support.vector.com/kb?id=kb_article_view&sysparm_article=KB0012141)는 Windows Rust FFI 후보일 뿐 Linux standalone activation 제어 증거는 아니다. 자동 제어 불가 시 벤치 activation 전제 명시.
- `automotive` 저장소에는 현재 `vector-xl` 어댑터도 있어 마스터의 “공개 Rust crate 없음”은 재검토할 사실이다. 다만 CAN FD/raw Ethernet ABI·SDK/배포·안전성을 확인하지 않았으므로 **CAN 팀 소유 XL 바인딩 결정은 유지**하고 참고/교차 검증 후보로만 넘긴다.

## 5. CANoe 없는 seed-key

- **권장**: 알고리즘 실행은 항상 별도 Rust helper. `libloading`으로 제공 DLL 로드, 2 s watchdog·고정 입력/출력 상한·크래시/잘못된 반환 길이 거부, key는 이벤트/감사에서 마스킹한다. helper가 계산만 하고 승인된 0x27 송신·원자 구간·epoch는 러너가 통제한다. 실패 시 0 키 대체·자동 재송신 금지.
- **근거/시그니처**: [Vector SN-IND-1-040](https://support.vector.com/sys_attachment.do?sys_id=2163eca59750f9181245b1ece053af31&sysparm_viewer_id=4c03758d1b58f994afd085d2604bcb2d&sysparm_viewer_table=kb_knowledge)의 `GenerateKeyEx(seed:u8*, seed_len:u32, level:u32, variant:char*, key:u8*, capacity:u32, actual_len:u32&) → VKeyGenResultEx`; 모두 앞 입력 포인터는 const. `GenerateKeyExOpt`는 variant 뒤 `options:const char*`를 추가하고 Opt 반환 enum을 쓴다([Vector 작성 구 안내 §3.4](https://robertscaplblog.wordpress.com/wp-content/uploads/2016/04/sn-ind-1-003_securityaccess_canoe.pdf), [실제 구현자 선언](https://app.whyengineer.com/docs/um/uds/buildInScript/buildInScript.html)).
- Rust FFI는 `actual_len`을 out-pointer로, enum 크기·함수 export·x86 calling convention은 OEM 헤더/PE에서 검증한다. `extern C/system`을 무조건 고정하지 않는다. C++ mangling/exception이면 작은 C ABI shim을 helper에 둔다. 계산 DLL 자체에 CANoe 의존이 없다면 CANoe host는 필요 없다.
- [Microsoft](https://learn.microsoft.com/en-us/windows/win32/winprog64/process-interoperability): x64 프로세스는 x86 DLL을 로드할 수 없다. **별도 `i686-pc-windows-msvc` helper exe**가 필요하다([Rust 지원 타깃](https://doc.rust-lang.org/rustc/platform-support.html)); x64 DLL도 x64 helper로 격리(C2). Windows DLL을 Linux에서 네이티브 로드하는 경로는 제안하지 않는다.
- 사용자 알고리즘 우선순위: ① 버전 있는 bounded stdin/stdout JSON 외부 실행파일(Rust 우선, 언어 독립) ② 기존 C ABI DLL helper ③ 고객이 Python 구현만 제공하고 대체 구현이 없는 경우에 한해 명시적 외부 Python worker. in-process Rust dylib/entry-point 플러그인은 사용하지 않는다(C3). seed/key ≤4 KiB, 메시지 ≤64 KiB 제안.
- **미확인**: 실제 OEM x86/x64 DLL·MSVC 런타임·export ABI·의존 DLL·시간/RSS·crash 주입. macOS 조사에서 Windows FFI 성공을 주장하지 않는다.

## 6. 진단 DB

- **권장**: M1 자체 DB는 serde + 유지되는 YAML 후보 `serde-saphyr`, 형식/해석 한계가 분명한 스키마로 제한한다. 외부 include·환경 치환·사용자 tag 실행은 금지, 중복 키/alias·중첩·입력 크기 제한을 고정한다. `serde_yaml`은 deprecated라 신규 기본 의존으로 택하지 않는다.
- **ODX 근거**: `odx` 0.0.1은 예약 stub이나 **`odx-rs` 0.39.0은 실제 존재**한다. crates.io 배포 tarball의 MIT·Cargo·README를 확인했고 PDX ZIP/ODX XML·서비스/DOP 모델·검증 API가 있다. 다만 저장소 URL은 404라 최근 커밋 미확인, README의 파싱/검증 범위를 동적 UDS encode/decode 완성으로 간주하지 않는다. M3 실 OEM PDX의 상속·variant·DOP/COMPU·참조·가변 길이·부정 응답을 odxtools 결과와 비교한 뒤 제한 채택한다.
- CDD XML은 `quick-xml` streaming으로 읽되 OEM 방언→내부 DID/RID/DTC 모델 변환은 직접 구현한다. DTD/외부 엔티티 금지, PDX unzip byte/entry/깊이 상한을 둔다. 불완전 import를 성공으로 표시하거나 DB로 위험 등급을 낮추지 않는다.
- **미확인/Python 판단**: Rust ODX 후보가 있으므로 “Rust로 불가” 근거는 없다. 현재 DB 런타임 Python 잔존 필요는 **입증되지 않음**. 지원 불가 OEM 기능이 확인된 경우에만 odxtools 외부 변환 worker를 별도 결정하고, 불가 기능·fixture·제한을 기록한다. M1 핵심 루프에 ODX 완성을 선행시키지 않는다.

## 7. CANoe P4 영향

- **권장**: `_xteam/canoe-bridge.md`의 CAN 소유 STA worker·UDS proxy·decision_id·generation·progress/late·승인 raw bytes·Loom 소유 TP만 정지·자동 failover 없음은 유지한다. M2c/P2 필수 경로에서 P4 선택 어댑터로 내리고, CANoe 실행 중에도 기본은 자체 스택이며 실제 채널 공존 제약을 명시적으로 판정한다.
- **근거/미확인**: C4가 명시적으로 허용한 Python 예외를 재사용하되 **Rust COM 불가능이라는 증거는 없다**(`windows-rs`로 가능성 존재). 왜 Rust 미채택인지에는 “기존 pywin32 STA/이벤트·버전별 타입 연동을 재사용하는 선택 어댑터, Rust 대체 미검증”을 기술 부채로 기록한다. C1의 엄격한 예외 판정이 충족되지 않으면 P4를 유예하며 기본 진단에 Python을 설치하지 않는다. 실제 CANoe/Windows 자동화는 이번에 시험하지 않았다.

## 8. C2 — 상한·넘침 정책 제안

모든 수치는 **구현/CORE 합의 전 초기 예산**, 실측 결과가 아니다. 개수와 bytes 중 먼저 닿는 상한 적용, unbounded channel/Vec 누적 금지. 이벤트 원본은 CORE 디스크 저널로 보내고 스트림·trace는 cursor로 읽는다.

| 항목 | 초기 상한 | 초과 동작 |
|---|---|---|
| 대상/요청 큐 | 물리 ECU 64, 대상별 32·전역 512·8 MiB, in-flight 1/대상 | 송신 전 busy exit 6 `E_LOCKED(reason=queue_full)`; 큐 대기도 총 deadline에 포함. 사용자 입력/STOP 용량 예약, 폴링 tick은 skipped로 집계·밀린 tick 재생 안 함 |
| DoIP 수신/송신 | 연결 32, 각각 64 KiB/연결; 기본 UDS PDU ≤4095 B(DoIP 주소 헤더 별도) | 헤더 길이를 할당 전 검사, oversize는 연결 종료/exit 7; 가득 찬 TX는 새 요청 거부. 수신 task를 느린 UI/저널 대기로 막아 alive/FC를 굶기지 않음 |
| 이벤트 발행 | 제어 예약 256/1 MiB + 데이터 4096/4 MiB | CORE bounded 저널 경로 사용; UI 표시만 명시적 집계/gap 가능. 기록 실패는 health/누락 구간 표시, agent·script R1+만 exit 4 `E_RECORDING_FAILED`(A12), 사용자 입력은 예약 경로+stderr 감사로 계속 허용 |
| 폴링/샘플 | job 64, 고정 링 4096개 또는 1 MiB | 오래된 표시 샘플 overwrite·집계 수 표시, 원본 저널; 중첩 요청은 tick skip, 동시성 증가로 보상하지 않음 |
| 0x78 | 트랜잭션당 counter+마지막 상태만, 기본 50회·프로필 hard cap 1024 | 남은 총 deadline과 먼저 닿는 한도로 종료(exit 7, 쓰기 outcome_unknown)+U 등록; 응답 배열 누적 없음 |
| quarantine U | 대상별 SID 집합 ≤256/64 KiB, 전역 ≤4 MiB | bitmap/최소 식별 상태로 보존; 용량 부족 시 새 대상/요청 거부. LRU/TTL eviction으로 격리를 해제하지 않고 D19 증거로만 해제 |
| trace 조회 | 페이지 32 KiB·응답 64 KiB, 동시 8·cache 2 MiB | 저널 streaming/cursor, 전량 메모리 로드 금지; 스캔/보존 한계는 exit 8 + gap/다음 cursor, 동시 초과 busy |
| DB 로드 | 정규화 모델 8 MiB, XML 깊이 64·PDX 파일 1024/확장 총 32 MiB | 제한 넘으면 명시적 `E_DB` exit 2; importer helper 격리(64 MiB·5 s 초기 예산), 부분·잘린 모델을 활성 DB로 바꾸지 않음 |

- UDS live 할당 합계 **64 MiB quota** 제안(위 큐·DB·task 포함); 초과 전 새 job/target 거부·cache 회수, U/확정 상태는 보존한다. gateway RSS는 공유라 UDS 단독 RSS로 오인하지 않고 CORE allocation 계측+전체 RSS로 판정한다. UDS 포함 gateway RSS soft 96/hard 128 MiB는 CORE 검토용이며 soft에서는 cache 회수, hard에서는 신규 UDS 수용 중단·health 경고로 다루며 gateway kill을 정상 정책으로 두지 않는다.
- seed-key helper 64 MiB·2 s, 선택 CANoe Python worker 128 MiB 초기 예산. OS별 프로세스 제한/감독은 CORE 소유; 초과/크래시는 해당 worker만 종료, generation 폐기·영향 요청 outcome_unknown/U 처리, 재기동 후 승인/쓰기 자동 재생 금지. 정상 자체 UDS 경로에 Python RSS는 없다.
- **24 h soak 제안**: CAN FD 고부하+UART flood와 동시, 64 가상 대상/각 20 Hz 폴링을 스트레스 프로필로 수행(실 ECU 레이트 정책은 우회하지 않음); 정상 100 tx/s 구간도 분리한다. 0x78 장기/상한, 무응답·반복 timeout/U 포화, SID 교대·늦은 78/최종 응답, 연결 flap/세대 교체, 느린 웹 UI·trace, 저널 정지/디스크 full, seed helper crash를 반복 주입한다.
- 관문 초안: 30분 warmup 뒤 전체 RSS 증가 ≤16 MiB/24 h·기울기 ≤0.5 MiB/h, UDS 할당 64 MiB 유지, worker 사망에도 daemon/다른 장치/저널 생존. 정상 경로 실제 **장치 이벤트→웹 렌더** p95 ≤100/p99 ≤200 ms(공통 harness), 부하 구간 gap/집계·lag 명시, 사용자 개입/STOP 보장. CORE 보존 quota·rotation 후 cursor gap도 시험한다. **이번에 24 h·실 UI·HW 관문을 수행하지 않았다.**

## Rust 스파이크 결과

한 Cargo 프로젝트에서 후보 API와 최소 tokio DoIP loopback을 확인했다([소스](../../../spikes/uds/rust/src/main.rs), [잠금 파일](../../../spikes/uds/rust/Cargo.lock), [증거 JSON](../../../spikes/uds/evidence/v2-rust.json)). cargo/rustc 1.93.1, macOS arm64, release/locked, 단일 스레드, 20회 warmup+1000회: **p50 0.019875 / p99 0.120583 / max 0.363917 ms**. RA 후 ACK+VIN 응답, 분할/연속 코덱 및 0xE0·oversize 결과를 확인했다.
기존 Python DoIP 100회 median 0.305084/p99 0.850958 ms, CAN IPC median 0.92/p99 3.09 ms와는 표본·스택·IPC 조건이 달라 속도 배수 비교 불가. 새 측정에 TLS·발견·alive·완전한 러너·IPC·웹 UI·Windows/Linux·장비는 없다. 로컬 캐시/의존성만 사용했고 전역 설치는 하지 않았다.

## 직접 구현 범위

| 소유 | 직접 구현/유지 | 관문 |
|---|---|---|
| UDS | M1 서비스 코덱·러너(P2/78/총 deadline/SPRMIB/epoch/D19), bounded DoIP·LA/alive/discovery, DB 모델·가드 훅·웹 패널/TC 명세 | sim 4요소 E2E→Windows/Linux VN/PCAN FD HIL |
| CAN | XL CAN FD/raw Ethernet·PCAN-Basic/SocketCAN FD·ISO-TP worker·rx_start/barrier | FD/채널 공존·timestamp·STmin/FC/abort |
| CORE/UI | gateway/IPC/저널·큐·승인/lease/감사·스키마 생성/worker 감독·웹 shell/공통 TC runner | 24 h 혼합 부하·실 렌더 지연·사용자 개입 |
| helper/P4 | seed-key x86/x64 격리 exe·DB importer; 기존 CANoe bridge 선택 이식 | ABI/crash/RSS·generation·승인 raw bytes |

## 외부 crate 채택

각 crate 링크는 registry 근거(버전/라이선스), 날짜 링크는 실제 조회한 저장소 HEAD 커밋이다. `MIT/Apache`는 `MIT OR Apache-2.0` 축약, “조건부”는 추가 검증 전 채택 확정 아님.

| crate / 조회 버전 | 라이선스 | 최근 커밋(UTC) | 판단 |
|---|---|---|---|
| [automotive_diag 0.1.29](https://crates.io/crates/automotive_diag/0.1.29) | MIT/Apache | [2026-08-19](https://github.com/oxibus/automotive_diag/commit/6bed2f35f8833fb499493b89043bb136b2799899) | 정의만 조건부 채택, uds/doip features |
| [ecu_diagnostics 0.107.5](https://crates.io/crates/ecu_diagnostics/0.107.5) | GPL-3.0-only | [2026-08-21](https://github.com/rnd-ash/ecu_diagnostics/commit/45df8bce561b66e66225a65e17340fd7f3f26749) | 비채택 |
| [automotive 0.6.0](https://crates.io/crates/automotive/0.6.0) | MIT | [2026-11-09](https://github.com/I-CAN-hack/automotive/commit/96657f1f805ba4ab1ca413d5b3d308a5a7d52a16) | 러너/XL 참조·상호 운용 시험 후보 |
| [uds-rs 0.3.0](https://crates.io/crates/uds-rs/0.3.0) | MIT(배포 LICENCE; registry non-standard) | [2026-07-25](https://github.com/japawBlob/uds-rs/commit/232d24c5c2e7a98fe84649a990050089c884c53b) | Linux 결합, 비채택 |
| [doip-definitions 3.0.14](https://crates.io/crates/doip-definitions/3.0.14) | MIT | [2026-02-17](https://github.com/samp-reston/doip-definitions/commit/36ab2310a1e93107b0c3bdf01576c213fe2a33ac) | 0xE0 오류로 비채택 |
| [doip-codec 2.0.9](https://crates.io/crates/doip-codec/2.0.9) | MIT | [2026-02-17](https://github.com/samp-reston/doip-codec/commit/40c996ab58b8834897cbb3aaf432ee6f789225ec) | 비채택, 스파이크 비교용 |
| [doip-sockets 0.2.4](https://crates.io/crates/doip-sockets/0.2.4) | MIT | [2025-08-03](https://github.com/samp-reston/doip-sockets/commit/e28b8a944812a090b834ee691905635849e142a5) | thin wrapper, 비채택 |
| [isotp-rs 0.2.1](https://crates.io/crates/isotp-rs/0.2.1) | GPL-3.0 | 미확인(저장소 404) | 전 버전 yanked, 제외 |
| [socketcan-isotp 1.0.2](https://crates.io/crates/socketcan-isotp/1.0.2) | BSD-3-Clause | [2023-06-28](https://github.com/marcelbuesing/socketcan-isotp/commit/a0b543ac5e4a8821bfedb7d3fdde26e16d439deb) | CAN 팀 Linux 조건부 |
| [tokio-socketcan-isotp 0.2.0](https://crates.io/crates/tokio-socketcan-isotp/0.2.0) | MIT(배포 LICENCE; registry non-standard) | [2026-06-09](https://github.com/japawBlob/tokio-socketcan-isotp/commit/029dc98a6f9e9d6100bc8ef6ddcf4b1ff3c7d4ac) | CAN 팀 커널/비동기 검토 |
| [can-hal-isotp 0.4.3](https://crates.io/crates/can-hal-isotp/0.4.3) | MIT/Apache | [2026-11-08](https://github.com/can-hal-rs-contributors/can-hal-rs/commit/e34695a79356a4d373d34b04583e49e2565840a2) | CAN 팀 userspace 조건부 |
| [smoltcp 0.14.0](https://crates.io/crates/smoltcp/0.14.0) | 0BSD | [2026-11-04](https://github.com/smoltcp-rs/smoltcp/commit/8b0e5c8598647fe92e2a1b87932d1932354fee0b) | XL 경로 필요 시, MSRV 1.91 |
| [libloading 0.9.0](https://crates.io/crates/libloading/0.9.0) | ISC | [2026-08-02](https://github.com/nagisa/rust_libloading/commit/f5dc3b81485b978596d518f7574d941ff811bf74) | helper에 채택 |
| [quick-xml 0.42.0](https://crates.io/crates/quick-xml/0.42.0) | MIT | [2026-11-02](https://github.com/tafia/quick-xml/commit/250824b3f7c1bfa986177083b89263d7103b5e62) | CDD streaming 채택 |
| [serde 1.0.229](https://crates.io/crates/serde/1.0.229) | MIT/Apache | [2026-10-16](https://github.com/serde-rs/serde/commit/0db9b3b504bcb0a7fa9cf001429869519afd3e9a) | CORE 정본/DB 채택 |
| [serde_yaml 0.9.34+deprecated](https://crates.io/crates/serde_yaml/0.9.34+deprecated) | MIT/Apache | [2024-03-25](https://github.com/dtolnay/serde-yaml/commit/2009506d33767dfc88e979d6bc0d53d09f941c94) | deprecated, 비채택 |
| [serde-saphyr 2.1.1](https://crates.io/crates/serde-saphyr/2.1.1) | MIT/Apache | [2026-11-09](https://github.com/bourumir-wyngs/serde-saphyr/commit/a96e17928d12f133a16cc4e8fb61a1fce8bdcd8b) | 제한 스키마/리소스 검증 후 조건부 |
| [odx 0.0.1](https://crates.io/crates/odx/0.0.1) | MIT/Apache | 미확인(빈 저장소 409) | stub 제외 |
| [odx-rs 0.39.0](https://crates.io/crates/odx-rs/0.39.0) | MIT(배포 LICENSE 확인) | 미확인(저장소 404) | M3 제한 ODX importer 후보, MSRV 1.88 |
| [tokio 1.54.1](https://crates.io/crates/tokio/1.54.1) | MIT | [2026-11-09](https://github.com/tokio-rs/tokio/commit/c1489e0ef85f00f3590585eab1f1712129834742) | DoIP/runtime 채택, MSRV 1.85 |
| [rustls 0.23.46](https://crates.io/crates/rustls/0.23.46) | Apache-2.0 OR ISC OR MIT | [2026-11-09](https://github.com/rustls/rustls/commit/e7506724218a5317081bc194843478a0058f72af) | 선택 TLS 채택 |
| [tokio-rustls 0.26.6](https://crates.io/crates/tokio-rustls/0.26.6) | MIT/Apache | [2026-11-02](https://github.com/rustls/tokio-rustls/commit/0b8390a9c6e4356550154c26687866063e8f6038) | 선택 TLS 어댑터 채택 |
