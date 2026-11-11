# can-sol v2.0 작업 — Rust crate·드라이버·FFI 조사와 실현성

먼저 `docs/plan/can/tasks/v2-common.md`를 읽고 따른다. 출력: **`docs/plan/can/v2-sol.md`** (한국어). 실제 문서·소스·crates.io/docs.rs를 확인하고 출처를 단다. 필요하면 `spikes/can/rust/`에 짧은 Rust 검증(빌드·간단 측정)을 하고 결과를 `spikes/can/rust/RESULTS.md`에 남긴다.

## 써야 할 것
1. **crate 선택표** (이름·버전·라이선스·최근 릴리스·유지 상태·Windows/Linux 지원·FD 지원·채택/보류 이유):
   - SocketCAN(Linux, FD): `socketcan` crate 등. netlink로 netdev 값 읽기(v1.2 §8.4 "사람 사전 설정 + 검증" 유지)에 쓸 수 있는 crate.
   - PCAN-Basic: **C5의 "PEAK 공식 Rust FFI(PCAN-Basic 5.1+)"가 실제로 존재하는지** 확인(PEAK 다운로드·문서). 없으면 직접 FFI(`libloading`) 범위.
   - Vector XL: 공개 Rust crate가 정말 없는지 재확인. 자체 바인딩 범위 = CAN FD(`xlCanFdSetConfiguration`, `xlCanTransmitEx`, `xlCanReceive`, 이벤트·타임스탬프), 채널 공존용 init access 없이 열기, **이더넷 원시 프레임 API**(`xlEth*` — 이름과 기능 확인). `bindgen` vs 손으로 쓴 선언. `vxlapi.h`를 어디서 얻는지(라이선스상 저장소에 넣을 수 있는지).
   - pyvxl: 공개판(cmcerove/pyvxl)이 32비트·CAN 전용인지 확인하고, 교차 검증용으로 무엇을 비교할지.
   - ISO-TP: 기존 Rust crate(예: `automotive`, `ecu_diagnostics`, `isotp-rs` 등) 조사. **결론: 채택 vs 직접 구현**. 요구: v1.2 §15.2·`_xteam/can-uds-isotp.md`의 계약(normal 11/29bit, FD, BS/STmin, FC 타이밍, 소유권·예약 tx_id, `tx_state: driver_accepted/bus_confirmed`, abort, 결함 훅). FC 타이밍 전용 스레드 설계(OS 스레드 우선순위, Windows 타이머 해상도).
   - DBC: `can-dbc` 등 파서 crate 비교(멀티플렉싱, 값 테이블, 속성, 비표준 DBC 내성). 인코드/디코드 엔진은 직접?
   - 로그 포맷: BLF/ASC/MF4/TRC 쓰기·읽기 Rust crate 유무. 없으면 범위(BLF는 export 우선).
   - 비동기 런타임·스레드 모델: tokio vs 전용 OS 스레드(수신·주기 송신·FC), 크로스빔 채널 등 상한 큐.
2. **C2 큐·메모리 표 (CAN worker):** 모든 큐(드라이버 수신 → 링 → 저널 writer, → gateway IPC, TX 요청, 주기 스케줄러, ISO-TP)마다 상한·넘칠 때 정책(drop·block·spill)·카운터 이름(`source.gap`/`record.gap`/`display.gap`). 링 버퍼 고정 크기, worker 메모리 예산(목표 수치와 근거), 초과 시 동작. **24h soak 부하 패턴**(CAN 측: 예 5k fps 혼합 + FD 64B + 주기 job N개 + ISO-TP 왕복 반복 + 버스오프 주입)과 관문 지표(RSS 증가 상한, 지연 p99, 손실 0 조건).
3. **벤더 DLL 격리(C2):** `vxlapi64.dll`·`PCANBasic.dll`은 worker 프로세스에서만 로드. 한 프로세스에 둘 다 넣을지 분리할지(근거), 크래시 시 재기동·재열기 규칙.
4. **VN5640 이더넷 위 DoIP 경로 (a)/(b)/(c) 사실 조사** (UDS와 합의할 `_xteam/vn5640-eth.md`의 CAN 초안 재료):
   (a) VN5640을 OS 네트워크 어댑터로 노출할 수 있는지(Vector 드라이버 설정 이름, "Network-based access"/"Ethernet 포트를 Windows에 노출" 같은 기능이 실제 있는지, 출처), (b) XL 이더넷 원시 프레임 위 사용자 공간 TCP/IP(`smoltcp` — 버전, TCP 지원 범위, 실현성), (c) PC NIC + 미디어 컨버터(100BASE-T1/1000BASE-T1). 각각 지연·구현량·실측 필요 항목·라이선스 제약. 확인 못 한 것은 미확인.
5. **M0/M1 실현성**: Rust virtual CAN 백엔드(프로세스 간 sim 버스 — python-can virtual 대체: 공유 메모리/로컬 소켓 등 방식 제안), macOS 개발에서 무엇이 돌아가는지. v1.2 실측(FC p99 52 ms, 주기 오차 p99 47 ms 등)을 Rust에서 다시 잴 스파이크 목록(가능하면 macOS에서 1~2개 직접 측정: 예 전용 스레드 10 ms 주기 오차, 크로스빔 채널 처리량).
6. CANoe 브리지 강등: Python 어댑터 worker로 남길 때 "왜 Rust로 안 되는가"(COM은 Rust `windows` crate로도 가능한지 — IDispatch late binding 난이도, 이벤트 싱크) 근거 한 단락.

끝나면 파일 저장 후 멈춘다.
