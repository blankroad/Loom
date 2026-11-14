# Logic analyzer 실시간 로깅 — Opus 조사·권고 (phase 1)

조사일 2026-11-14. 공식/1차 소스만 확인. 제품 수정·빌드·설치·USB 스캔은 하지 않음. `sigrok-cli`·`pulseview`는 현재 PATH에 없음(`which` 확인).
선행 문서: `docs/plan/explore/v12-saleae-check.md`, `work/reports/saleae-live/SUMMARY.md`.

## 1. 결론 (권고)

**하나의 `logic` 도메인, 두 개의 어댑터, 하나의 이벤트 계약.**

| 순위 | 어댑터 | 쓰는 경우 | 이유 |
|---|---|---|---|
| A (먼저 구현) | `sigrok` — worker가 `sigrok-cli` 자식 프로세스를 소유 | 장비 없는 재현 데모, fx2lafw 계열·Logic16·Pro 16(실험적) | Loom이 프로세스 수명(시작·STOP·kill)을 직접 쥔다. 디코더 출력이 annotation마다 flush되고 sample 번호를 준다. `-d demo`는 libsigrok의 실제 acquisition 경로라 Loom 내부 mock이 아니다 |
| B (장비 확정 후) | `saleae-hla` — Logic 2 안의 전달 HLA → localhost JSONL → worker | Logic 8 / Pro 8 / Pro 16 + Logic 2 앱 | Saleae 신형 장비에서 STOP 전에 외부로 데이터를 받는 유일한 공개 경로. MCP/Automation은 제어 경로로만 사용 |

sigrok을 "Saleae가 안 된다"는 옛 판단 때문에 고른 것이 아니다. A를 먼저 하는 이유는 (1) 장비 없이 끝까지 실제로 도는 vertical slice가 가능하고, (2) Logic 2 GUI 앱 의존 없이 daemon이 수명주기를 소유하기 때문이다. 사용자의 실제 장비가 Saleae Logic 8 / Pro 8이면 **B가 필수**이다(§3).

Serial Studio는 제외: 데이터 소스는 Serial/BLE/네트워크(+Pro: MQTT, Modbus, CAN 등)이고 frame parser로 텍스트→수치를 만드는 텔레메트리 대시보드이다. logic analyzer 하드웨어나 raw 디지털 파형 디코딩에 대한 기술이 없다. GPLv3 + 상용 이중 라이선스. [repo](https://github.com/Serial-Studio/Serial-Studio)

## 2. Saleae — 최신 사실 (2.4.48 반영)

- **2.4.48 (2026-10-07)**: "Fix bug where adding an analyzer from the automation interface **or the MCP interface** while a capture was in progress would not return until the capture was manually stopped." → 캡처 중에 analyzer를 추가하는 것이 이제 지원되는 동작으로 보인다. Capture 문서의 "다른 호출 전에 wait/stop" 규칙은 이 수정보다 오래된 문서다. [release](https://discuss.saleae.com/t/logic-2-4-48/3820)
- 그래도 **캡처 중에 analyzer 결과나 raw 데이터를 읽는 API는 공개되어 있지 않다**: `export_data_table` / `export_raw_data_*`는 문서상 캡처 종료 뒤에 쓰고, release note에도 읽기 쪽 변경은 없다. HLA에 대한 언급도 없다. → 2.4.48은 "실행 중 analyzer/HLA 부착"을 풀어 줄 뿐, 데이터를 밖으로 꺼내는 것은 여전히 HLA `decode()` 안에서 전달해야 한다. `add_high_level_analyzer`를 캡처 중에 부를 수 있는지는 **미확인**(실기 PoC 1순위).
- **MCP**: Experimental, Settings > Automation, `http://127.0.0.1:10530` Streamable HTTP. 지원 장치는 Logic 8 / Pro 8 / Pro 16(MSO는 예정). 공식 도구 목록, 데모 장치 지원, 캡처 중 결과 읽기는 문서에 **없음**. [MCP](https://docs.saleae.com/mcp/guides/getting-started) · [llms-mcp](https://docs.saleae.com/llms-mcp.txt)
- 장비 없는 경로: Automation `get_devices(include_simulation_devices=True)` + `F4241`(Pro 16 demo). 단, 데모의 raw는 실제 프로토콜 데이터가 아니다(공식 예제 주석). 따라서 B의 hardware-free 데모는 "제어 경로 + HLA 전달" 검증까지만 가능하고, 디코드 내용 검증은 할 수 없다.
- **B 경로 PoC 순서 (ROOT 앱 인벤토리 이후)**: Logic 2 ≥ 2.4.48 확인 → worker가 TCP listener를 연다 → MCP 또는 Automation으로 sim/실장비 `start_capture`(manual) → 캡처 중 `add_analyzer` → `add_high_level_analyzer(전달 HLA)`가 반환되는지 확인 → STOP 전에 daemon eventlog에 frame이 들어오는지 확인 → stop. 반환되지 않으면 GUI에서 미리 구성한 뒤 GUI Start로 fallback한다(자동 fake fallback은 하지 않음).

## 3. 정확한 하드웨어 지원 (드라이버 소스 기준, wiki 기준 아님)

| 장비 | USB VID:PID | libsigrok | Saleae Logic 2 / MCP |
|---|---|---|---|
| Saleae Logic (orig) 및 클론 | 0925:3881 | `fx2lafw` 지원 | 지원(구형) |
| Logic16 | 21a9:1001 | `saleae-logic16` (펌웨어 추출 필요) | 지원(구형) |
| Logic 8 | 21a9:1004 | **미지원** (udev 규칙도 주석 처리: "currently unsupported") | 지원 |
| Logic Pro 8 | 21a9:1005 | **미지원**: udev 규칙에는 있지만 `saleae-logic-pro/api.c` `scan()`은 `idProduct != 0x1006`이면 건너뛴다 | 지원 |
| Logic Pro 16 | 21a9:1006 | `saleae-logic-pro`: 실험적, 디지털 16ch만, 1/2/2.5/10/25/50 MHz(기본 10), FPGA bitstream 추출 필요, HW revision 검사 없음. 2.4.48에 "new Pro 16 hardware revisions" 추가 → 신형 revision 호환은 **미확인** | 지원 |

출처: [api.c](https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/saleae-logic-pro/api.c), [protocol.c](https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/saleae-logic-pro/protocol.c) (연속 bulk 재제출, analog 경로 없음), [60-libsigrok.rules](https://raw.githubusercontent.com/sigrokproject/libsigrok/master/contrib/60-libsigrok.rules).

## 4. sigrok-cli 실시간 동작 (소스 확인)

- `--continuous`: 멈출 때까지 샘플링한다. `--time`/`--samples`로 bounded 캡처를 한다. [manual](https://sigrok.org/wiki/Sigrok-cli)
- `-P uart:...`, `--protocol-decoder-samplenum`: annotation 줄마다 `"%llu-%llu "`(start-end sample)를 찍고 **매 줄 `fflush(stdout)`**. `-B`(binary decoder output)와 jsontrace도 flush한다. 따라서 STOP 전 ingest가 가능하다. [decode.c](https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/decode.c)
- `-i -`: **stdin을 입력으로 받는다**(fd 0, `read()` 루프, 4 MiB chunk). pipe에서는 `read()`가 도착한 만큼만 반환하므로 스트리밍이 된다. 입력 모듈의 decoder 연결은 datafeed 경로를 따른다. [input.c](https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/input.c)
- demo 드라이버: logic 기본 8ch, 200 kHz, 패턴은 `sigrok, random, incremental, walking-one, walking-zero, all-low, all-high, squid, graycode`이고 `SR_CONF_CONTINUOUS`를 지원한다. **UART 패턴은 없다.** [demo/api.c](https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/demo/api.c)

### 권장 프로세스 토폴로지 (raw + decode를 장치 1개 소유로)

```
[장치 또는 demo] --sigrok-cli #1 (-O binary, --continuous|--time)--> stdout raw 바이트
      worker: raw chunk → edge 압축(logic.edges) ──┐
               └─ tee → sigrok-cli #2 (-i - -I binary:numchannels=N:samplerate=R
                                        -P <pd> --protocol-decoder-samplenum) → logic.annotation
```
- 장치를 여는 프로세스는 하나다(#1). raw 파형과 디코드가 같은 sample 축을 공유한다.
- **hardware-free 데모**: #1 대신 worker가 결정적 UART 파형(예: 115200 8N1 "LOOM 0001\r\n"…, samplerate 1 MHz, 1ch)을 생성해 #2 stdin에 넣는다. 디코딩은 실제 libsigrok `uart` PD가 한다. 이벤트에는 `source: "synthetic"`를 정직하게 표기한다. 추가로 `-d demo`(pattern=graycode)로 raw 경로만 실제 libsigrok acquisition으로 검증한다.
- **미확인(구현 전 실측)**: `-I binary` 입력 + `-P` 조합에서 pipe chunk 단위 디코드 지연, `-O binary`와 `-P` 동시 지정 시 stdout 동작(그래서 2-프로세스 구조를 쓴다), SIGINT 시 `--continuous` 정상 종료와 마지막 annotation flush.

## 5. 프로세스·자원 정책

- 출력 경계: stdout line 최대 4 KiB(초과분 잘라내고 `truncated` 카운트), stderr는 64 KiB ring(종료 사유에 tail 첨부), raw read chunk 64 KiB, 이벤트당 edge 최대 4096개(초과 시 `dropped_edges`), 세션 `max_duration` 기본 60 s / 상한 1 h, `max_samples` 상한.
- STOP: latch → #1에 SIGINT → 2 s 뒤 SIGKILL → #2 stdin EOF → drain(2 s 이내) → `logic.capture` state=stopped 이벤트(사유, exit code, stderr tail). STOP 래치 중에는 새 start를 거부한다. 자식은 process group으로 띄우고, daemon이 죽을 때도 정리한다(macOS: worker drop, Linux: `PR_SET_PDEATHSIG` 고려).
- 자동 fake fallback은 없다. `sigrok-cli`가 없거나 장치가 열리지 않으면 `capture` state=failed와 원인을 기록하고, 사용자가 `--source synthetic`을 명시해야만 합성 입력을 쓴다.
- 의존성: `brew install sigrok-cli`(phase 2, ROOT 승인 후). 경로는 `loom.toml [logic] sigrok_cli = …` 또는 PATH에서 찾는다. 버전은 `sigrok-cli --version`으로 capability에 기록한다.

## 6. ROOT 확인 반영 (2026-11-14)

ROOT가 확인한 사항: decode.c는 annotation 줄마다 flush한다(L762-778). Pro 8은 지원이 보장되지 않는다(api.c는 21a9:1006만 받음). 이 머신에는 brew glib/libusb가 있고, sigrok-cli와 /Applications의 Logic 2 앱은 없다. → B(Saleae)는 지금 이 머신에서 검증할 수 없다. 첫 slice는 A(sigrok-cli 프로세스 경계, CFFI 없음, 외부 PD 사용)로 확정을 권고한다. raw 파형과 디코드 타이밍의 구분은 `CONTRACT.md` §3에 정리했다.

## 7. ROOT 결정 요청

1. 백엔드 소유: `crates/loom-logic` (gateway+worker+cli, `loom-echo` 복제 패턴, `domains.rs`에 `feature = "logic"`). 어댑터 A를 먼저 하고, B는 사용자 장비/앱 인벤토리 확인 뒤 같은 worker에 추가한다.
2. sigrok-cli 설치 승인(phase 2 첫 단계). USB 스캔(`--scan`)은 별도 승인을 받는다.
3. 계약: `CONTRACT.md` 동결.
