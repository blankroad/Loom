# P0 보고 — `crates/loom-can-isotp` (ISO-TP 상태기계, can-sol 작업 마무리)

작성: can-opus, 2026-11-11. can-sol이 거의 다 만든 crate를 이어받아 마무리했다(`p0-isotp-finish.md`). 범위: `crates/loom-can-isotp/`와 이 보고서만. 커밋 안 함.

## 결과 (워크스페이스 루트에서)
- `cargo fmt -p loom-can-isotp --check` ✓ · `cargo clippy -p loom-can-isotp --all-targets -- -D warnings` ✓(경고 0, 워크스페이스 lint 포함) · `cargo test -p loom-can-isotp` ✓ **30개 통과 + 1개 ignored**(전수 행렬).
- **기본 시험 시간: 디버그 0.35 s**(빌드 포함 약 2 s). 이전에는 58 s.
- 전수 행렬(`--release -- --ignored`, 73,710 전송): 통과, 3.6 s.
- `unsafe` 없음(`#![forbid(unsafe_code)]`), Python 없음. 의존성은 `loom-can-frame`만.

## 리드 지시 반영
- `Cargo.toml`: 워크스페이스 package 필드(`version/edition/rust-version/publish.workspace = true`) + `[lints] workspace = true`(`loom-can-sim`과 같은 형태).
- `tests/protocol.rs` 맨 위에 `#![allow(clippy::unwrap_used, clippy::expect_used)]`(CORE와 같은 방식). 라이브러리 코드에는 `unwrap`/`expect`가 없다. 모든 공개 타입이 `Debug`라 `missing_debug_implementations`도 통과.
- crate 폴더에 남은 옛 단독 빌드 산출물(`Cargo.lock`, `target/`)은 `.gitignore` 대상이라 그대로 두었다(워크스페이스에서는 쓰이지 않음).

## can-sol 작업 대비 고친 점
| # | 문제 | 수정 | 회귀 시험 |
|---|---|---|---|
| 1 | **제어(FC) 기록 누수**: 종료형 FC(큰 FF에 대한 OVFLW, OVFLW fault)는 이어지는 수신 상태도 그것을 정리할 상대 CF도 없어, 버스 확인을 주지 않는 백엔드에서는 영원히 남는다. 64개(`MAX_CONTROLS`)가 차면 그 뒤 **모든 FF에 `ResourceExhausted`** — 엔드포인트가 사실상 멈춤 | 종료형 FC는 제출(`on_frame_submitted_at`) 시 **그 handle 하나만** 정리(OVFLW는 모두 token 0을 공유하므로 `retire_controls(token)`을 쓰면 아직 큐에 있는 다른 OVFLW까지 지워진다 — 처음 고칠 때 이 실수를 잡아냈다). residual 추정에는 포함 | `overflow_controls_retire_on_submission_without_confirms`(확인 없이 큰 FF 192개, 큐에 OVFLW 2개 중 하나만 제출, OVFLW fault 128회) |
| 2 | **32비트 escape FF(길이 > 4095)에 OVFLW 미송신**: 오류 이벤트만 내고 FC가 없어 송신자가 N_Bs(기본 1 s)까지 기다린다. ISO 15765-2는 FF_DL이 버퍼보다 크면 FC OVFLW | `RxError(PduTooLarge)` + **FC OVFLW**. 진행 중 수신이 있으면 먼저 `UnexpectedPdu`로 끝냄. 12비트에 들어가는 길이를 escape로 보낸 FF는 malformed(`InvalidLength`, FC 없음) 유지 | `escape_ff_above_4095_answers_overflow`(CC·FD) |
| 3 | **자기 STmin 검증 없음**: 설정이 예약값(0x80–0xF0, 0xFA–0xFF)을 받아 FC로 내보낼 수 있었다(송신자가 쓰면 안 되는 값) | `validate`에서 `InvalidConfig("st_min reserved value")`. 받은 예약값은 계속 127 ms | `reserved_stmin_is_refused_in_config_and_means_127ms_on_receive`(0x80/0xFA 수신 시 다음 CF가 정확히 127 ms) |
| 4 | **stale-FC handle(can-sol 마지막 미적용 단계)**: 엔진 동작은 이미 안전했다(수신이 교체되면 `fail_rx → retire_controls`가 옛 FC를 지우고, 그 handle은 `is_frame_pending = false`, 제출·확인은 무시·카운트). 그러나 회귀 시험이 한 경로(새 FF)만, 포맷되지 않은 채(fmt 실패) 있었다 | 엔진 변경 없음(1번 수정이 terminal FC 경로만 바꿈). 시험을 넓혔다 | 아래 3개 |
| 5 | **fmt 실패**: 마지막 시험이 한 줄에 여러 문장 | 워크스페이스 `rustfmt.toml`(max_width 110)로 포맷 | — |
| 6 | **시험 58 s**: 원인은 `all_pdu_lengths_cc_fd_bs_stmin_matrix`(2 × 3 × 3 × 4095 = 73,710 왕복) | 기본 시험은 같은 18개 설정 × **모든 경계 길이**(1, SF 최대/+1, 첫 CF, 1바이트 마지막 CF, CF 1·2·7·8·9·15·16·17·32개, BS·BS+1·2BS 블록 경계, SN wrap, 4094/4095) + 설정마다 seed 표본 24개. 전수 행렬은 `exhaustive_pdu_length_matrix`(`#[ignore]`)로 보존 | `pdu_length_matrix_boundaries_and_sample` |

**stale-FC 회귀 시험(4번):**
- `stale_fc_handle_after_rx_replacement_cannot_touch_new_receive` — 옛 CTS를 꺼내 들고 있는 동안 수신이 4가지로 교체된다: 새 FF / SF 후 FF / N_Cr 타임아웃 후 FF / CTS 전에 온 CF로 실패 후 FF. 각 경우 옛 handle은 pending이 아니고, 제출·확인이 무시·카운트되며, 이벤트·타이머(poll_at)가 바뀌지 않는다. 새 수신은 자기 CTS 제출 뒤 정상 완료된다.
- `stale_cts_does_not_unlock_cf_for_new_receive` — 옛 CTS를 제출해도 새 수신은 여전히 CTS 전 CF를 `UnexpectedPdu`로 거절한다(새 상태가 오염되지 않음).
- `stale_fc_handle_after_abort_is_fenced_by_generation` — abort 뒤 옛 FC는 세대가 달라 fencing된다.

**변형 시험으로 확인:** 1·2번 수정을 일시적으로 끄면 해당 회귀 시험 2개가 실패하고, 되돌리면 통과한다.

## 원래 지시 요구 1–10 대조
| 요구 | 상태 |
|---|---|
| 1 의존 | `loom-can-frame`만 ✓ |
| 2 설정 | 기본값 DL8, padding `Some(0xCC)`, BS 8, STmin 0, N_Bs/N_Cr 1000 ms, wft_max 10, max_pdu 4095 ✓. extended/mixed는 `Addressing`에 자리만 두고 `Unsupported` ✓. FD 유효 길이 올림은 padding=None이어도 0xCC로 채움(README) ✓. 자기 STmin 검증 추가(3번) |
| 3 codec | SF(CC 1..7, FD escape 8..62, CAN_DL > 8이면 짧은 PDU도 escape), FF 12비트·4095 이하, CF SN wrap, FC CTS/WAIT/OVFLW, STmin 0x00–0x7F ms·0xF1–0xF9 100–900 µs·예약값 127 ms ✓. 잘못된 PCI·길이는 오류(전 바이트 × 길이 0..64 무패닉 시험) ✓ |
| 4 송신기 | Busy, N_Bs(FF·블록 끝 CF의 실제 제출 시각부터), BS 단위 CF, STmin은 실제 CF 제출 시각 기준(블록 경계 포함), WAIT 초과 → `WaitLimit`(wft_max번까지 허용), OVFLW → 오류 ✓ |
| 5 수신기 | SF 즉시 완료, FF → `RxStart`를 FC 요청보다 먼저, SN 검사, N_Cr, 수신 중 새 SF/FF → 오류 후 새 수신, 상관없는 FC 무시·카운트 ✓. 큰 FF → OVFLW(2번으로 escape까지) |
| 6 완료 상태 | 모든 프레임 제출 → `DriverAccepted`, 모든 프레임 확인 → `BusConfirmed`(같은 transfer id, 단조). 마지막 프레임만 확인해서는 오르지 않음 ✓ |
| 7 abort | 남은 CF/FC 제거, 세대 증가, `residual_tx_possible` 보고, 늦은 FC/CF·콜백 무시·카운트 ✓ |
| 8 poll_at | STmin 다음 CF, N_Bs, N_Cr, 지연 FC 중 가장 이른 시각 ✓ |
| 9 결함 훅 5종 | CF n번째 누락, 잘못된 SN, FC 지연, FC WAIT k회, FC OVFLW. 기본 꺼짐, 실HW 차단은 worker 책임(README) ✓ |
| 10 C2 | 출력 큐 고정 용량(기본 32/16, 최대 64) — 각 입력이 최악의 경우 슬롯을 먼저 예약하고 모자라면 상태 변경 전에 `OutputFull`. PDU 버퍼 max_pdu, 전송 기록 4, 제어 기록 64, 카운터 포화 ✓. 1번 누수 수정 |

## 공개 API 요약 (README와 대조 완료)
- `IsoTpConfig::new(tx_id, rx_id)` / `validate()` / `single_frame_capacity()`, `Addressing`, `FaultPlan`, `QueueLimits`, 상수 `MAX_PDU`(4095), `MAX_OUTPUTS`(64), `MAX_TRANSFERS`(4), `MAX_CONTROLS`(64), `MAX_DATA_FRAMES`(586).
- codec: `encode_sf/ff/cf/fc`, `decode -> Pci`, `FlowStatus`, `st_min_ns`.
- `Engine::new` / `with_limits`, `send(pdu, now) -> TransferId`, `on_frame(frame, now)` / `on_frame_in_generation(frame, generation, now)`, `pop_frame() -> FrameRequest { handle, transfer_id, frame, not_before_ns }`, `pop_event() -> Event`(`TxDone{tx_state}`, `TxError{residual_tx_possible}`, `RxStart`, `RxComplete`, `RxError`), `is_frame_pending(handle)`, `on_frame_submitted_at(handle, now)` / `on_frame_submitted(handle)`, `on_frame_confirmed(handle)`, `release_transfer(id)`, `poll_at()`, `on_timer(now)`, `abort(now) -> AbortReport`, `generation()`, `stats()`, `is_sending()`, `is_receiving()`, `output_len()`, `config()`, `fault_plan()`.
- README의 사용 절차(1–7), 완료 상태, 시간 의미, abort/세대, C2 상한 설명이 코드와 맞는다. 바뀐 부분(escape FF OVFLW, STmin 검증, 종료형 FC 정리, 시험 명령)을 README에 반영했다.

## 시험 목록 (30 + 1 ignored)
codec(PCI 표, STmin 256값 전체 표, SF/FF 경계 7/8·62/63·4095/4096, padding/FD escape, 잘못된 입력 무패닉) · 행렬(경계 + 표본, 18개 설정) · FD DL 8..64 × padding 3종 · seed 무작위 왕복 512개(29비트, 무작위 DL·BS·STmin·padding·확인 유무) · N_Bs/N_Cr·Busy·DriverAccepted · 완료 단조·상관 · 모든 프레임 확인 필요 · 잘못된 SN·새 FF 교체 · OVFLW/WAIT 초과/상관없는 FC · abort 세대 울타리 · 수신 abort 시 지연·대기 FC 제거 · 결함 훅 5종(+WAIT 한도 안·지연은 성공) · 큐 상한·전송 기록 상한 · 큐가 가득 찬 상태의 abort · 지원 안 하는 주소·잘못된 설정 · 작은 max_pdu에서 할당 없는 OVFLW · 잘못된 id·모드·시계 역행·빈 PDU · 서로 다른 TX_DL 전이중 · 블록 경계에서 실제 제출 기준 STmin · 마감 시각 입력·큐 재시도에서 거짓 성공 없음 · **신규 6개**(위 표) · ignored: 전수 행렬.

## 계획과 다른 점 / 리드 결정 요청
1. **CAN 모드 불일치는 엄격하다**: FD 엔드포인트에 Classic 프레임이 오면(또는 그 반대) `RxError(InvalidLength)`. TX_DL 8인 상대가 FD 엔드포인트에 Classic FC로 답하는 혼합 구성이 실제로 허용되는지는 **미확인**(ISO 15765-2:2016 원문 대조 필요). 혼합을 받아야 하면 "수신은 양쪽 허용" 옵션을 추가할 수 있다.
2. **CF 누락 fault의 의미**: 송신자는 누락된 CF를 제출한 것처럼 진행하므로 송신 측에 `TxDone(DriverAccepted)`가 나온다(버스에서 프레임이 사라진 상황의 흉내, 수신자는 N_Cr). sim 전용이라 그대로 두었고 README에 적혀 있다.
3. **확인을 주지 않는 백엔드의 전송 기록**: 기본 4개 기록이 차면 `send`가 `ResourceExhausted`이므로, worker는 `DriverAccepted`/실패 뒤 `release_transfer(id)`를 불러야 한다(can-sol 설계, README 명시). 백엔드 capability(`tx_confirmation=false`)이면 자동 해제하는 설정 플래그를 둘지 결정 요청.
4. `residual_tx_possible`은 마지막 abort 이후 정리된 제출·미확인 FC를 누적해 보수적으로 크게 잡는다(종료형 FC 정리분 포함). "확실히 버스에 남은 수"가 아니다(README 명시).
5. 공개 crate(`can-iso-tp`, `automotive`)와의 프레임 바이트 차등 비교(선택 항목)는 하지 않았다. 자기 왕복은 적합성 인증이 아니다.
6. `proptest` 대신 seed 고정 무작위 왕복을 유지했다(can-sol 선택, 의존성 추가 없음).
