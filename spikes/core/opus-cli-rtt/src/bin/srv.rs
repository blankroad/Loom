// 최소 gateway 대역: WS로 hello → RPC 1개에 즉시 응답 (허브·정책·저널 없음)
use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade}, routing::get, Router};
async fn handle(mut s: WebSocket) {
    while let Some(Ok(m)) = s.recv().await {
        if let Message::Text(t) = m {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            let r = serde_json::json!({"jsonrpc":"2.0","id":v["id"],"result":{"ok":true,"role":"agent"}});
            if s.send(Message::Text(r.to_string().into())).await.is_err() { break; }
        }
    }
}
#[tokio::main]
async fn main() {
    let app = Router::new().route("/rpc", get(|ws: WebSocketUpgrade| async { ws.on_upgrade(handle) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    std::fs::write(std::env::args().nth(1).unwrap(), port.to_string()).unwrap();
    axum::serve(l, app).await.unwrap();
}
