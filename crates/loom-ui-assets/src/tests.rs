use std::error::Error;

use axum::http::{Method, Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;

#[derive(Embed)]
#[folder = "tests/fixtures"]
struct Fixtures;

// This directory deliberately does not exist. Compilation exercises the same
// allow_missing derive used by Distribution, without changing shared ui/dist.
#[derive(Embed)]
#[folder = "tests/fixtures/dist-does-not-exist"]
#[allow_missing = true]
struct MissingDistribution;

type TestResult = Result<(), Box<dyn Error>>;

async fn request(app: Router, uri: &str) -> Response {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("valid request"),
    )
    .await
    .expect("infallible router")
}

fn assert_security(headers: &HeaderMap) {
    for (name, value) in security_headers() {
        assert_eq!(headers.get(name.expect("header name")), Some(&value));
    }
}

#[tokio::test]
async fn serves_embedded_mime_and_cache_policy() -> TestResult {
    for (path, content_type, cache) in [
        ("/ui/index.html", "text/html; charset=utf-8", REVALIDATE),
        (
            "/ui/assets/style-A1b2C3d4.css",
            "text/css; charset=utf-8",
            IMMUTABLE,
        ),
        ("/ui/opaque.bin", "application/octet-stream", REVALIDATE),
        (
            "/ui/assets/settings.js",
            "text/javascript; charset=utf-8",
            REVALIDATE,
        ),
        (
            "/ui/assets/app-Ab12_-Cd.js",
            "text/javascript; charset=utf-8",
            IMMUTABLE,
        ),
    ] {
        let response = request(router_for(Fixtures::get), path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], content_type, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], cache, "{path}");
        assert_security(response.headers());
        let body = response.into_body().collect().await?.to_bytes();
        assert!(!body.is_empty(), "{path}");
    }
    Ok(())
}

#[tokio::test]
async fn etag_is_a_stable_quoted_sha256_and_revalidates() -> TestResult {
    let path = "/ui/assets/app-Ab12_-Cd.js";
    let app = router_for(Fixtures::get);
    let first = request(app.clone(), path).await;
    let etag = first.headers()[header::ETAG].clone();
    let text = etag.to_str()?;
    // Independently computed SHA-256 of the checked-in fixture bytes.
    assert_eq!(
        text,
        "\"ddf066ee341c060cca67ed6faa462602690b490d98c6fecdf607c447754e14bb\""
    );
    assert_eq!(text.len(), 66);
    assert!(text.starts_with('"') && text.ends_with('"'));
    assert!(text[1..65].bytes().all(|byte| byte.is_ascii_hexdigit()));

    for value in [
        text.to_owned(),
        format!("W/{text}"),
        format!("\"old\", W/{text}"),
        "*".to_owned(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::IF_NONE_MATCH, value)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()[header::ETAG], etag);
        assert_eq!(response.headers()[header::CACHE_CONTROL], IMMUTABLE);
        assert_security(response.headers());
        assert!(response.into_body().collect().await?.to_bytes().is_empty());
    }
    let miss = app
        .oneshot(
            Request::builder()
                .uri(path)
                .header(header::IF_NONE_MATCH, "\"old\"")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(miss.status(), StatusCode::OK);
    assert!(!miss.into_body().collect().await?.to_bytes().is_empty());
    Ok(())
}

#[tokio::test]
async fn distinct_content_has_distinct_etags() {
    let app = router_for(Fixtures::get);
    let index = request(app.clone(), "/ui/").await;
    let script = request(app, "/ui/assets/app-Ab12_-Cd.js").await;
    assert_ne!(index.headers()[header::ETAG], script.headers()[header::ETAG]);
}

#[tokio::test]
async fn spa_fallback_uses_index_mime_etag_and_no_cache() -> TestResult {
    let app = router_for(Fixtures::get);
    let index = request(app.clone(), "/ui/").await;
    let etag = index.headers()[header::ETAG].clone();
    let body = index.into_body().collect().await?.to_bytes();
    for path in ["/ui/can/trace?ch=ch0", "/ui/assets/missing-A1b2C3d4.js"] {
        let response = request(app.clone(), path).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], REVALIDATE);
        assert_eq!(response.headers()[header::ETAG], etag);
        assert_eq!(response.into_body().collect().await?.to_bytes(), body);
    }
    Ok(())
}

#[tokio::test]
async fn missing_dist_serves_embedded_guidance() -> TestResult {
    assert!(MissingDistribution::iter().next().is_none());
    let response = request(router_for(MissingDistribution::get), "/ui/uds/console").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], REVALIDATE);
    assert_security(response.headers());
    let bytes = response.into_body().collect().await?.to_bytes();
    let html = std::str::from_utf8(&bytes)?;
    assert!(html.contains("pnpm build"));
    assert!(html.contains("실시간 데이터를 표시하지 않습니다"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<style"));
    Ok(())
}

#[tokio::test]
async fn ui_redirect_preserves_query_and_security_headers() {
    let response = request(router_for(Fixtures::get), "/ui?next=can%2Ftrace").await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(response.headers()[header::LOCATION], "/ui/?next=can%2Ftrace");
    assert_security(response.headers());
}

#[tokio::test]
async fn head_retains_get_metadata_without_a_body() -> TestResult {
    let app = router_for(Fixtures::get);
    let get = request(app.clone(), "/ui/").await;
    let head = app
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/ui/")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::ETAG], get.headers()[header::ETAG]);
    assert_eq!(
        head.headers()[header::CONTENT_TYPE],
        get.headers()[header::CONTENT_TYPE]
    );
    assert!(head.into_body().collect().await?.to_bytes().is_empty());
    Ok(())
}

#[tokio::test]
async fn traversal_and_wrong_methods_are_rejected_with_headers() -> TestResult {
    for path in ["/ui/%2e%2e/Cargo.toml", "/ui/assets%5cfile.js", "/ui/%00"] {
        let response = request(router_for(Fixtures::get), path).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_security(response.headers());
    }
    let response = router_for(Fixtures::get)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/ui/")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_security(response.headers());
    Ok(())
}

#[tokio::test]
async fn router_merges_without_capturing_gateway_routes() -> TestResult {
    let app = Router::new()
        .route("/rpc", get(|| async { "gateway" }))
        .merge(router());
    let response = request(app.clone(), "/rpc").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.into_body().collect().await?.to_bytes(), "gateway");
    assert_eq!(
        request(app.clone(), "/api/v1/missing").await.status(),
        StatusCode::NOT_FOUND
    );
    let response = request(app, "/ui/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_security(response.headers());
    Ok(())
}

#[test]
fn canonical_redirect_changes_only_the_exact_ip_authority() {
    assert_eq!(
        canonical_origin_redirect("127.0.0.1:47123", 47123, "/ui/?next=can%2Ftrace"),
        Some(HeaderValue::from_static(
            "http://localhost:47123/ui/?next=can%2Ftrace"
        )),
    );
    for host in [
        "localhost:47123",
        "127.0.0.1:47124",
        "[::1]:47123",
        "127.0.0.1:47123.evil",
        "other:47123",
    ] {
        assert!(canonical_origin_redirect(host, 47123, "/ui/").is_none(), "{host}");
    }
    for path in [
        "http://other/ui/",
        "//other/ui/",
        "/ui/\r\nInjected: yes",
        "/ui/\\other",
    ] {
        assert!(canonical_origin_redirect("127.0.0.1:47123", 47123, path).is_none());
    }
    assert!(canonical_origin_redirect("127.0.0.1:0", 0, "/ui/").is_none());
}

#[test]
fn immutable_detection_is_scoped_to_vite_hashed_assets() {
    for path in ["assets/app-Ab12_-Cd.js", "assets/sub/한글-A1b2C3d4.css"] {
        assert!(is_hashed_asset(path), "{path}");
    }
    for path in [
        "index.html",
        "other/app-A1b2C3d4.js",
        "assets/settings.js",
        "assets/app-short.js",
        "assets/한글.css",
    ] {
        assert!(!is_hashed_asset(path), "{path}");
    }
}
