docs/plan/02-core-principle.md 준수

# P1 daemon RSS 성장 조사 (조사 전용, 소스 변경 없음)

상태: 조사 완료 / main 확정 원인(ring_order) 반영 / 나머지 누적 필드 정량화 / patch 제안만
대상 binary: `49114f19…00cc` (HEAD e861e5b, diff 없음, **dev 프로파일**)
근거: `/private/tmp/loom-p1-soak-final.json.partial.json` (1441 s), 실행 중 home
`$TMPDIR/loom-browser-smoke-LYCt7q` journal 세그먼트(읽기만), `git show e861e5b:` 정적 분석.
진행 중 소크는 건드리지 않음(`heap`/`vmmap` 미사용, 추가 RPC 없음, `ps` 1회).

## 결론

1. **확정 원인 (main 수정 중)**: e861e5b `hub/mod.rs` `ring_order: VecDeque<ChannelKey>`는 채널별 축출 시 항목을 지우지 않고,
   4 Mi 압축(`retain(!q.is_empty())`)도 hot 채널 키를 전부 남김. 전역 축출은 28 MiB < 32 MiB라 거의 동작 안 함
   → hub 발행 이벤트 수에 비례해 무한 증가.
   정량: 발행 ~391 건/s(뷰어 수신 집계 563 k / 1441 s) × 항목 ~80 B(슬롯 48 B + `domain`/`channel` String 할당 2×16 B)
   ≈ **1.8 MiB/분, 24분에 ~43 MiB**. (4 Mi 압축 첫 발동은 ~3시간 뒤라 소크 중 한 번도 안 돎.)
2. **수정 후에도 남는 성장 (재시험 판정에 영향)**: 아래 두 개는 유한하지만 30분 소크의 측정 창 안에서 계속 늘어남.
   - hub 링 채움(warmup): 저속 채널 core/psu/uds가 4 MiB(JSON 기준) 채우는 데 21–80분.
     측정 창 사이 ΔJSON ≈ **13 MiB**, heap 배율 r = 1.3–3.5 → **+17–45 MiB**.
   - daemon `Notable.items` (cap 65 536, ~22 건/s, 포화 ~50분): 측정 창 사이 **+~5 MiB**.
   → **ring_order 수정 binary로 같은 부하 30분을 돌려도 growth ≈ 20–50 MiB로 ≤10 MiB 게이트 실패 가능성이 높음.**
   이것은 누수가 아니라 상한 도달 전 채움이며, 판정 방식 또는 링 예산 계산을 함께 다뤄야 함(§3, §4).
3. Journal(writer/mapping/index/retention), UDS manager/lanes/module, CORE mirror(`CoreState`), cursor/focus/input 캐시:
   **소크 경로에 무한 누적 없음**(§2 표). journal 증가는 디스크(`_mapping` 132 MB/26분)뿐.
4. 측정 주의: `process_rss_bytes` = `memory_stats.physical_mem` = macOS `resident_size` — **압축된 콜드 페이지 제외**.
   분 15(−20 MiB)·22(−12 MiB) 급락이 그 흔적. 콜드한 `ring_order`/링 항목이 압축되어 RSS가 실제 증가를 덜 보여줄 수 있음.

## 1. 측정 근거와 분해

daemon RSS 분 중앙값(MiB): 0:59 → 2:89 → 5:101 → 10:126 → 20:150 → 24:157 (ps 25:38 = 166).
worker 5개 −0.8 ~ −1.3 MiB 안정. 오류 0, 재시작 0, slow 재연결 0. 판정 창: first t∈[120,420) 중앙 t≈270 s,
last t≥duration−300 중앙 t≈1290 s → 98 → 146 MiB (+48.4).

링 키별 JSON 유입(journal 최근 1분 실측, 링 상한은 `json.len()` 기준 4 MiB/채널):

| 링 키 | 주요 kind (건/분 × 평균 B) | JSON 유입 | 4 MiB 도달 |
|---|---|---|---|
| can/ch0 | can.frame 12560×498, can.tx (gw 1794×453 + sim 1196×566) | ~8 MiB/분 | ~30 s |
| uart/dut | write (1197+1794)×~470, data 1196×457, input_owner 1196×406 | ~2.4 MiB/분 | ~2분 |
| eth/cap | tx 406×~520, pkt 118×1552, stats 60×448 | ~0.41 MiB/분 | ~10분 |
| psu/bench | meas 362×532 | ~0.19 MiB/분 | ~21분 |
| core/– | activity 415×422 (+latch/cursor) | ~0.17 MiB/분 | ~23분 |
| uds/ecu | tx 59×806, rx 59×552, result 59×1087 | ~0.14 MiB/분 | ~28분 |
| uds/– (gateway 생산 uds.tx, channel 없음) | 116×451 | ~0.05 MiB/분 | ~80분 |

판정 창 사이(t 270 → 1290 s) 분해:

| 항목 | 계산 | 추정 |
|---|---|---|
| ring_order (확정) | 391/s × 1020 s × 80 B | ~30 MiB (압축 시 더 작게 보임) |
| 링 채움 | 링 JSON 12.3 → 23.8 MiB, Δ11.5 × r | r=1.3 → 15, r=3.5 → 40 MiB |
| Notable | 22/s × 1020 s × ~170 B | ~3.6 MiB |
| 합 vs 실측 | | 실측 48.4 ⇒ r ≈ 1.3 (압축으로 ring_order가 덜 잡혔다면 r 더 큼) |

정적 추정 r(링 항목 heap/JSON): `Arc<Published>` = JSON `Arc<str>` + `PublishedRecord`(String/Option<String> 20여 필드 중
7–10개 실할당) + `payload: Value`(serde_json 기본 BTreeMap: 객체 1개당 leaf 노드 ~630 B + 키 String;
`bytes:{data,encoding}` 같은 중첩 객체마다 노드 추가). 단순 레코드 ~2×, uds.result ~4× → r ≈ 2–3.5.
실측 역산(~1.3)과의 차이는 `resident_size` 압축 제외로 설명 가능 → **heap 직접 계측 필요**(§5).

## 2. 누적 객체 분리 조사

| 위치 | 필드 | 상한 | 소크 영향 |
|---|---|---|---|
| hub `ring_order` (e861e5b) | 이벤트당 `ChannelKey` | **사실상 없음** | **확정 원인**, main 수정 중 |
| hub `rings` | 채널별 `VecDeque<Arc<Published>>` | JSON 4 MiB/채널, 32 MiB 전역 (**JSON만 계산**) | 채움 중 증가 (유한) |
| hub `Evictions.by_ring` | 키당 1 | 4096 | 무시 |
| hub `CoreState.writes` | request_id | 1024, final/Failed/OutcomeUnknown 시 제거 | ≤ ~1 MiB |
| hub `CoreState.workers/channels` | 워커/채널 | 열린 수 | 무시 |
| hub `Sub.q` | 구독 | 2 MiB/구독, 32 MiB 전체 (Arc 공유) | 수 초 내 포화 후 일정 |
| daemon `Notable.items` | Requested 쓰기/latch/policy/mark | 65 536 | 채움 중 증가 (~0.2 MiB/분, cap ~10 MiB) |
| daemon `cursors` | client | CURSOR_CAP + TTL | 무시 |
| daemon `event_cursors`, `focus_last` | client 키 | **상한 없음** | 소크는 고정 client 3개 → 0. 새 client_id를 매번 쓰는 CLI면 무한(별도) |
| daemon `presence` / `inputs` / `activity` | conn / (conn,key) / 배치 | 해제·flush·take | 무시 |
| fence `Commits.map` | producer | MAX_PRODUCERS | 무시 |
| journal `writers` / `gw_seq` | producer | daemon측 3개 / 2개 (실측 디렉터리) | 무시 |
| loom-journal writer | 큐 8 MiB·32 768건, write 64 KiB, 희소 index 2필드, gap MAX_PENDING_GAPS | 유한 | 무시 |
| loom-journal `MappingLog` | 직렬화 후 writer로, 메모리 보관 없음 | — | RAM 무시 (디스크만) |
| loom-uds manager | targets 16, write log 1024, client cursors 256, timed_out 16 | 유한 | 무시 |
| loom-uds lanes / module | waiters MAX_PENDING_*, snapshot targets 64 | 유한 | 무시 |
| hub `Cmd::Query`, snapshot backlog | 호출별 Arc Vec | 응답 후 해제 | 일시적 |

## 3. 제안 patch (미적용, main 판단)

### P1 링 예산을 실제 보존 비용으로 계산 — hub/mod.rs (main 수정 범위, ring_order 작업과 같이 반영 권장)

`ring_push`/채널 축출/전역 축출의 `json.len()` 대신 항목 비용 함수 사용:

```rust
/// Ring cost of one event: JSON text + parsed record (owned Strings + serde_json::Value tree).
fn ring_cost(p: &Published) -> usize {
    const REC_FACTOR: usize = 2;   // heap 계측(§5)으로 확정
    const FIXED: usize = 256;      // Arc/VecDeque/ring_order 항목
    p.json.len() * (1 + REC_FACTOR) + FIXED
}
```
- 효과: "32 MiB"가 heap 근사 상한이 되고 저속 채널 포화가 ~3배 빨라짐(uds/– ~27분). 대신 링 보존 이벤트 수 ~1/3 —
  UI backlog(DEFAULT_BACKLOG 1000)/`core.events.query` 깊이 요구와 맞는지 main 확인 필요.
- 근본안(P1 이후): 링 항목은 필터용 메타(seq, ring id, kind, origin, request_id, at, prio) + `json`만 보존.
  `matches()`/snapshot이 `p.rec`를 쓰므로 hub 전반 변경.
- `ring_order` 값은 `ChannelKey` 복제 대신 정수 ring id 또는 `Arc<ChannelKey>` — 이벤트당 String 2할당 제거.

### P2 Notable 항목 경량화 — daemon.rs

```rust
struct NotableItem {
    seq: u64,
    origin: Origin,
    client_id: Option<Arc<str>>,
    kind: Arc<str>,
    key: Arc<str>,   // "domain:channel"
}
```
작은 인터닝 맵(상한 4096, 초과 시 새 Arc)으로 항목당 3할당 → 0. cap 시 ~10 → ~3 MiB, 채움 기울기 1/3.
`others_since_last` 의미 변경 없음. 대안: cap 16 384 + 커서가 front보다 오래되면 `truncated` 보고(계약 03 §3.2 확인 필요).

### P3 (소크 무관) `event_cursors`, `focus_last` 상한
`cursors`와 같은 CAP + TTL 적용.

## 4. status metric 제안 (main의 `hub_cache{records, order_entries, json_bytes}`에 추가)

- `hub_cache.per_channel: [{domain, channel, records, json_bytes, full}]` — 채움 완료 판정.
- `hub_cache.cost_bytes` (P1 적용 시).
- `notable: {items, cap}`, `core_writes: {entries, cap}`, `subs.queue_bytes_total`, `cursors: {agent, event, focus}`.
- `process_footprint_bytes`: macOS `TASK_VM_INFO.phys_footprint`(압축 포함), Linux는 기존 RSS. 판정을 대체하지 않고
  RSS 옆에 기록 — 압축이 증가를 가리는지 확인용.

재시험 판정 제안(측정 리셋·워밍업 조작 아님): RSS와 위 지표를 같이 1 s 샘플링. 판정 시 `order_entries == records`
불변식 확인 + growth를 (링 채움, Notable, 기타)로 귀속. "기타" ≤ 10 MiB이고 링/Notable이 상한 이내면 누수 없음으로 해석 가능.
30분 안에 순수 RSS growth ≤ 10 MiB를 원하면 P1(+P2)이 필요 — 현 예산으로는 uds/– 키가 ~80분까지 채움 중.

## 5. 미확인 / 다음 근거

- 링 heap 배율 r은 범위(1.3–3.5)로만 추정. 소크 종료 후 같은 binary에 `heap <pid>` 1회(수 초 정지) 또는
  footprint 지표가 있으면 `serde_json` BTreeMap 노드/String 수로 바로 확정 가능.
- dev 프로파일: 할당 크기는 release와 같으나 기준선(~59 MiB)은 다를 수 있음.

## 메인 정정 / 인수 후 수정

- 1회차 immutable binary의 실제 소스는 **e861e5b + 당시 미커밋**이다. sampler의 diff hash가 빈 값인 것은 ui cwd에서 상대 pathspec으로 root crates/ui를 잘못 조회한 메타데이터 버그이며 clean tree의 근거가 아니다. 별도 `/private/tmp/loom-p1-soak-build.json`의 source_snapshot_sha256 `49617c6e882b6bda50cc1c295ca7dce865e94eae2d19a6da94638896dafc67fa`와 binary SHA256 `49114f193d864ffbc7cf005d5988bec369c3c838b7a742bb644b924b0c1a00cc`가 해당 실행의 식별자다. sampler는 git root를 명시하도록 수정했다. RSS 값 자체와 부하·시간은 영향을 받지 않았다.
- **측정 binary는 VecDeque<ChannelKey> 기반 ring_order**였다. 조사 당시 읽은 main 소스는 이미 BTreeMap 수정 상태이므로, 위 “링과 동일하게 유한” 설명은 1회차 binary에 적용할 수 없다. 이전 VecDeque는 per-channel payload 축출 뒤 오래된 순서 항목을 제거하지 않았으며, 4M 항목 압축도 해당 채널이 비지 않았으면 모든 옛 항목을 남겼다. 이 부분은 실제 무한 누적 원인이며, 4.4배 추정에는 그 누적분도 섞여 있다. payload heap만의 확정 배율로 쓰지 않는다.
- 메인은 순서 인덱스를 보관 중인 stream seq와 1:1인 BTreeMap으로 수정했다. 추가로 JSON 문자열뿐 아니라 PublishedRecord/Value 트리·소유 문자열·metrics/quality·순서 인덱스 비용을 보수적으로 과금해 기존 4/32MiB 링 예산에 적용했다. 실제 allocator RSS의 정확한 값이라고 주장하지 않는다. JSON 전용 예산보다 라이브 보관 깊이가 줄 수 있고, 원본 저널은 유지된다. 축출된 구간은 앞서 통합한 coverage/journal 경로로 정직하게 처리한다.
- Notable은 65,536건 상한을 유지하고 동일 kind/channel/client 문자열을 최대 4,096키 캐시에서 공유하도록 수정했다. 카운트·커서·others_since_last 의미는 유지한다.
- 부하율, 최초120초 제외, 앞뒤5분 중앙값, 30분, 10MiB 및 coverage 기준은 바꾸지 않고 새 binary로 재측정한다. 1회차의 실제 실패는 보존한다.
