BLOCKED: proptest dev-dependency 미등록으로 필수 all-targets clippy·전체 cargo test가 컴파일 단계에서 중단됨

2026-11-11, uart-sol. 코드 구현·독립 회귀/성능 시험까지 완료했으나 **필수 관문 전체 녹색은 아직 아니다**. 리드 소유 manifest/lock은 수정하지 않았다. 커밋·stash·질문 창 없음. `CARGO_TARGET_DIR`를 지정하지 않았고 worktree 기본 `target/`을 사용했다.

## 변경 파일·공개 API

코드는 `/Users/ctmctm/workspace/Loom/.worktrees/uart/`의 다음 배정 파일만 수정/추가했다.

- `crates/loom-uart/src/classify.rs`: 기존 `Verdict`, `Rule`, `classify_text`, `warm`, `BASE_RULES` 시그니처 유지·하위 구현 재공개.
- `crates/loom-uart/src/classify/engine.rs`: 새 `classify_text_with(profile: &str, line_state: &[u8], bytes: &[u8]) -> Verdict`, 상한 상수·프로필 규칙·편집/접두 상한 처리.
- `crates/loom-uart/tests/classify_rules.rs`, `classify_limits.rs`, `classify_perf.rs`, `classify_properties.rs`.

`generic`은 25개 정본 규칙의 보수적 합집합, 나머지 프로필은 공통 generic 8개 + 자기 규칙을 사용한다. 미지 프로필은 generic. 최고 등급을 반환하고 같은 등급이면 명시한 프로필의 규칙을 우선한다. RegexSet은 generic/linux/uboot/esp-idf/zephyr 각각 1개(25/12/12/13/12패턴), OnceLock으로 한 번 컴파일한다. `warm()`이 등록 시 전부 컴파일·검색 캐시 초기화한다. **분류 함수에는 차단/승인 코드가 없고 R4도 라벨뿐**이다(F1).

기존 6개 selector도 유지했다. `classify_text`는 기존 flash.write/flash.erase/fs.destroy/otp.fuse 및 fw_setenv→uboot.saveenv 매핑을 유지한다. 새 API는 generic.flash_write 등의 정본 id를 반환한다. 리드의 기존 `module.rs`가 BASE_RULES를 순회하므로 별도 수정 없이 정본 25개+호환 별칭 4개 모두 등록된다. 프로필별 연결 시 정책 selector의 이 차이를 반영해야 한다.

## 상수·상한·초과 정책

| 상수/대상 | 상한 | 정책 |
|---|---|---|
| MAX_CLASSIFY_BYTES | 32 KiB | line_state+bytes를 합친 **원시 접두부만** 검사, 초과하면 reason에 truncated |
| MAX_LINE_BYTES | 4 KiB/CR·LF 줄 | 원시 줄의 접두부만 검사, 나머지와 그 안의 편집 키는 CR/LF까지 무시, truncated |
| MAX_SCRATCH_BYTES | 바이트 scratch ≤96 KiB/호출 | 합친 입력·편집 뷰·보수 뷰가 각각 ≤32 KiB. 입력이 이미 단일 slice면 복사 생략. reason/RegexSet 내부 메모리와 구별 |
| PROFILES / BASE_RULES | 5프로필 / 등록 selector 29개 | 고정 소스 배열. 런타임 규칙 삽입·무한 큐 없음 |
| REGEX_SIZE_LIMIT | 512 KiB/프로필 set(근사) | 컴파일 실패를 빈 RegexSet/R1로 숨기지 않고 R4+classifier unavailable 진단. 정책 차단은 하지 않음 |
| REGEX_DFA_SIZE_LIMIT | 256 KiB/set(근사) | 라이브러리 cache 제한. 전체 프로세스 hard heap 상한이라는 의미 아님 |
| REGEX_NEST_LIMIT | 32 | 고정 규칙의 파서 깊이 제한 |

상수 옆에 plan §3.13/§7.4 또는 §10.2·P1 지시 근거를 달았다. 버퍼는 원본 TX를 변경하지 않는 분류용 데이터다. ASCII 명령은 Latin-1의 1:1 바이트 표현에 ASCII case-insensitive regex로 검사하므로 UTF-8 오류·높은 비출력 비율을 이유로 R1로 건너뛰지 않는다. 한 문자당 UTF-8 2바이트 확장도 필요 없다.

## 시험·품질 관문

환경: macOS 27.0.1 arm64, rustc 1.93.1, cargo 1.93.1. 모든 명령은 지정 worktree에서 실행했다.

| 명령 | 결과 |
|---|---|
| `cargo fmt -p loom-uart -- --check` | PASS, exit 0 |
| `cargo clippy -p loom-uart --all-targets -- -D warnings` | **BLOCKED**, exit 101: classify_properties.rs의 proptest 미등록 |
| `cargo test -p loom-uart` | **BLOCKED**, exit 101: 같은 E0433 |
| `cargo clippy -p loom-uart --lib --test cli --test registry --test classify_rules --test classify_limits --test classify_perf -- -D warnings` | PASS, exit 0. proptest 대상 제외 결과이며 전체 관문을 대신하지 않음 |
| `cargo test -p loom-uart --lib --test cli --test registry --test classify_rules --test classify_limits` | PASS: 3+4+3+10+2 = **22개**, 실HW·sleep 없음 |
| `cargo test -p loom-uart --test classify_perf -- --ignored --nocapture --test-threads=1` | PASS: test 빌드 32 KiB·5프로필×6부하·각 200회, 0.92 s |

출력 끝의 핵심 줄:

```text
error[E0433]: failed to resolve: use of unresolved module or unlinked crate `proptest`
 --> crates/loom-uart/tests/classify_properties.rs:9:5
error: cannot find macro `proptest` in this scope
error: could not compile `loom-uart` (test "classify_properties")

# proptest 외 회귀/품질 확인
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
test result: ok. 3 passed; 0 failed ... finished in 0.04s  # lib
test result: ok. 2 passed; 0 failed ... finished in 0.01s  # limits
test result: ok. 10 passed; 0 failed ... finished in 0.10s # rules
test result: ok. 4 passed; 0 failed ... finished in 0.00s  # CLI
test result: ok. 3 passed; 0 failed ... finished in 0.04s  # registry
test result: ok. 1 passed; 0 failed ... finished in 0.92s  # explicit timing probe
```

시험 내용: 등록 selector 29개 각각 양성/음성 1쌍·대소문자/탭 변형, 계획의 명령 예, 프로필 상속/미지 프로필/등급 우선, `sf er`+`ase 0 100\r`, BS/DEL/^U·지워도 다른 줄을 넘어가지 않음·편집 키 경계 우회, 4/32 KiB 경계와 reason, 줄 초과 뒤 정상 다음 줄, 높은 바이너리 비율·00–FF·입력 불변, 동시 warm/classify, 반쪽 chunk/line 한계에서의 줄 길이 oracle.

`classify_properties.rs`에는 **실제 proptest 4개(각 256 case, failure_persistence=None)**를 작성했다: 임의 바이트 panic/입력 변경 없음, 같은 미완 줄의 임의 분할 재조립 결과 동일, 바이트 prefilter와 개별 regex oracle 일치, 접두/줄 상한의 truncated 표시 일치. **현재는 컴파일·실행되지 못했으며 통과로 세지 않는다.** 의존성 등록 후 먼저 이 4개와 전체 관문을 실행해야 한다.

## 32 KiB 성능 측정

release가 아닌 기본 `cargo test` 빌드(`unoptimized + debuginfo`, debug_assertions=true). `warm()` 후 부하별 16회 예열, 200회 개별 wall 측정의 p99(198번째 표본), 단위 µs. 각 입력은 32,768 B다.

| 프로필 | regular | late_risk | many_lines | edited | edited_late_risk | long_line |
|---|---:|---:|---:|---:|---:|---:|
| generic | 85.083 | 361.125 | 25.000 | 91.125 | 447.583 | 19.458 |
| linux | 85.250 | 270.000 | 21.625 | 84.500 | 404.625 | 23.209 |
| uboot | 82.458 | 268.000 | 19.125 | 80.709 | 383.000 | 17.708 |
| esp-idf | 80.792 | 265.417 | 19.083 | 82.333 | **459.500** | 18.875 |
| zephyr | 81.583 | 266.292 | 19.084 | 82.042 | 415.167 | 25.125 |

regular=4 KiB마다 개행한 x열, late_risk=마지막 줄 끝의 sf erase, many_lines=x+개행 16,384쌍, edited=64 B마다 BS, edited_late_risk=BS와 후미 sf erase를 함께 넣은 입력, long_line=개행 없는 32 KiB(4 KiB까지만 검사하고 truncated).

최대 관측 p99 **459.500 µs <1 ms**, 이 실행의 최대 단일 표본도 468.167 µs. 이는 **위 30개 고정 부하에 대한 로컬 측정**이지 모든 입력·스케줄링의 hard wall 보증이 아니다. cold compile은 훅 예산 밖이므로 리드가 이미 하는 등록 시 warm 호출을 유지해야 한다. 타이밍 probe는 일반 병렬 시험의 잡음으로 실패하지 않도록 ignored이며 위 명령으로 별도 실행했다.

## 계획과 다르게 한 점·판단 근거

- **호환 selector를 지우지 않았다.** 신규 규칙은 `<profile>.<name>`, 기존 비프로필 이름 4개만 호환 별칭으로 유지한다. 기존 hooks/module의 공개 계약·설정 이름을 깨지 않기 위해서다. 새 profile API를 연결하면 정본 id로 정책을 사용한다.
- **generic은 합집합**: 프로필을 아직 안 넘기는 기존 hooks와 미지 프로필에서 saveenv·ESP·Zephyr 위험 명령이 사라지지 않도록 했다. 명시 프로필은 자기 규칙+공통 규칙이다.
- **편집 후 결과와 원시 편집 경계를 모두 검사**한다. `sav DEL DEL saveenv`가 최종 `ssaveenv`가 되어도 R3로 표시한다. `sf erase ... ^U echo safe`도 보수적으로 R4다. 동작을 막지는 않는다. line_state는 원시 미완 줄(편집 키 포함)이어야 동일한 보수적 결과를 재현한다. 과거 완결 줄은 호출자가 새 line_state에 넣지 않아야 한다.
- **echo 판단**: `echo "reboot later"`, `echo saveenv`, `echo sf write`는 R1. 따옴표 안 `;`는 완전한 셸 파싱 없이 구분자로 보므로 `echo "safe; reboot"` R2, `echo "safe; sf erase ..."` R4 오탐을 허용했다. 임의 셸·보드에서 의미를 보장하지 않는 실수 방지 라벨이다.
- **필요 문자 prefilter/후보 줄 축소**: 모든 고정 위험 명령이 필요로 하는 ASCII `afors` 중 하나도 없는 바이트는 규칙을 만족할 수 없다(대소문자 포함). 편집은 없던 문자를 만들 수 없으므로 이런 줄의 regex/편집 복사를 생략한다. 새 규칙 추가 시 양성 fixture와 proptest oracle로 이 불변식을 검증·필요 문자 집합을 갱신해야 한다. 줄 길이·truncated 검사는 생략하지 않는다.
- **Latin-1**: 바이트 기반 regex로 ASCII 문법을 평가하며 원시 바이트를 UTF-8로 강제 해석하지 않는다. high-byte/control 경계를 보수적 명령 경계로 인정해 바이너리 안의 명령도 검사한다. source/offset/TX payload는 수정하지 않는다.
- hook 본문·mirror·module·manifest는 리드 소유이므로 수정하지 않았다. 프로필/원시 미완 줄을 공급하는 연결은 리드 작업이다. `classify_request`의 사전 32 KiB 절단이 classifier에 앞서 발생하면 그 이전 초과를 여기서 알 수 없으므로, 리드가 원래 길이를 보존해 truncated 표시에 합쳐야 한다.

## 리드에게 묻는 것·인계

1. **블로커 해소**: `crates/loom-uart/Cargo.toml`의 `[dev-dependencies]`에 `proptest = "=1.12.0"` 등록이 필요하다(현재 workspace lock의 proptest 버전). 워커는 소유 경계 때문에 manifest/lock을 고치지 않았다. 이후 필수 fmt/clippy/test와 proptest를 재실행하기 전에는 DONE/merge-ready로 취급하지 않는다.
2. 리드의 hooks/mirror에서 `classify_text_with(channel_profile, raw_unfinished_line, payload)`를 연결한다. 사용자 Interactive 입력의 문자열 분류 예외·F1 기본 모두 자동은 그대로 유지한다.
3. 아래 표는 코드의 정본 25개와 기존 호환 4개를 추출한 것이다. 신규 API의 id와 기존 API 별칭을 구분해서 help/설정에 노출한다.

## 규칙 표 — id·등급·패턴·양성/음성 예

신규 25개 패턴은 공통 접두부 P 뒤에 아래 tail을 붙인다. ASCII case-insensitive(`unicode(false)`)이며 프로필 RegexSet의 고정 옵션이다. 별칭 4개는 원래 공개 pattern을 유지한다.

```text
P = (?:^|[;&|\x00-\x1f\x7f-\xff])[ \t]*(?:sudo[ \t]+)?(?:/[a-z0-9_./-]+/)?
```

| id | 등급 | 패턴(P + tail / 별칭은 전문) | 양성 예 | 음성 예 |
|---|---|---|---|---|
| generic.reboot | R2 | `(?:reboot\|reset)\b` | `reboot` | `echo reboot later` |
| generic.flash_write | R3 | `(?:sf\|nand\|flash)[ \t]+(?:write\|update)\b` | `sf write 0 100` | `sf read 0 100` |
| generic.flash_erase | R4 | `(?:(?:sf\|nand\|flash)[ \t]+(?:erase\|scrub)\b\|flash_erase(?:all)?\b)` | `sf erase 0 100` | `flash_erase_help` |
| generic.fs_destroy | R4 | `(?:mkfs[a-z0-9_.+-]*\b\|dd[ \t]+[^\r\n]*\bof[ \t]*=[ \t]*/dev/\|rm[ \t]+(?:-[a-z]*r[a-z]*f[a-z]*\b\|-[a-z]*f[a-z]*r[a-z]*\b\|-[a-z]*r[a-z]*[ \t]+-[a-z]*f[a-z]*\b\|-[a-z]*f[a-z]*[ \t]+-[a-z]*r[a-z]*\b\|--recursive[ \t]+--force\b\|--force[ \t]+--recursive\b))` | `rm -rf /tmp/dut` | `echo /dev/null` |
| generic.otp_write | R4 | `(?:efuse\|otp\|fuse)[ \t]+(?:prog(?:ram)?\|write\|burn[a-z0-9_]*\|lock)\b` | `fuse prog 0 1` | `fuse read 0` |
| generic.secure_boot | R4 | `secure[_-]?boot[ \t]+(?:enable\|disable\|key\|write\|burn\|lock)\b` | `secure_boot enable` | `secure_boot status` |
| generic.lock_bits | R4 | `(?:(?:lock[_-]?bits?\|option[_-]?bytes?)[ \t]+(?:write\|set\|prog(?:ram)?\|lock)\|(?:lock\|protect)[ \t]+(?:flash\|device\|chip))\b` | `lock_bits write 1` | `lock_bits read` |
| generic.bootloader | R3 | `(?:(?:reboot\|reset)[ \t]+(?:bootloader\|download\|dfu)\|bootloader\|download)\b` | `reset bootloader` | `echo download complete` |
| linux.power | R2 | `(?:shutdown\|poweroff\|halt\|systemctl[ \t]+(?:reboot\|poweroff\|halt))\b` | `systemctl poweroff` | `systemctl status` |
| linux.fw_setenv | R3 | `fw_setenv\b` | `fw_setenv bootdelay 0` | `fw_printenv` |
| linux.flash_write | R3 | `(?:flashcp\|nandwrite\|mtd[ \t]+write)\b` | `mtd write image.bin kernel` | `mtd read 0` |
| linux.bootloader | R3 | `(?:systemctl[ \t]+reboot[ \t]+--boot-loader[^ \t\r\n]*\|reboot[ \t]+(?:bootloader\|download\|dfu))\b` | `reboot bootloader` | `systemctl status` |
| uboot.saveenv | R3 | `(?:saveenv\|env[ \t]+(?:save\|default[ \t]+-a)\|fw_setenv)\b` | `saveenv` | `printenv bootcmd` |
| uboot.bootloader | R3 | `(?:fastboot\|dfu\|ums\|loadb\|loadx\|loady)\b` | `fastboot usb 0` | `version` |
| uboot.environment | R2 | `(?:setenv\|env[ \t]+set)\b` | `setenv bootdelay 0` | `printenv` |
| uboot.mmc_write | R3 | `mmc[ \t]+write\b` | `mmc write 0 100 20` | `mmc read 0 100 20` |
| esp-idf.flash_write | R3 | `(?:esptool(?:\.py)?[ \t]+[^;\r\n]*\bwrite[_-]flash\|idf\.py[ \t]+(?:[^;\r\n]*[ \t])?flash)\b` | `esptool --port COM3 write_flash 0 image.bin` | `esptool read_flash 0 100` |
| esp-idf.flash_erase | R4 | `esptool(?:\.py)?[ \t]+[^;\r\n]*\berase[_-](?:flash\|region)\b` | `esptool.py erase_flash` | `esptool read_flash 0 100` |
| esp-idf.efuse | R4 | `espefuse(?:\.py)?[ \t]+[^;\r\n]*\bburn[a-z0-9_]*\b` | `espefuse.py burn_efuse ABS_DONE_0 1` | `espefuse summary` |
| esp-idf.secure_boot | R4 | `espsecure(?:\.py)?[ \t]+[^;\r\n]*\b(?:generate_signing_key\|burn_key\|write_key\|enable_secure_boot)\b` | `espsecure.py generate_signing_key key.pem` | `espsecure verify_signature` |
| esp-idf.bootloader | R3 | `idf\.py[ \t]+(?:[^;\r\n]*[ \t])?bootloader-flash\b` | `idf.py bootloader-flash` | `idf.py monitor` |
| zephyr.reboot | R2 | `(?:kernel\|sys)[ \t]+reboot\b` | `kernel reboot cold` | `kernel uptime` |
| zephyr.flash_write | R3 | `flash[ \t]+(?:write\|load)\b` | `flash write 0 1122` | `flash read 0 100` |
| zephyr.flash_erase | R4 | `flash[ \t]+erase\b` | `flash erase 0 100` | `flash read 0 100` |
| zephyr.bootloader | R3 | `mcuboot[ \t]+(?:reboot\|serial\|enter)\b` | `mcuboot serial` | `mcuboot status` |
| flash.write | R3 | `\b(sf\|nand)\s+write\b` | `sf write 0 100` | `sf read 0 100` |
| flash.erase | R4 | `\b(sf\|nand)\s+erase\b\|\bflash_erase\b` | `sf erase 0 100` | `sf read 0 100` |
| fs.destroy | R4 | `\bmkfs(\.\w+)?\b\|\bdd\s+.*of=/dev/\|\brm\s+-[a-zA-Z]*r[a-zA-Z]*f\|\brm\s+-[a-zA-Z]*f[a-zA-Z]*r` | `mkfs.ext4 /dev/test` | `echo hello` |
| otp.fuse | R4 | `\b(fuse\s+prog\|efuse\s+burn\|espefuse(\.py)?\s+burn)` | `fuse prog 0 1` | `fuse read 0` |
