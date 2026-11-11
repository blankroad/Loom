# P1c 보고 — explore-opus: `--keep-open`, export 주석·Loom 메타 보존 (2026-11-11)

**완료.** eth worktree, 커밋 안 함(c8a752d·9dff39e 기준). 관문 4개 green(fmt · clippy workspace · clippy eth live · test workspace, 3회 연속 실패 0). 새 시험 3개(gateway 2 + cli 1).

## 바꾼 것
- CLI `send tcp … --keep-open`(tcp만, 아니면 `E_USAGE`), `--from-port`는 udp만 검사. `keep_open`은 켰을 때만 params에 넣는다.
- gateway `eth.send`: 같은 검사를 서버에서도(`check_send_options`, dry-run·실송신 공통). schema에 `keep_open`. tcp+keep_open dry-run은 기존과 같은 키 `{proto, info, len, risk, reason, dry_run}`.
- export: `gateway/export.rs`(`to_packet`)로 분리. 결과에 `comments_dropped` 추가.
- 스킬: `--keep-open` 한 줄 + "자기 연결만, 남의 TCP 스트림 주입 없음".

## 결정 (export 주석 형식)
1. **X10 `loom1 seq=… dir=… org=… [req=…]`**: dir과 `attributed_origin`이 있는 패킷(자기 송신)만. seq = **stream seq**(REF 좌표). req는 `link.request_id`가 X10 규칙(비어 있지 않음·공백 없음·≤ 512 B)일 때만.
2. **장치 패킷에는 X10을 쓰지 않는다**: X10 org는 agent|user|script뿐(pcapng crate가 다른 값을 attribution으로 읽지 않음). 주석 없음 = 장치 관측.
3. **원래 X10(`bytes.loom_comment`) → `loom1-orig <원래 본문>`**(예: `loom1-orig seq=12 dir=tx org=agent req=q-7`). 접두가 `loom1 `이 아니라 다시 읽을 때 attribution이 되지 않고 사용자 주석으로 남는다. 원래 좌표임이 접두로 드러난다.
4. 순서: `loom1-orig`가 먼저(주석이 많아도 살아남게), 그다음 `bytes.comments` 원문 그대로. 사용자 주석이 X10처럼 파싱되면 그것도 `loom1-orig`로 내린다(crate가 "reserved loom1 comment"로 거부하므로).
5. 상한: `MAX_COMMENTS`(64, meta 포함)·`MAX_COMMENT_BYTES`(4 KiB) 넘는 주석은 버리고 `comments_dropped`로 센다(export 전체를 실패시키지 않음).
- 시험: export → `PcapNgReader`로 다시 읽어 장치/자기 송신/replay 3종의 meta·주석 왕복 확인.

## CORE·리드 요청
- 없음. (pcapng X10에 `org=device`가 필요하면 crate 소유자(sol)·리드 결정 — 지금은 불필요로 판단.)
