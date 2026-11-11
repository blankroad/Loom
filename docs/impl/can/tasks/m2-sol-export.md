# can-sol M2a — `crates/loom-can-export` (BLF/ASC export, K12)

공통 규칙 `docs/impl/can/tasks/p0-common.md`(질문 금지, 결정은 보고서, workspace 멤버 검사: `cargo fmt --all --check`, `cargo clippy -p loom-can-export --all-targets -- -D warnings`, `cargo test -p loom-can-export`, 커밋 안 함). **범위:** 새 crate `crates/loom-can-export/`와 보고서 `docs/impl/can/reports/m2-export.md`만(`crates/loom-can/`은 건드리지 않음 — 나중에 리드가 export job/RPC로 연결).

정본: CAN plan §6.3(v2.0), D68, `v2-sol.md` §2.3(autors 후보, 상한, streaming만).
1. 입력: `ExportFrame{ts_mono_ns, wall_ns?, channel_index, dir, frame: CanFrame, origin, device_ts?}` iterator(+ 선택 gap 표시) — 저널 연결은 나중.
2. **BLF**: `autors-blf`(버전 확인) **streaming writer만**(`BlfFile::parse_reader`/`write()` 금지), 상한 명시(header 64 KiB·container 8 MiB·object 1 MiB·writer 256 KiB), CC/FD/remote/error 객체, 방향·채널, 정확한 시간 단위, seek 가능한 임시 파일 → finish 후 원자 rename, 실패 시 `.partial`. **ASC**: `autors-asc` 또는 자체 line writer(CANoe 형식: `date`·`base hex timestamps absolute`, CC/FD 줄 형식) — 선택 이유 기록.
3. **sidecar**(JSONL): origin·request_id·원본 seq·gap·불확실 시각 — 표준 포맷에 없는 정보.
4. 시험: 자기 writer → **독립 reader**(autors reader 또는 직접 쓴 최소 파서)로 왕복 + 손으로 확인한 바이트/줄 golden(헤더·객체 하나). CC 11/29·RTR·FD64/BRS/ESI·Tx/Rx·여러 채널·자정 넘김·빈 입력·대용량(10만 프레임, 메모리 상한 확인 — 10 s 이내). "자기 왕복만으로 CANoe 호환 통과"라고 쓰지 않는다 — 외부 도구 확인은 미확인으로 남김.
끝나면 보고서를 쓰고 멈춘다.
