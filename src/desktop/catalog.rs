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
    pub pools: Vec<Pool>,
}
pub(crate) struct Pool {
    pub id: String,
    pub name: String,
    pub merchant: String,
    pub merchant_id: String,
    pub provider: String,
    pub multiplier: String,
    pub state: String,
    pub label: String,
    pub account_count: usize,
}
pub(crate) struct Model {
    pub id: String,
    pub provider: String,
    pub pool: String,
    pub protocols: String,
}
pub(crate) struct Account {
    pub pool: String,
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
    let mut result = Catalog::default();
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
            result.accounts.len() + accounts.len() <= 10000,
            "账号记录过多"
        );
        ensure!(!result.pools.iter().any(|p| p.id == name), "账号池 ID 重复");
        let state = if accounts.is_empty() {
            "无账号"
        } else if accounts
            .iter()
            .any(|a| a["enabled"] != false && a["available"] == true)
        {
            "可用"
        } else if accounts.iter().all(|a| a["enabled"] == false) {
            "已停用"
        } else if accounts.iter().any(|a| a["catalog"]["state"] == "error") {
            "目录获取失败"
        } else if accounts
            .iter()
            .any(|a| a["catalog"]["state"] == "refreshing")
        {
            "目录刷新中"
        } else {
            "暂不可用"
        };
        let label = [pool_name.as_str(), merchant.as_str(), rate.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        result.pools.push(Pool {
            id: name.clone(),
            name: pool_name.clone(),
            merchant: merchant.clone(),
            merchant_id: text(&pool["merchant_id"]),
            provider: provider(&pool["provider"]),
            multiplier: rate.clone(),
            state: state.into(),
            label,
            account_count: accounts.len(),
        });
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
    // A row is a (pool, model) pair. Never merge prices or capabilities across pools.
    let mut seen = BTreeSet::new();
    for model in models {
        let wire_id = text(&model["id"]);
        ensure!(!wire_id.is_empty(), "模型目录缺少模型 ID");
        let memberships: BTreeSet<_> = model["router_pools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for membership in &memberships {
            // The pool endpoint is authoritative for the pools this key may see.
            let Some(pool) = result.pools.iter().find(|p| p.id == *membership) else {
                continue;
            };
            let merchant = model["router_merchant"]["id"].as_str().unwrap_or("");
            ensure!(
                pool.merchant_id.is_empty() || merchant == pool.merchant_id,
                "模型与账号池的商户不匹配，请刷新列表"
            );
            let id = if merchant.is_empty() {
                wire_id.clone()
            } else {
                wire_id
                    .strip_prefix(&format!("{merchant}/"))
                    .filter(|s| !s.is_empty())
                    .context("模型 ID 与商户不匹配")?
                    .to_owned()
            };
            if !seen.insert((pool.id.clone(), id.clone())) {
                continue;
            }
            ensure!(result.models.len() < 10000, "账号池模型记录过多");
            let caps: Vec<_> = model["router_capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| {
                    c["pool"].as_str() == Some(membership)
                        || (memberships.len() == 1 && c["pool"].is_null())
                })
                .collect();
            let source = if pool.provider.is_empty() {
                joined(caps.iter().map(|c| provider(&c["provider"])))
            } else {
                pool.provider.clone()
            };
            let protocols = joined(
                caps.iter()
                    .flat_map(|c| c["protocols"].as_array().into_iter().flatten())
                    .map(text),
            );
            result.models.push(Model {
                id,
                pool: pool.id.clone(),
                provider: source,
                protocols,
            });
        }
    }
    result
        .pools
        .sort_by(|a, b| (&a.name, &a.merchant, &a.id).cmp(&(&b.name, &b.merchant, &b.id)));
    let mut labels = std::collections::BTreeMap::<String, usize>::new();
    for pool in &result.pools {
        *labels.entry(pool.label.clone()).or_default() += 1;
    }
    for pool in &mut result.pools {
        if labels[&pool.label] > 1 {
            pool.label = format!("{} · {}", pool.label, pool.id);
        }
    }
    result
        .models
        .sort_by(|a, b| (&a.pool, &a.id).cmp(&(&b.pool, &b.id)));
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
    fn models_are_scoped_to_authorized_pools_with_distinct_merchants_and_capabilities() {
        let pools = json!({"data":[
            {"id":"m_a.cheap","pool_name":"cheap","merchant_id":"m_a","merchant_name":"Shop","provider":"openai","multiplier_millionths":200000,"accounts":[]},
            {"id":"m_a.premium","pool_name":"premium","merchant_id":"m_a","merchant_name":"Shop","provider":"openai","multiplier_millionths":1000000,"accounts":[]},
            {"id":"m_b.cheap","pool_name":"cheap","merchant_id":"m_b","merchant_name":"Shop","provider":"openai","multiplier_millionths":200000,"accounts":[]}
        ]});
        let models = json!({"data":[
            {"id":"m_a/vendor/model","router_merchant":{"id":"m_a"},"router_pools":["m_a.cheap","m_a.premium","m_a.hidden"],"router_capabilities":[
                {"pool":"m_a.cheap","protocols":["responses"]},
                {"pool":"m_a.premium","protocols":["chat_completions"]},
                {"pool":"m_a.hidden","protocols":["never-expose"]}]},
            {"id":"m_b/vendor/model","router_merchant":{"id":"m_b"},"router_pools":["m_b.cheap"]}
        ]});
        let catalog = parse(&models, &pools).unwrap();
        assert_eq!(catalog.models.len(), 3);
        assert!(catalog.models.iter().all(|m| m.id == "vendor/model"));
        assert_eq!(catalog.models[0].pool, "m_a.cheap");
        assert_eq!(catalog.models[0].protocols, "responses");
        assert_eq!(catalog.models[1].protocols, "chat_completions");
        assert!(catalog.models[2].protocols.is_empty());
        assert_eq!(catalog.pools[0].multiplier, "0.2×");
        assert_ne!(catalog.pools[0].label, catalog.pools[1].label);
        let mut wrong = models.clone();
        wrong["data"][0]["router_merchant"]["id"] = json!("m_b");
        assert!(parse(&wrong, &pools).is_err());
        let mut duplicate = models.clone();
        duplicate["data"]
            .as_array_mut()
            .unwrap()
            .push(models["data"][0].clone());
        assert_eq!(parse(&duplicate, &pools).unwrap().models.len(), 3);
    }
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
        let row = &catalog.pools[0];
        assert_eq!(row.id, "m_id.pool");
        assert_eq!(row.name, "pool");
        assert_eq!(row.merchant, "Shop");
        assert_eq!(row.multiplier, "0.2×");
        assert_eq!(row.account_count, 0);
        assert!(catalog.accounts.is_empty());
    }
    #[test]
    fn inventory_uses_public_fields_and_preserves_unavailable_accounts_and_empty_pools() {
        let catalog = parse(&json!({"data":[{"id":"model-a","router_pools":["p"],"router_capabilities":[{"provider":"openai","protocols":["responses"]}]}]}),
            &json!({"data":[{"id":"p","accounts":[{"id":"account-a","provider":"openai","enabled":true,"available":false,"catalog":{"state":"error","visible_models":0},"credential":"NEVER_DISPLAY"}]},{"id":"empty","accounts":[]}]})).unwrap();
        assert_eq!(catalog.models[0].id, "model-a");
        assert_eq!(catalog.models[0].protocols, "responses");
        assert_eq!(catalog.accounts.len(), 1);
        assert_eq!(catalog.accounts[0].state, "暂不可用");
        assert_eq!(catalog.accounts[0].catalog, "获取失败");
        assert_eq!(catalog.pools[0].state, "无账号");
        assert_eq!(catalog.pools[0].account_count, 0);
        assert_eq!(catalog.pools[1].account_count, 1);
        assert_eq!(catalog.pools.len(), 2);
        assert!(parse(&json!({"data":[]}), &json!({"error":"bad"})).is_err());
    }
}
