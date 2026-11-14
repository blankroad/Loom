# sigrok real-engine feasibility probe

2026-11-14 KST. **성공: 실제 sigrok 엔진 + demo acquisition → raw tee → 별도 decoder에서 STOP/EOF 전 출력 확인.** 실장비 성공을 뜻하지 않는다. ROOT가 동결한 `logic` 도메인에 적용할 근거이며 제품 소스/Git/Cargo/하위 에이전트 작업은 하지 않았다.

## 설치·범위

`HOMEBREW_NO_AUTO_UPDATE=1 brew install sigrok-cli` 정상 설치 완료. 런타임 의존성으로 유지한다. `/opt/homebrew/bin/sigrok-cli` **0.7.2**, libsigrok **0.5.2**, libsigrokdecode **0.5.3**, Apple Silicon. 보안 설정 변경 없음. demo만 명시적으로 선택했고 USB/실제 driver scan·firmware 접근은 하지 않았다. 진단 로그의 driver sanity check는 라이브러리 초기화이며 발견 장치는 demo 한 개다.

## 확정 argv / 포맷

실제 사용한 acquisition argv (stdin PIPE를 열어 둠):

```sh
/opt/homebrew/bin/sigrok-cli -d demo -c samplerate=1000000 \
  -C D0,D1,D2,D3,D4,D5,D6,D7 --continuous -O binary -l 2
```

Loom collector 하나가 stdout을 raw 파일/파형 처리와 아래 decoder stdin으로 tee한다. **장치 acquisition은 한 프로세스뿐**이다. `-C`로 analog를 제외해야 한다. 최초 진단에서 analog를 활성화한 채 `-O binary`만 주면 analog fallback 텍스트가 stdout에 섞였다. 연속 캡처 stdin을 닫힌 상태로 상속하면 이 실행 환경에서 즉시 종료했으므로 worker가 소유한 stdin PIPE를 유지한다.

```sh
# UART synthetic: D0 RX, 100 kbaud, 1 MS/s
/opt/homebrew/bin/sigrok-cli -i - -I binary:numchannels=8:samplerate=1000000 \
  -P uart:rx=0:baudrate=100000 --protocol-decoder-samplenum -l 2 -A uart=rx-data

# SPI synthetic: mode 0, MSB first, D0 CLK / D1 MOSI / D2 CS#
/opt/homebrew/bin/sigrok-cli -i - -I binary:numchannels=8:samplerate=1000000 \
  -P spi:clk=0:mosi=1:cs=2:wordsize=8 --protocol-decoder-samplenum -l 2 -A spi=mosi-data

# demo tee decoder: 임의 demo pattern에 맞춰 CS 없이 word decode
/opt/homebrew/bin/sigrok-cli -i - -I binary:numchannels=8:samplerate=1000000 \
  -P spi:clk=0:mosi=1:wordsize=8 --protocol-decoder-samplenum -l 2 -A spi=mosi-data
```

8채널은 **sample당 1 byte**, bit 0=D0 … bit 7=D7. Synthetic UART는 bit당 10 samples이며 예상 `55 A3 00 FF`, SPI는 반주기당 10 samples이며 예상 `A5 3C`를 실제 decode했다. 9채널 stdin 별도 시험은 sample당 2 bytes, 첫 byte bit0–7 / 둘째 byte bit0=channel8인 packing으로 `rx=8`에서 같은 UART 결과를 확인했다. 일반 규칙은 `ceil(numchannels/8)` bytes/sample, low-channel bits부터 배치하되 다른 hardware 출력의 채널 remap/unitsize는 별도 확인 대상이다.

`-l 2 --protocol-decoder-samplenum`에서 실제 행: `210-290 uart-1: 55`, `140-300 spi-1: A5`. **인스턴스 ID는 `uart-1`/`spi-1`**이므로 고정 `uart:` 파서를 쓰지 않는다. 숫자 sample span과 첫 ID separator 이후 text를 보존한다. 필터링하지 않으면 bit/start/transfer 등의 annotation이 섞이고 sample span 순서도 출력 순서와 다를 수 있다.

`-A uart=rx-data`, `-A spi=mosi-data`로 **word/frame 데이터 class만 선택**하는 것을 기본값으로 권고한다. 동일 synthetic 입력에서 UART 484→44행, SPI 209→22행으로 감소했다. RX/TX·MISO/MOSI 양쪽 또는 warning이 필요하면 별도 class 선택으로 명시한다. 이는 텍스트 annotation 계약이며 구조화된 버스 transaction API로 과장하지 않는다.

## 측정 결과

시간은 해당 `run()` 시작부터의 host monotonic 값이며 process launch를 포함한다. sample clock latency 보장이 아니다. Synthetic은 약 100 ms마다 11회 입력하고 **약 1.5 s까지 stdin을 닫지 않았다**. 첫 완전한 newline 행을 별도 기록했다.

| 시험 | 첫 raw | 첫 완전 annotation | STOP 요청 / decoder EOF | 결과 |
|---|---:|---:|---:|---|
| UART synthetic | — | 112.375 ms | 1500.102 / 1500.148 ms | 44행, `55 A3 00 FF`, rc=0 |
| SPI synthetic | — | 112.609 ms | 1507.402 / 1507.426 ms | 22행, `A5 3C`, rc=0 |
| demo → raw tee → SPI, newline STOP | 122.668 ms | 123.094 ms | 1507.741 / 1508.810 ms | raw **1,435,280 bytes = decoder 입력 bytes**, 22,426행 |
| demo → raw tee → SPI, SIGINT | 124.158 ms | 124.459 ms | 1500.009 / 1500.862 ms | raw **1,448,166 bytes = decoder 입력 bytes**, 22,627행 |
| 9채널 UART rx=8 | — | 111.874 ms | 1503.249 / 1503.293 ms | 44행, 같은 예상값, rc=0 |

모든 annotation 성공 시험이 **STOP 이전이면서 stdin EOF 이전**이다. stdout이 파이프로 연결돼 있어도 전 캡처/EOF를 기다리지 않음을 실측했다. 개별 출력마다의 flush syscall 자체를 추적한 것은 아니며, 이전 source 조사에서 확인한 `fflush`와 일치하는 관측이다. 실측한 demo 처리량/지연을 실장비 보장으로 사용하지 않는다.

## STOP/reap와 malformed 입력

- 정상 경로: capture stdin에 newline → capture rc=0 → raw stdout EOF까지 drain → tee pending bytes 전달 → decoder stdin close → decoder stdout drain/wait rc=0. STOP부터 전체 reap까지 약 **21 ms**. 두 child 모두 `wait()` 완료.
- SIGINT 경로: capture rc=-2, decoder rc=0, pending raw 모두 전달 후 reap. Linux/macOS 신호 종료를 성공 rc=0처럼 해석하지 말고 `requested_stop`과 묶어 결과를 분류한다. 강제 신호 경로는 완전한 backend flush를 보장하지 않는다.
- Helper는 비차단 pipe IO, tee queue 상한 4 MiB, 각 실행 loop 4 s + terminate/kill/wait deadline을 사용했다. 측정 run은 각각 약 1.5 s, 모두 foreground에서 join 완료. 느린 decoder의 제품 정책 검증까지 수행한 것은 아니다.
- **잘린 sample은 CLI exit code로 검출 불가:** 9채널에 45,067 bytes를 전달해도 decoder rc=0 / stderr 없음. 별도 raw 재출력 진단에서 4,097 input bytes 중 유효 sample은 4,096 bytes만 나왔다. 여기에 `META samplerate: 1000000\n` **25 bytes**가 stdout prefix로 붙어 총 4,121 bytes였음도 기록했다. 따라서 파일 입력→`-O binary` 경로를 검증 없이 pure-binary capture와 동일 취급하지 않는다.
- Collector는 pipe read 경계에서 `unitsize` 미만 bytes를 carry하고 완전 sample만 publish/decode한다. EOF에 carry가 남으면 **명시적 truncated/source-gap 오류**, 원본 trailing bytes 보존, silently pad/drop 금지. 작은 검증 예: 5 bytes를 임의 분할 수신 → 4 bytes/2 samples emit + 1 dangling byte 검출.

## backend/UI 인계 계약

`logic` worker가 raw acquisition을 단독 소유한다. raw chunk에는 `capture_id/epoch`, monotonic sample offset/count, sample rate, unitsize/channel mapping, bytes 또는 bounded raw reference, real/demo/sim provenance를 담는다. annotations는 같은 capture의 sample span + decoder epoch/instance/class selection/text를 갖는다. host receipt time과 sample time을 구분한다. UI/agent는 기존 gateway가 publish한 동일 canonical event/sequence를 소비하며 둘 다 STOP 전에 raw/annotation을 볼 수 있어야 한다. 높은 data rate의 모든 행을 UI에 그리는 대신 raw 보존과 bounded 표시/명시적 gap을 분리한다.

## 증거 파일

- `ENGINE-PROBE-helper.py`: 재현 가능한 bounded foreground capture/tee/decoder helper.
- `ENGINE-PROBE-results.json`, `ENGINE-PROBE-run.jsonl`, `ENGINE-PROBE-bit8.json`: exact argv, 시각, bytes, 행 수, 종료 코드/reap.
- `ENGINE-PROBE-*.stdout.txt`, `ENGINE-PROBE-*.stderr.txt`, `.bin`: 실제 출력 및 입력/raw trace. UART/SPI fixture 파일은 해당 생성기 11회 입력과 동일한 bytes다.
- `ENGINE-PROBE-checks.txt`: pre-EOF, bytes count, expected decoded words, packing/reap assertions 통과.
- `ENGINE-PROBE-truncation.json`: odd-byte truncation 및 metadata prefix 관측.
- `ENGINE-PROBE-{version,help,binary-options,demo-options,uart-options,spi-options}.txt`: 설치본 flags/options.
- `ENGINE-PROBE-install.log`, `ENGINE-PROBE-demo-diagnostic.txt`: 설치 및 초기 실패 원인 진단. 최종 권고는 위 성공 argv를 따른다.

제품 구현/독립 리뷰는 ROOT/Opus의 다음 요청을 기다린다. 실장비 호환성, I2C, arbitrary channel selection의 packed mapping, 장기 soak 및 decoder stall recovery는 이번 성공 범위에 포함하지 않았다.

## 추가: 부분/역순 채널 mapping probe (ROOT §8 검토)

설치본 demo만 대상으로 위 capture argv에서 `--continuous`를 `--samples 4096`으로 바꾸고 `-C`를 아래와 같이 변경했다. 각 subprocess timeout 3 s, 총 6개 capture 정상 종료·reap(rc=0), 각 4,096 bytes. USB/physical scan·Cargo 실행 없음.

| `-C` | raw 값 범위/비교 |
|---|---|
| `D0,D1,D2,D3,D4,D5,D6,D7` | 1 byte/sample, 첫 bytes `d9 b6 b6 b6 cd ff …` |
| `D5,D7` | 값 집합 `{0,1,2,3}`, 첫 bytes `01 02 02 02 01 03 …` |
| `D7,D5` | **D5,D7과 전체 4,096 bytes 동일**: CLI 선택 문자열 순서가 bit 순서를 정의하지 않음 |
| `D5` | 값 집합 `{0,1}`: bit5 위치 보존이라고 해석하면 틀림 |
| `D0,D7` / `D0,D1` | 둘 다 D5,D7과 전체 bytes 동일 |

**관측상 demo 부분 선택은 원래 물리 bit mask 보존도, 선택 문자열 순서대로 full trace를 compact한 결과도 아니다.** 특히 D5,D7 출력을 full trace의 `(bit5→bit0, bit7→bit1)` 투영과 비교해도 불일치했다. 따라서 이를 일반 하드웨어의 정확한 remap 증명으로 확대하지 않는다. 설치 버전의 demo/CLI acquisition 경로에서 채널 선택에 따라 표현이 바뀐다는 근거로 충분하며, 단순 `numchannels=선택개수` 또는 물리 bit 번호 가정은 금지한다. libsigrok 0.5.2 binary output 자체는 받은 `logic->data`를 그대로 복사하므로 binary output 모듈이 물리 이름 metadata를 복구해 주지도 않는다.

**첫 slice 권고:** acquisition은 **항상 D0…D7 전체 8개 고정·정렬**, decoder stdin은 `numchannels=8`, `rx/clk/mosi/cs`는 같은 0…7 bit 번호로 유지한다. 사용자 `channels`는 UI/edge 표시 필터로 취급하고 acquisition `-C`에 그대로 전달하지 않는다. event에는 acquisition mapping과 표시 선택을 혼동하지 않게 기록한다. 실제 장비의 채널 이름이 D0…D7과 다르거나, 원래 16채널인데 첫 8개 선택으로도 raw unitsize가 2 bytes인 경우에는 §8의 1 byte 고정 계약으로 **사용 불가/미검증** 처리해야 한다. driver allowlist만으로 unitsize=1이 보장되지는 않는다.

**`sigrok-device` validation 권고:** `-L`의 전체 목록을 허용목록으로 사용하지 말고 ROOT 계약의 명시적 후보 **`fx2lafw`, `saleae-logic16`, `saleae-logic-pro`** exact-name allowlist와 설치본 목록의 교집합으로 제한한다. 세 이름 모두 설치본 `-L` 정적 목록에 존재했다. 다만 이 목록은 컴파일 포함 여부만 증명하며 실장비/firmware/최신 revision/8ch packing 검증은 아니다. `saleae-logic-pro`는 experimental 표시를 유지하고 계약의 `21a9:1006` 제한도 모델 지원 보증으로 바꾸지 않는다. `saleae-logic16`/Pro는 특히 고정1byte 계약 충족이 아직 확인되지 않았으므로 첫 slice에서 device runtime 사용을 막거나 별도 packing gate를 통과시켜야 한다. 임의 driver 문자열/옵션 주입은 거부하고 conn은 별도 검증 필드로 둔다. `demo`는 `sigrok-demo` source 전용이다. 어떤 검증 단계에서도 `--scan` 또는 driver `--show`로 실제 장치 탐색을 실행할 필요가 없다.

증거: `ENGINE-PROBE-mapping.py`, `ENGINE-PROBE-mapping.json`, `ENGINE-PROBE-mapping-extra.json`, `ENGINE-PROBE-mapping-*.bin`, `ENGINE-PROBE-list-supported.txt`. 정적 참고 소스 사본: `ENGINE-PROBE-demo-protocol-source.c`, `ENGINE-PROBE-binary-source.c`; 원본 https://github.com/sigrokproject/libsigrok/blob/libsigrok-0.5.2/src/hardware/demo/protocol.c 및 https://github.com/sigrokproject/libsigrok/blob/libsigrok-0.5.2/src/output/binary.c . ROOT/Opus가 실제 backend 소스를 생성하면 이 mapping/unitsize 경계를 독립 리뷰한다.
