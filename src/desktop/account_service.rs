//! Loopback account UI, independent of the possibly LAN-facing model proxy.
use super::Event;
use anyhow::{Result, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, Response, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    sync::{Arc, mpsc},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Semaphore},
    task::JoinHandle,
};
use zeroize::Zeroizing;

type Reply = Response<Full<Bytes>>;
#[derive(Default)]
struct Session {
    cookie: Zeroizing<String>,
    csrf: Zeroizing<String>,
}
struct Server {
    origin: String,
    platform: String,
    local: String,
    csrf: String,
    client: reqwest::Client,
    session: Mutex<Session>,
    sender: mpsc::Sender<Event>,
}
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| anyhow::anyhow!("随机数生成失败"))?;
    Ok(bytes.iter().map(|n| format!("{n:02x}")).collect())
}
pub(super) fn platform(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "平台地址必须是 HTTPS 根地址"
    );
    Ok(url.as_str().trim_end_matches('/').to_owned())
}
pub(super) async fn start(
    base: &str,
    sender: mpsc::Sender<Event>,
) -> Result<(String, JoinHandle<()>)> {
    let platform = platform(base)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = Arc::new(Server {
        origin: origin.clone(),
        platform,
        local: random()?,
        csrf: random()?,
        sender,
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()?,
        session: Mutex::new(Session::default()),
    });
    let task = tokio::spawn(async move {
        let slots = Arc::new(Semaphore::new(16));
        while let Ok((socket, peer)) = listener.accept().await {
            if !peer.ip().is_loopback() {
                continue;
            }
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let server = server.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let service = service_fn(move |request| {
                    let server = server.clone();
                    async move {
                        let reply = server.handle(request).await.unwrap_or_else(|_| {
                            response(
                                502,
                                json!({"error":"平台连接失败，请刷新重试"}).to_string(),
                                "application/json",
                            )
                        });
                        Ok::<_, Infallible>(reply)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .header_read_timeout(Duration::from_secs(10))
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    Ok((origin + "/account", task))
}
fn response(status: u16, data: impl Into<Bytes>, mime: &str) -> Reply {
    Response::builder().status(status).header("Content-Type",mime).header("Cache-Control","no-store")
      .header("X-Content-Type-Options","nosniff").header("Referrer-Policy","no-referrer")
      .header("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")
      .body(Full::new(data.into())).unwrap()
}
fn denied(status: u16, message: &str) -> Reply {
    response(
        status,
        json!({"error":message}).to_string(),
        "application/json",
    )
}
fn asset(name: &str) -> Option<(&'static str, &'static str)> {
    macro_rules! file {
        ($name:literal) => {
            include_str!(concat!("../../resources/account/", $name))
        };
    }
    let mime = if name.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if name.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "text/javascript; charset=utf-8"
    };
    Some((
        match name {
            "index.html" => file!("index.html"),
            "login.html" => file!("login.html"),
            "register.html" => file!("register.html"),
            "common.css" => file!("common.css"),
            "local.css" => file!("local.css"),
            "browser-utils.js" => file!("browser-utils.js"),
            "common.js" => file!("common.js"),
            "finance.js" => file!("finance.js"),
            "payments.js" => file!("payments.js"),
            "workspace-receipts.js" => file!("workspace-receipts.js"),
            "user.js" => file!("user.js"),
            "workspace.js" => file!("workspace.js"),
            "local.js" => file!("local.js"),
            "auth.js" => file!("auth.js"),
            _ => return None,
        },
        mime,
    ))
}
fn allowed(method: &str, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    match (method, parts.as_slice()) {
        ("GET", ["info" | "me" | "finance" | "payments"]) => true,
        (
            "POST",
            [
                "login" | "register" | "logout" | "password" | "payments" | "keys" | "use-key"
                | "access",
            ],
        ) => true,
        ("GET", ["receipts", "mine"]) => true,
        ("GET", ["receipts", "records", seq]) => {
            !seq.is_empty() && seq.bytes().all(|c| c.is_ascii_digit())
        }
        ("GET", ["payments", id])
        | ("GET", ["payments", id, "qr.svg"])
        | ("POST", ["payments", id, "refresh"])
        | ("POST", ["keys", id, "reveal"])
        | ("DELETE", ["keys", id]) => {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        }
        _ => false,
    }
}
impl Server {
    async fn handle(&self, request: Request<Incoming>) -> Result<Reply> {
        let headers = request.headers();
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
        };
        if text("host") != self.origin.trim_start_matches("http://")
            || headers.get_all("host").iter().count() != 1
            || (!text("origin").is_empty() && text("origin") != self.origin)
            || !["", "none", "same-origin"].contains(&text("sec-fetch-site"))
        {
            return Ok(denied(403, "访问来源不匹配"));
        }
        let path = request.uri().path().to_owned();
        let method = request.method().as_str().to_owned();
        if method == "GET"
            && ["/account", "/account/login", "/account/register"].contains(&path.as_str())
        {
            let file = match path.as_str() {
                "/account/login" => "login.html",
                "/account/register" => "register.html",
                _ => "index.html",
            };
            let (html, mime) = asset(file).unwrap();
            let mut result = response(
                200,
                html.replace("LOCAL_CSRF", &self.csrf)
                    .replace("MERCHANT_ID", "")
                    .replace("ACCOUNT_MODE", "account"),
                mime,
            );
            result.headers_mut().insert(
                "Set-Cookie",
                format!(
                    "tp_account={}; HttpOnly; SameSite=Strict; Path=/",
                    self.local
                )
                .parse()?,
            );
            return Ok(result);
        }
        let session_cookie = format!("tp_account={}", self.local);
        if !text("cookie")
            .split(';')
            .any(|p| p.trim() == session_cookie)
        {
            return Ok(denied(401, "请从 teeproxy 打开我的账户"));
        }
        if method == "GET"
            && let Some(name) = path.strip_prefix("/account-assets/")
        {
            return Ok(match asset(name) {
                Some((data, mime)) => response(200, data, mime),
                None => denied(404, "not_found"),
            });
        }
        let Some(name) = path.strip_prefix("/account-api/") else {
            return Ok(denied(404, "not_found"));
        };
        if !allowed(&method, name) || request.uri().query().is_some_and(|q| q.len() > 2048) {
            return Ok(denied(404, "not_found"));
        }
        let image = method == "GET" && name.ends_with("/qr.svg");
        if !image && text("x-csrf-token") != self.csrf {
            return Ok(denied(403, "页面会话无效，请刷新"));
        }
        let operation = text("idempotency-key").to_owned();
        let suffix = request
            .uri()
            .path_and_query()
            .unwrap()
            .as_str()
            .trim_start_matches("/account-api/")
            .to_owned();
        let body = match Limited::new(request.into_body(), 64 * 1024).collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => return Ok(denied(413, "request_too_large")),
        };
        let mut data: Value = if body.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(&body)?
        };
        if name == "register" && data.get("role").and_then(Value::as_str) != Some("user") {
            return Ok(denied(403, "商户注册请使用 teemerchant"));
        }
        let mut session = self.session.lock().await;
        let route = if name == "use-key" {
            let id = data.get("id").and_then(Value::as_str).unwrap_or("");
            let route = format!("keys/{id}/reveal");
            if !allowed("POST", &route) {
                return Ok(denied(422, "invalid_key"));
            }
            data = json!({});
            route
        } else {
            suffix
        };
        let mut upstream = self.client.request(
            method.parse()?,
            format!("{}/portal/api/{}", self.platform, route),
        );
        if !session.cookie.is_empty() {
            upstream = upstream
                .header("Cookie", session.cookie.as_str())
                .header("X-CSRF-Token", session.csrf.as_str());
        }
        if !operation.is_empty() {
            upstream = upstream.header("Idempotency-Key", operation);
        }
        if method != "GET" {
            upstream = upstream.json(&data);
        }
        let mut reply = upstream.send().await?;
        let status = reply.status().as_u16();
        let cookie = reply
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = reply.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 16 * 1024 * 1024,
                "account_response_too_large"
            );
            bytes.extend_from_slice(&chunk);
        }
        if image && status == 200 {
            return Ok(response(status, bytes, "image/svg+xml"));
        }
        let mut value: Value = serde_json::from_slice(&bytes)?;
        if status < 300 {
            if name == "login" || name == "register" {
                if value.pointer("/account/role").and_then(Value::as_str) != Some("user") {
                    *session = Session::default();
                    return Ok(denied(403, "请使用用户账号登录，商户请使用 teemerchant"));
                }
                ensure!(
                    cookie.starts_with("teerouter_portal=") && cookie.len() < 256,
                    "invalid_platform_session"
                );
                session.cookie = Zeroizing::new(cookie);
                session.csrf = Zeroizing::new(value["csrf"].as_str().unwrap_or("").to_owned());
            }
            if name == "logout" || name == "password" {
                *session = Session::default();
            }
            if name == "use-key" {
                let key = Zeroizing::new(value["key"].as_str().unwrap_or("").to_owned());
                ensure!(!key.is_empty(), "invalid_key");
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.sender
                    .send(Event::AccountKey(key, tx))
                    .map_err(|_| anyhow::anyhow!("window_closed"))?;
                return Ok(
                    match tokio::time::timeout(Duration::from_secs(10), rx).await {
                        Ok(Ok(Ok(()))) => response(200, "{\"saved\":true}", "application/json"),
                        Ok(Ok(Err(message))) => denied(409, &message),
                        _ => denied(503, "主窗口未响应，请重试"),
                    },
                );
            }
            if let Some(object) = value.as_object_mut() {
                object.remove("csrf");
            }
        } else if status == 401 {
            *session = Session::default();
        }
        Ok(response(
            status,
            serde_json::to_vec(&value)?,
            "application/json; charset=utf-8",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn loopback_ui_rejects_cross_origin_and_unauthenticated_api() {
        let (sender, _receiver) = mpsc::channel();
        let (url, task) = start("https://example.invalid", sender).await.unwrap();
        let origin = url.trim_end_matches("/account");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let page = client.get(&url).send().await.unwrap();
        assert_eq!(page.status(), 200);
        let cookie = page.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let html = page.text().await.unwrap();
        assert!(!html.contains("LOCAL_CSRF"));
        assert_eq!(
            client
                .get(&url)
                .header("Host", "evil.example")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(&url)
                .header("Origin", "https://evil.example")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{origin}/account-api/me"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            client
                .post(format!("{origin}/account-api/payments"))
                .header("Cookie", &cookie)
                .json(&json!({"amount":"1"}))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{origin}/account-assets/common.js"))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .get(format!("{origin}/account-api/admin/users"))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        task.abort();
    }
    #[test]
    fn restricted_routes() {
        assert!(allowed("POST", "payments"));
        assert!(allowed("GET", "receipts/records/123"));
        for path in [
            "../api/users",
            "withdrawals",
            "merchant/credentials/reveal",
            "keys/../admin",
            "payments/a/b/qr.svg",
        ] {
            assert!(!allowed("POST", path));
        }
        assert!(platform("http://127.0.0.1").is_err());
        assert!(platform("https://user:secret@example.com").is_err());
        assert_eq!(
            platform("https://teerouter.org/").unwrap(),
            "https://teerouter.org"
        );
    }
}
