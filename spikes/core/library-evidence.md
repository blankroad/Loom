# CORE R1 라이브러리 공개 메타데이터

조회 UTC: `2026-11-09T23:07:37.829420+00:00`. crates.io 전체 다운로드 누적값이며 품질·성능 지표가 아니다. 최근값은 API `recent_downloads`를 원본 JSON에 보존했다. 최신 비 yanked 정식 릴리스와 실제 스파이크 lock 버전은 다를 수 있다.

[확인] 이름·버전·라이선스·릴리스·다운로드 = crates.io API 원본. 플랫폼 열에서 `후보`는 이식 가능성 판단이며 3 OS 실행을 뜻하지 않는다. 플랫폼을 뒷받침하는 별도 문서는 본문과 `evidence/`를 참고한다.

| crate / API | 정식판 | SPDX | 릴리스 UTC | 누적 다운로드 | Windows / 제한 |
|---|---|---|---|---:|---|
| [tokio](https://crates.io/api/v1/crates/tokio) | 1.54.1 | MIT | 2026-10-21 | 1,169,162,556 | 공식 Windows named pipe API 확인 |
| [axum](https://crates.io/api/v1/crates/axum) | 0.8.9 | MIT | 2026-04-14 | 578,465,102 | HTTP/WS 이식 가능; Windows 실행은 CI 관문 |
| [tokio-tungstenite](https://crates.io/api/v1/crates/tokio-tungstenite) | 0.30.0 | MIT | 2026-07-11 | 351,929,612 | Tokio/TCP 이식 가능; Windows 실행은 CI 관문 |
| [fastwebsockets](https://crates.io/api/v1/crates/fastwebsockets) | 0.8.1 | Apache-2.0 | 2026-01-01 | 6,239,584 | Tokio/TCP 이식 가능; Windows 실행은 CI 관문 |
| [rust-embed](https://crates.io/api/v1/crates/rust-embed) | 8.13.0 | MIT | 2026-10-07 | 66,800,256 | 빌드 시 임베드; 경로·대소문자 CI 필요 |
| [serde](https://crates.io/api/v1/crates/serde) | 1.0.229 | MIT OR Apache-2.0 | 2026-07-18 | 1,675,153,794 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [serde_json](https://crates.io/api/v1/crates/serde_json) | 1.0.154 | MIT OR Apache-2.0 | 2026-10-11 | 1,581,119,632 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [simd-json](https://crates.io/api/v1/crates/simd-json) | 0.19.0 | Apache-2.0 OR MIT | 2026-10-27 | 24,690,597 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [postcard](https://crates.io/api/v1/crates/postcard) | 1.1.3 | MIT OR Apache-2.0 | 2025-07-24 | 90,362,760 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [bincode](https://crates.io/api/v1/crates/bincode) | 3.0.0 | MIT | 2025-12-16 | 371,301,735 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [rmp-serde](https://crates.io/api/v1/crates/rmp-serde) | 1.3.1 | MIT | 2025-12-23 | 150,982,711 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [rkyv](https://crates.io/api/v1/crates/rkyv) | 0.8.18 | MIT | 2026-08-05 | 183,259,434 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [prost](https://crates.io/api/v1/crates/prost) | 0.14.4 | Apache-2.0 | 2026-06-07 | 698,225,183 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [capnp](https://crates.io/api/v1/crates/capnp) | 0.27.3 | MIT | 2026-10-27 | 16,099,256 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [flatbuffers](https://crates.io/api/v1/crates/flatbuffers) | 25.12.19 | Apache-2.0 | 2025-12-19 | 123,271,928 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [interprocess](https://crates.io/api/v1/crates/interprocess) | 2.4.8 | 0BSD OR Apache-2.0 | 2026-11-08 | 17,590,667 | 공식 Windows named pipe/Unix UDS 문서 확인 |
| [shared_memory](https://crates.io/api/v1/crates/shared_memory) | 0.12.4 | MIT OR Apache-2.0 | 2022-03-01 | 5,795,570 | Windows/Unix 구현 소스 확인 필요; 장기 릴리스 공백 |
| [iceoryx2](https://crates.io/api/v1/crates/iceoryx2) | 0.10.0 | MIT OR Apache-2.0 | 2026-09-18 | 1,009,138 | 공식 지원표 Windows tier 2, Linux tier 1 |
| [jsonrpsee](https://crates.io/api/v1/crates/jsonrpsee) | 0.26.1 | MIT | 2026-09-30 | 27,459,162 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [schemars](https://crates.io/api/v1/crates/schemars) | 1.2.3 | MIT | 2026-10-12 | 631,196,494 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [ts-rs](https://crates.io/api/v1/crates/ts-rs) | 12.0.1 | MIT | 2026-01-31 | 22,414,349 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [specta](https://crates.io/api/v1/crates/specta) | 1.0.5 | MIT | 2023-07-17 | 3,758,833 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [typify](https://crates.io/api/v1/crates/typify) | 0.8.0 | Apache-2.0 | 2026-09-09 | 34,499,090 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [rusqlite](https://crates.io/api/v1/crates/rusqlite) | 0.41.1 | MIT | 2026-10-28 | 149,532,350 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [redb](https://crates.io/api/v1/crates/redb) | 4.5.0 | MIT OR Apache-2.0 | 2026-11-05 | 15,669,605 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [arrow](https://crates.io/api/v1/crates/arrow) | 60.1.0 | Apache-2.0 | 2026-10-28 | 97,625,451 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [parquet](https://crates.io/api/v1/crates/parquet) | 60.1.0 | Apache-2.0 | 2026-10-28 | 92,577,971 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [zstd](https://crates.io/api/v1/crates/zstd) | 0.13.4 | BSD-3-Clause | 2026-11-01 | 470,048,966 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [lz4_flex](https://crates.io/api/v1/crates/lz4_flex) | 0.14.0 | MIT | 2026-07-14 | 169,985,449 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [windows](https://crates.io/api/v1/crates/windows) | 0.62.2 | MIT OR Apache-2.0 | 2025-10-06 | 400,025,391 | Windows 전용 API 바인딩 |
| [win32job](https://crates.io/api/v1/crates/win32job) | 2.0.3 | MIT OR Apache-2.0 | 2025-05-15 | 1,683,946 | Windows 전용 |
| [process-wrap](https://crates.io/api/v1/crates/process-wrap) | 8.2.2 | Apache-2.0 OR MIT | 2026-09-23 | 25,442,644 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [clap](https://crates.io/api/v1/crates/clap) | 4.6.7 | MIT OR Apache-2.0 | 2026-09-14 | 1,319,137,932 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [tracing](https://crates.io/api/v1/crates/tracing) | 0.1.44 | MIT | 2025-12-18 | 1,027,670,059 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [tracing-subscriber](https://crates.io/api/v1/crates/tracing-subscriber) | 0.3.23 | MIT | 2026-03-13 | 733,037,558 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [tikv-jemallocator](https://crates.io/api/v1/crates/tikv-jemallocator) | 0.7.0 | MIT/Apache-2.0 | 2026-05-25 | 112,698,141 | Windows MSVC 기본 선택에서 제외 |
| [mimalloc](https://crates.io/api/v1/crates/mimalloc) | 0.1.52 | MIT | 2026-05-22 | 73,540,667 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [dhat](https://crates.io/api/v1/crates/dhat) | 0.3.3 | MIT OR Apache-2.0 | 2024-02-04 | 15,392,567 | 플랫폼 중립 포맷/라이브러리 후보; Windows 런타임 검증 미실시 |
| [sysinfo](https://crates.io/api/v1/crates/sysinfo) | 0.40.1 | MIT | 2026-10-30 | 242,665,035 | Windows/Linux 지원; 최신 MSRV 1.95로 현 rustc 1.93.1과 불일치 |
| [cargo-dist](https://crates.io/api/v1/crates/cargo-dist) | 0.34.0 | MIT OR Apache-2.0 | 2026-11-02 | 189,113 | 배포 도구; Windows/Linux 네이티브 CI에서 검증 |

## 해석 주의

- bincode 최신 3.0.0은 사용 가능한 업그레이드가 아니다. [공식 docs.rs](https://docs.rs/bincode/latest/bincode/)는 유지보수 종료와 compiler error만 포함한 최종 릴리스임을 명시한다. 측정은 2.0.1 고정이며 제품 신규 채택은 기각한다.
- specta의 정식판 1.0.5와 2.x 사전판을 혼동하지 않는다. 최신 버전만 보고 타입 생성 경로를 확정하지 않는다.
- `sysinfo` 최신판의 MSRV 1.95 때문에 이 환경에서는 OS API로 메모리를 측정했다.
- `typify`는 JSON Schema → Rust 방향이며 TS/Python 생성기가 아니다. `ts-rs`·specta는 Rust → TS 직접 경로이므로 C3 기본 생성 경로로 택하지 않는다.
- Python 생성기는 `datamodel-code-generator` 0.91.0(MIT, 2026-11-07 릴리스), TS 후보 `quicktype` 26.0.0(Apache-2.0). PyPI/npm 공개 원본 JSON 보존. 전역 설치하지 않았으며 실제 생성·드리프트 검증은 P0 관문이다.

## 공개 자료 수집 오류와 복구

초기 datamodel-code-generator 구 문서 URL은 404, 구 cargo-dist 호스트는 DNS 실패였다. 샌드박스 밖 재시도에서도 동일했다. 공식 GitHub 저장소로 대체해 HTTP 200을 확인했다(`extra-sources.json`). 오류를 성공으로 처리하지 않았다.

## 원본·소스 확인 범위

crate별 API 원본 `crate-*.json`, 공식 문서 HTML/텍스트, 실제 빌드에 사용된 소스 `../.cargo/registry/src/`와 각 Cargo.lock을 보존했다. 다운로드 수가 크다는 이유로 승인하지 않으며 transitive 라이선스 검사는 구현 CI에서 별도 수행한다.
