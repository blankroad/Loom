# ETH P1 — explore-opus: `loom-eth` gateway + CLI + 스킬

먼저 `p1-common.md`, `p0-common.md`를 읽는다. 다음도 읽는다:
- `docs/plan/eth/plan.md` §3.5–3.6·§4·§5·§10·§11
- `docs/impl/eth/schema-v1.md`
- `crates/loom-echo`(gateway.rs·cli.rs)
- `crates/loom-domain-api`(registry·ctx·verify·hooks)
- `crates/loom-cli-kit`, `crates/loom-testkit`

배정 범위: `crates/loom-eth/src/gateway.rs`(하위 모듈은 `src/gateway/` 폴더로 나눠도 됨), `crates/loom-eth/src/cli.rs`(또는 `src/cli/`), 그리고 `skills/loom-eth/`. **`src/worker/`는 explore-sol 담당**이다. `src/lib.rs`·`src/model.rs`는 리드 소유다.

## 1. gateway (`Eth: DomainModule`)
등록할 것:
- **명령**: schema v1 RPC 표 전부 — `eth.ifaces`, `eth.capture.start/stop/status`, `eth.query`, `eth.show`, `eth.wait_for`, `eth.stats`, `eth.follow`, `eth.send`, `eth.export`, `eth.filter.check`.
  - `Bounds`를 명시한다: query 기본 100행·최대 2000, wait 최대 3600 s, follow 최대 64 KiB.
  - 쓰기 클래스: `capture.start/stop`은 R0 관찰이지만 감사 대상(Request 클래스), `send`는 Request.
- **처리 위치**:
  - worker로 넘김(`ctx.worker(...)`, 채널별 인스턴스 주소 방식은 domain-api에서 확인): `ifaces`·`capture.*`·`send`(dry-run 제외).
  - gateway에서 처리:
    - `query`·`show`·`stats`·`follow`·`wait_for`: `ctx.journal().scan`(상한 있음)으로 `eth.pkt`를 읽는다. `bytes`를 `loom-eth-decode`로 디코드하고 `loom-eth-filter`로 거른다. 같은 디코더·필터를 쓰므로 CLI·UI·TEST가 같은 결과를 본다. 스트림 재조립이 필요한 follow는 디코더 `StreamTable`을 범위 안에서 다시 돌린다.
    - `send{dry_run:true}`: 프레임·페이로드를 디코드해 `{proto, info, len, risk}`를 낸다. 행·감사·worker 호출 없음.
    - `filter.check`.
  - `wait_for`: 대기 수단이 domain-api에 없으면 짧은 주기 저널 재조회로 구현한다. 상한·deadline을 지키고 CORE 요청 목록에 "도메인용 bounded wait/subscribe API"를 적는다.
- **이벤트 kind와 고정 JSON Pointer**: model.rs의 kind 6개. 포인터는 TEST 술어용으로 `/payload/proto`, `/payload/info`, `/payload/len`, `/payload/src`, `/payload/dst`, `/payload/attributed_origin`, `/payload/link/request_id` 등을 둔다.
- **risk table + hooks.classify**(plan §10, F1이라 등급은 표시만):
  - 조회·캡처: R0
  - send → sim·replay·loopback: R1
  - send → 실 NIC: R2
  - 브로드캐스트·멀티캐스트: R3
  - **raw DoIP 진단(0x8001) 프레임**: 안의 UDS SID로 등급을 매긴다. 표는 UDS 계획의 서비스 등급을 따르고 근거 주석을 단다. 예: 0x11·0x10 R2, 0x2E·0x31·0x27 R3, 0x34–0x37 R4.
  - hooks 규칙: 동기, < 1 ms, I/O 없음.
  - `presend_check`: 채널 generation 확인.
- **reducer**: 채널별 `capture_state` + 최근 `eth.pkt` 요약 200행(late viewer snapshot).
- **evidence_verifier** (TEST §3.1 · 계약 07):
  - 도메인 술어: `eth.match{filter}`, `eth.count{filter, min, max}`, `eth.absent{filter}`, `eth.period{filter, period_ms, tol_ms}`.
  - **전수 평가**(범위 전체 재조회)로 한다.
  - `attributed_origin` 패킷은 관측 증거에서 제외한다.
  - 창 안에 `source.gap`·`record.gap`이 있으면 부재·개수·주기는 uncertain이다.
  - 예산은 `EvalBudget`을 지킨다.
- **display_summarizer**: Activity 한 줄. **panel_manifest**: `eth.packets`(+ `eth.stream`·`eth.stats` 자리)로, UI 형식을 따른다. **help**(C8): 요약·예시·주의(BPF와 `-Y` 구분, 출력 상한).
- **UDS 링크(`eth.link`)**: UDS 쪽 `wire_key`가 아직 없으므로 이번에는 만들지 않는다. 대신 `docs/impl/requests/`용 요청 문구를 보고서에 적는다.

## 2. CLI (`DomainCli`, plan §4)
- 하위 명령:
  - 캡처: `ifaces`, `capture start <iface> [--ch] [--backend live|replay|sim] [--replay FILE --speed max|N --repeat] [--bpf] [--snaplen] [--promisc] [--buffer MiB]`, `capture stop|status`
  - 조회: `trace [<ch>] [-Y FILTER] [--since CUR] [--max N] [--follow --timeout T]`, `last [-n]`, `show <REF> [--hex]`
  - 대기·통계: `wait-for -Y F [--count N[..M]] [--within T --anchor REF] --timeout T`, `stats --by …`, `follow tcp|udp <stream>`
  - 송신·기타: `send raw|udp|tcp …`(`--dry-run`), `export pcapng -o FILE`, `filter check EXPR`
- **사람용 렌더**는 tshark 열을 따른다: `seq time source destination proto len info org`.
  - 끝줄: `(N rows · source.gap N · display a/b · next: --since seq:X)`.
  - `show`는 계층 트리 + `--hex` 덤프(선택 필드 범위 표시는 선택).
- `--follow`는 cli-kit에 구독이 없으면 커서 기반 반복 조회로 구현한다. 최대 600 s이고 상한이 있다.
- `--json`은 cli-kit 봉투를 그대로 쓴다.
- CLI 바이너리는 장치 crate를 링크하지 않는다(불변식 I1). CLI 조각은 decode·filter(순수 라이브러리)만 써도 되고, capture는 쓰지 않는다.

## 3. 스킬 `skills/loom-eth/SKILL.md` (60줄 이하, C8)
- 내용: tshark 대응표, 표시 필터 요약(Wireshark 부분집합·`!=` 의미·비지원 문법), BPF와 `-Y` 차이, 출력 상한·커서, 대기 명령으로 기다리기(sleep 금지), 증거 REF는 결과에서 복사만, 자기 송신은 증거가 아님, "TC 단계로 쓸 때" 절.
- 명령 상세는 `loom eth help --json`이 정본이다.

## 시험
- `RegistryProbe`로 등록 전체(명령·kind·포인터·risk·worker spec·help) 검증
- `HookHarness`로 classify 표·시간 상한
- 가짜 저널(testkit fixture 또는 직접 만든 `JournalReader`)로 query·show·wait_for·evidence(전수·gap·attributed 제외)
- CLI 파싱과 렌더 golden

## 보고
`docs/impl/eth/reports/p1-opus.md`(60줄 이하, CORE 요청·UI 영향 목록 포함). 커밋하지 않는다.
