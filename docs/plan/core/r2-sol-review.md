# CORE R2 교차 리뷰 — Sol → Opus

2026-11-10 · `02-core-principle.md` 준수. 대상: [Opus R1](r1-opus.md), [core-test O1–O9](../_xteam/core-test.md). 비교: [Sol R1](r1-sol.md). **초안·xteam 파일은 수정하지 않았다.** P0는 계약 동결 전 필수 수정, P1은 해당 기능 수용 전 수정이다. `[확인]`은 원문/공식 API/소스 확인, `[산술]`·`[설계 반례]`는 제품 실행 결과가 아니다.

## 1. 요약 판정

조건부 채택: Opus의 API·UX 골격을 통합 기준으로 삼되, 아래 P0를 고치기 전 계약 v1을 동결하면 안 된다.
강점 3개: 구체적인 Registry/worker trait, 사용자 개입·출처·승인 UX, TEST의 상태·증거·freeze 규칙을 연결한 점.
큰 문제 3개: 합산되지 않은 메모리 예산, 발행/기록/STOP 실패 시 불변식 약화, 기대조건·승인 횟수·증거 완전성의 결속 부족.
crate 버전 대부분은 실제 존재한다. **zstd0.14.3은 오류가 아니며, 이 부분은 Sol의 최신 버전 수집 방식을 정정해야 한다.**
새 성능 벤치는 하지 않았다. 공식 API 재조회·산술/설계 반례를 수행했고, Rust 보조 검증은 실행 오류/timeout 때문에 소스 확인과 구분했다.

## 2. 틀린 사실·불변식·내부 모순

표의 “실제”는 구현 버그를 발견했다는 뜻이 아니다. 원문에서 확정 가능한 모순과 그 설계를 그대로 구현했을 때의 반례다. 해결안은 마지막 열에 함께 적었다.

| ID·우선순위 | 위치·상대 주장 | 실제·수정안 | 근거·확인 상태 |
|---|---|---|---|
| F1 **P0** | §9.1 L987/996–997, §9.2 L1012:64연결·연결당 32구독, 구독당 3 MiB, gateway256/512 MiB | **2048구독·논리 큐 용량 6144 MiB**다. 예산 설명은 64개 큐만 세고 그 합도 64+192+32+64=352 MiB라 soft256을 넘는다. Arc 공유 때문에 실제 RSS가 꼭 6 GiB인 것은 아니지만 shared backing/고유 payload/참조 비용의 합산 증명이 없다. 전체 구독·전체 queued bytes admission을 추가한다. | `[산술]` [반례 JSON](../../../spikes/core/r2-evidence/design-counterexamples.json) `subscriber_budget` |
| F2 **P0** | §9.1 L982/1000, §9.2 L1014:UART8포트·링 16 MiB/포트, worker64/128 MiB | 링만 128 MiB, journal queue cap64 MiB와 runtime은 별도다. “포트 수에 비례 설정”이면 **기동 전 산정식·전체 상한·포트 거절 규칙**이 필요하다. 기본 8포트와 기본 hard128을 동시에 수용 가능하다고 표시하면 안 된다. | `[산술]` 동일 JSON `uart_budget` |
| F3 **P0** | §6.2 L840/849, Q17 L1002, O3 L80:stream 인덱스는 드롭해도 원본에서 복원 가능 | worker 원본에 stream seq가 없다고 L767이 명시한다. 두 producer의 순서를 보존하는 4이벤트는 **가능한 전역 순서 6개**라 원본만으로 옛 stream mapping을 확정할 수 없다. source offset 인덱스와 gateway의 **정본 mapping log**를 구분하고, mapping durable 범위 밖은 uncertain/source ref 조회로 처리한다. endpoint 범위만으로 불연속 source seq를 선형 매핑하지 않는다. | `[확인: 원문]` L767/840/1002, O3; `[설계 반례]` JSON `lost_stream_mapping` |
| F4 **P0** | §3.3 L269, §10.3 L1145:50 ms 발행 대기 후 `publish_lag` 경고로 응답, 순서 위반 아님 | covers42·published41에서 응답하면 **publish-before-reply 위반**이다. 경고가 순서를 회복하지 않는다. 발행 완료로 선형화하거나 정상 성공과 구분한 관측 미완료 결과를 반환해야 한다. 이미 송신한 장치 동작을 “failed, retryable”로 바꾸지도 않는다. 허브 enqueue·socket write·브라우저 렌더 세 의미를 분리하고 T2도 같은 정의로 고친다. | `[설계 반례]` JSON `publish_timeout`; D2, T2 L1024와 본문 불일치 |
| F5 **P0** | Q14–15 L999–1000, §10.5 L1185:저널 큐 넘침·record.gap이어도 degraded이며 agent TX 허용 | 실제 원본을 버린 상태와 단순 backlog 경고를 섞었다. **기록 손실이 발생한 경로는 A12의 기록 불능으로 다뤄 agent/script 새 R1+를 차단**하고 user 입력은 유지한다. 회복의 조건·loss window도 명시한다. “큐만 느림, 아직 무손실”일 때의 degraded와 구분해야 한다. | `[확인]` 마스터 A12·D7, can W3/uds C-10/TEST TC-C8 |
| F6 **P0** | §10.3 L1143/1147, §10.4 L1179, O7 L126:backend 후 once 승인 consume | `uses_left=1`을 두 요청이 먼저 확인하면 **둘 다 송신한 뒤 소비**할 수 있다. request id/idempotency도 완료 후 store만 있고 pending 원자 예약이 없다(L1109/1149). presend 앞에서 승인 횟수·idempotency를 원자 reserve, backend 제출 시 consume 확정; 미제출 rollback과 outcome_unknown의 비재사용 규칙을 둔다. | `[확인: 의사코드 순서]` `[설계 반례]` JSON `approval_check_then_consume`; 최신 uds C-7의 원자 소비 요구 |
| F7 **P0** | §3.5 L352–354:gateway latch.json fsync 후 worker에 STOP 전달 | 디스크 full/hang이면 **이미 실행 중인 worker job의 latch 전달이 디스크를 기다린다**. gateway 메모리 latch→worker 제어 전달을 먼저 하고 persistence를 독립 수행한다. 저장 실패는 명시하되 STOP 자체를 거절하지 않는다. 재시작 시 불명 상태는 TX-latched로 시작한다. | `[확인: 순서]` `[설계 반례]` 디스크 대기 100 ms면 worker ACK<10 ms 불가. A3·A12·can W5 |
| F8 **P1** | §6.2 L824/835:정본을 MessagePack array 세그먼트로 채택, D7 유지 | D7의 **JSONL 청크+raw 정본**을 바꾸는 제안이다(C3가 D7 유지). binary가 나쁘다는 뜻이 아니라 **CR 누락**이다. R3는 M1 JSONL+raw 유지 또는 메인의 명시적 D7 변경 중 하나를 택한다. 성능 측정은 동일 full schema·내구성으로 비교한다. | `[확인]` 마스터 D7/C3, 형식 L835. Sol 저널 실측은 JSONL+zstd이며 MsgPack 정본 수치가 아님 |
| F9 **P1** | §6.2 L835, §7.1 L892:`crc32c` 대신`crc32fast` 가능 | CRC32C(Castagnoli)와`crc32fast`의 CRC32(IEEE)는 다르다. `123456789` 결과는각 e3069283/cbf43926이다. LJ1이 CRC32C라면`crc32c`를 고정하거나 header에 checksum algorithm id를 넣고 형식 변경을 선언한다. | `[확인]` [crc32fast 공식 문서](https://docs.rs/crc32fast/1.5.4/crc32fast/), [crc32c](https://docs.rs/crc32c/0.6.9/crc32c/); `[산술]` JSON `crc_polynomial` |
| F10 **P1** | §3.3 L289, §8 L965, §9.2:Windows Job 메모리 상한을 RSS 예산과 연결 | `JOB_OBJECT_LIMIT_PROCESS_MEMORY`는 **commit 한계**, RSS hard cap이 아니다. WorkingSet/PrivateUsage·Job committed bytes를 따로 기록하고 예산의 기준을 분리한다. Linux RLIMIT_DATA도 전체 RSS/자식 합산 hard cap이라고 부를 수 없다. 4.7 이후 일부 mmap에도 적용되므로 “heap만”이라고 반대로 단정하지도 않는다. | `[확인]` [Microsoft limit 구조체](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information), [Linux getrlimit](https://man7.org/linux/man-pages/man2/getrlimit.2.html) |
| F11 **P1** | §0.3 L60/62, §9.1 L982:classic 최소프레임 20k fps·UDS 화면 지연은 버스 부하와 무관 | DLC0 표준프레임의 stuffing 없는 47 bit+IFS 모델은약 21.3k fps이므로 20k 자체는 대략적 부하값으로 가능하다. **FD64 B·8 Mbit/s의 부하 계산은 아니다.** 두 IPC lane도버스 arbitration·driver queue를 없애지 못한다. UDS **응답 host-ingest→화면**과 요청→ECU응답을 나눠 보장한다. | `[확인/산술]` [CiA CAN FD 설명](https://www.can-cia.org/can-knowledge/can-fd-the-basic-idea), Sol§9.1; 시험에 DLC/ID/BRS/nominal·data bitrate 필수 |
| F12 **P1** | §1.3 L125, §13 L1333, opus-repr RESULT:단일 JSON 인코드 1.05M/s로설계최대 80k의 10배여유 | 확인된 것은 **8 B payload의 사전 생성 이벤트 encode loop**다. fullR1타입·FD64 B·decode·JCS·journal·필터·fan-out·UI가 빠졌다. “허브/소켓 경로 10배 여유”로 확대할 수 없다. R1 Sol100k/s 역시 축약 tuple·3 s paced 부하라 최대 지속 처리량 증명이 아니다. | `[확인: 소스]` [opus-repr](../../../spikes/core/opus-repr/src/main.rs), [Sol 실측 한계](../../../spikes/core/README.md) |
| F13 **P1** | opus-repr `Bytes::deserialize`, src/main.rs L35–38 | 홀수 길이 hex는`s[i..i+2]`범위초과, Unicode는 UTF-8경계로 panic 가능하다. 유효값 round-trip만으로 외부 입력 안전성을 확인했다고 할 수 없다. 길이·ASCII·최대 bytes를 먼저검사하고 fallible decoder·invalid corpus를 P0에 둔다. | `[확인: 소스]` [견본](../../../spikes/core/opus-repr/src/main.rs). [R2 검증 코드](../../../spikes/core/r2-contract/src/main.rs)는작성했으나 Cargo 실행 미완료이므로실측 panic으로표기하지않음 |
| F14 **P1** | §3.9 L455 전후`presend_check`동기 Result, §10.3 L1138에서는`.await` | 같은 API 계약이 아니다. 동기·≤1 ms 훅은`.await`없이호출하고 await가 필요한 backend와 분리한다. 예시 의사코드지만 첫 주 stub복사 시 compile실패하므로 인터페이스 검증을 선행한다. | `[확인: 시그니처 대조]` Rust의 Result는 Future가 아님; 제품 compile실패를측정한것은아님 |
| F15 **P1** | §6.2 L855:GC는 seal된오래된세션, 열린세션과인용세션전체보호; §9.5 quota동작 | daemon_session이 24 h열려있으면그안의 sealed segment도세션단위제외라 GC할 수 없다. Opus부하 평균 CAN15k/s는 8 B tuple120 B를가정해도하루약 145 GiB, SolJSON580.778 B대입시약 701 GiB(다른포맷의계획용산술)다. **active 파일만 제외·필요 segment만 pin·rolling retention**또는충분한별도 soak저장소가필요하다. | `[산술]` JSON `storage_arithmetic`; 실제 MsgPack/zstd 용량으로 일반화하지 않음 |

### crate 사실 확인의 결론

- `[확인]` tokio1.54.1·axum0.8.9·rust-embed8.13.0·rmp-serde1.3.1·interprocess2.4.8·schemars1.2.3·rusqlite0.41.1·clap4.6.7·win32job2.0.3은 R1공개 API원본과일치한다. `crc32fast1.5.4`, `sha2 0.11.1`, `serde_jcs0.2.0`, `notify9.0.0-rc.8`도 R2직접조회에서존재했다. **없는 crate/버전이라고지적할근거는없다.**
- `[확인]` [zstd API](https://crates.io/api/v1/crates/zstd)의`max_stable_version/default_version=0.14.3`, 최근게시`newest_version=0.13.4`는동시에가능하다. Opus0.14.3은정상후보, Sol실측 0.13.4와의차이를명시한다. [notify](https://crates.io/api/v1/crates/notify) 정식후보는 8.2.2(CC0-1.0),9.0.0-rc.8은사전판이다.
- `[확인]` axum0.8.9의**내부 WS의존성은 tokio-tungstenite0.29.0**다([패키지소스](https://docs.rs/crate/axum/0.8.9/source/Cargo.toml)). 별도 client0.30.0사용은가능하나“axum의버전”과“CLI직접의존버전”을분리한다. 이는주버전후보자체가틀렸다는뜻은아니다.
- Windows핵심은 interprocess의 UDS/named-pipe매핑과 Windows Job API가실재한다는점까지확인했다. **Windows벤더 DLL·실장치·RSS enforcement 실행확인은없다.** 전체라이선스/다운로드/릴리스표는 Sol R1자료로통합하되버전선택기준을고친다. R2추가 11개 API원본은[조사폴더](../../../spikes/core/r2-evidence/README.md)에보존했다.

## 3. 빠진 것·답변이 불완전한 것

### 3.1 brief 범위 10항목 점검

| 항목 | 판정·보강할 것 |
|---|---|
| 1 프로세스 | W1–W5·UDS/DLL분리명확. gateway가 CPU/메모리한계를넘을때 worker이전관문, UART그룹의실제예산식필요(F2). |
| 2 worker protocol | 2물리 lane·credit·worker저널수용. ctl FIFO안의 STOP/Health/PDU우선순위·바이트 cap·부분 frame/read deadline·재부착인증 nonce·heartbeat와 job lease독립성은부족. Q6 block이도메인 RX/FC스레드를막지않는반환형/overflow정책필요. |
| 3 WS/hub | snapshot의 H캡처는좋음. **snapshot전송중 live별도큐상한·overflow시재동기화**, snapshot일관성에따른불변스냅샷수명, WS연결당단일 writer·send timeout·제어셀전역구독이빠졌다. F1/F4와연결. |
| 4 저널 | worker직접쓰기선택명확. D7변경 CR·정본 mapping·retention·CRC중간손상과 tail구분·written/durable watermark필요(F3/F8/F15). flush스케줄은지연상한의목표이며전원손실보장으로단정하지않는다. |
| 5 CLI | 작은동기 CLI·detach제안구체적. 동시부팅 lock기다림상한, **idempotency pending/TTL/바이트예산**, epoch포함 cursor·agent결과`data.evidence[]`계약이추가로필요. 기동 30/60 ms는미측정목표. |
| 6 정책·STOP | hooks/등급/승인 UX구체적. F5/F6/F7·차단된요청도 Requested/Denied관측가능하게·A3비상 OFF와 A15일반복구분리필요. L1112의 safe_state예외가 agent에게도적용되지않게 role검사를명시한다. |
| 7 schema | 정본경로일치. Schema draft·binary tuple evolution·JSON 정수 newtype범위·nullable/enum호환·TS/Python생성 golden검증·도메인 JSON Pointer정본위치가부족. |
| 8 runner/test | O1–O9가실질적계약. **등록된 plan expect hash결속**, coverage별술어분류, user개입시자동판정유효성, 세션 pause/retry/answer정의가추가필요(아래). |
| 9 C2 | 큐표/soak계획있지만 F1/F2로합산불가. 구독·리듀서·snapshot·idempotency·job/캐시·압축·tracing채널의**바이트예산**이빠짐. restart구간을섞은 RSS회귀와정상 UI의의도적축약을관문에서분리해야함. |
| 10 빌드·배포 | 2바이너리·동일 loomd재실행·3OS CI추천. P0fake가실제 hub/policy를의존하는범위축소, 정적 UIfixture서버의첫 주일정, 실제 브라우저와코어수신 gate분리가필요. vendor DLL미동봉은배포선택이며모든 vendor의재배포금지를확인한사실은아님. |

### 3.2 02 원칙 6항목 점검

| 원칙 | 판정 |
|---|---|
| 맨앞핵심루프표 | 충족(§0.1). |
| 구체적 E2E와사용자개입 | 충족(§0.2). 단 L51의홉별 p95합 60 ms는**예산합**이지종단 p95증명이아님. |
| 지연목표·고부하정직성 | 부분충족. F4/F11/F12·제어 HOL·실제렌더 ACK보강전보장문구완화. |
| M1걷는뼈대 | 방향충족. §0.6은세도메인인데§11수용예시는 echo/UART중심이고 TEST최소표시는 P2로밀림. UI통합관문·세도메인 E2E대상을명시해야함. |
| 늦은합류·숨은세션없음 | 설계충족, 시험은미실행. frame분할중 live폭주상한·worker화면미러 gap후상태 stale품질과복구방법추가. |
| 비핵심후순위 | 충족(§1.3), 과도한 P0FakeGateway는축소대상. |

### 3.3 타팀 요청 전수 대조

| 파일·번호 | 수용된 부분 | 빠짐·잘못 답함·통합 조치 |
|---|---|---|
| can-core W1–W4 | 2lane·credit·worker직접저널·원본 seq | F3. Q6의 block은 RX/FC와분리된 IPC스레드에만허용; 도메인 Emitter가 block하지않게명세. |
| can-core W5–W10 | STOP ACK·메트릭·DLL격리·RX만재개·Python공통 protocol | W5는 F7수정. W6는§3.3재부착 설계/§11 C12는 M1처럼쓰지만§13/§15는 M2로후퇴. **M1범위와갭표시를한곳에서확정**. Python SDK는선택 P4의존인데 P0필수완성관문으로확대하지않기. |
| can-core §2–§6 | Registry·JournalReader구상·EvidenceVerifier·부하생성분담 | Fixed/신호 snapshot이화면 drop후정확하다고오인하지않도록 quality/stale와복구질의. 전체 32sub상한·binary필드·source/streammapping확정. |
| uart-core #1–#4 | 별도 worker·10 ms배치·raw bin·입력빠른경로·감사묶음 | UI bin은 M1base64→M2협상이라는의도적차이, #2완전수용이라고하지않기. #3은장문 Input/일반 Call뒤 Ctrl-C가같은 ctl FIFO에갇히지않는구현필요. |
| uart-core #5–#8 | 웹자격·원시저널·now·안정 crate | viewer-tty표 L310와§10.6 L1192의 STOP해제/worker재시작권한불일치. C2메모리 F2·raw_ref권한/크기·SourceRecord와 PublishedRecord분리필요. |
| uart-core #9–#11 | evidence hook·help정본·soak | UART원문 byte-rangeAPI/스캔공통예산·세대/gap완전성시험. |
| uart-core #12–#16 | presence·입력우선·exit3 CR·within_pattern | **최신#12는 core.time/core.metrics.report수용으로갱신됨**. Opus§15의 core.clock.ping/latency_report“그대로수용”을갱신. #15는 CR인상태유지, #16은 F6원자 예약추가. |
| uds-core C-1–C-4 | sync훅·digest·invalidate·job/idle·recover | F6/F14. 최신 C-7의매송신 binding재생성·횟수원자소비필수. 단순범위내여부검사만으로 before값/보안상태변화를무시하면안됨. |
| uds-core C-5–C-10 | snapshot·TTL·검증 hook·seed-key분리·origin·record상태 | C-5는**viewer preview감사제외요구**인데 L1156/1414는매 preview감사 1행. UI150 ms입력미리보기와일반 CLI dry-run을분리. C-8의대상 32요청/링 10k를 core전체 byte cap에포함. C-10은 F5수정. |
| uds-core C-11–C-13 | exit3 CR·link-only재연결·전용 runtime선택 | 최신 C-12의세션/보안 stale·늦은응답 quarantine해제조건을보존. C-13은공유 runtime기본이아닌전용 런타임활성 fixture와 p99≤2 ms실측관문으로. |
| core-test TC-C1/C2 | client태깅·단계/epoch·술어재평가·check 후보≤20 | 장치 RX의단계귀속은 publish순서만이아닌 source fence/수집시각으로판정. **plan expect_ref/hash↔citation predicate**검사누락. F3·아래 O4보강. |
| core-test hold/measure·TC-C3 | 같은 source/신호/epoch·maxgap150 ms·서버직접시간계산·범위승인 | hold/measure/order/within도필요한구간 coverage와시계 quality를검사. endpoint만존재하는것으로통과시키지않기. R4포함 plan전체거부와 R4를분리승인하는사용흐름을구별. |
| core-test TC-C4/C8/C9·판정수정 | runner공유·freeze재바인딩·record불능 PASS거부·세션 1/최대 4·REF64·viewer수정 | F5/F6, 평가 CPU200 ms는**동기 trait를강제중단할수있는 hardlimit이아님**. AST깊이/노드수/regex입력상한·협력 budget token·초과후남은작업처리규칙필요. 100k/200k두평가 상한(L1224/1227)통일. |
| core-ui U1–U3 | credential분리·snapshot·JSON기본/binary옵션·embed | F1/F4, global control subscription·coalesce_by/id통계·불변 snapshotpendingcap·가변 binary schema와 JS corpus. M1rawbinary요구와 base64fallback합의필요. |
| core-ui U4–U8 | clock·query/HTTP·같은쓰기 RPC·schema생성·test이벤트 | `core.time`과`core.clock.ping`가문서내혼재. recv/send mono·오차폭·**미렌더 oldest age/rendered cursor**·metric보고형식확정. U7첫 주 fixture replay stub gateway는 P1 gateway일정과충돌. layout/RPC·focus이벤트명은 P0하나로정리. |
| test-domains TC-C5/C7 | domain EvidenceVerifier확장점 | **결과 data.evidence[] 공통 DTO·REF에 epoch/source 정체성·JSON Pointer정본경로**가부족. UDS/UART요구는`/payload/...`인데 Opus는 typed rawpayload와`data`map으로분리해경로불일치. R3에서하나를선택하고 fixture고정. |
| test-domains §3 timeout | 조건대기 timeout과장치무응답분리방향 | timeout event seq+**스캔한창/완전성**을 REF로제공. timeout발생자체는“DUT가정말발생하지않음”의충분한증거가아님. |
| ui-test 전부 | test snapshot/증거상태·사용자수정·STOP원칙 | M1테스트화면요구와 core P2전체세션기능사이선행의존조정. `pause/resume/skip/retry/ask/answer/plan.show` RPC·비동기 askdeadline·역할표가누락. user조작은자동 PASS의원인판정에서별도, `end{abort}`와사용자 STOP의 finally차이명시. |
| ui-panel-template | 셸/도메인내용소유분리일치 | test snapshot의최근증거 200행/상한·데이터원천·쓰기가능 role·지연수용을 C26명세에묶기. CORE가 xterm/viz렌더를구현할필요는없음. |

### O1–O9에서 먼저 보강할 판정 규칙

1. **O3/O4의 predicate는아무 JSON이아니라등록된 step.expect의 id/hash에결속**해야한다. “RPM≥2900”계획에 RPM1000인 device이벤트를인용하며`exists(device)`를제출하면현재명시조건은 PASS가능하다(`[설계 반례]` JSON `unbound_predicate`). 계획변경은 revision·사용자검토·승인무효화로별도처리한다. Sol R1에도이결속이명료하지않다.
2. **O4 coverage분류를 O3의모든술어에닫히게정의**한다. `hold`·`order`·`measure`·“N ms안에”는단순존재가아니다. `count≥N`은부분구간만으로도긍정증명가능하지만 exact/max/absence는완전성이필요하다. 단위·clock quality·reset/generation·user개입에대한규칙도함께둔다.
3. **O5의 device이벤트≥1을일괄필수로두지않는다.** 완전한창에서 CAN ID가 0건임을확인한정당한 absence PASS는 matched device event가 0건이다. 반대로 device이벤트 1건이있어도위 1번의약한술어로계획이검증되지는않는다. 긍정관측/부재 coverage/측정/사용자 assertion에맞는 witness타입을명시한다.
4. O4의 query/pin/GC는경합한다. 조회시**임시 pin lease**를얻고검증결과 commit과정식 pin을결속, 실패하면임시 pin을해제한다. 부재증거 digest는일치이벤트가없어도동일해지지않게**plan predicate·window·filter·source coverage·DB/정책버전·gap상태**를포함한다.
5. O7의승인 max_uses는송신전에원자 예약한다(F6). O9의“전부수용”은**반영완료·추가 CR·미해결**로구분하고, TC-C5/C7도 test-domains와함께추적한다.

### P0 첫 주 범위의 현실성 `[추정]`

Opus의 C00을 1–2일에공개하는방향은맞다. 그러나 C04가**실제허브+정책 sim**을전제로하고 C05가그뒤면 P1 C10/C14를 P0에다시만드는의존이생긴다. 동시에 6crate·echo전체·TS/Python생성·3OS·3도메인 PR까지한주완료는작업 견적근거가없다. Sol도 Rust↔Python전부를첫 주필수로둬같은과장이있다.

추천은 Day1–2 `loom-proto`타입/오류/SourceRef·StreamCursor·fixture와 trait **compile-only** 공개, Day3–5 순수 fixture FakeWorker·echo·JSON Schema/TS 생성·UI용 fixture replay endpoint다. FakeGateway가완제품 hub/policy를재현하거나여러도메인 real backend를구현하지않게한다. Python은공통 wire golden fixture를고정하고어댑터의실제완성은 P4선행스파이크로분리한다. 필수 P0/P0후반/P1을한표로동결한다.

### soak·CI 관문을 실제 판정 가능하게 만드는 수정

- §9.5의 20k fps Trace는§0.3의화면예산 2k/s와같이돌면**의도적표시생략**이생긴다. 동시에“정상 viewer display.gap0”을요구하면설계대로동작해도실패다. `unexplained_transport_loss=0`, 계획된축약/화면생략은카운터정합성·정본무손실로판정한다.
- RSS는 PID/producer_epoch별수명구간으로나누고워밍업·fault/restart·GC·TC종료후안정창을명시한다. 재시작으로낮아진 RSS를한 24 h직선에섞어누수를숨기지않는다. handle/FD도 restart별 baseline으로센다. UI탭힙관문도별도로둔다.
- `ready≤5 s`는정상단일크래시복구관문, 의도적 crash-loop는 backoff최대 30 s/quarantine관문으로분리한다. UART재 open기본 None과 soak flood재개를맞출 sim profile이필요하다.
- 성능 reference OS/CPU/RAM/전원모드·스토리지·빌드/allocator·실제 브라우저버전·표본수·누락/제외수를고정한다. p95회귀+20%는 baseline이거의 0일때불안정하므로절대허용폭도둔다. fault window는명시한주입 id와유한복구구간만제외한다.
- core Rust구독자시험과 UI실제 브라우저시험을**둘다 M1합격의존**으로연결한다. “웹대용 Rust구독자”만 green이어도 4요소 M1완료로보고하지않는다. 24 h는양 OS실행전까지계획관문이다.

## 4. 결정 비교

| 쟁점 | Opus 안 | Sol 안 | 추천 | 이유 |
|---|---|---|---|---|
| 프로세스구조 | W1–W5, UDSgateway·선택 dedicated runtime, UART전체 1worker | DLL/블로킹·CPU/예산판정, UDSdedicated thread·지터실패시 worker이동 | **Opus판정표+Sol타이밍 fallback** | 도메인과부하경계를명확히하며작은순수 Rust모듈은 gateway허용. UART포트수별실예산필수. |
| worker전송·레인 | interprocess/직접 Tokio2물리소켓 | interprocess기본·2물리연결 | **공통채택** | IPC HOL분리. ctl안의긴 Call/Input과 STOP까지독립되는것은아니므로 mailbox/우선처리추가. |
| 직렬화·frame cap | ctl map256 KiB, data tuple1 MiB·credit4 MiB | 기본 map/bin·frame64 KiB·credit512 KiB | **v1 map/bin64 KiB, chunking** | Python·schema진화우선. 1 MiB필요시기능별별도 상한으로협상; tuple은형식 hash/고정 layout버전 golden후에만. 원시 PDU큰것은 chunk/raw_ref. |
| 기록위치(can W3) | worker내저널 library, gateway자체 event별도 | 동일 | **그대로** | gateway/구독 credit이원본수신을막지않음. |
| 저널형식·sync | MessagePack LJ1+SQLite, 원본 sync1 s·auditwrite후 group100 ms | JSONL+raw+SQLite, sync100 ms·요청형 intentdurable | **M1 D7유지·sync프로필명시** | binary는 D7 CR후. 기본 written/durable구분; 고위험 mutation durable intent와일반 group정책을명시선택. Sol의모든요청 fsync도성능실증전무조건확정하지않기. |
| UI고속형식 | Sol/UI측정위임, JSON기본·M2열형식 | JSON기본·협상 binary column·seq delta/meta | **Sol/UI초안상세채택** | 실제 WS크기/CPU방향근거가있고실제 브라우저검증한계명시. 고정 64 B실험 codec은제품으로복사하지않음. |
| JSON-RPC | axum자체얇은라우터, jsonrpsee불일치추정 | axum제한 dispatcher·공식오류/notification검증, jsonrpsee미검증 | **조건부자체 dispatcher** | jsonrpsee가불가능하다는확인없음. auth/snapshot/frame검증기능을분리하고 batch지원/거절의프로토콜범위를명시한다. |
| schema생성 | loom-proto정본, loom-schema exporter, schemars→TS/Python | loom-schema타입 crate, json-schema-to-typescript16/Python0.91·draft검증 | **Opus명칭분리+Sol도구 pin/golden** | `loom-schema`가타입인지생성기인지혼동제거. OpusU64/Bytes단일 serde표현원리채택, parser는 F13수정. |
| 메모리수치 | gateway256/512·ring64·64연결×32sub; UART64/128 | gateway256/384·ring32·8연결/전체 32sub·전체 worker1 GiB admission | **Sol의전체 cap체계** | 값자체는미측정. 타입별현재 bytes·shared backing·task/캐시·전체 worker수까지합산해재산정. |
| 24 h soak | RSS≤1 MiB/h+peak범위 32/16 MiB, TC10분반복·fault | ≤0.5 MiB/h+초/말중앙값증가 max16 MiB/5%, 3 Mbaud·100k burst | **혼합후 reference별잠정값** | OpusTC/fault패턴+Sol3 Mbaud/큰 burst·quota산정. PID별 stationary창/고수위/전체 RSS를함께판정, 측정전숫자를보장하지않음. |
| crate/P0 | 6stub+echo·구체 trait·동작 FakeGateway | 가벼운 schema/api/sdk·fixture gateway·작업경계 | **Opustrait, Sol식단계축소** | 첫 주앞부분 compile surface를즉시해제. 제품 hub/policy완성을가짜구현의전제로두지않기. |
| CLI기동 | 동기 tungstenite·작은 loom/loomd분리·warm30/60 ms | TokioWSclient·warm50 ms/cold500 ms | **Opus동기 CLI·30/60 ms잠정목표** | 작은단발 CLI에 runtime불필요. cold는 Opus1–1.5 s목표로시작해실측. 어느쪽도 CLI기동시험미실행. |
| 배포 | loom+loomd(worker재실행)2파일, seedkey32선택·DLL미동봉 | worker bundle/동일 exe옵션·native3OS·cargo-dist후보 | **Opus2파일+Solrelease체계** | 같은빌드 worker·CLI크기분리장점. vendor재배포권리일반화안하기. |
| 증거검증 | O1–O9상세·단계창/예산·PASS device필수 | source ref/coverage·domain verifier·user assertion분리 | **Opus상세흐름+Sol source ref**, O5수정 | plan AST결속·부재 witness·원자승인·전송전 pin은양쪽보강. |
| 재부착·orphan | 30 s orphan, M1/M2범위충돌, Job kill-on-close미사용 | 60 s orphan·worker Job핸들유지제안, M1재부착 | **Opus단순 Job수명선호, 범위명문화** | worker자체 deadline/latch·singleton lock·nonce확인. M1재부착을뺄경우 W6미충족·기록단절을명시하고리드결정, 조용히수용 완료라쓰지않음. |

## 5. 합의 가능한 부분

- C1–C8 우선, Rust코어/장치경로·선택 Python COM은별도 worker·CANoeP4·VN5640/PCAN FD자체진단우선.
- 사용자에게단일 loomd, DLL/타이밍/블로킹경로 worker격리, UDS순수상태기계는 gateway허용·seed-key별도 worker.
- worker2물리 lane·MessagePack bin·byte credit·bounded mirror, 기록은 worker직접, source.gap/record.gap/display.gap분리.
- core쓰기게이트와도메인 3훅, job단위 worker가드, 사용자입력 lease의빠른경로, STOP은출력 OFF가아님·user입력허용·사용자 STOP후 finally자동실행 0.
- 순번 2층·snapshot+live·같은스트림의 CLI/UI·필터 seq불연속은손실아님. publish와렌더는구분해야함.
- JSON제어·고속 data만협상 binary·HTTP대용량·UI정적파일임베드, Rust→Schema→TS/Python·고정 generator·drift CI.
- natural-language해석은 agent/스킬, 코어는세션·bounded AST검증·보고데이터·선택결정적 runner, 임의 eval/자동 mutation재시도없음.
- M1실제웹최소패널+사용자개입, 3OS sim CI와 Windows/Linux실 HW·24 h는분리된관문.

## 6. 자기 초안 수정 사항 (R1 파일은 수정하지 않음)

| 내 초안의 약점 | R3에 반영할 수정 |
|---|---|
| §7의“최신 zstd0.13.4”와수집기첫 stable선택 | **최고 SemVer0.14.3/최근게시 0.13.4/실측 lock0.13.4**를구분. Opus버전이맞다. 수집기에 default/max_stable/newest·yanked·선택기준저장. |
| API이름·타입소유가간략하고 loom-schema가타입 crate | Opus의`loom-proto`/`loom-domain-api`/exporter분리와구체 CommandSpec/WriteHooks/EvidenceVerifier를기본으로한다. stub는 compilefixture부터. |
| JSON/BinaryDTO변환방향만설명 | Opus의`Serializer::is_human_readable()`단일 newtype표현스파이크를채택, u64범위·bytes invalid corpus·Schema동등성추가. F13견본 panic은수정전복사금지. |
| CLI에 TokioWSclient, cold500 ms·warm50 ms를단일목표로제시 | 단발 CLI는동기 tungstenite우선비교, OS별 warm30/60 ms·cold1–1.5 s잠정선으로완화. 실제프로세스기동·백신·fsync포함측정전둘다추정. |
| 테스트동시 session16·active step32/run, 현재 TEST최대 1/설정 4와불일치 | TEST TC-C9의 1/최대 4·500steps·64REF를따르고 bytes/TTL추가. Opus의 10분 TC반복·종료후 RSS검사를 soak에추가. |
| §15.3이존재/창/술어검증만강조 | plan.expect hash결속·부재/hold coverage·user개입·원자승인예약·검증중 pin·판정수정감사규칙을추가. Opus O1–O9의장점을받되 F3/F6/O5문제도같이고친다. |
| 모든요청형 intent fsync를기본처럼기술 | 감사무기한/순서요구와**전원차단내구프로필**을구분. write-confirmed/group100 ms와고위험 durable-intent프로필의 CLI/장치지연을같은환경에서비교하고리드가선택. |
| P0첫 주 Rust↔Python실제교차·생성기완성·stub gateway까지필수화 | 공통 wire fixture/schema는 P0에고정, Python실행 SDK의완성·모든 minor호환은선택 adapter선행관문으로분리. 첫날동작 FakeGateway를약속하지않음. |
| 내스파이크의짧은 100k/s·고정열 14,416 B가제품근거처럼읽힐수있음 | paced3 s·축약 Event·Rust receiver·높은합성압축률임을모든비교표에동반. 실제 binary full schema/JS/Windows·Linux/최대처리량은미검증. 초기 WS정상 viewer손실사례도그대로유지. |
| 내 UI계약/§9.2도구독 32개큐외전체 shared backing/admission계산은불완전 | 전체 바이트 ledger·리듀서/Fixed/승인/캐시/task/trace상한·화면 drop후 snapshot stale를추가. 내낮은메모리숫자도제품실측값은아님. |

## 7. 리드에게 남은 사실 확인 질문

추가 사용자질문이아니라 R3에서발주할짧은확인작업이다. 기본통합방향은위표대로진행가능하다.

1. **P0 contract fixture:** 등록 step.expect와다른약한 predicate·hold중 gap·정당한 absence/device0건·동시 once2요청·mapping유실을정확히어떤결과로거절할지 golden JSON으로확정한다. 최우선은기대조건 결속/F3/F6이다.
2. **scope 결정:** M1재부착(W6)·M1최소 test marker와 UI-test요구·D7binary정본 CR·A6즉시 check exit3·emergency OFF예외를한결정표에고정한다. 확정전“수용 완료”로표시하지않는다.
3. **full schema+4 client:** 가변 DLC/FD64 B·메타/스키마/hash보존 binary를넣고역직렬화·허브/리듀서·JSON각 role본·실제 브라우저까지처리량/CPU/RSS/지연을측정한다. ctl장문 Call중 STOP도주입한다.
4. **Windows/Linux 메모리·수명:** Job commit/RSS·cgroup권한유무·RLIMITfallback·orphan/재부착·자식 process정리를가짜 DLL worker로실행한다. macOS결과로대체하지않는다.
5. **P0 schema/toolchain:** Rust버전·serde/schemars·draft·TS/Python생성기·U64/Bytes·nullable/enum/tuple진화를고정하고일회 compile/golden/drift를실행한다. notify8.2.2와 zstd0.14.3/MSRV도실제 선택 lock에서확인한다.
6. **기동/내구성:** 동일원본·동일 sync정책으로동기 CLI왕복+audit intent+segment/SQLite를양 OS측정. Rust macOS sync_data는 F_FULLFSYNC이므로 SQLite fullfsync=OFF와동등비교하지않는다([R1검증](../../../spikes/core/README.md)).

검증자료: [R2 공식 API·반례 설명](../../../spikes/core/r2-evidence/README.md), [반례 결과 JSON](../../../spikes/core/r2-evidence/design-counterexamples.json), [R1 실측](../../../spikes/core/README.md). R2의 Rust 보조 probe는디렉터리조회 EINTR/45 s timeout으로완료하지못했으며통과값을인용하지않았다. 리뷰는소스·공식 API·실행된산술반례와기존 R1측정의범위안에서작성했다.
