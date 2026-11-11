# P0 보고 — `crates/loom-can-sim` (sim 버스 + SimNode + 결함 주입)

작성: can-opus, 2026-11-11. 범위: `crates/loom-can-sim/`만(커밋 안 함).

## 결과
- `cargo fmt --check` ✓ · `cargo clippy --all-targets -- -D warnings` ✓ · `cargo test` ✓ **13개 통과**(통합 12 + doctest 1, doctest는 §2.0 축약판). `unsafe`·Python 없음.
- 모든 시험은 `ManualClock`으로 결정적이다.

## 만든 것
| 파일 | 내용 |
|---|---|
| `src/hub.rs` | `SimHub`: 이름별 버스, 엔드포인트 슬롯, STOP latch, SimNode 실행, deadline 이벤트 힙, 통계 |
| `src/endpoint.rs` | `Endpoint` trait(`offer`, 막지 않음) + `LocalEndpoint`/`LocalReceiver`(고정 용량 큐, 느린 소비자 fault) |
| `src/record.rs` | `Origin`(Agent/User/Device/Script), `SimRecord`(고정 크기, `TimedFrame` 포함), id 타입 |
| `src/config.rs` | `NodeConfig`(코드 구성) + `parse_nodes`(TOML) + 검증 |
| `src/clock.rs` | `Clock` trait, `MonotonicClock`, `ManualClock`(clone이 시간을 공유) |
| `tests/fixtures/bcm.toml` | §2.0 가짜 BCM |

## 설정 파일 crate 선택
**`toml` 1.2.1**(toml-rs, MIT/Apache-2.0, 활발히 유지) + `serde` 1(derive). `loom.toml`과 같은 형식이다. YAML 후보 확인(2026-11-11 `cargo info`): `serde_yaml` 0.9.34는 "+deprecated"(보관됨), `serde_yml`은 DEPRECATED(다른 crate로 넘기는 shim), `serde_yaml_ng`/`serde_norway`는 fork라 장기 유지가 불확실하다 → TOML. `toml`은 `display`(writer) 없이 `parse, serde, std`만 켰다. 중첩 깊이 상한은 기본값(`unbounded` 끔)을 쓴다.

## 공개 API 요약
- `SimHub::new(clock, HubLimits)` — 스레드를 만들지 않음. `Send`(시험으로 확인).
- 엔드포인트: `attach(bus, name, origin, Box<dyn Endpoint>)`, `attach_local(...)` → `(EndpointId, LocalReceiver)`(기본 8,192), `attach_local_with_capacity`, `detach`. origin은 붙일 때 호출자(연결 role)가 정하고 프레임마다 바꿀 수 없다. `EndpointId`에 generation이 있어 detach한 id는 슬롯이 재사용돼도 `UnknownEndpoint`.
- 송신: `send(ep, CanFrame) -> Result<source_seq, SimError>`. 송신자를 포함한 모든 엔드포인트에 같은 레코드(Tx 에코, 같은 seq)가 간다. 같은 이름 = 같은 버스.
- 구동: `poll() -> PollReport { events, more_pending }`, `next_deadline_ns()`(worker가 잠들 시각), `now_ns()`.
- STOP: `latch(bus, &[Origin])` / `release(bus)`. latch된 origin의 송신은 `SimError::Latched`이고 카운트된다. **Device는 latch할 수 없다**(넘겨도 무시) — SimNode는 ECU 역할이므로 계속 동작(plan §10.6). User는 호출자가 넘기지 않으면 통과한다(A3: 정책이 Agent·Script만 넘김).
- 노드: `start_node(NodeConfig)`, `stop_node`, `crash_node`(fault), `remove_node`, `nodes() -> Vec<NodeInfo { state, tx_frames, fault_dropped, deadline_overflow, reply_errors, pending }>`.
- 통계: `bus_stats(name) -> BusStats { last_seq, last_time_ns, frames_total, frames_by_origin, latched, latch_rejected, endpoints: [EndpointStats { delivered, drops: DropStats { total, gaps: [Gap { first_seq, last_seq, count }], gaps_evicted } }] }`. fps는 `frames_total` 차분으로 계산(호출자).

## 의미 규칙
- **레코드:** `source_seq`는 버스마다 1부터 +1, 재사용 없음. `dir`은 origin으로 정한다(Device = Rx, 나머지 = Tx) — 관찰자마다 다르지 않다(UI가 `AGT tx / DEV rx`로 표시). `ingest_mono_ns` = 버스 시각, `device_ts = None`.
- **시간 모델(이산 사건):** 노드 응답·주기 프레임은 `poll`이 늦게 불려도 **자기 deadline 시각**으로 기록되므로 간격이 정확하다(1 s 한 번에 진행해도 50, 150, … 950 ms). 버스 시각은 뒤로 가지 않는다(`max(시각, 마지막 버스 시각)`) → seq 순서 = 시간 순서.
- **SimNode(M1 raw 규칙):** `on id [data/mask] → reply_id (copy | 고정 바이트) after delay`, `periodic id every period (offset) data`, `follow`(지정 바이트가 마지막 수신 프레임의 바이트를 따름 — §2.0의 0x310 byte0 = 마지막 0x300 byte0). 트리거가 FD면 copy 응답도 FD. 노드는 자기가 보낸 프레임에는 반응하지 않고 다른 노드의 프레임에는 반응한다. seed 고정(splitmix64, jitter에만 쓰임).
- **결함(기본 끔):** `drop_ids`(노드가 그 id를 안 보냄), `extra_delay`, `jitter`(seed로 재현), `crash_after`(시각) / `crash_node`(즉시) — 대기 중 프레임 폐기, 상태 Crashed. 엔드포인트 느림: 작은 큐 용량 + `LocalReceiver::set_throttle(Some(n))`(호출당 n개만 읽음).

## C2 상한과 넘칠 때 동작
| 대상 | 상한 | 넘치면 |
|---|---|---|
| 버스 수 | 16 | `TooManyBuses` |
| 버스당 엔드포인트 | 16 (plan §9.1) | `TooManyEndpoints` |
| 엔드포인트 수신 큐 | 8,192 기본(1..=65,536) | **그 엔드포인트만** 레코드를 못 받음 + 연속 drop을 gap 하나로(첫/마지막 seq·개수). 송신·다른 엔드포인트는 영향 없음 |
| 엔드포인트당 보관 gap | 32 | 오래된 gap 버림 + `gaps_evicted` |
| 노드 슬롯 | 16 | `TooManyNodes`(정지·크래시 슬롯은 재사용) |
| 노드당 대기 응답 | 1,024 | 새 응답 예약 안 함 + `deadline_overflow`(주기 프레임은 자기 슬롯 1개를 항상 유지) |
| poll 한 번의 이벤트 | 65,536 | 나머지는 다음 poll(`more_pending`) — 0 ms 핑퐁 규칙으로도 끝없는 루프 없음 |
| 설정 파일 | 64 KiB, 노드 16, 규칙 64, 주기 64, follow 8, drop_ids 64, 이름 64 B, 주기 ≥ 0.1 ms, 지연 ≤ 1 h | `ConfigError { path: "node[0].on[1].mask", message }` |

## 시험 목록 (전부 통과)
1. **§2.0 축소판:** Agent `300#01` → BCM 5 ms 뒤 `301#01` → User `300#00` → `301#00`. A·B가 **같은 레코드 4개**(seq 1..4, Agent Tx / Device Rx / User Tx / Device Rx, 시각 0/5/5/10 ms)를 받음.
2. 주기 간격(1 s 한 번 진행 → 10개, 정확히 100 ms 간격)과 follow(byte0 = 0x07).
3. seq 단조·무중복(200 송신 + 응답, 1..n 연속, 시간 비감소), 같은 이름 두 채널 공유, 다른 버스는 독립 seq.
4. latch: Agent·Script 거절(카운트 1/1), User 통과, SimNode 응답·주기 계속, Device latch 무시, release 전 유지 → release 후 재개.
5. 느린 엔드포인트(용량 4)만 drop: gap `(5..10, 6)`, `(15..16, 2)` 정확. 빠른 엔드포인트 1..16 무손실. throttle fault.
6. 결함: drop_ids(0x310 2개 drop), extra_delay(5 + 3 = 8 ms), crash_after 250 ms(이후 침묵, pending 0), stop/crash/remove API.
7. jitter seed 재현(같은 seed = 같은 지연, 다른 seed = 다름, 5..=7 ms 범위).
8. 상한: 엔드포인트 16, 버스 16, 노드 16, 대기 응답 2개 → overflow 3, poll당 이벤트 3 → `more_pending`, detach 후 stale id.
9. 규칙 매칭: data/mask, 짧은 프레임 불일치, 고정 응답, 29비트 + FD copy.
10. 설정 오류 경로(잘못된 hex, mask 길이, 너무 짧은 주기, follow 범위, Classic 길이, 모르는 키, 29비트 플래그, 64 KiB, 노드 17개)와 FD·29비트 정상 설정.
11. `SimHub: Send`, `LocalReceiver: Send + Sync`.
12. **처리량 스모크:** 버스 시간 1 s 동안 Agent 10k fps + 응답 10k fps(+ 주기 10) = 20,010 레코드, 관찰자 2개, 1,000 송신마다 drain → 무손실, seq 연속, 큐 ≤ 8,192. [실측, macOS arm64] **release 5.1 ms, debug 42 ms**.

## 계획과 다르게 한 점 / 리드 확인 요청
1. **`Origin` 열거형을 이 crate에 두었다.** 공통 타입(D3)이므로 나중에 `loom-proto`(CORE) 또는 `loom-can-frame`으로 옮기는 것이 맞다. 요청: 리드가 위치를 정해 주면 바꾼다(지금은 같은 이름·값 4종).
2. **버스 단 `drop_ids` fault는 만들지 않았다.** "특정 ID drop"은 노드 설정(`faults.drop_ids`, ECU가 그 프레임을 안 보냄)으로 구현했다. 에이전트가 보낸 프레임을 버스가 몰래 버리면 "보낸 것이 기록·화면에 그대로 간다"는 원칙과 충돌하므로 뺐다. 필요하면 Device origin에만 적용하는 버스 fault로 추가할 수 있다.
3. `dir`은 수신자별이 아니라 origin으로 정한다(Device = Rx, 나머지 = Tx). 송신자 자신도 Tx 에코로 같은 레코드를 받는다. 브리프의 "Agent tx, Device rx" 표기와 같다.
4. `ingest_mono_ns`는 노드 프레임의 경우 deadline 시각(이산 사건)이다. 실시계 worker에서 `poll`이 늦으면 기록 시각이 실제 처리 시각보다 이를 수 있다(sim 전용이며 seq·시간 순서는 유지). worker는 `next_deadline_ns()`에 맞춰 깨우면 된다.
5. 의존성: `serde 1`(derive), `toml 1`(parse·serde·std). 그 외 없음.

## 남은 것 (이번 범위 밖)
로컬 소켓 엔드포인트(CORE IPC 후 `Endpoint` 구현), 저널 연결, gateway·CLI(`loom can node/demo`), DBC 신호 노드(M2, `loom-can-dbc` 사용), ISO-TP 서버 노드(M2, `loom-can-isotp`). `loom-can-frame` 변경 요청은 없다(위 1번 `Origin` 위치만).
