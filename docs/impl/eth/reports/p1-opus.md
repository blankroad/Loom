# P1 보고 — explore-opus: eth gateway · CLI · skill (2026-11-11)

**리드 메모**: ① `cargo clippy --workspace -D warnings`가 지금 빨갛다 — 원인은 explore-sol 작업 중 파일(`worker/shared.rs` 미사용 import, `worker/packet.rs` 미사용 상수)뿐, 내 파일은 깨끗하다. ② model.rs 변경 요청 2건(아래 "model.rs").

**완료.** worktree `.worktrees/eth`, 커밋 안 함. `cargo fmt --all --check` OK · `cargo clippy -p loom-eth`(내 파일) 경고 0 · `cargo test --workspace` 전부 green(loom-eth: gateway 10 + cli 6).

## 만든 것
- `src/gateway.rs` + `gateway/{common,handlers,hooks,evidence}.rs`: `Eth: DomainModule`. 명령 12개(schema v1 이름 그대로), 이벤트 kind 6개(포인터는 전부 `/payload/…`), 위험표, hooks, PerChannel(4) 워커, reducer, 증거 검증기 4개, display summarizer, 패널 3개(`eth.packets`·`eth.stream`·`eth.stats`), help.
- 워커로 보냄: `ifaces`·`capture.start/stop`·채널 지정 `capture.status`·실제 `send`. gateway 처리: `query`·`show`·`stats`·`follow`·`wait_for`·`export`·`filter.check`·send dry-run — 저널의 같은 `eth.pkt`를 decode·filter crate로 읽는다(에이전트·UI·TEST 한 결과).
- `src/cli.rs`: `loom eth ifaces|capture start/stop/status|trace|last|show|wait-for|stats|follow|send|export|filter check`. tshark 열(`SEQ TIME SOURCE DESTINATION PROTO LEN INFO ORG`) + 꼬리 `(N rows · source.gap N · display a/b · older: --until … · next: --since …)`. 장치 crate 안 씀.
- `skills/loom-eth/SKILL.md`(49줄): tshark 대응표, 필터 부분집합·`!=` 의미·비지원 목록, BPF vs `-Y`, 상한·커서, sleep 대신 wait-for, REF 복사, 자기 송신 비증거, TC 단계 절.
- 시험: RegistryProbe, HookHarness(분류표 11건 + 시간 < 10 ms, presend), 가짜 저널·워커·CtxServices(`tests/support`, testkit에 없음)로 query/show/stats/wait_for/send, 증거(전수·gap·자기 송신 제외·예산), reducer, CLI 파싱·사용 오류·렌더 golden.

## 결정 (계약·schema v1 기준으로 스스로 정함)
1. 행 상한: Registry는 `max_rows ≤ 1000`(D5)만 받는다 → 1000 등록, 요청 값도 1000으로 자른다(model `QUERY_MAX_ROWS=2000`과 충돌).
2. `eth.query` = "to까지 최근 max행"(UI 답변 3). 잘린 경우 뒤로 가는 `prev_cursor`(`--until`용)를 추가 필드로 준다. `from` 없으면 최근 600 s만 본다(역방향 스캔 없음 — CORE 요청 4).
3. dry-run은 `eth.send{dry_run:true}` 그대로(UI 답변 6). handler는 워커·행 없이 `{proto, info, len, risk, reason, dry_run}`. 단 감사 생략은 CORE 파이프라인 몫(요청 1).
4. `wait_for`: 50 ms 폴링, runtime 없는 `Delay`(짧은 std 스레드 + waker). anchor 없으면 명령 시작 전 wall 시각 패킷은 세지 않는다. 손실 있으면 `E_PARTIAL`(8), 개수 범위 밖 `E_COUNT_OUT_OF_RANGE`, 시간 초과 `E_TIMEOUT`(3). 상세에 같은 결과 JSON.
5. 위험(F1, 막지 않음): capture R0 · sim/replay/loopback R1 · 실 NIC R2 · 브로드캐스트/멀티캐스트(IP·MAC) R3 · raw DoIP 0x8001 = 안의 UDS SID 등급(UDS 계획 §10.3 하한, DB 없음)으로 상향만. hooks는 동기·I/O 없음, 송신 크기 ≤ 9216 B.
6. reducer가 `capture_state`로 채우는 채널 장부를 hooks가 같이 읽는다(sim 판정·presend 세대 비교). presend: 미개방/정지 채널 `Locked`, 세대 불일치 `EpochChanged`.
7. 증거: `eth.match` Positive · `eth.count/absent` Complete · `eth.period` Measure. 전수 평가, `attributed_origin` 제외, `source.gap`·`record.gap`·`eth.batch(total>shown)`·coverage 불완전이면 부재/개수/주기 Inconclusive(단 max 초과와 양성 일치는 확정). `EvalBudget`을 256개 단위로 차감.
8. `export`는 job API가 없어 동기·상한(10만 패킷/256 MiB/600 s)으로 `{path, packets, bytes}`. 경로 없으면 임시 폴더.
9. 워커 예산은 계획에 수치가 없어 256/512 MiB, credit 8 MiB로 정했다.

## CORE 요청 (`docs/impl/requests/eth-core.md`에 올려 주세요)
1. write class 명령의 `dry_run:true`는 정책·감사·행 없이 handler로 바로 보내기(또는 CommandSpec에 `preview_param`).
2. `CallCtx::worker(kind)`에 채널 지정이 없다 — 지금은 args의 `ch`로 워커가 고른다. `worker_for(kind, ch)` 필요.
3. P1-4 bounded wait/subscribe API(폴링 스레드 제거).
4. 저널 꼬리 스캔(`Cursor::Last` + max_events=N이 "최근 N개") — 없으면 lookback 600 s.
5. `source.gap`/`record.gap`에 원 채널을 넣거나 `SubFilter.channels`가 해당 채널의 core gap을 포함해야 한다. 안 그러면 채널 필터 스캔이 손실을 못 봐 부재를 잘못 확정할 수 있다.
6. 증거 검증 `events`에 window의 gap·`eth.batch`가 같이 오는지 계약에 명시.
7. export job API(`core.job_progress`) + 출력 폴더 정책(지금은 에이전트가 준 경로에 데몬이 쓴다).
8. 미발행 패킷(display 샘플링) 조회: P1-3 원본 기록(pcap-epb/1) 읽기 API — 없으면 그 구간 개수/부재는 항상 Inconclusive.

## model.rs (리드 소유, 고치지 않음)
- `QUERY_MAX_ROWS` 2000 → 1000(D5와 맞춤). `QueryParams`에 `wait_ms`(trace --follow) 없음.

## UDS 요청 문구 (`docs/impl/requests/uds-eth.md` 후보)
"UDS가 DoIP로 보낸 요청마다 `wire_key = {src_ip, dst_ip, tcp_stream_or_ports, doip_sa, doip_ta, uds_bytes_hash}`를 `uds.tx` 행 payload에 넣어 주세요. eth gateway가 같은 키의 `eth.pkt`에 `eth.link{eth_ref, link{domain:"uds", request_id, role:"request|response", confidence}}`를 냅니다. 그때까지 eth.link는 만들지 않습니다."

## UI 영향
- `eth.query` 결과에 `prev_cursor`·`matched`·`complete`·`gaps_omitted` 추가. gap 항목 `{kind: source|record|scan|unpublished, cause, count, after_seq}`.
- `eth.show`에 `summary`·`warnings` 추가. dry-run 결과에 `reason`·`dry_run` 추가.
- reducer snapshot = 채널별 `{kind:"eth.capture_state", ch, state, backend, iface, generation}` + `{kind:"eth.last_packets", rows[≤200]}`.
- 패널 manifest `eth.packets`(topics pkts/state/gaps, 동작 capture.start/stop(reduce)/send(card)/export), `eth.stream`·`eth.stats`는 M2 자리.

## 질문
- 없음(위 결정으로 진행). p1-common.md는 처음 읽을 때 없었고, 추가된 절을 반영했다.
