# CORE R2 확인 자료

이번 라운드의 쓰기는 R2 리뷰와 `spikes/core/`에만 제한했다. R1 초안과 xteam 계약은 수정하지 않는다.

`registry-summary.json`과 crate별 JSON은 2026-11-10 공개 crates.io API를 직접 조회한 원본이다. 초기 summary의 `latest_stable` 선택은 **배열에서 첫 비 yanked 정식 릴리스**였으므로, 최종 summary에서는 `newest_stable_release_by_date`로 이름을 명확히 하고 API의 `max_stable_version/default_version/newest_version`을 분리한다. zstd는 `newest_version=0.13.4`, `max_stable_version=default_version=0.14.3`이다. Opus의 0.14.3은 존재하고 yanked가 아니므로 오류로 지적하지 않는다. R1 Sol의 최신 버전 표현을 수정할 근거다.

`design_counterexamples.py`는 제품 코드가 아닌 계획의 산술/순서/정보 손실 반례다. `r2-contract/`는 R1 `opus-repr`의 Bytes decoder를 읽기 전용으로 포함하고 잘못된 입력과 MessagePack tuple 확장 호환을 확인하는 별도 프로젝트다. 실행 결과가 확인되지 않은 항목은 리뷰에서 소스 확인/설계 반례로만 표기한다.

재현 명령(저장소 루트):

```sh
python3 -I spikes/core/r2-evidence/design_counterexamples.py
env CARGO_HOME="$PWD/spikes/core/.cargo" CARGO_TARGET_DIR="$PWD/spikes/core/r2-contract/target" cargo run --release --manifest-path spikes/core/r2-contract/Cargo.toml
```

실제 Windows/Linux, 제품 STOP/승인 동시성, 24 h soak은 수행하지 않았다.

산술/설계 반례는 Python 격리 실행(`-I`)으로 완료해 `design-counterexamples.json`에 보존했다. 일반 Python 실행은 로컬 디렉터리 조회 EINTR로 실패했으며, Rust 보조 probe는 Cargo의 작업 디렉터리 조회 오류 및 별도45 s timeout으로 완료되지 않았다. `r2-contract/results.json`이 비어 있더라도 통과 결과로 해석하지 않는다. 소스에서 확인한 문제와 실제 실행한 결과를 리뷰에서 구분했다.
