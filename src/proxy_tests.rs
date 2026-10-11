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
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let (socket, _) = socket.unwrap();
                    let acceptor = acceptor.clone(); let certificate = certificate.clone();
                    let captured = captured.clone(); let attests = attests.clone();
                    tasks.spawn(async move {
                        let Ok(tls) = acceptor.accept(socket).await else { return; };
                        let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                            let certificate = certificate.clone(); let captured = captured.clone(); let attests = attests.clone();
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
    }
}

fn config(upstream: &Upstream) -> Config {
    let mut config: Config = toml::from_str(include_str!("../teeproxy.example.toml")).unwrap();
    config.router = upstream.running.url.clone();
    config.listen = "127.0.0.1:0".parse().unwrap();
    config.tdx_policy = None;
    config.max_in_flight = 1;
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
async fn local_api_verifies_once_preserves_bytes_headers_and_reuses_the_attested_channel() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let mut config = config(&upstream);
    config.pool = Some("selected-pool".into());
    let proxy = proxy(config, true, None).await;
    let agent = agent();
    let raw = "{ \"model\":\"literal-model-id\", \"input\":\"hello 世界\", \"stream\":true }";
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
    assert_eq!(calls.len(), 2);
    for call in calls.iter() {
        assert_eq!(call.path, "/v1/responses?beta=true");
        assert_eq!(call.body, raw);
        assert_eq!(
            call.headers["authorization"],
            "Bearer router-key-not-provider-key"
        );
        assert_eq!(call.headers["anthropic-version"], "2023-06-01");
        assert_eq!(call.headers["x-router-pool"], "selected-pool");
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
            .body("never-forward-this-prompt")
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
        .body("{}")
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
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(busy.status(), 503);
    assert_eq!(response.text().await.unwrap(), "data: [DONE]\n\n");
    let response = agent
        .post(format!("{}/v1/responses", proxy.url))
        .body("{}")
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
        .body("{}")
        .send()
        .await
        .unwrap();
    response.chunk().await.unwrap();
    drop(response);
    timeout(Duration::from_secs(3), async {
        loop {
            let response = agent
                .post(format!("{}/v1/responses", proxy.url))
                .body("{}")
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
    gzip.write_all(b"{ \"input\": \"exact bytes\" }").unwrap();
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
    assert_eq!(calls[0].path, "/v1/responses");
    assert_eq!(calls[0].body, "{ \"input\": \"exact bytes\" }");
    assert!(!calls[0].headers.contains_key("content-encoding"));
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
                .body("{}")
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
                .body("{}")
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
            .body("{}")
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
            .body("{}")
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
        .body("{}")
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
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(upstream.attestations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_gemini_path_is_forwarded_without_model_rewriting() {
    let upstream = upstream("teeproxy-unit-test", "json").await;
    let proxy = proxy(config(&upstream), true, None).await;
    let response = agent()
        .post(format!(
            "{}/v1beta/models/model-id:streamGenerateContent?alt=sse",
            proxy.url
        ))
        .header("x-goog-api-key", "agent-secret")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.bytes().await.unwrap();
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(
        calls[0].path,
        "/v1beta/models/model-id:streamGenerateContent?alt=sse"
    );
    assert!(!calls[0].headers.contains_key("x-goog-api-key"));
}
