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
pub(super) fn open_google_page(url: &str) -> std::result::Result<(),Failure> {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW,WindowsAndMessaging::SW_SHOWNORMAL};
    let wide:Vec<u16>=url.encode_utf16().chain(std::iter::once(0)).collect();
    let result=unsafe{ShellExecuteW(std::ptr::null_mut(),windows_sys::core::w!("open"),wide.as_ptr(),std::ptr::null(),std::ptr::null(),SW_SHOWNORMAL)};
    if result as isize<=32 {return Err(Failure{status:0,message:"无法打开系统浏览器，请检查默认浏览器设置".into()});}
    Ok(())
}
fn google_url_allowed(base: &str, url: &str) -> bool {
    let (Ok(expected), Ok(parsed)) = (reqwest::Url::parse(base), reqwest::Url::parse(url)) else { return false; };
    parsed.origin() == expected.origin() && parsed.path() == "/portal/oauth/google/start"
        && parsed.fragment().is_none() && parsed.username().is_empty() && parsed.password().is_none()
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
        ("GET", ["info" | "me" | "finance" | "payments" | "withdrawals" | "pricing" | "inventory"]) => true,
        ("POST", ["withdrawals"]) => !route.contains('?'),
        ("POST", ["withdrawals", key, "cancel"]) => !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) && !route.contains('?'),
        ("GET", ["receipts", "mine"]) => true,
        ("POST", ["identity", "challenge" | "bind"]) => !route.contains('?'),
        ("POST", ["identity", "google", "start" | "status"]) => !route.contains('?'),
        ("GET", ["receipts", "records", seq]) => {
            !seq.is_empty() && seq.bytes().all(|c| c.is_ascii_digit())
        }
        ("POST", ["login" | "register" | "logout" | "connection" | "payments"]) => {
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
        "identity_not_registered" => "此本地 ID 尚未注册",
        "google_verification_required" => "新用户请通过 Google 登录完成注册",
        "google_login_unavailable" => "Google 登录暂不可用，请稍后重试",
        "google_account_already_registered" => "此 Google 账号已注册，请选择原有本地 ID 登录",
        "google_authorization_denied" => "已取消 Google 授权",
        "google_flow_invalid" | "google_state_invalid" | "google_flow_used" => "Google 登录已过期，请重新注册",
        "google_token_exchange_failed" | "google_identity_invalid" => "Google 验证失败，请重新注册",
        "google_email_unverified" => "请先完成 Google 账号的邮箱验证",
        "invalid_password" | "password_incorrect" => "当前密码错误",
        "merchant_required" | "user_required" => "请使用用户账号，商户请使用 teemerchant",
        "insufficient_balance" => "余额不足",
        "insufficient_withdrawable_balance" => "可提现余额不足，赠送额度只可消费",
        "below_withdrawal_minimum" => "金额低于最低提现门槛",
        "withdrawal_policy_changed" => "提现规则已更新，请重新进入提现页确认手续费",
        "withdrawal_state_conflict" => "申请状态已变化，请重新读取记录",
        "withdrawal_not_found" => "未找到此提现申请",
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
    pub async fn google_register(&self, signer: &crate::user_auth::Signer, open: impl Fn(&str) -> std::result::Result<(),Failure>) -> std::result::Result<Value,Failure> {
        // A lost registration response must not send the same identity through OAuth again.
        match self.identity_login(signer,"login").await {
            Ok(value)=>return Ok(value),
            Err(error) if error.status==404=>{},
            Err(error)=>return Err(error),
        }
        let challenge=self.request("POST","identity/challenge",json!({"role":"user","public_key":signer.public_key(),"purpose":"register"}),"").await?;
        let proof=signer.identity_proof(&challenge["challenge"],&self.base,"register").map_err(|_|Failure{status:401,message:"身份验证挑战无效".into()})?;
        let flow=self.request("POST","identity/google/start",proof,"").await?;
        let url=flow["authorization_url"].as_str().ok_or_else(Failure::network)?;
        if !google_url_allowed(&self.base,url) {
            return Err(Failure{status:401,message:"Google 登录地址不符合平台配置".into()});
        }
        let flow_id=flow["flow_id"].as_str().ok_or_else(Failure::network)?.to_owned();
        let poll=Zeroizing::new(flow["poll_token"].as_str().ok_or_else(Failure::network)?.to_owned());
        if poll.len()!=64 || !poll.bytes().all(|b|b.is_ascii_hexdigit()) || !flow_id.starts_with("gf_") || flow_id.len()!=35 {return Err(Failure::network());}
        open(url)?;
        let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(600);
        loop {
            if tokio::time::Instant::now()>=deadline {return Err(Failure{status:408,message:"Google 登录超时，请重试".into()});}
            let status=self.request("POST","identity/google/status",json!({"flow_id":flow_id,"poll_token":poll.as_str()}),"").await?;
            match status["state"].as_str() {
                Some("verified")=>break,
                Some("failed")=>return Err(Failure{status:403,message:error_message(status["error"].as_str().unwrap_or("google_identity_invalid"),403)}),
                Some("pending")=>tokio::time::sleep(std::time::Duration::from_secs(2)).await,
                _=>return Err(Failure{status:401,message:"Google 登录状态已失效，请重试".into()}),
            }
        }
        let challenge=self.request("POST","identity/challenge",json!({"role":"user","public_key":signer.public_key(),"purpose":"register"}),"").await?;
        let mut proof=signer.identity_proof(&challenge["challenge"],&self.base,"register").map_err(|_|Failure{status:401,message:"身份验证挑战无效".into()})?;
        proof["google_flow_id"]=json!(flow_id);proof["google_poll_token"]=json!(poll.as_str());
        let value=self.request("POST","register",proof,"").await?;
        self.check_identity(signer,value).await
    }
    async fn check_identity(&self,signer:&crate::user_auth::Signer,value:Value)->std::result::Result<Value,Failure>{
        if value["account"]["identity_id"]!=signer.user_id() || value["account"]["public_key"]!=signer.public_key() {
            *self.session.lock().await=Session::default();
            return Err(Failure{status:401,message:"平台返回的身份与本机密钥不匹配".into()});
        }
        Ok(value)
    }
    pub async fn identity_login(&self, signer: &crate::user_auth::Signer, purpose: &str) -> std::result::Result<Value,Failure> {
        let value=self.request("POST","identity/challenge",json!({"role":"user","public_key":signer.public_key(),"purpose":purpose}),"").await?;
        let proof=signer.identity_proof(&value["challenge"],&self.base,purpose).map_err(|_|Failure{status:401,message:"身份验证挑战无效".into()})?;
        let response=self.request("POST",if purpose=="bind"{"identity/bind"}else{purpose},proof,"").await;
        let value=response?;
        self.check_identity(signer,value).await
    }
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
        let authenticating = route == "login" || route == "register" || route=="identity/bind";
        if authenticating && route!="identity/bind" {
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
        for key in ["password", "current_password", "new_password", "poll_token", "google_poll_token"] {
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
    fn google_browser_handoff_cannot_change_platform_or_open_other_pages() {
        let base="https://teerouter.org";
        assert!(google_url_allowed(base,"https://teerouter.org/portal/oauth/google/start?state=123"));
        for url in ["http://teerouter.org/portal/oauth/google/start", "https://evil.example/portal/oauth/google/start",
            "https://teerouter.org:444/portal/oauth/google/start", "https://teerouter.org/admin", "https://user@teerouter.org/portal/oauth/google/start",
            "https://teerouter.org/portal/oauth/google/start#fragment", "javascript:alert(1)"] {
            assert!(!google_url_allowed(base,url),"{url}");
        }
    }
    #[tokio::test]
    async fn google_registration_uses_key_bound_ticket_and_keeps_existing_key_login() {
        use axum::{Json,Router,http::{HeaderMap,StatusCode},routing::post};
        use std::sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}};
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let signer=crate::user_auth::fixture();
        let account=json!({"identity_id":signer.user_id(),"public_key":signer.public_key(),"role":"user"});
        let registered=Arc::new(AtomicBool::new(false));
        let registered_login=registered.clone();let registered_save=registered.clone();
        let login_account=account.clone();let public=signer.public_key();let origin=base.clone();let auth_origin=base.clone();
        let mock=Router::new()
            .route("/portal/api/identity/challenge",post(move|Json(v):Json<Value>|{let origin=origin.clone();let public=public.clone();async move{
                assert_eq!(v["public_key"],public);
                Json(json!({"challenge":{"origin":origin,"purpose":v["purpose"],"role":"user","public_key":public,
                    "id":"challenge","nonce":"nonce","expires":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()+300}}))
            }}))
            .route("/portal/api/login",post(move||{let registered=registered_login.clone();let account=login_account.clone();async move{
                if registered.load(Ordering::SeqCst){(StatusCode::OK,[("set-cookie","teerouter_portal=login")],Json(json!({"account":account,"csrf":"csrf"})))}
                else{(StatusCode::NOT_FOUND,[("set-cookie","")],Json(json!({"error":"identity_not_registered"})))}
            }}))
            .route("/portal/api/identity/google/start",post(move|Json(v):Json<Value>|{let origin=auth_origin.clone();async move{
                assert_eq!(v["signature"].as_str().unwrap().len(),128);
                Json(json!({"flow_id":format!("gf_{}","a".repeat(32)),"poll_token":"b".repeat(64),"authorization_url":origin+"/portal/oauth/google/start?state=test"}))
            }}))
            .route("/portal/api/identity/google/status",post(|Json(v):Json<Value>|async move{
                assert_eq!(v["poll_token"],"b".repeat(64));Json(json!({"state":"verified"}))
            }))
            .route("/portal/api/register",post(move|headers:HeaderMap,Json(v):Json<Value>|{let registered=registered_save.clone();let account=account.clone();async move{
                assert!(headers.get("cookie").is_none());assert_eq!(v["google_flow_id"],format!("gf_{}","a".repeat(32)));
                assert_eq!(v["google_poll_token"],"b".repeat(64));assert_eq!(v["role"],"user");
                assert_eq!(v["signature"].as_str().unwrap().len(),128);registered.store(true,Ordering::SeqCst);
                ([("set-cookie","teerouter_portal=registered; HttpOnly")],Json(json!({"account":account,"csrf":"csrf"})))
            }}));
        let server=tokio::spawn(async{axum::serve(listener,mock).await.unwrap()});
        let client=AccountClient::test_client(base.clone());let opened=AtomicUsize::new(0);
        let open=|url:&str|{assert!(url.starts_with(&(base.clone()+"/portal/oauth/google/start?")));opened.fetch_add(1,Ordering::SeqCst);Ok(())};
        assert!(client.google_register(&signer,&open).await.is_ok());
        assert_eq!(client.session.lock().await.cookie.as_str(),"teerouter_portal=registered");
        assert!(client.google_register(&signer,&open).await.is_ok());
        assert_eq!(opened.load(Ordering::SeqCst),1);
        server.abort();
    }
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
