# P1 메모리 보관 수정 — 독립 코드 리뷰

대상: `37feca4 fix(memory): bound ring ownership and account retained event heap` (작업 지시 기준 13948ae+미커밋 diff와 동일 내용, 리뷰 중 커밋됨). 소스/Cargo/lock/index 변경 없음. 실행 중인 2회차 소크(loomd pid 9794)에는 RPC·heap 도구를 쓰지 않았고 `/private/tmp/loom-p1-soak-attempt2.json.partial.json`만 읽었다.

## 결론

- **blocker 0 / major 0.** 1회차 실패 원인(VecDeque `ring_order` 무한 누적)은 해소됐다. 순서 인덱스는 보관 레코드와 1:1이고, 전역 축출은 실제 최고령 seq부터 진행하며, 바이트 회계는 대칭이다.
- minor 5건, 소크 도구 nit 2건, 2회차 판정 해석 주의 1건.
- 보고서는 논리 charge를 RSS라고 주장하지 않는다(정확). 코드 주석의 "conservative"는 작은 객체에서 엄밀히 성립하지 않는다(m3).

## 검증하고 문제 없던 항목

| 항목 | 근거 |
|---|---|
| 회계 대칭·오버플로 | `cache_cost`는 publish 때 한 번 고정되고 push/채널 축출/전역 축출에서 같은 값을 더하고 뺀다(hub/mod.rs:1083-1119). 항목당 ≤ `RING_BYTES+1`, `ring_bytes` ≤ RING_BYTES 상태에서 더하므로 64비트 overflow·underflow 없음. `json_bytes`도 같은 방식 |
| 1:1 인덱스 | 채널 축출은 `ring_order.remove(seq)`(1099), 전역은 `pop_first`(1110). 링은 제거되지 않고 seq는 단일 Sequencer에서 단조 증가하며 리셋 경로 없음(1134-1135). 따라서 채널 front == 전역 최소 seq, `debug_assert` 성립 |
| 동시 publish | 모든 publish는 Sequencer actor에서 직렬화. `only`(per-conn gap) publish는 링에 넣지 않음(1146). Status의 `records/order_entries`는 같은 Cmd 안에서 계산되어 원자적 |
| Range/Query/coverage | 읽기 경로 변경 없음. 전역 축출도 `evicted.note`로 채널 watermark 갱신 → daemon:2593/2722의 `evicted_through` 경로 유지 |
| Notable 의미 | 상한 65,536, `seq`/`hw`/커서 동일. `client_key(it.client_id.as_deref())` 비교와 `kinds`/`by_channel` 키 문자열 동일 → `others_since_last` count/origins 의미 보존 |
| self RSS | `memory_stats` `physical_mem` = 현재 resident(peak 아님). 워커도 같은 `loomd worker` binary라 binary hash가 워커까지 덮음 |
| 2회차 실측 불변식 | t=0~180 s 동안 `order_entries == records` 유지(최종 7,141/7,141), 오류 0 |

## minor

### m1. 깊이>64 sentinel이 채널 링 전체를 비움 — hub/mod.rs:140-141, 1094-1104

`value_heap`은 depth>64에서 `RING_BYTES+1`을 반환한다. 이 레코드는 채널 상한을 넘으므로 per-channel 루프가 **그 채널의 이전 이벤트 전부와 새 이벤트**를 축출한다. 클라이언트가 이 경로를 실제로 탈 수 있다. `core.mark`의 `note`는 임의 JSON이며(daemon.rs:1439-1450) 채널 키 `("core", None)`에 publish된다.

- 재현(테스트 스케치): core 채널에 이벤트 10개 publish → `payload: {"note": [[[…65단…]]]}` 1건 publish → `range(channels=core)` 결과 0건, `evicted_through` = 마지막 seq.
- 깊지 않아도 숫자 배열은 원소당 32 B로 과금된다. JSON 약 280 KB(`[0,0,…]` 약 14만 원소)짜리 mark 1건으로 같은 결과가 난다. 종전에는 JSON이 4 MiB를 넘어야 했다.
- coverage는 정직하게 보고되고 journal 원본도 유지되므로 데이터 손실은 아니다. 다만 core 채널(latch/activity/worker_state) 라이브 backlog를 한 번의 작은 요청으로 비울 수 있다.
- 제안: sentinel을 유한 상한으로 바꾸기(예: 남은 subtree 과금을 `json_bytes.saturating_mul(48)` 이하로 상한, 또는 반복 순회). 단일 레코드가 채널 상한을 넘을 때 기존 링을 비우지 않고 그 레코드만 보관 제외하는 방식은 `evicted_through` watermark 의미와 충돌하므로 별도 설계가 필요하다(이번 범위 밖).

### m2. 전역 축출 경로에 테스트가 없음 — tests/hub.rs:198-238, hub/mod.rs:1108-1123

새 회귀 테스트의 총 charge는 약 4 MiB로 `RING_BYTES`(32 MiB)에 못 미쳐 `pop_first`·`debug_assert`·교차 채널 최고령 축출을 한 번도 실행하지 않는다. `ring_events < 130`은 JSON 전용 예산(약 127건)에서도 통과하므로 charge 도입을 구분하지 못한다. 실제 구분력은 `order_entries == records`와 `cost > json` 단언에만 있다.
- 제안: 채널 9개 이상에 각각 >4 MiB charge를 보내고 다음을 단언한다. ① 가장 먼저 쓴 채널의 front부터 사라짐 ② `ring_cost_bytes ≤ RING_BYTES` ③ `order_entries == records` ④ 축출된 채널의 `evicted_through`가 존재. debug 빌드에서 돌려 `debug_assert`도 활성화한다.

### m3. "conservative" 과금이 1~3키 객체에서 과소 — hub/mod.rs:150-155

serde_json `Map`은 BTreeMap이다(`preserve_order` 미사용). 첫 삽입 때 leaf 노드 약 632 B(8+4+11×24+11×32)를 할당하는데, 과금은 쌍당 `56+96=152` B다. 그래서 1키 객체는 152 vs 약 640, 3키는 456 vs 약 640으로 과소다. 5키 이상은 과대.
실측상 문제는 작다. 2회차 t=0→180 s에서 cost가 +19.95 MiB, daemon RSS가 +21.6 MiB(다른 warm-up 포함)로 거의 1:1이다. 보고서도 RSS가 아니라고 명시한다. 주석을 "approximate logical charge"로 고치거나 객체당 최소 leaf 크기(`max(n×152, 640)`)를 과금하는 것을 권장한다.

### m4. Notable 캐시가 차면 공유 효과가 끝남 — daemon.rs:213-225

캐시는 축출 없이 4,096키에서 멈춘다. CLI client id는 `name#pid` 형태라(daemon.rs:333-336) 호출마다 새 키가 생긴다. 장기 실행 daemon에서는 오래된 client id가 캐시를 채우고, 그 뒤 새 kind/channel 문자열은 공유되지 않아 항목마다 Arc를 할당한다. 그래도 NOTABLE_CAP 이내이고 종전보다 나빠지지는 않으므로 회귀는 아니다. 고정 client 3개인 소크에는 영향이 없다. 키 String과 Arc를 중복 보관한다.
- 제안: 가득 찼을 때 `strings.retain(|_, a| Arc::strong_count(a) > 1)` 한 번 후 삽입, 또는 `BTreeSet<Arc<str>>`(`Borrow<str>`)로 중복 제거.

### m5. 라이브 보관 깊이 감소폭(정보, 결함 아님) — hub/mod.rs:46-48, 65

2회차 실측 charge/JSON 비율은 5.3~5.5×, 레코드당 약 2.9 KB다. `can:ch0` 링은 약 1,400건 ≈ 210 frame/s에서 약 7초 분량이다(JSON 전용이었다면 약 7,900건 ≈ 38초). `DEFAULT_BACKLOG` 1000건은 이 프로파일에서는 링으로 채워진다. CAN-FD 64 B·디코드 신호가 많은 payload에서는 1000건 아래로 떨어질 수 있고, 그때 viewer 진입 backlog는 coverage 표시와 함께 일부만 링에서 나온다. journal 경로는 그대로다. 수용 보고에 이 수치를 명시할 것을 권장한다.

## 소크 도구(ui/tools/p1-soak.ts) — 계측 진실성

- 판정 로직은 기준과 일치한다(최초 120 s 제외, 120~420 s / 마지막 300 s 중앙값, 구간별 coverage ≥ 0.9, ≤ 10 MiB). 워커 재시작은 키(pid·epoch) 분리로 insufficient → fail이 되어 정직하다. `order_entries ≠ records`면 오류로 계상되어 fail한다.
- nit s1(28행): `diff_sha256`은 tracked 변경만 포함한다. untracked 파일(현재 `ui/e2e/real-tc1-server.ts`, `ui/tools/p1-browser-preflight.ts`)과 root `Cargo.toml`은 빠지고, 무관한 `ui/e2e/real-smoke.spec.ts` 수정은 들어간다. 2회차는 crates가 `37feca4`에서 clean이므로 `head` + `binary_sha256 db06869f…`가 권위 식별자다. `git status --porcelain` 해시를 함께 기록하는 것을 권장한다.
- nit s2(29행): `LOOMD_BIN`이 없으면 cwd 기준 `../target/debug/loomd`를 해시하지만, 서버는 repo 기준 경로를 실행한다(real-smoke-server.ts:22). cwd≠ui면 불일치한다. 2회차는 `LOOMD_BIN=~/.cache/loom-p1-soak-build/attempt2/loomd`라 영향 없다.

## 2회차 판정 해석 주의 (코드 결함 아님, 기준 변경 제안 아님)

t=180 s 시점에 링 charge는 20.26 MiB이고 분당 +2.5 MiB로 아직 채우는 중이다. RSS는 57.67 MiB로 charge를 약 1:1로 따라간다. 저속 채널(core/eth/psu/uds)의 링이 앞 구간 중앙값(약 t=270 s) 이후 포화되면, 유한한 남은 채움분이 growth에 들어간다. 이 경우 실패/통과와 별개로 `cacheSamples`의 앞·뒤 구간 `cost_bytes` 중앙값 차이로 growth를 링 채움과 그 밖으로 나눠 보고하면, 누수가 아니라 상한 있는 채움이라는 근거를 남길 수 있다.

## 제안 patch

없음(m1·m2만 실질 제안, 위 서술로 충분). 이번 diff 범위 밖이라 확대하지 않은 것: 빈 링 키의 HashMap 잔존, `event_cursors`/`focus_last` 상한.

## 메인 후속 보완

- m1: 채널 예산보다 큰 단일 레코드는 기존 링을 축출하지 않고 그 레코드만 보관 제외한다. `evicted_through`는 마지막 누락 seq의 보수적 watermark이므로 그 앞의 보관된 레코드를 반환하면서 coverage를 incomplete로 보고할 수 있다. journal/실시간 전달은 유지한다. 깊이 68 JSON 회귀가 기존 10건 보존과 누락 seq 11을 확인했다.
- m2: 9개 채널의 실제 전역 압력을 만들어 최고령 채널 축출·32MiB charge 상한·순서 인덱스 1:1·coverage 누락을 확인했다.
- m3: 주석을 approximate logical charge로 바로잡았다. charge 수식과 실행 중인 2회차의 부하·판정 기준은 바꾸지 않았다.
- 검사: `/private/tmp/loom-p1-retention-review-regression.log`, gateway hub 통합 8 PASS.
- 이 보완은 정식 2회차 binary 동결 뒤의 변경이다. 측정의 보통 payload는 단일 레코드 예산 초과 분기를 타지 않아 해당 보완의 실행 결과를 측정했다고 주장하지 않는다.
