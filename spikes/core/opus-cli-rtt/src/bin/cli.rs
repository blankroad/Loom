// 최소 `loom` 대역: clap 파싱 → daemon.json 대용 파일에서 포트 → 동기 WS 연결 → hello → RPC 1회 → 봉투 출력 → 종료
use clap::Parser;
#[derive(Parser)] struct A { #[arg(long)] port_file: String, #[arg(long)] json: bool, cmd: Vec<String> }
fn main() {
    let a = A::parse();
    let port = std::fs::read_to_string(&a.port_file).unwrap();
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{}/rpc", port.trim())).unwrap();
    for (id, m) in [(1, "core.hello"), (2, "uart.send")] {
        let req = serde_json::json!({"jsonrpc":"2.0","id":id,"method":m,"params":{"args":a.cmd}});
        ws.send(tungstenite::Message::Text(req.to_string().into())).unwrap();
        let _ = ws.read().unwrap();
    }
    let _ = ws.close(None);
    if a.json { println!("{{\"ok\":true}}"); } else { println!("sent"); }
}
