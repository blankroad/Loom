//! 계획·정책·증거가 함께 사용하는 JSON 정규화와 SHA-256 정본.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// 모든 객체 키를 정렬하고 공백 없이 직렬화한다. Map의 BTreeMap 동작은 시험으로 고정한다.
pub fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).expect("serde_json::Value는 항상 JSON으로 직렬화할 수 있다")
}

/// SHA-256을 소문자 64자리 hex로 반환한다.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_is_sorted_and_nested_json_has_no_whitespace() {
        let mut map = serde_json::Map::new();
        map.insert("z".into(), serde_json::json!([{"b":2,"a":1}]));
        map.insert("a".into(), serde_json::json!("한 글"));
        assert_eq!(map.keys().map(String::as_str).collect::<Vec<_>>(), ["a", "z"]);
        assert_eq!(
            canonical_json(&Value::Object(map)),
            r#"{"a":"한 글","z":[{"a":1,"b":2}]}"#
        );
    }

    #[test]
    fn standard_sha256_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
