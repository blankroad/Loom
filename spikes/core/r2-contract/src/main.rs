//! R1 견본 코드를 수정하지 않고 잘못된 입력·튜플 확장을 재현한다.
#[allow(dead_code)]
#[path = "../../opus-repr/src/main.rs"]
mod opus_repr;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Old {
    n: u64,
}
#[derive(Serialize, Deserialize, Debug)]
struct New {
    n: u64,
    #[serde(default)]
    extra: Option<String>,
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let cases = ["00ff", "0", "가", "zz"];
    let malformed: Vec<_> = cases
        .iter()
        .map(|input| {
            let encoded = serde_json::to_string(input).unwrap();
            let result = std::panic::catch_unwind(|| {
                serde_json::from_str::<opus_repr::Bytes>(&encoded)
            });
            let status = match result {
                Err(_) => "panic",
                Ok(Err(_)) => "error",
                Ok(Ok(_)) => "ok",
            };
            serde_json::json!({"input": input, "status":status})
        })
        .collect();
    let new = New {
        n: 7,
        extra: Some("added".into()),
    };
    let tuple = rmp_serde::to_vec(&new).unwrap();
    let map = rmp_serde::to_vec_named(&new).unwrap();
    let tuple_old = rmp_serde::from_slice::<Old>(&tuple);
    let map_old = rmp_serde::from_slice::<Old>(&map);
    assert!(tuple_old.is_err());
    assert!(map_old.is_ok());
    assert_eq!(malformed[1]["status"], "panic");
    assert_eq!(malformed[2]["status"], "panic");
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "note":"실제 제품이 아닌 R1 opus-repr의 Bytes decoder를 직접 호출했다. tuple/map은 독립 최소 예제다.",
            "malformed_hex":malformed,
            "tuple_added_field_old_reader":format!("{tuple_old:?}"),
            "map_added_field_old_reader":format!("{map_old:?}")
        }))
        .unwrap()
    );
}
