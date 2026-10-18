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
    release_headers: Arc<tokio::sync::Notify>,
    generation_seen: Arc<tokio::sync::Notify>,
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
    let release_headers = Arc::new(tokio::sync::Notify::new());
    let gate = release_headers.clone();
    let generation_seen = Arc::new(tokio::sync::Notify::new());
    let seen = generation_seen.clone();
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let (socket, _) = socket.unwrap();
                    let acceptor = acceptor.clone(); let certificate = certificate.clone();
                    let captured = captured.clone(); let attests = attests.clone(); let gate = gate.clone(); let seen = seen.clone();
                    tasks.spawn(async move {
                        let Ok(tls) = acceptor.accept(socket).await else { return; };
                        let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                            let certificate = certificate.clone(); let captured = captured.clone(); let attests = attests.clone(); let gate = gate.clone(); let seen = seen.clone();
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
                                let rule = parts.headers.get("x-router-routing").map(|v|serde_json::from_slice::<serde_json::Value>(v.as_bytes()).unwrap());
                                let hold = response_mode == "live-update" && serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|v|v["input"] == "hold");
                                captured.lock().unwrap().push(Captured { path: parts.uri.to_string(), headers: parts.headers, body: bytes });
                                if rule.is_some() { seen.notify_one(); }
                                if hold { gate.notified().await; }
                                if parts.uri.path() == "/health/live" {
                                    let health = if response_mode == "old-router" { json!({"status":"ok"}) } else { json!({"status":"ok","routing_protocol_versions":[1]}) };
                                    return Ok(Response::new(Body::from(health.to_string())));
                                }
                                if response_mode == "rule-denied" {
                                    let mut response = Response::new(Body::from(r#"{"error":{"code":"routing_multiplier_exceeded"}}"#));
                                    *response.status_mut() = hyper::StatusCode::FORBIDDEN;
                                    return Ok(response);
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
                                        "/v1/pricing" => json!({"revision":9,"currency":"USD","unit":"micro_usd_per_million_tokens","providers":[]}),
                                        _ => panic!("unexpected inventory path"),
                                    };
                                    return Ok(Response::new(Body::from(body.to_string())));
                                }
                                let body = if matches!(response_mode, "stream" | "stall" | "live-update") {
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
                                if let Some(rule) = rule {
                                    let pool = if rule["mode"] == "auto" { "m_router.selected" } else { rule["pool"].as_str().unwrap() };
                                    response.headers_mut().insert("x-router-pool",pool.parse().unwrap());
                                    let rate = if response_mode == "live-update" { rule["max_multiplier_millionths"].as_u64().unwrap().min(200000) } else { 200000 };
                                    response.headers_mut().insert("x-router-multiplier-millionths", rate.into());
                                    response.headers_mut().insert("x-router-price-version", "9".parse().unwrap());
                                    if response_mode != "unconfirmed-rule" { response.headers_mut().insert("x-router-routing-version", "1".parse().unwrap()); }
                                }
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
        release_headers,
        generation_seen,
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
    let listen = config.listen;
    let trust = Trust {
        allow_test_evidence: allow_test,
        ..Trust::default()
    };
    let proxy = Proxy::new(config, trust, "router-key-not-provider-key", auth).unwrap();
    let listener = TcpListener::bind(listen).await.unwrap();
    let url = crate::config::local_api_url(listener.local_addr().unwrap())
        .trim_end_matches("/v1")
        .to_owned();
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
async fn live_rules_apply_to_new_requests_without_changing_pending_or_streaming_requests() {
    use crate::routing::{Policy, Priority, Rules};
    let upstream = upstream("teeproxy-unit-test", "live-update").await;
    let mut config = config(&upstream);
    config.max_in_flight = 3;
    let gateway = Proxy::new(
        config,
        Trust {
            allow_test_evidence: true,
            ..Trust::default()
        },
        "key",
        None,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let server = gateway.clone();
    let _running = Running {
        url: url.clone(),
        task: tokio::spawn(async move {
            server.serve(listener).await.unwrap();
        }),
    };
    let client = agent();
    let pending = client
        .post(&url)
        .json(&json!({"model":"same-model","input":"hold"}))
        .send();
    let pending = tokio::spawn(pending);
    timeout(Duration::from_secs(3), upstream.generation_seen.notified())
        .await
        .unwrap();
    // The old request has reached the router but has no response headers yet.
    gateway.replace_routing(
        Rules::new(
            Policy {
                max_multiplier_millionths: 100000,
                ..Policy::default()
            },
            Some("m_new.pool".into()),
        )
        .unwrap(),
    );
    let mut streaming = client
        .post(&url)
        .json(&json!({"model":"same-model"}))
        .send()
        .await
        .unwrap();
    assert_eq!(streaming.status(), 201);
    assert_eq!(streaming.headers()["x-teeproxy-pool"], "m_new.pool");
    assert_eq!(streaming.chunk().await.unwrap().unwrap(), "data: first\n\n");
    // Another save while this response streams must not close either connection.
    gateway.replace_routing(
        Rules::new(
            Policy {
                automatic: true,
                max_multiplier_millionths: 50000,
                priority: Priority::Speed,
            },
            None,
        )
        .unwrap(),
    );
    upstream.release_headers.notify_one();
    let old = pending.await.unwrap().unwrap();
    assert_eq!(old.status(), 201);
    assert_eq!(old.headers()["x-teeproxy-pool"], "m_a.pool");
    assert_eq!(old.headers()["x-teeproxy-multiplier"], "0.2");
    assert!(old.text().await.unwrap().contains("[DONE]"));
    assert!(streaming.text().await.unwrap().contains("[DONE]"));
    let newest = client
        .post(&url)
        .json(&json!({"model":"same-model"}))
        .send()
        .await
        .unwrap();
    assert_eq!(newest.status(), 201);
    assert_eq!(newest.headers()["x-teeproxy-pool"], "m_router.selected");
    assert_eq!(newest.headers()["x-teeproxy-multiplier"], "0.05");
    assert!(newest.text().await.unwrap().contains("[DONE]"));
    let calls = upstream.calls.lock().unwrap();
    let rules: Vec<serde_json::Value> = calls
        .iter()
        .filter(|c| c.path == "/v1/responses")
        .map(|c| serde_json::from_slice(c.headers["x-router-routing"].as_bytes()).unwrap())
        .collect();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0]["pool"], "m_a.pool");
    assert_eq!(rules[0]["max_multiplier_millionths"], 1000000);
    assert_eq!(rules[1]["pool"], "m_new.pool");
    assert_eq!(rules[1]["max_multiplier_millionths"], 100000);
    assert_eq!(rules[2]["mode"], "auto");
    assert_eq!(rules[2]["priority"], "speed");
    assert_eq!(rules[2]["max_multiplier_millionths"], 50000);
    assert!(rules[2].get("pool").is_none());
}

#[tokio::test]
async fn rules_are_sent_without_local_catalog_selection_and_router_result_is_displayed() {
    for automatic in [false, true] {
        let upstream = upstream("teeproxy-unit-test", "json").await;
        let mut config = config(&upstream);
        config.routing.automatic = automatic;
        config.pool = Some("m_b.pool".into());
        config.routing.max_multiplier_millionths = 500000;
        config.routing.priority = crate::routing::Priority::Speed;
        let gateway = proxy(config, true, None).await;
        let raw = r#"{ "model":"agent-model", "input":"hello", "stream":true }"#;
        let response = agent()
            .post(format!("{}/v1/responses", gateway.url))
            .header("x-router-pool", "override")
            .header("x-router-routing", r#"{"version":99}"#)
            .header("x-router-price-version", "99")
            .header("x-router-account", "override")
            .header("x-router-official-price-revision", "99")
            .body(raw)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        assert_eq!(
            response.headers()["x-teeproxy-pool"],
            if automatic {
                "m_router.selected"
            } else {
                "m_b.pool"
            }
        );
        assert_eq!(response.headers()["x-teeproxy-multiplier"], "0.2");
        response.bytes().await.unwrap();
        let calls = upstream.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].path, "/health/live");
        assert!(calls[0].body.is_empty());
        assert!(!calls[0].headers.contains_key("authorization"));
        let sent = &calls[1];
        assert_eq!(sent.body, raw);
        let rule: serde_json::Value =
            serde_json::from_slice(sent.headers["x-router-routing"].as_bytes()).unwrap();
        assert_eq!(rule["version"], 1);
        assert_eq!(rule["max_multiplier_millionths"], 500000);
        assert_eq!(rule["priority"], "speed");
        assert_eq!(rule["mode"], if automatic { "auto" } else { "fixed" });
        assert_eq!(
            rule.get("pool").and_then(serde_json::Value::as_str),
            if automatic { None } else { Some("m_b.pool") }
        );
        for name in [
            "x-router-pool",
            "x-router-account",
            "x-router-price-version",
            "x-router-official-price-revision",
        ] {
            assert!(!sent.headers.contains_key(name));
        }
    }
}
#[tokio::test]
async fn router_rejections_are_forwarded_once_and_old_routers_receive_no_generation() {
    for (mode, status, code, count) in [
        ("rule-denied", 403, "routing_multiplier_exceeded", 2),
        ("old-router", 502, "router_routing_protocol_required", 1),
        ("unconfirmed-rule", 502, "router_routing_result_invalid", 2),
    ] {
        let upstream = upstream("teeproxy-unit-test", mode).await;
        let gateway = proxy(config(&upstream), true, None).await;
        let response = agent()
            .post(format!("{}/v1/responses", gateway.url))
            .json(&json!({"model":"model-a"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(response.text().await.unwrap().contains(code));
        assert_eq!(upstream.calls.lock().unwrap().len(), count);
    }
}
#[tokio::test]
async fn fixed_mode_requires_a_configured_pool_not_a_caller_header() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let mut config = config(&upstream);
    config.pool = None;
    let gateway = proxy(config, true, None).await;
    let response = agent()
        .post(format!("{}/v1/responses", gateway.url))
        .header("x-router-pool", "m_a.pool")
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
            .contains("routing_pool_required")
    );
    assert!(upstream.calls.lock().unwrap().is_empty());
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

#[tokio::test]
async fn official_prices_are_read_only_without_pool_selection_or_routing_headers() {
    let upstream = upstream("teeproxy-unit-test", "catalog").await;
    let mut config = config(&upstream);
    config.pool = None;
    config.routing.automatic = false;
    let gateway = proxy(config, true, None).await;
    let response = agent()
        .get(format!("{}/v1/pricing", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["revision"],
        9
    );
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 1);
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "/v1/pricing");
    assert!(!calls[0].headers.contains_key("x-router-routing"));
    assert!(!calls[0].headers.contains_key("x-router-pool"));
    assert!(!allowed_api("PUT", "/v1/pricing"));
    assert!(!allowed_api("GET", "/admin/pricing"));
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
    assert_eq!(calls.len(), 3);
    for call in calls.iter().filter(|c| c.path.starts_with("/v1/responses")) {
        assert_eq!(call.path, "/v1/responses?beta=true");
        assert_eq!(call.body, raw);
        assert_eq!(
            call.headers["authorization"],
            "Bearer router-key-not-provider-key"
        );
        assert_eq!(call.headers["anthropic-version"], "2023-06-01");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                call.headers["x-router-routing"].as_bytes()
            )
            .unwrap()["pool"],
            "m_a.pool"
        );
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
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].path, "/v1/responses");
    assert_eq!(
        calls[1].body,
        "{ \"model\":\"model-a\", \"input\": \"exact bytes\" }"
    );
    assert!(!calls[1].headers.contains_key("content-encoding"));
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
    config.client_key_env = Some("".into());
    assert!(config.validate().is_err());
    config.client_key_env = Some("LOCAL_KEY".into());
    for address in [
        "0.0.0.0:8899",
        "192.168.1.6:8899",
        "[::]:8899",
        "[::1]:8899",
    ] {
        config.listen = address.parse().unwrap();
        config.validate().unwrap();
        assert!(Proxy::new(config.clone(), Trust::default(), "test-router-key", None).is_err());
        assert!(
            Proxy::new(
                config.clone(),
                Trust::default(),
                "test-router-key",
                Some("")
            )
            .is_err()
        );
    }
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
async fn wildcard_listener_requires_auth_and_checks_the_accepted_socket_host() {
    for bind in ["0.0.0.0:0", "[::]:0"] {
        let upstream = upstream("teeproxy-unit-test", "json").await;
        let mut config = config(&upstream);
        config.listen = bind.parse().unwrap();
        let token = "wildcard-local-client-key";
        let proxy = proxy(config, true, Some(token)).await;
        let agent = agent();
        let url = format!("{}/v1/models", proxy.url);
        for request in [agent.get(&url), agent.get(&url).bearer_auth("wrong-key")] {
            assert_eq!(request.send().await.unwrap().status(), 401);
        }
        let port = reqwest::Url::parse(&url).unwrap().port().unwrap();
        for host in [
            format!("0.0.0.0:{port}"),
            format!("[::]:{port}"),
            format!("attacker.invalid:{port}"),
        ] {
            assert_eq!(
                agent
                    .get(&url)
                    .bearer_auth(token)
                    .header("host", host)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                400
            );
        }
        assert_eq!(
            agent
                .get(&url)
                .bearer_auth(token)
                .header("origin", "https://attacker.invalid")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
        assert!(upstream.calls.lock().unwrap().is_empty());
        let response = agent.get(&url).bearer_auth(token).send().await.unwrap();
        assert_eq!(response.status(), 201);
        response.bytes().await.unwrap();
        let calls = upstream.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].headers["authorization"],
            "Bearer router-key-not-provider-key"
        );
    }
}

#[tokio::test]
async fn serving_a_different_listener_cannot_bypass_configured_bind_restrictions() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = Proxy::new(config(&upstream), Trust::default(), "router-key", None).unwrap();
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    assert!(proxy.serve(listener).await.is_err());
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
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
async fn electron_native_fetch_metadata_allows_copilot_and_preserves_local_auth() {
    for key in [None, Some("local-key-at-least-16-characters")] {
        let upstream = upstream("teeproxy-unit-test", "json").await;
        let gateway = proxy(config(&upstream), true, key).await;
        let client = agent();
        let url = format!("{}/v1/chat/completions", gateway.url);
        for mode in ["no-cors", "cors", "same-origin"] {
            let body = r#"{ "model":"model-a", "messages":[{"role":"user","content":"hello"}] }"#;
            let response = client
                .post(&url)
                .header(
                    "authorization",
                    format!("Bearer {}", key.unwrap_or("teeproxy-local")),
                )
                .header("user-agent", "GitHubCopilotChat/test")
                .header("x-vscode-user-agent-library-version", "electron-fetch")
                .header("sec-fetch-site", "none")
                .header("sec-fetch-mode", mode)
                .header("sec-fetch-dest", "empty")
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 201);
            response.bytes().await.unwrap();
            let calls = upstream.calls.lock().unwrap();
            let sent = calls.last().unwrap();
            assert_eq!(sent.path, "/v1/chat/completions");
            assert_eq!(sent.body, body);
            assert_eq!(
                sent.headers["authorization"],
                "Bearer router-key-not-provider-key"
            );
            for name in [
                "origin",
                "sec-fetch-site",
                "sec-fetch-mode",
                "sec-fetch-dest",
                "sec-fetch-user",
            ] {
                assert!(!sent.headers.contains_key(name));
            }
        }
        if key.is_some() {
            let calls_before = upstream.calls.lock().unwrap().len();
            let response = client
                .post(&url)
                .header("authorization", "Bearer wrong-key")
                .header("sec-fetch-site", "none")
                .header("sec-fetch-mode", "no-cors")
                .header("sec-fetch-dest", "empty")
                .body("{}")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 401);
            assert_eq!(upstream.calls.lock().unwrap().len(), calls_before);
        }
    }
}

#[tokio::test]
async fn electron_exception_does_not_allow_web_origins_navigation_or_ambiguous_metadata() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let gateway = proxy(config(&upstream), true, None).await;
    let client = agent();
    let url = format!("{}/v1/chat/completions", gateway.url);
    let native: hyper::HeaderMap = hyper::HeaderMap::from_iter([
        (
            hyper::header::HeaderName::from_static("sec-fetch-site"),
            "none".parse().unwrap(),
        ),
        (
            hyper::header::HeaderName::from_static("sec-fetch-mode"),
            "no-cors".parse().unwrap(),
        ),
        (
            hyper::header::HeaderName::from_static("sec-fetch-dest"),
            "empty".parse().unwrap(),
        ),
    ]);
    let mut rejected = Vec::new();
    for (name, values) in [
        (
            "origin",
            &[
                "https://example.com",
                "null",
                "",
                "vscode-file://vscode-app",
            ][..],
        ),
        (
            "sec-fetch-site",
            &[
                "cross-site",
                "same-site",
                "same-origin",
                "none, cross-site",
                "null",
                "",
            ][..],
        ),
        (
            "sec-fetch-mode",
            &[
                "navigate",
                "nested-navigate",
                "websocket",
                "no-cors, navigate",
            ][..],
        ),
        (
            "sec-fetch-dest",
            &["document", "iframe", "image", "empty, document"][..],
        ),
    ] {
        for value in values {
            let mut headers = native.clone();
            headers.insert(name, value.parse().unwrap());
            rejected.push(headers);
        }
    }
    for name in ["sec-fetch-mode", "sec-fetch-dest"] {
        let mut headers = native.clone();
        headers.remove(name);
        rejected.push(headers);
    }
    for name in ["sec-fetch-site", "sec-fetch-mode", "sec-fetch-dest"] {
        let mut headers = native.clone();
        headers.append(name, native[name].clone());
        rejected.push(headers);
    }
    for headers in rejected {
        let response = client
            .post(&url)
            .headers(headers)
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
        assert!(
            response
                .text()
                .await
                .unwrap()
                .contains("browser_request_rejected")
        );
    }
    let preflight = client
        .request(reqwest::Method::OPTIONS, &url)
        .header("origin", "https://example.com")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), 403);
    assert!(
        !preflight
            .headers()
            .contains_key("access-control-allow-origin")
    );
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 0);
    assert!(upstream.calls.lock().unwrap().is_empty());
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
async fn header_timeout_is_explicit_and_never_replays_the_generation() {
    let upstream = upstream("teeproxy-unit-test", "live-update").await;
    let mut config = config(&upstream);
    config.response_header_timeout_secs = 1;
    let proxy = proxy(config, true, None).await;
    let agent = agent();
    let response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a","input":"hold"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 504);
    let error: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        error["error"]["code"],
        "router_response_timeout_request_not_replayed"
    );
    assert_eq!(
        upstream
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/responses")
            .count(),
        1
    );

    // The timed-out TLS session is discarded and its local concurrency permit freed.
    let response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body(r#"{"model":"model-a","input":"next"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 2);
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
