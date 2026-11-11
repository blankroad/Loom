# loom-bench

코어 지연·CLI 왕복 측정과 별도 프로세스 혼합 부하 soak를 제공한다. 결과의 실행 완료와 관문 통과는 구분한다. RSS 표본을 얻지 못하거나 실행 시간이 부족하면 P1 관문은 `판정 불가`다.

## 준비

```sh
cargo build --release --manifest-path /Users/ctmctm/workspace/Loom/.worktrees/core/Cargo.toml -p loom-bench -p loom-gateway -p loom-cli
```

`loomd`, `loom`은 loom-bench와 같은 target profile 디렉터리에서 찾는다. 별도 위치는 `LOOMD_BIN`, `LOOM_BIN` 또는 `CARGO_BIN_EXE_loomd`, `CARGO_BIN_EXE_loom`으로 지정한다. soak는 워크트리 target 아래 임시 LOOM_HOME을 생성하며 기존 사용자 데몬을 사용하지 않는다. CLI에는 `LOOM_NO_AUTOSTART=1`을 적용한다.

## 혼합 부하

개발 실행은 부하 구간 10분 이내로 제한한다. 소수 minutes는 짧은 점검용이다.

```sh
/Users/ctmctm/workspace/Loom/.worktrees/core/target/release/loom-bench soak --minutes 10 --profile mixed --out /Users/ctmctm/workspace/Loom/.worktrees/core/bench/results/soak-10m.json
```

30분 P1 관문은 리드가 기준 머신에서 같은 명령의 minutes를 30으로 바꾸어 별도로 실행한다. 이 개발 작업에서는 실행하지 않는다. minutes의 유효 범위는 0 초과 1440 이하이며 전체 시간열·송수신 장부·프로세스 추적·오류 목록 모두 상한을 갖는다.

- agent가 20 Hz로 echo를 쓰며 매 100번째 요청은 같은 idempotency 키로 다시 호출한다. 지연 시 밀린 송신을 무제한으로 몰아 보내지 않고 예정 슬롯을 건너뛴다.
- viewer 2개 중 정상 구독은 계속 읽고, 느린 구독은 2초 정지/2초 읽기를 반복하며 읽는 구간도 초당 4프레임으로 제한한다. 두 구독은 5분마다 마지막 echo의 stream cursor로 재연결한다. 서버가 연결을 닫으면 즉시 복구도 시도하고 오류를 집계한다.
- echo 입력은 16 KiB로 채널 링의 과거 구간과 느린 TCP 경로에 부하를 준다. 시작 전에 같은 임시 홈의 이전 데몬 세션에 작은 echo 하나를 기록·정상 종료하여 현재 링 밖의 기록을 준비한다. 측정 시작 이후에는 하나의 데몬 수명을 측정한다. 질의는 매초, 과거 cursor+echo 필터 질의는 5초마다 보내며 이전 세션과 현재 세션의 seq:1을 번갈아 사용한다. 이전 세션 질의는 10초마다 저널 경로를 확실히 거친다. 동일 label의 mark는 10초마다 보낸다. 결과의 `journal_source_runs`로 실제 저널 경로 실행 여부를 확인한다.
- 정상 viewer가 1분마다 STOP→release를 수행한다. agent 쓰기·CLI와 같은 짧은 gate로 겹침을 막는다. 이 권한은 임시 sim 데몬의 시험용 operate viewer에만 발급한다.
- CLI `loom --json echo send`는 10초마다 별도 프로세스로 실행한다. 부하 종료 후 송신 스레드를 먼저 정리하고 두 구독을 추가로 drain한 뒤 `core.shutdown`으로 데몬을 종료한다. 오류 경로의 Child guard도 자신이 시작한 데몬을 정리한다.

## 측정과 판정

RSS는 외부 `ps -o rss= -p PID`의 KiB를 MiB로 바꾼다. 스레드는 Linux ps nlwp, macOS ps -M의 PID 행 수를 사용한다. Linux FD는 /proc/PID/fd, macOS FD는 60초마다 lsof의 숫자 FD만 센다. RSS·스레드 표본은 1초마다 시도하며 명령은 각각 2초 상한을 둔다. 전체 process tree는 ps로 탐색하고 PID와 시작 시각으로 worker의 수명을 나눈다. 프로세스 탐색 실패·32개 상한 초과는 완전성을 보장하지 못하므로 관문을 통과시키지 않는다. Windows 자원 측정은 P2다.

수명별 첫 120초를 제외하고 선형 기울기(시간당), 처음/마지막 5분 중앙값 차이, FD·스레드 증가 추세와 baseline+4 범위를 출력한다. 짧은 실행에서는 두 5분 창의 겹침을 표시한다. 30분 이상 실행하고 각 수명의 워밍업 이후 RSS 표본이 90% 이상 있으며 관측 span이 600초 이상일 때만 RSS 중앙값 증가 ≤ 10 MiB를 판정한다. 짧은 worker 수명도 자료가 부족하면 판정 불가다. 이는 §9.6의 전체 24시간 관문을 대신하지 않는다.

성공 응답을 받은 고유 echo를 비트 장부에 기록하고 실제 수신 집합과 비교한다. idempotency 재응답은 기대 echo를 늘리지 않는다. batch의 gap 요약은 우선 이벤트 `display.gap`과 중복 합산하지 않는다. 정상 viewer의 누락과 정확한 gap count가 일치하는지 확인하며, unknown gap·성공 응답 없는 수신·중복·record/source gap도 별도로 노출한다. 느린 viewer가 종료되거나 재구독 시 오래된 커서가 거절되면 손실 수를 확정할 수 없으므로 해당 수치를 숨기지 않는다.

JSON에는 전체 시간열, 측정 오류, 부하 집계, 분석과 판정이 있다. 실행 중에는 같은 위치의 `.partial.json`을 매분 갱신하고 완료 후 정리한다. 데몬 stderr는 옆의 `.daemon.log`에 남긴다. 토큰·쿠키·bootstrap 코드는 결과에 넣지 않는다. 샌드박스가 ps를 차단하면 RSS·스레드는 null이며, 이 경우 30분을 실행해도 관문은 판정 불가다. Linux와 제한 없는 macOS의 실제 자원 명령 검증은 해당 환경에서 수행해야 한다.

## 기존 J3 기준 측정

```sh
/Users/ctmctm/workspace/Loom/.worktrees/core/target/release/loom-bench --scenario all --n 200 --rate 1000 --secs 10 --cli-runs 30 --out-dir /Users/ctmctm/workspace/Loom/.worktrees/core/bench/results/baseline-1
```

서로 다른 out-dir로 3회 실행하면 결과를 덮어쓰지 않는다. J3 코어 지연은 인프로세스 echo의 공통 Instant 기준이며, soak의 외부 프로세스 자원 측정과 다른 시험이다. Rust WS 수신 시각을 측정하며 브라우저 렌더·실제 장비 지연은 포함하지 않는다.
