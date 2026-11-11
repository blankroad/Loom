# P0 sol — loom-eth-capture
- 위치: `.worktrees/eth/crates/loom-eth-capture` (eth/main). 직접 커밋하지 않았다. 진행 중 리드 커밋 4ebec5e에 pcapng가 포함된 것을 확인했다.
- 구현: `CaptureSource`, `ReplaySource`, `SimTapSource`/`SimTapHandle`, feature `live`(기본 OFF)의 `LiveSource`, `CaptureLoop`, `BoundedFrameQueue`.
- Replay: pcapng/고전 pcap streaming Read+Seek, Max/Realtime{factor}, 반복. factor는 원본 간격의 배수(2=두 배 느림); interface/section identity 보존, 빈 반복은 EOF, 긴 metadata 구간은 poll당 32 block 뒤 yield.
- SimTap: etherparse로 Ethernet/IPv4/TCP/UDP 합성; 연결별 양방향 seq/ack·SYN/FIN·최종 ACK, 큐 거부 시 상태를 전진시키지 않는다. `ts_source=sim`이며 decode crate 의존 없음.
- Live: pcap 2.5.0, 활성화 전 buffer/snaplen/promisc/Nano/immediate 설정, 비차단 poll, sendpacket, Permission 오류 분리. 실제 NIC/BPF는 열지 않았다.
- pcap의 안전한 precision setter가 반환 코드를 버리고 getter가 없어, 같은 활성 핸들이 쓰는 24-byte pcap 헤더로 실제 해상도·snaplen을 확인하는 safe 경로를 구현했다(임시 private dir·정리). Micro 관찰/실패 시 명시 Micro reopen. 실제 fallback은 미확인.
- drop stats는 보수적으로 Partial/Unavailable을 노출한다. if_drop=0은 unknown(None); 0=무손실로 간주하지 않는다. 100ms마다(무트래픽 포함) 비교하고 마지막 유효 counter를 유지해 복구 시 중복/누락을 막는다. counter reset은 unknown CaptureRestart.
- CaptureLoop: source_seq 단조 증가·host monotonic, drop-newest Record gap, 정지 signal, restart 시 seq/mono 연속+CaptureRestart gap. thread 종료/오류 시 큐 close; join으로 오류 관찰.
- gap overflow는 Mixed/Coalesced 1개로 합친다. first/last는 enclosing 범위이고 `coalesced=true`면 연속 손실을 뜻하지 않는다. 종류별 count를 별도 유지한다.

| C2 대상 | 기본/hard 상한 | 넘칠 때 |
|---|---|---|
| raw queue | 128 MiB / 512,000 frames, 낮게 설정 가능 | drop-newest, 정확한 seq range/count |
| raw byte 회계 | reserved pointer ring+frame+Vec/String capacity 포함 | 예산 부족이면 enqueue 거부 |
| gap deque | 1024 reports | 종류별 건수 보존 coalesce |
| Sim ingress | 4 MiB / 4096 frames | 명시 Limit, seq 변화 없음 |
| Sim TCP map | 1024 connections, reserved storage 보수 회계 | 신규 거부; final ACK/forget으로 제거 |
| raw frame / replay block | 약 1 MiB / 1 MiB | 할당 전 오류(pcapng); native copy 전 확인 |
| live kernel buffer | 64 MiB 요청 상한 | 설정 오류; 실제 OS 할당량은 미확인 |
| waits / replay factor | capture poll 20ms, condvar sleep ≤1s / (0,1e6] | reject 또는 bounded yield |

- 관문: `cargo fmt --check`, 기본 `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과. `--features live` check·all-targets clippy 통과(장치 열기 실행 없음).
- 테스트: **20개 통과**(unit 3+integration 17). 샘플9개 바이트/seq, 98개 drop 범위, byte/count 상한, gap overflow 혼합 계수, unavailable/복구/reset, idle 100ms, multi-IDB/section/고전 pcap, empty repeat, TCP SYN/data/FIN/final ACK, Sim 상한, restart, 정지 <200ms 포함.
- live 테스트 2개는 컴파일만 했다(실제 open은 #[ignore]); W/Linux 실행·NIC·타임스탬프·드라이버 통계·Wireshark 교차 확인은 미확인. 임의 blocking Read/사용자 소스의 timeout 불준수는 safe Rust로 강제 취소할 수 없다.
- 처리량: **macOS arm64, 캡처 제외 경로 실측**, release, 가상 classic pcap Max replay→queue→consumer, 각 200,000개. 입력 전체 누적 없이 packet template 1개; queue=8 MiB/4096, decode/journal/UI 제외. 측정 명령은 10분 이내(최초 release build 6.13초, 최종 실행 약0.18초).

| frame bytes | 소비 건수 / 기록 drop | wall ms | 소비 pps / ingress pps |
|---|---|---|---|
| 64 | 166,566 / 33,434 | 82.517 | 2,018,574 / 2,423,753 |
| 1500 | 200,000 / 0 | 99.969 | 2,000,625 / 2,000,625 |

- 단발 burst 수치이며 무손실/선속도 보증이 아니다. 출력: `.worktrees/eth/crates/loom-eth-capture/throughput.stdout.txt`; 재현: 같은 crate의 `examples/throughput.rs`·README.md.
- CORE workspace.dependencies 추가 요청: etherparse =0.21.0, pcap =2.5.0(optional live), loom-eth-pcapng path =0.1.0. CORE worker SDK 접합은 다음 단계.
