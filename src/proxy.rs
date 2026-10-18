use crate::{
    client::{Trust, VerifiedSession, verify},
    config::Config,
    protocol::{MAX_BODY, allowed_api},
};
use anyhow::{Result, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited, combinators::UnsyncBoxBody};
use hyper::{
    HeaderMap, Method, Request, Response, StatusCode, Uri,
    body::{Body, Frame, Incoming},
    header::{HeaderName, HeaderValue},
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    future::Future,
    io::{self, Read},
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Sleep, timeout},
};

type ProxyBody = UnsyncBoxBody<Bytes, io::Error>;

struct SessionPool {
    idle: Mutex<Vec<VerifiedSession>>,
    endpoint: String,
    trust: Trust,
}
impl SessionPool {
    async fn take(&self) -> Result<VerifiedSession> {
        {
            let mut idle = self.idle.lock().unwrap();
            while let Some(session) = idle.pop() {
                if session.is_current() {
                    return Ok(session);
                }
            }
        }
        let attempt = verify(&self.endpoint, &self.trust).await;
        attempt
            .session
            .ok_or_else(|| anyhow::anyhow!("Router attestation failed"))
    }
    fn put(&self, session: VerifiedSession) {
        if session.is_current() {
            self.idle.lock().unwrap().push(session);
        }
    }
}

/// An upstream session remains owned by the response body until end-of-stream.
/// Dropping or timing out a stream closes its connection; it cannot enter the pool.
struct ForwardBody {
    inner: Incoming,
    session: Option<VerifiedSession>,
    pool: Arc<SessionPool>,
    permit: Option<OwnedSemaphorePermit>,
    idle: Pin<Box<Sleep>>,
    idle_duration: Duration,
}
impl Body for ForwardBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(None) => {
                if let Some(session) = this.session.take() {
                    this.pool.put(session);
                }
                this.permit.take();
                Poll::Ready(None)
            }
            Poll::Ready(Some(Ok(frame))) => {
                this.idle
                    .as_mut()
                    .reset(tokio::time::Instant::now() + this.idle_duration);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(_))) => {
                this.session.take();
                this.permit.take();
                Poll::Ready(Some(Err(io::Error::other("Router stream interrupted"))))
            }
            Poll::Pending if this.idle.as_mut().poll(cx).is_ready() => {
                this.session.take();
                this.permit.take();
                Poll::Ready(Some(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Router stream idle timeout",
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct Proxy {
    config: Config,
    pool: Arc<SessionPool>,
    requests: Arc<Semaphore>,
    connections: Arc<Semaphore>,
    router_auth: HeaderValue,
    client_key: Option<zeroize::Zeroizing<String>>,
    routing: Arc<crate::routing::State>,
    rules: Mutex<crate::routing::Rules>,
}

impl Proxy {
    pub fn new(
        config: Config,
        trust: Trust,
        router_key: &str,
        client_key: Option<&str>,
    ) -> Result<Arc<Self>> {
        config.validate()?;
        ensure!(
            !router_key.is_empty() && router_key.len() <= 8192,
            "Invalid router key"
        );
        let mut router_auth: HeaderValue = format!("Bearer {router_key}")
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid router key"))?;
        router_auth.set_sensitive(true);
        ensure!(
            config.client_key_env.is_some() == client_key.is_some(),
            "Missing or unexpected local client key"
        );
        if let Some(key) = client_key {
            ensure!(
                key.len() >= 16 && key.len() <= 256 && key.bytes().all(|c| c.is_ascii_graphic()),
                "Client key must contain 16 to 256 visible ASCII characters"
            );
        }
        Ok(Arc::new(Self {
            requests: Arc::new(Semaphore::new(config.max_in_flight)),
            connections: Arc::new(Semaphore::new(config.max_connections)),
            pool: Arc::new(SessionPool {
                endpoint: config.router.clone(),
                trust,
                idle: Mutex::new(Vec::new()),
            }),
            router_auth,
            client_key: client_key.map(|key| zeroize::Zeroizing::new(key.to_owned())),
            routing: Default::default(),
            rules: Mutex::new(crate::routing::Rules::new(
                config.routing.clone(),
                config.pool.clone(),
            )?),
            config,
        }))
    }

    pub async fn serve(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        self.serve_until(listener, std::future::pending()).await
    }
    pub fn observe_routes(&self, observer: Arc<dyn Fn(String) + Send + Sync>) {
        self.routing.observe(observer);
    }

    pub(crate) fn routing_rules(&self) -> crate::routing::Rules {
        self.rules.lock().unwrap().clone()
    }

    /// Prepared and persisted by the UI before replacing the active rules.
    #[cfg(any(windows, test))]
    pub(crate) fn replace_routing(&self, rules: crate::routing::Rules) {
        *self.rules.lock().unwrap() = rules;
    }

    /// Stops accepting and closes all existing client streams before returning.
    pub async fn serve_until(
        self: Arc<Self>,
        listener: TcpListener,
        shutdown: impl Future<Output = ()>,
    ) -> Result<()> {
        let address = listener.local_addr()?;
        ensure!(
            address.ip() == self.config.listen.ip()
                && (self.config.listen.port() == 0 || address.port() == self.config.listen.port()),
            "Listener does not match the configured address"
        );
        // Owning these tasks makes stopping the listener stop existing clients too.
        let mut clients = tokio::task::JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            let (socket, peer) = tokio::select! {
                biased;
                _ = &mut shutdown => break,
                _ = clients.join_next(), if !clients.is_empty() => continue,
                accepted = listener.accept() => accepted?,
            };
            if self.client_key.is_none() && !peer.ip().is_loopback() {
                continue;
            }
            // A wildcard listener has no concrete destination address. Validate
            // Host against the actual local address of this accepted socket.
            let address = socket.local_addr()?;
            let Ok(permit) = self.connections.clone().try_acquire_owned() else {
                continue;
            };
            let proxy = self.clone();
            clients.spawn(async move {
                let _permit = permit;
                let service = service_fn(move |request| {
                    let proxy = proxy.clone();
                    async move { Ok::<_, Infallible>(proxy.forward(request, address).await) }
                });
                let _ = http_server()
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
        drop(listener);
        clients.shutdown().await;
        Ok(())
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = &self.client_key else {
            return true;
        };
        let mut found = false;
        for name in ["authorization", "x-api-key", "x-goog-api-key"] {
            for value in headers.get_all(name) {
                let Ok(value) = value.to_str() else {
                    return false;
                };
                let token = if name == "authorization" {
                    let Some((scheme, token)) = value.split_once(' ') else {
                        return false;
                    };
                    if !scheme.eq_ignore_ascii_case("Bearer") {
                        return false;
                    }
                    token
                } else {
                    value
                };
                if !bool::from(token.as_bytes().ct_eq(expected.as_bytes())) {
                    return false;
                }
                found = true;
            }
        }
        found
    }

    async fn forward(
        self: Arc<Self>,
        request: Request<Incoming>,
        address: SocketAddr,
    ) -> Response<ProxyBody> {
        if request.method() == Method::CONNECT {
            return failure(405, "connect_not_supported_use_api_base_url");
        }
        if !self.authorized(request.headers()) {
            let mut response = failure(401, "client_authentication_required");
            response.headers_mut().insert(
                "www-authenticate",
                HeaderValue::from_static("Bearer realm=\"teeproxy\""),
            );
            return response;
        }
        if is_browser_request(request.headers()) {
            return failure(403, "browser_request_rejected");
        }
        if request.headers().contains_key("upgrade") {
            return failure(501, "websocket_not_supported_use_http_sse");
        }
        let uri = match route_request(&request, address) {
            Ok(value) => value,
            Err(code) => return failure(code, "request_not_allowed"),
        };
        let Ok(permit) = self.requests.clone().try_acquire_owned() else {
            return failure(503, "proxy_busy");
        };
        // Snapshot before awaiting the body or TEE handshake. Live edits only
        // affect later requests, never the validation of this one's response.
        let rules = self.routing_rules();
        let (parts, body) = request.into_parts();
        let data = match timeout(
            Duration::from_secs(30),
            Limited::new(body, MAX_BODY).collect(),
        )
        .await
        {
            Ok(Ok(body)) => body.to_bytes(),
            Ok(Err(_)) => return failure(413, "request_body_limit"),
            Err(_) => return failure(408, "request_body_timeout"),
        };
        let data = match decode_body(data, &parts.headers) {
            Ok(data) => data,
            Err(code) => return failure(code, "invalid_content_encoding_or_body_limit"),
        };
        let rule = if parts.method == Method::POST {
            if !matches!(
                uri.path(),
                "/v1/chat/completions" | "/v1/responses" | "/v1/messages"
            ) {
                return failure(400, "routing_unsupported_protocol");
            }
            match rules.policy.header(rules.pool.as_deref()) {
                Ok(rule) => Some(rule),
                Err(_) => return failure(400, "routing_pool_required"),
            }
        } else {
            None
        };
        // Credentials and request bytes cannot reach this connection before take()
        // has independently verified evidence and its binding to this exact TLS key.
        let mut session = match self.pool.take().await {
            Ok(session) => session,
            Err(_) => return failure(502, "router_attestation_failed"),
        };
        let generation = rule.is_some();
        if generation && session.require_routing_v1().await.is_err() {
            self.routing
                .notify("Router 不支持路由规则 v1，未发送生成请求".into());
            return failure(502, "router_routing_protocol_required");
        }
        let mut request = Request::new(Full::new(data));
        *request.method_mut() = parts.method;
        *request.uri_mut() = uri;
        *request.headers_mut() = clean_headers(&parts.headers, true);
        request
            .headers_mut()
            .insert("authorization", self.router_auth.clone());
        for name in [
            "x-router-routing",
            "x-router-pool",
            "x-router-account",
            "x-router-price-version",
            "x-router-official-price-revision",
        ] {
            request.headers_mut().remove(name);
        }
        if let Some(rule) = rule {
            request.headers_mut().insert("x-router-routing", rule);
        }
        let response_timeout = Duration::from_secs(if generation {
            self.config.response_header_timeout_secs
        } else {
            90
        });
        let response = match session
            .send_request_with_timeout(request, response_timeout)
            .await
        {
            Ok(response) => response,
            // Never retry a request once its send has started.
            Err(error) => {
                if error
                    .downcast_ref::<crate::client::ResponseHeaderTimeout>()
                    .is_some()
                {
                    self.routing.notify(format!(
                        "等待 Router 首个响应超过 {} 秒；请求已发送，未自动重发。",
                        response_timeout.as_secs()
                    ));
                    return failure(504, "router_response_timeout_request_not_replayed");
                }
                self.routing
                    .notify("Router 连接中断；请求处理结果未知，未自动重发。".into());
                return failure(502, "router_connection_failed_request_not_replayed");
            }
        };
        let (parts, body) = response.into_parts();
        let selected = if generation && parts.status.is_success() {
            let pool = parts
                .headers
                .get("x-router-pool")
                .and_then(|v| v.to_str().ok());
            let rate = parts
                .headers
                .get("x-router-multiplier-millionths")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            match (pool, rate) {
                (Some(pool), Some(rate))
                    if parts
                        .headers
                        .get("x-router-routing-version")
                        .is_some_and(|v| v == "1")
                        && !pool.is_empty()
                        && pool.len() <= 128
                        && pool
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                        && rate <= rules.policy.max_multiplier_millionths
                        && (rules.policy.automatic || rules.pool.as_deref() == Some(pool)) =>
                {
                    self.routing.notify(format!(
                        "Router 已选择 {} · {}×",
                        pool,
                        crate::routing::format_multiplier(rate)
                    ));
                    Some((pool.to_owned(), rate))
                }
                _ => return failure(502, "router_routing_result_invalid"),
            }
        } else {
            if generation {
                self.routing
                    .notify(format!("Router 拒绝请求：HTTP {}", parts.status.as_u16()));
            }
            None
        };
        let duration = Duration::from_secs(self.config.stream_idle_secs);
        let body = ForwardBody {
            inner: body,
            session: Some(session),
            pool: self.pool.clone(),
            permit: Some(permit),
            idle: Box::pin(tokio::time::sleep(duration)),
            idle_duration: duration,
        };
        let mut response = Response::new(body.boxed_unsync());
        *response.status_mut() = parts.status;
        *response.headers_mut() = clean_headers(&parts.headers, false);
        response
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
        if let Some((pool, multiplier)) = selected {
            response
                .headers_mut()
                .insert("x-teeproxy-pool", pool.parse().expect("validated pool"));
            response.headers_mut().insert(
                "x-teeproxy-multiplier",
                crate::routing::format_multiplier(multiplier)
                    .parse()
                    .expect("formatted multiplier"),
            );
        }
        response
    }
}

fn http_server() -> hyper::server::conn::http1::Builder {
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(15));
    builder.max_buf_size(32 * 1024).max_headers(100);
    builder
}

fn is_browser_request(headers: &HeaderMap) -> bool {
    if headers.contains_key("origin") {
        return true;
    }
    if !headers.contains_key("sec-fetch-site") {
        return false;
    }
    // Electron's native net.fetch (including VS Code Copilot) sends Fetch
    // Metadata without a web origin. Only accept its API-request shape; page
    // requests, navigation, ambiguous duplicates and partial metadata stay denied.
    ![
        ("sec-fetch-site", &["none"][..]),
        ("sec-fetch-mode", &["no-cors", "cors", "same-origin"][..]),
        ("sec-fetch-dest", &["empty"][..]),
    ]
    .iter()
    .all(|(name, allowed)| {
        let mut values = headers.get_all(*name).iter();
        values
            .next()
            .is_some_and(|value| allowed.iter().any(|expected| value == *expected))
            && values.next().is_none()
    })
}

fn route_request(request: &Request<Incoming>, address: SocketAddr) -> Result<Uri, u16> {
    let uri = request.uri();
    // Accept origin-form requests to this listener, never forward-proxy targets.
    if uri.scheme().is_some() || uri.authority().is_some() || !uri.path().starts_with('/') {
        return Err(400);
    }
    if request.headers().get_all("host").iter().count() != 1 {
        return Err(400);
    }
    let authority = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<http::uri::Authority>().ok())
        .ok_or(400u16)?;
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let matches_listener = (address.ip().is_loopback() && host.eq_ignore_ascii_case("localhost"))
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip == address.ip());
    if authority.as_str().contains('@')
        || !matches_listener
        || authority.port_u16().unwrap_or(80) != address.port()
    {
        return Err(400);
    }
    if !allowed_api(request.method().as_str(), uri.path()) {
        return Err(404);
    }
    if let Some(query) = uri.query() {
        let parsed =
            reqwest::Url::parse(&format!("https://local.invalid/?{query}")).map_err(|_| 400u16)?;
        if parsed
            .query_pairs()
            .any(|(key, _)| matches!(key.as_ref(), "key" | "api_key" | "access_token"))
        {
            return Err(400);
        }
    }
    Ok(uri.clone())
}

fn clean_headers(headers: &HeaderMap, request: bool) -> HeaderMap {
    let mut result = headers.clone();
    for value in headers.get_all("connection") {
        if let Ok(value) = value.to_str() {
            for name in value.split(',').map(str::trim) {
                if let Ok(name) = HeaderName::from_bytes(name.as_bytes()) {
                    result.remove(name);
                }
            }
        }
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "content-length",
    ] {
        result.remove(name);
    }
    if request {
        for name in [
            "host",
            "authorization",
            "x-api-key",
            "x-goog-api-key",
            "cookie",
            "cookie2",
            "openai-organization",
            "openai-project",
            "chatgpt-account-id",
            "forwarded",
            "x-forwarded-for",
            "x-forwarded-host",
            "content-encoding",
            "expect",
            "origin",
            "sec-fetch-site",
            "sec-fetch-mode",
            "sec-fetch-dest",
            "sec-fetch-user",
        ] {
            result.remove(name);
        }
    }
    result
}

fn decode_body(bytes: Bytes, headers: &HeaderMap) -> Result<Bytes, u16> {
    if headers.get_all("content-encoding").iter().count() > 1 {
        return Err(415);
    }
    match headers
        .get("content-encoding")
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| 415u16)?
    {
        None | Some("identity") => Ok(bytes),
        Some("gzip") => {
            let mut output = Vec::new();
            flate2::read::MultiGzDecoder::new(bytes.as_ref())
                .take(MAX_BODY as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|_| 400u16)?;
            if output.len() > MAX_BODY {
                return Err(413);
            }
            Ok(output.into())
        }
        _ => Err(415),
    }
}

fn failure(code: u16, kind: &'static str) -> Response<ProxyBody> {
    let message = match kind {
        "router_response_timeout_request_not_replayed" => {
            "Timed out waiting for Router response; request was sent and was not replayed"
        }
        "router_connection_failed_request_not_replayed" => {
            "Router connection interrupted; request outcome is unknown and request was not replayed"
        }
        _ => kind,
    };
    let bytes = serde_json::to_vec(
        &serde_json::json!({"error":{"type":kind,"code":kind,"message":message}}),
    )
    .unwrap();
    let mut response = Response::new(
        Full::new(Bytes::from(bytes))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = StatusCode::from_u16(code).unwrap();
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
