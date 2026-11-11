# CAN ↔ UDS: CAN-TP(ISO 15765-2) 경계 인터페이스

상태: **초안 v0.3 (UDS 리드 작성, 2026-11-09 — v0.2 변경점 §9, v0.3 추가 요구 §11. UDS 쪽 확정안은 `docs/plan/uds/plan.md` §15.2)** — CAN 팀 의견을 `## can 의견` 절에 덧붙여 주세요. 합의되면 양 팀 plan.md가 이 문서를 참조합니다.
근거: 공통 브리프 §3-8 (CAN 데몬이 ISO-TP 스택을 프로세스 안에서 호스팅).

## 1. 원칙

1. **ISO-TP 프레임 처리(FF/CF/FC, STmin, BS, N_* 타이머)는 전부 CAN 데몬 프로세스 안**에서 한다. IPC 너머로는 **PDU(재조립된 UDS 메시지) 단위**만 오간다.
2. UDS 측은 ISO-TP 내부 타이밍을 몰라도 된다. 대신 데몬은 **타임스탬프를 붙여** 보고한다 (UDS가 P2/P2* 측정을 IPC 지연이 아닌 버스 시각 기준으로 할 수 있도록).
3. ISO-TP가 주고받은 원시 CAN 프레임도 **CAN 트레이스/기록에 그대로 보인다** (origin 태그 유지). 숨은 세션 금지(공통 §3-5).
4. 구현 후보: `can-isotp` (python-can-isotp, MIT) 2.x의 `NotifierBasedCanStack` — 데몬의 python-can `Notifier`를 공유. 최종 선택은 CAN 팀 결정.

## 2. 채널 열기

```
isotp.open(
  can_channel: str,              # CAN 데몬 채널 이름, 예 "ch0"
  addressing: "normal_11" | "normal_29" | "normal_fixed_29" | "extended_11" | "extended_29" | "mixed_11" | "mixed_29",
  tx_id: int, rx_id: int,        # physical
  func_tx_id: int | null,        # functional 요청용 CAN ID (예 0x7DF, 0x18DB33F1)
  target_address / source_address / address_extension: int | null,   # extended/mixed/fixed 용
  params: {
    can_fd: bool, tx_dl: 8|12|16|20|24|32|48|64, bitrate_switch: bool,
    tx_padding: int | null (기본 0xCC 또는 0x55 — 미정),
    stmin_rx: int (우리가 FC로 요청할 STmin, 기본 0),
    blocksize_rx: int (기본 0 = 무제한),
    override_tx_stmin: float | null,   # 상대가 준 STmin 무시(테스트용)
    n_bs_ms: int (기본 1000), n_cr_ms: int (기본 1000),
    wft_max: int (기본 0), max_pdu: int (기본 4095, FD는 2^32-1 허용 여부 CAN팀 판단)
  },
  owner: {client_id, origin: "agent"|"user"|"script"}
) -> { isotp_id, effective_params }
isotp.close(isotp_id)
isotp.list() -> [{isotp_id, can_channel, ids, owner, state, counters}]
```

- **소유권**: 같은 (can_channel, tx_id, rx_id) 조합은 동시에 한 isotp 채널만 열 수 있다. 두 번째 open은 `ERR_BUSY` + 현재 소유자 정보. UDS 세션 관리자가 대표로 열고, 에이전트/사용자 CLI는 UDS 세션 관리자를 경유한다.
- 같은 rx_id 프레임을 다른 클라이언트가 raw로 구독하는 것은 허용(읽기 전용). 같은 tx_id로 raw 송신은 경고 이벤트 발생(차단 여부는 CAN 팀 판단).

## 3. 송수신

```
isotp.send(isotp_id, data: bytes, functional: bool = false, req_id: str) 
   -> 즉시 반환 {accepted}            # 큐잉
event isotp.tx_done   {isotp_id, req_id, ts_first_frame, ts_last_frame}
event isotp.tx_error  {isotp_id, req_id, code: "N_BS_TIMEOUT"|"FC_OVERFLOW"|"WFT_OVRN"|"BUS_ERROR"|"BUSY", detail}
event isotp.rx        {isotp_id, data, ts_first_frame, ts_last_frame, rx_can_id, functional_rx: bool}
event isotp.rx_error  {isotp_id, code: "N_CR_TIMEOUT"|"WRONG_SN"|"UNEXPECTED_PDU"|"OVERFLOW", detail}
```

- 타임스탬프: 공통 이벤트 레코드의 `ts` 형식(monotonic + wall). 가능하면 HW 타임스탬프 기준.
- **UDS P2 타이머 시작점 = `tx_done.ts_last_frame`**, 응답 시각 = `rx.ts_first_frame`. 따라서 IPC 지연은 판정에 영향을 주지 않는다 (단, 타임아웃 *대기* 자체는 UDS 측 벽시계로 하므로 IPC 지연만큼 여유를 더한다).
- 선택: `send_and_wait(isotp_id, data, timeout_ms)` 동기 헬퍼 — 단순 클라이언트용.

## 4. 기능 주소(functional) 요청

- functional 송신은 **Single Frame만** 허용 (ISO 15765-2). CAN FD면 SF 최대 62바이트, 아니면 7바이트. 초과 시 `ERR_FUNC_TOO_LONG`.
- 응답은 여러 ECU의 서로 다른 rx_id로 온다. 요구: **isotp 그룹** 개념
  ```
  isotp.open_group(can_channel, func_tx_id, members: [{tx_id, rx_id}...], params) -> group_id
  isotp.send(group_id, data, functional=true) → 각 멤버의 isotp.rx 이벤트가 member 태그와 함께 옴
  ```
  멤버 응답이 멀티프레임이면 데몬이 해당 멤버 rx_id에 FC를 보내야 하므로 멤버별 ISO-TP 상태기가 필요.
- (선택) "알 수 없는 응답자 탐색": 정해진 rx_id 범위(예 0x7E8–0x7EF, 29bit 0x18DAF1xx)를 수신해 응답자 목록을 보고 — UDS `discover --can` 용.

## 5. 주기 PDU (TesterPresent 유지)

`loom uds` CLI는 명령마다 프로세스가 끝나므로 S3(기본 5 s) 동안 세션을 유지할 상주 주체가 필요하다. UDS 세션 관리자가 상주 프로세스 안에서 돌리는 것이 기본안이지만, 지터를 줄이고 UDS 쪽이 죽어도 정리되도록 데몬 기능으로 요청한다:

```
isotp.periodic_start(isotp_id | group_id, data=b"\x3E\x80", period_ms=2000, functional: bool) -> job_id
isotp.periodic_stop(job_id)
```
- 소유 클라이언트 연결이 끊기면 **자동 정지** (ECU가 S3 만료로 기본 세션 복귀 = 안전 쪽 실패).
- 주기 PDU 송신도 트레이스에 origin=`script`(또는 `agent:tp-keepalive`)로 보인다. 뷰어에서 접기(collapse) 가능하면 좋음.

## 6. 에러·상태 보고

- 채널 상태 이벤트: `isotp.state {isotp_id, state: "open"|"closed"|"bus_off"|"error_passive"}` — CAN 채널 상태 변화 전파.
- 카운터: tx_pdu, rx_pdu, tx_err, rx_err, fc_wait_rx 등 `isotp.list`에 포함.

## 7. 성능 요구 (UDS 기준)

| 항목 | 요구 | 근거 |
|---|---|---|
| FC 응답 지연(데몬 내) | ≤ 수 ms, N_Br 여유 내 | 상대 ECU N_Bs 기본 1000 ms지만 실차 ECU는 더 짧게 설정되기도 함 |
| CF 송신 STmin 준수 | STmin 0 ~ 127 ms, F1–F9(100–900 µs)는 best-effort | Python 타이머 해상도 한계 — 실측 필요(CAN 팀 스파이크) |
| PDU → UDS 이벤트 전달 | ≤ 10 ms (로컬 IPC) | P2 50 ms 대비 충분, 판정은 타임스탬프 기반 |
| 0x36 TransferData 처리량 | 4 KB 블록 기준 수 KB/s 이상 (플래시 범위에 포함될 경우) | 실측 후 확정 |

## 8. 미결 질문 (CAN 팀께)

1. ISO-TP 엔진: can-isotp 2.x `NotifierBasedCanStack` 사용 OK? 아니면 자체 구현?
2. 패딩 기본값(0xCC vs 0x55 vs 없음) — 채널 open 파라미터로 지정, 기본값만 정하면 됨.
3. 같은 tx_id raw 송신 충돌 시 차단 vs 경고.
4. 주기 PDU 기능을 데몬이 제공할지(§5). 거절 시 UDS 세션 관리자가 자체 타이머로 처리.
5. IPC 프로토콜(공통 §3-3) 확정 전까지 위 API는 이름·필드 수준 합의만 하고 전송 방식은 공통 결정을 따른다.

## 9. v0.2 변경점 (UDS R1 조사·스파이크 근거) — v0.1 해당 절보다 우선

근거: `docs/plan/uds/r1-sol.md` §3.3, §7, §9.2 (virtual bus + can-isotp 2.0.7 + 별도 프로세스 Pipe IPC로 `22F190` 100회 왕복 median 0.92 ms / p99 3.09 ms, FF/FC/CF 재조립 오류 0 — macOS ARM64 실측).

1. **핵심 원칙(02-core-principle.md)**: ISO-TP 채널로 오가는 원시 프레임(FF/CF/FC 포함)과 PDU 이벤트는 모두 공통 이벤트 스트림에 실린다. 사용자 뷰어가 늦게 붙어도 같은 스트림을 받는다. UDS 전용 숨은 채널 없음.
2. **필드 추가**: `isotp.open` 결과에 `epoch`(재오픈마다 증가), `isotp.send`에 `req_id`, `deadline_ms`. 늦게 도착한 응답은 이전 epoch로 격리.
3. **`rx_start` 이벤트 추가 (필수)**: `event isotp.rx_start {isotp_id, epoch, ts_first_frame, expected_length, rx_can_id}` — FF(또는 SF) 수신 즉시 발행. 이유: UDS P2는 "첫 응답 프레임" 기준인데 v0.1처럼 완성 PDU 때만 알리면, 제시간에 온 FF 뒤 CF가 길게 이어질 때 UDS가 P2 타임아웃으로 오판한다. `rx`는 `rx_complete`로 이름 변경, `ts_first_frame/ts_last_frame/frame_count` 포함.
4. **타임스탬프 출처 표시**: `tx_done`·`rx_*`에 `ts_source: "hw"|"host"`와 가능하면 불확실성. `tx_done`이 드라이버 큐 적재 시각인지 실제 버스 송신 시각인지 구분.
5. **TesterPresent는 UDS 세션 관리자가 소유** (v0.1 §5 철회 → 선택 기능으로 강등). UDS 큐/인플라이트/제어권을 모르는 무조건 주기 송신은 진행 중 요청과 충돌할 수 있다. CAN 팀이 주기 PDU를 제공한다면 `idle_only`(인플라이트 중 억제), 소유자 연결 끊김 시 자동 정지, 감사 이벤트가 계약에 포함돼야 하며 UDS는 기본 미사용.
6. **origin**: 공통 enum(agent/user/device/script)을 늘리지 않는다. keepalive 같은 구분은 `purpose: "tester_present"`, `actor_id` 별도 필드. ECU 응답 프레임은 origin=device, `request_origin`으로 송신 주체 연결.
7. **소유권 키**: (can_channel, addressing, tx_id, rx_id, source/target/extension address). 활성 ISO-TP 채널의 tx_id로 다른 클라이언트가 raw 송신하면 **기본 차단**(경고만으로는 응답 혼선을 막지 못함). 사용자 명시 override 시에만 허용 + 감사.
8. **기능 주소 SF 최대 길이**: 주소 바이트에 따라 다름 — normal: classic 7 / FD 62, extended·mixed(주소 1바이트): classic 6 / FD 61. v0.1 §4 수정.
9. **max_pdu**: FD라도 초기 4095 B 상한. 2^32−1 무제한 할당 금지.
10. **PDU 이벤트 전달 지연 목표**: 데몬 내 PDU 완성 → UDS 수신 p99 ≤ 5 ms (같은 프로세스면 사실상 0). CAN 팀 측정 조건과 맞춰 재협의.

## 10. 일정 합의 제안 (UDS 리드)

- CAN 팀 R1(Opus 초안)은 **ISO-TP 최소판(normal 11bit, virtual)을 CAN M2**, 전체(주소 모드/FD/기능주소)를 M4에 둔다.
- UDS는 이에 맞춰 **UDS M1 걷는 뼈대를 DoIP 가상 ECU로 완성**(CAN 의존 없음)하고, **CAN-TP 경로는 UDS M2**(CAN M2 ISO-TP 최소판 위)로 둔다. UDS가 CAN 데몬을 우회해 별도 Bus를 여는 일은 없다.
- 요청: CAN M2 ISO-TP 최소판에 위 §9의 `rx_start`, `epoch`, `req_id/deadline`, 소유권 키, raw tx 차단까지 포함. 주소 모드 전체·FD·기능주소 그룹은 M4 그대로 OK.
- 프로세스 구성: CAN 팀 Opus 초안과 UDS Opus 초안 모두 **단일 상주 `loomd` 안의 도메인 모듈**을 제안 → ISO-TP API는 인프로세스 호출이 기본이고, 같은 API를 RPC로도 노출(분리 배치 대비). 최종 결정은 메인(공통 §3-2/3-3).

## 11. v0.3 추가 요구 (UDS R2/R3 통합 결과)

| ID | 요구 | 필요 시점 |
|---|---|---|
| R-TP-07 | `isotp.send`에 `request_id` 전달 → 해당 ISO-TP 원시 프레임(FF/CF/FC) 트레이스 이벤트에 같은 `request_id` 주석 (viz V-UDS-5, CAN·UDS 트레이스 같은 타임라인) | M1b |
| R-TP-08 | `isotp.flush/abort(isotp_id)` — 진행 중 송수신 중단 (사용자 긴급 정지) | M1b |
| R-TP-12 | **서버 측 채널**: virtual 백엔드에서 tx/rx를 뒤집어 open 가능 (가상 CAN-TP ECU 노드용) | M1b |
| R-TP-13 | 결함 주입 훅(sim): CF 누락, FC 지연/WAIT 반복, 잘못된 SN, CF 지연 | M1b(최소) / M3 |
| R-TP-04 | 응답자 범위 수신 모드(rx_id 범위) — `discover --can` | M3 (필수) |
| R-TP-11 | `isotp.set_params(isotp_id, …)` 재open 없이 파라미터 변경 | M3 |
| R-TP-14 | 채널 상태 이벤트(bus_off/error_passive) → UDS 대상 link 상태 | M1b |
| R-TP-15 | 진단 PDU 이벤트를 원시 프레임 이벤트보다 우선 전달 (CAN 100% 부하에서도 UDS 트랜잭션 → 뷰어 목표 유지). 단 장치 응답 도착 시간 자체는 보장 대상 아님 — 수신 이벤트→화면 지연과 loss/degraded 표시를 시험 | M1b 측정, M3 수용 |
| R-TP-06 | 인프로세스 API와 RPC API 동일 시그니처(asyncio) | M1b |
| — | 외부 송신 감지는 "예약 tx ID의 예상 밖 프레임" 수준(외부 송신자 신원은 확정 불가, 로컬 송신만 확정) | M3 |

UDS 측 일정: **UDS M1 = DoIP sim**(CAN 의존 없음), **UDS M1b = CAN-TP sim**(CAN M2 K9a ISO-TP 최소판 + §9 필드 + 위 M1b 항목), **UDS M3 = CAN-TP 전체**(CAN M4 K9b).


## 12. R4 추가 (UDS 최종 점검 결과)

- **활성 ISO-TP tx ID 차단 범위**: raw 송신뿐 아니라 로그 재생(replay), 주기 송신, IG(신호 송신 패널) 경로에도 같은 차단 또는 해당 ID 필터를 적용해 달라. 진단 프레임이 섞인 BLF를 재생하면 UDS 세션 중간에 요청이 주입된다. 의도적 주입 강건성 시험은 "UDS disconnect 후" 절차로 한다.
- 외부 송신 감지는 "예약 tx ID의 예상 밖 프레임" 이벤트로 충분(외부 송신자 신원 확정 불가, 로컬 송신만 확정).
- CAN 100% 부하에서는 장치 응답 도착 시간을 보장 대상으로 두지 않고, 수신 이벤트 → 화면 지연과 loss/degraded 표시를 시험한다.

## can 의견

작성: can-lead, 2026-11-09 (CAN plan.md §15.2와 동일 내용. CAN R1/R2 실측 근거: `spikes/can/r1-results.json`, `r2-results.json`)

**전체: v0.2를 기준으로 수용한다.** PDU 단위 IPC, ISO-TP 프레임 처리는 CAN 쪽 장치 owner 프로세스 안, FF/CF/FC 원시 프레임과 PDU 이벤트가 공통 스트림·저널에 보임, can-isotp 2.0.7 `NotifierBasedCanStack`(Notifier 단일 소비자 공유), §9의 epoch·req_id·deadline·`rx_start`/`rx_complete`·ts_source·소유권 키·raw tx 차단·functional SF 길이·max_pdu 4095·origin 4종, §10 일정(CAN M2 최소판 / M4 완성, UDS M1은 DoIP) 모두 동의.

**중요 변경 1건 (공통 계약 변경 요청 CR1로 메인에 올림): CAN 장치 owner는 M1부터 별도 프로세스(CAN worker).** 사용자에게는 `loomd` 하나(포트·URL·승인·감사 하나)지만 내부는 gateway + CAN worker. 근거: CAN R1 실측에서 같은 프로세스에 CPU-bound Python 스레드가 있으면 FF→FC host 송신 지연 p99 1.49 → 51.9 ms(max 60.5), 10 ms 주기 송신 오차 p99 4.6 → 46.8 ms. 따라서 v0.2 §10의 "ISO-TP API는 인프로세스 호출이 기본"은 **"UDS 세션 관리자(gateway 측) ↔ CAN worker 간 PDU RPC가 기본"**으로 바뀐다. UDS 실측(별도 프로세스 Pipe IPC 22F190 왕복 p99 3.09 ms)으로 감당 가능하다고 본다. §9-10의 "같은 프로세스면 사실상 0"은 해당 없음 → PDU 완성→UDS 수신 p99 ≤ 5 ms를 CAN M2에서 측정해 보고한다.

| 항목 | CAN 결정 | 이유 |
|---|---|---|
| §8-1 엔진 | can-isotp 2.0.7 `NotifierBasedCanStack` | Notifier 공유, MIT, python-can 4.6.1 + virtual 왕복 실측 성공. 구형 `CanStack`(직접 recv) 금지 |
| §8-2 패딩 기본 | **Classic CAN 기본 0xCC**, `tx_padding:null`(패딩 없음) 명시 가능, `loom.toml`의 ECU 프로파일(`isotp_profiles.<name>`)이 우선. `isotp.open`에 `profile` 인자(선택) 추가 제안 | 실 ECU 다수가 DLC 8 요구 → 패딩 없음 기본은 원인 찾기 어려운 무응답. FD의 DLC 맞춤 패딩은 라이브러리 규칙 |
| BS 기본 (우리가 수신측) | `blocksize_rx` **8** (v0.1 초안 0에서 변경 제안), 프로파일로 0 가능 | 수신 큐 보호 |
| §8-3 / §9-7 같은 tx_id raw 송신 | agent·user 모두 **기본 차단**, 명시 takeover(기존 소유자 정지·감사)만 | v0.2와 동일 |
| §8-4 / §9-5 주기 PDU | **TesterPresent는 UDS 세션 관리자 소유**에 동의. CAN은 별도 ISO-TP 주기 API를 만들지 않는다. 일반 CAN 주기 job은 존재하나 UDS 기본 미사용 | 진행 중 요청과 충돌은 UDS만 안다 |
| §8-5 IPC | 공통(메인) 결정 따름. CAN 제안: JSON-RPC 2.0 over WS(외부) + 내부 worker IPC. 바이트는 **hex 문자열**로 통일. `send_and_wait`는 같은 async job을 기다리는 얇은 wrapper. 이벤트를 못 받는 클라이언트용 `isotp.recv({isotp_id, after_pdu_seq, timeout_ms, max_pdus})` 추가 | |
| §9-4 시각 | `tx_done`은 기본 **드라이버 접수 시각**(`tx_state:"accepted"`), TX 확인을 주는 backend에서만 `"confirmed"`. 첫/마지막 프레임 시각은 CAN 쪽 frame→PDU 추적 hook으로 붙이고, 추적이 불확실하면 `ts_quality:"estimated"`. HW 시각도 python-can이 이미 host offset을 더한 값임(Vector) | `bus.send` 반환 ≠ wire 완료 |
| P2 판정 (추가 제안) | UDS 측 단조 deadline + **drain barrier**: deadline 시점에 UDS가 CAN worker에 watermark("seq X까지 처리했나")를 요청하거나 CAN이 주기 watermark 이벤트를 보내, 그 이전에 `rx_start`가 없었음을 확인한 뒤 timeout 판정. late 이벤트는 epoch/req_id로 격리 | "timestamp만 쓰면 IPC 무관"은 불충분 — deadline 시각에 이벤트가 아직 IPC 위에 있을 수 있음 |
| 취소 (추가) | `isotp.cancel({transfer_id}) -> {state, residual_tx_possible}`, `tx_error.code`에 `CANCELLED` | 이미 드라이버 큐에 들어간 CF는 취소 불가할 수 있음 |
| §4 unknown responder | 관측(SF 기록)만. 새 주소에 자동 FC 송신·자동 stack 생성 금지. 응답 범위는 allowlist | 자동 FC는 장비에 새 쓰기 |
| lease (추가) | `isotp.open`에 `lease_ms`와 소유자 heartbeat. 소유자 끊김/lease 만료/STOP/bus-off 시 신규 송신 중단, 크래시 후 자동 재개 없음 | |
| 위험 등급 | CAN은 `isotp.open`을 R3(bench, `tx=confirm`이면 세션 승인 1회)로 판정하고, 이후 PDU 의미(리셋·보안접근·플래시)는 UDS 정책이 판정. CAN은 프레임 레벨 rate·STOP·ID 예약만 적용 | |
| 성능 목표 | FC: CAN worker에 CPU 작업이 없는 조건에서 측정해 M2 보고(idle 실측 p99 1.49 ms). STmin 0~127 ms 준수, F1–F9는 best-effort | |

**UDS 팀에 질문**
1. UDS 세션 관리자는 gateway 프로세스 안에 사는가(=CAN worker와 PDU RPC)? 아니면 별도 UDS worker인가? (CAN 쪽 API는 동일, 배치만 확인)
2. 플래시(0x36) 범위 포함 시 필요한 처리량(KB/s)과 FD 사용 여부 — M4 성능 시험 조건으로 쓴다.
3. UDS 가상 ECU(서버 시뮬레이터)를 CAN worker의 SimNode로 붙일 때, UDS 팀이 "PDU in → PDU out" 콜백(순수 함수형)을 제공해 줄 수 있는가? (virtual은 같은 프로세스에서만 연결되므로 CAN worker 안에서 돌아야 함)

## uds 답변 (CAN 의견에 대해)

작성: uds-lead, 2026-11-09. UDS 확정안: `docs/plan/uds/plan.md` §3.1, §3.4, §9.1, §16 D3.

**전체: CAN 의견을 수용한다.** CR1(CAN worker 별도 프로세스)에 동의하며 UDS plan.md D3·C1을 그에 맞게 바꿨다. 패딩 0xCC 기본 + `profile` 인자, `blocksize_rx` 8, `tx_state: accepted|confirmed`·`ts_quality`, drain barrier, `isotp.cancel`, `isotp.recv`(폴백), unknown responder 관측만(자동 FC 금지), `lease_ms` + 소유자 heartbeat, 바이트 hex 문자열 — 모두 동의.

질문 답:
1. **UDS 세션 관리자는 gateway 프로세스 안**에 산다(core와 같은 프로세스). CAN worker와는 PDU RPC(`IsoTpPort`). 별도 UDS worker는 두지 않는다(UDS 트래픽은 저속, CPU 부하 작음). seed-key 플러그인만 서브프로세스.
2. **플래시는 비범위**(R4, OEM 프로필 M5 선택). M4 성능 시험 조건으로는 TransferData 근사 부하만: classic 500 kbit/s, 4 KB 블록, STmin 0, BS 0(우리가 송신측이므로 상대 FC를 따름) / FD 2 Mbit/s tx_dl 64. 처리량 목표는 정하지 않고 측정값 보고만 요청(추정 상한 classic ≈ 수십 KB/s).
3. **가능.** `VirtualEcuModel.handle(pdu: bytes, now_ns: int) -> list[tuple[delay_ms, bytes]]` — 상태는 모델 내부, I/O·스레드 없음, 결정적(시드 고정). CAN worker SimNode가 서버 측 ISO-TP 채널(R-TP-12)에 붙여 실행하고, 결함 주입 설정 변경은 RPC로 전달한다. 같은 모델을 UDS의 DoIP 엔티티 어댑터도 쓴다.

위험 등급 조율 (이중 승인 방지 제안):
- `isotp.open` 자체는 프레임을 보내지 않으므로, **UDS 모듈이 연 채널**은 CAN 쪽에서 별도 R3 세션 승인을 요구하지 않고 "UDS 정책 위임" 표시로 감사만 남기는 것을 제안한다. 채널 레벨의 송신 금지(listen-only)·STOP·ID 예약·프레임 rate는 CAN이 그대로 집행하고, PDU 의미 판정(리셋·보안·쓰기)은 UDS 가드가 매 PDU 집행한다. UDS 대상 화이트리스트 밖이면 UDS가 R0만 허용한다.
- CAN이 그래도 채널 open 승인을 유지해야 한다면, `connect --can` 1회에 UDS 승인 패널 하나로 합쳐 보여 주는 방식(공통 승인 서비스 C4)으로 사용자에게 두 번 묻지 않게 하자.

## can 회신 (v0.3 §11·§12 + uds 답변에 대해)

작성: can-lead, 2026-11-09. CAN 확정안: `docs/plan/can/plan.md` §3.3, §6.1, §9.1, §10.2, §10.6, §11 M2/M4, §12 K10/K14, §15.2, §16 D13·D25·D28·D29.

**합의 완료로 본다.** uds 답변 전체(UDS 세션 관리자는 gateway 안, 플래시 비범위·M4 측정 조건, `VirtualEcuModel.handle(pdu, now_ns) -> [(delay_ms, bytes)]`)를 수용하고, v0.3 §11·§12 요구를 전부 일정에 넣었다.

| 요구 | CAN 수용 | 시점 (CAN 작업) |
|---|---|---|
| R-TP-06 인프로세스/RPC 동일 async 시그니처 | 수용 | M2 (K10) |
| R-TP-07 `request_id` → 원시 FF/CF/FC 트레이스 주석 | 수용 — 프레임 이벤트에 `request_id`, `isotp_id` 필드 | M2 (K10) |
| R-TP-08 `isotp.abort(isotp_id)` | 수용 — `{aborted_tx, aborted_rx, residual_tx_possible}`. CAN STOP(`stop-all`)도 내부적으로 TX latch + abort 사용 | M2 (K10) |
| R-TP-12 서버 측 채널 (virtual) | 수용 — `isotp.open({…, role:"server"})`. `can-sim` worker의 SimNode가 VirtualEcuModel을 여기에 붙임 | M2 (K10) |
| R-TP-13 결함 주입 훅 | 최소(CF 누락, FC 지연/WAIT 반복, 잘못된 SN, CF 지연) M2, 확대 M4 | M2 / M4 |
| R-TP-14 채널 상태 이벤트 | 수용 — `isotp.state`, CAN 채널 bus_off/error_passive 전파 | M2 (K10) |
| R-TP-15 진단 PDU 우선 전달 | 수용 — PDU·rx_start·오류·STOP·상태는 raw 배치와 분리된 **우선 제어 큐**. M2 측정, M3 수용(viz V-UDS-0 p95 ≤ 100 ms / p99 ≤ 200 ms, 100% 부하 포함) | M2 / M3 |
| R-TP-04 응답자 범위 수신 | 수용 — 관측만(자동 FC 금지, allowlist) | M4 (K14) |
| R-TP-11 `set_params` 재open 없이 | 수용 | M4 (K14) |
| 외부 송신 감지 | 수용 — "예약 tx ID의 예상 밖 프레임" 이벤트(`external_actor=unknown`) | M4 (K14) |
| §12 예약 ID 차단 범위 | 수용 — raw·IG·주기·replay 모두 하드 차단(agent·user) | M2 (K8/K10) |
| 이중 승인 방지 | 수용 — UDS 모듈이 연 채널은 CAN R3 승인 생략, "UDS 정책 위임" 감사. CAN은 채널 tx 정책(tx=deny면 listen_mode만)·STOP·ID 예약·프레임 rate 집행 | M2 |

**작업 이름 매핑:** UDS 문서의 "CAN M2 K9a" = CAN plan **K10**(ISO-TP 최소판, CAN M2), "CAN M4 K9b" = CAN plan **K14**(ISO-TP 완성, CAN M4). UDS M1b ↔ CAN M2, UDS M3 ↔ CAN M4.

**추가 계약 2건 (CAN R4에서 확정)**
1. **drain barrier 정의:** `isotp.barrier({isotp_id, epoch, deadline_mono_ns, timeout_ms}) -> {covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}`. CAN worker는 해당 host-ingest 시각까지의 **ISO-TP transport 처리와 `rx_start` 발행 완료**를 확인한 뒤 응답한다(raw 큐 drain만으로 complete라 하지 않음). barrier도 유한하며 실패 시 UDS는 "판정 불확실"로 보고.
2. **순번:** 프레임·PDU 이벤트의 원본 ID는 `(producer_id, producer_epoch, source_seq)`(CAN worker 할당, 저널 보존), 구독·커서는 gateway의 `stream_epoch/stream_seq`. `isotp.open`의 `epoch`(재오픈 카운터)와는 별개 필드다.

## v1.1 이름·상태 정본 (마스터 A14·A16 반영) — can-lead 작성, 2026-11-10

**이 절은 문서 위쪽(§2~§12, can 의견/회신)의 모든 이름과 `tx_state` 값보다 우선한다.** 의미는 그대로 두고 이름만 바꾼다. 정확한 이벤트 표기(구분자)는 CORE 계약 v1이 확정하며, 그때까지 아래 형식을 쓴다. CAN 정본: `docs/plan/can/plan.md` §15.2, §16 D35·D41.

**A16 — RPC/이벤트 이름 (`<domain>.<noun>.<verb>`, 이벤트 `<domain>.<event>`)**
| 이전 | v1.1 |
|---|---|
| `isotp.open` / `send` / `recv` / `cancel` / `abort` / `barrier` / `close` / `list` / `set_params` / `open_group` | `can.isotp.open` / `can.isotp.send` / … / `can.isotp.open_group` (모든 메서드에 `can.` 접두) |
| `event isotp.tx_done` / `isotp.tx_error` | `can.isotp_tx_done` / `can.isotp_tx_error` |
| `event isotp.rx_start` / `isotp.rx_complete` / `isotp.rx_error` | `can.isotp_rx_start` / `can.isotp_rx_complete` / `can.isotp_rx_error` |
| `event isotp.state` | `can.isotp_state` |
| (v0.1) `isotp.periodic_start/stop` | 폐기(TesterPresent는 UDS 소유, v0.2 §9-5) |

**A14 — 상태 단계**
- 전송 계층 세부 필드 `tx_state`: 이전 `accepted | confirmed` → **`driver_accepted | bus_confirmed`**. `driver_accepted` = 마지막 프레임까지 드라이버/소켓에 넘김(wire 완료 아님), `bus_confirmed` = backend의 TX 확인(지원 backend만).
- 공통 쓰기 단계(D4/A14)와의 대응. UDS 요청 1건 기준:
  - `requested` = UDS 가드가 판정 시작
  - `accepted` = `can.isotp.send` 결과 `{accepted:true}`(큐 적재)
  - `sent` = `tx_state=driver_accepted`
  - `confirmed` = `tx_state=bus_confirmed`
  - `effective_verified`/`effect_unverified` = UDS가 응답으로 판단(UDS 소유)
  - 어느 단계든 `failed`(`can.isotp_tx_error`) / `outcome_unknown`(전송 timeout·끊김·abort 후 잔여 가능)
- `can.isotp.send` 결과의 `accepted` 필드는 D4 `accepted` 단계를 뜻한다(이름 유지).
- 무응답(P2/P2* 초과)은 UDS가 exit 7 `E_NO_RESPONSE`로 보고(A6). ISO-TP 계층 오류(`N_BS_TIMEOUT` 등)도 exit 7.

**함께 바뀐 CAN 측 사항 (참고)**
- STOP(A3)은 agent·script 요청의 ISO-TP 전송만 abort한다. 사용자(origin=user) 요청의 전송과 FC는 latch하지 않는다.
- UDS가 연 ISO-TP 채널은 잡형 쓰기(A1)다. 채널 open = UDS 정책 위임 감사, PDU = UDS 가드, 프레임 단위 latch·예약 ID·rate는 CAN worker가 집행.
- 예약 tx_id 차단 위반은 exit 4 `E_DENIED`(`detail.reason=tx_reserved`), 같은 주소 채널 중복 open은 exit 6 `E_LOCKED`(이전 `ERR_BUSY`/`AddressInUse`).

(uds-lead 확인란)

**uds-lead 확인 (2026-11-10)** — A16 이름 표, `tx_state: driver_accepted|bus_confirmed`, STOP·잡형·예약 ID(exit 4)·중복 open(exit 6)·무응답 exit 7은 **확인·동의**하며 UDS `plan.md` v1.1(§3.4, §6.1, §15.2, U-13)에 이미 반영했다. 한 가지만 정정 요청:
- **UDS 요청의 D4/A14 단계는 UDS 요청 행 하나가 소유**한다. UDS 정본 매핑(UDS plan §3.4·§6.1): `requested` = 코덱 완료·core 판정 시작, `accepted` = core 판정 통과·UDS 큐 진입, `sent` = DoIP 소켓 쓰기 / ISO-TP `tx_state=driver_accepted`, **`confirmed` = ECU 긍정 응답**(D4 "장치 확인"), `effective_verified`/`effect_unverified` = 되읽기, `failed` = NRC·`can.isotp_tx_error`, `outcome_unknown` = 무응답·끊김·abort 후 잔여.
- 따라서 위 목록의 "`accepted` = `can.isotp.send` `{accepted:true}`"와 "`confirmed` = `tx_state=bus_confirmed`"는 **전송 계층 세부 단계**로만 쓰고(UDS 행의 `tx_state` 필드·CAN 저널), UDS 요청의 `write_state`로는 쓰지 않는다. 버스 송신 확인을 `confirmed`로 표시하면 ECU가 아직 응답하지 않았는데 쓰기가 확인된 것처럼 보여 D4의 "거짓 성공 금지"와 충돌한다. CAN 도메인이 직접 보내는 raw/주기 프레임에는 CAN 매핑을 그대로 쓰면 된다.
- 위 문단 수정에 동의하면 이 절이 양 팀 정본이다. 다른 차이는 없다.

**메인 확정 (2026-11-10)** — uds-lead 확인 문단의 수정안을 채택한다(마스터 A14와 일치). `can.isotp.send`의 `{accepted:true}`와 `tx_state=driver_accepted|bus_confirmed`는 **전송 계층 세부**로만 쓰고, 요청의 `write_state`(requested/accepted/sent/confirmed/…)는 UDS 요청 행이 소유한다. 이 절까지가 CAN·UDS 공동 정본이다.
