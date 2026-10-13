//! Local API gateway integration tests. Synthetic trust exists only under cfg(test).
use crate::{
    client::{Trust, verify},
    config::Config,
    protocol::*,
    proxy::Proxy,
};
use axum::body::{Body, to_bytes};
use bytes::Bytes;
use hyper::{Request, Response, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::{
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle, time::timeout};

struct Running {
    url: String,
    task: JoinHandle<()>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Captured {
    path: String,
    headers: hyper::HeaderMap,
    body: Bytes,
}
struct Upstream {
    running: Running,
    calls: Arc<Mutex<Vec<Captured>>>,
    attestations: Arc<AtomicUsize>,
    quote_version: Arc<AtomicUsize>,
}

async fn upstream(evidence_type: &'static str, response_mode: &'static str) -> Upstream {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate = cert.der().to_vec();
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let captured = calls.clone();
    let attestations = Arc::new(AtomicUsize::new(0));
    let attests = attestations.clone();
    let quote_version = Arc::new(AtomicUsize::new(7));
    let quotes = quote_version.clone();
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let (socket, _) = socket.unwrap();
                    let acceptor = acceptor.clone(); let certificate = certificate.clone();
                    let captured = captured.clone(); let attests = attests.clone(); let quotes = quotes.clone();
                    tasks.spawn(async move {
                        let Ok(tls) = acceptor.accept(socket).await else { return; };
                        let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                            let certificate = certificate.clone(); let captured = captured.clone(); let attests = attests.clone(); let quotes = quotes.clone();
                            async move {
                                let (parts, body) = request.into_parts();
                                let bytes = to_bytes(Body::new(body), MAX_BODY + 1024).await.unwrap();
                                if parts.uri.path() == ATTESTATION_PATH {
                                    assert!(!parts.headers.contains_key("authorization"));
                                    assert!(!parts.headers.contains_key("x-api-key"));
                                    attests.fetch_add(1, Ordering::SeqCst);
                                    let challenge: Challenge = serde_json::from_slice(&bytes).unwrap();
                                    let mut value = json!({"protocol_version":1,"evidence_type":evidence_type,
                                        "nonce":challenge.nonce,"certificate_sha256":certificate_hash(&certificate),
                                        "evidence":{"binding":hex(&report_data(&challenge.nonce, &certificate).unwrap())}});
                                    if response_mode == "bad-binding" { value["nonce"] = json!("00".repeat(32)); }
                                    return Ok::<_, Infallible>(Response::new(Body::from(value.to_string())));
                                }
                                captured.lock().unwrap().push(Captured { path: parts.uri.to_string(), headers: parts.headers, body: bytes });
                                if !["catalog", "catalog-denied", "inventory-unavailable"].contains(&response_mode) {
                                    let (mut models, mut pools) = crate::routing::tests::fixture();
                                    if quotes.load(Ordering::SeqCst) == 8 {
                                        pools["data"][1]["multiplier_millionths"] = json!(900000);
                                        pools["data"][1]["price_version"] = json!(8);
                                        models["data"][1]["router_pricing"]["version"] = json!(8);
                                        models["data"][1]["router_pricing"]["rates"][0]["multiplier_millionths"] = json!(900000);
                                    }
                                    if parts.uri.path() == "/v1/models" { let mut r = Response::new(Body::from(models.to_string())); *r.status_mut() = hyper::StatusCode::CREATED; return Ok(r); }
                                    if parts.uri.path() == "/v1/pools" { return Ok(Response::new(Body::from(pools.to_string()))); }
                                    if response_mode == "automatic-price-changed" || response_mode == "price-race" {
                                        if response_mode == "price-race" { quotes.store(8, Ordering::SeqCst); }
                                        let mut response = Response::new(Body::from("{\"error\":{\"code\":\"price_changed\"}}"));
                                        *response.status_mut() = hyper::StatusCode::CONFLICT;
                                        return Ok(response);
                                    }
                                }
                                if response_mode == "catalog" || response_mode == "catalog-denied" {
                                    if response_mode == "catalog-denied" && parts.uri.path() == "/v1/pools" {
                                        let mut response = Response::new(Body::from("DO_NOT_DISPLAY_SERVER_BODY"));
                                        *response.status_mut() = hyper::StatusCode::FORBIDDEN;
                                        return Ok(response);
                                    }
                                    let body = match parts.uri.path() {
                                        "/v1/models" => json!({"data":[{"id":"model-a", "router_pools":["pool-a"], "router_capabilities":[{"provider":"openai", "protocols":["responses"]}]}]}),
                                        "/v1/pools" => json!({"data":[{"id":"pool-a", "accounts":[{"id":"account-a", "provider":"openai", "enabled":true, "available":true, "catalog":{"state":"fetched", "visible_models":1}}]}]}),
                                        _ => panic!("unexpected inventory path"),
                                    };
                                    return Ok(Response::new(Body::from(body.to_string())));
                                }
                                let body = if response_mode == "stream" || response_mode == "stall" {
                                    Body::from_stream(futures_util::stream::unfold(0, move |state| async move {
                                        match state {
                                            0 => Some((Ok::<_, std::io::Error>(Bytes::from_static(b"data: first\n\n")), 1)),
                                            1 => {
                                                tokio::time::sleep(Duration::from_secs(if response_mode == "stall" { 10 } else { 1 })).await;
                                                Some((Ok(Bytes::from_static(b"data: [DONE]\n\n")), 2))
                                            }
                                            _ => None,
                                        }
                                    }))
                                } else { Body::from("{\"ok\":true}") };
                                let mut response = Response::new(body);
                                *response.status_mut() = hyper::StatusCode::CREATED;
                                response.headers_mut().insert("content-type", "text/event-stream".parse().unwrap());
                                response.headers_mut().insert("x-request-id", "fixture-request".parse().unwrap());
                                response.headers_mut().insert("connection", "x-hop-only".parse().unwrap());
                                response.headers_mut().insert("x-hop-only", "remove-me".parse().unwrap());
                                Ok(response)
                            }
                        });
                        let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls), service).await;
                    });
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
    });
    Upstream {
        running: Running { url, task },
        calls,
        attestations,
        quote_version,
    }
}

fn config(upstream: &Upstream) -> Config {
    let mut config: Config = toml::from_str(include_str!("../teeproxy.example.toml")).unwrap();
    config.router = upstream.running.url.clone();
    config.listen = "127.0.0.1:0".parse().unwrap();
    config.tdx_policy = None;
    config.max_in_flight = 1;
    config.pool = Some("m_a.pool".into());
    config
}

async fn proxy(mut config: Config, allow_test: bool, auth: Option<&str>) -> Running {
    config.client_key_env = auth.map(|_| "TEST_CLIENT_KEY".into());
    let trust = Trust {
        allow_test_evidence: allow_test,
        ..Trust::default()
    };
    let proxy = Proxy::new(config, trust, "router-key-not-provider-key", auth).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        proxy.serve(listener).await.unwrap();
    });
    Running { url, task }
}

fn agent() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn fixed_pool_reads_latest_multiplier_before_each_request_and_cannot_escape_cap() {
    let upstream = upstream("teeproxy-unit-test", "automatic").await;
    let mut config = config(&upstream);
    config.pool = Some("m_b.pool".into());
    config.routing.max_multiplier_millionths = 500000;
    let gateway = proxy(config, true, None).await;
    let body = r#"{ "model":"model-a", "input":"exact bytes" }"#;
    let response = agent()
        .post(format!("{}/v1/responses", gateway.url))
        .header("x-router-pool", "m_a.pool")
        .header("x-router-account", "bypass")
        .header("x-router-price-version", "1")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-teeproxy-pool"], "m_b.pool");
    assert_eq!(response.headers()["x-teeproxy-multiplier"], "0.5");
    response.bytes().await.unwrap();
    // The merchant changes its multiplier while the same local proxy is running.
    upstream.quote_version.store(8, Ordering::SeqCst);
    let response = agent()
        .post(format!("{}/v1/responses", gateway.url))
        .header("x-router-pool", "m_a.pool")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("routing_multiplier_exceeded")
    );
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 5); // Two fresh inventories; only the first generation was sent.
    let generated: Vec<_> = calls.iter().filter(|c| c.path == "/v1/responses").collect();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].body, body);
    assert_eq!(generated[0].headers["x-router-pool"], "m_b.pool");
    assert_eq!(generated[0].headers["x-router-price-version"], "7");
    assert!(!generated[0].headers.contains_key("x-router-account"));
}

#[tokio::test]
async fn fixed_pool_price_race_is_version_bound_never_replayed_and_next_request_is_blocked() {
    let upstream = upstream("teeproxy-unit-test", "price-race").await;
    let mut config = config(&upstream);
    config.pool = Some("m_b.pool".into());
    config.routing.max_multiplier_millionths = 500000;
    let gateway = proxy(config, true, None).await;
    for status in [409, 403] {
        let response = agent()
            .post(format!("{}/v1/responses", gateway.url))
            .json(&json!({"model":"model-a"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        response.bytes().await.unwrap();
    }
    let calls = upstream.calls.lock().unwrap();
    let generated: Vec<_> = calls.iter().filter(|c| c.path == "/v1/responses").collect();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].headers["x-router-price-version"], "7");
}

#[tokio::test]
async fn fixed_pool_missing_quote_model_protocol_or_selection_never_forwards_generation() {
    for (mode, pool, path, body, status, error) in [
        (
            "inventory-unavailable",
            Some("m_b.pool"),
            "/v1/responses",
            json!({"model":"model-a"}),
            503,
            "routing_inventory_unavailable",
        ),
        (
            "automatic",
            Some("m_b.pool"),
            "/v1/responses",
            json!({"model":"unknown"}),
            503,
            "routing_no_matching_pool",
        ),
        (
            "automatic",
            Some("m_b.pool"),
            "/v1/messages",
            json!({"model":"model-a"}),
            503,
            "routing_no_matching_pool",
        ),
        (
            "automatic",
            Some("m_b.pool"),
            "/v1/responses",
            json!({"model":"m_a/model-a"}),
            503,
            "routing_no_matching_pool",
        ),
        (
            "automatic",
            None,
            "/v1/responses",
            json!({"model":"model-a"}),
            400,
            "routing_pool_required",
        ),
    ] {
        let upstream = upstream("teeproxy-unit-test", mode).await;
        let mut config = config(&upstream);
        config.pool = pool.map(String::from);
        let gateway = proxy(config, true, None).await;
        let response = agent()
            .post(format!("{}{path}", gateway.url))
            .header("x-router-pool", "m_a.pool")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(response.text().await.unwrap().contains(error));
        assert!(
            upstream
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| c.body.is_empty())
        );
    }
}

#[tokio::test]
async fn automatic_routing_uses_attested_quotes_and_overrides_caller_pool_without_rewriting_body() {
    let upstream = upstream("teeproxy-unit-test", "automatic").await;
    let mut config = config(&upstream);
    config.pool = Some("m_c.pool".into());
    config.routing.automatic = true;
    let proxy = proxy(config, true, None).await;
    let body = "{\n  \"model\":\"model-a\", \"input\":\"hello\"\n}";
    let response = agent()
        .post(format!("{}/v1/responses", proxy.url))
        .header("x-router-pool", "m_c.pool")
        .header("x-router-price-version", "1")
        .header("x-router-account", "do-not-pin-an-account")
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-teeproxy-pool"], "m_a.pool");
    assert_eq!(response.headers()["x-teeproxy-multiplier"], "0.2");
    response.bytes().await.unwrap();
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].path, "/v1/models");
    assert_eq!(calls[1].path, "/v1/pools");
    assert_eq!(calls[2].headers["x-router-pool"], "m_a.pool");
    assert_eq!(calls[2].headers["x-router-price-version"], "7");
    assert!(!calls[2].headers.contains_key("x-router-account"));
    assert_eq!(calls[2].body.as_ref(), body.as_bytes());
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn automatic_routing_fails_closed_for_budget_and_price_changes_without_replay() {
    let upstream = upstream("teeproxy-unit-test", "automatic-price-changed").await;
    let mut config = config(&upstream);
    config.routing.automatic = true;
    let gateway = proxy(config.clone(), true, None).await;
    for _ in 0..2 {
        let response = agent()
            .post(format!("{}/v1/responses", gateway.url))
            .json(&json!({"model":"model-a","input":"test"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 409);
        response.bytes().await.unwrap();
    }
    assert_eq!(
        upstream
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.path == "/v1/responses")
            .count(),
        2
    );
    assert_eq!(
        upstream
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.path == "/v1/models")
            .count(),
        2
    );
    config.routing.max_multiplier_millionths = 100000;
    let gateway = proxy(config, true, None).await;
    let response = agent()
        .post(format!("{}/v1/responses", gateway.url))
        .json(&json!({"model":"model-a","input":"test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("routing_no_matching_pool")
    );
    assert_eq!(
        upstream
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.path == "/v1/responses")
            .count(),
        2
    );
}

#[tokio::test]
async fn merchant_credentials_use_the_attested_connection_and_cannot_reach_admin() {
    let upstream = upstream("teeproxy-unit-test", "normal").await;
    let trust = Trust {
        allow_test_evidence: true,
        ..Trust::default()
    };
    let mut session = verify(&upstream.running.url, &trust).await.session.unwrap();
    let response = session
        .send_merchant(
            "GET",
            "/merchant/state",
            "merchant-owned-token",
            b"null".to_vec(),
        )
        .await
        .unwrap();
    to_bytes(Body::new(response.into_body()), MAX_BODY)
        .await
        .unwrap();
    assert!(
        session
            .send_merchant("GET", "/admin/checkpoint", "merchant-owned-token", vec![])
            .await
            .is_err()
    );
    assert!(
        session
            .send_merchant(
                "GET",
                "https://other.invalid/merchant/state",
                "merchant-owned-token",
                vec![]
            )
            .await
            .is_err()
    );
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "/merchant/state");
    assert_eq!(
        calls[0].headers["authorization"],
        "Bearer merchant-owned-token"
    );
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_attestation_cannot_create_a_merchant_session() {
    let upstream = upstream("teeproxy-unit-test", "bad-binding").await;
    let trust = Trust {
        allow_test_evidence: true,
        ..Trust::default()
    };
    assert!(
        verify(&upstream.running.url, &trust)
            .await
            .session
            .is_none()
    );
    assert!(upstream.calls.lock().unwrap().is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn desktop_inventory_reads_models_and_accounts_on_one_attested_connection() {
    let upstream = upstream("teeproxy-unit-test", "catalog").await;
    let trust = Trust {
        allow_test_evidence: true,
        ..Trust::default()
    };
    let catalog = crate::desktop::catalog::fetch_verified(
        &upstream.running.url,
        &trust,
        "inventory-business-key",
    )
    .await
    .unwrap();
    assert_eq!(catalog.models[0].id, "model-a");
    assert_eq!(catalog.accounts[0].id, "account-a");
    assert_eq!(catalog.accounts[0].state, "可用");
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(
        calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["/v1/models", "/v1/pools"]
    );
    for call in calls.iter() {
        assert_eq!(
            call.headers["authorization"],
            "Bearer inventory-business-key"
        );
        assert!(call.body.is_empty());
        assert!(!call.headers.contains_key("x-router-pool"));
    }
}

#[cfg(windows)]
#[tokio::test]
async fn desktop_inventory_rejects_bad_trust_and_does_not_expose_error_bodies() {
    for (kind, mode, allow_test) in [
        ("sample", "catalog", false),
        ("teeproxy-unit-test", "bad-binding", true),
    ] {
        let upstream = upstream(kind, mode).await;
        let trust = Trust {
            allow_test_evidence: allow_test,
            ..Trust::default()
        };
        assert!(
            crate::desktop::catalog::fetch_verified(
                &upstream.running.url,
                &trust,
                "never-send-this-key"
            )
            .await
            .is_err()
        );
        assert!(upstream.calls.lock().unwrap().is_empty());
    }
    let upstream = upstream("teeproxy-unit-test", "catalog-denied").await;
    let trust = Trust {
        allow_test_evidence: true,
        ..Trust::default()
    };
    let result =
        crate::desktop::catalog::fetch_verified(&upstream.running.url, &trust, "business-key")
            .await;
    let error = result.err().unwrap().to_string();
    assert!(error.contains("403"));
    assert!(!error.contains("DO_NOT_DISPLAY_SERVER_BODY"));
}

#[tokio::test]
async fn shutdown_closes_active_streams_and_releases_the_listener() {
    let upstream = upstream("teeproxy-unit-test", "stall").await;
    let config = config(&upstream);
    let trust = Trust {
        allow_test_evidence: true,
        ..Trust::default()
    };
    let gateway = Proxy::new(config, trust, "test-router-key", None).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(gateway.serve_until(listener, async {
        let _ = stopped.await;
    }));
    let mut response = agent()
        .post(format!("http://{address}/v1/responses"))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert!(response.chunk().await.unwrap().is_some());
    stop.send(()).unwrap();
    timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        timeout(Duration::from_secs(2), response.chunk())
            .await
            .unwrap()
            .is_err()
    );
    let listener = TcpListener::bind(address).await.unwrap();
    drop(listener);
}

#[tokio::test]
async fn local_api_verifies_once_preserves_bytes_headers_and_reuses_the_attested_channel() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let mut config = config(&upstream);
    config.pool = Some("m_a.pool".into());
    let proxy = proxy(config, true, None).await;
    let agent = agent();
    let raw = "{ \"model\":\"model-a\", \"input\":\"hello 世界\", \"stream\":true }";
    for _ in 0..2 {
        let response = agent
            .post(format!("{}/v1/responses?beta=true", proxy.url))
            .header("authorization", "Bearer agent-credential-must-not-leak")
            .header("x-api-key", "another-agent-secret")
            .header("cookie", "session=private")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(raw)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        assert_eq!(response.headers()["x-request-id"], "fixture-request");
        assert!(!response.headers().contains_key("x-hop-only"));
        assert_eq!(response.text().await.unwrap(), "{\"ok\":true}");
    }
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 6);
    for call in calls.iter().filter(|c| c.path.starts_with("/v1/responses")) {
        assert_eq!(call.path, "/v1/responses?beta=true");
        assert_eq!(call.body, raw);
        assert_eq!(
            call.headers["authorization"],
            "Bearer router-key-not-provider-key"
        );
        assert_eq!(call.headers["anthropic-version"], "2023-06-01");
        assert_eq!(call.headers["x-router-pool"], "m_a.pool");
        for name in ["cookie", "x-api-key", "proxy-authorization"] {
            assert!(!call.headers.contains_key(name));
        }
    }
}

#[tokio::test]
async fn sample_evidence_bad_binding_and_empty_trust_send_no_business_data() {
    for (kind, mode, allow_test) in [
        ("sample", "json", false),
        ("teeproxy-unit-test", "bad-binding", true),
        ("snp", "json", false),
        ("dstack_tdx", "json", false),
    ] {
        let upstream = upstream(kind, mode).await;
        let proxy = proxy(config(&upstream), allow_test, None).await;
        let response = agent()
            .post(format!("{}/v1/messages", proxy.url))
            .body(r#"{"model":"model-a","input":"never-forward-this-prompt"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        assert!(
            response
                .text()
                .await
                .unwrap()
                .contains("router_attestation_failed")
        );
        assert_eq!(upstream.calls.lock().unwrap().len(), 0);
        assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn sse_is_not_buffered_and_concurrency_is_held_until_stream_finishes() {
    let upstream = upstream("teeproxy-unit-test", "stream").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let agent = agent();
    let mut response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_millis(500), response.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        "data: first\n\n"
    );
    let busy = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(busy.status(), 503);
    assert_eq!(response.text().await.unwrap(), "data: [DONE]\n\n");
    let response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.bytes().await.unwrap();
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn streaming_cancellation_discards_connection_and_releases_capacity() {
    let upstream = upstream("teeproxy-unit-test", "stall").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let agent = agent();
    let mut response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    response.chunk().await.unwrap();
    drop(response);
    timeout(Duration::from_secs(3), async {
        loop {
            let response = agent
                .post(format!("{}/v1/responses", proxy.url))
                .body(r#"{"model":"model-a"}"#)
                .send()
                .await
                .unwrap();
            if response.status() == 201 {
                break;
            }
            assert_eq!(response.status(), 503);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn local_api_gzip_preserves_decompressed_bytes() {
    use std::io::Write;
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(b"{ \"model\":\"model-a\", \"input\": \"exact bytes\" }")
        .unwrap();
    let response = agent()
        .post(format!("{}/v1/responses", proxy.url))
        .header("content-encoding", "gzip")
        .body(gzip.finish().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.bytes().await.unwrap();
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].path, "/v1/responses");
    assert_eq!(
        calls[2].body,
        "{ \"model\":\"model-a\", \"input\": \"exact bytes\" }"
    );
    assert!(!calls[2].headers.contains_key("content-encoding"));
}

#[tokio::test]
async fn proxy_targets_admin_paths_query_credentials_and_websockets_are_blocked() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let agent = agent();
    for (path, code) in [
        ("/admin/checkpoint", 404),
        ("/v1/responses?api_key=secret", 400),
        ("/v1/responses?%6bey=secret", 400),
        ("/v1/responses/compact", 404),
        ("/backend-api/codex/responses", 404),
        ("/v1/%72esponses", 404),
    ] {
        assert_eq!(
            agent
                .post(format!("{}{path}", proxy.url))
                .body(r#"{"model":"model-a"}"#)
                .send()
                .await
                .unwrap()
                .status(),
            code
        );
    }
    assert_eq!(
        agent
            .get(format!("{}/v1/responses", proxy.url))
            .header("upgrade", "websocket")
            .send()
            .await
            .unwrap()
            .status(),
        501
    );
    let address = proxy.url.strip_prefix("http://").unwrap();
    for (target, code) in [
        ("CONNECT api.openai.com:443 HTTP/1.1", "405"),
        ("GET http://api.openai.com/v1/models HTTP/1.1", "400"),
    ] {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!("{target}\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        timeout(Duration::from_secs(3), stream.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {code}")),
            "{response}"
        );
    }
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
    assert!(upstream.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn optional_client_key_accepts_agent_auth_headers_and_never_forwards_them() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let token = "local-client-key-for-test";
    let proxy = proxy(config(&upstream), true, Some(token)).await;
    let agent = agent();
    let url = format!("{}/v1/models", proxy.url);
    for request in [
        agent.get(&url),
        agent.get(&url).bearer_auth("wrong-key"),
        agent
            .get(&url)
            .bearer_auth(token)
            .header("x-api-key", "conflicting-key"),
    ] {
        assert_eq!(request.send().await.unwrap().status(), 401);
    }
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
    for (header, value) in [
        ("authorization", format!("Bearer {token}")),
        ("x-api-key", token.to_owned()),
        ("x-goog-api-key", token.to_owned()),
    ] {
        let response = agent.get(&url).header(header, value).send().await.unwrap();
        assert_eq!(response.status(), 201);
        response.bytes().await.unwrap();
    }
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    for call in calls.iter() {
        assert_eq!(
            call.headers["authorization"],
            "Bearer router-key-not-provider-key"
        );
        assert!(!call.headers.contains_key("x-api-key"));
        assert!(!call.headers.contains_key("x-goog-api-key"));
    }
}

#[test]
fn config_and_policy_fail_closed() {
    let mut config: Config = toml::from_str(include_str!("../teeproxy.example.toml")).unwrap();
    config.validate().unwrap();
    assert!(config.trust().is_err());
    config.listen = "0.0.0.0:8899".parse().unwrap();
    assert!(config.validate().is_err());
    config.listen = "127.0.0.1:8899".parse().unwrap();
    config.router = "http://router.invalid".into();
    assert!(config.validate().is_err());
    config.router = "https://user:secret@router.invalid".into();
    assert!(config.validate().is_err());
    assert!(
        toml::from_str::<Config>(
            &(include_str!("../teeproxy.example.toml").to_owned() + "\nca_dir = \"old-ca\"\n")
        )
        .is_err()
    );
}

#[tokio::test]
async fn plaintext_router_url_never_yields_a_verified_session() {
    assert!(
        verify("http://127.0.0.1:1", &Trust::default())
            .await
            .session
            .is_none()
    );
}

#[tokio::test]
async fn host_mismatch_browser_requests_and_body_limits_fail_before_attestation() {
    use std::io::Write;
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let agent = agent();
    let url = format!("{}/v1/responses", proxy.url);
    for host in ["attacker.invalid", "127.0.0.1:1", "localhost:1"] {
        assert_eq!(
            agent
                .post(&url)
                .header("host", host)
                .body(r#"{"model":"model-a"}"#)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        agent
            .post(&url)
            .header("origin", "https://example.com")
            .body(r#"{"model":"model-a"}"#)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let url = format!("{}/v1/responses", proxy.url);
    assert_eq!(
        agent
            .post(&url)
            .body(vec![b'x'; MAX_BODY + 1])
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&vec![b'x'; MAX_BODY + 1]).unwrap();
    assert_eq!(
        agent
            .post(&url)
            .header("content-encoding", "gzip")
            .body(gzip.finish().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    assert_eq!(
        agent
            .post(&url)
            .header("content-encoding", "br")
            .body(r#"{"model":"model-a"}"#)
            .send()
            .await
            .unwrap()
            .status(),
        415
    );
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
    assert!(upstream.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn idle_stream_timeout_closes_channel_and_releases_capacity() {
    let upstream = upstream("teeproxy-unit-test", "stall").await;
    let mut config = config(&upstream);
    config.stream_idle_secs = 1;
    let proxy = proxy(config, true, None).await;
    let agent = agent();
    let mut response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert!(response.chunk().await.unwrap().is_some());
    assert!(
        timeout(Duration::from_secs(3), response.chunk())
            .await
            .unwrap()
            .is_err()
    );
    let response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unsupported_generation_protocol_cannot_bypass_price_checks() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let response = agent()
        .post(format!(
            "{}/v1beta/models/model-id:streamGenerateContent?alt=sse",
            proxy.url
        ))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("routing_unsupported_protocol")
    );
    assert!(upstream.calls.lock().unwrap().is_empty());
}
