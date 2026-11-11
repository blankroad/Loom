# CORE R1 독립 초안 — Sol

작성: 2026-11-10 · 상태: **계획 초안, 구현 착수 아님**. `02-core-principle.md` 최우선, 마스터 **C > B > A > D** 적용. 상대 CORE 워커 초안은 읽지 않았다. `[확인]`은 저장소 계약·공식 자료·직접 실행 결과, `[추정]`은 설계 선택·목표·용량 산정이며 제품 보장이 아니다.

## 0. 핵심 루프

| 요소 | 코어 부품·경로 | M1에서 보이는 증거 |
|---|---|---|
| 직접 접근 | `loom` → 인증된 gateway → Registry → 장치 단독 소유 worker. UI/CLI는 장치를 다시 열지 않는다 | sim UART/CAN/DoIP 장치가 열리고 같은 session/channel 표시 |
| 읽기 | worker 원본 seq → worker 내 저널 → 화면 미러 → 허브 stream seq → 경계 있는 CLI 질의와 웹 구독 | 같은 원본 ID·바이트를 CLI와 UI에서 대조 |
| 쓰기 | 연결 자격 → 도메인 분류 → 정책·승인·감사 → presend → worker 최종 가드 | 요청자 출처·쓰기 상태·실제 송신 바이트가 UI에 점진 표시 |
| 실시간 표시·개입 | 같은 허브의 웹 패널, 사용자 입력 lease, STOP·승인·복구 송신 우선 경로 | 늦게 연 UI도 snapshot+live, 사용자 Ctrl-C/직접 송신이 agent 결과의 `others_since_last`에 나타남 |

**종단 간 시나리오:** agent가 `loom uart send dut 'reboot\n' --json`(예시 바이트, 실제 줄끝은 UART 프로필)을 실행한다. `requested(origin=agent)`가 웹 Activity에 뜨고 UART worker가 보낸 바이트와 sim bootshell의 `boot ok` RX가 같은 터미널·Trace에 나타난다. 사용자는 패널에서 `Ctrl-C`를 누르거나 `status\n`을 입력한다. 입력권이 user로 넘어가며 대기 agent 송신은 exit 6, 다음 agent 결과에는 사용자 개입 범위가 붙는다. `loom stop-all`은 agent/script와 모든 job을 latch하지만 사용자 키 입력은 계속 통한다. R2 시나리오는 승인 대기 행에서 시작하며 승인 대기 시간은 장치→화면 SLO에 포함하지 않는다.

| 홉 | 정상 부하 예산 `[추정]` | 큐·상한 / 초과 시 |
|---|---:|---|
| CLI 실행·WS 인증·분류·감사 후 TX 제출 | warm p95 50 ms 이내, 장치 대기 별도 | RPC 128개/전체 2 MiB, 채널 TX 32개/256 KiB; 거절 |
| worker 장치 읽기 → 원본 이벤트 | 5 ms | 수신 4 MiB; 장치 수신 중단 대신 원본 손실 표시 |
| 저널 enqueue·worker→gateway | 10 ms | 저널 16 MiB, 화면 미러 4 MiB; 화면만 drop 가능 |
| 허브 순번·상태 갱신 | 2 ms | 입력 8 MiB, 최근 링 32 MiB 고정 |
| 구독 적응형 배치·WS 전송 | UART 10 ms + 전송 5 ms | 구독 데이터 512 KiB, 연결 제어 256 KiB |
| 브라우저 decode·렌더 완료 | 33 ms | UI Worker/패널 버퍼는 UI가 고정, 넘치면 gap/축약 |
| 합계 | RX host ingest→화면 65 ms 예산, **UART/UDS p95 ≤100 ms**, CAN ≤200 ms | p99 ≤200/200/500 ms 목표; 홉 분위수의 합을 실측 분위수로 간주하지 않음 |

물리 장치 내부 응답시간·USB 드라이버 지연·OS 스케줄링은 별도 분포로 보고한다. CAN ISO-TP FC는 이 루프를 왕복하지 않고 CAN worker 전용 스레드에서 처리한다. 고부하에서는 UI를 20 Hz로 축약하되 gap·축약 수·오래된 미렌더 이벤트 나이를 표시한다. 장치가 조용한 idle을 LAG로 표시하지 않는다.

**P0 첫 산출물**은 제품 데몬보다 먼저 제공하는 스키마·인터페이스 stub crate와 fixture다(§3.6, §11). **M1 = 시뮬레이션 위 걷는 뼈대**, 웹 UI를 포함한 4요소와 사용자 개입이 모두 통과해야 한다(C6).

## 1. 범위 / 비범위

CORE 소유: Rust gateway·CLI 공통부, worker SDK·감독, 이벤트 허브·순번·snapshot, worker에 링크하는 공통 저널 라이브러리와 조회 인덱스, Registry·정책·승인·감사, 잡·러너·테스트 세션·증거 검증, 스키마 생성, 성능 하네스·배포 골격.

UART 팀은 `loom-console`(링·expect·InputArbiter·sanitizer)과 UART backend·risk 훅, CAN 팀은 DLL 바인딩·ISO-TP hot path·CAN 질의·SignalCache, UDS 팀은 세션·P2/P2*·가드·진단 DB를 소유한다. UI 팀이 셸·패널 프레임워크·viz를 소유한다. TEST는 TC 표현·해석 규칙·보고서 내용·스킬을 소유한다.

후순위: 공유 메모리·zero-copy, Arrow Flight, 원격 LAN 직접 bind, 동적 Rust ABI 플러그인, Tauri 셸, Parquet 정본, 다중 호스트 시계 동기, 화려한 TUI. 모두 핵심 루프의 현재 병목을 증명하기 전에는 도입하지 않는다. 이번 라운드는 `spikes/core/` 안의 검증 코드와 문서만 작성한다.

## 2. 사용 시나리오

| 관점 | 구체 행동 | 성공 조건 |
|---|---|---|
| agent 1 | `loom uart open dut --backend sim`, send/read/expect | 숨은 장치 연결 없이 공통 세션; 출력 64 KiB 이내·커서 제공 |
| agent 2 | `loom can trace ch0 --tail 100 --json`, PCAN FD 프레임 1회 송신 | 사용자 Trace에 TX 출처·ID·바이트가 보이고 wire 확인은 별도 |
| agent 3 | `loom test start boot-smoke`, UART 부팅·CAN 신호·UDS DTC 단계 실행 | 각 단계에 실제 이벤트 인용, 증거 없는 PASS 거부 |
| user 1 | agent 실행 중 `loom ui`로 참가 | 1초 내 상태 snapshot과 이후 live, 별칭·세션 유지 |
| user 2 | CAN flood 중 STOP, UART Ctrl-C, 채널 take-control | agent/job만 정지; 사용자 복구 경로·취소 불가 TX 수 표시 |
| user 3 | UDS DID 쓰기 승인 전 before 값·요청 바이트 검토 후 거부 | 승인 대상 digest·epoch 검증, agent는 exit 5, 장치 TX 0 |
| 공통 | 뷰어 1개가 멈춘 동안 3개는 계속 관측 | 느린 연결만 gap/해제, 원본 저널 수 불변 |

VN5640·PCAN-USB FD가 우선이고 진단은 자체 Rust 스택이 기본(C4/C5). CANoe 측정 여부로 자동 경로를 고르던 B3 기본값은 C4가 대체했다. Saleae는 캡처 중 파형을 Loom으로 스트리밍할 수 있다고 쓰지 않는다. 상태는 live, 파형은 캡처 후 artifact로 연결한다.

## 3. 아키텍처

### 3.1 프로세스 선택

| 대안 | 장점 | 한계 | 추천 |
|---|---|---|---|
| gateway 안 모든 Rust 도메인 | 복사·IPC 최소 | DLL·OOM·CPU 정지가 모든 화면에 전파 | 기각 |
| gateway + 장치 owner worker | 장애·예산·스케줄링 분리, 공통 UI | 감독·재부착 필요 | **기본** |
| 기능마다 별도 프로세스 | CPU 격리 세밀 | 배포·메모리·홉 증가 | export/무거운 decode만 측정 후 |

```text
agent loom ─JSON-RPC/WS─┐              ┌─ 웹 UI (viewer)
                       ▼              ▼
loomd: 인증 → Registry → 정책/감사 → 허브/상태/stream mapping
       UDS 세션 런타임    │               │
       bounded 질의 풀   │ control/data  └→ 구독별 WS writer
                         ▼ (물리 연결 2개)
UART worker    CAN worker(ISO-TP/주기 전용 OS 스레드)   DLL/COM worker
장치 RX ─┬→ 공통 저널 writer thread → 세그먼트/인덱스
         └→ bounded 화면 미러 → gateway
```

DLL·FFI·Python·외부 코드가 있으면 worker 필수. 지속 CPU >1 core의 20% 또는 메모리 독립 상한이 필요하거나 FC/주기 deadline이 있으면 worker로 분리한다(`[추정]` 초기 판정선). 순수 Rust의 작고 상한 있는 상태기계만 gateway crate 허용. UDS는 D1 유지하되 전용 Tokio runtime thread 1개와 최대 대상 32개를 둔다. Tokio task에는 일반적인 우선순위 보장이 없으므로 "우선 task"라는 이름으로 타이밍을 보장하지 않는다. seed-key DLL·32bit DLL은 별도 helper worker, 기본 deadline 2 s. UDS 타이머 지터 p99 ≤2 ms 관문을 못 넘으면 같은 trait의 `uds-session` worker로 이동한다.

### 3.2 worker 전송·프레이밍·직렬화

| 선택지 | 이식성·지연·복잡도 | 추천 |
|---|---|---|
| stdio pipe | 3 OS, 부모 자식 단순 | 로그 혼입·재부착·제어 HOL 문제로 M0용만 |
| `interprocess` local socket | Unix UDS + Windows named pipe, Tokio 통합 | **기본**; current-user ACL·remote pipe 거부·peer 검증은 OS별 적용 |
| Tokio UDS/named pipe 직접 | 제어가 명확하고 의존 축소 | 추상화·Windows DACL을 직접 유지; 문제 시 대안 |
| shared_memory/raw mmap ring | 복사 감소 가능 | ownership·wrap·메모리 장벽·크래시 복구·Windows 핸들 보안까지 필요, P3 이후 |
| iceoryx2 | pub/sub·공유 메모리 프레임워크 | Windows tier 2 기능 제한·규모 대비 복잡성; 초기 비채택 |

`[확인]` interprocess 공식 문서는 Windows named pipe와 Unix UDS 매핑을 명시한다([문서](https://docs.rs/interprocess/latest/interprocess/local_socket/)). Tokio도 Windows named pipe를 직접 제공한다([문서](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/)). 스파이크는 **macOS std UDS**이며 interprocess/named pipe 성능을 측정한 것은 아니다.

제어와 대량 데이터는 **별도 물리 연결**로 나눠 큰 프레임 뒤에 STOP이 갇히지 않게 한다(CAN W1). worker hello에서 major/minor·schema hash·지원 codec·frame cap·producer epoch·기능을 협상한다. major 불일치는 해당 worker만 비활성, minor는 additive 필드/선택 기능 교집합으로 연결한다. 길이 `u32 LE` + 고정 헤더 `(magic, version, lane, kind, request_id, producer_epoch, payload_len)` + payload. **전체 frame ≤64 KiB**, length를 allocation 전에 검사하고 read deadline·부분 프레임 EOF·알 수 없는 tag를 시험한다. 대용량은 chunk id/offset/total/hash 또는 raw_ref, 재조립도 4 MiB/대상 상한이다.

| codec | worker/Python 관점 | UI 관점 | 추천 |
|---|---|---|---|
| serde_json | 투명·진단 쉬움, bytes 변환·필드 반복 비용 | 모든 브라우저 기본 | 제어·CLI 기본 |
| simd-json | JSON 파싱 가속 후보, 가변 입력·CPU feature 검증 필요 | 브라우저 decoder 아님 | 먼저 serde_json, 프로파일 후 |
| postcard | compact·Rust serde, wire 규격 있음 | JS 구현·enum 호환 직접 관리 | Rust 전용 실험 대안 |
| bincode 2 | 실측 가능하나 2.0.1 유지보수 종료 | 직접 decoder 필요 | 신규 채택 기각 |
| rmp-serde / MessagePack | Python 표준 생태계와 교차, bin bytes | JS decoder 추가 | **worker v1 추천**, map+명시 tag·필드명 |
| rkyv | 검증된 archived 접근 가능 | Python/JS와 레이아웃 공유 곤란 | 내부 캐시 외 후순위 |
| prost/protobuf | 다언어·필드 진화 우수 | generator·런타임 필요 | IDL 정본이 Rust 정본과 갈라져 후순위 |
| Cap'n Proto | 다언어·직접 접근 | compiler/JS·안전 검증 비용 | 후순위 |
| FlatBuffers | 열/벡터·다언어 접근 | JS 지원, IDL/toolchain 추가 | 대형 artifact의 별도 스파이크 후 |

MessagePack는 **Rust/Python 동일 프로토콜** 요구가 선택 근거다. 수치 ID enum 대신 안정된 문자열 kind와 map 키, raw bytes는 MessagePack bin(`serde_bytes`)로 고정한다. 스파이크의 compact tuple과 제품 map은 다르며 숫자를 그대로 예산에 대입하지 않는다. Rust↔Python golden corpus(최대 u64, null, Unicode, bytes, 알 수 없는 필드)를 P0에서 통과시킨 뒤 확정한다. Postcard보다 무조건 빠르다고 주장하지 않는다.

### 3.3 흐름 제어·감독

data credit은 **바이트 단위**, 연결당 미승인 512 KiB. 수신 완료 뒤 credit 반환; 제어 credit은 독립이다. credit 소진 시 장치 RX는 막지 않고 worker의 화면 미러만 버린다. 저널은 계속 쓰고 `display.gap`에 **원본 seq 범위**를 남긴다. gateway에 들어오지 않은 이벤트에는 stream seq가 없으므로 stream seq 범위를 지어내지 않는다. 저널 큐가 꽉 차면 디스크 spill은 이미 저널 경로이므로 무한 2차 spill을 만들지 않는다. 장치가 멈출 수 없으면 정확한 `record.gap` 또는 `count=unknown`, health failed, agent/script 새 R1+ 차단이다.

heartbeat 1 s, 3회 누락 suspect, 5 s에 worker unresponsive. lease 만료 시 agent/job TX를 worker 자체에서 latch한다. STOP 요청은 control lane으로 보내 각 worker의 **latch 적용 ACK**를 모은다(p99 <10 ms 목표); timeout 100 ms면 부분 STOP/미확인 worker·드라이버 접수 수를 보이고 성공이라 하지 않는다. 사용자 긴급 개입을 기록·승인 대기 뒤에 세우지 않는다.

재시작 250 ms →500 ms→1→2→4→최대30 s, ±20% jitter; 60 s 내 5회 크래시면 circuit open, 사용자 명시 재시작 필요. 재연결은 RX·상태만 복구하고 주기·replay·진행 mutation은 재전송하지 않는다. pending은 `outcome_unknown`, producer_epoch/generation 변경을 발행한다. gateway 소유 runtime lock과 worker lock/nonce를 통해 살아 있는 worker에 재부착하며 재오픈을 막는다.

**감독 수명과 재부착의 선택:** process-wrap/Job의 kill-on-parent-close와 "gateway 사망 후 worker 계속 기록"은 동시에 성립하지 않는다. M1은 worker가 bounded orphan 모드(기본 60 s)에 들어가 TX latch+저널 유지, 새 gateway가 nonce로 재부착한다. Windows Job handle은 worker도 유지하며 부모가 사라졌다고 즉시 닫히지 않게 한다. orphan deadline 후 종료. 자식까지 정리할 때는 Job handle 종료/TerminateJobObject. Linux cgroup 역시 gateway 단독 수명에 묶어 즉시 kill하는 설정을 피한다. 이 동작은 3 OS kill 시험에서 검증 전까지 미확정이다.

### 3.4 WS·허브

| WS/JSON-RPC 대안 | 비용·제약 | 추천 |
|---|---|---|
| axum + 제한된 JSON-RPC dispatcher | HTTP static/GET/쿠키/WS 동일 서버, 프로토콜 테스트 필요 | **채택** |
| tokio-tungstenite 직접 | CLI client 좋음, HTTP 서버 조립 별도 | CLI client 채택 |
| fastwebsockets | 프레임 처리 최적화 후보 | 현재 병목 실증 전 교체 안 함 |
| jsonrpsee 전체 서버 | JSON-RPC 오류·method·subscription 제공 | custom hello/쿠키/우선 lane/binary batch 통합 확인 후 대안 |

자체 dispatcher는 제한된 JSON-RPC 2.0 subset을 정확히 구현한다. unknown method -32601, parse -32700, invalid -32600/-32602, id echo·notification 무응답·application error 범위를 공식 규격대로 시험한다. JSON-RPC batch array는 M1 거부(명시 capability), scalar id 길이·깊이·동시 호출·전체 크기 제한. jsonrpsee 기능을 다시 다 만들지 않고 복잡해지면 동일 fixture로 교체한다([규격](https://www.jsonrpc.org/specification), [jsonrpsee](https://docs.rs/jsonrpsee/latest/jsonrpsee/)).

허브는 단일 sequencer가 `stream_epoch, stream_seq`를 할당한다. `(producer_id, producer_epoch, source_seq)`는 worker 정본 식별자이며 별개다. snapshot은 H에서 provider 상태와 이후 구독 등록을 같은 sequencer turn에서 고정한 **불변 참조**로 캡처한다. 큰 snapshot 직렬화는 밖에서 하고 `snapshot_part`/`snapshot_end{H}` 뒤 live `>H`를 잇는다. pending live 1 MiB를 넘으면 해당 snapshot을 취소하고 gap+새 snapshot을 요구한다. 구독 필터로 빠진 전역 seq는 손실이 아니다.

`publish-before-reply` = 상태/저널 접수·허브 순번 부여·viewer 우선 큐 발행 후 RPC 결과를 내보내는 순서. **브라우저가 실제 그렸다는 보장은 아니다.** 느린 뷰어 ACK를 RPC 완료의 무한 전제로 삼을 수 없다. 결과에 `published_through_seq`, viewer별 `rendered_through_seq/visible`를 구분하고 UI가 실제 렌더 ACK를 보고한다. 이 의미 정밀화는 §15 변경 요청이다.

제어 이벤트도 유한 큐다. 정상 상태에서 drop 금지이며 넘치면 **연결을 해제하고 snapshot 재동기화**, emergency STOP mailbox 1개는 별도 보존한다. 제어 우선 전송 중에도 request별 causal watermark를 지켜 sent가 requested보다 먼저 보이지 않게 한다. 단일 WS/TCP에는 이미 전송 중인 데이터 뒤 HOL이 남으므로 64 KiB 프레임·send timeout 100 ms·연결별 writer로 제한한다. 물리적으로 정지한 클라이언트에 제어 도착을 보장한다고 쓰지 않는다.

### 3.5 UI 고속 형식

기본 JSON 텍스트 제어/이벤트는 유지한다. 협상한 고속 구독만 `loom.columns.v1` 바이너리 프레임 후보를 쓴다. 헤더는 magic/version, sub_id, batch_id, epoch, count, first/last seq, 발행 mono/wall, encoding/flags/길이. 본문은 CAN id/u64 ts/dlc/flags/offsets/raw bytes 열 + 공통 메타 dictionary. **필터 후 seq가 불연속이면 delta/명시 seq 열이 필수**이며 first+count로 추정하지 않는다. 정수는 LE·정렬/길이를 명세하고 JS `BigInt`→화면 변환 경계를 고정한다. 제어·쓰기 최종 결과는 binary 대량 프레임에만 싣지 않는다.

batch는 최초 idle 이벤트 즉시, 터미널 상한10 ms, 일반 상한40 ms, count≤200·wire≤64 KiB 중 먼저 도달한 조건으로 flush. Fixed는 `coalesce_by:id` opt-in, `coalesced_count`·min/max와 최신값의 차이를 표시한다. 축약은 화면에만 적용하며 CLI 원문·정본은 그대로다. 실제 스파이크 수치·한계는 [스파이크 보고서](../../../spikes/core/README.md), UI 협의안은 [core-ui](../_xteam/core-ui.md)를 따른다.

### 3.6 P0 안정 API·crate 경계 (계획)

```text
crates/loom-schema        serde 타입/오류/커서/worker wire DTO, 가벼운 의존
crates/loom-core-api      Registry, DomainHooks, SnapshotProvider,
                         EvidenceVerifier, JournalReader, WorkerClient trait
crates/loom-worker-sdk    bounded 채널, framing, control latch, heartbeat
crates/loom-journal       세그먼트/복구/인덱스/조회, worker 내 writer 사용
crates/loom-core          hub/policy/audit/supervisor/jobs/test/dispatcher
bins/loomd, bins/loom     조립·daemon, clap CLI (필요 시 workspace 위치 조정)
crates/loom-sim-worker    fixture UART/CAN/PDU 견본; 실제 도메인 sim 대체 아님
```

첫 주 앞부분에 `EventRecord/SourceRef/StreamCursor/Timestamp/Gap/WriteState/ApprovalBinding/WorkerHello/CliEnvelope/TestCitation` 타입과 golden JSON/MessagePack fixture를 배포한다. API 예: `DomainHooks::{classify,approval_binding,presend_check}`, `SnapshotProvider::capture(at)`, `EvidenceVerifier::verify(pred, events, completeness)`, `Registry::register_domain`, `JournalReader::scan(query, byte_limit)`. 동기 훅은 무I/O·≤1 ms 예산, 무거운 해석은 제한된 질의 풀에서 수행한다. stub은 `E_NOT_IMPLEMENTED` 또는 fixture 응답만 돌려주고 가짜 장치 성공을 만들지 않는다. UI용 fixture replay stub gateway는 P0 후반에 제공한다. 제품 구현은 이 초안 승인·리드 통합 후다.

## 4. CLI / API 명세 초안

| 명령/RPC | 경계·출력 |
|---|---|
| `loom daemon start/stop/status/logs` | runtime lock, version/token 발견, timeout 유한 |
| `loom ui`, `core.subscribe`, `core.time` | bootstrap, snapshot+after_seq, 시계 offset 샘플 |
| `loom stop-all` / `core.stop_all` | latch ACK별 결과·미확인 worker·출력 유지 배지 |
| `loom core events --since ... --tail 100 --json` | ≤1000행/64 KiB, 다음 커서·원본 참조 |
| `loom run plan/start/wait/status/pause/resume/abort` | wait timeout 필수, 쓰기 자동 재시도 없음 |
| `loom test start/step/attach-evidence/verdict/end` | test id·step marker·verified/inconclusive |
| `loom <domain> help --json` | 인자·결과 schema ref·risk/write class·오류·예시·상한 |

`--json` 봉투는 D5의 `{schema_version,ok,cmd,data,meta,warnings,others_since_last,error}`를 유지하며 `meta`에 range/truncated/next_cursor/display_gap/record_health/env/visibility를 둔다. 모든 명령의 `others_since_last`는 `(LOOM_CLIENT, channel)` 커서 이후 타 origin 이벤트 범위다. 읽기 cursor TTL24 h·전체 4096개, 만료는 명시한다. idempotency 결과도 TTL24 h·총64 MiB로 제한하며 만료/부재일 때 **재송신**하지 않고 조회 결과 `unknown`을 준다.

종료 코드: 0 성공, 1 내부, 2 사용법/설정/DB, 3 **조건 대기 timeout**, 4 정책/레이트/기록 거부, 5 확인·승인, 6 사용자/락, 7 장치/전송/무응답, 8 부분/gap, 9 데몬, 10 UDS NRC. 즉시 `check` 불일치 exit3 제안은 A6 변경이므로 §15에서 결정 요청하고, 확정 전 exit0+`data.matched=false`를 쓴다. UDS P2 timeout은 exit7+`outcome_unknown`이며 exit3가 아니다.

커서는 epoch를 포함한 opaque 정본이며 `seq:N/mark:X/-5s/ISO8601/@last`, UART `off:N/send:last`를 해석한다. `now`를 요청 접수 시 H에 원자적으로 바인딩하는 확장으로 추천한다. HH:MM은 session 시간대/날짜를 명시하고 모호하면 거부. cursor가 보존 범위 밖이면 exit8, 필터 생략은 gap 아님.

### 기동·자동 시작 선택

| 대안 | 평가 | 추천 |
|---|---|---|
| 매 호출 daemon fork 또는 Python 런타임 | 빈번한 agent 호출에 불리 | 기각 |
| 작은 clap CLI → 기존 WS → RPC | parser/접속 비용 제한 가능 | **기본** |
| 한 바이너리 서브커맨드로 daemon/worker 모두 통합 | 배포 간단, CLI 크기 증가·DLL 의존 조심 | 배포 옵션; cold start 실측 후 |

`[추정]` warm CLI 기동+WS hello+로컬 R0 RPC p95≤50 ms/p99≤100 ms, 자동 시작 cold p95≤500 ms(장치 open/백신 제외 보고). clap derive를 작은 CLI에만 링크, 도메인별 help는 manifest로 생성해 heavy crate를 CLI에 넣지 않는다. 정적 링크가 항상 빠르다고 주장하지 않는다. 바이너리 크기·페이지 fault·Windows Defender·LTO/strip을 함께 비교한다. **이번 라운드 CLI 기동 측정은 미실행**, 후순위 스파이크다. D8대로 동시 첫 호출은 lock 하나로 기동하고 `LOOM_NO_AUTOSTART=1`을 존중한다.

## 5. 사용자 뷰어 UX

M1부터 같은 오리진 웹 UI: 상단 env/연결/기록/STOP/승인/보이는 viewer 수, 중앙 UART 터미널·CAN Trace·UDS txn, 하단 Activity·test step. CORE는 데이터를 제공하고 UI 팀이 구현한다. 요청 행은 requested→accepted→sent→confirmed/effective_verified로 갱신하고 UART는 sent가 성공 종단임을 표시한다.

뷰어 시작은 일회성 bootstrap(60 s TTL·once), HttpOnly/SameSite=Strict·세션 쿠키, token 없는 hello는 viewer로 승격하지 않는다. agent 토큰과 viewer credential은 분리한다. Host/Origin allowlist·loopback bind·쿠키 HTTP/WS 인증·CSP·민감 URL 로그 마스킹. loopback HTTP의 Secure 쿠키 동작은 브라우저별 시험하고 TLS에서는 Secure 필수. bootstrap은 URL fragment→POST 교환을 우선 제안하고 UI 초안 GET query는 no-store/referrer-policy/로그 제외·즉시 리다이렉트 조건으로만 수용한다.

TTY 유무·화면 challenge는 사람 자격의 암호학적 증명이 아니다. 동일 OS 사용자/브라우저를 악의적으로 제어하는 agent를 방어한다는 주장은 하지 않는다(D6 위협 모델). M1 브라우저 명시 승인 UX와 자격 분리로 실수·자기 승인 경로를 줄이고, 강한 경계가 필요하면 OS 사용자 확인/별도 principal을 후속 설계한다. SSH 터널은 허용하되 remote LAN listener는 M1 비범위. 터널의 정확한 Host/Origin을 설정하고 wildcard 허용은 하지 않는다.

`core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}` heartbeat. agent 결과에서 `no_viewer_attached`와 `no_visible_viewer`를 구분해 숨겨진 탭을 살아 있는 화면으로 오인하지 않는다. LIVE/LAG/STALLED/LIVE-DEGRADED·idle은 UI 계산 근거 필드를 전달한다. STOP은 출력 OFF가 아니므로 유지 상태와 safe_state 복구 버튼을 표시한다. Replay는 source namespace와 원본 origin을 유지하고 live TX 경로와 분리한다.

## 6. 데이터 / 기록 포맷

### 저널 선택 비교

| 형식 | 수집·복구·질의 | 선택 |
|---|---|---|
| JSONL+raw append segment | 순차 쓰기·언어 독립·단순 tail 복구, 인덱스 별도 | **M1 정본**, D7 유지 |
| binary segment+zstd | 부피 감소, 자체 도구·버전 비용 | 측정 후 D7 변경 승인 대상 |
| SQLite WAL event table | 트랜잭션·인덱스 쉬움, 행/페이지/WAL 증폭·checkpoint | 상태/인덱스에 채택, 원본 대량 정본 대안 |
| redb | embedded Rust KV·ACID, 바인딩 단순 | Python/외부 분석·기존 SQL 비교 불리, 후순위 |
| Arrow/Parquet | 열 조회·압축·분석 우수 | seal 후 export; 작은 실시간 append 정본 비추천 |
| 혼합 | 원본 append + 작은 SQLite catalog + Parquet export | **추천**, 저장·질의 역할 분리 |

worker가 공통 `loom-journal` writer thread를 호출한다(CAN W3). 디스크 원본 경로는 gateway·viewer credit과 분리한다. 각 worker 원본은 `(producer_id,epoch,seq)` 정본, gateway의 별도 mapping 로그가 stream seq와 source ref를 연결한다. gateway 재시작은 새 stream_epoch, mapping durable 범위 밖 과거 stream cursor는 불완전으로 표시하고 원본 인용으로 재조회한다. **worker가 알 수 없는 stream_seq를 원본 저널에 미리 쓸 수는 없다.**

64 MiB 또는 60 s segment rotate. chunk ≤256 KiB 또는 100 ms, len+CRC32C+첫/끝 source seq+개수·압축 길이·schema/codec. raw bytes는 raw chunk 파일과 이벤트 raw_ref로 연결, manifest에 hash·길이·clock·드라이버/DB/정책 버전. seal 시 zstd1 기본 후보, CPU 병목이면 lz4_flex; 압축되지 않은 active tail도 조회 가능하다. zstd 독립 청크로 손상 범위를 제한한다.

기본 flush≤10 ms, `sync_data`≤100 ms의 그룹 동기화로 `written_through_source_seq`와 `durable_through_source_seq`를 구별한다. 프로세스 kill과 전원 차단은 다르다. 일반 macOS `fsync`와 `F_FULLFSYNC`는 다르다. **사용한 Rust 1.93.1의 `File::sync_data`는 Apple에서 `fcntl(F_FULLFSYNC)`를 호출함을 [표준 라이브러리 소스](https://github.com/rust-lang/rust/blob/1.93.1/library/std/src/sys/fs/unix.rs)로 확인했다.** SQLite 기본 `fullfsync=OFF`와 동등하게 비교하면 안 된다. 이번 스파이크는 실제 전원 차단 내구성 시험이 아니다. 엄격 내구성 프로필은 매 mutation intent 감사 fsync + 원본 관련 durable barrier 비용을 측정하고 선택한다. SQLite WAL NORMAL은 commit별 내구 fsync가 없으며 FULL과 같은 내구성/성능으로 비교하지 않는다([공식 synchronous](https://sqlite.org/pragma.html#pragma_synchronous), [WAL](https://sqlite.org/wal.html)).

재시작은 마지막 active segment의 유효 CRC·길이까지 복구, 미완성 tail을 quarantine하고 recovery 이벤트를 남긴다. sealed 파일 중간 CRC 오류를 단순 tail 잘림으로 숨기지 않는다. seq/time sparse index는 4096건 또는1 s마다 `(source ref,file,offset)` 저장, 원본에서 재생성 가능. 질의는 scan bytes·time·동시성 상한을 둔다.

보존 기본 30일/전체20 GiB 중 먼저 도달한 조건. active·pinned 증거 segment·감사는 삭제하지 않는다. quota 80% 경고,90% 오래된 unpinned sealed만 제거,100%에서 기록 실패 전환. 삭제는 retention manifest로 남기고 ingest 손실인 `record.gap`과 구별한다. 감사는 무기한이므로 "전체20 GiB"와 충돌하는 예외를 메인에 명시하고 감사 별도 용량 경고·송신 중단 정책을 둔다. **보존 기간 동안 무조건 무손실·무제한 수집**이라는 약속은 하지 않는다.

대형 조회는 authenticated 같은 오리진 HTTP GET 스트리밍(64 KiB 요청 cap 유지, 응답 body는 별도 총량/페이지 제한), raw_ref는 임의 경로가 아닌 서버 소유 opaque artifact id다. `--no-record`는 payload만 생략하되 감사 metadata/hash는 유지, 증거 completeness가 false이므로 원문 일치 PASS 불가. 마스킹은 디스크·UI·CLI·export 모두 동일 규칙, VIN 등 보존 선택은 정책으로 기록한다.

## 7. 라이브러리 선택과 근거

**40개 crate의 정확한 버전·SPDX·최근 정식 릴리스·누적 다운로드·플랫폼 제한**은 [라이브러리 조사표](../../../spikes/core/library-evidence.md), 원본은 [metadata.json](../../../spikes/core/evidence/metadata.json)에 있다. 조회 2026-11-10 KST. 릴리스·다운로드는 변경 가능하며 정식 최신/스파이크 lock/제품 권장 버전을 구별한다.

| 영역 | 추천·실제 확인한 사실 | 대안/미확인 |
|---|---|---|
| runtime/HTTP | tokio1.54.1·axum0.8.9(MIT), 실제 release 빌드 | fastwebsockets0.8.1(Apache-2.0), 미측정 |
| WS client/static | tokio-tungstenite 최신0.30.0, **측정0.29.0**(axum 전이 버전과 정렬), rust-embed8.13.0(MIT) | UI bundle 빌드 후 임베드 검증 P0 |
| serde/JSON | serde1.0.229·serde_json1.0.154, 측정에 사용 | simd-json0.19.0 parser만 최적화 후보 |
| worker codec | rmp-serde1.3.1(MIT), postcard1.1.3 비교 | bincode2.0.1 측정만; 3.0.0은 의도적 compiler error 릴리스 |
| JSON-RPC | axum 제한 dispatcher 추천 | jsonrpsee0.26.1(MIT), 커스텀 구독+binary 통합 미검증 |
| schema | schemars1.2.3(MIT) → JSON Schema → json-schema-to-typescript16.0.0(TS, MIT)/datamodel-code-generator0.91.0(Python) | 생성 체인 실행은 P0 관문, 이번엔 metadata/문서 확인 |
| DB/압축 | rusqlite 최신0.41.1, **측정0.38.0**(MIT), zstd0.13.4(BSD-3-Clause) | redb4.5.0, arrow/parquet60.1.0, lz4_flex0.14.0은 후보 |
| OS/감독 | windows0.62.2(Windows API), 직접 Job/cgroup 제어 | win32job2.0.3/process-wrap8.2.2 편의 wrapper 검토 |
| CLI | clap4.6.7(MIT OR Apache-2.0) | 기동·크기 미측정 |
| 관측 | tracing0.1.44·tracing-subscriber0.3.23(MIT), OS RSS 직접 수집 | sysinfo0.40.1 MSRV1.95 불일치, 이전 compatible판 pin 또는 OS API |
| 할당자 | 기본 allocator에서 측정 | mimalloc0.1.52, tikv-jemallocator0.7.0, dhat0.3.3은 선택적 프로파일 도구 |
| 배포 | cargo workspace/lock·3 OS native CI·cargo-dist0.34.0 | 도구 전역 설치 안 함, release packaging 실행은 후속 |

`[확인]` bincode의 [최신 공식 docs.rs](https://docs.rs/bincode/latest/bincode/)는 유지보수 종료와 compiler error만 포함한 마지막 릴리스임을 명시한다. 옛 GitHub archive만 보고 결론 내리지 않고 현재 자료를 확인했다. `iceoryx2` 최신 공식 [플랫폼 표](https://github.com/eclipse-iceoryx/iceoryx2)는 Windows tier2를 명시하므로 "Windows 미지원"이라고 쓰지 않는다. `shared_memory`0.12.4(2022-03-01)는 릴리스 공백을 리스크로 둔다.

### 스키마 경로 선택

| 경로 | C3 적합성 | 추천 |
|---|---|---|
| Rust serde + schemars → Schema → TS/Python | 단일 정본·다언어 공통 검증 | **채택** |
| ts-rs12.0.1 / specta1.0.5 직접 TS | 편리하지만 JSON Schema를 경유하지 않음 | 생성 결과 비교용, 기본 정본 경로 제외 |
| typify0.8.0 | **JSON Schema → Rust**, 방향 반대 | 외부 schema import용만 |
| protobuf/FlatBuffers IDL 정본 | generator 성숙 | C3와 정본 중복, 변경 제안 없이는 비채택 |

JSON u64/i64 nanoseconds는 decimal string newtype + schema pattern으로 정본화한다. binary DTO의 숫자와 JSON DTO의 문자열 변환을 명시하고 TypeScript에서 number로 암묵 변환하지 않는다. Schema draft를 **2020-12**로 고정해 generator 호환을 P0에서 시험한다. 지원이 불완전하면 schemars에서 합의한 draft7 subset을 별도 산출하고 동등성 fixture를 검사한다. enum tag·nullable·bytes·flatten/untagged·additionalProperties·unknown field 처리까지 golden corpus로 고정한다. TS 타입만으로 런타임 검증이 끝난다고 하지 않는다.

UI Sol 초안과 맞춰 TS 생성 기본 후보는 [json-schema-to-typescript16.0.0](https://github.com/bcherny/json-schema-to-typescript)(MIT, npm 메타데이터 확인)로 정렬한다. quicktype26.0.0은 대안으로 남긴다. 실제 생성 호환성은 P0에서 검증한다.

CI는 Rust schema exporter → TS/Python generator의 고정 버전 → committed schema/types diff가0이어야 통과. hash manifest를 daemon hello·UI bundle·Python worker에 내장. additive optional 필드는 minor, 필수필드/의미/enum 파괴 변경은 major, unknown kind는 관측 가능한 unsupported로 처리한다. 도메인 payload schema namespace는 각 팀이 소유하고 CORE envelope만 독단 변경하지 않는다.

## 8. HW / OS 지원 매트릭스·배포

| 플랫폼 | 코어·실행 | 장치 경로 | 필수 검증 |
|---|---|---|---|
| Windows x64 MSVC | loom/loomd/worker, Job/DACL/named pipe | PCAN FD, VN5640 XL CAN FD/이더넷, UART, 자체 UDS; CANoe 선택 P4 | 실HW·Defender cold start·DLL 크래시·COM watchdog |
| Linux x64 GNU | 동일, UDS/cgroup v2 | SocketCAN FD/peak_usb, UART, OS TCP DoIP | udev/dialout·vcan·실PCAN FD·cgroup 권한 없는 경우 fallback |
| macOS arm64/x64 | 개발·sim, UDS 로컬 | 실제 HW 제품 지원으로 주장 안 함 | CI sim·스파이크·메모리 추세 |

VN5640 DoIP는 XL raw Ethernet만으로 OS TCP 소켓이 생긴다고 가정하지 않는다. UDS/CAN 팀의 OS NIC 노출·smoltcp·PC NIC 대안 실측을 기다리며 코어는 transport capability와 timestamp quality만 전달한다. CAN/UDS gateway 모듈은 장치의 두 번째 owner가 아니다.

"단일 바이너리"는 Rust CLI가 Python 설치 없이 실행된다는 뜻과 UI 정적 파일이 loomd에 내장된다는 뜻을 우선 확정한다. 실제 배포 bundle에는 **loom + loomd + worker 실행 파일**·필요한 vendor runtime·선택 Python adapter가 포함될 수 있다. Python COM까지 한 독립 파일이라고 약속하지 않는다. 필요 시 동일 Rust executable의 `--worker-kind`로 process 격리를 유지할 수 있지만 CLI 크기·DLL 로드 분리를 검증한 뒤 결정한다.

GitHub Actions 3 OS native build/test, Windows MSVC vendor DLL 없는 fake link/backend 시험, Linux vcan job(권한 필요한 별도 job), macOS sim. 기본 cross 컴파일은 사용하지 않는다(MSVC·C SQLite/zstd·vendor ABI·서명). `cargo-dist`는 고정 도구 버전의 CI에서 archive/installer·SHA256·SBOM·서명 절차를 조립한다. runtime DLL 재배포 권한은 vendor 조건 확인 후 bundle manifest에 기록한다. Python worker는 선택 기능별 격리 환경, 생성 타입·protocol golden test가 Rust와 같아야 한다.

| 빌드·배포 대안 | 장단점 | 추천 |
|---|---|---|
| 한 OS에서 전 플랫폼 cross | CI 비용 감소, MSVC/vendor/C 라이브러리·서명 검증 간극 | 초기 비채택 |
| 3 OS native cargo workspace + cargo-dist bundle | 각 ABI·실행·설치 검증 가능, 작업 수 증가 | **채택** |
| 모든 기능·Python을 단일 거대 executable에 통합 | 파일 수 감소, 기동/라이선스/업데이트·고장 격리 복잡 | CLI 독립 Rust 바이너리+worker bundle 우선 |

## 9. 시뮬레이션 & 테스트 전략 / 성능·메모리

### 9.1 부하 산정 (실측과 구별)

`[추정·산술]` UART 3 Mbaud 8N1 = 3,000,000/10 = **300,000 B/s**, 24 h raw 약25.92 GB(24.14 GiB). 1 B=1 event면300k events/s라는 불필요한 오버헤드가 생기므로 최대4 KiB/5 ms 청크로 읽고 텍스트 line·수치 해석은 별도 bounded 작업이다.

CAN FD 64 B payload만 계산한 절대 느슨한 상한은8 Mbit/s÷512 bit = **15,625 frames/s**. 실제는 arbitration·CRC·stuff·ACK·IFS 때문에 낮고 bitrate 하나만으로 100% frame/s를 정할 수 없다. 모델 `T≈N_nom/R_nom+N_data/R_data`: 11bit ID·BRS·64 B에서 **명목 구간30–45 bit, data구간540–680 bit 가정** 시 500k/8M은 약5.7–7.8k fps,1M/8M은 약7.7–10.3k fps. 이는 규격의 정확한 bit accounting이 아닌 준비 부하 범위다. 실제 ID/DLC/BRS/bit-stuff 패턴의 버스 analyzer bus-load와 대조해 확정한다([CiA 기본 설명](https://www.can-cia.org/can-knowledge/can-fd-the-basic-idea)). DLC 작은 프레임은 payload-only64 B 상한을 적용할 수 없다.

1채널10k fps FD + UART flood + UDS50 req/s를 첫 기준, 4 CAN채널40k fps를 제품 부하 확장 기준, **100k events/s는 여유·다채널 합성 스트레스**다. 한 CAN FD 버스가100k 64 B fps라고 쓰지 않는다. payload/codec 평균160 B면100k/s=16 MB/s·24h1.3824 TB, full JSONL이600 B면60 MB/s·5.184 TB. 디스크20 GiB가30일 원본을 보장하지 않는다.

### 9.2 모든 큐·지속 상태 상한 `[추정 초기값]`

상한은 **건수와 바이트 중 먼저** 적용. 공통 bounded wrapper가 항목별 사용량을 계측한다. OS/driver 큐도 요청값·실제값을 manifest에 기록하며 무한이라 가정하지 않는다.

| 메모리·배압 대안 | 고부하 결과 | 추천 |
|---|---|---|
| 무한 channel/Vec로 전체 이력 보유 | 순간 성능은 좋으나 장기 OOM, C2 위반 | 기각 |
| 전체 경로를 수신자 속도로 block | queue는 유한하지만 CAN/UART driver 손실·FC 지연 | 장치 수신에는 기각 |
| 원본 저널+고정 화면 링+구독별 drop·gap | 원본/화면 경로 분리, 디스크 실패는 별도 처리 필요 | **채택** |

| 이름·위치 | 상한 | 넘칠 때 정책 | 지표 |
|---|---:|---|---|
| driver/OS RX(worker) | backend별 실제값 조회, 요청4 MiB | 수신 계속, 오버런 `source.gap`, unknown 허용 | overruns/source_gap |
| RX→journal(worker) | 16 MiB/32k건 | backpressure 가능한 소스만 bounded block≤5 ms, 나머지 record.gap+failed | depth/oldest_age/record_loss |
| RX→display mirror(worker) | 4 MiB/8192건 | 오래된 화면만 drop·source ref gap, 저널 유지 | display_drop/credit |
| IPC data read/write(각 끝) | 512 KiB in-flight, frame64 KiB | credit 없으면 display drop, connection timeout | inflight/read_deadline |
| IPC control(각 끝) | 256 KiB/256건 | STOP mailbox 우선, 포화 시 unhealthy/연결 재설정 | control_depth/stop_ack |
| TX request(worker/channel) | 256 KiB/32개 | admission reject, 사용자 Ctrl-C 우선 | tx_depth/rejected |
| interactive input(worker/channel) | 16 KiB/64청크 | 키 입력 busy 표시, Ctrl-C 1개 mailbox 우선; 임의 재전송 금지 | input_rejected/lease |
| hub ingest(gateway) | 8 MiB/16k건 | 화면 원본 참조 gap, 저널 영향 없음 | hub_depth/source_refs_skipped |
| recent ring(gateway) | 전체32 MiB, channel별4 MiB | oldest eviction, 필요 시 디스크 조회 | ring_bytes/oldest_seq |
| subscriber data(gateway) | 구독512 KiB/8 batch, 최대32구독/8연결 | 그 구독만 drop+gap/축약, 연결 송신100 ms 제한 | depth/gap/coalesced |
| connection control(gateway) | 256 KiB/256건 | 정상 drop금지, 포화 연결 해제→snapshot | disconnect_reason |
| snapshot pending live(gateway/sub) | 1 MiB/2 s | snapshot 취소·재시도, 전체 동시2개 | snapshot_aborted |
| RPC admission(gateway) | 128개/2 MiB, client당16 | E_BUSY, 승인/STOP reserved slot | inflight/reject |
| UDS pending/txn cache | target별32요청/64 KiB; cache전체8 MiB/target최근100행 | busy, 완료행 eviction→저널 | p2_jitter/cache_bytes |
| blocking query/decode pool | 4 worker thread, pending16개/4 MiB | reject/HTTP429, deadline 취소 | scan_bytes/query_age |
| audit writer(gateway) | 1 MiB/1024개 | agent TX admission 중단; 사용자 입력은 stderr fallback | audit_health/durable_seq |
| policy/approval state | 256 pending/2 MiB,10분 TTL | 새 승인 거절, 만료·취소 | pending/expired |
| job/test/runner state | job128개, run16개, active step32개/run, 전체8 MiB | admission reject, 이력은 디스크 | active_jobs/steps |
| evidence/export work | 동시2개, pending8개,64 MiB cache | paging/reject, artifact disk quota | pinned_bytes/scan_limit |
| journal compress/seal(worker) | 동시1개, pending2청크·1 MiB | 압축 대기 제한·비압축 seal, 실패 표시 | compress_age |
| trace/export diagnostics | tracing channel1 MiB/1024건 | debug drop 집계, 감사와 분리 | diagnostic_dropped |
| cursor/idempotency cache | cursor4096/4 MiB; 결과64 MiB·TTL24h | TTL/LRU, expired/unknown 명시 | evictions/cache_bytes |
| UI decode/render queue | 패널별 목표4 MiB/2만행, 전체64 MiB(UI 합의) | display gap/축약, raw는 HTTP | oldest_pending/render_ack |

연결·도메인·구독 수 상한을 함께 걸어 `상한×무제한 클라이언트`로 메모리가 늘지 않게 한다. 표에 없는 새 큐는 설계 리뷰에서 추가하기 전 merge 금지. job별 내부 타이머·map에도 위 active cap과 payload 상한을 적용한다.

### 9.3 프로세스 예산·측정

| 프로세스 | soft / hard | 구성 근거·초과 동작 |
|---|---:|---|
| gateway | 256/384 MiB | ring32+idempotency64+query32+송신/스냅샷24+상태32+runtime/할당 여유72≈256; soft에서 신규 구독/질의 제한 |
| CAN worker 1개 | 96/192 MiB | journal16+mirror4+driver4+decode/cache16+runtime/여유56; DLL native 메모리 포함 |
| UART worker 1개 | 48/96 MiB | journal16+mirror4+console8+runtime20; 줄/expect 입력 상한 |
| Python COM 선택 | 192/384 MiB | Python/COM/native 예측 어려움, 실측 후 조정·별도 worker |
| export helper | 128/256 MiB | page/rowgroup 크기 제한, 기본 미기동 |

기본 bundle(게이트웨이+CAN2+UART1)의 soft 합496 MiB, hard 합864 MiB. export/Python 기동은 전체 supervisor **1 GiB admission budget** 안에서만 허용하거나 명시 프로필로 올린다. RSS cap과 committed virtual cap은 다른 지표다. Windows Job `ProcessMemoryLimit/JobMemoryLimit`는 **commit 한계**, RSS 목표는 working set/PrivateUsage를 별도 감시한다([공식 구조체](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)).

Windows는 suspended create→Job 할당→resume·자식 job·메모리 실패 알림 처리. Linux는 사용자 위임 cgroup v2가 있으면 `memory.high/max`, `memory.events`, process tree kill을 쓰고, 없으면 prlimit/RLIMIT_AS+RSS watchdog이다. RLIMIT_AS는 가상주소 한계라 RSS 한계라고 하지 않고 thread stack/mmap 여유를 둔다. macOS는 보편적인 cgroup/RSS hard cap이 없으므로 rlimit 가능한 범위+100 ms watchdog·프로세스 종료로 대응, 짧은 overshoot는 보장 불가. soft 지속10 s는 화면 캐시 축소·경고, hard 초과는 해당 worker latch→종료/재시작→gap·outcome_unknown. gateway hard 초과 전 admission을 차단하고 제어 슬롯은 남긴다.

메모리 표본 1 s: Linux `/proc/<pid>/status` VmRSS/RssAnon와 cgroup memory.current(캐시 포함 차이 표시), Windows `GetProcessMemoryInfo` WorkingSetSize/PrivateUsage, macOS task_info resident_size/phys_footprint. 이번 Rust 측정의 `getrusage.ru_maxrss`는 macOS **byte 단위 peak**이며 현재 RSS/추세가 아니다. allocator profiling은 dhat·heap counter를 별도 빌드에만, mimalloc/jemalloc 교체는 같은 workload A/B 후. 해제된 메모리의 allocator 보유·파일 캐시를 live leak와 구별한다.

### 9.4 측정 하네스·24 h 관문

장치 source ts(있으면)→host ingest→journal enqueue/written/durable→hub publish→WS receive→UI decode→requestAnimationFrame 렌더 완료를 **같은 source ref**로 연결한다. 전체 이벤트마다 추가 tracing allocation을 하지 않고1/1000 샘플과 bounded histogram을 쓴다. 브라우저 시계는 core.time 반복 RTT 중 최소 지연으로 offset·오차폭 추정, 서버 monotonic과 `performance.now()`를 직접 빼지 않는다. 측정 분포에 window·표본 수·p50/p95/p99/max·미수신/제외 수를 함께 적어 coordinated omission/생존 표본 편향을 드러낸다.

CI: fixture boot/FD/DoIP sim,3 OS 60 s 기본 smoke, Linux nightly30분, release reference Windows/Linux 각각24 h. CAN sim은 5k fps 기저+매10분60 s 20k fps·혼합DLC/ID, 매1시간60 s 100k/s 합성 worker stress. UART 3 Mbaud 상당300 kB/s를 상시, UDS20 req/s와0x78/늦은 응답·50 req/s burst,4 viewer 중1개는 주기2 s pause, attach/detach1000회. live 화면 성능은 실제 브라우저로 측정한다.

24 h는 **이번에 실행하지 않았다**. 관문 `[추정]`: warmup30분 제외, gateway/worker별 RSS 마지막30분 중앙값−첫30분 중앙값 ≤max(16 MiB, 초기의5%), 선형기울기≤0.5 MiB/h, hard cap 초과0; 정상 viewer UART/UDS p95≤100 ms/p99≤200 ms·CAN p95≤200 ms/p99≤500 ms, STOP ACK p99<10 ms; 누락/중복0(허용된 표시 drop은 gap 카운터와 정확히 일치), 원본 `record.gap/source.gap=0`(명시 fault window 제외), 승인·제어 누락0, 열린 파일/스레드 수 baseline+4 이하. allocator plateau 때문에 RSS 한 지표만으로 누수 판정하지 않는다.

디스크 산정은 실행 전 estimator가 주입량×평균정본크기×기간과 압축 **실측 하한**을 계산한다. 전구간 보존 soak은 충분한 저장소를 준비하고 기본20 GiB를 그대로 쓰지 않는다. 256 GiB reference quota/rolling 1 h retention을 쓰는 모드는 oldest unpinned seal 삭제·retention 수를 별도 집계하고 "24h 원본 전체 보존"이라고 보고하지 않는다. pin된 증거는 삭제 불가, 공간 부족 fault를 별도로 실시한다.

### 9.5 크래시·결함 주입

| 시험 | 주입 | 합격 기준 |
|---|---|---|
| worker hard kill | 자신이 띄운 sim worker SIGKILL/TerminateProcess | gateway/다른 장치 alive, pending unknown·epoch변경, 자동 TX 재개0 |
| Rust panic·vendor DLL crash | child abort/access violation 가짜 DLL | 프로세스만 사망, 저널 유효 tail 복구·서명된 오류 상태 |
| gateway kill | TX 중 부모 kill, worker heartbeat lease 만료 | worker RX/기록 orphan기간 유지, TX latch, 새 gateway 재부착·중복 open0 |
| 느린 viewer | 읽기2 s pause/저속, 별도 연결4개 | 정상3개 지연 관문, 느린쪽만 gap/해제, 정본 수 동일 |
| 디스크 full/쓰기 오류 | 임시 quota filesystem 또는 writer syscall fault hook(실루트 disk 채우지 않음) | record.health failed, agent R1+ 차단·user 입력 허용+stderr 감사 |
| audit 불능 | fsync/permission 오류 주입 | agent 새 mutation0, 승인 소모/결과 ambiguity 표시 |
| IPC·credit 고장 | EOF/잘린길이/oversize/STOP ack누락/credit0 | 메모리 cap·오류 격리, FF/FC 스레드는 정지하지 않음 |
| 타이밍·재연결 | wall jump/sleep-wake/epoch변경/late PDU | clock quality·quarantine, 과거응답으로 새 PASS 없음 |

이번 스파이크는 위 acceptance 전체를 검증한 것이 아니다. 핵심 transport/저널 선택의 초기 근거만 제공한다. 제품 M1의 인증·승인·CRC 복구·실제 브라우저 렌더 시험을 대체하지 않는다.

### 9.6 이번 라운드 실측 요약 `[확인]`

최종 실행은 `spikes/core/runs/20261109T231749Z/`, Apple M1·16 GiB·macOS27.0.1·Rust1.93.1 release다. [전체 방법·원본·반복 범위](../../../spikes/core/README.md)를 함께 읽어야 한다.

| 실험 | 직접 측정한 결과 | 설계에 주는 근거·한계 |
|---|---|---|
| UDS IPC 3반복 | MessagePack RTT p50 7.000–8.125 µs / p99 21.000–29.125 µs,100k/s 300k건 연속 수신, stream p99 0.171–0.213 ms, 양 프로세스 CPU 4.96–5.92%/1core | 2프로세스 IPC 자체의 여유; 제품 map/envelope·named pipe·Python은 제외 |
| WS 정상4개 2반복 | JSON 수신 p99 0.895–1.124 ms·최대57,519 B/batch; 고정 열 0.319–0.431 ms·14,416 B/batch, 모두300k건 | binary 방향 근거. 소형 합성 프레임이며 브라우저 렌더가 아님 |
| WS 느린1개+정상3개 | 정상3개 모두300k건, JSON p99≤1.230 ms·열≤0.364 ms; 느린 연결 timeout1회 | 연결별 writer/큐 격리 방향 확인; 실제 제어/gap 프로토콜은 미구현 |
| 별도 최초 WS 실행 | 열4개 중정상3개가각400건 누락, queue full6회 | 8배치큐로 모든 순간지터를 흡수 못 함. 원시 실패 보존, soak/정확 gap 요구 근거 |
| JSONL segment·100k건 | 200건마다 F_FULLFSYNC:46,138 events/s,2000건:368,461,10000건:882,191; 평균580.778 B/event | 빈번한 강한sync 비용,100 ms 그룹동기 후보; 실제 timer·원본CRC/인덱스 제외 |
| SQLite WAL | fullfsync=ON/FULL: batch200 41,424·2000 230,384·10000 419,790 events/s, 마지막 checkpoint별도 | fullfsync=OFF의 FULL과 동등 비교 금지, SQLite 일괄 열위라는 결론도 금지 |

WS 서버 peak RSS는 정상4개 JSON3.94–4.08 MiB/열3.48–3.52 MiB였지만, 허브·저널·정책·실제 상태가 없는 짧은 서버라 제품 메모리 예산을 이 값으로 축소하지 않는다. zstd 압축률도 규칙적 합성 payload의 낮은 엔트로피 결과여서 디스크 장기 용량비로 일반화하지 않는다. 최종 raw 결과와 소스/Cargo.lock 해시를 보존했다.

## 10. 안전 / 가드레일

| 집행 대안 | 한계·장점 | 추천 |
|---|---|---|
| 스킬/CLI 안내만 | 다른 클라이언트·job에서 우회, 승인·감사 순서 불일치 | 기각 |
| 도메인마다 정책 엔진 | 세부 분류는 적합하나 공통 STOP·승인 의미가 갈라짐 | 분류 훅만 도메인 소유 |
| core 공통 게이트+worker 최종 TX 가드 | 승인은 한 곳, 프레임 deadline은 worker에서 집행 | **채택** |

| 항목 | 선택·이유 |
|---|---|
| 쓰기 등급 | R0 자동/R1 기본자동/R2 auto·token_or_viewer·viewer 정책/R3 viewer만·R4 매번once. sim R2 auto, bench=vehicle(B4), 도메인 더 엄격한 기본 허용 |
| 차단 | default_blocked/unlock·hard deny·immutable_deny를 구분, 사용자 임의 deny는 기본 비어 있음, 승인으로 hard deny 우회 불가 |
| 요청형 | classify→deny/rate/interlock/lease→confirm/approval→**audit intent durable**→presend 재검사→backend→events→reply |
| 잡형 | start/update/renew/stop만 코어 승인·감사, worker 단위 TX에 latch/ID예약/rate/plan 재집행 |
| 대화형 | lease 획득 시 R1 정책1회, key chunk 최저지연 경로, Ctrl-C 우선, 줄/1 s 감사 묶음 |

감사 intent fsync 이후 presend 실패도 audit cancel로 남긴다. backend 송신 후 결과를 기록하기 전 죽으면 intent만 남은 요청은 `outcome_unknown`, 자동 재시도 금지. publish-before-reply와 **intent 감사 선행**은 각각 관측/side-effect 순서이므로 충돌하지 않는다. `[추정]` intent fsync는 장치 TX 지연에 영향을 주며 warm CLI50 ms 관문에서 측정해야 한다. 키 입력은 A1 예외의 묶음 감사이며 키별 fsync를 하지 않는다.

승인 binding = canonical bytes의 SHA256(plan/request bytes·대상 물리주소·epoch/generation·profile/DB/policy hash·before_seq/값 hash·lease·scope·횟수·expiry). canonical serialization을 명시하고 임의 JSON 문자열의 키 순서에 기대지 않는다. domain invalidation API 제공. 승인 사용기한≤10분, 시작 job max_duration 기본1 h 안 renew 가능(A9). 테스트 사전 승인은 대상·SID/DID·값범위·횟수가 고정된 R2/R3 pattern만, R4는 매번 viewer. 새로운 값이 범위를 벗어나면 재승인이다.

STOP(A3/A15)은 agent/script 신규 쓰기·모든 job latch, 사용자 요청형 입력 허용, 출력 상태 유지. 사용자 STOP이면 finally 자동 송신0; safe_state 복구 동작을 user 버튼으로 제시하고 interlock/deny 검사. **비상 OFF**는 A3의 별도 사용자 경로(확인 없음·인터록 경고)이며 일반 `core.recover`의 interlock 필수와 혼동하지 않는다. emergency OFF의 도메인 의미·대상을 Registry에 명시하고 아무 RPC나 이를 주장하지 못하게 한다.

정책2층(project loom.toml, user policy.toml)은 위험별 보수적으로 merge, 완화는 viewer 승인 전 effective hash를 바꾸지 않고 강화는 즉시. 파일 감시 이벤트만 믿지 않고 송신 전 effective hash/generation을 검사한다. 감사는 append canonical JSON+prev_hash/SHA256, 일별 seal root·manifest 연계. hash chain은 **탐지**, 로컬 관리자에 대한 위변조 방지가 아니다. A12대로 record/audit 불능 시 agent/script 새 R1+만 차단하고 user 키는 허용, 실패 구간 stderr와 회복 후 gap 보고.

대화형 감사 묶음 필드 제안: `{lease_id,channel,origin:user,actor_id,source_range,start_mono_ns,end_mono_ns,byte_count,sha256,redacted,line_complete,record_health}`. 원문 key나 비밀은 감사에 중복 저장하지 않고 저널 정책을 따른다. 무한 줄도1 s/4 KiB마다 묶어 메모리가 누적되지 않는다.

## 11. 마일스톤 (수용 기준)

| 단계 | 산출물 | 측정 가능한 완료 기준 |
|---|---|---|
| **M0 / P0 첫 주 앞부분** | loom-schema/core-api/worker-sdk **stub**, golden fixture·sim worker 견본 | 도메인3팀이 schema/trait 의존 compile, Rust↔Python bytes/u64 corpus 일치, schema→TS/Python drift0; backend 미구현은 명시 오류 |
| P0 후반 | fixture replay gateway stub·HTTP/WS hello·snapshot·sim DLL 없이3 OS CI | UI가 stub로 패널 개발, 64 KiB/unknown/proto mismatch·role 거절 시험 green |
| **M1 걷는 뼈대** | Rust gateway+sim UART/CAN/DoIP worker·저널·기본정책·CLI+웹3패널 | 세 도메인 각각 agent open/read/write→웹 렌더→user 개입 E2E, 늦은 합류1 s·무표시누락0, 정상 지연 §0, 60 s5k CAN+UART flood, STOP/기록실패/worker kill 시험 |
| M2 성능·HW | PCAN FD Win/Linux·실UART, schema v1 freeze·조회·승인2층·잡·세션증거 | 실제 양OS 장치↔UI 지연 보고, 느린 viewer 격리, RSS cap,24 h soak, 증거없는PASS 차단·epoch검사 |
| M3 러너/대량 | bounded 러너·TEST 보고 연계·열형식/질의·export | 혼합TC3개·STOP finally0·복구 버튼,100k 합성 burst·coalesced/gap 수 검증 |
| M4 선택 | VN5640 도메인 결과 통합·Python CANoe 별도 worker·선택 셸 | 해당 HW capability·COM crash·메모리 cap·공통 protocol corpus, Python 예외 결정 기록 |

M0는 선행 계약 준비이며 첫 사용자 기능 마일스톤 M1은 반드시 걷는 뼈대다. P0에서 제품 기능을 모두 완성하느라 도메인 팀을 기다리게 하지 않는다. 24 h 관문을 마치지 않고 "장기 안정성 완료"라고 표시하지 않는다.

## 12. 구현 단계 작업 분해

O=Opus 워커, S=Sol 워커 추천. 실제 에이전트를 새로 기동하거나 타팀에 메시지를 보내는 지시가 아니다.

| 작업 | 추천 | 의존 | 기다리는 팀·완료물 |
|---|---|---|---|
| C00 계약 DTO/오류/커서·schema exporter·fixture | O+S(schema/codec 검증) | 없음 | 전팀 P0 stub |
| C01 core-api trait/Registry·fake backend | O | C00 초안 | UART/CAN/UDS/TEST compile surface |
| C02 worker framing/2lane/credit/SDK | S | C00 | CAN W1–W7·UART worker·Python |
| C03 supervisor/lock/재부착/Job/cgroup | S | C02 | 전 장치팀 crash·메모리 budget |
| C04 axum auth/bootstrap/RPC·발견/자동기동 | O | C00 | UI stub gateway·CLI |
| C05 hub/snapshot/provider/priority/gap | S | C01/C02 | UI·도메인 gateway 모듈 |
| C06 segment/raw/index/recovery/retention | S | C00 | CAN/UART worker·증거 조회 |
| C07 hooks/승인2층/감사/STOP·입력 묶음 | O | C01/C04; C06 durable API | UDS before binding·UART lease·CAN jobs |
| C08 clap CLI 봉투/도메인 help/기동 bench | O | C04/C00 | 모든 스킬, UI bootstrap |
| C09 측정 하네스·sim fixture·3OS/kill/soak | S | C02/C05/C06 | UI 실렌더 ACK·TEST E2E |
| C10 test session/markers/citation/pin·검증훅 | O+S(조회 검증) | C05/C06/C07 | TEST 기획·3도메인 evidence |
| C11 runner/job·사전범위승인/finally | O | C07/C10 | EXPLORE 단계 어댑터·TEST |
| C12 열형식/golden JS decoder·HTTP쿼리 | S + UI 팀 | C05/스파이크 | UI/CAN Graphics |
| C13 release/static embed/CI native matrix | S | C04/UI bundle | 전팀 설치 bundle |

P0 병렬: C00 fixture 초안 공개 즉시 C01/C02/C04, C06은 SourceRef만 의존해 시작. 두 worker가 C00를 같이 검토한 뒤 O는 인증·정책, S는 IPC·저널 순으로 가며 동시 단일 파일 수정을 피한다. M1 통합은 C05/C07·도메인 sim·UI 최소패널이 교차 관문이다.

## 13. 리스크

| 리스크 | 근거·대응 |
|---|---|
| 아주 짧은 macOS 실험의 과잉 일반화 | Windows/Linux 실HW·백신·named pipe·디스크를 측정하지 않음; 각 OS 수용 기준 별도 |
| worker MessagePack와 JSON 정본 비용 | tuple/bin 합성치가 제품 map/envelope보다 작음; full schema 재측정·압축·필드 dictionary |
| 유한 제어 큐와 drop금지 모순 | 정상 drop금지+overflow disconnect/snapshot, STOP reserved mailbox; 무한큐 금지 |
| gateway crash와 worker 자동kill 충돌 | orphan lease·TX latch·Job handle 수명 별도, 부모kill 시험 필수 |
| Journal 무손실·quota·전원차단 혼동 | retention/ingest/durability 세 축 분리, full disk는 실패로 정직하게 표시 |
| UDS runtime starvation | 타이머 전용 runtime·bounded 질의, p99>2 ms면 worker 이동 |
| 승인 before 값이 stale | before_seq/value hash·lease·epoch 바인딩·presend·invalidation |
| UI 실제 렌더와 WS 수신 혼동 | 브라우저 ACK·clock error·누락 수 측정, 스파이크 수치는 수신만 |
| Rust unsafe·vendor allocation·OOM | FFI worker 격리·Job/cgroup·watchdog, Rust 자체가 메모리 상한을 보장하지 않음 |
| 라이브러리 유지보수/신MSRV | bincode 제외, compatible pin·Cargo.lock·라이선스/보안 CI |

## 14. 사용자에게 물어볼 질문 (기본값으로 진행)

현재 답변이 없어도 계획 작성은 계속한다. 보유 장비/실차 여부를 다시 묻지 않는다.

| 미결 정보 | 답 전 기본값 |
|---|---|
| 24 h 기준 Windows/Linux CPU·RAM·저장소 | 4core/16 GiB/SSD, host명·전원모드·여유공간 기록, 숫자는 해당 reference만 보장 |
| 20 GiB와30일 중 우선 기대 | 먼저 도달한 조건, pinned evidence는 보존·용량부족 명시 |
| R2 실HW 토큰 재제출 허용 범위 | 도메인 기본 유지, UI 승인 가능·CAN 실HW viewer 기본; bench와vehicle 동일 |
| 강한 사람 승인 보안 경계 필요 여부 | 동일 사용자 악성 agent 방어는 비범위, 실수 방지·사용자 통제권 목표 |
| CANoe/Vector 드라이버·Saleae 버전·모델 세부 | 사용자 확인 후 adapter capability·지원표 업데이트, 자체 Rust 진단 먼저 |

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 계약 결론·미확정 결정

| 항목 | CORE 제안·근거 | 확정 주체 |
|---|---|---|
| publish-before-reply | enqueue/publish watermark와 rendered ACK 분리; 실제 렌더 보장으로 해석하면 느린뷰어가 RPC를 막음 | 메인·UI |
| worker protocol v1 | interprocess 2물리lane + MessagePack bin/map, frame64 KiB·credit 바이트 | CORE lead·CAN/UART·UDS |
| D7 정본 | M1 JSONL+raw chunk 유지, binary 정본 변경은 full schema 실측 후 | 메인 |
| `now` cursor | 접수 시 H에 고정, UART `-0s` 대체 | CORE lead |
| 즉시 check 불일치 exit3 | UDS C-11 제안, A6 변경이므로 이번 초안에서 확정하지 않음 | 메인·TEST |
| audit quota 예외 | 감사 무기한과전체20 GiB 동시충족 불가, 별도 경고·agent TX 차단 | 메인 |
| viewer 자격 | 브라우저 명시승인·자격분리, TTY/짧은코드가 동일OS 악성agent를 막는다는 표현 기각 | 메인·UI·UART·UDS |
| core-test 초안 | 세션·source ref/stream mapping·완전성·PASS 재검증 계약 | CORE lead·TEST |

### 15.2 들어온 요구 반영 추적

`[확인]` `_xteam/can-core.md`, `uart-core.md`, `uds-core.md`, `core-ui.md`, `can-uds-isotp.md`, `canoe-bridge.md`, `viz-requirements.md`를 읽었다. 다른 팀 파일은 수정하지 않았다. 이 초안 작성 시점 미존재 요구는 최종 점검에서 다시 확인한다.

| 요청 | 반영/판단 |
|---|---|
| CAN W1–W4 | §3.2/3.3 두lane·credit·worker 저널·원본seq 수용 |
| CAN W5–W10 | STOP ACK·재부착·OS스레드·telemetry·DLL/Python 격리 수용, §3.3/§9 |
| CAN gateway/evidence | Registry/JournalReader/EvidenceVerifier·seq mapping·마커, §3.6/§6 |
| UART1–4 | 별도worker·≤10 ms·raw bytes·사용자Ctrl-C·감사묶음, §0/§3/§10 |
| UART5–8 | 자격분리/위협한계·저널조회·now 제안·loom-schema/core-api 경계 |
| UART9–11 | source/stream 인용·help schema·24h 하네스 |
| UDS C1–4 | 3훅/1ms·before digest·invalidation·job/idle callback·recover |
| UDS C5–7 | snapshot provider·core.time·cursorTTL·증거완전성·범위승인; agent dry-run도 기존 계약대로 유지 |
| UDS C8–10 | pure session gateway/runtime 유지, DLL만worker·record fail agent만·자격한계 |
| UDS C11–13 | exit3는 메인결정 요청, 공통 reconnect=link_only/새epoch/재송신0, 전용runtime p99 지터관문 |
| ISO-TP 합의 | PDU 경계·rx_start 우선·barrier coverage·deadline·source ts quality 보존, FC를 gateway로 이동하지 않음 |
| C4 이후 CANoe | B3 문서 P2/자동선택은 이 초안에 적용하지 않음, P4 선택 Python worker+같은protocol |

### 15.3 테스트 세션 코어 계약 제안

| 러너·세션 대안 | 장점·한계 | 추천 |
|---|---|---|
| agent 자유 실행만, 결과는 채팅 | 자연어 유연, 증거 존재/완전성 검증·회귀 고정 불가 | 기각 |
| core 내 LLM/TC별 임의 코드 | 실행계층과 해석 결합·새 런타임·재현성 부담 | 비범위 |
| agent 자연어 해석+공통 세션/검증+선택 선언형 runner | TC별 코딩 없이 같은 장치/UI/정책 경로, 결정적 회귀도 가능 | **채택** |

별도 [core-test 초안](../_xteam/core-test.md)에 같은 내용을 제공한다. `test.start`는 TC원문 hash·장비목록·환경·정책/DB hash·사전승인 plan을 고정한다. `test.step.begin/end`는 허브 마커를 만들고 worker journal에 control 메시지로 복제하되 물리동시 barrier를 보장하지 않으며 source 범위 fence/ACK를 기록한다.

인용 `{session_id,step_id,stream_epoch,seq_range,source_refs,predicate,expected_hash,request_id,complete}`의 존재·같은단계·시계/세대·저널hash·보존pin을 코어가 검사하고 도메인 `verify_evidence`가 실제 바이트/신호/응답을 재평가한다. `Match/Mismatch/Inconclusive`를 보존하며 `record.gap/source.gap/retention/no-record/scan_limit`가 걸린 부재 판정은 inconclusive다. display.gap만으로 디스크 증거를 무효화하지는 않는다. 수동 관찰 PASS는 user assertion 별도 타입이며 자동 검증 PASS로 위장하지 않는다.

runner는 선언형 `loom_scenario:1` 단계·timeout/반복/총step/총시간 상한, 임의eval없음, origin=script로 같은 RPC를 호출한다. natural-language 해석은 agent/TEST 스킬 몫이며 코어가 LLM을 내장할 필요가 없다. 실제 실행 기록에서 결정적 시나리오를 저장할 수 있지만 mutation 자동재시도는 하지 않는다. TEST의 보고서 문구/HTML·JUnit은 검증 결과·원본 인용을 소비하고, UI는 같은 `test.*` 이벤트를 표시한다.

### 원칙 자체 점검

- [x] 직접 접근·읽기·쓰기·사용자 실시간 표시+개입의 코어 경로와 E2E 시나리오.
- [x] 홉 예산·모든 큐 상한·고부하 손실/축약 표시·프로세스 메모리 예산.
- [x] M1 웹 걷는 뼈대·늦은 합류·P0 stub 선행·장치 owner 단일화.
- [x] 성능 실측과 제품목표 구분·미실행 CLI/브라우저/Windows/Linux/24h를 명시.
- [x] 원칙에 직접 기여하지 않는 최적화·기능은 후순위.
