# UART 계획 스파이크 (R1 Sol)

구현 코드가 아닌 PTY 타당성 검증이다. 실제 UART 장치를 열지 않는다.
전역 설치나 사용자 설정 변경이 필요 없다. macOS/Linux 전용이며 Windows에서는 실행하지 않는다.

```sh
python3 -m venv spikes/uart/.venv
spikes/uart/.venv/bin/python -m pip install --no-cache-dir pyserial==3.5
spikes/uart/.venv/bin/python -u spikes/uart/pty_probe.py
```

검증: `os.openpty()` slave를 pyserial로 열어 0x00–0xff 양방향 보존,
두 번째 `exclusive=True` open, BREAK API, DTR/RTS ioctl, 별도 producer가
쓰는 4 MiB 바이트열의 수신 및 SHA-256을 확인한다. bulk producer/reader는
각 10초 제한이다. 작은 왕복 블록은 256바이트로 두었다. 첫 실험에서
reader 시작 전에 master에 4096바이트를 동기 write하면 macOS PTY 버퍼에서
대기했으므로, bulk는 생산자 스레드와 nonblocking/select를 쓴다.

2026-11-09 실제 결과: macOS 27.0.1 arm64 / Python 3.15.0 / pyserial 3.5.

| 항목 | 결과 |
|---|---|
| binary 왕복 | 방향마다 256바이트 일치 |
| 두 번째 exclusive open | Errno 35, lock 거절 |
| `send_break(0.02)` | API 성공, 403.190 ms 소요 |
| `break_condition=True/False` | API 성공, 합계 3.751 ms |
| `dtr=False`, `rts=False` | 각각 Errno 25, Inappropriate ioctl for device |
| bulk | 4,194,304바이트 일치 / 2.2499초 / 1.78 MiB/s |
| bulk SHA-256 | `2b07811057df887086f06a67edc6ebf911de8b6741156e7a2eb1416a4b8b1b2e` |

PTY는 baud로 속도를 제한하지 않는다. 이 수치는 IPC/다중 포트/실제 USB-UART
3 Mbaud 성능 검증이 아니다. BREAK 성공은 물리 신호나 요청한 지속시간의 보장이
아니다. POSIX exclusive는 협조적 flock 결과이며 비협조적 앱까지 막는다는 뜻이 아니다.
Python 3.11 지원은 별도 CI에서 검증해야 한다.

## R2 사실 확인

```sh
spikes/uart/.venv/bin/python spikes/uart/r2_fact_probe.py
```

[r2_fact_probe.py](r2_fact_probe.py)는 물리 장치 없이 새 PTY에서 다음을 확인한다.
실행 결과는 [r2-facts-result.json](r2-facts-result.json), 환경은 위와 같다.

- 첫 pyserial `exclusive=True` 이후 두 번째 `exclusive=True`는 Errno35로
  거절되지만, exclusive를 지정하지 않은 두 번째 open은 **성공**했다.
  pyserial의 POSIX 락을 TIOCEXCL/모든 앱 차단으로 해석하면 안 된다.
- Python `re.MULTILINE`의 `[#$] $`는 개행 없는 `root@dut:~# `에서 매치한다.
  `OK$`는 현재 chunk `OK`에서 매치하지만 뒤에 `AY`가 오면 매치가 아니다.
- `(a+)+$`와 32개 `a` 뒤 `!`의 조합은 평가 시작 후200 ms 안에 끝나지
  않아 격리된 child를 종료했다. 큰 로그 없이도 정규식 평가가 지연될 수 있다.
- `tcdrain`을80 ms sleep으로 mock한 **합성 시험**에서 write_timeout10 ms인
  pyserial의 `flush()`는88.073 ms 뒤 반환했다. 실제 CTS/HW 실험은 아니다.
- RX-only 8N1 기준16 MiB 보존시간은115200에서24.273분,
  921600에서3.034분이다. TX/메타데이터 overhead는 별도다.

R2에서는 R1 초안 두 파일을 변경하지 않았다. 이 코드는 계획 근거용이며
데몬·뷰어·입력 중재 기능을 구현하거나 그 성능을 측정한 것이 아니다.

## R3 regex timeout / GIL

```sh
spikes/uart/.venv/bin/python -m pip install --no-cache-dir regex==2026.11.9
spikes/uart/.venv/bin/python spikes/uart/r3_regex_probe.py
spikes/uart/.venv/bin/python spikes/uart/r3_regex_probe.py --child direct '(a+)+$' 32768
spikes/uart/.venv/bin/python spikes/uart/r3_regex_probe.py --child gil true
spikes/uart/.venv/bin/python spikes/uart/r3_regex_probe.py --child loop
spikes/uart/.venv/bin/python spikes/uart/r3_regex_probe.py --child clock
```

패턴당7회, GIL/asyncio 조건당5회 실행한다. timeout 예외는
`builtins.TimeoutError`였다. 작은 입력은 빠르게 no-match,32 KiB 입력과
모호한 alternation은 timeout에 도달했다. `concurrent=True`에서10 ms counter와
별도 asyncio heartbeat가 진행하고, False 대조군에서는 counter가 정지했다.
POSIX 구현의 timeout 검사는 C `clock()` 기반이므로50 ms CPU budget이
50 ms wall deadline은 아니다. 합성 CPU/wall 확인에서 중앙값은 각각
50.038/104.066 ms였다.

원시 결과: `r3-regex-results.json`, `r3-regex-default-results.json`,
`r3-regex-clock-results.json`. 상세 판단·표·공식 근거는
[R3 사실 확인](../../docs/plan/uart/r3-facts-sol.md)에 있다.
child의8초 watchdog은 실험 안전장치이며 제품 matcher 구현은 아니다.
