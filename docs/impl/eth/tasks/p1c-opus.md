# ETH P1c — explore-opus: CLI `--keep-open`, export가 주석·Loom 메타 보존

먼저 `p1-common.md`를 읽는다(질문 창 금지 절 포함). 리드 커밋 c8a752d(sol P1b)와 그 다음 커밋(스키마 v1.2: `SendParams.keep_open`, replay `bytes.comments`/`bytes.loom_comment`)을 확인한다.

배정 범위: `crates/loom-eth/src/gateway*`, `crates/loom-eth/src/cli.rs`, `skills/loom-eth/`. worker·capture·model은 고치지 않는다.

1. **CLI**: `loom eth send tcp … --keep-open`을 추가한다(tcp에서만 허용, 다른 kind면 `E_USAGE`). `send udp --from-port`는 udp 전용으로 검사한다.
2. **gateway export**: pcapng로 내보낼 때 각 패킷의 Loom 메타를 X10 `loom1 seq=… dir=… req=… org=…` 주석으로 쓴다.
   - seq는 **stream seq**(REF와 같은 좌표).
   - 레코드에 `bytes.comments`(사용자 주석)가 있으면 그대로 EPB 주석으로 덧붙인다.
   - `bytes.loom_comment`(원래 X10)는 `loom1 orig=…`처럼 원래 좌표임을 구분해 보존한다. 형식은 스스로 정하고 보고서에 적는다.
   - 시험: export → `loom-eth-pcapng`로 다시 읽어 주석·메타 왕복.
3. **dry-run 미리보기**: `eth.send{kind:"tcp", keep_open}`에서도 기존과 같은 결과 모양을 낸다.
4. 스킬에 `--keep-open`과 "자기 연결만, 남의 스트림 주입 없음" 한 줄을 추가한다.

관문: p1-common의 4개 명령. 보고서: `/Users/ctmctm/workspace/Loom/docs/impl/eth/reports/p1c-opus.md`(25줄 이하). 커밋하지 않는다.
