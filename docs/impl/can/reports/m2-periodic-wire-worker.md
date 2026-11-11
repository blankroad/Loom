# can-sol M2a — periodic worker 연결

2026-11-11 KST. 지정 worktree에서 작업. 변경은 `src/worker.rs`, 새 `src/periodic.rs`, 새 `tests/periodic.rs`와 이 보고서뿐이다. gateway/CLI/signals·공통 schema/proto·lib/Cargo manifest·스킬은 수정하지 않았고 커밋하지 않았다. 동시에 보이는 다른 변경은 can-opus 작업이다.

## 연결한 것

- **worker 공용 Scheduler 하나**: 전체 채널 합계128잡·256 pending patch 상한. 채널별 scheduler로 상한이 곱절이 되는 것을 피했다. metadata도 활성/종료 이벤트 대기 잡당1개이며 종료 이벤트 처리 후 삭제한다. stopped-job archive 없음.
- worker write로 `can.periodic.start/update/stop/renew/takeover`, call로 `list`. start의 FramePayload와 CAN text를 지원하며 period1..60000ms·기간 ns overflow·metadata 길이를 검증한다. `can.status` 전체/채널별 job 수와 failure/inhibited 상태 추가.
- pump는 기존 ISO-TP settle을 먼저 하고 periodic 한 batch를 처리한다. `poll_at`을 다음 wake에 합치고, `is_due_current`와 **최종 worker latch**를 enqueue 직전 재검사한다. 잡 origin의 sim sender 사용, DispatchId callback으로 접수/실패를 보고한다. callback 압력이 있으면 callback만 재시도하며 실제 프레임을 다시 보내지 않는다.
- `can.job_state` priority lifecycle과 일반 `can.frame` tx를 발행한다. frame top-level `job_id`, `purpose="periodic"`, payload revision을 보존한다. 공유 sim bus의 다른 관찰 채널에도 동일 job 태그가 유지된다. 기존 data-lane API 제약 때문에 origin/request를 가진 프레임은 현재 worker와 같이 priority lane을 쓴다.
- STOP은 worker gate부터 잠근 뒤 Scheduler를 latch한다. User를 포함한 **모든 periodic** 잡을 정지하고 새 시작을 거절한다. 해제 후 자동 재개 없음. User1회 raw 송신은 계속 가능하다. SDK atomic latch만 먼저 바뀐 경우도 pump가 감지한다.
- ISO-TP 예약 tx_id는 모든 origin의 periodic/raw 송신에서 `E_DENIED{reason:tx_reserved}`. 반대로 periodic이 쓰는 id에 ISO-TP open은 `E_LOCKED`. 진행 중 burst와 같은 id의 periodic 시작도 거절한다.
- periodic id의 raw 송신은 User의 count1만 허용한다. `payload.override_of`와 write detail에 job을 넣고 잡 payload·소유권은 유지한다. Agent/Script raw와 User burst는 `E_LOCKED`.
- 채널 close는 해당 periodic을 먼저 정지하고 `reason=channel_closed`를 발행한다. reopen에 잡을 복원하지 않는다. lifecycle 발행 실패는 acceptance 실패·periodic inhibition으로 처리하여 보이지 않는 잡이 계속 송신하지 않게 했다.

`periodic.rs`는 worker-local `#[path]` 모듈로 연결했다. 공유 `lib.rs`를 수정할 필요가 없다.

## 목록·CAS·소유권 계약

| 필드/동작 | 의미 |
|---|---|
| `revision` + `data/frame` | **최신 접수 revision의 바이트**. 첫 tick 전·pending patch 중에도 IG가 CAS 기준으로 사용할 수 있다 |
| `applied_revision` + `effective_frame` | 실제 다음 송신에 반영된 revision/프레임. 접수와 적용 시점을 구분한다 |
| `sent/missed/lateness_p99_us` | Scheduler의 실제 제출 통계. p99는 histogram 상한 근사, sim driver 접수이며 wire 확인 아님 |
| `count_left/lease_left_ms/state` | 잔여 성공 제출 수·lease·running/paused |
| mutation 권한 | 같은 client+origin 또는 User 개입. `all_mine`은 client+origin이 같은 잡만 정지 |
| takeover | 명시 takeover가 fresh JobId를 만든다. 새 소유는 요청 client/origin. 원래 grid·lease/max cap·count 보존, old-owner 미반영 patch 폐기 |

`data/frame`은 revision과 일치하는 **접수** 바이트이므로 `effective_revision`이라는 이름으로 이전 송신 revision을 내보내지 않는다. 적용된 revision은 `applied_revision`으로 별도 제공한다. can-opus의 IG가 목록의 data를 우선 사용할 수 있고, CAS 충돌의 `detail.current_rev`도 제공한다.

## 시험·결과

최종 소스에서 통과:

```sh
cargo fmt --all --check
cargo clippy -p loom-can --all-targets -- -D warnings
cargo test -p loom-can
```

**전체94개 통과, 실패0**: unit7·CLI13·gateway17·ISO-TP8·신규 periodic16·signals16·기존 worker17. 공유 worktree의 can-opus 변경을 포함한 시점의 결과다. 신규16개는 모두 FakeWorker/ManualClock 계열이고, 발행 실패 시험은 capacity1 MemorySink를 직접 사용한다.

- 10ms×1초100프레임, 정확한 sim 시각·origin/job/purpose, 목록 통계·status job 수.
- CAS 성공/충돌·연속 masked patch 합성·다음 tick 반영·접수 payload와 적용 payload 구분.
- count 완료와 마지막 echo 뒤 stopped 이벤트, lease renew/만료·누적 cap·offset.
- Agent/Script/User STOP·신규 거절·사용자1회 송신·해제 후 재개 없음·atomic latch 선반영.
- 중복·client 소유권·takeover와 pending patch 폐기, ISO-TP 예약 충돌 양방향.
- raw User override 태그·원래 producer 유지, all_mine 범위, close/reopen 복원 없음.
- 채널 합계129번째 잡 거절·metadata 회수, 늦은 pump skip3·FD flags/patch.
- 공유 bus 관찰 채널 태그, lifecycle 발행 실패 시 accepted 거부·잡0·periodic inhibition.

실HW·Windows·실제 IPC·브라우저 E2E·장시간 soak·worker 버전 지터는 측정하지 않았다. Scheduler 자체 스모크 수치를 worker 성능으로 재인용하지 않는다.

## 결정·리드 요청

1. **공통 `ErrorCode`에 `E_CONFLICT`가 없다.** 소유 경계를 지켜 proto enum을 수정하지 않았다. CAS는 현재 `E_LOCKED` + `detail{current_rev, reason:"revision_conflict", code:"E_CONFLICT"}`로 전달한다. 기능적 CAS·IG 재시도는 가능하지만 최상위 wire code는 요청 계약과 다르다. CORE가 정식 Conflict를 추가한 뒤 `schedule_error` 매핑을 교체할 것.
2. control write의 완료는 `WriteOutcome{state:accepted, detail:...}`이고 lifecycle을 먼저 발행한다. 실제 주기 frame 제출은 별도 이벤트/통계다. generic SDK `JobCmd`는 매핑하지 않고 이 작업의 `Write(class=Job, op.method=can.periodic.*)` 경로를 구현했다. live core IPC가 이 write 경로를 보존하는지 통합 때 확인할 것.
3. publication fault의 `periodic.inhibited`는 worker 재시작 전까지 유지한다. STOP 해제만으로 기록 장애 상태를 정상으로 바꾸거나 잡을 재개하지 않는다. 기존 raw 경로·priority lane·관찰 손실 카운터는 유지했다. CORE 기록 건강 상태·공통 감사와의 연계는 후속이다.
4. User의 일반 변경은 다른 잡에도 허용해 IG 개입을 보장한다. Agent/Script는 자기 client+origin 외의 mutation을 거절하고 명시 takeover를 요구한다. `owner` 문자열은 표시 label이며 인증 수단으로 쓰지 않는다.
5. sim backend에는 BusOff/ErrorPassive 알림 입력이 아직 없어 새 pause RPC를 임의로 만들지 않았다. 실제 backend 연결 시 Scheduler `pause_channel/resume_channel`와 worker 상태 이벤트를 연결할 것. 기존 raw burst의 STOP 동작은 이번 periodic 범위에서 바꾸지 않았다.
