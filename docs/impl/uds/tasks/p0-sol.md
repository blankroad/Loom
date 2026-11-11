docs/plan/02-core-principle.md 준수

# UDS P0 — uds-sol (DoIP 전송 → 트랜잭션 러너)

먼저 `docs/impl/uds/tasks/p0-common.md`를 읽는다. **대화형 질문 창을 띄우지 않는다** — 스스로 정해 보고서에 적거나, 보고서에 질문을 쓰고 끝낸다(공통 규칙 "질문 금지"). 순서대로 2개 crate. 근거 조사는 네가 쓴 `docs/plan/uds/v2-sol.md`와 `spikes/uds/rust/`(재사용 가능, 단 제품 규칙 — unwrap 금지·C2 상한 — 을 지킬 것). DoIP는 C4.1에 따라 **OS TCP/IP 소켓 위**(PC NIC + 미디어 컨버터)다.

## 1. `crates/loom-uds-doip` (plan U-04 + sim 엔티티 측)
의존: tokio, thiserror, tracing. UDS 바이트는 불투명 payload(codec 의존 없음).
- **프레이밍**: 헤더 8바이트(버전 0x02/0x03 + 반전 검사, payload type, length). **할당 전 길이 검사** — 상한 `MAX_DOIP_PAYLOAD`(진단 메시지 기준, 근거와 함께 상수), 초과는 NACK 0x02/연결 종료 규칙대로. payload type: 0x0000 NACK, 0x0001–0x0004 차량 식별, 0x0005/0x0006 라우팅 활성화(활성화 타입 0x00·0x01·**0xE0** 허용, 응답 코드 전부 — 0x02 등 crate들의 틀린 값 주의), 0x0007/0x0008 alive check, 0x4001–0x4004 상태, 0x8001 진단, 0x8002 ACK, 0x8003 NACK.
- **tokio 클라이언트**: TCP 연결(timeout), 라우팅 활성화, 수신 task 1개(alive check 요청에 즉시 응답 — 진단 대기 중에도), 진단 송신 → ACK/NACK 대기(T_ACK), 응답을 소스 논리 주소별 **상한 있는** 채널로 분배, 호출별 deadline, 링크 이벤트(closed/error/reactivated) 상한 채널. 소켓 하나를 여러 대상 LA가 공유.
- **UDP 발견**: 차량 식별 요청(브로드캐스트/유니캐스트) + 응답·announce 수집(개수·시간 상한).
- **엔티티(서버) 측** `DoipEntity`: 시험과 `connect --sim`용. TCP accept, 활성화, alive check, 진단 → ACK 후 핸들러 호출. 핸들러 인터페이스는 `loom-uds-sim`에 맞춘다: `fn handle(&mut self, target_la: u16, now_mono: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)>`(지연, 응답 바이트) — trait로 두고, sim crate를 직접 의존하지 않는다(통합은 리드/`loom-uds`가 붙임). UDP 식별 응답. 결함 주입: NACK, ACK 누락, 소켓 닫기, 활성화 거부 코드.
- **관문 시험**(DoIP M1): 루프백 왕복, 활성화 코드별, 응답 대기 중 alive check, 길이 u32::MAX 헤더 무할당 거부, 0xE0 수용, 발견, 소켓 끊김 → 링크 이벤트, 무작위 바이트 10만 회 panic 없음. 루프백 지연 p50/p99를 시험 출력이나 `examples/`로 측정해 보고서에(1분 이내).

## 2. `crates/loom-uds-runner` (plan U-06 러너 — D2 관문)
의존: tokio, `loom-uds-codec`(응답 분류 `classify_response`만 — 아직 없으면 임시 정의 후 교체 목록에), `loom-proto`(`WriteState`).
- **`Transport` trait**: 대상 하나에 바이트 송신 + 응답 스트림 수신 + 링크 generation/epoch. DoIP 어댑터(`loom-uds-doip` 위)와 가짜 transport 둘 다 구현. dyn 호환 방식은 네가 정한다(외부 `async-trait` 없이 — `loom_domain_api::BoxFuture`가 견본).
- **러너**: 대상별 직렬 실행(대기열 상한 `MAX_PENDING_PER_TARGET`, 넘치면 `E_BUSY`), 첫 응답 P2 타이머, 0x78이면 P2*로 전환·0x78 횟수 상한, 총 deadline, 요청 중 generation 변경 → `outcome_unknown`, SPRMIB → P2 동안 NRC만 기다리고 없으면 `ok_suppressed`.
- **격리(quarantine)**: timeout 뒤 같은 대상에 늦게 온 응답은 `late:true`로 표시만 하고 다음 요청의 응답으로 쓰지 않는다. 응답 식별자(SID·서브기능·DID 에코) 기반으로 다음 요청이 구분 불가하면 격리 기간 동안 `E_QUARANTINE`, 구분 가능하면 진행. 고유 생존 프로브(격리 해제용 0x3E 등) 포함 — plan §3.4의 반례 시험을 넣는다.
- **`write_state` 매핑**(계약 02 + 리드 결정): 소켓에 넘김 = `sent`, ECU 긍정 응답 = `confirmed`, NRC = `failed` + nrc, 읽기 timeout = `failed` + `timeout:true`, 쓰기 timeout·링크 끊김 = `outcome_unknown`(재전송 없음). `effective_verified`(되읽기)는 도메인 몫이라 러너는 하지 않는다.
- 결과 타입에 시각: 송신·첫 응답·최종 응답 mono ns(UI가 `core.time` 기준으로 비교 — UI 요청 5).
- **D2 관문 시험 목록**: plan §12 U-06의 시험(P2/0x78/완료 상한/총 deadline/epoch/격리/반례/에코/SPRMIB)을 가짜 transport + tokio `test-util` 정지 시계로, 그리고 `loom-uds-doip` 엔티티 루프백으로 1개 이상 종단 시험.

보고서 2개(crate마다). 끝나면 멈춘다.
