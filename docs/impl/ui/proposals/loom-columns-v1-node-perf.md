# loom.columns.v1 Node 성능 메모

2026-11-11, UI 소비자 제안 1. [원본 25회 표본/환경 JSON](loom-columns-v1-node-perf.json), [형식 제안](loom-columns-v1.md).

**Node에서 측정했다. 브라우저·화면 렌더·장비→사용자 지연 수치가 아니다.** Apple M1 arm64, 8 logical CPU, RAM16 GiB, macOS, Node v25.6.1. 최종 원본 측정 시각은2026-11-11T10:21:05.173Z(UTC)이다.

| 10,000행 처리 경로 | median | p95 | min–max |
|---|---:|---:|---:|
| decode + 마지막 CAN ID getter | 41.79 ms | 128.88 ms | 14.72–206.68 ms |
| decode + 전체 D3 materialize | 186.62 ms | 357.36 ms | 105.11–419.91 ms |

50프레임×200행, CAN FD64 B, D3 필수 envelope(session/source identity/stream cursor/timestamp/origin)와 channel/client_id/dir/quality를 포함했다. 고정 metadata를 반복하는 합성 입력이다. 프레임당34,069 B, 총1,703,450 B(170.345 B/행)이다. 이 수치를 과거 축약 스파이크의24.2 B/행과 같은 스키마의 비교로 해석하지 않는다.

각 모드 warmup5회 뒤25회씩 번갈아 측정했다. 매회10k행 전체를 처리하며 p95는 정렬한25개 표본의24번째 값이다. encoding은 측정 밖에서 한 번 수행한다. 두 모드 모두64 KiB/정렬/offset/비트맵/u64/seq/CAN/JSON/확장 예산 검증과 metadata cache 할당을 포함한다. getter 경로는 full LoomEvent 행 객체를 만들지 않고, materialize 경로는 새 D3 객체·u64 decimal string·hex/base64·독립 metadata 복사를 추가한다.

GC와 호스트 부하를 격리하지 않아 표본 범위가 넓다. 메모리/RSS 측정이나 성능 PASS 임계값은 없다. 결론은 이 입력에서 행 객체 펼치기 비용이 추가로 관측되었다는 범위에 한한다. Rust encoder/브라우저 worker/저장소·렌더 통합·고 cardinality metadata·실제 CAN/UART 부하는 아직 측정하지 않았다.

재현(실제 경로를 workdir로 설정):

```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/ui/ui
pnpm exec tsx src/codec/columns.perf.ts
```

출력 JSON에는 모든 표본과 sink가 있다. frame/row 상한을 유지하므로10k행을 단일64 KiB 프레임으로 가장하지 않는다.
