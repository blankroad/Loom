# MessagePack arbitrary_precision 조합 분석

19:43 메인 방침을 따른다. P3 선행 revert 후 원래 시험을 유지하며, 아래 시험 변경은 활성 소스에서 제거했다. 진단 패치는 본선에 적용하지 않는다. 기존 R1 필드 변경은 보존했다.

- 원본 시험: `cargo test -p loom-proto --features serde_json/arbitrary_precision --test msgpack unknown_fields_are_ignored -- --exact`로 main과 같은 TypeMismatch(FixMap(1)) 재현. 기본 feature 단독은 통과한다.
- 원인: typed record의 native 숫자를 serde_json::Value로 읽고 재직렬화하면 arbitrary_precision Number가 private 단일 map으로 쓰인다. U64의 binary deserializer는 이를 숫자로 받지 않는다. UI feature를 직접 변경하지 않았다.
- 진단용 시험 수정은 알려진 typed MessagePack 바이트를 보존하고 미지 array/map 필드만 추가했다. 기존 JSON 검증은 유지하고 u64::MAX 사례도 추가했다. 이 조합에서 msgpack 시험 7개는 통과했다. 그러나 이 사실은 운영 wire의 상호운용 안전성을 뜻하지 않는다.
- 실제 UnixStream pair + SDK FrameWriter/FrameReader에서 Events.payload와 ReplyResult::Ok를 왕복했다. u64::MAX, 2^53+1, i64::MIN, 중첩 배열·소수는 기본/AP 두 조합의 Rust↔Rust 왕복에서 모두 정밀도를 보존했다.
- 같은 실제 Reply 프레임을 serde_json::Value가 아닌 `max:u64` native payload 구조체로 읽는 진단에서는 기본 조합은 `18446744073709551615`를 얻고 AP 조합은 TypeMismatch(FixMap(1))로 실패했다. **실제 payload의 MessagePack 표현도 바뀐다. 시험만의 문제가 아니다.** P3 기능을 다시 통합하기 전에 native 숫자 wire를 보존하는 직렬화 경계를 별도 검증해야 한다.

분리한 `proto-ap-feature-test-only.patch`는 R1 리터럴 보강 hunk를 포함하지 않는 두 시험 파일의 진단안이다. 본선 반영 대상이 아니며 운영 직렬화 수정도 아니다. 현재 활성 proto 시험은 원본이다.

증거 로그는 워크트리 target/proto-hotfix-before.log(원본 AP 실패), proto-hotfix-after-feature.log(진단안 7개 통과), proto-hotfix-ipc-feature.log(Rust IPC), proto-hotfix-native-feature.log(native 소비자 실패), proto-hotfix-native-default.log(native 소비자 정상)에 있다. 커밋하지 않았다.
