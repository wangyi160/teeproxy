//! Read-only client inventory on the exact attested TLS connection.
use crate::{client, config::Config, protocol::MAX_BODY};
use anyhow::{Context, Result, ensure};
use http_body_util::{BodyExt, Limited};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

#[derive(Default)]
pub(crate) struct Catalog {
    pub models: Vec<Model>,
    pub accounts: Vec<Account>,
    pub pools: usize,
}
pub(crate) struct Model {
    pub id: String,
    pub provider: String,
    pub pools: String,
    pub protocols: String,
}
pub(crate) struct Account {
    pub pool: String,
    pub pool_name: String,
    pub merchant: String,
    pub multiplier: String,
    pub id: String,
    pub provider: String,
    pub state: String,
    pub catalog: String,
    pub models: String,
}
pub(crate) async fn fetch(config: &Config, key: &str) -> Result<Catalog> {
    let trust = config.trust()?;
    fetch_verified(&config.router, &trust, key).await
}
pub(crate) async fn fetch_verified(
    router: &str,
    trust: &client::Trust,
    key: &str,
) -> Result<Catalog> {
    ensure!(!key.is_empty(), "请填写 Router key 后刷新列表");
    tokio::time::timeout(Duration::from_secs(90), async {
        let attempt = client::verify(router, trust).await;
        let mut session = attempt
            .session
            .context("TEE 验证未通过，未发送密钥或读取列表")?;
        let mut values = Vec::new();
        for path in ["/v1/models", "/v1/pools"] {
            let response = session.send("GET", path, key, String::new()).await?;
            ensure!(
                response.status().is_success(),
                "列表读取失败：{path} 返回 HTTP {}",
                response.status()
            );
            let bytes = Limited::new(response.into_body(), MAX_BODY)
                .collect()
                .await
                .map_err(|_| anyhow::anyhow!("目录响应过大或连接已中断"))?
                .to_bytes();
            values.push(serde_json::from_slice(&bytes).context("目录响应不是有效 JSON")?);
        }
        parse(&values[0], &values[1])
    })
    .await
    .context("刷新列表超时")?
}
fn text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}
fn joined(values: impl Iterator<Item = String>) -> String {
    values
        .filter(|s| !s.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" / ")
}
fn provider(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "openai" => "OpenAI".into(),
        "anthropic" | "claude" => "Claude".into(),
        "xai" | "grok" => "Grok".into(),
        "workbuddy" => "Workbuddy（测试）".into(),
        _ => text(value),
    }
}
fn multiplier(value: &Value) -> String {
    let Some(n) = value.as_u64().filter(|n| *n <= 1_000_000_000_000_000) else {
        return "未设置".into();
    };
    let whole = n / 1_000_000;
    let fraction = n % 1_000_000;
    if fraction == 0 {
        format!("{whole}×")
    } else {
        format!(
            "{whole}.{}×",
            format!("{fraction:06}").trim_end_matches('0')
        )
    }
}
pub(crate) fn parse(models: &Value, pools: &Value) -> Result<Catalog> {
    let models = models["data"]
        .as_array()
        .context("模型目录缺少 data 列表")?;
    let pools = pools["data"]
        .as_array()
        .context("账号池目录缺少 data 列表")?;
    ensure!(
        models.len() <= 10000 && pools.len() <= 10000,
        "目录记录过多"
    );
    let mut result = Catalog {
        pools: pools.len(),
        ..Default::default()
    };
    for model in models {
        let id = text(&model["id"]);
        ensure!(!id.is_empty(), "模型目录缺少模型 ID");
        let caps = model["router_capabilities"].as_array();
        result.models.push(Model {
            id,
            provider: joined(caps.into_iter().flatten().map(|v| provider(&v["provider"]))),
            pools: joined(
                model["router_pools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text),
            ),
            protocols: joined(
                caps.into_iter()
                    .flatten()
                    .flat_map(|v| v["protocols"].as_array().into_iter().flatten())
                    .map(text),
            ),
        });
    }
    for pool in pools {
        let name = text(&pool["id"]);
        let pool_name = match text(&pool["pool_name"]) {
            s if s.is_empty() => name.clone(),
            s => s,
        };
        let merchant = text(&pool["merchant_name"]);
        let rate = multiplier(&pool["multiplier_millionths"]);
        ensure!(!name.is_empty(), "账号池目录缺少 ID");
        let accounts = pool["accounts"]
            .as_array()
            .context("账号池缺少 accounts 列表")?;
        ensure!(
            result.accounts.len() + accounts.len().max(1) <= 10000,
            "账号记录过多"
        );
        if accounts.is_empty() {
            result.accounts.push(Account {
                pool: name.clone(),
                pool_name: pool_name.clone(),
                merchant: merchant.clone(),
                multiplier: rate.clone(),
                id: "（空池）".into(),
                provider: provider(&pool["provider"]),
                state: "无账号".into(),
                catalog: "—".into(),
                models: "0".into(),
            });
        }
        for account in accounts {
            ensure!(result.accounts.len() < 10000, "账号记录过多");
            let id = text(&account["id"]);
            ensure!(!id.is_empty(), "账号目录缺少 ID");
            let state = if account["enabled"] == false {
                "已停用"
            } else if account["available"] == true {
                "可用"
            } else if account["available"] == false {
                "暂不可用"
            } else {
                "未知"
            };
            let catalog = match account["catalog"]["state"].as_str() {
                Some("fetched") => "已获取",
                Some("refreshing") => "刷新中",
                Some("not_fetched") => "尚未获取",
                Some("error") => "获取失败",
                Some("disabled") => "已停用",
                _ => "未知",
            };
            result.accounts.push(Account {
                pool: name.clone(),
                pool_name: pool_name.clone(),
                merchant: merchant.clone(),
                multiplier: rate.clone(),
                id,
                provider: provider(&account["provider"]),
                state: state.into(),
                catalog: catalog.into(),
                models: account["catalog"]["visible_models"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or("—".into()),
            });
        }
    }
    result.models.sort_by(|a, b| a.id.cmp(&b.id));
    result
        .accounts
        .sort_by(|a, b| (&a.pool, &a.id).cmp(&(&b.pool, &b.id)));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pool_multiplier_is_exact_and_missing_is_not_assumed_to_be_one() {
        for (value, expected) in [
            (json!(20000), "0.02×"),
            (json!(200000), "0.2×"),
            (json!(1000000), "1×"),
            (json!(1), "0.000001×"),
            (json!(0), "0×"),
            (json!(1000000000000000u64), "1000000000×"),
            (Value::Null, "未设置"),
            (json!(-1), "未设置"),
            (json!(0.2), "未设置"),
        ] {
            assert_eq!(multiplier(&value), expected);
        }
        let catalog = parse(&json!({"data":[]}), &json!({"data":[{"id":"m_id.pool","pool_name":"pool","merchant_name":"Shop","multiplier_millionths":200000,"accounts":[]}]})).unwrap();
        let row = &catalog.accounts[0];
        assert_eq!(row.pool, "m_id.pool");
        assert_eq!(row.pool_name, "pool");
        assert_eq!(row.merchant, "Shop");
        assert_eq!(row.multiplier, "0.2×");
    }
    #[test]
    fn inventory_uses_public_fields_and_preserves_unavailable_accounts_and_empty_pools() {
        let catalog = parse(&json!({"data":[{"id":"model-a","router_pools":["p"],"router_capabilities":[{"provider":"openai","protocols":["responses"]}]}]}),
            &json!({"data":[{"id":"p","accounts":[{"id":"account-a","provider":"openai","enabled":true,"available":false,"catalog":{"state":"error","visible_models":0},"credential":"NEVER_DISPLAY"}]},{"id":"empty","accounts":[]}]})).unwrap();
        assert_eq!(catalog.models[0].id, "model-a");
        assert_eq!(catalog.models[0].protocols, "responses");
        assert_eq!(catalog.accounts[1].state, "暂不可用");
        assert_eq!(catalog.accounts[1].catalog, "获取失败");
        assert_eq!(catalog.accounts[0].state, "无账号");
        assert_eq!(catalog.pools, 2);
        assert!(parse(&json!({"data":[]}), &json!({"error":"bad"})).is_err());
    }
}
