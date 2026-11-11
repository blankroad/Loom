docs/plan/02-core-principle.md 준수

# v2.0 — uds-sol: Rust 이식 실현성·crate·전송 조사 (plan.md 수정 금지, 보고서만)

먼저 읽기: `docs/plan/00-master-plan.md` §2.3 **C1–C8**(최우선, C > B > A > D), `docs/plan/04-v2-tasks.md`, 현재 `docs/plan/uds/plan.md` v1.2(§3·§7·§8·§9·§10.8, 결정 D2–D4·D19·D23). 새 CORE 팀 브리프 `docs/plan/core/brief.md`(Rust 코어, worker 프로토콜).

배경: 코어와 장치 경로를 Rust로 바꾼다(Python은 대안이 없을 때만, 예: CANoe COM). UDS 진단 기본 경로는 자체 스택 over **Vector VN5640(CAN FD + 차량 이더넷)** 과 **PEAK PCAN-USB FD**. Vector XL FFI 바인딩 crate(CAN FD + 이더넷 원시 프레임)는 CAN 팀 소유, DoIP 전송은 UDS 소유. 실HW 호스트는 Windows x64·Linux x64, macOS는 개발.

## 조사·판단 (근거 URL·버전·라이선스·최근 커밋 날짜, crates.io/GitHub 실제 조회)
1. **UDS 코덱·클라이언트 crate**: 예) `automotive_diag`, `ecu_diagnostics`, `automotive`, `uds-rs`류 등 실제 존재하는 것 조사. v1의 결정(자체 트랜잭션 러너 + 코덱만 차용, D2)을 Rust에서 유지할지: 0x78·P2/P2*·총 deadline·epoch·응답 식별자 기반 격리(D19)·SPRMIB를 자체 러너로 두고 코덱만 쓸 수 있는지, 아니면 코덱도 자체 구현이 나은지.
2. **DoIP crate**: 예) `doip-definitions`, `doip-codec`, `doip-sockets` 등. 자체 tokio DoIP 전송(엔티티·SA당 소켓 1개 + LA 분배, 단일 수신 task, alive check, deadline, TLS는 rustls) 대비 이득. 0xE0 central security 값 처리 확인.
3. **ISO-TP**: CAN-TP는 CAN worker가 호스팅하는 ISO-TP(`IsoTpPort` PDU RPC)를 쓴다(D11 유지). Rust ISO-TP crate(예: `isotp-rs`, `socketcan-isotp`(Linux 커널 ISO-TP)) 현황만 정리해 CAN 팀 결정에 줄 의견. Linux 커널 `CAN_ISOTP`(5.10+) 사용 가능성 vs 사용자 공간 구현(PCAN·Vector는 사용자 공간 필요).
4. **VN5640 이더넷 위 DoIP** (C4): (a) VN5640을 OS 네트워크 어댑터로 노출해 OS TCP/IP 사용(Vector 드라이버 설정 이름·조건·Windows/Linux 지원), (b) XL 원시 이더넷 프레임 위 사용자 공간 TCP/IP(`smoltcp` — TCP 재전송·윈도우·ARP·DoIP 13400 UDP 공지 처리 수준, 지연·처리량 우려), (c) PC NIC + 미디어 컨버터(100BASE-T1 ↔ 100BASE-TX). 각각의 전제·위험·실측 필요 항목. Vector 공개 문서 우선. 권장안과 근거.
5. **seed-key (CANoe 없이)**: Windows DLL 직접 로드 FFI(`libloading`, `GenerateKeyEx`/`GenerateKeyExOpt` 시그니처 — Vector seed-key DLL 인터페이스 공개 자료), **32비트 DLL은 64비트 프로세스에서 로드 불가 → 32비트 helper exe(Rust i686 타깃) 프로세스** 필요 여부, 크래시 격리(별도 프로세스, C2). 사용자 스크립트(Rust 플러그인? 외부 프로세스 stdin/stdout JSON — Python 스크립트도 외부 프로세스로 허용 가능?)의 우선순위 제안.
6. **진단 DB**: ODX(odxtools 대체 Rust crate 유무), CDD XML 읽기(quick-xml), 자체 YAML(serde). Python이 남아야 하는 곳이 있으면 "왜 Rust로 안 되는가" 근거.
7. **CANoe 백엔드 강등**(P4, Python 어댑터 별도 프로세스) — `_xteam/canoe-bridge.md` 합의는 유지. 영향만 짧게.
8. **C2 성능·메모리**: UDS 쪽 큐(대상별 요청 큐, 이벤트 발행 큐, 폴링 샘플, 0x78 대기, quarantine 집합 U, trace 조회) 상한과 넘칠 때 정책(drop/block/spill/거부 exit) 제안, UDS 메모리 예산, 24 h soak에 넣을 UDS 부하 패턴(예: 다중 대상 폴링 20 Hz, 0x78 장기, timeout 반복, 재연결 반복).

## 짧은 Rust 스파이크 (선택, 1–2개만)
`spikes/uds/rust/` 안에서만(cargo는 이미 설치돼 있음, 전역 설치·`cargo install` 금지, 의존성은 로컬 Cargo.toml). 예: tokio DoIP 헤더 코덱 + loopback 가상 엔티티 왕복 p50/p99/max (Python 스파이크 `spikes/uds/evidence/*.json`과 비교), 또는 후보 crate 하나 빌드·API 확인. 결과를 `spikes/uds/evidence/v2-*.json`에.

## 출력
`docs/plan/uds/v2-sol.md`, 150줄 이하, 한국어. 각 항목: 권장안 / 근거 / 미확인. 끝에 "직접 구현 범위"와 "외부 crate 채택" 표. 끝나면 멈춘다.
