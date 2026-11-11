# can-sol P0 — `crates/loom-can-isotp` (ISO-TP 상태기계)

먼저 `docs/impl/can/tasks/p0-common.md`를 읽고 따른다. 범위: **`crates/loom-can-isotp/` 폴더만.** 보고서: `docs/impl/can/reports/p0-isotp.md`.

## 목표
CAN 소유 ISO-TP(ISO 15765-2) 엔진(plan D65)의 P0판. **sans-IO 상태기계**: 스레드·시계·소켓을 갖지 않는다. 호출자(나중에 worker의 TX 중재 전용 스레드)가 수신 프레임과 현재 시각 `now_mono_ns`를 넣고, 엔진은 보낼 프레임·이벤트·다음 깨울 시각을 돌려준다. 계약은 `docs/plan/_xteam/can-uds-isotp.md`(맨 아래 "v1.1 이름·상태 정본" 절이 우선)와 plan §3.6.

## 요구
1. 의존: `loom-can-frame = { path = "../loom-can-frame" }`. 프레임은 `CanFrame`으로 주고받는다.
2. **채널 설정** `IsoTpConfig`: tx_id/rx_id(`CanId`, 11/29bit), normal addressing(P0). extended/mixed addressing은 M4지만 주소 바이트 자리를 설정 구조에 남겨 둔다(지금은 `Normal`만 구현, 나머지는 `Unsupported` 오류). CC(TX_DL 8) / FD(TX_DL 8..64 중 유효 길이), `padding: Option<u8>`(기본 `Some(0xCC)`; FD에서 유효 길이로 올려야 할 때는 None이어도 0xCC로 채우고 그 사실을 문서화), `block_size`(기본 8), `st_min`(기본 0), `n_bs_ms`/`n_cr_ms`(기본 1000), `wft_max`(기본 상한, 예 10), `max_pdu`(4095 고정 상한, 더 큰 FF_DL은 거절).
3. **프레임 codec**: SF(CC 1..7, FD는 escape 형식으로 8..62), FF(12bit 길이, 4095 이하만), CF(SN 0..15 wrap), FC(CTS/WAIT/OVFLW, BS, STmin). STmin 인코딩: 0x00–0x7F ms, 0xF1–0xF9 = 100–900 µs, 예약 값은 127 ms로 처리(표준 규칙). 잘못된 PCI·길이는 오류로 분류(패닉 금지).
4. **송신기**: `send(pdu, now)` → 진행 중 송신은 엔드포인트당 1개(두 번째는 `Busy` 오류). FF 후 FC 대기(N_Bs), CTS면 BS 단위 CF 송출, STmin을 지키도록 각 CF의 "보낼 수 있는 가장 이른 시각"을 돌려준다. WAIT는 `wft_max` 초과 시 오류, OVFLW → 오류. 블록 끝마다 다시 FC 대기.
5. **수신기**: SF → 즉시 완료. FF → **`rx_start` 이벤트를 즉시**(UDS가 P2 판정 전에 FF를 봐야 함 — R-TP-14/15) + FC(CTS, 설정 BS/STmin) 송출 요청 → CF 수신, SN 검사, N_Cr 타임아웃, 완료 시 `rx_complete{pdu}`. 수신 중 새 SF/FF가 오면 표준대로 현재 수신을 버리고(오류 이벤트) 새 것을 시작. 진행 중 송신 중에 받은 FC만 송신기로, 상관없는 FC는 무시(카운트).
6. **완료 상태(A14)**: 호출자가 각 송신 프레임에 대해 `on_frame_submitted(handle)`(드라이버 접수)와 선택적으로 `on_frame_confirmed(handle)`(버스 TX 확인)을 알려 준다. 전송 완료 이벤트 `tx_done{ tx_state }`의 `tx_state`는 모든 프레임이 접수되면 `DriverAccepted`, 백엔드가 모든 프레임 확인을 줬으면 `BusConfirmed`. 확인을 못 받는 백엔드에서 `BusConfirmed`로 올리지 않는다.
7. **abort**: `abort(now)` → 남은 CF·FC 제거, 상태 초기화, 결과에 "이미 접수돼 취소할 수 없는 프레임 수(`residual_tx_possible`)" 보고. abort 뒤 늦게 온 FC/CF는 이전 전송에 섞이지 않게(세대 번호) 무시·카운트.
8. **다음 깨울 시각**: `poll_at() -> Option<u64>`(가장 이른 deadline: STmin 다음 CF, N_Bs, N_Cr). 호출자가 그 시각에 `on_timer(now)`를 부르면 타임아웃·다음 CF를 처리.
9. **결함 주입 훅(sim 전용, R-TP-13 최소판)**: 설정으로 켜는 `FaultPlan`: CF n번째 누락, 잘못된 SN, FC 지연(ms), FC WAIT k회 반복, FC OVFLW 응답. 기본 꺼짐. 실HW에서 켤 수 없게 하는 것은 상위(worker) 책임이라 문서에만 적는다.
10. **C2**: PDU 버퍼는 `max_pdu`로 미리 상한, 출력 큐(보낼 프레임·이벤트)도 상한 있는 구조로(예: 고정 용량 + 넘치면 오류). 할당이 진행 중 무한히 늘지 않게.

## 시험 (필수)
- codec: 각 PCI 종류 인코드/디코드 표 시험, STmin 값 전체 표, FD SF escape, FF 길이 경계(7/8, 62/63(FD), 4095/4096).
- 송신기↔수신기 왕복: 두 엔진을 가짜 링크로 연결하고 시각을 직접 진행시켜 PDU 1..4095 바이트, CC/FD, BS 0/1/8, STmin 0/1 ms/0xF1(100 µs)에서 내용 일치 + **CF 간격 ≥ STmin**(가상 시각으로 확인).
- 오류: N_Bs·N_Cr 타임아웃, 잘못된 SN, OVFLW, WAIT 초과, 수신 중 새 FF, abort 후 늦은 프레임, 4096 거절, Busy.
- 결함 훅이 기대한 오류를 만들어 내는지.
- 가능하면 `proptest`(dev-dependency)로 길이·BS·STmin 무작위 왕복.
- (선택, 시간이 되면) 공개 crate `can-iso-tp` 0.2 또는 `automotive` 0.6과 같은 입력에 대한 프레임 바이트 차등 비교(dev-dependency, oracle). 어렵거나 API가 안 맞으면 보고서에 이유만.

## 하지 않는 것
스레드·OS 시계·실제 드라이버·IPC·`can.isotp.*` RPC(나중에 worker·gateway가 이 엔진을 감쌈). functional/group addressing(M4).
