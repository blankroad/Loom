# CAN P0 — `crates/loom-can-sim` (sim 버스 + SimNode + 결함 주입)

먼저 `docs/impl/can/tasks/p0-common.md`를 읽고 따른다. 범위: **`crates/loom-can-sim/` 폴더만.** 보고서: `docs/impl/can/reports/p0-sim.md`.

## 목표
M1 걷는 뼈대의 장치 쪽(plan §9.1, D72). `can-sim` worker 안에서 돌 **sim 버스 owner**와 **가짜 ECU(SimNode)**. CORE의 worker 프로토콜(`loom-proto`, 태그 `core-p0-d2`)은 아직 없으므로 IPC는 만들지 않고, 나중에 worker가 감쌀 수 있는 라이브러리 API로 만든다. 핵심 원칙: 에이전트가 보낸 프레임에 가짜 ECU가 응답하고, 그 모든 프레임이 같은 순서·같은 출처 정보로 관찰자(나중에 저널·gateway·UI)에게 간다.

## 요구
1. 의존: `loom-can-frame = { path = "../loom-can-frame" }`. 노드 설정 파일 파서 crate는 유지보수 상태를 확인해 고르고 이유를 보고서에(YAML이면 `serde_yaml`(보관됨) 대신 유지되는 대안, 마땅치 않으면 TOML — `loom.toml`과 같은 형식).
2. **SimBus**(채널 이름별): 엔드포인트 attach/detach(상한 16, plan §9.1), 각 엔드포인트의 수신 큐는 **고정 상한**(기본 8,192 프레임) — 넘치면 그 엔드포인트만 drop + 범위 카운트(다른 엔드포인트·버스는 막지 않음). 버스 owner가 프레임을 정규화하고 **`source_seq`(u64, 채널별 단조 증가)** 와 `ingest_mono_ns`, 출처(`Origin` 열거형: Agent/User/Device/Script — 호출자가 연결 role로 정해 넘김, SimNode는 항상 Device)를 붙여 모든 엔드포인트에 같은 순서로 전달. 송신자 자신에게도 Tx 에코(`Direction::Tx`)로 전달(같은 seq). 같은 이름 두 채널 = 같은 버스.
3. **시계 주입**: 시각은 `Clock` trait로 받는다(실시계 + 시험용 수동 시계). 시험은 수동 시계로 결정적으로.
4. **TX latch(STOP, A3)**: 버스 단에 `latch(origins)`/`release()` — latch 중에는 지정 origin(Agent·Script)의 새 송신을 `Latched` 오류로 거절하고 카운트, User 송신은 통과, **SimNode(Device)는 계속 동작**(plan §10.6 ⑤). 해제는 명시 호출만.
5. **SimNode(raw 규칙, M1)**: 설정 예
   ```
   node BCM, channel sim1
   on 0x300 (data mask 선택) → reply 0x301 with data 복사 또는 고정 바이트, delay 5 ms
   periodic 0x310 every 100 ms data 01 (byte0는 마지막 0x300 byte0를 따르게 하는 옵션)
   ```
   응답·주기 송신은 deadline 큐(상한)로 처리하고, 시계가 진행될 때 `poll(now)`에서 낸다. 고정 seed. 노드 시작/정지/목록 API.
6. **결함 주입(sim 전용)**: 노드·버스 설정으로 특정 ID drop, 응답 지연 추가, 노드 크래시 흉내(정지), 엔드포인트 느림(큐 넘침 유발). 기본 꺼짐.
7. **통계·카운터**: 채널별 fps 계산용 누계, 엔드포인트별 drop 범위(첫/마지막 source_seq·개수 — 나중에 `display.gap`/`record.gap`의 근거), latch 거절 수.
8. **스레드 모델**: 라이브러리는 스레드를 만들지 않는다(호출자가 `poll` 구동). `Send` 가능해야 함. 나중 프로세스 간 엔드포인트(로컬 소켓)는 CORE IPC가 나오면 붙이므로, 엔드포인트를 trait 뒤에 둔다.
9. **C2**: 모든 큐·맵 상한(엔드포인트 16, 수신 큐 8,192, 노드 deadline 큐, 규칙 수). 넘침 정책을 코드·문서에.

## 시험 (필수, 수동 시계로 결정적)
- §2.0 축소판: 엔드포인트 A(Agent)가 `0x300#01` 송신 → BCM이 5 ms 뒤 `0x301#01` → 엔드포인트 A·B(관찰자) 모두 같은 순서·같은 seq·출처(Agent tx, Device rx)로 수신. 이어서 B(User)가 `0x300#00` → `0x301#00`.
- 주기 송신 간격, seq 단조·무중복, 같은 이름 두 채널 공유.
- latch: Agent 거절·User 통과·SimNode 계속, release 후 재개.
- 느린 엔드포인트만 drop(범위 정확), 다른 엔드포인트 무손실.
- 결함: drop ID, 지연, 노드 정지.
- 처리량 스모크(릴리스 빌드에서 20k fps 상당을 수동 시계로 1초분 주입, 무손실·상한 유지) — 10초 이내.

## 하지 않는 것
IPC·소켓, 저널 쓰기, gateway·CLI, DBC 신호 기반 노드(M2), ISO-TP 서버 노드(M2, `loom-can-isotp` 사용 예정).
