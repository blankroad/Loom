//! Minimal bounded HTTP/1.1 GET to the daemon on 127.0.0.1 (no async runtime, no TLS).

use std::io::{self, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Largest response accepted (headers + body). The bootstrap answer is ~200 bytes.
const MAX_RESPONSE: usize = 64 * 1024;

pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// `GET path` with `Authorization: Bearer <token>`, every step bounded by `timeout`.
pub(crate) fn get_bearer(port: u16, path: &str, token: &str, timeout: Duration) -> io::Result<Response> {
    get(port, path, Some(token), timeout)
}

/// Unauthenticated `GET path` (public static assets such as the `/ui/` page), bounded like [`get_bearer`].
pub(crate) fn get_public(port: u16, path: &str, timeout: Duration) -> io::Result<Response> {
    get(port, path, None, timeout)
}

fn get(port: u16, path: &str, token: Option<&str>, timeout: Duration) -> io::Result<Response> {
    let timeout = timeout.max(Duration::from_millis(1));
    let end = Instant::now() + timeout;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_nodelay(true)?;
    s.set_write_timeout(Some(timeout))?;
    // Bearer calls keep the original JSON request byte-for-byte; public assets accept anything.
    // Public (non-/api) GETs must name the daemon's canonical host: loomd answers any other
    // `Host: 127.0.0.1` GET outside /rpc and /api/ with 308 → http://localhost:<port>/… .
    let (host, auth, accept) = match token {
        Some(t) => (
            "127.0.0.1",
            format!("Authorization: Bearer {t}\r\n"),
            "application/json",
        ),
        None => ("localhost", String::new(), "*/*"),
    };
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n{auth}Accept: {accept}\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::with_capacity(1024);
    let mut buf = [0u8; 4096];
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "response timed out"));
        }
        s.set_read_timeout(Some(left))?;
        let n = match s.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > MAX_RESPONSE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response too large"));
        }
        if response_complete(&raw) {
            break;
        }
    }
    parse(&raw)
}

fn split_head(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    let i = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some((&raw[..i], &raw[i + 4..]))
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn response_complete(raw: &[u8]) -> bool {
    let Some((head, body)) = split_head(raw) else {
        return false;
    };
    let head = String::from_utf8_lossy(head);
    if let Some(n) = header(&head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        return body.len() >= n;
    }
    if header(&head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return body.ends_with(b"0\r\n\r\n");
    }
    false
}

fn parse(raw: &[u8]) -> io::Result<Response> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_owned());
    let (head, body) = split_head(raw).ok_or_else(|| bad("incomplete response"))?;
    let head = std::str::from_utf8(head).map_err(|_| bad("non-UTF-8 headers"))?;
    let status_line = head.split("\r\n").next().unwrap_or_default();
    let mut parts = status_line.split_whitespace();
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(bad("not an HTTP/1.x response"));
    }
    let status: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| bad("bad status line"))?;
    let body = if header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        dechunk(body).ok_or_else(|| bad("bad chunked body"))?
    } else if let Some(n) = header(head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.get(..n).ok_or_else(|| bad("truncated body"))?.to_vec()
    } else {
        body.to_vec()
    };
    Ok(Response { status, body })
}

fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let i = rest.windows(2).position(|w| w == b"\r\n")?;
        let len = std::str::from_utf8(&rest[..i]).ok()?;
        let len = len.split(';').next()?.trim();
        let n = usize::from_str_radix(len, 16).ok()?;
        rest = &rest[i + 2..];
        if n == 0 {
            return Some(out);
        }
        out.extend_from_slice(rest.get(..n)?);
        rest = rest.get(n..)?.strip_prefix(b"\r\n")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_and_chunked() {
        let r = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhiXX").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"hi"[..]));
        let r = parse(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, b"abcde");
        assert!(parse(b"SSH-2.0-x\r\n\r\n").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nhi").is_err());
    }
}
