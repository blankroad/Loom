# can-sol M2a — periodic 스케줄러 보고

2026-11-11 KST. worktree: `/Users/ctmctm/workspace/Loom/.worktrees/can`.
구현 범위: `crates/loom-can-periodic/`와 이 보고서. `loom-can/`·공통 frame·root manifest는 수정하지 않았고 커밋하지 않았다.

## 구현·공개 API

의존은 `loom-can-frame` path 하나다. workspace package/lints 상속, unsafe·Python·IO·스레드·시계 없는 sans-IO 코어다. 상세 호출 계약은 [README](../../../../crates/loom-can-periodic/README.md).

| API | 동작 |
|---|---|
| `Scheduler::new/with_limits`, `start/start_at(JobSpec, now)` | 채널·CAN ID producer 중재, 잡128개, 최초 deadline=`start+period+offset` |
| `poll/on_timer(now)`, `poll_at`, `pop_due/pop_event` | 절대 deadline min-heap. 잡당 미확인 출력1개. 지연 시1개만 송신하고 나머지 tick skip, 격자 유지 |
| `patch(job, expected_rev, Patch)` | byte mask/전체 payload 교체, CAS, 다음 실제 송신 tick에 원자 반영. queued Due는 바뀌지 않음 |
| `on_submitted/on_send_failed`, `on_dispatch_submitted/on_dispatch_failed` | 실제 드라이버 접수·실패와 missed/sent/failed·고정 histogram 통계. bus 확인을 주장하지 않음 |
| `is_due_current` | 최종 enqueue 직전 removed Due의 dispatch/owner/origin/bytes/revision·수명·pause/latch 검사 |
| `renew`, `stop`, `latch/latch_at`, `takeover` | 누적 수명·lease·count, 전체 잡 STOP, 소유자 교체·이전 pending 취소 |
| `pause_channel(_at)`, `resume_channel(_at)`, `job` | Agent/Script 보류·명시 재개, User 잡은 worker 장치 관문 적용, 상태·통계 조회 |

heap은 잡당 노드1개만 유지하고 변경 후 재구성한다. lazy tombstone이 쌓이지 않는다. 모든 poll의 처리 계획과 출력 공간을 사전 검사하므로 `OutputFull`이 clock·deadline·missed·patch revision을 부분 변경하지 않는다. stopped job은 삭제하고 bounded event에 최종 통계를 넣는다.

## C2 상한

| 자원 | hard cap·정책 |
|---|---|
| 잡·heap·poll action·resume ID 임시 목록 | 각각 최대128개; admission 전에 `ResourceExhausted` |
| patch 대기 |256개, 결과 CanFrame+revision으로 저장; 초과 시 CAS/revision 변경 없음 |
| Due / Event 출력 |128 /256개, 초과 전 `OutputFull`. 기본 빈 큐는 전체128잡 poll/STOP 수용 가능 |
| 정지 채널 기록 |128개, 새 채널 초과 시 변경 전 거절, resume 시 회수 |
| 문자열·patch | channel64B·owner128B, patch payload/mask64B 고정 값 |
| 통계 | 잡당128 histogram bins, 모든 누계 saturation. ID/revision 고갈은 wrap 대신 오류 |

quantile은 상한 근사값이다:0ns 별도 bin,8ms까지125µs 폭64개, 이후2배 폭으로 u64 최대까지. max는 정확값이다. 기간·grid 곱셈은 u128, 표현 불가능한 시작·수명은 admission에서 거절한다. worker 전체 RSS/24h soak·pool 합산 관문은 이번 범위가 아니다.

## 검증 결과

```sh
cargo fmt --all --check
cargo clippy -p loom-can-periodic --all-targets -- -D warnings
cargo test -p loom-can-periodic
cargo test -p loom-can-periodic --test scheduler realtime_10ms_500_submissions -- --ignored --nocapture
```

모두 통과했다. 기본 시험 **23개 통과**(unit1 + integration22), 선택 스모크는 기본1개 ignore이고 별도로 실행해 **1개 통과**했다. 기본 integration 실행0.33s, 스모크5.02s로10분 제한 이내다.

- 10ms×1초 정확히100개, offset·절대 격자·지연35ms에서 skip3 및 catch-up 없음.
- 미확인 driver 출력 동안 backpressure, 늦은 접수 뒤에도 다음 tick으로 진행.
- patch CAS·여러 byte mask의 원자 합성·다음 실제 tick 반영·이미 꺼낸 frame 불변·FD 길이/flags·256개 상한.
- finite count·송신 실패·lease renew/만료/누적 상한·max duration과 정확한 만료 시각의 우선순위.
- Agent/Script/User 전체 latch·신규 periodic 차단·해제 후 재개 없음·중재 channel/ext/id·takeover.
- BusOff/ErrorPassive·명시 resume·새 Agent/Script에 정지 채널 적용·만료 잡 재개 금지·stale dispatch 격리.
-129번째 잡·Due/Event 출력 압력·callback 오류 재시도에서 부분 상태 변경 없음·counter saturation·timestamp overflow.
- **128개 서로 다른10..1280ms 주기 ×1분 가상 시각:32,546프레임**. 순서·잡별 정확한 개수·missed0·lateness0 확인.

초기 Clippy의 saturating arithmetic/contains 지적2건을 수정한 뒤 전체 관문을 통과했다. 실HW·ISO-TP와의 공유 TX 스레드·정책·UI는 아직 연결하지 않았다.

## 선택 스모크 — 실측, macOS aarch64

전용 OS 스레드, 기본 우선순위, 절대 Instant deadline에 `std::thread::sleep`, spin 없음. driver를 부르지 않는 host 제출 hook이다.500회, elapsed5.013563s, missed1. 인위적인 경쟁 부하를 추가하지 않았지만 호스트의 다른 작업은 통제하지 않았다.

| 지표 | p50 | p99 | max |
|---|---:|---:|---:|
| 제출 lateness(정확 표본, ms) |2.477792|6.824875|13.109000|
|10ms 절대 간격 오차(정확 표본, ms) |1.688958|6.330625|12.189625|
| histogram lateness 상한 근사(ms) |2.500000|6.875000|13.109000(정확 max)|

이전 `spikes/can/rust/RESULTS.md`의 비교 가능한 지표는 **간격 오차 p99 4.527333ms(약4.53)**다. 이번6.330625ms가 더 높다. 표본 수·호스트 부하가 다른1회 시험이므로 코드의 성능 회귀나 개선으로 단정하지 않는다. 잠정 p99<3ms 목표는 이번 조건에서 **미달**이다. Windows·실버스 정확도나 FC timing으로 확대할 수 없다. spin/우선순위 옵션은 이 sans-IO 코어에 넣지 않았으며 CPU 비용은 측정하지 않았다.

## 결정·리드 통합 요청

1. 첫 tick은1period 후(+offset). count는 **성공한 driver submission 수**로 정했다. 실패/skip은 소비하지 않으며 max duration이 무한 실패 수명을 제한한다.
2. STOP은 모든 periodic job을 latch하므로 **새 User periodic도 거절**한다. User1회 송신은 이 crate 밖에서 계속 허용해야 한다. 해제만으로 재개하지 않는다.
3. BusOff/ErrorPassive는 `Stopped{resumable:true}` 이벤트와 paused 잡 유지로 표현한다. terminal stop은 resumable=false. lease/max 수명은 pause·takeover로 초기화하지 않는다. takeover는 fresh JobId·revision0·통계0, 적용된 bytes·격자·잔여 count·원래 수명 cap을 보존하며 old-owner 미반영 patch는 폐기한다.
4. worker의 **독립 최종 TX latch를 먼저 설정**한 뒤 scheduler latch를 호출할 것. event 큐가 full이면 상태 변경 전에 오류를 반환하므로 drain/retry 동안 실제 enqueue 관문을 계속 잠가야 한다. 좁힌 custom 큐는 동시 batch를 수용하도록 잡 수/offset과 함께 설계해야 한다.
5. removed Due는 최종 enqueue 직전 `is_due_current` 검사, callback은 **DispatchId 버전**을 사용해야 pause/resume 뒤 늦은 callback이 새 출력을 완료시키지 않는다. callback OutputFull 시 재시도하는 것은 callback이며 물리 송신을 반복하지 않는다. submitted 보고가 만료 뒤 도착하면 driver 사실은 sent로 세고 expired_submissions를 올린 뒤 정지한다.
6. 소유권·승인·예약 tx_id·rate·device state·publish-before-submit·raw journal·실제 드라이버 timeout은 worker/gateway에서 연결할 것. `loom-can/`은 can-opus 소유이므로 건드리지 않았다.
7. Cargo가 시험 중 만든 root Cargo.lock의 이 신규 package 항목만 정리해 최종 scope diff를 유지한다. 리드가 새 crate를 통합/커밋할 때 **workspace Cargo.lock을 재생성해 포함**할 것. 다른 root 파일이나 기존 package 항목은 변경하지 않는다.
