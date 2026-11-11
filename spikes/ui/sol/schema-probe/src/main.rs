use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(transparent)]
struct DecimalU64(String);

#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", content = "payload")]
enum WireEvent {
    #[serde(rename = "can.frame")]
    CanFrame {
        seq: DecimalU64,
        mono_ns: DecimalU64,
        data_hex: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    #[serde(rename = "record.health")]
    RecordHealth { ok: bool },
}

fn main() {
    std::fs::create_dir_all("schema-probe/out").unwrap();
    let schema = schema_for!(WireEvent);
    std::fs::write("schema-probe/out/schema.json",serde_json::to_string_pretty(&schema).unwrap()).unwrap();
    let fixture=WireEvent::CanFrame {
        seq:DecimalU64(u64::MAX.to_string()),
        mono_ns:DecimalU64("9007199254740993".into()),
        data_hex:"0102030405060708".into(),note:None,
    };
    let json=serde_json::to_string_pretty(&fixture).unwrap();
    let _:WireEvent=serde_json::from_str(&json).unwrap();
    std::fs::write("schema-probe/out/fixture.json",json).unwrap();
    let config=ts_rs::Config::new();
    std::fs::write("schema-probe/out/direct-ts-rs.ts",format!("{}\n{}\n",DecimalU64::decl(&config),WireEvent::decl(&config))).unwrap();
    println!("schema+max-u64-string fixture+ts-rs declaration generated");
}
