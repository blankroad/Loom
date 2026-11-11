# can-sol M2a export 구현 보고 (2026-11-11)

`crates/loom-can-export/` 구현 완료. 정본은 CORE 저널이며 이 crate는 iterator를 받는 파생 파일 변환기다. CAN worker·gateway·CLI·공유 타입·루트 manifest는 수정하지 않았다. 커밋하지 않았다. **CANoe/PCAN-View 외부 호환 관문은 미확인**이다.

## 구현과 API

- `ExportFrame`: host monotonic ns, optional wall/device ns, zero-based channel, Tx/Rx, 검증된 `CanFrame`, origin, request ID·원본 source 좌표·시간 품질.
- `export_frames`: 프레임 iterator → 파일. `export_file`: `Result<ExportRecord, Error>` iterator → 프레임·generic error·선택 gap 처리. 입력 이력을 수집하지 않는다.
- `Exporter<W: Write + Seek, S: Write>`: `new/push/summary/finish`. 오류 이후 push/finish 거부. `Summary`에는 개수·최초/최종 원본 좌표·최대 행 크기만 보관한다.
- BLF: `autors-blf = 0.1.1`의 streaming `BlfWriter`만 사용. CC11/29·RTR 요청 DLC·FD64/BRS/ESI·방향·복수 채널·generic CAN_ERROR. 종료 시 upstream finish 후 SYSTEMTIME end 필드만 갱신한다.
- ASC: 자체 bounded line writer. `date`·`base hex timestamps absolute`·trigger block·CC/FD/RTR/error 줄과 정수 ns 9자리. `autors-asc = 0.1.1` writer는 전체 records/String 수집과 소수점 6자리 출력이라 production에서 쓰지 않고 dev reader로만 사용했다.
- JSONL sidecar: versioned header, record/object ordinal join, origin·request_id·원본 `(producer_id, producer_epoch, source_seq)`·독립 wall/device 시각·품질·손실 3층·완료/실패 행. u64는 문자열. unknown gap count는 null이며 fake CAN object를 만들지 않는다.
- 파일: 같은 디렉터리의 `.partial` 두 개를 `create_new` → finish/flush/fsync → sidecar rename → primary atomic rename. primary가 commit marker다. `Box<ExportFailure>`로 경로·마지막 staged source 범위를 돌려주고 `.partial` 및 best-effort 실패 행을 남긴다. 기존 final/partial은 거부한다.

## 상한과 시간 결정

`bounded_blf_reader`는 header 64 KiB / compressed·decompressed container 8 MiB / object 1 MiB를 명시한다. 생성 객체는 최대 128 B다. upstream writer의 Vec 기하 증가를 고려해 flush threshold를 **128 KiB**로 낮춰 staging capacity **256 KiB 이하**로 제한했다. 실제 생성 container도 128 KiB 미만임을 대용량 파일에서 직접 검사했다.

sidecar 행 4096 B·ASC 행 512 B는 확장 전에 오류를 반환한다. request/producer label 각 128 B, reason/description 각 256 B, path 4096 B, 파일 BufWriter 각 64 KiB, object count u32 상한. 요약은 최초/최종 원본 두 개만 보관한다. upstream container 복사·선택 zlib scratch 및 파일 버퍼는 staging 256 KiB와 별도인 고정 범위 자원이다. 따라서 256 KiB를 프로세스 전체 RSS 상한으로 주장하지 않는다.

파일 시간은 `ts_mono_ns - anchor_mono_ns + wall_anchor % 1_000_000` ns. 밀리초 date 헤더의 나머지를 event timestamp에 더해 절대 ns를 보존한다. UTC 고정이며 현재 시계/로컬 시간대 추정은 없다. wall anchor가 없으면 epoch placeholder + `wall_anchor_known=false`. 개별 wall/device 값은 독립 사실로 보존하며 자동 동기·품질 승격을 하지 않는다. 내림차순·anchor 이전 시각·산술 overflow는 실패한다.

## 검증 결과

macOS sandbox, `rustc 1.93.1`, debug profile에서 **16개 시험 통과**(unit 1 + format 7 + recovery 7 + volume 1, 실패 0).

- `cargo fmt --all --check`: 통과.
- `cargo clippy -p loom-can-export --all-targets -- -D warnings`: 통과.
- `cargo test -p loom-can-export`: 통과.
- 독립 autors BLF reader로 비압축/압축 객체 모든 주요 필드 비교. ASC reader로 CC11/29/RTR/FD64·Tx/Rx·채널·payload·시간 비교. 손으로 확인한 BLF header/container/CAN_MESSAGE 48 B golden과 ASC 고정 줄을 별도로 비교했다.
- 자정 넘김·sub-ms anchor·빈 입력·FD0·u64 최대 원본 seq/device 값·unknown/estimated 시각·손실 3층·최대 JSON escape를 검증했다. upstream ASC reader의 FD0→RTR 오분류는 독립 token 검증으로 분리했다.
- 과대 header/container/inflated-size/object 선언과 절단 파일 거부, 행 버퍼 확장 거부, 잘못된 메타데이터·채널·count·시각, poisoned stream, I/O/finish 장애, 입력 실패 시 partial/source 보존, 기존 파일 보존, 변환 중 목적지 생성 시 publish 거부를 시험했다.

**실측:** iterator로 CC/FD64 혼합 100,000개를 BLF와 ASC에 각각 export하고 독립 streaming 검증. BLF export **2.612 s**, ASC **2.734 s**, 검증 포함 시험 **5.541 s**, 프로세스 경과 **6.00 s**. sidecar 최대 행 **374 B**, ASC **255 B**. 테스트는 10 s 초과 시 실패한다.

**메모리 실측:** zsh builtin `time`의 `TIMEFMT='%E elapsed, %M maxrss_kib'`로 별도 프로세스 peak RSS를 측정했다. 100,000개씩 처리 시 **3,760 KiB**, `LOOM_EXPORT_VOLUME_COUNT=1000` 비교 실행 **3,456 KiB**. 레코드 수 100배에서 +304 KiB였다. 입력/reader/sidecar 검사 모두 한 항목·한 행만 유지한다. 이 표본은 장기 soak 증명이 아니며 코드 상한·wire container 크기 검사를 함께 근거로 삼는다. `/usr/bin/time -l`은 sandbox의 `sysctl kern.clockrate` 제한으로 RSS 출력이 불가능해 zsh 측정을 사용했다.

기본 Cargo cache가 쓰기 거부되어 모든 Cargo 검증에 `CARGO_HOME=/private/tmp/loom-can-export-cargo`를 붙였다. 기존 캐시는 읽고 새 의존성은 전용 임시 캐시에 받았다. 전역 설정·설치는 변경하지 않았다.

## 의존성·남은 것·리드 결정 요청

- crates.io 조회와 소스로 최신 버전 0.1.1 확인: [autors-blf](https://docs.rs/crate/autors-blf/0.1.1), [stream.rs](https://docs.rs/crate/autors-blf/0.1.1/source/src/stream.rs), [autors-asc](https://docs.rs/crate/autors-asc/0.1.1/source/src/lib.rs). BLF streaming writer와 독립 reader, ASC dev reader 용도이며 Apache-2.0이다. JSONL은 기존 workspace serde/serde_json을 사용한다. Python·authored unsafe·whole-file BLF API는 없다.
- 공유 Cargo.lock에 자동 생성된 export 의존성 14개 block만 제거해 HEAD와 일치시켰다. **리드가 검토 후 Cargo.lock을 재생성·커밋해야 한다.** 새 crate는 기존 `crates/*` workspace glob에 포함된다.
- 리드가 sealed journal adapter·별도 export job/RPC·대상 경로 직렬화·clock segment anchor를 연결한다. worker RX/TX 경로에서 변환하지 않는다.
- 두 파일 rename은 단일 트랜잭션이 아니다. 중간 crash의 orphan sidecar는 primary 부재로 미완료 판정한다. directory fsync에 의한 전원 장애 내구성과 cross-process atomic no-clobber는 제공하지 않는다. torn partial은 정본에서 재생성한다.
- generic error의 상세/방향은 sidecar, FD의 미측정 wire timing은 ASC 0 placeholder다. 외부 golden·CANoe/PCAN-View import·Windows/Linux 실행은 미확인. 내부 왕복/golden 통과를 CANoe 호환 통과로 쓰지 않는다. TRC/MF4·import·unknown-object 통계는 이번 export 범위 밖이다.

상세 계약·예제·복구 규칙은 `crates/loom-can-export/README.md`에 정리했다.
