//! Offline UI assets, scoped to `/ui`, for the CORE gateway.
//!
//! Merge [`router`] into the gateway (do not nest it under `/ui` again).
//! Host/Origin validation and authentication remain gateway responsibilities.
//! `rust-embed`'s `allow_missing` permits builds before `pnpm build`; an embedded
//! guidance page is served when the distribution has no `index.html`.

use std::borrow::Cow;

use axum::{
    Router,
    body::Body,
    extract::{OriginalUri, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use rust_embed::{Embed, EmbeddedFile};

/// Value suitable for the HTTP `Location` response header.
pub type Location = HeaderValue;

// Scripts stay strict. Styles allow 'unsafe-inline' because xterm.js 6 injects <style>
// elements at runtime (Viewport, DomRenderer) — decision UI lead 2026-11-11: style
// injection cannot execute code; script-src remains 'self' only.
const CSP: &str = "default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";
const PERMISSIONS_POLICY: &str = "serial=(), usb=(), hid=(), bluetooth=()";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

#[derive(Embed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct Distribution;

#[derive(Embed)]
#[folder = "fallback"]
struct Guidance;

type Lookup = fn(&str) -> Option<EmbeddedFile>;

#[derive(Clone, Copy, Debug)]
struct Assets {
    get: Lookup,
}

/// Serve `/ui`, `/ui/`, and `/ui/{*path}` with the mandatory security headers.
///
/// Unmatched paths outside `/ui` are left to the gateway. Missing paths within
/// `/ui` use `index.html` for SPA navigation, always with `no-cache`.
/// All bytes are embedded even in debug builds, so no request performs disk I/O
/// or grows a runtime cache. Request parsing/body limits belong to the gateway.
pub fn router() -> Router {
    router_for(Distribution::get)
}

fn router_for(distribution: Lookup) -> Router {
    let lookup = if distribution("index.html").is_some() {
        distribution
    } else {
        Guidance::get
    };
    Router::new()
        .route("/ui", get(directory_redirect))
        .route("/ui/", get(index))
        .route("/ui/{*path}", get(asset))
        .with_state(Assets { get: lookup })
        .layer(middleware::from_fn(add_security_headers))
}

async fn directory_redirect(OriginalUri(uri): OriginalUri) -> Redirect {
    let location = match uri.query() {
        Some(query) => format!("/ui/?{query}"),
        None => "/ui/".to_owned(),
    };
    Redirect::permanent(&location)
}

/// Headers required by contract 01 §1.2, also usable on gateway responses.
///
/// [`router`] already applies them to its routes, including redirects, 304s and
/// method errors. The gateway should apply them to its other response paths.
pub fn security_headers() -> HeaderMap {
    let mut headers = HeaderMap::with_capacity(4);
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    headers.insert("permissions-policy", HeaderValue::from_static(PERMISSIONS_POLICY));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers
}

async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().extend(security_headers());
    response
}

/// Redirect an exact loopback-IP authority to the canonical localhost origin.
///
/// `path` must be an origin-form path plus optional query, such as the gateway's
/// `OriginalUri`. The gateway must validate Host/Origin *before* calling this
/// helper: `None` is not an allowlist decision. No forwarded headers are used.
pub fn canonical_origin_redirect(host: &str, port: u16, path: &str) -> Option<Location> {
    if port == 0
        || host != format!("127.0.0.1:{port}")
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return None;
    }
    HeaderValue::from_str(&format!("http://localhost:{port}{path}")).ok()
}

async fn index(State(assets): State<Assets>, headers: HeaderMap) -> Response {
    serve(assets, "index.html", &headers)
}

async fn asset(State(assets): State<Assets>, Path(path): Path<String>, headers: HeaderMap) -> Response {
    // No arbitrary paths are ever opened. Reject traversal syntax explicitly
    // rather than accidentally treating it as SPA navigation.
    if path.contains(['\\', '\0']) || path.split('/').any(|part| matches!(part, "." | "..")) {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("constant response is valid");
    }
    serve(assets, &path, &headers)
}

fn serve(assets: Assets, path: &str, request_headers: &HeaderMap) -> Response {
    let (served_path, file) = match (assets.get)(path) {
        Some(file) => (path, file),
        None => match (assets.get)("index.html") {
            Some(file) => ("index.html", file),
            None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
    };
    let etag = etag(file.metadata.sha256_hash());
    let cache_control = if is_hashed_asset(served_path) {
        IMMUTABLE
    } else {
        REVALIDATE
    };
    let not_modified = request_headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag);

    let mut response = if not_modified {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        response
    } else {
        match file.data {
            Cow::Borrowed(bytes) => Response::new(Body::from(bytes)),
            Cow::Owned(bytes) => Response::new(Body::from(bytes)),
        }
    };
    let mime = mime_guess::from_path(served_path).first_or_octet_stream();
    let content_type = if mime.essence_str().starts_with("text/") {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.to_string()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("hex ETag is valid"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type).expect("MIME is valid"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response
}

fn etag(hash: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(66);
    value.push('"');
    for byte in hash {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 15)]));
    }
    value.push('"');
    value
}

fn is_hashed_asset(path: &str) -> bool {
    // Vite's default content hash is eight base64url characters. Be conservative
    // for other naming schemes: they remain revalidated, never stale forever.
    let Some(file) = path
        .strip_prefix("assets/")
        .and_then(|path| path.rsplit('/').next())
    else {
        return false;
    };
    let Some((stem, _extension)) = file.rsplit_once('.') else {
        return false;
    };
    let split = stem.len().saturating_sub(8);
    let (Some(prefix), Some(hash)) = (stem.get(..split), stem.get(split..)) else {
        return false;
    };
    prefix.ends_with('-')
        && hash.len() == 8
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests;
