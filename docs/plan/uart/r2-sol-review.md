# UART R2 교차 리뷰 — Sol → Opus R1

검토일: 2026-11-09. 대상은 [r1-opus.md](r1-opus.md), 비교안은 [r1-sol.md](r1-sol.md)이다. 최우선 [핵심 원칙](../02-core-principle.md), [공통 브리프](../00-brief.md), [UART 브리프](brief.md) 및 R2 지시를 기준으로 했다. **계획 리뷰이며 제품 구현 완료 판정이 아니다. R1 두 파일은 수정하지 않았다.**

표기: **P0**=잘못된 대상 송신·입력 혼합·출처/승인 왜곡을 막기 위해 통합 전에 결정, **P1**=M1 수용 전에 수정, **P2**=후속 단계에 명시. 사실 오류와 설계 취향을 구분한다. 외부 근거는 공식 문서·선택 버전 소스·PyPI를 확인했다. [외부 근거 요약](r2-sol-source-evidence.json), [R2 검증 코드](../../../spikes/uart/r2_fact_probe.py), [실행 결과](../../../spikes/uart/r2-facts-result.json), [실행법](../../../spikes/uart/README.md)을 남겼다.

판정의 중심: Opus의 **shared daemon + PTY M1 + live attach + 사용자 개입** 방향은 채택할 만하다. 다만 **3초 입력 가드, 원시 패스스루와 상태바 결합, 화면 지연 측정, blocking flush/regex, 재연결 신원**을 현재 설명대로 구현하면 핵심 루프 수용 기준을 충족했다고 할 수 없다.

## 1. 틀린 사실

### F1. `exclusive=True`를 TIOCEXCL 및 모든 외부 open 차단으로 설명한 것은 오류 — P0

- **대상**: Opus §3.12, §8. §3.12는 POSIX `exclusive=True`를 TIOCEXCL이라 쓰고 pyserial/screen의 별도 open 자체가 실패한다고 단정한다.
- **확인 사실**: pyserial **v3.5** POSIX 코드는 `fcntl.flock(fd, LOCK_EX | LOCK_NB)`를 사용한다. 이는 협조적 advisory lock이며 TIOCEXCL 호출이 아니다. [선택 버전 소스](https://github.com/pyserial/pyserial/blob/v3.5/serial/serialposix.py#L381).
- **재현**: 이번 macOS PTY에서 첫 `exclusive=True` 후 두 번째 `exclusive=True`는 Errno35로 실패했지만, exclusive를 지정하지 않은 두 번째 pyserial open은 **성공**했다. [결과](../../../spikes/uart/r2-facts-result.json). screen 자체는 이번에 실행하지 않았으므로 screen의 모든 버전이 성공한다고도 단정하지 않는다.
- **수정**: “데몬 내부 registry·단일 RX 소유·스킬의 우회 금지 + POSIX 협조적 flock”으로 표현한다. 외부 프로그램까지 강제 차단하려면 OS별 추가 배타 정책을 별도로 검증해야 한다. Windows COM share-mode0 배타 설명은 맞다. 내부 Transport 비공개화도 동일 OS 사용자 임의 Python 실행을 강제로 차단하는 보안 경계는 아니다.
- **확신도: 높음** — v3.5 소스와 로컬 반례 일치. Linux 실험은 하지 않았으나 같은 POSIX 구현에서 flock을 쓰는 사실은 확인했다.

### F2. Python `$`의 의미 설명이 반대다 — P1

- **대상**: Opus §4.5, §12 T07, §13. “`$`는 현재 버퍼 끝이 아니라 줄 끝이므로 버퍼 끝도 줄 끝으로 취급하는 처리가 필요”하다는 설명.
- **확인 사실**: Python `re`의 `$`는 원래 문자열 끝에서 매치하고, MULTILINE이면 개행 앞에서도 매치한다. [Python3.11 문서](https://docs.python.org/3.11/library/re.html#regular-expression-syntax).
- **재현**: `re.search(r'[#$] $', 'root@dut:~# ', re.MULTILINE)`는 개행 없이도 매치했다. `OK$`는 chunk=`OK`에서 매치하지만 최종 문자열=`OKAY`에서는 매치하지 않았다.
- **수정**: 별도 “버퍼 끝=줄 끝” 규칙을 제거한다. 실제 쟁점은 **아직 완결되지 않은 스트림에 대한 조기 매치**, 프롬프트 false positive, ANSI/UTF-8 상태·원시 offset 매핑이다. 50 ms 정적 대기는 heuristic일 뿐 명령 완료 증명이 아니며, literal·profile prompt·선택 settle window의 의미를 명시한다.
- **확신도: 높음** — 공식 문서와 짧은 실행 확인. Python3.11 문서의 의미를 확인했고 로컬 실행 환경은3.15다.

### F3. `stop=1.5`, `dsrdtr`, BREAK/DTR/RTS를 OS 공통 기능처럼 제시하면 실제 동작과 다르다 — P1

- **대상**: Opus §1 범위1, §4.2 공통 open 옵션, §8 신호 “지원” 표, §10.6 리셋 방지 설명.
- **확인 사실**: pyserial 문서는 **POSIX1.5 stop bits가2로 fallback**, 일부 플랫폼의 **dsrdtr가 조용히 무시될 수 있음**, open 시 **OS/driver가 DTR/RTS를 assert하거나 glitch를 만들 수 있음**을 명시한다. open 전 속성 지정은 가능하지만 무조건 reset 방지가 되지는 않는다. [공식 API/상수 문서](https://pyserial.readthedocs.io/en/latest/pyserial_api.html).
- **실험 범위**: R1 PTY에서는 BREAK API가 성공했지만20 ms 요청이403.190 ms 걸렸고, DTR/RTS는 각각 unsupported ioctl Errno25였다. 이는 **PTY의 결과**이며 실제 macOS UART 신호가 불가하다는 반례로 쓰면 안 된다. [R1 결과](../../../spikes/uart/README.md).
- **수정**: 플랫폼/driver capability 표로 바꾸고 POSIX1.5는 명시 거절 또는 effective=2와 경고를 반환한다. dsrdtr는 검증 전 비범위가 간단하다. 신호는 “API 경로 있음, 물리 동작·길이·극성은 HW gate”로 제한한다. ESP32 예시를 `open_dtr=false → 리셋 방지 보장`처럼 설명하지 않는다.
- **확신도: 높음** — 문서상 제한은 확인. 특정 보드의 reset waveform은 미확인이다.

### F4. 16 MiB ring의 보존시간 계산이 과대다 — P2

- **대상**: Opus §3.6의115200≈27분,921600≈3.4분.
- **계산**:8N1 RX-only에서16×2²⁰/(baud/10)/60은 각각 **24.273분**, **3.034분**이다. TX도 같은 ring에 넣으면 보존시간은 더 짧다. event/object overhead까지16 MiB에 포함하면 추가 감소한다.
- **수정**: 수치를 교체하고 RX-only·payload-only 가정을 붙인다. 운영 화면에서는 추정 분량 대신 실제 earliest/latest offset과 retained age를 보여준다.
- **확신도: 높음** — 산술 확인, [probe 결과](../../../spikes/uart/r2-facts-result.json).

### 오류로 단정하지 말아야 할 검증 결과

| 검토 주장 | 이번 확인 | 리뷰 반영 |
|---|---|---|
|Opus §7 최신 버전 |pyserial3.5, asyncio0.6, fast0.16, websockets17.4, aiohttp3.14.5, FastAPI0.162.2, uvicorn0.54.0, msgspec0.22.0, Textual8.2.8, prompt_toolkit3.0.53, urwid5.1.0, pyte0.8.2, pexpect4.9.0, xmodem0.5.0, platformdirs4.14.5 모두 PyPI 응답과 일치 |버전 오류를 만들어 지적하지 않는다. FastAPI/uvicorn은 날짜도 각각2026-11-07/2026-09-25로 보강 가능. [PyPI 근거](r2-sol-source-evidence.json) |
|Textual `App.suspend()` |8.2.8에 실제 context manager가 있으며 Unix와 **Windows** 지원, Textual Web 미지원. WindowsDriver.can_suspend=True 확인 |“Windows라 불가”라는 반론은 틀리다. 다만 suspend 중 별도 raw console·reader·구독의 수명주기는 설계가 필요. [App 소스](https://github.com/Textualize/textual/blob/v8.2.8/src/textual/app.py#L4717), [Windows driver](https://github.com/Textualize/textual/blob/v8.2.8/src/textual/drivers/windows_driver.py) |
|websockets unix socket |공식 `websockets.asyncio.server.unix_serve` 존재, Unix 전용 |Opus의 `[추정]`을 확인으로 바꿀 수 있다. Windows 공통 endpoint로 쓸 수 있다는 뜻은 아님. [API](https://websockets.readthedocs.io/en/stable/reference/asyncio/server.html#websockets.asyncio.server.unix_serve) |
|platformdirs macOS runtime 경로 |4.14.5 소스가 `~/Library/Caches/TemporaryItems`를 기반으로 app/version을 붙임 |Opus의 경로 가정은 맞다. `~/.loom` 선택은 취향/운영 계약 문제. [선택 버전 소스](https://github.com/tox-dev/platformdirs/blob/4.14.5/src/platformdirs/macos.py#L203) |
|pyserial overrun 일반 API 부재 |v3.5 Windows 구현은 ClearCommError의 flags를 얻지만 in_waiting/read에서 호출자에게 error flags를 내보내지 않고 cbInQue/수신 바이트를 사용 |portable public API로 장치/driver byte loss를 완전히 안다고 할 수 없다는 Opus 경고는 타당. OS별 확장 가능성까지 부정하지 않는다. [v3.5 소스](https://github.com/pyserial/pyserial/blob/v3.5/serial/serialwin32.py#L253) |
|`xmodem`으로 YMODEM까지 가능한가 |확인한 upstream 소스는 YMODEM 지원을 pending으로 설명하고 mode가xmodem/xmodem1k |Opus도 미확인 표시했으므로 확정 오류로 세지 않는다. **YMODEM 완료를 해당 패키지 채택만으로 약속하면 안 됨**. 정확0.5.0 artifact 기능은 후순위 gate. [upstream](https://github.com/tehmaze/xmodem/blob/master/xmodem/__init__.py) |
|라이선스 |aiohttp PyPI SPDX는Apache-2.0 **AND** MIT, urwid는LGPL-2.1-only. pyte PyPI license는 빈 값이고 시도한0.8.2 COPYING URL은404 |aiohttp의 `/` 표기를 양쪽 고지 필요로 명확히 하고 urwid exact SPDX 보강. pyte LGPL-3.0 추정은 정확 tag/artifact LICENSE 확인 전 채택 근거로 확정하지 않는다. 빈 metadata가 무면허라는 뜻도 아님 |

## 2. 빠진 것

15개 절과 agent/user 시나리오는 모두 있다. 아래는 제목 누락이 아니라 **구현 시 실패하는 의미론·상한·시험의 빈칸**이다.

### B1. 3초 무입력 후 미완성 사용자 줄을 풀면 명령이 섞인다 — P0

**대상**: §3.7 규칙2와 상황표, §2.0④. 사용자가 Enter 없이 `ifconfig eth`를 쓰고4초 생각하면, 제안된 규칙상 agent의 `ip -br addr\r`를 보내 **`ifconfig ethip -br addr\r`**가 된다. same-send 원자성은 이미 장치에 들어간 사용자 prefix를 지우지 못한다. §3.7 표의 “10초 넘게 반쯤 친 상태→agent 명령 안 감”은3초 idle 규칙과 서로 모순이다.

추가할 것: line mode는 로컬 편집 중인 줄이 있으면 명시 yield/취소까지 agent 금지. raw mode는 줄 상태를 추측하지 말고 사용자 lease를 유지한다. 최소 반례 시험은 `prefix →4초 idle →agent send →assert 미전송`, 그리고 full-screen에서 Enter가 명령 완료가 아닌 경우다. 사용자 Enter 직후 agent를 자동 송신해도 앞 명령이 prompt로 돌아왔다는 보장이 없으므로, 입력권 양도와 장치 명령 완료를 별개로 다룬다.

### B2. send 원자성·대용량 전송·“사용자 Ctrl-C 즉시”의 우선순위가 충돌한다 — P0

**대상**: §3.5의 `serial.write + flush`, §3.7 규칙1/4/6, §3.11 키→송신20ms. 파일 전송 중 모든 사용자 키를 보류하면 Ctrl-C 개입이 즉시라는 요구와 맞지 않는다. 락 탈취 y/N 동안 첫 Ctrl-C를 멈추는지도 불명확하다.

`[확인]` pyserial v3.5 POSIX flush는 `tcdrain(fd)`, Windows는 `while out_waiting: sleep(0.05)`이며 **flush 자체의 deadline은 없다**. write_timeout을 줘도 flush deadline이 되는 것은 아니다. [POSIX](https://github.com/pyserial/pyserial/blob/v3.5/serial/serialposix.py#L666), [Windows](https://github.com/pyserial/pyserial/blob/v3.5/serial/serialwin32.py#L340). 이번 합성 실험에서는 write_timeout10ms인데 mock tcdrain80ms의 flush가88.073ms 걸렸다. 실제 CTS 실험은 아니다.

추가할 것: blocking write/drain은 event loop 밖, TX transaction을≤1KiB chunk로 분할, 사용자 takeover는 다음 chunk 전 잔여 전송을 취소. 이미 driver가 받은 바이트는 취소 불가능하므로 `written_bytes/remaining_bytes/delivery_unknown` 공개. Ctrl-C 우선 처리 예외를 명시하고 “절대 즉시” 대신 지원 부하에서의 목표와 취소 한계를 쓴다. `flush`를 명령마다 무조건 호출하지 말고 written/queued/drained를 구분한다. CTS low·write stall·파일 중 취소 때 **viewer와 RPC event loop가 계속 반응**하는 시험이 필요하다.

### B3. terminal passthrough + 한 줄 상태바 + echo tint는 일반 VT에 대해 격리되지 않는다 — P0

**대상**: §5.1–5.4, §3.11, M1/M3. DECSTBM으로 마지막 줄을 예약해도 장치가 `CSI r`로 스크롤 영역을 reset하거나 CUP·ED·alternate screen을 보내면 상태바/커서가 변한다. tmux는 별도 pane terminal 모델로 격리하는 것이므로 단순 passthrough에 tmux와 같은 보장을 적용할 수 없다. [VT 명세: DECSTBM 기본=전체 화면, OSC52 등](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).

장치 byte stream에 timestamp·주석·색상 reset을 섞으면 커서 상태/SGR을 바꾸고, echo를 틴트해도 원본 origin은 **device RX**다. 같은 문자열을 우연히 낸 결과를 agent TX로 강조할 수 있다. echo off일 때 주석에 “6 bytes”만 보이면 실제 agent 명령/결과를 확인하기도 어렵다.

추가할 것: M1 기본은 안전한 **log/trace + 명시적 TX 행 + raw 키 입력**. 프레임워크는 prompt_toolkit 또는 최소 Textual 중 키 시험으로 고른다. passthrough는 선택적 trusted-terminal 모드로 한정하고 status는 별도 pane/모델 영역에 둔다. full-screen 충실도가 필수라면 VT parser/screen 모델 비용을 인정한다. `OSC52`, title/query responses, BEL flood, escape 중간 chunk 분리도 정책·시험에 포함한다. 장치 제어문자를 host에 전달하면 terminal query 응답이 사용자 키처럼 다시 장치로 갈 수 있다는 점도 검토한다.

### B4. 64KiB raw backlog·gap 이후 “같은 화면 복원”의 조건이 없다 — P1

**대상**: §3.12 subscribe, §5.1 모드 복귀, §3.11 subscriber drop. 임의 바이트 경계64KiB는 UTF-8/CSI/OSC 한가운데일 수 있고 화면을 만든 earlier cursor/scroll/alternate state가 없다. “같은 바이트를 이어받기”와 “동일 VT 화면 복원”은 다르다.

추가할 것: M1은 safe log의 완결 이벤트/디코더 경계부터 backlog를 보여주고, 서버 high-water seq H 기준 snapshot `(from,H]`와 live `(H,∞)`를 붙인다. VT 모드는 emulator snapshot 또는 명시 reset+불완전 화면 경고가 필요하다. gap은 decoder/parser resync 없이 단순 raw 이어붙이기 금지. slow-client별 view gap과 원본 RX/기록 손실은 별도 카운터이며, `fully recorded`는 해당 범위 recorder 상태를 확인할 때만 쓴다.

### B5. “화면 지연”을 client receive로 측정하고 idle을 LAG로 오인한다 — P1

**대상**: §2.0 지연 표, §3.11. fake viewer가 WS를 받은 시각은 TUI 렌더·stdout blocking·herdr paint를 포함하지 않는다. 따라서 **rx→client receipt** 실험으로 **rx→사용자 화면 p95<100ms**를 통과시킬 수 없다. 간단한 USB 지연 예산도 `[추정]`이며 합계표는 장치60B 선로시간을 넣고 마지막 바이트 기준이라 부르므로 시작점을 통일해야 한다.

또 `now - 마지막으로 그린 chunk.ts`는 장치가5초 조용해도LAG5초가 된다. 무수신 idle과 backlog/render 지연이 혼동된다. 보완: host_read_complete→subscriber receipt→stdout write complete→선택 paint hook 구간을 분리하고, last received seq/last rendered seq·oldest queued age로 lag 계산, idle age는 별도 표기한다. 이벤트 루프 heartbeat는 데이터0에서도 갱신되어야 한다. 정상 부하 목표와 고부하 degradation 목표를 나누고, **계속 전부 drop해도 gap만 정확하면 p95 gate 통과**하는 판정은 금지한다. 921600에서 최소 렌더 서비스 수준·최대 허용 생략·사용자 Ctrl-C 반응 목표가 같이 있어야 한다.

### B6. reader→loop bridge·expect/regex·디스크 catch-up에 실행 예산이 없다 — P1

**대상**: §3.5 call_soon_threadsafe, §3.6 disk fallback, §3.11 “expect는 폭주에도 놓치지 않음”, §7 “수백 줄 규모”. ring/record/subscriber 큐가 bounded여도 **바이트마다 예약한 loop callback**, 긴 부분행, regex window, disk reader, 활성 expect 개수가 무한하면 전체 메모리·CPU가 bounded가 아니다.

이번 probe에서 **단32개 a+!**에 `(a+)+$`는 평가 시작 후200ms 내 끝나지 않아 child를 종료했다. Python re를 event loop에 두면 CLI timeout 체크·키 입력·WS fanout도 함께 멈출 수 있다. 이는 대용량 입력만의 문제가 아니다.

추가할 것: bridge byte 상한+합친 wakeup, read 청크 상한, line/escape parser 상한, literal 우선·regex isolated worker/시간 제한·pattern/window/동시 수 상한. 과거 기록 스캔에는 최대 bytes·wall deadline·취소·진행 cursor를 붙인다. 디스크에 기록돼 있다는 것과 deadline 안에 scan 가능하다는 것은 다르다. 이어 읽는 disk/live seam에서 snapshot seq를 고정하고 중복·누락 시험을 한다. “모든 정규식/폭주에서도 never miss” 대신 지원 window·matcher budget와 explicit gap/overrun error를 약속한다.

### B7. 재연결의 약한 신원 fallback 및 연결 세대 경계가 위험하다 — P0

**대상**: §3.8 규칙2/3, §3.4 wait-connected/expect 지속. 동일VID/PID와 이전COM 이름의 후보가1개여도 **그 장치가 원래 보드라는 증거는 없다**. 다른 adapter로 교체해도 해당 조건이 성립한다. USB location은 동일 소켓/topology이지 장치 영구 신원이 아니다. serial이 있더라도 다중 UART interface/port index·복제serial을 구분해야 한다.

추가할 것: exact VID/PID+serial+interface, 사용자 사전 bind한location만 조건부 허용, 그 외 UNKNOWN/AMBIGUOUS로 송신 금지. reconnect generation/connection_epoch를 추가하고 pending send·lock·approval은 무효화, 미완료 전송 자동 재시도 금지. `--wait-connected`와 expect의 세대 넘기기는 opt-in이며 기본 CONNECTION_CHANGED로 종료. “앞 장치의 명령→재연결한 새 장치의 부팅로그”를 같은 응답 성공으로 처리하지 않는다. reset-sensitive 보드는 reopen도 위험 profile에 묶는다. 부팅 PID 변화 alias는 명시 승인된 profile만 지원한다.

### B8. client role·confirm ID만으로 승인과 출처를 강제할 수 없다 — P0

**대상**: §3.9 hello(role), §10.2, §5.6. 같은 token을 읽는 client가 role=user/attach flag만 보내면 user가 될 수 있다. confirm ID를 최초 거절 응답으로 받은 agent가 그대로 재제출할 수 있는 기본ask 모드도 **사용자 승인 증거가 아니다**. 바이트 hash·단회성은 대상 바꿔치기/재사용 제한이며 승인 유무와 다른 속성이다. Opus가 이 한계를 §10.2에서 인정한 점은 좋지만 §11의 “R3 확인 없이 절대 송신 안 됨” 수용 기준과 불일치한다.

추가할 것: 서버가 credential/capability에 role을 묶고 origin/actor_id를 정한다. 승인 상태가PENDING→APPROVED가 된 뒤 같은 identity+generation+payload/action digest+expiry만 실행한다. 채팅 승인은 신뢰된 host가 전달하는 경계가 있을 때만 runtime 승인으로 말한다. 동일 OS 계정의 임의 shell을 허용한 agent를 악의적 주체로 완전 격리하려면 더 큰 권한 분리가 필요하므로, **의도된 CLI 경로의 정책 보호**와 강한 OS 보안 경계를 구분한다. R3 “매번”과 팝업의 “rule10분 승인”도 충돌하므로 정책 의미를 하나로 정한다. 원칙에 불필요한 다단계 승인 UI를 M1에 크게 넣기보다, 검증되지 않은 위험 작업을 M1 비범위로 두는 편이 작다.

### B9. byte-exact 기록과 비밀값 표시·export 정책의 연결이 약하다 — P1

**대상**: §4.3, §10.5, §6.5. TX 원문을 bytes.bin에 저장하는 기본은 사용자에게 명확히 알려야 한다.2초 동안 같은 RX byte열을 지우는 방식은 UTF-8·chunk split·변형 echo·지연 echo를 놓칠 수 있다. WS/JSON base64, hex/raw export, `dry-run bytes_hex`·approval popup에서 비밀이 새는지도 규정돼 있지 않다. raw log 보존을 택하는 것 자체를 사실 오류로 부르지는 않는다.

추가할 것: secret span→TX/각 표현/감사/event/뷰어/export에 적용되는 단일 정책, bounded streaming echo matcher와 변형 echo에 대한 한계 고지. secret TX는 기본redaction+길이/해시 metadata, byte-exact opt-in은 사용자 결정. 장치 echo RX를 완벽히 자동 감추겠다고 약속하지 않는다. 보호된 원본을 보관하면 파일ACL·명시 retention이 필요하다. SKILL에는 장치 출력이 instruction이 아니라 untrusted data라는 한 줄을 넣는다.

### B10. 포트/커서/기록 복원의 경계가 부족하다 — P1

**대상**: §3.3 default cursor=`agent`, §3.6 끝까지 전진, §6 기록, §13 daemon 복원. 여러 agent는 기본 커서를 공유하므로 별도 이름 지정 누락 시 서로 데이터를 소비한다. JSON response가 사라진 뒤 cursor를 이미 끝까지 옮기면 agent는 한 번도 보지 못한 결과를 놓친다. 머리+꼬리 omitted를 알리는 것은 좋지만 CLI output truncation과 RX gap을 같은 성공으로 취급하면 안 된다.

추가할 것: 기본 key는 `(session_id, authenticated client_id, cursor_name)`; alias 재사용·daemon 재시작·reconnect epoch 구분. `next_cursor` 반환/명시 ack 또는 stateless read 범위로 response loss를 재조회할 수 있게 한다. canonical read는 반환분까지만 전진, Opus의 머리+꼬리 요약은 `--summary`로 제공할 것을 추천한다. global seq와 mixed raw byte offset, RX-only offset, 회전 file 번호·file offset의 의미를 명확히 한다. “off=파일 offset”을 회전 이후에도 global cursor로 쓸 수 있는 매핑이 필요하다.

flush/fsync를 정상 종료 때만 한다면 power/crash까지 durable이라고 말하면 안 된다. raw append와 chunks index commit 순서, 잘린 마지막 JSONL/raw tail 복구, durable_through_seq, quota/full disk·active session 보존/삭제 정책을 추가한다. wall_at_start+monotonic 차이는 **고정 anchor로 만든 추정 wall**이며 시간 동기화 후의 실제 wall과 다를 수 있으므로 anchor epoch·clock correction/재시작 구분을 기록한다.

### B11. 다중 포트·다중 입력 사용자·Windows 핵심 루프의 수용 범위가 얕다 — P1

**대상**: §5.7 모든 viewer 입력 가능, §9 CI, §11/12. 사용자 두 명의 raw keystroke는 각각 작은 send여서 transaction 원자성을 지켜도 한 쉘 줄에서 섞인다. watcher 여럿+입력 lease 하나를 기본으로 정해야 한다. global memory/포트 수/expect 수/WS message 상한과 포트 간 공정성도 필요하다.

Windows `sim://` 테스트는 daemon·CLI semantics에 유용하지만 Win32 console raw Ctrl-C/VT keys와 COM driver를 증명하지 않는다. M1 목표에서 Windows 사용자 개입을 지원한다고 부를지, POSIX만 입증·Windows는 M2/M3 예정이라고 할지 명시한다. 최소 Windows Terminal 키 smoke를 HW 없이 수행하고 물리 COM 테스트는 별도 gate로 남긴다. CI의 3.11/3.13은 틀린 조합이 아니지만, 최신 지원 Python을 넣지 않는 이유·최대 지원 버전을 표기한다.

### B12. M1 작업량과 마일스톤 의존성에 불일치가 있다 — P1

- M1에 다중regex expect,send--expect,3개 안전규칙profile,confirm 흐름,raw VT 상태바/echo tint,auto-start,2 simulation backend,skill agent 실행까지 넣었다. 네 요소를 시연하기 전에 복잡한 가지가 동시에 막힐 가능성이 크다. **send/read/live attach/user Ctrl-C·기록/seq**를 먼저 통합하고 regex/에코추론·팝업 등을 M1b/M2로 옮긴다.
- M2 수용에서 R-7 release/reclaim 레시피를 요구하지만 release/reclaim 구현은 M3/T21이다. M2에 당기거나 레시피 gate를M3로 옮긴다. §14 Q2는Windows M2 smoke, §8/11은M3여서 통일해야 한다.
- M2 text.log와 M3/T21 text.log가 중복이며, M1이R3승인을 강제한다면서 user popup은M2다. M1의 승인 경로/비범위를 먼저 확정한다.
- §12 “S는T02/T03/T04/T06/T11 동시에”는 표 의존성과 다르다. T03→T02interface, T06→T04, T11→T03/T04/T01이므로 interface mock을 선행한 pipeline이지 모두 독립 병렬 작업은 아니다. 추천 담당이 한 명이면 구현 동시성도 별도 자원 가정이 필요하다.

## 3. 더 나은 선택

### 두 초안의 핵심 비교 — 요구된 여섯 기준

각 안의 장점을 합쳐 핵심 루프의 실패 가능성을 줄이는 기준이다.

| 기준 | Opus R1 | Sol R1 | 추천 |
|---|---|---|---|
| 단순성 | raw terminal+Textual 두 모드, 풍부한 동사와 넓은 M1 범위 | 한 trace/입력 경로와 작은 M1, matcher/file은 후속 | M1은 Sol처럼 줄이고 CLI 표기·SKILL 레시피는 Opus 활용 |
| 안정성 | idle typing guard·blocking flush·regex·disk fallback 경계 미정 | 명시 lease·chunk 취소·상한·격리·세대 경계 있음. 대신 운영 구현량 증가 | 공통 TX actor/lease·bounded bridge 채택, disk fallback은 별도 job |
| 에이전트 사용성 | send --expect/run/mark/omitted 범위·상태 warning이 편리 | after-request·독립 cursor·부분 송신 의미 명확. 기본 EOL none/낮은 rate는 번거로움 | 보드 profile EOL+명시 override, request 관측점과 사용자 개입 warning, run은 M2 |
| 사용자 가시성 | 구체 화면·HOLD·agent activity 우수. echo 추론과 늦은 TX 표기가 취약 | origin별 명시 TX 행·pending/result·view gap 분리 명확. 전체 화면 충실도는 후속 | 명시 TX 행을 기본으로, Opus의 activity/hold UI·모드 전환은 후속 |
| Windows 호환 | thread I/O와 sim은 좋음. raw console은 M3, suspend는 실제로 지원 | thread I/O+키 framework 활용, 그러나 실제 COM 검증 없음 | 두 안 모두 미검증을 표기. HW 없는 console smoke부터, COM gate는 별도 |
| 구현 비용 | passthrough 자체는 작지만 상태바·echo·복원·안전 규칙을 더하면 커짐 | safe trace는 작음. 완전한 VT parser·승인 credential 구현은 큼 | M1은 safe trace+raw 입력, emulator/강한 권한 격리는 필요 시 범위 결정 |

### 쟁점별 선택

1. **데몬 배치**: Opus의 도메인 프로세스 분리는 장애/부하 격리 근거가 있고 내 단일 daemon 선호보다 강점이 있다. 반대로 RPC/gateway/process lifecycle을 도메인마다 중복하면 비용이 크다. 공통 endpoint·registry·이벤트 계약을 정하고 UART owner process 배치는 교체 가능하게 할 것을 추천한다. 메인이 도메인 daemon을 택하면 수용한다. 프로세스 분리만으로 CAN-TP hard timing을 보장하지는 않는다. M1의 단일 UART owner 구현으로 이 운영 결정을 먼저 고정할 필요는 없다.
2. **IPC 라이브러리**: Opus websockets와 Sol aiohttp 모두 가능하다. M1이 WS-only면 **websockets**가 충분하고 내 aiohttp를 필수로 고집할 이유가 없다. HTTP health/static/export가 공통 요구라면 aiohttp를 택한다. schema/message cap/auth·느린 client 정책을 먼저 확정한다. msgspec은 성능과 3 OS wheel 검증 후 결정하고, M1은 stdlib JSON으로도 충분한지 측정한다.
3. **뷰어**: Opus의 Textual 대안이 Windows에서도 가능함을 인정한다. 프레임워크 이름보다 **원시 제어키+명시 TX trace+안전한 renderer+UI 지연**을 gate로 고른다. M1은 Sol의 safe trace 방향을 추천한다. raw passthrough는 별도 trusted mode로 보관한다. Textual의 두 모드가 더 낫다는 실측이 나오면 전환할 수 있다.
4. **typing guard**: 3초 idle 양도는 기각하고 명시 사용자 lease를 추천한다. attach는 watch가 기본이므로 사용자가 입력하지 않을 때 agent 자동화를 계속 막지는 않는다. Opus의 hold와 takeover UX는 유지한다.
5. **read/cursor**: Opus의 머리+꼬리 요약은 컨텍스트 보호에 유용하므로 summary/read-tail에 채택한다. canonical read는 Sol처럼 반환분 전진+next_cursor+명시 gap, 기본 client 분리로 한다. 과거 디스크 조회는 유한 job으로 후속 제공한다.
6. **EOL·rate**: 내 R1의 send 기본 none·1 KiB/s는 binary에 명확하지만 대화형 작업과 파일에 불편하다. 검증된 profile의 text EOL 기본을 허용하고 generic/hex/file은 none으로 한다. rate는 Opus처럼 profile별 8 KiB/s 등으로 정하되 물리 속도와 queue 한도를 함께 표시한다. 숫자 하나를 보편적 안전값으로 취급하지 않는다.
7. **expect transaction**: Opus `send --expect`는 내 분리 send/expect보다 race 예방 동선을 잘 보여준다. 채택하되 **RX watermark 확보·watch 등록→TX intent→write→result**를 직렬화하고 generation을 고정한다. 이는 송신 직전부터 관측하는 것이지 장치 응답의 인과관계를 증명하는 것은 아니다. 기존 prompt·비동기 RX·사용자 takeover를 시험한다.
8. **M1 안전 범위**: 많은 위험 문자열 규칙을 runtime 보안 보장으로 포장하기보다 읽기+승인된 status/echo 시험을 먼저 허용한다. R2 이상 제어/flash는 M1 비범위로 둔다. 위험 분류 5단계/4단계는 메인이 합의하되 승인 상태·action ID·세대 바인딩은 공통 필수다. 동일 사용자 권한이라는 한계는 두 안 모두에 적용된다.

## 4. 합의 가능한 부분

1. **PTY walking skeleton과 실제 pyserial 경로 재사용**은 그대로 채택한다. 내 R1의 개별 probe 결과를 제품 E2E로 과대평가하지 않고 §11 수용에 연결한 점이 좋다. Windows driverless `sim://`와 물리 COM 검증 분리도 맞다.
2. **agent activity/사용자 개입 warning/hold**가 내 초안보다 구체적이다. `user_input_since_last_call`, `deferred_ms`, hold 이유, expect 남은 시간을 공유하면 함께 장치를 다루는 효과가 분명해진다. origin은 추론한 echo가 아닌 확정 TX 행을 기준으로 쓴다.
3. **풍부한 CLI UX**: `send --expect`, mark/range 조회, grep/omitted 범위, profile prompt 규칙은 실제 agent 사용에 좋다. run/grep/mark는 M2에 두면서 명세를 가져올 수 있다.
4. **짧은 SKILL+references/레시피 동기화 시험** 구조가 좋다. 내 초안보다 recipes·troubleshooting·herdr 동선이 구체적이다. `no_viewer_attached` 안내, exit 5 재시도 루프 금지도 채택한다. 단 direct open이 언제나 실패한다는 문장과 M1에 없는 probe 사용 예는 고친다. secret 출력·장치 prompt injection 경고도 추가한다.
5. **구독자별 queue/drop+gap·기록 우선·기기 쪽 손실 unknown**은 일치한다. Opus가 64 MiB record queue가 넘치는 상황까지 언급한 점은 좋다. recorder 상태를 확인하지 않은 `fully recorded` 표시만 조건부로 고친다.
6. **Transport 추상화와 OS별 신호 gate**, DeviceWatcher의 fake enumerator 주입, CLI subprocess 골든 시험, PTY TUI 입출력 시험을 채택한다. fake backend로 driver/물리 파형까지 검증했다는 주장은 하지 않는다.
7. **raw byte+chunk metadata+파생 text 로그**는 공통 기초로 좋다. 하나의 seq로 TX/RX/제어를 조회하고 바이트 offset은 별도 좌표로 유지하면 된다. 시각 출처·crash commit·secret 정책을 보완한다.
8. **웹/XMODEM/viz/자동 baud/고급 로그 기능 후순위**는 핵심 원칙과 맞다. Opus의 release/reclaim은 외부 플래셔와 공존하는 실용적 대안이다. 다만 그동안 가시성이 중단됨을 명시하고 기본 핵심 루프 수행 경로로 권하지 않는다.
9. **후보 라이브러리를 직접 확인하고 추정을 표시한 점**은 적절하다. PyPI 버전은 확인한 범위에서 정확했고 Textual suspend·platformdirs 경로도 맞았다. Opus의 라이브러리 선택이 사실상 틀렸다는 결론은 내리지 않는다.

## 5. 핵심 원칙 점검

아래 “충족”은 **계획 요건을 충족**한다는 뜻이다. 제품·실시간성 실측을 마친 뜻은 아니다. 말미의 전부 ✅는 운영상 보장을 과하게 읽힐 수 있으므로 부분 항목은 통합 시 보완해야 한다.

| 체크리스트 6항목 | 판정 | 근거 | 보완안 |
|---|---|---|---|
| 맨 앞 핵심 루프 4요소 표 | **충족** | §0에 직접 접근/읽기/쓰기/표시·개입과 M1 동작 명시 | 핵심 표를 요약 결정표보다 앞으로 옮기면 우선순위가 더 명확. 의무적인 결함은 아님 |
| send→응답→ms 표시→사용자 개입 E2E | **부분** | §2.0 명령·3개 화면·Ctrl-C·지연 예산은 구체적 | 3초 typing 반례(B1), 큰 TX 중 Ctrl-C(B2), 확정 출처 TX 표시(B3)를 고쳐 E1④가 실제로 성립하게 함 |
| 실시간 목표+고부하에도 정직한 화면 | **부분** | p95<100 ms·60/30 Hz·queue/drop/gap·pause 표시가 있음 | client receipt와 화면 완료 분리(B5), idle≠LAG, raw drop 후 resync(B4), 실행 상한(B6)·고부하 최소 서비스 수준 고정. 숫자는 현재 제안이며 미측정 |
| M1=sim 위 4요소 걷는 뼈대 | **부분** | 네 요소를 함께 넣었으며 agent 먼저/viewer 나중 방식이 아님 | 다중 regex·echo 틴트·3 profile 확인 흐름을 줄이고 M1 E2E 먼저 통과. M2 release 레시피의 M3 의존성 해소(B12). Windows 사용자 입력 gate 명시 |
| 같은 session·late attach·숨은 연결 금지 | **부분** | 단독 owner·no-direct·같은 subscription·backlog/live 원칙 명확 | F1 강제 배타 주장 수정, seq watermark(B4)·generation(B7)·client cursor(B10)로 같은 session의 의미 보장 |
| 무관한 기능 후순위/비범위 | **충족** | §1/M4에서 웹/XMODEM/viz/probe/msgpack 등 후순위 명시 | 같은 기준으로 M1의 echo 추론·고급 matcher·여러 profile 범위도 줄임. RFC2217 remote 지원도 초기 기본 경로 밖에 둠 |

**현실적인 M1 수용안**: 한 PTY와 같은 PySerialTransport, agent open/send/read, 뒤늦은 attach의 직전 trace+live, 명시 TX/RX origin, 사용자 raw 키/03 전송, 입력권 충돌 거절, 기록과 queue 상한을 먼저 종단 간 시험한다. 115200에서 p95<100 ms를 실제 뷰어 stdout 완료까지 측정한다. 921600 paced flood 60초에서는 원본 hash·UI heartbeat·Ctrl-C 반응·view gap을 별도로 측정한다. fake viewer의 WS 수신만 통과하면 화면 gate 통과로 치지 않는다. 네 요소를 함께 검증하며 물리 UART/3 Mbaud/8포트는 별도 수용이다.

## 6. 쟁점 목록

| 쟁점 | Opus 안 | Sol 안 | 내 추천 | 근거 |
|---|---|---|---|---|
| 사용자 미완성 입력 | 3초 idle 후 agent 지연 해제 | 명시 yield/lease | **Sol**, 자동 idle 양도 금지 | B1: 4초 생각 중 명령 혼합 |
| Ctrl-C/대형 TX | 전송 중 키 보류·락 탈취 팝업 | chunk 경계 취소·부분 결과 | **Sol** 취소+Opus 상태 UI | B2: 무기한 flush·이미 전송분 취소 불가 |
| 기본 viewer | passthrough+상태바/echo tint, Textual log는 M3 | safe trace+raw 입력 | **Sol 방향**, framework는 선택 가능 | B3/B4: VT 격리·host escape·출처 오인 |
| agent 송신 표시 | 추정 device echo 틴트/byte 수 주석 | origin TX 행+pending/result | **명시 TX 행 필수**, Opus activity 추가 | core 출처·실시간 가시성 |
| Textual suspend | 미확인 대안 | framework 비교 필요 | **사용 가능 확인**, raw loop 설계 별도 | Textual 8.2.8 Unix/Windows 지원 |
| late attach/backlog | raw 64 KiB replay와 off 연속 | seq H snapshot/live | **Sol seq 경계+안전한 replay** | B4: 임의 raw 절단은 VT 복원이 아님 |
| 실시간 지표 | emit→WS receive를 화면 목표로 | host read→stdout write/paint hook | **구간 분리**, UI gate 측정 | B5: fake client가 terminal 정지를 못 봄 |
| cursor/다중 입력 | agent 기본 cursor, viewer 모두 입력 | client cursor, 입력 lease 1개 | **Sol 분리 기본** | B10/B11: 읽기 소비·사용자 문자 혼합 |
| read 요약/전진 | head+tail/전체 끝 전진 | 반환분 전진+next_cursor | canonical은 **Sol**, summary는 **Opus** | 복구·컨텍스트 보호 둘 다 필요 |
| send --expect/run | 원자 동사·profile prompt | after-request/send 분리 | **Opus 동사**, watermark/epoch 보강 | race 예방과 응답 인과 구분 |
| regex·과거 기록 | 증분 scan+disk fallback, 예산 미정 | window/worker/deadline 제한 | **Sol 상한**, disk는 bounded job | 33바이트 반례가 200 ms 초과 |
| POSIX exclusive | TIOCEXCL/다른 앱 open 실패 | flock 협조적 보조 | **Sol 사실 정정** | F1 소스+기본 open 성공 |
| 재연결 신원 | serial→location→VID/PID+같은 path | serial/interface→승인 location→중지 | **Sol**, 약한 신원은 사전 bind만 | B7: 원래 보드 확인 안 됨 |
| 재연결 send/expect | wait-connected·expect 지속 기본 | generation 변경 기본 실패 | **Sol 기본**, 관찰 연속 expect opt-in | 다른 세대 응답 오인·승인 재사용 |
| 승인/role | hello 역할 flag·confirm ID·ask 기본 | 서버 capability·승인 상태·action hash | **승인 상태 필수**, 신뢰 경계 명시 | B8: hash는 승인 증거가 아님 |
| 위험 등급 | R0–R4 | R0–R3 | **메인 공통 결정**, 비가역 기본 차단 | 등급 개수보다 runtime 의미가 중요 |
| secret/raw | 원문 보존·text/echo 2초 마스킹 | secret TX redaction 기본 | **redaction 기본+보호 원본 opt-in** | B9: 표현·export·지연 echo 누출 |
| text EOL·TX rate | profile CR/8 KiB/s | none/1 KiB/s | **profile EOL+override**, profile별 rate | 내 안의 대화 편의·bulk 부족 인정 |
| daemon 배치 | 도메인별 process | 공통 daemon 모듈 | **메인 결정**, owner 배치 교체 가능 | Opus 격리 근거 유효·공통 계약 우선 |
| IPC server | websockets | aiohttp | **WS-only면 websockets**, HTTP 필요 시 aiohttp | 둘 다 가능·기능 범위로 결정 |
| M1 범위 | multi-expect/confirm/echo 등 다수 | 작은 E2E | **작은 M1**+Opus 레시피·경고 UX | B12: 통합을 막는 가지 감소 |
| YMODEM | xmodem 후순위 후보 | 일정 비범위 | **후순위**, 별도 검증 전 약속 금지 | upstream pending·Opus도 미확인 |
| 기록 복원·시각 | 종료 fsync·고정 wall anchor | commit·watermark/clock ID | **durability/epoch 명시** 공통화 | B10: crash/시계 보정·회전 좌표 |

통합 우선순위는 **F1/B1/B2/B3/B7/B8 → 화면 계측·seq/커서·실행 예산 → 부가 UX/성능 최적화**다. 동일 장치·동일 세션·확정 출처·사용자 개입이 실제로 성립하는 M1을 작게 검증해야 한다.
