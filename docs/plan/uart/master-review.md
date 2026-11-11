ISSUES
# UART 리드 검토 — 00-master-plan.md v1 대비 uart/plan.md v2 (2026-11-10, uart-lead)
대부분 수용 가능하다. 아래 (a)는 그대로 적용하면 **사용자 개입(02 원칙 4요소 중 4)** 이 깨지거나 범위가 모호한 것 6건이다.

## (a) 설계를 깨는 결정 — 이유와 제안
1. **D6/D7 "기록·감사 불능이면 새 R1+ 송신 중단"이 사용자 키 입력까지 막는다.** 디스크가 가득 찬 상태에서 폭주하는 보드를 사용자가 Ctrl-C로 멈출 수 없게 된다(uart DR-23은 에이전트 TX만 차단).
   → 제안: 중단 대상은 `origin=agent|script`로 한정한다. 뷰어 자격의 사용자 입력은 허용하고 `record.health=failed` 배너만 띄운다. 이 구간의 감사는 데몬 로그(stderr)에 남긴다.
2. **D10 "모든 쓰기가 core 파이프라인(policy→confirm→audit→backend)을 지난다"를 원시 키 입력에도 그대로 쓰면 키마다 정책 판정과 해시 체인 감사가 일어난다.** 사용자 키→장치 p95 < 20 ms 목표(uart §3.11)를 지키기 어렵고, 감사량이 타이핑 바이트 수만큼 늘어난다.
   → 제안: 쓰기 클래스로 **"사용자 대화형 입력 스트림"** 을 둔다. 입력권(lease)을 얻을 때 policy를 한 번 판정하고(R1, 문자열 규칙 미적용), 감사는 줄 단위 또는 1 s 단위 묶음 해시로 남긴다. 저널은 청크마다 그대로 기록한다. 입력권·InputArbiter는 UART worker 안에 두어 청크 단위 취소와 Ctrl-C 우선 처리를 유지한다.
3. **D9/D6 STOP(`Ctrl-] x`, `loom stop-all`)의 대상이 정의되지 않았다.** 사용자 입력까지 잠그면 사용자가 장치에 개입할 수 없다. 또 `Ctrl-] x`는 uart 계획에서 hex 토글 키다.
   → 제안: STOP latch는 agent·script TX와 주기 job만 잠그고 사용자 뷰어 입력은 유지한다고 계약에 명시한다. UART hex 토글은 `Ctrl-] g`로 옮긴다(UART 쪽에서 수정).
4. **D10 + §3.3 소유권 충돌.** "UART 콘솔 엔진(버퍼·커서·expect·InputArbiter)과 TUI 위젯은 core 재사용 컴포넌트"인데, `loom/core/`는 CORE 팀만 수정할 수 있다. 그런데 §3.2는 이것들(T09–T11)을 UART M1 작업으로 배정했다. 누가 어느 경로에 만드는지 정해지지 않았다.
   → 제안: UART 팀이 소유하는 공유 패키지 `loom/console/`(안정 인터페이스, SSH/ADB/RTT 도메인이 import)로 둔다. CORE는 쓰기 게이트·이벤트 허브만 소유한다. 메인 결정이 필요하다.
5. **D2 배치 "≤ 40 ms"가 지연 예산을 거의 다 쓴다.** uart 예산은 배치 10 ms 기준 최악 ≈ 35 ms였다. 40 ms 배치면 게이트 구간 최악 ≈ 65 ms, USB 지연과 herdr 그리기를 더한 사용자 체감 최악 ≈ 97 ms가 되어 p95 < 100 ms 목표에 여유가 없다.
   → 제안: 40 ms는 **상한**으로 두고, 조용한 구간 뒤 첫 이벤트는 즉시 flush하는 적응형 배치를 쓴다. 또는 구독 옵션 `max_batch_ms`(터미널 뷰어 10 ms)를 둔다.
6. **D5 "`--follow` 최대 600 s"가 모든 대기형 명령에 적용되는지 모호하다.** uart `expect`는 단일 결과 대기로 최대 3600 s를 허용한다(A5 장시간 테스트 감시).
   → 제안: 600 s 상한은 스트리밍(`--follow`)에만 적용하고, 단일 결과 대기는 3600 s까지 허용한다. 600 s로 통일한다면 UART는 `resume_from` 반복 레시피로 대응한다(수용 가능).

## (b) 마스터 문서의 UART 계획 오기·누락
1. D5 "기본 출력 UART 100행": uart 계획은 200줄 / 16 KiB다. 100행으로 바꾸는 것은 수용한다(c-4). 다만 출처는 메인 결정으로 표기해 주길 요청한다.
2. D9 "터미널 모드는 termios/Win32 원시 입력": uart 계획에서 Windows 원시 입력은 M2에서 prompt_toolkit 입력 계층을 재사용하는 것이다(M1 터미널 모드는 POSIX만). §5 리스크 문구와는 일치한다.
3. §3.2 P0 "UART T05 RingBuffer/Recorder": D7의 기록 정본(도메인 저널)과 CORE P0 "기록기 기반"이 겹친다. UART T05는 core 기록기 위의 UART 저널 어댑터와 RingBuffer로 범위를 좁힌다(c-15).
4. D6 "UART 계획의 사용자 터미널 approve 명령은 채택하지 않음"은 정확하다. 그 결과 uart 계획 M1에는 R3/R4 승인 경로가 없어진다(c-10). 같은 이유로 사용자 CLI `hold/unhold/input clear`도 role 모델(agent|viewer)에 맞지 않아 뷰어 키로만 남긴다(c-7).

## (c) 결정을 따르기 위한 plan.md 수정 목록 (plan.md는 아직 수정하지 않음)
1. **종료 코드 재매핑(D5)**:
   - 2 = `E_CONFIG_CONFLICT`·`E_UNSUPPORTED_PARAM`·`E_SECRET_MISSING`
   - 4 = `E_DENIED`(구 `E_FORBIDDEN`)·`E_RATE_LIMIT`·`E_RECORDING_FAILED`
   - 5 = `E_CONFIRM_REQUIRED`·`E_APPROVAL_REQUIRED`·`E_NO_APPROVER`
   - 6 = `E_USER_ACTIVE`·`E_HELD_BY_USER`·`E_LOCKED`·`E_LOCK_BROKEN`·`E_REVISION_CONFLICT`·`E_INTERRUPTED_BY_USER`
   - 7 = `E_NOT_OPEN`·`E_NOT_FOUND`·`E_PERMISSION`·`E_PORT_BUSY`·`E_DISCONNECTED`·`E_CONNECTION_CHANGED`·`E_AMBIGUOUS_DEVICE`
   - 8 = gap·`E_SCAN_LIMIT`·`E_PATTERN_TOO_EXPENSIVE`, 9 = 데몬
   - §4.1 표와 SKILL Rules 분기를 함께 고친다.
2. **이벤트·레코드(D3)**: `gap`→`display.gap`, 기록 손실은 `record.gap`, `recording` 상태는 `record.health`로 바꾼다. 필드는 `client`→`client_id`, `actor_id`·`job_id`·`epoch`·`purpose` 추가, `ts.kind`→`ts.source` + `ts.quality`, `(producer_id, producer_epoch, source_seq)` 추가. 외부 `seq` = gateway `stream_seq`로 하고 `rx_offset`은 UART 내부 좌표로만 쓴다. RPC 바이트 인자는 hex(`uart.send.data_b64`→`data` hex), 이벤트 payload는 base64를 유지한다.
3. **쓰기 상태(D4)**: queued/written/drained와 partial/delivery를 `requested→accepted→sent→failed|outcome_unknown`으로 바꾼다. UART 성공 종단은 `sent`다(장치 ACK가 없음). partial은 `outcome_unknown` + `written_bytes`. `send --expect` 매치는 `confirmed`로 승격하지 않고 별도 결과로 둔다. `request_id` 외에 `idempotency_key`를 받는다.
4. **CLI 봉투(D5)**: `meta{truncated,next_cursor,range,display_gap,record_health}`를 쓴다. `user_input_since_last_call` 경고는 `others_since_last`로 대체하고 사람용 출력은 `NOTE:` 줄로 낸다. read 기본 100행, 하드 1000행 / 64 KiB.
5. **커서 문법(D5)**: `@last | seq:N | mark:X | -5s | HH:MM:SS.mmm | ISO8601`로 바꾼다(`cursor`→`@last`, `30s`→`-30s`). UART 확장 `off:N`(rx_offset)과 `send:last`는 유지를 요청한다(승인되지 않으면 삭제).
6. **`LOOM_CLIENT` 기본값(D5)**: `agent` 대신 부모 셸 세션+사용자로 만든 안정 id를 쓴다. DR-38의 `shared_default_cursor` 경고는 삭제한다.
7. **발견·role(D2)**: `~/.loom/run/daemon.json` 하나로 합친다(`uart.json`, `uart.attach.json` 삭제). role은 `agent|viewer`이고 viewer = 대화형 TTY 뷰어다. 사용자 CLI `approve/deny/hold/unhold/input clear`는 삭제한다. 대신 뷰어 키로 남긴다: `Ctrl-] h`, `Ctrl-] A`, `Ctrl-] y`, 그리고 input clear용 키 신설.
8. **프로세스(D1)**: UART는 `loomd` 아래 worker로 동작한다. reader·TX actor·InputArbiter·RingBuffer는 worker 안에 둔다. `loom uart daemon …`은 `loom daemon …`으로 바꾼다. DR-1과 §15-1은 "해결됨"으로 표시한다.
9. **안전(D6)**: 신뢰 모드 3종을 삭제하고 환경 축(`sim|bench|vehicle`) + `r2_channel` + 프로필 allowlist로 바꾼다. 위험 표를 다음처럼 재매핑한다.
   - R2: reset·break·DTR/RTS·configure baud·release·`reboot` 문자열 → `loom uart confirm <tok>` 또는 뷰어 `Y`
   - R3: saveenv·fw_setenv·`sf write`·부트로더 진입 시퀀스·파일 전송 → 뷰어 승인만, UART 기본 scope=once
   - R4: `*erase`·mkfs·`dd of=/dev`·`rm -rf` → 매번 뷰어 승인
   - R4 `default_blocked`: efuse/OTP/락비트
   - R2 세션 승인은 10분 상한. DR-10·DR-11과 §14 Q4·Q5를 다시 쓴다.
10. **M1 안전**: 공통 승인 패널이 CORE P2에 오므로, UART M1의 R3/R4는 `E_NO_APPROVER`(exit 5)로 거절하고 R2는 confirm 토큰으로 처리한다. 뷰어 승인 화면(`Ctrl-] A`)은 공통 승인 패널을 써서 M2로 옮긴다. M1 수용 기준 (f)를 "R2는 토큰 없이 무송신, R3/R4는 항상 무송신"으로 수정한다.
11. **기록(D7)**: `--no-record`는 데이터 페이로드만 생략하고 감사는 항상 남긴다(§3.12와 §15-8 수정). 기록·감사 불능 시 에이전트 TX를 중단하는 규칙은 (a-1) 결정을 따른다.
12. **뷰어(D9)**: `Ctrl-] x` = STOP(전 도메인 latch, (a-3) 범위), hex는 `Ctrl-] g`로 옮긴다. 상태줄에 `LIVE/LAG/STALLED/LIVE-DEGRADED` 배지를 쓰고, 화면 생략 표기를 `display.gap`으로 맞춘다.
13. **스킬(D13)**: `loom skills sync` 복사를 기본으로 한다(DR-29의 커밋 심링크안 삭제). 종료 코드·승인·`others_since_last` 공통 규칙은 공통 `loom` 스킬로 옮기고 `loom-uart` SKILL은 UART 동사와 레시피만 남긴다.
14. **플러그인(D10)**: SafetyGate를 삭제하고 core 쓰기 파이프라인에 UART risk table(문자열 규칙·줄 재조립 판정)을 등록한다. Registry 항목(backend, commands, event kinds, capabilities, risk table, sim)을 §3에 추가한다. 콘솔 엔진 배치는 (a-4) 결정을 따른다.
15. **작업 분해(§3.2)**: T01·T02는 CORE로 넘기고 삭제한다. T05는 core 기록기 위의 UART 저널 어댑터와 RingBuffer로 범위를 줄인다. T08–T13은 CORE stub에 의존하게 하고, T12는 CORE 공통 지연 하네스를 쓴다. P0 = T03–T07이다.
16. **지연 예산 표(§2.0②·§3.11)**: (a-5) 결정에 맞춰 배치 구간 값을 다시 계산한다.
