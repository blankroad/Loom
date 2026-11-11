// 스파이크: 같은 serde 타입이 JSON(사람·UI)에서는 u64=문자열, bytes=hex,
// MessagePack(worker 프로토콜)에서는 u64=정수, bytes=bin 으로 나가는지 확인 + 크기·속도 비교.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct U64(pub u64);
impl Serialize for U64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() { s.collect_str(&self.0) } else { s.serialize_u64(self.0) }
    }
}
impl<'de> Deserialize<'de> for U64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            let s = String::deserialize(d)?; s.parse().map(U64).map_err(serde::de::Error::custom)
        } else { u64::deserialize(d).map(U64) }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Bytes(pub Vec<u8>);
impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let mut h = String::with_capacity(self.0.len()*2);
            for b in &self.0 { h.push_str(&format!("{:02x}", b)); }
            s.serialize_str(&h)
        } else { serde_bytes::serialize(&self.0, s) }
    }
}
impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            let s = String::deserialize(d)?;
            (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2],16)).collect::<Result<Vec<_>,_>>()
                .map(Bytes).map_err(serde::de::Error::custom)
        } else { serde_bytes::deserialize(d).map(Bytes) }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Ts { mono_ns: U64, wall_ns: U64, clock_id: String, source: String, quality: Option<String> }
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Payload { encoding: String, data: Bytes }
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Ev {
    schema_version: u16, session_id: String, domain: String, channel: String, kind: String,
    seq: U64, epoch: U64, producer_id: String, producer_epoch: U64, source_seq: U64,
    ts: Ts, dir: Option<String>, origin: String, client_id: Option<String>, actor_id: Option<String>,
    request_id: Option<String>, job_id: Option<String>, generation: u32, purpose: Option<String>,
    risk: Option<String>, payload: Option<Payload>,
}
fn ev(i: u64) -> Ev {
    Ev { schema_version: 1, session_id: "s12".into(), domain: "can".into(), channel: "ch0".into(),
        kind: "can.frame".into(), seq: U64(1_840_201 + i), epoch: U64(3), producer_id: "can-sim".into(),
        producer_epoch: U64(2), source_seq: U64(99_000 + i),
        ts: Ts { mono_ns: U64(81_234_567_890_123 + i*200_000), wall_ns: U64(1_794_146_701_234_567_000 + i),
                 clock_id: "host-boot-7f3a".into(), source: "driver".into(), quality: None },
        dir: Some("rx".into()), origin: "device".into(), client_id: None, actor_id: None, request_id: None,
        job_id: None, generation: 1, purpose: None, risk: None,
        payload: Some(Payload { encoding: "hex".into(), data: Bytes(vec![0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88]) }) }
}
fn main() {
    let e = ev(0);
    let j = serde_json::to_string(&e).unwrap();
    let m_map = rmp_serde::to_vec_named(&e).unwrap();
    let m_arr = rmp_serde::to_vec(&e).unwrap();
    println!("json: {}", j);
    println!("sizes json={} msgpack_named={} msgpack_array={}", j.len(), m_map.len(), m_arr.len());
    assert_eq!(serde_json::from_str::<Ev>(&j).unwrap(), e);
    assert_eq!(rmp_serde::from_slice::<Ev>(&m_map).unwrap(), e);
    assert_eq!(rmp_serde::from_slice::<Ev>(&m_arr).unwrap(), e);
    // msgpack 안에서 seq가 정수인지: 문자열 "1840201"이 바이트열에 없어야 함
    let has_str = m_map.windows(7).any(|w| w == b"1840201");
    println!("msgpack contains decimal string of seq? {}", has_str);
    let n = 200_000u64;
    let evs: Vec<Ev> = (0..n).map(ev).collect();
    for (name, f) in [("json", 0), ("msgpack_named", 1), ("msgpack_array", 2)] {
        let t = Instant::now(); let mut bytes = 0usize;
        for e in &evs { bytes += match f { 0 => serde_json::to_vec(e).unwrap().len(), 1 => rmp_serde::to_vec_named(e).unwrap().len(), _ => rmp_serde::to_vec(e).unwrap().len() }; }
        let dt = t.elapsed().as_secs_f64();
        println!("{name}: encode {:.0} ev/s, {:.1} MB/s out, avg {} B", n as f64/dt, bytes as f64/dt/1e6, bytes/(n as usize));
    }
}
