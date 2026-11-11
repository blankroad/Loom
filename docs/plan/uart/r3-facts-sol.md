# UART R3 — regex timeout / GIL 사실 확인

2026-11-09, 15분 이내 사실 확인. 제품 구현·plan.md 수정 없음. 목적은 expect 평가가 핵심 루프의 사용자 실시간 표시와 개입을 막지 않는지 확인하는 것이다.

환경: **macOS 27.0.1 arm64 / CPython 3.15.0 / GIL enabled=True**. [PyPI 최신 안정판 regex 2026.11.9](https://pypi.org/project/regex/2026.11.9/)를 확인해 `spikes/uart/.venv`에만 설치했다. 첫 artifact 배포 시각은2026-11-09 00:39:39 UTC, Python≥3.11, 라이선스 **Apache-2.0 AND CNRI-Python**. Windows/Linux·Python3.11에서는 이번에 측정하지 않았다.

재현 시 `P=spikes/uart/.venv/bin/python`, `S=spikes/uart/r3_regex_probe.py`로 둔다. 아래 timeout은 **초 단위**다. 입력은 불변 built-in `str`이고 pattern은 미리 compile했다. `direct`는 요청 예시 그대로 `regex.search(..., timeout=0.05)`를 호출하며 `concurrent`를 생략한다. 각 패턴7회, GIL/asyncio 시험은 각5회다. 경과 시간은 `perf_counter()` 기준으로 측정했다.

```sh
spikes/uart/.venv/bin/python -m pip install --no-cache-dir regex==2026.11.9
P=spikes/uart/.venv/bin/python
S=spikes/uart/r3_regex_probe.py
ALT='^(a|aa)+$'
"$P" "$S"                       # 주요 9개 case, JSON 출력
"$P" "$S" --child direct '(a+)+$' 32
"$P" "$S" --child clock
```

## 1. timeout 발동과 미발동

| 항목 | 결과: 7회 중앙값 [최소–최대], wall ms | 재현 명령 | 확신도 |
|---|---|---|---|
| 요청 예시 `(a+)+$`, `'a'*32+'!'`, timeout=.05 | **7/7 no_match**, timeout 미발동. **0.215 [0.205–0.294]** | `"$P" "$S" --child direct '(a+)+$' 32` | 높음, 해당 환경·입력 |
| Opus 계열 `^(\w+\s?)*$`, `'a'*24+'!'`, timeout=.05 | **7/7 no_match**, timeout 미발동. **0.009 [0.009–0.116]** | `"$P" "$S" --child direct '^(\w+\s?)*$' 24` | 높음 |
| `(a+)+$`, `'a'*32768+'!'`, timeout=.05 | **7/7 builtins.TimeoutError**, **51.633 [50.171–59.172]** | `"$P" "$S" --child direct '(a+)+$' 32768` | 높음 |
| `^(\w+\s?)*$`, `'a'*32768+'!'`, timeout=.05 | **7/7 builtins.TimeoutError**, **50.485 [50.155–52.130]** | `"$P" "$S" --child direct '^(\w+\s?)*$' 32768` | 높음 |
| 짧은 입력에서도 폭주하는 대안 `^(a\|aa)+$`, `'a'*40+'!'`, timeout=.05 | **7/7 builtins.TimeoutError**, **50.180 [50.030–52.717]** | `"$P" "$S" --child direct "$ALT" 40` | 높음 |
| 위3개 큰/모호한 패턴, **concurrent=True** | 모두 **7/7 builtins.TimeoutError: regex timed out**. 중앙값은 순서대로 **104.330 /70.363 /61.650**, 최대 **141.462 /215.831 /85.532** | `"$P" "$S" --child pattern '(a+)+$' 32768`, `... pattern '^(\w+\s?)*$' 32768`, `"$P" "$S" --child pattern "$ALT" 40` | 높음, 별도 배치 측정 |

작은 예에서 timeout이 안 난 것은 **호출이 빨리 끝났기 때문**이며 timeout 기능이 무효라는 뜻이 아니다. 긴 입력과 모호한 alternation에서는 실제 발동했다. 예외는 별도 `regex.TimeoutError`가 아니라 **builtins.TimeoutError**로 잡는다.

**중요한 제한: `timeout=.05`를 wall deadline 50 ms로 쓰면 안 된다.** 위 두 배치는 시스템 부하·스케줄링 조건이 통제되지 않았으므로 concurrent=True 자체가 지연을 늘렸다고 해석하지 않는다. 선택 버전 sdist의 `src/_regex.c`는 timeout을 `clock() - state->start_time`으로 검사한다. POSIX `clock()`은 process CPU time이다. [소스 확인 기록](r3-regex-source-sol.json), [POSIX clock 정의](https://pubs.opengroup.org/onlinepubs/9799919799/functions/clock.html).

| 추가 확인 | 결과 | 재현 명령 | 확신도 |
|---|---|---|---|
| `^(a\|aa)+$`,40개 a+!, concurrent=True, timeout=.05, CPU/wall 동시 계측7회 | CPU 중앙값 **50.038 ms**, wall 중앙값 **104.066 ms**; wall 범위 **82.445–120.749 ms**. 모두 TimeoutError | `"$P" "$S" --child clock` | 높음, POSIX 구현과 실측 일치 |

process CPU는 다른 스레드의 CPU 사용도 포함하므로 matcher 전용 CPU budget라고도 가정하지 않는다. Windows의 clock 동작·timeout 지연은 별도 측정 필요다. compile 비용은 matching timeout 시험에 포함하지 않았다.

## 2. concurrent=True와 다른 스레드 / asyncio 진행

같은 `^(a|aa)+$`와40개 a+!를 사용하고 match timeout=.25로 충분히 길게 실행했다. 별도 counter thread가10 ms마다 tick을 기록했다. “최대 gap 중앙값”은 각 run의 전체 tick 간 최대 간격을 구한 뒤5회 중앙값을 낸 것이다. 모든 run은 TimeoutError로 끝났다.

| 항목 | 결과 | 재현 명령 | 확신도 |
|---|---|---|---|
| concurrent=True, GIL 있는 CPython | match 중 tick **중앙값28회 [20–37]**, 최대 gap 중앙값 **26.529 ms**. match wall 중앙값432.982 ms | `"$P" "$S" --child gil true` | 높음: matcher가 실행되는 동안 다른 Python thread가 실제 진행 |
| concurrent=False, 대조군 | match 중 tick **중앙값0회 [0–1]**, 최대 gap 중앙값 **300.056 ms**. match wall 중앙값295.314 ms | `"$P" "$S" --child gil false` | 높음: 같은 입력에서 대조군 정지 관측 |
| concurrent 생략, immutable str | match 중 tick **중앙값23회 [20–26]**, 최대 gap 중앙값 **18.983 ms** | `"$P" "$S" --child gil default` | 높음: 기본 str 매칭의 GIL 해제 설명과 일치 |
| **전용 ThreadPoolExecutor(max_workers=1)**에서 concurrent=True match, main asyncio heartbeat10 ms | 5회 match 중 heartbeat **41/35/61/43/31회** 진행. 전체 heartbeat gap 중앙값 **12.048 ms**, 최대 **29.233 ms** | `"$P" "$S" --child loop` | 높음: 이 합성 loop의 진행. 실제 TUI/8포트 성능 확신도는 미검증 |

[regex 공식 설명](https://pypi.org/project/regex/2026.11.9/)도 built-in immutable 문자열 매칭 중 GIL 해제와 `concurrent=True`를 설명한다. 변경 가능한 입력을 동시에 수정하면 동작이 정의되지 않으므로, ring의 mutable 저장소를 직접 넘기지 않고 **크기 제한된 immutable snapshot**을 넘긴다.

`concurrent=True`만 붙여 **event loop thread에서 동기 search를 호출하는 것은 해결책이 아니다**. 다른 OS thread는 실행될 수 있어도 그 asyncio thread는 search가 반환할 때까지 자신의 callback을 처리하지 못한다. 반드시 전용 worker에서 compile/search하고 결과만 loop에 돌려준다.

## 3. 결론과 통합 조건

**결론 한 줄: bounded immutable 입력·literal 기본·전용 worker thread+regex(timeout, concurrent=True)는 이번 시험에서 loop 정지를 막았으므로 subprocess는 기본 필수가 아니지만, “절대 멈추지 않음/50 ms wall 이내 강제 종료” 보장은 성립하지 않으며 강제 취소·비신뢰 compile·C 확장 장애 격리가 필요하면 subprocess를 유지한다.**

통합 시 붙일 최소 조건:

- literal은 regex 엔진을 거치지 않는다. regex pattern/window·동시 matcher·대기 queue를 제한하고 compile도 event loop 밖에서 수행한다. timeout은 matching 보호이며 compile/memory 비용 전체를 제한하지 않는다.
- 별도 monotonic wall deadline으로 CLI expect의 총 대기/큐 대기를 제한한다. 만료 결과는 폐기하고 loop는 계속 반응한다. `asyncio.wait_for()`나 future.cancel()만으로 이미 실행 중인 thread의 C 매칭을 강제 종료할 수 있다고 쓰지 않는다.
- worker가 오래 점유되면 새 작업을 bounded queue에서 거절/만료시키고 상태를 표시한다. hard cancellation이 요구되는 matcher만 process backend로 격리한다. 프로세스 종료도 OS 스케줄러의 hard realtime 보장과는 다르다.
- 본 결과는 **GIL 점유로 인한 loop 정지 회피의 근거**다. UART RX→실제 뷰어 표시<100 ms, 다중 포트·느린 disk·다수 동시 regex의 성능 검증은 별도 M1/M3 gate다.

재료: [주요 원시 결과](../../../spikes/uart/r3-regex-results.json), [요청 API 그대로의 결과](../../../spikes/uart/r3-regex-default-results.json), [CPU/wall 결과](../../../spikes/uart/r3-regex-clock-results.json), [재현 코드](../../../spikes/uart/r3_regex_probe.py). 코드의 child watchdog은 **실험 harness 보호용**이며 production에서 subprocess가 꼭 필요하다는 근거로 세지 않았다.
