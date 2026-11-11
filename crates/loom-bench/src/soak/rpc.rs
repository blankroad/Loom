//! 실제 loopback HTTP 인증과 WebSocket JSON-RPC 클라이언트. 비밀은 결과에 넣지 않는다.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use loom_gateway::home::DaemonInfo;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::{Message, WebSocket};

fn http(info: &DaemonInfo, method: &str, path: &str, body: &str) -> Result<(String, String), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    write!(s,"{method} {path} HTTP/1.1\r\nHost: localhost:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",info.port,info.token,body.len()).map_err(|e|e.to_string())?;
    let mut text = String::new();
    s.take(64 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or("HTTP 헤더 없음")?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1));
    let expected = if method == "POST" && path == "/api/v1/bootstrap" {
        "204"
    } else {
        "200"
    };
    if status != Some(expected) {
        return Err("HTTP 인증 요청 실패".into());
    }
    Ok((head.into(), body.into()))
}

pub(super) fn cookie(info: &DaemonInfo, operate: bool) -> Result<String, String> {
    let path = if operate {
        "/api/v1/bootstrap/new?rights=operate"
    } else {
        "/api/v1/bootstrap/new?rights=watch"
    };
    let (_, body) = http(info, "GET", path, "")?;
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let code = v["code"].as_str().ok_or("bootstrap 코드 없음")?;
    let (headers, _) = http(
        info,
        "POST",
        "/api/v1/bootstrap",
        &json!({"code":code}).to_string(),
    )?;
    headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("set-cookie")
                .then(|| value.trim().split(';').next().unwrap_or_default().to_owned())
        })
        .filter(|v| !v.is_empty())
        .ok_or("viewer 쿠키 없음".into())
}

pub(super) struct Rpc {
    ws: WebSocket<TcpStream>,
    next: u64,
    pub epoch: u64,
}

pub(super) fn number(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

impl Rpc {
    pub fn connect(info: &DaemonInfo, cookie: Option<&str>, name: &str) -> Result<Self, String> {
        let mut req = format!("ws://127.0.0.1:{}/rpc", info.port)
            .into_client_request()
            .map_err(|e| e.to_string())?;
        if let Some(cookie) = cookie {
            req.headers_mut()
                .insert("cookie", cookie.parse().map_err(|_| "쿠키 헤더 오류")?);
            req.headers_mut().insert(
                "origin",
                format!("http://localhost:{}", info.port)
                    .parse()
                    .map_err(|_| "origin 헤더 오류")?,
            );
        }
        let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| e.to_string())?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        // 느린 상대에게 쓰기가 막혀도 클라이언트 버퍼가 계속 증가하지 않게 한다.
        // resume 한 프레임에는 채널 링(4 MiB) backlog가 들어올 수 있다.
        let config = tungstenite::protocol::WebSocketConfig::default()
            .write_buffer_size(0)
            .max_write_buffer_size(256 * 1024)
            .max_message_size(Some(8 * 1024 * 1024))
            .max_frame_size(Some(8 * 1024 * 1024));
        let (ws, _) =
            tungstenite::client::client_with_config(req, stream, Some(config)).map_err(|e| e.to_string())?;
        let mut rpc = Self {
            ws,
            next: 0,
            epoch: 0,
        };
        let mut params = json!({"proto":"loom/1.0","client_id":name,"want_role":if cookie.is_some(){"viewer"} else {"agent"},"client":{"kind":"bench","version":"1","tty":false}});
        if cookie.is_none() {
            params["token"] = json!(info.token);
        }
        let v = rpc.call("core.hello", params, &mut |_| {})?;
        rpc.epoch = number(&v["stream_epoch"]).ok_or("stream epoch 없음")?;
        Ok(rpc)
    }

    pub fn read(&mut self) -> Result<Option<Value>, String> {
        match self.ws.read() {
            Ok(Message::Text(text)) => serde_json::from_str(text.as_str())
                .map(Some)
                .map_err(|e| e.to_string()),
            Ok(Message::Ping(p)) => {
                self.ws.send(Message::Pong(p)).map_err(|e| e.to_string())?;
                Ok(None)
            }
            Ok(Message::Close(_)) => Err("WebSocket 종료".into()),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        notify: &mut impl FnMut(&Value),
    ) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        self.ws
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|e| e.to_string())?;
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            let Some(v) = self.read()? else {
                continue;
            };
            if v["id"].as_u64() == Some(id) {
                if let Some(error) = v.get("error") {
                    return Err(format!(
                        "RPC 오류 {}",
                        error["data"]["code"].as_str().unwrap_or("rpc")
                    ));
                }
                let result = v.get("result").cloned().ok_or("RPC 결과 없음")?;
                if result["ok"] == false {
                    return Err(format!(
                        "명령 오류 {}",
                        result["error"]["code"].as_str().unwrap_or("unknown")
                    ));
                }
                return Ok(result);
            }
            notify(&v);
        }
        Err(format!("{method} 응답 3초 시간 초과"))
    }

    pub fn subscribe(
        &mut self,
        after: Option<(u64, u64)>,
        notify: &mut impl FnMut(&Value),
    ) -> Result<(), String> {
        // C-9: 초기 구독이 snapshot_end를 기다리려면 snapshot을 명시해야 한다.
        let mut p = json!({"filter":{"kinds":["echo.data"]},"snapshot":after.is_none(),"max_batch_ms":1});
        if let Some((epoch, seq)) = after {
            p["after_seq"] = json!(seq.to_string());
            p["stream_epoch"] = json!(epoch.to_string());
        }
        self.call("core.subscribe", p, notify)?;
        // resume 성공 경로는 별도 end 프레임 없이 backlog/live를 보낸다.
        if after.is_some() {
            return Ok(());
        }
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if let Some(v) = self.read()? {
                notify(&v);
                if v["method"] == "core.snapshot_end" || v["method"] == "core.resume_end" {
                    return Ok(());
                }
            }
        }
        Err("구독 시작 프레임 3초 시간 초과".into())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use loom_gateway::daemon::{Config, start};
    use loom_gateway::home::Home;
    use loom_worker_sdk::SimClock;
    use std::sync::Arc;

    #[test]
    fn bootstrap_204_and_explicit_snapshot_then_resume_work_with_real_gateway() {
        let home = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.clock = Some(Arc::new(SimClock::new(0)));
        let run = rt.block_on(start(cfg)).unwrap();
        let session = cookie(&run.info, true).unwrap();
        let mut rpc = Rpc::connect(&run.info, Some(&session), "bench-subscribe-test").unwrap();
        let mut snapshot_end = false;
        rpc.subscribe(None, &mut |v| snapshot_end |= v["method"] == "core.snapshot_end")
            .unwrap();
        assert!(snapshot_end);
        let mut second_snapshot = false;
        rpc.subscribe(Some((rpc.epoch, 1)), &mut |v| {
            second_snapshot |= v["method"] == "core.snapshot_end"
        })
        .unwrap();
        assert!(!second_snapshot, "resume는 snapshot_end 없이 응답해야 한다");
        drop(rpc);
        run.daemon.journal.close();
        run.server.abort();
        drop(run);
        rt.shutdown_timeout(Duration::from_secs(2));
    }
}
