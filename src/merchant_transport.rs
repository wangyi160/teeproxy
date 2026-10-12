//! Merchant-owned private pipe. Reuses the user's exact attestation and TLS verifier.
use crate::client::{self, Trust, VerifiedSession};
use anyhow::{Result, ensure};
use http_body_util::{BodyExt, Limited};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub fn allowed(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        (
            "GET",
            "/merchant/state" | "/merchant/usage" | "/merchant/refusals"
        ) | (
            "POST",
            "/merchant/unlock" | "/merchant/models/refresh" | "/merchant/usage/ack"
        ) | ("PUT", "/merchant/config")
            | ("DELETE", "/merchant/refusals")
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    method: String,
    path: String,
    token: String,
    #[serde(default)]
    body: Value,
}
fn output(value: Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &value)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}
pub async fn run(endpoint: &str, trust: &Trust) -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut session: Option<VerifiedSession> = None;
    output(json!({"ready":true,"scope":"merchant"}))?;
    loop {
        let mut line = zeroize::Zeroizing::new(Vec::new());
        loop {
            let bytes = input.fill_buf()?;
            if bytes.is_empty() {
                ensure!(line.is_empty(), "Truncated private input");
                return Ok(());
            }
            let n = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |i| i + 1);
            ensure!(
                line.len() + n <= 20 * 1024 * 1024,
                "Merchant input too large"
            );
            line.extend_from_slice(&bytes[..n]);
            input.consume(n);
            if line.ends_with(b"\n") {
                break;
            }
        }
        let request: Input = match serde_json::from_slice(&line) {
            Ok(r) => r,
            Err(_) => {
                output(json!({"error":"invalid_merchant_input"}))?;
                continue;
            }
        };
        let token = zeroize::Zeroizing::new(request.token);
        let verify_only = request.method == "VERIFY" && request.path.is_empty();
        let path = request.path.split('?').next().unwrap_or("");
        if !verify_only && !allowed(&request.method, path) {
            output(json!({"error":"merchant_endpoint_forbidden"}))?;
            continue;
        }
        if session.as_ref().is_none_or(|s| !s.is_current()) {
            let verified = client::verify(endpoint, trust).await;
            session = verified.session;
            if session.is_none() {
                output(
                    json!({"error":"router_attestation_failed","checks":verified.report.checks}),
                )?;
                continue;
            }
        }
        if verify_only {
            output(json!({"status":200,"body":{"verified":true,"scope":"merchant"}}))?;
            continue;
        }
        let result: Result<Value> = async {
            let response = session
                .as_mut()
                .unwrap()
                .send_merchant(
                    &request.method,
                    &request.path,
                    &token,
                    serde_json::to_vec(&request.body)?,
                )
                .await?;
            let status = response.status().as_u16();
            let body = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                Limited::new(response.into_body(), 32 * 1024 * 1024).collect(),
            )
            .await?
            .map_err(|_| anyhow::anyhow!("Invalid or oversized merchant response"))?
            .to_bytes();
            Ok(json!({"status":status,"body":serde_json::from_slice::<Value>(&body)?}))
        }
        .await;
        match result {
            Ok(value) => output(value)?,
            Err(_) => {
                session = None;
                output(
                    json!({"error":"merchant_request_failed","retry":"inspect_before_retrying_mutations"}),
                )?;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merchant_scope_does_not_grant_platform_or_user_apis() {
        for path in [
            "/admin/checkpoint",
            "/admin/accounts/a/credential",
            "/v1/messages",
            "/merchant/../admin/config",
            "/merchant/accounts/a/credential",
            "https://other.invalid/merchant/state",
        ] {
            assert!(!allowed("GET", path));
            assert!(!allowed("POST", path));
        }
        assert!(allowed("PUT", "/merchant/config"));
        assert!(!allowed("POST", "/merchant/config"));
    }
}
