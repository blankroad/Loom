# can-opus P0 — `crates/loom-can-dbc` (DBC 파서 + codec)

먼저 `docs/impl/can/tasks/p0-common.md`를 읽고 따른다. 범위: **`crates/loom-can-dbc/` 폴더만.** 보고서: `docs/impl/can/reports/p0-dbc.md`.

## 목표
plan D67: `can-dbc` 10.0.0(MIT/Apache) 파서로 AST를 얻고, CAN 소유 **불변 compiled DB(IR) + runtime codec**을 만든다. 사용 중 DB 교체(IG·CLI)를 해야 하므로 codegen 방식은 쓰지 않는다. 나중에 gateway CAN 모듈(G1/K6)의 디코더·SignalCache·IG 인코드 미리보기·증거 신호 pointer(§9.5 `#/signals/<name>`)가 이 crate를 쓴다.

## 요구
1. 의존: `loom-can-frame = { path = "../loom-can-frame" }`, `can-dbc = "10"`(최신 10.x 확인). 그 밖은 꼭 필요한 것만.
2. **로드**: `Database::load(bytes, LoadOptions)` → `(Database, Vec<Warning>)`. 인코딩은 명시 옵션(UTF-8 기본, Latin-1/CP1252 선택); 자동 추측을 하면 결과에 표시. **상한 [목표, plan §7]**: 파일 16 MiB, 메시지 50k, 신호 500k, 문자열 64 KiB, mux 깊이 16 — 넘으면 명시 오류(`LimitExceeded{what, limit}`). DB의 `sha256`(파일 바이트)를 보관(세션 고정용 `db_sha256`).
3. **IR**: 메시지(id 11/29bit, 이름, 길이(DLC 0..64, FD 여부는 속성 `VFrameFormat`·길이로 판단), 송신 노드, `GenMsgCycleTime`), 신호(시작 비트·길이 1..64, Intel/Motorola, signed/unsigned, IEEE float32/double(`SIG_VALTYPE_`), factor/offset, min/max, 단위, 값 테이블(`VAL_`), 초기값(`GenSigStartValue` 속성, raw 기준), mux 역할(멀티플렉서/피멀티플렉스 값, 확장 `SG_MUL_VAL_` 범위·중첩)). 조회: id→메시지, 이름→메시지/신호.
4. **decode**(`&CanFrame` → `DecodedMessage`): 신호별 `raw`(i64/u64), `physical`(f64), enum 라벨(있으면), 활성 mux 분기만. 프레임 길이가 DB보다 짧으면 해당 신호는 `Unavailable`(패닉 금지), 길이 불일치는 경고 표시.
5. **encode**(메시지 + 신호 값 + 기준 → `CanFrame`): 기준값 규칙(plan D17): 기본 = DBC 초기값, 초기값이 없는 신호는 **반드시 값을 줘야** 함(누락은 오류 `MissingSignal`); 기준을 "이전 페이로드"로 주는 API도 둔다(`--base last`용, 호출자가 바이트를 줌). 물리값 → raw 변환(반올림 규칙 명시), **범위 검사**(DBC min/max 밖 = `OutOfRange` 오류, `force_range` 옵션은 호출자 책임), enum 라벨로도 설정 가능, 비활성 mux 분기 신호에 값을 주면 오류, 멀티플렉서 값 자동 설정. FD 길이는 `fd_padded_len` 규칙.
6. **E2E 보호 추정 헬퍼**(plan §10.2 B4 이후): `is_e2e_suspect(message) -> Option<Reason>` — 신호·메시지 이름 패턴(CRC, Chk, Checksum, Counter, Alive)과 DB 속성으로 "추정". 차단 판단은 상위(정책) 책임, 여기선 판정 근거만.
7. **미지원 문법**: `can-dbc`가 못 읽는 부분·쓰지 않는 섹션은 위치(줄)와 원문 요약을 `Warning`으로. 쓰기 결과에 영향을 주는 항목(예: 신호 레이아웃을 알 수 없음)은 그 메시지 encode를 **fail closed**(오류).
8. **C2**: 모든 컬렉션은 로드 시 상한 확인 후 고정. decode/encode는 할당을 최소로(신호 수만큼).

## 시험 (필수, Python·cantools 금지 — 기대값은 손계산·사양으로)
- 직접 쓴 DBC fixture(`tests/fixtures/*.dbc`) 여러 개: Intel/Motorola 경계(바이트 넘는 신호, 1비트, 64비트), signed 음수, float32/double, factor/offset 소수, 값 테이블, 기본 mux, 확장 mux(`SG_MUL_VAL_` 범위·중첩), FD 64바이트 메시지, 29bit id, `GenMsgCycleTime`·`GenSigStartValue` 속성.
- encode→decode 왕복(무작위 값은 `proptest` 허용)과 **손계산 기대 바이트** 표(Motorola sawtooth 비트 번호 포함).
- 오류: 범위 밖, 누락 신호, 비활성 mux, 너무 큰 파일(상한), 잘못된 DBC, 짧은 프레임 decode.
- 실제 공개 DBC가 필요하면 라이선스가 명확한 것만(예: opendbc MIT) 작은 일부를 fixture로, 출처를 fixture 머리에 적는다.

## 하지 않는 것
gateway 연동·SignalCache·시계열·RPC(나중 K6/G2), ARXML, 신호 그룹 의미, CRC/카운터 계산(E2E 플러그인은 후순위).
