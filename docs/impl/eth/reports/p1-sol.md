# P1 보고 — explore-sol: eth worker (2026-11-11)

**리드 메모**: worker 구현·필수 관문은 통과. live 장치 열기 경로는 연결했지만 장치 목록/권한 사전 판정과 BPF는 공개 capture API가 없어 `E_NOT_IMPLEMENTED`; 아래 요청 필요. opus 보고서의 작업 중 clippy 오류는 현재 해소됨.
작업: `/Users/ctmctm/workspace/Loom/.worktrees/eth`, `eth/main`. 코드 변경은 `crates/loom-eth/src/worker/`만, 커밋 없음. model/lib/manifest 및 다른 담당 파일은 수정하지 않음.

## 구현
- `EthWorker: Worker`, `new`/`with_config(WorkerConfig)`/`health`. 한 인스턴스·한 채널, start/stop/status/interest/reset, 기본 빌드의 sim/replay 가상 ifaces.
- live(feature)/replay/sim → CaptureLoop E2 → StreamTable → 동일 `Emitter::rx` eth.pkt 경로. 요약·layers·base64 원본, 100 ms batch, 1 s stats, capture_state, queue/lag/publication/unrecorded 지표.
- raw는 live 드라이버 send, sim/replay 가상 탭. UDP는 실제 비차단 OS 소켓 송신(가상 백엔드는 캡처 복사본 추가). TCP/jobs/input은 명시적 미구현.
- E8 requested→accepted→sent/failed, sent는 드라이버/소켓 수락이며 캡처 복사본보다 먼저 발행. STOP은 agent/script를 큐 진입 전·드라이버 전 검사, user 허용. deadline/레이트/크기/큐 포화 오류 처리.
- 최근 송신 해시+실제 바이트 비교로 origin/link·observed_src_seq 갱신. 완전 일치 exact, 잘린 프레임·중복 요청·UDP socket tuple+payload는 probable. 자기 송신은 발행 예산을 우회.
- sim 시작/reset 시 DoIP 공지 3회(0/100/200 ms), SD 기본 1 s+설정 지터, 완전한 IPv4 TCP DoIP routing activation 최소 응답. sim 프레임 ts_src=sim, UDS 가상 ECU 연결 없음.
- kernel/if/stats unavailable/restart→source.gap, E2/정지 폐기→record.gap, 발행 실패→display.gap. producer_range를 만들지 않고 capture 좌표를 detail에 명시.
- 제어 작업은 인스턴스당 백그라운드 1개, 취소된 start는 캡처 정리. STOP은 생산자 join 뒤 잔여 원본을 손실로 집계하며 느린 backlog 디코드를 하지 않음.

## 상한·결정
- E2 기본/최대 128 MiB·512,000프레임(초과 newest drop), capture gap 큐 1,024개(coalesce). StreamTable 64 MiB; section/interface 전환은 재설정(동시 IDB별 TCP 상태 유지 안 함).
- 발행 기본 5,000/max 20,000 pkt/s, 송신 기본/max 1,000 pkt/s(RateGuard). 관심 필터 ≤16/텍스트 합계 8 KiB, 교체는 전체 컴파일 성공 후 원자적으로 적용.
- 송신 ≤9 KiB, TX 큐 64명령/1 MiB, recent 256개/TTL 2 s/프레임 사본 ≤2,375,680 B, sim 응답 64개/64 KiB.
- control JSON ≤32 KiB, 식별자 ≤256 B, decode/inline ≤16 KiB, layers ≤4 KiB, batch protocol ≤32. 상한 위반은 거부/표시, 무한 재시도·큐 없음.
- WorkerConfig로 예산·E2·SD period(10..60,000 ms)/jitter(<period)·live env 설정. 공유 모델은 v1 그대로 유지. replay speed N은 N배속으로 capture interval factor=1/N 변환.
- replay는 regular file만 허용. 과거 replay 프레임을 현재 송신과 매칭하지 않음. P0 RawFrame에 과거 LoomMeta가 없어 기존 귀속을 복원하지는 못함.
- fixture 원시 timestamp가 u64를 넘는 경우 decoder에는 capture mono 대체값, `_capture.ts_ns`에는 원래 u128 문자열, health에는 fallback 횟수. source seq는 재시작 후에도 단조 유지.
- 직접 worker dry-run은 행/송신 없이 보수적 R4 preview. 실제 gateway dry-run은 gateway가 처리. RSS/실제 장치 권한/미구현 기능의 성공을 꾸미지 않음.

## 원본 공백 (M1)
- journal writer 없음: 개별 발행된 eth.pkt.bytes만 원본이다. 예산 생략/Emitter 실패/정지 폐기 원본은 어디에도 남지 않는다.
- batch total−shown, record.gap `m1_unpublished_no_journal`, health.unrecorded/journal_available=false로 드러냄. inline 잘림도 별도 record.gap. 이 구간은 전수 증거로 쓰면 안 됨.

## 검증 (실 NIC 없이)
- worker 16개 통과: replay fixture 9프레임·연속 seq·요약/원본, 예산1+관심필터(batch total9/shown5), sim 공지/SD/reset/시간 점프, raw 귀속·RA 응답·송신 순서.
- STOP agent/script 거부·user 허용, 실제 loopback UDP, stats unavailable gap, 정지 <200 ms, E2=1의 전달+손실=9, MemorySink 역압력/복구 gap, 필터/9KiB/rate/TCP 오류, 취소 정리·재시작 seq.
- native UDP tuple/TTL 및 snaplen probable 귀속 단위시험, routing parser 100,000개 결정적 임의/변형 입력 panic 없음. 물리 NIC·W/L 런타임·Wireshark 교차검증은 미실시.
- worktree 루트 `cargo fmt --all --check` PASS, `git diff --check -- crates/loom-eth/src/worker` PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` PASS.
- `cargo clippy -p loom-eth --features live --all-targets --locked -- -D warnings` PASS.
- `cargo test --workspace --locked` PASS(총 166개). CARGO_HOME=`$worktree/.cache/cargo`, CARGO_TARGET_DIR=`$worktree/target`; 의존성 다운로드 성공.

## CORE·capture 요청
1. worker journal writer + pcap-epb/1 codec/읽기 API. capture RawFrame에 기존 LoomMeta 전달 계약이 있어야 replay 귀속도 보존 가능.
2. capture 공개 `list_interfaces`(주소·권한 판정/실패 이유), BPF setter 필요. 또는 리드가 직접 pcap 의존성 허용/manifest 반영. 현재 live eth.ifaces·nonempty BPF는 E_NOT_IMPLEMENTED.
3. SDK data draft API(dir/purpose/quality): rx는 top-level origin=device/dir=rx 고정. 현재 자기 송신은 payload.dir=tx/attributed_origin에 보존; UI ethModel은 payload.dir보다 record.dir을 우선하므로 수정 요청.
4. SDK typed record/display gap emitter: 현재 kind는 record.gap/display.gap이나 priority의 domain은 eth 고정(source_gap만 source 도메인 API 제공). 표준 gap domain으로 정규화 필요.
5. capture.source_seq와 SDK producer src_seq는 상태/통계 이벤트 때문에 다름. 모델에 좌표·매핑 계약, Health adapter/runtime 연결 필요(현재 health()만 제공).

## model 변경 제안·질문 (리드 소유)
- BatchPayload에 `unrecorded: true` 또는 `unrecorded_count` 추가, “원본은 전부 기록됨” 주석 수정. 현재 v1 필드를 임의 추가하지 않고 gap/health로 표시.
- Batch/Stats/Tx/Pkt seq 등 u64 payload의 canonical JSON U64 문자열 serde 적용 필요(v1은 primitive 숫자); `_capture` 좌표는 이미 문자열. 공유 모델을 worker에서 독자 변형하지 않음.
- 질문: live 목록/BPF API 소유·상기 SDK gap/dir 및 model 정규화 반영 순서를 리드가 결정해 주세요. 그 전까지 위 명시적 미구현/제약을 유지.
