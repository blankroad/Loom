# P1 core-sol B1·B2 보고

작업 위치는 `/Users/ctmctm/workspace/Loom/.worktrees/core`, 기준은 `2b261d9`다. 코드·사용법은 loom-bench 안에서만 수정했고, 측정 결과는 워크트리 bench/results/p1-sol-b 아래에 저장했다. 이 보고는 지시대로 저장소 루트에 작성했다. 커밋·스테이징하지 않았다. 동시 작업자의 gateway·echo·Cargo.lock 변경은 보존했다.

## B1. 별도 프로세스 soak

`loom-bench soak --minutes N [--profile mixed] [--out JSON]`을 추가했다. 기존 J3 옵션은 유지한다. 구현은 loom-bench의 soak/{mod,process,rpc,metrics,state}.rs로 나눴고, 한국어 README에 실행·측정·판정 방법을 적었다. 외부 의존성·루트 Cargo.toml 수정은 없다.

- 워크트리 target 아래 임시 LOOM_HOME으로 빌드된 실제 loomd를 실행한다. LOOMD_BIN/LOOM_BIN, CARGO_BIN_EXE 이름 또는 동일 target profile의 형제 바이너리에서 찾는다. 데몬 준비를 discovery PID와 인증된 core.hello로 확인하고, 부하 종료 후 core.shutdown·wait로 정리한다. 오류 경로도 Child guard가 자신이 시작한 데몬을 kill/wait한다.
- agent 20 Hz, 16 KiB echo 입력, 매 100번째 요청의 동일 idempotency 키 재호출을 수행한다. 지연된 송신 슬롯은 건너뛰므로 catch-up 폭주를 만들지 않는다. 성공한 고유 송신 집합과 두 viewer의 실제 수신 집합을 비트 장부로 비교한다. 멱등 재응답을 새 echo로 세지 않는다.
- viewer는 정확히 2개다. 정상 구독은 계속 읽는다. 느린 구독은 2초 정지/2초 읽기이며 읽는 구간도 4프레임/s로 제한한다. 5분마다 마지막 echo cursor로 재연결하고, 서버가 연결을 닫아도 복구를 시도한다. 정상 viewer는 임시 sim 데몬의 operate 권한으로 1분마다 STOP→release를 수행한다. 짧은 공유 gate로 agent·CLI 쓰기와 겹치지 않게 한다.
- 매초 core.events.query, 5초마다 seq:1+echo.data 필터의 과거 구간 질의, 10초마다 동일 label의 core.mark, 10초마다 별도 CLI 프로세스 loom --json echo send를 실행한다. 시작 전에 같은 임시 홈의 이전 세션에 echo 하나를 기록하고 정상 종료하여 **현재 링 밖의 저널 자료**를 준비한다. 과거 질의는 이전 세션과 현재 세션을 번갈아 사용하므로 이전 세션의 저널 경로를 10초마다 확실히 거친다. 준비 세션의 PID·부하는 본 측정 시간열에 섞지 않는다. CLI는 LOOM_NO_AUTOSTART=1로 이미 실행된 임시 데몬에만 붙는다.
- 외부 ps로 1초마다 RSS·스레드를 측정하고 프로세스 트리를 탐색한다. worker를 포함해 PID+시작 시각으로 수명을 분리한다. RSS는 ps의 KiB→MiB, Linux FD는 /proc/PID/fd, macOS FD는 60초마다 lsof의 숫자 FD를 사용한다. 명령은 각각 2초 상한, 프로세스 추적은 32개 상한이다. 탐색 실패·측정 실패는 0이 아닌 null과 오류 집계로 남긴다.
- PID 수명별 첫 120초를 제외하고 회귀 기울기(/h), 첫/마지막 5분 중앙값 차이, FD·스레드 증가 추세와 baseline+4를 계산한다. 5분 창이 겹치면 명시한다. 30분 이상 실행·충분한 RSS 표본·독립 창이 있어야 RSS 증가 ≤ 10 MiB 관문을 판정한다. Windows 자원 측정은 P2다.
- 시간열·송수신 장부·오류 목록 모두 유한하다. 최종 구현은 WS 송신 버퍼 256 KiB, 수신 메시지·프레임 8 MiB 상한도 명시했다(채널 링 resume를 수용). 실행 중 매분 partial JSON, 완료 후 최종 JSON과 옆의 daemon.log를 남긴다. 토큰·쿠키·bootstrap 코드는 결과에 기록하지 않는다. 부하 종료 후 송신 스레드를 먼저 정리하고 viewer를 drain하여 순간적인 미수신을 최종 손실로 세지 않는다.

### 개발 실행

3초 smoke와 66초 점검, 사전 세션 없는 10분 비교 측정 후 사전 세션을 포함한 최종 release 10분 실행을 완료했다. 비교 측정은 정상 수신 12,059/12,059·누락 0, 느린 display.gap count 134, 쓰기 실패 0이었으나 journal source 질의가 0회였다. 그래서 현재 링 밖의 기록을 미리 준비하여 최종 측정에서는 저널 경로까지 검증했다. 사전 세션을 포함한 3초 smoke에서 실제 journal source 1회·질의 실패 0을 먼저 확인했다. 측정 대상 loomd는 실제 별도 프로세스이며 echo worker는 in_process다. IPC worker 자원 측정은 프로세스 탐색 구현과 수명 분리 단위 시험까지만 검증했다.

**환경 제약:** 이 샌드박스에서 ps는 `Operation not permitted (os error 1)`로 차단된다. RSS·스레드·worker 탐색은 측정 불가이며 P1 RSS 관문은 판정 불가다. lsof는 실행되어 FD 표본은 얻는다. 수치 없는 RSS를 0 또는 PASS로 바꾸지 않았다.

최종 결과 파일은 `/Users/ctmctm/workspace/Loom/.worktrees/core/bench/results/p1-sol-b/soak-10m-journal.json`이다. 실제 부하 구간 600.002초, PID 96125의 시간열 600개, 워밍업 제외 표본 480개다. 부하 10분에 준비·종료 정리 시간은 별도로 붙는다. 30분 관문은 실행하지 않았다.

| 지표 | 최종 10분 결과 | 판정·해석 |
| --- | --- | --- |
| RSS 기울기 MiB/h | 측정 불가 | ps 권한 차단 600회 |
| RSS 마지막/처음 5분 중앙값 차이 | 측정 불가 | **P1 RSS 관문 판정 불가** |
| FD(워밍업 제외) | 21–22개, 처음/마지막 중앙값 21→21, 차이 0 | 8개 표본, 회귀 −2.836 FD/h, 증가 추세 없음·baseline+4 이내 |
| 스레드 | 측정 불가 | ps 권한 차단 600회 |
| 정상 viewer | 12,060/12,060 수신, 누락·중복·gap 0 | 누락=정확 gap count 정합, record/source gap 0 |
| 느린 viewer | 969/12,060 수신, 누락 11,091 | 아래 한계 참조 |
| 느린 display.gap | 이벤트 4개, 정확 count 합계 132 | heartbeat drop 합계 243; batch 중복 요약 22는 합산하지 않음 |
| agent 쓰기 | 12,000회, 실패 0 | 약 20 Hz |
| idempotency 재요청 | 120회, 실패 0 | 정상 viewer 중복 수신 0 |
| CLI 프로세스 | 60회, 실패 0 | 자동 기동 제외 |
| events.query | 601회, 실패 0 | 과거 cursor 121회, 실제 journal source 61회 |
| core.mark | 61회 | 동일 label 사용 |
| STOP→release | 10회 | 정상 viewer가 수행 |
| 재연결 | 정상 2회, 느림 5회 | 느림 연결 종료 5회 기록 |

워밍업 제외 후 관측 시간은 약 8분이므로 처음/마지막 5분 창이 겹친다. JSON도 windows_overlap=true로 표시한다. FD의 짧은 회귀를 장기 누수 부재의 증거로 보지 않는다. 실행 당시 추가 uptime 기록은 load averages 2.20 / 3.93 / 6.06이다.

### 확인된 한계와 소유자 검토 사항

- 정상 viewer의 손실·gap 정합은 통과했다. 그러나 느린 viewer의 미수신 11,091개 중 정확한 display.gap count는 132개뿐이며 미설명 차이는 10,959개다. 연결 종료·재구독을 포함한 느린 경로 전체의 손실 정합은 **통과하지 않았다**. unknown gap으로 대체하거나 카운터를 맞추어 쓰지 않았다. 24시간 전체 soak 관문 통과로 보고하지 않는다.
- 원인 후보(소스에 근거한 추론): gateway hub의 range 및 subscribe는 각각 hub/mod.rs:853·1068에서 모든 링의 전역 최솟값을 oldest로 사용한다. 작은 core 링에 오래된 seq가 남고 echo 채널 링만 먼저 비워지면, 오래된 echo cursor도 in_ring으로 판단하여 저널 복구·gap 표시를 놓칠 수 있다. 사전 세션 없는 측정에서 오래된 echo 질의 121회가 모두 ring source였던 관찰과 일치한다. gateway 소유자가 채널/필터별 보존 경계와 종료 후 재구독 손실을 확인해야 한다. 범위 밖 gateway는 수정하지 않았다.
- 최종 측정의 실제 journal source 61회는 **이전 세션** 자료 조회의 증거다. 동일 epoch에서 채널 링이 비워진 경우의 저널 fallback 전체가 정상이라는 증거로 확대하지 않는다.
- RSS·스레드·IPC worker 탐색은 샌드박스 제약으로 검증하지 못했다. Windows 자원 측정은 P2이고 Linux 명령도 이 환경에서 실측하지 않았다. 30분 P1 관문은 ps가 허용되는 기준 머신에서 리드가 실행해야 한다.

## B2. release J3 재측정 3회

동일 머신 macOS/aarch64에서 release로 실행했다. 각 회 idle 200건, burst 1,000 ev/s×10초=10,000건, CLI 왕복 30회다. 각 실행은 약 15초이며 별도 run-1/run-2/run-3 디렉터리에 저장하여 덮어쓰지 않았다. CLI는 동일 profile의 실제 loom 바이너리다.

| 회차·측정 | p50 ms | p95 ms | p99 ms | max ms | 표본 | 누락·실패 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 idle | 0.176 | 1.808 | 4.075 | 5.595 | 200 | 0 |
| 2 idle | 0.183 | 6.005 | 15.390 | 20.198 | 200 | 0 |
| 3 idle | 0.102 | 0.196 | 0.300 | 0.341 | 200 | 0 |
| 1 burst | 1.786 | 5.426 | 10.082 | 19.719 | 10,000 | 0 |
| 2 burst | 1.615 | 3.807 | 7.652 | 20.724 | 10,000 | 0 |
| 3 burst | 1.505 | 2.527 | 8.460 | 41.040 | 10,000 | 0 |
| 1 CLI | 11.341 | 17.951 | 18.776 | 18.776 | 30 | 0 |
| 2 CLI | 6.194 | 8.169 | 10.745 | 10.745 | 30 | 0 |
| 3 CLI | 10.180 | 40.133 | 40.138 | 40.138 | 30 | 0 |

| 측정 | p50 범위 ms | p95 범위 ms | p99 범위 ms |
| --- | ---: | ---: | ---: |
| core idle | 0.102–0.183 | 0.196–6.005 | 0.300–15.390 |
| core burst | 1.505–1.786 | 2.527–5.426 | 7.652–10.082 |
| CLI 왕복 | 6.194–11.341 | 8.169–40.133 | 10.745–40.138 |

측정 시작의 uptime 기록은 load averages 3.41 / 8.60 / 7.39, 사용자 세션 6개였다. 다른 에이전트의 개발·빌드도 진행 중이므로 기준 머신 확정치가 아니다. idle·CLI 꼬리의 변동이 크다. J3 코어 수치는 인프로세스 공통 Instant를 사용한 Rust WS 수신 지연이고, IPC·브라우저 렌더·실제 장비 지연은 포함하지 않는다.

결과 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core/bench/results/p1-sol-b/`. run-1~3 각각의 all-macos-2026-11-11.json과 실행 로그, machine-load.txt를 보존했다. 결과 파일 커밋 여부는 리드가 판단한다.

## 품질·한계

- cargo fmt --all --check, cargo clippy -p loom-bench --all-targets -- -D warnings, cargo test -p loom-bench 통과(시험 5개, 실패·무시 0). git diff --check도 통과했다. 사전 기록 준비와 최종 버퍼 상한을 포함한 release 빌드도 통과했다. 10분 측정은 최종 WS 방어 상한 명시 직전 빌드이며, 상한 보강 후에는 최종 빌드·clippy·시험과 짧은 실제 프로세스 smoke로 확인했다. final-smoke.json은 정상 수신 60/60·누락 0, 저널 조회 1회, 쓰기·질의 실패 0이다. 기존 10분 결과를 재작성하지 않았다.
- 현재 단위 시험 5개: 워밍업 제외·PID 재사용 분리, 누락/짧은 실행의 거짓 PASS 차단과 실제 RSS 증가 FAIL, 평탄한 FD·겹친 창, 자식 프로세스 트리, 멱등 수신 장부·gap 요약 중복 방지다.
- 30분 P1 관문은 이 작업에서 실행하지 않는다. 제한 없는 측정 환경에서 리드가 실행해야 한다. Linux 자원 명령과 Windows 런타임은 이 macOS 작업에서 실측하지 않았다.
