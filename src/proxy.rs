use crate::{
    ca::LocalCa,
    client::{Trust, VerifiedSession, verify},
    config::{Config, Route},
    protocol::{MAX_BODY, allowed_api},
};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
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
    collections::HashMap,
    convert::Infallible,
    future::Future,
    io::{self, Read},
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
use tokio_rustls::TlsAcceptor;

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
    certificates: HashMap<String, Arc<rustls::ServerConfig>>,
    pool: Arc<SessionPool>,
    requests: Arc<Semaphore>,
    connections: Arc<Semaphore>,
    router_auth: HeaderValue,
    proxy_auth: Option<String>,
}

impl Proxy {
    pub fn new(
        config: Config,
        trust: Trust,
        router_key: &str,
        proxy_token: Option<&str>,
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
        if let Some(token) = proxy_token {
            ensure!(
                token.len() >= 16 && token.len() <= 256 && !token.contains(['\r', '\n']),
                "Proxy token must contain 16 to 256 characters"
            );
        }
        let ca = LocalCa::load(&config.ca_dir)?;
        let certificates = config
            .routes
            .iter()
            .map(|r| Ok((r.host.clone(), ca.tls_for_host(&r.host)?)))
            .collect::<Result<_>>()?;
        Ok(Arc::new(Self {
            certificates,
            requests: Arc::new(Semaphore::new(config.max_in_flight)),
            connections: Arc::new(Semaphore::new(config.max_connections)),
            pool: Arc::new(SessionPool {
                endpoint: config.router.clone(),
                trust,
                idle: Mutex::new(Vec::new()),
            }),
            router_auth,
            proxy_auth: proxy_token
                .map(|t| format!("Basic {}", STANDARD.encode(format!("teeproxy:{t}")))),
            config,
        }))
    }

    pub async fn serve(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        ensure!(
            listener.local_addr()?.ip().is_loopback(),
            "Listener must be loopback"
        );
        loop {
            let (socket, peer) = listener.accept().await?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let Ok(permit) = self.connections.clone().try_acquire_owned() else {
                continue;
            };
            let proxy = self.clone();
            tokio::spawn(async move {
                let permit = Arc::new(permit);
                let guard = permit.clone();
                let service = service_fn(move |request| {
                    let proxy = proxy.clone();
                    let permit = permit.clone();
                    async move { Ok::<_, Infallible>(proxy.handle(request, permit).await) }
                });
                let _guard = guard;
                let _ = http_server()
                    .serve_connection(TokioIo::new(socket), service)
                    .with_upgrades()
                    .await;
            });
        }
    }

    async fn handle(
        self: Arc<Self>,
        mut request: Request<Incoming>,
        permit: Arc<OwnedSemaphorePermit>,
    ) -> Response<ProxyBody> {
        if !self.authorized(request.headers()) {
            let mut response = failure(407, "proxy_authentication_required");
            response.headers_mut().insert(
                "proxy-authenticate",
                HeaderValue::from_static("Basic realm=\"teeproxy\""),
            );
            return response;
        }
        if request.headers().contains_key("origin")
            || request.headers().contains_key("sec-fetch-site")
        {
            return failure(403, "browser_request_rejected");
        }
        if request.method() != Method::CONNECT {
            return self.forward(request, None).await;
        }
        let Some(authority) = request.uri().authority() else {
            return failure(400, "invalid_connect");
        };
        if authority.port_u16() != Some(443)
            || authority.as_str().contains('@')
            || request.uri().scheme().is_some()
            || request.uri().path_and_query().is_some()
            || request.headers().contains_key("transfer-encoding")
            || request
                .headers()
                .get("content-length")
                .is_some_and(|v| v != "0")
        {
            return failure(400, "invalid_connect");
        }
        let host = authority.host().to_ascii_lowercase();
        let Some(tls) = self.certificates.get(&host).cloned() else {
            return failure(403, "host_not_configured");
        };
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(Ok(upgraded)) =
                timeout(Duration::from_secs(10), hyper::upgrade::on(&mut request)).await
            else {
                return;
            };
            let Ok(Ok(stream)) = timeout(
                Duration::from_secs(10),
                TlsAcceptor::from(tls).accept(TokioIo::new(upgraded)),
            )
            .await
            else {
                return;
            };
            if stream
                .get_ref()
                .1
                .server_name()
                .is_some_and(|s| !s.eq_ignore_ascii_case(&host))
            {
                return;
            }
            let service = service_fn(move |request| {
                let proxy = self.clone();
                let host = host.clone();
                async move { Ok::<_, Infallible>(proxy.forward(request, Some(&host)).await) }
            });
            let _ = http_server()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
        Response::new(empty())
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        match &self.proxy_auth {
            None => true,
            Some(expected) => headers
                .get("proxy-authorization")
                .is_some_and(|v| bool::from(v.as_bytes().ct_eq(expected.as_bytes()))),
        }
    }

    async fn forward(
        self: Arc<Self>,
        request: Request<Incoming>,
        tunnel: Option<&str>,
    ) -> Response<ProxyBody> {
        if request.headers().contains_key("origin")
            || request.headers().contains_key("sec-fetch-site")
        {
            return failure(403, "browser_request_rejected");
        }
        if request.headers().contains_key("upgrade") {
            return failure(501, "websocket_not_supported_use_http_sse");
        }
        let (route, uri) = match route_request(&self.config.routes, &request, tunnel) {
            Ok(value) => value,
            Err(code) => return failure(code, "request_not_allowed"),
        };
        let pool_name = route.pool.clone();
        let Ok(permit) = self.requests.clone().try_acquire_owned() else {
            return failure(503, "proxy_busy");
        };
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
        // Credentials and request bytes cannot reach this connection before take()
        // has independently verified evidence and its binding to this exact TLS key.
        let mut session = match self.pool.take().await {
            Ok(session) => session,
            Err(_) => return failure(502, "router_attestation_failed"),
        };
        let mut request = Request::new(Full::new(data));
        *request.method_mut() = parts.method;
        *request.uri_mut() = uri;
        *request.headers_mut() = clean_headers(&parts.headers, true);
        request
            .headers_mut()
            .insert("authorization", self.router_auth.clone());
        if let Some(pool) = pool_name {
            request
                .headers_mut()
                .insert("x-router-pool", pool.parse().expect("validated pool"));
        }
        let response = match session.send_request(request).await {
            Ok(response) => response,
            // Never retry a request once its send has started.
            Err(_) => return failure(502, "router_connection_failed_request_not_replayed"),
        };
        let (parts, body) = response.into_parts();
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

fn route_request<'a>(
    routes: &'a [Route],
    request: &Request<Incoming>,
    tunnel: Option<&str>,
) -> Result<(&'a Route, Uri), u16> {
    let uri = request.uri();
    let host_header = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<http::uri::Authority>().ok())
        .ok_or(400u16)?;
    if host_header.as_str().contains('@') {
        return Err(400);
    }
    let host = host_header.host().to_ascii_lowercase();
    if let Some(expected) = tunnel {
        if host != expected
            || !matches!(host_header.port_u16(), None | Some(443))
            || uri.scheme().is_some()
            || uri.authority().is_some()
        {
            return Err(400);
        }
    } else {
        let authority = uri.authority().ok_or(400u16)?;
        let port = match uri.scheme_str() {
            Some("http") => 80,
            Some("https") => 443,
            _ => return Err(400),
        };
        if authority.as_str().contains('@')
            || !authority.host().eq_ignore_ascii_case(&host)
            || authority.port_u16().unwrap_or(port) != port
            || host_header.port_u16().unwrap_or(port) != port
        {
            return Err(400);
        }
    }
    let route = routes.iter().find(|r| r.host == host).ok_or(403u16)?;
    let suffix = uri
        .path()
        .strip_prefix(&route.prefix)
        .filter(|s| s.starts_with('/'))
        .ok_or(404u16)?;
    let path = format!("{}{suffix}", route.router_prefix);
    if !allowed_api(request.method().as_str(), &path) {
        return Err(404);
    }
    let uri = if let Some(query) = uri.query() {
        let parsed =
            reqwest::Url::parse(&format!("https://local.invalid/?{query}")).map_err(|_| 400u16)?;
        if parsed
            .query_pairs()
            .any(|(key, _)| matches!(key.as_ref(), "key" | "api_key" | "access_token"))
        {
            return Err(400);
        }
        format!("{path}?{query}")
    } else {
        path
    };
    Ok((route, uri.parse().map_err(|_| 400u16)?))
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

fn empty() -> ProxyBody {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed_unsync()
}
fn failure(code: u16, kind: &'static str) -> Response<ProxyBody> {
    let bytes =
        serde_json::to_vec(&serde_json::json!({"error":{"type":kind,"message":kind}})).unwrap();
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
