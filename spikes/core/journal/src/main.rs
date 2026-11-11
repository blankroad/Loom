//! 합성 JSONL 세그먼트와 SQLite WAL: 실제 fsync 횟수·파일 크기를 함께 기록한다.
use rusqlite::{Connection, params};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};
fn size(p: &Path) -> u64 {
    fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}
fn main() {
    let base = std::env::current_dir().unwrap().join("data");
    fs::create_dir_all(&base).unwrap();
    let n = 100000;
    let records:Vec<Vec<u8>>=(0..n).map(|i|{let mut b=serde_json::to_vec(&serde_json::json!({"schema_version":1,"session_id":"sim","domain":"can","channel":"ch0","kind":"can.frame","producer_id":"sim","producer_epoch":"e1","source_seq":i.to_string(),"ts":{"mono_ns":(i*10000u64).to_string(),"wall_ns":"1794240000000000000","clock_id":"host-boot","source":"host","quality":"simulated"},"dir":"rx","origin":"device","client_id":null,"actor_id":null,"request_id":null,"job_id":null,"epoch":1,"generation":1,"purpose":"sim","risk":"R0","payload":{"id":256,"data":(0..64).map(|j|format!("{:02x}",(i+j)%251)).collect::<String>()}})).unwrap();b.push(b'\n');b}).collect();
    let input: usize = records.iter().map(Vec::len).sum();
    let mut rows = vec![];
    for chunk in [200, 2000, 10000] {
        for compressed in [false, true] {
            let path = base.join(format!("seg-{chunk}-{compressed}.bin"));
            let mut f = BufWriter::with_capacity(1024 * 1024, File::create(&path).unwrap());
            let t = Instant::now();
            let mut sync_s = 0.;
            for batch in records.chunks(chunk) {
                let bytes: Vec<u8> = batch.iter().flatten().copied().collect();
                let body = if compressed {
                    zstd::stream::encode_all(bytes.as_slice(), 1).unwrap()
                } else {
                    bytes
                };
                f.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
                f.write_all(&body).unwrap();
                f.flush().unwrap();
                let s = Instant::now();
                f.get_ref().sync_data().unwrap();
                sync_s += s.elapsed().as_secs_f64();
            }
            let wall = t.elapsed().as_secs_f64();
            drop(f);
            rows.push(serde_json::json!({"format":if compressed{"segment-zstd1"}else{"segment-jsonl"},"batch_events":chunk,"nominal_sync_interval_ms_at_100k":chunk as f64/100.,"fsync_count":n as usize/chunk,"sync_total_s":sync_s,"wall_s":wall,"events_s":n as f64/wall,"file_bytes":size(&path)}));
        }
        for mode in ["NORMAL", "FULL", "FULL_FULLFSYNC"] {
            let path = base.join(format!("sqlite-{chunk}-{mode}.db"));
            assert!(
                !path.exists(),
                "다시 실행할 때 새 data 디렉터리를 사용해야 함"
            );
            let mut c = Connection::open(&path).unwrap();
            let sync_mode = if mode == "FULL_FULLFSYNC" {
                "FULL"
            } else {
                mode
            };
            let fullsync = if mode == "FULL_FULLFSYNC" {
                "ON"
            } else {
                "OFF"
            };
            c.execute_batch(&format!("PRAGMA journal_mode=WAL; PRAGMA synchronous={sync_mode}; PRAGMA fullfsync={fullsync}; PRAGMA checkpoint_fullfsync={fullsync}; PRAGMA wal_autocheckpoint=0; CREATE TABLE events(seq INTEGER PRIMARY KEY, payload BLOB NOT NULL);")).unwrap();
            let t = Instant::now();
            for (bi, batch) in records.chunks(chunk).enumerate() {
                let tx = c.transaction().unwrap();
                {
                    let mut stmt = tx
                        .prepare_cached("INSERT INTO events VALUES(?1,?2)")
                        .unwrap();
                    for (i, r) in batch.iter().enumerate() {
                        stmt.execute(params![(bi * chunk + i) as i64, r]).unwrap();
                    }
                }
                tx.commit().unwrap();
            }
            let wall = t.elapsed().as_secs_f64();
            let count: i64 = c
                .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, n as i64);
            let wal = path.with_extension("db-wal");
            let live_bytes = size(&path) + size(&wal);
            let t = Instant::now();
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
            let checkpoint_s = t.elapsed().as_secs_f64();
            rows.push(serde_json::json!({"format":format!("sqlite-wal-{mode}"),"batch_events":chunk,"nominal_sync_interval_ms_at_100k":chunk as f64/100.,"wall_s":wall,"events_s":n as f64/wall,"file_bytes_after_checkpoint":size(&path),"live_db_plus_wal_bytes":live_bytes,"checkpoint_s":checkpoint_s,"note":"NORMAL은 commit별 내구성 fsync 보장 없음; FULL과 동일 조건으로 해석 금지"}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"events":n,"input_bytes":input,"mean_record_bytes":input as f64/n as f64,"rows":rows,"note":"Rust 1.93.1 sync_data는 Apple에서 F_FULLFSYNC 호출(공식 std 소스 확인). SQLite fullfsync 설정과 구별. 실제 전원 차단 시험 아님. 입력 미리 생성, codec 생성 CPU 제외"})).unwrap());
}
