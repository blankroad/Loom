# P1b 보고 — explore-sol (2026-11-11)

완료. eth worktree에서 capture/worker만 변경, 커밋 없음. b497a9e의 U64·batch.unrecorded 변경을 유지했으며 model.rs·loom-eth/src/lib.rs·gateway·cli·manifest는 수정하지 않음.

## 구현·공개 API
- live `list_interfaces() -> Result<Vec<InterfaceInfo>>`: 이름/설명/IP/up/loopback/can_capture/reason/permission_denied. promisc=false·snaplen64로 열고 즉시 닫아 현재 권한/백엔드 실패를 구분. worker eth.ifaces가 백그라운드에서 호출.
- `LiveSource::set_bpf(expr)`는 `pcap.filter(expr, true)`. `CaptureError::Bpf{msg}` → worker E_USAGE/exit2. sim/replay의 bpf 지정(빈 문자열 포함)은 E_NOT_IMPLEMENTED와 `-Y` hint.
- TCP는 자기 TcpStream만 생성/재사용/종료. 기본은 write 후 닫기, keep_open=true는 목적지별 최대4개/유휴30s. 연결 실패는 failed+E_NOT_OPEN. 캡처 poll과 분리한 송신 스레드, STOP/deadline 재검사, 자동 재접속/재송신 없음.
- 부분 write 후 실패는 outcome_unknown+accepted_bytes, 전부 OS에 수락되었을 때만 sent. 캡처가 먼저 도착해도 미완료 송신을 sent로 만들지 않고 observed_src_seq가 뒤로 가지 않도록 동기화.
- sim/replay TCP는 OS 접속 없이 SimTap의 SYN/SYN-ACK/ACK·data·FIN 흐름을 같은 CaptureLoop로 발행. 기존 IPv4 API 유지, `SocketConnection`/`push_tcp_socket`/`forget_socket_connection`으로 IPv6 추가.
- RawFrame에 `meta: Option<LoomMeta>`·`comments: Vec<String>` 추가, replay가 pcapng X10/사용자 주석을 이동·보존. E2 heap 계산에도 주석/req를 포함.
- replay의 세 origin·rx/tx·req 링크를 decoder/payload에 복원. X10 req 링크 role=tx/confidence=exact는 저장된 연관을 뜻함(새 wire 관측 주장 아님). 과거 메타로 현재 eth.tx 행을 만들지 않음.
- 원래 X10 seq는 layers._capture.replay_src_seq(문자열), 새 capture seq와 구분. 사용자 주석 전체는 bytes.comments, 원래 X10 문자열은 bytes.loom_comment에 보존하여 layers 4KiB 상한을 유지.

## 상한·결정
- 장치256개/주소64개/장치, 설명·보존 실패 이유 각각4KiB, BPF8KiB/NUL 거부. 목록·주소·설명 초과는 오류, 긴 이유는 UTF-8 경계에서 `...` 표시 후 자름.
- TCP 연결4개(일회성 포함), native 큐4개(기존 TX64명령/1MiB 예산도 적용), connect≤50ms·write≤50ms·STOP poll2ms, idle30s. sim 교환≤7프레임, idle 정리≤12프레임, 기존 pending64개/64KiB.
- 송신 payload≤9KiB, recent256개/TTL2s. IPv6 TCP 헤더까지 귀속되도록 프레임 사본 상한=9KiB+128B, 총 사본≤256배. native TCP tuple 및 연결 중 목적지 일치 SYN은 보수적 probable(장치 증거에서 제외); idle FIN도 귀속 갱신.
- 주석은 pcapng 상수64개(메타 포함)/개당4096B, req≤512B를 재사용. E2 128MiB/512,000개, decode/inline16KiB·layers4KiB 등 P1 상한 유지.
- 숫자 IP:PORT만 받음(DNS 없음), TCP from_port는 모델의 UDP 전용 의미대로 E_USAGE. TCP는 응답 읽기/외부 TCP stream 주입을 하지 않음.

## 검증
- 필수 4관문 PASS: cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; cargo clippy -p loom-eth --features live --all-targets -- -D warnings; cargo test --workspace(190개).
- worker 24개 PASS: P1 16개 + BPF 거부/exit2, native loopback 재사용·닫기·만료·연결 실패·4개 상한·STOP, sim/replay TCP·IPv6·9KiB IPv6 귀속, pending SYN/tuple/TTL, X10 origin×dir 6조합·8KiB 사용자 주석 보존.
- cargo test -p loom-eth-capture --features live: 24개 PASS, 실제 목록/open+BPF 2개는 #[ignore]. dead handle 최적화 BPF 정상/문법 오류·NUL/길이, 순수 장치 판정·권한/백엔드 오류·상한, replay RawFrame 메타/주석 검증 포함.
- 물리 장치를 열거나 시스템 권한을 바꾸지 않음. 실제 native capture/BPF 적용·wire 교차검증은 미실시. git diff --check PASS.
- 모든 cargo는 worktree .cache/cargo·target, 최종 clippy/test는 --offline. 외부 의존성 추가 없음. 빌드 중 Cargo가 PSU workspace lock 항목을 보완했으나 최종 Cargo.lock diff는 없음.

## 리드 요청·질문
- SendParams/schema/CLI에 keep_open(false 기본)/--keep-open 정식 추가 요청. 현재 worker는 원래 Write.op의 bool을 읽으며 gateway는 그 JSON을 그대로 전달하므로 RPC 사용 가능.
- bytes.comments/loom_comment의 보존 위치를 공유 모델/스키마에 명시하고 gateway export가 이 사용자 주석도 복사하도록 후속 반영 필요(현재 export는 주석을 읽지 않음).
- SDK rx는 top-level dir=rx 고정이므로 복원 dir은 payload.dir/decoder에 보존. CORE data-draft API/UI payload.dir 우선 처리는 P1 요청 유지. 저널 없는 예산 생략 원본 손실도 P1과 동일.
- 질문: 위 공유 필드·export·SDK 정규화의 반영 순서는 리드가 결정. 구현을 막는 미결 질문 없음.
