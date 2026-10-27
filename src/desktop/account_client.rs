//! Native account API transport. Local Agent credentials never enter this module.
use anyhow::{Result, ensure};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Default)]
struct Session {
    cookie: Zeroizing<String>,
    csrf: Zeroizing<String>,
}
pub(super) struct AccountClient {
    base: String,
    http: Client,
    session: Mutex<Session>,
}
pub(super) struct Failure {
    pub status: u16,
    pub message: String,
}
impl Failure {
    fn network() -> Self {
        Self {
            status: 0,
            message: "平台连接失败，请重试；已发送的操作不会自动重发。".into(),
        }
    }
}
pub(super) fn platform(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value.trim())?;
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
pub(super) fn operation_id() -> Result<String> {
    let mut bytes = [0; 24];
    getrandom::getrandom(&mut bytes).map_err(|_| anyhow::anyhow!("无法生成操作编号"))?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}
fn allowed(method: &str, route: &str) -> bool {
    let path = route.split('?').next().unwrap_or("");
    if route.len() > 2048 || route.contains('#') || route.contains('\\') {
        return false;
    }
    let parts: Vec<_> = path.split('/').collect();
    let id = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            && s != "."
            && s != ".."
    };
    match (method, parts.as_slice()) {
        ("GET", ["info" | "me" | "finance" | "payments" | "pricing" | "inventory"]) => true,
        ("GET", ["receipts", "mine"]) => true,
        ("GET", ["receipts", "records", seq]) => {
            !seq.is_empty() && seq.bytes().all(|c| c.is_ascii_digit())
        }
        ("POST", ["login" | "register" | "logout" | "password" | "connection" | "payments"]) => {
            !route.contains('?')
        }
        ("GET", ["payments", key]) | ("POST", ["payments", key, "refresh"]) => {
            id(key) && !route.contains('?')
        }
        _ => false,
    }
}
pub(super) fn error_message(code: &str, status: u16) -> String {
    let message = match code {
        "invalid_credentials" => "用户名或密码错误",
        "connection_revoked" => "此设备的连接权限已被停用，请联系管理员",
        "user_disabled" => "账户已停用",
        "pricing_snapshot_pending" => "管理平台正在更新模型价格，请稍后重试",
        "username_unavailable" => "用户名已被注册",
        "registration_disabled" => "平台暂未开放注册",
        "invalid_password" | "password_incorrect" => "当前密码错误",
        "merchant_required" | "user_required" => "请使用用户账号，商户请使用 teemerchant",
        "insufficient_balance" => "余额不足",
        "payment_disabled" | "payments_disabled" => "充值通道尚未启用",
        "payment_amount_out_of_range" => "充值金额超出允许范围",
        "payment_order_unconfirmed" => "订单结果尚未确认，可用相同金额和网络重试查询",
        "rate_limit_exceeded" | "rate_limited" => "操作过于频繁，请稍后重试",
        _ => match status {
            401 => "登录已失效，请重新登录",
            403 => "当前账号没有此操作权限",
            409 => "操作状态已变化，请刷新后重试",
            422 | 400 => "输入不符合要求，请检查后重试",
            429 => "操作过于频繁，请稍后重试",
            _ => "平台请求失败，请稍后重试",
        },
    };
    message.into()
}
impl AccountClient {
    pub fn restore(base: &str, value: &Value) -> Result<Self> {
        let mut client = Self::new(base)?;
        let cookie = value["cookie"].as_str().unwrap_or("");
        let csrf = value["csrf"].as_str().unwrap_or("");
        ensure!(
            cookie.starts_with("teerouter_portal=")
                && cookie.len() < 256
                && !csrf.is_empty()
                && csrf.len() < 256,
            "登录信息无效"
        );
        client.session = Mutex::new(Session {
            cookie: Zeroizing::new(cookie.into()),
            csrf: Zeroizing::new(csrf.into()),
        });
        Ok(client)
    }
    pub async fn saved_session(&self) -> Zeroizing<Vec<u8>> {
        let session = self.session.lock().await;
        Zeroizing::new(
            serde_json::to_vec(
                &json!({"cookie":session.cookie.as_str(),"csrf":session.csrf.as_str()}),
            )
            .unwrap_or_default(),
        )
    }
    #[cfg(test)]
    pub(super) fn test_client(base: String) -> Self {
        let mut client = Self::new("https://example.invalid").unwrap();
        client.base = base;
        client
    }
    pub fn new(base: &str) -> Result<Self> {
        Ok(Self {
            base: platform(base)?,
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
            session: Mutex::new(Session::default()),
        })
    }
    pub async fn request(
        &self,
        method: &str,
        route: &str,
        body: Value,
        operation: &str,
    ) -> std::result::Result<Value, Failure> {
        if !allowed(method, route) || (route == "register" && body["role"] != "user") {
            return Err(Failure {
                status: 403,
                message: "不支持此账户操作".into(),
            });
        }
        let mut session = self.session.lock().await;
        let authenticating = route == "login" || route == "register";
        if authenticating {
            *session = Session::default();
        }
        let mut request = self.http.request(
            Method::from_bytes(method.as_bytes()).map_err(|_| Failure::network())?,
            format!("{}/portal/api/{route}", self.base),
        );
        if !session.cookie.is_empty() {
            request = request
                .header("Cookie", session.cookie.as_str())
                .header("X-CSRF-Token", session.csrf.as_str());
        }
        if !operation.is_empty() {
            request = request.header("Idempotency-Key", operation);
        }
        // Serialize secrets into a zeroizing buffer rather than retaining a JSON value in the task.
        let encoded = Zeroizing::new(serde_json::to_vec(&body).map_err(|_| Failure::network())?);
        let mut body = body;
        for key in ["password", "current_password", "new_password"] {
            if let Some(Value::String(secret)) = body.get_mut(key) {
                zeroize::Zeroize::zeroize(secret);
            }
        }
        drop(body);
        if method != "GET" {
            request = request
                .header("Content-Type", "application/json")
                .body(encoded.to_vec());
        }
        let result = async {
            let mut response = request.send().await.map_err(|_| Failure::network())?;
            let status = response.status().as_u16();
            let cookie = response
                .headers()
                .get("set-cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .to_owned();
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(chunk) = response.chunk().await.map_err(|_| Failure::network())? {
                if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                    return Err(Failure::network());
                }
                bytes.extend_from_slice(&chunk);
            }
            if status == 401 {
                *session = Session::default();
            }
            let mut value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Failure::network())?;
            if !(200..300).contains(&status) {
                let code = value["error"]
                    .as_str()
                    .or_else(|| value["error"]["code"].as_str())
                    .unwrap_or("");
                return Err(Failure {
                    status,
                    message: error_message(code, status),
                });
            }
            if authenticating {
                if value.pointer("/account/role").and_then(Value::as_str) != Some("user") {
                    return Err(Failure {
                        status: 401,
                        message: "请使用用户账号登录，商户请使用 teemerchant".into(),
                    });
                }
                let csrf = value["csrf"].as_str().unwrap_or("");
                if !cookie.starts_with("teerouter_portal=")
                    || cookie.len() >= 256
                    || csrf.is_empty()
                {
                    return Err(Failure::network());
                }
                session.cookie = Zeroizing::new(cookie);
                session.csrf = Zeroizing::new(csrf.to_owned());
            }
            if route == "logout" || route == "password" {
                *session = Session::default();
            }
            if let Some(object) = value.as_object_mut() {
                object.remove("csrf");
            }
            Ok(value)
        }
        .await;
        // Local sign-out always ends the local session, even if the remote endpoint is unreachable.
        if route == "logout" {
            *session = Session::default();
        }
        result
    }
    pub async fn load(&self, route: &str) -> std::result::Result<Value, Failure> {
        let me = self.request("GET", "me", json!({}), "").await?;
        if me.pointer("/account/role").and_then(Value::as_str) != Some("user") {
            *self.session.lock().await = Session::default();
            return Err(Failure {
                status: 401,
                message: "请重新使用用户账号登录".into(),
            });
        }
        let view = if route == "me" {
            Value::Null
        } else {
            self.request("GET", route, json!({}), "").await?
        };
        Ok(json!({"me":me,"view":view}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_routes_and_platform_are_restricted() {
        for route in [
            "../api/users",
            "keys/../reveal",
            "merchant/credentials/reveal",
            "https://evil/me",
            "payments/a/b/refresh",
        ] {
            assert!(!allowed("POST", route));
        }
        assert!(allowed("GET", "receipts/mine?before=12"));
        assert!(!allowed("POST", "keys/k_123/reveal"));
        assert!(allowed("POST", "connection"));
        assert!(platform("http://127.0.0.1").is_err());
        assert!(platform("https://user:secret@example.com").is_err());
    }
    #[tokio::test]
    async fn sessions_are_scoped_and_cleared_on_expiry_or_wrong_role() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::{get, post},
        };
        let mock = Router::new()
            .route("/portal/api/login", post(|Json(data): Json<Value>| async move {
                ([("set-cookie", "teerouter_portal=test-session; HttpOnly")], Json(json!({
                    "account":{"role":if data["username"] == "merchant" {"merchant"} else {"user"}},"csrf":"csrf-test"})))
            }))
            .route("/portal/api/me", get(|headers: HeaderMap| async move {
                if headers.get("cookie").is_some() && headers.get("x-csrf-token").is_some() {
                    (StatusCode::OK, Json(json!({"account":{"role":"user"}})))
                } else { (StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized"}))) }
            }))
            .route("/portal/api/finance", get(|| async { (StatusCode::UNAUTHORIZED, Json(json!({"error":"expired"}))) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async {
            axum::serve(listener, mock).await.unwrap();
        });
        let mut client = AccountClient::new("https://example.invalid").unwrap();
        client.base = base;
        assert!(
            client
                .request(
                    "POST",
                    "login",
                    json!({"username":"user","password":"example"}),
                    ""
                )
                .await
                .is_ok()
        );
        assert!(client.request("GET", "me", json!({}), "").await.is_ok());
        let saved = client.saved_session().await;
        let mut restored = AccountClient::restore(
            "https://example.invalid",
            &serde_json::from_slice(&saved).unwrap(),
        )
        .unwrap();
        restored.base = client.base.clone();
        assert!(restored.request("GET", "me", json!({}), "").await.is_ok());
        assert!(
            client
                .request("GET", "finance", json!({}), "")
                .await
                .is_err()
        );
        assert!(client.session.lock().await.cookie.is_empty());
        assert!(
            client
                .request("POST", "login", json!({"username":"merchant"}), "")
                .await
                .is_err()
        );
        assert!(client.session.lock().await.cookie.is_empty());
        server.abort();
    }
}
