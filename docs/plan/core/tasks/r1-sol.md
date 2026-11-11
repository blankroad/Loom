docs/plan/02-core-principle.md 준수

# CORE R1 — core-sol 지시

먼저 `docs/plan/core/tasks/r1-common.md`를 읽고 그대로 따른다. 이 파일은 당신 몫의 관점과 추가 산출물이다.

## 관점 (15절 전체를 쓰되 아래에 무게)
- **crate·라이브러리 조사 (실제 문서·소스·crates.io 확인, 버전·라이선스·최근 릴리스·다운로드·Windows 지원)**: 
  - 런타임/WS/HTTP: tokio, axum vs tokio-tungstenite vs fastwebsockets, rust-embed(정적 웹 임베드)
  - 직렬화: serde_json, simd-json, postcard, bincode 2, rmp-serde(MessagePack), rkyv, prost/protobuf, Cap'n Proto, FlatBuffers — worker 프로토콜용과 UI 바이너리 프레임용 각각 추천
  - 로컬 IPC: interprocess(UDS + Windows named pipe), tokio named pipe, 공유 메모리 링(shared_memory, raw mmap, iceoryx2) — 이식성·복잡도·지연
  - JSON-RPC: jsonrpsee 등 vs 자체 구현
  - 스키마 생성: schemars, ts-rs, specta, typify, datamodel-code-generator(Python) 등 Rust→JSON Schema→TS/Python 경로
  - 저널: 자체 세그먼트 파일, rusqlite(WAL), redb, Arrow/Parquet(arrow-rs, parquet), zstd/lz4 압축, fsync 비용
  - 감독·자원 제한: Windows Job Object(windows-rs/win32job), Linux prlimit/cgroup v2, process-wrap 등
  - CLI: clap, 기동 시간 영향(바이너리 크기, 정적 링크)
  - 관측: tracing, 메모리 측정(jemalloc/mimalloc, dhat), Windows/Linux RSS 측정법
  - 빌드·배포: cargo workspace, cargo-dist, GitHub Actions 3 OS, cross 컴파일 여부
- **실현 가능성·타이밍/성능**: 홉별 지연 예산의 근거 수치, 처리량 상한(CAN FD 8 Mbit/s 100% 부하 프레임/s 계산, UART 3 Mbaud 폭주), 메모리 예산 근거.
- **테스트·시뮬레이션**: HW 없는 CI, sim worker 견본, 지연 측정 하네스(장치 이벤트 → UI 수신 타임스탬프), 24 h soak 설계(부하 패턴·측정·관문 수치), 크래시 격리 시험(worker kill -9, 패닉, 벤더 DLL 크래시 모사), fault injection(디스크 가득, 느린 구독자).

## 스파이크 (`spikes/core/`, 각각 짧게, 결과는 JSON/MD로 남기고 초안에 인용)
우선순위 순으로, 시간이 부족하면 위에서부터:
1. **worker 프로토콜 왕복 지연**: 로컬 소켓(macOS UDS) 위 길이 접두 + 바이너리 직렬화(postcard 또는 bincode) 요청→응답 p50/p99, 그리고 100k 이벤트/s 단방향 스트림 처리량·CPU.
2. **tokio WS 처리량**: axum(또는 tungstenite) WS로 이벤트 배치 송신 — JSON 배치 vs 바이너리 배치, 클라이언트 1개·4개, 이벤트/s·지연 p99·서버 RSS. 느린 구독자 1개가 다른 구독자를 막지 않는지.
3. **저널 쓰기 속도**: 자체 append 세그먼트(+zstd 청크) vs rusqlite WAL 배치 insert — 이벤트/s, fsync 주기별 비용, 파일 크기.
4. (여유 있으면) `loom` CLI 왕복: 최소 clap 바이너리 기동 + WS 연결 + JSON-RPC 1회 왕복 시간.
측정 환경(macOS, CPU, 빌드 프로파일)을 반드시 기록. macOS 수치가 Windows/Linux에 그대로 옮겨지지 않음을 표시.

## 추가 산출물
1. `docs/plan/core/r1-sol.md` — 15절 전체 초안 (주 산출물)
2. `spikes/core/README.md` — 스파이크 목록, 실행법, 결과 요약
3. `docs/plan/_xteam/core-ui.md` — **초안 v0.1 (CORE 작성)**: UI ↔ 코어 데이터 계약 — 연결·인증(hello/role/쿠키), 구독(snapshot+after_seq, 적응형 배치), **고속 데이터 전송 형식**(JSON 텍스트 프레임 vs 바이너리 프레임/열 단위 배치 — 스파이크 수치로 근거), 질의(HTTP GET 대용량), 쓰기·승인·STOP RPC, gap/LAG 표시용 필드, 스키마→TS 타입 생성 방식. 파일 맨 아래에 빈 `## ui 의견` 절. 이미 파일이 있으면 덮어쓰지 말고 `## core 의견` 절로 덧붙인다.
