//! Connection credentials (contract 01 §1.3): `daemon.json` token = agent; web bootstrap code →
//! HttpOnly/SameSite=Strict cookie = viewer with rights. Origins come from credentials only.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use loom_proto::rpc::Rights;

use crate::home::{ct_eq, random_hex};
use crate::hub::lock;

/// Bootstrap code lifetime.
pub const CODE_TTL: Duration = Duration::from_secs(60);
/// Viewer session cookie name.
pub const COOKIE: &str = "loom_session";
/// Most live viewer sessions kept (oldest evicted).
pub const MAX_SESSIONS: usize = 256;

/// Credentials of the daemon.
#[derive(Debug)]
pub struct Auth {
    token: String,
    codes: Mutex<HashMap<String, (Instant, Rights)>>,
    sessions: Mutex<Vec<(String, Rights)>>,
}

impl Auth {
    /// New with the agent token.
    pub fn new(token: String) -> Self {
        Auth {
            token,
            codes: Mutex::new(HashMap::new()),
            sessions: Mutex::new(Vec::new()),
        }
    }

    /// The agent token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Agent token check (constant time).
    pub fn is_token(&self, t: &str) -> bool {
        ct_eq(t, &self.token)
    }

    /// `Authorization: Bearer <token>`.
    pub fn bearer_ok(&self, h: &HeaderMap) -> bool {
        h.get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|t| self.is_token(t.trim()))
    }

    /// Issue a one-time bootstrap code (60 s).
    pub fn issue_code(&self, rights: Rights) -> String {
        let code = random_hex(16);
        let mut c = lock(&self.codes);
        c.retain(|_, (at, _)| at.elapsed() < CODE_TTL);
        c.insert(code.clone(), (Instant::now(), rights));
        code
    }

    /// Redeem a code once → new session cookie value and its rights.
    pub fn redeem(&self, code: &str) -> Option<(String, Rights)> {
        let (at, rights) = lock(&self.codes).remove(code)?;
        if at.elapsed() >= CODE_TTL {
            return None;
        }
        let session = random_hex(32);
        let mut s = lock(&self.sessions);
        if s.len() >= MAX_SESSIONS {
            s.remove(0);
        }
        s.push((session.clone(), rights));
        Some((session, rights))
    }

    /// Viewer rights from the request's session cookie.
    pub fn cookie_rights(&self, h: &HeaderMap) -> Option<Rights> {
        let v = h
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .find_map(|kv| {
                let (k, v) = kv.trim().split_once('=')?;
                (k == COOKIE).then(|| v.to_owned())
            })?;
        lock(&self.sessions)
            .iter()
            .find(|(s, _)| ct_eq(s, &v))
            .map(|(_, r)| *r)
    }

    /// `Set-Cookie` value for a session.
    pub fn set_cookie(session: &str) -> String {
        format!("{COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn code_is_one_time_and_cookie_maps_rights() {
        let a = Auth::new("t".repeat(64));
        let code = a.issue_code(Rights::Watch);
        let (s, r) = a.redeem(&code).unwrap();
        assert_eq!(r, Rights::Watch);
        assert!(a.redeem(&code).is_none(), "one-time");
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("x=1; {COOKIE}={s}")).unwrap(),
        );
        assert_eq!(a.cookie_rights(&h), Some(Rights::Watch));
        h.insert(header::COOKIE, HeaderValue::from_static("loom_session=forged"));
        assert_eq!(a.cookie_rights(&h), None);
    }
}
